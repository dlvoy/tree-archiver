//! Preset and user-imported `.gitignore`-style rulesets, and the matcher
//! that applies them across the whole staged tree.
//!
//! Real gitignore semantics — anchoring, `**`, negation, directory-only
//! patterns — are exactly the kind of fiddly parsing this app avoids writing
//! by hand elsewhere (see `archive.rs`'s use of `tar`/`flate2`/`sevenz-rust2`),
//! so matching goes through the `ignore` crate rather than a bespoke parser.
//!
//! A ruleset is matched **per staged source**, not against the arena's own
//! root. That root is often a spine directory that was never actually
//! scanned — just a connector between two unrelated staged folders — so a
//! `/`-anchored pattern would behave unpredictably if tested against it.
//! Anchoring at each `Source::path` is what a `/`-anchored pattern intends:
//! one `.gitignore` per folder the user actually added.

use crate::fsutil;
use crate::model::arena::{Arena, CheckState};
use crate::model::check;
use crate::scan::Source;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A named collection of gitignore-style pattern lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreRuleset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<String>,
    /// Whether this ruleset starts out ticked the first time it's ever seen
    /// — a fresh install, or a built-in shipped after the user's last save.
    /// Once the user has explicitly toggled it either way, that choice is
    /// what persists; this is only the tie-break for "never touched".
    pub default_checked: bool,
}

/// Built-in ids are namespaced so a user-imported ruleset can never collide
/// with one, and so a built-in is instantly recognisable wherever an id
/// shows up on its own (settings, the checked-set, deletion guards).
const BUILTIN_PREFIX: &str = "builtin:";

pub fn is_builtin_id(id: &str) -> bool {
    id.starts_with(BUILTIN_PREFIX)
}

fn ruleset(
    id: &str,
    name: &str,
    description: &str,
    rules: &[&str],
    default_checked: bool,
) -> IgnoreRuleset {
    IgnoreRuleset {
        id: format!("{BUILTIN_PREFIX}{id}"),
        name: name.to_string(),
        description: description.to_string(),
        rules: rules.iter().map(|s| s.to_string()).collect(),
        default_checked,
    }
}

