use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use sysinfo::System;
use tauri::{AppHandle, Emitter, Manager};

use crate::core::{
    cleaner::{self, CleanCategoryReport, CleanResult},
    config::{load_config, save_config, AppConfig, GovernorProfile},
    detective::{self, SystemSnapshot},
    governor::{self, GovernorState, PowerPlanInfo, RamTrimReport},
    privilege,
    safety::{is_protected_process, AppError},
    startup::{self, StartupItem},
    tweaker::{self, AppxPackageItem, BackupEntry, BackupSnapshot, RegValueBackup, TweakItem},
};

pub struct AppState {
    pub sys: Mutex<System>,
    pub config: Mutex<AppConfig>,
    pub governor: Mutex<GovernorState>,
    pub cached_cleanable_bytes: Mutex<Option<u64>>,
    pub startup_enabled_count: AtomicUsize,
    pub window_visible: AtomicBool,
    pub cfg_dir: PathBuf,
    pub backup_dir: PathBuf,
}

impl AppState {
    pub fn new(
        cfg: AppConfig,
        cfg_dir: PathBuf,
        backup_dir: PathBuf,
        start_minimized: bool,
    ) -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_usage();

        let gov_active = cfg.governor_enabled && cfg.active_profile != GovernorProfile::Off;
        let governor = GovernorState {
            enabled: gov_active,
            profile: if gov_active {
                cfg.active_profile
            } else {
                GovernorProfile::Off
            },
            ..Default::default()
        };

        let initial_startup_count = startup::list_startup_items()
            .into_iter()
            .filter(|i| i.enabled)
            .count();

