use std::collections::BTreeMap;
use superspec_aml::material::Material;
use superspec_engine::worldspec::themes::kowloon::{
    deck::{self, Settings},
    mesh::{Mesh, Mode},
    pavement::materials,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
fn initial() -> Vec<Material> {
    serde_json::from_value(serde_json::json!([
        {"id":"screed","color":"#ffffff","repeat":0.125,"roughness":0.96,"metalness":0,"texture":"superspec-material:concrete-shared.png","normalTexture":"superspec-material:concrete-shared-normal.png","normalScale":1},
        {"id":"felt","color":"#ffffff","repeat":0.65,"roughness":0.88,"metalness":0,"texture":"superspec-material:roofing.png"},
        {"id":"$kowloon/wall-ceramic","color":"#ffffff","repeat":2.5,"roughness":0.68,"metalness":0,"texture":"superspec-material:wall-ceramic-weathered.png","normalTexture":"superspec-material:wall-ceramic-normal.png","normalScale":0.55},
        {"id":"$kowloon/wall-tile-bed","color":"#aaa69a","repeat":0.125,"roughness":0.96,"metalness":0,"texture":"superspec-material:concrete-shared.png","normalTexture":"superspec-material:concrete-shared-normal.png","normalScale":0.4},
        {"id":"$kowloon/wall-tile-grime","color":"#725139","repeat":1,"roughness":1,"metalness":0}
    ]))
    .unwrap()
}
fn lay(mode: Mode, corners: &[V3], seed: &str, felt: bool, fine_marks: bool) -> Mesh<'static> {
    let initial = initial();
    let authored = initial.iter().map(|m| m.id.clone()).collect();
    let mut catalog = initial.into_iter().map(|m| (m.id.clone(), m)).collect();
    let declared = if felt {
        BTreeMap::from([("asphalt".into(), "felt".into())])
    } else {
        BTreeMap::new()
    };
    let colors = BTreeMap::from([("tile".into(), "#a8674c".into())]);
    let assets = materials::assets(
        &materials::Settings {
            scope: "$deck/screed",
            base: "screed",
            declared: &declared,
            colors: &colors,
            authored: &authored,
        },
        &mut catalog,
    )
    .unwrap();
    let settings = Settings {
        host: "host",
        seed,
        grime: 0.6,
        wear: 0.7,
        fine_marks,
    };
    let mut mesh = Mesh::new(mode, 500_000, &never).unwrap();
    let mut budget = Budget::new(64_000_000, &|| false).unwrap();
    deck::lay(
        &mut mesh,
        &settings,
        &assets,
        "terrace",
        corners,
        &mut budget,
    )
    .unwrap();
    mesh
}
fn never() -> bool {
    false
}
#[test]
fn roof_terrace_surveys_match_render_for_rotated_scaled_felt_and_tiled_decks() {
    let cases: [(&[V3], bool, bool); 6] = [
        (
            &[
                [2., -3., 8.],
                [7., -3., 8.],
                [8., -1., 8.],
                [7., 1., 8.],
                [2., 1., 8.],
            ],
            true,
            true,
        ),
        (
            &[
                [-0.440469091547548, 4.618668559085707, 8.],
                [10.987533747081013, 8.279372196407028, 8.],
                [13.428002838628561, 0.6607036373213218, 8.],
                [2., -3., 8.],
            ],
            false,
            false,
        ),
        (
            &[[2., -3., 8.], [32., -3., 8.], [32., 13., 8.], [2., 13., 8.]],
            false,
            true,
        ),
        (
            &[
                [0.779765454226226, 0.8093342795428535, 8.],
                [5.5414333036547925, 2.3346274617600713, 8.],
                [6.761667849428567, -1.4747068177827825, 8.],
                [2., -3., 8.],
            ],
            true,
            true,
        ),
        (
            &[
                [2., -3., 8.],
                [14., -3., 8.],
                [15., 1., 8.],
                [14., 5., 8.],
                [2., 5., 8.],
            ],
            false,
            false,
        ),
        (
            &[[2., -3., 8.], [7., -3., 8.], [7., 1., 8.], [2., 1., 8.]],
            true,
            true,
        ),
    ];
    for (i, (corners, felt, fine)) in cases.into_iter().enumerate() {
        let seed = format!("deck/{i}");
        let render = lay(Mode::Render, corners, &seed, felt, fine);
        let survey = lay(Mode::Survey, corners, &seed, felt, fine);
        assert!(render.triangle_count() > 0, "{seed}");
        assert_eq!(render.triangle_count(), survey.triangle_count(), "{seed}");
    }
}
