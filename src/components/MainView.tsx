import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Copy,
  FileDown,
  FolderOpen,
  HardDrive,
  Pencil,
  Play,
  Square,
  Terminal,
  Trash2,
} from "lucide-react";
import logoUrl from "../assets/logo.png";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { LaunchGameLink, PlayButton } from "./PlayButton";
import { StatusPill } from "./StatusPill";
import { PrefixCard } from "./PrefixCard";
import { Scanner } from "./Scanner";
import { TrainerPanel } from "./TrainerPanel";
import { Workshop } from "./Workshop";
import { useTrainer } from "../hooks/useTrainer";
import { useI18n, type TranslationKey } from "../i18n";
import { useBanner } from "../hooks/useBanner";
import { api, formatCommand, toTuxError } from "../lib/api";
import { accentFromAppId, basename, formatBytes, formatRelative } from "../lib/format";
import type { Dependencies, GameView, Profile } from "../lib/types";

interface MainViewProps {
  game: GameView | null;
  dependencies: Dependencies | null;
  busy: boolean;
  /** Un lancement de jeu est en attente de détection du processus. */
  launchingGame: boolean;
  onImportTrainer: () => void;
  onRemoveTrainer: () => void;
  onLaunch: () => void;
  onLaunchGame: () => void;
  onStop: () => void;
  onNotice: (message: string) => void;
}

