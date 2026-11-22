use std::path::{Component, Path};

use serde::ser::SerializeStruct;
use serde::Serialize;
use thiserror::Error;

/// Structured error type shared across core modules and Tauri IPC commands.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    #[error("Elevation required: {0}")]
    ElevationRequired(String),
    #[error("Protected target blocked: {0}")]
    ProtectedTarget(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Win32 API error: {0}")]
    Win32(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl AppError {
    /// Returns the discriminant name used in serialized IPC payloads.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ElevationRequired(_) => "ElevationRequired",
            Self::ProtectedTarget(_) => "ProtectedTarget",
            Self::Io(_) => "Io",
            Self::Win32(_) => "Win32",
            Self::InvalidInput(_) => "InvalidInput",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::InvalidInput(err.to_string())
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for AppError {
    fn from(err: windows::core::Error) -> Self {
        Self::Win32(err.to_string())
    }
}

const PROTECTED_PROCESSES: &[&str] = &[
    "system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "dwm.exe",
    "audiodg.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "securityhealthservice.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "wmiprvse.exe",
    "rtv-highperf.exe",
    "easyanticheat.exe",
    "beservice.exe",
    "vgtray.exe",
    "vgc.exe",
];

/// Returns true if a process must never be throttled, lowered in priority, or RAM-trimmed.
pub fn is_protected_process(name: &str, pid: u32, self_pid: u32) -> bool {
    if pid <= 4 || pid == self_pid {
        return true;
    }

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Extract basename in case a full path is supplied.
    let base = trimmed
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(trimmed)
        .to_ascii_lowercase();

    PROTECTED_PROCESSES.contains(&base.as_str())
}

/// Extracts lowercase normal path segments from an absolute Windows path,
/// returning `None` if the path contains `..`, is relative, or has no normal segments.
fn extract_normalized_segments(path: &Path) -> Option<Vec<String>> {
    let mut has_root = false;
    let mut segments = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => return None,
            Component::CurDir => return None,
            Component::Prefix(_) => {}
            Component::RootDir => {
                has_root = true;
            }
            Component::Normal(part) => {
                let s = part.to_string_lossy().to_ascii_lowercase();
                if !s.is_empty() {
                    segments.push(s);
                }
            }
        }
    }

    if !has_root || segments.is_empty() {
        return None;
    }

    // Strip verbatim prefix marker if present on drive-based paths (e.g. `\\?\c:\...`).
    if segments.first().map(String::as_str) == Some("?") {
        segments.remove(0);
    }

    if segments.is_empty() {
        return None;
    }

    Some(segments)
}

fn is_explicitly_denied_segments(segs: &[String]) -> bool {
    if segs.is_empty() {
        return true;
    }

    let first = segs[0].as_str();
    if first == "program files" || first == "program files (x86)" {
        return true;
    }

    if first == "windows" {
        if segs.len() == 1 {
            return true;
        }
        let second = segs[1].as_str();
        if matches!(
            second,
            "system32" | "syswow64" | "winsxs" | "boot" | "fonts" | "inf" | "servicing"
        ) {
            return true;
        }
    }

    if first == "users" {
        if segs.len() <= 2 {
            return true;
        }
        if matches!(
            segs[2].as_str(),
            "documents" | "desktop" | "pictures" | "downloads" | "videos" | "music" | "onedrive"
        ) {
            return true;
        }
    }

    false
}

fn is_under_system_temp_dir(segs: &[String]) -> bool {
    let temp = std::env::temp_dir();
    if let Some(temp_segs) = extract_normalized_segments(&temp) {
        if !is_explicitly_denied_segments(&temp_segs) && segs.starts_with(&temp_segs) {
            return true;
        }
    }
    if let Ok(canon) = std::fs::canonicalize(&temp) {
        if let Some(canon_segs) = extract_normalized_segments(&canon) {
            if !is_explicitly_denied_segments(&canon_segs) && segs.starts_with(&canon_segs) {
                return true;
            }
        }
    }
    false
}

fn is_known_safe_windows_clean_root(segs: &[String]) -> bool {
    let strs: Vec<&str> = segs.iter().map(String::as_str).collect();

    // Windows system temp, update download cache, crash minidumps, live kernel reports.
    if strs.starts_with(&["windows", "temp"])
        || strs.starts_with(&["windows", "softwaredistribution", "download"])
        || strs.starts_with(&["windows", "minidump"])
        || strs.starts_with(&["windows", "livekernelreports"])
    {
        return true;
    }

    // User profile temp / cache / dump / opt-in developer cache roots.
    if strs.len() >= 4 && strs[0] == "users" {
        let rest = &strs[2..];
        if rest.starts_with(&["appdata", "local", "temp"])
            || rest.starts_with(&["appdata", "local", "crashdumps"])
            || rest.starts_with(&["appdata", "local", "d3dscache"])
            || rest.starts_with(&["appdata", "local", "nvidia", "dxcache"])
            || rest.starts_with(&["appdata", "local", "nvidia", "glcache"])
            || rest.starts_with(&["appdata", "locallow", "nvidia", "dxcache"])
            || rest.starts_with(&["appdata", "local", "amd", "dxcache"])
            || rest.starts_with(&["appdata", "local", "amd", "glcache"])
            || rest.starts_with(&["appdata", "local", "amd", "vkcache"])
            || rest.starts_with(&["appdata", "local", "microsoft", "windows", "wer"])
            || rest.starts_with(&["appdata", "local", "microsoft", "windows", "inetcache"])
            || rest.starts_with(&["appdata", "local", "npm-cache"])
            || rest.starts_with(&["appdata", "roaming", "npm-cache"])
            || rest.starts_with(&["appdata", "local", "pnpm", "store"])
            || rest.starts_with(&["appdata", "local", "pnpm-cache"])
            || rest.starts_with(&["appdata", "local", "pip", "cache"])
            || rest.starts_with(&[".cargo", "registry", "cache"])
        {
            return true;
        }

        // Browser cache directories under User Data / Profiles.
        if (rest.starts_with(&["appdata", "local", "google", "chrome", "user data"])
            || rest.starts_with(&["appdata", "local", "microsoft", "edge", "user data"])
            || rest.starts_with(&[
                "appdata",
                "local",
                "bravesoftware",
                "brave-browser",
                "user data",
            ]))
            && matches!(
                rest.last().copied(),
                Some("cache" | "code cache" | "gpucache")
            )
        {
            return true;
        }

        if rest.starts_with(&["appdata", "local", "mozilla", "firefox", "profiles"])
            && rest.contains(&"cache2")
        {
            return true;
        }
    }

    false
}

/// Validates that a directory path is strictly within an approved, safe cleaning root.
/// Rejects relative paths, `..` traversal, drive roots, system directories, and personal folders.
pub fn is_allowed_clean_root(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.is_empty() || raw.contains("..") {
        return false;
    }

    let Some(segs) = extract_normalized_segments(path) else {
        return false;
    };

    if is_explicitly_denied_segments(&segs) {
        return false;
    }

    is_under_system_temp_dir(&segs) || is_known_safe_windows_clean_root(&segs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_protected_processes_blocked() {
        let self_pid = 4242;
        assert!(is_protected_process("System", 4, self_pid));
        assert!(is_protected_process("anything.exe", 0, self_pid));
        assert!(is_protected_process("my_clone.exe", self_pid, self_pid));

        assert!(is_protected_process("csrss.exe", 512, self_pid));
        assert!(is_protected_process("LSASS.EXE", 780, self_pid));
        assert!(is_protected_process("dwm.exe", 1100, self_pid));
        assert!(is_protected_process("MsMpEng.exe", 2200, self_pid));
        assert!(is_protected_process("svchost.exe", 900, self_pid));
        assert!(is_protected_process("rtv-highperf.exe", 9999, self_pid));
        assert!(is_protected_process("EasyAntiCheat.exe", 3333, self_pid));
        assert!(is_protected_process("vgc.exe", 4444, self_pid));
        assert!(is_protected_process(
            r"C:\Windows\System32\audiodg.exe",
            1500,
            self_pid
        ));

        assert!(!is_protected_process("chrome.exe", 8120, self_pid));
        assert!(!is_protected_process("notepad.exe", 9100, self_pid));
    }

    #[test]
    fn test_clean_root_guards() {
        // Rejected system, user personal, traversal, and drive root paths.
        assert!(!is_allowed_clean_root(Path::new(r"C:\")));
        assert!(!is_allowed_clean_root(Path::new(r"D:\")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Windows")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Windows\System32")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Program Files")));
        assert!(!is_allowed_clean_root(Path::new(
            r"C:\Program Files (x86)\Common Files"
        )));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Users\Foo\Documents")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Users\Foo\Desktop")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Users\Foo\Pictures")));
        assert!(!is_allowed_clean_root(Path::new(r"C:\Users\Foo\Downloads")));
        assert!(!is_allowed_clean_root(Path::new(
            r"C:\Windows\Temp\..\System32"
        )));
        assert!(!is_allowed_clean_root(Path::new(r"relative\temp")));

        // Allowed safe clean roots and explicit test temp directories.
        let sys_temp = std::env::temp_dir();
        assert!(is_allowed_clean_root(&sys_temp));
        let test_sub = sys_temp.join("rtv-highperf-test-clean-root");
        assert!(is_allowed_clean_root(&test_sub));

        assert!(is_allowed_clean_root(Path::new(r"C:\Windows\Temp")));
        assert!(is_allowed_clean_root(Path::new(
            r"C:\Windows\SoftwareDistribution\Download"
        )));
        assert!(is_allowed_clean_root(Path::new(r"C:\Windows\Minidump")));
        assert!(is_allowed_clean_root(Path::new(
            r"C:\Users\Evan\AppData\Local\Temp"
        )));
        assert!(is_allowed_clean_root(Path::new(
            r"C:\Users\Evan\AppData\Local\CrashDumps"
        )));
        assert!(is_allowed_clean_root(Path::new(
            r"C:\Users\Evan\AppData\Local\NVIDIA\DXCache"
        )));

        let empty: PathBuf = PathBuf::new();
        assert!(!is_allowed_clean_root(&empty));
    }

    #[test]
    fn test_app_error_serializes_structured() {
        let err = AppError::ElevationRequired("HKLM access".to_string());
        let json = serde_json::to_value(&err).expect("AppError should serialize");
        assert_eq!(json["kind"], "ElevationRequired");
        assert!(json["message"].as_str().unwrap().contains("HKLM access"));
    }
}
