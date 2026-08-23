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
  /** Only what has been fetched; needed to key a `<files>` group by its parent. */
  parents: Map<NodeId, NodeId>;
  expanded: Set<NodeId>;
  /** The same branches named by path, which is all that survives a rebuild. */
  expandedKeys: Set<string>;
  loading: Set<NodeId>;
  summary: Summary | null;
  issues: ScanIssue[];
  sort: SortKey;
  selected: NodeId | null;
  busy: boolean;

  applyTreeUpdate: (u: TreeUpdate) => Promise<void>;
  toggleExpand: (id: NodeId) => Promise<void>;
  expand: (id: NodeId) => Promise<void>;
  collapse: (id: NodeId) => void;
  collapseAll: () => void;
  toggleCheck: (id: NodeId) => Promise<void>;
  checkAll: (checked: boolean) => Promise<void>;
  select: (id: NodeId | null) => void;
  changeSort: (by: SortBy, dir: SortDir) => Promise<void>;
  adoptSort: (sort: SortKey) => void;
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

/** Separator in composite keys; cannot occur in a Windows path. */
const NUL = String.fromCharCode(0);

/**
 * The key a node keeps across a rebuild. Must match `view_key` in
 * `commands.rs`: the display path, except for the two node kinds that have no
 * path of their own.
 */
function keyOf(
  nodes: Map<NodeId, NodeView>,
  parents: Map<NodeId, NodeId>,
  id: NodeId,
): string | null {
  const n = nodes.get(id);
  if (!n) return null;
  if (n.kind === "syntheticRoot") return NUL + "sources";
  if (n.kind === "filesGroup") {
    const parent = parents.get(id);
    const path = parent === undefined ? null : (nodes.get(parent)?.path ?? null);
    return path === null ? null : path + NUL + "<files>";
  }
  return n.path;
}

export const useTree = create<TreeStore>((set, get) => ({
  root: null,
  nodes: new Map(),
  children: new Map(),
  parents: new Map(),
  expanded: new Set(),
  expandedKeys: new Set(),
  loading: new Set(),
  summary: null,
  issues: [],
  sort: { by: "name", dir: "asc" },
  selected: null,
  busy: false,

  setBusy: (busy) => set({ busy }),

  adoptSort: (sort) => set({ sort }),

  /**
   * A rebuild renumbers every node, so all id-keyed caches are dropped. The
   * open branches are then asked for again by path, which is the one thing a
   * rebuild leaves intact — otherwise adding a folder would collapse the tree
   * the user just finished arranging.
   */
  applyTreeUpdate: async (u) => {
    const { expandedKeys, nodes: prevNodes, parents: prevParents, selected } = get();
    const wantOpen = [...expandedKeys];
    const wantSelected =
      selected === null ? null : keyOf(prevNodes, prevParents, selected);

    const fresh = new Map<NodeId, NodeView>();
    if (u.root) fresh.set(u.root.id, u.root);
    set({
      root: u.root,
      nodes: fresh,
      children: new Map(),
      parents: new Map(),
      expanded: new Set(),
      expandedKeys: new Set(),
      loading: new Set(),
      summary: u.summary,
      issues: u.issues,
      sort: u.sort,
      selected: null,
    });

    if (!u.root || (wantOpen.length === 0 && wantSelected === null)) return;

    let view;
    try {
      view = await api.restoreView(wantOpen, wantSelected);
    } catch {
      return; // losing the expansion is a far smaller failure than losing the tree
    }

    set((s) => {
      const nodes = new Map(s.nodes);
      const children = new Map(s.children);
      const parents = new Map(s.parents);
      const expanded = new Set(s.expanded);
      const keys = new Set(s.expandedKeys);
      for (const b of view.branches) {
        for (const kid of b.children) {
          nodes.set(kid.id, kid);
          parents.set(kid.id, b.id);
        }
        children.set(
          b.id,
          b.children.map((k) => k.id),
        );
        expanded.add(b.id);
        keys.add(b.key);
      }
      return {
        nodes,
        children,
        parents,
        expanded,
        expandedKeys: keys,
        selected: view.selected,
      };
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
        const nextParents = new Map(s.parents);
        for (const k of kids) {
          nextNodes.set(k.id, k);
          nextParents.set(k.id, id);
        }
        const nextChildren = new Map(s.children);
        nextChildren.set(
          id,
          kids.map((k) => k.id),
        );
        const nextLoading = new Set(s.loading);
        nextLoading.delete(id);
        return {
          nodes: nextNodes,
          children: nextChildren,
          parents: nextParents,
          loading: nextLoading,
        };
      });
    }
    set((s) => {
      const key = keyOf(s.nodes, s.parents, id);
      return {
        expanded: new Set(s.expanded).add(id),
        expandedKeys:
          key === null ? s.expandedKeys : new Set(s.expandedKeys).add(key),
      };
    });
  },

  collapse: (id) =>
    set((s) => {
      const expanded = new Set(s.expanded);
      expanded.delete(id);
      const key = keyOf(s.nodes, s.parents, id);
      const keys = new Set(s.expandedKeys);
      if (key !== null) keys.delete(key);
      return { expanded, expandedKeys: keys };
    }),

  collapseAll: () => set({ expanded: new Set(), expandedKeys: new Set() }),

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
      const parents = new Map(s.parents);
      const expanded = new Set(s.expanded);
      const keys = new Set(s.expandedKeys);

      const shut = (n: NodeId) => {
        const key = keyOf(s.nodes, s.parents, n);
        if (key !== null) keys.delete(key);
        children.delete(n);
        expanded.delete(n);
      };

      for (const d of descendantIds(s.children, id)) {
        shut(d);
        nodes.delete(d);
        parents.delete(d);
      }
      shut(id);

      nodes.set(update.node.id, update.node);
      for (const a of update.ancestors) nodes.set(a.id, a);

      // Ancestors arrive nearest-parent first, so the last one is the root.
      const last = update.ancestors.at(-1);
      const root =
        update.node.id === s.root?.id ? update.node : (last ?? s.root);

      return {
        nodes,
        children,
        parents,
        expanded,
        expandedKeys: keys,
        summary: update.summary,
        root,
      };
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
      const parents = new Map(s.parents);
      if (update.root) nodes.set(update.root.id, update.root);
      for (const [id, kids] of fetched) {
        for (const k of kids) {
          nodes.set(k.id, k);
          parents.set(k.id, id);
        }
        children.set(
          id,
          kids.map((k) => k.id),
        );
      }
      return { nodes, children, parents, root: update.root, summary: update.summary };
    });
  },

  select: (selected) => set({ selected }),

  changeSort: async (by, dir) => {
    await api.setSort(by, dir);
    // Ordering is decided in Rust, so every fetched child list is now stale.
    // Ids survive a re-sort, so the same branches are simply read again.
    const open = [...get().expanded];
    set({ sort: { by, dir }, children: new Map(), expanded: new Set() });
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
