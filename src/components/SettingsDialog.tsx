import { useState } from "react";
import { Modal } from "./Modal";
import { api, toTuxError } from "../lib/api";
import { forgetBanners } from "../hooks/useBanner";
import type { AppPaths, Backend, Dependencies, Settings } from "../lib/types";

const BACKENDS: { value: Backend; label: string; description: string }[] = [
  {
    value: "auto",
    label: "Automatique",
    description: "protontricks si présent, sinon Proton natif, sinon Wine système.",
  },
  {
    value: "protontricks",
    label: "protontricks",
    description: "protontricks -c \"wine '<trainer>'\" <AppID>",
  },
  {
    value: "proton",
    label: "Proton natif",
    description: "Le script proton du jeu, sans dépendance supplémentaire.",
  },
  {
    value: "wine",
    label: "Wine système",
    description: "WINEPREFIX pointé sur compatdata. Peut modifier le préfixe.",
  },
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
      onNotice(`${removed} fichier(s) de visuels supprimé(s).`);
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  return (
    <Modal
      title="Réglages"
      onClose={onClose}
      footer={
        <>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-ink-600 px-4 py-2 text-sm text-mist-300 transition-colors hover:bg-white/5"
          >
            Annuler
          </button>
          <button
            type="button"
            onClick={save}
            disabled={saving}
            className="rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-60"
          >
            {saving ? "Enregistrement…" : "Enregistrer"}
          </button>
        </>
      }
    >
      <div className="space-y-5 text-sm">
        <fieldset>
          <legend className="mb-2 font-medium text-mist-100">
            Méthode d'injection
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
                    {backend.label}
                  </span>
                  <span className="block font-mono text-xs text-mist-500">
                    {backend.description}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        <Toggle
          checked={draft.warnIfGameNotRunning}
          onChange={(value) => setDraft({ ...draft, warnIfGameNotRunning: value })}
          label="Avertir si le jeu n'est pas lancé"
          description="Demande confirmation avant d'injecter un trainer dans un jeu fermé."
        />
        <Toggle
          checked={draft.allowNetworkArtwork}
          onChange={(value) => setDraft({ ...draft, allowNetworkArtwork: value })}
          label="Télécharger les jaquettes manquantes"
          description="Utilise le CDN Steam quand le cache local ne contient pas le visuel."
        />

        <div className="space-y-2 border-t border-ink-700 pt-4 text-xs text-mist-500">
          <p>
            <span className="text-mist-400">protontricks :</span>{" "}
            {dependencies?.protontricks ??
              (dependencies?.protontricksFlatpak ? "Flatpak" : "non installé")}
          </p>
          <p>
            <span className="text-mist-400">wine :</span>{" "}
            {dependencies?.wine ?? "non installé"}
          </p>
          {paths && (
            <>
              <p className="break-all">
                <span className="text-mist-400">Configuration :</span> {paths.config}
              </p>
              <p className="break-all">
                <span className="text-mist-400">Cache visuels :</span>{" "}
                {paths.bannerCache}
              </p>
            </>
          )}
          <button
            type="button"
            onClick={clearCache}
            className="mt-1 rounded-md border border-ink-600 px-3 py-1.5 text-mist-300 transition-colors hover:bg-white/5"
          >
            Vider le cache des visuels
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
