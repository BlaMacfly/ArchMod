import { Activity, CircleSlash } from "lucide-react";
import { useI18n } from "../i18n";

interface StatusPillProps {
  running: boolean;
  /** Libellés personnalisés (« Trainer actif » / « Trainer arrêté », ...). */
  labels?: { on: string; off: string };
  title?: string;
  compact?: boolean;
  /** État « éteint » neutre plutôt qu'alarmant (trainer non lancé). */
  neutralWhenOff?: boolean;
}

/** Pastille d'état : vert quand ça tourne, rouge sinon. */
export function StatusPill({
  running,
  labels,
  title,
  compact,
  neutralWhenOff,
}: StatusPillProps) {
  const { t } = useI18n();
  const text = running
    ? (labels?.on ?? t("main.statusRunning"))
    : (labels?.off ?? t("main.statusClosed"));
  const Icon = running ? Activity : CircleSlash;

  return (
    <span
      title={title}
      className={[
        "inline-flex items-center gap-2 rounded-full border font-medium transition-colors",
        compact ? "px-2 py-0.5 text-[11px]" : "px-3 py-1 text-xs",
        running
          ? "border-live-500/30 bg-live-500/10 text-live-500"
          : neutralWhenOff
            ? "border-ink-600 bg-ink-800 text-mist-400"
            : "border-halt-500/25 bg-halt-500/10 text-halt-500",
      ].join(" ")}
    >
      <span className="relative flex h-2 w-2">
        <span
          className={[
            "h-2 w-2 rounded-full",
            running
              ? "bg-live-500 animate-pulse-ring"
              : neutralWhenOff
                ? "bg-ink-500"
                : "bg-halt-500",
          ].join(" ")}
        />
      </span>
      <Icon className={compact ? "h-3 w-3" : "h-3.5 w-3.5"} aria-hidden />
      {text}
    </span>
  );
}
