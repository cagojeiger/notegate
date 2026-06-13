import type { ReactNode } from "react";

export function MetaRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] gap-3 text-sm leading-5">
      <dt className="font-medium text-text">{label}</dt>
      <dd className="min-w-0 break-words text-muted">{value}</dd>
    </div>
  );
}
