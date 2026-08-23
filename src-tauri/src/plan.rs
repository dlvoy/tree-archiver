//! The archive plan: a saved, reusable description of what to include.
//!
//! The baseline is "everything under `sources`"; `rules` subtract from it.
//! Unchecking a folder therefore saves as a *single* rule covering that whole
//! branch — the plan never enumerates the children it is dropping.

use crate::fsutil;
use crate::model::arena::{Arena, CheckState, NodeId, NodeKind};
use crate::model::check;
use crate::model::sort::SortKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PLAN_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// The node and everything beneath it.
    Tree,
    /// Only the direct file children of a directory — its `<files>` group.
    /// Subdirectories are unaffected.
    Files,
    /// One single file.
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Include,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Forward-slash path relative to the plan root, root itself excluded.
    pub path: String,
    pub scope: Scope,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Compression {
    None,
    Gzip,
}

impl Compression {
    pub fn extension(&self) -> &'static str {
        match self {
            Compression::None => "tar",
            Compression::Gzip => "tar.gz",
        }
    }
}

/// How much of a file's original path is kept inside the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PathMode {
    /// Each staged folder sits at the top of the archive, under its own name.
    #[default]
    FoldersOnly,
    /// The folder every staged path shares sits at the top.
    CommonRoot,
    /// The whole path is kept, with the drive or share at the top.
    FullPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputOptions {
    pub compression: Compression,
    pub gzip_level: u32,
    /// Absent in v1 plans, which predate the setting.
    #[serde(default)]
    pub path_mode: PathMode,
}

