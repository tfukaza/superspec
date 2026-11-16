//! The occluder service: per-texel planes that say how a scene's geometry shades a receiver, for
//! material programs that read them with `external(name, fallback)`.
//!
//! For every texel of a receiver's atlas (the atlas a material bake of the same request lays
//! out, see [`Atlas::for_request`]) rays are cast against the scene's triangles:
//!
//! * [`SHADOW`]: the fraction of the sun's disk that is blocked, 0 lit to 1 shadowed. The disk
//!   has an angular radius ([`OcclusionSettings::sun_radius`]) and is sampled by one fixed
//!   pattern for every texel. A surface facing away from a direction counts as blocked there.
//!   Without a sun, or with one that casts no shadow, the plane is 0.
//! * [`OCCLUSION`]: ambient occlusion, 0 open to 1 closed: the cosine-weighted fraction of the
//!   hemisphere about the surface's face normal that meets geometry within
//!   [`OcclusionSettings::occlusion_distance`]. A hit counts in full, whatever its distance.
//! * [`SUN_EXPOSURE`]: what falls from the sky reaches, 0 sheltered to 1 open: the fraction of
//!   directions, cosine-distributed about world up (+Z), that leave the surface on its outer
//!   side and meet no geometry within [`OcclusionSettings::exposure_distance`]. It is the share
//!   of rain or sky light a texel receives relative to open level ground, so an open wall holds
//!   0.5 and a ceiling 0. Despite the name it does not read the sun: the name is the roadmap's.
//!
//! A ray starts on the texel's sample, lifted along the face normal by
//! [`OcclusionSettings::bias`] texel footprints, so the texel's own surface and surfaces lying
//! on it are not met at distance zero. Gutter texels repeat the nearest inner texel of their
//! chart; texels outside every chart hold NaN (no value: a program reads its fallback).
//!
//! Everything is a pure function of its inputs, computed with IEEE basic operations and the
//! shared deterministic `sin` and `cos`, so native and Wasm builds and any number of threads give
//! the same bytes. Directions are a fixed pattern turned per texel by a hash of the texel's
//! sample; there is no random state.
//!
//! A plane's [`OcclusionPlane::key`] is a hash of exactly what decides its values: the receiver's
//! layout, the triangles its rays can meet (in scene order), and the settings and light fields
//! the plane reads. The rays are cast against those triangles only, so the key is sound by
//! construction. `occlusion` and `sunExposure` read no light: moving the sun changes the key of
//! `shadow` alone. Shadow style, intensity and colour of the lighting record are never read.
use super::{
    Diagnostic, PlaneSource, Project, Receiver, Request, Session,
    project::{material_request, receivers},
    receiver::{Atlas, GUTTER, add, cross, dot, mul, sub, unit},
};
use crate::materialspec::program::Plane;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use superspec_geometry::math::{cos, sin};
use superspec_model::Scene;
use superspec_types::lighting::Lighting;

/// External input: the blocked fraction of the sun's disk.
pub const SHADOW: &str = "shadow";
/// External input: ambient occlusion within a distance.
pub const OCCLUSION: &str = "occlusion";
/// External input: the open fraction of the sky above, as rain or sky light arrives.
pub const SUN_EXPOSURE: &str = "sunExposure";
/// Every plane the service makes, in the order it makes them.
pub const PLANES: [&str; 3] = [SHADOW, OCCLUSION, SUN_EXPOSURE];
/// Part of every key; a change in what a plane's values mean bumps it.
pub const OCCLUSION_VERSION: u32 = 1;

/// Coordinates beyond this many meters are refused.
const MAX_COORDINATE: f64 = 1_000_000.;
/// Triangles one scene may hold.
const MAX_TRIANGLES: usize = 4_000_000;
/// Hits nearer than this, in meters, are the ray's own start.
const NEAR: f64 = 1e-6;
/// Slack, in meters, around the region a plane's rays can reach, above any rounding of the rays.
const SLACK: f64 = 1e-6;

type V3 = [f64; 3];
type Triangle = [V3; 3];

/// Quality and reach of the planes. Every field but `max_rays` and `threads` decides values and
/// is part of the keys of the planes that read it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OcclusionSettings {
    /// Angular radius of the sun's disk, radians; 0 gives a hard edge from one ray.
    pub sun_radius: f64,
    /// Rays per texel toward the sun's disk.
    pub shadow_samples: usize,
    /// How far toward the sun occluders are looked for, meters.
    pub shadow_distance: f64,
    /// Rays per texel for ambient occlusion.
    pub occlusion_samples: usize,
    /// How far around a texel ambient occluders are looked for, meters.
    pub occlusion_distance: f64,
    /// Directions per texel for sky exposure.
    pub exposure_samples: usize,
    /// How far above a texel shelter is looked for, meters.
    pub exposure_distance: f64,
    /// Lift of a ray's start along the face normal, in texel footprints.
    pub bias: f64,
    /// Most rays one call may cast, counted before any is cast as texels times samples.
    pub max_rays: u64,
    /// Worker threads for a receiver's rows; 0 chooses (one on Wasm). Never changes a value.
    #[serde(skip)]
    pub threads: usize,
}

