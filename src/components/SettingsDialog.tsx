import { useState } from "react";
import { Modal } from "./Modal";
import { api, toTuxError } from "../lib/api";
import { forgetBanners } from "../hooks/useBanner";
import type { AppPaths, Backend, Dependencies, Settings } from "../lib/types";
import { LANGUAGES, useI18n, type TranslationKey } from "../i18n";

/** Les libellés viennent du dictionnaire ; protontricks garde son nom propre. */
const BACKENDS: { value: Backend; label?: TranslationKey; hint: TranslationKey }[] = [
  { value: "auto", label: "settings.backendAuto", hint: "settings.backendAutoHint" },
  { value: "protontricks", hint: "settings.backendProtontricksHint" },
  { value: "proton", label: "settings.backendProton", hint: "settings.backendProtonHint" },
  { value: "wine", label: "settings.backendWine", hint: "settings.backendWineHint" },
];

interface SettingsDialogProps {
  settings: Settings;
  dependencies: Dependencies | null;
  paths: AppPaths | null;
  onClose: () => void;
  onSaved: (settings: Settings) => void;
  onNotice: (message: string) => void;
}

export function SettingsDialog({
  settings,
  dependencies,
  paths,
  onClose,
  onSaved,
  onNotice,
}: SettingsDialogProps) {
  const { t, language, setLanguage } = useI18n();
  const [draft, setDraft] = useState<Settings>(settings);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    try {
      const saved = await api.updateSettings(draft);
      onSaved(saved);
      onClose();
    } catch (error) {
      onNotice(toTuxError(error).message);
    } finally {
      setSaving(false);
    }
  };

  const clearCache = async () => {
    try {
      const removed = await api.clearBannerCache();
      forgetBanners();
      onNotice(t("settings.cacheCleared", { count: removed }));
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  return (
    <Modal
      title={t("settings.title")}
      onClose={onClose}
      footer={
        <>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-ink-600 px-4 py-2 text-sm text-mist-300 transition-colors hover:bg-white/5"
          >
            {t("dialog.cancel")}
          </button>
          <button
            type="button"
            onClick={save}
            disabled={saving}
            className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-60"
          >
            {saving ? t("settings.saving") : t("settings.save")}
          </button>
        </>
      }
    >
      <div className="space-y-5 text-sm">
        <label className="block">
          <span className="mb-1.5 block font-medium text-mist-100">
            {t("settings.language")}
          </span>
          <select
            value={language}
            onChange={(event) => setLanguage(event.target.value)}
            className="w-full rounded-lg border border-ink-600 bg-ink-800 px-3 py-2 text-sm text-mist-100 outline-none focus:border-brand-500/70"
          >
            {LANGUAGES.map((entry) => (
              <option key={entry.code} value={entry.code}>
                {entry.label}
              </option>
            ))}
          </select>
          <span className="mt-1 block text-xs text-mist-500">
            {t("settings.languageHint")}
          </span>
        </label>

        <fieldset>
          <legend className="mb-2 font-medium text-mist-100">
            {t("settings.backend")}
          </legend>
          <div className="space-y-2">
            {BACKENDS.map((backend) => (
              <label
                key={backend.value}
                className={[
                  "flex cursor-pointer gap-3 rounded-lg border p-3 transition-colors",
                  draft.backend === backend.value
                    ? "border-brand-500/60 bg-brand-500/10"
                    : "border-ink-600 hover:bg-white/5",
                ].join(" ")}
              >
                <input
                  type="radio"
                  name="backend"
                  className="mt-1 accent-[var(--color-brand-500)]"
                  checked={draft.backend === backend.value}
                  onChange={() => setDraft({ ...draft, backend: backend.value })}
                />
                <span>
                  <span className="block font-medium text-mist-100">
                    {backend.label ? t(backend.label) : "protontricks"}
                  </span>
                  <span className="block font-mono text-xs text-mist-500">
                    {t(backend.hint)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        <Toggle
          checked={draft.warnIfGameNotRunning}
          onChange={(value) => setDraft({ ...draft, warnIfGameNotRunning: value })}
          label={t("settings.warnLabel")}
          description={t("settings.warnHint")}
        />
        <Toggle
          checked={draft.allowNetworkArtwork}
          onChange={(value) => setDraft({ ...draft, allowNetworkArtwork: value })}
          label={t("settings.artworkLabel")}
          description={t("settings.artworkHint")}
        />

        <div className="space-y-2 border-t border-ink-700 pt-4 text-xs text-mist-500">
          <p>
            <span className="text-mist-400">protontricks :</span>{" "}
            {dependencies?.protontricks ??
              (dependencies?.protontricksFlatpak ? "Flatpak" : t("settings.notInstalled"))}
          </p>
          <p>
            <span className="text-mist-400">wine :</span>{" "}
            {dependencies?.wine ?? t("settings.notInstalled")}
          </p>
          {paths && (
            <>
              <p className="break-all">
                <span className="text-mist-400">{t("settings.configPath")}</span>{" "}
                {paths.config}
              </p>
              <p className="break-all">
                <span className="text-mist-400">{t("settings.cachePath")}</span>{" "}
                {paths.bannerCache}
              </p>
            </>
          )}
          <button
            type="button"
            onClick={clearCache}
            className="mt-1 rounded-md border border-ink-600 px-3 py-1.5 text-mist-300 transition-colors hover:bg-white/5"
          >
            {t("settings.clearCache")}
          </button>
        </div>
      </div>
    </Modal>
  );
}

interface ToggleProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  description: string;
}

function Toggle({ checked, onChange, label, description }: ToggleProps) {
  return (
    <label className="flex cursor-pointer items-start gap-3">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={[
          "mt-0.5 h-5 w-9 shrink-0 rounded-full p-0.5 transition-colors",
          checked ? "bg-brand-600" : "bg-ink-600",
        ].join(" ")}
      >
        <span
          className={[
            "block h-4 w-4 rounded-full bg-white transition-transform",
            checked ? "translate-x-4" : "translate-x-0",
          ].join(" ")}
        />
      </button>
      <span>
        <span className="block font-medium text-mist-100">{label}</span>
        <span className="block text-xs text-mist-500">{description}</span>
      </span>
    </label>
  );
}
