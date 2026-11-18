use super::{Settings, on};
use crate::worldspec::themes::kowloon::{
    facade_cells::{Kind, Role},
    facade_plan::{Plan, Queries, Request, Zone, inside},
    fixture::Fixture,
    household,
    massing::Rect,
    palette::{shade, srgb},
    pipes::{Options, PipeSystem},
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
struct Drain<'a> {
    r: &'a Request<'a>,
    p: &'a Plan,
    q: &'a Queries<'a>,
    s: &'a Settings<'a>,
    iron: V3,
}
impl Drain<'_> {
    fn pipe(&self, radius: f64) -> Result<PipeSystem, String> {
        PipeSystem::new(Options {
            radius,
            color: self.iron,
            sides: 16,
            variant: "pipe-standard".into(),
            weathering: Some(self.s.kit.settings().weathering.clone()),
        })
    }
    fn drop(&self, d: &mut Fixture<'_, '_>, u: f64, radius: f64, top: f64) -> Result<bool, String> {
        let r = self.r;
        let ground = r.lot.ground;
        let body = Rect {
            u0: u - 0.1,
            u1: u + 0.1,
            v0: ground,
            v1: top,
        };
        let base = Rect {
            v0: 0.,
            v1: ground,
            ..body
        };
        if !inside(r.body, body)
            || !inside(r.ground, base)
            || self.q.reach(Zone::Ground, base, 0.15)? == 0.
            || self.q.reach(Zone::Body, body, 0.15)? == 0.
        {
            return Ok(false);
        }
        let y = radius + 0.03;
        let blocked = self
            .p
            .bays
            .iter()
            .filter_map(|b| b.room.as_ref())
            .any(|s| u + radius > s.x0 - 0.06 && u - radius < s.x1 + 0.06);
        let mut pen = d.pen.into_part("fixtures")?;
        let path = if blocked {
            pen = pen.moved([0., r.shift, 0.])?;
            vec![
                [u, y, top],
                [u, y, ground + 0.16],
                [u, 0.005, ground + 0.08],
            ]
        } else if r.shift.abs() < 1e-6 {
            vec![[u, y, top], [u, y, 0.1]]
        } else if r.shift > 0. {
            pen = pen.moved([0., r.shift, 0.])?;
            vec![
                [u, y, top],
                [u, y, ground + 0.2],
                [u, y - r.shift, ground - 0.2],
                [u, y - r.shift, 0.1],
            ]
        } else {
            pen = pen.moved([0., r.shift, 0.])?;
            vec![[u, y, top], [u, y, ground + 0.08]]
        };
        self.pipe(radius)?.run(&mut on(d, pen), &path, Some(0.))?;
        Ok(true)
    }
}
pub(super) fn draw(
    d: &mut Fixture<'_, '_>,
    r: &Request<'_>,
    p: &Plan,
    s: &Settings<'_>,
    q: &Queries<'_>,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let l = r.lot;
    let width = r.width;
    let body = l.top - l.ground > 0.05;
    let trim = l.style.trim;
    let fixtures = d.pen.into_part("fixtures")?;
    if r.role == Role::Front && l.kind == "tenement" && body {
        let rect = Rect {
            u0: 0.,
            u1: width,
            v0: l.ground - 0.2,
            v1: l.ground,
        };
        if r.shift == 0. && l.style.canopy && inside(r.ground, rect) {
            let reach = q.reach(Zone::Ground, rect, 0.75)?;
            if reach >= 0.4 {
                on(d, fixtures.clone()).cuboid(
                    "wall",
                    [0., 0., l.ground - 0.13],
                    [width, reach, l.ground],
                    trim,
                    "b",
                )?;
            }
        }
        if r.shift >= 0.25 {
            for u in [0.18, width - 0.18] {
                if q.reach(
                    Zone::Ground,
                    Rect {
                        u0: u - 0.12,
                        u1: u + 0.12,
                        v0: l.ground - 0.3,
                        v1: l.ground,
                    },
                    r.shift,
                )? >= r.shift - 1e-6
                {
                    on(d, fixtures.clone()).cuboid(
                        "wall",
                        [u - 0.1, 0., l.ground - 0.28],
                        [u + 0.1, r.shift, l.ground],
                        trim,
                        "bu",
                    )?;
                }
            }
        }
    }
    if !body {
        return Ok(());
    }
    if matches!(r.role, Role::Side | Role::Back)
        && s.kit
            .option(&format!("{}/industrial-riser", r.key), 0.8)?
            .shown
        && d.mesh.has_source_material("vent-metal")
    {
        let pen = fixtures
            .into_part(&format!("{}/industrial-riser", r.key))?
            .moved([0., r.shift, 0.])?;
        industrial(&mut on(d, pen), r, p, q)?;
    }
    let iron = rng.pick(&[
        srgb("#3a3c3a"),
        shade(l.style.wall, 0.6),
        household::metal_color(srgb("#a4a9aa"), 0.4, Some(&s.kit.settings().weathering)),
    ]);
    let drain = Drain { r, p, q, s, iron };
    let body_fixtures = fixtures.moved([0., r.shift, 0.])?;
    if r.role != Role::Side || rng.chance(0.5) {
        let order = if rng.chance(0.5) {
            [0.16, width - 0.16]
        } else {
            [width - 0.16, 0.16]
        };
        let clear: Vec<_> = order
            .into_iter()
            .filter(|u| {
                !p.columns
                    .iter()
                    .any(|c| c.kind == Kind::Service && *u > c.u0 && *u < c.u1)
            })
            .collect();
        for u in if clear.is_empty() {
            order.to_vec()
        } else {
            clear
        } {
            if drain.drop(d, u, 0.05, l.roof - 0.1)? {
                on(d, body_fixtures.clone()).cuboid(
                    "paint",
                    [u - 0.1, 0., l.roof - 0.12],
                    [u + 0.1, 0.16, l.roof + 0.08],
                    iron,
                    "b",
                )?;
                let supply = rng.chance(0.6);
                if s.kit.detail() > 0 && supply {
                    drain.drop(
                        d,
                        u + if u < width / 2. { 0.16 } else { -0.16 },
                        0.022,
                        l.roof - 0.4,
                    )?;
                }
                break;
            }
        }
    }
    if s.kit.detail() == 0 {
        return Ok(());
    }
    for c in &p.columns {
        if c.kind != Kind::Service || c.u0 < 0. || c.u1 > width {
            continue;
        }
        let u = c.u0 + 0.14;
        if !drain.drop(d, u, 0.055, l.roof - 0.1)? {
            continue;
        }
        let mut floor = l.ground + 0.5;
        while floor < l.roof - 1. {
            d.mesh.spend_work(1)?;
            drain.pipe(0.03)?.run(
                &mut on(d, body_fixtures.clone()),
                &[
                    [u, 0.085, floor],
                    [u + 0.5_f64.min(c.u1 - c.u0 - 0.3), 0.085, floor + 0.12],
                ],
                Some(0.),
            )?;
            floor += l.story_height;
        }
    }
    Ok(())
}

