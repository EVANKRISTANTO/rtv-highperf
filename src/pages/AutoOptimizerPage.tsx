import { useEffect, useState, type FormEvent } from "react";
import { IconCheck, IconPlus, IconRefresh, IconX } from "../components/Icons";
import { EmptyState, LoadingState } from "../components/StateViews";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { formatTimestamp } from "../lib/format";
import {
  parseAppError,
  tauriApi,
  type AppConfig,
  type GovernorProfile,
  type GovernorState,
  type ParsedAppError,
  type PowerPlanInfo,
  type RamTrimReport,
} from "../lib/tauri";

interface AutoOptimizerPageProps {
  connected: boolean;
  config: AppConfig | null;
  governorState: GovernorState | null;
  onConfigSaved: (next: AppConfig) => void;
  onGovernorUpdated: (next: GovernorState) => void;
  onError: (err: ParsedAppError) => void;
}

const PROFILES: Array<{
  id: GovernorProfile;
  label: string;
  summary: string;
}> = [
  {
    id: "Off",
    label: "Off",
    summary: "Governor paused. No background priority or power plan overrides active.",
  },
  {
    id: "BalancedWork",
    label: "Balanced Work",
    summary:
      "Applies EcoQoS to idle background hogs while keeping active productivity windows responsive.",
  },
  {
    id: "GamingFocus",
    label: "Gaming Focus",
    summary:
      "Boosts foreground game priority, throttles background updaters, and switches to High/Ultimate Performance power plan.",
  },
  {
    id: "CodingCompile",
    label: "Coding & Compile",
    summary:
      "Keeps IDE, terminal, and compiler workers unthrottled while reigning in unrelated background browsers.",
  },
];

