/** Browser delivery acceptance: native-only producer, independent Khronos validator and actual GLTFLoader. */
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, copyFile, cp, mkdir } from 'node:fs/promises';
import { resolve, sep, extname } from 'node:path';
import { build } from 'vite';
import { PNG } from 'pngjs';
import { launchBrowser, waitForFrames } from '../tools/browser.mjs';

const root=resolve('generated/rust-export-browser');
await build({configFile:false,root:'tests/browser',publicDir:false,
  build:{outDir:root,emptyOutDir:true,rollupOptions:{input:resolve('tests/browser/rust-export.html')}}});
for(const file of ['superspec_wasm.js','superspec_wasm_bg.wasm'])await copyFile(resolve('generated/wasm',file),resolve(root,file));
// wasm-bindgen snippets hold the KTX2 encoder module that superspec_wasm.js imports.
await cp(resolve('generated/wasm/snippets'),resolve(root,'snippets'),{recursive:true});
// Three's Basis Universal transcoder for the KTX2 sample, served where the loader expects it.
await mkdir(resolve(root,'basis'),{recursive:true});
for(const file of ['basis_transcoder.js','basis_transcoder.wasm'])await copyFile(resolve('node_modules/three/examples/jsm/libs/basis',file),resolve(root,'basis',file));
const executable=resolve(process.env.RUST_CLI??`target/release/superspec${process.platform==='win32'?'.exe':''}`);
const pixels={color:[231,48,72,255],normal:[112,145,242,255],data:[0,73,198,255]};
for(const [name,pixel] of Object.entries(pixels)) {
  const png=new PNG({width:2,height:2});
  for(let i=0;i<4;i++)png.data.set(pixel,i*4);
  await writeFile(`${root}/${name}.png`,PNG.sync.write(png));
}
const source=`<architecture version="1" id="export-preview">
<material id="finish" color="#808080" texture="color.png" normal-texture="normal.png" normal-scale="0.35" metallic-roughness-texture="data.png" emissive="#204080" emissive-strength="3" alpha-cutoff="0.4"/>
<box id="surface" position="1 2 3" width="2" depth="3" height="4" material="finish"/>
<box id="visual-only" position="5 0 0" width="1" depth="1" height="2" collidable="false"/>
<box id="collision-only" position="-2 0 0" width="1" depth="1" height="2" visible="false"/>
</architecture>`;
await writeFile(`${root}/source.aml`,source);
const run=(args)=>execFileSync(executable,args,{env:{...process.env,PATH:'',NODE_OPTIONS:'',NODE_PATH:''},maxBuffer:128*1024*1024});
// Exact channel checks use PNG; the default KTX2 export is checked separately below.
for(const purpose of ['visual','collision'])await writeFile(`${root}/${purpose}.glb`,run(['glb',`${root}/source.aml`,'--textures','png',...(purpose==='collision'?['--collision']:[])]));
await writeFile(`${root}/ktx2.glb`,run(['glb',`${root}/source.aml`]));
const packet=run(['scene',`${root}/source.aml`]);
const jsonLength=packet.readUInt32LE(4);const manifest=JSON.parse(packet.subarray(8,8+jsonLength));
const positionStart=8+jsonLength;
const expected=Object.fromEntries(manifest.meshes.map(mesh=>[mesh.id,Array.from({length:mesh.vertexCount},(_,i)=>{
  const at=positionStart+(mesh.vertexStart+i)*12;
  return [packet.readFloatLE(at),packet.readFloatLE(at+8),-packet.readFloatLE(at+4)];
}).flat()]));
const server=createServer(async(request,response)=>{
  try {
    const name=decodeURIComponent(new URL(request.url,'http://localhost').pathname);
    const path=resolve(root,name==='/'?'rust-export.html':`.${name}`);
    if(!path.startsWith(root+sep)){response.writeHead(403).end();return;}
    response.setHeader('Content-Type',{'.html':'text/html','.js':'text/javascript','.glb':'model/gltf-binary','.wasm':'application/wasm'}[extname(path)]??'application/octet-stream');
    response.end(await readFile(path));
  }catch{response.writeHead(404).end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
try {
  browser=await launchBrowser();const page=await browser.newPage({viewport:{width:960,height:640}});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);await page.waitForFunction(()=>window.exportDriver);
  assert.equal(await page.evaluate(async()=>{
    const wasm=await import('/superspec_wasm.js');await wasm.default({module_or_path:'/superspec_wasm_bg.wasm'});
    const compiler=new wasm.Compiler();
    try {
      for(const name of ['color','normal','data'])compiler.set_asset(`${name}.png`,new Uint8Array(await (await fetch(`/${name}.png`)).arrayBuffer()));
      const source=await (await fetch('/source.aml')).text();
      for(const purpose of ['visual','collision']) {
        const actual=compiler.export_glb(source,purpose==='collision',false);
        const expected=new Uint8Array(await (await fetch(`/${purpose}.glb`)).arrayBuffer());
        if(actual.length!==expected.length || !actual.every((value,index)=>value===expected[index]))return false;
      }
      return true;
    } finally {compiler.free();}
  }),true,'production browser Wasm exports must equal native GLB bytes');
  const reports=[];
  for(const purpose of ['visual','collision']) {
    const result=await page.evaluate(purpose=>window.exportDriver.load(`/${purpose}.glb`),purpose);
    assert.equal(result.report.issues.numErrors,0,JSON.stringify(result.report.issues));
    assert.equal(result.report.issues.numWarnings,0,JSON.stringify(result.report.issues));
    reports.push(result.report);
    const selected=purpose==='visual'?manifest.meshes.filter(mesh=>mesh.visible).map(mesh=>mesh.id):manifest.colliders;
    assert.deepEqual(Object.keys(result.meshes).sort(),selected.slice().sort());
    for(const id of selected) {
      const mesh=result.meshes[id];assert.equal(mesh.world.length,expected[id].length);
      mesh.world.forEach((value,index)=>assert.ok(Math.abs(value-expected[id][index])<1e-5,`${id} world coordinate ${index}`));
      assert.deepEqual(mesh.source,manifest.meshes.find(mesh=>mesh.id===id).source);
      assert.equal(mesh.objectId,manifest.meshes.find(mesh=>mesh.id===id).objectId);
    }
    if(purpose==='visual') {
      const material=result.meshes.surface;
      assert.deepEqual(material.colorPixel,pixels.color);assert.deepEqual(material.normalPixel,pixels.normal);assert.deepEqual(material.dataPixel,pixels.data);
      assert.equal(material.colorSpace,'srgb');assert.equal(material.normalColorSpace,'');assert.equal(material.dataColorSpace,'');
      assert.deepEqual(material.normalScale,[.35,.35]);assert.equal(material.hasTangents,true);assert.equal(material.emissiveIntensity,3);assert.equal(material.alphaTest,.4);
      await waitForFrames(page);await page.screenshot({path:`${root}/visual.png`});
    }
  }
  // The default native export stores KTX2 textures, which load compressed through createGltfLoader.
  const compressed=await page.evaluate(()=>window.exportDriver.load('/ktx2.glb',true));
  assert.equal(compressed.report.issues.numErrors,0,JSON.stringify(compressed.report.issues));
  assert.deepEqual(compressed.meshes.surface.compressed,[true,true,true]);
  assert.deepEqual(errors,[]);
  await writeFile(`${root}/result.json`,JSON.stringify({nativeNoNode:true,axisConversion:true,embeddedChannels:true,collisionSelection:true,reports},null,2));
  console.log('Rust GLB passes Khronos validation and Three.js browser delivery.');
}finally {await browser?.close();await new Promise(resolve=>server.close(resolve));}
