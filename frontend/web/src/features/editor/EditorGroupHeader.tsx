import { X } from "lucide-react";
import type { ReactNode } from "react";

import { IconButton } from "../../shared/ui";

export function EditorGroupHeader({ title, icon, titleActions, actions, canClose, onClose, dirty, active }: { title: string; icon?: ReactNode; titleActions?: ReactNode; actions?: ReactNode; canClose: boolean; onClose: () => void; dirty?: boolean; active?: boolean }) {
  return (
    <div className={`flex h-10 items-center justify-between border-b px-4 ${active ? "border-[var(--ng-active-border)] bg-[var(--ng-active-surface)]" : "border-seam"}`}>
      <div className="flex min-w-0 items-center gap-2 text-sm font-semibold">{icon}<span className="truncate">{title}</span>{dirty ? <span className="size-1.5 shrink-0 rounded-full bg-warning" title="Unsaved changes" /> : null}{titleActions ? <div className="ml-1 flex shrink-0 items-center gap-1">{titleActions}</div> : null}</div>
      <div className="flex items-center gap-1">
        {actions}
        {canClose ? <IconButton label="Close editor group" onClick={onClose} size="sm"><X size={15} /></IconButton> : null}
      </div>
    </div>
  );
}
