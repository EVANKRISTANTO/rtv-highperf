import { useMemo, useState } from "react";
import {
  IconCheck,
  IconRefresh,
  IconSearch,
  IconShieldCheck,
} from "../components/Icons";
import { Sparkline } from "../components/Sparkline";
import { EmptyState, LoadingState } from "../components/StateViews";
import { formatBps, formatBytes, formatPct, formatTimestamp } from "../lib/format";
import {
  parseAppError,
  tauriApi,
  type ParsedAppError,
  type ProcessInfo,
  type SystemSnapshot,
} from "../lib/tauri";

type SortKey = "cpu_pct" | "ram_mb" | "name" | "pid";

interface LagDetectivePageProps {
  connected: boolean;
  snapshot: SystemSnapshot | null;
  cpuHistory: number[];
  ramHistory: number[];
  loading: boolean;
  onRefresh: () => void;
  onError: (err: ParsedAppError) => void;
}

export function LagDetectivePage({
  connected,
  snapshot,
  cpuHistory,
  ramHistory,
  loading,
  onRefresh,
  onError,
}: LagDetectivePageProps) {
  const [sortKey, setSortKey] = useState<SortKey>("cpu_pct");
  const [sortAsc, setSortAsc] = useState(false);
  const [procFilter, setProcFilter] = useState("");
  const [busyFindingId, setBusyFindingId] = useState<string | null>(null);
  const [busyProcKey, setBusyProcKey] = useState<string | null>(null);
  const [actionNotice, setActionNotice] = useState<string | null>(null);

  const ramPct = useMemo(() => {
    if (!snapshot || snapshot.ram_total_bytes <= 0) return null;
    return (snapshot.ram_used_bytes / snapshot.ram_total_bytes) * 100;
  }, [snapshot]);

  const sortedProcesses = useMemo(() => {
    if (!snapshot) return [];
    const q = procFilter.trim().toLowerCase();
    const list = snapshot.top_processes.filter((p) => {
      if (!q) return true;
      return p.name.toLowerCase().includes(q) || String(p.pid).includes(q);
    });
    return [...list].sort((a, b) => {
      let cmp = 0;
      if (sortKey === "name") {
        cmp = a.name.localeCompare(b.name);
      } else {
        cmp = a[sortKey] - b[sortKey];
      }
      return sortAsc ? cmp : -cmp;
    });
  }, [snapshot, procFilter, sortKey, sortAsc]);

  function handleSort(nextKey: SortKey) {
    if (sortKey === nextKey) {
      setSortAsc((v) => !v);
    } else {
      setSortKey(nextKey);
      setSortAsc(nextKey === "name" || nextKey === "pid");
    }
  }

  async function handleFixFinding(findingId: string) {
    setBusyFindingId(findingId);
    setActionNotice(null);
    try {
      await tauriApi.applyFindingFix(findingId);
      setActionNotice(`Applied fix for finding "${findingId}".`);
      onRefresh();
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyFindingId(null);
    }
  }

  async function handleProcessAction(proc: ProcessInfo, action: string) {
    const key = `${proc.pid}:${action}`;
    setBusyProcKey(key);
    setActionNotice(null);
    try {
      await tauriApi.applyProcessAction(proc.pid, action, proc.name);
      setActionNotice(`Executed "${action}" on ${proc.name} (PID ${proc.pid}).`);
      onRefresh();
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyProcKey(null);
    }
  }

  if (!connected) {
    return (
      <EmptyState
        title="Live telemetry unavailable in browser preview"
        detail="Lag Detective reads real Windows CPU, memory, disk throughput, active power plan, and process working sets via the Rust backend. Launch the desktop binary or run cargo tauri dev to inspect your machine."
      />
    );
  }

  if (loading && !snapshot) {
    return (
      <LoadingState
        title="Sampling Windows telemetry..."
        detail="Collecting processor cores, memory status, power scheme, and active process metrics."
      />
    );
  }

  if (!snapshot) {
    return (
      <EmptyState
        title="No telemetry snapshot received yet"
        detail="Click Refresh Sample to request an immediate system snapshot from the backend."
        action={
          <button type="button" className="btn btn-primary" onClick={onRefresh}>
            <IconRefresh />
            Refresh Sample
          </button>
        }
      />
    );
  }

  const score = Math.max(0, Math.min(100, snapshot.health_score));
  const scoreTone =
    score >= 80 ? "var(--accent)" : score >= 50 ? "var(--warn)" : "var(--danger)";
  const radius = 38;
  const circumference = 2 * Math.PI * radius;
  const strokeOffset = circumference * (1 - score / 100);

  return (
    <>
      {actionNotice ? (
        <div className="banner banner-accent" role="status">
          <div className="banner-body">
            <span className="banner-icon">
              <IconCheck />
            </span>
            <div>
              <p className="banner-title">{actionNotice}</p>
            </div>
          </div>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setActionNotice(null)}
          >
            Dismiss
          </button>
        </div>
      ) : null}

      <div className="detective-hero-grid">
        <section className="panel health-gauge-panel" aria-label="PC Health Score">
          <div className="panel-header" style={{ marginBottom: 0 }}>
            <div>
              <h2 className="panel-title">PC Health Score</h2>
              <p className="panel-subtitle mono-num">
                Sampled {formatTimestamp(snapshot.sampled_at_ms)}
              </p>
            </div>
            <button
              type="button"
              className="btn btn-sm"
              onClick={onRefresh}
              aria-label="Refresh telemetry snapshot"
            >
              <IconRefresh />
              Refresh
            </button>
          </div>

          <div className="gauge-row">
            <div className="gauge-ring-wrap">
              <svg width="96" height="96" viewBox="0 0 96 96" aria-hidden="true">
                <circle
                  cx="48"
                  cy="48"
                  r={radius}
                  fill="none"
                  stroke="var(--border-subtle)"
                  strokeWidth="8"
                />
                <circle
                  cx="48"
                  cy="48"
                  r={radius}
                  fill="none"
                  stroke={scoreTone}
                  strokeWidth="8"
                  strokeLinecap="round"
                  strokeDasharray={circumference}
                  strokeDashoffset={strokeOffset}
                  transform="rotate(-90 48 48)"
                  style={{
                    transition: "stroke-dashoffset var(--dur-slow) var(--ease-out-expo)",
                  }}
                />
              </svg>
              <div className="gauge-value">
                <span className="gauge-score-num mono-num" style={{ color: scoreTone }}>
                  {score}
                </span>
                <span className="gauge-score-cap">/ 100</span>
              </div>
            </div>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontWeight: 600, fontSize: 14 }}>
                {score >= 80
                  ? "Healthy baseline"
                  : score >= 50
                  ? "Moderate bottlenecks found"
                  : "Critical resource pressure"}
              </div>
              <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--text-muted)" }}>
                Deterministic score computed from live RAM pressure, disk I/O, startup count,
                and active power scheme.
              </p>
            </div>
          </div>

          <div className="telemetry-kv-grid">
            <div className="kv-cell">
              <div className="kv-label">Physical RAM</div>
              <div className="kv-val mono-num">
                {formatBytes(snapshot.ram_used_bytes)} /{" "}
                {formatBytes(snapshot.ram_total_bytes)}
              </div>
            </div>
            <div className="kv-cell">
              <div className="kv-label">Commit / Available</div>
              <div className="kv-val mono-num">
                {formatPct(snapshot.commit_pct)} /{" "}
                {formatBytes(snapshot.ram_available_bytes)}
              </div>
            </div>
            <div className="kv-cell">
              <div className="kv-label">Disk R / W</div>
              <div className="kv-val mono-num">
                {formatBps(snapshot.disk_read_bps)} /{" "}
                {formatBps(snapshot.disk_write_bps)}
              </div>
            </div>
            <div className="kv-cell">
              <div className="kv-label">Power Plan</div>
              <div className="kv-val" title={snapshot.power_plan_guid}>
                {snapshot.active_power_plan}
                {snapshot.is_on_ac_power !== null
                  ? ` (${snapshot.is_on_ac_power ? "AC" : "Battery"})`
                  : ""}
              </div>
            </div>
            <div className="kv-cell">
              <div className="kv-label">Enabled Startup</div>
              <div className="kv-val mono-num">{snapshot.startup_enabled_count} entries</div>
            </div>
            <div className="kv-cell">
              <div className="kv-label">Cached Cleanable</div>
              <div className="kv-val mono-num">
                {snapshot.cleanable_bytes_cached !== null
                  ? formatBytes(snapshot.cleanable_bytes_cached)
                  : "Not scanned"}
              </div>
            </div>
          </div>
        </section>

        <section className="panel" aria-label="Bottleneck Findings">
          <div className="panel-header">
            <div>
              <h2 className="panel-title">Detected Bottlenecks</h2>
              <p className="panel-subtitle">
                Every deduction lists its measured cause and direct remediation action
              </p>
            </div>
            <span className="badge mono-num">
              {snapshot.findings.length}{" "}
              {snapshot.findings.length === 1 ? "finding" : "findings"}
            </span>
          </div>

          {snapshot.findings.length === 0 ? (
            <EmptyState
              title="No active bottlenecks detected"
              detail="All monitored indicators (memory pressure, power scheme, startup load, and background hogs) are within healthy thresholds."
            />
          ) : (
            <div className="finding-list">
              {snapshot.findings.map((finding) => {
                const sevLower = finding.severity.toLowerCase();
                const badgeClass =
                  sevLower === "critical"
                    ? "badge-danger"
                    : sevLower === "warning"
                    ? "badge-warn"
                    : "badge";
                const isBusy = busyFindingId === finding.id;
                return (
                  <div key={finding.id} className="finding-item">
                    <div style={{ minWidth: 0 }}>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                          flexWrap: "wrap",
                        }}
                      >
                        <span className={`badge ${badgeClass}`}>{finding.severity}</span>
                        <span className="badge mono-num">-{finding.deduction} pts</span>
                        <span style={{ fontWeight: 600 }}>{finding.title}</span>
                      </div>
                      <p
                        style={{
                          margin: "4px 0 0",
                          fontSize: 12,
                          color: "var(--text-muted)",
                        }}
                      >
                        {finding.detail}
                      </p>
                    </div>
                    {finding.fix_action ? (
                      <button
                        type="button"
                        className="btn btn-primary btn-sm"
                        disabled={isBusy}
                        onClick={() => handleFixFinding(finding.id)}
                      >
                        {isBusy ? "Applying..." : "Fix This"}
                      </button>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>

      <div className="sparkline-grid">
        <Sparkline
          title="CPU utilization across recent telemetry ticks"
          subtitle="Total processor usage (%)"
          values={cpuHistory}
          tone={
            snapshot.cpu_total_pct >= 85
              ? "danger"
              : snapshot.cpu_total_pct >= 65
              ? "warn"
              : "accent"
          }
        />
        <Sparkline
          title="Physical RAM utilization across recent telemetry ticks"
          subtitle={`Commit charge ${formatPct(snapshot.commit_pct)}`}
          values={ramHistory}
          tone={
            ramPct !== null && ramPct >= 85
              ? "danger"
              : ramPct !== null && ramPct >= 75
              ? "warn"
              : "accent"
          }
        />
      </div>

      {snapshot.cpu_cores.length > 0 ? (
        <section className="panel" aria-label="Logical core load">
          <div className="panel-header" style={{ marginBottom: 6 }}>
            <div>
              <h3 className="panel-title">Logical Core Distribution</h3>
              <p className="panel-subtitle">
                {snapshot.cpu_cores.length} logical threads reported by Windows
              </p>
            </div>
          </div>
          <div className="core-bars">
            {snapshot.cpu_cores.map((pct, idx) => (
              <div key={idx} className="core-bar-item">
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    fontSize: 11,
                  }}
                >
                  <span style={{ color: "var(--text-muted)" }}>C{idx}</span>
                  <span className="mono-num">{pct.toFixed(0)}%</span>
                </div>
                <div className="core-bar-track">
                  <div
                    className="core-bar-fill"
                    style={{
                      transform: `scaleX(${Math.max(0, Math.min(1, pct / 100))})`,
                      background:
                        pct >= 85
                          ? "var(--danger)"
                          : pct >= 65
                          ? "var(--warn)"
                          : "var(--accent)",
                    }}
                  />
                </div>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      <section className="panel" aria-label="Top Resource Hogs">
        <div className="toolbar-row" style={{ marginBottom: 12 }}>
          <div>
            <h2 className="panel-title">Top Resource Hogs</h2>
            <p className="panel-subtitle">
              Real-time process priority, EcoQoS efficiency mode, and working set controls
            </p>
          </div>
          <div className="toolbar-group">
            <div style={{ position: "relative" }}>
              <IconSearch
                style={{
                  position: "absolute",
                  left: 10,
                  top: "50%",
                  transform: "translateY(-50%)",
                  color: "var(--text-muted)",
                  pointerEvents: "none",
                }}
              />
              <input
                type="search"
                className="input"
                style={{ paddingLeft: 32, width: 220 }}
                placeholder="Filter process or PID..."
                aria-label="Filter processes by name or PID"
                value={procFilter}
                onChange={(e) => setProcFilter(e.target.value)}
              />
            </div>
          </div>
        </div>

        {sortedProcesses.length === 0 ? (
          <EmptyState
            title="No matching processes"
            detail="Clear the search filter to view active top resource consumers."
          />
        ) : (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>
                    <button
                      type="button"
                      className="sort-btn"
                      onClick={() => handleSort("pid")}
                    >
                      PID {sortKey === "pid" ? (sortAsc ? "▲" : "▼") : ""}
                    </button>
                  </th>
                  <th>
                    <button
                      type="button"
                      className="sort-btn"
                      onClick={() => handleSort("name")}
                    >
                      Process {sortKey === "name" ? (sortAsc ? "▲" : "▼") : ""}
                    </button>
                  </th>
                  <th>
                    <button
                      type="button"
                      className="sort-btn"
                      onClick={() => handleSort("cpu_pct")}
                    >
                      CPU % {sortKey === "cpu_pct" ? (sortAsc ? "▲" : "▼") : ""}
                    </button>
                  </th>
                  <th>
                    <button
                      type="button"
                      className="sort-btn"
                      onClick={() => handleSort("ram_mb")}
                    >
                      Working Set {sortKey === "ram_mb" ? (sortAsc ? "▲" : "▼") : ""}
                    </button>
                  </th>
                  <th>Priority / QoS</th>
                  <th>Quick Actions</th>
                </tr>
              </thead>
              <tbody>
                {sortedProcesses.map((proc) => (
                  <tr key={proc.pid}>
                    <td className="mono-num">{proc.pid}</td>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <span style={{ fontWeight: 600 }}>{proc.name}</span>
                        {proc.is_protected ? (
                          <span
                            className="badge badge-accent"
                            title="Protected Windows or security process (never throttled or trimmed)"
                          >
                            <IconShieldCheck size={11} />
                            Protected
                          </span>
                        ) : null}
                        {proc.is_excluded ? (
                          <span className="badge">Excluded</span>
                        ) : null}
                      </div>
                    </td>
                    <td className="mono-num">{proc.cpu_pct.toFixed(1)}%</td>
                    <td className="mono-num">{proc.ram_mb.toFixed(1)} MB</td>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <span className="badge mono">{proc.priority}</span>
                        {proc.eco_qos ? (
                          <span className="badge badge-accent">EcoQoS</span>
                        ) : null}
                      </div>
                    </td>
                    <td>
                      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                        <button
                          type="button"
                          className="btn btn-sm"
                          disabled={
                            proc.is_protected || busyProcKey === `${proc.pid}:eco_qos`
                          }
                          onClick={() => handleProcessAction(proc, "eco_qos")}
                          title="Apply Windows EcoQoS efficiency throttling"
                        >
                          EcoQoS
                        </button>
                        <button
                          type="button"
                          className="btn btn-sm"
                          disabled={
                            proc.is_protected || busyProcKey === `${proc.pid}:boost`
                          }
                          onClick={() => handleProcessAction(proc, "boost")}
                          title="Boost process priority and clear EcoQoS"
                        >
                          Boost
                        </button>
                        <button
                          type="button"
                          className="btn btn-sm"
                          disabled={
                            proc.is_protected || busyProcKey === `${proc.pid}:trim`
                          }
                          onClick={() => handleProcessAction(proc, "trim")}
                          title="Trim idle working set memory"
                        >
                          Trim
                        </button>
                        <button
                          type="button"
                          className="btn btn-sm btn-ghost"
                          disabled={busyProcKey === `${proc.pid}:exclude`}
                          onClick={() => handleProcessAction(proc, "exclude")}
                          title="Add process to Auto-Optimizer exclusion list"
                        >
                          {proc.is_excluded ? "Excluded" : "Exclude"}
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </>
  );
}
