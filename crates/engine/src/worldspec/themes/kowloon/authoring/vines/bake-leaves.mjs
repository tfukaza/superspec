// Hand-painted vine blade studies. UV follows the mesh from petiole (.5,.16) to tip (.5,.96).
import { resolve } from "node:path";

const side = 512;
const palettes = [
  [[55, 78, 66], [121, 144, 101], [162, 165, 103]], // mature jade
  [[65, 78, 62], [134, 146, 104], [169, 159, 102]], // shaded moss
  [[75, 89, 58], [154, 165, 104], [191, 180, 105]], // younger sage
  [[104, 102, 55], [179, 177, 93], [204, 184, 100]], // new olive growth
];
const clamp = x => Math.min(1, Math.max(0, x));
const mix = (a, b, w) => a + (b - a) * w;
const smooth = x => { const t = clamp(x); return t * t * (3 - 2 * t); };
function noise(x, y, seed) {
  function lattice(i, j) {
    let n = Math.imul(i + seed, 374761393) + Math.imul(j, 668265263);
    n = Math.imul(n ^ n >>> 13, 1274126177);
    return ((n ^ n >>> 16) >>> 0) / 4294967295;
  }
  const i = Math.floor(x), j = Math.floor(y), a = smooth(x - i), b = smooth(y - j);
  return mix(mix(lattice(i,j), lattice(i+1,j), a), mix(lattice(i,j+1), lattice(i+1,j+1), a), b);
}

// Each primary vein tapers as it sweeps into the margin. Smaller branches stop
// short of the silhouette. Render this mask at the final texel density.
async function venation(variant, sharp) {
  const paths = [];
  function vein(x, y, cx, cy, ex, ey, width, shade) {
    const dx = cx - x, dy = cy - y, d = Math.hypot(dx,dy) || 1;
    const nx = -dy / d * width, ny = dx / d * width;
    paths.push(`<path fill="rgb(${shade},${shade},${shade})" d="M${x+nx} ${y+ny} Q${cx+nx} ${cy+ny} ${ex} ${ey} Q${cx-nx*.35} ${cy-ny*.35} ${x-nx} ${y-ny} Z"/>`);
  }
  for (const sign of [-1,1]) {
    const jitter = k => (noise(k * 2.7, sign * 4.9, variant + 31) - .5) * 3;
    // Palmately forked secondary veins rise around the heart-shaped basal notch.
    vein(50,16, 50+sign*13,7, 50+sign*29,5+jitter(0), .64,130);
    vein(50,16, 50+sign*20,15, 50+sign*43,25+jitter(1), .83,155);
    for (const [i,[y,extent,rise]] of [[22,44,16],[35,39,18],[49,34,16],[62,24,14],[75,15,10],[84,8,7]].entries()) {
      const z = y + jitter(i+2), x = 50 + sign * extent;
      vein(50,z, 50+sign*extent*.46,z+rise*.82, x,z+rise,.70-i*.075,167-i*10);
      for (const k of [.39,.69]) {
        const u = 50+sign*extent*k, v = z+rise*(1.5*k-.5*k*k);
        vein(u,v, u+sign*extent*.12,v+1, u+sign*extent*.21,v-1.6,.26,85);
      }
    }
  }
  vein(50,16, 50.35,55, 50,96,1.02,232);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 100 100"><rect width="100" height="100" fill="black"/>${paths.join("")}</svg>`;
  return sharp(Buffer.from(svg)).removeAlpha().greyscale().raw().toBuffer();
}

export async function bakeLeaves(sharp, destination) {
  const size = side * 2;
  const color = Buffer.alloc(size * size * 3), properties = Buffer.alloc(color.length);
  for (const [index,[shadow,body,warm]] of palettes.entries()) {
    const veins = await venation(index, sharp), seed = 163 + index * 19;
    for (let y = 0; y < side; y++) for (let x = 0; x < side; x++) {
      const u = (x + .5) / side, v = (y + .5) / side, offset = u - .5;
      const broad = noise(u*5.6,v*4.7,seed);
      const wash = noise(u*10+v*2,v*8-u*5,seed+6);
      const tooth = noise(u*117,v*107,seed+1);
      const fleck = noise(u*46,v*59,seed+18);
      const blade = clamp(.52 + .32*(broad-.5) + .22*(wash-.5) +
        .14*smooth(offset*20+.55) - .10*smooth((v-.78)*5) + .045*(tooth-.5));
      const farEdge = smooth((Math.abs(offset)-.21)*4) + .5*smooth((v-.79)*5);
      const bronze = clamp(.07 + .17*farEdge + .11*(broad-.5) + .24*smooth((fleck-.78)*7)*farEdge);
      const vein = veins[y*side+x] / 255;
      const i = (((index >> 1)*side+y)*size + (index % 2)*side+x)*3;
      for (let c = 0; c < 3; c++) {
        const pigment = mix(mix(shadow[c], body[c], blade), warm[c], bronze);
        color[i+c] = Math.round(mix(pigment, warm[c]+(c===2?9:19), vein*.67));
      }
      properties[i] = 255;
      properties[i+1] = Math.round(181 + wash*29 + tooth*6 - vein*15 - index*3);
      properties[i+2] = 0;
    }
  }
  for (const [name, pixels] of [["leaf-color",color],["leaf-properties",properties]]) {
    await sharp(pixels,{raw:{width:size,height:size,channels:3}}).png()
      .toFile(resolve(destination, `${name}.png`));
  }
}
