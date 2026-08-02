import type { PointerEventHandler, ReactNode } from "react";

import { usePointerDrag } from "../shared/hooks/usePointerDrag";
import { WORKBENCH_LAYOUT, type WorkbenchPanelMode } from "../shared/model/workbenchLayout";
import { ResizeSeparator } from "../shared/ui";
import { useUiStore } from "../stores/uiStore";

export function PrimarySidebarFrame({ mode, children, id }: { mode: WorkbenchPanelMode; children: ReactNode; id?: string }) {
  const width = useUiStore((state) => state.primaryWidth);
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
  visible
}: {
  visible: boolean;
}) {
  const value = useUiStore((state) => state.primaryWidth);
  const onValueChange = useUiStore((state) => state.setPrimaryWidth);
  const onPointerDown = useSidebarResize(value, onValueChange, 1);

  return <SidebarResizeHandle visible={visible} value={value} min={WORKBENCH_LAYOUT.minPrimaryWidth} max={WORKBENCH_LAYOUT.maxPrimaryWidth} label="Resize Files sidebar" controls="primary-sidebar-panel" onPointerDown={onPointerDown} onValueChange={onValueChange} />;
}

export function AuxiliarySidebarResizeHandle({
  visible
}: {
  visible: boolean;
}) {
  const value = useUiStore((state) => state.auxiliaryWidth);
  const onValueChange = useUiStore((state) => state.setAuxiliaryWidth);
  const onPointerDown = useSidebarResize(value, onValueChange, -1);

  return <SidebarResizeHandle visible={visible} value={value} min={WORKBENCH_LAYOUT.minAuxiliaryWidth} max={WORKBENCH_LAYOUT.maxAuxiliaryWidth} label="Resize Inspector" controls="auxiliary-sidebar-panel" overlayBoundary reverseArrowDirection onPointerDown={onPointerDown} onValueChange={onValueChange} />;
}

function useSidebarResize(value: number, onValueChange: (value: number) => void, direction: 1 | -1): PointerEventHandler<HTMLDivElement> {
  const startPointerDrag = usePointerDrag();

  return (event) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = value;
    startPointerDrag((moveEvent) => onValueChange(startWidth + direction * (moveEvent.clientX - startX)));
  };
}

function SidebarResizeHandle({
  visible,
  value,
  min,
  max,
  label,
  controls,
  overlayBoundary = false,
  reverseArrowDirection = false,
  onPointerDown,
  onValueChange
}: {
  visible: boolean;
  value: number;
  min: number;
  max: number;
  label: string;
  controls: string;
  overlayBoundary?: boolean;
  reverseArrowDirection?: boolean;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
  onValueChange: (value: number) => void;
}) {
  if (!visible) return null;
  return (
    <div className={`relative hidden shrink-0 bg-transparent md:block ${overlayBoundary ? "w-0" : "w-1"}`}>
      <ResizeSeparator
        orientation="vertical"
        label={label}
        value={value}
        min={min}
        max={max}
        step={10}
        valueText={`${value} pixels`}
        controls={controls}
        reverseArrowDirection={reverseArrowDirection}
        onPointerDown={onPointerDown}
        onValueChange={onValueChange}
      />
    </div>
  );
}

export function AuxiliarySidebarFrame({ mode, children, id }: { mode: WorkbenchPanelMode; children: ReactNode; id?: string }) {
  const width = useUiStore((state) => state.auxiliaryWidth);
  if (mode === "hidden") return null;

  const style = mode === "docked" ? { width } : undefined;
  const className =
    mode === "docked"
      ? "min-h-0 flex shrink-0"
      : "fixed inset-x-0 bottom-[calc(3.5rem+env(safe-area-inset-bottom))] z-40 flex h-[70dvh] min-h-0 max-w-none rounded-t-2xl shadow-2xl";

  return (
    <div id={id} style={style} className={className}>
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
