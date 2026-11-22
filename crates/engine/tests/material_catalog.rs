use std::cell::RefCell;
use superspec_aml::material_catalog::{image_name, material};
use superspec_engine::compile_with_assets;

const PNG: &[u8] = include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png");
const SOURCE: &str = r##"<architecture version="1" id="library">
<material id="unused" texture="unreadable.png"/>
<material id="paint" color="#112233"/>
<box id="one" material="builtin:concrete"/>
<box id="two" position="2 0 0" material="builtin:tile-bed"/>
<box id="named" position="4 0 0" material="paint"/>
</architecture>"##;

#[test]
fn builtin_materials_keep_their_namespace_channels_and_used_asset_set() {
    let reads = RefCell::new(vec![]);
    let output = compile_with_assets(SOURCE, 0, &|name| {
        reads.borrow_mut().push(name.to_owned());
        Ok(PNG.to_vec())
    })
    .unwrap();
    let mut names = reads.borrow().clone();
    names.sort();
    assert_eq!(
        names,
        [
            "superspec-material:concrete-shared-normal.png",
            "superspec-material:concrete-shared.png"
        ]
    );
    let concrete = output
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "builtin:concrete")
        .unwrap();
    assert_eq!(concrete.repeat, 0.125);
    assert_eq!(concrete.roughness, 0.96);
    assert_eq!(concrete.normal_scale, Some(1.));
    assert_eq!(concrete.color, "#ffffff");
    assert_eq!(concrete.metalness, 0.);
    assert_eq!(
        output
            .manifest
            .materials
            .iter()
            .find(|m| m.id == "paint")
            .unwrap()
            .color,
        "#112233"
    );
    assert_eq!(output.manifest.assets.len(), 2);
    assert_eq!(output.asset_bytes, [PNG, PNG].concat());
    assert!(!output.manifest.materials.iter().any(|m| m.id == "unused"));
    let hidden = r#"<architecture version="1" id="hidden"><box id="physics" material="builtin:concrete" visible="false"/></architecture>"#;
    let output = compile_with_assets(hidden, 0, &|_| {
        panic!("Collision-only geometry must not read render images")
    })
    .unwrap();
    assert_eq!(output.manifest.colliders.len(), 1);
    assert!(output.manifest.assets.is_empty());
    assert!(output.manifest.meshes.iter().all(|m| m.material.is_none()));
    assert_eq!(
        image_name("superspec-material:concrete-shared.png"),
        Some("concrete-shared.png")
    );
    for invalid in [
        "concrete-shared.png",
        "superspec-material:../concrete-shared.png",
        "superspec-material:unknown.png",
    ] {
        assert!(image_name(invalid).is_none());
    }
    assert!(material("concrete").is_none());
    let source = r#"<architecture version="1" id="channels"><box id="food" material="builtin:restaurant-food"/><box id="glyph" material="builtin:lettering" position="2 0 0"/></architecture>"#;
    let scene = compile_with_assets(source, 0, &|_| Ok(PNG.to_vec())).unwrap();
    let food = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "builtin:restaurant-food")
        .unwrap();
    assert_eq!(
        food.texture.as_deref(),
        Some("superspec-material:restaurant-food.png")
    );
    assert_eq!(
        food.normal_texture.as_deref(),
        Some("superspec-material:restaurant-food-normal.png")
    );
    assert_eq!(
        food.metallic_roughness_texture.as_deref(),
        Some("superspec-material:restaurant-food-properties.png")
    );
    let lettering = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "builtin:lettering")
        .unwrap();
    assert_eq!(
        lettering.texture.as_deref(),
        Some("superspec-material:lettering.png")
    );
    assert_eq!(lettering.alpha_cutoff, Some(0.5));
    assert_eq!(scene.manifest.assets.len(), 4);
}

#[test]
fn structural_surfaces_retain_catalog_lighting_operations() {
    let source = r#"<architecture version="1" id="metal"><box id="trim" material="builtin:casement-enamel"/></architecture>"#;
    let scene = compile_with_assets(source, 0, &|_| Ok(PNG.to_vec())).unwrap();
    let trim = scene
        .manifest
        .materials
        .iter()
        .find(|m| m.id == "builtin:casement-enamel")
        .unwrap();
    assert!(trim.live.iter().any(|op| op.id == "virtualSky"));
    assert!(trim.live.iter().any(|op| op.id == "lightFloor"));
    assert_eq!(trim, &material(&trim.id).unwrap());
}

#[test]
fn builtin_errors_never_poison_later_compilation_or_hide_authored_overrides() {
    for source in [
        SOURCE.replace("builtin:concrete", "builtin:missing"),
        SOURCE.replace(
            "<box id=\"one\"",
            "<material id=\"builtin:concrete\"/><box id=\"one\"",
        ),
    ] {
        assert_eq!(
            compile_with_assets(&source, 0, &|_| Ok(PNG.to_vec()))
                .unwrap_err()
                .code,
            "AML_MATERIAL"
        );
    }
    assert_eq!(
        compile_with_assets(SOURCE, 0, &|name| Err(format!("Missing {name}")))
            .unwrap_err()
            .code,
        "AML_ASSET"
    );
    assert_eq!(
        compile_with_assets(SOURCE, 0, &|_| Ok(vec![1, 2, 3]))
            .unwrap_err()
            .code,
        "AML_ASSET"
    );
    assert!(
        compile_with_assets(SOURCE, 0, &|_| Ok(PNG.to_vec()))
            .unwrap()
            .manifest
            .success
    );
}