impl Default for OcclusionSettings {
    fn default() -> Self {
        Self {
            sun_radius: 0.02,
            shadow_samples: 16,
            shadow_distance: 100.,
            occlusion_samples: 32,
            occlusion_distance: 0.5,
            exposure_samples: 32,
            exposure_distance: 20.,
            bias: 0.5,
            max_rays: 256 * 1024 * 1024,
            threads: 0,
        }
    }
}

impl OcclusionSettings {
    fn validate(&self) -> Result<(), String> {
        let samples = [
            ("shadowSamples", self.shadow_samples),
            ("occlusionSamples", self.occlusion_samples),
            ("exposureSamples", self.exposure_samples),
        ];
        for (name, count) in samples {
            if !(1..=1024).contains(&count) {
                return Err(format!("Occlusion setting {name} must be 1 to 1024."));
            }
        }
        let distances = [
            ("shadowDistance", self.shadow_distance),
            ("occlusionDistance", self.occlusion_distance),
            ("exposureDistance", self.exposure_distance),
        ];
        for (name, distance) in distances {
            if !(distance.is_finite() && distance > 0. && distance <= MAX_COORDINATE) {
                return Err(format!(
                    "Occlusion setting {name} must be above 0 and at most {MAX_COORDINATE} meters."
                ));
            }
        }
        if !(self.sun_radius.is_finite() && (0. ..=0.5).contains(&self.sun_radius)) {
            return Err("Occlusion setting sunRadius must be 0 to 0.5 radians.".into());
        }
        if !(self.bias.is_finite() && (0. ..=16.).contains(&self.bias)) {
            return Err("Occlusion setting bias must be 0 to 16 texel footprints.".into());
        }
        Ok(())
    }

    fn threads(&self) -> usize {
        if cfg!(target_arch = "wasm32") {
            1
        } else if self.threads == 0 {
            std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
        } else {
            self.threads.min(64)
        }
    }
}

/// One mesh of the scene that may shade a receiver.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OccluderMesh {
    pub id: String,
    /// Three numbers per vertex, meters.
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    /// Column-major object-to-world transform of `positions`; absent, they are world positions.
    pub object_to_world: Option<[f64; 16]>,
}

/// The triangles of a scene in world meters, in scene order. Both faces of a triangle block.
#[derive(Clone, Debug, Default)]
pub struct Occluders {
    triangles: Vec<Triangle>,
}

impl Occluders {
    pub fn new(meshes: &[OccluderMesh]) -> Result<Self, String> {
        let mut triangles = Vec::new();
        for mesh in meshes {
            let count = mesh.positions.len() / 3;
            if !mesh.positions.len().is_multiple_of(3)
                || !mesh.indices.len().is_multiple_of(3)
                || mesh.indices.iter().any(|index| *index as usize >= count)
            {
                return Err(format!(
                    "Occluder mesh {} has invalid triangle attributes.",
                    mesh.id
                ));
            }
            let point = |index: u32| -> V3 {
                let at = index as usize * 3;
                let p = [
                    mesh.positions[at],
                    mesh.positions[at + 1],
                    mesh.positions[at + 2],
                ];
                match &mesh.object_to_world {
                    None => p,
                    Some(m) => std::array::from_fn(|axis| {
                        m[axis] * p[0] + m[4 + axis] * p[1] + m[8 + axis] * p[2] + m[12 + axis]
                    }),
                }
            };
            for triangle in mesh.indices.chunks_exact(3) {
                triangles.push([point(triangle[0]), point(triangle[1]), point(triangle[2])]);
            }
        }
        Self::admitted(triangles)
    }

    /// Every visible mesh of a compiled scene.
    pub fn from_scene(scene: &Scene) -> Result<Self, String> {
        let mut triangles = Vec::new();
        for mesh in scene.manifest.meshes.iter().filter(|mesh| mesh.visible) {
            let point = |index: u32| -> V3 {
                let at = (mesh.vertex_start + index as usize) * 3;
                std::array::from_fn(|axis| f64::from(scene.positions[at + axis]))
            };
            for triangle in
                scene.indices[mesh.index_start..mesh.index_start + mesh.index_count].chunks_exact(3)
            {
                triangles.push([point(triangle[0]), point(triangle[1]), point(triangle[2])]);
            }
        }
        Self::admitted(triangles)
    }

    fn admitted(triangles: Vec<Triangle>) -> Result<Self, String> {
        if triangles.len() > MAX_TRIANGLES {
            return Err(format!(
                "Occluders hold {} triangles, above the allowance of {MAX_TRIANGLES}.",
                triangles.len()
            ));
        }
        if triangles
            .iter()
            .flatten()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > MAX_COORDINATE)
        {
            return Err("Occluder triangles need finite coordinates within 1000 km.".into());
        }
        Ok(Self { triangles })
    }

    pub fn triangles(&self) -> usize {
        self.triangles.len()
    }
}

