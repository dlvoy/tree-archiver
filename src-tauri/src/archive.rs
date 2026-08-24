//! Archive writing.
//!
//! Two rules shape this module. Unchecked folders contribute *nothing* — not
//! even an empty directory entry. And a file that cannot be read is logged and
//! skipped: an unreadable file must never end the run.
//!
//! Three formats come out of one traversal. `.tar` and `.tar.gz` are the same
//! tar stream with an optional compressor around it; `.7z` is a different
//! container entirely. `EntrySink` is what lets all three share the loop, the
//! progress accounting and the error policy above. Solid 7z is the one
//! exception — it has to hand every file over at once — and gets its own
//! function.

use crate::fsutil;
use crate::model::arena::{Arena, CheckState, NodeId, NodeKind};
use crate::model::sort::{sort_children, SortKey};
use crate::naming::NamingContext;
use crate::plan::{Compression, OutputOptions, PathMode};
use serde::Serialize;
use sevenz_rust2::encoder_options::Lzma2Options;
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// One thing to write into the archive.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Path inside the archive, forward slashes, no drive letter.
    pub name: String,
    /// Path on disk. `None` for nodes with no filesystem identity.
    pub path: Option<PathBuf>,
    pub is_dir: bool,
    /// Size recorded at scan time; re-read at write time in case it moved.
    pub size: u64,
}

/// Walks the tree in display order and lists everything the archive will hold.
///
/// A node that is `Unchecked` is skipped outright, subtree and all. `Partial`
/// directories are entered so their surviving children come through.
pub fn collect_entries(
    arena: &Arena,
    root: NodeId,
    sort: SortKey,
    mode: PathMode,
    ctx: &NamingContext,
) -> Vec<Entry> {
    let mut out = Vec::new();
    visit(arena, root, sort, mode, ctx, &mut out);
    out
}

