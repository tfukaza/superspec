import { readFile } from "node:fs/promises";
// Unique developed face of the passage wall, 6.50 x 3.25 m. V increases toward the floor.
import { resolve } from "node:path";

export async function bakeOchre(sharp, destination) {
  const width = 2048, height = 1024;
  const clamp = v => Math.max(0, Math.min(1,v));
  const mix = (a,b,t) => a+(b-a)*t;
  const smooth = t => { t=clamp(t); return t*t*(3-2*t); };
  function hash(x,y,seed) {
    let n=Math.imul(x+53,374761393)^Math.imul(y+79,668265263)^seed;
    n=Math.imul(n^(n>>>13),1274126177);
    return ((n^(n>>>16))>>>0)/4294967296;
  }
  function wash(x,y,scale,seed) {
    const a=x/scale,b=y/scale,i=Math.floor(a),j=Math.floor(b);
    const u=smooth(a-i),v=smooth(b-j);
    return mix(mix(hash(i,j,seed),hash(i+1,j,seed),u),
      mix(hash(i,j+1,seed),hash(i+1,j+1,seed),u),v);
  }
  const grainSource = new URL("../../../../../materialspec/program/examples/textures/concrete-plain.png",import.meta.url);
  const mineral = await sharp(await readFile(grainSource)).resize(640,640).greyscale().raw().toBuffer();
  const color=Buffer.alloc(width*height*3), properties=Buffer.alloc(color.length), normal=Buffer.alloc(color.length);
  const relief=new Float32Array(width*height);

  // Fine, discontinuous shrinkage cracks. The long stretches of intact painted plaster
  // between them leave clear space for the passage stencil.
  const cracks=[
    [[3,47],[54,67],[85,85],[109,90],[135,122],[151,127],[166,152]],
    [[100,93],[102,117],[90,139]],
    [[1987,0],[1979,25],[1949,43],[1937,66],[1905,78],[1899,100]],
    [[1874,929],[1836,907],[1810,909],[1784,882],[1758,872],[1744,849]],
    [[1809,911],[1791,931],[1785,957]],
    [[384,1023],[362,991],[336,976],[328,951]],
  ];
  const paths=cracks.map(points => `<polyline points="${points.map(p=>p.join(",")).join(" ")}"/>`).join("");
  const mask=await sharp(Buffer.from(`<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="black"/><g fill="none" stroke="white" stroke-width="1.6">${paths}</g></svg>`)).greyscale().raw().toBuffer();
  const pigments=[[190,157,98],[207,178,125],[168,155,132],[146,140,123]];
  for(let y=0;y<height;y++) for(let x=0;x<width;x++) {
    const i=y*width+x,k=i*3;
    const broad=wash(x,y,238,13), plaster=wash(x,y,68,41), coarse=wash(x,y,21,97);
    const flake=wash(x,y,7,239), grit=wash(x,y,2.6,117);
    const grain=mineral[(y%640)*640+(x%640)]/255-.56;
    const edge=Math.min(x,width-1-x);
    const dampZone=1-smooth((height-1-y)/(65+61*wash(x,41,87,31)));
    const loss=.38*coarse+.34*flake+.18*grit+.1*plaster
      +.32*dampZone+.16*(1-smooth(edge/37))+.18*(1-smooth(y/32));
    const undercoat=smooth((loss-.705)*12);
    const bare=smooth((loss-.775)*16);
    const dust=clamp(.09+.60*broad+.36*(plaster-.5)+.15*(coarse-.5));
    const crack=mask[i]/255*smooth(.3+3*flake)*(.45+.55*grit);
    const brush=wash(x*.37+y*.12,y-x*.18,11,613);
    const texture=2.1*(brush-.5)+3.0*(grit-.5)+22*grain-17*dampZone-37*crack;
    for(let c=0;c<3;c++) {
      const intact=mix(pigments[0][c],pigments[1][c],dust);
      color[k+c]=Math.round(mix(mix(intact,pigments[2][c],undercoat),pigments[3][c],bare)+texture);
    }
    // Relative surface height in millimetres. The exposed mineral coat is rougher
    // and slightly recessed, so its edge also reads under grazing light.
    relief[i]=1.25*(coarse-.5)+.43*(brush-.5)+2.1*grain-.62*undercoat-.57*bare-.83*crack;
    properties[k]=255; properties[k+1]=Math.round(216+11*coarse+12*undercoat-5*dampZone); properties[k+2]=0;
  }
  const sample=(x,y)=>relief[Math.min(height-1,Math.max(0,y))*width+Math.min(width-1,Math.max(0,x))];
  const pixelMM=6500/width;
  for(let y=0;y<height;y++) for(let x=0;x<width;x++) {
    const dx=(sample(x+1,y)-sample(x-1,y))/(2*pixelMM);
    const dy=(sample(x,y+1)-sample(x,y-1))/(2*pixelMM);
    const length=Math.hypot(dx,dy,1), k=(y*width+x)*3;
    normal[k]=Math.round(127.5*(1-dx/length));
    normal[k+1]=Math.round(127.5*(1-dy/length));
    normal[k+2]=Math.round(127.5*(1+1/length));
  }
  for(const [suffix,pixels] of [["base",color],["normal",normal],["properties",properties]]) {
    await sharp(pixels,{raw:{width,height,channels:3}}).png()
      .toFile(resolve(destination, `ochre-${suffix}.png`));
  }
}
