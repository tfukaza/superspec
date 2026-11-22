import { DEFAULT_GRAPHICS_PRESET, GRAPHICS_PRESETS, GRAPHICS_RENDER_SCALE,
  resolveGraphicsSettings, type GraphicsOverrides, type GraphicsPreset } from "./graphicsSettings.ts";

interface Setting<T> {
  readonly fallback: T;
  accepts(value: unknown): value is T;
  readonly legacy: string;
  read(query: URLSearchParams): T | undefined;
}

function toggle(legacy: string, fallback: boolean): Setting<boolean> {
  return { fallback, accepts: (value): value is boolean => typeof value === "boolean", legacy,
    read: query => { const value = query.get(legacy); return value === "on" ? true : value === "off" ? false : undefined; } };
}

function choice<const T extends string>(legacy: string, fallback: T, ...values: T[]): Setting<T> {
  const allowed = new Set<unknown>([fallback, ...values]);
  const accepts = (value: unknown): value is T => allowed.has(value);
  return { fallback, accepts, legacy, read: query => { const value = query.get(legacy); return accepts(value) ? value : undefined; } };
}

const number = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);
const hex = (value: unknown): value is string => typeof value === "string" && /^[\da-f]{6}$/i.test(value);
function scalar<T extends number | string>(legacy: string, accepts: (value: unknown) => value is T,
  convert: (value: string) => unknown, fallback?: T): Setting<T | undefined> {
  return { fallback, accepts, legacy, read: query => {
    const raw = query.get(legacy);
    const value = raw === null ? undefined : convert(raw);
    return accepts(value) ? value : undefined;
  } };
}

/** One declaration per runtime option. Legacy aliases are read only at host boundaries. */
const diagnosticSchema = {
  visibility: {
    cityCulling: toggle("citycull", true), lod: toggle("lod", true), instanceCulling: toggle("instancecull", true),
    merge: toggle("citymerge", true), clusters: toggle("cityclusters", true),
    detail: choice("propdetail", "default", "balanced", "full"), hardwareGroups: toggle("hardwaregroups", true),
    clusterDraws: choice("clusterdraws", "wide", "pairs", "off"), occlusionGrid: choice("occlusiongrid", "fine", "coarse"),
    glazingGroups: toggle("glazinggroups", true), vegetationLod: toggle("weedlod", true), blockProxy: toggle("blockproxy", false),
  },
  materials: {
    textureLod: toggle("texturelod", true), grain: toggle("grain", true), bounce: choice("gi", "painted", "smooth", "off"),
    reflectionProbes: toggle("reflectionprobes", true), worktopReflection: choice("worktopreflection", "default", "strip", "raw"),
    singlePassGlazing: choice("glazingsinglepass", "default", "off", "exact"), textureChannels: choice("texturechannels", "narrow", "full"),
    shopFinish: toggle("shopfinish", true), hardwareBounce: choice("hardwarebounce", "full", "coarse"),
  },
  lighting: {
    fixtures: toggle("fixtures", true), shopLight: choice("shoplight", "default", "baseline"),
    paintedSpots: toggle("paintedspots", false), paintedShadows: toggle("paintedshadows", false),
    shadows: toggle("shadows", true), shadowCache: toggle("shadowcache", true), shadowInstanceCull: toggle("shadowinstancecull", true),
    ambientOcclusion: choice("ao", "auto", "on", "off"), contours: choice("contours", "auto", "on", "off"),
  },
  effects: {
    edgeBlur: toggle("edgeblur", false), displayAA: toggle("aa", true), edgeBoost: choice("edgeboost", "auto", "on", "off"),
    bloom: choice("bloom", "auto", "on", "off"), outline: choice("outline", "auto", "on", "off", "debug"),
    exclusionDepth: choice("exclusiondepth", "depth", "full"), exclusionCull: toggle("exclusioncull", true),
    shopArt: choice("shopart", "default", "off", "wash", "brush", "palette"),
  },
  loading: {
    warmup: toggle("warmup", true), shaderWarmup: choice("shaderwarmup", "parallel", "serial"),
    textureWarmupMiB: {
      fallback: 32, accepts: (value: unknown): value is number => number(value) && value >= 0,
      legacy: "texturewarmupmib",
      read: (query: URLSearchParams) => {
        const raw = query.get("texturewarmupmib"), value = raw === null ? NaN : Number(raw);
        return Number.isFinite(value) && value >= 0 ? value : undefined;
      },
    } satisfies Setting<number>,
  },
  instrumentation: {
    gpuProbe: { ...toggle("gpuprobe", false), read: (query: URLSearchParams) => query.has("gpuprobe") || undefined },
  },
  // Values in this group use the same ranges and transforms as the lighting editor.
  look: {
    shadowStyle: scalar("shadowstyle", (value): value is "plain" | "illustration" => value === "plain" || value === "illustration",
      value => ["plain", "off", "none"].includes(value) ? "plain" : "illustration"),
    shadowBlur: scalar("shadowblur", number, Number), shadowSpread: scalar("shadowspread", number, Number),
    shadowBlurMax: scalar("shadowblurmax", number, Number), shadowDark: scalar("shadowdark", number, Number),
    shadowTint: scalar("shadowtint", hex, String), shadowTintAlt: scalar("shadowtint2", hex, String),
    shadowTintStrength: scalar("shadowtintstrength", number, Number), shadowWrap: scalar("shadowwrap", number, Number),
    shadowNoiseScale: scalar("shadownoisescale", number, Number), shadowEdge: scalar("shadowedge", number, Number),
    shadowNoise: scalar("shadownoise", number, Number), shadowSeed: scalar("shadowseed", number, Number),
    shadowForm: scalar("shadowform", number, Number), shadowFormFill: scalar("shadowformfill", number, Number),
    aoRadius: scalar("aoradius", number, Number), aoIntensity: scalar("aointensity", number, Number),
    aoThickness: scalar("aothickness", number, Number), contourStrength: scalar("contourstrength", number, Number),
    contourWidth: scalar("contourwidth", number, Number), contourDarken: scalar("contourdarken", number, Number),
    contourColor: scalar("contourcolor", hex, String), contourDepth: scalar("contourdepth", number, Number),
    contourNormal: scalar("contournormal", number, Number), contourFade: scalar("contourfade", number, Number),
    edgeThreshold: scalar("edgethreshold", number, Number),
  },
};

