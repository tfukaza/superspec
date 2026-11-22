import * as THREE from "three";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import type { ContourSource } from "./contours.ts";

/** A one-pixel catch on the sun-facing side of a physical corner. Color and material
 * normals never enter the edge test. Use the sun's actual depth comparison to keep
 * the catch off occluded surfaces, including across a shadow line on a flat wall. */
export class LitEdges extends ShaderPass {
  private readonly empty: THREE.WebGLRenderTarget;
  private readonly targetPosition = new THREE.Vector3();
  constructor(private readonly view: THREE.Camera, private readonly source: () => ContourSource,
    readonly sun: THREE.DirectionalLight) {
    super({
      uniforms: {
        tDiffuse: { value: null }, depth: { value: null }, normals: { value: null }, leaves: { value: null },
        hasLeaves: { value: false }, usable: { value: false }, sunDepth: { value: null },
        inverseProjection: { value: new THREE.Matrix4() }, toShadow: { value: new THREE.Matrix4() },
        lightDir: { value: new THREE.Vector3() }, lightColor: { value: new THREE.Color() },
        texel: { value: new THREE.Vector2() }, bias: { value: 0 },
      },
      vertexShader: "varying vec2 vUv; void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}",
      fragmentShader: `
      varying vec2 vUv;
      uniform sampler2D tDiffuse,depth,normals,leaves;
      uniform highp sampler2DShadow sunDepth;
      uniform mat4 inverseProjection,toShadow;
      uniform vec2 texel; uniform vec3 lightDir,lightColor;
      uniform float bias; uniform bool hasLeaves,usable;
      vec3 point(vec2 uv,float z){
        vec4 p=inverseProjection*vec4(uv*2.-1.,z*2.-1.,1.); return p.xyz/p.w;
      }
      float edge(vec2 off,vec3 p,vec3 n,float facing,float footprint){
        vec2 uv=vUv+off; float z=texture2D(depth,uv).x;
        if(z>=1.)return 1.;
        vec3 delta=point(uv,z)-p, other=normalize(texture2D(normals,uv).xyz*2.-1.);
        // A flat sloping face stays in its own tangent plane. Only the near surface
        // owns an occlusion edge. A convex crease belongs to its brighter face.
        float behind=-dot(delta,n);
        float lip=smoothstep(2.*footprint,4.*footprint,behind);
        float crease=smoothstep(.13,.38,1.-dot(n,other))*smoothstep(.04,.35,facing-dot(other,lightDir));
        return max(lip,crease*smoothstep(-.08*footprint,.2*footprint,behind));
      }
      void main(){
        vec4 c=texture2D(tDiffuse,vUv); gl_FragColor=c;
        float z=texture2D(depth,vUv).x;
        if(!usable || c.a<.99 || z>=1. || (hasLeaves && texture2D(leaves,vUv).a>.01))return;
        vec3 n=normalize(texture2D(normals,vUv).xyz*2.-1.),p=point(vUv,z);
        float facing=dot(n,lightDir); if(facing<.12)return;
        float footprint=max(.0001,length(point(vUv+vec2(texel.x,0),z)-p));
        // Integrate neighboring directions so a diagonal corner has fractional coverage.
        float rim=0.;
        for(int i=0;i<8;i++){
          float angle=float(i)*.78539816;
          vec2 offset=round(vec2(cos(angle),sin(angle)))*texel;
          rim+=edge(offset,p,n,facing,footprint);
        }
        rim=min(1.,rim*.24);
        if(rim<=0.)return;
        vec4 clip=toShadow*vec4(p,1.);vec3 sc=clip.xyz/clip.w;
        if(any(lessThan(sc,vec3(0))) || any(greaterThan(sc,vec3(1))))return;
        float lit=smoothstep(.8,1.,texture(sunDepth,vec3(sc.xy,sc.z+bias)));
        // Keep the catch below the material's own broad highlight, even on HDR metals.
        gl_FragColor.rgb+=min(c.rgb,vec3(1.))*lightColor*(.055*rim*lit*smoothstep(.18,.8,facing));
      }`,
    });
    const depth = new THREE.DepthTexture(1, 1, THREE.FloatType);
    depth.compareFunction = THREE.LessEqualCompare;
    this.empty = new THREE.WebGLRenderTarget(1, 1, { depthTexture: depth });
  }

  override render(renderer: THREE.WebGLRenderer, write: THREE.WebGLRenderTarget, read: THREE.WebGLRenderTarget, delta: number, masked: boolean): void {
    const g = this.source(), u = this.uniforms, s = this.sun, map = s.shadow.map?.depthTexture;
    u.depth.value = g.depth; u.normals.value = g.normal; u.leaves.value = g.foliage; u.hasLeaves.value = g.hasFoliage;
    u.usable.value = Boolean(s.visible && s.castShadow && renderer.shadowMap.enabled && map && map.compareFunction !== null);
    u.sunDepth.value = u.usable.value ? map : this.empty.depthTexture;
    u.texel.value.set(1/read.width, 1/read.height);
    u.inverseProjection.value.copy(this.view.projectionMatrixInverse);
    u.toShadow.value.multiplyMatrices(s.shadow.matrix, this.view.matrixWorld);
    s.getWorldPosition(u.lightDir.value).sub(s.target.getWorldPosition(this.targetPosition)).transformDirection(this.view.matrixWorldInverse);
    u.lightColor.value.copy(s.color).multiplyScalar(Math.min(1,s.intensity));
    u.bias.value = s.shadow.bias;
    super.render(renderer, write, read, delta, masked);
  }
  override dispose(): void { this.empty.dispose(); super.dispose(); }
}
