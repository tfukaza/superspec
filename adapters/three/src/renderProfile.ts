import { GRAPHICS_PRESETS, GRAPHICS_RENDER_SCALE, graphicsParameterOverrides, isGraphicsPlatform, resolveGraphicsParameters,
  type GraphicsOverrides, type GraphicsParameterOverrides, type GraphicsPlatform, type GraphicsPreset, type GraphicsResolution,
  type GraphicsVariant } from "./graphicsSettings.ts";

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
    // firstFrameShaders links the first view's programs before it is drawn. warmup is the fuller
    // preparation after the first frame, and shaderWarmup "serial" turns parallel linking off for both.
    firstFrameShaders: toggle("firstframeshaders", true),
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

/** Version 1: a preset, and override groups that each take another preset's values. */
export interface RenderProfile {
  readonly version: 1;
  readonly preset?: GraphicsPreset;
  readonly overrides?: GraphicsOverrides;
  readonly diagnostics?: RenderDiagnosticOverrides;
}

/** Version 2: a platform profile, a quality preset and individual parameter overrides. */
export interface RenderProfileV2 {
  readonly version: 2;
  /** Omit to use the platform the host states or detects. */
  readonly platform?: GraphicsPlatform;
  readonly quality?: GraphicsPreset;
  readonly overrides?: GraphicsParameterOverrides;
  readonly diagnostics?: RenderDiagnosticOverrides;
}
export type AnyRenderProfile = RenderProfile | RenderProfileV2;

/**
 * `profile` is the input in its own version, with the link's short forms folded in. `settings`
 * and `diagnostics` are what the renderer reads. `parameters` holds every parameter after the
 * platform, preset and overrides; `diagnostics` can still differ from it, because diagnostics are
 * development switches that no cap limits. `changed` lists what a cap or a missing variant altered.
 */
export interface ResolvedRenderProfile<P extends AnyRenderProfile = RenderProfile> extends GraphicsResolution {
  readonly profile: P;
  readonly diagnostics: RenderDiagnostics;
}

export interface RenderProfileOptions {
  /** The platform the host states or detects. A profile's or a link's own platform wins. */
  readonly platform?: GraphicsPlatform;
  /** Variants the map is published in, when the host knows. */
  readonly publishedVariants?: readonly GraphicsVariant[];
}

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