/// One plane of one receiver.
#[derive(Clone, Debug)]
pub struct OcclusionPlane {
    /// The external input the plane supplies: one of [`PLANES`].
    pub name: String,
    /// Content hash of everything that decides the plane's values (see the module documentation).
    pub key: String,
    pub width: usize,
    pub height: usize,
    /// The encoded plane ([`Plane::encode_external`]); `None` when the caller holds this key.
    pub bytes: Option<Arc<[u8]>>,
    /// Rays cast for it (directions leaving through the surface's back are not cast).
    pub rays: u64,
    /// Triangles its rays could meet: the geometry in its key.
    pub occluders: usize,
}

impl OcclusionPlane {
    /// The asset name [`bind_occlusion`] binds the plane under.
    pub fn asset(&self) -> String {
        format!("occlusion/{}/{}", self.name, self.key)
    }
}

/// The planes of one receiver, in the order of [`PLANES`].
#[derive(Clone, Debug)]
pub struct ReceiverOcclusion {
    /// The receiver's mesh identity.
    pub id: String,
    pub planes: Vec<OcclusionPlane>,
}

/// What to bake: the light, the quality, which planes, and which the caller already has.
#[derive(Clone, Copy)]
pub struct OcclusionJob<'a> {
    pub lighting: &'a Lighting,
    pub settings: &'a OcclusionSettings,
    /// Names from [`PLANES`]; others are refused.
    pub names: &'a [&'a str],
    /// Whether the caller holds the plane of a key: such a plane is keyed but not baked.
    pub held: &'a dyn Fn(&str) -> bool,
}

/// Bakes the planes of `job` for each receiver over the atlas a material bake of `request` gives
/// it (only the request's sizing is read: resolution, mapping, domain, gravity, profile and texel
/// density). Receivers are part of the scene only if `occluders` holds their triangles too.
pub fn occlusion_planes(
    occluders: &Occluders,
    receivers: &[Receiver],
    request: &Request,
    job: &OcclusionJob<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<ReceiverOcclusion>, String> {
    let settings = job.settings;
    settings.validate()?;
    if let Some(unknown) = job.names.iter().find(|name| !PLANES.contains(name)) {
        return Err(format!(
            "The occluder service makes no plane named '{unknown}'; it makes shadow, occlusion and sunExposure."
        ));
    }
    let sun = match job.lighting.sun().filter(|sun| sun.casts_shadow) {
        Some(sun) => Some(unit(sun.direction).map_err(|_| "The sun has no direction.")?),
        None => None,
    };
    // Lay out every receiver and key every plane first, so the ray allowance is checked before
    // any ray is cast and a call either bakes everything it was asked for or nothing.
    let mut plans = Vec::with_capacity(receivers.len());
    let mut wanted = 0u64;
    for receiver in receivers {
        if cancelled() {
            return Err("Occlusion bake cancelled.".into());
        }
        let id = receiver.id.clone();
        let atlas = Atlas::for_request(receiver.clone(), request)
            .map_err(|message| format!("{id}: {message}"))?;
        let layout = Layout::new(&atlas)?;
        let mut planes = Vec::new();
        for name in PLANES.into_iter().filter(|name| job.names.contains(name)) {
            let kind = Kind::new(name, sun, settings);
            let (key, reach) = kind.key(&layout, occluders, settings);
            let held = (job.held)(&key);
            if !held {
                wanted = wanted
                    .saturating_add((layout.inner as u64).saturating_mul(kind.samples() as u64));
            }
            planes.push((name, kind, key, reach, held));
        }
        plans.push((atlas, layout, planes));
    }
    if wanted > settings.max_rays {
        return Err(format!(
            "The occlusion planes need up to {wanted} rays, above the allowance of {} (maxRays); lower the sample counts or the resolution, or raise the allowance.",
            settings.max_rays
        ));
    }
    let mut baked = Vec::with_capacity(plans.len());
    for (atlas, layout, planes) in plans {
        let mut made = Vec::with_capacity(planes.len());
        for (name, kind, key, reach, held) in planes {
            let occluders = reach.len();
            let (bytes, rays) = if held {
                (None, 0)
            } else {
                let bvh = Bvh::new(reach);
                let (values, rays) = trace(&atlas, &layout, &kind, &bvh, settings, cancelled)?;
                let bytes = Plane::encode_external(atlas.width, atlas.height, 1, &values)?;
                (Some(Arc::from(bytes)), rays)
            };
            made.push(OcclusionPlane {
                name: name.to_owned(),
                key,
                width: atlas.width,
                height: atlas.height,
                bytes,
                rays,
                occluders,
            });
        }
        baked.push(ReceiverOcclusion {
            id: atlas.receiver.id.clone(),
            planes: made,
        });
    }
    Ok(baked)
}

/// Binds every baked plane to its receiver's external input of the same name in `request`, and
/// returns the assets the bindings name ([`OcclusionPlane::asset`]) for the planes that carry
/// bytes. The host serves them, with the ones it held, through the build's asset lookup.
pub fn bind_occlusion(
    request: &mut Request,
    baked: &[ReceiverOcclusion],
) -> BTreeMap<String, Arc<[u8]>> {
    let mut assets = BTreeMap::new();
    for receiver in baked {
        let bound = request.planes.entry(receiver.id.clone()).or_default();
        for plane in &receiver.planes {
            let asset = plane.asset();
            bound.insert(plane.name.clone(), PlaneSource::Asset(asset.clone()));
            if let Some(bytes) = &plane.bytes {
                assets.insert(asset, bytes.clone());
            }
        }
    }
    assets
}

impl Session {
    /// Bakes the planes of `job` for the assigned receivers of a version 2 material project
    /// (`meshes` narrows them), against every visible mesh of its composed models. The atlases
    /// are the ones `build_project` and [`Session::project_layouts`] give those receivers.
    pub fn project_occlusion(
        &mut self,
        project: &Project,
        meshes: Option<&[String]>,
        job: &OcclusionJob<'_>,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<ReceiverOcclusion>, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have occlusion planes.".into(),
            ));
        }
        self.begin_authoring(revision, &project.entry)?;
        let (model, transforms) = self.compose_models(project, revision, assets, cancelled)?;
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        let occluders = Occluders::from_scene(&scene).map_err(&fail)?;
        let mut baked = Vec::new();
        for assignment in &project.materials {
            let ids: BTreeSet<String> = assignment
                .meshes
                .iter()
                .filter(|id| meshes.is_none_or(|wanted| wanted.contains(id)))
                .cloned()
                .collect();
            if ids.is_empty() {
                continue;
            }
            // Only the sizing inputs are read, as for a layout.
            let request = material_request(project, assignment, "", Vec::new());
            let found = receivers(&scene, &ids, &transforms);
            baked.extend(
                occlusion_planes(&occluders, &found, &request, job, cancelled).map_err(&fail)?,
            );
        }
        Ok(baked)
    }
}

