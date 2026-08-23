import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import * as api from "../api/commands";
import type { ArchiveProgress, ArchiveSummary, LogEntry } from "../api/commands";
import * as fmt from "../lib/format";
import { Modal } from "./ArchiveDialog";

const LOG_ROW = 22;

export function ProgressView({
  outPath,
  progress,
  log,
  summary,
  onClose,
}: {
  outPath: string;
  progress: ArchiveProgress | null;
  log: LogEntry[];
  summary: ArchiveSummary | null;
  onClose: () => void;
}) {
  const [showLog, setShowLog] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const done = summary !== null;
  const errors = useMemo(() => log.filter((l) => l.level === "error").length, [log]);
  const fraction =
    progress && progress.bytesTotal > 0
      ? Math.min(1, progress.bytesDone / progress.bytesTotal)
      : done
        ? 1
        : 0;

  // Errors are the reason to look at the log, so open it when the first lands.
  useEffect(() => {
    if (errors > 0) setShowLog(true);
  }, [errors > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  const cancel = async () => {
    setCancelling(true);
    await api.cancelArchive();
  };

  const writeLog = async () => {
    const path = await save({
      title: "Save log",
      defaultPath: "tree-archiver-log.txt",
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (!path) return;
    try {
      const n = await api.saveLog(path);
      setNote(`Wrote ${fmt.count(n)} log ${n === 1 ? "line" : "lines"}.`);
    } catch (e) {
      setNote(String(e));
    }
  };

  const reveal = async () => {
    try {
      await revealItemInDir(summary!.outPath);
    } catch (e) {
      setNote(String(e));
    }
  };

  return (
    <Modal title={done ? headline(summary!) : "Building archive"} onClose={done ? onClose : undefined} wide>
      <p className="progress__target" title={outPath}>
        {fmt.shortenPath(summary?.outPath ?? outPath, 78)}
      </p>

      <div className="progress">
        <div className="progress__track">
          <div
            className={`progress__fill ${done && !summary!.ok ? "progress__fill--stopped" : ""}`}
            style={{ width: `${fraction * 100}%` }}
          />
        </div>
        <div className="progress__pct">{Math.round(fraction * 100)}%</div>
      </div>

      <dl className="titleblock__grid titleblock__grid--flat">
        <Cell
          label="Files"
          value={
            progress
              ? `${fmt.count(progress.filesDone)} / ${fmt.count(progress.filesTotal)}`
              : "—"
          }
        />
        <Cell
          label="Written"
          value={progress ? fmt.bytes(progress.bytesDone) : "—"}
          note={progress ? `of ${fmt.bytes(progress.bytesTotal)}` : undefined}
        />
        <Cell label="Rate" value={progress ? fmt.rate(progress.bps) : "—"} />
        <Cell
          label={done ? "Elapsed" : "Remaining"}
          value={done ? fmt.duration(summary!.elapsedSecs) : fmt.duration(progress?.etaSecs)}
          strong
        />
      </dl>

      {done && (
        <div className={`alert ${summary!.ok ? "alert--ok" : summary!.cancelled ? "alert--warn" : "alert--error"}`}>
          {summary!.cancelled ? (
            <>Cancelled. The partial archive was deleted.</>
          ) : summary!.ok ? (
            <>
              Wrote {fmt.count(summary!.filesWritten)} files and{" "}
              {fmt.count(summary!.dirsWritten)} folders — {fmt.bytes(summary!.bytesWritten)} on disk.
              {summary!.errors > 0 && (
                <>
                  {" "}
                  {fmt.count(summary!.errors)} {summary!.errors === 1 ? "item" : "items"} could not
                  be read and {summary!.skipped > 0 ? "were skipped" : "were padded"}; the archive is
                  complete otherwise.
                </>
              )}
            </>
          ) : (
            <>The archive could not be completed. The log below says why.</>
          )}
        </div>
      )}

      <div className="logbox">
        <button
          type="button"
          className={`logbox__toggle ${showLog ? "logbox__toggle--open" : ""}`}
          onClick={() => setShowLog((v) => !v)}
          aria-expanded={showLog}
        >
          <svg viewBox="0 0 10 10" width="9" height="9" aria-hidden="true">
            <path d="M3.5 1.5L7 5l-3.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          Log
          <span className="logbox__count">
            {fmt.count(log.length)}
            {errors > 0 && <em className="logbox__errors">{fmt.count(errors)} failed</em>}
          </span>
        </button>
        {showLog && <LogList entries={log} />}
      </div>

      {note && <p className="alert alert--info">{note}</p>}

      <div className="modal__actions">
        <button type="button" className="btn" onClick={writeLog} disabled={log.length === 0}>
          Save log
        </button>
        <div className="bar__spacer" />
        {!done && (
          <button type="button" className="btn btn--danger" onClick={cancel} disabled={cancelling}>
            {cancelling ? "Stopping…" : "Cancel"}
          </button>
        )}
        {done && summary!.ok && (
          <button type="button" className="btn" onClick={reveal}>
            Show in folder
          </button>
        )}
        {done && (
          <button type="button" className="btn btn--go" onClick={onClose}>
            Done
          </button>
        )}
      </div>
    </Modal>
  );
}

function headline(s: ArchiveSummary): string {
  if (s.cancelled) return "Archive cancelled";
  if (!s.ok) return "Archive failed";
  return s.errors > 0 ? "Archive built with warnings" : "Archive built";
}

function LogList({ entries }: { entries: LogEntry[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: entries.length,
    getScrollElement: () => ref.current,
    estimateSize: () => LOG_ROW,
    overscan: 10,
  });

  // Follow the tail as new lines arrive.
  useEffect(() => {
    if (entries.length) virtual.scrollToIndex(entries.length - 1);
  }, [entries.length]); // eslint-disable-line react-hooks/exhaustive-deps

  if (entries.length === 0) {
    return <div className="logbox__empty">Nothing logged yet.</div>;
  }

  return (
    <div className="logbox__scroll" ref={ref}>
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((item) => {
          const e = entries[item.index];
          return (
            <div
              key={item.index}
              className={`logline logline--${e.level}`}
              style={{ transform: `translateY(${item.start}px)`, height: LOG_ROW }}
            >
              <span className="logline__ts">{fmt.clockTime(e.ts)}</span>
              <span className="logline__path" title={e.path}>
                {e.path}
              </span>
              <span className="logline__msg">{e.message}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Cell({
  label,
  value,
  note,
  strong,
}: {
  label: string;
  value: string;
  note?: string;
  strong?: boolean;
}) {
  return (
    <div className={`tbfield ${strong ? "tbfield--strong" : ""}`}>
      <dt className="tbfield__label">{label}</dt>
      <dd className="tbfield__value">
        {value}
        {note && <span className="tbfield__note">{note}</span>}
      </dd>
    </div>
  );
}
