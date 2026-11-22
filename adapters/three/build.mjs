import { build } from "vite";
import { fileURLToPath } from "node:url";

const hosts = ["browser", "editor"].map(name => fileURLToPath(new URL("../../" + name + "/", import.meta.url)).replaceAll("\\", "/"));

await build({
  configFile: false,
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
    lib: { entry: "src/index.ts", formats: ["es"], fileName: () => "index.js" },
    rollupOptions: { external: id => id === "three" || id.startsWith("three/") },
  },
});
