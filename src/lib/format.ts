/** Petits utilitaires d'affichage partagés par les composants. */

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  const units = ["o", "Ko", "Mo", "Go", "To"];
  const exponent = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / 1024 ** exponent;
  return `${value.toFixed(value >= 10 || exponent === 0 ? 0 : 1)} ${units[exponent]}`;
}

/** Timestamp Unix (secondes) → « il y a 3 jours ». */
export function formatRelative(seconds: number | null | undefined): string {
  if (!seconds) return "jamais";
  const deltaSeconds = Date.now() / 1000 - seconds;
  if (deltaSeconds < 60) return "à l'instant";

  const steps: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "minute"],
    [3600, "hour"],
    [86400, "day"],
    [604800, "week"],
    [2629800, "month"],
    [31557600, "year"],
  ];

  let chosen: [number, Intl.RelativeTimeFormatUnit] = steps[0];
  for (const step of steps) {
    if (deltaSeconds >= step[0]) chosen = step;
  }
  const formatter = new Intl.RelativeTimeFormat("fr", { numeric: "auto" });
  return formatter.format(-Math.round(deltaSeconds / chosen[0]), chosen[1]);
}

export function formatClock(milliseconds: number): string {
  return new Date(milliseconds).toLocaleTimeString("fr-FR", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function basename(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Couleur stable dérivée de l'AppID, pour les vignettes sans jaquette. */
export function accentFromAppId(appId: number): string {
  const hue = (appId * 47) % 360;
  return `linear-gradient(150deg, hsl(${hue} 62% 32%), hsl(${(hue + 48) % 360} 58% 18%))`;
}

/** Initiales affichées dans une vignette de repli. */
export function initials(name: string): string {
  return name
    .replace(/[^\p{L}\p{N} ]/gu, " ")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => word[0]?.toUpperCase() ?? "")
    .join("");
}
