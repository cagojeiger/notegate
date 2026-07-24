import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Toast } from "./Toast";

describe("Toast", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("asks its owner to clear the current message after two seconds", () => {
    vi.useFakeTimers();
    const onClear = vi.fn();

    render(<Toast message="Saved" onClear={onClear} />);

    expect(screen.getByText("Saved")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(2_000));
    expect(onClear).toHaveBeenCalledOnce();
  });
});
