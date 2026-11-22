/**
 * The washroom corner from `concept.png` as plain geometry: one mesh per material receiver.
 * Meters, Z up. The parts are laid out with the tall wall's face on the plane x = 0 and the low
 * wall's face on y = 0, then turned a quarter turn about Z, so the room opens toward (+x, -y):
 * the corner Material Studio's camera starts from. The tall wall faces -y and the low wall +x.
 */
import { Mesh, add, cross, dot, mul, unit, writeGlb } from "./geometry.mjs";

const X = [1, 0, 0], Y = [0, 1, 0], Z = [0, 0, 1];

const clerestory = { left: 0.54, right: 1.99, sill: 1.26, head: 2.12 };

/** The daylight wall has an open center with concrete jambs, sill and head. */
function daylightWall(wall) {
  const { left, right, sill, head } = clerestory;
  const xs = [0, left, right, 2.3], zs = [0, sill, head, 2.5];
  const solid = (column, row) => column >= 0 && column < 3 && row >= 0 && row < 3 && !(column === 1 && row === 1);
  const outside = -0.4, inside = 0;
  for (let column = 0; column < 3; column++) for (let row = 0; row < 3; row++) {
    if (!solid(column, row)) continue;
    const [a, b] = xs.slice(column, column + 2), [c, d] = zs.slice(row, row + 2);
    for (const y of [outside, inside]) {
      wall.quad([a, y, c], [b, y, c], [b, y, d], [a, y, d], y === inside ? Y : mul(Y, -1));
    }
    for (const step of [-1, 1]) {
      if (!solid(column + step, row)) {
        const x = step < 0 ? a : b;
        wall.quad([x, outside, c], [x, inside, c], [x, inside, d], [x, outside, d], mul(X, step));
      }
      if (!solid(column, row + step)) {
        const z = step < 0 ? c : d;
        wall.quad([a, outside, z], [b, outside, z], [b, inside, z], [a, inside, z], mul(Z, step));
      }
    }
  }
}

/** An unglazed steel frame divides the opening into three lights. */
function daylightFrame(steel) {
  const { left, right, sill, head } = clerestory;
  const depth = -0.06, face = 0.005, width = 0.035;
  for (const z of [sill, head - width]) steel.box([left, depth, z], [right, face, z + width]);
  for (const x of [left, left + (right - left) / 3, right - (right - left) / 3, right - width]) {
    steel.box([x, depth, sill + width], [x + width, face, head - width]);
  }
}

/** A wall bracket: an arm leaving the wall at `origin`, a leg down the wall and a brace between. */
function bracket(mesh, origin, out, along, reach, drop, width = 0.04, thickness = 0.006) {
  const down = mul(Z, -1);
  const at = (o, d) => add(origin, add(mul(out, o), mul(down, d)));
  mesh.bar(at(0, thickness / 2), at(reach, thickness / 2), along, width, thickness);
  mesh.bar(at(thickness / 2, 0), at(thickness / 2, drop), along, width, thickness);
  mesh.bar(at(reach * 0.9, thickness), at(thickness, drop * 0.92), along, width * 0.7, thickness);
}

/**
 * Where the bolt template stands: the one mesh every bolt of the scene is an instance of (an instanced batch, see
 * `buildBatches`). It is an ordinary mesh with a material, so the project bakes its material once, here; it stands
 * well below the slab so that it touches and shades nothing, and it is not drawn where it stands.
 */
const BOLT_ORIGIN = [0, 0, -5];

/** A hex-head bolt on a washer, its axis along +Z from `origin`: about 3 cm across. */
function bolt(mesh, origin) {
  const at = (x, y, z) => add(origin, [x, y, z]);
  // The washer: a low smooth cylinder.
  mesh.tube([at(0, 0, 0), at(0, 0, 0.003)], 0.016, { segments: 12 });
  // The head: six flat sides and a top.
  const radius = 0.011, low = 0.003, high = 0.012;
  const corner = index => [Math.cos(index * Math.PI / 3) * radius, Math.sin(index * Math.PI / 3) * radius];
  for (let side = 0; side < 6; side++) {
    const [a, b] = [corner(side), corner(side + 1)];
    mesh.quad(at(a[0], a[1], low), at(b[0], b[1], low), at(b[0], b[1], high), at(a[0], a[1], high), [a[0] + b[0], a[1] + b[1], 0]);
  }
  mesh.cap(Array.from({ length: 6 }, (_, index) => at(...corner(index), high)), Z);
  return mesh;
}

/** The scene's parts turned to face the camera: (x, y) becomes (y, -x). */
const turned = ([x, y, z]) => [y, -x, z];

/**
 * Every bolt of the scene as one instanced batch of the `bolt` mesh: the pipe's two flanges, the anchor bolts of the
 * four wall brackets and the screws of the vent's frame. Each instance is a point on a surface and the surface's
 * normal (in the layout's coordinates, before the turn), a size and a turn about its own axis. A placement moves the
 * template from where it stands, so its translation undoes the template's own offset: p - R (s O).
 */
