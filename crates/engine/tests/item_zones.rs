//! Weathering zones an item states: a panel's rule hangs a zone below its top and its sills,
//! which stops at the openings below and never rises above its edge; an authored feature may
//! carry a zone of its own; and a material reads the zone's coordinates (`zoneCoord`,
//! `zoneExtent`, `zoneSpan`) where the receiver is, on the CPU and, from the zones packed for
//! it, in a GPU host's bake shader.
//! Expectations come from the definitions' dimensions.
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::{
    authoring::{
        Diagnostic, Mapping, MaterialAssignment, ModelSource, Project, Session, item_features,
    },
    itemspec::definition::{self as item, GeneratedItem, Values},
    materialspec::program::{NamedFeatures, ZoneHit},
};

fn absent(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("Missing {name}"))
}

/// A 4 m by 3 m wall (front at y = 0, facing -y) with a window 1 m wide from x = 1, its sill at
/// z = 1 and head at z = 2.2, and a door 0.8 m wide from x = 2.6, its head at z = 2.03.
fn wall(zones: Value, features: Value) -> String {
    json!({
        "format": "itemspec", "version": 2, "id": "zoned-wall",
        "parameters": {"u0": {"type": "number", "default": 5, "min": -100, "max": 100}},
        "parts": [{
            "id": "wall",
            "generator": {"kind": "panel", "width": 4, "height": 3, "thickness": 0.25,
                "openings": [
                    {"id": "window", "width": 1, "height": 1.2, "u": {"from": "start", "offset": 1}, "v": {"from": "start", "offset": 1},
                     "sill": {"depth": 0.05, "overhang": 0.05}},
                    {"id": "door", "width": 0.8, "height": 2, "u": {"from": "start", "offset": 2.6}, "v": {"from": "start", "offset": 0.03}}
                ]},
            "zones": zones
        }],
        "features": features
    })
    .to_string()
}

