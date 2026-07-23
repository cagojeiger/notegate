import { useEffect, useId, useRef, useState } from "react";

import { useUiStore } from "../../stores/uiStore";
import { useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

export function MermaidBlock({ code }: { code: string }) {
  const reactId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const theme = useUiStore((state) => state.theme);
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setSvg(null);
    setError(null);

    async function renderDiagram() {
      try {
        const mermaid = (await import("mermaid")).default;
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: theme === "dark" ? "dark" : "neutral"
        });
        const result = await mermaid.render(`ng-mermaid-${reactId}`, code);
        if (active) setSvg(result.svg);
      } catch (err) {
        if (active) setError(err instanceof Error ? err.message : String(err));
      }
    }

    void renderDiagram();
    return () => {
      active = false;
    };
  }, [code, reactId, theme]);

  if (svg) {
    return <div className="ng-mermaid" dangerouslySetInnerHTML={{ __html: svg }} />;
  }

  if (error) {
    return <MermaidError code={code} />;
  }

  return <div className="ng-mermaid-loading">Rendering diagram…</div>;
}

function MermaidError({ code }: { code: string }) {
  const preRef = useRef<HTMLPreElement | null>(null);
  useResetHorizontalScrollOnGrow(preRef);

  return (
    <div className="ng-mermaid-error">
      <div className="mb-2 font-medium text-danger">Mermaid diagram could not be rendered.</div>
      <pre ref={preRef}>{code}</pre>
    </div>
  );
}
