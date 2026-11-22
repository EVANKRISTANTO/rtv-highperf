use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::core::config::default_config_dir;
use crate::core::privilege::is_elevated;
use crate::core::safety::AppError;

static SNAPSHOT_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TweakCategory {
    LatencyGaming,
    PrivacyTelemetry,
    UiSnappiness,
    BackgroundServices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Safe,
    Moderate,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TweakState {
    Applied,
    Default,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TweakItem {
    pub id: String,
    pub category: TweakCategory,
    pub title: String,
    pub description: String,
    pub risk: RiskLevel,
    pub requires_admin: bool,
    pub target_summary: String,
    pub current_value_display: String,
    pub target_value_display: String,
    pub state: TweakState,
    pub is_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegValueBackup {
    NotExisted,
    Dword(u32),
    Qword(u64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupEntry {
    Registry {
        hive: String,
        subkey: String,
        value_name: String,
        previous: RegValueBackup,
    },
    Service {
        service_name: String,
        previous_start_type: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSnapshot {
    pub id: String,
    pub created_at: String,
    pub label: String,
    pub entries: Vec<BackupEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppxPackageItem {
    pub name: String,
    pub package_full_name: String,
    pub publisher: String,
    pub is_removable_bloat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegHive {
    Hkcu,
    Hklm,
}

impl RegHive {
    fn as_short_str(self) -> &'static str {
        match self {
            Self::Hkcu => "HKCU",
            Self::Hklm => "HKLM",
        }
    }

    fn as_reg_export_str(self) -> &'static str {
        match self {
            Self::Hkcu => "HKEY_CURRENT_USER",
            Self::Hklm => "HKEY_LOCAL_MACHINE",
        }
    }

    fn parse(raw: &str) -> Result<Self, AppError> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "HKCU" | "HKEY_CURRENT_USER" => Ok(Self::Hkcu),
            "HKLM" | "HKEY_LOCAL_MACHINE" => Ok(Self::Hklm),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported registry hive: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegTargetValue {
    Dword(u32),
    #[allow(dead_code)]
    Qword(u64),
    String(&'static str),
}

impl RegTargetValue {
    fn to_backup(self) -> RegValueBackup {
        match self {
            Self::Dword(v) => RegValueBackup::Dword(v),
            Self::Qword(v) => RegValueBackup::Qword(v),
            Self::String(s) => RegValueBackup::String(s.to_string()),
        }
    }

    fn matches(self, current: &RegValueBackup) -> bool {
        match (self, current) {
            (Self::Dword(a), RegValueBackup::Dword(b)) => a == *b,
            (Self::Qword(a), RegValueBackup::Qword(b)) => a == *b,
            (Self::String(a), RegValueBackup::String(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TweakTargetSpec {
    Registry {
        hive: RegHive,
        subkey: &'static str,
        value_name: &'static str,
        target: RegTargetValue,
        default: Option<RegTargetValue>,
    },
    Service {
        service_name: &'static str,
        target_start_type: u32,
        default_start_type: u32,
    },
}

#[derive(Debug, Clone, Copy)]
struct TweakDefinition {
    id: &'static str,
    category: TweakCategory,
    title: &'static str,
    description: &'static str,
    risk: RiskLevel,
    requires_admin: bool,
    target_summary: &'static str,
    target_value_display: &'static str,
    targets: &'static [TweakTargetSpec],
}

const SERVICE_START_AUTO: u32 = 2;
const SERVICE_START_DEMAND: u32 = 3;
const SERVICE_START_DISABLED: u32 = 4;

fn format_service_start_type(start_type: u32) -> String {
    match start_type {
        0 => "Boot (0)".to_string(),
        1 => "System (1)".to_string(),
        2 => "Automatic (2)".to_string(),
        3 => "Manual (3)".to_string(),
        4 => "Disabled (4)".to_string(),
        other => format!("Unknown ({other})"),
    }
}

fn format_reg_backup_display(val: &RegValueBackup) -> String {
    match val {
        RegValueBackup::NotExisted => "Not set".to_string(),
        RegValueBackup::Dword(v) => format!("{v} (0x{v:X})"),
        RegValueBackup::Qword(v) => format!("{v} (0x{v:X})"),
        RegValueBackup::String(s) => format!("\"{s}\""),
    }
}

const TWEAK_CATALOG: &[TweakDefinition] = &[
    // --- Latency & Gaming ---
    TweakDefinition {
        id: "gaming_disable_gamedvr",
        category: TweakCategory::LatencyGaming,
        title: "Disable Xbox Game DVR Background Capture",
        description: "Disables Xbox Game DVR background recording and capture hooks to reduce frame-time spikes and GPU encoder overhead.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\System\GameConfigStore\GameDVR_Enabled & GameDVR\AppCaptureEnabled",
        target_value_display: "0 (Disabled)",
        targets: &[
            TweakTargetSpec::Registry {
                hive: RegHive::Hkcu,
                subkey: r"System\GameConfigStore",
                value_name: "GameDVR_Enabled",
                target: RegTargetValue::Dword(0),
                default: Some(RegTargetValue::Dword(1)),
            },
            TweakTargetSpec::Registry {
                hive: RegHive::Hkcu,
                subkey: r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                value_name: "AppCaptureEnabled",
                target: RegTargetValue::Dword(0),
                default: Some(RegTargetValue::Dword(1)),
            },
        ],
    },
    TweakDefinition {
        id: "gaming_disable_mouse_accel",
        category: TweakCategory::LatencyGaming,
        title: "Disable Mouse Acceleration (Enhance Pointer Precision)",
        description: "Sets MouseSpeed to 0 for linear 1:1 pointer movement without Windows acceleration curve distortion.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Control Panel\Mouse\MouseSpeed",
        target_value_display: "\"0\" (Off)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Control Panel\Mouse",
            value_name: "MouseSpeed",
            target: RegTargetValue::String("0"),
            default: Some(RegTargetValue::String("1")),
        }],
    },
    TweakDefinition {
        id: "gaming_network_throttling",
        category: TweakCategory::LatencyGaming,
        title: "Disable Multimedia Network Throttling",
        description: "Sets NetworkThrottlingIndex to 0xFFFFFFFF so Windows does not throttle background/game packets during media playback.",
        risk: RiskLevel::Moderate,
        requires_admin: true,
        target_summary: r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\NetworkThrottlingIndex",
        target_value_display: "0xFFFFFFFF (Disabled)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hklm,
            subkey: r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
            value_name: "NetworkThrottlingIndex",
            target: RegTargetValue::Dword(0xFFFF_FFFF),
            default: Some(RegTargetValue::Dword(10)),
        }],
    },
    TweakDefinition {
        id: "gaming_system_responsiveness",
        category: TweakCategory::LatencyGaming,
        title: "Prioritize Foreground CPU Scheduling (MMCSS)",
        description: "Lowers Multimedia Class Scheduler background CPU reservation from 20% to 10% to favor active foreground applications and games.",
        risk: RiskLevel::Moderate,
        requires_admin: true,
        target_summary: r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\SystemResponsiveness",
        target_value_display: "10 (10% background reserve)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hklm,
            subkey: r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
            value_name: "SystemResponsiveness",
            target: RegTargetValue::Dword(10),
            default: Some(RegTargetValue::Dword(20)),
        }],
    },
    TweakDefinition {
        id: "gaming_disable_power_throttling",
        category: TweakCategory::LatencyGaming,
        title: "Disable Windows CPU Power Throttling",
        description: "Disables OS-level power throttling for background/foreground workloads (recommended on AC power or desktops; may increase laptop battery draw).",
        risk: RiskLevel::Moderate,
        requires_admin: true,
        target_summary: r"HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerThrottling\PowerThrottlingOff",
        target_value_display: "1 (Throttling Off)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hklm,
            subkey: r"SYSTEM\CurrentControlSet\Control\Power\PowerThrottling",
            value_name: "PowerThrottlingOff",
            target: RegTargetValue::Dword(1),
            default: None,
        }],
    },
    // --- Privacy & Telemetry ---
    TweakDefinition {
        id: "privacy_disable_telemetry",
        category: TweakCategory::PrivacyTelemetry,
        title: "Minimize Windows Diagnostic Telemetry",
        description: "Sets AllowTelemetry policy to 0 (minimum security/required diagnostic level supported by your Windows edition) without breaking Windows Update.",
        risk: RiskLevel::Safe,
        requires_admin: true,
        target_summary: r"HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection\AllowTelemetry",
        target_value_display: "0 (Minimum)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hklm,
            subkey: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            value_name: "AllowTelemetry",
            target: RegTargetValue::Dword(0),
            default: None,
        }],
    },
    TweakDefinition {
        id: "privacy_disable_advertising_id",
        category: TweakCategory::PrivacyTelemetry,
        title: "Disable Windows Advertising ID",
        description: "Prevents Windows apps from using your per-user Advertising ID for cross-app ad tracking.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo\Enabled",
        target_value_display: "0 (Disabled)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            value_name: "Enabled",
            target: RegTargetValue::Dword(0),
            default: Some(RegTargetValue::Dword(1)),
        }],
    },
    TweakDefinition {
        id: "privacy_disable_tailored_experiences",
        category: TweakCategory::PrivacyTelemetry,
        title: "Disable Tailored Experiences",
        description: "Stops Microsoft from using diagnostic data to show personalized tips, recommendations, and ads.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Privacy\TailoredExperiencesWithDiagnosticDataEnabled",
        target_value_display: "0 (Disabled)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Privacy",
            value_name: "TailoredExperiencesWithDiagnosticDataEnabled",
            target: RegTargetValue::Dword(0),
            default: Some(RegTargetValue::Dword(1)),
        }],
    },
    TweakDefinition {
        id: "privacy_disable_feedback_frequency",
        category: TweakCategory::PrivacyTelemetry,
        title: "Disable Windows Feedback Prompts (SIUF)",
        description: "Stops Windows from periodically asking for user feedback notifications.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Software\Microsoft\Siuf\Rules\NumberOfSIUFInPeriod",
        target_value_display: "0 (Never)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Siuf\Rules",
            value_name: "NumberOfSIUFInPeriod",
            target: RegTargetValue::Dword(0),
            default: None,
        }],
    },
    // --- UI Snappiness ---
    TweakDefinition {
        id: "ui_fast_menu_delay",
        category: TweakCategory::UiSnappiness,
        title: "Reduce Menu Show Delay",
        description: "Reduces hover delay before classic submenus open from 400 ms to 100 ms for snappier desktop navigation.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Control Panel\Desktop\MenuShowDelay",
        target_value_display: "\"100\" (100 ms)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Control Panel\Desktop",
            value_name: "MenuShowDelay",
            target: RegTargetValue::String("100"),
            default: Some(RegTargetValue::String("400")),
        }],
    },
    TweakDefinition {
        id: "ui_disable_suggestions",
        category: TweakCategory::UiSnappiness,
        title: "Disable Start & Settings Suggested Content",
        description: "Disables promotional suggestions in Windows Settings and Start menu via ContentDeliveryManager flags.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\...\ContentDeliveryManager\SubscribedContent-338393Enabled & 353698Enabled",
        target_value_display: "0 (Disabled)",
        targets: &[
            TweakTargetSpec::Registry {
                hive: RegHive::Hkcu,
                subkey: r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
                value_name: "SubscribedContent-338393Enabled",
                target: RegTargetValue::Dword(0),
                default: Some(RegTargetValue::Dword(1)),
            },
            TweakTargetSpec::Registry {
                hive: RegHive::Hkcu,
                subkey: r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
                value_name: "SubscribedContent-353698Enabled",
                target: RegTargetValue::Dword(0),
                default: Some(RegTargetValue::Dword(1)),
            },
        ],
    },
    TweakDefinition {
        id: "ui_disable_transparency",
        category: TweakCategory::UiSnappiness,
        title: "Disable Windows Acrylic Transparency Effects",
        description: "Turns off translucent blur composition in Start, Taskbar, and Settings to save GPU compositor overhead.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\EnableTransparency",
        target_value_display: "0 (Off)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            value_name: "EnableTransparency",
            target: RegTargetValue::Dword(0),
            default: Some(RegTargetValue::Dword(1)),
        }],
    },
    TweakDefinition {
        id: "ui_disable_bing_search",
        category: TweakCategory::UiSnappiness,
        title: "Disable Taskbar Bing Web Search",
        description: "Keeps Windows Search focused on local files and apps instead of fetching Bing web results on every keystroke.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search\BingSearchEnabled",
        target_value_display: "0 (Local Only)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Search",
            value_name: "BingSearchEnabled",
            target: RegTargetValue::Dword(0),
            default: Some(RegTargetValue::Dword(1)),
        }],
    },
    TweakDefinition {
        id: "ui_disable_lockscreen_tips",
        category: TweakCategory::UiSnappiness,
        title: "Disable Lock Screen Tips & Fun Facts",
        description: "Removes Microsoft Spotlight trivia and promotional overlays from the Windows lock screen.",
        risk: RiskLevel::Safe,
        requires_admin: false,
        target_summary: r"HKCU\...\ContentDeliveryManager\SubscribedContent-338387Enabled",
        target_value_display: "0 (Disabled)",
        targets: &[TweakTargetSpec::Registry {
            hive: RegHive::Hkcu,
            subkey: r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
            value_name: "SubscribedContent-338387Enabled",
            target: RegTargetValue::Dword(0),
            default: Some(RegTargetValue::Dword(1)),
        }],
    },
    // --- Background Services (Admin) ---
    TweakDefinition {
        id: "svc_disable_diagtrack",
        category: TweakCategory::BackgroundServices,
        title: "Disable DiagTrack Telemetry Service",
        description: "Sets Connected User Experiences and Telemetry (DiagTrack) startup type to Disabled (4) instead of Automatic (2).",
        risk: RiskLevel::Safe,
        requires_admin: true,
        target_summary: "Service: DiagTrack",
        target_value_display: "Disabled (4)",
        targets: &[TweakTargetSpec::Service {
            service_name: "DiagTrack",
            target_start_type: SERVICE_START_DISABLED,
            default_start_type: SERVICE_START_AUTO,
        }],
    },
    TweakDefinition {
        id: "svc_disable_dmwappush",
        category: TweakCategory::BackgroundServices,
        title: "Disable dmwappushservice (WAP Push Routing)",
        description: "Sets Device Management Wireless Application Protocol Push service startup type to Disabled (4).",
        risk: RiskLevel::Safe,
        requires_admin: true,
        target_summary: "Service: dmwappushservice",
        target_value_display: "Disabled (4)",
        targets: &[TweakTargetSpec::Service {
            service_name: "dmwappushservice",
            target_start_type: SERVICE_START_DISABLED,
            default_start_type: SERVICE_START_DEMAND,
        }],
    },
    TweakDefinition {
        id: "svc_disable_mapsbroker",
        category: TweakCategory::BackgroundServices,
        title: "Disable Downloaded Maps Manager (MapsBroker)",
        description: "Sets MapsBroker startup type to Disabled (4) when offline Windows Maps downloads are unused (default Manual 3).",
        risk: RiskLevel::Safe,
        requires_admin: true,
        target_summary: "Service: MapsBroker",
        target_value_display: "Disabled (4)",
        targets: &[TweakTargetSpec::Service {
            service_name: "MapsBroker",
            target_start_type: SERVICE_START_DISABLED,
            default_start_type: SERVICE_START_DEMAND,
        }],
    },
    TweakDefinition {
        id: "svc_disable_sysmain",
        category: TweakCategory::BackgroundServices,
        title: "Disable SysMain (Superfetch)",
        description: "Sets SysMain startup type to Disabled (4) to curb background disk pre-fetching. Recommended for SSD/NVMe drives; keep enabled on mechanical HDDs.",
        risk: RiskLevel::Moderate,
        requires_admin: true,
        target_summary: "Service: SysMain (SSD recommended, keep on HDD)",
        target_value_display: "Disabled (4)",
        targets: &[TweakTargetSpec::Service {
            service_name: "SysMain",
            target_start_type: SERVICE_START_DISABLED,
            default_start_type: SERVICE_START_AUTO,
        }],
    },
];

