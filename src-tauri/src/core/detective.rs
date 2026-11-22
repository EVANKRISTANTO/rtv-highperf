use std::cmp::Reverse;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

use crate::core::config::AppConfig;
use crate::core::safety::is_protected_process;

const POWER_SAVER_GUID: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
const FIVE_GIB: u64 = 5 * 1024 * 1024 * 1024;
const FIFTEEN_GIB: u64 = 15 * 1024 * 1024 * 1024;
const TOP_PROCESS_LIMIT: usize = 30;

static LAST_IO_SAMPLE: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixAction {
    TrimIdleRam,
    SwitchBalancedOrHighPower,
    OpenStartupManager,
    OpenDeepCleaner,
    EnableAutoGovernor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LagFinding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub deduction: u8,
    pub fix_action: Option<FixAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub ram_mb: f64,
    pub priority: String,
    pub eco_qos: bool,
    pub is_protected: bool,
    pub is_excluded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub cpu_total_pct: f32,
    pub cpu_cores: Vec<f32>,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub ram_available_bytes: u64,
    pub commit_pct: f32,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub active_power_plan: String,
    pub power_plan_guid: String,
    pub is_on_ac_power: Option<bool>,
    pub startup_enabled_count: usize,
    pub cleanable_bytes_cached: Option<u64>,
    pub top_processes: Vec<ProcessInfo>,
    pub health_score: u8,
    pub findings: Vec<LagFinding>,
    pub sampled_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthInput {
    pub cpu_total_pct: f32,
    pub ram_used_pct: f32,
    pub commit_pct: f32,
    pub disk_busy_bps: u64,
    pub power_plan_guid: String,
    pub active_power_plan: String,
    pub is_on_ac_power: Option<bool>,
    pub startup_enabled_count: usize,
    pub cleanable_bytes_cached: Option<u64>,
    pub governor_enabled: bool,
    pub high_bg_cpu_count: usize,
}

fn is_power_saver_plan(guid: &str, name: &str) -> bool {
    guid.eq_ignore_ascii_case(POWER_SAVER_GUID)
        || name.to_ascii_lowercase().contains("power saver")
}

/// Computes a deterministic PC Health Score (0..=100) and prioritized findings from observed metrics.
pub fn compute_health_score(input: &HealthInput) -> (u8, Vec<LagFinding>) {
    let mut score: i32 = 100;
    let mut findings = Vec::new();

    if input.ram_used_pct >= 90.0 {
        let deduction = 25;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "ram_critical".to_string(),
            severity: Severity::Critical,
            title: "Critical physical memory pressure".to_string(),
            detail: format!(
                "RAM usage is at {:.1}%, forcing heavy pagefile swapping.",
                input.ram_used_pct
            ),
            deduction,
            fix_action: Some(FixAction::TrimIdleRam),
        });
    } else if input.ram_used_pct >= 80.0 {
        let deduction = 15;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "ram_high".to_string(),
            severity: Severity::Warning,
            title: "Elevated memory pressure".to_string(),
            detail: format!(
                "RAM usage is at {:.1}%. Trimming idle background apps can free working set.",
                input.ram_used_pct
            ),
            deduction,
            fix_action: Some(FixAction::TrimIdleRam),
        });
    }

    if input.commit_pct >= 92.0 {
        let deduction = 10;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "commit_critical".to_string(),
            severity: Severity::Warning,
            title: "High virtual commit charge".to_string(),
            detail: format!(
                "System commit charge is {:.1}%, risking allocation stalls.",
                input.commit_pct
            ),
            deduction,
            fix_action: Some(FixAction::TrimIdleRam),
        });
    }

    if input.cpu_total_pct >= 95.0 {
        let deduction = 20;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "cpu_critical".to_string(),
            severity: Severity::Critical,
            title: "CPU near full saturation".to_string(),
            detail: format!(
                "Total CPU load is {:.1}%, causing foreground frame drops or input lag.",
                input.cpu_total_pct
            ),
            deduction,
            fix_action: Some(FixAction::EnableAutoGovernor),
        });
    } else if input.cpu_total_pct > 85.0 {
        let deduction = 12;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "cpu_high".to_string(),
            severity: Severity::Warning,
            title: "High CPU load".to_string(),
            detail: format!("Total CPU usage is {:.1}%.", input.cpu_total_pct),
            deduction,
            fix_action: Some(FixAction::EnableAutoGovernor),
        });
    }

    if !input.governor_enabled && input.high_bg_cpu_count > 0 {
        let deduction = 8;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "unthrottled_bg_cpu".to_string(),
            severity: Severity::Warning,
            title: "Unthrottled background CPU activity".to_string(),
            detail: format!(
                "{} non-protected background process(es) are consuming notable CPU while Auto-Governor is off.",
                input.high_bg_cpu_count
            ),
            deduction,
            fix_action: Some(FixAction::EnableAutoGovernor),
        });
    }

    if input.startup_enabled_count > 12 {
        let deduction = 12;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "startup_bloat".to_string(),
            severity: Severity::Warning,
            title: "Heavy startup program load".to_string(),
            detail: format!(
                "{} enabled startup items slow down Windows logon and linger in memory.",
                input.startup_enabled_count
            ),
            deduction,
            fix_action: Some(FixAction::OpenStartupManager),
        });
    } else if input.startup_enabled_count > 8 {
        let deduction = 7;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "startup_bloat".to_string(),
            severity: Severity::Warning,
            title: "Excessive startup entries".to_string(),
            detail: format!(
                "{} startup entries are enabled (recommended: 8 or fewer).",
                input.startup_enabled_count
            ),
            deduction,
            fix_action: Some(FixAction::OpenStartupManager),
        });
    }

    if input.is_on_ac_power == Some(true)
        && is_power_saver_plan(&input.power_plan_guid, &input.active_power_plan)
    {
        let deduction = 15;
        score -= i32::from(deduction);
        findings.push(LagFinding {
            id: "power_saver_on_ac".to_string(),
            severity: Severity::Warning,
            title: "Power Saver plan active on AC power".to_string(),
            detail: "Windows is throttling clock speeds while plugged into wall power.".to_string(),
            deduction,
            fix_action: Some(FixAction::SwitchBalancedOrHighPower),
        });
    }

    if let Some(cleanable_bytes) = input.cleanable_bytes_cached {
        if cleanable_bytes > FIVE_GIB {
            let (deduction, severity) = if cleanable_bytes > FIFTEEN_GIB {
                (10, Severity::Warning)
            } else {
                (5, Severity::Info)
            };
            score -= i32::from(deduction);
            let gb = cleanable_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            findings.push(LagFinding {
                id: "cleanable_junk".to_string(),
                severity,
                title: "Accumulated temporary & cache files".to_string(),
                detail: format!("{gb:.1} GB of safe-to-clean temporary and cache files detected."),
                deduction,
                fix_action: Some(FixAction::OpenDeepCleaner),
            });
        }
    }

    findings.sort_by_key(|f| (f.severity, Reverse(f.deduction), f.id.clone()));
    (score.clamp(0, 100) as u8, findings)
}

