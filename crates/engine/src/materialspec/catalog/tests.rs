use super::*;
use std::collections::BTreeSet;

const KINDS: [Type; 5] = [Scalar, Vec2, Vec3, Bool, Text];

/// The hand-written typing the checker used before the catalog existed. It is kept here as
/// an independent expectation for every argument combination.
fn legacy(name: &str, args: &[Type]) -> Option<Type> {
    fn numeric(t: Type) -> bool {
        matches!(t, Scalar | Vec2 | Vec3)
    }
    fn broadcast(a: Type, b: Type) -> Option<Type> {
        if numeric(a) && numeric(b) {
            if a == b || b == Scalar {
                Some(a)
            } else if a == Scalar {
                Some(b)
            } else {
                None
            }
        } else {
            None
        }
    }
    match (name, args) {
        ("vec2", [Scalar]) | ("vec2", [Scalar, Scalar]) | ("vec2", [Vec2]) => Some(Vec2),
        ("vec3", [Scalar])
        | ("vec3", [Scalar, Scalar, Scalar])
        | ("vec3", [Vec2, Scalar])
        | ("vec3", [Vec3]) => Some(Vec3),
        ("seed", [Text]) => Some(Scalar),
        ("noise" | "cellular", [Vec2 | Vec3, Scalar]) | ("fbm", [Vec2 | Vec3, Scalar, Scalar]) => {
            Some(Scalar)
        }
        ("sample_color" | "sample_data", [Text, Vec2]) => Some(Vec3),
        ("sample_alpha", [Text, Vec2]) | ("runoff", [Scalar, Scalar]) => Some(Scalar),
        ("rotate", [Vec2, Scalar]) => Some(Vec2),
        ("select", [Bool, a, b]) if a == b && *a != Text => Some(*a),
        ("length", [Vec2 | Vec3]) => Some(Scalar),
        ("normalize", [a @ (Vec2 | Vec3)]) => Some(*a),
        ("dot", [a, b]) if a == b && matches!(a, Vec2 | Vec3) => Some(Scalar),
        ("abs" | "floor" | "fract" | "sin" | "cos" | "sqrt", [a]) if numeric(*a) => Some(*a),
        ("min" | "max" | "step", [a, b]) => broadcast(*a, *b),
        ("scale" | "offset" | "repeat" | "warp", [a, b])
            if matches!(a, Vec2 | Vec3) && (*b == Scalar || a == b) =>
        {
            Some(*a)
        }
        ("mix", [a, b, t]) if a == b && numeric(*a) && (*t == Scalar || t == a) => Some(*a),
        ("clamp", [a, b, c])
            if numeric(*a) && (*b == Scalar || b == a) && (*c == Scalar || c == a) =>
        {
            Some(*a)
        }
        ("smoothstep", [a, b, c])
            if numeric(*c) && (*a == Scalar || a == c) && (*b == Scalar || b == c) =>
        {
            Some(*c)
        }
        _ => None,
    }
}

fn combinations(length: usize) -> Vec<Vec<Type>> {
    (0..length).fold(vec![Vec::new()], |previous, _| {
        previous
            .into_iter()
            .flat_map(|prefix| {
                KINDS.iter().map(move |&kind| {
                    let mut next = prefix.clone();
                    next.push(kind);
                    next
                })
            })
            .collect()
    })
}

/// The built-in names the checker accepted before the catalog existed, in documentation order.
/// Iterating the catalog cannot notice a dropped name, so this list is pinned independently.
const ORIGINAL_BUILTINS: [&str; 31] = [
    "vec2",
    "vec3",
    "mix",
    "smoothstep",
    "clamp",
    "noise",
    "fbm",
    "cellular",
    "seed",
    "scale",
    "offset",
    "rotate",
    "repeat",
    "warp",
    "sample_color",
    "sample_data",
    "sample_alpha",
    "runoff",
    "select",
    "min",
    "max",
    "abs",
    "floor",
    "fract",
    "sin",
    "cos",
    "sqrt",
    "length",
    "normalize",
    "dot",
    "step",
];

