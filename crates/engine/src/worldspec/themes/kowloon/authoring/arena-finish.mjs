/** Bake the washroom MaterialSpec in each original structural member's local frame. */
import { createHash } from "node:crypto";
import { readFile, writeFile, mkdir, rename } from "node:fs/promises";

import { resolve } from "node:path";
import { pavingRepeat, pavingTexture } from "./arena-paving.mjs";
import { openBaker, compile, bakeIdentity } from "../../../../../../../tools/headless/host.mjs";
import { concreteProject } from "../../../../../../../tools/concrete-members.mjs";

const surfaces = new Set([
  "m-floor",
  "m-concrete",
  "m-white",
  "m-roof",
  "m-shell",
  "m-orange",
  "m-blue",
  "m-teal",
]);
const wallSurfaces = new Set(["builtin:concrete"]);
const PAD = 8;
// Authored metres stay in the shader domain. These limits change sampling density only.
const ARENA_PAGE = 2048;
const WALL_PAGE = 4096;
const clean = (v) => v.map((x) => Math.round(x * 100000) / 100000);
const minus = (a, b) => a.map((x, i) => x - b[i]);
const add = (a, b) => a.map((x, i) => x + b[i]);
const times = (v, s) => v.map((x) => x * s);
const dot = (a, b) => a.reduce((s, x, i) => s + x * b[i], 0);
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a) => times(a, 1 / Math.hypot(...a));
const hash = (value) => createHash("sha256").update(value).digest("hex");

function collect(doc, accepted, concreteOnly) {
  const members = new Map();
  for (const node of doc.getRoot().listNodes()) {
    const mesh = node.getMesh();
    if (!mesh) continue;
    const id = node.getExtras().objectId ?? node.getName();
    let member = members.get(id);
    if (!member)
      members.set(
        id,
        (member = {
          id,
          min: [Infinity, Infinity, Infinity],
          max: [-Infinity, -Infinity, -Infinity],
          parts: [],
        }),
      );
    for (const p of mesh.listPrimitives()) {
      if (concreteOnly && !accepted.has(p.getMaterial()?.getName())) continue;
      const pos = p.getAttribute("POSITION");
      const lo = pos.getMin([]),
        hi = pos.getMax([]);
      member.min = member.min.map((x, i) => Math.min(x, lo[i]));
      member.max = member.max.map((x, i) => Math.max(x, hi[i]));
      member.parts.push({ mesh, p });
    }
  }
  return [...members.values()].filter((m) =>
    m.parts.some(({ p }) => accepted.has(p.getMaterial()?.getName())),
  );
}

