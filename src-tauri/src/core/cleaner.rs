use std::collections::HashSet;
#[cfg(any(windows, test))]
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use jwalk::WalkDir;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::core::privilege;
use crate::core::safety::{self, AppError};

/// Summary report for a single allowlisted cleaning category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanCategoryReport {
    pub id: String,
    pub name: String,
    pub description: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub file_count: u64,
    pub requires_admin: bool,
    pub opt_in_default_off: bool,
    pub warning: Option<String>,
}

/// Outcome of executing a clean operation across one or more categories or directories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CleanResult {
    pub freed_bytes: u64,
    pub deleted_files: u64,
    pub skipped_locked: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum CategoryKind {
    Directories(Vec<PathBuf>),
    RecycleBin,
}

#[derive(Debug, Clone)]
struct CleanCategorySpec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    requires_admin: bool,
    opt_in_default_off: bool,
    warning: Option<String>,
    kind: CategoryKind,
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn windows_dir() -> PathBuf {
    env_path("WINDIR").unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn local_appdata_dir() -> Option<PathBuf> {
    env_path("LOCALAPPDATA").or_else(|| {
        env_path("USERPROFILE").map(|p| p.join("AppData").join("Local"))
    })
}

fn roaming_appdata_dir() -> Option<PathBuf> {
    env_path("APPDATA").or_else(|| {
        env_path("USERPROFILE").map(|p| p.join("AppData").join("Roaming"))
    })
}

fn locallow_appdata_dir() -> Option<PathBuf> {
    env_path("USERPROFILE").map(|p| p.join("AppData").join("LocalLow"))
}

fn push_if_allowed_and_unique(out: &mut Vec<PathBuf>, seen: &mut HashSet<String>, path: PathBuf) {
    if !safety::is_allowed_clean_root(&path) {
        return;
    }
    let key = path.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        out.push(path);
    }
}

fn push_if_existing_allowed(out: &mut Vec<PathBuf>, seen: &mut HashSet<String>, path: PathBuf) {
    if path.exists() {
        push_if_allowed_and_unique(out, seen, path);
    }
}

fn collect_chromium_profile_caches(
    user_data_dir: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(user_data_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let profile_dir = entry.path();
        for cache_sub in ["Cache", "Code Cache", "GPUCache"] {
            let candidate = profile_dir.join(cache_sub);
            push_if_existing_allowed(out, seen, candidate);
        }
    }
}

fn collect_firefox_profile_caches(
    profiles_dir: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(profiles_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let candidate = entry.path().join("cache2");
        push_if_existing_allowed(out, seen, candidate);
    }
}

fn detect_running_browsers() -> Vec<&'static str> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());

    let mut running = Vec::new();
    let targets = [
        ("msedge.exe", "Microsoft Edge"),
        ("chrome.exe", "Google Chrome"),
        ("brave.exe", "Brave"),
        ("firefox.exe", "Mozilla Firefox"),
    ];

    for (exe, label) in targets {
        let found = sys.processes().values().any(|proc_info| {
            proc_info
                .name()
                .to_string_lossy()
                .eq_ignore_ascii_case(exe)
        });
        if found {
            running.push(label);
        }
    }

    running
}

