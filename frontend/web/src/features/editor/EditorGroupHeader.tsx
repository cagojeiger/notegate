import { Link2, X } from "lucide-react";
import type { MouseEventHandler, ReactNode } from "react";

import { copyText } from "../../shared/lib/clipboard";
import { IconButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

export function EditorGroupHeader({ title, icon, navigationActions, qualifiedPath, titleActions, actions, canClose, onClose, onContextMenu, dirty, active }: { title: string; icon?: ReactNode; navigationActions?: ReactNode; qualifiedPath?: string | null; titleActions?: ReactNode; actions?: ReactNode; canClose: boolean; onClose: () => void; onContextMenu?: MouseEventHandler<HTMLDivElement>; dirty?: boolean; active?: boolean }) {
  const showToast = useUiStore((state) => state.showToast);

  async function copyPath() {
    if (!qualifiedPath) return;
    showToast((await copyText(qualifiedPath)) ? "Path copied" : "Could not copy path");
  }

  return (
    <div onContextMenu={onContextMenu} className={`flex h-12 items-center justify-between border-b px-4 ${active ? "border-[var(--ng-active-border)] bg-[var(--ng-active-surface)]" : "border-seam"}`}>
      <div className="flex min-w-0 items-center gap-2 text-sm font-semibold">
        {navigationActions ? <div className="flex shrink-0 items-center gap-1">{navigationActions}</div> : null}
        {icon}
        <span className="min-w-0 truncate">{title}</span>
        {dirty ? <span className="size-1.5 shrink-0 rounded-full bg-warning" title="Unsaved changes" /> : null}
        {qualifiedPath || titleActions ? (
          <div className={`flex shrink-0 items-center gap-1 ${qualifiedPath ? "" : "ml-1"}`}>
            {qualifiedPath ? (
              <span title="Copy path">
                <IconButton label="Copy path" size="sm" onClick={() => { void copyPath(); }}>
                  <Link2 size={14} />
                </IconButton>
              </span>
            ) : null}
            {titleActions}
          </div>
        ) : null}
      </div>
      <div className="flex items-center gap-1">
        {actions}
        {canClose ? <IconButton label="Close editor group" onClick={onClose} size="sm"><X size={15} /></IconButton> : null}
      </div>
    </div>
  );
}
