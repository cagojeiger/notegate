import { createContext, useContext, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { PanelSection, SectionHeader } from "../../shared/ui";

type Inspector = {
  id: string;
  open: boolean;
  container: HTMLDivElement | null;
  onOpen: () => void;
  setAvailable: (available: boolean) => void;
};

export const SettingsInspectorContext = createContext<Inspector | null>(null);

export function useSettingsInspector() {
  return useContext(SettingsInspectorContext);
}

// Features own their controls; the workbench owns their presentation.
export function SettingsControls({ children }: { children: ReactNode }) {
  const inspector = useSettingsInspector();
  if (!inspector) return children;
  const container = inspector.container;
  return container ? createPortal(children, container) : null;
}

export function SettingsInspectorSection({ title, children }: { title: string; children: ReactNode }) {
  return <PanelSection>
    <SectionHeader title={title} />{children}
  </PanelSection>;
}
