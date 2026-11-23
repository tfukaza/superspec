use superspec_engine::worldspec::themes::kowloon::pavement::{
    context::Wall,
    field,
    layout::{self, Layout},
    plan::{Pigment, Planner, Settings},
    surface::Finish,
    terrain,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
/// An 8 m by 6 m footprint split into two receiving triangles, with one wall contact.
fn site(settings: &Settings, budget: &mut Budget<'_>) -> (Layout, Vec<Vec<V3>>, Vec<Wall>) {
    let outline = field::rectangle(0., 0., 8., 6., 0.);
    let layout = layout::footprint(
        &[outline.clone()],
        settings.patch_size,
        settings.material_size,
        budget,
    )
    .unwrap();
    let faces = vec![
        vec![outline[0], outline[1], outline[2]],
        vec![outline[0], outline[2], outline[3]],
    ];
    let walls = vec![Wall {
        host: "west".into(),
        seam: [[0.1, 0., 0.], [0.1, 6., 0.]],
    }];
    (layout, faces, walls)
}
#[test]
fn seeded_ground_pours_keep_supported_covers_wall_damage_and_damp_pigment() {
    for (seed, settings) in [
        (3, Settings::default()),
        (
            11,
            Settings {
                manholes: 1.,
                tile_share: 0.5,
                timber_share: 0.2,
                asphalt: 1.,
                ..Settings::default()
            },
        ),
    ] {
        let mut budget = Budget::new(32_000_000, &|| false).unwrap();
        let (grid, faces, walls) = site(&settings, &mut budget);
        let planner = Planner {
            layout: &grid,
            settings: &settings,
            seed,
            walls: &walls,
        };
        let patches = planner.patches(&faces, &mut budget).unwrap();
        assert!(!patches.is_empty());
        let json = |p: &[_]| serde_json::to_value(p).unwrap();
        assert_eq!(
            json(&patches),
            json(&planner.patches(&faces, &mut budget).unwrap()),
            "seeded planning is repeatable"
        );
        let mut manholes = 0;
        for patch in &patches {
            patch.validate().unwrap();
            let pigment = Pigment {
                patch,
                settings: &settings,
                seed,
                grime: 0.6,
                wear: 0.7,
                tile_size: 0.4,
            };
            pigment.validate().unwrap();
            let point = patch.center;
            assert!(pigment.height(point).is_finite());
            for finish in [Finish::Concrete, Finish::Repair, Finish::Crack, patch.kind] {
                assert!(pigment.color(point, finish).iter().all(|v| v.is_finite()));
            }
            let built = terrain::build(&grid, patch, &pigment, &faces, 0.3, true, &mut budget);
            let Ok(actual) = built else { continue };
            manholes += usize::from(patch.manhole.is_some());
            let original = actual
                .patch_faces
                .iter()
                .map(|p| field::area(p).abs())
                .sum::<f64>();
            let pieces = actual
                .pieces
                .iter()
                .map(|p| field::area(&p.points).abs())
                .sum::<f64>();
            assert!(
                (original - pieces).abs() < 1e-8,
                "manhole pieces retain the receiving footprint"
            );
        }
        if settings.manholes == 1. {
            assert!(manholes > 0, "admitted covers are built");
        }
    }
}
#[test]
fn invalid_or_cancelled_plans_fail_before_output_and_recover() {
    let mut settings = Settings::default();
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let (grid, faces, walls) = site(&settings, &mut budget);
    settings.tile_share = 2.;
    assert!(settings.validate().is_err());
    settings.tile_share = 0.1;
    let planner = Planner {
        layout: &grid,
        settings: &settings,
        seed: 3,
        walls: &walls,
    };
    assert!(
        planner
            .patches(&faces, &mut Budget::new(1000, &|| true).unwrap())
            .is_err()
    );
    assert!(
        planner
            .patches(&faces, &mut Budget::new(1, &|| false).unwrap())
            .is_err()
    );
    assert!(
        planner
            .patches(&[vec![[f64::NAN; 3]; 3]], &mut budget)
            .is_err()
    );
    assert!(!planner.patches(&faces, &mut budget).unwrap().is_empty());
}
