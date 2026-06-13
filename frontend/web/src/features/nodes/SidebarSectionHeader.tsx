import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

export function SidebarSectionHeader({ icon, label, open, onToggle, action }: { icon: ReactNode; label: string; open: boolean; onToggle: () => void; action: { label: string; icon: ReactNode; onClick: () => void } }) {
  return (
    <div className="flex items-center justify-between gap-1">
      <button onClick={onToggle} className="flex min-w-0 items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted hover:text-text">
        <ChevronRight size={12} className={`shrink-0 ${open ? "rotate-90 transition" : "transition"}`} />
        <span className="shrink-0">{icon}</span>
        <span className="truncate">{label}</span>
      </button>
      <button onClick={action.onClick} aria-label={action.label} title={action.label} className="grid size-5 shrink-0 place-items-center rounded text-muted hover:bg-surface hover:text-text">
        {action.icon}
      </button>
    </div>
  );
}
