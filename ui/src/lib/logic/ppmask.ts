/** Count the set bits (popcount) in a 32-bit PP feature mask. */
export function enabledFeatureCount(mask: number): number {
  let count = 0;
  let x = mask >>> 0;
  while (x) {
    x &= x - 1;
    count++;
  }
  return count;
}
