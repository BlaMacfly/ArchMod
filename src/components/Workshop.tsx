import { useState } from "react";
import {
  FlaskConical,
  Plus,
  Save,
  Trash2,
} from "lucide-react";
import { api, formatAddress, toTuxError, valueNumber } from "../lib/api";
import type {
  Anchor,
  GameView,
  OptionStatus,
  Profile,
  TrainerOption,
  ValueTypeKind,
} from "../lib/types";

const VALUE_TYPES: { kind: ValueTypeKind; label: string }[] = [
  { kind: "fourBytes", label: "4 octets (entier)" },
  { kind: "float", label: "Flottant" },
  { kind: "byte", label: "1 octet" },
  { kind: "twoBytes", label: "2 octets" },
  { kind: "eightBytes", label: "8 octets" },
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
  onNotice,
}: WorkshopProps) {
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [probe, setProbe] = useState<OptionStatus | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

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
      onNotice("Donne un nom à l'option.");
      return;
    }
    const option = draftToOption(draft);
    if (profile.options.some((existing) => existing.id === option.id)) {
      onNotice(`Une option porte déjà l'identifiant « ${option.id} ».`);
      return;
    }
    onProfileChange({ ...profile, options: [...profile.options, option] });
    setDraft({ ...EMPTY, category: draft.category, module: draft.module });
    setProbe(null);
    onNotice(`Option « ${option.name} » ajoutée au profil.`);
  };

  const remove = (id: string) =>
    onProfileChange({
      ...profile,
      options: profile.options.filter((option) => option.id !== id),
    });

  const save = async () => {
    try {
      const path = await api.saveProfile(profile);
      onNotice(`Profil enregistré : ${path}`);
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  return (
    <div className="grid gap-4 lg:grid-cols-[1fr_20rem]">
      {/* Éditeur */}
      <div className="space-y-4 rounded-card border border-ink-700 bg-ink-850 p-5">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Nom de l'option">
            <input
              value={draft.name}
              onChange={(event) => patch({ name: event.target.value })}
              placeholder="Endurance infinie"
              className={INPUT}
            />
          </Field>
          <Field label="Catégorie">
            <input
              value={draft.category}
              onChange={(event) => patch({ category: event.target.value })}
              placeholder="Joueur"
              className={INPUT}
            />
          </Field>
          <Field label="Type de valeur">
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
          <Field label="Contrôle">
            <select
              value={draft.control}
              onChange={(event) =>
                patch({ control: event.target.value as Draft["control"] })
              }
              className={INPUT}
            >
              <option value="toggle">Interrupteur (gèle la valeur)</option>
              <option value="number">Valeur saisie</option>
              <option value="action">Bouton à effet unique</option>
            </select>
          </Field>
          <Field
            label={
              draft.control === "number" ? "Valeur par défaut" : "Valeur imposée"
            }
          >
            <input
              value={draft.frozen}
              onChange={(event) => patch({ frozen: event.target.value })}
              className={INPUT}
            />
          </Field>
          <Field label="Raccourci (facultatif)">
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
                  ? "Motif d'octets (recommandé)"
                  : "Décalage depuis un module"}
              </button>
            ))}
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Module">
              <input
                value={draft.module}
                onChange={(event) => patch({ module: event.target.value })}
                placeholder="GameAssembly.dll"
                className={INPUT}
              />
            </Field>

            {draft.anchorKind === "aob" ? (
              <>
                <Field label="Décalage dans le motif">
                  <input
                    value={draft.patternOffset}
                    onChange={(event) =>
                      patch({ patternOffset: event.target.value })
                    }
                    className={INPUT}
                  />
                </Field>
                <Field label="Motif d'octets" wide>
                  <input
                    value={draft.pattern}
                    onChange={(event) => patch({ pattern: event.target.value })}
                    placeholder="48 8B 89 ?? ?? 00 00"
                    className={`${INPUT} font-mono`}
                  />
                </Field>
                <Field label="Correspondance n°">
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
              <Field label="Décalage (hex accepté)">
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

            <Field label="Chaîne de pointeurs" wide>
              <input
                value={draft.offsets}
                onChange={(event) => patch({ offsets: event.target.value })}
                placeholder="0x18 0xC0 — appliqués dans l'ordre"
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
            Déréférencer l'ancrage (équivalent des crochets de Cheat Engine)
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
            {testing ? "Essai…" : "Tester sur le jeu"}
          </button>
          <button
            type="button"
            onClick={add}
            className="inline-flex items-center gap-2 rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950"
          >
            <Plus className="h-4 w-4" />
            Ajouter au profil
          </button>

          {!game.running && (
            <span className="text-xs text-warn-500">
              Lance le jeu pour pouvoir éprouver une adresse.
            </span>
          )}
        </div>

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
              <>
                Adresse résolue :{" "}
                <span className="font-mono">{formatAddress(probe?.address ?? null)}</span>
                {" — valeur actuelle : "}
                <span className="font-mono">{valueNumber(probe?.value ?? null)}</span>
              </>
            )}
          </div>
        )}
      </div>

      {/* Profil en construction */}
      <aside className="space-y-3 rounded-card border border-ink-700 bg-ink-850 p-4">
        <header className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-mist-100">
            Profil ({profile.options.length})
          </h3>
          <button
            type="button"
            onClick={() => void save()}
            disabled={profile.options.length === 0}
            className="inline-flex items-center gap-1.5 rounded-md border border-ink-600 px-2.5 py-1 text-xs text-mist-300 transition-colors hover:bg-white/5 disabled:opacity-50"
          >
            <Save className="h-3.5 w-3.5" />
            Enregistrer
          </button>
        </header>

        <p className="text-[11px] text-mist-500">
          Build {profile.buildId ?? "inconnu"} — un profil ne vaut que pour la
          version du jeu sur laquelle il a été relevé.
        </p>

        {profile.options.length === 0 ? (
          <p className="py-6 text-center text-xs text-mist-500">
            Aucune option. Éprouve une adresse, puis ajoute-la.
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
                  title="Retirer"
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
