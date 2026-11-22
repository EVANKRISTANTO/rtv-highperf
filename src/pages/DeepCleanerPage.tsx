import { useMemo, useState } from "react";
import {
  IconAlertTriangle,
  IconCheck,
  IconLock,
  IconSearch,
  IconTrash,
} from "../components/Icons";
import { Modal } from "../components/Modal";
import { EmptyState, LoadingState } from "../components/StateViews";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { formatBytes } from "../lib/format";
import {
  parseAppError,
  tauriApi,
  type CleanCategoryReport,
  type CleanResult,
  type ParsedAppError,
} from "../lib/tauri";

interface DeepCleanerPageProps {
  connected: boolean;
  onError: (err: ParsedAppError) => void;
}

export function DeepCleanerPage({ connected, onError }: DeepCleanerPageProps) {
  const [categories, setCategories] = useState<CleanCategoryReport[] | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [moveToRecycleBin, setMoveToRecycleBin] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [lastResult, setLastResult] = useState<CleanResult | null>(null);

  async function handleScan() {
    setScanning(true);
    setLastResult(null);
    try {
      const report = await tauriApi.scanCleaner();
      setCategories(report);
      const defaults = new Set<string>();
      for (const item of report) {
        if (!item.opt_in_default_off && item.bytes > 0) {
          defaults.add(item.id);
        }
      }
      setSelectedIds(defaults);
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setScanning(false);
    }
  }

  function toggleCategory(id: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const selectedSummary = useMemo(() => {
    if (!categories) return { count: 0, bytes: 0, files: 0, requiresAdmin: false };
    let count = 0;
    let bytes = 0;
    let files = 0;
    let requiresAdmin = false;
    for (const c of categories) {
      if (selectedIds.has(c.id)) {
        count += 1;
        bytes += c.bytes;
        files += c.file_count;
        if (c.requires_admin) requiresAdmin = true;
      }
    }
    return { count, bytes, files, requiresAdmin };
  }, [categories, selectedIds]);

  async function executeClean() {
    setConfirmOpen(false);
    setCleaning(true);
    try {
      const res = await tauriApi.runCleaner(Array.from(selectedIds), moveToRecycleBin);
      setLastResult(res);
      await handleScanAfterClean();
    } catch (err) {
      onError(parseAppError(err));
    } finally {
      setCleaning(false);
    }
  }

  async function handleScanAfterClean() {
    try {
      const report = await tauriApi.scanCleaner();
      setCategories(report);
    } catch {
      // Keep clean result visible even if follow-up scan fails
    }
  }

  if (!connected) {
    return (
      <EmptyState
        title="Deep Cleaner requires the Windows desktop backend"
        detail="Parallel directory scanning (jwalk + rayon) and allowlisted cache removal run natively in Rust. Launch the desktop app to scan temp, shader, update, and browser caches."
      />
    );
  }

  return (
    <>
      <section className="panel" aria-label="Deep Cleaner Controls">
        <div className="toolbar-row">
          <div>
            <h2 className="panel-title">Parallel Deep Cleaner</h2>
            <p className="panel-subtitle">
              Strict allowlisted cache paths only. Locked files in active apps are skipped safely.
            </p>
          </div>
          <div className="toolbar-group">
            <button
              type="button"
              className="btn btn-primary"
              disabled={scanning || cleaning}
              onClick={handleScan}
            >
              <IconSearch />
              {scanning ? "Scanning..." : "Scan Junk"}
            </button>
            <button
              type="button"
              className="btn btn-danger"
              disabled={cleaning || scanning || selectedSummary.count === 0}
              onClick={() => setConfirmOpen(true)}
            >
              <IconTrash />
              {cleaning
                ? "Cleaning..."
                : `Clean Selected (${formatBytes(selectedSummary.bytes)})`}
            </button>
          </div>
        </div>

        <div
          style={{
            marginTop: 14,
            paddingTop: 12,
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 16,
            flexWrap: "wrap",
          }}
        >
          <ToggleSwitch
            checked={moveToRecycleBin}
            onChange={setMoveToRecycleBin}
            label="Move deleted files to Recycle Bin"
            description={
              moveToRecycleBin
                ? "Safer: moves cache files to Recycle Bin where supported instead of immediate unlink."
                : "Permanent Delete: unlinks temporary/cache files immediately to reclaim disk space."
            }
          />
          {categories && categories.length > 0 ? (
            <div className="toolbar-group">
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => {
                  const safe = new Set(
                    categories.filter((c) => !c.opt_in_default_off).map((c) => c.id)
                  );
                  setSelectedIds(safe);
                }}
              >
                Select Safe Defaults
              </button>
              <button
                type="button"
                className="btn btn-sm btn-ghost"
                onClick={() => setSelectedIds(new Set())}
              >
                Clear Selection
              </button>
            </div>
          ) : null}
        </div>
      </section>

      {lastResult ? (
        <section className="banner banner-accent" role="status">
          <div className="banner-body">
            <span className="banner-icon">
              <IconCheck />
            </span>
            <div>
              <p className="banner-title">Clean pass complete</p>
              <p className="banner-desc mono-num">
                Freed {formatBytes(lastResult.freed_bytes)} across{" "}
                {lastResult.deleted_files.toLocaleString()} deleted files. Skipped{" "}
                {lastResult.skipped_locked.toLocaleString()} locked files.
              </p>
              {lastResult.errors.length > 0 ? (
                <ul style={{ margin: "6px 0 0", paddingLeft: 18, fontSize: 12 }}>
                  {lastResult.errors.slice(0, 6).map((e, i) => (
                    <li key={i}>{e}</li>
                  ))}
                </ul>
              ) : null}
            </div>
          </div>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setLastResult(null)}
          >
            Dismiss
          </button>
        </section>
      ) : null}

      {scanning && !categories ? (
        <LoadingState
          title="Scanning allowlisted cache folders..."
          detail="Walking temp, shader cache, crash dump, Windows Update, and browser cache directories."
        />
      ) : !categories ? (
        <EmptyState
          title="No junk scan executed yet"
          detail="Click Scan Junk to measure real file counts and byte sizes across Windows temp, GPU shader caches, browser caches, and optional developer caches."
          action={
            <button type="button" className="btn btn-primary" onClick={handleScan}>
              <IconSearch />
              Scan Junk Now
            </button>
          }
        />
      ) : categories.length === 0 ? (
        <EmptyState
          title="No cleanable categories reported"
          detail="The backend returned zero cleaner categories for this environment."
        />
      ) : (
        <section className="panel" aria-label="Scanned Cache Categories">
          <div className="panel-header">
            <div>
              <h3 className="panel-title">Scanned Categories</h3>
              <p className="panel-subtitle mono-num">
                Selected: {selectedSummary.count} categories,{" "}
                {selectedSummary.files.toLocaleString()} files,{" "}
                {formatBytes(selectedSummary.bytes)}
              </p>
            </div>
          </div>

          <div className="finding-list">
            {categories.map((cat) => {
              const checked = selectedIds.has(cat.id);
              return (
                <label
                  key={cat.id}
                  className="tweak-card"
                  style={{ cursor: "pointer", alignItems: "center" }}
                >
                  <div style={{ display: "flex", alignItems: "flex-start", gap: 12, minWidth: 0 }}>
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleCategory(cat.id)}
                      style={{ marginTop: 3, width: 16, height: 16, accentColor: "var(--accent)" }}
                    />
                    <div style={{ minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                        <span style={{ fontWeight: 600, fontSize: 14 }}>{cat.name}</span>
                        {cat.requires_admin ? (
                          <span className="badge badge-warn">
                            <IconLock size={11} />
                            Admin
                          </span>
                        ) : null}
                        {cat.opt_in_default_off ? (
                          <span className="badge badge-warn">
                            <IconAlertTriangle size={11} />
                            Dev Cache (Opt-in)
                          </span>
                        ) : null}
                      </div>
                      <div
                        className="mono"
                        style={{
                          fontSize: 11,
                          color: "var(--text-muted)",
                          marginTop: 4,
                          wordBreak: "break-all",
                        }}
                      >
                        {cat.paths.join(" | ")}
                      </div>
                      {cat.warning ? (
                        <div style={{ fontSize: 12, color: "var(--warn)", marginTop: 4 }}>
                          {cat.warning}
                        </div>
                      ) : null}
                    </div>
                  </div>
                  <div style={{ textAlign: "right", flexShrink: 0 }}>
                    <div className="mono-num" style={{ fontWeight: 700, fontSize: 14 }}>
                      {formatBytes(cat.bytes)}
                    </div>
                    <div className="mono-num" style={{ fontSize: 11, color: "var(--text-muted)" }}>
                      {cat.file_count.toLocaleString()} files
                    </div>
                  </div>
                </label>
              );
            })}
          </div>
        </section>
      )}

      <Modal
        open={confirmOpen}
        title="Confirm Deep Clean"
        onClose={() => setConfirmOpen(false)}
        footer={
          <>
            <button
              type="button"
              className="btn"
              onClick={() => setConfirmOpen(false)}
            >
              Cancel
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={executeClean}
            >
              {moveToRecycleBin ? "Move to Recycle Bin" : "Delete Permanently"}
            </button>
          </>
        }
      >
        <p style={{ margin: 0 }}>
          You are about to clean <strong>{selectedSummary.count}</strong> categories totaling{" "}
          <strong className="mono-num">{formatBytes(selectedSummary.bytes)}</strong> (
          <span className="mono-num">{selectedSummary.files.toLocaleString()}</span> files).
        </p>
        <p style={{ margin: 0, fontSize: 13, color: "var(--text-muted)" }}>
          Mode: {moveToRecycleBin ? "Move to Recycle Bin" : "Permanent Delete"}. Files
          currently locked by running applications will be skipped cleanly.
        </p>
        {selectedSummary.requiresAdmin ? (
          <div className="banner banner-warn">
            <div className="banner-body">
              <span className="banner-icon">
                <IconLock />
              </span>
              <p className="banner-desc">
                One or more selected categories require Administrator privileges. If not
                elevated, the backend will prompt to restart as Administrator.
              </p>
            </div>
          </div>
        ) : null}
      </Modal>
    </>
  );
}
