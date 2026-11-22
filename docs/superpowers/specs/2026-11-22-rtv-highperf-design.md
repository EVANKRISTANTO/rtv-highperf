# rtv-highperf — Design Specification

> **Status:** Approved
> **Target Platform:** Windows 10 (19041+) / Windows 11 (x86_64)
> **License & Distribution:** Open-Source (MIT), GitHub Release (`EVANKRISTANTO/rtv-highperf`)

---

## 1. Intent & Why

Windows PCs frequently suffer from stuttering, high RAM/disk pressure, slow boot times, and FPS drops caused by unthrottled background processes, bloated startup entries, accumulated shader/update/temp caches, suboptimal power plans, and telemetry/background services. Existing "PC optimizer" tools are often closed-source, adware-ridden, or destructive PowerShell scripts without rollback.

**`rtv-highperf`** is a transparent, offline-first, open-source Windows performance optimizer written in Rust with a lightweight Tauri v2 desktop UI and System Tray agent. Every tweak is auditable, reversible via automatic snapshot backups, and safe by default.

### Goals
- Measurably reduce foreground lag/stutter in gaming, heavy multitasking, coding/compiling, and slow boot scenarios.
- Provide **real-time**, low-overhead background process & power governance via System Tray.
- Diagnose **actual** bottlenecks (CPU/RAM/Disk/Startup/Power) before recommending fixes—no fake "boost percentages" or snake-oil numbers.
- Guarantee **1-click rollback** via JSON/Registry snapshots before any system tweak or service change.
- Run **unprivileged by default**, requesting UAC elevation only for system-wide HKLM/service/Update-cache actions.
- Zero network telemetry, zero bundled ads, single clean GitHub repository with CI build workflow.

### Non-Goals & Anti-Goals
- **No kernel-mode drivers** (no custom `.sys` driver signing hurdles; uses documented Win32 APIs).
- **No blind RAM flushing every few seconds** (which causes page-fault stuttering); RAM trimming runs smartly only on idle background processes under real memory pressure.
- **No destructive Windows breaking** (never deletes Defender, Store core dependencies, or Windows Update engine silently; every tweak is documented and reversible).
- **No invented metrics or simulated scans** in the UI—if a metric cannot be read (e.g. requires elevation or unsupported hardware sensor), show an honest status instead of fabricated numbers.

---

## 2. Approaches Considered & Selected Architecture

| Approach | Summary | Trade-offs | Decision |
|---|---|---|---|
| **1. Tauri v2 (Rust Backend + React/TS UI + WebView2)** | Rust handles all Win32 APIs, telemetry, parallel cleaning, registry backup, and System Tray; React/TS renders the dashboard on Windows' pre-installed Edge WebView2. | Uses WebView2 (~25–40 MB RAM while UI open; <10 MB when window closed to tray). Needs Rust + MSVC + Node.js to build. | **Selected** — Best balance of modern UX, contributor accessibility on GitHub, and native Rust performance. |
| **2. Pure Rust GUI (`egui` / `iced`)** | 100% Rust immediate/retained GUI, zero WebView2. | Tiny binary (~5–10 MB), but typography, data tables, responsive polish, and theme transitions require heavy custom boilerplate. | Rejected |
| **3. Pure Rust (`Slint`)** | Declarative `.slint` compiled UI + Rust backend. | Lightweight, but smaller component ecosystem and licensing nuances (GPLv3/Royalty-free) complicate permissive MIT open-source contributions. | Rejected |

### Environment Prerequisite (Local PC — Phase 0)
- **Present:** `Node.js v24.22.0`, `npm 12.2.0`, `winget`, `Microsoft Edge WebView2`, `git`.
- **To Install in Phase 0 (Approved):** `Rustup` (`rustc`/`cargo`) and `Visual Studio 2022 Build Tools` (MSVC C++ workload + Windows 10/11 SDK) via `winget` so local compilation, `cargo test`, `cargo clippy`, and live Windows API testing run locally.

---

