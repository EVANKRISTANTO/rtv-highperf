import { useEffect, useMemo, useState } from "react";
import {
  IconAlertTriangle,
  IconCheck,
  IconDownload,
  IconLock,
  IconRefresh,
  IconSearch,
  IconTrash,
  IconUndo,
} from "../components/Icons";
import { Modal } from "../components/Modal";
import { EmptyState, LoadingState } from "../components/StateViews";
import { formatTimestamp, formatTweakState } from "../lib/format";
import {
  parseAppError,
  tauriApi,
  type AppxPackageItem,
  type BackupSnapshot,
  type ParsedAppError,
  type TweakItem,
} from "../lib/tauri";

type TweakerTab = "tweaks" | "backups" | "appx";

interface SafeTweakerPageProps {
  connected: boolean;
  onError: (err: ParsedAppError) => void;
}

const CATEGORY_ORDER = [
  "Latency & Gaming",
  "Privacy & Telemetry",
  "UI Snappiness",
  "Background Services",
];

function normalizeCategory(raw: string): string {
  const lower = raw.toLowerCase();
  if (lower.includes("latency") || lower.includes("gaming")) return "Latency & Gaming";
  if (lower.includes("privacy") || lower.includes("telemetry")) return "Privacy & Telemetry";
  if (lower.includes("ui") || lower.includes("snapp")) return "UI Snappiness";
  if (lower.includes("service")) return "Background Services";
  return raw;
}

