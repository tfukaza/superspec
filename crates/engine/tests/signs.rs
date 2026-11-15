use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_engine::{
    materialspec::surface::Sample,
    worldspec::themes::kowloon::{
        fixture::Fixture,
        mesh::{Mesh, Mode},
        pen::{Pen, Tint},
        signs::{
            lettering::{self, Direction, Face, Layout, Paint, Style, Surface},
            weathering::{self, Field, Recipe},
        },
        weathered,
    },
};
use superspec_geometry::{Matrix, V3};
#[derive(Deserialize)]
struct Line {
    text: String,
    face: Face,
    direction: Direction,
    width: f64,
    height: f64,
    layout: Layout,
}
#[derive(Deserialize)]
struct Atlas {
    face: Face,
    cells: Vec<Value>,
}
#[derive(Deserialize)]
struct Shadow {
    ink: V3,
    offset: f64,
}
#[derive(Deserialize)]
struct TextStyle {
    face: Face,
    direction: Direction,
    ink: V3,
    lift: f64,
    shadow: Option<Shadow>,
}
impl TextStyle {
    fn style(&self) -> Style {
        Style {
            face: self.face,
            direction: self.direction,
            ink: self.ink,
            lift: self.lift,
            shadow: self.shadow.as_ref().map(|s| (s.ink, s.offset)),
        }
    }
}
#[derive(Deserialize)]
struct Area {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Deserialize)]
struct Point {
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct Geometry {
    indices: Vec<u32>,
    positions: Vec<f64>,
    normals: Vec<f64>,
    colors: Vec<f64>,
    uv: Vec<f64>,
    slots: Vec<String>,
}
#[derive(Deserialize)]
struct Case {
    recipe: Recipe,
    reflected: bool,
    out: V3,
    style: TextStyle,
    area: Area,
    atlas: bool,
    text: String,
    paints: Vec<Paint>,
    samples: Vec<Point>,
    geometry: Geometry,
    #[serde(rename = "surfacePoints")]
    surface_points: Vec<V3>,
}
#[derive(Deserialize)]
struct Golden {
    repeats: BTreeMap<String, f64>,
    layouts: Vec<Line>,
    cells: Vec<Atlas>,
    cases: Vec<Case>,
}
fn golden() -> Golden {
    serde_json::from_str(include_str!("../../../tests/fixtures/rust/signs.json")).unwrap()
}
fn close(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 2e-10,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                close(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, a) in a {
                close(a, &b[k])
            }
        }
        _ => assert_eq!(a, b),
    }
}
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
pub fn check_reference_signs() {
    let golden = golden();
    for l in golden.layouts {
        close(
            &serde_json::to_value(
                lettering::layout_line(&l.text, l.face, l.width, l.height, l.direction).unwrap(),
            )
            .unwrap(),
            &serde_json::to_value(l.layout).unwrap(),
        );
    }
    for a in golden.cells {
        for cell in a.cells {
            close(
                &serde_json::to_value(
                    lettering::glyph_cell(
                        a.face,
                        cell["character"].as_str().unwrap().chars().next().unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap(),
                &cell["cell"],
            );
        }
    }
    for case in golden.cases {
        let field = Field::new(&case.recipe).unwrap();
        for p in &case.samples {
            close(
                &serde_json::to_value(field.sample(p.point[0], p.point[1]).unwrap()).unwrap(),
                &serde_json::to_value(&p.sample).unwrap(),
            );
        }
        let pen = pen(case.reflected);
        let surface = Surface::new(
            &pen,
            [0., -0.05, 0.],
            [case.recipe.width, 0., 0.],
            [0., 0., case.recipe.height],
            case.out,
        )
        .unwrap();
        for ([x, y, lift], expected) in [
            [0., 0., 0.],
            [case.recipe.width, 0., 0.],
            [0., case.recipe.height, 0.],
            [0.3, 0.4, 0.02],
        ]
        .into_iter()
        .zip(case.surface_points)
        {
            close(
                &serde_json::json!(surface.at(x, y, lift)),
                &serde_json::json!(expected),
            );
        }
        let a = &case.area;
        let paints = lettering::letter(
            &case.text,
            [a.x, a.y, a.width, a.height],
            case.style.style(),
            case.atlas,
            golden.repeats["paint"],
        )
        .unwrap();
        close(
            &serde_json::to_value(&paints).unwrap(),
            &serde_json::to_value(&case.paints).unwrap(),
        );
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        for p in paints {
            let points: Vec<_> = p
                .points
                .iter()
                .map(|[x, y]| surface.at(*x, *y, p.lift))
                .collect();
            let uv: Vec<_> = p.points.iter().map(|[x, y]| p.mapping.at(*x, *y)).collect();
            pen.face(
                &mut mesh,
                &p.slot,
                &points,
                surface.out,
                &Tint::Color(p.tint),
                golden.repeats[&p.slot],
                if case.atlas { Some(&uv) } else { None },
                None,
            )
            .unwrap();
        }
        assert_eq!(mesh.indices, case.geometry.indices);
        for (actual, expected) in [
            (
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                case.geometry.positions,
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                case.geometry.normals,
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                case.geometry.colors,
            ),
            (
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                case.geometry.uv,
            ),
        ] {
            close(&serde_json::json!(actual), &serde_json::json!(expected));
        }
        assert_eq!(
            mesh.triangle_surfaces
                .iter()
                .map(|i| mesh.surfaces[*i].slot.clone())
                .collect::<Vec<_>>(),
            case.geometry.slots
        );
        for mode in [Mode::Render, Mode::Survey] {
            let mut mesh = Mesh::new(mode, 150000, &|| false).unwrap();
            weathering::draw(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|s| golden.repeats[s],
                },
                &surface,
                case.recipe.clone(),
                128.,
            )
            .unwrap();
            let triangles = mesh.triangle_count();
            assert_eq!(triangles, case.recipe.paints[0].points.len() - 2);
            let plan = weathered::finish(&mut mesh, &|| false).unwrap();
            if mode == Mode::Render {
                assert_eq!(plan.receivers().len(), 1);
                assert_eq!(plan.receivers()[0].triangle_count, triangles)
            } else {
                assert!(plan.jobs().is_empty())
            }
        }
    }
}
#[test]
fn sign_layout_reader_orientation_and_paint_fields_match_reference() {
    check_reference_signs()
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
    let recipe = golden().cases.remove(0).recipe;
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

#[derive(Deserialize)]
struct Chart {
    triangles: Vec<usize>,
    point: [f64; 2],
    sample: Sample,
}
#[derive(Deserialize)]
struct PanelCase {
    design: superspec_engine::worldspec::themes::kowloon::signs::panel::Design,
    hardware: superspec_engine::worldspec::themes::kowloon::signs::panel::Hardware,
    settings: superspec_engine::worldspec::themes::kowloon::signs::panel::Settings,
    sides: superspec_engine::worldspec::themes::kowloon::signs::panel::Sides,
    reflected: bool,
    charts: Vec<Chart>,
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Panels {
    repeats: BTreeMap<String, f64>,
    cases: Vec<PanelCase>,
}
pub fn check_reference_panels() {
    use superspec_engine::worldspec::themes::kowloon::{
        metal::Metal, signs::panel, weathered::Recipe,
    };
    let golden: Panels = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-panels.json"
    ))
    .unwrap();
    for (index, case) in golden.cases.iter().enumerate() {
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        panel::draw(
            &mut Fixture {
                pen: pen(case.reflected),
                mesh: &mut mesh,
                repeats: &|s| golden.repeats[s],
            },
            &case.design,
            case.sides,
            &case.hardware,
            &case.settings,
        )
        .unwrap();
        assert_eq!(
            mesh.indices.len(),
            case.geometry.indices.len(),
            "panel {index} triangle count"
        );
        assert_eq!(
            mesh.indices
                .iter()
                .zip(&case.geometry.indices)
                .position(|(a, b)| a != b),
            None,
            "panel {index} index order"
        );
        for (name, actual, expected) in [
            (
                "position",
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &case.geometry.positions,
            ),
            (
                "normal",
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &case.geometry.normals,
            ),
            (
                "color",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &case.geometry.colors,
            ),
            (
                "uv",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &case.geometry.uv,
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "panel {index} {name}");
            if let Some(j) = actual
                .iter()
                .zip(expected)
                .position(|(a, b)| (a - b).abs() > 2e-10)
            {
                panic!("panel {index} {name} {j}: {} != {}", actual[j], expected[j])
            }
        }
        assert_eq!(
            mesh.triangle_surfaces
                .iter()
                .map(|i| mesh.surfaces[*i].slot.clone())
                .collect::<Vec<_>>(),
            case.geometry.slots,
            "panel {index} slots"
        );
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut covered = vec![false; case.geometry.indices.len() / 3];
        for receiver in plan.receivers() {
            let chart = case
                .charts
                .iter()
                .find(|c| c.triangles.contains(&receiver.first_triangle))
                .unwrap();
            for (t, seen) in covered
                .iter_mut()
                .enumerate()
                .skip(receiver.first_triangle)
                .take(receiver.triangle_count)
            {
                assert!(chart.triangles.contains(&t));
                assert!(!*seen);
                *seen = true
            }
            let job = &plan.jobs()[receiver.job];
            let [u, v] = chart.point;
            let sample = match &job.recipe {
                Recipe::Sign(r) => Field::new(r).unwrap().sample(u, v),
                Recipe::Metal { settings, .. } => Metal::new(settings, None).unwrap().sample(u, v),
                _ => panic!("Unexpected sign recipe"),
            }
            .unwrap();
            close(
                &serde_json::to_value(sample).unwrap(),
                &serde_json::to_value(&chart.sample).unwrap(),
            );
        }
        assert_eq!(
            covered.iter().filter(|x| **x).count(),
            case.charts.iter().map(|c| c.triangles.len()).sum::<usize>()
        );
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        panel::draw(
            &mut Fixture {
                pen: pen(case.reflected),
                mesh: &mut survey,
                repeats: &|s| golden.repeats[s],
            },
            &case.design,
            case.sides,
            &case.hardware,
            &case.settings,
        )
        .unwrap();
        assert_eq!(
            survey.triangle_count() * 3,
            case.geometry.indices.len(),
            "panel {index} survey"
        );
    }
}
#[test]
fn complete_sign_panels_match_reference() {
    check_reference_panels()
}

#[derive(Deserialize)]
struct PanelMetadata {
    design: superspec_engine::worldspec::themes::kowloon::signs::panel::Design,
    sides: superspec_engine::worldspec::themes::kowloon::signs::panel::Sides,
    matrix: [[f64; 3]; 4],
}
#[derive(Deserialize)]
struct ProposalMetadata {
    key: String,
    mount: superspec_engine::worldspec::themes::kowloon::signs::mounts::Mount,
    form: superspec_engine::worldspec::themes::kowloon::signs::mounts::Form,
    shown: bool,
    above: f64,
    business: Option<superspec_engine::worldspec::themes::kowloon::signs::copy::Business>,
    steel: superspec_engine::worldspec::themes::kowloon::signs::panel::Hardware,
    panels: Vec<PanelMetadata>,
}
#[derive(Deserialize)]
struct MountCase {
    room: Option<superspec_engine::worldspec::themes::kowloon::signs::mounts::PierRoom>,
    top: Option<f64>,
    detail: Option<u8>,
    wash: Option<[f64; 3]>,
    seed: String,
    category: String,
    kind: superspec_engine::worldspec::themes::kowloon::signs::mounts::FlatKind,
    rect: superspec_engine::worldspec::themes::kowloon::signs::mounts::WallRect,
    span: [f64; 2],
    base: f64,
    ceiling: f64,
    business: superspec_engine::worldspec::themes::kowloon::signs::copy::Business,
    settings: superspec_engine::worldspec::themes::kowloon::signs::panel::Settings,
    reflected: bool,
    shown: bool,
    next: f64,
    proposal: ProposalMetadata,
    charts: Vec<Chart>,
    geometry: Geometry,
}
#[derive(Deserialize)]
struct Mounts {
    repeats: BTreeMap<String, f64>,
    cases: Vec<MountCase>,
}
pub fn check_reference_mounts() {
    let golden: Mounts = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-mounts.json"
    ))
    .unwrap();
    check_mounts(golden);
}
pub fn check_reference_projecting() {
    check_mounts(
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/rust/sign-projecting.json"
        ))
        .unwrap(),
    );
}
fn check_mounts(golden: Mounts) {
    use superspec_aml::rng::SeededRng;
    use superspec_engine::worldspec::themes::kowloon::{
        metal::Metal, signs::mounts, weathered::Recipe,
    };
    for (i, c) in golden.cases.into_iter().enumerate() {
        let site = mounts::Site {
            detail: c.detail.unwrap_or(1),
            wash: c.wash,
            pen: pen(c.reflected),
            key: "sign/site".into(),
            elevation: 2.8,
            shown: c.shown,
        };
        let mut rng = SeededRng::from_text(&c.seed);
        let p = match c.category.as_str() {
            "blade" | "hung" => {
                let make = if c.category == "blade" {
                    mounts::blade_sign
                } else {
                    mounts::hung_sign
                };
                let p = make(&site, c.room.unwrap(), &c.business, &c.settings, &mut rng)
                    .unwrap()
                    .unwrap();
                assert_eq!(Some(p.top), c.top, "mount {i} top");
                p.proposal
            }
            "flat" => mounts::flat_sign(
                &site,
                c.rect,
                c.kind,
                0.13,
                &c.business,
                &c.settings,
                &mut rng,
            )
            .unwrap(),
            "painted" => {
                mounts::painted_sign(&site, c.rect, &c.business, &c.settings, &mut rng).unwrap()
            }
            "roof" => mounts::roof_sign(
                &site,
                c.span,
                c.base,
                c.ceiling,
                &c.business,
                &c.settings,
                &mut rng,
            )
            .unwrap()
            .unwrap(),
            _ => panic!("unknown mount"),
        };
        assert_eq!(rng.next_f64(), c.next, "mount {i} continuation");
        let expected = &c.proposal;
        assert_eq!(p.key, expected.key);
        assert_eq!(p.mount, expected.mount);
        assert_eq!(p.form, expected.form);
        assert_eq!(p.shown, expected.shown);
        assert_eq!(p.above, expected.above);
        close(
            &serde_json::to_value(&p.business).unwrap(),
            &serde_json::to_value(&expected.business).unwrap(),
        );
        close(
            &serde_json::to_value(&p.steel).unwrap(),
            &serde_json::to_value(&expected.steel).unwrap(),
        );
        assert_eq!(p.panels.len(), expected.panels.len());
        for (a, b) in p.panels.iter().zip(&expected.panels) {
            assert_eq!(a.sides, b.sides);
            close(
                &serde_json::to_value(&a.design).unwrap(),
                &serde_json::to_value(&b.design).unwrap(),
            );
            close(
                &serde_json::json!(a.pen.matrix.0),
                &serde_json::json!(b.matrix),
            )
        }
        let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
        p.draw(&mut mesh, &|s| golden.repeats[s], &c.settings)
            .unwrap();
        assert_eq!(
            mesh.indices.len(),
            c.geometry.indices.len(),
            "mount {i} count"
        );
        assert_eq!(
            mesh.indices
                .iter()
                .zip(&c.geometry.indices)
                .position(|(a, b)| a != b),
            None,
            "mount {i} index order"
        );
        for (name, actual, expected) in [
            (
                "position",
                mesh.vertices
                    .iter()
                    .flat_map(|v| v.position)
                    .collect::<Vec<_>>(),
                &c.geometry.positions,
            ),
            (
                "normal",
                mesh.vertices.iter().flat_map(|v| v.normal).collect(),
                &c.geometry.normals,
            ),
            (
                "color",
                mesh.vertices.iter().flat_map(|v| v.color).collect(),
                &c.geometry.colors,
            ),
            (
                "uv",
                mesh.vertices.iter().flat_map(|v| v.uv).collect(),
                &c.geometry.uv,
            ),
        ] {
            assert_eq!(actual.len(), expected.len(), "mount {i} {name}");
            if let Some(j) = actual
                .iter()
                .zip(expected)
                .position(|(a, b)| (a - b).abs() > 2e-10)
            {
                panic!("mount {i} {name} {j}: {} != {}", actual[j], expected[j])
            }
        }
        assert_eq!(
            mesh.triangle_surfaces
                .iter()
                .map(|i| mesh.surfaces[*i].slot.clone())
                .collect::<Vec<_>>(),
            c.geometry.slots
        );
        let plan = weathered::finish(&mut mesh, &|| false).unwrap();
        let mut covered = vec![false; c.geometry.indices.len() / 3];
        for receiver in plan.receivers() {
            let chart = c
                .charts
                .iter()
                .find(|c| c.triangles.contains(&receiver.first_triangle))
                .unwrap();
            for (t, seen) in covered
                .iter_mut()
                .enumerate()
                .skip(receiver.first_triangle)
                .take(receiver.triangle_count)
            {
                assert!(chart.triangles.contains(&t));
                assert!(!*seen);
                *seen = true
            }
            let [u, v] = chart.point;
            let sample = match &plan.jobs()[receiver.job].recipe {
                Recipe::Sign(r) => Field::new(r).unwrap().sample(u, v),
                Recipe::Metal { settings, .. } => Metal::new(settings, None).unwrap().sample(u, v),
                _ => panic!("Unexpected mount finish"),
            }
            .unwrap();
            close(
                &serde_json::to_value(sample).unwrap(),
                &serde_json::to_value(&chart.sample).unwrap(),
            );
        }
        assert_eq!(
            covered.iter().filter(|x| **x).count(),
            c.charts.iter().map(|c| c.triangles.len()).sum::<usize>()
        );
        let mut survey = Mesh::new(Mode::Survey, 150000, &|| false).unwrap();
        p.draw(&mut survey, &|s| golden.repeats[s], &c.settings)
            .unwrap();
        assert_eq!(
            survey.triangle_count() * 3,
            c.geometry.indices.len(),
            "mount {i} survey"
        );
    }
}
#[test]
fn complete_sign_mounts_match_reference() {
    check_reference_mounts()
}
#[test]
fn projecting_supports_panels_and_seeded_choices_match_reference() {
    check_reference_projecting()
}
#[test]
fn projecting_sign_clearance_and_cancellation_are_explicit() {
    use superspec_aml::rng::SeededRng;
    use superspec_engine::worldspec::themes::kowloon::signs::mounts;
    let cases: Mounts = serde_json::from_str(include_str!(
        "../../../tests/fixtures/rust/sign-projecting.json"
    ))
    .unwrap();
    let c = &cases.cases[2];
    let site = mounts::Site {
        pen: pen(false),
        key: "site".into(),
        shown: true,
        elevation: 0.,
        detail: 1,
        wash: Some([0.5; 3]),
    };
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
                &c.business,
                &c.settings,
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
                &c.business,
                &c.settings,
                &mut rng
            )
            .is_err()
        );
    }
    assert_eq!(rng.next_f64(), before);
    let p = mounts::blade_sign(
        &site,
        c.room.unwrap(),
        &c.business,
        &c.settings,
        &mut SeededRng::from_text(&c.seed),
    )
    .unwrap()
    .unwrap()
    .proposal;
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| true).unwrap();
    assert!(
        p.draw(&mut mesh, &|s| cases.repeats[s], &c.settings)
            .is_err()
    );
    assert!(mesh.indices.is_empty());
    let mut mesh = Mesh::new(Mode::Render, 150000, &|| false).unwrap();
    p.draw(&mut mesh, &|s| cases.repeats[s], &c.settings)
        .unwrap();
    assert!(
        !weathered::finish(&mut mesh, &|| false)
            .unwrap()
            .jobs()
            .is_empty()
    );
}