/// What a plane measures, with the directions it casts.
enum Kind {
    /// No sun casts a shadow: every covered texel is lit.
    Unlit,
    Shadow {
        sun: V3,
        /// Unit directions over the sun's disk, the same for every texel.
        directions: Vec<V3>,
    },
    /// Cosine-distributed directions about +Z, turned per texel onto the face normal.
    Occlusion { pattern: Vec<V3> },
    /// The same pattern about world up.
    Exposure { pattern: Vec<V3> },
}

/// `count` points of a sunflower spiral, even in area over the unit disk: x, y and the height of
/// the unit hemisphere above them (which makes the directions cosine-distributed about +Z).
fn disk(count: usize) -> Vec<V3> {
    // The golden angle, pi (3 - sqrt 5).
    const TURN: f64 = 2.399_963_229_728_653;
    (0..count)
        .map(|k| {
            let square = (k as f64 + 0.5) / count as f64;
            let radius = square.sqrt();
            let angle = k as f64 * TURN;
            [
                radius * cos(angle),
                radius * sin(angle),
                (1. - square).sqrt(),
            ]
        })
        .collect()
}

/// Two unit vectors that complete `normal` to a right-handed frame, from basic operations only
/// (Duff et al., "Building an Orthonormal Basis, Revisited").
fn frame(normal: V3) -> [V3; 2] {
    let sign = if normal[2] >= 0. { 1. } else { -1. };
    let a = -1. / (sign + normal[2]);
    let b = normal[0] * normal[1] * a;
    [
        [
            1. + sign * normal[0] * normal[0] * a,
            sign * b,
            -sign * normal[0],
        ],
        [b, sign + normal[1] * normal[1] * a, -normal[1]],
    ]
}

impl Kind {
    fn new(name: &str, sun: Option<V3>, settings: &OcclusionSettings) -> Self {
        match (name, sun) {
            (SHADOW, None) => Self::Unlit,
            (SHADOW, Some(sun)) => {
                let directions = if settings.sun_radius == 0. || settings.shadow_samples == 1 {
                    vec![sun]
                } else {
                    let [right, forward] = frame(sun);
                    let spread = sin(settings.sun_radius) / cos(settings.sun_radius);
                    disk(settings.shadow_samples)
                        .into_iter()
                        .map(|[x, y, _]| {
                            let aside = add(mul(right, x * spread), mul(forward, y * spread));
                            let toward = add(sun, aside);
                            mul(toward, 1. / dot(toward, toward).sqrt())
                        })
                        .collect()
                };
                Self::Shadow { sun, directions }
            }
            (OCCLUSION, _) => Self::Occlusion {
                pattern: disk(settings.occlusion_samples),
            },
            _ => Self::Exposure {
                pattern: disk(settings.exposure_samples),
            },
        }
    }

