//! Roller curtains use one unfolded field across the corrugation; hardware forms its own assembly.
use super::{
    graphics_detail::GraphicsDetail,
    mesh::Mesh,
    metal::Kind,
    noise::value_noise,
    palette::{mix, srgb},
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
    headed_roller_shutter(pen, mesh, span, paint, settings, [0.22, 0.])
}

/// [`roller_shutter`] with its head box as given: [height in meters, rust from 0 to 1].
pub fn headed_roller_shutter(
    pen: &Pen,
    mesh: &mut Mesh<'_>,
    span: [f64; 4],
    paint: V3,
    settings: &Settings,
    head_box: [f64; 2],
) -> Result<(), String> {
    slatted_roller_shutter(pen, mesh, span, paint, settings, head_box, SLATS)
}

/// The curtain's corrugation unless given: [pitch, rise] of a slat in meters.
pub const SLATS: [f64; 2] = [0.09, 0.022];
/// A corrugation of fewer, deeper slats, which reads from across a street.
pub const BOLD_SLATS: [f64; 2] = [0.15, 0.029];

/// [`headed_roller_shutter`] with its curtain's slats as given: [pitch from 0.04 to 0.5 m,
/// rise from 0.005 to 0.04 m out of the curtain's plane].
pub fn slatted_roller_shutter(
    pen: &Pen,
    mesh: &mut Mesh<'_>,
    span: [f64; 4],
    paint: V3,
    settings: &Settings,
    head_box: [f64; 2],
    slats: [f64; 2],
) -> Result<(), String> {
    let [x0, x1, raised, head] = span;
    let [box_height, rust] = head_box;
    if !(0.05..=2.).contains(&box_height) || !(0. ..=1.).contains(&rust) {
        return Err("A shutter head box is 0.05 to 2 m high and rusted from 0 to 1.".into());
    }
    let [pitch, rise] = slats;
    if !(0.04..=0.5).contains(&pitch) || !(0.005..=0.04).contains(&rise) {
        return Err("A shutter's slats are 0.04 to 0.5 m apart and rise 0.005 to 0.04 m.".into());
    }
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
    // Where a slat's crest stands; the usual slats keep theirs to the last bit.
    let proud = if slats == SLATS { 0.052 } else { 0.03 + rise };
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
    let detail = mesh.graphics_detail();
    let mut z = 0.;
    if detail == GraphicsDetail::VeryLow {
        // The lowest level hangs the curtain as one sheet at its slats' mean plane; its
        // paint field is the unfolded curtain's, so its colour and wear stay where they were.
        curtain.face(
            mesh,
            "metal",
            &[
                [0., (0.03 + proud) / 2., 0.],
                [width, (0.03 + proud) / 2., 0.],
                [width, (0.03 + proud) / 2., height],
                [0., (0.03 + proud) / 2., height],
            ],
            [0., 1., 0.],
            &color,
            settings.metal.repeat,
            None,
            None,
        )?;
        z = height;
    }
    while z < height - 1e-6 {
        let top = height.min(z + pitch);
        let crest = (z + top) / 2.;
        curtain.face(
            mesh,
            "metal",
            &[
                [0., 0.03, z],
                [width, 0.03, z],
                [width, proud, crest],
                [0., proud, crest],
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
                [0., proud, crest],
                [width, proud, crest],
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
    let boxed = if rust > 0. {
        Tint::Color(std::array::from_fn(|k| {
            paint[k] + ([0.196, 0.068, 0.023][k] - paint[k]) * rust
        }))
    } else {
        color
    };
    let [left, right, top] = [x0 - 0.06, x1 + 0.06, head + box_height];
    if rust == 0. {
        return hardware.cuboid(
            mesh,
            "metal",
            [left, 0., head],
            [right, 0.13, top],
            &boxed,
            settings.metal.repeat,
            "b",
        );
    }
    hardware.cuboid(
        mesh,
        "metal",
        [left, 0., head],
        [right, 0.13, top],
        &boxed,
        settings.metal.repeat,
        "bf",
    )?;
    // A rusted box's face is streaked: runs of rust down from its top, heavier toward its
    // drip edge, over patches where the grey paint still holds.
    let streaked = |p: V3| -> Result<V3, String> {
        let low = 1. - (p[2] - head) / box_height;
        let streak = value_noise(p[0] * 13., p[2] * 1.3, 31.);
        let fine = value_noise(p[0] * 41., p[2] * 2.5, 37.);
        let held = value_noise(p[0] * 2.4, p[2] * 5., 43.);
        let grey = mix(paint, srgb("#8e8b82"), 0.55);
        let base = mix(grey, paint, (held * 1.6 - 0.4).clamp(0., 1.));
        let amount =
            (rust * (0.45 + 0.9 * (streak - 0.5) + 0.45 * (fine - 0.5) + 0.4 * low)).clamp(0., 1.);
        let rusted = mix(base, srgb("#6b3a20"), amount);
        let dark = 1. - 0.4 * rust * ((fine - 0.55) / 0.45).max(0.);
        Ok(rusted.map(|v| v * dark))
    };
    // The streaks are painted per vertex: a reduced level samples them more coarsely.
    let columns = ((right - left) / detail.pick(0.03, 0.12, 0.4))
        .ceil()
        .clamp(1., 400.) as usize;
    let rows = ((box_height / 0.05).ceil() as usize).clamp(2, 16);
    let rows = detail.pick(rows, 2, 1);
    for i in 0..columns {
        let [a, b] = [i, i + 1].map(|k| left + (right - left) * k as f64 / columns as f64);
        for j in 0..rows {
            let [c, e] = [j, j + 1].map(|k| head + box_height * k as f64 / rows as f64);
            hardware.face(
                mesh,
                "metal",
                &[[a, 0.13, c], [b, 0.13, c], [b, 0.13, e], [a, 0.13, e]],
                [0., 1., 0.],
                &Tint::Field(&streaked),
                settings.metal.repeat,
                None,
                None,
            )?;
        }
    }
    Ok(())
}