## 3. System Architecture

```
rtv-highperf/
├── src/                          # React 19 + TypeScript + Vite Frontend
│   ├── styles/tokens.css         # Design tokens (Dark/Light, Instrument Sans + IBM Plex Mono)
│   ├── components/               # Reusable accessible UI primitives & micro-interactions
│   ├── pages/                    # 6 workspace views matching the sidebar rail
│   └── lib/tauri.ts              # Typed invoke() & event listener wrappers
├── src-tauri/                    # Rust Core Engine + Tauri v2 Host
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs / lib.rs      # App bootstrap, single-instance lock, tray lifecycle
│       ├── commands/             # Tauri IPC commands exposed to frontend
│       ├── core/
│       │   ├── governor.rs       # Auto-Optimizer: foreground boost, EcoQoS, smart RAM trim, power plan
│       │   ├── detective.rs      # Lag Detective: live telemetry, bottleneck rules, Health Score (0-100)
│       │   ├── cleaner.rs        # Parallel deep cleaner (jwalk + rayon) with dry-run & recycle/delete
│       │   ├── startup.rs        # Registry Run/StartupApproved, Startup Folder, Scheduled Tasks
│       │   ├── tweaker.rs        # Declarative Safe Tweaks catalog + live state reader + JSON backup/rollback
│       │   ├── privilege.rs      # IsUserAnAdmin check & on-demand elevated worker/restart
│       │   └── safety.rs         # Protected PID/process blocklist, path traversal/system-dir guards
│       ├── tray.rs               # Native System Tray menu & background loop controller
│       └── cli.rs                # Optional headless CLI mode (`--cli scan|boost|clean|revert`)
└── .github/workflows/ci.yml      # Cargo test/clippy + frontend typecheck/build + Windows release artifact
```

### Privilege & Safety Model
1. **Unprivileged by Default:** App starts as standard user. Read-only diagnostics, HKCU startup items, user temp cleaning, and user-owned process priority/EcoQoS work immediately.
2. **On-Demand Elevation (Our Call):** App runs unprivileged by default. When an admin-only action is triggered (HKLM registry tweak, Windows Update cache, system service toggle, global power scheme, elevated startup task), items show an `Admin` badge and trigger a single **"Restart as Administrator"** (`ShellExecuteW("runas")`) flow that saves the current UI route to `config.json` and relaunches cleanly without maintaining a permanent background Windows service.
3. **Protected Process & Path Allowlist (`safety.rs`):**
   - Never lower priority, apply EcoQoS, or trim working set on critical Windows/security/audio processes (`System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `services.exe`, `lsass.exe`, `svchost.exe`, `dwm.exe`, `audiodg.exe`, `MsMpEng.exe`, `explorer.exe`, anti-cheat processes, and `rtv-highperf.exe` itself).
   - Cleaner uses a strict **explicit target-path allowlist** (never arbitrary recursive user-supplied root paths) and skips locked files gracefully without failing the batch.
4. **Automatic Rollback:** Every registry/service change writes a pre-change snapshot to `%APPDATA%\rtv-highperf\backups\snapshot-<timestamp>.json` containing exact hive, key path, value name, previous type/data (or `"__NOT_EXISTED__"`), and service previous `StartType`. Applying or restoring is atomic per batch with per-item result logging.

---

## 4. Detailed Feature Modules

### Module 1: Lag Detective (Overview & Diagnostics)
- **Live Telemetry Poll (1s–2s configurable, paused when window hidden to tray):**
  - CPU total % + per-logical-core usage (`sysinfo`), frequency, and power/thermal throttle detection (`CallNtPowerInformation` / `ProcessorInformation`).
  - RAM physical used/total/available (`GlobalMemoryStatusEx`), Commit charge %, and Top Memory Consumers.
  - Disk read/write throughput & busy indicator (via PDH counters or `sysinfo` disk I/O + Windows physical disk queue counter).
  - Startup entries count & enabled background services count.
  - Current active Windows Power Plan (`PowerGetActiveScheme`).
- **Transparent PC Health Score (0–100):**
  - Computed deterministically from real observed metrics (e.g., deductions for RAM > 80%, Disk saturation, excessive enabled startup items > 8, Power Saver mode on AC power, >5 GB cleanable junk, unthrottled background CPU hogs).
  - Clicking any deduction shows the exact formula reason and a direct **"Fix This"** action.
- **Top Resource Hogs Table:** Real-time sortable list (PID, Name, CPU %, RAM MB, Priority, EcoQoS active, quick actions: `Apply EcoQoS`, `Trim Working Set`, `Add to Boost Allowlist`, `Ignore`).

### Module 2: Hybrid Auto-Optimizer (Real-Time Governor & Profiles)
- **Operating Profiles:** `Off`, `Balanced Work`, `Gaming Focus`, `Coding & Compile` (plus custom per-app rules).
- **Foreground Boost & Background EcoQoS Loop (every 1.5s when enabled):**
  - Reads `GetForegroundWindow()` -> `GetWindowThreadProcessId()`.
  - If foreground app matches active profile or user boost list: sets priority (`ABOVE_NORMAL` or `HIGH`, never `REALTIME`) and disables EcoQoS (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED = 0`).
  - Previously boosted app reverts cleanly to its original priority when losing focus.
  - Background non-protected apps exceeding idle CPU/RAM threshold get **EcoQoS** (`PROCESS_POWER_THROTTLING_EXECUTION_SPEED` enabled) + optional `BELOW_NORMAL` priority (user-configurable, respects exclusion list e.g. Discord, OBS, Spotify, compiler workers when in Coding profile).
