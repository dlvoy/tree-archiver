//! Naming entries inside the archive.
//!
//! Every entry name is built as a path relative to an *anchor* directory,
//! optionally behind a prefix segment. Which anchor applies is what separates
//! the three path modes:
//!
//! | Mode          | Anchor                     | Top-level folder      |
//! |---------------|----------------------------|-----------------------|
//! | `FoldersOnly` | the parent of each source  | the source's own name |
//! | `CommonRoot`  | the parent of a group root | the common folder     |
//! | `FullPath`    | the volume root            | the drive or share    |
//!
//! A volume root such as `C:\` has no file name, so anchoring above it is
//! impossible; those cases anchor *at* the volume root and carry an explicit
//! prefix instead. That is what keeps a drive letter and its colon out of
//! entry names — tar rejects anything that is not relative.

use crate::fsutil;
use crate::plan::PathMode;
use crate::scan::{Source, SourceTree};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// A folder name safe to use as an archive segment. A volume root falls back
/// to its volume name, so `C:\` becomes `C` and `\\server\share` becomes
/// `server_share`.
pub fn safe_name(path: &Path) -> String {
    match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => fsutil::volume_folder_name(&fsutil::volume_key(path)),
    }
}

/// The `C:\` (or `\\server\share\`, or `/`) that a path sits on.
fn volume_root(path: &Path) -> PathBuf {
    let root: PathBuf = path
        .components()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .collect();
    if root.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        root
    }
}

/// One base a set of paths is named against.
#[derive(Debug, Clone)]
struct Anchor {
    /// Entries are attributed to this anchor when they live under `scope`.
    scope: PathBuf,
    /// Names are made relative to this directory.
    anchor: PathBuf,
    /// Prepended when `anchor` is a volume root and cannot supply a name.
    prefix: Option<String>,
}

impl Anchor {
    /// Anchors just above `target` so `target`'s own name leads the entry.
    /// A volume root has nothing above it, so it anchors at itself and names
    /// itself through `prefix`.
    fn above(target: &Path, scope: PathBuf) -> Anchor {
        match target.parent() {
            Some(parent) => Anchor {
                scope,
                anchor: parent.to_path_buf(),
                prefix: None,
            },
            None => Anchor {
                scope,
                anchor: target.to_path_buf(),
                prefix: Some(safe_name(target)),
            },
        }
    }

    fn name_for(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.anchor).ok()?;
        let rel = to_slashes(rel);
        Some(match (&self.prefix, rel.is_empty()) {
            (Some(p), true) => p.clone(),
            (Some(p), false) => format!("{p}/{rel}"),
            (None, _) => rel,
        })
    }

    /// The first segment of anything named against this anchor.
    fn top_name(&self) -> String {
        match &self.prefix {
            Some(p) => p.clone(),
            None => safe_name(&self.scope),
        }
    }
}

fn to_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Everything the three modes need in order to name a path.
#[derive(Debug, Clone, Default)]
pub struct NamingContext {
    sources: Vec<Anchor>,
    groups: Vec<Anchor>,
}

impl NamingContext {
    pub fn from_sources<'a, I: IntoIterator<Item = &'a Source>>(sources: I) -> NamingContext {
        // A dragged-in file is a source in its own right, but it cannot host a
        // subtree; the folder that matters is its parent.
        let mut source_paths: Vec<(PathBuf, PathBuf)> = Vec::new(); // (scope, host)
        for s in sources {
            let host = match &s.tree {
                SourceTree::File(_) => s
                    .path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| s.path.clone()),
                SourceTree::Dir(_) => s.path.clone(),
            };
            source_paths.push((s.path.clone(), host));
        }

        // A file source names itself, so it anchors at its parent — the same
        // place its host directory would anchor a child.
        let source_anchors: Vec<Anchor> = source_paths
            .iter()
            .map(|(scope, host)| {
                if scope == host {
                    Anchor::above(scope, scope.clone())
                } else {
                    // File source: anchor at the parent directory so the entry
                    // is just the file name.
                    Anchor {
                        scope: scope.clone(),
                        anchor: host.clone(),
                        prefix: None,
                    }
                }
            })
            .collect();

        // One common root per volume; paths on different volumes share none.
        let mut order: Vec<String> = Vec::new();
        let mut by_volume: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for (_, host) in &source_paths {
            let key = fsutil::volume_key(host);
            if !by_volume.contains_key(&key) {
                order.push(key.clone());
            }
            by_volume.entry(key).or_default().push(host.clone());
        }

