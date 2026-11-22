import * as THREE from "three";

/** WorldSpec irradiance in Z-up meters. Six RGB8 volumes hold +X,-X,+Y,-Y,+Z,-Z. */
export type BounceRecord = {
  version: 1;
  origin: [number, number, number];
  dimensions: [number, number, number];
  spacing: number;
  strokeMeters: number;
  bufferView: number;
};

export function probeTexture(record: BounceRecord, buffer: ArrayBuffer): THREE.Data3DTexture {
  const [x, y, z] = record.dimensions;
  if (
    record.dimensions.some((n) => !Number.isInteger(n) || n < 1 || n > 256) ||
    z * 6 > 256 ||
    !(record.spacing > 0) ||
    !Number.isFinite(record.spacing) ||
    !record.origin.every(Number.isFinite) ||
    !Number.isFinite(record.strokeMeters) ||
    record.strokeMeters < 0 ||
    buffer.byteLength !== 18 * x * y * z
  )
    throw Error("Invalid WorldSpec bounce volume");
  const texture = new THREE.Data3DTexture(new Uint8Array(buffer), x, y, 6 * z);
  texture.format = THREE.RGBFormat;
  texture.internalFormat = "RGB8";
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.unpackAlignment = 1;
  texture.needsUpdate = true;
  return texture;
}

