//! Event names and payloads pushed from Rust to the webview.

use serde::Serialize;

pub const SCAN_PROGRESS: &str = "scan://progress";
pub const SCAN_DONE: &str = "scan://done";
pub const ARCHIVE_PROGRESS: &str = "archive://progress";
pub const ARCHIVE_LOG: &str = "archive://log";
pub const ARCHIVE_DONE: &str = "archive://done";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub dirs: u64,
    pub files: u64,
    pub bytes: u64,
    pub current: String,
}
