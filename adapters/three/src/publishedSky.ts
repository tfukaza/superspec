import * as THREE from "three";
import type { WorldSpecLighting } from "./worldSpecLighting.ts";

const rgb = (value: unknown): value is [number, number, number] =>
  Array.isArray(value) && value.length === 3 && value.every(n => typeof n === "number" && Number.isFinite(n));

/** The Stoa delivery stores sky and haze on its game record, in linear RGB and metres. */
export function publishedSky(scene: THREE.Scene, raw: unknown, lighting?: WorldSpecLighting) {
  const record = raw as { sky?: { zenith?: unknown; horizon?: unknown }; fog?: { near: number; far: number } } | undefined;
  if (!rgb(record?.sky?.zenith) || !rgb(record.sky.horizon)) return;
  const dome = new THREE.Mesh(
    new THREE.SphereGeometry(350, 32, 16),
    new THREE.ShaderMaterial({
      side: THREE.BackSide,
      depthWrite: false,
      fog: false,
      uniforms: {
        zenith: { value: new THREE.Color() },
        horizon: { value: new THREE.Color() },
        sunDir: { value: new THREE.Vector3(0, 1, 0) },
        sunColor: { value: new THREE.Color() },
      },
      vertexShader: /* glsl */ `
        varying vec3 vDir;
        void main() {
          vDir = position;
          gl_Position = projectionMatrix * modelViewMatrix * vec4( position, 1.0 );
        }`,
      fragmentShader: /* glsl */ `
        uniform vec3 zenith;
        uniform vec3 horizon;
        uniform vec3 sunDir;
        uniform vec3 sunColor;
        varying vec3 vDir;
        void main() {
          vec3 d = normalize( vDir );
          vec3 sky = mix( horizon, zenith, pow( max( d.y, 0.0 ), 0.55 ) );
          // Below the horizon the haze thickens to its own colour.
          sky = mix( sky, horizon * 0.92, smoothstep( 0.0, -0.25, d.y ) );
          float s = max( dot( d, sunDir ), 0.0 );
          sky += sunColor * ( pow( s, 900.0 ) * 8.0 + pow( s, 24.0 ) * 0.22 );
          gl_FragColor = vec4( sky, 1.0 );
          #include <tonemapping_fragment>
          #include <colorspace_fragment>
        }`,
    }),
  );
  dome.renderOrder = -1000;
  dome.frustumCulled = false;
  dome.matrixAutoUpdate = true;
  scene.add(dome);
  const u = dome.material.uniforms;
  u.zenith.value.setRGB(...record.sky.zenith);
  u.horizon.value.setRGB(...record.sky.horizon);
  const sun = lighting?.lights.find(light => light.kind === "directional" && light.role === "sun");
  if (sun) {
    u.sunDir.value.set(sun.direction[0], sun.direction[2], -sun.direction[1]).normalize();
    u.sunColor.value.setRGB(...sun.color);
  }
  const previousFog = scene.fog;
  if (record.fog && Number.isFinite(record.fog.near) && Number.isFinite(record.fog.far) && record.fog.far > record.fog.near)
    scene.fog = new THREE.Fog(u.horizon.value, record.fog.near, record.fog.far);
  return { update: (camera: THREE.Camera) => dome.position.copy(camera.position),
    dispose: () => { scene.fog = previousFog; dome.removeFromParent(); dome.geometry.dispose(); dome.material.dispose(); } };
}