/// The presets shipped with the app. Not persisted — recreated fresh every
/// time, so a later release can add or refine one without a settings
/// migration. Ordered from most to least likely to be wanted.
pub fn builtins() -> Vec<IgnoreRuleset> {
    vec![
        ruleset(
            "backup",
            "Backup",
            "Editor and tool backup files.",
            &[
                "*.bak",
                "*.bk",
                "*~",
                "*.orig",
                "*.old",
                "*.swp",
                "*.swo",
                "*.swn",
                "*.save",
                "*.backup",
                "*.rej",
                "*.autosave",
            ],
            true,
        ),
        ruleset(
            "temporary",
            "Temporary",
            "Temporary files and directories created during development or execution.",
            &[
                "*.tmp",
                "*.temp",
                "*.temporary",
                "*.tempfile",
                "tmp/",
                "temp/",
                ".tmp/",
                "temporary/",
                "scratch/",
            ],
            true,
        ),
        ruleset(
            "caches",
            "Caches",
            "Cache directories and files left behind by build tools, package managers, and editors.",
            &[
                "**/*cache*/",
                "**/.cache/",
                ".cache/",
                "*.cache",
                ".parcel-cache/",
                ".pytest_cache/",
                ".mypy_cache/",
                ".ruff_cache/",
                ".eslintcache",
                ".sass-cache/",
                ".gradle/",
                ".terraform/",
                ".turbo/",
                ".next/cache/",
                ".nuxt/",
                ".yarn/cache/",
                ".npm/",
            ],
            true,
        ),
        ruleset(
            "dev-packages",
            "Dev Packages",
            "Installed dependency folders, reproducible from a manifest or lockfile.",
            &[
                "node_modules/",
                "bower_components/",
                ".venv/",
                "venv/",
                "env/",
                ".env/",
                "vendor/",
                "packages/",
                "__pypackages__/",
                ".bundle/",
            ],
            true,
        ),
        ruleset(
            "precompiled",
            "Precompiled",
            "Python bytecode, native extensions, and interpreter cache files.",
            &[
                "__pycache__/",
                "*.pyc",
                "*.pyo",
                "*.pyd",
                "*.so",
                "*.dll",
                "*.dylib",
                "*.class",
                "*.o",
                "*.obj",
                "*.a",
                "*.lib",
            ],
            true,
        ),
        ruleset(
            "logs",
            "Logs",
            "Log files, rotated logs, traces, and log folders.",
            &[
                "*.log",
                "*.log.*",
                "*.trace",
                "*.out",
                "*.err",
                "logs/",
                "log/",
                "var/log/",
                "npm-debug.log*",
                "yarn-debug.log*",
                "yarn-error.log*",
                "pnpm-debug.log*",
                "lerna-debug.log*",
            ],
            true,
        ),
        ruleset(
            "database-temporary",
            "Database Temporary",
            "Temporary files created by database engines, including SQLite journal and WAL files.",
            &[
                "*.db-wal",
                "*.db-shm",
                "*.db-journal",
                "*.sqlite-wal",
                "*.sqlite-shm",
                "*.sqlite-journal",
                "*.sqlite3-wal",
                "*.sqlite3-shm",
                "*.sqlite3-journal",
                "*.mdb-wal",
                "*.mdb-shm",
                "*.laccdb",
            ],
            true,
        ),
        ruleset(
            "os-metadata",
            "OS Metadata",
            "Operating-system metadata and filesystem view files.",
            &[
                ".DS_Store",
                ".DS_Store?",
                "._*",
                ".Spotlight-V100/",
                ".Trashes/",
                ".AppleDouble/",
                ".LSOverride",
                "Thumbs.db",
                "Thumbs.db:encryptable",
                "ehthumbs.db",
                "ehthumbs_vista.db",
                "Desktop.ini",
                "$RECYCLE.BIN/",
            ],
            true,
        ),
        ruleset(
            "editor-state",
            "Editor State",
            "Local editor, IDE, workspace, and session state.",
            &[
                ".idea/",
                ".vscode/*.log",
                ".vscode/*.tmp",
                "*.code-workspace",
                "*.sublime-workspace",
                "*.sublime-project",
                ".history/",
                ".project",
                ".classpath",
                ".settings/",
                "*.iml",
                ".fleet/",
                ".zed/",
            ],
            true,
        ),
        ruleset(
            "crash-dumps",
            "Crash Dumps",
            "Crash dumps, core dumps, and diagnostic artifacts.",
            &[
                "core",
                "core.*",
                "*.core",
                "*.dmp",
                "*.dump",
                "*.mdmp",
                "*.stackdump",
                "*.hprof",
            ],
            true,
        ),
        ruleset(
            "build-output",
            "Build Output",
            "Generated build output and packaging artifacts.",
            &[
                "dist/",
                "build/",
                "out/",
                "target/",
                "bin/",
                "obj/",
                "_build/",
                ".build/",
                "cmake-build-*/",
                "*.egg-info/",
                "*.whl",
                "*.egg",
                "*.gem",
            ],
            false,
        ),
        ruleset(
            "coverage",
            "Coverage",
            "Generated test coverage reports and profiling data.",
            &[
                "coverage/",
                ".coverage",
                ".coverage.*",
                "htmlcov/",
                ".nyc_output/",
                "lcov-report/",
                "*.prof",
                "*.profraw",
                "*.gcda",
                "*.gcno",
            ],
            false,
        ),
        ruleset(
            "generated-files",
            "Generated Files",
            "Generated files that can be recreated from source or project configuration.",
            &[
                "*.generated.*",
                "*.gen.*",
                "*.stamp",
                "*.manifest",
                "*.patch",
                "*.diff",
                "generated/",
                "gen/",
            ],
            false,
        ),
    ]
}

