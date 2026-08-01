import { Copy } from "lucide-react";

import { copyText } from "../../shared/lib/clipboard";
import { IconButton } from "../../shared/ui";
import { useUiStore } from "../../stores/uiStore";

export function EndpointRow({ label, value, copyLabel }: { label: string; value: string; copyLabel: string }) {
  const showToast = useUiStore((state) => state.showToast);

  async function copy() {
    showToast((await copyText(value)) ? "Copied" : "Could not copy");
  }

  return (
    <div>
      <div className="mb-1 text-xs font-semibold uppercase tracking-[0.16em] text-muted">{label}</div>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-lg border border-border bg-bg px-3 py-2 font-mono text-xs">{value}</code>
        <IconButton label={copyLabel} onClick={() => { void copy(); }}><Copy size={15} /></IconButton>
      </div>
    </div>
  );
}