        Self {
            sys: Mutex::new(sys),
            config: Mutex::new(cfg),
            governor: Mutex::new(governor),
            cached_cleanable_bytes: Mutex::new(None),
            startup_enabled_count: AtomicUsize::new(initial_startup_count),
            window_visible: AtomicBool::new(!start_minimized),
            cfg_dir,
            backup_dir,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PrivilegeStatus {
    pub is_elevated: bool,
    pub exe_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GovernorStateDto {
    pub enabled: bool,
    pub profile: GovernorProfile,
    pub last_boosted_pid: Option<u32>,
    pub eco_qos_pids: Vec<u32>,
    pub last_trim_freed_mb: u64,
    pub last_trim_at: Option<u64>,
}

impl From<&GovernorState> for GovernorStateDto {
    fn from(s: &GovernorState) -> Self {
        Self {
            enabled: s.enabled,
            profile: s.profile,
            last_boosted_pid: s.last_boosted_pid,
            eco_qos_pids: s.eco_qos_pids.clone(),
            last_trim_freed_mb: s.last_trim_freed_mb,
            last_trim_at: s.last_trim_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TweakItemDto {
    pub id: String,
    pub category: tweaker::TweakCategory,
    pub title: String,
    pub description: String,
    pub risk: tweaker::RiskLevel,
    pub requires_admin: bool,
    pub target_summary: String,
    pub current_state: String,
    pub target_state: String,
    pub state: tweaker::TweakState,
    pub is_applied: bool,
}

impl From<TweakItem> for TweakItemDto {
    fn from(item: TweakItem) -> Self {
        Self {
            id: item.id,
            category: item.category,
            title: item.title,
            description: item.description,
            risk: item.risk,
            requires_admin: item.requires_admin,
            target_summary: item.target_summary,
            current_state: item.current_value_display,
            target_state: item.target_value_display,
            state: item.state,
            is_applied: item.is_applied,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupEntryDto {
    pub target: String,
    pub hive: Option<String>,
    pub path: Option<String>,
    pub value_name: Option<String>,
    pub service_name: Option<String>,
    pub previous_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupSnapshotDto {
    pub id: String,
    pub created_at: String,
    pub label: String,
    pub entries: Vec<BackupEntryDto>,
}

fn reg_backup_to_json(val: &RegValueBackup) -> serde_json::Value {
    match val {
        RegValueBackup::NotExisted => serde_json::json!({ "NotExisted": true }),
        RegValueBackup::Dword(v) => serde_json::json!({ "Dword": v }),
        RegValueBackup::Qword(v) => serde_json::Value::String(format!("QWORD 0x{v:016x}")),
        RegValueBackup::String(s) => serde_json::json!({ "Sz": s }),
    }
}

impl From<BackupSnapshot> for BackupSnapshotDto {
    fn from(snap: BackupSnapshot) -> Self {
        let entries = snap
            .entries
            .into_iter()
            .map(|entry| match entry {
                BackupEntry::Registry {
                    hive,
                    subkey,
                    value_name,
                    previous,
                } => BackupEntryDto {
                    target: format!(r"{hive}\{subkey}\{value_name}"),
                    hive: Some(hive),
                    path: Some(subkey),
                    value_name: Some(value_name),
                    service_name: None,
                    previous_value: reg_backup_to_json(&previous),
                    new_value: serde_json::Value::String("Applied tweak".to_string()),
                },
                BackupEntry::Service {
                    service_name,
                    previous_start_type,
                } => BackupEntryDto {
                    target: format!("Service: {service_name}"),
                    hive: None,
                    path: None,
                    value_name: None,
                    service_name: Some(service_name),
                    previous_value: serde_json::json!({ "ServiceStart": previous_start_type }),
                    new_value: serde_json::Value::String("Modified start type".to_string()),
                },
            })
            .collect();

        Self {
            id: snap.id,
            created_at: snap.created_at,
            label: snap.label,
            entries,
        }
    }
}

pub fn emit_governor_update(app: &AppHandle, dto: &GovernorStateDto) {
    let _ = app.emit("governor-update", dto);
}

/// Applies a governor enable/profile change, syncing `AppConfig`, `GovernorState`, power plan
/// (for `GamingFocus`), and emitting `governor-update`.
pub fn update_governor_state_sync(
    app: &AppHandle,
    enabled: bool,
    profile: GovernorProfile,
) -> Result<GovernorStateDto, AppError> {
    let state = app.state::<AppState>();
    let cfg_dir = state.cfg_dir.clone();

    let effective_enabled = enabled && profile != GovernorProfile::Off;
    let effective_profile = if effective_enabled {
        profile
    } else {
        GovernorProfile::Off
    };

    // Update config first (without holding lock across powercfg / file I/O).
    let mut cfg_snapshot = {
        let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg_guard.governor_enabled = effective_enabled;
        cfg_guard.active_profile = effective_profile;
        cfg_guard.clone()
    };
    save_config(&cfg_dir, &cfg_snapshot)?;

    if !effective_enabled {
        let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
        governor::restore_all_on_exit(&mut gov_guard, &cfg_dir);
        gov_guard.enabled = false;
        gov_guard.profile = GovernorProfile::Off;
    } else {
        // If leaving GamingFocus or entering non-GamingFocus, recover any dirty power scheme.
        if effective_profile != GovernorProfile::GamingFocus {
            let _ = governor::recover_dirty_power_scheme(&cfg_dir);
        } else {
            // Best-effort switch to Ultimate/High Performance for GamingFocus.
            let plans = governor::list_power_plans();
            if let Some(target) = plans
                .iter()
                .find(|p| p.guid.eq_ignore_ascii_case("e9a42b02-d5df-448d-aa00-03f14749eb61"))
                .or_else(|| {
                    plans.iter().find(|p| {
                        p.guid.eq_ignore_ascii_case("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")
                    })
                })
            {
                if !target.is_active {
                    let _ = governor::set_power_plan(&target.guid, &cfg_dir);
                }
            }
        }

        let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
        gov_guard.enabled = true;
        gov_guard.profile = effective_profile;
    }

    // Re-sync dirty_power_scheme from disk into in-memory config.
    if let Ok(reloaded) = load_config(&cfg_dir) {
        cfg_snapshot.dirty_power_scheme = reloaded.dirty_power_scheme;
        let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
        cfg_guard.dirty_power_scheme = cfg_snapshot.dirty_power_scheme;
    }

    let dto = {
        let gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
        GovernorStateDto::from(&*gov_guard)
    };
    emit_governor_update(app, &dto);
    Ok(dto)
}

pub fn sample_snapshot_from_state(state: &AppState) -> SystemSnapshot {
    let cfg = state
        .config
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let eco_qos_pids = state
        .governor
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .eco_qos_pids
        .clone();
    let cleanable_cached = *state
        .cached_cleanable_bytes
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let startup_count = state.startup_enabled_count.load(Ordering::Relaxed);

    let mut sys = state.sys.lock().unwrap_or_else(|e| e.into_inner());
    detective::sample_system(&mut sys, &cfg, cleanable_cached, startup_count, &eco_qos_pids)
}

#[tauri::command]
pub async fn get_system_snapshot(app: AppHandle) -> Result<SystemSnapshot, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        Ok(sample_snapshot_from_state(&state))
    })
    .await
    .map_err(|e| AppError::Win32(format!("Snapshot task failed: {e}")))?
}

#[tauri::command]
pub fn get_privilege_status() -> PrivilegeStatus {
    let exe_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    PrivilegeStatus {
        is_elevated: privilege::is_elevated(),
        exe_path,
    }
}

#[tauri::command]
pub fn restart_as_admin(last_route: String) -> Result<(), AppError> {
    privilege::restart_as_admin(&last_route)
}

#[tauri::command]
pub fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    let loaded = load_config(&state.cfg_dir)?;
    let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
    *cfg_guard = loaded.clone();
    Ok(loaded)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, config: AppConfig) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let cfg_dir = state.cfg_dir.clone();

    let mut next_cfg = config;
    {
        let existing = state.config.lock().unwrap_or_else(|e| e.into_inner());
        if next_cfg.dirty_power_scheme.is_none() {
            next_cfg.dirty_power_scheme = existing.dirty_power_scheme.clone();
        }
    }

    save_config(&cfg_dir, &next_cfg)?;
    {
        let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
        *cfg_guard = next_cfg.clone();
    }

    let gov_dto = {
        let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
        let active = next_cfg.governor_enabled && next_cfg.active_profile != GovernorProfile::Off;
        gov_guard.enabled = active;
        gov_guard.profile = if active {
            next_cfg.active_profile
        } else {
            GovernorProfile::Off
        };
        GovernorStateDto::from(&*gov_guard)
    };
    emit_governor_update(&app, &gov_dto);
    Ok(())
}

#[tauri::command]
pub async fn set_autostart(app: AppHandle, enable: bool, elevated: bool) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe_path = std::env::current_exe()?;
        governor::set_windows_autostart(enable, elevated, &exe_path)?;

        let state = app.state::<AppState>();
        let cfg_dir = state.cfg_dir.clone();
        let next_cfg = {
            let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
            cfg_guard.auto_start = enable;
            cfg_guard.auto_start_elevated = elevated;
            cfg_guard.clone()
        };
        save_config(&cfg_dir, &next_cfg)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Win32(format!("Autostart task failed: {e}")))?
}

#[tauri::command]
pub async fn set_governor_state(
    app: AppHandle,
    enabled: bool,
    profile: GovernorProfile,
) -> Result<GovernorStateDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || update_governor_state_sync(&app, enabled, profile))
        .await
        .map_err(|e| AppError::Win32(format!("Governor update task failed: {e}")))?
}

#[tauri::command]
pub fn get_governor_state(state: tauri::State<'_, AppState>) -> GovernorStateDto {
    let gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
    GovernorStateDto::from(&*gov_guard)
}

#[cfg(windows)]
fn trim_single_process_ws(pid: u32) -> Result<(), AppError> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    unsafe {
        let handle = OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| AppError::Win32(format!("OpenProcess({pid}) failed: {e}")))?;
        let _guard = HandleGuard(handle);
        EmptyWorkingSet(handle)
            .map_err(|e| AppError::Win32(format!("EmptyWorkingSet({pid}) failed: {e}")))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn trim_single_process_ws(_pid: u32) -> Result<(), AppError> {
    Err(AppError::Win32(
        "Working-set trimming is only supported on Windows".to_string(),
    ))
}

#[tauri::command]
pub async fn apply_process_action(
    app: AppHandle,
    pid: u32,
    action: String,
    name: Option<String>,
) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let proc_name = name.unwrap_or_else(|| {
            let sys = state.sys.lock().unwrap_or_else(|e| e.into_inner());
            sys.process(sysinfo::Pid::from_u32(pid))
                .map(|p| p.name().to_string_lossy().into_owned())
                .unwrap_or_default()
        });

