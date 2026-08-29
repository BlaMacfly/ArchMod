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
  /** Identifiant de build Steam : change à chaque mise à jour du jeu. */
  buildId: string | null;
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

// --- Profils de trainer ----------------------------------------------------

export type ValueTypeKind =
  | "byte" | "twoBytes" | "fourBytes" | "eightBytes"
  | "float" | "double" | "text" | "binary" | "script" | "other";

export interface ValueTypeRef {
  kind: ValueTypeKind;
  name?: string;
}

export type Value =
  | { type: "byte"; value: number }
  | { type: "twoBytes"; value: number }
  | { type: "fourBytes"; value: number }
  | { type: "eightBytes"; value: number }
  | { type: "float"; value: number }
  | { type: "double"; value: number };

export type Anchor =
  | { kind: "module"; module: string; offset: number }
  | {
      kind: "aob";
      module: string;
      pattern: string;
      offset: number;
      occurrence: number;
    };

export interface AddressRecipe {
  anchor: Anchor;
  /** Lit le pointeur rangé à l'adresse d'ancrage avant d'appliquer les décalages. */
  dereference: boolean;
  /** Appliqués dans l'ordre, avec déréférencement entre chaque étape. */
  offsets: number[];
}

export type Control =
  | { control: "toggle"; frozen: Value }
  | {
      control: "number";
      min: number | null;
      max: number | null;
      default: number | null;
      freeze: boolean;
    }
  | { control: "action"; value: Value }
  | { control: "display" };

export interface TrainerOption {
  id: string;
  category: string;
  name: string;
  description: string | null;
  valueType: ValueTypeRef;
  control: Control;
  address: AddressRecipe;
  hotkey: string | null;
}

export interface Profile {
  format: number;
  appId: number;
  game: string;
  buildId: string | null;
  author: string | null;
  notes: string | null;
  options: TrainerOption[];
}

export type BuildMatch =
  | { state: "exact" }
  | { state: "outdated"; detail: { profile: string; installed: string } }
  | { state: "unknown" };

export interface ProfileEntry {
  profile: Profile;
  buildMatch: BuildMatch;
}

export interface OptionStatus {
  id: string;
  address: number | null;
  value: Value | null;
  active: boolean;
  error: string | null;
}

export interface ActivationReport {
  appId: number;
  pid: number;
  options: OptionStatus[];
  resolved: number;
  failed: number;
}

export type PrefixComponent = "dotnet48" | "dotnet40" | "corefonts";

export interface Advice {
  message: string;
  install: PrefixComponent | null;
  blocking: boolean;
}

export interface PrefixReport {
  exists: boolean;
  path: string;
  proton: string | null;
  protonIsGe: boolean;
  wineMono: boolean;
  installed: string[];
  windowsVersion: string | null;
  advice: Advice[];
}
