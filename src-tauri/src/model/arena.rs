//! Arena-backed tree. Nodes live in a `Vec` and reference each other by index,
//! so walking parent chains during check propagation stays borrow-free.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type NodeId = u32;

/// Name shown for the pseudo-folder that groups a directory's direct files.
pub const FILES_GROUP_NAME: &str = "<files>";
/// Name shown for the synthetic root used when sources span several volumes.
pub const SYNTHETIC_ROOT_NAME: &str = "Sources";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NodeKind {
    /// A real directory. `scanned == false` marks a *spine* node: a directory
    /// that exists only to connect the common root to an added source. Its
    /// children are the added subtrees, never an enumeration of what is on disk.
    Dir { scanned: bool },
    File,
    /// The `<files>` pseudo-folder. Holds every direct file child of a `Dir`.
    FilesGroup,
    /// Only present when sources span more than one volume.
    SyntheticRoot,
}

impl NodeKind {
    pub fn is_dir(&self) -> bool {
        matches!(self, NodeKind::Dir { .. })
    }
    pub fn is_file(&self) -> bool {
        matches!(self, NodeKind::File)
    }
    /// True for anything that can hold children and is drawn with a twisty.
    pub fn is_container(&self) -> bool {
        !matches!(self, NodeKind::File)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    Checked,
    Unchecked,
    Partial,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub name: String,
    pub kind: NodeKind,
    /// `None` for `FilesGroup` and `SyntheticRoot`, which have no path on disk.
    pub path: Option<PathBuf>,
    pub children: Vec<NodeId>,
    /// Size of this file itself; always 0 for containers.
    pub own_size: u64,
    /// Recursive byte total of everything beneath this node. Disk truth: it
    /// never changes when boxes are checked, so size sorting stays stable.
    pub total_size: u64,
    /// Recursive byte total of the `Checked` portion.
    pub sel_size: u64,
    pub total_files: u64,
    pub sel_files: u64,
    pub check: CheckState,
    /// Lowercased extension, drives icon choice.
    pub ext: Option<String>,
}

impl Node {
    fn new(id: NodeId, parent: Option<NodeId>, name: String, kind: NodeKind) -> Self {
        Node {
            id,
            parent,
            name,
            kind,
            path: None,
            children: Vec::new(),
            own_size: 0,
            total_size: 0,
            sel_size: 0,
            total_files: 0,
            sel_files: 0,
            check: CheckState::Checked,
            ext: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct Arena {
    nodes: Vec<Node>,
    /// Absolute path -> node, for re-rooting and rule application. Excludes
    /// pathless nodes (`FilesGroup`, `SyntheticRoot`).
    by_path: HashMap<PathBuf, NodeId>,
}

impl Arena {
    pub fn new() -> Self {
        Arena::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// Panics if `id` is out of range. Used where the id provably came from
    /// this arena.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn find_by_path(&self, path: &Path) -> Option<NodeId> {
        self.by_path.get(path).copied()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id as usize].children
    }

    /// Creates a node and appends it to `parent`'s child list.
    pub fn add(&mut self, parent: Option<NodeId>, name: String, kind: NodeKind) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node::new(id, parent, name, kind));
        if let Some(p) = parent {
            self.nodes[p as usize].children.push(id);
        }
        id
    }

    /// Registers a node's on-disk path in the lookup index.
    pub fn set_path(&mut self, id: NodeId, path: PathBuf) {
        self.by_path.insert(path.clone(), id);
        self.nodes[id as usize].path = Some(path);
    }

    /// Walks from `id` up to the root, `id` itself excluded.
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            arena: self,
            next: self.nodes[id as usize].parent,
        }
    }

    /// Depth-first pre-order walk of the subtree rooted at `id`, `id` included.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.nodes[n as usize].children.iter().rev().copied());
        }
        out
    }

    /// The `FilesGroup` child of `id`, if the directory has one.
    pub fn files_group_of(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id as usize]
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes[c as usize].kind == NodeKind::FilesGroup)
    }

    /// Recomputes `total_size`/`total_files` bottom-up across the whole arena.
    /// Ids are assigned parent-before-child, so one reverse pass suffices.
    pub fn recompute_totals(&mut self) {
        for i in (0..self.nodes.len()).rev() {
            if self.nodes[i].kind.is_file() {
                self.nodes[i].total_size = self.nodes[i].own_size;
                self.nodes[i].total_files = 1;
                continue;
            }
            let mut size = 0u64;
            let mut files = 0u64;
            for idx in 0..self.nodes[i].children.len() {
                let c = self.nodes[i].children[idx] as usize;
                size = size.saturating_add(self.nodes[c].total_size);
                files += self.nodes[c].total_files;
            }
            self.nodes[i].total_size = size;
            self.nodes[i].total_files = files;
        }
    }
}

pub struct Ancestors<'a> {
    arena: &'a Arena,
    next: Option<NodeId>,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.arena.nodes[cur as usize].parent;
        Some(cur)
    }
}

/// Lowercased extension of a file name, or `None` when there isn't one.
pub fn extension_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None;
    }
    Some(name[dot + 1..].to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_extraction() {
        assert_eq!(extension_of("a.TXT").as_deref(), Some("txt"));
        assert_eq!(extension_of("archive.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of("noext"), None);
        // A leading dot is a hidden file, not an extension.
        assert_eq!(extension_of(".gitignore"), None);
        // A trailing dot leaves nothing to read.
        assert_eq!(extension_of("trailing."), None);
    }

    #[test]
    fn totals_roll_up_bottom_up() {
        let mut a = Arena::new();
        let root = a.add(None, "root".into(), NodeKind::Dir { scanned: true });
        let sub = a.add(Some(root), "sub".into(), NodeKind::Dir { scanned: true });
        let grp = a.add(Some(sub), FILES_GROUP_NAME.into(), NodeKind::FilesGroup);
        let f1 = a.add(Some(grp), "a.bin".into(), NodeKind::File);
        let f2 = a.add(Some(grp), "b.bin".into(), NodeKind::File);
        a.node_mut(f1).own_size = 100;
        a.node_mut(f2).own_size = 50;
        a.recompute_totals();

        assert_eq!(a.node(root).total_size, 150);
        assert_eq!(a.node(root).total_files, 2);
        assert_eq!(a.node(grp).total_size, 150);
        assert_eq!(a.node(sub).total_files, 2);
    }

    #[test]
    fn ancestors_walk_to_root() {
        let mut a = Arena::new();
        let root = a.add(None, "root".into(), NodeKind::Dir { scanned: true });
        let mid = a.add(Some(root), "mid".into(), NodeKind::Dir { scanned: true });
        let leaf = a.add(Some(mid), "leaf".into(), NodeKind::Dir { scanned: true });
        assert_eq!(a.ancestors(leaf).collect::<Vec<_>>(), vec![mid, root]);
        assert_eq!(a.ancestors(root).count(), 0);
    }

    #[test]
    fn descendants_are_pre_order() {
        let mut a = Arena::new();
        let root = a.add(None, "root".into(), NodeKind::Dir { scanned: true });
        let x = a.add(Some(root), "x".into(), NodeKind::Dir { scanned: true });
        let y = a.add(Some(root), "y".into(), NodeKind::Dir { scanned: true });
        let x1 = a.add(Some(x), "x1".into(), NodeKind::Dir { scanned: true });
        assert_eq!(a.descendants(root), vec![root, x, x1, y]);
    }
}