type DiagnosticSchema = typeof diagnosticSchema;
export type RenderDiagnostics = { readonly [G in keyof DiagnosticSchema]:
  { readonly [K in keyof DiagnosticSchema[G]]: DiagnosticSchema[G][K] extends Setting<infer T> ? T : never } };
export type RenderDiagnosticOverrides = { readonly [G in keyof RenderDiagnostics]?: Partial<RenderDiagnostics[G]> };

export interface RenderProfile {
  readonly version: 1;
  readonly preset?: GraphicsPreset;
  readonly overrides?: GraphicsOverrides;
  readonly diagnostics?: RenderDiagnosticOverrides;
}

export type ResolvedRenderProfile = ReturnType<typeof resolveRenderProfile>;

function record(value: unknown, path: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw Error(`${path} must be an object.`);
  return value as Record<string, unknown>;
}

function checkKeys(values: Record<string, unknown>, keys: object, path: string) {
  for (const key of Object.keys(values)) if (!Object.hasOwn(keys, key)) throw Error(`Unknown ${path}.${key}.`);
}

export function isGraphicsPreset(value: unknown): value is GraphicsPreset {
  return GRAPHICS_PRESETS.some(preset => value === preset.id);
}

function readDiagnostics(profile: unknown, legacyQuery: URLSearchParams | undefined, defaults: boolean) {
  const specified = profile === undefined ? {} : record(profile, "diagnostics");
  checkKeys(specified, diagnosticSchema, "diagnostics");
  const groups = Object.entries(diagnosticSchema).map(([group, fields]) => {
    const path = "diagnostics." + group;
    const values = Object.hasOwn(specified, group) ? record(specified[group], path) : {};
    checkKeys(values, fields, path);
    const entries = Object.entries<Setting<unknown>>(fields).map(([key, setting]) => {
      const stated = values[key];
      if (Object.hasOwn(values, key) && !setting.accepts(stated)) throw Error(`Invalid ${path}.${key}.`);
      return [key, (legacyQuery && setting.read(legacyQuery)) ?? stated ?? (defaults ? setting.fallback : undefined)] as const;
    }).filter(([, value]) => value !== undefined);
    return [group, Object.fromEntries(entries)];
  }).filter(([, values]) => defaults || Object.keys(values).length > 0);
  return Object.fromEntries(groups);
}

export function resolveRenderDiagnostics(profile?: unknown, legacyQuery?: URLSearchParams): RenderDiagnostics {
  return readDiagnostics(profile, legacyQuery, true) as RenderDiagnostics;
}

