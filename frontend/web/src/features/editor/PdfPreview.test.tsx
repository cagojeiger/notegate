import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PdfPreview } from "./PdfPreview";

const pdfMock = vi.hoisted(() => ({
  loadSuccess: undefined as undefined | ((document: { numPages: number }) => void),
  loadError: undefined as undefined | (() => void),
  pageLoadSuccess: undefined as undefined | ((page: {
    getViewport: () => { width: number; height: number };
  }) => void),
  options: undefined as undefined | { disableRange?: boolean; disableStream?: boolean },
  page: undefined as undefined | {
    devicePixelRatio?: number;
    pageNumber: number;
    renderMode?: "canvas" | "custom" | "none";
    width?: number;
  }
}));

vi.mock("react-pdf", () => ({
  pdfjs: { GlobalWorkerOptions: { workerSrc: "" } },
  Document: ({
    children,
    onLoadSuccess,
    onLoadError,
    options
  }: {
    children: ReactNode;
    onLoadSuccess: (document: { numPages: number }) => void;
    onLoadError: () => void;
    options?: { disableRange?: boolean; disableStream?: boolean };
  }) => {
    pdfMock.loadSuccess = onLoadSuccess;
    pdfMock.loadError = onLoadError;
    pdfMock.options = options;
    return <div>{children}</div>;
  },
  Page: ({
    devicePixelRatio,
    onLoadSuccess,
    pageNumber,
    renderMode,
    width
  }: {
    devicePixelRatio?: number;
    onLoadSuccess: (page: { getViewport: () => { width: number; height: number } }) => void;
    pageNumber: number;
    renderMode?: "canvas" | "custom" | "none";
    width?: number;
  }) => {
    pdfMock.pageLoadSuccess = onLoadSuccess;
    pdfMock.page = { devicePixelRatio, pageNumber, renderMode, width };
    return <div>Rendered page {pageNumber}</div>;
  }
}));

class ResizeObserverMock {
  constructor(private readonly callback: ResizeObserverCallback) {}

  observe() {
    this.callback(
      [{ contentRect: { width: 800 } } as ResizeObserverEntry],
      this as unknown as ResizeObserver
    );
  }

  disconnect() {}
  unobserve() {}
}

describe("PdfPreview", () => {
  beforeEach(() => {
    pdfMock.loadSuccess = undefined;
    pdfMock.loadError = undefined;
    pdfMock.pageLoadSuccess = undefined;
    pdfMock.options = undefined;
    pdfMock.page = undefined;
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
  });

  it("renders one page and navigates within the document", () => {
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={vi.fn()} />);

    loadPdf(3);

    expect(screen.getByRole("spinbutton", { name: "Page number" })).toHaveValue(1);
    expect(screen.getByText("/ 3")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "document.pdf PDF pages" })).toHaveAttribute("tabindex", "0");
    expect(pdfMock.options).toEqual({ disableRange: true, disableStream: true });
    expect(pdfMock.page?.devicePixelRatio).toBeLessThanOrEqual(2);
    expect(screen.getByText("Rendered page 1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Next page" }));

    expect(screen.getByRole("spinbutton", { name: "Page number" })).toHaveValue(2);
    expect(screen.getByText("Rendered page 2")).toBeInTheDocument();
  });

  it("supports direct page entry and clamps it to the document", () => {
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={vi.fn()} />);
    loadPdf(12);

    const pageInput = screen.getByRole("spinbutton", { name: "Page number" });
    fireEvent.change(pageInput, { target: { value: "7" } });
    fireEvent.keyDown(pageInput, { key: "Enter" });
    expect(pageInput).toHaveValue(7);
    expect(screen.getByText("Rendered page 7")).toBeInTheDocument();

    fireEvent.change(pageInput, { target: { value: "99" } });
    fireEvent.blur(pageInput);
    expect(pageInput).toHaveValue(12);
    expect(screen.getByText("Rendered page 12")).toBeInTheDocument();
  });

  it("zooms from the fitted page width and resets to fit", () => {
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={vi.fn()} />);
    loadPdf(2);

    expect(pdfMock.page?.width).toBe(768);
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(pdfMock.page?.width).toBe(960);
    expect(screen.getByRole("button", { name: "Reset zoom" })).toHaveTextContent("125%");

    fireEvent.click(screen.getByRole("button", { name: "Reset zoom" }));
    expect(pdfMock.page?.width).toBe(768);
    expect(screen.getByRole("button", { name: "Reset zoom" })).toHaveTextContent("100%");

    fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
    fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
    expect(pdfMock.page?.width).toBe(384);
    expect(screen.getByRole("button", { name: "Reset zoom" })).toHaveTextContent("50%");
    expect(screen.getByRole("button", { name: "Zoom out" })).toBeDisabled();

    for (let index = 0; index < 6; index += 1) {
      fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    }
    expect(pdfMock.page?.width).toBe(1536);
    expect(screen.getByRole("button", { name: "Reset zoom" })).toHaveTextContent("200%");
    expect(screen.getByRole("button", { name: "Zoom in" })).toBeDisabled();
  });

  it("bounds the canvas dimensions for pages with extreme aspect ratios", () => {
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={vi.fn()} />);
    loadPdf(1, 100, 100_000);

    const width = pdfMock.page?.width ?? 0;
    const pixelRatio = pdfMock.page?.devicePixelRatio ?? 1;
    const height = width * 1_000;

    expect(pdfMock.page?.renderMode).toBe("canvas");
    expect(width * pixelRatio).toBeLessThanOrEqual(8192);
    expect(height * pixelRatio).toBeLessThanOrEqual(8192);
    expect(width * height * pixelRatio * pixelRatio).toBeLessThanOrEqual(16 * 1024 * 1024);
  });

  it("does not render a canvas when no safe page width is available", () => {
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={vi.fn()} />);
    loadPdf(1, 1, 100_000);

    expect(screen.getByText("PDF page is too large to display")).toBeInTheDocument();
  });

  it("reports document errors once", () => {
    const onError = vi.fn();
    render(<PdfPreview url="https://storage.example/document.pdf" name="document.pdf" onError={onError} />);

    act(() => {
      pdfMock.loadError?.();
      pdfMock.loadError?.();
    });

    expect(onError).toHaveBeenCalledTimes(1);
  });
});

function loadPdf(numPages: number, pageWidth = 612, pageHeight = 792) {
  act(() => pdfMock.loadSuccess?.({ numPages }));
  act(() => pdfMock.pageLoadSuccess?.({
    getViewport: () => ({ width: pageWidth, height: pageHeight })
  }));
}
