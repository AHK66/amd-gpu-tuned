import { SvelteMap } from "svelte/reactivity";

interface Item {
  id: number;
  tone: "success" | "error" | "info";
  msg: string;
}

export const toasts = $state<Item[]>([]);
let seq = 0;
const timers = new SvelteMap<number, ReturnType<typeof setTimeout>>();

function scheduleDismiss(id: number) {
  clearTimeout(timers.get(id));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      dismiss(id);
    }, 4000)
  );
}

function push(tone: Item["tone"], msg: string) {
  const existing = toasts.find((t) => t.tone === tone && t.msg === msg);
  if (existing) {
    scheduleDismiss(existing.id);
    return;
  }
  const id = ++seq;
  toasts.push({ id, tone, msg });
  scheduleDismiss(id);
}

export function dismiss(id: number) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
  const timer = timers.get(id);
  if (timer !== undefined) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

export const toast = {
  success: (msg: string) => push("success", msg),
  error: (msg: string) => push("error", msg),
  info: (msg: string) => push("info", msg),
};

export function _resetToastsForTests() {
  toasts.length = 0;
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
}