/// Returns the default backup snapshot directory (`%APPDATA%\rtv-highperf\backups`).
pub fn default_backup_dir() -> PathBuf {
    default_config_dir().join("backups")
}

fn find_tweak_def(id: &str) -> Result<&'static TweakDefinition, AppError> {
    TWEAK_CATALOG
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::InvalidInput(format!("Unknown tweak ID: {id}")))
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod win32 {
    use super::*;
    use std::mem::size_of;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA,
        ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ,
        KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_QWORD, REG_SZ,
        REG_VALUE_TYPE,
    };
    use windows::Win32::System::Services::{
        ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW,
        QueryServiceConfigW, ENUM_SERVICE_TYPE, QUERY_SERVICE_CONFIGW, SC_HANDLE,
        SC_MANAGER_CONNECT, SERVICE_CHANGE_CONFIG, SERVICE_ERROR, SERVICE_NO_CHANGE,
        SERVICE_QUERY_CONFIG, SERVICE_START_TYPE,
    };

    struct HKeyGuard(HKEY);
    impl Drop for HKeyGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = RegCloseKey(self.0);
                }
            }
        }
    }

    struct ScHandleGuard(SC_HANDLE);
    impl Drop for ScHandleGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseServiceHandle(self.0);
                }
            }
        }
    }

    fn root_hkey(hive: RegHive) -> HKEY {
        match hive {
            RegHive::Hkcu => HKEY_CURRENT_USER,
            RegHive::Hklm => HKEY_LOCAL_MACHINE,
        }
    }

    fn is_access_denied_win32(err: WIN32_ERROR) -> bool {
        err == ERROR_ACCESS_DENIED
    }

    fn is_access_denied_core(err: &windows::core::Error) -> bool {
        WIN32_ERROR::from_error(err) == Some(ERROR_ACCESS_DENIED)
    }

    fn map_win32_error(context: &str, err: WIN32_ERROR) -> AppError {
        if is_access_denied_win32(err) {
            AppError::ElevationRequired(format!("{context} requires administrator privileges"))
        } else {
            AppError::Win32(format!("{context} failed with Win32 error {}", err.0))
        }
    }

    fn map_core_error(context: &str, err: windows::core::Error) -> AppError {
        if is_access_denied_core(&err) {
            AppError::ElevationRequired(format!("{context} requires administrator privileges"))
        } else {
            AppError::Win32(format!("{context} failed: {err}"))
        }
    }

    pub fn read_reg_value(
        hive: RegHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<RegValueBackup, AppError> {
        let subkey_w = to_wide_null(subkey);
        let value_w = to_wide_null(value_name);
        let mut hkey = HKEY::default();

        let open_err = unsafe {
            RegOpenKeyExW(
                root_hkey(hive),
                PCWSTR(subkey_w.as_ptr()),
                0,
                KEY_READ | KEY_QUERY_VALUE,
                &mut hkey,
            )
        };

        if open_err == ERROR_FILE_NOT_FOUND {
            return Ok(RegValueBackup::NotExisted);
        }
        if open_err != ERROR_SUCCESS {
            return Err(map_win32_error(
                &format!(r"RegOpenKeyExW({}\{subkey})", hive.as_short_str()),
                open_err,
            ));
        }
        let _guard = HKeyGuard(hkey);

        let mut val_type = REG_VALUE_TYPE::default();
        let mut data_len = 0u32;
        let size_err = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(value_w.as_ptr()),
                None,
                Some(&mut val_type),
                None,
                Some(&mut data_len),
            )
        };

        if size_err == ERROR_FILE_NOT_FOUND {
            return Ok(RegValueBackup::NotExisted);
        }
        if size_err != ERROR_SUCCESS && size_err != ERROR_MORE_DATA {
            return Err(map_win32_error(
                &format!(
                    r"RegQueryValueExW({}\{subkey}\{value_name})",
                    hive.as_short_str()
                ),
                size_err,
            ));
        }

        let mut buf = vec![0u8; data_len.max(4) as usize];
        let mut actual_len = buf.len() as u32;
        let query_err = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(value_w.as_ptr()),
                None,
                Some(&mut val_type),
                Some(buf.as_mut_ptr()),
                Some(&mut actual_len),
            )
        };

        if query_err == ERROR_FILE_NOT_FOUND {
            return Ok(RegValueBackup::NotExisted);
        }
        if query_err != ERROR_SUCCESS {
            return Err(map_win32_error(
                &format!(
                    r"RegQueryValueExW({}\{subkey}\{value_name})",
                    hive.as_short_str()
                ),
                query_err,
            ));
        }
        buf.truncate(actual_len as usize);

        if val_type == REG_DWORD {
            if buf.len() < 4 {
                return Err(AppError::Win32(format!(
                    "Malformed REG_DWORD payload length {} for {subkey}\\{value_name}",
                    buf.len()
                )));
            }
            let val = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            Ok(RegValueBackup::Dword(val))
        } else if val_type == REG_QWORD {
            if buf.len() < 8 {
                return Err(AppError::Win32(format!(
                    "Malformed REG_QWORD payload length {} for {subkey}\\{value_name}",
                    buf.len()
                )));
            }
            let val = u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]);
            Ok(RegValueBackup::Qword(val))
        } else if val_type == REG_SZ || val_type == REG_EXPAND_SZ {
            let u16_slice: Vec<u16> = buf
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let trimmed = match u16_slice.iter().position(|&c| c == 0) {
                Some(pos) => &u16_slice[..pos],
                None => &u16_slice[..],
            };
            Ok(RegValueBackup::String(String::from_utf16_lossy(trimmed)))
        } else {
            Err(AppError::Win32(format!(
                "Unsupported registry value type {} at {subkey}\\{value_name}",
                val_type.0
            )))
        }
    }

    pub fn write_reg_value(
        hive: RegHive,
        subkey: &str,
        value_name: &str,
        value: &RegValueBackup,
    ) -> Result<(), AppError> {
        match value {
            RegValueBackup::NotExisted => delete_reg_value(hive, subkey, value_name),
            RegValueBackup::Dword(v) => {
                let bytes = v.to_le_bytes();
                set_reg_raw(hive, subkey, value_name, REG_DWORD, &bytes)
            }
            RegValueBackup::Qword(v) => {
                let bytes = v.to_le_bytes();
                set_reg_raw(hive, subkey, value_name, REG_QWORD, &bytes)
            }
            RegValueBackup::String(s) => {
                let wide = to_wide_null(s);
                let mut bytes = Vec::with_capacity(wide.len() * 2);
                for ch in wide {
                    bytes.extend_from_slice(&ch.to_le_bytes());
                }
                set_reg_raw(hive, subkey, value_name, REG_SZ, &bytes)
            }
        }
    }

    fn set_reg_raw(
        hive: RegHive,
        subkey: &str,
        value_name: &str,
        val_type: REG_VALUE_TYPE,
        data: &[u8],
    ) -> Result<(), AppError> {
        let subkey_w = to_wide_null(subkey);
        let value_w = to_wide_null(value_name);
        let mut hkey = HKEY::default();

        let create_err = unsafe {
            RegCreateKeyExW(
                root_hkey(hive),
                PCWSTR(subkey_w.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut hkey,
                None,
            )
        };
        if create_err != ERROR_SUCCESS {
            return Err(map_win32_error(
                &format!(r"RegCreateKeyExW({}\{subkey})", hive.as_short_str()),
                create_err,
            ));
        }
        let _guard = HKeyGuard(hkey);

        let set_err = unsafe {
            RegSetValueExW(
                hkey,
                PCWSTR(value_w.as_ptr()),
                0,
                val_type,
                Some(data),
            )
        };
        if set_err != ERROR_SUCCESS {
            return Err(map_win32_error(
                &format!(
                    r"RegSetValueExW({}\{subkey}\{value_name})",
                    hive.as_short_str()
                ),
                set_err,
            ));
        }

        Ok(())
    }

    pub fn delete_reg_value(
        hive: RegHive,
        subkey: &str,
        value_name: &str,
    ) -> Result<(), AppError> {
        let subkey_w = to_wide_null(subkey);
        let value_w = to_wide_null(value_name);
        let mut hkey = HKEY::default();

        let open_err = unsafe {
            RegOpenKeyExW(
                root_hkey(hive),
                PCWSTR(subkey_w.as_ptr()),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            )
        };

        if open_err == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if open_err != ERROR_SUCCESS {
            return Err(map_win32_error(
                &format!(r"RegOpenKeyExW({}\{subkey})", hive.as_short_str()),
                open_err,
            ));
        }
        let _guard = HKeyGuard(hkey);

        let del_err = unsafe { RegDeleteValueW(hkey, PCWSTR(value_w.as_ptr())) };
        if del_err == ERROR_FILE_NOT_FOUND || del_err == ERROR_SUCCESS {
            return Ok(());
        }

        Err(map_win32_error(
            &format!(
                r"RegDeleteValueW({}\{subkey}\{value_name})",
                hive.as_short_str()
            ),
            del_err,
        ))
    }

    pub fn query_service_start_type(service_name: &str) -> Result<u32, AppError> {
        let svc_w = to_wide_null(service_name);
        let scm = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
            .map_err(|e| map_core_error("OpenSCManagerW", e))?;
        let _scm_guard = ScHandleGuard(scm);

        let svc = unsafe { OpenServiceW(scm, PCWSTR(svc_w.as_ptr()), SERVICE_QUERY_CONFIG) }
            .map_err(|e| map_core_error(&format!("OpenServiceW({service_name})"), e))?;
        let _svc_guard = ScHandleGuard(svc);

        let mut bytes_needed = 0u32;
        let first_res = unsafe { QueryServiceConfigW(svc, None, 0, &mut bytes_needed) };
        if let Err(err) = first_res {
            if WIN32_ERROR::from_error(&err) != Some(ERROR_INSUFFICIENT_BUFFER) {
                return Err(map_core_error(
                    &format!("QueryServiceConfigW({service_name})"),
                    err,
                ));
            }
        }

        let buf_bytes = (bytes_needed as usize).max(size_of::<QUERY_SERVICE_CONFIGW>());
        let word_count = buf_bytes.div_ceil(size_of::<u64>());
        let mut buf = vec![0u64; word_count];
        let cfg_ptr = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
        let actual_buf_bytes = (word_count * size_of::<u64>()) as u32;

        unsafe { QueryServiceConfigW(svc, Some(cfg_ptr), actual_buf_bytes, &mut bytes_needed) }
            .map_err(|e| map_core_error(&format!("QueryServiceConfigW({service_name})"), e))?;

        let start_type = unsafe { (*cfg_ptr).dwStartType.0 };
        Ok(start_type)
    }

    pub fn set_service_start_type(service_name: &str, start_type: u32) -> Result<(), AppError> {
        let svc_w = to_wide_null(service_name);
        let scm = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
            .map_err(|e| map_core_error("OpenSCManagerW", e))?;
        let _scm_guard = ScHandleGuard(scm);

        let svc = unsafe { OpenServiceW(scm, PCWSTR(svc_w.as_ptr()), SERVICE_CHANGE_CONFIG) }
            .map_err(|e| map_core_error(&format!("OpenServiceW({service_name})"), e))?;
        let _svc_guard = ScHandleGuard(svc);

        unsafe {
            ChangeServiceConfigW(
                svc,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                SERVICE_START_TYPE(start_type),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
        }
        .map_err(|e| map_core_error(&format!("ChangeServiceConfigW({service_name})"), e))?;

        Ok(())
    }
}

#[cfg(not(windows))]
mod win32 {
    use super::*;

    pub fn read_reg_value(
        _hive: RegHive,
        _subkey: &str,
        _value_name: &str,
    ) -> Result<RegValueBackup, AppError> {
        Err(AppError::Win32(
            "Windows Registry APIs are only supported on Windows".to_string(),
        ))
    }

    pub fn write_reg_value(
        _hive: RegHive,
        _subkey: &str,
        _value_name: &str,
        _value: &RegValueBackup,
    ) -> Result<(), AppError> {
        Err(AppError::Win32(
            "Windows Registry APIs are only supported on Windows".to_string(),
        ))
    }

    pub fn query_service_start_type(_service_name: &str) -> Result<u32, AppError> {
        Err(AppError::Win32(
            "Windows Service APIs are only supported on Windows".to_string(),
        ))
    }

    pub fn set_service_start_type(_service_name: &str, _start_type: u32) -> Result<(), AppError> {
        Err(AppError::Win32(
            "Windows Service APIs are only supported on Windows".to_string(),
        ))
    }
}

fn evaluate_tweak_status(def: &TweakDefinition) -> (TweakState, String) {
    let mut all_applied = true;
    let mut all_default = true;
    let mut any_error = false;
    let mut displays = Vec::with_capacity(def.targets.len());

    for target in def.targets {
        match *target {
            TweakTargetSpec::Registry {
                hive,
                subkey,
                value_name,
                target: desired,
                default,
            } => match win32::read_reg_value(hive, subkey, value_name) {
                Ok(current) => {
                    displays.push(format_reg_backup_display(&current));
                    if !desired.matches(&current) {
                        all_applied = false;
                    }
                    let is_def = match default {
                        Some(d) => d.matches(&current),
                        None => matches!(current, RegValueBackup::NotExisted),
                    };
                    if !is_def {
                        all_default = false;
                    }
                }
                Err(_) => {
                    any_error = true;
                    all_applied = false;
                    all_default = false;
                    displays.push("Unavailable".to_string());
                }
            },
            TweakTargetSpec::Service {
                service_name,
                target_start_type,
                default_start_type,
            } => match win32::query_service_start_type(service_name) {
                Ok(start_type) => {
                    displays.push(format_service_start_type(start_type));
                    if start_type != target_start_type {
                        all_applied = false;
                    }
                    if start_type != default_start_type {
                        all_default = false;
                    }
                }
                Err(_) => {
                    any_error = true;
                    all_applied = false;
                    all_default = false;
                    displays.push("Unavailable".to_string());
                }
            },
        }
    }

    let state = if any_error {
        TweakState::Unknown
    } else if all_applied {
        TweakState::Applied
    } else if all_default {
        TweakState::Default
    } else {
        TweakState::Custom
    };

    let current_display = if displays.len() == 1 {
        displays.into_iter().next().unwrap_or_default()
    } else {
        displays.join(" | ")
    };

    (state, current_display)
}

/// Lists all declarative tweaks in the catalog with their live Windows Registry/Service state.
pub fn list_tweaks() -> Vec<TweakItem> {
    TWEAK_CATALOG
        .iter()
        .map(|def| {
            let (state, current_value_display) = evaluate_tweak_status(def);
            TweakItem {
                id: def.id.to_string(),
                category: def.category,
                title: def.title.to_string(),
                description: def.description.to_string(),
                risk: def.risk,
                requires_admin: def.requires_admin,
                target_summary: def.target_summary.to_string(),
                current_value_display,
                target_value_display: def.target_value_display.to_string(),
                state,
                is_applied: state == TweakState::Applied,
            }
        })
        .collect()
}

fn build_snapshot_id_and_time() -> (String, String) {
    let now = Utc::now();
    let seq = SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
    let id = format!(
        "snapshot-{}-{:03}",
        now.format("%Y%m%dT%H%M%SZ"),
        seq % 1000
    );
    let created_at = now.to_rfc3339();
    (id, created_at)
}

fn validate_snapshot_id(snapshot_id: &str) -> Result<(), AppError> {
    if snapshot_id.is_empty()
        || snapshot_id.contains("..")
        || snapshot_id.contains('/')
        || snapshot_id.contains('\\')
        || snapshot_id.contains('\0')
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid snapshot ID: {snapshot_id}"
        )));
    }
    Ok(())
}

