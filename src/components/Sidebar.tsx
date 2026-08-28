import { useMemo, useState } from "react";
import { Filter, Gamepad2, RefreshCw, Search, Zap } from "lucide-react";
import { GameThumb } from "./GameThumb";
import type { GameView } from "../lib/types";

interface SidebarProps {
  games: GameView[];
  selectedAppId: number | null;
  loading: boolean;
  onSelect: (appId: number) => void;
  onRescan: () => void;
}

type Filtre = "tous" | "trainers" | "actifs";

export function Sidebar({
  games,
  selectedAppId,
  loading,
  onSelect,
  onRescan,
}: SidebarProps) {
  const [query, setQuery] = useState("");
  const [filtre, setFiltre] = useState<Filtre>("tous");

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return games.filter((game) => {
      if (filtre === "trainers" && !game.trainer) return false;
      if (filtre === "actifs" && !game.running) return false;
      if (!needle) return true;
      return (
        game.name.toLowerCase().includes(needle) ||
        String(game.appId).includes(needle)
      );
    });
  }, [games, query, filtre]);

  const runningCount = games.filter((game) => game.running).length;

  return (
    <aside className="flex w-[19rem] shrink-0 flex-col border-r border-ink-700/70 bg-ink-900">
      <div className="space-y-3 px-4 pb-3 pt-4">
        <div className="relative">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-mist-500" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Rechercher un jeu…"
            aria-label="Rechercher un jeu"
            className="w-full rounded-lg border border-ink-600/80 bg-ink-800 py-2 pl-9 pr-3 text-sm text-mist-100 placeholder:text-mist-500 outline-none transition-colors focus:border-brand-500/70 focus:ring-2 focus:ring-brand-500/20"
          />
        </div>

        <div className="flex items-center gap-1 rounded-lg bg-ink-800 p-1">
          {(
            [
              ["tous", "Tous"],
              ["trainers", "Trainers"],
              ["actifs", "Actifs"],
            ] as [Filtre, string][]
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              onClick={() => setFiltre(value)}
              className={[
                "flex-1 rounded-md px-2 py-1 text-xs font-medium transition-colors",
                filtre === value
                  ? "bg-brand-500/20 text-brand-400"
                  : "text-mist-400 hover:bg-white/5 hover:text-mist-100",
              ].join(" ")}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="flex items-center justify-between text-[11px] uppercase tracking-wider text-mist-500">
          <span className="inline-flex items-center gap-1.5">
            <Filter className="h-3 w-3" />
            {filtered.length} / {games.length} jeux
            {runningCount > 0 && (
              <span className="text-live-500">· {runningCount} actif(s)</span>
            )}
          </span>
          <button
            type="button"
            onClick={onRescan}
            title="Rescanner la bibliothèque Steam"
            className="rounded-md p-1 text-mist-400 transition-colors hover:bg-white/5 hover:text-mist-100"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      <div className="scroll-slim flex-1 overflow-y-auto px-2 pb-3">
        {filtered.length === 0 ? (
          <p className="px-3 py-8 text-center text-sm text-mist-500">
            {loading ? "Scan de la bibliothèque…" : "Aucun jeu ne correspond."}
          </p>
        ) : (
          <ul className="space-y-1">
            {filtered.map((game) => {
              const selected = game.appId === selectedAppId;
              return (
                <li key={game.appId}>
                  <button
                    type="button"
                    onClick={() => onSelect(game.appId)}
                    aria-current={selected}
                    className={[
                      "group flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left transition-all duration-150",
                      selected
                        ? "bg-brand-500/15 ring-1 ring-brand-500/40"
                        : "hover:bg-white/5",
                    ].join(" ")}
                  >
                    <GameThumb appId={game.appId} name={game.name} />
                    <span className="min-w-0 flex-1">
                      <span
                        className={[
                          "block truncate text-sm font-medium",
                          selected ? "text-white" : "text-mist-100",
                        ].join(" ")}
                        title={game.name}
                      >
                        {game.name}
                      </span>
                      <span className="mt-0.5 flex items-center gap-2 text-[11px] text-mist-500">
                        <span
                          className={[
                            "h-1.5 w-1.5 rounded-full",
                            game.running ? "bg-live-500" : "bg-ink-500",
                          ].join(" ")}
                        />
                        {game.running ? "en cours" : `AppID ${game.appId}`}
                      </span>
                    </span>
                    {game.trainer && (
                      <Zap
                        className={[
                          "h-3.5 w-3.5 shrink-0",
                          game.trainerMissing
                            ? "text-halt-500"
                            : game.trainerRunning
                              ? "text-live-500"
                              : "text-brand-400",
                        ].join(" ")}
                        aria-label={
                          game.trainerMissing ? "Trainer introuvable" : "Trainer lié"
                        }
                      />
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <footer className="flex items-center gap-2 border-t border-ink-700/70 px-4 py-2.5 text-[11px] text-mist-500">
        <Gamepad2 className="h-3.5 w-3.5" />
        Bibliothèque Steam locale
      </footer>
    </aside>
  );
}
