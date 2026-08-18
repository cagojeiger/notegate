import ebmlScriptUrl from "ts-ebml/dist/EBML.min.js?url";

type EbmlModule = typeof import("ts-ebml");
type EbmlGlobal = typeof globalThis & { EBML?: EbmlModule };

const LOAD_TIMEOUT_MS = 15_000;
let loadPromise: Promise<EbmlModule> | null = null;

export async function loadEbml(): Promise<EbmlModule> {
  const browserGlobal = globalThis as EbmlGlobal;
  if (browserGlobal.EBML) return browserGlobal.EBML;
  loadPromise ??= new Promise<EbmlModule>((resolve, reject) => {
    const script = document.createElement("script");
    let settled = false;
    const fail = () => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timeout);
      loadPromise = null;
      script.remove();
      reject(new Error("WebM finalizer failed to load"));
    };
    const timeout = window.setTimeout(fail, LOAD_TIMEOUT_MS);
    script.src = ebmlScriptUrl;
    script.async = true;
    script.addEventListener("load", () => {
      if (!browserGlobal.EBML) {
        fail();
        return;
      }
      settled = true;
      window.clearTimeout(timeout);
      resolve(browserGlobal.EBML);
    }, { once: true });
    script.addEventListener("error", fail, { once: true });
    document.head.append(script);
  });

  return loadPromise;
}