fn industrial(
    d: &mut Fixture<'_, '_>,
    r: &Request<'_>,
    p: &Plan,
    q: &Queries<'_>,
) -> Result<(), String> {
    let height = (r.lot.roof - r.lot.ground - 0.8).min(5.2);
    if height < 4.4 {
        return Ok(());
    }
    let overlaps = |a: Rect, b: Rect, gap: f64| {
        a.u0 < b.u1 + gap && a.u1 > b.u0 - gap && a.v0 < b.v1 + gap && a.v1 > b.v0 - gap
    };
    for bay in p
        .columns
        .iter()
        .filter(|bay| bay.kind == Kind::Blank && bay.u1 - bay.u0 >= 1.88)
    {
        let x = (bay.u0 + bay.u1) / 2.;
        for step in 0..=20 {
            let top = r.lot.roof - 0.26 - step as f64 * 0.1;
            let bottom = top - height;
            let area = Rect {
                u0: x - 0.86,
                u1: x + 0.86,
                v0: bottom - 0.045,
                v1: top + 0.045,
            };
            let contact = |v0, v1| {
                !p.construction
                    .bands
                    .iter()
                    .any(|band| overlaps(Rect { v0, v1, ..area }, *band, 0.035))
            };
            if bottom < r.lot.ground + 0.4
                || !inside(r.body, area)
                || p.construction
                    .outlets
                    .iter()
                    .any(|outlet| overlaps(area, *outlet, 0.1))
                || !contact(bottom - 0.04, bottom + 0.82)
                || !contact(top - 0.67, top + 0.04)
                || !contact(bottom + 1.14, bottom + 1.4)
                || !contact(top - 1.43, top - 1.17)
                || q.reach(Zone::Body, area, 1.08)? < 1.08 - 1e-6
            {
                continue;
            }
            draw_duct(d, x, bottom, top)?;
            return Ok(());
        }
    }
    Ok(())
}

