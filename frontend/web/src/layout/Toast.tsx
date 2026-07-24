import { useEffect } from "react";

export function Toast({ message, onClear }: { message: string | null; onClear: () => void }) {
  useEffect(() => {
    if (!message) return;
    const timer = window.setTimeout(onClear, 2000);
    return () => window.clearTimeout(timer);
  }, [message, onClear]);
  if (!message) return null;
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-20 z-50 flex justify-center md:bottom-10">
      <div className="rounded-full border border-border bg-panel-strong px-4 py-2 text-sm text-text shadow-lg">{message}</div>
    </div>
  );
}
