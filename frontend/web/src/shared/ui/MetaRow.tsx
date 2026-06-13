import type { ReactNode } from "react";

export function MetaRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="grid grid-cols-[6rem_1fr] gap-3 text-sm">
      <dt className="font-semibold text-text">{label}</dt>
      <dd className="min-w-0 break-words text-muted">{value}</dd>
    </div>
  );
}
