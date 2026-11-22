import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type RouteId =
  | "detective"
  | "optimizer"
  | "cleaner"
  | "startup"
  | "tweaker"
  | "settings";

export type ThemeMode = "dark" | "light";

export type GovernorProfile =
  | "Off"
  | "BalancedWork"
  | "GamingFocus"
  | "CodingCompile";

export interface DirtyPowerState {
  original_guid?: string;
  set_at_ms?: number;
  original_power_scheme_guid?: string;
  applied_power_scheme_guid?: string;
  changed_at?: string;
}

export interface AppConfig {
  theme: ThemeMode;
  active_profile: GovernorProfile;
  governor_enabled?: boolean;
  auto_start: boolean;
  auto_start_elevated?: boolean;
  minimize_to_tray: boolean;
  unload_webview_on_tray: boolean;
  ram_trim_threshold_pct: number;
  exclusions: string[];
  dirty_power_scheme: DirtyPowerState | null;
  last_route: string;
}

export interface ProcessInfo {
  pid: number;
  name: string;
  cpu_pct: number;
  ram_mb: number;
  priority: string;
  eco_qos: boolean;
  is_protected: boolean;
  is_excluded: boolean;
}

export type FixAction =
  | string
  | {
      kind?: string;
      type?: string;
      target?: string;
      [key: string]: unknown;
    };

export interface LagFinding {
  id: string;
  severity: "Critical" | "Warning" | "Info" | string;
  title: string;
  detail: string;
  deduction: number;
  fix_action: FixAction | null;
}

export interface SystemSnapshot {
  cpu_total_pct: number;
  cpu_cores: number[];
  ram_used_bytes: number;
  ram_total_bytes: number;
  ram_available_bytes: number;
  commit_pct: number;
  disk_read_bps: number;
  disk_write_bps: number;
  active_power_plan: string;
  power_plan_guid: string;
  is_on_ac_power: boolean | null;
  startup_enabled_count: number;
  cleanable_bytes_cached: number | null;
  top_processes: ProcessInfo[];
  health_score: number;
  findings: LagFinding[];
  sampled_at_ms: number;
}

export interface PrivilegeStatus {
  is_elevated: boolean;
  exe_path: string;
}

export interface GovernorState {
  enabled: boolean;
  profile: GovernorProfile;
  last_boosted_pid: number | null;
  eco_qos_pids: number[];
  last_trim_freed_mb: number;
  last_trim_at: number | null;
}

export interface RamTrimReport {
  trimmed_count: number;
  freed_mb: number;
}

export interface PowerPlanInfo {
  guid: string;
  name: string;
  is_active: boolean;
}

export interface CleanCategoryReport {
  id: string;
  name: string;
  paths: string[];
  bytes: number;
  file_count: number;
  requires_admin: boolean;
  opt_in_default_off: boolean;
  warning: string | null;
}

export interface CleanResult {
  freed_bytes: number;
  deleted_files: number;
  skipped_locked: number;
  errors: string[];
}

export interface StartupItem {
  id: string;
  name: string;
  command: string;
  scope: string;
  enabled: boolean;
  publisher: string | null;
  requires_admin?: boolean;
}

export interface TweakItem {
  id: string;
  category: string;
  title: string;
  description: string;
  risk: "Safe" | "Moderate" | "Advanced" | string;
  requires_admin: boolean;
  target_summary: string;
  current_state: unknown;
  target_state?: unknown;
  is_applied: boolean;
}

export interface BackupEntry {
  target?: string;
  hive?: string;
  path?: string;
  value_name?: string;
  service_name?: string;
  previous_value?: unknown;
  new_value?: unknown;
  [key: string]: unknown;
}

export interface BackupSnapshot {
  id: string;
  created_at: string;
  label: string;
  entries: BackupEntry[];
}

export interface AppxPackageItem {
  name: string;
  package_full_name: string;
  publisher: string;
  is_removable_bloat: boolean;
}