function readDiagnostics(profile: unknown, legacyQuery: URLSearchParams | undefined, defaults: boolean, quality?: RenderDiagnosticOverrides) {
  const specified = profile === undefined ? {} : record(profile, "diagnostics");
  checkKeys(specified, diagnosticSchema, "diagnostics");
  const groups = Object.entries(diagnosticSchema).map(([group, fields]) => {
    const path = "diagnostics." + group;
    const values = Object.hasOwn(specified, group) ? record(specified[group], path) : {};
    checkKeys(values, fields, path);
    const entries = Object.entries<Setting<unknown>>(fields).map(([key, setting]) => {
      const stated = values[key];
      if (Object.hasOwn(values, key) && !setting.accepts(stated)) throw Error(`Invalid ${path}.${key}.`);
      const groupDefaults = quality?.[group as keyof RenderDiagnostics] as Record<string, unknown> | undefined;
      return [key, (legacyQuery && setting.read(legacyQuery)) ?? stated ?? (defaults ? groupDefaults?.[key] ?? setting.fallback : undefined)] as const;
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

function validateVersion1(value: Record<string, unknown>): RenderProfile {
  checkKeys(value, { version: 1, preset: 1, overrides: 1, diagnostics: 1 }, "profile");
  if (Object.hasOwn(value, "preset") && !isGraphicsPreset(value.preset)) throw Error("Unknown graphics preset.");
  const overrides = Object.hasOwn(value, "overrides") ? record(value.overrides, "overrides") : {};
  checkKeys(overrides, { renderScale: 1, geometry: 1, materials: 1, shadows: 1 }, "overrides");
  for (const key of ["geometry", "materials", "shadows"])
    if (Object.hasOwn(overrides, key) && !isGraphicsPreset(overrides[key])) throw Error(`Unknown overrides.${key} preset.`);
  const scale = overrides.renderScale;
  if (Object.hasOwn(overrides, "renderScale") && (!number(scale) || scale < GRAPHICS_RENDER_SCALE.min || scale > GRAPHICS_RENDER_SCALE.max))
    throw Error(`overrides.renderScale must be between ${GRAPHICS_RENDER_SCALE.min} and ${GRAPHICS_RENDER_SCALE.max}.`);
  return value as unknown as RenderProfile;
}

function validateVersion2(value: Record<string, unknown>): RenderProfileV2 {
  checkKeys(value, { version: 1, platform: 1, quality: 1, overrides: 1, diagnostics: 1 }, "profile");
  if (Object.hasOwn(value, "quality") && !isGraphicsPreset(value.quality)) throw Error("Unknown graphics preset.");
  if (Object.hasOwn(value, "platform") && !isGraphicsPlatform(value.platform)) throw Error("Unknown platform profile.");
  if (Object.hasOwn(value, "overrides")) record(value.overrides, "overrides"); // Names and values are checked when resolved.
  return value as unknown as RenderProfileV2;
}

/** State a version 1 profile in version 2. Each override group becomes the parameter values it stood for. */
export function upgradeRenderProfile(profile: AnyRenderProfile): RenderProfileV2 {
  if (profile.version === 2) return profile;
  return { version: 2, ...(profile.preset ? { quality: profile.preset } : {}),
    ...(profile.overrides ? { overrides: graphicsParameterOverrides(profile.overrides) } : {}),
    ...(profile.diagnostics ? { diagnostics: profile.diagnostics } : {}) };
}

/**
 * Validate JSON from disk or a viewer link and resolve all defaults in one place. A version 1
 * profile is read as its version 2 equivalent and resolved the same way. A link's `graphics`
 * and `platform` short forms override the profile's preset and platform.
 */
export function resolveRenderProfile(input?: RenderProfile, legacyQuery?: URLSearchParams, options?: RenderProfileOptions): ResolvedRenderProfile;
export function resolveRenderProfile(input: RenderProfileV2, legacyQuery?: URLSearchParams, options?: RenderProfileOptions): ResolvedRenderProfile<RenderProfileV2>;
export function resolveRenderProfile(input: unknown, legacyQuery?: URLSearchParams, options?: RenderProfileOptions): ResolvedRenderProfile<AnyRenderProfile>;
export function resolveRenderProfile(input: unknown = { version: 1 }, legacyQuery?: URLSearchParams,
  options: RenderProfileOptions = {}): ResolvedRenderProfile<AnyRenderProfile> {
  const value = record(input, "profile");
  if (value.version !== 1 && value.version !== 2) throw Error("Render profile version must be 1 or 2.");
  const stated = value.version === 1 ? validateVersion1(value) : validateVersion2(value);
  const profile = upgradeRenderProfile(stated);
  const shorthand = legacyQuery?.get("graphics");
  if (shorthand != null && !isGraphicsPreset(shorthand)) throw Error("Unknown graphics preset in viewer link.");
  const linked = legacyQuery?.get("platform");
  if (linked != null && !isGraphicsPlatform(linked)) throw Error("Unknown platform profile in viewer link.");
  const chosenPlatform = linked ?? profile.platform;
  const resolution = resolveGraphicsParameters({ platform: chosenPlatform ?? options.platform, preset: shorthand ?? profile.quality,
    overrides: profile.overrides, publishedVariants: options.publishedVariants });
  const { preset, parameters: { cpu, gpu, memory } } = resolution;
  // What the resolved parameters ask of the renderer. Explicit diagnostics and legacy flags still win.
  const quality: RenderDiagnosticOverrides = {
    visibility: { occlusionGrid: cpu.occlusionGrid, clusterDraws: cpu.clusterDraws },
    // Baked bounce carries about half of a shaded street's brightness, so every preset keeps it.
    materials: { grain: gpu.grain, bounce: gpu.bounce, reflectionProbes: gpu.reflectionProbes, shopFinish: gpu.shopFinish },
    lighting: { ambientOcclusion: gpu.ambientOcclusion, contours: gpu.contours },
    effects: { bloom: gpu.bloom, outline: gpu.outline, edgeBoost: gpu.edgeBoost, displayAA: gpu.antialiasing !== "none", shopArt: gpu.shopArt },
    loading: { firstFrameShaders: cpu.firstFrameShaders !== "off", ...(cpu.shaderWarmup === "off" ? { warmup: false } : { warmup: true, shaderWarmup: cpu.shaderWarmup }),
      textureWarmupMiB: memory.textureWarmupMB },
    look: gpu.shadowStyle === "authored" ? {} : { shadowStyle: gpu.shadowStyle },
  };
  const diagnostics = readDiagnostics(profile.diagnostics, legacyQuery, true, quality) as RenderDiagnostics;
  const explicit = readDiagnostics(profile.diagnostics, legacyQuery, false) as RenderDiagnosticOverrides;
  const rest = { ...(stated.overrides ? { overrides: stated.overrides } : {}),
    ...(Object.keys(explicit).length ? { diagnostics: explicit } : {}) };
  return { ...resolution, diagnostics, profile: (stated.version === 1 ? { version: 1, preset, ...rest }
    : { version: 2, ...(chosenPlatform ? { platform: chosenPlatform } : {}), quality: preset, ...rest }) as AnyRenderProfile };
}

/** Compact named links, or one structured parameter whenever a link has overrides. */
export function writeRenderProfile(query: URLSearchParams, profile: AnyRenderProfile): void {
  for (const fields of Object.values(diagnosticSchema)) for (const setting of Object.values(fields)) query.delete(setting.legacy);
  query.delete("graphics");
  query.delete("profile");
  // A version 1 profile cannot state a platform, so a link's own platform stays.
  if (profile.version === 2) query.delete("platform");
  const preset = profile.version === 2 ? profile.quality : profile.preset;
  if (profile.overrides || profile.diagnostics) query.set("profile", JSON.stringify(profile));
  else {
    if (preset) query.set("graphics", preset);
    if (profile.version === 2 && profile.platform) query.set("platform", profile.platform);
  }
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
