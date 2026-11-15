//! Common room coordinates and independently seeded amenities.
use super::super::{fixture::Fixture, kit::Kit, pen::Pen};
use super::fittings;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy)]
pub struct Assets {
    pub posters: bool,
    pub grocer_props: bool,
    pub snack_packets: bool,
    pub market_apple: bool,
    pub shop_register: bool,
    pub market_trays: bool,
}
pub struct Room<'a> {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub kit: &'a Kit,
    pub key: &'a str,
    pub assets: Assets,
}
impl Room<'_> {
    pub fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || !(1.8 - 1e-8..=40.).contains(&self.width)
            || !self.depth.is_finite()
            || !(2.4..=20.).contains(&self.depth)
            || !self.height.is_finite()
            || !(2. ..=10.).contains(&self.height)
            || self.key.is_empty()
            || self.key.len() > 1024
        {
            Err("Shop room requires bounded usable dimensions and a seeded identity.".into())
        } else {
            Ok(())
        }
    }
    pub fn stream(&self, name: &str) -> Result<SeededRng, String> {
        self.kit.rng(&format!("{}/{name}", self.key))
    }
    pub fn extra(&self, name: &str, chance: f64) -> Result<bool, String> {
        self.kit.has(&format!("{}/{name}", self.key), chance)
    }
    pub fn left_wall(&self, pen: &Pen, y: f64, out: f64) -> Result<Pen, String> {
        pen.facing([out, y, 0.], [0., 1., 0.], [-1., 0., 0.])
    }
    pub fn right_wall(&self, pen: &Pen, y: f64, out: f64) -> Result<Pen, String> {
        pen.facing([self.width - out, y, 0.], [0., -1., 0.], [1., 0., 0.])
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interior {
    pub floor: V3,
    pub ceiling: V3,
    pub wall: V3,
    pub dado: V3,
    pub dado_height: f64,
}
pub fn amenities(
    d: &mut Fixture<'_, '_>,
    room: &Room<'_>,
    y: f64,
    wall_from: Option<f64>,
) -> Result<(), String> {
    room.validate()?;
    if !y.is_finite()
        || !(0. ..=room.depth).contains(&y)
        || wall_from.is_some_and(|v| !v.is_finite() || v < 0. || v > room.depth)
    {
        return Err("Shop amenities require bounded room positions.".into());
    }
    let (w, depth, h) = (room.width, room.depth, room.height);
    fittings::tube_light(d, [w * 0.2, y, h], [w * 0.8, y, h])?;
    let span = 1.1_f64.min(w - 0.7);
    let fan_at = (depth - 0.9).min(y + 0.75);
    if h >= 2.2 && fan_at - y >= 0.62 && room.extra("fan", 0.45)? {
        fittings::ceiling_fan(d, [w / 2., fan_at, h], 0.28, span, &mut room.stream("fan")?)?;
    }
    let Some(wall_from) = wall_from else {
        return Ok(());
    };
    let pen = room.right_wall(&d.pen, wall_from + 0.5, 0.)?;
    let wall = &mut Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    };
    if room.extra("calendar", 0.6)? {
        fittings::calendar(
            wall,
            [0.18, 0., (h - 0.25).min(1.95)],
            &mut room.stream("calendar")?,
            room.assets.posters,
        )?;
    }
    if room.extra("clock", 0.4)? {
        fittings::wall_clock(
            wall,
            [-0.2, 0., (h - 0.3).min(1.9)],
            &mut room.stream("clock")?,
        )?;
    }
    Ok(())
}
