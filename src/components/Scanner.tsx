import { useCallback, useEffect, useState } from "react";
import {
  Link2,
  Play,
  Plus,
  RotateCcw,
  Search,
  Snowflake,
  Trash2,
} from "lucide-react";
import {
  api,
  formatAddress,
  formatPointerPath,
  makeValue,
  toTuxError,
  valueNumber,
} from "../lib/api";
import { useI18n, type TranslationKey } from "../i18n";
import type {
  CandidateView,
  Filter,
  GameView,
  PointerPath,
  PointerScanReport,
  Profile,
  ScanReport,
  TrainerOption,
  ValueTypeKind,
} from "../lib/types";

const VALUE_TYPES: { kind: ValueTypeKind; label: string }[] = [
  { kind: "fourBytes", label: "4 Bytes" },
  { kind: "float", label: "Float" },
  { kind: "eightBytes", label: "8 Bytes" },
  { kind: "twoBytes", label: "2 Bytes" },
  { kind: "byte", label: "Byte" },
  { kind: "double", label: "Double" },
];

type FilterKind = Filter["kind"];

/** Les filtres d'évolution n'ont de sens qu'après une première recherche. */
const FILTERS: { kind: FilterKind; label: TranslationKey; needsValue: boolean }[] = [
  { kind: "exact", label: "scanner.filterExact", needsValue: true },
  { kind: "greater", label: "scanner.filterGreater", needsValue: true },
  { kind: "less", label: "scanner.filterLess", needsValue: true },
  { kind: "increased", label: "scanner.filterIncreased", needsValue: false },
  { kind: "decreased", label: "scanner.filterDecreased", needsValue: false },
  { kind: "changed", label: "scanner.filterChanged", needsValue: false },
  { kind: "unchanged", label: "scanner.filterUnchanged", needsValue: false },
];

interface ScannerProps {
  game: GameView;
  /** Profil en construction dans l'atelier, enrichi par les chemins trouvés. */
  draft: Profile | null;
  onDraftChange: (profile: Profile) => void;
  onNotice: (message: string) => void;
}

/**
 * Recherche de valeurs façon Cheat Engine : on cherche un nombre connu, on le
 * fait varier dans le jeu, on relance — et l'intersection isole l'adresse.
 */
