export const citySky = { zenith: [0.17, 0.34, 0.29], horizon: [1.95, 1.69, 1.32] };

const shopTube = { color: 0xebf1e6, candela: 52, range: 5.2, angle: Math.PI * 0.445, penumbra: 0.45 };
const displayCase = { color: 0xffdbb2, candela: 4.8, range: 2.05, angle: 0.77, penumbra: 0.62 };
const threshold = { color: 0xffdbb4, candela: 9, range: 3.25, angle: 0.82, penumbra: 0.62 };

export function shopTubeLight(position) {
  const [x, y, z] = position;
  const liquor = Math.abs(x - 18.07) < 0.15 && z >= 10.4 && z <= 12.65 && y < 2.9;
  return {
    kind: "shop-fixture",
    position,
    target: [x, y - 1, z],
    ...shopTube,
    ...(liquor ? { accentColor: z < 11.5 ? 0xe1ecdc : 0xf3e5cd } : {}),
  };
}

export const shopAccentLights = [
  { position: [18.90, 2.08, -3.08], target: [18.43, 1.20, -3.28], ...displayCase },
  { position: [18.91, 2.06, -4.18], target: [18.49, 1.16, -4.31], ...displayCase },
  { position: [20.2, 1.95, 11.57], target: [18.59, 0.68, 11.49], ...threshold },
].map(light => ({ kind: "shop-fixture", accent: true, ...light }));

// Local -Y faces out of the opal lens. Aim back at the plaster below it.
export function passageLampLight({ position: [x, y, z], heading, scale = 1 }) {
  const angle = (heading * Math.PI) / 180;
  const outward = 0.115 * scale;
  return {
    kind: "passage-lamp",
    position: [x + Math.sin(angle) * outward, z - 0.05 * scale, -y + Math.cos(angle) * outward],
    target: [x, z - 0.50 * scale, -y],
    color: 0xffe1c2,
    candela: 0.48,
    range: 1.15,
    angle: 1.02,
    penumbra: 0.96,
  };
}

// Ceiling mounts are Z-up in AML. Exported fixture and floor targets are Y-up.
export function arcadeBattenLight({ position: [x, y, z], scale = 1 }) {
  return {
    kind: "arcade-batten",
    position: [x, z - 0.11 * scale, -y],
    target: [x, 0, -y],
    color: 0xffe8ca,
    candela: 34,
    range: 5.4,
    angle: 0.89,
    penumbra: 0.90,
  };
}

const planterOptics = {
  "ceiling-twin-strip": { offset: 0.12, color: 0xe8f0e9, candela: 896, angle: 0.94, penumbra: 0.86 },
  "ceiling-bulkhead": { offset: 0.16, color: 0xffdfb2, candela: 728, angle: 0.83, penumbra: 0.88 },
};

export function planterCeilingLight({ position: [x, y, z], scale = 1, planterLight }) {
  const {fixture, floorY, undersideY} = planterLight;
  const {offset, ...profile} = planterOptics[fixture];
  const lensY = z - offset * scale;
  return {
    kind: "planter-ceiling", fixture, position: [x, lensY, -y], target: [x, floorY, -y],
    floorY, ceilingY: z,
    // Reach the upper floor but extinguish inside its slab, before the lower room.
    range: lensY - undersideY - 0.025, ...profile,
  };
}
