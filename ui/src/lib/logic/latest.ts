/**
 * Builds a "latest-only" async setter. Each call to the returned function
 * starts a fetch via `load` and forwards its result to `apply` only if no
 * later call has started in the meantime. This drops a stale response that
 * resolves after a newer fetch, so a slow, older read (e.g. a periodic poll
 * racing a post-mutation refresh) can never overwrite fresher state.
 */
export function makeLatestOnly<A>(
  apply: (value: A) => void,
): (load: () => Promise<A>) => Promise<void> {
  let seq = 0;
  return (load) => {
    const s = ++seq;
    return load().then(
      (value) => {
        if (s === seq) apply(value);
      },
      () => {
        // A stale failure must not clobber newer state.
      },
    );
  };
}
