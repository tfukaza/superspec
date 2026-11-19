import * as THREE from "three";
import type { FoliageMass } from "../model/packet.ts";
import { densityField, densitySurface, shadeNormals } from "./leaf-density.ts";
import { leafNormalMaterial, leafNormalTexture, smoothGeometry } from "./leaf-normals.ts";
import { contourFuzz } from "./contour-fuzz.ts";
import { branchCardTexture, densityCards } from "./density-cards.ts";
import { leafCardTextures } from "./foliage-plants.ts";
import { followPaintedFinishing, programSun } from "./shade.ts";

/**
 * The mass's palette (`FoliageMass.palette`) applied to every painted colour: turned about the grey axis by x radians,
 * its saturation scaled by y and its value by z; the darker colours (fading out by the light ones) also scaled by
 * `msGradeShade` and turned by w radians.
 */
const GRADE_GLSL = /* glsl */`
uniform vec4 msGrade;
uniform float msGradeShade;
vec3 msGradeTurn(vec3 c, float a) {
  const vec3 k = vec3(0.57735027);
  return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}
vec3 msGradeColour(vec3 c) {
  float luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
  float dark = 1.0 - smoothstep(0.12, 0.45, luma);
  c = msGradeTurn(c, msGrade.x + msGrade.w * dark);
  float grey = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = mix(vec3(grey), c, msGrade.y) * msGrade.z * mix(1.0, msGradeShade, dark);
  return max(c, vec3(0.0));
}
`;

/**
 * A plant painted as one mass (`FoliageMass`; docs/plans/tree-item-roadmap.md, milestone `painted-trees-milestone`):
 * the leaf density field of its clusters (leaf-density.ts) cut at `level` into a painted core (leaf-normals.ts) with
 * leafy fuzz on its outline (contour-fuzz.ts), and painted billboards through the thinner foliage around it
 * (density-cards.ts): round family cards where it is dense and sprays rooted on its twigs where it is sparse. Every part
 * is lit by the plant's broad form (a field smoothed wider), the whole plant as one blob with a darker shade side and
 * darker clumps deep inside, and banded with the scene's sun (`programSun`).
 *
 * The settings below are the milestone's (`tests/painted-mass-shrub-browser.mjs` with `BASE=density BRANCHES=1 STRIPS=0
 * TILE=1.0 SIZE=0.6 DENSITY=5 COARSE=0.3 STRENGTH=1.2`, the rest at that script's defaults); a mass states its field,
 * core level and card share.
 */
export const PAINTED_MASS = {
  /** The leaf map: meters a repeat, how far it tips the normals, the bands' shift, its brush (leaves a clump). */
  tile: 1.0, strength: 1.2, offset: -0.05, brush: 16,
  /** The core's outline fuzz: card meters, cards a meter of edge, reach past the edge, meters the edges are coarsened by. */
  fuzz: { size: 0.6, density: 5, reach: 0.08, coarse: 0.3 },
  /** Cards through the thinner foliage. */
  cards: { size: 0.55, spacing: 0.18, floor: 0.1, facing: 0.75, branchSize: 0.75, branchKeep: 0.9, roundSize: 0.7, roundKeep: 0.6,
    denseFacing: 1, denseDetail: 0.8, sprayScale: 0.75, tuck: 0.3 },
  /** Fields smoothed this many times wider: the card tiers', and the light's (its own bumpier light kept by `shadeMix`). */
  tierSpread: 1.6, shadeSpread: 2, shadeMix: 0.55, shadeJitter: 0.45,
  /** Light: highlight leaves, the blob's lit and shade sides, the bands' contrast, the darkening deep inside. */
  highlight: 1, macro: 0.12, macroShade: 0.3, contrast: 1.25, occlusion: 0.3,
} as const;

/**
 * The painted parts of `mass` placed by `placement` (the batch's instance matrix): its core, fuzz and cards, named
 * `painted/surface`, `painted/fuzz` and `painted/cards`, in the space the placement puts them in.
 */
