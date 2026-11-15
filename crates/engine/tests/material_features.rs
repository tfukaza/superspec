//! Receiver feature queries: creases and open edges derived from receiver geometry.
use std::collections::BTreeMap;
use superspec_engine::{
    authoring::{Receiver, Request, Session},
    materialspec::program::{
        self, BakeRequest, Domain, Feature, FeatureKind, FieldValue, Inputs, Program, Sample,
        Sources, orient_relief, receiver_features,
    },
};

/// A unit cube with split vertices per face (24 vertices), as exporters usually write it.
fn cube() -> (Vec<f64>, Vec<u32>) {
    let faces: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
        ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]), // top: origin, u, v
        ([0., 1., 0.], [1., 0., 0.], [0., -1., 0.]), // bottom (wound outward)
        ([0., 0., 0.], [1., 0., 0.], [0., 0., 1.]), // front y=0
        ([1., 1., 0.], [-1., 0., 0.], [0., 0., 1.]), // back y=1
        ([0., 1., 0.], [0., -1., 0.], [0., 0., 1.]), // left x=0
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]), // right x=1
    ];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (origin, u, v) in faces {
        let base = (positions.len() / 3) as u32;
        for (a, b) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
            for axis in 0..3 {
                positions.push(origin[axis] + u[axis] * a + v[axis] * b);
            }
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (positions, indices)
}

fn quad() -> (Vec<f64>, Vec<u32>) {
    (
        vec![0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.],
        vec![0, 1, 2, 0, 2, 3],
    )
}

/// A floor (z=0, normal +z) meeting a wall (x=1, normal -x) at an inner corner, plus a fold.
fn corner(fold_degrees: f64) -> (Vec<f64>, Vec<u32>) {
    let lift = fold_degrees.to_radians().tan();
    // Floor from x=0..1, wall rising at x=1; a second floor panel from x=-1..0 tilts by the fold.
    let positions = vec![
        0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0., // floor 0..3
        1., 0., 1., 1., 1., 1., // wall top 4, 5
        -1., 0., lift, -1., 1., lift, // tilted panel far edge 6, 7
    ];
    let indices = vec![
        0, 1, 2, 0, 2, 3, // floor
        1, 4, 5, 1, 5, 2, // wall (normal -x, facing the floor)
        6, 0, 3, 6, 3, 7, // tilted panel sharing edge x=0
    ];
    (positions, indices)
}

fn by_kind(features: &[Feature], kind: FeatureKind) -> Vec<&Feature> {
    features.iter().filter(|f| f.kind == kind).collect()
}