- **Smart RAM Trimmer:**
  - Triggered manually (`Trim Idle RAM Now`) or automatically when physical RAM usage > threshold (default 80%, cooldown 60s).
  - Calls `EmptyWorkingSet` / `K32EmptyWorkingSet` only on idle background processes (>30s unfocused, >150 MB working set, not in exclusion list). Reports actual MB freed by measuring working set delta.
- **Power Plan Automation:**
  - Reads installed Windows power schemes (`PowerEnumerate`).
  - Optionally switches to `High Performance` / `Ultimate Performance` when Gaming/Coding profile is active, and restores original scheme when disabled or on app exit.
- **Graceful Exit & Crash-Safe Power Recovery:** On quit (unless user opts to persist), restores all processes whose priorities/EcoQoS were altered during the session and restores the original power scheme. Persists `config.json` (active profile, auto-start preference, pre-override `original_power_scheme_guid` dirty flag) in `%APPDATA%\rtv-highperf\config.json` so after an unexpected crash or reboot, next launch detects any dangling power scheme override and restores the user's original scheme cleanly before re-applying.
- **Launch on Windows Startup (Optional Setting):** Toggle in `Settings & About` creates/removes a Windows Task Scheduler logon task (`rtv-highperf --minimized`, `HighestAvailable` only if user opts into elevated startup, otherwise standard user logon) and automatically resumes the last active Auto-Optimizer profile from `config.json`.

### Module 3: Safe Debloater & Registry Tweaker
- **Declarative Tweak Catalog (versioned in Rust):**
  - Categories:
    1. **Latency & Gaming:** Disable Xbox Game DVR background recording, Disable Mouse Acceleration (`MouseSpeed=0`), Hardware GPU Scheduling status, Network Throttling Index (`NetworkThrottlingIndex=0xFFFFFFFF`), TCP Nagle (`TCPNoDelay` / `TcpAckFrequency` per active NIC).
    2. **Privacy & Telemetry:** Disable Windows Telemetry (`AllowTelemetry`), Disable Advertising ID, Disable Tailored Experiences, Disable Activity History upload.
    3. **UI Snappiness:** Reduce `MenuShowDelay` (`400` -> `100` ms), Disable Windows tips/suggestions on Start/Lockscreen, Optional Disable Transparency Effects (`EnableTransparency=0`).
    4. **Background Services & Safe Tweaks:** Safe service start-type toggles (`DiagTrack`, `dmwappushservice`, `MapsBroker`, optional `SysMain` with SSD warning note). All Registry & Service tweaks are 100% covered by offline JSON snapshot rollback and `.reg` export.
