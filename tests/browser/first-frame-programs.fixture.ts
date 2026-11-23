import * as THREE from "three";

/** A WebGL2 context that accepts every call, for Three's own material, program-cache and
 * render-list logic to run in Node. Programs "link" at once and declare no uniforms or
 * attributes. `linked` counts `linkProgram` calls. */
export function recordingRenderer() {
  const constants = new Map<string, number>(), names = new Map<number, string>();
  const stats = { linked: 0 };
  const parameters: Record<string, unknown> = {
    VERSION: "WebGL 2.0 (recording)", SHADING_LANGUAGE_VERSION: "WebGL GLSL ES 3.00",
    MAX_TEXTURE_SIZE: 4096, MAX_CUBE_MAP_TEXTURE_SIZE: 4096, MAX_TEXTURE_IMAGE_UNITS: 16, MAX_COMBINED_TEXTURE_IMAGE_UNITS: 32,
    MAX_VERTEX_TEXTURE_IMAGE_UNITS: 16, MAX_VERTEX_ATTRIBS: 16, MAX_VERTEX_UNIFORM_VECTORS: 1024, MAX_FRAGMENT_UNIFORM_VECTORS: 1024,
    MAX_VARYING_VECTORS: 30, MAX_SAMPLES: 4, MAX_3D_TEXTURE_SIZE: 256, MAX_ARRAY_TEXTURE_LAYERS: 256, MAX_RENDERBUFFER_SIZE: 4096,
    MAX_DRAW_BUFFERS: 8, MAX_COLOR_ATTACHMENTS: 8,
    SCISSOR_BOX: new Int32Array([0, 0, 64, 64]), VIEWPORT: new Int32Array([0, 0, 64, 64]),
  };
  const calls: Record<string, (...args: any[]) => unknown> = {
    getParameter: (id: number) => parameters[names.get(id) ?? ""] ?? 0,
    getContextAttributes: () => ({ alpha: true, antialias: false, depth: true, stencil: false, premultipliedAlpha: true }),
    getExtension: (name: string) => name === "KHR_parallel_shader_compile" ? { COMPLETION_STATUS_KHR: 0x91b1 } : null,
    getSupportedExtensions: () => ["KHR_parallel_shader_compile"],
    getShaderPrecisionFormat: () => ({ precision: 23, rangeMin: 127, rangeMax: 127 }),
    getProgramParameter: (_program: unknown, id: number) => id === 0x91b1 || ["LINK_STATUS", "VALIDATE_STATUS"].includes(names.get(id)!) ? true : 0,
    getShaderParameter: () => true, getProgramInfoLog: () => "", getShaderInfoLog: () => "",
    checkFramebufferStatus: () => constants.get("FRAMEBUFFER_COMPLETE"), isContextLost: () => false, getError: () => 0,
    linkProgram: () => { stats.linked++; },
  };
  const gl = new Proxy({}, { get(_, property) {
    if (typeof property !== "string") return undefined;
    if (property === "canvas") return canvas;
    if (/^[A-Z][A-Z0-9_]*$/.test(property)) {
      if (!constants.has(property)) { constants.set(property, 0x10000 + constants.size); names.set(constants.get(property)!, property); }
      return constants.get(property);
    }
    return calls[property] ??= property.startsWith("create") ? () => ({}) : () => undefined;
  } }) as unknown as WebGL2RenderingContext;
  const canvas = Object.assign(new EventTarget(), { width: 64, height: 64, style: {}, clientWidth: 64, clientHeight: 64 }) as unknown as HTMLCanvasElement;
  const renderer = new THREE.WebGLRenderer({ canvas, context: gl });
  renderer.debug.checkShaderErrors = false;
  return { renderer, stats };
}
