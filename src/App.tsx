import { useCallback, useEffect, useState } from "react";
import { NAV_ITEMS, Sidebar } from "./components/Sidebar";
import { ErrorBanner, PreviewModeBanner } from "./components/StateViews";
import { TopBar } from "./components/TopBar";
import {
  isTauriRuntime,
  listenGovernorUpdate,
  listenTelemetryTick,
  parseAppError,
  tauriApi,
  type AppConfig,
  type GovernorState,
  type ParsedAppError,
  type PrivilegeStatus,
  type RouteId,
  type SystemSnapshot,
  type ThemeMode,
} from "./lib/tauri";
import { AutoOptimizerPage } from "./pages/AutoOptimizerPage";
import { DeepCleanerPage } from "./pages/DeepCleanerPage";
import { LagDetectivePage } from "./pages/LagDetectivePage";
import { SafeTweakerPage } from "./pages/SafeTweakerPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StartupPage } from "./pages/StartupPage";

const ROUTE_META: Record<RouteId, { title: string; subtitle: string }> = {
  detective: {
    title: "Lag Detective",
    subtitle: "Live system telemetry, deterministic Health Score, and bottleneck fixes",
  },
  optimizer: {
    title: "Auto-Optimizer",
    subtitle: "Foreground boost, background EcoQoS, smart RAM trim, and power plans",
  },
  cleaner: {
    title: "Deep Cleaner",
    subtitle: "Allowlisted temp, shader, update, and browser cache scanner",
  },
  startup: {
    title: "Startup & Tasks",
    subtitle: "Registry Run keys, StartupApproved flags, and Scheduled Tasks",
  },
  tweaker: {
    title: "Safe Tweaker",
    subtitle: "Reversible Registry/Service tweaks, snapshots, and AppX removal",
  },
  settings: {
    title: "Settings & About",
    subtitle: "Theme, Windows startup task, tray memory mode, and UAC elevation",
  },
};

const MAX_HISTORY = 60;

