import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppPaths,
  BannerKind,
  Dependencies,
  LaunchOutcome,
  LaunchPlan,
  LibrarySnapshot,
  LogLine,
  RunningTrainer,
  Settings,
  StatusUpdate,
  TrainerEntry,
  TrainerStateEvent,
  TuxError,
} from "./types";

export const EVENT_LOG = "archmod://log";
export const EVENT_TRAINER_STATE = "archmod://trainer-state";

/** Vrai si l'objet a la forme d'une `TuxError` sérialisée par le backend. */
export function isTuxError(value: unknown): value is TuxError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value
  );
}

/** Normalise n'importe quel rejet en `TuxError` exploitable par l'interface. */
export function toTuxError(value: unknown): TuxError {
  if (isTuxError(value)) return value;
  if (value instanceof Error) {
    return { kind: "internal", message: value.message, hint: null };
  }
  return { kind: "internal", message: String(value), hint: null };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toTuxError(error);
  }
}

export const api = {
  scanLibrary: () => call<LibrarySnapshot>("scan_library"),
  refreshStatus: () => call<StatusUpdate[]>("refresh_status"),
  getBanner: (appId: number, kind: BannerKind) =>
    call<string | null>("get_banner", { appId, kind }),
  clearBannerCache: () => call<number>("clear_banner_cache"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) =>
    call<Settings>("update_settings", { settings }),
  setTrainer: (appId: number, path: string) =>
    call<TrainerEntry>("set_trainer", { appId, path }),
  removeTrainer: (appId: number) => call<void>("remove_trainer", { appId }),
  repairConfig: () => call<string | null>("repair_config"),
  previewCommand: (appId: number) => call<LaunchPlan>("preview_command", { appId }),
  launchTrainer: (appId: number, force: boolean) =>
    call<LaunchOutcome>("launch_trainer", { appId, force }),
  stopTrainer: (appId: number) => call<boolean>("stop_trainer", { appId }),
  runningTrainers: () => call<RunningTrainer[]>("running_trainers"),
  checkDependencies: () => call<Dependencies>("check_dependencies"),
  appPaths: () => call<AppPaths>("app_paths"),
};

export function onLog(handler: (line: LogLine) => void): Promise<UnlistenFn> {
  return listen<LogLine>(EVENT_LOG, (event) => handler(event.payload));
}

export function onTrainerState(
  handler: (state: TrainerStateEvent) => void,
): Promise<UnlistenFn> {
  return listen<TrainerStateEvent>(EVENT_TRAINER_STATE, (event) =>
    handler(event.payload),
  );
}

/** Reconstitue la ligne de commande, quotée comme côté Rust. */
export function formatCommand(plan: LaunchPlan): string {
  const quote = (value: string) => `'${value.replaceAll("'", `'\\''`)}'`;
  const env = plan.env.map(([key, value]) => `${key}=${quote(value)}`).join(" ");
  const args = plan.args.map(quote).join(" ");
  return `${env ? `${env} ` : ""}${quote(plan.program)} ${args}`;
}
