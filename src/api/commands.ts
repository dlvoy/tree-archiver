/**
 * Typed wrappers over the Rust command surface.
 *
 * Every filesystem operation lives on the Rust side; nothing here touches
 * files directly. The tree is fetched one node's children at a time.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

export type NodeId = number;
export type CheckState = "checked" | "unchecked" | "partial";
export type NodeKind = "dir" | "file" | "filesGroup" | "syntheticRoot";
export type SortBy = "name" | "size";
export type SortDir = "asc" | "desc";
export type Compression = "none" | "gzip";
/** How much of a file's original path is kept inside the archive. */
export type PathMode = "foldersOnly" | "commonRoot" | "fullPath";
export type ThemePreference = "system" | "light" | "dark";
export type LanguagePreference = "system" | "en" | "pl" | "de";

export interface NodeView {
  id: NodeId;
  name: string;
  kind: NodeKind;
  /** A pass-through directory that was never enumerated. */
  spine: boolean;
  ext: string | null;
  hasChildren: boolean;
  check: CheckState;
  selSize: number;
  totalSize: number;
  selFiles: number;
  totalFiles: number;
  path: string | null;
}

export interface Summary {
  selFiles: number;
  selBytes: number;
  totalFiles: number;
  totalBytes: number;
  sources: number;
  issues: number;
}

export interface ScanIssue {
  path: string;
  message: string;
}

export interface SortKey {
  by: SortBy;
  dir: SortDir;
}

export interface TreeUpdate {
  root: NodeView | null;
  summary: Summary;
  issues: ScanIssue[];
  sort: SortKey;
}

export interface CheckUpdate {
  node: NodeView;
  ancestors: NodeView[];
  summary: Summary;
}

export interface OutputOptions {
  compression: Compression;
  gzipLevel: number;
  pathMode: PathMode;
}

export interface Settings {
  version: number;
  theme: ThemePreference;
  language: LanguagePreference;
  sort: SortKey;
  output: OutputOptions;
}

/**
 * Which layouts are usable, and what an entry looks like in each. A mode is
 * unavailable when two folders would land on the same name.
 */
export interface PathModeOptions {
  foldersOnly: boolean;
  commonRoot: boolean;
  foldersOnlyReason: string | null;
  commonRootReason: string | null;
  foldersOnlySample: string | null;
  commonRootSample: string | null;
  fullPathSample: string | null;
}

export interface Branch {
  key: string;
  id: NodeId;
  children: NodeView[];
}

export interface RestoredView {
  branches: Branch[];
  selected: NodeId | null;
}

export interface Estimate {
  entries: number;
  files: number;
  payloadBytes: number;
  tarBytes: number;
}

export interface UnresolvedRule {
  path: string;
  reason: string;
}

export interface LoadPlanResult {
  tree: TreeUpdate;
  unresolved: UnresolvedRule[];
  output: OutputOptions;
}

export interface ScanProgress {
  dirs: number;
  files: number;
  bytes: number;
  current: string;
}

export interface ArchiveProgress {
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
  bps: number;
  etaSecs: number | null;
}

export type LogLevel = "info" | "warn" | "error";

export interface LogEntry {
  ts: string;
  level: LogLevel;
  path: string;
  /** Translation key, so the line can be shown in the chosen language. */
  key: string;
  /** Values the key interpolates. */
  args: Record<string, string>;
  /** English rendering — what the saved log file holds, and the fallback. */
  message: string;
}

export interface ArchiveSummary {
  ok: boolean;
  cancelled: boolean;
  outPath: string;
  bytesWritten: number;
  filesWritten: number;
  dirsWritten: number;
  skipped: number;
  errors: number;
  elapsedSecs: number;
}

// ---------------------------------------------------------------- commands

export const addPaths = (paths: string[]) =>
  invoke<TreeUpdate>("add_paths", { paths });

export const removeNode = (id: NodeId) =>
  invoke<TreeUpdate>("remove_node", { id });

export const clearAll = () => invoke<TreeUpdate>("clear_all");

