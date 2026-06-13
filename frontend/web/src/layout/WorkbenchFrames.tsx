import type { PointerEventHandler, ReactNode } from "react";

export function PrimarySidebarFrame({ isMobile, open, mobileOpen, width, children }: { isMobile: boolean; open: boolean; mobileOpen: boolean; width: number; children: ReactNode }) {
  return (
    <div
      style={isMobile ? undefined : { width }}
      className={`min-h-0 max-md:fixed max-md:left-0 max-md:bottom-0 max-md:top-12 max-md:z-40 max-md:flex max-md:w-[85%] max-md:max-w-[320px] max-md:shadow-2xl max-md:transition-transform ${mobileOpen ? "max-md:translate-x-0" : "max-md:-translate-x-full"} ${open ? "md:flex md:shrink-0" : "md:hidden"}`}
    >
      {children}
    </div>
  );
}

export function PrimarySidebarResizeHandle({ visible, onPointerDown }: { visible: boolean; onPointerDown: PointerEventHandler<HTMLDivElement> }) {
  if (!visible) return null;
  return <div onPointerDown={onPointerDown} className="hidden w-1 shrink-0 cursor-col-resize bg-seam transition-colors hover:bg-[var(--ng-selection)] md:block" aria-hidden="true" />;
}

export function AuxiliarySidebarFrame({ open, mobileOpen, children }: { open: boolean; mobileOpen: boolean; children: ReactNode }) {
  return (
    <div
      className={`min-h-0 hidden max-md:fixed max-md:inset-x-0 max-md:bottom-0 max-md:top-auto max-md:z-40 max-md:flex max-md:h-[70vh] max-md:max-w-none max-md:rounded-t-2xl max-md:shadow-2xl max-md:transition-transform ${mobileOpen ? "max-md:translate-y-0" : "max-md:translate-y-full"} md:max-[1120px]:fixed md:max-[1120px]:right-0 md:max-[1120px]:top-12 md:max-[1120px]:bottom-7 md:max-[1120px]:z-30 md:max-[1120px]:w-[340px] md:max-[1120px]:shadow-2xl ${open ? "md:max-[1120px]:flex min-[1120px]:flex min-[1120px]:w-[320px] min-[1120px]:shrink-0" : "md:max-[1120px]:hidden min-[1120px]:hidden"}`}
    >
      {children}
    </div>
  );
}

export function MobilePanelOverlay({ visible, onClose }: { visible: boolean; onClose: () => void }) {
  if (!visible) return null;
  return <button type="button" aria-label="Close panel" onClick={onClose} className="fixed inset-x-0 bottom-0 top-12 z-30 bg-black/40 md:hidden" />;
}
