import { createContext, useEffect, useLayoutEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

export const ContextMenuItems = createContext(false);

const ITEM_SELECTOR = '[role="menuitem"]:not(:disabled)';

export function ContextMenu({ x, y, width = 208, label, onClose, children }: {
  x: number;
  y: number;
  width?: number;
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8))}px`;
  }, [x, y, width]);

  useEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const trigger = previousFocus.current;
    (menu.querySelector<HTMLElement>(ITEM_SELECTOR) ?? menu).focus();
    const dismiss = () => onCloseRef.current();
    window.addEventListener("resize", dismiss);
    return () => {
      window.removeEventListener("resize", dismiss);
      window.setTimeout(() => {
        if (!menu.isConnected && trigger?.isConnected && document.activeElement === document.body) {
          trigger.focus();
        }
      }, 0);
    };
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" || event.key === "Tab") {
      if (event.key === "Escape") event.preventDefault();
      event.stopPropagation();
      previousFocus.current?.focus();
      onCloseRef.current();
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(ITEM_SELECTOR));
    if (!items.length) return;
    const current = items.indexOf(document.activeElement as HTMLElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1
      : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next].focus();
  }

  return (
    <>
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} />
      <div
        ref={menuRef}
        role="menu"
        aria-label={label}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className="fixed z-50 max-h-[calc(100dvh-16px)] max-w-[calc(100vw-16px)] overflow-y-auto rounded-workbench-surface border border-border bg-surface p-1 font-ui text-workbench shadow-[var(--ng-focus-shadow)]"
        style={{ left: x, top: y, width }}
      >
        <ContextMenuItems.Provider value>{children}</ContextMenuItems.Provider>
      </div>
    </>
  );
}
