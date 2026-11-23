//! Smooth shop goods with physical-scale charts and bounded, cancellable emission.
use super::super::{
    fixture::Fixture,
    pen::{Pen, Tint},
};
use std::f64::consts::PI;
use superspec_geometry::{
    Matrix, V3,
    math::{add, atan2, cos, cross, dot, hypot, length, scale, sin, sub, unit},
};

pub enum Tints<'a> {
    Uniform(&'a Tint<'a>),
    Bands(&'a [Tint<'a>]),
}
impl Tints<'_> {
    fn band(&self, index: usize) -> Result<&Tint<'_>, String> {
        match self {
            Self::Uniform(tint) => Ok(tint),
            Self::Bands(bands) => bands
                .get(index.min(bands.len().saturating_sub(1)))
                .ok_or_else(|| "A turned profile needs at least one band tint.".into()),
        }
    }
}
fn point(p: V3) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 1000.)
}
fn sections(sides: usize) -> Result<(), String> {
    if !(3..=128).contains(&sides) {
        Err("Shop forms require 3–128 radial sides.".into())
    } else {
        Ok(())
    }
}
fn blend(own: [f64; 2], other: Option<&[f64; 2]>) -> [f64; 2] {
    let Some(other) = other else {
        return own;
    };
    if own[0] * other[0] + own[1] * other[1] < 0.5 {
        return own;
    }
    let sum = [own[0] + other[0], own[1] + other[1]];
    let n = hypot(sum[0], sum[1]);
    [sum[0] / n, sum[1] / n]
}
fn chart(
    start: [f64; 2],
    end: [f64; 2],
    sides: usize,
    distance: f64,
    repeat: f64,
    ring: usize,
    step: usize,
) -> [f64; 2] {
    let dr = end[0] - start[0];
    let dz = end[1] - start[1];
    let radius = if ring == 0 { start[0] } else { end[0] };
    let run = hypot(dr, dz);
    let chord = 2. * sin(PI / sides as f64);
    if dz.abs() < 1e-9 {
        let angle = step as f64 / sides as f64 * PI * 2.;
        return [cos(angle) * radius * repeat, sin(angle) * radius * repeat];
    }
    if dr.abs() < 1e-12 {
        return [
            step as f64 * chord * radius * repeat,
            (distance + ring as f64 * run) * repeat,
        ];
    }
    let slope = dr.abs() / run;
    let sine = (slope * chord / 2.).min(1.);
    let angle = step as f64 * 2. * atan2(sine, (1. - sine * sine).sqrt());
    [
        radius * sin(angle) / slope * repeat,
        (distance + ring as f64 * run
            - dr.signum() * radius * 2. * sin(angle / 2.).powi(2) / slope)
            * repeat,
    ]
}
trait Banded {
    fn band(&self, index: usize) -> Result<&Tint<'_>, String>;
}
impl Banded for Tints<'_> {
    fn band(&self, index: usize) -> Result<&Tint<'_>, String> {
        Tints::band(self, index)
    }
}
/// The tints of a profile reduced to its `kept` points.
struct Kept<'a, 'b> {
    tints: &'a Tints<'b>,
    kept: &'a [usize],
}
impl Banded for Kept<'_, '_> {
    fn band(&self, index: usize) -> Result<&Tint<'_>, String> {
        self.tints
            .band(self.kept[index.min(self.kept.len().saturating_sub(1))])
    }
}
/// The profile points that stand more than `tolerance` off the outline through the rest.
fn outline_points(profile: &[[f64; 2]], tolerance: f64) -> Vec<usize> {
    let last = profile.len() - 1;
    let mut keep = vec![false; profile.len()];
    keep[0] = true;
    keep[last] = true;
    let mut spans = vec![(0, last)];
    while let Some((a, b)) = spans.pop() {
        if b <= a + 1 {
            continue;
        }
        let (p, q) = (profile[a], profile[b]);
        let along = [q[0] - p[0], q[1] - p[1]];
        let run = along[0] * along[0] + along[1] * along[1];
        let mut worst = (0., a);
        for (i, v) in profile.iter().enumerate().take(b).skip(a + 1) {
            let offset = [v[0] - p[0], v[1] - p[1]];
            let t = if run < 1e-18 {
                0.
            } else {
                ((offset[0] * along[0] + offset[1] * along[1]) / run).clamp(0., 1.)
            };
            let distance = hypot(offset[0] - t * along[0], offset[1] - t * along[1]);
            if distance > worst.0 {
                worst = (distance, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            spans.push((a, worst.1));
            spans.push((worst.1, b));
        }
    }
    (0..profile.len()).filter(|i| keep[*i]).collect()
}
/// A vertical turned profile. Returning down the inside creates an open vessel.
pub fn lathe(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    base: V3,
    profile: &[[f64; 2]],
    tints: &Tints<'_>,
    sides: usize,
    texture_repeat: Option<f64>,
) -> Result<(), String> {
    sections(sides)?;
    if !point(base)
        || !(2..=256).contains(&profile.len())
        || profile.iter().any(|p| {
            !p[0].is_finite()
                || !(0. ..=100.).contains(&p[0])
                || !p[1].is_finite()
                || p[1].abs() > 100.
        })
        || texture_repeat.is_some_and(|r| !r.is_finite() || !(0.0001..=10000.).contains(&r))
    {
        return Err(
            "Turned shop profiles require bounded radii, heights and positive texture repeat."
                .into(),
        );
    }
    tints.band(0)?;
    // A reduced level turns the same profile with fewer sides, by its widest radius.
    let widest = profile.iter().map(|p| p[0]).fold(0., f64::max);
    let sides = d.mesh.graphics_detail().sides(widest, sides);
    // Shelf goods (tins, jars, mugs) stand by the hundred: five sides each when reduced.
    let sides = if d.mesh.graphics_detail().reduced() && widest < 0.07 {
        sides.min(5)
    } else {
        sides
    }
    .max(3);
    d.mesh.check_work(profile.len() * sides * 2)?;
    // ...and with only the profile points that shape its outline; a merged run of segments
    // takes its first one's tint.
    let detail = d.mesh.graphics_detail();
    let outline: Vec<[f64; 2]>;
    let kept: Vec<usize>;
    let banded;
    let (profile, tints) = if detail.reduced() && profile.len() > 2 {
        let size = profile
            .iter()
            .flat_map(|p| [p[0], p[1].abs()])
            .fold(0., f64::max)
            .max(1e-9);
        kept = outline_points(profile, size * detail.pick(0., 0.07, 0.12));
        outline = kept.iter().map(|i| profile[*i]).collect();
        banded = Kept { tints, kept: &kept };
        (&outline[..], &banded as &dyn Banded)
    } else {
        (profile, tints as &dyn Banded)
    };
    let slopes: Vec<_> = profile
        .windows(2)
        .map(|p| {
            let run = hypot(p[1][0] - p[0][0], p[1][1] - p[0][1]);
            if run < 1e-9 {
                [0.; 2]
            } else {
                [(p[1][1] - p[0][1]) / run, (p[0][0] - p[1][0]) / run]
            }
        })
        .collect();
    let mut distance = 0.;
    for (i, p) in profile.windows(2).enumerate() {
        let own = slopes[i];
        if own == [0.; 2] {
            continue;
        }
        let low = blend(own, i.checked_sub(1).and_then(|j| slopes.get(j)));
        let high = blend(own, slopes.get(i + 1));
        let tint = tints.band(i)?;
        for k in 0..sides {
            let a = k as f64 / sides as f64 * PI * 2.;
            let b = (k + 1) as f64 / sides as f64 * PI * 2.;
            let (ca, sa, cb, sb) = (cos(a), sin(a), cos(b), sin(b));
            let points = [
                [
                    base[0] + ca * p[0][0],
                    base[1] + sa * p[0][0],
                    base[2] + p[0][1],
                ],
                [
                    base[0] + cb * p[0][0],
                    base[1] + sb * p[0][0],
                    base[2] + p[0][1],
                ],
                [
                    base[0] + cb * p[1][0],
                    base[1] + sb * p[1][0],
                    base[2] + p[1][1],
                ],
                [
                    base[0] + ca * p[1][0],
                    base[1] + sa * p[1][0],
                    base[2] + p[1][1],
                ],
            ];
            let uv = texture_repeat.map(|r| {
                [(0, k), (0, k + 1), (1, k + 1), (1, k)]
                    .map(|(ring, step)| chart(p[0], p[1], sides, distance, r, ring, step))
            });
            let normals = [
                [ca * low[0], sa * low[0], low[1]],
                [cb * low[0], sb * low[0], low[1]],
                [cb * high[0], sb * high[0], high[1]],
                [ca * high[0], sa * high[0], high[1]],
            ];
            d.pen.face(
                d.mesh,
                slot,
                &points,
                [(ca + cb) * own[0], (sa + sb) * own[0], 2. * own[1]],
                tint,
                d.repeat(slot),
                uv.as_ref().map(|v| v.as_slice()),
                Some(&normals),
            )?;
        }
        distance += hypot(p[1][0] - p[0][0], p[1][1] - p[0][1]);
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Corner {
    point: V3,
    normal: V3,
}
fn smooth_face(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    corners: [Corner; 4],
    tint: &Tint<'_>,
) -> Result<(), String> {
    let hint = corners.iter().fold([0.; 3], |sum, c| add(sum, c.normal));
    d.pen.face(
        d.mesh,
        slot,
        &corners.map(|c| c.point),
        hint,
        tint,
        d.repeat(slot),
        None,
        Some(&corners.map(|c| c.normal)),
    )
}
struct Station {
    center: V3,
    radius: f64,
    tangent: V3,
    side: V3,
    other: V3,
    taper: f64,
}
impl Station {
    fn ring(&self, angle: f64, flatten: f64) -> Result<Corner, String> {
        let c = cos(angle);
        let n = sin(angle);
        let point = add(
            self.center,
            add(
                scale(self.side, c * self.radius),
                scale(self.other, n * self.radius * flatten),
            ),
        );
        let radial = add(scale(self.side, c), scale(self.other, n / flatten));
        Ok(Corner {
            point,
            normal: unit(sub(radial, scale(self.tangent, self.taper)))?,
        })
    }
}
/// Swept round goods use transported section axes; zero-radius endpoints close to a point.
pub fn limb(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    path: &[V3],
    radii: &[f64],
    tint: &Tint<'_>,
    sides: usize,
    flatten: f64,
) -> Result<(), String> {
    sections(sides)?;
    if !(2..=256).contains(&path.len())
        || path.len() != radii.len()
        || path.iter().any(|p| !point(*p))
        || radii
            .iter()
            .any(|r| !r.is_finite() || !(0. ..=100.).contains(r))
        || !flatten.is_finite()
        || !(0.001..=100.).contains(&flatten)
    {
        return Err("Swept shop goods require finite paths, matching bounded radii and positive flattening.".into());
    }
    // A reduced level sweeps fewer sides and draws a hairline limb 25 mm across.
    let detail = d.mesh.graphics_detail();
    let widest = radii.iter().copied().fold(0., f64::max);
    let sides = detail.sides(widest, sides).max(3);
    let widened: Vec<f64>;
    let radii = if detail.reduced() && widest > 0. && detail.readable(widest * 2.) > widest * 2. {
        let grow = detail.readable(widest * 2.) / (widest * 2.);
        widened = radii.iter().map(|r| r * grow).collect();
        &widened[..]
    } else {
        radii
    };
    d.mesh.check_work(path.len() * sides * 2)?;
    let last = path.len() - 1;
    let mut stations: Vec<Station> = Vec::with_capacity(path.len());
    for i in 0..=last {
        let before = i.saturating_sub(1);
        let after = last.min(i + 1);
        let chord = sub(path[after], path[before]);
        let size = length(chord);
        let tangent = unit(chord)?;
        let previous = stations.last();
        let mut seed = previous.map_or(
            if tangent[2].abs() > 0.9 {
                [1., 0., 0.]
            } else {
                [0., 0., 1.]
            },
            |s| s.side,
        );
        if let Some(prev) = previous
            && dot(seed, tangent).abs() > 0.95
        {
            seed = prev.other;
        }
        let side = unit(sub(seed, scale(tangent, dot(seed, tangent))))?;
        stations.push(Station {
            center: path[i],
            radius: radii[i],
            tangent,
            side,
            other: cross(tangent, side),
            taper: (radii[after] - radii[before]) / size,
        });
    }
    for pair in stations.windows(2) {
        for k in 0..sides {
            let a = k as f64 / sides as f64 * PI * 2.;
            let b = (k + 1) as f64 / sides as f64 * PI * 2.;
            smooth_face(
                d,
                slot,
                [
                    pair[0].ring(a, flatten)?,
                    pair[1].ring(a, flatten)?,
                    pair[1].ring(b, flatten)?,
                    pair[0].ring(b, flatten)?,
                ],
                tint,
            )?;
        }
    }
    Ok(())
}
/// Ellipsoid shading follows its scaled surface, including pole normals.
#[derive(Clone, serde::Deserialize)]
pub struct Ball {
    pub center: V3,
    pub radius: f64,
    pub stretch: V3,
    pub sides: usize,
    pub bands: usize,
}
pub fn ball(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    shape: &Ball,
    tint: &Tint<'_>,
) -> Result<(), String> {
    let Ball {
        center,
        radius,
        stretch,
        sides,
        bands,
    } = *shape;
    sections(sides)?;
    if !point(center)
        || !radius.is_finite()
        || !(0.0001..=100.).contains(&radius)
        || stretch
            .iter()
            .any(|s| !s.is_finite() || !(0.001..=100.).contains(s))
        || !(2..=128).contains(&bands)
    {
        return Err(
            "Shop ellipsoids require bounded positive dimensions and 2–128 latitude bands.".into(),
        );
    }
    let detail = d.mesh.graphics_detail();
    let sides = detail.sides(radius, sides).max(3);
    // Fruit-sized balls become bipyramids: a heap reads by its colours, not its rounds.
    let bands = detail.pick(bands, bands.min(if radius < 0.06 { 2 } else { 3 }), 2);
    d.mesh.check_work(sides * bands * 2)?;
    let at = |latitude, longitude| -> Result<Corner, String> {
        let level = cos(latitude);
        let v = [
            level * cos(longitude),
            level * sin(longitude),
            sin(latitude),
        ];
        Ok(Corner {
            point: std::array::from_fn(|i| center[i] + v[i] * radius * stretch[i]),
            normal: unit(std::array::from_fn(|i| v[i] / stretch[i]))?,
        })
    };
    for j in 0..bands {
        for k in 0..sides {
            let south = -PI / 2. + j as f64 / bands as f64 * PI;
            let north = -PI / 2. + (j + 1) as f64 / bands as f64 * PI;
            let west = k as f64 / sides as f64 * PI * 2.;
            let east = (k + 1) as f64 / sides as f64 * PI * 2.;
            smooth_face(
                d,
                slot,
                [
                    at(south, west)?,
                    at(south, east)?,
                    at(north, east)?,
                    at(north, west)?,
                ],
                tint,
            )?;
        }
    }
    Ok(())
}
pub fn front(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    span: [f64; 4],
    y: f64,
    tint: V3,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = span;
    d.face(
        slot,
        &[[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
        [0., -1., 0.],
        tint,
    )
}
/// New shapes reset scoped finishes but retain the original room/shelf shading frames.
pub fn rotated(pen: &Pen, origin: V3, axis: usize, angle: f64) -> Result<Pen, String> {
    if axis > 2 || !angle.is_finite() || !point(origin) {
        return Err("Shop rotation requires a finite local origin, angle and axis.".into());
    }
    let c = cos(angle);
    let s = sin(angle);
    let basis = match axis {
        0 => [[1., 0., 0.], [0., c, s], [0., -s, c]],
        1 => [[c, 0., -s], [0., 1., 0.], [s, 0., c]],
        _ => [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]],
    };
    pen.shape_frame(
        pen.matrix
            .then(Matrix([basis[0], basis[1], basis[2], origin])),
        &pen.part,
    )
}

#[cfg(test)]
mod rotation_tests {
    use super::super::super::{
        mesh::{Mesh, Mode},
        timber,
    };
    use super::super::{
        furniture::Shelf,
        grocer, grocer_layout, patina,
        room::{Assets, Room},
        snack_string,
    };
    use super::*;
    use superspec_types::rng::SeededRng;

    fn draw(pen: &Pen, slot: &str, mode: Mode) -> Mesh<'static> {
        let mut mesh = Mesh::new(mode, 1000, &|| false).unwrap();
        pen.face(
            &mut mesh,
            slot,
            &[[0., 0., 0.], [0.1, 0., 0.], [0., 0.1, 0.]],
            [0., 0., 1.],
            &Tint::Color([1.; 3]),
            1.,
            None,
            None,
        )
        .unwrap();
        mesh
    }

    #[test]
    fn rotation_keeps_room_and_stock_shading_in_their_original_frames() {
        let base = Pen::new(Matrix::default(), "stock").unwrap();
        let stock = base.with_shelf_patina(patina::Shelf::Stock {
            inverse: Matrix::default(),
            shelf: Shelf {
                x0: 0.,
                x1: 1.,
                y: 0.,
                depth: 1.,
                z: 0.,
                clear: 1.,
            },
        });
        let room = base
            .with_room_patina_frame(&base, [2., 2., 2.], 71)
            .unwrap();
        for parent in [stock, room] {
            let turn = rotated(&parent, [0.3, 0.4, 0.], 2, PI / 2.).unwrap();
            let facing = parent
                .facing([0.3, 0.4, 0.], [0., 1., 0.], [-1., 0., 0.])
                .unwrap();
            let actual = draw(&turn, "paint", Mode::Render);
            let expected = draw(&facing, "paint", Mode::Render);
            assert_eq!(actual.indices, expected.indices);
            for (a, b) in actual.vertices.iter().zip(&expected.vertices) {
                assert_eq!(a.position, b.position);
                assert_eq!(a.normal, b.normal);
                assert_eq!(a.color, b.color);
                assert!(a.color.iter().all(|c| *c < 0.8));
            }
            let bare = Pen::new(turn.matrix, "stock").unwrap();
            assert!(
                draw(&bare, "paint", Mode::Render)
                    .vertices
                    .iter()
                    .all(|v| v.color == [1.; 3])
            );
            assert_eq!(
                draw(&turn, "paint", Mode::Survey).into_survey(),
                draw(&bare, "paint", Mode::Survey).into_survey()
            );
        }
    }

    #[test]
    fn rotation_resets_scoped_materials_and_preserves_host_handedness() {
        let mut base = Pen::new(Matrix::default(), "fixture").unwrap();
        base.mirrored = true;
        for (parent, input, output) in [
            (base.with_brushed_metal(1.7).unwrap(), "metal", "metal"),
            (
                base.with_timber(timber::Settings::default()).unwrap(),
                "timber",
                "timber",
            ),
        ] {
            let turn = rotated(&parent, [0.1, 0.2, 0.3], 1, 0.37).unwrap();
            assert!(turn.mirrored);
            let mesh = draw(&turn, input, Mode::Render);
            assert!(!mesh.surfaces.is_empty());
            assert!(mesh.surfaces.iter().all(|s| s.slot == output));
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.uv.iter().all(|u| u.is_finite()))
            );
        }
    }

    fn same_geometry_with_shaded_stock(bare: &Mesh<'_>, shaded: &Mesh<'_>, slot: &str) {
        assert_eq!(bare.indices.len(), shaded.indices.len());
        let mut checked = 0;
        for (t, (before, after)) in bare
            .indices
            .chunks_exact(3)
            .zip(shaded.indices.chunks_exact(3))
            .enumerate()
        {
            let before_surface = &bare.surfaces[bare.triangle_surfaces[t]];
            let after_surface = &shaded.surfaces[shaded.triangle_surfaces[t]];
            assert_eq!(before_surface.part, after_surface.part);
            assert_eq!(before_surface.slot, after_surface.slot);
            for (&a, &b) in before.iter().zip(after) {
                let a = &bare.vertices[a as usize];
                let b = &shaded.vertices[b as usize];
                assert_eq!((a.position, a.normal, a.uv), (b.position, b.normal, b.uv));
                if before_surface.slot == slot {
                    for axis in 0..3 {
                        // Room shading has at least 2% attenuation; both colors
                        // are independently rounded to three decimal places:
                        // 0.0005 + 0.98 * 0.0005 bounds the combined error.
                        assert!(b.color[axis] <= a.color[axis] * 0.98 + 0.000991);
                        if a.color[axis] > 0.1 {
                            assert!(b.color[axis] < a.color[axis]);
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(
            checked > 0,
            "expected opaque {slot} stock to receive room shading"
        );
    }

    fn survey_matches(mesh: &Mesh<'_>, survey: &Mesh<'_>) {
        assert_eq!(
            mesh.indices
                .iter()
                .map(|&i| mesh.vertices[i as usize].position)
                .collect::<Vec<_>>(),
            survey.survey
        );
    }

    #[test]
    fn mirrored_grocer_keeps_room_shading_and_stock_geometry() {
        use super::super::super::{kit::Kit, metal::Binding, signs::panel::Settings, weathered};
        let binding = |material: &str| Binding {
            material: material.into(),
            repeat: 1.7,
        };
        let kit = Kit::new(
            Settings {
                weathering: weathered::Settings {
                    seed: "room/3".into(),
                    wear: 0.25,
                    grime: 0.6,
                    rust: 0.4,
                    texture_density: 32.,
                    metal: binding("metal"),
                    sheet: binding("sheet"),
                    paint: binding("paint"),
                },
                lettering: true,
                timber: timber::Settings {
                    textured: true,
                    timber_repeat: 1.7,
                    atlas_repeat: 1.7,
                    ..Default::default()
                },
            },
            1,
        )
        .unwrap();
        let room = Room {
            width: 3.2,
            depth: 2.4,
            height: 2.08,
            kit: &kit,
            key: "shop/3",
            assets: Assets {
                posters: true,
                grocer_props: true,
                snack_packets: true,
                market_apple: true,
                shop_register: false,
                market_trays: false,
            },
        };
        // This authored seed is the mirrored case in the independent layout corpus.
        assert!(
            grocer_layout::plan(
                room.width,
                room.depth,
                room.height,
                &mut room.stream("layout").unwrap()
            )
            .unwrap()
            .mirrored
        );
        let base = Pen::new(
            Matrix([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [3., -2., 1.]]),
            "fixture",
        )
        .unwrap();
        let shaded = base
            .with_room_patina_frame(&base, [room.width, room.depth, room.height], 71)
            .unwrap();
        let build = |pen: &Pen, mode| {
            let mut mesh = Mesh::new(mode, 400000, &|| false).unwrap();
            grocer::furnish(
                &mut Fixture {
                    pen: pen.clone(),
                    mesh: &mut mesh,
                    repeats: &|_| 1.7,
                },
                &room,
            )
            .unwrap();
            mesh
        };
        let bare = build(&base, Mode::Render);
        let shaded_mesh = build(&shaded, Mode::Render);
        same_geometry_with_shaded_stock(&bare, &shaded_mesh, "grocer-props");
        survey_matches(&bare, &build(&shaded, Mode::Survey));
    }

    #[test]
    fn posed_packet_strings_keep_room_shading_rng_and_geometry() {
        for mirrored in [false, true] {
            let mut base = Pen::new(
                Matrix([[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [3., -2., 1.]]),
                "packets",
            )
            .unwrap();
            base.mirrored = mirrored;
            let shaded = base
                .with_room_patina_frame(&base, [2., 2., 2.], 71)
                .unwrap();
            let build = |pen: &Pen, mode| {
                let mut mesh = Mesh::new(mode, 30000, &|| false).unwrap();
                let mut rng = SeededRng::from_u32(90210);
                snack_string::string(
                    &mut Fixture {
                        pen: pen.clone(),
                        mesh: &mut mesh,
                        repeats: &|_| 1.7,
                    },
                    [0.3, 0.4, 1.8],
                    0.8,
                    &mut rng,
                    true,
                )
                .unwrap();
                (mesh, rng.next_f64())
            };
            let (bare, next) = build(&base, Mode::Render);
            let (shaded_mesh, shaded_next) = build(&shaded, Mode::Render);
            assert_eq!(next, shaded_next);
            same_geometry_with_shaded_stock(&bare, &shaded_mesh, "snack-packet");
            let (survey, survey_next) = build(&shaded, Mode::Survey);
            assert_eq!(next, survey_next);
            survey_matches(&bare, &survey);
        }
    }
}
