use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

use crate::core::config::{load_config, save_config, AppConfig, DirtyPowerState, GovernorProfile};
use crate::core::detective::{is_valid_guid, parse_powercfg_active_scheme};
use crate::core::safety::{is_protected_process, AppError};

const AUTO_TRIM_COOLDOWN_MS: u64 = 60_000;
const AUTO_TRIM_MIN_WS_MB: u64 = 150;
const TASK_NAME: &str = "rtv-highperf";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GovernorState {
    pub enabled: bool,
    pub profile: GovernorProfile,
    pub last_boosted_pid: Option<u32>,
    pub eco_qos_pids: Vec<u32>,
    pub last_trim_freed_mb: u64,
    pub last_trim_at_ms: Option<u64>,
    pub original_priorities: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernorStatusDto {
    pub enabled: bool,
    pub profile: GovernorProfile,
    pub last_boosted_pid: Option<u32>,
    pub eco_qos_count: usize,
    pub last_trim_freed_mb: u64,
    pub last_trim_at_ms: Option<u64>,
}

impl From<&GovernorState> for GovernorStatusDto {
    fn from(s: &GovernorState) -> Self {
        Self {
            enabled: s.enabled,
            profile: s.profile,
            last_boosted_pid: s.last_boosted_pid,
            eco_qos_count: s.eco_qos_pids.len(),
            last_trim_freed_mb: s.last_trim_freed_mb,
            last_trim_at_ms: s.last_trim_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerPlanInfo {
    pub guid: String,
    pub name: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RamTrimReport {
    pub trimmed_count: usize,
    pub freed_mb: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessCandidate {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub ram_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GovernorActionPlan {
    pub boost_foreground_pid: Option<u32>,
    pub restore_pids: Vec<u32>,
    pub eco_qos_enable_pids: Vec<u32>,
    pub eco_qos_disable_pids: Vec<u32>,
    pub trim_candidate_pids: Vec<u32>,
}

pub trait PowerSchemeBackend {
    fn get_active_scheme(&self) -> Result<String, AppError>;
    fn set_active_scheme(&self, guid: &str) -> Result<(), AppError>;
}

pub struct WindowsPowerBackend;

impl PowerSchemeBackend for WindowsPowerBackend {
    fn get_active_scheme(&self) -> Result<String, AppError> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;

            let out = Command::new("powercfg")
                .arg("/getactivescheme")
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;

            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                return Err(AppError::Win32(format!(
                    "powercfg /getactivescheme failed: {stderr}"
                )));
            }

            let stdout = String::from_utf8_lossy(&out.stdout);
            let (guid, _) = parse_powercfg_active_scheme(&stdout).ok_or_else(|| {
                AppError::Win32("Failed to parse active power scheme GUID".to_string())
            })?;
            Ok(guid)
        }
        #[cfg(not(windows))]
        {
            Err(AppError::Win32(
                "Power schemes are only supported on Windows".to_string(),
            ))
        }
    }

    fn set_active_scheme(&self, guid: &str) -> Result<(), AppError> {
        if !is_valid_guid(guid) {
            return Err(AppError::InvalidInput(format!(
                "Invalid power plan GUID: {guid}"
            )));
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;

            let out = Command::new("powercfg")
                .args(["/setactive", guid])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;

            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                if detail.to_ascii_lowercase().contains("access")
                    || detail.to_ascii_lowercase().contains("denied")
                {
                    return Err(AppError::ElevationRequired(format!(
                        "Administrator privileges required to switch power scheme: {detail}"
                    )));
                }
                return Err(AppError::Win32(format!(
                    "powercfg /setactive {guid} failed: {detail}"
                )));
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(AppError::Win32(
                "Power schemes are only supported on Windows".to_string(),
            ))
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_proc_name(name: &str) -> String {
    name.trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

pub fn is_process_excluded(name: &str, exclusions: &[String]) -> bool {
    let base = normalize_proc_name(name);
    if base.is_empty() {
        return false;
    }
    exclusions
        .iter()
        .any(|ex| normalize_proc_name(ex) == base)
}

fn is_coding_toolchain_process(name: &str) -> bool {
    matches!(
        normalize_proc_name(name).as_str(),
        "cargo.exe"
            | "rustc.exe"
            | "rust-analyzer.exe"
            | "cl.exe"
            | "link.exe"
            | "msbuild.exe"
            | "ninja.exe"
            | "cmake.exe"
            | "node.exe"
            | "tsc.exe"
            | "esbuild.exe"
            | "vite.exe"
            | "clang.exe"
            | "gcc.exe"
            | "go.exe"
    )
}

/// Checks `config.json` in `cfg_dir` for a dangling `dirty_power_scheme` from an unclean shutdown,
/// restores the original power scheme, and clears the flag.
pub fn recover_dirty_power_scheme(cfg_dir: &Path) -> Result<Option<String>, AppError> {
    recover_dirty_power_scheme_with(cfg_dir, &WindowsPowerBackend)
}

pub fn recover_dirty_power_scheme_with<B: PowerSchemeBackend>(
    cfg_dir: &Path,
    backend: &B,
) -> Result<Option<String>, AppError> {
    let mut cfg = load_config(cfg_dir)?;
    let Some(dirty) = cfg.dirty_power_scheme.clone() else {
        return Ok(None);
    };

    backend.set_active_scheme(&dirty.original_guid)?;
    cfg.dirty_power_scheme = None;
    save_config(cfg_dir, &cfg)?;
    Ok(Some(dirty.original_guid))
}

/// Parses `powercfg /list` output into structured `PowerPlanInfo` entries.
pub fn parse_powercfg_list(output: &str) -> Vec<PowerPlanInfo> {
    let mut plans = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((guid, name)) = parse_powercfg_active_scheme(trimmed) else {
            continue;
        };
        let is_active = trimmed.ends_with('*');
        plans.push(PowerPlanInfo {
            guid,
            name,
            is_active,
        });
    }
    plans
}

/// Lists installed Windows power plans using `powercfg /list`.
pub fn list_power_plans() -> Vec<PowerPlanInfo> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let Ok(out) = Command::new("powercfg")
            .arg("/list")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return Vec::new();
        };

        if !out.status.success() {
            return Vec::new();
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        parse_powercfg_list(&stdout)
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Switches the active Windows power plan, persisting `dirty_power_scheme` first if not already recorded.
pub fn set_power_plan(guid: &str, cfg_dir: &Path) -> Result<(), AppError> {
    set_power_plan_with(guid, cfg_dir, &WindowsPowerBackend)
}

pub fn set_power_plan_with<B: PowerSchemeBackend>(
    guid: &str,
    cfg_dir: &Path,
    backend: &B,
) -> Result<(), AppError> {
    if !is_valid_guid(guid) {
        return Err(AppError::InvalidInput(format!(
            "Invalid power plan GUID: {guid}"
        )));
    }

    let mut cfg = load_config(cfg_dir)?;
    if cfg.dirty_power_scheme.is_none() {
        let current = backend.get_active_scheme()?;
        if !current.eq_ignore_ascii_case(guid) {
            cfg.dirty_power_scheme = Some(DirtyPowerState {
                original_guid: current,
                set_at_ms: now_ms(),
            });
            save_config(cfg_dir, &cfg)?;
        }
    }

    backend.set_active_scheme(guid)
}

#[cfg(windows)]
struct HandleGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn query_process_image_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let _guard = HandleGuard(handle);
        let mut buf = vec![0u16; 520];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(windows)]
fn ensure_unprotected_pid(pid: u32) -> Result<(), AppError> {
    let self_pid = std::process::id();
    if is_protected_process("", pid, self_pid) {
        return Err(AppError::ProtectedTarget(format!(
            "Process PID {pid} is protected"
        )));
    }

    if let Some(image_name) = query_process_image_name(pid) {
        if is_protected_process(&image_name, pid, self_pid) {
            return Err(AppError::ProtectedTarget(format!(
                "Protected process {image_name} (PID {pid}) cannot be modified"
            )));
        }
    }

    Ok(())
}

#[cfg(windows)]
fn map_win32_process_error(err: windows::core::Error, action: &str, pid: u32) -> AppError {
    const ERROR_ACCESS_DENIED_HRESULT: i32 = 0x80070005u32 as i32;
    if err.code().0 == ERROR_ACCESS_DENIED_HRESULT {
        AppError::ElevationRequired(format!(
            "Access denied for {action} on PID {pid}: {err}"
        ))
    } else {
        AppError::Win32(format!("{action} failed for PID {pid}: {err}"))
    }
}

/// Enables or disables Windows EcoQoS (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED`) on `pid`.
pub fn set_process_eco_qos(pid: u32, enable: bool) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use windows::Win32::System::Threading::{
            OpenProcess, ProcessPowerThrottling, SetProcessInformation,
            PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        };

        ensure_unprotected_pid(pid)?;

        unsafe {
            let handle = OpenProcess(
                PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
            .map_err(|e| map_win32_process_error(e, "OpenProcess", pid))?;
            let _guard = HandleGuard(handle);

            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: if enable {
                    PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                } else {
                    0
                },
            };

            SetProcessInformation(
                handle,
                ProcessPowerThrottling,
                (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
            .map_err(|e| map_win32_process_error(e, "SetProcessInformation", pid))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, enable);
        Err(AppError::Win32(
            "EcoQoS is only supported on Windows".to_string(),
        ))
    }
}

/// Queries the current Win32 priority class for `pid`.
pub fn get_process_priority_class(pid: u32) -> Result<u32, AppError> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            GetPriorityClass, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                .map_err(|e| map_win32_process_error(e, "OpenProcess", pid))?;
            let _guard = HandleGuard(handle);
            let class = GetPriorityClass(handle);
            if class == 0 {
                return Err(AppError::Win32(format!(
                    "GetPriorityClass returned 0 for PID {pid}"
                )));
            }
            Ok(class)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err(AppError::Win32(
            "Process priority is only supported on Windows".to_string(),
        ))
    }
}

/// Boosts `pid` to `HIGH_PRIORITY_CLASS` (`high == true`) or `ABOVE_NORMAL_PRIORITY_CLASS` (`high == false`).
/// Never sets `REALTIME_PRIORITY_CLASS` and refuses protected processes.
pub fn set_process_priority_boost(pid: u32, high: bool) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            OpenProcess, SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        };

        ensure_unprotected_pid(pid)?;

        unsafe {
            let handle = OpenProcess(
                PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
            .map_err(|e| map_win32_process_error(e, "OpenProcess", pid))?;
            let _guard = HandleGuard(handle);

            let class = if high {
                HIGH_PRIORITY_CLASS
            } else {
                ABOVE_NORMAL_PRIORITY_CLASS
            };

            SetPriorityClass(handle, class)
                .map_err(|e| map_win32_process_error(e, "SetPriorityClass", pid))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, high);
        Err(AppError::Win32(
            "Process priority boost is only supported on Windows".to_string(),
        ))
    }
}

/// Restores `pid` EcoQoS (off) and its original Win32 priority class.
pub fn restore_process_state(pid: u32, orig_priority: u32) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            OpenProcess, SetPriorityClass, PROCESS_CREATION_FLAGS,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        };

        ensure_unprotected_pid(pid)?;

        let eco_res = set_process_eco_qos(pid, false);

        if orig_priority != 0 {
            unsafe {
                let handle = OpenProcess(
                    PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    pid,
                )
                .map_err(|e| map_win32_process_error(e, "OpenProcess", pid))?;
                let _guard = HandleGuard(handle);

                SetPriorityClass(handle, PROCESS_CREATION_FLAGS(orig_priority))
                    .map_err(|e| map_win32_process_error(e, "SetPriorityClass", pid))?;
            }
        }

        eco_res
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, orig_priority);
        Err(AppError::Win32(
            "Process state restore is only supported on Windows".to_string(),
        ))
    }
}

