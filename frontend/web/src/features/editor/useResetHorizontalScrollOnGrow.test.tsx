import { act, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { describe, expect, it } from "vitest";

import { installElementResizeMock } from "../../test/browserLayout";
import { useResetHorizontalScrollDescendantsOnGrow, useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

describe("useResetHorizontalScrollOnGrow", () => {
  it("resets a direct element's horizontal scroll only when it grows wider", () => {
    const resize = installElementResizeMock();

    try {
      render(<DirectScrollBox />);
      const box = screen.getByTestId("scroll-box");

      box.scrollLeft = 120;
      resize.setWidth(box, 240);
      act(() => resize.trigger(box));
      expect(box.scrollLeft).toBe(120);

      resize.setWidth(box, 480);
      act(() => resize.trigger(box));
      expect(box.scrollLeft).toBe(0);
    } finally {
      resize.restore();
    }
  });

  it("resets matching descendants when they grow wider", () => {
    const resize = installElementResizeMock();

    try {
      render(<DescendantScrollBox />);
      const target = screen.getByTestId("scroll-target");

      target.scrollLeft = 90;
      resize.setWidth(target, 520);
      act(() => resize.trigger(target));
      expect(target.scrollLeft).toBe(0);
    } finally {
      resize.restore();
    }
  });
});

function DirectScrollBox() {
  const ref = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollOnGrow(ref);

  return <div ref={ref} data-testid="scroll-box" />;
}

function DescendantScrollBox() {
  const ref = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollDescendantsOnGrow(ref, ".scroll-target");

  return (
    <div ref={ref}>
      <pre className="scroll-target" data-testid="scroll-target" />
    </div>
  );
}
