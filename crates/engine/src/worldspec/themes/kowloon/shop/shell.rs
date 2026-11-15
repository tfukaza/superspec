//! Open shop shells, raised shutters, trade selection and separate street displays.
use super::super::{
    facade_plan,
    fixture::Fixture,
    kit::Kit,
    massing::Lot,
    metal,
    palette::{shade, srgb},
    pen::Pen,
    shutters,
    signs::{
        frontage as signs,
        mounts::{Proposal, WallRect},
    },
    tile,
    wall::finish_seed,
    windows,
};
use super::{
    display, frontage, greengrocer, grocer, hardware, herbalist, provisions, roast,
    room::{self, Interior, Room},
};
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Trade {
    Grocer,
    Greengrocer,
    Roast,
    Hardware,
    Herbalist,
    ReferenceProvisions,
}
impl Trade {
    pub fn name(self) -> &'static str {
        match self {
            Self::Grocer => "grocer",
            Self::Greengrocer => "greengrocer",
            Self::Roast => "roast",
            Self::Hardware => "hardware",
            Self::Herbalist => "herbalist",
            Self::ReferenceProvisions => "reference-provisions",
        }
    }
    fn interior(self, rng: &mut SeededRng) -> Interior {
        match self {
            Self::Grocer => grocer::interior(rng),
            Self::Greengrocer => greengrocer::interior(rng),
            Self::Roast => roast::interior(rng),
            Self::Hardware => hardware::interior(rng),
            Self::Herbalist => herbalist::interior(rng),
            Self::ReferenceProvisions => provisions::interior(),
        }
    }
    fn display(self) -> Option<display::Trade> {
        match self {
            Self::Grocer => Some(display::Trade::Grocer),
            Self::Greengrocer => Some(display::Trade::Greengrocer),
            Self::Roast | Self::ReferenceProvisions => None,
            Self::Hardware => Some(display::Trade::Hardware),
            Self::Herbalist => Some(display::Trade::Herbalist),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Assets {
    pub room: room::Assets,
    pub restaurant: roast::Assets,
}
pub struct Request<'a> {
    pub plan: &'a facade_plan::Room,
    pub lot: &'a Lot,
    pub coat: &'a str,
    pub kit: &'a Kit,
    pub assets: Assets,
    pub trade: Option<Trade>,
    pub shutter_color: Option<V3>,
    pub frontage: Option<frontage::Available<'a>>,
}
fn select(rng: &mut SeededRng) -> Trade {
    let mut roll = rng.next_f64() * 11.5;
    for (trade, weight) in [
        (Trade::Grocer, 3.),
        (Trade::Greengrocer, 2.5),
        (Trade::Roast, 2.),
        (Trade::Hardware, 2.),
        (Trade::Herbalist, 2.),
    ] {
        roll -= weight;
        if roll <= 0. {
            return trade;
        }
    }
    Trade::Herbalist
}
fn on<'a, 'c>(d: &'a mut Fixture<'_, 'c>, pen: Pen) -> Fixture<'a, 'c> {
    Fixture {
        pen,
        mesh: d.mesh,
        repeats: d.repeats,
    }
}
fn shell(
    d: &mut Fixture<'_, '_>,
    p: &facade_plan::Room,
    coat: &str,
    look: &Interior,
    tiled: bool,
    density: f64,
) -> Result<(), String> {
    let (x0, x1, floor, head, back) = (p.x0, p.x1, p.floor, p.head, -p.depth);
    let dado = floor + look.dado_height.min(head - floor - 0.3);
    if tiled {
        let root = d.pen.clone();
        let mut frame = root.matrix;
        frame.0 = [
            root.matrix.0[0],
            root.matrix.0[2],
            root.matrix.0[1].map(|v| -v),
            root.world([x0, 0., floor]),
        ];
        let pen = root.reframed(frame, &format!("{}/room-floor", root.part))?;
        let ceramic = "$kowloon/wall-ceramic";
        let bed = "$kowloon/wall-tile-bed";
        let recipe = tile::Recipe {
            bounds: [0., 0., x1 - x0, p.depth],
            tiles: tile::Settings {
                seed: finish_seed(&format!("{}/floor-tiles", p.key)),
                share: 1.,
                wear: 0.45,
                grime: 0.98,
                size: Some(0.28),
                aspect: Some(1.),
            },
            materials: tile::Materials {
                ceramic: ceramic.into(),
                bed: bed.into(),
                grime: "$kowloon/wall-tile-grime".into(),
                repeat: d.repeat(ceramic),
                bed_repeat: d.repeat(bed),
                bed_roughness: 0.96,
                roughness: 0.82,
            },
            bed_tint: [0.5, 0.47, 0.42],
            density,
        };
        tile::panel(
            &mut on(d, pen),
            "concrete",
            &recipe,
            &format!("{}/floor", p.key),
        )?;
    } else {
        d.face(
            "concrete",
            &[
                [x0, 0., floor],
                [x1, 0., floor],
                [x1, back, floor],
                [x0, back, floor],
            ],
            [0., 0., 1.],
            look.floor,
        )?;
    }
    d.face(
        "concrete",
        &[
            [x0, 0., head],
            [x1, 0., head],
            [x1, back, head],
            [x0, back, head],
        ],
        [0., 0., -1.],
        look.ceiling,
    )?;
    for (z0, z1, wash) in [(floor, dado, look.dado), (dado, head, look.wall)] {
        d.face(
            coat,
            &[[x0, 0., z0], [x0, back, z0], [x0, back, z1], [x0, 0., z1]],
            [1., 0., 0.],
            shade(wash, 0.94),
        )?;
        d.face(
            coat,
            &[[x1, 0., z0], [x1, back, z0], [x1, back, z1], [x1, 0., z1]],
            [-1., 0., 0.],
            shade(wash, 0.94),
        )?;
        d.face(
            coat,
            &[
                [x0, back, z0],
                [x1, back, z0],
                [x1, back, z1],
                [x0, back, z1],
            ],
            [0., 1., 0.],
            wash,
        )?;
    }
    Ok(())
}
fn shopfront(
    d: &mut Fixture<'_, '_>,
    r: &Request<'_>,
    rng: &mut SeededRng,
) -> Result<Vec<Proposal>, String> {
    let p = r.plan;
    let (x0, x1, floor, head) = (p.x0, p.x1, p.floor, p.head);
    let selected = if rng.chance(0.5) {
        r.lot.style.shutter
    } else {
        srgb(rng.pick(&[
            "#8f969a", "#4a6f95", "#4f7a5f", "#7a4038", "#d8d2c0", "#5a6268", "#9a8a5a",
        ]))
    };
    let paint = r.shutter_color.unwrap_or(selected);
    rng.float(0., 0.3);
    let curtain = head - 0.18;
    shutters::roller_shutter(
        &d.pen,
        d.mesh,
        [x0, x1, curtain, head],
        paint,
        &r.kit.settings().weathering,
    )?;
    let pen = d.pen.with_weathering(
        d.mesh,
        r.kit.settings().weathering.clone(),
        metal::Kind::Frame,
        None,
        vec!["paint".into(), "metal".into()],
    )?;
    on(d, pen).cuboid(
        "metal",
        [x0, 0., floor - 0.02],
        [x1, 0.025, floor],
        shade(srgb("#a4a9aa"), 0.7),
        "bd",
    )?;
    if rng.chance(0.5) {
        let steel = shade(srgb("#a4a9aa"), 0.6);
        rng.float(0.1, 0.6);
        let left = if rng.chance(0.5) {
            x0 + 0.01
        } else {
            x1 - 0.27
        };
        let pen = d.pen.moved([0., -0.05, 0.])?;
        windows::security_bars_sized(
            &mut on(d, pen),
            [left, floor + 0.01, 0.26, curtain - 0.1 - floor],
            windows::Guard::Diamond,
            steel,
            Some(&r.kit.settings().weathering),
        )?;
    }
    let height = (r.lot.ground - head - 0.22 - 0.06 - 0.12)
        .min(0.62)
        .min((x1 - x0) / 4.);
    if height >= 0.28 {
        Ok(vec![signs::wall_board(
            &d.pen,
            WallRect {
                x0: (x0 + x1) / 2. - height * 2.,
                z0: head + 0.22 + 0.06,
                width: height * 4.,
                height,
            },
            r.kit,
            rng,
        )?])
    } else {
        Ok(vec![])
    }
}
pub fn draw(d: &mut Fixture<'_, '_>, r: &Request<'_>) -> Result<Vec<Proposal>, String> {
    let p = r.plan;
    if [p.x0, p.x1, p.floor, p.head, p.depth]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1000.)
        || p.floor < 0.02
        || p.head > r.lot.ground - 0.5
        || r.coat.is_empty()
        || r.coat.len() > 256
        || r.shutter_color
            .is_some_and(|c| c.iter().any(|v| !v.is_finite() || *v < 0.))
    {
        return Err(
            "Shop shell requires finite bounds beneath its ground story and a valid coat.".into(),
        );
    }
    let room = Room {
        width: p.x1 - p.x0,
        depth: p.depth,
        height: p.head - p.floor,
        kit: r.kit,
        key: &p.key,
        assets: r.assets.room,
    };
    room.validate()?;
    d.mesh.check_work(0)?;
    let root = d.pen.clone();
    let mut rng = r.kit.rng(&format!("{}/trade", p.key))?;
    let trade = r.trade.unwrap_or_else(|| select(&mut rng));
    let tiled = matches!(trade, Trade::Grocer | Trade::Greengrocer | Trade::Roast)
        && d.mesh.has_source_material("$kowloon/wall-ceramic");
    let pen = root
        .into_part(&format!("shop-{}", trade.name()))?
        .moved([p.x0, 0., p.floor])?
        .facing([0.; 3], [1., 0., 0.], [0., -1., 0.])?;
    let room_seed = finish_seed(&format!("{}/room-patina", p.key));
    let room_size = [room.width, room.depth, room.height];
    let shell_pen = root.with_room_patina_frame(&pen, room_size, room_seed)?;
    shell(
        &mut on(d, shell_pen),
        p,
        r.coat,
        &trade.interior(&mut rng),
        tiled,
        r.kit.settings().weathering.texture_density.clamp(16., 128.),
    )?;
    let signs = shopfront(
        &mut on(d, root.into_part("openings")?),
        r,
        &mut r.kit.rng(&format!("{}/front", p.key))?,
    )?;
    let pen = pen.with_room_patina_frame(&pen, room_size, room_seed)?;
    let inside = &mut on(d, pen);
    match trade {
        Trade::Grocer => grocer::furnish(inside, &room),
        Trade::Greengrocer => greengrocer::furnish(inside, &room),
        Trade::Roast => roast::furnish(inside, &room, r.assets.restaurant),
        Trade::Hardware => hardware::furnish(inside, &room),
        Trade::Herbalist => herbalist::furnish(inside, &room),
        Trade::ReferenceProvisions => provisions::furnish(inside, &room),
    }?;
    super::room_details::services(inside, &room)?;
    if tiled {
        super::room_details::walls(inside, &room, trade == Trade::Roast)?;
    }
    if let (Some(available), Some(trade)) = (r.frontage, trade.display()) {
        let pen = root.moved([p.x0, 0., 0.])?;
        frontage::draw(
            &mut on(d, pen),
            room.width,
            trade,
            r.kit,
            &p.key,
            &|x0, x1, depth, height| available(p.x0 + x0, p.x0 + x1, depth, height),
            &display::Settings {
                timber: &r.kit.settings().timber,
                props: r.assets.room.grocer_props,
                apples: r.assets.room.market_apple,
            },
        )?;
    }
    Ok(signs)
}
