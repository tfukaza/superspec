//! Geometry receivers for metal assemblies. Mutable collection ends before immutable raster jobs.
#![allow(clippy::too_many_arguments)]
#[cfg(not(target_arch = "wasm32"))]
pub use super::bake_jobs::ParallelBaking;
pub use super::bake_jobs::{Job, Plan, ReceiverRange, Recipe};
use super::{
    exposure::Assembly,
    mesh::{Mesh, rounded},
    metal::{self, Binding, Kind, Metal},
    palette,
    pen::{Pen, Tint},
    surface::{Layout, RasterSettings},
    wall::finish_seed,
};
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::{
    Matrix, V3,
    math::{dot, length, scale, sub, unit},
};

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub seed: String,
    pub wear: f64,
    pub grime: f64,
    pub rust: f64,
    pub texture_density: f64,
    pub metal: Binding,
    pub sheet: Binding,
    pub paint: Binding,
}
impl Settings {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.seed.len() > 4096
            || [self.wear, self.grime, self.rust]
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || !self.texture_density.is_finite()
            || !(0. ..=4096.).contains(&self.texture_density)
            || [&self.metal, &self.sheet, &self.paint].iter().any(|b| {
                b.material.is_empty()
                    || b.material.len() > 256
                    || !b.repeat.is_finite()
                    || b.repeat <= 0.
            })
        {
            return Err("Invalid metal receiver settings.".into());
        }
        Ok(())
    }
}

/// Affine unfolded coordinates in the attaching pen's local frame. Derived pens keep that frame.
#[derive(Clone)]
pub struct Mapping {
    pub bounds: [f64; 4],
    pub axes: [V3; 2],
    pub offset: [f64; 2],
    pub gravity: [f64; 2],
    pub paint: V3,
    pub key: String,
}
impl Mapping {
    fn uv(&self, point: V3) -> [f64; 2] {
        std::array::from_fn(|i| dot(point, self.axes[i]) + self.offset[i])
    }
}

