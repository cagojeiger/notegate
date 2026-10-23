import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useUiStore } from "../stores/uiStore";
import { Toast } from "./Toast";

afterEach(() => {
  useUiStore.setState({ toast: null });
  vi.useRealTimers();
});

it("announces feedback politely and keeps the existing two-second lifetime", () => {
  vi.useFakeTimers();
  useUiStore.setState({ toast: null });
  render(<Toast />);
  act(() => useUiStore.getState().showToast("Content copied"));
  expect(screen.getByRole("status")).toHaveTextContent("Content copied");
  expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
  expect(screen.getByRole("status")).toHaveAttribute("aria-atomic", "true");
  act(() => vi.advanceTimersByTime(1_999));
  expect(screen.getByRole("status")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(1));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
