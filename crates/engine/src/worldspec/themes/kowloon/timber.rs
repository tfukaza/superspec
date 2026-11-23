//! Board-local atlas charts and contact wear. Geometry stays on the original member surfaces.
#![allow(clippy::too_many_arguments)]
use super::{
    graphics_detail::GraphicsDetail,
    mesh::{Mesh, rounded},
    noise::{smooth_step, value_noise},
    palette::srgb,
    pen::{Pen, Tint},
    wall::finish_seed,
};
use serde::Deserialize;
use superspec_geometry::{
    Matrix, V3,
    math::{cross, dot, scale, sub},
};

fn default_slots() -> Vec<String> {
    vec!["timber".into()]
}
fn textured() -> bool {
    true
}
fn one() -> f64 {
    1.
}
#[derive(Clone, Debug, Deserialize)]
pub struct FrontWear {
    pub width: f64,
    pub strength: f64,
    pub direction: Option<V3>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_slots")]
    pub slots: Vec<String>,
    pub grain_variant: Option<usize>,
    pub front_wear: Option<FrontWear>,
    pub face_grain_axis: Option<usize>,
    #[serde(default = "textured")]
    pub textured: bool,
    #[serde(default = "one")]
    pub timber_repeat: f64,
    #[serde(default = "one")]
    pub atlas_repeat: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            slots: default_slots(),
            grain_variant: None,
            front_wear: None,
            face_grain_axis: None,
            textured: true,
            timber_repeat: 1.,
            atlas_repeat: 1.,
        }
    }
}
#[derive(Clone)]
struct Board {
    low: V3,
    high: V3,
    along: usize,
    seed: u32,
}
#[derive(Clone)]
pub(super) struct Timber {
    settings: Settings,
    board: Option<Board>,
}
struct Chart {
    u: usize,
    v: usize,
    sign: f64,
    offset: f64,
    origin: f64,
    variant: usize,
    seed: u32,
    span: f64,
    vary_across: bool,
}
struct Wear {
    along: usize,
    across: usize,
    edges: Vec<f64>,
    width: f64,
    strength: f64,
}

/// Serializes the exact Three column-major matrix and dimensions used by the reference finish seed.
pub fn seed_key(pen: &Pen, dimensions: &[f64]) -> Result<String, String> {
    if pen.part.len() > 4096 || dimensions.len() > 32 || dimensions.iter().any(|v| !v.is_finite()) {
        return Err("Invalid timber seed dimensions.".into());
    }
    let mut key = pen.part.clone();
    let mut buffer = ryu_js::Buffer::new();
    for (column, values) in pen.matrix.0.iter().enumerate() {
        for value in values
            .iter()
            .copied()
            .chain(std::iter::once(if column == 3 { 1. } else { 0. }))
        {
            if !value.is_finite() {
                return Err("Invalid timber seed frame.".into());
            }
            key.push('/');
            key.push_str(buffer.format(value));
        }
    }
    for value in dimensions {
        key.push('/');
        key.push_str(buffer.format(*value));
    }
    Ok(key)
}
fn seed(pen: &Pen, dimensions: &[f64]) -> Result<u32, String> {
    Ok(finish_seed(&seed_key(pen, dimensions)?))
}
fn norm(vector: V3) -> V3 {
    let length = dot(vector, vector).sqrt();
    scale(vector, 1. / if length == 0. { 1. } else { length })
}

