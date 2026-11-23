import source from "./graphicsProfiles.json" with { type: "json" };

/** Player-selected runtime budgets. The scene still determines which render effects and LODs exist. */
export interface GraphicsSettings {
  readonly renderScale: number;
  readonly geometryLodBias: number;
  readonly materialLodBias: number;
  readonly materialAnisotropy?: number;
  readonly sunShadowMapSize?: number;
  readonly fixtureShadowMapSize?: number;
  /** Extra triangles a view may draw to remove one LOD band draw. */
  readonly lodBandMergeTriangles: number;
  /** Metres beyond which window glass is left out. Undefined draws all of it. */
  readonly glazingDistance?: number;
}

// The identifiers below are spelled out so the package's declarations do not depend on the data
// file. A test holds them equal to its keys.
export const GRAPHICS_PRESETS = [
  { id: "very-low", label: "Very Low" },
  { id: "low", label: "Low" },
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "very-high", label: "Very High" },
] as const;
export type GraphicsPreset = typeof GRAPHICS_PRESETS[number]["id"];
export const DEFAULT_GRAPHICS_PRESET: GraphicsPreset = "medium";
export const GRAPHICS_RENDER_SCALE = { min: 0.5, max: 1.5 } as const;

/** What a device and connection can afford. Baseline has no caps and is today's behaviour. */
export const GRAPHICS_PLATFORMS = [
  { id: "baseline", label: "Baseline" },
  { id: "phone-cellular", label: "Phone, cellular" },
  { id: "phone-wifi", label: "Phone, Wi-Fi" },
  { id: "laptop-web", label: "Laptop, web" },
  { id: "desktop-web", label: "Desktop, web" },
  { id: "native-pc", label: "Native PC" },
] as const;
export type GraphicsPlatform = typeof GRAPHICS_PLATFORMS[number]["id"];
export const DEFAULT_GRAPHICS_PLATFORM: GraphicsPlatform = "baseline";

/** Published asset variants, lowest first. Each is one publish of a map. */
export const GRAPHICS_VARIANTS = ["minimal", "reduced", "standard", "full"] as const;
export type GraphicsVariant = typeof GRAPHICS_VARIANTS[number];

type Auto = "auto" | "on" | "off";
/**
 * Every resolved parameter, grouped by the resource it spends. `graphicsProfiles.json` declares
 * each one's range, default and whether anything consumes it yet.
 */
export interface GraphicsParameters {
  readonly assets: {
    readonly geometryDetail: 0 | 1 | 2;
    readonly variantBudget: number;
    readonly nearErrorMeters: number;
    readonly farProfile: "standard" | "aggressive";
    readonly texelDensity: number;
    readonly maxPageSize: number;
    readonly maps: {
      readonly normal: "full" | "half" | "strong-only" | "none";
      readonly roughness: "full" | "half" | "constant";
      readonly edge: "separate" | "packed" | "none";
      readonly emissive: "page" | "atlas";
      readonly detail: "on" | "off";
    };
    readonly codec: "webp-lossless" | "jpeg-png" | "uastc" | "etc1s";
    readonly weathering: "unique" | "shared-parts" | "shared-all";
    readonly partInstancing: "off" | "geometry" | "geometry-and-texels";
    readonly materialLayout: "per-page" | "array-per-building";
    readonly meshCompression: "none" | "quantized" | "meshopt";
  };
  readonly delivery: {
    readonly variant: GraphicsVariant;
    readonly textureStreaming: "off" | "pairs";
    readonly firstFrameBudgetMB: number | "none";
    readonly order: "all" | "spawn-cell-first";
    readonly generation: "prebuilt" | "on-device";
    readonly transferEncoding: "none" | "brotli" | "zstd";
  };
  readonly memory: {
    readonly textureBudgetMB: number | "none";
    readonly textureMaxEdge: number | "none";
    readonly geometryBudgetMB: number | "none";
    readonly textureWarmupMB: number;
    readonly releaseSources: boolean;
  };
  readonly gpu: {
    readonly renderScale: number;
    readonly renderScaleFloor: number;
    readonly pixelBudget: number | "none";
    readonly sunShadowSize: number | "authored";
    readonly fixtureShadowSize: number | "authored";
    readonly shadowStyle: "authored" | "plain" | "illustration";
    readonly bounce: "off" | "smooth" | "painted";
    readonly reflectionProbes: boolean;
    readonly ambientOcclusion: Auto;
    readonly contours: Auto;
    readonly outline: Auto;
    readonly bloom: Auto;
    readonly edgeBoost: Auto;
    readonly grain: boolean;
    readonly shopFinish: boolean;
    readonly shopArt: "default" | "off" | "wash" | "brush" | "palette";
    readonly antialiasing: "none" | "display" | "msaa";
    readonly anisotropy: number | "authored";
    readonly materialLodBias: number;
  };
  readonly cpu: {
    readonly geometryLodBias: number;
    /** Extra triangles a view may draw to remove one LOD band draw. */
    readonly lodBandMerge: number;
    /** Metres beyond which window glass is left out. */
    readonly glazingDistance: number | "all";
    readonly partLod: 0 | 1 | 2;
    readonly drawBudget: number | "none";
    readonly occlusionGrid: "fine" | "coarse";
    readonly clusterDraws: "wide" | "pairs" | "off";
    readonly shadowCasters: "static-cached" | "dynamic";
    /** Link the first view's shaders off the main thread before its first frame. */
    readonly firstFrameShaders: "off" | "parallel";
    /** The fuller preparation after the first frame, hidden variants included. */
    readonly shaderWarmup: "off" | "parallel" | "serial";
  };
}

