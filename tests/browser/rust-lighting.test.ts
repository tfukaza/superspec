import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { DEFAULT_CONTOURS, DEFAULT_LIGHTING, LightingError, contoursOf, defaultLighting, formOf, hexFromLinear, lightingFromQuery, linearFromHex, paintedOf, parseLighting, projectLighting, statesAmbientOcclusion,
  storedLighting, sunAngles, sunDirection, sunOf, type Lighting } from "../../browser/renderer/lighting.ts";

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

test("painted finishing is a scene setting, held only when it differs from the default, as the Rust record writes it", () => {
  assert.deepEqual(paintedOf(DEFAULT_LIGHTING), { toneMapping: false, ambientOcclusion: true });
  assert.equal(parseLighting({ version: 1, painted: { toneMapping: false } }).painted, undefined);
  const toned = parseLighting({ version: 1, painted: { toneMapping: true } });
  assert.deepEqual(toned.painted, { toneMapping: true, ambientOcclusion: true });
  assert.deepEqual(paintedOf(toned), toned.painted);
  assert.throws(() => parseLighting({ version: 1, painted: { tint: 1 } }), (error: LightingError) => error.findings[0].field === "painted.tint");
  assert.throws(() => parseLighting({ version: 1, painted: { toneMapping: 1 } }), (error: LightingError) => error.findings[0].field === "painted.toneMapping");
});

