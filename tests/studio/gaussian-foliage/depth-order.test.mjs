import assert from "node:assert/strict";
import test from "node:test";
import { createSplatDepthOrder } from "../../../browser/renderer/splat-depth-order.mjs";

test("splat radix ordering matches a stable descending float32 comparison", () => {
  const values = new Float32Array(4096);
  for (let index = 0; index < values.length; index++) {
    values[index] = Math.sin(index * 91.17) * (index % 6 === 0 ? 1e-13 : index * 1.3);
  }
  values.set([3, 0, -0, -1.25, 3, -1.25, -4, 2.55]);
  const order = createSplatDepthOrder(values.length);
  for (const active of [
    Uint32Array.of(5, 2, 4, 3, 1, 0, 6, 7),
    Uint32Array.from(values.keys()).filter(index => index % 3 !== 1),
    Uint32Array.of(),
  ]) {
    order.depths.set(values);
    const expected = Array.from(active).sort((a, b) => values[b] - values[a]);
    assert.deepEqual(Array.from(order.sort(active, active.length)), expected);
  }
});