export function applyBounce(
  material: THREE.MeshStandardMaterial,
  texture: THREE.Data3DTexture,
  record: BounceRecord,
  painted: boolean,
): void {
  if (painted && record.strokeMeters > 0) {
    material.defines = { ...material.defines, WS_PAINTED_BOUNCE: 1 };
    // Detailed baked finishes can supply their own surface marks under continuous probe lighting.
    if (material.userData.worldSpecConcrete === true || material.userData.worldSpecSmoothBounce === true) {
      material.defines.WS_SMOOTH_BOUNCE = 1;
    }
  }
  const opaquePaint = !material.userData.grassField && !material.transparent && material.alphaTest === 0;
  const previous = material.onBeforeCompile,
    key = material.customProgramCacheKey.bind(material);
  material.onBeforeCompile = function (shader, renderer) {
    previous.call(this, shader, renderer);
    shader.uniforms.wsBounce = { value: texture };
    shader.uniforms.wsGridOrigin = { value: new THREE.Vector3(...record.origin) };
    shader.uniforms.wsGridSize = { value: new THREE.Vector3(...record.dimensions) };
    shader.uniforms.wsProbeSpacing = { value: record.spacing };
    shader.uniforms.wsStroke = { value: painted ? record.strokeMeters : 0 };
    shader.vertexShader =
      "varying vec3 wsPosition;\n" +
      shader.vertexShader.replace(
        "#include <project_vertex>",
        `#include <project_vertex>
      vec3 wsGamePosition = cameraPosition + mvPosition.xyz * mat3(viewMatrix);
      wsPosition = vec3(wsGamePosition.x,-wsGamePosition.z,wsGamePosition.y);`,
      );
    shader.fragmentShader =
      `
      varying vec3 wsPosition;
      uniform highp sampler3D wsBounce;
      uniform vec3 wsGridOrigin, wsGridSize;
      uniform float wsProbeSpacing, wsStroke;

      vec3 wsLobe(vec3 uv, float lobe) {
        return 2.0 * textureLod(wsBounce, vec3(uv.xy,(uv.z+lobe)/6.0),0.0).rgb;
      }
      vec3 wsIrradiance(vec3 p, vec3 n) {
        vec3 index = (p-wsGridOrigin)/wsProbeSpacing;
        vec3 edge = min(index,wsGridSize-1.0-index);
        float fade = clamp(min(min(edge.x,edge.y),edge.z)+0.5,0.0,1.0);
        vec3 uv = (clamp(index,vec3(0.),wsGridSize-1.)+0.5)/wsGridSize;
        vec3 w = n*n;
        vec3 light = vec3(0.);
        if (w.x > 0.001) light += w.x*wsLobe(uv,step(n.x,0.));
        if (w.y > 0.001) light += w.y*wsLobe(uv,2.+step(n.y,0.));
        if (w.z > 0.001) light += w.z*wsLobe(uv,4.+step(n.z,0.));
        return fade*light;
      }
      vec3 wsPaintedBounce(vec3 p, vec3 n, out float pigment) {
        pigment = 0.;
        // Probe outside the receiver so walls sample the room on the face's side.
        vec3 samplePoint = p + n * (wsProbeSpacing*0.48);
        #if !defined(WS_PAINTED_BOUNCE) || defined(WS_SMOOTH_BOUNCE)
        return wsIrradiance(samplePoint,n);
        #else
        vec3 a = abs(n);
        vec3 axis = a.z >= max(a.x,a.y) ? vec3(1.,0.,0.) : vec3(0.,0.,1.);
        vec3 v = normalize(cross(n,axis)), u = cross(v,n);
        vec2 meters = vec2(dot(p,u),dot(p,v));
        vec2 size = wsStroke*vec2(1.,0.34);
        float row = floor(meters.y/size.y);
        vec2 brush = meters/size + vec2(0.43*row + 0.13*sin(1.8*row),0.);
        // Stagger adjacent rows of overlapping chisel strokes on the receiving plane.
        vec2 dabSize = size*0.82;
        vec2 paper = meters/dabSize;
        paper.y += 0.36*paper.x + 0.53*sin(paper.x*0.86+paper.y*0.17);
        paper.x += 0.27*sin(1.43*paper.y+0.35*paper.x);
        float footprint = max(fwidth(meters.x)/dabSize.x, fwidth(meters.y)/dabSize.y);
        float paintSum = 0., weightSum = 0.;
        float firstRow = floor(paper.y - .5);
        for (int j=0; j<2; ++j) {
          float y = firstRow + float(j);
          float column = paper.x + y*.618;
          float firstColumn = floor(column - .5);
          for (int i=0; i<2; ++i) {
            vec2 id = vec2(firstColumn + float(i), y);
            vec3 seed = fract(id.xyx*vec3(.1031,.1030,.0973));
            seed += dot(seed, seed.yzx+33.33);
            float variation = fract((seed.x+seed.y)*seed.z);
            vec2 dab = vec2(column,paper.y) - (id + .5);
            float bristles = .025*sin(dab.y*22.+variation*5.);
            float end = abs(dab.x + dab.y*(variation-.5)*.22 + bristles);
            float side = abs(dab.y + .025*sin(dab.x*6.+variation*4.));
            float weight = 1.-smoothstep(.42,.66,max(end,side));
            weightSum += weight;
            paintSum += weight*(variation-.5);
          }
        }
        pigment = .30*paintSum/max(weightSum,.001)*(1.-smoothstep(.16,.75,footprint));
        vec2 offset = (0.5-fract(brush))*size;
        vec3 center = samplePoint + u*offset.x + v*offset.y;
        // Tensor-product Gaussian quadrature of the trilinear RGB field over the stroke.
        vec3 du = u * size.x * 0.28867513, dv = v * size.y * 0.28867513;
        // Inside one trilinear cell a grid-aligned rectangular stroke has
        // exactly its centre's mean. Keep quadrature across knots and near
        // the volume's falloff, where that equality no longer holds.
        vec3 grid = (center - wsGridOrigin) / wsProbeSpacing;
        vec3 extent = (abs(du) + abs(dv)) / wsProbeSpacing;
        vec3 cell = fract(grid);
        if (max(max(a.x, a.y), a.z) > 0.999999 &&
            all(greaterThan(grid - extent, vec3(0.5))) &&
            all(lessThan(grid + extent, wsGridSize - 1.5)) &&
            all(lessThan(extent, min(cell, 1.0 - cell))))
          return wsIrradiance(center, n);
        return 0.25*(wsIrradiance(center-du-dv,n)+wsIrradiance(center+du-dv,n)
                    +wsIrradiance(center-du+dv,n)+wsIrradiance(center+du+dv,n));
        #endif
      }
    ` +
      shader.fragmentShader.replace(
        "#include <opaque_fragment>",
        `
      ${material.userData.grassField ? `// Blades and turf use one receiving normal, including for indirect light.
      vec3 wsGrowth = geometryNormal * mat3(viewMatrix);
      vec3 wsFace = vec3(wsGrowth.x, -wsGrowth.z, wsGrowth.y);` : `vec3 wsCross = cross(dFdx(wsPosition),dFdy(wsPosition));
      vec3 wsFace = wsCross * inversesqrt(max(dot(wsCross,wsCross),1e-20));`}
      // The sun-shadow illustration has already shaped the direct lighting. The
      // baked transport carries its own sun visibility at the reflecting surface.
      float wsPigment;
      vec3 wsIndirect = wsPaintedBounce(wsPosition,wsFace,wsPigment) * BRDF_Lambert(material.diffuseContribution);
      #ifdef USE_AOMAP
        wsIndirect *= ambientOcclusion;
      #endif
      outgoingLight += wsIndirect;
      #ifdef WS_PAINTED_BOUNCE
      ${opaquePaint ? `
        vec3 wsBody = max(outgoingLight-totalEmissiveRadiance,vec3(0.));
        float wsValue = dot(wsBody,vec3(.2126,.7152,.0722));
        float wsShade = 1.-smoothstep(.035,.25,wsValue);
        float wsMatte = material.roughness*(1.-metalnessFactor);
        // The street reflects muted warm light into soffits. Graphite and olive
        // underpainting keep recessed planes readable beside that bounce.
        vec3 wsGround = sqrt(max(material.diffuseContribution,vec3(0.)));
        float wsUp = smoothstep(.45,.95,wsFace.z);
        float wsDown = smoothstep(.25,.9,-wsFace.z);
        vec3 wsWash = wsGround*mix(vec3(.062,.055,.046),vec3(.040,.049,.047),wsShade);
        wsWash += wsGround * vec3(.052,.043,.030)*wsDown*wsShade;
        vec3 wsGlaze = wsBody*mix(vec3(1.13,1.07,.98),vec3(.98,1.03,1.),wsShade);
        float wsFoot = exp2(-max(wsPosition.z,0.)*.85)*(1.-abs(wsFace.z));
        wsGlaze *= mix(vec3(1.),vec3(.79,.84,.82),wsFoot);
        wsGlaze *= 1.-.29*wsUp*(1.-wsShade);
        vec3 wsMark = (wsGlaze+wsWash)*mix(vec3(.35,.28,.18),vec3(.31,.29,.25),wsShade)*wsPigment;
        outgoingLight += wsMatte*(wsGlaze-wsBody+wsWash+wsMark);
      ` : ""}
      #endif
      #include <opaque_fragment>`,
      );
  };
  material.customProgramCacheKey = () => key() + "|worldspec-bounce-v3-" + Number(opaquePaint);
  Object.assign(material, { worldSpecBounceTexture: texture });
}
