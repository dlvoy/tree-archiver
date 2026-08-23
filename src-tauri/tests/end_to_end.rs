//! End-to-end checks against a real directory tree on disk, covering the
//! Windows-specific hazards that unit tests with synthetic trees cannot: paths
//! past the 260-character limit, directory junctions that form a cycle, and a
//! file that disappears between planning and writing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tree_archiver_lib::archive::{self, LogLevel};
use tree_archiver_lib::fsutil;
use tree_archiver_lib::model::arena::{Arena, CheckState, NodeId};
use tree_archiver_lib::model::check;
use tree_archiver_lib::model::sort::SortKey;
use tree_archiver_lib::naming::NamingContext;
use tree_archiver_lib::plan::{self, Action, OutputOptions, PathMode, Scope};
use tree_archiver_lib::roots::{rebuild, CheckSnapshot, Sources};
use tree_archiver_lib::scan::scan_path;

struct Fixture {
    root: PathBuf,
}

struct Scanned {
    arena: Arena,
    root: NodeId,
    ctx: NamingContext,
    issues: Vec<tree_archiver_lib::scan::ScanIssue>,
}

impl Fixture {
    fn build(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("tree-archiver-e2e-{tag}"));
        let _ = fs::remove_dir_all(&root);

        for d in ["app/src", "app/target/deep", "docs/notes", "media"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        write(&root.join("app/src/main.rs"), 120);
        write(&root.join("app/src/lib.rs"), 240);
        write(&root.join("app/README.md"), 60);
        write(&root.join("app/target/big.o"), 4096);
        write(&root.join("app/target/deep/huge.o"), 8192);
        write(&root.join("docs/CHANGELOG.md"), 300);
        write(&root.join("docs/notes/todo.md"), 90);
        write(&root.join("media/clip.bin"), 2048);

        Fixture { root }
    }

    /// A path well past 260 characters, reachable only through the verbatim
    /// prefix. Returns the file that was written.
    fn add_deep_path(&self) -> PathBuf {
        let mut deep = self.root.join("docs");
        for i in 1..=14 {
            deep = deep.join(format!("verylongdirectorysegment{i}"));
        }
        let verbatim = PathBuf::from(format!(r"\\?\{}", deep.display()));
        fs::create_dir_all(&verbatim).unwrap();
        let file = verbatim.join("buried.txt");
        fs::write(&file, b"found me at depth").unwrap();
        assert!(
            deep.join("buried.txt").as_os_str().len() > 260,
            "the deep path must exceed the legacy limit to be a real test"
        );
        file
    }

    /// A junction pointing at the fixture root, which would loop forever if
    /// the scanner followed it.
    fn add_cycle_junction(&self) -> bool {
        let link = self.root.join("media/loop");
        std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&self.root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn scan(&self) -> Scanned {
        self.scan_paths(&[self.root.clone()])
    }

    /// Scans an explicit set of sources, so a test can force the common
    /// ancestor higher than any one of them.
    fn scan_paths(&self, paths: &[PathBuf]) -> Scanned {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut sources = Sources::new();
        let mut issues = Vec::new();
        for p in paths {
            let out = scan_path(p, &cancel, |_, _| {}).expect("the fixture must scan");
            issues.extend(out.issues.clone());
            sources.add(out.source);
        }
        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &sources, &CheckSnapshot::new())
            .root
            .expect("a scanned source must produce a root");
        let ctx = NamingContext::from_sources(sources.iter());
        Scanned {
            arena,
            root,
            ctx,
            issues,
        }
    }

