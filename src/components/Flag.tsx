import type { Lang } from "../i18n";

/**
 * Flags as inline SVG.
 *
 * Not emoji: Windows ships no flag glyphs, so 🇵🇱 renders as the letters "PL".
 * Not images either — the CSP allows no external assets, and three tiny
 * rectangles cost less than a data URI would.
 *
 * `muted` marks a flag that was chosen for the user rather than by them, so
 * the button still says which language is active while the tooltip says why.
 */
export function Flag({ lang, muted }: { lang: Lang; muted?: boolean }) {
  return (
    <svg
      viewBox="0 0 16 12"
      width="16"
      height="12"
      aria-hidden="true"
      className={`flag ${muted ? "flag--auto" : ""}`}
    >
      {lang === "pl" && (
        <>
          <rect x="0" y="0" width="16" height="6" fill="#fff" />
          <rect x="0" y="6" width="16" height="6" fill="#dc143c" />
        </>
      )}

      {lang === "de" && (
        <>
          <rect x="0" y="0" width="16" height="4" fill="#000" />
          <rect x="0" y="4" width="16" height="4" fill="#dd0000" />
          <rect x="0" y="8" width="16" height="4" fill="#ffce00" />
        </>
      )}

      {lang === "en" && (
        <>
          <rect x="0" y="0" width="16" height="12" fill="#012169" />
          {/* Saltires: white beneath, red on top, symmetric rather than
              offset — the offset is invisible at this size. */}
          <path d="M0 0L16 12M16 0L0 12" stroke="#fff" strokeWidth="3.4" />
          <path d="M0 0L16 12M16 0L0 12" stroke="#c8102e" strokeWidth="1.6" />
          {/* The cross of St George, white ground then red. */}
          <path d="M8 0v12M0 6h16" stroke="#fff" strokeWidth="4" />
          <path d="M8 0v12M0 6h16" stroke="#c8102e" strokeWidth="2.2" />
        </>
      )}

      <rect
        x="0.35"
        y="0.35"
        width="15.3"
        height="11.3"
        fill="none"
        stroke="currentColor"
        strokeOpacity="0.35"
        strokeWidth="0.7"
      />
    </svg>
  );
}
