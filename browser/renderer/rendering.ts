import { shadowStyleFromQuery, type ShadowStyleParameters } from "./shadow-style.ts";

/** Screen-space ambient occlusion of a scene view. Lengths are meters in the scene, so the look does not change with zoom. */
export interface AmbientOcclusionParameters {
  /** How far around a point occluders are looked for, meters. */
  radius: number;
  /** Strength: up to 1, the share of the occlusion that darkens the image; above 1, creases deepen further (up to 4). */
  intensity: number;
  /** How thick surfaces are taken to be behind what the camera sees, meters: thin parts then cast no halo on what is behind them. */
  thickness: number;
}

/**
 * How a scene view is rendered, apart from its models and materials: global settings of the scene, never part of a
 * material. A viewer holds one of these; a page's query parameters fill it today, and a scene-level record in the
 * model packet can carry the same values later.
 */
export interface RenderingSettings {
  /** Whether the sun casts shadows at all. */
  shadows: boolean;
  /** The look of those shadows, or null for three.js's plain ones. */
  shadowStyle: ShadowStyleParameters | null;
  /** Ambient occlusion, or null for none. */
  ambientOcclusion: AmbientOcclusionParameters | null;
}

export const AMBIENT_OCCLUSION: AmbientOcclusionParameters = { radius: 0.35, intensity: 2, thickness: 0.5 };

/**
 * The Studio viewer's defaults (styled shadows and ambient occlusion on) with what the page's parameters change:
 * `shadows=off`, `shadowstyle=plain` and the `shadow*` overrides (see `shadowStyleFromQuery`), `ao=off`, `aoradius`,
 * `aointensity` and `aothickness`.
 */
export function renderingFromQuery(query: URLSearchParams): RenderingSettings {
  const shadows = query.get("shadows") !== "off";
  const number = (name: string, fallback: number, min: number, max: number): number => {
    const value = Number(query.get(name) ?? NaN);
    return Number.isFinite(value) ? Math.min(Math.max(value, min), max) : fallback;
  };
  return {
    shadows,
    shadowStyle: shadows ? shadowStyleFromQuery(query) : null,
    ambientOcclusion: query.get("ao") === "off" ? null : {
      radius: number("aoradius", AMBIENT_OCCLUSION.radius, 0.01, 10),
      intensity: number("aointensity", AMBIENT_OCCLUSION.intensity, 0, 4),
      thickness: number("aothickness", AMBIENT_OCCLUSION.thickness, 0.01, 10),
    },
  };
}
