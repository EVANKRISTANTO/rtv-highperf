import { useId } from "react";

interface SparklineProps {
  title: string;
  subtitle: string;
  values: number[];
  unit?: string;
  maxValue?: number;
  tone?: "accent" | "warn" | "danger";
}

export function Sparkline({
  title,
  subtitle,
  values,
  unit = "%",
  maxValue = 100,
  tone = "accent",
}: SparklineProps) {
  const gradId = useId();
  const width = 420;
  const height = 96;
  const padY = 8;

  const strokeColor =
    tone === "danger"
      ? "var(--danger)"
      : tone === "warn"
      ? "var(--warn)"
      : "var(--accent)";

  const latest = values.length > 0 ? values[values.length - 1] : null;
  const peak = values.length > 0 ? Math.max(...values) : null;

  const points = (() => {
    if (values.length === 0) return "";
    if (values.length === 1) {
      const y = height - padY - (Math.min(values[0], maxValue) / maxValue) * (height - padY * 2);
      return `0,${y.toFixed(1)} ${width},${y.toFixed(1)}`;
    }
    return values
      .map((v, idx) => {
        const x = (idx / (values.length - 1)) * width;
        const clamped = Math.max(0, Math.min(v, maxValue));
        const y = height - padY - (clamped / maxValue) * (height - padY * 2);
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  })();

  const areaPoints = points ? `0,${height} ${points} ${width},${height}` : "";

  return (
    <section className="panel" aria-label={title}>
      <div className="panel-header" style={{ marginBottom: 8 }}>
        <div>
          <h3 className="panel-title">{title}</h3>
          <p className="panel-subtitle">{subtitle}</p>
        </div>
        <div style={{ textAlign: "right" }}>
          <div className="mono-num" style={{ fontSize: 18, fontWeight: 700, color: strokeColor }}>
            {latest !== null ? `${latest.toFixed(1)}${unit}` : "Waiting"}
          </div>
          <div className="mono-num" style={{ fontSize: 11, color: "var(--text-muted)" }}>
            {peak !== null ? `Peak ${peak.toFixed(1)}${unit}` : "No samples"}
          </div>
        </div>
      </div>

      {values.length > 0 ? (
        <svg
          viewBox={`0 0 ${width} ${height}`}
          style={{ width: "100%", height: 96, display: "block" }}
          role="img"
          aria-label={`${title}: current ${latest?.toFixed(1)}${unit}`}
        >
          <defs>
            <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={strokeColor} stopOpacity="0.24" />
              <stop offset="100%" stopColor={strokeColor} stopOpacity="0.01" />
            </linearGradient>
          </defs>
          <line
            x1="0"
            y1={height / 2}
            x2={width}
            y2={height / 2}
            stroke="var(--border-subtle)"
            strokeDasharray="3 4"
            strokeWidth="1"
          />
          <polygon points={areaPoints} fill={`url(#${gradId})`} />
          <polyline
            points={points}
            fill="none"
            stroke={strokeColor}
            strokeWidth="2"
            strokeLinejoin="round"
            strokeLinecap="round"
          />
        </svg>
      ) : (
        <div
          style={{
            height: 96,
            display: "grid",
            placeItems: "center",
            borderRadius: "var(--radius-md)",
            border: "1px dashed var(--border-subtle)",
            color: "var(--text-muted)",
            fontSize: 12,
          }}
        >
          Waiting for telemetry ticks from backend
        </div>
      )}
    </section>
  );
}
