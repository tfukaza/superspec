import * as THREE from "three";
import { CityOccluderFaces, usesCityDepthTest, writesSolidCitySurface } from "./cityOccluderFaces.ts";
import { hasDeclaredLodOwner } from "./lodOwnership.ts";
import type { CityLodLevels } from "./cityLods.ts";
import type { CityFarHardware } from "./cityFarHardware.ts";
import { copyInstanceTint, equalInstanceTints, instanceTintLayout, sameInstanceTintLayout, uploadInstanceTints } from "./instanceTint.ts";

export type CityDetailCullOptions = {
  /** Largest individual component diameter in drawing-buffer pixels. Default 6. */
  maxComponentPixels?: number;
  /** Sum of silhouette area bounds in drawing-buffer pixels squared. Default 512. */
  maxCoveragePixels?: number;
  /** Camera distance in metres. Default 40; values below 24 are clamped. */
  minDistance?: number;
};
type CityDetailCullBudget = Required<CityDetailCullOptions>;

function detailCullBudget(options?: CityDetailCullOptions): CityDetailCullBudget | undefined {
  if (!options) return;
  const positive = (value: number | undefined, fallback: number) =>
    value !== undefined && Number.isFinite(value) && value > 0 ? value : fallback;
  return { maxComponentPixels: positive(options.maxComponentPixels, 6),
    maxCoveragePixels: positive(options.maxCoveragePixels, 512),
    minDistance: Math.max(24, positive(options.minDistance, 40)) };
}

/** Semantic admission is narrower than LOD admission. Architectural mass,
 * lettering, foliage and lighting retain geometry at every distance. */
function disposableCityDetail(material: THREE.Material): boolean {
  const surface = material.userData.worldSpecSurface;
  const eligible = surface?.recipe === "stock-hardware"
    || surface?.family === "Building atlases" && surface?.surface === "Mixed surfaces with metal"
      && surface?.recipe === "weathered-steel";
  const standard = material as THREE.MeshStandardMaterial;
  const finishes = material.userData.sourceFinishes;
  return eligible && !material.transparent && !material.alphaTest && !material.alphaHash
    && !standard.emissiveMap && !(standard.emissive && standard.emissiveIntensity > 0 && standard.emissive.getHex() !== 0)
    && !material.userData.cityDetailKeep
    && (finishes === undefined || Array.isArray(finishes) && finishes.every(finish => typeof finish === "string"
      && !/\b(?:sign|signage|lettering|poster|posters|tube-light|lens|emissive)\b/i.test(finish)));
}

/** Component area bounds use pi*diameter²/4. The projection Jacobian includes
 * off-axis perspective and the nearest placement depth. A hidden group must
 * grow 25% in projected dimensions to return, avoiding threshold flicker. */
export function omitCityDetail(box: THREE.Box3, diameter: number, area: number, distance: number,
  depth: number, camera: THREE.PerspectiveCamera, viewportHeight: number,
  hidden: boolean, budget: CityDetailCullBudget | undefined): boolean {
  if (!budget || !(diameter > 0) || !(area > 0) || !Number.isFinite(diameter + area)
    || !(viewportHeight > 0) || !(depth > camera.near) || distance < Math.max(24, budget.minDistance * (hidden ? .9 : 1))) return false;
  const m = camera.matrixWorldInverse.elements;
  const cx = (box.min.x + box.max.x) * .5, cy = (box.min.y + box.max.y) * .5, cz = (box.min.z + box.max.z) * .5;
  const hx = (box.max.x - box.min.x) * .5, hy = (box.max.y - box.min.y) * .5, hz = (box.max.z - box.min.z) * .5;
  const x = Math.abs(m[0] * cx + m[4] * cy + m[8] * cz + m[12]) + Math.abs(m[0]) * hx + Math.abs(m[4]) * hy + Math.abs(m[8]) * hz;
  const y = Math.abs(m[1] * cx + m[5] * cy + m[9] * cz + m[13]) + Math.abs(m[1]) * hx + Math.abs(m[5]) * hy + Math.abs(m[9]) * hz;
  const focal = viewportHeight * Math.max(Math.abs(camera.projectionMatrix.elements[5]), Math.abs(camera.projectionMatrix.elements[0]) * camera.aspect) / 2;
  const scale = focal / depth
    * Math.sqrt(1 + (x * x + y * y) / (depth * depth));
  const hysteresis = hidden ? 1.25 : 1;
  return diameter * scale <= budget.maxComponentPixels * hysteresis
    && area * scale * scale <= budget.maxCoveragePixels * hysteresis * hysteresis;
}

const FACE_STRIDE = 14; // four vertex slots, vertex count, double-sided flag
const MAX_SHADOW_CAMERAS = 8;
const DEPTH_TILE = 4;
const PLACEMENT_CELL_METERS = 8;
const MIN_CELL_PLACEMENTS = 4;
const CELL_BOUND_MARGIN = 1e-12;
const CELL_PROBE_COUNT = 24;
const MIN_VISIBLE_CELL_PROBES = 4;

function cellProbeOrder(value: number): number {
  value = Math.imul(value ^ value >>> 16, 0x45d9f3b);
  value = Math.imul(value ^ value >>> 16, 0x45d9f3b);
  return (value ^ value >>> 16) >>> 0;
}

/** Extra triangles one view may submit to remove one draw call. Each LOD band
 * of an instanced batch is its own draw. A coarser band joins the next finer
 * band the camera already selected when promoting its placements costs no
 * more than this. Promotion only ever draws more detail than was selected.
 *
 * Measured on the published Stoa city at Low, 1920x1080 (draws, triangles):
 * budget      street        city overview
 * none     672, 838k     1,366, 1,558k
 * 250      595, 845k     1,089, 1,580k
 * 1000     566, 867k     1,001, 1,627k
 * 4000     535, 927k       931, 1,801k
 * 16000    523, 995k       890, 2,157k
 * Up to 1000 each removed draw costs under 800 triangles. Beyond it the next
 * draws cost 1,800 and then 3,500 triangles each. */
export const CITY_LOD_MERGE_TRIANGLES = 1000;

/** `level` is the selected LOD and carries its hysteresis. `draw` is the level
 * submitted and rasterized into the occlusion buffer, never coarser than `level`. */
type Placement = { lodBox: THREE.Box3; box: THREE.Box3; corners: Float64Array; level: number; draw: number; diameter: number;
  detailArea: number; detailHidden: boolean; blockHidden?: boolean; far?: ReturnType<CityFarHardware["member"]>;
  cell?: PlacementCell };
/** One instanced batch's triangles per instance at each level, level 0 first. */
type LodBands = { mesh: THREE.Mesh; placements: Placement[]; triangles: number[]; counts: number[] };
/** Batches split from one primitive merge bands together. Continuous surfaces
 * keep the same level on both sides of a split, and equal placement lists keep joining. */
type LodBandGroup = { sources: LodBands[]; target: number[] };
type PlacementCell = { box: THREE.Box3; view: number; keep: boolean };
export type CityInstanceGroups = WeakMap<THREE.Mesh, readonly (THREE.Object3D | undefined)[]>;

function customMeshCallbacks(mesh: THREE.Mesh): boolean {
  const prototype = mesh instanceof THREE.InstancedMesh ? THREE.InstancedMesh.prototype : THREE.Mesh.prototype;
  return (["onBeforeRender", "onAfterRender", "onBeforeShadow", "onAfterShadow", "raycast"] as const)
    .some(callback => mesh[callback] !== prototype[callback]);
}

function placementFor(mesh: THREE.Mesh, transform: THREE.Matrix4, wholeDetail = false): Placement {
  const local = mesh.geometry.boundingBox!;
  const corners = new Float64Array(24);
  const point = new THREE.Vector3();
  for (let i = 0; i < 8; i++) {
    point.set(i & 1 ? local.max.x : local.min.x, i & 2 ? local.max.y : local.min.y,
      i & 4 ? local.max.z : local.min.z).applyMatrix4(transform).toArray(corners, 3 * i);
  }
  const box = local.clone().applyMatrix4(transform);
  const lodBox = (mesh.geometry.userData.lodDistanceBounds as THREE.Box3 | undefined)?.clone().applyMatrix4(transform) ?? box;
  const componentDiameter = mesh.geometry.userData.cityLodComponentDiameter as number | undefined;
  let diameter = lodBox.getSize(point).length();
  let detailArea = 0;
  if (!wholeDetail && componentDiameter !== undefined && Number.isFinite(componentDiameter) && componentDiameter >= 0) {
    const e = transform.elements;
    // sqrt(||M||1 * ||M||infinity) bounds transformed diameter even with shear.
    const columns = Math.max(Math.abs(e[0]) + Math.abs(e[1]) + Math.abs(e[2]),
      Math.abs(e[4]) + Math.abs(e[5]) + Math.abs(e[6]), Math.abs(e[8]) + Math.abs(e[9]) + Math.abs(e[10]));
    const rows = Math.max(Math.abs(e[0]) + Math.abs(e[4]) + Math.abs(e[8]),
      Math.abs(e[1]) + Math.abs(e[5]) + Math.abs(e[9]), Math.abs(e[2]) + Math.abs(e[6]) + Math.abs(e[10]));
    diameter = componentDiameter * Math.sqrt(columns * rows);
    if (mesh.frustumCulled && mesh.geometry.userData.cityLodIndependent === true && !Array.isArray(mesh.material)
      && disposableCityDetail(mesh.material)) detailArea = mesh.geometry.userData.cityLodComponentArea * columns * rows;
  } else if (wholeDetail && mesh.frustumCulled) detailArea = Math.PI / 4 * diameter * diameter;
  return { box, lodBox, corners, level: 0, draw: 0, diameter, detailArea, detailHidden: false };
}

