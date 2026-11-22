import * as THREE from "three";
import { ShaderPass } from "three/examples/jsm/postprocessing/ShaderPass.js";
import type { LiveOperation } from "../model/packet.ts";

export function usesKuwahara(material: THREE.Material): boolean {
  const live = material.userData.liveDescriptors as LiveOperation[] | undefined;
  return !!live?.some(operation => operation.id === "kuwahara");
}

const FILTER = {
  uniforms: { tDiffuse: { value: null }, tMask: { value: null }, texel: { value: new THREE.Vector2() } },
  vertexShader: `
    varying vec2 vUv;
    void main() {
      vUv = uv;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    }`,
  fragmentShader: `
    uniform sampler2D tDiffuse;
    uniform sampler2D tMask;
    uniform vec2 texel;
    varying vec2 vUv;

    void main() {
      vec4 center = texture2D(tDiffuse, vUv);
      float id = texture2D(tMask, vUv).r;
      gl_FragColor = center;
      if (id < 0.001 || center.a < 0.999) return;

      vec3 best = center.rgb;
      float lowest = 1e20;
      for (int region = 0; region < 4; ++region) {
        vec2 direction = vec2(region % 2 == 0 ? -1.0 : 1.0, region < 2 ? -1.0 : 1.0);
        vec3 sum = vec3(0.0);
        vec3 squareSum = vec3(0.0);
        float count = 0.0;
        for (int y = 0; y < 3; ++y) {
          for (int x = 0; x < 3; ++x) {
            vec2 at = vUv + direction * vec2(float(x), float(y)) * texel;
            if (abs(texture2D(tMask, at).r - id) > 0.001) continue;
            vec4 sampleColor = texture2D(tDiffuse, at);
            if (sampleColor.a < 0.999) continue;
            sum += sampleColor.rgb;
            squareSum += sampleColor.rgb * sampleColor.rgb;
            count += 1.0;
          }
        }
        // A quadrant cut off by a silhouette cannot win on a single clean sample.
        if (count < 4.0) continue;
        vec3 mean = sum / count;
        vec3 variance = max(squareSum / count - mean * mean, vec3(0.0));
        float score = dot(variance, vec3(0.2126, 0.7152, 0.0722));
        if (score < lowest) {
          lowest = score;
          best = mean;
        }
      }
      gl_FragColor.rgb = mix(center.rgb, best, 0.82);
    }`,
};

/** Filter shaded color before tone mapping. A depth-tested material ID mask
 * keeps the kernel inside the visible receiver, including around cutouts. */
export class KuwaharaPass extends ShaderPass {
  private readonly mask = new THREE.WebGLRenderTarget(1, 1, {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
  });
  private readonly masks = new Map<THREE.Material, THREE.MeshBasicMaterial>();
  private readonly clearColor = new THREE.Color();

  constructor(private readonly scene: THREE.Scene, private readonly camera: THREE.Camera) {
    super(FILTER);
    this.material.depthTest = this.material.depthWrite = false;
    this.material.toneMapped = false;
    this.uniforms.tMask.value = this.mask.texture;
  }

  override setSize(width: number, height: number): void {
    this.mask.setSize(width, height);
    this.uniforms.texel.value.set(1.4 / width, 1.4 / height);
  }

  private maskFor(material: THREE.Material, id: number): THREE.MeshBasicMaterial {
    let mask = this.masks.get(material);
    if (!mask) {
      mask = new THREE.MeshBasicMaterial({ toneMapped: false });
      // Preserve alpha from any mapped cutout while using the material ID as color.
      mask.onBeforeCompile = shader => {
        shader.fragmentShader = shader.fragmentShader.replace("#include <map_fragment>",
          "#include <map_fragment>\n diffuseColor.rgb = diffuse;");
      };
      this.masks.set(material, mask);
    }
    const { map, alphaMap } = material as THREE.MeshBasicMaterial;
    if (mask.map !== (map ?? null) || mask.alphaMap !== (alphaMap ?? null) || mask.side !== material.side) {
      mask.map = map ?? null;
      mask.alphaMap = alphaMap ?? null;
      mask.side = material.side;
      mask.needsUpdate = true;
    }
    mask.alphaTest = material.alphaTest;
    mask.opacity = material.opacity;
    mask.depthTest = material.depthTest;
    mask.depthWrite = material.depthWrite;
    mask.depthFunc = material.depthFunc;
    mask.visible = material.visible && (!material.transparent || material.alphaTest > 0);
    mask.color.setRGB(id / 255, 0, 0);
    return mask;
  }

  override render(renderer: THREE.WebGLRenderer, write: THREE.WebGLRenderTarget, read: THREE.WebGLRenderTarget,
    delta: number, masked: boolean): void {
    const replaced = new Map<THREE.Mesh, THREE.Material | THREE.Material[]>();
    const hidden: THREE.Object3D[] = [];
    const ids = new Map<THREE.Material, number>();
    const used = new Set<THREE.Material>();
    const replace = (material: THREE.Material) => {
      used.add(material);
      if (usesKuwahara(material) && !ids.has(material) && ids.size < 255) ids.set(material, ids.size + 1);
      return this.maskFor(material, ids.get(material) ?? 0);
    };
    const background = this.scene.background;
    const target = renderer.getRenderTarget();
    const alpha = renderer.getClearAlpha();
    renderer.getClearColor(this.clearColor);
    const shadows = renderer.shadowMap.enabled;
    try {
      this.scene.traverseVisible(object => {
        const mesh = object as THREE.Mesh;
        if (mesh.isMesh) {
          replaced.set(mesh, mesh.material);
          mesh.material = Array.isArray(mesh.material) ? mesh.material.map(replace) : replace(mesh.material);
        } else if ((object as THREE.Line).isLine || (object as THREE.Points).isPoints || (object as THREE.Sprite).isSprite) {
          hidden.push(object);
          object.visible = false;
        }
      });
      this.scene.background = null;
      renderer.shadowMap.enabled = false;
      renderer.setRenderTarget(this.mask);
      renderer.setClearColor(0x000000, 0);
      renderer.clear();
      renderer.render(this.scene, this.camera);
    } finally {
      for (const [mesh, material] of replaced) mesh.material = material;
      for (const object of hidden) object.visible = true;
      this.scene.background = background;
      renderer.shadowMap.enabled = shadows;
      renderer.setRenderTarget(target);
      renderer.setClearColor(this.clearColor, alpha);
    }
    for (const [material, mask] of this.masks) {
      if (used.has(material)) continue;
      mask.dispose();
      this.masks.delete(material);
    }
    super.render(renderer, write, read, delta, masked);
  }

  override dispose(): void {
    this.mask.dispose();
    for (const mask of this.masks.values()) mask.dispose();
    this.masks.clear();
    super.dispose();
  }
}
