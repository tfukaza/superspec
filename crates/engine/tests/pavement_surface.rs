use std::collections::BTreeMap;
use superspec_engine::worldspec::themes::kowloon::pavement::{
    field::rectangle,
    surface::{Deposit, Field, Finish, Recipe, Source},
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
const ORDER: [(Finish, &str); 15] = [
    (Finish::Concrete, "concrete"),
    (Finish::Repair, "repair"),
    (Finish::Tile, "tile"),
    (Finish::Timber, "timber"),
    (Finish::Grout, "grout"),
    (Finish::Substrate, "substrate"),
    (Finish::Asphalt, "asphalt"),
    (Finish::ManholeRim, "manhole-rim"),
    (Finish::ManholeCover, "manhole-cover"),
    (Finish::ManholeSlot, "manhole-slot"),
    (Finish::Crack, "crack"),
    (Finish::Joint, "joint"),
    (Finish::Spall, "spall"),
    (Finish::Chip, "chip"),
    (Finish::TileGrime, "tile-grime"),
];
fn color(p: V3, finish: Finish) -> V3 {
    [
        0.7 + p[0] * 0.01,
        0.8 + p[1] * 0.02,
        0.6 + ORDER.iter().position(|(f, _)| *f == finish).unwrap() as f64 * 0.005,
    ]
}
/// A 2.4 m by 2 m patch, optionally with asphalt overflow and joint/spall/chip/crack deposits.
fn recipe(kind: Finish, asphalt: bool, deposits: bool, relief: bool) -> Recipe {
    let polygon = rectangle(-1.2, -1., 2.4, 2., 0.);
    let deposit = |material, polygon| Deposit { polygon, material };
    Recipe {
        seed: 376,
        kind,
        receiving_edges: (0..4)
            .map(|i| [polygon[i], polygon[(i + 1) % 4]])
            .chain([[[0.75, -1., 0.], [0.75, 0.1, 0.]]])
            .collect(),
        polygon,
        asphalt: if asphalt {
            vec![rectangle(0.4, -1., 0.8, 0.8, 0.)]
        } else {
            vec![]
        },
        deposits: if deposits {
            vec![
                deposit(Finish::Joint, rectangle(-1.2, -0.003, 2.4, 0.006, 0.)),
                deposit(Finish::Spall, rectangle(-0.8, -0.7, 0.5, 0.4, 0.)),
                deposit(Finish::Chip, rectangle(-0.7, -0.6, 0.2, 0.2, 0.)),
                deposit(
                    Finish::Crack,
                    vec![[0.1, -0.8, 0.], [0.105, -0.8, 0.], [0.6, -0.3, 0.]],
                ),
            ]
        } else {
            vec![]
        },
        tile_size: 0.16,
        tile_aspect: Some(1.),
        tile_wear: Some(0.7),
        tile_grime: Some(0.35),
        worn_edges: vec![true, false, true, true],
        angle: 0.3,
        phase: 0.46,
        relief,
        sources: ORDER
            .iter()
            .enumerate()
            .map(|(i, (finish, name))| {
                (
                    *finish,
                    Source {
                        id: format!("ground/{name}"),
                        repeat: 0.7 + i as f64 * 0.13,
                        roughness: (i % 2 == 1).then_some(0.7),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    }
}
fn plate(finish: Finish) -> f64 {
    match finish {
        Finish::Asphalt => 0.0035,
        Finish::Joint | Finish::Crack | Finish::Repair => -0.0015,
        Finish::Spall => -0.004,
        Finish::Chip => -0.0025,
        _ => 0.,
    }
}
#[test]
fn pavement_fields_own_materials_in_layer_order_and_keep_weights_normalized() {
    for kind in [
        Finish::Concrete,
        Finish::Repair,
        Finish::Tile,
        Finish::Timber,
        Finish::ManholeCover,
    ] {
        for (asphalt, deposits, relief) in [
            (false, false, true),
            (true, false, false),
            (false, true, true),
            (true, true, false),
        ] {
            let recipe = recipe(kind, asphalt, deposits, relief);
            let mut budget = Budget::new(16_000_000, &|| false).unwrap();
            let field = Field::new(&recipe, &mut budget).unwrap();
            let mut finishes = vec![kind];
            if asphalt {
                finishes.push(Finish::Asphalt);
            }
            let laid = matches!(kind, Finish::Tile | Finish::Timber);
            if laid {
                finishes.extend([Finish::Substrate, Finish::Grout]);
            }
            if deposits {
                finishes.extend([Finish::Joint, Finish::Spall, Finish::Chip, Finish::Crack]);
            }
            let mut materials: Vec<_> = finishes
                .iter()
                .map(|f| recipe.sources[f].id.clone())
                .collect();
            if kind == Finish::Tile {
                materials.push(recipe.sources[&Finish::TileGrime].id.clone());
            }
            let label = format!("{kind:?} asphalt={asphalt} deposits={deposits}");
            assert_eq!(field.materials, materials, "{label}");
            // Only poured finishes carry discrete plate elevations; the base pour is level.
            let elevations = matches!(kind, Finish::Concrete | Finish::Repair).then(|| {
                finishes
                    .iter()
                    .enumerate()
                    .map(|(i, f)| if i == 0 { 0. } else { plate(*f) })
                    .collect::<Vec<_>>()
            });
            assert_eq!(field.elevations, elevations, "{label}");
            for x in [-1.1, -0.6, 0., 0.5, 0.9, 1.15] {
                for y in [-0.95, -0.5, 0., 0.45, 0.9] {
                    for footprint in [0.002, 0.01, 0.2] {
                        let s = field.sample(x, y, footprint).unwrap();
                        assert!(
                            (s.layers.iter().map(|l| l.weight).sum::<f64>() - 1.).abs() < 1e-10,
                            "{label} at {x} {y}"
                        );
                        assert!(field.rendered_height(x, y, footprint).unwrap().is_finite());
                        assert!(field.raster_sample(x, y, footprint, &color).is_ok());
                    }
                }
            }
            for bounds in [
                [-1.2, -1., 1.2, 1.],
                [-0.2, -0.2, 0.2, 0.2],
                [0.5, -0.9, 1.1, -0.3],
            ] {
                field.uniform_coverage(bounds, &mut budget).unwrap();
                assert!(field.feature_scale(bounds).is_finite());
                field.construction_cuts(bounds, &mut budget).unwrap();
            }
        }
    }
}
#[test]
fn invalid_cancelled_and_exhausted_fields_leave_valid_queries_usable() {
    let mut recipe = recipe(Finish::Timber, false, true, false);
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    assert!(Field::new(&recipe, &mut Budget::new(1, &|| false).unwrap()).is_err());
    assert!(Field::new(&recipe, &mut Budget::new(1000, &|| true).unwrap()).is_err());
    recipe.worn_edges.pop();
    assert!(Field::new(&recipe, &mut budget).is_err());
    recipe.worn_edges.push(true);
    let f = Field::new(&recipe, &mut budget).unwrap();
    assert!(f.sample(f64::NAN, 0., 0.).is_err());
    assert!(f.sample(0., 0., -1.).is_err());
    assert!(f.uniform_coverage([1., 0., 0., 1.], &mut budget).is_err());
    assert!(
        f.construction_cuts([0., 0., 20_000., 1.], &mut budget)
            .is_err()
    );
    assert!(
        f.uniform_coverage([0., 0., 1., 1.], &mut Budget::new(1000, &|| true).unwrap())
            .is_err()
    );
    assert!(f.raster_sample(0., 0., 0., &|_, _| [f64::NAN; 3]).is_err());
    assert!(f.raster_sample(0.01, 0.01, 0.01, &color).is_ok());
}
