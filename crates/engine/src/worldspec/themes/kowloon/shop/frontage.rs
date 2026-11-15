//! Clearance-aware street displays leave a one-meter central entry.
use super::super::{fixture::Fixture, kit::Kit};
use super::{
    display::{self, Kind, Settings, Trade},
    forms,
};
use superspec_geometry::{
    V3,
    math::{cos, sin},
};
use superspec_types::rng::SeededRng;
pub type Available<'a> = &'a dyn Fn(f64, f64, f64, f64) -> Result<bool, String>;
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Plan {
    pub side: String,
    pub kind: Kind,
    pub center: V3,
    pub yaw: f64,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
}
pub fn plan(
    width: f64,
    rng: &mut SeededRng,
    available: Available<'_>,
) -> Result<Vec<Plan>, String> {
    if !width.is_finite() || !(0. ..=40.).contains(&width) {
        return Err("Frontage width must be finite and bounded.".into());
    }
    if width < 1. || !available(width / 2. - 0.5, width / 2. + 0.5, 0.65, 2.2)? {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for side in ["left", "right"] {
        let mut local = SeededRng::from_u32((rng.next_f64() * 4294967295.) as u32);
        let x0 = if side == "left" {
            0.05
        } else {
            width / 2. + 0.5
        };
        let x1 = if side == "left" {
            width / 2. - 0.5
        } else {
            width - 0.05
        };
        if x1 <= x0 {
            continue;
        }
        let mut kinds = display::KINDS;
        for i in (1..kinds.len()).rev() {
            let j = local.integer(0, i);
            kinds.swap(i, j);
        }
        for kind in kinds {
            let [w, d, height] = kind.size();
            let mut yaw = local.float(-0.14, 0.14);
            let mut turned = w * cos(yaw) + d * sin(yaw).abs();
            let mut depth = d * cos(yaw) + w * sin(yaw).abs();
            if turned > x1 - x0 || depth + 0.04 > 0.65 {
                yaw = 0.;
                turned = w;
                depth = d;
            }
            if turned > x1 - x0 || depth + 0.04 > 0.65 {
                continue;
            }
            let x = if side == "left" {
                x0 + turned / 2.
            } else {
                x1 - turned / 2.
            };
            if !available(x - turned / 2., x + turned / 2., depth + 0.04, height)? {
                continue;
            }
            result.push(Plan {
                side: side.into(),
                kind,
                center: [x, depth / 2. + 0.04, 0.],
                yaw,
                width: turned,
                depth,
                height,
            });
            break;
        }
    }
    Ok(result)
}
pub fn draw(
    d: &mut Fixture<'_, '_>,
    width: f64,
    trade: Trade,
    kit: &Kit,
    key: &str,
    available: Available<'_>,
    settings: &Settings<'_>,
) -> Result<Vec<Plan>, String> {
    d.mesh.check_work(0)?;
    let mut shown = vec![];
    for plan in plan(
        width,
        &mut kit.rng(&format!("{key}/frontage-plan"))?,
        available,
    )? {
        if !kit.has(&format!("{key}/frontage/{}", plan.side), 0.85)? {
            continue;
        }
        let pen = forms::rotated(&d.pen.into_part("shop-frontage")?, plan.center, 2, plan.yaw)?;
        display::draw(
            &mut Fixture {
                pen,
                mesh: d.mesh,
                repeats: d.repeats,
            },
            plan.kind,
            trade,
            &mut kit.rng(&format!("{key}/frontage/{}/goods", plan.side))?,
            settings,
        )?;
        shown.push(plan);
    }
    Ok(shown)
}
