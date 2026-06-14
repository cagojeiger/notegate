import { useState } from "react";

import { Button, Modal } from "../../shared/ui";
import { dialogErrorMessage } from "./dialogErrors";
import type { AppDialog } from "./dialogTypes";

export function ConfirmDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "confirm" }>; onClose: () => void }) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    if (submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await dialog.onConfirm();
      onClose();
    } catch (confirmError) {
      setError(dialogErrorMessage(confirmError));
    } finally {
      setSubmitting(false);
    }
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={
        <>
          <Button secondary onClick={onClose}>Cancel</Button>
          <Button variant={dialog.danger ? "danger" : "primary"} onClick={() => void confirm()} disabled={submitting}>
            {dialog.confirmLabel ?? "Confirm"}
          </Button>
        </>
      }
    >
      <p className="text-sm leading-6 text-muted">{dialog.message}</p>
      {error ? <p className="mt-2 text-xs text-danger">{error}</p> : null}
    </Modal>
  );
}
