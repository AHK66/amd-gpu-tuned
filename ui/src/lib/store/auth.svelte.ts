// Auth UI state (write-endpoint password auth). The bearer token itself
// lives in lib/api.ts (module var, never persisted); this store only
// mirrors what the UI renders.
export const authState = $state({ enabled: false, unlocked: false });
