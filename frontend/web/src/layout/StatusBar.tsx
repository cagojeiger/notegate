import type { Space } from "../entities/space/model";
import type { SaveState } from "../shared/model/workbench";

const SAVE_LABEL: Record<string, { text: string; dot: string }> = {
  idle: { text: "ready", dot: "bg-success" },
  saving: { text: "saving…", dot: "bg-warning" },
  saved: { text: "saved", dot: "bg-success" },
  error: { text: "save failed", dot: "bg-danger" },
  conflict: { text: "conflict", dot: "bg-warning" }
};

export function StatusBar({
  activeSpace,
  saveState
}: {
  activeSpace: Space | null;
  saveState: SaveState;
}) {
  const status = SAVE_LABEL[saveState] ?? SAVE_LABEL.idle;
  return (
    <footer className="hidden h-7 items-center justify-between border-t border-seam bg-surface px-3 text-xs text-muted md:flex">
      <span className="flex items-center gap-2"><span className={`size-2 rounded-full ${status.dot}`} /> {status.text}</span>
      <span>{activeSpace?.name ?? "No space"}</span>
    </footer>
  );
}
