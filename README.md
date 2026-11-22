# rtv-highperf

An open-source, offline-first Windows 10/11 performance optimizer written in Rust with a Tauri v2 desktop interface, System Tray agent, and companion CLI.

`rtv-highperf` is designed for transparency and reversibility: it measures real system bottlenecks before suggesting changes, runs unprivileged by default, never phones home, and writes snapshot backups before modifying registry keys or service configurations.

---

## Core Pillars

### 1. Lag Detective (Live Telemetry & Bottleneck Diagnostics)
- **Live System Telemetry:** Monitors overall and per-logical-core CPU usage, physical RAM and commit charge, disk read/write throughput, active Windows power plan, enabled startup entry count, and cleanable cache size.
- **Deterministic Health Score (0–100):** Computes a transparent score from observed conditions (such as high RAM usage, disk saturation, excessive enabled startup items, Power Saver mode on AC power, and accumulated cache bloat). Each deduction explains the exact reason and links to a concrete remediation action.
- **Top Resource Hogs Table:** Inspect live processes (PID, name, CPU %, RAM MB, priority, and EcoQoS status) with per-process actions (`Apply EcoQoS`, `Trim Working Set`, `Add to Boost Allowlist`, or `Ignore`).
- **Honest Status Reporting:** No fabricated "boost percentages" or simulated scans. When a metric cannot be read without elevation or hardware support, the UI reports that status directly.

### 2. Hybrid Auto-Optimizer (Real-Time Governor & Profiles)
- **Operating Profiles:** Switch between `Off`, `Balanced Work`, `Gaming Focus`, and `Coding & Compile` profiles, plus custom per-application exclusion/boost rules.
- **Foreground Boost & Background EcoQoS:** Tracks the active foreground window (`GetForegroundWindow`) to prioritize the focused application (up to `ABOVE_NORMAL` / `HIGH`, never `REALTIME`) while applying Windows EcoQoS (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED`) to eligible idle background processes.
- **Smart Idle RAM Trimmer:** Avoids blind periodic RAM flushing that causes page-fault stutter. Working-set trimming (`EmptyWorkingSet`) targets idle, unfocused background processes above configurable thresholds when memory pressure rises, and reports the actual MB delta freed.
- **Power Plan Automation & Crash Recovery:** Optionally activates High/Ultimate Performance power schemes for demanding profiles and restores your original scheme on profile disable or exit. Persists a dirty-state marker in `%APPDATA%\rtv-highperf\config.json` so an unexpected crash or power loss cleanly restores your original power plan on the next launch.
- **Optional Startup Launch:** Can register a Windows Task Scheduler logon entry (`rtv-highperf --minimized`) to resume your last active profile in the background.

### 3. Deep Cleaner & Startup Manager
- **Parallel Deep Cleaner (`jwalk` + `rayon`):** Scans explicit, allowlisted cleanup targets with dry-run previews and size/file counts before deletion:
  - User Temp (`%TEMP%`) and Windows Temp (`C:\Windows\Temp`)
  - Windows Update Download Cache (`C:\Windows\SoftwareDistribution\Download`, requires Admin)
  - Windows Error Reporting & Crash Dumps (`%LOCALAPPDATA%\CrashDumps`, `C:\Windows\Minidump`)
  - GPU Shader Caches (DirectX, NVIDIA DXCache/GLCache, AMD DxCache)
  - Browser Caches (Edge, Chrome, Brave, Firefox — detects running browsers and gracefully skips locked files)
  - Developer Caches (opt-in, unchecked by default: `npm`, `pnpm`, `pip`, Cargo registry cache)
  - Recycle Bin (`SHQueryRecycleBinW` / `SHEmptyRecycleBinW`)
- **Move to Recycle Bin or Permanent Delete:** Configurable deletion mode with confirmation prompts and accurate `skipped_locked` reporting.
- **Startup & Scheduled Tasks Manager:** Lists `HKCU\...\Run`, `HKLM\...\Run`, Startup folders, and non-Microsoft Scheduled Tasks. Disabling Run entries uses Windows' native `StartupApproved\Run` binary flags so Task Manager and `rtv-highperf` stay in sync without deleting original entries.

### 4. Safe Tweaker & Separate AppX Bloatware Manager
- **Categorized Registry & Service Tweaks:** Covers Latency & Gaming, Privacy & Telemetry, UI Snappiness, and Safe Background Services. Every tweak displays its plain-language impact, risk level (`Safe`, `Moderate`, `Advanced`), whether Administrator privileges are required, the exact target registry path or service name, and current vs. target values.
- **Automatic JSON Snapshot Rollback & `.reg` Export:** Before applying any tweak batch, `rtv-highperf` saves a pre-change snapshot to `%APPDATA%\rtv-highperf\backups\snapshot-<timestamp>.json` (recording missing keys/values as `"NotExisted"` so rollback deletes newly created entries cleanly). Any snapshot can be restored in one click or exported as a standard `.reg` file.
- **Separate AppX Bloatware Manager:** Located in a distinct panel inside Safe Tweaker with an explicit non-reversible warning. Uninstalls UWP apps for the **current user only** (`Remove-AppxPackage`) and protects essential system packages (`Microsoft.WindowsStore`, `Microsoft.WindowsTerminal`, `Microsoft.DesktopAppInstaller`, `Microsoft.WindowsCalculator`, `Microsoft.Windows.Photos`). Reinstalling removed Store apps requires Microsoft Store or internet access and is intentionally separated from offline registry rollback.

---

## System Tray & Theme Support
- **Low-Overhead System Tray:** Closing the window can minimize `rtv-highperf` to the System Tray and optionally unload the WebView2 UI until reopened from the tray icon. Tray menu provides quick status, `Boost Now`, profile switching, `Open Dashboard`, and `Quit & Restore`.
- **Obsidian Dark & Daylight Light Themes:** Full dark and light themes with bundled local `Instrument Sans` and `IBM Plex Mono` fonts (`tabular-nums`), keyboard navigation, and `prefers-reduced-motion` support.

---

## Safety & Unprivileged-by-Default Model

`rtv-highperf` is built around strict operational guardrails:

1. **Unprivileged by Default:** Starts as a standard user. Live diagnostics, `HKCU` startup entries, user temp cleaning, and user-owned process priority/EcoQoS adjustments work without UAC elevation.
2. **On-Demand Elevation Only:** System-wide actions (`HKLM` tweaks, Windows Update cache, system services, global power schemes, elevated scheduled tasks) are marked with an `Admin` badge. Triggering one uses an explicit **"Restart as Administrator"** (`ShellExecuteW("runas")`) flow rather than installing a permanent privileged background service.
3. **No Kernel Drivers:** Uses documented Win32 APIs (`windows` crate) with zero custom `.sys` kernel drivers.
4. **Protected Process Blocklist:** The Auto-Optimizer and RAM trimmer never lower priority, apply EcoQoS, or trim working sets on critical Windows, security, audio, anti-cheat, or self processes (including `System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `services.exe`, `lsass.exe`, `svchost.exe`, `dwm.exe`, `audiodg.exe`, `MsMpEng.exe`, `explorer.exe`, and `rtv-highperf.exe`).
5. **Explicit Cleaner Allowlist:** Deep Cleaner only traverses predefined allowlisted locations—never arbitrary recursive user-supplied root directories.
6. **100% Offline-First:** Zero network requests, zero telemetry, zero ads.

