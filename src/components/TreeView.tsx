import { useCallback, useEffect, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTree } from "../store/tree";
import { ROW_HEIGHT, TreeRow } from "./TreeRow";
import type { NodeId } from "../api/commands";

/**
 * Virtualized tree. Only open branches are ever fetched and only visible rows
 * are ever mounted, which is what keeps a few hundred thousand files scrolling
 * smoothly.
 */
export function TreeView({
  onAddFolders,
  onCheckAll,
}: {
  onAddFolders: () => void;
  onCheckAll: (checked: boolean) => void;
}) {
  const store = useTree();
  const rows = useMemo(
    () => store.rows(),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [store.root, store.nodes, store.children, store.expanded],
  );

  const parentRef = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const move = useCallback(
    (delta: number) => {
      const i = rows.findIndex((r) => r.node.id === store.selected);
      const next = rows[Math.min(rows.length - 1, Math.max(0, (i < 0 ? 0 : i) + delta))];
      if (next) {
        store.select(next.node.id);
        virtual.scrollToIndex(rows.indexOf(next), { align: "auto" });
      }
    },
    [rows, store, virtual],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;

      const id = store.selected;
      const row = rows.find((r) => r.node.id === id);

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          move(1);
          break;
        case "ArrowUp":
          e.preventDefault();
          move(-1);
          break;
        case "ArrowRight":
          if (!row) return;
          e.preventDefault();
          if (row.node.kind !== "file" && !store.expanded.has(row.node.id)) {
            void store.expand(row.node.id);
          } else {
            move(1);
          }
          break;
        case "ArrowLeft": {
          if (!row) return;
          e.preventDefault();
          if (store.expanded.has(row.node.id)) {
            store.collapse(row.node.id);
          } else {
            // Step out to the parent row.
            const above = rows.slice(0, rows.indexOf(row)).reverse();
            const parent = above.find((r) => r.depth === row.depth - 1);
            if (parent) store.select(parent.node.id);
          }
          break;
        }
        case " ":
          if (!row) return;
          e.preventDefault();
          void store.toggleCheck(row.node.id);
          break;
        case "a":
        case "A":
          if (e.ctrlKey) {
            e.preventDefault();
            // Ctrl+Shift+A clears the selection, Ctrl+A takes everything.
            onCheckAll(!e.shiftKey);
          }
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rows, store, move, onCheckAll]);

  if (!store.root) {
    return (
      <div className="tree tree--empty">
        <div className="empty">
          <svg viewBox="0 0 64 44" width="132" height="90" aria-hidden="true" className="empty__mark">
            <path
              d="M2 8h18l5 6h37v28H2z"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.25"
              strokeDasharray="4 3"
              strokeLinejoin="round"
            />
            <path d="M32 20v12M26 26h12" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
          </svg>
          <h2 className="empty__title">Nothing staged yet</h2>
          <p className="empty__body">
            Drop folders anywhere in this window, or add them from the toolbar.
            Everything you add starts fully selected — uncheck what you want to
            leave out.
          </p>
          <button type="button" className="btn btn--primary" onClick={onAddFolders}>
            Add folders
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="tree" ref={parentRef}>
      <div className="tree__canvas" style={{ height: virtual.getTotalSize() }}>
        {virtual.getVirtualItems().map((item) => {
          const row = rows[item.index];
          const id: NodeId = row.node.id;
          return (
            <div
              key={id}
              className="tree__slot"
              style={{ transform: `translateY(${item.start}px)` }}
            >
              <TreeRow
                row={row}
                selected={store.selected === id}
                expanded={store.expanded.has(id)}
                loading={store.loading.has(id)}
                onToggleExpand={() => void store.toggleExpand(id)}
                onToggleCheck={() => void store.toggleCheck(id)}
                onSelect={() => store.select(id)}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
