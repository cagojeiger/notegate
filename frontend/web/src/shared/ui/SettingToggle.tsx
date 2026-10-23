import type { ReactNode } from "react";

import { Badge } from "./Badge";

type SettingToggleProps = {
  icon: ReactNode;
  label: string;
  badge?: string;
  checked: boolean;
  disabled: boolean;
  // Saving blocks activation while retaining focus; unavailable controls use disabled.
  pending?: boolean;
  onChange: (checked: boolean) => void;
};

export function SettingToggle({
  icon,
  label,
  badge,
  checked,
  disabled,
  pending = false,
  onChange
}: SettingToggleProps) {
  return (
    <div className="flex min-h-workbench-control items-center justify-between gap-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="grid size-5 shrink-0 place-items-center text-muted" aria-hidden="true">
          {icon}
        </span>
        <span className="font-ui text-workbench font-medium text-text">{label}</span>
        {badge ? <Badge>{badge}</Badge> : null}
      </div>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={checked}
        aria-disabled={pending || undefined}
        aria-busy={pending || undefined}
        disabled={disabled}
        onClick={() => { if (!pending) onChange(!checked); }}
        className="grid min-h-[var(--ng-min-target-size)] min-w-[var(--ng-min-target-size)] shrink-0 place-items-center rounded-workbench outline-none transition disabled:cursor-not-allowed disabled:opacity-40 aria-disabled:cursor-not-allowed aria-disabled:opacity-40"
      >
        <span
          data-switch-track
          aria-hidden="true"
          className={`relative block h-6 w-10 rounded-full transition ${checked ? "bg-primary [--ng-control-focus-color:var(--ng-primary-contrast)]" : "bg-panel-strong"}`}
        >
          <span className={`absolute top-0.5 size-5 rounded-full bg-white shadow-sm transition ${checked ? "left-[18px]" : "left-0.5"}`} />
        </span>
      </button>
    </div>
  );
}