    /// Rays one texel casts at most.
    fn samples(&self) -> usize {
        match self {
            Self::Unlit => 0,
            Self::Shadow { directions, .. } => directions.len(),
            Self::Occlusion { pattern } | Self::Exposure { pattern } => pattern.len(),
        }
    }

    /// The plane's key and the triangles its rays can meet, in scene order.
    fn key(
        &self,
        layout: &Layout,
        occluders: &Occluders,
        settings: &OcclusionSettings,
    ) -> (String, Vec<Triangle>) {
        let mut hash = Sha256::new();
        let number = |hash: &mut Sha256, value: f64| {
            // Adding zero gives both zeros one encoding.
            hash.update((value + 0.).to_bits().to_le_bytes());
        };
        hash.update(b"superspec-occluder");
        hash.update(OCCLUSION_VERSION.to_le_bytes());
        hash.update(layout.digest);
        number(&mut hash, settings.bias);
        // A ray's start is lifted off the receiver by at most this.
        let lift = settings.bias * layout.footprint;
        let low = layout.low;
        let high = layout.high;
        let within: Box<dyn Fn(&Triangle) -> bool> = match self {
            Self::Unlit => {
                hash.update(b"unlit");
                Box::new(|_| false)
            }
            Self::Shadow { sun, directions } => {
                hash.update(b"shadow");
                for axis in sun {
                    number(&mut hash, *axis);
                }
                number(&mut hash, settings.sun_radius);
                number(&mut hash, settings.shadow_distance);
                hash.update((directions.len() as u64).to_le_bytes());
                // In the sun's frame a ray advances toward the sun and drifts aside by at most
                // the disk's spread times that advance.
                let [right, forward] = frame(*sun);
                let axes = [right, forward, *sun];
                let along = move |point: V3| axes.map(|axis| dot(point, axis));
                let mut low = [f64::INFINITY; 3];
                let mut high = [f64::NEG_INFINITY; 3];
                for point in &layout.points {
                    let at = along(*point);
                    for axis in 0..3 {
                        low[axis] = low[axis].min(at[axis]);
                        high[axis] = high[axis].max(at[axis]);
                    }
                }
                let spread = sin(settings.sun_radius) / cos(settings.sun_radius);
                let distance = settings.shadow_distance;
                Box::new(move |triangle| {
                    let at = triangle.map(along);
                    let least = |axis: usize| at[0][axis].min(at[1][axis]).min(at[2][axis]);
                    let most = |axis: usize| at[0][axis].max(at[1][axis]).max(at[2][axis]);
                    let ahead = most(2) - low[2] + lift + SLACK;
                    if ahead < 0. || least(2) - high[2] - lift - SLACK > distance {
                        return false;
                    }
                    let grow = lift + SLACK + spread * ahead;
                    (0..2).all(|axis| {
                        least(axis) <= high[axis] + grow && most(axis) >= low[axis] - grow
                    })
                })
            }
            Self::Occlusion { pattern } => {
                hash.update(b"occlusion");
                number(&mut hash, settings.occlusion_distance);
                hash.update((pattern.len() as u64).to_le_bytes());
                let grow = settings.occlusion_distance + lift + SLACK;
                Box::new(move |triangle| {
                    (0..3).all(|axis| {
                        let least = triangle[0][axis]
                            .min(triangle[1][axis])
                            .min(triangle[2][axis]);
                        let most = triangle[0][axis]
                            .max(triangle[1][axis])
                            .max(triangle[2][axis]);
                        least <= high[axis] + grow && most >= low[axis] - grow
                    })
                })
            }
            Self::Exposure { pattern } => {
                hash.update(b"sunExposure");
                number(&mut hash, settings.exposure_distance);
                hash.update((pattern.len() as u64).to_le_bytes());
                let grow = settings.exposure_distance + lift + SLACK;
                Box::new(move |triangle| {
                    let top = triangle[0][2].max(triangle[1][2]).max(triangle[2][2]);
                    // The directions rise, so nothing wholly below the receiver is met.
                    top >= low[2] - lift - SLACK
                        && (0..3).all(|axis| {
                            let least = triangle[0][axis]
                                .min(triangle[1][axis])
                                .min(triangle[2][axis]);
                            let most = triangle[0][axis]
                                .max(triangle[1][axis])
                                .max(triangle[2][axis]);
                            least <= high[axis] + grow && most >= low[axis] - grow
                        })
                })
            }
        };
        let reach: Vec<Triangle> = occluders
            .triangles
            .iter()
            .filter(|triangle| within(triangle))
            .copied()
            .collect();
        hash.update((reach.len() as u64).to_le_bytes());
        for value in reach.iter().flatten().flatten() {
            number(&mut hash, *value);
        }
        (format!("{:x}", hash.finalize()), reach)
    }