/// Trims idle background process working sets via `EmptyWorkingSet`, skipping protected processes,
/// `foreground_pid`, and `exclusions`, and measuring actual working-set MB freed.
pub fn trim_idle_ram(
    exclusions: &[String],
    min_ws_mb: u64,
    foreground_pid: u32,
) -> Result<RamTrimReport, AppError> {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use windows::Win32::System::ProcessStatus::{
            EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
        };

        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let self_pid = std::process::id();
        let min_bytes = min_ws_mb.saturating_mul(1024 * 1024);

        let mut trimmed_count: usize = 0;
        let mut total_freed_bytes: u64 = 0;

        for (pid_val, proc_entry) in sys.processes() {
            let pid = pid_val.as_u32();
            if foreground_pid != 0 && pid == foreground_pid {
                continue;
            }
            if proc_entry.memory() < min_bytes {
                continue;
            }

            let name = proc_entry.name().to_string_lossy();
            if is_protected_process(&name, pid, self_pid) || is_process_excluded(&name, exclusions)
            {
                continue;
            }

            unsafe {
                let Ok(handle) = OpenProcess(
                    PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    pid,
                ) else {
                    continue;
                };
                let _guard = HandleGuard(handle);

                let mut before = PROCESS_MEMORY_COUNTERS {
                    cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                    ..Default::default()
                };
                if GetProcessMemoryInfo(handle, &mut before, before.cb).is_err() {
                    continue;
                }

                if EmptyWorkingSet(handle).is_err() {
                    continue;
                }

                let mut after = PROCESS_MEMORY_COUNTERS {
                    cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                    ..Default::default()
                };
                if GetProcessMemoryInfo(handle, &mut after, after.cb).is_ok() {
                    let delta = (before.WorkingSetSize as u64)
                        .saturating_sub(after.WorkingSetSize as u64);
                    total_freed_bytes = total_freed_bytes.saturating_add(delta);
                }
                trimmed_count += 1;
            }
        }

        Ok(RamTrimReport {
            trimmed_count,
            freed_mb: total_freed_bytes / (1024 * 1024),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (exclusions, min_ws_mb, foreground_pid);
        Ok(RamTrimReport {
            trimmed_count: 0,
            freed_mb: 0,
        })
    }
}

/// Creates or removes the Windows Task Scheduler logon task `rtv-highperf` passing `--minimized`.
pub fn set_windows_autostart(enable: bool, elevated: bool, exe_path: &Path) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        if enable {
            let exe_str = exe_path.to_str().ok_or_else(|| {
                AppError::InvalidInput("Executable path is not valid UTF-8".to_string())
            })?;
            let tr_value = format!("\"{exe_str}\" --minimized");
            let rl = if elevated { "HIGHEST" } else { "LIMITED" };

            let out = Command::new("schtasks")
                .args([
                    "/Create",
                    "/TN",
                    TASK_NAME,
                    "/TR",
                    &tr_value,
                    "/SC",
                    "ONLOGON",
                    "/RL",
                    rl,
                    "/F",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;

            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let detail = if stderr.is_empty() { stdout } else { stderr };
                if detail.to_ascii_lowercase().contains("access")
                    || detail.to_ascii_lowercase().contains("denied")
                {
                    return Err(AppError::ElevationRequired(format!(
                        "Elevated privileges required for HighestAvailable startup task: {detail}"
                    )));
                }
                return Err(AppError::Win32(format!(
                    "schtasks /Create failed: {detail}"
                )));
            }
            Ok(())
        } else {
            let out = Command::new("schtasks")
                .args(["/Delete", "/TN", TASK_NAME, "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;

            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let lower = stderr.to_ascii_lowercase();
                if lower.contains("cannot find") || lower.contains("does not exist") {
                    return Ok(());
                }
                if lower.contains("access") || lower.contains("denied") {
                    return Err(AppError::ElevationRequired(format!(
                        "Elevated privileges required to delete startup task: {stderr}"
                    )));
                }
                return Err(AppError::Win32(format!(
                    "schtasks /Delete failed: {stderr}"
                )));
            }
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (enable, elevated, exe_path);
        Err(AppError::Win32(
            "Task Scheduler autostart is only supported on Windows".to_string(),
        ))
    }
}

#[cfg(windows)]
pub fn get_foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

#[cfg(not(windows))]
pub fn get_foreground_pid() -> Option<u32> {
    None
}

#[cfg(windows)]
fn current_ram_used_pct() -> f32 {
    use std::mem::size_of;
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut mem = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        if GlobalMemoryStatusEx(&mut mem).is_ok() && mem.ullTotalPhys > 0 {
            let used = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys);
            (used as f64 / mem.ullTotalPhys as f64 * 100.0) as f32
        } else {
            0.0
        }
    }
}