        let group_anchors: Vec<Anchor> = order
            .iter()
            .filter_map(|k| {
                let paths = &by_volume[k];
                let root = fsutil::common_ancestor(paths)?;
                Some(Anchor::above(&root, root.clone()))
            })
            .collect();

        NamingContext {
            sources: source_anchors,
            groups: group_anchors,
        }
    }

    fn anchors_for(&self, mode: PathMode) -> &[Anchor] {
        match mode {
            PathMode::FoldersOnly => &self.sources,
            PathMode::CommonRoot => &self.groups,
            PathMode::FullPath => &[],
        }
    }

    /// The archive entry name for `path`.
    ///
    /// `None` means the node contributes nothing: under `FoldersOnly` a
    /// directory that merely leads *to* a source is not itself part of the
    /// archive.
    pub fn entry_name(&self, mode: PathMode, path: &Path) -> Option<String> {
        if mode == PathMode::FullPath {
            let root = volume_root(path);
            let anchor = Anchor {
                scope: root.clone(),
                anchor: root,
                prefix: Some(fsutil::volume_folder_name(&fsutil::volume_key(path))),
            };
            return anchor.name_for(path);
        }

        self.anchors_for(mode)
            .iter()
            .find(|a| fsutil::contains(&a.scope, path))
            .and_then(|a| a.name_for(path))
    }

    /// Top-level folder names a mode would create, in order.
    fn top_names(&self, mode: PathMode) -> Vec<String> {
        self.anchors_for(mode).iter().map(Anchor::top_name).collect()
    }
}

/// Which modes can be used without two branches landing on the same name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeAvailability {
    pub folders_only: bool,
    pub common_root: bool,
    /// Names the collision, for the dialog to show.
    pub folders_only_reason: Option<String>,
    pub common_root_reason: Option<String>,
}

impl ModeAvailability {
    /// The mode to fall back to, preferring the least path noise available.
    pub fn best(&self, wanted: PathMode) -> PathMode {
        match wanted {
            PathMode::FoldersOnly if self.folders_only => PathMode::FoldersOnly,
            PathMode::FoldersOnly | PathMode::CommonRoot if self.common_root => {
                PathMode::CommonRoot
            }
            PathMode::CommonRoot | PathMode::FoldersOnly => PathMode::FullPath,
            PathMode::FullPath => PathMode::FullPath,
        }
    }
}

/// Two folders sharing a top-level name would silently merge into one
/// directory inside the archive, so a mode that produces a duplicate is
/// blocked rather than quietly losing files.
pub fn available_modes(ctx: &NamingContext) -> ModeAvailability {
    let folders_only_reason = first_duplicate(&ctx.top_names(PathMode::FoldersOnly))
        .map(|n| format!("two staged folders are both named \u{201c}{n}\u{201d}"));
    let common_root_reason = first_duplicate(&ctx.top_names(PathMode::CommonRoot))
        .map(|n| format!("two common roots are both named \u{201c}{n}\u{201d}"));

    ModeAvailability {
        folders_only: folders_only_reason.is_none(),
        common_root: common_root_reason.is_none(),
        folders_only_reason,
        common_root_reason,
    }
}

