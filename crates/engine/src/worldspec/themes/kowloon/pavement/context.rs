use super::field::{edge_distance, valid_point};
use serde::Deserialize;
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{V3, math::hypot};

#[derive(Clone, Deserialize)]
pub struct Wall {
    pub host: String,
    pub seam: [V3; 2],
}
pub struct Exposure {
    pub wall_distance: f64,
    pub corridor_width: f64,
}
/// Finite opposing contacts from distinct walls establish width; absence remains infinity.
pub fn exposure(point: V3, walls: &[Wall], budget: &mut Budget<'_>) -> Result<Exposure, String> {
    budget.spend(walls.len())?;
    if !valid_point(point)
        || walls.len() > 20_000
        || walls.iter().any(|w| {
            w.host.is_empty() || w.host.len() > 4096 || w.seam.iter().any(|p| !valid_point(*p))
        })
    {
        return Err("Ground exposure requires bounded identified wall contacts.".into());
    }
    let mut wall_distance = f64::INFINITY;
    let mut corridor_width = f64::INFINITY;
    for w in walls {
        wall_distance = wall_distance.min(edge_distance(point, w.seam[0], w.seam[1]));
    }
    let mut directions: Vec<[f64; 2]> = Vec::new();
    for Wall { seam: [a, b], .. } in walls {
        budget.spend(1 + directions.len())?;
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = hypot(dx, dy);
        if length < 1e-6 || edge_distance(point, *a, *b) > 3. {
            continue;
        }
        let nx = -dy / length;
        let ny = dx / length;
        if directions
            .iter()
            .any(|[x, y]| (nx * x + ny * y).abs() > 0.9999)
        {
            continue;
        }
        directions.push([nx, ny]);
        let mut positive = f64::INFINITY;
        let mut negative = f64::INFINITY;
        let mut positive_host = "";
        let mut negative_host = "";
        for wall in walls {
            budget.spend(1)?;
            let [p, q] = wall.seam;
            let ux = q[0] - p[0];
            let uy = q[1] - p[1];
            let determinant = nx * uy - ny * ux;
            if determinant.abs() < 1e-8 {
                continue;
            }
            let vx = p[0] - point[0];
            let vy = p[1] - point[1];
            let t = (vx * uy - vy * ux) / determinant;
            let u = (vx * ny - vy * nx) / determinant;
            if !(-1e-7..=1. + 1e-7).contains(&u) {
                continue;
            }
            if t > 1e-5 && t < positive {
                positive = t;
                positive_host = &wall.host;
            }
            if t < -1e-5 && -t < negative {
                negative = -t;
                negative_host = &wall.host;
            }
        }
        if positive_host != negative_host {
            corridor_width = corridor_width.min(positive + negative);
        }
    }
    Ok(Exposure {
        wall_distance,
        corridor_width,
    })
}