test("form light and contours are off by default and held only when set, as the Rust record writes them", () => {
  assert.deepEqual(formOf(DEFAULT_LIGHTING.lights[0].shadow), { form: 0, fill: 0 });
  assert.equal(contoursOf(DEFAULT_LIGHTING).enabled, false);
  assert.deepEqual(parseLighting({ version: 1, contours: { enabled: false }, lights: [{ shadow: { form: 0, formFill: 0 } }] }), DEFAULT_LIGHTING);
  const drawn = parseLighting({ version: 1, contours: { enabled: true, width: 2.5 }, lights: [{ shadow: { form: 0.35 } }] });
  assert.deepEqual(drawn.contours, { ...DEFAULT_CONTOURS, enabled: true, width: 2.5 });
  assert.deepEqual(formOf(drawn.lights[0].shadow), { form: 0.35, fill: 0 });
  assert.equal("formFill" in drawn.lights[0].shadow, false);
  assert.deepEqual(parseLighting(storedLighting(drawn, false)), drawn);
  // The line is the surface's own color, darkened, with no tint unless one is stated.
  assert.equal(DEFAULT_CONTOURS.color, undefined);
  assert.equal("color" in drawn.contours!, false);
  const tinted = parseLighting({ version: 1, contours: { color: [0.5, 0.4, 0.3], darken: 1 } });
  assert.deepEqual(tinted.contours, { ...DEFAULT_CONTOURS, darken: 1, color: [0.5, 0.4, 0.3] });
  assert.deepEqual(lightingFromQuery(tinted, new URLSearchParams("")).contours, tinted.contours);
  assert.throws(() => parseLighting({ version: 1, contours: { darken: 2 } }), (error: LightingError) => error.findings[0].field === "contours.darken");
  // Page parameters are session overrides of both, and an untouched record stays the default.
  const query = (text: string): URLSearchParams => new URLSearchParams(text);
  const session = lightingFromQuery(defaultLighting(), query("contours=on&contourwidth=3&shadowform=0.4"));
  assert.deepEqual(session.contours, { ...DEFAULT_CONTOURS, enabled: true, width: 3 });
  assert.equal(session.lights[0].shadow.form, 0.4);
  assert.equal(lightingFromQuery(drawn, query("contours=off&shadowform=0")).contours?.enabled, false);
  assert.equal(lightingFromQuery(drawn, query("")).lights[0].shadow.form, 0.35);
  assert.doesNotThrow(() => parseLighting(session));
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

test("ao=on turns ambient occlusion on over a record that has it off", () => {
  const off = defaultLighting();
  off.ambientOcclusion.enabled = false;
  assert.equal(lightingFromQuery(off, new URLSearchParams("ao=on")).ambientOcclusion.enabled, true);
  assert.equal(lightingFromQuery(off, new URLSearchParams("")).ambientOcclusion.enabled, false);
});

test("sun angles and directions convert both ways", () => {
  // Independently known directions: +X on the horizon, +Y on the horizon, overhead, and halfway up toward -X.
  const near = (actual: number[], expected: number[]): void => actual.forEach((value, axis) => assert.ok(Math.abs(value - expected[axis]) < 1e-12, `${actual} is not ${expected}`));
  near(sunDirection(0, 0), [1, 0, 0]);
  near(sunDirection(90, 0), [0, 1, 0]);
  near(sunDirection(123, 90), [0, 0, 1]);
  near(sunDirection(180, 45), [-Math.SQRT1_2, 0, Math.SQRT1_2]);
  assert.deepEqual(sunAngles([0, -1, 0]), { azimuth: 270, elevation: 0 });
  const shipped = sunAngles(DEFAULT_LIGHTING.lights[0].direction);
  // The shipped sun stands at (-3, -5, 8): behind -X and -Y, well above the horizon.
  assert.ok(Math.abs(shipped.azimuth - (180 + Math.atan(5 / 3) * 180 / Math.PI)) < 1e-9);
  assert.ok(Math.abs(shipped.elevation - Math.atan(8 / Math.hypot(3, 5)) * 180 / Math.PI) < 1e-9);
  near(sunDirection(shipped.azimuth, shipped.elevation), DEFAULT_LIGHTING.lights[0].direction);
  // Every direction made from angles is a unit vector the record admits.
  for (const [azimuth, elevation] of [[0, 0], [37.5, -20], [359, 89.9], [200, -90]]) {
    const lighting = defaultLighting();
    lighting.lights[0].direction = sunDirection(azimuth, elevation);
    assert.doesNotThrow(() => parseLighting(lighting));
  }
});

test("colors convert between the record's linear RGB and a color control's hex", () => {
  assert.equal(hexFromLinear([0, 0, 0]), "000000");
  assert.equal(hexFromLinear([1, 1, 1]), "ffffff");
  // The shipped tints are stated in the record's comments as #2f6fe0 and #3f4fd8, the sun as #ffefdb.
  assert.equal(hexFromLinear(DEFAULT_LIGHTING.lights[0].shadow.tint), "2f6fe0");
  assert.equal(hexFromLinear(DEFAULT_LIGHTING.lights[0].shadow.tintAlt), "3f4fd8");
  assert.equal(hexFromLinear(DEFAULT_LIGHTING.lights[0].color), "ffefdb");
  assert.equal(hexFromLinear(DEFAULT_LIGHTING.ambient.ground), "777768");
  for (const hex of ["123456", "fedcba", "808080", "0a0b0c"]) assert.equal(hexFromLinear(linearFromHex(hex)), hex);
});

test("a project stores no record for the shipped look and says ambient occlusion only when the author chose it", () => {
  assert.equal(storedLighting(defaultLighting(), false), undefined);
  assert.equal(statesAmbientOcclusion(undefined), false);
  assert.equal(statesAmbientOcclusion({ version: 1, ambientOcclusion: { radius: 1 } }), false);
  assert.equal(statesAmbientOcclusion({ version: 1, ambientOcclusion: { enabled: true } }), true);

  // The shipped look with ambient occlusion chosen on is a record: it states the choice.
  const chosen = storedLighting(defaultLighting(), true)!;
  assert.equal(statesAmbientOcclusion(chosen), true);
  assert.deepEqual(parseLighting(chosen), DEFAULT_LIGHTING);

  // Another edit leaves ambient occlusion unsaid, and the stored record still reads back as the edited one.
  const brighter = defaultLighting();
  brighter.exposure = 1.5;
  const stored = storedLighting(brighter, false)!;
  assert.equal(statesAmbientOcclusion(stored), false);
  assert.deepEqual(stored.ambientOcclusion, { radius: 0.35, intensity: 2, thickness: 0.5 });
  assert.deepEqual(parseLighting(stored), brighter);
  assert.deepEqual(projectLighting({ lighting: JSON.parse(JSON.stringify(stored)) }), brighter);

  // Turned off is always said, chosen or not: silence would mean on.
  const off = defaultLighting();
  off.ambientOcclusion.enabled = false;
  assert.equal(statesAmbientOcclusion(storedLighting(off, false)), true);
  assert.deepEqual(parseLighting(storedLighting(off, false)), off);
});

test("a hex tint override is the color the record's default states", () => {
  const tinted = lightingFromQuery(defaultLighting(), new URLSearchParams("shadowtint=2f6fe0&shadowtint2=3f4fd8")).lights[0].shadow;
  const shipped = DEFAULT_LIGHTING.lights[0].shadow;
  for (let channel = 0; channel < 3; channel++) {
    assert.ok(Math.abs(tinted.tint[channel] - shipped.tint[channel]) < 1e-6);
    assert.ok(Math.abs(tinted.tintAlt[channel] - shipped.tintAlt[channel]) < 1e-6);
  }
});
