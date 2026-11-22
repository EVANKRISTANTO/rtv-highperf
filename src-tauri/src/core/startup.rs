#[cfg(windows)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::core::safety::AppError;

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_APPROVED_RUN_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const DISABLED_SUFFIX: &str = ".disabled";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Origin scope of a Windows startup entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupScope {
    HkcuRun,
    HklmRun,
    StartupFolder,
    ScheduledTask,
}

/// Represents a single startup program or scheduled task discovered on Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub scope: StartupScope,
    pub enabled: bool,
    pub publisher: Option<String>,
    pub requires_admin: bool,
}

/// Decodes Windows `StartupApproved\Run` binary flag bytes into an enabled/disabled boolean.
/// Empty or missing data defaults to enabled (`true`).
/// In Windows Explorer `StartupApproved` records, an even first byte (`0x02`, `0x00`) indicates
/// enabled, while an odd first byte (`0x03`, `0x01`) indicates disabled.
pub fn parse_startup_approved_enabled(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    (bytes[0] & 0x01) == 0
}

/// Updates `StartupApproved\Run` binary flag bytes for enable/disable while preserving the
/// existing byte buffer structure (typically 12 bytes).
pub fn set_startup_approved_bytes(existing: Option<&[u8]>, enable: bool) -> Vec<u8> {
    let mut buf = match existing {
        Some(orig) if !orig.is_empty() => orig.to_vec(),
        _ => vec![0u8; 12],
    };

    if buf.len() < 12 {
        buf.resize(12, 0);
    }

    buf[0] = if enable { 0x02 } else { 0x03 };
    buf
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn from_wide_lossy(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
struct RegKeyGuard(windows::Win32::System::Registry::HKEY);

#[cfg(windows)]
impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = windows::Win32::System::Registry::RegCloseKey(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn open_reg_key(
    root: windows::Win32::System::Registry::HKEY,
    subkey: &str,
    sam: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<Option<RegKeyGuard>, AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegOpenKeyExW, HKEY};

    let subkey_w = to_wide_null(subkey);
    let mut handle = HKEY::default();
    let status = unsafe { RegOpenKeyExW(root, PCWSTR(subkey_w.as_ptr()), 0, sam, &mut handle) };

    if status == ERROR_SUCCESS {
        Ok(Some(RegKeyGuard(handle)))
    } else if status == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else if status == ERROR_ACCESS_DENIED {
        Err(AppError::ElevationRequired(format!(
            "Administrator privileges required to access registry key '{subkey}'"
        )))
    } else {
        Err(AppError::Win32(format!(
            "RegOpenKeyExW('{subkey}') failed with error {}",
            status.0
        )))
    }
}

#[cfg(windows)]
fn create_reg_key_write(
    root: windows::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Result<RegKeyGuard, AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCreateKeyExW, HKEY, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
    };

    let subkey_w = to_wide_null(subkey);
    let mut handle = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut handle,
            None,
        )
    };

    if status == ERROR_SUCCESS {
        Ok(RegKeyGuard(handle))
    } else if status == ERROR_ACCESS_DENIED {
        Err(AppError::ElevationRequired(format!(
            "Administrator privileges required to modify registry key '{subkey}'"
        )))
    } else {
        Err(AppError::Win32(format!(
            "RegCreateKeyExW('{subkey}') failed with error {}",
            status.0
        )))
    }
}

#[cfg(windows)]
fn reg_value_exists(hkey: windows::Win32::System::Registry::HKEY, value_name: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows::Win32::System::Registry::RegQueryValueExW;

    let name_w = to_wide_null(value_name);
    let mut len = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            None,
            None,
            Some(&mut len),
        )
    };
    status == ERROR_SUCCESS || status == ERROR_MORE_DATA
}

#[cfg(windows)]
fn query_reg_binary(
    hkey: windows::Win32::System::Registry::HKEY,
    value_name: &str,
) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegQueryValueExW, REG_VALUE_TYPE};

    let name_w = to_wide_null(value_name);
    let mut val_type = REG_VALUE_TYPE::default();
    let mut len = 0u32;

    let status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut val_type),
            None,
            Some(&mut len),
        )
    };
    if status != ERROR_SUCCESS || len == 0 {
        return None;
    }

    let mut buf = vec![0u8; len as usize];
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut len),
        )
    };
    if status == ERROR_SUCCESS {
        buf.truncate(len as usize);
        Some(buf)
    } else {
        None
    }
}

