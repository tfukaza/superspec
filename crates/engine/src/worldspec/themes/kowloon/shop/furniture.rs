//! Shop support surfaces and their timber/steel/tile furniture.
use super::super::{
    fixture::Fixture,
    palette::{shade, srgb},
    pen::Pen,
    timber,
};
use super::forms::front;
use serde::{Deserialize, Serialize};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Shelf {
    pub x0: f64,
    pub x1: f64,
    pub y: f64,
    pub depth: f64,
    pub z: f64,
    pub clear: f64,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    Timber,
    TimberOpen,
    Steel,
    Wire,
}

pub fn shop_shelving(
    d: &mut Fixture<'_, '_>,
    family: Family,
    size: V3,
    levels: usize,
    tint: V3,
    above: f64,
    kit: &super::super::kit::Kit,
) -> Result<Vec<Shelf>, String> {
    if matches!(family, Family::Timber) {
        return shelving(
            d,
            size,
            levels,
            "timber",
            tint,
            above,
            &kit.settings().timber,
        );
    }
    dimensions(size, tint)?;
    let [width, depth, height] = size;
    let open = matches!(family, Family::TimberOpen);
    let wire = matches!(family, Family::Wire);
    let thickness = if open {
        0.04
    } else if wire {
        0.008
    } else {
        0.018
    };
    if !(1..=64).contains(&levels) || height < 0.16 {
        return Err("Open shelf dimensions leave no usable levels.".into());
    }
    let pitch = (height - 0.12 - thickness) / levels as f64;
    let deck = |row| {
        if row == levels {
            height
        } else {
            0.12 + row as f64 * pitch
        }
    };
    let pen = d.pen.with_shelf_patina(super::patina::Shelf::Frame {
        inverse: d.pen.matrix.inverse()?,
        size,
        decks: (0..=levels).map(deck).collect::<Vec<_>>().into(),
        closed: false,
        grime: if open { 0.65 } else { 0.85 },
        seed: super::super::wall::finish_seed(&d.pen.part) as f64,
    });
    let slot = if open { "timber" } else { "metal" };
    let pen = if open {
        wood(&pen, &kit.settings().timber, None, None)?
    } else {
        pen.with_weathering(
            d.mesh,
            kit.settings().weathering.clone(),
            super::super::metal::Kind::Frame,
            None,
            vec!["metal".into()],
        )?
    };
    let d = &mut on(d, pen);
    let bays = (width / 1.05).ceil().max(1.) as usize;
    let inset = if open { 0.025 } else { 0.018 };
    for bay in 0..=bays {
        let x = inset + (width - 2. * inset) * bay as f64 / bays as f64;
        for y in [inset, depth - inset] {
            if wire {
                d.tube(slot, &[[x, y, 0.012], [x, y, height]], 0.009, tint, 6)?;
            } else {
                let half = if open { 0.0225 } else { 0.012 };
                d.cuboid(
                    slot,
                    [x - half, y - half, 0.],
                    [x + half, y + half, height],
                    shade(tint, 0.86),
                    "",
                )?;
                // The uprights' punched slots are sub-millimetre marks: full detail only.
                if !open && !d.mesh.graphics_detail().reduced() {
                    let mut z = 0.18;
                    while z < height - 0.08 {
                        d.cuboid(
                            "paint",
                            [x - 0.003, y - 0.0125, z],
                            [x + 0.003, y - 0.012, z + 0.022],
                            srgb("#292e29"),
                            "d",
                        )?;
                        z += 0.14;
                    }
                }
            }
            if !open {
                d.cuboid(
                    "metal",
                    [(x - 0.016).max(0.), (y - 0.016).max(0.), 0.],
                    [(x + 0.016).min(width), (y + 0.016).min(depth), 0.012],
                    shade(tint, 0.7),
                    "",
                )?;
            }
        }
        if !open && bay < bays {
            let next = inset + (width - 2. * inset) * (bay + 1) as f64 / bays as f64;
            d.bar(
                slot,
                [x, depth - inset, 0.12],
                [next, depth - inset, height - 0.025],
                if wire { 0.007 } else { 0.011 },
                shade(tint, 0.75),
            )?;
        }
    }
    let mut shelves = Vec::new();
    for row in 0..=levels {
        let z = deck(row);
        d.cuboid(
            slot,
            [0.025, 0.025, z - thickness],
            [width - 0.025, depth - 0.025, z],
            tint,
            "",
        )?;
        if !open {
            for y in [inset, depth - inset] {
                d.bar(
                    "metal",
                    [inset, y, z - 0.008],
                    [width - inset, y, z - 0.008],
                    if wire { 0.009 } else { 0.023 },
                    tint,
                )?;
            }
        }
        if wire && row < levels {
            for x in [inset, width - inset] {
                d.bar(
                    "metal",
                    [x, inset, z + 0.045],
                    [x, depth - inset, z + 0.045],
                    0.005,
                    tint,
                )?;
                for y in [inset, depth - inset] {
                    d.bar("metal", [x, y, z], [x, y, z + 0.045], 0.005, tint)?;
                }
            }
        }
        let clear = if row == levels {
            above
        } else {
            pitch - thickness - 0.012
        };
        if clear > 0.1 {
            shelves.push(Shelf {
                x0: 0.035,
                x1: width - 0.035,
                y: 0.035,
                depth: depth - 0.07,
                z,
                clear,
            });
        }
    }
    Ok(shelves)
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Finish {
    Timber,
    Tiled,
    Steel,
}
fn dimensions(size: V3, color: V3) -> Result<(), String> {
    if size
        .iter()
        .any(|v| !v.is_finite() || !(0.08..=20.).contains(v))
        || color
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
    {
        Err("Shop furniture requires bounded positive dimensions and finite color.".into())
    } else {
        Ok(())
    }
}
fn wood(
    pen: &Pen,
    s: &timber::Settings,
    axis: Option<usize>,
    wear: Option<(f64, f64)>,
) -> Result<Pen, String> {
    pen.with_timber(timber::Settings {
        face_grain_axis: axis,
        front_wear: wear.map(|(width, strength)| timber::FrontWear {
            width,
            strength,
            direction: None,
        }),
        textured: s.textured,
        timber_repeat: s.timber_repeat,
        atlas_repeat: s.atlas_repeat,
        ..Default::default()
    })
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
/// Returned shelves are ordered bottom-to-top, then left-to-right; the unit top is last.
pub fn shelving(
    d: &mut Fixture<'_, '_>,
    size: V3,
    levels: usize,
    slot: &str,
    tint: V3,
    above: f64,
    settings: &timber::Settings,
) -> Result<Vec<Shelf>, String> {
    dimensions(size, tint)?;
    if !(1..=64).contains(&levels) || !above.is_finite() || !(0. ..=20.).contains(&above) {
        return Err("Shelving requires bounded levels and top clearance.".into());
    }
    let [width, depth, height] = size;
    let board = 0.022;
    let foot = 0.08;
    let pitch = (height - foot - board) / levels as f64;
    if width < 0.1 || depth < 0.06 || pitch <= board + 0.012 {
        return Err("Shelf dimensions leave no stock clearance.".into());
    }
    let bays = (width / 0.95).round().max(1.) as usize;
    let bay = (width - board) / bays as f64;
    let pen = d.pen.with_shelf_patina(super::patina::Shelf::Frame {
        inverse: d.pen.matrix.inverse()?,
        size,
        decks: (0..=levels)
            .map(|row| foot + row as f64 * pitch + board)
            .collect::<Vec<_>>()
            .into(),
        closed: true,
        grime: 0.65,
        seed: super::super::wall::finish_seed(&d.pen.part) as f64,
    });
    let pen = wood(&pen, settings, None, None)?;
    let d = &mut on(d, pen);
    front(
        d,
        slot,
        [0., width, 0., height],
        depth - 0.006,
        shade(tint, 0.62),
    )?;
    let pen = wood(&d.pen, settings, Some(0), None)?;
    front(
        &mut on(d, pen),
        slot,
        [board, width - board, 0., foot],
        0.02,
        shade(tint, 0.6),
    )?;
    for i in 0..=bays {
        let x = i as f64 * bay;
        let interior = i > 0 && i < bays;
        d.cuboid(
            slot,
            [x, 0., 0.],
            [x + board, depth - 0.006, height],
            shade(tint, 0.9),
            if interior { "dfu" } else { "df" },
        )?;
        if interior {
            d.cuboid(
                slot,
                [x, 0., 0.],
                [x + board, 0.004, height],
                shade(tint, 0.9),
                "lrbfd",
            )?;
        }
    }
    let mut shelves = vec![];
    for level in 0..=levels {
        let z = foot + level as f64 * pitch;
        d.cuboid(
            slot,
            [board, 0.004, z],
            [width - board, depth - 0.006, z + board],
            tint,
            "flr",
        )?;
        if level == levels {
            break;
        }
        for i in 0..bays {
            shelves.push(Shelf {
                x0: i as f64 * bay + board + 0.008,
                x1: (i + 1) as f64 * bay - 0.008,
                y: 0.012,
                depth: depth - 0.03,
                z: z + board,
                clear: pitch - board - 0.012,
            });
        }
    }
    if above > 0.1 {
        shelves.push(Shelf {
            x0: 0.02,
            x1: width - 0.02,
            y: 0.02,
            depth: depth - 0.04,
            z: height,
            clear: above,
        });
    }
    Ok(shelves)
}
pub fn counter(
    d: &mut Fixture<'_, '_>,
    size: V3,
    finish: Finish,
    tint: V3,
    rng: &mut SeededRng,
    settings: &timber::Settings,
) -> Result<Shelf, String> {
    dimensions(size, tint)?;
    let [width, depth, height] = size;
    if width < 0.25 || depth < 0.1 || height < 0.25 {
        return Err("Counter dimensions leave no top or front frame.".into());
    }
    let body = height - 0.04;
    let pen = if matches!(finish, Finish::Timber) {
        wood(&d.pen, settings, None, Some((0.014, 0.65)))?
    } else {
        d.pen.clone()
    };
    let d = &mut on(d, pen);
    let slot = match finish {
        Finish::Timber => "timber",
        Finish::Steel => "metal",
        Finish::Tiled => "paint",
    };
    d.cuboid(
        slot,
        [0.02, 0.03, 0.],
        [width - 0.02, depth, body],
        shade(
            tint,
            if matches!(finish, Finish::Tiled) {
                0.45
            } else {
                0.8
            },
        ),
        "dfu",
    )?;
    match finish {
        Finish::Tiled => {
            let columns = ((width - 0.04) / 0.15).round().max(2.) as usize;
            let rows = (body / 0.15).round().max(2.) as usize;
            let w = (width - 0.04) / columns as f64;
            let h = body / rows as f64;
            for i in 0..columns {
                for j in 0..rows {
                    front(
                        d,
                        "pane",
                        [
                            0.02 + i as f64 * w + 0.004,
                            0.02 + (i + 1) as f64 * w - 0.004,
                            j as f64 * h + 0.004,
                            (j + 1) as f64 * h - 0.004,
                        ],
                        0.024,
                        shade(tint, rng.float(0.86, 1.)),
                    )?;
                }
            }
        }
        Finish::Timber => {
            let panels = (width / 0.55).round().max(2.) as usize;
            let w = (width - 0.04) / panels as f64;
            for i in 0..=panels {
                d.cuboid(
                    "timber",
                    [0.02 + i as f64 * w - 0.03, 0.016, 0.06],
                    [0.02 + i as f64 * w + 0.03, 0.03, body],
                    tint,
                    "dfu",
                )?;
            }
            for z in [0.06, body - 0.09] {
                d.cuboid(
                    "timber",
                    [0.02, 0.016, z],
                    [width - 0.02, 0.03, z + 0.09],
                    tint,
                    "flr",
                )?;
            }
            front(
                d,
                "paint",
                [0.02, width - 0.02, 0., 0.06],
                0.028,
                shade(srgb("#2a2927"), 1.4),
            )?;
        }
        Finish::Steel => {
            d.cuboid(
                "metal",
                [0.02, 0.018, body * 0.5],
                [width - 0.02, 0.03, body * 0.5 + 0.03],
                shade(tint, 0.95),
                "flr",
            )?;
            for x in [0.03, width - 0.07] {
                d.cuboid(
                    "metal",
                    [x, 0.03, 0.],
                    [x + 0.04, 0.07, 0.1],
                    shade(tint, 0.5),
                    "dfu",
                )?;
            }
        }
    }
    let slab = match finish {
        Finish::Timber => shade(tint, 0.7),
        Finish::Steel => shade(srgb("#a4a9aa"), 1.1),
        Finish::Tiled => shade(srgb("#e8e6dc"), 0.95),
    };
    d.cuboid(
        if matches!(finish, Finish::Timber) {
            "timber"
        } else {
            "metal"
        },
        [0., 0., body],
        [width, depth + 0.02, height],
        slab,
        "df",
    )?;
    Ok(Shelf {
        x0: 0.03,
        x1: width - 0.03,
        y: 0.03,
        depth: depth - 0.05,
        z: height,
        clear: 1.,
    })
}
pub fn crate_box(
    d: &mut Fixture<'_, '_>,
    size: V3,
    tint: V3,
    rng: &mut SeededRng,
    settings: &timber::Settings,
) -> Result<Shelf, String> {
    dimensions(size, tint)?;
    let [width, depth, height] = size;
    let pen = wood(&d.pen, settings, None, Some((0.006, 0.75)))?;
    let d = &mut on(d, pen);
    let post = 0.022;
    let slat = 0.008;
    for x in [0., width - post] {
        for y in [0., depth - post] {
            d.cuboid(
                "timber",
                [x, y, 0.],
                [x + post, y + post, height],
                shade(tint, 0.85),
                "d",
            )?;
        }
    }
    d.face(
        "timber",
        &[
            [0., 0., 0.012],
            [width, 0., 0.012],
            [width, depth, 0.012],
            [0., depth, 0.012],
        ],
        [0., 0., 1.],
        shade(tint, 0.7),
    )?;
    let band = height / 3.;
    for row in 0..3 {
        let z0 = row as f64 * band + 0.006;
        let z1 = (row + 1) as f64 * band - 0.012;
        d.cuboid(
            "timber",
            [0., -slat, z0],
            [width, 0., z1],
            shade(tint, rng.float(0.82, 1.08)),
            "dlr",
        )?;
        d.cuboid(
            "timber",
            [0., depth, z0],
            [width, depth + slat, z1],
            shade(tint, rng.float(0.82, 1.08)),
            "dlr",
        )?;
        d.cuboid(
            "timber",
            [-slat, 0., z0],
            [0., depth, z1],
            shade(tint, rng.float(0.82, 1.08)),
            "dbf",
        )?;
        d.cuboid(
            "timber",
            [width, 0., z0],
            [width + slat, depth, z1],
            shade(tint, rng.float(0.82, 1.08)),
            "dbf",
        )?;
    }
    Ok(Shelf {
        x0: post,
        x1: width - post,
        y: post,
        depth: depth - post * 2.,
        z: 0.012,
        clear: height - 0.012,
    })
}