#[derive(Clone)]
pub(super) struct Style {
    settings: Arc<Settings>,
    identity: Arc<()>,
    assembly: usize,
    kind: Kind,
    slots: Arc<Vec<String>>,
    mapping: Option<Arc<Mapping>>,
    mapping_inverse: Matrix,
    member: Option<usize>,
    compact: bool,
    beveling: bool,
    tube_axis: Option<V3>,
}
pub(super) struct Receiver {
    pub(super) first_triangle: usize,
    pub(super) triangle_count: usize,
    style: Style,
    inverse: Matrix,
    projection: Projection,
    pub(super) settings: metal::Settings,
    density: f64,
}
pub(super) struct Prepared {
    pub(super) first_triangle: usize,
    pub(super) triangle_count: usize,
    pub(super) settings: metal::Settings,
    density: f64,
    key: Option<String>,
    runoff: Option<super::pipe_runoff::Recipe>,
}
const MAX_RECEIVERS: usize = 32_768;
const MAX_RECIPE_BYTES: usize = 64 * 1024 * 1024;
// Complete multi-lot hosts need more records than an isolated facade; storage remains bounded.
#[allow(clippy::large_enum_variant)]
pub(super) enum Pending {
    Component(Receiver),
    Prepared(Prepared),
    Surface {
        first: usize,
        count: usize,
        job: Job,
        group: String,
        spatial: Option<super::stains::Receiver>,
    },
}
#[derive(Default)]
pub(super) struct Collection {
    assemblies: Vec<Assembly>,
    pub(super) receivers: Vec<Pending>,
    sealed: bool,
    deposits: Vec<super::stains::Deposit>,
    members: usize,
    mapped_corners: usize,
    custom_identity_bytes: usize,
}
pub(super) fn deposit(mesh: &mut Mesh<'_>, deposit: super::stains::Deposit) -> Result<(), String> {
    if mesh.weathering.sealed || mesh.weathering.deposits.len() >= 4096 {
        return Err("Fixture deposit collection is sealed or exhausted.".into());
    }
    mesh.weathering.deposits.push(deposit);
    Ok(())
}
pub(super) fn receive_surface(
    mesh: &mut Mesh<'_>,
    first: usize,
    job: Job,
    group: String,
    spatial: Option<super::stains::Receiver>,
) -> Result<(), String> {
    if mesh.surveying() {
        return Ok(());
    }
    mesh.check_work(0)?;
    let count = mesh
        .triangle_count()
        .checked_sub(first)
        .ok_or("Surface receiver exceeds drawing.")?;
    if count == 0 {
        return Ok(());
    }
    job.validate()?;
    if mesh.weathering.sealed
        || mesh.weathering.receivers.len() >= MAX_RECEIVERS
        || group.len() > 4096
    {
        return Err("Invalid or exhausted surface receiver.".into());
    }
    // Inputs are cloned from borrowed slices, so spare caller capacities are never retained.
    let owned = match &job.recipe {
        Recipe::Paint(r) => {
            r.finish.panels.capacity() * std::mem::size_of::<[f64; 4]>()
                + r.binding.material.capacity()
        }
        Recipe::Frosted(r) => r.material.capacity(),
        Recipe::Facade(r) => r.coat.material.capacity() + r.backing.material.capacity(),
        Recipe::TiledSkin(r) => r.owned_bytes(),
        Recipe::Sign(r) => r.owned_bytes(),
        Recipe::Pavement(r) => r.owned_bytes(),
        Recipe::Tile(r) => {
            r.materials.ceramic.capacity()
                + r.materials.bed.capacity()
                + r.materials.grime.capacity()
        }
        Recipe::FacadeDeposits(r) => {
            r.finish.coat.material.capacity()
                + r.finish.backing.material.capacity()
                + r.condition.ledges.capacity() * std::mem::size_of::<super::wall::Ledge>()
                + r.condition.openings.capacity() * std::mem::size_of::<super::wall::Opening>()
        }
        _ => return Err("Expected flat-surface recipe.".into()),
    };
    let bytes = serde_json::to_string(&(&job.recipe, job.density, &group))
        .map_err(|e| e.to_string())?
        .len()
        + owned
        + group.capacity()
        + std::mem::size_of::<Pending>()
        + spatial
            .as_ref()
            .map_or(0, super::stains::Receiver::owned_bytes);
    if mesh.weathering.custom_identity_bytes + bytes > MAX_RECIPE_BYTES {
        return Err("Explicit raster recipe identity budget exceeded.".into());
    }
    mesh.weathering.custom_identity_bytes += bytes;
    mesh.weathering.receivers.push(Pending::Surface {
        first,
        count,
        job,
        group,
        spatial,
    });
    Ok(())
}
struct Projection {
    axes: [V3; 2],
    minimum: [f64; 2],
    bounds: [f64; 4],
}
impl Projection {
    fn new(points: &[V3]) -> Result<Self, String> {
        let edges: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, p)| sub(points[(i + 1) % points.len()], *p))
            .collect();
        let longest = edges
            .iter()
            .copied()
            .reduce(|a, b| if length(a) > length(b) { a } else { b })
            .unwrap();
        let mut along = unit(longest)?;
        let axis = if along[2].abs() > 0.5 {
            2
        } else if along[0].abs() >= along[1].abs() {
            0
        } else {
            1
        };
        if along[axis] < 0. {
            along = scale(along, -1.);
        }
        let other = edges
            .iter()
            .find(|e| dot(**e, along).abs() < length(**e) * 0.9)
            .copied()
            .unwrap_or(edges[1]);
        let mut across = unit(sub(other, scale(along, dot(other, along))))?;
        let mut axis = 0;
        for i in 1..3 {
            if across[i].abs() > across[axis].abs() {
                axis = i;
            }
        }
        if across[axis] < 0. {
            across = scale(across, -1.);
        }
        let axes = [across, along];
        let minimum = axes.map(|a| {
            points
                .iter()
                .map(|p| dot(*p, a))
                .fold(f64::INFINITY, f64::min)
        });
        let extent: [f64; 2] = std::array::from_fn(|i| {
            rounded(
                points
                    .iter()
                    .map(|p| dot(*p, axes[i]))
                    .fold(f64::NEG_INFINITY, f64::max)
                    - minimum[i],
                1e6,
            )
        });
        if extent.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err("Metal receiver has degenerate metric bounds.".into());
        }
        Ok(Self {
            axes,
            minimum,
            bounds: [0., 0., extent[0], extent[1]],
        })
    }
    fn uv(&self, p: V3) -> [f64; 2] {
        std::array::from_fn(|i| dot(p, self.axes[i]) - self.minimum[i])
    }
}

