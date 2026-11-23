//! Shared baked plants and fitted mesh garments; source assets remain renderer-independent data.
use super::{fixture::Fixture, graphics_detail::GraphicsDetail, palette::srgb, pen::Tint};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_geometry::{
    V3,
    math::{cos, hypot, length, sin},
};
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Plant {
    RubberPlant,
    TrailingPothos,
    SnakePlant,
}
impl Plant {
    fn name(self) -> &'static str {
        match self {
            Self::RubberPlant => "rubber-plant",
            Self::TrailingPothos => "trailing-pothos",
            Self::SnakePlant => "snake-plant",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GarmentName {
    HangingShirt,
    HangingTrousers,
    HangingBedsheet,
}
impl GarmentName {
    fn name(self) -> &'static str {
        match self {
            Self::HangingShirt => "hanging-shirt",
            Self::HangingTrousers => "hanging-trousers",
            Self::HangingBedsheet => "hanging-bedsheet",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Line,
    Pole,
    Hanger,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Garment {
    pub asset: GarmentName,
    pub width: f64,
    pub drop: f64,
    pub scale: f64,
    pub support: Support,
    pub tint: V3,
}
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Room {
    pub width: f64,
    pub drop: f64,
}
#[derive(Deserialize)]
struct Asset {
    faces: Vec<Vec<[f64; 11]>>,
    width: f64,
    drop: f64,
    anchors: Option<[V3; 2]>,
}
fn asset(name: &str) -> Result<&'static Asset, String> {
    static CATALOG: OnceLock<Result<BTreeMap<String, Asset>, String>> = OnceLock::new();
    let catalog = CATALOG
        .get_or_init(|| {
            let assets: BTreeMap<String, Asset> =
                serde_json::from_str(include_str!("assets/balcony-props.json"))
                    .map_err(|e| e.to_string())?;
            if assets.len() != 6
                || assets.values().any(|a| {
                    a.faces.len() > 10000
                        || !a.width.is_finite()
                        || a.width <= 0.
                        || !a.drop.is_finite()
                        || a.drop < 0.
                        || a.faces.iter().any(|f| {
                            !(3..=16).contains(&f.len())
                                || f.iter().flatten().any(|v| !v.is_finite())
                        })
                })
            {
                return Err("Invalid embedded balcony asset catalog.".into());
            }
            Ok(assets)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    catalog
        .get(name)
        .ok_or_else(|| format!("Missing embedded balcony asset {name}."))
}
fn point(p: V3) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 10000.)
}
fn emit(
    drawing: &mut Fixture<'_, '_>,
    slot: &str,
    face: &[[f64; 11]],
    points: &[V3],
    normals: &[V3],
    tint: V3,
    textured: bool,
) -> Result<(), String> {
    let hint = normals
        .iter()
        .fold([0.; 3], |a, n| std::array::from_fn(|i| a[i] + n[i]));
    if drawing.mesh.surveying() {
        return drawing.pen.face(
            drawing.mesh,
            slot,
            points,
            hint,
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        );
    }
    let uv: Vec<_> = face.iter().map(|v| [v[6], v[7]]).collect();
    let sampled = |p: V3| {
        let i = points
            .iter()
            .position(|v| *v == p)
            .ok_or("Missing balcony corner.")?;
        Ok(std::array::from_fn(|axis| tint[axis] * face[i][8 + axis]))
    };
    drawing.pen.face(
        drawing.mesh,
        slot,
        points,
        hint,
        &if textured {
            Tint::Color(tint)
        } else {
            Tint::Field(&sampled)
        },
        1.,
        Some(&uv),
        Some(normals),
    )
}
/// The share of a plant's surface its reduced levels keep, largest faces first: levels 1, 2.
const KEPT_AREA: [f64; 2] = [0.75, 0.5];

/// Which faces of a plant each reduced level draws: the largest, in the asset's own order,
/// until they cover [`KEPT_AREA`] of its surface.
fn largest(name: Plant) -> Result<&'static [Vec<bool>; 2], String> {
    static KEPT: OnceLock<Result<BTreeMap<&'static str, [Vec<bool>; 2]>, String>> = OnceLock::new();
    KEPT.get_or_init(|| {
        [Plant::RubberPlant, Plant::TrailingPothos, Plant::SnakePlant]
            .into_iter()
            .map(|plant| {
                let faces = &asset(plant.name())?.faces;
                let areas: Vec<f64> = faces
                    .iter()
                    .map(|face| {
                        let at = |i: usize| [face[i][0], face[i][1], face[i][2]];
                        (1..face.len() - 1)
                            .map(|i| {
                                let (a, b, c) = (at(0), at(i), at(i + 1));
                                let u: V3 = std::array::from_fn(|k| b[k] - a[k]);
                                let v: V3 = std::array::from_fn(|k| c[k] - a[k]);
                                length([
                                    u[1] * v[2] - u[2] * v[1],
                                    u[2] * v[0] - u[0] * v[2],
                                    u[0] * v[1] - u[1] * v[0],
                                ]) / 2.
                            })
                            .sum()
                    })
                    .collect();
                let total: f64 = areas.iter().sum();
                let mut order: Vec<usize> = (0..faces.len()).collect();
                // Ties keep the asset's order, so the choice is the same on every platform.
                order.sort_by(|a, b| areas[*b].total_cmp(&areas[*a]).then(a.cmp(b)));
                let kept = KEPT_AREA.map(|share| {
                    let mut kept = vec![false; faces.len()];
                    let mut covered = 0.;
                    for index in &order {
                        if covered >= total * share {
                            break;
                        }
                        kept[*index] = true;
                        covered += areas[*index];
                    }
                    kept
                });
                Ok((plant.name(), kept))
            })
            .collect()
    })
    .as_ref()
    .map_err(Clone::clone)?
    .get(name.name())
    .ok_or_else(|| format!("Missing embedded balcony asset {}.", name.name()))
}
pub fn plant(
    drawing: &mut Fixture<'_, '_>,
    name: Plant,
    at: V3,
    scale: f64,
    rotation: f64,
    textured: bool,
) -> Result<(), String> {
    if !point(at)
        || !scale.is_finite()
        || !(0.001..=100.).contains(&scale)
        || !rotation.is_finite()
        || rotation.abs() > 1e6
    {
        return Err("Plant placement requires finite bounded pose and positive scale.".into());
    }
    let source = asset(name.name())?;
    let c = cos(rotation);
    let s = sin(rotation);
    let turn = |x: f64, y: f64, z: f64| [c * x - s * y, s * x + c * y, z];
    // A reduced level keeps the plant's largest faces: the pot's sides and the broad leaves
    // that carry its shape and colour, and none of the small ones between them.
    let kept = match drawing.mesh.graphics_detail() {
        GraphicsDetail::Full => None,
        GraphicsDetail::Low => Some(&largest(name)?[0]),
        GraphicsDetail::VeryLow => Some(&largest(name)?[1]),
    };
    for (index, face) in source.faces.iter().enumerate() {
        if kept.is_some_and(|kept| !kept[index]) {
            continue;
        }
        let points: Vec<_> = face
            .iter()
            .map(|v| {
                let p = turn(v[0], v[1], v[2]);
                std::array::from_fn(|i| at[i] + p[i] * scale)
            })
            .collect();
        let normals: Vec<_> = face.iter().map(|v| turn(v[3], v[4], v[5])).collect();
        emit(
            drawing,
            "balcony-plants",
            face,
            &points,
            &normals,
            [1.; 3],
            textured,
        )?;
    }
    Ok(())
}
/// The dyes a garment is one of.
pub(super) const DYES: [&str; 6] = [
    "#f2efe8", "#a9c6cf", "#bdc6a4", "#d0a7a1", "#b8abc8", "#d7c695",
];
/// Selects and uniformly fits a garment without changing the RNG on invalid room admission.
pub fn choose(
    rng: &mut SeededRng,
    room: Room,
    support: Support,
) -> Result<Option<Garment>, String> {
    if [room.width, room.drop]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0. || *v > 100.)
    {
        return Err("Garment room requires positive finite dimensions within 100 m.".into());
    }
    let mut names = vec![
        GarmentName::HangingShirt,
        GarmentName::HangingTrousers,
        GarmentName::HangingBedsheet,
    ];
    names.retain(|name| {
        !((support != Support::Line && *name == GarmentName::HangingBedsheet)
            || (support == Support::Pole && *name == GarmentName::HangingShirt))
    });
    for i in (1..names.len()).rev() {
        let j = rng.integer(0, i);
        names.swap(i, j);
    }
    for name in names {
        let source = asset(name.name())?;
        let hanger_drop = if support == Support::Hanger {
            0.11
        } else if name == GarmentName::HangingBedsheet {
            0.014
        } else {
            0.
        };
        let scale = rng
            .float(0.76, 1.02)
            .min(room.width / source.width)
            .min((room.drop - hanger_drop - 0.015) / source.drop);
        if scale
            < if name == GarmentName::HangingBedsheet {
                0.52
            } else {
                0.65
            }
        {
            continue;
        }
        return Ok(Some(Garment {
            asset: name,
            width: source.width * scale,
            drop: source.drop * scale + hanger_drop + 0.015,
            scale,
            support,
            tint: srgb(rng.pick(&DYES)),
        }));
    }
    Ok(None)
}
/// Hanger normals use inverse-transpose shear; line anchors follow slope and remain fixed under sway.
pub fn hang(
    drawing: &mut Fixture<'_, '_>,
    garment: &Garment,
    left: V3,
    right: V3,
    sway: f64,
    textured: bool,
) -> Result<(), String> {
    let source = asset(garment.asset.name())?;
    let hanger = garment.support == Support::Hanger;
    let drop = if hanger { 0.11 } else { 0. };
    let lift = if garment.asset == GarmentName::HangingBedsheet {
        -0.014
    } else {
        0.
    };
    let measured_drop = source.drop * garment.scale + drop - lift + 0.015;
    if !point(left)
        || !point(right)
        || !sway.is_finite()
        || sway.abs() > 10.
        || !garment.scale.is_finite()
        || !(0.001..=100.).contains(&garment.scale)
        || !garment.width.is_finite()
        || (garment.width - source.width * garment.scale).abs() > 1e-9
        || !garment.drop.is_finite()
        || (garment.drop - measured_drop).abs() > 1e-9
        || (!drawing.mesh.surveying()
            && garment
                .tint
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v)))
    {
        return Err(
            "Mesh garment requires a finite fit, matching source dimensions and bounded support."
                .into(),
        );
    }
    let run = hypot(right[0] - left[0], right[1] - left[1]);
    if run < 1e-6 {
        return Ok(());
    }
    let along = [(right[0] - left[0]) / run, (right[1] - left[1]) / run, 0.];
    let outward = [-along[1], along[0], 0.];
    let center = std::array::from_fn(|i| (left[i] + right[i]) / 2.);
    let mut frame = Fixture {
        pen: drawing.pen.facing(center, along, outward)?,
        mesh: drawing.mesh,
        repeats: drawing.repeats,
    };
    let slope = if hanger {
        0.
    } else {
        (right[2] - left[2]) / run
    };
    let shear = sway / garment.drop.max(0.01);
    let place = |p: V3| {
        [
            p[0] * garment.scale,
            (p[1] - shear * p[2]) * garment.scale,
            (p[2] + slope * p[0]) * garment.scale - drop + lift,
        ]
    };
    for face in &source.faces {
        let points: Vec<_> = face.iter().map(|v| place([v[0], v[1], v[2]])).collect();
        let normals: Vec<_> = face
            .iter()
            .map(|v| {
                let up = v[5] + shear * v[4];
                let n = [v[3] - slope * up, v[4], up];
                let size = length(n);
                n.map(|v| v / size)
            })
            .collect();
        emit(
            &mut frame,
            "balcony-cloth",
            face,
            &points,
            &normals,
            garment.tint,
            textured,
        )?;
    }
    let anchors = source
        .anchors
        .ok_or("Garment source lacks suspension anchors.")?
        .map(place);
    if hanger {
        let neck = [0., 0., -0.045];
        let tint = [0.38, 0.40, 0.38];
        frame.tube(
            "paint",
            &[anchors[0], neck, anchors[1], anchors[0]],
            0.003,
            tint,
            5,
        )?;
        let mut hook = vec![neck, [0., 0., -0.018]];
        for i in 0..=8 {
            let angle = -std::f64::consts::PI / 2. + i as f64 / 8. * std::f64::consts::PI * 1.7;
            hook.push([0.018 * cos(angle), 0., 0.018 * sin(angle)]);
        }
        frame.tube("paint", &hook, 0.003, tint, 5)?;
    } else {
        for at in anchors {
            frame.bar(
                "paint",
                [at[0], at[1], at[2] - 0.025],
                [at[0], at[1], at[2] + 0.025],
                0.012,
                [0.48, 0.34, 0.20],
            )?;
        }
    }
    Ok(())
}