#[cfg(not(windows))]
fn current_ram_used_pct() -> f32 {
    0.0
}

/// Pure decision planner for the Auto-Governor loop.
/// Guarantees protected processes, `foreground_pid`, and `exclusions` are never EcoQoS-throttled or trimmed.
#[allow(clippy::too_many_arguments)]
pub fn plan_governor_actions(
    profile: GovernorProfile,
    foreground_pid: Option<u32>,
    candidates: &[ProcessCandidate],
    exclusions: &[String],
    last_boosted_pid: Option<u32>,
    current_eco_qos_pids: &[u32],
    ram_used_pct: f32,
    ram_trim_threshold_pct: u8,
    should_trim_now: bool,
    self_pid: u32,
) -> GovernorActionPlan {
    let mut plan = GovernorActionPlan::default();

    if profile == GovernorProfile::Off {
        if let Some(prev) = last_boosted_pid {
            plan.restore_pids.push(prev);
        }
        plan.eco_qos_disable_pids
            .extend_from_slice(current_eco_qos_pids);
        return plan;
    }

    let bg_cpu_threshold = match profile {
        GovernorProfile::BalancedWork => 5.0,
        GovernorProfile::GamingFocus => 2.0,
        GovernorProfile::CodingCompile => 8.0,
        GovernorProfile::Off => unreachable!(),
    };

    if let Some(fg_pid) = foreground_pid {
        let fg_candidate = candidates.iter().find(|c| c.pid == fg_pid);
        let fg_is_protected = fg_candidate
            .map(|c| is_protected_process(&c.name, c.pid, self_pid))
            .unwrap_or_else(|| is_protected_process("", fg_pid, self_pid));

        if !fg_is_protected {
            plan.boost_foreground_pid = Some(fg_pid);
        }

        if let Some(prev) = last_boosted_pid {
            if prev != fg_pid {
                plan.restore_pids.push(prev);
            }
        }
    } else if let Some(prev) = last_boosted_pid {
        plan.restore_pids.push(prev);
    }

    for c in candidates {
        let protected = is_protected_process(&c.name, c.pid, self_pid);
        let excluded = is_process_excluded(&c.name, exclusions);
        let is_fg = foreground_pid == Some(c.pid);
        let coding_exempt =
            profile == GovernorProfile::CodingCompile && is_coding_toolchain_process(&c.name);

        if protected || excluded || is_fg || coding_exempt {
            if current_eco_qos_pids.contains(&c.pid) && !protected {
                plan.eco_qos_disable_pids.push(c.pid);
            }
            continue;
        }

        if c.cpu_pct >= bg_cpu_threshold {
            if !current_eco_qos_pids.contains(&c.pid) {
                plan.eco_qos_enable_pids.push(c.pid);
            }
        } else if current_eco_qos_pids.contains(&c.pid) && c.cpu_pct < 1.0 {
            plan.eco_qos_disable_pids.push(c.pid);
        }

        if should_trim_now
            && ram_used_pct >= f32::from(ram_trim_threshold_pct)
            && c.ram_mb >= AUTO_TRIM_MIN_WS_MB
        {
            plan.trim_candidate_pids.push(c.pid);
        }
    }

    plan
}

