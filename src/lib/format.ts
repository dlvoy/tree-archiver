/** Binary units, matching what Windows file managers report. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let v = n / 1024;
  let u = 0;
  while (v >= 1024 && u + 1 < units.length) {
    v /= 1024;
    u += 1;
  }
  if (v >= 100) return `${v.toFixed(0)} ${units[u]}`;
  if (v >= 10) return `${v.toFixed(1)} ${units[u]}`;
  return `${v.toFixed(2)} ${units[u]}`;
}

export function count(n: number): string {
  return n.toLocaleString();
}

/** Compact duration for ETA and elapsed readouts. */
export function duration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !isFinite(secs)) return "—";
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function rate(bps: number): string {
  return bps > 0 ? `${bytes(bps)}/s` : "—";
}

/** Local wall-clock time from an ISO-8601 UTC stamp, for log rows. */
export function clockTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  return d.toLocaleTimeString(undefined, { hour12: false });
}

/**
 * Middle-truncates a path so both the drive and the file name stay readable.
 */
export function shortenPath(p: string, max = 64): string {
  if (p.length <= max) return p;
  const keepEnd = Math.floor(max * 0.6);
  const keepStart = max - keepEnd - 1;
  return `${p.slice(0, keepStart)}…${p.slice(-keepEnd)}`;
}
