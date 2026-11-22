import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import sharp from "sharp";
import { Document, NodeIO } from "@gltf-transform/core";
import { appendDeclaredLod, declaredLodBytes, readDeclaredLod } from "./declared-lod-pack.mjs";
import { readPrototypeJson } from "./prototype-json.mjs";
import { entryBytes, readCityEntry } from "./city-data-textures.mjs";
import { attachCityLod, entryBytes as jsonEntry } from "./city-lod.mjs";
import { reflectors } from "./bounced-light.mjs";
import { finishStreetTreeLods } from "../../crates/engine/src/worldspec/themes/kowloon/authoring/street-trees.mjs";

async function fixture() {
  const pixels = await sharp({create:{width:1,height:1,channels:4,background:{r:160,g:180,b:200,alpha:1}}}).png().toBuffer();
  const positions = Buffer.from(Float32Array.from([0,0,0,1,0,0,0,1,0]).buffer), indices = Buffer.from(Uint32Array.from([0,1,2]).buffer);
  const binary=Buffer.concat([positions,indices,pixels]), tail=Buffer.alloc(8+Math.ceil(binary.length/4)*4);
  tail.writeUInt32LE(tail.length-8,0);tail.writeUInt32LE(0x004e4942,4);binary.copy(tail,8);
  const json={asset:{version:"2.0"},scene:0,scenes:[{nodes:[0]}],
    extensionsUsed:["MSFT_lod","KHR_lights_punctual"],extensions:{KHR_lights_punctual:{lights:[{type:"directional"}]}},
    nodes:[{rotation:[-Math.SQRT1_2,0,0,Math.SQRT1_2],children:[1,2]},
      {name:"near",children:[4],extensions:{MSFT_lod:{ids:[3]}},extras:{worldSpecLod:{version:1,distances:[0,30]}}},
      {extensions:{KHR_lights_punctual:{light:0}}},{name:"far",children:[5]},
      {mesh:0,extras:{objectId:"item",collidable:true}},{mesh:1,extras:{objectId:"item",collidable:false}}],
    meshes:[0,1].map(material=>({name:"building-12",primitives:[{attributes:{POSITION:0},indices:1,material}]})),
    materials:[{name:"near-paint",pbrMetallicRoughness:{baseColorTexture:{index:0}},extensions:{MSFT_lod:{ids:[1]}},extras:{MSFT_screencoverage:[0.1,0]}},
      {name:"far-paint",pbrMetallicRoughness:{baseColorTexture:{index:0}},extras:{materialSpec:{version:1,live:[{id:"test",arguments:{mask:{texture:0,channel:0}}}]}}}],
    textures:[{source:0,sampler:0}],samplers:[{}],images:[{mimeType:"image/png",bufferView:2}],
    accessors:[{bufferView:0,componentType:5126,type:"VEC3",count:3,min:[0,0,0],max:[1,1,0]}, {bufferView:1,componentType:5125,type:"SCALAR",count:3}],
    bufferViews:[{buffer:0,byteOffset:0,byteLength:positions.length},{buffer:0,byteOffset:positions.length,byteLength:indices.length},
      {buffer:0,byteOffset:positions.length+indices.length,byteLength:pixels.length}],buffers:[{byteLength:binary.length}]};
  return {json,binary,bytes:entryBytes(json,tail)};
}
const target = () => ({asset:{version:"2.0"},scene:0,scenes:[{nodes:[],extras:{}}],nodes:[],meshes:[],materials:[],textures:[],samplers:[],images:[],accessors:[],bufferViews:[],buffers:[]});
function reachable(json) {
  const nodes=[];function visit(id){assert.ok(!nodes.includes(id));nodes.push(id);for(const child of json.nodes[id].children??[])visit(child)}
  for(const root of json.scenes[0].nodes)visit(root);return nodes;
}

test("declared native graphs survive placement with exact shared payloads and optional material chains", async()=>{
  const directory=await mkdtemp(join(tmpdir(),"superspec-authored-lod-"));
  try {
    const input=await fixture(), sourceFile=join(directory,"input.glb");await writeFile(sourceFile,input.bytes);
    const source=await readDeclaredLod(sourceFile),json=target();
    // Nonzero table offsets expose incorrect index copying rather than merely comparing zero-based sources.
    for(const key of ["materials","textures","images","samplers","accessors","bufferViews","buffers","meshes"])json[key].push({});
    await appendDeclaredLod(json,source,{positions:[{position:[4,5,6],heading:90},{position:[8,9,10],heading:0,scale:2}],name:"item",resourceDir:join(directory,"data"),folder:"data"});
    const active=reachable(json),near=active.filter(i=>json.nodes[i].extensions?.MSFT_lod);
    assert.equal(near.length,2);assert.equal(active.filter(i=>json.nodes[i].mesh!==undefined).length,2);
    assert.notEqual(json.nodes[near[0]].extensions.MSFT_lod.ids[0],json.nodes[near[1]].extensions.MSFT_lod.ids[0]);
    assert.ok(near.every(i=>!active.includes(json.nodes[i].extensions.MSFT_lod.ids[0])));
    assert.equal(json.meshes.length,3);assert.equal(json.materials.length,3);
    assert.equal(json.materials[1].extensions.MSFT_lod.ids[0],2);
    assert.equal(json.materials[1].pbrMetallicRoughness.baseColorTexture.index,1);
    assert.equal(json.materials[2].extras.materialSpec.live[0].arguments.mask.texture,1);
    assert.deepEqual(json.nodes[json.scenes[0].nodes[0]].translation,[4,6,-5]);
    assert.ok(json.nodes.every(n=>!n.extensions?.KHR_lights_punctual));
    assert.ok(!json.extensionsRequired?.includes("MSFT_lod"));
    assert.deepEqual(await readFile(join(directory,json.buffers[1].uri)),input.binary);
    assert.deepEqual(await readFile(sourceFile),input.bytes);
    const again=target();await appendDeclaredLod(again,source,{positions:[{position:[0,0,0],heading:0}],name:"again",resourceDir:join(directory,"data"),folder:"data"});
    assert.equal((await readdir(join(directory,"data"))).length,1);
  } finally {await rm(directory,{recursive:true,force:true})}
});