fn build_category_specs() -> Vec<CleanCategorySpec> {
    let win_dir = windows_dir();
    let local_app = local_appdata_dir();
    let roaming_app = roaming_appdata_dir();
    let locallow_app = locallow_appdata_dir();
    let user_profile = env_path("USERPROFILE");

    // 1. user_temp
    let mut user_temp_paths = Vec::new();
    let mut seen = HashSet::new();
    push_if_allowed_and_unique(&mut user_temp_paths, &mut seen, std::env::temp_dir());
    if let Some(ref la) = local_app {
        push_if_existing_allowed(&mut user_temp_paths, &mut seen, la.join("Temp"));
    }

    // 2. win_temp
    let mut win_temp_paths = Vec::new();
    seen.clear();
    push_if_allowed_and_unique(&mut win_temp_paths, &mut seen, win_dir.join("Temp"));

    // 3. wu_cache
    let mut wu_cache_paths = Vec::new();
    seen.clear();
    push_if_allowed_and_unique(
        &mut wu_cache_paths,
        &mut seen,
        win_dir.join("SoftwareDistribution").join("Download"),
    );

    // 4. crash_dumps
    let mut crash_paths = Vec::new();
    seen.clear();
    if let Some(ref la) = local_app {
        push_if_existing_allowed(&mut crash_paths, &mut seen, la.join("CrashDumps"));
        push_if_existing_allowed(
            &mut crash_paths,
            &mut seen,
            la.join("Microsoft").join("Windows").join("WER"),
        );
    }
    push_if_existing_allowed(&mut crash_paths, &mut seen, win_dir.join("Minidump"));
    push_if_existing_allowed(&mut crash_paths, &mut seen, win_dir.join("LiveKernelReports"));

    // 5. gpu_shaders
    let mut gpu_paths = Vec::new();
    seen.clear();
    if let Some(ref la) = local_app {
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("D3DSCache"));
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("NVIDIA").join("DXCache"));
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("NVIDIA").join("GLCache"));
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("AMD").join("DxCache"));
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("AMD").join("GLCache"));
        push_if_existing_allowed(&mut gpu_paths, &mut seen, la.join("AMD").join("VkCache"));
    }
    if let Some(ref lla) = locallow_app {
        push_if_existing_allowed(&mut gpu_paths, &mut seen, lla.join("NVIDIA").join("DXCache"));
    }

    // 6. browser_cache
    let mut browser_paths = Vec::new();
    seen.clear();
    if let Some(ref la) = local_app {
        collect_chromium_profile_caches(
            &la.join("Microsoft").join("Edge").join("User Data"),
            &mut browser_paths,
            &mut seen,
        );
        collect_chromium_profile_caches(
            &la.join("Google").join("Chrome").join("User Data"),
            &mut browser_paths,
            &mut seen,
        );
        collect_chromium_profile_caches(
            &la.join("BraveSoftware")
                .join("Brave-Browser")
                .join("User Data"),
            &mut browser_paths,
            &mut seen,
        );
        collect_firefox_profile_caches(
            &la.join("Mozilla").join("Firefox").join("Profiles"),
            &mut browser_paths,
            &mut seen,
        );
    }
    let running_browsers = detect_running_browsers();
    let browser_warning = if running_browsers.is_empty() {
        None
    } else {
        Some(format!(
            "Running browser(s) detected: {}. Locked cache files will be skipped automatically.",
            running_browsers.join(", ")
        ))
    };

    // 7. dev_cache (opt-in default off)
    let mut dev_paths = Vec::new();
    seen.clear();
    if let Some(ref la) = local_app {
        push_if_existing_allowed(&mut dev_paths, &mut seen, la.join("npm-cache"));
        push_if_existing_allowed(&mut dev_paths, &mut seen, la.join("pnpm").join("store"));
        push_if_existing_allowed(&mut dev_paths, &mut seen, la.join("pnpm-cache"));
        push_if_existing_allowed(&mut dev_paths, &mut seen, la.join("pip").join("cache"));
    }
    if let Some(ref ra) = roaming_app {
        push_if_existing_allowed(&mut dev_paths, &mut seen, ra.join("npm-cache"));
    }
    if let Some(ref up) = user_profile {
        push_if_existing_allowed(
            &mut dev_paths,
            &mut seen,
            up.join(".cargo").join("registry").join("cache"),
        );
    }

    vec![
        CleanCategorySpec {
            id: "user_temp",
            name: "User Temporary Files",
            description: "Application and installer temporary files in your user Temp directory.",
            requires_admin: false,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::Directories(user_temp_paths),
        },
        CleanCategorySpec {
            id: "win_temp",
            name: "Windows Temporary Files",
            description: "System-wide temporary files stored in Windows\\Temp.",
            requires_admin: true,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::Directories(win_temp_paths),
        },
        CleanCategorySpec {
            id: "wu_cache",
            name: "Windows Update Download Cache",
            description: "Downloaded Windows Update packages in SoftwareDistribution\\Download.",
            requires_admin: true,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::Directories(wu_cache_paths),
        },
        CleanCategorySpec {
            id: "crash_dumps",
            name: "Crash Dumps & Error Reports",
            description: "User crash dumps, Windows Error Reporting logs, and kernel minidumps.",
            requires_admin: false,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::Directories(crash_paths),
        },
        CleanCategorySpec {
            id: "gpu_shaders",
            name: "GPU Shader Caches",
            description: "Compiled DirectX, NVIDIA, and AMD shader caches (rebuilt as needed).",
            requires_admin: false,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::Directories(gpu_paths),
        },
        CleanCategorySpec {
            id: "browser_cache",
            name: "Browser Caches",
            description: "Web caches for Microsoft Edge, Google Chrome, Brave, and Mozilla Firefox.",
            requires_admin: false,
            opt_in_default_off: false,
            warning: browser_warning,
            kind: CategoryKind::Directories(browser_paths),
        },
        CleanCategorySpec {
            id: "dev_cache",
            name: "Developer Package Caches",
            description: "Local npm, pnpm, pip, and Cargo registry caches (opt-in).",
            requires_admin: false,
            opt_in_default_off: true,
            warning: Some(
                "Opt-in: clearing package caches frees disk space, but future installs or builds may re-download packages."
                    .to_string(),
            ),
            kind: CategoryKind::Directories(dev_paths),
        },
        CleanCategorySpec {
            id: "recycle_bin",
            name: "Recycle Bin",
            description: "Deleted files currently held in the Windows Recycle Bin.",
            requires_admin: false,
            opt_in_default_off: false,
            warning: None,
            kind: CategoryKind::RecycleBin,
        },
    ]
}