export function Scanner({ game, draft, onDraftChange, onNotice }: ScannerProps) {
  const { t } = useI18n();
  const [valueType, setValueType] = useState<ValueTypeKind>("fourBytes");
  const [filterKind, setFilterKind] = useState<FilterKind>("exact");
  const [input, setInput] = useState("");
  const [report, setReport] = useState<ScanReport | null>(null);
  const [results, setResults] = useState<CandidateView[]>([]);
  const [frozen, setFrozen] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [pointers, setPointers] = useState<PointerScanReport | null>(null);
  const [hunting, setHunting] = useState<number | null>(null);

  const started = report !== null;
  const definition = FILTERS.find((entry) => entry.kind === filterKind)!;

  // Les valeurs affichées doivent suivre le jeu, sinon on lit un instantané mort.
  useEffect(() => {
    if (!started || !game.running) return;
    const interval = window.setInterval(() => {
      api
        .scanRefresh(game.appId)
        .then(setResults)
        .catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(interval);
  }, [started, game.appId, game.running]);

  const buildFilter = useCallback((): Filter | null => {
    if (!definition.needsValue) return { kind: filterKind } as Filter;
    const parsed = Number(input.replace(",", "."));
    if (!Number.isFinite(parsed)) return null;
    const value = makeValue(valueType, parsed);
    return value ? ({ kind: filterKind, value } as Filter) : null;
  }, [definition, filterKind, input, valueType]);

  const run = async (first: boolean) => {
    const filter = buildFilter();
    if (!filter) {
      onNotice(t("scanner.needsValue"));
      return;
    }
    setBusy(true);
    try {
      const outcome = first
        ? await api.scanStart(game.appId, { kind: valueType }, filter)
        : await api.scanNext(game.appId, filter);
      setReport(outcome);
      setResults(outcome.sample);
    } catch (error) {
      onNotice(toTuxError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const reset = async () => {
    await api.scanReset(game.appId).catch(() => undefined);
    setReport(null);
    setResults([]);
    setFrozen(new Set());
  };

  const write = async (candidate: CandidateView) => {
    const parsed = Number(input.replace(",", "."));
    const value = Number.isFinite(parsed) ? makeValue(valueType, parsed) : null;
    if (!value) {
      onNotice(t("scanner.needsValue"));
      return;
    }
    try {
      await api.scanWrite(game.appId, candidate.address, value);
      onNotice(t("scanner.written", { address: formatAddress(candidate.address) }));
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  /** Remonte d'une adresse volatile jusqu'à un ancrage statique. */
  const huntPointers = async (candidate: CandidateView) => {
    setHunting(candidate.address);
    setPointers(null);
    try {
      setPointers(await api.pointerScan(game.appId, candidate.address));
    } catch (error) {
      onNotice(toTuxError(error).message);
    } finally {
      setHunting(null);
    }
  };

  /** Transforme un chemin vérifié en option du profil en construction. */
  const adopt = async (path: PointerPath) => {
    try {
      const reached = await api.pointerVerify(game.appId, path);
      if (reached !== pointers?.target) {
        onNotice(t("pointer.mismatch"));
        return;
      }
      if (!draft) return;

      const index = draft.options.length + 1;
      const option: TrainerOption = {
        id: `valeur-${index}`,
        category: t("pointer.foundCategory"),
        name: `${t("pointer.foundName")} ${index}`,
        description: formatPointerPath(path),
        valueType: { kind: valueType },
        control: { control: "number", min: null, max: null, default: null, freeze: true },
        address: {
          anchor: { kind: "module", module: path.module, offset: path.baseOffset },
          // Une variable globale contient un pointeur : il faut le suivre.
          dereference: true,
          offsets: path.offsets,
        },
        hotkey: null,
      };
      onDraftChange({ ...draft, options: [...draft.options, option] });
      onNotice(t("pointer.adopted"));
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  const toggleFreeze = async (candidate: CandidateView) => {
    const isFrozen = frozen.has(candidate.address);
    try {
      await api.scanFreeze(
        game.appId,
        candidate.address,
        isFrozen ? null : candidate.value,
      );
      setFrozen((current) => {
        const next = new Set(current);
        if (isFrozen) next.delete(candidate.address);
        else next.add(candidate.address);
        return next;
      });
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  if (!game.running) {
    return (
      <div className="rounded-card border border-warn-500/30 bg-warn-500/10 px-5 py-4 text-sm">
        <p className="font-medium text-warn-500">
          {t("panel.gameNotRunning", { game: game.name })}
        </p>
        <p className="mt-1 text-mist-400">{t("panel.gameNotRunningHint")}</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="rounded-card border border-ink-700 bg-ink-850 p-5">
        <p className="mb-4 text-xs text-mist-500">{t("scanner.how")}</p>

        <div className="grid gap-3 sm:grid-cols-3">
          <label>
            <span className="mb-1 block text-[11px] uppercase tracking-wider text-mist-500">
              {t("workshop.valueType")}
            </span>
            <select
              value={valueType}
              disabled={started}
              onChange={(event) =>
                setValueType(event.target.value as ValueTypeKind)
              }
              className={INPUT}
            >
              {VALUE_TYPES.map((type) => (
                <option key={type.kind} value={type.kind}>
                  {type.label}
                </option>
              ))}
            </select>
          </label>

          <label>
            <span className="mb-1 block text-[11px] uppercase tracking-wider text-mist-500">
              {t("scanner.filter")}
            </span>
            <select
              value={filterKind}
              onChange={(event) =>
                setFilterKind(event.target.value as FilterKind)
              }
              className={INPUT}
            >
              {FILTERS.map((entry) => (
                <option
                  key={entry.kind}
                  value={entry.kind}
                  disabled={!started && !entry.needsValue}
                >
                  {t(entry.label)}
                </option>
              ))}
            </select>
          </label>

          <label>
            <span className="mb-1 block text-[11px] uppercase tracking-wider text-mist-500">
              {t("scanner.value")}
            </span>
            <input
              value={input}
              onChange={(event) => setInput(event.target.value)}
              onKeyDown={(event) =>
                event.key === "Enter" && void run(!started)
              }
              disabled={!definition.needsValue}
              inputMode="decimal"
              placeholder="247"
              className={`${INPUT} disabled:opacity-40`}
            />
          </label>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => void run(!started)}
            disabled={busy}
            className="inline-flex items-center gap-2 rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-50"
          >
            <Search className="h-4 w-4" />
            {busy
              ? t("scanner.searching")
              : started
                ? t("scanner.refine")
                : t("scanner.first")}
          </button>

          {started && (
            <button
              type="button"
              onClick={() => void reset()}
              className="inline-flex items-center gap-2 rounded-lg border border-ink-600 px-3.5 py-2 text-sm text-mist-300 transition-colors hover:bg-white/5"
            >
              <RotateCcw className="h-4 w-4" />
              {t("scanner.reset")}
            </button>
          )}

          {report && (
            <span className="text-xs text-mist-400">
              {t("scanner.matches", {
                count: report.matches,
                ms: report.elapsedMs,
              })}
              {report.truncated && (
                <span className="text-warn-500"> · {t("scanner.truncated")}</span>
              )}
            </span>
          )}
        </div>
      </div>

      {results.length > 0 && (
        <div className="overflow-hidden rounded-card border border-ink-700 bg-ink-850">
          <table className="w-full text-sm">
            <thead className="bg-ink-800/60 text-[11px] uppercase tracking-wider text-mist-500">
              <tr>
                <th className="px-4 py-2 text-left font-medium">
                  {t("scanner.address")}
                </th>
                <th className="px-4 py-2 text-right font-medium">
                  {t("scanner.currentValue")}
                </th>
                <th className="px-4 py-2 text-left font-medium">
                  {t("scanner.anchorage")}
                </th>
                <th className="px-4 py-2" />
              </tr>
            </thead>
            <tbody className="divide-y divide-ink-700/60">
              {results.slice(0, 50).map((candidate) => (
                <tr key={candidate.address}>
                  <td className="px-4 py-2 font-mono text-xs text-mist-300">
                    {formatAddress(candidate.address)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-mist-100">
                    {valueNumber(candidate.value)}
                    {candidate.previous !== null && (
                      <span className="ml-2 text-xs text-mist-500">
                        ← {valueNumber(candidate.previous)}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-xs">
                    {candidate.anchorage.kind === "module" ? (
                      <span
                        className="text-live-500"
                        title={t("scanner.anchoredHint")}
                      >
                        {candidate.anchorage.module}+
                        {candidate.anchorage.offset.toString(16).toUpperCase()}
                      </span>
                    ) : (
                      <span
                        className="text-mist-500"
                        title={t("scanner.volatileHint")}
                      >
                        {t("scanner.volatile")}
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-2">
                    <div className="flex justify-end gap-1.5">
                      <button
                        type="button"
                        onClick={() => void write(candidate)}
                        title={t("scanner.write")}
                        className="rounded-md border border-ink-600 p-1.5 text-mist-400 transition-colors hover:border-brand-500/50 hover:text-brand-400"
                      >
                        <Play className="h-3.5 w-3.5" />
                      </button>
                      {candidate.anchorage.kind === "volatile" && (
                        <button
                          type="button"
                          onClick={() => void huntPointers(candidate)}
                          disabled={hunting !== null}
                          title={t("pointer.hunt")}
                          className="rounded-md border border-ink-600 p-1.5 text-mist-400 transition-colors hover:border-brand-500/50 hover:text-brand-400 disabled:opacity-40"
                        >
                          <Link2 className="h-3.5 w-3.5" />
                        </button>
                      )}
                      <button
                        type="button"
                        onClick={() => void toggleFreeze(candidate)}
                        title={t("scanner.freeze")}
                        className={[
                          "rounded-md border p-1.5 transition-colors",
                          frozen.has(candidate.address)
                            ? "border-brand-500/60 bg-brand-500/15 text-brand-400"
                            : "border-ink-600 text-mist-400 hover:text-mist-100",
                        ].join(" ")}
                      >
                        <Snowflake className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {report && report.matches > results.length && (
            <p className="border-t border-ink-700 px-4 py-2 text-xs text-mist-500">
              {t("scanner.narrow", { shown: results.length, total: report.matches })}
            </p>
          )}
        </div>
      )}

      {hunting !== null && (
        <p className="rounded-card border border-ink-700 bg-ink-850 px-5 py-4 text-sm text-mist-400">
          {t("pointer.hunting", { address: formatAddress(hunting) })}
        </p>
      )}

      {pointers && (
        <div className="rounded-card border border-ink-700 bg-ink-850 p-5">
          <h3 className="text-sm font-semibold text-mist-100">
            {t("pointer.title", { address: formatAddress(pointers.target) })}
          </h3>
          <p className="mt-1 text-xs text-mist-500">
            {t("pointer.summary", {
              count: pointers.paths.length,
              pointers: pointers.pointers,
              ms: pointers.elapsedMs,
            })}
          </p>

          {pointers.paths.length === 0 ? (
            <p className="mt-3 text-xs text-warn-500">{t("pointer.none")}</p>
          ) : (
            <ul className="mt-3 space-y-1.5">
              {pointers.paths.slice(0, 12).map((path, index) => (
                <li
                  key={index}
                  className="flex items-center gap-3 rounded-lg bg-ink-800 px-3 py-2"
                >
                  <span className="min-w-0 flex-1 truncate font-mono text-xs text-mist-200">
                    {formatPointerPath(path)}
                  </span>
                  <span className="shrink-0 text-[11px] text-mist-500">
                    {t("pointer.levels", { count: path.offsets.length })}
                  </span>
                  <button
                    type="button"
                    onClick={() => void adopt(path)}
                    disabled={!draft}
                    title={t("pointer.adopt")}
                    className="shrink-0 rounded-md border border-ink-600 p-1.5 text-mist-400 transition-colors hover:border-brand-500/50 hover:text-brand-400 disabled:opacity-40"
                  >
                    <Plus className="h-3.5 w-3.5" />
                  </button>
                </li>
              ))}
            </ul>
          )}
          <p className="mt-3 text-[11px] text-mist-500">{t("pointer.verifyNote")}</p>
        </div>
      )}

      {started && results.length === 0 && (
        <p className="rounded-card border border-ink-700 bg-ink-850 px-5 py-8 text-center text-sm text-mist-500">
          {t("scanner.noResult")}
        </p>
      )}

      <p className="flex items-start gap-2 text-xs text-mist-500">
        <Trash2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
        {t("scanner.pointerNote")}
      </p>
    </div>
  );
}

const INPUT =
  "w-full rounded-lg border border-ink-600 bg-ink-800 px-3 py-2 text-sm text-mist-100 placeholder:text-mist-500 outline-none transition-colors focus:border-brand-500/70";