function triangleCount(geometry: THREE.BufferGeometry): number {
  return Math.floor((geometry.index?.count ?? geometry.getAttribute("position")?.count ?? 0) / 3);
}

type Batch = {
  mesh: THREE.InstancedMesh;
  box: THREE.Box3;
  placements: Placement[];
  levels?: CityLodLevels;
  canSubset: boolean;
  subsets: THREE.InstancedMesh[];
  members: number[][];
  shadowSubsets?: Map<THREE.Camera, { mesh: THREE.InstancedMesh; members: number[] }>;
};

type StaticMesh = { mesh: THREE.Mesh; box: THREE.Box3; placement?: Placement; levels?: CityLodLevels; alternate?: THREE.Mesh };

// Only actual opaque triangles can cover cells. A cell stores an upper bound on
// their depth. Bounds straddling the near plane or an uncovered cell always draw.
// The published world is static. Recreate this controller after changing its
// transforms, geometry, material opacity, layers or authored visibility.
export class CityVisibility {
  private readonly perPlacement: boolean;
  private readonly disconnectedErrorPixels: number;
  private readonly distantDetailErrorPixels: number;
  private readonly distantArchitectureErrorPixels: number;
  private readonly detailCull?: CityDetailCullBudget;
  private readonly coalescing?: CityClusterDraws;
  private readonly farHardware?: CityFarHardware;
  private readonly width: number;
  private readonly height: number;
  private readonly depth: Float32Array;
  private readonly tileWidth: number;
  private readonly tileDepth: Float32Array;
  private tilesReady = false;
  private scannedCells = 0;
  private readonly faces: Float32Array;
  private readonly faceLayers: Uint32Array;
  private readonly bounds: Batch[] = [];
  private readonly meshes: StaticMesh[] = [];
  private readonly levelGroups: { mesh: THREE.Mesh; levels: CityLodLevels; placements: Placement[] }[] = [];
  private viewBounds = this.bounds;
  private viewMeshes = this.meshes;
  private viewLevels = this.levelGroups;
  private lodMergeTriangles: number;
  private readonly bandGroups: LodBandGroup[] = [];
  private viewBandGroups = this.bandGroups;
  private readonly viewGroups = new WeakMap<THREE.Mesh, THREE.Object3D>();
  private readonly groupPlacements: { group: THREE.Object3D; placement: Placement }[] = [];
  private readonly groupFaces: { group: THREE.Object3D; start: number; end: number; mask: number }[] = [];
  private readonly fixedTransforms: THREE.Mesh[] = [];
  private readonly hidden: THREE.Mesh[] = [];
  private readonly shownSubsets: THREE.Mesh[] = [];
  private readonly frustum = new THREE.Frustum();
  private readonly paddedCell = new THREE.Box3();
  private readonly shadowFrustum = new THREE.Frustum();
  private readonly shadowClip = new THREE.Matrix4();
  private readonly shadowHidden: THREE.InstancedMesh[] = [];
  private readonly shadowShown: THREE.InstancedMesh[] = [];
  private readonly shadowSelection: number[] = [];
  private readonly shadowCameras = new Set<THREE.Camera>();
  private readonly matrix = new THREE.Matrix4();
  private readonly clip = new THREE.Matrix4();
  private readonly previous = new THREE.Matrix4();
  private previousHeight = 0;
  private previousLodBias = 1;
  private cameraMask = 0;
  private view = 0;
  private cellCameraMargin = 0;
  private readonly cellProbes: PlacementCell[] = [];
  private usePlacementCells = false;
  private readonly p = new Float64Array(12);
  private readonly nearDistances = new Float64Array(4);
  private readonly clipped = new Float64Array(15);
  private readonly edges = new Float64Array(15);
  private clippingFar = 0;
  private readonly bands: number[][] = [];
  private readonly eye = new THREE.Vector3();
  private readonly lodFaces: { placement: Placement; offsets: number[] }[] = [];

