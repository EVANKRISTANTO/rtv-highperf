# Contributing to rtv-highperf

Thank you for helping improve `rtv-highperf`. Because this tool touches Windows processes, startup entries, filesystem caches, services, and the Windows Registry, every change must prioritize **safety, transparency, and reversibility**.

---

## Repository Layout

- `src/` — React 19 + TypeScript + Vite frontend (`Obsidian Dark` & `Daylight Light` themes, typed IPC wrapper in `src/lib/tauri.ts`)
- `src-tauri/src/core/` — Core Rust engine:
  - `safety.rs` — Protected process blocklist and cleaner path allowlist
  - `config.rs` — Persistent configuration and crash-safe dirty power-scheme state
  - `privilege.rs` — Elevation detection and on-demand `ShellExecuteW("runas")` restart flow
  - `detective.rs` — Live telemetry and deterministic Health Score engine
  - `governor.rs` — Foreground boost, EcoQoS, idle RAM trimming, power plan automation
  - `cleaner.rs` — Parallel deep cleaner (`jwalk` + `rayon`)
  - `startup.rs` — `Run` / `StartupApproved\Run`, Startup folders, and Scheduled Tasks
  - `tweaker.rs` — Declarative Safe Tweaks catalog, JSON snapshot/rollback, `.reg` export, and separate AppX panel
- `src-tauri/src/commands/` — Tauri IPC `#[tauri::command]` handlers
- `src-tauri/src/tray.rs` & `cli.rs` — System Tray integration and headless CLI

---

## Guidelines for Adding Safe, Reversible Tweaks

All new Registry or Service tweaks belong in `src-tauri/src/core/tweaker.rs` and must satisfy every requirement below:

1. **Verified Windows Schema:**
   - Confirm the exact behavior against documented Windows 10 (19041+) and Windows 11 schemas.
   - Do not add undocumented "placebo" registry keys or destructive tweaks that disable Windows Defender, core Microsoft Store dependencies, or the Windows Update engine.

2. **Exact Metadata & Documentation:**
   Every tweak definition must explicitly document:
   - **Unique ID & Category** (`Latency & Gaming`, `Privacy & Telemetry`, `UI Snappiness`, or `Background Services`)
   - **Title & Plain-Language Impact:** Explain clearly what changes and any side effects (e.g., disabling `SysMain` on HDDs vs. SSDs).
   - **Risk Level:** `Safe`, `Moderate`, or `Advanced`.
   - **Privilege Requirement:** `requires_admin: true` for `HKLM` keys or system services; `false` for `HKCU` user-scoped keys.
   - **Exact Target:** Full registry hive (`HKCU` / `HKLM`), key path, value name, value type (`REG_DWORD`, `REG_SZ`, etc.), target value, and default/revert behavior — or exact Windows service name and target `StartType`.

3. **Guaranteed Snapshot & Rollback Semantics:**
   - Before applying changes, `tweaker.rs` must read the live prior state and persist it into `%APPDATA%\rtv-highperf\backups\snapshot-<timestamp>.json`.
   - If a registry key or value does not exist prior to applying the tweak, the snapshot **must** record `"NotExisted"` (or equivalent missing-key marker) so that rollback and `.reg` export cleanly delete the added value instead of writing empty placeholder data.
   - Every tweak must be able to read its live state accurately (`is_applied`), revert via snapshot restoration, and export valid `.reg` syntax.

4. **Non-Reversible AppX Packages Stay Separate:**
   - Current-user UWP package removal (`Remove-AppxPackage`) must never be mixed into the snapshotted Registry/Service catalog.
   - Critical system packages (`Microsoft.WindowsStore`, `Microsoft.WindowsTerminal`, `Microsoft.DesktopAppInstaller`, `Microsoft.WindowsCalculator`, `Microsoft.Windows.Photos`, etc.) must remain protected in the AppX allowlist.

5. **Unit Tests Required:**
   - Add unit tests in `src-tauri/src/core/tweaker.rs` covering:
     - Snapshot capture when the target value previously existed vs. did not exist (`"NotExisted"`).
     - `.reg` export formatting for your tweak's value type and deletion syntax.
     - Protected AppX allowlist enforcement (if touching AppX filtering).
   - If you add a cleaner target or process governance rule, add tests verifying:
     - `safety::is_allowed_clean_root` / `safety::is_protected_process` guards remain intact.
     - Locked files are skipped without aborting the batch.

---

## Development & Verification Checklist

Before submitting a pull request, run all checks locally on Windows (or verify via the Windows CI workflow):

```powershell
# Frontend typecheck and production bundle
npx tsc --noEmit
npm run build

# Backend unit tests and strict Clippy lints
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Additional PR expectations:
- Keep changes focused and modular; avoid duplicating safety guards across commands.
- Maintain WCAG AA contrast in both Obsidian Dark and Daylight Light themes, and respect `prefers-reduced-motion` for UI additions.
- Never introduce network calls, analytics, or fabricated telemetry/metrics.
- In your PR description, list any registry paths, service names, or filesystem paths touched and confirm how rollback was tested.