/// Runs one Auto-Governor control tick using a persistent `sysinfo::System` for accurate CPU deltas.
pub fn tick_governor_with_sys(
    state: &mut GovernorState,
    cfg: &AppConfig,
    cfg_dir: &Path,
    sys: &mut System,
) {
    if !cfg.governor_enabled || cfg.active_profile == GovernorProfile::Off {
        if state.enabled || state.last_boosted_pid.is_some() || !state.eco_qos_pids.is_empty() {
            restore_all_on_exit(state, cfg_dir);
        }
        state.enabled = false;
        state.profile = GovernorProfile::Off;
        return;
    }

    state.enabled = true;
    state.profile = cfg.active_profile;

    sys.refresh_cpu_usage();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let core_count = sys.cpus().len().max(1) as f32;
    let candidates: Vec<ProcessCandidate> = sys
        .processes()
        .iter()
        .map(|(pid_val, proc_entry)| ProcessCandidate {
            pid: pid_val.as_u32(),
            name: proc_entry.name().to_string_lossy().into_owned(),
            cpu_pct: (proc_entry.cpu_usage() / core_count).clamp(0.0, 100.0),
            ram_mb: proc_entry.memory() / (1024 * 1024),
        })
        .collect();

    let foreground_pid = get_foreground_pid();
    let ram_pct = current_ram_used_pct();
    let now = now_ms();
    let should_trim_now = state
        .last_trim_at_ms
        .map(|prev| now.saturating_sub(prev) >= AUTO_TRIM_COOLDOWN_MS)
        .unwrap_or(true);

    let plan = plan_governor_actions(
        cfg.active_profile,
        foreground_pid,
        &candidates,
        &cfg.exclusions,
        state.last_boosted_pid,
        &state.eco_qos_pids,
        ram_pct,
        cfg.ram_trim_threshold_pct,
        should_trim_now,
        std::process::id(),
    );

    for pid in plan.restore_pids {
        if let Some(pos) = state
            .original_priorities
            .iter()
            .position(|(p, _)| *p == pid)
        {
            let (_, orig_prio) = state.original_priorities.remove(pos);
            let _ = restore_process_state(pid, orig_prio);
        }
        if state.last_boosted_pid == Some(pid) {
            state.last_boosted_pid = None;
        }
    }

    if let Some(fg_pid) = plan.boost_foreground_pid {
        if state.last_boosted_pid != Some(fg_pid) {
            if !state.original_priorities.iter().any(|(p, _)| *p == fg_pid) {
                if let Ok(orig) = get_process_priority_class(fg_pid) {
                    state.original_priorities.push((fg_pid, orig));
                }
            }
            let _ = set_process_eco_qos(fg_pid, false);
            let high = cfg.active_profile == GovernorProfile::GamingFocus;
            if set_process_priority_boost(fg_pid, high).is_ok() {
                state.last_boosted_pid = Some(fg_pid);
            }
        }
    }

    for pid in plan.eco_qos_disable_pids {
        let _ = set_process_eco_qos(pid, false);
        state.eco_qos_pids.retain(|&p| p != pid);
    }

    for pid in plan.eco_qos_enable_pids {
        if set_process_eco_qos(pid, true).is_ok() && !state.eco_qos_pids.contains(&pid) {
            state.eco_qos_pids.push(pid);
        }
    }

    if !plan.trim_candidate_pids.is_empty() {
        if let Ok(report) = trim_idle_ram(
            &cfg.exclusions,
            AUTO_TRIM_MIN_WS_MB,
            foreground_pid.unwrap_or(0),
        ) {
            state.last_trim_freed_mb = report.freed_mb;
            state.last_trim_at_ms = Some(now);
        }
    }
}

