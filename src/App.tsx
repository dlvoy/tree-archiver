import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import * as api from "./api/commands";
import type {
  ArchiveProgress,
  ArchiveSummary,
  LanguagePreference,
  LogEntry,
  OutputOptions,
  ScanProgress,
  Settings,
  SortBy,
  SortDir,
  ThemePreference,
  UnresolvedRule,
} from "./api/commands";
import { useTree } from "./store/tree";
import { LangContext } from "./i18n/context";
import { LANGS, resolveLang, translator, type Lang } from "./i18n";
import { Toolbar } from "./components/Toolbar";
import { TreeView } from "./components/TreeView";
import { StatusBar } from "./components/StatusBar";
import { ArchiveDialog, Modal } from "./components/ArchiveDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { ProgressView } from "./components/ProgressView";
import * as fmt from "./lib/format";

type Stage =
  | { at: "design" }
  | { at: "configure" }
  | { at: "running"; outPath: string };

/** Matches `Settings::default()` in Rust; replaced as soon as the file loads. */
const FALLBACK: Settings = {
  version: 1,
  theme: "system",
  language: "system",
  sort: { by: "name", dir: "asc" },
  output: { compression: "none", gzipLevel: 6, pathMode: "foldersOnly" },
};

const NEXT_THEME: Record<ThemePreference, ThemePreference> = {
  system: "light",
  light: "dark",
  dark: "system",
};

/**
 * How many log lines the window keeps. Every file added produces one, so a
 * large archive would otherwise put hundreds of thousands of objects on the
 * JS heap for no benefit — the complete log stays in Rust and is what
 * "Save log" writes.
 */
const LOG_WINDOW = 5000;

/**
 * `settings.json` is the source of truth, but it arrives a tick after mount.
 * The cached copies only avoid a flash of the wrong theme or language.
 */
function cached<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    if (v && (allowed as readonly string[]).includes(v)) return v as T;
  } catch {
    // A locked-down profile can refuse storage; the default still applies.
  }
  return fallback;
}

