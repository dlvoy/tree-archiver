import { memo } from "react";
import type { Row } from "../store/tree";
import { FileIcon } from "./FileIcon";
import { TriCheckbox } from "./TriCheckbox";
import { useT } from "../i18n/context";
import * as fmt from "../lib/format";

export const ROW_HEIGHT = 26;
const INDENT = 15;

/**
 * One line of the tree.
 *
 * The size column carries the signature device: a hairline under the number
 * filled to the selected fraction. A full rule means the whole branch is going
 * in, an empty track means none of it, and everything between is legible
 * without reading the digits.
 */
export const TreeRow = memo(function TreeRow({
  row,
  selected,
  expanded,
  loading,
  onToggleExpand,
  onToggleCheck,
  onSelect,
}: {
  row: Row;
  selected: boolean;
  expanded: boolean;
  loading: boolean;
  onToggleExpand: () => void;
  onToggleCheck: () => void;
  onSelect: () => void;
}) {
  const t = useT();
  const { node, depth, guides } = row;
  const container = node.kind !== "file";
  const partial = node.check === "partial";
  const fraction =
    node.totalSize > 0 ? node.selSize / node.totalSize : node.check === "checked" ? 1 : 0;

  return (
    <div
      className={`row ${selected ? "row--selected" : ""} ${
        node.check === "unchecked" ? "row--dropped" : ""
      }`}
      style={{ height: ROW_HEIGHT }}
      onClick={onSelect}
      onDoubleClick={() => container && onToggleExpand()}
    >
      {/* Drafting gridlines: one rule per ancestor level that still continues. */}
      <div className="row__indent" style={{ width: depth * INDENT }}>
        {guides.map((on, i) => (
          <span
            key={i}
            className={`guide ${on ? "" : "guide--stub"}`}
            style={{ left: i * INDENT + 7 }}
          />
        ))}
      </div>

      <button
        type="button"
        className={`twisty ${expanded ? "twisty--open" : ""}`}
        tabIndex={-1}
        aria-hidden={!container}
        aria-label={expanded ? t("tree.collapse") : t("tree.expand")}
        disabled={!container || !node.hasChildren}
        onClick={(e) => {
          e.stopPropagation();
          onToggleExpand();
        }}
      >
        {container && node.hasChildren && (
          <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
            <path
              d="M3.5 1.5L7 5l-3.5 3.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
      </button>

      <TriCheckbox state={node.check} onToggle={onToggleCheck} label={node.name} />

      <span className="row__icon">
        <FileIcon kind={node.kind} ext={node.ext} spine={node.spine} open={expanded} />
      </span>

      <span className={`row__name ${node.kind === "filesGroup" ? "row__name--group" : ""}`}>
        {node.name}
      </span>

      {node.spine && <span className="row__tag">{t("tree.passThrough")}</span>}
      {loading && <span className="row__tag row__tag--live">{t("tree.reading")}</span>}
      {node.autoIgnore && (
        <span className="row__tag row__tag--auto" title={node.autoIgnore}>
          {t("tree.autoIgnored")}
        </span>
      )}

      <span className="row__size">
        <span className="row__figures">
          {partial ? (
            <>
              <span className="row__sel">{fmt.bytes(node.selSize)}</span>
              <span className="row__of">/</span>
              <span className="row__total">{fmt.bytes(node.totalSize)}</span>
            </>
          ) : (
            <span className="row__sel">
              {fmt.bytes(node.check === "checked" ? node.totalSize : node.selSize)}
            </span>
          )}
        </span>
        <span className="meter" aria-hidden="true">
          <span className="meter__fill" style={{ width: `${fraction * 100}%` }} />
        </span>
      </span>
    </div>
  );
});
