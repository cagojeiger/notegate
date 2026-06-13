import { useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";

export function useSidebarKeyboardNavigation() {
  const asideRef = useRef<HTMLElement>(null);

  function onSidebarKeyDown(event: ReactKeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const buttons = Array.from(asideRef.current?.querySelectorAll<HTMLButtonElement>("[data-node-open]") ?? []);
    if (buttons.length === 0) return;
    event.preventDefault();
    const current = document.activeElement as HTMLElement | null;
    const index = current ? buttons.indexOf(current as HTMLButtonElement) : -1;
    const next = event.key === "ArrowDown" ? Math.min(index + 1, buttons.length - 1) : Math.max(index <= 0 ? 0 : index - 1, 0);
    buttons[next]?.focus();
  }

  return { asideRef, onSidebarKeyDown };
}