export function SafeTweakerPage({ connected, onError }: SafeTweakerPageProps) {
  const [activeTab, setActiveTab] = useState<TweakerTab>("tweaks");
  const [tweaks, setTweaks] = useState<TweakItem[] | null>(null);
  const [backups, setBackups] = useState<BackupSnapshot[] | null>(null);
  const [appxList, setAppxList] = useState<AppxPackageItem[] | null>(null);

  const [loadingTweaks, setLoadingTweaks] = useState(false);
  const [loadingBackups, setLoadingBackups] = useState(false);
  const [loadingAppx, setLoadingAppx] = useState(false);

  const [selectedTweakIds, setSelectedTweakIds] = useState<Set<string>>(new Set());
  const [tweakSearch, setTweakSearch] = useState("");
  const [appxSearch, setAppxSearch] = useState("");
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [diffSnapshot, setDiffSnapshot] = useState<BackupSnapshot | null>(null);
  const [regExportPreview, setRegExportPreview] = useState<{
    snapshotId: string;
    content: string;
  } | null>(null);
  const [appxConfirm, setAppxConfirm] = useState<AppxPackageItem | null>(null);

  async function loadTweaks() {
    if (!connected) return;
    setLoadingTweaks(true);
    try {
      const list = await tauriApi.listTweaks();
      setTweaks(list);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setLoadingTweaks(false);
    }
  }

  async function loadBackups() {
    if (!connected) return;
    setLoadingBackups(true);
    try {
      const list = await tauriApi.listBackups();
      setBackups(list);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setLoadingBackups(false);
    }
  }

  async function loadAppx() {
    if (!connected) return;
    setLoadingAppx(true);
    try {
      const list = await tauriApi.listAppxPackages();
      setAppxList(list);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setLoadingAppx(false);
    }
  }

  useEffect(() => {
    if (!connected) return;
    if (activeTab === "tweaks" && !tweaks) loadTweaks();
    if (activeTab === "backups" && !backups) loadBackups();
    if (activeTab === "appx" && !appxList) loadAppx();
  }, [connected, activeTab]);

  const groupedTweaks = useMemo(() => {
    if (!tweaks) return [];
    const q = tweakSearch.trim().toLowerCase();
    const map = new Map<string, TweakItem[]>();
    for (const item of tweaks) {
      if (
        q &&
        !item.title.toLowerCase().includes(q) &&
        !item.description.toLowerCase().includes(q) &&
        !item.target_summary.toLowerCase().includes(q)
      ) {
        continue;
      }
      const cat = normalizeCategory(item.category);
      const bucket = map.get(cat) ?? [];
      bucket.push(item);
      map.set(cat, bucket);
    }
    const entries = Array.from(map.entries());
    entries.sort((a, b) => {
      const ia = CATEGORY_ORDER.indexOf(a[0]);
      const ib = CATEGORY_ORDER.indexOf(b[0]);
      if (ia !== -1 && ib !== -1) return ia - ib;
      if (ia !== -1) return -1;
      if (ib !== -1) return 1;
      return a[0].localeCompare(b[0]);
    });
    return entries;
  }, [tweaks, tweakSearch]);

  const filteredAppx = useMemo(() => {
    if (!appxList) return [];
    const q = appxSearch.trim().toLowerCase();
    return appxList.filter((pkg) => {
      if (!q) return true;
      return (
        pkg.name.toLowerCase().includes(q) ||
        pkg.package_full_name.toLowerCase().includes(q) ||
        pkg.publisher.toLowerCase().includes(q)
      );
    });
  }, [appxList, appxSearch]);

  async function handleApplyTweaks(ids: string[]) {
    if (ids.length === 0) return;
    setBusyKey(ids.length === 1 ? `apply:${ids[0]}` : "apply:batch");
    setNotice(null);
    try {
      const snap = await tauriApi.applyTweaks(ids);
      setNotice(
        `Applied ${ids.length} tweak(s). Saved rollback snapshot "${snap.id}".`
      );
      setSelectedTweakIds(new Set());
      await Promise.all([loadTweaks(), loadBackups()]);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyKey(null);
    }
  }

  async function handleRevertTweak(id: string) {
    setBusyKey(`revert:${id}`);
    setNotice(null);
    try {
      await tauriApi.revertTweaks([id]);
      setNotice(`Reverted tweak "${id}".`);
      await Promise.all([loadTweaks(), loadBackups()]);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyKey(null);
    }
  }

  async function handleRestoreSnapshot(snapshotId: string) {
    setBusyKey(`restore:${snapshotId}`);
    setNotice(null);
    try {
      await tauriApi.restoreBackup(snapshotId);
      setNotice(`Restored snapshot "${snapshotId}".`);
      await Promise.all([loadTweaks(), loadBackups()]);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyKey(null);
    }
  }

  async function handleExportReg(snapshotId: string) {
    setBusyKey(`export:${snapshotId}`);
    try {
      const regText = await tauriApi.exportBackupReg(snapshotId);
      setRegExportPreview({ snapshotId, content: regText });
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyKey(null);
    }
  }

  function downloadRegFile(snapshotId: string, content: string) {
    const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${snapshotId}.reg`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function handleRemoveAppx(pkg: AppxPackageItem) {
    setAppxConfirm(null);
    setBusyKey(`appx:${pkg.package_full_name}`);
    setNotice(null);
    try {
      await tauriApi.removeAppxPackage(pkg.package_full_name);
      setNotice(`Removed "${pkg.name}" for current user.`);
      await loadAppx();
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setBusyKey(null);
    }
  }

  function handleTabKeyDown(e: React.KeyboardEvent, idx: number) {
    const order: TweakerTab[] = ["tweaks", "backups", "appx"];
    if (e.key === "ArrowRight") {
      setActiveTab(order[(idx + 1) % order.length]);
    } else if (e.key === "ArrowLeft") {
      setActiveTab(order[(idx - 1 + order.length) % order.length]);
    }
  }

  if (!connected) {
    return (
      <EmptyState
        title="Safe Tweaker requires the Windows desktop backend"
        detail="Reading live Windows registry values, service start types, JSON backup snapshots, and current-user AppX packages requires the Rust backend."
      />
    );
  }

  return (
    <>
      <div className="toolbar-row">
        <div className="tablist" role="tablist" aria-label="Safe Tweaker sections">
          {(
            [
              { id: "tweaks", label: "Registry & Service Tweaks" },
              { id: "backups", label: "Backup & Rollback History" },
              { id: "appx", label: "AppX Bloatware Manager" },
            ] as const
          ).map((tab, idx) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              className="tab-btn"
              onClick={() => setActiveTab(tab.id)}
              onKeyDown={(e) => handleTabKeyDown(e, idx)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </div>

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

      {activeTab === "tweaks" ? (
        <>
          <section className="panel" aria-label="Tweaks Controls">
            <div className="toolbar-row">
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
                  style={{ paddingLeft: 32, width: 260 }}
                  placeholder="Filter tweaks or registry path..."
                  aria-label="Filter registry and service tweaks"
                  value={tweakSearch}
                  onChange={(e) => setTweakSearch(e.target.value)}
                />
              </div>

              <div className="toolbar-group">
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={loadingTweaks}
                  onClick={loadTweaks}
                >
                  <IconRefresh />
                  Refresh
                </button>
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  disabled={
                    selectedTweakIds.size === 0 || busyKey === "apply:batch"
                  }
                  onClick={() => handleApplyTweaks(Array.from(selectedTweakIds))}
                >
                  {busyKey === "apply:batch"
                    ? "Creating Snapshot..."
                    : `Apply Selected (${selectedTweakIds.size})`}
                </button>
              </div>
            </div>
          </section>

          {loadingTweaks && !tweaks ? (
            <LoadingState
              title="Inspecting Windows registry & services..."
              detail="Reading current values for each tweak in the declarative catalog."
            />
          ) : groupedTweaks.length === 0 ? (
            <EmptyState
              title="No matching tweaks found"
              detail="Adjust your search filter or refresh the tweak catalog."
            />
          ) : (
            groupedTweaks.map(([category, items]) => (
              <section key={category} className="panel" aria-label={category}>
                <div className="panel-header">
                  <div>
                    <h3 className="panel-title">{category}</h3>
                    <p className="panel-subtitle">
                      100% covered by automatic pre-change JSON snapshot rollback
                    </p>
                  </div>
                  <span className="badge mono-num">{items.length} tweaks</span>
                </div>

                <div className="finding-list">
                  {items.map((tweak) => {
                    const riskLower = tweak.risk.toLowerCase();
                    const riskClass =
                      riskLower === "safe"
                        ? "badge-accent"
                        : riskLower === "moderate"
                        ? "badge-warn"
                        : "badge-danger";
                    const checked = selectedTweakIds.has(tweak.id);

                    return (
                      <div key={tweak.id} className="tweak-card">
                        <div style={{ display: "flex", gap: 12, minWidth: 0, flex: 1 }}>
                          <input
                            type="checkbox"
                            aria-label={`Select ${tweak.title}`}
                            checked={checked}
                            onChange={() => {
                              setSelectedTweakIds((prev) => {
                                const next = new Set(prev);
                                if (next.has(tweak.id)) next.delete(tweak.id);
                                else next.add(tweak.id);
                                return next;
                              });
                            }}
                            style={{
                              marginTop: 3,
                              width: 16,
                              height: 16,
                              accentColor: "var(--accent)",
                            }}
                          />
                          <div style={{ minWidth: 0 }}>
                            <div
                              style={{
                                display: "flex",
                                alignItems: "center",
                                gap: 8,
                                flexWrap: "wrap",
                              }}
                            >
                              <span style={{ fontWeight: 600, fontSize: 14 }}>
                                {tweak.title}
                              </span>
                              <span className={`badge ${riskClass}`}>{tweak.risk}</span>
                              {tweak.requires_admin ? (
                                <span className="badge badge-warn">
                                  <IconLock size={11} />
                                  Admin
                                </span>
                              ) : null}
                              {tweak.is_applied ? (
                                <span className="badge badge-accent">Applied</span>
                              ) : null}
                            </div>

                            <p
                              style={{
                                margin: "4px 0 0",
                                fontSize: 12,
                                color: "var(--text-secondary)",
                              }}
                            >
                              {tweak.description}
                            </p>

                            <div className="tweak-meta">
                              <span className="badge mono">{tweak.target_summary}</span>
                              <span className="badge mono-num">
                                Current: {formatTweakState(tweak.current_state)}
                              </span>
                              {tweak.target_state !== undefined ? (
                                <span className="badge mono-num">
                                  Target: {formatTweakState(tweak.target_state)}
                                </span>
                              ) : null}
                            </div>
                          </div>
                        </div>

                        <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
                          {tweak.is_applied ? (
                            <button
                              type="button"
                              className="btn btn-sm"
                              disabled={busyKey === `revert:${tweak.id}`}
                              onClick={() => handleRevertTweak(tweak.id)}
                            >
                              <IconUndo size={13} />
                              {busyKey === `revert:${tweak.id}` ? "Reverting..." : "Revert"}
                            </button>
                          ) : (
                            <button
                              type="button"
                              className="btn btn-primary btn-sm"
                              disabled={busyKey === `apply:${tweak.id}`}
                              onClick={() => handleApplyTweaks([tweak.id])}
                            >
                              {busyKey === `apply:${tweak.id}` ? "Applying..." : "Apply"}
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </section>
            ))
          )}
        </>
      ) : null}

      {activeTab === "backups" ? (
        <section className="panel" aria-label="Backup and Rollback History">
          <div className="panel-header">
            <div>
              <h3 className="panel-title">Snapshot Backup & Rollback History</h3>
              <p className="panel-subtitle">
                JSON pre-change snapshots stored locally in %APPDATA%\rtv-highperf\backups
              </p>
            </div>
            <button
              type="button"
              className="btn btn-sm"
              disabled={loadingBackups}
              onClick={loadBackups}
            >
              <IconRefresh />
              Refresh
            </button>
          </div>

          {loadingBackups && !backups ? (
            <LoadingState
              title="Loading snapshot history..."
              detail="Reading saved JSON rollback snapshots from disk."
            />
          ) : !backups || backups.length === 0 ? (
            <EmptyState
              title="No backup snapshots recorded yet"
              detail="Applying any registry or service tweak automatically creates a pre-change JSON snapshot here for 1-click rollback."
            />
          ) : (
            <div className="finding-list">
              {backups.map((snap) => (
                <div key={snap.id} className="finding-item">
                  <div style={{ minWidth: 0 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                      <span style={{ fontWeight: 600 }}>{snap.label}</span>
                      <span className="badge mono-num">
                        {snap.entries.length} entries
                      </span>
                    </div>
                    <div className="mono-num" style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 3 }}>
                      ID: {snap.id} | Created: {formatTimestamp(snap.created_at)}
                    </div>
                  </div>
                  <div className="toolbar-group">
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() => setDiffSnapshot(snap)}
                    >
                      Inspect Diff
                    </button>
                    <button
                      type="button"
                      className="btn btn-sm"
                      disabled={busyKey === `export:${snap.id}`}
                      onClick={() => handleExportReg(snap.id)}
                    >
                      <IconDownload size={14} />
                      Export .reg
                    </button>
                    <button
                      type="button"
                      className="btn btn-primary btn-sm"
                      disabled={busyKey === `restore:${snap.id}`}
                      onClick={() => handleRestoreSnapshot(snap.id)}
                    >
                      <IconUndo size={14} />
                      {busyKey === `restore:${snap.id}` ? "Restoring..." : "Restore Snapshot"}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      ) : null}

      {activeTab === "appx" ? (
        <>
          <div className="banner banner-warn" role="alert">
            <div className="banner-body">
              <span className="banner-icon">
                <IconAlertTriangle />
              </span>
              <div>
                <p className="banner-title">
                  Non-Reversible via Offline Snapshot (Current-User Removal Only)
                </p>
                <p className="banner-desc">
                  Removing a Windows Store app uninstalls it for the current user via
                  Remove-AppxPackage. Reinstalling requires Microsoft Store or internet access
                  and is not covered by offline JSON snapshot rollback. Critical system packages
                  (Microsoft Store, Windows Terminal, Winget, Calculator, Photos) are excluded
                  from removal.
                </p>
              </div>
            </div>
          </div>

          <section className="panel" aria-label="AppX Bloatware Manager">
            <div className="toolbar-row" style={{ marginBottom: 12 }}>
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
                  style={{ paddingLeft: 32, width: 260 }}
                  placeholder="Filter AppX packages..."
                  aria-label="Filter AppX packages"
                  value={appxSearch}
                  onChange={(e) => setAppxSearch(e.target.value)}
                />
              </div>
              <button
                type="button"
                className="btn btn-sm"
                disabled={loadingAppx}
                onClick={loadAppx}
              >
                <IconRefresh />
                {loadingAppx ? "Loading..." : "Refresh Packages"}
              </button>
            </div>

            {loadingAppx && !appxList ? (
              <LoadingState
                title="Enumerating current-user AppX packages..."
                detail="Querying installed removable UWP applications."
              />
            ) : filteredAppx.length === 0 ? (
              <EmptyState
                title="No removable AppX packages found"
                detail="Either no matching removable packages are installed for your user account, or your filter returned zero matches."
              />
            ) : (
              <div className="finding-list">
                {filteredAppx.map((pkg) => (
                  <div key={pkg.package_full_name} className="finding-item">
                    <div style={{ minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ fontWeight: 600 }}>{pkg.name}</span>
                        {pkg.is_removable_bloat ? (
                          <span className="badge badge-warn">Removable</span>
                        ) : (
                          <span className="badge">Protected</span>
                        )}
                      </div>
                      <div
                        className="mono"
                        style={{
                          fontSize: 11,
                          color: "var(--text-muted)",
                          marginTop: 3,
                          wordBreak: "break-all",
                        }}
                      >
                        {pkg.package_full_name}
                      </div>
                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
                        Publisher: {pkg.publisher}
                      </div>
                    </div>
                    <button
                      type="button"
                      className="btn btn-danger btn-sm"
                      disabled={
                        !pkg.is_removable_bloat ||
                        busyKey === `appx:${pkg.package_full_name}`
                      }
                      onClick={() => setAppxConfirm(pkg)}
                    >
                      <IconTrash size={14} />
                      {busyKey === `appx:${pkg.package_full_name}`
                        ? "Removing..."
                        : "Uninstall"}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>
        </>
      ) : null}

      <Modal
        open={Boolean(diffSnapshot)}
        title={diffSnapshot ? `Snapshot Diff: ${diffSnapshot.label}` : "Snapshot Diff"}
        onClose={() => setDiffSnapshot(null)}
        wide
        footer={
          diffSnapshot ? (
            <>
              <button
                type="button"
                className="btn"
                onClick={() => setDiffSnapshot(null)}
              >
                Close
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => {
                  const id = diffSnapshot.id;
                  setDiffSnapshot(null);
                  handleRestoreSnapshot(id);
                }}
              >
                <IconUndo size={14} />
                Restore This Snapshot
              </button>
            </>
          ) : null
        }
      >
        {diffSnapshot ? (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th>Target</th>
                  <th>Previous Value</th>
                  <th>Applied Value</th>
                </tr>
              </thead>
              <tbody>
                {diffSnapshot.entries.map((entry, idx) => (
                  <tr key={idx}>
                    <td className="mono" style={{ fontSize: 11, wordBreak: "break-all" }}>
                      {entry.target ??
                        ([entry.hive, entry.path, entry.value_name]
                          .filter(Boolean)
                          .join("\\") ||
                          entry.service_name ||
                          `Entry #${idx + 1}`)}
                    </td>
                    <td className="mono-num" style={{ fontSize: 12 }}>
                      {formatTweakState(entry.previous_value)}
                    </td>
                    <td className="mono-num" style={{ fontSize: 12 }}>
                      {formatTweakState(entry.new_value)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </Modal>

      <Modal
        open={Boolean(regExportPreview)}
        title={
          regExportPreview
            ? `Exported Registry File (${regExportPreview.snapshotId}.reg)`
            : "Export .reg"
        }
        onClose={() => setRegExportPreview(null)}
        wide
        footer={
          regExportPreview ? (
            <>
              <button
                type="button"
                className="btn"
                onClick={() => setRegExportPreview(null)}
              >
                Close
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() =>
                  downloadRegFile(regExportPreview.snapshotId, regExportPreview.content)
                }
              >
                <IconDownload size={14} />
                Download .reg File
              </button>
            </>
          ) : null
        }
      >
        {regExportPreview ? (
          <pre className="code-block">{regExportPreview.content}</pre>
        ) : null}
      </Modal>

      <Modal
        open={Boolean(appxConfirm)}
        title="Confirm Current-User AppX Uninstall"
        onClose={() => setAppxConfirm(null)}
        footer={
          appxConfirm ? (
            <>
              <button
                type="button"
                className="btn"
                onClick={() => setAppxConfirm(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => handleRemoveAppx(appxConfirm)}
              >
                Uninstall for Current User
              </button>
            </>
          ) : null
        }
      >
        {appxConfirm ? (
          <>
            <p style={{ margin: 0 }}>
              Uninstall <strong>{appxConfirm.name}</strong>?
            </p>
            <p className="mono" style={{ margin: 0, fontSize: 12, color: "var(--text-muted)" }}>
              {appxConfirm.package_full_name}
            </p>
            <p style={{ margin: 0, fontSize: 13, color: "var(--warn)" }}>
              This runs Remove-AppxPackage for your Windows user account only and cannot be
              undone from offline JSON snapshots. Reinstalling requires Microsoft Store.
            </p>
          </>
        ) : null}
      </Modal>
    </>
  );
}
