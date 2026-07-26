import { CircleHelp } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

export function HelpTooltip({
  label,
  children,
  align = "start"
}: {
  label: string;
  children: string;
  align?: "start" | "end";
}) {
  const tooltipId = useId();
  const rootRef = useRef<HTMLSpanElement>(null);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [latched, setLatched] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const open = !dismissed && (hovered || focused || latched);

  useEffect(() => {
    if (!open) return;

    function dismiss() {
      setLatched(false);
      setDismissed(true);
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      dismiss();
    }

    function onPointerDown(event: PointerEvent) {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) dismiss();
    }

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  return (
    <span
      ref={rootRef}
      className="relative inline-flex"
      onMouseEnter={() => {
        setHovered(true);
        setDismissed(false);
      }}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => {
        setFocused(true);
        setDismissed(false);
      }}
      onBlur={(event) => {
        if (
          event.relatedTarget instanceof Node &&
          event.currentTarget.contains(event.relatedTarget)
        ) {
          return;
        }
        setFocused(false);
        setLatched(false);
        setDismissed(false);
      }}
    >
      <button
        type="button"
        className="inline-grid size-6 shrink-0 place-items-center rounded text-muted outline-none hover:bg-panel-strong hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45"
        aria-label={label}
        aria-describedby={tooltipId}
        onClick={() => {
          if (latched) {
            setLatched(false);
            setDismissed(true);
          } else {
            setLatched(true);
            setDismissed(false);
          }
        }}
      >
        <CircleHelp size={13} />
      </button>
      <span
        id={tooltipId}
        role="tooltip"
        hidden={!open}
        className={[
          "absolute top-6 z-20 w-64 max-w-[calc(100vw-2rem)] rounded-md border border-border bg-panel px-2.5 py-2 text-xs font-normal normal-case leading-5 text-text shadow-lg",
          align === "end" ? "right-0" : "left-0"
        ].join(" ")}
      >
        {children}
      </span>
    </span>
  );
}