pub(crate) fn draw_duct(
    d: &mut Fixture<'_, '_>,
    x: f64,
    bottom: f64,
    top: f64,
) -> Result<(), String> {
    let steel = "vent-metal";
    let lo = bottom + 1.05;
    let hi = top - 0.72;
    for profile in [
        vec![
            [0.005, bottom],
            [0.3, bottom],
            [0.6, bottom + 0.12],
            [0.94, bottom + 0.62],
            [0.94, lo],
            [0.32, lo],
            [0.32, bottom + 0.78],
            [0.005, bottom + 0.78],
        ],
        vec![
            [0.005, top],
            [0.35, top],
            [0.94, top - 0.38],
            [0.94, hi],
            [0.32, hi],
            [0.32, top - 0.62],
            [0.005, top - 0.62],
        ],
    ] {
        let side_profile = superspec_geometry::profile::triangulate(
            vec![profile.iter().map(|[y, z]| [*y, *z, 0.]).collect()],
            &[],
            &[],
        )?;
        for side in [-1., 1.] {
            for triangle in &side_profile.triangles {
                let points = triangle.map(|index| {
                    let [y, z, _] = side_profile.points[index];
                    [x + side * 0.39, y, z]
                });
                d.face(steel, &points, [side, 0., 0.], [1.; 3])?;
            }
        }
        for pair in profile.windows(2) {
            let [[ay, az], [by, bz]] = [pair[0], pair[1]];
            if (az == lo && bz == lo) || (az == hi && bz == hi) {
                continue;
            }
            d.face(
                steel,
                &[
                    [x - 0.39, ay, az],
                    [x + 0.39, ay, az],
                    [x + 0.39, by, bz],
                    [x - 0.39, by, bz],
                ],
                [0., bz - az, ay - by],
                [1.; 3],
            )?;
        }
    }
    d.cuboid(
        steel,
        [x - 0.39, 0.32, lo],
        [x + 0.39, 0.94, hi],
        [1.; 3],
        "ud",
    )?;
    for section in 0..=3 {
        let z = lo + (hi - lo) * section as f64 / 3.;
        d.cuboid(
            "vent-edge",
            [x - 0.42, 0.29, z - 0.018],
            [x + 0.42, 0.97, z + 0.018],
            [0.7; 3],
            "ud",
        )?;
    }
    for z in [bottom + 1.25, top - 1.30] {
        for side in [-0.53, 0.53] {
            d.bar(
                steel,
                [x + side, 0.02, z - 0.11],
                [x + side, 0.83, z],
                0.05,
                [0.48; 3],
            )?;
            d.bar(
                steel,
                [x + side, 0.02, z - 0.11],
                [x + side, 0.02, z + 0.1],
                0.065,
                [0.48; 3],
            )?;
        }
        d.bar(
            steel,
            [x - 0.55, 0.83, z],
            [x + 0.55, 0.83, z],
            0.05,
            [0.48; 3],
        )?;
    }
    let hatch = (lo + hi) / 2. - 0.3;
    d.cuboid(
        steel,
        [x - 0.27, 0.94, hatch],
        [x + 0.27, 0.98, hatch + 0.6],
        [0.8; 3],
        "b",
    )?;
    d.bar(
        "vent-edge",
        [x + 0.16, 1.01, hatch + 0.2],
        [x + 0.16, 1.01, hatch + 0.4],
        0.035,
        [0.7; 3],
    )?;
    use crate::worldspec::themes::kowloon::stains::{RunoffSettings, runoff};
    for (z, drop) in [(bottom + 0.055, 1.55), (top + 0.055, 1.65)] {
        runoff(
            d.mesh,
            &d.pen,
            [x - 0.76, z - drop, x + 0.76, z],
            RunoffSettings {
                seed: &format!("{}/flange/{x}/{z}", d.pen.part),
                coarseness: 0.4,
                spread: 1.,
                density: 0.125,
                strength: 0.83,
                tint: [0.43, 0.23, 0.095],
                variation: 0.9,
            },
        )?;
    }
    for z in [bottom + 1.25, top - 1.30] {
        for side in [-0.53, 0.53] {
            let center = x + side;
            runoff(
                d.mesh,
                &d.pen,
                [center - 0.185, z - 1.3, center + 0.185, z - 0.5],
                RunoffSettings {
                    seed: &format!("{}/bracket/{center}/{z}", d.pen.part),
                    coarseness: 0.35,
                    spread: 0.7,
                    density: 0.1,
                    strength: 0.7,
                    tint: [0.36, 0.19, 0.075],
                    variation: 0.8,
                },
            )?;
        }
    }
    Ok(())
}
