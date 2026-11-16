/**
 * Live programs in a browser (tests/live-program-browser.mjs): the generated GLSL evaluated on the CPU reference's
 * sample grid, and the viewer's program material binding each render input. The fixture comes from the engine's
 * `material_live` test (`MATERIAL_LIVE_GLSL_OUTPUT`); every expected value here is worked out on this page or by the CPU.
 */
import * as THREE from "three";
import { programMaterial, programSun, type ProgramShader } from "../../browser/renderer/shade.ts";
import { GroundFields, sceneFieldTexture, setSceneField } from "../../browser/renderer/scene-fields.ts";

interface Sample { uv: number[]; surface: number[]; object: number[]; world: number[]; normal: number[] }
interface Binding { sun: number[]; cast: number; root: number[]; tint: number[]; random: number; placed: number[]; heightRatio: number }
interface Field { width: number; height: number; components: number; min: [number, number]; max: [number, number]; values: number[] }
interface Fixture {
  field: Field;
  programs: { name: string; shader: ProgramShader; parameters: Record<string, unknown>; seeds: number[]; values: number[][]; symbol: string }[];
  samples: Sample[]; bindings: Binding[]; probes: Record<string, ProgramShader>;
}

/**
 * Each program's GLSL on each sample of the grid, drawn into a float target: one 2x2 block of pixels per sample, so the
 * screen derivatives the shader filters by are zero, as the CPU's footprint is. Returns the largest difference to the
 * CPU's value per program.
 */
function grid(fixture: Fixture): Record<string, { worst: number; at: number; samples: number }> {
  const canvas = document.querySelector<HTMLCanvasElement>("#grid")!;
  const gl = canvas.getContext("webgl2")!;
  if (!gl.getExtension("EXT_color_buffer_float")) throw new Error("No float render targets.");
  const count = fixture.samples.length;
  const target = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, target);
  gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA32F, count * 2, 2);
  const framebuffer = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
  const field = gl.createTexture();
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, field);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, fixture.field.width, fixture.field.height, 0, gl.RGBA, gl.FLOAT, new Float32Array(fixture.field.values));
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  const compile = (type: number, text: string) => {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, text);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(`${gl.getShaderInfoLog(shader)}\n${text}`);
    return shader;
  };
  const vertex = compile(gl.VERTEX_SHADER, "#version 300 es\nin vec2 p;\nvoid main() { gl_Position = vec4(p, 0.0, 1.0); }");
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const results: Record<string, { worst: number; at: number; samples: number }> = {};
  for (const entry of fixture.programs) {
    const source = entry.shader.source;
    const fragment = compile(gl.FRAGMENT_SHADER, `#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;
uniform vec3 hWorld, hObject, hNormal, hRoot, hTint, hPlaced;
uniform vec2 hUv, hSurface;
uniform float hCast, hRandom, hHeight;
vec3 msInstanceRoot; vec3 msInstanceTint; float msInstanceRandom; vec3 msPlacedWorld;
#define msVertex_heightRatio hHeight
out vec4 msOut;
${source}
void main() {
  ms_world = hWorld; ms_object = hObject; ms_normal = hNormal; ms_uv = hUv; ms_surface = hSurface;
  ms_footprint = 0.0; ms_uv_footprint = 0.0;
  msInstanceRoot = hRoot; msInstanceTint = hTint; msInstanceRandom = hRandom; msPlacedWorld = hPlaced;
${source.includes("ms_cast_shadow") ? "  ms_cast_shadow = hCast;\n" : ""}  ms_evaluate();
  msOut = vec4(${entry.symbol}, 1.0);
}`);
    const program = gl.createProgram()!;
    gl.attachShader(program, vertex); gl.attachShader(program, fragment);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? "link");
    gl.useProgram(program);
    const at = (name: string) => gl.getUniformLocation(program, name);
    const position = gl.getAttribLocation(program, "p");
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
    for (const parameter of entry.shader.parameters) {
      const value = entry.parameters[parameter.name];
      if (parameter.type === "scalar") gl.uniform1f(at(parameter.uniform), value as number);
      else if (parameter.type === "bool") gl.uniform1i(at(parameter.uniform), value ? 1 : 0);
      else if (parameter.type === "vec2") gl.uniform2fv(at(parameter.uniform), value as number[]);
      else gl.uniform3fv(at(parameter.uniform), value as number[]);
    }
    entry.shader.seeds.forEach((seed, index) => gl.uniform1ui(at(seed.uniform), entry.seeds[index]));
    gl.uniform1i(at("msField0"), 0);
    gl.uniform4f(at("msFieldRect0"), fixture.field.min[0], fixture.field.min[1], fixture.field.max[0], fixture.field.max[1]);
    gl.uniform3f(at("ms_gravity"), 0, 0, -1);
    fixture.samples.forEach((sample, index) => {
      const binding = fixture.bindings[index];
      gl.uniform3fv(at("hWorld"), sample.world); gl.uniform3fv(at("hObject"), sample.object); gl.uniform3fv(at("hNormal"), sample.normal);
      gl.uniform2fv(at("hUv"), sample.uv); gl.uniform2fv(at("hSurface"), sample.surface);
      gl.uniform3fv(at("hRoot"), binding.root); gl.uniform3fv(at("hTint"), binding.tint); gl.uniform3fv(at("hPlaced"), binding.placed);
      gl.uniform1f(at("hCast"), binding.cast); gl.uniform1f(at("hRandom"), binding.random); gl.uniform1f(at("hHeight"), binding.heightRatio);
      gl.uniform3fv(at("msSunDirection"), binding.sun);
      gl.viewport(index * 2, 0, 2, 2);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    });
    const pixels = new Float32Array(count * 2 * 2 * 4);
    gl.readPixels(0, 0, count * 2, 2, gl.RGBA, gl.FLOAT, pixels);
    let worst = 0, worstAt = -1;
    entry.values.forEach((expected, index) => {
      for (let channel = 0; channel < 3; channel++) {
        const difference = Math.abs(pixels[index * 2 * 4 + channel] - expected[channel]);
        if (!(difference <= worst)) { worst = difference; worstAt = index; }
      }
    });
    results[entry.name] = { worst, at: worstAt, samples: count };
  }
  return results;
}

