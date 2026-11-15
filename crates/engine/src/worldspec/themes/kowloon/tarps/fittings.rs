use super::{Fit, Form, normal};
use crate::worldspec::themes::kowloon::{
    fixture::Fixture,
    metal::Kind,
    palette::{shade, srgb},
    weathered,
};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{add, cos, cross, scale, sin, sub},
};
pub(super) fn draw(
    d: &mut Fixture<'_, '_>,
    form: &Form,
    fit: &Fit<'_>,
    normals: &[V3],
    settings: Option<&weathered::Settings>,
) -> Result<(), String> {
    let pen = if let Some(s) = settings {
        d.pen
            .with_weathering(d.mesh, s.clone(), Kind::Frame, None, vec!["metal".into()])?
    } else {
        d.pen.clone()
    };
    let mut steel = Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    let tint = shade(srgb("#a4a9aa"), 0.58);
    let rear = form.supports.rear_rail.map(|p| fit.reference(p));
    let front = form.supports.front_rail.map(|p| fit.reference(p));
    steel.tube("metal", &rear, 0.012, tint, 8)?;
    steel.tube("metal", &front, 0.012, tint, 8)?;
    for i in 0..2 {
        steel.tube("metal", &[rear[i], front[i]], 0.012, tint, 8)?;
        let foot = [rear[i][0], 0.015, fit.envelope.z - fit.envelope.drop - 0.08];
        steel.tube("metal", &[foot, front[i]], 0.011, tint, 6)?;
        for at in [rear[i], foot] {
            steel.cuboid(
                "metal",
                [at[0] - 0.018, 0., at[2] - 0.022],
                [at[0] + 0.018, 0.027, at[2] + 0.007],
                tint,
                "",
            )?;
        }
    }
    for (i, pin) in form.attachments.iter().enumerate() {
        if pin.kind == "rear-seam" && i % 7 != 0 {
            continue;
        }
        let point = fit.at(pin.normalized);
        let mut rail = if pin.kind == "rear-seam" { rear } else { front };
        let mut axis = 0;
        if pin.kind == "side-tie" {
            let side = if pin.normalized[0] < 0.5 { 0 } else { 1 };
            rail = [rear[side], front[side]];
            axis = 1;
        }
        let amount = (point[axis] - rail[0][axis]) / (rail[1][axis] - rail[0][axis]);
        let anchor = std::array::from_fn(|i| rail[0][i] + (rail[1][i] - rail[0][i]) * amount);
        let n = normals[pin.vertex];
        let across = normal(add([1., 0., 0.], scale(n, -n[0])));
        let along = cross(n, across);
        let ring: Vec<_> = (0..9)
            .map(|i| {
                let angle = i as f64 / 8. * PI * 2.;
                add(
                    add(
                        add(point, scale(n, 0.002)),
                        scale(across, cos(angle) * 0.008),
                    ),
                    scale(along, sin(angle) * 0.008),
                )
            })
            .collect();
        d.tube("metal", &ring, 0.002, tint, 4)?;
        let rail_axis = normal(sub(rail[1], rail[0]));
        let up = normal(add([0., 0., 1.], scale(rail_axis, -rail_axis[2])));
        let around = cross(rail_axis, up);
        let mut rope = vec![point];
        rope.extend((0..9).map(|i| {
            let angle = i as f64 / 8. * PI * 2.;
            add(
                add(anchor, scale(up, cos(angle) * 0.017)),
                scale(around, sin(angle) * 0.017),
            )
        }));
        rope.push(point);
        d.tube("tarp", &rope, 0.003, srgb("#968779"), 5)?;
    }
    Ok(())
}
