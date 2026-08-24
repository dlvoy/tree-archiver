import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "../api/commands";
import type { AppInfo } from "../api/commands";
import { useT } from "../i18n/context";
import { Modal } from "./ArchiveDialog";

/**
 * Who wrote it, which build this is, and the licence it ships under.
 *
 * The version, date and commit are stamped into the executable at compile time
 * (`build.rs`), so a bug report can name the exact build rather than "the
 * latest one".
 */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [showLicense, setShowLicense] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.appInfo().then(setInfo).catch((e) => setError(String(e)));
  }, []);

  // The webview has no navigation of its own under the app's CSP, so the link
  // hands the URL to the system browser instead of following it.
  const openRelease = async () => {
    if (!info) return;
    try {
      await openUrl(info.releaseUrl);
    } catch (e) {
      setError(String(e));
    }
  };

  const dash = "—";

  return (
    <Modal title={t("about.title")} onClose={onClose} escapes={!showLicense}>
      <div className="about__head">
        <svg viewBox="0 0 20 20" width="26" height="26" aria-hidden="true" className="about__mark">
          <path
            d="M10 2v16M10 6h6M10 11h6M10 16h6M10 6H4M10 11H4"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
        </svg>
        <div className="about__names">
          <span className="about__name">Tree Archiver</span>
          <span className="about__tagline">{t("about.tagline")}</span>
        </div>
      </div>

      <div className="about__foot">
        <p className="about__copy">
          {"© Dominik Dzienia — "}
          <button type="button" className="linkbtn" onClick={() => setShowLicense(true)}>
            {t("about.license")}
          </button>
        </p>

        <p className="about__meta">
          <button
            type="button"
            className="linkbtn"
            onClick={() => void openRelease()}
            disabled={!info}
            title={info ? t("about.releaseTitle", { version: info.version }) : ""}
          >
            {info ? info.version : dash}
          </button>
          <span className="about__sep" aria-hidden="true">
            |
          </span>
          <span title={t("about.builtLabel")}>{info ? info.buildDate : dash}</span>
          <span className="about__sep" aria-hidden="true">
            |
          </span>
          <span title={t("about.commitLabel")}>{info ? info.gitHash : dash}</span>
        </p>
      </div>

      {error && <p className="alert alert--error">{error}</p>}

      <div className="modal__actions">
        <div className="bar__spacer" />
        <button type="button" className="btn btn--go" onClick={onClose}>
          {t("app.close")}
        </button>
      </div>

      {showLicense && (
        <Modal title={t("about.licenseTitle")} onClose={() => setShowLicense(false)} wider>
          {/* Verbatim and untranslated, which is the only way to show a licence. */}
          <pre className="license">{info?.license ?? ""}</pre>
          <div className="modal__actions">
            <div className="bar__spacer" />
            <button type="button" className="btn btn--go" onClick={() => setShowLicense(false)}>
              {t("app.close")}
            </button>
          </div>
        </Modal>
      )}
    </Modal>
  );
}
