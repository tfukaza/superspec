use super::{Image, image};
use ab_glyph_rasterizer::{Point, Rasterizer, point};
use serde_json::json;
use skrifa::{
    DrawMemory, Font, ScalerFont,
    raw::model::glyph::outline::{PathElement, PathSink},
};
use std::collections::{BTreeMap, BTreeSet};

const SIZE: usize = 4096;
const CELL: usize = 83;
const EM: usize = 63;
const ROWS: usize = 7;
const COLUMNS: usize = SIZE / CELL;
const SAMPLES: usize = 4;
const RASTER: usize = CELL * SAMPLES;
const LATIN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,-&'/():+#!";
const REFERENCES: &str = "國龍華興麵醫";
const WORDS: &str = include_str!("../../signs/words.json");
const ATLAS: &str = include_str!("../../signs/atlas.json");

#[derive(Clone, Copy)]
pub struct FontSpec {
    pub folder: &'static str,
    pub filename: &'static str,
    pub weight: Option<f32>,
}

const fn font(folder: &'static str, filename: &'static str, weight: Option<f32>) -> FontSpec {
    FontSpec {
        folder,
        filename,
        weight,
    }
}

const SANS: FontSpec = font("notosanshk", "NotoSansHK[wght].ttf", Some(900.));
const SERIF: FontSpec = font("notoserifhk", "NotoSerifHK[wght].ttf", Some(900.));
const WENKAI: FontSpec = font("lxgwwenkaitc", "LXGWWenKaiTC-Bold.ttf", None);

struct Face {
    id: &'static str,
    ideographs: &'static [FontSpec],
    latin: FontSpec,
    embolden: f32,
}

const FACES: [Face; 7] = [
    Face {
        id: "hei",
        ideographs: &[SANS],
        latin: font("oswald", "Oswald[wght].ttf", Some(700.)),
        embolden: 0.,
    },
    Face {
        id: "song",
        ideographs: &[SERIF],
        latin: SERIF,
        embolden: 0.,
    },
    Face {
        id: "kai",
        ideographs: &[WENKAI],
        latin: WENKAI,
        embolden: 0.014,
    },
    Face {
        id: "brush",
        ideographs: &[font("yujiboku", "YujiBoku-Regular.ttf", None), WENKAI],
        latin: font("yujisyuku", "YujiSyuku-Regular.ttf", None),
        embolden: 0.008,
    },
    Face {
        id: "heavy",
        ideographs: &[
            font("delagothicone", "DelaGothicOne-Regular.ttf", None),
            SANS,
        ],
        latin: font("anton", "Anton-Regular.ttf", None),
        embolden: 0.,
    },
    Face {
        id: "round",
        ideographs: &[font("huninn", "Huninn-Regular.ttf", None), SANS],
        latin: font("huninn", "Huninn-Regular.ttf", None),
        embolden: 0.022,
    },
    Face {
        id: "slim",
        ideographs: &[
            font("wdxllubrifonttc", "WDXLLubrifontTC-Regular.ttf", None),
            SANS,
        ],
        latin: font("bebasneue", "BebasNeue-Regular.ttf", None),
        embolden: 0.,
    },
];

fn advance(scaler: &ScalerFont, character: char) -> Result<f32, String> {
    let glyph = scaler
        .font()
        .charmap()
        .map_unicode(character)
        .ok_or_else(|| format!("Missing glyph {character:?}."))?;
    Ok(scaler.linear_glyph_metrics().h_advance(glyph))
}

#[derive(Clone, Copy)]
struct Bounds {
    min: [f32; 2],
    max: [f32; 2],
}

impl Bounds {
    fn center(self) -> [f32; 2] {
        std::array::from_fn(|i| (self.min[i] + self.max[i]) / 2.)
    }
}

fn outline(scaler: &ScalerFont, character: char) -> Result<(Vec<PathElement>, Bounds), String> {
    let glyph = scaler
        .font()
        .charmap()
        .map_unicode(character)
        .ok_or_else(|| format!("Missing glyph {character:?}."))?;
    let glyph = scaler
        .outline(glyph)
        .ok_or_else(|| format!("Missing outline for {character:?}."))?;
    let mut path = Vec::new();
    glyph
        .draw(DrawMemory::Scratch, &mut path)
        .map_err(|e| format!("{character:?}: {e}"))?;
    let bounds = path
        .iter()
        .filter_map(|segment| match *segment {
            PathElement::MoveTo { x, y }
            | PathElement::LineTo { x, y }
            | PathElement::QuadTo { x, y, .. }
            | PathElement::CurveTo { x, y, .. } => Some([x, y]),
            PathElement::Close => None,
        })
        .fold(None::<Bounds>, |bounds, p| {
            Some(match bounds {
                Some(bounds) => Bounds {
                    min: std::array::from_fn(|i| bounds.min[i].min(p[i])),
                    max: std::array::from_fn(|i| bounds.max[i].max(p[i])),
                },
                None => Bounds { min: p, max: p },
            })
        })
        .ok_or_else(|| format!("Glyph {character:?} has an empty outline."))?;
    Ok((path, bounds))
}

