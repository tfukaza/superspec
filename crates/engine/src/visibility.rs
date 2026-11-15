//! Optional bounded optical observations from supported standing positions.
use crate::{Scene, validation::standing::Standing};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::math::*;
use superspec_geometry::obstructions::{Budget, Obstructions};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    spacing: f64,
    eye_heights: Vec<f64>,
    long_distance: f64,
    fov_degrees: f64,
    vertical_fov_degrees: f64,
    max_samples: usize,
    max_pair_queries: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            spacing: 1.,
            eye_heights: vec![1.6, 1.05],
            long_distance: 35.,
            fov_degrees: 100.,
            vertical_fov_degrees: 75.,
            max_samples: 15000,
            max_pair_queries: 30_000_000,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    id: String,
    feet: V3,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    id: String,
    from: String,
    to: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bounds {
    min: V3,
    max: V3,
}
impl Bounds {
    fn contains(&self, p: V3) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] - 0.03 && p[i] <= self.max[i] + 0.03)
    }
    fn valid(&self) -> bool {
        (0..3).all(|i| {
            self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i]
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Area {
    id: String,
    from: Bounds,
    to: Bounds,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    points: Vec<V3>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hold {
    id: String,
    bounds: Bounds,
    approaches_complete: bool,
    entries: Vec<Entry>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Config {
    settings: Settings,
    points: Vec<Point>,
    protected_pairs: Vec<Pair>,
    protected_areas: Vec<Area>,
    hold_regions: Vec<Hold>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    id: String,
    feet: V3,
    regions: Vec<String>,
    max_visible_distance: f64,
    farthest: Option<String>,
}

fn height(p: V3, t: [V3; 3]) -> Option<f64> {
    let determinant = orient(t[0], t[1], t[2]);
    if determinant.abs() < 1e-12 {
        return None;
    }
    let u = orient(p, t[1], t[2]) / determinant;
    let v = orient(t[0], p, t[2]) / determinant;
    let w = 1. - u - v;
    (u >= -1e-7 && v >= -1e-7 && w >= -1e-7).then(|| u * t[0][2] + v * t[1][2] + w * t[2][2])
}
fn raised(p: V3, z: f64) -> V3 {
    [p[0], p[1], p[2] + z]
}
fn bearing_arc(mut angles: Vec<f64>) -> f64 {
    if angles.len() < 2 {
        return 0.;
    }
    for a in &mut angles {
        *a = a.rem_euclid(360.);
    }
    angles.sort_by(f64::total_cmp);
    let gap = angles
        .windows(2)
        .map(|a| a[1] - a[0])
        .fold(angles[0] + 360. - angles[angles.len() - 1], f64::max);
    360. - gap
}
struct Queries<'a> {
    geometry: Obstructions,
    budget: Budget<'a>,
    count: usize,
    limit: usize,
    exhausted: bool,
    cancelled: &'a dyn Fn() -> bool,
}
impl Queries<'_> {
    fn visible(&mut self, a: V3, b: V3) -> Result<Option<bool>, String> {
        if (self.cancelled)() {
            return Err("Visibility cancelled.".into());
        }
        if self.exhausted || self.count >= self.limit {
            self.exhausted = true;
            return Ok(None);
        }
        self.count += 1;
        match self.geometry.blocked(a, b, &mut self.budget) {
            Ok(blocked) => Ok(Some(!blocked)),
            Err(e) if (self.cancelled)() => Err(e),
            Err(_) => {
                self.exhausted = true;
                Ok(None)
            }
        }
    }
}

pub fn audit(scene: &Scene, config: Config, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let settings = &config.settings;
    if ![
        settings.spacing,
        settings.long_distance,
        settings.fov_degrees,
        settings.vertical_fov_degrees,
    ]
    .iter()
    .all(|n| n.is_finite() && *n > 0.)
        || settings.fov_degrees > 360.
        || settings.vertical_fov_degrees > 180.
        || !(1..=50000).contains(&settings.max_samples)
        || !(1..=100_000_000).contains(&settings.max_pair_queries)
        || settings.eye_heights.is_empty()
        || settings.eye_heights.len() > 8
        || settings
            .eye_heights
            .iter()
            .any(|n| !n.is_finite() || *n <= 0.)
        || config.points.len() > 1000
        || config.protected_pairs.len() > 1000
        || config.protected_areas.len() > 100
        || config.hold_regions.len() > 100
    {
        return Err("Invalid visibility settings or collection bounds.".into());
    }
    let mut ids = BTreeSet::new();
    for id in config
        .points
        .iter()
        .map(|p| &p.id)
        .chain(config.protected_pairs.iter().map(|p| &p.id))
        .chain(config.protected_areas.iter().map(|p| &p.id))
        .chain(config.hold_regions.iter().map(|p| &p.id))
    {
        if id.is_empty() || id.len() > 4096 || id.starts_with("grid-") || !ids.insert(id) {
            return Err("Visibility identities must be unique and nonempty.".into());
        }
    }
    if config
        .points
        .iter()
        .any(|p| p.feet.iter().any(|n| !n.is_finite()))
        || config.protected_pairs.iter().any(|p| {
            !config.points.iter().any(|n| n.id == p.from)
                || !config.points.iter().any(|n| n.id == p.to)
        })
        || config
            .protected_areas
            .iter()
            .any(|a| !a.from.valid() || !a.to.valid())
        || config.hold_regions.iter().any(|h| {
            !h.bounds.valid()
                || h.entries.len() > 100
                || h.entries.iter().any(|e| {
                    e.id.is_empty()
                        || e.points.len() > 1000
                        || e.points.iter().flatten().any(|n| !n.is_finite())
                })
        })
    {
        return Err("Invalid visibility positions, bounds or entry points.".into());
    }
    let report = scene
        .manifest
        .validation
        .as_ref()
        .ok_or("Visibility requires native validation first.")?;
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Visibility requires architectural context.")?;
    let position = |i: usize| std::array::from_fn(|axis| scene.positions[i * 3 + axis] as f64);
    let mut physical = vec![];
    let mut optical = vec![];
    let mut unknown = BTreeSet::new();
    let mut dynamic = 0;
    for mesh in &scene.manifest.meshes {
        let object = context.objects.iter().find(|o| o.id == mesh.object_id);
        let material = scene
            .manifest
            .materials
            .iter()
            .find(|m| Some(&m.id) == mesh.material.as_ref());
        let moving = object.is_some_and(|o| {
            matches!(
                o.attributes.get("dynamic"),
                Some(superspec_aml::architecture::Value::Boolean(true))
            )
        });
        let transparent = material
            .is_some_and(|m| m.texture.is_some() || m.color.len() == 9 && !m.color.ends_with("ff"))
            || mesh
                .material
                .as_ref()
                .is_some_and(|id| id.to_lowercase().contains("glass"))
            || object.is_some_and(|o| o.kind == "water");
        if mesh.visible && transparent {
            unknown.insert(mesh.material.clone().unwrap_or_default());
        }
        for t in
            scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
        {
            let tri = [
                position(mesh.vertex_start + t[0] as usize),
                position(mesh.vertex_start + t[1] as usize),
                position(mesh.vertex_start + t[2] as usize),
            ];
            if scene.manifest.colliders.contains(&mesh.id) {
                physical.extend(tri.into_iter().flatten().map(|v| v as f32));
            }
            if mesh.visible {
                if moving {
                    dynamic += 1
                } else if !transparent {
                    optical.push(tri);
                }
            }
        }
    }
    let walking = Standing::new(&physical, report.agent)?;
    let mut physical_hash = Sha256::new();
    for value in &physical {
        physical_hash.update(value.to_bits().to_le_bytes());
    }
    let physical_hash = format!("{:x}", physical_hash.finalize());
    let mut query = Queries {
        geometry: Obstructions::new(&optical, &mut Budget::new(128_000_000, cancelled)?)?,
        budget: Budget::new(128_000_000, cancelled)?,
        count: 0,
        limit: settings.max_pair_queries,
        exhausted: false,
        cancelled,
    };
    let navigation = &report.navigation;
    let mut components = BTreeSet::new();
    for floor in &context.floors {
        for tri in &floor.triangles {
            let center = scale(
                tri.iter().fold([0.; 3], |p, i| add(p, floor.points[*i])),
                1. / 3.,
            );
            if let Some((polygon, _)) =
                navigation.nearest(center, report.agent.radius, report.agent.step + 0.1)
            {
                components.insert(navigation.components[polygon]);
            }
        }
    }
    let supported_component = |point: V3| {
        navigation
            .nearest(point, report.agent.radius, 0.15)
            .is_some_and(|(polygon, _)| components.contains(&navigation.components[polygon]))
    };
    let mut samples: Vec<Sample> = vec![];
    let mut seen = BTreeSet::new();
    let mut candidates = 0;
    let mut rejected = 0;
    let mut excluded = 0;
    for (polygon, vertices) in navigation.polygons.iter().enumerate() {
        if !components.contains(&navigation.components[polygon]) {
            excluded += vertices.len().saturating_sub(2);
            continue;
        }
        for indices in vertices[1..].windows(2) {
            let tri = [
                navigation.vertices[vertices[0]],
                navigation.vertices[indices[0]],
                navigation.vertices[indices[1]],
            ];
            let low = std::array::from_fn::<_, 2, _>(|i| {
                tri.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)
            });
            let high = std::array::from_fn::<_, 2, _>(|i| {
                tri.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max)
            });
            for x in (low[0] / settings.spacing).ceil() as i64
                ..=(high[0] / settings.spacing).floor() as i64
            {
                for y in (low[1] / settings.spacing).ceil() as i64
                    ..=(high[1] / settings.spacing).floor() as i64
                {
                    candidates += 1;
                    if cancelled() {
                        return Err("Visibility cancelled.".into());
                    }
                    if candidates > settings.max_samples * 100 {
                        return Err("Visibility grid candidate allowance exceeded.".into());
                    }
                    let p = [x as f64 * settings.spacing, y as f64 * settings.spacing, 0.];
                    let Some(z) = height(p, tri) else { continue };
                    if !seen.insert((x, y, (z * 100.).round() as i64)) {
                        continue;
                    }
                    let Ok(feet) = walking.resolve([p[0], p[1], z]) else {
                        rejected += 1;
                        continue;
                    };
                    if (feet[2] - z).abs() >= 0.1 {
                        rejected += 1;
                        continue;
                    }
                    if samples.len() >= settings.max_samples {
                        return Err("Visibility sample allowance exceeded.".into());
                    }
                    samples.push(Sample {
                        id: format!("grid-{}", samples.len()),
                        feet,
                        regions: config
                            .hold_regions
                            .iter()
                            .filter(|r| r.bounds.contains(feet))
                            .map(|r| r.id.clone())
                            .collect(),
                        max_visible_distance: 0.,
                        farthest: None,
                    });
                }
            }
        }
    }
    for p in &config.points {
        if !supported_component(p.feet) {
            return Err(format!("Unsupported visibility point {}.", p.id));
        }
        let feet = walking
            .resolve(p.feet)
            .map_err(|_| format!("Unsupported visibility point {}.", p.id))?;
        if (feet[2] - p.feet[2]).abs() >= 0.1 || samples.len() >= settings.max_samples {
            return Err("Visibility point exceeds support or sample allowance.".into());
        }
        samples.push(Sample {
            id: p.id.clone(),
            feet,
            regions: config
                .hold_regions
                .iter()
                .filter(|r| r.bounds.contains(feet))
                .map(|r| r.id.clone())
                .collect(),
            max_visible_distance: 0.,
            farthest: None,
        });
    }
    if samples.is_empty() {
        return Err("Visibility has no supported samples.".into());
    }
    let named: BTreeMap<_, _> = samples.iter().map(|s| (&s.id, s.feet)).collect();
    let mut pairs = vec![];
    for pair in &config.protected_pairs {
        let mut heights = vec![];
        for a in &settings.eye_heights {
            for b in &settings.eye_heights {
                if let Some(visible) =
                    query.visible(raised(named[&pair.from], *a), raised(named[&pair.to], *b))?
                {
                    heights.push(json!({"fromHeight":a,"toHeight":b,"visible":visible}));
                }
            }
        }
        pairs.push(json!({"id":pair.id,"visible":heights.iter().any(|h|h["visible"]==true),"heightPairs":heights}));
    }
    let mut areas = vec![];
    for area in &config.protected_areas {
        let from: Vec<_> = samples
            .iter()
            .filter(|s| area.from.contains(s.feet))
            .collect();
        let to: Vec<_> = samples
            .iter()
            .filter(|s| area.to.contains(s.feet))
            .collect();
        if from.is_empty() || to.is_empty() {
            return Err(format!(
                "Protected area {} has no supported samples.",
                area.id
            ));
        }
        let (mut tested, mut visible, mut maximum) = (0, 0, 0f64);
        'area: for a in &from {
            for b in &to {
                for ah in &settings.eye_heights {
                    for bh in &settings.eye_heights {
                        let (a, b) = (raised(a.feet, *ah), raised(b.feet, *bh));
                        let Some(hit) = query.visible(a, b)? else {
                            break 'area;
                        };
                        tested += 1;
                        if hit {
                            visible += 1;
                            maximum = maximum.max(length(sub(a, b)));
                        }
                    }
                }
            }
        }
        areas.push(json!({"id":area.id,"testedPairs":tested,"visiblePairs":visible,"maxVisibleDistance":maximum}));
    }
    let mut holds = vec![];
    for region in &config.hold_regions {
        let entries: Vec<_> = region
            .entries
            .iter()
            .map(|e| {
                (
                    &e.id,
                    e.points
                        .iter()
                        .map(|p| {
                            supported_component(*p)
                                .then(|| walking.resolve(*p).ok())
                                .flatten()
                                .filter(|q| (q[2] - p[2]).abs() < 0.1)
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        for sample in samples.iter().filter(|s| region.bounds.contains(s.feet)) {
            for eye in &settings.eye_heights {
                if query.exhausted {
                    break;
                }
                let mut azimuths = vec![];
                let mut elevations = vec![];
                let mut visible = vec![];
                let mut hidden = vec![];
                let mut unverified = vec![];
                let mut partial = vec![];
                let mut counts = vec![];
                for (id, points) in &entries {
                    let (mut hits, mut misses, mut unsupported) =
                        (0, 0, usize::from(points.is_empty()));
                    for p in points {
                        let Some(p) = p.filter(|p| {
                            (p[0] - sample.feet[0]).hypot(p[1] - sample.feet[1]) >= 0.5
                        }) else {
                            unsupported += 1;
                            continue;
                        };
                        match query.visible(raised(sample.feet, *eye), raised(p, *eye))? {
                            Some(true) => {
                                hits += 1;
                                let d = sub(p, sample.feet);
                                azimuths.push(d[1].atan2(d[0]).to_degrees());
                                elevations.push(d[2].atan2(d[0].hypot(d[1])).to_degrees());
                            }
                            Some(false) => misses += 1,
                            None => unsupported += 1,
                        }
                    }
                    if hits > 0 {
                        visible.push(*id);
                    } else if unsupported == 0 {
                        hidden.push(*id);
                    }
                    if unsupported > 0 {
                        unverified.push(*id);
                    }
                    if hits > 0 && misses > 0 {
                        partial.push(*id);
                    }
                    counts.push(
                        json!({"id":id,"visible":hits,"occluded":misses,"unsupported":unsupported}),
                    );
                }
                let horizontal = (!azimuths.is_empty()).then(|| bearing_arc(azimuths));
                let vertical = (!elevations.is_empty()).then(|| {
                    elevations.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                        - elevations.iter().copied().fold(f64::INFINITY, f64::min)
                });
                let status = if region.approaches_complete
                    && visible.len() >= 2
                    && hidden.is_empty()
                    && unverified.is_empty()
                {
                    if horizontal.unwrap() <= settings.fov_degrees + 1e-8
                        && vertical.unwrap() <= settings.vertical_fov_degrees + 1e-8
                    {
                        "single-sector-candidate"
                    } else {
                        "split-attention"
                    }
                } else {
                    "insufficient-evidence"
                };
                holds.push(json!({"region":region.id,"sample":sample.id,"feet":sample.feet,"eyeHeight":eye,"status":status,"horizontalSpan":horizontal,"verticalSpan":vertical,"visibleEntries":visible,"hiddenEntries":hidden,"unverifiedEntries":unverified,"partialEntries":partial,"entrySamples":counts}));
            }
        }
    }
    let mut best = BTreeMap::new();
    let mut pair_candidates = 0;
    'pairs: for i in 0..samples.len() {
        for j in i + 1..samples.len() {
            pair_candidates += 1;
            if pair_candidates > settings.max_pair_queries {
                query.exhausted = true;
                break 'pairs;
            }
            let distance = length(sub(samples[i].feet, samples[j].feet));
            if distance < settings.long_distance {
                continue;
            }
            for eye in &settings.eye_heights {
                let Some(hit) =
                    query.visible(raised(samples[i].feet, *eye), raised(samples[j].feet, *eye))?
                else {
                    break 'pairs;
                };
                if hit {
                    for (a, b) in [(i, j), (j, i)] {
                        if distance > samples[a].max_visible_distance {
                            samples[a].max_visible_distance = distance;
                            samples[a].farthest = Some(samples[b].id.clone());
                            best.insert(a, (b, distance, *eye));
                        }
                    }
                }
            }
        }
    }
    let mut order: Vec<_> = best.into_iter().collect();
    order.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    let mut retained = BTreeSet::new();
    let mut lines = vec![];
    for (a, (b, distance, eye)) in order {
        if !retained.insert((a.min(b), a.max(b))) {
            continue;
        }
        lines.push(json!({"from":raised(samples[a].feet,eye),"to":raised(samples[b].feet,eye),"distance":distance,"horizontalDistance":(samples[a].feet[0]-samples[b].feet[0]).hypot(samples[a].feet[1]-samples[b].feet[1]),"eyeHeight":eye}));
        if lines.len() == 100 {
            break;
        }
    }
    Ok(json!({
        "physicalHash": physical_hash,
        "settings": {
            "spacing": settings.spacing,
            "eyeHeights": settings.eye_heights,
            "longDistance": settings.long_distance,
            "fovDegrees": settings.fov_degrees,
            "verticalFovDegrees": settings.vertical_fov_degrees,
            "maxSamples": settings.max_samples,
            "maxPairQueries": settings.max_pair_queries
        },
        "samples": samples,
        "complete": !query.exhausted && unknown.is_empty() && dynamic == 0,
        "queryCount": query.count,
        "longLines": lines,
        "protectedPairs": pairs,
        "protectedAreas": areas,
        "holds": holds,
        "omissions": {
            "unknownMaterials": unknown,
            "dynamicFaces": dynamic,
            "excludedNavigationTriangles": excluded,
            "rejectedBodySamples": rejected,
            "pairBudgetReached": query.exhausted,
            "scope": "Static opaque render segments, authored-floor navigation components and standing-body admission. Separate samples do not establish traversal. Low eye offsets are optical studies. Hidden or unsupported entries remain unknown. Sampling can miss narrow openings."
        }
    }))
}
