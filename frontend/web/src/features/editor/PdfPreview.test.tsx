import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "../../stores/uiStore";
import { PdfPreview } from "./PdfPreview";

const { pdfViewerMock } = vi.hoisted(() => ({
  pdfViewerMock: vi.fn()
}));

vi.mock("@embedpdf/react-pdf-viewer", () => ({
  PDFViewer: (props: unknown) => {
    pdfViewerMock(props);
    return <div data-testid="embedpdf-viewer" />;
  }
}));

describe("PdfPreview", () => {
  beforeEach(() => {
    pdfViewerMock.mockClear();
    useUiStore.setState({ theme: "light" });
  });

  it("configures a local, read-only, theme-aware viewer", () => {
    render(<PdfPreview name="document.pdf" onError={vi.fn()} url="https://storage.example/document.pdf" />);

    const props = pdfViewerMock.mock.calls[pdfViewerMock.mock.calls.length - 1][0];
    expect(props.config).toMatchObject({
      src: "https://storage.example/document.pdf",
      tabBar: "never",
      disabledCategories: expect.arrayContaining(["annotation", "redaction", "insert", "document-open", "panel-comment"]),
      export: { defaultFileName: "document.pdf" },
      fontFallback: null,
      fonts: { ui: null, signature: null },
      theme: { preference: "light" }
    });
  });

  it("reports document load errors and removes the listener on unmount", () => {
    const onError = vi.fn();
    const unsubscribe = vi.fn();
    let documentErrorListener: (() => void) | undefined;
    const onDocumentError = vi.fn((listener: () => void) => {
      documentErrorListener = listener;
      return unsubscribe;
    });
    const { unmount } = render(
      <PdfPreview name="document.pdf" onError={onError} url="https://storage.example/document.pdf" />
    );
    const props = pdfViewerMock.mock.calls[pdfViewerMock.mock.calls.length - 1][0];

    act(() => {
      props.onReady({
        getPlugin: () => ({
          provides: () => ({ onDocumentError })
        })
      });
    });
    if (!documentErrorListener) throw new Error("Document error listener was not registered");
    act(documentErrorListener);

    expect(onError).toHaveBeenCalledOnce();
    unmount();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});
