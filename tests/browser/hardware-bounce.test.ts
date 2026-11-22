import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import * as THREE from "three";

const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
  return nextResolve(specifier.startsWith(".") && !/\.[a-z]+$/i.test(specifier) ? specifier + ".ts" : specifier, context);
} });
const { acceptsDistantHardwareBounce, applyBounce } = await import("../../adapters/three/src/worldSpecBounce.ts");
hooks.deregister();

function hardware() {
  const material = new THREE.MeshStandardMaterial();
  material.userData = {
    worldSpecSurface: { family: "Building atlases", surface: "Mixed surfaces with metal", recipe: "weathered-steel" },
    sourceFinishes: ["weathered-steel", "stock-hardware"],
  };
  return material;
}
const record = { version: 1 as const, origin: [-48, -48, -4] as [number, number, number],
  dimensions: [65, 65, 17] as [number, number, number], spacing: 1.5, strokeMeters: .36, bufferView: 0 };

test("distant bounce admits authored opaque hardware and rejects protected material roles", () => {
  const accepted = hardware();
  assert.equal(acceptsDistantHardwareBounce(accepted), true);
  accepted.userData.worldSpecSurface.recipe = "stock-hardware";
  assert.equal(acceptsDistantHardwareBounce(accepted), true);
  const disallow: Array<(material: THREE.MeshStandardMaterial) => void> = [
    m => { m.userData.worldSpecSurface.family = "Shop inventory"; },
    m => { m.userData.worldSpecSurface.surface = "Concrete"; },
    m => { m.userData.worldSpecSurface.recipe = "architectural-glazing"; },
    m => { m.userData.worldSpecSurface = undefined; m.name = "Building atlases weathered-steel"; },
    m => { m.transparent = true; }, m => { m.opacity = .5; }, m => { m.alphaTest = .5; },
    m => { m.alphaHash = true; }, m => { m.emissive.setRGB(0, 0, .0001); },
    m => { m.emissiveMap = new THREE.Texture(); }, m => { m.userData.grassField = true; },
    m => { m.userData.worldSpecConcrete = true; }, m => { m.userData.worldSpecSmoothBounce = true; },
    m => { m.userData.cityDetailKeep = true; }, m => { m.userData.sourceFinishes = "weathered-steel"; },
    m => { m.userData.sourceFinishes = ["weathered-steel", undefined]; },
    ...["poster", "shop-sign", "lettering", "tube-light", "emissive", "lens"].map(finish =>
      (m: THREE.MeshStandardMaterial) => { m.userData.sourceFinishes.push(finish); }),
  ];
  for (const change of disallow) {
    const material = hardware(); change(material);
    assert.equal(acceptsDistantHardwareBounce(material), false, change.toString());
    material.emissiveMap?.dispose(); material.dispose();
  }
  accepted.dispose();
});

test("the approximation is opt-in and requires painted probes with a nonzero stroke", () => {
  const texture = new THREE.Data3DTexture();
  for (const [enabled, painted, strokeMeters, permitted] of [
    [false, true, .36, false], [true, false, .36, false], [true, true, 0, false], [true, true, .36, true],
  ] as const) {
    const material = hardware();
    applyBounce(material, texture, { ...record, strokeMeters }, painted, { distantHardware: enabled });
    assert.equal(!!material.defines?.WS_DISTANT_HARDWARE_BOUNCE, permitted);
    material.dispose();
  }
  const retained = hardware(); retained.userData.cityDetailKeep = true;
  applyBounce(retained, texture, record, true, { distantHardware: true });
  assert.equal(retained.defines?.WS_DISTANT_HARDWARE_BOUNCE, undefined);
  retained.dispose(); texture.dispose();
});

test("shader composition preserves textures, directional probes and earlier material hooks", () => {
  const texture = new THREE.Data3DTexture(), material = hardware(), original = hardware();
  const color = new THREE.Texture(), normal = new THREE.Texture(), ao = new THREE.Texture();
  material.map = color; material.normalMap = normal; material.aoMap = ao;
  let chained = 0;
  material.onBeforeCompile = shader => { chained++; shader.fragmentShader = "// custom metal finish\n" + shader.fragmentShader; };
  material.customProgramCacheKey = () => "custom-metal";
  applyBounce(material, texture, record, true, { distantHardware: true });
  applyBounce(original, texture, record, true);
  const shader = { uniforms: {}, vertexShader: THREE.ShaderLib.standard.vertexShader,
    fragmentShader: THREE.ShaderLib.standard.fragmentShader };
  material.onBeforeCompile(shader as THREE.WebGLProgramParametersWithUniforms, {} as THREE.WebGLRenderer);
  assert.equal(chained, 1);
  assert.equal(material.map, color); assert.equal(material.normalMap, normal); assert.equal(material.aoMap, ao);
  assert.equal(shader.uniforms.wsBounce.value, texture);
  assert.match(shader.fragmentShader, /custom metal finish/);
  assert.match(shader.fragmentShader, /vec3 w = n\*n/);
  assert.match(shader.fragmentShader, /smoothstep\(25\.,40\.,length\(vViewPosition\)\)/);
  assert.match(shader.fragmentShader, /smoothstep\(\.75,1\.5,footprint\)/);
  assert.match(shader.fragmentShader, /if \(wsFarBounce >= 1\.\) return wsIrradiance\(center,n\)/);
  assert.match(shader.fragmentShader, /wsGlaze-wsBody\+wsWash\+wsMark/);
  assert.notEqual(material.customProgramCacheKey(), original.customProgramCacheKey());
  assert.ok(material.customProgramCacheKey().startsWith("custom-metal"));
  for (const resource of [texture, color, normal, ao, material, original]) resource.dispose();
});
