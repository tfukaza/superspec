//! Surface-mounted oval bulkhead. The moulded lens and casting share an oval
//! tool shape. The cage and fasteners have round sections in physical metres.
use super::{Arguments, draw, on, room_pen};
use crate::itemspec::definition::{NativeOutput, Values};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    palette::srgb,
    pen::Tint,
    shop::forms::{Tints, lathe},
};
use std::f64::consts::{PI, TAU};
use superspec_geometry::math::{cos, sin, unit};

const OVAL: f64 = 1.36;
const REAR_SHELL: &[[f64; 2]] = &[
    [0.114, 0.006],
    [0.120, 0.009],
    [0.122, 0.014],
    [0.121, 0.025],
];
const GASKET_RETURN: &[[f64; 2]] = &[[0.114, 0.049], [0.119, 0.051], [0.120, 0.055]];
const BODY: &[[f64; 2]] = &[
    [0.121, 0.025],
    [0.116, 0.030],
    [0.115, 0.042],
    [0.111, 0.046],
];
const BEZEL: &[[f64; 2]] = &[
    [0.120, 0.055],
    [0.117, 0.059],
    [0.112, 0.060],
    [0.100, 0.062],
    [0.096, 0.065],
    [0.093, 0.065],
    [0.091, 0.062],
];

// Each ring on the tooling profile receives a radius in UV space. Unrolling
// the cross-section by arc length keeps vertical shoulders and returns distinct.
fn radial_chart(
    mesh: &mut crate::itemspec::definition::ItemMesh,
    profile: &[[f64; 2]],
    outer: f64,
    inner: f64,
) {
    let mut stations = vec![0.];
    for ring in profile.windows(2) {
        stations.push(
            stations.last().unwrap() + (ring[1][0] - ring[0][0]).hypot(ring[1][1] - ring[0][1]),
        );
    }
    let length = stations.last().unwrap();
    for (point, uv) in mesh.positions.iter().zip(&mut mesh.uvs) {
        let radius = point[0].hypot(point[1]);
        let distance = |p: &[f64; 2]| (p[0] - radius).powi(2) + (p[1] - point[2]).powi(2);
        let nearest = profile
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
            .unwrap()
            .0;
        let uv_radius = outer + (inner - outer) * stations[nearest] / length;
        *uv = [
            0.5 + point[0] * uv_radius / radius,
            0.5 + point[1] * uv_radius / radius,
        ];
    }
}

fn turned(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    profile: &[[f64; 2]],
    color: &str,
) -> Result<(), String> {
    lathe(
        d,
        slot,
        [0.; 3],
        profile,
        &Tints::Uniform(&Tint::Color(srgb(color))),
        40,
        Some(2.),
    )
}