/// Convenience wrapper for single-shot Auto-Governor ticks.
pub fn tick_governor(state: &mut GovernorState, cfg: &AppConfig, cfg_dir: &Path) {
    let mut sys = System::new();
    tick_governor_with_sys(state, cfg, cfg_dir, &mut sys);
}

/// Restores all altered process priorities/EcoQoS states and any dirty power scheme on exit.
pub fn restore_all_on_exit(state: &mut GovernorState, cfg_dir: &Path) {
    let priorities = std::mem::take(&mut state.original_priorities);
    for (pid, orig_prio) in priorities {
        let _ = restore_process_state(pid, orig_prio);
    }

    let eco_pids = std::mem::take(&mut state.eco_qos_pids);
    for pid in eco_pids {
        let _ = set_process_eco_qos(pid, false);
    }

    state.last_boosted_pid = None;
    state.enabled = false;
    let _ = recover_dirty_power_scheme(cfg_dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_test_dir(tag: &str) -> PathBuf {
        let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rtv-highperf-gov-{tag}-{}-{seq}",
            std::process::id()
        ))
    }

    struct MockPowerBackend {
        active: RefCell<String>,
        switched_to: RefCell<Vec<String>>,
    }

    impl PowerSchemeBackend for MockPowerBackend {
        fn get_active_scheme(&self) -> Result<String, AppError> {
            Ok(self.active.borrow().clone())
        }

        fn set_active_scheme(&self, guid: &str) -> Result<(), AppError> {
            *self.active.borrow_mut() = guid.to_string();
            self.switched_to.borrow_mut().push(guid.to_string());
            Ok(())
        }
    }

    #[test]
    fn test_governor_skips_protected_and_excluded_pids() {
        let self_pid = 9999;
        let fg_pid = 3001;

        let candidates = vec![
            ProcessCandidate {
                pid: 500,
                name: "csrss.exe".to_string(),
                cpu_pct: 25.0,
                ram_mb: 300,
            },
            ProcessCandidate {
                pid: 600,
                name: "dwm.exe".to_string(),
                cpu_pct: 20.0,
                ram_mb: 400,
            },
            ProcessCandidate {
                pid: 700,
                name: "MsMpEng.exe".to_string(),
                cpu_pct: 30.0,
                ram_mb: 500,
            },
            ProcessCandidate {
                pid: self_pid,
                name: "rtv-highperf.exe".to_string(),
                cpu_pct: 15.0,
                ram_mb: 200,
            },
            ProcessCandidate {
                pid: 2001,
                name: "discord.exe".to_string(),
                cpu_pct: 18.0,
                ram_mb: 450,
            },
            ProcessCandidate {
                pid: fg_pid,
                name: "game.exe".to_string(),
                cpu_pct: 55.0,
                ram_mb: 2500,
            },
            ProcessCandidate {
                pid: 4001,
                name: "background_hog.exe".to_string(),
                cpu_pct: 22.0,
                ram_mb: 600,
            },
        ];

        let exclusions = vec!["discord.exe".to_string()];
        let plan = plan_governor_actions(
            GovernorProfile::GamingFocus,
            Some(fg_pid),
            &candidates,
            &exclusions,
            None,
            &[],
            85.0,
            80,
            true,
            self_pid,
        );

        assert_eq!(plan.boost_foreground_pid, Some(fg_pid));
        assert_eq!(plan.eco_qos_enable_pids, vec![4001]);
        assert_eq!(plan.trim_candidate_pids, vec![4001]);

        for forbidden in [500, 600, 700, self_pid, 2001, fg_pid] {
            assert!(!plan.eco_qos_enable_pids.contains(&forbidden));
            assert!(!plan.trim_candidate_pids.contains(&forbidden));
        }

        // Also ensure protected processes in foreground are never boosted.
        let fg_protected_plan = plan_governor_actions(
            GovernorProfile::GamingFocus,
            Some(600),
            &candidates,
            &exclusions,
            Some(fg_pid),
            &[],
            50.0,
            80,
            false,
            self_pid,
        );
        assert_eq!(fg_protected_plan.boost_foreground_pid, None);
        assert_eq!(fg_protected_plan.restore_pids, vec![fg_pid]);
    }

    #[test]
    fn test_dirty_power_scheme_recovery_clears_flag() {
        let dir = unique_test_dir("dirty-power");
        let original_guid = "381b4222-f694-41f0-9685-ff5bb260df2e".to_string();
        let high_perf_guid = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".to_string();

        let cfg = AppConfig {
            dirty_power_scheme: Some(DirtyPowerState {
                original_guid: original_guid.clone(),
                set_at_ms: 1_763_820_000_000,
            }),
            ..AppConfig::default()
        };
        save_config(&dir, &cfg).expect("save dirty config");

        let backend = MockPowerBackend {
            active: RefCell::new(high_perf_guid),
            switched_to: RefCell::new(Vec::new()),
        };

        let recovered = recover_dirty_power_scheme_with(&dir, &backend)
            .expect("recovery should succeed");
        assert_eq!(recovered.as_deref(), Some(original_guid.as_str()));
        assert_eq!(backend.active.borrow().as_str(), original_guid);
        assert_eq!(backend.switched_to.borrow().as_slice(), &[original_guid]);

        let reloaded = load_config(&dir).expect("reload config");
        assert_eq!(reloaded.dirty_power_scheme, None);

        // Subsequent call is a no-op.
        let second = recover_dirty_power_scheme_with(&dir, &backend)
            .expect("second recovery should be Ok(None)");
        assert_eq!(second, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_set_power_plan_records_dirty_flag_once() {
        let dir = unique_test_dir("set-power");
        let balanced = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let high_perf = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
        let ultimate = "e9a42b02-d5df-448d-aa00-03f14749eb61";

        let backend = MockPowerBackend {
            active: RefCell::new(balanced.to_string()),
            switched_to: RefCell::new(Vec::new()),
        };

        set_power_plan_with(high_perf, &dir, &backend).expect("first switch succeeds");
        let after_first = load_config(&dir).expect("load config");
        assert_eq!(
            after_first.dirty_power_scheme.as_ref().map(|d| d.original_guid.as_str()),
            Some(balanced)
        );

        // Switching again should keep original_guid pointing to balanced, not high_perf.
        set_power_plan_with(ultimate, &dir, &backend).expect("second switch succeeds");
        let after_second = load_config(&dir).expect("load config");
        assert_eq!(
            after_second.dirty_power_scheme.as_ref().map(|d| d.original_guid.as_str()),
            Some(balanced)
        );
        assert_eq!(backend.active.borrow().as_str(), ultimate);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_powercfg_list() {
        let sample = "\
Existing Power Schemes (* Active)\r\n\
-----------------------------------\r\n\
Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\r\n\
Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)\r\n";

        let plans = parse_powercfg_list(sample);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].name, "Balanced");
        assert!(plans[0].is_active);
        assert_eq!(plans[1].name, "High performance");
        assert!(!plans[1].is_active);
    }
}
