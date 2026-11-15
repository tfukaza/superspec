use super::{FLOORS, ROOF, TOP};
use crate::worldspec::themes::kowloon::{
    aircon, balconies,
    fixture::Fixture,
    ground, household,
    kit::Kit,
    massing::{Rect, subtract_rect},
    metal::{Binding, Kind},
    molding::{Strip, profiled_strip},
    palette::{Awning, shade, srgb},
    pen::Tint,
    pipes::{Options, PipeSystem},
    skin,
    tarps::{self, Envelope},
    tile, tiled_skin,
};
use superspec_geometry::V3;

pub(super) fn tiled_bands(
    d: &mut Fixture<'_, '_>,
    plaster: &mut skin::Settings,
    backing: &Binding,
    side: &str,
    u0: f64,
    u1: f64,
) -> Result<(), String> {
    let bands = FLOORS[1..]
        .iter()
        .map(|z| Rect {
            u0,
            u1,
            v0: z + 0.16,
            v1: z + 0.83,
        })
        .collect::<Vec<_>>();
    for &band in &bands {
        plaster.rects = plaster
            .rects
            .iter()
            .flat_map(|&wall| subtract_rect(wall, band, 0.001))
            .collect();
    }
    let ceramic = "$kowloon/wall-ceramic";
    let bed = "$kowloon/wall-tile-bed";
    let mut skin = plaster.clone();
    skin.rects = bands;
    skin.finish.coat.tint = srgb("#7d9185");
    skin.tiles = Some(tiled_skin::Settings {
        seed: 23,
        field: tile::Settings {
            seed: 23,
            share: 1.,
            wear: 0.22,
            grime: 0.38,
            size: Some(0.18),
            aspect: Some(1.),
        },
        materials: tile::Materials {
            ceramic: ceramic.into(),
            bed: bed.into(),
            grime: "$kowloon/wall-tile-grime".into(),
            repeat: d.repeat(ceramic),
            bed_repeat: d.repeat(bed),
            bed_roughness: 0.96,
            roughness: 0.65,
        },
        repair: backing.clone(),
    });
    let slot = skin.finish.coat.material.clone();
    skin::draw(d, &slot, &skin, &format!("reference-corner/{side}/jade"))
}

pub(super) fn balcony_door(
    d: &mut Fixture<'_, '_>,
    [left, right, bottom, top]: [f64; 4],
    green: V3,
    trim: V3,
) -> Result<(), String> {
    d.panel("pane", [left, right, bottom, top], -0.237, srgb("#252c29"))?;
    for x in [left, right - 0.065] {
        d.frame_box([x, -0.15, bottom], [x + 0.065, -0.055, top], green)?;
    }
    for z in [bottom, bottom + 0.69, top - 0.36, top - 0.065] {
        d.frame_box(
            [left + 0.065, -0.15, z],
            [right - 0.065, -0.055, z + 0.065],
            green,
        )?;
    }
    d.cuboid(
        "paint",
        [left + 0.065, -0.14, bottom + 0.065],
        [right - 0.065, -0.09, bottom + 0.69],
        green,
        "",
    )?;
    let [x0, x1, z0, z1] = [left + 0.07, right - 0.07, bottom + 0.76, top - 0.37];
    d.panel("glass", [x0, x1, z0, z1], -0.11, srgb("#84918b"))?;
    screen(d, [x0, x1, z0, z1], -0.105, [0.095, 0.1], 0.0025)?;
    let x = right - 0.14;
    d.tube(
        "metal",
        &[
            [x, -0.04, bottom + 1.05],
            [x, -0.015, bottom + 1.05],
            [x, -0.015, bottom + 1.24],
        ],
        0.012,
        srgb("#dfcfaa"),
        8,
    )?;
    for z in [bottom + 0.35, top - 0.45] {
        d.cuboid(
            "metal",
            [left + 0.025, -0.06, z],
            [left + 0.085, -0.035, z + 0.12],
            green,
            "",
        )?;
    }
    d.cuboid(
        "concrete",
        [left - 0.03, -0.24, bottom - 0.03],
        [right + 0.03, 0.13, bottom + 0.025],
        trim,
        "",
    )
}

fn screen(
    d: &mut Fixture<'_, '_>,
    [x0, x1, z0, z1]: [f64; 4],
    y: f64,
    [dx, dz]: [f64; 2],
    radius: f64,
) -> Result<(), String> {
    let paint = srgb("#70756a");
    for column in 0..=((x1 - x0) / dx) as usize {
        let x = x0 + column as f64 * dx;
        d.tube("metal", &[[x, y, z0], [x, y, z1]], radius, paint, 4)?;
    }
    for row in 0..=((z1 - z0) / dz) as usize {
        let z = z0 + row as f64 * dz;
        d.tube("metal", &[[x0, y, z], [x1, y, z]], radius, paint, 4)?;
    }
    Ok(())
}

