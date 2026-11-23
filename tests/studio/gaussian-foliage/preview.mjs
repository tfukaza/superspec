import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import { decodePacket } from "../../../browser/model/packet.ts";
import { attributeName, batchGeometry, batchInstances, instancedBatch } from "../../../browser/renderer/batch.ts";
import { FoliageVisibility } from "../../../browser/renderer/foliage-visibility.ts";
import { reshapeGaussianClumpNormal, reshapeGaussianClumpPoint } from "../../../browser/renderer/treespec-gaussian-clump-form.ts";
import { treeSpecCanopyHue, treeSpecGaussianCoreDabs, treeSpecGaussianDabs, treeSpecGaussianForm, treeSpecGaussianTuftDabs } from "../../../browser/renderer/treespec-gaussian-dabs.ts";
import { createGaussianCanopyMaterial, createGaussianSplats } from "../../../browser/renderer/treespec-gaussian-renderer.mjs";
import shrubPacket from "../../../generated/treespec-gaussian-foliage/brush.tree.lsp3?url";
import grassPacket from "../../../generated/treespec-gaussian-foliage/brush.grass.lsp3?url";
import birchPacket from "../../../generated/treespec-gaussian-foliage/birch.tree.lsp3?url";
import poplarPacket from "../../../generated/treespec-gaussian-foliage/poplar.tree.lsp3?url";
import { ground, shrubBase, slab } from "./site.mjs";
import { treeDemoPresets } from "./tree-demo-presets.mjs";

const byId = id => document.getElementById(id);
const metrics = byId("metrics");
const viewport = byId("viewport");
const options = new URLSearchParams(location.search);
const treePackets = { birch: birchPacket, poplar: poplarPacket };
const treeScene = options.get("trees");
const treeNames = treeScene === "both" ? ["birch", "poplar"] : ["birch", "poplar"].filter(name => name === treeScene);
const tallTrees = treeNames.length > 0;
const side = tallTrees ? 1 : [4, 8].find(value => value === Number(options.get("field"))) ?? 1;
const patches = Array.from({ length: side * side }, (_, index) => new THREE.Vector3(
  (index % side - Math.floor(side / 2)) * 4.2, (Math.floor(index / side) - Math.floor(side / 2)) * 3.6, 0,
));
const plots = treeNames.length === 2 ? [new THREE.Vector3(-2.1, 0, 0), new THREE.Vector3(2.1, 0, 0)] : patches;
const focus = tallTrees ? new THREE.Vector3(0, 0, 4.25)
  : side === 1 ? new THREE.Vector3(-0.18, -0.12, 0.16) : new THREE.Vector3(-2.1, -1.8, 0.16);
byId("field").value = tallTrees ? treeScene : String(side);
byId("canopy-label").textContent = treeNames.length === 2 ? "Trees" : tallTrees ? treeDemoPresets[treeNames[0]].label : "Shrub";
byId("field").addEventListener("input", () => {
  const url = new URL(location.href);
  const choice = byId("field").value;
  url.searchParams.delete("trees");
  url.searchParams.delete("field");
  if (["birch", "poplar", "both"].includes(choice)) url.searchParams.set("trees", choice);
  else if (choice !== "1") url.searchParams.set("field", choice);
  location.href = url;
});

async function loadPacket(url) {
  const response = await fetch(url);
  if (!response.ok || response.headers.get("content-type")?.includes("text/html")) {
    throw new Error("Build the TreeSpec packets with build.mjs before opening the demo.");
  }
  return decodePacket(await response.arrayBuffer());
}

