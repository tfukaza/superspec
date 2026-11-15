//! Roof terraces use the same pavement implementation, scaled for their viewing distance.
use super::{
    mesh::Mesh,
    pavement::{
        context::Wall,
        field, layout, plan,
        render::{self, Assets, Site},
    },
};
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{V3, math::orient};
pub struct Settings<'a> {
    pub host: &'a str,
    pub seed: &'a str,
    pub grime: f64,
    pub wear: f64,
    pub fine_marks: bool,
}
/// Materials are resolved once per building: quarry-red ceramic and optional authored felt.
pub fn lay(
    mesh: &mut Mesh<'_>,
    settings: &Settings<'_>,
    assets: &Assets,
    key: &str,
    corners: &[V3],
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    if !(3..=4096).contains(&corners.len())
        || key.len() > 2048
        || corners
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("Deck requires a bounded identified convex outline.".into());
    }
    budget.spend(corners.len() * corners.len())?;
    let area = field::area(corners);
    let sign = area.signum();
    if area.abs() < 1e-8
        || corners.iter().any(|p| (p[2] - corners[0][2]).abs() > 0.001)
        || corners.iter().enumerate().any(|(i, &a)| {
            let b = corners[(i + 1) % corners.len()];
            corners.iter().any(|&p| sign * orient(a, b, p) < -1e-8)
        })
    {
        return Err("Deck requires a nondegenerate horizontal convex outline.".into());
    }
    let mut outline = corners.to_vec();
    if area < 0. {
        outline.reverse();
    }
    let scale = (area.abs() / 40.).sqrt();
    let sampling = (0.7 * scale).clamp(0.7, 1.3);
    let options = plan::Settings {
        patch_size: (2.4 * scale).clamp(2.4, 4.),
        material_size: (6. * scale).clamp(6., 10.),
        relief: 0.03,
        wetness: 0.35,
        litter: 0.,
        repairs: 0.6,
        tile_share: 0.12,
        timber_share: 0.,
        joined_pours: 0.6,
        asphalt: 0.4,
        manholes: 0.,
        breakup: 0.,
    };
    let layout = layout::footprint(
        &[outline.clone()],
        options.patch_size,
        options.material_size,
        budget,
    )?;
    let edges = outline
        .iter()
        .enumerate()
        .map(|(i, &p)| [p, outline[(i + 1) % outline.len()]])
        .collect::<Vec<_>>();
    let walls = edges
        .iter()
        .enumerate()
        .map(|(i, e)| Wall {
            host: format!("{key}/edge-{i}"),
            seam: *e,
        })
        .collect::<Vec<_>>();
    let faces = (1..outline.len() - 1)
        .map(|i| vec![outline[0], outline[i], outline[i + 1]])
        .collect::<Vec<_>>();
    let seed = format!("{}/{key}/deck", settings.seed);
    let site = Site {
        host: settings.host,
        part: "roof",
        seed: &seed,
        layout: &layout,
        faces: &faces,
        seams: &edges,
        walls: &walls,
        settings: &options,
        grime: settings.grime,
        wear: settings.wear,
        sampling,
        tile_size: 0.5,
        tile_aspect: None,
        tile_wear: None,
        tile_grime: None,
        density: None,
        asphalt_density: None,
        fine_marks: settings.fine_marks,
        flush: true,
    };
    render::draw_preserved(mesh, &site, assets, budget)?;
    Ok(())
}