    fn out_file(&self, name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Remove the junction first; deleting it must not follow into the
        // target, and leaving it behind would strand the temp tree.
        let link = self.root.join("media/loop");
        if link.exists() {
            let _ = fs::remove_dir(&link);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(path: &Path, bytes: usize) {
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn node(arena: &Arena, root: &Path, rel: &str) -> NodeId {
    let canonical = fsutil::canonical(&root.join(rel)).expect("path should exist");
    arena
        .find_by_path(&canonical)
        .unwrap_or_else(|| panic!("no node for {rel}"))
}

fn tar_entries(path: &Path) -> Vec<String> {
    let f = fs::File::open(path).unwrap();
    let mut a = tar::Archive::new(f);
    a.entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn scans_paths_longer_than_the_legacy_limit() {
    let fx = Fixture::build("deep");
    fx.add_deep_path();
    let Scanned { arena, root, ctx, .. } = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);
    let found = entries.iter().find(|e| e.name.ends_with("buried.txt"));
    assert!(found.is_some(), "the file past 260 characters must be scanned");
    assert_eq!(found.unwrap().size, 17);
}

#[test]
fn writes_and_reads_back_an_entry_past_the_legacy_limit() {
    let fx = Fixture::build("deepwrite");
    fx.add_deep_path();
    let Scanned { arena, root, ctx, .. } = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);
    let out = fx.out_file("tree-archiver-e2e-deep.tar");
    let summary = archive::run(
        &entries,
        &out,
        OutputOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        |_| {},
    );

    assert!(summary.ok, "{summary:?}");
    assert_eq!(summary.errors, 0);

    let listed = tar_entries(&out);
    let buried = listed.iter().find(|n| n.ends_with("buried.txt")).unwrap();
    // The long name survives via the GNU long-name header.
    assert!(buried.len() > 260, "entry name was truncated: {buried}");
    let _ = fs::remove_file(&out);
}

#[test]
fn a_junction_cycle_is_recorded_but_never_followed() {
    let fx = Fixture::build("junction");
    if !fx.add_cycle_junction() {
        eprintln!("skipping: this environment cannot create junctions");
        return;
    }

    let Scanned { arena, root, ctx, issues } = fx.scan();

    // The scan terminated, which is the headline result. Had the junction been
    // followed, it would have recursed until the path length gave out.
    assert!(
        issues.iter().any(|i| i.path.contains("loop")),
        "the junction should be reported: {issues:?}"
    );

    let names: Vec<String> = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx)
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(
        !names.iter().any(|n| n.contains("loop/")),
        "nothing should have been collected through the junction"
    );
    // Real content is still present.
    assert!(names.iter().any(|n| n.ends_with("clip.bin")));
}

#[test]
fn an_unchecked_folder_saves_as_one_rule_and_vanishes_from_the_tar() {
    let fx = Fixture::build("exclude");
    let Scanned { mut arena, root, ctx, .. } = fx.scan();

    let target = node(&arena, &fx.root, "app/target");
    check::set_checked(&mut arena, target, false);

    // The plan names the folder once and says nothing about its contents.
    let rules = plan::compact(&arena, root);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].scope, Scope::Tree);
    assert_eq!(rules[0].action, Action::Exclude);
    assert_eq!(rules[0].path, "app/target");

    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);
    let out = fx.out_file("tree-archiver-e2e-exclude.tar");
    let summary = archive::run(
        &entries,
        &out,
        OutputOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        |_| {},
    );
    assert!(summary.ok, "{summary:?}");

    let listed = tar_entries(&out);
    // No directory entry, no contents, nothing.
    assert!(!listed.iter().any(|n| n.contains("target")), "{listed:?}");
    assert!(!listed.iter().any(|n| n.contains("big.o")));
    assert!(!listed.iter().any(|n| n.contains("huge.o")));
    // Siblings are untouched.
    assert!(listed.iter().any(|n| n.ends_with("app/src/main.rs")));
    let _ = fs::remove_file(&out);
}

