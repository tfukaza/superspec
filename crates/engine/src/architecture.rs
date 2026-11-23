//! Architectural lowering. Every kernel allocation is owned by the compilation session.
use crate::{
    Mesh, Scene,
    resolve::{Resolution, Resolved},
};
use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::{architecture::Node, diagnostic::Diagnostic, material::Material};
use superspec_geometry::{Geometry, Operation, Profile, Solid, math::*};
pub(crate) mod build;
mod connection_proofs;
mod context;
mod enclosures;
mod export;
mod flights;
mod grading;
mod joins;
pub(crate) mod kowloon;
pub use context::Context;
mod materials;
mod retaining;
mod terrain;
mod validation;
mod walls;
mod windows;

trait At<T> {
    fn at(self, node: &Node) -> Result<T, Diagnostic>;
}
impl<T> At<T> for Result<T, String> {
    fn at(self, node: &Node) -> Result<T, Diagnostic> {
        self.map_err(|message| node.fail("AML_GEOMETRY", message))
    }
}
#[derive(Clone, Default)]
struct Product {
    solids: Vec<Solid>,
    render: Vec<Solid>,
    patches: Vec<(Geometry, u32)>,
}
#[derive(Clone)]
struct Surface {
    node: usize,
    material: String,
    visible: bool,
    collidable: bool,
    assigned: bool,
}
struct Compiler<'a> {
    cancelled: &'a (dyn Fn() -> bool + Sync),
    validate_image: &'a dyn Fn(&[u8]) -> Result<crate::assets::image_admission::ImageInfo, String>,
    assets: crate::assets::Sources<'a>,
    resolution: Resolution,
    cache: BTreeMap<usize, Product>,
    pending: BTreeSet<usize>,
    surfaces: BTreeMap<u32, Surface>,
    floors: BTreeMap<usize, Profile>,
    water: BTreeMap<usize, Profile>,
    floor_thickness: BTreeMap<usize, f64>,
    wall_paths: BTreeMap<usize, Vec<V3>>,
    wall_offsets: BTreeMap<usize, Vec<V3>>,
    wall_silhouettes: BTreeMap<usize, Vec<V3>>,
    window_openings: BTreeMap<usize, usize>,
    window_fits: BTreeMap<usize, windows::Fit>,
    flights: BTreeMap<usize, flights::Flight>,
    volumes: Vec<context::Volume>,
    authored: Vec<([V3; 3], u32)>,
    incidents: Vec<joins::Incident>,
    generated_triangles: usize,
}