export function MainView({
  game,
  dependencies,
  busy,
  launchingGame,
  onImportTrainer,
  onRemoveTrainer,
  onLaunch,
  onLaunchGame,
  onStop,
  onNotice,
}: MainViewProps) {
  const hero = useBanner(game?.appId ?? null, "hero");
  const header = useBanner(game && !hero ? game.appId : null, "header");
  const logo = useBanner(game?.appId ?? null, "logo");
  const background = hero ?? header;

  const [command, setCommand] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);
  const { t } = useI18n();
  const [tab, setTab] = useState<Onglet>("lanceur");

  const trainer = useTrainer(
    game?.appId ?? null,
    game?.name ?? "",
    game?.buildId ?? null,
    (error) => onNotice(toTuxError(error).message),
  );

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
    if (!game) return { label: t("main.selectGame"), disabled: true };
    if (game.trainerRunning) return { label: t("main.stop"), disabled: false };
    if (!game.trainer) return { label: t("main.noTrainerButton"), disabled: true };
    if (game.trainerMissing)
      return { label: t("main.trainerMissingButton"), disabled: true };
    if (dependencies && !dependencies.ready)
      return { label: t("main.depsMissingButton"), disabled: true };
    return { label: t("main.launch"), disabled: false };
  }, [game, dependencies, t]);

  if (!game) {
    return (
      <section className="flex flex-1 items-center justify-center bg-ink-950 px-8 text-center">
        <div className="max-w-sm space-y-3">
          <img src={logoUrl} alt="" className="mx-auto h-20 w-20 opacity-90" />
          <h2 className="text-lg font-semibold text-mist-100">
            {t("main.emptyTitle")}
          </h2>
          <p className="text-sm text-mist-400">{t("main.emptyText")}</p>
        </div>
      </section>
    );
  }

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      onNotice(t("app.copied", { what: label }));
    } catch {
      onNotice(t("app.copyFailed"));
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

          <div className="ml-auto shrink-0">
            <PlayButton
              running={game.running}
              launching={launchingGame}
              onLaunch={onLaunchGame}
            />
          </div>
        </div>
      </div>

      <div className="animate-fade-up space-y-5 px-8 pb-10 pt-5">
        {/* Statuts */}
        <div className="flex flex-wrap items-center gap-2.5">
          <StatusPill
            running={game.running}
            title={
              game.running
                ? t("main.processDetected")
                : t("main.processNotDetected")
            }
          />
          <StatusPill
            running={game.trainerRunning}
            compact
            neutralWhenOff
            labels={{ on: t("main.trainerActive"), off: t("main.trainerStopped") }}
          />
          <span className="rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            AppID {game.appId}
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            <HardDrive className="h-3.5 w-3.5" />
            {formatBytes(game.sizeOnDisk)}
          </span>
          <span className="rounded-full border border-ink-600 bg-ink-800 px-3 py-1 text-xs text-mist-400">
            {t("main.played", { when: formatRelative(game.lastPlayed) })}
          </span>
          {!game.prefixPath && (
            <span
              className="inline-flex items-center gap-1.5 rounded-full border border-warn-500/30 bg-warn-500/10 px-3 py-1 text-xs text-warn-500"
              title={t("main.noPrefixHint")}
            >
              <AlertTriangle className="h-3.5 w-3.5" />
              {t("main.noPrefix")}
            </span>
          )}
        </div>

        {/* Onglets */}
        <div className="flex gap-1 rounded-lg bg-ink-850 p-1">
          {ONGLETS.map(([value, label, hint]) => (
            <button
              key={value}
              type="button"
              onClick={() => setTab(value)}
              title={t(hint)}
              className={[
                "flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                tab === value
                  ? "bg-brand-500/20 text-brand-400"
                  : "text-mist-400 hover:bg-white/5 hover:text-mist-100",
              ].join(" ")}
            >
              {t(label)}
            </button>
          ))}
        </div>

        {tab === "panneau" && (
          <PanneauSection
            game={game}
            trainer={trainer}
            onEdit={(profile) => {
              trainer.setDraft(profile);
              setTab("atelier");
            }}
            onNotice={onNotice}
          />
        )}

        {tab === "scanner" && (
          <Scanner
            game={game}
            draft={trainer.draft}
            launching={launchingGame}
            onLaunchGame={onLaunchGame}
            onDraftChange={trainer.setDraft}
            onNotice={onNotice}
          />
        )}

        {tab === "atelier" && trainer.draft && (
          <Workshop
            game={game}
            profile={trainer.draft}
            onProfileChange={trainer.setDraft}
            onSaved={() => void trainer.refreshProfiles()}
            onNotice={onNotice}
          />
        )}

        {tab === "lanceur" && (
          <>
        {/* Carte trainer */}
        <div className="rounded-card border border-ink-700 bg-ink-850 p-5">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <h2 className="text-sm font-semibold uppercase tracking-wider text-mist-400">
                {t("main.trainerCard")}
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
                    {t("main.trainerStats", {
                      added: formatRelative(game.trainer.addedAt),
                      count: game.trainer.launchCount,
                      last: formatRelative(game.trainer.lastLaunchedAt),
                    })}
                  </p>
                </>
              ) : (
                <>
                  <p className="mt-1.5 text-sm text-mist-400">
                    {t("main.noTrainerLinked")}
                  </p>
                  {/* Deux autres formats s'importent ailleurs : sans cette
                      phrase, rien ne permet de le deviner. */}
                  <p className="mt-1 max-w-lg text-xs text-mist-500">
                    {t("main.otherFormats")}
                  </p>
                </>
              )}
            </div>

            <div className="flex shrink-0 gap-2">
              <button
                type="button"
                onClick={onImportTrainer}
                className="inline-flex items-center gap-2 rounded-lg border border-ink-600 bg-ink-800 px-3.5 py-2 text-sm font-medium text-mist-100 transition-all hover:border-brand-500/50 hover:bg-ink-700 active:scale-[0.98]"
              >
                <FileDown className="h-4 w-4" />
                {game.trainer ? t("main.change") : t("main.import")}
              </button>
              {game.trainer && (
                <>
                  <button
                    type="button"
                    onClick={() => reveal(game.trainer!.path)}
                    title={t("main.reveal")}
                    className="rounded-lg border border-ink-600 bg-ink-800 p-2 text-mist-400 transition-colors hover:text-mist-100"
                  >
                    <FolderOpen className="h-4 w-4" />
                  </button>
                  <button
                    type="button"
                    onClick={onRemoveTrainer}
                    title={t("main.unlink")}
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
              {t("main.trainerMissingWarning")}
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
          {busy ? t("main.working") : launchState.label}
        </button>

        {!game.running && game.trainer && !game.trainerMissing && (
          <p className="flex flex-wrap items-center gap-2 text-xs text-warn-500">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("main.notRunningWarning")}
            <LaunchGameLink launching={launchingGame} onLaunch={onLaunchGame} />
          </p>
        )}

        {/* État du préfixe Proton */}
        <PrefixCard appId={game.appId} onNotice={onNotice} />

        {/* Détails techniques */}
        <details className="group rounded-card border border-ink-700 bg-ink-850">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-5 py-3 text-sm font-medium text-mist-300 transition-colors hover:text-mist-100">
            <Terminal className="h-4 w-4" />
            {t("main.details")}
          </summary>
          <div className="space-y-3 border-t border-ink-700 px-5 py-4 text-xs">
            <Row
              label={t("main.rowCommand")}
              value={command ?? commandError ?? "—"}
              onCopy={
                command
                  ? () => copy(command, t("main.rowCommand"))
                  : undefined
              }
              mono
            />
            <Row
              label={t("main.rowInstall")}
              value={game.installPath}
              onCopy={() => copy(game.installPath, t("main.rowInstall"))}
              mono
            />
            <Row label={t("main.rowLibrary")} value={game.libraryPath} mono />
            <Row
              label={t("main.rowPrefix")}
              value={game.prefixPath ?? t("main.prefixMissing")}
              mono
            />
          </div>
        </details>
          </>
        )}
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
  const { t } = useI18n();
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
          title={t("main.copy")}
          className="shrink-0 rounded p-1 text-mist-500 transition-colors hover:bg-white/5 hover:text-mist-100"
        >
          <Copy className="h-3.5 w-3.5" />
        </button>
      )}
    </div>
  );
}

