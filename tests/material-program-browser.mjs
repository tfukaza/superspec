/** Runs the same bounded material fixtures natively and in actual Chromium/Wasm. */
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, resolve, sep } from 'node:path';
import { promisify } from 'node:util';
import { launchBrowser } from '../tools/browser.mjs';

const verification = resolve('generated/verification');
await mkdir(verification, { recursive: true });

/** Each fixture is one native test that writes its bytes, and the Wasm export that must reproduce them. */
const fixtures = [
  { name: 'ms programs', test: 'material_program', exact: 'portable_program_fixture_covers_the_same_native_and_wasm_evaluator',
    variable: 'MATERIAL_PROGRAM_PARITY_OUTPUT', native: 'material-program-native.json', wasm: 'material-program-wasm.json',
    export: 'material_program_result', covers: ['chipped concrete', 'painted metal', 'supplied image sampling and HDR emission'] },
  { name: 'typescript recipes', test: 'material_ts', exact: 'portable_typescript_fixture_covers_the_same_native_and_wasm_front_end',
    variable: 'MATERIAL_TS_PARITY_OUTPUT', native: 'material-ts-native.json', wasm: 'material-ts-wasm.json',
    export: 'material_ts_result', covers: ['chipped concrete', 'painted metal', 'tiled courtyard groups, layers and live operations', 'inspection and structured edits'] },
  { name: 'kowloon ports', test: 'material_primitives', exact: 'portable_kowloon_fixture_covers_the_same_native_and_wasm_ports',
    variable: 'MATERIAL_KOWLOON_PARITY_OUTPUT', native: 'material-kowloon-native.json', wasm: 'material-kowloon-wasm.json',
    export: 'material_kowloon_result', covers: ['Kowloon concrete wall, slab and plaster', 'Kowloon rusted steel on crate creases', 'varied sampling, value noise and receiver queries'] },
];
// Set REUSE_NATIVE_FIXTURES=1 to compare against native outputs already written by the same tests elsewhere.
const reuse = process.env.REUSE_NATIVE_FIXTURES === '1';
for (const fixture of fixtures) {
  fixture.nativePath = resolve(verification, fixture.native);
  if (!reuse) {
    await promisify(execFile)(process.env.CARGO ?? 'cargo', [
      'test', '-p', 'superspec-engine', '--test', fixture.test, fixture.exact, '--locked', '-j4', '--', '--exact',
    ], { env: { ...process.env, [fixture.variable]: fixture.nativePath }, maxBuffer: 2 * 1024 * 1024 });
  }
  fixture.bytes = await readFile(fixture.nativePath);
}
const root = resolve('generated/wasm-tests');
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    if (name === '/') {
      response.setHeader('Content-Type', 'text/html');
      response.end('<!doctype html><title>Material program parity</title>');
      return;
    }
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
    if (message.type() === 'error') console.error(`[material kernel] ${message.text()}`);
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const results = [];
  for (const fixture of fixtures) {
    const actual = Buffer.from(await page.evaluate(async name => {
      const kernel = await import('/superspec_kernel_acceptance.js');
      await kernel.default({ module_or_path: '/superspec_kernel_acceptance_bg.wasm' });
      return Array.from(kernel[name]());
    }, fixture.export));
    await writeFile(resolve(verification, fixture.wasm), actual);
    assert.equal(actual.length, fixture.bytes.length, `Native and Wasm ${fixture.name} results have different lengths.`);
    const difference = actual.findIndex((value, index) => value !== fixture.bytes[index]);
    assert.equal(difference, -1, `Native and Wasm ${fixture.name} results differ at byte ${difference}.`);
    results.push({ fixture: fixture.name, browser: 'Chromium', equal: true, bytes: actual.length,
      sha256: createHash('sha256').update(actual).digest('hex'), fixtures: fixture.covers });
  }
  await writeFile(resolve(verification, 'material-program-parity.json'), JSON.stringify(results, null, 2) + '\n');
  for (const result of results) console.log(`Material ${result.fixture} native/Chromium-Wasm parity passed: ${result.bytes} identical bytes, SHA-256 ${result.sha256}.`);
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
