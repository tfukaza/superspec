/**
 * The washroom corner from `concept.png` as plain geometry: one mesh per material receiver.
 * Meters, Z up. The parts are laid out with the tall wall's face on the plane x = 0 and the low
 * wall's face on y = 0, then turned a quarter turn about Z, so the room opens toward (+x, -y):
 * the corner Material Studio's camera starts from. The tall wall faces -y and the low wall +x.
 */
import { Mesh, add, cross, mul, sub, unit, writeGlb } from "./geometry.mjs";

const X = [1, 0, 0], Y = [0, 1, 0], Z = [0, 0, 1];

/** A wall bracket: an arm leaving the wall at `origin`, a leg down the wall and a brace between. */
function bracket(mesh, origin, out, along, reach, drop, width = 0.04, thickness = 0.006) {
  const down = mul(Z, -1);
  const at = (o, d) => add(origin, add(mul(out, o), mul(down, d)));
  mesh.bar(at(0, thickness / 2), at(reach, thickness / 2), along, width, thickness);
  mesh.bar(at(thickness / 2, 0), at(thickness / 2, drop), along, width, thickness);
  mesh.bar(at(reach * 0.9, thickness), at(thickness, drop * 0.92), along, width * 0.7, thickness);
}

export function buildMeshes() {
  const meshes = [];
  const mesh = (name, color) => { const created = new Mesh(name, color); meshes.push(created); return created; };

  mesh("slab", [0.5, 0.48, 0.45]).box([-0.4, -0.55, -0.2], [2.45, 2.45, 0]);
  mesh("wall-tall", [0.52, 0.5, 0.47]).box([-0.25, -0.4, 0], [0, 2.3, 2.6]);
  mesh("wall-low", [0.52, 0.5, 0.47]).box([0, -0.4, 0], [2.3, 0, 1.2]);

  // Tile: a panel behind the sink, a worn remnant below it and a patch of floor.
  mesh("tiles-wall", [0.55, 0.7, 0.62])
    .box([0, 0.8, 0.75], [0.012, 2.0, 1.65])
    .box([0, 1.1, 0.3], [0.012, 1.7, 0.75]);
  mesh("tiles-floor", [0.55, 0.7, 0.62]).box([0, 0.6, 0], [0.6, 1.8, 0.012]);

  mesh("sink", [0.85, 0.83, 0.78]).basin([0.012, 1.0, 0.74], [0.5, 1.8, 1.0], 0.035, 0.045);

  const planks = mesh("bench", [0.6, 0.42, 0.22]);
  for (const y of [0.03, 0.185, 0.34]) planks.box([0.45, y, 0.425], [1.85, y + 0.14, 0.47]);

  // Flat steel: bench, sink and duct brackets, and the riser's wall clamps.
  const steel = mesh("brackets", [0.4, 0.4, 0.4]);
  for (const x of [0.7, 1.6]) bracket(steel, [x, 0, 0.425], Y, X, 0.46, 0.33);
  for (const y of [1.1, 1.7]) bracket(steel, [0.012, y, 0.74], X, Y, 0.42, 0.3);
  for (const y of [0.5, 1.15]) bracket(steel, [0, y, 1.97], X, Y, 0.4, 0.25);
  for (const z of [0.75, 1.25, 2.2]) {
    steel.box([0, 2.05, z - 0.03], [0.008, 2.19, z + 0.03]);
    steel.box([0.008, 2.086, z - 0.02], [0.134, 2.154, z + 0.02]);
  }

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

  // Rusted supply pipes: the riser and its run along the top of the wall, and the branch to the tap.
  const pipes = mesh("pipes", [0.5, 0.33, 0.22]);
  pipes.tube([[0.1, 2.12, 0], [0.1, 2.12, 2.48], [0.1, 1.3, 2.48], [0, 1.3, 2.48]], 0.028, { bend: 0.075, segments: 16 });
  pipes.tube([[0.1, 2.12, 1.47], [0.1, 1.4, 1.47], [0.1, 1.4, 1.39]], 0.024, { bend: 0.06, segments: 16 });
  for (const z of [0.5, 1.47, 2.05]) pipes.tube([[0.1, 2.12, z - 0.035], [0.1, 2.12, z + 0.035]], 0.037, { segments: 16 });
  for (const y of [1.98, 1.72, 1.52]) pipes.tube([[0.1, y - 0.022, 1.47], [0.1, y + 0.022, 1.47]], 0.032, { segments: 16 });
  pipes.tube([[0.1, 1.72, 2.48], [0.1, 1.78, 2.48]], 0.037, { segments: 16 });

  // The valve: a brass body and bonnet under a red handwheel, tipped toward the room.
  const tilt = unit([0.6, 0, 0.8]);
  const brass = mesh("valve", [0.62, 0.48, 0.25]);
  brass.tube([[0.1, 1.78, 1.47], [0.1, 1.92, 1.47]], 0.041, { segments: 16 });
  brass.tube([[0.1, 1.85, 1.47], add([0.1, 1.85, 1.47], mul(tilt, 0.115))], 0.021, { segments: 12 });
  brass.tube([add([0.1, 1.85, 1.47], mul(tilt, 0.05)), add([0.1, 1.85, 1.47], mul(tilt, 0.075))], 0.03, { segments: 6 });
  const wheel = mesh("handwheel", [0.7, 0.12, 0.1]);
  const hub = add([0.1, 1.85, 1.47], mul(tilt, 0.125));
  wheel.torus(hub, tilt, 0.062, 0.0095);
  for (const spoke of [Y, cross(tilt, Y)]) wheel.bar(sub(hub, mul(spoke, 0.06)), add(hub, mul(spoke, 0.06)), cross(tilt, spoke), 0.015, 0.008);
  wheel.tube([sub(hub, mul(tilt, 0.008)), add(hub, mul(tilt, 0.012))], 0.018, { segments: 12 });

  // Chrome: the tap's spout and the sink's trap.
  const chrome = mesh("chrome", [0.75, 0.76, 0.78]);
  chrome.tube([[0.1, 1.4, 1.4], [0.1, 1.4, 1.31], [0.28, 1.4, 1.31], [0.28, 1.4, 1.2]], 0.018, { bend: 0.05, segments: 14 });
  chrome.tube([[0.28, 1.4, 1.215], [0.28, 1.4, 1.19]], 0.023, { segments: 14 });
  chrome.tube([[0.26, 1.4, 0.74], [0.26, 1.4, 0.42], [0.26, 1.56, 0.42], [0.26, 1.56, 0.57], [0.012, 1.56, 0.57]], 0.022, { bend: 0.065, segments: 14 });
  chrome.tube([[0.26, 1.4, 0.66], [0.26, 1.4, 0.7]], 0.031, { segments: 14 });
  chrome.tube([[0.26, 1.4, 0.5], [0.26, 1.4, 0.53]], 0.03, { segments: 14 });
  chrome.tube([[0.012, 1.56, 0.57], [0.022, 1.56, 0.57]], 0.05, { segments: 18 });

  // The floor drain's grate: a rim and bars cut to the circle.
  const drain = mesh("drain", [0.35, 0.35, 0.36]);
  drain.torus([1.2, 1.25, 0.008], Z, 0.085, 0.007, { segments: 24, sides: 8 });
  for (let barIndex = -3; barIndex <= 3; barIndex++) {
    const offset = barIndex * 0.022, reach = Math.sqrt(0.085 ** 2 - offset ** 2);
    drain.box([1.2 - reach, 1.25 + offset - 0.006, 0.005], [1.2 + reach, 1.25 + offset + 0.006, 0.011]);
  }

  // Turn the layout to face the camera: (x, y) becomes (y, -x).
  for (const turned of meshes) {
    for (const values of [turned.positions, turned.normals]) {
      for (let i = 0; i < values.length; i += 3) [values[i], values[i + 1]] = [values[i + 1], -values[i]];
    }
  }
  return meshes;
}

export function buildModel() {
  return writeGlb(buildMeshes());
}