impl Default for OutputOptions {
    fn default() -> Self {
        OutputOptions {
            compression: Compression::None,
            gzip_level: 6,
            path_mode: PathMode::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePlan {
    pub version: u32,
    pub generator: String,
    pub created: String,
    /// Absolute path of the tree root. `None` when sources span volumes and
    /// the root is synthetic.
    pub root: Option<String>,
    pub sources: Vec<String>,
    pub sort: SortKey,
    pub output: OutputOptions,
    pub rules: Vec<Rule>,
}

/// Path of `id` relative to the tree root, root itself excluded, using forward
/// slashes. The root returns an empty string.
///
/// Under a synthetic root each volume becomes an extra leading segment, so
/// `C:\proj\app` and `D:\proj\app` stay distinguishable as `C/proj/app` and
/// `D/proj/app`.
pub fn rel_from_root(arena: &Arena, root: NodeId, id: NodeId) -> String {
    if id == root {
        return String::new();
    }
    // Segments are collected leaf-to-root and reversed at the end, so within
    // one node the name is pushed before its volume prefix.
    let mut segments: Vec<String> = Vec::new();
    let mut cur = id;
    while cur != root {
        let node = arena.node(cur);
        // The `<files>` group is a display-only grouping. It has no path on
        // disk and must never appear in a rule or an archive entry.
        if node.kind != NodeKind::FilesGroup {
            segments.push(node.name.clone());
            let parent_is_synthetic = node
                .parent
                .map(|p| arena.node(p).kind == NodeKind::SyntheticRoot)
                .unwrap_or(false);
            if parent_is_synthetic {
                if let Some(p) = &node.path {
                    segments.push(fsutil::volume_folder_name(&fsutil::volume_key(p)));
                }
            }
        }
        match node.parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    segments.reverse();
    segments.join("/")
}


/// Reduces the tree's check state to the smallest rule set that reproduces it.
///
/// An `Unchecked` folder emits one `Tree` rule and the walk stops there. That
/// is the whole point: `folder/*`, not a list of everything inside it.
///
/// Only exclusions are produced. Tri-state semantics make a folder `Unchecked`
/// only when every descendant is unchecked, so a kept item can never sit
/// inside a dropped branch and no re-include is ever needed.
pub fn compact(arena: &Arena, root: NodeId) -> Vec<Rule> {
    let mut out = Vec::new();
    walk(arena, root, root, &mut out);
    out
}

fn walk(arena: &Arena, root: NodeId, id: NodeId, out: &mut Vec<Rule>) {
    let node = arena.node(id);
    match node.check {
        // Already covered by the baseline; nothing to say.
        CheckState::Checked => {}
        CheckState::Unchecked => {
            let rule = match node.kind {
                NodeKind::FilesGroup => node.parent.map(|p| Rule {
                    path: rel_from_root(arena, root, p),
                    scope: Scope::Files,
                    action: Action::Exclude,
                }),
                NodeKind::File => Some(Rule {
                    path: rel_from_root(arena, root, id),
                    scope: Scope::File,
                    action: Action::Exclude,
                }),
                _ => Some(Rule {
                    path: rel_from_root(arena, root, id),
                    scope: Scope::Tree,
                    action: Action::Exclude,
                }),
            };
            if let Some(r) = rule {
                out.push(r);
            }
            // Deliberately no descent.
        }
        CheckState::Partial => {
            for &c in arena.children(id) {
                walk(arena, root, c, out);
            }
        }
    }
}

/// A rule that referred to something no longer in the tree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedRule {
    pub path: String,
    pub reason: String,
}

/// Applies `rules` to a freshly built tree: everything starts included, then
/// each rule is applied in order with later rules winning.
pub fn apply(arena: &mut Arena, root: NodeId, rules: &[Rule]) -> Vec<UnresolvedRule> {
    for d in arena.descendants(root) {
        arena.node_mut(d).check = CheckState::Checked;
    }

    // A `<files>` group shares its parent directory's relative path, so it is
    // left out of the index; `Scope::Files` reaches it through the parent.
    let mut index: HashMap<String, NodeId> = HashMap::new();
    for d in arena.descendants(root) {
        if arena.node(d).kind == NodeKind::FilesGroup {
            continue;
        }
        index.insert(rel_from_root(arena, root, d), d);
    }

    let mut unresolved = Vec::new();
    for rule in rules {
        let Some(&target) = index.get(&rule.path) else {
            unresolved.push(UnresolvedRule {
                path: rule.path.clone(),
                reason: "path is not in the scanned tree".into(),
            });
            continue;
        };
        let checked = rule.action == Action::Include;

        match rule.scope {
            Scope::Tree => set_subtree(arena, target, checked),
            Scope::File => arena.node_mut(target).check = state_of(checked),
            Scope::Files => match arena.files_group_of(target) {
                Some(g) => set_subtree(arena, g, checked),
                None => unresolved.push(UnresolvedRule {
                    path: rule.path.clone(),
                    reason: "directory has no direct files".into(),
                }),
            },
        }
    }

    check::recompute_all(arena);
    unresolved
}

fn state_of(checked: bool) -> CheckState {
    if checked {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    }
}

fn set_subtree(arena: &mut Arena, id: NodeId, checked: bool) {
    let state = state_of(checked);
    for d in arena.descendants(id) {
        arena.node_mut(d).check = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::arena::FILES_GROUP_NAME;
    use crate::roots::{rebuild, snapshot_checks, CheckSnapshot, Sources};
    use crate::scan::{ScanDir, ScanFile, Source, SourceTree};
    use std::path::{Path, PathBuf};

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

    /// C:\proj
    ///   app/  src/{main.rs, lib.rs}  target/{big.o, deep/huge.o}  README.md
    ///   docs/ notes.md
    fn fixture() -> (Arena, NodeId, Sources) {
        let mut s = Sources::new();
        s.add(dir_source(
            r"C:\proj\app",
            sdir(
                "app",
                vec![
                    sdir("src", vec![], vec![("main.rs", 100), ("lib.rs", 200)]),
                    sdir(
                        "target",
                        vec![sdir("deep", vec![], vec![("huge.o", 8000)])],
                        vec![("big.o", 4000)],
                    ),
                ],
                vec![("README.md", 50)],
            ),
        ));
        s.add(dir_source(
            r"C:\proj\docs",
            sdir("docs", vec![], vec![("notes.md", 30)]),
        ));

        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &s, &CheckSnapshot::new()).root.unwrap();
        (arena, root, s)
    }

    fn node_at(arena: &Arena, path: &str) -> NodeId {
        arena
            .find_by_path(Path::new(path))
            .unwrap_or_else(|| panic!("no node for {path}"))
    }

    #[test]
    fn relative_paths_exclude_the_root() {
        let (arena, root, _) = fixture();
        assert_eq!(rel_from_root(&arena, root, root), "");
        let target = node_at(&arena, r"C:\proj\app\target");
        assert_eq!(rel_from_root(&arena, root, target), "app/target");
    }

    #[test]
    fn a_fully_checked_tree_needs_no_rules() {
        let (arena, root, _) = fixture();
        assert!(compact(&arena, root).is_empty());
    }

    #[test]
    fn unchecking_a_folder_emits_one_rule_and_never_lists_its_children() {
        let (mut arena, root, _) = fixture();
        let target = node_at(&arena, r"C:\proj\app\target");
        check::set_checked(&mut arena, target, false);

        let rules = compact(&arena, root);
        assert_eq!(
            rules,
            vec![Rule {
                path: "app/target".into(),
                scope: Scope::Tree,
                action: Action::Exclude,
            }]
        );
        // Nothing beneath the excluded folder is mentioned anywhere.
        assert!(!rules.iter().any(|r| r.path.contains("big.o")
            || r.path.contains("deep")
            || r.path.contains("huge.o")));
    }

    #[test]
    fn unchecking_a_files_group_scopes_the_rule_to_direct_files() {
        let (mut arena, root, _) = fixture();
        // `app` keeps its src/ and target/ subdirectories, so dropping its
        // direct files cannot collapse into a whole-tree exclusion.
        let app = node_at(&arena, r"C:\proj\app");
        let grp = arena.files_group_of(app).unwrap();
        check::set_checked(&mut arena, grp, false);

        let rules = compact(&arena, root);
        assert_eq!(
            rules,
            vec![Rule {
                path: "app".into(),
                scope: Scope::Files,
                action: Action::Exclude,
            }]
        );
    }

    #[test]
    fn dropping_the_only_files_collapses_to_a_whole_tree_rule() {
        let (mut arena, root, _) = fixture();
        // `src` holds nothing but files, so unchecking its group leaves the
        // folder empty and one tree rule says it more compactly.
        let src = node_at(&arena, r"C:\proj\app\src");
        let grp = arena.files_group_of(src).unwrap();
        check::set_checked(&mut arena, grp, false);

        assert_eq!(
            compact(&arena, root),
            vec![Rule {
                path: "app/src".into(),
                scope: Scope::Tree,
                action: Action::Exclude,
            }]
        );
    }

    #[test]
    fn unchecking_one_file_emits_a_file_scoped_rule() {
        let (mut arena, root, _) = fixture();
        let main = node_at(&arena, r"C:\proj\app\src\main.rs");
        check::set_checked(&mut arena, main, false);

        let rules = compact(&arena, root);
        assert_eq!(
            rules,
            vec![Rule {
                path: "app/src/main.rs".into(),
                scope: Scope::File,
                action: Action::Exclude,
            }]
        );
    }

    #[test]
    fn applying_rules_reproduces_the_selection() {
        let (mut arena, root, sources) = fixture();
        let target = node_at(&arena, r"C:\proj\app\target");
        let notes = node_at(&arena, r"C:\proj\docs\notes.md");
        check::set_checked(&mut arena, target, false);
        check::set_checked(&mut arena, notes, false);

        let want_sel = arena.node(root).sel_size;
        let rules = compact(&arena, root);

        // Rebuild from scratch, as loading a plan does.
        let mut fresh = Arena::new();
        let fresh_root = rebuild(&mut fresh, &sources, &CheckSnapshot::new())
            .root
            .unwrap();
        let unresolved = apply(&mut fresh, fresh_root, &rules);

        assert!(unresolved.is_empty());
        assert_eq!(fresh.node(fresh_root).sel_size, want_sel);
        let t = node_at(&fresh, r"C:\proj\app\target");
        assert_eq!(fresh.node(t).check, CheckState::Unchecked);
        let m = node_at(&fresh, r"C:\proj\app\src\main.rs");
        assert_eq!(fresh.node(m).check, CheckState::Checked);
    }

    #[test]
    fn compaction_is_a_fixpoint_over_application() {
        let (mut arena, root, sources) = fixture();
        for p in [
            r"C:\proj\app\target",
            r"C:\proj\app\src\main.rs",
            r"C:\proj\docs\notes.md",
        ] {
            let id = node_at(&arena, p);
            check::set_checked(&mut arena, id, false);
        }
        let first = compact(&arena, root);

        let mut fresh = Arena::new();
        let fresh_root = rebuild(&mut fresh, &sources, &CheckSnapshot::new())
            .root
            .unwrap();
        apply(&mut fresh, fresh_root, &first);
        let second = compact(&fresh, fresh_root);

        assert_eq!(first, second);
        assert!(!second.is_empty());
    }

    #[test]
    fn stale_rules_are_reported_rather_than_applied() {
        let (mut arena, root, _) = fixture();
        let rules = vec![
            Rule {
                path: "app/vanished".into(),
                scope: Scope::Tree,
                action: Action::Exclude,
            },
            Rule {
                path: "app/target/deep".into(),
                scope: Scope::Files,
                action: Action::Exclude,
            },
        ];
        let unresolved = apply(&mut arena, root, &rules);
        // "app/vanished" is gone; "app/target/deep" exists but holds files, so
        // only the first is unresolved.
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].path, "app/vanished");
    }

    #[test]
    fn a_later_include_rule_overrides_an_earlier_exclusion() {
        let (mut arena, root, _) = fixture();
        let rules = vec![
            Rule {
                path: "app/target".into(),
                scope: Scope::Tree,
                action: Action::Exclude,
            },
            Rule {
                path: "app/target/deep".into(),
                scope: Scope::Tree,
                action: Action::Include,
            },
        ];
        apply(&mut arena, root, &rules);

        let deep = node_at(&arena, r"C:\proj\app\target\deep");
        let target = node_at(&arena, r"C:\proj\app\target");
        assert_eq!(arena.node(deep).check, CheckState::Checked);
        assert_eq!(arena.node(target).check, CheckState::Partial);
    }

    #[test]
    fn plan_round_trips_through_json() {
        let (mut arena, root, _) = fixture();
        let t = node_at(&arena, r"C:\proj\app\target");
        check::set_checked(&mut arena, t, false);

        let plan = ArchivePlan {
            version: PLAN_VERSION,
            generator: "tree-archiver test".into(),
            created: "2026-08-23T10:26:00Z".into(),
            root: Some(r"C:\proj".into()),
            sources: vec![r"C:\proj\app".into(), r"C:\proj\docs".into()],
            sort: SortKey::default(),
            output: OutputOptions::default(),
            rules: compact(&arena, root),
        };

        let json = serde_json::to_string_pretty(&plan).unwrap();
        assert!(json.contains("app/target"));
        let back: ArchivePlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rules, plan.rules);
        assert_eq!(back.version, PLAN_VERSION);
    }

