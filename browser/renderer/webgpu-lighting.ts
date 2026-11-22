import * as THREE from "three";
import { PMREMGenerator, type WebGPURenderer } from "three/webgpu";
import { mix, normalWorldGeometry, vec3 } from "three/tsl";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { ShopLights } from "../../adapters/three/src/shopLights.ts";
import type { GraphicsSettings } from "../../adapters/three/src/graphicsSettings.ts";
import type { RenderDiagnostics } from "../../adapters/three/src/renderProfile.ts";
import { lightingFromQuery, parseLighting, sunOf, type Lighting } from "./lighting.ts";
import { GltfLights } from "./gltf-lights.ts";

/** The portable PBR subset of a published scene's lighting, in glTF Y-up space. */
export function webGpuLighting(renderer: WebGPURenderer, scene: THREE.Scene, model: THREE.Object3D,
  lighting: Lighting, hasRecord: boolean, bounds: THREE.Box3, diagnostics: RenderDiagnostics,
  graphics?: Pick<GraphicsSettings, "sunShadowMapSize" | "fixtureShadowMapSize">) {
  const embedded = new GltfLights();
  const { useRig, authored } = embedded.select([model], hasRecord ? lighting : undefined);
  const sun = useRig ? sunOf(lighting) : undefined;
  type DeviceBackend = { device?: { limits: { maxTextureDimension2D: number } } };
  const maxShadowMapSize = (renderer.backend as DeviceBackend | undefined)?.device?.limits.maxTextureDimension2D;
  const extras = model.userData;
  const environment = extras.worldSpecEnvironment ?? extras.klali;
  const isColor = (value: unknown): value is [number, number, number] =>
    Array.isArray(value) && value.length === 3 && value.every(Number.isFinite);
  const sunDirection = sun && new THREE.Vector3(sun.direction[0], sun.direction[2], -sun.direction[1]).normalize();
  if (isColor(environment?.sky?.zenith) && isColor(environment.sky.horizon)) {
    const horizon = vec3(...environment.sky.horizon), zenith = vec3(...environment.sky.zenith);
    const direction = normalWorldGeometry.normalize();
    const sky = mix(horizon, zenith, direction.y.max(0).pow(.55));
    const haze = mix(sky, horizon.mul(.92), direction.y.negate().div(.25).smoothstep(0, 1));
    if (sun && sunDirection) {
      const alignment = direction.dot(vec3(sunDirection)).max(0);
      scene.backgroundNode = haze.add(vec3(...sun.color).mul(alignment.pow(900).mul(8).add(alignment.pow(24).mul(.22))));
    } else scene.backgroundNode = haze;
    const fog = environment.fog;
    if (fog && Number.isFinite(fog.near) && Number.isFinite(fog.far) && fog.far > fog.near)
      scene.fog = new THREE.Fog(new THREE.Color(...environment.sky.horizon), fog.near, fog.far);
  } else scene.background = new THREE.Color(0x32363b);

  const lights: THREE.Light[] = [];
  if (useRig) {
    const ambient = lighting.ambient;
    lights.push(new THREE.HemisphereLight(new THREE.Color(...ambient.sky), new THREE.Color(...ambient.ground), ambient.intensity));
    if (sun && sunDirection) {
      const light = new THREE.DirectionalLight(new THREE.Color(...sun.color), sun.intensity);
      const sphere = bounds.getBoundingSphere(new THREE.Sphere());
      const radius = Math.max(.1, sphere.radius);
      light.target.position.copy(sphere.center);
      light.position.copy(sphere.center).addScaledVector(sunDirection, radius * 3);
      light.castShadow = sun.castsShadow && diagnostics.lighting.shadows;
      const requested = graphics?.sunShadowMapSize;
      const size = Math.min(requested ?? 2048, maxShadowMapSize ?? 2048);
      light.shadow.mapSize.set(size, size);
      light.shadow.bias = -.00015;
      light.shadow.normalBias = .008;
      light.shadow.autoUpdate = false;
      light.shadow.needsUpdate = true;
      Object.assign(light.shadow.camera, { left: -radius, right: radius, top: radius, bottom: -radius, near: radius * .5, far: radius * 5.5 });
      light.shadow.camera.updateProjectionMatrix();
      scene.add(light.target);
      lights.push(light);
    }
    scene.add(...lights);
  }
  const { fixtures: enableFixtures, shopLight, shadows } = diagnostics.lighting;
  const shadowMapSize = graphics?.fixtureShadowMapSize === undefined ? undefined
    : Math.min(graphics.fixtureShadowMapSize, maxShadowMapSize ?? 512);
  const fixtures = new ShopLights(scene, enableFixtures ? extras.shopLightPositions : [],
    enableFixtures && model.name === "Stoa" && shopLight !== "baseline", false,
    { staticShadows: true, stableSlots: true, shadowMapSize });
  if (!shadows) for (const light of fixtures.shadowLights) light.castShadow = false;
  const casters = [...lights, ...authored, ...fixtures.shadowLights].filter((light): light is THREE.DirectionalLight | THREE.SpotLight | THREE.PointLight =>
    shadows && light.castShadow && (light instanceof THREE.DirectionalLight
      || light instanceof THREE.SpotLight || light instanceof THREE.PointLight));
  renderer.shadowMap.enabled = casters.length > 0;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  renderer.toneMapping = { aces: THREE.ACESFilmicToneMapping, neutral: THREE.NeutralToneMapping, none: THREE.LinearToneMapping }[lighting.toneMapping];
  renderer.toneMappingExposure = lighting.exposure;
  let reflection: THREE.RenderTarget | undefined;
  function dispose() {
    embedded.restore();
    fixtures.dispose();
    for (const light of lights) {
      if (light instanceof THREE.DirectionalLight) light.target.removeFromParent();
      light.removeFromParent(); light.dispose();
    }
    scene.environment = null; scene.backgroundNode = null;
    reflection?.dispose();
  }
  if (lighting.environment.source !== "none" && lighting.environment.intensity > 0) {
    const room = new RoomEnvironment(), generator = new PMREMGenerator(renderer);
    try {
      reflection = generator.fromScene(room);
      scene.environment = reflection.texture;
      scene.environmentIntensity = lighting.environment.intensity;
      scene.environmentRotation.y = lighting.environment.rotation;
    } catch (error) { dispose(); throw error; }
    finally { room.dispose(); generator.dispose(); }
  }
  return {
    dispose, shadowCameras: new Set<THREE.Camera>(casters.map(light => light.shadow.camera)),
    update: (eye: THREE.Vector3) => fixtures.update(eye),
    invalidate() { for (const light of casters) light.shadow.needsUpdate = true; },
  };
}

