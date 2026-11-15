/** The browser drives a real Rust service; Node is only the acceptance driver. */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { launchBrowser, waitForFrames } from '../tools/browser.mjs';

const output = resolve('generated/rust-preview-browser');
await mkdir(output, { recursive: true });
const source = resolve(output, 'level.aml');
const original = await readFile('tests/fixtures/aml/rust-stage1.aml', 'utf8');
await writeFile(source, original);
const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === 'win32' ? '.exe' : ''}`);
let stdout = '';
let browser;
let serverError = '';
/** Launches a native server with no executable search path, including after a same-port restart. */
function launch(port = '0') {
  serverError = '';
  const child = spawn(executable, ['preview', source, '--ui', resolve('generated/rust-editor'), '--port', port], { cwd: output, env: { ...process.env, PATH: '', NODE_PATH: '', NODE_OPTIONS: '' }, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', bytes => { stdout += bytes; });
  child.stderr.on('data', bytes => { serverError += bytes; });
  return child;
}
let server = launch();
/** Bounded observable readiness also detects an exited native process. */
async function nativeReady(predicate) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    if (server.exitCode !== null) throw new Error(`Rust service exited: ${serverError}`);
    const value = await predicate();
    if (value) return value;
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  throw new Error(`Rust preview readiness timed out: ${serverError}`);
}
try {
  const url = await nativeReady(() => /Preview listening at (http:\/\/127\.0\.0\.1:\d+\/)/.exec(serverError)?.[1]);
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.goto(url);
  const ready = () => page.waitForFunction(() => document.querySelector('#status').dataset.state === 'ready');
  const meshes = () => page.locator('#viewport').evaluate(element => JSON.parse(element.dataset.meshes));
  await ready();
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 3);
  assert.equal(await page.locator('textarea').count(), 0, 'file preview offers no source mutation');
  const edit = width => original.replace('width="2" depth="0.65"', `width="${width}" depth="0.65"`);
  const firstSession = await page.locator('#viewport').getAttribute('data-session');
  assert.equal(await page.locator('#viewport').getAttribute('data-revision'), '1');
  server.kill(); await new Promise(resolve => server.once('exit', resolve));
  await writeFile(source, edit(3));
  server = launch(new URL(url).port);
  await nativeReady(() => serverError.includes('Preview listening at'));
  await page.waitForFunction(session => document.querySelector('#viewport').dataset.session !== session, firstSession);
  await ready();
  assert.equal(await page.locator('#viewport').getAttribute('data-revision'), '1');
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 4);
  assert.equal((await fetch(`${url}api/scene?session=${firstSession}&revision=1`)).status, 409);
  await writeFile(source, edit(3.5));
  await page.waitForFunction(() => JSON.parse(document.querySelector('#viewport').dataset.meshes).find(mesh => mesh.id === 'bench').bounds[1][0] === 4.5);
  await ready();
  const retainedRevision = await page.locator('#viewport').getAttribute('data-revision');
  const retainedMeshes = await meshes();
  await writeFile(source, '<architecture');
  await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
  assert.match(await page.locator('#diagnostics').textContent(), /\d+:\d+ XML_SYNTAX/);
  assert.match(await page.locator('#status').textContent(), /last valid/);
  assert.equal(await page.locator('#viewport').getAttribute('data-revision'), retainedRevision);
  assert.deepEqual(await meshes(), retainedMeshes);
  await page.screenshot({ path: `${output}/invalid-retains-scene.png` });

  // Retain an actual older HTTP response while a newer status and packet finish independently.
  let releaseOld;
  const gate = new Promise(resolve => { releaseOld = resolve; });
  let receivedOld;
  const oldReceived = new Promise(resolve => { receivedOld = resolve; });
  let gated = false;
  let gateTimeout;
  await page.route('**/api/scene?session=*&revision=*', async route => {
    if (gated) { await route.continue(); return; }
    gated = true;
    const response = await route.fetch();
    const bytes = await response.body();
    receivedOld();
    await gate;
    try { await route.fulfill({ response, body: bytes }); }
    catch (error) { if (!/closed|cancel|handled|Invalid InterceptionId/i.test(error.message)) throw error; }
  });
  try {
    await writeFile(source, edit(4));
    try { await Promise.race([oldReceived, new Promise((_, reject) => { gateTimeout = setTimeout(() => reject(new Error('Older packet was not requested.')), 15000); })]); }
    finally { clearTimeout(gateTimeout); }
    await writeFile(source, edit(5));
    await page.waitForFunction(() => JSON.parse(document.querySelector('#viewport').dataset.meshes).find(mesh => mesh.id === 'bench').bounds[1][0] === 6);
    await ready();
    const newest = await page.locator('#viewport').getAttribute('data-revision');
    releaseOld(); await page.unrouteAll({ behavior: 'wait' }); await waitForFrames(page);
    assert.equal(await page.locator('#viewport').getAttribute('data-revision'), newest);
    assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 6);
  } finally { releaseOld(); await page.unrouteAll({ behavior: 'wait' }); }

  await page.route('**/api/status', route => route.abort('failed'));
  await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'disconnected');
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 6);
  await writeFile(source, edit(1.25));
  await page.unrouteAll({ behavior: 'wait' });
  await page.waitForFunction(() => JSON.parse(document.querySelector('#viewport').dataset.meshes).find(mesh => mesh.id === 'bench').bounds[1][0] === 2.25);
  await ready(); await waitForFrames(page);
  // A ready compiler cannot erase a persistent scene-transfer failure on later status polls.
  let releaseRetry;
  const retryGate = new Promise(resolve => { releaseRetry = resolve; });
  let receivedRetry;
  const retryReceived = new Promise(resolve => { receivedRetry = resolve; });
  let retryTimeout;
  let firstDelivery = true;
  await page.route('**/api/scene?session=*&revision=*', async route => {
    if (firstDelivery) { firstDelivery = false; await route.fulfill({ status: 503, body: 'delivery unavailable' }); return; }
    receivedRetry(); await retryGate; await route.continue();
  });
  try {
    await writeFile(source, edit(2.5));
    await page.waitForFunction(() => document.querySelector('#status').textContent.startsWith('Preview delivery failed.'));
    // The retry proves a later successful status was processed; hold its packet so it cannot mask erasure.
    try { await Promise.race([retryReceived, new Promise((_, reject) => { retryTimeout = setTimeout(() => reject(new Error('Delivery retry was not requested.')), 5000); })]); }
    finally { clearTimeout(retryTimeout); }
    assert.match(await page.locator('#diagnostics').textContent(), /503/);
    assert.equal(await page.locator('#status').getAttribute('data-state'), 'error');
    assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 2.25);
  } finally { releaseRetry(); await page.unrouteAll({ behavior: 'wait' }); }
  await page.waitForFunction(() => JSON.parse(document.querySelector('#viewport').dataset.meshes).find(mesh => mesh.id === 'bench').bounds[1][0] === 3.5);
  await ready();
  await page.screenshot({ path: `${output}/recovered.png` });
  assert.equal(await readFile(source, 'utf8'), edit(2.5));
  assert.equal(stdout, '', 'native server reserves stdout and logs its URL on stderr');
  assert.deepEqual(errors, []);
  await writeFile(`${output}/result.json`, JSON.stringify({ nativeOnlyRuntime: true, realThreeRendering: true, sourceEdits: true, invalidRetention: true, obsoletePacketSuppression: true, connectionRecovery: true, processRestartIdentity: true, deliveryErrorRetention: true, readOnlySource: true }, null, 2));
  console.log(`Native Rust file preview acceptance passed: ${output}`);
} finally {
  await browser?.close();
  server.kill();
  if (server.exitCode === null) await new Promise(resolve => server.once('exit', resolve));
}
