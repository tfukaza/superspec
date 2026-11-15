//! Reusable structural hardware. Material/atlas choices belong to the calling fixture.
use super::{
    fasteners,
    fixture::Fixture,
    metal::Kind,
    palette::shade,
    pen::{Pen, Tint},
    timber, weathered,
};
use superspec_geometry::{
    V3,
    math::{add, cross, dot, length, scale, sub, unit},
};

#[derive(Clone, Copy)]
pub enum Finish<'a> {
    Metal(Option<&'a weathered::Settings>),
    Timber(&'a timber::Settings),
}
#[derive(Clone, Copy)]
pub struct Steel<'a> {
    pub slot: &'a str,
    pub tint: V3,
    pub finish: Finish<'a>,
}
impl Steel<'_> {
    pub fn toned(self, factor: f64) -> Self {
        Self {
            tint: shade(self.tint, factor),
            ..self
        }
    }
    fn pen(&self, drawing: &mut Fixture<'_, '_>, kind: Kind) -> Result<Pen, String> {
        if self.slot.is_empty()
            || self.slot.len() > 4096
            || self.tint.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("Hardware needs a bounded slot and finite nonnegative tint.".into());
        }
        match self.finish {
            Finish::Metal(Some(s)) => drawing.pen.with_weathering(
                drawing.mesh,
                s.clone(),
                kind,
                None,
                vec![self.slot.into()],
            ),
            Finish::Metal(None) => Ok(drawing.pen.clone()),
            Finish::Timber(s) => drawing.pen.with_timber(s.clone()),
        }
    }
}
pub fn member(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    ends: [V3; 2],
    side: V3,
    section: [f64; 2],
) -> Result<(), String> {
    let pen = steel.pen(drawing, Kind::Frame)?;
    let bevel = if matches!(steel.finish, Finish::Metal(None)) {
        0.
    } else {
        0.0015
    };
    pen.beveled_bar(
        drawing.mesh,
        steel.slot,
        ends[0],
        ends[1],
        side,
        section[0],
        section[1],
        bevel,
        &Tint::Color(steel.tint),
        drawing.repeat(steel.slot),
    )
}
pub fn angle(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    ends: [V3; 2],
    legs: [V3; 2],
    section: [f64; 2],
) -> Result<(), String> {
    let [width, gauge] = section;
    let axis = unit(sub(ends[1], ends[0]))?;
    if !width.is_finite()
        || !gauge.is_finite()
        || width <= 0.
        || gauge <= 0.
        || gauge > width
        || legs.iter().any(|leg| {
            leg.iter().any(|v| !v.is_finite())
                || (length(*leg) - 1.).abs() > 1e-7
                || dot(*leg, axis).abs() > 1e-7
        })
        || dot(legs[0], legs[1]).abs() > 1e-7
    {
        return Err(
            "Angle section requires an orthonormal frame and positive gauge no wider than its leg."
                .into(),
        );
    }
    let shift_a = add(scale(legs[0], width / 2.), scale(legs[1], gauge / 2.));
    let shift_b = add(scale(legs[1], width / 2.), scale(legs[0], gauge / 2.));
    member(
        drawing,
        steel,
        ends.map(|p| add(p, shift_a)),
        legs[0],
        section,
    )?;
    member(
        drawing,
        steel.toned(0.88),
        ends.map(|p| add(p, shift_b)),
        legs[1],
        section,
    )
}
pub fn gusset(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    points: [V3; 3],
    normal: V3,
    gauge: f64,
) -> Result<(), String> {
    if !gauge.is_finite()
        || !(0.000001..=100.).contains(&gauge)
        || (length(normal) - 1.).abs() > 1e-7
    {
        return Err("Gusset needs a unit normal and bounded positive gauge.".into());
    }
    let pen = steel.pen(drawing, Kind::Frame)?;
    let repeat = drawing.repeat(steel.slot);
    let tint = Tint::Color(steel.tint);
    let half = scale(normal, gauge / 2.);
    let front = points.map(|p| add(p, half));
    let back = points.map(|p| sub(p, half));
    pen.face(
        drawing.mesh,
        steel.slot,
        &front,
        normal,
        &tint,
        repeat,
        None,
        None,
    )?;
    pen.face(
        drawing.mesh,
        steel.slot,
        &back,
        scale(normal, -1.),
        &tint,
        repeat,
        None,
        None,
    )?;
    let center = scale(add(add(points[0], points[1]), points[2]), 1. / 3.);
    for i in 0..3 {
        let next = (i + 1) % 3;
        let out = sub(scale(add(points[i], points[next]), 0.5), center);
        pen.face(
            drawing.mesh,
            steel.slot,
            &[front[i], back[i], back[next], front[next]],
            out,
            &tint,
            repeat,
            None,
            None,
        )?;
    }
    Ok(())
}
pub fn rod(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    path: &[V3],
    radius: f64,
    sides: usize,
) -> Result<(), String> {
    steel.pen(drawing, Kind::Pipe)?.tube(
        drawing.mesh,
        steel.slot,
        path,
        radius,
        sides,
        &Tint::Color(steel.tint),
        drawing.repeat(steel.slot),
    )
}
pub fn bolt(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    center: V3,
    out: V3,
    radius: f64,
) -> Result<(), String> {
    let pen = steel.pen(drawing, Kind::Frame)?;
    fasteners::bolt(
        &mut Fixture {
            pen,
            mesh: drawing.mesh,
            repeats: drawing.repeats,
        },
        steel.slot,
        center,
        out,
        radius,
        steel.tint,
        None,
    )
}
pub struct Plate {
    pub center: V3,
    pub across: V3,
    pub up: V3,
    pub out: V3,
    pub size: [f64; 2],
    pub bolts: usize,
}
pub fn fixing_plate(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    plate: &Plate,
) -> Result<(), String> {
    let Plate {
        center,
        across,
        up,
        out,
        size: [width, height],
        bolts,
    } = *plate;
    if ![2, 4].contains(&bolts)
        || [width, height]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0. || *v > 100.)
        || [across, up, out]
            .iter()
            .any(|v| v.iter().any(|v| !v.is_finite()) || (length(*v) - 1.).abs() > 1e-7)
        || dot(across, up).abs() > 1e-7
        || dot(across, out).abs() > 1e-7
        || dot(up, out).abs() > 1e-7
    {
        return Err(
            "Fixing plate requires an orthonormal frame, bounded size and two or four bolts."
                .into(),
        );
    }
    let middle = add(center, scale(out, 0.004));
    member(
        drawing,
        steel,
        [
            add(middle, scale(up, -height / 2.)),
            add(middle, scale(up, height / 2.)),
        ],
        across,
        [width, 0.008],
    )?;
    let face = add(center, scale(out, 0.008));
    for [column, row] in [[-1., -1.], [1., 1.], [-1., 1.], [1., -1.]]
        .into_iter()
        .take(bolts)
    {
        bolt(
            drawing,
            steel.toned(0.7),
            add(
                face,
                add(
                    scale(across, width * 0.32 * column),
                    scale(up, height * 0.34 * row),
                ),
            ),
            out,
            0.011f64.min(width * 0.13).min(height * 0.115),
        )?;
    }
    Ok(())
}
pub fn turnbuckle(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    ends: [V3; 2],
    radius: f64,
) -> Result<(), String> {
    let axis = unit(sub(ends[1], ends[0]))?;
    let middle = scale(add(ends[0], ends[1]), 0.5);
    let half = radius * 5.;
    rod(
        drawing,
        steel,
        &[
            add(middle, scale(axis, -half)),
            add(middle, scale(axis, half)),
        ],
        radius * 2.2,
        6,
    )
}
pub fn chain(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    ends: [V3; 2],
    size: f64,
) -> Result<(), String> {
    let run = sub(ends[1], ends[0]);
    let length = length(run);
    if !size.is_finite()
        || size <= 0.
        || !length.is_finite()
        || length < 1e-6
        || length / (size * 0.8) > 4096.
    {
        return Err(
            "Chain requires distinct finite endpoints and at most 4096 positive-size links.".into(),
        );
    }
    let axis = scale(run, 1. / length);
    let side = unit(cross(
        axis,
        if axis[2].abs() > 0.9 {
            [1., 0., 0.]
        } else {
            [0., 0., 1.]
        },
    ))?;
    let other = cross(axis, side);
    let count = super::mesh::rounded(length / (size * 0.8), 1.).max(1.) as usize;
    for i in 0..count {
        let from = add(ends[0], scale(axis, length * i as f64 / count as f64));
        let to = add(ends[0], scale(axis, length * (i + 1) as f64 / count as f64));
        member(
            drawing,
            if i % 2 == 1 { steel.toned(0.85) } else { steel },
            [from, to],
            if i % 2 == 1 { side } else { other },
            [size * 0.55, size * 0.16],
        )?;
    }
    Ok(())
}
pub fn lattice(
    drawing: &mut Fixture<'_, '_>,
    steel: Steel<'_>,
    chords: [[V3; 2]; 2],
    bays: usize,
    section: [f64; 2],
) -> Result<(), String> {
    if !(1..=1024).contains(&bays) {
        return Err("Lattice requires 1–1024 bays.".into());
    }
    let [[a0, a1], [b0, b1]] = chords;
    let [chord, web] = section;
    let normal = unit(cross(unit(sub(a1, a0))?, unit(sub(b0, a0))?))?;
    member(drawing, steel, [a0, a1], normal, [chord, chord])?;
    member(drawing, steel, [b0, b1], normal, [chord, chord])?;
    let at = |a, b, t| add(a, scale(sub(b, a), t));
    let light = steel.toned(0.9);
    for bay in 0..=bays {
        let t = bay as f64 / bays as f64;
        member(
            drawing,
            light,
            [at(a0, a1, t), at(b0, b1, t)],
            normal,
            [web, web],
        )?;
        if bay == bays {
            continue;
        }
        let next = (bay + 1) as f64 / bays as f64;
        let ends = if bay % 2 == 1 {
            [at(b0, b1, t), at(a0, a1, next)]
        } else {
            [at(a0, a1, t), at(b0, b1, next)]
        };
        member(drawing, light, ends, normal, [web, web])?;
    }
    Ok(())
}