/**
 * Importe un profil téléchargé. Sans ce bouton, le dépôt communautaire ne
 * servirait à rien : il faudrait copier le fichier à la main.
 */
function ImportButton({
  trainer,
  onNotice,
}: {
  trainer: ReturnType<typeof useTrainer>;
  onNotice: (message: string) => void;
}) {
  const { t } = useI18n();
  const importer = async () => {
    try {
      const chosen = await openFileDialog({
        multiple: false,
        directory: false,
        title: t("panel.importProfile"),
        filters: [{ name: "ArchMod", extensions: ["json"] }],
      });
      if (typeof chosen !== "string") return;

      const profile = await api.importProfile(chosen);
      await trainer.refreshProfiles();
      onNotice(
        t("panel.profileImported", {
          game: profile.game,
          count: profile.options.length,
        }),
      );
    } catch (error) {
      onNotice(toTuxError(error).message);
    }
  };

  return (
    <button
      type="button"
      onClick={() => void importer()}
      className="inline-flex items-center gap-2 rounded-lg border border-ink-600 bg-ink-800 px-3.5 py-2 text-sm font-medium text-mist-100 transition-colors hover:border-brand-500/50 hover:bg-ink-700"
    >
      <FileDown className="h-4 w-4" />
      {t("panel.importProfile")}
    </button>
  );
}

type Onglet = "lanceur" | "panneau" | "scanner" | "atelier";

const ONGLETS: [Onglet, TranslationKey, TranslationKey][] = [
  ["lanceur", "main.tabLauncher", "main.tabLauncherHint"],
  ["panneau", "main.tabPanel", "main.tabPanelHint"],
  ["scanner", "scanner.tab", "scanner.tabHint"],
  ["atelier", "main.tabWorkshop", "main.tabWorkshopHint"],
];