    /// The plane's value at one texel and the rays cast for it.
    fn value(&self, texel: &Texel, bvh: &Bvh, settings: &OcclusionSettings) -> (f32, u32) {
        let mut rays = 0;
        let mut count = 0usize;
        let total = self.samples();
        match self {
            Self::Unlit => return (0., 0),
            Self::Shadow { directions, .. } => {
                for direction in directions {
                    if dot(*direction, texel.normal) <= 0. {
                        count += 1;
                    } else {
                        rays += 1;
                        count += usize::from(bvh.blocked(
                            texel.origin,
                            *direction,
                            settings.shadow_distance,
                        ));
                    }
                }
            }
            Self::Occlusion { pattern } => {
                let [right, forward] = frame(texel.normal);
                let (c, s) = turn(texel.seed);
                for [x, y, z] in pattern {
                    let (x, y) = (c * x - s * y, s * x + c * y);
                    let direction = add(add(mul(right, x), mul(forward, y)), mul(texel.normal, *z));
                    rays += 1;
                    count += usize::from(bvh.blocked(
                        texel.origin,
                        direction,
                        settings.occlusion_distance,
                    ));
                }
            }
            Self::Exposure { pattern } => {
                // Another turn than occlusion's, so the two planes' noise is unrelated.
                let (c, s) = turn(mixed(texel.seed ^ 0x5eed_0f5c_a1ab_1e55));
                for [x, y, z] in pattern {
                    let direction = [c * x - s * y, s * x + c * y, *z];
                    if dot(direction, texel.normal) > 0. {
                        rays += 1;
                        count += usize::from(!bvh.blocked(
                            texel.origin,
                            direction,
                            settings.exposure_distance,
                        ));
                    }
                }
            }
        }
        (count as f32 / total as f32, rays)
    }
}

