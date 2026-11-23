import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { DEFAULT_GRAPHICS_PLATFORM, GRAPHICS_PLATFORMS, GRAPHICS_PRESETS, GRAPHICS_VARIANTS, defaultGraphicsPreset,
  graphicsPackSuffix, graphicsPackUrl, resolveGraphicsParameters, resolveGraphicsSettings } from "../adapters/three/src/graphicsSettings.ts";
import { resolveRenderDiagnostics, resolveRenderProfile, upgradeRenderProfile, writeRenderProfile } from "../adapters/three/src/renderProfile.ts";
import { detectBrowserGraphicsPlatform, detectGraphicsPlatform, graphicsPlatformSignals } from "../adapters/three/src/graphicsPlatform.ts";

const data = JSON.parse(await readFile(new URL("../adapters/three/src/graphicsProfiles.json", import.meta.url), "utf8"));
const presets = GRAPHICS_PRESETS.map(preset => preset.id);
const at = (parameters, id) => id.split(".").reduce((group, name) => group[name], parameters);
const nested = (id, value) => id.split(".").reduceRight((inner, name) => ({ [name]: inner }), value);

// The budgets and feature defaults as they were written before parameters existed.
const before = {
  "very-low": { renderScale: 1, geometryLodBias: 0.55, materialLodBias: 0.5, materialAnisotropy: 1, sunShadowMapSize: 512, fixtureShadowMapSize: 0 },
  low: { renderScale: 1, geometryLodBias: 0.75, materialLodBias: 0.7, materialAnisotropy: 2, sunShadowMapSize: 1024, fixtureShadowMapSize: 0 },
  medium: { renderScale: 1, geometryLodBias: 1, materialLodBias: 1, materialAnisotropy: undefined, sunShadowMapSize: undefined, fixtureShadowMapSize: undefined },
  high: { renderScale: 1, geometryLodBias: 1.2, materialLodBias: 1.25, materialAnisotropy: 8, sunShadowMapSize: 4096, fixtureShadowMapSize: undefined },
  "very-high": { renderScale: 1, geometryLodBias: 1.4, materialLodBias: 1.5, materialAnisotropy: 16, sunShadowMapSize: 4096, fixtureShadowMapSize: 1024 },
};
// Deliberate changes since: draw-count budgets that follow the preset, not a version 1 override group.
// Low and Very Low spend more triangles to join LOD bands and leave distant window glass out.
const drawBudgets = {
  "very-low": { lodBandMergeTriangles: 16000, glazingDistance: 32 }, low: { lodBandMergeTriangles: 8000, glazingDistance: 16 },
  medium: { lodBandMergeTriangles: 1000, glazingDistance: undefined }, high: { lodBandMergeTriangles: 1000, glazingDistance: undefined },
  "very-high": { lodBandMergeTriangles: 1000, glazingDistance: undefined },
};
for (const preset of Object.keys(before)) Object.assign(before[preset], drawBudgets[preset]);
const mobile = preset => preset === "low" || preset === "very-low";
function settingsBefore(preset, overrides = {}) {
  const geometry = before[overrides.geometry ?? preset], materials = before[overrides.materials ?? preset];
  const shadows = before[overrides.shadows ?? preset];
  return { renderScale: overrides.renderScale ?? 1, geometryLodBias: geometry.geometryLodBias, materialLodBias: materials.materialLodBias,
    materialAnisotropy: materials.materialAnisotropy, sunShadowMapSize: shadows.sunShadowMapSize, fixtureShadowMapSize: shadows.fixtureShadowMapSize,
    ...drawBudgets[preset] };
}
function diagnosticsBefore(preset, overrides = {}) {
  const base = resolveRenderDiagnostics(), materials = overrides.materials ?? preset;
  return {
    ...base,
    visibility: { ...base.visibility, ...(mobile(overrides.geometry ?? preset) ? { occlusionGrid: "coarse" } : {}) },
    materials: { ...base.materials, ...(mobile(materials) ? { grain: false, reflectionProbes: false, shopFinish: materials === "low" } : {}) },
    lighting: { ...base.lighting, ...(mobile(preset) ? { ambientOcclusion: "off", contours: "off" } : {}) },
    effects: { ...base.effects, ...(mobile(preset) ? { bloom: "off", outline: "off", edgeBoost: "off", displayAA: false, shopArt: "off" } : {}) },
    // One deliberate change from before: the first view's shaders are linked ahead of its first
    // frame at every preset. Low and Very Low used to link them inside that frame, because the
    // preparation hung on warmup. Their warm-up after the first frame stays off.
    loading: { ...base.loading, ...(mobile(preset) ? { warmup: false, textureWarmupMiB: 0 } : {}), firstFrameShaders: true },
  };
}

