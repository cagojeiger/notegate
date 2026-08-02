import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";

import { parseDelimitedText, type DelimitedFormat } from "./tabularData";
import { useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

export type DelimitedPreviewMode = "table" | "source";

const HEADER_HEIGHT = 36;
const ROW_HEIGHT = 36;
const RECORD_COLUMN_WIDTH = 56;
const MIN_COLUMN_WIDTH = 120;
const MAX_COLUMN_WIDTH = 560;
const COLUMN_HORIZONTAL_PADDING = 24;
const APPROXIMATE_CHARACTER_WIDTH = 8;
const MAX_VIRTUAL_TABLE_DIMENSION = 16_000_000;

export function DelimitedPreview({ format, content, mode = "table", identity }: { format: DelimitedFormat; content: string; mode?: DelimitedPreviewMode; identity?: string }) {
  const [firstRowIsHeader, setFirstRowIsHeader] = useState(true);

  useEffect(() => setFirstRowIsHeader(true), [identity]);

  if (mode === "source") return <DelimitedSource format={format} content={content} />;
  return (
    <DelimitedTable
      format={format}
      content={content}
      firstRowIsHeader={firstRowIsHeader}
      onFirstRowIsHeaderChange={setFirstRowIsHeader}
    />
  );
}

function DelimitedTable({ format, content, firstRowIsHeader, onFirstRowIsHeaderChange }: { format: DelimitedFormat; content: string; firstRowIsHeader: boolean; onFirstRowIsHeaderChange: (value: boolean) => void }) {
  const parsed = useMemo(() => parseDelimitedText(format, content), [content, format]);

  if (!parsed.ok) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="border-b border-danger/30 bg-danger/10 px-4 py-2 text-sm text-danger" role="alert">
          Could not parse {format.toUpperCase()}: {parsed.message}. Showing source instead.
        </div>
        <DelimitedSource format={format} content={content} />
      </div>
    );
  }

  return <ParsedDelimitedTable format={format} content={content} rows={parsed.rows} columnCount={parsed.columnCount} firstRowIsHeader={firstRowIsHeader} onFirstRowIsHeaderChange={onFirstRowIsHeaderChange} />;
}

function ParsedDelimitedTable({ format, content, rows, columnCount, firstRowIsHeader, onFirstRowIsHeaderChange }: { format: DelimitedFormat; content: string; rows: string[][]; columnCount: number; firstRowIsHeader: boolean; onFirstRowIsHeaderChange: (value: boolean) => void }) {
  const recordCount = firstRowIsHeader ? Math.max(rows.length - 1, 0) : rows.length;
  const tableHeight = HEADER_HEIGHT + recordCount * ROW_HEIGHT;
  const minimumLayoutExceedsLimit = RECORD_COLUMN_WIDTH + columnCount * MIN_COLUMN_WIDTH > MAX_VIRTUAL_TABLE_DIMENSION
    || tableHeight > MAX_VIRTUAL_TABLE_DIMENSION;
  const columnWidths = useMemo(
    () => minimumLayoutExceedsLimit ? [] : widthsForColumns(rows, columnCount),
    [columnCount, minimumLayoutExceedsLimit, rows]
  );
  const tableWidth = minimumLayoutExceedsLimit
    ? MAX_VIRTUAL_TABLE_DIMENSION + 1
    : RECORD_COLUMN_WIDTH + columnWidths.reduce((total, width) => total + width, 0);

  if (tableWidth > MAX_VIRTUAL_TABLE_DIMENSION || tableHeight > MAX_VIRTUAL_TABLE_DIMENSION) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm text-warning" role="status">
          This table is too large to preview reliably. Showing source instead.
        </div>
        <DelimitedSource format={format} content={content} />
      </div>
    );
  }

  return <VirtualizedDelimitedTable format={format} rows={rows} columnCount={columnCount} columnWidths={columnWidths} firstRowIsHeader={firstRowIsHeader} onFirstRowIsHeaderChange={onFirstRowIsHeaderChange} />;
}

