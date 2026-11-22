//! Space-saving cafe tables and roast-rice place settings. All furniture is floor
//! anchored; a setting can also be placed directly on a shallow wall counter.
use super::super::super::palette::{shade, srgb};
use super::{Fixture, Tint, V3, food, forms, on, service};
use superspec_geometry::math::{cos, sin};

fn tissue_holder(d: &mut Fixture<'_, '_>, x: f64, y: f64) -> Result<(), String> {
    let at = [x, y, 0.];
    let base = service::rounded_rect(at, [0.042, 0.062], 0.006, 0.);
    let side = service::rounded_rect(at, [0.044, 0.064], 0.007, 0.048);
    let lid = service::rounded_rect(at, [0.040, 0.060], 0.006, 0.053);
    let teal = srgb("#647e73");
    service::join_rects(d, "paint", at, &base, &side, false, &Tint::Color(teal))?;
    service::join_rects(
        d,
        "pane",
        at,
        &side,
        &lid,
        false,
        &Tint::Color(shade(teal, 1.14)),
    )?;
    d.face("paint", &lid, [0., 0., 1.], teal)?;
    let slot = service::rounded_rect([x, y, 0.0534], [0.005, 0.041], 0.004, 0.);
    d.face("paint", &slot, [0., 0., 1.], srgb("#44463a"))?;
    // Creased paper is visible from either seat. The top tears at an angle.
    let fold = [
        ([x, y - 0.036, 0.0535], [x - 0.003, y - 0.041, 0.081]),
        (
            [x + 0.002, y - 0.011, 0.0535],
            [x + 0.013, y - 0.016, 0.096],
        ),
        (
            [x - 0.002, y + 0.011, 0.0535],
            [x - 0.009, y + 0.014, 0.088],
        ),
        ([x, y + 0.035, 0.0535], [x + 0.001, y + 0.033, 0.073]),
    ];
    for (i, edge) in fold.windows(2).enumerate() {
        let [(a, b), (c, e)] = [edge[0], edge[1]];
        let color = srgb(["#e3decc", "#eee7d6", "#d7d2c0"][i]);
        d.sheet("paint", &[a, c, e], color)?;
        d.sheet("paint", &[a, e, b], color)?;
    }
    Ok(())
}

fn meal(d: &mut Fixture<'_, '_>, at: V3, index: usize, textured: bool) -> Result<(), String> {
    let [x, y, z] = at;
    let rim = srgb(if index.is_multiple_of(2) {
        "#496e72"
    } else {
        "#7a564b"
    });
    service::ceramic_bowl(d, at, 0.088, 0.043, rim, None)?;
    let rice = |[px, py, pz]: V3| {
        let mottling = sin((px - x) * 310. + sin((py - y) * 200.)) * 0.035;
        Ok(shade(srgb("#dcd1ac"), 0.97 + mottling + 0.9 * (pz - z)))
    };
    forms::ball(
        d,
        "paint",
        &forms::Ball {
            center: [x, y, z + 0.039],
            radius: 0.069,
            stretch: [1., 0.96, 0.30],
            sides: 14,
            bands: 6,
        },
        &Tint::Field(&rice),
    )?;
    let food_pen = forms::rotated(
        &d.pen,
        [x - 0.012, y + 0.003, z + 0.053],
        2,
        if index.is_multiple_of(2) { -0.27 } else { 0.22 },
    )?;
    food::sliced_roast(
        &mut on(d, food_pen),
        [0.; 3],
        [0.100, 0.083, 0.037],
        textured,
    )?;
    // Blanched greens sit along the rice beside the overlapping roast slices.
    for (i, angle) in [-0.34, 0.01, 0.32].into_iter().enumerate() {
        let green_pen = forms::rotated(
            &d.pen,
            [x + 0.041, y + (i as f64 - 1.) * 0.010, z + 0.053],
            2,
            angle,
        )?;
        let g = &mut on(d, green_pen);
        g.bar(
            "paint",
            [-0.016, 0., 0.001],
            [0.017, 0., 0.003],
            0.0038,
            srgb("#95a354"),
        )?;
        forms::ball(
            g,
            "paint",
            &forms::Ball {
                center: [0.010, 0., 0.004],
                radius: 0.015,
                stretch: [1., 0.42, 0.23],
                sides: 8,
                bands: 4,
            },
            &Tint::Color(srgb(["#718842", "#5b763d", "#7b9248"][i])),
        )?;
    }
    Ok(())
}

