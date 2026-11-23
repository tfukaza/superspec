//! The Kowloon building's graphics detail: level 0 is the authored building, levels 1 and 2
//! draw the same placements with less geometry for the Low and Very Low presets.
use serde_json::json;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};
use superspec_engine::{
    itemspec::definition::{self as item, Environment, GeneratedItem, Values},
    worldspec::themes::kowloon::{
        fasteners,
        fixture::Fixture,
        graphics_detail::GraphicsDetail,
        items,
        mesh::{Mesh, Mode},
        pen::Pen,
        pipes::{self, PipeSystem},
    },
};
use superspec_model::visual::VisualInput;

fn library() -> BTreeMap<String, String> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/worldspec/themes/kowloon/items");
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".item.json"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

fn generate(name: &str, values: &Values) -> GeneratedItem {
    let files = library();
    let reader = |name: &str| -> Result<Arc<[u8]>, String> {
        files
            .get(name)
            .map(|text| Arc::from(text.as_bytes()))
            .ok_or(format!("Missing {name}"))
    };
    let definition = item::parse(name, &files[name]).unwrap();
    let mut environment = Environment::new(&reader).with_natives(items::natives());
    item::prepare_in(&definition, values, &mut environment, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .build(&|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The city's building families, as `city.rs` hands them to the building item.
fn family(index: usize) -> Values {
    let families: Vec<Values> = serde_json::from_str(include_str!(
        "../src/worldspec/themes/kowloon/materials/building-families.json"
    ))
    .unwrap();
    let mut values = families[index].clone();
    values.insert("floors".into(), json!(4));
    values.insert("seed".into(), json!(4 + index));
    values
}

fn triangles(item: &GeneratedItem) -> usize {
    item.geometry
        .meshes
        .iter()
        .map(|m| m.indices.len() / 3)
        .sum()
}

/// Every delivered vertex stream, index, slot, node and feature of the item.
fn digest(item: &GeneratedItem) -> u64 {
    #[allow(deprecated)]
    let mut hasher = std::hash::SipHasher::new();
    let geometry = &item.geometry;
    for mesh in &geometry.meshes {
        mesh.id.hash(&mut hasher);
        mesh.material_slot.hash(&mut hasher);
        mesh.indices.hash(&mut hasher);
        for values in [&mesh.positions, &mesh.normals] {
            for v in values.iter().flatten() {
                v.to_bits().hash(&mut hasher);
            }
        }
        for v in mesh.uvs.iter().flatten() {
            v.to_bits().hash(&mut hasher);
        }
        for v in mesh.colors.iter().chain(&mesh.tangents).flatten() {
            v.to_bits().hash(&mut hasher);
        }
        for (set, uvs) in &mesh.additional_uvs {
            set.hash(&mut hasher);
            for v in uvs.iter().flatten() {
                v.to_bits().hash(&mut hasher);
            }
        }
    }
    for node in &geometry.nodes {
        format!(
            "{} {:?} {:?} {:?} {:?}",
            node.id, node.name, node.parent, node.transform, node.meshes
        )
        .hash(&mut hasher);
    }
    for part in &geometry.parts {
        format!("{} {} {:?}", part.id, part.node, part.meshes).hash(&mut hasher);
    }
    format!("{:?} {:?}", geometry.features, geometry.bounds).hash(&mut hasher);
    hasher.finish()
}

fn at_level(index: usize, level: u8) -> GeneratedItem {
    let mut values = family(index);
    values.insert("graphicsDetail".into(), json!(level));
    generate("kowloon-building.item.json", &values)
}

/// Level 0 is the building as authored: asking for it is the same as stating no level, and
/// the same values always build the same geometry.
///
/// This test used to pin each family's triangle count and geometry digest, recorded before
/// the graphics detail existed. The variant catalogue (`variants.rs`, step 6 of
/// `docs/plans/runtime-efficiency.md`) changes the authored output by design: each part type
/// draws from a few shared shapes instead of one per instance. The pins are therefore gone;
/// `kowloon_variants.rs` checks what the catalogue must keep, and `variantBudget` 0 still
/// builds every instance from its own stream. Run with `--nocapture` to print the counts
/// and digests, including those of the building without a catalogue.
#[test]
fn level_zero_is_the_authored_building() {
    for index in 0..4 {
        let unset = generate("kowloon-building.item.json", &family(index));
        let again = generate("kowloon-building.item.json", &family(index));
        let zero = at_level(index, 0);
        let mut unlimited = family(index);
        unlimited.insert("variantBudget".into(), json!(0));
        let unlimited = generate("kowloon-building.item.json", &unlimited);
        println!(
            "family {index}: {} triangles, {} meshes, {} nodes, digest {:016x}; without a catalogue {} triangles, digest {:016x}",
            triangles(&unset),
            unset.geometry.meshes.len(),
            unset.geometry.nodes.len(),
            digest(&unset),
            triangles(&unlimited),
            digest(&unlimited)
        );
        assert!(triangles(&unset) > 0, "family {index}");
        assert_eq!(digest(&unset), digest(&again), "family {index}");
        assert_eq!(
            (triangles(&zero), digest(&zero)),
            (triangles(&unset), digest(&unset)),
            "family {index}"
        );
    }
}
#[test]
fn each_level_draws_less_of_the_same_building() {
    for index in 0..4 {
        let levels = [0, 1, 2].map(|level| at_level(index, level));
        let counts = levels.each_ref().map(triangles);
        println!("family {index}: triangles by level {counts:?}");
        assert!(
            counts[0] > counts[1] && counts[1] > counts[2] && counts[2] > 0,
            "family {index}: {counts:?}"
        );
        let full = &levels[0].geometry;
        for (level, reduced) in levels.iter().enumerate().skip(1) {
            let reduced = &reduced.geometry;
            // The same parts stand in the same places and report the same features.
            let placed = |geometry: &item::ItemGeometry| {
                geometry
                    .nodes
                    .iter()
                    .map(|node| {
                        format!(
                            "{} {:?} {:?} {:?}",
                            node.id, node.name, node.parent, node.transform
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                placed(full),
                placed(reduced),
                "family {index} level {level}"
            );
            let parts = |geometry: &item::ItemGeometry| {
                geometry
                    .parts
                    .iter()
                    .map(|part| (part.id.clone(), part.node))
                    .collect::<Vec<_>>()
            };
            assert_eq!(parts(full), parts(reduced), "family {index} level {level}");
            assert_eq!(
                full.features.len(),
                reduced.features.len(),
                "family {index} level {level}"
            );
            for (authored, drawn) in full.features.iter().zip(&reduced.features) {
                assert_eq!(authored, drawn, "family {index} level {level}");
            }
            // Omitted fastenings and widened hairlines move the bounds by centimetres at most.
            let bounds = |geometry: &item::ItemGeometry| {
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                let transforms = geometry.world_transforms().unwrap();
                for (node, matrix) in geometry.nodes.iter().zip(transforms) {
                    for mesh in &node.meshes {
                        for p in &geometry.meshes[*mesh].positions {
                            let p = matrix.point(*p);
                            for axis in 0..3 {
                                low[axis] = low[axis].min(p[axis]);
                                high[axis] = high[axis].max(p[axis]);
                            }
                        }
                    }
                }
                [low, high]
            };
            // Level 2 draws no aerials, so its top may be the roof kit's instead of a mast's:
            // lower by a mast at most, never higher.
            let (a, b) = (bounds(full), bounds(reduced));
            for corner in 0..2 {
                for axis in 0..3 {
                    let moved = b[corner][axis] - a[corner][axis];
                    let mast = if level == 2 && corner == 1 && axis == 2 {
                        3.
                    } else {
                        0.08
                    };
                    assert!(
                        moved < 0.08 && moved > -mast,
                        "family {index} level {level}: bounds {a:?} became {b:?}"
                    );
                }
            }
            assert_eq!(
                serde_json::to_string(&levels[0].collision).unwrap(),
                serde_json::to_string(&levels[level].collision).unwrap(),
                "family {index} level {level}"
            );
        }
    }
}

/// Where each level's triangles are, by part (indices dropped) and by material slot: run with
/// `--ignored --nocapture` to see what a reduced level still spends.
#[test]
#[ignore = "a report, not a check"]
fn report_triangles_by_part() {
    for index in 0..4 {
        let levels = [0, 1, 2].map(|level| at_level(index, level));
        let mut by_part: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        let mut by_slot: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        for (level, built) in levels.iter().enumerate() {
            for part in &built.geometry.parts {
                let kind = part
                    .id
                    .split('/')
                    .map(|step| step.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-'))
                    .collect::<Vec<_>>()
                    .join("/");
                for mesh in &part.meshes {
                    let mesh = &built.geometry.meshes[*mesh];
                    by_part.entry(kind.clone()).or_default()[level] += mesh.indices.len() / 3;
                    by_slot.entry(mesh.material_slot.clone()).or_default()[level] +=
                        mesh.indices.len() / 3;
                }
            }
        }
        for (title, table) in [("part", by_part), ("slot", by_slot)] {
            let mut rows: Vec<_> = table.into_iter().collect();
            rows.sort_by_key(|(_, counts)| std::cmp::Reverse(counts[2]));
            println!("family {index}, triangles by {title} at levels 0/1/2:");
            for (name, counts) in rows.iter().take(28) {
                println!(
                    "  {:>7} {:>7} {:>7}  {name}",
                    counts[0], counts[1], counts[2]
                );
            }
        }
    }
}

/// Where each level's triangles are by the fixture that drew them: the last named steps of
/// each mesh's path (indices dropped) and its slot. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a report, not a check"]
fn report_triangles_by_fixture() {
    for index in 0..4 {
        let levels = [0, 1, 2].map(|level| at_level(index, level));
        let mut by_fixture: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        for (level, built) in levels.iter().enumerate() {
            for mesh in &built.geometry.meshes {
                let steps: Vec<_> = mesh
                    .id
                    .split('/')
                    .map(|step| step.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-'))
                    .filter(|step| !step.is_empty())
                    .collect();
                let kind = steps[steps.len().saturating_sub(3)..].join("/");
                by_fixture.entry(kind).or_default()[level] += mesh.indices.len() / 3;
            }
        }
        let mut rows: Vec<_> = by_fixture.into_iter().collect();
        rows.sort_by_key(|(_, counts)| std::cmp::Reverse(counts[2]));
        println!("family {index}, triangles by fixture at levels 0/1/2:");
        for (name, counts) in rows.iter().take(60) {
            println!(
                "  {:>7} {:>7} {:>7}  {name}",
                counts[0], counts[1], counts[2]
            );
        }
    }
}

/// The widest extent of a drawing across the axis its member runs along (X here).
fn section(mesh: &Mesh<'_>) -> f64 {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for vertex in VisualInput::vertices(mesh) {
        for (k, axis) in [1, 2].into_iter().enumerate() {
            low[k] = low[k].min(vertex.position[axis]);
            high[k] = high[k].max(vertex.position[axis]);
        }
    }
    (high[0] - low[0]).max(high[1] - low[1])
}

#[test]
fn a_reduced_level_draws_no_hairlines() {
    for (level, detail) in [
        GraphicsDetail::Full,
        GraphicsDetail::Low,
        GraphicsDetail::VeryLow,
    ]
    .into_iter()
    .enumerate()
    {
        let drawn = |paint: &dyn Fn(&mut Fixture<'_, '_>)| {
            let mut mesh = Mesh::new(Mode::Render, 10_000, &|| false).unwrap();
            mesh.set_graphics_detail(detail);
            paint(&mut Fixture {
                pen: Pen::new(Default::default(), "member").unwrap(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            });
            (mesh.triangle_count(), section(&mesh))
        };
        // A 12 mm glazing bead, a 5 mm rod, a 4 mm wire and a 12 mm bore pipe, one metre long.
        let bead = drawn(&|d| {
            d.frame_box([0., 0., 0.], [1., 0.012, 0.008], [0.5; 3])
                .unwrap()
        });
        let rod = drawn(&|d| {
            d.bar("paint", [0.; 3], [1., 0., 0.], 0.005, [0.5; 3])
                .unwrap()
        });
        let wire = drawn(&|d| {
            d.tube("metal", &[[0.; 3], [1., 0., 0.]], 0.002, [0.5; 3], 5)
                .unwrap()
        });
        let pipe = drawn(&|d| {
            PipeSystem::new(pipes::Options {
                radius: 0.006,
                color: [0.5; 3],
                sides: 16,
                variant: "test".into(),
                weathering: None,
            })
            .unwrap()
            .straight(d, [0.; 3], [1., 0., 0.], &[])
            .unwrap()
        });
        // A sound member keeps its size at every level.
        let post = drawn(&|d| {
            d.frame_box([0., 0., 0.], [1., 0.045, 0.07], [0.5; 3])
                .unwrap()
        });
        let bolt = drawn(&|d| {
            fasteners::bolt(d, "metal", [0.; 3], [0., 1., 0.], 0.004, [0.5; 3], None).unwrap()
        });
        println!(
            "level {level}: bead {bead:?} rod {rod:?} wire {wire:?} pipe {pipe:?} post {post:?} bolt {bolt:?}"
        );
        assert!((post.1 - 0.07).abs() < 1e-9, "level {level}: {post:?}");
        if level == 0 {
            assert_eq!((bead.0, rod.0, wire.0, pipe.0, bolt.0), (28, 8, 10, 32, 62));
            assert!(bead.1 < 0.0121 && rod.1 < 0.0051 && wire.1 < 0.0041 && pipe.1 < 0.0121);
        } else {
            assert_eq!(bolt.0, 0, "level {level}");
            for (name, (count, width)) in
                [("bead", bead), ("rod", rod), ("wire", wire), ("pipe", pipe)]
            {
                // Four-sided rounds measure their flats: 25 mm across the corners.
                assert!(
                    width >= 0.025 * std::f64::consts::FRAC_1_SQRT_2 - 1e-9,
                    "level {level}: {name} is {width} m wide"
                );
                assert!(count <= 12, "level {level}: {name} draws {count} triangles");
            }
        }
    }
}

#[test]
fn a_city_building_takes_the_level_from_its_aml_and_keeps_its_physical_output() {
    let compiled = |level: Option<u8>| {
        let attribute = level.map_or(String::new(), |l| format!(r#" graphics-detail="{l}""#));
        let aml = format!(
            r#"<architecture version="1" id="lot"><city>
              <building id="block" position="0 0 0" floors="4" seed="5"{attribute}/>
            </city></architecture>"#
        );
        let asset = |id: &str| {
            let name = superspec_catalog::image_name(id).expect("shipped stock texture");
            std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/materials")
                    .join(name),
            )
            .map_err(|e| e.to_string())
        };
        (
            superspec_engine::compile_with_assets(&aml, 1, &asset).unwrap(),
            superspec_engine::compile_collision_with_assets(&aml, 1, &asset).unwrap(),
        )
    };
    let (authored, authored_collision) = compiled(None);
    let mut previous = usize::MAX;
    for level in 0..=2 {
        let (scene, collision) = compiled(Some(level));
        let indices = scene.manifest.index_count;
        println!("city building at level {level}: {} triangles", indices / 3);
        if level == 0 {
            assert_eq!(indices, authored.manifest.index_count);
            assert_eq!(scene.indices, authored.indices);
            assert_eq!(scene.positions, authored.positions);
        }
        assert!(indices < previous || level == 0, "level {level}");
        previous = indices;
        assert_eq!(scene.manifest.colliders, authored.manifest.colliders);
        assert_eq!(
            collision.manifest.colliders,
            authored_collision.manifest.colliders
        );
        assert_eq!(collision.indices, authored_collision.indices);
        assert_eq!(collision.positions, authored_collision.positions);
    }
}

fn stock_asset(id: &str) -> Result<Vec<u8>, String> {
    let name = superspec_catalog::image_name(id).ok_or(format!("Unknown stock texture {id}"))?;
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map_err(|e| e.to_string())
}

/// Each visible mesh of a compiled scene: its id and index count, in delivery order.
fn delivered(scene: &superspec_engine::Scene) -> Vec<(String, usize)> {
    scene
        .manifest
        .meshes
        .iter()
        .filter(|mesh| mesh.visible)
        .map(|mesh| (mesh.id.clone(), mesh.index_count))
        .collect()
}

/// The publication exports a prototype with the stateless compiler and bakes it on the
/// Studio session's compile of the same AML: both must draw the level the AML asks for.
/// The session is reused across levels, as a bake host reuses it across prototypes.
#[test]
fn the_export_and_the_session_draw_the_same_building_at_every_level() {
    let session = superspec_engine::Session::default();
    let mut revision = 0;
    // The published prototype, a second family and a building with shops and pavement
    // switched by attribute: balconies, trees, shop rooms and roof kits between them.
    for building in [
        r#"floors="5" seed="8""#,
        r#"floors="4" seed="5""#,
        r#"floors="4" seed="6" ground-shops="true" pavement="true""#,
    ] {
        let mut exported_levels = Vec::new();
        for level in [None, Some(0), Some(1), Some(2), Some(1), Some(0)] {
            let attribute = level.map_or(String::new(), |l| format!(r#" graphics-detail="{l}""#));
            let aml = format!(
                r#"<architecture version="1" id="building-0"><city><building id="prototype" position="0 0 0" {building}{attribute} /></city></architecture>"#
            );
            revision += 1;
            let exported =
                superspec_engine::compile_with_assets(&aml, revision, &stock_asset).unwrap();
            let baked = session
                .compile_cancellable(&aml, revision, &stock_asset, true, &|| false)
                .unwrap();
            let (exported_meshes, baked_meshes) = (delivered(&exported), delivered(&baked));
            println!(
                "{building} level {level:?}: {} meshes, {} indices exported, {} in the session",
                exported_meshes.len(),
                exported.manifest.index_count,
                baked.manifest.index_count
            );
            for (a, b) in exported_meshes.iter().zip(&baked_meshes) {
                assert_eq!(a, b, "{building} level {level:?}");
            }
            assert_eq!(
                exported_meshes.len(),
                baked_meshes.len(),
                "{building} level {level:?}"
            );
            assert_eq!(
                exported.indices, baked.indices,
                "{building} level {level:?}"
            );
            assert_eq!(
                exported.positions, baked.positions,
                "{building} level {level:?}"
            );
            exported_levels.push((level, exported.manifest.index_count));
        }
        // The level is honoured, not ignored by both paths alike.
        let count = |level| {
            exported_levels
                .iter()
                .find(|(l, _)| *l == level)
                .map(|(_, count)| *count)
                .unwrap()
        };
        assert_eq!(count(None), count(Some(0)), "{building}");
        assert!(
            count(Some(0)) > count(Some(1)) && count(Some(1)) > count(Some(2)),
            "{building}"
        );
    }
}

fn native(name: &str, parameters: serde_json::Value, level: Option<u8>) -> usize {
    let mut values: Values = serde_json::from_value(parameters).unwrap();
    if let Some(level) = level {
        values.insert("graphicsDetail".into(), json!(level));
    }
    items::natives()
        .generate(name, &values, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .meshes
        .iter()
        .map(|mesh| mesh.indices.len() / 3)
        .sum()
}

#[test]
fn every_native_takes_the_level_and_the_central_fixtures_use_it() {
    let natives = items::natives();
    for name in natives.names() {
        // A generator that ignores the level still accepts it, and level 0 is its own output.
        let Ok(plain) = natives.generate(&name, &Values::new(), &|| false) else {
            continue; // Needs parameters of its own.
        };
        let zero = natives
            .generate(
                &name,
                &[("graphicsDetail".to_owned(), json!(0))].into(),
                &|| false,
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(plain.meshes, zero.meshes, "{name}");
        for level in [1, 2] {
            natives
                .generate(
                    &name,
                    &[("graphicsDetail".to_owned(), json!(level))].into(),
                    &|| false,
                )
                .unwrap_or_else(|error| panic!("{name} at level {level}: {error}"));
        }
        assert!(
            natives
                .generate(
                    &name,
                    &[("graphicsDetail".to_owned(), json!(3))].into(),
                    &|| false
                )
                .is_err(),
            "{name}"
        );
    }
    for (name, parameters) in [
        ("kowloon/window", json!({"width": 1.85, "height": 1.75})),
        ("kowloon/window", json!({"kind": "slider"})),
        ("kowloon/window", json!({"kind": "timber"})),
        ("kowloon/window", json!({"lit": true, "room": 2.6})),
        ("kowloon/aircon", json!({"plates": true})),
        ("kowloon/condenser", json!({"plates": true})),
        ("kowloon/downpipe", json!({"height": 12.})),
    ] {
        let counts = [None, Some(1), Some(2)].map(|level| native(name, parameters.clone(), level));
        println!("{name} {parameters}: triangles by level {counts:?}");
        assert!(
            counts[0] > counts[1] && counts[1] > counts[2] && counts[2] > 0,
            "{name} {parameters}: {counts:?}"
        );
    }
}

/// One native generator's meshes at a level (`None`: the level left out).
fn native_meshes(
    name: &str,
    parameters: &serde_json::Value,
    level: Option<u8>,
) -> Vec<item::ItemMesh> {
    let mut values: Values = serde_json::from_value(parameters.clone()).unwrap();
    if let Some(level) = level {
        values.insert("graphicsDetail".into(), json!(level));
    }
    items::natives()
        .generate(name, &values, &|| false)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .meshes
}

/// The building-mounted props: what each still draws at levels 1 and 2. The ceilings are
/// the reductions these fixtures were given; a fixture that grows past one has lost its.
#[test]
fn props_keep_their_massing_and_shed_their_hardware() {
    for (name, parameters, low, very_low) in [
        ("kowloon/aircon", json!({"plates": true}), 120, 12),
        ("kowloon/condenser", json!({"plates": true}), 240, 20),
        (
            "kowloon/downpipe",
            json!({"height": 12., "clips": 1.2}),
            340,
            8,
        ),
        ("kowloon/cage", json!({}), 420, 200),
        ("kowloon/plants", json!({}), 1200, 520),
        ("kowloon/water-tank", json!({}), 1000, 260),
        ("kowloon/water-tank", json!({"kind": "box"}), 400, 80),
        ("kowloon/wall-vent", json!({}), 40, 12),
        ("kowloon/roof-outlet", json!({}), 44, 24),
        (
            "kowloon/riser-duct",
            json!({"height": 10.4, "standoff": 0.06}),
            400,
            120,
        ),
        ("kowloon/junction-box", json!({}), 40, 10),
        ("kowloon/wall-lamp", json!({"style": "round"}), 280, 80),
        ("kowloon/wall-lamp", json!({}), 40, 30),
        ("kowloon/drying-rack", json!({}), 800, 800),
        (
            "kowloon/tile-deck",
            json!({"width": 7.5, "depth": 7.5}),
            12,
            2,
        ),
        ("kowloon/rail", json!({}), 60, 60),
        ("kowloon/cornice", json!({}), 40, 40),
    ] {
        let counts = [None, Some(1), Some(2)].map(|level| {
            native_meshes(name, &parameters, level)
                .iter()
                .map(|mesh| mesh.indices.len() / 3)
                .sum::<usize>()
        });
        println!("native {name} {parameters}: triangles by level {counts:?}");
        // A fixture of a few plain blocks (a roof outlet) is already its level 1.
        assert!(
            counts[0] >= counts[1]
                && counts[1] >= counts[2]
                && counts[0] > counts[2]
                && counts[2] > 0,
            "{name} {parameters}: {counts:?}"
        );
        assert!(
            counts[1] <= low && counts[2] <= very_low,
            "{name} {parameters}: {counts:?} over {low} and {very_low}"
        );
    }
}

/// A reduced roof deck is one face whatever its size: one chart for the bake, and no seam
/// inside the roof for a distance level to keep. Level 0 lays a face per tile.
#[test]
fn a_reduced_tile_deck_is_one_face_over_the_whole_deck() {
    for (width, depth) in [(7.5, 7.5), (13.2, 6.4), (0.5, 0.5), (0.5, 2.)] {
        let parameters = json!({"width": width, "depth": depth, "seed": 3});
        // The triangles that face up, as [x, y] corners.
        let tops = |level: Option<u8>| -> Vec<[[f64; 2]; 3]> {
            native_meshes("kowloon/tile-deck", &parameters, level)
                .iter()
                .flat_map(|mesh| {
                    mesh.indices.chunks_exact(3).filter_map(|triangle| {
                        let corner = |k: usize| triangle[k] as usize;
                        (0..3).all(|k| mesh.normals[corner(k)][2] > 0.99).then(|| {
                            [0, 1, 2].map(|k| {
                                let p = mesh.positions[corner(k)];
                                [p[0], p[1]]
                            })
                        })
                    })
                })
                .collect()
        };
        let area = |triangles: &[[[f64; 2]; 3]]| -> f64 {
            triangles
                .iter()
                .map(|[a, b, c]| {
                    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.
                })
                .sum()
        };
        let authored = tops(None);
        assert!(authored.len() >= 2, "{width} x {depth}");
        for level in [1, 2] {
            let reduced = tops(Some(level));
            println!(
                "tile deck {width} x {depth} level {level}: {} faces up, {} authored",
                reduced.len() / 2,
                authored.len() / 2
            );
            assert_eq!(reduced.len(), 2, "{width} x {depth} level {level}");
            assert!(
                (area(&reduced) - width * depth).abs() < 1e-6,
                "{width} x {depth} level {level}: the face covers {} m2",
                area(&reduced)
            );
            // The two triangles share their diagonal's vertices: one welded quad.
            let mut corners: Vec<[u64; 2]> = reduced
                .iter()
                .flatten()
                .map(|p| p.map(f64::to_bits))
                .collect();
            corners.sort_unstable();
            corners.dedup();
            assert_eq!(corners.len(), 4, "{width} x {depth} level {level}");
            assert_eq!(
                native_meshes("kowloon/tile-deck", &parameters, Some(level)),
                native_meshes("kowloon/tile-deck", &parameters, Some(level)),
            );
        }
    }
}

/// Wires, conduits, meters, window bars, aerials and ladders are lines from across a
/// street: level 1 still draws them and level 2 draws none.
#[test]
fn level_two_draws_no_thin_decoratives() {
    for (name, parameters) in [
        ("kowloon/cable", json!({"count": 3, "boxes": "both"})),
        ("kowloon/conduit", json!({})),
        ("kowloon/meter-bank", json!({})),
        ("kowloon/security-bars", json!({})),
        ("kowloon/aerial", json!({})),
        ("kowloon/ladder", json!({"grab": 0.6})),
    ] {
        let counts = [None, Some(1), Some(2)].map(|level| {
            native_meshes(name, &parameters, level)
                .iter()
                .map(|mesh| mesh.indices.len() / 3)
                .sum::<usize>()
        });
        println!("native {name} {parameters}: triangles by level {counts:?}");
        // Window bars are plain square bars already: level 1 draws them as authored.
        assert!(
            counts[0] >= counts[1] && counts[1] > 0 && counts[2] == 0,
            "{name} {parameters}: {counts:?}"
        );
    }
}

/// Every vertex's distance in front of the wall plane (item -Y), in millimetres, sorted.
fn depths(meshes: &[item::ItemMesh]) -> Vec<i64> {
    let mut depths: Vec<i64> = meshes
        .iter()
        .flat_map(|mesh| mesh.positions.iter())
        .map(|p| (-p[1] * 1000.).round() as i64)
        .collect();
    depths.sort_unstable();
    depths.dedup();
    depths
}

#[test]
fn a_reduced_prop_draws_no_thin_plates() {
    // A pipe clip's ears are 13 mm plate: level 1 draws one plate of 25 mm behind the pipe.
    let clipped = json!({"height": 3., "clips": 1.2});
    let full = depths(&native_meshes("kowloon/downpipe", &clipped, None));
    let low = depths(&native_meshes("kowloon/downpipe", &clipped, Some(1)));
    assert!(full.contains(&13), "{full:?}");
    assert!(low.contains(&25) && !low.contains(&13), "{low:?}");
    // A junction box's 12 mm lid stays at level 1 and goes at level 2; the body stays.
    let boxed = depths(&native_meshes(
        "kowloon/junction-box",
        &json!({"stubs": 0}),
        Some(2),
    ));
    assert_eq!(boxed, [0, 75]);
    // Wall plates under an air conditioner's arms, 8 to 12 mm thick, go at level 1.
    for name in ["kowloon/aircon", "kowloon/condenser"] {
        let plated = json!({"plates": true});
        let slots = |level| -> Vec<(String, usize)> {
            native_meshes(name, &plated, level)
                .iter()
                .map(|mesh| (mesh.material_slot.clone(), mesh.indices.len() / 3))
                .collect()
        };
        println!("{name}: {:?} {:?}", slots(Some(1)), slots(Some(2)));
        // Level 2 is the case in its atlas, and a fan's dark disc: no steel.
        assert!(
            slots(Some(2))
                .iter()
                .all(|(slot, _)| slot == "aircon" || slot == "paint" || slot == "sheet"),
            "{name}"
        );
    }
}

#[test]
fn a_prop_reports_the_same_features_and_tints_at_every_level() {
    let natives = items::natives();
    for (name, parameters) in [
        (
            "kowloon/riser-duct",
            json!({"height": 10.4, "standoff": 0.06}),
        ),
        ("kowloon/riser-duct", json!({})),
    ] {
        let features = |level: u8| {
            let mut values: Values = serde_json::from_value(parameters.clone()).unwrap();
            values.insert("graphicsDetail".into(), json!(level));
            natives.features(name, &values)
        };
        assert!(!features(0).is_empty(), "{name}");
        assert_eq!(features(0), features(1), "{name}");
        assert_eq!(features(0), features(2), "{name}");
    }
    // A reduced level keeps each slot it still draws in, and its colours: a seeded fixture
    // draws the same stream, so the case, the cage and the tiles keep their tints.
    for (name, parameters) in [
        ("kowloon/aircon", json!({"seed": 7})),
        ("kowloon/condenser", json!({"seed": 7})),
        ("kowloon/cage", json!({"seed": 7})),
        ("kowloon/tile-deck", json!({"seed": 7})),
        ("kowloon/water-tank", json!({"seed": 7})),
    ] {
        // Under one variant budget: a level that states none has its preset's own, and so
        // may pick another catalogue entry, with that entry's contents and their colours
        // (`kowloon_variants.rs` checks that a fixture's own paint survives that).
        let mut parameters = parameters.clone();
        parameters["variantBudget"] = json!(4);
        let palette = |level: Option<u8>| {
            let mut tints: BTreeMap<String, std::collections::BTreeSet<[u32; 3]>> = BTreeMap::new();
            for mesh in native_meshes(name, &parameters, level) {
                let slot = tints.entry(mesh.material_slot.clone()).or_default();
                for color in &mesh.colors {
                    slot.insert([0, 1, 2].map(|k| (color[k] * 255.).round() as u32));
                }
            }
            tints
        };
        let full = palette(None);
        for level in [1, 2] {
            for (slot, tints) in palette(Some(level)) {
                let authored = full
                    .get(&slot)
                    .unwrap_or_else(|| panic!("{name} level {level} draws in a new slot {slot}"));
                // A reduced tile deck is one face in the mean of its tiles' colours, so each
                // of its tints lies among the authored ones, channel by channel; every other
                // tint is one the authored fixture draws in that slot.
                if name == "kowloon/tile-deck" {
                    for tint in &tints {
                        for k in 0..3 {
                            let channel = || authored.iter().map(|authored| authored[k]);
                            assert!(
                                (channel().min().unwrap()..=channel().max().unwrap())
                                    .contains(&tint[k]),
                                "{name} level {level}: {slot} is tinted {tint:?}"
                            );
                        }
                    }
                    continue;
                }
                // Washing in a cage is drawn from the cloth atlas at level 0 and as plain
                // cotton in each garment's dye at a reduced level (`laundry.rs`).
                if slot == "balcony-cloth" {
                    continue;
                }
                for tint in &tints {
                    assert!(
                        authored.contains(tint),
                        "{name} level {level}: {slot} is tinted {tint:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn signs_keep_their_faces_and_lettering() {
    for (name, parameters) in [
        ("kowloon/board-sign", json!({})),
        ("kowloon/board-sign", json!({"horizontal": true})),
        ("kowloon/sign", json!({"text": "TEA"})),
        ("kowloon/blade-sign", json!({})),
        ("kowloon/hung-sign", json!({})),
    ] {
        // Everything but the steel that carries a sign: its board, artwork and lettering.
        let faces = |level: Option<u8>| -> Vec<item::ItemMesh> {
            native_meshes(name, &parameters, level)
                .into_iter()
                .filter(|mesh| !["metal", "rust", "paint"].contains(&mesh.material_slot.as_str()))
                .collect()
        };
        let authored = faces(None);
        println!(
            "{name} {parameters}: face slots {:?}",
            authored
                .iter()
                .map(|mesh| (mesh.material_slot.as_str(), mesh.indices.len() / 3))
                .collect::<Vec<_>>()
        );
        assert!(!authored.is_empty(), "{name}");
        for level in [1, 2] {
            assert_eq!(authored, faces(Some(level)), "{name} level {level}");
        }
    }
}

/// A duct bracket's strap is 11 mm flat bar. A reduced level draws it 21 mm thick, thickened
/// towards the duct and the wall so the bracket fills the envelope it was authored with, and
/// reports the features and attachments of the 11 mm strap.
#[test]
fn a_reduced_duct_bracket_draws_a_thicker_strap_in_the_authored_envelope() {
    for feet in [true, false] {
        let levels = [0, 1, 2].map(|level| {
            generate(
                "kowloon-duct-bracket.item.json",
                &[
                    ("graphicsDetail".to_owned(), json!(level)),
                    ("feet".to_owned(), json!(feet)),
                ]
                .into(),
            )
        });
        // The strap leaves the wall plate along X: the cap at its start shows its section,
        // thickness along Y and the 62 mm flat along Z.
        let cap = |built: &GeneratedItem| {
            let geometry = &built.geometry;
            let part = geometry
                .parts
                .iter()
                .find(|part| part.id == "strap" || part.id == "hanger")
                .unwrap();
            let points: Vec<[f64; 3]> = part
                .meshes
                .iter()
                .flat_map(|mesh| &geometry.meshes[*mesh].positions)
                .copied()
                .collect();
            let start = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            let extent = |axis: usize| {
                let on_cap = || {
                    points
                        .iter()
                        .filter(|p| p[0] - start < 1e-9)
                        .map(|p| p[axis])
                };
                on_cap().fold(f64::NEG_INFINITY, f64::max) - on_cap().fold(f64::INFINITY, f64::min)
            };
            [extent(1), extent(2)]
        };
        let authored = cap(&levels[0]);
        assert!(
            (authored[0] - 0.011).abs() < 1e-12 && (authored[1] - 0.062).abs() < 1e-12,
            "feet {feet}: {authored:?}"
        );
        for level in [1, 2] {
            let drawn = cap(&levels[level]);
            println!("bracket feet {feet} level {level}: strap section {drawn:?}");
            assert!(
                drawn[0] >= 0.02 && (drawn[1] - 0.062).abs() < 1e-12,
                "feet {feet} level {level}: {drawn:?}"
            );
            assert_eq!(
                levels[0].geometry.features, levels[level].geometry.features,
                "feet {feet} level {level}"
            );
            assert_eq!(
                serde_json::to_string(&levels[0].attachments).unwrap(),
                serde_json::to_string(&levels[level].attachments).unwrap(),
                "feet {feet} level {level}"
            );
            assert_eq!(
                levels[0].geometry.bounds, levels[level].geometry.bounds,
                "feet {feet} level {level}"
            );
        }
    }
}

/// A timber window's boards wear pale along their edges. Level 0 draws that in short strips
/// along each edge; level 1 keeps one whole strip per worn edge, so the pale edge still
/// reads; level 2 draws each board face whole in its own tint.
#[test]
fn a_timber_window_keeps_its_pale_worn_edges_at_level_one() {
    let parameters = json!({"kind": "timber"});
    let timber = |level: Option<u8>| -> (usize, f64, f64) {
        let meshes: Vec<_> = native_meshes("kowloon/window", &parameters, level)
            .into_iter()
            .filter(|mesh| mesh.material_slot == "shop-timber")
            .collect();
        assert!(!meshes.is_empty(), "level {level:?}");
        let tints = || {
            meshes
                .iter()
                .flat_map(|mesh| &mesh.colors)
                .map(|color| color[0] as f64)
        };
        (
            meshes.iter().map(|mesh| mesh.indices.len() / 3).sum(),
            tints().fold(f64::INFINITY, f64::min),
            tints().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let [full, low, very_low] = [None, Some(1), Some(2)].map(timber);
    println!("timber window (triangles, darkest, palest): {full:?} {low:?} {very_low:?}");
    // Level 2 has the boards' own tints and no wear.
    assert!(full.2 > very_low.2 + 0.02, "{full:?} {very_low:?}");
    // Level 1 is paler at its worn edges than any plain board, by at least a third of the
    // palest authored wear (a whole strip lays the wear's mean cover, half its full cover).
    assert!(
        low.2 - very_low.2 > (full.2 - very_low.2) / 3.,
        "{full:?} {low:?} {very_low:?}"
    );
    assert!(low.2 <= full.2 + 1e-6, "{full:?} {low:?}");
    // The strips only lighten: level 1's darkest timber is level 2's plain board.
    assert!((low.1 - very_low.1).abs() < 1e-6, "{low:?} {very_low:?}");
    // The strips cost level 1 a fraction of what they cost level 0.
    assert!(
        very_low.0 < low.0 && (low.0 - very_low.0) * 4 <= full.0 - very_low.0,
        "{full:?} {low:?} {very_low:?}"
    );
}

/// The generic duct items have no mesh to read the level from: their item definitions
/// lower chord, wall and bolt counts by expression, and nothing a feature is made of.
#[test]
fn duct_items_take_the_level_in_their_definitions() {
    for (name, low, very_low) in [
        ("kowloon-duct-bracket.item.json", 160, 160),
        ("kowloon-duct-riser.item.json", 1800, 1800),
        ("kowloon-duct-run.item.json", 500, 500),
        ("kowloon-corner.item.json", 900, 500),
    ] {
        let levels = [0, 1, 2]
            .map(|level| generate(name, &[("graphicsDetail".to_owned(), json!(level))].into()));
        let counts = levels.each_ref().map(triangles);
        println!("{name}: triangles by level {counts:?}");
        assert_eq!(
            digest(&levels[0]),
            digest(&generate(name, &Values::new())),
            "{name}"
        );
        assert!(
            counts[0] > counts[1] && counts[1] >= counts[2] && counts[2] > 0,
            "{name}: {counts:?}"
        );
        assert!(
            counts[1] <= low && counts[2] <= very_low,
            "{name}: {counts:?}"
        );
        for level in [1, 2] {
            assert_eq!(
                levels[0].geometry.features, levels[level].geometry.features,
                "{name} level {level}"
            );
            assert_eq!(
                serde_json::to_string(&levels[0].attachments).unwrap(),
                serde_json::to_string(&levels[level].attachments).unwrap(),
                "{name} level {level}"
            );
        }
    }
}
