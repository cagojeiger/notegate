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
    recorder.addEventListener("dataavailable", (event) => {
      if (event.data.size > 0) chunks.push(event.data);
    });
    const stopped = new Promise<void>((resolve) => {
      recorder.addEventListener("stop", () => resolve(), { once: true });
    });
    oscillator.start();
    recorder.start(100);
    await new Promise((resolve) => setTimeout(resolve, 450));
    recorder.stop();
    oscillator.stop();
    await stopped;
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
      audio.currentTime = Math.min(0.1, audio.duration / 2);
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error("audio seek timed out")), 10_000);
        audio.addEventListener("seeked", () => {
          clearTimeout(timeout);
          resolve();
        }, { once: true });
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
  expect(result.duration).toBeLessThan(2);
  expect(result.seekableEnd).toBeCloseTo(result.duration, 2);
  expect(result.currentTime).toBeGreaterThan(0);
});