fn visit(
    arena: &Arena,
    id: NodeId,
    sort: SortKey,
    mode: PathMode,
    ctx: &NamingContext,
    out: &mut Vec<Entry>,
) {
    let node = arena.node(id);
    if node.check == CheckState::Unchecked {
        return;
    }

    // `entry_name` returns None for a directory that only leads to a staged
    // folder without being inside one. Such a directory contributes no entry
    // but is still walked, because the staged folders sit beneath it.
    let named = node
        .path
        .as_deref()
        .and_then(|p| ctx.entry_name(mode, p).map(|n| (n, p.to_path_buf())));

    match node.kind {
        NodeKind::File => {
            if let Some((name, path)) = named {
                out.push(Entry {
                    name,
                    path: Some(path),
                    is_dir: false,
                    size: node.own_size,
                });
            }
            return;
        }
        // Neither the synthetic root nor the `<files>` group is a real
        // directory, so neither gets an entry — only their contents do.
        NodeKind::SyntheticRoot | NodeKind::FilesGroup => {}
        NodeKind::Dir { .. } => {
            if let Some((name, path)) = named {
                out.push(Entry {
                    name,
                    path: Some(path),
                    is_dir: true,
                    size: 0,
                });
            }
        }
    }

    let mut kids = arena.children(id).to_vec();
    sort_children(arena, &mut kids, sort);
    for k in kids {
        visit(arena, k, sort, mode, ctx, out);
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub entries: u64,
    pub files: u64,
    /// Total bytes of file content.
    pub payload_bytes: u64,
    /// Exact size of the resulting uncompressed tar.
    pub tar_bytes: u64,
}

fn round512(n: u64) -> u64 {
    n.div_ceil(512) * 512
}

/// Exact size of the tar these entries produce.
///
/// Every entry costs a 512-byte header plus its content padded to a 512-byte
/// boundary; a name over 100 bytes costs an extra GNU long-name header and its
/// own padded payload. The archive ends with two zero blocks.
pub fn estimate(entries: &[Entry]) -> Estimate {
    let mut tar = 0u64;
    let mut payload = 0u64;
    let mut files = 0u64;

    for e in entries {
        tar += 512;
        // Directory entries carry a trailing slash in the archive.
        let name_len = e.name.len() as u64 + if e.is_dir { 1 } else { 0 };
        if name_len > 100 {
            tar += 512 + round512(name_len + 1);
        }
        if !e.is_dir {
            files += 1;
            payload = payload.saturating_add(e.size);
            tar += round512(e.size);
        }
    }

    Estimate {
        entries: entries.len() as u64,
        files,
        payload_bytes: payload,
        tar_bytes: tar + 1024,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts: String,
    pub level: LogLevel,
    pub path: String,
    /// Translation key, so the UI can render the line in the user's language.
    pub key: &'static str,
    /// Values to interpolate into the translated string.
    pub args: BTreeMap<String, String>,
    /// The same line in English. This is what `save_log` writes, and what the
    /// UI falls back to for a key it does not recognise.
    pub message: String,
}

/// A log line before it is stamped and levelled: a key for translation, the
/// arguments that key needs, and the English rendering.
pub struct LogMsg {
    key: &'static str,
    args: BTreeMap<String, String>,
    text: String,
}

impl LogMsg {
    pub fn new(key: &'static str, text: impl Into<String>) -> Self {
        LogMsg {
            key,
            args: BTreeMap::new(),
            text: text.into(),
        }
    }

    pub fn arg(mut self, name: &str, value: impl Into<String>) -> Self {
        self.args.insert(name.to_string(), value.into());
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Smoothed throughput in bytes per second.
    pub bps: u64,
    /// Seconds remaining, or `None` before throughput settles.
    pub eta_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSummary {
    pub ok: bool,
    pub cancelled: bool,
    pub out_path: String,
    pub bytes_written: u64,
    pub files_written: u64,
    pub dirs_written: u64,
    pub skipped: u64,
    pub errors: u64,
    pub elapsed_secs: f64,
}

/// Cancellation is the one condition that aborts a run. Everything else is
/// logged and stepped over.
const CANCELLED: &str = "__tree_archiver_cancelled__";

fn is_cancellation(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted && e.to_string().contains(CANCELLED)
}

/// Smooths throughput so the ETA does not jitter on every burst of I/O.
struct Rate {
    started: Instant,
    ewma: f64,
    last: Instant,
    last_bytes: u64,
}

impl Rate {
    fn new() -> Self {
        let now = Instant::now();
        Rate {
            started: now,
            ewma: 0.0,
            last: now,
            last_bytes: 0,
        }
    }

    /// Folds the latest sample in over a roughly 3-second window.
    fn sample(&mut self, total_bytes: u64) -> u64 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        if dt < 0.2 {
            return self.ewma as u64;
        }
        let delta = total_bytes.saturating_sub(self.last_bytes) as f64;
        let inst = delta / dt;
        let alpha = (dt / 3.0).min(1.0);
        self.ewma = if self.ewma == 0.0 {
            inst
        } else {
            self.ewma * (1.0 - alpha) + inst * alpha
        };
        self.last = now;
        self.last_bytes = total_bytes;
        self.ewma as u64
    }

    fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

/// Reads exactly `declared` bytes, whatever the underlying file does.
///
/// The tar header commits to a byte count before the data is written, so a
/// file that shrinks, grows, or starts failing mid-read must still yield
/// exactly that many bytes. Short reads are zero-padded and the problem is
/// recorded rather than propagated; only cancellation escapes as an error.
struct ExactReader<'a, F: FnMut(u64)> {
    inner: Option<File>,
    remaining: u64,
    degraded: bool,
    error: Option<String>,
    cancel: &'a AtomicBool,
    on_bytes: F,
}

impl<F: FnMut(u64)> Read for ExactReader<'_, F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }

        let want = buf.len().min(self.remaining as usize);
        let slice = &mut buf[..want];

        if !self.degraded {
            match self.inner.as_mut().unwrap().read(slice) {
                Ok(0) => {
                    // The file is shorter than its metadata claimed.
                    self.degraded = true;
                    self.error
                        .get_or_insert_with(|| "file shrank while being archived".into());
                }
                Ok(n) => {
                    self.remaining -= n as u64;
                    (self.on_bytes)(n as u64);
                    return Ok(n);
                }
                Err(e) => {
                    self.degraded = true;
                    self.error.get_or_insert_with(|| e.to_string());
                }
            }
        }

        // Pad the shortfall so the entry still matches its header.
        slice.fill(0);
        self.remaining -= want as u64;
        (self.on_bytes)(want as u64);
        Ok(want)
    }
}

/// Writes `entries` into `out_path`.
///
/// `on_progress` and `on_log` are called from this thread; the caller throttles
/// before forwarding to the UI.
pub fn run<P, L>(
    entries: &[Entry],
    out_path: &Path,
    options: OutputOptions,
    cancel: Arc<AtomicBool>,
    mut on_progress: P,
    mut on_log: L,
) -> ArchiveSummary
where
    P: FnMut(Progress),
    L: FnMut(LogEntry),
{
    let est = estimate(entries);
    let mut state = RunState {
        files_done: 0,
        dirs_done: 0,
        bytes_done: 0,
        skipped: 0,
        errors: 0,
        rate: Rate::new(),
    };

    // Open with the totals rather than nothing. Compressing the first file can
    // take a while, and until it lands there is otherwise no event at all and
    // the panel reads as though the run has not started.
    on_progress(Progress {
        files_done: 0,
        files_total: est.files,
        bytes_done: 0,
        bytes_total: est.payload_bytes,
        bps: 0,
        eta_secs: None,
    });

    let log = |level: LogLevel, path: &str, msg: LogMsg, on_log: &mut L| {
        emit(on_log, level, path, msg);
    };

    let file = match File::create(out_path) {
        Ok(f) => f,
        Err(e) => {
            log(
                LogLevel::Error,
                &fsutil::display_path(out_path),
                LogMsg::new("log.createFailed", format!("cannot create the archive: {e}"))
                    .arg("error", e.to_string()),
                &mut on_log,
            );
            return ArchiveSummary {
                ok: false,
                cancelled: false,
                out_path: fsutil::display_path(out_path),
                bytes_written: 0,
                files_written: 0,
                dirs_written: 0,
                skipped: 0,
                errors: 1,
                elapsed_secs: 0.0,
            };
        }
    };

    let writer = BufWriter::with_capacity(1 << 20, file);
    let result = match options.compression {
        Compression::None => write_all(
            TarSink(tar::Builder::new(writer)),
            entries,
            &est,
            &cancel,
            &mut state,
            &mut on_progress,
            &mut on_log,
        ),
        Compression::Gzip => {
            let level = flate2::Compression::new(options.gzip_level.clamp(1, 9));
            write_all(
                TarSink(tar::Builder::new(flate2::write::GzEncoder::new(writer, level))),
                entries,
                &est,
                &cancel,
                &mut state,
                &mut on_progress,
                &mut on_log,
            )
        }
        Compression::SevenZ => match sevenz_writer(writer, options.sevenz_level) {
            Err(e) => Err(e),
            Ok(w) if options.sevenz_solid => write_all_solid(
                w,
                entries,
                &est,
                &cancel,
                &mut state,
                &mut on_progress,
                &mut on_log,
            ),
            Ok(w) => write_all(
                SevenZSink(w),
                entries,
                &est,
                &cancel,
                &mut state,
                &mut on_progress,
                &mut on_log,
            ),
        },
    };

    let cancelled = cancel.load(Ordering::Relaxed);
    let mut ok = true;

    if let Err(e) = result {
        if !cancelled {
            ok = false;
            state.errors += 1;
            log(
                LogLevel::Error,
                &fsutil::display_path(out_path),
                LogMsg::new("log.writeFailed", format!("writing the archive failed: {e}"))
                    .arg("error", e.to_string()),
                &mut on_log,
            );
        }
    }

    if cancelled {
        ok = false;
        // A half-written archive is worse than none, so it goes.
        match std::fs::remove_file(out_path) {
            Ok(()) => log(
                LogLevel::Info,
                &fsutil::display_path(out_path),
                LogMsg::new(
                    "log.cancelledDeleted",
                    "cancelled; the partial archive was deleted",
                ),
                &mut on_log,
            ),
            Err(e) => log(
                LogLevel::Warn,
                &fsutil::display_path(out_path),
                LogMsg::new(
                    "log.cancelledKept",
                    format!("cancelled, but the partial archive could not be deleted: {e}"),
                )
                .arg("error", e.to_string()),
                &mut on_log,
            ),
        }
    }

    let bytes_written = if cancelled {
        0
    } else {
        std::fs::metadata(out_path).map(|m| m.len()).unwrap_or(0)
    };

    let summary = ArchiveSummary {
        ok,
        cancelled,
        out_path: fsutil::display_path(out_path),
        bytes_written,
        files_written: state.files_done,
        dirs_written: state.dirs_done,
        skipped: state.skipped,
        errors: state.errors,
        elapsed_secs: state.rate.elapsed(),
    };

    // The log closes with the same figures the summary panel shows, so a saved
    // log stands on its own without the window that produced it.
    for msg in summary_lines(&summary) {
        let level = if summary.ok {
            LogLevel::Info
        } else {
            LogLevel::Warn
        };
        log(level, &summary.out_path, msg, &mut on_log);
    }

    summary
}

/// The closing lines of the log: what was written, what failed, how long it
/// took. Separate from `run` so the wording can be tested directly.
fn summary_lines(s: &ArchiveSummary) -> Vec<LogMsg> {
    let mut out = Vec::new();

    if s.cancelled {
        out.push(LogMsg::new("log.summaryCancelled", "cancelled by the user"));
    } else if s.ok {
        out.push(
            LogMsg::new(
                "log.summaryWritten",
                format!(
                    "wrote {} files and {} folders, {} bytes on disk",
                    s.files_written, s.dirs_written, s.bytes_written
                ),
            )
            .arg("files", s.files_written.to_string())
            .arg("dirs", s.dirs_written.to_string())
            .arg("bytes", s.bytes_written.to_string()),
        );
    } else {
        out.push(LogMsg::new(
            "log.summaryFailed",
            "the archive could not be completed",
        ));
    }

    if s.errors > 0 {
        out.push(
            LogMsg::new(
                "log.summaryErrors",
                format!("{} could not be read, {} skipped", s.errors, s.skipped),
            )
            .arg("errors", s.errors.to_string())
            .arg("skipped", s.skipped.to_string()),
        );
    }

    out.push(
        LogMsg::new(
            "log.summaryElapsed",
            format!("finished in {:.1}s", s.elapsed_secs),
        )
        .arg("seconds", format!("{:.1}", s.elapsed_secs)),
    );

    out
}

struct RunState {
    files_done: u64,
    dirs_done: u64,
    bytes_done: u64,
    skipped: u64,
    errors: u64,
    rate: Rate,
}

/// Where entries end up once the loop has decided what to write.
///
/// Everything that is the same whatever the format — the traversal order, the
/// byte accounting, the cancellation checks, the rule that an unreadable file
/// is skipped rather than fatal — stays in `write_all`. Only the container
/// differs, and that is all this trait is.
trait EntrySink {
    fn add_dir(&mut self, entry: &Entry, disk_path: &Path, mtime: u64) -> io::Result<()>;

    fn add_file(
        &mut self,
        entry: &Entry,
        disk_path: &Path,
        size: u64,
        mtime: u64,
        reader: &mut dyn Read,
    ) -> io::Result<()>;

    fn finish(self) -> io::Result<()>;
}

struct TarSink<W: Write>(tar::Builder<W>);

impl<W: Write> EntrySink for TarSink<W> {
    fn add_dir(&mut self, entry: &Entry, _disk_path: &Path, mtime: u64) -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_mtime(mtime);
        header.set_uid(0);
        header.set_gid(0);

        // tar convention: directory names carry a trailing slash.
        let name = format!("{}/", entry.name);
        self.0.append_data(&mut header, name, io::empty())
    }

    fn add_file(
        &mut self,
        entry: &Entry,
        _disk_path: &Path,
        size: u64,
        mtime: u64,
        reader: &mut dyn Read,
    ) -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(size);
        header.set_mode(0o644);
        header.set_mtime(mtime);
        header.set_uid(0);
        header.set_gid(0);

        self.0.append_data(&mut header, &entry.name, reader)
    }

    fn finish(self) -> io::Result<()> {
        self.0.into_inner()?.flush()
    }
}