/// At most 65,536 repeat cells are materialized before mesh work/cancellation checks take over.
fn intervals(low: f64, high: f64, period: f64) -> Result<Vec<[f64; 3]>, String> {
    if !low.is_finite()
        || !high.is_finite()
        || !period.is_finite()
        || period <= 0.
        || low > high
        || low.abs() > 1e8
        || high.abs() > 1e8
        || (high - low) / period > 65535.
    {
        return Err("Timber repeat interval exceeds its finite work budget.".into());
    }
    let mut result = vec![];
    let mut cell = ((low + 1e-9) / period).floor();
    while cell * period < high - 1e-9 {
        if result.len() >= 65536 {
            return Err("Timber repeat cell budget exceeded.".into());
        }
        result.push([
            low.max(cell * period),
            high.min((cell + 1.) * period),
            cell * period,
        ]);
        cell += 1.;
    }
    Ok(result)
}
fn split_at(low: f64, high: f64, cuts: &[f64]) -> Vec<[f64; 2]> {
    let mut points = vec![low];
    points.extend(
        cuts.iter()
            .copied()
            .filter(|v| *v > low + 1e-8 && *v < high - 1e-8),
    );
    points.push(high);
    points.sort_by(f64::total_cmp);
    points
        .windows(2)
        .filter(|p| p[1] - p[0] > 1e-8)
        .map(|p| [p[0], p[1]])
        .collect()
}
fn clip(mut points: Vec<[f64; 2]>, bounds: [f64; 4]) -> Vec<[f64; 2]> {
    for (axis, edge, sign) in [
        (0, bounds[0], 1.),
        (0, bounds[2], -1.),
        (1, bounds[1], 1.),
        (1, bounds[3], -1.),
    ] {
        let input = points;
        points = vec![];
        for i in 0..input.len() {
            let a = input[i];
            let b = input[(i + 1) % input.len()];
            let da = (a[axis] - edge) * sign;
            let db = (b[axis] - edge) * sign;
            if da >= 0. {
                points.push(a);
            }
            if (da >= 0.) != (db >= 0.) {
                points.push(std::array::from_fn(|i| {
                    a[i] + da / (da - db) * (b[i] - a[i])
                }));
            }
        }
    }
    points
}
impl Timber {
    pub(super) fn new(settings: Settings) -> Result<Self, String> {
        if settings.slots.len() > 64
            || [settings.timber_repeat, settings.atlas_repeat]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.)
            || settings.slots.iter().map(String::len).sum::<usize>() > 16384
            || settings
                .slots
                .iter()
                .any(|s| s.is_empty() || s.len() > 4096)
            || settings.grain_variant.is_some_and(|v| v > 3)
            || settings.face_grain_axis.is_some_and(|v| v > 2)
            || settings.front_wear.as_ref().is_some_and(|w| {
                !w.width.is_finite()
                    || !(0. ..=100.).contains(&w.width)
                    || !w.strength.is_finite()
                    || !(0. ..=1.2).contains(&w.strength)
                    || w.direction.is_some_and(|d| {
                        d.iter().any(|v| !v.is_finite()) || (dot(d, d).sqrt() - 1.).abs() > 1e-5
                    })
            })
        {
            return Err("Invalid timber finish or identity budget.".into());
        }
        Ok(Self {
            settings,
            board: None,
        })
    }
    pub(super) fn derived(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            board: None,
        }
    }
    pub(super) fn for_box(&self, pen: &Pen, low: V3, high: V3) -> Result<Self, String> {
        let mut along = 0;
        for i in 1..3 {
            if high[i] - low[i] > high[along] - low[along] {
                along = i;
            }
        }
        let mut value = self.clone();
        value.board = Some(Board {
            low,
            high,
            along,
            seed: seed(pen, &[low, high].concat())?,
        });
        Ok(value)
    }
    fn chart(
        &self,
        pen: &Pen,
        minimum: V3,
        extent: V3,
        axes: &[usize],
        hint: V3,
    ) -> Result<Chart, String> {
        let Some(board) = &self.board else {
            let preferred = self.settings.face_grain_axis.unwrap_or(2);
            let along = if axes.contains(&preferred) {
                preferred
            } else {
                *axes
                    .iter()
                    .reduce(|a, b| if extent[*a] >= extent[*b] { a } else { b })
                    .unwrap()
            };
            let across = *axes.iter().find(|i| **i != along).unwrap();
            let seed = seed(pen, &[minimum, extent].concat())?;
            return Ok(Chart {
                u: across,
                v: along,
                sign: 1.,
                offset: -minimum[across],
                origin: minimum[along],
                variant: self.settings.grain_variant.unwrap_or(seed as usize % 4),
                seed,
                span: extent[along],
                vary_across: self.settings.grain_variant.is_none(),
            });
        };
        let cross: Vec<_> = (0..3).filter(|i| *i != board.along).collect();
        let (a, b) = (cross[0], cross[1]);
        if !axes.contains(&board.along) {
            return Ok(Chart {
                u: a,
                v: b,
                sign: 1.,
                offset: -board.low[a],
                origin: board.low[b],
                variant: 4 + self
                    .settings
                    .grain_variant
                    .unwrap_or(board.seed as usize % 4),
                seed: board.seed,
                span: board.high[b] - board.low[b],
                vary_across: false,
            });
        }
        let width = board.high[a] - board.low[a];
        let depth = board.high[b] - board.low[b];
        let (u, sign, offset) = if hint[b] < -0.9 {
            (a, 1., -board.low[a])
        } else if hint[a] > 0.9 {
            (b, 1., width - board.low[b])
        } else if hint[b] > 0.9 {
            (a, -1., width + depth + board.high[a])
        } else {
            (b, -1., 2. * width + depth + board.high[b])
        };
        Ok(Chart {
            u,
            v: board.along,
            sign,
            offset,
            origin: board.low[board.along],
            variant: self
                .settings
                .grain_variant
                .unwrap_or(board.seed as usize % 4),
            seed: board.seed,
            span: board.high[board.along] - board.low[board.along],
            vary_across: false,
        })
    }
    fn wear(&self, hint: V3) -> Option<Wear> {
        let board = self.board.as_ref()?;
        let size: V3 = std::array::from_fn(|i| board.high[i] - board.low[i]);
        if let Some(wear) = &self.settings.front_wear {
            let front = wear.direction.unwrap_or([0., -1., 0.]);
            let mut axis = 0;
            for i in 1..3 {
                if front[i].abs() > front[axis].abs() {
                    axis = i;
                }
            }
            if board.along == axis || front[axis].abs() < 0.99 {
                return None;
            }
            let across = (0..3).find(|i| *i != board.along && *i != axis).unwrap();
            let width = wear.width.min(size[across] * 0.24).min(size[axis] * 0.4);
            if hint[axis] * front[axis] > 0.9 {
                return Some(Wear {
                    along: board.along,
                    across,
                    edges: vec![board.low[across], board.high[across]],
                    width,
                    strength: wear.strength,
                });
            }
            if hint[across].abs() > 0.9 {
                return Some(Wear {
                    along: board.along,
                    across: axis,
                    edges: vec![if front[axis] < 0. {
                        board.low[axis]
                    } else {
                        board.high[axis]
                    }],
                    width,
                    strength: wear.strength,
                });
            }
            return None;
        }
        if board.along == 0 && size[0] >= 0.15 && size[2] <= 0.06 {
            if hint[2] > 0.9 {
                return Some(Wear {
                    along: 0,
                    across: 1,
                    edges: vec![board.low[1]],
                    width: 0.008,
                    strength: 1.,
                });
            }
            if hint[1] < -0.9 {
                return Some(Wear {
                    along: 0,
                    across: 2,
                    edges: vec![board.high[2]],
                    width: 0.008,
                    strength: 1.,
                });
            }
        }
        if board.along == 2 && size[2] >= 0.3 && size[0] <= 0.06 {
            if hint[1] < -0.9 {
                return Some(Wear {
                    along: 2,
                    across: 0,
                    edges: vec![board.low[0], board.high[0]],
                    width: 0.003,
                    strength: 0.45,
                });
            }
            if hint[0].abs() > 0.9 {
                return Some(Wear {
                    along: 2,
                    across: 1,
                    edges: vec![board.low[1]],
                    width: 0.003,
                    strength: 0.45,
                });
            }
        }
        None
    }
    pub(super) fn face(
        &self,
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        hint: V3,
        tint: &Tint<'_>,
        repeat: f64,
        uv: Option<&[[f64; 2]]>,
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        if points.len() < 3
            || points.len() > 4096
            || !repeat.is_finite()
            || matches!(tint,Tint::Color(color) if color.iter().any(|v|!v.is_finite()))
            || points.iter().flatten().chain(&hint).any(|v| !v.is_finite())
        {
            return Err("Invalid timber face.".into());
        }
        mesh.check_work(0)?;
        let wood = self.settings.slots.iter().any(|s| s == slot);
        if wood && !self.settings.textured {
            return pen.face_raw(
                mesh,
                "shop-timber",
                points,
                hint,
                tint,
                self.settings.atlas_repeat,
                uv,
                normals,
            );
        }
        let minimum: V3 =
            std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min));
        let extent: V3 = std::array::from_fn(|i| {
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max)
                - minimum[i]
        });
        let axes: Vec<_> = (0..3).filter(|i| extent[*i] > 1e-8).collect();
        if !wood
            || matches!(tint, Tint::Field(_))
            || axes.len() != 2
            || uv.is_some()
            || normals.is_some()
        {
            return pen.face_raw(
                mesh,
                if wood { "timber" } else { slot },
                points,
                hint,
                tint,
                if wood {
                    self.settings.timber_repeat
                } else {
                    repeat
                },
                uv,
                normals,
            );
        }
        let chart = self.chart(pen, minimum, extent, &axes, norm(hint))?;
        // Worn edge strips are a few millimetres wide and split along the board. Very Low
        // draws the board whole. Low keeps one strip per worn edge so its lighter tint still
        // reads: widened to a readable width (the strips of a narrow board meet in its
        // middle) and left whole along the board.
        let detail = mesh.graphics_detail();
        let wear = match detail {
            GraphicsDetail::Full => self.wear(norm(hint)),
            GraphicsDetail::Low => self.wear(norm(hint)).map(|mut wear| {
                let board = self.board.as_ref().expect("a worn face has a board");
                let room =
                    (board.high[wear.across] - board.low[wear.across]) / wear.edges.len() as f64;
                wear.width = detail.readable(wear.width).min(room).max(wear.width);
                wear
            }),
            GraphicsDetail::VeryLow => None,
        };
        let cuts: Vec<_> = wear
            .as_ref()
            .map(|w| {
                w.edges
                    .iter()
                    .flat_map(|edge| [edge - w.width, edge + w.width])
                    .map(|p| p * chart.sign + chart.offset)
                    .collect()
            })
            .unwrap_or_default();
        let field_width = (256. - 8. - 1.) / 494.;
        let u_ends = [minimum[chart.u], minimum[chart.u] + extent[chart.u]]
            .map(|v| v * chart.sign + chart.offset);
        let v0 = minimum[chart.v] - chart.origin;
        let Tint::Color(tint) = tint else {
            unreachable!()
        };
        let reference = srgb("#7a5a3c");
        let brightness =
            ((tint[0] / reference[0] + tint[1] / reference[1] + tint[2] / reference[2]) / 3.)
                .clamp(0.45, 1.4);
        for [u0, u1, u_origin] in intervals(
            u_ends[0].min(u_ends[1]),
            u_ends[0].max(u_ends[1]),
            field_width,
        )? {
            mesh.spend_work(1)?;
            let band = rounded(u_origin / field_width, 1.);
            let variant = if chart.vary_across {
                (chart.variant as i64 + band as i64).rem_euclid(4) as usize
            } else {
                chart.variant
            };
            let region_height = if variant < 4 { 1536. } else { 256. };
            let region_x = (variant % 4) as f64 * 256.;
            let region_y = if variant < 4 { 0. } else { 1536. };
            let field_length = (region_height - 8. - 1.) / 494.;
            let crop_seed = if chart.vary_across {
                finish_seed(&format!(
                    "{}/{}",
                    chart.seed,
                    ryu_js::Buffer::new().format(band)
                ))
            } else {
                chart.seed
            };
            let phase = (crop_seed % 10001) as f64 / 10001. * (field_length - chart.span).max(0.);
            for [v_low, v_high, v_origin] in
                intervals(v0 + phase, v0 + phase + extent[chart.v], field_length)?
            {
                for [u_low, u_high] in split_at(u0, u1, &cuts) {
                    let across = ((u_low + u_high) / 2. - chart.offset) / chart.sign;
                    let worn = wear
                        .as_ref()
                        .is_some_and(|w| w.edges.iter().any(|e| (across - e).abs() < w.width));
                    let along = if worn && detail == GraphicsDetail::Full {
                        intervals(v_low - phase, v_high - phase, 0.22)?
                            .into_iter()
                            .map(|[a, b, _]| [a + phase, b + phase])
                            .collect()
                    } else {
                        vec![[v_low, v_high]]
                    };
                    for [low, high] in along {
                        // Four clip boundaries scan the polygon even when the cell emits nothing.
                        // Charge before collecting, so empty cells cannot evade the work limit.
                        mesh.spend_work(points.len() * 4 + 16)?;
                        let corners = clip(
                            points
                                .iter()
                                .map(|p| {
                                    [
                                        p[chart.u] * chart.sign + chart.offset,
                                        p[chart.v] - chart.origin + phase,
                                    ]
                                })
                                .collect(),
                            [u_low, low, u_high, high],
                        );
                        if corners.len() < 3 {
                            continue;
                        }
                        let positions: Vec<_> = corners
                            .iter()
                            .map(|[u, v]| {
                                let mut p = minimum;
                                p[chart.u] = (u - chart.offset) / chart.sign;
                                p[chart.v] = v - phase + chart.origin;
                                p
                            })
                            .collect();
                        let coordinates: Vec<_> = corners
                            .iter()
                            .map(|[u, v]| {
                                [
                                    (region_x + 4. + 0.5 + (u - u_origin) * 494.) / 1024.,
                                    1. - (region_y + region_height
                                        - 4.
                                        - 0.5
                                        - (v - v_origin) * 494.)
                                        / 1792.,
                                ]
                            })
                            .collect();
                        let worn_tint = |p: V3| -> Result<V3, String> {
                            let w = wear.as_ref().unwrap();
                            let board = self.board.as_ref().unwrap();
                            let distance = w
                                .edges
                                .iter()
                                .map(|edge| (p[w.across] - edge).abs())
                                .fold(f64::INFINITY, f64::min);
                            let along = p[w.along] - board.low[w.along];
                            // The wear comes and goes along the board. A strip left whole has
                            // only its ends to sample that at, so it lays the mean cover.
                            let coverage = if detail == GraphicsDetail::Full {
                                smooth_step(
                                    0.25,
                                    0.7,
                                    value_noise(along * 7., 0., board.seed as f64),
                                )
                            } else {
                                0.5
                            };
                            let amount =
                                w.strength * (1. - smooth_step(0., w.width, distance)) * coverage;
                            Ok([1., 1.1, 1.18].map(|strength| {
                                (brightness * (1. + amount * strength) / 2.).min(1.)
                            }))
                        };
                        let color = if worn && self.board.is_some() {
                            Tint::Field(&worn_tint)
                        } else {
                            Tint::Color([brightness / 2.; 3])
                        };
                        pen.face_raw(
                            mesh,
                            "shop-timber",
                            &positions,
                            hint,
                            &color,
                            repeat,
                            Some(&coordinates),
                            None,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn beveled_bar(
        &self,
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        side: V3,
        width: f64,
        depth: f64,
        bevel: f64,
        tint: &Tint<'_>,
        repeat: f64,
    ) -> Result<(), String> {
        let delta = sub(b, a);
        let length = dot(delta, delta).sqrt();
        if a.iter()
            .chain(&b)
            .chain(&side)
            .chain([&width, &depth, &bevel])
            .any(|v| !v.is_finite())
            || length < 1e-6
            || width <= 0.
            || depth <= 0.
            || bevel < 0.
            || (dot(side, side).sqrt() - 1.).abs() > 1e-5
            || (dot(side, delta) / length).abs() > 1e-5
        {
            return pen.beveled_bar_raw(mesh, slot, a, b, side, width, depth, bevel, tint, repeat);
        }
        let axis = scale(delta, 1. / length);
        let other = norm(cross(axis, side));
        let frame = Matrix([side, other, axis, a]);
        let mut style = self.derived();
        style.settings.face_grain_axis = Some(2);
        if let Some(w) = &mut style.settings.front_wear {
            w.direction = Some(norm(
                frame
                    .inverse()?
                    .direction(w.direction.unwrap_or([0., -1., 0.])),
            ));
        }
        style.board = Some(Board {
            low: [-width / 2., -depth / 2., 0.],
            high: [width / 2., depth / 2., length],
            along: 2,
            seed: seed(pen, &[a.as_slice(), b.as_slice(), &[width, depth]].concat())?,
        });
        let mut local = pen.clone();
        local.matrix = pen.matrix.then(frame);
        local.timber = Some(style);
        let mapped = |p| tint.at(frame.point(p));
        let local_tint = match tint {
            Tint::Color(color) => Tint::Color(*color),
            Tint::Field(_) => Tint::Field(&mapped),
        };
        local.beveled_bar_raw(
            mesh,
            slot,
            [0.; 3],
            [0., 0., length],
            [1., 0., 0.],
            width,
            depth,
            bevel,
            &local_tint,
            repeat,
        )
    }
}
