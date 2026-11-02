// Module-level $state is read (never exported) via isOpen(), mirroring the
// `route()` pattern in lib/router.svelte.ts.
let openId = $state<string | null>(null);

export function openModal(id: string): void {
  openId = id;
}

export function closeModal(): void {
  openId = null;
}

export function isOpen(id: string): boolean {
  return openId === id;
}
