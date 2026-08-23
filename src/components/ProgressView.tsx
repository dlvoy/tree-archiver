import { useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import * as api from "../api/commands";
import type { ArchiveProgress, ArchiveSummary, LogEntry } from "../api/commands";
import { useLang, useT } from "../i18n/context";
import { translate, type Lang, type T } from "../i18n";
import * as fmt from "../lib/format";
import { Modal } from "./ArchiveDialog";

const LOG_ROW = 22;

/**
 * Renders one log line in the active language.
 *
 * The backend sends a key and its arguments rather than a sentence, so the
 * same run reads correctly whichever language is selected. `message` is the
 * English original and covers any key this build does not know about.
 */
function lineText(lang: Lang, e: LogEntry): string {
  const rendered = translate(lang, e.key, e.args);
  return rendered === e.key ? e.message : rendered;
}

export function ProgressView({
  outPath,
  progress,
  log,
  logTotal,
  logErrors,
  summary,
  onClose,
}: {
  outPath: string;
  progress: ArchiveProgress | null;
  /** The tail held for display. The full log lives in Rust. */
  log: LogEntry[];
  /** How many lines the run has produced in total, `log.length` or more. */
  logTotal: number;
  /** Failures across the whole run, including any no longer in `log`. */
  logErrors: number;
  summary: ArchiveSummary | null;
  onClose: () => void;
}) {
  const t = useT();
  const lang = useLang();
  const [showLog, setShowLog] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const done = summary !== null;
  const fraction =
    progress && progress.bytesTotal > 0
      ? Math.min(1, progress.bytesDone / progress.bytesTotal)
      : done
        ? 1
        : 0;

  // Errors are the reason to look at the log, so open it when the first lands.
  useEffect(() => {
    if (logErrors > 0) setShowLog(true);
  }, [logErrors > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  const cancel = async () => {
    setCancelling(true);
    await api.cancelArchive();
  };

  const writeLog = async () => {
    const path = await save({
      title: t("progress.saveLogTitle"),
      defaultPath: "tree-archiver-log.txt",
      filters: [{ name: t("progress.filterText"), extensions: ["txt"] }],
    });
    if (!path) return;
    try {
      const n = await api.saveLog(path);
      setNote(t("progress.savedLog", { count: n }));
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
    <Modal
      title={done ? headline(t, summary!) : t("progress.building")}
      onClose={done ? onClose : undefined}
      wide
    >
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
          label={t("progress.files")}
          value={
            progress
              ? `${fmt.count(progress.filesDone)} / ${fmt.count(progress.filesTotal)}`
              : "—"
          }
        />
        <Cell
          label={t("progress.written")}
          value={progress ? fmt.bytes(progress.bytesDone) : "—"}
          note={
            progress
              ? t("progress.ofBytes", { total: fmt.bytes(progress.bytesTotal) })
              : undefined
          }
        />
        <Cell label={t("progress.rate")} value={progress ? fmt.rate(progress.bps) : "—"} />
        <Cell
          label={done ? t("progress.elapsed") : t("progress.remaining")}
          value={done ? fmt.duration(summary!.elapsedSecs) : fmt.duration(progress?.etaSecs)}
          strong
        />
      </dl>

      {done && (
        <div className={`alert ${summary!.ok ? "alert--ok" : summary!.cancelled ? "alert--warn" : "alert--error"}`}>
          {summary!.cancelled ? (
            t("progress.cancelledNote")
          ) : summary!.ok ? (
            <>
              {t("progress.okNote", {
                files: fmt.count(summary!.filesWritten),
                dirs: fmt.count(summary!.dirsWritten),
                bytes: fmt.bytes(summary!.bytesWritten),
              })}
              {summary!.errors > 0 && (
                <> {t("progress.errorNote", { count: summary!.errors })}</>
              )}
            </>
          ) : (
            t("progress.failedNote")
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
          {t("progress.log")}
          <span className="logbox__count">
            {fmt.count(logTotal)}
            {logErrors > 0 && (
              <em className="logbox__errors">{t("progress.logFailed", { count: logErrors })}</em>
            )}
          </span>
        </button>
        {showLog && <LogList entries={log} total={logTotal} lang={lang} t={t} />}
      </div>

      {note && <p className="alert alert--info">{note}</p>}

      <div className="modal__actions">
        <button type="button" className="btn" onClick={writeLog} disabled={logTotal === 0}>
          {t("progress.saveLog")}
        </button>
        <div className="bar__spacer" />
        {!done && (
          <button type="button" className="btn btn--danger" onClick={cancel} disabled={cancelling}>
            {cancelling ? t("progress.stopping") : t("progress.cancel")}
          </button>
        )}
        {done && summary!.ok && (
          <button type="button" className="btn" onClick={reveal}>
            {t("progress.reveal")}
          </button>
        )}
        {done && (
          <button type="button" className="btn btn--go" onClick={onClose}>
            {t("progress.done")}
          </button>
        )}
      </div>
    </Modal>
  );
}

function headline(t: T, s: ArchiveSummary): string {
  if (s.cancelled) return t("progress.cancelled");
  if (!s.ok) return t("progress.failed");
  return s.errors > 0 ? t("progress.builtWarnings") : t("progress.built");
}

function LogList({
  entries,
  total,
  lang,
  t,
}: {
  entries: LogEntry[];
  total: number;
  lang: Lang;
  t: T;
}) {
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
    return <div className="logbox__empty">{t("progress.logEmpty")}</div>;
  }

  return (
    <>
      {total > entries.length && (
        <div className="logbox__trimmed">
          {t("progress.logTrimmed", {
            shown: fmt.count(entries.length),
            total: fmt.count(total),
          })}
        </div>
      )}
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
                <span className="logline__msg">{lineText(lang, e)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </>
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
