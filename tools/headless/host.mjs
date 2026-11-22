import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import * as THREE from "three";
import { createWebGL2Context } from "webgl-node";
export { createCanvas, ImageData } from "@napi-rs/canvas";
import { createCanvas, loadImage } from "@napi-rs/canvas";
import { runNativeBinary } from "./native-stdio.mjs";
import { GpuBaker, decodeNativeReport, writeArchive, setParameters, setSeeds } from "../../generated/headless/core.js";

const root = fileURLToPath(new URL("../../", import.meta.url));
export const studio = resolve(process.env.STUDIO_CLI ?? resolve(root, "target/release/superspec-studio" + (process.platform === "win32" ? ".exe" : "")));

export async function compile(action, project) {
  const assets = Object.entries(project.assets ?? {});
  const paths = Object.fromEntries(assets.map(([name], index) => [name, "assets/" + index]));
  const document = Buffer.from(JSON.stringify({ ...project, assets: paths }));
  const zip = writeArchive([{ name: "project.json", bytes: document }, ...assets.map(([name, bytes]) => ({
    name: paths[name], bytes: typeof bytes === "string" ? Buffer.from(bytes, "base64") : bytes,
  }))]);
  const archive = Buffer.from(await zip.arrayBuffer());
  let buffer;
  try {
    buffer = await compileWithPipes(action, archive);
  } catch (error) {
    if (error.code !== "EPERM" || error.syscall !== "spawn") throw error;
    buffer = await runNativeBinary(studio, ["bake-input", action], archive);
  }
  const bytes = buffer.byteOffset === 0 && buffer.byteLength === buffer.buffer.byteLength
    ? buffer.buffer : buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  return action === "aml-scene" ? bytes : decodeNativeReport(bytes, 1);
}

async function compileWithPipes(action, archive) {
  const child = spawn(studio, ["bake-input", action], { windowsHide: true, stdio: ["pipe", "pipe", "inherit"] });
  const chunks = [];
  child.stdout.on("data", chunk => chunks.push(chunk));
  const done = new Promise((yes, no) => {
    child.once("error", no);
    child.once("close", code => code === 0 ? yes() : no(Error("Native " + action + " exited with status " + code)));
  });
  child.stdin.on("error", () => {}); // A rejected request closes stdin before the archive finishes.
  child.stdin.end(archive);
  await done;
  return Buffer.concat(chunks);
}

export function openBaker() {
  const context = createWebGL2Context(8, 8);
  const { canvas, gl } = context;
  const renderer = new THREE.WebGLRenderer({ canvas, context: gl });
  const baker = new GpuBaker(renderer);
  if (baker.unavailable()) throw Error(baker.unavailable());
  console.log("Headless GPU:", gl.getParameter(gl.RENDERER));
  const textures = new Map();
  const inputs = new Set();
  async function texture(bytes, linear) {
    const bitmap = await loadImage(bytes);
    const canvas = createCanvas(bitmap.width, bitmap.height);
    const ctx = canvas.getContext("2d");
    ctx.translate(0, bitmap.height);
    ctx.scale(1, -1);
    ctx.drawImage(bitmap, 0, 0);
    const rgba = ctx.getImageData(0, 0, bitmap.width, bitmap.height).data;
    const image = new THREE.DataTexture(new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), bitmap.width, bitmap.height);
    image.wrapS = image.wrapT = THREE.RepeatWrapping;
    image.colorSpace = linear ? THREE.NoColorSpace : THREE.SRGBColorSpace;
    image.minFilter = THREE.LinearMipmapLinearFilter;
    image.magFilter = THREE.LinearFilter;
    image.generateMipmaps = true;
    image.anisotropy = 8;
    image.needsUpdate = true;
    return image;
  }
  return { baker, renderer, gl, inputs,
    async uniforms(material, values, assets) {
      const uniforms = {};
      setParameters(uniforms, material, values);
      setSeeds(uniforms, material);
      for (const entry of material.shader.images) {
        const catalog = /^superspec-material:([\w.-]+)$/.exec(entry.name);
        const input = assets[entry.name] ?? (catalog && await readFile(resolve(root, "assets/materials", catalog[1])));
        if (!input) throw Error("Missing shader image " + entry.name);
        const bytes = Buffer.from(input, typeof input === "string" ? "base64" : undefined);
        const key = createHash("sha256").update(bytes).digest("hex") + "/" + entry.linear;
        inputs.add(entry.name + "/" + key);
        if (!textures.has(key)) textures.set(key, await texture(bytes, entry.linear));
        uniforms[entry.uniform] = { value: textures.get(key) };
      }
      return uniforms;
    },
    close() {
      baker.dispose();
      for (const texture of textures.values()) texture.dispose();
      renderer.dispose();
      context.destroy();
    },
  };
}

export async function bakeIdentity(host) {
  const hash = createHash("sha256");
  for (const path of [studio, new URL("../../generated/headless/core.js", import.meta.url), new URL("./host.mjs", import.meta.url),
    new URL("./package-lock.json", import.meta.url), new URL("../../package-lock.json", import.meta.url)])
    hash.update(createHash("sha256").update(await readFile(path)).digest());
  return hash.update(host.gl.getParameter(host.gl.RENDERER)).update(host.gl.getParameter(host.gl.VERSION)).digest("hex");
}
