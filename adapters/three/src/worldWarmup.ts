import * as THREE from "three";

/** Estimate storage for the static 2D maps the glTF viewer can prepare. Leave
 * custom targets, live video, external textures and volume maps to their owner. */
function uploadBytes(texture: THREE.Texture): number {
  const image = texture.image as { width: number; height: number } | null;
  if (texture.isRenderTargetTexture || texture instanceof THREE.VideoTexture || texture instanceof THREE.ExternalTexture
      || texture instanceof THREE.DepthTexture || texture instanceof THREE.CubeTexture
      || texture instanceof THREE.DataArrayTexture || texture instanceof THREE.CompressedArrayTexture
      || texture instanceof THREE.Data3DTexture || texture.version === 0
      || !image || !Number.isSafeInteger(image.width) || !Number.isSafeInteger(image.height)
      || image.width < 1 || image.height < 1) return 0;
  if (texture instanceof THREE.CompressedTexture)
    return texture.mipmaps?.reduce((bytes, mip) => bytes + mip.data.byteLength, 0) ?? 0;
  // glTF bitmap and packed channel maps. Other formats may have a custom
  // internal storage layout, so leave those for the first actual draw.
  if (texture.mipmaps?.length || texture.type !== THREE.UnsignedByteType || !([THREE.RGBAFormat, THREE.RGBFormat, THREE.RGFormat,
    THREE.RedFormat] as number[]).includes(texture.format)) return 0;
  let width = image.width, height = image.height, bytes = 0;
  const channels = texture.format === THREE.RedFormat ? 1 : texture.format === THREE.RGFormat ? 2 : 4;
  if (texture.internalFormat && texture.internalFormat !== ({ 1: "R8", 2: "RG8", 4: "RGBA8" } as const)[channels]) return 0;
  for (;;) {
    bytes += width * height * channels;
    if (!texture.generateMipmaps || width === 1 && height === 1) return bytes;
    width = Math.max(1, Math.floor(width / 2)); height = Math.max(1, Math.floor(height / 2));
  }
}

/** A loaded image the renderer can store now. Targets, live video and external
 * textures belong to their owner. */
function uploadable(texture: THREE.Texture): boolean {
  return !texture.isRenderTargetTexture && !(texture instanceof THREE.VideoTexture) && !(texture instanceof THREE.ExternalTexture)
    && !(texture instanceof THREE.DepthTexture) && texture.version !== 0 && !!texture.image;
}

export interface WarmWorldTextureOptions {
  /** Admit only these meshes. Default: every mesh under the map, including hidden ones. */
  draws?: (mesh: THREE.Mesh) => boolean;
  /** Checked between uploads. An aborted preparation rejects with the signal's reason. */
  signal?: AbortSignal;
  /** Main-thread time before the next upload waits for a new task. Default 12 ms. */
  sliceMs?: number;
  /** Called before the first upload and after each one with the count stored so far. */
  onProgress?: (uploaded: number, total: number) => void;
}

/** Prepare already decoded bindings in order of the nearest placement, including
 * meshes hidden by the current camera unless `draws` narrows them. A material is
 * admitted as a set so that its first visible frame does not have to upload its
 * remaining maps together. An infinite budget admits every material, including
 * maps whose storage size is not estimated. The meshes are read before the first
 * wait, so the caller may restore its camera selection as soon as this returns its promise.
 * This never requests high texture LODs or reads any external assets. */
export async function warmWorldTextures(renderer: THREE.WebGLRenderer, map: THREE.Object3D, eye: THREE.Vector3,
  budgetBytes: number, { draws, signal, sliceMs = 12, onProgress }: WarmWorldTextureOptions = {}) {
  if (Number.isNaN(budgetBytes) || budgetBytes <= 0) return { uploadedTextures: 0, estimatedUploadBytes: 0 };
  const unlimited = budgetBytes === Infinity;
  const materials = new Map<THREE.Material, { distance: number; maps: Map<THREE.Texture, number> }>();
  const box = new THREE.Box3();
  map.updateWorldMatrix(true, true);
  map.traverse(object => {
    if (!(object instanceof THREE.Mesh) || (draws && !draws(object))) return;
    let distance: number | undefined;
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      let entry = materials.get(material);
      if (!entry) {
        const maps = new Map<THREE.Texture, number>();
        for (const value of Object.values(material)) {
          if (!(value instanceof THREE.Texture) || renderer.properties.has(value) || maps.has(value)) continue;
          const bytes = uploadBytes(value);
          if (bytes || (unlimited && uploadable(value))) maps.set(value, bytes);
        }
        entry = { distance: Infinity, maps }; materials.set(material, entry);
      }
      if (!entry.maps.size) continue;
      if (distance === undefined) {
        const bounds = object instanceof THREE.InstancedMesh ? object : object.geometry;
        if (!bounds.boundingBox) bounds.computeBoundingBox();
        distance = box.copy(bounds.boundingBox!).applyMatrix4(object.matrixWorld).distanceToPoint(eye);
      }
      entry.distance = Math.min(entry.distance, distance);
    }
  });
  let uploadedTextures = 0, estimatedUploadBytes = 0;
  const ordered = [...materials.values()].sort((a, b) => a.distance - b.distance);
  let total = 0;
  if (onProgress) {
    total = new Set(ordered.flatMap(({ maps }) => [...maps.keys()])).size;
    onProgress(0, total);
  }
  signal?.throwIfAborted();
  let sliceStart = performance.now();
  for (const { maps } of ordered) {
    const missing = [...maps].filter(([texture]) => !renderer.properties.has(texture));
    const bytes = missing.reduce((sum, [, size]) => sum + size, 0);
    if (bytes > budgetBytes - estimatedUploadBytes) continue;
    estimatedUploadBytes += bytes;
    for (const [texture] of missing) {
      // A single upload can exceed one frame. Let the loading UI repaint
      // between uploads when preparing a large desktop working set.
      if (performance.now() - sliceStart >= sliceMs) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        signal?.throwIfAborted();
        sliceStart = performance.now();
      }
      // Another material, or a draw during a wait, may have stored it already.
      if (renderer.properties.has(texture)) continue;
      renderer.initTexture(texture);
      uploadedTextures++;
      onProgress?.(uploadedTextures, total);
    }
  }
  return { uploadedTextures, estimatedUploadBytes };
}

