# rtv-highperf Implementation Plan

> **For agentic workers:** Execution uses the native `/swarm` team coordinated against this plan and the approved spec. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rtv-highperf`, an open-source, offline-first Windows 10/11 PC optimizer in Rust (Tauri v2 + React/TypeScript) with Lag Detective diagnostics, Hybrid Auto-Optimizer (foreground boost + EcoQoS + smart RAM trim + Power Plan + startup/crash recovery), Safe Registry/Service Tweaker with 1-click JSON/REG rollback, separate AppX Bloatware Manager, Parallel Deep Cleaner, Startup/Task Manager, System Tray, and CLI.

**Architecture:** Rust backend (`src-tauri/src/`) owns all Win32/WMI/Registry/Filesystem operations behind strict safety allowlists and snapshot backups; React 19 + TypeScript + Vite frontend (`src/`) communicates via typed Tauri IPC commands/events (`@tauri-apps/api`) with bundled `Instrument Sans` + `IBM Plex Mono` fonts and Obsidian Dark / Daylight Light themes. Unprivileged by default, with `ShellExecuteW("runas")` elevation flow and System Tray minimize/WebView-unload support.

**Tech Stack:** Rust (stable MSVC), Tauri v2 (`tauri`, `tauri-plugin-single-instance`), `windows` crate (Win32 Process/Power/Registry/Shell/Service APIs), `sysinfo`, `jwalk`, `rayon`, `serde`/`serde_json`, `clap`, `chrono`, `thiserror`, React 19, TypeScript, Vite, `@fontsource/instrument-sans`, `@fontsource/ibm-plex-mono`.

**Spec:** `docs/superpowers/specs/2026-11-22-rtv-highperf-design.md`

## Global Constraints
- Target OS: Windows 10 (19041+) / Windows 11 (`x86_64-pc-windows-msvc`).
- Zero network calls / zero telemetry; 100% offline-first.
- License: MIT (`LICENSE` at repo root).
- Unprivileged by default; admin-only tweaks/cleaners/services report `requires_admin: true` and use `ShellExecuteW("runas")` restart flow.
- Protected processes (`System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `services.exe`, `lsass.exe`, `svchost.exe`, `dwm.exe`, `audiodg.exe`, `MsMpEng.exe`, `explorer.exe`, anti-cheat processes, `rtv-highperf.exe`) are never lowered, EcoQoS-throttled, or RAM-trimmed.
- Deep Cleaner uses an explicit target-path allowlist only (never user-supplied arbitrary root recursion) and skips locked files gracefully.
- All Registry/Service tweaks write a pre-change JSON snapshot to `%APPDATA%\rtv-highperf\backups\snapshot-<timestamp>.json` before modification.
- UWP/AppX removal is separated from snapshotted Registry/Service tweaks, targets current-user (`Remove-AppxPackage`) only, and displays an explicit non-reversible warning.
- UI follows `antislop` (`DURING`), `typography-curation`, `micro-interactions`: Obsidian Dark + Daylight Light toggle (both WCAG AA verified), `Instrument Sans` + `IBM Plex Mono` (`tabular-nums`), no invented metrics/fake stats, no blue-purple gradient, no decorative emojis, full keyboard navigation + `:focus-visible`, explicit Empty/Loading/Error states, `prefers-reduced-motion: reduce`.

