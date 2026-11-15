use serde::Deserialize;
use serde_json::Value;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    pen::Pen,
    signs::{frontage, mounts::WallRect, panel::Settings},
};
use superspec_geometry::Matrix;
#[path = "support/sign_reference.rs"]
mod reference;
#[derive(Deserialize)]
struct Case {
    category: String,
    rect: WallRect,
    elevation: f64,
    span: [f64; 2],
    heights: [f64; 2],
    key: String,
    shown: bool,
    seed: String,
    next: f64,
    reflected: bool,
    detail: u8,
    settings: Settings,
    proposals: Vec<Value>,
}
#[derive(Deserialize)]
struct Golden {
    cases: Vec<Case>,
}
pub fn check_reference_frontages() {
    let g: Golden = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-frontage.json"
    ))
    .unwrap();
    for c in g.cases {
        let kit = Kit::new(c.settings, c.detail).unwrap();
        let mut rng = SeededRng::from_text(&c.seed);
        let pen = Pen::new(
            if c.reflected {
                Matrix([
                    [-1., 0., 0.],
                    [0., 1., 0.],
                    [0., 0., 1.],
                    [-0.0005, -2., 3.],
                ])
            } else {
                Matrix::default()
            },
            "body",
        )
        .unwrap();
        let proposal = match c.category.as_str() {
            "wall" => Some(frontage::wall_board(&pen, c.rect, &kit, &mut rng).unwrap()),
            "balcony" => Some(
                frontage::balcony_board(&pen, c.rect, c.elevation, &kit, &c.key, c.shown).unwrap(),
            ),
            "roof" => {
                frontage::roof_hoarding(&pen, c.span, c.heights, &kit, &c.key, c.shown).unwrap()
            }
            _ => panic!("unknown fixture"),
        };
        assert_eq!(rng.next_f64(), c.next);
        assert_eq!(usize::from(proposal.is_some()), c.proposals.len());
        if let Some(p) = proposal {
            reference::close(&reference::metadata(&p), &c.proposals[0]);
        }
    }
}
#[test]
fn frontage_signs_preserve_named_streams_and_signed_millimeter_identity() {
    check_reference_frontages()
}