function withColorTarget<T>(renderer: THREE.WebGLRenderer, target: THREE.WebGLRenderTarget | null, submit: () => T): T {
  const previous = renderer.getRenderTarget(), face = renderer.getActiveCubeFace(), mip = renderer.getActiveMipmapLevel();
  try {
    if (target !== previous) renderer.setRenderTarget(target);
    return submit();
  } finally {
    renderer.setRenderTarget(previous, face, mip);
  }
}

/** Three captures the destination synchronously, then polls its driver jobs. */
export function compileWorldStage(renderer: THREE.WebGLRenderer, stage: THREE.Object3D, camera: THREE.Camera,
  scene: THREE.Scene, target: THREE.WebGLRenderTarget | null) {
  return withColorTarget(renderer, target, () => renderer.compileAsync(stage, camera, scene));
}

const SHADOW_SIDE: Record<number, THREE.Side> = {
  [THREE.FrontSide]: THREE.BackSide, [THREE.BackSide]: THREE.FrontSide, [THREE.DoubleSide]: THREE.DoubleSide };

/** The depth materials Three's shadow pass will draw the casters with, one compile-only
 * mesh for each distinct program. Three derives them from each caster's surface when
 * the pass runs: its side, colour map, cutout and displacement, and the mesh's own
 * vertex layout. The returned materials own the programs until they are disposed. */
export function shadowDepthStage(renderer: Pick<THREE.WebGLRenderer, "shadowMap">, map: THREE.Object3D):
  { stage: THREE.Group; materials: THREE.Material[] } {
  const stage = new THREE.Group(), standIns = new Map<string, THREE.Material>();
  const variance = renderer.shadowMap.type === THREE.VSMShadowMap;
  const visit = (object: THREE.Object3D) => {
    if (object instanceof THREE.Mesh && object.castShadow) {
      for (const surface of Array.isArray(object.material) ? object.material : [object.material]) {
        if (!surface?.visible) continue;
        const { map: color, alphaMap, displacementMap } = surface as THREE.MeshStandardMaterial;
        const custom = object.customDepthMaterial;
        const side = surface.shadowSide ?? (variance ? surface.side : SHADOW_SIDE[surface.side]);
        const alphaTest = surface.alphaToCoverage ? 0.5 : surface.alphaTest;
        const key = [custom?.uuid, side, color?.channel, alphaMap?.channel, alphaTest > 0, displacementMap?.channel,
          (surface as THREE.MeshStandardMaterial).wireframe, object.constructor.name,
          !!(object as THREE.InstancedMesh).instanceColor, !!object.morphTargetInfluences?.length,
          !!object.geometry.getAttribute("uv"), !!object.geometry.getAttribute("uv1")].join("|");
        if (standIns.has(key)) continue;
        // Three writes these fields onto its own depth material, or the caster's custom one, before each draw.
        const depth = custom ? custom.clone() : new THREE.MeshDepthMaterial();
        if (custom) {
          depth.onBeforeCompile = custom.onBeforeCompile;
          depth.customProgramCacheKey = custom.customProgramCacheKey;
        }
        Object.assign(depth, { side, alphaMap: alphaMap ?? null, alphaTest, map: color ?? null,
          displacementMap: displacementMap ?? null, wireframe: (surface as THREE.MeshStandardMaterial).wireframe === true });
        standIns.set(key, depth);
        stage.add(warmupMesh(object, depth));
      }
    }
    for (const child of object.children) if (child.visible) visit(child);
  };
  visit(map);
  return { stage, materials: [...standIns.values()] };
}

export interface FirstColorStages {
  /** Compile-only meshes for variants the first frame selects by camera. They join the
   * scene for the submission only, so they see its lights. */
  variants?: THREE.Object3D;
  /** Compile-only shadow casters from `shadowDepthStage`. */
  shadowCasters?: THREE.Object3D;
}

