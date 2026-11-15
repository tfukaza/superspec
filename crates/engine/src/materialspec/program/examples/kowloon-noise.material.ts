// Noise fields shared by the Kowloon concrete and steel studies, ported from their GLSL.
// Value noise has one lattice cell per unit; a key (from seed("name")) selects the lattice.
import {
  valueNoise, hash, vec2, vec3, add, sub, mul, div, abs, smoothstep, softThreshold, cos, dot, max,
  floor, fract, sin, pow, exp, clamp, min, length,
} from "@superspec/material/v1";
import type { Scalar, ScalarInput, Vec2, Vec3 } from "@superspec/material/v1";

/** Value noise centered on zero: `noise(p) - 0.5`. */
const centered = (p: Vec2, key: ScalarInput): Scalar => sub(valueNoise(p, key), 0.5);

/** The study's octave step: rotate by mat2(0.8, -0.6, 0.6, 0.8), scale by 2.03, offset. */
const turn = (p: Vec2): Vec2 =>
  add(mul(vec2(add(mul(p.x, 0.8), mul(p.y, 0.6)), sub(mul(p.y, 0.8), mul(p.x, 0.6))), 2.03), vec2(13.7, 9.2));

const octaves = (a: Vec2, b: Vec2, c: Vec2, key: ScalarInput): Scalar =>
  div(add(add(mul(valueNoise(a, key), 0.5), mul(valueNoise(b, key), 0.14)), mul(valueNoise(c, key), 0.0392)), 0.6792);

/** Three rotated octaves with gain 0.28, normalized to [0, 1] (the study's `concreteFractal`). */
export const fractal = (p: Vec2, key: ScalarInput): Scalar => octaves(p, turn(p), turn(turn(p)), key);

const fractureDetail = (p: Vec2, key: ScalarInput): Scalar =>
  add(
    add(mul(centered(mul(p, 13), key), 0.2), mul(centered(add(mul(p, 31), 17.3), key), 0.11)),
    add(mul(centered(add(mul(p, 73), 39.1), key), 0.06), mul(centered(add(mul(p, 151), 81.7), key), 0.025)),
  );

/** The study's two-fractal domain warp, centered on zero. */
export const domainWarp = (p: Vec2, key: ScalarInput): Vec2 =>
  sub(vec2(fractal(add(mul(p, 2.3), vec2(7.1, 31.8)), key), fractal(add(mul(p, 2.3), vec2(53.2, 9.4)), key)), 0.5);

/** Fine erosion lives at the contour, leaving clump interiors coherent. */
const eroded = (broad: Scalar, p: Vec2, jaggedness: ScalarInput, key: ScalarInput): Scalar =>
  add(broad, mul(mul(fractureDetail(p, key), jaggedness), sub(1, smoothstep(0.045, 0.18, abs(sub(broad, 0.5))))));

/** Warped fractal clumps with eroded contours (the study's `concreteFracture`). */
export const fracture = (p: Vec2, jaggedness: ScalarInput, key: ScalarInput): Scalar =>
  eroded(fractal(add(p, mul(domainWarp(p, key), mul(0.18, jaggedness))), key), p, jaggedness, key);

/**
 * The study's `concreteEdge`: a transition at 0.5 that is `width` wide on each side, widened by
 * the field's own variation across the sample (its `fwidth`).
 */
export const contour = (field: ScalarInput, width: ScalarInput): Scalar =>
  softThreshold(field, 0.5, mul(width, 2));

/** The chipped shoulder shared by joints and exposed edges: one at the line, zero at `width`. */
export const shoulder = (distance: ScalarInput, width: ScalarInput): Scalar =>
  sub(1, smoothstep(mul(width, 0.4), width, distance));

/** Hue variation along a direction in degrees, scaled by luminance (the study's pigment). */
export const pigment = (color: Vec3, at: Vec2, amount: ScalarInput, degrees: ScalarInput, key: ScalarInput): Vec3 =>
  hued(color, cos(add(vec3(mul(degrees, 0.017453292519943295)), vec3(0, -2.0943951, 2.0943951))), sub(mul(fractal(add(mul(at, 4.68), vec2(41.2, 3.7)), key), 2), 0.7), amount);

const luminance = (color: Vec3): Scalar => dot(color, vec3(0.2126, 0.7152, 0.0722));
const hued = (color: Vec3, direction: Vec3, hue: Scalar, amount: ScalarInput): Vec3 =>
  max(vec3(0), add(color, mul(sub(direction, vec3(luminance(direction))), mul(mul(hue, amount), luminance(color)))));

/** Three independent lattice values for a cell, like the study's `concreteHash`. */
export const cellRandom = (cell: Vec2, key: ScalarInput): Vec3 =>
  vec3(hash(vec3(cell, 0), key), hash(vec3(cell, 1), key), hash(vec3(cell, 2), key));

/** Signed distance in meters from a point to an axis-aligned box. */
export const boxDistance = (offset: Vec2, halfSize: Vec2): Scalar =>
  boxFrom(sub(abs(offset), halfSize));
const boxFrom = (q: Vec2): Scalar => add(length(max(q, vec2(0))), min(max(q.x, q.y), 0));

/** GLSL `mod`: `x - period * floor(x / period)`, always in [0, period) for positive periods. */
export const modulo = (x: ScalarInput, period: ScalarInput): Scalar => sub(x, mul(period, floor(div(x, period))));

/** The study's periodic 1D grime noise: `cells` lattice values over one repeat of u. */
const periodicHash = (index: Scalar, cells: ScalarInput, row: ScalarInput): Scalar =>
  fract(mul(sin(add(mul(modulo(index, cells), 127.1), mul(row, 311.7))), 43758.5453));
const periodic = (u: ScalarInput, cells: ScalarInput, row: ScalarInput): Scalar =>
  periodicBlend(floor(mul(u, cells)), fract(mul(u, cells)), cells, row);
const periodicBlend = (cell: Scalar, f: Scalar, cells: ScalarInput, row: ScalarInput): Scalar =>
  add(mul(periodicHash(cell, cells, row), sub(1, mul(mul(f, f), sub(3, mul(f, 2))))), mul(periodicHash(add(cell, 1), cells, row), mul(mul(f, f), sub(3, mul(f, 2)))));

/**
 * The study's top-grime mask (grime.ts): a band below the top edge and drips, over eight meters
 * horizontally (`u` is meters / 8) and one meter down (`depth` in meters below the top).
 */
export const topGrime = (u: ScalarInput, depth: ScalarInput): Scalar =>
  grimeFrom(u, clamp(depth, 0, 1), periodic(u, 13, 7), periodic(u, 37, 17));
const grimeFrom = (u: ScalarInput, depth: Scalar, broad: Scalar, drop: Scalar): Scalar =>
  min(1, mul(mul(add(
    mul(exp(div(neg1(depth), add(0.045, mul(broad, 0.11)))), add(0.42, mul(broad, 0.5))),
    mul(mul(pow(periodic(u, 113, 31), 3), exp(div(neg1(depth), add(0.12, mul(drop, 0.6))))), 0.7),
  ), add(0.72, mul(periodic(u, 239, add(floor(mul(depth, 255)), 53)), 0.28))), clamp(div(sub(1, depth), 0.2), 0, 1)));
const neg1 = (x: Scalar): Scalar => mul(x, -1);