fn scan_directory_totals(paths: &[PathBuf]) -> (u64, u64) {
    paths
        .par_iter()
        .map(|root| {
            if !safety::is_allowed_clean_root(root) || !root.exists() {
                return (0u64, 0u64);
            }

            let mut total_bytes = 0u64;
            let mut total_files = 0u64;

            for entry in WalkDir::new(root)
                .skip_hidden(false)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if entry.depth == 0 || !entry.file_type().is_file() {
                    continue;
                }
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                total_bytes = total_bytes.saturating_add(size);
                total_files = total_files.saturating_add(1);
            }

            (total_bytes, total_files)
        })
        .reduce(|| (0u64, 0u64), |a, b| (a.0 + b.0, a.1 + b.1))
}

#[cfg(windows)]
fn query_recycle_bin() -> Result<(u64, u64), AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };

    unsafe {
        SHQueryRecycleBinW(PCWSTR::null(), &mut info)?;
    }

    Ok((
        u64::try_from(info.i64Size).unwrap_or(0),
        u64::try_from(info.i64NumItems).unwrap_or(0),
    ))
}

#[cfg(not(windows))]
fn query_recycle_bin() -> Result<(u64, u64), AppError> {
    Ok((0, 0))
}

#[cfg(windows)]
fn empty_recycle_bin() -> Result<(u64, u64), AppError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
    };

    let (bytes, items) = query_recycle_bin()?;
    if bytes == 0 && items == 0 {
        return Ok((0, 0));
    }

    unsafe {
        SHEmptyRecycleBinW(
            HWND::default(),
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )?;
    }

    Ok((bytes, items))
}

#[cfg(not(windows))]
fn empty_recycle_bin() -> Result<(u64, u64), AppError> {
    Ok((0, 0))
}

fn is_locked_io_error(err: &std::io::Error) -> bool {
    // Win32 sharing/lock/memory-mapped errors:
    // 32 = ERROR_SHARING_VIOLATION, 33 = ERROR_LOCK_VIOLATION, 1224 = ERROR_USER_MAPPED_FILE
    matches!(err.raw_os_error(), Some(32 | 33 | 1224))
}

#[cfg(windows)]
fn is_file_currently_locked(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    // Opening with share_mode(0) fails with sharing violation if any handle is open on the file.
    match OpenOptions::new().read(true).share_mode(0).open(path) {
        Ok(_) => false,
        Err(err) => is_locked_io_error(&err) || err.kind() == ErrorKind::PermissionDenied,
    }
}

