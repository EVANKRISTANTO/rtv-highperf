import { useEffect, useState } from "react";
import {
  IconCheck,
  IconExternalLink,
  IconMoon,
  IconShieldAlert,
  IconShieldCheck,
  IconSun,
} from "../components/Icons";
import { ToggleSwitch } from "../components/ToggleSwitch";
import {
  parseAppError,
  tauriApi,
  type AppConfig,
  type ParsedAppError,
  type PrivilegeStatus,
  type ThemeMode,
} from "../lib/tauri";

interface SettingsPageProps {
  connected: boolean;
  theme: ThemeMode;
  onChangeTheme: (theme: ThemeMode) => void;
  config: AppConfig | null;
  onConfigSaved: (next: AppConfig) => void;
  privilege: PrivilegeStatus | null;
  onRestartAsAdmin: () => void;
  elevating: boolean;
  onError: (err: ParsedAppError) => void;
}

export function SettingsPage({
  connected,
  theme,
  onChangeTheme,
  config,
  onConfigSaved,
  privilege,
  onRestartAsAdmin,
  elevating,
  onError,
}: SettingsPageProps) {
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [elevatedAutostart, setElevatedAutostart] = useState<boolean>(
    Boolean(config?.auto_start_elevated)
  );
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    setElevatedAutostart(Boolean(config?.auto_start_elevated));
  }, [config?.auto_start_elevated]);

  async function updateConfig(partial: Partial<AppConfig>, key: string) {
    if (!config) return;
    setSavingKey(key);
    setNotice(null);
    const next: AppConfig = { ...config, ...partial };
    try {
      await tauriApi.saveSettings(next);
      onConfigSaved(next);
      setNotice("Settings saved to config.json.");
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setSavingKey(null);
    }
  }

  async function handleAutostartToggle(enable: boolean, elevated: boolean) {
    setSavingKey("autostart");
    setNotice(null);
    try {
      await tauriApi.setAutostart(enable, elevated);
      setElevatedAutostart(elevated);
      if (config) {
        const next: AppConfig = {
          ...config,
          auto_start: enable,
          auto_start_elevated: elevated,
        };
        await tauriApi.saveSettings(next);
        onConfigSaved(next);
      }
      setNotice(
        enable
          ? `Windows startup task ${
              elevated ? "(HighestAvailable)" : "(Standard User)"
            } enabled with --minimized.`
          : "Windows startup task removed."
      );
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setSavingKey(null);
    }
  }

  return (
    <>
      {notice ? (
        <div className="banner banner-accent" role="status">
          <div className="banner-body">
            <span className="banner-icon">
              <IconCheck />
            </span>
            <p className="banner-title">{notice}</p>
          </div>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setNotice(null)}
          >
            Dismiss
          </button>
        </div>
      ) : null}

      <div className="two-col-split">
        <section className="panel" aria-label="Appearance Settings">
          <div className="panel-header">
            <div>
              <h2 className="panel-title">Appearance & Theme</h2>
              <p className="panel-subtitle">
                Persisted locally and synced to backend configuration
              </p>
            </div>
          </div>

          <div className="two-col-split" style={{ gap: 10 }}>
            <button
              type="button"
              className={`profile-card ${theme === "dark" ? "is-selected" : ""}`}
              aria-pressed={theme === "dark"}
              onClick={() => onChangeTheme("dark")}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <IconMoon size={16} />
                <span style={{ fontWeight: 700 }}>Obsidian Dark</span>
              </div>
              <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
                Low-glare telemetry palette for extended diagnostics
              </span>
            </button>

            <button
              type="button"
              className={`profile-card ${theme === "light" ? "is-selected" : ""}`}
              aria-pressed={theme === "light"}
              onClick={() => onChangeTheme("light")}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <IconSun size={16} />
                <span style={{ fontWeight: 700 }}>Daylight Slate</span>
              </div>
              <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
                High-contrast daylight workspace verified WCAG AA
              </span>
            </button>
          </div>
        </section>

        <section className="panel" aria-label="Privilege and Elevation Status">
          <div className="panel-header">
            <div>
              <h2 className="panel-title">Privilege & Elevation</h2>
              <p className="panel-subtitle">
                Unprivileged by default; elevate only when modifying HKLM, services, or Windows Update cache
              </p>
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="finding-item">
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                {privilege?.is_elevated ? (
                  <IconShieldCheck size={18} style={{ color: "var(--accent)" }} />
                ) : (
                  <IconShieldAlert size={18} style={{ color: "var(--warn)" }} />
                )}
                <div>
                  <div style={{ fontWeight: 600 }}>
                    {!connected
                      ? "Backend disconnected (Preview)"
                      : privilege?.is_elevated
                      ? "Running as Administrator"
                      : "Running as Standard User"}
                  </div>
                  <div
                    className="mono"
                    style={{ fontSize: 11, color: "var(--text-muted)", wordBreak: "break-all" }}
                  >
                    {privilege?.exe_path || "Executable path unavailable in browser preview"}
                  </div>
                </div>
              </div>

              {connected && !privilege?.is_elevated ? (
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  disabled={elevating}
                  onClick={onRestartAsAdmin}
                >
                  {elevating ? "Restarting..." : "Restart as Administrator"}
                </button>
              ) : null}
            </div>
          </div>
        </section>
      </div>

      <section className="panel" aria-label="Startup and System Tray Behavior">
        <div className="panel-header">
          <div>
            <h2 className="panel-title">Windows Startup & System Tray</h2>
            <p className="panel-subtitle">
              Controls Task Scheduler logon trigger (--minimized) and tray memory footprint
            </p>
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <ToggleSwitch
            checked={Boolean(config?.auto_start)}
            disabled={!connected || !config || savingKey === "autostart"}
            onChange={(next) => handleAutostartToggle(next, elevatedAutostart)}
            label="Launch on Windows Startup (--minimized)"
            description="Registers a Windows Task Scheduler logon task and resumes your last active Auto-Optimizer profile."
          />

          <ToggleSwitch
            checked={elevatedAutostart}
            disabled={
              !connected ||
              !config ||
              !config.auto_start ||
              savingKey === "autostart"
            }
            onChange={(next) => handleAutostartToggle(true, next)}
            label="Run Elevated at Startup (HighestAvailable)"
            description="Optional: grants the logon task elevated privileges without UAC prompts. Requires Administrator once to register."
          />

          <div style={{ borderTop: "1px solid var(--border-subtle)", paddingTop: 14 }}>
            <ToggleSwitch
              checked={config?.minimize_to_tray ?? true}
              disabled={!connected || !config || savingKey === "minimize_to_tray"}
              onChange={(next) =>
                updateConfig({ minimize_to_tray: next }, "minimize_to_tray")
              }
              label="Minimize to System Tray on Close"
              description="Closing the window hides it to the notification area while the Auto-Optimizer keeps running."
            />
          </div>

          <ToggleSwitch
            checked={config?.unload_webview_on_tray ?? false}
            disabled={
              !connected || !config || savingKey === "unload_webview_on_tray"
            }
            onChange={(next) =>
              updateConfig({ unload_webview_on_tray: next }, "unload_webview_on_tray")
            }
            label="Unload WebView When Minimized to Tray"
            description="Releases WebView2 memory while hidden in the tray and recreates the window cleanly on tray double-click."
          />
        </div>
      </section>

      <section className="panel" aria-label="About and Open Source License">
        <div className="panel-header">
          <div>
            <h2 className="panel-title">About rtv-highperf</h2>
            <p className="panel-subtitle">
              Offline-first Windows 10/11 performance optimizer and diagnostics studio
            </p>
          </div>
          <span className="badge badge-accent">MIT License</span>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <p style={{ margin: 0, fontSize: 13, color: "var(--text-secondary)", maxWidth: "70ch" }}>
            Zero network telemetry, zero background adware, and no kernel-mode drivers. Every
            registry and service tweak writes a local JSON snapshot before modification for
            deterministic rollback.
          </p>
          <div className="toolbar-group">
            <a
              href="https://github.com/EVANKRISTANTO/rtv-highperf"
              target="_blank"
              rel="noreferrer"
              className="btn btn-sm"
              style={{ textDecoration: "none" }}
            >
              <IconExternalLink size={14} />
              GitHub: EVANKRISTANTO/rtv-highperf
            </a>
            <span className="badge mono">v0.1.1</span>
            <span className="badge">Publisher: Rapticov Indonesia</span>
          </div>
        </div>
      </section>
    </>
  );
}
