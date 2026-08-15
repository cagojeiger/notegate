import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

export function SidebarSectionHeader({ label, open, onToggle, action }: { label: string; open: boolean; onToggle: () => void; action: { label: string; icon: ReactNode; onClick: () => void } }) {
  return (
    <div>
      <div className="flex items-center justify-between gap-1">
        <button onClick={onToggle} className="flex min-h-workbench-control min-w-0 items-center gap-1 font-ui text-workbench font-medium text-muted hover:text-text md:min-h-6">
          <ChevronRight size={12} className={`shrink-0 ${open ? "rotate-90 transition" : "transition"}`} />
          <span className="truncate">{label}</span>
        </button>
        <button onClick={action.onClick} aria-label={action.label} title={action.label} className="grid size-workbench-control shrink-0 place-items-center rounded-workbench text-muted hover:bg-surface hover:text-text md:size-6">
          {action.icon}
        </button>
      </div>
    </div>
  );
}