fn reference_center(scaler: &ScalerFont) -> Result<[f32; 2], String> {
    let mut count = 0.;
    let mut sum = [0.; 2];
    for character in REFERENCES.chars() {
        if scaler.font().charmap().map_unicode(character).is_some() {
            let (_, bounds) = outline(scaler, character)?;
            for (total, coordinate) in sum.iter_mut().zip(bounds.center()) {
                *total += coordinate;
            }
            count += 1.;
        }
    }
    if count == 0. {
        return Ok(outline(scaler, '一')?.1.center());
    }
    Ok(sum.map(|value| value / count))
}

struct RasterPath {
    raster: Rasterizer,
    origin: [f32; 2],
    current: Point,
    start: Point,
}

impl RasterPath {
    fn new(origin: [f32; 2]) -> Self {
        let start = point(0., 0.);
        Self {
            raster: Rasterizer::new(RASTER, RASTER),
            origin,
            current: start,
            start,
        }
    }

    fn coordinate(&self, x: f32, y: f32) -> Point {
        point(self.origin[0] + x, self.origin[1] - y)
    }
}

impl PathSink for RasterPath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current = self.coordinate(x, y);
        self.start = self.current;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let target = self.coordinate(x, y);
        self.raster.draw_line(self.current, target);
        self.current = target;
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let target = self.coordinate(x, y);
        self.raster
            .draw_quad(self.current, self.coordinate(cx, cy), target);
        self.current = target;
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let target = self.coordinate(x, y);
        self.raster.draw_cubic(
            self.current,
            self.coordinate(c0x, c0y),
            self.coordinate(c1x, c1y),
            target,
        );
        self.current = target;
    }
    fn close(&mut self) {
        self.raster.draw_line(self.current, self.start);
        self.current = self.start;
    }
}

fn glyph_mask(
    path: &[PathElement],
    origin: [f32; 2],
    bounds: Bounds,
    character: char,
    face: &str,
) -> Result<Vec<bool>, String> {
    let limits = [
        origin[0] + bounds.min[0],
        origin[0] + bounds.max[0],
        origin[1] - bounds.max[1],
        origin[1] - bounds.min[1],
    ];
    if limits
        .into_iter()
        .any(|v| v < 1. || v >= (RASTER - 1) as f32)
    {
        return Err(format!(
            "Glyph {character:?} of face {face} does not fit its cell."
        ));
    }
    let mut draw = RasterPath::new(origin);
    for segment in path {
        match *segment {
            PathElement::MoveTo { x, y } => draw.move_to(x, y),
            PathElement::LineTo { x, y } => draw.line_to(x, y),
            PathElement::QuadTo { cx0, cy0, x, y } => draw.quad_to(cx0, cy0, x, y),
            PathElement::CurveTo {
                cx0,
                cy0,
                cx1,
                cy1,
                x,
                y,
            } => draw.curve_to(cx0, cy0, cx1, cy1, x, y),
            PathElement::Close => draw.close(),
        }
    }
    let mut mask = vec![false; RASTER * RASTER];
    draw.raster
        .for_each_pixel(|index, alpha| mask[index] = alpha >= 0.5);
    if mask[..RASTER].contains(&true)
        || mask[mask.len() - RASTER..].contains(&true)
        || mask
            .chunks_exact(RASTER)
            .any(|row| row[0] || row[RASTER - 1])
    {
        return Err(format!(
            "Glyph {character:?} of face {face} reaches its cell boundary."
        ));
    }
    if !mask.contains(&true) {
        return Err(format!(
            "Glyph {character:?} of face {face} has no visible ink."
        ));
    }
    Ok(mask)
}

