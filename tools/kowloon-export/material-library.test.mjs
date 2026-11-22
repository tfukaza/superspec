import test from "node:test";
import assert from "node:assert/strict";
import { markCreases, metalCoverage, paintPigment } from "./metal-textures.mjs";
import { resolveSubmaterials } from "../../tools/materialspec/submaterials.mjs";
import { mergeEdgeVariants } from "./coalesce-materials.mjs";

test("a triangulated sheet marks four boundary edges and leaves its diagonal clean", () => {
  const lines = [];
  markCreases(
    Float32Array.of(0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0),
    [0, 1, 2, 0, 2, 3],
    [0, 0, 1, 0, 1, 1, 0, 1],
    [1, 1, 1],
    (...args) => lines.push(args),
  );
  assert.equal(lines.length, 4);
  assert.ok(lines.every(([x, y, s, t]) => x === s || y === t));
});
test("a metal island must overlap the bound UV triangle", () => {
  const data = Buffer.alloc(16 * 16 * 4);
  data[4 * (2 * 16 + 2) + 2] = 170;
  const hasMetal = metalCoverage(data, 16, 16);
  assert.ok(hasMetal([0, 0, 1, 0, 0, 1], [0, 1, 2]));
  assert.ok(!hasMetal([1, 0, 1, 1, 0, 1], [0, 1, 2]));
  assert.ok(hasMetal([1, 0, 2, 0, 1, 1], [0, 1, 2]));
});
test("Kuwahara preserves alpha and dielectric pixels in a shared color image", () => {
  const data = Buffer.from(
    Array.from({ length: 7 * 7 * 4 }, (_, i) => (i * 47 + 23) % 255),
  );
  const props = Buffer.alloc(data.length);
  for (let i = 0; i < data.length; i += 4) {
    data[i + 3] = 255;
    if (i % 28 > 12) props[i + 2] = 255;
  }
  const filtered = paintPigment(data, props, 7, 7, 1);
  assert.notDeepEqual(filtered, data);
  for (let i = 0; i < data.length; i += 4) {
    if (!props[i + 2])
      assert.deepEqual(filtered.subarray(i, i + 4), data.subarray(i, i + 4));
    assert.equal(filtered[i + 3], 255);
  }
});
test("finish inheritance rejects unknown, out-of-range and cyclic values", () => {
  const schema = { gain: { default: 0.5, min: 0, max: 1 } },
    base = { plain: {}, blue: { extends: "plain", parameters: { gain: 0.9 } } };
  assert.deepEqual(resolveSubmaterials(schema, base).blue, { gain: 0.9 });
  for (const bad of [
    { extends: "absent" },
    { extends: "bad" },
    { parameters: { gain: 2 } },
    { parameters: { unspecified: 0.7 } },
  ])
    assert.throws(() => resolveSubmaterials(schema, { ...base, bad }));
});
test("edge binding merges only when the sibling's occupied UVs are blank in both channels", async () => {
  for (const occupied of [false, true]) {
    const blank = {
      name: "sheet",
      extras: {
        sourceName: "ItemSpec OPAQUE",
        worldSpecSurface: { label: "sheet" },
      },
    };
    const donor = {
      ...blank,
      extras: { ...blank.extras, materialSpecEdge: { from: 0.3, to: 0.06 } },
      occlusionTexture: { index: 3, strength: 0 },
    };
    const j = {
      materials: [blank, donor],
      meshes: [{ primitives: [{ material: 0 }, { material: 1 }] }],
    };
    const rows = [0, 1].map((i) => ({ index: i, sourceRecords: [i] }));
    const uses = [
      [{ prim: { attributes: { TEXCOORD_0: 0 }, indices: 1 } }],
      [],
    ];
    const data = Buffer.alloc(16 * 16 * 4);
    data[(occupied ? 2 * 16 + 2 : 15 * 16 + 15) * 4 + 1] = 200;
    const result = await mergeEdgeVariants(
      j,
      rows,
      uses,
      { read: async (i) => (i ? [0, 1, 2] : [0.1, 0.1, 0.4, 0.1, 0.1, 0.4]) },
      async () => ({ data, info: { width: 16, height: 16 } }),
    );
    assert.equal(result.length, occupied ? 2 : 1);
    assert.equal(j.meshes[0].primitives[1].material, occupied ? 1 : 0);
  }
});
