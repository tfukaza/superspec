//! Items in scene projects (item MVP steps 4 to 7): a project places items, each part's face
//! group is a receiver with a material slot, an edit regenerates only the parts it changes,
//! a part's named features reach its receivers, decals are placed by rule and checked for
//! fit, and a declared handle links a dimension to a control.
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::authoring::{
    Diagnostic, Mapping, MaterialAssignment, ModelSource, Project, ProjectOutput, Session,
};
use superspec_geometry::image;

const WALL: &str = include_str!("../src/itemspec/definition/fixtures/washroom-wall.item.json");
const PIPE: &str = include_str!("../src/itemspec/definition/fixtures/washroom-pipe.item.json");

/// Reads `base` and `sill`, so a wall's receivers are given those features.
const FOLLOWS: &str = "material 1;
let base = feature_distance(\"base\", 0.3);
let sill = feature_distance(\"sill\", 0.2);
output base_color = vec3(base / 0.3, sill / 0.2, 0.5);
output roughness = 0.8;
output metalness = 0;";
const PLAIN: &str = "material 1;
output base_color = vec3(0.4, 0.3, 0.2);
output roughness = 0.5;
output metalness = 1;";

/// A panel with a window, a tile panel in front of it, a decal and a width handle.
fn tiled_wall(decal_offset: f64) -> String {
    json!({
        "format": "itemspec", "version": 2, "id": "tiled-wall",
        "parameters": {"width": {"type": "number", "default": 2.7, "min": 1.7, "max": 8, "unit": "m"}},
        "parts": [
            {"id": "wall", "generator": {"kind": "panel", "width": {"parameter": "width"}, "height": 2.6, "thickness": 0.25,
                "openings": [{"id": "window", "width": 0.6, "height": 0.7,
                    "u": {"from": "start", "offset": 0.5}, "v": {"from": "start", "offset": 0.95}}]},
             "slots": {"front": "concrete", "back": "concrete", "edge": "concrete", "reveal": "concrete"}},
            {"id": "tiles", "generator": {"kind": "box", "size": [1.2, 0.012, 0.9]},
             "placement": {"position": [1.2, -0.012, 0.75]},
             "slots": {"side": "tile", "top": "tile", "bottom": "tile"}}
        ],
        "decals": [{"id": "notice", "image": "notice.png", "size": [0.3, 0.2], "on": {"part": "wall", "face": "front"},
            "u": {"from": "start", "offset": decal_offset}, "v": {"from": "start", "offset": 2.0}}],
        "handles": [{"id": "width", "parameter": "width", "at": [{"parameter": "width"}, 0, 1.3], "axis": [1, 0, 0]}]
    })
    .to_string()
}

fn notice() -> Arc<[u8]> {
    let rgba = (0..8 * 4)
        .flat_map(|texel| [200, (texel * 7 % 256) as u8, 40, 255])
        .collect();
    image::encode(&image::Image {
        width: 8,
        height: 4,
        rgba,
        mime: "image/png",
    })
    .unwrap()
    .into()
}

fn asset(name: &str) -> Result<Arc<[u8]>, String> {
    match name {
        "notice.png" => Ok(notice()),
        _ => Err(format!("{name} was not supplied.")),
    }
}

fn item(id: &str, entry: &str, slots: &[(&str, &str)], at: [f64; 3]) -> ModelSource {
    ModelSource {
        id: id.into(),
        kind: "item".into(),
        entry: Some(entry.into()),
        slots: slots
            .iter()
            .map(|(slot, material)| ((*slot).to_owned(), (*material).to_owned()))
            .collect(),
        transform: Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], at]),
        ..ModelSource::default()
    }
}

fn assignment(id: &str, entry: &str) -> MaterialAssignment {
    MaterialAssignment {
        id: id.into(),
        entry: entry.into(),
        ..MaterialAssignment::default()
    }
}

