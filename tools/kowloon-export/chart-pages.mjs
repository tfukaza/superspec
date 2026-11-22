// Keep the phase of the original JPEG 8x8 blocks for color and roughness.
const align = (n, prior) => n + ((prior - n) & 7);
const channelTexels = pages => pages.reduce((total, page) => {
  const channels = 3 + Number(page.items.some(({ item }) => item.emitter))
    + Number(page.items.some(({ item }) => item.hasEdge));
  return total + page.width * page.height * channels;
}, 0);

// Charts keep their orientation, dimensions and gutter. Packing by optional channels
// keeps a single emissive or edge receiver from allocating an extra map on many pages.
export function chartPages(charts, limit, pad) {
  const side = n => Math.min(limit, 2 ** Math.ceil(Math.log2(n)));
  const legacy = [];
  let x = 0, y = 0, row = 0;
  for (const item of charts) {
    const w = item.w + 2 * pad, h = item.h + 2 * pad;
    if (w > limit || h > limit) throw Error("Chart exceeds page bounds");
    if (x + w > limit) { x = 0; y += row; row = 0; }
    if (!legacy.length || y + h > limit) {
      legacy.push({ width: limit, height: 0, items: [] });
      x = y = row = 0;
    }
    const page = legacy.at(-1);
    page.items.push({ item, x: x + pad, y: y + pad });
    x += w;
    row = Math.max(row, h);
    page.height = side(y + row);
  }

  // Preserve transparent draw order, lossless alpha pages and their original UVs.
  // Opaque runs can share earlier shelves and their unused area.
  const packed = [];
  for (let start = 0; start < legacy.length;) {
    let end = start;
    while (end < legacy.length && legacy[end].items.every(({ item }) => item.repack)) end++;
    if (end === start) { packed.push(legacy[start++]); continue; }
    const original = legacy.slice(start, end);
    const places = original.flatMap(page => page.items);
    const candidate = [];
    for (const place of places.toSorted((a, b) =>
      Number(b.item.hasEdge) - Number(a.item.hasEdge) || Number(b.item.emitter) - Number(a.item.emitter)
      || b.item.h - a.item.h || b.item.w - a.item.w)) {
      const { item } = place, w = item.w + 2 * pad, h = item.h + 2 * pad;
      let page, shelf, left, waste = Infinity;
      for (const p of candidate) for (const s of p.shelves) {
        const sx = align(s.end, place.x - pad), sy = align(s.top, place.y - pad);
        if (sx + w > limit || sy + h > s.bottom) continue;
        const gap = (s.bottom - sy - h) * w + limit - sx - w;
        if (gap < waste) { page = p; shelf = s; left = sx; waste = gap; }
      }
      if (!page) {
        page = candidate.find(p => align(p.usedHeight, place.y - pad) + h <= limit && align(0, place.x - pad) + w <= limit);
        if (!page) { page = { shelves: [], items: [], usedWidth: 0, usedHeight: 0 }; candidate.push(page); }
        const top = align(page.usedHeight, place.y - pad);
        shelf = { top, bottom: top + h, end: 0 };
        page.shelves.push(shelf);
        page.usedHeight = shelf.bottom;
        left = align(0, place.x - pad);
      }
      const top = align(shelf.top, place.y - pad);
      page.items.push({ item, x: left + pad, y: top + pad });
      shelf.end = left + w;
      page.usedWidth = Math.max(page.usedWidth, shelf.end);
    }
    for (const p of candidate) { p.width = side(p.usedWidth); p.height = side(p.usedHeight); }
    // A highly fragmented mix of sizes can still fit better in the original ordering.
    const cheaper = candidate.length <= original.length && channelTexels(candidate) < channelTexels(original);
    packed.push(...(cheaper ? candidate : original));
    start = end;
  }
  return packed.map(({ width, height, items }) => ({ width, height,
    items: items.map(({ item, x, y }) => ({ ...item, x, y })) }));
}
