import { useLayoutEffect, useRef, type ReactNode } from "react";

import { SettingsViewHeader } from "./SettingsViewHeader";
import { useSettingsInspector } from "./SettingsInspector";
import { useSettingsScroll } from "./settingsViewState";

export type SettingsPageRenderer = (body: ReactNode, inspector: ReactNode) => ReactNode;

// Each view supplies content; this frame owns its heading, spacing and scroll boundary.
export function SettingsPage({ title, inspectorLabel, leading, actions, summary, children, scrollKey,
  contained = false, disabled = false, label = title, contentLabel
}: {
  title: string;
  inspectorLabel?: string;
  leading?: ReactNode;
  actions?: ReactNode;
  summary?: ReactNode;
  children: ReactNode;
  scrollKey: string;
  contained?: boolean;
  disabled?: boolean;
  label?: string;
  contentLabel?: string;
}) {
  // Restore per-view scroll through the ref without remounting persistent controls.
  const scroll = useSettingsScroll(`settings-content-${scrollKey}`);
  const setInspectorAvailable = useSettingsInspector()?.setAvailable;
  const heading = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    setInspectorAvailable?.(!!inspectorLabel);
  }, [setInspectorAvailable, inspectorLabel]);
  useLayoutEffect(() => {
    // Selecting an item can replace the focused list or form in the body.
    if (document.activeElement === document.body) heading.current?.querySelector("h2")?.focus({ preventScroll: true });
  }, [scrollKey]);
  return (
    <section aria-label={label} className={`ng-settings-page flex h-full min-h-0 min-w-0 flex-col gap-4${summary ? " ng-settings-with-summary" : ""}`}>
      <div ref={heading} className="shrink-0"><SettingsViewHeader title={title} inspectorLabel={inspectorLabel} leading={leading} actions={actions} disabled={disabled} /></div>
      {summary ? <div className="shrink-0">{summary}</div> : null}
      <div ref={scroll} data-settings-content role={contentLabel ? "tabpanel" : undefined} aria-label={contentLabel}
        className={`min-h-0 min-w-0 flex-1 ${contained ? "flex flex-col overflow-hidden" : "overflow-y-auto overscroll-contain [scrollbar-gutter:stable]"}`}>
        {children}
      </div>
    </section>
  );
}
