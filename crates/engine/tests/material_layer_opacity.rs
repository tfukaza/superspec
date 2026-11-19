//! A layered slot keeps its base's opacity: glass under a weathering layer stays blended with the
//! glass's alpha wherever the layer covers nothing, a cutout base keeps its cutout, and a layer
//! that gives an opacity of its own changes it by its coverage (the composite of the stack is
//! what the CPU bakes; the GPU stack bake keeps the same rule, `browser/renderer/bake.ts`
//! `stackOpacity`). Expectations come from the recipes' constants.
use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::authoring::{Layer, Mapping, Output, Receiver, Request, Session};
use superspec_types::material::AlphaMode;

/// A pane of glass: a 1 m square facing -y.
fn pane() -> Receiver {
    Receiver {
        id: "pane".into(),
        object_id: "pane".into(),
        positions: vec![0., 0., 0., 1., 0., 0., 1., 0., 1., 0., 0., 1.],
        normals: vec![0., -1., 0., 0., -1., 0., 0., -1., 0., 0., -1., 0.],
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: None,
        contacts: Vec::new(),
    }
}

const GLASS: &str = r#"import { defineMaterial, vec3 } from "@superspec/material/v1";
export default defineMaterial({ name: "Glass", base: { baseColor: vec3(0.6, 0.7, 0.75), roughness: 0.05, metalness: 0, opacity: 0.4 } });
"#;
/// Opaque where x < 0.5, cut away beyond.
const CUTOUT: &str = r#"import { defineMaterial, vec3, alphaCutout, step, world } from "@superspec/material/v1";
export default defineMaterial({ name: "Cut", base: { baseColor: vec3(0.5, 0.5, 0.5), roughness: 0.8, metalness: 0, opacity: alphaCutout(step(world.x, 0.5), 0.5) } });
"#;
/// Dust over the upper half (z > 0.5), darker and rougher; it gives no opacity.
const DUST: &str = r#"import { defineLayer, vec3, step, world } from "@superspec/material/v1";
export default defineLayer({ name: "Dust", coverage: step(0.5, world.z), baseColor: vec3(0.3, 0.28, 0.25), roughness: 0.9 });
"#;
/// Dust that also clouds the pane: where it covers, the opacity becomes 0.9.
const CLOUDING: &str = r#"import { defineLayer, vec3, step, world } from "@superspec/material/v1";
export default defineLayer({ name: "Clouding", coverage: step(0.5, world.z), baseColor: vec3(0.3, 0.28, 0.25), opacity: 0.9 });
"#;

fn request(base: &str, layers: &[&str]) -> Request {
    let modules: BTreeMap<String, String> = [
        ("glass.material.ts", GLASS),
        ("cutout.material.ts", CUTOUT),
        ("dust.material.ts", DUST),
        ("clouding.material.ts", CLOUDING),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .collect();
    Request {
        kind: "material".into(),
        entry: base.into(),
        source: modules[base].clone(),
        modules,
        resolution: 32,
        mapping: Mapping::World,
        receivers: vec![pane()],
        layers: layers
            .iter()
            .map(|entry| Layer {
                entry: (*entry).into(),
                seed: 1,
                ..Layer::default()
            })
            .collect(),
        ..Request::default()
    }
}

fn absent(name: &str) -> Result<Arc<[u8]>, String> {
    Err(format!("{name} was not supplied."))
}

fn build(request: &Request) -> Output {
    Session::default()
        .build(request, 1, &absent, &|| false)
        .unwrap_or_else(|errors| panic!("{errors:?}"))
}

/// The material, and how many of its color map's texels hold each alpha (the pane's two
/// triangles are charts of their own, so the map is not the pane's picture).
fn alphas(output: &Output) -> (superspec_types::material::Material, BTreeMap<u8, usize>) {
    let material = output.model.materials.values().next().unwrap().clone();
    let bytes = &output.model.images[material.texture.as_ref().unwrap()].bytes;
    let image = superspec_geometry::image::decode(bytes).unwrap();
    let mut counts = BTreeMap::new();
    for texel in image.rgba.chunks_exact(4) {
        *counts.entry(texel[3]).or_insert(0) += 1;
    }
    (material, counts)
}

/// The share of texels whose alpha is within one of `value`.
fn share(counts: &BTreeMap<u8, usize>, value: f64) -> f64 {
    let byte = (value * 255.).round() as u8;
    let near: usize = counts
        .iter()
        .filter(|(alpha, _)| alpha.abs_diff(byte) <= 1)
        .map(|(_, n)| n)
        .sum();
    near as f64 / counts.values().sum::<usize>() as f64
}

#[test]
fn a_blended_base_stays_blended_with_its_alpha_under_a_layer() {
    // 0.4 everywhere; a layer that gives no opacity leaves it, covered or not.
    for layers in [&[][..], &["dust.material.ts"][..]] {
        let (material, counts) = alphas(&build(&request("glass.material.ts", layers)));
        assert_eq!(material.alpha_mode, Some(AlphaMode::Blend), "{layers:?}");
        assert!(share(&counts, 0.4) > 0.95, "{layers:?}: {counts:?}");
    }
    // A layer that gives opacity sets it where it covers (the upper half): 0.9 there, the
    // glass's 0.4 elsewhere.
    let (material, counts) = alphas(&build(&request(
        "glass.material.ts",
        &["dust.material.ts", "clouding.material.ts"],
    )));
    assert_eq!(material.alpha_mode, Some(AlphaMode::Blend));
    let (glass, clouded) = (share(&counts, 0.4), share(&counts, 0.9));
    assert!(
        glass > 0.35 && clouded > 0.35 && glass + clouded > 0.95,
        "{counts:?}"
    );
}

#[test]
fn a_cutout_base_keeps_its_cutout_under_a_layer() {
    for layers in [&[][..], &["dust.material.ts"][..]] {
        let (material, counts) = alphas(&build(&request("cutout.material.ts", layers)));
        assert_eq!(material.alpha_mode, Some(AlphaMode::Mask), "{layers:?}");
        assert_eq!(material.alpha_cutoff, Some(0.5), "{layers:?}");
        // Opaque where x < 0.5 and cut away beyond, under the layer as without it.
        let (kept, cut) = (share(&counts, 1.), share(&counts, 0.));
        assert!(
            kept > 0.35 && cut > 0.35 && kept + cut > 0.95,
            "{layers:?}: {counts:?}"
        );
    }
}

#[test]
fn an_opaque_base_stays_opaque_under_a_layer() {
    // A layer alone (no opacity anywhere in the stack) keeps a slot opaque.
    let mut opaque = request("glass.material.ts", &["dust.material.ts"]);
    opaque.source = opaque.modules["glass.material.ts"].replace(", opacity: 0.4", "");
    opaque
        .modules
        .insert("glass.material.ts".into(), opaque.source.clone());
    let (material, counts) = alphas(&build(&opaque));
    assert_eq!(material.alpha_mode, None);
    assert_eq!(share(&counts, 1.), 1., "{counts:?}");
}
