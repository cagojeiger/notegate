import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
  type Ref,
  type RefObject
} from "react";

import {
  classifyMarkdownLink,
  type MarkdownImagePolicy
} from "../../shared/lib/markdownLinks";

type MarkdownImageState =
  | { status: "idle" }
  | { status: "loading"; path: string }
  | { status: "loaded"; path: string; url: string }
  | { status: "not-found" | "unsupported" | "error"; path: string };

export function MarkdownImage({
  src,
  alt,
  imagePolicy,
  viewportRoot,
  ...props
}: ComponentProps<"img"> & {
  imagePolicy?: MarkdownImagePolicy;
  viewportRoot?: RefObject<Element | null>;
}) {
  const [state, setState] = useState<MarkdownImageState>({ status: "idle" });
  const [nearViewportPath, setNearViewportPath] = useState<string | null>(null);
  const [retriedPath, setRetriedPath] = useState<string | null>(null);
  const placeholderRef = useRef<HTMLSpanElement | null>(null);
  const imageIntent = useMemo(() => {
    if (!src) return { kind: "invalid" as const };
    return classifyMarkdownLink(imagePolicy?.sourcePath ?? "/", src);
  }, [imagePolicy, src]);

  useEffect(() => {
    if (imageIntent.kind !== "internal" || !imagePolicy) return;

    const path = imageIntent.path;
    const placeholder = placeholderRef.current;
    if (!placeholder || typeof IntersectionObserver === "undefined") {
      setNearViewportPath(path);
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        setNearViewportPath(path);
        observer.disconnect();
      },
      { root: viewportRoot?.current ?? null, rootMargin: "600px 0px" }
    );
    observer.observe(placeholder);
    return () => observer.disconnect();
  }, [imageIntent, imagePolicy, viewportRoot]);

  useEffect(() => {
    if (
      imageIntent.kind !== "internal"
      || !imagePolicy
      || nearViewportPath !== imageIntent.path
    ) return;

    let active = true;
    const path = imageIntent.path;
    setState({ status: "loading", path });

    const load = retriedPath === path
      ? imagePolicy.loadInternalImage(path, { forceRefresh: true })
      : imagePolicy.loadInternalImage(path);
    void load
      .then((result) => {
        if (!active) return;
        if (result.status === "loaded") {
          setState({ status: "loaded", path, url: result.url });
          return;
        }
        setState({ status: result.status, path });
      })
      .catch(() => {
        if (active) setState({ status: "error", path });
      });

    return () => {
      active = false;
    };
  }, [imageIntent, imagePolicy, nearViewportPath, retriedPath]);

  if (!src) return <ImageFallback alt={alt} message="Image unavailable" />;
  if (imageIntent.kind === "external") {
    return <ExternalMarkdownImage key={src} {...props} src={src} alt={alt} />;
  }
  if (imageIntent.kind === "invalid") {
    return <ImageFallback alt={alt} message="Invalid image link" />;
  }
  if (!imagePolicy) return <ImageFallback alt={alt} message="Image unavailable" />;
  if (state.status === "loaded" && state.path === imageIntent.path) {
    return (
      <img
        {...props}
        src={state.url}
        alt={alt ?? ""}
        loading="lazy"
        decoding="async"
        onError={() => {
          if (retriedPath === imageIntent.path) {
            setState({ status: "error", path: imageIntent.path });
            return;
          }
          setRetriedPath(imageIntent.path);
        }}
      />
    );
  }
  if (state.status === "not-found" && state.path === imageIntent.path) {
    return <ImageFallback alt={alt} message="Image not found" />;
  }
  if (state.status === "unsupported" && state.path === imageIntent.path) {
    return <ImageFallback alt={alt} message="Image cannot be displayed" />;
  }
  if (state.status === "error" && state.path === imageIntent.path) {
    return <ImageFallback alt={alt} message="Could not load image" />;
  }
  return <ImageFallback alt={alt} message="Loading image..." containerRef={placeholderRef} />;
}

function ExternalMarkdownImage({ src, alt, ...props }: ComponentProps<"img">) {
  const [shouldLoad, setShouldLoad] = useState(false);
  const [failed, setFailed] = useState(false);

  if (!shouldLoad) {
    const label = alt ? `Load external image: ${alt}` : "Load external image";
    return (
      <button
        type="button"
        className="markdown-image-fallback"
        onClick={() => setShouldLoad(true)}
      >
        {label}
      </button>
    );
  }
  if (failed) return <ImageFallback alt={alt} message="Could not load external image" />;
  return (
    <img
      {...props}
      src={src}
      alt={alt ?? ""}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
}

function ImageFallback({
  alt,
  message,
  containerRef
}: {
  alt?: string;
  message: string;
  containerRef?: Ref<HTMLSpanElement>;
}) {
  return (
    <span ref={containerRef} className="markdown-image-fallback">
      {alt ? `${message}: ${alt}` : message}
    </span>
  );
}