export interface ParsedAppError {
  kind:
    | "ElevationRequired"
    | "ProtectedTarget"
    | "Io"
    | "Win32"
    | "InvalidInput"
    | "Disconnected"
    | "Unknown";
  message: string;
  isElevationRequired: boolean;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function parseAppError(err: unknown): ParsedAppError {
  if (!isTauriRuntime()) {
    return {
      kind: "Disconnected",
      message: "Backend disconnected. Launch the desktop app to execute Windows system calls.",
      isElevationRequired: false,
    };
  }

  if (typeof err === "string") {
    const isElev =
      err.toLowerCase().includes("elevationrequired") ||
      err.toLowerCase().includes("administrator") ||
      err.toLowerCase().includes("elevation");
    return {
      kind: isElev ? "ElevationRequired" : "Unknown",
      message: err,
      isElevationRequired: isElev,
    };
  }

  if (err && typeof err === "object") {
    const obj = err as Record<string, unknown>;
    if ("kind" in obj && "message" in obj) {
      const kind = String(obj.kind) as ParsedAppError["kind"];
      return {
        kind,
        message: String(obj.message),
        isElevationRequired: kind === "ElevationRequired",
      };
    }
    if ("ElevationRequired" in obj) {
      return {
        kind: "ElevationRequired",
        message: String(obj.ElevationRequired),
        isElevationRequired: true,
      };
    }
    if ("ProtectedTarget" in obj) {
      return {
        kind: "ProtectedTarget",
        message: String(obj.ProtectedTarget),
        isElevationRequired: false,
      };
    }
    if ("Win32" in obj) {
      return {
        kind: "Win32",
        message: String(obj.Win32),
        isElevationRequired: false,
      };
    }
    if ("Io" in obj) {
      return {
        kind: "Io",
        message: String(obj.Io),
        isElevationRequired: false,
      };
    }
    if ("InvalidInput" in obj) {
      return {
        kind: "InvalidInput",
        message: String(obj.InvalidInput),
        isElevationRequired: false,
      };
    }
    if ("message" in obj) {
      const msg = String(obj.message);
      const isElev =
        msg.toLowerCase().includes("elevationrequired") ||
        msg.toLowerCase().includes("administrator");
      return {
        kind: isElev ? "ElevationRequired" : "Unknown",
        message: msg,
        isElevationRequired: isElev,
      };
    }
    return {
      kind: "Unknown",
      message: JSON.stringify(obj),
      isElevationRequired: false,
    };
  }

  return {
    kind: "Unknown",
    message: "An unexpected backend error occurred.",
    isElevationRequired: false,
  };
}

async function callTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) {
    throw {
      kind: "Disconnected",
      message: "Backend disconnected (browser preview mode).",
    };
  }
  return invoke<T>(cmd, args);
}

export const tauriApi = {
  getSystemSnapshot: () => callTauri<SystemSnapshot>("get_system_snapshot"),
  getPrivilegeStatus: () => callTauri<PrivilegeStatus>("get_privilege_status"),
  restartAsAdmin: (lastRoute: string) =>
    callTauri<void>("restart_as_admin", { lastRoute }),
  getConfig: () => callTauri<AppConfig>("get_config"),
  saveSettings: (config: AppConfig) =>
    callTauri<void>("save_settings", { config }),
  setAutostart: (enable: boolean, elevated: boolean) =>
    callTauri<void>("set_autostart", { enable, elevated }),
  setGovernorState: (enabled: boolean, profile: GovernorProfile) =>
    callTauri<GovernorState>("set_governor_state", { enabled, profile }),
  getGovernorState: () => callTauri<GovernorState>("get_governor_state"),
  applyProcessAction: (pid: number, action: string, name?: string) =>
    callTauri<void>("apply_process_action", { pid, action, name }),
  trimIdleRamNow: () => callTauri<RamTrimReport>("trim_idle_ram_now"),
  listPowerPlans: () => callTauri<PowerPlanInfo[]>("list_power_plans"),
  setPowerPlan: (guid: string) => callTauri<void>("set_power_plan", { guid }),
  scanCleaner: () => callTauri<CleanCategoryReport[]>("scan_cleaner"),
  runCleaner: (categoryIds: string[], moveToRecycleBin: boolean) =>
    callTauri<CleanResult>("run_cleaner", { categoryIds, moveToRecycleBin }),
  listStartupItems: () => callTauri<StartupItem[]>("list_startup_items"),
  toggleStartupItem: (id: string, enable: boolean) =>
    callTauri<void>("toggle_startup_item", { id, enable }),
  listTweaks: () => callTauri<TweakItem[]>("list_tweaks"),
  applyTweaks: (tweakIds: string[]) =>
    callTauri<BackupSnapshot>("apply_tweaks", { tweakIds }),
  revertTweaks: (tweakIds: string[]) =>
    callTauri<void>("revert_tweaks", { tweakIds }),
  listBackups: () => callTauri<BackupSnapshot[]>("list_backups"),
  restoreBackup: (snapshotId: string) =>
    callTauri<void>("restore_backup", { snapshotId }),
  exportBackupReg: (snapshotId: string) =>
    callTauri<string>("export_backup_reg", { snapshotId }),
  listAppxPackages: () => callTauri<AppxPackageItem[]>("list_appx_packages"),
  removeAppxPackage: (packageFullName: string) =>
    callTauri<void>("remove_appx_package", { packageFullName }),
  applyFindingFix: (findingId: string) =>
    callTauri<void>("apply_finding_fix", { findingId }),
};

export async function listenTelemetryTick(
  handler: (snapshot: SystemSnapshot) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<SystemSnapshot>("telemetry-tick", (event) => {
    handler(event.payload);
  });
}

export async function listenGovernorUpdate(
  handler: (state: GovernorState) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<GovernorState>("governor-update", (event) => {
    handler(event.payload);
  });
}
