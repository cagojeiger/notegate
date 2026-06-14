import { FileText } from "lucide-react";

import type { Space } from "../../api/types";
import { Button, Card } from "../../shared/ui";

export function EmptyEditor({ activeSpace, onCreateFolder, onCreateText, onFileSelected }: { activeSpace: Space | null; onCreateFolder: () => void; onCreateText: () => void; onFileSelected: (file: File | null) => void }) {
  return (
    <section className="grid min-w-0 flex-1 place-items-center bg-bg px-4 text-muted">
      <div className="w-full max-w-[24rem] text-center">
        <Card className="mx-auto mb-5 grid size-12 place-items-center rounded-2xl p-0"><FileText size={24} /></Card>
        <div className="text-lg font-semibold text-text">Open a node</div>
        <p className="mx-auto mt-2 max-w-[20rem] text-sm leading-6">Select an item from Files or Recent. Create a first item when this space is empty.</p>
        {activeSpace ? (
          <div className="mx-auto mt-6 flex max-w-[22rem] flex-wrap justify-center gap-2">
            <Button className="min-w-24" onClick={onCreateText}>New text</Button>
            <Button className="min-w-24" secondary onClick={onCreateFolder}>New folder</Button>
            <label className="inline-flex min-w-24 cursor-pointer items-center justify-center rounded-lg border border-border bg-surface px-3 py-2 text-sm font-semibold text-muted transition hover:bg-panel hover:text-text">
              Upload file
              <input className="hidden" type="file" onChange={(event) => onFileSelected(event.target.files?.[0] ?? null)} />
            </label>
          </div>
        ) : null}
      </div>
    </section>
  );
}
