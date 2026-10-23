import { useEffect, useId, useRef } from "react";

import { Button, Card } from "../../shared/ui";

export function AccessConfirmation({ title, description, confirmLabel, pending, error, onConfirm, onCancel }: {
  title: string;
  description: string;
  confirmLabel: string;
  pending: boolean;
  error?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const trigger = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);

  useEffect(() => { panel.current?.focus(); }, []);

  function cancel() {
    onCancel();
    if (trigger.current?.isConnected) trigger.current.focus();
  }

  return (
    <div ref={panel} tabIndex={-1} role="group" aria-labelledby={titleId} aria-describedby={descriptionId} className="mt-3 outline-none">
      <Card tone="danger" padding="sm" className="space-y-3 text-sm">
        <p id={titleId} className="font-semibold">{title}</p>
        <p id={descriptionId} className="text-muted">{description}</p>
        {error ? <p role="alert" className="text-text">{error}</p> : null}
        <div className="flex flex-wrap gap-2">
          <Button secondary disabled={pending} aria-busy={pending} onClick={onConfirm}>{pending ? "Working…" : confirmLabel}</Button>
          <Button secondary disabled={pending} onClick={cancel}>Cancel</Button>
        </div>
      </Card>
    </div>
  );
}
