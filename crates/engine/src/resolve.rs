use std::collections::{BTreeMap, BTreeSet};
use superspec_aml::{
    architecture::{Document, Node, Value},
    diagnostic::Diagnostic,
};
use superspec_geometry::{Profile, math::*, triangulate};

#[derive(Clone)]
pub struct Resolved {
    pub source: Node,
    pub id: String,
    pub matrix: Matrix,
    pub scope: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub instances: Vec<String>,
    pub assemblies: Vec<String>,
}
pub struct Resolution {
    pub document: Document,
    pub nodes: Vec<Resolved>,
    pub roots: Vec<usize>,
    pub ids: BTreeMap<String, usize>,
    pub paths: BTreeMap<String, Vec<V3>>,
    pub profiles: BTreeMap<String, Profile>,
    pub datums: BTreeMap<String, f64>,
    pub frames: BTreeMap<String, Matrix>,
}
pub fn placement(node: &Node) -> Result<Matrix, Diagnostic> {
    Matrix::placement(
        node.vector("position", [0.; 3]),
        node.vector("rotation", [0.; 3]),
        node.vector("scale", [1.; 3]),
    )
    .map_err(|e| node.fail("AML_PLACEMENT", e))
}

impl Resolution {
    pub fn new(document: Document) -> Result<Self, Diagnostic> {
        let mut result = Self {
            document,
            nodes: vec![],
            roots: vec![],
            ids: BTreeMap::new(),
            paths: BTreeMap::new(),
            profiles: BTreeMap::new(),
            datums: BTreeMap::new(),
            frames: BTreeMap::new(),
        };
        let declarations: Vec<_> = result.document.declarations.values().cloned().collect();
        for node in &declarations {
            match node.kind.as_str() {
                "coordinate-frame" => {
                    result.frame(&node.id, &mut BTreeSet::new())?;
                }
                "datum" => {
                    result.datum(&node.id, &mut BTreeSet::new())?;
                }
                _ => {}
            }
        }
        for node in &declarations {
            if node.kind == "path" {
                result.path(&node.id, &mut BTreeSet::new())?;
            }
        }
        for node in &declarations {
            if node.kind == "profile" {
                let mut loops = vec![result.loop_points(node)?];
                for child in &node.children {
                    if child.kind == "hole" {
                        loops.push(result.loop_points(child)?);
                    }
                }
                let breaklines: Vec<_> = node
                    .children
                    .iter()
                    .filter(|child| child.kind == "breakline")
                    .map(|child| child.points("points"))
                    .collect();
                let profile = triangulate(loops, &node.points("interior"), &breaklines)
                    .map_err(|e| node.fail("AML_PROFILE", e))?;
                let matrix = result.node_placement(node)?;
                result
                    .profiles
                    .insert(node.id.clone(), profile.transformed(matrix));
            }
        }
        for node in result.document.elements.clone() {
            let root = result.expand(node, "", None, vec![], vec![], &mut vec![])?;
            result.roots.push(root);
        }
        result.attach_enclosures()?;
        Ok(result)
    }
    fn attach_enclosures(&mut self) -> Result<(), Diagnostic> {
        let attachments: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.source.kind == "enclosure")
            .map(|(i, _)| i)
            .collect();
        let mut wrappers = BTreeMap::new();
        for index in attachments {
            let host = self.reference(index, self.nodes[index].source.text("host").unwrap())?;
            if !["stairs", "ramp", "slab", "landing", "roof", "canopy"]
                .contains(&self.nodes[host].source.kind.as_str())
            {
                return Err(self.nodes[index].source.fail(
                    "AML_ENCLOSURE",
                    "Enclosures require a flight or footprint host.",
                ));
            }
            let boundary = if let Some(id) = self.nodes[host].assemblies.last() {
                self.ids[id]
            } else if let Some(&wrapper) = wrappers.get(&host) {
                wrapper
            } else {
                let mut wrapper = self.nodes[host].clone();
                let parent = wrapper.parent;
                let id = format!("{}/$attachments", wrapper.id);
                wrapper.id = id.clone();
                wrapper.source.kind = "assembly".into();
                wrapper.children = vec![host];
                wrapper.matrix = Matrix::default();
                wrapper.assemblies.push(id.clone());
                let next = self.nodes.len();
                self.ids.insert(id, next);
                self.nodes.push(wrapper);
                let siblings = if let Some(parent) = parent {
                    &mut self.nodes[parent].children
                } else {
                    &mut self.roots
                };
                let position = siblings.iter().position(|i| *i == host).unwrap();
                siblings[position] = next;
                self.nodes[host].parent = Some(next);
                wrappers.insert(host, next);
                next
            };
            if self.nodes[index]
                .assemblies
                .last()
                .is_some_and(|id| *id != self.nodes[boundary].id)
            {
                return Err(self.nodes[index].source.fail(
                    "AML_ASSEMBLY",
                    "Attachment belongs to a different physical assembly from its host.",
                ));
            }
            if self.nodes[index].parent != Some(boundary) {
                let siblings = if let Some(parent) = self.nodes[index].parent {
                    &mut self.nodes[parent].children
                } else {
                    &mut self.roots
                };
                siblings.retain(|i| *i != index);
                self.nodes[boundary].children.push(index);
                self.nodes[index].parent = Some(boundary);
            }
            for key in ["dynamic", "visible", "collidable"] {
                if !self.nodes[index].source.attributes.contains_key(key)
                    && let Some(value) = self.nodes[host].source.attributes.get(key).cloned()
                {
                    self.nodes[index]
                        .source
                        .attributes
                        .insert(key.into(), value);
                }
            }
        }
        for i in 0..self.nodes.len() {
            let mut ancestors = vec![];
            let mut visited = BTreeSet::new();
            let mut parent = Some(i);
            while let Some(index) = parent {
                if !visited.insert(index) || visited.len() > 128 {
                    return Err(self.nodes[i]
                        .source
                        .fail("AML_CYCLE", "Attachment ancestry is cyclic."));
                }
                if self.nodes[index].source.kind == "assembly" {
                    ancestors.push(self.nodes[index].id.clone());
                }
                parent = self.nodes[index].parent;
            }
            ancestors.reverse();
            self.nodes[i].assemblies = ancestors;
        }
        Ok(())
    }
    pub fn declaration(&self, id: &str, kind: &str) -> Result<&Node, Diagnostic> {
        self.document
            .declarations
            .get(id)
            .filter(|n| n.kind == kind)
            .ok_or_else(|| {
                self.document
                    .root
                    .fail("AML_REFERENCE", format!("Unknown {kind} {id}."))
            })
    }
    fn frame(&mut self, id: &str, visiting: &mut BTreeSet<String>) -> Result<Matrix, Diagnostic> {
        if let Some(matrix) = self.frames.get(id) {
            return Ok(*matrix);
        }
        let node = self.declaration(id, "coordinate-frame")?.clone();
        if visiting.len() >= 64 {
            return Err(node.fail(
                "AML_COMPLEXITY",
                "Coordinate frame dependency depth exceeded.",
            ));
        }
        if !visiting.insert(id.into()) {
            return Err(node.fail("AML_CYCLE", "Cyclic coordinate frame."));
        }
        let parent = if let Some(parent) = node.text("frame") {
            self.frame(parent, visiting)?
        } else {
            Matrix::default()
        };
        let matrix = parent
            .then(placement(&node)?)
            .checked()
            .map_err(|e| node.fail("AML_TRANSFORM", e))?;
        self.frames.insert(id.into(), matrix);
        visiting.remove(id);
        Ok(matrix)
    }
    fn datum(&mut self, id: &str, visiting: &mut BTreeSet<String>) -> Result<f64, Diagnostic> {
        if let Some(value) = self.datums.get(id) {
            return Ok(*value);
        }
        let node = self.declaration(id, "datum")?.clone();
        if visiting.len() >= 64 {
            return Err(node.fail("AML_COMPLEXITY", "Datum dependency depth exceeded."));
        }
        if !visiting.insert(id.into()) {
            return Err(node.fail("AML_CYCLE", "Cyclic elevation datum."));
        }
        let parent = if let Some(parent) = node.text("datum") {
            self.datum(parent, visiting)?
        } else {
            0.
        };
        let elevation = parent + node.number("elevation", 0.);
        self.datums.insert(id.into(), elevation);
        visiting.remove(id);
        Ok(elevation)
    }
    pub fn elevation(&self, node: &Node) -> Result<f64, Diagnostic> {
        if let Some(id) = node.text("datum") {
            self.datums
                .get(id)
                .map(|z| z + node.number("offset", 0.))
                .ok_or_else(|| node.fail("AML_REFERENCE", format!("Unknown datum {id}.")))
        } else {
            Ok(node.number("elevation", 0.))
        }
    }
    pub fn node_placement(&self, node: &Node) -> Result<Matrix, Diagnostic> {
        let frame = if let Some(id) = node.text("frame") {
            *self.frames.get(id).ok_or_else(|| {
                node.fail("AML_REFERENCE", format!("Unknown coordinate frame {id}."))
            })?
        } else {
            Matrix::default()
        };
        frame
            .then(placement(node)?)
            .checked()
            .map_err(|e| node.fail("AML_TRANSFORM", e))
    }
    fn path(&mut self, id: &str, visiting: &mut BTreeSet<String>) -> Result<Vec<V3>, Diagnostic> {
        if let Some(path) = self.paths.get(id) {
            return Ok(path.clone());
        }
        let node = self.declaration(id, "path")?.clone();
        if visiting.len() >= 64 {
            return Err(node.fail("AML_COMPLEXITY", "Shared path dependency depth exceeded."));
        }
        if !visiting.insert(id.into()) {
            return Err(node.fail("AML_CYCLE", "Cyclic shared path."));
        }
        let tolerance = self.document.root.number("curve-tolerance", 0.01);
        let mut points = if let Some(references) = node.text("paths") {
            if !node.points("points").is_empty() || !node.children.is_empty() {
                return Err(node.fail("AML_PATH", "Choose references or inline path geometry."));
            }
            let mut points: Vec<V3> = vec![];
            for reference in references.split_whitespace() {
                let reversed = reference.starts_with('-');
                let mut path = self.path(reference.trim_start_matches('-'), visiting)?;
                if reversed {
                    path.reverse();
                }
                append_path(&mut points, &path, false).map_err(|e| node.fail("AML_PATH", e))?;
            }
            points
        } else {
            sample_path(&node, tolerance).map_err(|e| node.fail("AML_PATH", e))?
        };
        if points.len() < 2 || points.len() > 16384 {
            return Err(node.fail("AML_COMPLEXITY", "Path requires 2–16384 samples."));
        }
        if node.boolean("closed", false) && length(sub(points[0], *points.last().unwrap())) > 1e-9 {
            points.push(points[0]);
        }
        let matrix = self.node_placement(&node)?;
        points = points.into_iter().map(|p| matrix.point(p)).collect();
        self.paths.insert(id.into(), points.clone());
        visiting.remove(id);
        Ok(points)
    }
    fn loop_points(&self, node: &Node) -> Result<Vec<V3>, Diagnostic> {
        let mut points = node.points("points");
        if let Some(references) = node.text("paths") {
            if !points.is_empty() {
                return Err(node.fail("AML_PROFILE", "Choose points or shared paths."));
            }
            for reference in references.split_whitespace() {
                let mut path = self
                    .paths
                    .get(reference.trim_start_matches('-'))
                    .ok_or_else(|| {
                        node.fail(
                            "AML_REFERENCE",
                            format!("Unknown profile path {reference}."),
                        )
                    })?
                    .clone();
                if reference.starts_with('-') {
                    path.reverse();
                }
                append_path(&mut points, &path, false).map_err(|e| node.fail("AML_PROFILE", e))?;
            }
            if points.is_empty() || length(sub(points[0], *points.last().unwrap())) > 1e-7 {
                return Err(node.fail("AML_PROFILE", "Shared-path profile loop is not closed."));
            }
        }
        Ok(points)
    }
    fn expand(
        &mut self,
        mut source: Node,
        scope: &str,
        parent: Option<usize>,
        instances: Vec<String>,
        mut assemblies: Vec<String>,
        stack: &mut Vec<String>,
    ) -> Result<usize, Diagnostic> {
        if self.nodes.len() >= 100_000 || stack.len() > 64 {
            return Err(source.fail("AML_COMPLEXITY", "Element expansion budget exceeded."));
        }
        if let Some(parent) = parent {
            let inherited = &self.nodes[parent].source;
            for key in ["material", "dynamic", "group"] {
                if !source.attributes.contains_key(key)
                    && let Some(value) = inherited.attributes.get(key)
                {
                    source.attributes.insert(key.into(), value.clone());
                }
            }
            for key in ["visible", "collidable"] {
                if !inherited.boolean(key, true) {
                    source.attributes.insert(key.into(), Value::Boolean(false));
                }
            }
        }
        let id = if scope.is_empty() {
            source.id.clone()
        } else {
            format!("{scope}/{}", source.id)
        };
        if self.ids.contains_key(&id) {
            return Err(source.fail("AML_DUPLICATE_ID", format!("Duplicate resolved id {id}.")));
        }
        let matrix = parent
            .map_or_else(Matrix::default, |p| self.nodes[p].matrix)
            .then(self.node_placement(&source)?)
            .checked()
            .map_err(|e| source.fail("AML_TRANSFORM", e))?;
        if source.kind == "assembly" {
            assemblies.push(id.clone());
        }
        let index = self.nodes.len();
        self.ids.insert(id.clone(), index);
        self.nodes.push(Resolved {
            source: source.clone(),
            id: id.clone(),
            matrix,
            scope: scope.into(),
            parent,
            children: vec![],
            instances: instances.clone(),
            assemblies: assemblies.clone(),
        });
        if source.kind == "instance" {
            let reference = source.text("ref").expect("required ref");
            if stack.iter().any(|id| id == reference) {
                return Err(source.fail("AML_CYCLE", "Cyclic definition instance."));
            }
            let definition = self.declaration(reference, "definition")?.clone();
            stack.push(reference.into());
            let mut ancestry = instances;
            ancestry.push(id.clone());
            for child in definition.children {
                let child = self.expand(
                    child,
                    &id,
                    Some(index),
                    ancestry.clone(),
                    assemblies.clone(),
                    stack,
                )?;
                self.nodes[index].children.push(child);
            }
            stack.pop();
        } else if [
            "assembly",
            "group",
            "collision",
            "union",
            "subtract",
            "intersect",
        ]
        .contains(&source.kind.as_str())
        {
            for child in source.children {
                let child = self.expand(
                    child,
                    scope,
                    Some(index),
                    instances.clone(),
                    assemblies.clone(),
                    stack,
                )?;
                self.nodes[index].children.push(child);
            }
        }
        Ok(index)
    }
    pub fn reference(&self, owner: usize, id: &str) -> Result<usize, Diagnostic> {
        let node = &self.nodes[owner];
        let scoped = format!("{}/{id}", node.scope);
        self.ids
            .get(&scoped)
            .or_else(|| self.ids.get(id))
            .copied()
            .ok_or_else(|| {
                node.source.fail(
                    "AML_REFERENCE",
                    format!("Unknown architectural reference {id}."),
                )
            })
    }
}