  constructor(
    root: THREE.Object3D,
    perPlacement = true,
    lods: ReadonlyMap<THREE.BufferGeometry, CityLodLevels> = new Map(),
    options: { disconnectedErrorPixels?: number; distantDetailErrorPixels?: number; distantArchitectureErrorPixels?: number;
      occlusionSize?: readonly [number, number]; coalesceClusters?: boolean; clusterPairs?: WeakMap<THREE.Mesh, object>; farHardware?: CityFarHardware;
      detailCull?: CityDetailCullOptions; wideClusters?: boolean; groupFor?: (mesh: THREE.Mesh) => THREE.Object3D | undefined;
      instanceGroups?: CityInstanceGroups;
      /** Extra triangles allowed per removed LOD band draw. Default CITY_LOD_MERGE_TRIANGLES; a negative value keeps every band. */
      lodMergeTriangles?: number } = {},
  ) {
    this.perPlacement = perPlacement;
    const mergeBudget = options.lodMergeTriangles ?? CITY_LOD_MERGE_TRIANGLES;
    this.lodMergeTriangles = Number.isNaN(mergeBudget) ? CITY_LOD_MERGE_TRIANGLES : mergeBudget;
    this.detailCull = detailCullBudget(options.detailCull);
    this.farHardware = options.farHardware;
    const budget = options.disconnectedErrorPixels ?? 1.5;
    this.disconnectedErrorPixels = Number.isFinite(budget) && budget > 0 ? budget : 1.5;
    const distantBudget = options.distantDetailErrorPixels ?? this.disconnectedErrorPixels;
    this.distantDetailErrorPixels = Number.isFinite(distantBudget)
      ? Math.max(this.disconnectedErrorPixels, distantBudget) : this.disconnectedErrorPixels;
    const architectureBudget = options.distantArchitectureErrorPixels ?? 1.5;
    this.distantArchitectureErrorPixels = Number.isFinite(architectureBudget) ? Math.max(1.5, architectureBudget) : 1.5;
    const dimension = (value: number | undefined, fallback: number) =>
      value !== undefined && Number.isFinite(value) ? Math.max(8, Math.min(512, Math.floor(value))) : fallback;
    this.width = dimension(options.occlusionSize?.[0], 80);
    this.height = dimension(options.occlusionSize?.[1], 48);
    this.depth = new Float32Array(this.width * this.height);
    this.tileWidth = Math.ceil(this.width / DEPTH_TILE);
    this.tileDepth = new Float32Array(this.tileWidth * Math.ceil(this.height / DEPTH_TILE));
    root.updateWorldMatrix(true, true);
    const occluderFaces = new CityOccluderFaces();
    const vertices: number[] = [];
    const layers: number[] = [];
    const transform = new THREE.Matrix4(),
      v = new THREE.Vector3();
    root.traverseVisible((mesh) => {
      if (!(mesh instanceof THREE.Mesh) || !mesh.visible || hasDeclaredLodOwner(mesh)) return;
      const instanced = mesh instanceof THREE.InstancedMesh;
      const material = mesh.material;
      const materials = Array.isArray(material) ? material : [material];
      // Bounds cover static glTF meshes. Deformed vertices and child subtrees need
      // their own conservative bounds before they can participate in occlusion.
      if (customMeshCallbacks(mesh) || mesh.children.length || (mesh as THREE.SkinnedMesh).isSkinnedMesh || mesh.morphTargetInfluences?.length
        || (instanced && mesh.morphTexture) || (mesh.geometry as THREE.InstancedBufferGeometry).isInstancedBufferGeometry
        || materials.some(surface => !usesCityDepthTest(surface)
          || surface instanceof THREE.ShaderMaterial || (surface as THREE.MeshStandardMaterial).displacementMap)
        || Object.values(mesh.geometry.attributes).some(attribute => attribute instanceof THREE.InstancedBufferAttribute
          || (attribute instanceof THREE.InterleavedBufferAttribute && attribute.data instanceof THREE.InstancedInterleavedBuffer))) return;
      // The bounds and occluder faces below are static for the controller's
      // lifetime. Reuse these same local transforms in each Three.js pass.
      if (mesh.matrixAutoUpdate) {
        mesh.matrixAutoUpdate = false;
        this.fixedTransforms.push(mesh);
      }
      // A subset copies each kept instance's matrix and tint. Other per-instance
      // attributes live on the shared geometry and cannot be reordered per subset.
      const canSubset =
        instanced &&
        !Array.isArray(material) &&
        !material.transparent &&
        !mesh.morphTexture &&
        !Object.values(mesh.geometry.attributes).some((a) => a instanceof THREE.InstancedBufferAttribute);
      const levels = (instanced ? canSubset : !Array.isArray(material) && !material.transparent)
        ? lods.get(mesh.geometry) : undefined;
      const wholeDetail = !!this.detailCull && !levels && (!instanced || canSubset)
        && !Array.isArray(material) && disposableCityDetail(material);
      const placements: Placement[] = [];
      const mixed = options.instanceGroups?.get(mesh);
      if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
      if (instanced) {
        mesh.computeBoundingBox();
        // Each subset keeps the original conservative sphere as its instance selection changes.
        mesh.computeBoundingSphere();
        if (mixed || perPlacement || levels || wholeDetail) {
          for (let i = 0; i < mesh.count; i++) {
            mesh.getMatrixAt(i, transform);
            transform.premultiply(mesh.matrixWorld);
            const placement = placementFor(mesh, transform, wholeDetail);
            placement.far = this.farHardware?.member(mesh, i);
            if (mixed?.[i]) this.groupPlacements.push({ group: mixed[i]!, placement });
            placements.push(placement);
          }
        }
        this.bounds.push({
          mesh,
          placements,
          levels,
          canSubset,
          subsets: [],
          members: Array.from({ length: 1 + (levels?.length ?? 0) }, () => []),
          box: mesh.boundingBox!.clone().applyMatrix4(mesh.matrixWorld),
        });
      } else {
        const placement = levels || wholeDetail ? placementFor(mesh, mesh.matrixWorld, wholeDetail) : undefined;
        if (placement) {
          placement.far = this.farHardware?.member(mesh, 0);
          placements.push(placement);
        }
        const box = placement?.box ?? mesh.geometry.boundingBox!.clone().applyMatrix4(mesh.matrixWorld);
        this.meshes.push({ mesh, box, placement, levels });
      }
      if (levels || wholeDetail) this.levelGroups.push({ mesh, levels: levels ?? [], placements });
      const group = options.groupFor?.(mesh);
      if (group) this.viewGroups.set(mesh, group);
      if (!writesSolidCitySurface(material)) return;
      const geometries = levels ? [mesh.geometry, ...levels] : [mesh.geometry];
      const count = instanced ? mesh.count : 1;
      const firstFace = layers.length;
      for (let i = 0; i < count; i++) {
        const firstPlacementFace = layers.length;
        if (instanced) {
          mesh.getMatrixAt(i, transform);
          transform.premultiply(mesh.matrixWorld);
        } else transform.copy(mesh.matrixWorld);
        if (transform.determinant() <= 0) continue;
        const facesFor = occluderFaces.forPlacement(transform, instanced);
        const offsets: number[] = [];
        for (const geometry of geometries) {
          offsets.push(vertices.length);
          for (const tri of facesFor(geometry)) {
            for (const point of tri) {
              v.copy(point).applyMatrix4(transform);
              vertices.push(v.x, v.y, v.z);
            }
            if (tri.length === 3) vertices.push(0, 0, 0);
            vertices.push(tri.length, material.side === THREE.DoubleSide ? 1 : 0);
            layers.push(mesh.layers.mask);
          }
        }
        if ((levels || wholeDetail) && vertices.length > offsets[0]) {
          offsets.push(vertices.length);
          this.lodFaces.push({ placement: placements[i], offsets });
        }
        if (mixed?.[i] && layers.length !== firstPlacementFace)
          this.groupFaces.push({ group: mixed[i]!, start: firstPlacementFace, end: layers.length, mask: mesh.layers.mask });
      }
      if (group && layers.length !== firstFace)
        this.groupFaces.push({ group, start: firstFace, end: layers.length, mask: mesh.layers.mask });
    });
    this.faces = new Float32Array(vertices);
    this.faceLayers = new Uint32Array(layers);
    if (perPlacement) this.indexPlacementCells();
    {
      const split = new Map<object, LodBandGroup>();
      for (const { mesh, levels, placements } of this.bounds) {
        if (!levels?.length || placements.length < 2) continue;
        const source: LodBands = { mesh, placements, triangles: [mesh.geometry, ...levels].map(triangleCount),
          counts: Array<number>(1 + levels.length).fill(0) };
        const key = options.clusterPairs?.get(mesh);
        const siblings = key && split.get(key);
        if (siblings && siblings.target.length === source.counts.length
          && siblings.sources[0].placements.length === placements.length) siblings.sources.push(source);
        else {
          const group = { sources: [source], target: source.counts.map((_, level) => level) };
          this.bandGroups.push(group);
          if (key && !siblings) split.set(key, group);
        }
      }
    }
    if (options.coalesceClusters && options.clusterPairs)
      this.coalescing = new CityClusterDraws([...this.meshes, ...this.bounds], options.clusterPairs);
    this.setSubmission(options.wideClusters ? "clusters" : "ordinary");
  }

  /** Change the LOD band budget for later views. Undefined restores the default and a
   * negative value keeps every band. */
  setLodMergeTriangles(triangles: number = CITY_LOD_MERGE_TRIANGLES): void {
    if (Number.isNaN(triangles) || triangles === this.lodMergeTriangles) return;
    this.lodMergeTriangles = triangles;
    this.previousHeight = 0;
  }

  setSubmission(mode: "ordinary" | "clusters"): void {
    if (this.coalescing) this.coalescing.wide = mode === "clusters";
    this.previousHeight = 0;
  }

  /** Whole-block proxies replace color geometry only. Omit the detailed faces
   * from software depth as well; a removed balcony must never hide a neighbor. */
  setHiddenGroups(hidden: ReadonlySet<THREE.Object3D>): void {
    const show = ({ mesh }: { mesh: THREE.Mesh }) => {
      const group = this.viewGroups.get(mesh);
      return !group || !hidden.has(group);
    };
    this.viewBounds = hidden.size ? this.bounds.filter(show) : this.bounds;
    this.viewMeshes = hidden.size ? this.meshes.filter(show) : this.meshes;
    this.viewLevels = hidden.size ? this.levelGroups.filter(show) : this.levelGroups;
    this.viewBandGroups = hidden.size ? this.bandGroups.filter(group => show(group.sources[0])) : this.bandGroups;
    for (const r of this.groupFaces) this.faceLayers.fill(hidden.has(r.group) ? 0 : r.mask, r.start, r.end);
    for (const { placement, group } of this.groupPlacements) placement.blockHidden = hidden.has(group);
    this.previousHeight = 0;
  }

  /** Query only after frame() for the current perspective camera. */
  occludesBounds = (box: THREE.Box3): boolean => this.occluded(box);

  restore(): void {
    this.restoreShadow();
    this.coalescing?.restore();
    this.farHardware?.restore();
    for (const mesh of this.hidden) mesh.visible = true;
    for (const mesh of this.shownSubsets) mesh.visible = false;
  }

