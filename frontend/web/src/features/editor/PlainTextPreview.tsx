import { useRef } from "react";

import { useResetHorizontalScrollOnGrow } from "./useResetHorizontalScrollOnGrow";

export function PlainTextPreview({ content }: { content: string }) {
  const preRef = useRef<HTMLPreElement | null>(null);
  useResetHorizontalScrollOnGrow(preRef);

  return (
    <pre ref={preRef} className="mx-auto min-h-0 w-full max-w-[52rem] flex-1 overflow-auto whitespace-pre-wrap px-10 py-14 font-sans text-base leading-7 text-text">
      {content}
    </pre>
  );
}