test("the data file declares the adapter's presets, platforms and variants, and only valid values", () => {
  assert.deepEqual(Object.keys(data.presets), presets);
  assert.deepEqual(Object.keys(data.platforms), GRAPHICS_PLATFORMS.map(platform => platform.id));
  assert.deepEqual(Object.keys(data.variants), [...GRAPHICS_VARIANTS]);
  assert.equal(data.defaultPlatform, DEFAULT_GRAPHICS_PLATFORM);
  assert.deepEqual(data.platforms.baseline, { about: data.platforms.baseline.about, defaultPreset: "medium", values: {}, caps: {} });
  const valid = (id, value) => {
    const declared = data.parameters[id];
    assert.ok(declared, id + " is declared");
    // Resolving an override runs the adapter's own validation of the declared range.
    assert.equal(at(resolveGraphicsParameters({ overrides: nested(id, value), publishedVariants: GRAPHICS_VARIANTS }).parameters, id), value, id);
  };
  for (const [id, declared] of Object.entries(data.parameters)) {
    assert.equal(typeof declared.consumed, "boolean", id);
    assert.ok(["build", "run"].includes(declared.class), id);
  }
  for (const preset of Object.values(data.presets)) for (const [id, value] of Object.entries(preset.values)) valid(id, value);
  for (const platform of Object.values(data.platforms)) {
    for (const [id, value] of Object.entries(platform.values)) valid(id, value);
    for (const id of Object.keys(platform.caps)) assert.ok(data.parameters[id], id);
    assert.ok(presets.includes(platform.defaultPreset));
  }
  for (const variant of Object.values(data.variants)) for (const id of Object.keys(variant.assets)) assert.ok(data.parameters[id], id);
  for (const ids of Object.values(data.legacyGroups)) for (const id of ids) assert.equal(data.parameters[id].consumed, true, id);
  // Reserved names have no value shape yet, so nothing resolves them.
  for (const id of data.reserved) {
    assert.equal(data.parameters[id], undefined);
    assert.throws(() => resolveGraphicsParameters({ overrides: nested(id, 1) }), /Unknown overrides/);
  }
  const resolved = resolveGraphicsParameters();
  for (const [id, declared] of Object.entries(data.parameters)) assert.equal(at(resolved.parameters, id), data.presets.medium.values[id] ?? declared.default, id);
});