        let act = action.trim().to_ascii_lowercase();
        if act == "exclude" {
            let trimmed = proc_name.trim();
            if trimmed.is_empty() {
                return Err(AppError::InvalidInput(
                    "Process name is required to add an exclusion".to_string(),
                ));
            }
            let cfg_dir = state.cfg_dir.clone();
            let next_cfg = {
                let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
                if !cfg_guard
                    .exclusions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(trimmed))
                {
                    cfg_guard.exclusions.push(trimmed.to_string());
                }
                cfg_guard.clone()
            };
            save_config(&cfg_dir, &next_cfg)?;
            return Ok(());
        }

        if is_protected_process(&proc_name, pid, std::process::id()) {
            return Err(AppError::ProtectedTarget(format!(
                "Refusing to modify protected process '{proc_name}' (PID {pid})"
            )));
        }

        match act.as_str() {
            "eco_qos" => {
                governor::set_process_eco_qos(pid, true)?;
                let dto = {
                    let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                    if !gov_guard.eco_qos_pids.contains(&pid) {
                        gov_guard.eco_qos_pids.push(pid);
                    }
                    GovernorStateDto::from(&*gov_guard)
                };
                emit_governor_update(&app, &dto);
                Ok(())
            }
            "boost" => {
                let _ = governor::set_process_eco_qos(pid, false);
                if let Ok(orig) = governor::get_process_priority_class(pid) {
                    let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                    if !gov_guard.original_priorities.iter().any(|(p, _)| *p == pid) {
                        gov_guard.original_priorities.push((pid, orig));
                    }
                }
                governor::set_process_priority_boost(pid, false)?;
                let dto = {
                    let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                    gov_guard.eco_qos_pids.retain(|&p| p != pid);
                    gov_guard.last_boosted_pid = Some(pid);
                    GovernorStateDto::from(&*gov_guard)
                };
                emit_governor_update(&app, &dto);
                Ok(())
            }
            "trim" => trim_single_process_ws(pid),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported process action '{other}'"
            ))),
        }
    })
    .await
    .map_err(|e| AppError::Win32(format!("Process action task failed: {e}")))?
}

