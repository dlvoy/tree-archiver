import type { ReactNode } from "react";
import type { ScanProgress, SortBy, SortDir, SortKey } from "../api/commands";
import type { InterfaceMode, LanguagePreference, ThemePreference } from "../api/commands";
import { useT } from "../i18n/context";
import { LANG_NAMES, type Lang } from "../i18n";
import type { Key } from "../i18n";
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
  interfaceMode,
  onAddFolders,
  onAddFiles,
  onRemove,
  onClear,
  onCollapseAll,
  onCheckAll,
  onOpenAutoIgnore,
  onSort,
  onSavePlan,
  onLoadPlan,
  onCancelScan,
  onCycleTheme,
  onCycleLanguage,
  onOpenSettings,
  onOpenAbout,
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
  /** How the Sources/Plan/Sort/Selection buttons are drawn. The four icon
   * buttons on the right (language, theme, settings, about) never change. */
  interfaceMode: InterfaceMode;
  onAddFolders: () => void;
  onAddFiles: () => void;
  onRemove: () => void;
  onClear: () => void;
  onCollapseAll: () => void;
  onCheckAll: (checked: boolean) => void;
  onOpenAutoIgnore: () => void;
  onSort: (by: SortBy, dir: SortDir) => void;
  onSavePlan: () => void;
  onLoadPlan: () => void;
  onCancelScan: () => void;
  onCycleTheme: () => void;
  onCycleLanguage: () => void;
  onOpenSettings: () => void;
  onOpenAbout: () => void;
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

  // Only the four buttons on the right (language/theme/settings/about) are
  // exempt — they are icon-only regardless of this setting.
  const showIcons = interfaceMode !== "labels";
  const showLabels = interfaceMode !== "icons";

  /** A group button caption, shown only when labels are on; it also serves as
   * the accessible tooltip regardless of mode via the button's own title. */
  const label = (k: Key) => (showLabels ? t(k) : null);

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
          <button
            type="button"
            className="btn btn--primary"
            onClick={onAddFolders}
            title={t("toolbar.addFolders")}
            aria-label={t("toolbar.addFolders")}
          >
            {showIcons && <AddFoldersMark />}
            {label("toolbar.addFolders")}
          </button>
          <button
            type="button"
            className="btn"
            onClick={onAddFiles}
            title={t("toolbar.addFiles")}
            aria-label={t("toolbar.addFiles")}
          >
            {showIcons && <AddFilesMark />}
            {label("toolbar.addFiles")}
          </button>
          <button
            type="button"
            className="btn"
            onClick={onRemove}
            disabled={!canRemove}
            title={t("toolbar.remove")}
            aria-label={t("toolbar.remove")}
          >
            {showIcons && <RemoveMark />}
            {label("toolbar.remove")}
          </button>
          <button
            type="button"
            className="btn"
            onClick={onClear}
            disabled={!hasTree}
            title={t("toolbar.clear")}
            aria-label={t("toolbar.clear")}
          >
            {showIcons && <ClearMark />}
            {label("toolbar.clear")}
          </button>
        </div>

        <div className="group">
          <span className="group__label">{t("toolbar.plan")}</span>
          <button
            type="button"
            className="btn"
            onClick={onSavePlan}
            disabled={!hasTree}
            title={t("toolbar.planSaveTip")}
            aria-label={t("toolbar.planSaveTip")}
          >
            {showIcons && <SaveMark />}
            {label("toolbar.planSave")}
          </button>
          <button
            type="button"
            className="btn"
            onClick={onLoadPlan}
            title={t("toolbar.planOpenTip")}
            aria-label={t("toolbar.planOpenTip")}
          >
            {showIcons && <OpenMark />}
            {label("toolbar.planOpen")}
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

        <button
          type="button"
          className="btn btn--icon"
          onClick={onOpenAbout}
          aria-label={t("toolbar.about")}
          title={t("toolbar.about")}
        >
          <InfoMark />
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
              title={t("toolbar.sortName")}
              aria-label={t("toolbar.sortName")}
            >
              {showIcons && <SortNameMark />}
              {label("toolbar.sortName")}
              {sort.by === "name" && <Caret dir={sort.dir} />}
            </button>
            <button
              type="button"
              className={`seg__btn ${sort.by === "size" ? "seg__btn--on" : ""}`}
              onClick={() => flip("size")}
              title={t("toolbar.sortSize")}
              aria-label={t("toolbar.sortSize")}
            >
              {showIcons && <SortSizeMark />}
              {label("toolbar.sortSize")}
              {sort.by === "size" && <Caret dir={sort.dir} />}
            </button>
            <button
              type="button"
              className={`seg__btn ${sort.by === "count" ? "seg__btn--on" : ""}`}
              onClick={() => flip("count")}
              title={t("toolbar.sortCount")}
              aria-label={t("toolbar.sortCount")}
            >
              {showIcons && <SortCountMark />}
              {label("toolbar.sortCount")}
              {sort.by === "count" && <Caret dir={sort.dir} />}
            </button>
          </div>
        </div>

        <div className="group">
          <span className="group__label">{t("toolbar.selection")}</span>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={() => onCheckAll(true)}
            disabled={!hasTree}
            title={t("toolbar.checkAll")}
            aria-label={t("toolbar.checkAll")}
          >
            {showIcons && <CheckAllMark />}
            {label("toolbar.checkAll")}
          </button>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={() => onCheckAll(false)}
            disabled={!hasTree}
            title={t("toolbar.uncheckAll")}
            aria-label={t("toolbar.uncheckAll")}
          >
            {showIcons && <UncheckAllMark />}
            {label("toolbar.uncheckAll")}
          </button>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={onCollapseAll}
            disabled={!hasTree}
            title={t("toolbar.collapseAll")}
            aria-label={t("toolbar.collapseAll")}
          >
            {showIcons && <CollapseAllMark />}
            {label("toolbar.collapseAll")}
          </button>
          <button
            type="button"
            className="btn btn--quiet"
            onClick={onOpenAutoIgnore}
            disabled={!hasTree}
            title={t("toolbar.autoIgnore")}
            aria-label={t("toolbar.autoIgnore")}
          >
            {showIcons && <AutoIgnoreMark />}
            {label("toolbar.autoIgnore")}
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

