import { ChevronLeft, ChevronRight, ZoomIn, ZoomOut } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Document, Page, pdfjs } from "react-pdf";
import "react-pdf/dist/Page/TextLayer.css";
import type { PDFPageProxy } from "pdfjs-dist";

import { IconButton } from "../../shared/ui";

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url
).toString();

const MAX_PAGE_WIDTH = 960;
const PAGE_HORIZONTAL_PADDING = 32;
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 2;
const ZOOM_STEP = 0.25;
const MAX_CANVAS_PIXELS = 16 * 1024 * 1024;
const MAX_CANVAS_DIMENSION = 8192;
const PDF_DEVICE_PIXEL_RATIO = typeof window === "undefined"
  ? 1
  : Math.min(window.devicePixelRatio || 1, 2);
const PDF_LOAD_OPTIONS = {
  disableRange: true,
  disableStream: true
};

export function PdfPreview({
  url,
  name,
  onError
}: {
  url: string;
  name: string;
  onError: () => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const errorReported = useRef(false);
  const [containerWidth, setContainerWidth] = useState(0);
  const [pageCount, setPageCount] = useState(0);
  const [pageNumber, setPageNumber] = useState(1);
  const [pageInput, setPageInput] = useState("1");
  const [zoom, setZoom] = useState(1);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    setContainerWidth(element.clientWidth);
    const observer = new ResizeObserver(([entry]) => {
      setContainerWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  function reportError() {
    if (errorReported.current) return;
    errorReported.current = true;
    onError();
  }

  function goToPage(nextPage: number) {
    if (pageCount === 0) return;
    const page = Math.min(pageCount, Math.max(1, nextPage));
    setPageNumber(page);
    setPageInput(String(page));
  }

  function commitPageInput() {
    const requestedPage = Number(pageInput);
    if (pageInput.trim() === "" || !Number.isInteger(requestedPage)) {
      setPageInput(String(pageNumber));
      return;
    }
    goToPage(requestedPage);
  }

  function changeZoom(delta: number) {
    setZoom((current) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, current + delta)));
  }

  const fittedPageWidth = containerWidth > 0
    ? Math.max(1, Math.min(MAX_PAGE_WIDTH, containerWidth - PAGE_HORIZONTAL_PADDING))
    : undefined;
  const pageWidth = fittedPageWidth ? fittedPageWidth * zoom : undefined;

  return (
    <div
      ref={containerRef}
      data-pdf-preview
      className="flex min-h-0 flex-1 flex-col overflow-hidden bg-bg"
      role="region"
      aria-label={`${name} PDF preview`}
    >
      <div
        className="min-h-0 flex-1 overflow-auto bg-bg p-4 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45 sm:p-6"
        role="region"
        aria-label={`${name} PDF pages`}
        tabIndex={0}
      >
        <Document
          className="min-h-full"
          file={url}
          options={PDF_LOAD_OPTIONS}
          loading={<PdfStatus>Loading PDF…</PdfStatus>}
          error={<PdfStatus>PDF cannot be displayed</PdfStatus>}
          onLoadSuccess={({ numPages }) => {
            setPageCount(numPages);
            setPageNumber(1);
            setPageInput("1");
          }}
          onLoadError={reportError}
          onSourceError={reportError}
        >
          {pageWidth ? (
            <BoundedPdfPage
              key={pageNumber}
              pageNumber={pageNumber}
              requestedWidth={pageWidth}
              onError={reportError}
            />
          ) : null}
        </Document>
      </div>
      <div className="flex h-12 shrink-0 items-center justify-center gap-2 border-t border-seam bg-panel px-2">
        <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
          {pageCount > 0 ? `Page ${pageNumber} of ${pageCount}` : "PDF page count unavailable"}
        </p>
        <div className="flex items-center gap-1 rounded-lg border border-seam bg-[var(--ng-editor)] px-1 py-0.5 shadow-sm">
          <IconButton
            label="Previous page"
            size="sm"
            disabled={pageNumber <= 1}
            onClick={() => goToPage(pageNumber - 1)}
          >
            <ChevronLeft size={16} />
          </IconButton>
          <div className="flex items-center gap-1 text-xs tabular-nums text-muted">
            <input
              type="number"
              aria-label="Page number"
              min={1}
              max={pageCount || undefined}
              step={1}
              value={pageInput}
              disabled={pageCount === 0}
              onChange={(event) => setPageInput(event.target.value)}
              onBlur={commitPageInput}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  commitPageInput();
                  event.currentTarget.blur();
                } else if (event.key === "Escape") {
                  setPageInput(String(pageNumber));
                  event.currentTarget.blur();
                }
              }}
              className="h-7 w-10 rounded-md border border-seam bg-panel px-1 text-center text-xs tabular-nums text-text outline-none focus-visible:ring-2 focus-visible:ring-primary/45 disabled:opacity-50"
            />
            <span aria-live="polite">/ {pageCount || "–"}</span>
          </div>
          <IconButton
            label="Next page"
            size="sm"
            disabled={pageCount === 0 || pageNumber >= pageCount}
            onClick={() => goToPage(pageNumber + 1)}
          >
            <ChevronRight size={16} />
          </IconButton>
        </div>
        <div className="flex items-center gap-1 rounded-lg border border-seam bg-[var(--ng-editor)] px-1 py-0.5 shadow-sm">
          <IconButton
            label="Zoom out"
            size="sm"
            disabled={zoom <= MIN_ZOOM}
            onClick={() => changeZoom(-ZOOM_STEP)}
          >
            <ZoomOut size={15} />
          </IconButton>
          <button
            type="button"
            aria-label="Reset zoom"
            title="Reset zoom"
            disabled={zoom === 1}
            onClick={() => setZoom(1)}
            className="h-7 min-w-11 rounded-md px-1 text-xs tabular-nums text-muted outline-none transition hover:bg-[var(--ng-hover)] hover:text-text focus-visible:ring-2 focus-visible:ring-primary/45 disabled:cursor-default"
          >
            {Math.round(zoom * 100)}%
          </button>
          <IconButton
            label="Zoom in"
            size="sm"
            disabled={zoom >= MAX_ZOOM}
            onClick={() => changeZoom(ZOOM_STEP)}
          >
            <ZoomIn size={15} />
          </IconButton>
        </div>
      </div>
    </div>
  );
}