/** Submit against isolated Three render state. The first real shadow pass must
 * still observe its original light counts, even when the same color shaders are ready.
 * `renderer.compile` prepares every material under the scene, visible or not and
 * wherever the camera points. It does not reach materials a pass substitutes when it
 * draws, which `stages` supplies in the same batch. */
export async function prepareFirstColor(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  target: THREE.WebGLRenderTarget | null, signal?: AbortSignal, timeoutMs = 8_000, stages: FirstColorStages = {}) {
  const gl = renderer.getContext();
  const extension = gl.getExtension("KHR_parallel_shader_compile");
  const info = renderer.info;
  const current = () => !signal?.aborted && !gl.isContextLost() && renderer.getContext() === gl && renderer.info === info;
  if (!extension || !current()) return;
  const start = performance.now(), deadline = start + timeoutMs;
  const before = new Set(info.programs);
  const lighting = new THREE.Scene();
  lighting.fog = scene.fog;
  lighting.background = scene.background;
  lighting.environment = scene.environment;
  lighting.environmentIntensity = scene.environmentIntensity;
  lighting.environmentRotation.copy(scene.environmentRotation);
  const { variants, shadowCasters } = stages;
  try {
    if (variants) { variants.visible = false; scene.add(variants); }
    withColorTarget(renderer, target, () => renderer.compile(scene, camera, lighting));
  } finally { variants?.removeFromParent(); }
  if (shadowCasters?.children.length) {
    // Three draws shadow maps before it counts the frame's lights, into a linear target, with
    // shadow maps declared. An ambient light that "casts" declares them and adds to no count.
    const state = new THREE.Scene(), declared = new THREE.AmbientLight(), depth = new THREE.WebGLRenderTarget(1, 1);
    declared.castShadow = true;
    state.add(declared);
    try { withColorTarget(renderer, depth, () => renderer.compile(shadowCasters, camera, state)); }
    finally { depth.dispose(); }
  }
  const submitted = performance.now();
  const programs = (info.programs ?? []).filter(program => !before.has(program))
    .map(program => ({ program, handle: program.program as WebGLProgram | undefined }));
  const pending = new Set(programs);
  while (pending.size) {
    if (!current() || performance.now() >= deadline) return;
    for (const record of pending) {
      // Three clears the native handle when its last material releases it.
      if (!record.handle || record.program.program !== record.handle || gl.getProgramParameter(record.handle, extension.COMPLETION_STATUS_KHR))
        pending.delete(record);
    }
    if (pending.size) await new Promise<void>(resolve => setTimeout(resolve, 10));
  }
  if (!current()) return;
  const ready = performance.now();
  for (const { program, handle } of programs) if (handle && program.program === handle) program.getUniforms();
  const end = performance.now();
  return { programs: programs.length, submitMs: submitted - start, waitMs: ready - submitted,
    reflectionMs: end - ready, elapsedMs: end - start };
}

/** A compile-only receiver has the real vertex layout and material but needs
 * no instance matrices, transformed vertices or GPU buffers of its own. */
export function warmupMesh(source: THREE.Mesh, material: THREE.Material | THREE.Material[], geometry = source.geometry) {
  let mesh: THREE.Mesh;
  if (source instanceof THREE.InstancedMesh) {
    const instanced = new THREE.InstancedMesh(geometry, material, 0);
    instanced.instanceColor = source.instanceColor;
    instanced.morphTexture = source.morphTexture;
    mesh = instanced;
  } else if (source.constructor === THREE.Mesh) {
    mesh = new THREE.Mesh(geometry, material);
    mesh.layers.mask = source.layers.mask;
  } else {
    mesh = source.clone(false);
    mesh.geometry = geometry;
    mesh.material = material;
  }
  mesh.castShadow = source.castShadow;
  mesh.receiveShadow = source.receiveShadow;
  return mesh;
}

/** After the first world frame, prepare the actual color, glazing and exclusion
 * passes, including attached mesh variants hidden by the loading camera. */
export async function warmWorldColor(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  map: THREE.Object3D, colorTarget: THREE.WebGLRenderTarget | null, budgetBytes: number, otherPasses?: () => Promise<void>) {
  const start = performance.now();
  const before = new Set(renderer.info.programs);
  await compileWorldStage(renderer, scene, camera, scene, colorTarget);
  await otherPasses?.();
  // compileAsync polls the last program on each material. One material can own
  // both instanced and ordinary variants; complete their first-use reflection
  // here as well, including on drivers without KHR_parallel_shader_compile.
  const programs = renderer.info.programs ?? [];
  for (const program of programs) if (!before.has(program)) program.getUniforms();
  const beginTextures = performance.now();
  const stats = await warmWorldTextures(renderer, map, camera.getWorldPosition(new THREE.Vector3()), budgetBytes);
  const end = performance.now();
  return { elapsedMs: end - start, programsBefore: before.size, programsAfter: programs.length,
    textureMs: end - beginTextures, ...stats };
}