/// Checks that every non-comment, non-blank line in `text` parses as a valid
/// gitignore pattern. Returns the lines to store (comments and blank lines
/// dropped), or the first line that failed, verbatim, so the import dialog
/// can point at exactly what was wrong.
pub fn validate(text: &str) -> Result<Vec<String>, String> {
    // The root here only has to be *a* valid path for parsing purposes —
    // validation happens once at import time, decoupled from any tree.
    // Matching later builds a fresh, correctly-rooted matcher per source.
    let mut builder = GitignoreBuilder::new(Path::new("."));
    let mut rules = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Err(e) = builder.add_line(None, line) {
            return Err(format!("\"{line}\" is not a valid pattern: {e}"));
        }
        rules.push(line.to_string());
    }
    if rules.is_empty() {
        return Err("the file has no rule lines".to_string());
    }
    Ok(rules)
}

/// One combined matcher for every checked ruleset's lines, anchored at
/// `root`. Rulesets are added in order, so a later ruleset's `!` negation can
/// still override an earlier one's exclusion, the same as stacking multiple
/// `.gitignore` files.
fn build_matcher(root: &Path, checked: &[&IgnoreRuleset], case_insensitive: bool) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    // Case sensitivity only affects globs added after this call, so it has
    // to happen before the loop below, not after.
    let _ = builder.case_insensitive(case_insensitive);
    let mut any = false;
    for rs in checked {
        for line in &rs.rules {
            // Already validated at definition/import time; a bad line here
            // is simply skipped rather than failing the whole apply.
            if builder.add_line(None, line).is_ok() {
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    builder.build().ok()
}

/// Matches `checked` against every staged source in `arena`, unchecking
/// whatever a pattern hits and returning each excluded path mapped to the
/// *name* of the ruleset that caused it — the only thing the mark is ever
/// used for is a tooltip, so the name rather than the id travels with it.
///
/// Nodes already `Unchecked` — whether by hand or from an earlier rule in
/// this same pass — are left alone: this only ever turns *checked* content
/// off, it never re-derives anything a manual choice already settled.
///
/// `overrides` is the set of paths a manual re-check has already rescued
/// from a previous Apply. Without it, resync (see `apply_ignore_rulesets`,
/// which re-checks every previously auto-ignored path before calling this)
/// would immediately re-exclude one the moment any *other* ruleset gets
/// (re-)applied — a manual override has to outlive the Apply that follows
/// it, not just the one it was made during. The one limitation this doesn't
/// cover, matching a real `.gitignore`'s own limitation: a file nested
/// inside a directory that itself matches is swept with the rest of that
/// subtree regardless of an override, since un-ignoring one file inside an
/// ignored directory isn't something plain gitignore syntax can express
/// either.
///
/// `case_insensitive` applies uniformly to every checked ruleset in this
/// pass — real `.gitignore` has no per-line case toggle, so neither does
/// this.
///
/// A directory match unchecks and marks its whole subtree, then that
/// subtree is not descended into further — the same "don't look inside an
/// ignored directory" rule real git uses, and a free performance win besides.
pub fn apply(
    arena: &mut Arena,
    sources: &[Source],
    checked: &[&IgnoreRuleset],
    overrides: &std::collections::HashSet<PathBuf>,
    case_insensitive: bool,
) -> HashMap<PathBuf, String> {
    let mut marks = HashMap::new();
    if checked.is_empty() {
        return marks;
    }

    for source in sources {
        let Some(root_id) = arena.find_by_path(&source.path) else {
            continue;
        };
        let Some(matcher) = build_matcher(&source.path, checked, case_insensitive) else {
            continue;
        };

        // The path of the most recently matched directory, while we are
        // still walking its descendants. `None` once we've walked past it.
        let mut suppressed: Option<PathBuf> = None;

        for id in arena.descendants(root_id) {
            let (path, is_dir, state) = {
                let node = arena.node(id);
                (node.path.clone(), node.kind.is_dir(), node.check)
            };
            let Some(path) = path else {
                continue; // FilesGroup / SyntheticRoot: never a candidate.
            };
            if let Some(dir) = &suppressed {
                if fsutil::contains(dir, &path) {
                    continue;
                }
                suppressed = None;
            }
            if state == CheckState::Unchecked || overrides.contains(&path) {
                continue;
            }

            let hit = match matcher.matched(&path, is_dir) {
                Match::Ignore(glob) => {
                    let pattern = glob.original();
                    checked.iter().find(|rs| rs.rules.iter().any(|l| l == pattern))
                }
                _ => None,
            };
            let Some(rs) = hit else { continue };

            if is_dir {
                suppressed = Some(path);
            }
            for d in arena.descendants(id) {
                let dn = arena.node_mut(d);
                dn.check = CheckState::Unchecked;
                if let Some(p) = dn.path.clone() {
                    marks.insert(p, rs.name.clone());
                }
            }
        }
    }

    check::recompute_all(arena);
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::arena::{Arena, NodeKind};
    use crate::model::check::set_checked;
    use crate::scan::SourceTree;
    use std::path::PathBuf;

    /// Every shipped preset has to parse, or `build_matcher` would silently
    /// drop it and Apply would do nothing.
    #[test]
    fn every_builtin_ruleset_is_valid() {
        for rs in builtins() {
            for line in &rs.rules {
                assert!(
                    GitignoreBuilder::new(Path::new(".")).add_line(None, line).is_ok(),
                    "{}: {line:?} does not parse",
                    rs.name
                );
            }
        }
    }

    #[test]
    fn validate_rejects_bad_syntax_and_reports_the_line() {
        // A descending character range, invalid in any glob dialect.
        let err = validate("*.bak\n[z-a]").unwrap_err();
        assert!(err.contains("[z-a]"), "{err}");
    }

    #[test]
    fn validate_drops_comments_and_blank_lines() {
        let rules = validate("# a comment\n\n*.bak\n").unwrap();
        assert_eq!(rules, vec!["*.bak".to_string()]);
    }

    #[test]
    fn validate_rejects_a_file_with_no_rules() {
        assert!(validate("# nothing but comments\n").is_err());
    }

    /// A minimal tree, built by hand rather than scanned: one staged source
    /// `root` holding `keep.txt`, `drop.bak`, and `node_modules/pkg/index.js`.
    fn fixture() -> (Arena, Vec<Source>) {
        let mut arena = Arena::new();
        let root = arena.add(None, "root".into(), NodeKind::Dir { scanned: true });
        arena.set_path(root, PathBuf::from(r"C:\root"));

        let keep = arena.add(Some(root), "keep.txt".into(), NodeKind::File);
        arena.set_path(keep, PathBuf::from(r"C:\root\keep.txt"));

        let drop = arena.add(Some(root), "drop.bak".into(), NodeKind::File);
        arena.set_path(drop, PathBuf::from(r"C:\root\drop.bak"));

        let nm = arena.add(Some(root), "node_modules".into(), NodeKind::Dir { scanned: true });
        arena.set_path(nm, PathBuf::from(r"C:\root\node_modules"));
        let pkg = arena.add(Some(nm), "index.js".into(), NodeKind::File);
        arena.set_path(pkg, PathBuf::from(r"C:\root\node_modules\index.js"));

        let sources = vec![Source {
            path: PathBuf::from(r"C:\root"),
            tree: SourceTree::Dir(crate::scan::ScanDir {
                name: "root".into(),
                dirs: vec![],
                files: vec![],
            }),
        }];
        (arena, sources)
    }

    fn checked_of<'a>(all: &'a [IgnoreRuleset], ids: &[&str]) -> Vec<&'a IgnoreRuleset> {
        all.iter().filter(|r| ids.contains(&r.id.as_str())).collect()
    }

    fn no_overrides() -> std::collections::HashSet<PathBuf> {
        std::collections::HashSet::new()
    }

    #[test]
    fn a_matched_file_is_unchecked_and_marked() {
        let (mut arena, sources) = fixture();
        let all = builtins();
        let checked = checked_of(&all, &["builtin:backup"]);
        let marks = apply(&mut arena, &sources, &checked, &no_overrides(), false);

        let drop = arena.find_by_path(Path::new(r"C:\root\drop.bak")).unwrap();
        assert_eq!(arena.node(drop).check, CheckState::Unchecked);
        assert_eq!(marks.get(Path::new(r"C:\root\drop.bak")).unwrap(), "Backup");

        let keep = arena.find_by_path(Path::new(r"C:\root\keep.txt")).unwrap();
        assert_eq!(arena.node(keep).check, CheckState::Checked);
        assert!(!marks.contains_key(Path::new(r"C:\root\keep.txt")));
    }

    #[test]
    fn a_matched_directory_is_pruned_and_its_whole_subtree_is_marked() {
        let (mut arena, sources) = fixture();
        let all = builtins();
        let checked = checked_of(&all, &["builtin:dev-packages"]);
        let marks = apply(&mut arena, &sources, &checked, &no_overrides(), false);

        let nm = arena.find_by_path(Path::new(r"C:\root\node_modules")).unwrap();
        let pkg = arena.find_by_path(Path::new(r"C:\root\node_modules\index.js")).unwrap();
        assert_eq!(arena.node(nm).check, CheckState::Unchecked);
        assert_eq!(arena.node(pkg).check, CheckState::Unchecked);
        assert_eq!(marks.len(), 2, "{marks:?}");
    }

    #[test]
    fn a_manual_uncheck_is_left_alone_and_unmarked() {
        let (mut arena, sources) = fixture();
        let keep = arena.find_by_path(Path::new(r"C:\root\keep.txt")).unwrap();
        set_checked(&mut arena, keep, false);

        let all = builtins();
        // No ruleset matches keep.txt, so this proves the apply pass doesn't
        // touch or mark something it had no reason to.
        let checked = checked_of(&all, &["builtin:backup"]);
        let marks = apply(&mut arena, &sources, &checked, &no_overrides(), false);

        assert_eq!(arena.node(keep).check, CheckState::Unchecked);
        assert!(!marks.contains_key(Path::new(r"C:\root\keep.txt")));
    }

    /// `*.bak` and `**/*cache*/` are lowercase in the built-in presets; a
    /// case-sensitive match must miss `SOMETHING.BAK` and `Cache`, and a
    /// case-insensitive one must catch both.
    #[test]
    fn case_insensitive_matches_differently_cased_names() {
        let mut arena = Arena::new();
        let root = arena.add(None, "root".into(), NodeKind::Dir { scanned: true });
        arena.set_path(root, PathBuf::from(r"C:\root"));
        let bak = arena.add(Some(root), "SOMETHING.BAK".into(), NodeKind::File);
        arena.set_path(bak, PathBuf::from(r"C:\root\SOMETHING.BAK"));
        let cache = arena.add(Some(root), "Cache".into(), NodeKind::Dir { scanned: true });
        arena.set_path(cache, PathBuf::from(r"C:\root\Cache"));

        let sources = vec![Source {
            path: PathBuf::from(r"C:\root"),
            tree: SourceTree::Dir(crate::scan::ScanDir { name: "root".into(), dirs: vec![], files: vec![] }),
        }];

        let all = builtins();
        let checked = checked_of(&all, &["builtin:backup", "builtin:caches"]);

        // Case-sensitive: the differently-cased names are missed entirely.
        let marks = apply(&mut arena, &sources, &checked, &no_overrides(), false);
        assert!(marks.is_empty(), "{marks:?}");
        assert_eq!(arena.node(bak).check, CheckState::Checked);
        assert_eq!(arena.node(cache).check, CheckState::Checked);

        // Case-insensitive: both match now.
        let marks = apply(&mut arena, &sources, &checked, &no_overrides(), true);
        assert_eq!(arena.node(bak).check, CheckState::Unchecked);
        assert_eq!(arena.node(cache).check, CheckState::Unchecked);
        assert_eq!(marks.len(), 2, "{marks:?}");
    }

    /// A second Apply with the same checked set has nothing left to do —
    /// everything it would have unchecked is already `Unchecked`, so it
    /// leaves the tree exactly as the first Apply left it and marks nothing
    /// further.
    #[test]
    fn apply_is_idempotent() {
        let (mut arena, sources) = fixture();
        let all = builtins();
        let checked = checked_of(&all, &["builtin:backup", "builtin:dev-packages"]);
        let first = apply(&mut arena, &sources, &checked, &no_overrides(), false);
        assert!(!first.is_empty());

        let before: Vec<CheckState> =
            arena.descendants(arena.find_by_path(Path::new(r"C:\root")).unwrap())
                .into_iter().map(|id| arena.node(id).check).collect();

        let second = apply(&mut arena, &sources, &checked, &no_overrides(), false);
        assert!(second.is_empty(), "{second:?}");

        let after: Vec<CheckState> =
            arena.descendants(arena.find_by_path(Path::new(r"C:\root")).unwrap())
                .into_iter().map(|id| arena.node(id).check).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn unchecking_a_ruleset_and_reapplying_restores_its_matches() {
        let (mut arena, sources) = fixture();
        let all = builtins();

        let both = checked_of(&all, &["builtin:backup", "builtin:dev-packages"]);
        apply(&mut arena, &sources, &both, &no_overrides(), false);
        let nm = arena.find_by_path(Path::new(r"C:\root\node_modules")).unwrap();
        assert_eq!(arena.node(nm).check, CheckState::Unchecked);

        // The caller is responsible for re-checking previously auto-ignored
        // paths before re-applying with a smaller checked set — that's the
        // resync step `apply_ignore_rulesets` performs using the returned
        // marks map. Simulate it here directly.
        set_checked(&mut arena, nm, true);
        let backup_only = checked_of(&all, &["builtin:backup"]);
        apply(&mut arena, &sources, &backup_only, &no_overrides(), false);
        assert_eq!(arena.node(nm).check, CheckState::Checked);
    }

    /// The bug this guards against: a user manually rescues one auto-ignored
    /// file, then clicks Apply again for something *unrelated* (a different
    /// ruleset entirely). Without `overrides`, the resync step in
    /// `apply_ignore_rulesets` would re-check every previously auto-ignored
    /// path and then re-derive from scratch — silently re-excluding the
    /// file the user had just rescued, even though nothing about *its*
    /// ruleset changed.
    #[test]
    fn a_manual_override_survives_an_unrelated_reapply() {
        let (mut arena, sources) = fixture();
        let all = builtins();
        let both = checked_of(&all, &["builtin:backup", "builtin:dev-packages"]);

        apply(&mut arena, &sources, &both, &no_overrides(), false);
        let drop = arena.find_by_path(Path::new(r"C:\root\drop.bak")).unwrap();
        assert_eq!(arena.node(drop).check, CheckState::Unchecked);

        // The user rescues drop.bak by hand; the caller records the override
        // the same moment it clears the auto-ignore mark (see
        // `commands::set_checked`).
        set_checked(&mut arena, drop, true);
        let mut overrides = std::collections::HashSet::new();
        overrides.insert(PathBuf::from(r"C:\root\drop.bak"));

        // Re-apply with the *same* checked set, as if the user had toggled
        // an unrelated ruleset and clicked Apply again.
        let marks = apply(&mut arena, &sources, &both, &overrides, false);

        assert_eq!(arena.node(drop).check, CheckState::Checked, "the override was lost");
        assert!(!marks.contains_key(Path::new(r"C:\root\drop.bak")));
    }

    #[test]
    fn a_later_ruleset_can_negate_an_earlier_one() {
        let (mut arena, sources) = fixture();
        let all = builtins();
        let backup = all.iter().find(|r| r.id == "builtin:backup").unwrap().clone();
        let keep_it = IgnoreRuleset {
            id: "custom:keep-bak".into(),
            name: "Keep this one back".into(),
            description: String::new(),
            rules: vec!["!drop.bak".into()],
            default_checked: true,
        };
        // Order matters: the negation must come after the exclusion.
        let checked = vec![&backup, &keep_it];
        apply(&mut arena, &sources, &checked, &no_overrides(), false);

        let drop = arena.find_by_path(Path::new(r"C:\root\drop.bak")).unwrap();
        assert_eq!(arena.node(drop).check, CheckState::Checked);
    }
}