test("every preset and override group resolves to the values it had before parameters", () => {
  assert.equal(defaultGraphicsPreset(), "medium");
  assert.deepEqual(resolveRenderProfile().settings, before.medium);
  const choices = [undefined, ...presets];
  for (const preset of presets) {
    const named = resolveRenderProfile({ version: 1, preset });
    assert.deepEqual(named.settings, before[preset]);
    assert.deepEqual(named.diagnostics, diagnosticsBefore(preset));
    assert.deepEqual(resolveRenderProfile(undefined, new URLSearchParams({ graphics: preset })), named);
    assert.deepEqual([named.platform, named.preset, named.profile], ["baseline", preset, { version: 1, preset }]);
    for (const geometry of choices) for (const materials of choices) for (const shadows of choices) {
      const overrides = { ...(geometry && { geometry }), ...(materials && { materials }), ...(shadows && { shadows }), renderScale: 0.8 };
      const resolved = resolveRenderProfile({ version: 1, preset, overrides });
      assert.deepEqual(resolved.settings, settingsBefore(preset, overrides));
      assert.deepEqual(resolved.diagnostics, diagnosticsBefore(preset, overrides));
      assert.deepEqual(resolveGraphicsSettings(preset, overrides), resolved.settings);
    }
  }
  // First-frame shader preparation is its own switch, apart from the warm-up after the first frame.
  for (const platform of GRAPHICS_PLATFORMS) for (const preset of presets) {
    const { loading } = resolveRenderProfile({ version: 2, platform: platform.id, quality: preset }).diagnostics;
    assert.equal(loading.firstFrameShaders, true);
    if (mobile(preset) || platform.id.startsWith("phone-")) assert.deepEqual([loading.warmup, loading.textureWarmupMiB], [false, 0]);
  }
  const unprepared = resolveRenderProfile({ version: 2, quality: "medium", overrides: { cpu: { firstFrameShaders: "off" } } }).diagnostics.loading;
  assert.deepEqual([unprepared.firstFrameShaders, unprepared.warmup], [false, true]);
  const flagged = resolveRenderProfile({ version: 1, preset: "low" }, new URLSearchParams("firstframeshaders=off")).diagnostics.loading;
  assert.deepEqual([flagged.firstFrameShaders, flagged.warmup], [false, false]);
  const warmed = resolveRenderProfile({ version: 1, preset: "low", diagnostics: { loading: { warmup: true } } }).diagnostics.loading;
  assert.deepEqual([warmed.firstFrameShaders, warmed.warmup, warmed.textureWarmupMiB], [true, true, 0]);
  // Draw budgets are individual parameters: a link or a player can state them at any preset.
  const drawn = resolveRenderProfile({ version: 2, quality: "medium", overrides: { cpu: { lodBandMerge: 0, glazingDistance: 25 } } });
  assert.deepEqual([drawn.settings.lodBandMergeTriangles, drawn.settings.glazingDistance], [0, 25]);
  assert.equal(resolveRenderProfile({ version: 2, quality: "low", overrides: { cpu: { glazingDistance: "all" } } }).settings.glazingDistance, undefined);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { cpu: { glazingDistance: -1 } } }), /Invalid overrides.cpu.glazingDistance/);
  // The host-facing settings call still clamps a render scale instead of rejecting it.
  assert.equal(resolveGraphicsSettings("low", { renderScale: 9 }).renderScale, 1.5);
  assert.equal(resolveGraphicsSettings("low", { renderScale: NaN }).renderScale, 1);
});