interface PanneauSectionProps {
  game: GameView;
  trainer: ReturnType<typeof useTrainer>;
  /** Reprend un profil existant dans l'atelier, pour le corriger. */
  onEdit: (profile: Profile) => void;
  onNotice: (message: string) => void;
}

/** Choix du profil, puis panneau d'options une fois celui-ci chargé. */
function PanneauSection({
  game,
  trainer,
  onEdit,
  onNotice,
}: PanneauSectionProps) {
  const { t } = useI18n();
  const { entries, active, report, busy } = trainer;

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

  if (entries.length === 0) {
    return (
      <div className="rounded-card border border-ink-700 bg-ink-850 px-5 py-8 text-center">
        <p className="text-sm text-mist-300">
          {t("panel.noProfile", { game: game.name })}
        </p>
        <p className="mx-auto mt-2 max-w-md text-xs text-mist-500">
          {t("panel.noProfileHint")}
        </p>
        <div className="mt-4">
          <ImportButton trainer={trainer} onNotice={onNotice} />
        </div>
      </div>
    );
  }

  if (!active || !report) {
    return (
      <>
      <div className="mb-2 flex justify-end">
        <ImportButton trainer={trainer} onNotice={onNotice} />
      </div>
      <ul className="space-y-2">
        {entries.map(({ profile, buildMatch }) => (
          <li
            key={`${profile.buildId}-${profile.options.length}`}
            className="flex items-center gap-4 rounded-card border border-ink-700 bg-ink-850 px-5 py-4"
          >
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium text-mist-100">
                {profile.game} — {t("panel.optionCount", { count: profile.options.length })}
              </p>
              <p className="mt-0.5 text-xs text-mist-500">
                {profile.author ? t("panel.byAuthor", { author: profile.author }) : ""}
                {profile.buildId
                  ? t("panel.forBuild", { build: profile.buildId })
                  : t("panel.buildUnknown")}
              </p>
              {buildMatch.state === "outdated" && (
                <p className="mt-1 flex items-center gap-1.5 text-xs text-warn-500">
                  <AlertTriangle className="h-3.5 w-3.5" />
                  {t("panel.outdated", {
                    profile: buildMatch.detail.profile,
                    installed: buildMatch.detail.installed,
                  })}
                </p>
              )}
            </div>
            <button
              type="button"
              onClick={() => onEdit(profile)}
              title={t("panel.editInWorkshop")}
              className="shrink-0 rounded-lg border border-ink-600 p-2 text-mist-400 transition-colors hover:border-brand-500/50 hover:text-brand-400"
            >
              <Pencil className="h-4 w-4" />
            </button>
            <button
              type="button"
              disabled={busy === "profil"}
              onClick={() => void trainer.activate(profile)}
              className="shrink-0 rounded-lg bg-brand-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-brand-500 hover:text-ink-950 disabled:opacity-50"
            >
              {busy === "profil" ? t("panel.loading") : t("panel.load")}
            </button>
          </li>
        ))}
      </ul>
      </>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3 rounded-card border border-ink-700 bg-ink-850 px-5 py-3">
        <span className="min-w-0 flex-1 text-sm text-mist-300">
          {t("panel.resolved", { count: report.resolved })}
          {report.failed > 0 && (
            <span className="text-warn-500">
              {t("panel.failed", { count: report.failed })}
            </span>
          )}
          <span className="text-mist-500">{t("panel.pid", { pid: report.pid })}</span>
        </span>
        <button
          type="button"
          onClick={() => {
            void trainer.deactivate();
            onNotice(t("panel.unloaded"));
          }}
          className="shrink-0 rounded-lg border border-ink-600 px-3 py-1.5 text-xs text-mist-300 transition-colors hover:bg-white/5"
        >
          {t("panel.unload")}
        </button>
      </div>

      <TrainerPanel
        profile={active}
        report={report}
        busy={busy}
        debug
        onSet={trainer.setOption}
        onClear={trainer.clearOption}
      />
    </div>
  );
}