#[cfg(not(windows))]
fn is_file_currently_locked(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn send_file_to_recycle_bin(path: &Path) -> Result<(), (bool, String)> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    if is_file_currently_locked(path) {
        return Err((true, "File is locked by another process".to_string()));
    }

    let mut wide_from: Vec<u16> = path.as_os_str().encode_wide_null();
    // SHFileOperationW requires double-null termination for pFrom.
    wide_from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide_from.as_ptr()),
        pTo: PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0) as u16,
        fAnyOperationsAborted: false.into(),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };

    let code = unsafe { SHFileOperationW(&mut op) };
    if code == 0 && !op.fAnyOperationsAborted.as_bool() {
        Ok(())
    } else if code == 32 || code == 33 || is_file_currently_locked(path) {
        Err((true, format!("Locked during recycle (code {code})")))
    } else {
        Err((
            false,
            format!("Failed to recycle '{}': code {code}", path.display()),
        ))
    }
}

#[cfg(windows)]
trait OsStrWideNull {
    fn encode_wide_null(&self) -> Vec<u16>;
}

#[cfg(windows)]
impl OsStrWideNull for std::ffi::OsStr {
    fn encode_wide_null(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
fn send_file_to_recycle_bin(path: &Path) -> Result<(), (bool, String)> {
    std::fs::remove_file(path).map_err(|e| (is_locked_io_error(&e), e.to_string()))
}

enum FileDeleteOutcome {
    Deleted(u64),
    AlreadyGone,
    SkippedLocked,
    Error(String),
}

fn delete_single_file(path: &Path, size: u64, move_to_recycle_bin: bool) -> FileDeleteOutcome {
    if move_to_recycle_bin {
        return match send_file_to_recycle_bin(path) {
            Ok(()) => FileDeleteOutcome::Deleted(size),
            Err((true, _)) => FileDeleteOutcome::SkippedLocked,
            Err((false, msg)) => {
                if !path.exists() {
                    FileDeleteOutcome::AlreadyGone
                } else {
                    FileDeleteOutcome::Error(msg)
                }
            }
        };
    }

    match std::fs::remove_file(path) {
        Ok(()) => FileDeleteOutcome::Deleted(size),
        Err(err) if err.kind() == ErrorKind::NotFound => FileDeleteOutcome::AlreadyGone,
        Err(err) if is_locked_io_error(&err) => FileDeleteOutcome::SkippedLocked,
        Err(err) if err.kind() == ErrorKind::PermissionDenied && is_file_currently_locked(path) => {
            FileDeleteOutcome::SkippedLocked
        }
        Err(err) => FileDeleteOutcome::Error(format!("{}: {}", path.display(), err)),
    }
}

/// Cleans all files inside the specified allowlisted root directories.
/// Validates every root with `safety::is_allowed_clean_root` before walking,
/// skips locked files gracefully (`skipped_locked += 1`), and cleans empty subdirectories.
pub fn clean_dir_entries(
    paths: &[PathBuf],
    move_to_recycle_bin: bool,
) -> Result<CleanResult, AppError> {
    for root in paths {
        if !safety::is_allowed_clean_root(root) {
            return Err(AppError::ProtectedTarget(format!(
                "Path '{}' is not an allowed clean root",
                root.display()
            )));
        }
    }

    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let mut dirs: Vec<(usize, PathBuf)> = Vec::new();

    for root in paths {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if entry.depth == 0 {
                continue;
            }
            let ft = entry.file_type();
            if ft.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((entry.path(), size));
            } else if ft.is_dir() {
                dirs.push((entry.depth, entry.path()));
            }
        }
    }

    let freed_bytes = AtomicU64::new(0);
    let deleted_files = AtomicU64::new(0);
    let skipped_locked = AtomicU64::new(0);

