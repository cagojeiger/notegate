import { FileText } from "lucide-react";

import type { Space } from "../../api/types";
import { Button, Card } from "../../shared/ui";

export function EmptyEditor({ activeSpace, onCreateFolder, onCreateText, onFileSelected }: { activeSpace: Space | null; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  return (
    <section className="grid min-w-0 flex-1 place-items-center bg-bg px-6 text-muted">
      <div className="max-w-md text-center">
        <Card className="mx-auto mb-5 grid size-14 place-items-center rounded-2xl p-0"><FileText size={26} /></Card>
        <div className="text-xl font-semibold text-text">Open a node</div>
        <p className="mt-2 text-sm leading-6">Select an item from Tree or Recent. Create a first item when this space is empty.</p>
        {activeSpace ? (
          <div className="mt-6 flex justify-center gap-2">
            <Button onClick={onCreateText}>New text</Button>
            <Button secondary onClick={onCreateFolder}>New folder</Button>
            <label className="inline-flex cursor-pointer items-center rounded-lg border border-border bg-surface px-3 py-2 text-sm font-semibold text-muted transition hover:bg-panel hover:text-text">
              Upload file
              <input className="hidden" type="file" onChange={(event) => onFileSelected(event.target.files?.[0] ?? null)} />
            </label>
          </div>
        ) : null}
      </div>
    </section>
  );
}
