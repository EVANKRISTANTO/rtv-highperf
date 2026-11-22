import type { ReactNode } from "react";
import { IconAlertTriangle, IconInfo, IconLock, IconShieldAlert } from "./Icons";
import type { ParsedAppError } from "../lib/tauri";

export function PreviewModeBanner() {
  return (
    <div className="banner banner-warn" role="status">
      <div className="banner-body">
        <span className="banner-icon">
          <IconInfo />
        </span>
        <div>
          <p className="banner-title">Backend disconnected / Browser preview mode</p>
          <p className="banner-desc">
            Running outside the Tauri desktop host. Live Win32 telemetry, cleaner scans,
            startup enumeration, and registry tweaks require launching the compiled desktop
            application. No placeholder metrics are fabricated.
          </p>
        </div>
      </div>
    </div>
  );
}

export function ElevationPromptBanner({
  message,
  onElevate,
  busy = false,
}: {
  message: string;
  onElevate: () => void;
  busy?: boolean;
}) {
  return (
    <div className="banner banner-warn" role="alert">
      <div className="banner-body">
        <span className="banner-icon">
          <IconLock />
        </span>
        <div>
          <p className="banner-title">Administrator privileges required</p>
          <p className="banner-desc">{message}</p>
        </div>
      </div>
      <button
        type="button"
        className="btn btn-primary btn-sm"
        onClick={onElevate}
        disabled={busy}
      >
        <IconShieldAlert size={14} />
        {busy ? "Restarting..." : "Restart as Administrator"}
      </button>
    </div>
  );
}

export function ErrorBanner({
  error,
  onDismiss,
  onElevate,
  elevating = false,
}: {
  error: ParsedAppError | null;
  onDismiss?: () => void;
  onElevate?: () => void;
  elevating?: boolean;
}) {
  if (!error) return null;
  if (error.isElevationRequired && onElevate) {
    return (
      <ElevationPromptBanner
        message={error.message}
        onElevate={onElevate}
        busy={elevating}
      />
    );
  }
  return (
    <div className="banner banner-danger" role="alert">
      <div className="banner-body">
        <span className="banner-icon">
          <IconAlertTriangle />
        </span>
        <div>
          <p className="banner-title">Operation failed ({error.kind})</p>
          <p className="banner-desc">{error.message}</p>
        </div>
      </div>
      {onDismiss ? (
        <button type="button" className="btn btn-ghost btn-sm" onClick={onDismiss}>
          Dismiss
        </button>
      ) : null}
    </div>
  );
}

export function LoadingState({
  title,
  detail,
}: {
  title: string;
  detail: string;
}) {
  return (
    <div className="state-box" role="status" aria-live="polite">
      <p className="state-box-title">{title}</p>
      <p className="state-box-desc">{detail}</p>
    </div>
  );
}

export function EmptyState({
  title,
  detail,
  action,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
}) {
  return (
    <div className="state-box">
      <p className="state-box-title">{title}</p>
      <p className="state-box-desc">{detail}</p>
      {action ? <div style={{ marginTop: 4 }}>{action}</div> : null}
    </div>
  );
}
