use std::path::Path;

use crate::core::config::{default_config_dir, load_config, save_config};
use crate::core::safety::AppError;

/// Returns `true` if the current process token is elevated (Administrator).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let _guard = HandleGuard(token);

        let mut elevation = TOKEN_ELEVATION::default();
        let mut return_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_len,
        );

        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Saves `last_route` to `config.json` inside `cfg_dir` prior to an elevated relaunch.
pub fn persist_route_before_elevation(cfg_dir: &Path, last_route: &str) -> Result<(), AppError> {
    let mut cfg = load_config(cfg_dir)?;
    cfg.last_route = last_route.to_string();
    save_config(cfg_dir, &cfg)
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Saves `last_route` to config, launches an elevated instance of the current executable via
/// `ShellExecuteW("runas")`, and exits the current unprivileged process cleanly.
#[cfg(windows)]
pub fn restart_as_admin(last_route: &str) -> Result<(), AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let cfg_dir = default_config_dir();
    persist_route_before_elevation(&cfg_dir, last_route)?;

    let exe_path = std::env::current_exe()?;
    let exe_str = exe_path.to_str().ok_or_else(|| {
        AppError::InvalidInput("Current executable path is not valid UTF-8".to_string())
    })?;

    let verb_w = to_wide_null("runas");
    let exe_w = to_wide_null(exe_str);

    let hinst = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = hinst.0 as isize;
    if code <= 32 {
        return Err(AppError::Win32(format!(
            "ShellExecuteW(\"runas\") failed with code {code}"
        )));
    }

    std::process::exit(0);
}

#[cfg(not(windows))]
pub fn restart_as_admin(_last_route: &str) -> Result<(), AppError> {
    Err(AppError::Win32(
        "UAC elevation is only supported on Windows".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_is_elevated_does_not_panic() {
        // Simply verifies the Win32 token query executes cleanly without error or panic.
        let _elevated = is_elevated();
    }

    #[test]
    fn test_persist_route_before_elevation() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "rtv-highperf-priv-{}-{}",
            std::process::id(),
            nanos
        ));

        persist_route_before_elevation(&dir, "tweaker").expect("persist route should succeed");
        let loaded = load_config(&dir).expect("load_config should succeed");
        assert_eq!(loaded.last_route, "tweaker");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
