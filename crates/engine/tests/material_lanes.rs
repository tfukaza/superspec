//! Evaluating the samples of a row together returns, bit for bit, what evaluating each alone
//! returns, stays lazy where per-sample evaluation is lazy, and gives way to per-sample evaluation
//! for anything that could fail or exceed a limit.
use std::{path::PathBuf, sync::Arc};
use superspec_engine::materialspec::program::{
    self, Edge, Evaluator, FieldSelection, FieldValue, Image, Inputs, OutputSet, Outputs, Program,
    Sample, Sources,
};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/materialspec/program/examples")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(examples().join(name)).unwrap()
}

fn image(name: &str) -> Arc<Image> {
    let path = match name.strip_prefix("superspec-material:") {
        Some(catalog) => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(catalog),
        None => examples().join(name),
    };
    let decoded = superspec_geometry::image::decode(&std::fs::read(path).unwrap()).unwrap();
    Arc::new(Image {
        width: decoded.width,
        height: decoded.height,
        rgba: decoded.rgba,
    })
}

fn compile(entry: &str, modules: &[(&str, String)]) -> Program {
    program::compile(&Sources {
        entry: entry.into(),
        modules: modules
            .iter()
            .map(|(name, text)| ((*name).to_owned(), text.clone()))
            .collect(),
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}

fn source(text: &str) -> Program {
    compile("main.ms", &[("main.ms", text.to_owned())])
}

/// Deterministic samples that differ in every input a program can read: positions, normals of
/// the three dominant axes, footprints, UVs and bounds.
fn samples(count: usize) -> Vec<Sample> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..count)
        .map(|i| {
            let normal = [[0., 0., 1.], [0., -1., 0.], [1., 0., 0.], [0.6, 0., 0.8]][i % 4];
            let world = [next() * 1.2 - 0.6, next() * 0.22 - 0.11, next() * 1.1];
            Sample {
                uv: [next(), next()],
                surface: [world[0], world[2]],
                object: world,
                world,
                normal,
                footprint: 0.001 + next() * 0.01,
                uv_footprint: Some(0.002 + next() * 0.01),
                gravity: [0., 0., -1.],
                bounds_min: [-0.6, -0.11, 0.],
                bounds_max: [0.6, 0.11, 1.1],
            }
        })
        .collect()
}

fn bits(outputs: &Outputs) -> Vec<u64> {
    let mut all: Vec<f64> = outputs.base_color.to_vec();
    all.extend([
        outputs.roughness,
        outputs.metalness,
        outputs.height,
        outputs.opacity,
    ]);
    all.extend(outputs.emissive);
    all.extend(outputs.receiver_normal.unwrap_or([f64::NAN; 3]));
    all.extend(outputs.normal_offset.unwrap_or([f64::NAN; 3]));
    all.into_iter().map(f64::to_bits).collect()
}

fn field_bits(values: &[FieldValue]) -> Vec<u64> {
    values
        .iter()
        .flat_map(|value| match value {
            FieldValue::Scalar(v) => vec![*v],
            FieldValue::Vec2(v) => v.to_vec(),
            FieldValue::Vec3(v) => v.to_vec(),
            FieldValue::Bool(v) => vec![f64::from(u8::from(*v))],
        })
        .map(f64::to_bits)
        .collect()
}

/// Every output and every field of `program`, for `samples`, together and one at a time.
fn assert_same(
    program: &Program,
    inputs: &Inputs,
    features: &[program::Feature],
    samples: &[Sample],
    outputs: OutputSet,
    together: bool,
) {
    let names: Vec<&str> = program
        .metadata()
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    let selection = program.select_fields(&names).unwrap();
    let mut row: Evaluator = program.evaluator_with_features(inputs, features).unwrap();
    let mut alone: Evaluator = program.evaluator_with_features(inputs, features).unwrap();
    let mut row_fields = Vec::new();
    let found = row.evaluate_row_together(samples, outputs, &selection, &mut row_fields);
    assert_eq!(found.is_some(), together, "evaluated together");
    let row_results = match found {
        Some(found) => found,
        None => row
            .evaluate_row(samples, outputs, &selection, &mut row_fields)
            .unwrap_or_else(|e| panic!("{e}")),
    };
    let slots = selection.len();
    for (i, sample) in samples.iter().enumerate() {
        let mut values = vec![FieldValue::Scalar(0.); slots];
        // Fresh allowance per sample: a row has one allowance, and the comparison is of values.
        alone.replenish(64_000_000);
        let expected = alone
            .evaluate_outputs(sample, outputs, &selection, &mut values)
            .unwrap_or_else(|e| panic!("sample {i}: {e}"));
        assert_eq!(
            bits(&row_results[i]),
            bits(&expected),
            "outputs of sample {i}"
        );
        assert_eq!(
            field_bits(&row_fields[i * slots..(i + 1) * slots]),
            field_bits(&values),
            "fields of sample {i}"
        );
    }
}