impl Compiler<'_> {
    fn asset(&self, path: &str) -> Result<std::sync::Arc<[u8]>, String> {
        self.assets.read(path)
    }

    fn materials(&self) -> Result<Vec<Material>, Diagnostic> {
        let mut materials = vec![];
        for node in self
            .resolution
            .document
            .declarations
            .values()
            .filter(|node| node.kind == "material")
        {
            materials.push(superspec_aml::material_catalog::registered_material(node)?);
        }
        if !materials.iter().any(|m| m.id == "default") {
            materials.push(Material::default());
        }
        Ok(materials)
    }
    fn load_material_assets(
        &self,
        scene: &mut Scene,
        fallback_source: Option<&Node>,
    ) -> Result<(), Diagnostic> {
        let mut loaded: BTreeSet<_> = scene.manifest.assets.iter().map(|a| a.id.clone()).collect();
        let initial_pixels = scene.image_pixels();
        let mut pixels = initial_pixels;
        for material in &scene.manifest.materials {
            for path in material.companions() {
                if !loaded.insert(path.to_owned()) {
                    continue;
                }
                let source = self
                    .resolution
                    .document
                    .declarations
                    .get(&material.id)
                    .or(fallback_source)
                    .unwrap_or(&self.resolution.document.root);
                let bytes = self.asset(path).map_err(|e| source.fail("AML_ASSET", e))?;
                if bytes.len() > superspec_types::limits::MAX_IMAGE_BYTES
                    || scene.asset_bytes.len() + bytes.len() > crate::MAX_SCENE_IMAGE_BYTES
                {
                    return Err(
                        source.fail("AML_COMPLEXITY", format!("Material asset byte budget exceeded at {path}: {} bytes in image, {} in bundle.", bytes.len(), scene.asset_bytes.len() + bytes.len()))
                    );
                }
                let image =
                    (self.validate_image)(&bytes).map_err(|e| source.fail("AML_ASSET", e))?;
                pixels += image.width * image.height;
                if pixels > crate::MAX_SCENE_IMAGE_PIXELS {
                    return Err(source.fail(
                        "AML_COMPLEXITY",
                        format!("Material bundle decoded pixel budget exceeded at {path} ({pixels} pixels)."),
                    ));
                }
                let mime = image.mime;
                scene.manifest.assets.push(crate::Asset {
                    id: path.to_owned(),
                    mime,
                    byte_start: scene.asset_bytes.len(),
                    byte_length: bytes.len(),
                });
                scene.asset_bytes.extend_from_slice(&bytes);
            }
        }
        scene
            .reserve_image_pixels(pixels - initial_pixels)
            .map_err(|error| self.resolution.document.root.fail("AML_COMPLEXITY", error))?;
        scene.manifest.asset_byte_count = scene.asset_bytes.len();
        Ok(())
    }
    fn owner(&mut self, index: usize, material: Option<String>) -> Result<u32, Diagnostic> {
        let node = &self.resolution.nodes[index];
        let material = material.unwrap_or_else(|| node.source.string("material", "default"));
        if material != "default"
            && !self
                .resolution
                .document
                .declarations
                .get(&material)
                .is_some_and(|n| n.kind == "material")
        {
            return Err(node
                .source
                .fail("AML_REFERENCE", format!("Unknown material {material}.")));
        }
        let owner = Solid::reserve_owner();
        self.surfaces.insert(
            owner,
            Surface {
                node: index,
                material,
                visible: node.source.boolean("visible", true),
                collidable: node.source.boolean("collidable", true),
                assigned: false,
            },
        );
        Ok(owner)
    }
    fn path(&self, node: &Node, key: &str) -> Result<Vec<V3>, Diagnostic> {
        let id = node
            .text(key)
            .ok_or_else(|| node.fail("AML_REFERENCE", format!("Missing {key}.")))?;
        self.resolution
            .paths
            .get(id)
            .cloned()
            .ok_or_else(|| node.fail("AML_REFERENCE", format!("Unknown path {id}.")))
    }
    fn profile(&self, node: &Node) -> Result<Profile, Diagnostic> {
        let id = node
            .text("profile")
            .ok_or_else(|| node.fail("AML_REFERENCE", "Missing profile."))?;
        self.resolution
            .profiles
            .get(id)
            .cloned()
            .ok_or_else(|| node.fail("AML_REFERENCE", format!("Unknown profile {id}.")))
    }
    fn build(&mut self, index: usize) -> Result<Product, Diagnostic> {
        if let Some(product) = self.cache.get(&index) {
            return Ok(product.clone());
        }
        let node = self.resolution.nodes[index].clone();
        if !self.pending.insert(index) {
            return Err(node
                .source
                .fail("AML_CYCLE", "Cyclic architectural relationship."));
        }
        if self.pending.len() > 64 {
            return Err(node
                .source
                .fail("AML_COMPLEXITY", "Architectural dependency depth exceeded."));
        }
        let mut product = self.lower(index, &node)?;
        let triangles = product
            .solids
            .iter()
            .map(Solid::triangle_count)
            .sum::<usize>()
            + product
                .patches
                .iter()
                .map(|(g, _)| g.triangles.len())
                .sum::<usize>();
        self.generated_triangles = self.generated_triangles.saturating_add(triangles);
        if self.generated_triangles > 4_000_000 {
            return Err(node
                .source
                .fail("AML_COMPLEXITY", "Compilation triangle budget exceeded."));
        }
        if !["opening", "frame", "panel", "infill", "glazing"].contains(&node.source.kind.as_str())
            && !product.solids.is_empty()
        {
            let openings: Vec<_> = self
                .resolution
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.source.kind == "opening")
                .filter_map(|(i, n)| {
                    self.resolution
                        .reference(i, n.source.text("host").unwrap_or(""))
                        .ok()
                        .filter(|host| *host == index)
                        .map(|_| i)
                })
                .collect();
            for opening in openings {
                let cut = self.cutter(opening, index, &product.solids, None)?;
                let host = Solid::union(&product.solids).at(&node.source)?;
                if let Some(&window) = self.window_openings.get(&opening) {
                    let fit = self.fit_window(window, opening, index, &host, &cut)?;
                    self.window_fits.insert(window, fit);
                }
                let overlap = host
                    .boolean(&cut, Operation::Intersection)
                    .at(&node.source)?;
                if overlap.is_empty() || overlap.volume() < 1e-12 {
                    return Err(self.resolution.nodes[opening]
                        .source
                        .fail("AML_OPENING", "Opening does not intersect its named host."));
                }
                let aperture = &self.resolution.nodes[opening].source;
                if aperture.text("mode") == Some("notch") {
                    let placement = self.resolution.node_placement(aperture)?;
                    let shape = opening_shape(aperture, &self.resolution)
                        .at(aperture)?
                        .iter()
                        .map(|p| {
                            let p = placement.point([p[0], 0., p[1]]);
                            [p[0], p[2], 0.]
                        })
                        .collect::<Vec<_>>();
                    let outlines = if let Some(outline) = self.wall_silhouettes.get(&index) {
                        vec![outline.clone()]
                    } else {
                        host.transform(node.matrix.inverse().at(aperture)?)
                            .at(aperture)?
                            .silhouette_xz()
                    };
                    if !outlines.iter().any(|outline| loops_meet(&shape, outline)) {
                        return Err(aperture.fail(
                            "AML_OPENING",
                            "A notch must reach an edge of its host silhouette.",
                        ));
                    }
                }
                product.solids = vec![host.boolean(&cut, Operation::Difference).at(&node.source)?];
                product.render = product
                    .render
                    .iter()
                    .map(|s| s.boolean(&cut, Operation::Difference).at(&node.source))
                    .collect::<Result<_, _>>()?;
            }
        }
        let collection = [
            "assembly",
            "group",
            "instance",
            "collision",
            "union",
            "subtract",
            "intersect",
        ]
        .contains(&node.source.kind.as_str());
        if !collection {
            product.render = product.solids.clone();
            for (owner, geometry) in owned_geometry(&product.solids, &product.patches) {
                for t in geometry.triangles {
                    self.authored.push((t.map(|i| geometry.points[i]), owner));
                }
            }
        }
        if !node.source.boolean("visible", true)
            || matches!(node.source.kind.as_str(), "collision" | "window")
        {
            product.render.clear();
        }
        if !node.source.boolean("collidable", true) {
            product.solids.clear();
        }
        self.pending.remove(&index);
        self.cache.insert(index, product.clone());
        Ok(product)
    }
    fn lower(&mut self, index: usize, node: &Resolved) -> Result<Product, Diagnostic> {
        let source = &node.source;
        let kind = source.kind.as_str();
        let mut result = Product::default();
        if [
            "assembly",
            "group",
            "instance",
            "collision",
            "union",
            "subtract",
            "intersect",
        ]
        .contains(&kind)
        {
            for child in &node.children {
                let product = self.build(*child)?;
                result.solids.extend(product.solids);
                result.render.extend(product.render);
                result.patches.extend(product.patches);
            }
            if kind == "assembly" {
                let joins = self.joins(index)?;
                result.solids.extend(joins.solids);
                result.render.extend(joins.render);
            }
            if ["union", "subtract", "intersect"].contains(&kind) {
                if node.children.len() < 2
                    || result.solids.len() != node.children.len()
                    || !result.patches.is_empty()
                {
                    return Err(source.fail(
                        "AML_BOOLEAN",
                        "Boolean operations require at least two closed native operands.",
                    ));
                }
                let operation = match kind {
                    "union" => Operation::Union,
                    "subtract" => Operation::Difference,
                    _ => Operation::Intersection,
                };
                let states = result
                    .solids
                    .iter()
                    .flat_map(|s| s.mesh().run_original_id)
                    .filter_map(|id| self.surfaces.get(&id))
                    .map(|s| {
                        self.resolution.nodes[s.node]
                            .source
                            .boolean("dynamic", false)
                    })
                    .collect::<BTreeSet<_>>();
                if states.len() > 1 {
                    return Err(source.fail(
                        "AML_BOOLEAN",
                        "One boolean cannot combine static and dynamic collision states.",
                    ));
                }
                let mut solid = result.solids[0].clone();
                for operand in &result.solids[1..] {
                    solid = solid.boolean(operand, operation).at(source)?;
                }
                result.solids = vec![solid];
                result.render = result.solids.clone();
            } else if ["assembly", "collision"].contains(&kind) && !result.solids.is_empty() {
                result.solids = self.compose(result.solids, source)?;
                if !result.render.is_empty() {
                    result.render = self.compose(result.render, source)?;
                }
            }
            if kind == "collision" {
                for surface in self.surfaces.values_mut() {
                    let mut current = Some(surface.node);
                    while let Some(id) = current {
                        if id == index {
                            surface.visible = false;
                            break;
                        }
                        current = self.resolution.nodes[id].parent;
                    }
                }
            }
        } else if ["box", "wedge", "cylinder", "cone", "sphere"].contains(&kind) {
            let radius = positive(source, "radius", 0.5)?;
            let tolerance = self
                .resolution
                .document
                .root
                .number("curve-tolerance", 0.01);
            let segments = source.number(
                "segments",
                (std::f64::consts::PI / acos((1. - tolerance / radius).max(-1.)))
                    .ceil()
                    .max(12.),
            );
            if segments.fract() != 0. || !(3.0..=16384.).contains(&segments) {
                return Err(source.fail("AML_COMPLEXITY", "Primitive segment budget exceeded."));
            }
            let mut solid = match kind {
                "sphere" => Solid::sphere(radius, segments as usize).at(source)?,
                "cone" | "cylinder" => {
                    let height = positive(source, "height", 1.)?;
                    let top = if kind == "cylinder" {
                        radius
                    } else {
                        source.number("top-radius", 0.)
                    };
                    if top < 0. {
                        return Err(source.fail("AML_GEOMETRY", "Top radius cannot be negative."));
                    }
                    Solid::cylinder(height, radius, top, segments as usize).at(source)?
                }
                _ => {
                    let size = [
                        positive(source, "width", 1.)?,
                        positive(source, "depth", 1.)?,
                        positive(source, "height", 1.)?,
                    ];
                    let mut solid = Solid::cube(size).at(source)?;
                    if kind == "wedge" {
                        solid = solid.trim([-size[2], 0., size[0]], 0.).at(source)?;
                    }
                    if source.text("anchor") == Some("bottom-center") {
                        solid = solid
                            .transform(
                                Matrix::placement(
                                    [-size[0] / 2., -size[1] / 2., 0.],
                                    [0.; 3],
                                    [1.; 3],
                                )
                                .at(source)?,
                            )
                            .at(source)?;
                    }
                    solid
                }
            };
            let owner = self.owner(index, None)?;
            solid = solid
                .owned(owner)
                .at(source)?
                .transform(node.matrix)
                .at(source)?;
            result.solids.push(solid);
        } else if ["slab", "landing", "roof", "canopy", "extrusion"].contains(&kind) {
            let mut profile = self.profile(source)?;
            let elevation = self.resolution.elevation(source)?;
            for point in &mut profile.points {
                point[2] = if source.boolean("flat", false) {
                    elevation
                } else {
                    point[2] + elevation
                };
            }
            for ring in &mut profile.loops {
                for point in ring {
                    point[2] = if source.boolean("flat", false) {
                        elevation
                    } else {
                        point[2] + elevation
                    };
                }
            }
            let thickness = if kind == "extrusion" {
                positive(source, "distance", 0.)?
            } else {
                positive(source, "thickness", 0.2)?
            };
            if kind == "extrusion" {
                for p in &mut profile.points {
                    p[2] += thickness;
                }
            }
            let geometry = Geometry::surface(&profile, |p| [p[0], p[1], p[2] - thickness])
                .transformed(node.matrix);
            let owner = self.owner(index, None)?;
            result
                .solids
                .push(Solid::from_geometry(&geometry, owner).at(source)?);
            if kind != "extrusion" {
                self.floors.insert(index, profile.transformed(node.matrix));
                self.floor_thickness.insert(index, thickness);
            }
        } else if kind == "shared-edge" {
            for key in ["from", "to"] {
                self.resolution
                    .reference(index, source.text(key).unwrap())?;
            }
            self.path(source, "path")?;
        } else if kind == "face-material" {
            self.validate_assignment(index)?;
        } else if ["room", "zone", "volume", "clearance"].contains(&kind) {
            let volume = self.volume(node)?;
            self.volumes.push(volume);
        } else if kind == "terrain" {
            let profile = self.terrain(index, node)?;
            let owner = self.owner(index, None)?;
            if let Some(base) = source.optional_number("base") {
                let inverse = node.matrix.inverse().at(source)?;
                if profile.points.iter().any(|p| inverse.point(*p)[2] <= base) {
                    return Err(source.fail(
                        "AML_TERRAIN",
                        "Closure base must lie below the entire local surface.",
                    ));
                }
                let geometry = Geometry::surface(&profile, |p| {
                    let local = inverse.point(p);
                    node.matrix.point([local[0], local[1], base])
                });
                result
                    .solids
                    .push(Solid::from_geometry(&geometry, owner).at(source)?);
            } else {
                result.patches.push((
                    Geometry {
                        points: profile.points.clone(),
                        triangles: profile.triangles.clone(),
                    },
                    owner,
                ));
            }
            self.floors.insert(index, profile);
        } else if kind == "grade" {
            let host = self
                .resolution
                .reference(index, source.text("host").unwrap())?;
            if self.resolution.nodes[host].source.kind != "terrain" {
                return Err(source.fail("AML_GRADE", "Grade host must be terrain."));
            }
        } else if kind == "water" {
            let mut profile = self.profile(source)?;
            let elevation = self.resolution.elevation(source)?;
            for p in &mut profile.points {
                p[2] += elevation;
            }
            profile = profile.transformed(node.matrix);
            if profile
                .points
                .iter()
                .any(|p| (p[2] - profile.points[0][2]).abs() > 1e-7)
            {
                return Err(source.fail("AML_WATER", "Water must be a horizontal planar surface."));
            }
            let owner = self.owner(index, None)?;
            self.surfaces.get_mut(&owner).unwrap().collidable = false;
            self.water.insert(index, profile.clone());
            result.patches.push((
                Geometry {
                    points: profile.points,
                    triangles: profile.triangles,
                },
                owner,
            ));
        } else if kind == "faceted-solid" || kind == "collision-surface" {
            let geometry = Geometry {
                points: source.points("points"),
                triangles: source.triangles(),
            }
            .transformed(node.matrix);
            terrain::validate(&geometry).at(source)?;
            let owner = self.owner(index, None)?;
            if kind == "faceted-solid" {
                result
                    .solids
                    .push(Solid::from_geometry(&geometry, owner).at(source)?);
            } else {
                self.surfaces.get_mut(&owner).unwrap().visible = false;
                result.patches.push((geometry, owner));
            }
        } else if kind == "visual-mesh" {
            if source.boolean("collidable", false) {
                return Err(source.fail(
                    "AML_ASSET",
                    "Imported visual meshes require a separate native collision proxy.",
                ));
            }
            let name = source.text("src").unwrap();
            if !name.to_ascii_lowercase().ends_with(".glb") {
                return Err(source.fail(
                    "AML_ASSET",
                    "Imported visualization geometry requires a GLB asset.",
                ));
            }
            let geometry =
                crate::itemspec::imports::glb(&self.asset(name).at(source)?, self.cancelled)
                    .at(source)?
                    .transformed(node.matrix);
            let owner = self.owner(index, None)?;
            self.surfaces.get_mut(&owner).unwrap().collidable = false;
            result.patches.push((geometry, owner));
        } else if kind == "retaining-wall" {
            let geometries = self.retaining(index, node)?;
            let owner = self.owner(index, None)?;
            for geometry in geometries {
                result
                    .solids
                    .push(Solid::from_geometry(&geometry, owner).at(source)?);
            }
            if result.solids.len() > 1 {
                result.solids = vec![Solid::union(&result.solids).at(source)?];
            }
        } else if kind == "enclosure" {
            let geometries = self.enclosure(index, node)?;
            let owner = self.owner(index, None)?;
            let solids = geometries
                .iter()
                .map(|geometry| Solid::from_geometry(geometry, owner).at(source))
                .collect::<Result<Vec<_>, _>>()?;
            result.solids = vec![Solid::union(&solids).at(source)?];
        } else if kind == "stairs" || kind == "ramp" {
            let flight = self.flight(node)?;
            let owner = self.owner(index, None)?;
            let solids = flight
                .solids
                .iter()
                .map(|geometry| Solid::from_geometry(geometry, owner).at(source))
                .collect::<Result<Vec<_>, _>>()?;
            result.solids = if source.text("underside") == Some("separate") {
                solids
            } else {
                vec![Solid::union(&solids).at(source)?]
            };
            self.floors.insert(index, flight.floor.clone());
            self.floor_thickness
                .insert(index, source.number("thickness", 0.2));
            self.flights.insert(index, flight);
        } else if kind == "wall" {
            let geometries = self.wall(index, node)?;
            let owner = self.owner(index, None)?;
            for geometry in geometries {
                result
                    .solids
                    .push(Solid::from_geometry(&geometry, owner).at(source)?);
            }
            if result.solids.len() > 1 {
                result.solids = vec![Solid::union(&result.solids).at(source)?];
            }
        } else if ["sweep", "beam", "column"].contains(&kind) {
            let profile = self.profile(source)?;
            let path = self.path(source, "path")?;
            let geometry = sweep(&profile, &path, node.matrix).at(source)?;
            let owner = self.owner(index, None)?;
            result
                .solids
                .push(Solid::from_geometry(&geometry, owner).at(source)?);
        } else if ["panel", "glazing", "infill", "frame"].contains(&kind) {
            let opening = self
                .resolution
                .reference(index, source.text("opening").unwrap())?;
            if self.resolution.nodes[opening].source.kind != "opening" {
                return Err(source.fail("AML_REFERENCE", "Detail must reference an opening."));
            }
            let host = self.resolution.reference(
                opening,
                self.resolution.nodes[opening].source.text("host").unwrap(),
            )?;
            let product = self.build(host)?;
            let solid = self.cutter(
                opening,
                host,
                &product.solids,
                Some((
                    positive(source, "depth", 0.03)?,
                    source.number("offset", 0.),
                    if kind == "frame" {
                        Some(positive(source, "border", 0.05)?)
                    } else {
                        None
                    },
                )),
            )?;
            let owner = self.owner(index, None)?;
            let host_matrix = self.resolution.nodes[host].matrix;
            let placement = host_matrix
                .then(self.resolution.node_placement(source)?)
                .then(host_matrix.inverse().at(source)?);
            result.solids.push(
                solid
                    .owned(owner)
                    .at(source)?
                    .transform(placement)
                    .at(source)?,
            );
        } else if kind == "window" {
            result.solids = self.window_collision(index, source)?;
        } else if kind == "opening" {
            self.resolution
                .reference(index, source.text("host").unwrap())?;
        } else {
            return Err(source.fail(
                "AML_UNSUPPORTED",
                format!("Architectural lowering for {kind} is not yet implemented."),
            ));
        }
        Ok(result)
    }
    fn cutter(
        &mut self,
        opening: usize,
        host: usize,
        members: &[Solid],
        detail: Option<(f64, f64, Option<f64>)>,
    ) -> Result<Solid, Diagnostic> {
        let node = self.resolution.nodes[opening].clone();
        let host_node = self.resolution.nodes[host].clone();
        let source = &node.source;
        let shape = opening_shape(source, &self.resolution).at(source)?;
        let bounds = Solid::union(members)
            .at(source)?
            .transform(host_node.matrix.inverse().at(source)?)
            .at(source)?
            .bounds();
        let wall = host_node.source.kind == "wall";
        let thickness = host_node.source.number("thickness", 0.22);
        let negative = if wall {
            match host_node.source.text("alignment") {
                Some("left") => 0.,
                Some("right") => -thickness,
                _ => -thickness / 2.,
            }
        } else {
            bounds[0][1]
        };
        let mut low = if wall { negative - 0.01 } else { negative - 1. };
        let mut high = if wall {
            negative + thickness + 0.01
        } else {
            bounds[1][1] + 1.
        };
        if let Some((depth, offset, _)) = detail {
            low = offset;
            high = offset + depth;
        } else if source.text("mode") == Some("recess") {
            let depth = positive(source, "depth", 0.)?;
            let offset = source.number("offset", 0.);
            let width = if wall {
                thickness
            } else {
                bounds[1][1] - bounds[0][1]
            };
            if offset < 0. || offset + depth >= width - 1e-7 {
                return Err(source.fail("AML_OPENING", "Recess must leave a back web."));
            }
            low = negative + offset;
            high = low + depth;
        }
        let map = Matrix([[1., 0., 0.], [0., 0., 1.], [0., -1., 0.], [0., high, 0.]]);
        let mut cut = Solid::extrude(std::slice::from_ref(&shape), high - low)
            .at(source)?
            .transform(map)
            .at(source)?;
        if let Some((_, _, Some(border))) = detail {
            let inset = Solid::inset(&shape, border);
            if inset.is_empty() {
                return Err(source.fail("AML_OPENING", "Frame border consumes the whole opening."));
            }
            let inner = Solid::extrude(&inset, high - low + 2.)
                .at(source)?
                .transform(Matrix([
                    [1., 0., 0.],
                    [0., 0., 1.],
                    [0., -1., 0.],
                    [0., high + 1., 0.],
                ]))
                .at(source)?;
            cut = cut.boolean(&inner, Operation::Difference).at(source)?;
        }
        cut = cut
            .transform(self.resolution.node_placement(source)?)
            .at(source)?;
        if wall {
            let path = self
                .wall_paths
                .get(&host)
                .cloned()
                .ok_or_else(|| source.fail("AML_OPENING", "Wall path not compiled."))?;
            let offsets = &self.wall_offsets[&host];
            let stations = stations(&path);
            let mut pieces = vec![];
            for i in 1..path.len() {
                let start = stations[i - 1];
                let end = stations[i];
                let piece = cut
                    .trim([1., 0., 0.], start)
                    .at(source)?
                    .trim([-1., 0., 0.], -end)
                    .at(source)?;
                if piece.is_empty() {
                    continue;
                }
                let piece = piece
                    .warp(|p| {
                        let station = if (p[0] - start).abs() < 1e-7 {
                            start
                        } else if (p[0] - end).abs() < 1e-7 {
                            end
                        } else {
                            p[0]
                        };
                        let center = at(&path, station);
                        let normal = normal_at(&path, offsets, station);
                        [
                            center[0] + normal[0] * p[1],
                            center[1] + normal[1] * p[1],
                            center[2] + p[2],
                        ]
                    })
                    .at(source)?;
                pieces.push(piece);
            }
            if pieces.is_empty() {
                return Err(source.fail("AML_OPENING", "Opening lies outside host path."));
            }
            cut = Solid::union(&pieces).at(source)?;
        }
        let owner = self.owner(
            opening,
            Some(source.string(
                "reveal-material",
                &host_node.source.string("material", "default"),
            )),
        )?;
        if let Some(surface) = self.surfaces.get_mut(&owner) {
            surface.visible = host_node.source.boolean("visible", true);
            surface.collidable = host_node.source.boolean("collidable", true);
        }
        cut.owned(owner)
            .at(source)?
            .transform(host_node.matrix)
            .at(source)
    }
    fn emit_collision(
        &mut self,
        scene: &mut Scene,
        solids: &[Solid],
        patches: &[(Geometry, u32)],
    ) -> Result<(), Diagnostic> {
        self.append_owned(scene, owned_geometry(solids, patches), true)
    }
    fn append_owned(
        &mut self,
        scene: &mut Scene,
        by_owner: BTreeMap<u32, Geometry>,
        collision: bool,
    ) -> Result<(), Diagnostic> {
        let mut by_node: BTreeMap<(usize, String), Geometry> = BTreeMap::new();
        for (owner, geometry) in by_owner {
            let surface = self.surfaces.get(&owner).ok_or_else(|| {
                self.resolution
                    .document
                    .root
                    .fail("AML_KERNEL", "Lost kernel source ownership.")
            })?;
            if (collision && !surface.collidable) || (!collision && !surface.visible) {
                continue;
            }
            let output = by_node
                .entry((surface.node, surface.material.clone()))
                .or_default();
            let base = output.points.len();
            output.points.extend(geometry.points);
            output
                .triangles
                .extend(geometry.triangles.into_iter().map(|t| t.map(|i| i + base)));
        }
        self.append_nodes(scene, by_node, collision)
    }
    fn append_nodes(
        &mut self,
        scene: &mut Scene,
        by_node: BTreeMap<(usize, String), Geometry>,
        collision: bool,
    ) -> Result<(), Diagnostic> {
        for ((index, material), geometry) in by_node {
            let node = &self.resolution.nodes[index];
            let vertex_start = scene.positions.len() / 3;
            let index_start = scene.indices.len();
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for t in &geometry.triangles {
                if scene.positions.len() + 9 > 18_000_000 || scene.indices.len() + 3 > 6_000_000 {
                    return Err(node
                        .source
                        .fail("AML_COMPLEXITY", "Scene geometry budget exceeded."));
                }
                let points = t.map(|i| geometry.points[i]);
                let vector = cross(sub(points[1], points[0]), sub(points[2], points[0]));
                // Kernel output is f32. Zero-area triangles created by that storage
                // boundary carry no surface and follow the existing export policy.
                if length(vector) < 1e-12 {
                    continue;
                }
                let normal = unit(vector).at(&node.source)?;
                let base = (scene.positions.len() / 3 - vertex_start) as u32;
                for p in points {
                    scene.positions.extend(p.map(|v| v as f32));
                    scene.normals.extend(normal.map(|v| v as f32));
                    scene.colors.extend([1.; 3]);
                    let repeat = scene
                        .manifest
                        .materials
                        .iter()
                        .find(|m| m.id == material)
                        .map_or(1., |m| m.repeat);
                    let uv = if normal[2].abs() > 0.8 {
                        [p[0], p[1]]
                    } else if normal[1].abs() > 0.8 {
                        [p[0], p[2]]
                    } else {
                        [p[1], p[2]]
                    };
                    scene.uvs.extend(uv.map(|v| (v * repeat) as f32));
                    for axis in 0..3 {
                        bounds[0][axis] = bounds[0][axis].min(p[axis]);
                        bounds[1][axis] = bounds[1][axis].max(p[axis]);
                    }
                }
                scene.indices.extend([base, base + 1, base + 2]);
            }
            if scene.positions.len() / 3 == vertex_start {
                continue;
            }
            let surfaces: Vec<_> = self
                .surfaces
                .values()
                .filter(|s| s.node == index && s.material == material)
                .collect();
            let visible = !collision && surfaces.iter().any(|s| s.visible);
            let collidable = collision;
            let base_id = if collision {
                format!("$collision/{}", node.id)
            } else {
                node.id.clone()
            };
            let id = if scene.manifest.meshes.iter().any(|m| m.id == base_id) {
                format!("{base_id}/$material/{material}")
            } else {
                base_id
            };
            if collidable {
                scene.manifest.colliders.push(id.clone());
            }
            scene.manifest.meshes.push(Mesh {
                id,
                object_id: node.id.clone(),
                source: node.source.source.clone(),
                material: if collision { None } else { Some(material) },
                visible,
                vertex_start,
                vertex_count: scene.positions.len() / 3 - vertex_start,
                index_start,
                index_count: scene.indices.len() - index_start,
                bounds,
            });
        }
        Ok(())
    }
}