- **Separate Non-Reversible Section — UWP Bloatware Removal (`Appx`):**
  - Placed in a distinct **"AppX Bloatware Manager"** tab/panel inside `Safe Tweaker` (separate from snapshotted Registry/Service tweaks) with an explicit non-reversible warning: *"Removing a Windows Store app uninstalls it for the current user (`Remove-AppxPackage`). Reinstalling requires Microsoft Store / internet access and is not covered by offline snapshot rollback."*
  - Targets **current-user packages only** (`Remove-AppxPackage`), never deletes system-critical packages (`Microsoft.WindowsStore`, `Microsoft.WindowsTerminal`, `Microsoft.DesktopAppInstaller` / winget, `Microsoft.WindowsCalculator`, `Microsoft.Windows.Photos`) or deprovisions all-user payloads.
- **Inspection & Safety UX:**
  - Every tweak displays: Title, Plain-language impact, Risk Badge (`Safe` / `Moderate` / `Advanced`), `Requires Admin` flag, Exact Registry Path / Service Name, Current Value vs Target Value.
  - **Backup & Rollback History Tab:** Lists all saved snapshots with timestamp, number of tweaks applied, diff preview, and 1-click **"Restore Snapshot"** + **"Export `.reg`"** option.

### Module 4: Ultra-Fast Deep Cleaner & Startup Manager
- **Deep Cleaner (`jwalk` + `rayon`):**
  - Scannable categories (each shows exact folder path, size, file count, and whether Admin is needed):
    - User Temp (`%TEMP%`) & Windows Temp (`C:\Windows\Temp`)
    - Windows Update Download Cache (`C:\Windows\SoftwareDistribution\Download`)
    - Windows Error Reporting & Crash Dumps (`%LOCALAPPDATA%\CrashDumps`, `C:\Windows\Minidump`)
    - GPU Shader Caches (DirectX Shader Cache, NVIDIA DXCache/GLCache, AMD DxCache)
    - Browser Caches (Edge, Chrome, Brave, Firefox — detects if browser is running and warns/skips locked files)
    - Developer Caches (opt-in, unchecked by default: `npm` cache, `pnpm` store temp, `pip` cache, Cargo registry cache)
    - Recycle Bin (query via `SHQueryRecycleBinW` / empty via `SHEmptyRecycleBinW`)
  - Supports **Dry-Run / Preview** (default scan first) and **Move to Recycle Bin vs Permanent Delete** option (default Permanent Delete for temp/cache with confirmation dialog).
- **Startup & Scheduled Tasks Manager:**
  - Reads `HKCU\...\Run`, `HKLM\...\Run`, `StartupApproved\Run` (so disabling follows native Windows Task Manager compatibility instead of deleting registry entries!), User & Common Startup folders, and non-Microsoft Scheduled Tasks.
  - Displays Publisher, Command path, Scope (`HKCU`/`HKLM`/`Task`), Status (`Enabled`/`Disabled`), and instant toggle + "Open File Location".

### Module 5: System Tray & CLI Companion
- **System Tray:**
  - Close button (`X`) minimizes to tray by default (configurable in Settings); option to destroy WebView when minimized to drop RAM usage near ~5–10 MB, recreating cleanly on tray double-click.
  - Tray context menu: Status summary (`Auto-Governor: Active | RAM: 62%`), `Boost Now`, `Profile` submenu, `Open Dashboard`, `Quit & Restore`.
- **CLI Subcommands (`rtv-highperf.exe --cli ...`):**
  - `status` (prints JSON/table of health score & top processes)
  - `clean --dry-run` / `clean --categories temp,shader`
  - `boost --trim-ram`
  - `revert --latest`

---

## 5. UI/UX & Visual Design Direction

