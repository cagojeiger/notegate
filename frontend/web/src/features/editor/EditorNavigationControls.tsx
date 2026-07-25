import { ArrowLeft, ArrowRight } from "lucide-react";

import { IconButton } from "../../shared/ui";
import type { EditorGroup, EditorNavigationDirection } from "../../stores/uiStoreReducers";

type EditorNavigationControlsProps = {
  group: EditorGroup;
  pending: boolean;
  onNavigate: (groupId: number, direction: EditorNavigationDirection) => void;
};

export function EditorNavigationControls({ group, pending, onNavigate }: EditorNavigationControlsProps) {
  const back = group.back[group.back.length - 1];
  const forward = group.forward[group.forward.length - 1];

  return (
    <>
      <IconButton
        label={back ? `Back to ${back.nameSnapshot}` : "Back"}
        size="sm"
        disabled={!back || pending}
        onClick={() => onNavigate(group.id, "back")}
      >
        <ArrowLeft size={15} />
      </IconButton>
      <IconButton
        label={forward ? `Forward to ${forward.nameSnapshot}` : "Forward"}
        size="sm"
        disabled={!forward || pending}
        onClick={() => onNavigate(group.id, "forward")}
      >
        <ArrowRight size={15} />
      </IconButton>
    </>
  );
}