function VirtualizedDelimitedTable({ format, rows, columnCount, columnWidths, firstRowIsHeader, onFirstRowIsHeaderChange }: { format: DelimitedFormat; rows: string[][]; columnCount: number; columnWidths: number[]; firstRowIsHeader: boolean; onFirstRowIsHeaderChange: (value: boolean) => void }) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollOnGrow(scrollRef);

  const header = firstRowIsHeader ? rows[0] ?? [] : [];
  const records = firstRowIsHeader ? rows.slice(1) : rows;
  const rowVirtualizer = useVirtualizer({
    count: records.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    paddingStart: HEADER_HEIGHT,
    overscan: 8
  });
  const columnVirtualizer = useVirtualizer({
    horizontal: true,
    count: columnCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => columnWidths[index] ?? MIN_COLUMN_WIDTH,
    paddingStart: RECORD_COLUMN_WIDTH,
    overscan: 2
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const virtualColumns = columnVirtualizer.getVirtualItems();
  const tableWidth = columnVirtualizer.getTotalSize();
  const tableHeight = rowVirtualizer.getTotalSize();

  return (
    <div className="flex min-h-0 w-full flex-1 flex-col overflow-hidden">
      <div className="flex min-h-9 shrink-0 flex-wrap items-center justify-between gap-x-4 gap-y-1 border-b border-seam bg-panel px-4 py-1.5 text-xs text-muted">
        <label className="inline-flex min-h-6 cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            checked={firstRowIsHeader}
            onChange={(event) => onFirstRowIsHeaderChange(event.currentTarget.checked)}
          />
          First row is header
        </label>
        <span>{records.length} records · {columnCount} columns</span>
      </div>
      {columnCount === 0 ? (
        <div className="grid min-h-0 flex-1 place-items-center p-8 text-sm text-muted">No records to display.</div>
      ) : (
        <div
          ref={scrollRef}
          className="min-h-0 flex-1 overflow-auto outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/45"
          role="region"
          aria-label={`${format.toUpperCase()} table preview`}
          tabIndex={0}
          data-delimited-table-scroll
        >
          <div
            role="table"
            aria-label={`${format.toUpperCase()} data`}
            aria-rowcount={records.length + 1}
            aria-colcount={columnCount + 1}
            className="relative font-mono text-xs text-text"
            style={{ width: tableWidth, height: tableHeight }}
          >
            <div
              role="row"
              aria-rowindex={1}
              className="sticky top-0 z-20 bg-panel font-semibold"
              style={{ width: tableWidth, height: HEADER_HEIGHT }}
            >
              <div
                role="columnheader"
                aria-colindex={1}
                className="sticky left-0 z-30 flex h-9 items-center justify-end border-b border-r border-seam bg-panel px-3 text-faint"
                style={{ width: RECORD_COLUMN_WIDTH }}
              >
                #
              </div>
              {virtualColumns.map((virtualColumn) => {
                const columnIndex = virtualColumn.index;
                const label = firstRowIsHeader && header[columnIndex] !== undefined
                  ? header[columnIndex]
                  : `Column ${columnIndex + 1}`;
                return (
                  <div
                    key={virtualColumn.key}
                    role="columnheader"
                    aria-colindex={columnIndex + 2}
                    aria-label={`${label || `Column ${columnIndex + 1}`}, column ${columnIndex + 1}`}
                    className="absolute top-0 flex h-9 items-center overflow-hidden text-ellipsis whitespace-nowrap border-b border-r border-seam bg-panel px-3"
                    style={{ left: virtualColumn.start, width: virtualColumn.size }}
                    title={label || `Column ${columnIndex + 1}`}
                  >
                    {label || `Column ${columnIndex + 1}`}
                  </div>
                );
              })}
            </div>
            {virtualRows.map((virtualRow) => {
              const record = records[virtualRow.index] ?? [];
              const sourceRecordNumber = virtualRow.index + (firstRowIsHeader ? 2 : 1);
              return (
                <div
                  key={virtualRow.key}
                  role="row"
                  aria-rowindex={virtualRow.index + 2}
                  className="absolute left-0 top-0 hover:bg-[var(--ng-hover)]"
                  style={{ width: tableWidth, height: virtualRow.size, transform: `translateY(${virtualRow.start}px)` }}
                >
                  <div
                    role="rowheader"
                    aria-colindex={1}
                    className="sticky left-0 z-10 flex h-9 items-center justify-end border-b border-r border-seam bg-[var(--ng-editor)] px-3 text-faint"
                    style={{ width: RECORD_COLUMN_WIDTH }}
                  >
                    {sourceRecordNumber}
                  </div>
                  {virtualColumns.map((virtualColumn) => {
                    const value = record[virtualColumn.index] ?? "";
                    return (
                      <div
                        key={virtualColumn.key}
                        role="cell"
                        aria-colindex={virtualColumn.index + 2}
                        className="absolute top-0 flex h-9 items-center overflow-hidden text-ellipsis whitespace-nowrap border-b border-r border-seam px-3"
                        style={{ left: virtualColumn.start, width: virtualColumn.size }}
                        title={value || undefined}
                      >
                        {value}
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function DelimitedSource({ format, content }: { format: DelimitedFormat; content: string }) {
  return (
    <div className="min-h-0 w-full flex-1 overflow-auto px-5 py-8" role="region" aria-label={`${format.toUpperCase()} source`}>
      <pre className="m-0 min-w-max whitespace-pre font-mono text-sm leading-6 text-text">{content}</pre>
    </div>
  );
}

function widthsForColumns(rows: string[][], columnCount: number): number[] {
  const longestValues = Array.from({ length: columnCount }, (_, columnIndex) => `Column ${columnIndex + 1}`.length);
  for (const row of rows) {
    row.forEach((value, columnIndex) => {
      longestValues[columnIndex] = Math.max(longestValues[columnIndex] ?? 0, value.length);
    });
  }
  return longestValues.map((longest) => Math.min(
    MAX_COLUMN_WIDTH,
    Math.max(MIN_COLUMN_WIDTH, longest * APPROXIMATE_CHARACTER_WIDTH + COLUMN_HORIZONTAL_PADDING)
  ));
}
