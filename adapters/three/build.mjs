import { copyFile } from "node:fs/promises";
import { build } from "vite";
import { fileURLToPath } from "node:url";

const hosts = ["browser", "editor"].map(name => fileURLToPath(new URL("../../" + name + "/", import.meta.url)).replaceAll("\\", "/"));
const root = fileURLToPath(new URL(".", import.meta.url));

await build({
  configFile: false,
  root,
  plugins: [{
    name: "superspec-three-package-boundary",
    generateBundle() {
      for (const id of this.getModuleIds()) {
        if (hosts.some(root => id.replaceAll("\\", "/").startsWith(root)))
          this.error("The public rendering adapter cannot depend on a viewer or editor module: " + id);
      }
    },
  }],
  build: {
    lib: { entry: fileURLToPath(new URL("./src/index.ts", import.meta.url)), formats: ["es"], fileName: () => "index.js" },
    rollupOptions: { external: id => id === "three" || id.startsWith("three/") },
  },
});

await copyFile(new URL("../../LICENSE", import.meta.url), new URL("./dist/LICENSE", import.meta.url));
