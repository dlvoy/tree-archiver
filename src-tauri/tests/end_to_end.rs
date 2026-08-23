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
use tree_archiver_lib::plan::{self, Action, OutputOptions, Scope};
use tree_archiver_lib::roots::{rebuild, CheckSnapshot, Sources};
use tree_archiver_lib::scan::scan_path;

struct Fixture {
    root: PathBuf,
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

    fn scan(&self) -> (Arena, NodeId, Vec<tree_archiver_lib::scan::ScanIssue>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let out = scan_path(&self.root, &cancel, |_, _| {}).expect("the fixture must scan");
        let issues = out.issues.clone();
        let mut sources = Sources::new();
        sources.add(out.source);
        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &sources, &CheckSnapshot::new())
            .root
            .expect("a scanned source must produce a root");
        (arena, root, issues)
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
    let (arena, root, _) = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default());
    let found = entries.iter().find(|e| e.name.ends_with("buried.txt"));
    assert!(found.is_some(), "the file past 260 characters must be scanned");
    assert_eq!(found.unwrap().size, 17);
}

#[test]
fn writes_and_reads_back_an_entry_past_the_legacy_limit() {
    let fx = Fixture::build("deepwrite");
    fx.add_deep_path();
    let (arena, root, _) = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default());
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

    let (arena, root, issues) = fx.scan();

    // The scan terminated, which is the headline result. Had the junction been
    // followed, it would have recursed until the path length gave out.
    assert!(
        issues.iter().any(|i| i.path.contains("loop")),
        "the junction should be reported: {issues:?}"
    );

    let names: Vec<String> = archive::collect_entries(&arena, root, SortKey::default())
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
    let (mut arena, root, _) = fx.scan();

    let target = node(&arena, &fx.root, "app/target");
    check::set_checked(&mut arena, target, false);

    // The plan names the folder once and says nothing about its contents.
    let rules = plan::compact(&arena, root);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].scope, Scope::Tree);
    assert_eq!(rules[0].action, Action::Exclude);
    assert_eq!(rules[0].path, "app/target");

    let entries = archive::collect_entries(&arena, root, SortKey::default());
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
    let (mut arena, root, _) = fx.scan();

    let target = node(&arena, &fx.root, "app/target");
    let todo = node(&arena, &fx.root, "docs/notes/todo.md");
    check::set_checked(&mut arena, target, false);
    check::set_checked(&mut arena, todo, false);

    let expected_bytes = arena.node(root).sel_size;
    let rules = plan::compact(&arena, root);
    let json = serde_json::to_string_pretty(&rules).unwrap();

    // Rebuild from a fresh scan, exactly as loading a plan does.
    let (mut fresh, fresh_root, _) = fx.scan();
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
    let (arena, root, _) = fx.scan();
    let entries = archive::collect_entries(&arena, root, SortKey::default());

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
    let (arena, root, _) = fx.scan();

    let entries = archive::collect_entries(&arena, root, SortKey::default());
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
    let (mut arena, root, _) = fx.scan();
    let target = node(&arena, &fx.root, "app/target");
    check::set_checked(&mut arena, target, false);

    let entries = archive::collect_entries(&arena, root, SortKey::default());
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
