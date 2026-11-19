import * as THREE from "three";
import type { FoliageMass } from "../model/packet.ts";
import { analyseCrown, animeUniforms, generator, trackViewport } from "./anime-tree-crown.ts";
import { animeLeaves } from "./anime-tree-leaves.ts";
import { animeSprays, glintCard, stationCards } from "./anime-tree-sprays.ts";
import { animeShells, shellCards } from "./anime-tree-shells.ts";
import { animeImpostors } from "./anime-tree-impostors.ts";
import { animeBark } from "./anime-tree-bark.ts";

/**
 * Illustrated trees (prototype): looks that draw a plant's mass (`FoliageMass`) as distinct leaves on its twigs
 * instead of the painted mass (painted-mass.ts), compared in generated/verification/anime-tree:
 * - `leaves`: every leaf an instance of a small leaf-shaped mesh, in whorls along the twigs (anime-tree-leaves.ts);
 * - `sprays`: many small cards, each a painted spray of leaves, at the stations of the twigs (anime-tree-sprays.ts);
 * - `shells`: a leaf-patterned lumpy ellipsoid for each cluster, with sprays standing out of it (anime-tree-shells.ts);
 * - `impostors`: a card for each cluster showing a pre-drawn cluster of leaves, relit (anime-tree-impostors.ts).
 * All read the crown the same way and light it through the same ramp (anime-tree-crown.ts): clusters of many sizes and
 * shapes, each a dome of leaves over a dark underside, with small highlight leaves on the tops open to the sky. A mass
 * names its look (`FoliageMass.look`); the page's `?treelook=<name>` draws every mass with one. The plant's bark is
 * painted to match (`animeTreeBark`, anime-tree-bark.ts).
 */
export const ANIME_TREE_LOOKS = ["leaves", "sprays", "shells", "impostors"] as const;
export type AnimeTreeLook = typeof ANIME_TREE_LOOKS[number];

/** The illustrated look `mass` is drawn with, if any: the page's `?treelook`, else the mass's own. */
export function animeTreeLook(mass: FoliageMass): AnimeTreeLook | null {
  const asked = typeof location !== "undefined" ? new URLSearchParams(location.search).get("treelook") : null;
  const look = asked ?? mass.look;
  return (ANIME_TREE_LOOKS as readonly string[]).includes(look ?? "") ? look as AnimeTreeLook : null;
}

/** The height of the mass's twigs as placed: where they start and end, in the scene. */
function reach(mass: FoliageMass, placement: THREE.Matrix4): { low: number; high: number } {
  let low = Infinity, high = -Infinity;
  const p = new THREE.Vector3();
  for (const twig of mass.twigs) for (const [x, y, z] of twig) { p.set(x, y, z).applyMatrix4(placement); low = Math.min(low, p.z); high = Math.max(high, p.z); }
  return { low, high };
}

/**
 * The parts of `mass` placed by `placement` in `look`, in the space the placement puts them in. `sun` and `time` are
 * the scene's shared uniforms. Each part casts its own shadow (`customDepthMaterial`) and states what it draws in
 * `userData.animeTree` (`instances`, `triangles`). A tree, a shrub and a bed of groundcover are drawn alike, at their
 * own scale: leaves, cards and clusters shrink with the plant's height (to no less than half a tree's).
 */
export function animeTree(mass: FoliageMass, placement: THREE.Matrix4, look: AnimeTreeLook, sun: THREE.IUniform<THREE.Vector3>, time: THREE.IUniform<number>): THREE.Mesh[] {
  const uniforms = animeUniforms(sun, time);
  const { low, high } = reach(mass, placement), scale = placement.getMaxScaleOnAxis();
  // A tree's crown is some 6 m high; a shrub's a meter or two; groundcover is many small plants (its field's cells are fine).
  const low_plant = mass.cell < 0.2, small = low_plant ? 0.5 : Math.min(1, Math.max(0.55, (high - low) / (5 * scale)));
  const cluster = 0.85 * small;
  let parts: THREE.Mesh[];
  if (look === "leaves") {
    const crown = analyseCrown(mass, placement, { spacing: low_plant ? 0.3 : 0.24 * small, clusterRadius: cluster, fill: low_plant ? 1 : 3, tier: low_plant ? 0.25 : 0.6, under: 0.3 });
    parts = [animeLeaves(crown, uniforms, { length: 0.19 * small, width: 0.36, whorl: low_plant ? 4 : 6, whorlMore: low_plant ? 2 : 3, glints: 0.5 })];
  } else if (look === "sprays") {
    const crown = analyseCrown(mass, placement, { spacing: low_plant ? 0.3 : 0.24 * small, clusterRadius: cluster, fill: low_plant ? 1 : 4, tier: low_plant ? 0.25 : 0.6, under: 0.3 });
    parts = [animeSprays(stationCards(crown, 0.34 * small, 0.5), uniforms)];
  } else if (look === "shells") {
    const crown = analyseCrown(mass, placement, { spacing: 0.42 * small, clusterRadius: cluster });
    const cards = shellCards(crown, low_plant ? 8 : 30, 0.36 * small), random = generator(47);
    // Highlights on the shells' tops.
    for (const card of cards.slice()) if (random() < 0.35 && card.occlusion < 0.4 && card.normal.z > 0.25) cards.push(glintCard(card.position, card.axis, card.normal, card.occlusion, 0.36 * small * crown.scale, random));
    parts = [animeShells(crown, uniforms, 1.5 * small), animeSprays(cards, uniforms)];
  } else {
    parts = [animeImpostors(analyseCrown(mass, placement, { spacing: 0.42 * small, clusterRadius: 0.8 * small }), uniforms)];
  }
  for (const part of parts) {
    part.castShadow = true;
    // Built where they stand: no picking, and never culled by stale bounds.
    part.raycast = () => {};
    part.frustumCulled = false;
    (part.material as THREE.Material).toneMapped = false;
    trackViewport(part);
  }
  return parts;
}

/**
 * The bark of a plant whose mass is drawn in an illustrated look: its material and the one that casts its shadow, for
 * the plant's bark meshes (plain or instanced) as they stand. The trunk darkens as it rises into the crown.
 */
export function animeTreeBark(mass: FoliageMass, placement: THREE.Matrix4, sun: THREE.IUniform<THREE.Vector3>, time: THREE.IUniform<number>): { material: THREE.ShaderMaterial; depth: THREE.ShaderMaterial } {
  const { low, high } = reach(mass, placement);
  return animeBark(animeUniforms(sun, time), low - 0.15 * (high - low), low + 0.3 * (high - low));
}
