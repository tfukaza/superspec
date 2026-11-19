import { leafBrush, type LeafBrushOptions } from "./leaf-brush.ts";
import { leafCardAtlas } from "./leaf-cards.ts";

/**
 * Makes the foliage view's generated textures (foliage-plants.ts) off the page's thread: the leaf-card atlas and the
 * leaf brush are a second or two of arithmetic, which a scene with plants would otherwise wait for with nothing else
 * able to happen on the page. The functions are the page's own, so the texels are the ones the page would make.
 */
export type FoliageTextureJob = { id: number; job: "cards"; arguments: [grid: number, size: number, seed: number, melt: number] }
  | { id: number; job: "brush"; arguments: [options: LeafBrushOptions] };
export type FoliageTextureResult = { id: number; buffers: ArrayBuffer[] } | { id: number; error: string };

self.onmessage = (event: MessageEvent<FoliageTextureJob>) => {
  const request = event.data;
  try {
    const buffers = request.job === "cards"
      ? (atlas => [atlas.data.buffer, atlas.normals.buffer] as ArrayBuffer[])(leafCardAtlas(...request.arguments))
      : [leafBrush(...request.arguments).data.buffer as ArrayBuffer];
    (self as unknown as Worker).postMessage({ id: request.id, buffers } satisfies FoliageTextureResult, buffers);
  } catch (error) {
    (self as unknown as Worker).postMessage({ id: request.id, error: error instanceof Error ? error.message : String(error) } satisfies FoliageTextureResult);
  }
};
