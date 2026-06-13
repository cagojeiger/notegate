import { useState } from "react";

import { Button, Modal, TextField } from "../../shared/ui";
import type { AppDialog } from "./dialogTypes";

export function PromptDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "prompt" }>; onClose: () => void }) {
  const [value, setValue] = useState(dialog.initial);
  const trimmed = value.trim();
  function submit() {
    if (!trimmed) return;
    dialog.onSubmit(trimmed);
    onClose();
  }
  return (
    <Modal
      title={dialog.title}
      onClose={onClose}
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={submit} disabled={!trimmed}>{dialog.submitLabel ?? "Save"}</Button></>}
    >
      <TextField
        autoFocus
        label={dialog.label}
        value={value}
        placeholder={dialog.placeholder}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => { if (event.key === "Enter") submit(); }}
      />
    </Modal>
  );
}