    #[test]
    fn volume_segments_disambiguate_a_synthetic_root() {
        let mut s = Sources::new();
        s.add(dir_source(r"C:\proj", sdir("proj", vec![], vec![("a", 1)])));
        s.add(dir_source(r"D:\proj", sdir("proj", vec![], vec![("b", 1)])));
        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &s, &CheckSnapshot::new()).root.unwrap();

        let c = node_at(&arena, r"C:\proj");
        let d = node_at(&arena, r"D:\proj");
        assert_eq!(rel_from_root(&arena, root, c), "C/proj");
        assert_eq!(rel_from_root(&arena, root, d), "D/proj");
    }

    #[test]
    fn files_group_has_no_path_of_its_own_in_rules() {
        let (arena, root, _) = fixture();
        let src = node_at(&arena, r"C:\proj\app\src");
        let grp = arena.files_group_of(src).unwrap();
        // The pseudo-folder never leaks its display name into a path.
        assert!(!rel_from_root(&arena, root, grp).contains(FILES_GROUP_NAME));
    }

    #[test]
    fn snapshot_and_rules_agree_after_a_reroot() {
        let (mut arena, root, mut sources) = fixture();
        let target = node_at(&arena, r"C:\proj\app\target");
        check::set_checked(&mut arena, target, false);
        let rules_before = compact(&arena, root);

        let snap = snapshot_checks(&arena);
        sources.add(dir_source(
            r"C:\other",
            sdir("other", vec![], vec![("x", 5)]),
        ));
        let new_root = rebuild(&mut arena, &sources, &snap).root.unwrap();
        let rules_after = compact(&arena, new_root);

        // Same exclusion, now expressed against the new root.
        assert_eq!(rules_before[0].path, "app/target");
        assert_eq!(rules_after[0].path, "proj/app/target");
        assert_eq!(rules_after.len(), 1);
    }
}
