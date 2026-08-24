import { useEffect, useState, type ReactNode } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import * as api from "../api/commands";
import type {
  Compression,
  Estimate,
  OutputOptions,
  PathMode,
  PathModeOptions,
} from "../api/commands";
import { useT } from "../i18n/context";
import type { Key } from "../i18n";
import * as fmt from "../lib/format";

/** Everything that differs per format, in one place rather than four. */
const FORMATS: { c: Compression; label: Key; filter: Key; ext: string }[] = [
  { c: "none", label: "build.compressionNone", filter: "build.filterTar", ext: "tar" },
  { c: "gzip", label: "build.compressionGzip", filter: "build.filterTarGz", ext: "tar.gz" },
  { c: "7z", label: "build.compression7z", filter: "build.filter7z", ext: "7z" },
];

/** Every extension the app writes, so switching format replaces rather than appends. */
const EXT_RE = /\.(tar(\.gz)?|7z)$/i;

const formatOf = (c: Compression) => FORMATS.find((f) => f.c === c) ?? FORMATS[0];

const MODES: { mode: PathMode; label: Key; blurb: Key }[] = [
  { mode: "foldersOnly", label: "build.modeFoldersOnly", blurb: "build.blurbFoldersOnly" },
  { mode: "commonRoot", label: "build.modeCommonRoot", blurb: "build.blurbCommonRoot" },
  { mode: "fullPath", label: "build.modeFullPath", blurb: "build.blurbFullPath" },
];

/**
 * Pre-flight. The spec reads as a drawing's title block: fixed fields, fixed
 * order, so the same number is always in the same place.
 */