#[test]
fn the_catalog_keeps_every_original_builtin_and_adds_only_receiver_queries() {
    let catalog: BTreeSet<_> = operations().iter().map(|operation| operation.id).collect();
    let original: BTreeSet<_> = ORIGINAL_BUILTINS.into_iter().collect();
    assert!(catalog.is_superset(&original));
    let added: BTreeSet<_> = catalog.difference(&original).copied().collect();
    assert_eq!(
        added,
        BTreeSet::from([
            "crease_distance",
            "crease_normal",
            "crease_tangent",
            "crease_adjacent_normal",
            "cross",
            "adjust_normal",
            "remap",
            "soft_threshold",
            "distance_band",
            "soft_union",
            "mask_intersect",
            "mask_subtract",
            "hash",
            "value_noise",
            "pow",
            "exp",
            "triplanar_weights",
            "varied_color",
            "external",
            "feature_distance",
            "feature_tangent",
            "alpha_cutout",
            "authored_mask",
            "ramp",
            "instance_attribute",
            "vertex_attribute",
            "shade_band",
            "sample_field",
        ])
    );
    for name in ORIGINAL_BUILTINS {
        assert!(find(name).is_some(), "{name}");
    }
    // Operators, groups and renderer operations are never callable `.ms` built-ins, so a `.ms`
    // program may still declare helpers with those names.
    for name in [
        "add",
        "mul",
        "tileAndGrout",
        "runoffStreaks",
        "grazingFalloff",
    ] {
        assert!(find(name).is_none(), "{name}");
    }
}

#[test]
fn catalog_lists_the_documented_builtins_once() {
    let names: Vec<_> = operations().iter().map(|operation| operation.id).collect();
    assert_eq!(names.len(), 59);
    assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 59);
    for operation in operations() {
        assert_eq!(operation.version, 1, "{}", operation.id);
        assert!(!operation.overloads.is_empty(), "{}", operation.id);
        assert!(!operation.example.is_empty(), "{}", operation.id);
        assert!(!operation.summary.is_empty(), "{}", operation.id);
        // Examples are shown to recipe authors, so they use recipe spelling: camelCase names and
        // operator functions, never `.ms` snake_case or infix operators.
        assert!(
            operation.example.contains(&ts_name(operation.id)),
            "{} example should demonstrate the operation: {}",
            operation.id,
            operation.example
        );
        for infix in [" / ", " * ", " + ", " - ", " > ", " < ", "sample_"] {
            assert!(
                !operation.example.contains(infix),
                "{} example uses {infix:?}: {}",
                operation.id,
                operation.example
            );
        }
    }
}

#[test]
fn overload_resolution_matches_the_previous_checker_for_every_argument_combination() {
    for operation in operations()
        .iter()
        .filter(|op| ORIGINAL_BUILTINS.contains(&op.id))
    {
        for length in 0..=3 {
            for args in combinations(length) {
                assert_eq!(
                    resolve(operation.id, &args),
                    legacy(operation.id, &args),
                    "{}({args:?})",
                    operation.id
                );
            }
        }
    }
    assert_eq!(resolve("unknown", &[Scalar]), None);
}

#[test]
fn overloads_are_unique_and_listed_overloads_are_all_accepted_by_the_previous_checker() {
    for operation in operations()
        .iter()
        .filter(|op| ORIGINAL_BUILTINS.contains(&op.id))
    {
        for (index, overload) in operation.overloads.iter().enumerate() {
            assert_eq!(
                legacy(operation.id, &overload.args),
                Some(overload.result),
                "{} overload {index}",
                operation.id
            );
            assert!(
                !operation.overloads[..index].contains(overload),
                "{} repeats an overload",
                operation.id
            );
        }
    }
}

