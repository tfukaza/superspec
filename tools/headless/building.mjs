import * as THREE from "three";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { compile } from "./host.mjs";
import { decodePacket, modelsOf, cityProject, SceneBaker, programMaterial, atlasLayouts, gpuBakeUnsupported } from "../../generated/headless/core.js";

// Publication holds the maps for one complete prototype until its atlas pages
// have been read back. Large Stoa buildings can exceed the interactive budget.
const BUILDING_BAKE_ALLOWANCE_MIB = 8192;

function meshOf(packet, row, definition) {
  const geometry = new THREE.BufferGeometry();
  for (const [name, values, size] of [["position", packet.positions, 3], ["normal", packet.normals, 3], ["uv", packet.uvs, 2], ["color", packet.colors, 3]])
    geometry.setAttribute(name, new THREE.BufferAttribute(values.subarray(row.vertexStart * size, (row.vertexStart + row.vertexCount) * size), size));
  geometry.setIndex(new THREE.BufferAttribute(packet.indices.subarray(row.indexStart, row.indexStart + row.indexCount), 1));
  const material = new THREE.MeshStandardMaterial({ vertexColors: true });
  if (definition?.baseColorFactor) material.color.fromArray(definition.baseColorFactor);
  else material.color.set(definition?.color ?? "#b9c7d1");
  material.transparent = definition?.alphaMode === "BLEND";
  material.alphaTest = definition?.alphaMode === "MASK" ? definition.alphaCutoff ?? 0.5 : 0;
  const mesh = new THREE.Mesh(geometry, material);
  mesh.name = row.id;
  return mesh;
}

function receiverKey(spec) {
  const hash = createHash("sha256");
  hash.update(JSON.stringify([spec.features, spec.zones, spec.anchor, spec.objectToWorld, spec.boundsMin, spec.boundsMax]));
  return hash.digest("hex");
}

/** Bake one unplaced WorldSpec building using its native ItemSpec project and original receiver geometry. */
export async function bakeBuilding(host, source, nativeMeshes) {
  const packet = decodePacket(await compile("aml-scene", { source, revision: 1 }));
  if (!packet.manifest.success) throw Error(JSON.stringify(packet.manifest.diagnostics));
  const { models, owners } = modelsOf(packet);
  if (models.length !== 1) throw Error("Expected one building prototype");
  const project = cityProject(models, packet, {});
  // Source image companions used by the shipped weathering recipes. Catalog images resolve in the native host.
  for (const name of ["concrete-plain.png", "concrete-fine-sand.png"])
    project.assets["textures/" + name] = await readFile(new URL("../../crates/engine/src/materialspec/program/examples/textures/" + name, import.meta.url));
  const { metadata, layouts } = await compile("shade-layouts", { ...project, includeScenePacket: false });
  if (!layouts) throw Error(metadata.layoutError || "Missing native building atlas layouts");
  const layout = atlasLayouts(layouts);
  const specs = metadata.shading;
  for (const item of Object.values(metadata.items ?? {}))
    for (const part of item.parts) if (part.anchor)
      for (const id of part.meshes) if (specs[id]) specs[id].anchor = part.anchor;
  const toWorld = id => { const at = id.indexOf("/"); return owners.get(id.slice(0, at)) + id.slice(at); };
  const definitions = new Map(packet.manifest.materials.map(m => [m.id, m]));
  const meshes = packet.manifest.meshes.filter(m => m.visible).map(m => meshOf(packet, m, definitions.get(m.material)));
  const byId = new Map(meshes.map(m => [m.name, m]));
  const targets = [], prepared = new Map();
  const keep = new Set(nativeMeshes);
  for (const [local, spec] of Object.entries(specs)) {
    const mesh = byId.get(toWorld(local));
    if (!mesh || keep.has(mesh.name)) continue;
    const assigned = project.materials.find(m => m.id === spec.material) ?? project.materials.find(m => spec.material.startsWith(m.id + "."));
    if (!assigned) throw Error("Unknown assignment " + spec.material);
    mesh.userData.materialSpecId = assigned.id;
    let binding = prepared.get(spec.material);
    if (!binding) {
      const original = metadata.materials[spec.material];
      const material = { ...original, mapping: project.mappings?.[assigned.id] ?? original.mapping };
      const plan = original.plan;
      const use = plan && (gpuBakeUnsupported(material, true) || plan.shader.cpuFields?.some(f => f.planes?.length))
        ? { ...material, ...plan } : material;
      binding = { use, uniforms: await host.uniforms(use, assigned.parameters, project.assets), layers: [], channels: material.channels };
      for (const layer of material.layers ?? []) {
        if (!layer.plan) throw Error("Missing layer plan for " + layer.entry);
        const shading = { ...layer.plan, parameters: layer.metadata?.parameters ?? [], mapping: material.mapping };
        binding.layers.push({ id: layer.id, shading, reads: layer.reads ?? {}, uniforms: await host.uniforms(shading, layer.parameters ?? {}, project.assets) });
      }
      prepared.set(spec.material, binding);
    }
    const shader = programMaterial(binding.use, spec, project.gravity, binding.uniforms, "quad");
    const target = { mesh, material: spec.material, shader, shading: binding.use, uniforms: shader.userData.programUniforms,
      layout: layout.get(local), receiver: receiverKey(spec), extent: project.domain, fieldsId: local };
    if (!target.layout) throw Error("Missing chart for " + local);
    if (binding.layers.length) {
      target.stack = { channels: binding.channels ?? {}, layers: binding.layers.map((layer, i) => {
        const own = { ...spec, features: spec.layerFeatures?.[i] ?? [], zones: spec.layerZones?.[i] };
        const mat = programMaterial(layer.shading, own, project.gravity, layer.uniforms, "quad");
        return { ...layer, uniforms: mat.userData.programUniforms, receiver: receiverKey(own) };
      }) };
    }
    targets.push(target);
  }
  const shaded = new Set(targets.map(target => target.mesh.name));
  for (const row of packet.manifest.meshes) {
    if (!row.visible || keep.has(row.id) || shaded.has(row.id)) continue;
    const material = definitions.get(row.material);
    if ([material?.texture, material?.normalTexture, material?.metallicRoughnessTexture,
      material?.emissiveTexture, material?.occlusionTexture, material?.heightTexture].some(Boolean))
      throw Error("Textured receiver needs a native copy or material bake: " + row.id);
  }
  let status;
  const baking = new SceneBaker({ baker: () => host.baker, style() {}, compile: async () => {}, displayed: () => true, draw() {}, report(value) { status = value; } });
  host.baker.setSceneAllowance(BUILDING_BAKE_ALLOWANCE_MIB);
  const run = baking.prepare(targets, async requests => {
    const { projectFields } = await compile("project-fields", { ...project, fieldMeshes: requests, withOcclusion: requests.some(r => r.planes.some(p => p.length)) });
    return new Map(projectFields.map(f => [f.mesh, f]));
  }, true);
  await run.ready;
  run.commit(false);
  if (Object.keys(status.unsupported).length) throw Error("Unbaked receivers: " + JSON.stringify(status.unsupported));
  // The atlas each baked mesh's maps and display triangles follow, for a publisher that re-lays its charts.
  return { meshes, layouts: new Map(targets.map(target => [target.mesh.name, target.layout])), close: () => baking.dispose() };
}
