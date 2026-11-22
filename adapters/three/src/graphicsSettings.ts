/** Player-selected runtime budgets. The scene still determines which render effects and LODs exist. */
export interface GraphicsSettings {
  readonly renderScale: number;
  readonly geometryLodBias: number;
  readonly materialLodBias: number;
  readonly materialAnisotropy?: number;
  readonly sunShadowMapSize?: number;
  readonly fixtureShadowMapSize?: number;
}

export const GRAPHICS_PRESETS = [
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "very-high", label: "Very High" },
] as const;
export type GraphicsPreset = typeof GRAPHICS_PRESETS[number]["id"];
export const DEFAULT_GRAPHICS_PRESET: GraphicsPreset = "medium";
export const GRAPHICS_RENDER_SCALE = { min: 0.5, max: 1.5 } as const;

const presets: Record<GraphicsPreset, GraphicsSettings> = {
  medium: { renderScale: 1, geometryLodBias: 1, materialLodBias: 1 },
  high: { renderScale: 1, geometryLodBias: 1.2, materialLodBias: 1.25, materialAnisotropy: 8, sunShadowMapSize: 4096 },
  "very-high": { renderScale: 1.25, geometryLodBias: 1.4, materialLodBias: 1.5, materialAnisotropy: 16,
    sunShadowMapSize: 4096, fixtureShadowMapSize: 1024 },
};

export interface GraphicsOverrides {
  readonly renderScale?: number;
  readonly geometry?: GraphicsPreset;
  readonly materials?: GraphicsPreset;
  readonly shadows?: GraphicsPreset;
}

/** Resolve named quality groups independently so a host can build a custom preset. */
export function resolveGraphicsSettings(preset: GraphicsPreset = DEFAULT_GRAPHICS_PRESET, overrides: GraphicsOverrides = {}): GraphicsSettings {
  const base = presets[preset];
  const geometry = presets[overrides.geometry ?? preset];
  const materials = presets[overrides.materials ?? preset];
  const shadows = presets[overrides.shadows ?? preset];
  const scale = overrides.renderScale;
  return {
    renderScale: scale !== undefined && Number.isFinite(scale)
      ? Math.max(GRAPHICS_RENDER_SCALE.min, Math.min(GRAPHICS_RENDER_SCALE.max, scale)) : base.renderScale,
    geometryLodBias: geometry.geometryLodBias,
    materialLodBias: materials.materialLodBias,
    materialAnisotropy: materials.materialAnisotropy,
    sunShadowMapSize: shadows.sunShadowMapSize,
    fixtureShadowMapSize: shadows.fixtureShadowMapSize,
  };
}
