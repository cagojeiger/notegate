import Papa from "papaparse";

import type { TabularFormat } from "./textFormat";

export type DelimitedFormat = TabularFormat;

export type DelimitedParseResult =
  | { ok: true; rows: string[][]; columnCount: number }
  | { ok: false; message: string };

export function parseDelimitedText(format: DelimitedFormat, content: string): DelimitedParseResult {
  try {
    const normalizedContent = content.replace(/^\uFEFF/, "").replace(/\r\n/g, "\n");
    const result = Papa.parse<string[]>(normalizedContent, {
      delimiter: format === "csv" ? "," : "\t",
      newline: "\n",
      header: false,
      skipEmptyLines: false,
      dynamicTyping: false
    });

    if (result.errors.length > 0) {
      return { ok: false, message: result.errors.map((error) => error.message).join("; ") };
    }

    const rows = normalizedContent.endsWith("\n") && isEmptyRow(result.data[result.data.length - 1])
      ? result.data.slice(0, -1)
      : result.data;
    const columnCount = rows.reduce((maximum, row) => Math.max(maximum, row.length), 0);
    return { ok: true, rows, columnCount };
  } catch (error) {
    return { ok: false, message: error instanceof Error ? error.message : String(error) };
  }
}

function isEmptyRow(row: string[] | undefined): boolean {
  return row?.length === 1 && row[0] === "";
}
