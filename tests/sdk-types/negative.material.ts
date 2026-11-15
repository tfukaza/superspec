// Every line below must fail strict TypeScript. `@ts-expect-error` makes tsc report any line that
// unexpectedly compiles, so this file is the failing-case suite for the generated SDK.
import {
  defineMaterial, layer, param, tileAndGrout, mix, add, select, vec2, vec3, world, surface, noise, seed,
  // @ts-expect-error a misspelled operation is not exported by the SDK
  fbmm,
} from "@superspec/material/v1";

// The brand symbol is private, so recipes cannot forge branded values.
// @ts-expect-error `kind` is not exported
import { kind } from "@superspec/material/v1";
void kind;

// Exported group result types are importable (and the Rust import check accepts them too).
import type { TileAndGroutGroup, ScalarInput } from "@superspec/material/v1";
declare const typedTiles: TileAndGroutGroup;
const joint: ScalarInput = typedTiles.joint;
void joint;

// Receiver feature queries take one of the documented selectors.
import { creaseDistance, edgeWear, oxide } from "@superspec/material/v1";
void creaseDistance("exposed", 30, 0.05);
// @ts-expect-error unknown selector
void creaseDistance("edges", 30, 0.05);
// @ts-expect-error the selector is a literal word, not a number
void creaseDistance(1, 30, 0.05);
const wear = edgeWear({ width: 0.014, minAngle: 30, depth: 0.002 });
const rust = oxide({ edgeWidth: 0.02 });
void [wear.coverage, wear.height, wear.distance, rust.edge, rust.surface, rust.spot, rust.combined];
// @ts-expect-error edge wear exposes coverage, height and distance
void wear.mask;

// Receiver normal adjustment: vector normals, a scalar weight, and a vec3 channel.
import { creaseNormal, adjustNormal, roundedEdges, normal } from "@superspec/material/v1";
const edges = roundedEdges({ radius: 0.014, minAngle: 30 });
const rounded = adjustNormal(normal, creaseNormal("convex", 30, 0.014), edges.weight);
void defineMaterial({ base: { baseColor: vec3(0.5), receiverNormal: rounded }, channels: { receiverNormal: edges.receiverNormal } });
void layer({ id: "lean", mask: 0.5, receiverNormal: vec3(1, 0, 0) });
// @ts-expect-error the weight is a scalar
adjustNormal(normal, normal, vec3(1));
// @ts-expect-error normals are three-component vectors
adjustNormal(surface, normal, 0.5);
// @ts-expect-error unknown selector
creaseNormal("rims", 30, 0.014);
// @ts-expect-error the receiver normal channel is a direction, not a scalar
void defineMaterial({ base: { baseColor: vec3(0.5) }, channels: { receiverNormal: 1 } });
// @ts-expect-error rounded edges expose distance, weight and receiverNormal
void edges.normal;

// A misspelled port or an unknown option is rejected.
// @ts-expect-error unknown layer option
layer({ id: "a", mask: 1, rougness: 0.3 });
// @ts-expect-error a layer needs a mask
layer({ id: "a" });
// @ts-expect-error a layer needs an id
layer({ mask: 1 });

// A missing required input is rejected.
// @ts-expect-error mix takes three arguments
mix(0, 1);
// @ts-expect-error noise needs a seed
noise(surface);
// @ts-expect-error base.baseColor is required
defineMaterial({ base: {} });
// @ts-expect-error a recipe needs a base
defineMaterial({});

// A channel type mismatch is rejected: a color cannot drive roughness.
defineMaterial({
  // @ts-expect-error roughness is a scalar
  base: { baseColor: vec3(1), roughness: vec3(1) },
});
// @ts-expect-error a scalar cannot be a base color
defineMaterial({ base: { baseColor: 0.5 } });
// @ts-expect-error opacity is a scalar
layer({ id: "b", mask: 1, opacity: vec3(0) });

// Vector dimensions must match; scalars broadcast.
// @ts-expect-error no overload mixes vec2 and vec3
add(vec2(1), vec3(1));
add(vec2(1), 2);
// @ts-expect-error length requires a vector
noise(1, seed("x"));

