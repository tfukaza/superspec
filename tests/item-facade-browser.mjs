/**
 * The washroom built from items with the Kowloon building side across the lane (`buildProject(..., { facade: true })`):
 * a composed ItemSpec item whose walls carry a row of windows per floor, whose air conditioners and condensers are the
 * Kowloon fixtures as native generators chosen per bay by its seed, and whose duct riser is a sub-item spanning the
 * floors. Opens the project in the built Material Studio, waits for the bake and pictures it from the room, square on
 * and at an angle. Pictures go to OUT (default generated/verification/item-facade) as <view>.png. FACADE_PARAMETERS
 * (JSON) overrides the side's controls, for example {"floors":6,"seed":3}.
 */
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { launchBrowser, waitForFrames } from "../tools/browser.mjs";
import { buildProject } from "./studio/washroom-items/build.mjs";

const root = resolve(process.env.STUDIO_UI_ROOT ?? "generated/rust-editor");
const out = resolve(process.env.OUT ?? "generated/verification/item-facade");
await mkdir(out, { recursive: true });
// MODEL=building pictures the whole building generated from its footprint (the reference shop house by default;
// BUILDING_PARAMETERS overrides its controls) instead of the one side.
const choice = process.env.MODEL ?? "side";
// LANE=1 adds the road and the pedestrian avenue with its median's grass, trees and shrubs (write them first:
//   cargo test --release -p superspec-engine --test treespec_batches -- --ignored write_lane_plants).
const lane = process.env.LANE === "1" ? JSON.parse(await readFile(resolve(process.env.LANE_PLANTS ?? "generated/verification/treespec/lane-plants.json"), "utf8")) : null;
// RESOLUTION (default 2048) bounds every receiver's atlas: at 1024 a storey wall of 10 m or more cannot reach TEXELS
// (160 a metre) and is baked at about 100, which shows its streaks in bands at close range; 2048 costs about 0.6 GiB more
// of baked maps on screen.
const { project } = buildProject(Number(process.env.RESOLUTION ?? 2048), Number(process.env.TEXELS ?? 160), { facade: choice === "side" ? true : choice, lane, room: process.env.ROOM === "1" });
const facade = project.models.find(model => model.id === (choice === "side" ? "facade" : "building"));
// The whole building is GPU-baked within BAKE_MEMORY MiB (default 6144: the building with its avenue, far frontage and street furniture takes about 4 GiB; with its weathering layers it takes about 2.2 GiB at 1024 px and 160 texels a meter): beyond the allowance a mesh stays on the
// per-pixel look, which shows a layered slot's base alone (no paint or grime).
// Only the building, the road and the avenue (with its median's grass and plants) are drawn; ROOM=1 brings back the
// washroom items.
// BUILDING_PARAMETERS is JSON, or a .json file of it (tests/studio/washroom-items/reference-shop-house.json is the
// reference picture's plan); keys starting with "/" are comments.
const overrides = process.env.BUILDING_PARAMETERS?.endsWith(".json") ? await readFile(resolve(process.env.BUILDING_PARAMETERS), "utf8") : process.env.BUILDING_PARAMETERS;
Object.assign(facade.parameters, Object.fromEntries(Object.entries(JSON.parse(overrides ?? "{}")).filter(([key]) => !key.startsWith("/"))));
// Above 512 texels a project needs the production profile.
if (project.resolution > 512) project.profile = "production";
Object.assign(facade.parameters, JSON.parse(process.env.FACADE_PARAMETERS ?? "{}"));
const text = JSON.stringify(project);

const server = createServer(async (request, response) => {
  try {
    const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const path = resolve(root, name === "/" ? "index.html" : `.${name}`);
    if (!path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".png": "image/png" }[extname(path)] ?? "application/octet-stream");
    response.end(await readFile(path));
  } catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, "127.0.0.1", done));