fn positive(node: &Node, key: &str, default: f64) -> Result<f64, Diagnostic> {
    let value = node.number(key, default);
    if value <= 0. {
        Err(node.fail("AML_ATTRIBUTE", format!("{key} must be positive.")))
    } else {
        Ok(value)
    }
}
fn path_offsets(path: &[V3]) -> Result<Vec<V3>, String> {
    if path.len() < 2 {
        return Err("A path needs two points.".into());
    }
    let mut result = vec![];
    for i in 0..path.len() {
        let before = sub(path[i.max(1)], path[i.max(1) - 1]);
        let j = (i + 1).min(path.len() - 1);
        let after = sub(path[j], path[j - 1]);
        let a = unit([-before[1], before[0], 0.])?;
        let b = unit([-after[1], after[0], 0.])?;
        let denominator = 1. + dot(a, b);
        if denominator < 0.1 {
            return Err("Path turns too sharply for a miter.".into());
        }
        result.push(scale(add(a, b), 1. / denominator));
    }
    Ok(result)
}
fn loops_meet(a: &[V3], b: &[V3]) -> bool {
    let on_segment = |p: V3, a: V3, b: V3| {
        orient(a, b, p).abs() < 1e-7
            && p[0] >= a[0].min(b[0]) - 1e-7
            && p[0] <= a[0].max(b[0]) + 1e-7
            && p[1] >= a[1].min(b[1]) - 1e-7
            && p[1] <= a[1].max(b[1]) + 1e-7
    };
    (0..a.len()).any(|i| {
        (0..b.len()).any(|j| {
            let (p, q, r, s) = (a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]);
            (orient(p, q, r) * orient(p, q, s) < 0. && orient(r, s, p) * orient(r, s, q) < 0.)
                || on_segment(p, r, s)
                || on_segment(q, r, s)
                || on_segment(r, p, q)
                || on_segment(s, p, q)
        })
    })
}
fn normal_at(path: &[V3], offsets: &[V3], station: f64) -> V3 {
    let cuts = stations(path);
    for (i, &cut) in cuts.iter().enumerate() {
        if (cut - station).abs() < 1e-7 {
            return offsets[i];
        }
    }
    let i = cuts
        .partition_point(|s| *s < station)
        .clamp(1, path.len() - 1);
    let d = sub(path[i], path[i - 1]);
    let length = hypot(d[0], d[1]);
    [-d[1] / length, d[0] / length, 0.]
}
fn opening_shape(node: &Node, resolution: &Resolution) -> Result<Vec<V3>, String> {
    let [x, z] = node.vector("center", [0.; 2]);
    let tolerance = resolution.document.root.number("curve-tolerance", 0.01);
    let shape = node.text("shape").unwrap_or("rectangle");
    if shape == "profile" {
        let profile = resolution
            .profiles
            .get(node.text("profile").unwrap_or(""))
            .ok_or("Unknown opening profile.")?;
        if profile.loops.len() != 1 {
            return Err("Opening profile must have one loop.".into());
        }
        return Ok(profile.loops[0]
            .iter()
            .map(|p| [x + p[0], z + p[1], 0.])
            .collect());
    }
    let width = node.number("width", 1.);
    let height = node.number("height", 2.);
    if width <= 0. || height <= 0. {
        return Err("Opening dimensions must be positive.".into());
    }
    if shape == "rectangle" {
        return Ok(vec![
            [x - width / 2., z - height / 2., 0.],
            [x + width / 2., z - height / 2., 0.],
            [x + width / 2., z + height / 2., 0.],
            [x - width / 2., z + height / 2., 0.],
        ]);
    }
    let radius = node.number("radius", 0.5);
    let [rx, rz] = if shape == "circle" {
        [radius, radius]
    } else if shape == "arch" {
        [width / 2., width / 2.]
    } else {
        node.vector("radii", [0.5; 2])
    };
    if rx <= 0. || rz <= 0. {
        return Err("Opening radii must be positive.".into());
    }
    let count = (std::f64::consts::PI / acos((1. - tolerance / rx.max(rz)).max(-1.)))
        .ceil()
        .max(12.) as usize;
    if count > 16384 {
        return Err("Opening curve budget exceeded.".into());
    }
    if shape == "arch" {
        let shoulder = z + height / 2. - rx;
        if shoulder < z - height / 2. {
            return Err("Arch height cannot fit its crown.".into());
        }
        let mut points = vec![
            [x - width / 2., z - height / 2., 0.],
            [x + width / 2., z - height / 2., 0.],
        ];
        let n = count.div_ceil(2);
        for i in 0..=n {
            let a = std::f64::consts::PI * i as f64 / n as f64;
            points.push([x + rx * cos(a), shoulder + rx * sin(a), 0.]);
        }
        Ok(points)
    } else {
        Ok((0..count)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / count as f64;
                [x + rx * cos(a), z + rz * sin(a), 0.]
            })
            .collect())
    }
}
fn sweep(profile: &Profile, path: &[V3], matrix: Matrix) -> Result<Geometry, String> {
    if profile
        .points
        .len()
        .checked_mul(path.len())
        .is_none_or(|count| count > 500_000)
    {
        return Err("Sweep vertex budget exceeded before allocation.".into());
    }
    if profile
        .triangles
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_mul(path.len()))
        .is_none_or(|n| n > 1_000_000)
    {
        return Err("Sweep repeated cap budget exceeded before allocation.".into());
    }
    if profile.points.iter().any(|p| p[2].abs() > 1e-7)
        || path.len() < 2
        || length(sub(path[0], *path.last().unwrap())) < 1e-8
    {
        return Err("Sweep requires a planar XY section and an open nonzero path.".into());
    }
    let directions = path
        .windows(2)
        .map(|p| unit(sub(p[1], p[0])))
        .collect::<Result<Vec<_>, _>>()?;
    let horizontal = cross([0., 0., 1.], directions[0]);
    let mut x = if length(horizontal) < 1e-8 {
        [1., 0., 0.]
    } else {
        unit(horizontal)?
    };
    let mut sections = vec![];
    for i in 0..path.len() {
        let incoming = directions[i.saturating_sub(1)];
        let outgoing = directions[i.min(directions.len() - 1)];
        let cosine = dot(incoming, outgoing);
        if cosine < -1. + 1e-7 {
            return Err("Sweep reverses at a cusp.".into());
        }
        let normal = unit(add(incoming, outgoing))?;
        let y = unit(cross(incoming, x))?;
        let section = profile
            .points
            .iter()
            .map(|p| {
                let offset = add(scale(x, p[0]), scale(y, p[1]));
                add(
                    path[i],
                    add(
                        offset,
                        scale(incoming, -dot(offset, normal) / dot(incoming, normal)),
                    ),
                )
            })
            .collect::<Vec<_>>();
        sections.push(section);
        let axis = cross(incoming, outgoing);
        x = add(
            add(x, cross(axis, x)),
            scale(cross(axis, cross(axis, x)), 1. / (1. + cosine)),
        );
    }
    for i in 1..sections.len() {
        for (a, b) in sections[i - 1].iter().zip(&sections[i]) {
            if dot(sub(*b, *a), directions[i - 1]) <= 1e-8 {
                return Err("Sweep sections fold or overlap.".into());
            }
        }
    }
    validate_sweep(&sections, &profile.triangles, matrix)?;
    Ok(Geometry::loft(&sections, &profile.triangles)?.transformed(matrix))
}