## Review Focus
1. **Crash/Reboot Dirty Power-Plan State:** App switches Windows Power Plan to High Performance, then crashes or PC loses power -> next launch reads `dirty_power_scheme` from `config.json` and restores `original_power_scheme_guid` before applying any new profile. (Tested in Task 2 & Task 4)
2. **Locked Browser/Temp Files During Deep Clean:** Chrome/Edge is open and holds locks on cache files -> Cleaner skips locked files without panicking or aborting the batch, reports `skipped_locked_count` accurately. (Tested in Task 5)
3. **Missing Registry Key Before Tweak Apply & Rollback:** Target tweak key/value does not exist yet on clean Windows -> Snapshot records `"__NOT_EXISTED__"`; rollback deletes the created value cleanly instead of writing empty garbage. (Tested in Task 6)
4. **Protected Process & Self-PID Guard:** Governor loop encounters `csrss.exe`, `dwm.exe`, `MsMpEng.exe`, or `rtv-highperf.exe` as foreground/background -> `is_protected_process()` blocks priority downgrade, EcoQoS, and `EmptyWorkingSet`. (Tested in Task 2 & Task 4)
5. **Unprivileged User Invoking Admin Action:** Standard user clicks an Admin-tagged tweak or Windows Update cache clean -> Backend returns structured `ElevationRequired` error (never partial corruption), and UI offers "Restart as Administrator". (Tested in Task 2, Task 6, Task 8)

---

