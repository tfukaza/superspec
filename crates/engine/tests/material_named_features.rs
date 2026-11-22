//! Named features: a receiver's owner names polylines, programs measure to them by name, each
//! name has its own grid, and a row's allowance follows its length.
use superspec_engine::materialspec::program::{
    self, FeatureSet, FieldSelection, Inputs, NamedFeature, NamedFeatures, OutputSet, Outputs,
    Program, Sample, Sources,
};

fn compile(text: &str) -> Result<Program, Vec<program::ProgramDiagnostic>> {
    program::compile(&Sources {
        entry: "main.ms".into(),
        modules: [("main.ms".to_owned(), text.to_owned())].into(),
    })
}

fn source(text: &str) -> Program {
    compile(text).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Deterministic values in [0, 1).
fn sequence(mut state: u64) -> impl FnMut() -> f64 {
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// `count` short polylines of each name, scattered over a wall two meters wide and one high.
fn scattered(names: &[&str], count: usize) -> Vec<NamedFeature> {
    let mut next = sequence(0x9E37_79B9_7F4A_7C15);
    let mut features = Vec::new();
    for name in names {
        for _ in 0..count {
            let start = [next() * 2., 0., next()];
            let points = (0..3)
                .map(|step| {
                    let along = step as f64 * 0.01;
                    [start[0] + along, 0., start[2] + along * (next() - 0.5)]
                })
                .collect();
            features.push(NamedFeature {
                name: (*name).to_owned(),
                points,
                closed: false,
                zone: None,
            });
        }
    }
    features
}

fn samples(count: usize) -> Vec<Sample> {
    let mut next = sequence(0x2545_F491_4F6C_DD1D);
    (0..count)
        .map(|_| {
            let world = [next() * 2., 0., next()];
            Sample {
                surface: [world[0], world[2]],
                object: world,
                world,
                normal: [0., -1., 0.],
                footprint: 0.004,
                ..Sample::default()
            }
        })
        .collect()
}

/// Distance from a point to a segment, written apart from the engine's.
fn distance(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let ap: [f64; 3] = std::array::from_fn(|i| p[i] - a[i]);
    let length: f64 = ab.iter().map(|v| v * v).sum();
    let t = if length > 0. {
        (ab.iter().zip(&ap).map(|(x, y)| x * y).sum::<f64>() / length).clamp(0., 1.)
    } else {
        0.
    };
    (0..3)
        .map(|i| (p[i] - (a[i] + ab[i] * t)).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn bits(outputs: &Outputs) -> Vec<u64> {
    let mut all: Vec<f64> = outputs.base_color.to_vec();
    all.extend([outputs.roughness, outputs.metalness, outputs.height]);
    all.into_iter().map(f64::to_bits).collect()
}

const WEATHERED: &str = "material 1;
let sill = feature_distance(\"sill\", 0.05);
let joint = feature_distance(\"joint\", 0.02);
let along = feature_tangent(\"joint\", 0.02);
output base_color = vec3(sill / 0.05, joint / 0.02, abs(along.x));
output roughness = feature_distance(\"missing\", 0.25);
output metalness = 0;";

#[test]
fn queries_by_name_find_the_nearest_segment_of_that_name_among_thousands() {
    let program = source(WEATHERED);
    assert_eq!(
        program.metadata().named_features,
        ["joint", "missing", "sill"]
    );
    for input in ["feature:sill", "feature:joint", "named_features", "world"] {
        assert!(
            program
                .metadata()
                .required_inputs
                .contains(&input.to_owned()),
            "{input}"
        );
    }
    let table = program.dependency_table();
    let color = &table.outputs["base_color"];
    assert_eq!(
        color.named_features.iter().collect::<Vec<_>>(),
        ["joint", "sill"]
    );
    assert!(!color.intrinsic() && !color.features);
    assert!(table.outputs["metalness"].intrinsic());
    // Five names, 1,500 polylines of two segments each: 15,000 segments.
    let named = scattered(&["base", "joint", "opening-edge", "sill", "top"], 1500);
    let features = NamedFeatures::new(&named).unwrap();
    assert_eq!(features.names().count(), 5);
    assert_eq!(features.largest(), 3000);
    assert_eq!(features.get("sill").unwrap().features().len(), 3000);
    assert!(features.get("missing").is_none());
    let inputs = Inputs::default();
    let none = FeatureSet::default();
    let samples = samples(300);
    let selection = FieldSelection::default();
    let mut row = program
        .evaluator_with_feature_sets(&inputs, &none, &features)
        .unwrap();
    let mut alone = program
        .evaluator_with_feature_sets(&inputs, &none, &features)
        .unwrap();
    row.replenish_row(samples.len());
    alone.replenish_row(samples.len());
    let (before_row, before_alone) = (row.allowance(), alone.allowance());
    let mut unused = Vec::new();
    let together = row
        .evaluate_row_together(&samples, OutputSet::ALL, &selection, &mut unused)
        .expect("the row is evaluated together");
    let nearest = |name: &str, point: [f64; 3], limit: f64| {
        named
            .iter()
            .filter(|feature| feature.name == name)
            .flat_map(|feature| feature.points.windows(2))
            .map(|pair| distance(point, pair[0], pair[1]))
            .fold(limit, f64::min)
    };
    let mut reached = 0;
    for (i, sample) in samples.iter().enumerate() {
        let single = alone
            .evaluate_outputs(sample, OutputSet::ALL, &selection, &mut [])
            .unwrap();
        assert_eq!(bits(&together[i]), bits(&single), "sample {i}");
        let sill = nearest("sill", sample.world, 0.05);
        let joint = nearest("joint", sample.world, 0.02);
        assert!(
            (single.base_color[0] - sill / 0.05).abs() < 1e-12,
            "sample {i}"
        );
        assert!(
            (single.base_color[1] - joint / 0.02).abs() < 1e-12,
            "sample {i}"
        );
        assert_eq!(single.roughness, 0.25, "no feature of the name: the limit");
        reached += usize::from(sill < 0.05);
    }
    assert!(reached > 30, "{reached} samples are near a sill");
    assert_eq!(
        before_row - row.allowance(),
        before_alone - alone.allowance(),
        "a row charges what its samples charge one at a time"
    );
    // A query visits its own name's neighborhood, not fifteen thousand segments.
    let spent = (before_row - row.allowance()) / samples.len();
    assert!(spent < 6000, "{spent} operations a sample");
}

#[test]
fn named_feature_sets_are_bounded_and_refused_rather_than_cut() {
    let feature = |name: &str, points: Vec<[f64; 3]>, closed: bool| NamedFeature {
        name: name.to_owned(),
        points,
        closed,
        zone: None,
    };
    let square = vec![[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]];
    let open = NamedFeatures::new(&[feature("edge", square.clone(), false)]).unwrap();
    let closed = NamedFeatures::new(&[feature("edge", square.clone(), true)]).unwrap();
    assert_eq!(open.get("edge").unwrap().features().len(), 3);
    assert_eq!(closed.get("edge").unwrap().features().len(), 4);
    assert!(NamedFeatures::new(&[]).unwrap().is_empty());
    let failed = |named: &[NamedFeature]| {
        let error = NamedFeatures::new(named).err().expect("refused");
        assert_eq!(NamedFeatures::validate(named).unwrap_err(), error);
        error
    };
    assert!(failed(&[feature("two words", square.clone(), false)]).contains("'two words'"));
    assert!(failed(&[feature("", square.clone(), false)]).contains("letters"));
    assert!(
        failed(&[feature(
            "edge",
            vec![[0., f64::NAN, 0.], [1., 0., 0.]],
            false
        )])
        .contains("finite")
    );
    let long: Vec<[f64; 3]> = (0..program::MAX_NAMED_SEGMENTS + 2)
        .map(|i| [i as f64 * 1e-3, 0., 0.])
        .collect();
    assert!(
        failed(&[feature("edge", long.clone(), false)]).contains("65536 named feature segments")
    );
    assert!(
        NamedFeatures::new(&[feature(
            "edge",
            long[..program::MAX_NAMED_SEGMENTS + 1].to_vec(),
            false
        )])
        .is_ok()
    );
    let many: Vec<NamedFeature> = (0..program::MAX_FEATURE_NAMES + 1)
        .map(|i| feature(&format!("n{i}"), square.clone(), false))
        .collect();
    assert!(failed(&many).contains("at most 256 names"));
}

#[test]
fn feature_queries_are_checked_when_a_program_compiles_and_runs() {
    let failed = |text: &str| {
        let errors = compile(text).err().expect("a rejected program");
        (errors[0].code, errors[0].message.clone())
    };
    let (code, message) =
        failed("material 1; output base_color = vec3(feature_distance(\"a sill\", 0.1));");
    assert_eq!(code, "MS_TYPE");
    assert!(message.contains("Feature names"), "{message}");
    let (code, _) = failed("material 1; output base_color = feature_tangent(\"sill\", vec3(1));");
    assert_eq!(code, "MS_TYPE");
    let many = |count: usize| {
        let reads: Vec<String> = (0..count)
            .map(|i| format!("feature_distance(\"f{i}\", 0.01)"))
            .collect();
        format!(
            "material 1; output base_color = vec3({});",
            reads.join(" + ")
        )
    };
    assert_eq!(source(&many(32)).metadata().named_features.len(), 32);
    let (code, message) = failed(&many(33));
    assert_eq!(code, "MS_LIMIT");
    assert!(message.contains("at most 32 names"), "{message}");
    // A limit that is not positive fails the sample, in a row as alone.
    let program = source(
        "material 1; param reach: scalar = 0.1 range(0, 1);
         output base_color = vec3(feature_distance(\"sill\", reach));",
    );
    let inputs = Inputs {
        parameters: [("reach".to_owned(), 0.0.into())].into(),
        ..Inputs::default()
    };
    let mut evaluator = program.evaluator(&inputs).unwrap();
    let mut unused = Vec::new();
    let error = evaluator
        .evaluate_row(
            &samples(70),
            OutputSet::ALL,
            &FieldSelection::default(),
            &mut unused,
        )
        .unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
    assert!(
        error.message.contains("positive maximum distance"),
        "{error}"
    );
}

#[test]
fn a_row_s_allowance_follows_its_length() {
    // Thirty queries that each visit all four thousand segments of a name: about a million
    // operations a sample, within the sample's own step budget.
    let queries: Vec<String> = (0..30)
        .map(|i| format!("feature_distance(\"joint\", {})", 50 + i))
        .collect();
    let program = source(&format!(
        "material 1; output base_color = vec3(clamp(({}) / 3000, 0, 1));",
        queries.join(" + ")
    ));
    let named = NamedFeatures::new(&scattered(&["joint"], 2000)).unwrap();
    let inputs = Inputs::default();
    let none = FeatureSet::default();
    let samples = samples(100);
    let selection = FieldSelection::default();
    let mut evaluator = program
        .evaluator_with_feature_sets(&inputs, &none, &named)
        .unwrap();
    let mut unused = Vec::new();
    // One sample fits the allowance every row has always had; a hundred do not.
    evaluator.replenish(64_000_000);
    evaluator
        .evaluate_row(&samples[..1], OutputSet::ALL, &selection, &mut unused)
        .unwrap();
    evaluator.replenish(64_000_000);
    let error = evaluator
        .evaluate_row(&samples, OutputSet::ALL, &selection, &mut unused)
        .unwrap_err();
    assert_eq!(error.code, "MS_LIMIT");
    assert!(error.message.contains("operation allowance"), "{error}");
    // Sized by the row, the same samples finish, and a short row still has the old allowance.
    evaluator.replenish_row(samples.len());
    assert_eq!(evaluator.allowance(), 220_000_000);
    let long = evaluator
        .evaluate_row(&samples, OutputSet::ALL, &selection, &mut unused)
        .unwrap();
    assert_eq!(long.len(), 100);
    evaluator.replenish_row(3);
    assert_eq!(evaluator.allowance(), 64_000_000);
    evaluator.replenish_row(usize::MAX);
    assert_eq!(evaluator.allowance(), 4_000_000_000);
}

#[test]
fn the_shader_selects_named_features_by_a_kind_of_their_own() {
    let program = source(WEATHERED);
    let shader = program.shader().unwrap();
    assert!(shader.features);
    assert_eq!(shader.named_features, ["joint", "missing", "sill"]);
    // Selector 8 + k reads the features a host uploads with kind 5 + k.
    assert!(
        shader.source.contains("ms_crease_distance(10, 0.0, 0.05)"),
        "sill"
    );
    assert!(
        shader.source.contains("ms_crease_distance(8, 0.0, 0.02)"),
        "joint"
    );
    assert!(shader.source.contains("ms_crease_tangent(8, 0.0, 0.02)"));
    assert!(
        shader
            .source
            .contains("if (selector >= 8) return kind == float(selector - 3);")
    );
    assert!(shader.reads.outputs["base_color"].features);
    // A program without such queries says nothing of them.
    let plain = source("material 1; output base_color = vec3(0.5);");
    let json = serde_json::to_value(plain.shader().unwrap()).unwrap();
    assert!(json.get("named_features").is_none() && json.get("namedFeatures").is_none());
}
