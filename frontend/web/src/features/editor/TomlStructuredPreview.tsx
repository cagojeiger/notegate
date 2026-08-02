import { useMemo } from "react";
import { parse as parseToml } from "smol-toml";

import { StructuredPreviewLayout, type StructuredPreviewMode } from "./StructuredPreview";
import { parseStructuredTextWith } from "./structuredData";
import type { StructuredExpansionMode } from "./StructuredTreeView";

export function TomlStructuredPreview({ content, mode = "tree", expansionMode = "expanded" }: { content: string; mode?: StructuredPreviewMode; expansionMode?: StructuredExpansionMode }) {
  const parsed = useMemo(() => parseStructuredTextWith("toml", content, parseToml), [content]);

  return <StructuredPreviewLayout format="toml" content={content} parsed={parsed} mode={mode} expansionMode={expansionMode} />;
}
