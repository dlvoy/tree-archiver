//! Event names and payloads pushed from Rust to the webview.

use serde::Serialize;

pub const SCAN_PROGRESS: &str = "scan://progress";
pub const SCAN_DONE: &str = "scan://done";
pub const ARCHIVE_PROGRESS: &str = "archive://progress";
/// Carries a *batch* of log entries. Every added file produces a line, so one
/// message per line would swamp the IPC bridge on a large archive.
pub const ARCHIVE_LOG: &str = "archive://log";
pub const ARCHIVE_DONE: &str = "archive://done";

/// A tree rebuild the frontend did not ask for — paths arriving from File
/// Explorer while the window is already open.
pub const TREE_UPDATED: &str = "tree://updated";
pub const TREE_ERROR: &str = "tree://error";
/// Raised while an external hand-off is being scanned, so the UI can show the
/// same busy state a drag and drop produces.
pub const TREE_SCANNING: &str = "tree://scanning";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub dirs: u64,
    pub files: u64,
    pub bytes: u64,
    pub current: String,
}
