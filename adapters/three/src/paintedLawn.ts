import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";

type Slice = { byteOffset: number; byteLength: number };
type Bed = { layers: Slice[]; placements: { position: number[]; heading: number; scale: number }[] };
type Lawn = { version: number; buffer: number; atlas: Slice; beds: Bed[] };
const names = ["Fuzzy strokes", "Sunlit dashes", "Highlight blades"];
const stride = 13;

/** TreeSpec packs root pigment and the receiving normal alongside each seeded atlas brush. */
function brushGeometry(bytes: ArrayBuffer, range: Slice) {
  if (range.byteLength % (4 * stride)) throw Error("Invalid painted tuft layout");
  const data = new Float32Array(bytes, range.byteOffset, range.byteLength / 4);
  const attributes = new THREE.InstancedInterleavedBuffer(data, stride);
  const g = new THREE.InstancedBufferGeometry();
  g.setIndex([0,1,2,0,2,3]);
  g.setAttribute("position",new THREE.Float32BufferAttribute([-.5,0,0,.5,0,0,.5,1,0,-.5,1,0],3));
  g.setAttribute("normal",new THREE.Float32BufferAttribute([0,1,0,0,1,0,0,1,0,0,1,0],3));
  g.setAttribute("uv",new THREE.Float32BufferAttribute([0,0,1,0,1,1,0,1],2));
  for (const [name,components,offset] of [["lawnRoot",3,0],["lawnUp",3,3],["color",3,6],["lawnStroke",4,9]] as const)
    g.setAttribute(name,new THREE.InterleavedBufferAttribute(attributes,components,offset));
  g.instanceCount = data.length / stride;
  const box = new THREE.Box3(), v = new THREE.Vector3();
  let radius = 0;
  for (let i=0; i<data.length; i+=stride) {
    box.expandByPoint(v.fromArray(data,i));
    radius = Math.max(radius, data[i+9] + data[i+10]*.5);
  }
  g.boundingBox = box.expandByScalar(radius);
  g.boundingSphere = box.getBoundingSphere(new THREE.Sphere());
  return g;
}

export function stylePaintedStroke(material: THREE.MeshStandardMaterial, part: number) {
  const previous = material.onBeforeCompile, key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function(shader, renderer) {
    previous.call(this,shader,renderer);
    shader.vertexShader = `
      attribute vec3 lawnRoot, lawnUp;
      attribute vec4 lawnStroke;
      varying float lawnPick;
      varying float lawnDownView;
    `+shader.vertexShader
      .replace("#include <beginnormal_vertex>", `
        #include <beginnormal_vertex>
        vec3 eye = cameraPosition - (modelMatrix * vec4(lawnRoot,1.)).xyz;
        float away = max(length(eye), 0.0001);
        eye = normalize(eye * mat3(modelMatrix));
        float topView = smoothstep(.56,.94,abs(eye.y));
        lawnDownView = smoothstep(.65,.94,eye.y);
        float facing = dot(lawnUp, eye);
        objectNormal = normalize(lawnUp + eye * max(0.0, 0.35 - facing));`)
      .replace("#include <begin_vertex>", `
        float rank = fract(lawnStroke.w);
        float keep = lawnStroke.w >= 1.0 ? 1.0 - smoothstep(3.0, 6.0, away) : 1.0 - smoothstep(18.0,30.0,away);
        ${part === 2 ? "keep *= mix(0.45, 0.92, smoothstep(0.08,0.21,dot(color,vec3(.2126,.7152,.0722))));" : ""}
        keep *= mix(1.0, ${[0.44,0.80,0.26][part].toFixed(2)}, topView);
        ${part === 2 ? "keep += 0.24 * lawnDownView * (1.0-smoothstep(18.0,30.0,away));" : ""}
        vec3 grow = normalize(mix(lawnUp,vec3(0.,1.,0.),0.6));
        // At eye level each brush shows its silhouette. From above it
        // settles into its own seeded heading, avoiding rings about the eye.
        vec3 side = cross(vec3(0.,1.,0.), eye);
        float yaw = dot(side,side)<.000001 ? 0. : atan(side.z,side.x);
        float delta = rank*6.2831853-yaw;
        yaw += topView*atan(sin(delta),cos(delta));
        side = vec3(cos(yaw),0.,sin(yaw));
        float tilt = (fract(rank*17.31+.5)-.5)*${[0.35,1.6,0.9][part].toFixed(2)};
        grow = normalize(grow + side*tilt);
        ${part === 1 ? "grow = normalize(mix(grow, cross(side,vec3(0.,1.,0.)),.25+.55*topView));" : ""}
        ${part === 2 ? "grow = normalize(grow + cross(side,vec3(0.,1.,0.)) * (0.32 + 0.23*rank) * lawnDownView);" : ""}
        float mirror = fract(rank*91.7+.37) < .5 ? -1. : 1.;
        vec3 transformed = lawnRoot + (rank <= keep ? 1. : 0.) *
          (side * position.x * lawnStroke.y * mirror * ${[1.0,1.18,0.83][part].toFixed(2)} + grow * (position.y-${part === 0 ? "0.08" : "0.05"}) * lawnStroke.x * ${[1,0.65,0.96][part].toFixed(2)});
        float hue = fract(rank*53.7+.21);
        lawnPick = fract(hue*7.13+.3);
      `).replace("#include <uv_vertex>", `
        #include <uv_vertex>
        vMapUv = (vec2(mod(lawnStroke.z,4.), floor(lawnStroke.z/4.)) + clamp(uv,vec2(0.5/128.0),vec2(1.0-0.5/128.0))) / vec2(4.,6.);
      `);
    shader.fragmentShader = "varying float lawnPick; varying float lawnDownView;\n" + shader.fragmentShader
      .replace("#include <normal_fragment_begin>", THREE.ShaderChunk.normal_fragment_begin.replace("normal *= faceDirection;", ""))
      .replace("#include <map_fragment>", `
        vec4 stamp = texture2D(map, vMapUv);
        diffuseColor.a *= ${["smoothstep(0.,.85,stamp.a)","smoothstep(.02,.60,stamp.a)","smoothstep(.30,.60,stamp.a)"][part]};
        ${part === 0 ? "diffuseColor.a *= smoothstep(.025,.21,fract(vMapUv.y*6.));" : ""}
        diffuseColor.rgb *= mix(.94,1.06,stamp.r);
      `).replace("#include <opaque_fragment>", `
        ${part === 1 ? "outgoingLight *= mix(vec3(1.8),vec3(3.1,3.0,1.3),cityLawnBand);" :
          part === 2 ? "float lift = lawnPick < .7 ? mix(1.1,1.35,lawnPick/.7) : mix(1.7,2.3,(lawnPick-.7)/.3); outgoingLight *= mix(1.,1.+(lift-1.)*(.7+.3*cityLawnBand),smoothstep(.25,1.,stamp.g));" : ""}
        ${part === 2 ? `
          // Keep the contact pigment. Cool mid-blades alternate with warm lit
          // tips so an overhead view resolves strokes across the mottled turf.
          float upperBlade = smoothstep(.18,.85,stamp.g) * lawnDownView;
          vec3 ink = mix(vec3(.42,.62,.64), vec3(1.57,1.32,.72),
                         smoothstep(.48,.82,lawnPick) * (.55+.45*cityLawnBand));
          outgoingLight *= mix(vec3(1.), ink, upperBlade);
        ` : ""}
        #include <opaque_fragment>`);
  };
  material.customProgramCacheKey = () => key()+"|painted-stroke-"+part;
}