pub fn is_valid_guid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Parses `powercfg /getactivescheme` output in any Windows locale into `(guid, friendly_name)`.
pub fn parse_powercfg_active_scheme(output: &str) -> Option<(String, String)> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let guid = trimmed
            .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ':' | ','))
            .find(|tok| is_valid_guid(tok))?
            .to_ascii_lowercase();

        let name = if let (Some(open), Some(close)) = (trimmed.rfind('('), trimmed.rfind(')')) {
            if close > open + 1 {
                trimmed[open + 1..close].trim().to_string()
            } else {
                "Unknown Power Plan".to_string()
            }
        } else {
            "Unknown Power Plan".to_string()
        };

        return Some((guid, name));
    }
    None
}

#[cfg(windows)]
fn query_memory_status() -> (u64, u64, u64, f32) {
    use std::mem::size_of;
    use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut mem = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    let (total, avail, used, fallback_commit_pct) = unsafe {
        if GlobalMemoryStatusEx(&mut mem).is_ok() {
            let total = mem.ullTotalPhys;
            let avail = mem.ullAvailPhys;
            let used = total.saturating_sub(avail);
            let commit_pct = if mem.ullTotalPageFile > 0 {
                let committed = mem.ullTotalPageFile.saturating_sub(mem.ullAvailPageFile);
                (committed as f64 / mem.ullTotalPageFile as f64 * 100.0) as f32
            } else {
                0.0
            };
            (total, avail, used, commit_pct)
        } else {
            (0, 0, 0, 0.0)
        }
    };

    let mut perf = PERFORMANCE_INFORMATION {
        cb: size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
    let commit_pct = unsafe {
        if GetPerformanceInfo(&mut perf, perf.cb).is_ok() && perf.CommitLimit > 0 {
            (perf.CommitTotal as f64 / perf.CommitLimit as f64 * 100.0) as f32
        } else {
            fallback_commit_pct
        }
    };

    (used, total, avail, commit_pct.clamp(0.0, 100.0))
}

#[cfg(not(windows))]
fn query_memory_status() -> (u64, u64, u64, f32) {
    (0, 0, 0, 0.0)
}

#[cfg(windows)]
fn query_ac_power_status() -> Option<bool> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_ok() {
        match status.ACLineStatus {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(not(windows))]
fn query_ac_power_status() -> Option<bool> {
    None
}

#[cfg(windows)]
pub fn query_active_power_plan() -> (String, String) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = Command::new("powercfg")
        .arg("/getactivescheme")
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some((guid, name)) = parse_powercfg_active_scheme(&stdout) {
                return (guid, name);
            }
        }
    }

    (String::new(), "Unknown".to_string())
}