function plan(members, accepted, proceduralWall) {
  const pageLimit = proceduralWall ? WALL_PAGE : ARENA_PAGE;
  const charts = new Map(),
    assigned = [];
  for (const member of members) {
    const faces = [],
      edges = new Map();
    const size = clean(minus(member.max, member.min));
    for (const { mesh, p } of member.parts) {
      const pos = p.getAttribute("POSITION"),
        indices = p.getIndices();
      const groups = new Map(),
        base = p.getMaterial()?.getName();
      for (let k = 0; k < (indices?.getCount() ?? pos.getCount()); k += 3) {
        const ix = [0, 1, 2].map((j) => (indices ? indices.getScalar(k + j) : k + j));
        const points = ix.map((i) => clean(minus(pos.getElement(i, []), member.min)));
        const n0 = cross(minus(points[1], points[0]), minus(points[2], points[0]));
        if (Math.hypot(...n0) < 1e-9) continue;
        const n = clean(unit(n0)),
          key = [...n, Math.round(dot(n, points[0]) * 10000)].join(",");
        let face = groups.get(key);
        if (!face) {
          face = { n, base, mesh, p, triangles: [] };
          groups.set(key, face);
          if (accepted.has(base)) faces.push(face);
        }
        face.triangles.push({ ix, points });
        for (let i = 0; i < 3; i++) {
          let a = points[i],
            b = points[(i + 1) % 3];
          if (a.join(",") > b.join(",")) [a, b] = [b, a];
          const edgeKey = a.join(",") + ";" + b.join(",");
          let edge = edges.get(edgeKey);
          if (!edge) edges.set(edgeKey, (edge = { a, b, sides: new Map() }));
          edge.sides.set(key, { n, other: points[(i + 2) % 3] });
        }
      }
    }
    const features = [];
    for (const { a, b, sides } of edges.values()) {
      const [first, second] = sides.values();
      if (!second) continue; // Coplanar triangulation and open seams do not form rounded corners.
      const angle = (Math.acos(Math.max(-1, Math.min(1, dot(first.n, second.n)))) * 180) / Math.PI;
      if (angle < 1 || angle > 179) continue;
      const middle = clean(unit(add(first.n, second.n)));
      const concave = dot(minus(second.other, a), first.n) > 0.0001;
      features.push([...a, Number(concave), ...b, angle, ...middle, 1]);
    }
    features.sort((a, b) => a.join(",").localeCompare(b.join(",")));
    // A seed is appearance, not a texture unique to every placement. Repeated members share variants.
    const variant = parseInt(hash(JSON.stringify(clean(member.min))).slice(0, 2), 16) % 3;
    // MapKit's visible treads stand on its base plane and abut their neighbours along
    // the run. Their top heights differ by one riser; armoured walls and piers do not.
    const tread =
      member.min[2] < -3.9 &&
      members.some((other) => {
        const rise = Math.abs(other.max[2] - member.max[2]);
        if (rise < 0.04 || rise > 0.45 || Math.abs(other.min[2] - member.min[2]) > 0.001) return false;
        return [0, 1].some((axis) => {
          const along = 1 - axis;
          return (
            Math.abs(other.min[along] - member.min[along]) < 0.001 &&
            Math.abs(other.max[along] - member.max[along]) < 0.001 &&
            (Math.abs(other.min[axis] - member.max[axis]) < 0.001 ||
              Math.abs(other.max[axis] - member.min[axis]) < 0.001)
          );
        });
      });
    for (const f of faces) {
      const deck = size[2] <= 0.45 && Math.min(size[0], size[1]) >= 0.6;
      if (
        !proceduralWall && f.n[2] > 0.9 &&
        (f.base === "m-floor" || f.base === "m-roof" || member.id.endsWith("-pavement") || deck || tread)
      ) {
        assigned.push({ face: f, chart: { page: -1 } });
        continue;
      }
      const u = unit(cross(Math.abs(f.n[2]) > 0.95 ? [0, 1, 0] : [0, 0, 1], f.n)),
        v = unit(cross(f.n, u));
      const points = f.triangles.flatMap((t) => t.points),
        plane = dot(f.n, points[0]);
      const us = points.map((q) => dot(q, u)),
        vs = points.map((q) => dot(q, v));
      const u0 = Math.min(...us),
        v0 = Math.min(...vs),
        width = Math.max(...us) - u0,
        height = Math.max(...vs) - v0;
      if (!(width > 0 && height > 0)) continue;
      const largeGround = Math.abs(f.n[2]) > 0.8 && width * height > 28;
      const buried = dot(f.n, [0, 0, 1]) < -0.8 && member.min[2] < 0.2;
      const density = buried ? 32 : proceduralWall ? 560 : largeGround ? 96 : 224;
      const scale = Math.min(density, (pageLimit - 2 * PAD) / Math.max(width, height));
      const w = Math.max(2, Math.ceil(width * scale)) + 2 * PAD,
        h = Math.max(2, Math.ceil(height * scale)) + 2 * PAD;
      const origin = clean(add(add(times(u, u0), times(v, v0)), times(f.n, plane)));
      const recipe =
        f.base === "m-floor" && f.n[2] > 0.8 ? "slab" : proceduralWall || member.min[2] > 0.25 ? "elevated" : "wall";
      const domain = {
        width: w,
        height: h,
        gutter: PAD,
        origin,
        u: clean(times(u, width)),
        v: clean(times(v, height)),
        uvOrigin: [0, 0],
        uvU: [1, 0],
        uvV: [0, 1],
        normal: f.n,
        mapping: "world",
        extent: [width, height],
        objectOrigin: [0, 0, 0],
        boundsMin: [0, 0, 0],
        boundsMax: size,
        gravity: [0, 0, -1],
        features: features.flat(),
      };
      const group = recipe + "-" + variant,
        key = hash(JSON.stringify([group, domain]));
      let chart = charts.get(key);
      if (!chart) charts.set(key, (chart = { key, group, domain, w, h }));
      // Street-level piers and thin wall panels are seen from arm's length. Resolve their
      // top streaks, chipped arrises and damp foot independently of the shared fine aggregate.
      const narrow = Math.min(size[0], size[1]), span = Math.max(size[0], size[1]);
      const streetLevel = member.min[2] >= -2.1 && member.min[2] < 1 && member.max[2] >= 1.5;
      const upright = Math.abs(f.n[2]) < 0.2 && size[2] >= 1.6;
      if (!proceduralWall && streetLevel && upright && (span <= 1.6 || (span <= 8 && narrow <= 0.75))) chart.closeConcrete = true;
      assigned.push({ face: f, chart, u, v, u0, v0, width, height });
    }
  }
  // Bound GPU residency independent of map size. Short pillars and rails keep more texels per
  // metre than large decks and buried undersides. Normal/property atlases use lower densities.
  const tiles = [...charts.values()];
  const totalPixels = tiles.reduce((sum, c) => sum + c.w * c.h, 0);
  const scale = Math.min(1, Math.sqrt(116_000_000 / totalPixels));
  for (const c of tiles) {
    c.w = c.domain.width = 2 * PAD + Math.max(2, Math.round((c.w - 2 * PAD) * scale));
    c.h = c.domain.height = 2 * PAD + Math.max(2, Math.round((c.h - 2 * PAD) * scale));
  }
  const close = tiles.filter(c => c.closeConcrete);
  const closePixels = close.reduce((sum, c) => sum + (c.w - 2 * PAD) * (c.h - 2 * PAD), 0);
  // Spend at most 20M additional chart texels on close receivers, preserving all base densities.
  const gain = Math.min(2.25, Math.sqrt(1 + 20_000_000 / Math.max(1, closePixels)));
  for (const c of close) {
    const maxGain = (pageLimit - 2 * PAD) / Math.max(c.w - 2 * PAD, c.h - 2 * PAD);
    const fit = Math.min(gain, maxGain);
    c.w = c.domain.width = 2 * PAD + Math.round((c.w - 2 * PAD) * fit);
    c.h = c.domain.height = 2 * PAD + Math.round((c.h - 2 * PAD) * fit);
  }
  const sheets = [];
  for (const c of tiles.sort(
    (a, b) => Math.max(b.w, b.h) - Math.max(a.w, a.h) || b.w * b.h - a.w * a.h || a.key.localeCompare(b.key),
  )) {
    let best,
      score = Infinity,
      page = -1;
    const detailed = proceduralWall || Boolean(c.closeConcrete);
    for (const [i, sheet] of sheets.entries()) {
      if (sheet.detailed !== detailed) continue;
      for (const r of sheet.free) {
        if (r.w < c.w || r.h < c.h) continue;
        const remaining = Math.min(r.w - c.w, r.h - c.h) * pageLimit + Math.max(r.w - c.w, r.h - c.h);
        if (remaining < score) {
          best = r;
          score = remaining;
          page = i;
        }
      }
    }
    if (!best) {
      page = sheets.length;
      best = { x: 0, y: 0, w: pageLimit, h: pageLimit };
      sheets.push({ free: [best], charts: [], w: 0, h: 0, detailed });
    }
    const sheet = sheets[page];
    c.x = best.x;
    c.y = best.y;
    c.page = page;
    sheet.charts.push(c);
    sheet.w = Math.max(sheet.w, c.x + c.w);
    sheet.h = Math.max(sheet.h, c.y + c.h);
    const right = c.x + c.w,
      bottom = c.y + c.h,
      next = [];
    for (const r of sheet.free) {
      if (c.x >= r.x + r.w || c.y >= r.y + r.h || right <= r.x || bottom <= r.y) {
        next.push(r);
        continue;
      }
      if (r.x < c.x) next.push({ ...r, w: c.x - r.x });
      if (r.y < c.y) next.push({ ...r, h: c.y - r.y });
      if (right < r.x + r.w) next.push({ ...r, x: right, w: r.x + r.w - right });
      if (bottom < r.y + r.h) next.push({ ...r, y: bottom, h: r.y + r.h - bottom });
    }
    sheet.free = next.filter(
      (r, i) =>
        !next.some(
          (s, j) =>
            j !== i &&
            r.x >= s.x &&
            r.y >= s.y &&
            r.x + r.w <= s.x + s.w &&
            r.y + r.h <= s.y + s.h &&
            (r.x !== s.x || r.y !== s.y || r.w !== s.w || r.h !== s.h || j < i),
        ),
    );
  }
  // Crop unused atlas space, including on slender wall-cap prototypes. Round to the
  // coarsest channel grid so all three channel images have the same chart boundaries.
  for (const sheet of sheets) {
    sheet.w = Math.ceil(sheet.w / 4) * 4;
    sheet.h = Math.ceil(sheet.h / 4) * 4;
  }
  return { assigned, sheets };
}