test("GI sees only the default authored tier and automatic city streams skip complete authored graphs",async()=>{
  const directory=await mkdtemp(join(tmpdir(),"superspec-authored-gi-"));
  try {
    const source=declaredLodBytes((await fixture()).bytes), json=target();
    // These color-only materials exercise actual reflector traversal without an unused UV accessor.
    const parsed=readCityEntry(source.bytes);for(const material of parsed.json.materials)delete material.pbrMetallicRoughness.baseColorTexture;
    const colorSource=declaredLodBytes(entryBytes(parsed.json,parsed.tail));
    await appendDeclaredLod(json,colorSource,{positions:[{position:[0,0,0],heading:0},{position:[10,0,0],heading:0}],name:"item",resourceDir:join(directory,"city-data"),folder:"city-data"});
    const surfaces=await reflectors(json,directory,0.01);assert.equal(surfaces.length,2);
    const packed=await attachCityLod(join(directory,"city.glb"),jsonEntry(json),{workers:1});
    const document=readCityEntry(packed).json,sidecar=await readFile(join(directory,document.scenes[0].extras.cityLod));
    const manifest=JSON.parse(sidecar.toString("utf8",12,12+sidecar.readUInt32LE(8)));
    assert.deepEqual(manifest.primitives,[]);
  }finally{await rm(directory,{recursive:true,force:true})}
});

test("unsupported inputs and changed source fail; immutable content corruption cannot be overwritten",async()=>{
  const directory=await mkdtemp(join(tmpdir(),"superspec-authored-failure-"));
  try {
    const original=await fixture(),file=join(directory,"input.glb");await writeFile(file,original.bytes);
    for(const mutate of [json=>json.extensionsUsed.push("KHR_texture_basisu"),json=>json.nodes[1].extensions.MSFT_lod.ids=[1],
      json=>json.scenes[0].nodes.push(3),json=>json.nodes[3].children=[1],json=>json.accessors[0].bufferView=999,
      json=>json.buffers[0].uri="external.bin"]) {
      const json=structuredClone(original.json);mutate(json);assert.throws(()=>declaredLodBytes(entryBytes(json,readCityEntry(original.bytes).tail)),/Authored LOD publication/);
    }
    const source=await readDeclaredLod(file);await writeFile(file,Buffer.concat([original.bytes,Buffer.from([0])]));
    const options={positions:[{position:[0,0,0],heading:0}],name:"item",resourceDir:join(directory,"data"),folder:"data"};
    await assert.rejects(appendDeclaredLod(target(),source,options),/changed/);
    const memory=declaredLodBytes(original.bytes),json=target();await appendDeclaredLod(json,memory,options);
    await writeFile(join(directory,json.buffers[0].uri),Buffer.from("corrupt"));
    await assert.rejects(appendDeclaredLod(target(),memory,options),/different staged companion/);
    assert.deepEqual((await readdir(join(directory,"data"))).filter(name=>name.endsWith(".tmp")),[]);
  }finally{await rm(directory,{recursive:true,force:true})}
});

test("painted tree graph resolves its authored lower bark node without changing the near mesh",async()=>{
  const doc=new Document(),scene=doc.createScene();doc.getRoot().setDefaultScene(scene);
  const near=doc.createNode("tree").setExtras({worldSpecPendingTreeLod:"tree/far",worldSpecLod:{version:1,distances:[0,45]}});
  scene.addChild(near);doc.createNode("tree/far");
  const io=new NodeIO(),original=await io.writeBinary(doc),finished=finishStreetTreeLods(original),json=readCityEntry(Buffer.from(finished)).json;
  assert.deepEqual(json.nodes[0].extensions.MSFT_lod.ids,[1]);assert.deepEqual(json.scenes[0].nodes,[0]);
  assert.deepEqual(json.nodes[0].extras.worldSpecLod.distances,[0,45]);assert.ok(!json.nodes[0].extras.worldSpecPendingTreeLod);
  assert.ok(declaredLodBytes(finished));
});
test("metadata-only prototype reads reject inconsistent file or chunk lengths", async t => {
  const root = await mkdtemp(join(tmpdir(), "prototype-header-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const path = join(root, "prototype.glb");
  const header = Buffer.alloc(24);
  header.write("glTF"); header.writeUInt32LE(2, 4); header.writeUInt32LE(24, 8);
  header.writeUInt32LE(4, 12); header.write("JSON{}  ", 16);
  await writeFile(path, header);
  assert.deepEqual(await readPrototypeJson(path), {});
  await assert.rejects(readPrototypeJson(path, 3), /JSON size/);
  for (const [index, value, error] of [[8, 23, /Invalid prototype GLB/], [12, 8, /JSON size/], [12, 3, /JSON size/]]) {
    const bad = Buffer.from(header); bad.writeUInt32LE(value, index);
    await writeFile(path, bad);
    await assert.rejects(readPrototypeJson(path), error);
  }
  await writeFile(path, header.subarray(0, 16));
  await assert.rejects(readPrototypeJson(path), /Truncated/);
});
