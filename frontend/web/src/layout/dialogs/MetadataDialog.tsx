import { useMemo, useState } from "react";

import { Button, Modal, TextAreaField } from "../../shared/ui";
import type { AppDialog } from "./dialogTypes";

export function MetadataDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "metadata" }>; onClose: () => void }) {
  const [text, setText] = useState(() => JSON.stringify(dialog.node.metadata ?? {}, null, 2));
  const parsed = useMemo<{ ok: true; value: Record<string, unknown> } | { ok: false; error: string }>(() => {
    try {
      const value = JSON.parse(text);
      if (typeof value !== "object" || value === null || Array.isArray(value)) return { ok: false, error: "Metadata must be a JSON object" };
      return { ok: true, value: value as Record<string, unknown> };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : "Invalid JSON" };
    }
  }, [text]);
  return (
    <Modal
      title="Edit metadata"
      onClose={onClose}
      width="max-w-lg"
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={() => { if (parsed.ok) { dialog.onSave(parsed.value); onClose(); } }} disabled={!parsed.ok}>Save</Button></>}
    >
      <TextAreaField
        autoFocus
        label="Metadata JSON"
        value={text}
        onChange={(event) => setText(event.target.value)}
        spellCheck={false}
        rows={10}
        textareaClassName="font-mono text-xs"
      />
      <p className={`mt-2 text-xs ${parsed.ok ? "text-faint" : "text-danger"}`}>{parsed.ok ? "Valid JSON object. Metadata is stored unencrypted." : parsed.error}</p>
    </Modal>
  );
}