// Selection needs a boolean condition and matching branches.
// @ts-expect-error condition must be boolean
select(1, 2, 3);
// @ts-expect-error branches must have the same type
select(true, vec3(1), 0.5);

// Parameter defaults must match the parameter's kind.
// @ts-expect-error a scalar default is a number
param.scalar({ default: vec3(1) });
// @ts-expect-error a bool parameter takes no range
param.bool({ default: true, range: [0, 1] });
// @ts-expect-error a default is required
param.scalar({ range: [0, 1] });

// Groups check their arguments and expose only their declared fields.
// @ts-expect-error size is a vec2
tileAndGrout({ size: 1 });
const tiles = tileAndGrout({});
// @ts-expect-error tile groups expose face, joint, chip and substrate
tiles.grout;
// @ts-expect-error unknown group argument
tileAndGrout({ groutwidth: 0.01 });

// A height layer adds a delta or replaces the height, never both.
// @ts-expect-error add and replace are exclusive
layer({ id: "h", mask: 1, height: { add: 1, replace: 2 } });

// Swizzles stay inside the vector's dimension.
// @ts-expect-error a vec3 has no w component
world.w;
// @ts-expect-error a vec2 has no z component
surface.z;
void world.xz;
void surface.yx;

// Coordinate spaces: positions in different spaces never mix; literals, parameters and
// directions combine with any space, and generic helpers keep the space they receive.
import { sub, mul, dot, object, uv, gravity, boundsMin, type Vec3, type Space } from "@superspec/material/v1";
const lifted: Vec3<"world"> = add(world, mul(normal, 0.01));
const fromBase: Vec3<"world"> = sub(world, boundsMin);
const tinted: Vec3<"none"> = mix(vec3(0.2), param.vec3({ default: vec3(0.5) }), 0.5);
void [lifted, fromBase, tinted, dot(normal, gravity), noise(mul(object, 4), seed("o"))];
// @ts-expect-error world and object positions are different spaces
sub(world, object);
// @ts-expect-error surface coordinates and UVs are different spaces
add(surface, uv);
// @ts-expect-error a world-space result is not an object-space value
const misplaced: Vec3<"object"> = add(world, 1);
void misplaced;
const keep = <S extends Space>(p: Vec3<S>): Vec3<S> => mul(p, 2);
const scaled: Vec3<"world"> = keep(world);
void scaled;
// @ts-expect-error a generic helper keeps the space, which still conflicts with object
sub(keep(world), object);
const erase = (p: Vec3): Vec3 => mul(p, 2);
// @ts-expect-error a plain Vec3 helper result has no particular space, so it cannot mix with world
add(erase(world), world);
void add(erase(world), erase(object));
// @ts-expect-error spaces are the documented names
const unknown: Vec3<"screen"> = world;
void unknown;
// Feature frames, grids, strips and directional coordinates.
import { creaseTangent, creaseAdjacentNormal, cross, rectangularGrid, periodicStrips, directionalCoordinates } from "@superspec/material/v1";
const along: Vec3<"none"> = creaseTangent("exposed", 30, 0.05);
const facing = adjustNormal(normal, creaseAdjacentNormal("convex", 30, 0.02), 0.5);
const bricks = rectangularGrid({ size: vec2(0.24, 0.075), rowOffset: 0.5 });
const courses = periodicStrips({ position: surface.y, period: 0.075, gap: 0.01 });
const grain = directionalCoordinates({ direction: vec3(0, 0, 1), scale: vec2(0.2, 0.004) });
void [along, facing, cross(normal, gravity), bricks.sideDistance, bricks.cell, courses.joint, noise(grain.coordinates, seed("g"))];
// @ts-expect-error grids expose sideDistance, cornerDistance, random, cell and local
void bricks.mortar;
// @ts-expect-error strips run along one coordinate
periodicStrips({ position: surface });
// @ts-expect-error cross takes two three-component vectors
cross(surface, surface);