struct SevenZSink(ArchiveWriter<BufWriter<File>>);

impl EntrySink for SevenZSink {
    fn add_dir(&mut self, entry: &Entry, disk_path: &Path, _mtime: u64) -> io::Result<()> {
        self.0
            .push_archive_entry::<&[u8]>(sevenz_entry(entry, disk_path), None)
            .map(|_| ())
            .map_err(sevenz_io)
    }

    fn add_file(
        &mut self,
        entry: &Entry,
        disk_path: &Path,
        _size: u64,
        _mtime: u64,
        reader: &mut dyn Read,
    ) -> io::Result<()> {
        self.0
            .push_archive_entry(sevenz_entry(entry, disk_path), Some(reader))
            .map(|_| ())
            .map_err(sevenz_io)
    }

    fn finish(self) -> io::Result<()> {
        self.0.finish()?.flush()
    }
}

/// A 7z writer over the output file, set to LZMA2 at the chosen preset.
fn sevenz_writer(
    writer: BufWriter<File>,
    level: u32,
) -> io::Result<ArchiveWriter<BufWriter<File>>> {
    let mut w = ArchiveWriter::new(writer).map_err(sevenz_io)?;
    w.set_content_methods(vec![Lzma2Options::from_level(level.min(9)).into()]);
    Ok(w)
}

/// `from_path` collects the timestamps and Windows attributes, but the two
/// flags come from the plan rather than a fresh stat: a file that vanished
/// between the scan and now must still be written as a file, not silently
/// demoted to something without a stream.
fn sevenz_entry(entry: &Entry, disk_path: &Path) -> ArchiveEntry {
    let mut e = ArchiveEntry::from_path(disk_path, entry.name.clone());
    e.is_directory = entry.is_dir;
    e.has_stream = !entry.is_dir;
    e
}

/// Unwraps the `io::Error` a 7z failure is usually carrying. Without this a
/// cancellation stops looking like one by the time it reaches `run`, and the
/// partial archive is reported as a failure instead of being deleted quietly.
fn sevenz_io(e: sevenz_rust2::Error) -> io::Error {
    match e {
        sevenz_rust2::Error::Io(inner, _) | sevenz_rust2::Error::FileOpen(inner, _) => inner,
        other => io::Error::other(other.to_string()),
    }
}

fn emit(on_log: &mut dyn FnMut(LogEntry), level: LogLevel, path: &str, msg: LogMsg) {
    on_log(LogEntry {
        ts: fsutil::iso8601_utc(SystemTime::now()),
        level,
        path: path.to_string(),
        key: msg.key,
        args: msg.args,
        message: msg.text,
    });
}

#[allow(clippy::too_many_arguments)]
fn write_all<S: EntrySink>(
    mut sink: S,
    entries: &[Entry],
    est: &Estimate,
    cancel: &Arc<AtomicBool>,
    state: &mut RunState,
    on_progress: &mut dyn FnMut(Progress),
    on_log: &mut dyn FnMut(LogEntry),
) -> io::Result<()> {
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }

        let Some(disk_path) = entry.path.clone() else {
            continue;
        };

        if entry.is_dir {
            match sink.add_dir(entry, &disk_path, dir_mtime(&disk_path)) {
                Ok(()) => {
                    state.dirs_done += 1;
                    emit(
                        on_log,
                        LogLevel::Info,
                        &entry.name,
                        LogMsg::new("log.addedDir", "added folder"),
                    );
                }
                Err(e) => {
                    state.errors += 1;
                    emit(
                        on_log,
                        LogLevel::Error,
                        &entry.name,
                        LogMsg::new(
                            "log.dirFailed",
                            format!("could not add the directory: {e}"),
                        )
                        .arg("error", e.to_string()),
                    );
                }
            }
            continue;
        }

        // Opening is where access failures land. Nothing has been written for
        // this entry yet, so skipping keeps the archive intact.
        let file = match File::open(&disk_path) {
            Ok(f) => f,
            Err(e) => {
                state.errors += 1;
                state.skipped += 1;
                emit(
                    on_log,
                    LogLevel::Error,
                    &entry.name,
                    LogMsg::new("log.skipped", format!("skipped: {e}")).arg("error", e.to_string()),
                );
                continue;
            }
        };

        let md = file.metadata();
        let size = md.as_ref().map(|m| m.len()).unwrap_or(entry.size);
        let mtime = md
            .as_ref()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(fsutil::unix_secs)
            .unwrap_or(0);

        // The byte count is advanced from inside the reader rather than after
        // the entry is written, so a single large file moves the bar as it
        // goes instead of leaving the window still until it is finished.
        let mut since_report = 0u64;
        let mut reader = ExactReader {
            inner: Some(file),
            remaining: size,
            degraded: false,
            error: None,
            cancel,
            on_bytes: |n: u64| {
                state.bytes_done = state.bytes_done.saturating_add(n);
                since_report += n;
                if since_report >= REPORT_EVERY_BYTES {
                    since_report = 0;
                    on_progress(progress_of(state, est));
                }
            },
        };

        let append = sink.add_file(entry, &disk_path, size, mtime, &mut reader);
        let read_error = reader.error.take();
        drop(reader);

        match append {
            Ok(()) => {
                state.files_done += 1;
                match read_error {
                    // The entry is present and structurally valid, but its
                    // tail is zero padding rather than real data.
                    Some(msg) => {
                        state.errors += 1;
                        emit(
                            on_log,
                            LogLevel::Warn,
                            &entry.name,
                            LogMsg::new(
                                "log.padded",
                                format!("added with padding after a read failure: {msg}"),
                            )
                            .arg("error", msg),
                        );
                    }
                    None => emit(
                        on_log,
                        LogLevel::Info,
                        &entry.name,
                        LogMsg::new("log.addedFile", "added").arg("bytes", size.to_string()),
                    ),
                }
            }
            Err(e) if is_cancellation(&e) => return Err(e),
            Err(e) => return Err(e),
        }

        on_progress(progress_of(state, est));
    }

    sink.finish()?;

    // Nothing is left to do, so the estimate is no longer an estimate.
    on_progress(Progress {
        eta_secs: Some(0),
        ..progress_of(state, est)
    });
    Ok(())
}