#[cfg(not(windows))]
pub fn query_active_power_plan() -> (String, String) {
    (String::new(), "Unknown".to_string())
}

#[cfg(windows)]
fn query_process_priority_label(pid: u32) -> String {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::{
        GetPriorityClass, OpenProcess, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS,
        HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        PROCESS_QUERY_LIMITED_INFORMATION, REALTIME_PRIORITY_CLASS,
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

    if pid <= 4 {
        return "System".to_string();
    }

    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return "Unknown".to_string();
        };
        let _guard = HandleGuard(handle);
        let class = GetPriorityClass(handle);
        if class == 0 {
            return "Unknown".to_string();
        }
        if class == IDLE_PRIORITY_CLASS.0 {
            "Idle".to_string()
        } else if class == BELOW_NORMAL_PRIORITY_CLASS.0 {
            "Below Normal".to_string()
        } else if class == NORMAL_PRIORITY_CLASS.0 {
            "Normal".to_string()
        } else if class == ABOVE_NORMAL_PRIORITY_CLASS.0 {
            "Above Normal".to_string()
        } else if class == HIGH_PRIORITY_CLASS.0 {
            "High".to_string()
        } else if class == REALTIME_PRIORITY_CLASS.0 {
            "Realtime".to_string()
        } else {
            format!("0x{class:x}")
        }
    }
}

#[cfg(not(windows))]
fn query_process_priority_label(_pid: u32) -> String {
    "Normal".to_string()
}

fn is_name_excluded(name: &str, exclusions: &[String]) -> bool {
    let base = name
        .trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    exclusions.iter().any(|ex| {
        let ex_base = ex
            .trim()
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(ex)
            .to_ascii_lowercase();
        !ex_base.is_empty() && ex_base == base
    })
}