/// SplitMix64's finalizer: every input bit reaches every output bit.
fn mixed(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Cosine and sine of the turn a seed gives a texel's direction pattern.
fn turn(seed: u64) -> (f64, f64) {
    let angle = (seed >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
    (cos(angle), sin(angle))
}

/// What a plane's values depend on in a receiver's atlas, and what its rays start from.
struct Layout {
    /// Hash of the atlas: its size, charts and every triangle's corners.
    digest: [u8; 32],
    /// Chart of each atlas texel plus one; 0 outside every chart.
    owner: Vec<u32>,
    /// Unit face normal of each triangle, on the side its vertex normals point to.
    normals: Vec<V3>,
    /// Corner positions of every triangle.
    points: Vec<V3>,
    /// World bounds of the receiver.
    low: V3,
    high: V3,
    /// The largest texel footprint, meters.
    footprint: f64,
    /// Texels that cast rays: the inner texels of every chart.
    inner: usize,
}

impl Layout {
    fn new(atlas: &Atlas) -> Result<Self, String> {
        let mut hash = Sha256::new();
        let word = |hash: &mut Sha256, value: usize| hash.update((value as u64).to_le_bytes());
        word(&mut hash, atlas.width);
        word(&mut hash, atlas.height);
        word(&mut hash, atlas.charts.len());
        let mut owner = vec![0u32; atlas.width * atlas.height];
        let mut inner = 0;
        for (index, chart) in atlas.charts.iter().enumerate() {
            for value in [chart.x, chart.y, chart.width, chart.height, chart.first] {
                word(&mut hash, value);
            }
            word(&mut hash, chart.second.map_or(0, |second| second + 1));
            word(&mut hash, usize::from(chart.rotated));
            word(&mut hash, chart.samples[0]);
            word(&mut hash, chart.samples[1]);
            for y in chart.y..chart.y + chart.height {
                owner[y * atlas.width + chart.x..][..chart.width].fill(index as u32 + 1);
            }
            inner += (chart.width - 2 * GUTTER) * (chart.height - 2 * GUTTER);
        }
        let points = atlas.mesh.positions.clone();
        let triangles = points.len() / 3;
        let mut normals = Vec::with_capacity(triangles);
        let mut footprint = 0f64;
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for triangle in 0..triangles {
            let corners = &points[triangle * 3..triangle * 3 + 3];
            let shading = &atlas.mesh.normals[triangle * 3..triangle * 3 + 3];
            for vector in corners.iter().chain(shading) {
                for value in vector {
                    hash.update((value + 0.).to_bits().to_le_bytes());
                }
            }
            for point in corners {
                for axis in 0..3 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let face = unit(cross(
                sub(corners[1], corners[0]),
                sub(corners[2], corners[0]),
            ))?;
            let outward = add(add(shading[0], shading[1]), shading[2]);
            normals.push(if dot(face, outward) < 0. {
                mul(face, -1.)
            } else {
                face
            });
            footprint = footprint.max(atlas.sample(triangle, 0., 0.)?.footprint);
        }
        Ok(Self {
            digest: hash.finalize().into(),
            owner,
            normals,
            points,
            low,
            high,
            footprint,
            inner,
        })
    }
}

/// Where one texel's rays start.
struct Texel {
    origin: V3,
    /// The face normal.
    normal: V3,
    /// A hash of the texel's sample, which turns its direction pattern.
    seed: u64,
}

/// The values of one row's inner texels (NaN elsewhere) and the rays cast for them.
fn row(
    atlas: &Atlas,
    layout: &Layout,
    kind: &Kind,
    bvh: &Bvh,
    settings: &OcclusionSettings,
    y: usize,
) -> Result<(Vec<f32>, u64), String> {
    let mut values = vec![f32::NAN; atlas.width];
    let mut rays = 0u64;
    for (x, value) in values.iter_mut().enumerate() {
        let owner = layout.owner[y * atlas.width + x];
        if owner == 0 {
            continue;
        }
        let chart = &atlas.charts[owner as usize - 1];
        let (cx, cy) = (x - chart.x, y - chart.y);
        if cx < GUTTER || cy < GUTTER || cx >= chart.width - GUTTER || cy >= chart.height - GUTTER {
            continue;
        }
        let (triangle, u, v) = atlas.texel(chart, cx, cy);
        let sample = atlas.sample(triangle, u, v)?;
        let normal = layout.normals[triangle];
        let texel = Texel {
            origin: add(sample.world, mul(normal, settings.bias * sample.footprint)),
            normal,
            seed: mixed(mixed(mixed(triangle as u64 + 1) ^ u.to_bits()) ^ v.to_bits()),
        };
        let (found, cast) = kind.value(&texel, bvh, settings);
        *value = found;
        rays += u64::from(cast);
    }
    Ok((values, rays))
}

/// Every texel of one plane, rows from the atlas's upper edge, and the rays cast for it. Rows
/// are independent, so any number of threads gives the same values.
fn trace(
    atlas: &Atlas,
    layout: &Layout,
    kind: &Kind,
    bvh: &Bvh,
    settings: &OcclusionSettings,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Vec<f32>, u64), String> {
    let (width, height) = (atlas.width, atlas.height);
    let mut values = vec![f32::NAN; width * height];
    let mut rays = 0u64;
    let threads = settings.threads().min(height.max(1));
    let mut rows: Vec<(usize, (Vec<f32>, u64))> = Vec::with_capacity(height);
    if threads <= 1 {
        for y in 0..height {
            if cancelled() {
                return Err("Occlusion bake cancelled.".into());
            }
            rows.push((y, row(atlas, layout, kind, bvh, settings, y)?));
        }
    } else {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
            let next = AtomicUsize::new(0);
            let stop = AtomicBool::new(false);
            let mut stopped = false;
            let joined = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut done = Vec::new();
                            while !stop.load(Ordering::Relaxed) {
                                let y = next.fetch_add(1, Ordering::Relaxed);
                                if y >= height {
                                    break;
                                }
                                match row(atlas, layout, kind, bvh, settings, y) {
                                    Ok(made) => done.push((y, made)),
                                    Err(error) => {
                                        stop.store(true, Ordering::Relaxed);
                                        return Err(error);
                                    }
                                }
                            }
                            Ok(done)
                        })
                    })
                    .collect();
                // The caller's cancellation is asked on its own thread only.
                while !handles.iter().all(|handle| handle.is_finished()) {
                    if cancelled() {
                        stopped = true;
                        stop.store(true, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("occlusion rows do not panic"))
                    .collect::<Vec<_>>()
            });
            // Rows that finished before the caller was asked still belong to a cancelled bake.
            if stopped || cancelled() {
                return Err("Occlusion bake cancelled.".into());
            }
            for part in joined {
                rows.extend(part?);
            }
        }
    }
    for (y, (made, cast)) in rows {
        values[y * width..(y + 1) * width].copy_from_slice(&made);
        rays += cast;
    }
    // Gutter texels repeat the nearest inner texel, so filtering across a chart's edge reads the
    // edge's own value.
    for chart in &atlas.charts {
        for cy in 0..chart.height {
            for cx in 0..chart.width {
                let from = (
                    cx.clamp(GUTTER, chart.width - GUTTER - 1),
                    cy.clamp(GUTTER, chart.height - GUTTER - 1),
                );
                if from != (cx, cy) {
                    values[(chart.y + cy) * width + chart.x + cx] =
                        values[(chart.y + from.1) * width + chart.x + from.0];
                }
            }
        }
    }
    Ok((values, rays))
}

/// A bounding volume hierarchy over triangles, split at the median of the widest axis. It only
/// answers whether a ray meets anything, which does not depend on the order of its triangles.
struct Bvh {
    nodes: Vec<Node>,
    /// A corner and the two edges leaving it, in leaf order.
    triangles: Vec<Triangle>,
}

