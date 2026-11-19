import * as THREE from "three";
import type { FoliageMass } from "../model/packet.ts";
import { densityField, densitySurface, shadeNormals } from "./leaf-density.ts";
import { leafNormalMaterial, leafNormalTexture, smoothGeometry } from "./leaf-normals.ts";
import { contourFuzz } from "./contour-fuzz.ts";
import { branchCardTexture, densityCards } from "./density-cards.ts";
import { leafCardTextures } from "./foliage-plants.ts";
import { programSun } from "./shade.ts";

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
  shadeNormals(geometry, shadeField, s.shadeMix);
  geometry.computeBoundingBox();
  const centre = geometry.boundingBox!.getCenter(new THREE.Vector3());
  const sun = programSun.value.clone();
  const leaf = leafCardTextures(true);
  const leafMap = { texture: leafNormalTexture(s.brush), tile: s.tile, origin: centre, strength: s.strength, offset: s.offset };
  const surface = new THREE.Mesh(smoothGeometry(geometry), leafNormalMaterial(sun, s.tile, s.strength, 0.35, true, centre, s.brush, s.offset));
  surface.name = "painted/surface";
  const fuzz = contourFuzz(geometry, leaf.cards, { ...s.fuzz, sun, normals: leaf.normals, leafMap });
  fuzz.name = "painted/fuzz";
  const cards = densityCards(field, leaf.cards, { ...s.cards, level: mass.level, keep: mass.cardKeep, sun, twigs, branches: branchCardTexture(),
    roundFamily: true, shadeField, shadeMix: s.shadeMix, shadeJitter: s.shadeJitter,
    tierField: densityField(sites, mass.radius * s.tierSpread, mass.cell), leafMap });
  cards.name = "painted/cards";
  // The whole plant as one blob: an ellipsoid a little wider than the core's bounds.
  const radii = geometry.boundingBox!.getSize(new THREE.Vector3()).multiplyScalar(0.55);
  const parts = [surface, fuzz, cards];
  for (const part of parts) {
    const uniforms = (part.material as THREE.ShaderMaterial).uniforms;
    // The scene's sun, shared, so the bands follow the lighting.
    uniforms.sun = programSun;
    if (uniforms.highlight) uniforms.highlight.value = s.highlight;
    Object.assign(uniforms, {
      msMacroCentre: { value: centre.clone() }, msMacroRadii: { value: radii.clone() }, msMacroWeight: { value: s.macro },
      msContrast: { value: s.contrast }, msOcclusion: { value: s.occlusion }, msMacroShade: { value: s.macroShade },
    });
    // Built where they stand: no picking, and never culled by stale bounds.
    part.raycast = () => {};
    part.frustumCulled = false;
  }
  return parts;
}
