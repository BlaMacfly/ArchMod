import { Gamepad2, Loader2, Play } from "lucide-react";
import { useI18n } from "../i18n";

/**
 * Lancement du jeu depuis ArchMod, à la manière de WeMod : plus besoin de
 * repasser par la fenêtre de Steam pour que les adresses mémoire existent.
 */
export function PlayButton({
  running,
  launching,
  onLaunch,
}: {
  running: boolean;
  launching: boolean;
  onLaunch: () => void;
}) {
  const { t } = useI18n();

  if (running) {
    return (
      <span className="inline-flex items-center gap-2 rounded-lg border border-live-500/40 bg-live-500/15 px-4 py-2.5 text-sm font-semibold text-live-500 backdrop-blur">
        <Gamepad2 className="h-4 w-4" />
        {t("main.gameRunning")}
      </span>
    );
  }

  return (
    <button
      type="button"
      disabled={launching}
      onClick={onLaunch}
      title={t("main.playHint")}
      className={[
        "inline-flex items-center gap-2 rounded-lg px-5 py-2.5 text-sm font-bold uppercase tracking-wide transition-all",
        launching
          ? "cursor-wait bg-ink-800/90 text-mist-400 backdrop-blur"
          : "bg-white/95 text-ink-950 shadow-[0_8px_26px_-10px_rgba(0,0,0,0.9)] hover:bg-white active:scale-[0.98]",
      ].join(" ")}
    >
      {launching ? (
        <Loader2 className="h-4 w-4 animate-spin" />
      ) : (
        <Play className="h-4 w-4 fill-current" />
      )}
      {launching ? t("main.gameStarting") : t("main.play")}
    </button>
  );
}

/** Même action, en lien discret, là où l'absence du jeu bloque une opération. */
export function LaunchGameLink({
  launching,
  onLaunch,
}: {
  launching: boolean;
  onLaunch: () => void;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      disabled={launching}
      onClick={onLaunch}
      className="inline-flex items-center gap-1.5 rounded-md border border-current/30 px-2 py-0.5 font-medium transition-colors hover:bg-white/10 disabled:cursor-wait disabled:opacity-60"
    >
      {launching ? (
        <Loader2 className="h-3 w-3 animate-spin" />
      ) : (
        <Play className="h-3 w-3 fill-current" />
      )}
      {launching ? t("main.gameStarting") : t("main.play")}
    </button>
  );
}