#[tauri::command]
pub async fn trim_idle_ram_now(app: AppHandle) -> Result<RamTrimReport, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let exclusions = state
            .config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exclusions
            .clone();
        let fg_pid = governor::get_foreground_pid().unwrap_or(0);
        let report = governor::trim_idle_ram(&exclusions, 150, fg_pid)?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let dto = {
            let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
            gov_guard.last_trim_freed_mb = report.freed_mb;
            gov_guard.last_trim_at_ms = Some(now_ms);
            GovernorStateDto::from(&*gov_guard)
        };
        emit_governor_update(&app, &dto);
        Ok(report)
    })
    .await
    .map_err(|e| AppError::Win32(format!("RAM trim task failed: {e}")))?
}

#[tauri::command]
pub async fn list_power_plans() -> Result<Vec<PowerPlanInfo>, AppError> {
    tauri::async_runtime::spawn_blocking(governor::list_power_plans)
        .await
        .map_err(|e| AppError::Win32(format!("Power plan enumeration failed: {e}")))
}

#[tauri::command]
pub async fn set_power_plan(app: AppHandle, guid: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let cfg_dir = state.cfg_dir.clone();
        governor::set_power_plan(&guid, &cfg_dir)?;
        if let Ok(reloaded) = load_config(&cfg_dir) {
            let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
            cfg_guard.dirty_power_scheme = reloaded.dirty_power_scheme;
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Win32(format!("Set power plan task failed: {e}")))?
}