/// Samples live system telemetry using Win32 APIs and `sysinfo` deltas without fabricated numbers.
pub fn sample_system(
    sys: &mut System,
    cfg: &AppConfig,
    cleanable_cached: Option<u64>,
    startup_count: usize,
    eco_qos_pids: &[u32],
) -> SystemSnapshot {
    sys.refresh_cpu_usage();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let now = Instant::now();
    let elapsed_secs = {
        let mut guard = LAST_IO_SAMPLE.lock().unwrap_or_else(|e| e.into_inner());
        let dt = guard
            .map(|prev| now.saturating_duration_since(prev).as_secs_f64())
            .unwrap_or(0.0);
        *guard = Some(now);
        dt
    };

    let cpu_cores: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let core_count = cpu_cores.len().max(1) as f32;
    let cpu_total_pct = if cpu_cores.is_empty() {
        sys.global_cpu_usage()
    } else {
        cpu_cores.iter().sum::<f32>() / core_count
    };

    let (ram_used_bytes, ram_total_bytes, ram_available_bytes, commit_pct) = query_memory_status();
    let ram_used_pct = if ram_total_bytes > 0 {
        (ram_used_bytes as f64 / ram_total_bytes as f64 * 100.0) as f32
    } else {
        0.0
    };

    let self_pid = std::process::id();
    let mut total_read_delta: u64 = 0;
    let mut total_write_delta: u64 = 0;
    let mut high_bg_cpu_count: usize = 0;

    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid_val, proc_entry)| {
            let pid = pid_val.as_u32();
            let name = proc_entry.name().to_string_lossy().into_owned();
            let cpu_pct = (proc_entry.cpu_usage() / core_count).clamp(0.0, 100.0);
            let ram_mb = proc_entry.memory() as f64 / (1024.0 * 1024.0);
            let io = proc_entry.disk_usage();
            total_read_delta = total_read_delta.saturating_add(io.read_bytes);
            total_write_delta = total_write_delta.saturating_add(io.written_bytes);

            let is_protected = is_protected_process(&name, pid, self_pid);
            let is_excluded = is_name_excluded(&name, &cfg.exclusions);

            if !is_protected && !is_excluded && cpu_pct >= 12.0 {
                high_bg_cpu_count += 1;
            }

            ProcessInfo {
                pid,
                name,
                cpu_pct,
                ram_mb,
                priority: String::new(),
                eco_qos: eco_qos_pids.contains(&pid),
                is_protected,
                is_excluded,
            }
        })
        .collect();

    processes.sort_by(|a, b| {
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.ram_mb
                    .partial_cmp(&a.ram_mb)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.pid.cmp(&b.pid))
    });
    processes.truncate(TOP_PROCESS_LIMIT);

    for p in &mut processes {
        p.priority = query_process_priority_label(p.pid);
    }

    let (disk_read_bps, disk_write_bps) = if elapsed_secs >= 0.05 {
        (
            (total_read_delta as f64 / elapsed_secs) as u64,
            (total_write_delta as f64 / elapsed_secs) as u64,
        )
    } else {
        (0, 0)
    };

    let (power_plan_guid, active_power_plan) = query_active_power_plan();
    let is_on_ac_power = query_ac_power_status();

    let health_input = HealthInput {
        cpu_total_pct,
        ram_used_pct,
        commit_pct,
        disk_busy_bps: disk_read_bps.saturating_add(disk_write_bps),
        power_plan_guid: power_plan_guid.clone(),
        active_power_plan: active_power_plan.clone(),
        is_on_ac_power,
        startup_enabled_count: startup_count,
        cleanable_bytes_cached: cleanable_cached,
        governor_enabled: cfg.governor_enabled,
        high_bg_cpu_count,
    };

    let (health_score, findings) = compute_health_score(&health_input);

    let sampled_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    SystemSnapshot {
        cpu_total_pct,
        cpu_cores,
        ram_used_bytes,
        ram_total_bytes,
        ram_available_bytes,
        commit_pct,
        disk_read_bps,
        disk_write_bps,
        active_power_plan,
        power_plan_guid,
        is_on_ac_power,
        startup_enabled_count: startup_count,
        cleanable_bytes_cached: cleanable_cached,
        top_processes: processes,
        health_score,
        findings,
        sampled_at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_score_perfect_system() {
        let input = HealthInput {
            cpu_total_pct: 15.0,
            ram_used_pct: 40.0,
            commit_pct: 38.0,
            disk_busy_bps: 1024 * 1024,
            power_plan_guid: "381b4222-f694-41f0-9685-ff5bb260df2e".to_string(),
            active_power_plan: "Balanced".to_string(),
            is_on_ac_power: Some(true),
            startup_enabled_count: 3,
            cleanable_bytes_cached: Some(256 * 1024 * 1024),
            governor_enabled: false,
            high_bg_cpu_count: 0,
        };

        let (score, findings) = compute_health_score(&input);
        assert_eq!(score, 100);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_health_score_bottlenecks() {
        let input = HealthInput {
            cpu_total_pct: 35.0,
            ram_used_pct: 88.0,
            commit_pct: 75.0,
            disk_busy_bps: 0,
            power_plan_guid: POWER_SAVER_GUID.to_string(),
            active_power_plan: "Power saver".to_string(),
            is_on_ac_power: Some(true),
            startup_enabled_count: 14,
            cleanable_bytes_cached: Some(8 * 1024 * 1024 * 1024),
            governor_enabled: false,
            high_bg_cpu_count: 0,
        };

        let (score, findings) = compute_health_score(&input);
        // 100 - 15 (RAM >= 80%) - 12 (startup > 12) - 15 (Power Saver on AC) - 5 (cleanable > 5GB) = 53
        assert_eq!(score, 53);
        assert_eq!(findings.len(), 4);

        let actions: Vec<Option<FixAction>> = findings.iter().map(|f| f.fix_action).collect();
        assert!(actions.contains(&Some(FixAction::TrimIdleRam)));
        assert!(actions.contains(&Some(FixAction::OpenStartupManager)));
        assert!(actions.contains(&Some(FixAction::SwitchBalancedOrHighPower)));
        assert!(actions.contains(&Some(FixAction::OpenDeepCleaner)));

        // Findings must be ordered from highest severity to lowest.
        for pair in findings.windows(2) {
            assert!(pair[0].severity <= pair[1].severity);
        }
    }

    #[test]
    fn test_parse_powercfg_active_scheme() {
        let sample = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\r\n";
        let parsed = parse_powercfg_active_scheme(sample).expect("should parse GUID and plan name");
        assert_eq!(parsed.0, "381b4222-f694-41f0-9685-ff5bb260df2e");
        assert_eq!(parsed.1, "Balanced");
    }
}
