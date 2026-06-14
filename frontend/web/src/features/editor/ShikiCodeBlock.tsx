import { useEffect, useState } from "react";

const LANGUAGE_ALIASES: Record<string, string> = {
  md: "markdown",
  yml: "yaml",
  text: "text",
  txt: "text"
};

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
        const normalizedLanguage = LANGUAGE_ALIASES[language.toLowerCase()] ?? language.toLowerCase();
        const nextHtml = await highlightCode(code, normalizedLanguage);
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

  if (html && !failed) {
    return <div className={className} dangerouslySetInnerHTML={{ __html: html }} />;
  }

  return (
    <pre className={`ng-code-fallback ${className}`} tabIndex={0}>
      <code>{code}</code>
    </pre>
  );
}
