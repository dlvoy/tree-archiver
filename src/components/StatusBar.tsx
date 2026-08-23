import type { ScanIssue, Summary } from "../api/commands";
import { useT } from "../i18n/context";
import * as fmt from "../lib/format";

/**
 * The bottom rule restates, for the whole archive, the same proportion the
 * per-row meters show for each branch.
 */
export function StatusBar({
  summary,
  issues,
  onShowIssues,
  onArchive,
}: {
  summary: Summary | null;
  issues: ScanIssue[];
  onShowIssues: () => void;
  onArchive: () => void;
}) {
  const t = useT();
  const s = summary;
  const fraction = s && s.totalBytes > 0 ? s.selBytes / s.totalBytes : 0;
  const ready = !!s && s.selFiles > 0;

  return (
    <footer className="bar bar--bottom">
      <span className="total" aria-hidden="true">
        <span className="total__fill" style={{ width: `${fraction * 100}%` }} />
      </span>

      <div className="bar__row">
        {s ? (
          <div className="stats">
            <Stat label={t("status.sources")} value={fmt.count(s.sources)} />
            <Stat
              label={t("status.files")}
              value={t("status.ofFiles", {
                sel: fmt.count(s.selFiles),
                total: fmt.count(s.totalFiles),
              })}
            />
            <Stat
              label={t("status.selected")}
              value={fmt.bytes(s.selBytes)}
              sub={
                s.selBytes !== s.totalBytes
                  ? t("status.ofBytes", { total: fmt.bytes(s.totalBytes) })
                  : undefined
              }
              strong
            />
          </div>
        ) : (
          <div className="stats stats--idle">{t("status.idle")}</div>
        )}

        {issues.length > 0 && (
          <button type="button" className="chip chip--warn" onClick={onShowIssues}>
            {t("status.unreadable", { count: issues.length })}
          </button>
        )}

        <div className="bar__spacer" />

        <button
          type="button"
          className="btn btn--go"
          onClick={onArchive}
          disabled={!ready}
          title={ready ? t("status.archiveReady") : t("status.archiveEmpty")}
        >
          {t("status.archive")}
        </button>
      </div>
    </footer>
  );
}

function Stat({
  label,
  value,
  sub,
  strong,
}: {
  label: string;
  value: string;
  sub?: string;
  strong?: boolean;
}) {
  return (
    <div className={`stat ${strong ? "stat--strong" : ""}`}>
      <span className="stat__label">{label}</span>
      <span className="stat__value">
        {value}
        {sub && <span className="stat__sub">{sub}</span>}
      </span>
    </div>
  );
}
