import { defineConfig, type Plugin } from "vite";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/** Three's Basis Universal transcoder, which KTX2 textures need, served and built at `basis/`. */
function basisTranscoder(): Plugin {
  const directory = resolve("node_modules/three/examples/jsm/libs/basis");
  const files = { "basis_transcoder.js": "text/javascript", "basis_transcoder.wasm": "application/wasm" } as const;
  return {
    name: "superspec-basis-transcoder",
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const name = request.url?.split("?")[0].match(/\/basis\/(basis_transcoder\.(?:js|wasm))$/)?.[1] as keyof typeof files | undefined;
        if (!name) return next();
        response.setHeader("Content-Type", files[name]);
        response.end(readFileSync(resolve(directory, name)));
      });
    },
    generateBundle() {
      for (const name of Object.keys(files)) this.emitFile({ type: "asset", fileName: `basis/${name}`, source: readFileSync(resolve(directory, name)) });
    },
  };
}

export default defineConfig({
  root: "editor",
  publicDir: false,
  plugins: [basisTranscoder()],
  server: { host: "127.0.0.1", fs: { allow: [".."] }, headers: { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" } },
  build: { outDir: "../generated/rust-editor", emptyOutDir: true, rollupOptions: { input: { editor: "editor/index.html", preview: "editor/preview.html", material: "editor/material.html", item: "editor/item.html" } } },
  worker: { format: "es" },
});
