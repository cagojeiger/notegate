export function installElementResizeMock(defaultWidth = 320) {
  const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  const originalResizeObserver = globalThis.ResizeObserver;
  const widths = new WeakMap<Element, number>();
  const callbacks = new Map<Element, ResizeObserverCallback[]>();

  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get() {
      return widths.get(this) ?? defaultWidth;
    }
  });

  globalThis.ResizeObserver = class {
    private readonly targets = new Set<Element>();

    constructor(private readonly callback: ResizeObserverCallback) {}

    observe(target: Element) {
      this.targets.add(target);
      callbacks.set(target, [...(callbacks.get(target) ?? []), this.callback]);
    }

    unobserve(target: Element) {
      this.targets.delete(target);
      callbacks.set(
        target,
        (callbacks.get(target) ?? []).filter((callback) => callback !== this.callback)
      );
    }

    disconnect() {
      for (const target of this.targets) this.unobserve(target);
    }
  } as typeof ResizeObserver;

  return {
    setWidth: (element: Element, width: number) => widths.set(element, width),
    trigger: (target: Element) => {
      for (const callback of callbacks.get(target) ?? []) {
        callback([], {} as ResizeObserver);
      }
    },
    restore: () => {
      if (originalClientWidth) {
        Object.defineProperty(HTMLElement.prototype, "clientWidth", originalClientWidth);
      } else {
        delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
      }
      if (originalResizeObserver) globalThis.ResizeObserver = originalResizeObserver;
      else delete (globalThis as unknown as { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
    }
  };
}
