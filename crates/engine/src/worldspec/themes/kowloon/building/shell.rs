//! Roof terraces, soffits and parapets close the envelope without hidden interior faces.
use super::placement::Site;
use crate::worldspec::themes::kowloon::{
    deck, fixture::Fixture, massing::Lot, palette::shade, pavement::render::Assets, pen::Pen,
    rooftop,
};
use superspec_geometry::V3;
use superspec_geometry::obstructions::Budget;
pub struct Settings<'a> {
    pub deck: &'a deck::Settings<'a>,
    pub assets: &'a Assets,
    pub coat: &'a str,
}
/// Equipment placement is returned to the building driver so sign collection stays source ordered.
pub struct Roof {
    pub pen: Pen,
    pub request: rooftop::Request,
}
fn pave(
    d: &mut Fixture<'_, '_>,
    s: &Settings<'_>,
    key: &str,
    points: &[V3],
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    let world: Vec<_> = points.iter().map(|p| d.pen.world(*p)).collect();
    deck::lay(d.mesh, s.deck, s.assets, key, &world, budget)
}
fn rectangle(low: V3, high: V3) -> [V3; 4] {
    [
        [low[0], low[1], low[2]],
        [high[0], low[1], low[2]],
        [high[0], high[1], low[2]],
        [low[0], high[1], low[2]],
    ]
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    site: &Site,
    lot: &Lot,
    s: &Settings<'_>,
    budget: &mut Budget<'_>,
) -> Result<Option<Roof>, String> {
    budget.spend(1)?;
    if let Some(f) = &site.footprint {
        let inner = f.inset(0.14)?;
        pave(
            d,
            s,
            &format!("{}/deck", lot.key),
            &inner
                .iter()
                .map(|p| [p[0], p[1], lot.roof])
                .collect::<Vec<_>>(),
            budget,
        )?;
        let cap = shade(lot.style.wall, 0.92);
        let at = |p: V3, z: f64| [p[0], p[1], z];
        for (i, &a) in f.points().iter().enumerate() {
            budget.spend(1)?;
            let j = (i + 1) % f.points().len();
            let (b, c, e) = (f.points()[j], inner[j], inner[i]);
            d.face(
                "concrete",
                &[
                    at(a, lot.top),
                    at(b, lot.top),
                    at(c, lot.top),
                    at(e, lot.top),
                ],
                [0., 0., 1.],
                cap,
            )?;
            d.face(
                "concrete",
                &[
                    at(e, lot.roof),
                    at(c, lot.roof),
                    at(c, lot.top),
                    at(e, lot.top),
                ],
                [a[1] - b[1], b[0] - a[0], 0.],
                cap,
            )?;
        }
        return f
            .equipment_court()
            .map(|[x, y, width, depth]| {
                Ok(Roof {
                    pen: d.pen.moved([x, y, lot.roof])?.into_part("rooftop")?,
                    request: rooftop::Request {
                        lot_key: lot.key.clone(),
                        tenement: lot.kind == "tenement",
                        wall_tint: lot.style.wall,
                        width,
                        depth,
                        parapet: lot.top - lot.roof,
                        ceiling: 3.85,
                        perimeter: false,
                    },
                })
            })
            .transpose();
    }
    let (x0, x1, depth) = (lot.x0, lot.x1, site.depth);
    if lot.top - lot.ground <= 0.05 {
        pave(
            d,
            s,
            &format!("{}/deck", lot.key),
            &rectangle([x0, 0., lot.ground], [x1, depth, lot.ground]),
            budget,
        )?;
        return Ok(None);
    }
    for (y0, y1) in [(-lot.front, 0.), (depth, depth + lot.back)] {
        if y1 - y0 >= 1e-6 {
            d.face(
                s.coat,
                &rectangle([x0, y0, lot.ground], [x1, y1, lot.ground]),
                [0., 0., -1.],
                shade(lot.style.wall, 0.7),
            )?;
        }
    }
    if lot.front < -1e-6 {
        pave(
            d,
            s,
            &format!("{}/ledge", lot.key),
            &rectangle([x0, 0., lot.ground], [x1, -lot.front, lot.ground]),
            budget,
        )?;
    }
    let (y0, y1) = (-lot.front, depth + lot.back);
    let t = 0.14;
    let (ix0, ix1, iy0, iy1) = (x0 + t, x1 - t, y0 + t, y1 - t);
    let (roof, top) = (lot.roof, lot.top);
    pave(
        d,
        s,
        &format!("{}/deck", lot.key),
        &rectangle([ix0, iy0, roof], [ix1, iy1, roof]),
        budget,
    )?;
    let cap = shade(lot.style.wall, 0.92);
    for points in [
        rectangle([x0, y0, top], [x1, iy0, top]),
        rectangle([x0, iy1, top], [x1, y1, top]),
        rectangle([x0, iy0, top], [ix0, iy1, top]),
        rectangle([ix1, iy0, top], [x1, iy1, top]),
    ] {
        d.face("concrete", &points, [0., 0., 1.], cap)?;
    }
    for (points, normal) in [
        (
            [
                [ix0, iy0, roof],
                [ix1, iy0, roof],
                [ix1, iy0, top],
                [ix0, iy0, top],
            ],
            [0., 1., 0.],
        ),
        (
            [
                [ix0, iy1, roof],
                [ix1, iy1, roof],
                [ix1, iy1, top],
                [ix0, iy1, top],
            ],
            [0., -1., 0.],
        ),
        (
            [
                [ix0, iy0, roof],
                [ix0, iy1, roof],
                [ix0, iy1, top],
                [ix0, iy0, top],
            ],
            [1., 0., 0.],
        ),
        (
            [
                [ix1, iy0, roof],
                [ix1, iy1, roof],
                [ix1, iy1, top],
                [ix1, iy0, top],
            ],
            [-1., 0., 0.],
        ),
    ] {
        d.face("concrete", &points, normal, cap)?;
    }
    let mut fixtures = Fixture {
        pen: d.pen.into_part("fixtures")?,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let coping = shade(lot.style.trim, 0.95);
    let o = 0.03;
    for y in [y0 - o, y1] {
        fixtures.cuboid(
            "wall",
            [x0 - o, y, top - 0.07],
            [x1 + o, y + o, top],
            coping,
            "",
        )?;
    }
    for x in [x0 - o, x1] {
        fixtures.cuboid("wall", [x, y0, top - 0.07], [x + o, y1, top], coping, "bf")?;
    }
    Ok(Some(Roof {
        pen: d.pen.moved([x0, y0, roof])?.into_part("rooftop")?,
        request: rooftop::Request {
            lot_key: lot.key.clone(),
            tenement: lot.kind == "tenement",
            wall_tint: lot.style.wall,
            width: x1 - x0,
            depth: y1 - y0,
            parapet: top - roof,
            ceiling: site.height + 3.5 - roof,
            perimeter: true,
        },
    }))
}
