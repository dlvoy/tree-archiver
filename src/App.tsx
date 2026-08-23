import { useCallback, useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import * as api from "./api/commands";
import type {
  ArchiveProgress,
  ArchiveSummary,
  LogEntry,
  ScanProgress,
  SortBy,
  SortDir,
  UnresolvedRule,
} from "./api/commands";
import { useTree } from "./store/tree";
import { Toolbar } from "./components/Toolbar";
import { TreeView } from "./components/TreeView";
import { StatusBar } from "./components/StatusBar";
import { ArchiveDialog, Modal } from "./components/ArchiveDialog";
import { ProgressView } from "./components/ProgressView";
import * as fmt from "./lib/format";

type Theme = "light" | "dark";
type Stage =
  | { at: "design" }
  | { at: "configure" }
  | { at: "running"; outPath: string };

export default function App() {
  const store = useTree();
  const [stage, setStage] = useState<Stage>({ at: "design" });
  const [dragging, setDragging] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [progress, setProgress] = useState<ArchiveProgress | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [summary, setSummary] = useState<ArchiveSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [unresolved, setUnresolved] = useState<UnresolvedRule[] | null>(null);
  const [showIssues, setShowIssues] = useState(false);
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem("theme") as Theme) ?? "dark",
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("theme", theme);
    } catch {
      // A locked-down profile can refuse storage; the theme still applies.
    }
  }, [theme]);

  const ingest = useCallback(
    async (run: () => Promise<void>) => {
      setError(null);
      setScanning(true);
      try {
        await run();
      } catch (e) {
        setError(String(e));
      } finally {
        setScanning(false);
        setScanProgress(null);
      }
    },
    [],
  );

  const addPaths = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      return ingest(async () => {
        const update = await api.addPaths(paths);
        store.applyTreeUpdate(update);
      });
    },
    [ingest, store],
  );

  // --- events from Rust -----------------------------------------------

  useEffect(() => {
    const un = [
      api.onScanProgress(setScanProgress),
      api.onArchiveProgress(setProgress),
      api.onArchiveLog((e) => setLog((l) => [...l, e])),
      api.onArchiveDone(setSummary),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    const un = api.onFileDrop({
      over: () => setDragging(true),
      leave: () => setDragging(false),
      drop: (paths) => void addPaths(paths),
    });
    return () => {
      un.then((f) => f());
    };
  }, [addPaths]);

  // --- toolbar actions -------------------------------------------------

  const pickFolders = async () => {
    const picked = await open({ directory: true, multiple: true, title: "Add folders" });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const pickFiles = async () => {
    const picked = await open({ multiple: true, title: "Add files" });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const removeSelected = async () => {
    if (store.selected === null) return;
    try {
      store.applyTreeUpdate(await api.removeNode(store.selected));
    } catch (e) {
      setError(String(e));
    }
  };

  const clearAll = async () => {
    store.applyTreeUpdate(await api.clearAll());
  };

  const checkAll = useCallback(
    async (checked: boolean) => {
      try {
        await store.checkAll(checked);
      } catch (e) {
        setError(String(e));
      }
    },
    [store],
  );

  const changeSort = async (by: SortBy, dir: SortDir) => {
    await store.changeSort(by, dir);
  };

  const savePlan = async () => {
    const path = await save({
      title: "Save archive plan",
      defaultPath: "archive-plan.json",
      filters: [{ name: "Archive plan", extensions: ["json"] }],
    });
    if (!path) return;
    try {
      await api.savePlan(path);
      setNotice(`Plan saved to ${fmt.shortenPath(path, 60)}`);
    } catch (e) {
      setError(String(e));
    }
  };

  const loadPlan = async () => {
    const path = await open({
      title: "Open archive plan",
      multiple: false,
      filters: [{ name: "Archive plan", extensions: ["json"] }],
    });
    if (!path || Array.isArray(path)) return;
    await ingest(async () => {
      const result = await api.loadPlan(path);
      store.applyTreeUpdate(result.tree);
      if (result.unresolved.length > 0) setUnresolved(result.unresolved);
    });
  };

  const beginArchive = () => {
    setLog([]);
    setProgress(null);
    setSummary(null);
    setStage({ at: "configure" });
  };

  const closeRun = () => {
    setStage({ at: "design" });
    setProgress(null);
    setSummary(null);
  };

  return (
    <div className={`app ${dragging ? "app--dropping" : ""}`}>
      <Toolbar
        sort={store.sort}
        hasTree={!!store.root}
        canRemove={store.selected !== null}
        scanning={scanning}
        scanProgress={scanProgress}
        theme={theme}
        onAddFolders={pickFolders}
        onAddFiles={pickFiles}
        onRemove={removeSelected}
        onClear={clearAll}
        onCollapseAll={store.collapseAll}
        onCheckAll={(c) => void checkAll(c)}
        onSort={(by, dir) => void changeSort(by, dir)}
        onSavePlan={savePlan}
        onLoadPlan={loadPlan}
        onCancelScan={() => void api.cancelScan()}
        onToggleTheme={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
      />

      <main className="main">
        <TreeView onAddFolders={pickFolders} onCheckAll={(c) => void checkAll(c)} />
      </main>

      <StatusBar
        summary={store.summary}
        issues={store.issues}
        onShowIssues={() => setShowIssues(true)}
        onArchive={beginArchive}
      />

      {dragging && (
        <div className="dropzone">
          <div className="dropzone__inner">
            <svg viewBox="0 0 48 34" width="96" height="68" aria-hidden="true">
              <path
                d="M2 6h13l4 5h27v21H2z"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeDasharray="4 3"
                strokeLinejoin="round"
              />
            </svg>
            <p>Release to stage these paths</p>
          </div>
        </div>
      )}

      {stage.at === "configure" && (
        <ArchiveDialog
          onClose={() => setStage({ at: "design" })}
          onStarted={(outPath) => setStage({ at: "running", outPath })}
        />
      )}

      {stage.at === "running" && (
        <ProgressView
          outPath={stage.outPath}
          progress={progress}
          log={log}
          summary={summary}
          onClose={closeRun}
        />
      )}

      {showIssues && (
        <Modal title="Paths that could not be read" onClose={() => setShowIssues(false)} wide>
          <p className="modal__lede">
            These were skipped while scanning. Everything else is staged as usual.
          </p>
          <ul className="issues">
            {store.issues.map((i, n) => (
              <li key={n} className="issue">
                <span className="issue__path">{i.path}</span>
                <span className="issue__msg">{i.message}</span>
              </li>
            ))}
          </ul>
        </Modal>
      )}

      {unresolved && (
        <Modal title="Some plan rules no longer apply" onClose={() => setUnresolved(null)}>
          <p className="modal__lede">
            The tree has changed since the plan was written. These rules were skipped;
            everything they referred to is included by default.
          </p>
          <ul className="issues">
            {unresolved.map((u, n) => (
              <li key={n} className="issue">
                <span className="issue__path">{u.path}</span>
                <span className="issue__msg">{u.reason}</span>
              </li>
            ))}
          </ul>
        </Modal>
      )}

      {(error || notice) && (
        <div className={`toast ${error ? "toast--error" : ""}`} role="status">
          <span>{error ?? notice}</span>
          <button
            type="button"
            className="btn btn--icon"
            aria-label="Dismiss"
            onClick={() => {
              setError(null);
              setNotice(null);
            }}
          >
            <svg viewBox="0 0 14 14" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
              <path d="M3 3l8 8M11 3l-8 8" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