test("a version 1 profile reads as its version 2 equivalent", () => {
  const first = { version: 1, preset: "very-low", overrides: { renderScale: 0.75, materials: "high", shadows: "very-high" },
    diagnostics: { lighting: { ambientOcclusion: "on" } } };
  const second = upgradeRenderProfile(first);
  assert.deepEqual(second, { version: 2, quality: "very-low", overrides: {
    gpu: { renderScale: 0.75, materialLodBias: 1.25, anisotropy: 8, grain: true, reflectionProbes: true, shopFinish: true,
      sunShadowSize: 4096, fixtureShadowSize: 1024 },
  }, diagnostics: { lighting: { ambientOcclusion: "on" } } });
  const { profile: profileOne, ...one } = resolveRenderProfile(first);
  const { profile: profileTwo, ...two } = resolveRenderProfile(second);
  assert.deepEqual(two, one);
  assert.deepEqual([profileOne, profileTwo], [first, second]);
  assert.equal(one.diagnostics.lighting.ambientOcclusion, "on");
  assert.equal(one.parameters.gpu.ambientOcclusion, "off"); // Diagnostics are uncapped switches beside the parameters.
  assert.deepEqual(upgradeRenderProfile({ version: 1 }), { version: 2 });
  assert.equal(upgradeRenderProfile(second), second);

  // The plan's example. The phone profile caps nothing here, and Low's own variant is published.
  const example = { version: 2, platform: "phone-wifi", quality: "low",
    overrides: { gpu: { sunShadowSize: 512 }, cpu: { geometryLodBias: 0.9 } }, diagnostics: { visibility: { lod: false } } };
  const resolved = resolveRenderProfile(example);
  assert.deepEqual([resolved.platform, resolved.preset, resolved.variant, resolved.changed], ["phone-wifi", "low", "reduced", []]);
  assert.deepEqual([resolved.settings.sunShadowMapSize, resolved.settings.geometryLodBias, resolved.settings.materialLodBias], [512, 0.9, 0.7]);
  assert.equal(resolved.diagnostics.visibility.lod, false);
  assert.deepEqual(resolved.profile, example);
  assert.deepEqual(resolveRenderProfile(resolved.profile), resolved);

  // A platform without a stated preset starts on its own default; the link's short forms win over the file.
  assert.equal(resolveRenderProfile({ version: 2, platform: "phone-cellular" }).preset, "very-low");
  assert.equal(resolveRenderProfile({ version: 2 }).preset, "medium");
  const linked = resolveRenderProfile(example, new URLSearchParams("graphics=high&platform=desktop-web"));
  assert.deepEqual([linked.platform, linked.preset, linked.profile.platform, linked.profile.quality], ["desktop-web", "high", "desktop-web", "high"]);
  // The host's platform applies only when neither the profile nor the link states one, and is not written back.
  const hosted = resolveRenderProfile({ version: 2, quality: "high" }, undefined, { platform: "phone-wifi" });
  assert.deepEqual([hosted.platform, hosted.profile], ["phone-wifi", { version: 2, quality: "high" }]);
  assert.equal(resolveRenderProfile(example, undefined, { platform: "native-pc" }).platform, "phone-wifi");
  assert.equal(resolveRenderProfile({ version: 1, preset: "high" }, new URLSearchParams("platform=phone-wifi")).platform, "phone-wifi");

  const query = new URLSearchParams("gltf=/town.glb&graphics=low&platform=native-pc");
  writeRenderProfile(query, { version: 2, platform: "phone-wifi", quality: "high" });
  assert.equal(query.toString(), "gltf=%2Ftown.glb&graphics=high&platform=phone-wifi");
  assert.deepEqual(resolveRenderProfile(undefined, query).platform, "phone-wifi");
  writeRenderProfile(query, example);
  assert.deepEqual([...query.keys()], ["gltf", "profile"]);
  assert.deepEqual(resolveRenderProfile(JSON.parse(query.get("profile"))), resolved);
  writeRenderProfile(query, { version: 1, preset: "low" });
  assert.equal(query.toString(), "gltf=%2Ftown.glb&graphics=low");

  assert.throws(() => resolveRenderProfile({ version: 3 }), /version must be 1 or 2/);
  assert.throws(() => resolveRenderProfile({ version: 2, preset: "low" }), /Unknown profile.preset/);
  assert.throws(() => resolveRenderProfile({ version: 1, quality: "low" }), /Unknown profile.quality/);
  assert.throws(() => resolveRenderProfile({ version: 2, quality: "ultra" }), /Unknown graphics preset/);
  assert.throws(() => resolveRenderProfile({ version: 2, platform: "console" }), /Unknown platform profile/);
  assert.throws(() => resolveRenderProfile(undefined, new URLSearchParams("platform=console")), /Unknown platform profile in viewer link/);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { gpu: { sunShadow: 512 } } }), /Unknown overrides.gpu.sunShadow\./);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { gpu: { renderScale: 0.1 } } }), /Invalid overrides.gpu.renderScale/);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { assets: { maps: { normal: "quarter" } } } }), /Invalid overrides.assets.maps.normal/);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { materials: "high" } }), /Unknown overrides.materials/);
  assert.throws(() => resolveRenderProfile({ version: 2, overrides: { gpu: 1 } }), /overrides.gpu must be an object/);
});