#[test]
fn a_saved_plan_reloads_to_the_same_selection() {
    let fx = Fixture::build("roundtrip");
    let Scanned { mut arena, root, .. } = fx.scan();

    let target = node(&arena, &fx.root, "app/target");
    let todo = node(&arena, &fx.root, "docs/notes/todo.md");
    check::set_checked(&mut arena, target, false);
    check::set_checked(&mut arena, todo, false);

    let expected_bytes = arena.node(root).sel_size;
    let rules = plan::compact(&arena, root);
    let json = serde_json::to_string_pretty(&rules).unwrap();

    // Rebuild from a fresh scan, exactly as loading a plan does.
    let Scanned { arena: mut fresh, root: fresh_root, .. } = fx.scan();
    let parsed: Vec<plan::Rule> = serde_json::from_str(&json).unwrap();
    let unresolved = plan::apply(&mut fresh, fresh_root, &parsed);

    assert!(unresolved.is_empty(), "{unresolved:?}");
    assert_eq!(fresh.node(fresh_root).sel_size, expected_bytes);
    assert_eq!(
        fresh.node(node(&fresh, &fx.root, "app/target")).check,
        CheckState::Unchecked
    );
    assert_eq!(
        fresh.node(node(&fresh, &fx.root, "app/src/main.rs")).check,
        CheckState::Checked
    );
    // Compaction is stable across the round trip.
    assert_eq!(plan::compact(&fresh, fresh_root), rules);
}

#[test]
fn a_file_lost_after_planning_is_logged_and_the_archive_still_completes() {
    let fx = Fixture::build("lost");
    let Scanned { arena, root, ctx, .. } = fx.scan();
    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);

    // Vanishes between planning and writing, the way a build artifact would.
    fs::remove_file(fx.root.join("app/target/big.o")).unwrap();

    let out = fx.out_file("tree-archiver-e2e-lost.tar");
    let mut logs = Vec::new();
    let summary = archive::run(
        &entries,
        &out,
        OutputOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        |e| logs.push(e),
    );

    assert!(summary.ok, "a missing file must not fail the run: {summary:?}");
    assert_eq!(summary.errors, 1);
    assert_eq!(summary.skipped, 1);
    assert!(logs
        .iter()
        .any(|l| l.level == LogLevel::Error && l.path.ends_with("big.o")));

    let listed = tar_entries(&out);
    assert!(!listed.iter().any(|n| n.ends_with("big.o")));
    assert!(listed.iter().any(|n| n.ends_with("main.rs")));
    let _ = fs::remove_file(&out);
}

#[test]
fn the_uncompressed_estimate_matches_the_file_on_disk_exactly() {
    let fx = Fixture::build("estimate");
    fx.add_deep_path();
    let Scanned { arena, root, ctx, .. } = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);
    let est = archive::estimate(&entries);
    let out = fx.out_file("tree-archiver-e2e-estimate.tar");
    let summary = archive::run(
        &entries,
        &out,
        OutputOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        |_| {},
    );

    assert!(summary.ok, "{summary:?}");
    assert_eq!(
        summary.bytes_written, est.tar_bytes,
        "the estimate is advertised as exact for an uncompressed tar"
    );
    let _ = fs::remove_file(&out);
}

#[test]
fn extracting_the_archive_reproduces_the_selected_files() {
    let fx = Fixture::build("extract");
    let Scanned { mut arena, root, ctx, .. } = fx.scan();
    let target = node(&arena, &fx.root, "app/target");
    check::set_checked(&mut arena, target, false);

    let entries = archive::collect_entries(&arena, root, SortKey::default(), PathMode::FoldersOnly, &ctx);
    let out = fx.out_file("tree-archiver-e2e-extract.tar");
    archive::run(
        &entries,
        &out,
        OutputOptions::default(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
        |_| {},
    );

    let dest = std::env::temp_dir().join("tree-archiver-e2e-extract-dest");
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).unwrap();
    tar::Archive::new(fs::File::open(&out).unwrap())
        .unpack(&dest)
        .expect("the archive must unpack cleanly");

    // One top-level folder, named for the root, as promised.
    let top: Vec<_> = fs::read_dir(&dest).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(top.len(), 1, "expected a single top-level directory: {top:?}");

    let base = dest.join(top[0].to_string_lossy().to_string());
    assert_eq!(fs::read(base.join("app/src/main.rs")).unwrap().len(), 120);
    assert!(!base.join("app/target").exists(), "the excluded folder must not reappear");

    let _ = fs::remove_dir_all(&dest);
    let _ = fs::remove_file(&out);
}

