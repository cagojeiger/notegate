import type { ReactNode } from "react";

import { SettingsViewHeader } from "./SettingsViewHeader";
import { useSettingsScroll } from "./settingsViewState";

// Each view supplies content; this frame owns its heading, spacing and scroll boundary.
export function SettingsPage({ title, inspectorLabel, actions, summary, children, scrollKey,
  contained = false, disabled = false, label = title, contentLabel
}: {
  title: string;
  inspectorLabel: string;
  actions?: ReactNode;
  summary?: ReactNode;
  children: ReactNode;
  scrollKey: string;
  contained?: boolean;
  disabled?: boolean;
  label?: string;
  contentLabel?: string;
}) {
  const scroll = useSettingsScroll(`settings-content-${scrollKey}`);
  return (
    <section aria-label={label} className={`ng-settings-page flex h-full min-h-0 min-w-0 flex-col gap-4${summary ? " ng-settings-with-summary" : ""}`}>
      <SettingsViewHeader title={title} inspectorLabel={inspectorLabel} actions={actions} disabled={disabled} />
      {summary ? <div className="shrink-0">{summary}</div> : null}
      <div key={scrollKey} ref={scroll} data-settings-content role={contentLabel ? "tabpanel" : undefined} aria-label={contentLabel}
        className={`min-h-0 min-w-0 flex-1 ${contained ? "flex flex-col overflow-hidden" : "overflow-y-auto overscroll-contain [scrollbar-gutter:stable]"}`}>
        {children}
      </div>
    </section>
  );
}
