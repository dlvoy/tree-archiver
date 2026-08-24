//! Stamps the build so the About dialog can say which one it is.
//!
//! Both values degrade to a placeholder rather than failing: a source tarball
//! has no `.git`, and a build that cannot name itself is still a build.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rustc-env=TA_GIT_HASH={}", git_hash());
    println!("cargo:rustc-env=TA_BUILD_DATE={}", build_date());

    // A new commit changes the hash, so the stamp has to be recut. The build
    // date can still go stale across incremental rebuilds; release builds are
    // always cut fresh, which is where the date matters.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    tauri_build::build()
}

fn git_hash() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// `YYYY-MM-DD`, UTC. Honours `SOURCE_DATE_EPOCH` so a reproducible build can
/// pin the stamp.
fn build_date() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since the Unix epoch to a calendar date (Howard Hinnant's algorithm).
///
/// The same routine as `fsutil::civil_from_days`, duplicated because a build
/// script cannot import the crate it is building.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
