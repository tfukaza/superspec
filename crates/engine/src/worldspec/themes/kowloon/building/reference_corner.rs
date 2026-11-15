//! The reference shop house in its authored front-left Z-up frame.
use super::placement::Site;
use crate::worldspec::themes::kowloon::{
    aircon, facade_material, facade_plan, fixture::Fixture, kit::Kit, massing::Rect,
    metal::Binding, palette::srgb, pen::Pen, shop, signs::book::Book, skin, wall, windows,
};
use superspec_geometry::V3;
#[path = "reference_details.rs"]
mod details;

const FLOORS: [f64; 3] = [3.35, 6.35, 9.35];
const TOP: f64 = 13.1;
const ROOF: f64 = 12.35;

pub(super) fn draw(
    d: &mut Fixture<'_, '_>,
    site: &Site,
    kit: &Kit,
    settings: &super::materials::Settings<'_>,
    coat: Binding,
    backing: Binding,
    assets: shop::shell::Assets,
) -> Result<Book, String> {
    let cream = srgb("#eee2c9");
    let trim = srgb("#ddd2bc");
    let green = srgb("#3f554b");
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    let root = Pen::new(site.matrix, "reference-corner")?;
    let lot = crate::worldspec::themes::kowloon::massing::Lot {
        ground: FLOORS[0],
        ..site
            .lots
            .first()
            .ok_or("Reference building has no lot.")?
            .clone()
    };
    let mut signs = Book::default();
    let faces = [
        ("front", [0., 0., 0.], [1., 0., 0.], [0., -1., 0.], 9.56),
        (
            "corner",
            [9.56, 0., 0.],
            [diagonal, diagonal, 0.],
            [diagonal, -diagonal, 0.],
            0.24 / diagonal,
        ),
        ("right", [9.8, 0.24, 0.], [0., 1., 0.], [1., 0., 0.], 6.76),
        ("back", [9.8, 7., 0.], [-1., 0., 0.], [0., 1., 0.], 9.8),
        ("left", [0., 7., 0.], [0., -1., 0.], [-1., 0., 0.], 7.),
    ];
    for (name, origin, tangent, outward, width) in faces {
        d.pen = root
            .into_part(&format!("reference-corner/{name}"))?
            .facing(origin, tangent, outward)?;
        let mut openings = Vec::new();
        for (story, base) in FLOORS.into_iter().enumerate() {
            let banks: &[[f64; 2]] = match (name, story) {
                ("front", 0) => &[[0.55, 2.65], [3.2, 4.15], [5.95, 9.05]],
                ("front", _) => &[[0.55, 2.65], [3.2, 5.4], [5.95, 9.05]],
                ("right", _) => &[[0.45, 2.4], [3.05, 5.45]],
                _ => &[],
            };
            for &[u0, u1] in banks {
                openings.push(Rect {
                    u0,
                    u1,
                    v0: base + 0.85,
                    v1: base + 2.72,
                });
            }
        }
        if name == "front" {
            openings.push(Rect {
                u0: 4.43,
                u1: 5.4,
                v0: 3.38,
                v1: 6.07,
            });
        }
        let windows = openings.len();
        let storefronts: &[[f64; 4]] = match name {
            "front" => &[[0.4, 2.7, 0.12, 2.7], [3.05, 9.05, 0.12, 2.85]],
            "right" => &[[0.4, 2.55, 0.12, 2.65], [3.05, 5.9, 0.12, 2.65]],
            _ => &[],
        };
        openings.extend(
            storefronts
                .iter()
                .map(|&[u0, u1, v0, v1]| Rect { u0, u1, v0, v1 }),
        );
        let mut rng = kit.rng(&format!("reference-corner/{name}"))?;
        let seed = rng.integer(1, 1_000_000);
        let mut skin = skin::Settings {
            rects: vec![Rect {
                u0: 0.,
                u1: width,
                v0: 0.,
                v1: TOP,
            }],
            openings: openings.clone(),
            u_marks: openings.iter().flat_map(|r| [r.u0, r.u1]).collect(),
            z_marks: openings.iter().flat_map(|r| [r.v0, r.v1]).collect(),
            finish: facade_material::Settings {
                concrete: false,
                coat: facade_material::Coat {
                    material: coat.material.clone(),
                    repeat: coat.repeat,
                    tint: cream,
                },
                backing: backing.clone(),
                wear: settings.wear,
                density: settings.density,
                variant: (seed % 2) as u32,
            },
            condition: wall::Settings {
                base: Some(0.),
                ledges: FLOORS
                    .into_iter()
                    .chain([ROOF, TOP])
                    .map(|z| wall::Ledge {
                        u0: 0.,
                        u1: width,
                        z,
                        flow: 1.,
                    })
                    .chain(openings[..windows].iter().map(|r| wall::Ledge {
                        u0: r.u0,
                        u1: r.u1,
                        z: r.v0,
                        flow: 0.8,
                    }))
                    .collect(),
                openings: openings
                    .iter()
                    .map(|r| wall::Opening {
                        u0: r.u0,
                        u1: r.u1,
                        z0: r.v0,
                        z1: r.v1,
                    })
                    .collect(),
                grime: settings.grime,
                wear: settings.wear,
                seed: seed as f64,
            },
            washes: vec![skin::Wash {
                bounds: [0., 0., width, 1.02],
                tint: srgb("#343632"),
                lower_inclusive: true,
            }],
            tiles: None,
        };
        let band = match name {
            "front" => Some([5.76, width]),
            "corner" => Some([0., width]),
            "right" => Some([0., 5.7]),
            _ => None,
        };
        if let Some([u0, u1]) = band {
            details::tiled_bands(d, &mut skin, &backing, name, u0, u1)?;
        }
        skin::draw(
            d,
            &coat.material,
            &skin,
            &format!("reference-corner/{name}"),
        )?;
        for (i, r) in openings.iter().enumerate() {
            let span = [r.u0, r.u1, r.v0, r.v1];
            let depth = if i < windows { 0.24 } else { 0.55 };
            reveal(d, &coat.material, span, depth, trim)?;
            if name == "front" && i + 1 == windows {
                details::balcony_door(d, span, green, trim)?;
            } else if i < windows {
                windows::window(
                    d,
                    span,
                    &windows::Look {
                        weathering: Some(kit.settings().weathering.clone()),
                        kind: windows::Kind::Casement,
                        frame: green,
                        trim,
                        curtain: srgb("#dfcfaa"),
                        reach: 0.9,
                        hood: false,
                        lit: false,
                        boarded: false,
                        fixed: false,
                        recess: depth,
                        transom_ratio: Some(0.84),
                        lower_pane_bars: (name == "front" && (i / 3 + i % 3) % 2 == 0)
                            .then(|| vec![0.38]),
                        timber_textured: kit.settings().timber.textured,
                    },
                    &mut rng,
                )?;
                if name == "front" {
                    details::window_interior(d, span, i)?;
                }
            } else if i == windows + 1 {
                let (trade, depth, key) = if name == "front" {
                    (
                        shop::shell::Trade::ReferenceProvisions,
                        3.15,
                        "main-provisions",
                    )
                } else {
                    (shop::shell::Trade::Hardware, 2.4, "return-hardware")
                };
                let room = facade_plan::Room {
                    x0: r.u0,
                    x1: r.u1,
                    floor: r.v0,
                    head: r.v1,
                    depth,
                    key: format!("reference-corner/{key}"),
                };
                let proposals = shop::shell::draw(
                    d,
                    &shop::shell::Request {
                        plan: &room,
                        lot: &lot,
                        coat: &coat.material,
                        kit,
                        assets,
                        trade: Some(trade),
                        shutter_color: (name == "front").then(|| srgb("#758077")),
                        frontage: None,
                    },
                )?;
                for proposal in proposals {
                    signs.propose(&site.host, kit.settings(), proposal)?;
                }
            } else {
                details::gate(d, kit, name, span, green, trim, cream)?;
            }
        }
        details::cornices(d, name, width, trim)?;
        if matches!(name, "front" | "right") {
            details::services(d, kit, name, green)?;
            let weathering = Some(&kit.settings().weathering);
            for (index, z) in [7.38, 10.38].into_iter().enumerate() {
                if name == "front" {
                    aircon::condenser(
                        d,
                        [7.7, 9.03, z, 0.78],
                        &mut kit.rng(&format!("condenser-{index}"))?,
                        weathering,
                    )?;
                    aircon::air_conditioner(
                        d,
                        4.55,
                        z - 0.05,
                        0.36,
                        &mut kit.rng(&format!("central-ac-{index}"))?,
                        weathering,
                    )?;
                } else {
                    aircon::air_conditioner(
                        d,
                        1.5,
                        z + 0.2,
                        0.36,
                        &mut kit.rng(&format!("side-ac-{index}"))?,
                        weathering,
                    )?;
                }
            }
            let pipes = crate::worldspec::themes::kowloon::pipes::PipeSystem::new(
                crate::worldspec::themes::kowloon::pipes::Options {
                    radius: 0.043,
                    color: srgb("#605b49"),
                    sides: 12,
                    variant: "building-drains".into(),
                    weathering: weathering.cloned(),
                },
            )?;
            let positions: &[_] = if name == "front" {
                &[0.2, 5.65, 9.36]
            } else {
                &[2.76]
            };
            for &x in positions {
                pipes.run(
                    d,
                    &[
                        [x, 0.07, 12.28],
                        [x, 0.07, 3.15],
                        [x + 0.11, 0.12, 2.92],
                        [x + 0.11, 0.12, 0.12],
                    ],
                    Some(0.),
                )?;
            }
            if name == "front" {
                use crate::worldspec::themes::kowloon::{
                    household, palette::Awning, tarps::Envelope,
                };
                for (index, [x0, x1, z, depth, drop]) in [
                    [5.88, 8.22, 9.10, 0.32, 0.10],
                    [5.88, 8.22, 12.10, 0.32, 0.10],
                    [0.47, 2.72, 12.11, 0.36, 0.10],
                ]
                .into_iter()
                .enumerate()
                {
                    household::awning(
                        d,
                        Envelope {
                            x0,
                            x1,
                            z,
                            depth,
                            drop,
                            valance: 0.,
                        },
                        &Awning {
                            canvas: false,
                            base: srgb("#6c6a5a"),
                            hem: green,
                        },
                        &format!("reference-hood-{index}"),
                        weathering,
                    )?;
                }
                for (index, [x0, x1, z0, z1]) in [[4.08, 5.04, 7.3, 8.02], [1.6, 2.51, 7.29, 7.96]]
                    .into_iter()
                    .enumerate()
                {
                    household::cage(
                        d,
                        &household::Cage {
                            span: [x0, x1],
                            heights: [z0, z1],
                            depth: 0.48,
                            color: srgb("#857a61"),
                            stocked: false,
                        },
                        &mut kit.rng(&format!("reference-cage-{index}"))?,
                        weathering,
                        d.mesh.has_source_material("balcony-cloth"),
                    )?;
                }
                household::meter_bank(
                    d,
                    2.79,
                    1.47,
                    1,
                    &mut kit.rng("meters")?,
                    &kit.settings().weathering,
                )?;
                let posters = d.mesh.has_source_material("posters");
                crate::worldspec::themes::kowloon::posters::notices(
                    d,
                    [9.14, 9.38, 1.17, 1.97],
                    &mut kit.rng("notices")?,
                    posters,
                )?;
                d.cuboid(
                    "timber",
                    [5.65, 0.77, 3.73],
                    [7.45, 1.01, 3.78],
                    srgb("#78644b"),
                    "",
                )?;
                household::plants(
                    d,
                    [5.76, 7.47],
                    0.91,
                    3.78,
                    &mut kit.rng("balcony-plants")?,
                    d.mesh.has_source_material("balcony-plants"),
                )?;
                sign(
                    d,
                    kit,
                    [-1.03, 0.85, 4.72],
                    [0.94, 2.82],
                    "永昌藥行",
                    false,
                    false,
                )?;
            } else {
                sign(
                    d,
                    kit,
                    [0.02, 0.78, 6.05],
                    [0.94, 3.33],
                    "興隆五金",
                    true,
                    false,
                )?;
                sign(
                    d,
                    kit,
                    [-0.02, 0.83, 4.95],
                    [1.95, 0.64],
                    "電器修理",
                    false,
                    true,
                )?;
            }
        }
        if name == "front" {
            shop::provisions::street(d, kit, assets.room.grocer_props)?;
            details::balcony(d, kit, green, cream, trim)?;
        } else if name == "right" {
            shop::goods::carton_stack(
                d,
                [5.45, 0.22, 0.03],
                0.43,
                0.91,
                &mut kit.rng("side-cartons")?,
                assets.room.grocer_props,
            )?;
        }
    }
    d.pen = root;
    let deck: [V3; 5] = [
        [0., 0., ROOF],
        [9.56, 0., ROOF],
        [9.8, 0.24, ROOF],
        [9.8, 7., ROOF],
        [0., 7., ROOF],
    ];
    d.face("concrete", &deck, [0., 0., 1.], trim)?;
    d.face(
        "concrete",
        &deck.map(|[x, y, z]| [x, y, z - 0.2]),
        [0., 0., -1.],
        trim,
    )?;
    details::ground_contact(d, kit)?;
    d.pen = d.pen.into_part("roof-equipment")?.moved([0., 0., ROOF])?;
    crate::worldspec::themes::kowloon::roof_access::stair_hut(
        d,
        crate::worldspec::themes::kowloon::roof_access::Plot {
            x0: 2.,
            x1: 4.05,
            y0: 2.15,
            y1: 4.35,
        },
        2.35,
        &crate::worldspec::themes::kowloon::roof_access::Settings {
            weathering: kit.settings().weathering.clone(),
            wall: coat,
            concrete: backing,
            wall_tint: cream,
            lot_key: "reference-corner".into(),
            masonry: None,
        },
    )?;
    crate::worldspec::themes::kowloon::water_tanks::round_tank(
        d,
        [6.05, 3.05, 0.],
        &mut kit.rng("roof-tank")?,
        &crate::worldspec::themes::kowloon::water_tanks::Settings {
            weathering: kit.settings().weathering.clone(),
            brushed: Binding {
                material: "brushed-metal".into(),
                repeat: d.repeat("brushed-metal"),
            },
        },
    )?;
    d.tube(
        "metal",
        &[[0.8, 1.2, 0.05], [0.8, 1.2, 1.7]],
        0.018,
        green,
        8,
    )?;
    d.tube(
        "metal",
        &[[0.1, 1.2, 1.55], [1.5, 1.2, 1.55]],
        0.009,
        green,
        6,
    )?;
    for x in [0.22, 0.45, 0.68, 0.91, 1.14, 1.37] {
        d.tube(
            "metal",
            &[[x, 0.92, 1.55], [x, 1.48, 1.55]],
            0.006,
            green,
            6,
        )?;
    }
    for x in [4.25, 4.62] {
        d.tube(
            "metal",
            &[[x, 2.4, 0.05], [x, 2.4, 2.1], [x, 2.65, 2.22]],
            0.025,
            green,
            10,
        )?;
    }
    for i in 0..7 {
        let z = 0.28 + f64::from(i) * 0.3;
        d.tube("metal", &[[4.25, 2.4, z], [4.62, 2.4, z]], 0.017, green, 8)?;
    }
    Ok(signs)
}