impl Style {
    /// Nested fixtures with identical settings retain contacts from their enclosing assembly.
    pub(super) fn reuse(
        &self,
        mesh: &Mesh<'_>,
        settings: &Settings,
        kind: Kind,
        slots: &[String],
    ) -> Result<Option<Self>, String> {
        if self.mapping.is_some()
            || *self.settings != *settings
            || !slots.iter().all(|s| self.slots.contains(s))
        {
            return Ok(None);
        }
        self.check(mesh)?;
        let mut style = self.clone();
        if kind != self.kind {
            style.slots = Arc::new(slots.to_vec());
            style.member = None;
            style.compact = false;
            style.beveling = false;
            style.tube_axis = None;
        }
        style.kind = kind;
        Ok(Some(style))
    }
    pub(super) fn attach(
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        settings: Settings,
        kind: Kind,
        mapping: Option<Mapping>,
        slots: Vec<String>,
    ) -> Result<Self, String> {
        settings.validate()?;
        if mesh.weathering.sealed
            || mesh.weathering.assemblies.len() >= 8192
            || slots.is_empty()
            || slots.len() > 32
            || slots.iter().any(|s| s.is_empty() || s.len() > 256)
        {
            return Err("Invalid or exhausted metal receiver settings.".into());
        }
        if let Some(m) = &mapping
            && (m.key.len() > 4096
                || m.bounds
                    .iter()
                    .chain(m.axes.iter().flatten())
                    .chain(&m.offset)
                    .chain(&m.gravity)
                    .chain(&m.paint)
                    .any(|v| !v.is_finite())
                || m.bounds[0] >= m.bounds[2]
                || m.bounds[1] >= m.bounds[3]
                || m.paint.iter().any(|v| *v < 0.))
        {
            return Err("Invalid continuous metal mapping.".into());
        }
        let assembly = mesh.weathering.assemblies.len();
        mesh.weathering.assemblies.push(Assembly::default());
        Ok(Self {
            settings: Arc::new(settings),
            identity: mesh.identity.clone(),
            assembly,
            kind,
            slots: Arc::new(slots),
            mapping: mapping.map(Arc::new),
            mapping_inverse: pen.matrix.inverse()?,
            member: None,
            compact: false,
            beveling: false,
            tube_axis: None,
        })
    }
    fn check(&self, mesh: &Mesh<'_>) -> Result<(), String> {
        mesh.check_work(0)?;
        if !Arc::ptr_eq(&self.identity, &mesh.identity) || mesh.weathering.sealed {
            return Err("Metal pen belongs to another or finalized drawing.".into());
        }
        Ok(())
    }
    pub(super) fn selected(&self, slot: &str, mesh: &Mesh<'_>) -> bool {
        !mesh.surveying() && self.slots.iter().any(|s| s == slot)
    }
    pub(super) fn scoped(
        &self,
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        slot: &str,
        a: V3,
        b: V3,
        radius: f64,
        width: f64,
        beveling: bool,
        tube_axis: Option<V3>,
    ) -> Result<Pen, String> {
        if !self.selected(slot, mesh) {
            return Ok(pen.clone());
        }
        self.check(mesh)?;
        if mesh.weathering.members >= 16_384 {
            return Err("Metal drawing exceeds 16384 members.".into());
        }
        if !width.is_finite() || width <= 0. {
            return Err("Metal member needs positive finite width.".into());
        }
        let mut style = self.clone();
        style.member = Some(mesh.weathering.assemblies[self.assembly].member(
            pen.world(a),
            pen.world(b),
            radius,
        )?);
        mesh.weathering.members += 1;
        let axes = &pen.matrix.0[..3];
        let stretch = axes
            .iter()
            .map(|a| axes.iter().map(|b| dot(*a, *b).abs()).sum::<f64>())
            .fold(0., f64::max)
            .sqrt();
        style.compact = self.mapping.is_none()
            && matches!(self.kind, Kind::Frame | Kind::Cage)
            && width * stretch <= 0.04 + 1e-12;
        style.beveling = beveling;
        style.tube_axis = tube_axis;
        let mut scoped = pen.clone();
        scoped.weathered = Some(style);
        Ok(scoped)
    }
    pub(super) fn face(
        &self,
        pen: &Pen,
        mesh: &mut Mesh<'_>,
        slot: &str,
        points: &[V3],
        hint: V3,
        tint: &Tint<'_>,
        repeat: f64,
        uv: Option<&[[f64; 2]]>,
        normals: Option<&[V3]>,
    ) -> Result<(), String> {
        if !self.selected(slot, mesh) || !matches!(tint, Tint::Color(_)) {
            return pen.face_raw(mesh, slot, points, hint, tint, repeat, uv, normals);
        }
        self.check(mesh)?;
        if points.len() < 3
            || points.len() > 4096
            || points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err("Invalid metal receiver geometry.".into());
        }
        let Tint::Color(color) = tint else {
            unreachable!()
        };
        if color.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("Metal paint must be finite and nonnegative.".into());
        }
        let s = &self.settings;
        if self.compact {
            let aged = palette::mix(*color, palette::srgb("#7a4a2a"), s.rust * 0.12)
                .map(|v| v * (1. - s.grime * 0.12));
            return pen.face_raw(
                mesh,
                "paint",
                points,
                hint,
                &Tint::Color(aged),
                s.paint.repeat,
                None,
                normals,
            );
        }
        if mesh.weathering.receivers.len() >= MAX_RECEIVERS {
            return Err("Metal receiver count exceeds its allowance.".into());
        }
        let projection = Projection::new(points)?;
        let bounds = self
            .mapping
            .as_ref()
            .map_or(projection.bounds, |m| m.bounds);
        let coordinates: Vec<_> = points
            .iter()
            .map(|p| {
                self.mapping.as_ref().map_or_else(
                    || projection.uv(*p),
                    |m| m.uv(self.mapping_inverse.point(pen.world(*p))),
                )
            })
            .collect();
        let paint = self.mapping.as_ref().map_or(*color, |m| m.paint);
        let width = (bounds[2] - bounds[0]).min(bounds[3] - bounds[1]);
        if self.beveling && self.mapping.is_none() && width < 0.006 {
            return pen.face_raw(
                mesh,
                "paint",
                points,
                hint,
                &Tint::Color(paint),
                s.paint.repeat,
                Some(&coordinates),
                normals,
            );
        }
        let variant = if self.mapping.is_some() {
            0
        } else {
            let key = points[0]
                .map(|v| ryu_js::Buffer::new().format_to_fixed(v, 3).to_string())
                .join("/");
            finish_seed(&key) % 2
        };
        let kind = match self.kind {
            Kind::Frame => "frame",
            Kind::Cage => "cage",
            Kind::Pipe => "pipe",
            Kind::Sheet => "sheet",
        };
        let gravity = self
            .mapping
            .as_ref()
            .map_or_else(
                || projection.axes.map(|a| -pen.matrix.direction(a)[2]),
                |m| m.gravity,
            )
            .map(|v| rounded(v, 1e4));
        let edge_axes = if self.mapping.is_none() {
            self.tube_axis.map_or([true, true], |axis| {
                projection.axes.map(|a| dot(a, axis).abs() > 0.5)
            })
        } else {
            [true, true]
        };
        let settings = metal::Settings {
            kind: self.kind,
            bounds,
            paint,
            seed: finish_seed(&format!("{}/{kind}/{variant}", s.seed)) as f64,
            wear: (s.wear * 1.6).min(1.),
            grime: s.grime,
            rust: s.rust,
            paint_coverage: None,
            edge_axes: Some(edge_axes),
            contacts: None,
            metric: Some(metal::Metric {
                scale: [1., 1.],
                gravity,
            }),
            folds: vec![],
            wrap_u: None,
            substrate: if slot == "sheet" {
                s.sheet.clone()
            } else {
                s.metal.clone()
            },
            coating: s.paint.clone(),
        };
        Metal::new(&settings, None)?;
        let minimum = if self.mapping.is_some() || matches!(self.kind, Kind::Sheet) {
            256.
        } else if width < 0.04 {
            64.
        } else {
            128.
        };
        // Sub-default authored densities also lower the metal receiver's minimum detail.
        let minimum = minimum * (s.texture_density / 64.).clamp(0.25, 1.);
        let density = s.texture_density.max(minimum);
        let density = if self.mapping.is_none() && width < 0.006 {
            density.min(64.)
        } else {
            density
        };
        Layout::new(raster_settings(bounds, density))?;
        if self.mapping.is_some() {
            if mesh.weathering.mapped_corners + points.len() > 262_144 {
                return Err("Metal drawing exceeds 262144 mapped corners.".into());
            }
            mesh.weathering.assemblies[self.assembly].face(
                &points.iter().map(|p| pen.world(*p)).collect::<Vec<_>>(),
                pen.matrix.direction(hint),
                &coordinates,
            )?;
            mesh.weathering.mapped_corners += points.len();
        }
        let first_triangle = mesh.triangle_count();
        pen.face_raw(
            mesh,
            slot,
            points,
            hint,
            &Tint::Color([1.; 3]),
            repeat,
            Some(&coordinates),
            normals,
        )?;
        let triangle_count = mesh.triangle_count() - first_triangle;
        if triangle_count > 0 {
            mesh.weathering.receivers.push(Pending::Component(Receiver {
                first_triangle,
                triangle_count,
                style: self.clone(),
                inverse: pen.matrix.inverse()?,
                projection,
                settings,
                density,
            }));
        }
        Ok(())
    }
}