#[test]
fn constant_literal_and_dependency_flags_match_the_documented_rules() {
    let constant: BTreeSet<_> = operations()
        .iter()
        .filter(|operation| operation.constant)
        .map(|operation| operation.id)
        .collect();
    assert_eq!(
        constant,
        BTreeSet::from([
            "vec2",
            "vec3",
            "mix",
            "clamp",
            "smoothstep",
            "min",
            "max",
            "abs",
            "floor",
            "fract",
            "sin",
            "cos",
            "sqrt",
            "select",
        ])
    );
    let literal: BTreeSet<_> = operations()
        .iter()
        .filter(|operation| operation.literal_first)
        .map(|operation| operation.id)
        .collect();
    assert_eq!(
        literal,
        BTreeSet::from([
            "seed",
            "sample_color",
            "sample_data",
            "sample_alpha",
            "crease_distance",
            "crease_normal",
            "crease_tangent",
            "crease_adjacent_normal",
            "remap",
            "varied_color",
            "external",
            "feature_distance",
            "feature_tangent",
            "authored_mask",
            "ramp",
            "instance_attribute",
            "vertex_attribute",
            "sample_field",
        ])
    );
    assert_eq!(find("sample_field").unwrap().requires, ["field"]);
    assert_eq!(find("sample_field").unwrap().literal_values, SCENE_FIELDS);
    assert_eq!(find("ramp").unwrap().literal_values, RAMP_MODES);
    assert_eq!(find("instance_attribute").unwrap().requires, ["instance"]);
    assert_eq!(find("vertex_attribute").unwrap().requires, ["vertex"]);
    assert_eq!(find("external").unwrap().requires, ["planes"]);
    for (kind, fallback) in [(Scalar, Scalar), (Vec2, Vec2), (Vec3, Vec3)] {
        assert_eq!(resolve("external", &[Text, fallback]), Some(kind));
    }
    assert_eq!(resolve("external", &[Text, Bool]), None);
    assert_eq!(resolve("external", &[Scalar, Scalar]), None);
    assert_eq!(find("seed").unwrap().requires, ["seed"]);
    assert_eq!(
        find("runoff").unwrap().requires,
        ["world", "normal", "gravity", "edges"]
    );
    let images: BTreeSet<_> = operations()
        .iter()
        .filter(|operation| operation.image_input)
        .map(|operation| operation.id)
        .collect();
    assert_eq!(
        images,
        BTreeSet::from([
            "sample_color",
            "sample_data",
            "sample_alpha",
            "varied_color",
            "authored_mask"
        ])
    );
    // Operations that read sample inputs declare them, so ports.json is a complete contract.
    for group in groups().iter().filter(|group| group.unavailable.is_none()) {
        for input in ["surface", "world", "normal", "footprint"] {
            let words = group
                .template
                .split(|c: char| !c.is_alphanumeric() && c != '_');
            // A template may declare a field with an input's name (Oxide's `surface` mask); that
            // local is renamed per instance and is not a read of the sample input.
            let declared = group.template.lines().any(|line| {
                line.trim_start()
                    .strip_prefix("let ")
                    .and_then(|rest| rest.split_whitespace().next())
                    == Some(input)
            });
            let reads = !declared && words.clone().any(|word| word == input);
            // Reading an input directly requires declaring it. The converse need not hold: a group
            // can need inputs through a built-in it calls (`runoff` needs world and edges).
            if reads {
                assert!(
                    group.requires.contains(&input),
                    "{} reads {input} but does not declare it",
                    group.id
                );
            }
        }
    }
}

#[test]
fn operator_typing_rejects_every_combination_the_table_does_not_list() {
    use crate::materialspec::program::{self, Sources};
    let compiles = |expression: &str| {
        program::compile(&Sources {
            entry: "probe.ms".into(),
            modules: [(
                "probe.ms".to_owned(),
                format!("material 1; let probe = {expression}; output base_color = vec3(0.5);"),
            )]
            .into(),
        })
        .is_ok()
    };
    for operator in operators() {
        let arity = if matches!(operator.id, "neg" | "not") {
            1
        } else {
            2
        };
        for args in combinations(arity) {
            if args.contains(&Text) {
                continue;
            }
            let texts: Vec<String> = args
                .iter()
                .map(|kind| argument(operator.id, *kind, 0))
                .collect();
            let expression = match texts.as_slice() {
                [only] => format!("{}({only})", operator.symbol),
                [a, b] => format!("({a}) {} ({b})", operator.symbol),
                _ => unreachable!(),
            };
            let listed = operator
                .overloads
                .iter()
                .any(|overload| overload.args == args);
            assert_eq!(
                compiles(&expression),
                listed,
                "{} {args:?}: {expression}",
                operator.id
            );
        }
    }
}

#[test]
fn duplicate_broadcast_overloads_are_removed_from_the_published_table() {
    let raw = build();
    let raw_mix = raw.iter().find(|operation| operation.id == "mix").unwrap();
    assert!(
        raw_mix.overloads.len() > find("mix").unwrap().overloads.len(),
        "the raw enumeration repeats scalar-only combinations"
    );
}

