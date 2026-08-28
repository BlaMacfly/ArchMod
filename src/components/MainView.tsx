import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Copy,
  FileDown,
  FolderOpen,
  HardDrive,
  Play,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";
import logoUrl from "../assets/logo.png";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { StatusPill } from "./StatusPill";
import { useBanner } from "../hooks/useBanner";
import { api, formatCommand, toTuxError } from "../lib/api";
import { accentFromAppId, basename, formatBytes, formatRelative } from "../lib/format";
import type { Dependencies, GameView } from "../lib/types";

interface MainViewProps {
  game: GameView | null;
  dependencies: Dependencies | null;
  busy: boolean;
  onImportTrainer: () => void;
  onRemoveTrainer: () => void;
  onLaunch: () => void;
  onStop: () => void;
  onNotice: (message: string) => void;
}

export function MainView({
  game,
  dependencies,
  busy,
  onImportTrainer,
  onRemoveTrainer,
  onLaunch,
  onStop,
  onNotice,
}: MainViewProps) {
  const hero = useBanner(game?.appId ?? null, "hero");
  const header = useBanner(game && !hero ? game.appId : null, "header");
  const logo = useBanner(game?.appId ?? null, "logo");
  const background = hero ?? header;

  const [command, setCommand] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);

  // Prévisualisation de la commande : purement informative, elle ne doit
  // jamais empêcher l'affichage de la fiche du jeu.
  useEffect(() => {
    if (!game?.trainer || game.trainerMissing) {
      setCommand(null);
      setCommandError(null);
      return;
    }
    let active = true;
    api
      .previewCommand(game.appId)
      .then((plan) => {
        if (!active) return;
        setCommand(formatCommand(plan));
        setCommandError(null);
      })
      .catch((error) => {
        if (!active) return;
        setCommand(null);
        setCommandError(toTuxError(error).message);
      });
    return () => {
      active = false;
    };
  }, [game?.appId, game?.trainer?.path, game?.trainerMissing]);

  const launchState = useMemo(() => {
    if (!game) return { label: "Sélectionne un jeu", disabled: true };
    if (game.trainerRunning) return { label: "Arrêter le trainer", disabled: false };
    if (!game.trainer) return { label: "Aucun trainer lié", disabled: true };
    if (game.trainerMissing) return { label: "Trainer introuvable", disabled: true };
    if (dependencies && !dependencies.ready)
      return { label: "Dépendances manquantes", disabled: true };
    return { label: "Lancer le trainer", disabled: false };
  }, [game, dependencies]);

  if (!game) {
    return (
      <section className="flex flex-1 items-center justify-center bg-ink-950 px-8 text-center">
        <div className="max-w-sm space-y-3">
          <img src={logoUrl} alt="" className="mx-auto h-20 w-20 opacity-90" />
          <h2 className="text-lg font-semibold text-mist-100">
            Choisis un jeu dans la liste
          </h2>
          <p className="text-sm text-mist-400">
            ArchMod associe un trainer Windows à chaque jeu Steam et l'injecte dans
            son préfixe Proton, sans passer par un terminal.
          </p>
        </div>
      </section>
    );
  }

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      onNotice(`${label} copié dans le presse-papiers.`);
    } catch {
      onNotice("Copie impossible : le presse-papiers est indisponible.");
    }
  };

  const reveal = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  return (
    <section className="scroll-slim flex-1 overflow-y-auto bg-ink-950">
      {/* Bannière */}
      <div className="relative h-56 shrink-0 overflow-hidden">
        {background ? (
          <img
            src={background}
            alt=""
            draggable={false}
            className="h-full w-full object-cover object-center"
          />
        ) : (
          <div
            className="h-full w-full"
            style={{ backgroundImage: accentFromAppId(game.appId) }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-ink-950 via-ink-950/70 to-ink-950/10" />

        <div className="absolute inset-x-0 bottom-0 flex items-end gap-4 px-8 pb-5">
          {logo ? (
            <img
              src={logo}
              alt={game.name}
              draggable={false}
              className="max-h-20 max-w-[22rem] object-contain drop-shadow-[0_4px_18px_rgba(0,0,0,0.65)]"
            />
          ) : (
            <h1 className="text-3xl font-bold tracking-tight text-white drop-shadow-lg">
              {game.name}
            </h1>
          )}
        </div>
      </div>

      <div className="animate-fade-up space-y-5 px-8 pb-10 pt-5">
        {/* Statuts */}
        <div className="flex flex-wrap items-center gap-2.5">
          <StatusPill
            running={game.running}
            title={game.running ? "Processus du jeu détecté" : "Aucun processus détecté"}
          />
          <StatusPill
            running={game.trainerRunning}
            compact
            neutralWhenOff
            labels={{ on: "Trainer actif", off: "Trainer arrêté" }}
          />
          <span className="rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            AppID {game.appId}
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            <HardDrive className="h-3.5 w-3.5" />
            {formatBytes(game.sizeOnDisk)}
          </span>
          <span className="rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            Joué {formatRelative(game.lastPlayed)}
          </span>
          {!game.prefixPath && (
            <span
              className="inline-flex items-center gap-1.5 rounded-full border border-warn-500/30 bg-warn-500/10 px-3 py-1 text-xs text-warn-500"
              title="Le préfixe est créé au premier lancement du jeu via Steam"
            >
              <AlertTriangle className="h-3.5 w-3.5" />
              Pas de préfixe Proton
            </span>
          )}
        </div>

        {/* Carte trainer */}
        <div className="rounded-card border border-ink-700 bg-ink-850 p-5">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <h2 className="text-sm font-semibold uppercase tracking-wider text-mist-400">
                Trainer associé
              </h2>
              {game.trainer ? (
                <>
                  <p className="mt-1.5 truncate text-lg font-semibold text-mist-100">
                    {basename(game.trainer.path)}
                  </p>
                  <p
                    className="mt-0.5 truncate font-mono text-xs text-mist-500"
                    title={game.trainer.path}
                  >
                    {game.trainer.path}
                  </p>
                  <p className="mt-2 text-xs text-mist-500">
                    Ajouté {formatRelative(game.trainer.addedAt)} · lancé{" "}
                    {game.trainer.launchCount} fois · dernier lancement{" "}
                    {formatRelative(game.trainer.lastLaunchedAt)}
                  </p>
                </>
              ) : (
                <p className="mt-1.5 text-sm text-mist-400">
                  Aucun trainer n'est encore lié à ce jeu.
                </p>
              )}
            </div>

            <div className="flex shrink-0 gap-2">
              <button
                type="button"
                onClick={onImportTrainer}
                className="inline-flex items-center gap-2 rounded-lg border border-ink-600 bg-ink-800 px-3.5 py-2 text-sm font-medium text-mist-100 transition-all hover:border-brand-500/50 hover:bg-ink-700 active:scale-[0.98]"
              >
                <FileDown className="h-4 w-4" />
                {game.trainer ? "Changer" : "Importer un trainer (.exe)"}
              </button>
              {game.trainer && (
                <>
                  <button
                    type="button"
                    onClick={() => reveal(game.trainer!.path)}
                    title="Afficher dans le gestionnaire de fichiers"
                    className="rounded-lg border border-ink-600 bg-ink-800 p-2 text-mist-400 transition-colors hover:text-mist-100"
                  >
                    <FolderOpen className="h-4 w-4" />
                  </button>
                  <button
                    type="button"
                    onClick={onRemoveTrainer}
                    title="Retirer l'association"
                    className="rounded-lg border border-ink-600 bg-ink-800 p-2 text-mist-400 transition-colors hover:border-halt-500/40 hover:text-halt-500"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </>
              )}
            </div>
          </div>

          {game.trainerMissing && (
            <p className="mt-4 flex items-start gap-2 rounded-lg border border-halt-500/30 bg-halt-500/10 px-3 py-2 text-sm text-halt-500">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              Le fichier n'existe plus à cet emplacement. Réimporte le trainer pour
              rétablir le lien.
            </p>
          )}
        </div>

        {/* Bouton principal */}
        <button
          type="button"
          disabled={launchState.disabled || busy}
          onClick={game.trainerRunning ? onStop : onLaunch}
          className={[
            "group relative flex w-full items-center justify-center gap-3 overflow-hidden rounded-card px-6 py-5 text-lg font-bold uppercase tracking-wide transition-all duration-200",
            launchState.disabled || busy
              ? "cursor-not-allowed bg-ink-800 text-mist-500"
              : game.trainerRunning
                ? "bg-halt-500/90 text-white hover:bg-halt-500 active:scale-[0.99]"
                : "bg-gradient-to-r from-spark-600 via-brand-700 to-brand-600 text-white shadow-[0_10px_34px_-12px_rgba(34,184,245,0.85)] hover:brightness-110 active:scale-[0.99]",
          ].join(" ")}
        >
          {game.trainerRunning ? (
            <Square className="h-5 w-5" />
          ) : (
            <Play className="h-5 w-5 fill-current" />
          )}
          {busy ? "Patiente…" : launchState.label}
        </button>

        {!game.running && game.trainer && !game.trainerMissing && (
          <p className="flex items-center gap-2 text-xs text-warn-500">
            <AlertTriangle className="h-3.5 w-3.5" />
            Le jeu n'est pas détecté : lance-le depuis Steam avant d'injecter le
            trainer (une confirmation te sera demandée).
          </p>
        )}

        {/* Détails techniques */}
        <details className="group rounded-card border border-ink-700 bg-ink-850">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-5 py-3 text-sm font-medium text-mist-300 transition-colors hover:text-mist-100">
            <Terminal className="h-4 w-4" />
            Commande et chemins
          </summary>
          <div className="space-y-3 border-t border-ink-700 px-5 py-4 text-xs">
            <Row label="Commande" value={command ?? commandError ?? "—"} onCopy={command ? () => copy(command, "Commande") : undefined} mono />
            <Row label="Installation" value={game.installPath} onCopy={() => copy(game.installPath, "Chemin")} mono />
            <Row label="Bibliothèque" value={game.libraryPath} mono />
            <Row
              label="Préfixe Proton"
              value={game.prefixPath ?? "non créé (lance le jeu une fois)"}
              mono
            />
          </div>
        </details>
      </div>
    </section>
  );
}

interface RowProps {
  label: string;
  value: string;
  mono?: boolean;
  onCopy?: () => void;
}

function Row({ label, value, mono, onCopy }: RowProps) {
  return (
    <div className="flex items-start gap-3">
      <span className="w-28 shrink-0 pt-0.5 uppercase tracking-wider text-mist-500">
        {label}
      </span>
      <span
        className={[
          "min-w-0 flex-1 break-all text-mist-300",
          mono ? "font-mono" : "",
        ].join(" ")}
      >
        {value}
      </span>
      {onCopy && (
        <button
          type="button"
          onClick={onCopy}
          title="Copier"
          className="shrink-0 rounded p-1 text-mist-500 transition-colors hover:bg-white/5 hover:text-mist-100"
        >
          <Copy className="h-3.5 w-3.5" />
        </button>
      )}
    </div>
  );
}
