import { cleanup } from "@testing-library/svelte";
import { afterEach, expect } from "vitest";

class ResizeObserverPolyfill {
  private cb: ResizeObserverCallback;
  constructor(cb: ResizeObserverCallback) {
    this.cb = cb;
  }
  observe(el: Element) {
    this.cb(
      [{ contentRect: { width: 600, height: 160 } } as ResizeObserverEntry],
      this,
    );
  }
  unobserve() {}
  disconnect() {}
}
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = ResizeObserverPolyfill as unknown as typeof ResizeObserver;
}

// Unmount rendered components between tests so DOM does not leak across
// tests (required because vitest runs without `globals: true`, which is what
// @testing-library/svelte relies on for its built-in auto-cleanup).
afterEach(() => {
  cleanup();
});

// Minimal `toBeVisible` matcher so component tests need no extra
// dependencies (e.g. @testing-library/jest-dom).
expect.extend({
  toBeVisible(element: unknown) {
    const pass =
      element instanceof Element &&
      element.isConnected &&
      element.ownerDocument.defaultView?.getComputedStyle(element).display !== "none";
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}visible`,
    };
  },
  // Dep-free stand-in for jest-dom's `toHaveTextContent`.
  toHaveTextContent(element: unknown, text: string | RegExp) {
    const content = element instanceof Element ? element.textContent ?? "" : "";
    const pass = text instanceof RegExp ? text.test(content) : content.includes(text);
    return {
      pass,
      message: () =>
        `expected element text "${content}" to ${pass ? "not " : ""}contain ${text}`,
    };
  },
  // Dep-free stand-in for jest-dom's `toBeChecked`.
  toBeChecked(element: unknown) {
    const el = element instanceof HTMLInputElement ? element : null;
    const pass =
      el !== null && (el.type === "checkbox" || el.type === "radio") && el.checked;
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}checked`,
    };
  },
  // Dep-free stand-in for jest-dom's `toHaveValue`.
  toHaveValue(element: unknown, value: string | number) {
    const el = element instanceof HTMLInputElement ? element : null;
    const pass = el !== null && el.value === String(value);
    return {
      pass,
      message: () => `expected value to be ${pass ? "not " : ""}"${value}"`,
    };
  },
  // Dep-free stand-in for jest-dom's `toBeReadOnly`.
  toBeReadOnly(element: unknown) {
    const el = element instanceof HTMLInputElement ? element : null;
    const pass = el !== null && el.readOnly === true;
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}read-only`,
    };
  },
  // Dep-free stand-in for jest-dom's `toBeEditable`.
  toBeEditable(element: unknown) {
    const el = element instanceof HTMLInputElement ? element : null;
    const pass = el !== null && !el.readOnly && !el.disabled;
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}editable`,
    };
  },
  // Dep-free stand-in for jest-dom's `toBeEnabled`.
  toBeEnabled(element: unknown) {
    const el = element instanceof HTMLElement ? (element as HTMLInputElement) : null;
    const pass = el !== null && !el.disabled;
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}enabled`,
    };
  },
  // Dep-free stand-in for jest-dom's `toBeDisabled`.
  toBeDisabled(element: unknown) {
    const el = element instanceof HTMLElement ? (element as HTMLInputElement) : null;
    const pass = el !== null && el.disabled;
    return {
      pass,
      message: () => `expected element to be ${pass ? "not " : ""}disabled`,
    };
  },
  // Dep-free stand-in for jest-dom's `toHaveAttribute`.
  toHaveAttribute(element: unknown, name: string, value?: string) {
    const el = element instanceof Element ? element : null;
    const actual = el ? el.getAttribute(name) : null;
    const pass = value === undefined ? actual !== null : actual === value;
    return {
      pass,
      message: () =>
        value === undefined
          ? `expected element to ${pass ? "not " : ""}have attribute "${name}"`
          : `expected attribute "${name}" to be ${pass ? "not " : ""}"${value}", got "${actual}"`,
    };
  },
});
