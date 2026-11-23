import { TextureLoader, type LoadingManager, type CompressedTexture, type WebGLRenderer } from "three";
import type { WebGPURenderer } from "three/webgpu";
import { KTX2Loader } from "three/addons/loaders/KTX2Loader.js";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { MeshoptDecoder } from "three/addons/libs/meshopt_decoder.module.js";
import type { GraphicsPreset } from "./graphicsSettings.ts";
import { applyGraphicsVisibility } from "./graphicsVisibility.ts";
import { releaseSceneMipsAfterUpload } from "./uploadedMips.ts";

type TextureRenderer = WebGLRenderer | WebGPURenderer;
const transcoders = new WeakMap<TextureRenderer, Map<string, KTX2Loader>>();
const LARGE_IMAGE_PACK = 512;

function transcoder(renderer: TextureRenderer, path: string): KTX2Loader {
  const directory = new URL(path.endsWith("/") ? path : path + "/", document.baseURI).href;
  let loaders = transcoders.get(renderer);
  if (!loaders) transcoders.set(renderer, loaders = new Map());
  let loader = loaders.get(directory);
  if (!loader) {
    loader = new KTX2Loader().setTranscoderPath(directory).detectSupport(renderer);
    loaders.set(directory, loader);
  }
  return loader;
}

/** Whether a glTF document can only be shown with a Basis Universal transcoder. */
export function requiresKtx2Transcoder(json: { extensionsRequired?: string[] }): boolean {
  return json.extensionsRequired?.includes("KHR_texture_basisu") === true;
}

export interface RenderGltfLoaderOptions {
  manager?: LoadingManager;
  /** Directory serving basis_transcoder.js and basis_transcoder.wasm. */
  transcoderPath?: string;
  /** Override the browser image strategy. Auto uses browser images for large published packs. */
  imageDecoder?: "auto" | "html" | "three";
  /** Opt into WorldSpec's visual entity selection before resolving scene dependencies. */
  graphicsPreset?: GraphicsPreset;
  /** Free the mip arrays of transcoded textures bound to the loaded scene after their first WebGL
   * upload. The host must then load the scene again after a lost context. Default false. */
  releaseUploadedMips?: boolean;
}

/** Create a normal Three.js loader with optional SuperSpec texture delivery support. */
export function createRenderGltfLoader(renderer?: TextureRenderer, options: RenderGltfLoaderOptions = {}): GLTFLoader {
  const loader = new GLTFLoader(options.manager).setMeshoptDecoder(MeshoptDecoder);
  if (options.graphicsPreset) {
    const preset = options.graphicsPreset;
    loader.register(parser => ({
      name: "SuperSpecGraphicsVisibility",
      beforeRoot: async () => { applyGraphicsVisibility(parser.json, preset); },
    }));
  }
  if (options.transcoderPath !== undefined) {
    if (!renderer) throw new Error("A renderer is required to select a KTX2 texture format.");
    loader.setKTX2Loader(transcoder(renderer, options.transcoderPath));
  } else {
    // A pack without KTX2, or with PNG/JPEG fallbacks, loads as it always has.
    // One that requires KTX2 fails here, once, not once per texture.
    loader.register(parser => ({
      name: "SuperSpecKtx2Requirement",
      beforeRoot: async () => {
        if (requiresKtx2Transcoder(parser.json))
          throw new Error("This scene's textures are KTX2. Pass transcoderPath, a directory serving basis_transcoder.js and basis_transcoder.wasm.");
      },
    }));
  }
  if (options.releaseUploadedMips) {
    loader.register(() => ({
      name: "SuperSpecUploadedMips",
      afterRoot: async gltf => { releaseSceneMipsAfterUpload(gltf.scenes); },
    }));
  }
  if (options.imageDecoder !== "three") {
    loader.register(parser => {
      // The stock ImageBitmap path can stall on the thousands of image decodes
      // scheduled together by a city pack. This changes only the PNG/JPEG decoder.
      const json = parser.json;
      const published = json.scenes?.[json.scene ?? 0]?.extras?.worldSpec === true;
      if (options.imageDecoder === "html" || (published && (json.images?.length ?? 0) >= LARGE_IMAGE_PACK)) {
        parser.textureLoader = new TextureLoader(loader.manager)
          .setCrossOrigin(loader.crossOrigin)
          .setRequestHeader(loader.requestHeader);
      }
      return { name: "SuperSpecHtmlImageDecoder" };
    });
  }
  return loader;
}

/** Transcode a KTX2 texture delivered outside of glTF, such as a live preview packet. */
export function loadRenderKtx2(renderer: TextureRenderer, bytes: Uint8Array, path: string): Promise<CompressedTexture> {
  // Three transfers the buffer to a worker; preserve the packet's original bytes.
  const buffer = bytes.slice().buffer;
  return new Promise((resolve, reject) => {
    void transcoder(renderer, path).parse(buffer, texture => resolve(texture as CompressedTexture), reject);
  });
}

/** Release worker pools when their owner disposes the renderer. */
export function disposeRenderGltfLoader(renderer: TextureRenderer): void {
  const loaders = transcoders.get(renderer);
  transcoders.delete(renderer);
  for (const loader of loaders?.values() ?? []) {
    // A PNG-only scene never starts the worker. An initializing worker creates its URL asynchronously.
    if (loader.transcoderPending) void loader.transcoderPending.then(() => loader.dispose(), () => loader.dispose());
  }
}