fn sign(
    d: &mut Fixture<'_, '_>,
    kit: &Kit,
    at: V3,
    [width, height]: [f64; 2],
    text: &str,
    red: bool,
    horizontal: bool,
) -> Result<(), String> {
    use crate::worldspec::themes::kowloon::signs::{
        Construction, Outline,
        lettering::{Direction, Face},
        panel,
    };
    let ivory = srgb("#dfcfaa");
    let oxblood = srgb("#793f32");
    let (ground, ink) = if red {
        (oxblood, ivory)
    } else {
        (ivory, oxblood)
    };
    let mut pen = d
        .pen
        .into_part(&format!("{}/sign-{text}", d.pen.part))?
        .moved(at)?;
    if red {
        pen = pen.facing([0.; 3], [0.6, 0.8, 0.], [-0.8, 0.6, 0.])?;
    }
    let hardware = panel::Hardware {
        slot: "metal".into(),
        tint: srgb("#504d3d"),
        weathering: Some(kit.settings().weathering.clone()),
    };
    panel::draw(
        &mut Fixture {
            pen,
            mesh: d.mesh,
            repeats: d.repeats,
        },
        &panel::Design {
            width,
            height,
            depth: 0.045,
            construction: Construction::Sheet,
            outline: Outline::Rectangle,
            ground,
            border: Some(ink),
            stack: panel::Stack::Down,
            blocks: vec![panel::Block {
                text: text.into(),
                face: Face::Kai,
                direction: if horizontal {
                    Direction::Ltr
                } else {
                    Direction::Ttb
                },
                ink,
                share: 1.,
                shadow: None,
            }],
            age: 0.7,
            phase: at[0] * 5. + at[2],
        },
        panel::Sides::Both,
        &hardware,
        kit.settings(),
    )?;
    for z in [at[2] + 0.12, at[2] + height - 0.12] {
        if red {
            let end = [at[0] + width * 0.6, at[1] + width * 0.8, z];
            d.tube(
                "metal",
                &[[at[0], 0.02, z], [at[0], at[1], z], end],
                0.027,
                hardware.tint,
                10,
            )?;
            d.tube(
                "metal",
                &[[at[0] + 0.5, 0.02, z + 0.25], end],
                0.018,
                hardware.tint,
                8,
            )?;
            continue;
        }
        let anchor = if at[0] < 0. && !horizontal {
            0.15
        } else {
            at[0] - 0.12
        };
        let y = at[1] + if horizontal { 0.08 } else { 0. };
        let endpoint = if horizontal {
            at[0] + width + 0.1
        } else {
            at[0] - 0.08
        };
        d.tube(
            "metal",
            &[[anchor, 0.02, z], [anchor, y, z], [endpoint, y, z]],
            0.023,
            hardware.tint,
            8,
        )?;
        let brace = if horizontal {
            anchor
        } else {
            at[0] + width - 0.05
        };
        d.tube(
            "metal",
            &[[anchor, 0.02, z + 0.3], [brace, y, z]],
            0.018,
            hardware.tint,
            8,
        )?;
    }
    Ok(())
}

fn reveal(
    d: &mut Fixture<'_, '_>,
    slot: &str,
    [u0, u1, v0, v1]: [f64; 4],
    depth: f64,
    tint: V3,
) -> Result<(), String> {
    for (x, sign) in [(u0, 1.), (u1, -1.)] {
        d.face(
            slot,
            &[[x, 0., v0], [x, -depth, v0], [x, -depth, v1], [x, 0., v1]],
            [sign, 0., 0.],
            tint,
        )?;
    }
    for (z, sign) in [(v0, 1.), (v1, -1.)] {
        d.face(
            slot,
            &[[u0, 0., z], [u0, -depth, z], [u1, -depth, z], [u1, 0., z]],
            [0., 0., sign],
            tint,
        )?;
    }
    Ok(())
}
