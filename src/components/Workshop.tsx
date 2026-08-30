import { useState } from "react";
import {
  FileInput,
  FlaskConical,
  Plus,
  Save,
  Trash2,
} from "lucide-react";
import {
  confirm as askConfirmation,
  open as openFileDialog,
} from "@tauri-apps/plugin-dialog";
import { api, formatAddress, toTuxError, valueNumber } from "../lib/api";
import { useI18n } from "../i18n";
import type {
  Anchor,
  GameView,
  OptionStatus,
  Profile,
  SkippedEntry,
  TrainerOption,
  ValueTypeKind,
} from "../lib/types";

/** Les types de valeur portent des noms techniques, identiques dans toutes les
 *  langues : « 4 Bytes », « Float »… ce sont ceux de Cheat Engine. */
const VALUE_TYPES: { kind: ValueTypeKind; label: string }[] = [
  { kind: "fourBytes", label: "4 Bytes" },
  { kind: "float", label: "Float" },
  { kind: "byte", label: "Byte" },
  { kind: "twoBytes", label: "2 Bytes" },
  { kind: "eightBytes", label: "8 Bytes" },
  { kind: "double", label: "Double" },
];

/** Brouillon d'option en cours d'édition, avant conversion en `TrainerOption`. */
interface Draft {
  name: string;
  category: string;
  valueType: ValueTypeKind;
  control: "toggle" | "number" | "action";
  frozen: string;
  anchorKind: "module" | "aob";
  module: string;
  moduleOffset: string;
  pattern: string;
  patternOffset: string;
  occurrence: string;
  dereference: boolean;
  offsets: string;
  hotkey: string;
}

const EMPTY: Draft = {
  name: "",
  category: "Joueur",
  valueType: "fourBytes",
  control: "toggle",
  frozen: "999",
  anchorKind: "aob",
  module: "",
  moduleOffset: "0x0",
  pattern: "",
  patternOffset: "0",
  occurrence: "0",
  dereference: false,
  offsets: "",
  hotkey: "",
};

/** Identifiant stable dérivé du libellé, comme un slug d'URL. */
function slugify(name: string): string {
  return (
    name
      .toLowerCase()
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || "option"
  );
}

function parseNumber(raw: string): number {
  const trimmed = raw.trim();
  if (!trimmed) return 0;
  return trimmed.toLowerCase().startsWith("0x")
    ? Number.parseInt(trimmed.slice(2), 16)
    : Number(trimmed);
}

function draftToOption(draft: Draft): TrainerOption {
  const anchor: Anchor =
    draft.anchorKind === "module"
      ? {
          kind: "module",
          module: draft.module.trim(),
          offset: parseNumber(draft.moduleOffset),
        }
      : {
          kind: "aob",
          module: draft.module.trim(),
          pattern: draft.pattern.trim(),
          offset: parseNumber(draft.patternOffset),
          occurrence: parseNumber(draft.occurrence),
        };

  const frozen = parseNumber(draft.frozen);
  return {
    id: slugify(draft.name),
    category: draft.category.trim() || "Divers",
    name: draft.name.trim(),
    description: null,
    valueType: { kind: draft.valueType },
    control:
      draft.control === "toggle"
        ? { control: "toggle", frozen: { type: draft.valueType, value: frozen } as never }
        : draft.control === "action"
          ? { control: "action", value: { type: draft.valueType, value: frozen } as never }
          : {
              control: "number",
              min: null,
              max: null,
              default: frozen,
              freeze: true,
            },
    address: {
      anchor,
      dereference: draft.dereference,
      offsets: draft.offsets
        .split(/[,\s]+/)
        .filter(Boolean)
        .map(parseNumber),
    },
    hotkey: draft.hotkey.trim() || null,
  };
}

interface WorkshopProps {
  game: GameView;
  profile: Profile;
  onProfileChange: (profile: Profile) => void;
  /// Prévient la vue parente qu'un profil vient d'être écrit sur le disque.
  onSaved: () => void;
  onNotice: (message: string) => void;
}

/**
 * Atelier de création : l'auteur saisit une recette d'adresse, l'éprouve sur le
 * jeu en cours, puis l'ajoute au profil. L'essai en direct est l'essentiel —
 * sans lui, on écrirait du JSON à l'aveugle.
 */