const browser = await launchBrowser();
try {
  // SCALE: device pixels per CSS pixel (default 1), for sharper pictures of small details.
  // VIEWPORT=<width>x<height> sets the page size (default 1400x1000), e.g. 1080x1080 for a square video.
const [viewWidth, viewHeight] = (process.env.VIEWPORT ?? "1400x1000").split("x").map(Number);
const page = await browser.newPage({ viewport: { width: viewWidth, height: viewHeight }, deviceScaleFactor: Number(process.env.SCALE ?? 1) });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  // Errors are reported; CONSOLE=1 reports everything the page says (a bake that never starts usually says why).
  page.on("console", message => { if (message.type() === "error" || process.env.CONSOLE === "1") errors.push(`${message.type()}: ${message.text()}`.slice(0, 600)); });
  await page.goto(`http://127.0.0.1:${server.address().port}/material.html?gpuprobe&ao=${process.env.AO ?? "on"}&bakedtints=${process.env.TINTS ?? "on"}&bakememory=${process.env.BAKE_MEMORY ?? 6144}${
    // TREE_LOOK=<leaves|sprays|shells|impostors>: every tree's and shrub's mass drawn in that illustrated look (anime-tree.ts).
    process.env.TREE_LOOK ? `&treelook=${process.env.TREE_LOOK}` : ""}${
    // QUERY adds page parameters (for example QUERY="aoshare=off").
    process.env.QUERY ? `&${process.env.QUERY}` : ""}`);
  await page.waitForFunction(() => document.querySelector("#status")?.dataset.state === "ready", {}, { timeout: 600000 });
  // The project is handed over as a file: a building with its images can pass the 50 MB an in-memory upload may hold.
  const projectFile = resolve(out, "washroom-facade.studio.json");
  await writeFile(projectFile, text);
  await page.locator("#open-project").setInputFiles(projectFile);
  try {
    await page.waitForFunction(() => ["ready", "error"].includes(document.querySelector("#status")?.dataset.state)
      && (document.querySelector("#status")?.dataset.state === "error" || JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "null")?.state === "baked"), {}, { timeout: Number(process.env.BAKE_TIMEOUT ?? 900000) });
  } finally {
    const state = await page.evaluate(() => ({ status: document.querySelector("#status")?.textContent, state: document.querySelector("#status")?.dataset.state,
      diagnostics: [...document.querySelectorAll("#diagnostics button")].map(button => button.textContent) }));
    if (state.state !== "ready") { console.log(`Not ready: ${JSON.stringify(state)}\n${errors.slice(0, 8).join("\n")}`); process.exitCode = 1; }
    // A bake that never finished (the wait above then timed out) says what it was waiting for.
    const [bake, fallback] = await page.evaluate(() => [document.querySelector("#viewport").dataset.sceneBake ?? "", document.querySelector("#viewport").dataset.sceneBakeFallback ?? ""]);
    if (JSON.parse(bake || "null")?.state !== "baked") console.log(`Bake unfinished: ${bake.slice(0, 3000)}\n${fallback && `No GPU bake: ${fallback.slice(0, 2000)}\n`}${JSON.stringify(state).slice(0, 3000)}\n${errors.filter(line => !/warning X\d+/.test(line)).slice(-25).join("\n")}`);
  }
  if (!process.exitCode) {
    await waitForFrames(page, 6);
    // What the studio says of the bake: its status line and, per material, any layered slot it could not compose.
    const report = await page.evaluate(() => ({ status: document.querySelector("#status")?.textContent,
      bake: (document.querySelector("#viewport").dataset.sceneBake ?? "").slice(0, 1500) }));
    console.log(`status: ${report.status}\nbake: ${report.bake}`);
    // ATLASES=<text>: the atlas size and triangle count of every mesh whose name holds the text, and the bake's memory.
    if (process.env.ATLASES) {
      const found = await page.evaluate(text => {
        const bake = JSON.parse(document.querySelector("#viewport").dataset.sceneBake ?? "{}");
        const lines = Object.entries(bake.atlases ?? {}).filter(([name]) => name.includes(text)).map(([name, size]) => {
          const mesh = window.__scene.models.children.find(child => child.name === name), geometry = mesh?.userData.sourceGeometry ?? mesh?.geometry;
          return `${name}: ${size}, ${geometry ? (geometry.getIndex()?.count ?? geometry.getAttribute("position").count) / 3 : "?"} triangles`;
        });
        lines.push(...(bake.details ?? []).filter(line => line.includes(text)).slice(0, 6).map(line => `drawn ${line}`));
        // What each weathering layer of the first such mesh was given: its features by kind (5 and up are the names it follows, in order).
        const entry = window.__scene.sceneBaker?.current?.entries.find(candidate => candidate.target.mesh.name.includes(text) && candidate.target.stack);
        for (const layer of entry?.target.stack.layers ?? []) {
          const data = layer.uniforms.ms_features?.value?.image?.data, count = layer.uniforms.ms_feature_count?.value ?? 0, kinds = {};
          for (let row = 0; data && row < count; row++) kinds[data[row * 12 + 3]] = (kinds[data[row * 12 + 3]] ?? 0) + 1;
          lines.push(`layer ${layer.id} of ${entry.target.mesh.name}: ${layer.shading?.shader?.namedFeatures?.join(",") ?? ""} ${count} features, by kind ${JSON.stringify(kinds)}`);
        }
        lines.push(...Object.entries(bake.unsupported ?? {}).slice(0, 12).map(([name, why]) => `unsupported ${name}: ${why}`));
        return [...lines, `maps on screen ${(bake.bytes / 1048576).toFixed(0)} MiB, bake ${bake.ms} ms`];
      }, process.env.ATLASES);
      console.log(found.join("\n"));
    }
    // The sun stands to the upper left of the reference camera, as in both reference pictures: `sunOffset` degrees round
    // from the camera's azimuth (-50, toward the viewer's left) and `sunElevation` high (48), so every caster throws its
    // shadow down and to the right where the camera sees it (a sun just off the camera hid each shadow behind its caster).
    // A plan's "/view" may state either; SUN=<azimuth>,<elevation> (degrees from +x toward +y) replaces both.
    const plannedView = { azimuth: 22, sunOffset: -50, sunElevation: 48, ...JSON.parse(overrides ?? "{}")["/view"] };
    const sunAt = `${plannedView.azimuth + plannedView.sunOffset},${plannedView.sunElevation}`;
    // LIGHT: "keep" leaves the scene's lighting alone; otherwise the sun is turned round and `softSun` (below) is merged into
    // the sun, then LIGHT_SUN, a JSON object merged the same way (nested objects merge: {"shadow":{"softness":0.05}}).
    // The record is the scene's global lighting (docs/plans/lighting-settings.md); nothing here is a material setting.
    const softSun = {
      intensity: 1.8, color: [1, 0.9, 0.77],
      shadow: {
        // An edge that is tight at contact and widens with the distance to what casts it: a thin caster (a cage bar, a
        // bracket, a pipe) a hand's breadth off the wall leaves a soft, light shadow, while a course, a sign, a cage, an
        // awning or the building itself, wider than its penumbra, shades clearly.
        softness: 0.045, spread: 0.12, maxSoftness: 0.4,
        // A soft cool shade, as in the reference pictures, faintly mottled (broad, gentle patches about 1.5 m across) with a
        // slight wobble to its edge: clear enough that large forms read (darkness 0.1, tried first, washed out everything).
        darkness: 0.34, tintStrength: 0.28, tint: [0.05, 0.2, 0.8], tintAlt: [0.06, 0.18, 0.78], wrap: 0.45,
        noise: { scale: 1.5, edge: 0.04, amount: 0.08, seed: 7 },
      },
    };
    // The rest of the same global record (exposure, tone mapping, ambient and environment light, ambient occlusion):
    // `softScene`, then LIGHT_SCENE, a JSON object merged over it the same way.
    // A bright overcast day: more light from the sky than from the sun. Ambient occlusion is for the large forms only
    // (under the balcony, awnings and ledges): blended in at less than full strength, with surfaces taken to be fairly
    // thin (`thickness` 0.3 m), so a pipe, a cage bar or a bracket leaves no dark halo. (A wider radius with a smaller
    // thickness, tried first, broke the occlusion up into blocky noise over open wall.)
    // (AO=off, in the page's address, leaves it out).
    const softScene = {
      exposure: 1.06,
      ambient: { sky: [1, 0.98, 0.95], ground: [0.42, 0.4, 0.35], intensity: 1.15 },
      ambientOcclusion: { enabled: true, radius: 0.55, intensity: 0.8, thickness: 0.3 },
    };
    // >>> ILLUSTRATION EXPERIMENTS: two switchable settings of the same global record (docs/plans/lighting-settings.md,
    // "Illustration experiments"), both off in the shipped look and on here.
    // FORM=<0 to 1> is the sun's form light in shadow (`shadow.form`, default 0.35; 0 is off) and FORM_FILL its fill
    // (`shadow.formFill`, default 0); CONTOURS=0 leaves the contour lines out (`contours`, default on).
    const form = Number(process.env.FORM ?? 0.35), formFill = Number(process.env.FORM_FILL ?? 0);
    if (form > 0) softSun.shadow.form = form;
    if (formFill > 0) softSun.shadow.formFill = formFill;
    if (process.env.CONTOURS !== "0") softScene.contours = { enabled: true };
    // <<< ILLUSTRATION EXPERIMENTS
    const lit = await page.evaluate(({ sun, soft, extra, keep, scene }) => {
      const s = window.__scene, record = JSON.parse(document.querySelector("#viewport").dataset.lighting), light = record.lights[0];
      const before = JSON.stringify(light);
      if (keep) return { before };
      const merge = (target, source) => { for (const [key, value] of Object.entries(source)) target[key] = value && typeof value === "object" && !Array.isArray(value) ? merge(target[key] ?? {}, value) : value; return target; };
      if (sun) { const [az, el] = sun.split(",").map(v => Number(v) * Math.PI / 180); light.direction = [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)]; }
      else light.direction = [-light.direction[0], -light.direction[1], light.direction[2]];
      merge(light, soft); merge(light, extra); merge(record, scene);
      s.setLighting(record); s.renderer.shadowMap.needsUpdate = true; s.invalidate?.();
      window.__facadeLighting = record;
      return { before, after: JSON.stringify(light), scene: JSON.stringify({ ...record, lights: undefined }) };
    }, { sun: process.env.SUN ?? sunAt, soft: softSun, extra: JSON.parse(process.env.LIGHT_SUN ?? "{}"), keep: process.env.LIGHT === "keep",
      scene: { ...softScene, ...JSON.parse(process.env.LIGHT_SCENE ?? "{}") } });
    console.log(`sun before ${lit.before}`);
    if (lit.after) console.log(`sun after  ${lit.after}\nscene ${lit.scene}`);
    await waitForFrames(page, 6);
    const info = await page.evaluate(id => {
      const s = window.__scene, T = window.__three, box = new T.Box3();
      let meshes = 0;
      for (const m of s.models.children) if (m.name.startsWith(`${id}/`)) { box.expandByObject(m); meshes++; }
      return { meshes, min: box.min.toArray(), max: box.max.toArray() };
    }, facade.id);
    console.log(`facade: ${info.meshes} meshes, ${info.min.map(v => v.toFixed(2))} to ${info.max.map(v => v.toFixed(2))}`);
    // SLOTS=1 lists the building's meshes by the last part of their names (a native fixture's mesh is named after its
    // slot), with how many there are and their mean vertex colour: what a material bound to that slot is multiplied by.
    if (process.env.SLOTS === "1") {
      const slots = await page.evaluate(id => {
        const groups = {};
        for (const m of window.__scene.models.children) {
          if (!m.name.startsWith(`${id}/`)) continue;
          const key = m.name.split("/").slice(-2).join("/"), color = m.geometry.getAttribute("color"), group = groups[key] ??= { meshes: 0, vertices: 0, sum: [0, 0, 0], sample: m.name };
          group.meshes++;
          if (color) for (let i = 0; i < color.count; i++) { group.vertices++; group.sum[0] += color.getX(i); group.sum[1] += color.getY(i); group.sum[2] += color.getZ(i); }
        }
        return Object.entries(groups).map(([key, g]) => `${key}: ${g.meshes} meshes, colour ${g.sum.map(v => (v / Math.max(1, g.vertices)).toFixed(2))} (${g.sample})`);
      }, facade.id);
      console.log(slots.join("\n"));
    }
    const [x0, y0, z0] = info.min, [x1, y1, z1] = info.max;
    const cy = (y0 + y1) / 2, cz = (z0 + z1) / 2, front = x1;
    // The reference picture's camera: at the cut corner, 22 degrees off the front's normal toward the side, 8 above the
    // ground, 62 m from the building through a long lens (26 degrees), so the front is wide and the side foreshortened.
    // A plan file's "/view" key replaces any of these: azimuth, elevation and fov in degrees, distance and (target) height in meters.
    const look = { azimuth: 22, elevation: 8, distance: 62, height: 7.4, fov: 19.5, ...JSON.parse(overrides ?? "{}")["/view"] };
    const azimuth = look.azimuth * Math.PI / 180, elevation = look.elevation * Math.PI / 180, distance = look.distance;
    const views = {
      reference: { position: [(x0 + front) / 2 + distance * Math.cos(elevation) * Math.cos(azimuth), cy + distance * Math.cos(elevation) * Math.sin(azimuth), look.height + distance * Math.sin(elevation)],
        target: [(x0 + front) / 2, cy, look.height], fov: look.fov, bare: true },
      // Square on, from the lane.
      front: { position: [front + 30, cy, cz], target: [front, cy, cz] },
      // At an angle from down the lane, to see the fixtures and riser stand off the wall.
      angle: { position: [front + 6, y0 - 7, 4.5], target: [front, cy, cz * 0.8] },
      // Close on the riser's corner and a few bays.
      close: { position: [front + 4, y1 + 1.5, 6], target: [front, y1 - 2, 5.5] },
      // From the street corner (+x, +y), raised, as the reference picture of the shop house is taken.
      corner: { position: [front + 13, y1 + 11, 10.5], target: [(x0 + front) / 2, (y0 + y1) / 2, 5.5] },
      // Along the pedestrian avenue (`tests/studio/washroom-items/avenue.json`) from beyond its far end, toward the side
      // face and the street corner: the median's planting, the benches and the paving. Its own sun (`sun`: azimuth and
      // elevation, degrees) comes from behind the camera on the avenue's side, so the crowns' lit clumps face it and their
      // dappled shadows fall on the building's side face and shop fronts; every other view keeps the shared sun.
      // Raised over the roofs past the avenue's far end and looking down along the median toward the building's street
      // corner, the building and its neighbour on the right and the neighbours' shopfronts closing the left.
      avenue: { position: [x0 - 21, y1 + 5.6, 16.5], target: [front - 2, y1 - 0.5, 2], sun: [135, 54] },
      // At eye height on the avenue's near walkway, from its mouth along the median, with the avenue's sun: the trunks
      // rising from the planter, the crowns over the walkway and their shadows on the paving and the side face.
      street: { position: [front + 2.5, y1 + 0.15, 1.7], target: [front - 21, y1 + 3.05, 4.2], sun: [135, 54] },
      // The side face and its shop fronts from across the avenue's mouth, with the avenue's sun: the crowns' dappled shadows on them.
      // Across the avenue from the building's first floor toward the far side: the neighbours' shopfronts and upper
      // storeys, unit by unit.
      "far-side": { position: [front - 7, y1 + 0.6, 5.2], target: [front - 17, y1 + 8.5, 4.4], fov: 62, sun: [135, 54] },
      "side-face": { position: [front + 4, y1 + 6.5, 3.2], target: [x0 + 3, y1 - 1.6, 2.6], sun: [135, 54] },
      // The same without the lane's plants, to compare the building alone.
      "corner-bare": { position: [front + 13, y1 + 11, 10.5], target: [(x0 + front) / 2, (y0 + y1) / 2, 5.5], bare: true },
      // Details, without the plants: the corner's foot and the pavement, the balcony and what hangs under it, the signs
      // on the corner, a storey of windows and fixtures, and the shops from the street.
      base: { position: [front + 4.2, y1 + 3.6, 1.7], target: [front - 0.3, y1 - 0.6, 0.85], bare: true },
      balcony: { position: [front + 6.5, cy - 3, 5.4], target: [front - 0.2, cy + 0.5, 3.7], bare: true },
      signs: { position: [front + 6.5, y1 + 5, 8.2], target: [front - 0.6, y1 - 0.4, 7.2], bare: true },
      storey: { position: [front + 6.5, cy + 0.6, 8.3], target: [front, cy + 0.6, 8.1], bare: true },
      shops: { position: [front + 8, cy + 1, 1.75], target: [front, cy - 0.3, 1.9], bare: true },
      // The edges off the street: the back (-x) and the left end (-y), a little from one side to show what stands off them.
      back: { position: [x0 - 26, cy - 4, cz + 2], target: [x0, cy, cz], bare: true },
      left: { position: [(x0 + front) / 2 - 4, y0 - 26, cz + 2], target: [(x0 + front) / 2, y0, cz], bare: true },
      // High three-quarter from the street corner, looking down on the roof: its rail, tank, stair hut, ladder, aerials
      // and the aerial's cable, and what stands about on it.
      roof: { position: [front + 6, y1 + 5, z1 + 6], target: [(x0 + front) / 2, cy, z1 - 2], bare: true },
    };
    // Only the picture: the page's panels over the viewport are hidden (CHROME=1 keeps them).
    if (process.env.CHROME !== "1") await page.evaluate(() => {
      const canvas = document.querySelector("#viewport canvas");
      for (const element of document.querySelectorAll("body *")) if (element !== canvas && !element.contains(canvas)) element.style.visibility = "hidden";
    });
    // A warm pale backdrop, as the reference picture's (BACKDROP=<css colour>, or "keep" for the studio's own).
    if (process.env.BACKDROP !== "keep") await page.evaluate(color => {
      const s = window.__scene, T = window.__three;
      if (s.scene?.background?.isColor) s.scene.background.set(color); else if (s.scene) s.scene.background = new T.Color(color);
      s.renderer.setClearColor?.(color); s.invalidate?.();
    }, process.env.BACKDROP ?? "#d2ccc4");
    // VIEWS=<name>,<name> pictures only those views.
    const wanted = process.env.VIEWS?.split(",");
    const frameReport = {};
    for (const [name, view] of Object.entries(views).filter(([name]) => !wanted || wanted.includes(name))) {
      // Every view keeps the scene's ambient occlusion: foliage neither casts nor receives it (ambient-occlusion.ts).
      await page.evaluate(view => { const s = window.__scene;
        // A view's own sun, or the shared one (as the lighting above set it).
        const record = window.__facadeLighting;
        if (record) {
          const light = record.lights[0], shared = window.__facadeSun ??= [...light.direction];
          const [az, el] = (view.sun ?? [0, 0]).map(v => v * Math.PI / 180);
          const direction = view.sun ? [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)] : shared;
          if (direction.some((v, i) => v !== light.direction[i])) { light.direction = direction; s.setLighting(record); s.renderer.shadowMap.needsUpdate = true; }
        }
        (s.scene ?? s.models).traverse(m => { if (m.userData?.foliage) { m.visible = !view.bare; if (m.userData.source) m.userData.source.visible = !view.bare; } }); s.defaultFov ??= s.camera.fov; s.camera.fov = view.fov ?? s.defaultFov; s.camera.updateProjectionMatrix(); s.camera.position.fromArray(view.position); s.controls.target.fromArray(view.target); s.controls.update(); s.draw(); }, view);
      await waitForFrames(page, 6);
      await writeFile(resolve(out, `${name}.png`), await page.locator("#viewport canvas").screenshot({ timeout: 300000 }));
      // PICK=<view>:<x>,<y>;<x>,<y>: what mesh (and material) the picture shows at those points, in the picture's pixels.
      for (const spec of (process.env.PICK ?? "").split(" ").filter(spec => spec.startsWith(`${name}:`))) {
        const points = spec.slice(name.length + 1).split(";").map(point => point.split(",").map(Number));
        const scale = Number(process.env.SCALE ?? 1);
        const hits = await page.evaluate(({ points, scale }) => {
          const s = window.__scene, T = window.__three, canvas = document.querySelector("#viewport canvas"), ray = new T.Raycaster();
          const meshes = []; s.models.traverse(m => { if (m.isMesh && m.visible) meshes.push(m); });
          return points.map(([x, y]) => {
            ray.setFromCamera(new T.Vector2((x / scale) / canvas.clientWidth * 2 - 1, 1 - (y / scale) / canvas.clientHeight * 2), s.camera);
            const hit = ray.intersectObjects(meshes, false)[0];
            const material = hit?.object.material, target = hit && s.sceneBaker?.current?.entries.find(entry => entry.target.mesh === hit.object)?.target;
            return `${x},${y}: ${hit ? `${hit.object.name} [${target?.material ?? material?.type}] at ${hit.point.toArray().map(v => v.toFixed(2))}` : "nothing"}`;
          });
        }, { points, scale });
        console.log(hits.map(line => `pick ${name} ${line}`).join("\n"));
      }
      // FRAMES=1: the frame cost of this view (tests/scene-frames.mjs) at each of FRAMES_SIZES (default 1400x1000 and
      // 1920x1080), plants shown even in a bare view, into <out>/frames.json.
      if (process.env.FRAMES === "1") {
        const { measureFrames } = await import("./scene-frames.mjs");
        const results = frameReport[name] = {};
        const viewport = page.viewportSize();
        await page.evaluate(() => (window.__scene.scene ?? window.__scene.models).traverse(m => { if (m.userData?.foliage) { m.visible = true; if (m.userData.source) m.userData.source.visible = true; } }));
        // The viewport fills the page while frames are measured, so its canvas is the size measured (the Studio's panels
        // otherwise take half the page).
        await page.evaluate(() => { const host = document.querySelector("#viewport"); host.dataset.framesStyle ??= host.getAttribute("style") ?? "";
          Object.assign(host.style, { position: "fixed", left: "0", top: "0", width: "100vw", height: "100vh", zIndex: "50", margin: "0" }); });
        for (const size of (process.env.FRAMES_SIZES ?? "1400x1000,1920x1080").split(",")) {
          const [width, height] = size.split("x").map(Number);
          await page.setViewportSize({ width, height });
          await waitForFrames(page, 10);
          results[size] = await measureFrames(page, { frames: Number(process.env.FRAMES_COUNT ?? 90) });
          const r = results[size];
          console.log(`frames ${name} ${size} (canvas ${r.canvas.join("x")}, ao ${r.ao}, contours ${r.contours}): GPU ${r.steady.gpuMs} ms, page ${r.steady.cpuMs} ms, ${r.steady.calls} draws, ${r.steady.triangles} triangles over ${r.steady.frames} frames; shadow map in a frame ${r.shadowInFrame}, redrawn ${r.shadowForcedMs} ms`);
          console.log(`  passes (draws/triangles): ${Object.entries(r.counted).map(([pass, p]) => `${pass} ${p.calls}/${p.triangles}`).join(", ")}; shadow redraw ${r.shadowRedraw ? `${r.shadowRedraw.calls}/${r.shadowRedraw.triangles}` : "none"}; finished ms ${JSON.stringify(r.msFinished)}; memory ${JSON.stringify(r.memory)}`);
          const scene = r.counted.scene?.categories ?? {};
          console.log(`  scene pass by category: ${Object.entries(scene).sort((a, b) => b[1].calls - a[1].calls).map(([c, v]) => `${c} ${v.calls}/${v.triangles}`).join(", ")}`);
        }
        await page.evaluate(() => { const host = document.querySelector("#viewport"); host.setAttribute("style", host.dataset.framesStyle); delete host.dataset.framesStyle; });
        await page.setViewportSize(viewport);
        await waitForFrames(page, 6);
        await writeFile(resolve(out, "frames.json"), JSON.stringify(frameReport, null, 1));
      }
    }
    // RECORD=<seconds>: records a looping turntable video of the building alone straight from the canvas (MediaRecorder,
    // H.264 MP4 where the browser has it, else WebM) to <out>/building-orbit.<ext>: the camera swings ±ORBIT_DEGREES
    // (default 45) about the building's vertical axis and back, one full sine period, so the last frame meets the first.
    // Everything but the building's own meshes is hidden (plants, the lane, the road, the neighbour stubs).
    // Use with VIEWPORT=1080x1080 for a square video.
    if (process.env.RECORD) {
      const seconds = Number(process.env.RECORD), swing = Number(process.env.ORBIT_DEGREES ?? 45) * Math.PI / 180;
      const centre = [(x0 + front) / 2, cy, (z0 + z1) / 2], fov = Number(process.env.RECORD_FOV ?? 24), recordElevation = Number(process.env.RECORD_ELEVATION ?? 10) * Math.PI / 180;
      const recordDistance = Number(process.env.RECORD_DISTANCE ?? 62);
      // The viewport fills the page, so the canvas is the page's size (the Studio's panels otherwise take half of it).
      await page.evaluate(() => Object.assign(document.querySelector("#viewport").style, { position: "fixed", left: "0", top: "0", width: "100vw", height: "100vh", zIndex: "50", margin: "0" }));
      await waitForFrames(page, 10);
      const result = await page.evaluate(async ({ id, centre, azimuth, swing, seconds, fov, elevation, distance, fps, hide }) => {
        const s = window.__scene;
        // The building only: its own meshes, without the neighbour stubs it stands against.
        const hidden = new RegExp(hide);
        const parts = new Set();
        s.models.traverse(m => { if (m.isMesh) { m.visible = m.name.startsWith(`${id}/`) && !hidden.test(m.name) && !m.userData?.foliage; if (m.visible) parts.add(m.name.split("/").slice(0, 4).join("/")); } });
        (s.scene ?? s.models).traverse(m => { if (m.userData?.foliage) { m.visible = false; if (m.userData.source) m.userData.source.visible = false; } });
        s.renderer.shadowMap.needsUpdate = true;
        s.camera.fov = fov; s.camera.updateProjectionMatrix();
        const place = t => {
          const angle = azimuth + swing * Math.sin(2 * Math.PI * t);
          s.camera.position.set(centre[0] + distance * Math.cos(elevation) * Math.cos(angle), centre[1] + distance * Math.cos(elevation) * Math.sin(angle), centre[2] + distance * Math.sin(elevation));
          s.controls.target.fromArray(centre); s.controls.update(); s.draw();
        };
        place(0);
        for (let i = 0; i < 30; i++) await new Promise(r => requestAnimationFrame(r));
        const canvas = document.querySelector("#viewport canvas");
        // Each frame is drawn, then encoded (WebCodecs, H.264) with its own fixed timestamp, so the video runs at an even
        // `fps` however long a frame takes to draw. The node side wraps the chunks in an MP4 (tools/video/mp4.mjs).
        const count = Math.round(seconds * fps), samples = [];
        const base64 = bytes => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); return btoa(s); };
        let description = null, failure = null;
        const encoder = new VideoEncoder({
          output: (chunk, meta) => { const data = new Uint8Array(chunk.byteLength); chunk.copyTo(data); samples.push({ data: base64(data), key: chunk.type === "key" });
            if (meta?.decoderConfig?.description) description = base64(new Uint8Array(meta.decoderConfig.description)); },
          error: e => { failure = String(e); },
        });
        encoder.configure({ codec: "avc1.640028", width: canvas.width, height: canvas.height, bitrate: 20_000_000, framerate: fps, avc: { format: "avc" } });
        for (let i = 0; i < count; i++) {
          // Captured right after the draw, before the compositor can clear the drawing buffer.
          place(i / count);
          const frame = new VideoFrame(canvas, { timestamp: Math.round(i * 1e6 / fps), duration: Math.round(1e6 / fps) });
          await new Promise(r => requestAnimationFrame(r));
          encoder.encode(frame, { keyFrame: i % fps === 0 });
          frame.close();
          if (encoder.encodeQueueSize > 8) await new Promise(r => setTimeout(r, 5));
        }
        await encoder.flush(); encoder.close();
        return { failure, description, samples, frames: count, size: [canvas.width, canvas.height], parts: [...parts].sort() };
      }, { id: facade.id, centre, azimuth, swing, seconds, fov, elevation: recordElevation, distance: recordDistance, fps: Number(process.env.RECORD_FPS ?? 60),
        hide: process.env.RECORD_HIDE ?? "\\/party|neighbour|frontage|stub" });
      console.log(`record parts shown: ${result.parts.join(" ")}`);
      if (result.failure || !result.description) throw new Error(`encoding failed: ${result.failure ?? "no decoder description"}`);
      const { mp4 } = await import("../tools/video/mp4.mjs");
      const file = resolve(out, "building-orbit.mp4");
      await writeFile(file, mp4({ width: result.size[0], height: result.size[1], fps: Number(process.env.RECORD_FPS ?? 60), description: Buffer.from(result.description, "base64"),
        samples: result.samples.map(s => ({ data: Buffer.from(s.data, "base64"), key: s.key })) }));
      console.log(`Recorded ${seconds} s (${result.frames} frames, canvas ${result.size.join("x")}, H.264) to ${file}`);
    }
    // ORBIT=<frames>: a turntable swing for a video — the reference camera swings ±ORBIT_DEGREES (default 45) about the
    // building's vertical axis and back, one picture per frame in <out>/orbit/frame-NNNN.png (building only, no plants).
    if (process.env.ORBIT) {
      const frames = Number(process.env.ORBIT), swing = Number(process.env.ORBIT_DEGREES ?? 45) * Math.PI / 180;
      await mkdir(resolve(out, "orbit"), { recursive: true });
      const centre = [(x0 + front) / 2, cy, look.height];
      for (let frame = 0; frame < frames; frame++) {
        const angle = azimuth + swing * Math.sin(2 * Math.PI * frame / frames);
        const position = [centre[0] + distance * Math.cos(elevation) * Math.cos(angle), centre[1] + distance * Math.cos(elevation) * Math.sin(angle), look.height + distance * Math.sin(elevation)];
        await page.evaluate(({ position, target, fov }) => { const s = window.__scene;
          (s.scene ?? s.models).traverse(m => { if (m.userData?.foliage) { m.visible = false; if (m.userData.source) m.userData.source.visible = false; } });
          s.camera.fov = fov; s.camera.updateProjectionMatrix(); s.camera.position.fromArray(position); s.controls.target.fromArray(target); s.controls.update(); s.draw(); },
          { position, target: centre, fov: look.fov });
        await waitForFrames(page, 3);
        await writeFile(resolve(out, "orbit", `frame-${String(frame).padStart(4, "0")}.png`), await page.locator("#viewport canvas").screenshot({ timeout: 300000 }));
      }
      console.log(`Wrote ${frames} orbit frames to ${resolve(out, "orbit")}`);
    }
    console.log(`Wrote ${wanted?.length ?? Object.keys(views).length} pictures to ${out}${errors.length ? `; page errors:\n${errors.slice(0, 8).join("\n")}` : ""}`);
  }
} finally {
  await browser.close();
  server.close();
}