#[cfg(windows)]
fn read_startup_approved_map(
    root: windows::Win32::System::Registry::HKEY,
) -> HashMap<String, Vec<u8>> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegEnumValueW, KEY_READ};

    let mut map = HashMap::new();
    let Ok(Some(guard)) = open_reg_key(root, STARTUP_APPROVED_RUN_SUBKEY, KEY_READ) else {
        return map;
    };

    let mut index = 0u32;
    loop {
        let mut name_buf = vec![0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut data_buf = vec![0u8; 64];
        let mut data_len = data_buf.len() as u32;
        let mut val_type = 0u32;

        let status = unsafe {
            RegEnumValueW(
                guard.0,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                Some(&mut val_type),
                Some(data_buf.as_mut_ptr()),
                Some(&mut data_len),
            )
        };

        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS {
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            data_buf.truncate(data_len as usize);
            map.insert(name.to_ascii_lowercase(), data_buf);
        } else if status == ERROR_MORE_DATA {
            // Unusually large buffer; query directly after reading name.
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            if let Some(bytes) = query_reg_binary(guard.0, &name) {
                map.insert(name.to_ascii_lowercase(), bytes);
            }
        } else {
            break;
        }

        index += 1;
    }

    map
}

#[cfg(windows)]
fn read_registry_run_scope(
    root: windows::Win32::System::Registry::HKEY,
    scope: StartupScope,
    id_prefix: &str,
    requires_admin: bool,
) -> Vec<StartupItem> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegEnumValueW, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };

    let Ok(Some(guard)) = open_reg_key(root, RUN_SUBKEY, KEY_READ) else {
        return Vec::new();
    };
    let approved_map = read_startup_approved_map(root);

    let mut items = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = vec![0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut data_buf = vec![0u8; 2048];
        let mut data_len = data_buf.len() as u32;
        let mut val_type = 0u32;

        let status = unsafe {
            RegEnumValueW(
                guard.0,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                Some(&mut val_type),
                Some(data_buf.as_mut_ptr()),
                Some(&mut data_len),
            )
        };

        if status == ERROR_NO_MORE_ITEMS {
            break;
        }

        if status == ERROR_MORE_DATA {
            // Re-run with larger data buffer.
            let mut big_name = vec![0u16; 512];
            let mut big_name_len = big_name.len() as u32;
            let mut big_data = vec![0u8; (data_len as usize).max(4096)];
            let mut big_data_len = big_data.len() as u32;
            let retry = unsafe {
                RegEnumValueW(
                    guard.0,
                    index,
                    PWSTR(big_name.as_mut_ptr()),
                    &mut big_name_len,
                    None,
                    Some(&mut val_type),
                    Some(big_data.as_mut_ptr()),
                    Some(&mut big_data_len),
                )
            };
            if retry == ERROR_SUCCESS {
                name_buf = big_name;
                name_len = big_name_len;
                data_buf = big_data;
                data_len = big_data_len;
            } else {
                index += 1;
                continue;
            }
        } else if status != ERROR_SUCCESS {
            break;
        }

        let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        if !name.is_empty() && (val_type == REG_SZ.0 || val_type == REG_EXPAND_SZ.0) {
            let u16_slice: Vec<u16> = data_buf[..data_len as usize]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_le_bytes(c))
                .collect();
            let command = from_wide_lossy(&u16_slice).trim().to_string();
            let enabled = approved_map
                .get(&name.to_ascii_lowercase())
                .map(|bytes| parse_startup_approved_enabled(bytes))
                .unwrap_or(true);

            items.push(StartupItem {
                id: format!("{id_prefix}:{name}"),
                name,
                command,
                scope,
                enabled,
                publisher: None,
                requires_admin,
            });
        }

        index += 1;
    }

    items
}

