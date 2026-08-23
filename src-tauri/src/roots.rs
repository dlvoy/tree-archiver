//! Source bookkeeping and arena (re)construction.
//!
//! The tree root is always the topmost folder common to every added source.
//! That means the arena is rebuilt from scratch on each add and remove; the
//! scanned `Source` trees are the durable data, the arena is a projection of
//! them. Check state is carried across a rebuild by absolute path.

use crate::fsutil;
use crate::model::arena::{
    extension_of, Arena, CheckState, NodeId, NodeKind, FILES_GROUP_NAME, SYNTHETIC_ROOT_NAME,
};
use crate::model::check;
use crate::scan::{file_name_of, ScanDir, Source, SourceTree};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Check states worth carrying across a rebuild: files and childless
/// containers. Every other node's state is derived from its children.
pub type CheckSnapshot = HashMap<PathBuf, CheckState>;

#[derive(Debug, Default)]
pub struct Sources(Vec<Source>);

impl Sources {
    pub fn new() -> Self {
        Sources(Vec::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Source> {
        self.0.iter()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.0.iter().map(|s| s.path.clone()).collect()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// True when an existing source already covers `path`.
    pub fn covers(&self, path: &Path) -> bool {
        self.0.iter().any(|s| fsutil::contains(&s.path, path))
    }

    /// Adds a source, keeping the set mutually non-containing: a new source
    /// that already sits inside an existing one is dropped, and one that
    /// contains existing sources absorbs them.
    pub fn add(&mut self, source: Source) -> bool {
        if self.covers(&source.path) {
            return false;
        }
        self.0.retain(|s| !fsutil::contains(&source.path, &s.path));
        self.0.push(source);
        true
    }

    pub fn remove_path(&mut self, path: &Path) -> bool {
        let before = self.0.len();
        self.0.retain(|s| s.path != path);
        self.0.len() != before
    }
}

/// Every node whose state cannot be re-derived from children.
pub fn snapshot_checks(arena: &Arena) -> CheckSnapshot {
    let mut map = HashMap::new();
    for i in 0..arena.len() {
        let n = arena.node(i as NodeId);
        let leafish = n.kind.is_file() || n.children.is_empty();
        if leafish {
            if let Some(p) = &n.path {
                map.insert(p.clone(), n.check);
            }
        }
    }
    map
}

pub struct BuildResult {
    pub root: Option<NodeId>,
}

/// Rebuilds `arena` from `sources`, restoring check state from `prior`.
///
/// Sources on different volumes have no shared ancestor, so they get a
/// synthetic root holding one subtree per volume.
pub fn rebuild(arena: &mut Arena, sources: &Sources, prior: &CheckSnapshot) -> BuildResult {
    *arena = Arena::new();
    if sources.is_empty() {
        return BuildResult { root: None };
    }

    // Group by volume, preserving first-seen order for stable display.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&Source>> = HashMap::new();
    for s in sources.iter() {
        let key = fsutil::volume_key(&s.path);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(s);
    }

    let root = if order.len() == 1 {
        build_group(arena, None, &groups[&order[0]])
    } else {
        let synthetic = arena.add(None, SYNTHETIC_ROOT_NAME.into(), NodeKind::SyntheticRoot);
        for key in &order {
            build_group(arena, Some(synthetic), &groups[key]);
        }
        synthetic
    };

    arena.recompute_totals();
    restore_checks(arena, prior);
    check::recompute_all(arena);

    BuildResult { root: Some(root) }
}

/// Builds one volume's subtree and returns its root node.
fn build_group(arena: &mut Arena, parent: Option<NodeId>, group: &[&Source]) -> NodeId {
    let root_path = group_root_path(group);

    let root_kind = if group.len() == 1 && group[0].path == root_path {
        // The single source *is* the root, so it is fully enumerated.
        NodeKind::Dir { scanned: true }
    } else {
        NodeKind::Dir { scanned: false }
    };
    let root = arena.add(parent, file_name_of(&root_path), root_kind);
    arena.set_path(root, root_path.clone());

    for s in group {
        attach_source(arena, root, &root_path, s);
    }
    root
}

/// The common ancestor of a volume group. A lone file source roots at its
/// parent directory, since a file cannot hold the tree.
fn group_root_path(group: &[&Source]) -> PathBuf {
    let mut paths: Vec<PathBuf> = Vec::with_capacity(group.len());
    for s in group {
        match &s.tree {
            SourceTree::File(_) => {
                paths.push(s.path.parent().map(Path::to_path_buf).unwrap_or_else(|| s.path.clone()))
            }
            SourceTree::Dir(_) => paths.push(s.path.clone()),
        }
    }
    fsutil::common_ancestor(&paths).unwrap_or_else(|| paths[0].clone())
}

/// Walks the spine from `root` down to `source`, creating pass-through
/// directories, then grafts the scanned subtree at the end.
fn attach_source(arena: &mut Arena, root: NodeId, root_path: &Path, source: &Source) {
    let host_path = match &source.tree {
        SourceTree::File(_) => source
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| source.path.clone()),
        SourceTree::Dir(_) => source.path.clone(),
    };

    let rel = host_path.strip_prefix(root_path).unwrap_or(Path::new(""));
    let mut cur = root;
    let mut cur_path = root_path.to_path_buf();

    for comp in rel.components() {
        let name = comp.as_os_str().to_string_lossy().into_owned();
        cur_path = cur_path.join(&name);
        cur = match arena.find_by_path(&cur_path) {
            Some(existing) => existing,
            None => {
                // Spine node: it exists only to reach a source, so its
                // contents are deliberately not enumerated.
                let id = arena.add(Some(cur), name, NodeKind::Dir { scanned: false });
                arena.set_path(id, cur_path.clone());
                id
            }
        };
    }

    match &source.tree {
        SourceTree::Dir(d) => {
            // The host node is now fully enumerated rather than a spine.
            arena.node_mut(cur).kind = NodeKind::Dir { scanned: true };
            graft_dir(arena, cur, &cur_path, d);
        }
        SourceTree::File(f) => {
            let grp = match arena.files_group_of(cur) {
                Some(g) => g,
                None => arena.add(Some(cur), FILES_GROUP_NAME.into(), NodeKind::FilesGroup),
            };
            let fid = arena.add(Some(grp), f.name.clone(), NodeKind::File);
            arena.node_mut(fid).own_size = f.size;
            arena.node_mut(fid).ext = extension_of(&f.name);
            arena.set_path(fid, cur_path.join(&f.name));
        }
    }
}

/// Copies a scanned directory tree into the arena beneath `at`.
fn graft_dir(arena: &mut Arena, at: NodeId, at_path: &Path, dir: &ScanDir) {
    let mut stack: Vec<(&ScanDir, NodeId, PathBuf)> = vec![(dir, at, at_path.to_path_buf())];

    while let Some((sd, nid, npath)) = stack.pop() {
        // Requirement: files are never direct children of a directory. A
        // directory with at least one file gets exactly one `<files>` group.
        if !sd.files.is_empty() {
            let grp = arena.add(Some(nid), FILES_GROUP_NAME.into(), NodeKind::FilesGroup);
            for f in &sd.files {
                let fid = arena.add(Some(grp), f.name.clone(), NodeKind::File);
                arena.node_mut(fid).own_size = f.size;
                arena.node_mut(fid).ext = extension_of(&f.name);
                arena.set_path(fid, npath.join(&f.name));
            }
        }
        for cd in &sd.dirs {
            let cpath = npath.join(&cd.name);
            let cid = arena.add(Some(nid), cd.name.clone(), NodeKind::Dir { scanned: true });
            arena.set_path(cid, cpath.clone());
            stack.push((cd, cid, cpath));
        }
    }
}

/// Reapplies remembered check states. Anything not in the snapshot keeps the
/// default of `Checked`, which is what makes a freshly added folder arrive
/// fully selected.
fn restore_checks(arena: &mut Arena, prior: &CheckSnapshot) {
    if prior.is_empty() {
        return;
    }
    for i in 0..arena.len() {
        let id = i as NodeId;
        let (leafish, path) = {
            let n = arena.node(id);
            (n.kind.is_file() || n.children.is_empty(), n.path.clone())
        };
        if !leafish {
            continue;
        }
        if let Some(p) = path {
            if let Some(&state) = prior.get(&p) {
                arena.node_mut(id).check = state;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ScanFile;

    fn sdir(name: &str, dirs: Vec<ScanDir>, files: Vec<(&str, u64)>) -> ScanDir {
        ScanDir {
            name: name.into(),
            dirs,
            files: files
                .into_iter()
                .map(|(n, s)| ScanFile {
                    name: n.into(),
                    size: s,
                })
                .collect(),
        }
    }

    fn dir_source(path: &str, tree: ScanDir) -> Source {
        Source {
            path: PathBuf::from(path),
            tree: SourceTree::Dir(tree),
        }
    }

    fn file_source(path: &str, size: u64) -> Source {
        let name = file_name_of(Path::new(path));
        Source {
            path: PathBuf::from(path),
            tree: SourceTree::File(ScanFile { name, size }),
        }
    }

    fn build(sources: &Sources) -> (Arena, NodeId) {
        let mut arena = Arena::new();
        let r = rebuild(&mut arena, sources, &CheckSnapshot::new());
        let root = r.root.expect("expected a root");
        (arena, root)
    }

    fn child_named(arena: &Arena, parent: NodeId, name: &str) -> NodeId {
        arena
            .children(parent)
            .iter()
            .copied()
            .find(|&c| arena.node(c).name == name)
            .unwrap_or_else(|| panic!("no child {name:?}"))
    }

    #[test]
    fn single_source_roots_at_itself() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![("a.txt", 10)])));
        let (arena, root) = build(&s);

        assert_eq!(arena.node(root).name, "proj");
        assert_eq!(arena.node(root).kind, NodeKind::Dir { scanned: true });
        assert_eq!(arena.node(root).total_size, 10);
    }

    #[test]
    fn two_siblings_reroot_to_their_parent() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj\app", sdir("app", vec![], vec![("a", 100)])));
        s.add(dir_source(r"C:\proj\docs", sdir("docs", vec![], vec![("b", 200)])));
        let (arena, root) = build(&s);

        assert_eq!(arena.node(root).name, "proj");
        // The common root was never scanned, so it is a spine node.
        assert_eq!(arena.node(root).kind, NodeKind::Dir { scanned: false });
        assert_eq!(arena.children(root).len(), 2);
        assert_eq!(arena.node(root).total_size, 300);
    }

