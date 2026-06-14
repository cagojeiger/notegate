import { useEffect, useId, useState } from "react";

export function MermaidBlock({ code }: { code: string }) {
  const reactId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
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
          theme: "neutral"
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
  }, [code, reactId]);

  if (svg) {
    return <div className="ng-mermaid" dangerouslySetInnerHTML={{ __html: svg }} />;
  }

  if (error) {
    return (
      <div className="ng-mermaid-error">
        <div className="mb-2 font-medium text-danger">Mermaid diagram could not be rendered.</div>
        <pre>{code}</pre>
      </div>
    );
  }

  return <div className="ng-mermaid-loading">Rendering diagram…</div>;
}