fn snapshot_file_path(backup_dir: &Path, snapshot_id: &str) -> PathBuf {
    backup_dir.join(format!("{snapshot_id}.json"))
}

fn save_snapshot(backup_dir: &Path, snapshot: &BackupSnapshot) -> Result<(), AppError> {
    fs::create_dir_all(backup_dir)?;

    let target = snapshot_file_path(backup_dir, &snapshot.id);
    let seq = SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = backup_dir.join(format!(
        "{}.tmp.{}.{}",
        snapshot.id,
        std::process::id(),
        seq
    ));
    let json = serde_json::to_string_pretty(snapshot)?;

    let write_res = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(AppError::from(err));
    }

    if let Err(err) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AppError::from(err));
    }

    Ok(())
}

fn load_snapshot(snapshot_id: &str, backup_dir: &Path) -> Result<BackupSnapshot, AppError> {
    validate_snapshot_id(snapshot_id)?;
    let direct_path = snapshot_file_path(backup_dir, snapshot_id);
    if direct_path.is_file() {
        let raw = fs::read_to_string(&direct_path)?;
        return Ok(serde_json::from_str::<BackupSnapshot>(&raw)?);
    }

    // Fallback: match by `id` field across JSON files in `backup_dir`.
    if let Some(found) = list_backups(backup_dir)
        .into_iter()
        .find(|s| s.id == snapshot_id)
    {
        return Ok(found);
    }

    Err(AppError::InvalidInput(format!(
        "Snapshot '{snapshot_id}' not found in {}",
        backup_dir.display()
    )))
}

