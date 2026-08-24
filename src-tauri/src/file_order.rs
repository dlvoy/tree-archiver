//! Reorders a collected entry list for writing, independent of the tree's own
//! on-screen order.
//!
//! Directory entries are always pinned first, in their original traversal
//! (parent-before-child) order — a directory carries no extension and no
//! content bytes, so it has nothing to sort by, and this is the same shape
//! solid 7z already uses internally (`archive::write_all_solid`). Only file
//! entries are ever reordered among themselves.

use crate::archive::Entry;
use crate::model::arena::extension_of;
use crate::model::sort::natural_cmp;
use crate::plan::FileOrder;
use std::cmp::Ordering;

/// Reorders `entries` for writing. `FileOrder::AsInPlan` is a true identity —
/// the fully interleaved traversal order collect_entries produced.
pub fn reorder(entries: Vec<Entry>, order: FileOrder) -> Vec<Entry> {
    match order {
        FileOrder::AsInPlan => entries,
        FileOrder::Alphabetical => partition_and_sort(entries, |a, b| {
            natural_cmp(&a.name, &b.name)
        }),
        FileOrder::Optimal => partition_and_sort(entries, |a, b| optimal_key(a).cmp(&optimal_key(b))),
    }
}

/// Splits into directories (kept in place) and files (sorted by `cmp`), then
/// recombines dirs-first. Both halves keep their relative order among ties,
/// since `sort_by` is stable.
fn partition_and_sort(
    entries: Vec<Entry>,
    cmp: impl Fn(&Entry, &Entry) -> Ordering,
) -> Vec<Entry> {
    let (mut dirs, mut files): (Vec<Entry>, Vec<Entry>) =
        entries.into_iter().partition(|e| e.is_dir);
    files.sort_by(cmp);
    dirs.append(&mut files);
    dirs
}

/// Extension → rough compressibility bucket, most compressible first.
/// Deliberately not exhaustive, and not shared with the frontend's own
/// extension taxonomy (`FileIcon.tsx`), which categorizes for a display
/// purpose across the IPC boundary rather than a compression one.
fn category_rank(ext: &str) -> u8 {
    const TEXT: &[&str] = &["txt", "md", "rst", "csv", "tsv", "log", "ini", "cfg", "conf", "properties", "po"];
    const SOURCE: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "java", "c", "h", "cpp", "cs", "rb",
        "php", "sh", "ps1", "sql", "html", "css", "scss", "json", "toml", "yaml", "yml", "xml",
        "kt", "swift", "lua", "vue", "svelte", "dart", "gradle", "proto",
    ];
    const DOCUMENTS: &[&str] = &["doc", "docx", "odt", "rtf", "pdf", "ppt", "pptx", "xls", "xlsx", "ods", "epub"];
    const TEMPORARY: &[&str] = &[
        "tmp", "temp", "bak", "bk", "backup", "old", "orig", "swp", "swo", "part", "crdownload",
        "cache", "lock",
    ];
    const BINARIES: &[&str] = &["exe", "dll", "so", "dylib", "bin", "obj", "o", "lib", "pdb", "msi", "class", "pyc", "wasm"];
    const MULTIMEDIA: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tif", "psd", "svg", "mp3", "wav",
        "flac", "ogg", "m4a", "aac", "mp4", "mkv", "avi", "mov", "webm",
    ];
    const ARCHIVES: &[&str] = &["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "iso", "cab", "zst", "jar", "war", "apk"];

    if TEXT.contains(&ext) {
        0
    } else if SOURCE.contains(&ext) {
        1
    } else if DOCUMENTS.contains(&ext) {
        2
    } else if TEMPORARY.contains(&ext) {
        3
    } else if BINARIES.contains(&ext) {
        4
    } else if MULTIMEDIA.contains(&ext) {
        5
    } else if ARCHIVES.contains(&ext) {
        6
    } else {
        7
    }
}

/// The composite sort key for `FileOrder::Optimal`, built once and compared
/// as a tuple so `Ord` gives the four-part precedence for free.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct OptimalKey {
    category: u8,
    ext: String,
    file_name: NaturalString,
    // The parent path's components, reversed and each wrapped for natural
    // comparison, so "src/emdash/somedir" and "src/emdash/other/long/path"
    // (leaf-first) diverge only where the two paths actually differ, putting
    // same-named files nested under different roots near each other.
    reversed_path: Vec<NaturalString>,
}

