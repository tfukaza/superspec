import * as THREE from "three";

/**
 * Scene fields (roadmap 1D): named top-down fields over the world's XY plane that materials read with
 * `sampleField(name, worldXY)`, first `ground.color` (linear RGB) and `ground.normal` (a world-space unit normal). They
 * are made from the ground receiver's own material and bound as textures: each a float texture whose texel (i, j)
 * stands for the point `min + (i + 0.5, j + 0.5) * (max - min) / size`, row 0 at the lowest Y, sampled bilinearly
 * between texel centers by the program (`ms_sample_field` in the engine's `glsl.rs`, `SceneField::sample` on the CPU).
 *
 * Every program material that reads a field holds the same two uniforms per field (`sceneFieldUniforms`), so a field
 * made again reaches all of them without a recompile. A field nothing supplies reads zero, and that is said once.
 */

/**
 * While 1, program materials draw what a scene field is made of instead of their look: their base color, linear,
 * unlit and unshaded (shade.ts), so a blade that samples it shades it by its own band. 0 is the ordinary look.
 */
export const programFieldPass: THREE.IUniform<number> = { value: 0 };

interface FieldUniforms { texture: THREE.IUniform<THREE.Texture>; rect: THREE.IUniform<THREE.Vector4> }

const fields = new Map<string, FieldUniforms & { supplied: boolean }>();
const reported = new Set<string>();

