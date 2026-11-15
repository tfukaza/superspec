import * as THREE from "three";
import { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

/**
 * KTX2 (Basis Universal) textures upload compressed: the transcoder picks the GPU format the
 * renderer supports (BC7, ASTC, ETC2, ...), so they stay several times smaller in GPU memory than
 * PNG. Three's transcoder (`basis_transcoder.js` and `.wasm`) is served from `basis/` beside the
 * page by the SuperSpec builds; hosts serving it elsewhere call `setTranscoderPath` first.
 */
let transcoderPath = new URL("basis/", document.baseURI).href;
const loaders = new WeakMap<THREE.WebGLRenderer, KTX2Loader>();

/** Directory URL (ending in `/`) that serves `basis_transcoder.js` and `basis_transcoder.wasm`. */
export function setTranscoderPath(path: string): void {
  transcoderPath = path.endsWith("/") ? path : `${path}/`;
}

/** One loader per renderer: it owns a transcoder worker pool and the renderer's format support. */
export function ktx2Loader(renderer: THREE.WebGLRenderer): KTX2Loader {
  let loader = loaders.get(renderer);
  if (!loader) {
    loader = new KTX2Loader().setTranscoderPath(transcoderPath).detectSupport(renderer);
    loaders.set(renderer, loader);
  }
  return loader;
}

/** Transcodes one KTX2 file into a compressed texture for this renderer. */
export function loadKtx2(renderer: THREE.WebGLRenderer, bytes: Uint8Array): Promise<THREE.CompressedTexture> {
  // The loader transfers its buffer to a worker, so it gets a copy of exactly these bytes.
  const buffer = bytes.slice().buffer;
  return new Promise((resolve, reject) => {
    void ktx2Loader(renderer).parse(buffer, texture => resolve(texture as THREE.CompressedTexture), reject);
  });
}

/** A GLTFLoader that reads SuperSpec exports, including their default KTX2 textures. */
export function createGltfLoader(renderer: THREE.WebGLRenderer): GLTFLoader {
  return new GLTFLoader().setKTX2Loader(ktx2Loader(renderer));
}
