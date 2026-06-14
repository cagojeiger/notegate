import { useRef, type PointerEvent as ReactPointerEvent } from "react";

import { useUiStore } from "../../stores/uiStore";

export function usePrimarySidebarSections() {
  const treeRatio = useUiStore((state) => state.treeRatio);
  const setTreeRatio = useUiStore((state) => state.setTreeRatio);
  const treeSectionOpen = useUiStore((state) => state.treeSectionOpen);
  const recentSectionOpen = useUiStore((state) => state.recentSectionOpen);
  const recentDensity = useUiStore((state) => state.recentDensity);
  const toggleTreeSection = useUiStore((state) => state.toggleTreeSection);
  const toggleRecentSection = useUiStore((state) => state.toggleRecentSection);
  const toggleRecentDensity = useUiStore((state) => state.toggleRecentDensity);
  const gridRef = useRef<HTMLDivElement>(null);
  const bothSectionsOpen = treeSectionOpen && recentSectionOpen;

  function startTreeResize(event: ReactPointerEvent) {
    if (!bothSectionsOpen) return;
    event.preventDefault();
    const rect = gridRef.current?.getBoundingClientRect();
    if (!rect) return;
    const move = (e: PointerEvent) => setTreeRatio((e.clientY - rect.top) / rect.height);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("select-none");
    };
    document.body.classList.add("select-none");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  const gridRows = bothSectionsOpen
    ? `${treeRatio}fr 6px ${1 - treeRatio}fr`
    : treeSectionOpen
      ? "1fr 6px auto"
      : recentSectionOpen
        ? "auto 6px 1fr"
        : "auto 6px auto";

  return {
    gridRef,
    gridRows,
    bothSectionsOpen,
    treeSectionOpen,
    recentSectionOpen,
    recentDensity,
    toggleTreeSection,
    toggleRecentSection,
    toggleRecentDensity,
    startTreeResize
  };
}