pub(super) fn window_interior(
    d: &mut Fixture<'_, '_>,
    [left, right, bottom, top]: [f64; 4],
    index: usize,
) -> Result<(), String> {
    let [x0, x1, z0, z1] = [left + 0.06, right - 0.06, bottom + 0.06, top - 0.33];
    if index == 7 {
        let end = x0 + (x1 - x0) * 0.46;
        d.panel("glass", [x0, end, z0, z1], -0.1, srgb("#8d998f"))?;
        return screen(d, [x0, end, z0, z1], -0.095, [0.045, 0.05], 0.0018);
    }
    let width = (x1 - x0) * 0.34;
    let start = match index % 3 {
        0 => x0,
        2 => x1 - width,
        _ => return Ok(()),
    };
    for fold in 0..8 {
        let a = start + width * f64::from(fold) / 8.;
        let b = start + width * f64::from(fold + 1) / 8.;
        let ya = -0.16 + f64::from(fold % 2) * 0.034;
        let yb = -0.16 + f64::from((fold + 1) % 2) * 0.034;
        d.sheet(
            "tarp",
            &[[a, ya, z0], [b, yb, z0 + 0.025], [b, yb, z1], [a, ya, z1]],
            srgb("#8d8874"),
        )?;
    }
    Ok(())
}

pub(super) fn gate(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    side: &str,
    [left, right, _, head]: [f64; 4],
    green: V3,
    trim: V3,
    cream: V3,
) -> Result<(), String> {
    let front = side == "front";
    let key = if front {
        "closed-pharmacy"
    } else {
        "return-gate"
    };
    ground::draw(
        d,
        ground::Kind::Gate,
        &ground::Unit {
            u0: 0.15,
            u1: if front { 2.95 } else { 2.8 },
            height: FLOORS[0],
            key: key.into(),
        },
        &ground::Settings {
            kit,
            style: &ground::Style {
                frame: green,
                trim,
                shutter: green,
                accent: srgb("#7d9185"),
            },
            posters: false,
        },
        &mut kit.rng(key)?,
    )?;
    d.panel("wall", [left, right, 2.58, head], 0., cream)
}

fn profile(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    width: f64,
    section: &[[f64; 2]],
    miters: [Option<f64>; 2],
    setback: f64,
    color: V3,
) -> Result<(), String> {
    let repeat = d.repeat(slot);
    profiled_strip(
        &d.pen,
        d.mesh,
        slot,
        &Strip {
            span: [0., width],
            section,
            omitted_edges: &[],
            miters,
            setback,
        },
        &Tint::Color(color),
        repeat,
    )?;
    Ok(())
}

pub(super) fn cornices(
    d: &mut Fixture<'_, '_>,
    side: &str,
    width: f64,
    trim: V3,
) -> Result<(), String> {
    let clip = (std::f64::consts::PI / 8.).tan();
    let miters = match side {
        "front" => [-1., clip],
        "corner" => [-clip, clip],
        "right" => [-clip, 1.],
        _ => [-1., 1.],
    }
    .map(Some);
    for z in [FLOORS[1], FLOORS[2], ROOF] {
        profile(
            d,
            "concrete",
            width,
            &[
                [-0.01, z - 0.12],
                [0.11, z - 0.12],
                [0.16, z - 0.035],
                [0.16, z + 0.065],
                [0.02, z + 0.11],
            ],
            miters,
            0.012,
            trim,
        )?;
    }
    profile(
        d,
        "concrete",
        width,
        &[
            [-0.24, TOP],
            [0.08, TOP],
            [0.08, TOP + 0.08],
            [-0.24, TOP + 0.08],
        ],
        miters,
        0.012,
        trim,
    )?;
    d.cuboid("wall", [0., -0.2, ROOF], [width, -0.04, TOP], trim, "f")
}