#[cfg(not(windows))]
fn read_registry_run_scope(
    _root: (),
    _scope: StartupScope,
    _id_prefix: &str,
    _requires_admin: bool,
) -> Vec<StartupItem> {
    Vec::new()
}

fn user_startup_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(|appdata| {
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup")
        })
}

fn common_startup_folder() -> Option<PathBuf> {
    std::env::var_os("ProgramData")
        .filter(|v| !v.is_empty())
        .map(|pd| {
            PathBuf::from(pd)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup")
        })
}

fn read_startup_folder(dir: &Path, folder_tag: &str, requires_admin: bool) -> Vec<StartupItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }

        let lower = file_name.to_ascii_lowercase();
        let (base_name, enabled) = if lower.ends_with(DISABLED_SUFFIX) {
            let trimmed_len = file_name.len() - DISABLED_SUFFIX.len();
            (file_name[..trimmed_len].to_string(), false)
        } else {
            (file_name.clone(), true)
        };

        items.push(StartupItem {
            id: format!("folder:{folder_tag}:{base_name}"),
            name: base_name,
            command: entry.path().to_string_lossy().into_owned(),
            scope: StartupScope::StartupFolder,
            enabled,
            publisher: None,
            requires_admin,
        });
    }

    items
}

/// Parses a single `schtasks /Query /FO CSV /V` line.
/// `schtasks` wraps fields in `"` and separates them with `","`, but can emit unescaped
/// quotes inside `Task To Run` (e.g. `""C:\Program Files\App\app.exe" --arg"`).
fn parse_schtasks_csv_row(line: &str) -> Vec<String> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let inner = trimmed
        .strip_prefix('"')
        .unwrap_or(trimmed)
        .strip_suffix('"')
        .unwrap_or(trimmed);
    inner.split("\",\"").map(str::to_string).collect()
}