fn append_path(output: &mut Vec<V3>, path: &[V3], closed: bool) -> Result<(), String> {
    if path.is_empty() {
        return Err("Shared path is empty.".into());
    }
    if let Some(last) = output.last() {
        if length(sub(*last, path[0])) > 1e-7 {
            return Err("Shared path has a broken seam.".into());
        }
        output.extend_from_slice(&path[1..]);
    } else {
        output.extend_from_slice(path);
    }
    if closed && length(sub(output[0], *output.last().unwrap())) > 1e-7 {
        return Err("Path is not closed.".into());
    }
    if output.len() > 16384 {
        return Err("Path sample budget exceeded.".into());
    }
    Ok(())
}
fn sample_path(node: &Node, tolerance: f64) -> Result<Vec<V3>, String> {
    let mut points = node.points("points");
    let elevation = match node.attributes.get("elevation") {
        Some(Value::Vector(value)) => Some([value[0], value[1]]),
        _ => None,
    };
    if elevation.is_some() {
        for p in &mut points {
            p[2] = 0.;
        }
    }
    for segment in &node.children {
        if segment.kind == "arc" {
            let center = segment.vector("center", [0.; 3]);
            let [rx, ry] = segment.vector("radii", [0.; 2]);
            let sweep = segment.number("sweep", 0.);
            if rx <= 0. || ry <= 0. || sweep == 0. {
                return Err("Arc requires positive radii and nonzero sweep.".into());
            }
            let angle = 2. * acos((1. - tolerance / rx.max(ry)).max(-1.));
            let count = (sweep.to_radians().abs() / angle.min(std::f64::consts::FRAC_PI_4))
                .ceil()
                .max(1.);
            if !count.is_finite() || count + points.len() as f64 + 1. > 16384. {
                return Err("Curve sample budget exceeded.".into());
            }
            let count = count as usize;
            let rotation = segment.number("rotation", 0.).to_radians();
            let (sine, cosine) = (sin(rotation), cos(rotation));
            for i in 0..=count {
                let a =
                    (segment.number("start", 0.) + sweep * i as f64 / count as f64).to_radians();
                let (x, y) = (rx * cos(a), ry * sin(a));
                let p = [
                    center[0] + x * cosine - y * sine,
                    center[1] + x * sine + y * cosine,
                    center[2],
                ];
                if i == 0 && !points.is_empty() {
                    if length(sub(*points.last().unwrap(), p)) > tolerance {
                        return Err("Arc start does not meet preceding segment.".into());
                    }
                } else {
                    points.push(p);
                }
            }
        } else {
            let start = *points
                .last()
                .ok_or("A line or curve needs an initial point.")?;
            let mut end = segment.vector("to", [0.; 3]);
            if elevation.is_some() {
                end[2] = 0.;
            }
            if segment.kind == "line" {
                points.push(end);
            } else {
                let mut controls = segment.points("controls");
                if controls.is_empty()
                    || (segment.kind == "quadratic" && controls.len() != 1)
                    || (segment.kind == "cubic" && controls.len() != 2)
                {
                    return Err("Bezier control count does not match its degree.".into());
                }
                if elevation.is_some() {
                    for p in &mut controls {
                        p[2] = 0.;
                    }
                }
                let mut row = vec![start];
                row.extend(controls);
                row.push(end);
                bezier(&row, tolerance, &mut points, 0)?;
            }
        }
    }
    if let Some([start, end]) = elevation {
        let cuts = stations(&points);
        let total = *cuts.last().unwrap_or(&0.);
        if total <= 1e-12 {
            return Err("Elevation path has zero plan length.".into());
        }
        for (p, s) in points.iter_mut().zip(cuts) {
            p[2] = start + (end - start) * s / total;
        }
    }
    Ok(points)
}
fn bezier(row: &[V3], tolerance: f64, output: &mut Vec<V3>, depth: usize) -> Result<(), String> {
    let a = row[0];
    let b = *row.last().unwrap();
    let chord = sub(b, a);
    let len = length(chord);
    let flat = row[1..row.len() - 1]
        .iter()
        .map(|p| {
            if len > 1e-12 {
                length(cross(sub(*p, a), chord)) / len
            } else {
                length(sub(*p, a))
            }
        })
        .fold(0., f64::max);
    if output.len() > 16384 {
        return Err("Curve sample budget exceeded.".into());
    }
    if flat <= tolerance {
        output.push(b);
        return Ok(());
    }
    if depth >= 20 {
        return Err("Curve subdivision depth exceeded.".into());
    }
    let mut left = vec![a];
    let mut right = vec![b];
    let mut row = row.to_vec();
    while row.len() > 1 {
        row = row
            .windows(2)
            .map(|pair| lerp(pair[0], pair[1], 0.5))
            .collect();
        left.push(row[0]);
        right.insert(0, *row.last().unwrap());
    }
    bezier(&left, tolerance, output, depth + 1)?;
    bezier(&right, tolerance, output, depth + 1)
}
