function withTwigDetail(definition, { leaves, twig, mass, dabSize, middleCards }) {
  const tree = structuredClone(definition);
  tree.levels.push(twig);
  Object.assign(tree.leaves, { ...leaves, shape: "leaf" });
  // The canopy uses this diameter for each sampled surface mark, in meters.
  Object.assign(tree.mass, mass, { dab: dabSize });
  const [near, distant, far] = tree.lods;
  tree.lods = [
    { ...near, maxLevel: tree.levels.length, leafCards: leaves.cards, hull: "none" },
    { ...near, distance: 6, leafKeep: 0.68, leafCards: middleCards, hull: "none" },
    { ...distant, leafCards: Math.min(distant.leafCards, middleCards) },
    // Sparse broad dabs break up the coarse hull outline.
    { ...far, leafKeep: 0.14, hull: "backing" },
  ];
  return tree;
}

export const treeDemoPresets = {
  birch: {
    label: "Birch",
    barkColor: "#b6aa91",
    // The short terminal twigs droop and carry small fans without closing the gaps between limbs.
    customize: definition => withTwigDetail(definition, {
      leaves: { spacing: 0.045, size: [0.078, 0.123], aspect: 0.6, cards: 3, spread: 130 },
      twig: {
        count: 3, start: 0.34, end: 0.91, angle: [47, 35], length: [0.33, 0.19],
        radius: 0.43, tip: 0.28, segments: 3, gravity: 34, wobble: 7, jitter: 0.65, reach: 0.22,
      },
      mass: { group: 3, radius: 0.17, dense: 6, clump: 5, cell: 0.145, sprig: 4 },
      dabSize: 0.19,
      middleCards: 3,
    }),
  },
  poplar: {
    label: "Poplar",
    barkColor: "#69614b",
    // Short rising offshoots carry separate terminal clusters along the narrow column.
    customize: definition => withTwigDetail(definition, {
      leaves: { spacing: 0.044, thin: 0.58, size: [0.082, 0.127], aspect: 0.6, cards: 1 },
      twig: {
        count: 3, start: 0.32, end: 0.92, angle: [44, 29], length: [0.35, 0.19],
        radius: 0.45, tip: 0.30, segments: 3, gravity: -13, wobble: 6, jitter: 0.61, reach: 0.25,
      },
      mass: { group: 3, radius: 0.19, dense: 10, clump: 7, cell: 0.15, sprig: 6 },
      dabSize: 0.18,
      middleCards: 1,
    }),
  },
};