/// Registers a complete explicit chart after shape emission. It participates in the same bounded jobs.
pub fn receive(
    mesh: &mut Mesh<'_>,
    first_triangle: usize,
    triangle_count: usize,
    mut settings: metal::Settings,
    density: f64,
    key: String,
    mut runoff: Option<super::pipe_runoff::Recipe>,
) -> Result<(), String> {
    if mesh.surveying() {
        return Ok(());
    }
    mesh.check_work(0)?;
    if mesh.weathering.sealed
        || mesh.weathering.receivers.len() >= MAX_RECEIVERS
        || triangle_count == 0
        || first_triangle
            .checked_add(triangle_count)
            .is_none_or(|end| end > mesh.triangle_count())
        || key.len() > 65536
    {
        return Err("Invalid or exhausted explicit metal receiver.".into());
    }
    Metal::new(&settings, None)?;
    Layout::new(raster_settings(settings.bounds, density))?;
    if let Some(recipe) = &runoff {
        recipe.working_bytes()?;
    }
    // Public callers can pass small logical inputs with arbitrarily large spare allocations.
    // Release that capacity before retaining the recipe, and charge its owned storage as well.
    settings.contacts = settings.contacts.map(|v| v.into_boxed_slice().into_vec());
    settings.folds = settings.folds.into_boxed_slice().into_vec();
    settings.substrate.material = settings.substrate.material.into_boxed_str().into_string();
    settings.coating.material = settings.coating.material.into_boxed_str().into_string();
    let key = key.into_boxed_str().into_string();
    if let Some(recipe) = &mut runoff {
        recipe.rings = std::mem::take(&mut recipe.rings)
            .into_boxed_slice()
            .into_vec();
        recipe.seams = std::mem::take(&mut recipe.seams)
            .into_boxed_slice()
            .into_vec();
    }
    let owned_bytes = std::mem::size_of::<Prepared>()
        + key.len()
        + settings.substrate.material.len()
        + settings.coating.material.len()
        + settings.folds.len() * std::mem::size_of::<metal::Fold>()
        + settings
            .contacts
            .as_ref()
            .map_or(0, |v| v.len() * std::mem::size_of::<[f64; 2]>())
        + runoff.as_ref().map_or(0, |r| {
            r.rings.len() * std::mem::size_of::<super::pipe_runoff::Ring>()
                + r.seams.len() * std::mem::size_of::<f64>()
        });
    let identity = serde_json::to_string(&(&settings, density, &key, &runoff))
        .map_err(|e| e.to_string())?
        .len()
        + owned_bytes;
    if mesh.weathering.custom_identity_bytes + identity > MAX_RECIPE_BYTES {
        return Err("Explicit metal recipe identity budget exceeded.".into());
    }
    mesh.weathering.custom_identity_bytes += identity;
    mesh.weathering.receivers.push(Pending::Prepared(Prepared {
        first_triangle,
        triangle_count,
        settings,
        density,
        key: Some(key),
        runoff,
    }));
    Ok(())
}

