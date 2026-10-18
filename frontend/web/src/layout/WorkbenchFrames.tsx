import type { PointerEventHandler, ReactNode } from "react";

import { usePointerDrag } from "../shared/hooks/usePointerDrag";
import { WORKBENCH_LAYOUT, type WorkbenchPanelMode } from "../shared/model/workbenchLayout";
import { ResizeSeparator } from "../shared/ui";
import { useUiStore } from "../stores/uiStore";
import { resolveSidebarWidths, type SidebarLayout } from "./workbenchLayout";

function useSidebarWidths(layout?: SidebarLayout) {
  const primaryWidth = useUiStore((state) => state.primaryWidth);
  const auxiliaryWidth = useUiStore((state) => state.auxiliaryWidth);
  return resolveSidebarWidths(primaryWidth, auxiliaryWidth, layout);
}

export function PrimarySidebarFrame({ mode, children, id, sidebarLayout }: { mode: WorkbenchPanelMode; children: ReactNode; id?: string; sidebarLayout?: SidebarLayout }) {
  const { primaryWidth: width } = useSidebarWidths(sidebarLayout);
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
  sidebarLayout
}: {
  visible: boolean;
  sidebarLayout?: SidebarLayout;
}) {
  const { primaryWidth: value } = useSidebarWidths(sidebarLayout);
  const max = sidebarMaximum("primary", sidebarLayout);
  const onValueChange = (nextWidth: number) => resizeSidebar("primary", nextWidth, sidebarLayout);
  const onPointerDown = useSidebarResize(value, onValueChange, 1);

  return <SidebarResizeHandle visible={visible} value={value} min={WORKBENCH_LAYOUT.minPrimaryWidth} max={max} label="Resize Files sidebar" controls="primary-sidebar-panel" onPointerDown={onPointerDown} onValueChange={onValueChange} />;
}

export function AuxiliarySidebarResizeHandle({
  visible,
  sidebarLayout
}: {
  visible: boolean;
  sidebarLayout?: SidebarLayout;
}) {
  const { auxiliaryWidth: value } = useSidebarWidths(sidebarLayout);
  const max = sidebarMaximum("auxiliary", sidebarLayout);
  const onValueChange = (nextWidth: number) => resizeSidebar("auxiliary", nextWidth, sidebarLayout);
  const onPointerDown = useSidebarResize(value, onValueChange, -1);

  return <SidebarResizeHandle visible={visible} value={value} min={WORKBENCH_LAYOUT.minAuxiliaryWidth} max={max} label="Resize Inspector" controls="auxiliary-sidebar-panel" reverseArrowDirection onPointerDown={onPointerDown} onValueChange={onValueChange} />;
}

function sidebarMaximum(side: "primary" | "auxiliary", layout?: SidebarLayout): number {
  const ownMax = side === "primary" ? WORKBENCH_LAYOUT.maxPrimaryWidth : WORKBENCH_LAYOUT.maxAuxiliaryWidth;
  if (!layout) return ownMax;
  const otherMin = side === "primary"
    ? layout.auxiliaryDocked ? WORKBENCH_LAYOUT.minAuxiliaryWidth : 0
    : layout.primaryDocked ? WORKBENCH_LAYOUT.minPrimaryWidth : 0;
  return Math.min(ownMax, layout.availableWidth - otherMin);
}

function resizeSidebar(side: "primary" | "auxiliary", width: number, layout?: SidebarLayout) {
  const state = useUiStore.getState();
  const min = side === "primary" ? WORKBENCH_LAYOUT.minPrimaryWidth : WORKBENCH_LAYOUT.minAuxiliaryWidth;
  const nextWidth = Math.max(min, Math.min(sidebarMaximum(side, layout), width));
  if (side === "primary") {
    state.setPrimaryWidth(nextWidth);
    if (layout?.auxiliaryDocked) state.setAuxiliaryWidth(Math.min(state.auxiliaryWidth, layout.availableWidth - nextWidth));
  } else {
    state.setAuxiliaryWidth(nextWidth);
    if (layout?.primaryDocked) state.setPrimaryWidth(Math.min(state.primaryWidth, layout.availableWidth - nextWidth));
  }
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
  reverseArrowDirection?: boolean;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
  onValueChange: (value: number) => void;
}) {
  if (!visible) return null;
  return (
    <div className="relative hidden w-0 shrink-0 bg-transparent md:block">
      <ResizeSeparator
        orientation="vertical"
        lineVisible={false}
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

export function AuxiliarySidebarFrame({ mode, children, id, sidebarLayout, isMobile = true }: { mode: WorkbenchPanelMode; children: ReactNode; id?: string; sidebarLayout?: SidebarLayout; isMobile?: boolean }) {
  const { auxiliaryWidth: width } = useSidebarWidths(sidebarLayout);
  if (mode === "hidden") return null;

  const style = mode === "docked" || !isMobile ? { width } : undefined;
  const className =
    mode === "docked"
      ? "min-h-0 flex shrink-0"
      : isMobile
        ? "fixed inset-x-0 bottom-[calc(3.5rem+env(safe-area-inset-bottom))] z-40 flex h-[70dvh] min-h-0 max-w-none rounded-t-2xl shadow-2xl"
        : "fixed bottom-[var(--ng-workbench-status-size)] right-0 top-[var(--ng-workbench-header-size)] z-40 flex min-h-0 max-w-[calc(100vw-44px)] shadow-2xl";

  return (
    <div id={id} style={style} className={className}>
      {children}
    </div>
  );
}

export function PanelOverlay({ visible, onClose, isMobile = true }: { visible: boolean; onClose: () => void; isMobile?: boolean }) {
  if (!visible) return null;
  return (
    <button
      type="button"
      aria-label="Close panel"
      onClick={onClose}
      className={`fixed z-30 bg-black/40 ${isMobile
        ? "inset-x-0 bottom-[calc(3.5rem+env(safe-area-inset-bottom))] top-[calc(3rem+env(safe-area-inset-top))]"
        : "bottom-[var(--ng-workbench-status-size)] left-11 right-0 top-[var(--ng-workbench-header-size)]"}`}
    />
  );
}
