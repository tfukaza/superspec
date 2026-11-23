use serde_json::json;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    kit::Kit,
    pen::Pen,
    signs::{frontage, mounts::WallRect, panel::Settings},
};
use superspec_geometry::Matrix;
fn kit(detail: u8) -> Kit {
    let settings: Settings = serde_json::from_value(json!({
        "weathering": {"seed": "frontage/0", "wear": 0.25, "grime": 0.6, "rust": 0.4,
            "textureDensity": 32,
            "metal": {"material": "metal", "repeat": 1},
            "paint": {"material": "paint", "repeat": 1},
            "sheet": {"material": "sheet", "repeat": 1}},
        "lettering": true,
        "timber": {"textured": false, "timberRepeat": 1.2, "atlasRepeat": 1}
    }))
    .unwrap();
    Kit::new(settings, detail).unwrap()
}
fn rect() -> WallRect {
    WallRect {
        x0: 0.2,
        z0: 2.6,
        width: 2.8,
        height: 0.7,
    }
}
fn reflected() -> Pen {
    Pen::new(
        Matrix([
            [-1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [-0.0005, -2., 3.],
        ]),
        "body",
    )
    .unwrap()
}
#[test]
fn wall_boards_take_one_caller_draw_and_key_by_signed_world_millimeters() {
    let kit = kit(0);
    for (pen, key) in [
        (
            Pen::new(Matrix::default(), "body").unwrap(),
            "fascia@200,0,2600",
        ),
        // World corner (-0.2005, -2, 5.6) rounds half up to signed millimeters.
        (reflected(), "fascia@-200,-2000,5600"),
    ] {
        let mut rng = SeededRng::from_text("frontage/wall");
        let mut expected = rng.clone();
        expected.next_u32();
        let p = frontage::wall_board(&pen, rect(), &kit, &mut rng).unwrap();
        assert_eq!(rng.next_f64(), expected.next_f64(), "one designated draw");
        assert_eq!(p.key, key);
        assert!(p.shown);
        assert!(p.business.is_some());
    }
}
#[test]
fn balcony_and_roof_boards_use_their_named_streams_and_respect_room() {
    let kit = kit(1);
    let pen = Pen::new(Matrix::default(), "body").unwrap();
    for shown in [true, false] {
        let a = frontage::balcony_board(&pen, rect(), 2.8, &kit, "frontage/choice", shown).unwrap();
        let b = frontage::balcony_board(&pen, rect(), 2.8, &kit, "frontage/choice", shown).unwrap();
        assert_eq!(a.key, "frontage/choice");
        assert_eq!(a.shown, shown);
        assert_eq!(
            serde_json::to_value(&a.business).unwrap(),
            serde_json::to_value(&b.business).unwrap(),
            "named stream is repeatable"
        );
        let roof =
            frontage::roof_hoarding(&pen, [0.3, 7.], [0.7, 3.6], &kit, "frontage/roof", shown)
                .unwrap()
                .unwrap();
        assert_eq!(roof.shown, shown);
    }
    // Roof hoardings need at least 3.2 m of span.
    assert!(
        frontage::roof_hoarding(&pen, [0.3, 2.], [0.7, 3.6], &kit, "frontage/roof", true)
            .unwrap()
            .is_none()
    );
}
