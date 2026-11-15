//! Solid washers and chamfered hexagonal bolt heads, in fixture-local meters.
use super::{fixture::Fixture, palette::shade};
use std::f64::consts::PI;
use superspec_geometry::{
    V3,
    math::{add, cos, cross, length, scale, sin, unit},
};

pub fn bolt(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    center: V3,
    out: V3,
    radius: f64,
    tint: V3,
    weathering: Option<&super::weathered::Settings>,
) -> Result<(), String> {
    let original = drawing.pen.clone();
    if let Some(settings) = weathering {
        drawing.pen = original.with_weathering(
            drawing.mesh,
            settings.clone(),
            super::metal::Kind::Frame,
            None,
            vec![slot.into()],
        )?;
    }
    let result = bolt_geometry(drawing, slot, center, out, radius, tint);
    drawing.pen = original;
    result
}
fn bolt_geometry(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    center: V3,
    out: V3,
    radius: f64,
    tint: V3,
) -> Result<(), String> {
    if !radius.is_finite()
        || !(0.000001..=10000.).contains(&radius)
        || center
            .iter()
            .chain(&out)
            .chain(&tint)
            .any(|v| !v.is_finite())
        || (length(out) - 1.).abs() > 1e-7
        || tint.iter().any(|v| *v < 0.)
    {
        return Err("Bolt requires a bounded positive radius, unit outward axis and finite nonnegative tint.".into());
    }
    let washer_top = add(center, scale(out, radius * 0.18));
    drawing.tube(
        slot,
        &[center, washer_top],
        radius * 1.28,
        shade(tint, 0.8),
        12,
    )?;
    drawing.disc(slot, washer_top, radius * 1.28, out, shade(tint, 0.8), 12)?;
    let shoulder = add(washer_top, scale(out, radius * 0.5));
    let top = add(shoulder, scale(out, radius * 0.12));
    drawing.tube(slot, &[washer_top, shoulder], radius, tint, 6)?;
    let side = unit(cross(
        out,
        if out[2].abs() > 0.9 {
            [1., 0., 0.]
        } else {
            [0., 0., 1.]
        },
    ))?;
    let other = cross(out, side);
    for i in 0..6 {
        let radial = |angle: f64| add(scale(side, cos(angle)), scale(other, sin(angle)));
        let a = radial(i as f64 * PI / 3.);
        let b = radial((i + 1) as f64 * PI / 3.);
        drawing.face(
            slot,
            &[
                add(shoulder, scale(a, radius)),
                add(shoulder, scale(b, radius)),
                add(top, scale(b, radius * 0.83)),
                add(top, scale(a, radius * 0.83)),
            ],
            add(a, b),
            tint,
        )?;
    }
    drawing.disc(slot, top, radius * 0.83, out, tint, 6)
}
