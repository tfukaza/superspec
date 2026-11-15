use crate::Scene;
use superspec_aml::architecture::Node;
use superspec_geometry::{image::Image, math::*};

pub(super) struct Camera {
    position: V3,
    right: V3,
    up: V3,
    forward: V3,
    aspect: f64,
    tangent: f64,
    near: f64,
    far: f64,
}
impl Camera {
    pub fn new(reference: &Node) -> Result<Self, String> {
        let position = reference.vector("position", [0.; 3]);
        let forward = unit(sub(reference.vector("target", [0.; 3]), position))?;
        let right = unit(cross(forward, reference.vector("up", [0., 0., 1.])))?;
        let up = cross(right, forward);
        Ok(Self {
            position,
            right,
            up,
            forward,
            aspect: reference.number("width", 1.) / reference.number("height", 1.),
            tangent: (reference.number("fov", 45.).to_radians() / 2.).tan(),
            near: reference.number("near", 0.05),
            far: reference.number("far", 2000.),
        })
    }
    pub fn project(&self, p: V3) -> Option<[f64; 3]> {
        let d = sub(p, self.position);
        let z = dot(d, self.forward);
        (z >= self.near && z <= self.far).then(|| {
            [
                0.5 + dot(d, self.right) / (2. * self.tangent * z * self.aspect),
                0.5 - dot(d, self.up) / (2. * self.tangent * z),
                z,
            ]
        })
    }
}
pub struct Raster {
    pub image: Image,
    pub surfaces: Vec<i32>,
    pub depth: Vec<f64>,
}
fn edge(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

pub fn render(
    scene: &Scene,
    reference: &Node,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Raster, String> {
    if !(32..=4096).contains(&limit) {
        return Err("Reference raster limit must be 32..4096.".into());
    }
    let camera = Camera::new(reference)?;
    let scale = (limit as f64
        / reference
            .number("width", 1.)
            .max(reference.number("height", 1.)))
    .min(1.);
    let width = (reference.number("width", 1.) * scale).round().max(1.) as usize;
    let height = (reference.number("height", 1.) * scale).round().max(1.) as usize;
    let mut rgba = vec![0; 4 * width * height];
    for p in rgba.chunks_exact_mut(4) {
        p.copy_from_slice(&[230, 238, 243, 255]);
    }
    let mut raster = Raster {
        image: Image {
            width,
            height,
            rgba,
            mime: "image/png",
        },
        surfaces: vec![-1; width * height],
        depth: vec![f64::INFINITY; width * height],
    };
    let materials = scene
        .manifest
        .materials
        .iter()
        .map(|m| (m.id.as_str(), m.color.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut work = 0usize;
    for (mesh_index, mesh) in scene.manifest.meshes.iter().enumerate() {
        if !mesh.visible {
            continue;
        }
        let hex = mesh
            .material
            .as_deref()
            .and_then(|id| materials.get(id))
            .copied()
            .unwrap_or("#b9c7d1");
        let color = [
            u8::from_str_radix(&hex[1..3], 16).map_err(|e| e.to_string())?,
            u8::from_str_radix(&hex[3..5], 16).map_err(|e| e.to_string())?,
            u8::from_str_radix(&hex[5..7], 16).map_err(|e| e.to_string())?,
            255,
        ];
        let projected = (mesh.vertex_start..mesh.vertex_start + mesh.vertex_count)
            .map(|i| {
                camera.project([
                    scene.positions[i * 3] as f64,
                    scene.positions[i * 3 + 1] as f64,
                    scene.positions[i * 3 + 2] as f64,
                ])
            })
            .collect::<Vec<_>>();
        for triangle in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            if cancelled() {
                return Err("Reference rasterization cancelled.".into());
            }
            let [Some(a), Some(b), Some(c)] = [
                projected[triangle[0] as usize],
                projected[triangle[1] as usize],
                projected[triangle[2] as usize],
            ] else {
                continue;
            };
            let p = [a, b, c].map(|p| [p[0] * width as f64, p[1] * height as f64, p[2]]);
            let area = edge(p[0], p[1], p[2]);
            if area.abs() < 1e-9 {
                continue;
            }
            let min_x = p
                .iter()
                .map(|p| p[0])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.) as usize;
            let max_x = p
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .min((width - 1) as f64)
                .max(0.) as usize;
            let min_y = p
                .iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.) as usize;
            let max_y = p
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .min((height - 1) as f64)
                .max(0.) as usize;
            if min_x >= width || min_y >= height {
                continue;
            }
            work = work.saturating_add(
                (max_x.saturating_sub(min_x) + 1) * (max_y.saturating_sub(min_y) + 1),
            );
            if work > 250000000 {
                return Err("Reference raster work allowance exceeded.".into());
            }
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let q = [x as f64 + 0.5, y as f64 + 0.5, 0.];
                    let w0 = edge(p[1], p[2], q) / area;
                    let w1 = edge(p[2], p[0], q) / area;
                    let w2 = 1. - w0 - w1;
                    if w0.min(w1).min(w2) < -1e-9 {
                        continue;
                    }
                    let z = 1. / (w0 / p[0][2] + w1 / p[1][2] + w2 / p[2][2]);
                    let i = y * width + x;
                    if z < raster.depth[i] {
                        raster.depth[i] = z;
                        raster.surfaces[i] = mesh_index as i32;
                        raster.image.rgba[i * 4..i * 4 + 4].copy_from_slice(&color);
                    }
                }
            }
        }
    }
    Ok(raster)
}
