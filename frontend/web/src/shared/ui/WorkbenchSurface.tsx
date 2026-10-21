import { useCallback, useLayoutEffect, useRef, useState, type ReactNode, type Ref } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";

import { WORKBENCH_LAYOUT } from "../model/workbenchLayout";
import { IconButton } from "./IconButton";
import { Modal } from "./Modal";
import { PanelHeader } from "./PanelHeader";
import { Tabs, type TabItem } from "./Tabs";

type WorkbenchSurfaceProps<T extends string> = {
  title: string;
  tabs: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  navigationLabel: string;
  headerActions?: ReactNode;
  bodyRef?: Ref<HTMLDivElement>;
  bodyClassName?: string;
  children: ReactNode;
  isMobile: boolean;
  inspector?: {
    title: string;
    label?: string;
    closeLabel: string;
    open: boolean;
    onClose: () => void;
    // Settings portals need their desktop host even while the panel is hidden.
    keepMounted?: boolean;
    children: ReactNode;
  };
};

// Features own navigation and operations; this shell only owns their presentation.
export function WorkbenchSurface<T extends string>({ title, tabs, value, onChange, navigationLabel,
  headerActions, bodyRef, bodyClassName = "", children, isMobile, inspector
}: WorkbenchSurfaceProps<T>) {
  const current = tabs.find((tab) => tab.id === value);
  return <div data-workbench-surface className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
    <section aria-label={title} className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <h1 className="sr-only">{title}</h1>
      <PanelHeader tabs>
        <WorkbenchTabs items={tabs} value={value} onChange={onChange} label={navigationLabel} />
        {headerActions ? <div className="flex h-full shrink-0 items-center">{headerActions}</div> : null}
      </PanelHeader>
      <div key={value} ref={bodyRef} id={current?.controls} role="tabpanel"
        aria-labelledby={current?.controls ? `${current.controls}-tab` : undefined}
        className={`min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain p-4 md:p-5 ${bodyClassName}`}>
        {children}
      </div>
    </section>
    {inspector && !isMobile && (inspector.open || inspector.keepMounted) ? (
      <aside aria-label={inspector.label ?? inspector.title}
        className={inspector.open ? "flex min-h-0 shrink-0 flex-col border-l border-seam bg-surface" : "hidden"}
        style={{ width: WORKBENCH_LAYOUT.defaultAuxiliaryWidth }}>
        <PanelHeader>
          <h2 className="font-medium">{inspector.title}</h2>
          <IconButton label={inspector.closeLabel} onClick={(event) => {
            event.currentTarget.closest("[data-workbench-surface]")
              ?.querySelector<HTMLButtonElement>('[role="tablist"] [aria-selected="true"]')?.focus();
            inspector.onClose();
          }}><X size={16} /></IconButton>
        </PanelHeader>
        {inspector.children}
      </aside>
    ) : inspector?.open && isMobile ? (
      <Modal title={inspector.title} placement="bottom" width="max-w-none" onClose={inspector.onClose}>{inspector.children}</Modal>
    ) : null}
  </div>;
}

function WorkbenchTabs<T extends string>({ items, value, onChange, label }: {
  items: TabItem<T>[]; value: T; onChange: (value: T) => void; label: string;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ overflow: false, before: false, after: false });
  const revealSelected = useCallback(() => {
    const list = frame.current?.querySelector<HTMLDivElement>('[role="tablist"]');
    const active = list?.querySelector<HTMLButtonElement>('[aria-selected="true"]');
    if (!list || !active) return;
    const visible = list.getBoundingClientRect();
    const selected = active.getBoundingClientRect();
    if (selected.left < visible.left) list.scrollLeft -= Math.ceil(visible.left - selected.left);
    else if (selected.right > visible.right) list.scrollLeft += Math.ceil(selected.right - visible.right);
  }, []);
  useLayoutEffect(() => {
    const outer = frame.current!;
    const list = outer.querySelector<HTMLDivElement>('[role="tablist"]')!;
    const update = () => {
      const next = { overflow: list.scrollWidth > outer.clientWidth + 1, before: list.scrollLeft > 1,
        after: list.scrollWidth - list.clientWidth - list.scrollLeft > 1 };
      setEdges((current) => current.overflow === next.overflow && current.before === next.before && current.after === next.after ? current : next);
    };
    update();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => { revealSelected(); update(); });
    observer?.observe(outer);
    observer?.observe(list);
    list.addEventListener("scroll", update);
    return () => { observer?.disconnect(); list.removeEventListener("scroll", update); };
  }, [items, revealSelected]);
  useLayoutEffect(() => { revealSelected(); }, [value, edges.overflow, revealSelected]);
  const scroll = (direction: number) => {
    const list = frame.current?.querySelector<HTMLDivElement>('[role="tablist"]');
    if (list) list.scrollLeft += direction * list.clientWidth * 0.75;
  };
  return <div ref={frame} className="flex h-full min-w-0 flex-1 items-center">
    {edges.overflow ? <IconButton label={`Previous ${label.toLowerCase()}`} disabled={!edges.before} onClick={() => scroll(-1)}><ChevronLeft size={16} /></IconButton> : null}
    <div className="h-full min-w-0 flex-1"><Tabs items={items} value={value} onChange={onChange} label={label} variant="header" /></div>
    {edges.overflow ? <IconButton label={`More ${label.toLowerCase()}`} disabled={!edges.after} onClick={() => scroll(1)}><ChevronRight size={16} /></IconButton> : null}
  </div>;
}
