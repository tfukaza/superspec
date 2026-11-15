/** Executes native-shared golden kernel assertions in actual browser Wasm, with no production test API. */
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, sep, extname } from 'node:path';
import { launchBrowser } from '../tools/browser.mjs';

const root = resolve('generated/wasm-tests');
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    if (name === '/') { response.setHeader('Content-Type', 'text/html'); response.end('<!doctype html><title>Rust kernel acceptance</title>'); return; }
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
  page.on('console', message => {
    if (message.type() === 'error') console.error(`[kernel] ${message.text()}`);
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const passed = await page.evaluate(async () => {
    const kernel = await import('/superspec_kernel_acceptance.js');
    await kernel.default({ module_or_path: '/superspec_kernel_acceptance_bg.wasm' });
    for (const name of ['check_material_features', 'check_surface_kernels', 'check_procedural_meshes', 'check_metal_recipes', 'check_wall_recipes']) {
      try {
        if (!kernel[name]()) return false;
      } catch (cause) {
        throw new Error(`${name}: ${cause.message}`, { cause });
      }
    }
    return true;
  });
  assert.equal(passed, true);
  await writeFile(`${root}/result.json`, JSON.stringify({ browser: 'Chromium', receiverFeatureIndices: true, nativeSharedRasterGoldens: true, srgbTransitions: true, proceduralMeshesAndSurveys: true, intrinsicMetalPixels: true, wallConditionAndPlaster: true }));
  console.log('Rust receiver features, raster, procedural mesh/survey, metal and wall checks pass in Chromium/Wasm.');
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