/// Shared by every `SolidReader`.
///
/// In solid mode the readers are the only place a byte can be observed:
/// `push_archive_entries` takes the whole batch and does not come back until
/// the last file has been compressed. Everything the tar loop would do between
/// entries therefore has to happen inside `read`.
struct SolidCtx<'c> {
    state: &'c mut RunState,
    est: &'c Estimate,
    on_progress: &'c mut dyn FnMut(Progress),
    on_log: &'c mut dyn FnMut(LogEntry),
    /// Total bytes at the last progress event, so that a 4 KiB read does not
    /// produce a UI update.
    reported_at: u64,
}

/// How much has to be read before the progress bar is worth moving again.
///
/// Every format reports from inside the reader, because that is the only place
/// a byte is visible while a single large file is being written — and an LZMA2
/// pass over one can run for tens of seconds.
const REPORT_EVERY_BYTES: u64 = 1 << 20;

impl SolidCtx<'_> {
    fn progress(&mut self) {
        (self.on_progress)(progress_of(self.state, self.est));
        self.reported_at = self.state.bytes_done;
    }
}

/// Where the run has got to. Sampling the rate is a side effect, so this is
/// called once per report rather than per field.
fn progress_of(state: &mut RunState, est: &Estimate) -> Progress {
    let bps = state.rate.sample(state.bytes_done);
    // The smoothed rate needs a couple of samples to say anything. A run that
    // finishes before then still moved real bytes in real time, so fall back to
    // the plain average rather than reporting a dash.
    let bps = if bps == 0 {
        let secs = state.rate.elapsed();
        if secs > 0.0 {
            (state.bytes_done as f64 / secs) as u64
        } else {
            0
        }
    } else {
        bps
    };
    Progress {
        files_done: state.files_done,
        files_total: est.files,
        bytes_done: state.bytes_done,
        bytes_total: est.payload_bytes,
        bps,
        eta_secs: eta(est.payload_bytes, state.bytes_done, bps),
    }
}

/// One file inside a solid block.
///
/// Opens on the first read and drops the handle at EOF, because
/// `push_archive_entries` wants every reader up front and a staged tree can
/// hold hundreds of thousands of files. At most one is open at a time.
struct SolidReader<'c> {
    path: PathBuf,
    name: String,
    file: Option<File>,
    opened: bool,
    done: bool,
    bytes: u64,
    cancel: Arc<AtomicBool>,
    ctx: Rc<RefCell<SolidCtx<'c>>>,
}

impl SolidReader<'_> {
    /// Closes the entry out: releases the handle and writes the log line the
    /// tar loop would have written once `append_data` returned.
    fn close(&mut self, error: Option<String>) {
        if self.done {
            return;
        }
        self.done = true;
        self.file = None;

        let mut ctx = self.ctx.borrow_mut();
        ctx.state.files_done += 1;
        match error {
            Some(msg) => {
                ctx.state.errors += 1;
                let line = LogMsg::new(
                    "log.truncated",
                    format!("added short after a read failure: {msg}"),
                )
                .arg("error", msg);
                emit(ctx.on_log, LogLevel::Warn, &self.name, line);
            }
            None => {
                let line =
                    LogMsg::new("log.addedFile", "added").arg("bytes", self.bytes.to_string());
                emit(ctx.on_log, LogLevel::Info, &self.name, line);
            }
        }
        ctx.progress();
    }
}

impl Read for SolidReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }

        if !self.opened {
            self.opened = true;
            match File::open(&self.path) {
                Ok(f) => self.file = Some(f),
                Err(e) => {
                    // The probe in `write_all_solid` found this file readable,
                    // so something has changed underneath us. The block is
                    // already committed to holding this entry, so it is written
                    // short rather than the whole run being failed.
                    self.close(Some(e.to_string()));
                    return Ok(0);
                }
            }
        }

        match self.file.as_mut().unwrap().read(buf) {
            Ok(0) => {
                self.close(None);
                Ok(0)
            }
            Ok(n) => {
                self.bytes += n as u64;
                let mut ctx = self.ctx.borrow_mut();
                ctx.state.bytes_done = ctx.state.bytes_done.saturating_add(n as u64);
                if ctx.state.bytes_done - ctx.reported_at >= REPORT_EVERY_BYTES {
                    ctx.progress();
                }
                Ok(n)
            }
            Err(e) => {
                self.close(Some(e.to_string()));
                Ok(0)
            }
        }
    }
}