/// Rejects actual nonadjacent overlap before composition can hide it; touching faces are permitted.
fn validate_sweep(sections: &[Vec<V3>], caps: &[[usize; 3]], matrix: Matrix) -> Result<(), String> {
    let mut bounds = vec![];
    for pair in sections.windows(2) {
        let mut extent = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for p in pair.iter().flatten() {
            let p = matrix.point(*p);
            for axis in 0..3 {
                extent[0][axis] = extent[0][axis].min(p[axis]);
                extent[1][axis] = extent[1][axis].max(p[axis]);
            }
        }
        bounds.push(extent);
    }
    let mut solids = BTreeMap::new();
    let mut checks = 0;
    let owner = Solid::reserve_owner();
    for i in 0..bounds.len() {
        for j in i + 2..bounds.len() {
            checks += 1;
            if checks > 1_000_000 {
                return Err("Sweep intersection-check budget exceeded.".into());
            }
            if (0..3).any(|axis| {
                bounds[i][1][axis] <= bounds[j][0][axis] + 1e-7
                    || bounds[j][1][axis] <= bounds[i][0][axis] + 1e-7
            }) {
                continue;
            }
            for k in [i, j] {
                if let std::collections::btree_map::Entry::Vacant(entry) = solids.entry(k) {
                    let geometry = Geometry::loft(&sections[k..k + 2], caps)?.transformed(matrix);
                    entry.insert(Solid::from_geometry(&geometry, owner)?);
                }
            }
            if solids[&i]
                .boolean(&solids[&j], Operation::Intersection)?
                .volume()
                > 1e-9
            {
                return Err("Sweep intersects itself.".into());
            }
        }
    }
    Ok(())
}