type DeepPartial<T> = { readonly [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] };
/** Individual parameter values a player or a link states. Platform caps still limit them. */
export type GraphicsParameterOverrides = DeepPartial<GraphicsParameters>;
export type GraphicsParameterValue = number | string | boolean;

/** One parameter whose resolved value differs from what the preset or an override asked for. */
export interface GraphicsParameterChange {
  /** Dotted parameter name, such as `gpu.sunShadowSize`. */
  readonly parameter: string;
  readonly requested: GraphicsParameterValue;
  readonly resolved: GraphicsParameterValue;
  /**
   * `cap`: the platform profile limits it. `variant`: it follows the asset variant, which was
   * lowered to fit the resolved asset parameters. `unpublished`: the variant has no pack.
   */
  readonly reason: "cap" | "variant" | "unpublished";
}

export interface GraphicsResolution {
  readonly platform: GraphicsPlatform;
  readonly preset: GraphicsPreset;
  readonly parameters: GraphicsParameters;
  /** The asset variant to fetch. Equal to `parameters.delivery.variant`. */
  readonly variant: GraphicsVariant;
  readonly changed: readonly GraphicsParameterChange[];
  readonly settings: GraphicsSettings;
}

export interface GraphicsResolutionOptions {
  readonly preset?: GraphicsPreset;
  /** Stated by the host or detected. Defaults to the baseline profile. */
  readonly platform?: GraphicsPlatform;
  readonly overrides?: GraphicsParameterOverrides;
  /** Variants the map is published in, when the host knows better than the data file. */
  readonly publishedVariants?: readonly GraphicsVariant[];
}

interface Declaration {
  readonly consumed: boolean;
  readonly values?: readonly GraphicsParameterValue[];
  readonly min?: number;
  readonly max?: number;
  readonly sentinel?: string;
  readonly sentinelValue?: number;
  readonly default: GraphicsParameterValue;
  readonly variantFit?: "atLeast" | "atMost";
}
type Values = Record<string, GraphicsParameterValue>;
interface ProfileData {
  readonly defaultPlatform: GraphicsPlatform;
  readonly parameters: Record<string, Declaration>;
  readonly legacyGroups: Record<"geometry" | "materials" | "shadows", readonly string[]>;
  readonly presets: Record<GraphicsPreset, { readonly values: Values }>;
  readonly platforms: Record<GraphicsPlatform, {
    readonly defaultPreset: GraphicsPreset;
    readonly values: Values;
    readonly caps: Record<string, { readonly min?: number; readonly max?: number }>;
  }>;
  readonly variants: Record<GraphicsVariant, { readonly packSuffix: string | null; readonly assets: Values }>;
}
const data = source as unknown as ProfileData;
const defaults: Values = Object.fromEntries(Object.entries(data.parameters).map(([id, declared]) => [id, declared.default]));

function accepts(declared: Declaration, value: unknown): value is GraphicsParameterValue {
  if (declared.sentinel !== undefined && value === declared.sentinel) return true;
  if (declared.values) return declared.values.includes(value as GraphicsParameterValue);
  return typeof value === "number" && Number.isFinite(value)
    && value >= (declared.min ?? -Infinity) && value <= (declared.max ?? Infinity);
}

