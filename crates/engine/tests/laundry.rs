use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    fixture::Fixture,
    laundry::{self, Garment, Room, Support},
    mesh::{Mesh, Mode},
    pen::Pen,
};
use superspec_geometry::{Matrix, V3};
fn pen(reflected: bool) -> Pen {
    Pen::new(
        if reflected {
            Matrix([[-1., 0., 0.], [0., -1., 0.], [0., 0., -1.], [4., 3., 5.]])
        } else {
            Matrix::default()
        },
        "prop",
    )
    .unwrap()
}
const LEFT: V3 = [-0.4, 0.2, 2.4];
const RIGHT: V3 = [1.1, 0.8, 2.7];
fn hang(mode: Mode, garment: &Garment, reflected: bool) -> usize {
    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
    laundry::hang(
        &mut Fixture {
            pen: pen(reflected),
            mesh: &mut mesh,
            repeats: &|_| 1.,
        },
        garment,
        LEFT,
        RIGHT,
        0.05,
        true,
    )
    .unwrap();
    mesh.triangle_count()
}
/// Draws a row and returns its triangle count and the RNG continuation.
fn row(mode: Mode, support: Support, seed: &str, reflected: bool) -> (usize, u32) {
    let mut rng = SeededRng::from_text(seed);
    let mut mesh = Mesh::new(mode, 100000, &|| false).unwrap();
    let mut draw = Fixture {
        pen: pen(reflected),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    if support == Support::Hanger {
        laundry::hangers(
            &mut draw,
            &laundry::HangerRow {
                span: [-0.4, 2.4],
                z: 2.5,
                slope: 0.1,
                reach: 0.6,
                drop: 1.5,
            },
            &mut rng,
            true,
        )
        .unwrap();
    } else {
        laundry::washing(
            &mut draw,
            [-0.4, 2.4],
            2.5,
            &mut rng,
            &laundry::WashingOptions {
                fill: 0.85,
                drop: 1.5,
                support,
            },
            true,
        )
        .unwrap();
    }
    (mesh.triangle_count(), rng.next_u32())
}
#[test]
fn fitted_laundry_stays_inside_its_room_and_surveys_like_render() {
    let mut hung = 0;
    for support in [Support::Line, Support::Pole, Support::Hanger] {
        for i in 0..24 {
            let room = Room {
                width: 0.5 + 0.08 * i as f64,
                drop: 0.6 + 0.08 * i as f64,
            };
            let mut rng = SeededRng::from_text(&format!("laundry/{i}"));
            let Some(g) = laundry::choose(&mut rng, room, support).unwrap() else {
                continue;
            };
            assert!(g.width() <= room.width + 1e-12, "{} width", g.kind());
            assert!(g.drop() <= room.drop + 1e-12, "{} drop", g.kind());
            let render = hang(Mode::Render, &g, false);
            assert!(render > 0, "{}", g.kind());
            assert_eq!(hang(Mode::Survey, &g, false), render, "{} survey", g.kind());
            assert_eq!(
                hang(Mode::Render, &g, true),
                render,
                "{} mirrored",
                g.kind()
            );
            hung += 1;
        }
    }
    assert!(hung > 0);
}
#[test]
fn washing_rows_survey_like_render_and_consume_the_same_stream() {
    for support in [Support::Line, Support::Pole, Support::Hanger] {
        for seed in ["row/0", "row/1", "row/2"] {
            let render = row(Mode::Render, support, seed, false);
            assert!(render.0 > 0);
            assert_eq!(row(Mode::Survey, support, seed, false), render);
            assert_eq!(row(Mode::Render, support, seed, true), render);
        }
    }
}
#[test]
fn invalid_laundry_preserves_rng_and_next_draw_recovers() {
    let mut rng = SeededRng::from_text("recovery");
    let mut untouched = rng.clone();
    assert!(
        laundry::choose(
            &mut rng,
            Room {
                width: f64::NAN,
                drop: 1.
            },
            Support::Line
        )
        .is_err()
    );
    assert_eq!(rng.next_u32(), untouched.next_u32());
    let garment = laundry::choose(
        &mut rng,
        Room {
            width: 1.5,
            drop: 1.6,
        },
        Support::Line,
    )
    .unwrap()
    .unwrap();
    let mut mesh = Mesh::new(Mode::Render, 100000, &|| false).unwrap();
    let mut draw = Fixture {
        pen: pen(false),
        mesh: &mut mesh,
        repeats: &|_| 1.,
    };
    assert!(
        laundry::hang(
            &mut draw,
            &garment,
            [f64::NAN, 0., 0.],
            [1., 0., 0.],
            0.,
            true
        )
        .is_err()
    );
    assert_eq!(draw.mesh.triangle_count(), 0);
    laundry::hang(&mut draw, &garment, [0., 0., 2.], [1., 0., 2.], 0., true).unwrap();
    assert!(mesh.triangle_count() > 0);
    let mut cancelled = Mesh::new(Mode::Render, 100000, &|| true).unwrap();
    assert!(
        laundry::hang(
            &mut Fixture {
                pen: pen(false),
                mesh: &mut cancelled,
                repeats: &|_| 1.
            },
            &garment,
            [0., 0., 2.],
            [1., 0., 2.],
            0.,
            true
        )
        .is_err()
    );
}
