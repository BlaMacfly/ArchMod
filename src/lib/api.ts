import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActivationReport,
  CandidateView,
  CheatTableImport,
  EnableReport,
  Filter,
  AddressRecipe,
  AppPaths,
  BannerKind,
  Dependencies,
  LaunchOutcome,
  LaunchPlan,
  LibrarySnapshot,
  LogLine,
  RunningTrainer,
  OptionStatus,
  PrefixComponent,
  PrefixReport,
  PointerPath,
  PointerScanOptions,
  PointerScanReport,
  Profile,
  ProfileEntry,
  ScanReport,
  Settings,
  StatusUpdate,
  TrainerEntry,
  TrainerStateEvent,
  TuxError,
  Value,
  ValueTypeRef,
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

  // Préparation du préfixe
  inspectPrefix: (appId: number) => call<PrefixReport>("inspect_prefix", { appId }),
  installComponent: (appId: number, component: PrefixComponent) =>
    call<boolean>("install_component", { appId, component }),

  // Profils de trainer
  profilesForGame: (appId: number) =>
    call<ProfileEntry[]>("profiles_for_game", { appId }),
  activateProfile: (appId: number, profile: Profile) =>
    call<ActivationReport>("activate_profile", { appId, profile }),
  trainerReport: (appId: number) =>
    call<ActivationReport | null>("trainer_report", { appId }),
  setOption: (appId: number, optionId: string, value?: Value) =>
    call<OptionStatus>("set_option", { appId, optionId, value: value ?? null }),
  clearOption: (appId: number, optionId: string) =>
    call<boolean>("clear_option", { appId, optionId }),
  refreshValues: (appId: number) =>
    call<ActivationReport>("refresh_values", { appId }),
  deactivateProfile: (appId: number) =>
    call<boolean>("deactivate_profile", { appId }),
  probeRecipe: (appId: number, recipe: AddressRecipe, valueType: ValueTypeRef) =>
    call<OptionStatus>("probe_recipe", { appId, recipe, valueType }),
  // Scripts d'auto-assembleur
  runScript: (appId: number, source: string) =>
    call<EnableReport>("run_script", { appId, source }),
  revertScript: (appId: number) => call<boolean>("revert_script", { appId }),

  // Recherche de pointeurs
  pointerScan: (appId: number, address: number, options?: PointerScanOptions) =>
    call<PointerScanReport>("pointer_scan", { appId, address, options: options ?? null }),
  pointerVerify: (appId: number, path: PointerPath) =>
    call<number>("pointer_verify", { appId, path }),

  // Recherche de valeurs
  scanStart: (appId: number, valueType: ValueTypeRef, filter: Filter) =>
    call<ScanReport>("scan_start", { appId, valueType, filter }),
  scanNext: (appId: number, filter: Filter) =>
    call<ScanReport>("scan_next", { appId, filter }),
  scanRefresh: (appId: number) =>
    call<CandidateView[]>("scan_refresh", { appId }),
  scanReset: (appId: number) => call<boolean>("scan_reset", { appId }),
  scanWrite: (appId: number, address: number, value: Value) =>
    call<void>("scan_write", { appId, address, value }),
  scanFreeze: (appId: number, address: number, value: Value | null) =>
    call<boolean>("scan_freeze", { appId, address, value }),

  importCheatTable: (appId: number, path: string) =>
    call<CheatTableImport>("import_cheat_table", { appId, path }),
  saveProfile: (profile: Profile) => call<string>("save_profile", { profile }),
  importProfile: (path: string) => call<Profile>("import_profile", { path }),
};

/** Nombre porté par une valeur typée, quel que soit son type. */
export function valueNumber(value: Value | null): number | null {
  return value ? value.value : null;
}

/** Fabrique une valeur du type attendu par une option. */
export function makeValue(kind: ValueTypeRef["kind"], number: number): Value | null {
  switch (kind) {
    case "byte":
    case "twoBytes":
    case "fourBytes":
    case "eightBytes":
    case "float":
    case "double":
      return { type: kind, value: number } as Value;
    default:
      return null;
  }
}

/** Écriture d'un chemin de pointeurs façon Cheat Engine. */
export function formatPointerPath(path: PointerPath): string {
  let text = `[${path.module}+${path.baseOffset.toString(16).toUpperCase()}]`;
  path.offsets.forEach((offset, index) => {
    const hex = offset.toString(16).toUpperCase();
    text = index + 1 === path.offsets.length ? `${text}+${hex}` : `[${text}+${hex}]`;
  });
  return text;
}

/** Adresse en hexadécimal, telle qu'on l'écrit dans un éditeur mémoire. */
export function formatAddress(address: number | null): string {
  return address === null ? "—" : `0x${address.toString(16).toUpperCase()}`;
}

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
