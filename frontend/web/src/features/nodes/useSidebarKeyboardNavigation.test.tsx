import { fireEvent, render } from "@testing-library/react";
import { useLayoutEffect } from "react";
import { describe, expect, it, vi } from "vitest";

import type { TreeKeyboardNavigation } from "./types";
import { useSidebarKeyboardNavigation } from "./useSidebarKeyboardNavigation";

describe("useSidebarKeyboardNavigation", () => {
  it("moves from the first Recent row to the logical last Files node", () => {
    const focusLastNode = vi.fn(() => true);
    const view = render(<Harness navigation={{ focusLastNode }} />);
    const recent = view.getByRole("button", { name: "Recent one" });

    recent.focus();
    fireEvent.keyDown(recent, { key: "ArrowUp" });

    expect(focusLastNode).toHaveBeenCalledOnce();
    expect(view.getByRole("button", { name: "Mounted Files row" })).not.toHaveFocus();
  });

  it("moves from the last Files row to the first Recent row", () => {
    const view = render(<Harness navigation={{ focusLastNode: () => true }} />);
    const file = view.getByRole("button", { name: "Mounted Files row" });

    file.focus();
    fireEvent.keyDown(file, { key: "ArrowDown" });

    expect(view.getByRole("button", { name: "Recent one" })).toHaveFocus();
  });
});

function Harness({ navigation }: { navigation: TreeKeyboardNavigation }) {
  const { asideRef, onSidebarKeyDown, registerTreeNavigation } = useSidebarKeyboardNavigation();
  useLayoutEffect(() => {
    registerTreeNavigation(navigation);
    return () => registerTreeNavigation(null);
  }, [navigation, registerTreeNavigation]);

  return (
    <aside ref={asideRef} onKeyDown={onSidebarKeyDown}>
      <div role="tree" aria-label="Files">
        <div data-node-row><button data-node-open>Mounted Files row</button></div>
      </div>
      <div data-recent-list>
        <div data-node-row><button data-node-open>Recent one</button></div>
        <div data-node-row><button data-node-open>Recent two</button></div>
      </div>
    </aside>
  );
}
