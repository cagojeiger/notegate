import type { PointerEventHandler, ReactNode } from "react";

import { WORKBENCH_LAYOUT, type WorkbenchPanelMode } from "../shared/model/workbenchLayout";
import { ResizeSeparator } from "../shared/ui";

export function PrimarySidebarFrame({ mode, width, children, id }: { mode: WorkbenchPanelMode; width: number; children: ReactNode; id?: string }) {
  if (mode === "hidden") return null;

  const style = mode === "docked" ? { width } : { width: WORKBENCH_LAYOUT.mobilePrimaryWidthPercent, maxWidth: WORKBENCH_LAYOUT.mobilePrimaryMaxWidth };
  const className =
    mode === "docked"
      ? "min-h-0 flex shrink-0"
      : "fixed bottom-[calc(3.5rem+env(safe-area-inset-bottom))] left-0 top-[calc(3rem+env(safe-area-inset-top))] z-40 flex min-h-0 shadow-2xl";

  return (
    <div id={id} style={style} className={className}>
      {children}
    </div>
  );
}

export function PrimarySidebarResizeHandle({
  visible,
  value,
  onPointerDown,
  onValueChange
}: {
  visible: boolean;
  value: number;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
  onValueChange: (value: number) => void;
}) {
  if (!visible) return null;
  return (
    <div className="relative hidden w-1 shrink-0 bg-transparent md:block">
      <ResizeSeparator
        orientation="vertical"
        label="Resize Files sidebar"
        value={value}
        min={WORKBENCH_LAYOUT.minPrimaryWidth}
        max={WORKBENCH_LAYOUT.maxPrimaryWidth}
        step={10}
        valueText={`${value} pixels`}
        controls="primary-sidebar-panel"
        onPointerDown={onPointerDown}
        onValueChange={onValueChange}
      />
    </div>
  );
}

export function AuxiliarySidebarFrame({ mode, children }: { mode: WorkbenchPanelMode; children: ReactNode }) {
  if (mode === "hidden") return null;

  const style = mode === "docked" ? { width: WORKBENCH_LAYOUT.auxiliaryWidth } : undefined;
  const className =
    mode === "docked"
      ? "min-h-0 flex shrink-0"
      : "fixed inset-x-0 bottom-[calc(3.5rem+env(safe-area-inset-bottom))] z-40 flex h-[70vh] min-h-0 max-w-none rounded-t-2xl shadow-2xl";

  return (
    <div style={style} className={className}>
      {children}
    </div>
  );
}

export function PanelOverlay({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  if (!visible) return null;
  return (
    <button
      type="button"
      aria-label="Close panel"
      onClick={onClose}
      className="fixed inset-x-0 bottom-[calc(3.5rem+env(safe-area-inset-bottom))] top-[calc(3rem+env(safe-area-inset-top))] z-30 bg-black/40"
    />
  );
}
