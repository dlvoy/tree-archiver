import type { CheckState } from "../api/commands";

/**
 * Three-state checkbox drawn as SVG rather than a native input, so the
 * partial state is a real mark instead of a platform-styled dash.
 */
export function TriCheckbox({
  state,
  onToggle,
  label,
}: {
  state: CheckState;
  onToggle: () => void;
  label: string;
}) {
  return (
    <button
      type="button"
      className={`tick tick--${state}`}
      role="checkbox"
      aria-checked={state === "partial" ? "mixed" : state === "checked"}
      aria-label={label}
      tabIndex={-1}
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
    >
      <svg viewBox="0 0 14 14" width="14" height="14" aria-hidden="true">
        <rect
          className="tick__box"
          x="0.75"
          y="0.75"
          width="12.5"
          height="12.5"
          rx="1.5"
        />
        {state === "checked" && (
          <path
            className="tick__mark"
            d="M3.4 7.2l2.5 2.5 4.7-5.1"
            fill="none"
            strokeWidth="1.75"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        )}
        {state === "partial" && (
          <rect className="tick__mark tick__mark--partial" x="3.5" y="6.25" width="7" height="1.5" rx="0.75" />
        )}
      </svg>
    </button>
  );
}
