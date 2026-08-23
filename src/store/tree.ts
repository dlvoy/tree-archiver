/**
 * Tree state.
 *
 * The backend owns the real tree. This store keeps only what has actually been
 * looked at: nodes seen, children fetched, branches expanded. Flattening those
 * into visible rows is what the virtualizer renders.
 */
import { create } from "zustand";
import * as api from "../api/commands";
import type {
  NodeId,
  NodeView,
  ScanIssue,
  SortBy,
  SortDir,
  SortKey,
  Summary,
  TreeUpdate,
} from "../api/commands";

export interface Row {
  node: NodeView;
  depth: number;
  /** Which ancestor levels still have a sibling below, for the indent guides. */
  guides: boolean[];
}

interface TreeStore {
  root: NodeView | null;
  nodes: Map<NodeId, NodeView>;
  children: Map<NodeId, NodeId[]>;
  expanded: Set<NodeId>;
  loading: Set<NodeId>;
  summary: Summary | null;
  issues: ScanIssue[];
  sort: SortKey;
  selected: NodeId | null;
  busy: boolean;

  applyTreeUpdate: (u: TreeUpdate) => void;
  toggleExpand: (id: NodeId) => Promise<void>;
  expand: (id: NodeId) => Promise<void>;
  collapse: (id: NodeId) => void;
  collapseAll: () => void;
  toggleCheck: (id: NodeId) => Promise<void>;
  checkAll: (checked: boolean) => Promise<void>;
  select: (id: NodeId | null) => void;
  changeSort: (by: SortBy, dir: SortDir) => Promise<void>;
  setBusy: (b: boolean) => void;
  rows: () => Row[];
}

/** Everything cached beneath `id`, `id` excluded. */
function descendantIds(children: Map<NodeId, NodeId[]>, id: NodeId): NodeId[] {
  const out: NodeId[] = [];
  const stack = [...(children.get(id) ?? [])];
  while (stack.length) {
    const n = stack.pop()!;
    out.push(n);
    const kids = children.get(n);
    if (kids) stack.push(...kids);
  }
  return out;
}

export const useTree = create<TreeStore>((set, get) => ({
  root: null,
  nodes: new Map(),
  children: new Map(),
  expanded: new Set(),
  loading: new Set(),
  summary: null,
  issues: [],
  sort: { by: "name", dir: "asc" },
  selected: null,
  busy: false,

  setBusy: (busy) => set({ busy }),

  /**
   * A rebuild renumbers every node, so all caches are dropped. Expansion state
   * cannot survive it either — the ids it refers to no longer mean anything.
   */
  applyTreeUpdate: (u) => {
    const nodes = new Map<NodeId, NodeView>();
    if (u.root) nodes.set(u.root.id, u.root);
    set({
      root: u.root,
      nodes,
      children: new Map(),
      expanded: new Set(),
      loading: new Set(),
      summary: u.summary,
      issues: u.issues,
      sort: u.sort,
      selected: null,
    });
  },

  expand: async (id) => {
    const { children, expanded, loading } = get();
    if (expanded.has(id)) return;

    if (!children.has(id)) {
      set({ loading: new Set(loading).add(id) });
      const kids = await api.getChildren(id);
      set((s) => {
        const nextNodes = new Map(s.nodes);
        for (const k of kids) nextNodes.set(k.id, k);
        const nextChildren = new Map(s.children);
        nextChildren.set(
          id,
          kids.map((k) => k.id),
        );
        const nextLoading = new Set(s.loading);
        nextLoading.delete(id);
        return { nodes: nextNodes, children: nextChildren, loading: nextLoading };
      });
    }
    set((s) => ({ expanded: new Set(s.expanded).add(id) }));
  },

  collapse: (id) =>
    set((s) => {
      const next = new Set(s.expanded);
      next.delete(id);
      return { expanded: next };
    }),

  collapseAll: () => set({ expanded: new Set() }),

  toggleExpand: async (id) => {
    const { expanded, collapse, expand } = get();
    if (expanded.has(id)) collapse(id);
    else await expand(id);
  },

  /**
   * Flips one node. The backend returns the node plus its ancestors; the
   * subtree below is dropped from the cache rather than patched, so a folder
   * holding a hundred thousand files costs one small message either way.
   */
  toggleCheck: async (id) => {
    const node = get().nodes.get(id);
    if (!node) return;
    const nextChecked = node.check !== "checked";
    const update = await api.setChecked(id, nextChecked);

    set((s) => {
      const nodes = new Map(s.nodes);
      const children = new Map(s.children);
      const expanded = new Set(s.expanded);

      for (const d of descendantIds(s.children, id)) {
        nodes.delete(d);
        children.delete(d);
        expanded.delete(d);
      }
      children.delete(id);
      expanded.delete(id);

      nodes.set(update.node.id, update.node);
      for (const a of update.ancestors) nodes.set(a.id, a);

      // Ancestors arrive nearest-parent first, so the last one is the root.
      const last = update.ancestors.at(-1);
      const root =
        update.node.id === s.root?.id ? update.node : (last ?? s.root);

      return { nodes, children, expanded, summary: update.summary, root };
    });
  },

  /**
   * Checks or clears the whole tree. No rebuild happens on the Rust side, so
   * node ids stay valid and the user keeps their place — only the rows already
   * on screen need re-reading.
   */
  checkAll: async (checked) => {
    const update = await api.setAllChecked(checked);
    const open = [...get().children.keys()];
    const fetched = await Promise.all(
      open.map(async (id) => [id, await api.getChildren(id)] as const),
    );

    set((s) => {
      const nodes = new Map(s.nodes);
      const children = new Map(s.children);
      if (update.root) nodes.set(update.root.id, update.root);
      for (const [id, kids] of fetched) {
        for (const k of kids) nodes.set(k.id, k);
        children.set(
          id,
          kids.map((k) => k.id),
        );
      }
      return { nodes, children, root: update.root, summary: update.summary };
    });
  },

  select: (selected) => set({ selected }),

  changeSort: async (by, dir) => {
    await api.setSort(by, dir);
    // Ordering is decided in Rust, so every fetched child list is now stale.
    set((s) => ({ sort: { by, dir }, children: new Map(), expanded: new Set(s.expanded) }));
    // Re-fetch whatever is currently open, deepest-last so parents fill first.
    const open = [...get().expanded];
    set({ expanded: new Set() });
    for (const id of open) {
      if (get().nodes.has(id)) await get().expand(id);
    }
  },

  /** Flattens the open branches into the list the virtualizer draws. */
  rows: () => {
    const { root, nodes, children, expanded } = get();
    if (!root) return [];
    const out: Row[] = [];

    const walk = (id: NodeId, depth: number, guides: boolean[]) => {
      const node = nodes.get(id);
      if (!node) return;
      out.push({ node, depth, guides });
      if (!expanded.has(id)) return;
      const kids = children.get(id);
      if (!kids) return;
      kids.forEach((kid, i) => {
        walk(kid, depth + 1, [...guides, i < kids.length - 1]);
      });
    };

    walk(root.id, 0, []);
    return out;
  },
}));
