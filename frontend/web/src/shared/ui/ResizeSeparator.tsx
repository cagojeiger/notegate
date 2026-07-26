import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEventHandler
} from "react";

type ResizeSeparatorProps = {
  orientation: "horizontal" | "vertical";
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  valueText?: string;
  controls?: string;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
  onValueChange: (value: number) => void;
};

export function ResizeSeparator({
  orientation,
  label,
  value,
  min,
  max,
  step,
  valueText,
  controls,
  onPointerDown,
  onValueChange
}: ResizeSeparatorProps) {
  const vertical = orientation === "vertical";

  function handleKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const decreaseKey = vertical ? "ArrowLeft" : "ArrowUp";
    const increaseKey = vertical ? "ArrowRight" : "ArrowDown";
    let nextValue: number | null = null;

    if (event.key === decreaseKey) nextValue = value - step;
    if (event.key === increaseKey) nextValue = value + step;
    if (event.key === "Home") nextValue = min;
    if (event.key === "End") nextValue = max;
    if (nextValue === null) return;

    event.preventDefault();
    onValueChange(Math.max(min, Math.min(max, nextValue)));
  }

  return (
    <div
      role="separator"
      tabIndex={0}
      aria-label={label}
      aria-orientation={orientation}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={valueText}
      aria-controls={controls}
      onPointerDown={onPointerDown}
      onKeyDown={handleKeyDown}
      className={[
        "group absolute z-10 touch-none outline-none",
        vertical
          ? "inset-y-0 left-1/2 w-6 -translate-x-1/2 cursor-col-resize"
          : "inset-x-0 top-1/2 h-6 -translate-y-1/2 cursor-row-resize"
      ].join(" ")}
    >
      <span
        aria-hidden="true"
        className={[
          "pointer-events-none absolute bg-seam transition-colors group-hover:bg-[var(--ng-active-border)] group-focus-visible:bg-primary",
          vertical
            ? "inset-y-0 left-1/2 w-px -translate-x-1/2"
            : "inset-x-0 top-1/2 h-px -translate-y-1/2"
        ].join(" ")}
      />
    </div>
  );
}
