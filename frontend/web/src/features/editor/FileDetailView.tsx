import { Download } from "lucide-react";
import { useState } from "react";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { filePreviewKind } from "../../shared/lib/filePreview";
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
  const previewKind = filePreviewKind(preview.data?.media_type);
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
    <article className="min-h-0 w-full flex-1 overflow-y-auto" data-file-detail-scroll>
      <div className={`mx-auto px-6 py-10 sm:px-10 sm:py-14 ${previewKind === "pdf" ? "max-w-5xl" : "max-w-[44rem]"}`}>
        <p className="text-sm text-muted">{node.path}</p>
        <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">{node.name}</h1>
        {previewUrl && previewKind === "image" && !previewFailed ? (
          <img
            className="mt-8 max-h-[65vh] max-w-full object-contain"
            src={previewUrl}
            alt={node.name}
            loading="lazy"
            decoding="async"
            onError={handlePreviewError}
          />
        ) : null}
        {previewUrl && previewKind === "pdf" ? (
          <iframe
            title={`PDF preview: ${node.name}`}
            src={previewUrl}
            loading="lazy"
            referrerPolicy="no-referrer"
            className="mt-8 h-[70vh] min-h-96 w-full rounded-xl border border-border bg-surface"
          />
        ) : null}
        {previewFailed ? <p className="mt-8 text-sm text-muted">Image cannot be displayed</p> : null}
        {previewRequestFailed ? <p className="mt-8 text-sm text-muted">File preview cannot be displayed</p> : null}
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
      </div>
    </article>
  );
}
