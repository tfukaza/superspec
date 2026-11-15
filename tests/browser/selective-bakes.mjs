import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

/** Exercises session reuse in the shipped browser worker and compares full native packets. */
export async function selectiveBakes(page, workerFile, executable, output) {
  const source = await readFile('tests/fixtures/aml/rust-selective-bakes.aml', 'utf8');
  const image = await readFile('tests/fixtures/aml/asymmetric-heightmap.png');
  const unrelated = source.replace('#63869f', '#b05d72');
  const dependent = unrelated.replace('repeat="1.7"', 'repeat="2.3"');
  const sources = [source, source, unrelated, dependent];
  const directory = resolve(output, 'selective-bakes');
  await mkdir(directory, { recursive: true });
  await writeFile(resolve(directory, 'asymmetric-heightmap.png'), image);
  const nativeHashes = [];
  for (const [index, text] of sources.entries()) {
    const path = resolve(directory, `${index}.aml`);
    await writeFile(path, text);
    const packet = execFileSync(executable, ['scene', path], {
      env: { ...process.env, PATH: '' }, maxBuffer: 64 * 1024 * 1024,
    });
    nativeHashes.push(createHash('sha256').update(packet).digest('hex'));
  }

  const runs = await page.evaluate(async ({ sources, workerFile, image }) => {
    const hash = async bytes => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)))
      .map(value => value.toString(16).padStart(2, '0')).join('');
    const run = (worker, source) => new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Selective bake worker timed out.')), 60000);
      worker.onerror = event => { clearTimeout(timer); reject(new Error(event.message)); };
      worker.onmessage = async event => {
        clearTimeout(timer);
        try {
          if (event.data.error) throw new Error(event.data.error);
          const { buffer, bakeStats } = event.data;
          const length = new DataView(buffer).getUint32(4, true);
          const manifest = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 8, length)));
          if (!manifest.success) throw new Error(JSON.stringify(manifest.diagnostics));
          resolve({
            packetHash: await hash(buffer),
            assetHash: await hash(buffer.slice(buffer.byteLength - manifest.assetByteCount)),
            stats: bakeStats,
          });
        } catch (error) { reject(error); }
      };
      worker.postMessage({ source, revision: 0, assets: { 'asymmetric-heightmap.png': new Uint8Array(image) } });
    });
    const shared = new Worker(`/assets/${workerFile}`, { type: 'module' });
    try {
      const runs = [];
      for (const source of sources) {
        const incremental = await run(shared, source);
        const fresh = new Worker(`/assets/${workerFile}`, { type: 'module' });
        try { runs.push({ incremental, fresh: await run(fresh, source) }); }
        finally { fresh.terminate(); }
      }
      return runs;
    } finally { shared.terminate(); }
  }, { sources, workerFile, image: Array.from(image) });

  for (const [index, { incremental, fresh }] of runs.entries()) {
    assert.equal(incremental.packetHash, fresh.packetHash, `edit ${index} matches a fresh browser session`);
    assert.equal(incremental.packetHash, nativeHashes[index], `edit ${index} matches native output`);
  }
  const [cold, warm, unrelatedEdit, dependentEdit] = runs.map(run => run.incremental);
  assert.ok(cold.stats.bakedJobs > 0, 'fixture actually rasterizes materials');
  assert.ok(cold.stats.sourceDecodes > 0, 'fixture decodes procedural source images');
  assert.equal(warm.stats.bakedJobs, cold.stats.bakedJobs, 'warm request reuses every raster');
  assert.equal(warm.stats.sourceDecodes, cold.stats.sourceDecodes, 'warm request skips procedural source decoding');
  assert.equal(unrelatedEdit.stats.bakedJobs, warm.stats.bakedJobs, 'structural finish edit reuses unrelated rasters');
  assert.equal(unrelatedEdit.stats.sourceDecodes, warm.stats.sourceDecodes, 'unrelated edit skips procedural source decoding');
  assert.notEqual(unrelatedEdit.packetHash, warm.packetHash, 'structural finish edit reaches the scene');
  assert.equal(unrelatedEdit.assetHash, warm.assetHash, 'unrelated finish preserves baked pixels');
  assert.ok(dependentEdit.stats.bakedJobs > unrelatedEdit.stats.bakedJobs, 'sampled texture-scale edit regenerates dependent rasters');
  assert.notEqual(dependentEdit.assetHash, unrelatedEdit.assetHash, 'sampled texture-scale edit changes baked pixels');
  await writeFile(resolve(directory, 'result.json'), JSON.stringify(runs, null, 2));
}
