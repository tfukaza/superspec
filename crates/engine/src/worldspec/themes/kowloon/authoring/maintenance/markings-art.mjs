import { resolve } from "node:path";
// Source for the checked-in marking atlas. Regeneration uses Noto Sans JP for the
// original water-service and inspection lettering; ordinary map builds use the PNG.
import { writeFile } from "node:fs/promises";
export async function bakeMarkings(sharp, destination) {
  const width = 1024,
    height = 512;
  function stencil(x, title, code, left) {
    const arrow = left ? "M134 169H82l23 -13v8h29z" : "M380 169h52l-23 -13v8h-29z";
    return `<g transform="translate(${x},0)" fill="#e6e0cd" font-family="Noto Sans JP, sans-serif" font-weight="bold">
      <text x="256" y="111" text-anchor="middle" font-size="86" letter-spacing="16">${title}</text>
      <path d="M61 129h390v6H61z ${arrow}"/>
      <text x="257" y="202" text-anchor="middle" font-size="54" letter-spacing="6">${code}</text></g>`;
  }
  function docket(x, serial, date, paper, clipped) {
    const edge = clipped
      ? "46,28 95,24 138,29 248,24 344,27 448,25 466,47 461,194 443,214 397,209 351,218 252,211 155,216 52,208"
      : "50,30 189,24 314,30 448,26 463,52 455,194 413,211 365,205 338,217 259,210 135,220 50,208";
    return `<g transform="translate(${x},256)">
      <polygon points="${edge}" fill="${paper}"/><g fill="#514f44" font-family="Noto Sans JP, sans-serif">
      <text x="87" y="76" font-size="31" letter-spacing="6">配管點檢</text>
      <path d="M77 86h345v2H77z M77 133h344v2H77z M77 172h344v2H77z"/>
      <text x="91" y="121" font-size="25">No. ${serial}</text><text x="95" y="160" font-size="24">${date}</text>
      <text x="93" y="196" font-size="16" letter-spacing="3">已查驗</text></g>
      <g transform="rotate(-12 380 163)" fill="#9b5947" stroke="#9b5947"><circle cx="380" cy="160" r="24" fill="none" stroke-width="3"/><text x="380" y="170" font-size="28" text-anchor="middle" stroke="none" font-family="Noto Sans JP,sans-serif">驗</text></g>
      <path d="M437 29l15 3 11 19 -23 -4z" fill="#a99d80"/></g>`;
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">${stencil(0, "供水", "W-075", false)}${stencil(512, "水管", "W-042", true)}${docket(0, "075", "06 / 18", "#e0d2b5", false)}${docket(512, "042", "11 / 04", "#d9cdb2", true)}</svg>`;
  const pixels = await sharp(Buffer.from(svg)).ensureAlpha().raw().toBuffer();
  function random(x, y) {
    let n = Math.imul(x + 17, 374761393) + Math.imul(y + 31, 668265263);
    n = Math.imul(n ^ (n >>> 13), 1274126177);
    return ((n ^ (n >>> 16)) >>> 0) / 4294967296;
  }
  function field(x, y, size) {
    x /= size;
    y /= size;
    const i = Math.floor(x),
      j = Math.floor(y),
      u = x - i,
      v = y - j;
    return (
      (random(i, j) * (1 - u) + random(i + 1, j) * u) * (1 - v) +
      (random(i, j + 1) * (1 - u) + random(i + 1, j + 1) * u) * v
    );
  }
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      if (!pixels[i + 3]) continue;
      const chip = 0.46 * field(x, y, 13) + 0.34 * field(x + 131, y + 59, 4) + 0.2 * random(x, y);
      const grain = field(x, y, 29) - 0.5,
        stipple = random(x + 39, y) - 0.5;
      if (y < 256) {
        const bridge = [179, 245, 308, 362].some((s) => Math.abs((x % 512) - s) < 1.65);
        pixels[i + 3] *= bridge || chip > 0.586 ? 0 : Math.min(1, (0.586 - chip) * 26) * (0.74 + 0.17 * grain);
      } else {
        if (chip > 0.77) pixels[i + 3] = 0;
        const bottom = 256 - (y % 256),
          stain = Math.max(0, 1 - bottom / 85);
        for (let k = 0; k < 3; k++)
          pixels[i + k] = Math.max(
            0,
            Math.min(255, pixels[i + k] * (1 + 0.18 * grain + 0.065 * stipple - 0.1 * stain)),
          );
      }
    }
  await writeFile(
    resolve(destination, "markings.png"),
    await sharp(pixels, { raw: { width, height, channels: 4 } })
      .png()
      .toBuffer(),
  );
}
