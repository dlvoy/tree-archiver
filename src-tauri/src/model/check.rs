//! Tri-state check propagation.
//!
//! Setting a node descends to rewrite its whole subtree, then ascends to
//! recompute each ancestor's selected totals and derived state. Cost is
//! O(subtree) down plus O(depth) up.

use super::arena::{Arena, CheckState, NodeId};

/// Applies `checked` to `id` and its entire subtree, then repairs ancestors.
/// Returns the ancestor ids whose displayed state may have changed, ordered
/// nearest-parent first.
pub fn set_checked(arena: &mut Arena, id: NodeId, checked: bool) -> Vec<NodeId> {
    let state = if checked {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    };

    for d in arena.descendants(id) {
        let n = arena.node_mut(d);
        n.check = state;
        n.sel_size = if checked { n.total_size } else { 0 };
        n.sel_files = if checked { n.total_files } else { 0 };
    }

    repair_ancestors(arena, id)
}

/// Recomputes selected totals and check state for every ancestor of `id`.
/// Returns them nearest-parent first.
pub fn repair_ancestors(arena: &mut Arena, id: NodeId) -> Vec<NodeId> {
    let chain: Vec<NodeId> = arena.ancestors(id).collect();
    for &a in &chain {
        recompute_from_children(arena, a);
    }
    chain
}

/// Rebuilds one node's `sel_size`, `sel_files` and `check` from its children.
/// A container with no children keeps whatever state it already had, so an
/// empty directory stays checkable in its own right.
pub fn recompute_from_children(arena: &mut Arena, id: NodeId) {
    let kids = arena.children(id).to_vec();
    if kids.is_empty() {
        return;
    }

    let mut sel_size = 0u64;
    let mut sel_files = 0u64;
    let mut all_checked = true;
    let mut all_unchecked = true;

    for k in kids {
        let n = arena.node(k);
        sel_size = sel_size.saturating_add(n.sel_size);
        sel_files += n.sel_files;
        match n.check {
            CheckState::Checked => all_unchecked = false,
            CheckState::Unchecked => all_checked = false,
            CheckState::Partial => {
                all_checked = false;
                all_unchecked = false;
            }
        }
    }

    let node = arena.node_mut(id);
    node.sel_size = sel_size;
    node.sel_files = sel_files;
    node.check = if all_checked {
        CheckState::Checked
    } else if all_unchecked {
        CheckState::Unchecked
    } else {
        CheckState::Partial
    };
}

/// Rebuilds selected totals and check state for the whole arena bottom-up,
/// preserving the states already stored on leaves. Used after applying a
/// loaded plan, where rules mark arbitrary nodes in arbitrary order.
pub fn recompute_all(arena: &mut Arena) {
    for i in (0..arena.len()).rev() {
        let id = i as NodeId;
        if arena.node(id).kind.is_file() {
            let n = arena.node_mut(id);
            let checked = n.check == CheckState::Checked;
            n.sel_size = if checked { n.total_size } else { 0 };
            n.sel_files = if checked { 1 } else { 0 };
        } else {
            recompute_from_children(arena, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::arena::{NodeKind, FILES_GROUP_NAME};

    /// root
    ///   docs (dir)
    ///     <files>
    ///       a.txt (100)
    ///       b.txt (200)
    ///   img (dir)
    ///     <files>
    ///       c.png (400)
    struct Fx {
        arena: Arena,
        root: NodeId,
        docs: NodeId,
        docs_grp: NodeId,
        a: NodeId,
        b: NodeId,
        img: NodeId,
        c: NodeId,
    }

    fn fixture() -> Fx {
        let mut arena = Arena::new();
        let root = arena.add(None, "root".into(), NodeKind::Dir { scanned: true });
        let docs = arena.add(Some(root), "docs".into(), NodeKind::Dir { scanned: true });
        let docs_grp = arena.add(Some(docs), FILES_GROUP_NAME.into(), NodeKind::FilesGroup);
        let a = arena.add(Some(docs_grp), "a.txt".into(), NodeKind::File);
        let b = arena.add(Some(docs_grp), "b.txt".into(), NodeKind::File);
        let img = arena.add(Some(root), "img".into(), NodeKind::Dir { scanned: true });
        let img_grp = arena.add(Some(img), FILES_GROUP_NAME.into(), NodeKind::FilesGroup);
        let c = arena.add(Some(img_grp), "c.png".into(), NodeKind::File);
        arena.node_mut(a).own_size = 100;
        arena.node_mut(b).own_size = 200;
        arena.node_mut(c).own_size = 400;
        arena.recompute_totals();
        recompute_all(&mut arena);
        Fx { arena, root, docs, docs_grp, a, b, img, c }
    }

    #[test]
    fn everything_starts_checked_and_fully_selected() {
        let f = fixture();
        assert_eq!(f.arena.node(f.root).check, CheckState::Checked);
        assert_eq!(f.arena.node(f.root).sel_size, 700);
        assert_eq!(f.arena.node(f.root).sel_files, 3);
    }

    #[test]
    fn unchecking_a_folder_clears_its_whole_subtree() {
        let mut f = fixture();
        set_checked(&mut f.arena, f.docs, false);

        for id in [f.docs, f.docs_grp, f.a, f.b] {
            assert_eq!(f.arena.node(id).check, CheckState::Unchecked);
            assert_eq!(f.arena.node(id).sel_size, 0);
        }
        // The unrelated branch is untouched.
        assert_eq!(f.arena.node(f.c).check, CheckState::Checked);
    }

    #[test]
    fn partial_state_propagates_up() {
        let mut f = fixture();
        set_checked(&mut f.arena, f.a, false);

        assert_eq!(f.arena.node(f.docs_grp).check, CheckState::Partial);
        assert_eq!(f.arena.node(f.docs).check, CheckState::Partial);
        assert_eq!(f.arena.node(f.root).check, CheckState::Partial);
        assert_eq!(f.arena.node(f.docs).sel_size, 200);
        assert_eq!(f.arena.node(f.root).sel_size, 600);
        assert_eq!(f.arena.node(f.root).sel_files, 2);
        // total_size is disk truth and must not move.
        assert_eq!(f.arena.node(f.root).total_size, 700);
    }

    #[test]
    fn unchecking_files_group_unchecks_every_file_under_it() {
        let mut f = fixture();
        set_checked(&mut f.arena, f.docs_grp, false);
        assert_eq!(f.arena.node(f.a).check, CheckState::Unchecked);
        assert_eq!(f.arena.node(f.b).check, CheckState::Unchecked);
        // docs has no other children, so it collapses to Unchecked too.
        assert_eq!(f.arena.node(f.docs).check, CheckState::Unchecked);
    }

    #[test]
    fn rechecking_restores_full_selection() {
        let mut f = fixture();
        set_checked(&mut f.arena, f.root, false);
        assert_eq!(f.arena.node(f.root).sel_size, 0);
        set_checked(&mut f.arena, f.root, true);
        assert_eq!(f.arena.node(f.root).sel_size, 700);
        assert_eq!(f.arena.node(f.root).check, CheckState::Checked);
        assert_eq!(f.arena.node(f.img).check, CheckState::Checked);
    }

    #[test]
    fn ancestors_returned_nearest_first() {
        let mut f = fixture();
        let touched = set_checked(&mut f.arena, f.a, false);
        assert_eq!(touched, vec![f.docs_grp, f.docs, f.root]);
    }
}
