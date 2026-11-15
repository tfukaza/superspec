//! Owned surface fields and portable texture sampling. Geometry height and raster bump are distinct.
use super::color::{LINEAR, srgb};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use superspec_geometry::{
    image::Image,
    math::{V3, hypot, length},
};
use superspec_types::material::Material;

pub type Uv = [f64; 2];
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Coat {
    pub material: String,
    pub uv: Uv,
    pub opacity: f64,
    pub tint: Option<V3>,
    pub roughness: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Layer {
    pub material: String,
    pub weight: f64,
    pub uv: Uv,
    pub tint: Option<V3>,
    pub roughness: Option<f64>,
    #[serde(default)]
    pub coatings: Vec<Coat>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub height: f64,
    pub layers: Vec<Layer>,
    pub bump: Option<f64>,
    pub roughness: Option<f64>,
    pub height_weight: Option<f64>,
    pub layer_elevation_weight: Option<f64>,
}

/// Periodic lower-left UV sampling; color texels are decoded before interpolation, alpha is linear.
pub fn image_sample(image: &Image, uv: Uv, color: bool) -> Result<[f64; 4], String> {
    if uv.iter().any(|v| !v.is_finite()) {
        return Err("Texture UVs must be finite.".into());
    }
    if image.width == 0
        || image.height == 0
        || image
            .width
            .checked_mul(image.height)
            .and_then(|n| n.checked_mul(4))
            != Some(image.rgba.len())
    {
        return Err("Image requires complete RGBA pixels.".into());
    }
    let x = uv[0] * image.width as f64 - 0.5;
    let y = (1. - uv[1]) * image.height as f64 - 0.5;
    if !x.is_finite() || !y.is_finite() {
        return Err("Texture coordinate exceeds the finite sampling range.".into());
    }
    let ix = x.floor();
    let iy = y.floor();
    let fx = x - ix;
    let fy = y - iy;
    let wrap =
        |value: f64, size: usize| ((value % size as f64 + size as f64) % size as f64) as usize;
    let left = wrap(ix, image.width) * 4;
    let right = wrap(ix + 1., image.width) * 4;
    let top = wrap(iy, image.height) * image.width * 4;
    let bottom = wrap(iy + 1., image.height) * image.width * 4;
    let weights = [
        (1. - fx) * (1. - fy),
        fx * (1. - fy),
        (1. - fx) * fy,
        fx * fy,
    ];
    let offsets = [top + left, top + right, bottom + left, bottom + right];
    Ok(std::array::from_fn(|channel| {
        let values = offsets.map(|offset| {
            if color && channel < 3 {
                LINEAR[image.rgba[offset + channel] as usize]
            } else {
                image.rgba[offset + channel] as f64 / 255.
            }
        });
        weights[0] * values[0]
            + weights[1] * values[1]
            + weights[2] * values[2]
            + weights[3] * values[3]
    }))
}

pub struct Inputs<'a> {
    pub materials: &'a BTreeMap<String, Material>,
    pub images: &'a BTreeMap<String, Image>,
}
impl Inputs<'_> {
    fn material(&self, id: &str) -> Result<&Material, String> {
        self.materials
            .get(id)
            .ok_or_else(|| format!("Unknown surface material {id}."))
    }
    fn image(&self, id: Option<&str>) -> Result<Option<&Image>, String> {
        id.map(|id| {
            self.images
                .get(id)
                .ok_or_else(|| format!("Missing surface image {id}."))
        })
        .transpose()
    }
    /// Adds selected-plate or normalized blended height-map relief in meters.
    pub fn height(
        &self,
        sample: &Sample,
        selected: Option<usize>,
        elevations: Option<&[f64]>,
    ) -> Result<f64, String> {
        let total = coverage(sample)?;
        if selected.is_some_and(|i| i >= sample.layers.len()) {
            return Err("Invalid selected surface layer.".into());
        }
        let mut height = sample.height;
        let weight = sample.layer_elevation_weight.unwrap_or(1.);
        if !normalized(weight) {
            return Err("Invalid layer elevation weight.".into());
        }
        if let Some(elevations) = elevations {
            let value = selected
                .and_then(|i| elevations.get(i))
                .ok_or("Selected plate elevation is required.")?;
            height += value * weight;
        }
        let height_weight = sample.height_weight.unwrap_or(1.);
        if !normalized(height_weight) {
            return Err("Invalid height-map coverage.".into());
        }
        for (index, layer) in sample.layers.iter().enumerate() {
            let weight =
                selected.map_or(layer.weight / total, |i| if i == index { 1. } else { 0. });
            if weight <= 0. {
                continue;
            }
            let material = self.material(&layer.material)?;
            if let Some(image) = self.image(material.height_texture.as_deref())? {
                let value = image_sample(image, layer.uv, false)?[0];
                height += height_weight
                    * weight
                    * (value * material.height_scale.unwrap_or(0.01)
                        + material.height_offset.unwrap_or(0.));
            }
        }
        if !height.is_finite() {
            return Err("Surface height must be finite.".into());
        }
        Ok(height)
    }
}
fn normalized(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn coverage(sample: &Sample) -> Result<f64, String> {
    if sample.layers.is_empty()
        || sample.layers.len() > 16
        || sample
            .layers
            .iter()
            .any(|l| !l.weight.is_finite() || l.weight < 0.)
    {
        return Err("Surface requires bounded nonnegative material coverage.".into());
    }
    let total: f64 = sample.layers.iter().map(|l| l.weight).sum();
    if total <= 0. || !total.is_finite() {
        return Err("Surface requires finite positive material coverage.".into());
    }
    Ok(total)
}

/// Charges retained capacities, including owned source identities; long AML IDs are legitimate inputs.
pub(crate) fn sample_storage(sample: &Sample) -> Result<usize, String> {
    let mut bytes = sample
        .layers
        .capacity()
        .checked_mul(256)
        .and_then(|n| n.checked_add(256));
    for layer in &sample.layers {
        bytes = bytes
            .and_then(|n| n.checked_add(layer.material.capacity()))
            .and_then(|n| {
                layer
                    .coatings
                    .capacity()
                    .checked_mul(256)
                    .and_then(|coats| n.checked_add(coats))
            });
        for coat in &layer.coatings {
            bytes = bytes.and_then(|n| n.checked_add(coat.material.capacity()));
        }
    }
    bytes.ok_or_else(|| "Raster sample storage overflow.".into())
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RasterSettings {
    pub bounds: [f64; 4],
    pub texels_per_meter: f64,
    pub preserve: bool,
    pub repeat: bool,
    pub uv_coordinates: bool,
    pub bump: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    pub bounds: [f64; 4],
    padding: usize,
}
impl Layout {
    /// Rechecks public transport fields before a stored raster is bound or encoded.
    pub fn validate(&self) -> Result<(), String> {
        if self.bounds.iter().any(|v| !v.is_finite())
            || !(self.bounds[2] - self.bounds[0]).is_finite()
            || !(self.bounds[3] - self.bounds[1]).is_finite()
            || self.bounds[2] <= self.bounds[0]
            || self.bounds[3] <= self.bounds[1]
            || self.width <= 2 * self.padding
            || self.height <= 2 * self.padding
            || self.width > 8192
            || self.height > 8192
            || self
                .width
                .checked_mul(self.height)
                .is_none_or(|pixels| pixels > 16_777_216)
        {
            return Err("Invalid raster layout dimensions or bounds.".into());
        }
        Ok(())
    }
    pub fn new(settings: RasterSettings) -> Result<Self, String> {
        let [u0, v0, u1, v1] = settings.bounds;
        if settings.bounds.iter().any(|v| !v.is_finite())
            || u1 <= u0
            || v1 <= v0
            || !settings.texels_per_meter.is_finite()
            || settings.texels_per_meter <= 0.
        {
            return Err("Raster requires finite ordered bounds and positive density.".into());
        }
        if settings.repeat && !settings.preserve {
            return Err("Repeating rasters require preserved geometry.".into());
        }
        let padding = if settings.repeat { 0 } else { 2 };
        let width = ((u1 - u0) * settings.texels_per_meter).ceil().max(1.) + (padding * 2) as f64;
        let height = ((v1 - v0) * settings.texels_per_meter).ceil().max(1.) + (padding * 2) as f64;
        if !width.is_finite()
            || !height.is_finite()
            || width > 8192.
            || height > 8192.
            || width * height > 16_777_216.
        {
            return Err("Raster exceeds the 16 Mi-texel image limit.".into());
        }
        Ok(Self {
            width: width as usize,
            height: height as usize,
            bounds: settings.bounds,
            padding,
        })
    }
    pub fn uv(&self, [u, v]: Uv) -> Uv {
        let [u0, v0, u1, v1] = self.bounds;
        [
            (self.padding as f64 + (u - u0) / (u1 - u0) * (self.width - 2 * self.padding) as f64)
                / self.width as f64,
            (self.padding as f64 + (v - v0) / (v1 - v0) * (self.height - 2 * self.padding) as f64)
                / self.height as f64,
        ]
    }
}
pub struct RasterLayer {
    pub source: String,
    pub color: Image,
    pub metallic_roughness: Image,
    pub normal: Option<Image>,
}
pub struct Raster {
    pub layout: Layout,
    pub layers: Vec<RasterLayer>,
}
type Field<'a> = dyn Fn(f64, f64) -> Result<Sample, String> + 'a;

/// The inverse transfer thresholds are the first IEEE values producing each sRGB byte in the reference.
/// This preserves byte rounding without target-dependent powers or approximation near transitions.
pub fn color_byte(value: f64) -> Result<u8, String> {
    const THRESHOLDS: [u64; 255] = include!("srgb_thresholds.rs");
    if !value.is_finite() {
        return Err("Nonfinite raster color.".into());
    }
    Ok(THRESHOLDS.partition_point(|bits| value >= f64::from_bits(*bits)) as u8)
}
fn byte(value: f64) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn basis(
    uv_at: &dyn Fn(f64, f64) -> Result<Uv, String>,
    bounds: [f64; 4],
) -> Result<[f64; 4], String> {
    let [left, bottom, right, top] = bounds;
    let u = (left + right) / 2.;
    let v = (bottom + top) / 2.;
    let epsilon = 0.01_f64.min((right - left) / 4.).min((top - bottom) / 4.);
    let origin = uv_at(u, v)?;
    let east = uv_at(u + epsilon, v)?;
    let north = uv_at(u, v + epsilon)?;
    let a = (east[0] - origin[0]) / epsilon;
    let b = (north[0] - origin[0]) / epsilon;
    let c = (east[1] - origin[1]) / epsilon;
    let d = (north[1] - origin[1]) / epsilon;
    let determinant = a * d - b * c;
    if [a, b, c, d, determinant].iter().any(|v| !v.is_finite()) || determinant.abs() < 1e-12 {
        return Err("Raster normals require nondegenerate affine UVs.".into());
    }
    for x in [left, u, right] {
        for y in [bottom, v, top] {
            let actual = uv_at(x, y)?;
            let expected = [
                origin[0] + a * (x - u) + b * (y - v),
                origin[1] + c * (x - u) + d * (y - v),
            ];
            if actual
                .iter()
                .zip(expected)
                .any(|(a, b)| !a.is_finite() || (*a - b).abs() > 1e-6)
            {
                return Err("Raster normals require affine UVs.".into());
            }
        }
    }
    let sign = determinant.signum();
    let norm = hypot(c, d);
    let tx = sign * d / norm;
    let ty = -sign * c / norm;
    Ok([tx, ty, -sign * ty, sign * tx])
}
struct Source<'a> {
    material: &'a Material,
    factor: V3,
    color: Option<&'a Image>,
    packed: Option<&'a Image>,
    normal: Option<&'a Image>,
    basis: [f64; 4],
}
#[derive(Clone, Copy, Default)]
struct Pixel {
    color: [f64; 4],
    packed: V3,
    normal: V3,
}
impl<'a> Source<'a> {
    fn new(
        inputs: &'a Inputs<'a>,
        id: &str,
        uv_at: &dyn Fn(f64, f64) -> Result<Uv, String>,
        bounds: [f64; 4],
    ) -> Result<Self, String> {
        let material = inputs.material(id)?;
        if material.color.len() != 7
            || !material.color.starts_with('#')
            || !material.color.as_bytes()[1..]
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            return Err("Raster material color must be six-digit sRGB.".into());
        }
        let normal = inputs.image(material.normal_texture.as_deref())?;
        Ok(Self {
            material,
            factor: srgb(&material.color),
            color: inputs.image(material.texture.as_deref())?,
            packed: inputs.image(material.metallic_roughness_texture.as_deref())?,
            normal,
            basis: if normal.is_some() {
                basis(uv_at, bounds)?
            } else {
                [1., 0., 0., 1.]
            },
        })
    }
    fn pixel(&self, uv: Uv, roughness: Option<f64>) -> Result<Pixel, String> {
        if uv.iter().any(|v| !v.is_finite()) {
            return Err("Raster UVs must be finite.".into());
        }
        let mut color = if let Some(image) = self.color {
            image_sample(image, uv, true)?
        } else {
            [1.; 4]
        };
        for (axis, factor) in self.factor.iter().enumerate() {
            color[axis] *= factor;
        }
        let alpha = self
            .material
            .alpha_cutoff
            .map_or(color[3], |cutoff| if color[3] >= cutoff { 1. } else { 0. });
        color[3] = alpha * self.material.opacity.unwrap_or(1.);
        let data = if let Some(image) = self.packed {
            image_sample(image, uv, false)?
        } else {
            [1.; 4]
        };
        let packed = [
            data[0],
            data[1] * roughness.unwrap_or(self.material.roughness),
            data[2] * self.material.metalness,
        ];
        let normal = if let Some(image) = self.normal {
            let data = image_sample(image, uv, false)?;
            let nx = (data[0] * 2. - 1.) * self.material.normal_scale.unwrap_or(1.);
            let ny = (data[1] * 2. - 1.) * self.material.normal_scale.unwrap_or(1.);
            let nz = (data[2] * 2. - 1.).max(0.01);
            let [tx, ty, bx, by] = self.basis;
            let x = tx * nx + bx * ny;
            let y = ty * nx + by * ny;
            let norm = length([x, y, nz]);
            [x / norm, y / norm, nz / norm]
        } else {
            [0., 0., 1.]
        };
        Ok(Pixel {
            color,
            packed,
            normal,
        })
    }
}