/// Solid 7z: one shared LZMA2 stream for every file.
///
/// Cannot go through `EntrySink`, because `push_archive_entries` wants the
/// whole batch at once. The per-entry work `write_all` does between calls
/// happens inside `SolidReader` instead.
#[allow(clippy::too_many_arguments)]
fn write_all_solid(
    mut writer: ArchiveWriter<BufWriter<File>>,
    entries: &[Entry],
    est: &Estimate,
    cancel: &Arc<AtomicBool>,
    state: &mut RunState,
    on_progress: &mut dyn FnMut(Progress),
    on_log: &mut dyn FnMut(LogEntry),
) -> io::Result<()> {
    // Directories carry no stream, so they go in one at a time, ahead of the
    // block.
    for entry in entries.iter().filter(|e| e.is_dir) {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }
        let Some(disk_path) = entry.path.as_ref() else {
            continue;
        };
        match writer.push_archive_entry::<&[u8]>(sevenz_entry(entry, disk_path), None) {
            Ok(_) => {
                state.dirs_done += 1;
                emit(
                    on_log,
                    LogLevel::Info,
                    &entry.name,
                    LogMsg::new("log.addedDir", "added folder"),
                );
            }
            Err(e) => {
                state.errors += 1;
                emit(
                    on_log,
                    LogLevel::Error,
                    &entry.name,
                    LogMsg::new("log.dirFailed", format!("could not add the directory: {e}"))
                        .arg("error", e.to_string()),
                );
            }
        }
    }

    // Every file has to be handed over before any of them is compressed, so an
    // unreadable one has to be found now. The tar loop gets skip-and-continue
    // for free by opening as it goes; here it costs a probe.
    let mut batch: Vec<ArchiveEntry> = Vec::new();
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED));
        }
        let Some(disk_path) = entry.path.as_ref() else {
            continue;
        };
        if let Err(e) = File::open(disk_path) {
            state.errors += 1;
            state.skipped += 1;
            emit(
                on_log,
                LogLevel::Error,
                &entry.name,
                LogMsg::new("log.skipped", format!("skipped: {e}")).arg("error", e.to_string()),
            );
            continue;
        }
        batch.push(sevenz_entry(entry, disk_path));
        sources.push((disk_path.clone(), entry.name.clone()));
    }

    // The context borrows `state` and both callbacks for as long as the readers
    // live. `push_archive_entries` takes them by value and drops them, and the
    // `Rc` goes out of scope with the block, which releases the borrows.
    if !batch.is_empty() {
        let ctx = Rc::new(RefCell::new(SolidCtx {
            state,
            est,
            on_progress,
            on_log,
            reported_at: 0,
        }));
        let readers: Vec<SourceReader<SolidReader>> = sources
            .into_iter()
            .map(|(path, name)| {
                SourceReader::new(SolidReader {
                    path,
                    name,
                    file: None,
                    opened: false,
                    done: false,
                    bytes: 0,
                    cancel: Arc::clone(cancel),
                    ctx: Rc::clone(&ctx),
                })
            })
            .collect();

        writer
            .push_archive_entries(batch, readers)
            .map_err(sevenz_io)?;
    }

    writer.finish()?.flush()?;

    // Nothing is left to do, so the estimate is no longer an estimate.
    on_progress(Progress {
        eta_secs: Some(0),
        ..progress_of(state, est)
    });
    Ok(())
}

fn dir_mtime(disk_path: &Path) -> u64 {
    std::fs::metadata(disk_path)
        .and_then(|m| m.modified())
        .map(fsutil::unix_secs)
        .unwrap_or(0)
}