/// Unpacks `tar` into a fresh directory and lists its top level.
fn unpack_top_level(tar_path: &Path, dest: &Path) -> Vec<String> {
    let _ = fs::remove_dir_all(dest);
    fs::create_dir_all(dest).unwrap();
    tar::Archive::new(fs::File::open(tar_path).unwrap())
        .unpack(dest)
        .expect("the archive must unpack cleanly");
    let mut top: Vec<String> = fs::read_dir(dest)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    top
}

/// Two sources side by side, so the common ancestor sits above both. This is
/// the shape that used to produce absolute entry names when the ancestor was a
/// drive root; here it exercises all three layouts end to end.
#[test]
fn each_path_mode_lays_the_archive_out_as_documented() {
    let fx = Fixture::build("pathmodes");
    let sources = vec![fx.root.join("app"), fx.root.join("docs")];

    for (mode, tag) in [
        (PathMode::FoldersOnly, "folders"),
        (PathMode::CommonRoot, "common"),
        (PathMode::FullPath, "full"),
    ] {
        let Scanned { arena, root, ctx, .. } = fx.scan_paths(&sources);
        let entries = archive::collect_entries(&arena, root, SortKey::default(), mode, &ctx);
        assert!(!entries.is_empty(), "{mode:?} produced no entries");

        // Whatever the layout, a tar entry is never allowed to be absolute.
        for e in &entries {
            assert!(!e.name.contains(':'), "{mode:?} produced {:?}", e.name);
            assert!(!e.name.starts_with('/'), "{mode:?} produced {:?}", e.name);
        }

        let out = fx.out_file(&format!("tree-archiver-e2e-mode-{tag}.tar"));
        let summary = archive::run(
            &entries,
            &out,
            OutputOptions::default(),
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
        );
        assert!(summary.ok, "{mode:?}: {summary:?}");
        assert_eq!(summary.errors, 0, "{mode:?} logged errors");

        let dest = std::env::temp_dir().join(format!("tree-archiver-e2e-mode-{tag}-dest"));
        let top = unpack_top_level(&out, &dest);

        match mode {
            // Each staged folder stands on its own at the top.
            PathMode::FoldersOnly => {
                assert_eq!(top, vec!["app".to_string(), "docs".to_string()]);
                assert_eq!(
                    fs::read(dest.join("app/src/main.rs")).unwrap().len(),
                    120
                );
            }
            // One folder: the directory both sources share.
            PathMode::CommonRoot => {
                assert_eq!(top.len(), 1, "expected the shared parent alone: {top:?}");
                let base = dest.join(&top[0]);
                assert!(base.join("app/src/main.rs").exists());
                assert!(base.join("docs/CHANGELOG.md").exists());
            }
            // The drive letter, with the whole path beneath it.
            PathMode::FullPath => {
                assert_eq!(top.len(), 1, "expected one volume folder: {top:?}");
                assert!(
                    !top[0].contains(':'),
                    "the volume folder must not carry a colon: {top:?}"
                );
                // The full path is preserved, so the fixture's own name is in there.
                let deep = walkdown(&dest, "main.rs");
                assert!(deep.is_some(), "main.rs should exist somewhere under {top:?}");
            }
        }

        let _ = fs::remove_dir_all(&dest);
        let _ = fs::remove_file(&out);
    }
}

/// Finds a file by name anywhere beneath `dir`.
fn walkdown(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(found) = walkdown(&p, name) {
                return Some(found);
            }
        } else if p.file_name().map(|f| f == name).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}
