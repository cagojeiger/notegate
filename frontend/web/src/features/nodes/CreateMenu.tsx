import { FilePlus, FolderPlus, Mic, Upload } from "lucide-react";
import { useEffect } from "react";

import { Card, MenuButton } from "../../shared/ui";

export default function CreateMenu({
  onCreateFolder,
  onCreateText,
  onRecordAudio,
  onFileSelected,
  onClose
}: {
  onCreateFolder: () => void;
  onCreateText: () => void;
  onRecordAudio: () => void;
  onFileSelected: (file: File | null) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function run(action: () => void) {
    action();
    onClose();
  }

  return (
    <>
      <div className="fixed inset-0 z-10" onClick={onClose} onContextMenu={(event) => { event.preventDefault(); onClose(); }} aria-hidden="true" />
      <Card className="absolute right-3 top-11 z-20 w-44 p-1 text-sm shadow-[var(--ng-focus-shadow)]" padding="none">
        <MenuButton onClick={() => run(onCreateFolder)}><FolderPlus size={14} /> New folder</MenuButton>
        <MenuButton onClick={() => run(onCreateText)}><FilePlus size={14} /> New document</MenuButton>
        <MenuButton onClick={() => run(onRecordAudio)}><Mic size={14} /> Record audio</MenuButton>
        <label className="flex cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-muted hover:bg-panel hover:text-text">
          <Upload size={14} /> Upload file
          <input
            className="hidden"
            type="file"
            onChange={(event) => {
              onFileSelected(event.target.files?.[0] ?? null);
              onClose();
            }}
          />
        </label>
      </Card>
    </>
  );
}
