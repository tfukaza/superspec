//! Evaluation against independent image annotations. Authored landmarks never supply acceptance evidence.
mod raster;
mod workflow;
use crate::Scene;
pub use raster::render;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use superspec_aml::architecture::Node;
use superspec_geometry::math::*;
pub use workflow::{Attempt, LoopOptions, LoopResult, RevisionRequest, run};
type V2 = [f64; 2];
pub fn silhouette(bytes: &[u8]) -> Result<Silhouette, String> {
    let image = superspec_geometry::image::decode(bytes)?;
    Ok(Silhouette {
        width: image.width,
        height: image.height,
        pixels: image
            .rgba
            .chunks_exact(4)
            .map(|p| u8::from(p[3] > 127 && (p[0] as u16 + p[1] as u16 + p[2] as u16) > 381))
            .collect(),
    })
}

pub fn render_png(
    scene: &Scene,
    reference: &Node,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, String> {
    superspec_geometry::image::encode_cancellable(
        &render(scene, reference, limit, cancelled)?.image,
        cancelled,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub reference_id: String,
    #[serde(default)]
    pub landmarks: Vec<Landmark>,
    #[serde(default)]
    pub masks: Vec<Mask>,
    pub image_filename: Option<String>,
    pub image_dimensions: Option<[usize; 2]>,
    pub silhouette: Option<Silhouette>,
    #[serde(default)]
    pub strokes: Vec<Stroke>,
}
#[derive(Deserialize)]
pub struct Landmark {
    pub id: String,
    pub position: V3,
    pub image: V2,
}
#[derive(Deserialize)]
pub struct Mask {
    pub points: Vec<V2>,
}
#[derive(Deserialize)]
pub struct Stroke {
    pub id: String,
    pub points: Vec<V2>,
}
#[derive(Deserialize)]
pub struct Silhouette {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thresholds {
    pub landmark_rms_pixels: f64,
    pub silhouette_io_u: f64,
    pub stroke_error_pixels: f64,
    pub minimum_verified_views: Option<usize>,
    pub minimum_camera_landmarks: usize,
}
impl Default for Thresholds {
    fn default() -> Self {
        Self {
            landmark_rms_pixels: 8.,
            silhouette_io_u: 0.8,
            stroke_error_pixels: 5.,
            minimum_verified_views: None,
            minimum_camera_landmarks: 4,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewReport {
    pub reference_id: String,
    pub size: [usize; 2],
    pub landmark_rms_pixels: Option<f64>,
    pub compared_landmarks: usize,
    pub clipped_landmarks: usize,
    pub silhouette_io_u: Option<f64>,
    pub stroke_error_pixels: Option<f64>,
    pub camera_calibrated: bool,
    pub verified: bool,
    pub passed: bool,
    pub issues: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub passed: bool,
    pub views: Vec<ViewReport>,
    pub verified_views: usize,
    pub navigation_passed: bool,
    pub geometry_passed: bool,
    pub issues: Vec<String>,
}
fn normalized(p: V2) -> bool {
    p.iter().all(|n| n.is_finite() && (0. ..=1.).contains(n))
}
fn masked(p: V2, masks: &[Mask]) -> bool {
    masks.iter().any(|m| {
        let mut inside = false;
        for (a, b) in m
            .points
            .iter()
            .zip(m.points.iter().cycle().skip(1))
            .take(m.points.len())
        {
            let cross = (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0]);
            if cross.abs() < 1e-10
                && p[0] >= a[0].min(b[0]) - 1e-10
                && p[0] <= a[0].max(b[0]) + 1e-10
                && p[1] >= a[1].min(b[1]) - 1e-10
                && p[1] <= a[1].max(b[1]) + 1e-10
            {
                return true;
            }
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                inside = !inside;
            }
        }
        inside
    })
}
fn calibrated(e: &Evidence, limit: usize) -> bool {
    if limit == 0 {
        return true;
    }
    let p = e
        .landmarks
        .iter()
        .filter(|l| !masked(l.image, &e.masks))
        .collect::<Vec<_>>();
    if p.len() < limit {
        return false;
    }
    let extent = p
        .iter()
        .map(|l| length(sub(l.position, p[0].position)))
        .fold(1e-9, f64::max);
    for i in 1..p.len() {
        for j in i + 1..p.len() {
            if length(cross(
                sub(p[i].position, p[0].position),
                sub(p[j].position, p[0].position),
            )) > 1e-3 * extent * extent
                && ((p[i].image[0] - p[0].image[0]) * (p[j].image[1] - p[0].image[1])
                    - (p[i].image[1] - p[0].image[1]) * (p[j].image[0] - p[0].image[0]))
                    .abs()
                    > 1e-6
            {
                return true;
            }
        }
    }
    false
}
fn field(r: &raster::Raster) -> Vec<f64> {
    let (w, h) = (r.image.width, r.image.height);
    let mut d = vec![1e6; w * h];
    if w < 3 || h < 3 {
        return d;
    }
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            if r.surfaces[i] >= 0
                && [i - 1, i + 1, i - w, i + w].iter().any(|&j| {
                    r.surfaces[j] != r.surfaces[i]
                        || (r.depth[j].is_finite()
                            && (r.depth[j] - r.depth[i]).abs() > 0.02 * r.depth[i].max(1.))
                })
            {
                d[i] = 0.;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x > 0 {
                d[i] = d[i].min(d[i - 1] + 1.);
            }
            if y > 0 {
                d[i] = d[i].min(d[i - w] + 1.);
                if x > 0 {
                    d[i] = d[i].min(d[i - w - 1] + 2f64.sqrt());
                }
                if x + 1 < w {
                    d[i] = d[i].min(d[i - w + 1] + 2f64.sqrt());
                }
            }
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if x + 1 < w {
                d[i] = d[i].min(d[i + 1] + 1.);
            }
            if y + 1 < h {
                d[i] = d[i].min(d[i + w] + 1.);
                if x > 0 {
                    d[i] = d[i].min(d[i + w - 1] + 2f64.sqrt());
                }
                if x + 1 < w {
                    d[i] = d[i].min(d[i + w + 1] + 2f64.sqrt());
                }
            }
        }
    }
    d
}
fn evaluate_view(
    scene: &Scene,
    reference: &Node,
    e: Option<&Evidence>,
    limits: &Thresholds,
    max_dimension: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<ViewReport, String> {
    let empty = Evidence {
        reference_id: reference.id.clone(),
        landmarks: vec![],
        masks: vec![],
        image_filename: None,
        image_dimensions: None,
        silhouette: None,
        strokes: vec![],
    };
    let e = e.unwrap_or(&empty);
    let landmark_ids = e
        .landmarks
        .iter()
        .map(|l| l.id.as_str())
        .collect::<BTreeSet<_>>();
    let stroke_ids = e
        .strokes
        .iter()
        .map(|s| s.id.as_str())
        .collect::<BTreeSet<_>>();
    if landmark_ids.len() != e.landmarks.len()
        || stroke_ids.len() != e.strokes.len()
        || landmark_ids.contains("")
        || stroke_ids.contains("")
    {
        return Err("Independent landmark and stroke IDs must be nonempty and unique.".into());
    }
    if e.landmarks.len() > 4096
        || e.strokes.len() > 4096
        || e.masks.len() > 1024
        || e.landmarks
            .iter()
            .any(|l| !normalized(l.image) || l.position.iter().any(|n| !n.is_finite()))
        || e.strokes
            .iter()
            .any(|s| s.points.len() > 4096 || s.points.iter().any(|&p| !normalized(p)))
        || e.masks.iter().any(|m| {
            m.points.len() < 3 || m.points.len() > 1024 || m.points.iter().any(|&p| !normalized(p))
        })
    {
        return Err("Invalid or oversized independent reference annotations.".into());
    }
    if e.image_filename
        .as_deref()
        .is_some_and(|name| Some(name) != reference.text("image"))
    {
        return Err("Reference image differs from independently supplied filename.".into());
    }
    let dimensions = [
        reference.number("width", 1.) as usize,
        reference.number("height", 1.) as usize,
    ];
    if e.image_dimensions.is_some_and(|d| d != dimensions) {
        return Err("Reference dimensions differ from independent evidence.".into());
    }
    if (!e.landmarks.is_empty() || !e.strokes.is_empty() || e.silhouette.is_some())
        && e.image_dimensions.is_none()
    {
        return Err(
            "Independent image dimensions are required for reconstruction acceptance.".into(),
        );
    }
    let camera = raster::Camera::new(reference)?;
    let mut errors = vec![];
    let mut clipped = 0;
    for l in &e.landmarks {
        if masked(l.image, &e.masks) {
            continue;
        }
        match camera.project(l.position) {
            Some(p) if normalized([p[0], p[1]]) => errors.push(
                ((p[0] - l.image[0]) * dimensions[0] as f64)
                    .hypot((p[1] - l.image[1]) * dimensions[1] as f64),
            ),
            _ => clipped += 1,
        }
    }
    let rms = (!errors.is_empty())
        .then(|| (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt());
    let raster = render(scene, reference, max_dimension, cancelled)?;
    let (w, h) = (raster.image.width, raster.image.height);
    if w.saturating_mul(h)
        .saturating_mul(e.masks.iter().map(|m| m.points.len()).sum::<usize>())
        > 250000000
    {
        return Err("Reference mask evaluation exceeds its work allowance.".into());
    }
    let mut iou = None;
    let mut observed_silhouette = false;
    if let Some(s) = &e.silhouette {
        if s.width == 0
            || s.height == 0
            || s.width.checked_mul(s.height) != Some(s.pixels.len())
            || s.pixels.len() > 20000000
        {
            return Err("Invalid independent silhouette dimensions.".into());
        }
        let (mut intersection, mut union, mut target_count, mut rendered_count) = (0, 0, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if masked(
                    [(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64],
                    &e.masks,
                ) {
                    continue;
                }
                let a = raster.surfaces[y * w + x] >= 0;
                let b = s.pixels[(((y as f64 + 0.5) * s.height as f64 / h as f64) as usize)
                    .min(s.height - 1)
                    * s.width
                    + (((x as f64 + 0.5) * s.width as f64 / w as f64) as usize).min(s.width - 1)]
                    != 0;
                intersection += usize::from(a && b);
                union += usize::from(a || b);
                target_count += usize::from(b);
                rendered_count += usize::from(a);
            }
        }
        iou = Some(if union == 0 {
            0.
        } else {
            intersection as f64 / union as f64
        });
        observed_silhouette = target_count > 0 && rendered_count > 0;
    }
    let mut stroke_errors = vec![];
    let distance = field(&raster);
    let mut structural = false;
    for s in &e.strokes {
        for segment in s.points.windows(2) {
            let (a, b) = (segment[0], segment[1]);
            let length = ((a[0] - b[0]) * w as f64).hypot((a[1] - b[1]) * h as f64);
            structural |= (a[0] - b[0]).hypot(a[1] - b[1]) > 0.001;
            let count = length.ceil().max(1.) as usize;
            for i in 0..=count {
                let t = i as f64 / count as f64;
                let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                if !masked(p, &e.masks) {
                    let x = (p[0] * w as f64).floor().min((w - 1) as f64) as usize;
                    let y = (p[1] * h as f64).floor().min((h - 1) as f64) as usize;
                    stroke_errors.push(distance[y * w + x].min(64.));
                }
            }
        }
    }
    stroke_errors.sort_by(f64::total_cmp);
    let keep = (stroke_errors.len() as f64 * 0.8).ceil() as usize;
    let stroke = (!stroke_errors.is_empty())
        .then(|| stroke_errors[..keep].iter().sum::<f64>() / keep as f64);
    let calibration = calibrated(e, limits.minimum_camera_landmarks) && clipped == 0;
    let observed = observed_silhouette || (structural && stroke.is_some());
    let verified = calibration && observed;
    let mut issues = vec![];
    if !calibration {
        issues.push("Independent camera calibration is incomplete.".into());
    }
    if !observed {
        issues.push("Independent silhouette or structural stroke evidence is required.".into());
    }
    if clipped > 0 {
        issues.push(format!(
            "{clipped} independent landmarks lie outside the view."
        ));
    }
    if rms.is_some_and(|v| v > limits.landmark_rms_pixels) {
        issues.push("Landmark RMS exceeds the threshold.".into());
    }
    if iou.is_some_and(|v| v < limits.silhouette_io_u) {
        issues.push("Silhouette IoU is below the threshold.".into());
    }
    if stroke.is_some_and(|v| v > limits.stroke_error_pixels) {
        issues.push("Structural stroke error exceeds the threshold.".into());
    }
    Ok(ViewReport {
        reference_id: reference.id.clone(),
        size: [w, h],
        landmark_rms_pixels: rms,
        compared_landmarks: errors.len(),
        clipped_landmarks: clipped,
        silhouette_io_u: iou,
        stroke_error_pixels: stroke,
        camera_calibrated: calibration,
        verified,
        passed: verified && issues.is_empty(),
        issues,
    })
}
pub fn evaluate(
    scene: &Scene,
    evidence: &[Evidence],
    limits: &Thresholds,
    max_dimension: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Report, String> {
    if !limits.landmark_rms_pixels.is_finite()
        || limits.landmark_rms_pixels < 0.
        || !limits.silhouette_io_u.is_finite()
        || !(0. ..=1.).contains(&limits.silhouette_io_u)
        || !limits.stroke_error_pixels.is_finite()
        || limits.stroke_error_pixels < 0.
    {
        return Err("Invalid reconstruction thresholds.".into());
    }
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Reconstruction needs architecture metadata.")?;
    let refs = context
        .metadata
        .iter()
        .filter(|n| n.kind == "reference")
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    for e in evidence {
        if !seen.insert(&e.reference_id) || !refs.iter().any(|r| r.id == e.reference_id) {
            return Err("Duplicate or unknown reconstruction reference ID.".into());
        }
    }
    let views = refs
        .iter()
        .map(|r| {
            evaluate_view(
                scene,
                r,
                evidence.iter().find(|e| e.reference_id == r.id),
                limits,
                max_dimension,
                cancelled,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let verified = views.iter().filter(|v| v.verified).count();
    let mut issues = views
        .iter()
        .flat_map(|v| v.issues.iter().map(|i| format!("{}: {i}", v.reference_id)))
        .collect::<Vec<_>>();
    if verified
        < limits
            .minimum_verified_views
            .unwrap_or(refs.len().min(2))
            .max(1)
    {
        issues.push("Insufficient independently verified views.".into());
    }
    let navigation = scene
        .manifest
        .validation
        .as_ref()
        .is_some_and(|v| v.success);
    if !navigation {
        issues.push("Navigation has not passed validation.".into());
    }
    if !scene.manifest.success {
        issues.push("Geometry compilation contains errors.".into());
    }
    Ok(Report {
        passed: issues.is_empty() && !views.is_empty(),
        views,
        verified_views: verified,
        navigation_passed: navigation,
        geometry_passed: scene.manifest.success,
        issues,
    })
}
