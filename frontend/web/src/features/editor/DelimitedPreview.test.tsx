import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DelimitedPreview } from "./DelimitedPreview";

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count, estimateSize, paddingStart = 0 }: { count: number; estimateSize: (index: number) => number; paddingStart?: number }) => {
    const items = Array.from({ length: count }, (_, index) => {
      const size = estimateSize(index);
      const start = Array.from({ length: index }, (__, previous) => estimateSize(previous))
        .reduce((total, value) => total + value, paddingStart);
      return { index, key: index, size, start };
    });
    return {
      getTotalSize: () => items.reduce((total, item) => total + item.size, paddingStart),
      getVirtualItems: () => items
    };
  }
}));

vi.mock("./useResetHorizontalScrollOnGrow", () => ({
  useResetHorizontalScrollOnGrow: vi.fn()
}));

describe("DelimitedPreview", () => {
  it("renders CSV records with semantic headers and preserves unsafe text", () => {
    const unsafe = "</div><img src=x onerror=alert(1)>";
    render(<DelimitedPreview format="csv" content={`name,note\nAda,"hello, friend"\nGrace,"${unsafe}"`} identity="csv-1" />);

    const table = screen.getByRole("table", { name: "CSV data" });
    expect(table).toHaveAttribute("aria-rowcount", "3");
    expect(table).toHaveAttribute("aria-colcount", "3");
    expect(screen.getByRole("columnheader", { name: "name, column 1" })).toBeInTheDocument();
    expect(screen.getByRole("rowheader", { name: "2" })).toBeInTheDocument();
    expect(screen.getByText("hello, friend")).toBeInTheDocument();
    expect(screen.getByText(unsafe)).toBeInTheDocument();
    expect(document.querySelector("img")).not.toBeInTheDocument();
    expect(screen.getByText("2 records · 2 columns")).toBeInTheDocument();
  });

  it("can treat the first record as data and resets that choice for another document", async () => {
    const user = userEvent.setup();
    const view = render(<DelimitedPreview format="tsv" content={"name\tscore\nAda\t42"} identity="tsv-1" />);
    const headerToggle = screen.getByRole("checkbox", { name: "First row is header" });

    await user.click(headerToggle);

    expect(headerToggle).not.toBeChecked();
    expect(screen.getByRole("columnheader", { name: "Column 1, column 1" })).toBeInTheDocument();
    expect(screen.getByRole("rowheader", { name: "1" })).toBeInTheDocument();
    expect(screen.getByText("2 records · 2 columns")).toBeInTheDocument();

    view.rerender(<DelimitedPreview format="tsv" content={"city\tcountry\nSeoul\tKorea"} identity="tsv-2" />);

    await waitFor(() => expect(headerToggle).toBeChecked());
    expect(screen.getByRole("columnheader", { name: "city, column 1" })).toBeInTheDocument();
  });

  it("shows exact raw content in source mode", () => {
    const content = "name,note\nAda,\"line 1\nline 2\"\n";
    render(<DelimitedPreview format="csv" content={content} mode="source" />);

    expect(screen.getByRole("region", { name: "CSV source" }).querySelector("pre")?.textContent).toBe(content);
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("retains the header choice while switching between table and source", async () => {
    const user = userEvent.setup();
    const content = "name,score\nAda,42";
    const view = render(<DelimitedPreview format="csv" content={content} identity="csv-1" />);

    await user.click(screen.getByRole("checkbox", { name: "First row is header" }));
    view.rerender(<DelimitedPreview format="csv" content={content} identity="csv-1" mode="source" />);
    view.rerender(<DelimitedPreview format="csv" content={content} identity="csv-1" mode="table" />);

    expect(screen.getByRole("checkbox", { name: "First row is header" })).not.toBeChecked();
  });

  it("falls back to exact source when parsing fails", () => {
    const content = 'name,note\nAda,"unterminated';
    render(<DelimitedPreview format="csv" content={content} />);

    expect(screen.getByRole("alert")).toHaveTextContent(/Could not parse CSV/i);
    expect(screen.getByRole("region", { name: "CSV source" }).querySelector("pre")?.textContent).toBe(content);
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("falls back to exact source when the virtual table would exceed browser layout limits", () => {
    const content = ",".repeat(134_000);
    render(<DelimitedPreview format="csv" content={content} />);

    expect(screen.getByRole("status")).toHaveTextContent(/too large to preview reliably/i);
    expect(screen.getByRole("region", { name: "CSV source" }).querySelector("pre")?.textContent).toBe(content);
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });
});