test("platform caps limit presets and overrides alike and are reported", () => {
  for (const preset of presets) assert.deepEqual(resolveGraphicsParameters({ preset }).changed.filter(change => change.reason === "cap"), []);

  const phone = resolveGraphicsParameters({ platform: "phone-wifi", preset: "very-high" });
  assert.deepEqual(phone.changed, [
    { parameter: "assets.geometryDetail", requested: 0, resolved: 1, reason: "cap" },
    { parameter: "assets.texelDensity", requested: 1, resolved: 0.25, reason: "cap" },
    { parameter: "gpu.sunShadowSize", requested: 4096, resolved: 1024, reason: "cap" },
    { parameter: "gpu.fixtureShadowSize", requested: 1024, resolved: 0, reason: "cap" },
    { parameter: "delivery.variant", requested: "full", resolved: "reduced", reason: "variant" },
  ]);
  assert.deepEqual([phone.settings.sunShadowMapSize, phone.settings.fixtureShadowMapSize, phone.variant], [1024, 0, "reduced"]);
  // Parameters without a cap keep the preset's values, and the platform's own values come through.
  assert.deepEqual([phone.settings.geometryLodBias, phone.settings.materialAnisotropy], [1.4, 16]);
  assert.deepEqual([phone.parameters.assets.codec, phone.parameters.memory.textureBudgetMB, phone.parameters.cpu.shaderWarmup], ["uastc", 150, "off"]);
  assert.equal(resolveRenderProfile({ version: 2, platform: "phone-wifi", quality: "very-high" }).diagnostics.loading.warmup, false);

  // A cap meets a scene-authored size at the size the data file says authored stands for.
  const medium = resolveGraphicsParameters({ platform: "phone-wifi", preset: "medium" });
  assert.deepEqual(medium.changed.filter(change => change.parameter.startsWith("gpu.")), [
    { parameter: "gpu.sunShadowSize", requested: "authored", resolved: 1024, reason: "cap" },
    { parameter: "gpu.fixtureShadowSize", requested: "authored", resolved: 0, reason: "cap" },
  ]);
  const laptop = resolveGraphicsParameters({ platform: "laptop-web", preset: "medium" });
  assert.deepEqual([laptop.parameters.gpu.sunShadowSize, laptop.settings.sunShadowMapSize], ["authored", undefined]);

  // An override is a request like any other.
  const asked = resolveGraphicsParameters({ platform: "phone-cellular", preset: "low", overrides: { gpu: { sunShadowSize: 4096, materialLodBias: 1.5 } } });
  assert.deepEqual(asked.changed, [{ parameter: "gpu.sunShadowSize", requested: 4096, resolved: 1024, reason: "cap" }]);
  assert.equal(asked.settings.materialLodBias, 1.5);
  // Diagnostics stay uncapped development switches.
  const forced = resolveRenderProfile({ version: 2, platform: "phone-wifi", quality: "low", diagnostics: { materials: { reflectionProbes: true } } });
  assert.equal(forced.diagnostics.materials.reflectionProbes, true);
});

