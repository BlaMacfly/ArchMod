/** Types miroir des structures Rust exposées par les `tauri::command`. */

export type Backend = "auto" | "protontricks" | "proton" | "wine";

export type BannerKind = "portrait" | "hero" | "header" | "logo";

export type LogLevel =
  | "info"
  | "command"
  | "stdout"
  | "stderr"
  | "warn"
  | "error"
  | "success";

/** Forme sérialisée de `TuxError` côté Rust. */
export interface TuxError {
  kind: string;
  message: string;
  hint: string | null;
}

export interface SteamGame {
  appId: number;
  name: string;
  steamRoot: string;
  libraryPath: string;
  installPath: string;
  sizeOnDisk: number;
  /** Timestamp Unix en secondes ; 0 si jamais lancé. */
  lastPlayed: number;
  prefixPath: string | null;
}

export interface TrainerEntry {
  path: string;
  label: string;
  addedAt: number;
  lastLaunchedAt: number | null;
  launchCount: number;
}

export interface GameView extends SteamGame {
  trainer: TrainerEntry | null;
  trainerMissing: boolean;
  running: boolean;
  trainerRunning: boolean;
}

export interface LibrarySnapshot {
  games: GameView[];
  steamRoots: string[];
  configError: TuxError | null;
}

export interface StatusUpdate {
  appId: number;
  running: boolean;
  pids: number[];
  matchedBy: string | null;
  trainerRunning: boolean;
}

export interface Settings {
  allowNetworkArtwork: boolean;
  warnIfGameNotRunning: boolean;
  backend: Backend;
}

export interface Dependencies {
  protontricks: string | null;
  protontricksFlatpak: boolean;
  wine: string | null;
  ready: boolean;
  installCommand: string;
}

export interface LaunchPlan {
  backend: Backend;
  program: string;
  args: string[];
  env: [string, string][];
  workingDir: string | null;
  note: string;
}

export interface LaunchOutcome {
  started: boolean;
  requiresConfirmation: boolean;
  message: string;
  plan: LaunchPlan | null;
  pid: number | null;
}

export interface RunningTrainer {
  appId: number;
  pid: number;
  backend: Backend;
  startedAt: number;
}

export interface LogLine {
  timestamp: number;
  level: LogLevel;
  appId: number | null;
  message: string;
}

export interface TrainerStateEvent {
  appId: number;
  running: boolean;
  exitCode: number | null;
  message: string;
}

export interface AppPaths {
  config: string;
  bannerCache: string;
}
