import { Download } from "lucide-react";

import type { RestNode } from "../../api/types";
import { downloadBlob } from "../../shared/lib/downloadBlob";
import { Button, Card, MetaRow } from "../../shared/ui";
import { useFileDownload } from "./useEditorQueries";

export function FileDetailView({ node }: { node: RestNode }) {
  const download = useFileDownload(node);
  async function handleDownload() {
    const blob = await download();
    downloadBlob(blob, node.original_filename ?? node.name);
  }
  return (
    <article className="mx-auto max-w-[44rem] px-10 py-14">
      <p className="text-sm text-muted">{node.path}</p>
      <h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1>
      <Card className="mt-8">
        <dl className="space-y-3">
          <MetaRow label="Media type" value={node.media_type ?? "unknown"} />
          <MetaRow label="Bytes" value={node.byte_len ?? 0} />
          <MetaRow label="SHA-256" value={node.content_sha256} />
        </dl>
      </Card>
      <Button className="mt-8" onClick={handleDownload}><Download size={16} /> Download</Button>
    </article>
  );
}