/** Bilinear between texel centers, edge texels continuing outside: the field as `SceneField::sample` reads it. */
function sampleField(field: Field, x: number, y: number): number[] {
  const axis = (value: number, low: number, high: number, size: number) => {
    const t = Math.min(Math.max((value - low) / (high - low) * size - 0.5, 0), size - 1);
    const base = Math.floor(t);
    return { base, next: Math.min(base + 1, size - 1), f: t - base };
  };
  const u = axis(x, field.min[0], field.max[0], field.width), v = axis(y, field.min[1], field.max[1], field.height);
  const texel = (i: number, j: number, c: number) => field.values[(j * field.width + i) * 4 + c];
  return [0, 1, 2].map(c => {
    const low = texel(u.base, v.base, c) * (1 - u.f) + texel(u.next, v.base, c) * u.f;
    const high = texel(u.base, v.next, c) * (1 - u.f) + texel(u.next, v.next, c) * u.f;
    return low * (1 - v.f) + high * v.f;
  });
}

/**
 * The viewer's program material (shade.ts) on an instanced batch of two quads, drawn from straight above into a float
 * target: each probe's painted color is one render input, read at each instance's middle and compared with what this
 * page placed there.
 */
function bindings(fixture: Fixture): { name: string; instance: number; expected: number[]; found: number[]; tolerance: number }[] {
  const renderer = new THREE.WebGLRenderer();
  const size = 64, left = 0, right = 3.5, bottom = 10, top = 12.5;
  const target = new THREE.WebGLRenderTarget(size, size, { type: THREE.FloatType });
  const camera = new THREE.OrthographicCamera(left, right, top, bottom, 0.1, 10);
  camera.position.set(0, 0, 5); camera.lookAt(0, 0, 0); camera.updateMatrixWorld();
  const scene = new THREE.Scene();
  const geometry = new THREE.PlaneGeometry(0.4, 0.4);
  geometry.setAttribute("msVertex_heightRatio", new THREE.Float32BufferAttribute([0.25, 0.25, 0.25, 0.25], 1));
  const placements = [
    { position: new THREE.Vector3(1, 11, 0.2), turn: 0, tint: [0.9, 0.5, 0.25] },
    { position: new THREE.Vector3(2.25, 11.25, 0.3), turn: Math.PI / 2, tint: [0.1, 0.6, 1] },
  ];
  const field = fixture.field;
  setSceneField("ground.color", sceneFieldTexture(field.width, field.height, new Float32Array(field.values)), field.min, field.max);
  programSun.value.set(0.3, 0.4, Math.sqrt(1 - 0.25));
  const results: { name: string; instance: number; expected: number[]; found: number[]; tolerance: number }[] = [];
  // The index hash a batch without a `random` attribute uses (shade.ts), written out here.
  const random = (index: number) => ((Math.imul(index, 2654435761) >>> 0) >>> 8) / 16777216;
  const pixelWorld = (x: number, y: number) => {
    const px = Math.floor((x - left) / (right - left) * size), py = Math.floor((y - bottom) / (top - bottom) * size);
    return { px, py, center: [left + (px + 0.5) / size * (right - left), bottom + (py + 0.5) / size * (top - bottom)] };
  };
  for (const [name, shader] of Object.entries(fixture.probes)) {
    const material = programMaterial({ shader, seedValues: [], parameters: [], mapping: "world" },
      { material: name, boundsMin: [0, 0, 0], boundsMax: [1, 1, 1], features: [] }, [0, 0, -1], {}, "quad", "looped", true, Object.keys(geometry.attributes));
    material.side = THREE.DoubleSide;
    const mesh = new THREE.InstancedMesh(geometry, material, placements.length);
    placements.forEach((placement, index) => {
      mesh.setMatrixAt(index, new THREE.Matrix4().compose(placement.position, new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), placement.turn), new THREE.Vector3(1, 1, 1)));
      mesh.setColorAt(index, new THREE.Color().setRGB(placement.tint[0], placement.tint[1], placement.tint[2], THREE.LinearSRGBColorSpace));
    });
    scene.add(mesh);
    renderer.setRenderTarget(target);
    renderer.setClearColor(0x000000, 0);
    renderer.clear();
    renderer.render(scene, camera);
    placements.forEach((placement, index) => {
      const { px, py, center } = pixelWorld(placement.position.x, placement.position.y);
      const found = new Float32Array(4);
      renderer.readRenderTargetPixels(target, px, py, 1, 1, found);
      const expected = {
        root: [placement.position.x, placement.position.y, placement.position.z],
        tint: placement.tint,
        random: [random(index), random(index), random(index)],
        placed: [center[0], center[1], placement.position.z],
        height: [0.25, 0.25, 0.25],
        fallback: [0.7, 0.7, 0.7],
        field: sampleField(field, placement.position.x, placement.position.y),
        sun: programSun.value.toArray(),
      }[name]!;
      results.push({ name, instance: index, expected, found: [...found.slice(0, 3)], tolerance: name === "placed" ? 1e-3 : 1e-5 });
    });
    scene.remove(mesh);
    material.dispose();
  }
  // The ground fields from a ground the page colors: a flat plane of one linear color, drawn from above.
  const groundScene = new THREE.Scene();
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.MeshStandardMaterial());
  (ground.material as THREE.MeshStandardMaterial).color.setRGB(0.2, 0.4, 0.6, THREE.LinearSRGBColorSpace);
  ground.position.set(5, 6, 0.1);
  groundScene.add(ground);
  ground.updateMatrixWorld();
  const fields = new GroundFields(renderer, 32);
  const made = fields.make(groundScene, [ground]);
  const color = fields.read("ground.color")!, normal = fields.read("ground.normal")!;
  const middle = (data: { width: number; height: number; values: Float32Array }) => {
    const at = ((data.height >> 1) * data.width + (data.width >> 1)) * 4;
    return [...data.values.slice(at, at + 3)];
  };
  results.push({ name: "ground.color", instance: 0, expected: [0.2, 0.4, 0.6], found: middle(color), tolerance: 1e-3 });
  results.push({ name: "ground.normal", instance: 0, expected: [0, 0, 1], found: middle(normal), tolerance: 1e-3 });
  results.push({ name: "ground.rectangle", instance: 0, expected: [4, 5, 6, 7], found: made ? [...made.min, ...made.max] : [], tolerance: 1e-6 });
  fields.dispose();
  renderer.dispose();
  return results;
}

Object.assign(window, { liveProgram: { grid, bindings } });
