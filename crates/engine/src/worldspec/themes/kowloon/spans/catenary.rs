//! Uniform hanging line in world meters; sampling retains exact authored support endpoints.
use superspec_geometry::obstructions::Budget;
use superspec_geometry::{V3, math::hypot};
pub fn hanging(
    from: V3,
    to: V3,
    slack: f64,
    segments: usize,
    budget: &mut Budget<'_>,
) -> Result<Vec<V3>, String> {
    if from
        .iter()
        .chain(&to)
        .any(|v| !v.is_finite() || v.abs() > 1e6)
        || !slack.is_finite()
        || !(-1. ..=1.).contains(&slack)
        || !(1..=4096).contains(&segments)
    {
        return Err(
            "Hanging lines require finite supports, bounded slack and 1–4096 segments.".into(),
        );
    }
    budget.spend(segments + 12)?;
    let run = hypot(to[0] - from[0], to[1] - from[1]);
    let rise = to[2] - from[2];
    let length = hypot(run, rise) * (1. + slack.max(0.));
    let ratio = if run > 1e-6 {
        (length * length - rise * rise).sqrt() / run
    } else {
        1.
    };
    let straight = ratio - 1. < 1e-9 || run < 0.05 * rise.abs();
    let (mut scale, mut low, mut base) = (0., 0., 0.);
    if !straight {
        let mut u = (6. * (ratio - 1.)).sqrt();
        for _ in 0..12 {
            u -= (u.sinh() - ratio * u) / (u.cosh() - ratio);
        }
        scale = run / (2. * u);
        low = run / 2. - scale * (rise / length).atanh();
        base = from[2] - scale * (low / scale).cosh();
    }
    let mut points: Vec<_> = (0..=segments)
        .map(|i| {
            let t = i as f64 / segments as f64;
            [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
                if straight {
                    from[2] + rise * t
                } else {
                    scale * ((run * t - low) / scale).cosh() + base
                },
            ]
        })
        .collect();
    points[0] = from;
    points[segments] = to;
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Hanging line solver produced a nonfinite point.".into());
    }
    Ok(points)
}
