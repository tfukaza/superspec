use std::path::Path;
use superspec_engine::{Session, compile_collision_with_assets, compile_with_assets};

const COURTYARD: &str = include_str!("../src/worldspec/fixtures/courtyard-building.aml");

fn assets(id: &str) -> Result<Vec<u8>, String> {
    let name = superspec_catalog::image_name(id).ok_or_else(|| format!("Unexpected asset {id}"))?;
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/materials")
            .join(name),
    )
    .map_err(|error| error.to_string())
}

#[test]
fn city_courtyard_uses_the_shared_kit_and_keeps_authored_collision() {
    let physical = compile_collision_with_assets(COURTYARD, 1, &|_| {
        panic!("Collision-only export requested a building texture")
    })
    .unwrap();
    let visual = compile_with_assets(COURTYARD, 1, &assets).unwrap();
    assert_eq!(visual.manifest.colliders, physical.manifest.colliders);
    for edge in 0..8 {
        let part = format!("city/ring/part/side-{edge}/");
        assert!(
            visual
                .manifest
                .meshes
                .iter()
                .any(|m| m.id.starts_with(&part)),
            "Missing kit facade {edge}"
        );
    }
    assert!(
        visual
            .manifest
            .meshes
            .iter()
            .any(|m| m.id.ends_with("/roof/roof/top"))
    );
    assert!(
        visual
            .manifest
            .meshes
            .iter()
            .any(|m| m.id.contains("/part/window-"))
    );
}

#[test]
fn failed_city_building_asset_load_recovers_in_the_same_session() {
    let source = r#"<architecture version="1" id="shop-test"><city>
      <building id="shop" position="0 0 0" floors="2" seed="4" front-trade="provisions"/>
    </city></architecture>"#;
    let session = Session::default();
    let failure = session
        .compile(source, 1, &|asset| {
            Err(format!("Deliberately missing {asset}"))
        })
        .unwrap_err();
    assert!(failure.message.contains("Deliberately missing"));
    assert_eq!(failure.source.line, 2);

    let recovered = session.compile(source, 2, &assets).unwrap();
    let fresh = Session::default().compile(source, 2, &assets).unwrap();
    assert_eq!(recovered.packet(), fresh.packet());
}
