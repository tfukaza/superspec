/** Exercises production WorldSpec project export and catalog admission in actual Chromium/Wasm. */
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, extname, resolve, sep } from 'node:path';
import { promisify } from 'node:util';
import { PNG } from 'pngjs';
import { launchBrowser } from '../tools/browser.mjs';

const output = resolve('generated/verification/authoring-world');
await mkdir(output, { recursive: true });
const suffix = process.platform === 'win32' ? '.exe' : '';
const cli = resolve(process.env.RUST_CLI ?? `target/release/superspec${suffix}`);
const studio = resolve(process.env.STUDIO_CLI ?? resolve(dirname(cli), `superspec-studio${suffix}`));
const grime = PNG.sync.write({ width: 2, height: 2, data: Buffer.from([
  30, 100, 170, 255, 220, 140, 60, 255,
  80, 190, 110, 255, 180, 50, 230, 255,
]) });
const recipe = image => `material 1;
import "./shared.ms";
param tint: vec3 = vec3(1,1,1);
output base_color = mix(grime(uv), sample_color("superspec-material:${image}",uv), 0.25) * tint;
output roughness = 0.65;
`;
const item = {
  format: 'itemspec', version: 1, id: 'saved-pipe',
  parameters: { length: { type: 'number', role: 'geometry', default: 1, min: 0.1, max: 4 } },
  generator: { kind: 'pipe', length: { parameter: 'length' }, radius: 0.1, wallThickness: 0.02, segments: 8, capped: true },
  slots: { surface: { entry: 'saved.ms', parameters: { tint: [0.8, 1, 0.7] }, seed: 5 } },
  collision: 'mesh',
};
const placement = x => [[1, 0, 0], [0, 1, 0], [0, 0, 1], [x, 0, 0.2]];
const project = {
  schemaVersion: 1, kind: 'world', entry: 'scene.aml',
  modules: {
    'scene.aml': `<architecture version="1" id="browser-world" name="Authored world parity">
<box id="slab" width="4" depth="2" height="0.2" material="builtin:timber"/>
<box id="trim" position="0 1.5 0" width="4" depth="0.1" height="0.2" material="builtin:timber"/>
</architecture>`,
    'pipe.item.json': JSON.stringify(item, null, 2),
    'shared.ms': `material 1;
fn grime(p:vec2)->vec3 = sample_color("grime.png",p);
fn unused(p:vec2)->vec3 = sample_color("superspec-material:rust.png",p);
`,
    'saved.ms': recipe('metal.png'),
    'override.ms': recipe('sheet.png'),
    'slab.ms': recipe('roofing.png'),
    'unreferenced.ms': 'material 1; output base_color=sample_color("superspec-material:rust.png",uv);',
  },
  parameters: {}, seed: 7, resolution: 16, inspection: 'final', mapping: 'object', domain: [1, 1],
  base: { kind: 'swatch' }, target: { scope: 'mesh', enabled: false }, edges: [], gravity: [0, 0, -1],
  world: {
    themePresets: { 'pipe.item.json': { length: 1.2 } },
    projectPresets: { 'pipe.item.json': { length: 1.4 } },
    items: [
      { id: 'saved', entry: 'pipe.item.json', transform: placement(-0.8) },
      { id: 'override', entry: 'pipe.item.json', parameters: { length: 1.8 }, transform: placement(0.8),
        slots: { surface: { entry: 'override.ms', parameters: { tint: [1, 0.6, 0.9] }, seed: 11 } } },
    ],
    materials: [{ objects: ['slab'], entry: 'slab.ms', parameters: { tint: [0.7, 0.8, 1] }, seed: 13, mapping: 'world' }],
  },
};
const projectPath = resolve(output, 'world.project.json');
await writeFile(projectPath, JSON.stringify({ ...project, assets: { 'grime.png': grime.toString('base64') } }, null, 2));
const { stdout: native } = await promisify(execFile)(studio, ['glb', projectPath], { encoding: 'buffer', maxBuffer: 64 * 1024 * 1024 });
await writeFile(resolve(output, 'native.glb'), native);

const expectedCatalog = ['metal.png', 'roofing.png', 'sheet.png', 'timber.png'].map(name => `superspec-material:${name}`).sort();
const catalog = Object.fromEntries(await Promise.all(expectedCatalog.map(async name =>
  [name, Array.from(await readFile(resolve('assets/materials', name.slice('superspec-material:'.length))))])));
