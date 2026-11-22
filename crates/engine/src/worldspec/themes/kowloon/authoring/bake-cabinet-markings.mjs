// Four transfer prints for the service cabinet. The ItemSpec supplies the gauge needle.
import { resolve } from "node:path";


export async function bakeCabinetMarkings(sharp, out) {
  function line(x1, y1, x2, y2, width = 1) {
    return `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="currentColor" stroke-width="${width}"/>`;
  }

  function text(x, y, label, size, align = "middle", more = "") {
    return `<text x="${x}" y="${y}" fill="currentColor" font-family="sans-serif" font-size="${size}" text-anchor="${align}" ${more}>${label}</text>`;
  }

  function meter(x, unit, maximum) {
    const polar = (r, angle) => [128 + r * Math.cos(angle), 194.56 + r * Math.sin(angle)];
    const at = i => (-143 + 106 * i / 30) * Math.PI / 180;
    const start = polar(121, at(0)), end = polar(121, at(30));
    const ticks = Array.from({length: 31}, (_, i) => {
      const major = i % 5 === 0;
      const [x1, y1] = polar(121, at(i)), [x2, y2] = polar(major ? 107 : 115, at(i));
      const [tx, ty] = polar(139, at(i));
      return line(x1, y1, x2, y2, major ? 2 : 1) + (major ? text(tx, ty + 4, String(maximum * i / 30), 14) : "");
    }).join("");
    return `<g transform="translate(${x} 0)" color="#49453c">
      <rect width="256" height="256" fill="url(#stock)"/>
      ${text(128, 29, "S T O A   I N S T R U M E N T", 7.6)}
      <path d="M${start} A121,121 0 0,1 ${end}" fill="none" stroke="#5e584a" stroke-width="1"/>
      ${ticks}${text(128, 157, unit, 33)}
      ${text(128, 174, unit === "V" ? "AC VOLTS" : "AC AMPERES", 8)}
      ${line(21, 217, 235, 217, .7)}
      ${text(23, 232, "CLASS  2.5", 7.8, "start")}${text(232, 232, "50 Hz   ~", 9, "end")}
    </g>`;
  }

  const inspection = (row, circuit, date) => `<svg x="0" y="${row}" width="256" height="128" viewBox="0 0 210 150" preserveAspectRatio="none">
    <rect width="210" height="150" fill="#cfaa65"/><g color="#65503c">
    <rect x="6" y="6" width="198" height="138" fill="none" stroke="#7b623e" stroke-width="1.4"/>
    ${text(13, 27, "PERIODIC INSPECTION", 13.3, "start", 'font-weight="bold"')}
    ${line(7, 36, 202, 36, 1.5)}${line(7, 78, 202, 78, 1)}${line(7, 113, 202, 113, 1)}
    ${text(14, 48, "DATE", 7, "start")}${text(25, 68, date, 18.5, "start", 'style="font-family:serif;font-style:italic"')}
    ${text(14, 91, "CIRCUIT", 7, "start")}${text(86, 104, circuit, 17)}
    ${text(13, 132, "STOA  /  ELECTRICAL", 8, "start")}</g>
    <g transform="rotate(-12 172 111)" color="#95583e"><circle cx="172" cy="111" r="21" fill="none" stroke="currentColor" stroke-width="2"/>
    <circle cx="172" cy="111" r="18" fill="none" stroke="currentColor" stroke-width=".7"/>${text(172, 116, "OK", 16)}</g></svg>`;

  const idPlate = (row, code) => `<svg x="256" y="${row}" width="256" height="128" viewBox="0 0 222 74" preserveAspectRatio="none" color="#404d4e">
    <rect width="222" height="74" fill="#ccc8ad"/><rect x="4" y="4" width="214" height="66" fill="none" stroke="currentColor"/>
    ${text(14, 18, "STOA   ELECTRIC   WORKS", 8.4, "start")}${line(12, 23, 210, 23, .9)}
    ${text(14, 53, code, 26, "start", 'font-weight="bold"')}${text(207, 51, "SERIES  7", 10, "end")}
    ${text(14, 64, "DISTRIBUTION BOARD", 6.3, "start")}</svg>`;

  const source = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512">
    <defs><linearGradient id="stock" x2=".28" y2="1" gradientUnits="objectBoundingBox">
    <stop stop-color="#d7ceb4"/><stop offset=".6" stop-color="#e6ddc1"/><stop offset="1" stop-color="#cec4a8"/></linearGradient></defs>
    ${meter(0, "V", 300)}${meter(256, "A", 60)}${inspection(256, "N-04", "14 / 08 / 92")}${inspection(384, "E-07", "21 / 05 / 93")}${idPlate(256, "DB – 04")}${idPlate(384, "DB – 07")}</svg>`;

  // Fine paper tooth extends through the printing. Slight edge foxing stays on the tag stock.
  const pixels = await sharp(Buffer.from(source)).removeAlpha().raw().toBuffer();
  let seed = 427;
  for (let y = 0; y < 512; y++) for (let x = 0; x < 512; x++) {
    seed = (Math.imul(1664525, seed) + 1013904223) >>> 0;
    const grain = (seed / 4294967296 - 0.5) * 7;
    const u = x % 256, cellHeight = y < 256 ? 256 : 128, v = y % cellHeight;
    const edge = Math.min(u, v, 255 - u, cellHeight - 1 - v);
    const wear = y < 256 ? 0 : Math.exp(-edge / 4) * (7 + 6 * Math.sin(v * .21 + u * .11));
    for (let c = 0; c < 3; c++) {
      const i = (y * 512 + x) * 3 + c;
      pixels[i] = Math.max(0, Math.min(255, pixels[i] + grain - wear * (0.5 + c * .4)));
    }
  }
  await sharp(pixels, {raw: {width: 512, height: 512, channels: 3}}).png().toFile(resolve(out));
}