pub(super) fn bulkhead(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    Arguments::new(name, values).finish()?;
    let mut result = draw(cancelled, |mesh| {
        let mut d = on(mesh, room_pen("bulkhead", [0.; 3])?)?;
        // Rear isolation pad, cast shell, gasket line, and the retaining bezel.
        turned(
            &mut d,
            "seal",
            &[[0.111, 0.], [0.114, 0.003], [0.114, 0.008]],
            "#898782",
        )?;
        turned(&mut d, "casting", REAR_SHELL, "#ced0c3")?;
        turned(
            &mut d,
            "seal",
            &[[0.112, 0.044], [0.114, 0.046], [0.114, 0.050]],
            "#565853",
        )?;
        turned(&mut d, "gasket-return", GASKET_RETURN, "#dad9cb")?;
        turned(&mut d, "spun-body", BODY, "#cbd3d6")?;
        turned(&mut d, "rim", BEZEL, "#f4f0e0")?;
        turned(
            &mut d,
            "seal",
            &[[0.093, 0.061], [0.092, 0.065], [0.088, 0.065]],
            "#686660",
        )?;

        // A pressed-glass bullseye. Narrow raised rings on the crown catch
        // oblique light; a shallow inner well keeps the centre legible.
        let mut lens = vec![
            [0.090, 0.061],
            [0.091, 0.071],
            [0.088, 0.082],
            [0.080, 0.095],
            [0.071, 0.106],
            [0.066, 0.110],
        ];
        for radius in [0.064, 0.052, 0.040, 0.028, 0.016] {
            let z = 0.112 + (0.064 - radius) * 0.24;
            lens.extend([
                [radius, z],
                [radius - 0.0016, z + 0.0028],
                [radius - 0.0045, z + 0.0015],
            ]);
        }
        lens.push([0.007, 0.126]);
        turned(&mut d, "lens", &lens, "#fff4d9")?;
        d.disc(
            "lens",
            [0., 0., 0.126],
            0.007,
            [0., 0., 1.],
            srgb("#fff5dc"),
            40,
        )?;

        // Welded parallel cage hoops follow an ellipsoid outside the glass.
        let guard = srgb("#707773");
        for station in [-0.58_f64, 0., 0.58] {
            let chord = (1. - station * station).sqrt();
            let hoop: Vec<_> = (0..=14)
                .map(|i| {
                    let a = PI * i as f64 / 14.;
                    [
                        0.100 * OVAL * station,
                        0.100 * chord * cos(a),
                        0.062 + 0.084 * chord * sin(a),
                    ]
                })
                .collect();
            d.tube("guard", &hoop, 0.0042, guard, 8)?;
        }
        let spine: Vec<_> = (0..=20)
            .map(|i| {
                let a = PI * i as f64 / 20.;
                [0.100 * OVAL * cos(a), 0., 0.062 + 0.090 * sin(a)]
            })
            .collect();
        d.tube("guard", &spine, 0.0042, guard, 8)?;
        let loop_wire: Vec<_> = (0..=40)
            .map(|i| {
                let a = TAU * i as f64 / 40.;
                [0.100 * OVAL * cos(a), 0.100 * sin(a), 0.064]
            })
            .collect();
        d.tube("guard", &loop_wire, 0.0037, guard, 8)?;

        for corner in 0..4 {
            let a = PI * 0.25 + corner as f64 * PI * 0.5;
            let (x, y) = (0.109 * OVAL * cos(a), 0.109 * sin(a));
            d.drum("fastener", [x, y, 0.058], 0.008, 0.004, srgb("#abb1a6"), 12)?;
            d.drum(
                "fastener",
                [x, y, 0.061],
                0.0062,
                0.004,
                srgb("#d5d6c9"),
                12,
            )?;
            d.cuboid(
                "slot",
                [x - 0.0043, y - 0.0007, 0.065],
                [x + 0.0043, y + 0.0007, 0.0653],
                srgb("#66665e"),
                "d",
            )?;
        }
        // Compression gland and an anchored, short run of painted conduit. Its
        // far end turns up into the soffit through a seating washer.
        let y = 0.018;
        for (a, b, r, n) in [
            (0.155, 0.171, 0.017, 16),
            (0.169, 0.184, 0.019, 6),
            (0.182, 0.195, 0.012, 12),
        ] {
            d.tube(
                "fastener",
                &[[a, y, 0.024], [b, y, 0.024]],
                r,
                srgb("#aaae9e"),
                n,
            )?;
        }
        let cable = srgb("#b8b5a6");
        d.tube(
            "guard",
            &[
                [0.191, y, 0.024],
                [0.335, y, 0.024],
                [0.344, y, 0.021],
                [0.351, y, 0.013],
                [0.352, y, 0.006],
            ],
            0.008,
            cable,
            10,
        )?;
        d.drum("guard", [0.352, y, 0.], 0.018, 0.008, cable, 16)?;
        d.cuboid(
            "casting-foot",
            [0.259, y - 0.018, 0.],
            [0.279, y + 0.018, 0.009],
            srgb("#c0c4b8"),
            "d",
        )?;
        let saddle: Vec<_> = (0..=10)
            .map(|i| {
                let a = PI * i as f64 / 10.;
                [0.269, y + 0.012 * cos(a), 0.008 + 0.025 * sin(a)]
            })
            .collect();
        d.tube("guard", &saddle, 0.0032, cable, 8)
    })?;
    // Stretch just the surfaces formed on the oval tooling. Apply the inverse
    // transpose to their normals; the mounting hardware and wire stay round.
    for mesh in &mut result.meshes {
        match mesh.material_slot.as_str() {
            "casting" => radial_chart(mesh, REAR_SHELL, 0.38, 0.47),
            "gasket-return" => radial_chart(mesh, GASKET_RETURN, 0.37, 0.46),
            "rim" => radial_chart(mesh, BEZEL, 0.46, 0.29),
            "spun-body" => radial_chart(mesh, BODY, 0.47, 0.36),
            _ => {}
        }
        if [
            "casting",
            "gasket-return",
            "rim",
            "spun-body",
            "seal",
            "lens",
        ]
        .contains(&mesh.material_slot.as_str())
        {
            for p in &mut mesh.positions {
                p[0] *= OVAL;
            }
            for n in &mut mesh.normals {
                *n = unit([n[0] / OVAL, n[1], n[2]])?;
            }
        }
    }
    Ok(result)
}
