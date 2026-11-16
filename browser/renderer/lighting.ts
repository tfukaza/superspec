/**
 * The portable lighting record (version 1): how a scene is lit and rendered, apart from its models and materials.
 * Global settings (exposure, tone mapping, environment, ambient light, ambient occlusion) and per-light settings
 * (direction, color, shadow style); never part of a material. It mirrors the Rust record in
 * `crates/types/src/lighting.rs`: the same fields, defaults and limits. Two fixtures in `crates/model/src`
 * (`lighting.default.json`, `lighting.invalid.json`) are checked by both sides, so they cannot drift apart.
 *
 * Meters, right-handed Z-up, radians, linear RGB in [0, 1]. Every field but `version` has a default, so
 * `{ "version": 1 }` is the shipped look. This module has no renderer in it; `rendering.ts` builds a view from a record.
 */
export type Rgb = [number, number, number];

/** Noise in the light's plane; a pure function of position there and `seed`. */
export interface ShadowNoise {
  /** Feature size, meters. */
  scale: number;
  /** Displacement of the shadow's edge, meters. */
  edge: number;
  /** Variation of darkness and hue inside the shadow, 0 to 1. It only darkens. */
  amount: number;
  seed: number;
}

/** The look of one light's shadows. */
export interface LightShadow {
  /** `plain` is the renderer's own shadow; the values below are then kept but unused. */
  style: "plain" | "illustration";
  /** Penumbra radius where the occluder touches the receiver, meters. */
  softness: number;
  /** Penumbra radius gained per meter between occluder and receiver. */
  spread: number;
  /** Largest penumbra radius, meters; also how far around a point occluders are looked for. */
  maxSoftness: number;
  /** How much of the shaded color the shadow removes beyond the tint, 0 to 1. */
  darkness: number;
  /** Hue of the shadow, linear RGB. */
  tint: Rgb;
  /** Second hue the interior noise drifts to. */
  tintAlt: Rgb;
  /** How saturated the shadow is, 0 (neutral gray) to 1 (the tint's own saturation). */
  tintStrength: number;
  /** Cosine of the angle to the light below which a face starts to count as shadowed. */
  wrap: number;
  noise: ShadowNoise;
}

export interface Light {
  id: string;
  /** Version 1 has one light, the sun. */
  role: "sun";
  kind: "directional";
  /** Unit vector from the scene toward the light. */
  direction: Rgb;
  /** Linear RGB. */
  color: Rgb;
  intensity: number;
  castsShadow: boolean;
  shadow: LightShadow;
}

/** Screen-space ambient occlusion. Lengths are meters in the scene, so the look does not change with zoom. */
export interface AmbientOcclusionSettings {
  enabled: boolean;
  /** How far around a point occluders are looked for, meters. */
  radius: number;
  /** Strength: up to 1, the share of the occlusion that darkens the image; above 1, creases deepen further (up to 4). */
  intensity: number;
  /** How thick surfaces are taken to be behind what the camera sees, meters: thin parts then cast no halo on what is behind them. */
  thickness: number;
}

export interface Lighting {
  version: 1;
  /** Linear multiplier before tone mapping. */
  exposure: number;
  toneMapping: "aces" | "neutral" | "none";
  /** Image-based light. `studio` is the built-in neutral room, `none` disables it; `rotation` is radians about +Z. */
  environment: { source: string; intensity: number; rotation: number };
  /** Hemisphere light: `sky` from +Z, `ground` from -Z, linear RGB. */
  ambient: { sky: Rgb; ground: Rgb; intensity: number };
  ambientOcclusion: AmbientOcclusionSettings;
  /** An empty list means no direct light. */
  lights: Light[];
}

/**
 * The shipped look: the Studio viewer's lights, the tight neutral-gray illustration shadow and ambient occlusion.
 * Use `defaultLighting()` for a copy to change.
 */
export const DEFAULT_LIGHTING: Lighting = {
  version: 1,
  exposure: 1,
  toneMapping: "aces",
  environment: { source: "studio", intensity: 0.35, rotation: 0 },
  // Ground is #777768.
  ambient: { sky: [1, 1, 1], ground: [0.184474994500441, 0.184474994500441, 0.13843161503245183], intensity: 0.65 },
  ambientOcclusion: { enabled: true, radius: 0.35, intensity: 2, thickness: 0.5 },
  lights: [{
    id: "sun",
    role: "sun",
    kind: "directional",
    // (-3, -5, 8) normalized.
    direction: [-0.30304576336566325, -0.5050762722761054, 0.8081220356417685],
    // #ffefdb.
    color: [1, 0.8631572134541023, 0.7083757798916868],
    intensity: 3,
    castsShadow: true,
    shadow: {
      style: "illustration",
      softness: 0.012,
      spread: 0.025,
      maxSoftness: 0.06,
      darkness: 0.25,
      // #2f6fe0 and #3f4fd8.
      tint: [0.028426039504420793, 0.1589608350608804, 0.7454042095403874],
      tintAlt: [0.04970656598412723, 0.07818742180518633, 0.6866853124353135],
      tintStrength: 0.22,
      wrap: 0.3,
      noise: { scale: 1.1, edge: 0.07, amount: 0.6, seed: 7 },
    },
  }],
};

