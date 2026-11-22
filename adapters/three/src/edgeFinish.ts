import * as THREE from "three";
import type { MetalFinish } from "./reflectionField";

export type EdgeWear = {
  chipDepth: number;
  chipCoverage: number;
  chipScale: number;
  chipPolish: number;
  chipDiffuse: number;
};

// One repeatable 32 KiB volume supplies both brush breakup and chip pockets.
let noiseTexture: THREE.Data3DTexture | undefined;
function brushNoise(): THREE.Data3DTexture {
  if (noiseTexture) return noiseTexture;
  const size = 32, bytes = new Uint8Array(size ** 3);
  let state = 0x53746f61;
  for (let i = 0; i < bytes.length; i++) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    bytes[i] = state >>> 24;
  }
  const texture = new THREE.Data3DTexture(bytes,size,size,size);
  texture.name = "Painted edge breakup";
  texture.format = THREE.RedFormat;
  texture.wrapS = texture.wrapT = texture.wrapR = THREE.RepeatWrapping;
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  return noiseTexture = texture;
}

const wearShader = /* glsl */ `
  uniform highp sampler3D wsBrushNoise;
  float wsNoise(vec3 p) {
    vec3 f=fract(p);
    return texture(wsBrushNoise,(floor(p)+f*f*(3.-2.*f)+.5)/32.).r;
  }
`;
export function applyEdgeFinish(
  material: THREE.MeshStandardMaterial,
  finish: MetalFinish | undefined,
  wear: EdgeWear,
  detail?: THREE.Texture,
) {
  const previous = material.onBeforeCompile,
    key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    shader.uniforms.wsBrushNoise = {value:brushNoise()};
    shader.uniforms.wsChip = {
      value: new THREE.Vector4(
        wear.chipDepth,
        wear.chipCoverage,
        wear.chipScale,
        wear.chipPolish,
      ),
    };
    shader.uniforms.wsChipPigment = { value: wear.chipDiffuse };
    if (detail) shader.uniforms.wsFinishDetail = { value: detail };
    if (finish) {
      shader.uniforms.wsCoatingResponse = { value: finish.coatingResponse ?? 0 };
      shader.uniforms.wsFilterMix = { value: finish.filterMix };
      shader.uniforms.wsMetalEdge = {
        value: new THREE.Vector4(
          finish.edgeDiffuse,
          finish.edgeSpecular,
          finish.normalPull,
          finish.edgeWidth,
        ),
      };
      shader.uniforms.wsAlign = {
        value: new THREE.Vector2(
          finish.alignmentThreshold,
          finish.alignmentFeather,
        ),
      };
      shader.uniforms.wsBrush = {
        value: new THREE.Vector4(
          finish.brushThreshold,
          finish.brushWidth,
          finish.brushBrightness,
          finish.brushScale,
        ),
      };
    }
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <common>",
      `#include <common>
      uniform sampler2D wsFinishDetail; uniform float wsFilterMix, wsCoatingResponse;
      uniform vec4 wsChip,wsMetalEdge,wsBrush; uniform vec2 wsAlign; uniform float wsChipPigment;
      ${wearShader}`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <normal_fragment_maps>",
      `#include <normal_fragment_maps>
      float wsMask=0.;
      ${
        finish
          ? `#ifdef USE_METALNESSMAP
        ${
          detail
            ? `vec4 wsDetail=texture2D(wsFinishDetail,vMetalnessMapUv);
        wsMask=wsDetail.a;
        diffuseColor.rgb*=exp2((wsDetail.rgb*255.-128.)*(2.2/64.)*wsFilterMix*smoothstep(.055,.42,metalnessFactor));`
            : ""
        }
        #ifdef USE_AOMAP
          ${material.userData.materialSpecEdge ? "wsMask=max(wsMask,texture2D(aoMap,vAoMapUv).g);" : ""}
        #endif
      #else
        // Untextured cast metal uses its modelled bevels and grazing rim.
        wsMask=max(smoothstep(.015,.18,length(fwidth(nonPerturbedNormal))),1.-smoothstep(.025,.17,abs(dot(normal,normalize(vViewPosition)))));
      #endif`
          : `#ifdef USE_AOMAP
        wsMask=texture2D(aoMap,vAoMapUv).g;
      #endif`
      }
      vec3 wsP=cameraPosition-vViewPosition*mat3(viewMatrix);
      vec3 wsDpX=dFdx(-vViewPosition),wsDpY=dFdy(-vViewPosition);
      float wsPixel=max(length(wsDpX),length(wsDpY));
      float wsClean=${finish ? "max(wsCoatingResponse,smoothstep(.055,.42,metalnessFactor))" : "1."};
      float wsFreq=1./max(.001,wsChip.z);
      float wsGrain=wsNoise(wsP*wsFreq);
      float wsPocket=smoothstep(1.-wsChip.y,1.12-wsChip.y,wsGrain);
      float wsChipMask=wsMask*wsClean*wsPocket*(1.-smoothstep(.3,1.,wsPixel*wsFreq));
      float wsHeight=-wsChip.x*wsChipMask;
      vec3 wsCrossX=cross(wsDpY,normal), wsCrossY=cross(normal,wsDpX);
      float wsDet=dot(wsDpX,wsCrossX);
      vec3 wsGrad=sign(wsDet)*(dFdx(wsHeight)*wsCrossX+dFdy(wsHeight)*wsCrossY)/max(abs(wsDet),1e-9);
      normal=normalize(normal-wsGrad/max(1.,length(wsGrad)*2.));
      roughnessFactor=clamp(roughnessFactor-wsChip.w*wsChipMask,.08,1.);
      diffuseColor.rgb*=1.+wsChipPigment*wsChipMask;
      ${
        finish
          ? `float wsEdge=0.;
      #if NUM_DIR_LIGHTS > 0
        vec3 wsEye=isOrthographic?vec3(0.,0.,1.):normalize(vViewPosition);
        vec3 wsSun=directionalLights[0].direction,wsSum=wsSun+wsEye;
        vec3 wsHalf=wsSum*inversesqrt(max(1e-8,dot(wsSum,wsSum)));
        float wsAlignment=dot(normal,wsHalf);
        float wsEligible=smoothstep(wsAlign.x,wsAlign.x+wsAlign.y,wsAlignment);
        float wsShadow=1.;
        #if defined(USE_SHADOWMAP) && NUM_DIR_LIGHT_SHADOWS > 0
          if(receiveShadow && wsMask*wsClean>.001) {
            DirectionalLightShadow sh=directionalLightShadows[0];
            wsShadow=getShadow(directionalShadowMap[0],sh.shadowMapSize,1.,sh.shadowBias,sh.shadowRadius,vDirectionalShadowCoord[0]);
          }
        #endif
        float wsLight=wsShadow*max(dot(normal,wsSun),0.)*dot(directionalLights[0].color,vec3(.2126,.7152,.0722));
        wsLight*=.55+.45*sqrt(max(0.,dot(diffuseColor.rgb,vec3(.2126,.7152,.0722))));
        float wsDark=1.-smoothstep(.75*wsBrush.x,max(.0001,wsBrush.x),wsLight);
        float wsStrokeNoise=(wsNoise(wsP/max(.001,wsBrush.w))-.5)*2.;
        float wsVariation=wsDark*wsStrokeNoise*(1.-smoothstep(.3,1.,wsPixel/max(.001,wsBrush.w)));
        wsEdge=pow(wsMask,1./max(.1,wsMetalEdge.w*(1.+wsBrush.y*wsVariation)))*wsClean;
        wsEdge*=1.+wsBrush.z*wsVariation;
        diffuseColor.rgb*=1.+wsMetalEdge.x*wsEdge;
        vec3 wsSpecN=normalize(mix(normal,wsHalf,wsEligible*wsMetalEdge.z*min(1.,wsEdge)));
        float wsHighlight=pow(max(0.,dot(reflect(-wsSun,wsSpecN),wsEye)),mix(36.,7.,roughnessFactor));
      #endif`
          : ""
      }`,
    );
    if (finish)
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <lights_fragment_end>",
        `#include <lights_fragment_end>
      #if NUM_DIR_LIGHTS > 0
        reflectedLight.directSpecular+=directionalLights[0].color*material.specularColorBlended
          *wsShadow*wsEdge*wsMetalEdge.y*wsHighlight*smoothstep(0.,.3,dot(normal,wsSun));
      #endif`,
      );
  };
  material.customProgramCacheKey = () =>
    key() +
    "|chipped-shoulder-" +
    [!!finish, !!detail, !!material.userData.materialSpecEdge]
      .map(Number)
      .join("-");
}
