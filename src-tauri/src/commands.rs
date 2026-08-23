//! The IPC surface.
//!
//! The webview never touches the filesystem and never receives the whole tree.
//! It asks for one node's children at a time and caches them, which keeps a
//! scan of a few hundred thousand files off the IPC boundary entirely.

use crate::archive::{self, ArchiveSummary, Entry, Estimate, LogEntry, LogLevel};
use crate::events;
use crate::fsutil;
use crate::model::arena::{Arena, CheckState, NodeId, NodeKind};
use crate::model::check;
use crate::model::sort::{sort_children, SortBy, SortDir, SortKey};
use crate::plan::{self, ArchivePlan, Compression, OutputOptions, UnresolvedRule, PLAN_VERSION};
use crate::roots::{rebuild, snapshot_checks, Sources};
use crate::scan::{scan_path, ScanIssue};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, State};

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct Tree {
    pub arena: Arena,
    pub root: Option<NodeId>,
    pub sources: Sources,
    pub sort: SortKey,
    pub issues: Vec<ScanIssue>,
    pub output: OutputOptions,
}

impl Tree {
    /// Rebuilds the arena around the current source set. Taking `&mut self`
    /// lets the borrow checker see the arena and source borrows as disjoint.
    fn reroot(&mut self, prior: &crate::roots::CheckSnapshot) {
        self.root = rebuild(&mut self.arena, &self.sources, prior).root;
    }
}

pub struct AppState {
    pub tree: Mutex<Tree>,
    /// Kept apart from the tree lock so a burst of archive errors never
    /// contends with the UI reading rows. Shared as an `Arc` so the archive
    /// thread can append to it directly.
    pub log: Arc<Mutex<Vec<LogEntry>>>,
    pub scan_cancel: Arc<AtomicBool>,
    pub archive_cancel: Arc<AtomicBool>,
    pub archiving: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            tree: Mutex::new(Tree::default()),
            log: Arc::new(Mutex::new(Vec::new())),
            scan_cancel: Arc::new(AtomicBool::new(false)),
            archive_cancel: Arc::new(AtomicBool::new(false)),
            archiving: Arc::new(AtomicBool::new(false)),
        }
    }
}

type Cmd<T> = Result<T, String>;

fn lock(state: &AppState) -> Cmd<std::sync::MutexGuard<'_, Tree>> {
    state.tree.lock().map_err(|_| "internal state was poisoned".to_string())
}

// ---------------------------------------------------------------- DTOs

/// One row. Flat by design: the frontend renders straight from this.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub id: NodeId,
    pub name: String,
    /// `dir` | `file` | `filesGroup` | `syntheticRoot`
    pub kind: &'static str,
    /// A pass-through directory that was never enumerated.
    pub spine: bool,
    pub ext: Option<String>,
    pub has_children: bool,
    pub check: CheckState,
    pub sel_size: u64,
    pub total_size: u64,
    pub sel_files: u64,
    pub total_files: u64,
    pub path: Option<String>,
}

fn view(arena: &Arena, id: NodeId) -> NodeView {
    let n = arena.node(id);
    let (kind, spine) = match n.kind {
        NodeKind::Dir { scanned } => ("dir", !scanned),
        NodeKind::File => ("file", false),
        NodeKind::FilesGroup => ("filesGroup", false),
        NodeKind::SyntheticRoot => ("syntheticRoot", false),
    };
    NodeView {
        id,
        name: n.name.clone(),
        kind,
        spine,
        ext: n.ext.clone(),
        has_children: !n.children.is_empty(),
        check: n.check,
        sel_size: n.sel_size,
        total_size: n.total_size,
        sel_files: n.sel_files,
        total_files: n.total_files,
        path: n.path.as_deref().map(fsutil::display_path),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub sel_files: u64,
    pub sel_bytes: u64,
    pub total_files: u64,
    pub total_bytes: u64,
    pub sources: usize,
    pub issues: usize,
}

fn summary(t: &Tree) -> Summary {
    let (sf, sb, tf, tb) = match t.root {
        Some(r) => {
            let n = t.arena.node(r);
            (n.sel_files, n.sel_size, n.total_files, n.total_size)
        }
        None => (0, 0, 0, 0),
    };
    Summary {
        sel_files: sf,
        sel_bytes: sb,
        total_files: tf,
        total_bytes: tb,
        sources: t.sources.paths().len(),
        issues: t.issues.len(),
    }
}

/// Everything the UI needs after the tree is rebuilt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeUpdate {
    pub root: Option<NodeView>,
    pub summary: Summary,
    pub issues: Vec<ScanIssue>,
    pub sort: SortKey,
}