#[test]
fn a_cube_has_twelve_convex_right_angle_edges_and_no_triangulation_diagonals() {
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    assert_eq!(features.len(), 12, "{features:?}");
    assert!(features.iter().all(|f| f.kind == FeatureKind::Convex));
    assert!(features.iter().all(|f| (f.angle - 90.).abs() < 1e-9));
    for feature in &features {
        let length: f64 = (0..3)
            .map(|i| (feature.end[i] - feature.start[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(
            (length - 1.).abs() < 1e-12,
            "diagonals would be longer: {feature:?}"
        );
    }
    assert_eq!(
        receiver_features(&positions, &indices),
        features,
        "deterministic"
    );
}

#[test]
fn an_open_panel_has_four_boundary_edges() {
    let (positions, indices) = quad();
    let features = receiver_features(&positions, &indices);
    assert_eq!(features.len(), 4);
    assert!(
        features
            .iter()
            .all(|f| f.kind == FeatureKind::Boundary && f.angle == 180.)
    );
}

#[test]
fn inner_corners_are_concave_and_shallow_folds_keep_their_angle() {
    let (positions, indices) = corner(20.);
    let features = receiver_features(&positions, &indices);
    // The floor meets the wall at x = 1 (90 degrees), and the panel tilted 20 degrees upward away
    // from the floor makes a shallow valley at x = 0. Both bend inward.
    let concave = by_kind(&features, FeatureKind::Concave);
    assert_eq!(concave.len(), 2, "{features:?}");
    let wall = concave
        .iter()
        .find(|f| f.start[0] == 1. && f.end[0] == 1.)
        .unwrap();
    assert!((wall.angle - 90.).abs() < 1e-9);
    let fold: Vec<_> = features
        .iter()
        .filter(|f| f.start[0] == 0. && f.end[0] == 0.)
        .collect();
    assert_eq!(fold.len(), 1);
    assert_eq!(fold[0].kind, FeatureKind::Concave);
    assert!((fold[0].angle - 20.).abs() < 1e-9);
    // Tilting the panel downward turns the same edge into a ridge.
    let (positions, indices) = corner(-20.);
    let ridge: Vec<_> = receiver_features(&positions, &indices)
        .into_iter()
        .filter(|f| f.start[0] == 0. && f.end[0] == 0.)
        .collect();
    assert_eq!(ridge.len(), 1);
    assert_eq!(ridge[0].kind, FeatureKind::Convex);
    assert!((ridge[0].angle - 20.).abs() < 1e-9);
    // A nearly flat fold is a triangulation seam, not a feature.
    let (positions, indices) = corner(0.5);
    assert!(
        receiver_features(&positions, &indices)
            .iter()
            .all(|f| !(f.start[0] == 0. && f.end[0] == 0.))
    );
}

#[test]
fn degenerate_and_out_of_range_triangles_are_ignored() {
    let (mut positions, mut indices) = quad();
    indices.extend([0, 0, 1, 2, 9, 1]);
    positions.extend([f64::NAN, 0., 0.]);
    assert_eq!(receiver_features(&positions, &indices).len(), 4);
}

/// Shared by native tests and the acceptance-only Wasm artifact.
pub fn check_large_feature_indices() {
    let (positions, indices) = quad();
    let expected = receiver_features(&positions, &indices);
    assert_eq!(expected.len(), 4);
    for index in [0x5555_5555, 0x5555_5556, u32::MAX] {
        assert!(
            receiver_features(&positions, &[0, 1, index]).is_empty(),
            "out-of-range vertex {index} cannot wrap into valid coordinates"
        );
        let mut mixed = indices.clone();
        mixed.extend([0, 1, index]);
        assert_eq!(
            receiver_features(&positions, &mixed),
            expected,
            "ignored triangle {index} cannot alter the valid receiver features"
        );
    }
}

#[test]
fn large_indices_are_ignored_without_overflow_on_any_target() {
    check_large_feature_indices();
}

fn ms(text: &str) -> Program {
    program::compile(&Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([("main.ms".into(), text.into())]),
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}

fn distance(program: &Program, features: &[Feature], world: [f64; 3]) -> f64 {
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_features(&inputs, features).unwrap();
    let out = evaluator
        .evaluate(&Sample {
            world,
            ..Sample::default()
        })
        .unwrap();
    out.height
}

#[test]
fn crease_distance_measures_meters_to_selected_edges_and_reports_the_limit_when_absent() {
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let exposed = ms(
        "material 1; output base_color = vec3(0.5); output height = crease_distance(\"exposed\", 30.0, 0.25);",
    );
    // Top face center is 0.5 m from every top edge, beyond the 0.25 m limit.
    assert_eq!(distance(&exposed, &features, [0.5, 0.5, 1.]), 0.25);
    assert!((distance(&exposed, &features, [0.5, 0.9, 1.]) - 0.1).abs() < 1e-12);
    assert_eq!(distance(&exposed, &features, [1., 0.5, 1.]), 0.);
    // The cube has no concave edges, and a 120 degree minimum excludes its right angles.
    let concave = ms(
        "material 1; output base_color = vec3(0.5); output height = crease_distance(\"concave\", 0.0, 0.25);",
    );
    assert_eq!(distance(&concave, &features, [1., 0.5, 1.]), 0.25);
    let sharp = ms(
        "material 1; output base_color = vec3(0.5); output height = crease_distance(\"convex\", 120.0, 0.25);",
    );
    assert_eq!(distance(&sharp, &features, [1., 0.5, 1.]), 0.25);
    // A receiver without features is an empty result, not an error.
    assert_eq!(distance(&exposed, &[], [1., 0.5, 1.]), 0.25);
    // Inputs.features serve direct callers that evaluate one receiver.
    let inputs = Inputs {
        features: features.clone(),
        ..Inputs::default()
    };
    let direct = exposed
        .evaluate(
            &inputs,
            &Sample {
                world: [1., 0.5, 1.],
                ..Sample::default()
            },
        )
        .unwrap();
    assert_eq!(direct.height, 0.);
    assert!(
        exposed
            .metadata()
            .required_inputs
            .contains(&"features".to_owned())
    );
}

#[test]
fn selectors_are_checked_at_compile_time_and_limits_at_evaluation() {
    for (text, fragment) in [
        ("crease_distance(\"edges\", 30.0, 0.1)", "selects one of"),
        ("crease_distance(\"\", 30.0, 0.1)", "literal"),
        ("crease_distance(1.0, 30.0, 0.1)", "literal"),
        ("crease_distance(\"convex\", 30.0)", "Invalid arguments"),
    ] {
        let Err(errors) = program::compile(&Sources {
            entry: "main.ms".into(),
            modules: BTreeMap::from([(
                "main.ms".into(),
                format!("material 1; output base_color = vec3(0.5); output height = {text};"),
            )]),
        }) else {
            panic!("{text} should not compile");
        };
        assert_eq!(errors[0].code, "MS_TYPE", "{text}");
        assert!(
            errors[0].message.contains(fragment),
            "{text}: {}",
            errors[0].message
        );
    }
    let zero = ms(
        "material 1; output base_color = vec3(0.5); output height = crease_distance(\"any\", 30.0, 0.0);",
    );
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let inputs = Inputs::default();
    let mut evaluator = zero.evaluator_with_features(&inputs, &features).unwrap();
    assert_eq!(
        evaluator.evaluate(&Sample::default()).unwrap_err().code,
        "MS_INPUT"
    );
    let many = vec![features[0].clone(); program::MAX_FEATURES + 1];
    assert_eq!(
        zero.evaluator_with_features(&inputs, &many)
            .err()
            .unwrap()
            .code,
        "MS_LIMIT"
    );
    let mut broken = features[0].clone();
    broken.angle = f64::NAN;
    assert_eq!(
        zero.evaluator_with_features(&inputs, &[broken])
            .err()
            .unwrap()
            .code,
        "MS_INPUT"
    );
}

const SDK: &str = "@superspec/material/v1";

fn ts(text: &str) -> Program {
    program::compile(&Sources {
        entry: "main.material.ts".into(),
        modules: BTreeMap::from([("main.material.ts".into(), text.into())]),
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}

fn field(outputs: &program::Outputs, name: &str) -> f64 {
    match outputs.fields[name] {
        FieldValue::Scalar(value) => value,
        ref other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn edge_wear_covers_exposed_edges_and_leaves_face_centers_and_featureless_receivers_clean() {
    let program = ts(&format!(
        "import {{ defineMaterial, edgeWear, layer, vec3 }} from \"{SDK}\";\nconst wear = edgeWear({{ width: 0.05, jitter: 0 }});\nexport default defineMaterial({{\n  base: {{ baseColor: vec3(0.5, 0.5, 0.5) }},\n  layers: [layer({{ id: \"chipped\", mask: wear.coverage, baseColor: vec3(0.8, 0.8, 0.8), height: {{ add: -0.002 }} }})],\n}});\n"
    ));
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_features(&inputs, &features).unwrap();
    let at = |world: [f64; 3]| Sample {
        world,
        ..Sample::default()
    };
    let edge = evaluator.evaluate(&at([1., 0.5, 1.])).unwrap();
    let near = evaluator.evaluate(&at([0.98, 0.5, 1.])).unwrap();
    let center = evaluator.evaluate(&at([0.5, 0.5, 1.])).unwrap();
    assert_eq!(field(&edge, "wear.coverage"), 1.);
    assert!(
        field(&near, "wear.coverage") > 0.9,
        "{}",
        field(&near, "wear.coverage")
    );
    assert_eq!(field(&center, "wear.coverage"), 0.);
    assert!((field(&edge, "wear.height") + 0.002).abs() < 1e-12);
    assert!(
        edge.base_color[0] > center.base_color[0],
        "the wear layer lightens edges"
    );
    assert!(edge.height < center.height);
    // Coarse bakes widen the shoulder by half the footprint, but faces away from edges stay clean:
    // the query limit lies beyond the shoulder, so a capped distance never reads as partial wear.
    for footprint in [0.05, 0.25, 0.4] {
        let sample = Sample {
            footprint,
            ..at([0.5, 0.5, 1.])
        };
        assert_eq!(
            field(&evaluator.evaluate(&sample).unwrap(), "wear.coverage"),
            0.,
            "footprint {footprint}"
        );
    }
    // A receiver with no exposed edges has no wear anywhere.
    let mut bare = program.evaluator_with_features(&inputs, &[]).unwrap();
    assert_eq!(
        field(&bare.evaluate(&at([1., 0.5, 1.])).unwrap(), "wear.coverage"),
        0.
    );
}

#[test]
fn oxide_combines_edge_surface_and_spot_masks() {
    let program = ts(&format!(
        "import {{ defineMaterial, oxide, vec3 }} from \"{SDK}\";\nconst rust = oxide({{ edgeWidth: 0.05, surfaceAmount: 0 , spotAmount: 0 }});\nexport default defineMaterial({{ base: {{ baseColor: vec3(0.5, 0.5, 0.5) }}, channels: {{ roughness: rust.combined }} }});\n"
    ));
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_features(&inputs, &features).unwrap();
    let edge = evaluator
        .evaluate(&Sample {
            world: [1., 0.5, 1.],
            ..Sample::default()
        })
        .unwrap();
    let center = evaluator
        .evaluate(&Sample {
            world: [0.5, 0.5, 1.],
            ..Sample::default()
        })
        .unwrap();
    assert!(field(&edge, "rust.edge") > 0.5);
    assert_eq!(field(&center, "rust.edge"), 0.);
    for name in ["rust.surface", "rust.spot"] {
        assert_eq!(field(&center, name), 0., "{name} is off at zero amount");
    }
    assert!(
        (edge.roughness - field(&edge, "rust.edge")).abs() < 1e-12,
        "the union equals the only active mask"
    );
}

#[test]
fn a_misspelled_selector_is_a_source_linked_error_in_recipes() {
    let Err(errors) = program::compile(&Sources {
        entry: "main.material.ts".into(),
        modules: BTreeMap::from([(
            "main.material.ts".into(),
            format!(
                "import {{ defineMaterial, creaseDistance, vec3 }} from \"{SDK}\";\nconst d = creaseDistance(\"convx\", 30, 0.1);\nexport default defineMaterial({{ base: {{ baseColor: vec3(1) }}, channels: {{ height: d }} }});\n"
            ),
        )]),
    }) else {
        panic!("expected a selector error");
    };
    assert_eq!(errors[0].code, "MS_TYPE");
    assert_eq!(errors[0].source.location.line, 2);
}

#[test]
fn authoring_bakes_each_receiver_against_its_own_creases() {
    let (positions, indices) = cube();
    let normals = vec![0., 0., 1.].repeat(positions.len() / 3);
    let uvs = (0..positions.len() / 3)
        .flat_map(|i| [(i % 2) as f64, ((i / 2) % 2) as f64])
        .collect();
    let receiver = Receiver {
        id: "box".into(),
        object_id: "box".into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    };
    let recipe = format!(
        "import {{ defineMaterial, edgeWear, vec3 }} from \"{SDK}\";\nconst wear = edgeWear({{ width: 0.05 }});\nexport default defineMaterial({{ base: {{ baseColor: vec3(0.5, 0.5, 0.5) }}, channels: {{ roughness: wear.coverage }} }});\n"
    );
    let mut session = Session::default();
    let request = Request {
        entry: "main.material.ts".into(),
        source: recipe,
        receivers: vec![receiver],
        resolution: 32,
        inspection: Some("wear.coverage".into()),
        ..Request::default()
    };
    let output = session
        .build(&request, 1, &|name| Err(format!("no {name}")), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        output.metadata["requiredInputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "features")
    );
    // The baked inspection map of wear coverage has fully worn texels along the cube's edges and
    // clean texels at face centers.
    let mut worn = false;
    let mut clean = false;
    for resource in output.model.images.values() {
        let image = superspec_geometry::image::decode(&resource.bytes).unwrap();
        for texel in image.rgba.chunks_exact(4).filter(|texel| texel[3] > 0) {
            worn |= texel[0] > 240;
            clean |= texel[0] < 15;
        }
    }
    assert!(worn && clean, "worn {worn}, clean {clean}");
}

fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tolerance)
}

fn vector(outputs: &program::Outputs, name: &str) -> [f64; 3] {
    match outputs.fields[name] {
        FieldValue::Vec3(value) => value,
        ref other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn creases_carry_the_normal_halfway_between_their_faces_and_boundaries_carry_none() {
    let (positions, indices) = cube();
    for feature in receiver_features(&positions, &indices) {
        // Independently: a cube edge's rounded middle points from the cube's center through the
        // edge's midpoint, with no component along the edge.
        let outward: [f64; 3] =
            std::array::from_fn(|i| (feature.start[i] + feature.end[i]) / 2. - 0.5);
        let length = outward.iter().map(|v| v * v).sum::<f64>().sqrt();
        let expected = outward.map(|v| v / length);
        let normal = feature.normal.expect("a crease has a middle normal");
        assert!(close(normal, expected, 1e-12), "{feature:?}");
    }
    let (positions, indices) = quad();
    assert!(
        receiver_features(&positions, &indices)
            .iter()
            .all(|f| f.normal.is_none())
    );
}

#[test]
fn crease_normal_returns_the_nearest_facing_crease_middle_or_the_receiver_normal() {
    let program = ms(
        "material 1; let n = crease_normal(\"convex\", 30.0, 0.05); output base_color = vec3(0.5);",
    );
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_features(&inputs, &features).unwrap();
    let at = |world: [f64; 3], normal: [f64; 3]| Sample {
        world,
        normal,
        ..Sample::default()
    };
    let half = 0.5f64.sqrt();
    let up = [0., 0., 1.];
    let near = evaluator.evaluate(&at([0.98, 0.5, 1.], up)).unwrap();
    assert!(close(vector(&near, "n"), [half, 0., half], 1e-12));
    let center = evaluator.evaluate(&at([0.5, 0.5, 1.], up)).unwrap();
    assert_eq!(
        vector(&center, "n"),
        up,
        "beyond the limit the receiver normal returns"
    );
    // A sample facing down sees the same nearby top edge as facing away, so it is ignored.
    let down = [0., 0., -1.];
    let flipped = evaluator.evaluate(&at([0.98, 0.5, 1.], down)).unwrap();
    assert_eq!(vector(&flipped, "n"), down);
    // Boundaries have one face and never bend the normal.
    let (positions, indices) = quad();
    let open = receiver_features(&positions, &indices);
    let mut panel = program.evaluator_with_features(&inputs, &open).unwrap();
    assert_eq!(
        vector(&panel.evaluate(&at([0., 0.5, 0.], up)).unwrap(), "n"),
        up
    );
}

#[test]
fn adjust_normal_turns_along_the_great_circle_by_a_clamped_weight() {
    let turned = |weight: &str| {
        let program = ms(&format!(
            "material 1; let n = adjust_normal(vec3(0.0, 0.0, 2.0), vec3(1.0, 0.0, 0.0), {weight}); output base_color = vec3(0.5);"
        ));
        vector(
            &program
                .evaluate(&Inputs::default(), &Sample::default())
                .unwrap(),
            "n",
        )
    };
    let degrees = |angle: f64| [angle.to_radians().sin(), 0., angle.to_radians().cos()];
    assert!(
        close(turned("0.0"), [0., 0., 1.], 1e-12),
        "inputs are normalized"
    );
    assert!(close(turned("1.0"), [1., 0., 0.], 1e-12));
    assert!(close(turned("0.5"), degrees(45.), 1e-12));
    assert!(close(turned("0.25"), degrees(22.5), 1e-12));
    assert!(
        close(turned("2.0"), [1., 0., 0.], 1e-12),
        "weights clamp to one"
    );
    assert!(close(turned("-1.0"), [0., 0., 1.], 1e-12), "and to zero");
    let opposite = ms(
        "material 1; let n = adjust_normal(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0), 0.5); output base_color = vec3(0.5);",
    );
    let value = opposite
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap();
    assert_eq!(
        vector(&value, "n"),
        [0., 0., 1.],
        "no unique path between opposites"
    );
    let zero = ms(
        "material 1; let n = adjust_normal(vec3(0.0), vec3(0.0, 0.0, 1.0), 0.5); output base_color = vec3(0.5);",
    );
    assert!(
        zero.evaluate(&Inputs::default(), &Sample::default())
            .is_err()
    );
}

#[test]
fn relief_is_rotated_with_the_adjusted_receiver_normal_in_the_encoded_axes() {
    let (x, y, up) = ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]);
    let relief = [0., 0.6, 0.8];
    assert_eq!(orient_relief(relief, x, y, up, up).unwrap(), relief);
    // Tilting the receiver 30 degrees about +y turns a flat relief to that direction, and a tilted
    // relief turns with it, computed here by rotating about y explicitly.
    let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let tilt = [s, 0., c];
    assert!(close(
        orient_relief(up, x, y, up, tilt).unwrap(),
        tilt,
        1e-12
    ));
    let rotated = [relief[2] * s, relief[1], relief[2] * c];
    assert!(close(
        orient_relief(relief, x, y, up, tilt).unwrap(),
        rotated,
        1e-12
    ));
    // The axes follow the supplied handedness, so a reversed bitangent mirrors y.
    let mirrored = orient_relief(relief, x, [0., -1., 0.], up, tilt).unwrap();
    assert!(close(mirrored, rotated, 1e-12), "{mirrored:?}");
    // A receiver normal off the face plane (smooth shading) is the frame's own z axis.
    let smooth = [0., s, c];
    assert!(close(
        orient_relief(relief, x, y, smooth, smooth).unwrap(),
        relief,
        1e-12
    ));
    // An adjusted normal at or beyond the receiver's horizon is rejected.
    assert!(orient_relief(relief, x, y, up, [1., 0., 0.]).is_err());
    assert!(orient_relief(relief, x, y, up, [0., 0., -1.]).is_err());
}

fn bake(text: &str) -> program::Baked {
    ms(text)
        .bake(
            &BakeRequest {
                width: 8,
                height: 6,
                domain: Domain {
                    u: [0.4, 0., 0.],
                    v: [0., 0.3, 0.],
                    ..Domain::default()
                },
                inspection: None,
            },
            &Inputs::default(),
        )
        .unwrap()
}

#[test]
fn receiver_normal_output_rotates_baked_relief_and_its_identity_changes_no_bytes() {
    let relief = "output base_color = vec3(0.5); output height = 0.01 * sin(surface.x * 40.0);";
    let plain = bake(&format!("material 1; {relief}"));
    let identity = bake(&format!(
        "material 1; {relief} output receiver_normal = normal;"
    ));
    assert_eq!(
        identity.normal, plain.normal,
        "the receiver's own normal is a no-op"
    );
    assert_eq!(identity.base_color, plain.base_color);
    // A constant tilt on a flat receiver encodes that direction in the u, v, n tangent axes.
    // (0.28, 0, 0.96) encodes to 0.64 * 255 = 163.2 and 0.98 * 255 = 249.9: no rounding ties.
    let flat = bake(
        "material 1; output base_color = vec3(0.5); output receiver_normal = vec3(0.28, 0.0, 0.96);",
    );
    for texel in flat.normal.chunks_exact(4) {
        assert_eq!(texel, [163, 128, 250, 255]);
    }
    let backwards = ms(
        "material 1; output base_color = vec3(0.5); output receiver_normal = vec3(0.0, 0.0, -1.0);",
    )
    .bake(
        &BakeRequest {
            width: 2,
            height: 2,
            domain: Domain::default(),
            inspection: None,
        },
        &Inputs::default(),
    )
    .unwrap_err();
    assert_eq!(backwards.code, "MS_OUTPUT");
    let Err(errors) = program::compile(&Sources {
        entry: "main.ms".into(),
        modules: BTreeMap::from([(
            "main.ms".into(),
            "material 1; output base_color = vec3(0.5); output receiver_normal = 1.0;".into(),
        )]),
    }) else {
        panic!("a scalar receiver normal must not compile");
    };
    assert_eq!(errors[0].code, "MS_TYPE");
}

/// The cube with outward per-face vertex normals, as a flat-shaded export writes it.
fn shaded_cube() -> Receiver {
    let (positions, indices) = cube();
    let mut normals = vec![0.; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let p =
            |i: u32| -> [f64; 3] { std::array::from_fn(|axis| positions[i as usize * 3 + axis]) };
        let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
        let (u, v) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        );
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = n.iter().map(|v| v * v).sum::<f64>().sqrt();
        for index in triangle {
            for axis in 0..3 {
                normals[*index as usize * 3 + axis] = n[axis] / length;
            }
        }
    }
    let uvs = (0..positions.len() / 3)
        .flat_map(|i| [(i % 2) as f64, ((i / 2) % 2) as f64])
        .collect();
    Receiver {
        id: "box".into(),
        object_id: "box".into(),
        positions,
        normals,
        uvs,
        indices,
        object_to_world: None,
        contacts: Vec::new(),
    }
}

fn studio_images(recipe: String) -> BTreeMap<String, Vec<u8>> {
    let mut session = Session::default();
    let request = Request {
        entry: "main.material.ts".into(),
        source: recipe,
        receivers: vec![shaded_cube()],
        resolution: 32,
        ..Request::default()
    };
    let output = session
        .build(&request, 1, &|name| Err(format!("no {name}")), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    output
        .model
        .images
        .iter()
        .map(|(id, resource)| (id.to_string(), resource.bytes.to_vec()))
        .collect()
}

#[test]
fn rounded_edges_bend_baked_normals_near_creases_and_leave_faces_flat() {
    let recipe = |channels: &str, imports: &str| {
        format!(
            "import {{ defineMaterial, vec3{imports} }} from \"{SDK}\";\nconst edges = roundedEdges({{ radius: 0.2 }});\nexport default defineMaterial({{ base: {{ baseColor: vec3(0.5, 0.5, 0.5) }}, channels: {{ {channels} }} }});\n"
        )
    };
    let plain = studio_images(recipe("", ", roundedEdges"));
    let identity = studio_images(recipe("receiverNormal: normal", ", roundedEdges, normal"));
    assert_eq!(
        identity, plain,
        "the receiver normal channel at its default is byte-identical"
    );
    let rounded = studio_images(recipe(
        "receiverNormal: edges.receiverNormal",
        ", roundedEdges",
    ));
    let changed: Vec<_> = rounded
        .keys()
        .filter(|id| !plain.contains_key(*id))
        .collect();
    assert_eq!(changed.len(), 1, "only the normal map changes: {changed:?}");
    let image = superspec_geometry::image::decode(&rounded[changed[0]]).unwrap();
    let texels: Vec<_> = image.rgba.chunks_exact(4).filter(|t| t[3] > 0).collect();
    // On the crease the normal turns halfway (45 degrees) toward the neighbor face, so one
    // tangent component reaches about 0.7; face centers keep the flat (128, 128, 255) encoding.
    let bent = texels
        .iter()
        .filter(|t| (t[0] as i32 - 128).abs() > 70 || (t[1] as i32 - 128).abs() > 70)
        .count();
    let flat = texels.iter().filter(|t| t[..3] == [128, 128, 255]).count();
    assert!(
        bent > 0 && flat > 0,
        "bent {bent}, flat {flat} of {}",
        texels.len()
    );
    assert!(
        texels.iter().all(|t| t[2] >= 128),
        "every adjusted normal faces out of its face"
    );
}

#[test]
fn layers_turn_the_receiver_normal_by_their_coverage() {
    let program = ts(&format!(
        "import {{ defineMaterial, layer, vec3 }} from \"{SDK}\";\nexport default defineMaterial({{\n  base: {{ baseColor: vec3(0.5, 0.5, 0.5) }},\n  layers: [layer({{ id: \"lean\", mask: 0.5, receiverNormal: vec3(1, 0, 0) }})],\n}});\n"
    ));
    let value = program
        .evaluate(&Inputs::default(), &Sample::default())
        .unwrap();
    let half = 45f64.to_radians();
    assert!(close(
        value.receiver_normal.unwrap(),
        [half.sin(), 0., half.cos()],
        1e-12
    ));
}

#[test]
fn normal_offsets_bend_the_shaded_normal_after_relief() {
    use superspec_engine::materialspec::program::bend_shaded;
    let (x, y, up) = ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]);
    let shaded = [0., 0.6, 0.8];
    assert_eq!(
        bend_shaded(shaded, x, y, up, [0.; 3]).unwrap(),
        shaded,
        "a zero offset changes nothing"
    );
    // Independently: the shaded normal plus the offset, renormalized.
    let sum = [0.5, 0.6, 0.8];
    let length = (0.25f64 + 0.36 + 0.64).sqrt();
    assert!(close(
        bend_shaded(shaded, x, y, up, [0.5, 0., 0.]).unwrap(),
        sum.map(|v| v / length),
        1e-12
    ));
    assert!(
        bend_shaded(shaded, x, y, up, [0., 0., -2.]).is_err(),
        "turning inside out is rejected"
    );
    let plain = bake(
        "material 1; output base_color = vec3(0.5); output height = 0.01 * sin(surface.x * 40.0);",
    );
    let zero = bake(
        "material 1; output base_color = vec3(0.5); output height = 0.01 * sin(surface.x * 40.0); output normal_offset = vec3(0.0);",
    );
    assert_eq!(zero.normal, plain.normal, "a zero offset is byte-identical");
    // 7/24 across a flat relief normalizes to (0.28, 0, 0.96): 163 and 250 with no rounding ties.
    let bent = bake(
        "material 1; output base_color = vec3(0.5); output normal_offset = vec3(7.0 / 24.0, 0.0, 0.0);",
    );
    for texel in bent.normal.chunks_exact(4) {
        assert_eq!(texel, [163, 128, 250, 255]);
    }
}

#[test]
fn the_concrete_round_over_bends_crease_texels_after_relief() {
    let recipe = |crease: bool| {
        let concrete = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/materialspec/program/examples/kowloon-concrete.material.ts"),
        )
        .unwrap();
        let noise = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/materialspec/program/examples/kowloon-noise.material.ts"),
        )
        .unwrap();
        let program = program::compile(&Sources {
            entry: "kowloon-concrete.material.ts".into(),
            modules: BTreeMap::from([
                ("kowloon-concrete.material.ts".into(), concrete),
                ("kowloon-noise.material.ts".into(), noise),
            ]),
        })
        .unwrap();
        let (positions, indices) = cube();
        let features = receiver_features(&positions, &indices);
        let inputs = Inputs {
            parameters: BTreeMap::from([
                ("creaseWear".into(), serde_json::json!(crease)),
                ("variation".into(), serde_json::json!(false)),
            ]),
            images: [
                "textures/concrete-plain.png",
                "textures/concrete-fine-sand.png",
                "superspec-material:building-plaster-plain.png",
            ]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    program::Image {
                        width: 2,
                        height: 2,
                        rgba: vec![128; 16],
                    },
                )
            })
            .collect(),
            ..Inputs::default()
        };
        let mut evaluator = program.evaluator_with_features(&inputs, &features).unwrap();
        let at = |world: [f64; 3]| Sample {
            world,
            object: world,
            normal: [0., 0., 1.],
            bounds_min: [0.; 3],
            bounds_max: [1.; 3],
            ..Sample::default()
        };
        (
            evaluator
                .evaluate_channels(&at([0.999, 0.5, 1.]), None)
                .unwrap()
                .normal_offset,
            evaluator
                .evaluate_channels(&at([0.5, 0.5, 1.]), None)
                .unwrap()
                .normal_offset,
        )
    };
    let (edge, center) = recipe(true);
    let edge = edge.expect("the concrete recipe declares a normal offset");
    // On the top face next to the x = 1 crease, the adjacent face's normal is +x.
    assert!(edge[0] > 0.9 && edge[2].abs() < 1e-9, "{edge:?}");
    assert_eq!(
        center.unwrap().map(|v| v.abs()).iter().sum::<f64>(),
        0.,
        "face centers are not bent"
    );
    let (off, _) = recipe(false);
    assert_eq!(
        off.unwrap().map(|v| v.abs()).iter().sum::<f64>(),
        0.,
        "edge wear off disables the round-over"
    );
}

