// Basis Universal KTX2 encoding for published city textures. One recipe per
// texture role; every pixel-affecting input is part of gpuTextureKey.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { availableParallelism } from "node:os";
import { dirname } from "node:path";
import { Worker } from "node:worker_threads";

export const GPU_TEXTURE_RECIPE = "superspec-city-gpu-texture-v1";

/** UASTC transcodes to ASTC 4x4 without further loss and to BC7 with very
 * little; ETC1S is several times smaller and visibly softer. Roles whose
 * texels are read as numbers (normals, roughness, masks) stay UASTC.
 * `rdo` is the UASTC rate-distortion scalar: 0 disables it. */
export const gpuTextureProfiles = Object.freeze({
  color: { codec: "uastc", srgb: true, level: 2, rdo: 1 },
  emissive: { codec: "etc1s", srgb: true, quality: 255, effort: 2 },
  normal: { codec: "uastc", srgb: false, normal: true, level: 2, rdo: 1 },
  data: { codec: "uastc", srgb: false, level: 2, rdo: .5 },
  // Smallest delivery: colour as ETC1S. Selected by the publisher, never implied.
  "color-etc1s": { codec: "etc1s", srgb: true, quality: 255, effort: 2 },
  // Reduced tiers, where the project owner prefers size to fidelity.
  "color-etc1s-small": { codec: "etc1s", srgb: true, quality: 128, effort: 2 },
  "data-small": { codec: "uastc", srgb: false, level: 2, rdo: 2 },
});

export function gpuTextureKey(profile, width, height, rgba) {
  return createHash("sha256").update(GPU_TEXTURE_RECIPE).update(JSON.stringify(Object.entries(profile).sort()))
    .update(`\0${width}x${height}\0`).update(rgba).digest("hex");
}

let encoderModule;
/** The Basis Universal encoder shipped by the ktx2-encoder package. */
export async function loadBasisEncoder() {
  return encoderModule ??= import("ktx2-encoder").then(({ NodeBasisEncoder }) => new NodeBasisEncoder().init());
}

/** Encode top-down RGBA8 texels with a complete clamped mip chain. */
export function encodeGpuTexture(basis, rgba, width, height, profile) {
  if (rgba.length !== width * height * 4 || width % 4 || height % 4) throw Error("GPU textures need RGBA8 texels and edges that are multiples of four.");
  const encoder = new basis.BasisEncoder();
  try {
    const uastc = profile.codec === "uastc";
    if (!uastc && profile.codec !== "etc1s") throw Error("Unknown GPU texture codec: " + profile.codec);
    encoder.setCreateKTX2File(true);
    encoder.setUASTC(uastc);
    encoder.setTexType(0);
    encoder.setDebug(false);
    encoder.setStatusOutput(false);
    encoder.setPrintStats(false);
    encoder.setPerceptual(!!profile.srgb);
    encoder.setKTX2AndBasisSRGBTransferFunc(!!profile.srgb);
    encoder.setMipGen(true);
    encoder.setMipSRGB(!!profile.srgb);
    encoder.setMipWrapping(false);
    if (profile.normal) { encoder.setNormalMapPreset(); encoder.setMipRenormalize(true); }
    if (uastc) {
      encoder.setPackUASTCFlags(profile.level ?? 2);
      encoder.setKTX2UASTCSupercompression(true);
      if (profile.rdo) { encoder.setRDOUASTC(true); encoder.setRDOUASTCQualityScalar(profile.rdo); }
    } else {
      encoder.setQualityLevel(profile.quality ?? 255);
      encoder.setETC1SCompressionLevel(profile.effort ?? 2);
    }
    if (encoder.setSliceSourceImage(0, rgba, width, height, 0) === false) throw Error("The Basis encoder rejected the source image.");
    const output = new Uint8Array(Math.ceil(width * height * 4 * 4 / 3) + 65536);
    const length = encoder.encode(output);
    if (!(length > 0)) throw Error(`Basis encoding failed for a ${width}x${height} ${profile.codec} texture.`);
    return output.slice(0, length);
  } finally { encoder.delete(); }
}

