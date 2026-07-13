export function Tabs<T extends string>({ items, value, onChange, label = "Tabs" }: { items: { id: T; label: string }[]; value: T; onChange: (value: T) => void; label?: string }) {
  return (
    <div role="tablist" aria-label={label} className="mb-5 flex max-w-full gap-1 overflow-x-auto border-b border-seam">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="tab"
          aria-selected={value === item.id}
          onClick={() => onChange(item.id)}
          className={`-mb-px shrink-0 rounded-t-lg border-b-2 px-3 py-2 text-sm font-medium transition ${value === item.id ? "border-primary text-text" : "border-transparent text-muted hover:bg-[var(--ng-hover)] hover:text-text"}`}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