/** Read `{ gpu: { sunShadowSize: 512 } }` as `gpu.sunShadowSize`, rejecting unknown names and values. */
function flatten(overrides: unknown, path = "overrides", prefix = "", out: Values = {}): Values {
  if (overrides === null || typeof overrides !== "object" || Array.isArray(overrides)) throw Error(`${path} must be an object.`);
  for (const [key, value] of Object.entries(overrides)) {
    const id = prefix + key, declared = data.parameters[id];
    if (declared) {
      if (!accepts(declared, value)) throw Error(`Invalid ${path}.${key}.`);
      out[id] = value;
    } else if (Object.keys(data.parameters).some(known => known.startsWith(id + "."))) {
      flatten(value, `${path}.${key}`, id + ".", out);
    } else throw Error(`Unknown ${path}.${key}.`);
  }
  return out;
}

function nest(values: Values): Record<string, unknown> {
  const out: Record<string, any> = {};
  for (const [id, value] of Object.entries(values)) {
    const names = id.split("."), leaf = names.pop()!;
    let group = out;
    for (const name of names) group = group[name] ??= {};
    group[leaf] = value;
  }
  return out;
}

export function isGraphicsPlatform(value: unknown): value is GraphicsPlatform {
  return GRAPHICS_PLATFORMS.some(platform => value === platform.id);
}

export function isGraphicsVariant(value: unknown): value is GraphicsVariant {
  return GRAPHICS_VARIANTS.includes(value as GraphicsVariant);
}

/** The preset a platform starts on when the player has not chosen one. */
export function defaultGraphicsPreset(platform: GraphicsPlatform = data.defaultPlatform): GraphicsPreset {
  return data.platforms[platform].defaultPreset;
}

/**
 * Resolve one session's parameters. Later layers win: schema defaults, the platform profile's
 * values, the quality preset, then individual overrides. The platform's caps limit the result.
 * The asset variant is then the highest one, at or below the one asked for, that the resolved
 * asset parameters allow and that is published.
 */
export function resolveGraphicsParameters(options: GraphicsResolutionOptions = {}): GraphicsResolution {
  const platform = options.platform ?? data.defaultPlatform;
  const preset = options.preset ?? data.platforms[platform].defaultPreset;
  const profile = data.platforms[platform];
  const values: Values = { ...defaults, ...profile.values, ...data.presets[preset].values,
    ...(options.overrides ? flatten(options.overrides) : {}) };
  const changed: GraphicsParameterChange[] = [];
  const change = (parameter: string, resolved: GraphicsParameterValue, reason: GraphicsParameterChange["reason"]) => {
    changed.push({ parameter, requested: values[parameter], resolved, reason });
    values[parameter] = resolved;
  };

  for (const [id, cap] of Object.entries(profile.caps)) {
    const declared = data.parameters[id], stated = values[id];
    const value = stated === declared.sentinel ? declared.sentinelValue : stated;
    if (typeof value !== "number") continue;
    const limited = Math.max(cap.min ?? -Infinity, Math.min(cap.max ?? Infinity, value));
    if (limited !== value) change(id, limited, "cap");
  }

  const fitted = Object.entries(data.parameters).filter(([, declared]) => declared.variantFit);
  const built = (variant: GraphicsVariant, id: string) => data.variants[variant].assets[id] ?? defaults[id];
  const fits = (variant: GraphicsVariant) => fitted.every(([id, declared]) => {
    const has = built(variant, id) as number, wanted = values[id] as number;
    return declared.variantFit === "atMost" ? has <= wanted : has >= wanted;
  });
  const published = options.publishedVariants ?? GRAPHICS_VARIANTS.filter(variant => data.variants[variant].packSuffix !== null);
  const asked = GRAPHICS_VARIANTS.indexOf(values["delivery.variant"] as GraphicsVariant);
  let fit = asked;
  while (fit > 0 && !fits(GRAPHICS_VARIANTS[fit])) fit--;
  if (fit !== asked) change("delivery.variant", GRAPHICS_VARIANTS[fit], "variant");
  // Prefer a lighter published variant to a heavier one the device may not afford.
  const available = (index: number) => published.includes(GRAPHICS_VARIANTS[index]);
  let chosen = fit;
  while (chosen >= 0 && !available(chosen)) chosen--;
  if (chosen < 0) chosen = GRAPHICS_VARIANTS.findIndex((_, index) => index > fit && available(index));
  if (chosen >= 0 && chosen !== fit) change("delivery.variant", GRAPHICS_VARIANTS[chosen], "unpublished");
  const variant = values["delivery.variant"] as GraphicsVariant;
  for (const [id] of fitted) if (built(variant, id) !== values[id]) change(id, built(variant, id), "variant");

  // "authored" leaves the scene's own value, which the settings state as undefined.
  const size = (id: string) => { const value = values[id]; return typeof value === "number" ? value : undefined; };
  return {
    platform, preset, parameters: nest(values) as unknown as GraphicsParameters, variant, changed,
    settings: {
      renderScale: values["gpu.renderScale"] as number,
      geometryLodBias: values["cpu.geometryLodBias"] as number,
      materialLodBias: values["gpu.materialLodBias"] as number,
      materialAnisotropy: size("gpu.anisotropy"),
      sunShadowMapSize: size("gpu.sunShadowSize"),
      fixtureShadowMapSize: size("gpu.fixtureShadowSize"),
      lodBandMergeTriangles: values["cpu.lodBandMerge"] as number,
      glazingDistance: size("cpu.glazingDistance"),
    },
  };
}