fn kowloon_features() -> Vec<program::Feature> {
    // A box's creases, as a receiver bake would derive them.
    let (min, max) = ([-0.6, -0.11, 0.], [0.6, 0.11, 1.1]);
    let positions: Vec<f64> = (0..8)
        .flat_map(|corner| {
            [
                [min[0], max[0]][corner & 1],
                [min[1], max[1]][(corner >> 1) & 1],
                [min[2], max[2]][(corner >> 2) & 1],
            ]
        })
        .collect();
    let indices: Vec<u32> = vec![
        0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2, 6, 0, 6, 4,
        1, 5, 7, 1, 7, 3,
    ];
    program::receiver_features(&positions, &indices)
}

#[test]
fn rows_match_per_sample_evaluation_on_the_example_programs() {
    let noise = read("kowloon-noise.material.ts");
    let studies: Vec<(&str, Program, Vec<&str>)> = vec![
        (
            "kowloon-concrete",
            compile(
                "kowloon-concrete.material.ts",
                &[
                    ("kowloon-noise.material.ts", noise.clone()),
                    (
                        "kowloon-concrete.material.ts",
                        read("kowloon-concrete.material.ts"),
                    ),
                ],
            ),
            vec![
                "textures/concrete-plain.png",
                "textures/concrete-fine-sand.png",
                "superspec-material:building-plaster-plain.png",
            ],
        ),
        (
            "kowloon-rusted-steel",
            compile(
                "kowloon-rusted-steel.material.ts",
                &[
                    ("kowloon-noise.material.ts", noise),
                    (
                        "kowloon-rusted-steel.material.ts",
                        read("kowloon-rusted-steel.material.ts"),
                    ),
                ],
            ),
            vec!["superspec-material:rust.png"],
        ),
        (
            "tiled-courtyard",
            compile(
                "main.material.ts",
                &[("main.material.ts", read("tiled-courtyard.material.ts"))],
            ),
            vec![],
        ),
        (
            "painted-metal",
            compile(
                "main.material.ts",
                &[
                    ("main.material.ts", read("painted-metal.material.ts")),
                    ("common.material.ts", read("common.material.ts")),
                ],
            ),
            vec![],
        ),
    ];
    for (name, program, images) in studies {
        let inputs = Inputs {
            seed: 7,
            images: images.iter().map(|n| ((*n).to_owned(), image(n))).collect(),
            edges: vec![Edge {
                start: [-0.5, 0., 1.],
                end: [0.5, 0., 1.],
            }],
            ..Inputs::default()
        };
        let features = kowloon_features();
        // More than one row's worth, and a partial last row.
        let samples = samples(150);
        eprintln!("{name}");
        assert_same(&program, &inputs, &features, &samples, OutputSet::ALL, true);
        let with_features = Inputs {
            features: features.clone(),
            ..inputs.clone()
        };
        for count in [1, 40, 64, 150] {
            assert!(agree(
                &program,
                &with_features,
                &ordered(count),
                OutputSet::ALL,
                name
            ));
        }
        // An allowance one short of what the row takes declines, and one sample at a time fails.
        let row = ordered(70);
        let selection = FieldSelection::default();
        let mut unused = Vec::new();
        let mut full = program.evaluator(&with_features).unwrap();
        full.evaluate_row_together(&row, OutputSet::ALL, &selection, &mut unused)
            .unwrap();
        let taken = 64_000_000 - full.allowance();
        let mut exact = program
            .evaluator_with_budget(&with_features, taken)
            .unwrap();
        assert!(
            exact
                .evaluate_row_together(&row, OutputSet::ALL, &selection, &mut unused)
                .is_some()
        );
        assert_eq!(exact.allowance(), 0);
        let mut short = program
            .evaluator_with_budget(&with_features, taken - 1)
            .unwrap();
        assert!(
            short
                .evaluate_row_together(&row, OutputSet::ALL, &selection, &mut unused)
                .is_none()
        );
        assert_eq!(
            short
                .evaluate_row(&row, OutputSet::ALL, &selection, &mut unused)
                .unwrap_err()
                .code,
            "MS_LIMIT"
        );
        // A subset of outputs, as a map's bake asks for.
        assert_same(
            &program,
            &inputs,
            &features,
            &samples[..70],
            OutputSet::ROUGHNESS.union(OutputSet::HEIGHT),
            true,
        );
    }
}

