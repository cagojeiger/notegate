import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

export function SidebarSectionHeader({ label, open, onToggle, trailing }: { label: string; open: boolean; onToggle: () => void; trailing: ReactNode }) {
  return (
    <div>
      <div className="flex items-center justify-between gap-1">
        <button onClick={onToggle} className="flex min-h-workbench-control min-w-0 items-center gap-1 font-ui text-workbench font-medium text-muted hover:text-text md:min-h-6">
          <ChevronRight size={12} className={`shrink-0 ${open ? "rotate-90 transition" : "transition"}`} />
          <span className="truncate">{label}</span>
        </button>
        {trailing}
      </div>
    </div>
  );
}
