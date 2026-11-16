//! Lighting delivery: a scene exported with a lighting record carries its sun as a
//! `KHR_lights_punctual` light and the whole record in the extras; one without is unchanged.
use serde_json::{Value, json};
use superspec_engine::{Scene, compile};
use superspec_export::{Options, Purpose, glb, project_lighting};
use superspec_model::lighting::{Lighting, from_json};

fn scene() -> Scene {
    compile(
        r##"<architecture version="1" id="lit"><box id="block" width="1" depth="1" height="1"/></architecture>"##,
        1,
    )
    .unwrap()
}

fn document(bytes: &[u8]) -> Value {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + length]).unwrap()
}

/// Hamilton product of quaternions (x, y, z, w).
fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// `v` turned by the unit quaternion `q`, by the rotation matrix a glTF reader builds from it.
fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let rows = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    rows.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn quaternion(value: &Value) -> [f64; 4] {
    let parts: Vec<f64> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_f64().unwrap())
        .collect();
    parts.try_into().unwrap()
}

fn record() -> Lighting {
    from_json(
        r#"{"version":1,"exposure":1.4,"toneMapping":"neutral","ambientOcclusion":{"enabled":false},
        "lights":[{"id":"low-sun","direction":[0.6,-0.64,0.48],"color":[1.0,0.5,0.25],"intensity":4.5,
        "shadow":{"style":"plain","darkness":0.5}}]}"#,
    )
    .unwrap()
}

#[test]
fn a_scene_without_a_record_exports_as_before() {
    let scene = scene();
    let doc = document(&glb(&scene, &Options::default()).unwrap());
    assert!(doc.get("extensions").is_none());
    assert!(doc.get("extensionsUsed").is_none());
    assert!(doc["extras"]["superspec"].get("lighting").is_none());
    // Mesh nodes and then the root, with nothing between them.
    let nodes = doc["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), doc["meshes"].as_array().unwrap().len() + 1);
}

#[test]
fn the_sun_is_a_punctual_light_that_shines_the_recorded_way_in_y_up() {
    let mut scene = scene();
    let without = glb(&scene, &Options::default()).unwrap();
    let lighting = record();
    scene.manifest.lighting = Some(lighting.clone());
    let bytes = glb(&scene, &Options::default()).unwrap();
    assert_eq!(bytes, glb(&scene, &Options::default()).unwrap());
    // The same record handed over with the export instead of in the scene writes the same file.
    scene.manifest.lighting = None;
    let handed = Options {
        lighting: Some(&lighting),
        ..Options::default()
    };
    assert_eq!(glb(&scene, &handed).unwrap(), bytes);
    assert_eq!(glb(&scene, &Options::default()).unwrap(), without);

    let doc = document(&bytes);
    assert_eq!(doc["extensionsUsed"], json!(["KHR_lights_punctual"]));
    assert!(doc.get("extensionsRequired").is_none());
    let lights = doc["extensions"]["KHR_lights_punctual"]["lights"]
        .as_array()
        .unwrap();
    assert_eq!(
        lights[..],
        [json!({"name":"low-sun","type":"directional","color":[1.0,0.5,0.25],"intensity":4.5})]
    );

    let nodes = doc["nodes"].as_array().unwrap();
    let root_index = doc["scenes"][0]["nodes"][0].as_u64().unwrap() as usize;
    let root = &nodes[root_index];
    let (index, node) = nodes
        .iter()
        .enumerate()
        .find(|(_, node)| node.get("extensions").is_some())
        .expect("a light node");
    assert_eq!(node["name"], "low-sun");
    assert_eq!(node["extensions"]["KHR_lights_punctual"]["light"], 0);
    assert!(node.get("mesh").is_none());
    assert!(
        root["children"].as_array().unwrap().contains(&json!(index)),
        "the light is placed in the domain frame, under the root"
    );
    // A punctual light shines along its node's -Z. In the world the file describes (Y-up), that
    // must be the recorded direction reversed, with the domain's (x, y, z) at glTF's (x, z, -y).
    let world = multiply(quaternion(&root["rotation"]), quaternion(&node["rotation"]));
    let travel = rotate(world, [0.0, 0.0, -1.0]);
    let [x, y, z] = lighting.lights[0].direction;
    for (actual, expected) in travel.iter().zip([-x, -z, y]) {
        assert!((actual - expected).abs() < 1e-9, "{travel:?}");
    }
}

#[test]
fn the_whole_record_round_trips_through_the_extras() {
    let mut scene = scene();
    let lighting = record();
    scene.manifest.lighting = Some(lighting.clone());
    let doc = document(&glb(&scene, &Options::default()).unwrap());
    let extras = &doc["extras"]["superspec"]["lighting"];
    assert_eq!(from_json(&extras.to_string()), Ok(lighting.clone()));
    assert_eq!(extras["lights"][0]["shadow"]["style"], "plain");
    assert_eq!(extras["toneMapping"], "neutral");
    // The record a project document carries is the one the export is handed.
    let project = json!({"schemaVersion": 2, "lighting": extras});
    assert_eq!(project_lighting(&project), Ok(Some(lighting)));
}

#[test]
fn a_record_without_lights_and_a_collision_export_carry_no_punctual_light() {
    let mut scene = scene();
    let mut dark = record();
    dark.lights.clear();
    scene.manifest.lighting = Some(dark);
    let doc = document(&glb(&scene, &Options::default()).unwrap());
    assert!(doc.get("extensions").is_none());
    assert_eq!(doc["extras"]["superspec"]["lighting"]["lights"], json!([]));

    scene.manifest.lighting = Some(record());
    let collision = Options {
        purpose: Purpose::Collision,
        ..Options::default()
    };
    let doc = document(&glb(&scene, &collision).unwrap());
    assert!(doc.get("extensions").is_none());
    assert!(doc["extras"]["superspec"].get("lighting").is_none());
}