const launch = async () => {
  const [grass, ...plants] = await Promise.all(
    [grassPacket, ...(tallTrees ? treeNames.map(name => treePackets[name]) : [shrubPacket])].map(loadPacket),
  );
  const scene = new THREE.Scene();
  scene.background = new THREE.Color("#aeb9a9");
  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.5));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  renderer.shadowMap.autoUpdate = false;
  viewport.append(renderer.domElement);
  const camera = new THREE.PerspectiveCamera(38, 1, 0.08, 140);
  camera.up.set(0, 0, 1);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.minDistance = 0.55;
  controls.maxDistance = tallTrees ? 55 : side === 1 ? 14 : 105;
  controls.maxPolarAngle = Math.PI * 0.49;
  controls.autoRotateSpeed = 1.05;
  function resetView() {
    if (tallTrees) camera.position.set(3.7, treeNames.length === 2 ? -17.0 : -14.7, 6.3);
    else if (side === 1) camera.position.set(3.15, -3.84, 1.98);
    else camera.position.copy(focus).add(new THREE.Vector3(1.7 * side, -5.6 * side, 3.4 * side));
    controls.target.copy(focus);
    controls.update();
  }
  resetView();
  addGround(scene, plots);
  addConcrete(scene, plots);
  scene.add(new THREE.HemisphereLight("#f2f7df", "#69634a", 1.15));
  const sun = new THREE.DirectionalLight("#fff0cd", 2.25);
  const shadowRange = tallTrees ? 9 : side === 1 ? 3.2 : 8;
  Object.assign(sun.shadow.camera, { left: -shadowRange, right: shadowRange, top: shadowRange, bottom: -shadowRange, near: 0.1, far: 42 });
  sun.shadow.camera.updateProjectionMatrix();
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.normalBias = 0.014;
  sun.shadow.bias = -0.0003;
  sun.shadow.intensity = 0.82;
  if (tallTrees) sun.target.position.z = 2.8;
  scene.add(sun, sun.target);
  const setSun = (azimuth, elevation) => {
    const angle = THREE.MathUtils.degToRad(azimuth), height = THREE.MathUtils.degToRad(elevation);
    sun.position.set(Math.cos(angle) * Math.cos(height), Math.sin(angle) * Math.cos(height), Math.sin(height))
      .multiplyScalar(22).add(sun.target.position);
    renderer.shadowMap.needsUpdate = true;
  };

  const bark = new THREE.Group();
  const canopyCasters = new THREE.Group();
  const grassCasters = new THREE.Group();
  scene.add(bark, canopyCasters, grassCasters);
  const plantBase = new THREE.Vector3().fromArray(shrubBase);
  const grassTemplatePlaces = tallTrees ? plots : [new THREE.Vector3()];
  // Keep shadow texel density fixed. The large field has a local sun-shadow region.
  const nearPlots = plots.filter(at => Math.abs(at.x) <= shadowRange && Math.abs(at.y) <= shadowRange);
  const shadowOnly = new THREE.MeshBasicMaterial({ colorWrite: false, depthWrite: false, stencilWrite: false });
  const canopyHueStrength = { value: 1 };
  const hullMaterial = createGaussianCanopyMaterial(canopyHueStrength);
  const treeDetails = [];
  const dabs = [];
  const interiorDabs = [];
  const tuftDabs = [];
  function placeDabs(brush, offsets, tree, level, target = dabs) {
    for (const { x, y, z } of offsets) {
      for (const dab of brush) {
        const [px, py, pz] = dab.position;
        target.push({ ...dab, position: [px + x, py + y, pz + z], tree, level });
      }
    }
  }
  for (const [index, packet] of plants.entries()) {
    const form = treeSpecGaussianForm(packet);
    const places = tallTrees ? [plots[index].clone().add(plantBase)] : plots.map(at => at.clone().add(plantBase));
    const shadowPlaces = tallTrees ? places : nearPlots.map(at => at.clone().add(plantBase));
    const barkColor = tallTrees ? treeDemoPresets[treeNames[index]].barkColor : "#69553a";
    const barkMaterial = new THREE.MeshStandardMaterial({ color: barkColor, roughness: 0.94 });
    const distances = [];
    const detailMeshes = [];
    const bounds = new THREE.Box3();
    for (const batch of packet.manifest.batches) {
      const look = batch.foliage;
      if (!look) continue;
      const { family, level, distance } = look;
      if (tallTrees && (family === "bark" || (family === "hull" && level >= 2))) {
        const mesh = repeatBatch(packet, batch, family === "bark" ? barkMaterial : hullMaterial, places);
        if (family === "hull") prepareCanopyHull(mesh, form?.clumps);
        mesh.receiveShadow = true;
        mesh.visible = false;
        bark.add(mesh);
        detailMeshes.push({ level, mesh });
        if (family === "bark") distances.push([level, distance]);
        if (family === "bark" && level === 1) canopyCasters.add(shadowCaster(packet, batch, shadowOnly, places));
      } else if (!tallTrees && family === "bark" && level === (side === 1 ? 0 : 1)) {
        const mesh = repeatBatch(packet, batch, barkMaterial, places);
        mesh.castShadow = mesh.receiveShadow = true;
        bark.add(mesh);
      } else if (family === "shadow") {
        canopyCasters.add(shadowCaster(packet, batch, shadowOnly, shadowPlaces));
      } else if (family === "leaves" && (tallTrees || level === 0)) {
        const brush = treeSpecGaussianDabs(packet, batch, tallTrees && level === 0, form);
        placeDabs(brush, tallTrees ? places : [plantBase], tallTrees ? index : undefined, level);
      }
      if (tallTrees && level === 0 && (family === "bark" || family === "leaves" || family === "hull")) {
        bounds.expandByPoint(new THREE.Vector3().fromArray(batch.bounds[0]).add(places[0]));
        bounds.expandByPoint(new THREE.Vector3().fromArray(batch.bounds[1]).add(places[0]));
      }
    }
    if (form) {
      placeDabs(treeSpecGaussianCoreDabs(form), tallTrees ? places : [plantBase], tallTrees ? index : undefined, 0, interiorDabs);
      if (tallTrees) placeDabs(treeSpecGaussianTuftDabs(packet, form), places, index, 0, tuftDabs);
    }
    if (tallTrees) {
      const visibility = new FoliageVisibility(new Map([[0, bounds.expandByScalar(0.25)]]),
        distances.sort(([a], [b]) => a - b), Infinity);
      treeDetails.push({ meshes: detailMeshes, label: treeDemoPresets[treeNames[index]].label, level: -1, visibility });
    }
  }
  for (const batch of grass.manifest.batches) {
    if (batch.foliage?.level === 1) grassCasters.add(shadowCaster(grass, batch, shadowOnly, nearPlots));
    else if (batch.foliage?.level === 0) placeDabs(treeSpecGaussianDabs(grass, batch), grassTemplatePlaces, undefined, 0);
  }
  if (!dabs.some(dab => dab.kind === "leaves") || !dabs.some(dab => dab.kind === "grass")) {
    throw new Error("The TreeSpec packets need both leaf cards and grass blades.");
  }
  const splats = createGaussianSplats([...dabs, ...interiorDabs, ...tuftDabs], patches);
  splats.material.uniforms.canopyHueStrength = canopyHueStrength;
  scene.add(splats.mesh);

  function selectDabs() {
    const fill = byId("interior-fill").checked;
    splats.select({
      leaves: bark.visible, grass: byId("grass").checked, density: byId("density").valueAsNumber / 100,
      policy: tallTrees ? "all" : byId("detail").value,
      includes: dab => {
        if (dab.interior && !fill) return false;
        if (!tallTrees || dab.tree === undefined) return true;
        const level = treeDetails[dab.tree].level;
        return dab.interior ? level === 0 || level === 1 : level === dab.level;
      },
    });
  }
  const frustum = new THREE.Frustum(), cameraClip = new THREE.Matrix4();
  function updateTreeDetail() {
    if (!tallTrees) return;
    const policy = byId("detail").value;
    camera.updateMatrixWorld();
    frustum.setFromProjectionMatrix(cameraClip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
    let changed = false;
    for (const detail of treeDetails) {
      detail.visibility.update(frustum, camera.position, policy === "lod" ? 1 : false);
      const level = policy === "all" ? 0 : detail.visibility.chosen.get(0);
      if (level === detail.level) continue;
      detail.level = level;
      for (const part of detail.meshes) part.mesh.visible = part.level === level;
      changed = true;
    }
    if (changed) selectDabs();
  }
  const updateControls = () => {
    bark.visible = byId("shrub").checked;
    byId("density-value").value = byId("density").value + "%";
    selectDabs();
  };
  for (const id of ["density", "detail", "interior-fill"]) byId(id).addEventListener("input", updateControls);
  const updateProjection = () => { splats.material.uniforms.billboard.value = byId("projection").value === "billboard"; };
  const updateOrbit = () => { controls.autoRotate = byId("orbit").checked; };
  const updateFirmness = () => { splats.material.uniforms.firmness.value = byId("firmness").valueAsNumber / 100; };
  const updateLeafNormals = () => {
    const degrees = byId("leaf-normal-variation").valueAsNumber;
    splats.material.uniforms.leafNormalTilt.value = Math.tan(THREE.MathUtils.degToRad(degrees));
    byId("leaf-normal-value").value = `${degrees}°`;
  };
  const updatePigments = () => { splats.setPigmentPatches(byId("pigment-patches").checked); };
  const updateHue = () => { canopyHueStrength.value = byId("leaf-hue-variation").checked ? 1 : 0; };
  const updateLight = () => {
    setSun(byId("sun").valueAsNumber, byId("elevation").valueAsNumber);
    splats.material.uniforms.relight.value = byId("relight").checked;
  };
  const updateCasters = () => {
    sun.castShadow = byId("shadows").checked;
    canopyCasters.visible = sun.castShadow && byId("shrub").checked;
    grassCasters.visible = sun.castShadow && byId("grass").checked;
    renderer.shadowMap.needsUpdate = true;
  };
  for (const id of ["shrub", "grass"]) byId(id).addEventListener("input", () => { updateControls(); updateCasters(); });
  byId("projection").addEventListener("input", updateProjection);
  byId("orbit").addEventListener("input", updateOrbit);
  byId("firmness").addEventListener("input", updateFirmness);
  byId("leaf-normal-variation").addEventListener("input", updateLeafNormals);
  byId("pigment-patches").addEventListener("input", updatePigments);
  byId("leaf-hue-variation").addEventListener("input", updateHue);
  for (const id of ["sun", "elevation", "relight"]) byId(id).addEventListener("input", updateLight);
  byId("shadows").addEventListener("input", updateCasters);
  byId("reset").addEventListener("click", resetView);
  if (tallTrees) {
    const closeup = byId("canopy-closeup");
    closeup.hidden = false;
    closeup.addEventListener("click", () => {
      controls.target.copy(plots[0]).add(new THREE.Vector3(0.1, 0.15, 4.35));
      camera.position.copy(controls.target).add(new THREE.Vector3(2.8, -6.65, -1.2));
      controls.update();
    });
  }
  updateControls();
  updateProjection();
  updateOrbit();
  updateFirmness();
  updateLeafNormals();
  updatePigments();
  updateHue();
  updateLight();
  updateCasters();
  let last = performance.now(), frames = 0, sortTime = 0, sortedFrames = 0;
  const resize = new ResizeObserver(() => {
    const width = viewport.clientWidth, height = viewport.clientHeight;
    renderer.setSize(width, height, false);
    renderer.getDrawingBufferSize(splats.material.uniforms.viewport.value);
    camera.aspect = width / height;
    camera.fov = width < height ? Math.min(64, 38 * height / width) : 38;
    camera.updateProjectionMatrix();
  });
  resize.observe(viewport);
  function draw(timer) {
    updateTreeDetail();
    const prepare = splats.prepare(camera);
    const submitStart = performance.now();
    const gl = renderer.getContext();
    const redrawShadow = sun.castShadow && renderer.shadowMap.needsUpdate;
    if (timer) gl.beginQuery(timer.target, timer.query);
    renderer.render(scene, camera);
    if (timer) gl.endQuery(timer.target);
    return { ...prepare, submitMs: performance.now() - submitStart, ...splats.stats(), calls: renderer.info.render.calls, redrawShadow,
      treeLevels: treeDetails.map(({ label, level }) => ({ label, level })) };
  }
  if (options.has("bench")) window.foliageBenchmark = { renderer, scene, camera, focus, draw, setSun, resetView };
  renderer.setAnimationLoop(now => {
    controls.update();
    const sample = draw();
    sortTime += sample.sortMs + sample.packMs;
    if (sample.sortMs + sample.packMs > 0) sortedFrames++;
    frames++;
    if (now - last < 400) return;
    const fps = Math.round(frames * 1000 / (now - last));
    const sort = sortedFrames ? (sortTime / sortedFrames).toFixed(2) + " ms sort" : "cached sort";
    const sceneDetail = tallTrees
      ? sample.treeLevels.map(({ label, level }) => level < 0 ? `${label} offscreen` : `${label} LOD ${level}`).join(" · ")
      : `${sample.visiblePatches}/${patches.length} patches`;
    const dabCount = tallTrees ? sample.activeCount.toLocaleString()
      : `${sample.activeCount.toLocaleString()} / ${sample.capacity.toLocaleString()}`;
    metrics.value = `${dabCount} dabs · ${sceneDetail} · ${sample.calls} draws · ${fps} FPS · ${sort}`;
    last = now;
    frames = 0;
    sortTime = 0;
    sortedFrames = 0;
  });
};

