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

/** Prepare already decoded bindings in order of the nearest placement, including
 * meshes hidden by the current camera. A material is admitted as a set so that
 * its first visible frame does not have to upload its remaining maps together.
 * This never requests high texture LODs or reads any external assets. */
export async function warmWorldTextures(renderer: THREE.WebGLRenderer, map: THREE.Object3D, eye: THREE.Vector3,
  budgetBytes: number) {
  if (!Number.isFinite(budgetBytes) || budgetBytes <= 0) return { uploadedTextures: 0, estimatedUploadBytes: 0 };
  const materials = new Map<THREE.Material, { distance: number; maps: Map<THREE.Texture, number> }>();
  const box = new THREE.Box3();
  map.updateWorldMatrix(true, true);
  map.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return;
    let distance: number | undefined;
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      let entry = materials.get(material);
      if (!entry) {
        const maps = new Map<THREE.Texture, number>();
        for (const value of Object.values(material)) {
          if (!(value instanceof THREE.Texture) || renderer.properties.has(value) || maps.has(value)) continue;
          const bytes = uploadBytes(value);
          if (bytes) maps.set(value, bytes);
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
  let sliceStart = performance.now();
  for (const { maps } of [...materials.values()].sort((a, b) => a.distance - b.distance)) {
    const missing = [...maps].filter(([texture]) => !renderer.properties.has(texture));
    const bytes = missing.reduce((sum, [, size]) => sum + size, 0);
    if (bytes > budgetBytes - estimatedUploadBytes) continue;
    for (const [texture] of missing) {
      // A single upload can exceed one frame. Let the loading UI repaint
      // between uploads when preparing a large desktop working set.
      if (performance.now() - sliceStart >= 12) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        sliceStart = performance.now();
      }
      renderer.initTexture(texture);
    }
    uploadedTextures += missing.length;
    estimatedUploadBytes += bytes;
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

/** Submit against isolated Three render state. The first real shadow pass must
 * still observe its original light counts, even when the same color shaders are ready. */
export async function prepareFirstColor(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera,
  target: THREE.WebGLRenderTarget | null, signal?: AbortSignal, timeoutMs = 8_000) {
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
  withColorTarget(renderer, target, () => renderer.compile(scene, camera, lighting));
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
export function warmupMesh(source: THREE.Mesh, material: THREE.Material | THREE.Material[]) {
  if (source instanceof THREE.InstancedMesh) {
    const mesh = new THREE.InstancedMesh(source.geometry, material, 0);
    mesh.instanceColor = source.instanceColor;
    mesh.morphTexture = source.morphTexture;
    return mesh;
  }
  const mesh = source.clone(false);
  mesh.material = material;
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
