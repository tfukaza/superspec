import { fileURLToPath } from "node:url";

const width = 1024, height = 512;
// The six folds share a metric chart, with 50 mm between blade origins.
const chartWidth = 0.6, chartHeight = 0.3;
const front = Math.hypot(0.031, 0.016);
const grainSource = new URL(
  "../../../../../materialspec/program/examples/textures/concrete-plain.png",
  import.meta.url,
);
const clamp = (value, lo, hi) => Math.max(lo, Math.min(hi, value));

async function bladeTextures(doc, sharp) {
  const grain = await sharp(fileURLToPath(grainSource)).resize(width, height, { fit: "cover" })
    .greyscale().raw().toBuffer();
  const softened = async (radius) => sharp(grain, { raw: { width, height, channels: 1 } })
    .blur(radius).greyscale().raw().toBuffer();
  const [patches, pores] = await Promise.all([softened(12), softened(1.25)]);
  const pigment = Buffer.alloc(width * height * 3), properties = Buffer.alloc(pigment.length);
  const normal = Buffer.alloc(pigment.length);
  for (let y = 0; y < height; y++) {
    const chord = (((y + 0.5) / height) * chartHeight) % 0.05;
    for (let x = 0; x < width; x++) {
      const i = y * width + x;
      const fine = grain[i] - pores[i];
      const pocket = clamp(0.55 + (patches[i] - 164) / 10 + (pores[i] - patches[i]) / 15, 0, 1);
      // Dust catches behind the fold, with scattered clean patches near its nose.
      const dust = pocket * clamp(1.35 - chord / front, 0.12, 1);
      const lipDistance = Math.abs(chord - front);
      const wear = clamp(1 - lipDistance / (0.0016 + 0.0045 * pocket), 0, 1) * pocket;
      const value = 143 + 38 * dust + 91 * wear + 0.72 * fine + 0.42 * (pores[i] - patches[i]);
      pigment.set([value + 13 * dust + 6 * wear, value + 1, value + 4 - 10 * dust]
        .map(v => clamp(Math.round(v), 0, 255)), i * 3);
      properties.set([255, Math.round(146 + 74 * dust - 47 * wear), Math.round(169 - 99 * dust + 52 * wear)], i * 3);
      const left = grain[y * width + Math.max(0, x - 1)];
      const above = grain[Math.max(0, y - 1) * width + x];
      normal.set([128 + clamp((left - grain[i]) * 0.20, -14, 14), 128 + clamp((grain[i] - above) * 0.34, -19, 19), 254], i * 3);
    }
  }
  return Promise.all([["pigment", pigment], ["properties", properties], ["grain", normal]]
    .map(async ([name, data]) => doc.createTexture("Extraction grille " + name).setMimeType("image/png")
      .setImage(await sharp(data, { raw: { width, height, channels: 3 } }).png().toBuffer())));
}

export async function finishGrilleBlades(doc, sharp) {
  const blades = doc.getRoot().listNodes()
    .filter(node => node.getName().endsWith("/grille/blades"))
    .flatMap(node => node.getMesh().listPrimitives());
  if (!blades.length) throw Error("Extraction grille is missing its folded blade surfaces");
  const [pigment, properties, normal] = await bladeTextures(doc, sharp);
  const metal = blades[0].getMaterial().clone().setName("Dust-settled extraction grille aluminum")
    .setBaseColorTexture(pigment).setBaseColorFactor([1, 1, 1, 1])
    .setMetallicRoughnessTexture(properties).setMetallicFactor(1).setRoughnessFactor(1)
    .setNormalTexture(normal).setNormalScale(0.7)
    .setEmissiveTexture(pigment).setEmissiveFactor([0.23, 0.24, 0.26]);
  for (const part of blades) {
    const chart = part.getAttribute("TEXCOORD_0").clone();
    for (let i = 0; i < chart.getCount(); i++) {
      const [u, v] = chart.getElement(i, []);
      chart.setElement(i, [u / chartWidth, v / chartHeight]);
    }
    part.setAttribute("TEXCOORD_0", chart).setMaterial(metal);
  }
}
