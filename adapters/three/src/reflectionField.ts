import * as THREE from "three";
import { worktopLampShader } from "./worktopLamp.ts";
import { reflectionPanorama } from "./paintedMetalSky.ts";

type Triple = [number, number, number];
type Box = {
  id: string;
  min: Triple;
  max: Triple;
  center: Triple;
  fade: number;
  rgb: string;
};
type RecordV1 = {
  version: number;
  encoding?: string;
  width: number;
  height: number;
  probes: Box[];
};
export type ReflectionField = {
  texture: THREE.DataArrayTexture;
  centers: THREE.Vector3[];
  low: THREE.Vector4[];
  high: THREE.Vector3[];
  ids: string[];
};
export type MetalFinish = {
  coatingResponse?: number;
  edgeDiffuse: number;
  edgeSpecular: number;
  normalPull: number;
  alignmentThreshold: number;
  alignmentFeather: number;
  edgeWidth: number;
  filterMix: number;
  brushThreshold: number;
  brushWidth: number;
  brushBrightness: number;
  brushScale: number;
  skyRed: number;
  skyGreen: number;
  skyBlue: number;
  reflection: number;
};
const vector = (v: unknown): v is Triple =>
  Array.isArray(v) && v.length === 3 && v.every(Number.isFinite);

export function createReflectionField(
  record?: RecordV1,
): ReflectionField | undefined {
  if (!record || record.version !== 1) return;
  const { width, height, probes } = record;
  if (
    ![width, height].every((v) => Number.isInteger(v) && v > 0 && v <= 256) ||
    !Array.isArray(probes) ||
    !probes.length ||
    probes.length > 8
  )
    return;
  const pixels = new Uint8Array(width * height * 4 * probes.length);
  const centers: THREE.Vector3[] = [],
    low: THREE.Vector4[] = [],
    high: THREE.Vector3[] = [];
  for (const [i, box] of probes.entries()) {
    if (
      !box ||
      ![box.center, box.min, box.max].every(vector) ||
      !Number.isFinite(box.fade) ||
      box.fade <= 0 ||
      box.min.some(
        (v, k) => v >= box.center[k] || box.center[k] >= box.max[k],
      ) ||
      typeof box.rgb !== "string" ||
      box.rgb.length !== 4 * Math.ceil((width * height * 3) / 3)
    )
      return;
    let raw: string;
    try {
      raw = atob(box.rgb);
    } catch {
      return;
    }
    if (raw.length !== 3 * width * height) return;
    if (record.encoding === "kuwahara-linear-rgb8") {
      for (let p = 0; p < width * height; p++) {
        const out = 4 * (i * width * height + p);
        pixels[out] = raw.charCodeAt(3 * p);
        pixels[out + 1] = raw.charCodeAt(3 * p + 1);
        pixels[out + 2] = raw.charCodeAt(3 * p + 2);
        pixels[out + 3] = 255;
      }
    } else {
      const source = Float32Array.from(raw, (c) => linear[c.charCodeAt(0)]);
      const filtered = reflectionPanorama(source, width, height, box.id);
      pixels.set(filtered.image.data as Uint8Array, i * width * height * 4);
      filtered.dispose();
    }
    centers.push(new THREE.Vector3(...box.center));
    low.push(new THREE.Vector4(...box.min, box.fade));
    high.push(new THREE.Vector3(...box.max));
  }
  const texture = new THREE.DataArrayTexture(
    pixels,
    width,
    height,
    probes.length,
  );
  texture.wrapS = THREE.RepeatWrapping;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.generateMipmaps = true;
  texture.needsUpdate = true;
  texture.name = "Stoa painted reflection probes";
  return { texture, centers, low, high, ids: probes.map((p) => p.id) };
}
const linear = Float32Array.from(
  { length: 256 },
  (_, i) =>
    THREE.ColorManagement.colorSpaceToWorking(
      new THREE.Color(i / 255, 0, 0),
      THREE.SRGBColorSpace,
    ).r,
);

export function applyReflectionField(
  material: THREE.MeshStandardMaterial,
  field: ReflectionField,
  strength: number,
  masonry: boolean,
) {
  const previous = material.onBeforeCompile,
    key = material.customProgramCacheKey.bind(material);
  const count = field.centers.length;
  const bench =
    material.name === "builtin:kitchen-worktop"
      ? field.ids.indexOf("duck-worktop")
      : -1;
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    shader.uniforms.wsProbeAtlas = { value: field.texture };
    shader.uniforms.wsProbeCenters = { value: field.centers };
    shader.uniforms.wsProbeLow = { value: field.low };
    shader.uniforms.wsProbeHigh = { value: field.high };
    shader.uniforms.wsProbeStrength = { value: strength };
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <common>",
      `#include <common>
      ${bench >= 0 ? worktopLampShader : ""}
      uniform highp sampler2DArray wsProbeAtlas;
      uniform vec3 wsProbeCenters[${count}], wsProbeHigh[${count}];
      uniform vec4 wsProbeLow[${count}];
      uniform float wsProbeStrength;
      vec3 wsBoxSample(int i,vec3 p,vec3 ray,float lod) {
        vec3 signRay=mix(vec3(-1.),vec3(1.),step(vec3(0.),ray));
        vec3 wall=mix(wsProbeLow[i].xyz,wsProbeHigh[i],step(vec3(0.),ray));
        vec3 dist=(wall-p)/(signRay*max(abs(ray),vec3(.00001)));
        vec3 hit=p+ray*min(dist.x,min(dist.y,dist.z))-wsProbeCenters[i];
        return textureLod(wsProbeAtlas,vec3(equirectUv(normalize(hit)),float(i)),lod).rgb;
      }`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <aomap_fragment>",
      `#include <aomap_fragment>
      {
        vec3 p=cameraPosition-vViewPosition*mat3(viewMatrix);
        vec3 v=isOrthographic?vec3(0.,0.,1.):normalize(vViewPosition);
        vec3 ray=transformDirectionByInverseViewMatrix(reflect(-v,normal),viewMatrix);
        float weight=${masonry ? "0.095" : "smoothstep(.055,.5,material.metalness)"}*wsProbeStrength*(1.-material.roughness);
        if(weight>.0001) {
          int nearest=-1, outer=-1; float blend=0., outerBlend=0.;
          for(int i=0;i<${count};i++) {
            vec3 d=min(p-wsProbeLow[i].xyz,wsProbeHigh[i]-p);
            float fade=smoothstep(0.,wsProbeLow[i].w,min(d.x,min(d.y,d.z)));
            if(fade>0.) { outer=nearest; outerBlend=blend; nearest=i; blend=fade; }
          }
          if(nearest>=0) {
            float lod=.25+4.*material.roughness*material.roughness;
            vec3 radiance=blend*wsBoxSample(nearest,p,ray,lod);
            if(outer>=0 && blend<1.) radiance+=(1.-blend)*outerBlend*wsBoxSample(outer,p,ray,lod);
            ${
              bench >= 0
                ? `if(nearest==${bench}) {
              float upward=smoothstep(.45,.95,dot(normal,viewMatrix[1].xyz));
              vec3 stroke=msWorktopLamp(radiance*1.35,p,ray);
              radiance=mix(radiance,vec3(.035)+1.55*stroke,blend*upward);
            }`
                : ""
            }
            float facing=1.-abs(dot(normal,v));
            reflectedLight.indirectSpecular+=weight*radiance*mix(material.specularColorBlended,vec3(1.),.32*facing);
          }
        }
      }`,
    );
  };
  material.customProgramCacheKey = () =>
    key() + "|reflection-boxes-" + count + "-" + Number(masonry) + "-" + bench;
}
