import { X } from "lucide-react";
import type { ReactNode } from "react";

import { IconButton } from "../../shared/ui";

export function EditorGroupHeader({ title, icon, actions, canClose, onClose, dirty }: { title: string; icon?: ReactNode; actions?: ReactNode; canClose: boolean; onClose: () => void; dirty?: boolean }) {
  return (
    <div className="flex h-12 items-center justify-between border-b border-seam px-4">
      <div className="flex min-w-0 items-center gap-2 font-semibold">{icon}<span className="truncate">{title}</span>{dirty ? <span className="size-1.5 shrink-0 rounded-full bg-warning" title="Unsaved changes" /> : null}</div>
      <div className="flex items-center gap-1">
        {actions}
        {canClose ? <IconButton label="Close editor group" onClick={onClose}><X size={16} /></IconButton> : null}
      </div>
    </div>
  );
}