/** Inspect the manifest before the GLTFLoader starts decoding geometry or textures. */
export function webGpuSceneProfile(json: { scene?: number; scenes?: { extras?: Record<string, any> }[];
  materials?: { extras?: Record<string, any> }[]; extras?: { superspec?: { lighting?: unknown } } },
  lightingQuery: URLSearchParams, diagnostics: RenderDiagnostics) {
  const extras = json.scenes?.[json.scene ?? 0]?.extras ?? {};
  const record = json.extras?.superspec?.lighting ?? extras.worldSpecLighting;
  const lighting = lightingFromQuery(parseLighting(record ?? { version: 1 }), lightingQuery);
  const { ambientOcclusion, contours, paintedSpots, paintedShadows } = diagnostics.lighting;
  const { edgeBoost, edgeBlur } = diagnostics.effects;
  let missing: string | undefined;
  if (extras.worldSpecRender || extras.worldSpecConcreteDetail || extras.worldSpecFinishes || extras.paintedLawn)
    missing = "the published WorldSpec material and finishing effects";
  else if (extras.worldSpecBounce || extras.worldSpecReflectionProbes)
    missing = "the published bounce or reflection probes";
  else if (json.materials?.some(({ extras: material }) => {
    const spec = material?.materialSpec;
    return spec?.program || spec?.live?.length || spec?.shading === "painted" || material?.materialSpecEdge ||
      material?.materialSpecNormalFrame || material?.worldSpecShadeColor;
  })) missing = "the published MaterialSpec shading";
  else if (record !== undefined && (lighting.ambientOcclusion.enabled || lighting.contours?.enabled ||
    lighting.lights.some(light => light.castsShadow && light.shadow.style === "illustration")))
    missing = "the published illustration shadows or screen-space lighting";
  else if (ambientOcclusion === "on" || contours === "on" || edgeBoost === "on" || edgeBlur || paintedSpots || paintedShadows)
    missing = "the requested illustration effects";
  // The WorldSpec adapter enables outline finishing by default, including for older exports
  // which carry the worldSpec scene flag and predate the worldSpecRender record.
  else if (extras.worldSpec === true) missing = "WorldSpec's default outline finishing";
  return { lighting, hasRecord: record !== undefined, missing };
}