test("the pack comes from the resolved asset variant", () => {
  const base = new URL("https://example.invalid/maps/stoacity.glb?v=2");
  const pack = resolved => (graphicsPackUrl(base, resolved.variant) ?? base).pathname;
  // The baseline platform requests the packs each preset always did.
  const packs = { "very-low": "/maps/stoacity.very-low.glb", low: "/maps/stoacity.low.glb", medium: "/maps/stoacity.glb",
    high: "/maps/stoacity.glb", "very-high": "/maps/stoacity.glb" };
  const variants = { "very-low": "minimal", low: "reduced", medium: "standard", high: "standard", "very-high": "standard" };
  for (const preset of presets) {
    const resolved = resolveRenderProfile({ version: 1, preset });
    assert.equal(pack(resolved), packs[preset]);
    assert.deepEqual([resolved.variant, resolved.parameters.delivery.variant], [variants[preset], variants[preset]]);
    // A caller that still passes the preset name gets the same pack.
    assert.equal((graphicsPackUrl(base, preset) ?? base).pathname, packs[preset]);
    // Very High asks for the full variant, which has no pack yet.
    assert.deepEqual(resolved.changed, preset === "very-high"
      ? [{ parameter: "delivery.variant", requested: "full", resolved: "standard", reason: "unpublished" }] : []);
  }
  assert.equal(graphicsPackUrl(base, "reduced").search, "?v=2");
  assert.deepEqual(GRAPHICS_VARIANTS.map(graphicsPackSuffix), [".very-low", ".low", "", undefined]);
  for (const entry of ["stoacity.low.glb", "stoacity.very-low.glb", "stoacity.gltf"])
    assert.equal(graphicsPackUrl(new URL("https://example.invalid/maps/" + entry), "minimal"), undefined);
  assert.equal(graphicsPackUrl(base, "standard"), undefined);
  assert.equal(graphicsPackUrl(base, undefined), undefined);

  // The preset name no longer decides: a phone on High fetches the reduced pack, and a desktop on High the base one.
  assert.equal(pack(resolveRenderProfile({ version: 2, platform: "phone-wifi", quality: "high" })), "/maps/stoacity.low.glb");
  assert.equal(pack(resolveRenderProfile({ version: 2, platform: "desktop-web", quality: "high" })), "/maps/stoacity.glb");
  assert.equal(pack(resolveRenderProfile({ version: 2, platform: "desktop-web", quality: "low" })), "/maps/stoacity.low.glb");
  // Asking for lighter assets lowers the variant whatever the preset is called.
  const lighter = resolveRenderProfile({ version: 2, quality: "medium", overrides: { assets: { texelDensity: 0.25 } } });
  assert.deepEqual([lighter.variant, lighter.changed], ["reduced", [
    { parameter: "delivery.variant", requested: "standard", resolved: "reduced", reason: "variant" },
    { parameter: "assets.geometryDetail", requested: 0, resolved: 1, reason: "variant" },
  ]]);
  assert.equal(resolveRenderProfile({ version: 2, quality: "low", overrides: { delivery: { variant: "standard" } } }).variant, "reduced");

  // A laptop's texel cap sits between two variants, so it takes the lower and reports what that costs.
  const laptop = resolveGraphicsParameters({ platform: "laptop-web", preset: "medium" });
  assert.deepEqual(laptop.changed, [
    { parameter: "assets.texelDensity", requested: 1, resolved: 0.5, reason: "cap" },
    { parameter: "delivery.variant", requested: "standard", resolved: "reduced", reason: "variant" },
    { parameter: "assets.geometryDetail", requested: 0, resolved: 1, reason: "variant" },
    { parameter: "assets.texelDensity", requested: 0.5, resolved: 0.25, reason: "variant" },
  ]);

  // A host that knows which packs a map has overrides the data file's list.
  const only = (published, options) => resolveGraphicsParameters({ ...options, publishedVariants: published });
  assert.deepEqual(only(GRAPHICS_VARIANTS, { preset: "very-high" }).changed, []);
  assert.equal(only(GRAPHICS_VARIANTS, { preset: "very-high" }).variant, "full");
  const baseOnly = only(["standard"], { preset: "very-low" });
  assert.deepEqual([baseOnly.variant, baseOnly.changed], ["standard", [
    { parameter: "delivery.variant", requested: "minimal", resolved: "standard", reason: "unpublished" },
    { parameter: "assets.geometryDetail", requested: 2, resolved: 0, reason: "variant" },
    { parameter: "assets.texelDensity", requested: 0.125, resolved: 1, reason: "variant" },
  ]]);
  assert.equal(only(["minimal", "standard"], { preset: "low" }).variant, "minimal");
  assert.equal(only([], { preset: "low" }).variant, "reduced");
});

test("parameters that nothing consumes yet change neither the renderer's inputs nor the pack", () => {
  const unconsumed = Object.entries(data.parameters).filter(([, declared]) => !declared.consumed);
  assert.ok(unconsumed.some(([id]) => id === "assets.codec") && unconsumed.some(([id]) => id === "cpu.drawBudget"));
  for (const preset of presets) {
    const { settings, diagnostics, variant } = resolveRenderProfile({ version: 2, quality: preset });
    for (const [id, declared] of unconsumed) {
      const values = declared.values ?? [declared.min, declared.max ?? declared.min + 1];
      for (const value of values) {
        const resolved = resolveRenderProfile({ version: 2, quality: preset, overrides: nested(id, value) });
        assert.deepEqual([resolved.settings, resolved.diagnostics, resolved.variant], [settings, diagnostics, variant], id);
        assert.equal(at(resolved.parameters, id), value);
      }
    }
  }
  // The platform profiles differ from the baseline only through consumed parameters and caps.
  const desktop = resolveRenderProfile({ version: 2, platform: "desktop-web", quality: "high" });
  const baseline = resolveRenderProfile({ version: 2, quality: "high" });
  assert.deepEqual([desktop.settings, desktop.diagnostics, desktop.variant], [baseline.settings, baseline.diagnostics, baseline.variant]);
});

