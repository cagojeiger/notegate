import pdfiumWasmUrl from "@embedpdf/pdfium/pdfium.wasm?url";
import { PDFViewer } from "@embedpdf/react-pdf-viewer";
import type {
  DocumentManagerPlugin,
  EmbedPdfContainer,
  PDFViewerConfig,
  PluginRegistry,
  ThemeColors
} from "@embedpdf/react-pdf-viewer";
import { useCallback, useEffect, useMemo, useRef } from "react";

import { useUiStore } from "../../stores/uiStore";
import { observeEmbedPdfAccessibility } from "./embedPdfAccessibility";

const absolutePdfiumWasmUrl = new URL(pdfiumWasmUrl, window.location.origin).href;

const NOTE_GATE_THEME: Partial<ThemeColors> = {
  background: {
    app: "var(--ng-editor)",
    surface: "var(--ng-surface)",
    surfaceAlt: "var(--ng-panel)",
    elevated: "var(--ng-surface)",
    overlay: "color-mix(in srgb, var(--ng-text) 48%, transparent)",
    input: "var(--ng-surface)"
  },
  foreground: {
    primary: "var(--ng-text)",
    secondary: "var(--ng-muted)",
    muted: "var(--ng-faint)",
    disabled: "var(--ng-faint)",
    onAccent: "var(--ng-primary-contrast)"
  },
  border: {
    default: "var(--ng-border)",
    subtle: "var(--ng-seam)",
    strong: "var(--ng-border-strong)"
  },
  accent: {
    primary: "var(--ng-primary)",
    primaryHover: "var(--ng-primary-hover)",
    primaryActive: "var(--ng-primary-hover)",
    primaryLight: "var(--ng-selection)",
    primaryForeground: "var(--ng-primary-contrast)"
  },
  interactive: {
    hover: "var(--ng-hover)",
    active: "var(--ng-active-surface)",
    selected: "var(--ng-selection)",
    focus: "var(--ng-focus-ring)",
    focusRing: "var(--ng-focus-ring)"
  }
};

export function PdfPreview({
  name,
  onError,
  url
}: {
  name: string;
  onError: () => void;
  url: string;
}) {
  const theme = useUiStore((state) => state.theme);
  const accessibilityCleanupRef = useRef<(() => void) | null>(null);
  const containerRef = useRef<EmbedPdfContainer | null>(null);
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const config = useMemo<PDFViewerConfig>(() => ({
    src: url,
    wasmUrl: absolutePdfiumWasmUrl,
    tabBar: "never",
    disabledCategories: [
      "annotation",
      "redaction",
      "insert",
      "history",
      "document-open",
      "document-protect",
      "panel-comment",
      "security"
    ],
    export: { defaultFileName: name },
    fontFallback: null,
    fonts: { ui: null, signature: null },
    stamp: { defaultLibrary: false, manifests: [] },
    theme: {
      preference: theme,
      light: NOTE_GATE_THEME,
      dark: NOTE_GATE_THEME
    }
  }), [name, theme, url]);

  const handleReady = useCallback((registry: PluginRegistry) => {
    unsubscribeRef.current?.();
    const documentManager = registry.getPlugin<DocumentManagerPlugin>("document-manager");
    unsubscribeRef.current = documentManager?.provides().onDocumentError(onError) ?? null;
  }, [onError]);

  const handleInit = useCallback((container: EmbedPdfContainer) => {
    accessibilityCleanupRef.current?.();
    containerRef.current = container;
    const root = container.shadowRoot;
    if (!root) return;
    accessibilityCleanupRef.current = observeEmbedPdfAccessibility(root);
  }, []);

  useEffect(() => {
    containerRef.current?.setTheme(theme);
  }, [theme]);

  useEffect(() => () => {
    accessibilityCleanupRef.current?.();
    accessibilityCleanupRef.current = null;
    containerRef.current = null;
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
  }, []);

  return (
    <section
      aria-label={`PDF preview: ${name}`}
      className="mt-8 h-[70vh] min-h-96 w-full overflow-hidden rounded-xl border border-border bg-surface"
      data-pdf-preview
    >
      <PDFViewer
        className="h-full w-full"
        config={config}
        onInit={handleInit}
        onReady={handleReady}
        style={{ height: "100%", width: "100%" }}
      />
    </section>
  );
}