fn write_pixel(
    target: &mut RasterLayer,
    pixel: Pixel,
    offset: usize,
    bump: Uv,
) -> Result<(), String> {
    if pixel
        .color
        .iter()
        .chain(pixel.packed.iter())
        .chain(pixel.normal.iter())
        .any(|v| !v.is_finite())
    {
        return Err("Nonfinite raster output.".into());
    }
    for axis in 0..3 {
        target.color.rgba[offset + axis] = color_byte(pixel.color[axis])?;
        target.metallic_roughness.rgba[offset + axis] = byte(pixel.packed[axis]);
    }
    target.color.rgba[offset + 3] = byte(pixel.color[3]);
    target.metallic_roughness.rgba[offset + 3] = 255;
    if let Some(normal) = &mut target.normal {
        let [nx, ny, nz] = pixel.normal;
        let x = nx - bump[0] * nz;
        let y = ny - bump[1] * nz;
        let norm = length([x, y, nz]);
        if !norm.is_finite() || norm == 0. {
            return Err("Degenerate raster normal.".into());
        }
        for (axis, value) in [x, y, nz].into_iter().enumerate() {
            normal.rgba[offset + axis] = byte(value / norm * 0.5 + 0.5);
        }
        normal.rgba[offset + 3] = 255;
    }
    Ok(())
}