fn eta(total: u64, done: u64, bps: u64) -> Option<u64> {
    if bps == 0 || done == 0 {
        return None;
    }
    Some(total.saturating_sub(done) / bps.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_of(ok: bool, cancelled: bool, errors: u64) -> ArchiveSummary {
        ArchiveSummary {
            ok,
            cancelled,
            out_path: "out.tar".into(),
            bytes_written: 4096,
            files_written: 7,
            dirs_written: 2,
            skipped: errors,
            errors,
            elapsed_secs: 1.25,
        }
    }

    /// A saved log has to stand on its own, so the closing lines repeat the
    /// figures the summary panel shows.
    #[test]
    fn a_clean_run_closes_with_counts_and_a_duration() {
        let lines = summary_lines(&summary_of(true, false, 0));
        let keys: Vec<&str> = lines.iter().map(|l| l.key).collect();
        assert_eq!(keys, vec!["log.summaryWritten", "log.summaryElapsed"]);
        assert_eq!(lines[0].args.get("files").map(String::as_str), Some("7"));
        assert_eq!(lines[0].args.get("dirs").map(String::as_str), Some("2"));
        // `{:.1}` rounds half to even, so 1.25 formats as 1.2.
        assert_eq!(lines[1].args.get("seconds").map(String::as_str), Some("1.2"));
    }

    /// Failures are counted in their own line rather than buried in the first.
    #[test]
    fn unreadable_files_get_their_own_closing_line() {
        let lines = summary_lines(&summary_of(true, false, 3));
        let keys: Vec<&str> = lines.iter().map(|l| l.key).collect();
        assert_eq!(
            keys,
            vec![
                "log.summaryWritten",
                "log.summaryErrors",
                "log.summaryElapsed"
            ]
        );
        assert_eq!(lines[1].args.get("errors").map(String::as_str), Some("3"));
    }

    #[test]
    fn a_cancelled_run_says_so_instead_of_reporting_totals() {
        let lines = summary_lines(&summary_of(false, true, 0));
        assert_eq!(lines[0].key, "log.summaryCancelled");
    }

    #[test]
    fn a_failed_run_says_so_instead_of_reporting_totals() {
        let lines = summary_lines(&summary_of(false, false, 1));
        assert_eq!(lines[0].key, "log.summaryFailed");
    }

    /// Every key the frontend has to translate, in one place, so adding a log
    /// line without a translation shows up here first.
    #[test]
    fn the_translatable_keys_are_the_documented_ones() {
        let all = summary_lines(&summary_of(true, false, 1));
        for l in &all {
            assert!(l.key.starts_with("log."), "{} is not namespaced", l.key);
            assert!(!l.text.is_empty(), "{} has no English fallback", l.key);
        }
    }
    use crate::model::arena::FILES_GROUP_NAME;
    use crate::model::check;
    use crate::naming::NamingContext;
    use crate::roots::{rebuild, CheckSnapshot, Sources};
    use crate::scan::{scan_path, ScanDir, ScanFile, Source, SourceTree};
    use std::collections::HashSet;
    use std::fs;

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

    /// C:\proj  app/{src/{main.rs}, target/{big.o}}  docs/{notes.md}
    fn fixture() -> (Arena, NodeId, NamingContext) {
        let mut s = Sources::new();
        s.add(Source {
            path: PathBuf::from(r"C:\proj"),
            tree: SourceTree::Dir(sdir(
                "proj",
                vec![
                    sdir(
                        "app",
                        vec![
                            sdir("src", vec![], vec![("main.rs", 100)]),
                            sdir("target", vec![], vec![("big.o", 4000), ("small.o", 20)]),
                        ],
                        vec![],
                    ),
                    sdir("docs", vec![], vec![("notes.md", 30)]),
                ],
                vec![],
            )),
        });
        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &s, &CheckSnapshot::new()).root.unwrap();
        let ctx = NamingContext::from_sources(s.iter());
        (arena, root, ctx)
    }

    /// With a single staged folder every mode agrees, so the default is used
    /// throughout except where a test says otherwise.
    fn entries_of(arena: &Arena, root: NodeId, ctx: &NamingContext) -> Vec<Entry> {
        collect_entries(arena, root, SortKey::default(), PathMode::FoldersOnly, ctx)
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn every_checked_node_becomes_an_entry() {
        let (arena, root, ctx) = fixture();
        let e = entries_of(&arena, root, &ctx);
        let n: HashSet<String> = names(&e).into_iter().collect();

        assert!(n.contains("proj"));
        assert!(n.contains("proj/app/src"));
        assert!(n.contains("proj/app/src/main.rs"));
        assert!(n.contains("proj/docs/notes.md"));
        // The pseudo-folder is never an entry.
        assert!(!n.iter().any(|s| s.contains(FILES_GROUP_NAME)));
    }

    /// Pins `Node::total_items` (Count sort mode) to the figure the Build
    /// dialog already shows: on a tree with a single source and no spine
    /// complications, every real node produces exactly one entry.
    #[test]
    fn total_items_matches_the_entry_count_the_build_dialog_shows() {
        let (arena, root, ctx) = fixture();
        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);
        assert_eq!(arena.node(root).total_items, est.entries);
    }

    #[test]
    fn an_unchecked_folder_contributes_nothing_at_all() {
        let (mut arena, root, ctx) = fixture();
        let target = arena.find_by_path(Path::new(r"C:\proj\app\target")).unwrap();
        check::set_checked(&mut arena, target, false);

        let e = entries_of(&arena, root, &ctx);
        let n = names(&e);

        // Neither the directory entry nor anything beneath it.
        assert!(!n.iter().any(|s| s.contains("target")));
        assert!(!n.iter().any(|s| s.contains("big.o")));
        // Siblings survive.
        assert!(n.contains(&"proj/app/src/main.rs".to_string()));
    }

    #[test]
    fn a_partial_directory_is_entered_not_skipped() {
        let (mut arena, root, ctx) = fixture();
        let big = arena
            .find_by_path(Path::new(r"C:\proj\app\target\big.o"))
            .unwrap();
        check::set_checked(&mut arena, big, false);

        let n = names(&entries_of(&arena, root, &ctx));
        // The folder stays, since small.o keeps it only partially excluded.
        assert!(n.contains(&"proj/app/target".to_string()));
        assert!(n.contains(&"proj/app/target/small.o".to_string()));
        assert!(!n.contains(&"proj/app/target/big.o".to_string()));
    }

    #[test]
    fn estimate_matches_the_tar_block_layout() {
        let entries = vec![
            Entry {
                name: "proj".into(),
                path: Some(PathBuf::from(r"C:\proj")),
                is_dir: true,
                size: 0,
            },
            Entry {
                name: "proj/a.bin".into(),
                path: Some(PathBuf::from(r"C:\proj\a.bin")),
                is_dir: false,
                size: 100,
            },
            Entry {
                name: "proj/b.bin".into(),
                path: Some(PathBuf::from(r"C:\proj\b.bin")),
                is_dir: false,
                size: 513,
            },
        ];
        let est = estimate(&entries);
        // 3 headers + 512 (100 padded) + 1024 (513 padded) + 1024 trailer.
        assert_eq!(est.tar_bytes, 3 * 512 + 512 + 1024 + 1024);
        assert_eq!(est.payload_bytes, 613);
        assert_eq!(est.files, 2);
        assert_eq!(est.entries, 3);
    }

    #[test]
    fn long_names_add_a_gnu_header_to_the_estimate() {
        let long = format!("proj/{}", "d/".repeat(60));
        let entries = vec![Entry {
            name: long.clone(),
            path: Some(PathBuf::from(r"C:\proj")),
            is_dir: false,
            size: 0,
        }];
        let est = estimate(&entries);
        let extra = 512 + round512(long.len() as u64 + 1);
        assert_eq!(est.tar_bytes, 512 + extra + 1024);
    }

    // --- end-to-end writing, against a real temp tree ---

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("tree-archiver-arch-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempTree(p)
        }
        fn file(&self, rel: &str, bytes: usize) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, vec![b'x'; bytes]).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Scans a real directory and returns its arena.
    fn scan_fixture(t: &TempTree) -> (Arena, NodeId, NamingContext) {
        let cancel = Arc::new(AtomicBool::new(false));
        let out = scan_path(&t.0, &cancel, |_, _| {}).unwrap();
        let mut s = Sources::new();
        s.add(out.source);
        let mut arena = Arena::new();
        let root = rebuild(&mut arena, &s, &CheckSnapshot::new()).root.unwrap();
        let ctx = NamingContext::from_sources(s.iter());
        (arena, root, ctx)
    }

    fn tar_names(path: &Path) -> Vec<String> {
        let f = File::open(path).unwrap();
        let mut a = tar::Archive::new(f);
        a.entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn writes_a_readable_archive_with_exact_predicted_size() {
        let t = TempTree::new("write");
        t.file("keep/a.txt", 100);
        t.file("keep/b.txt", 250);
        let (arena, root, ctx) = scan_fixture(&t);

        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);
        let out = t.0.join("..").join(format!("{}-out.tar", t.0.file_name().unwrap().to_string_lossy()));

        let cancel = Arc::new(AtomicBool::new(false));
        let summary = run(
            &entries,
            &out,
            OutputOptions::default(),
            cancel,
            |_| {},
            |_| {},
        );

        assert!(summary.ok, "archive run failed: {summary:?}");
        assert_eq!(summary.errors, 0);
        assert_eq!(summary.files_written, 2);
        // The estimate is exact for an uncompressed tar.
        assert_eq!(summary.bytes_written, est.tar_bytes);

        let listed = tar_names(&out);
        assert!(listed.iter().any(|n| n.ends_with("keep/a.txt")));
        assert!(listed.iter().any(|n| n.ends_with("keep/b.txt")));
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn an_excluded_folder_is_absent_from_the_written_archive() {
        let t = TempTree::new("exclude");
        t.file("keep/a.txt", 10);
        t.file("drop/secret.txt", 10);
        let (mut arena, root, ctx) = scan_fixture(&t);

        let drop_id = arena.find_by_path(&fsutil::canonical(&t.0.join("drop")).unwrap()).unwrap();
        check::set_checked(&mut arena, drop_id, false);

        let entries = entries_of(&arena, root, &ctx);
        let out = t.0.join("..").join("tree-archiver-exclude-out.tar");
        let cancel = Arc::new(AtomicBool::new(false));
        let summary = run(&entries, &out, OutputOptions::default(), cancel, |_| {}, |_| {});

        assert!(summary.ok);
        let listed = tar_names(&out);
        assert!(listed.iter().any(|n| n.contains("keep")));
        // No directory entry, no contents.
        assert!(!listed.iter().any(|n| n.contains("drop")));
        assert!(!listed.iter().any(|n| n.contains("secret")));
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn a_missing_file_is_logged_and_the_run_still_finishes() {
        let t = TempTree::new("missing");
        t.file("a.txt", 10);
        t.file("vanishes.txt", 10);
        let (arena, root, ctx) = scan_fixture(&t);

        let entries = entries_of(&arena, root, &ctx);
        // Delete after scanning, so the entry survives into the write phase.
        fs::remove_file(t.0.join("vanishes.txt")).unwrap();

        let out = t.0.join("..").join("tree-archiver-missing-out.tar");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut logs = Vec::new();
        let summary = run(
            &entries,
            &out,
            OutputOptions::default(),
            cancel,
            |_| {},
            |l| logs.push(l),
        );

        // The failure is reported but never fatal.
        assert!(summary.ok);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.files_written, 1);
        assert!(logs
            .iter()
            .any(|l| l.level == LogLevel::Error && l.path.contains("vanishes.txt")));

        let listed = tar_names(&out);
        assert!(listed.iter().any(|n| n.ends_with("a.txt")));
        assert!(!listed.iter().any(|n| n.ends_with("vanishes.txt")));
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn gzip_output_is_smaller_than_the_uncompressed_estimate() {
        let t = TempTree::new("gzip");
        // Highly compressible content, so the gap is unambiguous.
        t.file("big.txt", 200_000);
        let (arena, root, ctx) = scan_fixture(&t);

        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);
        let out = t.0.join("..").join("tree-archiver-gzip-out.tar.gz");
        let cancel = Arc::new(AtomicBool::new(false));
        let summary = run(
            &entries,
            &out,
            OutputOptions {
                compression: Compression::Gzip,
                gzip_level: 6,
                path_mode: PathMode::FoldersOnly,
                ..OutputOptions::default()
            },
            cancel,
            |_| {},
            |_| {},
        );

        assert!(summary.ok);
        assert!(
            summary.bytes_written < est.tar_bytes,
            "gzip wrote {} bytes, estimate was {}",
            summary.bytes_written,
            est.tar_bytes
        );
        let _ = fs::remove_file(&out);
    }

    #[test]
    fn cancelling_deletes_the_partial_archive() {
        let t = TempTree::new("cancel");
        t.file("a.txt", 1000);
        let (arena, root, ctx) = scan_fixture(&t);
        let entries = entries_of(&arena, root, &ctx);

        let out = t.0.join("..").join("tree-archiver-cancel-out.tar");
        let cancel = Arc::new(AtomicBool::new(true));
        let summary = run(&entries, &out, OutputOptions::default(), cancel, |_| {}, |_| {});

        assert!(summary.cancelled);
        assert!(!summary.ok);
        assert!(!out.exists(), "a cancelled run must not leave an archive");
    }


    fn sevenz_options(solid: bool) -> OutputOptions {
        OutputOptions {
            compression: Compression::SevenZ,
            sevenz_solid: solid,
            ..OutputOptions::default()
        }
    }

    fn sevenz_names(path: &Path) -> Vec<String> {
        let reader = sevenz_rust2::ArchiveReader::open(path, Default::default()).unwrap();
        reader
            .archive()
            .files
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }

    fn sevenz_file(path: &Path, name: &str) -> Vec<u8> {
        let mut reader = sevenz_rust2::ArchiveReader::open(path, Default::default()).unwrap();
        reader.read_file(name).unwrap()
    }

    /// The whole point of the format, and the one thing a bad encoder chain
    /// would break silently.
    #[test]
    fn sevenz_output_is_readable_and_smaller_than_the_estimate() {
        let t = TempTree::new("sevenz");
        t.file("big.txt", 200_000);
        let (arena, root, ctx) = scan_fixture(&t);

        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);
        let out = t.0.join("..").join("tree-archiver-sevenz-out.7z");
        let cancel = Arc::new(AtomicBool::new(false));
        let summary = run(&entries, &out, sevenz_options(false), cancel, |_| {}, |_| {});

        assert!(summary.ok, "7z run failed: {summary:?}");
        assert_eq!(summary.errors, 0);
        assert!(
            summary.bytes_written < est.tar_bytes,
            "7z wrote {} bytes, the uncompressed estimate was {}",
            summary.bytes_written,
            est.tar_bytes
        );

        let listed = sevenz_names(&out);
        assert!(listed.iter().any(|n| n.ends_with("big.txt")), "{listed:?}");
        let _ = fs::remove_file(&out);
    }

    /// Solid and non-solid must differ only in how the bytes are packed. If the
    /// two ever disagree about what is *in* the archive, one of the two paths
    /// has dropped an entry.
    #[test]
    fn solid_and_non_solid_hold_the_same_entries() {
        let t = TempTree::new("solid");
        t.file("keep/a.txt", 20_000);
        t.file("keep/b.txt", 20_000);
        t.file("keep/nested/c.txt", 20_000);
        let (arena, root, ctx) = scan_fixture(&t);
        let entries = entries_of(&arena, root, &ctx);

        let loose = t.0.join("..").join("tree-archiver-solid-loose.7z");
        let solid = t.0.join("..").join("tree-archiver-solid-solid.7z");

        let a = run(
            &entries,
            &loose,
            sevenz_options(false),
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
        );
        let b = run(
            &entries,
            &solid,
            sevenz_options(true),
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| {},
        );

        assert!(a.ok && b.ok, "{a:?} {b:?}");
        assert_eq!(a.files_written, b.files_written);
        assert_eq!(a.dirs_written, b.dirs_written);

        let mut names_a = sevenz_names(&loose);
        let mut names_b = sevenz_names(&solid);
        names_a.sort();
        names_b.sort();
        assert_eq!(names_a, names_b);

        // Three identical files in one stream compress better than three
        // streams that cannot see each other.
        assert!(
            b.bytes_written < a.bytes_written,
            "solid wrote {} bytes, a stream per file wrote {}",
            b.bytes_written,
            a.bytes_written
        );

        let _ = fs::remove_file(&loose);
        let _ = fs::remove_file(&solid);
    }

    /// Extract-and-compare: the header can look right while the payload is
    /// mangled.
    #[test]
    fn a_sevenz_entry_reads_back_byte_for_byte() {
        let t = TempTree::new("szround");
        t.file("keep/a.txt", 4096);
        let original = fs::read(t.0.join("keep").join("a.txt")).unwrap();
        let (arena, root, ctx) = scan_fixture(&t);
        let entries = entries_of(&arena, root, &ctx);

        for (tag, solid) in [("loose", false), ("solid", true)] {
            let out = t.0.join("..").join(format!("tree-archiver-szround-{tag}.7z"));
            let summary = run(
                &entries,
                &out,
                sevenz_options(solid),
                Arc::new(AtomicBool::new(false)),
                |_| {},
                |_| {},
            );
            assert!(summary.ok, "{tag}: {summary:?}");

            let name = sevenz_names(&out)
                .into_iter()
                .find(|n| n.ends_with("a.txt"))
                .unwrap_or_else(|| panic!("{tag}: a.txt is missing"));
            assert_eq!(sevenz_file(&out, &name), original, "{tag} payload differs");
            let _ = fs::remove_file(&out);
        }
    }

    /// The rule that outlives every format: an unreadable file is logged and
    /// skipped, and the archive still completes. Solid mode has to work for
    /// this rather than getting it for free.
    #[test]
    fn a_missing_file_is_skipped_in_both_sevenz_modes() {
        for (tag, solid) in [("loose", false), ("solid", true)] {
            let t = TempTree::new(&format!("szmiss-{tag}"));
            t.file("a.txt", 10);
            t.file("vanishes.txt", 10);
            let (arena, root, ctx) = scan_fixture(&t);
            let entries = entries_of(&arena, root, &ctx);
            fs::remove_file(t.0.join("vanishes.txt")).unwrap();

            let out = t.0.join("..").join(format!("tree-archiver-szmiss-{tag}.7z"));
            let mut logs = Vec::new();
            let summary = run(
                &entries,
                &out,
                sevenz_options(solid),
                Arc::new(AtomicBool::new(false)),
                |_| {},
                |l| logs.push(l),
            );

            assert!(summary.ok, "{tag}: {summary:?}");
            assert_eq!(summary.errors, 1, "{tag}");
            assert_eq!(summary.skipped, 1, "{tag}");
            assert_eq!(summary.files_written, 1, "{tag}");
            assert!(
                logs.iter()
                    .any(|l| l.level == LogLevel::Error && l.path.contains("vanishes.txt")),
                "{tag}: nothing was logged about the missing file"
            );

            let listed = sevenz_names(&out);
            assert!(listed.iter().any(|n| n.ends_with("a.txt")), "{tag}");
            assert!(!listed.iter().any(|n| n.ends_with("vanishes.txt")), "{tag}");
            let _ = fs::remove_file(&out);
        }
    }

    #[test]
    fn cancelling_a_sevenz_run_deletes_the_partial_archive() {
        for (tag, solid) in [("loose", false), ("solid", true)] {
            let t = TempTree::new(&format!("szcancel-{tag}"));
            t.file("a.txt", 1000);
            let (arena, root, ctx) = scan_fixture(&t);
            let entries = entries_of(&arena, root, &ctx);

            let out = t.0.join("..").join(format!("tree-archiver-szcancel-{tag}.7z"));
            let summary = run(
                &entries,
                &out,
                sevenz_options(solid),
                Arc::new(AtomicBool::new(true)),
                |_| {},
                |_| {},
            );

            assert!(summary.cancelled, "{tag}");
            assert!(!summary.ok, "{tag}");
            assert!(!out.exists(), "{tag}: a cancelled run left an archive behind");
        }
    }

    /// Solid mode reports progress from inside the readers, which is the only
    /// place a byte is visible once the batch has been handed over.
    #[test]
    fn solid_progress_reaches_the_full_payload() {
        let t = TempTree::new("szprogress");
        t.file("a.bin", 5000);
        t.file("b.bin", 7000);
        let (arena, root, ctx) = scan_fixture(&t);
        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);

        let out = t.0.join("..").join("tree-archiver-szprogress-out.7z");
        let mut last: Option<Progress> = None;
        let summary = run(
            &entries,
            &out,
            sevenz_options(true),
            Arc::new(AtomicBool::new(false)),
            |p| last = Some(p),
            |_| {},
        );

        assert!(summary.ok);
        let last = last.expect("progress was never reported");
        assert_eq!(last.bytes_done, est.payload_bytes);
        assert_eq!(last.files_done, est.files);
        assert_eq!(last.eta_secs, Some(0));
        let _ = fs::remove_file(&out);
    }


    /// The complaint that started this: one large file meant one progress
    /// event, at the end. With 7z that is tens of seconds of a window that
    /// looks hung. Every format now reports as it reads.
    #[test]
    fn a_single_large_file_reports_while_it_is_being_written() {
        for (tag, opts) in [
            ("tar", OutputOptions::default()),
            ("7z", sevenz_options(false)),
            ("7z-solid", sevenz_options(true)),
        ] {
            let t = TempTree::new(&format!("midfile-{tag}"));
            // Comfortably more than one report interval's worth of bytes.
            t.file("big.bin", 4 << 20);
            let (arena, root, ctx) = scan_fixture(&t);
            let entries = entries_of(&arena, root, &ctx);
            assert_eq!(entries.iter().filter(|e| !e.is_dir).count(), 1, "{tag}");

            let out = t.0.join("..").join(format!("tree-archiver-midfile-{tag}"));
            let mut seen: Vec<Progress> = Vec::new();
            let summary = run(
                &entries,
                &out,
                opts,
                Arc::new(AtomicBool::new(false)),
                |p| seen.push(p),
                |_| {},
            );
            assert!(summary.ok, "{tag}: {summary:?}");

            // One file, so anything past the opening event and the closing one
            // can only have come from inside the read.
            assert!(
                seen.len() > 2,
                "{tag}: only {} progress events for an 8 MB file",
                seen.len()
            );

            // The panel is filled in from the first event rather than showing
            // dashes until the first file lands.
            let first = seen.first().unwrap();
            assert_eq!(first.bytes_done, 0, "{tag}");
            assert_eq!(first.files_total, 1, "{tag}");
            assert!(first.bytes_total > 0, "{tag}");

            // Bytes only ever move forwards, and land exactly on the payload.
            let mut prev = 0;
            for p in &seen {
                assert!(p.bytes_done >= prev, "{tag}: progress went backwards");
                prev = p.bytes_done;
            }
            let last = seen.last().unwrap();
            assert_eq!(last.bytes_done, last.bytes_total, "{tag}");
            assert_eq!(last.files_done, last.files_total, "{tag}");

            let _ = fs::remove_file(&out);
        }
    }

    /// Whatever the last event says has to match what the summary says, or the
    /// panel contradicts its own completion message.
    #[test]
    fn the_last_progress_event_agrees_with_the_summary() {
        for (tag, opts) in [
            ("tar", OutputOptions::default()),
            ("gzip", OutputOptions { compression: Compression::Gzip, ..OutputOptions::default() }),
            ("7z", sevenz_options(false)),
            ("7z-solid", sevenz_options(true)),
        ] {
            let t = TempTree::new(&format!("agree-{tag}"));
            // A big file first and a tail of small ones: the shape that hid the
            // bug, because the small ones all finished inside one tick.
            t.file("a-big.bin", 2 << 20);
            for i in 0..12 {
                t.file(&format!("small-{i:02}.txt"), 64);
            }
            let (arena, root, ctx) = scan_fixture(&t);
            let entries = entries_of(&arena, root, &ctx);

            let out = t.0.join("..").join(format!("tree-archiver-agree-{tag}"));
            let mut last: Option<Progress> = None;
            let summary = run(
                &entries,
                &out,
                opts,
                Arc::new(AtomicBool::new(false)),
                |p| last = Some(p),
                |_| {},
            );

            assert!(summary.ok, "{tag}: {summary:?}");
            let last = last.expect("no progress at all");
            assert_eq!(last.files_done, summary.files_written, "{tag}");
            assert_eq!(last.files_done, last.files_total, "{tag}");
            assert_eq!(last.bytes_done, last.bytes_total, "{tag}");
            // A run too short to smooth a rate still reports one, rather than
            // leaving the speed as a dash.
            assert!(last.bps > 0, "{tag}: no throughput reported");
            let _ = fs::remove_file(&out);
        }
    }

    #[test]
    fn progress_reaches_the_full_payload() {
        let t = TempTree::new("progress");
        t.file("a.bin", 5000);
        t.file("b.bin", 7000);
        let (arena, root, ctx) = scan_fixture(&t);
        let entries = entries_of(&arena, root, &ctx);
        let est = estimate(&entries);

        let out = t.0.join("..").join("tree-archiver-progress-out.tar");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut last: Option<Progress> = None;
        let summary = run(
            &entries,
            &out,
            OutputOptions::default(),
            cancel,
            |p| last = Some(p),
            |_| {},
        );

        assert!(summary.ok);
        let last = last.expect("progress was never reported");
        assert_eq!(last.bytes_done, est.payload_bytes);
        assert_eq!(last.files_done, est.files);
        assert_eq!(last.eta_secs, Some(0));
        let _ = fs::remove_file(&out);
    }
}
