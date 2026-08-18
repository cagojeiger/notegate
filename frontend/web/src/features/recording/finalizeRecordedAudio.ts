import { loadEbml } from "./ebmlBrowser";

const WEBM_MEDIA_TYPE = /^audio\/webm(?:;|$)/i;

export async function finalizeRecordedAudio(blob: Blob): Promise<Blob> {
  if (!WEBM_MEDIA_TYPE.test(blob.type) || blob.size === 0) return blob;

  const ebml = await loadEbml();
  const decoder = new ebml.Decoder();
  const reader = new ebml.Reader();
  reader.logging = false;
  reader.drop_default_duration = false;

  const stream = blob.stream().getReader();
  for (;;) {
    const { done, value } = await stream.read();
    if (done) break;
    const chunk = value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
    for (const element of decoder.decode(chunk)) reader.read(element);
  }
  reader.stop();

  if (reader.metadataSize <= 0 || reader.duration <= 0 || reader.cues.length === 0) return blob;
  const metadata = ebml.tools.makeMetadataSeekable(reader.metadatas, reader.duration, reader.cues);
  return new Blob([metadata, blob.slice(reader.metadataSize)], { type: blob.type });
}