/// Compared case-insensitively: Windows would treat `Build` and `build` as one
/// directory on extraction even though the archive keeps them apart.
fn first_duplicate(names: &[String]) -> Option<String> {
    let mut seen: HashMap<String, &String> = HashMap::new();
    for n in names {
        let key = n.to_lowercase();
        if let Some(prev) = seen.insert(key, n) {
            return Some(prev.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{ScanDir, ScanFile};

    fn dir_source(path: &str) -> Source {
        Source {
            path: PathBuf::from(path),
            tree: SourceTree::Dir(ScanDir {
                name: safe_name(Path::new(path)),
                dirs: vec![],
                files: vec![],
            }),
        }
    }

    fn file_source(path: &str) -> Source {
        Source {
            path: PathBuf::from(path),
            tree: SourceTree::File(ScanFile {
                name: safe_name(Path::new(path)),
                size: 10,
            }),
        }
    }

    fn ctx(paths: &[&str]) -> NamingContext {
        let sources: Vec<Source> = paths.iter().map(|p| dir_source(p)).collect();
        NamingContext::from_sources(sources.iter())
    }

    fn name(ctx: &NamingContext, mode: PathMode, path: &str) -> Option<String> {
        ctx.entry_name(mode, Path::new(path))
    }

    // ---------------------------------------------------------------- safe_name

    #[test]
    fn a_volume_root_names_itself_without_punctuation() {
        assert_eq!(safe_name(Path::new(r"C:\")), "C");
        assert_eq!(safe_name(Path::new(r"\\server\share\")), "server_share");
        assert_eq!(safe_name(Path::new(r"C:\proj\app")), "app");
    }

    // ---------------------------------------------------------------- the reported bug

    /// The setup from the user's archive-plan.json: several folders under
    /// C:\Users\Nick plus one under C:\DOWN, so the common ancestor is the
    /// drive root itself.
    fn drive_root_ctx() -> NamingContext {
        ctx(&[
            r"C:\Users\Nick\.aws",
            r"C:\Users\Nick\.android",
            r"C:\DOWN\bd",
        ])
    }

    #[test]
    fn a_drive_root_common_ancestor_never_leaks_into_entry_names() {
        let c = drive_root_ctx();
        for mode in [PathMode::FoldersOnly, PathMode::CommonRoot, PathMode::FullPath] {
            for p in [
                r"C:\",
                r"C:\Users",
                r"C:\Users\Nick",
                r"C:\Users\Nick\.aws",
                r"C:\Users\Nick\.aws\config",
                r"C:\DOWN\bd",
            ] {
                if let Some(n) = name(&c, mode, p) {
                    assert!(!n.contains(':'), "{mode:?} produced {n:?} for {p}");
                    assert!(!n.contains('\\'), "{mode:?} produced {n:?} for {p}");
                    assert!(
                        !n.starts_with('/'),
                        "{mode:?} produced an absolute {n:?} for {p}"
                    );
                }
            }
        }
    }

    #[test]
    fn folders_only_drops_the_directories_above_the_sources() {
        let c = drive_root_ctx();
        // Nothing leads to an entry until a source is reached.
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\"), None);
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\Users"), None);
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\Users\Nick"), None);

        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\Users\Nick\.aws").as_deref(),
            Some(".aws")
        );
        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\Users\Nick\.aws\config").as_deref(),
            Some(".aws/config")
        );
        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\DOWN\bd\clip.mp4").as_deref(),
            Some("bd/clip.mp4")
        );
    }

    #[test]
    fn common_root_at_a_drive_root_uses_the_drive_name() {
        let c = drive_root_ctx();
        assert_eq!(name(&c, PathMode::CommonRoot, r"C:\").as_deref(), Some("C"));
        assert_eq!(
            name(&c, PathMode::CommonRoot, r"C:\Users\Nick\.aws").as_deref(),
            Some("C/Users/Nick/.aws")
        );
    }

    // ---------------------------------------------------------------- the three modes

    /// The example from the request: C:\A\B\{D,E} and D:\F\{G,H}.
    fn two_volume_ctx() -> NamingContext {
        ctx(&[r"C:\A\B\D", r"C:\A\B\E", r"D:\F\G", r"D:\F\H"])
    }

    #[test]
    fn folders_only_puts_each_staged_folder_at_the_top() {
        let c = two_volume_ctx();
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\A\B\D").as_deref(), Some("D"));
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\A\B\E").as_deref(), Some("E"));
        assert_eq!(name(&c, PathMode::FoldersOnly, r"D:\F\G").as_deref(), Some("G"));
        assert_eq!(name(&c, PathMode::FoldersOnly, r"D:\F\H").as_deref(), Some("H"));
        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\A\B\D\sub\x.txt").as_deref(),
            Some("D/sub/x.txt")
        );
        // The shared parents are not part of the archive.
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\A\B"), None);
    }

    #[test]
    fn common_root_puts_the_shared_folder_at_the_top() {
        let c = two_volume_ctx();
        assert_eq!(name(&c, PathMode::CommonRoot, r"C:\A\B").as_deref(), Some("B"));
        assert_eq!(name(&c, PathMode::CommonRoot, r"C:\A\B\D").as_deref(), Some("B/D"));
        assert_eq!(name(&c, PathMode::CommonRoot, r"D:\F\H").as_deref(), Some("F/H"));
        // No drive segment: that belongs to FullPath now.
        assert_eq!(name(&c, PathMode::CommonRoot, r"C:\A"), None);
    }

    #[test]
    fn full_path_puts_the_drive_at_the_top() {
        let c = two_volume_ctx();
        assert_eq!(name(&c, PathMode::FullPath, r"C:\A\B\D").as_deref(), Some("C/A/B/D"));
        assert_eq!(name(&c, PathMode::FullPath, r"D:\F\H").as_deref(), Some("D/F/H"));
        assert_eq!(name(&c, PathMode::FullPath, r"C:\").as_deref(), Some("C"));
        assert_eq!(
            name(&c, PathMode::FullPath, r"\\server\share\x\y").as_deref(),
            Some("server_share/x/y")
        );
    }

    #[test]
    fn a_single_source_reduces_to_its_own_folder() {
        let c = ctx(&[r"C:\proj"]);
        assert_eq!(name(&c, PathMode::FoldersOnly, r"C:\proj").as_deref(), Some("proj"));
        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\proj\src\main.rs").as_deref(),
            Some("proj/src/main.rs")
        );
        // With one source the common root is that source, so B matches A.
        assert_eq!(name(&c, PathMode::CommonRoot, r"C:\proj").as_deref(), Some("proj"));
    }

    #[test]
    fn a_dropped_file_is_named_by_itself() {
        let sources = [file_source(r"C:\proj\notes.txt")];
        let c = NamingContext::from_sources(sources.iter());
        assert_eq!(
            name(&c, PathMode::FoldersOnly, r"C:\proj\notes.txt").as_deref(),
            Some("notes.txt")
        );
        assert_eq!(
            name(&c, PathMode::FullPath, r"C:\proj\notes.txt").as_deref(),
            Some("C/proj/notes.txt")
        );
    }

    // ---------------------------------------------------------------- conflicts

    #[test]
    fn duplicate_source_names_block_folders_only() {
        // Two staged folders both called "build" would merge into one.
        let c = ctx(&[r"C:\proj\build", r"D:\work\build"]);
        let a = available_modes(&c);
        assert!(!a.folders_only);
        assert!(a.folders_only_reason.as_deref().unwrap().contains("build"));

        // With one source per volume the common root *is* that source, so the
        // same collision reaches B and only the full path stays unambiguous.
        assert!(!a.common_root);
        assert_eq!(a.best(PathMode::FoldersOnly), PathMode::FullPath);
    }

    #[test]
    fn a_distinct_common_root_rescues_colliding_folder_names() {
        // build/ and dist/ repeat across volumes, but the roots above them
        // differ, so B separates what A cannot.
        let c = ctx(&[
            r"C:\proj\build",
            r"C:\proj\dist",
            r"D:\work\build",
            r"D:\work\dist",
        ]);
        let a = available_modes(&c);
        assert!(!a.folders_only);
        assert!(a.common_root);
        assert_eq!(a.best(PathMode::FoldersOnly), PathMode::CommonRoot);
        assert_eq!(
            name(&c, PathMode::CommonRoot, r"C:\proj\build").as_deref(),
            Some("proj/build")
        );
        assert_eq!(
            name(&c, PathMode::CommonRoot, r"D:\work\build").as_deref(),
            Some("work/build")
        );
    }

    #[test]
    fn duplicate_common_roots_block_that_mode_too() {
        // Both volumes reduce to a common root named "shared".
        let c = ctx(&[
            r"C:\a\shared\one",
            r"C:\a\shared\two",
            r"D:\b\shared\three",
            r"D:\b\shared\four",
        ]);
        let a = available_modes(&c);
        assert!(a.folders_only, "the four leaf names are all distinct");
        assert!(!a.common_root);
        assert!(a.common_root_reason.as_deref().unwrap().contains("shared"));
        assert_eq!(a.best(PathMode::CommonRoot), PathMode::FullPath);
    }

    #[test]
    fn duplicates_are_caught_regardless_of_case() {
        let c = ctx(&[r"C:\x\Build", r"D:\y\build"]);
        assert!(!available_modes(&c).folders_only);
    }

    #[test]
    fn distinct_names_leave_every_mode_open() {
        let a = available_modes(&two_volume_ctx());
        assert!(a.folders_only && a.common_root);
        assert_eq!(a.best(PathMode::FoldersOnly), PathMode::FoldersOnly);
        assert_eq!(a.best(PathMode::CommonRoot), PathMode::CommonRoot);
        assert_eq!(a.best(PathMode::FullPath), PathMode::FullPath);
    }

    #[test]
    fn full_path_is_always_available() {
        let c = ctx(&[r"C:\proj\build", r"D:\work\build"]);
        assert_eq!(available_modes(&c).best(PathMode::FullPath), PathMode::FullPath);
    }
}