// Lower envelope of parabolas yields exact squared Euclidean distances in two passes.
fn distance_line(input: &[f32; RASTER], result: &mut [f32; RASTER]) {
    let mut sites = [0; RASTER];
    let mut edges = [0.; RASTER];
    let mut count = 0;
    for (q, &value) in input.iter().enumerate().filter(|(_, v)| v.is_finite()) {
        let edge = loop {
            if count == 0 {
                break f32::NEG_INFINITY;
            }
            let p = sites[count - 1];
            let intersection =
                (value + (q * q) as f32 - input[p] - (p * p) as f32) / (2 * (q - p)) as f32;
            if intersection > edges[count - 1] {
                break intersection;
            }
            count -= 1;
        };
        sites[count] = q;
        edges[count] = edge;
        count += 1;
    }
    if count == 0 {
        result.fill(f32::INFINITY);
        return;
    }
    let mut site = 0;
    for (q, pixel) in result.iter_mut().enumerate() {
        while site + 1 < count && (q as f32) > edges[site + 1] {
            site += 1;
        }
        let p = sites[site];
        *pixel = (q.abs_diff(p).pow(2)) as f32 + input[p];
    }
}

fn distances(mask: &[bool], target: bool) -> Vec<f32> {
    let mut result = vec![0.; RASTER * RASTER];
    let mut input = [0.; RASTER];
    let mut output = [0.; RASTER];
    for (row, destination) in mask
        .chunks_exact(RASTER)
        .zip(result.chunks_exact_mut(RASTER))
    {
        for (value, &inside) in input.iter_mut().zip(row) {
            *value = if inside == target { 0. } else { f32::INFINITY };
        }
        distance_line(&input, &mut output);
        destination.copy_from_slice(&output);
    }
    for column in 0..RASTER {
        for (value, row) in input.iter_mut().zip(result.chunks_exact(RASTER)) {
            *value = row[column];
        }
        distance_line(&input, &mut output);
        for (row, value) in result.chunks_exact_mut(RASTER).zip(output) {
            row[column] = value;
        }
    }
    result
}

fn distance_alpha(mask: &[bool], embolden: f32) -> Vec<u8> {
    let outside = distances(mask, true);
    let inside = distances(mask, false);
    let mut alpha = vec![0; CELL * CELL];
    for y in 1..CELL - 1 {
        for x in 1..CELL - 1 {
            let mut sum = 0.;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let index = (y * SAMPLES + sy) * RASTER + x * SAMPLES + sx;
                    sum += (inside[index].sqrt() - outside[index].sqrt()) / SAMPLES as f32;
                }
            }
            let signed = sum / (SAMPLES * SAMPLES) as f32 + embolden;
            let spread = if signed >= 0. { 3. } else { 8. };
            alpha[y * CELL + x] = ((0.5 + 0.5 * signed / spread) * 255.)
                .round()
                .clamp(0., 255.) as u8;
        }
    }
    alpha
}

/// Renders every cell and advance afresh from the fonts, with the ideographs in code point order.
pub fn bake(
    extra: &str,
    load_font: &mut dyn FnMut(FontSpec) -> Result<Font, String>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    render(extra, None, load_font)
}

/// Adds glyphs to the shipped atlas image. Its cells, their order and its advances are kept
/// exactly; only characters it lacks are rendered, in the cells after the last shipped one.
pub fn extend(
    extra: &str,
    shipped: &Image,
    load_font: &mut dyn FnMut(FontSpec) -> Result<Font, String>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    render(extra, Some(shipped), load_font)
}