  /** Bias adjusts geometry selection while small-detail omission uses the actual viewport. */
  frame(camera: THREE.Camera, viewportHeight = 1080, geometryLodBias = 1): void {
    this.restoreShadow();
    if (!(camera instanceof THREE.PerspectiveCamera)) {
      this.restore();
      return;
    }
    camera.updateWorldMatrix(true, false);
    this.clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    // The software occluder clips against z + w = 0. Normalize the GPU's
    // [0, w] clip depth once, before using the same matrix for faces and bounds.
    if (camera.coordinateSystem === THREE.WebGPUCoordinateSystem || camera.reversedDepth) {
      const m = this.clip.elements, sign = camera.reversedDepth ? -1 : 1;
      for (let z = 2; z < 16; z += 4) m[z] = sign * (2 * m[z] - m[z + 1]);
    }
    if (!this.previous.equals(this.clip) || this.previousHeight !== viewportHeight
        || this.previousLodBias !== geometryLodBias || this.cameraMask !== camera.layers.mask) {
      this.restore();
      this.hidden.length = 0;
      this.shownSubsets.length = 0;
      this.previous.copy(this.clip);
      this.previousHeight = viewportHeight;
      this.previousLodBias = geometryLodBias;
      this.cameraMask = camera.layers.mask;
      this.view++;
      this.frustum.setFromProjectionMatrix(this.clip);
      this.clippingFar = camera.far;
      this.depth.fill(Infinity);
      this.tilesReady = false;
      this.scannedCells = 0;
      this.eye.setFromMatrixPosition(camera.matrixWorld);
      this.cellCameraMargin = CELL_BOUND_MARGIN * Math.max(Math.abs(this.eye.x), Math.abs(this.eye.y), Math.abs(this.eye.z));
      const geometryHeight = viewportHeight * geometryLodBias;
      this.farHardware?.select(camera, geometryHeight);
      const m = camera.matrixWorldInverse.elements;
      for (const { levels, placements } of this.viewLevels) {
        const projected = hasCityLodErrors(levels);
        for (const p of placements) {
          if (p.far?.level) {
            p.level = p.far.level;
            p.detailHidden = false;
            continue;
          }
          const b = p.lodBox;
          const depth = -(m[2] * (b.min.x + b.max.x) + m[6] * (b.min.y + b.max.y) + m[10] * (b.min.z + b.max.z)) / 2 - m[14]
            - (Math.abs(m[2]) * (b.max.x - b.min.x) + Math.abs(m[6]) * (b.max.y - b.min.y) + Math.abs(m[10]) * (b.max.z - b.min.z)) / 2;
          // A projected bound crossing the near plane is always full detail.
          // Legacy levels still advance their distance hysteresis behind the camera.
          if (projected && depth <= camera.near) {
            p.level = 0;
            p.detailHidden = false;
            continue;
          }
          const distance = b.distanceToPoint(this.eye);
          const lodDistance = projected ? distance : distance / geometryLodBias;
          p.level = selectCityLod(levels, lodDistance, camera, geometryHeight, p.level, depth,
            this.disconnectedErrorPixels, p.diameter, this.distantDetailErrorPixels, this.distantArchitectureErrorPixels);
          p.detailHidden = p.detailArea > 0 && omitCityDetail(p.box, p.diameter, p.detailArea, distance, depth,
            camera, viewportHeight, p.detailHidden, this.detailCull);
        }
        for (const p of placements) p.draw = p.level;
      }
      if (this.lodMergeTriangles >= 0) for (const group of this.viewBandGroups) this.mergeLodBands(group);
      // The occlusion buffer must use the same silhouettes as the levels the camera draws.
      let first = 0;
      for (const { placement, offsets } of this.lodFaces) {
        for (let i = first; i < offsets[0]; i += FACE_STRIDE) this.cover(i);
        if (!placement.detailHidden && this.frustum.intersectsBox(placement.box))
          for (let i = offsets[placement.draw]; i < offsets[placement.draw + 1]; i += FACE_STRIDE) this.cover(i);
        first = offsets[offsets.length - 1];
      }
      for (let i = first; i < this.faces.length; i += FACE_STRIDE) this.cover(i);
      this.usePlacementCells = this.probePlacementCells();
      for (const entry of this.viewMeshes) {
        const { mesh, box, placement } = entry;
        if (!mesh.visible) continue;
        if (placement?.detailHidden || !this.frustum.intersectsBox(box) || this.occluded(box, placement?.corners)) {
          this.hidden.push(mesh);
        } else if (placement?.far?.level) {
          placement.far.keep = true;
          this.hidden.push(mesh);
        } else if (placement?.level) {
          const geometry = entry.levels![placement.level - 1];
          if (geometry === mesh.geometry) continue;
          this.hidden.push(mesh);
          if (geometry.index?.count === 0) continue;
          if (!entry.alternate) {
            entry.alternate = mesh.clone(false);
            entry.alternate.customDepthMaterial = mesh.customDepthMaterial;
            entry.alternate.customDistanceMaterial = mesh.customDistanceMaterial;
            entry.alternate.castShadow = false;
            entry.alternate.raycast = () => {};
            mesh.parent!.add(entry.alternate);
          }
          entry.alternate.geometry = geometry;
          this.shownSubsets.push(entry.alternate);
        }
      }
      for (const batch of this.viewBounds) {
        if (!batch.mesh.visible) continue;
        const single = batch.placements.length === 1 ? batch.placements[0].corners : undefined;
        if ((this.perPlacement && !this.frustum.intersectsBox(batch.box)) || this.occluded(batch.box, single)) {
          this.hidden.push(batch.mesh);
        } else if (batch.placements.length > 1 || batch.levels || batch.placements[0]?.detailHidden) {
          this.selectPlacements(batch);
        }
      }
      this.farHardware?.finish();
    }
    this.coalescing?.restore();
    for (const mesh of this.hidden) mesh.visible = false;
    for (const mesh of this.shownSubsets) mesh.visible = true;
    this.coalescing?.frame();
    this.farHardware?.show();
  }

  /** Full-resolution casters selected against the light, independently of view
   * LOD and occlusion. Each persistent light camera owns its instance buffers:
   * Three uploads each instance object only once per renderer frame. More than
   * eight camera identities conservatively use the original complete casters. */
  shadowFrame(shadowCamera: THREE.Camera, viewCamera?: THREE.Camera): void {
    this.restore();
    if (!this.shadowCameras.has(shadowCamera)) {
      if (this.shadowCameras.size >= MAX_SHADOW_CAMERAS) return;
      this.shadowCameras.add(shadowCamera);
    }
    shadowCamera.updateWorldMatrix(true, false);
    this.shadowClip.multiplyMatrices(shadowCamera.projectionMatrix, shadowCamera.matrixWorldInverse);
    this.shadowFrustum.setFromProjectionMatrix(this.shadowClip, shadowCamera.coordinateSystem, shadowCamera.reversedDepth);
    for (const batch of this.bounds) {
      const mesh = batch.mesh;
      if (!mesh.visible || !mesh.castShadow || !mesh.frustumCulled || (viewCamera && !mesh.layers.test(viewCamera.layers))) continue;
      // A custom shadow shader can move vertices beyond the visible mesh bounds.
      if ([mesh.customDepthMaterial, mesh.customDistanceMaterial].some(material =>
        material instanceof THREE.ShaderMaterial || (material as THREE.MeshDepthMaterial | undefined)?.displacementMap)) continue;
      if (!this.shadowFrustum.intersectsBox(batch.box)) {
        mesh.visible = false;
        this.shadowHidden.push(mesh);
        continue;
      }
      if (!batch.canSubset || batch.placements.length < 2) continue;
      const keep = this.shadowSelection;
      keep.length = 0;
      for (let i = 0; i < batch.placements.length; i++) {
        if (this.shadowFrustum.intersectsBox(batch.placements[i].box)) keep.push(i);
      }
      if (keep.length === mesh.count) continue;
      mesh.visible = false;
      this.shadowHidden.push(mesh);
      if (!keep.length) continue;
      let selection = batch.shadowSubsets?.get(shadowCamera);
      if (!selection) {
        const subset = mesh.clone(false);
        subset.geometry = mesh.geometry;
        subset.customDepthMaterial = mesh.customDepthMaterial;
        subset.customDistanceMaterial = mesh.customDistanceMaterial;
        subset.raycast = () => {};
        mesh.parent!.add(subset);
        selection = { mesh: subset, members: [] };
        (batch.shadowSubsets ??= new Map()).set(shadowCamera, selection);
      }
      const { mesh: subset, members: previous } = selection;
      if (previous.length !== keep.length || keep.some((index, i) => previous[i] !== index)) {
        previous.length = keep.length;
        for (let i = 0; i < keep.length; i++) {
          previous[i] = keep[i];
          mesh.getMatrixAt(keep[i], this.matrix);
          subset.setMatrixAt(i, this.matrix);
          copyInstanceTint(mesh, keep[i], subset, i);
        }
        subset.instanceMatrix.clearUpdateRanges();
        subset.instanceMatrix.addUpdateRange(0, keep.length * 16);
        subset.instanceMatrix.needsUpdate = true;
        uploadInstanceTints(subset, keep.length);
      }
      subset.count = keep.length;
      subset.visible = true;
      this.shadowShown.push(subset);
    }
  }

  private restoreShadow(): void {
    for (const mesh of this.shadowHidden) mesh.visible = true;
    for (const mesh of this.shadowShown) mesh.visible = false;
    this.shadowHidden.length = 0;
    this.shadowShown.length = 0;
  }

  /** Release generated selection buffers while keeping the host's scene assets. */
  dispose(): void {
    this.restore();
    this.coalescing?.dispose();
    this.farHardware?.dispose();
    for (const mesh of this.fixedTransforms) mesh.matrixAutoUpdate = true;
    for (const entry of this.meshes) entry.alternate?.removeFromParent();
    for (const batch of this.bounds) {
      for (const { mesh } of batch.shadowSubsets?.values() ?? []) {
        mesh.removeFromParent();
        mesh.dispose();
      }
      batch.shadowSubsets?.clear();
    }
    this.shadowCameras.clear();
    for (const batch of this.bounds) for (const subset of batch.subsets) {
      if (!subset) continue;
      subset.removeFromParent();
      subset.dispose();
    }
    this.hidden.length = 0;
    this.shownSubsets.length = 0;
  }

