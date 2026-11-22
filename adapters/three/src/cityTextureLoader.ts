import * as THREE from "three";
import type { GLTF, GLTFLoader, GLTFLoaderPlugin, GLTFParser } from "three/addons/loaders/GLTFLoader.js";

export type CityTextureSlot = "map" | "normalMap" | "metalnessMap" | "roughnessMap";
export interface CityTextureBinding {
  slot: CityTextureSlot;
  low: THREE.Texture;
  /** Adapter-owned clone, shared only by bindings of this exact low texture.
   * A host may dispose its GPU allocation on demotion and request it again. */
  loadHigh(): Promise<THREE.Texture>;
}
type Binding = { low: THREE.Texture; near: number; high?: Promise<THREE.Texture>; loadHigh(): Promise<THREE.Texture> };
const slots: readonly CityTextureSlot[] = ["map", "normalMap", "metalnessMap", "roughnessMap"];
const registries = new WeakMap<GLTFParser, CityTextureLoader>();
const installed = new WeakSet<GLTFLoader>();

/** Only standard, single-purpose texture bindings may use the optional pair.
 * Unknown texture references and aliases keep their original source contract. */
function pairsFor(json: any): { pairs: Map<number, number>; infos: WeakMap<object, Set<string>> } {
  const record = json.scenes?.[json.scene ?? 0]?.extras?.worldSpecTextureLod;
  const pairs = new Map<number, number>(), infos = new WeakMap<object, Set<string>>();
  if (record?.version !== 1 || !Array.isArray(record.pairs)) return { pairs, infos };
  const textures = json.textures ?? [], images = json.images ?? [];
  const roles = new Map<number, Set<string>>(), blocked = new Set<number>(), referenced = new Set<number>();
  const add = (info: any, slot: CityTextureSlot, role: string, streamable: boolean) => {
    if (!info || !Number.isInteger(info.index)) return;
    const slots = infos.get(info) ?? new Set<string>();
    if (streamable) slots.add(slot);
    infos.set(info, slots);
    const set = roles.get(info.index) ?? new Set<string>();
    set.add(role); roles.set(info.index, set); referenced.add(info.index);
  };
  // GLTFLoader can reuse one material on triangles and on lines or points,
  // whose runtime materials are outside the mesh residency controller.
  const nonTriangleMaterials = new Set<number>();
  for (const mesh of json.meshes ?? []) for (const primitive of mesh.primitives ?? []) {
    if (primitive.mode !== undefined && ![4, 5, 6].includes(primitive.mode)) nonTriangleMaterials.add(primitive.material);
  }
  for (const [index, material] of (json.materials ?? []).entries()) {
    const pbr = material.pbrMetallicRoughness;
    const streamable = !nonTriangleMaterials.has(index) && !material.extensions?.KHR_materials_unlit;
    add(pbr?.baseColorTexture, "map", "color", streamable);
    add(material.normalTexture, "normalMap", "normal", streamable);
    add(pbr?.metallicRoughnessTexture, "metalnessMap", "data", streamable);
    add(pbr?.metallicRoughnessTexture, "roughnessMap", "data", streamable);
  }
  const opaque = new Set<number>();
  const scan = (value: any, unknown = false): void => {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (key === "worldSpecTextureLod") continue;
      if (/texture/i.test(key) && key !== "textures") {
        const index = typeof child === "number" ? child : (child as any)?.index;
        if (Number.isInteger(index)) {
          referenced.add(index);
          if (!child || typeof child !== "object" || !infos.has(child)) blocked.add(index);
        }
      }
      if (unknown && Number.isInteger(child)) opaque.add(child as number);
      scan(child, unknown || key === "extras" || key === "extensions");
    }
  };
  scan(json);
  const imageRoles = new Map<number, Set<string>>(), blockedImages = new Set<number>();
  for (const index of blocked) blockedImages.add(textures[index]?.source);
  for (const [index, usage] of roles) {
    const source = textures[index]?.source, set = imageRoles.get(source) ?? new Set<string>();
    for (const role of usage) set.add(role);
    imageRoles.set(source, set);
  }
  const nearCounts = new Map<number, number>(), farCounts = new Map<number, number>();
  for (const p of record.pairs) {
    if (!p) continue;
    nearCounts.set(p.near, (nearCounts.get(p.near) ?? 0) + 1);
    farCounts.set(p.far, (farCounts.get(p.far) ?? 0) + 1);
  }
  for (const p of record.pairs) {
    if (!p || !Number.isInteger(p.near) || !Number.isInteger(p.far) || p.near === p.far
      || nearCounts.get(p.near) !== 1 || farCounts.get(p.far) !== 1 || referenced.has(p.far)) continue;
    const near = textures[p.near], far = textures[p.far];
    if (!near || !far || !roles.has(p.near) || near.sampler !== far.sampler
      || !Number.isInteger(near.source) || !Number.isInteger(far.source) || near.source === far.source
      || !images[near.source] || !images[far.source] || blocked.has(p.near) || blockedImages.has(near.source)
      || opaque.has(p.near) || opaque.has(near.source)
      || imageRoles.get(near.source)?.size !== 1
      || [near, far, images[near.source], images[far.source]].some(v => Object.keys(v.extensions ?? {}).length)) continue;
    pairs.set(p.near, p.far);
  }
  return { pairs, infos };
}

/** Copy binding state after theme adapters have adjusted filtering or UVs.
 * Source, format and mip chain remain the exact full-resolution glTF image. */
