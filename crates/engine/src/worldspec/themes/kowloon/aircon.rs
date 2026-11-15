//! Window units and split-system condensers, with atlas-unwrapped cases and physical fans/hardware.
use super::{
    corrugated,
    fixture::Fixture,
    mesh::rounded,
    metal::Kind,
    palette::{mix, shade, srgb},
    pen::Tint,
    steel::{self, Finish, Plate, Steel},
    weathered::Settings,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{atan2, cos, sin},
};
use superspec_types::rng::SeededRng;
const SHADOW: V3 = [0.05, 0.05, 0.055];
const FAN_ASPECT: f64 = 1.4545;
pub const CONDENSER_ROOM: f64 = 0.06 + 0.7 + 0.15;
pub const CONDENSER_HEIGHT: f64 = 0.5;
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Bounds {
    pub x0: f64,
    pub x1: f64,
    pub z: f64,
}
struct Cells {
    front: usize,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
    back: Option<usize>,
}
fn atlas(cell: usize, s: f64, t: f64) -> [f64; 2] {
    [
        (cell % 8) as f64 / 8. + s.clamp(0.014, 0.986) / 8.,
        1. - ((cell / 8) as f64 + t.clamp(0.014, 0.986)) / 4.,
    ]
}
fn unwrapped(
    drawing: &mut Fixture<'_, '_>,
    cell: usize,
    points: &[V3],
    hint: V3,
    tint: V3,
    project: &dyn Fn(V3) -> [f64; 2],
) -> Result<(), String> {
    if drawing.mesh.surveying() {
        return drawing.pen.face(
            drawing.mesh,
            "aircon",
            points,
            hint,
            &Tint::Color(tint),
            1.,
            None,
            None,
        );
    }
    let uv: Vec<_> = points
        .iter()
        .map(|p| {
            let [s, t] = project(*p);
            atlas(cell, s, t)
        })
        .collect();
    drawing.pen.face(
        drawing.mesh,
        "aircon",
        points,
        hint,
        &Tint::Color(tint),
        drawing.repeat("aircon"),
        Some(&uv),
        None,
    )
}
fn on_face([x0, x1, z0, z1]: [f64; 4]) -> impl Fn(V3) -> [f64; 2] {
    move |p| [(p[0] - x0) / (x1 - x0), (z1 - p[2]) / (z1 - z0)]
}
fn casing(
    d: &mut Fixture<'_, '_>,
    lo: V3,
    hi: V3,
    b: f64,
    c: Cells,
    tint: V3,
) -> Result<(), String> {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    let front = on_face([x0, x1, z0, z1]);
    let left = |p: V3| [(p[1] - y0) / (y1 - y0), (z1 - p[2]) / (z1 - z0)];
    let right = |p: V3| [(y1 - p[1]) / (y1 - y0), (z1 - p[2]) / (z1 - z0)];
    let top = |p: V3| [(p[0] - x0) / (x1 - x0), (p[1] - y0) / (y1 - y0)];
    unwrapped(
        d,
        c.left,
        &[
            [x0, y0, z0 + b],
            [x0, y1 - b, z0 + b],
            [x0, y1 - b, z1 - b],
            [x0, y0, z1 - b],
        ],
        [-1., 0., 0.],
        tint,
        &left,
    )?;
    unwrapped(
        d,
        c.right,
        &[
            [x1, y1 - b, z0 + b],
            [x1, y0, z0 + b],
            [x1, y0, z1 - b],
            [x1, y1 - b, z1 - b],
        ],
        [1., 0., 0.],
        tint,
        &right,
    )?;
    unwrapped(
        d,
        c.top,
        &[
            [x0 + b, y1 - b, z1],
            [x1 - b, y1 - b, z1],
            [x1 - b, y0, z1],
            [x0 + b, y0, z1],
        ],
        [0., 0., 1.],
        tint,
        &top,
    )?;
    unwrapped(
        d,
        c.bottom,
        &[
            [x0 + b, y0, z0],
            [x1 - b, y0, z0],
            [x1 - b, y1 - b, z0],
            [x0 + b, y1 - b, z0],
        ],
        [0., 0., -1.],
        shade(tint, 0.7),
        &top,
    )?;
    if let Some(back) = c.back {
        unwrapped(
            d,
            back,
            &[[x1, y0, z0], [x0, y0, z0], [x0, y0, z1], [x1, y0, z1]],
            [0., -1., 0.],
            shade(tint, 0.8),
            &front,
        )?;
    }
    for [x, inner, side] in [[x0, x0 + b, -1.], [x1, x1 - b, 1.]] {
        for (z, flat, up, cell) in [(z1 - b, z1, 1., c.top), (z0 + b, z0, -1., c.bottom)] {
            unwrapped(
                d,
                cell,
                &[
                    [x, y0, z],
                    [x, y1 - b, z],
                    [inner, y1 - b, flat],
                    [inner, y0, flat],
                ],
                [side, 0., up],
                shade(tint, 0.94),
                &top,
            )?;
        }
    }
    for [z, flat, up] in [[z1 - b, z1, 1.], [z0 + b, z0, -1.]] {
        unwrapped(
            d,
            c.front,
            &[
                [x0 + b, y1, z],
                [x1 - b, y1, z],
                [x1 - b, y1 - b, flat],
                [x0 + b, y1 - b, flat],
            ],
            [0., 1., up],
            shade(tint, 0.96),
            &front,
        )?;
    }
    for [x, inner, side] in [[x0, x0 + b, -1.], [x1, x1 - b, 1.]] {
        unwrapped(
            d,
            c.front,
            &[
                [inner, y1, z0 + b],
                [inner, y1, z1 - b],
                [x, y1 - b, z1 - b],
                [x, y1 - b, z0 + b],
            ],
            [side, 1., 0.],
            shade(tint, 0.96),
            &front,
        )?;
        for [z, flat, up] in [[z1 - b, z1, 1.], [z0 + b, z0, -1.]] {
            unwrapped(
                d,
                c.front,
                &[[inner, y1, z], [x, y1 - b, z], [inner, y1 - b, flat]],
                [side, 1., up],
                shade(tint, 0.9),
                &front,
            )?;
        }
    }
    Ok(())
}
fn grille(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    y: f64,
    recess: f64,
    cell: usize,
    tint: V3,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let project = on_face(span);
    let ring = |inset: f64, depth| {
        let dx = (x1 - x0) * inset;
        let dz = (z1 - z0) * inset;
        [
            [x0 + dx, depth, z0 + dz],
            [x1 - dx, depth, z0 + dz],
            [x1 - dx, depth, z1 - dz],
            [x0 + dx, depth, z1 - dz],
        ]
    };
    let outer = ring(0., y);
    let lip = ring(0.05, y);
    let panel = ring(0.075, y - recess);
    let inward = [[0., 0., 1.], [-1., 0., 0.], [0., 0., -1.], [1., 0., 0.]];
    for k in 0..4 {
        let next = (k + 1) % 4;
        unwrapped(
            d,
            cell,
            &[outer[k], outer[next], lip[next], lip[k]],
            [0., 1., 0.],
            tint,
            &project,
        )?;
        let facing = inward[k].map(|v| v * recess.signum());
        unwrapped(
            d,
            cell,
            &[lip[k], lip[next], panel[next], panel[k]],
            [facing[0], 0.4, facing[2]],
            shade(tint, if recess > 0. { 0.6 } else { 0.9 }),
            &project,
        )?;
    }
    unwrapped(d, cell, &panel, [0., 1., 0.], tint, &project)
}
fn louvers(
    d: &mut Fixture<'_, '_>,
    x: f64,
    side: f64,
    span: [f64; 4],
    tint: V3,
) -> Result<(), String> {
    let [y0, y1, z0, z1] = span;
    let out = |depth| x + side * depth;
    d.face(
        "paint",
        &[
            [out(0.001), y0, z0],
            [out(0.001), y1, z0],
            [out(0.001), y1, z1],
            [out(0.001), y0, z1],
        ],
        [side, 0., 0.],
        SHADOW,
    )?;
    let slats = rounded((z1 - z0) / 0.03, 1.).max(3.) as usize;
    let pitch = (z1 - z0) / slats as f64;
    for i in 0..slats {
        let foot = z0 + i as f64 * pitch;
        let lip = foot + pitch * 0.72;
        d.face(
            "metal",
            &[
                [out(0.011), y0, foot],
                [out(0.011), y1, foot],
                [out(0.002), y1, lip],
                [out(0.002), y0, lip],
            ],
            [side, 0., -0.6],
            shade(tint, 0.92),
        )?;
    }
    Ok(())
}
fn brackets(
    drawing: &mut Fixture<'_, '_>,
    span: [f64; 2],
    depth: f64,
    z: f64,
    drop: f64,
    tint: V3,
    weathering: Option<&Settings>,
) -> Result<(), String> {
    let fall = drop.min(0.32);
    let pen = if let Some(s) = weathering {
        drawing.pen.with_weathering(
            drawing.mesh,
            s.clone(),
            Kind::Frame,
            None,
            vec!["paint".into(), "metal".into()],
        )?
    } else {
        drawing.pen.clone()
    };
    let mut d = Fixture {
        pen,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    let steel = Steel {
        slot: "metal",
        tint,
        finish: Finish::Metal(weathering),
    };
    for x in [span[0] + 0.07, span[1] - 0.07] {
        steel::angle(
            &mut d,
            steel,
            [[x - 0.018, 0.008, z - 0.04], [x - 0.018, depth, z - 0.04]],
            [[1., 0., 0.], [0., 0., 1.]],
            [0.035, 0.004],
        )?;
        if fall < 0.12 {
            continue;
        }
        steel::member(
            &mut d,
            steel,
            [[x, depth - 0.05, z - 0.04], [x, 0.014, z - fall + 0.035]],
            [1., 0., 0.],
            [0.026, 0.018],
        )?;
        steel::fixing_plate(
            &mut d,
            steel,
            &Plate {
                center: [x, 0., z - fall / 2.],
                across: [1., 0., 0.],
                up: [0., 0., 1.],
                out: [0., 1., 0.],
                size: [0.07, fall],
                bolts: 4,
            },
        )?;
    }
    Ok(())
}
fn rain_cover(
    d: &mut Fixture<'_, '_>,
    span: [f64; 2],
    depth: f64,
    top: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x0, x1] = span;
    if rng.chance(0.6) {
        let sheet = mix(srgb("#a4a9aa"), srgb("#7a4a2a"), rng.float(0.1, 0.6));
        return corrugated::draw(
            d,
            "sheet",
            [
                [x0 - 0.06, 0., top + 0.11],
                [x1 + 0.06, 0., top + 0.11],
                [x1 + 0.06, depth + 0.06, top + 0.02],
                [x0 - 0.06, depth + 0.06, top + 0.02],
            ],
            0.09,
            sheet,
            None,
        );
    }
    let lo = [x0 - 0.04, 0., top + 0.002];
    let hi = [x1 + 0.04, depth + 0.05, top + 0.016];
    let flat = |p: V3| [(p[0] - lo[0]) / (hi[0] - lo[0]), p[1] / hi[1]];
    let edge = |p: V3| {
        [
            (p[0] - lo[0] + p[1]) / (hi[0] - lo[0] + hi[1]),
            (hi[2] - p[2]) / 0.2,
        ]
    };
    let wood = shade([1.; 3], rng.float(0.7, 0.95));
    unwrapped(
        d,
        24,
        &[
            [lo[0], hi[1], hi[2]],
            [hi[0], hi[1], hi[2]],
            [hi[0], 0., hi[2]],
            [lo[0], 0., hi[2]],
        ],
        [0., 0., 1.],
        wood,
        &flat,
    )?;
    unwrapped(
        d,
        24,
        &[
            [lo[0], hi[1], lo[2]],
            [hi[0], hi[1], lo[2]],
            [hi[0], hi[1], hi[2]],
            [lo[0], hi[1], hi[2]],
        ],
        [0., 1., 0.],
        shade(wood, 0.7),
        &edge,
    )?;
    for x in [lo[0], hi[0]] {
        unwrapped(
            d,
            24,
            &[
                [x, 0., lo[2]],
                [x, hi[1], lo[2]],
                [x, hi[1], hi[2]],
                [x, 0., hi[2]],
            ],
            [if x == lo[0] { -1. } else { 1. }, 0., 0.],
            shade(wood, 0.7),
            &edge,
        )?;
    }
    let bx = x0 + (x1 - x0) * rng.float(0.2, 0.7);
    d.cuboid(
        "paint",
        [bx, depth * 0.3, hi[2]],
        [bx + 0.2, depth * 0.3 + 0.1, hi[2] + 0.06],
        shade(srgb("#8f5a48"), rng.float(0.8, 1.1)),
        "d",
    )
}
/// Three fixed wear classes use one draw, including the reference's inclusive cumulative boundary.
fn wear(rng: &mut SeededRng, weights: [f64; 3]) -> usize {
    let mut roll = rng.next_f64() * weights.iter().sum::<f64>();
    for (i, w) in weights.iter().enumerate() {
        roll -= w;
        if roll <= 0. {
            return i;
        }
    }
    2
}
fn body(rng: &mut SeededRng) -> V3 {
    srgb(rng.pick(&["#e0ddd2", "#cfc8b4", "#b8bcc0", "#d8d0bc"]))
}
/// Window-mounted unit. Returned facade footprint is also the source of its wall stain.
pub fn air_conditioner(
    d: &mut Fixture<'_, '_>,
    x: f64,
    z: f64,
    drop: f64,
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
) -> Result<Bounds, String> {
    if [x, z, drop]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || !(0. ..=100.).contains(&drop)
    {
        return Err("Air conditioner needs bounded finite placement and nonnegative drop.".into());
    }
    let model = rng.integer(0, 3);
    let wear = wear(rng, [3., 4., 3.]);
    let [narrowest, widest] = [[0.58, 0.68], [0.62, 0.72], [0.5, 0.58], [0.6, 0.7]][model];
    let w = rng.float(narrowest, widest);
    let h = w * rng.float(0.6, 0.67);
    let depth = rng.float(0.3, 0.37);
    let body = mix(
        mix(
            body(rng),
            srgb("#cdbf98"),
            wear as f64 * rng.float(0.1, 0.3),
        ),
        srgb("#7a4a2a"),
        wear as f64 * rng.float(0., 0.06),
    );
    let x0 = x - w / 2.;
    let x1 = x + w / 2.;
    let b = 0.012;
    let rear = 16 + model + if wear > 0 { 4 } else { 0 };
    let side = wear;
    let top = if wear > 0 && rng.chance(0.6) { 4 } else { 3 };
    casing(
        d,
        [x0, 0., z],
        [x1, depth, z + h],
        b,
        Cells {
            front: rear,
            left: side,
            right: side,
            top,
            bottom: 3,
            back: None,
        },
        body,
    )?;
    let recess = [0.006, 0.02, 0.028, -0.026][model];
    grille(
        d,
        [x0 + b, x1 - b, z + b, z + h - b],
        depth,
        recess,
        rear,
        body,
    )?;
    let wire = shade(body, 0.65);
    let grille_y = depth - recess + 0.003;
    let mut u = x0 + 0.055;
    while u < x1 - 0.045 {
        d.tube(
            "metal",
            &[[u, grille_y, z + 0.05], [u, grille_y, z + h - 0.05]],
            0.0018,
            wire,
            5,
        )?;
        u += 0.055;
    }
    let mut v = z + 0.065;
    while v < z + h - 0.05 {
        d.tube(
            "metal",
            &[[x0 + 0.05, grille_y, v], [x1 - 0.05, grille_y, v]],
            0.0018,
            wire,
            5,
        )?;
        v += 0.055;
    }
    let screws = Steel {
        slot: "metal",
        tint: shade(body, 0.7),
        finish: Finish::Metal(None),
    };
    for u in [x0 + 0.026, x1 - 0.026] {
        for v in [z + 0.026, z + h - 0.026] {
            steel::bolt(d, screws, [u, depth + 0.001, v], [0., 1., 0.], 0.004)?;
        }
    }
    for [face, direction] in [[x0, -1.], [x1, 1.]] {
        louvers(
            d,
            face,
            direction,
            [depth * 0.42, depth - 0.05, z + h * 0.24, z + h * 0.78],
            body,
        )?;
    }
    d.cuboid(
        "metal",
        [x0 - 0.015, 0., z - 0.015],
        [x1 + 0.015, 0.035, z + h + 0.015],
        shade(body, 0.8),
        "b",
    )?;
    if rng.chance(0.6) {
        rain_cover(d, [x0, x1], depth, z + h, rng)?;
    }
    let steel = mix(
        shade(srgb("#a4a9aa"), 0.6),
        shade(srgb("#7a4a2a"), 0.75),
        rng.float(0.2, 0.85),
    );
    brackets(d, [x0, x1], depth - 0.02, z, drop, steel, weathering)?;
    let hose = x + rng.float(-0.2, 0.2);
    let extent = rng.float(0.3, 0.7).min(drop) - 0.005;
    if extent > 0.15 {
        d.tube(
            "paint",
            &[
                [hose, depth - 0.05, z],
                [hose, depth - 0.06, z - 0.12],
                [hose + 0.03, depth - 0.1, z - extent],
            ],
            0.011,
            srgb("#3a3a38"),
            5,
        )?;
    }
    Ok(Bounds { x0, x1, z })
}

