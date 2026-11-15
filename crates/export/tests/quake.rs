use superspec_engine::Session;
use superspec_export::{Options, quake};

#[test]
fn concave_solids_openings_and_palette_textures_produce_quake_sources() {
    let source = include_str!("../../../tests/fixtures/aml/rust-architecture.aml")
        .replace("</architecture>", r#"<marker id="start" kind="spawn" element="floor-slab" position="0.5 0.5 2"/></architecture>"#);
    let scene = Session::default()
        .compile_for_brushes(&source, 0, &|_| Err("Unexpected asset".into()), &|| false)
        .unwrap();
    let palette: Vec<u8> = (0..256).flat_map(|i| [i as u8; 3]).collect();
    let files = quake::encode(&scene, &palette, &Options::default()).unwrap();
    let text = String::from_utf8(files.map).unwrap();
    assert!(text.contains("info_player_start"));
    assert!(text.contains("info_player_deathmatch"));
    assert!(files.report.brushes > scene.export_solids.len() + 6);
    assert_eq!(files.report.spawns[0], [16., 16., 89.]);
    assert_eq!(&files.wad[..4], b"WAD2");
    assert_eq!(u32::from_le_bytes(files.wad[4..8].try_into().unwrap()), 2);
    assert!(quake::encode(&scene, &palette[..767], &Options::default()).is_err());
    assert!(
        quake::encode(
            &scene,
            &palette,
            &Options {
                cancelled: &|| true,
                ..Default::default()
            }
        )
        .is_err()
    );
}