test("device and connection signals select a platform profile", () => {
  const iphone = { userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Mobile/15E148 Safari/604.1",
    coarsePointer: true, maxTouchPoints: 5 };
  const android = { mobile: true, userAgent: "Mozilla/5.0 (Linux; Android 15; Pixel 9) Chrome/131.0 Mobile Safari/537.36", deviceMemory: 8 };
  const mac = { userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15" };
  // An iPhone reports neither memory nor connection, so it is a phone on Wi-Fi until someone says otherwise.
  assert.equal(detectGraphicsPlatform(iphone), "phone-wifi");
  assert.equal(detectGraphicsPlatform({ ...iphone, saveData: true }), "phone-cellular");
  assert.equal(detectGraphicsPlatform(android), "phone-wifi");
  // Wi-Fi also reports a 4G effective type, so only a slower one, or the connection's own type, shows cellular.
  assert.equal(detectGraphicsPlatform({ ...android, effectiveType: "4g" }), "phone-wifi");
  assert.equal(detectGraphicsPlatform({ ...android, effectiveType: "4g", connectionType: "cellular" }), "phone-cellular");
  for (const effectiveType of ["slow-2g", "2g", "3g"]) assert.equal(detectGraphicsPlatform({ ...android, effectiveType }), "phone-cellular");
  assert.equal(detectGraphicsPlatform({ ...android, connectionType: "wifi", saveData: true }), "phone-cellular");
  // The client hint is enough without a recognisable user agent, and so is a touch-first device such as an iPad.
  assert.equal(detectGraphicsPlatform({ mobile: true }), "phone-wifi");
  assert.equal(detectGraphicsPlatform({ ...mac, coarsePointer: true, maxTouchPoints: 5 }), "phone-wifi");
  // A touch screen beside a mouse is not a phone, and neither is a coarse pointer without touch.
  assert.equal(detectGraphicsPlatform({ ...mac, coarsePointer: false, maxTouchPoints: 10 }), "desktop-web");
  assert.equal(detectGraphicsPlatform({ ...mac, coarsePointer: true, maxTouchPoints: 0 }), "desktop-web");

  // Desktop unless the device shows it is smaller. A browser that reports nothing stays uncapped.
  assert.equal(detectGraphicsPlatform(), "desktop-web");
  assert.equal(detectGraphicsPlatform(mac), "desktop-web");
  assert.equal(detectGraphicsPlatform({ ...mac, deviceMemory: 8, maxTextureSize: 16384 }), "desktop-web");
  assert.equal(detectGraphicsPlatform({ ...mac, deviceMemory: 4, maxTextureSize: 16384 }), "laptop-web");
  assert.equal(detectGraphicsPlatform({ ...mac, deviceMemory: 8, maxTextureSize: 8192 }), "laptop-web");
  assert.equal(detectGraphicsPlatform({ ...mac, maxTextureSize: 8192 }), "laptop-web");
  // Data saving and connection speed describe a phone's link; they do not shrink a desktop.
  assert.equal(detectGraphicsPlatform({ ...mac, saveData: true, effectiveType: "3g" }), "desktop-web");
  // A native host decides on its own, whatever the embedded browser reports.
  for (const signals of [{}, iphone, { ...android, effectiveType: "2g" }, { deviceMemory: 2 }])
    assert.equal(detectGraphicsPlatform({ ...signals, native: true }), "native-pc");
  assert.equal(detectGraphicsPlatform({ ...iphone, native: false }), "phone-wifi");
});

test("detection is opt-in, the browser's signals are gathered on request, and a stated platform wins", t => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const originalMedia = Object.getOwnPropertyDescriptor(globalThis, "matchMedia");
  t.after(() => {
    if (original) Object.defineProperty(globalThis, "navigator", original); else delete globalThis.navigator;
    if (originalMedia) Object.defineProperty(globalThis, "matchMedia", originalMedia); else delete globalThis.matchMedia;
  });
  const browse = (navigator, coarse = false) => {
    Object.defineProperty(globalThis, "navigator", { value: navigator, configurable: true });
    Object.defineProperty(globalThis, "matchMedia", { configurable: true,
      value: query => ({ matches: coarse && query === "(pointer: coarse)" }) });
  };
  browse({ userAgent: "Mozilla/5.0 (Linux; Android 15) Mobile", userAgentData: { mobile: true }, maxTouchPoints: 5, deviceMemory: 4,
    connection: { type: "cellular", effectiveType: "4g", saveData: false } }, true);
  assert.deepEqual(graphicsPlatformSignals(), { mobile: true, userAgent: "Mozilla/5.0 (Linux; Android 15) Mobile", coarsePointer: true,
    maxTouchPoints: 5, deviceMemory: 4, connectionType: "cellular", effectiveType: "4g", saveData: false });
  assert.equal(detectBrowserGraphicsPlatform(), "phone-cellular");
  // What the host states is laid over what the browser reports.
  assert.equal(detectBrowserGraphicsPlatform({ native: true }), "native-pc");
  assert.equal(graphicsPlatformSignals({ maxTextureSize: 4096, saveData: true }).maxTextureSize, 4096);

  // Nothing is detected unless the host asks: on this phone every preset still resolves on the baseline.
  for (const preset of presets) {
    const resolved = resolveRenderProfile({ version: 1, preset });
    assert.equal(resolved.platform, "baseline");
    assert.deepEqual([resolved.settings, resolved.diagnostics], [settingsBefore(preset), diagnosticsBefore(preset)]);
  }
  assert.equal(resolveRenderProfile(undefined, new URLSearchParams("graphics=high")).platform, "baseline");

  // A host that opts in passes the detected platform, and the phone's caps and default preset apply.
  const detected = { platform: detectBrowserGraphicsPlatform() };
  const opted = resolveRenderProfile(undefined, undefined, detected);
  assert.deepEqual([opted.platform, opted.preset, opted.variant], ["phone-cellular", "very-low", "minimal"]);
  const high = resolveRenderProfile({ version: 1, preset: "high" }, undefined, detected);
  assert.deepEqual([high.variant, high.settings.sunShadowMapSize, high.settings.fixtureShadowMapSize], ["reduced", 1024, 0]);
  // A phone goes without shader warm-up and antialiasing at every preset, not only the light ones.
  assert.deepEqual([high.parameters.gpu.antialiasing, high.diagnostics.effects.displayAA, high.diagnostics.loading.warmup,
    high.diagnostics.loading.textureWarmupMiB], ["none", false, false, 0]);
  const desk = resolveRenderProfile({ version: 1, preset: "high" }, undefined, { platform: detectGraphicsPlatform({ deviceMemory: 8 }) });
  assert.deepEqual([desk.platform, desk.parameters.gpu.antialiasing, desk.diagnostics.loading.warmup], ["desktop-web", "msaa", true]);

  // The player's stored choice and a link's platform both win over detection.
  assert.equal(resolveRenderProfile({ version: 2, platform: "desktop-web", quality: "high" }, undefined, detected).platform, "desktop-web");
  assert.equal(resolveRenderProfile({ version: 1, preset: "high" }, new URLSearchParams("platform=laptop-web"), detected).platform, "laptop-web");
  assert.equal(resolveRenderProfile({ version: 2, platform: "baseline", quality: "high" }, undefined, detected).variant, "standard");

  // A desktop browser that exposes nothing, and no browser at all.
  browse({ userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0) Gecko/20100101 Firefox/133.0", maxTouchPoints: 0 });
  assert.equal(detectBrowserGraphicsPlatform(), "desktop-web");
  assert.equal(detectBrowserGraphicsPlatform({ deviceMemory: 4 }), "laptop-web");
  browse(undefined);
  assert.deepEqual(graphicsPlatformSignals({ native: true }), { native: true });
  assert.equal(detectBrowserGraphicsPlatform(), "desktop-web");
});