export function buildBatches() {
  const bolts = [];
  const ring = (center, normal, radius, count, scale) => {
    const u = unit(cross(normal, Math.abs(normal[2]) < 0.9 ? Z : X)), v = cross(normal, u);
    for (let index = 0; index < count; index++) {
      const angle = (index + 0.5) / count * Math.PI * 2;
      bolts.push({ point: add(center, add(mul(u, radius * Math.cos(angle)), mul(v, radius * Math.sin(angle)))), normal, scale });
    }
  };
  // The pipe's floor flange and the flange where it enters the tall wall: eight bolts each, between pipe and rim.
  ring([0.14, 2.12, 0.03], Z, 0.11, 8, 1);
  ring([0.03, 2.12, 2.3], X, 0.11, 8, 1);
  // Anchor bolts through each bracket's leg: the duct's two on the tall wall, the bench's two on the low wall.
  for (const y of [0.5, 1.15]) for (const drop of [0.06, 0.19]) bolts.push({ point: [0.006, y, 1.97 - drop], normal: X, scale: 0.8 });
  for (const x of [0.7, 1.6]) for (const drop of [0.08, 0.25]) bolts.push({ point: [x, 0.006, 0.425 - drop], normal: Y, scale: 0.8 });
  // Screws at the corners and the middles of the vent's frame.
  for (const y of [1.555, 1.725, 1.895]) for (const z of [1.975, 2.12, 2.265]) {
    if (y === 1.725 && z === 2.12) continue;
    bolts.push({ point: [0.436, y, z], normal: X, scale: 0.5 });
  }
  const instances = bolts.map(({ point, normal, scale }, index) => {
    const n = turned(normal), p = turned(point);
    // The shortest turn from +Z to the normal, after a turn about +Z that differs per bolt (a hex head rarely lines up).
    const spin = index * 2.399963, half = [0, 0, Math.sin(spin / 2), Math.cos(spin / 2)];
    const axis = cross(Z, n), w = 1 + dot(Z, n);
    const length = Math.hypot(...axis, w);
    const align = [...mul(axis, 1 / length), w / length];
    const multiply = (a, b) => [
      a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1], a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
      a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3], a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]];
    // A number per bolt in [0, 1) that repeats from build to build.
    const random = (index * 0.6180339887 + 0.137) % 1;
    return {
      // The template's origin is on its axis, so R (s O) is s |O| along the normal.
      translation: add(p, mul(n, -scale * BOLT_ORIGIN[2])), rotation: multiply(align, half), scale: [scale, scale, scale],
      value: [random, index / bolts.length, 0, 1], tint: [0.8 + 0.3 * random, 0.8 + 0.3 * random, 0.8 + 0.3 * random],
    };
  });
  return [{ id: "bolts", mesh: "bolt", collision: "none", instances }];
}

/**
 * The lawn's ground from its TreeSpec document (`washroom-lawn.json`, see build.mjs): world points (already in the
 * scene's turned frame) and triangles, with smooth normals weighted by triangle area.
 */
