import { FileText, Mic } from "lucide-react";
import { useRef } from "react";

import type { Space } from "../../api/types";
import { Button, Card } from "../../shared/ui";

export function EmptyEditor({ activeSpace, canWriteActiveSpace, onCreateFolder, onCreateText, onRecordAudio, onFileSelected }: { activeSpace: Space | null; canWriteActiveSpace: boolean; onCreateFolder: () => void; onCreateText: () => void; onRecordAudio: () => void; onFileSelected: (file: File | null) => void }) {
  const fileInput = useRef<HTMLInputElement>(null);
  return (
    <section className="grid min-w-0 flex-1 place-items-center bg-bg px-4 text-muted">
      <div className="w-full max-w-[24rem] text-center">
        <Card padding="none" className="mx-auto mb-5 grid size-12 place-items-center"><FileText size={24} /></Card>
        <div className="text-lg font-semibold text-text">Choose something from Files</div>
        <p className="mx-auto mt-2 max-w-[20rem] text-sm leading-6">Select an item from Files or Recent{canWriteActiveSpace ? ". Create a first item when this space is empty." : "."}</p>
        {activeSpace && canWriteActiveSpace ? (
          <div className="mx-auto mt-6 grid max-w-[22rem] grid-cols-2 gap-2">
            <Button onClick={onCreateText}>New document</Button>
            <Button secondary onClick={onCreateFolder}>New folder</Button>
            <Button secondary onClick={() => fileInput.current?.click()}>Upload file</Button>
            <input ref={fileInput} className="hidden" type="file" onChange={(event) => onFileSelected(event.target.files?.[0] ?? null)} />
            <Button secondary onClick={onRecordAudio}><Mic size={15} /> Record audio</Button>
          </div>
        ) : null}
      </div>
    </section>
  );
}
