import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "../api/commands";
import type { IgnoreRuleset } from "../api/commands";
import { useT } from "../i18n/context";
import { useTree } from "../store/tree";
import { Modal } from "./ArchiveDialog";

/**
 * A checkbox list of `.gitignore`-style rulesets — five built-in presets
 * plus whatever the user has imported. Apply matches every checked one
 * against the whole staged tree; whatever it excludes is tagged **auto** on
 * the row, distinct from a plain manual uncheck.
 */
export function AutoIgnoreDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [rulesets, setRulesets] = useState<IgnoreRuleset[] | null>(null);
  const [caseInsensitive, setCaseInsensitive] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importing, setImporting] = useState<
    { path: string; name: string; busy: boolean; error: string | null } | null
  >(null);

  useEffect(() => {
    api
      .listIgnoreRulesets()
      .then((catalog) => {
        setRulesets(catalog.rulesets);
        setCaseInsensitive(catalog.caseInsensitive);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const toggle = (id: string) => {
    setRulesets((prev) =>
      prev ? prev.map((r) => (r.id === id ? { ...r, checked: !r.checked } : r)) : prev,
    );
  };

  const selected = rulesets?.find((r) => r.id === selectedId) ?? null;

  const startImport = async () => {
    setError(null);
    const path = await open({ title: t("autoignore.importPickTitle"), multiple: false });
    if (!path || Array.isArray(path)) return;
    const stem = path.replace(/^.*[\\/]/, "").replace(/\.[^.]*$/, "");
    setImporting({ path, name: stem || "ruleset", busy: false, error: null });
  };

  const confirmImport = async () => {
    if (!importing) return;
    setImporting({ ...importing, busy: true, error: null });
    try {
      const created = await api.importIgnoreRuleset(importing.name, importing.path);
      setRulesets((prev) => (prev ? [...prev, created] : [created]));
      setImporting(null);
    } catch (e) {
      setImporting({ ...importing, busy: false, error: String(e) });
    }
  };

  const deleteSelected = async () => {
    if (!selected) return;
    setError(null);
    try {
      await api.deleteIgnoreRuleset(selected.id);
      setRulesets((prev) => prev?.filter((r) => r.id !== selected.id) ?? null);
      setSelectedId(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const apply = async () => {
    if (!rulesets) return;
    setApplying(true);
    setError(null);
    try {
      const checkedIds = rulesets.filter((r) => r.checked).map((r) => r.id);
      await useTree.getState().applyAutoIgnore(checkedIds, caseInsensitive);
      onClose();
    } catch (e) {
      setError(String(e));
      setApplying(false);
    }
  };

  return (
    <Modal title={t("autoignore.title")} onClose={onClose} wide escapes={!importing}>
      <p className="modal__lede">{t("autoignore.lede")}</p>

      {rulesets && (
        <ul className="issues">
          {rulesets.map((r) => (
            <li
              key={r.id}
              className={`setting setting--check ${r.id === selectedId ? "setting--selected" : ""}`}
              onClick={() => setSelectedId(r.id)}
            >
              <input
                id={`ruleset-${r.id}`}
                type="checkbox"
                className="check"
                checked={r.checked}
                onChange={() => toggle(r.id)}
              />
              <span className="setting__body">
                <span className="setting__name setting__name--strong">{r.name}</span>
                <span className="setting__help">{r.description}</span>
                <RuleSnippet rules={r.rules} />
              </span>
              <span className="pill">
                {r.builtin ? t("autoignore.builtin") : t("autoignore.custom")}
              </span>
            </li>
          ))}
        </ul>
      )}

      {error && <p className="alert alert--error">{error}</p>}

      <div className="modal__actions">
        <button type="button" className="btn" onClick={() => void startImport()}>
          {t("autoignore.add")}
        </button>
        <button
          type="button"
          className="btn btn--danger"
          disabled={!selected || selected.builtin}
          onClick={() => void deleteSelected()}
        >
          {t("autoignore.delete")}
        </button>
        <label className="setting" htmlFor="autoignore-case">
          <input
            id="autoignore-case"
            type="checkbox"
            className="check"
            checked={caseInsensitive}
            onChange={(e) => setCaseInsensitive(e.target.checked)}
          />
          <span className="setting__name">{t("autoignore.caseInsensitive")}</span>
        </label>
        <div className="bar__spacer" />
        <button
          type="button"
          className="btn btn--go"
          disabled={!rulesets || applying}
          onClick={() => void apply()}
        >
          {applying ? t("autoignore.applying") : t("autoignore.apply")}
        </button>
      </div>

      {importing && (
        <Modal title={t("autoignore.importTitle")} onClose={() => setImporting(null)}>
          <div className="field">
            <label className="field__label" htmlFor="ruleset-name">
              {t("autoignore.importName")}
            </label>
            <input
              id="ruleset-name"
              className="input"
              value={importing.name}
              spellCheck={false}
              onChange={(e) => setImporting({ ...importing, name: e.target.value })}
            />
            <p className="field__hint">{importing.path}</p>
          </div>

          {importing.error && <p className="alert alert--error">{importing.error}</p>}

          <div className="modal__actions">
            <button type="button" className="btn" onClick={() => setImporting(null)}>
              {t("autoignore.importCancel")}
            </button>
            <div className="bar__spacer" />
            <button
              type="button"
              className="btn btn--go"
              disabled={importing.busy || !importing.name.trim()}
              onClick={() => void confirmImport()}
            >
              {importing.busy ? t("autoignore.importing") : t("autoignore.importGo")}
            </button>
          </div>
        </Modal>
      )}
    </Modal>
  );
}

/** Up to four rule lines verbatim, with a count of anything past that — the
 * built-in presets are short enough to show in full; an imported file might
 * not be. */
function RuleSnippet({ rules }: { rules: string[] }) {
  const shown = rules.slice(0, 4);
  const more = rules.length - shown.length;
  return (
    <pre className="ruleset__rules">
      {shown.join("\n")}
      {more > 0 && `\n+${more} more`}
    </pre>
  );
}