fn chopsticks(d: &mut Fixture<'_, '_>, x: f64, y: f64, turn: f64) -> Result<(), String> {
    let pen = forms::rotated(&d.pen, [x, y, 0.], 2, turn)?;
    let d = &mut on(d, pen);
    d.cuboid(
        "pane",
        [-0.014, 0.043, 0.],
        [0.017, 0.055, 0.009],
        srgb("#698380"),
        "d",
    )?;
    for (i, x) in [-0.004, 0.006].into_iter().enumerate() {
        let p = |t: f64| [x + t * 0.002, -0.099 + t * 0.196, 0.003 + t * 0.012];
        let bamboo = Tint::Color(srgb(if i == 0 { "#b68b5e" } else { "#bf9567" }));
        forms::limb(
            d,
            "timber",
            &[p(0.), p(0.25), p(0.79)],
            &[0.0019, 0.0024, 0.0030],
            &bamboo,
            6,
            1.,
        )?;
        forms::limb(
            d,
            "paint",
            &[p(0.79), p(1.)],
            &[0.0030, 0.0031],
            &Tint::Color(srgb("#873e34")),
            6,
            1.,
        )?;
        d.disc("paint", p(1.), 0.0031, [0., 1., 0.06], srgb("#93493a"), 6)?;
    }
    Ok(())
}

/// A meal, rest and tea at each occupied place. X runs from the diner towards
/// the wall; the narrower version fits one place on a 0.38 × 0.28 m ledge.
pub(super) fn setting(
    d: &mut Fixture<'_, '_>,
    width: f64,
    depth: f64,
    index: usize,
    textured: bool,
) -> Result<(), String> {
    if !width.is_finite() || !depth.is_finite() || width < 0.38 || depth < 0.28 {
        return Err("Roast dining settings need at least 0.38 × 0.28 meters.".into());
    }
    let places = if depth >= 0.57 || (depth >= 0.44 && index.is_multiple_of(2)) {
        2
    } else {
        1
    };
    for seat in 0..places {
        let y = (seat as f64 + 0.5) * depth / places as f64;
        let x = (width * 0.37).min(0.21);
        meal(d, [x, y, 0.], index + seat, textured)?;
        chopsticks(d, x + 0.109, y, if seat == 0 { 0.06 } else { -0.08 })?;
        let tea = [
            width - 0.050,
            y + if seat == 0 { -0.059 } else { 0.059 },
            0.,
        ];
        service::ceramic_bowl(d, tea, 0.034, 0.055, srgb("#4e7371"), Some(srgb("#88602e")))?;
    }
    if places == 2 {
        tissue_holder(d, width - 0.056, depth * 0.50)?;
    } else if depth >= 0.37 {
        tissue_holder(d, width - 0.056, depth - 0.070)?;
    } else {
        service::ceramic_bowl(
            d,
            [width - 0.050, depth * 0.50 + 0.050, 0.],
            0.029,
            0.019,
            srgb("#49736e"),
            Some(srgb("#7c422b")),
        )?;
    }
    Ok(())
}

pub(super) fn table(
    d: &mut Fixture<'_, '_>,
    width: f64,
    depth: f64,
    index: usize,
    textured: bool,
) -> Result<(), String> {
    let at = [width * 0.5, depth * 0.5, 0.];
    let outline =
        |inset: f64, z| service::rounded_rect(at, [at[0] - inset, at[1] - inset], 0.020 - inset, z);
    let under = outline(0.003, 0.729);
    let edge = outline(0., 0.733);
    let lip = outline(0., 0.761);
    let top = outline(0.004, 0.766);
    let tan = srgb("#ac825e");
    service::join_rects(
        d,
        "timber",
        at,
        &under,
        &edge,
        false,
        &Tint::Color(shade(tan, 0.86)),
    )?;
    service::join_rects(d, "timber", at, &edge, &lip, false, &Tint::Color(tan))?;
    service::join_rects(
        d,
        "pane",
        at,
        &lip,
        &top,
        false,
        &Tint::Color(srgb("#aea48a")),
    )?;
    d.face("timber", &under, [0., 0., -1.], tan)?;
    let laminate = |[x, y, _]: V3| {
        Ok(shade(
            srgb("#b7b49c"),
            0.97 + 0.027 * sin(x * 17. + y * 6.) + 0.018 * cos(y * 23.),
        ))
    };
    d.pen.face(
        d.mesh,
        "pane",
        &top,
        [0., 0., 1.],
        &Tint::Field(&laminate),
        d.repeat("pane"),
        None,
        None,
    )?;
    let metal = d.pen.with_kitchen_metal(d.repeat("kitchen-metal"))?;
    let frame = &mut on(d, metal);
    for x in [0.045, width - 0.045] {
        for y in [0.045, depth - 0.045] {
            frame.drum("paint", [x, y, 0.], 0.0165, 0.025, srgb("#484d44"), 10)?;
            frame.tube(
                "metal",
                &[[x, y, 0.022], [x, y, 0.730]],
                0.014,
                [0.53, 0.60, 0.53],
                10,
            )?;
        }
        frame.bar(
            "metal",
            [x, 0.045, 0.702],
            [x, depth - 0.045, 0.702],
            0.021,
            [0.39, 0.45, 0.39],
        )?;
    }
    for y in [0.045, depth - 0.045] {
        frame.bar(
            "metal",
            [0.045, y, 0.702],
            [width - 0.045, y, 0.702],
            0.021,
            [0.39, 0.45, 0.39],
        )?;
    }
    let pen = d.pen.moved([0., 0., 0.766])?;
    setting(&mut on(d, pen), width, depth, index, textured)
}
