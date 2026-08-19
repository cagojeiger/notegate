import { useEffect, useRef, useState } from "react";

import { finalizeRecordedAudio } from "../recording/finalizeRecordedAudio";

const WEBM_MEDIA_TYPE = /^audio\/webm(?:;|$)/i;
const LEGACY_AUDIO_REPAIR_MAX_BYTES = 64 * 1024 * 1024;

type RepairStatus = "idle" | "repairing" | "failed";
type RepairPhase = "idle" | "repairing" | "repaired" | "failed";

export function AudioPreview({
  url,
  name,
  mediaType,
  byteLen,
  onError
}: {
  url: string;
  name: string;
  mediaType: string;
  byteLen?: number;
  onError: () => void;
}) {
  const [playbackUrl, setPlaybackUrl] = useState(url);
  const [repairStatus, setRepairStatus] = useState<RepairStatus>("idle");
  const activeRef = useRef(true);
  const repairAttemptedRef = useRef(false);
  const repairPhaseRef = useRef<RepairPhase>("idle");
  const reportFailureRef = useRef(false);
  const failureReportedRef = useRef(false);
  const controllerRef = useRef<AbortController | null>(null);
  const objectUrlRef = useRef<string | null>(null);
  const onErrorRef = useRef(onError);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
      controllerRef.current?.abort();
      if (objectUrlRef.current) URL.revokeObjectURL(objectUrlRef.current);
    };
  }, []);

  function reportErrorOnce() {
    if (failureReportedRef.current) return;
    failureReportedRef.current = true;
    onErrorRef.current();
  }

  function prepareLegacyWebm(reportFailure: boolean) {
    if (reportFailure) reportFailureRef.current = true;
    if (!canRepairLegacyWebm(mediaType, byteLen)) {
      if (reportFailure) reportErrorOnce();
      return;
    }
    if (repairAttemptedRef.current) {
      if (reportFailure && repairPhaseRef.current !== "repairing") reportErrorOnce();
      return;
    }

    repairAttemptedRef.current = true;
    repairPhaseRef.current = "repairing";
    setRepairStatus("repairing");
    const controller = new AbortController();
    controllerRef.current = controller;

    void fetchAndFinalizeWebm(url, mediaType, controller.signal)
      .then((finalized) => {
        if (!activeRef.current || controller.signal.aborted) return;
        const objectUrl = URL.createObjectURL(finalized);
        objectUrlRef.current = objectUrl;
        repairPhaseRef.current = "repaired";
        setPlaybackUrl(objectUrl);
        setRepairStatus("idle");
      })
      .catch(() => {
        if (!activeRef.current || controller.signal.aborted) return;
        repairPhaseRef.current = "failed";
        setRepairStatus("failed");
        if (reportFailureRef.current) reportErrorOnce();
      });
  }

  return (
    <div aria-busy={repairStatus === "repairing"}>
      <audio
        className="w-full"
        src={playbackUrl}
        controls
        preload="metadata"
        aria-label={`Play ${name}`}
        onLoadedMetadata={(event) => {
          const duration = event.currentTarget.duration;
          if (!Number.isFinite(duration) || duration <= 0) prepareLegacyWebm(false);
        }}
        onError={() => prepareLegacyWebm(true)}
      />
      {repairStatus === "repairing" ? (
        <p className="mt-3 text-sm text-muted" role="status">Preparing older recording…</p>
      ) : null}
      {repairStatus === "failed" ? (
        <p className="mt-3 text-sm text-muted">This older recording could not be prepared. Download it to play locally.</p>
      ) : null}
    </div>
  );
}

function canRepairLegacyWebm(mediaType: string, byteLen: number | undefined): boolean {
  return WEBM_MEDIA_TYPE.test(mediaType)
    && byteLen !== undefined
    && byteLen > 0
    && byteLen <= LEGACY_AUDIO_REPAIR_MAX_BYTES;
}

async function fetchAndFinalizeWebm(
  url: string,
  mediaType: string,
  signal: AbortSignal
): Promise<Blob> {
  const response = await fetch(url, {
    cache: "no-store",
    credentials: "omit",
    referrerPolicy: "no-referrer",
    signal
  });
  if (!response.ok) throw new Error("Audio preview request failed");

  const contentLength = Number(response.headers.get("content-length"));
  if (Number.isFinite(contentLength) && contentLength > LEGACY_AUDIO_REPAIR_MAX_BYTES) {
    throw new Error("Audio preview is too large to repair");
  }

  const downloaded = await response.blob();
  if (downloaded.size > LEGACY_AUDIO_REPAIR_MAX_BYTES) {
    throw new Error("Audio preview is too large to repair");
  }
  if (signal.aborted) throw new DOMException("Audio preview canceled", "AbortError");

  const source = WEBM_MEDIA_TYPE.test(downloaded.type)
    ? downloaded
    : new Blob([downloaded], { type: mediaType });
  const finalized = await finalizeRecordedAudio(source);
  if (signal.aborted) throw new DOMException("Audio preview canceled", "AbortError");
  if (finalized === source) throw new Error("Audio preview metadata could not be repaired");
  return finalized;
}