struct Node {
    low: V3,
    high: V3,
    /// A leaf's first triangle, or the first of an inner node's two children.
    first: u32,
    /// Triangles of a leaf; 0 for an inner node.
    count: u32,
}

/// Triangles a leaf holds at most, unless their centers coincide.
const LEAF: usize = 4;

impl Bvh {
    fn new(triangles: Vec<Triangle>) -> Self {
        let centers: Vec<V3> = triangles
            .iter()
            .map(|t| std::array::from_fn(|axis| t[0][axis] + t[1][axis] + t[2][axis]))
            .collect();
        let mut order: Vec<u32> = (0..triangles.len() as u32).collect();
        let mut nodes = Vec::with_capacity(triangles.len() / 2 + 1);
        if !triangles.is_empty() {
            nodes.push(Node {
                low: [0.; 3],
                high: [0.; 3],
                first: 0,
                count: 0,
            });
            Self::split(
                &triangles,
                &centers,
                &mut order,
                &mut nodes,
                0,
                0,
                triangles.len(),
            );
        }
        let triangles = order
            .iter()
            .map(|index| {
                let [a, b, c] = triangles[*index as usize];
                [a, sub(b, a), sub(c, a)]
            })
            .collect();
        Self { nodes, triangles }
    }

    fn split(
        triangles: &[Triangle],
        centers: &[V3],
        order: &mut [u32],
        nodes: &mut Vec<Node>,
        node: usize,
        start: usize,
        end: usize,
    ) {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        let mut center_low = [f64::INFINITY; 3];
        let mut center_high = [f64::NEG_INFINITY; 3];
        for index in &order[start..end] {
            for point in &triangles[*index as usize] {
                for axis in 0..3 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let center = centers[*index as usize];
            for axis in 0..3 {
                center_low[axis] = center_low[axis].min(center[axis]);
                center_high[axis] = center_high[axis].max(center[axis]);
            }
        }
        let extent = sub(center_high, center_low);
        let axis = (0..3)
            .max_by(|a, b| extent[*a].total_cmp(&extent[*b]))
            .unwrap_or(0);
        if end - start <= LEAF || extent[axis] <= 0. {
            nodes[node] = Node {
                low,
                high,
                first: start as u32,
                count: (end - start) as u32,
            };
            return;
        }
        let middle = (start + end) / 2;
        // Ties break by index, so the two halves are the same sets on every target.
        order[start..end].select_nth_unstable_by(middle - start, |a, b| {
            centers[*a as usize][axis]
                .total_cmp(&centers[*b as usize][axis])
                .then(a.cmp(b))
        });
        let children = nodes.len();
        for _ in 0..2 {
            nodes.push(Node {
                low: [0.; 3],
                high: [0.; 3],
                first: 0,
                count: 0,
            });
        }
        nodes[node] = Node {
            low,
            high,
            first: children as u32,
            count: 0,
        };
        Self::split(triangles, centers, order, nodes, children, start, middle);
        Self::split(triangles, centers, order, nodes, children + 1, middle, end);
    }

    /// Whether the ray from `origin` along the unit `direction` meets a triangle (either face)
    /// between [`NEAR`] and `reach` meters.
    fn blocked(&self, origin: V3, direction: V3, reach: f64) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let inverse = direction.map(|v| 1. / v);
        // Median splits halve a node, so 64 levels hold any admitted scene.
        let mut stack = [0u32; 64];
        let mut depth = 1;
        while depth > 0 {
            depth -= 1;
            let node = &self.nodes[stack[depth] as usize];
            let mut near = NEAR;
            let mut far = reach;
            for axis in 0..3 {
                let a = (node.low[axis] - origin[axis]) * inverse[axis];
                let b = (node.high[axis] - origin[axis]) * inverse[axis];
                // A ray in a face of the box gives NaN, which `min` and `max` pass over.
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            // Widened by a few units in the last place, so rounding never drops a box.
            if near > far * 1.000_000_000_000_001 {
                continue;
            }
            if node.count == 0 {
                if depth + 2 > stack.len() {
                    continue;
                }
                stack[depth] = node.first;
                stack[depth + 1] = node.first + 1;
                depth += 2;
                continue;
            }
            for [corner, first, second] in
                &self.triangles[node.first as usize..(node.first + node.count) as usize]
            {
                let p = cross(direction, *second);
                let determinant = dot(*first, p);
                if determinant == 0. {
                    continue;
                }
                let scale = 1. / determinant;
                let from = sub(origin, *corner);
                let u = dot(from, p) * scale;
                if !(0. ..=1.).contains(&u) {
                    continue;
                }
                let q = cross(from, *first);
                let v = dot(direction, q) * scale;
                if v < 0. || u + v > 1. {
                    continue;
                }
                let t = dot(*second, q) * scale;
                if t > NEAR && t < reach {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
#[path = "occlusion_tests.rs"]
mod tests;
