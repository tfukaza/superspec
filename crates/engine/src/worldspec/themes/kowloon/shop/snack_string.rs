//! Ceiling-hung packets fitted by their actual pouch and clip envelopes.
use super::super::{fixture::Fixture, palette::srgb};
use super::stock_mesh;
use superspec_geometry::{
    Matrix, V3,
    math::{add, cos, scale, sin},
};
use superspec_types::rng::SeededRng;

#[derive(Clone, Debug)]
pub struct HungPacket {
    pub matrix: Matrix,
    pub clip: V3,
    pub seam: V3,
    pub min: V3,
    pub max: V3,
}
fn point(v: &[f64; 8], mirrored: bool) -> V3 {
    [
        v[0] * 0.12 * if mirrored { -1. } else { 1. },
        v[1] * 0.06,
        v[2] * 0.16,
    ]
}
fn measure(points: impl Iterator<Item = V3>) -> (V3, V3) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in points {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    (min, max)
}
fn bounds(matrix: Matrix, mirrored: bool) -> (V3, V3) {
    measure(
        stock_mesh::packet()
            .iter()
            .flatten()
            .map(|v| matrix.point(point(v, mirrored))),
    )
}
fn clip_bounds(matrix: Matrix, seam: V3) -> (V3, V3) {
    measure([-0.0075, 0.0075].into_iter().flat_map(|x| {
        [-0.0085, 0.006].into_iter().flat_map(move |y| {
            [-0.007, 0.019]
                .into_iter()
                .map(move |z| matrix.point(add(seam, [x, y, z])))
        })
    }))
}
/// Poses in ceiling-local meters, with exact seam attachments and a bounded drop.
pub fn plan(drop: f64, rng: &mut SeededRng, mirrored: bool) -> Result<Vec<HungPacket>, String> {
    if !drop.is_finite() || !(0. ..=10.).contains(&drop) {
        return Err("Packet strings require a finite drop of 0–10 meters.".into());
    }
    // Repeated seam corners deliberately retain the source sampling distribution.
    let seam: Vec<_> = stock_mesh::packet()
        .iter()
        .flatten()
        .filter(|v| v[2] > 0.965)
        .collect();
    let mut result = Vec::new();
    let mut ceiling = -0.09;
    while ceiling > -drop {
        let anchor = point(rng.pick(&seam), mirrored);
        let roll = -anchor[0] * 3. + rng.float(-0.08, 0.08);
        let yaw = rng.float(-0.3, 0.3);
        let rz = Matrix([
            [cos(yaw), sin(yaw), 0.],
            [-sin(yaw), cos(yaw), 0.],
            [0., 0., 1.],
            [0.; 3],
        ]);
        let ry = Matrix([
            [cos(roll), 0., -sin(roll)],
            [0., 1., 0.],
            [sin(roll), 0., cos(roll)],
            [0.; 3],
        ]);
        let mut matrix = rz.then(ry);
        matrix.0[3] = matrix.direction(scale(anchor, -1.));
        let (_, max) = bounds(matrix, mirrored);
        let (_, pin) = clip_bounds(matrix, anchor);
        let clip = [
            rng.float(-0.012, 0.012),
            -0.075,
            ceiling - max[2].max(pin[2]),
        ];
        matrix.0[3] = add(matrix.0[3], clip);
        let (min, max) = bounds(matrix, mirrored);
        if min[2] < -drop {
            break;
        }
        result.push(HungPacket {
            matrix,
            clip,
            seam: anchor,
            min,
            max,
        });
        ceiling = min[2] - rng.float(0.006, 0.018);
    }
    Ok(result)
}
fn check(d: &Fixture<'_, '_>, values: &[f64]) -> Result<(), String> {
    if values.iter().any(|v| !v.is_finite() || v.abs() > 1000.) {
        return Err("Packet displays require finite bounded coordinates.".into());
    }
    d.mesh.check_work(0)
}
fn draw(
    d: &mut Fixture<'_, '_>,
    at: V3,
    packets: &[HungPacket],
    variant: usize,
    textured: bool,
) -> Result<(), String> {
    let Some(last) = packets.last() else {
        return Ok(());
    };
    let local = d.pen.moved(at)?;
    let cord = srgb("#b29b72");
    let mut local = Fixture {
        pen: local,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    local.bar(
        "paint",
        [0.; 3],
        [0., 0., last.clip[2] - 0.015],
        0.003,
        cord,
    )?;
    for (index, pose) in packets.iter().enumerate() {
        let body_pen = local
            .pen
            .shape_frame(local.pen.matrix.then(pose.matrix), &local.pen.part)?;
        let mut body = Fixture {
            pen: body_pen.clone(),
            mesh: local.mesh,
            repeats: local.repeats,
        };
        stock_mesh::snack_packet(
            &mut body,
            [0.; 3],
            [0.12, 0.16],
            srgb("#bd7b31"),
            textured,
            (variant + index) % 4,
        )?;
        local.bar(
            "metal",
            [0., 0., pose.clip[2]],
            pose.clip,
            0.004,
            srgb("#77776b"),
        )?;
        let mut body = Fixture {
            pen: body_pen,
            mesh: local.mesh,
            repeats: local.repeats,
        };
        body.cuboid(
            "timber",
            add(pose.seam, [-0.005, -0.006, -0.007]),
            add(pose.seam, [0.005, 0.006, 0.019]),
            cord,
            "",
        )?;
        body.bar(
            "metal",
            add(pose.seam, [-0.006, -0.007, 0.004]),
            add(pose.seam, [0.006, -0.007, 0.004]),
            0.003,
            srgb("#77776b"),
        )?;
    }
    Ok(())
}
pub fn string(
    d: &mut Fixture<'_, '_>,
    at: V3,
    drop: f64,
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    check(d, &at)?;
    let packets = plan(drop, rng, d.pen.mirrored)?;
    let variant = rng.integer(0, 3);
    draw(d, at, &packets, variant, textured)
}
/// Packs complete strings using rotated pouch and clothespin bounds.
pub fn row(
    d: &mut Fixture<'_, '_>,
    span: [f64; 4],
    rng: &mut SeededRng,
    textured: bool,
) -> Result<(), String> {
    check(d, &span)?;
    let [x0, x1, y, ceiling] = span;
    if x1 < x0 || x1 - x0 > 40. {
        return Err("Packet rows require an ordered span of at most 40 meters.".into());
    }
    let mut left = x0;
    while left < x1 {
        d.mesh.check_work(0)?;
        let packets = plan(rng.float(0.5, 0.85), rng, d.pen.mirrored)?;
        if packets.is_empty() {
            break;
        }
        let mut min: f64 = -0.002;
        let mut max: f64 = 0.002;
        for p in &packets {
            let (a, b) = clip_bounds(p.matrix, p.seam);
            min = min.min(p.min[0]).min(a[0]);
            max = max.max(p.max[0]).max(b[0]);
        }
        if left + max - min > x1 {
            break;
        }
        let at = [left - min, y + rng.float(-0.025, 0.025), ceiling];
        let variant = rng.integer(0, 3);
        draw(d, at, &packets, variant, textured)?;
        left += max - min + rng.float(0.006, 0.018);
    }
    Ok(())
}
