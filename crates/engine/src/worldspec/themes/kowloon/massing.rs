use super::palette::{LotStyle, lot_style};
use serde::{Deserialize, Serialize};
use superspec_types::rng::SeededRng;

pub const ROOF_RESERVE: f64 = 0.35;
const MIN_STORY: f64 = 2.4;
const MIN_GROUND: f64 = 2.6;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
}

/// Partitions the area outside a rectangular opening; ignores overlaps below the meter tolerance.
pub fn subtract_rect(rect: Rect, hole: Rect, sliver: f64) -> Vec<Rect> {
    let u0 = rect.u0.max(hole.u0);
    let u1 = rect.u1.min(hole.u1);
    let v0 = rect.v0.max(hole.v0);
    let v1 = rect.v1.min(hole.v1);
    if u1 - u0 < sliver || v1 - v0 < sliver {
        return vec![rect];
    }
    [
        Rect {
            u0: rect.u0,
            u1: u0,
            v0: rect.v0,
            v1: rect.v1,
        },
        Rect {
            u0: u1,
            u1: rect.u1,
            v0: rect.v0,
            v1: rect.v1,
        },
        Rect {
            u0,
            u1,
            v0: rect.v0,
            v1: v0,
        },
        Rect {
            u0,
            u1,
            v0: v1,
            v1: rect.v1,
        },
    ]
    .into_iter()
    .filter(|p| p.u1 - p.u0 > 1e-6 && p.v1 - p.v0 > 1e-6)
    .collect()
}

