import type { ScanIssue, Summary } from "../api/commands";
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
            <Stat label="Sources" value={fmt.count(s.sources)} />
            <Stat
              label="Files"
              value={`${fmt.count(s.selFiles)} of ${fmt.count(s.totalFiles)}`}
            />
            <Stat
              label="Selected"
              value={fmt.bytes(s.selBytes)}
              sub={s.selBytes !== s.totalBytes ? `of ${fmt.bytes(s.totalBytes)}` : undefined}
              strong
            />
          </div>
        ) : (
          <div className="stats stats--idle">No sources staged</div>
        )}

        {issues.length > 0 && (
          <button type="button" className="chip chip--warn" onClick={onShowIssues}>
            {fmt.count(issues.length)} {issues.length === 1 ? "path" : "paths"} could not be read
          </button>
        )}

        <div className="bar__spacer" />

        <button
          type="button"
          className="btn btn--go"
          onClick={onArchive}
          disabled={!ready}
          title={ready ? "Choose an output file and build the archive" : "Select something first"}
        >
          Archive…
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