pub(super) fn balcony(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    green: V3,
    cream: V3,
    trim: V3,
) -> Result<(), String> {
    let original = d.pen.clone();
    let pen = original.into_part("stepped-balcony")?;
    d.pen = pen.clone();
    let main = [
        [2.5, 0., 0.],
        [9.25, 0., 0.],
        [9.05, 1.13, 0.],
        [2.78, 1.13, 0.],
    ];
    let joint = [2.5 + 0.28 * 0.43 / 1.13, 0.43, 0.];
    let shallow = [[0.42, 0., 0.], main[0], joint, [0.42, 0.43, 0.]];
    for outline in [main, shallow] {
        d.face(
            "concrete",
            &outline.map(|[x, y, _]| [x, y, 3.35]),
            [0., 0., 1.],
            trim,
        )?;
        d.face(
            "concrete",
            &outline.map(|[x, y, _]| [x, y, 3.08]),
            [0., 0., -1.],
            trim,
        )?;
        d.panel(
            "concrete",
            [outline[0][0], outline[1][0], 3.08, 3.35],
            0.,
            trim,
        )?;
    }
    let path = [shallow[0], shallow[3], joint, main[3], main[2], main[1]];
    let segments: Vec<_> = path
        .windows(2)
        .map(|p| {
            let [x, y] = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let length = x.hypot(y);
            ([x / length, y / length, 0.], length)
        })
        .collect();
    let turn = |a: V3, b: V3| (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]);
    for (i, &(tangent, length)) in segments.iter().enumerate() {
        let miters = [
            i.checked_sub(1)
                .map(|j| (turn(segments[j].0, tangent) / 2.).tan()),
            segments
                .get(i + 1)
                .map(|&(next, _)| -(turn(tangent, next) / 2.).tan()),
        ];
        d.pen = pen.facing(path[i], tangent, [-tangent[1], tangent[0], 0.])?;
        d.panel("concrete", [0., length, 3.08, 3.35], 0., trim)?;
        d.pen = d.pen.moved([0., 0., 3.35])?;
        profile(
            d,
            "wall",
            length,
            &[[-0.075, 0.02], [0., 0.02], [0., 0.465], [-0.075, 0.465]],
            miters,
            0.015,
            cream,
        )?;
        d.pen = d.pen.with_weathering(
            d.mesh,
            kit.settings().weathering.clone(),
            Kind::Cage,
            None,
            vec!["metal".into()],
        )?;
        let start = -0.035 * miters[0].unwrap_or(0.);
        let end = length - 0.035 * miters[1].unwrap_or(0.);
        for z in [0.59, 0.83, 1.08] {
            d.tube(
                "metal",
                &[[start, -0.035, z], [end, -0.035, z]],
                0.023,
                green,
                10,
            )?;
        }
        let posts = (length / 1.4).ceil() as usize;
        for j in 0..=posts {
            let x = start + (end - start) * j as f64 / posts as f64;
            d.tube(
                "metal",
                &[[x, -0.035, 0.28], [x, -0.035, 1.11]],
                0.026,
                green,
                10,
            )?;
        }
    }
    d.pen = pen;
    for x in [3.05, 4.85, 6.7, 8.55] {
        let a = [
            [x - 0.11, 0.02, 2.79],
            [x - 0.11, 1.02, 3.08],
            [x - 0.11, 0.02, 3.08],
        ];
        let b = a.map(|[x, y, z]| [x + 0.22, y, z]);
        d.face("concrete", &a, [-1., 0., 0.], trim)?;
        d.face("concrete", &b, [1., 0., 0.], trim)?;
        d.face("concrete", &[a[0], a[1], b[1], b[0]], [0., 0.29, -1.], trim)?;
    }
    balconies::anti_climb(d, 2.64, 4.45, 0.97, green, &kit.settings().weathering)?;
    d.pen = original;
    Ok(())
}

pub(super) fn ground_contact(d: &mut Fixture<'_, '_>, kit: &Kit) -> Result<(), String> {
    let original = d.pen.clone();
    d.pen = original.into_part("pavement")?;
    d.face(
        "concrete",
        &[
            [-0.85, -1.91, -0.14],
            [11.25, -1.91, -0.14],
            [11.25, 7.78, -0.14],
            [-0.85, 7.78, -0.14],
        ],
        [0., 0., 1.],
        srgb("#55564f"),
    )?;
    d.cuboid(
        "concrete",
        [-0.45, -1.31, -0.14],
        [10.5, -0.012, -0.08],
        srgb("#696a60"),
        "d",
    )?;
    d.cuboid(
        "concrete",
        [9.81, -0.012, -0.14],
        [10.8, 7.68, -0.08],
        srgb("#696a60"),
        "d",
    )?;
    let mut rng = kit.rng("worn-paving")?;
    let mut stone = || shade(srgb("#a8a394"), rng.float(0.67, 1.08));
    for column in 0..20 {
        let x = -0.45 + f64::from(column) * 0.55;
        let right = (x + 0.547).min(10.5);
        for row in 0..3 {
            let y = -1.3 + f64::from(row) * 0.43;
            d.cuboid(
                "concrete",
                [x, y, -0.09],
                [right, (y + 0.427).min(-0.012), 0.015],
                stone(),
                "d",
            )?;
        }
        d.cuboid(
            "concrete",
            [x, -1.44, -0.13],
            [right, -1.31, 0.005],
            stone(),
            "d",
        )?;
    }
    for row in 0..14 {
        let y = 0.02 + f64::from(row) * 0.55;
        for x in [9.81, 10.23] {
            d.cuboid(
                "concrete",
                [x, y, -0.09],
                [x + 0.417, y + 0.547, 0.015],
                stone(),
                "d",
            )?;
        }
        d.cuboid(
            "concrete",
            [10.66, y, -0.13],
            [10.8, y + 0.547, 0.005],
            stone(),
            "d",
        )?;
    }
    let iron = srgb("#343632");
    d.cuboid(
        "paint",
        [10.12, -1.21, 0.017],
        [10.52, -0.68, 0.023],
        iron,
        "",
    )?;
    for row in 0..7 {
        let y = -1.16 + f64::from(row) * 0.072;
        d.cuboid(
            "metal",
            [10.13, y, 0.025],
            [10.51, y + 0.025, 0.038],
            iron,
            "",
        )?;
    }
    d.pen = original;
    Ok(())
}