  private indexPlacementCells(): void {
    const cells = new Map<string, { box: THREE.Box3; placements: Placement[] }>();
    for (const { placements } of this.bounds) {
      if (placements.length < 2) continue;
      for (const placement of placements) {
        const { min, max } = placement.box;
        const x = Math.floor((min.x + max.x) / (2 * PLACEMENT_CELL_METERS));
        const y = Math.floor((min.y + max.y) / (2 * PLACEMENT_CELL_METERS));
        const z = Math.floor((min.z + max.z) / (2 * PLACEMENT_CELL_METERS));
        if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) continue;
        const key = `${x},${y},${z}`;
        let cell = cells.get(key);
        if (!cell) {
          cell = { box: new THREE.Box3(), placements: [] };
          cells.set(key, cell);
        }
        // The grid assigns membership only. A placement can extend outside its cell.
        cell.box.union(placement.box);
        cell.placements.push(placement);
      }
    }
    const probes: { rank: number; cell: PlacementCell }[] = [];
    for (const { box, placements } of cells.values()) {
      if (placements.length < MIN_CELL_PLACEMENTS) continue;
      const { min, max } = box;
      const scale = Math.max(1, Math.abs(min.x), Math.abs(min.y), Math.abs(min.z), Math.abs(max.x), Math.abs(max.y), Math.abs(max.z));
      if (!Number.isFinite(2 * scale)) continue;
      box.expandByScalar(scale * CELL_BOUND_MARGIN);
      const cell: PlacementCell = { box, view: 0, keep: true };
      for (const placement of placements) placement.cell = cell;
      probes.push({ rank: cellProbeOrder(probes.length), cell });
    }
    probes.sort((a, b) => a.rank - b.rank);
    for (const { cell } of probes.slice(0, CELL_PROBE_COUNT)) this.cellProbes.push(cell);
  }

  private probePlacementCells(): boolean {
    if (this.cellProbes.length < MIN_VISIBLE_CELL_PROBES || !Number.isFinite(this.cellCameraMargin)) return false;
    let inFrustum = 0, occluded = 0;
    for (const cell of this.cellProbes) {
      const box = this.paddedCell.copy(cell.box).expandByScalar(this.cellCameraMargin);
      cell.view = this.view;
      cell.keep = false;
      if (!this.frustum.intersectsBox(box)) continue;
      inFrustum++;
      cell.keep = !this.occluded(box);
      if (!cell.keep) occluded++;
    }
    // Sample actual current-view occlusion before adding cell tests to the
    // placement loop. Reuse the sample's decisions if the view enables the grid.
    return inFrustum >= MIN_VISIBLE_CELL_PROBES && occluded * 4 >= inFrustum;
  }

  private cellKeepsPlacement(cell: PlacementCell): boolean {
    if (cell.view !== this.view) {
      cell.view = this.view;
      // A visible cell still needs the existing placement tests. Its box can
      // span doorways and other openings that expose only part of the cell.
      // The parent-only margin covers projected corner ties when evaluating
      // distant world coordinates or subtracting a large camera translation.
      const box = this.paddedCell.copy(cell.box).expandByScalar(this.cellCameraMargin);
      cell.keep = !Number.isFinite(this.cellCameraMargin) || (this.frustum.intersectsBox(box) && !this.occluded(box));
    }
    return cell.keep;
  }

  /** Walk from the coarsest selected band to the finest. A band joins the next
   * finer one when that removes a draw within the triangle budget. Occlusion is
   * not known yet, so in-frustum placements bound the cost from above. */
  private mergeLodBands({ sources, target }: LodBandGroup): void {
    let selected = -1, several = false;
    for (const { placements, triangles } of sources) {
      for (const p of placements) {
        if (p.far?.level || !triangles[p.level] || p.level === selected) continue;
        if (selected >= 0) { several = true; break; }
        selected = p.level;
      }
      if (several) break;
    }
    if (!several) return;
    for (const { placements, counts } of sources) {
      counts.fill(0);
      for (const p of placements) {
        if (!p.detailHidden && !p.blockHidden && !p.far?.level && (!this.perPlacement || this.frustum.intersectsBox(p.box)))
          counts[p.level]++;
      }
    }
    // One joined draw serves every part of a split primitive.
    const joined = this.coalescing !== undefined && sources.length > 1;
    let coarse = -1, merged = false;
    for (let level = target.length - 1; level >= 0; level--) {
      target[level] = level;
      if (!sources.some(({ counts, triangles }) => counts[level] && triangles[level])) continue;
      if (coarse >= 0) {
        let extra = 0, saved = 0;
        for (const { counts, triangles } of sources) {
          if (!counts[coarse] || !triangles[coarse]) continue;
          extra += counts[coarse] * (triangles[level] - triangles[coarse]);
          if (counts[level] && triangles[level]) saved++;
        }
        if (saved && extra <= this.lodMergeTriangles * (joined ? 1 : saved)) {
          for (const { counts, triangles } of sources) {
            if (!triangles[coarse]) continue;
            counts[level] += counts[coarse];
            counts[coarse] = 0;
          }
          target[coarse] = level;
          merged = true;
        }
      }
      coarse = level;
    }
    if (!merged) return;
    for (let level = 1; level < target.length; level++) target[level] = target[target[level]];
    // An empty level omits its part. Promotion never brings omitted geometry back.
    for (const { placements, triangles } of sources) {
      for (const p of placements) if (!p.far?.level && triangles[p.level]) p.draw = target[p.level];
    }
  }

  private selectPlacements(batch: Batch): void {
    const { mesh, placements } = batch;
    const useCells = this.usePlacementCells;
    const bands = this.bands;
    while (bands.length < 1 + (batch.levels?.length ?? 0)) bands.push([]);
    for (const band of bands) band.length = 0;
    for (let i = 0; i < placements.length; i++) {
      const p = placements[i];
      // The far draw uses the whole atlas. A sibling cell already admitted
      // this exact placement at this level during the current frame.
      if (p.detailHidden || p.blockHidden || (p.far?.level && p.far.keep)) continue;
      if (
        !this.perPlacement ||
        placements.length === 1 ||
        ((!useCells || !p.cell || this.cellKeepsPlacement(p.cell)) && this.frustum.intersectsBox(p.box) && !this.occluded(p.box, p.corners))
      ) {
        if (p.far?.level) p.far.keep = true;
        else bands[p.draw].push(i);
      }
    }
    if (bands[0].length === placements.length) return;
    if (!batch.canSubset) {
      if (!bands[0].length) this.hidden.push(mesh);
      return;
    }
    for (let level = 0; level < bands.length; level++) {
      const keep = bands[level];
      if (!keep.length) continue;
      const geometry = level ? batch.levels![level - 1] : mesh.geometry;
      if (geometry.index?.count === 0 || !geometry.getAttribute("position")?.count) continue;
      let subset = batch.subsets[level];
      if (!subset) {
        subset = mesh.clone(false);
        subset.customDepthMaterial = mesh.customDepthMaterial;
        subset.customDistanceMaterial = mesh.customDistanceMaterial;
        subset.geometry = geometry;
        subset.castShadow = false;
        subset.raycast = () => {};
        mesh.parent!.add(subset);
        batch.subsets[level] = subset;
      }
      const previous = batch.members[level];
      if (previous.length !== keep.length || keep.some((id, i) => previous[i] !== id)) {
        previous.length = keep.length;
        for (let i = 0; i < keep.length; i++) {
          previous[i] = keep[i];
          mesh.getMatrixAt(keep[i], this.matrix);
          subset.setMatrixAt(i, this.matrix);
          copyInstanceTint(mesh, keep[i], subset, i);
        }
        subset.instanceMatrix.needsUpdate = true;
        uploadInstanceTints(subset, keep.length);
      }
      subset.count = keep.length;
      this.shownSubsets.push(subset);
    }
    // The original instance buffer remains available for shadows and picking.
    this.hidden.push(mesh);
  }

  private cover(i: number, half = 0): void {
    if ((this.faceLayers[i / FACE_STRIDE] & this.cameraMask) === 0) return;
    const t = this.faces,
      p = this.p;
    const m = this.clip.elements, distances = this.nearDistances;
    const count = half ? 3 : t[i + 12];
    let inside = 0;
    for (let j = 0; j < count; j++) {
      const k = j * 3, at = i + k + (half === 2 && j > 0 ? 3 : 0);
      const x = t[at], y = t[at + 1], z = t[at + 2];
      const w = m[3] * x + m[7] * y + m[11] * z + m[15];
      if (w >= this.clippingFar) return;
      p[k] = m[0] * x + m[4] * y + m[8] * z + m[12];
      p[k + 1] = m[1] * x + m[5] * y + m[9] * z + m[13];
      p[k + 2] = w;
      // Move the clipping plane slightly inward so its contour stays inside the GPU's.
      const d = m[2] * x + m[6] * y + m[10] * z + m[14] + w - 1e-6;
      distances[j] = d;
      if (d > 0) inside++;
    }
    if (inside === 0) return;
    if (count === 4 && inside < count) {
      // Float32 world transforms can make a planar pair slightly bent. Clip the original
      // diagonal as the GPU does when the near plane cuts that surface.
      this.cover(i, 1);
      this.cover(i, 2);
      return;
    }
    let polygon = p, length = count * 3;
    if (inside < count) {
      // Clip in homogeneous coordinates, preserving winding even when a vertex is behind the eye.
      polygon = this.clipped;
      length = 0;
      for (let j = 0, prev = count - 1; j < count; prev = j++) {
        const from = prev * 3, to = j * 3;
        if ((distances[prev] > 0) !== (distances[j] > 0)) {
          const fraction = distances[prev] / (distances[prev] - distances[j]);
          for (let k = 0; k < 3; k++) polygon[length++] = p[from + k] + fraction * (p[to + k] - p[from + k]);
        }
        if (distances[j] > 0) {
          for (let k = 0; k < 3; k++) polygon[length++] = p[to + k];
        }
      }
    }
    for (let j = 0; j < length; j += 3) {
      const w = polygon[j + 2];
      if (w <= 0.001) return;
      polygon[j] = (0.5 + 0.5 * polygon[j] / w) * this.width;
      polygon[j + 1] = (0.5 + 0.5 * polygon[j + 1] / w) * this.height;
    }
    this.coverPolygon(polygon, length, t[i + 13]);
  }

  private coverPolygon(p: Float64Array, length: number, doubleSided: number): void {
    let winding = 0, minX = this.width, minY = this.height, maxX = 0, maxY = 0, far = 0;
    for (let j = 0; j < length; j += 3) {
      const k = j + 3 === length ? 0 : j + 3;
      winding += p[j] * p[k + 1] - p[j + 1] * p[k];
      minX = Math.min(minX, p[j]); maxX = Math.max(maxX, p[j]);
      minY = Math.min(minY, p[j + 1]); maxY = Math.max(maxY, p[j + 1]);
      far = Math.max(far, p[j + 2]);
    }
    if (Math.abs(winding) < 1 || (winding < 0 && !doubleSided)) return;
    // Coverage requires all four corners of a cell inside the polygon. Its
    // bounds can discard partially touched border rows and columns up front.
    const left = Math.max(0, Math.ceil(minX)), right = Math.min(this.width, Math.floor(maxX));
    const bottom = Math.max(0, Math.ceil(minY)), top = Math.min(this.height, Math.floor(maxY));
    if (right <= left || top <= bottom) return;
    const sign = Math.sign(winding);
    for (let j = 0; j < length; j += 3) {
      const k = j + 3 === length ? 0 : j + 3, next = k + 3 === length ? 0 : k + 3;
      const a = sign * (p[j + 1] - p[k + 1]), b = sign * (p[k] - p[j]);
      // A folded or numerically degenerate projection never substitutes a filled quad.
      if (a * (p[next] - p[k]) + b * (p[next + 1] - p[k + 1]) < 0) return;
      this.edges[j] = a;
      this.edges[j + 1] = b;
      this.edges[j + 2] = -a * p[j] - b * p[j + 1] + Math.min(a, 0) + Math.min(b, 0);
    }
    // Reciprocal eye depth is affine on a projected triangle. Bound it over
    // the entire cell, including its farthest corner. For a slightly bent
    // quad, the largest vertex residual bounds either triangle's deviation.
    const ax = p[3]-p[0], ay = p[4]-p[1], bx = p[6]-p[0], by = p[7]-p[1];
    const area = ax*by - ay*bx, base = 1/p[2];
    const dz1 = 1/p[5]-base, dz2 = 1/p[8]-base;
    let dx = 0, dy = 0, intercept = 1/far;
    if (Math.abs(area) > 1e-5) {
      dx = (dz1*by - dz2*ay)/area;
      dy = (ax*dz2 - bx*dz1)/area;
      intercept = base - dx*p[0] - dy*p[1];
      let residual = 0;
      for (let j = 0; j < length; j += 3)
        residual = Math.max(residual, intercept + dx*p[j] + dy*p[j+1] - 1/p[j+2]);
      intercept += Math.min(dx,0) + Math.min(dy,0) - residual - 1e-8;
    }
    for (let y = bottom; y < top; y++) {
      let start = left, end = right;
      for (let j = 0; j < length && start < end; j += 3) {
        // Minimum signed edge value over the four corners must exceed the inward margin.
        const a = this.edges[j], threshold = 0.01 - this.edges[j + 1] * y - this.edges[j + 2];
        if (a > 0) start = Math.max(start, Math.floor(threshold / a) + 1);
        else if (a < 0) end = Math.min(end, Math.ceil(threshold / a));
        else if (threshold >= 0) end = start;
      }
      for (let x = start; x < end; x++) {
        const k = y * this.width + x;
        const reciprocal = intercept + dx*x + dy*y;
        const depth = (reciprocal > 0 ? Math.min(far, 1/reciprocal) : far) + 0.05;
        if (depth < this.depth[k]) this.depth[k] = depth;
      }
    }
  }

  private occluded(box: THREE.Box3, corners?: Float64Array): boolean {
    const m = this.clip.elements;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity, near = Infinity;
    for (let i = 0; i < 8; i++) {
      const k = i ^ 6; // Start on the upper face, often exposed from above.
      const x = corners ? corners[k * 3] : k & 1 ? box.max.x : box.min.x;
      const y = corners ? corners[k * 3 + 1] : k & 2 ? box.max.y : box.min.y;
      const z = corners ? corners[k * 3 + 2] : k & 4 ? box.max.z : box.min.z;
      const w = m[3] * x + m[7] * y + m[11] * z + m[15];
      if (w <= 0.001 || m[2] * x + m[6] * y + m[10] * z + m[14] <= -w || w >= this.clippingFar) return false;
      const sx = (0.5 + (0.5 * (m[0] * x + m[4] * y + m[8] * z + m[12])) / w) * this.width;
      const sy = (0.5 + (0.5 * (m[1] * x + m[5] * y + m[9] * z + m[13])) / w) * this.height;
      // This cell is inside the projected rectangle and near <= w. It alone
      // can prove visibility before projecting the other seven corners.
      if (i === 0 && sx >= 0 && sy >= 0 && sx < this.width && sy < this.height
        && w <= this.depth[Math.floor(sy) * this.width + Math.floor(sx)]) return false;
      x0 = Math.min(x0, sx); x1 = Math.max(x1, sx);
      y0 = Math.min(y0, sy); y1 = Math.max(y1, sy);
      near = Math.min(near, w);
    }
    const left = Math.max(0, Math.floor(x0 - 0.001)), right = Math.min(this.width, Math.ceil(x1 + 0.001));
    const bottom = Math.max(0, Math.floor(y0 - 0.001)), top = Math.min(this.height, Math.ceil(y1 + 0.001));
    if (left >= right || bottom >= top) return true;
    // Build the coarse buffer only after enough fine-grid work in this view
    // to amortize it. A small scene may issue a single screen-sized query.
    const span = right - left, area = span * (top - bottom);
    if (area > 25 && this.scannedCells >= 4 * this.depth.length)
      return this.tilesOcclude(left, right, bottom, top, near);
    for (let y = bottom; y < top; y++) {
      for (let x = left; x < right; x++) {
        if (near <= this.depth[y * this.width + x]) {
          if (area > 25) this.scannedCells += (y - bottom) * span + x - left + 1;
          return false;
        }
      }
    }
    if (area > 25) this.scannedCells += area;
    return true;
  }

  private tilesOcclude(left: number, right: number, bottom: number, top: number, near: number): boolean {
    if (!this.tilesReady) {
      this.tileDepth.fill(-Infinity);
      for (let y = 0; y < this.height; y++) {
        const tileRow = Math.floor(y / DEPTH_TILE) * this.tileWidth;
        for (let x = 0; x < this.width; x++) {
          const tile = tileRow + Math.floor(x / DEPTH_TILE);
          this.tileDepth[tile] = Math.max(this.tileDepth[tile], this.depth[y * this.width + x]);
        }
      }
      this.tilesReady = true;
    }
    for (let ty = Math.floor(bottom / DEPTH_TILE); ty * DEPTH_TILE < top; ty++) {
      const startY = Math.max(bottom, ty * DEPTH_TILE), endY = Math.min(top, (ty + 1) * DEPTH_TILE);
      for (let tx = Math.floor(left / DEPTH_TILE); tx * DEPTH_TILE < right; tx++) {
        if (near > this.tileDepth[ty * this.tileWidth + tx]) continue;
        const startX = Math.max(left, tx * DEPTH_TILE), endX = Math.min(right, (tx + 1) * DEPTH_TILE);
        // Only a fully enclosed tile proves visibility from its maximum. A
        // doorway in the adjoining part of a boundary tile must not count.
        if (endX - startX === DEPTH_TILE && endY - startY === DEPTH_TILE) return false;
        for (let y = startY; y < endY; y++) for (let x = startX; x < endX; x++) {
          if (near <= this.depth[y * this.width + x]) return false;
        }
      }
    }
    return true;
  }
}

