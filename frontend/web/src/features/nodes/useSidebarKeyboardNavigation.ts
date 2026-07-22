import { useCallback, useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";

import type { TreeKeyboardNavigation, TreeKeyboardNavigationRegistrar } from "./types";

export function useSidebarKeyboardNavigation() {
  const asideRef = useRef<HTMLElement>(null);
  const treeNavigationRef = useRef<TreeKeyboardNavigation | null>(null);
  const registerTreeNavigation: TreeKeyboardNavigationRegistrar = useCallback((navigation) => {
    treeNavigationRef.current = navigation;
  }, []);

  function onSidebarKeyDown(event: ReactKeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const target = event.target as HTMLElement;

    if (target.closest('[role="tree"]')) {
      event.preventDefault();
      if (event.key === "ArrowDown") {
        asideRef.current?.querySelector<HTMLButtonElement>("[data-recent-list] [data-node-open]")?.focus();
      }
      return;
    }

    const recentRow = target.closest("[data-recent-list] [data-node-row]");
    if (!recentRow) return;
    const buttons = Array.from(
      asideRef.current?.querySelectorAll<HTMLButtonElement>("[data-recent-list] [data-node-open]") ?? []
    );
    if (buttons.length === 0) return;
    event.preventDefault();
    const current = target.closest<HTMLButtonElement>("[data-node-open]")
      ?? recentRow.querySelector<HTMLButtonElement>("[data-node-open]");
    const index = current ? buttons.indexOf(current) : -1;

    if (event.key === "ArrowUp" && index <= 0) {
      if (!treeNavigationRef.current?.focusLastNode()) current?.focus();
      return;
    }

    const nextIndex = event.key === "ArrowDown"
      ? Math.min(index + 1, buttons.length - 1)
      : Math.max(index - 1, 0);
    buttons[nextIndex]?.focus();
  }

  return { asideRef, onSidebarKeyDown, registerTreeNavigation };
}