/** Keep ordinary readers' static tuft geometry as a fallback if the companion fails. */
export async function installPaintedLawns(gltf: GLTF) {
  const root = gltf.scene, record = root.userData.paintedLawn as Lawn | undefined;
  if (record?.version !== 1) return;
  const bytes: ArrayBuffer = await gltf.parser.getDependency("buffer",record.buffer);
  const png = new Uint8Array(bytes,record.atlas.byteOffset,record.atlas.byteLength);
  const url = URL.createObjectURL(new Blob([png],{type:"image/png"}));
  let texture: THREE.Texture;
  try { texture = await new THREE.TextureLoader().loadAsync(url); } finally { URL.revokeObjectURL(url); }
  texture.flipY = false;
  texture.minFilter = THREE.LinearFilter;
  texture.generateMipmaps = false;
  const group = new THREE.Group();
  const geometries: THREE.BufferGeometry[] = [];
  const materials = names.map((name,i)=>{
    const m = new THREE.MeshStandardMaterial({map:texture, vertexColors:true, roughness:.96,
      side:THREE.DoubleSide, transparent:true, depthWrite:false});
    m.forceSinglePass = true;
    m.name = name;
    m.userData = {grassField:true, paintedStroke:i, castsShadow:false};
    return m;
  });
  try {
    for (const bed of record.beds) {
      if (bed.layers.length !== 3) throw Error("Missing painted tuft layer");
      bed.layers.forEach((range,i)=>{
        const g = brushGeometry(bytes,range); geometries.push(g);
        for(const {position:[x,y,z],heading,scale} of bed.placements) {
          const mesh = new THREE.Mesh(g, materials[i]);
          mesh.name = names[i];
          mesh.position.set(x,z,-y);
          mesh.rotation.y = THREE.MathUtils.degToRad(heading);
          mesh.scale.setScalar(scale);
          mesh.receiveShadow = true;
          mesh.renderOrder = 3+i;
          group.add(mesh);
        }
      });
    }
  } catch (error) {
    for (const g of geometries) g.dispose();
    for (const m of materials) m.dispose();
    texture.dispose();
    throw error;
  }
  root.traverse(m=>{
    if(m instanceof THREE.Mesh && m.material?.userData?.grassField && m.material.map) m.visible=false;
  });
  root.add(group);
}