function hasCityLodErrors(levels: CityLodLevels): boolean {
  return levels[0]?.userData.cityLodErrorMeters !== undefined && levels[1]?.userData.cityLodErrorMeters !== undefined;
}

/** The exporter records conservative world-space error, including placement scale.
 * Keep nearby detail intact, then budget geometric displacement in screen pixels. */
export function selectCityLod(levels: CityLodLevels, distance: number, camera: THREE.PerspectiveCamera,
  viewportHeight: number, previous: number, projectionDistance = distance, disconnectedErrorPixels = 1.5,
  detailDiameter = Infinity, distantDetailErrorPixels = disconnectedErrorPixels, distantArchitectureErrorPixels = 1.5): number {
  if (!hasCityLodErrors(levels)) {
    const legacy = distance >= (previous >= 2 ? 46 : 50) ? 2 : distance >= (previous > 0 ? 21 : 24) ? 1 : 0;
    return Math.min(levels.length, legacy);
  }
  if (distance < (previous > 0 ? 10 : 12) || projectionDistance <= camera.near) return 0;
  const pixelsPerMeter = Math.max(1, viewportHeight) * Math.abs(camera.projectionMatrix.elements[5]) / (2 * projectionDistance);
  const distanceFraction = Math.max(0, Math.min(1, (distance - 20) / 25));
  const sizeFraction = Math.max(0, Math.min(1, (192 - detailDiameter * pixelsPerMeter) / 160));
  const transition = distanceFraction * distanceFraction * (3 - 2 * distanceFraction)
    * sizeFraction * sizeFraction * (3 - 2 * sizeFraction);
  const detailBudget = disconnectedErrorPixels + (distantDetailErrorPixels - disconnectedErrorPixels) * transition;
  const architectureFraction = Math.max(0, Math.min(1, (distance - 40) / 60));
  const architectureBudget = 1.5 + (distantArchitectureErrorPixels - 1.5)
    * architectureFraction * architectureFraction * (3 - 2 * architectureFraction);
  for (let index = levels.length - 1; index >= 0; index--) {
    const error = levels[index].userData.cityLodErrorMeters as number | undefined;
    if (error === undefined) continue;
    const budget = levels[index].userData.cityLodIndependent ? detailBudget : architectureBudget;
    const threshold = previous > index ? budget + Math.max(0.25, budget / 8) : budget;
    if (error * pixelsPerMeter <= threshold) return index + 1;
  }
  return 0;
}