fn generate(text: &str) -> GeneratedItem {
    let definition = item::parse("zoned.item.json", text).unwrap();
    item::generate(&definition, &Values::new(), &absent, &|| false)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// The item's features as a receiver at the origin is given them.
fn named(generated: &GeneratedItem) -> NamedFeatures {
    let identity = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    NamedFeatures::new(&item_features(&generated.geometry.features, &identity)).unwrap()
}

const FRONT: [f64; 3] = [0., -1., 0.];

fn at(named: &NamedFeatures, zone: &str, x: f64, z: f64) -> Option<ZoneHit> {
    named
        .zones(zone)
        .expect("a zone of the name")
        .at([x, 0., z], FRONT)
        .0
}

#[test]
fn a_zone_hangs_below_its_edge_and_stops_at_the_openings_below() {
    let generated = generate(&wall(
        json!([
            {"name": "runoff", "below": "top", "face": "front", "length": 2.5, "u0": {"parameter": "u0"}},
            {"name": "sill-runoff", "below": "sill", "face": "front", "length": 0.6, "u0": {"parameter": "u0"}}
        ]),
        json!([]),
    ));
    let named = named(&generated);
    // Below the top, between the openings: u is u0 plus x, v the drop from z = 3.
    let hit = at(&named, "runoff", 0.5, 2.).unwrap();
    assert!(
        (hit.u - 5.5).abs() < 1e-9 && (hit.v - 1.).abs() < 1e-9,
        "{hit:?}"
    );
    assert_eq!(hit.limit, 2.5);
    // Never above the edge; never past its length.
    assert!(at(&named, "runoff", 0.5, 3.05).is_none());
    assert!(at(&named, "runoff", 0.5, 0.45).is_none());
    // Over the window the column ends at its head (z = 2.2), over the door at its head
    // (z = 2.03).
    let over = at(&named, "runoff", 1.5, 2.5).unwrap();
    assert!((over.limit - 0.8).abs() < 1e-9, "{over:?}");
    assert!(at(&named, "runoff", 1.5, 2.15).is_none());
    assert!(
        at(&named, "runoff", 1.5, 0.9).is_none(),
        "not below the window either"
    );
    assert!((at(&named, "runoff", 3., 2.05).unwrap().limit - 0.97).abs() < 1e-9);
    assert!(at(&named, "runoff", 3., 1.9).is_none());
    // Below the sill (z = 1, with its 5 cm overhang): from the sill down, not above it, and
    // not beside it.
    let sill = at(&named, "sill-runoff", 1.02, 0.7).unwrap();
    assert!(
        (sill.u - 6.02).abs() < 1e-9 && (sill.v - 0.3).abs() < 1e-9,
        "{sill:?}"
    );
    assert!(at(&named, "sill-runoff", 1.5, 1.1).is_none());
    assert!(at(&named, "sill-runoff", 0.9, 0.7).is_none());
    assert!(at(&named, "sill-runoff", 0.97, 0.7).is_some());
    assert!(
        at(&named, "sill-runoff", 1.5, 0.35).is_none(),
        "past its length"
    );
    // The back face and the wall's edge faces are not the front's zone.
    let zones = named.zones("runoff").unwrap();
    assert!(zones.at([0.5, 0.25, 2.], [0., 1., 0.]).0.is_none());
    assert!(zones.at([0., 0.1, 2.], [-1., 0., 0.]).0.is_none());
    // A rule named after its feature puts the zone on the feature itself; the generated
    // features stay as they were for distance queries.
    let tops = generated
        .geometry
        .features
        .iter()
        .filter(|f| f.name == "top")
        .count();
    assert_eq!(tops, 2);
    assert!(
        generated
            .geometry
            .features
            .iter()
            .all(|f| f.name != "runoff" || f.zone.is_some())
    );
}

/// `ms_zone_find` of the GPU runtime (`glsl.rs`, `ZONES`), read off the packed rows in single
/// precision as a GPU does: the shader's algorithm against the CPU's, through the packing.
fn shader_zone(
    rows: &[f64],
    count: usize,
    place: usize,
    p: [f32; 3],
    n: [f32; 3],
) -> Option<[f32; 6]> {
    let texel = |row: usize, column: usize| -> [f32; 4] {
        let at = (row * 7 + column) * 4;
        std::array::from_fn(|i| rows[at + i] as f32)
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let xyz = |t: [f32; 4]| [t[0], t[1], t[2]];
    let mut best: Option<[f32; 6]> = None;
    for i in 0..count {
        let kind = texel(i, 5);
        if (kind[0] + 0.5) as usize != place {
            continue;
        }
        let (start, along, down, across) = (texel(i, 0), texel(i, 1), texel(i, 2), texel(i, 3));
        let from = [p[0] - start[0], p[1] - start[1], p[2] - start[2]];
        let off = dot(from, xyz(across));
        if off.abs() > down[3] || dot(n, xyz(across)).abs() < 0.5 {
            continue;
        }
        let q: [f32; 3] = std::array::from_fn(|k| from[k] - across[k] * off);
        let on_down = dot(q, xyz(down));
        let t = (dot(q, xyz(along)) - kind[1] * on_down) / kind[2];
        if !(0. ..=1.).contains(&t) {
            continue;
        }
        let v = on_down - t * kind[1];
        if v < -1e-4 {
            continue;
        }
        let v = v.max(0.);
        if best.is_some_and(|b| b[1] <= v) {
            continue;
        }
        let u = start[3] + t * along[3];
        let zone = texel(i, 4);
        let mut limit = across[3];
        let mut left = false;
        for c in 0..(zone[3] + 0.5) as usize {
            let row = (zone[2] + 0.5) as usize + c;
            let clip = texel(row, 0);
            if u < clip[0] || u > clip[1] {
                continue;
            }
            if texel(row, 1)[0] < 0.5 {
                limit = limit.min(clip[2]);
            } else if v >= clip[2] && v <= clip[3] {
                left = true;
            }
        }
        if v > limit || left {
            continue;
        }
        best = Some([u, v, zone[0], limit, zone[1], texel(i, 6)[1]]);
    }
    best
}

#[test]
fn the_packed_zones_a_gpu_reads_give_what_the_cpu_finds() {
    use superspec_engine::materialspec::program::pack_zones;
    let generated = generate(&wall(
        json!([
            {"name": "runoff", "below": "top", "face": "front", "length": 2.5, "u0": {"parameter": "u0"}},
            {"name": "sill-runoff", "below": "sill", "face": "front", "length": 0.6, "u0": {"parameter": "u0"}}
        ]),
        json!([{"name": "drip", "part": "wall", "points": [[0.2, 0, 2.6], [0.6, 0, 2.6]], "zone": {"length": 0.8}}]),
    ));
    let identity = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    let features = item_features(&generated.geometry.features, &identity);
    let named = NamedFeatures::new(&features).unwrap();
    // A layer that reads `zone`, following the top's runoff and the drip together.
    let follow = [(
        "zone".to_owned(),
        superspec_engine::materialspec::program::Follow::Many(vec!["drip".into(), "runoff".into()]),
    )]
    .into();
    let names = ["sill-runoff".to_owned(), "zone".to_owned()];
    let wall_box = [[0., 0., 0.], [4., 0., 3.]];
    let (rows, count) = pack_zones(&features, &names, &follow, wall_box);
    assert_eq!(rows.len() % 28, 0);
    // A receiver nowhere near is given none of them.
    assert_eq!(
        pack_zones(&features, &names, &follow, [[20., 0., 0.], [21., 0., 3.]]).1,
        0
    );
    let followed = NamedFeatures::new(&superspec_engine::materialspec::program::followed_features(
        &features, &follow,
    ))
    .unwrap();
    let mut compared = 0;
    for (place, name) in names.iter().enumerate() {
        let set = if name == "zone" {
            followed.zones("zone")
        } else {
            named.zones(name)
        }
        .unwrap();
        for i in 0..=80 {
            for j in 0..=60 {
                // (Off the stops' and openings' exact edges, where single precision may go either way.)
                let p = [0.05 * i as f64 + 0.0071, 0., 0.05 * j as f64 + 0.0123];
                let cpu = set.at(p, FRONT).0;
                let gpu = shader_zone(&rows, count, place, p.map(|v| v as f32), [0., -1., 0.]);
                match (cpu, gpu) {
                    (None, None) => {}
                    (Some(cpu), Some(gpu)) => {
                        compared += 1;
                        let close = |a: f64, b: f32| (a - f64::from(b)).abs() < 1e-4;
                        assert!(
                            close(cpu.u, gpu[0])
                                && close(cpu.v, gpu[1])
                                && close(cpu.id, gpu[2])
                                && close(cpu.limit, gpu[3])
                                && close(cpu.scale, gpu[4])
                                && close(cpu.span[1], gpu[5]),
                            "{name} at {p:?}: {cpu:?} against {gpu:?}"
                        );
                    }
                    (cpu, gpu) => panic!("{name} at {p:?}: {cpu:?} against {gpu:?}"),
                }
            }
        }
    }
    assert!(compared > 1000, "{compared}");
}

#[test]
fn clipping_leaves_an_opening_out_and_lets_the_zone_go_on_below_it() {
    let generated = generate(&wall(
        json!([{"name": "runoff", "below": "top", "face": "front", "length": 3, "until": "none", "clip": "openings"}]),
        json!([]),
    ));
    let named = named(&generated);
    assert!(
        at(&named, "runoff", 1.5, 1.5).is_none(),
        "inside the window"
    );
    let below = at(&named, "runoff", 1.5, 0.5).unwrap();
    assert!((below.v - 2.5).abs() < 1e-9 && below.limit == 3.);
}

#[test]
fn an_authored_feature_carries_its_own_zone_and_ids_differ_per_edge() {
    let generated = generate(&wall(
        json!([]),
        json!([
            {"name": "drip", "part": "wall", "points": [[0.2, 0, 2.6], [0.6, 0, 2.6]], "zone": {"length": 0.8, "u0": 1}},
            {"name": "drip", "part": "wall", "points": [[3.6, 0, 2.6], [3.9, 0, 2.6]], "zone": {"length": 0.8, "scale": 2}}
        ]),
    ));
    let named = named(&generated);
    let first = at(&named, "drip", 0.3, 2.).unwrap();
    let second = at(&named, "drip", 3.7, 2.).unwrap();
    assert!((first.u - 1.1).abs() < 1e-9 && (second.u - 0.1).abs() < 1e-9);
    assert_eq!((first.scale, second.scale), (1., 2.));
    // Each zone's span: where its edge starts and ends along u.
    assert!((first.span[0] - 1.).abs() < 1e-9 && (first.span[1] - 1.4).abs() < 1e-9);
    assert!(second.span[0].abs() < 1e-9 && (second.span[1] - 0.3).abs() < 1e-9);
    assert_ne!(first.id, second.id);
    assert!(at(&named, "drip", 2., 2.).is_none());
    // A zone that is not one is refused at its definition.
    let bad = wall(
        json!([]),
        json!([{"name": "drip", "part": "wall", "points": [[0.2, 0, 2.6], [0.6, 0, 2.6]], "zone": {"length": 0}}]),
    );
    let definition = item::parse("zoned.item.json", &bad).unwrap();
    let error = item::generate(&definition, &Values::new(), &absent, &|| false)
        .err()
        .unwrap();
    assert_eq!(error.code, "ITEM_ZONE");
}

/// The zoned wall with a layer over its zones, baked natively, for `tests/item-zone-parity.mjs`
/// to build in Wasm and compare byte for byte (`ZONE_PARITY_OUTPUT=<dir>` writes the project
/// and its packet).
#[test]
fn native_zone_scene_packet_for_parity() {
    let mut project = project();
    project
        .modules
        .insert("zone-layer.material.ts".into(), LAYER.into());
    let mut definition: Value = serde_json::from_str(&project.modules["wall.item.json"]).unwrap();
    definition["layers"] = json!({"front": [
        {"entry": "zone-layer.material.ts", "seed": 1, "follow": {"zone": "runoff"}}
    ]});
    project
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    let asset = |name: &str| absent(name);
    let output = Session::default()
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let packet = output.scene(1).unwrap().packet();
    assert_eq!(&packet[..4], b"LSP3");
    if let Ok(directory) = std::env::var("ZONE_PARITY_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join("packet.bin"), packet).unwrap();
    }
}

/// A weathering layer over whatever zones it follows as `zone`.
const LAYER: &str = r#"import { defineLayer, zoneCoord, zoneExtent, vec3 } from "@superspec/material/v1";
const zone = zoneCoord("zone");
const inside = zoneExtent("zone");
export default defineLayer({ name: "Zone grime", coverage: inside.x, baseColor: vec3(0.1, 0.1, 0.1) });
"#;

#[test]
fn a_layer_follows_a_list_of_zones_by_its_own_name() {
    let mut project = project();
    project
        .modules
        .insert("zone-layer.material.ts".into(), LAYER.into());
    let mut definition: Value = serde_json::from_str(&project.modules["wall.item.json"]).unwrap();
    definition["features"] = json!([
        {"name": "drip", "part": "wall", "points": [[0.1, 0, 0.4], [0.6, 0, 0.4]], "zone": {"length": 0.3}}
    ]);
    definition["layers"] = json!({"front": [
        {"entry": "zone-layer.material.ts", "seed": 1, "follow": {"zone": ["runoff", "drip"]}}
    ]});
    project
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    let asset = |name: &str| absent(name);
    let mut session = Session::default();
    let mesh = "wall/part/wall/wall/front";
    let field = |session: &mut Session, project: &Project| {
        session
            .bake_project_fields(project, mesh, &["stack.1/zone".into()], 1, &asset, &|| {
                false
            })
            .unwrap_or_else(|errors| panic!("{}", messages(errors)))
            .fields
            .remove(0)
    };
    let both = field(&mut session, &project);
    // The drip's zone, 0.4 m up, holds texels the top's (down to 0.5 m) does not.
    let low = |texels: &[f32]| {
        texels
            .chunks_exact(4)
            .filter(|t| t[1] >= 0. && t[0] < 1. && t[2] != 0.)
            .count()
    };
    assert!(
        low(&both) > 0,
        "the drip's zone is seen under the layer's name"
    );
    let mut alone = project.clone();
    let mut definition: Value = serde_json::from_str(&alone.modules["wall.item.json"]).unwrap();
    definition["layers"]["front"][0]["follow"] = json!({"zone": "runoff"});
    alone
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    assert_eq!(low(&field(&mut Session::default(), &alone)), 0);
    // The layer composes over the base on the CPU as well.
    session
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
}

/// Reads the zone the layer follows as `zone` in a field (so a GPU host bakes it on the CPU).
const ZONED: &str = r#"import { defineMaterial, zoneCoord, zoneExtent, vec3, mul, add } from "@superspec/material/v1";
const zone = zoneCoord("runoff");
const extent = zoneExtent("runoff");
export default defineMaterial({
  name: "Zone probe",
  base: { baseColor: vec3(mul(zone.x, 0.05), add(mul(zone.y, 0.25), 0.25), mul(extent.x, 0.5)), roughness: 0.5, metalness: 0 },
});
"#;

fn messages(errors: Vec<Diagnostic>) -> String {
    errors
        .into_iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn project() -> Project {
    let text = wall(
        json!([{"name": "runoff", "below": "top", "face": "front", "length": 2.5, "u0": {"parameter": "u0"}}]),
        json!([]),
    );
    let modules: BTreeMap<String, String> = [
        ("zoned.material.ts", ZONED.to_owned()),
        ("wall.item.json", text),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text))
    .collect();
    Project {
        schema_version: 2,
        kind: "material".into(),
        entry: "zoned.material.ts".into(),
        modules,
        resolution: 64,
        mapping: Mapping::World,
        models: vec![ModelSource {
            id: "wall".into(),
            kind: "item".into(),
            entry: Some("wall.item.json".into()),
            slots: [("front".to_owned(), "zoned".to_owned())].into(),
            transform: Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [10., 0., 0.]]),
            ..ModelSource::default()
        }],
        materials: vec![MaterialAssignment {
            id: "zoned".into(),
            entry: "zoned.material.ts".into(),
            ..MaterialAssignment::default()
        }],
        ..Project::default()
    }
}