function rebind(doc, assigned, materials, paving) {
  const pavedMaterials = new Map();
  const buffer = doc.getRoot().listBuffers()[0],
    byPrimitive = new Map();
  for (const assignment of assigned) {
    const { face, chart } = assignment;
    let pages = byPrimitive.get(face.p);
    if (!pages) byPrimitive.set(face.p, (pages = { mesh: face.mesh, groups: new Map() }));
    let list = pages.groups.get(chart.page);
    if (!list) pages.groups.set(chart.page, (list = []));
    list.push(assignment);
  }
  for (const [p, { mesh, groups }] of byPrimitive) {
    const base = p.getMaterial();
    const tint = { "m-orange": [0.84, 0.48, 0.32], "m-blue": [0.42, 0.6, 0.7], "m-teal": [0.55, 0.69, 0.6] }[
      base.getName()
    ] ?? [1, 1, 1];
    for (const [page, assignments] of groups) {
      let material, pageW, pageH;
      if (page === -1) {
        if (!pavedMaterials.has(base)) {
          const mat = base
            .clone()
            .setName(base.getName() + "-paving")
            .setExtras({ ...base.getExtras(), worldSpecAggregate: true })
            .setBaseColorTexture(paving);
          if (["m-white", "m-floor", "m-roof", "m-concrete", "m-shell"].includes(base.getName()))
            mat.setBaseColorFactor([0.88, 0.87, 0.84, 1]);
          if (base.getName() === "m-teal") mat.setBaseColorFactor([0.46, 0.53, 0.47, 1]);
          pavedMaterials.set(base, mat);
        }
        material = pavedMaterials.get(base);
      } else ({ material, width: pageW, height: pageH } = materials[page]);
      const pos = [],
        norm = [],
        uv = [],
        col = [],
        originalColor = p.getAttribute("COLOR_0");
      for (const { face: f, chart: c, u, v, u0, v0, width, height } of assignments)
        for (const tri of f.triangles) {
          for (let j = 0; j < 3; j++) {
            const index = tri.ix[j],
              q = tri.points[j];
            pos.push(...p.getAttribute("POSITION").getElement(index, []));
            norm.push(...p.getAttribute("NORMAL").getElement(index, []));
            const vertex = originalColor?.getElement(index, []) ?? [1, 1, 1];
            if (page === -1) {
              const xy = p.getAttribute("POSITION").getElement(index, []);
              uv.push(xy[0] / pavingRepeat, xy[1] / pavingRepeat);
              col.push(...vertex.slice(0, 3));
            } else {
              const s = (dot(q, u) - u0) / width,
                t = (dot(q, v) - v0) / height;
              uv.push((c.x + PAD + s * (c.w - 2 * PAD)) / pageW, (c.y + PAD + t * (c.h - 2 * PAD)) / pageH);
              col.push(...tint.map((m, i) => m * vertex[i]));
            }
          }
        }
      const create = (arr, type) =>
        doc.createAccessor().setBuffer(buffer).setType(type).setArray(new Float32Array(arr));
      mesh.addPrimitive(
        doc
          .createPrimitive()
          .setMaterial(material)
          .setAttribute("POSITION", create(pos, "VEC3"))
          .setAttribute("NORMAL", create(norm, "VEC3"))
          .setAttribute("COLOR_0", create(col, "VEC3"))
          .setAttribute("TEXCOORD_0", create(uv, "VEC2")),
      );
    }
    mesh.removePrimitive(p);
    p.dispose();
  }
}

