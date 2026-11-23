use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::pavement::materials::{self, Settings};
#[test]
fn ground_materials_keep_authored_pbr_shared_sources_and_scoped_color_overrides() {
    let base = Material {
        id: "base".into(),
        repeat: 2.,
        texture: Some("superspec-material:concrete-shared.png".into()),
        normal_texture: Some("superspec-material:concrete-shared-normal.png".into()),
        ..Material::default()
    };
    let authored_repair = Material {
        id: "authored-repair".into(),
        color: "#334455".into(),
        ..Material::default()
    };
    let mut materials = BTreeMap::from([
        (base.id.clone(), base.clone()),
        (authored_repair.id.clone(), authored_repair.clone()),
    ]);
    let authored = materials.keys().cloned().collect();
    // Ceramic finishes share the scene's Kowloon wall materials when present.
    for id in [
        "$kowloon/wall-ceramic",
        "$kowloon/wall-tile-bed",
        "$kowloon/wall-tile-grime",
    ] {
        materials.insert(
            id.into(),
            Material {
                id: id.into(),
                color: "#808080".into(),
                ..Material::default()
            },
        );
    }
    let declared = BTreeMap::from([("repair".into(), "authored-repair".into())]);
    let colors = BTreeMap::from([
        ("concrete".into(), "#112233".into()),
        ("asphalt".into(), "#445566".into()),
    ]);
    let settings = Settings {
        scope: "test",
        base: "base",
        declared: &declared,
        colors: &colors,
        authored: &authored,
    };
    let expected = [
        ("repair", "authored-repair"),
        ("concrete", "$ground/test/concrete"),
        ("asphalt", "$ground/test/asphalt"),
        ("timber", "builtin:timber"),
        ("crack", "$ground/test/crack"),
    ];
    for (name, id) in expected {
        assert_eq!(
            materials::resolve(name, &settings, &mut materials).unwrap(),
            id
        );
    }
    let resolved = materials.clone();
    for (name, id) in expected {
        assert_eq!(
            materials::resolve(name, &settings, &mut materials).unwrap(),
            id
        );
    }
    assert_eq!(materials, resolved, "resolution is idempotent");
    assert_eq!(materials["authored-repair"], authored_repair);
    let concrete = &materials["$ground/test/concrete"];
    assert_eq!(concrete.color, "#112233");
    assert_eq!(concrete.repeat, base.repeat);
    assert_eq!(concrete.texture, base.texture);
    assert_eq!(concrete.normal_texture, base.normal_texture);
    let crack = &materials["$ground/test/crack"];
    assert_eq!(
        (crack.texture.as_ref(), crack.normal_texture.as_ref()),
        (None, None)
    );
    let asphalt = &materials["$ground/test/asphalt"];
    let shipped = &materials["builtin:asphalt"];
    assert_eq!(asphalt.color, "#445566");
    assert_eq!(
        (asphalt.roughness, &asphalt.texture),
        (shipped.roughness, &shipped.texture),
        "color overrides keep the shared source PBR"
    );
    materials::assets(&settings, &mut materials).unwrap();
    let before = materials.clone();
    materials::assets(&settings, &mut materials).unwrap();
    assert_eq!(materials, before);
}
#[test]
fn reserved_unknown_and_invalid_materials_are_rejected() {
    let mut materials = BTreeMap::from([(
        "base".into(),
        Material {
            id: "base".into(),
            ..Material::default()
        },
    )]);
    let authored = BTreeSet::from(["$ground/test/concrete".into()]);
    let empty = BTreeMap::new();
    let settings = Settings {
        scope: "test",
        base: "base",
        declared: &empty,
        colors: &empty,
        authored: &authored,
    };
    assert!(materials::resolve("concrete", &settings, &mut materials).is_err());
    assert!(materials::resolve("unknown", &settings, &mut materials).is_err());
    let declarations = BTreeMap::from([("asphalt".into(), "missing".into())]);
    let settings = Settings {
        declared: &declarations,
        ..settings
    };
    assert!(materials::resolve("asphalt", &settings, &mut materials).is_err());
    assert!(materials::resolve("repair", &settings, &mut materials).is_ok());
}