/// Local building coordinates: X across the front, Y into the site, Z up, in meters.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Block {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub z0: f64,
    pub z1: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lot {
    pub index: usize,
    pub kind: &'static str,
    pub x0: f64,
    pub x1: f64,
    pub stories: usize,
    pub ground: f64,
    pub story_height: f64,
    pub roof: f64,
    pub top: f64,
    pub front: f64,
    pub back: f64,
    pub style: LotStyle,
    pub key: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteBudget {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub stories: usize,
    pub base_height: f64,
}

pub fn default_stories(height: f64) -> usize {
    ((height - ROOF_RESERVE) / 3.).floor().max(1.) as usize
}

/// Ground and upper body remain separate so jetty/setback geometry retains its lower soffit.
pub fn lot_blocks(lot: &Lot, depth: f64) -> Vec<Block> {
    let mut blocks = vec![Block {
        x0: lot.x0,
        x1: lot.x1,
        y0: 0.,
        y1: depth,
        z0: 0.,
        z1: lot.ground,
    }];
    if lot.top - lot.ground >= 0.05 {
        blocks.push(Block {
            x0: lot.x0,
            x1: lot.x1,
            y0: -lot.front,
            y1: depth + lot.back,
            z0: lot.ground,
            z1: lot.top,
        });
    }
    blocks
}

struct Stack {
    stories: usize,
    ground: f64,
    story_height: f64,
    roof: f64,
}
fn stack_stories(stories: usize, shop: f64, roof: f64, base_height: f64) -> Stack {
    let lowest = base_height.max(MIN_GROUND);
    let deck = roof.max(lowest);
    let fitting = stories.min(1 + ((deck - lowest) / MIN_STORY + 1e-9).floor() as usize);
    if fitting <= 1 {
        return Stack {
            stories: 1,
            ground: deck,
            story_height: 0.,
            roof: deck,
        };
    }
    let ground = lowest.max(shop.min(deck - MIN_STORY * (fitting - 1) as f64));
    Stack {
        stories: fitting,
        ground,
        story_height: (deck - ground) / (fitting - 1) as f64,
        roof: deck,
    }
}

/// Plans tenements and service wells before geometry or appearance allocations.
/// Named style streams preserve layout when optional fixture details change.
pub fn plan_lots(site: SiteBudget, seed: &str) -> Result<Vec<Lot>, String> {
    if [site.width, site.depth, site.height, site.base_height]
        .iter()
        .any(|v| !v.is_finite())
        || !(3.0..=40.0).contains(&site.width)
        || !(3.0..=40.0).contains(&site.depth)
        || site.height > 40.
        || site.stories == 0
        || site.stories > 14
        || (site.height - ROOF_RESERVE) / (site.stories as f64) < 2.8
        || site.base_height < 0.
        || site.base_height > site.height - ROOF_RESERVE
    {
        return Err("Building budget requires 3–40 m width/depth, at most 40 m height, fitting stories, and a base wall below the roof.".into());
    }
    let mut rng = SeededRng::from_text(&format!("{seed}/lots"));
    let reference = (site.height - ROOF_RESERVE) / site.stories as f64;
    let count = if site.width < 8.5 {
        1
    } else {
        ((site.width / rng.float(5.2, 8.)).round() as usize).max(2)
    };
    let wells: Vec<_> = (0..count - 1)
        .map(|_| {
            let fits = site.depth >= 5. && site.width / count as f64 > 4.4;
            if fits && rng.chance(0.3) {
                rng.float(1., 1.5)
            } else {
                0.
            }
        })
        .collect();
    let weights: Vec<_> = (0..count).map(|_| rng.float(0.75, 1.3)).collect();
    let share = (site.width - wells.iter().sum::<f64>()) / weights.iter().sum::<f64>();
    let tallest = rng.integer(0, count - 1);
    let mut drops: Vec<usize> = (0..count)
        .map(|i| {
            if i == tallest {
                0
            } else {
                rng.pick(&[0, 0, 1, 1, 2])
                    .min(site.stories.saturating_sub(2))
            }
        })
        .collect();
    if count > 1 && site.stories >= 3 && !drops.iter().any(|d| *d > 0) {
        drops[(tallest + 1 + rng.integer(0, count - 2)) % count] = 1;
    }
    let slack = (site.height - ROOF_RESERVE - site.stories as f64 * 2.8).max(0.);
    let mut tenements = vec![];
    let mut x = 0.;
    for i in 0..count {
        let width = if i == count - 1 {
            site.width - x
        } else {
            weights[i] * share
        };
        let stories = site.stories - drops[i];
        let key = format!("lot-{i}");
        let style = lot_style(&mut SeededRng::from_text(&format!("{seed}/{key}/style")));
        let shop = (reference * rng.float(1., 1.22)).min(3.9);
        let (stack, top) = if i == tallest {
            let parapet = (ROOF_RESERVE + slack * 0.6).min(1.);
            let roof = site.height - parapet;
            (
                stack_stories(
                    stories,
                    shop.min(roof - (stories - 1) as f64 * 2.6),
                    roof,
                    site.base_height,
                ),
                site.height,
            )
        } else {
            let wanted = shop + (stories - 1) as f64 * reference * rng.float(0.94, 1.);
            let limit = site.height - 0.3;
            let parapet = rng.float(0.5, 1.).min(limit - wanted).max(0.3);
            let stack = stack_stories(stories, shop, wanted.min(limit - parapet), site.base_height);
            let top = site.height.min(stack.roof + parapet);
            (stack, top)
        };
        let roll = rng.next_f64();
        let jetty = if stack.stories > 1 && roll < 0.34 {
            rng.float(0.3, 0.55)
        } else {
            0.
        };
        let setback = if stack.stories > 2 && site.depth >= 6. && roll > 0.9 {
            rng.float(0.6, 1.2)
        } else {
            0.
        };
        tenements.push(Lot {
            index: 0,
            kind: "tenement",
            x0: x,
            x1: x + width,
            stories: stack.stories,
            ground: stack.ground,
            story_height: stack.story_height,
            roof: stack.roof,
            top,
            front: jetty - setback,
            back: if stack.stories > 1 && rng.chance(0.2) {
                0.3
            } else {
                0.
            },
            style,
            key,
        });
        x += width + wells.get(i).copied().unwrap_or(0.);
    }
    let mut lots = vec![];
    for i in 0..count {
        lots.push(tenements[i].clone());
        if wells.get(i).copied().unwrap_or(0.) == 0. {
            continue;
        }
        let left = &tenements[i];
        let right = &tenements[i + 1];
        let ground = left.ground.min(right.ground);
        let roof = left.roof.min(right.roof) - rng.float(0.4, 2.4);
        let rises = roof - ground > MIN_STORY;
        lots.push(Lot {
            index: 0,
            kind: "well",
            x0: left.x1,
            x1: right.x0,
            stories: if rises { 2 } else { 1 },
            ground,
            story_height: if rises { roof - ground } else { 0. },
            roof: if rises { roof } else { ground },
            top: if rises { roof + 0.3 } else { ground },
            front: -rng.float(1.2, (site.depth * 0.4).min(3.)),
            back: 0.,
            style: left.style.clone(),
            key: format!("well-{i}"),
        });
    }
    for (i, lot) in lots.iter_mut().enumerate() {
        lot.index = i;
    }
    Ok(lots)
}
