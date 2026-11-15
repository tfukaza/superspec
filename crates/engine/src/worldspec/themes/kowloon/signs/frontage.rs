//! Building callers get owned proposals; only their designated seeded stream advances.
use super::{
    copy::{self, Brief, Sector},
    mounts::{self, FlatKind, Proposal, Site, WallRect},
};
use crate::worldspec::themes::kowloon::{kit::Kit, pen::Pen};
use superspec_types::rng::SeededRng;
fn site(pen: Pen, kit: &Kit, key: String, shown: bool, elevation: f64) -> Site {
    Site {
        pen,
        key,
        shown,
        elevation,
        detail: kit.detail(),
        wash: None,
    }
}
fn trade(sectors: Vec<Sector>, rng: &mut SeededRng) -> Result<copy::Business, String> {
    copy::invent(
        rng,
        &Brief {
            sectors: Some(sectors),
            ..Default::default()
        },
    )
}
pub fn wall_board(
    pen: &Pen,
    rect: WallRect,
    kit: &Kit,
    rng: &mut SeededRng,
) -> Result<Proposal, String> {
    let mut design = SeededRng::from_u32(rng.next_u32());
    let place = pen
        .world([rect.x0, 0., rect.z0])
        .map(|v| -> String {
            let rounded = (v * 1000. + 0.5).floor();
            if rounded == 0. {
                "0".into()
            } else {
                ryu_js::Buffer::new().format(rounded).into()
            }
        })
        .join(",");
    let business = trade(
        vec![
            Sector::Food,
            Sector::Retail,
            Sector::Service,
            Sector::Industry,
        ],
        &mut design,
    )?;
    let inset = WallRect {
        x0: rect.x0 + 0.02,
        z0: rect.z0 + 0.02,
        width: rect.width - 0.04,
        height: rect.height - 0.04,
    };
    mounts::flat_sign(
        &site(
            pen.into_part("signage")?,
            kit,
            format!("fascia@{place}"),
            true,
            0.,
        ),
        inset,
        FlatKind::Fascia,
        0.13,
        &business,
        kit.settings(),
        &mut design,
    )
}
pub fn balcony_board(
    pen: &Pen,
    rect: WallRect,
    elevation: f64,
    kit: &Kit,
    key: &str,
    shown: bool,
) -> Result<Proposal, String> {
    let mut rng = kit.rng(key)?;
    let business = trade(
        vec![Sector::Medical, Sector::Service, Sector::Retail],
        &mut rng,
    )?;
    mounts::flat_sign(
        &site(pen.into_part("signage")?, kit, key.into(), shown, elevation),
        rect,
        FlatKind::Balcony,
        0.13,
        &business,
        kit.settings(),
        &mut rng,
    )
}
pub fn roof_hoarding(
    pen: &Pen,
    span: [f64; 2],
    heights: [f64; 2],
    kit: &Kit,
    key: &str,
    shown: bool,
) -> Result<Option<Proposal>, String> {
    let mut rng = kit.rng(key)?;
    let business = trade(
        vec![
            Sector::Food,
            Sector::Retail,
            Sector::Industry,
            Sector::Service,
        ],
        &mut rng,
    )?;
    mounts::roof_sign(
        &site(pen.clone(), kit, key.into(), shown, 0.),
        span,
        heights[0],
        heights[1],
        &business,
        kit.settings(),
        &mut rng,
    )
}
