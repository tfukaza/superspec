/** Hash the raw GPU words. The two lanes rotate between words (xxHash32 and Murmur3
 * block rounds). Word-wise FNV loses *every* even number of isolated IEEE sign changes,
 * including a mirrored pair of positions or normals, regardless of the initial seed. */
export class LayoutDigest {
  private a = 0x165667b1;
  private b = 0x9747b28c;
  private length = 0;

  add(word: number): void {
    const summed = this.a + Math.imul(word, 0x85ebca77) | 0;
    this.a = Math.imul((summed << 13) | (summed >>> 19), 0x9e3779b1);
    const product = Math.imul(word, 0xcc9e2d51);
    const mixed = this.b ^ Math.imul((product << 15) | (product >>> 17), 0x1b873593);
    this.b = Math.imul((mixed << 13) | (mixed >>> 19), 5) + 0xe6546b64 | 0;
    this.length++;
  }

  key(): string { return `${this.length}:${this.a >>> 0}:${this.b >>> 0}`; }
}

export function hashFloats(values: Float32Array | Float64Array): string {
  const bits = new Uint32Array(values.buffer, values.byteOffset, values.byteLength / Uint32Array.BYTES_PER_ELEMENT);
  const digest = new LayoutDigest();
  for (let index = 0; index < bits.length; index++) digest.add(bits[index]);
  return digest.key();
}
