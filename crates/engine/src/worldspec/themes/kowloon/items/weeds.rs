use super::{Arguments, ItemMesh, NativeOutput, Values, srgb};
use superspec_geometry::math::{cos, sin};

/// Small fans of arching grass. Each blade has a lit keel and a tapered tip.
pub(super) fn blades(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut args = Arguments::new(name, values);
    let mut rng = args.rng()?;
    let variant: String = args.choice("variant", "mound", &["mound", "seam"])?;
    args.finish()?;
    let seam = variant == "seam";
    let mut mesh = ItemMesh {
        id: "blades".into(),
        material_slot: "grass".into(),
        positions: vec![],
        normals: vec![],
        indices: vec![],
        colors: vec![],
        uvs: vec![],
        tangents: vec![],
        additional_uvs: Default::default(),
    };
    let crowns = if seam {
        [
            (-0.153, -0.002, 0.42, 1),
            (-0.059, 0.001, 0.56, 3),
            (0.137, 0.003, 0.43, 1),
        ]
    } else {
        [
            (-0.134, 0.009, 0.76, 2),
            (-0.047, -0.012, 1.0, 4),
            (0.106, 0.006, 0.60, 2),
        ]
    };
    let root = srgb("#48594e");
    for (x, y, vigor, count) in crowns {
        if cancelled() {
            return Err("Weed generation cancelled.".into());
        }
        let rotation = rng.float(0., std::f64::consts::TAU);
        for blade in 0..count {
            let angle = rotation
                + (blade as f64 + rng.float(-0.14, 0.14)) * std::f64::consts::TAU / count as f64;
            let (dx, dy) = (cos(angle), sin(angle));
            let shoot = [x + rng.float(-0.012, 0.012), y + rng.float(-0.008, 0.008)];
            let height = vigor
                * if blade == 0 {
                    rng.float(0.215, 0.235)
                } else {
                    rng.float(0.149, 0.211)
                };
            let reach = vigor * rng.float(0.056, 0.121);
            let half_width = vigor.sqrt() * rng.float(0.0048, 0.0073);
            let leaf = srgb(if blade == 0 {
                "#b5b57a"
            } else {
                rng.pick(&["#91a06f", "#a8ac71", "#899868", "#a1a877"])
            });
            let base = mesh.positions.len() as u32;
            for row in 0..=4 {
                let t = row as f64 / 5.;
                let along = reach * t * (0.24 + 0.76 * t);
                let z = height * t * (1.43 - 0.43 * t);
                let width =
                    half_width * (0.44 + 0.67 * sin(std::f64::consts::PI * t)) * (1. - 0.50 * t);
                let lit = (1.45 * t).min(1.);
                let color = std::array::from_fn::<_, 3, _>(|c| root[c] + lit * (leaf[c] - root[c]));
                for side in [-1., 0., 1.] {
                    let keel = if side == 0. { width * 0.5 } else { 0. };
                    mesh.positions.push([
                        shoot[0] + dx * (along - keel) - dy * side * width,
                        shoot[1] + dy * (along - keel) + dx * side * width,
                        z,
                    ]);
                    let normal = [
                        -dx * 0.39 - dy * side * 0.48,
                        -dy * 0.39 + dx * side * 0.48,
                        0.74 + t * 0.18,
                    ];
                    let len = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
                    mesh.normals.push(normal.map(|v| v / len));
                    mesh.colors.push([color[0], color[1], color[2], 1.]);
                }
                if row > 0 {
                    let a = base + 3 * (row - 1);
                    for i in [0, 3, 1, 1, 3, 4, 1, 4, 2, 2, 4, 5, 0, 2, 3, 2, 5, 3] {
                        mesh.indices.push(a + i);
                    }
                }
            }
            let tip = mesh.positions.len() as u32;
            mesh.positions
                .push([shoot[0] + dx * reach, shoot[1] + dy * reach, height]);
            let normal = [-0.3 * dx, -0.3 * dy, (1.0_f64 - 0.09).sqrt()];
            mesh.normals.push(normal);
            mesh.colors.push([leaf[0], leaf[1], leaf[2], 1.]);
            let end = base + 12;
            mesh.indices
                .extend([end, tip, end + 1, end + 1, tip, end + 2, end + 2, tip, end]);
        }
    }
    Ok(NativeOutput {
        meshes: vec![mesh],
        attachments: vec![],
    })
}