export const ready = launch().catch(error => {
  metrics.value = error.message;
  throw error;
});

function prepareCanopyHull(mesh, clumps) {
  const geometry = mesh.geometry;
  const positions = geometry.getAttribute("position"), normals = geometry.getAttribute("normal");
  const elements = geometry.getAttribute(attributeName("vertex", "element"));
  const point = new THREE.Vector3(), normal = new THREE.Vector3();
  const colors = new Float32Array(positions.count);
  for (let vertex = 0; vertex < positions.count; vertex++) {
    point.fromBufferAttribute(positions, vertex);
    // TreeSpec numbers shell clumps from zero and encodes their leaf batch IDs as -1 - index.
    const clump = elements && clumps?.get(-1 - elements.getX(vertex));
    if (clump) {
      reshapeGaussianClumpPoint(point, clump);
      reshapeGaussianClumpNormal(normal.fromBufferAttribute(normals, vertex), clump);
      positions.setXYZ(vertex, point.x, point.y, point.z);
      normals.setXYZ(vertex, normal.x, normal.y, normal.z);
    }
    colors[vertex] = treeSpecCanopyHue(point.x, point.y, point.z);
  }
  geometry.setAttribute("canopyHue", new THREE.BufferAttribute(colors, 1));
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  mesh.computeBoundingBox();
  mesh.computeBoundingSphere();
}