export function lawnGround(mesh, ground) {
  const { positions, indices } = ground;
  const normals = new Array(positions.length).fill(0);
  for (let at = 0; at < indices.length; at += 3) {
    const [a, b, c] = [indices[at], indices[at + 1], indices[at + 2]].map(index => positions.slice(index * 3, index * 3 + 3));
    const face = cross([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    for (const index of [indices[at], indices[at + 1], indices[at + 2]]) for (let axis = 0; axis < 3; axis++) normals[index * 3 + axis] += face[axis];
  }
  for (let vertex = 0; vertex < positions.length / 3; vertex++) {
    const normal = normals.slice(vertex * 3, vertex * 3 + 3);
    mesh.vertex(positions.slice(vertex * 3, vertex * 3 + 3), Math.hypot(...normal) > 0 ? unit(normal) : Z);
  }
  mesh.indices.push(...indices);
  return mesh;
}

/**
 * `batches: false` leaves out the bolt template, for the scene as it was before it had instanced batches. `lawn` (a
 * TreeSpec lawn document) adds its ground as the mesh `lawn`, last. `daylight` raises the bench wall, with an open
 * clerestory and a shallow soffit above the bench.
 */
export function buildMeshes({ batches = true, lawn = null, daylight = false } = {}) {
  const meshes = [];
  const mesh = (name, color) => { const created = new Mesh(name, color); meshes.push(created); return created; };

  mesh("slab", [0.5, 0.48, 0.45]).box([-0.4, -0.55, -0.2], [2.45, 2.45, 0]);
  mesh("wall-tall", [0.52, 0.5, 0.47]).box([-0.25, -0.4, 0], [0, 2.3, 2.6]);
  const returnWall = mesh("wall-low", [0.52, 0.5, 0.47]);
  if (daylight) daylightWall(returnWall);
  else returnWall.box([0, -0.4, 0], [2.3, 0, 1.2]);

  // Tile: a panel on the tall wall, a worn remnant below it and a patch of floor.
  mesh("tiles-wall", [0.55, 0.7, 0.62])
    .box([0, 0.8, 0.75], [0.012, 2.0, 1.65])
    .box([0, 1.1, 0.3], [0.012, 1.7, 0.75]);
  mesh("tiles-floor", [0.55, 0.7, 0.62]).box([0, 0.6, 0], [0.6, 1.8, 0.012]);

  const planks = mesh("bench", [0.6, 0.42, 0.22]);
  for (const y of [0.03, 0.185, 0.34]) planks.box([0.45, y, 0.425], [1.85, y + 0.14, 0.47]);

  // Flat steel: the bench and duct brackets.
  const steel = mesh("brackets", [0.4, 0.4, 0.4]);
  for (const x of [0.7, 1.6]) bracket(steel, [x, 0, 0.425], Y, X, 0.46, 0.33);
  for (const y of [0.5, 1.15]) bracket(steel, [0, y, 1.97], X, Y, 0.4, 0.25);
  if (daylight) daylightFrame(steel);

  // Galvanized duct: a run along the tall wall, an elbow down onto the low wall, and the vent box.
  const duct = mesh("duct", [0.6, 0.62, 0.63]);
  duct.duct([[0.21, 1.5, 2.12], [0.21, -0.17, 2.12], [0.21, -0.17, 1.2]], X, 0.34, 0.3, { bend: 0.3 });
  for (const y of [1.488, 0.85, 0.15]) duct.box([0.028, y - 0.012, 1.958], [0.392, y + 0.012, 2.282]);
  for (const z of [1.8, 1.212]) duct.box([0.028, -0.332, z - 0.012], [0.392, -0.008, z + 0.012]);
  duct.box([0.02, 1.5, 1.92], [0.42, 1.95, 2.32]);
  // The louvered face: a frame and five slats tilted to shed water.
  for (const [low, high] of [[[1.54, 1.96], [1.91, 1.99]], [[1.54, 2.25], [1.91, 2.28]], [[1.54, 1.99], [1.57, 2.25]], [[1.88, 1.99], [1.91, 2.25]]]) {
    duct.box([0.42, low[0], low[1]], [0.436, high[0], high[1]]);
  }
  for (let slat = 0; slat < 5; slat++) {
    const z = 2.02 + slat * 0.052;
    duct.bar([0.44, 1.57, z], [0.44, 1.88, z], [0.6, 0, -0.8], 0.062, 0.004);
  }

  // Dark openings: behind the louvers and under the drain grate.
  const shadow = mesh("openings", [0.03, 0.03, 0.03]);
  shadow.box([0.42, 1.57, 1.99], [0.421, 1.88, 2.25]);
  shadow.tube([[1.2, 1.25, 0.002], [1.2, 1.25, 0.005]], 0.085, { segments: 24 });

  // One large rusted pipe: up from the floor beside the tile and into the tall wall. Detailed
  // plumbing (valves, taps, a sink) is left to ItemSpec.
  const pipe = mesh("pipe", [0.5, 0.33, 0.22]);
  pipe.tube([[0.14, 2.12, 0], [0.14, 2.12, 2.3], [0, 2.12, 2.3]], 0.09, { bend: 0.2, segments: 24, steps: 8 });
  // Rings: a floor flange, three couplings up the riser and a flange where it enters the wall.
  pipe.tube([[0.14, 2.12, 0], [0.14, 2.12, 0.03]], 0.13, { segments: 24 });
  for (const z of [0.55, 1.1, 1.65]) pipe.tube([[0.14, 2.12, z - 0.035], [0.14, 2.12, z + 0.035]], 0.108, { segments: 24 });
  pipe.tube([[0, 2.12, 2.3], [0.03, 2.12, 2.3]], 0.13, { segments: 24 });

  // The floor drain's grate: a rim and bars cut to the circle.
  const drain = mesh("drain", [0.35, 0.35, 0.36]);
  drain.torus([1.2, 1.25, 0.008], Z, 0.085, 0.007, { segments: 24, sides: 8 });
  for (let barIndex = -3; barIndex <= 3; barIndex++) {
    const offset = barIndex * 0.022, reach = Math.sqrt(0.085 ** 2 - offset ** 2);
    drain.box([1.2 - reach, 1.25 + offset - 0.006, 0.005], [1.2 + reach, 1.25 + offset + 0.006, 0.011]);
  }

  // A shallow soffit shades the bench and leaves the rest of the room open for inspection.
  if (daylight) {
    const soffit = mesh("soffit", [0.52, 0.5, 0.47]);
    soffit.box([0, -0.4, 2.5], [2.3, 0.8, 2.6]);
  }

  // Turn the layout to face the camera: (x, y) becomes (y, -x).
  for (const part of meshes) {
    for (const values of [part.positions, part.normals]) {
      for (let i = 0; i < values.length; i += 3) [values[i], values[i + 1]] = [values[i + 1], -values[i]];
    }
  }
  // The bolt template, last, so every other mesh keeps its place in the model.
  if (batches) bolt(mesh("bolt", [0.4, 0.4, 0.4]), BOLT_ORIGIN);
  if (lawn) lawnGround(mesh("lawn", [0.3, 0.48, 0.14]), lawn.ground);
  return meshes;
}

export function buildModel() {
  return writeGlb(buildMeshes());
}
