// Develop the molded cover flat, with U across the ribs and V between the end caps.
import { join } from "node:path";

export async function bakeBattenLens(sharp, destination) {
  const width = 256, height = 1024;
  const base = Buffer.alloc(width * height * 3), glow = Buffer.alloc(base.length);
  const normal = Buffer.alloc(base.length), properties = Buffer.alloc(base.length);
  const mix = (a,b,t) => a + (b-a)*t;
  const smooth = t => { t = Math.max(0,Math.min(1,t)); return t*t*(3-2*t); };
  const gaussian = x => Math.exp(-x*x/2);
  const ribCount = 18;
  for (let y = 0; y < height; y++) {
    const v = (y+.5)/height;
    const endLight = .48 + .52*smooth(Math.min(v,1-v)/.073);
    for (let x = 0; x < width; x++) {
      const u = (x+.5)/width, k = 3*(y*width+x);
      const phase = 2*Math.PI*ribCount*u;
      const crest = .5+.5*Math.cos(phase);
      const shoulder = smooth(Math.min(u,1-u)/.20);
      // The two lamps behind the cover overlap into a soft central wash.
      const tubes = gaussian((u-.30)/.14)+gaussian((u-.70)/.14);
      const light = endLight*(.65+.19*shoulder+.14*tubes)*(.85+.15*crest);
      const cool = (1-shoulder)*.72 + (1-endLight)*.22;
      const frost = Math.sin(93*u + 2.3*Math.sin(v*17))*.85 + Math.sin(47*v + 12*u)*.55;
      for (let c=0;c<3;c++) {
        const albedo = mix([223,214,193][c], [188,197,194][c],cool);
        base[k+c] = Math.round(albedo*(.90+.10*crest)+frost);
        glow[k+c] = Math.round(255*Math.pow(Math.max(0,light*[.98,.84,.65][c]),1/2.2));
      }
      // The modeled ribs supply the broad change of angle. A shallow secondary
      // flute breaks the reflection on each shoulder without flooding it with noise.
      const slope = .095*Math.sin(2*phase+.6);
      normal[k] = Math.round(127.5+127.5*slope/Math.hypot(1,slope));
      normal[k+1]=128; normal[k+2]=Math.round(127.5+127.5/Math.hypot(1,slope));
      properties[k]=255; properties[k+1]=Math.round(124-24*crest+frost); properties[k+2]=0;
    }
  }
  await Promise.all([["",base],["-emission",glow],["-normal",normal],["-properties",properties]].map(([suffix,data]) =>
    sharp(data,{raw:{width,height,channels:3}}).png().toFile(join(destination,`batten-opal${suffix}.png`))));
}
