//! Layered slots: a base and weathering layers composed as a chain of planes. The stack bakes
//! the bytes of the same appearance written as one program (the oracle below), reads each
//! channel from the material that last changed it, and an edit, an added or a removed layer
//! bakes only the materials it can change. The base's intrinsic outputs are kept by position of
//! its own domain, so a resized receiver reads them instead of evaluating them.
use super::*;
use crate::authoring::{Anchor, Layer, Mapping, Profile, Receiver, Request};

/// A grained base with relief.
const BASE: &str = "material 1;
param grit: scalar = 0.5 range(0, 1);
let grain = fbm(surface * 9, seed(\"g\"), 3);
output base_color = vec3(0.2 + 0.6 * grain, 0.3, 0.25);
output roughness = clamp(0.3 + grain * grit, 0, 1);
output height = 0.002 * grain;";

/// Grime that follows a feature the slot names: darker and rougher near it.
const GRIME: &str = r#"import { defineLayer, param, featureDistance, smoothstep, sub, mul, vec3 } from "@superspec/material/v1";
const amount = param.scalar({ default: 0.8, range: [0, 1] });
export default defineLayer({
  coverage: mul(amount, sub(1, smoothstep(0, 0.3, featureDistance("follow", 0.3)))),
  baseColor: vec3(0.1, 0.08, 0.06),
  roughness: 0.95,
  blend: { baseColor: "multiply", roughness: "max" },
});
"#;

/// Moss in patches, raised over the surface beneath and reading its height.
const MOSS: &str = r#"import { defineLayer, below, param, noise, seed, surface, smoothstep, mul, add, vec3 } from "@superspec/material/v1";
const spread = param.scalar({ default: 0.5, range: [0, 1] });
export default defineLayer({
  coverage: smoothstep(0.6, 0.7, add(noise(mul(surface, 12), seed("moss")), mul(spread, 0.2))),
  baseColor: vec3(0.07, 0.09, 0.02),
  height: add(0.0008, mul(below.height, 0.5)),
  blend: { height: "add" },
});
"#;

/// A stain that only darkens.
const STAIN: &str = r#"import { defineLayer, smoothstep, surface, sub, vec3 } from "@superspec/material/v1";
export default defineLayer({
  coverage: sub(1, smoothstep(0.1, 0.35, surface.y)),
  baseColor: vec3(0.6, 0.55, 0.5),
  blend: { baseColor: "multiply" },
});
"#;

/// The base, grime and moss written as one program: what the stack must bake.
fn oracle(grit: f64, amount: f64, spread: f64) -> String {
    format!(
        "material 1;
param grit: scalar = {grit:?} range(0, 1);
param amount: scalar = {amount:?} range(0, 1);
param spread: scalar = {spread:?} range(0, 1);
let grain = fbm(surface * 9, seed(\"g\"), 3);
let color0 = vec3(0.2 + 0.6 * grain, 0.3, 0.25);
let rough0 = clamp(0.3 + grain * grit, 0, 1);
let height0 = 0.002 * grain;
let cover1 = clamp(amount * (1 - smoothstep(0, 0.3, feature_distance(\"sill\", 0.3))), 0, 1);
let color1 = mix(color0, color0 * vec3(0.1, 0.08, 0.06), cover1);
let rough1 = mix(rough0, max(rough0, 0.95), cover1);
let cover2 = clamp(smoothstep(0.6, 0.7, noise(surface * 12, seed(\"moss\")) + spread * 0.2), 0, 1);
output base_color = mix(color1, vec3(0.07, 0.09, 0.02), cover2);
output roughness = rough1;
output height = mix(height0, height0 + (0.0008 + height0 * 0.5), cover2);"
    )
}

/// A sill across the swatch (which stands in the XZ plane), 0.4 m up.
fn sill() -> Vec<program::NamedFeature> {
    vec![program::NamedFeature {
        name: "sill".into(),
        points: vec![[-1., 0., 0.4], [2., 0., 0.4]],
        closed: false,
    }]
}

fn layer(entry: &str, parameters: &[(&str, f64)]) -> Layer {
    Layer {
        entry: entry.into(),
        parameters: parameters
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).into()))
            .collect(),
        seed: 7,
        follow: if entry == "grime.material.ts" {
            [("follow".to_owned(), "sill".into())].into()
        } else {
            BTreeMap::new()
        },
    }
}

fn modules() -> BTreeMap<String, String> {
    [
        ("grime.material.ts", GRIME),
        ("moss.material.ts", MOSS),
        ("stain.material.ts", STAIN),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .collect()
}

/// The base with `layers` over it on the swatch.
fn stack(grit: f64, layers: Vec<Layer>, resolution: usize) -> Request {
    Request {
        entry: "base.ms".into(),
        source: BASE.into(),
        modules: modules(),
        parameters: [("grit".to_owned(), grit.into())].into(),
        seed: 7,
        resolution,
        features: [("studio/swatch".to_owned(), sill())].into(),
        layers,
        ..Request::default()
    }
}

fn grime_and_moss(grit: f64, amount: f64, spread: f64, resolution: usize) -> Request {
    stack(
        grit,
        vec![
            layer("grime.material.ts", &[("amount", amount)]),
            layer("moss.material.ts", &[("spread", spread)]),
        ],
        resolution,
    )
}

fn one_program(grit: f64, amount: f64, spread: f64, resolution: usize) -> Request {
    Request {
        entry: "oracle.ms".into(),
        source: oracle(grit, amount, spread),
        seed: 7,
        resolution,
        features: [("studio/swatch".to_owned(), sill())].into(),
        ..Request::default()
    }
}

/// A build, with each output plane bake it made: entry, outputs, points read from a raster and
/// points evaluated.
type Bakes = Vec<(String, Vec<String>, usize, usize)>;

fn build(session: &mut Session, revision: u64, request: &Request) -> (Output, Bakes) {
    LAYER_BAKES.with(|bakes| bakes.borrow_mut().clear());
    let output = session
        .build(
            request,
            revision,
            &|name| Err(format!("{name} is unavailable")),
            &|| false,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
    (
        output,
        LAYER_BAKES.with(|bakes| std::mem::take(&mut *bakes.borrow_mut())),
    )
}

fn fresh(request: &Request) -> Output {
    build(&mut Session::default(), 1, request).0
}

fn assert_same(a: &Output, b: &Output, what: &str) {
    assert_eq!(a.model.images.len(), b.model.images.len(), "{what}");
    let maps = |output: &Output| {
        let material = output.model.materials.values().next().unwrap();
        [
            material.texture.clone(),
            material.normal_texture.clone(),
            material.metallic_roughness_texture.clone(),
        ]
        .map(|id| {
            image::decode(&output.model.images[&id.unwrap()].bytes)
                .unwrap()
                .rgba
        })
    };
    for (role, (a, b)) in ["color", "normal", "properties"]
        .into_iter()
        .zip(maps(a).into_iter().zip(maps(b)))
    {
        assert!(a == b, "{what}: the {role} maps differ");
    }
    assert_eq!(
        a.model.materials.keys().collect::<Vec<_>>(),
        b.model.materials.keys().collect::<Vec<_>>(),
        "{what}: material identities hash every map"
    );
}

/// The materials each bake of a build evaluated, by entry, with their outputs sorted.
fn baked(bakes: &Bakes) -> Vec<(String, Vec<String>)> {
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (entry, values, _, _) in bakes {
        found
            .entry(entry.clone())
            .or_default()
            .extend(values.clone());
    }
    found
        .into_iter()
        .map(|(entry, mut values)| {
            values.sort();
            (entry, values)
        })
        .collect()
}

fn named(list: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    list.iter()
        .map(|(entry, values)| {
            let values = values.iter().map(|value| (*value).to_owned()).collect();
            ((*entry).to_owned(), values)
        })
        .collect()
}

#[test]
fn a_stack_of_layers_bakes_the_bytes_of_the_same_appearance_written_as_one_program() {
    for (grit, amount, spread) in [(0.5, 0.8, 0.5), (0.9, 1.0, 0.0), (0.0, 0.3, 1.0)] {
        let what = format!("grit {grit}, amount {amount}, spread {spread}");
        let mut session = Session::default();
        let (output, bakes) = build(&mut session, 1, &grime_and_moss(grit, amount, spread, 64));
        assert_same(
            &output,
            &fresh(&one_program(grit, amount, spread, 64)),
            &what,
        );
        // Each material bakes the channels it gives, once: the moss reads the base's height and
        // the grime's color, and the composite reads the grime's roughness, which the moss
        // leaves.
        assert_eq!(
            baked(&bakes),
            named(&[
                ("base.ms", &["base_color", "height", "roughness"]),
                ("grime.material.ts", &["base_color", "roughness"]),
                ("moss.material.ts", &["base_color", "height"]),
            ]),
            "{what}"
        );
        assert_eq!(bakes.len(), 3, "one pass a material: {bakes:?}");
        // The relief is the moss's over the base's.
        let material = output.model.materials.values().next().unwrap();
        let normal =
            image::decode(&output.model.images[material.normal_texture.as_ref().unwrap()].bytes)
                .unwrap()
                .rgba;
        assert!(normal.chunks_exact(4).any(|texel| texel[0] != 128));
    }
    // The slot reports its base, its layers and where each channel comes from.
    let (output, _) = build(
        &mut Session::default(),
        1,
        &grime_and_moss(0.5, 0.8, 0.5, 32),
    );
    let metadata = &output.metadata;
    assert_eq!(metadata["entry"], "base.ms");
    assert_eq!(metadata["layers"][0]["kind"], "layer");
    assert_eq!(metadata["layers"][1]["entry"], "moss.material.ts");
    assert_eq!(
        metadata["channels"],
        serde_json::json!({ "base_color": "stack.2", "height": "stack.2", "roughness": "stack.1" })
    );
    // Production threads bake the same planes and maps.
    let production = Request {
        profile: Profile::Production,
        ..grime_and_moss(0.5, 0.8, 0.5, 96)
    };
    let expected = Request {
        profile: Profile::Production,
        ..one_program(0.5, 0.8, 0.5, 96)
    };
    assert_same(&fresh(&production), &fresh(&expected), "production");
}

#[test]
fn an_edit_an_added_or_a_removed_layer_bakes_only_the_materials_it_can_change() {
    let mut session = Session::default();
    let mut revision = 0;
    let mut step = |request: &Request| {
        revision += 1;
        build(&mut session, revision, request)
    };
    let (first, _) = step(&grime_and_moss(0.5, 0.8, 0.5, 48));
    // The top layer's parameter: only the moss.
    let (output, bakes) = step(&grime_and_moss(0.5, 0.8, 0.9, 48));
    assert_eq!(
        baked(&bakes),
        named(&[("moss.material.ts", &["base_color", "height"])])
    );
    assert_same(&output, &fresh(&one_program(0.5, 0.8, 0.9, 48)), "spread");
    // The grime's: the grime, and the moss over it (one pass bakes a material's planes over a
    // receiver, so each reads all the planes beneath that any of them reads); never the base.
    let (output, bakes) = step(&grime_and_moss(0.5, 0.4, 0.9, 48));
    assert_eq!(
        baked(&bakes),
        named(&[
            ("grime.material.ts", &["base_color", "roughness"]),
            ("moss.material.ts", &["base_color", "height"]),
        ])
    );
    assert_eq!(bakes.len(), 2, "one pass a material: {bakes:?}");
    assert_same(&output, &fresh(&one_program(0.5, 0.4, 0.9, 48)), "amount");
    // Removing the moss bakes no plane: the composite reads the grime's and the base's.
    let without = stack(
        0.5,
        vec![layer("grime.material.ts", &[("amount", 0.4)])],
        48,
    );
    let (_, bakes) = step(&without);
    assert!(bakes.is_empty(), "{bakes:?}");
    // Adding a stain on top bakes the stain alone.
    let mut stained = without.clone();
    stained.layers.push(layer("stain.material.ts", &[]));
    let (_, bakes) = step(&stained);
    assert_eq!(
        baked(&bakes),
        named(&[("stain.material.ts", &["base_color"])])
    );
    // The base's grit: its roughness and everything above that reads it.
    let (_, bakes) = step(&Request {
        parameters: [("grit".to_owned(), 0.2.into())].into(),
        ..stained.clone()
    });
    assert_eq!(
        baked(&bakes),
        named(&[
            ("base.ms", &["roughness"]),
            ("grime.material.ts", &["base_color", "roughness"]),
            ("stain.material.ts", &["base_color"]),
        ])
    );
    // Back to the first stack: everything is kept.
    let (back, bakes) = step(&grime_and_moss(0.5, 0.8, 0.5, 48));
    assert!(bakes.is_empty(), "{bakes:?}");
    assert_same(&back, &first, "the first stack again");
}

/// The swatch-sized wall of `width` meters, standing in the XZ plane, on its pattern anchor at
/// 64 texels a meter.
fn wall(width: f64, layers: Vec<Layer>) -> Request {
    let receiver = Receiver {
        id: "wall".into(),
        object_id: "wall".into(),
        positions: vec![0., 0., 0., width, 0., 0., width, 0., 1., 0., 0., 1.],
        normals: [0., -1., 0.].repeat(4),
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: None,
        contacts: Vec::new(),
    };
    Request {
        mapping: Mapping::Anchored,
        texel_density: Some(64.),
        resolution: 256,
        anchors: [(
            "wall".to_owned(),
            Anchor {
                origin: [0.; 3],
                u: [1., 0., 0.],
                v: [0., 0., 1.],
            },
        )]
        .into(),
        features: [("wall".to_owned(), sill())].into(),
        receivers: vec![receiver],
        ..stack(0.5, layers, 256)
    }
}

#[test]
fn a_resized_wall_reads_its_base_from_the_intrinsic_raster_and_bakes_its_layers_again() {
    let layers = || {
        vec![
            layer("grime.material.ts", &[("amount", 0.8)]),
            layer("moss.material.ts", &[("spread", 0.5)]),
        ]
    };
    let mut session = Session::default();
    let (output, bakes) = build(&mut session, 1, &wall(1.5, layers()));
    assert_same(&output, &fresh(&wall(1.5, layers())), "1.5 m");
    let base = |bakes: &Bakes| {
        let found = bakes.iter().find(|(entry, ..)| entry == "base.ms");
        let (_, _, held, evaluated) = found.expect("the base was baked");
        (*held, *evaluated)
    };
    // Before the first bake nothing was kept (a few points that two texels of the atlas sample
    // are read from what this bake already put there).
    let (held, evaluated) = base(&bakes);
    assert!(held * 100 < evaluated, "{held} read, {evaluated} evaluated");
    // Widened, the wall bakes its layers over the new layout, and the base only where the
    // wall grew: what both widths cover is read from the raster.
    let (output, bakes) = build(&mut session, 2, &wall(2.0, layers()));
    assert_same(&output, &fresh(&wall(2.0, layers())), "2.0 m");
    let (held, grown) = base(&bakes);
    assert!(
        held as f64 >= evaluated as f64 * 0.95 && grown < held / 2,
        "{held} read, {grown} evaluated of a first bake of {evaluated}"
    );
    assert!(bakes.iter().any(|(entry, ..)| entry == "moss.material.ts"));
    // Another stack over the same base on the same wall reads all of it.
    let (output, bakes) = build(
        &mut session,
        3,
        &wall(2.0, vec![layer("stain.material.ts", &[])]),
    );
    assert_same(
        &output,
        &fresh(&wall(2.0, vec![layer("stain.material.ts", &[])])),
        "another stack",
    );
    let found = bakes.iter().find(|(entry, ..)| entry == "base.ms");
    assert!(
        found.is_none_or(|(_, _, _, evaluated)| *evaluated == 0),
        "{bakes:?}"
    );
}

/// A base with a live operation, as the shipped steel has.
const LIVE: &str = r#"import { defineMaterial, modulateDiffuse, noise, seed, surface, mul, add, vec3 } from "@superspec/material/v1";
const grain = noise(mul(surface, 20), seed("grain"));
export default defineMaterial({
  base: { baseColor: vec3(0.3, 0.32, 0.35), roughness: add(0.2, mul(grain, 0.3)), metalness: 0.9 },
  live: [modulateDiffuse({ factor: add(0.9, mul(grain, 0.2)), mask: grain })],
});
"#;

#[test]
fn a_base_with_live_operations_keeps_them_under_its_layers() {
    let mut request = stack(0.5, vec![layer("stain.material.ts", &[])], 48);
    request.entry = "live.material.ts".into();
    request.source = LIVE.into();
    request.parameters.clear();
    let (layered, bakes) = build(&mut Session::default(), 1, &request);
    let alone = fresh(&Request {
        layers: Vec::new(),
        ..request.clone()
    });
    let material = |output: &Output| output.model.materials.values().next().unwrap().clone();
    let (over, under) = (material(&layered), material(&alone));
    // The live operation and its maps are the base's: the stain changes no live input.
    let images = |material: &superspec_types::material::Material, output: &Output| {
        material
            .live
            .iter()
            .flat_map(|operation| operation.arguments.values())
            .filter_map(|argument| match argument {
                superspec_types::material::LiveArgument::Texture { texture, .. } => {
                    Some(output.model.images[texture].bytes.to_vec())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(over.live.len(), 1);
    assert_eq!(over.live, under.live, "the live operations are the base's");
    let maps = images(&over, &layered);
    assert!(!maps.is_empty());
    assert_eq!(maps, images(&under, &alone), "and so are their maps");
    // The color is the stain's over the base's.
    assert_ne!(over.texture, under.texture);
    // The live inputs are baked once as field planes of the base.
    assert!(bakes.iter().any(|(entry, ..)| entry == "live.material.ts"));
}

/// A build of `request` shared with `helpers` (`Session::distribute_bakes`): the session stops
/// for its bake, each helper bakes its share of the rows, and the session builds again with
/// them. Returns the output, the plane bakes of the session (both builds) and of the helpers.
fn distributed(
    session: &mut Session,
    helpers: &mut [Session],
    revision: u64,
    request: &Request,
) -> (Output, Bakes, Bakes) {
    let assets = |name: &str| Err(format!("{name} is unavailable"));
    LAYER_BAKES.with(|bakes| bakes.borrow_mut().clear());
    session.distribute_bakes(true);
    let stopped = session
        .build(request, revision, &assets, &|| false)
        .err()
        .expect("the bake is handed to other sessions");
    assert!(
        stopped.iter().all(|error| error.code == "BAKE_PENDING"),
        "{stopped:?}"
    );
    let mut own = LAYER_BAKES.with(|bakes| std::mem::take(&mut *bakes.borrow_mut()));
    let pending = session.take_pending_bake().expect("a pending bake");
    assert!(session.take_pending_bake().is_none());
    // Helpers are handed the slot as written, not its composite.
    assert_eq!(pending.request.layers, request.layers);
    let count = helpers.len();
    for (index, helper) in helpers.iter_mut().enumerate() {
        let rows = helper
            .bake_share(&pending, index, count, &assets, &|| false)
            .unwrap_or_else(|e| panic!("{e:?}"));
        session.provide_bake_share(&rows).unwrap();
    }
    let shared = LAYER_BAKES.with(|bakes| std::mem::take(&mut *bakes.borrow_mut()));
    let output = session
        .build(request, revision, &assets, &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    session.distribute_bakes(false);
    own.extend(LAYER_BAKES.with(|bakes| std::mem::take(&mut *bakes.borrow_mut())));
    (output, own, shared)
}

#[test]
fn a_distributed_stack_bakes_the_bytes_of_a_local_one_and_its_session_makes_no_plane() {
    let mut live = stack(0.5, vec![layer("stain.material.ts", &[])], 96);
    live.entry = "live.material.ts".into();
    live.source = LIVE.into();
    live.parameters.clear();
    let layers = || {
        vec![
            layer("grime.material.ts", &[("amount", 0.8)]),
            layer("moss.material.ts", &[("spread", 0.5)]),
        ]
    };
    for (what, request, edited) in [
        (
            "swatch",
            grime_and_moss(0.5, 0.8, 0.5, 96),
            grime_and_moss(0.5, 0.8, 0.9, 96),
        ),
        ("anchored wall", wall(1.5, layers()), wall(2.0, layers())),
        ("live base", live.clone(), {
            let mut edited = live.clone();
            edited
                .layers
                .push(layer("moss.material.ts", &[("spread", 0.2)]));
            edited
        }),
        (
            "production",
            Request {
                profile: Profile::Production,
                ..grime_and_moss(0.5, 0.8, 0.5, 96)
            },
            Request {
                profile: Profile::Production,
                ..grime_and_moss(0.5, 0.4, 0.5, 96)
            },
        ),
    ] {
        for count in [1, 3, 5] {
            let mut session = Session::default();
            let mut helpers: Vec<Session> = (0..count).map(|_| Session::default()).collect();
            let (output, own, shared) = distributed(&mut session, &mut helpers, 1, &request);
            assert_same(
                &output,
                &fresh(&request),
                &format!("{what}, {count} shares"),
            );
            assert!(
                own.is_empty(),
                "{what}: the reporting session made planes: {own:?}"
            );
            assert!(!shared.is_empty(), "{what}: the helpers made the planes");
            // An edit: every share reads the planes beneath it that it kept for its rows.
            let (output, own, shared) = distributed(&mut session, &mut helpers, 2, &edited);
            assert_same(
                &output,
                &fresh(&edited),
                &format!("{what} edited, {count} shares"),
            );
            assert!(own.is_empty(), "{what}: {own:?}");
            if what == "swatch" {
                // One pass of the moss for each share, over its rows.
                assert_eq!(shared.len(), count, "{what}: {shared:?}");
                for (entry, values, _, _) in &shared {
                    assert_eq!((entry.as_str(), values.len()), ("moss.material.ts", 2));
                }
            }
            // The same edit again is kept whole by the reporting session.
            let again = session
                .build(
                    &edited,
                    3,
                    &|name| Err(format!("{name} is unavailable")),
                    &|| false,
                )
                .unwrap();
            assert_same(&again, &output, &format!("{what} kept"));
        }
    }
}

#[test]
fn a_stack_that_reads_more_than_one_program_may_is_refused_on_its_base() {
    // Sixty live operations, each reading a field of its own, under a layer that adds height and
    // opacity: the composite would read sixty-five planes.
    let operations: Vec<String> = (1..=60)
        .map(|k| {
            format!(
                "scaleSpecular({{ factor: add(1, mul(0.01, noise(mul(surface, {k}), seed(\"s{k}\")))) }})"
            )
        })
        .collect();
    let busy = format!(
        "import {{ defineMaterial, scaleSpecular, noise, seed, surface, mul, add, vec3 }} from \"@superspec/material/v1\";
export default defineMaterial({{
  base: {{ baseColor: vec3(0.3, 0.32, 0.35), roughness: 0.4, metalness: 0.9 }},
  live: [{}],
}});
",
        operations.join(", ")
    );
    let veil = r#"import { defineLayer, vec3 } from "@superspec/material/v1";
export default defineLayer({ coverage: 0.5, baseColor: vec3(0.5, 0.4, 0.3), height: 0.001, opacity: 0.9 });
"#;
    let mut request = stack(0.5, Vec::new(), 32);
    request.entry = "busy.material.ts".into();
    request.source = busy;
    request.parameters.clear();
    request
        .modules
        .insert("veil.material.ts".into(), veil.into());
    request.layers = vec![layer("veil.material.ts", &[])];
    let errors = Session::default()
        .build(
            &request,
            1,
            &|name| Err(format!("{name} is unavailable")),
            &|| false,
        )
        .err()
        .expect("the slot is refused");
    assert!(
        errors
            .iter()
            .any(|error| error.source_name == "busy.material.ts"
                && error.message.contains("cannot compose its base and layers")
                && error.message.contains("at most 64 external inputs")),
        "{errors:?}"
    );
    // The GPU preview's check refuses it the same way.
    let errors = Session::default().check_layered(&request).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.source_name == "busy.material.ts"),
        "{errors:?}"
    );
    // So is a layer's parameter out of its range, on the layer's module.
    let mut request = grime_and_moss(0.5, 3.0, 0.5, 32);
    let errors = Session::default().check_layered(&request).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.source_name == "grime.material.ts"
                && error.message.contains("amount")),
        "{errors:?}"
    );
    request.layers[0]
        .parameters
        .insert("amount".into(), 0.5.into());
    assert!(Session::default().check_layered(&request).is_ok());
}

#[test]
fn a_slot_written_wrong_says_how() {
    let mut session = Session::default();
    let mut revision = 0;
    let mut fails = |request: &Request, fragment: &str| {
        revision += 1;
        let errors = session
            .build(
                request,
                revision,
                &|name| Err(format!("{name} is unavailable")),
                &|| false,
            )
            .err()
            .expect("the slot is refused");
        assert!(
            errors.iter().any(|error| error.message.contains(fragment)),
            "{errors:?}"
        );
    };
    // A material is not a layer, and a layer is not a base.
    let mut request = grime_and_moss(0.5, 0.8, 0.5, 32);
    request.modules.insert("plain.ms".into(), BASE.into());
    request.layers.push(Layer {
        entry: "plain.ms".into(),
        ..Layer::default()
    });
    fails(&request, "is a weathering layer (defineLayer)");
    let mut request = grime_and_moss(0.5, 0.8, 0.5, 32);
    request.entry = "stain.material.ts".into();
    request.source = STAIN.into();
    request.parameters.clear();
    fails(&request, "The base of a layered slot is a material");
    let mut request = grime_and_moss(0.5, 0.8, 0.5, 32);
    request.layers[1].entry = "missing.material.ts".into();
    fails(&request, "Missing source module missing.material.ts");
    let mut request = grime_and_moss(0.5, 0.8, 0.5, 32);
    request.layers = vec![layer("stain.material.ts", &[]); 9];
    fails(&request, "at most 8 layers");
    let mut request = grime_and_moss(0.5, 0.8, 0.5, 32);
    request.inspection = Some("grain".into());
    fails(&request, "inspected through its base and each layer");
    // A layer's parameters are checked as its own.
    let request = grime_and_moss(0.5, 3.0, 0.5, 32);
    fails(&request, "amount");
}
