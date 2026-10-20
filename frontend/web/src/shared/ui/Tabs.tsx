import { useRef, type KeyboardEvent } from "react";

type TabItem<T extends string> = {
  id: T;
  label: string;
  disabled?: boolean;
  controls?: string;
};

type TabsProps<T extends string> = {
  items: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  label?: string;
  variant?: "default" | "header";
  orientation?: "horizontal" | "vertical";
};

export function Tabs<T extends string>({ items, value, onChange, label = "Tabs", variant = "default", orientation = "horizontal" }: TabsProps<T>) {
  const vertical = orientation === "vertical";
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = items.findIndex((item) => item.id === value && !item.disabled);
  const tabStopIndex = selectedIndex >= 0 ? selectedIndex : items.findIndex((item) => !item.disabled);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, currentIndex: number) => {
    const previousKey = vertical ? "ArrowUp" : "ArrowLeft";
    const nextKey = vertical ? "ArrowDown" : "ArrowRight";
    if (![previousKey, nextKey, "Home", "End"].includes(event.key)) return;

    const enabledIndexes = items.flatMap((item, index) => (item.disabled ? [] : [index]));
    const currentPosition = enabledIndexes.indexOf(currentIndex);
    if (currentPosition < 0) return;

    event.preventDefault();

    let nextPosition = currentPosition;
    if (event.key === previousKey) nextPosition = (currentPosition - 1 + enabledIndexes.length) % enabledIndexes.length;
    if (event.key === nextKey) nextPosition = (currentPosition + 1) % enabledIndexes.length;
    if (event.key === "Home") nextPosition = 0;
    if (event.key === "End") nextPosition = enabledIndexes.length - 1;

    const nextIndex = enabledIndexes[nextPosition];
    const nextItem = items[nextIndex];
    if (!nextItem) return;

    tabRefs.current[nextIndex]?.focus();
    if (nextItem.id !== value) onChange(nextItem.id);
  };

  return (
    <div
      role="tablist"
      aria-label={label}
      aria-orientation={orientation}
      className={vertical ? "flex max-w-full flex-col gap-1" : variant === "header"
        ? "flex h-full max-w-full items-end gap-1 overflow-x-auto"
        : "mb-5 flex max-w-full gap-1 overflow-x-auto border-b border-seam"}
    >
      {items.map((item, index) => (
        <button
          key={item.id}
          ref={(element) => {
            tabRefs.current[index] = element;
          }}
          type="button"
          role="tab"
          id={item.controls ? `${item.controls}-tab` : undefined}
          aria-selected={value === item.id}
          aria-disabled={item.disabled || undefined}
          aria-controls={item.controls}
          disabled={item.disabled}
          tabIndex={index === tabStopIndex ? 0 : -1}
          onClick={() => onChange(item.id)}
          onKeyDown={(event) => handleKeyDown(event, index)}
          className={`${vertical ? "border-l-2 px-3 py-2 text-left" : `-mb-px rounded-t-workbench-surface border-b-2 ${variant === "header" ? "flex h-[calc(var(--ng-workbench-header-size)-4px)] items-center px-2" : "px-2.5 py-1.5"}`} min-h-[var(--ng-min-target-size)] shrink-0 text-workbench font-medium transition ${value === item.id ? `border-primary text-text ${vertical ? "bg-[var(--ng-selection)]" : ""}` : item.disabled ? "border-transparent text-muted" : "border-transparent text-muted hover:bg-[var(--ng-hover)] hover:text-text"}${item.disabled ? " cursor-not-allowed opacity-50" : ""}`}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