    let errors: Vec<String> = if move_to_recycle_bin {
        // SHFileOperationW interacts with Shell COM state; run sequentially to stay thread-safe.
        let mut errs = Vec::new();
        for (path, size) in &files {
            match delete_single_file(path, *size, true) {
                FileDeleteOutcome::Deleted(bytes) => {
                    freed_bytes.fetch_add(bytes, Ordering::Relaxed);
                    deleted_files.fetch_add(1, Ordering::Relaxed);
                }
                FileDeleteOutcome::AlreadyGone => {}
                FileDeleteOutcome::SkippedLocked => {
                    skipped_locked.fetch_add(1, Ordering::Relaxed);
                }
                FileDeleteOutcome::Error(msg) => {
                    if errs.len() < 50 {
                        errs.push(msg);
                    }
                }
            }
        }
        errs
    } else {
        files
            .par_iter()
            .filter_map(|(path, size)| match delete_single_file(path, *size, false) {
                FileDeleteOutcome::Deleted(bytes) => {
                    freed_bytes.fetch_add(bytes, Ordering::Relaxed);
                    deleted_files.fetch_add(1, Ordering::Relaxed);
                    None
                }
                FileDeleteOutcome::AlreadyGone => None,
                FileDeleteOutcome::SkippedLocked => {
                    skipped_locked.fetch_add(1, Ordering::Relaxed);
                    None
                }
                FileDeleteOutcome::Error(msg) => Some(msg),
            })
            .collect()
    };

    // Remove empty subdirectories deepest-first; non-empty ones (holding locked files) stay intact.
    dirs.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, dir) in dirs {
        let _ = std::fs::remove_dir(dir);
    }

    Ok(CleanResult {
        freed_bytes: freed_bytes.load(Ordering::Relaxed),
        deleted_files: deleted_files.load(Ordering::Relaxed),
        skipped_locked: skipped_locked.load(Ordering::Relaxed),
        errors: errors.into_iter().take(50).collect(),
    })
}

/// Scans all allowlisted cleaning categories in parallel and returns size/file-count reports.
pub fn scan_categories() -> Vec<CleanCategoryReport> {
    let specs = build_category_specs();

    specs
        .into_par_iter()
        .map(|spec| {
            let mut warning = spec.warning;
            let (paths, bytes, file_count) = match spec.kind {
                CategoryKind::Directories(dir_paths) => {
                    let (b, f) = scan_directory_totals(&dir_paths);
                    let strs = dir_paths
                        .into_iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    (strs, b, f)
                }
                CategoryKind::RecycleBin => match query_recycle_bin() {
                    Ok((b, f)) => (vec!["$Recycle.Bin".to_string()], b, f),
                    Err(err) => {
                        warning = Some(format!("Unable to query Recycle Bin: {err}"));
                        (vec!["$Recycle.Bin".to_string()], 0, 0)
                    }
                },
            };

            CleanCategoryReport {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                description: spec.description.to_string(),
                paths,
                bytes,
                file_count,
                requires_admin: spec.requires_admin,
                opt_in_default_off: spec.opt_in_default_off,
                warning,
            }
        })
        .collect()
}