/// A literal argument of the given kind that satisfies each operation's domain rules.
fn argument(operation: &str, kind: Type, position: usize) -> String {
    let scalar = match (operation, position) {
        ("smoothstep", 0) | ("clamp", 1) | ("remap", 2) | ("distance_band", 1) => "0.2",
        ("smoothstep", 1) | ("clamp", 2) | ("remap", 3) | ("distance_band", 2) => "0.8",
        ("fbm", 2) => "3.0",
        _ => "0.5",
    };
    match kind {
        Scalar => scalar.into(),
        Vec2 => format!("vec2({scalar}, {scalar})"),
        Vec3 => format!("vec3({scalar}, {scalar}, {scalar})"),
        Bool => "true".into(),
        Text if operation.starts_with("crease_") => "\"exposed\"".into(),
        Text if operation == "remap" => "\"clamp\"".into(),
        Text if operation == "ramp" => "\"linear\"".into(),
        Text if operation.ends_with("_attribute") => "\"root\"".into(),
        Text if operation == "sample_field" => "\"ground.color\"".into(),
        Text => "\"img\"".into(),
    }
}

/// Reduces any result to a scalar so one program shape can probe every operation.
fn scalar_of(expression: String, result: Type) -> String {
    match result {
        Scalar => expression,
        Vec2 | Vec3 => format!("({expression}).x"),
        Bool => format!("select({expression}, 1.0, 0.0)"),
        Text => unreachable!("no operation returns a string"),
    }
}

#[test]
fn every_catalog_overload_compiles_and_evaluates() {
    use crate::materialspec::program::{self, Image, Inputs, Sample, Sources};
    let mut probes = Vec::new();
    // A cutout is only ever the whole opacity channel (see tests/material_cutout.rs).
    for operation in operations().iter().filter(|op| op.id != "alpha_cutout") {
        for overload in &operation.overloads {
            let args: Vec<String> = overload
                .args
                .iter()
                .enumerate()
                .map(|(position, kind)| argument(operation.id, *kind, position))
                .collect();
            probes.push((
                format!("{}({:?})", operation.id, overload.args),
                scalar_of(
                    format!("{}({})", operation.id, args.join(", ")),
                    overload.result,
                ),
            ));
        }
    }
    for operator in operators() {
        for overload in &operator.overloads {
            let args: Vec<String> = overload
                .args
                .iter()
                .enumerate()
                .map(|(position, kind)| argument(operator.id, *kind, position))
                .collect();
            let expression = match args.as_slice() {
                [only] => format!("{}({only})", operator.symbol),
                [a, b] => format!("({a}) {} ({b})", operator.symbol),
                _ => unreachable!("operators take one or two arguments"),
            };
            probes.push((
                format!("{}({:?})", operator.id, overload.args),
                scalar_of(expression, overload.result),
            ));
        }
    }
    assert!(probes.len() > 150, "{} probes", probes.len());
    let mut inputs = Inputs::default();
    inputs.images.insert(
        "img".into(),
        Image {
            width: 1,
            height: 1,
            rgba: vec![128, 64, 32, 255],
        }
        .into(),
    );
    for (label, expression) in probes {
        let text = format!("material 1; output base_color = vec3(clamp({expression}, 0.0, 1.0));");
        let program = program::compile(&Sources {
            entry: "probe.ms".into(),
            modules: [("probe.ms".to_owned(), text.clone())].into(),
        })
        .unwrap_or_else(|errors| panic!("{label}: {text}\n{errors:?}"));
        // Render-only inputs exist where the material is drawn; a CPU evaluation says so.
        if label.contains("_attribute") || label.starts_with("sample_field") {
            let error = program.evaluate(&inputs, &Sample::default()).unwrap_err();
            assert!(error.message.contains("render-only"), "{label}: {error}");
            continue;
        }
        let value = program
            .evaluate(&inputs, &Sample::default())
            .unwrap_or_else(|error| panic!("{label} failed to evaluate: {error}"));
        // Evaluation already rejects non-finite or out-of-range output; reaching here means it ran.
        let _ = value;
    }
}

#[test]
fn crease_distance_is_typed_with_a_selector_literal_and_two_scalars() {
    assert_eq!(
        resolve("crease_distance", &[Text, Scalar, Scalar]),
        Some(Scalar)
    );
    assert_eq!(resolve("crease_distance", &[Text, Scalar]), None);
    assert_eq!(resolve("crease_distance", &[Scalar, Scalar, Scalar]), None);
    let operation = find("crease_distance").unwrap();
    assert!(operation.literal_first && !operation.constant && !operation.image_input);
    assert_eq!(operation.literal_values, SELECTORS);
    assert_eq!(operation.requires, ["world", "features"]);
}
