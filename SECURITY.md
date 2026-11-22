# Security Policy

## Local-Only & Zero-Telemetry Guarantee

`rtv-highperf` is strictly **offline-first**:

- **Zero network telemetry:** The application makes no outbound network requests, sends no analytics or crash reports, and includes no auto-updater phone-home endpoints or third-party ad SDKs.
- **No kernel-mode drivers:** All system inspection and optimization uses documented user-mode Win32 APIs (`windows` crate). No `.sys` kernel drivers are installed or loaded.
- **Unprivileged by default:** `rtv-highperf` launches as a standard user and never installs a persistent elevated Windows service. System-wide operations (`HKLM` registry writes, Windows Update cache cleaning, service `StartType` changes, global power plan changes) explicitly require on-demand UAC elevation via `ShellExecuteW("runas")`.
- **Fail-closed safety boundaries:**
  - **Protected process blocklist (`core/safety.rs`):** Critical Windows, security, audio, anti-cheat, and self processes (`System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `services.exe`, `lsass.exe`, `svchost.exe`, `dwm.exe`, `audiodg.exe`, `MsMpEng.exe`, `explorer.exe`, `rtv-highperf.exe`, etc.) are blocked from priority downgrades, EcoQoS throttling, and working-set trimming.
  - **Explicit cleaner allowlist (`core/cleaner.rs`):** Directory traversal and deletion are restricted to predefined cache/temp paths and reject path traversal (`..`) or system directories such as `C:\Windows\System32`.
  - **Pre-change snapshots (`core/tweaker.rs`):** Every Registry and Service change records a JSON rollback snapshot under `%APPDATA%\rtv-highperf\backups\` before modifying system state.

---

## Supported Versions

Security fixes apply to the latest release and the `main` branch of `EVANKRISTANTO/rtv-highperf` on Windows 10 (19041+) and Windows 11 (`x86_64`).

---

## Reporting a Vulnerability

If you discover a security vulnerability—such as a path traversal bypass in the cleaner, an unsafe UAC/command-injection vector in elevation or CLI handling, a protected-process bypass, or an unintended privilege escalation—please **do not** open a public GitHub issue with exploit details.

Instead, report it privately through GitHub:

1. Navigate to the **Security** tab of the `EVANKRISTANTO/rtv-highperf` repository on GitHub.
2. Select **Advisories** -> **Report a vulnerability** (GitHub Private Vulnerability Reporting).
3. Include:
   - Affected component/file (e.g., `core/cleaner.rs`, `core/privilege.rs`, `cli.rs`, or Tauri IPC command)
   - Windows version and elevation state (standard user vs. Administrator)
   - Clear reproduction steps or proof-of-concept
   - Potential security or system-integrity impact

We will acknowledge reports, coordinate a fix and regression test, and credit reporters in the advisory unless anonymity is requested.