#[test]
fn branches_fields_and_helpers_stay_lazy_per_sample() {
    // The untaken branch divides by zero or takes the root of a negative number; a field read in
    // only some samples is computed for just those, and read again by others later.
    let program = source(
        "material 1;
         param flip: bool = false;
         fn same(x: scalar) -> scalar = x;
         fn shifted(p: vec2) -> vec2 = p + vec2(same(p.x), 1);
         let guarded = select(world.x > 0.1, sqrt(world.x), 0);
         let inverse = select(world.x > 0.05, 1 / (world.x - 0.05), 0.5);
         let heavy = noise(world * 3, seed(\"h\"));
         let unused = select(flip, 1 / (world.x * 0), 1);
         let both = select(uv.x > 0.3 && world.z < 1 || uv.y > 0.9, heavy, 0.2);
         output base_color = vec3(guarded, clamp(inverse, 0, 1), both);
         output roughness = select(uv.x > 0.6, heavy, 0.3) * unused;
         output metalness = select(world.x < 0 && uv.x > 0.2, 1, 0);
         output height = shifted(uv).x * 0.001;",
    );
    let samples = samples(200);
    assert_same(
        &program,
        &Inputs::default(),
        &[],
        &samples,
        OutputSet::ALL,
        true,
    );
    // A parameter changes which branches every sample takes.
    let mut inputs = Inputs::default();
    inputs
        .parameters
        .insert("flip".into(), serde_json::json!(false));
    assert_same(&program, &inputs, &[], &samples[..65], OutputSet::ALL, true);
}

#[test]
fn a_failing_sample_makes_the_row_evaluate_alone_and_report_the_same_error() {
    let program = source(
        "material 1;
         output base_color = vec3(clamp(abs(1 / (world.x - 0.5)) * 0.05, 0, 1), 0, 0);
         output roughness = 0.5;",
    );
    let mut samples = samples(100);
    for sample in &mut samples {
        sample.world[0] = 0.2;
    }
    samples[37].world[0] = 0.5;
    samples[80].world[0] = 0.5;
    let inputs = Inputs::default();
    let selection = FieldSelection::default();
    let mut row = program.evaluator(&inputs).unwrap();
    let mut values = Vec::new();
    assert!(
        row.evaluate_row_together(&samples, OutputSet::ALL, &selection, &mut values)
            .is_none(),
        "the row declines"
    );
    let together = row
        .evaluate_row(&samples, OutputSet::ALL, &selection, &mut values)
        .unwrap_err();
    // Evaluating one sample at a time stops at the first failing sample with its diagnostic.
    let mut alone = program.evaluator(&inputs).unwrap();
    let mut expected = None;
    for sample in &samples {
        if let Err(error) = alone.evaluate_outputs(sample, OutputSet::ALL, &selection, &mut []) {
            expected = Some(error);
            break;
        }
    }
    let expected = expected.expect("one sample fails");
    assert_eq!(together.to_string(), expected.to_string());
    assert_eq!(together.code, expected.code);
    // The evaluator recovers: the same rows without the failing samples evaluate.
    let mut fine = samples.clone();
    fine.retain(|s| s.world[0] != 0.5);
    assert!(
        row.evaluate_row_together(&fine, OutputSet::ALL, &selection, &mut values)
            .is_some()
    );
    // Invalid samples decline too, with the usual diagnostic.
    let mut bad = samples.clone();
    bad.retain(|s| s.world[0] != 0.5);
    bad[10].footprint = -1.;
    assert!(
        row.evaluate_row_together(&bad, OutputSet::ALL, &selection, &mut values)
            .is_none()
    );
    let error = row
        .evaluate_row(&bad, OutputSet::ALL, &selection, &mut values)
        .unwrap_err();
    assert_eq!(error.code, "MS_INPUT");
}

#[test]
fn programs_whose_bounds_could_reach_a_limit_are_evaluated_one_sample_at_a_time() {
    // The expensive branch is never taken by these samples, but its cost is bounded statically.
    let program = source(
        "material 1;
         fn l1(x: scalar) -> scalar = x + x + x + x + x + x + x + x + x + x;
         fn l2(x: scalar) -> scalar = l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x) + l1(x);
         fn l3(x: scalar) -> scalar = l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x) + l2(x);
         fn l4(x: scalar) -> scalar = l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x) + l3(x);
         output base_color = vec3(select(world.x > 100, l4(world.x), 0.25));
         output roughness = 0.5;",
    );
    let samples = samples(80);
    assert_same(
        &program,
        &Inputs::default(),
        &[],
        &samples,
        OutputSet::ALL,
        false,
    );
    // And a program whose allowance for the row cannot be bounded is evaluated alone as well.
    let inputs = Inputs::default();
    let mut evaluator = program.evaluator_with_budget(&inputs, 10).unwrap();
    let selection = FieldSelection::default();
    let mut values = Vec::new();
    assert!(
        evaluator
            .evaluate_row_together(&samples, OutputSet::ALL, &selection, &mut values)
            .is_none()
    );
}

