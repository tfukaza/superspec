import test from "node:test";
import assert from "node:assert/strict";
import { arcadeBattenLight, planterCeilingLight } from "./arcade-lighting.mjs";

test("upper emitters are seated at the cool strip and warm bulkhead and stop inside the floor",()=>{
  for (const [fixture, lens, x, y] of [["ceiling-twin-strip",6.68,-1,5.6],["ceiling-bulkhead",6.64,1,-5.6]]){
    const lamp=planterCeilingLight({position:[x,y,6.8],planterLight:{fixture,floorY:3.5,undersideY:3.25}});
    assert.deepEqual(lamp.position,[x,lens,-y]);
    assert.deepEqual(lamp.target,[x,3.5,-y]);
    assert.ok(lamp.position[1]-lamp.range>3.25 && lamp.position[1]-lamp.range<3.5);
    assert.equal(lamp.floorY,3.5);
    assert.equal(lamp.ceilingY,6.8);
    assert.ok(lamp.candela>0 && lamp.penumbra>.7);
  }
  const down=arcadeBattenLight({position:[-4.8,0,3.25]});
  assert.deepEqual(down.position,[-4.8,3.14,-0]);
  assert.equal(down.kind,"arcade-batten");
});