fn is_microsoft_task(task_name: &str, author: &str) -> bool {
    let name_lower = task_name.to_ascii_lowercase();
    let author_lower = author.to_ascii_lowercase();

    name_lower.starts_with(r"\microsoft\")
        || author_lower.contains("microsoft")
}

fn parse_schtasks_csv(csv_text: &str) -> Vec<StartupItem> {
    let mut lines = csv_text.lines().filter(|l| !l.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Vec::new();
    };

    let headers = parse_schtasks_csv_row(header_line);
    let idx = |col_name: &str| -> Option<usize> {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(col_name))
    };

    let Some(col_task_name) = idx("TaskName") else {
        return Vec::new();
    };
    let col_status = idx("Status");
    let col_author = idx("Author");
    let col_task_to_run = idx("Task To Run");
    let col_state = idx("Scheduled Task State");
    let col_run_as = idx("Run As User");

    let mut seen_tasks = HashSet::new();
    let mut items = Vec::new();

    for line in lines {
        let fields = parse_schtasks_csv_row(line);
        if fields.len() < headers.len() {
            continue;
        }
        // Skip repeated header lines that localized/multi-block schtasks can emit.
        if fields[col_task_name].eq_ignore_ascii_case("TaskName") {
            continue;
        }

        let task_name = fields[col_task_name].trim();
        if task_name.is_empty() {
            continue;
        }
        let author = col_author
            .and_then(|i| fields.get(i))
            .map(|s| s.trim())
            .unwrap_or("");

        if is_microsoft_task(task_name, author) {
            continue;
        }

        let dedup_key = task_name.to_ascii_lowercase();
        if !seen_tasks.insert(dedup_key) {
            continue;
        }

        let command = col_task_to_run
            .and_then(|i| fields.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let state_str = col_state
            .and_then(|i| fields.get(i))
            .map(|s| s.trim())
            .unwrap_or("");
        let status_str = col_status
            .and_then(|i| fields.get(i))
            .map(|s| s.trim())
            .unwrap_or("");
        let enabled = !state_str.eq_ignore_ascii_case("Disabled")
            && !status_str.eq_ignore_ascii_case("Disabled");

        let run_as = col_run_as
            .and_then(|i| fields.get(i))
            .map(|s| s.trim())
            .unwrap_or("");
        let run_as_lower = run_as.to_ascii_lowercase();
        let requires_admin = run_as_lower == "system"
            || run_as_lower == "local service"
            || run_as_lower == "network service"
            || run_as_lower.contains("administrator");

        let publisher = if author.is_empty() || author.eq_ignore_ascii_case("N/A") {
            None
        } else {
            Some(author.to_string())
        };

        let display_name = task_name
            .rsplit('\\')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(task_name)
            .to_string();

        items.push(StartupItem {
            id: format!("task:{task_name}"),
            name: display_name,
            command,
            scope: StartupScope::ScheduledTask,
            enabled,
            publisher,
            requires_admin,
        });
    }

    items
}

#[cfg(windows)]
fn read_scheduled_tasks() -> Vec<StartupItem> {
    use std::os::windows::process::CommandExt;

    let Ok(output) = Command::new("schtasks")
        .args(["/Query", "/FO", "CSV", "/V"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let csv_text = String::from_utf8_lossy(&output.stdout);
    parse_schtasks_csv(&csv_text)
}

#[cfg(not(windows))]
fn read_scheduled_tasks() -> Vec<StartupItem> {
    Vec::new()
}

/// Lists Windows startup items across HKCU Run, HKLM Run, User/Common Startup folders,
/// and non-Microsoft Scheduled Tasks.
pub fn list_startup_items() -> Vec<StartupItem> {
    let mut items = Vec::new();

    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        items.extend(read_registry_run_scope(
            HKEY_CURRENT_USER,
            StartupScope::HkcuRun,
            "hkcu",
            false,
        ));
        items.extend(read_registry_run_scope(
            HKEY_LOCAL_MACHINE,
            StartupScope::HklmRun,
            "hklm",
            true,
        ));
    }

    if let Some(dir) = user_startup_folder() {
        items.extend(read_startup_folder(&dir, "user", false));
    }
    if let Some(dir) = common_startup_folder() {
        items.extend(read_startup_folder(&dir, "common", true));
    }

    items.extend(read_scheduled_tasks());

    items.sort_by_key(|a| a.name.to_ascii_lowercase());
    items
}

#[cfg(windows)]
fn toggle_registry_startup_item(
    root: windows::Win32::System::Registry::HKEY,
    value_name: &str,
    enable: bool,
) -> Result<(), AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{RegSetValueExW, KEY_READ, REG_BINARY};

    // Verify the startup entry exists in Run first.
    let run_key = open_reg_key(root, RUN_SUBKEY, KEY_READ)?.ok_or_else(|| {
        AppError::InvalidInput(format!("Registry Run key not found for '{value_name}'"))
    })?;
    if !reg_value_exists(run_key.0, value_name) {
        return Err(AppError::InvalidInput(format!(
            "Startup entry '{value_name}' does not exist in Run key"
        )));
    }

    let approved_key = create_reg_key_write(root, STARTUP_APPROVED_RUN_SUBKEY)?;
    let existing = query_reg_binary(approved_key.0, value_name);
    let updated = set_startup_approved_bytes(existing.as_deref(), enable);

    let name_w = to_wide_null(value_name);
    let status = unsafe {
        RegSetValueExW(
            approved_key.0,
            PCWSTR(name_w.as_ptr()),
            0,
            REG_BINARY,
            Some(&updated),
        )
    };

    if status == ERROR_SUCCESS {
        Ok(())
    } else if status == ERROR_ACCESS_DENIED {
        Err(AppError::ElevationRequired(format!(
            "Administrator privileges required to modify startup item '{value_name}'"
        )))
    } else {
        Err(AppError::Win32(format!(
            "RegSetValueExW('{value_name}') failed with error {}",
            status.0
        )))
    }
}

fn toggle_startup_folder_item(
    folder_tag: &str,
    base_name: &str,
    enable: bool,
) -> Result<(), AppError> {
    if base_name.is_empty()
        || base_name.contains(['\\', '/', ':'])
        || base_name.contains("..")
    {
        return Err(AppError::InvalidInput(format!(
            "Invalid startup file name '{base_name}'"
        )));
    }

    let dir = match folder_tag {
        "user" => user_startup_folder().ok_or_else(|| {
            AppError::Io("APPDATA environment variable is not set".to_string())
        })?,
        "common" => common_startup_folder().ok_or_else(|| {
            AppError::Io("ProgramData environment variable is not set".to_string())
        })?,
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unknown startup folder scope '{folder_tag}'"
            )))
        }
    };

    let enabled_path = dir.join(base_name);
    let disabled_path = dir.join(format!("{base_name}{DISABLED_SUFFIX}"));

    let (src, dst) = if enable {
        if enabled_path.exists() {
            return Ok(());
        }
        (disabled_path, enabled_path)
    } else {
        if disabled_path.exists() {
            return Ok(());
        }
        (enabled_path, disabled_path)
    };

    if !src.exists() {
        return Err(AppError::InvalidInput(format!(
            "Startup folder item '{}' not found",
            src.display()
        )));
    }

    match std::fs::rename(&src, &dst) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(AppError::ElevationRequired(format!(
                "Administrator privileges required to modify '{}'",
                src.display()
            )))
        }
        Err(err) => Err(AppError::from(err)),
    }
}

