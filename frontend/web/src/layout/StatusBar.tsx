import type { Space } from "../api/types";

export function StatusBar({ activeSpace }: { activeSpace: Space | null }) {
  return (
    <footer className="flex h-7 items-center justify-between border-t border-border bg-surface px-3 text-xs text-muted">
      <span className="flex items-center gap-2"><span className="size-2 rounded-full bg-success" /> ready</span>
      <span>{activeSpace?.name ?? "No space"}</span>
    </footer>
  );
}