pub(super) fn services(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    side: &str,
    green: V3,
) -> Result<(), String> {
    let weathering = &kit.settings().weathering;
    let pipe = |variant: &str, color: &str, radius| {
        PipeSystem::new(Options {
            radius,
            color: srgb(color),
            sides: 10,
            variant: variant.into(),
            weathering: Some(weathering.clone()),
        })
    };
    let canvas = |d: &mut Fixture<'_, '_>,
                  [x0, x1, z, depth, drop, valance]: [f64; 6],
                  [base, hem]: [&str; 2],
                  key: &str,
                  cloth: bool| {
        household::awning(
            d,
            Envelope {
                x0,
                x1,
                z,
                depth,
                drop,
                valance,
            },
            &Awning {
                canvas: cloth,
                base: srgb(base),
                hem: srgb(hem),
            },
            key,
            Some(weathering),
        )
    };
    if side == "front" {
        aircon::air_conditioner(
            d,
            2.05,
            7.34,
            0.36,
            &mut kit.rng("left-ac")?,
            Some(weathering),
        )?;
        let connections = pipe("condenser-lines", "#9b9985", 0.014)?;
        for z in [7.38, 10.38] {
            connections.run(
                d,
                &[
                    [8.97, 0.22, z + 0.19],
                    [9.12, 0.12, z + 0.19],
                    [9.12, 0.12, z - 0.49],
                    [9.3, 0.09, z - 0.49],
                    [9.3, 0.09, z + 1.32],
                ],
                Some(0.),
            )?;
            connections.run(
                d,
                &[
                    [5.01, 0.27, z + 0.03],
                    [5.48, 0.08, z + 0.03],
                    [5.48, 0.08, z - 0.38],
                ],
                Some(0.),
            )?;
        }
        pipe("electrical", "#4d5048", 0.012)?.run(
            d,
            &[
                [2.85, 0.04, 0.18],
                [2.85, 0.04, 3.],
                [5.56, 0.04, 3.],
                [5.56, 0.04, 12.2],
            ],
            Some(0.),
        )?;
        canvas(
            d,
            [0.12, 2.95, 2.75, 1.27, 0.46, 0.],
            ["#778778", "#a39f81"],
            "left-canvas",
            true,
        )?;
        tarps::canvas(
            d,
            Envelope {
                x0: 2.98,
                x1: 9.18,
                z: 2.7,
                depth: 1.5,
                drop: 0.2,
                valance: 0.22,
            },
            srgb("#ad7b6b"),
            srgb("#d2c6af"),
            "main-canvas",
            Some(weathering),
            Some(tarps::Pattern {
                color: srgb("#d2c6af"),
                width: 0.22,
            }),
        )?;
    } else {
        pipe("side-lines", "#696c60", 0.014)?.run(
            d,
            &[
                [0.2, 0.055, 12.2],
                [0.2, 0.055, 6.2],
                [2.68, 0.055, 6.2],
                [2.68, 0.055, 3.01],
            ],
            Some(0.),
        )?;
        household::cage(
            d,
            &household::Cage {
                span: [1.08, 1.93],
                heights: [10.54, 11.19],
                depth: 0.48,
                color: green,
                stocked: false,
            },
            &mut kit.rng("return-cage")?,
            Some(weathering),
            false,
        )?;
        canvas(
            d,
            [0.36, 2.48, 9.12, 0.36, 0.11, 0.],
            ["#777364", "#3f554b"],
            "return-hood",
            false,
        )?;
        canvas(
            d,
            [3., 5.96, 2.72, 0.99, 0.37, 0.],
            ["#868879", "#dfcfaa"],
            "side-canvas",
            true,
        )?;
    }
    Ok(())
}
