import { createContext, useContext, useLayoutEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { SectionHeader } from "../../shared/ui";

import { useSettingsScroll } from "./settingsViewState";

type Inspector = {
  container: HTMLDivElement | null;
  navigationContainer: HTMLDivElement | null;
  onOpen: () => void;
  onNavigate: () => void;
};

export const SettingsInspectorContext = createContext<Inspector | null>(null);

export function useSettingsInspector() {
  return useContext(SettingsInspectorContext);
}

// Features own their controls; the workbench owns their presentation.
export function SettingsControls({ children, slot = "content" }: { children: ReactNode; slot?: "navigation" | "content" }) {
  const inspector = useSettingsInspector();
  if (!inspector) return children;
  const container = slot === "navigation" ? inspector.navigationContainer : inspector.container;
  return container ? createPortal(children, container) : null;
}

export function SettingsInspectorSection({ title, children }: { title: string; children: ReactNode }) {
  return <section className="border-t border-seam pt-4 first:border-t-0 first:pt-0">
    <SectionHeader title={title} />{children}
  </section>;
}

export function useSettingsNavigationScroll(key: string, ready = true) {
  const inspector = useSettingsInspector();
  const scroll = useSettingsScroll(`settings-navigation-${key}`);
  useLayoutEffect(() => {
    if (ready) return scroll(inspector?.navigationContainer ?? null);
  }, [scroll, inspector?.navigationContainer, ready]);
}
