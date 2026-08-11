import { lazy, Suspense, useState } from "react";
import { AudioLines } from "lucide-react";

import { ApiError } from "../../api/errors";
import type { RestNode } from "../../api/types";
import { Card, MetaRow } from "../../shared/ui";
import { filePreviewKindForNode, useFilePreviewUrl } from "./useFilePreviewQueries";

const PdfPreview = lazy(() => import("./PdfPreview").then((module) => ({ default: module.PdfPreview })));

export function FileDetailView({ node }: { node: RestNode }) {
  const preview = useFilePreviewUrl(node);
  const previewKind = filePreviewKindForNode(node);
  const [previewRecovery, setPreviewRecovery] = useState<{
    nodeId: string;
    retried: boolean;
    failedUrl: string | null;
  }>({ nodeId: node.id, retried: false, failedUrl: null });
  const currentRecovery = previewRecovery.nodeId === node.id
    ? previewRecovery
    : { nodeId: node.id, retried: false, failedUrl: null };
  const isPdfPreview = previewKind === "pdf";
  const previewUrl = previewKind === null ? undefined : preview.data?.url;
  const previewFailed = Boolean(previewUrl && previewUrl === currentRecovery.failedUrl);
  const previewRequestFailed = !previewUrl
    && preview.isError
    && !(preview.error instanceof ApiError && preview.error.status === 404);
  const previewFailureLabel = isPdfPreview ? "PDF" : "Image";

  function handlePreviewError() {
    if (!previewUrl) return;
    if (currentRecovery.retried) {
      setPreviewRecovery({ nodeId: node.id, retried: true, failedUrl: previewUrl });
      return;
    }

    setPreviewRecovery({ nodeId: node.id, retried: true, failedUrl: previewUrl });
    void preview.refetch().then((result) => {
      const nextUrl = result.data?.url;
      if (!result.isSuccess || !nextUrl || nextUrl === previewUrl) return;
      setPreviewRecovery((current) => current.nodeId === node.id
        ? { ...current, failedUrl: null }
        : current);
    });
  }

  if (previewUrl && !previewFailed && isPdfPreview) {
    return (
      <Suspense fallback={<div className="grid min-h-0 flex-1 place-items-center text-sm text-muted">Preparing PDF…</div>}>
        <PdfPreview
          key={previewUrl}
          url={previewUrl}
          name={node.name}
          onError={handlePreviewError}
        />
      </Suspense>
    );
  }

  if (previewUrl && !previewFailed) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-6">
        <img
          className="max-h-full max-w-full object-contain"
          src={previewUrl}
          alt={node.name}
          loading="lazy"
          decoding="async"
          referrerPolicy="no-referrer"
          onError={handlePreviewError}
        />
      </div>
    );
  }

  if (previewKind !== null && preview.isLoading) {
    return <div className="grid min-h-0 flex-1 place-items-center text-sm text-muted">Loading preview…</div>;
  }

  return (
    <article className="min-h-0 w-full flex-1 overflow-y-auto" data-file-detail-scroll>
      <div className="mx-auto max-w-[44rem] px-6 py-10 sm:px-10 sm:py-14">
        <p className="text-sm text-muted">{node.path}</p>
        {node.file_media_kind === "audio" ? (
          <div className="mt-6 flex items-center gap-2 text-sm font-medium text-muted">
            <AudioLines size={18} /> Audio recording
          </div>
        ) : null}
        <h1 className="mt-4 text-3xl font-semibold tracking-tight sm:text-4xl">{node.name}</h1>
        {previewFailed || previewRequestFailed ? <p className="mt-8 text-sm text-muted">{previewFailureLabel} cannot be displayed</p> : null}
        <Card className="mt-8">
          <dl className="space-y-3">
            <MetaRow label="Media type" value={node.media_type ?? "unknown"} />
            {node.detected_media_type || preview.data?.media_type ? (
              <MetaRow label="Detected type" value={node.detected_media_type ?? preview.data?.media_type ?? "unknown"} />
            ) : null}
            <MetaRow label="Bytes" value={node.byte_len ?? 0} />
          </dl>
        </Card>
      </div>
    </article>
  );
}
