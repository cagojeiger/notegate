import { ConfirmDialog } from "./ConfirmDialog";
import type { AppDialog } from "./dialogTypes";
import { MetadataDialog } from "./MetadataDialog";
import { MoveDialog } from "./MoveDialog";
import { PromptDialog } from "./PromptDialog";

export type { AppDialog } from "./dialogTypes";

export function DialogHost({ dialog, onClose }: { dialog: AppDialog | null; onClose: () => void }) {
  if (!dialog) return null;
  if (dialog.kind === "prompt") return <PromptDialog dialog={dialog} onClose={onClose} />;
  if (dialog.kind === "confirm") return <ConfirmDialog dialog={dialog} onClose={onClose} />;
  if (dialog.kind === "move") return <MoveDialog dialog={dialog} onClose={onClose} />;
  return <MetadataDialog dialog={dialog} onClose={onClose} />;
}
