import { expect, test } from "@playwright/test";

test("a browser recording is finalized into seekable WebM audio", async ({ page }) => {
  await page.route("**/__audio-finalization-test__", (route) => route.fulfill({
    status: 200,
    contentType: "text/html",
    body: "<!doctype html><title>Audio finalization test</title>"
  }));
  await page.goto("/__audio-finalization-test__");

  const result = await page.evaluate(async () => {
    const moduleUrl = `${location.origin}/src/features/recording/finalizeRecordedAudio.ts`;
    const { finalizeRecordedAudio } = await import(/* @vite-ignore */ moduleUrl) as {
      finalizeRecordedAudio: (blob: Blob) => Promise<Blob>;
    };
    const { loadEbml } = await import(/* @vite-ignore */ `${location.origin}/src/features/recording/ebmlBrowser.ts`) as typeof import("../src/features/recording/ebmlBrowser");
    const ebml = await loadEbml();
    const decoder = new ebml.Decoder();
    const reader = new ebml.Reader();
    reader.logging = false;
    reader.drop_default_duration = false;
    const context = new AudioContext({ sampleRate: 48_000 });
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    const destination = context.createMediaStreamDestination();
    gain.gain.value = 0.03;
    oscillator.frequency.value = 440;
    oscillator.connect(gain).connect(destination);

    const recorder = new MediaRecorder(destination.stream, {
      mimeType: "audio/webm;codecs=opus",
      audioBitsPerSecond: 64_000
    });
    const chunks: Blob[] = [];
    let processing = Promise.resolve();
    const usableRecording = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("audio recording timed out")), 10_000);
      recorder.addEventListener("dataavailable", (event) => {
        if (event.data.size === 0) return;
        chunks.push(event.data);
        processing = processing.then(async () => {
          for (const element of decoder.decode(await event.data.arrayBuffer())) reader.read(element);
          // The first nonempty chunk can still have zero media duration.
          // Stop only after the encoded stream contains usable audio metadata.
          if (reader.metadataSize > 0 && reader.duration > 0 && reader.cues.length > 0) {
            clearTimeout(timeout);
            resolve();
          }
        }).catch((error) => {
          clearTimeout(timeout);
          reject(error);
        });
      });
      recorder.addEventListener("error", () => {
        clearTimeout(timeout);
        reject(new Error("audio recording failed"));
      }, { once: true });
    });
    const stopped = new Promise<void>((resolve) => {
      recorder.addEventListener("stop", () => resolve(), { once: true });
    });
    oscillator.start();
    recorder.start(100);
    await usableRecording;
    recorder.stop();
    oscillator.stop();
    await stopped;
    await processing;
    reader.stop();
    await context.close();

    const raw = new Blob(chunks, { type: recorder.mimeType });
    const finalized = await finalizeRecordedAudio(raw);
    const audio = new Audio(URL.createObjectURL(finalized));
    try {
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error("audio metadata timed out")), 10_000);
        audio.addEventListener("loadedmetadata", () => {
          clearTimeout(timeout);
          resolve();
        }, { once: true });
        audio.addEventListener("error", () => {
          clearTimeout(timeout);
          reject(new Error(`audio failed with media error ${audio.error?.code ?? "unknown"}`));
        }, { once: true });
      });
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error("audio seek timed out")), 10_000);
        audio.addEventListener("seeked", () => {
          clearTimeout(timeout);
          resolve();
        }, { once: true });
        audio.currentTime = Math.min(0.1, audio.duration / 2);
      });
      return {
        currentTime: audio.currentTime,
        duration: audio.duration,
        finalizedSize: finalized.size,
        rawSize: raw.size,
        seekableEnd: audio.seekable.length > 0 ? audio.seekable.end(0) : 0
      };
    } finally {
      URL.revokeObjectURL(audio.src);
    }
  });

  expect(result.finalizedSize).toBeGreaterThan(result.rawSize);
  expect(result.duration).toBeGreaterThan(0);
  expect(Number.isFinite(result.duration)).toBe(true);
  expect(result.seekableEnd).toBeCloseTo(result.duration, 2);
  expect(result.currentTime).toBeGreaterThan(0);
});