#[tauri::command]
pub async fn scan_cleaner(app: AppHandle) -> Result<Vec<CleanCategoryReport>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let reports = cleaner::scan_categories();
        let total_bytes: u64 = reports.iter().map(|r| r.bytes).sum();
        let state = app.state::<AppState>();
        *state
            .cached_cleanable_bytes
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(total_bytes);
        reports
    })
    .await
    .map_err(|e| AppError::Win32(format!("Cleaner scan task failed: {e}")))
}

#[tauri::command]
pub async fn run_cleaner(
    app: AppHandle,
    category_ids: Vec<String>,
    move_to_recycle_bin: bool,
) -> Result<CleanResult, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let res = cleaner::clean_categories(&category_ids, move_to_recycle_bin)?;
        let state = app.state::<AppState>();
        let mut cached = state
            .cached_cleanable_bytes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(prev) = *cached {
            *cached = Some(prev.saturating_sub(res.freed_bytes));
        }
        Ok(res)
    })
    .await
    .map_err(|e| AppError::Win32(format!("Cleaner execution task failed: {e}")))?
}

#[tauri::command]
pub async fn list_startup_items(app: AppHandle) -> Result<Vec<StartupItem>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let items = startup::list_startup_items();
        let enabled_count = items.iter().filter(|i| i.enabled).count();
        let state = app.state::<AppState>();
        state
            .startup_enabled_count
            .store(enabled_count, Ordering::Relaxed);
        items
    })
    .await
    .map_err(|e| AppError::Win32(format!("Startup list task failed: {e}")))
}

#[tauri::command]
pub async fn toggle_startup_item(app: AppHandle, id: String, enable: bool) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        startup::toggle_startup_item(&id, enable)?;
        let items = startup::list_startup_items();
        let enabled_count = items.iter().filter(|i| i.enabled).count();
        let state = app.state::<AppState>();
        state
            .startup_enabled_count
            .store(enabled_count, Ordering::Relaxed);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Win32(format!("Startup toggle task failed: {e}")))?
}

#[tauri::command]
pub async fn list_tweaks() -> Result<Vec<TweakItemDto>, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        tweaker::list_tweaks()
            .into_iter()
            .map(TweakItemDto::from)
            .collect()
    })
    .await
    .map_err(|e| AppError::Win32(format!("List tweaks task failed: {e}")))
}

#[tauri::command]
pub async fn apply_tweaks(
    app: AppHandle,
    tweak_ids: Vec<String>,
) -> Result<BackupSnapshotDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_dir = app.state::<AppState>().backup_dir.clone();
        let snap = tweaker::apply_tweaks(&tweak_ids, &backup_dir)?;
        Ok(BackupSnapshotDto::from(snap))
    })
    .await
    .map_err(|e| AppError::Win32(format!("Apply tweaks task failed: {e}")))?
}

#[tauri::command]
pub async fn revert_tweaks(app: AppHandle, tweak_ids: Vec<String>) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_dir = app.state::<AppState>().backup_dir.clone();
        tweaker::revert_tweaks(&tweak_ids, &backup_dir)
    })
    .await
    .map_err(|e| AppError::Win32(format!("Revert tweaks task failed: {e}")))?
}

#[tauri::command]
pub async fn list_backups(app: AppHandle) -> Result<Vec<BackupSnapshotDto>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_dir = app.state::<AppState>().backup_dir.clone();
        tweaker::list_backups(&backup_dir)
            .into_iter()
            .map(BackupSnapshotDto::from)
            .collect()
    })
    .await
    .map_err(|e| AppError::Win32(format!("List backups task failed: {e}")))
}

#[tauri::command]
pub async fn restore_backup(app: AppHandle, snapshot_id: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_dir = app.state::<AppState>().backup_dir.clone();
        tweaker::restore_snapshot(&snapshot_id, &backup_dir)
    })
    .await
    .map_err(|e| AppError::Win32(format!("Restore backup task failed: {e}")))?
}