export const getChildren = (id: NodeId) =>
  invoke<NodeView[]>("get_children", { id });

export const setSort = (by: SortBy, dir: SortDir) =>
  invoke<void>("set_sort", { by, dir });

export const setChecked = (id: NodeId, checked: boolean) =>
  invoke<CheckUpdate>("set_checked", { id, checked });

export const setAllChecked = (checked: boolean) =>
  invoke<TreeUpdate>("set_all_checked", { checked });

export const getState = () => invoke<TreeUpdate>("get_state");

export const getIssues = () => invoke<ScanIssue[]>("get_issues");

export const savePlan = (path: string) => invoke<void>("save_plan", { path });

export const loadPlan = (path: string) =>
  invoke<LoadPlanResult>("load_plan", { path });

export const setOutput = (options: OutputOptions) =>
  invoke<void>("set_output", { options });

export const estimate = (mode?: PathMode) =>
  invoke<Estimate>("estimate", { mode: mode ?? null });

export const pathModeOptions = () =>
  invoke<PathModeOptions>("path_mode_options");

/**
 * Re-opens branches by path after a rebuild renumbered every node. One round
 * trip regardless of how many are open.
 */
export const restoreView = (expanded: string[], selected: string | null) =>
  invoke<RestoredView>("restore_view", { expanded, selected });

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<void>("save_settings", { settings });

export const suggestedOutputName = () =>
  invoke<string>("suggested_output_name");

export const startArchive = (outPath: string, options: OutputOptions) =>
  invoke<void>("start_archive", { request: { outPath, options } });

export const cancelArchive = () => invoke<void>("cancel_archive");

export const cancelScan = () => invoke<void>("cancel_scan");

export const saveLog = (path: string) => invoke<number>("save_log", { path });

// ------------------------------------------------------------ explorer menu

export const explorerStatus = () => invoke<boolean>("explorer_status");

/** `label` is the menu text, already translated. */
export const explorerInstall = (label: string) =>
  invoke<boolean>("explorer_install", { label });

export const explorerUninstall = () => invoke<boolean>("explorer_uninstall");

// ---------------------------------------------------------------- events

export const onScanProgress = (fn: (p: ScanProgress) => void) =>
  listen<ScanProgress>("scan://progress", (e) => fn(e.payload));

export const onScanDone = (fn: () => void) =>
  listen<null>("scan://done", () => fn());

export const onArchiveProgress = (fn: (p: ArchiveProgress) => void) =>
  listen<ArchiveProgress>("archive://progress", (e) => fn(e.payload));

/**
 * Log lines arrive in batches. Every file added produces one, so a message per
 * line would swamp the bridge on a large archive.
 */
export const onArchiveLog = (fn: (entries: LogEntry[]) => void) =>
  listen<LogEntry[]>("archive://log", (e) => fn(e.payload));

/** A rebuild the window did not ask for — paths arriving from File Explorer. */
export const onTreeUpdated = (fn: (u: TreeUpdate) => void) =>
  listen<TreeUpdate>("tree://updated", (e) => fn(e.payload));

export const onTreeError = (fn: (message: string) => void) =>
  listen<string>("tree://error", (e) => fn(e.payload));

export const onTreeScanning = (fn: (busy: boolean) => void) =>
  listen<boolean>("tree://scanning", (e) => fn(e.payload));

export const onArchiveDone = (fn: (s: ArchiveSummary) => void) =>
  listen<ArchiveSummary>("archive://done", (e) => fn(e.payload));

/**
 * Native drag and drop. Tauri delivers real filesystem paths here, which the
 * HTML5 drop API cannot provide.
 */
export function onFileDrop(handlers: {
  over: () => void;
  leave: () => void;
  drop: (paths: string[]) => void;
}): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "enter" || p.type === "over") handlers.over();
    else if (p.type === "leave") handlers.leave();
    else if (p.type === "drop") {
      handlers.leave();
      handlers.drop(p.paths);
    }
  });
}
