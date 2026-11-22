export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) {
    return "N/A";
  }
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const idx = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, idx);
  return `${value >= 100 || idx === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[idx]}`;
}

export function formatBps(bps: number | null | undefined): string {
  if (bps === null || bps === undefined || !Number.isFinite(bps)) {
    return "N/A";
  }
  return `${formatBytes(bps)}/s`;
}

export function formatPct(val: number | null | undefined, digits = 1): string {
  if (val === null || val === undefined || !Number.isFinite(val)) {
    return "N/A";
  }
  return `${val.toFixed(digits)}%`;
}

export function formatTimestamp(ts: string | number | null | undefined): string {
  if (ts === null || ts === undefined) return "Never";
  const d = typeof ts === "number" ? new Date(ts) : new Date(ts);
  if (Number.isNaN(d.getTime())) return String(ts);
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatTweakState(state: unknown): string {
  if (state === null || state === undefined) return "Unknown";
  if (typeof state === "string" || typeof state === "number" || typeof state === "boolean") {
    return String(state);
  }
  if (typeof state === "object") {
    const obj = state as Record<string, unknown>;
    if ("Dword" in obj) return `DWORD ${String(obj.Dword)}`;
    if ("Sz" in obj) return `"${String(obj.Sz)}"`;
    if ("ServiceStart" in obj) return `Start=${String(obj.ServiceStart)}`;
    if ("NotExisted" in obj) return "Not set";
    return JSON.stringify(obj);
  }
  return String(state);
}
