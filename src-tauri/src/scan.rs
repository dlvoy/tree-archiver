//! Directory scanning.
//!
//! A scan produces a standalone `ScanDir` tree rather than writing into the
//! arena directly. Sources have to survive re-rooting — every add or remove
//! rebuilds the arena around a new common root — so the scanned data is kept
//! detached and grafted in each time.

use crate::fsutil;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ScanFile {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ScanDir {
    pub name: String,
    pub dirs: Vec<ScanDir>,
    /// Direct file children. Grafted under a `<files>` group.
    pub files: Vec<ScanFile>,
}

/// One user-added path. A dragged-in file is a source in its own right; its
/// parent directory becomes part of the spine.
#[derive(Debug, Clone)]
pub enum SourceTree {
    Dir(ScanDir),
    File(ScanFile),
}

#[derive(Debug, Clone)]
pub struct Source {
    /// Canonical absolute path, verbatim form on Windows.
    pub path: PathBuf,
    pub tree: SourceTree,
}

impl Source {
    pub fn total_size(&self) -> u64 {
        match &self.tree {
            SourceTree::File(f) => f.size,
            SourceTree::Dir(d) => dir_size(d),
        }
    }
}

fn dir_size(d: &ScanDir) -> u64 {
    let files: u64 = d.files.iter().map(|f| f.size).sum();
    d.dirs
        .iter()
        .fold(files, |acc, c| acc.saturating_add(dir_size(c)))
}

/// A path that could not be read. Scanning never aborts on these.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScanStats {
    pub dirs: u64,
    pub files: u64,
    pub bytes: u64,
}

pub struct ScanOutcome {
    pub source: Source,
    pub issues: Vec<ScanIssue>,
    pub stats: ScanStats,
}

/// Scans `path` recursively.
///
/// `on_progress` is called as directories are entered; the caller is
/// responsible for throttling it before it reaches the UI. `cancel` is polled
/// per directory so a scan of a huge tree can be abandoned.
pub fn scan_path<F>(
    path: &Path,
    cancel: &Arc<AtomicBool>,
    mut on_progress: F,
) -> std::io::Result<ScanOutcome>
where
    F: FnMut(ScanStats, &Path),
{
    let canonical = fsutil::canonical(path)?;
    let md = std::fs::symlink_metadata(&canonical)?;
    let name = file_name_of(&canonical);

    let mut issues = Vec::new();
    let mut stats = ScanStats::default();

    let tree = if md.is_dir() && !fsutil::is_reparse_point(&md) {
        let mut root = ScanDir {
            name: name.clone(),
            ..Default::default()
        };
        stats.dirs += 1;
        scan_into(
            &canonical,
            &mut root,
            cancel,
            &mut issues,
            &mut stats,
            &mut on_progress,
        );
        SourceTree::Dir(root)
    } else {
        stats.files += 1;
        stats.bytes = md.len();
        SourceTree::File(ScanFile {
            name: name.clone(),
            size: md.len(),
        })
    };

    Ok(ScanOutcome {
        source: Source {
            path: canonical,
            tree,
        },
        issues,
        stats,
    })
}