### Task 1: Toolchain Provisioning & Tauri v2 Project Scaffold
**Files:**
- Create: `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`, `src/main.tsx`, `src/App.tsx`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/icons/*`
- Create: `LICENSE`, `.gitignore`

**Interfaces:**
- Produces: Compilable Tauri v2 workspace (`cargo check` + `npm run build` succeed) with `src-tauri/src/core/mod.rs` module layout declared and IPC command registration stubbed in `lib.rs`.

- [ ] **Step 1: Install Rustup (MSVC) & VS 2022 Build Tools if absent**
  Run `winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements` and `winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` (plus verify `rustc --version`, `cargo --version`, and `cl.exe` availability in PATH).
- [ ] **Step 2: Initialize React 19 + TypeScript + Vite + Fontsource dependencies**
  Create `package.json` with `react`, `react-dom`, `@tauri-apps/api`, `@tauri-apps/cli`, `@fontsource/instrument-sans`, `@fontsource/ibm-plex-mono`, `lucide-react`? (Note: use crisp custom/minimal technical SVG icons or deliberate icon set per `antislop-ui` R-04; let's hand-craft lightweight inline SVG icons in `src/components/Icons.tsx` for zero dependency bloat & bespoke character!). Install npm packages.
- [ ] **Step 3: Initialize `src-tauri/` with Cargo.toml & `tauri.conf.json`**
  Configure crates: `tauri = { version = "2", features = ["tray-icon"] }`, `tauri-plugin-single-instance = "2"`, `windows = { version = "0.58", features = [...] }`, `sysinfo = "0.32"`, `jwalk = "0.8"`, `rayon = "1.10"`, `serde`, `serde_json`, `chrono`, `clap`, `thiserror`. Configure `tauri.conf.json` (window title `rtv-highperf`, size `1160x760`, min `900x620`, tray icon, identifier `dev.evankristanto.rtvhighperf`).
- [ ] **Step 4: Verify empty scaffold compiles cleanly**
  Run: `npm run build` and `cargo check --manifest-path src-tauri/Cargo.toml` -> Expected: Exit 0.
- [ ] **Step 5: Commit scaffold**
  `git add . && git commit -m "chore: scaffold Tauri v2 + React/TS project"`

---

### Task 2: Safety Guards, Config Persistence & Privilege Elevation (`core/safety.rs`, `core/config.rs`, `core/privilege.rs`)
**Files:**
- Create: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/src/core/safety.rs`
- Create: `src-tauri/src/core/config.rs`
- Create: `src-tauri/src/core/privilege.rs`

**Interfaces:**
- Produces (`safety.rs`):
  - `pub fn is_protected_process(name: &str, pid: u32, self_pid: u32) -> bool`
  - `pub fn is_allowed_clean_root(path: &Path) -> bool`
  - `pub enum AppError { ElevationRequired(String), ProtectedTarget(String), Io(std::io::Error), Win32(String), InvalidInput(String) }`
- Produces (`config.rs`):
  - `pub struct AppConfig { pub theme: String, pub active_profile: GovernorProfile, pub auto_start: bool, pub minimize_to_tray: bool, pub unload_webview_on_tray: bool, pub ram_trim_threshold_pct: u8, pub exclusions: Vec<String>, pub dirty_power_scheme: Option<DirtyPowerState>, pub last_route: String }`
  - `pub fn load_config(dir: &Path) -> Result<AppConfig, AppError>`
  - `pub fn save_config(dir: &Path, cfg: &AppConfig) -> Result<(), AppError>`
- Produces (`privilege.rs`):
  - `pub fn is_elevated() -> bool`
  - `pub fn restart_as_admin(last_route: &str) -> Result<(), AppError>`

- [ ] **Step 1: Write failing unit tests in `safety.rs` & `config.rs`**
  - `test_protected_processes_blocked`: asserts `csrss.exe`, `LSASS.EXE` (case-insensitive), `dwm.exe`, `MsMpEng.exe`, `svchost.exe`, `rtv-highperf.exe`, and `pid == self_pid` or `pid <= 4` return `true`; `chrome.exe` / `notepad.exe` return `false`.
  - `test_clean_root_guards`: rejects `C:\Windows\System32`, `C:\Users\Foo\Documents`, `..` traversal; allows `%TEMP%` or `SoftwareDistribution\Download`.
  - `test_config_roundtrip_and_dirty_power_state`: saves `AppConfig` with `dirty_power_scheme: Some(...)` to temp dir, loads back identically.
- [ ] **Step 2: Run `cargo test --manifest-path src-tauri/Cargo.toml core::safety` to verify fail**
- [ ] **Step 3: Implement `safety.rs`, `config.rs`, `privilege.rs`**
- [ ] **Step 4: Run `cargo test` for `safety` and `config` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 3: Lag Detective Diagnostics & Health Score Engine (`core/detective.rs`)
**Files:**
- Create: `src-tauri/src/core/detective.rs`

**Interfaces:**
- Produces:
  - `pub struct SystemSnapshot { pub cpu_total_pct: f32, pub cpu_cores: Vec<f32>, pub ram_used_bytes: u64, pub ram_total_bytes: u64, pub ram_available_bytes: u64, pub commit_pct: f32, pub disk_read_bps: u64, pub disk_write_bps: u64, pub active_power_plan: String, pub power_plan_guid: String, pub is_on_ac_power: Option<bool>, pub startup_enabled_count: usize, pub cleanable_bytes_cached: Option<u64>, pub top_processes: Vec<ProcessInfo>, pub health_score: u8, pub findings: Vec<LagFinding>, pub sampled_at_ms: u64 }`
  - `pub struct LagFinding { pub id: String, pub severity: Severity, pub title: String, pub detail: String, pub deduction: u8, pub fix_action: Option<FixAction> }`
  - `pub fn compute_health_score(input: &HealthInput) -> (u8, Vec<LagFinding>)`
  - `pub fn sample_system(sys: &mut sysinfo::System, cleanable_cached: Option<u64>, startup_count: usize) -> SystemSnapshot`

- [ ] **Step 1: Write failing unit tests in `detective.rs`**
  - `test_health_score_perfect_system`: RAM 40%, CPU 15%, startup 3, balanced/high perf plan -> score 100, 0 findings.
  - `test_health_score_bottlenecks`: RAM 88%, startup 14, Power Saver on AC, cleanable 8 GB -> deterministic deductions, findings sorted by severity with actionable `fix_action` variants.
- [ ] **Step 2: Run test to verify failure**
- [ ] **Step 3: Implement `detective.rs` (Win32 `GlobalMemoryStatusEx`, `GetSystemPowerStatus`, `PowerGetActiveScheme` + `sysinfo` CPU/process deltas)**
- [ ] **Step 4: Run `cargo test core::detective` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 4: Hybrid Auto-Optimizer Governor & Startup Scheduler (`core/governor.rs`)
**Files:**
- Create: `src-tauri/src/core/governor.rs`

**Interfaces:**
- Produces:
  - `pub enum GovernorProfile { Off, BalancedWork, GamingFocus, CodingCompile }`
  - `pub struct GovernorState { pub enabled: bool, pub profile: GovernorProfile, pub last_boosted_pid: Option<u32>, pub eco_qos_pids: Vec<u32>, pub last_trim_freed_mb: u64, pub last_trim_at: Option<u64> }`
  - `pub fn recover_dirty_power_scheme(cfg_dir: &Path) -> Result<Option<String>, AppError>`
  - `pub fn set_process_eco_qos(pid: u32, enable: bool) -> Result<(), AppError>`
  - `pub fn trim_idle_ram(exclusions: &[String], min_ws_mb: u64, foreground_pid: u32) -> Result<RamTrimReport, AppError>`
  - `pub fn list_power_plans() -> Vec<PowerPlanInfo>`
  - `pub fn set_power_plan(guid: &str, cfg_dir: &Path) -> Result<(), AppError>`
  - `pub fn set_windows_autostart(enable: bool, elevated: bool, exe_path: &Path) -> Result<(), AppError>`
  - `pub fn tick_governor(state: &mut GovernorState, cfg: &AppConfig, cfg_dir: &Path)`

- [ ] **Step 1: Write failing unit tests in `governor.rs`**
  - `test_governor_skips_protected_and_excluded_pids`: verifies pure decision planner `plan_governor_actions(...)` never schedules EcoQoS or RAM trim on protected processes, foreground PID, or user exclusion list.
  - `test_dirty_power_scheme_recovery_clears_flag`: writes `dirty_power_scheme` into temp `config.json` and verifies recovery clears it (with mockable/testable switcher trait).
- [ ] **Step 2: Run test to verify fail**
- [ ] **Step 3: Implement `governor.rs` using Win32 `GetForegroundWindow`, `GetWindowThreadProcessId`, `OpenProcess`, `SetPriorityClass`, `SetProcessInformation(ProcessPowerThrottling)`, `EmptyWorkingSet`, `powercfg` / Power APIs, and Task Scheduler (`schtasks` / COM XML task for `--minimized`)**
- [ ] **Step 4: Run `cargo test core::governor` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 5: Parallel Deep Cleaner & Startup/Task Manager (`core/cleaner.rs`, `core/startup.rs`)
**Files:**
- Create: `src-tauri/src/core/cleaner.rs`
- Create: `src-tauri/src/core/startup.rs`

**Interfaces:**
- Produces (`cleaner.rs`):
  - `pub struct CleanCategoryReport { pub id: String, pub name: String, pub paths: Vec<String>, pub bytes: u64, pub file_count: u64, pub requires_admin: bool, pub opt_in_default_off: bool, pub warning: Option<String> }`
  - `pub struct CleanResult { pub freed_bytes: u64, pub deleted_files: u64, pub skipped_locked: u64, pub errors: Vec<String> }`
  - `pub fn scan_categories() -> Vec<CleanCategoryReport>`
  - `pub fn clean_categories(category_ids: &[String], move_to_recycle_bin: bool) -> Result<CleanResult, AppError>`
- Produces (`startup.rs`):
  - `pub struct StartupItem { pub id: String, pub name: String, pub command: String, pub scope: StartupScope, pub enabled: bool, pub publisher: Option<String> }`
  - `pub fn list_startup_items() -> Vec<StartupItem>`
  - `pub fn toggle_startup_item(id: &str, enable: bool) -> Result<(), AppError>`

- [ ] **Step 1: Write failing unit tests in `cleaner.rs` & `startup.rs`**
  - `test_cleaner_deletes_temp_files_and_skips_locked`: creates temp dir files + opens an exclusive lock on one file -> verifies unlocked files deleted, locked file counted in `skipped_locked` without failing batch.
  - `test_startup_approved_byte_mask`: verifies Windows `StartupApproved\Run` binary flag parsing (`02 00 ...` = enabled, `03 00 ...` = disabled) and toggling preserves original bytes structure.
- [ ] **Step 2: Run test to verify fail**
- [ ] **Step 3: Implement `cleaner.rs` (`jwalk` + `rayon` parallel size sum & safe deletion + `SHQueryRecycleBinW`/`SHEmptyRecycleBinW`) and `startup.rs` (`HKCU`/`HKLM` Run + `StartupApproved\Run` + Startup folder + non-Microsoft Scheduled Tasks)**
- [ ] **Step 4: Run `cargo test core::cleaner core::startup` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 6: Safe Tweaker with Snapshot Rollback & Separate AppX Bloatware Manager (`core/tweaker.rs`)
**Files:**
- Create: `src-tauri/src/core/tweaker.rs`

**Interfaces:**
- Produces:
  - `pub struct TweakItem { pub id: String, pub category: TweakCategory, pub title: String, pub description: String, pub risk: RiskLevel, pub requires_admin: bool, pub target_summary: String, pub current_state: TweakState, pub is_applied: bool }`
  - `pub struct BackupSnapshot { pub id: String, pub created_at: String, pub label: String, pub entries: Vec<BackupEntry> }`
  - `pub struct AppxPackageItem { pub name: String, pub package_full_name: String, pub publisher: String, pub is_removable_bloat: bool }`
  - `pub fn list_tweaks() -> Vec<TweakItem>`
  - `pub fn apply_tweaks(tweak_ids: &[String], backup_dir: &Path) -> Result<BackupSnapshot, AppError>`
  - `pub fn revert_tweaks(tweak_ids: &[String], backup_dir: &Path) -> Result<(), AppError>`
  - `pub fn list_backups(backup_dir: &Path) -> Vec<BackupSnapshot>`
  - `pub fn restore_snapshot(snapshot_id: &str, backup_dir: &Path) -> Result<(), AppError>`
  - `pub fn export_snapshot_reg(snapshot_id: &str, backup_dir: &Path) -> Result<String, AppError>`
  - `pub fn list_removable_appx() -> Result<Vec<AppxPackageItem>, AppError>`
  - `pub fn remove_appx_current_user(package_full_name: &str) -> Result<(), AppError>`

- [ ] **Step 1: Write failing unit tests in `tweaker.rs`**
  - `test_snapshot_records_missing_key_and_exports_reg`: creates a snapshot entry where `previous_value` is `NotExisted` and one `Dword(400)` -> verifies `.reg` exporter generates `"-"` deletion syntax for `NotExisted` and `dword:00000190` for `Dword(400)`.
  - `test_appx_protected_allowlist`: ensures `Microsoft.WindowsStore`, `Microsoft.WindowsTerminal`, `Microsoft.DesktopAppInstaller`, `Microsoft.WindowsCalculator` are never classified as removable bloat.
- [ ] **Step 2: Run test to verify fail**
- [ ] **Step 3: Implement `tweaker.rs` (16+ safe, verified Windows 10/11 tweaks across Latency/Gaming, Privacy/Telemetry, UI Snappiness, and Safe Services + Win32 Registry/Service APIs + JSON snapshot & `.reg` export + PowerShell `Get-AppxPackage`/`Remove-AppxPackage` current-user only)**
- [ ] **Step 4: Run `cargo test core::tweaker` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 7: Tauri IPC Commands, System Tray & Headless CLI (`commands/mod.rs`, `tray.rs`, `cli.rs`, `lib.rs`)
**Files:**
- Create: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/tray.rs`
- Create: `src-tauri/src/cli.rs`
- Modify: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: All Tauri IPC `#[tauri::command]` handlers, background governor tokio/std thread emitting `telemetry-tick` and `governor-update` events, System Tray with `Boost Now`, profile switcher, `Open Dashboard`, `Quit & Restore`, and `--cli` subcommands (`status`, `clean`, `boost`, `revert`) plus `--minimized` startup arg.

- [ ] **Step 1: Write failing unit tests for CLI argument parser (`cli.rs`)**
  - `test_cli_parses_clean_dry_run_and_revert_latest`
- [ ] **Step 2: Run test to verify fail**
- [ ] **Step 3: Implement `commands/mod.rs`, `tray.rs`, `cli.rs`, and wire `lib.rs` / `main.rs`**
- [ ] **Step 4: Run `cargo test` & `cargo clippy -- -D warnings` -> Expected: PASS**
- [ ] **Step 5: Commit**

---

### Task 8: Precision Telemetry Studio Frontend UI (`src/`)
**Files:**
- Create: `src/styles/tokens.css`, `src/styles/app.css`
- Create: `src/lib/tauri.ts`, `src/lib/format.ts`
- Create: `src/components/Icons.tsx`, `src/components/Sidebar.tsx`, `src/components/TopBar.tsx`, `src/components/ToggleSwitch.tsx`, `src/components/Modal.tsx`, `src/components/StateViews.tsx`, `src/components/Sparkline.tsx`
- Create: `src/pages/LagDetectivePage.tsx`, `src/pages/AutoOptimizerPage.tsx`, `src/pages/DeepCleanerPage.tsx`, `src/pages/StartupPage.tsx`, `src/pages/SafeTweakerPage.tsx`, `src/pages/SettingsPage.tsx`
- Modify: `src/App.tsx`, `src/main.tsx`, `index.html`

**Interfaces:**
- Consumes: Typed Tauri IPC commands & events from `src-tauri/src/commands/mod.rs`.

- [ ] **Step 1: Implement `tokens.css` & `app.css` (Obsidian Dark + Daylight Light, `Instrument Sans` + `IBM Plex Mono`, micro-interactions, `prefers-reduced-motion`, `:focus-visible`)**
- [ ] **Step 2: Implement typed IPC layer (`src/lib/tauri.ts`) & accessible primitive components (`Icons`, `Sidebar`, `TopBar`, `ToggleSwitch`, `Modal`, `StateViews`, `Sparkline`)**
- [ ] **Step 3: Implement all 6 workspace pages wired to live Rust backend commands (zero fake metrics; honest Loading/Empty/Error/Admin-Required states; separate AppX panel with non-reversible warning)**
- [ ] **Step 4: Verify TypeScript & Vite production build (`npx tsc --noEmit && npm run build`) -> Expected: PASS with 0 errors**
- [ ] **Step 5: Commit**

---

### Task 9: Open-Source Docs, GitHub Actions CI & Full Verification
**Files:**
- Create: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `.github/workflows/ci.yml`

- [ ] **Step 1: Write `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, and `.github/workflows/ci.yml`**
- [ ] **Step 2: Run full backend & frontend verification suite**
  - `npx tsc --noEmit && npm run build`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
  - `cargo tauri build` (or verify debug/release binary launches cleanly and `--cli status` outputs real live JSON telemetry!)
- [ ] **Step 3: UI Visual & Interactive Verification**
  - Launch built/dev app or preview UI to verify Obsidian Dark & Daylight Light contrast, keyboard Tab navigation, empty/error states, and real telemetry rendering.
- [ ] **Step 4: Commit final verified state**

---

## Self-Review Checklist
- **Spec coverage:** Every module in spec (`Lag Detective`, `Hybrid Auto-Optimizer`, `Safe Debloater + Rollback + separate AppX`, `Deep Cleaner`, `Startup & Tasks`, `Tray & CLI`, `Privilege & Safety`, `UI/UX Light+Dark`, `GitHub CI/Docs`) maps directly to Tasks 1–9.
- **Type consistency:** `AppConfig`, `SystemSnapshot`, `GovernorProfile`, `CleanCategoryReport`, `StartupItem`, `TweakItem`, `BackupSnapshot`, `AppxPackageItem` align across Rust modules and `src/lib/tauri.ts`.
- **Review Focus tests:** All 5 high-risk failure modes have dedicated unit/integration test cases pinned to Tasks 2–6.
