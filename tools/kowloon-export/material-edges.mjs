/** Pack the MaterialSpec grazing mask and specular gain for the Klali PBR material. */
export function readMaterialEdges({ gl, bind, texture, grazing, specular, width, height }) {
  const sources = new Map();
  function channel(argument) {
    if (argument === undefined) return () => 1;
    if ("value" in argument) return () => argument.value;
    const image = texture(argument.texture);
    if (!sources.has(image)) {
      const [w, h] = bind(image);
      const values = new Float32Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.FLOAT, values);
      if (gl.getError() !== gl.NO_ERROR) throw Error("Live material channel readback failed");
      sources.set(image, values);
    }
    const values = sources.get(image);
    return (i) => values[i + argument.channel] * argument.scale + argument.offset;
  }

  const mask = channel(grazing.mask);
  const factor = channel(specular.factor);
  const weight = channel(specular.mask);
  const pixels = new Uint8ClampedArray(width * height * 4);
  let active = false;
  for (let i = 0; i < pixels.length; i += 4) {
    pixels[i] = Math.round(255 * mask(i));
    pixels[i + 1] = Math.round(255 * (factor(i) - 1) * weight(i));
    pixels[i + 3] = 255;
    active ||= !!(pixels[i] || pixels[i + 1]);
  }
  return { pixels, active };
}
