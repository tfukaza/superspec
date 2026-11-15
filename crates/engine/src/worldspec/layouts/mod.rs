//! Seeded native layout producers. Geometry and publication use the ordinary AML compiler.
mod builder;
mod layouts;
mod placement;
use builder::Builder;
pub use placement::{Run, populate};
use serde::Deserialize;

pub const ARCHETYPES: [&str; 7] = [
    "three_lane",
    "ring",
    "stacked",
    "compound",
    "bridges",
    "sky_islands",
    "fightyard",
];

pub struct Options<'a> {
    pub seed: &'a str,
    pub depth: usize,
    pub minimum_routes: usize,
    pub archetype: Option<&'a str>,
    pub theme: Option<&'a str>,
}
pub struct Proposal {
    pub aml: String,
    pub archetype: String,
    pub theme: String,
    rng: Rng,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Theme {
    id: String,
    wall_thickness: f64,
    ceiling: [f64; 2],
    palette: Palette,
    decor: Decor,
    surface: Option<Finish>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Palette {
    roles: std::collections::BTreeMap<String, u32>,
    sky: u32,
    hemi_intensity: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decor {
    clutter: f64,
    centre_cover: f64,
    beams: f64,
    crate_height: [f64; 2],
}
#[derive(Deserialize)]
struct Finish {
    roughness: f64,
    metalness: f64,
}

struct Rng(superspec_types::rng::SeededRng);
impl Rng {
    fn new(seed: &str) -> Self {
        Self(superspec_types::rng::SeededRng::from_text(seed))
    }
    fn next(&mut self) -> f64 {
        self.0.next_f64()
    }
    fn int(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() * (hi - lo + 1) as f64).floor() as usize
    }
    fn float(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next() * (hi - lo)
    }
    fn bool(&mut self, p: f64) -> bool {
        self.next() < p
    }
    fn word(&mut self) -> String {
        let a = [
            "iron", "cold", "red", "deep", "salt", "ash", "glass", "dry", "black", "high", "low",
            "far",
        ];
        let b = [
            "gate", "yard", "vault", "span", "works", "reach", "hollow", "ridge", "well",
            "quarter", "basin", "line",
        ];
        format!(
            "{}-{}-{}",
            a[self.int(0, 11)],
            b[self.int(0, 11)],
            self.int(100, 999)
        )
    }
}

pub fn propose(options: &Options<'_>, cancelled: &dyn Fn() -> bool) -> Result<Proposal, String> {
    if !(1..=8).contains(&options.depth)
        || !(1..=16).contains(&options.minimum_routes)
        || options.seed.is_empty()
        || options.seed.len() > 4096
    {
        return Err("Generation requires a seed, depth 1..8 and route count 1..16.".into());
    }
    let mut rng = Rng::new(options.seed);
    let themes: Vec<Theme> =
        serde_json::from_str(include_str!("themes.json")).map_err(|e| e.to_string())?;
    let theme = if let Some(id) = options.theme {
        themes
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Unknown theme {id}."))?
    } else {
        &themes[rng.int(0, themes.len() - 1)]
    };
    let index = if let Some(id) = options.archetype {
        ARCHETYPES
            .iter()
            .position(|a| *a == id)
            .ok_or_else(|| format!("Unknown archetype {id}."))?
    } else if options.depth <= 1 {
        if rng.bool(0.5) { 0 } else { 6 }
    } else {
        let weights = [
            3,
            3,
            if options.depth >= 3 { 2 } else { 0 },
            if options.depth >= 3 { 3 } else { 0 },
            2,
            if options.depth >= 4 { 2 } else { 0 },
            2,
        ];
        let mut roll = rng.next() * weights.iter().sum::<usize>() as f64;
        let mut selected = 6;
        for (i, w) in weights.iter().enumerate() {
            roll -= *w as f64;
            if roll <= 0. {
                selected = i;
                break;
            }
        }
        selected
    };
    let id = format!("gen_v2_{}_{}", ARCHETYPES[index], rng.word());
    let scale = (0.95 + options.depth as f64 * 0.055 + rng.float(-0.06, 0.1)).min(1.55);
    let mut builder = Builder::new(&id, theme, &mut rng, scale);
    let ends = layouts::build(index, &mut builder, &mut rng);
    let aml = builder.finish(ends, options.minimum_routes, theme, &mut rng, cancelled)?;
    Ok(Proposal {
        aml,
        archetype: ARCHETYPES[index].into(),
        theme: theme.id.clone(),
        rng,
    })
}
