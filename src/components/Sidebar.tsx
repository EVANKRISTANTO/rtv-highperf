import { useEffect, type ComponentType } from "react";
import type { RouteId } from "../lib/tauri";
import {
  IconCleaner,
  IconGear,
  IconGovernor,
  IconRadar,
  IconShieldAlert,
  IconShieldCheck,
  IconSidebarCollapse,
  IconStartup,
  IconTelemetryBrand,
  IconTweaker,
  type IconProps,
} from "./Icons";

export interface NavItem {
  id: RouteId;
  label: string;
  group: "Telemetry & Control" | "Maintenance & System";
  icon: ComponentType<IconProps>;
  shortcut: string;
}

export const NAV_ITEMS: NavItem[] = [
  {
    id: "detective",
    label: "Lag Detective",
    group: "Telemetry & Control",
    icon: IconRadar,
    shortcut: "1",
  },
  {
    id: "optimizer",
    label: "Auto-Optimizer",
    group: "Telemetry & Control",
    icon: IconGovernor,
    shortcut: "2",
  },
  {
    id: "cleaner",
    label: "Deep Cleaner",
    group: "Maintenance & System",
    icon: IconCleaner,
    shortcut: "3",
  },
  {
    id: "startup",
    label: "Startup & Tasks",
    group: "Maintenance & System",
    icon: IconStartup,
    shortcut: "4",
  },
  {
    id: "tweaker",
    label: "Safe Tweaker",
    group: "Maintenance & System",
    icon: IconTweaker,
    shortcut: "5",
  },
  {
    id: "settings",
    label: "Settings & About",
    group: "Maintenance & System",
    icon: IconGear,
    shortcut: "6",
  },
];

interface SidebarProps {
  activeRoute: RouteId;
  onSelectRoute: (route: RouteId) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  mobileOpen: boolean;
  onCloseMobile: () => void;
  isElevated: boolean | null;
}

export function Sidebar({
  activeRoute,
  onSelectRoute,
  collapsed,
  onToggleCollapse,
  mobileOpen,
  onCloseMobile,
  isElevated,
}: SidebarProps) {
  useEffect(() => {
    if (!mobileOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCloseMobile();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mobileOpen, onCloseMobile]);

  const groups: Array<NavItem["group"]> = [
    "Telemetry & Control",
    "Maintenance & System",
  ];

  return (
    <aside
      className={`sidebar-rail ${collapsed ? "is-collapsed" : ""} ${
        mobileOpen ? "is-mobile-open" : ""
      }`}
      aria-label="Primary navigation"
    >
      <div className="rail-header">
        {!collapsed ? (
          <div className="brand-mark">
            <span className="brand-glyph">
              <IconTelemetryBrand size={17} />
            </span>
            <div className="brand-text">
              <span className="brand-name">rtv-highperf</span>
              <span className="brand-sub">Telemetry Studio</span>
            </div>
          </div>
        ) : (
          <span className="brand-glyph" title="rtv-highperf">
            <IconTelemetryBrand size={17} />
          </span>
        )}
        <button
          type="button"
          className="rail-collapse-btn"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "Expand sidebar rail" : "Collapse sidebar rail"}
          title={collapsed ? "Expand sidebar (Ctrl+B)" : "Collapse sidebar (Ctrl+B)"}
          onClick={onToggleCollapse}
        >
          <IconSidebarCollapse collapsed={collapsed} />
        </button>
      </div>

      <nav className="rail-nav">
        {groups.map((group) => (
          <div key={group} className="rail-group">
            <div className="rail-group-label">{group}</div>
            {NAV_ITEMS.filter((item) => item.group === group).map((item) => {
              const Icon = item.icon;
              const isActive = activeRoute === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={`rail-item ${isActive ? "is-active" : ""}`}
                  aria-current={isActive ? "page" : undefined}
                  aria-label={item.label}
                  title={collapsed ? `${item.label} (Ctrl+${item.shortcut})` : undefined}
                  onClick={() => {
                    onSelectRoute(item.id);
                    onCloseMobile();
                  }}
                >
                  <span className="rail-item-icon">
                    <Icon size={17} />
                  </span>
                  <span className="rail-item-label">{item.label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <div className="rail-footer">
        <div
          className={`priv-pill ${isElevated ? "is-elevated" : ""}`}
          title={
            isElevated === null
              ? "Privilege status unknown in preview mode"
              : isElevated
              ? "Running with Administrator privileges"
              : "Running as Standard User (unprivileged)"
          }
        >
          {isElevated ? <IconShieldCheck size={14} /> : <IconShieldAlert size={14} />}
          {!collapsed ? (
            <span>
              {isElevated === null
                ? "Preview Mode"
                : isElevated
                ? "Administrator"
                : "Standard User"}
            </span>
          ) : null}
        </div>
      </div>
    </aside>
  );
}
