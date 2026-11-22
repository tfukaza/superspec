/** Static hosting only: every AML compilation runs in the browser's Rust/Wasm worker. */
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, readdir, mkdir, writeFile, copyFile, mkdtemp, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { resolve, extname, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { launchBrowser, waitForFrames } from '../tools/browser.mjs';
import { selectiveBakes } from './browser/selective-bakes.mjs';
import { MAX_COMPANION_BYTES, MAX_COMPANIONS, MAX_FILE_BYTES } from '../browser/model/limits.ts';

const root = resolve('generated/rust-editor');
const output = resolve('generated/rust-architecture-acceptance');
await mkdir(output, { recursive: true });
const selectionRoot = await mkdtemp(resolve(tmpdir(), 'superspec-open-aml-'));
const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === 'win32' ? '.exe' : ''}`);
const sample = await readFile('tests/fixtures/aml/rust-stage1.aml', 'utf8');
/** Textareas expose LF line endings even when the selected AML file uses CRLF. */
function editorText(source) { return source.replace(/\r\n?/g, '\n'); }
const native = execFileSync(executable, ['scene', 'tests/fixtures/aml/rust-stage1.aml'], { env: { ...process.env, PATH: '' } });
const workerFile = (await readdir(`${root}/assets`)).find(file => /^worker-.*\.js$/.test(file));
assert.ok(workerFile, 'production worker asset exists');
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    const path = resolve(root, name === '/' ? 'index.html' : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    const bytes = await readFile(path);
    response.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css' }[extname(path)] ?? 'application/octet-stream');
    response.end(bytes);
  } catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  const consoleMessages = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() }));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const ready = async () => {
    try {
      await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'ready');
    } catch (error) {
      const state = await page.evaluate(() => ({
        status: document.querySelector('#status')?.outerHTML,
        diagnostics: document.querySelector('#diagnostics')?.textContent,
        revision: document.querySelector('#viewport')?.getAttribute('data-revision'),
        source: document.querySelector('#aml')?.value,
      }));
      await writeFile(`${output}/readiness-failure.json`, JSON.stringify({ state, errors, consoleMessages }, null, 2));
      await page.screenshot({ path: `${output}/readiness-failure.png` });
      throw error;
    }
  };
  const meshes = () => page.locator('#viewport').evaluate(element => JSON.parse(element.dataset.meshes));
  await ready();
  assert.equal(await page.evaluate(() => crossOriginIsolated), false, 'single worker works without shared-memory hosting');
  const initial = await meshes();
  assert.equal(initial.find(mesh => mesh.id === 'bench').bounds[1][0], 3);
  await waitForFrames(page);
  await page.screenshot({ path: `${output}/initial.png` });

  const wasmBytes = await page.evaluate(({ sample, workerFile }) => new Promise((resolve, reject) => {
    const worker = new Worker(`/assets/${workerFile}`, { type: 'module' });
    const timer = setTimeout(() => { worker.terminate(); reject(new Error('Wasm parity timeout')); }, 15000);
    worker.onerror = event => { clearTimeout(timer); worker.terminate(); reject(new Error(event.message)); };
    worker.onmessage = event => {
      clearTimeout(timer); worker.terminate();
      if (event.data.error) reject(new Error(event.data.error));
      else resolve(Array.from(new Uint8Array(event.data.buffer)));
    };
    worker.postMessage({ source: sample, revision: 0 });
  }), { sample, workerFile });
  assert.deepEqual(Buffer.from(wasmBytes), native, 'native and actual browser Wasm packets match byte for byte');

  for (const fixture of ['tests/fixtures/aml/rust-architecture.aml', 'tests/fixtures/aml/rust-floor-dressing.aml', 'tests/fixtures/aml/rust-materials.aml']) {
    const source = await readFile(fixture, 'utf8');
    const expected = execFileSync(executable, ['scene', fixture], { env: { ...process.env, PATH: '' }, maxBuffer: 64 * 1024 * 1024 });
    const assets = fixture.endsWith('rust-materials.aml') ? { 'asymmetric-heightmap.png': new Uint8Array(await readFile('tests/fixtures/aml/asymmetric-heightmap.png')) } : {};
    const bytes = await page.evaluate(({ source, workerFile, assets }) => new Promise((resolve, reject) => {
      const worker = new Worker(`/assets/${workerFile}`, { type: 'module' });
      const timer = setTimeout(() => { worker.terminate(); reject(new Error('Architectural Wasm timeout')); }, 60000);
      worker.onerror = event => { clearTimeout(timer); worker.terminate(); reject(new Error(event.message)); };
      worker.onmessage = event => {
        clearTimeout(timer); worker.terminate();
        if (event.data.error) reject(new Error(event.data.error));
        else resolve(Array.from(new Uint8Array(event.data.buffer)));
      };
      worker.postMessage({ source, revision: 0, assets });
    }), { source, workerFile, assets });
    if (!Buffer.from(bytes).equals(expected)) {
      await writeFile(`${output}/parity-native.lsp`, expected);
      await writeFile(`${output}/parity-wasm.lsp`, Buffer.from(bytes));
      throw new Error(`Native/Wasm packet difference: ${fixture}; retained both packets for diagnosis.`);
    }
    await page.locator('#aml').fill(source);
    if (fixture.endsWith('rust-materials.aml')) await page.locator('#assets').setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await ready();
    await waitForFrames(page);
    await page.screenshot({ path: `${output}/${fixture.split('/').at(-1)}.png` });
  }

  // Control only the browser File read boundary; all imports still pass through the real editor.
  await page.evaluate(() => {
    const original = File.prototype.arrayBuffer;
    window.fileReads = { count: 0, completed: 0, hold: false, release: undefined, rejectName: undefined };
    File.prototype.arrayBuffer = async function() {
      window.fileReads.count++;
      if (this.name === window.fileReads.rejectName) throw new DOMException('Injected unreadable file', 'NotReadableError');
      if (window.fileReads.hold) {
        window.fileReads.hold = false;
        await new Promise(resolve => { window.fileReads.release = resolve; });
      }
      const bytes = await original.call(this);
      window.fileReads.completed++;
      return bytes;
    };
    // A file may report a size of its own (`fileReads.sizes`, by name), so the limits are tested without allocating them.
    const size = Object.getOwnPropertyDescriptor(Blob.prototype, 'size').get;
    window.fileReads.sizes = {};
    Object.defineProperty(File.prototype, 'size', { configurable: true, get() { return window.fileReads.sizes[this.name] ?? size.call(this); } });
    window.restoreFileReads = () => { File.prototype.arrayBuffer = original; delete File.prototype.size; };
  });
  try {
    const originalMaterials = await readFile('tests/fixtures/aml/rust-materials.aml', 'utf8');
    const folderSource = originalMaterials.replace('id="materials"', 'id="relocated"').replace('asymmetric-heightmap.png', 'textures/asymmetric-heightmap.png');
    const folderInput = page.locator('#open-folder');
    const fileInput = page.locator('#open-aml');
    const nestedImage = 'textures/asymmetric-heightmap.png';
    const imageSource = resolve('tests/fixtures/aml/asymmetric-heightmap.png');
    const paired = resolve(selectionRoot, 'paired.aml');
    await writeFile(paired, originalMaterials);
    await fileInput.setInputFiles([paired, imageSource]);
    await ready();
    assert.equal(await page.locator('#aml').inputValue(), editorText(originalMaterials));
    assert.equal((await meshes()).find(mesh => mesh.id === 'surface').bounds[1][0], 4);
    assert.equal(await fileInput.inputValue(), '');
    for (const name of ['relocated scene 窗', 'missing', 'older']) {
      await mkdir(resolve(selectionRoot, name, 'textures'), { recursive: true });
      await writeFile(resolve(selectionRoot, name, 'scene.aml'), folderSource);
    }
    const relocated = resolve(selectionRoot, 'relocated scene 窗');
    await copyFile(imageSource, resolve(relocated, nestedImage));
    await writeFile(resolve(relocated, 'night.aml'), sample);
    await folderInput.setInputFiles(relocated);
    await ready();
    assert.equal(await page.locator('#aml').inputValue(), editorText(folderSource), 'scene.aml is preferred over an alternate entry');
    assert.equal((await meshes()).find(mesh => mesh.id === 'surface').bounds[1][0], 4);
    assert.equal(await folderInput.inputValue(), '');

    /** Failed opens retain the editable source, uploaded model and camera together. */
    const retained = async () => ({
      source: await page.locator('#aml').inputValue(), meshes: await meshes(),
      revision: await page.locator('#viewport').getAttribute('data-revision'),
      camera: await page.locator('#viewport').getAttribute('data-camera'),
    });
    const stable = await retained();
    const missing = resolve(selectionRoot, 'missing');
    const correctedSource = folderSource.replace('width="4"', 'width="5"');
    await writeFile(resolve(missing, 'scene.aml'), correctedSource);
    await folderInput.setInputFiles(missing);
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /textures\/asymmetric-heightmap\.png/);
    assert.deepEqual(await retained(), stable);
    await copyFile(imageSource, resolve(missing, nestedImage));
    await folderInput.setInputFiles(missing);
    await ready();
    assert.equal(await page.locator('#aml').inputValue(), editorText(correctedSource));
    assert.equal((await meshes()).find(mesh => mesh.id === 'surface').bounds[1][0], 5);
    const recovered = await retained();

    const malformed = resolve(selectionRoot, 'malformed.aml');
    await writeFile(malformed, '<architecture');
    await fileInput.setInputFiles(malformed);
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /XML_SYNTAX/);
    assert.deepEqual(await retained(), recovered);
    const ambiguous = resolve(selectionRoot, 'ambiguous');
    await mkdir(ambiguous);
    for (const name of ['first.aml', 'second.aml']) await writeFile(resolve(ambiguous, name), sample);
    await folderInput.setInputFiles(ambiguous);
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /one scene\.aml|exactly one AML/);
    assert.deepEqual(await retained(), recovered);
    await page.locator('#restart').click();
    await ready();
    assert.equal((await meshes()).find(mesh => mesh.id === 'surface').bounds[1][0], 5, 'failed opens retain usable companions');

    const newerSource = sample.replace('width="2" depth="0.65"', 'width="1.75" depth="0.65"');
    const standalone = resolve(selectionRoot, 'standalone.aml');
    await writeFile(standalone, newerSource);
    const older = resolve(selectionRoot, 'older');
    await copyFile(imageSource, resolve(older, nestedImage));
    for (const replacement of ['open', 'edit']) {
      await page.evaluate(() => { window.fileReads.hold = true; window.fileReads.completed = 0; });
      await folderInput.setInputFiles(older);
      await page.waitForFunction(() => window.fileReads.release);
      const expected = replacement === 'open' ? newerSource : sample;
      if (replacement === 'open') await fileInput.setInputFiles(standalone);
      else await page.locator('#aml').fill(expected);
      await ready();
      const latest = await retained();
      assert.equal(latest.source, editorText(expected));
      await page.evaluate(() => { window.fileReads.release(); window.fileReads.release = undefined; });
      await page.waitForFunction(() => window.fileReads.completed === 1 && document.querySelector('#status').dataset.state === 'ready');
      assert.deepEqual(await retained(), latest, `older folder reads cannot replace a newer ${replacement}`);
    }

    await fileInput.setInputFiles([paired, imageSource]);
    await ready();
    const assetInput = page.locator('#assets');
    await page.locator('#clear-assets').click();
    assert.equal(await assetInput.inputValue(), '');
    await assetInput.setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await ready();
    await page.evaluate(() => { window.fileReads.hold = true; window.fileReads.count = 0; });
    await assetInput.setInputFiles([
      { name: 'asymmetric-heightmap.png', mimeType: 'image/png', buffer: Buffer.from('obsolete invalid image') },
      { name: 'should-not-be-read.png', mimeType: 'image/png', buffer: Buffer.from('unused') },
    ]);
    await page.waitForFunction(() => window.fileReads.release);
    await assetInput.setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await ready();
    await page.evaluate(() => { window.fileReads.release(); window.fileReads.release = undefined; });
    await page.locator('#restart').click();
    await ready();
    assert.equal(await page.evaluate(() => window.fileReads.count), 2, 'superseded selection stops before reading its next file');

    await page.evaluate(() => { window.fileReads.count = 0; window.fileReads.rejectName = 'unreadable.png'; });
    await assetInput.setInputFiles([
      { name: 'asymmetric-heightmap.png', mimeType: 'image/png', buffer: Buffer.from('invalid replacement') },
      { name: 'unreadable.png', mimeType: 'image/png', buffer: Buffer.from('unreadable') },
    ]);
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.equal(await page.evaluate(() => window.fileReads.count), 2, 'failure occurs after the first replacement was read');
    await page.locator('#restart').click();
    await ready();
    await page.evaluate(() => { window.fileReads.rejectName = undefined; });
    await assetInput.setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await ready();

    // A late invalid member must not partially replace the prior valid same-name asset.
    // (The limits are the editor's own, `browser/model/limits.ts`; the files only report the sizes.)
    await page.evaluate(limit => {
      window.fileReads.count = 0;
      window.fileReads.sizes = { 'too-large.png': limit + 1 };
      const files = new DataTransfer();
      files.items.add(new File(['invalid replacement'], 'asymmetric-heightmap.png'));
      files.items.add(new File(['x'], 'too-large.png'));
      const input = document.querySelector('#assets');
      input.files = files.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    }, MAX_FILE_BYTES);
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /too-large\.png exceeds/);
    assert.equal(await page.evaluate(() => window.fileReads.count), 0, 'size preflight performs no reads');
    await page.locator('#restart').click();
    await ready();
    await page.evaluate(({ file, total }) => {
      window.fileReads.count = 0;
      // Each within the file limit, together above the total.
      const count = Math.floor(total / file) + 1, each = Math.floor(total / count) + 1;
      window.fileReads.sizes = Object.fromEntries(Array.from({ length: count }, (_, i) => [`large-${i}.png`, each]));
      const files = new DataTransfer();
      for (let i = 0; i < count; i++) files.items.add(new File(['x'], `large-${i}.png`));
      const input = document.querySelector('#assets');
      input.files = files.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    }, { file: MAX_FILE_BYTES, total: MAX_COMPANION_BYTES });
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /Assets exceed/);
    assert.equal(await page.evaluate(() => window.fileReads.count), 0, 'aggregate preflight performs no reads');
    await page.evaluate(() => { window.fileReads.sizes = {}; });
    await page.locator('#restart').click();
    await ready();
    await assetInput.setInputFiles(Array.from({ length: MAX_COMPANIONS + 1 }, (_, i) => ({ name: `small-${i}.png`, mimeType: 'image/png', buffer: Buffer.from('x') })));
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /Assets exceed/);
    assert.equal(await page.evaluate(() => window.fileReads.count), 0, 'count preflight performs no reads');
    await page.locator('#restart').click();
    await ready();

    await page.evaluate(() => { window.fileReads.hold = true; });
    await assetInput.setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await page.waitForFunction(() => window.fileReads.release);
    await page.locator('#clear-assets').click();
    await page.evaluate(() => { window.fileReads.release(); window.fileReads.release = undefined; });
    await page.waitForFunction(() => document.querySelector('#status').dataset.state === 'error');
    assert.match(await page.locator('#diagnostics').textContent(), /asset|image|companion/i);
    assert.equal(await assetInput.inputValue(), '');
    await assetInput.setInputFiles('tests/fixtures/aml/asymmetric-heightmap.png');
    await ready();
  } finally {
    await page.evaluate(() => { window.fileReads.release?.(); window.restoreFileReads(); });
  }

  const edited = sample.replace('width="2" depth="0.65"', 'width="3.5" depth="0.65"');
  await page.locator('#aml').fill(edited);
  await ready();
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 4.5);
  const validRevision = await page.locator('#viewport').getAttribute('data-revision');
  const validMeshes = await meshes();
  const badTag = '<unrecognized/>';
  const badDraft = editorText(edited).replace('<box', '<!-- 🌿 -->' + badTag + '<box');
  await page.locator('#aml').fill(badDraft);
  await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'error');
  assert.equal(await page.locator('#viewport').getAttribute('data-revision'), validRevision);
  assert.deepEqual(await meshes(), validMeshes);
  const finding = page.locator('#diagnostics button').first();
  assert.match(await finding.textContent(), /AML_CHILD/);
  await finding.click();
  assert.deepEqual(await page.locator('#aml').evaluate(e => [e.selectionStart, e.selectionEnd]),
    [badDraft.indexOf(badTag), badDraft.indexOf(badTag) + badTag.length]);
  assert.match(await page.locator('#status').textContent(), /last valid/);
  assert.equal(await page.locator('#download-glb').isDisabled(), true);
  await page.screenshot({ path: `${output}/invalid-retains-scene.png` });
  await page.locator('#aml').fill(edited);
  await ready();

  // Hold initialization at the network boundary to prove cancellation without relying on CPU timing.
  const heavy = `<architecture version="1" id="many">${Array.from({ length: 9000 }, (_, i) => `<box id="b${i}"/>`).join('')}</architecture>`;
  let releaseWasm;
  const wasmGate = new Promise(resolve => { releaseWasm = resolve; });
  await page.route('**/*.wasm', async route => {
    await wasmGate;
    // The obsolete worker's fetch may already have disappeared when the gate opens.
    try { await route.continue(); } catch (error) {
      if (!/closed|cancel|handled|Invalid InterceptionId/i.test(error.message)) throw error;
    }
  });
  try {
    await page.locator('#aml').fill(heavy);
    const heavyWorker = page.waitForEvent('worker');
    const wasmRequest = page.waitForRequest('**/*.wasm');
    await page.locator('#restart').click();
    const obsoleteWorker = await heavyWorker;
    await wasmRequest;
    const cancelled = obsoleteWorker.waitForEvent('close');
    await page.locator('#aml').fill(sample.replace('width="2" depth="0.65"', 'width="1.25" depth="0.65"'));
    await cancelled;
    releaseWasm();
    await ready();
  } finally {
    releaseWasm();
    await page.unrouteAll({ behavior: 'wait' });
  }
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 2.25);
  await page.locator('#restart').click();
  await ready();
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 2.25);

  // A failed module fetch must not poison all subsequent edits in the same editor.
  await page.route('**/*.wasm', route => route.abort('failed'));
  await page.locator('#restart').click();
  await page.waitForFunction(() => document.querySelector('#status')?.dataset.state === 'error');
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 2.25);
  await page.unrouteAll({ behavior: 'wait' });
  await page.locator('#aml').fill(edited);
  await ready();
  assert.equal((await meshes()).find(mesh => mesh.id === 'bench').bounds[1][0], 4.5);
  for (let i = 0; i < 8; i++) { await page.locator('#aml').fill(i % 2 ? sample : edited); await ready(); }
  await page.screenshot({ path: `${output}/recovered.png` });
  const downloadEvent = page.waitForEvent('download');
  await page.locator('#download').click();
  const download = await downloadEvent;
  assert.equal(await readFile(await download.path(), 'utf8'), editorText(sample));

  // Export initialization failure leaves the valid preview intact and permits a later download.
  const beforeExportRevision=await page.locator('#viewport').getAttribute('data-revision');
  const beforeExportMeshes=await meshes();
  await page.route('**/*.wasm',route=>route.abort('failed'));
  await page.locator('#download-glb').click();
  await page.waitForFunction(()=>document.querySelector('#export-status').textContent.startsWith('Export failed:'));
  assert.equal(await page.locator('#download-glb').isEnabled(),true);
  assert.equal(await page.locator('#status').getAttribute('data-state'),'ready');
  assert.equal(await page.locator('#viewport').getAttribute('data-revision'),beforeExportRevision);
  assert.deepEqual(await meshes(),beforeExportMeshes);
  await page.unrouteAll({behavior:'wait'});

  // Hold the next export at a real fetch so an edit can cancel it before any download.
  let releaseExport;
  const exportGate=new Promise(resolve=>{releaseExport=resolve;});
  let exportDownloads=0;page.on('download',()=>exportDownloads++);
  await page.route('**/*.wasm',async route=>{
    await exportGate;
    try {await route.continue();}catch(error){if(!/closed|cancel|handled|Invalid InterceptionId/i.test(error.message))throw error;}
  });
  try {
    const workerEvent=page.waitForEvent('worker');const requestEvent=page.waitForRequest('**/*.wasm');
    await page.locator('#download-glb').click();
    const worker=await workerEvent;await requestEvent;
    const close=worker.waitForEvent('close');
    await page.locator('#aml').fill(edited);await close;
    releaseExport();await ready();
    assert.equal(exportDownloads,0,'cancelled export must not download an obsolete revision');
  }finally{releaseExport();await page.unrouteAll({behavior:'wait'});}
  const exported=page.waitForEvent('download');await page.locator('#download-glb').click();
  const glbDownload=await exported;assert.equal(glbDownload.suggestedFilename(),'level.glb');
  const exportedSource=`${output}/export-edited.aml`;await writeFile(exportedSource,editorText(edited));
  const expectedGlb=execFileSync(executable,['glb',exportedSource],{env:{...process.env,PATH:''}});
  assert.deepEqual(await readFile(await glbDownload.path()),expectedGlb,'download must contain the newest edited scene compiled entirely by Rust');
  assert.equal(exportDownloads,1);
  await page.waitForFunction(()=>document.querySelector('#export-status').textContent==='GLB downloaded.');

  // A complete valid 128 MiB snapshot can replace a previous one even when the first asset grows.
  assert.deepEqual(await page.evaluate(async({sample,workerFile})=>{
    const worker=new Worker(`/assets/${workerFile}`,{type:'module'});
    try {
      const large=new Uint8Array(16*1024*1024),small=new Uint8Array(1024*1024),medium=new Uint8Array(15*1024*1024);
      const assets={A:small,B:large,C:medium};
      for(let i=0;i<6;i++)assets[`unused-${i}`]=large;
      const run=revision=>new Promise((resolve,reject)=>{
        const timeout=setTimeout(()=>reject(new Error('Asset snapshot worker timed out.')),30000);
        worker.onerror=event=>{clearTimeout(timeout);reject(new Error(event.message));};
        worker.onmessage=event=>{
          clearTimeout(timeout);
          if(event.data.error){reject(new Error(event.data.error));return;}
          const view=new DataView(event.data.buffer);
          resolve(JSON.parse(new TextDecoder().decode(new Uint8Array(event.data.buffer,8,view.getUint32(4,true)))).success);
        };
        worker.postMessage({source:sample,revision,assets,format:'scene'});
      });
      const first=await run(1);assets.A=large;assets.B=small;
      return [first,await run(2)];
    }finally{worker.terminate();}
  },{sample,workerFile}),[true,true]);
  const imageBytes=Array.from(await readFile('tests/fixtures/aml/asymmetric-heightmap.png'));
  const cacheRuns=await page.evaluate(async({workerFile,imageBytes,companions})=>{
    let worker=new Worker(`/assets/${workerFile}`,{type:'module'});
    let revision=0;
    const source='<architecture version="1" id="cache"><material id="m" texture="image.png"/><box id="box" material="m" width="2" depth="1" height="1"/></architecture>';
    const run=(source,assets)=>new Promise((resolve,reject)=>{
      const timeout=setTimeout(()=>reject(new Error('Cache worker timed out.')),30000);
      worker.onerror=event=>{clearTimeout(timeout);reject(new Error(event.message));};
      worker.onmessage=event=>{
        clearTimeout(timeout);
        if(event.data.error){reject(new Error(event.data.error));return;}
        const view=new DataView(event.data.buffer),size=view.getUint32(4,true);
        const manifest=JSON.parse(new TextDecoder().decode(new Uint8Array(event.data.buffer,8,size)));
        resolve({success:manifest.success,stats:event.data.cacheStats,bounds:manifest.meshes.find(m=>m.visible)?.bounds});
      };
      worker.postMessage({source,revision:++revision,assets});
    });
    try {
      const bytes=new Uint8Array(imageBytes),assets={'image.png':bytes};
      const first=await run(source,assets);
      const edit=await run(source.replace('width="2"','width="3"'),assets);
      const missing=await run(source,{});
      const corrupt=bytes.slice();corrupt[20]^=1;
      const invalid=await run(source,{'image.png':corrupt});
      let partialError;
      // One companion more than the budget allows (its byte limits are hundreds of megabytes; the count is cheap to exceed).
      const excess=Object.fromEntries(Array.from({length:companions},(_,i)=>[`extra-${i}.bin`,new Uint8Array(1)]));
      try {await run(source,{'image.png':bytes,...excess});}catch(error){partialError=String(error);}
      const afterPartial=await run(source,{});
      const recovery=await run(source,assets);
      worker.terminate();worker=new Worker(`/assets/${workerFile}`,{type:'module'});
      const restart=await run(source,assets);
      return {first,edit,missing,invalid,partialError,afterPartial,recovery,restart};
    }finally{worker.terminate();}
  },{workerFile,imageBytes,companions:MAX_COMPANIONS});
  assert.equal(cacheRuns.first.stats.misses,1);assert.equal(cacheRuns.first.stats.hits,0);
  assert.equal(cacheRuns.edit.stats.misses,1);assert.equal(cacheRuns.edit.stats.hits,1);
  assert.equal(cacheRuns.edit.bounds[1][0],3);assert.equal(cacheRuns.first.bounds[1][0],2);
  assert.equal(cacheRuns.missing.success,false);assert.equal(cacheRuns.invalid.success,false);
  assert.match(cacheRuns.partialError,new RegExp(`More than ${MAX_COMPANIONS} files were supplied`));assert.equal(cacheRuns.afterPartial.success,false);
  assert.equal(cacheRuns.recovery.success,true);assert.equal(cacheRuns.recovery.stats.hits,2);
  assert.equal(cacheRuns.restart.success,true);assert.equal(cacheRuns.restart.stats.hits,0);assert.equal(cacheRuns.restart.stats.misses,1);
  await selectiveBakes(page, workerFile, executable, output);
  assert.deepEqual(errors, []);
  await writeFile(`${output}/result.json`, JSON.stringify({ nativeWasmBytesEqual: true, packetBytes: native.length, initialMeshes: initial, openRelocatedFolder: true, openSelectionAtomicity: true, openSelectionRecovery: true, openSelectionSupersession: true, dimensionEdit: true, malformedRetention: true, latestRevision: true, assetImportPreflight: true, assetImportSupersession: true, assetImportAtomicity: true, clearAndReimport: true, initializationCancellation: true, workerRestart: true, failedInitializationRecovery: true, repeatedEdits: 8, explicitDownload: true, glbDownloadNativeParity: true, obsoleteExportCancelled: true, failedExportRetention: true, fullAssetSnapshotReplacement: true }, null, 2));
  console.log(`Rust/Wasm editor acceptance passed: ${output}`);
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
  await rm(selectionRoot, { recursive: true, force: true });
}