See [SECURITY.md](SECURITY.md) for full details on our security model and vulnerability reporting.

---

## CLI Usage

`rtv-highperf.exe` includes a headless CLI mode for scripting and quick terminal checks:

```powershell
# Print live system health score, power plan, and top resource consumers
rtv-highperf.exe --cli status

# Preview cleanable caches without deleting files
rtv-highperf.exe --cli clean --dry-run

# Clean specific cache categories (e.g. user temp and GPU shader caches)
rtv-highperf.exe --cli clean --categories user_temp,gpu_shaders

# Run a manual idle-RAM trim pass
rtv-highperf.exe --cli boost --trim-ram

# Restore the most recent Safe Tweaker snapshot
rtv-highperf.exe --cli revert --latest
```

To launch the desktop app directly into the System Tray (used by the optional Windows startup task):

```powershell
rtv-highperf.exe --minimized
```

---

## Build from Source

### Prerequisites
- **Operating System:** Windows 10 (build 19041+) or Windows 11 (`x86_64-pc-windows-msvc`)
- **Rust Toolchain:** Stable MSVC toolchain via [rustup](https://rustup.rs/) (`x86_64-pc-windows-msvc`)
- **C++ Build Tools:** Visual Studio 2022 Build Tools with the **Desktop development with C++** (`VCTools`) workload and Windows 10/11 SDK
- **Node.js & npm:** Node.js 24+
- **WebView2 Runtime:** Microsoft Edge WebView2 (pre-installed on Windows 10/11)

### Setup & Verification Commands

```powershell
# 1. Install frontend dependencies
npm ci

# 2. Type-check and build the React/TypeScript frontend
npx tsc --noEmit
npm run build

# 3. Run Rust backend unit tests and Clippy lints
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# 4. Run in development mode or build a release installer
npx tauri dev
npx tauri build
```

> **Note:** On Windows PowerShell setups with a restricted script execution policy, invoke npm/npx via `cmd /c npm ...` and `cmd /c npx ...` if `.ps1` shims are blocked.

---

## Contributing

Contributions are welcome, especially well-documented, reversible Windows tweaks and diagnostics improvements. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

`rtv-highperf` is open-source software licensed under the [MIT License](LICENSE).
