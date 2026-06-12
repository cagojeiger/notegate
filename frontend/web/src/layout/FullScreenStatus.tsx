import { Loader2 } from "lucide-react";

export function FullScreenStatus({ label, detail }: { label: string; detail?: string }) {
  return (
    <main className="grid h-full place-items-center bg-bg text-text">
      <div className="rounded-2xl border border-border bg-surface p-6 text-center">
        <Loader2 className="mx-auto mb-3 animate-spin text-primary" size={24} />
        <div className="font-semibold">{label}</div>
        {detail ? <div className="mt-2 max-w-md text-sm text-muted">{detail}</div> : null}
      </div>
    </main>
  );
}
