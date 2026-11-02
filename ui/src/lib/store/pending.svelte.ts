export interface PendingTile {
  label: string;
  save: () => Promise<boolean>;
  discard: () => Promise<boolean>;
}

// Per-slot registry of tiles with unsaved preset changes. The Dashboard tiles
// register here while `modified` and clear on save/discard. A single leave-
// confirmation dialog acts on the whole set.
const tiles = $state<Record<string, PendingTile>>({});

export function registerTile(slot: string, tile: PendingTile): void {
  tiles[slot] = tile;
}

export function unregisterTile(slot: string): void {
  delete tiles[slot];
}

export function hasPending(): boolean {
  return Object.keys(tiles).length > 0;
}

export function pendingLabels(): string[] {
  return Object.values(tiles).map((t) => t.label);
}

// Resolves true only if every tile's action succeeded, so the caller can stay
// put (rather than navigate and strand a failed tile as "adhoc") on failure.
export async function saveAll(): Promise<boolean> {
  const results = await Promise.all(Object.values(tiles).map((t) => t.save()));
  return results.every(Boolean);
}

export async function discardAll(): Promise<boolean> {
  const results = await Promise.all(Object.values(tiles).map((t) => t.discard()));
  return results.every(Boolean);
}

export function resetForTests(): void {
  for (const k of Object.keys(tiles)) delete tiles[k];
}