/// A flat unit quad whose diagonal is shared by two triangles; `split` gives the second
/// triangle its own vertices with shifted UVs, a texture seam along the diagonal.
fn diagonal_quad(split: bool) -> (Vec<f64>, Vec<f64>, Vec<u32>) {
    let mut positions = vec![0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.];
    let mut uvs = vec![0., 0., 1., 0., 1., 1., 0., 1.];
    let mut indices = vec![0, 1, 2];
    if split {
        positions.extend([0., 0., 0., 1., 1., 0.]);
        uvs.extend([0.5, 0.5, 1.5, 1.5]);
        indices.extend([4, 5, 3]);
    } else {
        indices.extend([0, 2, 3]);
    }
    (positions, uvs, indices)
}

#[test]
fn seams_are_edges_whose_uvs_differ_across_faces_even_when_flat() {
    let (positions, uvs, indices) = diagonal_quad(true);
    let seams = program::receiver_seams(&positions, &uvs, &indices);
    assert_eq!(seams.len(), 1, "{seams:?}");
    assert_eq!(seams[0].kind, FeatureKind::Seam);
    let ends = [seams[0].start, seams[0].end];
    assert!(
        ends.contains(&[0., 0., 0.]) && ends.contains(&[1., 1., 0.]),
        "the diagonal"
    );
    // The coplanar diagonal is no crease, and continuous UVs make no seam.
    assert!(
        receiver_features(&positions, &indices)
            .iter()
            .all(|f| f.kind == FeatureKind::Boundary)
    );
    let (positions, uvs, indices) = diagonal_quad(false);
    assert!(program::receiver_seams(&positions, &uvs, &indices).is_empty());
    // Seams are selected only by name.
    let (positions, uvs, indices) = diagonal_quad(true);
    let features = program::receiver_seams(&positions, &uvs, &indices);
    let near = |selector: &str| {
        distance(
            &ms(&format!(
                "material 1; output base_color = vec3(0.5); output height = crease_distance(\"{selector}\", 0.0, 0.5);"
            )),
            &features,
            [0.5, 0.5, 0.],
        )
    };
    assert_eq!(near("seam"), 0.);
    assert_eq!(near("any"), 0.5);
    assert_eq!(near("contact"), 0.5);
}

