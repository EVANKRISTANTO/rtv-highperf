import type { SVGProps } from "react";

export interface IconProps extends SVGProps<SVGSVGElement> {
  size?: number;
}

const base = (size: number): SVGProps<SVGSVGElement> => ({
  width: size,
  height: size,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "square",
  strokeLinejoin: "miter",
  "aria-hidden": true,
});

export function IconTelemetryBrand({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <rect x="2.5" y="2.5" width="15" height="15" rx="2" />
      <polyline points="4.5,11 7.5,11 9,6.5 11.2,13.5 12.8,9.5 15.5,9.5" />
    </svg>
  );
}

export function IconRadar({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <circle cx="10" cy="10" r="7" />
      <circle cx="10" cy="10" r="3" />
      <line x1="10" y1="10" x2="15" y2="5" />
    </svg>
  );
}

export function IconGovernor({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M3.5 13.5a7.5 7.5 0 1 1 13 0" />
      <line x1="10" y1="11" x2="14" y2="7" />
      <circle cx="10" cy="11.5" r="1.3" fill="currentColor" stroke="none" />
      <line x1="5.5" y1="15.5" x2="14.5" y2="15.5" />
    </svg>
  );
}

export function IconCleaner({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M4 7.5h12" />
      <path d="M5.5 7.5l1 9h7l1-9" />
      <path d="M8 7.5V4.5h4v3" />
      <line x1="8.5" y1="10.5" x2="8.5" y2="13.5" />
      <line x1="11.5" y1="10.5" x2="11.5" y2="13.5" />
    </svg>
  );
}

export function IconStartup({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="10" y1="3" x2="10" y2="9.5" />
      <path d="M6.2 5.2a6.5 6.5 0 1 0 7.6 0" />
      <line x1="6" y1="16.5" x2="14" y2="16.5" />
    </svg>
  );
}

export function IconTweaker({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="3" y1="6" x2="17" y2="6" />
      <rect x="6" y="4" width="3" height="4" fill="var(--bg-surface)" />
      <line x1="3" y1="10.5" x2="17" y2="10.5" />
      <rect x="11.5" y="8.5" width="3" height="4" fill="var(--bg-surface)" />
      <line x1="3" y1="15" x2="17" y2="15" />
      <rect x="7.5" y="13" width="3" height="4" fill="var(--bg-surface)" />
    </svg>
  );
}

export function IconGear({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <circle cx="10" cy="10" r="2.8" />
      <path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M15.3 4.7l-1.4 1.4M6.1 13.9l-1.4 1.4" />
    </svg>
  );
}

export function IconSidebarCollapse({
  size = 16,
  collapsed = false,
  ...props
}: IconProps & { collapsed?: boolean }) {
  return (
    <svg {...base(size)} {...props}>
      <rect x="2.5" y="3.5" width="15" height="13" rx="1.5" />
      <line x1="7.5" y1="3.5" x2="7.5" y2="16.5" />
      {collapsed ? (
        <polyline points="11,8 13,10 11,12" />
      ) : (
        <polyline points="13,8 11,10 13,12" />
      )}
    </svg>
  );
}

export function IconMenu({ size = 18, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="3" y1="5.5" x2="17" y2="5.5" />
      <line x1="3" y1="10" x2="17" y2="10" />
      <line x1="3" y1="14.5" x2="17" y2="14.5" />
    </svg>
  );
}

export function IconSun({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <circle cx="10" cy="10" r="3.2" />
      <path d="M10 2.5v1.8M10 15.7v1.8M2.5 10h1.8M15.7 10h1.8M4.7 4.7l1.3 1.3M14 14l1.3 1.3M15.3 4.7L14 6M6 14l-1.3 1.3" />
    </svg>
  );
}

export function IconMoon({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M14.8 12.2A5.8 5.8 0 0 1 7.8 5.2a6 6 0 1 0 7 7z" />
    </svg>
  );
}

export function IconShieldCheck({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M10 2.5l6 2.3v4.7c0 3.7-2.5 6.8-6 8-3.5-1.2-6-4.3-6-8V4.8l6-2.3z" />
      <polyline points="7.3,10 9.3,12 13,8" />
    </svg>
  );
}

export function IconShieldAlert({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M10 2.5l6 2.3v4.7c0 3.7-2.5 6.8-6 8-3.5-1.2-6-4.3-6-8V4.8l6-2.3z" />
      <line x1="10" y1="7" x2="10" y2="10.8" />
      <line x1="10" y1="13.2" x2="10" y2="13.6" />
    </svg>
  );
}

export function IconRefresh({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M16 8.5A6.5 6.5 0 0 0 4.8 5.8" />
      <polyline points="4,3.5 4.5,6.2 7.2,5.8" />
      <path d="M4 11.5a6.5 6.5 0 0 0 11.2 2.7" />
      <polyline points="16,16.5 15.5,13.8 12.8,14.2" />
    </svg>
  );
}

export function IconAlertTriangle({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <polygon points="10,3 17.5,16.5 2.5,16.5" />
      <line x1="10" y1="8.2" x2="10" y2="12" />
      <line x1="10" y1="14.2" x2="10" y2="14.6" />
    </svg>
  );
}

export function IconInfo({ size = 16, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <circle cx="10" cy="10" r="7" />
      <line x1="10" y1="9" x2="10" y2="13.8" />
      <line x1="10" y1="6.4" x2="10" y2="6.8" />
    </svg>
  );
}

export function IconLock({ size = 14, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <rect x="4.5" y="9" width="11" height="8" rx="1.2" />
      <path d="M6.8 9V6.5a3.2 3.2 0 0 1 6.4 0V9" />
    </svg>
  );
}

export function IconSearch({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <circle cx="8.8" cy="8.8" r="5.3" />
      <line x1="12.8" y1="12.8" x2="16.8" y2="16.8" />
    </svg>
  );
}

export function IconX({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="5" y1="5" x2="15" y2="15" />
      <line x1="15" y1="5" x2="5" y2="15" />
    </svg>
  );
}

export function IconCheck({ size = 14, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <polyline points="4,10.5 8.2,14.5 16,6" />
    </svg>
  );
}

export function IconUndo({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <polyline points="6.5,4.5 3.5,7.5 6.5,10.5" />
      <path d="M4 7.5h7.5a4.5 4.5 0 0 1 0 9H8" />
    </svg>
  );
}

export function IconDownload({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="10" y1="3.5" x2="10" y2="12.5" />
      <polyline points="6.5,9.5 10,13 13.5,9.5" />
      <path d="M4 14.5v2h12v-2" />
    </svg>
  );
}

export function IconTrash({ size = 15, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="4" y1="6" x2="16" y2="6" />
      <path d="M6 6l.8 10.5h6.4L14 6" />
      <path d="M8 6V3.8h4V6" />
    </svg>
  );
}

export function IconExternalLink({ size = 14, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <path d="M11 3.5h5.5V9" />
      <line x1="9.5" y1="10.5" x2="16.2" y2="3.8" />
      <path d="M14.5 12v4.5h-11v-11H8" />
    </svg>
  );
}

export function IconPlus({ size = 14, ...props }: IconProps) {
  return (
    <svg {...base(size)} {...props}>
      <line x1="10" y1="4" x2="10" y2="16" />
      <line x1="4" y1="10" x2="16" y2="10" />
    </svg>
  );
}