export function defaultLighting(): Lighting {
  return structuredClone(DEFAULT_LIGHTING);
}

/** The light materials and surveys mean by "the sun", if the scene has one. */
export function sunOf(lighting: Lighting): Light | undefined {
  return lighting.lights.find(light => light.role === "sun");
}

/** A six-digit sRGB hex color as linear RGB, by the transfer function three.js uses for its colors. */
function linearFromHex(hex: string): Rgb {
  const channel = (start: number): number => {
    const value = parseInt(hex.slice(start, start + 2), 16) / 255;
    return value < 0.04045 ? value * 0.0773993808 : Math.pow(value * 0.9478672986 + 0.0521327014, 2.4);
  };
  return [channel(0), channel(2), channel(4)];
}

/**
 * A lighting record with what the page's parameters change: the viewer session's overrides, never saved or exported.
 * The record itself is not changed.
 *
 * - `shadows=off` stops the lights casting.
 * - `shadowstyle=plain` (or `off`, `none`) asks for the renderer's plain shadows; any other value for the styled ones.
 * - `shadowblur`, `shadowspread`, `shadowblurmax`, `shadowdark`, `shadowtint` and `shadowtint2` (sRGB hex),
 *   `shadowtintstrength`, `shadowwrap`, `shadownoisescale`, `shadowedge`, `shadownoise` and `shadowseed` replace single
 *   values of the shadow's look.
 * - `ao=off` turns ambient occlusion off; `aoradius`, `aointensity` and `aothickness` replace its values.
 */
export function lightingFromQuery(record: Lighting, query: URLSearchParams): Lighting {
  const lighting = structuredClone(record);
  const number = (name: string, fallback: number, min: number, max: number): number => {
    const value = Number(query.get(name) ?? NaN);
    return Number.isFinite(value) ? Math.min(Math.max(value, min), max) : fallback;
  };
  const color = (name: string, fallback: Rgb): Rgb => {
    const value = query.get(name);
    return value && /^[0-9a-f]{6}$/i.test(value) ? linearFromHex(value) : fallback;
  };
  const style = query.get("shadowstyle");
  for (const light of lighting.lights) {
    if (query.get("shadows") === "off") light.castsShadow = false;
    const shadow = light.shadow;
    light.shadow = {
      style: style === null ? shadow.style : style === "plain" || style === "off" || style === "none" ? "plain" : "illustration",
      softness: number("shadowblur", shadow.softness, 0, 1),
      spread: number("shadowspread", shadow.spread, 0, 2),
      maxSoftness: number("shadowblurmax", shadow.maxSoftness, 0, 2),
      darkness: number("shadowdark", shadow.darkness, 0, 1),
      tint: color("shadowtint", shadow.tint),
      tintAlt: color("shadowtint2", shadow.tintAlt),
      tintStrength: number("shadowtintstrength", shadow.tintStrength, 0, 1),
      wrap: number("shadowwrap", shadow.wrap, 0.01, 1),
      noise: {
        scale: number("shadownoisescale", shadow.noise.scale, 0.01, 100),
        edge: number("shadowedge", shadow.noise.edge, 0, 1),
        amount: number("shadownoise", shadow.noise.amount, 0, 1),
        seed: Math.floor(number("shadowseed", shadow.noise.seed, 0, 4294967295)),
      },
    };
  }
  const occlusion = lighting.ambientOcclusion;
  if (query.get("ao") === "off") occlusion.enabled = false;
  occlusion.radius = number("aoradius", occlusion.radius, 0.01, 10);
  occlusion.intensity = number("aointensity", occlusion.intensity, 0, 4);
  occlusion.thickness = number("aothickness", occlusion.thickness, 0.01, 10);
  return lighting;
}

/** One refused value: the field's path in the record (`lights[0].shadow.softness`) and why. */
export interface LightingFinding { field: string; message: string }

/** A record that was refused, with every finding. */
export class LightingError extends Error {
  readonly findings: LightingFinding[];

  constructor(findings: LightingFinding[]) {
    super(`Invalid lighting record: ${findings.map(finding => `${finding.field || "lighting"} ${finding.message}`).join("; ")}.`);
    this.name = "LightingError";
    this.findings = findings;
  }
}