function empty(): THREE.DataTexture {
  const texture = new THREE.DataTexture(new Float32Array(4), 1, 1, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.needsUpdate = true;
  return texture;
}

/** The uniforms a program binds for field `name` (`msField<k>` and `msFieldRect<k>`): shared by every material. */
export function sceneFieldUniforms(name: string): FieldUniforms {
  let found = fields.get(name);
  if (!found) {
    found = { texture: { value: empty() }, rect: { value: new THREE.Vector4(0, 0, 1, 1) }, supplied: false };
    fields.set(name, found);
  }
  return found;
}

/** Fields a program asked for that nothing has supplied yet. */
export function missingSceneFields(): string[] {
  return [...fields].filter(([, field]) => !field.supplied).map(([name]) => name);
}

/** Says once, on the console, that a program reads a field the scene does not supply. */
export function reportMissingField(name: string, material: string): void {
  if (fields.get(name)?.supplied || reported.has(name)) return;
  reported.add(name);
  console.warn(`Scene field "${name}" is not supplied by this scene; the material ${material} reads zero where it samples it.`);
}

/** Binds a field's texture: `texture` covers the XY rectangle from `min` to `max`, row 0 at `min[1]`. */
export function setSceneField(name: string, texture: THREE.Texture, min: [number, number], max: [number, number]): void {
  sceneFieldUniforms(name);
  const field = fields.get(name)!;
  if (field.supplied && field.texture.value !== texture) field.texture.value.dispose();
  field.texture.value = texture;
  field.rect.value.set(min[0], min[1], max[0], max[1]);
  field.supplied = true;
}

/** A field from values on the CPU (four per texel, rows from the lowest Y), as `SceneField` holds them. */
export function sceneFieldTexture(width: number, height: number, values: Float32Array): THREE.DataTexture {
  const texture = new THREE.DataTexture(values, width, height, THREE.RGBAFormat, THREE.FloatType);
  texture.minFilter = texture.magFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

const NORMAL_VERTEX = /* glsl */`
varying vec3 vWorldNormal;
void main() {
  vWorldNormal = normalize(mat3(modelMatrix) * normal);
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;
const NORMAL_FRAGMENT = /* glsl */`
varying vec3 vWorldNormal;
void main() { gl_FragColor = vec4(normalize(vWorldNormal), 1.0); }`;
const POSITION_VERTEX = /* glsl */`
varying vec3 vWorldPosition;
void main() {
  vWorldPosition = (modelMatrix * vec4(position, 1.0)).xyz;
  gl_Position = projectionMatrix * viewMatrix * vec4(vWorldPosition, 1.0);
}`;
const POSITION_FRAGMENT = /* glsl */`
varying vec3 vWorldPosition;
void main() { gl_FragColor = vec4(vWorldPosition, 1.0); }`;

/** The ground as the last `GroundFields.make` drew it: its world points (alpha 1 where there is ground) and normals. */
export interface GroundPlan { positions: THREE.Texture; normals: THREE.Texture; rect: THREE.Vector4; width: number; height: number }

/**
 * The ground fields of a scene: `ground.color` and `ground.normal`, drawn from straight above over the ground meshes'
 * plan. The color is the ground's own material before lighting and shading: a program material's base color
 * (`programFieldPass`; a painted ground's paint without its shade band, which a blade applies itself from
 * `ground.normal`), or another material's color map times its color. The normal is the geometry's, so the
 * relief of a normal map is not in it. Where several ground meshes overlap in plan, the highest is drawn.
 */
export class GroundFields {
  private color?: THREE.WebGLRenderTarget;
  private normal?: THREE.WebGLRenderTarget;
  /** The ground's world points under each texel (not a scene field: foliage's boundary is made from them). */
  private position?: THREE.WebGLRenderTarget;
  private plan: GroundPlan | null = null;
  private readonly normals = new THREE.ShaderMaterial({ vertexShader: NORMAL_VERTEX, fragmentShader: NORMAL_FRAGMENT });
  private readonly positions = new THREE.ShaderMaterial({ vertexShader: POSITION_VERTEX, fragmentShader: POSITION_FRAGMENT, side: THREE.DoubleSide });

  /** The ground as last drawn, or null before any. */
  ground(): GroundPlan | null { return this.plan; }

  constructor(private readonly renderer: THREE.WebGLRenderer, private readonly resolution = 256) {}

  /**
   * Draws both fields from `ground` (meshes of `scene`, which keep their places; everything else is hidden for the
   * draw) and binds them. Returns the rectangle they cover, or null without a ground.
   */
  make(scene: THREE.Scene, ground: THREE.Mesh[]): { min: [number, number]; max: [number, number] } | null {
    const bounds = new THREE.Box3();
    for (const mesh of ground) bounds.expandByObject(mesh);
    if (bounds.isEmpty()) return null;
    const min: [number, number] = [bounds.min.x, bounds.min.y], max: [number, number] = [bounds.max.x, bounds.max.y];
    const span = Math.max(max[0] - min[0], max[1] - min[1], 1e-3);
    const width = Math.max(1, Math.round(this.resolution * (max[0] - min[0]) / span));
    const height = Math.max(1, Math.round(this.resolution * (max[1] - min[1]) / span));
    const type = this.renderer.extensions.has("EXT_color_buffer_float") ? THREE.FloatType : THREE.HalfFloatType;
    const target = (old?: THREE.WebGLRenderTarget) => {
      if (old && old.width === width && old.height === height) return old;
      old?.dispose();
      const made = new THREE.WebGLRenderTarget(width, height, { type, format: THREE.RGBAFormat, depthBuffer: true });
      made.texture.minFilter = made.texture.magFilter = THREE.NearestFilter;
      made.texture.generateMipmaps = false;
      return made;
    };
    this.color = target(this.color);
    this.normal = target(this.normal);
    this.position = target(this.position);
    const camera = new THREE.OrthographicCamera(min[0], max[0], max[1], min[1], 0.01, bounds.max.z - bounds.min.z + 2);
    camera.position.set(0, 0, bounds.max.z + 1);
    camera.up.set(0, 1, 0);
    // Looking straight down with +Y up from above the origin, the view's x and y are the world's, so the projection's
    // bounds are world XY.
    camera.lookAt(0, 0, bounds.min.z - 1);
    camera.updateMatrixWorld();
    const kept = new Set<THREE.Object3D>(ground);
    for (const mesh of ground) mesh.traverseAncestors(ancestor => kept.add(ancestor));
    const hidden: THREE.Object3D[] = [];
    scene.traverse(object => {
      if (object !== scene && !kept.has(object) && object.visible && !(object as THREE.Light).isLight) { object.visible = false; hidden.push(object); }
    });
    const materials = ground.map(mesh => mesh.material);
    const background = scene.background, previousTarget = this.renderer.getRenderTarget();
    const clear = this.renderer.getClearColor(new THREE.Color()), alpha = this.renderer.getClearAlpha();
    scene.background = null;
    try {
      this.renderer.setClearColor(0x000000, 0);
      // Color: program materials draw their field pass; others an unlit copy of their color.
      const unlit: THREE.Material[] = [];
      ground.forEach(mesh => {
        const material = mesh.material as THREE.MeshStandardMaterial;
        if (material.userData?.programUniforms) return;
        const copy = new THREE.MeshBasicMaterial({ map: material.map ?? null, color: material.color ?? 0xffffff, vertexColors: material.vertexColors, toneMapped: false, side: THREE.DoubleSide });
        unlit.push(copy);
        mesh.material = copy;
      });
      programFieldPass.value = 1;
      this.renderer.setRenderTarget(this.color);
      this.renderer.clear();
      this.renderer.render(scene, camera);
      programFieldPass.value = 0;
      ground.forEach((mesh, index) => { mesh.material = materials[index]; });
      for (const material of unlit) material.dispose();
      for (const mesh of ground) mesh.material = this.normals;
      this.renderer.setRenderTarget(this.normal);
      this.renderer.clear();
      this.renderer.render(scene, camera);
      for (const mesh of ground) mesh.material = this.positions;
      this.renderer.setRenderTarget(this.position);
      this.renderer.clear();
      this.renderer.render(scene, camera);
    } finally {
      programFieldPass.value = 0;
      ground.forEach((mesh, index) => { mesh.material = materials[index]; });
      for (const object of hidden) object.visible = true;
      scene.background = background;
      this.renderer.setRenderTarget(previousTarget);
      this.renderer.setClearColor(clear, alpha);
    }
    setSceneField("ground.color", this.color.texture, min, max);
    setSceneField("ground.normal", this.normal.texture, min, max);
    this.plan = { positions: this.position.texture, normals: this.normal.texture, rect: new THREE.Vector4(min[0], min[1], max[0], max[1]), width, height };
    return { min, max };
  }

  /** The fields' values (four floats per texel, rows from the lowest Y), for inspection and tests. */
  read(name: "ground.color" | "ground.normal"): { width: number; height: number; values: Float32Array } | null {
    const target = name === "ground.color" ? this.color : this.normal;
    if (!target) return null;
    const values = new Float32Array(target.width * target.height * 4);
    if (target.texture.type === THREE.FloatType) this.renderer.readRenderTargetPixels(target, 0, 0, target.width, target.height, values);
    else {
      const half = new Uint16Array(values.length);
      this.renderer.readRenderTargetPixels(target, 0, 0, target.width, target.height, half);
      half.forEach((value, index) => { values[index] = THREE.DataUtils.fromHalfFloat(value); });
    }
    return { width: target.width, height: target.height, values };
  }

  dispose(): void {
    this.color?.dispose();
    this.normal?.dispose();
    this.position?.dispose();
    this.normals.dispose();
    this.positions.dispose();
  }
}

/**
 * The ground receiver of a scene where none is named: the visible, non-instanced mesh with the largest plan area among
 * those that are flat (their height at most a quarter of their smaller plan side). Undefined without one.
 */
export function findGround(meshes: THREE.Mesh[]): THREE.Mesh | undefined {
  let best: THREE.Mesh | undefined, area = 0;
  for (const mesh of meshes) {
    if (!mesh.visible || (mesh as THREE.InstancedMesh).isInstancedMesh) continue;
    const size = new THREE.Box3().setFromObject(mesh).getSize(new THREE.Vector3());
    if (size.z > 0.25 * Math.min(size.x, size.y)) continue;
    if (size.x * size.y > area) { area = size.x * size.y; best = mesh; }
  }
  return best;
}