#[cfg(windows)]
fn toggle_scheduled_task(task_name: &str, enable: bool) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;

    if task_name.trim().is_empty() {
        return Err(AppError::InvalidInput("Empty scheduled task name".to_string()));
    }

    let action_flag = if enable { "/ENABLE" } else { "/DISABLE" };
    let output = Command::new("schtasks")
        .args(["/Change", "/TN", task_name, action_flag])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr} {stdout}").trim().to_string();
    let lower = combined.to_ascii_lowercase();

    if lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("0x80070005")
    {
        return Err(AppError::ElevationRequired(format!(
            "Administrator privileges required to toggle scheduled task '{task_name}'"
        )));
    }

    Err(AppError::Win32(format!(
        "schtasks /Change failed for '{task_name}': {combined}"
    )))
}

#[cfg(not(windows))]
fn toggle_scheduled_task(_task_name: &str, _enable: bool) -> Result<(), AppError> {
    Err(AppError::Win32(
        "Scheduled tasks are only supported on Windows".to_string(),
    ))
}

/// Enables or disables a startup item identified by its `StartupItem::id`.
/// Returns `AppError::ElevationRequired` cleanly if modifying HKLM, Common Startup, or a
/// protected Scheduled Task fails with access denied while unprivileged.
pub fn toggle_startup_item(id: &str, enable: bool) -> Result<(), AppError> {
    if let Some(name) = id.strip_prefix("hkcu:") {
        #[cfg(windows)]
        {
            use windows::Win32::System::Registry::HKEY_CURRENT_USER;
            return toggle_registry_startup_item(HKEY_CURRENT_USER, name, enable);
        }
        #[cfg(not(windows))]
        {
            let _ = (name, enable);
            return Err(AppError::Win32(
                "Registry startup items are only supported on Windows".to_string(),
            ));
        }
    }

    if let Some(name) = id.strip_prefix("hklm:") {
        #[cfg(windows)]
        {
            use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
            return toggle_registry_startup_item(HKEY_LOCAL_MACHINE, name, enable);
        }
        #[cfg(not(windows))]
        {
            let _ = (name, enable);
            return Err(AppError::Win32(
                "Registry startup items are only supported on Windows".to_string(),
            ));
        }
    }

    if let Some(rest) = id.strip_prefix("folder:") {
        let Some((folder_tag, base_name)) = rest.split_once(':') else {
            return Err(AppError::InvalidInput(format!(
                "Malformed startup folder id '{id}'"
            )));
        };
        return toggle_startup_folder_item(folder_tag, base_name, enable);
    }

    if let Some(task_name) = id.strip_prefix("task:") {
        return toggle_scheduled_task(task_name, enable);
    }

    Err(AppError::InvalidInput(format!(
        "Unknown startup item id '{id}'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_approved_byte_mask() {
        // Missing / empty defaults to enabled.
        assert!(parse_startup_approved_enabled(&[]));

        // Standard Windows StartupApproved enabled/disabled masks.
        let enabled_bytes = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let disabled_bytes = [0x03, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0x11, 0x22, 0x33, 0x44];
        assert!(parse_startup_approved_enabled(&enabled_bytes));
        assert!(!parse_startup_approved_enabled(&disabled_bytes));

        // Enabling preserves trailing timestamp/structure bytes while flipping flag to 0x02.
        let re_enabled = set_startup_approved_bytes(Some(&disabled_bytes), true);
        assert_eq!(re_enabled.len(), 12);
        assert_eq!(re_enabled[0], 0x02);
        assert_eq!(&re_enabled[1..], &disabled_bytes[1..]);
        assert!(parse_startup_approved_enabled(&re_enabled));

        // Disabling flips flag to 0x03 and preserves trailing bytes.
        let re_disabled = set_startup_approved_bytes(Some(&re_enabled), false);
        assert_eq!(re_disabled[0], 0x03);
        assert_eq!(&re_disabled[1..], &disabled_bytes[1..]);
        assert!(!parse_startup_approved_enabled(&re_disabled));

        // Creating from None produces a valid 12-byte StartupApproved payload.
        let fresh_disabled = set_startup_approved_bytes(None, false);
        assert_eq!(fresh_disabled, vec![0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(!parse_startup_approved_enabled(&fresh_disabled));
    }

    #[test]
    fn test_schtasks_csv_parser_filters_microsoft_and_parses_quotes() {
        let sample = concat!(
            "\"HostName\",\"TaskName\",\"Status\",\"Author\",\"Task To Run\",\"Scheduled Task State\",\"Run As User\"\n",
            "\"HOST\",\"\\C__Program Files_HeidiSQL_heidisql.exe\",\"Ready\",\"HeidiSQL 12.21\",\"\"C:\\Program Files\\HeidiSQL\\heidisql.exe\" --runfrom=scheduler\",\"Enabled\",\"Administrator\"\n",
            "\"HOST\",\"\\Microsoft\\Windows\\Defrag\\ScheduledDefrag\",\"Ready\",\"Microsoft Corporation\",\"defrag.exe\",\"Enabled\",\"SYSTEM\"\n",
            "\"HOST\",\"\\UserApp\\Updater\",\"Disabled\",\"Acme Corp\",\"C:\\Tools\\updater.exe\",\"Disabled\",\"User\"\n",
            "\"HOST\",\"\\UserApp\\Updater\",\"Disabled\",\"Acme Corp\",\"C:\\Tools\\updater.exe\",\"Disabled\",\"User\"\n"
        );

        let items = parse_schtasks_csv(sample);
        assert_eq!(items.len(), 2);

        let heidi = &items[0];
        assert_eq!(heidi.id, r"task:\C__Program Files_HeidiSQL_heidisql.exe");
        assert_eq!(heidi.command, r#""C:\Program Files\HeidiSQL\heidisql.exe" --runfrom=scheduler"#);
        assert!(heidi.enabled);
        assert!(heidi.requires_admin);
        assert_eq!(heidi.publisher.as_deref(), Some("HeidiSQL 12.21"));

        let updater = &items[1];
        assert_eq!(updater.id, r"task:\UserApp\Updater");
        assert_eq!(updater.name, "Updater");
        assert!(!updater.enabled);
        assert!(!updater.requires_admin);
    }

    #[test]
    fn test_startup_folder_toggle_roundtrip() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "rtv-highperf-startup-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).expect("create temp startup dir");

        let lnk = dir.join("SampleApp.lnk");
        std::fs::write(&lnk, b"shortcut").expect("write sample shortcut");
        std::fs::write(dir.join("desktop.ini"), b"[LocalizedFileNames]").expect("write desktop.ini");

        let initial = read_startup_folder(&dir, "user", false);
        assert_eq!(initial.len(), 1);
        assert!(initial[0].enabled);
        assert_eq!(initial[0].id, "folder:user:SampleApp.lnk");

        let disabled_path = dir.join("SampleApp.lnk.disabled");
        std::fs::rename(&lnk, &disabled_path).expect("rename to disabled");

        let after_disable = read_startup_folder(&dir, "user", false);
        assert_eq!(after_disable.len(), 1);
        assert!(!after_disable[0].enabled);
        assert_eq!(after_disable[0].id, "folder:user:SampleApp.lnk");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
