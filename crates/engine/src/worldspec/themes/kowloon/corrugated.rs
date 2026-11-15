//! Two-sided corrugated sheets with a continuous unfolded weathering chart.
use super::{
    fixture::Fixture,
    mesh::rounded,
    metal::Kind,
    palette::shade,
    pen::Tint,
    weathered::{Mapping, Settings},
};
use superspec_geometry::{
    V3,
    math::{cross, dot, hypot, length, scale, sub},
};

pub fn draw(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    corners: [V3; 4],
    pitch: f64,
    tint: V3,
    weathering: Option<&Settings>,
) -> Result<(), String> {
    let [a, b, c, d] = corners;
    if corners
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 10000.)
        || !pitch.is_finite()
        || pitch <= 0.
        || pitch > 10000.
    {
        return Err("Corrugation needs bounded finite corners and a positive pitch.".into());
    }
    if !drawing.mesh.surveying() && tint.iter().any(|v| !v.is_finite() || *v < 0.) {
        return Err("Corrugation requires finite nonnegative tint.".into());
    }
    let across = sub(b, a);
    let along = sub(d, a);
    let normal = cross(across, along);
    let size = length(normal);
    let width = length(across);
    let run = length(along);
    let steps = (rounded(width / pitch, 1.) * 2.).max(2.);
    if size < 1e-10 || width < 1e-6 || run < 1e-6 || steps > 4096. {
        return Err("Corrugated sheet needs a nondegenerate frame and at most 4096 facets.".into());
    }
    let unfolded = steps * hypot(width / steps, pitch * 0.28);
    let pen = if let Some(settings) = weathering.filter(|_| !drawing.mesh.surveying()) {
        let axes = [
            scale(across, unfolded / (width * width)),
            scale(along, -1. / run),
        ];
        let format = |v| ryu_js::Buffer::new().format(v).to_owned();
        let key = format!(
            "sheet/{}/{}/{}",
            a.map(format).join(","),
            format(width),
            format(run)
        );
        drawing.pen.with_weathering(
            drawing.mesh,
            settings.clone(),
            Kind::Sheet,
            Some(Mapping {
                bounds: [0., 0., unfolded, run],
                axes,
                offset: [-dot(a, axes[0]), run - dot(a, axes[1])],
                gravity: [-across[2] / unfolded, along[2] / run],
                paint: tint,
                key,
            }),
            vec![slot.into()],
        )?
    } else {
        drawing.pen.clone()
    };
    let at = |from: V3, to: V3, i: usize| {
        let t = i as f64 / steps;
        let lift = if i % 2 == 1 { 1. } else { -1. } * pitch * 0.14;
        std::array::from_fn(|k| from[k] + (to[k] - from[k]) * t + (normal[k] / size) * lift)
    };
    let repeat = drawing.repeat(slot);
    for i in 0..steps as usize {
        pen.sheet(
            drawing.mesh,
            slot,
            &[at(a, b, i), at(a, b, i + 1), at(d, c, i + 1), at(d, c, i)],
            &Tint::Color(shade(tint, if i % 2 == 1 { 1. } else { 0.88 })),
            &Tint::Color(shade(tint, 0.6)),
            repeat,
        )?;
    }
    Ok(())
}