export function ArchiveDialog({
  options,
  onOptionsChange,
  onClose,
  onStarted,
}: {
  options: OutputOptions;
  onOptionsChange: (o: OutputOptions) => void;
  onClose: () => void;
  onStarted: (outPath: string) => void;
}) {
  const t = useT();
  const [est, setEst] = useState<Estimate | null>(null);
  const [modes, setModes] = useState<PathModeOptions | null>(null);
  const [outPath, setOutPath] = useState<string>("");
  const [suggestion, setSuggestion] = useState<string>("archive.tar");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [savingEntries, setSavingEntries] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  // Both Start and Save-the-entry-list pay the same collect-and-reorder
  // cost, which can take a noticeable moment on a large tree.
  const busy = starting || savingEntries;

  const set = (patch: Partial<OutputOptions>) =>
    onOptionsChange({ ...options, ...patch });

  useEffect(() => {
    api.suggestedOutputName().then(setSuggestion).catch(() => {});
    api
      .pathModeOptions()
      .then((m) => {
        setModes(m);
        // Two folders with the same name would silently merge, so a blocked
        // mode is corrected here rather than at the point of no return.
        const ok =
          options.pathMode === "foldersOnly"
            ? m.foldersOnly
            : options.pathMode === "commonRoot"
              ? m.commonRoot
              : true;
        if (!ok) {
          set({ pathMode: m.commonRoot ? "commonRoot" : "fullPath" });
        }
      })
      .catch((e) => setError(String(e)));
    // Runs once: the tree cannot change while this dialog is up.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Names differ per mode, and a name past 100 bytes costs an extra header
  // block, so the predicted size is genuinely mode-specific.
  useEffect(() => {
    let live = true;
    setEst(null);
    api
      .estimate(options.pathMode)
      .then((e) => live && setEst(e))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [options.pathMode]);

  // Keep the suggested name's extension honest about the chosen compression.
  useEffect(() => {
    const { ext } = formatOf(options.compression);
    setSuggestion((s) => s.replace(EXT_RE, "") + "." + ext);
    setOutPath((p) => (p ? p.replace(EXT_RE, "") + "." + ext : p));
  }, [options.compression]);

  const pick = async () => {
    const format = formatOf(options.compression);
    const chosen = await save({
      title: t("build.saveAs"),
      defaultPath: suggestion,
      filters: [{ name: t(format.filter), extensions: [format.ext] }],
    });
    if (chosen) setOutPath(chosen);
  };

  const start = async () => {
    if (!outPath) {
      setError(t("build.pickOutputFirst"));
      return;
    }
    setStarting(true);
    setError(null);
    try {
      await api.startArchive(outPath, options);
      onStarted(outPath);
    } catch (e) {
      setError(String(e));
      setStarting(false);
    }
  };

  const saveEntries = async () => {
    const stem = (outPath || suggestion).replace(EXT_RE, "");
    const chosen = await save({
      title: t("build.saveEntriesTitle"),
      defaultPath: `${stem}-entries.txt`,
      filters: [{ name: t("build.filterText"), extensions: ["txt"] }],
    });
    if (!chosen) return;
    setError(null);
    setNote(null);
    setSavingEntries(true);
    try {
      const n = await api.saveEntryList(chosen, options.pathMode);
      setNote(t("build.savedEntries", { count: n }));
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingEntries(false);
    }
  };

  const gz = options.compression === "gzip";
  const sevenz = options.compression === "7z";
  // Only an uncompressed tar has a size that can be predicted exactly.
  const compressed = options.compression !== "none";

  const blocked = (mode: PathMode) =>
    mode === "foldersOnly"
      ? (modes && !modes.foldersOnly
          ? (modes.foldersOnlyReason ?? t("build.notUsable"))
          : null)
      : mode === "commonRoot"
        ? (modes && !modes.commonRoot
            ? (modes.commonRootReason ?? t("build.notUsable"))
            : null)
        : null;

  const sample =
    modes === null
      ? null
      : options.pathMode === "foldersOnly"
        ? modes.foldersOnlySample
        : options.pathMode === "commonRoot"
          ? modes.commonRootSample
          : modes.fullPathSample;

  const chosen = MODES.find((m) => m.mode === options.pathMode);
  const reason = blocked(options.pathMode);

  return (
    <Modal title={t("build.title")} onClose={onClose}>
      {/* A native fieldset disables and dims every field for free while the
          collect-and-reorder pass runs, whether Start or the entry-list save
          triggered it — Cancel/actions live outside it and stay clickable. */}
      <fieldset className="build__fields" disabled={busy}>
      <div className="field">
        <label className="field__label" htmlFor="outpath">
          {t("build.output")}
        </label>
        <div className="field__row">
          <input
            id="outpath"
            className="input input--path"
            value={outPath}
            placeholder={suggestion}
            spellCheck={false}
            onChange={(e) => setOutPath(e.target.value)}
          />
          <button type="button" className="btn" onClick={pick}>
            {t("build.browse")}
          </button>
        </div>
      </div>

      <div className="field">
        <span className="field__label">{t("build.paths")}</span>
        <div className="seg seg--wide">
          {MODES.map((m) => {
            const why = blocked(m.mode);
            return (
              <button
                key={m.mode}
                type="button"
                className={`seg__btn ${options.pathMode === m.mode ? "seg__btn--on" : ""}`}
                disabled={why !== null}
                title={why ?? t(m.blurb)}
                onClick={() => set({ pathMode: m.mode })}
              >
                {t(m.label)}
              </button>
            );
          })}
        </div>
        <p className="field__hint">
          {reason ? (
            <span className="field__hint--warn">{t("build.unavailable", { reason })}</span>
          ) : (
            <>
              {chosen && t(chosen.blurb)}
              {sample && (
                <>
                  {" "}
                  <code className="sample">{sample}</code>
                </>
              )}
            </>
          )}
        </p>
      </div>

      <div className="field">
        <span className="field__label">{t("build.compression")}</span>
        <div className="seg seg--wide">
          {FORMATS.map((f) => (
            <button
              key={f.c}
              type="button"
              className={`seg__btn ${options.compression === f.c ? "seg__btn--on" : ""}`}
              onClick={() => set({ compression: f.c })}
            >
              {t(f.label)}
            </button>
          ))}
        </div>
        {gz && (
          <div className="field__row field__row--slider">
            <label className="slider__label" htmlFor="gzlevel">
              {t("build.level", { level: options.gzipLevel })}
            </label>
            <input
              id="gzlevel"
              className="slider"
              type="range"
              min={1}
              max={9}
              value={options.gzipLevel}
              onChange={(e) => set({ gzipLevel: Number(e.target.value) })}
            />
            <span className="slider__ends">
              <span>{t("build.faster")}</span>
              <span>{t("build.smaller")}</span>
            </span>
          </div>
        )}
        {sevenz && (
          <>
            <div className="field__row field__row--slider">
              <label className="slider__label" htmlFor="szlevel">
                {t("build.level", { level: options.sevenzLevel })}
              </label>
              <input
                id="szlevel"
                className="slider"
                type="range"
                min={0}
                max={9}
                value={options.sevenzLevel}
                onChange={(e) => set({ sevenzLevel: Number(e.target.value) })}
              />
              <span className="slider__ends">
                <span>{t("build.faster")}</span>
                <span>{t("build.smaller")}</span>
              </span>
            </div>
            <label className="setting setting--check" htmlFor="szsolid">
              <input
                id="szsolid"
                type="checkbox"
                className="check"
                checked={options.sevenzSolid}
                onChange={(e) => set({ sevenzSolid: e.target.checked })}
              />
              <span className="setting__body">
                <span className="setting__name">{t("build.solid")}</span>
                <span className="setting__help">{t("build.solidHint")}</span>
              </span>
            </label>
          </>
        )}
      </div>

      <div className="titleblock">
        <div className="titleblock__head">{t("build.spec")}</div>
        <dl className="titleblock__grid">
          <Field
            label={t("build.entries")}
            value={est ? fmt.count(est.entries) : "—"}
            action={
              <button
                type="button"
                className="btn btn--icon"
                title={t("build.saveEntries")}
                aria-label={t("build.saveEntries")}
                disabled={!est || est.entries === 0}
                onClick={() => void saveEntries()}
              >
                <SaveMark />
              </button>
            }
          />
          <Field label={t("build.files")} value={est ? fmt.count(est.files) : "—"} />
          <Field label={t("build.content")} value={est ? fmt.bytes(est.payloadBytes) : "—"} />
          <Field
            label={compressed ? t("build.maxSize") : t("build.archiveSize")}
            value={est ? fmt.bytes(est.tarBytes) : "—"}
            note={compressed ? t("build.beforeCompression") : t("build.exact")}
            strong
          />
        </dl>
      </div>
      </fieldset>

      {busy && (
        <div className="build__busy">
          <span className="meter meter--busy" aria-hidden="true">
            <span className="meter__fill" />
          </span>
          <p className="build__busy-label">{t("build.preparing")}</p>
        </div>
      )}

      {note && <p className="alert alert--info">{note}</p>}
      {error && <p className="alert alert--error">{error}</p>}

      <div className="modal__actions">
        <button type="button" className="btn" onClick={onClose}>
          {t("build.cancel")}
        </button>
        <button
          type="button"
          className="btn btn--go"
          onClick={start}
          disabled={busy || !est || est.entries === 0}
        >
          {starting ? t("build.starting") : t("build.start")}
        </button>
      </div>
    </Modal>
  );
}

function Field({
  label,
  value,
  note,
  strong,
  action,
}: {
  label: string;
  value: string;
  note?: string;
  strong?: boolean;
  action?: ReactNode;
}) {
  return (
    <div className={`tbfield ${strong ? "tbfield--strong" : ""}`}>
      <dt className="tbfield__label">{label}</dt>
      <dd className="tbfield__value">
        {value}
        {note && <span className="tbfield__note">{note}</span>}
        {action}
      </dd>
    </div>
  );
}

function SaveMark() {
  return (
    <svg
      viewBox="0 0 14 14"
      width="12"
      height="12"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M2.5 2.5h7l2 2v7h-9z" />
      <path d="M4.5 2.5v3.5h4v-3.5" />
      <path d="M4.5 11.5v-3.2h5v3.2" />
    </svg>
  );
}

export function Modal({
  title,
  onClose,
  children,
  wide,
  wider,
  escapes = true,
}: {
  title: string;
  onClose?: () => void;
  children: React.ReactNode;
  wide?: boolean;
  /** Wider still — for content that genuinely needs the room, like the licence text. */
  wider?: boolean;
  /**
   * Set false while a modal of your own is open on top: both would otherwise
   * see the same keydown and Escape would dismiss two dialogs at once.
   */
  escapes?: boolean;
}) {
  useEffect(() => {
    if (!onClose || !escapes) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, escapes]);

  return (
    <div className="scrim" onClick={escapes ? onClose : undefined}>
      <div
        className={`modal ${wide ? "modal--wide" : ""} ${wider ? "modal--wider" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal__head">
          <h2 className="modal__title">{title}</h2>
          {onClose && (
            <button type="button" className="btn btn--icon" onClick={onClose} aria-label="Close">
              <svg viewBox="0 0 14 14" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
                <path d="M3 3l8 8M11 3l-8 8" />
              </svg>
            </button>
          )}
        </div>
        <div className="modal__body">{children}</div>
      </div>
    </div>
  );
}