function repeatBatch(packet, batch, material, offsets) {
  const source = batchInstances(packet, batch);
  const placements = new Float32Array(source.placements.length * offsets.length);
  for (const [index, offset] of offsets.entries()) {
    const start = index * source.placements.length;
    placements.set(source.placements, start);
    for (let at = start; at < start + source.placements.length; at += 10) {
      placements[at] += offset.x;
      placements[at + 1] += offset.y;
      placements[at + 2] += offset.z;
    }
  }
  return instancedBatch(batchGeometry(packet, batch), material, { count: source.count * offsets.length, placements });
}

function shadowCaster(packet, batch, material, offsets) {
  const mesh = repeatBatch(packet, batch, material, offsets);
  mesh.castShadow = true;
  return mesh;
}

function addCopies(scene, geometry, material, offsets, center = new THREE.Vector3()) {
  const mesh = new THREE.InstancedMesh(geometry, material, offsets.length);
  const matrix = new THREE.Matrix4();
  for (const [index, offset] of offsets.entries()) {
    matrix.makeTranslation(offset.x + center.x, offset.y + center.y, offset.z + center.z);
    mesh.setMatrixAt(index, matrix);
  }
  mesh.receiveShadow = true;
  scene.add(mesh);
  return mesh;
}

function addGround(scene, patches) {
  const surface = new THREE.BufferGeometry();
  surface.setIndex(ground.indices);
  surface.setAttribute("position", new THREE.Float32BufferAttribute(ground.positions.flat(), 3));
  surface.computeVertexNormals();
  const deep = new THREE.Color("#556543"), light = new THREE.Color("#86925a"), color = new THREE.Color();
  const colors = [];
  for (const [x, y] of ground.positions) {
    const variation = Math.sin(6.8 * x + 5 * y) * Math.cos(5.3 * y - x) * 0.07 + 0.17 * Math.sin(2.1 * x - 3 * y);
    color.copy(deep).lerp(light, 0.57 + variation);
    colors.push(...color);
  }
  surface.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  addCopies(scene, surface, new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1 }), patches);

  // The exposed sides give the lawn section a continuous edge when orbiting.
  const edges = new Map();
  for (let index = 0; index < ground.indices.length; index += 3) {
    const [a, b, c] = ground.indices.slice(index, index + 3);
    for (const [from, to] of [[a, b], [b, c], [c, a]]) {
      const key = Math.min(from, to) + ":" + Math.max(from, to);
      if (edges.has(key)) edges.delete(key);
      else edges.set(key, [from, to]);
    }
  }
  const sides = [];
  for (const [a, b] of edges.values()) {
    const topA = ground.positions[a], topB = ground.positions[b];
    const baseA = [topA[0], topA[1], -0.16], baseB = [topB[0], topB[1], -0.16];
    sides.push(...topA, ...baseA, ...baseB, ...topA, ...baseB, ...topB);
  }
  const soil = new THREE.BufferGeometry();
  soil.setAttribute("position", new THREE.Float32BufferAttribute(sides, 3));
  soil.computeVertexNormals();
  addCopies(scene, soil, new THREE.MeshStandardMaterial({ color: "#616447", roughness: 1 }), patches);
}

function addConcrete(scene, patches) {
  const depth = 0.16;
  const geometry = new RoundedBoxGeometry(slab.max[0] - slab.min[0], slab.max[1] - slab.min[1], depth, 4, 0.014);
  const material = new THREE.MeshStandardMaterial({ color: "#b8b2a2", roughness: 0.96, vertexColors: true });
  const vertices = geometry.attributes.position;
  const colors = [];
  for (let index = 0; index < vertices.count; index++) {
    const x = vertices.getX(index), y = vertices.getY(index);
    const shade = 0.84 + 0.07 * Math.sin(x * 19 + Math.cos(y * 16)) * Math.sin(y * 29 - x * 5);
    colors.push(shade, shade, shade);
  }
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  const center = new THREE.Vector3((slab.min[0] + slab.max[0]) / 2, (slab.min[1] + slab.max[1]) / 2, slab.top - depth / 2);
  addCopies(scene, geometry, material, patches, center).castShadow = true;
}
