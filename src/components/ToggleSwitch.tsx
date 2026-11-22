interface ToggleSwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
  id?: string;
}

export function ToggleSwitch({
  checked,
  onChange,
  label,
  description,
  disabled = false,
  id,
}: ToggleSwitchProps) {
  return (
    <div className="toggle-row">
      <div>
        <div style={{ fontWeight: 600, fontSize: 13 }}>{label}</div>
        {description ? (
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>
            {description}
          </div>
        ) : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        className="toggle-switch"
        onClick={() => onChange(!checked)}
      >
        <span className="toggle-thumb" />
      </button>
    </div>
  );
}