const IDENTITY = /^[A-Za-z0-9._-]{1,64}$/;
const IDENTITY_MESSAGE = "must be an identity of 1 to 64 letters, digits, '.', '_' or '-'";

/**
 * Admits a lighting record from parsed JSON and returns a complete copy: missing fields take the shipped defaults.
 * Unknown fields, wrong types, a missing `version` and out-of-range values are refused together, each by field path
 * (`LightingError.findings`). The limits bound what an author may ask for; they are not what a renderer must reach.
 */
export function parseLighting(value: unknown): Lighting {
  const findings: LightingFinding[] = [];
  const fail = (field: string, message: string): void => { findings.push({ field, message }); };
  const fields = (value: unknown, path: string, names: string[]): Record<string, unknown> => {
    if (value === undefined) return {};
    if (typeof value !== "object" || value === null || Array.isArray(value)) { fail(path, "must be an object"); return {}; }
    for (const key of Object.keys(value)) if (!names.includes(key)) fail(path ? `${path}.${key}` : key, "is not a field of the version 1 record");
    return value as Record<string, unknown>;
  };
  const number = (value: unknown, field: string, fallback: number, min: number, max: number): number => {
    if (value === undefined) return fallback;
    if (typeof value !== "number") { fail(field, "must be a number"); return fallback; }
    if (!(Number.isFinite(value) && value >= min && value <= max)) { fail(field, `must be between ${min} and ${max}`); return fallback; }
    return value;
  };
  const choice = <T extends string>(value: unknown, field: string, fallback: T, choices: readonly T[]): T => {
    if (value === undefined) return fallback;
    if (!choices.includes(value as T)) { fail(field, `must be one of ${choices.join(", ")}`); return fallback; }
    return value as T;
  };
  const boolean = (value: unknown, field: string, fallback: boolean): boolean => {
    if (value === undefined) return fallback;
    if (typeof value !== "boolean") { fail(field, "must be true or false"); return fallback; }
    return value;
  };
  const identity = (value: unknown, field: string, fallback: string): string => {
    if (value === undefined) return fallback;
    if (typeof value !== "string" || !IDENTITY.test(value)) { fail(field, IDENTITY_MESSAGE); return fallback; }
    return value;
  };
  const triple = (value: unknown, field: string, fallback: Rgb): Rgb | undefined => {
    if (value === undefined) return [...fallback];
    if (!Array.isArray(value) || value.length !== 3 || !value.every(axis => typeof axis === "number")) { fail(field, "must be three numbers"); return undefined; }
    return [value[0], value[1], value[2]];
  };
  const color = (value: unknown, field: string, fallback: Rgb): Rgb => {
    const rgb = triple(value, field, fallback);
    if (rgb && !rgb.every(channel => Number.isFinite(channel) && channel >= 0 && channel <= 1)) { fail(field, "must be linear RGB between 0 and 1"); return [...fallback]; }
    return rgb ?? [...fallback];
  };

  const base = DEFAULT_LIGHTING, baseSun = base.lights[0];
  if (value === undefined || value === null || typeof value !== "object" || Array.isArray(value)) throw new LightingError([{ field: "", message: "must be an object" }]);
  const root = fields(value, "", ["version", "exposure", "toneMapping", "environment", "ambient", "ambientOcclusion", "lights"]);
  if (root.version === undefined) fail("version", "is required");
  else if (root.version !== 1) fail("version", "must be 1");
  const environment = fields(root.environment, "environment", ["source", "intensity", "rotation"]);
  const ambient = fields(root.ambient, "ambient", ["sky", "ground", "intensity"]);
  const occlusion = fields(root.ambientOcclusion, "ambientOcclusion", ["enabled", "radius", "intensity", "thickness"]);
  const lighting: Lighting = {
    version: 1,
    exposure: number(root.exposure, "exposure", base.exposure, 0, 100),
    toneMapping: choice(root.toneMapping, "toneMapping", base.toneMapping, ["aces", "neutral", "none"]),
    environment: {
      source: identity(environment.source, "environment.source", base.environment.source),
      intensity: number(environment.intensity, "environment.intensity", base.environment.intensity, 0, 100),
      rotation: number(environment.rotation, "environment.rotation", base.environment.rotation, -2 * Math.PI, 2 * Math.PI),
    },
    ambient: {
      sky: color(ambient.sky, "ambient.sky", base.ambient.sky),
      ground: color(ambient.ground, "ambient.ground", base.ambient.ground),
      intensity: number(ambient.intensity, "ambient.intensity", base.ambient.intensity, 0, 100),
    },
    ambientOcclusion: {
      enabled: boolean(occlusion.enabled, "ambientOcclusion.enabled", base.ambientOcclusion.enabled),
      radius: number(occlusion.radius, "ambientOcclusion.radius", base.ambientOcclusion.radius, 0.01, 10),
      intensity: number(occlusion.intensity, "ambientOcclusion.intensity", base.ambientOcclusion.intensity, 0, 4),
      thickness: number(occlusion.thickness, "ambientOcclusion.thickness", base.ambientOcclusion.thickness, 0.01, 10),
    },
    lights: [],
  };
  let lights: unknown[] = [undefined];
  if (root.lights !== undefined) {
    if (Array.isArray(root.lights)) lights = root.lights;
    else fail("lights", "must be a list");
  }
  if (lights.length > 1) fail("lights", "version 1 admits at most 1 light, the sun");
  lights.forEach((entry, index) => {
    const at = (name: string): string => `lights[${index}].${name}`;
    if (root.lights !== undefined && (typeof entry !== "object" || entry === null || Array.isArray(entry))) { fail(`lights[${index}]`, "must be an object"); return; }
    const light = fields(entry, `lights[${index}]`, ["id", "role", "kind", "direction", "color", "intensity", "castsShadow", "shadow"]);
    const shadow = fields(light.shadow, at("shadow"), ["style", "softness", "spread", "maxSoftness", "darkness", "tint", "tintAlt", "tintStrength", "wrap", "noise"]);
    const noise = fields(shadow.noise, at("shadow.noise"), ["scale", "edge", "amount", "seed"]);
    const id = identity(light.id, at("id"), baseSun.id);
    if (lighting.lights.some(other => other.id === id)) fail(at("id"), "must be unique");
    let direction = triple(light.direction, at("direction"), baseSun.direction);
    if (direction && !(Math.abs(Math.sqrt(direction[0] ** 2 + direction[1] ** 2 + direction[2] ** 2) - 1) <= 1e-3)) { fail(at("direction"), "must be a unit vector"); direction = undefined; }
    let seed = baseSun.shadow.noise.seed;
    if (noise.seed !== undefined) {
      if (typeof noise.seed === "number" && Number.isInteger(noise.seed) && noise.seed >= 0 && noise.seed <= 4294967295) seed = noise.seed;
      else fail(at("shadow.noise.seed"), "must be an integer between 0 and 4294967295");
    }
    lighting.lights.push({
      id,
      role: choice(light.role, at("role"), baseSun.role, ["sun"]),
      kind: choice(light.kind, at("kind"), baseSun.kind, ["directional"]),
      direction: direction ?? [...baseSun.direction],
      color: color(light.color, at("color"), baseSun.color),
      intensity: number(light.intensity, at("intensity"), baseSun.intensity, 0, 100),
      castsShadow: boolean(light.castsShadow, at("castsShadow"), baseSun.castsShadow),
      shadow: {
        style: choice(shadow.style, at("shadow.style"), baseSun.shadow.style, ["plain", "illustration"]),
        softness: number(shadow.softness, at("shadow.softness"), baseSun.shadow.softness, 0, 2),
        spread: number(shadow.spread, at("shadow.spread"), baseSun.shadow.spread, 0, 2),
        maxSoftness: number(shadow.maxSoftness, at("shadow.maxSoftness"), baseSun.shadow.maxSoftness, 0, 2),
        darkness: number(shadow.darkness, at("shadow.darkness"), baseSun.shadow.darkness, 0, 1),
        tint: color(shadow.tint, at("shadow.tint"), baseSun.shadow.tint),
        tintAlt: color(shadow.tintAlt, at("shadow.tintAlt"), baseSun.shadow.tintAlt),
        tintStrength: number(shadow.tintStrength, at("shadow.tintStrength"), baseSun.shadow.tintStrength, 0, 1),
        wrap: number(shadow.wrap, at("shadow.wrap"), baseSun.shadow.wrap, 0, 1),
        noise: {
          scale: number(noise.scale, at("shadow.noise.scale"), baseSun.shadow.noise.scale, 0.01, 100),
          edge: number(noise.edge, at("shadow.noise.edge"), baseSun.shadow.noise.edge, 0, 1),
          amount: number(noise.amount, at("shadow.noise.amount"), baseSun.shadow.noise.amount, 0, 1),
          seed,
        },
      },
    });
  });
  if (findings.length) throw new LightingError(findings);
  return lighting;
}

/**
 * The lighting record a Studio project carries in its `lighting` field, or the shipped look when it has none. An
 * invalid record is refused (`LightingError`), so a project never opens with a look other than the one it states.
 */
export function projectLighting(project: object): Lighting {
  const record = (project as { lighting?: unknown }).lighting;
  return record === undefined ? defaultLighting() : parseLighting(record);
}