export function paintedMass(mass: FoliageMass, placement: THREE.Matrix4): THREE.Mesh[] {
  const s = PAINTED_MASS;
  const scale = placement.getMaxScaleOnAxis();
  const p = new THREE.Vector3();
  const sites = new Float32Array(mass.sites.length * 4);
  mass.sites.forEach(([x, y, z, size], i) => { p.set(x, y, z).applyMatrix4(placement).toArray(sites, i * 4); sites[i * 4 + 3] = size * scale; });
  const twigs = mass.twigs.map(twig => twig.flatMap(([x, y, z]) => p.set(x, y, z).applyMatrix4(placement).toArray()));
  const field = densityField(sites, mass.radius, mass.cell);
  const geometry = densitySurface(field, mass.level);
  const shadeField = densityField(sites, mass.radius * s.shadeSpread, mass.cell);
  // The mass's form and jitter, or the milestone's.
  const form = mass.form ?? s.shadeMix, jitter = mass.jitter ?? s.shadeJitter;
  shadeNormals(geometry, shadeField, form);
  geometry.computeBoundingBox();
  const centre = geometry.boundingBox!.getCenter(new THREE.Vector3());
  const sun = programSun.value.clone();
  const leaf = leafCardTextures(true);
  // The painted leaves' size: the leaf map's repeat, the fuzz's cards and the density cards scale together.
  const leafScale = mass.leafScale ?? 1, tile = s.tile * leafScale, cardScale = mass.cardScale ?? 1;
  const leafMap = { texture: leafNormalTexture(s.brush), tile, origin: centre, strength: s.strength, offset: s.offset };
  const surface = new THREE.Mesh(smoothGeometry(geometry), leafNormalMaterial(sun, tile, s.strength, 0.35, true, centre, s.brush, s.offset));
  surface.name = "painted/surface";
  const fuzz = contourFuzz(geometry, leaf.cards, { ...s.fuzz, size: s.fuzz.size * leafScale * cardScale, density: s.fuzz.density / cardScale, sun, normals: leaf.normals, leafMap });
  fuzz.name = "painted/fuzz";
  const cards = densityCards(field, leaf.cards, { ...s.cards, size: s.cards.size * leafScale * cardScale, branchSize: s.cards.branchSize * leafScale * cardScale,
    // Smaller cards, more of them: the candidates' spacing shrinks so their cover (count times area) holds.
    spacing: s.cards.spacing * Math.pow(cardScale, 2 / 3), shapes: mass.shapes ?? 0, variation: mass.variation ?? 0, sparkles: mass.sparkles ?? 0, sprayScale: s.cards.sprayScale * (mass.sprayScale ?? 1), level: mass.level, keep: mass.cardKeep, sun, twigs, branches: branchCardTexture(),
    roundFamily: true, shadeField, shadeMix: form, shadeJitter: jitter,
    tierField: densityField(sites, mass.radius * s.tierSpread, mass.cell), leafMap });
  cards.name = "painted/cards";
  // The whole plant as one blob: an ellipsoid a little wider than the core's bounds.
  const radii = geometry.boundingBox!.getSize(new THREE.Vector3()).multiplyScalar(0.55);
  const parts = [surface, fuzz, cards];
  const palette = mass.palette ?? { hue: 0, saturation: 1, value: 1, shade: 1, shadeHue: 0, contrast: 1, depth: 1, bands: 6, deep: 0 };
  const grade = { value: new THREE.Vector4(palette.hue * Math.PI / 180, palette.saturation, palette.value, palette.shadeHue * Math.PI / 180) };
  const shade = { value: palette.shade };
  for (const part of parts) {
    const material = part.material as THREE.ShaderMaterial;
    const uniforms = material.uniforms;
    // Shadows: the parts cast as they are drawn, from the sun's view (cards face whatever camera draws them, so in the
    // sun's shadow pass they face the sun), their leaves' cutout dappling the shadow. The depth pass runs their own
    // shaders (a plain depth material would not stand the cards up), sharing their uniforms.
    part.castShadow = true;
    part.customDepthMaterial = new THREE.ShaderMaterial({ uniforms, vertexShader: material.vertexShader, fragmentShader: material.fragmentShader, side: material.side });
    // The painted bands are display colours: not tone mapped (unless the scene's painted finishing asks for it).
    followPaintedFinishing(material);
    uniforms.msGrade = grade;
    uniforms.msGradeShade = shade;
    // Soft edges (`FoliageMass.softEdges`): a card's cutout becomes its coverage, sharpened to about a pixel and blended
    // over what is behind (the core and the scene are drawn first; the cards still write depth, so a card behind a rim
    // drawn later is hidden there: a pixel of rim then shows what was behind both). The shadow pass keeps the hard cutout.
    const soft = !!mass.softEdges && part !== surface;
    if (soft) {
      material.transparent = true;
      material.depthWrite = true;
      material.blending = THREE.CustomBlending;
      material.blendSrc = THREE.SrcAlphaFactor; material.blendDst = THREE.OneMinusSrcAlphaFactor;
      material.blendSrcAlpha = THREE.OneFactor; material.blendDstAlpha = THREE.OneMinusSrcAlphaFactor;
    }
    material.onBeforeCompile = shader => {
      let fragment = shader.fragmentShader;
      if (soft) fragment = "float msCoverage = 1.0;\n" + fragment.replace(/if \((\w+)\.a < 0\.5\) discard;/g,
        "msCoverage = clamp(($1.a - 0.5) / max(fwidth($1.a), 1e-4) + 0.5, 0.0, 1.0); if (msCoverage < 0.02) discard;");
      shader.fragmentShader = GRADE_GLSL + fragment.replaceAll("#include <colorspace_fragment>",
        `gl_FragColor.rgb = msGradeColour(gl_FragColor.rgb);${soft ? " gl_FragColor.a = msCoverage;" : ""}\n#include <colorspace_fragment>`);
    };
    material.customProgramCacheKey = () => `painted-mass:${soft}`;
    // The scene's sun, shared, so the bands follow the lighting.
    uniforms.sun = programSun;
    if (uniforms.highlight) uniforms.highlight.value = s.highlight;
    Object.assign(uniforms, {
      msMacroCentre: { value: centre.clone() }, msMacroRadii: { value: radii.clone() }, msMacroWeight: { value: s.macro },
      // The palette's contrast and depth scale the milestone's light stretch, inner darkening and shade-side shift.
      msContrast: { value: s.contrast * (palette.contrast ?? 1) }, msOcclusion: { value: s.occlusion * (palette.depth ?? 1) },
      msMacroShade: { value: s.macroShade * (palette.depth ?? 1) },
      // Its band count and the deep end of its ramp (leaf-normals.ts BAND_GLSL).
      msBands: { value: palette.bands ?? 6 }, msDeep: { value: palette.deep ?? 0 },
    });
    // Built where they stand: no picking, and never culled by stale bounds.
    part.raycast = () => {};
    part.frustumCulled = false;
  }
  return parts;
}