- **Aesthetic Identity:** *Precision Telemetry Studio* — crisp, high-density engineering utility built for clarity and speed, not flashy "gamer RGB" or generic SaaS cards.
- **Theme Support:** Full **Obsidian Dark (default)** and **Daylight Slate (Light)** theme toggle, both verified for WCAG AA contrast (`>= 4.5:1` normal text, `>= 3:1` large text).
  - **Dark Tokens:** Canvas `#0B0E14`, Surface `#121722`, Elevated `#182030`, Border `#253047`, Text Primary `#EDF2F7`, Text Muted `#94A3B8`.
  - **Light Tokens:** Canvas `#F4F6F9`, Surface `#FFFFFF`, Elevated `#EAEFF5`, Border `#D0D9E4`, Text Primary `#0F172A`, Text Muted `#475569`.
  - **Core Accent & Semantic Signals (max 3):** Primary Signal **Phosphor Emerald (`#10B981` dark / `#059669` light)** for active boost & healthy states; **Signal Amber (`#F59E0B`)** for moderate warnings/elevated pressure; **Coral Red (`#EF4444`)** for critical bottlenecks/destructive actions. Zero blue-purple gradients, zero ambient glow spam, zero decorative emojis.
- **Typography (`typography-curation`):**
  - Display & UI Sans: `Instrument Sans` (bundled locally via `@fontsource/instrument-sans` so offline works 100%!).
  - Tabular Metrics, PIDs, Registry Paths, Sizes: `IBM Plex Mono` (`@fontsource/ibm-plex-mono`) with `font-variant-numeric: tabular-nums`.
- **Navigation & Layout:**
  - Collapsible left navigation rail (with keyboard shortcut & `aria-expanded`) grouping 6 real destinations:
    1. `Lag Detective` (Health Score + Bottleneck Findings + Live System Load & Top Processes)
    2. `Auto-Optimizer` (Governor toggle, Profiles, Exclusions, RAM Trimmer, Power Plan)
    3. `Deep Cleaner` (Category scan, size breakdown, clean confirmation)
    4. `Startup & Tasks` (Startup items & Scheduled Tasks toggle)
    5. `Safe Tweaker` (Categorized tweaks + Backup/Rollback history)
    6. `Settings & About` (Theme Light/Dark, Launch on Windows Startup `--minimized` + Resume Last Profile, Tray minimize/WebView-unload behavior, Elevation status, GitHub/MIT License info)
  - Every data panel explicitly implements **Loading**, **Empty**, and **Error/Permission** states with actionable next steps.
- **Micro-Interactions & Accessibility:**
  - Shared CSS motion tokens (`--ease-snappy`, `--ease-spring`, `--dur-fast`, `--dur-base`), GPU-only properties (`transform`, `opacity`), tactile button `:active` press (`scale(0.97)`), smooth active nav indicator, animated toggle switches, `:focus-visible` outlines, and `@media (prefers-reduced-motion: reduce)`.

---

## 6. Open-Source GitHub Readiness

- `LICENSE` (MIT License)
- `README.md` (Clear feature list, safety philosophy, screenshots placeholder, build instructions, CLI usage)
- `CONTRIBUTING.md` & `SECURITY.md` (How to add safe tweaks, responsible disclosure)
- `.github/workflows/ci.yml` (Runs `npm run build`, `cargo check`, `cargo test`, `cargo clippy` on `windows-latest`, and builds release installer on tag push)

---

## 7. Open Builder Decisions (Left to Implementation Plan)
- Exact chart visualization implementation (lightweight custom SVG sparklines vs canvas) for CPU/RAM history so bundle size and CPU overhead stay minimal.
- Exact crate versions resolved at setup (`tauri = "2"`, `windows = "0.58+"`, `sysinfo`, `jwalk`, `rayon`, `serde`, `clap`).
- Specific list of 15–20 initial tweaks in `tweaker.rs` (must all be verified against Windows 10/11 registry schemas before shipping).