/// The washroom wall and pipe fixtures as two items of one scene.
fn washroom() -> Project {
    let modules: BTreeMap<String, String> = [
        ("follows.ms", FOLLOWS),
        ("plain.ms", PLAIN),
        ("wall.item.json", WALL),
        ("pipe.item.json", PIPE),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .collect();
    Project {
        schema_version: 2,
        kind: "material".into(),
        entry: "follows.ms".into(),
        modules,
        resolution: 32,
        mapping: Mapping::World,
        models: vec![
            item(
                "wall",
                "wall.item.json",
                &[("concrete", "concrete"), ("frame", "steel")],
                [1., 2., 0.],
            ),
            item(
                "pipe",
                "pipe.item.json",
                &[("steel", "steel")],
                [1., 0., 0.],
            ),
        ],
        materials: vec![
            assignment("concrete", "follows.ms"),
            assignment("steel", "plain.ms"),
        ],
        ..Project::default()
    }
}

fn tiled(width: f64, decal_offset: f64) -> Project {
    let mut project = washroom();
    project
        .modules
        .insert("tiled.item.json".into(), tiled_wall(decal_offset));
    let mut model = item(
        "wall",
        "tiled.item.json",
        &[("concrete", "concrete"), ("tile", "steel")],
        [1., 2., 0.],
    );
    model.parameters.insert("width".into(), json!(width));
    project.models = vec![model];
    project
}

/// Edits the tiled wall's definition as JSON.
fn redefine(project: &mut Project, edit: impl FnOnce(&mut Value)) {
    let mut definition: Value = serde_json::from_str(&project.modules["tiled.item.json"]).unwrap();
    edit(&mut definition);
    project
        .modules
        .insert("tiled.item.json".into(), definition.to_string());
}

fn close(a: f32, b: f64) -> bool {
    (f64::from(a) - b).abs() < 1e-6
}

fn messages(errors: Vec<Diagnostic>) -> String {
    errors
        .iter()
        .map(|error| format!("{} {}", error.code, error.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn shade(session: &mut Session, project: &Project, revision: u64) -> ProjectOutput {
    session
        .shade_project(project, revision, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)))
}

fn positions(output: &ProjectOutput, id: &str) -> Vec<f32> {
    let scene = output.scene(1).unwrap();
    let mesh = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id == id)
        .unwrap_or_else(|| panic!("no mesh {id}"));
    scene.positions[mesh.vertex_start * 3..(mesh.vertex_start + mesh.vertex_count) * 3].to_vec()
}

fn part<'a>(metadata: &'a Value, item: &str, part: &str) -> &'a Value {
    metadata["items"][item]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["id"] == part)
        .unwrap_or_else(|| panic!("no part {part}"))
}

