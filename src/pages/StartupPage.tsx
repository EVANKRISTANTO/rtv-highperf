import { useEffect, useMemo, useState } from "react";
import { IconLock, IconRefresh, IconSearch } from "../components/Icons";
import {
  ElevationPromptBanner,
  EmptyState,
  LoadingState,
} from "../components/StateViews";
import { ToggleSwitch } from "../components/ToggleSwitch";
import {
  parseAppError,
  tauriApi,
  type ParsedAppError,
  type StartupItem,
} from "../lib/tauri";

interface StartupPageProps {
  connected: boolean;
  isElevated: boolean | null;
  onRestartAsAdmin: () => void;
  elevating: boolean;
  onError: (err: ParsedAppError) => void;
}

export function StartupPage({
  connected,
  isElevated,
  onRestartAsAdmin,
  elevating,
  onError,
}: StartupPageProps) {
  const [items, setItems] = useState<StartupItem[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [scopeFilter, setScopeFilter] = useState<string>("All");
  const [statusFilter, setStatusFilter] = useState<"All" | "Enabled" | "Disabled">("All");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [needsElevationMsg, setNeedsElevationMsg] = useState<string | null>(null);

  async function loadItems() {
    if (!connected) return;
    setLoading(true);
    try {
      const list = await tauriApi.listStartupItems();
      setItems(list);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadItems();
  }, [connected]);

  const scopes = useMemo(() => {
    if (!items) return ["All"];
    const unique = Array.from(new Set(items.map((i) => i.scope)));
    return ["All", ...unique];
  }, [items]);

  const filtered = useMemo(() => {
    if (!items) return [];
    const q = search.trim().toLowerCase();
    return items.filter((item) => {
      if (scopeFilter !== "All" && item.scope !== scopeFilter) return false;
      if (statusFilter === "Enabled" && !item.enabled) return false;
      if (statusFilter === "Disabled" && item.enabled) return false;
      if (!q) return true;
      return (
        item.name.toLowerCase().includes(q) ||
        item.command.toLowerCase().includes(q) ||
        (item.publisher ?? "").toLowerCase().includes(q)
      );
    });
  }, [items, search, scopeFilter, statusFilter]);

  async function handleToggle(item: StartupItem, enable: boolean) {
    setBusyId(item.id);
    setNeedsElevationMsg(null);
    try {
      await tauriApi.toggleStartupItem(item.id, enable);
      setItems((prev) =>
        prev ? prev.map((x) => (x.id === item.id ? { ...x, enabled: enable } : x)) : prev
      );
    } catch (err) {
      const parsed = parseAppError(err);
      if (parsed.isElevationRequired) {
        setNeedsElevationMsg(
          `Modifying "${item.name}" (${item.scope}) requires Administrator privileges.`
        );
      } else {
        onError(parsed);
      }
    } finally {
      setBusyId(null);
    }
  }

  if (!connected) {
    return (
      <EmptyState
        title="Startup & Scheduled Tasks requires the Windows desktop backend"
        detail="Enumerating HKCU/HKLM Run keys, Windows StartupApproved flags, Startup folders, and non-Microsoft Scheduled Tasks requires the Rust backend."
      />
    );
  }

  const enabledCount = items ? items.filter((i) => i.enabled).length : 0;

  return (
    <>
      {needsElevationMsg ? (
        <ElevationPromptBanner
          message={needsElevationMsg}
          onElevate={onRestartAsAdmin}
          busy={elevating}
        />
      ) : null}

      <section className="panel" aria-label="Startup and Scheduled Tasks Filters">
        <div className="toolbar-row">
          <div>
            <h2 className="panel-title">Startup Entries & Scheduled Tasks</h2>
            <p className="panel-subtitle">
              Uses Windows StartupApproved flags so disabling stays compatible with Task Manager
            </p>
          </div>
          <div className="toolbar-group">
            {items ? (
              <span className="badge mono-num">
                {enabledCount} enabled / {items.length} total
              </span>
            ) : null}
            <button
              type="button"
              className="btn btn-sm"
              disabled={loading}
              onClick={loadItems}
            >
              <IconRefresh />
              {loading ? "Refreshing..." : "Refresh"}
            </button>
          </div>
        </div>

        <div className="toolbar-row" style={{ marginTop: 14 }}>
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
                style={{ paddingLeft: 32, width: 240 }}
                placeholder="Search name, command, publisher..."
                aria-label="Filter startup items"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>

            <select
              className="select"
              aria-label="Filter by startup scope"
              value={scopeFilter}
              onChange={(e) => setScopeFilter(e.target.value)}
            >
              {scopes.map((s) => (
                <option key={s} value={s}>
                  Scope: {s}
                </option>
              ))}
            </select>

            <select
              className="select"
              aria-label="Filter by enabled status"
              value={statusFilter}
              onChange={(e) =>
                setStatusFilter(e.target.value as "All" | "Enabled" | "Disabled")
              }
            >
              <option value="All">Status: All</option>
              <option value="Enabled">Status: Enabled</option>
              <option value="Disabled">Status: Disabled</option>
            </select>
          </div>
        </div>
      </section>

      {loading && !items ? (
        <LoadingState
          title="Reading Windows startup sources..."
          detail="Inspecting HKCU/HKLM Run keys, Startup folders, and Scheduled Tasks."
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No startup items match your filters"
          detail="Try clearing the search query or switching scope/status filters back to All."
        />
      ) : (
        <section className="panel" aria-label="Startup Items Table">
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Scope</th>
                  <th>Publisher</th>
                  <th>Command</th>
                  <th>Enabled</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((item) => {
                  const scopeLower = item.scope.toLowerCase();
                  const needsAdmin =
                    item.requires_admin ??
                    (scopeLower.includes("hklm") ||
                      scopeLower.includes("task") ||
                      scopeLower.includes("common"));
                  return (
                    <tr key={item.id}>
                      <td>
                        <div style={{ fontWeight: 600 }}>{item.name}</div>
                      </td>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <span className="badge mono">{item.scope}</span>
                          {needsAdmin && isElevated === false ? (
                            <span
                              className="badge badge-warn"
                              title="Toggling this scope may require Administrator elevation"
                            >
                              <IconLock size={11} />
                              Admin
                            </span>
                          ) : null}
                        </div>
                      </td>
                      <td style={{ color: "var(--text-secondary)" }}>
                        {item.publisher || "Unknown publisher"}
                      </td>
                      <td>
                        <div
                          className="mono"
                          style={{
                            fontSize: 11,
                            color: "var(--text-muted)",
                            maxWidth: 420,
                            wordBreak: "break-all",
                          }}
                        >
                          {item.command}
                        </div>
                      </td>
                      <td>
                        <ToggleSwitch
                          checked={item.enabled}
                          disabled={busyId === item.id}
                          onChange={(next) => handleToggle(item, next)}
                          label={`Toggle ${item.name}`}
                        />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>
      )}
    </>
  );
}
