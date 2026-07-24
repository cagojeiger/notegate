export type FilePreviewKind = "image" | "pdf";

const IMAGE_PREVIEW_MEDIA_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/avif",
  "image/gif"
]);

export function filePreviewKind(mediaType?: string): FilePreviewKind | null {
  if (!mediaType) return null;
  if (IMAGE_PREVIEW_MEDIA_TYPES.has(mediaType)) return "image";
  if (mediaType === "application/pdf") return "pdf";
  return null;
}
