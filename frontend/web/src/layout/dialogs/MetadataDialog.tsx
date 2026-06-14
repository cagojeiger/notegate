import { useMemo, useState } from "react";

import { Button, Modal, TextAreaField } from "../../shared/ui";
import { dialogErrorMessage } from "./dialogErrors";
import type { AppDialog } from "./dialogTypes";

export function MetadataDialog({ dialog, onClose }: { dialog: Extract<AppDialog, { kind: "metadata" }>; onClose: () => void }) {
  const [text, setText] = useState(() => JSON.stringify(dialog.node.metadata ?? {}, null, 2));
  const [submitting, setSubmitting] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const parsed = useMemo<{ ok: true; value: Record<string, unknown> } | { ok: false; error: string }>(() => {
    try {
      const value = JSON.parse(text);
      if (typeof value !== "object" || value === null || Array.isArray(value)) return { ok: false, error: "Metadata must be a JSON object" };
      return { ok: true, value: value as Record<string, unknown> };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : "Invalid JSON" };
    }
  }, [text]);

  async function save() {
    if (!parsed.ok || submitting) return;
    setSubmitting(true);
    setSaveError(null);
    try {
      await dialog.onSave(parsed.value);
      onClose();
    } catch (error) {
      setSaveError(dialogErrorMessage(error));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal
      title="Edit metadata"
      onClose={onClose}
      width="max-w-lg"
      footer={<><Button secondary onClick={onClose}>Cancel</Button><Button onClick={() => void save()} disabled={!parsed.ok || submitting}>Save</Button></>}
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
      {saveError ? <p className="mt-2 text-xs text-danger">{saveError}</p> : null}
    </Modal>
  );
}