function BoundedPdfPage({
  pageNumber,
  requestedWidth,
  onError
}: {
  pageNumber: number;
  requestedWidth: number;
  onError: () => void;
}) {
  const [pageSize, setPageSize] = useState<{ width: number; height: number } | null>(null);
  const pageWidth = pageSize
    ? boundedPageWidth(requestedWidth, pageSize.width, pageSize.height)
    : undefined;

  if (pageWidth === null) {
    return <PdfStatus>PDF page is too large to display</PdfStatus>;
  }

  return (
    <Page
      className="mx-auto w-fit overflow-hidden rounded-sm border border-seam bg-white shadow-md"
      pageNumber={pageNumber}
      width={pageWidth}
      devicePixelRatio={PDF_DEVICE_PIXEL_RATIO}
      renderMode={pageSize ? "canvas" : "none"}
      renderAnnotationLayer={false}
      renderTextLayer
      loading={<PdfStatus>Loading page…</PdfStatus>}
      onLoadSuccess={(page) => setPageSize(pageViewportSize(page))}
      onLoadError={onError}
      onRenderError={onError}
    />
  );
}

function pageViewportSize(page: PDFPageProxy) {
  const viewport = page.getViewport({ scale: 1 });
  return { width: viewport.width, height: viewport.height };
}

function boundedPageWidth(requestedWidth: number, sourceWidth: number, sourceHeight: number) {
  const aspectRatio = sourceHeight / sourceWidth;
  const pixelRatio = PDF_DEVICE_PIXEL_RATIO;
  const maxByArea = Math.sqrt(MAX_CANVAS_PIXELS / (aspectRatio * pixelRatio * pixelRatio));
  const maxByWidth = MAX_CANVAS_DIMENSION / pixelRatio;
  const maxByHeight = MAX_CANVAS_DIMENSION / (aspectRatio * pixelRatio);
  const width = Math.min(requestedWidth, maxByArea, maxByWidth, maxByHeight);
  return width >= 1 ? width : null;
}

function PdfStatus({ children }: { children: string }) {
  return <div className="grid min-h-48 place-items-center text-sm text-muted" role="status">{children}</div>;
}
