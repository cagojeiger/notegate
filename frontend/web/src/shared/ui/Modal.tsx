import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";

import { IconButton } from "./IconButton";

// Shared modal shell matching the SettingsModal tone: soft backdrop + paper panel.
// Escape and backdrop click both dismiss. Keep panels small and focused.
export function Modal({ title, onClose, children, footer, width = "max-w-md" }: { title: string; onClose: () => void; children: ReactNode; footer?: ReactNode; width?: string }) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <button type="button" aria-label="Close" className="absolute inset-0 bg-black/40" onClick={onClose} />
      <div className={`relative w-full ${width} rounded-2xl border border-border bg-panel p-6 shadow-[var(--ng-focus-shadow)]`}>
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold">{title}</h2>
          <IconButton label="Close" onClick={onClose}><X size={16} /></IconButton>
        </div>
        {children}
        {footer ? <div className="mt-5 flex justify-end gap-2">{footer}</div> : null}
      </div>
    </div>
  );
}