// Bind the canonical recipe, images, defaults and channel layout through Material Studio.
// The nine tiny receivers only compile the two contact conditions, floor finish and seed choices.
// Actual evaluation below supplies each real face and its member's complete set of creases.
async function bindMaterials(host) {
  const { Mesh, writeGlb } = await import("../../../../../../../tests/studio/washroom/geometry.mjs");
  const ids = [],
    examples = [];
  for (let i = 0; i < 9; i++) {
    const mesh = new Mesh("finish-" + i, [1, 1, 1]);
    mesh.quad([0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1], [0, -1, 0]);
    examples.push(mesh);
    ids.push(`concrete/node/${i}/mesh/${i}/primitive/0`);
  }
  const project = concreteProject(writeGlb(examples), [ids[0]], {
    elevated: [ids[3]],
    resolution: 64,
    texelDensity: 32,
  });
  const [wall, elevated] = project.materials;
  project.materials = ids.map((id, i) => {
    const source = i >= 3 && i < 6 ? elevated : wall;
    return {
      ...source,
      id: ["wall", "elevated", "slab"][Math.floor(i / 3)] + "-" + (i % 3),
      // Break up the aggregate transition with source pigment. Keep the dry
      // concrete matte away from mechanically burnished arrises.
      parameters: {
        ...source.parameters, aggregateFinish: 1, flatten: 0.67,
        darknessA: 0.105, darknessB: 0.115, mottlingBreakup: 1, jaggedness: 1.02,
        illustration: 0.40, edgeWidth: 0.027, edgeContrast: 0.96, edgeSpecular: 0.74,
        intactRoughness: 0.79, ...(i >= 6 ? { ground: true } : {}),
      },
      seed: [7, 29, 71][i % 3],
      meshes: [id],
    };
  });
  const { metadata } = await compile("shade", { ...project, includeScenePacket: false });
  const recipes = new Map();
  for (const slot of project.materials) {
    const material = metadata.materials[slot.id];
    recipes.set(slot.id, { material, uniforms: await host.uniforms(material, slot.parameters, project.assets) });
  }
  return recipes;
}

