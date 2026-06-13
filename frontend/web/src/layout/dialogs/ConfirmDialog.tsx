import { Button, Modal } from "../../shared/ui";
import type { AppDialog } from "./dialogTypes";

export function ConfirmDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "confirm" }>; onClose: () => void }) {
  function confirm() {
    dialog.onConfirm();
    onClose();
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={
        <>
          <Button secondary onClick={onClose}>Cancel</Button>
          <Button variant={dialog.danger ? "danger" : "primary"} onClick={confirm}>
            {dialog.confirmLabel ?? "Confirm"}
          </Button>
        </>
      }
    >
      <p className="text-sm leading-6 text-muted">{dialog.message}</p>
    </Modal>
  );
}