let legacyDiagnostics: { search: string; diagnostics: RenderDiagnostics } | undefined;
/** Compatibility for adapter consumers still using the old per-flag links. */
export function legacyRenderDiagnostics(): RenderDiagnostics {
  const search = globalThis.location?.search ?? "";
  if (legacyDiagnostics?.search !== search)
    legacyDiagnostics = { search, diagnostics: resolveRenderDiagnostics(undefined, new URLSearchParams(search)) };
  return legacyDiagnostics.diagnostics;
}

/** Validate JSON from disk or a viewer link and resolve all defaults in one place. */
export function resolveRenderProfile(input: unknown = { version: 1 }, legacyQuery?: URLSearchParams) {
  const value = record(input, "profile");
  checkKeys(value, { version: 1, preset: 1, overrides: 1, diagnostics: 1 }, "profile");
  if (value.version !== 1) throw Error("Render profile version must be 1.");
  if (Object.hasOwn(value, "preset") && !isGraphicsPreset(value.preset)) throw Error("Unknown graphics preset.");
  const overrides = Object.hasOwn(value, "overrides") ? record(value.overrides, "overrides") : {};
  checkKeys(overrides, { renderScale: 1, geometry: 1, materials: 1, shadows: 1 }, "overrides");
  for (const key of ["geometry", "materials", "shadows"])
    if (Object.hasOwn(overrides, key) && !isGraphicsPreset(overrides[key])) throw Error(`Unknown overrides.${key} preset.`);
  const scale = overrides.renderScale;
  if (Object.hasOwn(overrides, "renderScale") && (!number(scale) || scale < GRAPHICS_RENDER_SCALE.min || scale > GRAPHICS_RENDER_SCALE.max))
    throw Error(`overrides.renderScale must be between ${GRAPHICS_RENDER_SCALE.min} and ${GRAPHICS_RENDER_SCALE.max}.`);
  const shorthand = legacyQuery?.get("graphics");
  if (shorthand != null && !isGraphicsPreset(shorthand)) throw Error("Unknown graphics preset in viewer link.");
  const profile = value as unknown as RenderProfile;
  const preset = shorthand ?? profile.preset ?? DEFAULT_GRAPHICS_PRESET;
  const diagnostics = resolveRenderDiagnostics(profile.diagnostics, legacyQuery);
  const explicit = readDiagnostics(profile.diagnostics, legacyQuery, false) as RenderDiagnosticOverrides;
  return { profile: { version: 1, preset, ...(profile.overrides ? { overrides: profile.overrides } : {}),
    ...(Object.keys(explicit).length ? { diagnostics: explicit } : {}) } satisfies RenderProfile,
    preset, settings: resolveGraphicsSettings(preset, profile.overrides), diagnostics };
}

/** Compact named links, or one structured parameter whenever a link has overrides. */
export function writeRenderProfile(query: URLSearchParams, profile: RenderProfile): void {
  for (const fields of Object.values(diagnosticSchema)) for (const setting of Object.values(fields)) query.delete(setting.legacy);
  query.delete("graphics");
  query.delete("profile");
  if (profile.overrides || profile.diagnostics) query.set("profile", JSON.stringify(profile));
  else if (profile.preset) query.set("graphics", profile.preset);
}

/** Bridge to the existing lighting record overrides used by the authoring tools. */
export function lightingOverridesQuery(diagnostics: RenderDiagnostics): URLSearchParams {
  const query = new URLSearchParams();
  if (!diagnostics.lighting.shadows) query.set("shadows", "off");
  const { ambientOcclusion, contours } = diagnostics.lighting;
  if (ambientOcclusion !== "auto") query.set("ao", ambientOcclusion);
  if (contours !== "auto") query.set("contours", contours);
  const { edgeBlur, edgeBoost, displayAA, outline } = diagnostics.effects;
  if (edgeBlur) query.set("edgeblur", "on");
  if (edgeBoost !== "auto") query.set("edgeboost", edgeBoost);
  if (!displayAA) query.set("aa", "off");
  if (outline !== "auto") query.set("outline", outline);
  for (const key of Object.keys(diagnosticSchema.look) as (keyof RenderDiagnostics["look"])[]) {
    const value = diagnostics.look[key];
    if (value !== undefined) query.set(diagnosticSchema.look[key].legacy, String(value));
  }
  return query;
}