/** Join index streams after placement culling while keeping the original vertex
 * buffers and materials. Both the pair and mixed-LOD caches have fixed budgets. */
class CityClusterDraws {
  wide = false;
  private readonly groups: { sources: (StaticMesh | Batch)[]; draws: Map<string, THREE.Mesh> }[] = [];
  private readonly pairs: { left: StaticMesh | Batch; right: StaticMesh | Batch; draws: Map<number, THREE.Mesh> }[] = [];
  private readonly suppressed: THREE.Mesh[] = [];
  private readonly shown: THREE.Mesh[] = [];
  private bytes = 0;
  private entries = 0;
  private wideBytes = 0;
  private wideEntries = 0;

  constructor(sources: (StaticMesh | Batch)[], provenance: WeakMap<THREE.Mesh, object>) {
    const first = new Map<object, StaticMesh | Batch>();
    const groups = new Map<object, (StaticMesh | Batch)[]>();
    for (const source of sources) {
      const mesh = source.mesh, key = provenance.get(mesh);
      if (!key || Array.isArray(mesh.material) || mesh.material.transparent) continue;
      const siblings = groups.get(key);
      if (siblings) siblings.push(source); else groups.set(key, [source]);
      const previous = first.get(key);
      if (!previous) { first.set(key, source); continue; }
      const other = previous.mesh;
      if (other.parent !== mesh.parent || other.material !== mesh.material || other.layers.mask !== mesh.layers.mask
        || other.renderOrder !== mesh.renderOrder || other.receiveShadow !== mesh.receiveShadow
        || !other.matrix.equals(mesh.matrix) || (other instanceof THREE.InstancedMesh) !== (mesh instanceof THREE.InstancedMesh)
        || (other instanceof THREE.InstancedMesh && mesh instanceof THREE.InstancedMesh && !sameInstanceTintLayout(other, mesh))) continue;
      this.pairs.push({ left: previous, right: source, draws: new Map() });
      first.delete(key);
    }
    for (const sources of groups.values()) {
      if (sources.length < 2) continue;
      const base = sources[0].mesh;
      if (Array.isArray(base.material) || base.layers.mask !== 1 || !base.material.depthTest || !base.material.depthWrite
        || base.material.blending !== THREE.NormalBlending || base.material.stencilWrite || base.material.polygonOffset
        || base.material.depthFunc !== THREE.LessEqualDepth || base.material.alphaHash || base.material.alphaTest > 0
        || base.material.onBeforeRender !== THREE.Material.prototype.onBeforeRender) continue;
      const match = sources.every(({ mesh }) => {
        if (base.parent !== mesh.parent || base.material !== mesh.material || base.layers.mask !== mesh.layers.mask
          || base.renderOrder !== mesh.renderOrder || base.receiveShadow !== mesh.receiveShadow
          || !base.matrix.equals(mesh.matrix)) return false;
        if (base instanceof THREE.InstancedMesh && mesh instanceof THREE.InstancedMesh) {
          return base.count === mesh.count && base.instanceMatrix.array.every((v, i) => v === mesh.instanceMatrix.array[i])
            && equalInstanceTints(base, mesh, base.count);
        }
        return !(base instanceof THREE.InstancedMesh) && !(mesh instanceof THREE.InstancedMesh);
      });
      if (match) this.groups.push({ sources, draws: new Map() });
    }
  }

  private selected(source: StaticMesh | Batch, level: number): THREE.Mesh | undefined {
    if (level === 0 && source.mesh.visible) return source.mesh;
    const mesh = "placements" in source ? source.subsets[level]
      : source.placement?.level === level ? source.alternate : undefined;
    return mesh?.visible ? mesh : undefined;
  }

  frame(): void {
    if (this.wide) this.wideFrame();
    for (const pair of this.pairs) {
      const count = 1 + Math.min(pair.left.levels?.length ?? 0, pair.right.levels?.length ?? 0);
      for (let level = 0; level < count; level++) {
        const left = this.selected(pair.left, level), right = this.selected(pair.right, level);
        if (!left || !right) continue;
        if (left instanceof THREE.InstancedMesh && right instanceof THREE.InstancedMesh) {
          if (left.count !== right.count) continue;
          const a = left.instanceMatrix.array, b = right.instanceMatrix.array;
          let equal = true;
          for (let i = 0; i < left.count * 16; i++) if (a[i] !== b[i]) { equal = false; break; }
          // One draw carries one tint per instance, so both halves must agree on it.
          if (!equal || !equalInstanceTints(left, right, left.count)) continue;
        }
        let combined = pair.draws.get(level);
        if (!combined) {
          combined = this.combine([left, right]);
          if (!combined) continue;
          pair.draws.set(level, combined);
        }
        if (combined instanceof THREE.InstancedMesh && left instanceof THREE.InstancedMesh) {
          combined.instanceMatrix = left.instanceMatrix;
          combined.instanceColor = left.instanceColor;
          combined.count = left.count;
        }
        left.visible = right.visible = false;
        combined.visible = true;
        this.suppressed.push(left, right);
        this.shown.push(combined);
      }
    }
  }

