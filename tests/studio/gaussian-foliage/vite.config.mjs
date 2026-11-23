import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const path = relative => fileURLToPath(new URL(relative, import.meta.url));
export default defineConfig({
  root: path("./"),
  base: "./",
  publicDir: false,
  server: { host: "127.0.0.1", fs: { allow: [path("../../../")] } },
  build: { outDir: path("../../../generated/treespec-gaussian-demo"), emptyOutDir: true },
});
