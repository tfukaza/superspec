/**
 * Stable, descending order for finite float32 center depths. IEEE 754 bits become
 * unsigned radix keys by reversing the positive range and leaving the negative
 * range intact. Negative depths can occur because the eye's depth offset is
 * common to all dabs and can be omitted.
 */
export function createSplatDepthOrder(capacity) {
  const depths = new Float32Array(capacity);
  const keys = new Uint32Array(depths.buffer);
  const order = new Uint32Array(capacity), spare = new Uint32Array(capacity);
  const counts = new Uint32Array(4 * 256);
  function sort(active, count) {
    const sorted = order.subarray(0, count);
    sorted.set(active.subarray(0, count));
    counts.fill(0);
    for (let i = 0; i < count; i++) {
      const index = sorted[i];
      const bits = depths[index] === 0 ? 0 : keys[index];
      const key = (bits >>> 31) === 0 ? bits ^ 0x7fffffff : bits;
      keys[index] = key;
      counts[key & 255]++;
      counts[256 + ((key >>> 8) & 255)]++;
      counts[512 + ((key >>> 16) & 255)]++;
      counts[768 + (key >>> 24)]++;
    }
    let read = order, write = spare;
    for (let pass = 0; pass < 4; pass++) {
      const start = pass * 256, shift = pass * 8;
      let offset = 0;
      for (let digit = start; digit < start + 256; digit++) {
        const size = counts[digit];
        counts[digit] = offset;
        offset += size;
      }
      for (let i = 0; i < count; i++) {
        const index = read[i];
        const digit = start + ((keys[index] >>> shift) & 255);
        write[counts[digit]++] = index;
      }
      [read, write] = [write, read];
    }
    return sorted;
  }
  // Refill the active depths before each sort. Keys reuse the same storage.
  return { depths, sort };
}