  /** A spatial cluster can choose a different authored LOD on either side of
   * a corner. Join their selected indices when the exact placement list agrees. */
  private wideFrame(): void {
    const placements = new Map<string | number, THREE.Mesh[]>();
    const bins: THREE.Mesh[][] = [];
    for (const group of this.groups) {
      placements.clear();
      let used = 0;
      for (const source of group.sources) {
        const count = 1 + (source.levels?.length ?? 0);
        for (let level = 0; level < count; level++) {
          const draw = this.selected(source, level);
          if (!draw) continue;
          let key: number | string = -1;
          if ("placements" in source && draw !== source.mesh && (draw as THREE.InstancedMesh).count !== source.mesh.count) {
            const indices = source.members[level];
            key = indices.length === 1 ? indices[0] : indices.join(",");
          }
          let list = placements.get(key);
          if (!list) {
            list = bins[used] ??= [];
            list.length = 0; used++;
            placements.set(key, list);
          }
          list.push(draw);
        }
      }
      for (let i = 0; i < used; i++) {
        const list = bins[i];
        if (list.length < 2) continue;
        const key = list.map(m => m.id + "/" + m.geometry.id).join(",");
        let mesh = group.draws.get(key);
        if (!mesh) {
          mesh = this.combine(list, true);
          if (!mesh) continue;
          group.draws.set(key, mesh);
        }
        const first = list[0];
        if (mesh instanceof THREE.InstancedMesh && first instanceof THREE.InstancedMesh) {
          mesh.instanceMatrix = first.instanceMatrix; mesh.instanceColor = first.instanceColor; mesh.count = first.count;
        }
        for (const original of list) { original.visible = false; this.suppressed.push(original); }
        mesh.visible = true; this.shown.push(mesh);
      }
    }
  }

  private combine(parts: readonly THREE.Mesh[], wide = false): THREE.Mesh | undefined {
    const left = parts[0], a = left.geometry;
    const names = Object.keys(a.attributes);
    let count = 0;
    for (const { geometry: b } of parts) {
      if (!b.index || b.groups.length || b.drawRange.start || b.drawRange.count < b.index.count
        || names.length !== Object.keys(b.attributes).length || names.some(name => a.attributes[name] !== b.attributes[name])) return;
      count += b.index.count;
    }
    const useShort = wide && a.attributes.position.count <= 65535;
    const bytes = count * (useShort ? 2 : 4);
    if (wide ? this.wideEntries >= 512 || this.wideBytes + bytes > 32 * 1024 * 1024
      : this.entries >= 512 || this.bytes + bytes > 32 * 1024 * 1024) return;
    const indices = useShort ? new Uint16Array(count) : new Uint32Array(count);
    const geometry = new THREE.BufferGeometry();
    geometry.boundingBox = new THREE.Box3();
    let offset = 0;
    for (const { geometry: part } of parts) {
      indices.set(part.index!.array, offset);
      offset += part.index!.count;
      if (!part.boundingBox) part.computeBoundingBox();
      geometry.boundingBox.union(part.boundingBox!);
    }
    for (const name of names) geometry.setAttribute(name, a.attributes[name]);
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.boundingSphere = geometry.boundingBox.getBoundingSphere(new THREE.Sphere());
    const mesh = left.clone(false);
    mesh.customDepthMaterial = left.customDepthMaterial;
    mesh.customDistanceMaterial = left.customDistanceMaterial;
    mesh.geometry = geometry;
    mesh.castShadow = false;
    mesh.raycast = () => {};
    if (mesh instanceof THREE.InstancedMesh) {
      mesh.boundingBox = new THREE.Box3();
      for (const part of parts as THREE.InstancedMesh[]) {
        if (!part.boundingBox) part.computeBoundingBox();
        mesh.boundingBox.union(part.boundingBox!);
      }
      mesh.boundingSphere = mesh.boundingBox.getBoundingSphere(new THREE.Sphere());
    }
    left.parent!.add(mesh);
    if (wide) { this.wideBytes += bytes; this.wideEntries++; }
    else { this.bytes += bytes; this.entries++; }
    return mesh;
  }

  restore(): void {
    for (const mesh of this.suppressed) mesh.visible = true;
    for (const mesh of this.shown) mesh.visible = false;
    this.suppressed.length = 0;
    this.shown.length = 0;
  }

  dispose(): void {
    this.restore();
    const detachedMatrix = new THREE.InstancedBufferAttribute(new Float32Array(0), 16);
    for (const group of [...this.pairs, ...this.groups]) for (const mesh of group.draws.values()) {
      mesh.removeFromParent();
      // Three releases every registered attribute on disposal. Only the
      // concatenated index and this draw's VAOs belong to the combined mesh.
      for (const name of Object.keys(mesh.geometry.attributes)) mesh.geometry.deleteAttribute(name);
      mesh.geometry.dispose();
      if (mesh instanceof THREE.InstancedMesh) {
        mesh.instanceMatrix = detachedMatrix;
        // The tints belong to the source draw the combined mesh last stood in for.
        mesh.instanceColor = null;
        mesh.dispose();
      }
    }
  }
}

/** Once visibility is tested per placement, opaque copies of the same primitive
 * can share one draw across street cells. Keep the node transform identical so
 * quantized coordinates and instance normal transforms remain bit-for-bit intact. */
export function mergeCityInstances(root: THREE.Object3D, groupFor?: (mesh: THREE.Mesh) => THREE.Object3D | undefined,
  instanceGroups?: CityInstanceGroups): void {
  root.updateWorldMatrix(true, true);
  const groups = new Map<string, THREE.InstancedMesh[]>();
  root.traverseVisible(mesh => {
    if (!(mesh instanceof THREE.InstancedMesh) || hasDeclaredLodOwner(mesh) || customMeshCallbacks(mesh) || mesh.children.length || mesh.morphTexture ||
        Array.isArray(mesh.material) || mesh.material.transparent ||
        Object.values(mesh.geometry.attributes).some(a => a instanceof THREE.InstancedBufferAttribute)) return;
    // Mixed batches require the same conservative subset support as CityVisibility.
    const partition = groupFor?.(mesh);
    const mixed = instanceGroups && !mesh.morphTargetInfluences?.length && !(mesh.geometry instanceof THREE.InstancedBufferGeometry)
      && mesh.material.depthTest && !(mesh.material instanceof THREE.ShaderMaterial) && !mesh.material.displacementMap
      && !Object.values(mesh.geometry.attributes).some(a =>
        a instanceof THREE.InterleavedBufferAttribute && a.data instanceof THREE.InstancedInterleavedBuffer);
    // Tinted and untinted copies draw with different programs, so they merge separately.
    const key = [mixed ? undefined : partition?.id, mesh.geometry.uuid, mesh.material.uuid, instanceTintLayout(mesh), mesh.layers.mask, mesh.castShadow, mesh.receiveShadow,
      mesh.renderOrder, mesh.customDepthMaterial?.uuid, mesh.customDistanceMaterial?.uuid, mesh.matrixWorld.elements.join(",")].join("|");
    const copies = groups.get(key);
    if (copies) copies.push(mesh);
    else groups.set(key, [mesh]);
  });
  for (const copies of groups.values()) {
    if (copies.length < 2) continue;
    const partitions = instanceGroups && groupFor ? copies.map(groupFor) : undefined;
    const mixed = partitions && partitions.some(group => group !== partitions[0]);
    // An ordinary outside anchor has the identical world transform. A batch
    // shared only by different proxy blocks has no always-visible anchor.
    const anchor = mixed ? partitions.findIndex(group => !group) : 0;
    if (anchor < 0) continue;
    const first = copies[anchor];
    const combined = first.clone(false);
    combined.count = copies.reduce((sum, mesh) => sum + mesh.count, 0);
    const matrices = new Float32Array(combined.count * 16);
    let offset = 0;
    for (const mesh of copies) {
      matrices.set(mesh.instanceMatrix.array.subarray(0, mesh.count * 16), offset);
      offset += mesh.count * 16;
    }
    combined.instanceMatrix = new THREE.InstancedBufferAttribute(matrices, 16);
    const tint = first.instanceColor;
    if (tint) {
      // Every copy has the anchor's tint layout: it is part of the grouping key.
      const size = tint.itemSize;
      const tints = new (tint.array.constructor as new (length: number) => THREE.TypedArray)(combined.count * size);
      let at = 0;
      for (const mesh of copies) {
        tints.set(mesh.instanceColor!.array.subarray(0, mesh.count * size), at);
        at += mesh.count * size;
      }
      combined.instanceColor = new THREE.InstancedBufferAttribute(tints, size, tint.normalized);
    }
    if (mixed) instanceGroups!.set(combined, copies.flatMap((mesh, i) => Array<THREE.Object3D | undefined>(mesh.count).fill(partitions![i])));
    combined.boundingBox = combined.boundingSphere = null;
    combined.customDepthMaterial = first.customDepthMaterial;
    combined.customDistanceMaterial = first.customDistanceMaterial;
    first.parent!.add(combined);
    for (const mesh of copies) {
      mesh.removeFromParent();
      mesh.dispose();
    }
  }
}