function InfoMark() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round">
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 7.1v4.2" />
      <path d="M8 4.7v.1" strokeWidth="1.7" />
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

/**
 * Icons for the twelve buttons the "App interface" setting affects. Same
 * house style as the marks above: a 16px grid, a single stroke weight, round
 * caps and joins, colour inherited from the button via `currentColor`.
 */
function ToolbarIcon({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width="15"
      height="15"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

function AddFoldersMark() {
  return (
    <ToolbarIcon>
      <path d="M1.5 4.5h4.5l1.5 2h7v7h-13z" />
      <path d="M11 8.3v4M9 10.3h4" />
    </ToolbarIcon>
  );
}

function AddFilesMark() {
  return (
    <ToolbarIcon>
      <path d="M4 1.5h5l3 3v10h-8z" />
      <path d="M8 8v4M6 10h4" />
    </ToolbarIcon>
  );
}

function RemoveMark() {
  return (
    <ToolbarIcon>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M5.2 8h5.6" />
    </ToolbarIcon>
  );
}

function ClearMark() {
  return (
    <ToolbarIcon>
      <path d="M3.5 5h9" />
      <path d="M6.3 5V3.3h3.4V5" />
      <path d="M4.8 5l.6 8h5.2l.6-8" />
      <path d="M6.8 7v4M9.2 7v4" />
    </ToolbarIcon>
  );
}

function SaveMark() {
  return (
    <ToolbarIcon>
      <path d="M2.5 2.5h8.3l2.2 2.2v8.8h-10.5z" />
      <path d="M5 2.5v3.5h5.5v-3.5" />
      <rect x="5.2" y="9.2" width="5" height="4" />
    </ToolbarIcon>
  );
}

function OpenMark() {
  return (
    <ToolbarIcon>
      <path d="M1.5 4.5h4.5l1.5 2h7v2" />
      <path d="M1.5 13.5l1.8-6h11.2l-1.8 6z" />
    </ToolbarIcon>
  );
}

function SortNameMark() {
  return (
    <ToolbarIcon>
      <path d="M2.5 4.5h9M2.5 8h6.5M2.5 11.5h4" />
    </ToolbarIcon>
  );
}

function SortSizeMark() {
  return (
    <ToolbarIcon>
      <path d="M3 13V9M7.5 13V6M12 13V3" />
    </ToolbarIcon>
  );
}

function SortCountMark() {
  return (
    <ToolbarIcon>
      {/* A tally of five: four uprights and a crossing stroke, the plainest
          possible glyph for "count". */}
      <path d="M3.5 3v10M6 3v10M8.5 3v10M11 3v10" />
      <path d="M2.5 4.5l9.5 6" />
    </ToolbarIcon>
  );
}

function CheckAllMark() {
  return (
    <ToolbarIcon>
      <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
      <path d="M5 8.2l2.1 2.1L11.5 6" />
    </ToolbarIcon>
  );
}

function UncheckAllMark() {
  return (
    <ToolbarIcon>
      <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
    </ToolbarIcon>
  );
}

function CollapseAllMark() {
  return (
    <ToolbarIcon>
      <path d="M3.5 10.5l4.5-4.5 4.5 4.5" />
    </ToolbarIcon>
  );
}

/** A filter funnel — presets sieve the tree down to what's worth keeping. */
function AutoIgnoreMark() {
  return (
    <ToolbarIcon>
      <path d="M2.5 3h11l-4.25 5.5v4.5l-2.5 1.25v-5.75z" />
    </ToolbarIcon>
  );
}