export default function App() {
  const store = useTree();
  const [stage, setStage] = useState<Stage>({ at: "design" });
  const [dragging, setDragging] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const [progress, setProgress] = useState<ArchiveProgress | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [logTotal, setLogTotal] = useState(0);
  // Counted across every batch, not just the tail on screen, so a run whose
  // failures scrolled out of the window still reports them honestly.
  const [logErrors, setLogErrors] = useState(0);
  const [summary, setSummary] = useState<ArchiveSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [unresolved, setUnresolved] = useState<UnresolvedRule[] | null>(null);
  const [showIssues, setShowIssues] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [settings, setSettings] = useState<Settings>(() => ({
    ...FALLBACK,
    theme: cached<ThemePreference>("theme", ["system", "light", "dark"], "system"),
    language: cached<LanguagePreference>("language", ["system", ...LANGS], "system"),
  }));
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  );

  // --- preferences ------------------------------------------------------

  // Held in a ref as well so a patch always merges onto the newest copy,
  // however many preferences change in one interaction.
  const latest = useRef(settings);

  const patchSettings = useCallback((patch: Partial<Settings>) => {
    const next = { ...latest.current, ...patch };
    latest.current = next;
    setSettings(next);
    // Also applies to the running session on the Rust side.
    api.saveSettings(next).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        latest.current = s;
        setSettings(s);
        // The backend already started in this sort order; the toolbar just
        // has not been told yet.
        useTree.getState().adoptSort(s.sort);
      })
      .catch(() => {
        // Unreadable preferences must never stop the app from opening.
      });
  }, []);

  const theme = settings.theme === "system" ? (systemDark ? "dark" : "light") : settings.theme;
  const lang: Lang = useMemo(() => resolveLang(settings.language), [settings.language]);
  const t = useMemo(() => translator(lang), [lang]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    document.documentElement.lang = lang;
    fmt.setLocale(lang);
  }, [lang]);

  useEffect(() => {
    try {
      localStorage.setItem("theme", settings.theme);
      localStorage.setItem("language", settings.language);
    } catch {
      // See `cached`.
    }
  }, [settings.theme, settings.language]);

  // Follow the OS live while the preference is System.
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

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
        await store.applyTreeUpdate(update);
      });
    },
    [ingest, store],
  );

  // --- events from Rust -----------------------------------------------

  useEffect(() => {
    const un = [
      api.onScanProgress(setScanProgress),
      api.onArchiveProgress(setProgress),
      // Lines arrive in batches, and only the tail is kept — see LOG_WINDOW.
      api.onArchiveLog((batch) => {
        setLogTotal((n) => n + batch.length);
        setLogErrors((n) => n + batch.filter((e) => e.level === "error").length);
        setLog((l) => {
          const next = [...l, ...batch];
          return next.length > LOG_WINDOW ? next.slice(-LOG_WINDOW) : next;
        });
      }),
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

  // Paths handed over by the Explorer menu, which the window did not ask for.
  useEffect(() => {
    const un = [
      api.onTreeScanning(setScanning),
      api.onTreeError(setError),
      api.onTreeUpdated((update) => {
        void useTree.getState().applyTreeUpdate(update);
      }),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, []);

  // --- toolbar actions -------------------------------------------------

  const pickFolders = async () => {
    const picked = await open({ directory: true, multiple: true, title: t("app.addFoldersTitle") });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const pickFiles = async () => {
    const picked = await open({ multiple: true, title: t("app.addFilesTitle") });
    if (picked) await addPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const removeSelected = async () => {
    if (store.selected === null) return;
    try {
      await store.applyTreeUpdate(await api.removeNode(store.selected));
    } catch (e) {
      setError(String(e));
    }
  };

  const clearAll = async () => {
    await store.applyTreeUpdate(await api.clearAll());
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
    patchSettings({ sort: { by, dir } });
  };

  const changeOutput = (output: OutputOptions) => patchSettings({ output });

  const cycleLanguage = () => {
    const order: LanguagePreference[] = ["system", ...LANGS];
    const i = order.indexOf(settings.language);
    patchSettings({ language: order[(i + 1) % order.length] });
  };

  const savePlan = async () => {
    const path = await save({
      title: t("app.planSaveTitle"),
      defaultPath: "archive-plan.json",
      filters: [{ name: t("app.planFilter"), extensions: ["json"] }],
    });
    if (!path) return;
    try {
      await api.savePlan(path);
      setNotice(t("app.planSaved", { path: fmt.shortenPath(path, 60) }));
    } catch (e) {
      setError(String(e));
    }
  };

  const loadPlan = async () => {
    const path = await open({
      title: t("app.planOpenTitle"),
      multiple: false,
      filters: [{ name: t("app.planFilter"), extensions: ["json"] }],
    });
    if (!path || Array.isArray(path)) return;
    await ingest(async () => {
      const result = await api.loadPlan(path);
      await store.applyTreeUpdate(result.tree);
      // The plan carries its own output options; adopt them as the current
      // preferences so the dialog opens on what the plan asked for.
      patchSettings({ output: result.output });
      if (result.unresolved.length > 0) setUnresolved(result.unresolved);
    });
  };

  const beginArchive = () => {
    setLog([]);
    setLogTotal(0);
    setLogErrors(0);
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
    <LangContext.Provider value={lang}>
      <div className={`app ${dragging ? "app--dropping" : ""}`}>
        <Toolbar
          sort={store.sort}
          hasTree={!!store.root}
          canRemove={store.selected !== null}
          scanning={scanning}
          scanProgress={scanProgress}
          theme={settings.theme}
          language={settings.language}
          resolvedLanguage={lang}
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
          onCycleTheme={() => patchSettings({ theme: NEXT_THEME[settings.theme] })}
          onCycleLanguage={cycleLanguage}
          onOpenSettings={() => setShowSettings(true)}
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
              <p>{t("tree.dropHere")}</p>
            </div>
          </div>
        )}

        {showSettings && (
          <SettingsDialog
            theme={settings.theme}
            language={settings.language}
            onThemeChange={(theme) => patchSettings({ theme })}
            onLanguageChange={(language) => patchSettings({ language })}
            onClose={() => setShowSettings(false)}
          />
        )}

        {stage.at === "configure" && (
          <ArchiveDialog
            options={settings.output}
            onOptionsChange={changeOutput}
            onClose={() => setStage({ at: "design" })}
            onStarted={(outPath) => setStage({ at: "running", outPath })}
          />
        )}

        {stage.at === "running" && (
          <ProgressView
            outPath={stage.outPath}
            progress={progress}
            log={log}
            logTotal={logTotal}
            logErrors={logErrors}
            summary={summary}
            onClose={closeRun}
          />
        )}

        {showIssues && (
          <Modal title={t("app.issuesTitle")} onClose={() => setShowIssues(false)} wide>
            <p className="modal__lede">{t("app.issuesLede")}</p>
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
          <Modal title={t("app.unresolvedTitle")} onClose={() => setUnresolved(null)}>
            <p className="modal__lede">{t("app.unresolvedLede")}</p>
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
              aria-label={t("app.dismiss")}
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
    </LangContext.Provider>
  );
}
