// A 400 mm tile of sand-cast iron. The grate profile supplies the polished slot lips.
import { resolve } from "node:path";

export async function bakeDrain(sharp, destination) {
  const size = 512;
  function mix(a,b,t) { return a+(b-a)*t; }
  function smooth(t) { t=Math.min(1,Math.max(0,t)); return t*t*(3-2*t); }
  function random(i,j,seed) {
    let n=Math.imul(i+37,374761393)^Math.imul(j+61,668265263)^seed;
    n=Math.imul(n^(n>>>13),1274126177);
    return ((n^(n>>>16))>>>0)/4294967296;
  }
  function field(u,v,cells,seed) {
    const x=u*cells,y=v*cells,i=Math.floor(x),j=Math.floor(y);
    const sample=(i,j)=>random(i%cells,j%cells,seed);
    return mix(mix(sample(i,j),sample(i+1,j),smooth(x-i)),
      mix(sample(i,j+1),sample(i+1,j+1),smooth(x-i)),smooth(y-j));
  }
  const base=Buffer.alloc(size*size*3),normal=Buffer.alloc(base.length),properties=Buffer.alloc(base.length);
  const relief=new Float32Array(size*size);
  for(let y=0;y<size;y++) for(let x=0;x<size;x++) {
    const u=(x+.5)/size,v=(y+.5)/size,i=y*size+x,k=i*3;
    const casting=field(u,v,7,17), fleck=field(u,v,41,59);
    const sand=field(u,v,193,23), tooth=random(x,y,131);
    const crust=smooth((.46*casting+.34*fleck+.20*sand-.49)*3.3);
    const pit=smooth((tooth*.45+sand*.55-.78)*7);
    const skin=7*(casting-.5)+10*(fleck-.5)+6*(tooth-.5)-11*pit;
    for(let c=0;c<3;c++) {
      const iron=[124,119,112][c], oxide=[127,104,84][c];
      base[k+c]=Math.round(mix(iron,oxide,crust)+skin);
    }
    properties[k]=255;
    properties[k+1]=Math.round(153+14*sand+50*crust+12*pit);
    properties[k+2]=Math.round(196-153*crust-40*pit);
    relief[i]=.035*casting+.071*fleck+.13*sand+.06*crust-.24*pit;
  }
  const sample=(x,y)=>relief[((y+size)%size)*size+(x+size)%size];
  for(let y=0;y<size;y++) for(let x=0;x<size;x++) {
    const dx=(sample(x+1,y)-sample(x-1,y))/(800/size);
    const dy=(sample(x,y+1)-sample(x,y-1))/(800/size);
    const len=Math.hypot(dx,dy,1), k=(y*size+x)*3;
    normal[k]=Math.round(127.5*(1-dx/len)); normal[k+1]=Math.round(127.5*(1-dy/len));
    normal[k+2]=Math.round(127.5*(1+1/len));
  }
  for(const [suffix,pixels] of [["base",base],["normal",normal],["properties",properties]]) {
    await sharp(pixels,{raw:{width:size,height:size,channels:3}}).png()
      .toFile(resolve(destination, `drain-${suffix}.png`));
  }
}
