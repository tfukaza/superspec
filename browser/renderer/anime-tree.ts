import * as THREE from "three";
import type { FoliageMass } from "../model/packet.ts";
import { analyseCrown, animeUniforms } from "./anime-tree-crown.ts";
import { animeLeaves } from "./anime-tree-leaves.ts";
import { animeSprays, stationCards } from "./anime-tree-sprays.ts";
import { animeShells, shellCards } from "./anime-tree-shells.ts";
import { animeImpostors } from "./anime-tree-impostors.ts";

/**
 * Illustrated trees (prototype): looks that draw a plant's mass (`FoliageMass`) as distinct leaves on its twigs
 * instead of the painted mass (painted-mass.ts), compared in generated/verification/anime-tree:
 * - `leaves`: every leaf an instance of a small leaf-shaped mesh, in whorls along the twigs (anime-tree-leaves.ts);
 * - `sprays`: a card with a painted spray of leaves at each station of a twig (anime-tree-sprays.ts);
 * - `shells`: a leaf-patterned lumpy ellipsoid for each cluster, with sprays standing out of it (anime-tree-shells.ts);
 * - `impostors`: a card for each cluster showing a pre-drawn cluster of leaves, relit (anime-tree-impostors.ts).
 * All read the crown the same way and light it through the same ramp (anime-tree-crown.ts). A mass names its look
 * (`FoliageMass.look`); the page's `?treelook=<name>` draws every tree's mass with one.
 */
export const ANIME_TREE_LOOKS = ["leaves", "sprays", "shells", "impostors"] as const;
export type AnimeTreeLook = typeof ANIME_TREE_LOOKS[number];

/** The illustrated look `mass` is drawn with, if any: the page's `?treelook` for a tree's mass, else the mass's own. */
export function animeTreeLook(mass: FoliageMass): AnimeTreeLook | null {
  const asked = typeof location !== "undefined" ? new URLSearchParams(location.search).get("treelook") : null;
  // The page's choice is for trees: a mass of fine cells (groundcover) keeps its own look.
  const look = asked && mass.cell >= 0.2 ? asked : mass.look;
  return (ANIME_TREE_LOOKS as readonly string[]).includes(look ?? "") ? look as AnimeTreeLook : null;
}

/**
 * The parts of `mass` placed by `placement` in `look`, in the space the placement puts them in. `sun` and `time` are
 * the scene's shared uniforms. Each part casts its own shadow (`customDepthMaterial`) and states what it draws in
 * `userData.animeTree` (`instances`, `triangles`).
 */
export function animeTree(mass: FoliageMass, placement: THREE.Matrix4, look: AnimeTreeLook, sun: THREE.IUniform<THREE.Vector3>, time: THREE.IUniform<number>): THREE.Mesh[] {
  const uniforms = animeUniforms(sun, time);
  const leaf = Math.sqrt(mass.leafScale ?? 1);
  let parts: THREE.Mesh[];
  if (look === "leaves") {
    parts = [animeLeaves(analyseCrown(mass, placement, { spacing: 0.24, fill: 2, tier: 0.65, under: 0.3 }), uniforms, { length: 0.19 * leaf, width: 0.36, whorl: 6 })];
  } else if (look === "sprays") {
    const crown = analyseCrown(mass, placement, { spacing: 0.3, fill: 1, tier: 0.7, under: 0.3 });
    parts = [animeSprays(stationCards(crown, 0.5 * leaf), uniforms)];
  } else if (look === "shells") {
    const crown = analyseCrown(mass, placement, { spacing: 0.42 });
    parts = [animeShells(crown, uniforms, 1.5 * leaf), animeSprays(shellCards(crown, 22, 0.5 * leaf), uniforms)];
  } else {
    parts = [animeImpostors(analyseCrown(mass, placement, { spacing: 0.42, clusterRadius: 0.8 }), uniforms)];
  }
  for (const part of parts) {
    part.castShadow = true;
    // Built where they stand: no picking, and never culled by stale bounds.
    part.raycast = () => {};
    part.frustumCulled = false;
    (part.material as THREE.Material).toneMapped = false;
  }
  return parts;
}