/// Iterative depth-first walk. Recursion would risk a stack overflow on
/// pathological trees, and an explicit stack makes cancellation checks cheap.
fn scan_into<F>(
    root_path: &Path,
    root: &mut ScanDir,
    cancel: &Arc<AtomicBool>,
    issues: &mut Vec<ScanIssue>,
    stats: &mut ScanStats,
    on_progress: &mut F,
) where
    F: FnMut(ScanStats, &Path),
{
    // Each frame addresses a directory by the index path from `root`, which
    // sidesteps holding a mutable borrow across iterations.
    let mut stack: Vec<(PathBuf, Vec<usize>)> = vec![(root_path.to_path_buf(), Vec::new())];

    while let Some((dir_path, index_path)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        let entries = match std::fs::read_dir(&dir_path) {
            Ok(e) => e,
            Err(e) => {
                issues.push(ScanIssue {
                    path: fsutil::display_path(&dir_path),
                    message: e.to_string(),
                });
                continue;
            }
        };

        let mut sub_dirs: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<ScanFile> = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    issues.push(ScanIssue {
                        path: fsutil::display_path(&dir_path),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let epath = entry.path();
            let md = match std::fs::symlink_metadata(&epath) {
                Ok(m) => m,
                Err(e) => {
                    issues.push(ScanIssue {
                        path: fsutil::display_path(&epath),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let name = file_name_of(&epath);

            if md.is_dir() {
                // Junctions and directory symlinks are recorded but never
                // followed; descending them can loop forever.
                if fsutil::is_reparse_point(&md) {
                    issues.push(ScanIssue {
                        path: fsutil::display_path(&epath),
                        message: "reparse point (junction or symlink) not followed".into(),
                    });
                    continue;
                }
                sub_dirs.push((name, epath));
            } else if md.is_file() {
                stats.files += 1;
                stats.bytes = stats.bytes.saturating_add(md.len());
                files.push(ScanFile {
                    name,
                    size: md.len(),
                });
            }
        }

        stats.dirs += sub_dirs.len() as u64;
        on_progress(*stats, &dir_path);

        let node = dir_at_mut(root, &index_path);
        node.files = files;
        node.dirs = sub_dirs
            .iter()
            .map(|(n, _)| ScanDir {
                name: n.clone(),
                ..Default::default()
            })
            .collect();

        for (i, (_, p)) in sub_dirs.into_iter().enumerate() {
            let mut child_index = index_path.clone();
            child_index.push(i);
            stack.push((p, child_index));
        }
    }
}

fn dir_at_mut<'a>(root: &'a mut ScanDir, index_path: &[usize]) -> &'a mut ScanDir {
    let mut cur = root;
    for &i in index_path {
        cur = &mut cur.dirs[i];
    }
    cur
}

/// Last component of a path, falling back to the whole path for volume roots
/// such as `C:\`, which have no file name.
pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| fsutil::display_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Builds a throwaway tree under the OS temp dir and hands back its root.
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!(
                "tree-archiver-test-{tag}-{}",
                std::process::id() as u64 * 31 + tag.len() as u64
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempTree(p)
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn file(&self, rel: &str, bytes: usize) {
            let p = self.0.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, vec![b'x'; bytes]).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scan_captures_nested_dirs_and_file_sizes() {
        let t = TempTree::new("nested");
        t.dir("a/b");
        t.file("root.txt", 10);
        t.file("a/one.bin", 100);
        t.file("a/b/two.bin", 200);

        let cancel = Arc::new(AtomicBool::new(false));
        let out = scan_path(&t.0, &cancel, |_, _| {}).unwrap();

        let SourceTree::Dir(root) = &out.source.tree else {
            panic!("expected a directory source");
        };
        assert_eq!(root.files.len(), 1);
        assert_eq!(root.dirs.len(), 1);
        assert_eq!(dir_size(root), 310);
        assert_eq!(out.stats.files, 3);
        assert_eq!(out.stats.bytes, 310);

        let a = &root.dirs[0];
        assert_eq!(a.name, "a");
        assert_eq!(a.files.len(), 1);
        assert_eq!(a.dirs[0].files[0].size, 200);
    }

    #[test]
    fn scanning_a_single_file_yields_a_file_source() {
        let t = TempTree::new("single");
        t.file("solo.dat", 42);
        let cancel = Arc::new(AtomicBool::new(false));
        let out = scan_path(&t.0.join("solo.dat"), &cancel, |_, _| {}).unwrap();

        match &out.source.tree {
            SourceTree::File(f) => {
                assert_eq!(f.name, "solo.dat");
                assert_eq!(f.size, 42);
            }
            _ => panic!("expected a file source"),
        }
        assert_eq!(out.source.total_size(), 42);
    }

    #[test]
    fn empty_directories_scan_clean() {
        let t = TempTree::new("empty");
        t.dir("hollow");
        let cancel = Arc::new(AtomicBool::new(false));
        let out = scan_path(&t.0, &cancel, |_, _| {}).unwrap();

        let SourceTree::Dir(root) = &out.source.tree else {
            panic!("expected a directory source");
        };
        assert_eq!(root.dirs.len(), 1);
        assert!(root.dirs[0].dirs.is_empty());
        assert!(root.dirs[0].files.is_empty());
        assert!(out.issues.is_empty());
    }

    #[test]
    fn progress_reports_running_totals() {
        let t = TempTree::new("progress");
        t.file("a/1.bin", 5);
        t.file("b/2.bin", 5);
        let cancel = Arc::new(AtomicBool::new(false));

        let mut seen = 0usize;
        let out = scan_path(&t.0, &cancel, |_, _| seen += 1).unwrap();

        // Root plus two subdirectories.
        assert_eq!(seen, 3);
        assert_eq!(out.stats.files, 2);
    }

    #[test]
    fn cancellation_stops_the_walk() {
        let t = TempTree::new("cancel");
        t.file("a/1.bin", 5);
        t.file("b/2.bin", 5);
        let cancel = Arc::new(AtomicBool::new(true));
        let out = scan_path(&t.0, &cancel, |_, _| {}).unwrap();
        assert_eq!(out.stats.files, 0);
    }

    #[test]
    fn missing_path_is_an_error_not_a_panic() {
        let cancel = Arc::new(AtomicBool::new(false));
        let missing = std::env::temp_dir().join("tree-archiver-does-not-exist-xyz");
        assert!(scan_path(&missing, &cancel, |_, _| {}).is_err());
    }
}
