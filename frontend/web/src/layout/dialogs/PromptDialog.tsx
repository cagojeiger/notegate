import { useState } from "react";

import { Button, Modal, TextField } from "../../shared/ui";
import { dialogErrorMessage } from "./dialogErrors";
import type { AppDialog } from "./dialogTypes";

export function PromptDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "prompt" }>; onClose: () => void }) {
  const [value, setValue] = useState(dialog.initial);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const trimmed = value.trim();
  async function submit() {
    if (!trimmed || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await dialog.onSubmit(trimmed);
      onClose();
    } catch (submitError) {
      setError(dialogErrorMessage(submitError));
    } finally {
      setSubmitting(false);
    }
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={() => void submit()} disabled={!trimmed || submitting}>{dialog.submitLabel ?? "Save"}</Button></>}
    >
      <TextField
        autoFocus
        label={dialog.label}
        value={value}
        placeholder={dialog.placeholder}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); void submit(); } }}
      />
      {error ? <p className="mt-2 text-xs text-danger">{error}</p> : null}
    </Modal>
  );
}