const root = resolve('generated/wasm');
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    if (name === '/') { response.setHeader('Content-Type', 'text/html'); response.end('<!doctype html><title>World project parity</title>'); return; }
    const path = resolve(root, `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader('Content-Type', extname(path) === '.wasm' ? 'application/wasm' : 'text/javascript');
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async ({ project, catalog, grime }) => {
    const engine = await import('/superspec_wasm.js');
    await engine.default({ module_or_path: '/superspec_wasm_bg.wasm' });
    const session = new engine.Authoring();
    try {
      const preview = JSON.parse(session.build(JSON.stringify({ kind: 'material', entry: 'preview.ms', source: 'material 1; output base_color=vec3(0.4);', resolution: 16 }), 0));
      if (!preview.success) throw new Error(JSON.stringify(preview));
      const before = Array.from(session.packet());
      const encoded = JSON.stringify(project);
      const discovered = JSON.parse(session.project_library_assets(encoded));
      const overriddenOnly = structuredClone(project);
      overriddenOnly.world.items = overriddenOnly.world.items.filter(item => item.id === 'override');
      const overrideCatalog = JSON.parse(session.project_library_assets(JSON.stringify(overriddenOnly)));
      for (const name of discovered) {
        if (!catalog[name]) throw new Error(`Unexpected catalog dependency ${name}`);
        session.set_asset(name, Uint8Array.from(catalog[name]));
      }
      let failure;
      try { session.export_project(encoded, 0); }
      catch (error) { failure = JSON.parse(String(error)); }
      const afterFailure = Array.from(session.packet());
      session.set_asset('grime.png', Uint8Array.from(grime));
      const glb = Array.from(session.export_project(encoded, 0));
      const after = Array.from(session.packet());
      return { discovered, overrideCatalog, failure, before, afterFailure, after, glb };
    } finally { session.free(); }
  }, { project, catalog, grime: Array.from(grime) });
  assert.deepEqual(errors, []);
  assert.deepEqual(result.discovered, expectedCatalog, 'World preflight finds each active catalog source without loading unrelated modules or uncalled helpers.');
  assert.deepEqual(result.overrideCatalog, expectedCatalog.filter(name => name !== 'superspec-material:metal.png'), 'Explicit placement slots replace saved finish dependencies; unused original finishes are not preloaded.');
  assert.ok(result.failure?.some(error => error.message.includes('grime.png') && typeof error.sourceName === 'string' && error.line >= 1), 'A missing shared image produces a source-linked project diagnostic.');
  assert.deepEqual(result.afterFailure, result.before, 'Failed world export retains the accepted preview.');
  assert.deepEqual(result.after, result.before, 'Successful world export retains the accepted preview.');
  const actual = Buffer.from(result.glb);
  await writeFile(resolve(output, 'wasm.glb'), actual);
  assert.equal(actual.length, native.length, 'World native/Wasm GLB lengths match.');
  const difference = actual.findIndex((value, index) => value !== native[index]);
  assert.equal(difference, -1, `World native/Wasm GLBs differ at byte ${difference}.`);
  assert.equal(actual.subarray(0, 4).toString(), 'glTF');
  assert.equal(actual.readUInt32LE(8), actual.length);
  const gltf = JSON.parse(actual.subarray(20, 20 + actual.readUInt32LE(12)).toString());
  for (const owner of ['item/saved', 'item/override']) {
    const nodes = gltf.nodes.filter(node => node.extras?.objectId === owner);
    assert.ok(nodes.length > 0 && nodes.every(node => node.extras.collidable === true));
    assert.ok(nodes.every(node => Number.isInteger(node.extras.source.offset) && node.extras.source.line >= 1));
  }
  const finish = owner => {
    const node = gltf.nodes.find(node => node.extras?.objectId === owner);
    const primitive = gltf.meshes[node.mesh].primitives[0];
    const material = gltf.materials[primitive.material];
    const image = gltf.images[gltf.textures[material.pbrMetallicRoughness.baseColorTexture.index].source];
    const view = gltf.bufferViews[image.bufferView];
    const start = 28 + actual.readUInt32LE(12) + (view.byteOffset ?? 0);
    return PNG.sync.read(actual.subarray(start, start + view.byteLength)).data;
  };
  assert.notDeepEqual(finish('item/saved'), finish('item/override'), 'Saved and explicit override finishes produce distinct baked color channels.');
  const slab = gltf.nodes.filter(node => node.extras?.objectId === 'slab');
  assert.ok(slab.length > 0 && slab.every(node => node.extras.collidable === false), 'Structural render meshes remain distinct from canonical collision meshes.');
  const metadata = gltf.extras.superspec;
  assert.equal(metadata.sourceAxes, 'right-handed-z-up');
  assert.equal(metadata.units, 'meters');
  assert.equal(metadata.geometryValidation, 'not-run');
  const owners = new Set(metadata.architecture.objects.map(object => object.id));
  for (const id of ['slab', 'trim', 'item/saved', 'item/override']) assert.ok(owners.has(id), `Source owner ${id} survives export.`);
  const evidence = { browser: 'Chromium', equal: true, bytes: actual.length,
    sha256: createHash('sha256').update(actual).digest('hex'), catalog: result.discovered, overriddenOnlyCatalog: result.overrideCatalog,
    collidableItemOwners: ['item/saved', 'item/override'], missingImageRecovery: true, acceptedPreviewPreserved: true };
  await writeFile(resolve(output, 'parity.json'), JSON.stringify(evidence, null, 2) + '\n');
  console.log(`World project native/Chromium-Wasm parity passed: ${evidence.bytes} identical GLB bytes, SHA-256 ${evidence.sha256}.`);
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
