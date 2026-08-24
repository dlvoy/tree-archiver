import { useEffect, useState } from "react";
import * as api from "../api/commands";
import type {
  FileOrder,
  InterfaceMode,
  LanguagePreference,
  ThemePreference,
} from "../api/commands";
import { useT } from "../i18n/context";
import { LANGS, LANG_NAMES, type Lang } from "../i18n";
import { Flag } from "./Flag";
import { Modal } from "./ArchiveDialog";

/**
 * Preferences that are not worth a toolbar button, plus the one setting that
 * reaches outside the app: the Explorer context-menu entry.
 */
export function SettingsDialog({
  theme,
  language,
  interfaceMode,
  fileOrder,
  onThemeChange,
  onLanguageChange,
  onInterfaceModeChange,
  onFileOrderChange,
  onClose,
}: {
  theme: ThemePreference;
  language: LanguagePreference;
  interfaceMode: InterfaceMode;
  fileOrder: FileOrder;
  onThemeChange: (t: ThemePreference) => void;
  onLanguageChange: (l: LanguagePreference) => void;
  onInterfaceModeChange: (m: InterfaceMode) => void;
  onFileOrderChange: (o: FileOrder) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [confirming, setConfirming] = useState<null | "install" | "remove">(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const verb = t("settings.explorerVerb");

  useEffect(() => {
    api.explorerStatus().then(setInstalled).catch(() => setInstalled(false));
  }, []);

  const apply = async (install: boolean) => {
    setWorking(true);
    setError(null);
    try {
      // The registry is the source of truth, so the checkbox reflects what
      // the call actually left behind rather than what was asked for.
      const now = install ? await api.explorerInstall(verb) : await api.explorerUninstall();
      setInstalled(now);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
      setConfirming(null);
    }
  };

  return (
    <Modal title={t("settings.title")} onClose={onClose} escapes={confirming === null}>
      <div className="field">
        <span className="field__label">{t("settings.appearance")}</span>

        <label className="setting" htmlFor="set-theme">
          <span className="setting__name">{t("settings.theme")}</span>
          <select
            id="set-theme"
            className="select"
            value={theme}
            onChange={(e) => onThemeChange(e.target.value as ThemePreference)}
          >
            <option value="system">{t("theme.systemLong")}</option>
            <option value="light">{t("theme.light")}</option>
            <option value="dark">{t("theme.dark")}</option>
          </select>
        </label>

        <label className="setting" htmlFor="set-lang">
          <span className="setting__name">{t("settings.language")}</span>
          <span className="setting__control">
            <span className="setting__icon" aria-hidden="true">
              {language === "system" ? <GlobeMark /> : <Flag lang={language as Lang} />}
            </span>
            <select
              id="set-lang"
              className="select"
              value={language}
              onChange={(e) => onLanguageChange(e.target.value as LanguagePreference)}
            >
              <option value="system">{t("lang.systemLong")}</option>
              {LANGS.map((l) => (
                <option key={l} value={l}>
                  {LANG_NAMES[l]}
                </option>
              ))}
            </select>
          </span>
        </label>

        <label className="setting" htmlFor="set-interface">
          <span className="setting__name">{t("settings.interface")}</span>
          <select
            id="set-interface"
            className="select"
            value={interfaceMode}
            onChange={(e) => onInterfaceModeChange(e.target.value as InterfaceMode)}
          >
            <option value="icons">{t("settings.interfaceIcons")}</option>
            <option value="labels">{t("settings.interfaceLabels")}</option>
            <option value="iconsAndLabels">{t("settings.interfaceIconsAndLabels")}</option>
          </select>
        </label>
      </div>

      <div className="field">
        <span className="field__label">{t("settings.archiving")}</span>

        <label className="setting" htmlFor="set-file-order">
          <span className="setting__name">{t("settings.archiveOrder")}</span>
          <select
            id="set-file-order"
            className="select"
            value={fileOrder}
            onChange={(e) => onFileOrderChange(e.target.value as FileOrder)}
          >
            <option value="optimal">{t("settings.archiveOptimal")}</option>
            <option value="asInPlan">{t("settings.archiveAsInPlan")}</option>
            <option value="alphabetical">{t("settings.archiveAlphabetical")}</option>
          </select>
        </label>
        <p className="field__hint">{t("settings.archiveOrderHint")}</p>
      </div>

      <div className="field">
        <span className="field__label">{t("settings.integration")}</span>

        <label className="setting setting--check" htmlFor="set-explorer">
          <input
            id="set-explorer"
            type="checkbox"
            className="check"
            checked={installed === true}
            disabled={installed === null || working}
            onChange={(e) => setConfirming(e.target.checked ? "install" : "remove")}
          />
          <span className="setting__body">
            <span className="setting__name">{t("settings.explorer")}</span>
            <span className="setting__help">{t("settings.explorerBody", { verb })}</span>
            <span className="setting__help setting__help--quiet">
              {t("settings.explorerNote")}
            </span>
          </span>
          <span className={`pill ${installed ? "pill--on" : ""}`}>
            {installed ? t("settings.explorerOn") : t("settings.explorerOff")}
          </span>
        </label>
      </div>

      {error && <p className="alert alert--error">{error}</p>}

      <div className="modal__actions">
        <div className="bar__spacer" />
        <button type="button" className="btn btn--go" onClick={onClose}>
          {t("settings.close")}
        </button>
      </div>

      {confirming && (
        <Modal
          title={
            confirming === "install"
              ? t("settings.confirmInstallTitle")
              : t("settings.confirmRemoveTitle")
          }
          onClose={() => setConfirming(null)}
        >
          <p className="modal__lede">
            {confirming === "install"
              ? t("settings.confirmInstallBody", { verb })
              : t("settings.confirmRemoveBody", { verb })}
          </p>
          <div className="modal__actions">
            <button type="button" className="btn" onClick={() => setConfirming(null)}>
              {t("settings.confirmCancel")}
            </button>
            <div className="bar__spacer" />
            <button
              type="button"
              className={`btn ${confirming === "install" ? "btn--go" : "btn--danger"}`}
              disabled={working}
              onClick={() => void apply(confirming === "install")}
            >
              {confirming === "install"
                ? t("settings.confirmInstallGo")
                : t("settings.confirmRemoveGo")}
            </button>
          </div>
        </Modal>
      )}
    </Modal>
  );
}

function GlobeMark() {
  return (
    <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.2">
      <circle cx="8" cy="8" r="6.2" />
      <path d="M1.8 8h12.4M8 1.8c1.9 2 2.9 4 2.9 6.2S9.9 12.2 8 14.2C6.1 12.2 5.1 10.2 5.1 8S6.1 3.8 8 1.8z" />
    </svg>
  );
}