fn fan_rotor(
    d: &mut Fixture<'_, '_>,
    center: V3,
    radius: f64,
    blades: usize,
) -> Result<(), String> {
    let [x, y, z] = center;
    let dark = srgb("#50564f");
    let point =
        |angle: f64, r: f64, pitch: f64| [x + cos(angle) * r, y + pitch, z + sin(angle) * r];
    for blade in 0..blades {
        let start = blade as f64 / blades as f64 * PI * 2.;
        for ring in 0..4 {
            let inner = 0.18 + ring as f64 * 0.19;
            let outer = inner + 0.19;
            let a = start + inner * 0.55;
            let b = start + outer * 0.55;
            d.sheet(
                "metal",
                &[
                    point(a, inner * radius, -0.007),
                    point(b, outer * radius, -0.007),
                    point(b + 0.56, outer * radius, 0.012),
                    point(a + 0.56, inner * radius, 0.012),
                ],
                dark,
            )?;
        }
    }
    d.tube(
        "metal",
        &[[x, y - 0.025, z], [x, y + 0.018, z]],
        radius * 0.19,
        dark,
        20,
    )?;
    d.disc(
        "metal",
        [x, y + 0.018, z],
        radius * 0.19,
        [0., 1., 0.],
        dark,
        20,
    )
}
fn fan_front(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    y: f64,
    cell: usize,
    fan: usize,
    tint: V3,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    let project = on_face(span);
    let cx = x0 + (x1 - x0) * 0.37;
    let cz = (z0 + z1) / 2.;
    let radius = (z1 - z0) * 0.4;
    let mut angles: Vec<_> = (0..40).map(|i| i as f64 / 40. * PI * 2.).collect();
    for [x, z] in [[x0, z0], [x1, z0], [x1, z1], [x0, z1]] {
        angles.push((atan2(z - cz, x - cx) + PI * 2.) % (PI * 2.));
    }
    angles.sort_by(f64::total_cmp);
    let rim = |angle: f64, scale: f64, depth: f64| {
        [
            cx + cos(angle) * radius * scale,
            depth,
            cz + sin(angle) * radius * scale,
        ]
    };
    let edge = |angle: f64| {
        let c = cos(angle);
        let s = sin(angle);
        let reach_x = if c.abs() < 1e-9 {
            f64::INFINITY
        } else {
            ((if c > 0. { x1 } else { x0 }) - cx) / c
        };
        let reach_z = if s.abs() < 1e-9 {
            f64::INFINITY
        } else {
            ((if s > 0. { z1 } else { z0 }) - cz) / s
        };
        let reach = reach_x.min(reach_z);
        [cx + c * reach, y, cz + s * reach]
    };
    let shroud = y - 0.05;
    for i in 0..angles.len() {
        let a = angles[i];
        let b = if i + 1 < angles.len() {
            angles[i + 1]
        } else {
            angles[0] + PI * 2.
        };
        if (b - a).abs() < 1e-6 {
            continue;
        }
        let middle = [-cos((a + b) / 2.), 0.3, -sin((a + b) / 2.)];
        unwrapped(
            d,
            cell,
            &[rim(a, 1., y), rim(b, 1., y), edge(b), edge(a)],
            [0., 1., 0.],
            tint,
            &project,
        )?;
        d.face(
            "paint",
            &[
                rim(a, 1., y),
                rim(b, 1., y),
                rim(b, 0.96, shroud),
                rim(a, 0.96, shroud),
            ],
            middle,
            SHADOW,
        )?;
    }
    d.disc(
        "paint",
        [cx, shroud - 0.035, cz],
        radius * 0.96,
        [0., 1., 0.],
        srgb("#222724"),
        40,
    )?;
    fan_rotor(
        d,
        [cx, shroud + 0.01, cz],
        radius * 0.9,
        if fan == 14 { 3 } else { 5 },
    )?;
    let wire = shade(tint, 0.72);
    let proud = y + 0.014;
    for scale in [0.26, 0.42, 0.58, 0.74, 0.9, 0.99] {
        let path: Vec<_> = (0..41)
            .map(|i| {
                rim(
                    i as f64 / 40. * PI * 2.,
                    scale,
                    y + 0.006 + 0.012 * (1. - scale * scale),
                )
            })
            .collect();
        d.tube("metal", &path, 0.0019, wire, 5)?;
    }
    for i in 0..10 {
        d.tube(
            "metal",
            &[
                rim(i as f64 / 10. * PI * 2., 0.2, proud),
                rim(i as f64 / 10. * PI * 2., 0.99, y + 0.006),
            ],
            0.0018,
            wire,
            5,
        )?;
    }
    d.disc(
        "metal",
        [cx, proud + 0.002, cz],
        radius * 0.19,
        [0., 1., 0.],
        shade(tint, 0.9),
        24,
    )
}
fn line_set(
    d: &mut Fixture<'_, '_>,
    from: V3,
    rise: f64,
    rng: &mut SeededRng,
) -> Result<(), String> {
    let [x, y, z] = from;
    let out = x + 0.07;
    let insulation = srgb("#d8d6cc");
    let hose = srgb("#3a3a38");
    if rise < 0.3 {
        d.tube(
            "paint",
            &[[x, y, z], [out, y, z], [out, 0.01, z + 0.04]],
            0.016,
            insulation,
            6,
        )?;
        return d.tube(
            "paint",
            &[
                [x, y, z - 0.05],
                [out + 0.03, y, z - 0.05],
                [out + 0.03, 0.01, z],
            ],
            0.011,
            hose,
            5,
        );
    }
    d.tube(
        "paint",
        &[[x, y, z], [out, y, z], [out, 0.04, z + 0.05]],
        0.016,
        insulation,
        6,
    )?;
    d.tube(
        "paint",
        &[[x, y, z - 0.05], [out, y, z - 0.05], [out, 0.04, z]],
        0.011,
        hose,
        5,
    )?;
    let top = z + rise;
    let half = 0.04;
    let deep = 0.055;
    let white = shade([1.; 3], rng.float(0.78, 0.95));
    let mut foot = z - 0.02;
    while foot < top - 1e-6 {
        let head = top.min(foot + 0.5);
        let share = (head - foot) / 0.5;
        let face = |p: V3| {
            [
                0.25 + ((p[0] - out + half) / (2. * half)) * 0.5,
                ((head - p[2]) / (head - foot)) * share,
            ]
        };
        let flank = |p: V3| {
            [
                (p[1] / deep) * 0.25,
                ((head - p[2]) / (head - foot)) * share,
            ]
        };
        unwrapped(
            d,
            25,
            &[
                [out - half, deep, foot],
                [out + half, deep, foot],
                [out + half, deep, head],
                [out - half, deep, head],
            ],
            [0., 1., 0.],
            white,
            &face,
        )?;
        for side in [-1., 1.] {
            unwrapped(
                d,
                25,
                &[
                    [out + side * half, 0., foot],
                    [out + side * half, deep, foot],
                    [out + side * half, deep, head],
                    [out + side * half, 0., head],
                ],
                [side, 0., 0.],
                shade(white, 0.85),
                &flank,
            )?;
        }
        foot += 0.5;
    }
    d.face(
        "paint",
        &[
            [out - half, 0., top],
            [out + half, 0., top],
            [out + half, deep, top],
            [out - half, deep, top],
        ],
        [0., 0., 1.],
        shade(white, 0.9),
    )
}
/// One- or two-fan condenser within [left,right,base,free height], including its support/line-set allowances.
pub fn condenser(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    rng: &mut SeededRng,
    weathering: Option<&Settings>,
) -> Result<Bounds, String> {
    let [x0, x1, z, height] = span;
    if span.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || !(CONDENSER_ROOM..=100.).contains(&(x1 - x0))
        || !(CONDENSER_HEIGHT..=100.).contains(&height)
    {
        return Err(
            "Condenser needs bounded finite placement and its declared minimum clearance.".into(),
        );
    }
    let bevel = 0.014;
    let fan_height_of = |width: f64| (width - 2. * bevel) / FAN_ASPECT + 2. * bevel;
    let spare = x1 - x0 - 0.06 - 0.15;
    let widths: Vec<_> = [0.7, 0.78, 0.86]
        .into_iter()
        .filter(|w| *w <= spare && fan_height_of(*w) <= height)
        .collect();
    let w = if widths.is_empty() {
        0.7
    } else {
        rng.pick(&widths)
    };
    let fan_height = fan_height_of(w);
    let fans = if height >= fan_height * 2. + 0.08 && rng.chance(0.25) {
        2
    } else {
        1
    };
    let h = fan_height * fans as f64;
    let depth = w * 0.36;
    let wear = wear(rng, [3., 4., 2.]);
    let front = if wear == 2 { 10 } else { rng.pick(&[8, 9, 11]) };
    let body = mix(
        body(rng),
        srgb("#cdbf98"),
        wear as f64 * rng.float(0.05, 0.25),
    );
    let left = x0 + 0.06 + rng.float(0., (spare - w).max(0.));
    let right = left + w;
    let y0 = 0.07;
    let y1 = y0 + depth;
    let coil = if rng.chance(0.3) { 7 } else { 5 + wear.min(1) };
    let cells = Cells {
        front,
        left: coil,
        right: 12 + wear.min(1),
        top: if wear > 0 && rng.chance(0.5) { 4 } else { 3 },
        bottom: 3,
        back: Some(coil),
    };
    let fan = rng.pick(&[14, 15]);
    casing(d, [left, y0, z], [right, y1, z + h], bevel, cells, body)?;
    for i in 0..fans {
        let foot = z + i as f64 * fan_height;
        fan_front(
            d,
            [
                left + bevel,
                right - bevel,
                foot + bevel,
                foot + fan_height - bevel,
            ],
            y1,
            front,
            fan,
            body,
        )?;
    }
    if fans == 2 {
        d.panel(
            "metal",
            [
                left + bevel,
                right - bevel,
                z + fan_height - bevel,
                z + fan_height + bevel,
            ],
            y1,
            body,
        )?;
    }
    let fasteners = Steel {
        slot: "metal",
        tint: shade(body, 0.65),
        finish: Finish::Metal(None),
    };
    for x in [left + 0.035, right - 0.035] {
        for v in [z + 0.035, z + h - 0.035] {
            steel::bolt(d, fasteners, [x, y1 + 0.001, v], [0., 1., 0.], 0.004)?;
        }
    }
    d.cuboid(
        "metal",
        [right, y0 + depth * 0.3, z + 0.05],
        [right + 0.035, y1 - depth * 0.12, z + fan_height * 0.5],
        shade(body, 0.95),
        "l",
    )?;
    let steel = mix(
        shade(srgb("#a4a9aa"), 0.6),
        shade(srgb("#7a4a2a"), 0.75),
        rng.float(0.15, 0.75),
    );
    for rail in [left + 0.1, right - 0.1] {
        d.cuboid(
            "metal",
            [rail - 0.03, y0 + 0.02, z - 0.016],
            [rail + 0.03, y1 - 0.02, z],
            shade(body, 0.75),
            "u",
        )?;
        for y in [y0 + 0.05, y1 - 0.05] {
            d.drum(
                "paint",
                [rail, y, z - 0.04],
                0.024,
                0.024,
                srgb("#3a3a38"),
                10,
            )?;
            steel::bolt(d, fasteners, [rail, y, z - 0.012], [0., 0., 1.], 0.006)?;
        }
    }
    brackets(
        d,
        [left + 0.03, right - 0.03],
        y1,
        z - 0.04,
        0.32,
        steel,
        weathering,
    )?;
    let rise = rng.float(0.5, 1.3).min(height - 0.2);
    line_set(d, [right + 0.035, (y0 + y1) / 2., z + 0.14], rise, rng)?;
    if wear > 0 && height >= h + 0.14 && rng.chance(0.3) {
        rain_cover(d, [left, right], y1, z + h, rng)?;
    }
    Ok(Bounds {
        x0: left,
        x1: right,
        z,
    })
}