/// Bakes a coherent map set with an explicit byte budget. Fields return owned samples; no scratch aliases escape.
/// Cancellation is observed between rows and each 128 samples; single-thread Wasm hosts still terminate busy workers.
pub fn rasterize(
    settings: RasterSettings,
    field: &Field<'_>,
    inputs: &Inputs<'_>,
    maximum_bytes: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Raster, String> {
    let checkpoint = || {
        if cancelled() {
            Err("Raster cancelled.".to_owned())
        } else {
            Ok(())
        }
    };
    checkpoint()?;
    let layout = Layout::new(settings)?;
    let [u0, v0, u1, v1] = layout.bounds;
    let du = (u1 - u0) / (layout.width - 2 * layout.padding) as f64;
    let dv = (v1 - v0) / (layout.height - 2 * layout.padding) as f64;
    let reference = field((u0 + u1) / 2., (v0 + v1) / 2.)?;
    coverage(&reference)?;
    // Lettered signs have one ordered coating per glyph and shadow. Bound their total
    // independently of the tighter owned-byte admission for the raster and bump rows.
    if reference
        .layers
        .iter()
        .map(|l| l.coatings.len())
        .sum::<usize>()
        > 1024
    {
        return Err("Raster coating budget exceeded.".into());
    }
    let mut sources = vec![];
    for (index, layer) in reference.layers.iter().enumerate() {
        let uv_at = |u, v| {
            field(u, v)?
                .layers
                .get(index)
                .map(|l| l.uv)
                .ok_or("Raster layer shape changed.".into())
        };
        let base = Source::new(inputs, &layer.material, &uv_at, settings.bounds)?;
        let mut coats = vec![];
        for (coating, coat) in layer.coatings.iter().enumerate() {
            let uv_at = |u, v| {
                field(u, v)?
                    .layers
                    .get(index)
                    .and_then(|l| l.coatings.get(coating))
                    .map(|c| c.uv)
                    .ok_or("Raster coating shape changed.".into())
            };
            coats.push(Source::new(
                inputs,
                &coat.material,
                &uv_at,
                settings.bounds,
            )?);
        }
        sources.push((base, coats));
    }
    let has_normal = sources
        .iter()
        .any(|(base, coats)| base.normal.is_some() || coats.iter().any(|c| c.normal.is_some()));
    let output_count = if settings.preserve { 1 } else { sources.len() };
    let normals: Vec<_> = (0..output_count)
        .map(|i| {
            settings.bump
                || if settings.preserve {
                    has_normal
                } else {
                    sources[i].0.normal.is_some() || sources[i].1.iter().any(|c| c.normal.is_some())
                }
        })
        .collect();
    let maps = output_count * 2 + normals.iter().filter(|n| **n).count();
    let sample_bytes = sample_storage(&reference)?;
    drop(reference);
    let output_metadata = sources
        .iter()
        .take(output_count)
        .try_fold(
            output_count * std::mem::size_of::<RasterLayer>(),
            |bytes, (source, _)| bytes.checked_add(source.material.id.len()),
        )
        .ok_or("Raster output identity byte budget overflow.")?;
    let scratch = (layout.width + 2)
        .checked_mul(3)
        .and_then(|n| {
            sample_bytes
                .checked_add(8)
                .and_then(|bytes| n.checked_mul(bytes))
        })
        .ok_or("Raster scratch byte budget overflow.")?;
    let required = layout
        .width
        .checked_mul(layout.height)
        .and_then(|n| n.checked_mul(4 * maps))
        .and_then(|n| n.checked_add(output_metadata))
        .and_then(|n| n.checked_add(scratch));
    if required.is_none_or(|n| n > maximum_bytes) {
        return Err("Raster output and scratch byte budget exceeded before allocation.".into());
    }
    let output_bytes = layout.width * layout.height * 4 * maps + output_metadata;
    let per_sample_limit = (maximum_bytes - output_bytes) / ((layout.width + 2) * 3) - 8;
    let image = || Image {
        width: layout.width,
        height: layout.height,
        rgba: vec![0; layout.width * layout.height * 4],
        mime: "image/png",
    };
    let mut layers: Vec<_> = (0..output_count)
        .map(|i| RasterLayer {
            source: sources[i].0.material.id.clone(),
            color: image(),
            metallic_roughness: image(),
            normal: if normals[i] { Some(image()) } else { None },
        })
        .collect();
    let row_samples = |row: isize, mut samples: Vec<Sample>| -> Result<Vec<Sample>, String> {
        let v = v1 - (row as f64 - layout.padding as f64 + 0.5) * dv;
        samples.clear();
        samples.reserve(layout.width + 2);
        for column in 0..layout.width + 2 {
            if column % 128 == 0 {
                checkpoint()?;
            }
            let u = u0 + (column as f64 - layout.padding as f64 - 0.5) * du;
            let sample = field(u, v)?;
            if sample_storage(&sample)? > per_sample_limit {
                return Err("Raster row sample exceeds its owned-storage budget.".into());
            }
            if sample.bump.is_none_or(|v| !v.is_finite())
                || sample.layers.len() != sources.len()
                || sample
                    .layers
                    .iter()
                    .zip(&sources)
                    .any(|(l, s)| l.coatings.len() != s.1.len())
            {
                return Err("Invalid raster bump row shape or value.".into());
            }
            samples.push(sample);
        }
        Ok(samples)
    };
    let mut rows = if settings.bump {
        vec![
            row_samples(-1, vec![])?,
            row_samples(0, vec![])?,
            row_samples(1, vec![])?,
        ]
    } else {
        vec![]
    };
    for row in 0..layout.height {
        checkpoint()?;
        for column in 0..layout.width {
            if column % 128 == 0 {
                checkpoint()?;
            }
            let owned;
            let sample = if settings.bump {
                &rows[1][column + 1]
            } else {
                owned = field(
                    u0 + (column as f64 - layout.padding as f64 + 0.5) * du,
                    v1 - (row as f64 - layout.padding as f64 + 0.5) * dv,
                )?;
                &owned
            };
            let total = coverage(sample)?;
            if sample.layers.len() != sources.len()
                || sample.roughness.is_some_and(|v| !v.is_finite())
                || !sample.height.is_finite()
                || (settings.preserve && sample.height != 0.)
                || (settings.uv_coordinates
                    && (sample.height != 0.
                        || sample.layers.len() != 1
                        || sample.layers[0].weight != 1.))
            {
                return Err("Invalid raster layer count, displacement or coverage.".into());
            }
            let bump = if settings.bump {
                [
                    (rows[1][column + 2].bump.unwrap() - rows[1][column].bump.unwrap()) / (2. * du),
                    (rows[0][column + 1].bump.unwrap() - rows[2][column + 1].bump.unwrap())
                        / (2. * dv),
                ]
            } else {
                [0.; 2]
            };
            if bump.iter().any(|v| !v.is_finite()) {
                return Err("Invalid raster bump derivative.".into());
            }
            let offset = (row * layout.width + column) * 4;
            let mut mixed = Pixel::default();
            for (index, (base, coats)) in sources.iter().enumerate() {
                let layer = &sample.layers[index];
                if layer.material != base.material.id
                    || layer.uv.iter().any(|v| !v.is_finite())
                    || layer.roughness.is_some_and(|v| !normalized(v))
                {
                    return Err("Raster layers must keep stable material identities and finite UVs/factors.".into());
                }
                if settings.preserve && layer.weight == 0. {
                    continue;
                }
                let tint = layer.tint.unwrap_or([1.; 3]);
                if tint.iter().any(|v| !v.is_finite() || *v < 0.)
                    || layer.coatings.len() != coats.len()
                {
                    return Err("Invalid raster tint or coating count.".into());
                }
                let mut pixel = base.pixel(layer.uv, layer.roughness.or(sample.roughness))?;
                for (coat, source) in layer.coatings.iter().zip(coats) {
                    let coat_tint = coat.tint.unwrap_or([1.; 3]);
                    if coat.material != source.material.id
                        || coat.uv.iter().any(|v| !v.is_finite())
                        || coat_tint.iter().any(|v| !v.is_finite())
                        || !normalized(coat.opacity)
                        || coat.roughness.is_some_and(|v| !v.is_finite())
                    {
                        return Err("Invalid raster coating.".into());
                    }
                    if coat.opacity == 0. {
                        continue;
                    }
                    let top = source.pixel(coat.uv, coat.roughness)?;
                    let coverage = coat.opacity * top.color[3];
                    for (axis, tint) in coat_tint.into_iter().enumerate() {
                        pixel.color[axis] =
                            pixel.color[axis] * (1. - coverage) + top.color[axis] * tint * coverage;
                        pixel.packed[axis] =
                            pixel.packed[axis] * (1. - coverage) + top.packed[axis] * coverage;
                        if source.normal.is_some() {
                            pixel.normal[axis] =
                                pixel.normal[axis] * (1. - coverage) + top.normal[axis] * coverage;
                        }
                    }
                    let norm = length(pixel.normal);
                    pixel.normal = pixel.normal.map(|v| v / norm);
                }
                for (axis, tint) in tint.into_iter().enumerate() {
                    pixel.color[axis] *= tint;
                }
                if settings.preserve {
                    let weight = layer.weight / total;
                    for axis in 0..4 {
                        mixed.color[axis] += pixel.color[axis] * weight;
                    }
                    for axis in 0..3 {
                        mixed.packed[axis] += pixel.packed[axis] * weight;
                        mixed.normal[axis] += pixel.normal[axis] * weight;
                    }
                } else {
                    write_pixel(&mut layers[index], pixel, offset, bump)?;
                }
            }
            if settings.preserve {
                write_pixel(&mut layers[0], mixed, offset, bump)?;
            }
        }
        if settings.bump && row + 1 < layout.height {
            rows.rotate_left(1);
            let recycled = std::mem::take(&mut rows[2]);
            rows[2] = row_samples(row as isize + 2, recycled)?;
        }
    }
    checkpoint()?;
    Ok(Raster { layout, layers })
}
