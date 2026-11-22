use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::core::safety::AppError;

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GovernorProfile {
    #[default]
    Off,
    BalancedWork,
    GamingFocus,
    CodingCompile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyPowerState {
    pub original_guid: String,
    pub set_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: String,
    pub active_profile: GovernorProfile,
    pub governor_enabled: bool,
    pub auto_start: bool,
    pub auto_start_elevated: bool,
    pub minimize_to_tray: bool,
    pub unload_webview_on_tray: bool,
    pub ram_trim_threshold_pct: u8,
    pub exclusions: Vec<String>,
    pub dirty_power_scheme: Option<DirtyPowerState>,
    pub last_route: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            active_profile: GovernorProfile::Off,
            governor_enabled: false,
            auto_start: false,
            auto_start_elevated: false,
            minimize_to_tray: true,
            unload_webview_on_tray: false,
            ram_trim_threshold_pct: 80,
            exclusions: Vec::new(),
            dirty_power_scheme: None,
            last_route: "detective".to_string(),
        }
    }
}

/// Returns the default configuration directory `%APPDATA%\rtv-highperf`.
pub fn default_config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("rtv-highperf");
        }
    }

    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        if !user_profile.is_empty() {
            return PathBuf::from(user_profile)
                .join("AppData")
                .join("Roaming")
                .join("rtv-highperf");
        }
    }

    std::env::temp_dir().join("rtv-highperf")
}

/// Returns the path to `config.json` inside `dir`.
pub fn config_file_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Loads `config.json` from `dir`, returning `AppConfig::default()` if the file does not exist yet.
pub fn load_config(dir: &Path) -> Result<AppConfig, AppError> {
    let path = config_file_path(dir);
    match fs::read_to_string(&path) {
        Ok(raw) => Ok(serde_json::from_str::<AppConfig>(&raw)?),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(err) => Err(AppError::from(err)),
    }
}

/// Atomically persists `cfg` to `dir/config.json` via a temporary file and rename.
pub fn save_config(dir: &Path, cfg: &AppConfig) -> Result<(), AppError> {
    fs::create_dir_all(dir)?;

    let target = config_file_path(dir);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!("config.json.tmp.{}.{}", std::process::id(), seq));
    let json = serde_json::to_string_pretty(cfg)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rtv-highperf-cfg-{}-{}-{}-{}",
            tag,
            std::process::id(),
            nanos,
            seq
        ))
    }

    #[test]
    fn test_config_roundtrip_and_dirty_power_state() {
        let dir = unique_test_dir("roundtrip");

        // Loading from a non-existent directory yields sensible defaults.
        let initial = load_config(&dir).expect("missing config should yield default");
        assert_eq!(initial, AppConfig::default());

        let custom = AppConfig {
            theme: "light".to_string(),
            active_profile: GovernorProfile::GamingFocus,
            governor_enabled: true,
            auto_start: true,
            auto_start_elevated: false,
            minimize_to_tray: false,
            unload_webview_on_tray: true,
            ram_trim_threshold_pct: 75,
            exclusions: vec!["discord.exe".to_string(), "obs64.exe".to_string()],
            dirty_power_scheme: Some(DirtyPowerState {
                original_guid: "381b4222-f694-41f0-9685-ff5bb260df2e".to_string(),
                set_at_ms: 1_763_820_000_000,
            }),
            last_route: "optimizer".to_string(),
        };

        save_config(&dir, &custom).expect("save_config should succeed");
        let loaded = load_config(&dir).expect("load_config should succeed");
        assert_eq!(loaded, custom);

        // Overwriting atomically updates the existing file without leaving temp artifacts.
        let mut cleared = loaded;
        cleared.dirty_power_scheme = None;
        cleared.governor_enabled = false;
        save_config(&dir, &cleared).expect("second save_config should overwrite atomically");

        let reloaded = load_config(&dir).expect("reload should succeed");
        assert_eq!(reloaded, cleared);

        let leftover_tmp = fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover_tmp, "no temp file should remain after rename");

        let _ = fs::remove_dir_all(&dir);
    }
}