fn owned_geometry(solids: &[Solid], patches: &[(Geometry, u32)]) -> BTreeMap<u32, Geometry> {
    let mut by_owner: BTreeMap<u32, Geometry> = BTreeMap::new();
    for solid in solids {
        let mesh = solid.mesh();
        for (run, &owner) in mesh.run_original_id.iter().enumerate() {
            let geometry = by_owner.entry(owner).or_default();
            let start = mesh.run_index[run] as usize;
            let end = mesh.run_index[run + 1] as usize;
            for triangle in mesh.tri_verts[start..end].as_chunks::<3>().0 {
                let base = geometry.points.len();
                for &index in triangle {
                    let offset = index as usize * mesh.num_prop as usize;
                    geometry.points.push([
                        mesh.vert_properties[offset] as f64,
                        mesh.vert_properties[offset + 1] as f64,
                        mesh.vert_properties[offset + 2] as f64,
                    ]);
                }
                geometry.triangles.push([base, base + 1, base + 2]);
            }
        }
    }
    for (geometry, owner) in patches {
        let output = by_owner.entry(*owner).or_default();
        let base = output.points.len();
        output.points.extend(&geometry.points);
        output
            .triangles
            .extend(geometry.triangles.iter().map(|t| t.map(|i| i + base)));
    }

    by_owner
}
