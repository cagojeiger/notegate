import { useEffect, useId, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

import { IconButton } from "./IconButton";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  '[tabindex]:not([tabindex="-1"])'
].join(",");

// Shared modal shell for focused dialogs and mobile Inspector sheets.
// Escape and backdrop click both dismiss. Keep panels small and focused.
export function Modal({
  title,
  onClose,
  children,
  footer,
  width = "max-w-md",
  fixedHeight = false,
  placement = "center"
}: {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
  fixedHeight?: boolean;
  placement?: "center" | "bottom";
}) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const previousFocusRef = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const bottomSheet = placement === "bottom";
  const centeredLayout = fixedHeight
    ? "flex h-[min(48rem,calc(100dvh-2rem))] flex-col overflow-hidden"
    : "max-h-[calc(100vh-2rem)] overflow-y-auto";

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const currentDialog = dialogRef.current;
    if (!currentDialog) return;
    const dialog: HTMLDivElement = currentDialog;
    const previousFocus = previousFocusRef.current;

    function focusableElements(): HTMLElement[] {
      return Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
        .filter((element) => element.tabIndex >= 0 && !element.matches(":disabled"));
    }

    function onKey(event: KeyboardEvent) {
      const dialogs = document.querySelectorAll('[role="dialog"][aria-modal="true"]');
      if (dialogs[dialogs.length - 1] !== dialog) return;

      if (event.key === "Escape") {
        event.stopPropagation();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = focusableElements();
      if (focusable.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialog.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    }

    if (!dialog.contains(document.activeElement)) {
      (focusableElements()[0] ?? dialog).focus();
    }
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.setTimeout(() => {
        const active = document.activeElement;
        if (!dialog.isConnected && previousFocus?.isConnected &&
          (!active || active === document.body || dialog.contains(active))) previousFocus.focus();
      }, 0);
    };
  }, []);

  return (
    <div className={`fixed inset-0 z-50 flex ${bottomSheet ? "items-end" : "items-center justify-center p-4"}`}>
      <button type="button" aria-hidden="true" tabIndex={-1} className="absolute inset-0 bg-black/40" onClick={onClose} />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={`relative w-full ${width} border border-border bg-panel font-ui text-workbench shadow-[var(--ng-focus-shadow)] ${
          bottomSheet
            ? "flex h-[82dvh] flex-col overflow-hidden rounded-t-2xl border-b-0"
            : `${centeredLayout} rounded-lg p-6`
        }`}
      >
        <div className={`flex shrink-0 items-center justify-between ${bottomSheet ? "h-12 border-b border-seam px-3" : "mb-4"}`}>
          <h2 id={titleId} className={bottomSheet ? "text-sm font-medium" : "text-base font-semibold"}>{title}</h2>
          <IconButton label="Close" onClick={onClose}><X size={16} /></IconButton>
        </div>
        {bottomSheet ? (
          <div className="min-h-0 flex-1 overflow-hidden pb-[env(safe-area-inset-bottom)]">
            {children}
          </div>
        ) : children}
        {footer ? <div className="mt-5 flex shrink-0 justify-end gap-2">{footer}</div> : null}
      </div>
    </div>
  );
}
