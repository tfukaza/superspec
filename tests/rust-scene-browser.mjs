/** Real Three.js material realization and asynchronous image ownership. */
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile, copyFile, cp } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { resolve, extname, sep } from 'node:path';
import { build } from 'vite';
import { PNG } from 'pngjs';
import { launchBrowser, waitForFrames } from '../tools/browser.mjs';

const scope = process.env.RUST_SCENE_SCOPE ?? 'all';
assert.ok(['all','ground','rooftop','cells','facade','shop-equipment','shop-stock','grocer','greengrocer','hardware','herbalist','roast','shop-facade','pavement','spans','buildings'].includes(scope), 'Unknown Rust scene acceptance scope');
const root = resolve(scope === 'all' ? 'generated/rust-scene-browser' : `generated/rust-scene-browser-${scope}`);
await build({ configFile: false, root: 'tests/browser', publicDir: false,
  build: { outDir: root, emptyOutDir: true, rollupOptions: { input: resolve('tests/browser/rust-scene.html') } } });
await mkdir(`${root}/inputs`, { recursive: true });
for (const file of ['superspec_kernel_acceptance.js', 'superspec_kernel_acceptance_bg.wasm']) {
  await copyFile(resolve('generated/wasm-tests', file), resolve(root, file));
}
const executable = resolve(process.env.RUST_CLI ?? `target/release/superspec${process.platform === 'win32' ? '.exe' : ''}`);
const pixels = { color: [231, 48, 72, 255], normal: [112, 145, 242, 255], data: [0, 73, 198, 255], height: [92, 92, 92, 255] };
for (const [name, pixel] of Object.entries(pixels)) {
  const png = new PNG({ width: 2, height: 2 });
  for (let i = 0; i < 4; i++) png.data.set(pixel.map((v, channel) => channel < 3 ? Math.max(0, v - i * 8) : v), i * 4);
  await writeFile(`${root}/inputs/${name}.png`, PNG.sync.write(png));
}
const source = `<architecture version="1" id="channels">
<material id="glass" texture="color.png" normal-texture="normal.png" normal-scale="0.35" metallic-roughness-texture="data.png" height-texture="height.png" height-scale="0.05" opacity="0.6"/>
<material id="cutout-paint" texture="color.png" alpha-cutoff="0.4"/>
<box id="front" width="2" depth="1" height="2" material="glass"/>
<box id="back" width="2" depth="1" height="2" position="0 1.2 0" material="glass"/>
<box id="cutout" width="1" depth="1" height="2" position="2.5 0 0" material="cutout-paint"/>
</architecture>`;
await writeFile(`${root}/inputs/scene.aml`, source);
const textured = [...execFileSync(executable, ['scene', `${root}/inputs/scene.aml`], { env: { ...process.env, PATH: '' } })];
const plain = [...execFileSync(executable, ['scene', 'tests/fixtures/aml/rust-stage1.aml'], { env: { ...process.env, PATH: '' } })];
let building;
if (['all','buildings'].includes(scope)) {
  for (const file of ['superspec_wasm.js','superspec_wasm_bg.wasm']) await copyFile(resolve('generated/wasm',file),resolve(root,file));
  // The module imports its snippets (the texture encoder's glue) from beside itself.
  await cp(resolve('generated/wasm/snippets'),resolve(root,'snippets'),{recursive:true});
  const cases=[];
  for(const name of ['dressing','transformed','polygon','shops-open','shops-hole','shops-hidden','multiple-buildings','multiple-buildings-materials','base-wall','base-wall-transformed']) {
    const stem=name.startsWith('multiple-')?`rust-${name}`:`rust-building-${name}`;
    const path=`tests/fixtures/aml/${stem}.aml`;
    const source=await readFile(path,'utf8');
    const packet=execFileSync(executable,['scene',path],{env:{...process.env,PATH:''},maxBuffer:128*1024*1024});
    await writeFile(`${root}/building-${name}-native.packet`,packet);
    cases.push({name,source,hash:createHash('sha256').update(packet).digest('hex')});
  }
  const catalog=JSON.parse(await readFile('crates/catalog/src/material_catalog.json','utf8'));
  const names=[...new Set(Object.values(catalog).flatMap(m=>Object.values(m)).filter(v=>typeof v==='string'&&v.startsWith('superspec-material:')))];
  await mkdir(root+'/library',{recursive:true});
  for (const name of names) await copyFile('assets/materials/'+name.split(':')[1],root+'/library/'+name.split(':')[1]);
  building={cases,names};
}
const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    const path = resolve(root, name === '/' ? 'rust-scene.html' : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm' }[extname(path)] ?? 'application/octet-stream');
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 960, height: 640 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.driver);
  const apply = bytes => page.evaluate(bytes => window.driver.apply(bytes), bytes);
  const snapshot = () => page.evaluate(() => window.driver.snapshot());
  // Keep packed Wasm scenes in the browser; JSON numeric arrays multiply test-runner memory.
  const applyFixture = (name, ...args) => page.evaluate(async ({name,args}) => {
    const kernel = await import('/superspec_kernel_acceptance.js');
    return window.driver.apply(kernel[name](...args));
  }, {name,args});
  assert.equal(await apply(textured), true);
  await waitForFrames(page);
  const baseline = await snapshot();
  const glass = baseline.materials.front;
  assert.equal(glass.colorSpace, 'srgb');
  assert.equal(glass.vertexColors, true);
  assert.equal(glass.vertexColorSize, 3);
  assert.ok(glass.positionCount > 0);
  assert.equal(glass.vertexColorCount, glass.positionCount);
  assert.equal(glass.vertexColorsData.length, glass.vertexColorCount * 3);
  assert.ok(glass.vertexColorsData.every(value => value === 1));
  assert.equal(glass.normalColorSpace, '');
  assert.equal(glass.dataColorSpace, '');
  assert.equal(glass.sharedDataMap, true);
  assert.equal(glass.distinctMaps, true);
  assert.deepEqual(glass.colorPixel, pixels.color);
  assert.deepEqual(glass.normalPixel, pixels.normal);
  assert.deepEqual(glass.dataPixel, pixels.data);
  assert.deepEqual(glass.normalScale, [0.35, 0.35]);
  assert.equal(glass.bump, false);
  assert.equal(glass.opacity, 0.6);
  assert.equal(glass.transparent, true);
  assert.equal(glass.depthWrite, false);
  assert.equal(glass.side, 0);
  assert.equal(baseline.materials.cutout.alphaTest, 0.4);
  assert.equal(baseline.materials.cutout.alphaToCoverage, true);
  assert.equal(baseline.materials.cutout.depthWrite, true);
  assert.equal(baseline.materials.cutout.side, 2);
  assert.equal(baseline.materials.cutout.polygonOffset, true);
  assert.equal(baseline.materials.cutout.polygonOffsetFactor, -1);
  assert.equal(baseline.materials.cutout.polygonOffsetUnits, -2);
  await page.screenshot({ path: `${root}/materials.png` });
  await page.evaluate(async()=>{
    const kernel=await import('/superspec_kernel_acceptance.js');
    await kernel.default({module_or_path:'/superspec_kernel_acceptance_bg.wasm'});
  });
  if(scope === 'all') {
  assert.equal(await page.evaluate(async () => {
    const kernel = await import('/superspec_kernel_acceptance.js');
    return window.driver.apply(Array.from(kernel.procedural_scene_packet()));
  }), true);
  await waitForFrames(page);
  const procedural = await snapshot();
  for (const [id, tint] of [['assembly:paint', [.25, .5, 1.5]], ['rail:metal', [.75, 0, .25]]]) {
    const material = procedural.materials[`placement/host/$dressing/${id}`];
    assert.equal(material.vertexColors, true);
    assert.equal(material.vertexColorSize, 3);
    assert.ok(material.positionCount > 0);
    assert.equal(material.vertexColorCount, material.positionCount);
    assert.equal(material.vertexColorsData.length, material.vertexColorCount * 3);
    assert.ok(material.vertexColorsData.every((value, index) => value === tint[index % 3]));
    assert.deepEqual(material.materialColor, [1, 1, 1], 'vertex tint must not be folded into the material factor a second time');
    assert.deepEqual(material.colorPixel, pixels.color, 'color payload offsets preserve the actual source image');
  }
  await page.screenshot({ path: `${root}/procedural.png` });
  assert.equal(await page.evaluate(async () => {
    const kernel = await import('/superspec_kernel_acceptance.js');
    return window.driver.apply(Array.from(kernel.window_scene_packet()));
  }), true);
  await waitForFrames(page);
  const windows = await snapshot();
  for (let i=0;i<3;i++) {
    const glass=windows.materials[`host/$dressing/window-${i}:glass`];
    assert.ok(glass.positionCount>0);
    assert.equal(glass.vertexColorCount,glass.positionCount);
    assert.equal(glass.opacity,0.28);
    assert.equal(glass.transparent,true);
    assert.ok(windows.materials[`host/$dressing/window-${i}:paint`].positionCount>0);
    assert.ok(windows.materials[`host/$dressing/window-${i}:pane`].positionCount>0);
  }
  await page.screenshot({ path: `${root}/windows.png` });
  assert.equal(await page.evaluate(async () => {
    const kernel=await import('/superspec_kernel_acceptance.js');
    return window.driver.apply(Array.from(kernel.timber_scene_packet()));
  }),true);
  await waitForFrames(page);
  const timber=(await snapshot()).materials['host/$dressing/window-0:shop-timber'];
  assert.ok(timber.positionCount>0);assert.equal(timber.uvCount,timber.positionCount);
  assert.equal(timber.vertexColorCount,timber.positionCount);
  assert.ok(timber.uvData.every(value=>Number.isFinite(value) && value>=0 && value<=1));
  assert.equal(timber.colorSpace,'srgb');assert.equal(timber.normalColorSpace,'');assert.equal(timber.dataColorSpace,'');
  assert.deepEqual(timber.materialColor,[1,1,1]);assert.equal(timber.sharedDataMap,true);
  for(const [field,file] of [['colorPixel','shop-timber.png'],['normalPixel','shop-timber-normal.png'],['dataPixel','shop-timber-properties.png']]) {
    const image=PNG.sync.read(await readFile(`assets/materials/${file}`));
    assert.deepEqual(timber[field],[...image.data.subarray(0,4)]);
  }
  await page.evaluate(() => window.driver.focus('host/$dressing/window-0:shop-timber'));
  await waitForFrames(page);
  await page.screenshot({path:`${root}/timber-window.png`});
  const bakedBytes=Buffer.from(await page.evaluate(async()=>{
    const kernel=await import('/superspec_kernel_acceptance.js');return Array.from(kernel.baked_scene_packet());
  }));
  assert.equal(await apply(Array.from(bakedBytes)),true);
  await waitForFrames(page);
  const baked=(await snapshot()).materials['host/$dressing/panel:metal'];
  assert.equal(baked.positionCount,4);assert.equal(baked.uvCount,4);
  assert.equal(baked.colorSpace,'srgb');assert.equal(baked.normalColorSpace,'');assert.equal(baked.dataColorSpace,'');
  assert.deepEqual(baked.materialColor,[1,1,1]);assert.equal(baked.sharedDataMap,true);assert.equal(baked.bump,false);
  const jsonLength=bakedBytes.readUInt32LE(4),manifest=JSON.parse(bakedBytes.toString('utf8',8,8+jsonLength));
  const assetStart=8+jsonLength+manifest.vertexCount*11*4+manifest.indexCount*4;
  const bakedMaterial=manifest.materials.find(m=>m.id==='$surface/raster/fixture-metal/0');
  for(const [field,channel] of [['colorPixel','texture'],['normalPixel','normalTexture'],['dataPixel','metallicRoughnessTexture']]) {
    const asset=manifest.assets.find(a=>a.id===bakedMaterial[channel]);
    const decoded=PNG.sync.read(bakedBytes.subarray(assetStart+asset.byteStart,assetStart+asset.byteStart+asset.byteLength));
    assert.deepEqual(baked[field],[...decoded.data.subarray(0,4)]);
  }
  await page.evaluate(()=>window.driver.focus('host/$dressing/panel:metal'));
  await waitForFrames(page);await page.screenshot({path:`${root}/baked-metal.png`});
  assert.equal(await applyFixture('shutter_scene_packet'),true);
  const shutter=await snapshot();
  const curtainName='host/$dressing/shutter:$surface/raster/shutter/0/0';
  const curtain=shutter.materials[curtainName];
  assert.ok(curtain.positionCount>100);
  assert.equal(curtain.colorSpace,'srgb');assert.equal(curtain.normalColorSpace,'');assert.equal(curtain.dataColorSpace,'');
  assert.equal(curtain.sharedDataMap,true);assert.equal(curtain.bump,false);
  assert.ok(shutter.materials['host/$dressing/shutter:paint'].positionCount>0,'Compact guide members retain simple paint');
  await page.evaluate(name=>window.driver.focus(name),curtainName);
  await waitForFrames(page);await page.screenshot({path:`${root}/shutter.png`});
  assert.equal(await applyFixture('weathered_window_packet'),true);
  const finishedWindow=await snapshot();
  assert.ok(Object.keys(finishedWindow.materials).some(name=>name.includes('$surface/raster/window/')));
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/weathered-window.png`});
  assert.equal(await applyFixture('pipe_scene_packet'),true);
  const pipes=await snapshot();
  const pipeMaterials=Object.entries(pipes.materials).filter(([name])=>name.includes('$surface/raster/pipes/'));
  assert.ok(pipeMaterials.length>3);
  for(const [,material] of pipeMaterials) {
    assert.ok(material.positionCount>0);assert.equal(material.uvCount,material.positionCount);
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.bump,false);
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/pipes.png`});
  for(const round of [true,false]) {
    assert.equal(await applyFixture('tank_scene_packet',round),true);
    const tank=await snapshot();
    assert.equal(tank.reflectionLighting,true);
    assert.ok(Object.keys(tank.materials).some(name=>name.includes('$surface/raster/tank/')));
    if(round) {
      const stainless=tank.materials['host/$dressing/tank:brushed-metal'];
      assert.ok(stainless.positionCount>100);
      assert.equal(stainless.uvCount,stainless.positionCount);
      assert.equal(stainless.colorSpace,'srgb');assert.equal(stainless.normalColorSpace,'');assert.equal(stainless.dataColorSpace,'');
      assert.equal(stainless.sharedDataMap,true);
      for(const [field,file] of [['colorPixel','brushed-metal.png'],['normalPixel','brushed-metal-normal.png'],['dataPixel','brushed-metal-properties.png']]) {
        const image=PNG.sync.read(await readFile(`assets/materials/${file}`));
        assert.deepEqual(stainless[field],[...image.data.subarray(0,4)]);
      }
    }
    await page.evaluate(()=>window.driver.focus());
    await waitForFrames(page);await page.screenshot({path:`${root}/${round?'round':'box'}-tank.png`});
  }
  assert.equal(await applyFixture('aircon_scene_packet'),true);
  const aircon=await snapshot();
  const atlasImage=PNG.sync.read(await readFile('assets/materials/aircon.png'));
  for(const part of ['window-unit','condenser']) {
    const material=aircon.materials[`host/$dressing/${part}:aircon`];
    assert.ok(material.positionCount>0);assert.equal(material.uvCount,material.positionCount);assert.equal(material.colorSpace,'srgb');
    assert.ok(material.uvData.every(v=>Number.isFinite(v)&&v>=0&&v<=1));
    assert.deepEqual(material.colorPixel,[...atlasImage.data.subarray(0,4)]);
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/aircon.png`});
  assert.equal(await applyFixture('painted_scene_packet'),true);
  const painted=await snapshot();
  const paintMaps=Object.entries(painted.materials).filter(([name])=>name.includes('$surface/raster/painted/'));
  assert.ok(paintMaps.length>=3);
  for(const [,material] of paintMaps){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.bump,false);
    assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/painted.png`});
  assert.equal(await applyFixture('door_scene_packet'),true);
  const doors=await snapshot();
  assert.ok(Object.keys(doors.materials).some(name=>name.includes('$surface/raster/doors/')));
  const timberDoor=doors.materials['host/$dressing/door-4:shop-timber'];
  assert.ok(timberDoor.positionCount>100);assert.equal(timberDoor.colorSpace,'srgb');
  assert.equal(timberDoor.normalColorSpace,'');assert.equal(timberDoor.dataColorSpace,'');assert.equal(timberDoor.sharedDataMap,true);
  const notices=doors.materials['host/$dressing/fixtures/notice-0:posters'];
  assert.ok(notices.positionCount>0);assert.equal(notices.colorSpace,'srgb');assert.equal(notices.uvCount,notices.positionCount);
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/doors.png`});
  await page.evaluate(()=>window.driver.focus('host/$dressing/door-4:shop-timber'));
  await waitForFrames(page);await page.screenshot({path:`${root}/door-timber.png`});
  for(const concrete of [false,true]) {
    assert.equal(await applyFixture('facade_scene_packet',concrete),true);
    const facade=await snapshot();
    const walls=Object.entries(facade.materials).filter(([name])=>name.startsWith('host/$dressing/wall-'));
    assert.equal(walls.length,2);
    for(const [name,material] of walls){
      assert.ok(name.endsWith(':$surface/raster/facade/0/0'));
      assert.equal(material.repeatMaps,true);
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.bump,false);
    }
    await page.evaluate(()=>window.driver.focus());
    await waitForFrames(page);await page.screenshot({path:`${root}/facade-${concrete?'concrete':'plaster'}.png`});
  }
  assert.equal(await applyFixture('skin_scene_packet'),true);
  const skin=await snapshot();
  const skinMaterials=Object.entries(skin.materials).filter(([name])=>name.startsWith('host/$dressing/skin:'));
  assert.equal(skinMaterials.length,4);
  for(const [,material] of skinMaterials){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.bump,false);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/skin.png`});
  assert.equal(await applyFixture('tile_scene_packet'),true);
  const tiles=await snapshot();
  const tileMaterials=Object.entries(tiles.materials).filter(([name])=>name.startsWith('host/$dressing/tile-'));
  assert.equal(tileMaterials.length,3);
  for(const [,material] of tileMaterials){
    assert.equal(material.positionCount,4);assert.equal(material.uvCount,4);assert.equal(material.bump,false);
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');assert.equal(material.sharedDataMap,true);
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/tiles.png`});
  assert.equal(await applyFixture('tiled_skin_scene_packet'),true);
  const tiledSkin=await snapshot();
  const tiledWalls=Object.entries(tiledSkin.materials).filter(([name])=>name.startsWith('host/$dressing/skin:'));
  assert.equal(tiledWalls.length,1);
  assert.equal(tiledWalls[0][1].normalColorSpace,'');assert.equal(tiledWalls[0][1].sharedDataMap,true);
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/tiled-skin.png`});
  for(const tiled of [false,true]) {
    assert.equal(await applyFixture('stained_skin_scene_packet',tiled),true);
    const state=await snapshot();
    const walls=Object.entries(state.materials).filter(([name])=>name.startsWith('host/$dressing/skin:'));
    assert.equal(walls.length,tiled?1:4);
    for(const [,material] of walls) {assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.sharedDataMap,true);}
    await page.evaluate(()=>window.driver.focus());
    await waitForFrames(page);await page.screenshot({path:`${root}/stained-${tiled?'tiles':'plaster'}.png`});
  }
  assert.equal(await applyFixture('roof_access_scene_packet'),true);
  const roofAccess=await snapshot();
  const roofMaterials=Object.entries(roofAccess.materials).filter(([name])=>name.startsWith('host/$dressing/roof-access:'));
  assert.ok(roofMaterials.length>4);
  for(const [name,material] of roofMaterials){
    if(name.includes('$surface/raster/')){
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/roof-access.png`});
  await page.evaluate(()=>window.driver.focus(undefined,[1.1,-1.5,0.8]));
  await waitForFrames(page);await page.screenshot({path:`${root}/roof-access-overview.png`});
  assert.equal(await applyFixture('balcony_props_scene_packet'),true);
  const balconyProps=await snapshot();
  for(const slot of ['balcony-plants','balcony-cloth']){
    const parts=Object.entries(balconyProps.materials).filter(([name])=>name.endsWith(`:${slot}`));assert.equal(parts.length,3);
    const atlas=PNG.sync.read(await readFile(`assets/materials/${slot}.png`));
    for(const [,material] of parts){
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');assert.equal(material.sharedDataMap,true);
      assert.deepEqual(material.colorPixel,[...atlas.data.subarray(0,4)]);assert.equal(material.uvCount,material.positionCount);
    }
  }
  await page.evaluate(()=>window.driver.focus());
  await waitForFrames(page);await page.screenshot({path:`${root}/balcony-props.png`});
  assert.equal(await applyFixture('laundry_scene_packet'),true);
  const laundry=await snapshot();
  const cloth=Object.entries(laundry.materials).filter(([name])=>name.endsWith(':balcony-cloth'));
  assert.equal(cloth.length,3);
  for(const [,material] of cloth){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.5,-1.5,0.7]));
  await waitForFrames(page);await page.screenshot({path:`${root}/laundry.png`});
  assert.equal(await applyFixture('household_scene_packet'),true);
  const household=await snapshot();
  const householdBakes=Object.entries(household.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(householdBakes.length>4);
  for(const [,material] of householdBakes){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.35,-1.5,0.65]));
  await waitForFrames(page);await page.screenshot({path:`${root}/household.png`});
  assert.equal(await applyFixture('tarps_scene_packet'),true);
  const tarps=await snapshot();
  const canvas=Object.entries(tarps.materials).filter(([name])=>name.endsWith(':tarp'));
  assert.equal(canvas.length,3);
  const canvasAtlas=PNG.sync.read(await readFile('assets/materials/tarp.png'));
  for(const [,material] of canvas){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    assert.deepEqual(material.colorPixel,[...canvasAtlas.data.subarray(0,4)]);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.25,1.5,0.55]));
  await waitForFrames(page);await page.screenshot({path:`${root}/tarps.png`});
  await page.evaluate(()=>window.driver.focus('host/$dressing/canvas-1:tarp',[0.25,1.5,0.55]));
  await waitForFrames(page);await page.screenshot({path:`${root}/tarps-close.png`});
  assert.equal(await applyFixture('balconies_scene_packet'),true);
  const balconies=await snapshot();
  const balconyBakes=Object.entries(balconies.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(balconyBakes.length>4);
  for(const [,material] of balconyBakes){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.65,1.5,0.8]));
  await waitForFrames(page);await page.screenshot({path:`${root}/balconies.png`});
  assert.equal(await applyFixture('facade_details_scene_packet'),true);
  const facadeDetails=await snapshot();
  const facadeDetailBakes=Object.entries(facadeDetails.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(facadeDetailBakes.length>4);
  for(const [,material] of facadeDetailBakes){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.25,1.5,0.2]));
  await waitForFrames(page);await page.screenshot({path:`${root}/facade-details.png`});
  assert.equal(await applyFixture('signs_scene_packet'),true);
  const signs=await snapshot();
  const signBakes=Object.entries(signs.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(signBakes.length>=5);
  for(const [,material] of signBakes){
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
    assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[0.15,1.5,0.25]));
  await waitForFrames(page);await page.screenshot({path:`${root}/signs.png`});
  assert.equal(await applyFixture('projecting_signs_scene_packet'),true);
  const projecting=await snapshot();
  const projectingBakes=Object.entries(projecting.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(projectingBakes.length>=8);
  for(const [,material] of projectingBakes) {
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.sharedDataMap,true);
  }
  await page.evaluate(()=>window.driver.focus(undefined,[1.2,1.5,0.35]));
  await waitForFrames(page);await page.screenshot({path:`${root}/projecting-signs.png`});
  }
  if(['all','ground'].includes(scope)) {
  assert.equal(await applyFixture('ground_scene_packet'),true);
  const ground=await snapshot();
  const groundBakes=Object.entries(ground.materials).filter(([name])=>name.includes('$surface/raster/'));
  assert.ok(groundBakes.length>=5);
  for(const [,material] of groundBakes) {
    assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.sharedDataMap,true);
    assert.equal(material.uvCount,material.positionCount);
  }
  const wood=Object.entries(ground.materials).find(([name])=>name.endsWith(':shop-timber'));
  assert.ok(wood);assert.equal(wood[1].normalColorSpace,'');assert.equal(wood[1].sharedDataMap,true);
  await page.evaluate(()=>window.driver.focus(undefined,[0.1,-1.5,0.25]));
  await waitForFrames(page);await page.screenshot({path:`${root}/ground.png`});
  }
  if(['all','buildings'].includes(scope)) {
    const views=[];
    for(const entry of building.cases) {
    const parity=await page.evaluate(async ({source,names,hash})=>{
      const wasm=await import('/superspec_wasm.js');await wasm.default();
      const compiler=new wasm.Compiler();
      try {
        for(const name of names) compiler.set_asset(name,new Uint8Array(await (await fetch('/library/'+name.split(':')[1])).arrayBuffer()));
        const packet=compiler.compile(source,0);
        const digest=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',packet))).map(b=>b.toString(16).padStart(2,'0')).join('');
        if(digest!==hash) return {equal:false,applied:false};
        return {equal:true,applied:await window.driver.apply(packet)};
      } finally {compiler.free();}
    },{...entry,names:building.names});
    assert.deepEqual(parity,{equal:true,applied:true},`${entry.name}: full AML scene packet must match native and Wasm`);
    const state=await snapshot();
    assert.ok(Object.keys(state.materials).some(name=>name.startsWith('host/$dressing/roof:')));
    if(['dressing','polygon','shops-open','multiple-buildings','multiple-buildings-materials','base-wall','base-wall-transformed'].includes(entry.name)) {
    views.push({name:entry.name,view:await page.evaluate(()=>window.driver.focus(undefined,[0.35,-0.6,0.6]))});
    await waitForFrames(page);await page.screenshot({path:`${root}/building-${entry.name}.png`});
    const wall=Object.keys(state.materials).find(name=>name.startsWith('host/$dressing/envelope:'));
    assert.ok(wall);
    views.push({name:entry.name+'-close',view:await page.evaluate(name=>window.driver.focus(name,[0.25,-1,0.4]),wall)});
    await waitForFrames(page);await page.screenshot({path:`${root}/building-${entry.name}-close.png`});
    if(entry.name==='shops-open') {
      const sign=Object.keys(state.materials).find(name=>name.startsWith('host/$dressing/signage:'));
      assert.ok(sign);
      views.push({name:'shop-sign',view:await page.evaluate(name=>window.driver.focus(name,[-0.6,-1,0.2]),sign)});
      await waitForFrames(page);await page.screenshot({path:root+'/building-shop-sign.png'});
    }
    if(entry.name.startsWith('multiple-')) {
      assert.ok(Object.keys(state.materials).some(name=>name.startsWith('second/$dressing/roof:')));
      const washing=Object.keys(state.materials).find(name=>name.startsWith('second/$dressing/span-washing:')&&name.endsWith(':$library/balcony-cloth'))
        ??Object.keys(state.materials).find(name=>name.startsWith('second/$dressing/span-washing:'));
      assert.ok(washing);
      views.push({name:entry.name+'-washing',view:await page.evaluate(name=>window.driver.focus(name,[0,-1,0.2]),washing)});
      await waitForFrames(page);await page.screenshot({path:`${root}/building-${entry.name}-washing.png`});
    }
    }
    }
    await writeFile(root+'/building-views.json',JSON.stringify(views,null,2));
  }
  if(['all','spans'].includes(scope)) {
    assert.equal(await applyFixture('span_scene_packet'),true);
    const spans=await snapshot();
    const bakes=Object.entries(spans.materials).filter(([name])=>name.includes('$surface/raster/'));
    assert.ok(bakes.length>=8,'Span signs and hardware must deliver baked material fields');
    for(const [,material] of bakes) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    const washing=Object.keys(spans.materials).find(name=>name.includes('span-washing')&&name.endsWith(':balcony-cloth'));
    assert.ok(washing,'Washing retains its source garment atlas');
    assert.equal(spans.materials[washing].normalColorSpace,'');
    await page.evaluate(()=>window.driver.focus(undefined,[-1,0,0.6]));
    await waitForFrames(page);await page.screenshot({path:root+'/spans.png'});
    await page.evaluate(name=>window.driver.focus(name,[-1,0,0.15]),washing);
    await waitForFrames(page);await page.screenshot({path:root+'/span-washing.png'});
  }
  if(['all','pavement'].includes(scope)) {
    assert.equal(await applyFixture('pavement_scene_packet'),true);
    const pavement=await snapshot();
    const bakes=Object.entries(pavement.materials).filter(([name])=>name.includes('$surface/raster/'));
    assert.ok(bakes.length>=8,'Pavement pours must deliver their baked material fields');
    for(const [,material] of bakes) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    const water=Object.entries(pavement.materials).filter(([name])=>name.includes('water-film'));
    assert.ok(water.length>0,'Depressions retain transparent water films');
    for(const [,material] of water) assert.equal(material.opacity,0.16);
    await page.evaluate(()=>window.driver.focus(undefined,[0.5,-1,1.4]));
    await waitForFrames(page);await page.screenshot({path:root+'/pavement.png'});
    const tiled=Object.keys(pavement.materials).find(name=>name.includes('panel-1:')&&name.includes('$surface/raster/'));
    assert.ok(tiled,'Tiled floor must have a real raster receiver');
    await page.evaluate(name=>window.driver.focus(name,[0.4,-0.8,1.4]),tiled);
    await waitForFrames(page);await page.screenshot({path:root+'/pavement-tiles.png'});
  }
  if(['all','rooftop'].includes(scope)) {
    assert.equal(await applyFixture('rooftop_scene_packet'),true);
    const rooftop=await snapshot();
    const bakes=Object.entries(rooftop.materials).filter(([name])=>name.includes('$surface/raster/'));
    assert.ok(bakes.length>=8);
    for(const [,material] of bakes) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    await page.evaluate(()=>window.driver.focus(undefined,[0.8,-1.3,1.1]));
    await waitForFrames(page);await page.screenshot({path:`${root}/rooftop.png`});
    await page.evaluate(()=>window.driver.focus(undefined,[-0.8,1.3,1.1]));
    await waitForFrames(page);await page.screenshot({path:`${root}/rooftop-rear.png`});
  }
  if(['all','cells'].includes(scope)) {
    assert.equal(await page.evaluate(async()=>{
      const kernel=await import('/superspec_kernel_acceptance.js');
      return window.driver.apply(kernel.cells_scene_packet());
    }),true);
    const cells=await snapshot();
    const bakes=Object.entries(cells.materials).filter(([name])=>name.includes('$surface/raster/'));
    assert.ok(bakes.length>=12);
    for(const [,material] of bakes) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    await page.evaluate(()=>window.driver.focus(undefined,[0.1,1.5,0.35]));
    await waitForFrames(page);await page.screenshot({path:`${root}/cells.png`});
    const balconyWall=Object.keys(cells.materials).find(name=>name.includes('/bay-4:'));
    assert.ok(balconyWall,'Glazed balcony wall must be present in the delivered scene');
    await page.evaluate(name=>window.driver.focus(name,[0.65,1.5,0.45]),balconyWall);
    await waitForFrames(page);await page.screenshot({path:`${root}/glazed-balcony.png`});
  }
  if(['all','facade'].includes(scope)) {
    assert.equal(await applyFixture('complete_facade_scene_packet'),true);
    const facade=await snapshot();
    const bakes=Object.entries(facade.materials).filter(([name])=>name.includes('$surface/raster/'));
    assert.ok(bakes.length>=12);
    for(const [,material] of bakes) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    await page.evaluate(()=>window.driver.focus(undefined,[0.25,1.5,0.4]));
    await waitForFrames(page);await page.screenshot({path:`${root}/facade.png`});
    await page.evaluate(()=>window.driver.focus(undefined,[-0.8,1.2,0.25]));
    await waitForFrames(page);await page.screenshot({path:`${root}/facade-oblique.png`});
  }
  if(['all','shop-facade'].includes(scope)) {
    assert.equal(await applyFixture('open_facade_scene_packet'),true);
    const state=await snapshot();
    const shopParts=Object.keys(state.materials).filter(name=>name.includes('/shop-'));
    assert.ok(shopParts.some(name=>name.includes('/shop-frontage:')));
    const interior=shopParts.find(name=>!name.includes('/shop-frontage:')&&name.endsWith(':grocer-props'));
    assert.ok(interior,'A furnished shop must deliver its stocked interior');
    await page.evaluate(()=>window.driver.focus(undefined,[0.25,1.5,0.4]));
    await waitForFrames(page);await page.screenshot({path:root+'/shop-facade.png'});
    await page.evaluate(name=>window.driver.focus(name,[0.2,1.5,0.25]),interior);
    await waitForFrames(page);await page.screenshot({path:root+'/shop-interior.png'});
  }
  if(['all','shop-equipment'].includes(scope)) {
    assert.equal(await applyFixture('shop_equipment_scene_packet'),true);
    const equipment=await snapshot();
    const wood=Object.entries(equipment.materials).filter(([name])=>name.endsWith(':shop-timber'));
    assert.ok(wood.length>=2);
    for(const [,material] of wood) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    assert.ok(Object.keys(equipment.materials).some(name=>name.endsWith(':posters')));
    await page.evaluate(()=>window.driver.focus(undefined,[0.35,-1.4,0.65]));
    await waitForFrames(page);await page.screenshot({path:`${root}/shop-equipment.png`});
    await page.evaluate(()=>window.driver.focus('host/$dressing/equipment-0:shop-timber',[0.5,-1.3,0.6]));
    await waitForFrames(page);await page.screenshot({path:`${root}/shop-timber.png`});
  }
  if(['all','shop-stock'].includes(scope)) {
    assert.equal(await applyFixture('shop_stock_scene_packet'),true);
    const stock=await snapshot();
    const props=Object.entries(stock.materials).filter(([name])=>name.endsWith(':grocer-props'));
    assert.ok(props.length>=21);
    for(const [,material] of props) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    for(const slot of ['market-apple','snack-packet'])assert.ok(Object.keys(stock.materials).some(name=>name.endsWith(`:${slot}`)));
    await page.evaluate(()=>window.driver.focus(undefined,[0.2,-1.5,1.3]));
    await waitForFrames(page);await page.screenshot({path:`${root}/shop-stock.png`});
    await page.evaluate(()=>window.driver.focus('host/$dressing/bag-group-1:grocer-props',[0.4,-1.4,0.6]));
    await waitForFrames(page);await page.screenshot({path:`${root}/bag-group.png`});
  }
  assert.equal(await apply(textured), true);
  for (const trade of ['grocer','greengrocer','hardware','herbalist','roast']) if(['all',trade].includes(scope)) {
    assert.equal(await applyFixture(trade+'_scene_packet'),true);
    const state=await snapshot();
    const expectedSlots={grocer:['shop-timber','grocer-props','snack-packet'],greengrocer:['shop-timber','grocer-props'],hardware:['paint','metal','grocer-props'],herbalist:['shop-timber','paint','pane'],roast:['kitchen-metal','restaurant-food','restaurant-tiles','display-glass']}[trade];
    for(const slot of expectedSlots)assert.ok(Object.keys(state.materials).some(name=>name.endsWith(':'+slot)));
    const props=Object.entries(state.materials).filter(([name])=>name.endsWith(':grocer-props'));
    for(const [,material] of props) {
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');
      assert.equal(material.sharedDataMap,true);assert.equal(material.uvCount,material.positionCount);
    }
    await page.evaluate(()=>window.driver.focus(undefined,[0.15,-1.4,0.8]));
    await waitForFrames(page);await page.screenshot({path:root+'/'+trade+'.png'});
    await page.evaluate(trade=>window.driver.focus('host/$dressing/grocer:'+(trade==='grocer'?'snack-packet':trade==='roast'?'kitchen-metal':'paint'),[0.3,-1.3,0.4]),trade);
    await waitForFrames(page);await page.screenshot({path:root+'/'+(trade==='grocer'?'packet-strings':trade==='greengrocer'?'produce-crates':trade+'-detail')+'.png'});
    assert.equal(await apply(textured),true);
  }
  // A decode can only be held when one happens: the viewer keeps the displayed scene's decoded images (its texture
  // pool), so a scene whose images are on screen decodes nothing when it is applied again. Each gated scene therefore
  // follows the untextured one, whose display released them.
  for (const fail of [false, true]) {
    // Superseded while its image decodes: the obsolete scene is dropped whether the decode then succeeds or fails. A
    // failure of a scene nobody waits for any more is not an error of the viewer (`RustScene.apply` answers false).
    assert.equal(await apply(plain), true);
    await page.evaluate(bytes => window.driver.start(bytes), textured);
    await page.waitForFunction(() => window.driver.gated());
    assert.equal(await apply(plain), true);
    const latest = await snapshot();
    const result = await page.evaluate(fail => window.driver.release(fail), fail);
    assert.deepEqual(result, { applied: false, error: false });
    assert.deepEqual(await snapshot(), latest, 'obsolete decode must not replace the latest rendered scene');
    assert.equal(await apply(textured), true, 'textured edits recover after cancelled or failed decoding');
  }
  {
    // A late failure of the scene that is still wanted: the apply rejects, the scene on screen stays, and the image is
    // decoded again (not answered from the failed attempt) the next time it is asked for.
    assert.equal(await apply(plain), true);
    const latest = await snapshot();
    await page.evaluate(bytes => window.driver.start(bytes), textured);
    await page.waitForFunction(() => window.driver.gated());
    assert.deepEqual(await page.evaluate(() => window.driver.release(true)), { applied: false, error: true });
    assert.deepEqual(await snapshot(), latest, 'a scene whose image fails to decode must not replace the rendered scene');
    assert.equal(await apply(textured), true, 'the same scene applies once its image decodes');
    assert.deepEqual((await snapshot()).materials.front.colorPixel, pixels.color);
  }
  for (let i = 0; i < 12; i++) { await apply(i % 2 ? textured : plain); await waitForFrames(page); }
  assert.deepEqual((await snapshot()).memory, baseline.memory, 'repeated textured edits release replaced GPU resources');
  assert.equal(await apply(plain), true);
  await page.evaluate(bytes => window.driver.start(bytes), textured);
  await page.waitForFunction(() => window.driver.gated());
  await page.evaluate(() => window.driver.close());
  const closed = await snapshot();
  assert.equal(closed.reflectionLighting,false);
  assert.deepEqual(await page.evaluate(() => window.driver.release()), { applied: false, error: false });
  assert.equal((await snapshot()).canvases, 0);
  assert.equal((await snapshot()).revision, closed.revision);
  assert.deepEqual(errors, []);
  await writeFile(`${root}/result.json`, JSON.stringify({ scope, materialChannels: true, timberAtlasChannels: ['all','ground'].includes(scope), supersession: true, lateFailure: true, closeDuringDecode: true, repeatedTexturedEdits: 12, baseline }, null, 2));
  console.log('Rust scene material/lifecycle acceptance passed.');
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
