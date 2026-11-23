import { parentPort } from "node:worker_threads";
import { encodeGpuTexture, loadBasisEncoder } from "./gpu-texture-codec.mjs";

parentPort.on("message", async ({ rgba, width, height, profile }) => {
  try {
    const bytes = encodeGpuTexture(await loadBasisEncoder(), rgba, width, height, profile);
    parentPort.postMessage({ bytes }, [bytes.buffer]);
  } catch (error) { parentPort.postMessage({ error: error.message }); }
});
