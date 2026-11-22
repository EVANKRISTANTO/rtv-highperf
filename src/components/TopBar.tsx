import type { GovernorState, ThemeMode } from "../lib/tauri";
import { IconMenu, IconMoon, IconShieldAlert, IconSun } from "./Icons";

interface TopBarProps {
  title: string;
  subtitle: string;
  theme: ThemeMode;
  onToggleTheme: () => void;
  onOpenMobileNav: () => void;
  connected: boolean;
  governorState: GovernorState | null;
  isElevated: boolean | null;
  onRestartAsAdmin: () => void;
  elevating: boolean;
}

export function TopBar({
  title,
  subtitle,
  theme,
  onToggleTheme,
  onOpenMobileNav,
  connected,
  governorState,
  isElevated,
  onRestartAsAdmin,
  elevating,
}: TopBarProps) {
  const govActive = Boolean(governorState?.enabled && governorState.profile !== "Off");

  return (
    <header className="topbar">
      <div className="topbar-left">
        <button
          type="button"
          className="mobile-menu-btn"
          aria-label="Open navigation menu"
          onClick={onOpenMobileNav}
        >
          <IconMenu />
        </button>
        <div className="topbar-title-wrap">
          <h1 className="topbar-title">{title}</h1>
          <span className="topbar-sub">{subtitle}</span>
        </div>
      </div>

      <div className="topbar-actions">
        {connected ? (
          <div className={`status-chip ${govActive ? "is-active" : ""}`}>
            <span className="status-dot" />
            <span>
              {govActive ? `Governor: ${governorState?.profile}` : "Governor: Idle"}
            </span>
          </div>
        ) : (
          <div className="status-chip is-warn">
            <span className="status-dot" />
            <span>Preview Mode</span>
          </div>
        )}

        {connected && isElevated === false ? (
          <button
            type="button"
            className="btn btn-sm"
            onClick={onRestartAsAdmin}
            disabled={elevating}
            title="Relaunch with UAC elevation for HKLM tweaks, services, and Windows Update cache"
          >
            <IconShieldAlert size={14} />
            {elevating ? "Restarting..." : "Elevate"}
          </button>
        ) : null}

        <button
          type="button"
          className="btn btn-sm"
          onClick={onToggleTheme}
          aria-label={
            theme === "dark" ? "Switch to Daylight Light theme" : "Switch to Obsidian Dark theme"
          }
          title={theme === "dark" ? "Switch to Light theme" : "Switch to Dark theme"}
        >
          {theme === "dark" ? <IconSun size={15} /> : <IconMoon size={15} />}
          <span>{theme === "dark" ? "Light" : "Dark"}</span>
        </button>
      </div>
    </header>
  );
}