#[test]
fn feature_frames_give_the_edge_tangent_and_the_face_across_the_crease() {
    let program = ms("material 1;
        let t = crease_tangent(\"exposed\", 30.0, 0.05);
        let across = crease_adjacent_normal(\"convex\", 30.0, 0.05);
        let c = cross(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0));
        output base_color = vec3(0.5);");
    let (positions, indices) = cube();
    let features = receiver_features(&positions, &indices);
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_features(&inputs, &features).unwrap();
    let at = |world: [f64; 3], normal: [f64; 3]| Sample {
        world,
        normal,
        ..Sample::default()
    };
    let up = [0., 0., 1.];
    // Near the top face's x = 1 edge: that edge runs along y, and the face across it is x = 1.
    let near = evaluator.evaluate(&at([0.98, 0.5, 1.], up)).unwrap();
    let tangent = vector(&near, "t");
    assert!(
        close(tangent.map(f64::abs), [0., 1., 0.], 1e-12),
        "{tangent:?}"
    );
    assert!(close(vector(&near, "across"), [1., 0., 0.], 1e-12));
    assert_eq!(vector(&near, "c"), [0., 0., 1.]);
    // Beyond the limit there is no edge: no tangent, and the receiver's own face.
    let center = evaluator.evaluate(&at([0.5, 0.5, 1.], up)).unwrap();
    assert_eq!(vector(&center, "t"), [0., 0., 0.]);
    assert_eq!(vector(&center, "across"), up);
    // Open boundaries have tangents too, though no face lies across them.
    let (positions, indices) = quad();
    let open = receiver_features(&positions, &indices);
    let mut panel = program.evaluator_with_features(&inputs, &open).unwrap();
    let edge = panel.evaluate(&at([0.01, 0.5, 0.], up)).unwrap();
    assert!(close(vector(&edge, "t").map(f64::abs), [0., 1., 0.], 1e-12));
    assert_eq!(vector(&edge, "across"), up);
}
