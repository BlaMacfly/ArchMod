import { useBanner } from "../hooks/useBanner";
import { accentFromAppId, initials } from "../lib/format";

interface GameThumbProps {
  appId: number;
  name: string;
  className?: string;
}

/**
 * Jaquette verticale d'un jeu, avec repli sur une vignette générée quand
 * aucun visuel n'est disponible (cache Steam vide et réseau désactivé).
 */
export function GameThumb({ appId, name, className }: GameThumbProps) {
  const portrait = useBanner(appId, "portrait");
  const header = useBanner(portrait ? null : appId, "header");
  const source = portrait ?? header;

  return (
    <div
      className={[
        "relative shrink-0 overflow-hidden rounded-md bg-ink-700 ring-1 ring-white/5",
        className ?? "h-14 w-10",
      ].join(" ")}
      style={source ? undefined : { backgroundImage: accentFromAppId(appId) }}
    >
      {source ? (
        <img
          src={source}
          alt=""
          loading="lazy"
          draggable={false}
          className="h-full w-full object-cover"
        />
      ) : (
        <span className="flex h-full w-full items-center justify-center text-xs font-semibold text-white/80">
          {initials(name)}
        </span>
      )}
    </div>
  );
}