/// A `String` that compares with `natural_cmp` instead of byte order.
#[derive(PartialEq, Eq)]
struct NaturalString(String);

impl Ord for NaturalString {
    fn cmp(&self, other: &Self) -> Ordering {
        natural_cmp(&self.0, &other.0)
    }
}
impl PartialOrd for NaturalString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn optimal_key(entry: &Entry) -> OptimalKey {
    let mut parts: Vec<&str> = entry.name.split('/').collect();
    let file_name = parts.pop().unwrap_or(&entry.name);
    let ext = extension_of(file_name).unwrap_or_default();

    parts.reverse();
    let reversed_path = parts.into_iter().map(|s| NaturalString(s.to_string())).collect();

    OptimalKey {
        category: category_rank(&ext),
        ext,
        file_name: NaturalString(file_name.to_string()),
        reversed_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir(name: &str) -> Entry {
        Entry { name: name.into(), path: Some(PathBuf::from(name)), is_dir: true, size: 0 }
    }

    fn file(name: &str) -> Entry {
        Entry { name: name.into(), path: Some(PathBuf::from(name)), is_dir: false, size: 10 }
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn as_in_plan_is_the_identity() {
        let entries = vec![dir("a"), file("a/2.txt"), dir("a/b"), file("a/b/1.txt")];
        let want: Vec<String> = names(&entries).into_iter().map(String::from).collect();
        let got = reorder(entries, FileOrder::AsInPlan);
        assert_eq!(names(&got), want);
    }

    #[test]
    fn alphabetical_sorts_files_but_pins_directories_first() {
        let entries = vec![file("z.txt"), dir("mid"), file("a.txt"), dir("late")];
        let got = reorder(entries, FileOrder::Alphabetical);
        // Both dirs first, in their original relative order; then files, sorted.
        assert_eq!(names(&got), vec!["mid", "late", "a.txt", "z.txt"]);
    }

    #[test]
    fn optimal_groups_by_category_then_extension_then_name() {
        let entries = vec![file("photo.png"), file("notes.txt"), file("readme.md")];
        let got = reorder(entries, FileOrder::Optimal);
        // text (md, txt) sorts before multimedia (png); within text, "md" < "txt".
        assert_eq!(names(&got), vec!["readme.md", "notes.txt", "photo.png"]);
    }

    #[test]
    fn optimal_pins_directories_first_too() {
        let entries = vec![file("z.png"), dir("only"), file("a.txt")];
        let got = reorder(entries, FileOrder::Optimal);
        assert_eq!(names(&got)[0], "only");
    }

    /// The worked example from the request: two copies of the same file,
    /// nested under unrelated roots but sharing an immediate parent chain,
    /// should land next to each other rather than where an unrelated file of
    /// the same extension happens to sort alphabetically.
    #[test]
    fn optimal_keys_two_same_named_files_by_reversed_parent_path() {
        let entries = vec![
            file("somedir/emdash/src/file.js"),
            file("unrelated/zzz/other.js"),
            file("other/long/path/emdash/src/file.js"),
        ];
        let got = reorder(entries, FileOrder::Optimal);
        let n = names(&got);
        let a = n.iter().position(|&x| x == "somedir/emdash/src/file.js").unwrap();
        let b = n.iter().position(|&x| x == "other/long/path/emdash/src/file.js").unwrap();
        assert!(
            (a as isize - b as isize).abs() == 1,
            "the two file.js copies should be adjacent: {n:?}"
        );
    }

    #[test]
    fn a_tie_on_every_key_keeps_original_order() {
        // Same name, same extension, same (empty) parent path — genuinely
        // indistinguishable by the key, so stability must preserve order.
        let entries = vec![file("dup.txt"), file("dup.txt")];
        let got = reorder(entries, FileOrder::Optimal);
        assert_eq!(names(&got), vec!["dup.txt", "dup.txt"]);
    }

    #[test]
    fn extensionless_files_land_in_other_after_every_named_category() {
        let entries = vec![file("Makefile"), file("readme.md")];
        let got = reorder(entries, FileOrder::Optimal);
        assert_eq!(names(&got), vec!["readme.md", "Makefile"]);
    }
}
