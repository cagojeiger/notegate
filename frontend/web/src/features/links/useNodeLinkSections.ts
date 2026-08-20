import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import { usePointerDrag } from "../../shared/hooks/usePointerDrag";
import { useUiStore } from "../../stores/uiStore";

export function useNodeLinkSections() {
  const linkRatio = useUiStore((state) => state.linkRatio);
  const setLinkRatio = useUiStore((state) => state.setLinkRatio);
  const [outgoingOpen, setOutgoingOpen] = useState(true);
  const [incomingOpen, setIncomingOpen] = useState(true);
  const gridRef = useRef<HTMLDivElement>(null);
  const startPointerDrag = usePointerDrag();
  const bothOpen = outgoingOpen && incomingOpen;

  function startResize(event: ReactPointerEvent) {
    if (!bothOpen) return;
    event.preventDefault();
    const rect = gridRef.current?.getBoundingClientRect();
    if (!rect) return;
    startPointerDrag((moveEvent) => setLinkRatio((moveEvent.clientY - rect.top) / rect.height));
  }

  const gridRows = bothOpen
    ? `${linkRatio}fr 6px ${1 - linkRatio}fr`
    : outgoingOpen
      ? "1fr 6px auto"
      : incomingOpen
        ? "auto 6px 1fr"
        : "auto 6px auto";

  return {
    gridRef,
    gridRows,
    bothOpen,
    linkRatio,
    setLinkRatio,
    outgoingOpen,
    incomingOpen,
    toggleOutgoing: () => setOutgoingOpen((open) => !open),
    toggleIncoming: () => setIncomingOpen((open) => !open),
    startResize
  };
}