/// A small deterministic generator of scalar expressions over the sample inputs, parameters,
/// earlier fields and helpers, with lazy branches, comparisons, vectors and noise.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn number(&mut self) -> String {
        format!("{:.3}", (self.below(2000) as f64 - 700.) / 400.)
    }
    fn leaf(&mut self, fields: usize) -> String {
        match self.below(9) {
            0 => self.number(),
            1 => "uv.x".into(),
            2 => "uv.y".into(),
            3 => "world.x".into(),
            4 => "world.z".into(),
            5 => "(surface.x * 3)".into(),
            6 => ["p0", "p1"][self.below(2) as usize].into(),
            7 if fields > 0 => format!("f{}", self.below(fields as u64)),
            _ => "footprint".into(),
        }
    }
    fn condition(&mut self, depth: u32, fields: usize) -> String {
        let (a, b) = (self.scalar(depth, fields), self.scalar(depth, fields));
        let operator = ["<", ">", "<=", ">="][self.below(4) as usize];
        let first = format!("{a} {operator} {b}");
        match self.below(5) {
            0 => {
                let (c, d) = (self.scalar(depth, fields), self.scalar(depth, fields));
                format!("({first}) && ({c} > {d})")
            }
            1 => {
                let (c, d) = (self.scalar(depth, fields), self.scalar(depth, fields));
                format!("({first}) || ({c} < {d})")
            }
            2 => format!("!({first})"),
            _ => first,
        }
    }
    fn scalar(&mut self, depth: u32, fields: usize) -> String {
        if depth == 0 {
            return self.leaf(fields);
        }
        let d = depth - 1;
        match self.below(25) {
            0 | 1 => format!("({} + {})", self.scalar(d, fields), self.scalar(d, fields)),
            2 => format!("({} - {})", self.scalar(d, fields), self.scalar(d, fields)),
            3 | 4 => format!("({} * {})", self.scalar(d, fields), self.scalar(d, fields)),
            5 => format!(
                "({} / (abs({}) + 0.25))",
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            6 => format!(
                "min({}, {})",
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            7 => format!(
                "max({}, {})",
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            8 => format!("clamp({}, -2, 3)", self.scalar(d, fields)),
            9 => format!("smoothstep(0.1, 0.9, {})", self.scalar(d, fields)),
            10 => format!(
                "mix({}, {}, {})",
                self.scalar(d, fields),
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            11 => format!("sin({})", self.scalar(d, fields)),
            12 => format!("cos({})", self.scalar(d, fields)),
            13 => format!("fract({})", self.scalar(d, fields)),
            14 => format!("floor({})", self.scalar(d, fields)),
            15 => format!("sqrt(abs({}))", self.scalar(d, fields)),
            16 => format!("pow(abs({}) + 0.01, 1.7)", self.scalar(d, fields)),
            17 => format!(
                "value_noise(vec2({}, {}), seed(\"k{}\"))",
                self.scalar(d, fields),
                self.scalar(d, fields),
                self.below(3)
            ),
            18 => format!(
                "select({}, {}, {})",
                self.condition(d, fields),
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            19 => format!(
                "h{}({}, {})",
                self.below(2),
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
            22 => format!("sqrt({})", self.scalar(d, fields)),
            23 => format!("(1 / {})", self.scalar(d, fields)),
            20 => format!(
                "length(vec2({}, {}) * {})",
                self.scalar(d, fields),
                self.scalar(d, fields),
                self.number()
            ),
            _ => format!(
                "dot(vec2({}, {}), vec2({}, {}))",
                self.scalar(d, fields),
                self.scalar(d, fields),
                self.scalar(d, fields),
                self.scalar(d, fields)
            ),
        }
    }
    fn program(&mut self) -> String {
        let mut text = String::from(
            "material 1;
param p0: scalar = 0.3 range(-1, 2);
param p1: scalar = 0.7 range(-1, 2);
fn h0(x: scalar, y: scalar) -> scalar = x * y + sin(x);
fn h1(x: scalar, y: scalar) -> scalar = select(x > y, x - y, h0(y, x) * 0.5);
",
        );
        let fields = 4;
        for i in 0..fields {
            let expression = self.scalar(3, i);
            text += &format!("let f{i} = {expression};\n");
        }
        for (name, scale) in [
            ("roughness", "0.2"),
            ("metalness", "0.1"),
            ("height", "0.001"),
        ] {
            let expression = self.scalar(4, fields);
            text += &match name {
                "height" => format!("output height = {expression} * {scale};\n"),
                _ => format!("output {name} = clamp({expression} * {scale} + 0.5, 0, 1);\n"),
            };
        }
        let (r, g, b) = (
            self.scalar(3, fields),
            self.scalar(3, fields),
            self.scalar(4, fields),
        );
        text += &format!(
            "output base_color = vec3(clamp({r} * 0.2 + 0.5, 0, 1), clamp({g} * 0.2 + 0.5, 0, 1), clamp({b} * 0.2 + 0.5, 0, 1));\n"
        );
        text
    }
}

#[test]
fn generated_programs_give_the_same_results_together_and_alone() {
    let mut generator = Generator(0x9E37_79B9_7F4A_7C15);
    let (mut agreed, mut failed, mut together) = (0, 0, 0);
    for case in 0..400 {
        let text = generator.program();
        let program = match program::compile(&Sources {
            entry: "main.ms".into(),
            modules: [("main.ms".to_owned(), text.clone())].into(),
        }) {
            Ok(program) => program,
            Err(errors) => panic!("case {case}: {errors:?}\n{text}"),
        };
        let inputs = Inputs {
            seed: case,
            ..Inputs::default()
        };
        let selection = program.select_fields(&["f1", "f3"]).unwrap();
        let samples = samples(70);
        let mut row = program.evaluator(&inputs).unwrap();
        let mut row_fields = Vec::new();
        let attempted =
            row.evaluate_row_together(&samples, OutputSet::ALL, &selection, &mut row_fields);
        together += usize::from(attempted.is_some());
        let result = row.evaluate_row(&samples, OutputSet::ALL, &selection, &mut row_fields);
        let mut alone = program.evaluator(&inputs).unwrap();
        let mut expected = Vec::new();
        let mut expected_fields = Vec::new();
        let mut failure = None;
        for sample in &samples {
            let mut values = vec![FieldValue::Scalar(0.); selection.len()];
            match alone.evaluate_outputs(sample, OutputSet::ALL, &selection, &mut values) {
                Ok(outputs) => {
                    expected.push(outputs);
                    expected_fields.extend(values);
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        match (result, failure) {
            (Ok(found), None) => {
                agreed += 1;
                assert_eq!(found.len(), expected.len(), "case {case}\n{text}");
                for (i, (a, b)) in found.iter().zip(&expected).enumerate() {
                    assert_eq!(bits(a), bits(b), "case {case} sample {i}\n{text}");
                }
                assert_eq!(
                    field_bits(&row_fields),
                    field_bits(&expected_fields),
                    "case {case}\n{text}"
                );
            }
            (Err(a), Some(b)) => {
                failed += 1;
                assert_eq!(a.to_string(), b.to_string(), "case {case}\n{text}");
                assert!(
                    attempted.is_none(),
                    "a failing row must decline: case {case}"
                );
            }
            (Ok(_), Some(b)) => {
                panic!("case {case}: together succeeded, alone failed with {b}\n{text}")
            }
            (Err(a), None) => {
                panic!("case {case}: together failed with {a}, alone succeeded\n{text}")
            }
        }
    }
    eprintln!("agreed {agreed}, both failed {failed}, evaluated together {together}");
    assert!(
        agreed > 100,
        "too few generated programs evaluate: {agreed}"
    );
    assert!(together > 100, "too few rows evaluate together: {together}");
}

/// Samples in order along a chart row: `uv.x`, `world.x` and `surface.x` rise with the index.
fn ordered(count: usize) -> Vec<Sample> {
    (0..count)
        .map(|i| {
            let x = (i as f64 + 0.5) / count as f64;
            let world = [x * 1.2 - 0.6, 0.05, 0.3 + 0.4 * x];
            Sample {
                uv: [x, 0.25 + 0.5 * x],
                surface: [world[0], world[2]],
                object: world,
                world,
                normal: [0., 0., 1.],
                footprint: 0.004,
                uv_footprint: Some(0.004),
                gravity: [0., 0., -1.],
                bounds_min: [-0.6, -0.11, 0.],
                bounds_max: [0.6, 0.11, 1.1],
            }
        })
        .collect()
}

/// Evaluates the row both ways and requires equal results, or the same error. Returns whether
/// the row was evaluated together.
fn agree(
    program: &Program,
    inputs: &Inputs,
    samples: &[Sample],
    outputs: OutputSet,
    what: &str,
) -> bool {
    let names: Vec<&str> = program
        .metadata()
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    let selection = program.select_fields(&names).unwrap();
    let slots = selection.len();
    let mut row_fields = Vec::new();
    let attempted = program
        .evaluator(inputs)
        .unwrap()
        .evaluate_row_together(samples, outputs, &selection, &mut row_fields)
        .is_some();
    let mut row = program.evaluator(inputs).unwrap();
    let result = row.evaluate_row(samples, outputs, &selection, &mut row_fields);
    let mut alone = program.evaluator(inputs).unwrap();
    let mut expected = Vec::new();
    let mut expected_fields = Vec::new();
    let mut failure = None;
    for sample in samples {
        let mut values = vec![FieldValue::Scalar(0.); slots];
        match alone.evaluate_outputs(sample, outputs, &selection, &mut values) {
            Ok(found) => {
                expected.push(found);
                expected_fields.extend(values);
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    match (result, failure) {
        (Ok(found), None) => {
            for (i, (a, b)) in found.iter().zip(&expected).enumerate() {
                assert_eq!(bits(a), bits(b), "{what}: outputs of sample {i}");
            }
            assert_eq!(
                field_bits(&row_fields),
                field_bits(&expected_fields),
                "{what}: fields"
            );
            // Both ways take the same from the batch allowance, so where it runs out is the same.
            assert_eq!(row.allowance(), alone.allowance(), "{what}: allowance left");
        }
        (Err(a), Some(b)) => {
            assert_eq!(a.to_string(), b.to_string(), "{what}");
            assert!(!attempted, "{what}: a failing row must decline");
        }
        (Ok(_), Some(b)) => panic!("{what}: together succeeded, alone failed with {b}"),
        (Err(a), None) => panic!("{what}: together failed with {a}, alone succeeded"),
    }
    attempted
}

const SUBSETS: [OutputSet; 5] = [
    OutputSet::ALL,
    OutputSet::ROUGHNESS,
    OutputSet::HEIGHT,
    OutputSet::BASE_COLOR,
    OutputSet::BASE_COLOR.union(OutputSet::HEIGHT),
];

#[test]
fn a_field_some_samples_read_first_is_not_taken_for_the_same_everywhere() {
    // `f` steps at uv.x = 0.5. Color reads it only below 0.25, where every sample takes one
    // branch; roughness and height read it everywhere afterwards.
    let program = source(
        "material 1;
         let f = select(uv.x < 0.5, 1.0, 2.0);
         let g = select(uv.x < 0.75 && uv.x > 0.1, f, 3.0);
         output base_color = vec3(select(uv.x < 0.25, f, 0.0) * 0.1, select(uv.x > 0.9, g, 0.0) * 0.1, 0.2);
         output roughness = f * 0.25;
         output metalness = g * 0.1;
         output height = f * 0.001 + g * 0.002;",
    );
    let inputs = Inputs::default();
    for count in [1, 3, 8, 63, 64, 65, 128, 200] {
        let samples = ordered(count);
        for outputs in SUBSETS {
            assert!(agree(
                &program,
                &inputs,
                &samples,
                outputs,
                &format!("{count} samples")
            ));
        }
        // The value of one output does not depend on which others were evaluated with it.
        let mut evaluator = program.evaluator(&inputs).unwrap();
        let selection = FieldSelection::default();
        let mut unused = Vec::new();
        let all = evaluator
            .evaluate_row(&samples, OutputSet::ALL, &selection, &mut unused)
            .unwrap();
        let only = evaluator
            .evaluate_row(&samples, OutputSet::HEIGHT, &selection, &mut unused)
            .unwrap();
        for (i, (a, b)) in all.iter().zip(&only).enumerate() {
            assert_eq!(
                a.height.to_bits(),
                b.height.to_bits(),
                "height of sample {i} of {count}"
            );
            assert_eq!(a.roughness, if samples[i].uv[0] < 0.5 { 0.25 } else { 0.5 });
        }
    }
}

#[test]
fn kept_planes_hold_each_sample_s_own_value_and_only_change_what_is_computed() {
    let program = source(
        "material 1;
         let f = select(uv.x < 0.5, 1.0, 2.0);
         let n = noise(vec2(uv.x * 9, uv.y * 7), seed(\"n\")) * f;
         output base_color = vec3(select(uv.x < 0.25, f, 0.0) * 0.1, select(uv.x < 0.6, n, 0.0) * 0.1, 0.2);
         output roughness = f * 0.25 + n * 0.1;",
    );
    let inputs = Inputs::default();
    let index = |name: &str| {
        program
            .metadata()
            .fields
            .iter()
            .position(|f| f.name == name)
            .unwrap()
    };
    let selection = FieldSelection::default();
    for count in [5, 64, 150] {
        let samples = ordered(count);
        let texels: Vec<usize> = (0..count).map(|i| i * 2 + 1).collect();
        let mut evaluator = program.evaluator(&inputs).unwrap();
        let mut unused = Vec::new();
        // Color reads each field for only some samples; the planes keep exactly those.
        let mut planes = vec![
            program::FieldPlane::new(index("f"), 1, count * 2 + 1),
            program::FieldPlane::new(index("n"), 1, count * 2 + 1),
        ];
        evaluator
            .evaluate_row_with_planes(
                &samples,
                &texels,
                OutputSet::BASE_COLOR,
                &selection,
                &mut unused,
                &[],
                &mut planes,
            )
            .unwrap();
        // Reading them back, every output equals the one computed without planes.
        let kept: Vec<&program::FieldPlane> = planes.iter().collect();
        for outputs in [OutputSet::ROUGHNESS, OutputSet::ALL, OutputSet::BASE_COLOR] {
            let with = evaluator
                .evaluate_row_with_planes(
                    &samples,
                    &texels,
                    outputs,
                    &selection,
                    &mut unused,
                    &kept,
                    &mut [],
                )
                .unwrap();
            let mut alone = program.evaluator(&inputs).unwrap();
            for (i, sample) in samples.iter().enumerate() {
                let expected = alone
                    .evaluate_outputs(sample, outputs, &selection, &mut [])
                    .unwrap();
                assert_eq!(bits(&with[i]), bits(&expected), "sample {i} of {count}");
            }
        }
    }
}

impl Generator {
    /// A value in [0, 1.5]: a number, a parameter, an earlier field or arithmetic that stays bounded.
    fn step_value(&mut self, fields: usize, depth: u32) -> String {
        match self.below(if depth == 0 { 4 } else { 8 }) {
            0 | 1 if fields > 0 => format!("f{}", self.below(fields as u64)),
            0..=2 => format!("{:.2}", self.below(100) as f64 / 100.),
            3 => "p0".into(),
            4 => format!(
                "min({}, {})",
                self.step_value(fields, depth - 1),
                self.step_value(fields, depth - 1)
            ),
            5 => format!(
                "({} * 0.5 + {} * 0.5)",
                self.step_value(fields, depth - 1),
                self.step_value(fields, depth - 1)
            ),
            _ => self.step(fields, depth - 1),
        }
    }
    fn threshold(&mut self) -> String {
        let axis = ["uv.x", "uv.x", "uv.x", "uv.y", "world.x"][self.below(5) as usize];
        let operator = ["<", ">", "<=", ">="][self.below(4) as usize];
        let limit = (self.below(17) as f64 - 2.) / 12.;
        match self.below(6) {
            0 => format!(
                "{axis} {operator} {limit:.3} && uv.x > {:.3}",
                self.below(10) as f64 / 10.
            ),
            1 => format!(
                "{axis} {operator} {limit:.3} || uv.x > {:.3}",
                self.below(10) as f64 / 10.
            ),
            2 if true => format!("!({axis} {operator} {limit:.3})"),
            _ => format!("{axis} {operator} {limit:.3}"),
        }
    }
    /// A `select` on a threshold of the sample, over values that may be shared fields or selects.
    fn step(&mut self, fields: usize, depth: u32) -> String {
        format!(
            "select({}, {}, {})",
            self.threshold(),
            self.step_value(fields, depth),
            self.step_value(fields, depth)
        )
    }
    fn stepped_program(&mut self) -> String {
        let mut text = String::from(
            "material 1;\nparam p0: scalar = 0.3 range(-1, 2);\nfn pick(x: scalar, y: scalar) -> scalar = select(x > y, x, y * 0.5);\n",
        );
        let fields = 5;
        for i in 0..fields {
            let body = if self.below(4) == 0 && i > 0 {
                format!("pick({}, {})", self.step_value(i, 1), self.step_value(i, 1))
            } else {
                self.step(i, 2)
            };
            text += &format!("let f{i} = {body};\n");
        }
        let r = self.step(fields, 1);
        let g = self.step(fields, 2);
        text += &format!(
            "output base_color = vec3(clamp({r} * 0.5, 0, 1), clamp({g} * 0.5, 0, 1), 0.2);\n"
        );
        text += &format!(
            "output roughness = clamp({} * 0.5, 0, 1);\n",
            self.step(fields, 1)
        );
        text += &format!(
            "output metalness = clamp({} * 0.5, 0, 1);\n",
            self.step_value(fields, 2)
        );
        text += &format!("output height = {} * 0.001;\n", self.step(fields, 2));
        text
    }
}

#[test]
fn generated_steps_over_shared_fields_agree_on_ordered_rows_of_every_size() {
    let mut generator = Generator(0x1234_5678_9ABC_DEF1);
    let mut together = 0;
    for case in 0..300 {
        let text = generator.stepped_program();
        let program = program::compile(&Sources {
            entry: "main.ms".into(),
            modules: [("main.ms".to_owned(), text.clone())].into(),
        })
        .unwrap_or_else(|e| panic!("case {case}: {e:?}\n{text}"));
        let inputs = Inputs::default();
        for count in [1, 6, 33, 64, 65, 190] {
            let samples = ordered(count);
            for outputs in SUBSETS {
                together += usize::from(agree(
                    &program,
                    &inputs,
                    &samples,
                    outputs,
                    &format!("case {case}, {count} samples\n{text}"),
                ));
            }
        }
        // Unordered samples of the earlier generator too.
        agree(
            &program,
            &inputs,
            &samples(70),
            OutputSet::ALL,
            &format!("case {case} unordered\n{text}"),
        );
    }
    assert!(together > 8000, "rows must evaluate together: {together}");
}
