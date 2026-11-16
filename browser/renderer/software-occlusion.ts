import type * as THREE from "three";

/**
 * POLICY, ASSUMED PENDING REVIEW (2026-11-16, docs/plans/lighting-settings.md): on a software rasterizer ambient
 * occlusion is off unless it is asked for.
 *
 * The occlusion pass costs a software renderer several times the scene itself (it has no GPU to spread the samples
 * over), which is what headless test runs and machines without a usable GPU draw with. So where the lighting record is
 * silent about `ambientOcclusion.enabled` and the page does not say `ao=on`, such a renderer leaves it off; a record
 * that states it, or the page parameter, is honored as on any renderer. The record's default (on) is unchanged.
 *
 * To remove the policy: delete this file and the one call of `softwareOcclusionDefault` in `rendering.ts`.
 */

/** Names of renderers that rasterize on the CPU: SwiftShader (Chrome), llvmpipe and softpipe (Mesa), WARP (Windows). */
const SOFTWARE = /swiftshader|llvmpipe|softpipe|software|\bwarp\b|microsoft basic render/i;

/** The renderer's name when it is a software rasterizer, else null. */
export function softwareRasterizer(renderer: THREE.WebGLRenderer): string | null {
  const gl = renderer.getContext();
  const info = gl.getExtension("WEBGL_debug_renderer_info");
  const name = String(info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER));
  return SOFTWARE.test(name) ? name : null;
}

/**
 * Why ambient occlusion that nobody asked for stays off on this renderer, or null when it is drawn. `asked` says the
 * record as stated, or the page, turns it on itself.
 */
export function softwareOcclusionDefault(renderer: THREE.WebGLRenderer, asked: boolean): string | null {
  if (asked) return null;
  const software = softwareRasterizer(renderer);
  return software ? `off by default on the software renderer ${software}; a record that states ambientOcclusion.enabled, or ?ao=on, turns it on` : null;
}