#[test]
fn a_scene_places_items_and_bakes_a_material_per_slot() {
    let project = washroom();
    let output = Session::default()
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let scene = output.scene(1).unwrap();
    let ids: Vec<&str> = scene
        .manifest
        .meshes
        .iter()
        .map(|mesh| mesh.id.as_str())
        .collect();
    // One receiver per part and face group, under the model's identity.
    for id in [
        "wall/part/wall/wall/front",
        "wall/part/wall/wall/back",
        "wall/part/wall/wall/edge",
        "wall/part/wall/wall/reveal",
        "wall/part/wall/wall/frame",
        "wall/part/wall/wall/glazing",
        "wall/part/wall/wall/sill",
        "pipe/part/riser/riser/body",
        "pipe/part/riser/riser/fitting",
        "pipe/part/riser/riser/bolt",
        "pipe/part/riser/riser/end",
    ] {
        assert!(ids.contains(&id), "{id} of {ids:?}");
    }
    let bindings = &output.metadata["bindings"];
    assert_eq!(bindings["wall/part/wall/wall/front"], "concrete");
    assert_eq!(bindings["wall/part/wall/wall/sill"], "concrete");
    assert_eq!(bindings["wall/part/wall/wall/frame"], "steel");
    assert_eq!(bindings["pipe/part/riser/riser/body"], "steel");
    // The glass slot is bound to no material of the project: it keeps a plain surface.
    assert!(bindings.get("wall/part/wall/wall/glazing").is_none());
    let material = |id: &str| {
        scene
            .manifest
            .meshes
            .iter()
            .find(|mesh| mesh.id == id)
            .unwrap()
            .material
            .clone()
            .unwrap()
    };
    assert_eq!(
        material("wall/part/wall/wall/glazing"),
        "studio/item/slot/glass"
    );
    assert_ne!(
        material("wall/part/wall/wall/front"),
        "studio/item/slot/concrete"
    );
    assert_ne!(
        material("wall/part/wall/wall/front"),
        material("pipe/part/riser/riser/body")
    );
    // The wall stands where the model's transform puts it: its front face is the plane y = 2.
    let front = positions(&output, "wall/part/wall/wall/front");
    assert!(front.chunks_exact(3).all(|p| p[1] == 2.));
    let (low, high) = front
        .chunks_exact(3)
        .fold((f32::MAX, f32::MIN), |(low, high), p| {
            (low.min(p[0]), high.max(p[0]))
        });
    assert!(close(low, 1.) && close(high, 3.7), "{low} {high}");
    // A binding for a slot the item does not emit is unused (an item's slots follow its
    // controls); a material the project does not have is refused.
    let mut spare = washroom();
    spare.models[0].slots.insert("paint".into(), "steel".into());
    let spared = Session::default()
        .build_project(&spare, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    assert_eq!(spared.metadata["bindings"], output.metadata["bindings"]);
    let mut wrong = washroom();
    wrong.models[1]
        .slots
        .insert("steel".into(), "chrome".into());
    let error = messages(
        Session::default()
            .build_project(&wrong, 1, &asset, &|| false)
            .err()
            .unwrap(),
    );
    assert!(error.contains("chrome, which is not a material"), "{error}");
}

#[test]
fn an_edit_regenerates_only_the_parts_whose_key_changed() {
    let mut session = Session::default();
    let mut project = washroom();
    let first = shade(&mut session, &project, 1);
    let wall_key = part(&first.metadata, "wall", "wall")["key"].clone();
    let riser_key = part(&first.metadata, "pipe", "riser")["key"].clone();
    assert_eq!(first.metadata["items"]["wall"]["generation"], 1);
    assert_eq!(first.metadata["items"]["wall"]["rebuilt"], json!(["wall"]));
    assert_eq!(first.metadata["items"]["pipe"]["rebuilt"], json!(["riser"]));
    // A pipe edit: the wall is not generated again and its meshes are the same bytes.
    project.models[1]
        .parameters
        .insert("height".into(), json!(2.5));
    let second = shade(&mut session, &project, 2);
    assert_eq!(second.metadata["items"]["wall"]["generation"], 1);
    assert_eq!(part(&second.metadata, "wall", "wall")["key"], wall_key);
    assert_eq!(second.metadata["items"]["pipe"]["generation"], 2);
    assert_eq!(
        second.metadata["items"]["pipe"]["rebuilt"],
        json!(["riser"])
    );
    assert_ne!(part(&second.metadata, "pipe", "riser")["key"], riser_key);
    for group in [
        "front", "back", "edge", "reveal", "frame", "glazing", "sill",
    ] {
        let id = format!("wall/part/wall/wall/{group}");
        assert_eq!(positions(&first, &id), positions(&second, &id), "{id}");
    }
    // The riser grew by what the control says: its top went from 2.3 + the wall bend to 2.5.
    let top = |output: &ProjectOutput| {
        positions(output, "pipe/part/riser/riser/body")
            .chunks_exact(3)
            .map(|p| p[2])
            .fold(f32::MIN, f32::max)
    };
    assert!((top(&second) - top(&first) - 0.2).abs() < 1e-5);
    // The same values again build nothing.
    let third = shade(&mut session, &project, 3);
    assert_eq!(third.metadata["items"]["pipe"]["generation"], 2);

    // Within one item: widening the wall regenerates the panel and keeps the tile panel.
    let mut session = Session::default();
    let narrow = shade(&mut session, &tiled(2.7, 0.2), 1);
    let wide = shade(&mut session, &tiled(3.4, 0.2), 2);
    assert_eq!(wide.metadata["items"]["wall"]["rebuilt"], json!(["wall"]));
    assert_eq!(part(&wide.metadata, "wall", "wall")["rebuilt"], true);
    assert_eq!(part(&wide.metadata, "wall", "tiles")["rebuilt"], false);
    assert_eq!(
        part(&wide.metadata, "wall", "tiles")["key"],
        part(&narrow.metadata, "wall", "tiles")["key"]
    );
    for group in ["side", "top", "bottom"] {
        let id = format!("wall/part/tiles/tiles/{group}");
        assert_eq!(positions(&narrow, &id), positions(&wide, &id), "{id}");
    }
    // The window keeps its place from the start; the wall's far edge moved by the difference.
    let reveal = |output: &ProjectOutput| {
        positions(output, "wall/part/wall/wall/reveal")
            .chunks_exact(3)
            .fold((f32::MAX, f32::MIN), |(low, high), p| {
                (low.min(p[0]), high.max(p[0]))
            })
    };
    let (low, high) = reveal(&narrow);
    assert!(close(low, 1.5) && close(high, 2.1), "{low} {high}");
    assert_eq!(reveal(&wide), reveal(&narrow));
}

/// The named-feature records of a mesh's shading entry: twelve numbers each, kind 5 and up.
fn named(metadata: &Value, mesh: &str) -> Vec<Vec<f64>> {
    let numbers: Vec<f64> = metadata["shading"][mesh]["features"]
        .as_array()
        .unwrap_or_else(|| panic!("no shading for {mesh}"))
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect();
    numbers
        .chunks_exact(12)
        .filter(|record| record[3] >= 5.)
        .map(<[f64]>::to_vec)
        .collect()
}

#[test]
fn a_parts_features_reach_its_receivers_in_world_meters() {
    let mut session = Session::default();
    let output = shade(&mut session, &washroom(), 1);
    // The program queries `base` and `sill` (kinds 5 and 6, by name). The wall is 2.7 m wide at
    // (1, 2, 0), 0.25 m thick; its window is 0.9 m wide, 0.6 m from the far end, its sill at
    // min(1.1, 2.6 - 1.0) = 1.1 m with a 0.03 m overhang on the front.
    let front = named(&output.metadata, "wall/part/wall/wall/front");
    let has = |records: &[Vec<f64>], kind: f64, start: [f64; 3], end: [f64; 3]| {
        records.iter().any(|record| {
            record[3] == kind
                && (0..3).all(|axis| {
                    (record[axis] - start[axis]).abs() < 1e-9
                        && (record[4 + axis] - end[axis]).abs() < 1e-9
                })
        })
    };
    assert!(has(&front, 5., [1., 2., 0.], [3.7, 2., 0.]), "{front:?}");
    assert!(
        has(&front, 5., [1., 2.25, 0.], [3.7, 2.25, 0.]),
        "{front:?}"
    );
    assert!(
        has(&front, 6., [2.2 - 0.03, 2., 1.1], [3.1 + 0.03, 2., 1.1]),
        "{front:?}"
    );
    // Every receiver of the part is given them; the pipe's receivers are given none of the wall's.
    assert!(!named(&output.metadata, "wall/part/wall/wall/sill").is_empty());
    assert!(
        output.metadata["shading"]["pipe/part/riser/riser/body"]["features"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The item reports the same features, its anchors and attachments for overlays.
    let wall = &output.metadata["items"]["wall"];
    let names: Vec<&str> = wall["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature["name"].as_str().unwrap())
        .collect();
    for name in ["base", "top", "sill", "opening-edge"] {
        assert!(names.contains(&name), "{name} of {names:?}");
    }
    assert_eq!(
        part(&output.metadata, "wall", "wall")["anchor"]["origin"],
        json!([1.0, 2.0, 0.0])
    );
    let pipe = &output.metadata["items"]["pipe"];
    assert!(
        pipe["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["name"] == "joint")
    );
    assert!(
        pipe["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|attachment| attachment["name"] == "riser/start")
    );
    assert_eq!(pipe["derived"]["spacing"], json!((2.3 - 0.1) / 4.));
}

#[test]
fn a_decal_is_a_render_only_sheet_two_millimeters_off_its_face() {
    for width in [2.2, 2.7, 3.4] {
        let output = shade(&mut Session::default(), &tiled(width, 0.1), 1);
        let scene = output.scene(1).unwrap();
        assert_eq!(scene.manifest.decals, ["wall/decal/notice"]);
        assert!(scene.manifest.colliders.is_empty());
        // The wall's front face is the plane y = 2; the sheet stands 2 mm in front of it,
        // 0.1 m from the wall's start and 2 m up, whatever the wall's width.
        let sheet = positions(&output, "wall/decal/notice");
        assert_eq!(sheet.len(), 12);
        for point in sheet.chunks_exact(3) {
            assert!(
                (f64::from(point[1]) - (2. - 0.002)).abs() < 1e-6,
                "{point:?}"
            );
        }
        let bounds =
            sheet
                .chunks_exact(3)
                .fold(([f32::MAX; 2], [f32::MIN; 2]), |(low, high), p| {
                    (
                        [low[0].min(p[0]), low[1].min(p[2])],
                        [high[0].max(p[0]), high[1].max(p[2])],
                    )
                });
        let ([left, low], [right, high]) = bounds;
        assert!(
            close(left, 1.1) && close(low, 2.) && close(right, 1.4) && close(high, 2.2),
            "{bounds:?}"
        );
        let decal = &output.metadata["items"]["wall"]["decals"][0];
        assert_eq!(decal["mesh"], "wall/decal/notice");
        assert_eq!(decal["offset"], 0.002);
        // The image is one resource named by its bytes; the placement is not in its name.
        let material = scene
            .manifest
            .materials
            .iter()
            .find(|material| material.id.starts_with("studio/decal/material/"))
            .unwrap();
        let texture = material.texture.as_ref().unwrap();
        assert!(texture.starts_with("studio/decal/image/"));
        assert!(!texture.contains("notice") && !texture.contains("wall"));
    }
}

#[test]
fn a_decal_that_does_not_fit_is_a_source_linked_error_and_the_session_goes_on() {
    let mut session = Session::default();
    shade(&mut session, &tiled(2.7, 2.2), 1);
    // Narrowed to 2.4 m, a decal 2.2 m from the start no longer fits inside the margin.
    let narrow = tiled(2.4, 2.2);
    let errors = session
        .shade_project(&narrow, 2, &asset, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "ITEM_DECAL_FIT");
    assert_eq!(errors[0].source_name, "tiled.item.json");
    assert!(
        errors[0].message.starts_with("/decals/0 "),
        "{}",
        errors[0].message
    );
    assert!(
        errors[0]
            .message
            .contains("must stay 0.02 m inside the 2.4 by 2.6 m face")
    );
    // The diagnostic selects the decal in the definition text.
    let text = &narrow.modules["tiled.item.json"];
    let selected = &text[errors[0].offset..errors[0].end.unwrap()];
    assert!(
        selected.starts_with('{') && selected.contains("\"notice\""),
        "{selected}"
    );
    // In the window (0.5 to 1.1 m from the start, 0.95 to 1.65 m up) it lands in an opening.
    let mut inside = tiled(2.7, 0.6);
    redefine(&mut inside, |definition| {
        definition["decals"][0]["v"]["offset"] = json!(1.0)
    });
    let errors = session
        .shade_project(&inside, 3, &asset, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors[0].code, "ITEM_DECAL_FIT");
    assert!(
        errors[0].message.contains("lands in opening window"),
        "{}",
        errors[0].message
    );
    // The session still builds the last valid scene, without generating anything again.
    let again = shade(&mut session, &tiled(2.7, 2.2), 4);
    assert_eq!(again.metadata["items"]["wall"]["generation"], 1);
}

#[test]
fn a_decal_wraps_a_round_route_on_a_straight_run_clear_of_fittings() {
    let pipe = |along: f64| {
        let mut definition: Value = serde_json::from_str(PIPE).unwrap();
        definition["decals"] = json!([{"id": "label", "image": "notice.png", "size": [0.12, 0.08],
            "on": {"part": "riser"}, "along": {"from": "start", "offset": along}, "around": 90}]);
        let mut project = washroom();
        project
            .modules
            .insert("pipe.item.json".into(), definition.to_string());
        project
    };
    let output = shade(&mut Session::default(), &pipe(0.3), 1);
    // The riser stands at (standoff, 2.12) of the item, placed at (1, 0, 0): its axis is the
    // line x = 1.14, y = 2.12. Every vertex of the sheet is 0.09 + 0.002 m from it.
    let sheet = positions(&output, "pipe/decal/label");
    assert!(sheet.len() > 12);
    for point in sheet.chunks_exact(3) {
        let radius = f64::from(point[0] - 1.14).hypot(f64::from(point[1] - 2.12));
        assert!((radius - 0.092).abs() < 1e-5, "{radius}");
        assert!((0.26 - 1e-6..=0.34 + 1e-6).contains(&f64::from(point[2])));
    }
    // Every triangle faces away from the pipe's axis, so the sheet is seen from outside.
    let scene = output.scene(1).unwrap();
    let mesh = scene
        .manifest
        .meshes
        .iter()
        .find(|mesh| mesh.id == "pipe/decal/label")
        .unwrap();
    let indices = &scene.indices[mesh.index_start..mesh.index_start + mesh.index_count];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| {
            let at = triangle[k] as usize * 3;
            [sheet[at], sheet[at + 1], sheet[at + 2]].map(f64::from)
        });
        let normal = [
            (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
        ];
        let outward = [
            (a[0] + b[0] + c[0]) / 3. - 1.14,
            (a[1] + b[1] + c[1]) / 3. - 2.12,
        ];
        assert!(
            normal[0] * outward[0] + normal[1] * outward[1] > 0.,
            "{triangle:?}"
        );
    }
    // On the coupling at 0.55 m it is refused; so it is on the bend at the top.
    for along in [0.55, 2.25] {
        let errors = Session::default()
            .shade_project(&pipe(along), 1, &asset, &|| false)
            .err()
            .unwrap();
        assert_eq!(errors[0].code, "ITEM_DECAL_FIT", "{}", errors[0].message);
    }
}

#[test]
fn a_handle_links_a_dimension_to_a_control() {
    let narrow = shade(&mut Session::default(), &tiled(2.7, 0.1), 1);
    let wide = shade(&mut Session::default(), &tiled(3.4, 0.1), 1);
    let handle = &narrow.metadata["items"]["wall"]["handles"][0];
    assert_eq!(
        handle,
        &json!({"id": "width", "parameter": "width", "position": [3.7, 2.0, 1.3], "axis": [1.0, 0.0, 0.0],
            "scale": 1.0, "value": 2.7, "min": 1.7, "max": 8.0})
    );
    // The handle follows the dimension it drags.
    assert_eq!(
        wide.metadata["items"]["wall"]["handles"][0]["position"],
        json!([4.4, 2.0, 1.3])
    );
    // The controls are reported with their ranges and current values.
    let parameters = narrow.metadata["items"]["wall"]["parameters"]
        .as_array()
        .unwrap();
    assert_eq!(parameters[0]["name"], "width");
    assert_eq!(parameters[0]["value"], 2.7);
    assert_eq!(parameters[0]["min"], 1.7);
    // A handle on a control that is not a numeric geometry control is refused where it is declared.
    let mut wrong = tiled(2.7, 0.1);
    redefine(&mut wrong, |definition| {
        definition["handles"][0]["parameter"] = json!("depth")
    });
    let errors = Session::default()
        .shade_project(&wrong, 1, &asset, &|| false)
        .err()
        .unwrap();
    assert_eq!(errors[0].code, "ITEM_HANDLE");
}

/// Writes a built item scene for the Wasm parity check (`tests/item-scene-parity.mjs`): the
/// project, its companion image and the packet the native engine delivers.
/// An item that weathers a slot: its wall's concrete gets the shipped top grime following the
/// window's sill, as a layer over the material the project binds.
#[test]
fn an_items_layers_weather_its_slot_in_an_assignment_of_their_own() {
    let mut project = washroom();
    for (name, text) in [
        (
            "kowloon-noise.material.ts",
            include_str!("../src/materialspec/program/examples/kowloon-noise.material.ts"),
        ),
        (
            "kowloon-top-grime.material.ts",
            include_str!("../src/materialspec/program/examples/kowloon-top-grime.material.ts"),
        ),
    ] {
        project.modules.insert(name.into(), text.into());
    }
    let mut definition: Value = serde_json::from_str(WALL).unwrap();
    definition["layers"] = json!({"concrete": [
        {"entry": "kowloon-top-grime.material.ts", "parameters": {"amount": 0.7}, "seed": 3, "follow": {"top": "sill"}}
    ]});
    project
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    let output = Session::default()
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let bindings = &output.metadata["bindings"];
    // The weathered slot's receivers are bound to the derived assignment; others are not.
    assert_eq!(
        bindings["wall/part/wall/wall/front"],
        "concrete.wall.concrete"
    );
    assert_eq!(bindings["wall/part/wall/wall/frame"], "steel");
    assert_eq!(bindings["pipe/part/riser/riser/body"], "steel");
    // A GPU host composing the layer is given the wall's sill as the feature the layer follows.
    let shaded = Session::default()
        .shade_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let front = &shaded.metadata["shading"]["wall/part/wall/wall/front"];
    assert_eq!(front["material"], "concrete.wall.concrete");
    let layers = front["layerFeatures"]
        .as_array()
        .unwrap_or_else(|| panic!("{front}"));
    let rows = layers[0].as_array().unwrap();
    let named = rows
        .chunks_exact(12)
        .filter(|row| row[3].as_f64().unwrap() >= 5.)
        .count();
    assert!(
        named > 0,
        "the layer sees no sill: {} rows",
        rows.len() / 12
    );
}
/// The rows of named features (kind 5 and up) among a receiver's feature rows.
fn named_rows(rows: &Value) -> Vec<Vec<f64>> {
    rows.as_array()
        .unwrap_or_else(|| panic!("{rows}"))
        .chunks_exact(12)
        .map(|row| row.iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>())
        .filter(|row| row[3] >= 5.)
        .collect()
}

#[test]
fn a_layer_follows_several_features_under_one_name() {
    let layered = |follow: Value| {
        let mut project = washroom();
        for (name, text) in [
            (
                "kowloon-noise.material.ts",
                include_str!("../src/materialspec/program/examples/kowloon-noise.material.ts"),
            ),
            (
                "kowloon-top-grime.material.ts",
                include_str!("../src/materialspec/program/examples/kowloon-top-grime.material.ts"),
            ),
        ] {
            project.modules.insert(name.into(), text.into());
        }
        let mut definition: Value = serde_json::from_str(WALL).unwrap();
        definition["layers"] = json!({"concrete": [
            {"entry": "kowloon-top-grime.material.ts", "parameters": {"amount": 0.7}, "seed": 3, "follow": {"top": follow}}
        ]});
        project
            .modules
            .insert("wall.item.json".into(), definition.to_string());
        project
    };
    let rows = |follow: Value| {
        let shaded = Session::default()
            .shade_project(&layered(follow), 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)));
        let front = &shaded.metadata["shading"]["wall/part/wall/wall/front"];
        let described = &shaded.metadata["materials"]["concrete.wall.concrete"]["layers"][0];
        (
            named_rows(&front["layerFeatures"][0]),
            described["follow"]["top"].clone(),
        )
    };
    let (sill, _) = rows(json!("sill"));
    let (base, _) = rows(json!("base"));
    let (both, described) = rows(json!(["sill", "base"]));
    assert!(!sill.is_empty() && !base.is_empty());
    // The one name holds the segments of both features, each exactly once, under one kind.
    assert_eq!(both.len(), sill.len() + base.len());
    for row in sill.iter().chain(&base) {
        assert!(both.contains(row), "{row:?} is missing");
    }
    assert!(both.iter().all(|row| row[3] == both[0][3]));
    // A set of features has one spelling: ascending, and one name alone is the name.
    assert_eq!(described, json!(["base", "sill"]));
    assert_eq!(rows(json!(["sill"])).1, json!("sill"));
    assert_eq!(rows(json!(["sill", "sill"])).0, sill);
    // The CPU bake composes the same layer, and what it follows is part of what it bakes.
    let image = |follow: Value| {
        let output = Session::default()
            .build_project(&layered(follow), 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)));
        let model = output.scene(1).unwrap();
        serde_json::to_string(&model.manifest.materials).unwrap()
    };
    let (one, two) = (image(json!("sill")), image(json!(["sill", "base"])));
    assert_ne!(one, two, "following the base too changes what is baked");
    assert_eq!(two, image(json!(["base", "sill", "base"])));
    // A name follows a feature or a list of 1 to 16 of them.
    let refused = Session::default()
        .shade_project(&layered(json!([])), 1, &asset, &|| false)
        .err()
        .expect("an empty list is refused");
    assert_eq!(refused[0].code, "ITEM_LAYER");
}

/// Reads `drip`, so receivers are given the features of that name.
const DRIPS: &str = "material 1;
let drip = feature_distance(\"drip\", 0.3);
output base_color = vec3(drip / 0.3, 0.5, 0.5);
output roughness = 0.8;
output metalness = 0;";

#[test]
fn a_feature_reaches_other_parts_and_items_only_as_far_as_it_says() {
    // The tile panel reports a drip line along its foot; the wall behind it is another part, and
    // the pipe (2 m away) another item.
    let scene = |feature: Value| {
        let mut project = tiled(2.7, 0.1);
        project.modules.insert("drips.ms".into(), DRIPS.into());
        project.materials = vec![assignment("concrete", "drips.ms")];
        redefine(&mut project, |definition| {
            let mut drip = json!({"name": "drip", "part": "tiles", "points": [[1.2, -0.012, 0.75], [2.4, -0.012, 0.75]]});
            for (key, value) in feature.as_object().unwrap() {
                drip[key] = value.clone();
            }
            definition["features"] = json!([drip]);
        });
        project.models[0].slots = [("concrete", "concrete"), ("tile", "concrete")]
            .map(|(a, b)| (a.into(), b.into()))
            .into();
        project.models.push(item(
            "pipe",
            "pipe.item.json",
            &[("steel", "concrete")],
            [1., 0., 0.],
        ));
        project
    };
    let drips = |feature: Value| {
        let shaded = Session::default()
            .shade_project(&scene(feature), 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)));
        let count = |mesh: &str| {
            let described = &shaded.metadata["shading"][mesh];
            assert!(described.is_object(), "{mesh} is not shaded");
            named_rows(&described["features"])
        };
        (
            count("wall/part/tiles/tiles/side"),
            count("wall/part/wall/wall/front"),
            count("pipe/part/riser/riser/body"),
            shaded.metadata["items"]["wall"]["reachDropped"].clone(),
        )
    };
    // By default a feature reaches its own part alone.
    let (own, wall, pipe, _) = drips(json!({}));
    assert_eq!((own.len(), wall.len(), pipe.len()), (1, 0, 0));
    // Reach `item`: the wall behind the tiles is given the same line, in world meters (the item
    // stands at (1, 2, 0)); another item is not.
    let (tiles, wall, pipe, dropped) = drips(json!({"reach": "item"}));
    assert_eq!(wall, own);
    assert_eq!((tiles.len(), pipe.len()), (1, 0));
    assert!((wall[0][0] - 2.2).abs() < 1e-9 && (wall[0][2] - 0.75).abs() < 1e-9);
    assert_eq!(dropped, 0);
    // Reach `scene`: the pipe, 2 m in front, is within the 3 m a feature reaches by default,
    // and not within 0.5 m.
    let (_, wall, pipe, _) = drips(json!({"reach": "scene"}));
    assert_eq!((wall.len(), pipe.len()), (1, 1));
    assert_eq!(pipe, own);
    let (_, wall, pipe, _) = drips(json!({"reach": "scene", "within": 0.5}));
    assert_eq!((wall.len(), pipe.len()), (1, 0));
    // A distance needs a wider reach, and is more than 0 and at most 50 m.
    for feature in [
        json!({"within": 1}),
        json!({"reach": "item", "within": 0}),
        json!({"reach": "item", "within": 51}),
    ] {
        let refused = Session::default()
            .shade_project(&scene(feature.clone()), 1, &asset, &|| false)
            .err()
            .unwrap_or_else(|| panic!("{feature} is accepted"));
        assert_eq!(refused[0].code, "ITEM_FEATURE", "{feature}");
    }
    // A layout or a bake of a receiver holds the features it was given: the CPU build agrees.
    Session::default()
        .build_project(&scene(json!({"reach": "scene"})), 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
}

/// An item's own `densities` sizes its slots' receivers under what the project and the model
/// state, and over a material's density; a bake of the receiver changes with it.
#[test]
fn an_item_asks_for_its_slots_densities_under_what_its_host_states() {
    // A receiver's layout (its size and density) is what its bakes are keyed by.
    let layouts = |project: &Project| -> BTreeMap<String, (String, f64, (usize, usize))> {
        Session::default()
            .project_layouts(project, 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)))
            .receivers
            .into_iter()
            .map(|layout| {
                (
                    layout.id,
                    (
                        layout.material,
                        layout.density,
                        (layout.width, layout.height),
                    ),
                )
            })
            .collect()
    };
    let mut project = washroom();
    project.resolution = 512;
    project.texel_density = Some(16.);
    project.texel_densities.insert("steel".into(), 64.);
    let before = layouts(&project);
    // The wall asks for 128 texels a meter on its frame; the pipe (steel too) is not the wall.
    let mut definition: Value = serde_json::from_str(WALL).unwrap();
    definition["densities"] = json!({"frame": 128});
    project
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    let asked = layouts(&project);
    let frames: Vec<String> = asked
        .iter()
        .filter(|(id, _)| id.starts_with("wall/") && id.ends_with("/frame"))
        .map(|(id, _)| id.clone())
        .collect();
    assert!(!frames.is_empty());
    for (id, (material, density, key)) in &asked {
        if frames.contains(id) {
            // Over the material's 64.
            assert_eq!((material.as_str(), *density), ("steel", 128.), "{id}");
            // (A receiver too small to fill the smallest atlas keeps it.)
            assert!(key.0 * key.1 >= before[id].2.0 * before[id].2.1, "{id}");
        } else {
            assert_eq!((*density, key), (before[id].1, &before[id].2), "{id}");
        }
    }
    assert!(frames.iter().any(|id| asked[id].2 != before[id].2));
    // What is baked on the frame changes with the density the item asks for.
    let baked = |project: &Project| {
        let output = Session::default()
            .build_project(project, 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)));
        serde_json::to_string(&output.metadata["materials"]).unwrap()
    };
    let mut plain = project.clone();
    plain
        .modules
        .insert("wall.item.json".into(), WALL.to_owned());
    assert_ne!(baked(&plain), baked(&project));
    // The model's slot density comes before the item's, and a mesh's own before both.
    project.models[0].slot_densities.insert("frame".into(), 32.);
    let model = layouts(&project);
    assert!(frames.iter().all(|id| model[id].1 == 32.));
    project.texel_densities.insert(frames[0].clone(), 24.);
    let mesh = layouts(&project);
    assert_eq!(mesh[&frames[0]].1, 24.);
    assert!(frames[1..].iter().all(|id| mesh[id].1 == 32.));
    // An item density out of range is refused at its slot.
    let mut definition: Value = serde_json::from_str(WALL).unwrap();
    definition["densities"] = json!({"frame": 0});
    project
        .modules
        .insert("wall.item.json".into(), definition.to_string());
    assert!(
        Session::default()
            .project_layouts(&project, 1, &asset, &|| false)
            .is_err()
    );
}

