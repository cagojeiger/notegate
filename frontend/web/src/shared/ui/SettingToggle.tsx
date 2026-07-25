import type { ReactNode } from "react";

import { Badge } from "./Badge";

type SettingToggleProps = {
  icon: ReactNode;
  label: string;
  badge?: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
};

export function SettingToggle({
  icon,
  label,
  badge,
  checked,
  disabled,
  onChange
}: SettingToggleProps) {
  return (
    <div className="flex min-h-8 items-center justify-between gap-3">
      <div className="flex min-w-0 items-center gap-2">
        <span className="grid size-6 shrink-0 place-items-center text-muted" aria-hidden="true">
          {icon}
        </span>
        <span className="text-sm font-medium text-text">{label}</span>
        {badge ? <Badge>{badge}</Badge> : null}
      </div>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-6 w-10 shrink-0 rounded-full outline-none transition focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-not-allowed disabled:opacity-40 ${
          checked ? "bg-primary" : "bg-panel-strong"
        }`}
      >
        <span
          className={`absolute top-0.5 size-5 rounded-full bg-white shadow-sm transition ${
            checked ? "left-[18px]" : "left-0.5"
          }`}
        />
      </button>
    </div>
  );
}
