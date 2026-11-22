const saturate = (v) => Math.min(1, Math.max(0, v));

// Seeded periodic knots. Quintic interpolation matches slope and curvature
// at knot boundaries, including the tooling chart's -pi/pi seam.
function widthField(count, seed) {
  const knots = Array.from({ length: count }, () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  });
  return (turn) => {
    const point = (turn - Math.floor(turn)) * count;
    const i = Math.floor(point);
    const t = point - i;
    const blend = t * t * t * (t * (t * 6 - 15) + 10);
    return knots[i] + (knots[(i + 1) % count] - knots[i]) * blend;
  };
}

const innerWidth = widthField(13, 0x63a21);
const outerWidth = widthField(15, 0x15e49);
const glints = widthField(17, 0x7d065);

// A local illustrated reflection sheet on the oval tool chart. Lobes follow
// the curvature while short glints break across selected turning marks.
function reflection(u, v) {
  const r = Math.hypot(u, v);
  const a = Math.atan2(v, u);
  const drift = 0.1 * Math.sin(a * 3 + r * 38);
  const lobe = (direction, width) =>
    Math.exp((Math.cos(a - direction + drift) - 1) / width);
  const silver = lobe(2.35, 0.17);
  const blue = lobe(-0.82, 0.25);
  const champagne = lobe(0.45, 0.12);
  // The body's chart starts at .36, so only the machined lip reflects the
  // diffuser. Its inner horizontal land occupies .309–.324 on the bezel.
  const turn = a / (2 * Math.PI);
  const distance = r - 0.321;
  // Profile scale is about 0.20 m/UV. Independent widths feather the
  // glassward and housingward slopes across irregular 4–7 cm sectors.
  const width =
    distance < 0
      ? 0.0045 + 0.0125 * innerWidth(turn)
      : 0.004 + 0.0185 * outerWidth(turn);
  const ring = Math.exp(-((distance / width) ** 2));
  const shoulder = Math.exp(-((distance / (width * 1.65)) ** 2));
  const glint = 1.30 * glints(turn) ** 5 * (0.25 + silver + blue);
  const arc = 0.18 + 0.68 * silver + 0.56 * blue + 0.19 * champagne + glint;
  const graphite = 0.0018 + 0.004 * lobe(2.8, 0.35);
  // End the highlight inside the retaining bezel, before the separate outer
  // housing chart begins. Its wall should respond only to the moving sky.
  const edge = saturate((0.356 - r) / 0.011);
  const catchlight = (ring * arc + 0.035 * shoulder) * edge * edge * (3 - 2 * edge);
  return [
    graphite + catchlight * (0.91 + 0.09 * champagne),
    graphite + catchlight * 0.96,
    graphite + catchlight * (0.91 + 0.09 * blue),
  ].map((value) => value / 2.0);
}

export async function bakeBulkheadReflection(sharp, path, size = 384) {
  const rgb = new Float32Array(size * size * 3);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      rgb.set(
        reflection((x + 0.5) / size - 0.5, 0.5 - (y + 0.5) / size),
        (x + y * size) * 3,
      );
    }
  }
  const output = Buffer.alloc(size * size * 4, 255);
  const radius = 3;
  const count = (radius + 1) ** 2;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let bestVariance = Infinity;
      // Kuwahara smoothing in linear radiance preserves the edge of a bright
      // stroke when the adjacent quadrant contains both metal and highlight.
      for (const sy of [-1, 1]) {
        for (const sx of [-1, 1]) {
          let sum = 0,
            sumSquared = 0;
          const channels = [0, 0, 0];
          for (let j = 0; j <= radius; j++) {
            const row = Math.max(0, Math.min(size - 1, y + sy * j)) * size;
            for (let i = 0; i <= radius; i++) {
              const k = 3 * (row + Math.max(0, Math.min(size - 1, x + sx * i)));
              const luminance =
                rgb[k] * 0.2126 + rgb[k + 1] * 0.7152 + rgb[k + 2] * 0.0722;
              sum += luminance;
              sumSquared += luminance * luminance;
              for (let c = 0; c < 3; c++) channels[c] += rgb[k + c];
            }
          }
          const variance = count * sumSquared - sum * sum;
          if (variance >= bestVariance) continue;
          bestVariance = variance;
          for (let c = 0; c < 3; c++) {
            const linear = saturate(channels[c] / count);
            const srgb =
              linear < 0.0031308
                ? linear * 12.92
                : 1.055 * linear ** (1 / 2.4) - 0.055;
            output[(y * size + x) * 4 + c] = Math.round(srgb * 255);
          }
        }
      }
    }
  }
  await sharp(output, { raw: { width: size, height: size, channels: 4 } })
    .png()
    .toFile(path);
}