#[test]
fn native_item_scene_packet_for_parity() {
    let project = tiled(2.7, 0.1);
    let output = Session::default()
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    let packet = output.scene(1).unwrap().packet();
    assert_eq!(&packet[..4], b"LSP3");
    // The same build twice delivers the same bytes.
    let again = Session::default()
        .build_project(&project, 1, &asset, &|| false)
        .unwrap_or_else(|errors| panic!("{}", messages(errors)));
    assert_eq!(again.scene(1).unwrap().packet(), packet);
    if let Ok(directory) = std::env::var("ITEM_SCENE_PARITY_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join("notice.png"), notice()).unwrap();
        std::fs::write(directory.join("packet.bin"), packet).unwrap();
    }
}

/// A material, an item's slot and a single mesh can each be baked at a texel density of their
/// own (a sign's lettering finer than the walls): the mesh's comes first, then its item slot's,
/// then its material's, then the project's. Nothing else's atlas changes.
#[test]
fn a_material_a_slot_or_a_mesh_has_a_texel_density_of_its_own() {
    let densities = |project: &Project| -> BTreeMap<String, (String, f64, usize)> {
        Session::default()
            .project_layouts(project, 1, &asset, &|| false)
            .unwrap_or_else(|errors| panic!("{}", messages(errors)))
            .receivers
            .into_iter()
            .map(|layout| {
                (
                    layout.id,
                    (
                        layout.material,
                        layout.density,
                        layout.width * layout.height,
                    ),
                )
            })
            .collect()
    };
    let mut project = washroom();
    project.resolution = 512;
    project.texel_density = Some(16.);
    let plain = densities(&project);
    assert!(plain.len() > 2);
    assert!(plain.values().all(|(_, density, _)| *density == 16.));

    // By material: every receiver of `steel`, in both items, and no receiver of `concrete`.
    project.texel_densities.insert("steel".into(), 64.);
    let by_material = densities(&project);
    for (id, (material, density, texels)) in &by_material {
        let expected = if material == "steel" { 64. } else { 16. };
        assert_eq!(*density, expected, "{id}");
        // (A receiver too small to fill the smallest atlas keeps it.)
        assert!(*texels >= plain[id].2, "{id}");
        assert!(material == "steel" || *texels == plain[id].2, "{id}");
    }
    assert!(
        by_material
            .iter()
            .any(|(id, (material, _, texels))| material == "steel" && *texels > plain[id].2)
    );

    // By item slot: the wall's frame alone (the pipe is steel too and keeps the material's).
    project.models[0]
        .slot_densities
        .insert("frame".into(), 128.);
    let by_slot = densities(&project);
    let frames: Vec<&String> = by_slot
        .iter()
        .filter(|(_, (_, density, _))| *density == 128.)
        .map(|(id, _)| id)
        .collect();
    assert!(!frames.is_empty() && frames.iter().all(|id| id.starts_with("wall/")));
    for (id, (_, density, _)) in &by_slot {
        if !frames.contains(&id) {
            assert_eq!(*density, by_material[id].1, "{id}");
        }
    }

    // By mesh: one of those frames, over its slot's density.
    let one = frames[0].clone();
    project.texel_densities.insert(one.clone(), 32.);
    let by_mesh = densities(&project);
    assert_eq!(by_mesh[&one].1, 32.);
    for (id, (_, density, _)) in &by_mesh {
        if *id != one {
            assert_eq!(*density, by_slot[id].1, "{id}");
        }
    }

    // The description of the project lays its receivers out the same way, and the project's
    // text carries the densities.
    let text = serde_json::to_string(&project).unwrap();
    assert!(text.contains("\"texelDensities\"") && text.contains("\"slotDensities\""));
    let read: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(densities(&read), by_mesh);
}
