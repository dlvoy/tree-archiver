import type { ScanProgress, SortBy, SortDir, SortKey } from "../api/commands";
import type { LanguagePreference, ThemePreference } from "../api/commands";
import { useT } from "../i18n/context";
import { LANG_NAMES, type Lang } from "../i18n";
import { Flag } from "./Flag";
import * as fmt from "../lib/format";

export function Toolbar({
  sort,
  hasTree,
  canRemove,
  scanning,
  scanProgress,
  theme,
  language,
  resolvedLanguage,
  onAddFolders,
  onAddFiles,
  onRemove,
  onClear,
  onCollapseAll,
  onCheckAll,
  onSort,
  onSavePlan,
  onLoadPlan,
  onCancelScan,
  onCycleTheme,
  onCycleLanguage,
  onOpenSettings,
}: {
  sort: SortKey;
  hasTree: boolean;
  canRemove: boolean;
  scanning: boolean;
  scanProgress: ScanProgress | null;
  theme: ThemePreference;
  language: LanguagePreference;
  /** What `system` currently resolves to, for the flag on the button. */
  resolvedLanguage: Lang;
  onAddFolders: () => void;
  onAddFiles: () => void;
  onRemove: () => void;
  onClear: () => void;
  onCollapseAll: () => void;
  onCheckAll: (checked: boolean) => void;
  onSort: (by: SortBy, dir: SortDir) => void;
  onSavePlan: () => void;
  onLoadPlan: () => void;
  onCancelScan: () => void;
  onCycleTheme: () => void;
  onCycleLanguage: () => void;
  onOpenSettings: () => void;
}) {
  const t = useT();

  const flip = (by: SortBy) => {
    if (sort.by === by) onSort(by, sort.dir === "asc" ? "desc" : "asc");
    else onSort(by, "asc");
  };

  const themeName =
    theme === "system" ? t("theme.systemLong") : theme === "light" ? t("theme.light") : t("theme.dark");
  const langName =
    language === "system" ? t("lang.systemLong") : LANG_NAMES[language];

  return (
    <header className="bar bar--top">
      <div className="bar__row">
        <div className="brand">
          <svg viewBox="0 0 20 20" width="17" height="17" aria-hidden="true" className="brand__mark">
            <path
              d="M10 2v16M10 6h6M10 11h6M10 16h6M10 6H4M10 11H4"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
          <span className="brand__name">Tree Archiver</span>
        </div>

        <div className="group">
          <span className="group__label">{t("toolbar.sources")}</span>
          <button type="button" className="btn btn--primary" onClick={onAddFolders}>
            {t("toolbar.addFolders")}
          </button>
          <button type="button" className="btn" onClick={onAddFiles}>
            {t("toolbar.addFiles")}
          </button>
          <button type="button" className="btn" onClick={onRemove} disabled={!canRemove}>
            {t("toolbar.remove")}
          </button>
          <button type="button" className="btn" onClick={onClear} disabled={!hasTree}>
            {t("toolbar.clear")}
          </button>
        </div>

        <div className="group">
          <span className="group__label">{t("toolbar.plan")}</span>
          <button type="button" className="btn" onClick={onSavePlan} disabled={!hasTree}>
            {t("toolbar.planSave")}
          </button>
          <button type="button" className="btn" onClick={onLoadPlan}>
            {t("toolbar.planOpen")}
          </button>
        </div>

        <div className="bar__spacer" />

        <button
          type="button"
          className="btn btn--icon"
          onClick={onCycleLanguage}
          aria-label={t("lang.label", { name: langName })}
          title={t("lang.label", { name: langName })}
        >
          <Flag lang={language === "system" ? resolvedLanguage : language} muted={language === "system"} />
        </button>

        <button
          type="button"
          className="btn btn--icon"
          onClick={onCycleTheme}
          aria-label={t("theme.label", { name: themeName })}
          title={t("theme.label", { name: themeName })}
        >
          <ThemeMark theme={theme} />
        </button>

        <button
          type="button"
          className="btn btn--icon"
          onClick={onOpenSettings}
          aria-label={t("toolbar.settings")}
          title={t("toolbar.settings")}
        >
          <CogMark />
        </button>
      </div>

      <div className="bar__row bar__row--sub">
        <div className="group">
          <span className="group__label">{t("toolbar.sort")}</span>
          <div className="seg">
            <button
              type="button"
              className={`seg__btn ${sort.by === "name" ? "seg__btn--on" : ""}`}
              onClick={() => flip("name")}
            >
              {t("toolbar.sortName")}
              {sort.by === "name" && <Caret dir={sort.dir} />}
            </button>
            <button
              type="button"
              className={`seg__btn ${sort.by === "size" ? "seg__btn--on" : ""}`}
              onClick={() => flip("size")}
            >
              {t("toolbar.sortSize")}
              {sort.by === "size" && <Caret dir={sort.dir} />}
            </button>
          </div>
        </div>

        <div className="group">
          <span className="group__label">{t("toolbar.selection")}</span>
          <button type="button" className="btn btn--quiet" onClick={() => onCheckAll(true)} disabled={!hasTree}>
            {t("toolbar.checkAll")}
          </button>
          <button type="button" className="btn btn--quiet" onClick={() => onCheckAll(false)} disabled={!hasTree}>
            {t("toolbar.uncheckAll")}
          </button>
          <button type="button" className="btn btn--quiet" onClick={onCollapseAll} disabled={!hasTree}>
            {t("toolbar.collapseAll")}
          </button>
        </div>

        <div className="bar__spacer" />

        {scanning && (
          <div className="scanning">
            <span className="scanning__pulse" aria-hidden="true" />
            <span className="scanning__text">
              {scanProgress
                ? t("toolbar.scanned", {
                    files: fmt.count(scanProgress.files),
                    bytes: fmt.bytes(scanProgress.bytes),
                  })
                : t("toolbar.reading")}
            </span>
            <span className="scanning__path" title={scanProgress?.current ?? ""}>
              {scanProgress ? fmt.shortenPath(scanProgress.current, 48) : ""}
            </span>
            <button type="button" className="btn btn--quiet" onClick={onCancelScan}>
              {t("toolbar.stop")}
            </button>
          </div>
        )}
      </div>
    </header>
  );
}

