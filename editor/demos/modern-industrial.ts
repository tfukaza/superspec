import source from "./modern-industrial-bench.aml?raw";
import { concreteMaps, concreteResolution } from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/authoring/concrete.ts";
import powerPlug from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/power-plug-body.glb?url";
import powerSocket from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/power-socket.glb?url";
import powerCables from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/power-cable-bundle.glb?url";
import criticalPower01 from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/critical-power-01.glb?url";
import criticalPower02 from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/critical-power-02.glb?url";
import criticalPower03 from "../../crates/engine/src/worldspec/themes/sci_fi_industrial/items/critical-power-03.glb?url";
import type { AmlSelection } from "../open-aml.ts";

export async function modernIndustrialDemo(isCurrent: () => boolean): Promise<AmlSelection> {
  const canvas = new OffscreenCanvas(concreteResolution, concreteResolution);
  const context = canvas.getContext("2d")!;
  const assets: AmlSelection["assets"] = {};
  const companions = {
    "power-plug-body.glb": powerPlug, "power-socket.glb": powerSocket, "power-cable-bundle.glb": powerCables,
    "critical-power-01.glb": criticalPower01, "critical-power-02.glb": criticalPower02, "critical-power-03.glb": criticalPower03,
  };
  await Promise.all(Object.entries(companions).map(async ([name, url]) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Could not load ${name} (${response.status}).`);
    assets[name] = new Uint8Array(await response.arrayBuffer());
  }));
  for (const [name, pixels] of concreteMaps()) {
    if (!isCurrent()) throw new DOMException("Demo selection superseded.", "AbortError");
    context.putImageData(new ImageData(pixels, concreteResolution, concreteResolution), 0, 0);
    const blob = await canvas.convertToBlob({ type: "image/png" });
    assets[name] = new Uint8Array(await blob.arrayBuffer());
  }
  return { source, assets };
}
