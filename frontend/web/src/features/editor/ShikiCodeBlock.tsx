import { useEffect, useRef, useState } from "react";

import { normalizeCodeLanguage } from "./codeBlockLanguage";
import { useResetHorizontalScrollDescendantsOnGrow, useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

export function ShikiCodeBlock({ code, language = "text", className = "" }: { code: string; language?: string; className?: string }) {
  const [html, setHtml] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    setHtml(null);
    setFailed(false);

    async function highlight() {
      try {
        const { highlightCode } = await import("./highlightCode");
        const nextHtml = await highlightCode(code, normalizeCodeLanguage(language));
        if (active) setHtml(nextHtml);
      } catch {
        if (active) setFailed(true);
      }
    }

    void highlight();
    return () => {
      active = false;
    };
  }, [code, language]);

  if (html && !failed) return <HighlightedCodeBlock className={className} html={html} />;
  return <CodeFallback code={code} className={className} />;
}

function HighlightedCodeBlock({ className, html }: { className: string; html: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  useResetHorizontalScrollDescendantsOnGrow(containerRef, ".shiki, .ng-code-fallback");

  return <div ref={containerRef} className={className} dangerouslySetInnerHTML={{ __html: html }} />;
}

function CodeFallback({ code, className }: { code: string; className: string }) {
  const preRef = useRef<HTMLPreElement | null>(null);
  useResetHorizontalScrollOnGrow(preRef);

  return (
    <pre ref={preRef} className={`ng-code-fallback ${className}`} tabIndex={0}>
      <code>{code}</code>
    </pre>
  );
}