fn restore_backup_entry(entry: &BackupEntry) -> Result<(), AppError> {
    match entry {
        BackupEntry::Registry {
            hive,
            subkey,
            value_name,
            previous,
        } => {
            let parsed_hive = RegHive::parse(hive)?;
            win32::write_reg_value(parsed_hive, subkey, value_name, previous)
        }
        BackupEntry::Service {
            service_name,
            previous_start_type,
        } => win32::set_service_start_type(service_name, *previous_start_type),
    }
}

fn entry_requires_admin(entry: &BackupEntry) -> bool {
    match entry {
        BackupEntry::Registry { hive, .. } => matches!(RegHive::parse(hive), Ok(RegHive::Hklm)),
        BackupEntry::Service { .. } => true,
    }
}

/// Captures exact pre-change values into a JSON snapshot in `backup_dir`, then applies the
/// requested tweaks. Rolls back any partial changes on error.
pub fn apply_tweaks(tweak_ids: &[String], backup_dir: &Path) -> Result<BackupSnapshot, AppError> {
    if tweak_ids.is_empty() {
        return Err(AppError::InvalidInput(
            "No tweak IDs specified to apply".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    let mut defs = Vec::with_capacity(tweak_ids.len());
    for id in tweak_ids {
        if seen.insert(id.as_str()) {
            defs.push(find_tweak_def(id)?);
        }
    }

    if defs.iter().any(|d| d.requires_admin) && !is_elevated() {
        return Err(AppError::ElevationRequired(
            "One or more selected tweaks modify HKLM or Windows Services and require administrator privileges".to_string(),
        ));
    }

    // Record exact previous values BEFORE mutating anything.
    let mut backup_entries = Vec::new();
    let mut targets_to_apply = Vec::new();
    for def in &defs {
        for target in def.targets {
            match *target {
                TweakTargetSpec::Registry {
                    hive,
                    subkey,
                    value_name,
                    target: desired,
                    ..
                } => {
                    let previous = win32::read_reg_value(hive, subkey, value_name)?;
                    backup_entries.push(BackupEntry::Registry {
                        hive: hive.as_short_str().to_string(),
                        subkey: subkey.to_string(),
                        value_name: value_name.to_string(),
                        previous,
                    });
                    targets_to_apply.push(TweakTargetSpec::Registry {
                        hive,
                        subkey,
                        value_name,
                        target: desired,
                        default: None,
                    });
                }
                TweakTargetSpec::Service {
                    service_name,
                    target_start_type,
                    ..
                } => {
                    let previous_start_type = win32::query_service_start_type(service_name)?;
                    backup_entries.push(BackupEntry::Service {
                        service_name: service_name.to_string(),
                        previous_start_type,
                    });
                    targets_to_apply.push(TweakTargetSpec::Service {
                        service_name,
                        target_start_type,
                        default_start_type: previous_start_type,
                    });
                }
            }
        }
    }

    let (id, created_at) = build_snapshot_id_and_time();
    let label = if defs.len() == 1 {
        format!("Before: {}", defs[0].title)
    } else {
        format!("Batch of {} tweaks", defs.len())
    };

    let snapshot = BackupSnapshot {
        id: id.clone(),
        created_at,
        label,
        entries: backup_entries,
    };

    save_snapshot(backup_dir, &snapshot)?;

    let mut applied_count = 0usize;
    for target in &targets_to_apply {
        let apply_res = match *target {
            TweakTargetSpec::Registry {
                hive,
                subkey,
                value_name,
                target: desired,
                ..
            } => win32::write_reg_value(hive, subkey, value_name, &desired.to_backup()),
            TweakTargetSpec::Service {
                service_name,
                target_start_type,
                ..
            } => win32::set_service_start_type(service_name, target_start_type),
        };

        if let Err(err) = apply_res {
            // Roll back already-applied entries in reverse order.
            for entry in snapshot.entries[..applied_count].iter().rev() {
                let _ = restore_backup_entry(entry);
            }
            if applied_count == 0 {
                let _ = fs::remove_file(snapshot_file_path(backup_dir, &snapshot.id));
            }
            return Err(err);
        }
        applied_count += 1;
    }

    Ok(snapshot)
}

fn find_latest_backup_for_registry(
    backups: &[BackupSnapshot],
    hive: RegHive,
    subkey: &str,
    value_name: &str,
) -> Option<RegValueBackup> {
    for snap in backups {
        for entry in &snap.entries {
            if let BackupEntry::Registry {
                hive: entry_hive,
                subkey: entry_subkey,
                value_name: entry_val,
                previous,
            } = entry
            {
                if RegHive::parse(entry_hive).ok() == Some(hive)
                    && entry_subkey.eq_ignore_ascii_case(subkey)
                    && entry_val.eq_ignore_ascii_case(value_name)
                {
                    return Some(previous.clone());
                }
            }
        }
    }
    None
}

fn find_latest_backup_for_service(backups: &[BackupSnapshot], service_name: &str) -> Option<u32> {
    for snap in backups {
        for entry in &snap.entries {
            if let BackupEntry::Service {
                service_name: entry_svc,
                previous_start_type,
            } = entry
            {
                if entry_svc.eq_ignore_ascii_case(service_name) {
                    return Some(*previous_start_type);
                }
            }
        }
    }
    None
}

/// Reverts the specified tweaks using the latest matching snapshot entry in `backup_dir`,
/// falling back to the catalog's documented Windows default if no snapshot entry exists.
pub fn revert_tweaks(tweak_ids: &[String], backup_dir: &Path) -> Result<(), AppError> {
    if tweak_ids.is_empty() {
        return Err(AppError::InvalidInput(
            "No tweak IDs specified to revert".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    let mut defs = Vec::with_capacity(tweak_ids.len());
    for id in tweak_ids {
        if seen.insert(id.as_str()) {
            defs.push(find_tweak_def(id)?);
        }
    }

    if defs.iter().any(|d| d.requires_admin) && !is_elevated() {
        return Err(AppError::ElevationRequired(
            "Reverting one or more selected tweaks requires administrator privileges".to_string(),
        ));
    }

    let backups = list_backups(backup_dir);
    for def in defs {
        for target in def.targets {
            match *target {
                TweakTargetSpec::Registry {
                    hive,
                    subkey,
                    value_name,
                    default,
                    ..
                } => {
                    let restore_val =
                        find_latest_backup_for_registry(&backups, hive, subkey, value_name)
                            .unwrap_or_else(|| match default {
                                Some(d) => d.to_backup(),
                                None => RegValueBackup::NotExisted,
                            });
                    win32::write_reg_value(hive, subkey, value_name, &restore_val)?;
                }
                TweakTargetSpec::Service {
                    service_name,
                    default_start_type,
                    ..
                } => {
                    let restore_start = find_latest_backup_for_service(&backups, service_name)
                        .unwrap_or(default_start_type);
                    win32::set_service_start_type(service_name, restore_start)?;
                }
            }
        }
    }

    Ok(())
}

/// Lists all valid saved `BackupSnapshot` files in `backup_dir`, sorted newest-first.
pub fn list_backups(backup_dir: &Path) -> Vec<BackupSnapshot> {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return Vec::new();
    };

    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(snapshot) = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<BackupSnapshot>(&raw).ok())
        {
            snapshots.push(snapshot);
        }
    }

    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
    snapshots
}

/// Restores every entry recorded in the specified snapshot, deleting newly created values that
/// previously had `RegValueBackup::NotExisted`.
pub fn restore_snapshot(snapshot_id: &str, backup_dir: &Path) -> Result<(), AppError> {
    let snapshot = load_snapshot(snapshot_id, backup_dir)?;

    if snapshot.entries.iter().any(entry_requires_admin) && !is_elevated() {
        return Err(AppError::ElevationRequired(
            "Restoring this snapshot requires administrator privileges for HKLM or Service entries"
                .to_string(),
        ));
    }

    for entry in snapshot.entries.iter().rev() {
        restore_backup_entry(entry)?;
    }

    Ok(())
}

fn escape_reg_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

fn format_reg_value_name(name: &str) -> String {
    if name.is_empty() {
        "@".to_string()
    } else {
        format!("\"{}\"", escape_reg_string(name))
    }
}

fn format_reg_qword_hex(val: u64) -> String {
    val.to_le_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Exports a saved `BackupSnapshot` to a Windows Registry Editor Version 5.00 `.reg` string.
/// `RegValueBackup::NotExisted` entries are emitted with `"ValueName"=-` deletion syntax.
pub fn export_snapshot_reg(snapshot_id: &str, backup_dir: &Path) -> Result<String, AppError> {
    let snapshot = load_snapshot(snapshot_id, backup_dir)?;

    let mut out = String::from("Windows Registry Editor Version 5.00\r\n");
    out.push_str(&format!(
        "; Snapshot: {} ({})\r\n",
        snapshot.id, snapshot.created_at
    ));

    // Group registry entries by full [HIVE\Subkey] header while preserving order.
    let mut section_order: Vec<String> = Vec::new();
    let mut section_lines: Vec<Vec<String>> = Vec::new();
    let mut service_notes: Vec<String> = Vec::new();

    for entry in &snapshot.entries {
        match entry {
            BackupEntry::Registry {
                hive,
                subkey,
                value_name,
                previous,
            } => {
                let parsed_hive = RegHive::parse(hive)?;
                let section = format!(r"{}\{subkey}", parsed_hive.as_reg_export_str());
                let key_token = format_reg_value_name(value_name);
                let line = match previous {
                    RegValueBackup::NotExisted => format!("{key_token}=-"),
                    RegValueBackup::Dword(v) => format!("{key_token}=dword:{v:08x}"),
                    RegValueBackup::Qword(v) => {
                        format!("{key_token}=hex(b):{}", format_reg_qword_hex(*v))
                    }
                    RegValueBackup::String(s) => {
                        format!("{key_token}=\"{}\"", escape_reg_string(s))
                    }
                };

                if let Some(pos) = section_order
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(&section))
                {
                    section_lines[pos].push(line);
                } else {
                    section_order.push(section);
                    section_lines.push(vec![line]);
                }
            }
            BackupEntry::Service {
                service_name,
                previous_start_type,
            } => {
                service_notes.push(format!(
                    "; Service: {service_name} -> previous StartType = {previous_start_type} ({})",
                    format_service_start_type(*previous_start_type)
                ));
            }
        }
    }

    for (section, lines) in section_order.iter().zip(section_lines.iter()) {
        out.push_str("\r\n[");
        out.push_str(section);
        out.push_str("]\r\n");
        for line in lines {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }

    if !service_notes.is_empty() {
        out.push_str("\r\n; Service startup states are managed via Windows Service Control Manager:\r\n");
        for note in service_notes {
            out.push_str(&note);
            out.push_str("\r\n");
        }
    }

    Ok(out)
}

// --- Separate AppX Bloatware Manager ---

const PROTECTED_APPX_PREFIXES: &[&str] = &[
    "microsoft.windowsstore",
    "microsoft.windowsterminal",
    "microsoft.desktopappinstaller",
    "microsoft.windowscalculator",
    "microsoft.windows.photos",
    "microsoft.vclibs",
    "microsoft.ui.xaml",
    "microsoft.net",
    "microsoft.windows.shellexperiencehost",
    "microsoft.windows.startmenuexperiencehost",
    "microsoft.windows.search",
    "microsoft.windows.sechealthui",
    "microsoft.aad.brokerplugin",
    "microsoft.accountscontrol",
    "microsoft.lockapp",
    "microsoft.bioenrollment",
    "microsoft.creddialoghost",
    "microsoft.ecapp",
    "microsoft.win32webviewhost",
    "microsoft.storepurchaseapp",
    "microsoft.services.store.engagement",
    "microsoft.windowsappruntime",
    "microsoftwindows.client",
    "windows.immersivecontrolpanel",
];

const KNOWN_BLOAT_APPX_PREFIXES: &[&str] = &[
    "microsoft.bingnews",
    "microsoft.bingweather",
    "microsoft.bingfinance",
    "microsoft.bingsports",
    "microsoft.gethelp",
    "microsoft.getstarted",
    "microsoft.microsoft3dviewer",
    "microsoft.microsoftofficehub",
    "microsoft.microsoftsolitairecollection",
    "microsoft.mixedreality.portal",
    "microsoft.people",
    "microsoft.skypeapp",
    "microsoft.todos",
    "microsoft.windowsfeedbackhub",
    "microsoft.windowsmaps",
    "microsoft.yourphone",
    "microsoft.zunemusic",
    "microsoft.zunevideo",
    "microsoft.549981c3f5f10",
    "microsoft.powerautomatedesktop",
    "microsoft.windowssoundrecorder",
    "microsoftteams",
    "msteams",
    "microsoftcorporationii.quickassist",
    "clipchamp.clipchamp",
    "spotifyab.spotifymusic",
    "disney.",
    "bytedancep",
    "king.com.",
];

/// Returns `true` if the package is protected and must never be classified as removable or uninstalled.
pub fn is_protected_appx_package(
    name: &str,
    package_full_name: &str,
    is_framework: bool,
    non_removable: bool,
) -> bool {
    if is_framework || non_removable {
        return true;
    }

    let name_lc = name.trim().to_ascii_lowercase();
    let full_lc = package_full_name.trim().to_ascii_lowercase();
    let full_prefix = full_lc.split('_').next().unwrap_or(&full_lc);

    PROTECTED_APPX_PREFIXES.iter().any(|&protected| {
        name_lc == protected
            || name_lc.starts_with(&format!("{protected}."))
            || full_prefix == protected
            || full_prefix.starts_with(&format!("{protected}."))
    })
}

fn is_known_appx_bloat(name: &str, package_full_name: &str) -> bool {
    let name_lc = name.trim().to_ascii_lowercase();
    let full_lc = package_full_name.trim().to_ascii_lowercase();
    let full_prefix = full_lc.split('_').next().unwrap_or(&full_lc);

    KNOWN_BLOAT_APPX_PREFIXES.iter().any(|&bloat| {
        name_lc.starts_with(bloat) || full_prefix.starts_with(bloat)
    })
}

fn validate_package_full_name(package_full_name: &str) -> Result<&str, AppError> {
    let trimmed = package_full_name.trim();
    if trimmed.is_empty() || !trimmed.contains('_') {
        return Err(AppError::InvalidInput(format!(
            "Invalid AppX PackageFullName: {package_full_name}"
        )));
    }
    let valid_chars = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if !valid_chars {
        return Err(AppError::InvalidInput(format!(
            "Unsafe characters in AppX PackageFullName: {package_full_name}"
        )));
    }
    Ok(trimmed)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawAppxRecord {
    name: Option<String>,
    package_full_name: Option<String>,
    publisher: Option<String>,
    is_framework: Option<bool>,
    non_removable: Option<bool>,
}

fn parse_appx_json_records(stdout: &str) -> Result<Vec<AppxPackageItem>, AppError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let val: serde_json::Value = serde_json::from_str(trimmed)?;
    let raw_list: Vec<RawAppxRecord> = match val {
        serde_json::Value::Array(_) => serde_json::from_value(val)?,
        serde_json::Value::Object(_) => vec![serde_json::from_value(val)?],
        _ => return Ok(Vec::new()),
    };

    let mut items = Vec::new();
    for rec in raw_list {
        let name = rec.name.unwrap_or_default();
        let package_full_name = rec.package_full_name.unwrap_or_default();
        let publisher = rec.publisher.unwrap_or_default();
        let is_framework = rec.is_framework.unwrap_or(false);
        let non_removable = rec.non_removable.unwrap_or(false);

        if name.is_empty() || package_full_name.is_empty() {
            continue;
        }

        if is_protected_appx_package(&name, &package_full_name, is_framework, non_removable) {
            continue;
        }

        let is_removable_bloat = is_known_appx_bloat(&name, &package_full_name);
        items.push(AppxPackageItem {
            name,
            package_full_name,
            publisher,
            is_removable_bloat,
        });
    }

    items.sort_by(|a, b| {
        b.is_removable_bloat
            .cmp(&a.is_removable_bloat)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });

    Ok(items)
}

#[cfg(windows)]
fn run_powershell_hidden(script: &str, env_vars: &[(&str, &str)]) -> Result<String, AppError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ])
    .creation_flags(CREATE_NO_WINDOW);

    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Win32(format!(
            "PowerShell command failed (exit {:?}): {stderr}",
            output.status.code()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(windows))]
fn run_powershell_hidden(_script: &str, _env_vars: &[(&str, &str)]) -> Result<String, AppError> {
    Err(AppError::Win32(
        "AppX management is only supported on Windows".to_string(),
    ))
}

/// Lists current-user AppX packages after strictly filtering out framework, non-removable,
/// and core protected Windows packages.
pub fn list_removable_appx() -> Result<Vec<AppxPackageItem>, AppError> {
    let script = concat!(
        "$ErrorActionPreference='Stop'; ",
        "Get-AppxPackage | Where-Object { -not $_.IsFramework -and -not $_.NonRemovable -and $_.SignatureKind -ne 'System' } ",
        "| Select-Object Name,PackageFullName,Publisher,IsFramework,NonRemovable ",
        "| ConvertTo-Json -Compress"
    );
    let stdout = run_powershell_hidden(script, &[])?;
    parse_appx_json_records(&stdout)
}

/// Uninstalls an AppX package for the current user only (`Remove-AppxPackage`),
/// rejecting protected packages and unsafe package identifiers.
pub fn remove_appx_current_user(package_full_name: &str) -> Result<(), AppError> {
    let clean_name = validate_package_full_name(package_full_name)?;
    let short_name = clean_name.split('_').next().unwrap_or(clean_name);

    if is_protected_appx_package(short_name, clean_name, false, false) {
        return Err(AppError::ProtectedTarget(format!(
            "Protected Windows AppX package cannot be removed: {clean_name}"
        )));
    }

    let script = "$ErrorActionPreference='Stop'; Remove-AppxPackage -Package $env:RTV_APPX_PKG";
    let _ = run_powershell_hidden(script, &[("RTV_APPX_PKG", clean_name)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_backup_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rtv-highperf-tweaker-{}-{}-{}-{}",
            tag,
            std::process::id(),
            nanos,
            seq
        ))
    }

    #[test]
    fn test_catalog_has_16_plus_tweaks_across_all_categories() {
        assert!(
            TWEAK_CATALOG.len() >= 16,
            "Expected 16+ tweaks, found {}",
            TWEAK_CATALOG.len()
        );
        for cat in [
            TweakCategory::LatencyGaming,
            TweakCategory::PrivacyTelemetry,
            TweakCategory::UiSnappiness,
            TweakCategory::BackgroundServices,
        ] {
            assert!(
                TWEAK_CATALOG.iter().any(|t| t.category == cat),
                "Missing tweak in category {cat:?}"
            );
        }

        let mut ids = HashSet::new();
        for tweak in TWEAK_CATALOG {
            assert!(ids.insert(tweak.id), "Duplicate tweak ID: {}", tweak.id);
        }
    }

    #[test]
    fn test_snapshot_records_missing_key_and_exports_reg() {
        let dir = unique_backup_dir("reg-export");
        let snapshot = BackupSnapshot {
            id: "snapshot-test-001".to_string(),
            created_at: "2026-11-22T15:30:00Z".to_string(),
            label: "Test missing key & dword".to_string(),
            entries: vec![
                BackupEntry::Registry {
                    hive: "HKCU".to_string(),
                    subkey: r"Software\Microsoft\Windows\CurrentVersion\Search".to_string(),
                    value_name: "BingSearchEnabled".to_string(),
                    previous: RegValueBackup::NotExisted,
                },
                BackupEntry::Registry {
                    hive: "HKCU".to_string(),
                    subkey: r"Control Panel\Desktop".to_string(),
                    value_name: "MenuShowDelay".to_string(),
                    previous: RegValueBackup::Dword(400),
                },
                BackupEntry::Registry {
                    hive: "HKCU".to_string(),
                    subkey: r"Control Panel\Mouse".to_string(),
                    value_name: "MouseSpeed".to_string(),
                    previous: RegValueBackup::String("1".to_string()),
                },
            ],
        };

        save_snapshot(&dir, &snapshot).expect("save_snapshot should succeed");

        let listed = list_backups(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], snapshot);

        let reg_text =
            export_snapshot_reg("snapshot-test-001", &dir).expect("export_snapshot_reg should work");

        assert!(reg_text.starts_with("Windows Registry Editor Version 5.00"));
        assert!(reg_text.contains(r"[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Search]"));
        assert!(reg_text.contains(r#""BingSearchEnabled"=-"#));
        assert!(reg_text.contains(r"[HKEY_CURRENT_USER\Control Panel\Desktop]"));
        assert!(reg_text.contains(r#""MenuShowDelay"=dword:00000190"#));
        assert!(reg_text.contains(r"[HKEY_CURRENT_USER\Control Panel\Mouse]"));
        assert!(reg_text.contains(r#""MouseSpeed"="1""#));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_appx_protected_allowlist() {
        let protected_cases = [
            (
                "Microsoft.WindowsStore",
                "Microsoft.WindowsStore_22409.1401.3.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.WindowsTerminal",
                "Microsoft.WindowsTerminal_1.21.2911.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.DesktopAppInstaller",
                "Microsoft.DesktopAppInstaller_1.24.25180.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.WindowsCalculator",
                "Microsoft.WindowsCalculator_11.2409.0.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.Windows.Photos",
                "Microsoft.Windows.Photos_2024.11100.16009.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.VCLibs.140.00",
                "Microsoft.VCLibs.140.00_14.0.33519.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.UI.Xaml.2.8",
                "Microsoft.UI.Xaml.2.8_8.2310.30001.0_x64__8wekyb3d8bbwe",
            ),
            (
                "Microsoft.NET.Native.Runtime.2.2",
                "Microsoft.NET.Native.Runtime.2.2_2.2.28604.0_x64__8wekyb3d8bbwe",
            ),
        ];

        for (name, full) in protected_cases {
            assert!(
                is_protected_appx_package(name, full, false, false),
                "{name} must be protected"
            );
            let err = remove_appx_current_user(full)
                .expect_err("Protected AppX removal must return ProtectedTarget error");
            assert!(
                matches!(err, AppError::ProtectedTarget(_)),
                "Expected ProtectedTarget, got {err:?}"
            );
        }

        // Known bloat should not be protected and should parse as removable bloat.
        let json = r#"[
            {
                "Name": "Microsoft.WindowsStore",
                "PackageFullName": "Microsoft.WindowsStore_22409.1401.3.0_x64__8wekyb3d8bbwe",
                "Publisher": "CN=Microsoft Corporation",
                "IsFramework": false,
                "NonRemovable": false
            },
            {
                "Name": "Microsoft.BingNews",
                "PackageFullName": "Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe",
                "Publisher": "CN=Microsoft Corporation",
                "IsFramework": false,
                "NonRemovable": false
            },
            {
                "Name": "Microsoft.WindowsTerminal",
                "PackageFullName": "Microsoft.WindowsTerminal_1.21.2911.0_x64__8wekyb3d8bbwe",
                "Publisher": "CN=Microsoft Corporation",
                "IsFramework": false,
                "NonRemovable": false
            },
            {
                "Name": "Microsoft.DesktopAppInstaller",
                "PackageFullName": "Microsoft.DesktopAppInstaller_1.24.25180.0_x64__8wekyb3d8bbwe",
                "Publisher": "CN=Microsoft Corporation",
                "IsFramework": false,
                "NonRemovable": false
            },
            {
                "Name": "Microsoft.WindowsCalculator",
                "PackageFullName": "Microsoft.WindowsCalculator_11.2409.0.0_x64__8wekyb3d8bbwe",
                "Publisher": "CN=Microsoft Corporation",
                "IsFramework": false,
                "NonRemovable": false
            }
        ]"#;

        let parsed = parse_appx_json_records(json).expect("JSON parse should succeed");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Microsoft.BingNews");
        assert!(parsed[0].is_removable_bloat);
    }
}