async function bakeSheet(host, recipes, sheet) {
  const roles = ["baseColor", "normal", "properties"];
  const defaults = [[164,162,154,255], [128,128,255,255], [255,205,0,255]];
  const pages = defaults.map(rgba => Buffer.alloc(sheet.w * sheet.h * 4, Buffer.from(rgba)));
  for (const chart of sheet.charts) {
    const recipe = recipes.get(chart.group);
    const baked = await host.baker.bake(recipe.material, recipe.uniforms, chart.domain);
    if (!baked) throw Error("Interrupted arena chart " + chart.key);
    const pixels = host.baker.readBack(baked);
    for (const [i, role] of roles.entries()) {
      const src = pixels[role], stride = chart.w * 4;
      for (let row = 0; row < chart.h; row++)
        pages[i].set(src.subarray(row * stride, (row + 1) * stride), ((chart.y + row) * sheet.w + chart.x) * 4);
    }
  }
  return pages;
}

export async function finishArena(doc, directory, { sharp, proceduralWall = false }) {
  const accepted = proceduralWall ? wallSurfaces : surfaces;
  const { assigned, sheets } = plan(collect(doc, accepted, proceduralWall), accepted, proceduralWall),
    materials = [];
  if (proceduralWall && !assigned.length) throw Error("Procedural wall has no concrete receiver");
  const cache = resolve(directory, "arena-surface-cache");
  await mkdir(cache, { recursive: true });
  const dependencies = [
    new URL("./arena-finish.mjs", import.meta.url),
    new URL("./arena-paving.mjs", import.meta.url),
    new URL("../../../../../../../tools/concrete-members.mjs", import.meta.url),
    ...[
      "kowloon-concrete.material.ts",
      "kowloon-noise.material.ts",
      "textures/concrete-plain.png",
      "textures/concrete-fine-sand.png",
    ].map(
      (n) =>
        new URL("../../../../materialspec/program/examples/" + n, import.meta.url),
    ),
    new URL(
      "../materials/street-materials.json",
      import.meta.url,
    ),
    new URL("../../../../../../../browser/renderer/bake.ts", import.meta.url),
  ];
  const host = openBaker();
  try {
    const recipes = await bindMaterials(host);
    const version = hash(JSON.stringify([await bakeIdentity(host), [...host.inputs].sort(), ...await Promise.all(dependencies.map(async p => hash(await readFile(p))))]));
    console.log(
      "Arena concrete:",
      assigned.length,
      "faces,",
      sheets.reduce((sum, s) => sum + s.charts.length, 0),
      "distinct charts,",
      sheets.length,
      "sheets",
    );
    for (const [i, sheet] of sheets.entries()) {
      const key = hash(
        JSON.stringify([version, sheet.w, sheet.h, sheet.charts.map((c) => [c.key, c.x, c.y, c.w, c.h])]),
      );
      const files = ["albedo", "normal", "properties"].map((role) =>
        resolve(cache, key + "-" + role + ".jpg"),
      );
      let images;
      try {
        images = await Promise.all(files.map((f) => readFile(f)));
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
        const pages = await bakeSheet(host, recipes, sheet);
        // Nearby receivers keep full-density normals and half-density roughness.
        // Grouping by this policy avoids inflating the unrelated deck and roof pages.
        const divisors = sheet.detailed ? [1, 1, 2] : [1, 2, 4];
        images = await Promise.all(
          pages.map((rgba, j) => {
            let image = sharp(rgba, { raw: { width: sheet.w, height: sheet.h, channels: 4 } }).removeAlpha();
            if (divisors[j] > 1)
              image = image.resize(
                Math.max(1, Math.round(sheet.w / divisors[j])),
                Math.max(1, Math.round(sheet.h / divisors[j])),
                { fit: "fill" },
              );
            return image.jpeg({ quality: 93, chromaSubsampling: "4:4:4" }).toBuffer();
          }),
        );
        for (let j = 0; j < 3; j++) {
          const temporary = files[j] + "." + process.pid + ".tmp";
          await writeFile(temporary, images[j]);
          await rename(temporary, files[j]);
        }
      }
      const [color, normal, properties] = images.map((data, j) =>
        doc
          .createTexture(`Washroom concrete ${i} ${["color", "normal", "properties"][j]}`)
          .setMimeType("image/jpeg")
          .setImage(data),
      );
      const material = doc
        .createMaterial((proceduralWall ? "Procedural wall concrete " : "Arena concrete ") + i)
        .setBaseColorTexture(color)
        .setNormalTexture(normal)
        .setMetallicRoughnessTexture(properties)
        .setMetallicFactor(1)
        .setRoughnessFactor(1);
      material.setExtras({
        worldSpecConcrete: true,
        worldSpecConcreteEdge: true,
        ...(proceduralWall ? { worldSpecProceduralWall: true } : {}),
      });
      materials.push({ material, width: sheet.w, height: sheet.h });
      console.log("Arena concrete sheet", i + 1, "/", sheets.length);
    }
  } finally {
    host.close();
  }
  rebind(doc, assigned, materials, proceduralWall ? null : await pavingTexture(doc, sharp));
}
