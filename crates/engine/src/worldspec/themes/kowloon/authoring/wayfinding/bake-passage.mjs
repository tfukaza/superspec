// Source artwork for the two handed versions. The lettering stays upright.
import { resolve } from "node:path";

export async function bakePassage(sharp, destination) {
  const width = 1024, height = 384;
  const clamp = value => Math.max(0, Math.min(1,value));
  function hash(x,y) {
    let h = Math.imul(x+13,374761393) + Math.imul(y+71,668265263);
    h = Math.imul(h^(h>>>13),1274126177);
    return ((h^(h>>>16))>>>0)/4294967296;
  }
  function pigment(x,y,scale) {
    x /= scale; y /= scale;
    const i = Math.floor(x), j = Math.floor(y), u=x-i, v=y-j;
    return (1-v)*((1-u)*hash(i,j)+u*hash(i+1,j)) + v*((1-u)*hash(i,j+1)+u*hash(i+1,j+1));
  }
  for (const left of [false,true]) {
    const textX = left ? 427 : 67;
    const arrow = left ? "translate(1024 0) scale(-1 1)" : "";
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="384">
      <path fill="#cfc5a4" d="M67 37l891 -1 -2 7 -889 1z"/>
      <text x="${textX}" y="240" fill="#394147" font-family="Microsoft JhengHei, Noto Sans JP, sans-serif" font-size="220" font-weight="bold" letter-spacing="17">通道</text>
      <text x="${textX+12}" y="336" fill="#343b40" font-family="Arial, sans-serif" font-size="68" font-weight="bold" letter-spacing="12">PASSAGE</text>
      <path transform="${arrow}" fill="#394147" d="M626 142l202 0 0-54 123 91 -123 93 0-56 -202 0z"/>
      </svg>`;
    const pixels = await sharp(Buffer.from(svg)).ensureAlpha().raw().toBuffer();
    const bridges = [textX+65,textX+180,textX+314,textX+396];
    for (let y=0; y<height; y++) for (let x=0; x<width; x++) {
      const k=(y*width+x)*4;
      if (!pixels[k+3]) continue;
      const large=pigment(x,y,33), grain=hash(x,y);
      const wear=.31*large+.37*pigment(x+86,y-23,11)+.21*pigment(x,y,3)+.11*grain;
      const bridge=y>78 && y<252 && bridges.some(c=>Math.abs(x-c)<1.5);
      const cover=bridge ? 0 : clamp((.682-wear)*17)*(.88+.09*large);
      pixels[k+3] = Math.round(pixels[k+3]*cover);
      const dust = 9*(large-.5)+5*(grain-.5);
      for(let c=0;c<3;c++) pixels[k+c] = Math.round(pixels[k+c]+dust);
    }
    await sharp(pixels,{raw:{width,height,channels:4}}).png().toFile(resolve(destination, left ? "passage-left.png" : "passage.png"));
  }
}