#[test]
fn a_material_reads_its_receivers_zones_and_a_gpu_host_is_given_them_packed() {
    let project = project();
    let asset = |name: &str| absent(name);
    let mut session = Session::default();
    let shaded = session
        .shade_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    // A GPU host evaluates the zones in its shader: no field is baked on the CPU for them, the
    // shader names the zones it reads, and each receiver comes with its zones packed for it.
    let material = &shaded.metadata["materials"]["zoned"];
    assert!(material.get("plan").is_none(), "{material}");
    assert_eq!(material["shader"]["zones"], json!(["runoff"]));
    let mesh = "wall/part/wall/wall/front";
    let packed = &shaded.metadata["shading"][mesh]["zones"];
    assert!(packed["count"].as_u64().unwrap() > 0, "{packed}");
    let baked = session
        .bake_project_fields(
            &project,
            mesh,
            &["zone".into(), "extent".into()],
            1,
            &asset,
            &|| false,
        )
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let (zone, extent) = (&baked.fields[0], &baked.fields[1]);
    let texels = baked.width * baked.height;
    assert_eq!(zone.len(), texels * 4);
    let (mut inside, mut outside) = (0, 0);
    for texel in 0..texels {
        let [u, v, id] = [zone[texel * 4], zone[texel * 4 + 1], zone[texel * 4 + 2]];
        let [is, limit, scale] = [
            extent[texel * 4],
            extent[texel * 4 + 1],
            extent[texel * 4 + 2],
        ];
        if is > 0.5 {
            inside += 1;
            // u0 (5) along the 4 m wall, never above the edge, never past its column's end.
            assert!((4.99..=9.01).contains(&u), "u {u}");
            assert!(
                v >= 0. && v <= limit + 1e-4 && limit <= 2.5 + 1e-6,
                "v {v} limit {limit}"
            );
            assert!((0. ..1.).contains(&id) && scale == 1.);
        } else if [u, v, id] == [0., -1., 0.] {
            outside += 1;
        } else {
            // A texel no chart covers holds nothing.
            assert_eq!([u, v, id, limit], [0.; 4]);
        }
    }
    // The wall's lower half (below 0.5 m and below the openings' heads) is outside.
    assert!(
        inside > texels / 4 && outside > texels / 10,
        "{inside} in, {outside} out"
    );
    // The CPU bake of the material agrees: it builds, and its zone changes what it bakes.
    let built = session
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let mut moved = project.clone();
    moved.models[0].parameters.insert("u0".into(), json!(6));
    let other = Session::default()
        .build_project(&moved, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let looks = |output: &superspec_engine::authoring::ProjectOutput| {
        serde_json::to_string(&output.scene(1).unwrap().manifest.materials).unwrap()
    };
    assert_ne!(
        looks(&built),
        looks(&other),
        "a zone's u0 is part of what is baked"
    );
}