export function AutoOptimizerPage({
  connected,
  config,
  governorState,
  onConfigSaved,
  onGovernorUpdated,
  onError,
}: AutoOptimizerPageProps) {
  const [powerPlans, setPowerPlans] = useState<PowerPlanInfo[]>([]);
  const [loadingPlans, setLoadingPlans] = useState(false);
  const [trimReport, setTrimReport] = useState<RamTrimReport | null>(null);
  const [trimming, setTrimming] = useState(false);
  const [savingThreshold, setSavingThreshold] = useState(false);
  const [thresholdDraft, setThresholdDraft] = useState<number>(
    config?.ram_trim_threshold_pct ?? 80
  );
  const [newExclusion, setNewExclusion] = useState("");
  const [busyPlanGuid, setBusyPlanGuid] = useState<string | null>(null);

  useEffect(() => {
    if (config) {
      setThresholdDraft(config.ram_trim_threshold_pct);
    }
  }, [config]);

  async function loadPowerPlans() {
    if (!connected) return;
    setLoadingPlans(true);
    try {
      const plans = await tauriApi.listPowerPlans();
      setPowerPlans(plans);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setLoadingPlans(false);
    }
  }

  useEffect(() => {
    loadPowerPlans();
  }, [connected]);

  async function handleToggleGovernor(enabled: boolean) {
    const currentProfile = governorState?.profile ?? config?.active_profile ?? "BalancedWork";
    const nextProfile: GovernorProfile =
      enabled && currentProfile === "Off" ? "BalancedWork" : currentProfile;
    try {
      const next = await tauriApi.setGovernorState(enabled, nextProfile);
      onGovernorUpdated(next);
    } catch (err) {
      onError(parseAppError(err));
    }
  }

  async function handleSelectProfile(profile: GovernorProfile) {
    const enabled = profile !== "Off";
    try {
      const next = await tauriApi.setGovernorState(enabled, profile);
      onGovernorUpdated(next);
      if (config) {
        const updatedCfg: AppConfig = {
          ...config,
          active_profile: profile,
          governor_enabled: enabled,
        };
        await tauriApi.saveSettings(updatedCfg);
        onConfigSaved(updatedCfg);
      }
    } catch (err) {
      onError(parseAppError(err));
    }
  }

  async function handleSaveThreshold() {
    if (!config) return;
    setSavingThreshold(true);
    try {
      const nextCfg: AppConfig = {
        ...config,
        ram_trim_threshold_pct: thresholdDraft,
      };
      await tauriApi.saveSettings(nextCfg);
      onConfigSaved(nextCfg);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setSavingThreshold(false);
    }
  }

  async function handleTrimNow() {
    setTrimming(true);
    try {
      const res = await tauriApi.trimIdleRamNow();
      setTrimReport(res);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setTrimming(false);
    }
  }

  async function handleSetPowerPlan(guid: string) {
    setBusyPlanGuid(guid);
    try {
      await tauriApi.setPowerPlan(guid);
      await loadPowerPlans();
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyPlanGuid(null);
    }
  }

  async function handleAddExclusion(e: FormEvent) {
    e.preventDefault();
    if (!config) return;
    const cleaned = newExclusion.trim();
    if (!cleaned) return;
    if (config.exclusions.some((x) => x.toLowerCase() === cleaned.toLowerCase())) {
      setNewExclusion("");
      return;
    }
    const nextCfg: AppConfig = {
      ...config,
      exclusions: [...config.exclusions, cleaned],
    };
    try {
      await tauriApi.saveSettings(nextCfg);
      onConfigSaved(nextCfg);
      setNewExclusion("");
    } catch (err) {
      onError(parseAppError(err));
    }
  }

  async function handleRemoveExclusion(name: string) {
    if (!config) return;
    const nextCfg: AppConfig = {
      ...config,
      exclusions: config.exclusions.filter((x) => x !== name),
    };
    try {
      await tauriApi.saveSettings(nextCfg);
      onConfigSaved(nextCfg);
    } catch (err) {
      onError(parseAppError(err));
    }
  }

  if (!connected) {
    return (
      <EmptyState
        title="Auto-Optimizer requires the Windows desktop backend"
        detail="Process priority boosting, Windows EcoQoS throttling, working-set RAM trimming, and power plan switching execute through Win32 APIs in the Tauri backend."
      />
    );
  }

  const activeProfile = governorState?.profile ?? config?.active_profile ?? "Off";
  const isEnabled = Boolean(governorState?.enabled && activeProfile !== "Off");

  return (
    <>
      <section className="panel" aria-label="Governor Master Control">
        <div className="panel-header">
          <div>
            <h2 className="panel-title">Hybrid Auto-Optimizer Governor</h2>
            <p className="panel-subtitle">
              Real-time foreground priority boost + background EcoQoS loop (1.5s interval)
            </p>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            {governorState?.last_boosted_pid ? (
              <span className="badge badge-accent mono-num">
                Boosted PID {governorState.last_boosted_pid}
              </span>
            ) : null}
            <span className="badge mono-num">
              EcoQoS PIDs: {governorState?.eco_qos_pids.length ?? 0}
            </span>
          </div>
        </div>

        <ToggleSwitch
          checked={isEnabled}
          onChange={handleToggleGovernor}
          label="Enable Background Governor"
          description="Restores all altered priorities and original power plan automatically on exit."
        />

        <div className="profile-grid" style={{ marginTop: 16 }}>
          {PROFILES.map((p) => {
            const selected = activeProfile === p.id;
            return (
              <button
                key={p.id}
                type="button"
                className={`profile-card ${selected ? "is-selected" : ""}`}
                aria-pressed={selected}
                onClick={() => handleSelectProfile(p.id)}
              >
                <div
                  style={{
                    display: "flex",
                    width: "100%",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 8,
                  }}
                >
                  <span style={{ fontWeight: 700, fontSize: 14 }}>{p.label}</span>
                  {selected ? (
                    <span className="badge badge-accent">
                      <IconCheck size={11} />
                      Active
                    </span>
                  ) : null}
                </div>
                <span style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.45 }}>
                  {p.summary}
                </span>
              </button>
            );
          })}
        </div>
      </section>

      <div className="two-col-split">
        <section className="panel" aria-label="Smart RAM Trimmer">
          <div className="panel-header">
            <div>
              <h3 className="panel-title">Smart RAM Trimmer</h3>
              <p className="panel-subtitle">
                Trims idle background working sets only under real memory pressure
              </p>
            </div>
            <button
              type="button"
              className="btn btn-primary btn-sm"
              disabled={trimming}
              onClick={handleTrimNow}
            >
              {trimming ? "Trimming..." : "Trim Idle RAM Now"}
            </button>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <label htmlFor="ram-threshold-range" style={{ fontWeight: 600, fontSize: 13 }}>
                Automatic Trim Threshold
              </label>
              <span className="mono-num badge">{thresholdDraft}% physical RAM</span>
            </div>
            <input
              id="ram-threshold-range"
              type="range"
              min={50}
              max={95}
              step={1}
              className="range-input"
              value={thresholdDraft}
              onChange={(e) => setThresholdDraft(Number(e.target.value))}
            />
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
                60s cooldown between automatic trims
              </span>
              <button
                type="button"
                className="btn btn-sm"
                disabled={
                  savingThreshold ||
                  !config ||
                  thresholdDraft === config.ram_trim_threshold_pct
                }
                onClick={handleSaveThreshold}
              >
                {savingThreshold ? "Saving..." : "Save Threshold"}
              </button>
            </div>

            {trimReport ? (
              <div className="panel-elevated" role="status">
                <div style={{ fontWeight: 600, fontSize: 13 }}>Last Manual Trim Result</div>
                <div className="mono-num" style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 4 }}>
                  Trimmed {trimReport.trimmed_count} idle processes, freed {trimReport.freed_mb.toFixed(1)} MB
                </div>
              </div>
            ) : governorState?.last_trim_at ? (
              <div className="panel-elevated">
                <div style={{ fontWeight: 600, fontSize: 13 }}>Last Recorded Trim</div>
                <div className="mono-num" style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                  Freed {governorState.last_trim_freed_mb} MB at{" "}
                  {formatTimestamp(governorState.last_trim_at)}
                </div>
              </div>
            ) : null}
          </div>
        </section>

        <section className="panel" aria-label="Windows Power Plans">
          <div className="panel-header">
            <div>
              <h3 className="panel-title">Windows Power Plans</h3>
              <p className="panel-subtitle">
                Installed schemes via Win32 PowerEnumerate
              </p>
            </div>
            <button
              type="button"
              className="btn btn-sm"
              disabled={loadingPlans}
              onClick={loadPowerPlans}
            >
              <IconRefresh />
              Refresh
            </button>
          </div>

          {loadingPlans && powerPlans.length === 0 ? (
            <LoadingState
              title="Enumerating Windows power schemes..."
              detail="Reading active power plan list from Windows power policy."
            />
          ) : powerPlans.length === 0 ? (
            <EmptyState
              title="No power plans returned"
              detail="Windows did not return any power schemes. Try refreshing."
            />
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {powerPlans.map((plan) => (
                <div
                  key={plan.guid}
                  className="finding-item"
                  style={{
                    borderColor: plan.is_active ? "var(--accent-border)" : undefined,
                  }}
                >
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontWeight: 600, fontSize: 13 }}>{plan.name}</div>
                    <div
                      className="mono"
                      style={{ fontSize: 11, color: "var(--text-muted)", wordBreak: "break-all" }}
                    >
                      {plan.guid}
                    </div>
                  </div>
                  {plan.is_active ? (
                    <span className="badge badge-accent">Active</span>
                  ) : (
                    <button
                      type="button"
                      className="btn btn-sm"
                      disabled={busyPlanGuid === plan.guid}
                      onClick={() => handleSetPowerPlan(plan.guid)}
                    >
                      {busyPlanGuid === plan.guid ? "Switching..." : "Activate"}
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      <section className="panel" aria-label="Process Exclusion List">
        <div className="panel-header">
          <div>
            <h3 className="panel-title">Process Exclusion List</h3>
            <p className="panel-subtitle">
              Processes excluded from background EcoQoS throttling and idle RAM trimming
            </p>
          </div>
          <span className="badge mono-num">{config?.exclusions.length ?? 0} excluded</span>
        </div>

        <form onSubmit={handleAddExclusion} className="toolbar-group" style={{ marginBottom: 12 }}>
          <input
            type="text"
            className="input"
            style={{ width: 260 }}
            placeholder="e.g. discord.exe or obs64.exe"
            aria-label="Executable name to exclude"
            value={newExclusion}
            onChange={(e) => setNewExclusion(e.target.value)}
          />
          <button type="submit" className="btn btn-primary btn-sm" disabled={!newExclusion.trim()}>
            <IconPlus size={14} />
            Add Exclusion
          </button>
        </form>

        {!config || config.exclusions.length === 0 ? (
          <EmptyState
            title="No custom exclusions configured"
            detail="Critical Windows, audio, and anti-cheat processes remain protected automatically by safety.rs."
          />
        ) : (
          <div className="exclusion-chips">
            {config.exclusions.map((proc) => (
              <span key={proc} className="exclusion-chip mono">
                {proc}
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={`Remove ${proc} from exclusions`}
                  onClick={() => handleRemoveExclusion(proc)}
                >
                  <IconX size={13} />
                </button>
              </span>
            ))}
          </div>
        )}
      </section>
    </>
  );
}
