import { defineConfig } from "vite";

/** The illustrated tree lab (tests/browser/anime-tree, driven by tests/anime-tree-browser.mjs), built to generated/anime-tree-lab. */
export default defineConfig({
  root: "tests/browser/anime-tree",
  publicDir: false,
  base: "./",
  build: { outDir: "../../../generated/anime-tree-lab", emptyOutDir: true, minify: false },
  worker: { format: "es" },
});
