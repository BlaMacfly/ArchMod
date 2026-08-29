import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronDown,
  Crosshair,
  Info,
  Zap,
} from "lucide-react";
import { formatAddress, makeValue, valueNumber } from "../lib/api";
import type {
  ActivationReport,
  OptionStatus,
  Profile,
  TrainerOption,
  Value,
} from "../lib/types";

interface TrainerPanelProps {
  profile: Profile;
  report: ActivationReport | null;
  busy: string | null;
  /** Affiche les adresses résolues, utile pour mettre au point un profil. */
  debug: boolean;
  onSet: (option: TrainerOption, value?: Value) => void;
  onClear: (option: TrainerOption) => void;
}

/** Panneau de triche : une section par catégorie, un contrôle par option. */
export function TrainerPanel({
  profile,
  report,
  busy,
  debug,
  onSet,
  onClear,
}: TrainerPanelProps) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const statuses = useMemo(() => {
    const map = new Map<string, OptionStatus>();
    report?.options.forEach((status) => map.set(status.id, status));
    return map;
  }, [report]);

  const categories = useMemo(() => {
    const groups: { name: string; options: TrainerOption[] }[] = [];
    for (const option of profile.options) {
      const existing = groups.find((group) => group.name === option.category);
      if (existing) existing.options.push(option);
      else groups.push({ name: option.category, options: [option] });
    }
    return groups;
  }, [profile]);

  return (
    <div className="space-y-3">
      {categories.map((category) => {
        const folded = collapsed[category.name] ?? false;
        return (
          <section
            key={category.name}
            className="overflow-hidden rounded-card border border-ink-700 bg-ink-850"
          >
            <button
              type="button"
              onClick={() =>
                setCollapsed((current) => ({
                  ...current,
                  [category.name]: !folded,
                }))
              }
              className="flex w-full items-center gap-2.5 bg-ink-800/60 px-5 py-3 text-left transition-colors hover:bg-ink-800"
            >
              <Crosshair className="h-4 w-4 text-brand-400" />
              <span className="flex-1 text-sm font-semibold text-mist-100">
                {category.name}
              </span>
              <span className="text-[11px] text-mist-500">
                {category.options.length}
              </span>
              <ChevronDown
                className={`h-4 w-4 text-mist-400 transition-transform ${
                  folded ? "-rotate-90" : ""
                }`}
              />
            </button>

            {!folded && (
              <ul className="divide-y divide-ink-700/60">
                {category.options.map((option) => (
                  <OptionRow
                    key={option.id}
                    option={option}
                    status={statuses.get(option.id)}
                    busy={busy === option.id}
                    debug={debug}
                    onSet={onSet}
                    onClear={onClear}
                  />
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
}

interface OptionRowProps {
  option: TrainerOption;
  status: OptionStatus | undefined;
  busy: boolean;
  debug: boolean;
  onSet: (option: TrainerOption, value?: Value) => void;
  onClear: (option: TrainerOption) => void;
}

function OptionRow({
  option,
  status,
  busy,
  debug,
  onSet,
  onClear,
}: OptionRowProps) {
  const unresolved = !status?.address;
  const active = status?.active ?? false;

  return (
    <li className="flex items-center gap-4 px-5 py-3">
      <Zap
        className={`h-4 w-4 shrink-0 ${
          active ? "text-live-500" : unresolved ? "text-ink-500" : "text-brand-400"
        }`}
      />

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span
            className={`truncate text-sm ${
              unresolved ? "text-mist-500" : "text-mist-100"
            }`}
          >
            {option.name}
          </span>
          {option.description && (
            <span title={option.description}>
              <Info className="h-3.5 w-3.5 shrink-0 text-mist-500" />
            </span>
          )}
        </div>

        {unresolved && status?.error && (
          <p
            className="mt-0.5 flex items-center gap-1.5 truncate text-[11px] text-warn-500"
            title={status.error}
          >
            <AlertTriangle className="h-3 w-3 shrink-0" />
            {status.error}
          </p>
        )}
        {debug && status?.address && (
          <p className="mt-0.5 font-mono text-[11px] text-mist-500">
            {formatAddress(status.address)}
            {status.value !== null && ` = ${valueNumber(status.value)}`}
          </p>
        )}
      </div>

      <OptionControl
        option={option}
        status={status}
        busy={busy}
        disabled={unresolved}
        onSet={onSet}
        onClear={onClear}
      />

      {option.hotkey && (
        <span className="hidden shrink-0 rounded-md border border-ink-600 bg-ink-800 px-2.5 py-1 font-mono text-[11px] text-mist-400 lg:inline">
          {option.hotkey}
        </span>
      )}
    </li>
  );
}

interface OptionControlProps {
  option: TrainerOption;
  status: OptionStatus | undefined;
  busy: boolean;
  disabled: boolean;
  onSet: (option: TrainerOption, value?: Value) => void;
  onClear: (option: TrainerOption) => void;
}

function OptionControl({
  option,
  status,
  busy,
  disabled,
  onSet,
  onClear,
}: OptionControlProps) {
  const active = status?.active ?? false;
  const control = option.control;

  const [draft, setDraft] = useState<string>(() => {
    if (control.control === "number" && control.default !== null) {
      return String(control.default);
    }
    return "";
  });

  if (control.control === "toggle") {
    return (
      <div className="flex shrink-0 overflow-hidden rounded-lg border border-ink-600">
        {[false, true].map((wanted) => (
          <button
            key={String(wanted)}
            type="button"
            disabled={disabled || busy}
            onClick={() => (wanted ? onSet(option) : onClear(option))}
            className={[
              "px-4 py-1.5 text-xs font-semibold transition-colors",
              active === wanted
                ? wanted
                  ? "bg-brand-600 text-white"
                  : "bg-ink-600 text-mist-200"
                : "bg-ink-800 text-mist-500 hover:text-mist-200",
              disabled ? "cursor-not-allowed opacity-50" : "",
            ].join(" ")}
          >
            {wanted ? "On" : "Off"}
          </button>
        ))}
      </div>
    );
  }

  if (control.control === "action") {
    return (
      <button
        type="button"
        disabled={disabled || busy}
        onClick={() => onSet(option)}
        className="shrink-0 rounded-lg bg-brand-600 px-4 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {busy ? "…" : "Appliquer"}
      </button>
    );
  }

  const apply = () => {
    const parsed = Number(draft);
    if (!Number.isFinite(parsed)) return;
    const value = makeValue(option.valueType.kind, parsed);
    if (value) onSet(option, value);
  };

  return (
    <div className="flex shrink-0 items-center gap-2">
      <input
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => event.key === "Enter" && apply()}
        inputMode="decimal"
        aria-label={`Valeur pour ${option.name}`}
        className="w-24 rounded-lg border border-ink-600 bg-ink-800 px-3 py-1.5 text-right text-sm text-mist-100 outline-none focus:border-brand-500/70"
      />
      <button
        type="button"
        disabled={disabled || busy}
        onClick={active ? () => onClear(option) : apply}
        className={[
          "rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-50",
          active
            ? "bg-live-600 text-white hover:bg-halt-500"
            : "bg-brand-600 text-white hover:bg-brand-500 hover:text-ink-950",
        ].join(" ")}
      >
        {active ? "Gelé" : "Appliquer"}
      </button>
    </div>
  );
}
