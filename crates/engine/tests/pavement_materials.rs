use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::pavement::materials::{self, Settings};
#[derive(Deserialize)]
struct Case {
    scope: String,
    initial: Vec<Material>,
    declared: BTreeMap<String, String>,
    colors: BTreeMap<String, String>,
    ids: BTreeMap<String, String>,
    result: Vec<Material>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_pavement_materials() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/pavement-materials.json"
    ))
    .unwrap();
    for c in g.cases {
        let authored = c.initial.iter().map(|m| m.id.clone()).collect();
        let mut materials = c
            .initial
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<BTreeMap<_, _>>();
        let settings = Settings {
            scope: &c.scope,
            base: "base",
            declared: &c.declared,
            colors: &c.colors,
            authored: &authored,
        };
        for _ in 0..2 {
            for (name, id) in &c.ids {
                assert_eq!(
                    materials::resolve(name, &settings, &mut materials).unwrap(),
                    *id
                );
            }
        }
        assert_eq!(
            materials,
            c.result.into_iter().map(|m| (m.id.clone(), m)).collect()
        );
        let before = materials.clone();
        materials::assets(&settings, &mut materials).unwrap();
        assert_eq!(materials, before);
    }
}
#[test]
fn ground_materials_keep_authored_pbr_shared_sources_and_scoped_color_overrides() {
    check_reference_pavement_materials();
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
