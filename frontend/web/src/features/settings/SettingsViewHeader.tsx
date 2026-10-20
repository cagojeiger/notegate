import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

import { SectionHeader } from "../../shared/ui";
import { useSettingsInspector } from "./SettingsInspector";

export function SettingsViewHeader({ title, inspectorLabel, actions, disabled = false }: {
  title: string;
  inspectorLabel: string;
  actions?: ReactNode;
  disabled?: boolean;
}) {
  const inspector = useSettingsInspector();
  if (!inspector) return <SectionHeader title={title} actions={actions} />;
  return (
    <div className="mb-3 flex shrink-0 items-center justify-between gap-3">
      <h3 aria-label={title} className="min-w-0">
        <button type="button" aria-label={`${title}: ${inspectorLabel}`} disabled={disabled} onClick={inspector.onOpen}
          className="flex min-h-workbench-control max-w-full items-center gap-2 rounded-workbench-surface px-1 text-workbench font-semibold hover:bg-[var(--ng-hover)] disabled:cursor-not-allowed disabled:opacity-50">
          <span className="truncate">{title}</span><ChevronDown size={14} className="shrink-0 text-muted" />
        </button>
      </h3>
      {actions ? <div className="shrink-0">{actions}</div> : null}
    </div>
  );
}
