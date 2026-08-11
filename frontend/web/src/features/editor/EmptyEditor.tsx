import { FileText, Mic } from "lucide-react";

import type { Space } from "../../api/types";
import { Button, Card } from "../../shared/ui";

export function EmptyEditor({ activeSpace, canWriteActiveSpace, onCreateFolder, onCreateText, onRecordAudio, onFileSelected }: { activeSpace: Space | null; canWriteActiveSpace: boolean; onCreateFolder: () => void; onCreateText: () => void; onRecordAudio: () => void; onFileSelected: (file: File | null) => void }) {
  return (
    <section className="grid min-w-0 flex-1 place-items-center bg-bg px-4 text-muted">
      <div className="w-full max-w-[24rem] text-center">
        <Card className="mx-auto mb-5 grid size-12 place-items-center rounded-2xl p-0"><FileText size={24} /></Card>
        <div className="text-lg font-semibold text-text">Choose something from Files</div>
        <p className="mx-auto mt-2 max-w-[20rem] text-sm leading-6">Select an item from Files or Recent{canWriteActiveSpace ? ". Create a first item when this space is empty." : "."}</p>
        {activeSpace && canWriteActiveSpace ? (
          <div className="mx-auto mt-6 grid max-w-[22rem] grid-cols-2 gap-2">
            <Button onClick={onCreateText}>New document</Button>
            <Button secondary onClick={onCreateFolder}>New folder</Button>
            <label className="inline-flex cursor-pointer items-center justify-center rounded-lg border border-border bg-surface px-3 py-2 text-sm font-semibold text-muted transition hover:bg-panel hover:text-text">
              Upload file
              <input className="hidden" type="file" onChange={(event) => onFileSelected(event.target.files?.[0] ?? null)} />
            </label>
            <Button secondary onClick={onRecordAudio}><Mic size={15} /> Record audio</Button>
          </div>
        ) : null}
      </div>
    </section>
  );
}
