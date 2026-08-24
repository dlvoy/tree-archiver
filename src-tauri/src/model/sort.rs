//! Child ordering. Sorting happens here so the frontend never has to.

use super::arena::{Arena, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    Name,
    Size,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKey {
    pub by: SortBy,
    pub dir: SortDir,
}

impl Default for SortKey {
    fn default() -> Self {
        SortKey {
            by: SortBy::Name,
            dir: SortDir::Asc,
        }
    }
}

/// Case-insensitive comparison that reads digit runs as numbers, so `file2`
/// sorts before `file10` instead of after it.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number(&mut ai);
                    let nb = take_number(&mut bi);
                    match na.cmp(&nb) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                }
                let la = ca.to_lowercase().next().unwrap_or(ca);
                let lb = cb.to_lowercase().next().unwrap_or(cb);
                match la.cmp(&lb) {
                    Ordering::Equal => {
                        ai.next();
                        bi.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

/// Consumes a run of digits and returns its value. Saturates rather than
/// wrapping, so absurdly long digit runs still compare sanely.
fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut v: u128 = 0;
    while let Some(c) = it.peek().copied() {
        if !c.is_ascii_digit() {
            break;
        }
        v = v.saturating_mul(10).saturating_add((c as u8 - b'0') as u128);
        it.next();
    }
    v
}

/// Orders a directory's children for display.
///
/// The `<files>` group always sinks to the bottom of its parent regardless of
/// sort key or direction, so the grouping stays visually anchored while the
/// real folders reorder around it.
pub fn sort_children(arena: &Arena, ids: &mut [NodeId], key: SortKey) {
    ids.sort_by(|&x, &y| {
        let nx = arena.node(x);
        let ny = arena.node(y);

        let gx = nx.kind == NodeKind::FilesGroup;
        let gy = ny.kind == NodeKind::FilesGroup;
        if gx != gy {
            return if gx { Ordering::Greater } else { Ordering::Less };
        }

        let ord = match key.by {
            SortBy::Name => natural_cmp(&nx.name, &ny.name),
            // Sort on disk truth, not selection, so rows do not jump around
            // while the user is ticking boxes.
            SortBy::Size => nx
                .total_size
                .cmp(&ny.total_size)
                .then_with(|| natural_cmp(&nx.name, &ny.name)),
            SortBy::Count => nx
                .total_items
                .cmp(&ny.total_items)
                .then_with(|| natural_cmp(&nx.name, &ny.name)),
        };

        match key.dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::arena::FILES_GROUP_NAME;

    #[test]
    fn digit_runs_compare_numerically() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file10", "file2"), Ordering::Greater);
        assert_eq!(natural_cmp("a1b2", "a1b2"), Ordering::Equal);
        assert_eq!(natural_cmp("img009", "img9"), Ordering::Equal);
    }

    #[test]
    fn comparison_ignores_case() {
        assert_eq!(natural_cmp("Apple", "apple"), Ordering::Equal);
        assert_eq!(natural_cmp("apple", "Banana"), Ordering::Less);
    }

    #[test]
    fn prefix_sorts_before_longer_name() {
        assert_eq!(natural_cmp("doc", "document"), Ordering::Less);
    }

    fn dir_fixture() -> (Arena, NodeId) {
        let mut a = Arena::new();
        let root = a.add(None, "root".into(), NodeKind::Dir { scanned: true });
        // Item counts deliberately disagree with the size ordering, so a
        // Count-mode test can't pass by accident of sharing Size's order.
        for (name, size, items) in [
            ("zeta", 10u64, 100u64),
            ("alpha", 500, 1),
            ("mid10", 50, 50),
            ("mid2", 90, 10),
        ] {
            let d = a.add(Some(root), name.into(), NodeKind::Dir { scanned: true });
            a.node_mut(d).own_size = size;
            a.node_mut(d).total_size = size;
            a.node_mut(d).total_items = items;
        }
        a.add(Some(root), FILES_GROUP_NAME.into(), NodeKind::FilesGroup);
        (a, root)
    }

    fn names(a: &Arena, ids: &[NodeId]) -> Vec<String> {
        ids.iter().map(|&i| a.node(i).name.clone()).collect()
    }

    #[test]
    fn name_sort_ascending_is_natural() {
        let (a, root) = dir_fixture();
        let mut ids = a.children(root).to_vec();
        sort_children(&a, &mut ids, SortKey { by: SortBy::Name, dir: SortDir::Asc });
        assert_eq!(names(&a, &ids), ["alpha", "mid2", "mid10", "zeta", FILES_GROUP_NAME]);
    }

    #[test]
    fn size_sort_descending_orders_by_total() {
        let (a, root) = dir_fixture();
        let mut ids = a.children(root).to_vec();
        sort_children(&a, &mut ids, SortKey { by: SortBy::Size, dir: SortDir::Desc });
        assert_eq!(names(&a, &ids), ["alpha", "mid2", "mid10", "zeta", FILES_GROUP_NAME]);
    }

    #[test]
    fn count_sort_ascending_orders_by_total_items() {
        let (a, root) = dir_fixture();
        let mut ids = a.children(root).to_vec();
        sort_children(&a, &mut ids, SortKey { by: SortBy::Count, dir: SortDir::Asc });
        assert_eq!(names(&a, &ids), ["alpha", "mid2", "mid10", "zeta", FILES_GROUP_NAME]);
    }

    #[test]
    fn count_sort_descending_still_sinks_the_files_group() {
        let (a, root) = dir_fixture();
        let mut ids = a.children(root).to_vec();
        sort_children(&a, &mut ids, SortKey { by: SortBy::Count, dir: SortDir::Desc });
        assert_eq!(names(&a, &ids), ["zeta", "mid10", "mid2", "alpha", FILES_GROUP_NAME]);
    }

    #[test]
    fn files_group_stays_last_when_reversed() {
        let (a, root) = dir_fixture();
        let mut ids = a.children(root).to_vec();
        sort_children(&a, &mut ids, SortKey { by: SortBy::Name, dir: SortDir::Desc });
        assert_eq!(names(&a, &ids).last().unwrap(), FILES_GROUP_NAME);
    }
}
