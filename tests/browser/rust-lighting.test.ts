import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DEFAULT_LIGHTING, LightingError, defaultLighting, lightingFromQuery, parseLighting, projectLighting, sunOf, type Lighting } from "../../browser/renderer/lighting.ts";

/** The fixtures the Rust record is checked against (`crates/model/src/lighting.rs`). */
const fixture = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../crates/model/src/${name}`, import.meta.url), "utf8"));

test("the viewer's default is the Rust record's default", () => {
  assert.deepEqual(DEFAULT_LIGHTING, fixture("lighting.default.json"));
  assert.deepEqual(parseLighting(fixture("lighting.default.json")), DEFAULT_LIGHTING);
  assert.deepEqual(parseLighting({ version: 1 }), DEFAULT_LIGHTING);
  const sun = sunOf(DEFAULT_LIGHTING)!;
  // The viewer's sun stood at (-3, -5, 8) from the scene.
  assert.deepEqual(sun.direction.map(axis => Math.round(axis * Math.sqrt(98) * 1e9) / 1e9), [-3, -5, 8]);
});

test("a partial record keeps the other defaults and is a copy", () => {
  const lighting = parseLighting({ version: 1, ambient: { intensity: 0.2 }, lights: [{ shadow: { style: "plain", noise: { seed: 3 } } }] });
  const expected = defaultLighting();
  expected.ambient.intensity = 0.2;
  expected.lights[0].shadow.style = "plain";
  expected.lights[0].shadow.noise.seed = 3;
  assert.deepEqual(lighting, expected);
  lighting.lights[0].shadow.tint[0] = 1;
  assert.notEqual(DEFAULT_LIGHTING.lights[0].shadow.tint[0], 1);
  assert.deepEqual(parseLighting({ version: 1, lights: [] }).lights, []);
  assert.equal(sunOf(parseLighting({ version: 1, lights: [] })), undefined);
});

test("every record Rust refuses is refused, by the same field", () => {
  const cases = fixture("lighting.invalid.json") as { name: string; record: unknown; field?: string; structural?: boolean }[];
  assert.ok(cases.length >= 12);
  for (const { name, record, field, structural } of cases) {
    let error: unknown;
    try { parseLighting(record); } catch (caught) { error = caught; }
    assert.ok(error instanceof LightingError, `${name} must be refused`);
    assert.ok(error.findings.length >= 1 && error.message.startsWith("Invalid lighting record: "), name);
    if (!structural) assert.deepEqual(error.findings.map(finding => finding.field), [field], name);
  }
});

test("limits are inclusive and findings are reported together", () => {
  const sun = { shadow: { softness: 2, noise: { scale: 0.01 } } };
  assert.doesNotThrow(() => parseLighting({ version: 1, exposure: 100, ambientOcclusion: { intensity: 0 }, lights: [sun] }));
  assert.throws(() => parseLighting({ version: 2, exposure: -1, bloom: true, lights: [{ shadow: { wrap: 2 } }] }), (error: unknown) => {
    assert.ok(error instanceof LightingError);
    assert.deepEqual(error.findings.map(finding => finding.field), ["bloom", "version", "exposure", "lights[0].shadow.wrap"]);
    return true;
  });
});

test("a project's lighting field is its record; without one it has the shipped look", () => {
  assert.deepEqual(projectLighting({ kind: "material" }), DEFAULT_LIGHTING);
  assert.equal(projectLighting({ lighting: { version: 1, exposure: 2 } }).exposure, 2);
  assert.throws(() => projectLighting({ lighting: { exposure: 2 } }), LightingError);
  assert.throws(() => projectLighting({ lighting: null }), LightingError);
});

test("page parameters override the record without changing it", () => {
  const query = (text: string): URLSearchParams => new URLSearchParams(text);
  const record = defaultLighting();
  assert.deepEqual(lightingFromQuery(record, query("")), DEFAULT_LIGHTING);
  assert.deepEqual(lightingFromQuery(record, query("preview=bake&resolution=256")), DEFAULT_LIGHTING);

  const plain = lightingFromQuery(record, query("shadowstyle=plain&ao=off"));
  assert.equal(plain.lights[0].shadow.style, "plain");
  assert.equal(plain.lights[0].castsShadow, true);
  assert.equal(plain.ambientOcclusion.enabled, false);
  // Everything else is still the record's.
  const rest = structuredClone(plain);
  rest.lights[0].shadow.style = "illustration"; rest.ambientOcclusion.enabled = true;
  assert.deepEqual(rest, DEFAULT_LIGHTING);
  assert.deepEqual(record, DEFAULT_LIGHTING);

  assert.equal(lightingFromQuery(record, query("shadows=off")).lights[0].castsShadow, false);
  const authoredPlain: Lighting = defaultLighting();
  authoredPlain.lights[0].shadow.style = "plain"; authoredPlain.ambientOcclusion.enabled = false;
  assert.equal(lightingFromQuery(authoredPlain, query("")).lights[0].shadow.style, "plain");
  assert.equal(lightingFromQuery(authoredPlain, query("shadowstyle=illustration")).lights[0].shadow.style, "illustration");
  assert.equal(lightingFromQuery(authoredPlain, query("")).ambientOcclusion.enabled, false);

  const tuned = lightingFromQuery(record, query("shadowblur=0.05&shadowdark=7&shadowtint=ffffff&shadowtint2=000000&shadowseed=11.9&shadownoise=x&aoradius=0.8&aointensity=9&aothickness=0.2"));
  const shadow = tuned.lights[0].shadow;
  assert.equal(shadow.softness, 0.05);
  assert.equal(shadow.darkness, 1, "values are clamped to their range");
  assert.deepEqual(shadow.tint.map(channel => Math.round(channel * 1e6) / 1e6), [1, 1, 1]);
  assert.deepEqual(shadow.tintAlt, [0, 0, 0]);
  assert.equal(shadow.noise.seed, 11);
  assert.equal(shadow.noise.amount, 0.6, "an unreadable value leaves the record's");
  assert.deepEqual(tuned.ambientOcclusion, { enabled: true, radius: 0.8, intensity: 4, thickness: 0.2 });
  // An overridden record is still a valid one.
  assert.doesNotThrow(() => parseLighting(tuned));
});

test("a hex tint override is the color the record's default states", () => {
  const tinted = lightingFromQuery(defaultLighting(), new URLSearchParams("shadowtint=2f6fe0&shadowtint2=3f4fd8")).lights[0].shadow;
  const shipped = DEFAULT_LIGHTING.lights[0].shadow;
  for (let channel = 0; channel < 3; channel++) {
    assert.ok(Math.abs(tinted.tint[channel] - shipped.tint[channel]) < 1e-6);
    assert.ok(Math.abs(tinted.tintAlt[channel] - shipped.tintAlt[channel]) < 1e-6);
  }
});
