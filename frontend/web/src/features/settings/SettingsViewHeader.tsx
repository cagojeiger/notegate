import { PanelRight } from "lucide-react";
import type { ReactNode } from "react";

import { IconButton } from "../../shared/ui";
import { useSettingsInspector } from "./SettingsInspector";

export function SettingsViewHeader({ title, inspectorLabel, leading, actions, disabled = false }: {
  title: string;
  inspectorLabel?: string;
  leading?: ReactNode;
  actions?: ReactNode;
  disabled?: boolean;
}) {
  const inspector = useSettingsInspector();
  return (
    <div data-settings-heading className="flex min-h-workbench-control shrink-0 items-center justify-between gap-3">
      <div className="flex min-w-0 items-center gap-3">
        {leading ? <div className="shrink-0">{leading}</div> : null}
        <h2 tabIndex={-1} className="min-w-0 truncate text-workbench font-semibold outline-none">{title}</h2>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {actions}
        {inspector && inspectorLabel ? <IconButton label={inspectorLabel} disabled={disabled} onClick={inspector.onOpen}
          expanded={inspector.open} controls={inspector.open ? inspector.id : undefined}><PanelRight size={16} /></IconButton> : null}
      </div>
    </div>
  );
}