/** Published mobile packs use these names alongside the full glTF. */
export function mobileGraphicsPreset(preset: GraphicsPreset | undefined): "low" | "very-low" | undefined {
  return preset === "low" || preset === "very-low" ? preset : undefined;
}

/** What a variant's pack inserts before `.glb`: empty for the base pack, undefined when none is published. */
export function graphicsPackSuffix(variant: GraphicsVariant): string | undefined {
  return data.variants[variant].packSuffix ?? undefined;
}

/**
 * The entry of the resolved asset variant beside a map's base entry, or undefined when the base
 * entry is the one to fetch. Pass `resolveRenderProfile(...).variant`. A preset name still works
 * and means that preset's variant on the baseline platform.
 */
export function graphicsPackUrl(url: URL, selection: GraphicsVariant | GraphicsPreset | undefined): URL | undefined {
  if (!selection) return;
  const suffix = graphicsPackSuffix(isGraphicsVariant(selection) ? selection : resolveGraphicsParameters({ preset: selection }).variant);
  const path = url.pathname.toLowerCase();
  const packed = Object.values(data.variants).some(({ packSuffix }) => packSuffix && path.endsWith(packSuffix + ".glb"));
  if (!suffix || !path.endsWith(".glb") || packed) return;
  const variant = new URL(url);
  variant.pathname = url.pathname.slice(0, -4) + suffix + ".glb";
  return variant;
}

/** Version 1 overrides: each group takes another preset's values. */
export interface GraphicsOverrides {
  readonly renderScale?: number;
  readonly geometry?: GraphicsPreset;
  readonly materials?: GraphicsPreset;
  readonly shadows?: GraphicsPreset;
}

/** State version 1 group overrides as the parameter values they stand for. */
export function graphicsParameterOverrides(overrides: GraphicsOverrides): GraphicsParameterOverrides {
  const values: Values = {};
  if (overrides.renderScale !== undefined) values["gpu.renderScale"] = overrides.renderScale;
  for (const [group, ids] of Object.entries(data.legacyGroups) as [keyof typeof data.legacyGroups, readonly string[]][]) {
    const preset = overrides[group];
    if (preset) for (const id of ids) values[id] = data.presets[preset].values[id] ?? defaults[id];
  }
  return nest(values) as GraphicsParameterOverrides;
}

/** Resolve named quality groups independently so a host can build a custom preset. */
export function resolveGraphicsSettings(preset: GraphicsPreset = DEFAULT_GRAPHICS_PRESET, overrides: GraphicsOverrides = {}): GraphicsSettings {
  const { renderScale: scale, ...groups } = overrides;
  return resolveGraphicsParameters({ preset, overrides: graphicsParameterOverrides({ ...groups,
    ...(scale !== undefined && Number.isFinite(scale)
      ? { renderScale: Math.max(GRAPHICS_RENDER_SCALE.min, Math.min(GRAPHICS_RENDER_SCALE.max, scale)) } : {}) }) }).settings;
}
