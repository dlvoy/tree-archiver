//! Filesystem helpers: long-path handling, reparse-point detection, volume
//! grouping and byte formatting.

use std::fs::Metadata;
use std::path::{Component, Path, PathBuf, Prefix};

/// Absolute, `..`-free form of `path`, kept in whatever verbatim form the OS
/// hands back. `canonicalize` on Windows yields the `\\?\` prefix, which is
/// what lets us open paths longer than 260 characters.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Display form of a path: strips the `\\?\` prefix when the path is short
/// enough to work without it. Only ever used for UI and log text.
pub fn display_path(path: &Path) -> String {
    dunce::simplified(path).to_string_lossy().into_owned()
}

/// True when the entry is a symlink or (on Windows) any other reparse point,
/// such as a junction. These are recorded as leaves and never descended into,
/// which is what keeps a scan from looping forever.
#[cfg(windows)]
pub fn is_reparse_point(md: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    md.file_type().is_symlink() || (md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
}

#[cfg(not(windows))]
pub fn is_reparse_point(md: &Metadata) -> bool {
    md.file_type().is_symlink()
}

/// The volume a path lives on: a drive letter, a UNC share, or `/` elsewhere.
/// Paths in different volume groups have no common ancestor, which is why the
/// tree grows a synthetic root instead of pretending they do.
pub fn volume_key(path: &Path) -> String {
    for c in path.components() {
        if let Component::Prefix(p) = c {
            return match p.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => {
                    (d as char).to_ascii_uppercase().to_string()
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    "{}#{}",
                    server.to_string_lossy().to_lowercase(),
                    share.to_string_lossy().to_lowercase()
                ),
                _ => p.as_os_str().to_string_lossy().to_lowercase(),
            };
        }
        break;
    }
    "/".to_string()
}

/// A filesystem-safe folder name for a volume, used as the top-level directory
/// when an archive spans several volumes.
pub fn volume_folder_name(key: &str) -> String {
    key.replace('#', "_")
}

/// Longest shared directory prefix of `paths`, which must all be on the same
/// volume. Returns `None` for an empty slice.
///
/// A single path is its own common root when it is a directory; the caller
/// passes directories only, having already mapped an added file to its parent.
pub fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = paths.iter();
    let mut acc: Vec<Component> = iter.next()?.components().collect();

    for p in iter {
        let other: Vec<Component> = p.components().collect();
        let keep = acc
            .iter()
            .zip(other.iter())
            .take_while(|(a, b)| a == b)
            .count();
        acc.truncate(keep);
        if acc.is_empty() {
            break;
        }
    }

    if acc.is_empty() {
        return None;
    }
    Some(acc.iter().collect())
}

/// True when `ancestor` is `path` or contains it. Component-wise, so `C:\ab`
/// is correctly *not* treated as containing `C:\abc`.
pub fn contains(ancestor: &Path, path: &Path) -> bool {
    let a: Vec<Component> = ancestor.components().collect();
    let p: Vec<Component> = path.components().collect();
    a.len() <= p.len() && a.iter().zip(p.iter()).all(|(x, y)| x == y)
}

/// ISO-8601 UTC timestamp, e.g. `2026-08-23T10:26:00Z`. Hand-rolled so the
/// crate does not pull in a date library for two format calls.
pub fn iso8601_utc(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since the Unix epoch to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Seconds since the Unix epoch, for tar entry mtimes.
pub fn unix_secs(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Human-readable byte count. Binary units, because that is what file managers
/// on this platform show.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0usize;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 {
        format!("{:.0} {}", v, UNITS[u])
    } else if v >= 10.0 {
        format!("{:.1} {}", v, UNITS[u])
    } else {
        format!("{:.2} {}", v, UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn common_ancestor_of_siblings_is_their_parent() {
        let got = common_ancestor(&[pb(r"C:\proj\app"), pb(r"C:\proj\docs")]);
        assert_eq!(got, Some(pb(r"C:\proj")));
    }

    #[test]
    fn common_ancestor_of_nested_paths_is_the_outer_one() {
        let got = common_ancestor(&[pb(r"C:\proj"), pb(r"C:\proj\app\src")]);
        assert_eq!(got, Some(pb(r"C:\proj")));
    }

    #[test]
    fn common_ancestor_of_one_path_is_itself() {
        assert_eq!(common_ancestor(&[pb(r"C:\proj\app")]), Some(pb(r"C:\proj\app")));
    }

    #[test]
    fn common_ancestor_falls_back_to_drive_root() {
        let got = common_ancestor(&[pb(r"C:\a\x"), pb(r"C:\b\y")]);
        assert_eq!(got, Some(pb(r"C:\")));
    }

    #[test]
    fn common_ancestor_of_nothing_is_none() {
        assert_eq!(common_ancestor(&[]), None);
    }

    #[test]
    fn volume_key_reads_drive_letters_case_insensitively() {
        assert_eq!(volume_key(&pb(r"c:\proj")), "C");
        assert_eq!(volume_key(&pb(r"C:\proj")), "C");
        assert_ne!(volume_key(&pb(r"D:\proj")), volume_key(&pb(r"C:\proj")));
    }

    #[test]
    fn volume_key_reads_unc_shares() {
        assert_eq!(volume_key(&pb(r"\\server\share\dir")), "server#share");
    }

    #[test]
    fn containment_is_component_wise() {
        assert!(contains(&pb(r"C:\proj"), &pb(r"C:\proj\app")));
        assert!(contains(&pb(r"C:\proj"), &pb(r"C:\proj")));
        // The prefix "C:\ab" must not swallow "C:\abc".
        assert!(!contains(&pb(r"C:\ab"), &pb(r"C:\abc")));
        assert!(!contains(&pb(r"C:\proj\app"), &pb(r"C:\proj")));
    }

    #[test]
    fn iso_timestamps_match_known_epochs() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(iso8601_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            iso8601_utc(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
            "2001-09-09T01:46:40Z"
        );
        // A leap day, which is where naive date math usually breaks.
        assert_eq!(
            iso8601_utc(UNIX_EPOCH + Duration::from_secs(1_709_164_800)),
            "2024-02-29T00:00:00Z"
        );
    }

    #[test]
    fn byte_formatting_scales_and_keeps_precision_readable() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(15 * 1024), "15.0 KB");
        assert_eq!(format_bytes(512 * 1024), "512 KB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }
}
