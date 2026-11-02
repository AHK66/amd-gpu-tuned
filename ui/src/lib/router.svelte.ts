const ROUTES = ["", "presets", "drift", "setup"] as const;
export type Route = (typeof ROUTES)[number];

export function parseRoute(hash: string): Route {
  const h = hash.replace(/^#\/?/, "").split("?")[0];
  return (ROUTES as readonly string[]).includes(h) ? (h as Route) : "";
}

let current = $state<Route>(
  typeof location !== "undefined" ? parseRoute(location.hash) : "",
);

type NavGuard = (target: Route) => boolean;

let guard: NavGuard | null = null;
let onBlocked: ((target: Route) => void) | null = null;
let blockedTarget: Route | null = null;

export function setNavGuard(g: NavGuard | null, onBlockedCb?: (target: Route) => void): void {
  guard = g;
  onBlocked = onBlockedCb ?? null;
}

// Returns true if navigation may proceed. When an active guard vetoes a change
// of route, it records the target and notifies the caller (to show the
// confirm dialog) without touching the hash.
function attempt(target: Route): boolean {
  if (target !== current && guard != null && guard(target)) {
    blockedTarget = target;
    onBlocked?.(target);
    return false;
  }
  return true;
}

export function navigate(r: Route): void {
  if (!attempt(r)) return;
  location.hash = `#/${r}`;
}

// Resume a navigation that the guard blocked (after saving/discarding the
// pending changes). Bypasses the guard — the tiles are clean by then.
export function proceed(r: Route): void {
  blockedTarget = null;
  location.hash = `#/${r}`;
}

export function blockedRoute(): Route | null {
  return blockedTarget;
}

// Test-only: clear guard state and resync `current` from the hash.
export function resetForTests(): void {
  guard = null;
  onBlocked = null;
  blockedTarget = null;
  current = parseRoute(typeof location !== "undefined" ? location.hash : "");
}

// Plain listener (no $effect): mutating module $state is reactive everywhere
// it is read, and this keeps the module side-effect-free of Svelte lifecycle.
if (typeof window !== "undefined") {
  window.addEventListener("hashchange", () => {
    current = parseRoute(window.location.hash);
  });
  // Sidebar links are plain <a href="#/…">. Intercept their clicks in the
  // capture phase so the guard runs before the hash changes — a blocked nav
  // adds no history entry.
  window.addEventListener(
    "click",
    (e) => {
      const a = (e.target as Element | null)?.closest?.('a[href^="#/"]');
      if (!a) return;
      if (attempt(parseRoute(a.getAttribute("href") ?? ""))) return;
      e.preventDefault();
    },
    true,
  );
}

export function route(): Route {
  return current;
}