/** Level count, base size and whether the payload is Basis-compressed. */
export function inspectKtx2(bytes) {
  const magic = [0xab, 0x4b, 0x54, 0x58, 0x20, 0x32, 0x30, 0xbb, 13, 10, 26, 10];
  if (bytes.length < 80 || magic.some((value, index) => bytes[index] !== value)) throw Error("Expected a KTX2 file.");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return { vkFormat: view.getUint32(12, true), width: view.getUint32(20, true), height: view.getUint32(24, true),
    levels: view.getUint32(40, true), supercompression: view.getUint32(44, true) };
}

let transcoderModule;
/** Decode with the transcoder three.js ships, as a browser would before the
 * GPU block format. Verification and quality measurement only. */
export async function decodeGpuTexture(bytes, level = 0) {
  transcoderModule ??= (async () => {
    const require = createRequire(import.meta.url);
    const script = require.resolve("three/examples/jsm/libs/basis/basis_transcoder.js");
    const wasmBinary = await readFile(script.replace(/\.js$/, ".wasm"));
    // The transcoder is a classic script that expects CommonJS globals under Node.
    const factory = new Function("require", "__dirname", await readFile(script, "utf8") + "; return BASIS;")(require, dirname(script));
    const basis = await factory({ wasmBinary });
    basis.initializeBasis();
    return basis;
  })();
  const basis = await transcoderModule, file = new basis.KTX2File(new Uint8Array(bytes));
  try {
    if (!file.isValid() || !file.startTranscoding()) throw Error("Invalid Basis KTX2 texture.");
    const info = file.getImageLevelInfo(level, 0, 0), RGBA32 = 13;
    const rgba = new Uint8Array(file.getImageTranscodedSizeInBytes(level, 0, 0, RGBA32));
    if (!file.transcodeImage(rgba, level, 0, 0, RGBA32, 0, -1, -1)) throw Error("Basis transcoding failed.");
    return { width: info.origWidth, height: info.origHeight, rgba, levels: file.getLevels(), hasAlpha: !!file.getHasAlpha(),
      uastc: !!file.isUASTC() };
  } finally { file.close(); file.delete(); }
}

/** Encode on worker threads. Jobs resolve in any order; close() ends the pool. */
export function gpuTexturePool(size = Math.max(1, Math.min(availableParallelism() - 2, 10))) {
  const idle = [], queue = [], workers = new Set();
  let closed = false;
  const fail = (worker, error) => {
    workers.delete(worker);
    worker.job?.reject(error);
    worker.job = undefined;
  };
  const spawn = () => {
    const worker = new Worker(new URL("./gpu-texture-worker.mjs", import.meta.url));
    workers.add(worker);
    worker.on("message", ({ bytes, error }) => {
      const job = worker.job;
      worker.job = undefined;
      if (error) job.reject(Error(error)); else job.resolve(bytes);
      idle.push(worker); pump();
    });
    worker.on("error", error => fail(worker, error));
    worker.on("exit", code => { if (worker.job) fail(worker, Error("GPU texture worker exited with status " + code)); });
    return worker;
  };
  const pump = () => {
    while (queue.length && !closed) {
      const worker = idle.pop() ?? (workers.size < size ? spawn() : undefined);
      if (!worker) return;
      const job = worker.job = queue.shift();
      worker.postMessage({ rgba: job.rgba, width: job.width, height: job.height, profile: job.profile }, [job.rgba.buffer]);
    }
  };
  return {
    size,
    /** `rgba` must own its ArrayBuffer; it is transferred to the worker. */
    encode(rgba, width, height, profile) {
      return new Promise((resolve, reject) => { queue.push({ rgba, width, height, profile, resolve, reject }); pump(); });
    },
    async close() {
      closed = true;
      for (const job of queue.splice(0)) job.reject(Error("The GPU texture pool was closed."));
      await Promise.all([...workers].map(worker => worker.terminate()));
    },
  };
}