export function Workshop({
  game,
  profile,
  onProfileChange,
  onSaved,
  onNotice,
}: WorkshopProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [probe, setProbe] = useState<OptionStatus | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [skipped, setSkipped] = useState<SkippedEntry[]>([]);
  const [script, setScript] = useState("");
  const [symbols, setSymbols] = useState<Record<string, number> | null>(null);
  const [running, setRunning] = useState(false);

  const patch = (fields: Partial<Draft>) =>
    setDraft((current) => ({ ...current, ...fields }));

  const test = async () => {
    setTesting(true);
    setProbeError(null);
    setProbe(null);
    try {
      const option = draftToOption(draft);
      const status = await api.probeRecipe(
        game.appId,
        option.address,
        option.valueType,
      );
      setProbe(status);
    } catch (error) {
      setProbeError(toTuxError(error).message);
    } finally {
      setTesting(false);
    }
  };

  const add = () => {
    if (!draft.name.trim()) {
      onNotice(t("workshop.needsName"));
      return;
    }
    const option = draftToOption(draft);
    if (profile.options.some((existing) => existing.id === option.id)) {
      onNotice(t("workshop.duplicateId", { id: option.id }));
      return;
    }
    onProfileChange({ ...profile, options: [...profile.options, option] });
    setDraft({ ...EMPTY, category: draft.category, module: draft.module });
    setProbe(null);
    onNotice(t("workshop.added", { name: option.name }));
  };

  /**
   * Reprend une table Cheat Engine publiée. Les entrées dont l'adresse repose
   * sur un module, ou sur un symbole qu'un scan de la table produit, sont
   * converties ; les autres sont listées avec leur raison, plutôt que
   * silencieusement perdues.
   */
  const importTable = async () => {
    try {
      const chosen = await openFileDialog({
        multiple: false,
        directory: false,
        title: t("workshop.importTable"),
        filters: [{ name: "Table Cheat Engine", extensions: ["CT", "ct", "xml"] }],
      });
      if (typeof chosen !== "string") return;

      const imported = await api.importCheatTable(game.appId, chosen);
      const known = new Set(profile.options.map((option) => option.id));
      const added = imported.profile.options.filter(
        (option) => !known.has(option.id),
      );

      onProfileChange({ ...profile, options: [...profile.options, ...added] });
      setSkipped(imported.skipped);
      // Le script de la table est repris tel quel : c'est lui qui donnera vie
      // aux symboles dont dépendent les entrées écartées.
      if (imported.scripts.length > 0) setScript(imported.scripts[0]);
      onNotice(
        t("workshop.tableImported", {
          added: added.length,
          skipped: imported.skipped.length,
        }),
      );
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  const runScript = async () => {
    setRunning(true);
    try {
      const report = await api.runScript(game.appId, script);
      setSymbols(report.symbols);
      onNotice(
        t("script.done", {
          count: Object.keys(report.symbols).length,
          address: formatAddress(report.allocation),
        }),
      );
    } catch (error) {
      onNotice(toTuxError(error).message);
    } finally {
      setRunning(false);
    }
  };

  const revertScript = async () => {
    try {
      const reverted = await api.revertScript(game.appId);
      setSymbols(null);
      onNotice(reverted ? t("script.reverted") : t("script.nothing"));
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  const remove = (id: string) =>
    onProfileChange({
      ...profile,
      options: profile.options.filter((option) => option.id !== id),
    });

  const save = async (overwrite = false) => {
    try {
      const path = await api.saveProfile(profile, overwrite);
      // Le panneau doit voir immédiatement le profil qu'on vient d'écrire.
      onSaved();
      onNotice(t("workshop.saved", { path }));
    } catch (error) {
      const failure = toTuxError(error);
      // Deux profils pour le même build portent le même nom : on ne remplace
      // jamais le travail d'un autre sans le demander.
      if (failure.kind === "profile_exists" && !overwrite) {
        if (await askConfirmation(t("workshop.overwrite"))) {
          await save(true);
        }
        return;
      }
      onNotice(failure.message);
    }
  };

  return (
    <div className="grid gap-4 lg:grid-cols-[1fr_20rem]">
      {/* Éditeur */}
      <div className="space-y-4 rounded-card border border-ink-700 bg-ink-850 p-5">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label={t("workshop.name")}>
            <input
              value={draft.name}
              onChange={(event) => patch({ name: event.target.value })}
              placeholder={t("workshop.namePlaceholder")}
              className={INPUT}
            />
          </Field>
          <Field label={t("workshop.category")}>
            <input
              value={draft.category}
              onChange={(event) => patch({ category: event.target.value })}
              placeholder={t("workshop.categoryPlaceholder")}
              className={INPUT}
            />
          </Field>
          <Field label={t("workshop.valueType")}>
            <select
              value={draft.valueType}
              onChange={(event) =>
                patch({ valueType: event.target.value as ValueTypeKind })
              }
              className={INPUT}
            >
              {VALUE_TYPES.map((type) => (
                <option key={type.kind} value={type.kind}>
                  {type.label}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t("workshop.control")}>
            <select
              value={draft.control}
              onChange={(event) =>
                patch({ control: event.target.value as Draft["control"] })
              }
              className={INPUT}
            >
              <option value="toggle">{t("workshop.controlToggle")}</option>
              <option value="number">{t("workshop.controlNumber")}</option>
              <option value="action">{t("workshop.controlAction")}</option>
            </select>
          </Field>
          <Field
            label={
              draft.control === "number"
                ? t("workshop.defaultValue")
                : t("workshop.forcedValue")
            }
          >
            <input
              value={draft.frozen}
              onChange={(event) => patch({ frozen: event.target.value })}
              className={INPUT}
            />
          </Field>
          <Field label={t("workshop.hotkey")}>
            <input
              value={draft.hotkey}
              onChange={(event) => patch({ hotkey: event.target.value })}
              placeholder="Numpad1"
              className={INPUT}
            />
          </Field>
        </div>

        <div className="border-t border-ink-700 pt-4">
          <div className="mb-3 flex gap-1 rounded-lg bg-ink-800 p-1">
            {(["aob", "module"] as const).map((kind) => (
              <button
                key={kind}
                type="button"
                onClick={() => patch({ anchorKind: kind })}
                className={[
                  "flex-1 rounded-md px-3 py-1.5 text-xs font-medium transition-colors",
                  draft.anchorKind === kind
                    ? "bg-brand-500/20 text-brand-400"
                    : "text-mist-400 hover:text-mist-100",
                ].join(" ")}
              >
                {kind === "aob"
                  ? t("workshop.anchorAob")
                  : t("workshop.anchorModule")}
              </button>
            ))}
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <Field label={t("workshop.module")}>
              <input
                value={draft.module}
                onChange={(event) => patch({ module: event.target.value })}
                placeholder="GameAssembly.dll"
                className={INPUT}
              />
            </Field>

            {draft.anchorKind === "aob" ? (
              <>
                <Field label={t("workshop.patternOffset")}>
                  <input
                    value={draft.patternOffset}
                    onChange={(event) =>
                      patch({ patternOffset: event.target.value })
                    }
                    className={INPUT}
                  />
                </Field>
                <Field label={t("workshop.pattern")} wide>
                  <input
                    value={draft.pattern}
                    onChange={(event) => patch({ pattern: event.target.value })}
                    placeholder="48 8B 89 ?? ?? 00 00"
                    className={`${INPUT} font-mono`}
                  />
                </Field>
                <Field label={t("workshop.occurrence")}>
                  <input
                    value={draft.occurrence}
                    onChange={(event) =>
                      patch({ occurrence: event.target.value })
                    }
                    className={INPUT}
                  />
                </Field>
              </>
            ) : (
              <Field label={t("workshop.moduleOffset")}>
                <input
                  value={draft.moduleOffset}
                  onChange={(event) =>
                    patch({ moduleOffset: event.target.value })
                  }
                  placeholder="0x4194FD4"
                  className={`${INPUT} font-mono`}
                />
              </Field>
            )}

            <Field label={t("workshop.pointers")} wide>
              <input
                value={draft.offsets}
                onChange={(event) => patch({ offsets: event.target.value })}
                placeholder={t("workshop.pointersPlaceholder")}
                className={`${INPUT} font-mono`}
              />
            </Field>
          </div>

          <label className="mt-3 flex cursor-pointer items-center gap-2 text-sm text-mist-300">
            <input
              type="checkbox"
              checked={draft.dereference}
              onChange={(event) => patch({ dereference: event.target.checked })}
              className="accent-[var(--color-brand-500)]"
            />
            {t("workshop.dereference")}
          </label>
        </div>

        <div className="flex flex-wrap items-center gap-2 border-t border-ink-700 pt-4">
          <button
            type="button"
            onClick={() => void test()}
            disabled={testing || !game.running}
            className="inline-flex items-center gap-2 rounded-lg border border-brand-500/50 bg-brand-500/10 px-4 py-2 text-sm font-medium text-brand-400 transition-colors hover:bg-brand-500/20 disabled:opacity-50"
          >
            <FlaskConical className="h-4 w-4" />
            {testing ? t("workshop.testing") : t("workshop.test")}
          </button>
          <button
            type="button"
            onClick={add}
            className="inline-flex items-center gap-2 rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950"
          >
            <Plus className="h-4 w-4" />
            {t("workshop.add")}
          </button>

          <button
            type="button"
            onClick={() => void importTable()}
            className="inline-flex items-center gap-2 rounded-lg border border-ink-600 bg-ink-800 px-4 py-2 text-sm font-medium text-mist-100 transition-colors hover:border-brand-500/50 hover:bg-ink-700"
          >
            <FileInput className="h-4 w-4" />
            {t("workshop.importTable")}
          </button>

          {!game.running && (
            <span className="text-xs text-warn-500">
              {t("workshop.needsRunning")}
            </span>
          )}
        </div>

        {script && (
          <details className="rounded-lg border border-ink-600 bg-ink-800 px-4 py-3">
            <summary className="cursor-pointer text-sm text-mist-200">
              {t("script.title")}
            </summary>
            <p className="mt-2 text-xs text-mist-500">{t("script.explain")}</p>
            <textarea
              value={script}
              onChange={(event) => setScript(event.target.value)}
              spellCheck={false}
              rows={10}
              className="mt-2 w-full rounded-lg border border-ink-600 bg-ink-900 px-3 py-2 font-mono text-xs text-mist-200 outline-none focus:border-brand-500/70"
            />
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={() => void runScript()}
                disabled={running || !game.running}
                className="rounded-lg bg-brand-600 px-3.5 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-50"
              >
                {running ? t("script.running") : t("script.run")}
              </button>
              <button
                type="button"
                onClick={() => void revertScript()}
                className="rounded-lg border border-ink-600 px-3.5 py-1.5 text-xs text-mist-300 transition-colors hover:bg-white/5"
              >
                {t("script.revert")}
              </button>
              <span className="text-[11px] text-warn-500">{t("script.warning")}</span>
            </div>

            {symbols && Object.keys(symbols).length > 0 && (
              <ul className="mt-3 space-y-1 font-mono text-xs">
                {Object.entries(symbols).map(([name, address]) => (
                  <li key={name} className="text-live-500">
                    {name} = {formatAddress(address)}
                  </li>
                ))}
              </ul>
            )}
          </details>
        )}

        {skipped.length > 0 && (
          <details className="rounded-lg border border-ink-600 bg-ink-800 px-4 py-3 text-xs">
            <summary className="cursor-pointer text-mist-300">
              {t("workshop.skippedTitle", { count: skipped.length })}
            </summary>
            <ul className="mt-2 space-y-1 text-mist-500">
              {skipped.map((entry, index) => (
                <li key={index}>
                  <span className="text-mist-300">
                    {entry.description || t("workshop.unnamed")}
                  </span>{" "}
                  — {entry.reason}
                </li>
              ))}
            </ul>
          </details>
        )}

        {(probe || probeError) && (
          <div
            className={[
              "rounded-lg border px-4 py-3 text-sm",
              probeError
                ? "border-halt-500/30 bg-halt-500/10 text-halt-500"
                : "border-live-500/30 bg-live-500/10 text-live-500",
            ].join(" ")}
          >
            {probeError ?? (
              t("workshop.resolved", {
                address: formatAddress(probe?.address ?? null),
                value: String(valueNumber(probe?.value ?? null)),
              })
            )}
          </div>
        )}
      </div>

      {/* Profil en construction */}
      <aside className="space-y-3 rounded-card border border-ink-700 bg-ink-850 p-4">
        <header className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-mist-100">
            {t("workshop.profileTitle", { count: profile.options.length })}
          </h3>
          <button
            type="button"
            onClick={() => void save()}
            disabled={profile.options.length === 0}
            className="inline-flex items-center gap-1.5 rounded-md border border-ink-600 px-2.5 py-1 text-xs text-mist-300 transition-colors hover:bg-white/5 disabled:opacity-50"
          >
            <Save className="h-3.5 w-3.5" />
            {t("workshop.saveProfile")}
          </button>
        </header>

        <p className="text-[11px] text-mist-500">
          {t("workshop.buildNote", {
            build: profile.buildId ?? t("prefix.unknown"),
          })}
        </p>

        {profile.options.length === 0 ? (
          <p className="py-6 text-center text-xs text-mist-500">
            {t("workshop.emptyProfile")}
          </p>
        ) : (
          <ul className="space-y-1">
            {profile.options.map((option) => (
              <li
                key={option.id}
                className="flex items-center gap-2 rounded-lg bg-ink-800 px-3 py-2"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-xs text-mist-100">
                    {option.name}
                  </span>
                  <span className="block truncate text-[10px] text-mist-500">
                    {option.category} · {option.control.control}
                  </span>
                </span>
                <button
                  type="button"
                  onClick={() => remove(option.id)}
                  title={t("workshop.remove")}
                  className="shrink-0 rounded p-1 text-mist-500 transition-colors hover:text-halt-500"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </aside>
    </div>
  );
}

const INPUT =
  "w-full rounded-lg border border-ink-600 bg-ink-800 px-3 py-2 text-sm text-mist-100 placeholder:text-mist-500 outline-none transition-colors focus:border-brand-500/70";

function Field({
  label,
  children,
  wide,
}: {
  label: string;
  children: React.ReactNode;
  wide?: boolean;
}) {
  return (
    <label className={wide ? "sm:col-span-2" : undefined}>
      <span className="mb-1 block text-[11px] uppercase tracking-wider text-mist-500">
        {label}
      </span>
      {children}
    </label>
  );
}