fn render(
    extra: &str,
    shipped: Option<&Image>,
    load_font: &mut dyn FnMut(FontSpec) -> Result<Font, String>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let metrics: serde_json::Value = serde_json::from_str(ATLAS).map_err(|e| e.to_string())?;
    let current = metrics["characters"]
        .as_str()
        .ok_or("Missing shipped atlas character list.")?;
    let latin_order = format!(" {LATIN}");
    let ideographs: BTreeSet<_> = current
        .chars()
        .chain(
            WORDS
                .chars()
                .filter(|c| ('\u{3400}'..='\u{9fff}').contains(c)),
        )
        .chain(extra.chars())
        .filter(|&c| !LATIN.contains(c) && !c.is_whitespace())
        .collect();
    let (characters, kept): (String, usize) = if let Some(image) = shipped {
        let faces: Vec<_> = FACES.iter().map(|face| face.id).collect();
        if (image.width, image.height) != (SIZE, SIZE)
            || image.rgba.len() != SIZE * SIZE * 4
            || !current.starts_with(LATIN)
            || metrics["size"] != json!(SIZE)
            || metrics["cell"] != json!(CELL)
            || metrics["em"] != json!(EM)
            || metrics["columns"] != json!(COLUMNS)
            || metrics["rows"] != json!(ROWS)
            || metrics["faces"] != json!(faces)
            || metrics["latin"] != json!(latin_order)
        {
            return Err(
                "The shipped atlas has another layout and cannot be extended; bake it afresh."
                    .into(),
            );
        }
        (
            current
                .chars()
                .chain(ideographs.into_iter().filter(|&c| !current.contains(c)))
                .collect(),
            current.chars().count(),
        )
    } else {
        (LATIN.chars().chain(ideographs).collect(), 0)
    };
    if characters.chars().count() > COLUMNS * ROWS {
        return Err(format!(
            "The atlas can fit {} characters per face.",
            COLUMNS * ROWS
        ));
    }
    let em = (EM * SAMPLES) as f32;
    let middle = RASTER as f32 / 2.;
    let mut fonts = BTreeMap::new();
    for face in &FACES {
        for spec in face.ideographs.iter().copied().chain([face.latin]) {
            if let std::collections::btree_map::Entry::Vacant(entry) = fonts.entry(spec.filename) {
                entry.insert(load_font(spec)?);
            }
        }
    }
    let mut advance_table = BTreeMap::new();
    let mut rgba = match shipped {
        Some(image) => image.rgba.clone(),
        None => [255, 255, 255, 0].repeat(SIZE * SIZE),
    };
    for (face_index, face) in FACES.iter().enumerate() {
        let mut ideograph_fonts = Vec::new();
        for spec in face.ideographs {
            let scaler = ScalerFont::unhinted(fonts[spec.filename].clone(), Some(em));
            let center = reference_center(&scaler)?;
            ideograph_fonts.push((scaler, [middle - center[0], middle + center[1]]));
        }
        let latin_source = &fonts[face.latin.filename];
        let probe = ScalerFont::unhinted(latin_source.clone(), Some(em));
        let cap = outline(&probe, 'H')?.1;
        let cap_height = cap.max[1] - cap.min[1];
        let widest = LATIN
            .chars()
            .map(|c| advance(&probe, c))
            .try_fold(0_f32, |max, value| value.map(|v| max.max(v)))?;
        let scale = (0.72 * em / cap_height).min(0.98 * em / widest);
        let latin = ScalerFont::unhinted(latin_source.clone(), Some((em * scale).round().max(8.)));
        let cap_center = outline(&latin, 'H')?.1.center()[1];
        let advances = latin_order
            .chars()
            .map(|c| advance(&latin, c).map(|a| (f64::from(a / em) * 10_000.).round() / 10_000.))
            .collect::<Result<Vec<_>, _>>()?;
        advance_table.insert(face.id, advances);
        for (index, character) in characters.chars().enumerate().skip(kept) {
            let (scaler, origin) = if LATIN.contains(character) {
                (
                    &latin,
                    [
                        middle - advance(&latin, character)? / 2.,
                        middle + cap_center,
                    ],
                )
            } else {
                let (scaler, origin) = ideograph_fonts
                    .iter()
                    .find(|(scaler, _)| scaler.font().charmap().map_unicode(character).is_some())
                    .ok_or_else(|| format!("No font of face {} covers {character:?}.", face.id))?;
                (scaler, *origin)
            };
            let (outline, bounds) = outline(scaler, character)?;
            let mask = glyph_mask(&outline, origin, bounds, character, face.id)?;
            let alpha = distance_alpha(&mask, face.embolden * EM as f32);
            let column = index % COLUMNS * CELL;
            let row = (face_index * ROWS + index / COLUMNS) * CELL;
            for (y, line) in alpha.chunks_exact(CELL).enumerate() {
                let start = ((row + y) * SIZE + column) * 4 + 3;
                for (target, &alpha) in rgba[start..].iter_mut().step_by(4).take(CELL).zip(line) {
                    *target = alpha;
                }
            }
        }
    }
    let layout = json!({
        "size": SIZE,
        "cell": CELL,
        "em": EM,
        "columns": COLUMNS,
        "rows": ROWS,
        "faces": FACES.iter().map(|face| face.id).collect::<Vec<_>>(),
        "characters": characters,
        "latin": latin_order,
        "advances": if shipped.is_some() { metrics["advances"].clone() } else { json!(advance_table) },
    });
    // The atlas ships with every build, so spend the slower, smaller lossless encoding on it.
    let png = image::encode_small_cancellable(
        &Image {
            width: SIZE,
            height: SIZE,
            rgba,
            mime: "image/png",
        },
        &|| false,
    )?;
    let mut json = serde_json::to_vec_pretty(&layout).map_err(|e| e.to_string())?;
    json.push(b'\n');
    Ok((png, json))
}
