//! The IPC surface.
//!
//! The webview never touches the filesystem and never receives the whole tree.
//! It asks for one node's children at a time and caches them, which keeps a
//! scan of a few hundred thousand files off the IPC boundary entirely.

use crate::archive::{self, ArchiveSummary, Entry, Estimate, LogEntry, LogLevel, Progress};
use crate::events;
use crate::fsutil;
use crate::model::arena::{Arena, CheckState, NodeId, NodeKind};
use crate::model::check;
use crate::model::sort::{sort_children, SortBy, SortDir, SortKey};
use crate::naming::{self, ModeAvailability, NamingContext};
use crate::plan::{
    self, ArchivePlan, Compression, FileOrder, OutputOptions, PathMode, UnresolvedRule,
    PLAN_VERSION,
};
use crate::roots::{rebuild, snapshot_checks, Sources};
use crate::scan::{scan_path, ScanIssue};
use crate::explorer;
use crate::settings::{self, Settings};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct Tree {
    pub arena: Arena,
    pub root: Option<NodeId>,
    pub sources: Sources,
    pub sort: SortKey,
    pub issues: Vec<ScanIssue>,
    pub output: OutputOptions,
    pub file_order: FileOrder,
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
    /// Paths handed over by File Explorer, waiting to be coalesced. Explorer
    /// starts one process per selected item, so a five-file selection arrives
    /// as five separate hand-offs within a few milliseconds.
    pub external: Arc<Mutex<Vec<String>>>,
    /// Bumped on every hand-off. The debounce task only acts if it still holds
    /// the newest value, so only the last arrival in a burst does the work.
    pub external_gen: Arc<AtomicU64>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            tree: Mutex::new(Tree::default()),
            log: Arc::new(Mutex::new(Vec::new())),
            scan_cancel: Arc::new(AtomicBool::new(false)),
            archive_cancel: Arc::new(AtomicBool::new(false)),
            archiving: Arc::new(AtomicBool::new(false)),
            external: Arc::new(Mutex::new(Vec::new())),
            external_gen: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// A batch is flushed on this many lines even if the timer has not expired,
/// so a fast local disk cannot build an unbounded backlog.
const LOG_BATCH_MAX: usize = 2000;

/// How often the progress panel is refreshed while a run is going. Ten a
/// second is smooth without flooding the IPC channel.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Rate-limits progress events on their way to the window.
///
/// Two things here are load-bearing, and both were once wrong. The first event
/// goes out immediately, so a run shorter than one interval still reports
/// something. And whatever arrives between the last tick and the end of the run
/// is kept, so `flush` can send it: that final event is the one carrying the
/// full file count, and dropping it left the panel reading "4 / 61" beside a
/// summary that said 61.
struct ProgressThrottle {
    every: Duration,
    last_emit: Instant,
    latest: Option<Progress>,
    /// Whether `latest` has already gone out, so `flush` does not repeat it.
    sent: bool,
}

impl ProgressThrottle {
    fn new(every: Duration) -> Self {
        ProgressThrottle {
            every,
            // Far enough back that the first event is due at once.
            last_emit: Instant::now() - every,
            latest: None,
            sent: true,
        }
    }

    /// The event to send now, if one is due.
    fn push(&mut self, p: Progress) -> Option<Progress> {
        self.latest = Some(p);
        self.sent = false;
        if self.last_emit.elapsed() >= self.every {
            self.last_emit = Instant::now();
            self.sent = true;
            return Some(p);
        }
        None
    }

    /// The last event held back, if any. Called once the run is over.
    fn flush(&mut self) -> Option<Progress> {
        if self.sent {
            return None;
        }
        self.sent = true;
        self.latest
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


// ---------------------------------------------------------------- view state

/// A key that survives a rebuild.
///
/// Adding or removing a source renumbers every node, so ids cannot be used to
/// remember which branches were open. Paths can.
fn view_key(arena: &Arena, id: NodeId) -> Option<String> {
    let n = arena.node(id);
    match n.kind {
        // The group has no path of its own, so it is keyed by its parent.
        NodeKind::FilesGroup => {
            let parent = n.parent?;
            let p = arena.node(parent).path.as_deref()?;
            Some(format!("{}\u{0}<files>", fsutil::display_path(p)))
        }
        NodeKind::SyntheticRoot => Some("\u{0}sources".into()),
        _ => n.path.as_deref().map(fsutil::display_path),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub key: String,
    pub id: NodeId,
    pub children: Vec<NodeView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredView {
    pub branches: Vec<Branch>,
    pub selected: Option<NodeId>,
}

/// Re-opens the branches named by `expanded` after a rebuild, in one round
/// trip regardless of how many are open. Keys that no longer resolve are
/// dropped, which is the right outcome when their source was removed.
#[tauri::command]
pub fn restore_view(
    state: State<'_, AppState>,
    expanded: Vec<String>,
    selected: Option<String>,
) -> Cmd<RestoredView> {
    let t = lock(&state)?;
    let Some(root) = t.root else {
        return Ok(RestoredView {
            branches: Vec::new(),
            selected: None,
        });
    };

    let mut index: HashMap<String, NodeId> = HashMap::new();
    for d in t.arena.descendants(root) {
        if let Some(k) = view_key(&t.arena, d) {
            index.insert(k, d);
        }
    }

    // A branch is only visible if everything above it is open too, and a
    // rebuild can insert levels that were never in the saved set — a higher
    // common root, or the synthetic root that appears with a second volume. So
    // each resolved branch drags its ancestors open with it.
    let mut open: HashSet<NodeId> = HashSet::new();
    for k in &expanded {
        let Some(&id) = index.get(k) else { continue };
        let mut cur = Some(id);
        while let Some(c) = cur {
            if !open.insert(c) {
                break; // this chain was already walked
            }
            cur = t.arena.node(c).parent;
        }
    }

    let branches = open
        .into_iter()
        .filter(|&id| !t.arena.children(id).is_empty())
        .filter_map(|id| {
            let key = view_key(&t.arena, id)?;
            let mut kids = t.arena.children(id).to_vec();
            sort_children(&t.arena, &mut kids, t.sort);
            Some(Branch {
                key,
                id,
                children: kids.into_iter().map(|c| view(&t.arena, c)).collect(),
            })
        })
        .collect();

    Ok(RestoredView {
        branches,
        selected: selected.and_then(|s| index.get(&s).copied()),
    })
}

// ---------------------------------------------------------------- settings

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

/// Applies the settings to the running session as well as writing them, so
/// there is one path for "the user changed a preference".
#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Cmd<()> {
    {
        let mut t = lock(&state)?;
        t.sort = settings.sort;
        t.output = settings.output;
        t.file_order = settings.file_order;
    }
    settings::save(&app, &settings)
}

// ---------------------------------------------------------------- explorer

#[tauri::command]
pub fn explorer_status() -> bool {
    explorer::is_installed()
}

/// `label` is the menu text, already translated by the frontend — the registry
/// stores one fixed string, so it is written in whatever language was active.
#[tauri::command]
pub fn explorer_install(label: String) -> Cmd<bool> {
    explorer::install(&label)?;
    Ok(explorer::is_installed())
}

#[tauri::command]
pub fn explorer_uninstall() -> Cmd<bool> {
    explorer::uninstall()?;
    Ok(explorer::is_installed())
}

// ---------------------------------------------------------------- external staging

/// How long to wait for more paths before scanning. Explorer starts one
/// process per selected item, so they arrive a few milliseconds apart.
const EXTERNAL_COALESCE: Duration = Duration::from_millis(400);

/// Takes paths handed over from outside the window — the command line, or a
/// second instance started by the Explorer menu — and stages them as though
/// they had been dropped on the window.
///
/// Arrivals are coalesced so that selecting five folders produces one scan and
/// one tree rebuild rather than five of each.
pub fn stage_external(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    {
        let Ok(mut queue) = state.external.lock() else {
            return;
        };
        queue.extend(paths);
    }
    let mine = state.external_gen.fetch_add(1, Ordering::SeqCst) + 1;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio_sleep(EXTERNAL_COALESCE).await;

        let state = app.state::<AppState>();
        // A later arrival owns the batch; this task has nothing left to do.
        if state.external_gen.load(Ordering::SeqCst) != mine {
            return;
        }
        let batch = match state.external.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(_) => return,
        };
        if batch.is_empty() {
            return;
        }

        let _ = app.emit(events::TREE_SCANNING, true);
        let result = add_paths(app.clone(), app.state::<AppState>(), batch).await;
        let _ = app.emit(events::TREE_SCANNING, false);

        match result {
            Ok(update) => {
                let _ = app.emit(events::TREE_UPDATED, update);
            }
            Err(e) => {
                let _ = app.emit(events::TREE_ERROR, e);
            }
        }
        focus_main_window(&app);
    });
}

async fn tokio_sleep(d: Duration) {
    // Tauri's async runtime is tokio, but the re-export is not public API, so
    // the sleep goes through a blocking task rather than importing tokio here.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

/// Explorer starts the app behind whatever window had focus, so a hand-off to
/// an already-running instance has to raise the window itself.
pub fn focus_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Pulls stageable paths out of a command line.
///
/// Accepts `--add <path>` as the Explorer verb sends it, and bare paths so the
/// exe can be used from a shell without ceremony. The first argument is the
/// executable and is always dropped.
pub fn paths_from_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    let mut iter = args.into_iter().skip(1).peekable();
    while let Some(arg) = iter.next() {
        let a = arg.as_ref();
        if a == "--add" {
            if let Some(next) = iter.next() {
                out.push(next.as_ref().to_string());
            }
        } else if let Some(rest) = a.strip_prefix("--add=") {
            out.push(rest.to_string());
        } else if !a.starts_with("--") {
            out.push(a.to_string());
        }
    }
    out
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

/// Entry names differ per mode, and a name over 100 bytes costs an extra
/// header block, so the predicted size is mode-specific.
#[tauri::command]
pub fn estimate(state: State<'_, AppState>, mode: Option<PathMode>) -> Cmd<Estimate> {
    let t = lock(&state)?;
    let root = t.root.ok_or("there is nothing to archive yet")?;
    let ctx = NamingContext::from_sources(t.sources.iter());
    let mode = mode.unwrap_or(t.output.path_mode);
    Ok(archive::estimate(&archive::collect_entries(
        &t.arena, root, t.sort, mode, &ctx,
    )))
}

/// Which path modes can be used without two folders colliding at the top of
/// the archive, plus a sample entry name for each so the dialog can show what
/// the choice actually does.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathModeOptions {
    #[serde(flatten)]
    pub availability: ModeAvailability,
    pub folders_only_sample: Option<String>,
    pub common_root_sample: Option<String>,
    pub full_path_sample: Option<String>,
}

#[tauri::command]
pub fn path_mode_options(state: State<'_, AppState>) -> Cmd<PathModeOptions> {
    let t = lock(&state)?;
    let ctx = NamingContext::from_sources(t.sources.iter());
    let sample_path = t
        .root
        .map(|r| first_named_descendant(&t.arena, r))
        .unwrap_or(None);

    let sample = |mode: PathMode| -> Option<String> {
        sample_path.as_deref().and_then(|p| ctx.entry_name(mode, p))
    };

    Ok(PathModeOptions {
        availability: naming::available_modes(&ctx),
        folders_only_sample: sample(PathMode::FoldersOnly),
        common_root_sample: sample(PathMode::CommonRoot),
        full_path_sample: sample(PathMode::FullPath),
    })
}

/// A representative file, for the sample entry names. Falls back to any node
/// with a path when the tree holds no files at all.
fn first_named_descendant(arena: &Arena, root: NodeId) -> Option<PathBuf> {
    let mut fallback = None;
    for d in arena.descendants(root) {
        let n = arena.node(d);
        let Some(p) = &n.path else { continue };
        if n.kind.is_file() {
            return Some(p.clone());
        }
        fallback.get_or_insert_with(|| p.clone());
    }
    fallback
}

/// A sensible default file name: the root folder plus today's date.
#[tauri::command]
pub fn suggested_output_name(state: State<'_, AppState>) -> Cmd<String> {
    let t = lock(&state)?;
    // A drive-root tree is named `C:\`, which would sanitise to "C__".
    let stem = t
        .root
        .and_then(|r| t.arena.node(r).path.as_deref().map(naming::safe_name))
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
        let mode = request.options.path_mode;
        let file_order = t.file_order;
        let ctx = NamingContext::from_sources(t.sources.iter());
        let entries = archive::collect_entries(&t.arena, root, sort, mode, &ctx);
        crate::file_order::reorder(entries, file_order)
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

    // Every entry added now produces a log line, so batching is not an
    // optimisation — one IPC message per file would stall the window.
    let pending: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());

    let throttle: Mutex<ProgressThrottle> = Mutex::new(ProgressThrottle::new(PROGRESS_EVERY));

    std::thread::spawn(move || {
        let mut last_log = Instant::now();
        let flush = |app: &AppHandle, pending: &Mutex<Vec<LogEntry>>| {
            let batch = match pending.lock() {
                Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
                _ => return,
            };
            let _ = app.emit(events::ARCHIVE_LOG, batch);
        };

        let summary: ArchiveSummary = archive::run(
            &entries,
            &out_path,
            options,
            cancel,
            |p| {
                let due = throttle.lock().ok().and_then(|mut t| t.push(p));
                if let Some(p) = due {
                    let _ = app_handle.emit(events::ARCHIVE_PROGRESS, p);
                }
            },
            |entry| {
                // The full log stays in memory for `save_log`; only the live
                // view is rate-limited.
                if let Ok(mut l) = log_for_thread.lock() {
                    l.push(entry.clone());
                }
                let full = match pending.lock() {
                    Ok(mut p) => {
                        p.push(entry);
                        p.len() >= LOG_BATCH_MAX
                    }
                    Err(_) => false,
                };
                if full || last_log.elapsed() >= Duration::from_millis(120) {
                    last_log = Instant::now();
                    flush(&app_handle, &pending);
                }
            },
        );

        // Whatever the throttle was still holding when the run ended. Without
        // this the panel keeps the last figures that happened to fall on a
        // tick — a file count short of the total, and a bar short of 100%.
        let last = throttle.lock().ok().and_then(|mut t| t.flush());
        if let Some(p) = last {
            let _ = app_handle.emit(events::ARCHIVE_PROGRESS, p);
        }

        // The closing summary lines land in the same batch as the tail.
        flush(&app_handle, &pending);
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

// ---------------------------------------------------------------- about

/// What the About dialog shows, and the licence it links to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    /// `YYYY-MM-DD`, UTC. `unknown` only if the build script could not run.
    pub build_date: &'static str,
    /// Seven characters, or `unknown` when built outside a git checkout.
    pub git_hash: &'static str,
    /// The release this version was cut as. Assembled here so the repository
    /// URL has one source of truth: `Cargo.toml`.
    pub release_url: String,
    pub license: &'static str,
}

/// The licence is compiled in with `include_str!` rather than read at runtime,
/// so it travels with the executable and cannot go missing.
#[tauri::command]
pub fn app_info() -> AppInfo {
    let version = env!("CARGO_PKG_VERSION");
    AppInfo {
        version,
        build_date: env!("TA_BUILD_DATE"),
        git_hash: env!("TA_GIT_HASH"),
        release_url: format!(
            "{}/releases/tag/v{version}",
            env!("CARGO_PKG_REPOSITORY").trim_end_matches('/')
        ),
        license: include_str!("../../LICENSE.txt"),
    }
}

#[cfg(test)]
mod tests {
    use super::{app_info, paths_from_args, Progress, ProgressThrottle};
    use std::time::Duration;

    fn progress(files_done: u64, bytes_done: u64) -> Progress {
        Progress {
            files_done,
            files_total: 61,
            bytes_done,
            bytes_total: 50_000_000,
            bps: 0,
            eta_secs: None,
        }
    }

    /// A run shorter than one interval used to report nothing at all, leaving
    /// every figure in the panel as a dash.
    #[test]
    fn the_first_event_is_never_held_back() {
        let mut t = ProgressThrottle::new(Duration::from_secs(60));
        assert!(t.push(progress(1, 10)).is_some());
    }

    /// The bug behind "4 / 61" beside a summary saying 61: everything after the
    /// last tick was dropped, and the last tick is never the final event.
    #[test]
    fn the_final_event_survives_the_throttle() {
        let mut t = ProgressThrottle::new(Duration::from_secs(60));
        t.push(progress(1, 10)).unwrap();

        // Nothing else is due for a minute, so these are all held.
        assert!(t.push(progress(2, 20)).is_none());
        assert!(t.push(progress(61, 50_000_000)).is_none());

        let last = t.flush().expect("the last event was dropped");
        assert_eq!(last.files_done, 61);
        assert_eq!(last.bytes_done, 50_000_000);
    }

    /// Flushing an event that already went out would emit it twice.
    #[test]
    fn flush_repeats_nothing_that_was_already_sent() {
        let mut t = ProgressThrottle::new(Duration::from_secs(60));
        t.push(progress(1, 10)).unwrap();
        assert!(t.flush().is_none());
    }

    #[test]
    fn flushing_before_anything_happened_yields_nothing() {
        let mut t = ProgressThrottle::new(Duration::from_secs(60));
        assert!(t.flush().is_none());
    }

    /// The licence is compiled in, so an empty or missing one is a build fault
    /// rather than something the user finds at runtime.
    #[test]
    fn the_licence_travels_inside_the_binary() {
        let info = app_info();
        assert!(info.license.contains("MIT License"), "{}", info.license);
        assert!(info.license.contains("Dominik Dzienia"));
        assert!(info.license.contains("WITHOUT WARRANTY OF ANY KIND"));
    }

    /// The About dialog links straight at the tag for this version, so the URL
    /// has to match what the release workflow actually publishes.
    #[test]
    fn the_release_link_points_at_this_version() {
        let info = app_info();
        assert_eq!(
            info.release_url,
            format!(
                "https://github.com/dlvoy/tree-archiver/releases/tag/v{}",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    /// A stamp is only useful if it is a stamp. "unknown" is the documented
    /// fallback, but the shape still has to be right.
    #[test]
    fn the_build_stamp_is_filled_in() {
        let info = app_info();
        assert!(!info.build_date.is_empty());
        assert!(!info.git_hash.is_empty());
        assert!(
            info.build_date == "unknown" || info.build_date.len() == 10,
            "odd build date: {}",
            info.build_date
        );
    }

    /// The shape the registry writes: `"<exe>" --add "%1"`.
    #[test]
    fn the_explorer_verb_form_is_understood() {
        assert_eq!(
            paths_from_args(["tree-archiver.exe", "--add", r"C:\Users\Nick\.aws"]),
            vec![r"C:\Users\Nick\.aws".to_string()]
        );
    }

    #[test]
    fn the_equals_form_is_understood_too() {
        assert_eq!(
            paths_from_args(["exe", r"--add=C:\DOWN\bd"]),
            vec![r"C:\DOWN\bd".to_string()]
        );
    }

    #[test]
    fn bare_paths_work_so_a_shell_needs_no_ceremony() {
        assert_eq!(
            paths_from_args(["exe", r"C:\one", r"D:\two"]),
            vec![r"C:\one".to_string(), r"D:\two".to_string()]
        );
    }

    /// The executable itself is never a path to stage.
    #[test]
    fn the_program_name_is_always_dropped() {
        assert!(paths_from_args(["tree-archiver.exe"]).is_empty());
    }

    /// Tauri and WebView2 add their own switches; none of them are paths.
    #[test]
    fn unknown_flags_are_ignored() {
        assert_eq!(
            paths_from_args(["exe", "--no-sandbox", "--add", r"C:\keep"]),
            vec![r"C:\keep".to_string()]
        );
    }

    /// A dangling `--add` at the end must not panic or invent an entry.
    #[test]
    fn a_trailing_add_without_a_path_yields_nothing() {
        assert!(paths_from_args(["exe", "--add"]).is_empty());
    }
}