/** Monitor, sun, moon — the state the button is in, not the one it moves to. */
function ThemeMark({ theme }: { theme: ThemePreference }) {
  if (theme === "system") {
    return (
      <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <rect x="1.6" y="2.6" width="12.8" height="8.6" rx="1" />
        <path d="M5.6 14h4.8M8 11.2V14" />
      </svg>
    );
  }
  if (theme === "light") {
    return (
      <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round">
        <circle cx="8" cy="8" r="3.1" />
        <path d="M8 1v1.6M8 13.4V15M15 8h-1.6M2.6 8H1M12.9 3.1l-1.1 1.1M4.2 11.8l-1.1 1.1M12.9 12.9l-1.1-1.1M4.2 4.2L3.1 3.1" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
      <path d="M13.5 9.8A6 6 0 0 1 6.2 2.5a6 6 0 1 0 7.3 7.3z" />
    </svg>
  );
}

function CogMark() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinejoin="round">
      <circle cx="8" cy="8" r="2.3" />
      <path d="M8 1.2l1 1.7 1.9-.5.4 2 1.9.7-.8 1.8.8 1.8-1.9.7-.4 2-1.9-.5-1 1.7-1-1.7-1.9.5-.4-2-1.9-.7.8-1.8-.8-1.8 1.9-.7.4-2 1.9.5z" />
    </svg>
  );
}

function Caret({ dir }: { dir: SortDir }) {
  return (
    <svg
      className={`caret ${dir === "desc" ? "caret--down" : ""}`}
      viewBox="0 0 8 8"
      width="8"
      height="8"
      aria-hidden="true"
    >
      <path d="M1.5 5L4 2.5 6.5 5" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
