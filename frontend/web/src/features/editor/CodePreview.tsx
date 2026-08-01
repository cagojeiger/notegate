import { ShikiCodeBlock } from "./ShikiCodeBlock";
import { shikiLangForFormat, type CodeFormat } from "./textFormat";

export function CodePreview({ format, content }: { format: CodeFormat; content: string }) {
  return (
    <div className="mx-auto flex min-h-0 w-full max-w-[52rem] flex-1 flex-col overflow-hidden px-10 py-10">
      <div className="ng-source-flat min-h-0 flex-1 overflow-auto py-2">
        <ShikiCodeBlock code={content} language={shikiLangForFormat(format)} />
      </div>
    </div>
  );
}
