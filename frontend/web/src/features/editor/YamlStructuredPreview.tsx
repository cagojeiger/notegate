import { useMemo } from "react";
import { parse as parseYaml } from "yaml";

import { StructuredPreviewLayout, type StructuredPreviewMode } from "./StructuredPreview";
import { parseStructuredTextWith } from "./structuredData";
import type { StructuredExpansionMode } from "./StructuredTreeView";

export function YamlStructuredPreview({ content, mode = "tree", expansionMode = "expanded" }: { content: string; mode?: StructuredPreviewMode; expansionMode?: StructuredExpansionMode }) {
  const parsed = useMemo(() => parseStructuredTextWith("yaml", content, parseYaml), [content]);

  return <StructuredPreviewLayout format="yaml" content={content} parsed={parsed} mode={mode} expansionMode={expansionMode} />;
}