export default function App() {
  const connected = isTauriRuntime();

  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      const stored = localStorage.getItem("rtv-theme");
      if (stored === "light" || stored === "dark") return stored;
    } catch {
      // ignore storage failure
    }
    return "dark";
  });

  const [activeRoute, setActiveRoute] = useState<RouteId>("detective");
  const [railCollapsed, setRailCollapsed] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);

  const [config, setConfig] = useState<AppConfig | null>(null);
  const [privilege, setPrivilege] = useState<PrivilegeStatus | null>(null);
  const [snapshot, setSnapshot] = useState<SystemSnapshot | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [ramHistory, setRamHistory] = useState<number[]>([]);
  const [governorState, setGovernorState] = useState<GovernorState | null>(null);
  const [loadingInitial, setLoadingInitial] = useState(connected);
  const [errorBanner, setErrorBanner] = useState<ParsedAppError | null>(null);
  const [elevating, setElevating] = useState(false);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    try {
      localStorage.setItem("rtv-theme", theme);
    } catch {
      // ignore storage failure
    }
  }, [theme]);

  const pushSnapshot = useCallback((snap: SystemSnapshot) => {
    setSnapshot(snap);
    setCpuHistory((prev) => [...prev.slice(-(MAX_HISTORY - 1)), snap.cpu_total_pct]);
    const ramPct =
      snap.ram_total_bytes > 0
        ? (snap.ram_used_bytes / snap.ram_total_bytes) * 100
        : 0;
    setRamHistory((prev) => [...prev.slice(-(MAX_HISTORY - 1)), ramPct]);
  }, []);

  const refreshTelemetry = useCallback(async () => {
    if (!connected) return;
    try {
      const snap = await tauriApi.getSystemSnapshot();
      pushSnapshot(snap);
    } catch (err) {
      setErrorBanner(parseAppError(err));
    }
  }, [connected, pushSnapshot]);

  useEffect(() => {
    if (!connected) return;
    let cancelled = false;

    async function bootstrap() {
      setLoadingInitial(true);
      try {
        const [cfg, priv, gov, snap] = await Promise.all([
          tauriApi.getConfig(),
          tauriApi.getPrivilegeStatus(),
          tauriApi.getGovernorState(),
          tauriApi.getSystemSnapshot(),
        ]);
        if (cancelled) return;
        setConfig(cfg);
        setPrivilege(priv);
        setGovernorState(gov);
        pushSnapshot(snap);
        if (cfg.theme === "light" || cfg.theme === "dark") {
          setTheme(cfg.theme);
        }
        if (
          cfg.last_route &&
          NAV_ITEMS.some((item) => item.id === cfg.last_route)
        ) {
          setActiveRoute(cfg.last_route as RouteId);
        }
      } catch (err) {
        if (!cancelled) {
          setErrorBanner(parseAppError(err));
        }
      } finally {
        if (!cancelled) {
          setLoadingInitial(false);
        }
      }
    }

    bootstrap();

    let unlistenTick: (() => void) | undefined;
    let unlistenGov: (() => void) | undefined;

    listenTelemetryTick((snap) => {
      if (!cancelled) pushSnapshot(snap);
    }).then((fn) => {
      unlistenTick = fn;
    });

    listenGovernorUpdate((gov) => {
      if (!cancelled) setGovernorState(gov);
    }).then((fn) => {
      unlistenGov = fn;
    });

    return () => {
      cancelled = true;
      unlistenTick?.();
      unlistenGov?.();
    };
  }, [connected, pushSnapshot]);

  // Keyboard shortcuts: Ctrl+B toggles sidebar rail, Ctrl+1..6 switches route
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "b") {
        e.preventDefault();
        setRailCollapsed((v) => !v);
        return;
      }
      const match = NAV_ITEMS.find((item) => item.shortcut === e.key);
      if (match) {
        e.preventDefault();
        setActiveRoute(match.id);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const handleThemeChange = useCallback(
    async (nextTheme: ThemeMode) => {
      setTheme(nextTheme);
      if (connected && config) {
        const updated: AppConfig = { ...config, theme: nextTheme };
        setConfig(updated);
        try {
          await tauriApi.saveSettings(updated);
        } catch (err) {
          setErrorBanner(parseAppError(err));
        }
      }
    },
    [connected, config]
  );

  const handleRestartAsAdmin = useCallback(async () => {
    if (!connected) return;
    setElevating(true);
    try {
      await tauriApi.restartAsAdmin(activeRoute);
    } catch (err) {
      setErrorBanner(parseAppError(err));
      setElevating(false);
    }
  }, [connected, activeRoute]);

  const meta = ROUTE_META[activeRoute];

  return (
    <div className="app-shell">
      <Sidebar
        activeRoute={activeRoute}
        onSelectRoute={(route) => {
          setErrorBanner(null);
          setActiveRoute(route);
        }}
        collapsed={railCollapsed}
        onToggleCollapse={() => setRailCollapsed((v) => !v)}
        mobileOpen={mobileNavOpen}
        onCloseMobile={() => setMobileNavOpen(false)}
        isElevated={connected ? privilege?.is_elevated ?? false : null}
      />

      <div className="main-column">
        <TopBar
          title={meta.title}
          subtitle={meta.subtitle}
          theme={theme}
          onToggleTheme={() =>
            handleThemeChange(theme === "dark" ? "light" : "dark")
          }
          onOpenMobileNav={() => setMobileNavOpen((v) => !v)}
          connected={connected}
          governorState={governorState}
          isElevated={connected ? privilege?.is_elevated ?? false : null}
          onRestartAsAdmin={handleRestartAsAdmin}
          elevating={elevating}
        />

        <main className="workspace-scroll">
          <div className="workspace-inner">
            {!connected ? <PreviewModeBanner /> : null}

            <ErrorBanner
              error={errorBanner}
              onDismiss={() => setErrorBanner(null)}
              onElevate={connected ? handleRestartAsAdmin : undefined}
              elevating={elevating}
            />

            {activeRoute === "detective" ? (
              <LagDetectivePage
                connected={connected}
                snapshot={snapshot}
                cpuHistory={cpuHistory}
                ramHistory={ramHistory}
                loading={loadingInitial}
                onRefresh={refreshTelemetry}
                onError={setErrorBanner}
              />
            ) : null}

            {activeRoute === "optimizer" ? (
              <AutoOptimizerPage
                connected={connected}
                config={config}
                governorState={governorState}
                onConfigSaved={setConfig}
                onGovernorUpdated={setGovernorState}
                onError={setErrorBanner}
              />
            ) : null}

            {activeRoute === "cleaner" ? (
              <DeepCleanerPage
                connected={connected}
                onError={setErrorBanner}
              />
            ) : null}

            {activeRoute === "startup" ? (
              <StartupPage
                connected={connected}
                isElevated={connected ? privilege?.is_elevated ?? false : null}
                onRestartAsAdmin={handleRestartAsAdmin}
                elevating={elevating}
                onError={setErrorBanner}
              />
            ) : null}

            {activeRoute === "tweaker" ? (
              <SafeTweakerPage
                connected={connected}
                onError={setErrorBanner}
              />
            ) : null}

            {activeRoute === "settings" ? (
              <SettingsPage
                connected={connected}
                theme={theme}
                onChangeTheme={handleThemeChange}
                config={config}
                onConfigSaved={setConfig}
                privilege={privilege}
                onRestartAsAdmin={handleRestartAsAdmin}
                elevating={elevating}
                onError={setErrorBanner}
              />
            ) : null}
          </div>
        </main>
      </div>
    </div>
  );
}