    #[test]
    fn spine_holds_only_added_branches() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj\deep\nested\app", sdir("app", vec![], vec![("a", 1)])));
        s.add(dir_source(r"C:\proj\other", sdir("other", vec![], vec![("b", 1)])));
        let (arena, root) = build(&s);

        assert_eq!(arena.node(root).name, "proj");
        let deep = child_named(&arena, root, "deep");
        assert_eq!(arena.node(deep).kind, NodeKind::Dir { scanned: false });
        // "deep" leads only to the added branch; no siblings appear.
        assert_eq!(arena.children(deep).len(), 1);
        let nested = child_named(&arena, deep, "nested");
        let app = child_named(&arena, nested, "app");
        assert_eq!(arena.node(app).kind, NodeKind::Dir { scanned: true });
    }

    #[test]
    fn sources_on_different_volumes_get_a_synthetic_root() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![("a", 100)])));
        s.add(dir_source(r"D:\media", sdir("media", vec![], vec![("b", 200)])));
        let (arena, root) = build(&s);

        assert_eq!(arena.node(root).kind, NodeKind::SyntheticRoot);
        assert_eq!(arena.node(root).name, SYNTHETIC_ROOT_NAME);
        assert_eq!(arena.children(root).len(), 2);
        assert_eq!(arena.node(root).total_size, 300);
    }

    #[test]
    fn nested_source_is_absorbed_by_its_ancestor() {
        let mut s = Sources::new();
        assert!(s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![]))));
        // Already covered, so it is dropped rather than duplicated.
        assert!(!s.add(dir_source(r"C:\proj\app", sdir("app", vec![], vec![]))));
        assert_eq!(s.paths().len(), 1);
    }

    #[test]
    fn adding_an_ancestor_absorbs_existing_sources() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj\app", sdir("app", vec![], vec![])));
        s.add(dir_source(r"C:\proj\docs", sdir("docs", vec![], vec![])));
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![])));
        assert_eq!(s.paths(), vec![PathBuf::from(r"C:\proj")]);
    }

    #[test]
    fn files_are_grouped_under_the_pseudo_folder() {
        let mut s = Sources::new();
        s.add(dir_source(
            r"C:\proj",
            sdir("proj", vec![sdir("sub", vec![], vec![])], vec![("a.txt", 10), ("b.txt", 20)]),
        ));
        let (arena, root) = build(&s);

        let grp = child_named(&arena, root, FILES_GROUP_NAME);
        assert_eq!(arena.node(grp).kind, NodeKind::FilesGroup);
        assert_eq!(arena.children(grp).len(), 2);
        // No file is ever a direct child of a directory.
        assert!(arena
            .children(root)
            .iter()
            .all(|&c| !arena.node(c).kind.is_file()));
        // A directory with no files gets no group.
        let sub = child_named(&arena, root, "sub");
        assert!(arena.files_group_of(sub).is_none());
    }

    #[test]
    fn a_dropped_file_roots_at_its_parent() {
        let mut s = Sources::new();
        s.add(file_source(r"C:\proj\solo.dat", 42));
        let (arena, root) = build(&s);

        assert_eq!(arena.node(root).name, "proj");
        let grp = child_named(&arena, root, FILES_GROUP_NAME);
        let f = child_named(&arena, grp, "solo.dat");
        assert_eq!(arena.node(f).own_size, 42);
        assert_eq!(arena.node(f).ext.as_deref(), Some("dat"));
    }

    #[test]
    fn everything_added_starts_checked() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![("a", 10)])));
        let (arena, root) = build(&s);
        assert_eq!(arena.node(root).check, CheckState::Checked);
        assert_eq!(arena.node(root).sel_size, 10);
    }

    #[test]
    fn check_state_survives_rerooting() {
        let mut s = Sources::new();
        s.add(dir_source(
            r"C:\proj\app",
            sdir("app", vec![], vec![("keep.txt", 100), ("drop.txt", 200)]),
        ));
        let mut arena = Arena::new();
        rebuild(&mut arena, &s, &CheckSnapshot::new());

        let dropped = arena.find_by_path(Path::new(r"C:\proj\app\drop.txt")).unwrap();
        check::set_checked(&mut arena, dropped, false);
        let snap = snapshot_checks(&arena);

        // Adding a sibling re-roots the tree from C:\proj\app to C:\proj.
        s.add(dir_source(r"C:\proj\docs", sdir("docs", vec![], vec![("c", 1)])));
        let r = rebuild(&mut arena, &s, &snap);
        let root = r.root.unwrap();

        assert_eq!(arena.node(root).name, "proj");
        let still_dropped = arena.find_by_path(Path::new(r"C:\proj\app\drop.txt")).unwrap();
        assert_eq!(arena.node(still_dropped).check, CheckState::Unchecked);
        // 100 kept + 1 from the new source; the 200 stays excluded.
        assert_eq!(arena.node(root).sel_size, 101);
        assert_eq!(arena.node(root).total_size, 301);
    }

    #[test]
    fn removing_the_last_source_empties_the_tree() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![])));
        assert!(s.remove_path(Path::new(r"C:\proj")));
        let mut arena = Arena::new();
        let r = rebuild(&mut arena, &s, &CheckSnapshot::new());
        assert!(r.root.is_none());
        assert!(arena.is_empty());
    }
}
