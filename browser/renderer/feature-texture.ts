import * as THREE from "three";

// Which GLSL crease selectors can read each geometric kind (ms_selected). Named kind k has selector k + 3.
const GEOMETRIC_SELECTORS = [[0, 3, 4], [1, 4], [2, 3, 4], [5], [6]];

/** Three texels per segment, followed by an optional [first, end) row range for each selector.
 * Preserve the order of all geometric segments and of each named feature's segments: ties in
 * the nearest normal and tangent keep the first match. No selector mixes named and geometric kinds. */
export function featureTexture(features: number[], maxTextureSize = 2048): THREE.DataTexture {
  const count = features.length / 12;
  let rows = Math.max(1, count), data: Float32Array;
  let selectors = 0;
  // A full scan of a handful of rows is cheaper than a dependent texture fetch.
  if (count >= 8) {
    selectors = 8;
    for (let i = 3; i < features.length; i += 12) {
      const kind = Math.fround(features[i]);
      if (!Number.isSafeInteger(kind) || kind < 0) { selectors = 0; break; }
      selectors = Math.max(selectors, kind + 4);
    }
    // Keep the texture usable on every WebGL2 device that could hold its original feature rows.
    if (count + Math.ceil(selectors / 3) > maxTextureSize) selectors = 0;
  }
  if (selectors) {
    rows += Math.ceil(selectors / 3);
    data = new Float32Array(rows * 12);
    const ordered = Array.from({ length: count }, (_, i) => i);
    const kindOf = (row: number) => Math.fround(features[row * 12 + 3]);
    ordered.sort((a, b) => Math.max(4, kindOf(a)) - Math.max(4, kindOf(b)));
    for (const [row, source] of ordered.entries()) {
      for (let j = 0; j < 12; j++) data[row * 12 + j] = features[source * 12 + j];
      const kind = kindOf(source);
      for (const selector of GEOMETRIC_SELECTORS[kind] ?? [kind + 3]) {
        const at = count * 12 + selector * 4;
        if (data[at + 1] === 0) data[at] = row;
        data[at + 1] = row + 1;
      }
    }
  } else {
    data = new Float32Array(rows * 12);
    data.set(features);
  }
  const texture = new THREE.DataTexture(data, 3, rows, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

export function featureUniforms(texture: THREE.DataTexture, count: number): Record<string, THREE.IUniform> {
  // Any unused entries at the end of the final row are zero-length ranges.
  return { ms_features: { value: texture }, ms_feature_count: { value: count },
    ms_feature_selectors: { value: (texture.image.height - Math.max(1, count)) * 3 } };
}
