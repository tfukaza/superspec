use super::*;

pub use superspec_model::architecture::{
    Connection, Context, Floor, MouthObstruction, Object, Volume,
};
impl Compiler<'_> {
    pub(super) fn context(&self, product: &Product) -> Result<Context, Diagnostic> {
        let boat = self
            .resolution
            .document
            .metadata
            .iter()
            .find(|n| n.kind == "boat");
        let draft = boat.map(|n| n.number("draft", 0.5)).unwrap_or(0.5);
        let height = boat.map(|n| n.number("height", 2.)).unwrap_or(2.);
        let mut water_obstacles = BTreeMap::new();
        for (index, water) in &self.water {
            if (self.cancelled)() {
                return Err(self.resolution.nodes[*index]
                    .source
                    .fail("CANCELLED", "Water navigation preparation cancelled."));
            }
            let level = water.points[0][2];
            let mut loops = vec![];
            for solid in &product.solids {
                let bounds = solid.bounds();
                if bounds[1][2] <= level - draft + 1e-6 || bounds[0][2] >= level + height - 1e-6 {
                    continue;
                }
                let clipped = solid
                    .trim([0., 0., 1.], level - draft + 1e-6)
                    .and_then(|s| s.trim([0., 0., -1.], -level - height + 1e-6))
                    .at(&self.resolution.nodes[*index].source)?;
                loops.extend(clipped.project_xy());
            }
            water_obstacles.insert(self.resolution.nodes[*index].id.clone(), loops);
        }
        Ok(Context {
            connection_obstructions: self.connection_obstructions(product)?,
            metadata: self.resolution.document.metadata.clone(),
            declarations: self
                .resolution
                .document
                .declarations
                .values()
                .filter(|n| n.kind != "definition")
                .cloned()
                .collect(),
            objects: self
                .resolution
                .nodes
                .iter()
                .map(|n| Object {
                    id: n.id.clone(),
                    source_id: n.source.id.clone(),
                    kind: n.source.kind.clone(),
                    source: n.source.source.clone(),
                    matrix: n.matrix.0,
                    instances: n.instances.clone(),
                    assemblies: n.assemblies.clone(),
                    contributors: vec![],
                    attributes: n.source.attributes.clone(),
                })
                .collect(),
            floors: self
                .floors
                .iter()
                .map(|(id, p)| Floor {
                    id: self.resolution.nodes[*id].id.clone(),
                    area: self.resolution.nodes[*id]
                        .source
                        .string("area", &self.resolution.nodes[*id].id),
                    points: p.points.clone(),
                    triangles: p.triangles.clone(),
                    loops: p.loops.clone(),
                })
                .collect(),
            connections: self
                .resolution
                .document
                .metadata
                .iter()
                .filter(|n| n.kind == "connection")
                .map(|n| Connection {
                    id: n.id.clone(),
                    from: n.string("from", ""),
                    to: n.string("to", ""),
                    source: n.source.clone(),
                    path: n
                        .text("path")
                        .and_then(|id| self.resolution.paths.get(id))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
            water: self
                .water
                .iter()
                .map(|(id, p)| Floor {
                    id: self.resolution.nodes[*id].id.clone(),
                    area: self.resolution.nodes[*id].id.clone(),
                    points: p.points.clone(),
                    triangles: p.triangles.clone(),
                    loops: p.loops.clone(),
                })
                .collect(),
            water_obstacles,
            volumes: self.volumes.clone(),
        })
    }
    pub(super) fn volume(&self, node: &Resolved) -> Result<Volume, Diagnostic> {
        let source = &node.source;
        let height = positive(source, "height", 3.)?;
        if source.kind == "volume"
            && !["spawn", "objective", "trigger", "out-of-bounds"]
                .contains(&source.text("role").unwrap_or(""))
        {
            return Err(source.fail("AML_VOLUME", "Gameplay volumes require a supported role."));
        }
        if source.boolean("require-support", false) && source.kind != "clearance" {
            return Err(source.fail("AML_VOLUME", "require-support is a clearance assertion."));
        }
        let mut profile = self.profile(source)?;
        if profile
            .points
            .iter()
            .any(|p| (p[2] - profile.points[0][2]).abs() > 1e-7)
        {
            return Err(source.fail(
                "AML_VOLUME",
                "Volume bases must be planar in their local XY frame.",
            ));
        }
        let elevation = self.resolution.elevation(source)?;
        for p in &mut profile.points {
            p[2] += elevation;
        }
        for ring in &mut profile.loops {
            for p in ring {
                p[2] += elevation;
            }
        }
        let profile = profile.transformed(node.matrix);
        let extrusion = node.matrix.direction([0., 0., height]);
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for p in &profile.points {
            for p in [*p, add(*p, extrusion)] {
                for i in 0..3 {
                    bounds[0][i] = bounds[0][i].min(p[i]);
                    bounds[1][i] = bounds[1][i].max(p[i]);
                }
            }
        }
        if bounds
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e7)
        {
            return Err(source.fail(
                "AML_VOLUME",
                "Spatial volume exceeds finite world-coordinate bounds.",
            ));
        }
        let mut properties = BTreeMap::new();
        for p in &source.children {
            let name = p.text("name").unwrap();
            if properties
                .insert(name.into(), p.string("value", ""))
                .is_some()
            {
                return Err(p.fail("AML_VOLUME", "Duplicate volume property."));
            }
        }
        Ok(Volume {
            id: node.id.clone(),
            kind: source.kind.clone(),
            role: source.text("role").map(str::to_owned),
            movement: source.string("movement", "land"),
            points: profile.points,
            triangles: profile.triangles,
            loops: profile.loops,
            extrusion,
            require_support: source.boolean("require-support", false),
            properties,
            source: source.source.clone(),
            bounds,
        })
    }
    pub(super) fn validate_relationships(&self) -> Result<(), Diagnostic> {
        let metadata = &self.resolution.document.metadata;
        for source in metadata {
            let reference = |id: &str| {
                self.resolution.ids.get(id).ok_or_else(|| {
                    source.fail(
                        "AML_REFERENCE",
                        format!("Unknown architectural reference {id}."),
                    )
                })
            };
            match source.kind.as_str() {
                "connection" => {
                    for key in ["from", "to"] {
                        reference(source.text(key).ok_or_else(|| {
                            source.fail("AML_REFERENCE", format!("Missing {key}."))
                        })?)?;
                    }
                    if let Some(id) = source.text("path") {
                        let path = self.resolution.paths.get(id).ok_or_else(|| {
                            source.fail("AML_REFERENCE", format!("Unknown connection path {id}."))
                        })?;
                        if *stations(path).last().unwrap_or(&0.) <= 1e-7 {
                            return Err(source.fail(
                                "AML_CONNECTION_PATH",
                                "Connection mouth requires a nonzero horizontal span.",
                            ));
                        }
                    }
                }
                "marker" => {
                    if let Some(id) = source.text("element") {
                        reference(id)?;
                    }
                    if source.text("kind").is_none() || !source.attributes.contains_key("position")
                    {
                        return Err(
                            source.fail("AML_ATTRIBUTE", "Marker requires kind and position.")
                        );
                    }
                }
                "route" | "no-spawn-los" => {
                    let ids = if source.kind == "route" {
                        vec![
                            source.text("from").unwrap_or(""),
                            source.text("to").unwrap_or(""),
                        ]
                    } else {
                        source
                            .text("markers")
                            .unwrap_or("")
                            .split_whitespace()
                            .collect()
                    };
                    if ids.len() != 2
                        || ids
                            .iter()
                            .any(|id| !metadata.iter().any(|n| n.kind == "marker" && n.id == *id))
                    {
                        return Err(source.fail(
                            "AML_REFERENCE",
                            "Route or visibility assertion requires two known markers.",
                        ));
                    }
                    if source.kind == "route" {
                        let count = source.number("min-routes", 1.);
                        if count < 1. || count.fract() != 0. {
                            return Err(source
                                .fail("AML_ROUTE", "Minimum routes must be a positive integer."));
                        }
                        if source
                            .optional_number("min-distance")
                            .is_some_and(|d| d < 0.)
                            || source
                                .optional_number("max-distance")
                                .is_some_and(|d| d < source.number("min-distance", 0.))
                        {
                            return Err(
                                source.fail("AML_ROUTE", "Route distance bounds are invalid.")
                            );
                        }
                    }
                }
                "link" => {
                    for key in ["from-area", "to-area"] {
                        if let Some(id) = source.text(key) {
                            reference(id)?;
                        }
                    }
                }
                "player" | "boat" => {
                    for (key, default) in [("radius", 0.25), ("height", 2.)] {
                        positive(source, key, default)?;
                    }
                    for key in ["step", "crouch", "vault", "draft", "slope"] {
                        if source.optional_number(key).is_some_and(|v| v < 0.) {
                            return Err(
                                source.fail("AML_ATTRIBUTE", format!("{key} cannot be negative."))
                            );
                        }
                    }
                }
                "reference" => {
                    for key in ["width", "height"] {
                        let value = positive(source, key, 0.)?;
                        if value.fract() != 0. {
                            return Err(source.fail(
                                "AML_REFERENCE_VIEW",
                                "Reference image dimensions must be integers.",
                            ));
                        }
                    }
                    let position = source.vector("position", [0.; 3]);
                    let target = source.vector("target", [0.; 3]);
                    let up = source.vector("up", [0., 0., 1.]);
                    let fov = source.number("fov", 45.);
                    let near = positive(source, "near", 0.05)?;
                    let far = positive(source, "far", 2000.)?;
                    if length(sub(target, position)) < 1e-8
                        || length(cross(sub(target, position), up)) < 1e-8
                        || fov <= 0.
                        || fov >= 180.
                        || far <= near
                    {
                        return Err(source.fail(
                            "AML_REFERENCE_VIEW",
                            "Reference camera is degenerate or has invalid clipping planes.",
                        ));
                    }
                    for item in &source.children {
                        if item.kind == "mask" && item.points("points").len() < 3 {
                            return Err(item.fail(
                                "AML_REFERENCE_VIEW",
                                "Reference mask requires at least three points.",
                            ));
                        }
                    }
                }
                "light" | "sky" if source.number("intensity", 1.) < 0. => {
                    return Err(source.fail("AML_ATTRIBUTE", "Light intensity cannot be negative."));
                }
                _ => {}
            }
        }
        for (i, node) in self.resolution.nodes.iter().enumerate() {
            let source = &node.source;
            if let Some(id) = source.text("material-owner") {
                if !["assembly", "union", "intersect", "subtract", "collision"]
                    .contains(&source.kind.as_str())
                    || source.text("coincident-material") == Some("retained-face")
                {
                    return Err(source.fail(
                        "AML_MATERIAL",
                        "Material ownership requires a physical boundary and one ownership policy.",
                    ));
                }
                let owner = self.resolution.reference(i, id)?;
                let mut ancestor = self.resolution.nodes[owner].parent;
                let mut found = false;
                while let Some(parent) = ancestor {
                    if parent == i {
                        found = true;
                        break;
                    }
                    ancestor = self.resolution.nodes[parent].parent;
                }
                if !found {
                    return Err(
                        source.fail("AML_REFERENCE", "Material owner must name a descendant.")
                    );
                }
            }
            if source.kind == "wall" {
                if (source.text("top").is_some() && source.text("top-floor").is_some())
                    || (source.text("base").is_some() && source.text("base-floor").is_some())
                {
                    return Err(source.fail(
                        "AML_ANCHOR",
                        "Choose a longitudinal profile or a floor anchor.",
                    ));
                }
                for key in ["base-floor", "contact-floor", "top-floor"] {
                    if let Some(id) = source.text(key) {
                        let reference = self.resolution.reference(i, id)?;
                        let target = &self.resolution.nodes[reference].source;
                        if !["slab", "landing", "roof", "canopy", "ramp"]
                            .contains(&target.kind.as_str())
                            || !target.boolean("collidable", true)
                        {
                            let mut d = source.fail(
                                "AML_ANCHOR",
                                "Wall anchors require a native physical floor or soffit.",
                            );
                            d.objects.push(self.resolution.nodes[reference].id.clone());
                            return Err(d);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