fn tree_update(t: &Tree) -> TreeUpdate {
    TreeUpdate {
        root: t.root.map(|r| view(&t.arena, r)),
        summary: summary(t),
        issues: t.issues.clone(),
        sort: t.sort,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckUpdate {
    pub node: NodeView,
    /// Nearest parent first. The frontend patches these rows and drops its
    /// cached subtree under `node`, so no per-descendant payload is needed.
    pub ancestors: Vec<NodeView>,
    pub summary: Summary,
}

// ---------------------------------------------------------------- tree commands

#[tauri::command]
pub async fn add_paths(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Cmd<TreeUpdate> {
    let cancel = state.scan_cancel.clone();
    cancel.store(false, Ordering::Relaxed);

    let mut scanned = Vec::new();
    let mut issues = Vec::new();

    for raw in paths {
        // Skip anything an existing source already covers, so dropping a
        // folder twice does not rescan it.
        {
            let t = lock(&state)?;
            if let Ok(c) = fsutil::canonical(Path::new(&raw)) {
                if t.sources.covers(&c) {
                    continue;
                }
            }
        }

        let app_handle = app.clone();
        let c = cancel.clone();
        let target = PathBuf::from(&raw);

        let outcome = tauri::async_runtime::spawn_blocking(move || {
            let mut last = Instant::now();
            scan_path(&target, &c, move |stats, current| {
                // ~15 updates a second is enough to look live without
                // flooding the IPC channel.
                if last.elapsed() >= Duration::from_millis(66) {
                    last = Instant::now();
                    let _ = app_handle.emit(
                        events::SCAN_PROGRESS,
                        events::ScanProgress {
                            dirs: stats.dirs,
                            files: stats.files,
                            bytes: stats.bytes,
                            current: fsutil::display_path(current),
                        },
                    );
                }
            })
        })
        .await
        .map_err(|e| format!("the scan task failed: {e}"))?;

        match outcome {
            Ok(o) => {
                issues.extend(o.issues);
                scanned.push(o.source);
            }
            // One unreadable path must not sink the whole drop.
            Err(e) => issues.push(ScanIssue {
                path: raw,
                message: e.to_string(),
            }),
        }
    }

    let mut t = lock(&state)?;
    let prior = snapshot_checks(&t.arena);
    for s in scanned {
        t.sources.add(s);
    }
    t.issues.extend(issues);
    t.reroot(&prior);

    let update = tree_update(&t);
    drop(t);
    let _ = app.emit(events::SCAN_DONE, ());
    Ok(update)
}

/// Removes whichever source contains `id`, which is more forgiving than
/// demanding the user select the source root exactly.
#[tauri::command]
pub fn remove_node(state: State<'_, AppState>, id: NodeId) -> Cmd<TreeUpdate> {
    let mut t = lock(&state)?;
    let path = t
        .arena
        .get(id)
        .and_then(|n| n.path.clone())
        .ok_or("that row is not backed by a folder on disk")?;

    let owner = t
        .sources
        .iter()
        .map(|s| s.path.clone())
        .find(|p| fsutil::contains(p, &path))
        .ok_or("no source covers that row")?;

    let prior = snapshot_checks(&t.arena);
    t.sources.remove_path(&owner);
    t.issues.clear();
    t.reroot(&prior);
    Ok(tree_update(&t))
}

#[tauri::command]
pub fn clear_all(state: State<'_, AppState>) -> Cmd<TreeUpdate> {
    let mut t = lock(&state)?;
    t.sources.clear();
    t.issues.clear();
    t.arena = Arena::new();
    t.root = None;
    Ok(tree_update(&t))
}

#[tauri::command]
pub fn get_children(state: State<'_, AppState>, id: NodeId) -> Cmd<Vec<NodeView>> {
    let t = lock(&state)?;
    if t.arena.get(id).is_none() {
        return Ok(Vec::new());
    }
    let mut kids = t.arena.children(id).to_vec();
    sort_children(&t.arena, &mut kids, t.sort);
    Ok(kids.into_iter().map(|c| view(&t.arena, c)).collect())
}

#[tauri::command]
pub fn set_sort(state: State<'_, AppState>, by: SortBy, dir: SortDir) -> Cmd<()> {
    lock(&state)?.sort = SortKey { by, dir };
    Ok(())
}

#[tauri::command]
pub fn set_checked(state: State<'_, AppState>, id: NodeId, checked: bool) -> Cmd<CheckUpdate> {
    let mut t = lock(&state)?;
    if t.arena.get(id).is_none() {
        return Err("that row no longer exists".into());
    }
    let ancestors = check::set_checked(&mut t.arena, id, checked);
    Ok(CheckUpdate {
        node: view(&t.arena, id),
        ancestors: ancestors.into_iter().map(|a| view(&t.arena, a)).collect(),
        summary: summary(&t),
    })
}

#[tauri::command]
pub fn set_all_checked(state: State<'_, AppState>, checked: bool) -> Cmd<TreeUpdate> {
    let mut t = lock(&state)?;
    if let Some(root) = t.root {
        check::set_checked(&mut t.arena, root, checked);
    }
    Ok(tree_update(&t))
}

#[tauri::command]
pub fn get_summary(state: State<'_, AppState>) -> Cmd<Summary> {
    let t = lock(&state)?;
    Ok(summary(&t))
}

// ---------------------------------------------------------------- plan I/O

#[tauri::command]
pub fn save_plan(state: State<'_, AppState>, path: String) -> Cmd<()> {
    let t = lock(&state)?;
    let root = t.root.ok_or("there is nothing to save yet")?;

    let root_path = if t.arena.node(root).kind == NodeKind::SyntheticRoot {
        None
    } else {
        t.arena.node(root).path.as_deref().map(fsutil::display_path)
    };

    let plan = ArchivePlan {
        version: PLAN_VERSION,
        generator: concat!("tree-archiver ", env!("CARGO_PKG_VERSION")).into(),
        created: fsutil::iso8601_utc(SystemTime::now()),
        root: root_path,
        sources: t.sources.paths().iter().map(|p| fsutil::display_path(p)).collect(),
        sort: t.sort,
        output: t.output,
        rules: plan::compact(&t.arena, root),
    };

    let json = serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("could not write the plan: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadPlanResult {
    pub tree: TreeUpdate,
    pub unresolved: Vec<UnresolvedRule>,
    pub output: OutputOptions,
}

#[tauri::command]
pub async fn load_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Cmd<LoadPlanResult> {
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read the plan: {e}"))?;
    let loaded: ArchivePlan =
        serde_json::from_str(&text).map_err(|e| format!("that is not a valid plan file: {e}"))?;

    if loaded.version > PLAN_VERSION {
        return Err(format!(
            "the plan was written by a newer version (plan v{}, this app reads v{PLAN_VERSION})",
            loaded.version
        ));
    }

    let cancel = state.scan_cancel.clone();
    cancel.store(false, Ordering::Relaxed);

    // Sources are rescanned: the plan records what to include, never the
    // contents, so the tree is rebuilt from whatever is on disk now.
    let mut scanned = Vec::new();
    let mut issues = Vec::new();
    for src in &loaded.sources {
        let app_handle = app.clone();
        let c = cancel.clone();
        let target = PathBuf::from(src);

        let outcome = tauri::async_runtime::spawn_blocking(move || {
            let mut last = Instant::now();
            scan_path(&target, &c, move |stats, current| {
                if last.elapsed() >= Duration::from_millis(66) {
                    last = Instant::now();
                    let _ = app_handle.emit(
                        events::SCAN_PROGRESS,
                        events::ScanProgress {
                            dirs: stats.dirs,
                            files: stats.files,
                            bytes: stats.bytes,
                            current: fsutil::display_path(current),
                        },
                    );
                }
            })
        })
        .await
        .map_err(|e| format!("the scan task failed: {e}"))?;

        match outcome {
            Ok(o) => {
                issues.extend(o.issues);
                scanned.push(o.source);
            }
            Err(e) => issues.push(ScanIssue {
                path: src.clone(),
                message: e.to_string(),
            }),
        }
    }

    let mut t = lock(&state)?;
    t.sources.clear();
    t.issues = issues;
    for s in scanned {
        t.sources.add(s);
    }
    t.sort = loaded.sort;
    t.output = loaded.output;

    t.reroot(&Default::default());

    let unresolved = match t.root {
        Some(root) => {
            let rules = loaded.rules.clone();
            plan::apply(&mut t.arena, root, &rules)
        }
        None => Vec::new(),
    };

    let result = LoadPlanResult {
        tree: tree_update(&t),
        unresolved,
        output: t.output,
    };
    drop(t);
    let _ = app.emit(events::SCAN_DONE, ());
    Ok(result)
}

// ---------------------------------------------------------------- archiving

#[tauri::command]
pub fn set_output(state: State<'_, AppState>, options: OutputOptions) -> Cmd<()> {
    lock(&state)?.output = options;
    Ok(())
}

#[tauri::command]
pub fn estimate(state: State<'_, AppState>) -> Cmd<Estimate> {
    let t = lock(&state)?;
    let root = t.root.ok_or("there is nothing to archive yet")?;
    Ok(archive::estimate(&archive::collect_entries(
        &t.arena, root, t.sort,
    )))
}

/// A sensible default file name: the root folder plus today's date.
#[tauri::command]
pub fn suggested_output_name(state: State<'_, AppState>) -> Cmd<String> {
    let t = lock(&state)?;
    let stem = t
        .root
        .map(|r| t.arena.node(r).name.clone())
        .unwrap_or_else(|| "archive".into());
    let safe: String = stem
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let date = &fsutil::iso8601_utc(SystemTime::now())[..10];
    Ok(format!("{safe}-{date}.{}", t.output.compression.extension()))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequest {
    pub out_path: String,
    pub options: OutputOptions,
}

#[tauri::command]
pub fn start_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ArchiveRequest,
) -> Cmd<()> {
    if state.archiving.load(Ordering::Relaxed) {
        return Err("an archive is already being written".into());
    }

    let entries: Vec<Entry> = {
        let mut t = lock(&state)?;
        let root = t.root.ok_or("there is nothing to archive yet")?;
        t.output = request.options;
        let sort = t.sort;
        archive::collect_entries(&t.arena, root, sort)
    };

    if entries.is_empty() {
        return Err("nothing is selected".into());
    }

    state.log.lock().map_err(|_| "the log was poisoned")?.clear();
    state.archive_cancel.store(false, Ordering::Relaxed);
    state.archiving.store(true, Ordering::Relaxed);

    let cancel = state.archive_cancel.clone();
    let archiving = state.archiving.clone();
    let out_path = PathBuf::from(&request.out_path);
    let options = request.options;
    let app_handle = app.clone();

    // The same Arc app state holds, so `save_log` can read the log while the
    // run is still going.
    let log_for_thread = state.log.clone();

    std::thread::spawn(move || {
        let mut last_emit = Instant::now();
        let summary: ArchiveSummary = archive::run(
            &entries,
            &out_path,
            options,
            cancel,
            |p| {
                // 10 updates a second keeps the bar smooth without spamming.
                if last_emit.elapsed() >= Duration::from_millis(100) {
                    last_emit = Instant::now();
                    let _ = app_handle.emit(events::ARCHIVE_PROGRESS, p);
                }
            },
            |entry| {
                if let Ok(mut l) = log_for_thread.lock() {
                    l.push(entry.clone());
                }
                let _ = app_handle.emit(events::ARCHIVE_LOG, entry);
            },
        );

        archiving.store(false, Ordering::Relaxed);
        let _ = app_handle.emit(events::ARCHIVE_DONE, summary);
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_archive(state: State<'_, AppState>) -> Cmd<()> {
    state.archive_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> Cmd<()> {
    state.scan_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn save_log(state: State<'_, AppState>, path: String) -> Cmd<usize> {
    let log = state.log.lock().map_err(|_| "the log was poisoned")?;
    let mut text = String::new();
    for e in log.iter() {
        let level = match e.level {
            LogLevel::Info => "INFO ",
            LogLevel::Warn => "WARN ",
            LogLevel::Error => "ERROR",
        };
        text.push_str(&format!("{} {} {}  {}\n", e.ts, level, e.path, e.message));
    }
    std::fs::write(&path, text).map_err(|e| format!("could not write the log: {e}"))?;
    Ok(log.len())
}

/// Scan problems are surfaced separately from the archive log, since they
/// happen while the user is still designing.
#[tauri::command]
pub fn get_issues(state: State<'_, AppState>) -> Cmd<Vec<ScanIssue>> {
    Ok(lock(&state)?.issues.clone())
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Cmd<TreeUpdate> {
    let t = lock(&state)?;
    Ok(tree_update(&t))
}

/// The extension the current compression setting implies, so the save dialog
/// can suggest the right one.
#[tauri::command]
pub fn output_extension(compression: Compression) -> String {
    compression.extension().to_string()
}