function copyBinding(high: THREE.Texture, low: THREE.Texture): void {
  const uploadChanged = high.wrapS !== low.wrapS || high.wrapT !== low.wrapT
    || high.magFilter !== low.magFilter || high.minFilter !== low.minFilter || high.anisotropy !== low.anisotropy
    || high.flipY !== low.flipY || high.premultiplyAlpha !== low.premultiplyAlpha
    || high.unpackAlignment !== low.unpackAlignment || high.colorSpace !== low.colorSpace;
  high.mapping = low.mapping; high.channel = low.channel;
  high.wrapS = low.wrapS; high.wrapT = low.wrapT;
  high.magFilter = low.magFilter; high.minFilter = low.minFilter; high.anisotropy = low.anisotropy;
  high.offset.copy(low.offset); high.repeat.copy(low.repeat); high.center.copy(low.center);
  high.rotation = low.rotation; high.matrixAutoUpdate = low.matrixAutoUpdate; high.matrix.copy(low.matrix);
  high.flipY = low.flipY; high.premultiplyAlpha = low.premultiplyAlpha;
  high.unpackAlignment = low.unpackAlignment; high.colorSpace = low.colorSpace;
  if (uploadChanged) high.needsUpdate = true;
}

class CityTextureLoader implements GLTFLoaderPlugin {
  readonly name = "WorldSpecTextureLod";
  private readonly bindings = new WeakMap<THREE.Texture, Binding>();
  private readonly views = new WeakMap<THREE.Texture, Map<string, THREE.Texture>>();
  private readonly owned = new Set<THREE.Texture>();
  hasBindings = false;
  private disposed = false;
  constructor(private readonly parser: GLTFParser) {
    const { pairs, infos } = pairsFor(parser.json);
    const assign = parser.assignTexture.bind(parser);
    // Intercept the binding, not loadTexture: built-in image extensions run
    // before custom loadTexture hooks. No source JSON is ever rewritten.
    parser.assignTexture = async (params, slot, info, colorSpace) => {
      const far = pairs.get(info.index);
      if (this.disposed || far === undefined || !infos.get(info)?.has(slot)) return assign(params, slot, info, colorSpace);
      let low: THREE.Texture | null = null;
      try { low = await assign(params, slot, { ...info, index: far }, colorSpace); } catch { /* portable fallback below */ }
      if (!low) return assign(params, slot, info, colorSpace);
      // Identical coarse PNGs may represent different full-resolution sources.
      // GLTFLoader caches by image/sampler and mutates colorSpace on assignment;
      // isolate bound views while retaining the same decoded image Source.
      const key = `${info.index}:${colorSpace ?? THREE.NoColorSpace}`;
      const views = this.views.get(low) ?? new Map<string, THREE.Texture>();
      this.views.set(low, views);
      let view = views.get(key);
      if (!view) {
        view = low.clone();
        view.colorSpace = colorSpace ?? THREE.NoColorSpace;
        const reference = parser.associations.get(low);
        if (reference) parser.associations.set(view, reference);
        views.set(key, view);
      }
      low = view; params[slot] = low;
      if (!this.bindings.has(low)) {
        const binding: Binding = { low, near: info.index, loadHigh: () => this.loadHigh(binding) };
        this.bindings.set(low, binding);
        this.hasBindings = true;
      }
      return low;
    };
  }
  forMaterial(material: THREE.Material): CityTextureBinding[] {
    if (this.disposed) return [];
    const result: CityTextureBinding[] = [];
    for (const slot of slots) {
      const texture = (material as THREE.MeshStandardMaterial)[slot];
      const binding = texture && this.bindings.get(texture);
      if (binding) result.push({ slot, low: binding.low, loadHigh: binding.loadHigh });
    }
    return result;
  }
  private async loadHigh(binding: Binding): Promise<THREE.Texture> {
    if (this.disposed) throw new DOMException("Texture LOD scene was disposed", "AbortError");
    if (!binding.high) {
      binding.high = this.parser.getDependency("texture", binding.near).then((source: THREE.Texture | null) => {
        if (this.disposed) throw new DOMException("Texture LOD scene was disposed", "AbortError");
        if (!source) throw new Error(`Full-resolution glTF texture ${binding.near} could not be loaded`);
        const high = source.clone();
        this.owned.add(high); this.bindings.set(high, binding);
        return high;
      }).catch(error => { binding.high = undefined; throw error; });
    }
    const high = await binding.high;
    if (this.disposed) throw new DOMException("Texture LOD scene was disposed", "AbortError");
    copyBinding(high, binding.low);
    return high;
  }
  dispose(texturesReleased = false): void {
    this.disposed = true;
    if (!texturesReleased) for (const texture of this.owned) texture.dispose();
    this.owned.clear();
  }
}

/** Opt-in initial coarse loading. Each parse receives an independent registry;
 * ordinary GLTFLoader consumers keep the published full-resolution bindings. */
export function registerCityTextureLod<T extends GLTFLoader>(loader: T): T {
  if (!installed.has(loader)) {
    installed.add(loader);
    loader.register(parser => {
      const plugin = new CityTextureLoader(parser);
      registries.set(parser, plugin);
      return plugin;
    });
  }
  return loader;
}

export function cityTextureBindings(gltf: GLTF, material: THREE.Material): CityTextureBinding[] {
  return registries.get(gltf.parser)?.forMaterial(material) ?? [];
}

/** Only decoded materials that actually substituted a coarse image need residency. */
export function hasCityTextureBindings(gltf: GLTF): boolean {
  return registries.get(gltf.parser)?.hasBindings === true;
}

/** Call before disposing a parsed scene. Pending requests reject with AbortError;
 * core glTF textures and all other parsed scenes retain their own ownership. */
export function disposeCityTextureLod(gltf: GLTF, options: { texturesReleased?: boolean } = {}): void {
  registries.get(gltf.parser)?.dispose(options.texturesReleased);
  registries.delete(gltf.parser);
}
