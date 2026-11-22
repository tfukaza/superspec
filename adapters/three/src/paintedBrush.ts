// Shared by the diffuse probe wash and painted cast-shadow penumbra.
export const paintBrushField = /* glsl */ `
#ifndef CITY_BRUSH_FIELD
#define CITY_BRUSH_FIELD
float cityBrushPigment(vec2 meters, float strokeMeters, float footprint) {
  // The existing footprint filter fully suppresses these subpixel strokes.
  if (footprint >= .75) return 0.;
  // Stagger adjacent rows of overlapping chisel strokes on the receiving plane.
  vec2 dabSize = strokeMeters * vec2(1., .34) * .82;
  vec2 paper = meters/dabSize;
  paper.y += 0.36*paper.x + 0.53*sin(paper.x*0.86+paper.y*0.17);
  paper.x += 0.27*sin(1.43*paper.y+0.35*paper.x);
  float paintSum = 0., weightSum = 0.;
  float firstRow = floor(paper.y - .5);
  for (int j=0; j<2; ++j) {
    float y = firstRow + float(j);
    float column = paper.x + y*.618;
    float firstColumn = floor(column - .5);
    for (int i=0; i<2; ++i) {
      vec2 id = vec2(firstColumn + float(i), y);
      vec3 seed = fract(id.xyx*vec3(.1031,.1030,.0973));
      seed += dot(seed, seed.yzx+33.33);
      float variation = fract((seed.x+seed.y)*seed.z);
      vec2 dab = vec2(column,paper.y) - (id + .5);
      float bristles = .025*sin(dab.y*22.+variation*5.);
      float end = abs(dab.x + dab.y*(variation-.5)*.22 + bristles);
      float side = abs(dab.y + .025*sin(dab.x*6.+variation*4.));
      float weight = 1.-smoothstep(.42,.66,max(end,side));
      weightSum += weight;
      paintSum += weight*(variation-.5);
    }
  }
  return .60*paintSum/max(weightSum,.001)*(1.-smoothstep(.16,.75,footprint));
}
#endif
`;

const fract = (v: number) => v - Math.floor(v);

// CPU equivalent of cityBrushPigment, for baking projected light textures.
export function brushPigmentAt(u: number, v: number, strokeMeters: number): number {
  let x = u / (0.82 * strokeMeters), y = v / (0.34 * 0.82 * strokeMeters);
  y += 0.36 * x + 0.53 * Math.sin(x * 0.86 + y * 0.17);
  x += 0.27 * Math.sin(1.43 * y + 0.35 * x);
  let paint = 0, weights = 0;
  const firstRow = Math.floor(y - 0.5);
  for (let row = firstRow; row < firstRow + 2; row++) {
    const column = x + row * 0.618;
    const firstColumn = Math.floor(column - 0.5);
    for (let id = firstColumn; id < firstColumn + 2; id++) {
      const a = fract(id * 0.1031), b = fract(row * 0.1030), c = fract(id * 0.0973);
      const dot = a * (b + 33.33) + b * (c + 33.33) + c * (a + 33.33);
      const variation = fract((a + b + 2 * dot) * (c + dot));
      const dx = column - (id + 0.5), dy = y - (row + 0.5);
      const end = Math.abs(dx + dy * (variation - 0.5) * 0.22 + 0.025 * Math.sin(dy * 22 + variation * 5));
      const side = Math.abs(dy + 0.025 * Math.sin(dx * 6 + variation * 4));
      const t = Math.max(0, Math.min(1, (Math.max(end, side) - 0.42) / 0.24));
      const weight = 1 - t * t * (3 - 2 * t);
      paint += weight * (variation - 0.5);
      weights += weight;
    }
  }
  return 0.6 * paint / Math.max(weights, 0.001);
}
