import { Download } from "lucide-react";
import { useState } from "react";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { Button, Card, MetaRow } from "../../shared/ui";
import { useFileDownload } from "./useEditorQueries";
import { useFilePreviewUrl } from "./useFilePreviewQueries";

export function FileDetailView({ node }: { node: RestNode }) {
  const download = useFileDownload(node);
  const preview = useFilePreviewUrl(node);
  const [previewRecovery, setPreviewRecovery] = useState<{
    nodeId: string;
    retried: boolean;
    failedUrl: string | null;
  }>({ nodeId: node.id, retried: false, failedUrl: null });
  const currentRecovery = previewRecovery.nodeId === node.id
    ? previewRecovery
    : { nodeId: node.id, retried: false, failedUrl: null };
  const previewUrl = preview.data?.url;
  const previewFailed = Boolean(previewUrl && previewUrl === currentRecovery.failedUrl);
  const previewRequestFailed = !previewUrl
    && preview.isError
    && !(preview.error instanceof ApiError && preview.error.status === 404);
  async function handleDownload() {
    await download();
  }
  function handlePreviewError() {
    if (!previewUrl) return;
    if (currentRecovery.retried) {
      setPreviewRecovery({ nodeId: node.id, retried: true, failedUrl: previewUrl });
      return;
    }

    setPreviewRecovery({ nodeId: node.id, retried: true, failedUrl: previewUrl });
    void preview.refetch().then(() => {
      setPreviewRecovery((current) => current.nodeId === node.id
        ? { ...current, failedUrl: null }
        : current);
    });
  }
  return (
    <article className="mx-auto max-w-[44rem] px-10 py-14">
      <p className="text-sm text-muted">{node.path}</p>
      <h1 className="mt-4 text-4xl font-semibold tracking-tight">{node.name}</h1>
      {previewUrl && !previewFailed ? (
        <img
          className="mt-8 max-h-[65vh] max-w-full object-contain"
          src={previewUrl}
          alt={node.name}
          loading="lazy"
          decoding="async"
          onError={handlePreviewError}
        />
      ) : null}
      {previewFailed || previewRequestFailed ? <p className="mt-8 text-sm text-muted">Image cannot be displayed</p> : null}
      <Card className="mt-8">
        <dl className="space-y-3">
          <MetaRow label="Media type" value={node.media_type ?? "unknown"} />
          {node.detected_media_type || preview.data?.media_type ? (
            <MetaRow label="Detected type" value={node.detected_media_type ?? preview.data?.media_type ?? "unknown"} />
          ) : null}
          <MetaRow label="Bytes" value={node.byte_len ?? 0} />
        </dl>
      </Card>
      <Button className="mt-8" onClick={handleDownload}><Download size={16} /> Download</Button>
    </article>
  );
}
