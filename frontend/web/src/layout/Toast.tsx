import { useEffect } from "react";

import { useUiStore } from "../stores/uiStore";

export function Toast() {
  const toast = useUiStore((state) => state.toast);
  const clearToast = useUiStore((state) => state.clearToast);
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(clearToast, 2000);
    return () => window.clearTimeout(timer);
  }, [toast, clearToast]);
  if (!toast) return null;
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-20 z-50 flex justify-center md:bottom-10">
      <div className="rounded-full border border-border bg-panel-strong px-4 py-2 text-sm text-text shadow-lg">{toast}</div>
    </div>
  );
}
