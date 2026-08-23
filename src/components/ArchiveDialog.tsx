import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import * as api from "../api/commands";
import type { Compression, Estimate, OutputOptions } from "../api/commands";
import * as fmt from "../lib/format";

/**
 * Pre-flight. The spec reads as a drawing's title block: fixed fields, fixed
 * order, so the same number is always in the same place.
 */
export function ArchiveDialog({
  onClose,
  onStarted,
}: {
  onClose: () => void;
  onStarted: (outPath: string) => void;
}) {
  const [est, setEst] = useState<Estimate | null>(null);
  const [options, setOptions] = useState<OutputOptions>({
    compression: "none",
    gzipLevel: 6,
  });
  const [outPath, setOutPath] = useState<string>("");
  const [suggestion, setSuggestion] = useState<string>("archive.tar");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    api.estimate().then(setEst).catch((e) => setError(String(e)));
    api.suggestedOutputName().then(setSuggestion).catch(() => {});
  }, []);

  // Keep the suggested name's extension honest about the chosen compression.
  useEffect(() => {
    const ext = options.compression === "gzip" ? "tar.gz" : "tar";
    setSuggestion((s) => s.replace(/\.tar(\.gz)?$/i, "") + "." + ext);
    setOutPath((p) => (p ? p.replace(/\.tar(\.gz)?$/i, "") + "." + ext : p));
  }, [options.compression]);

  const pick = async () => {
    const ext = options.compression === "gzip" ? "tar.gz" : "tar";
    const chosen = await save({
      title: "Save archive as",
      defaultPath: suggestion,
      filters: [{ name: options.compression === "gzip" ? "Gzipped tar" : "Tar archive", extensions: [ext] }],
    });
    if (chosen) setOutPath(chosen);
  };

  const start = async () => {
    if (!outPath) {
      setError("Choose where to save the archive first.");
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

  const gz = options.compression === "gzip";

  return (
    <Modal title="Build archive" onClose={onClose}>
      <div className="field">
        <label className="field__label" htmlFor="outpath">
          Output file
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
            Browse…
          </button>
        </div>
      </div>

      <div className="field">
        <span className="field__label">Compression</span>
        <div className="seg seg--wide">
          {(["none", "gzip"] as Compression[]).map((c) => (
            <button
              key={c}
              type="button"
              className={`seg__btn ${options.compression === c ? "seg__btn--on" : ""}`}
              onClick={() => setOptions((o) => ({ ...o, compression: c }))}
            >
              {c === "none" ? "None (.tar)" : "gzip (.tar.gz)"}
            </button>
          ))}
        </div>
        {gz && (
          <div className="field__row field__row--slider">
            <label className="slider__label" htmlFor="gzlevel">
              Level {options.gzipLevel}
            </label>
            <input
              id="gzlevel"
              className="slider"
              type="range"
              min={1}
              max={9}
              value={options.gzipLevel}
              onChange={(e) =>
                setOptions((o) => ({ ...o, gzipLevel: Number(e.target.value) }))
              }
            />
            <span className="slider__ends">
              <span>faster</span>
              <span>smaller</span>
            </span>
          </div>
        )}
      </div>

      <div className="titleblock">
        <div className="titleblock__head">Archive spec</div>
        <dl className="titleblock__grid">
          <Field label="Entries" value={est ? fmt.count(est.entries) : "—"} />
          <Field label="Files" value={est ? fmt.count(est.files) : "—"} />
          <Field label="Content" value={est ? fmt.bytes(est.payloadBytes) : "—"} />
          <Field
            label={gz ? "Max size" : "Archive size"}
            value={est ? fmt.bytes(est.tarBytes) : "—"}
            note={gz ? "before compression" : "exact"}
            strong
          />
        </dl>
      </div>

      {error && <p className="alert alert--error">{error}</p>}

      <div className="modal__actions">
        <button type="button" className="btn" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="btn btn--go"
          onClick={start}
          disabled={starting || !est || est.entries === 0}
        >
          {starting ? "Starting…" : "Start"}
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

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose?: () => void;
  children: React.ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    if (!onClose) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="scrim" onClick={onClose}>
      <div
        className={`modal ${wide ? "modal--wide" : ""}`}
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