#[tauri::command]
pub async fn export_backup_reg(app: AppHandle, snapshot_id: String) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let backup_dir = app.state::<AppState>().backup_dir.clone();
        tweaker::export_snapshot_reg(&snapshot_id, &backup_dir)
    })
    .await
    .map_err(|e| AppError::Win32(format!("Export .reg task failed: {e}")))?
}

#[tauri::command]
pub async fn list_appx_packages() -> Result<Vec<AppxPackageItem>, AppError> {
    tauri::async_runtime::spawn_blocking(tweaker::list_removable_appx)
        .await
        .map_err(|e| AppError::Win32(format!("List AppX task failed: {e}")))?
}

#[tauri::command]
pub async fn remove_appx_package(package_full_name: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        tweaker::remove_appx_current_user(&package_full_name)
    })
    .await
    .map_err(|e| AppError::Win32(format!("Remove AppX task failed: {e}")))?
}

#[tauri::command]
pub async fn apply_finding_fix(app: AppHandle, finding_id: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        match finding_id.as_str() {
            "ram_critical" | "ram_high" | "commit_critical" => {
                let exclusions = state
                    .config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .exclusions
                    .clone();
                let fg_pid = governor::get_foreground_pid().unwrap_or(0);
                let report = governor::trim_idle_ram(&exclusions, 150, fg_pid)?;
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let dto = {
                    let mut gov_guard = state.governor.lock().unwrap_or_else(|e| e.into_inner());
                    gov_guard.last_trim_freed_mb = report.freed_mb;
                    gov_guard.last_trim_at_ms = Some(now_ms);
                    GovernorStateDto::from(&*gov_guard)
                };
                emit_governor_update(&app, &dto);
                Ok(())
            }
            "power_saver_on_ac" => {
                let plans = governor::list_power_plans();
                let target_guid = plans
                    .iter()
                    .find(|p| p.guid.eq_ignore_ascii_case("381b4222-f694-41f0-9685-ff5bb260df2e"))
                    .or_else(|| {
                        plans.iter().find(|p| {
                            p.guid.eq_ignore_ascii_case("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")
                        })
                    })
                    .map(|p| p.guid.clone())
                    .unwrap_or_else(|| "381b4222-f694-41f0-9685-ff5bb260df2e".to_string());
                let cfg_dir = state.cfg_dir.clone();
                governor::set_power_plan(&target_guid, &cfg_dir)?;
                if let Ok(reloaded) = load_config(&cfg_dir) {
                    let mut cfg_guard = state.config.lock().unwrap_or_else(|e| e.into_inner());
                    cfg_guard.dirty_power_scheme = reloaded.dirty_power_scheme;
                }
                Ok(())
            }
            "cpu_critical" | "cpu_high" | "unthrottled_bg_cpu" => {
                let current_profile = state
                    .config
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .active_profile;
                let next_profile = if current_profile == GovernorProfile::Off {
                    GovernorProfile::BalancedWork
                } else {
                    current_profile
                };
                update_governor_state_sync(&app, true, next_profile)?;
                Ok(())
            }
            "cleanable_junk" => {
                let reports = cleaner::scan_categories();
                let safe_ids: Vec<String> = reports
                    .into_iter()
                    .filter(|c| !c.opt_in_default_off && !c.requires_admin && c.bytes > 0)
                    .map(|c| c.id)
                    .collect();
                if !safe_ids.is_empty() {
                    let res = cleaner::clean_categories(&safe_ids, false)?;
                    let mut cached = state
                        .cached_cleanable_bytes
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Some(prev) = *cached {
                        *cached = Some(prev.saturating_sub(res.freed_bytes));
                    }
                }
                Ok(())
            }
            "startup_bloat" => Err(AppError::InvalidInput(
                "Review individual entries in Startup & Tasks before disabling startup programs."
                    .to_string(),
            )),
            other => Err(AppError::InvalidInput(format!(
                "Unknown finding ID '{other}'"
            ))),
        }
    })
    .await
    .map_err(|e| AppError::Win32(format!("Finding fix task failed: {e}")))?
}