/// Cleans the requested allowlisted category IDs.
/// Returns `AppError::ElevationRequired` upfront if any selected category requires admin
/// privileges and the process is not elevated.
pub fn clean_categories(
    category_ids: &[String],
    move_to_recycle_bin: bool,
) -> Result<CleanResult, AppError> {
    let specs = build_category_specs();
    let mut selected = Vec::new();
    let mut seen_ids = HashSet::new();

    for id in category_ids {
        let Some(spec) = specs.iter().find(|s| s.id == id.as_str()) else {
            return Err(AppError::InvalidInput(format!(
                "Unknown cleaner category '{id}'"
            )));
        };
        if seen_ids.insert(spec.id) {
            selected.push(spec);
        }
    }

    if !privilege::is_elevated() {
        for spec in &selected {
            if spec.requires_admin {
                return Err(AppError::ElevationRequired(format!(
                    "Cleaning '{}' requires Administrator privileges",
                    spec.name
                )));
            }
        }
    }

    let mut total = CleanResult::default();
    for spec in selected {
        match &spec.kind {
            CategoryKind::Directories(paths) => {
                let res = clean_dir_entries(paths, move_to_recycle_bin)?;
                total.freed_bytes = total.freed_bytes.saturating_add(res.freed_bytes);
                total.deleted_files = total.deleted_files.saturating_add(res.deleted_files);
                total.skipped_locked = total.skipped_locked.saturating_add(res.skipped_locked);
                for err in res.errors {
                    if total.errors.len() < 50 {
                        total.errors.push(err);
                    }
                }
            }
            CategoryKind::RecycleBin => match empty_recycle_bin() {
                Ok((bytes, items)) => {
                    total.freed_bytes = total.freed_bytes.saturating_add(bytes);
                    total.deleted_files = total.deleted_files.saturating_add(items);
                }
                Err(err) => {
                    if total.errors.len() < 50 {
                        total.errors.push(format!("Recycle Bin: {err}"));
                    }
                }
            },
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "rtv-highperf-cleaner-{}-{}-{}",
            tag,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn test_cleaner_deletes_temp_files_and_skips_locked() {
        let root = unique_temp_dir("locked");
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("create test temp dir");

        let file_a = root.join("temp_a.tmp");
        let file_b = sub.join("temp_b.log");
        let locked_file = root.join("in_use.lock");

        {
            let mut f = File::create(&file_a).expect("create temp_a");
            f.write_all(b"0123456789ab").expect("write temp_a"); // 12 bytes
        }
        {
            let mut f = File::create(&file_b).expect("create temp_b");
            f.write_all(b"01234567890123456789").expect("write temp_b"); // 20 bytes
        }

        // Hold an exclusive handle (share_mode = 0) so Windows blocks deletion with ERROR_SHARING_VIOLATION.
        #[cfg(windows)]
        let locked_handle = {
            use std::os::windows::fs::OpenOptionsExt;
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&locked_file)
                .expect("open locked_file")
        };
        #[cfg(not(windows))]
        let locked_handle = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&locked_file)
            .expect("open locked_file");
        {
            let mut h_ref = &locked_handle;
            h_ref.write_all(b"locked_contents!").expect("write locked");
        }

        let res = clean_dir_entries(std::slice::from_ref(&root), false)
            .expect("clean_dir_entries should succeed");

        assert_eq!(res.deleted_files, 2, "both unlocked files should be deleted");
        assert_eq!(res.freed_bytes, 32, "freed_bytes should sum unlocked files");
        assert_eq!(res.skipped_locked, 1, "locked file should be skipped");
        assert!(res.errors.is_empty(), "unexpected errors: {:?}", res.errors);
        assert!(!file_a.exists());
        assert!(!file_b.exists());
        assert!(locked_file.exists(), "locked file must remain intact");

        drop(locked_handle);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_cleaner_rejects_disallowed_root() {
        let err = clean_dir_entries(&[PathBuf::from(r"C:\Windows\System32")], false)
            .expect_err("System32 must be rejected");
        assert!(matches!(err, AppError::ProtectedTarget(_)));
    }

    #[test]
    fn test_scan_categories_contains_expected_allowlist() {
        let reports = scan_categories();
        let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
        for expected in [
            "user_temp",
            "win_temp",
            "wu_cache",
            "crash_dumps",
            "gpu_shaders",
            "browser_cache",
            "dev_cache",
            "recycle_bin",
        ] {
            assert!(ids.contains(&expected), "missing category {expected}");
        }

        let win_temp = reports.iter().find(|r| r.id == "win_temp").unwrap();
        assert!(win_temp.requires_admin);
        let wu_cache = reports.iter().find(|r| r.id == "wu_cache").unwrap();
        assert!(wu_cache.requires_admin);
        let dev_cache = reports.iter().find(|r| r.id == "dev_cache").unwrap();
        assert!(dev_cache.opt_in_default_off);
    }

    #[test]
    fn test_clean_categories_rejects_unknown_id() {
        let err = clean_categories(&["nonexistent_cat".to_string()], false)
            .expect_err("unknown category should return InvalidInput");
        assert!(matches!(err, AppError::InvalidInput(_)));
    }
}
