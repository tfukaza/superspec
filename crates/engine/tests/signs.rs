use serde_json::json;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    mesh::{Mesh, Mode},
    pen::Pen,
    signs::{
        copy::Business,
        lettering::{self, Direction, Face, Surface},
        mounts::{self, Proposal},
        panel,
        weathering::{self, Field, Recipe},
    },
    weathered,
};
use superspec_geometry::Matrix;
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "sign",
    )
    .unwrap()
}
fn repeats(slot: &str) -> f64 {
    match slot {
        "board" => 1.3,
        "paint" | "timber" => 1.2,
        "rust" => 1.7,
        "metal" | "sheet" => 1.5,
        "wall" => 1.3,
        _ => 1.,
    }
}
/// A diamond board with a painted field and one lettering quad.
fn recipe() -> Recipe {
    serde_json::from_value(json!({
        "width": 2.8, "height": 1.1, "construction": "sheet", "outline": "diamond",
        "age": 0, "phase": 0.125, "grime": 0.6, "wear": 0.25, "rust": 0.4,
        "underpaint": "paint", "oxide": "rust",
        "paints": [
            {"slot": "board", "points": [[1.4, 0], [2.8, 0.55], [1.4, 1.1], [0, 0.55]], "lift": 0,
             "tint": [0.3, 0.5, 0.1],
             "mapping": {"origin": [0.481, 0.793], "u": [1.3, 0], "v": [0, 1.3]}},
            {"slot": "paint", "points": [[0.2, 0.2], [2.6, 0.2], [2.6, 0.9], [0.2, 0.9]], "lift": 0.003,
             "tint": [0.9, 0.1, 0.2],
             "mapping": {"origin": [0, 0], "u": [1.2, 0], "v": [0, 1.2]}},
            {"slot": "lettering", "points": [[0.25, 0.2], [0.83, 0.2], [0.83, 0.85], [0.25, 0.85]],
             "lift": 0.005, "tint": [0.9, 0.8, 0.6],
             "mapping": {"origin": [0.13, 0.91], "u": [0.0345, 0], "v": [0, 0.0304]}}
        ]
    }))
    .unwrap()
}
fn settings(seed: &str) -> panel::Settings {
    serde_json::from_value(json!({
        "weathering": {"seed": seed, "wear": 0.25, "grime": 0.6, "rust": 0.4, "textureDensity": 32,
            "metal": {"material": "metal", "repeat": 1.5},
            "paint": {"material": "paint", "repeat": 1.2},
            "sheet": {"material": "sheet", "repeat": 1.5}},
        "lettering": true,
        "timber": {"textured": false, "timberRepeat": 1.2, "atlasRepeat": 1}
    }))
    .unwrap()
}
fn business() -> Business {
    serde_json::from_value(json!({
        "name": "黃記", "trade": "洋服", "english": "WONG KEE TAILOR",
        "tagline": "冷氣開放", "phone": "3-596072", "sector": "service"
    }))
    .unwrap()
}
/// Receivers never overlap and stay inside the mesh; survey emits the same triangles as render.
fn check_receivers(mesh: &mut Mesh<'_>) {
    let triangles = mesh.triangle_count();
    let plan = weathered::finish(mesh, &|| false).unwrap();
    let mut covered = vec![false; triangles];
    for r in plan.receivers() {
        assert!(r.first_triangle + r.triangle_count <= triangles);
        for seen in &mut covered[r.first_triangle..r.first_triangle + r.triangle_count] {
            assert!(!*seen, "receivers overlap");
            *seen = true;
        }
        assert!(r.job < plan.jobs().len());
    }
}
/// Draws a proposal and hands the mesh to `inspect`.
fn with_drawn<R>(
    p: &Proposal,
    mode: Mode,
    settings: &panel::Settings,
    inspect: impl FnOnce(&mut Mesh<'_>) -> R,
) -> R {
    let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
    p.draw(&mut mesh, &repeats, settings).unwrap();
    inspect(&mut mesh)
}
fn triangles(p: &Proposal, mode: Mode, settings: &panel::Settings) -> usize {
    with_drawn(p, mode, settings, |m| m.triangle_count())
}
#[test]
fn sign_surface_maps_board_corners_onto_its_axes() {
    let pen = pen(false);
    let surface = Surface::new(
        &pen,
        [0., -0.05, 0.],
        [2.8, 0., 0.],
        [0., 0., 1.1],
        [0., -1., 0.],
    )
    .unwrap();
    for ([x, y, lift], expected) in [
        ([0., 0., 0.], [0., -0.05, 0.]),
        ([2.8, 0., 0.], [2.8, -0.05, 0.]),
        ([0., 1.1, 0.], [0., -0.05, 1.1]),
        ([0.3, 0.4, 0.02], [0.3, -0.07, 0.4]),
    ] {
        let got = surface.at(x, y, lift);
        for (a, b) in got.iter().zip(expected) {
            assert!((a - b).abs() < 1e-12, "{got:?} != {expected:?}");
        }
    }
}
#[test]
fn weathered_board_fans_its_outline_into_one_render_receiver_and_no_survey_jobs() {
    let recipe = recipe();
    let pen = pen(false);
    let surface = Surface::new(
        &pen,
        [0., -0.05, 0.],
        [recipe.width, 0., 0.],
        [0., 0., recipe.height],
        [0., -1., 0.],
    )
    .unwrap();
    for mode in [Mode::Render, Mode::Survey] {
        let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
        weathering::draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &repeats,
            },
            &surface,
            recipe.clone(),
            128.,
        )
        .unwrap();
        let triangles = mesh.triangle_count();
        assert_eq!(triangles, recipe.paints[0].points.len() - 2);
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        if mode == Mode::Render {
            assert_eq!(plan.receivers().len(), 1);
            assert_eq!(plan.receivers()[0].triangle_count, triangles)
        } else {
            assert!(plan.jobs().is_empty())
        }
    }
}
#[test]
fn sign_admission_and_cancelled_drawing_recover() {
    for dimensions in [[0., 1.], [f64::NAN, 1.], [1., 101.]] {
        assert!(
            lettering::layout_line(
                "TEST",
                Face::Hei,
                dimensions[0],
                dimensions[1],
                Direction::Ltr
            )
            .is_err()
        )
    }
    assert!(lettering::layout_line(&"A".repeat(2049), Face::Hei, 1., 1., Direction::Ltr).is_err());
    let recipe = recipe();
    assert!(Field::new(&recipe).is_ok());
    for change in 0..5 {
        let mut bad = recipe.clone();
        match change {
            0 => bad.paints[0].points.reverse(),
            1 => bad.phase = f64::NAN,
            2 => bad.paints[0].points[1] = bad.paints[0].points[0],
            3 => bad.paints[0].mapping.u[0] = f64::INFINITY,
            _ => bad.paints = vec![bad.paints[0].clone(); 513],
        };
        assert!(Field::new(&bad).is_err())
    }
    let pen = pen(false);
    let surface = Surface::new(
        &pen,
        [0.; 3],
        [recipe.width, 0., 0.],
        [0., 0., recipe.height],
        [0., 1., 0.],
    )
    .unwrap();
    for cancelled in [true, false] {
        let stop = || cancelled;
        let mut mesh = Mesh::new(Mode::Render, 150000, &stop).unwrap();
        let result = weathering::draw(
            &mut Fixture {
                pen: pen.clone(),
                mesh: &mut mesh,
                repeats: &|_| 1.,
            },
            &surface,
            recipe.clone(),
            128.,
        );
        assert_eq!(result.is_err(), cancelled);
    }
}
#[test]
fn panels_survey_like_render_mirror_without_changing_topology_and_keep_receivers_disjoint() {
    let design: panel::Design = serde_json::from_value(json!({
        "width": 2.8, "height": 1.1, "depth": 0.08, "construction": "sheet", "outline": "rectangle",
        "ground": [0.32, 0.12, 0.08], "border": [0.85, 0.7, 0.5], "stack": "across",
        "blocks": [
            {"text": "永發茶餐廳", "face": "kai", "direction": "ttb", "ink": [0.9, 0.8, 0.6],
             "share": 2, "shadow": [0.04, 0.08, 0.1]},
            {"text": "PEARL 98", "face": "hei", "direction": "ltr", "ink": [0.1, 0.3, 0.1], "share": 1}
        ],
        "age": 0.8, "phase": 0.125
    }))
    .unwrap();
    let hardware: panel::Hardware =
        serde_json::from_value(json!({"slot": "metal", "tint": [0.25, 0.3, 0.28]})).unwrap();
    let settings = settings("sign-panel/0");
    let draw = |mode, sides, reflected| {
        let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
        panel::draw(
            &mut Fixture {
                pen: pen(reflected),
                mesh: &mut mesh,
                repeats: &repeats,
            },
            &design,
            sides,
            &hardware,
            &settings,
        )
        .unwrap();
        mesh
    };
    let front = draw(Mode::Render, panel::Sides::Front, false).triangle_count();
    assert!(front > 0);
    for sides in [panel::Sides::Front, panel::Sides::Both] {
        let mut render = draw(Mode::Render, sides, false);
        let count = render.triangle_count();
        assert_eq!(draw(Mode::Survey, sides, false).triangle_count(), count);
        assert_eq!(draw(Mode::Render, sides, true).triangle_count(), count);
        if sides == panel::Sides::Both {
            assert!(count > front, "a second face adds geometry");
        }
        check_receivers(&mut render);
    }
}
#[test]
fn seeded_mounts_are_repeatable_and_survey_like_render() {
    let site = mounts::Site {
        detail: 1,
        wash: None,
        pen: pen(false),
        key: "sign/site".into(),
        elevation: 2.8,
        shown: true,
    };
    let settings = settings("sign-mounts/0");
    let business = business();
    let rect: mounts::WallRect =
        serde_json::from_value(json!({"x0": 0.2, "z0": 2.6, "width": 2.8, "height": 0.7})).unwrap();
    let make = |seed: &str| -> Vec<Proposal> {
        let mut rng = SeededRng::from_text(seed);
        vec![
            mounts::flat_sign(
                &site,
                rect,
                mounts::FlatKind::Fascia,
                0.13,
                &business,
                &settings,
                &mut rng,
            )
            .unwrap(),
            mounts::painted_sign(&site, rect, &business, &settings, &mut rng).unwrap(),
            mounts::roof_sign(&site, [0.3, 6.8], 0.7, 3.6, &business, &settings, &mut rng)
                .unwrap()
                .unwrap(),
        ]
    };
    let first = make("mount/0");
    let again = make("mount/0");
    for (a, b) in first.iter().zip(&again) {
        assert_eq!(a.key, b.key);
        assert_eq!(
            serde_json::to_value(&a.business).unwrap(),
            serde_json::to_value(&b.business).unwrap()
        );
        assert_eq!(a.panels.len(), b.panels.len());
        let indices = with_drawn(a, Mode::Render, &settings, |m| {
            check_receivers(m);
            m.indices.clone()
        });
        assert!(!indices.is_empty());
        assert_eq!(
            with_drawn(b, Mode::Render, &settings, |m| m.indices.clone()),
            indices
        );
        assert_eq!(triangles(a, Mode::Survey, &settings) * 3, indices.len());
    }
    // Narrow roofs have no room for a hoarding and inverted spans are rejected.
    let mut rng = SeededRng::from_text("mount/narrow");
    assert!(
        mounts::roof_sign(&site, [0.3, 2.], 0.7, 3.6, &business, &settings, &mut rng)
            .unwrap()
            .is_none()
    );
    assert!(mounts::roof_sign(&site, [2., 0.3], 0.7, 3.6, &business, &settings, &mut rng).is_err());
}
#[test]
fn projecting_signs_stay_within_their_pier_room() {
    let site = mounts::Site {
        detail: 1,
        wash: Some([0.5; 3]),
        pen: pen(false),
        key: "sign/site".into(),
        elevation: 2.8,
        shown: true,
    };
    let settings = settings("sign-projecting/2");
    for (i, room) in [
        mounts::PierRoom {
            u: 0.7,
            foot: 2.2,
            ceiling: 4.1,
            reach: 0.9,
        },
        mounts::PierRoom {
            u: 0.7,
            foot: 2.2,
            ceiling: 6.8,
            reach: 1.5,
        },
    ]
    .into_iter()
    .enumerate()
    {
        for make in [mounts::blade_sign, mounts::hung_sign] {
            let mut rng = SeededRng::from_text(&format!("mount/{i}"));
            let Some(p) = make(&site, room, &business(), &settings, &mut rng).unwrap() else {
                continue;
            };
            assert!(
                p.top <= room.ceiling + 1e-9 && p.top > room.foot,
                "top {}",
                p.top
            );
            let render = with_drawn(&p.proposal, Mode::Render, &settings, |m| {
                let count = m.triangle_count();
                check_receivers(m);
                count
            });
            assert!(render > 0);
            assert_eq!(triangles(&p.proposal, Mode::Survey, &settings), render);
        }
    }
}
#[test]
fn projecting_sign_clearance_and_cancellation_are_explicit() {
    let site = mounts::Site {
        pen: pen(false),
        key: "site".into(),
        shown: true,
        elevation: 0.,
        detail: 1,
        wash: Some([0.5; 3]),
    };
    let business = business();
    let settings = settings("sign-projecting/2");
    let mut rng = SeededRng::from_text("limits");
    let before = rng.clone().next_f64();
    for make in [mounts::blade_sign, mounts::hung_sign] {
        assert!(
            make(
                &site,
                mounts::PierRoom {
                    u: 0.,
                    foot: 0.,
                    ceiling: 1.,
                    reach: 0.5
                },
                &business,
                &settings,
                &mut rng
            )
            .unwrap()
            .is_none()
        );
        assert!(
            make(
                &site,
                mounts::PierRoom {
                    u: f64::NAN,
                    foot: 0.,
                    ceiling: 4.,
                    reach: 1.
                },
                &business,
                &settings,
                &mut rng
            )
            .is_err()
        );
    }
    assert_eq!(rng.next_f64(), before);
    let p = mounts::blade_sign(
        &site,
        mounts::PierRoom {
            u: 0.7,
            foot: 2.2,
            ceiling: 6.8,
            reach: 1.5,
        },
        &business,
        &settings,
        &mut SeededRng::from_text("mount/2"),
    )
    .unwrap()
    .unwrap()
    .proposal;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(p.draw(&mut mesh, &repeats, &settings).is_err());
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    p.draw(&mut mesh, &repeats, &settings).unwrap();
    assert!(
        !weathered::finish(&mut mesh, &|| false)
            .unwrap()
            .jobs()
            .is_empty()
    );
}