impl Receiver {
    fn prepare(mut self, exposure: &[super::exposure::Exposure]) -> Prepared {
        let e = &exposure[self.style.assembly];
        let mut contacts = if self.style.mapping.is_none() {
            self.style
                .member
                .and_then(|i| e.joints(i))
                .unwrap_or(&[])
                .iter()
                .map(|p| self.projection.uv(self.inverse.point(*p)))
                .filter(|p| {
                    (0..2).all(|i| {
                        p[i] >= self.settings.bounds[i] && p[i] <= self.settings.bounds[i + 2]
                    })
                })
                .map(|p| p.map(|v| rounded(v / 0.005, 1.) * 0.005))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        contacts.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
        contacts.dedup();
        self.settings.contacts = Some(contacts);
        self.settings.folds = e.folds.clone();
        Prepared {
            first_triangle: self.first_triangle,
            triangle_count: self.triangle_count,
            settings: self.settings,
            density: self.density,
            key: self.style.mapping.as_ref().map(|m| m.key.clone()),
            runoff: None,
        }
    }
}

pub(super) fn raster_settings(bounds: [f64; 4], density: f64) -> RasterSettings {
    RasterSettings {
        bounds,
        texels_per_meter: density,
        preserve: true,
        repeat: false,
        uv_coordinates: true,
        bump: true,
    }
}

/// Seals this detached drawing. On failure discard the job; no scene data has been published.
pub fn finish<'mesh, 'cancel>(
    mesh: &'mesh mut Mesh<'cancel>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Plan<'mesh, 'cancel>, String> {
    if mesh.weathering.sealed {
        return Err("Metal drawing already finalized.".into());
    }
    let collection = std::mem::take(&mut mesh.weathering);
    mesh.weathering.sealed = true;
    let mut exposure = Vec::new();
    for assembly in collection.assemblies {
        exposure.push(assembly.finish(cancelled)?);
    }
    let mut jobs = Vec::new();
    let mut receivers = Vec::new();
    let mut groups = BTreeMap::new();
    let mut key_bytes = 0;
    let mut pixels = 0usize;
    let mut deposit_bytes = 0usize;
    for pending in collection.receivers {
        if cancelled() {
            return Err("Metal planning cancelled.".into());
        }
        let (first, count, recipe, group) = match pending {
            Pending::Surface {
                first,
                count,
                job,
                group,
                spatial,
            } => {
                let job = match spatial {
                    Some(r) => r.apply(job, &collection.deposits, mesh, &mut deposit_bytes)?,
                    None => job,
                };
                (first, count, job, Some(group))
            }
            pending => {
                let r = match pending {
                    Pending::Component(receiver) => receiver.prepare(&exposure),
                    Pending::Prepared(receiver) => receiver,
                    Pending::Surface { .. } => unreachable!(),
                };
                (
                    r.first_triangle,
                    r.triangle_count,
                    Job {
                        density: r.density,
                        recipe: Recipe::Metal {
                            settings: Box::new(r.settings),
                            runoff: r.runoff,
                        },
                    },
                    r.key,
                )
            }
        };
        recipe.validate()?;
        let key = serde_json::to_string(&(&recipe.recipe, recipe.density, &group))
            .map_err(|e| e.to_string())?;
        let job = if let Some(id) = groups.get(&key) {
            *id
        } else {
            key_bytes += key.len();
            if key_bytes > MAX_RECIPE_BYTES {
                return Err("Metal recipe identity budget exceeded.".into());
            }
            let layout = Layout::new(recipe.raster_settings())?;
            pixels += layout.width * layout.height * 3;
            if pixels > 64 * 1024 * 1024 {
                return Err("Surface raster jobs exceed 64 Mi channel-pixels per host.".into());
            }
            let id = jobs.len();
            groups.insert(key, id);
            jobs.push(recipe);
            id
        };
        receivers.push(ReceiverRange {
            first_triangle: first,
            triangle_count: count,
            job,
        });
    }
    Ok(Plan {
        jobs,
        receivers,
        mesh,
        edge_vertices: Default::default(),
        edge_index: None,
    })
}
