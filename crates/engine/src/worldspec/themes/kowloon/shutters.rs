//! Roller curtains use one unfolded field across the corrugation; hardware forms its own assembly.
use super::{
    mesh::Mesh,
    metal::Kind,
    pen::{Pen, Tint},
    weathered::{Mapping, Settings},
};
use superspec_geometry::{V3, math::hypot};

/// Facade-local meters: [left, right, raised bottom, head]. Errors discard the detached host job.
pub fn roller_shutter(
    pen: &Pen,
    mesh: &mut Mesh<'_>,
    span: [f64; 4],
    paint: V3,
    settings: &Settings,
) -> Result<(), String> {
    let [x0, x1, raised, head] = span;
    if span.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
        || x1 <= x0
        || head <= raised
        || raised < 0.
        || head > 100.
        || x1 - x0 > 100.
        || paint.iter().any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("Roller shutter requires bounded positive dimensions and paint.".into());
    }
    let width = x1 - x0;
    let height = head - raised;
    let pitch = 0.09;
    let rise = 0.022;
    let stretch = hypot(pitch / 2., rise) / (pitch / 2.);
    let key = format!(
        "curtain/{}/{}",
        ryu_js::Buffer::new().format(width),
        ryu_js::Buffer::new().format(height)
    );
    let curtain = pen.moved([x0, 0., raised])?.with_weathering(
        mesh,
        settings.clone(),
        Kind::Sheet,
        Some(Mapping {
            bounds: [0., 0., width, height * stretch],
            axes: [[1., 0., 0.], [0., 0., stretch]],
            offset: [0., 0.],
            gravity: [0., -1.],
            paint,
            key,
        }),
        vec!["paint".into(), "metal".into()],
    )?;
    let color = Tint::Color(paint);
    let mut z = 0.;
    while z < height - 1e-6 {
        let top = height.min(z + pitch);
        let crest = (z + top) / 2.;
        curtain.face(
            mesh,
            "metal",
            &[
                [0., 0.03, z],
                [width, 0.03, z],
                [width, 0.052, crest],
                [0., 0.052, crest],
            ],
            [0., 1., -0.4],
            &color,
            settings.metal.repeat,
            None,
            None,
        )?;
        curtain.face(
            mesh,
            "metal",
            &[
                [0., 0.052, crest],
                [width, 0.052, crest],
                [width, 0.03, top],
                [0., 0.03, top],
            ],
            [0., 1., 0.4],
            &color,
            settings.metal.repeat,
            None,
            None,
        )?;
        z += pitch;
    }
    let hardware = pen.with_weathering(
        mesh,
        settings.clone(),
        Kind::Frame,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    hardware.cuboid(
        mesh,
        "metal",
        [x0, 0.02, raised],
        [x1, 0.065, raised + 0.07],
        &color,
        settings.metal.repeat,
        "b",
    )?;
    for x in [x0 - 0.06, x1] {
        hardware.cuboid(
            mesh,
            "metal",
            [x, 0., 0.],
            [x + 0.06, 0.075, head],
            &color,
            settings.metal.repeat,
            "bd",
        )?;
        hardware.cuboid(
            mesh,
            "metal",
            [x + 0.012, 0.075, 0.],
            [x + 0.025, 0.092, head],
            &color,
            settings.metal.repeat,
            "bd",
        )?;
    }
    hardware.cuboid(
        mesh,
        "metal",
        [x0 - 0.06, 0., head],
        [x1 + 0.06, 0.13, head + 0.22],
        &color,
        settings.metal.repeat,
        "b",
    )
}
