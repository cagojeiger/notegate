import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject
} from "react";
import { createPortal } from "react-dom";

const VIEWPORT_GUTTER = 12;
const OVERLAY_OFFSET = 6;
const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "a[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])'
].join(",");

type Position = {
  left: number;
  top: number;
  width: number;
};

function restoreAnchorFocus(anchor: HTMLElement | null) {
  if (!anchor) return;
  const focusTarget = anchor.matches(FOCUSABLE_SELECTOR)
    ? anchor
    : anchor.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
  focusTarget?.focus();
}

export function AnchoredOverlay({
  anchorRef,
  open,
  onClose,
  id,
  label,
  role,
  width,
  estimatedHeight,
  children
}: {
  anchorRef: RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  id?: string;
  label: string;
  role: "dialog" | "menu";
  width: number;
  estimatedHeight: number;
  children: ReactNode;
}) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const [position, setPosition] = useState<Position | null>(null);
  onCloseRef.current = onClose;

  useLayoutEffect(() => {
    if (open && !anchorRef.current) {
      onCloseRef.current();
    }
  });

  useLayoutEffect(() => {
    if (!open) {
      setPosition((current) => current === null ? current : null);
      return;
    }
    const rect = anchorRef.current?.getBoundingClientRect();
    if (!rect) {
      setPosition((current) => current === null ? current : null);
      return;
    }

    const availableWidth = Math.max(0, window.innerWidth - VIEWPORT_GUTTER * 2);
    const renderedWidth = Math.min(width, availableWidth);
    const maxLeft = Math.max(VIEWPORT_GUTTER, window.innerWidth - renderedWidth - VIEWPORT_GUTTER);
    const left = Math.min(
      Math.max(VIEWPORT_GUTTER, rect.right - renderedWidth),
      maxLeft
    );
    const below = rect.bottom + OVERLAY_OFFSET;
    const top = below + estimatedHeight <= window.innerHeight - VIEWPORT_GUTTER
      ? below
      : Math.max(VIEWPORT_GUTTER, rect.top - estimatedHeight - OVERLAY_OFFSET);

    setPosition((current) => (
      current?.left === left && current.top === top && current.width === renderedWidth
        ? current
        : { left, top, width: renderedWidth }
    ));
  }, [anchorRef, estimatedHeight, open, width]);

  const positioned = position !== null;
  useEffect(() => {
    if (!open || !positioned) return;
    const surface = surfaceRef.current;
    const focusTarget = surface?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ?? surface;
    focusTarget?.focus();

    function dismiss(restoreFocus: boolean) {
      onCloseRef.current();
      if (restoreFocus) restoreAnchorFocus(anchorRef.current);
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss(true);
    }

    function onViewportChange(event: Event) {
      if (
        event.type === "scroll"
        && event.target instanceof Node
        && surface?.contains(event.target)
      ) {
        return;
      }
      dismiss(false);
    }

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
    };
  }, [anchorRef, open, positioned]);

  if (!open || !position) return null;

  return createPortal(
    <>
      <div
        aria-hidden="true"
        className="fixed inset-0 z-40 cursor-default bg-transparent"
        onClick={() => {
          onCloseRef.current();
          restoreAnchorFocus(anchorRef.current);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          onCloseRef.current();
          restoreAnchorFocus(anchorRef.current);
        }}
      />
      <div
        ref={surfaceRef}
        id={id}
        role={role}
        aria-label={label}
        tabIndex={-1}
        className="fixed z-50 font-ui outline-none"
        style={position}
      >
        {children}
      </div>
    </>,
    document.body
  );
}
