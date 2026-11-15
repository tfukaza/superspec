use super::*;
use superspec_geometry::clipping::{dominant_axis, partition, project};

#[derive(Clone)]
struct Fragment {
    points: Vec<V3>,
    owner: u32,
    sources: BTreeSet<u32>,
    assignment: Option<usize>,
}
fn normal(points: [V3; 3]) -> Option<V3> {
    unit(cross(sub(points[1], points[0]), sub(points[2], points[0]))).ok()
}
fn plane_key(points: [V3; 3]) -> Option<[i64; 4]> {
    normal(points)
        .map(|n| [n[0], n[1], n[2], dot(n, points[0])].map(|v| (v * 1e5 + 0.5).floor() as i64))
}
impl Compiler<'_> {
    fn contains(&self, boundary: usize, node: usize) -> bool {
        let mut ancestor = Some(node);
        while let Some(i) = ancestor {
            if i == boundary {
                return true;
            }
            ancestor = self.resolution.nodes[i].parent;
        }
        false
    }
    fn boundaries(&self, node: usize) -> Vec<usize> {
        let mut result = vec![];
        let mut ancestor = self.resolution.nodes[node].parent;
        while let Some(i) = ancestor {
            if ["assembly", "union", "subtract", "intersect", "collision"]
                .contains(&self.resolution.nodes[i].source.kind.as_str())
            {
                result.push(i);
            }
            ancestor = self.resolution.nodes[i].parent;
        }
        result
    }
    pub(super) fn validate_assignment(&self, index: usize) -> Result<(), Diagnostic> {
        let node = &self.resolution.nodes[index];
        let source = &node.source;
        let host = self
            .resolution
            .reference(index, source.text("host").unwrap())?;
        if [
            "visual-mesh",
            "face-material",
            "water",
            "room",
            "zone",
            "volume",
            "clearance",
        ]
        .contains(&self.resolution.nodes[host].source.kind.as_str())
        {
            return Err(source.fail(
                "AML_REFERENCE",
                "Face assignments require native architectural faces.",
            ));
        }
        if ["position", "rotation", "scale", "frame"]
            .iter()
            .any(|k| source.attributes.contains_key(*k))
        {
            return Err(source.fail(
                "AML_MATERIAL",
                "Face assignments use the host frame; place the partition profile instead.",
            ));
        }
        if source.text("profile").is_some() {
            self.profile(source)?;
        }
        let material = source.text("material").unwrap();
        if material != "default"
            && !self
                .resolution
                .document
                .declarations
                .get(material)
                .is_some_and(|n| n.kind == "material")
        {
            return Err(source.fail("AML_REFERENCE", format!("Unknown material {material}.")));
        }
        Ok(())
    }
    fn selected(&self, assignment: usize, host: usize, points: [V3; 3], surface: &Surface) -> bool {
        let face = self.resolution.nodes[assignment]
            .source
            .text("face")
            .unwrap_or("all");
        if face == "all" {
            return true;
        }
        if face == "reveal" {
            let n = &self.resolution.nodes[surface.node];
            return n.source.kind == "opening"
                && self
                    .resolution
                    .reference(surface.node, n.source.text("host").unwrap())
                    .ok()
                    == Some(host);
        }
        let Some(n) = normal(points) else {
            return false;
        };
        let matrix = self.resolution.nodes[host].matrix;
        let Ok(n) = unit([
            dot(matrix.0[0], n),
            dot(matrix.0[1], n),
            dot(matrix.0[2], n),
        ]) else {
            return false;
        };
        if face == "side" {
            return n[2].abs() < 1. - 1e-6;
        }
        let component = match face {
            "top" => n[2],
            "bottom" => -n[2],
            "front" => -n[1],
            "back" => n[1],
            "left" => -n[0],
            _ => n[0],
        };
        component > 1. - 1e-6
    }
    fn conflict(&self, indices: impl IntoIterator<Item = usize>) -> Diagnostic {
        let mut ids = indices.into_iter().collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        let mut d=self.resolution.nodes[ids[0]].source.fail("AML_MATERIAL_CONFLICT","Exposed coincident faces have conflicting materials; declare material-owner or disjoint partitions.");
        d.objects = ids
            .iter()
            .map(|i| self.resolution.nodes[*i].id.clone())
            .collect();
        d
    }
    pub(super) fn paint(
        &mut self,
        input: BTreeMap<u32, Geometry>,
    ) -> Result<BTreeMap<u32, Geometry>, Diagnostic> {
        let assignments: Vec<_> = self
            .resolution
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.source.kind == "face-material")
            .map(|(i, _)| i)
            .collect();
        let mut indexed: BTreeMap<[i64; 4], Vec<([V3; 3], u32)>> = BTreeMap::new();
        for &(points, owner) in &self.authored {
            if let Some(key) = plane_key(points) {
                indexed.entry(key).or_default().push((points, owner));
            }
        }
        let mut output: BTreeMap<u32, Geometry> = BTreeMap::new();
        let mut painted = BTreeMap::new();
        let mut work = 0usize;
        for (owner, geometry) in input {
            let surface = self.surfaces[&owner].clone();
            if !surface.visible {
                continue;
            }
            let current_boundaries = self.boundaries(surface.node);
            for t in geometry.triangles {
                let points = t.map(|i| geometry.points[i]);
                let Some(n) = normal(points) else {
                    continue;
                };
                let axis = dominant_axis(n);
                let mut fragments = vec![Fragment {
                    points: points.to_vec(),
                    owner,
                    sources: BTreeSet::from([owner]),
                    assignment: None,
                }];
                for &(candidate, other) in indexed
                    .get(&plane_key(points).unwrap())
                    .into_iter()
                    .flatten()
                {
                    work += 1;
                    if work > 20_000_000 {
                        return Err(self
                            .resolution
                            .document
                            .root
                            .fail("AML_COMPLEXITY", "Face partition work budget exceeded."));
                    }
                    let other_surface = &self.surfaces[&other];
                    if other_surface.node == surface.node
                        || other_surface.collidable != surface.collidable
                        || other_surface.visible != surface.visible
                        || self.resolution.nodes[other_surface.node]
                            .source
                            .boolean("dynamic", false)
                            != self.resolution.nodes[surface.node]
                                .source
                                .boolean("dynamic", false)
                        || normal(candidate).is_none_or(|other| dot(n, other) < 1. - 1e-6)
                        || candidate
                            .iter()
                            .any(|p| dot(n, sub(*p, points[0])).abs() > 1e-6)
                        || !current_boundaries
                            .iter()
                            .any(|i| self.contains(*i, other_surface.node))
                    {
                        continue;
                    }
                    let mut next = vec![];
                    for fragment in fragments {
                        let (inside, outside) =
                            partition(&fragment.points, candidate, |p| project(p, axis));
                        if inside.is_empty() {
                            next.push(fragment);
                            continue;
                        }
                        for points in outside {
                            let mut f = fragment.clone();
                            f.points = points;
                            next.push(f);
                        }
                        let mut f = fragment;
                        f.points = inside;
                        f.sources.insert(other);
                        next.push(f);
                    }
                    fragments = next;
                    if fragments.len() > 100000 {
                        return Err(self
                            .resolution
                            .document
                            .root
                            .fail("AML_COMPLEXITY", "Face fragment budget exceeded."));
                    }
                }
                for fragment in &mut fragments {
                    let materials = fragment
                        .sources
                        .iter()
                        .map(|o| &self.surfaces[o].material)
                        .collect::<BTreeSet<_>>();
                    if materials.len() <= 1 {
                        continue;
                    }
                    let mut preferred = None;
                    for &boundary in &current_boundaries {
                        if !fragment
                            .sources
                            .iter()
                            .all(|o| self.contains(boundary, self.surfaces[o].node))
                        {
                            continue;
                        }
                        let policy = &self.resolution.nodes[boundary].source;
                        if policy.text("coincident-material") == Some("retained-face") {
                            preferred = Some(owner);
                            break;
                        }
                        if let Some(id) = policy.text("material-owner") {
                            let target = self.resolution.reference(boundary, id)?;
                            let candidates = fragment
                                .sources
                                .iter()
                                .copied()
                                .filter(|o| self.contains(target, self.surfaces[o].node))
                                .collect::<Vec<_>>();
                            if !candidates.is_empty() {
                                if candidates
                                    .iter()
                                    .map(|o| &self.surfaces[o].material)
                                    .collect::<BTreeSet<_>>()
                                    .len()
                                    > 1
                                {
                                    return Err(self.conflict(
                                        candidates.iter().map(|o| self.surfaces[o].node),
                                    ));
                                }
                                preferred = candidates.into_iter().min_by_key(|o| {
                                    &self.resolution.nodes[self.surfaces[o].node].id
                                });
                                break;
                            }
                        }
                        if policy.text("coincident-material") == Some("error") {
                            break;
                        }
                    }
                    if let Some(preferred) = preferred {
                        fragment.owner = preferred;
                    } else {
                        return Err(
                            self.conflict(fragment.sources.iter().map(|o| self.surfaces[o].node))
                        );
                    }
                }
                for &assignment in &assignments {
                    let source = self.resolution.nodes[assignment].source.clone();
                    let host = self
                        .resolution
                        .reference(assignment, source.text("host").unwrap())?;
                    let opening_host =
                        if self.resolution.nodes[surface.node].source.kind == "opening" {
                            self.resolution
                                .reference(
                                    surface.node,
                                    self.resolution.nodes[surface.node]
                                        .source
                                        .text("host")
                                        .unwrap(),
                                )
                                .ok()
                        } else {
                            None
                        };
                    if (!self.contains(host, surface.node)
                        && !opening_host.is_some_and(|h| self.contains(host, h)))
                        || !self.selected(assignment, host, points, &surface)
                    {
                        continue;
                    }
                    let profile = source
                        .text("profile")
                        .map(|_| self.profile(&source))
                        .transpose()?;
                    let inverse = self.resolution.nodes[host].matrix.inverse().at(&source)?;
                    let projection = source.text("projection").unwrap_or("xy");
                    let project = |p| {
                        let p = inverse.point(p);
                        match projection {
                            "xz" => [p[0], p[2], 0.],
                            "yz" => [p[1], p[2], 0.],
                            _ => [p[0], p[1], 0.],
                        }
                    };
                    let mut regions = vec![];
                    if let Some(profile) = profile {
                        for t in profile.triangles {
                            let lifted = t.map(|i| {
                                let p = profile.points[i];
                                self.resolution.nodes[host].matrix.point(match projection {
                                    "xz" => [p[0], 0., p[1]],
                                    "yz" => [0., p[0], p[1]],
                                    _ => [p[0], p[1], 0.],
                                })
                            });
                            regions.push(lifted);
                        }
                    }
                    let mut passes = if regions.is_empty() {
                        vec![None]
                    } else {
                        regions.into_iter().map(Some).collect()
                    };
                    for region in passes.drain(..) {
                        let mut next = vec![];
                        for fragment in fragments {
                            work += 1;
                            if work > 20_000_000 {
                                return Err(source.fail(
                                    "AML_COMPLEXITY",
                                    "Face partition work budget exceeded.",
                                ));
                            }
                            let (inside, outside) = if let Some(region) = region {
                                partition(&fragment.points, region, project)
                            } else {
                                (fragment.points.clone(), vec![])
                            };
                            if inside.is_empty() {
                                next.push(fragment);
                                continue;
                            }
                            for points in outside {
                                let mut f = fragment.clone();
                                f.points = points;
                                next.push(f);
                            }
                            let mut f = fragment;
                            f.points = inside;
                            let material = source.string("material", "default");
                            if let Some(prior) = f.assignment
                                && self.surfaces[&f.owner].material != material
                            {
                                return Err(self.conflict([prior, assignment]));
                            }
                            let key = (f.owner, material.clone());
                            let new_owner = if let Some(o) = painted.get(&key) {
                                *o
                            } else {
                                let o = Solid::reserve_owner();
                                let mut s = self.surfaces[&f.owner].clone();
                                s.material = material;
                                s.assigned = true;
                                self.surfaces.insert(o, s);
                                painted.insert(key, o);
                                o
                            };
                            f.owner = new_owner;
                            f.assignment = Some(assignment);
                            next.push(f);
                        }
                        fragments = next;
                    }
                }
                for fragment in fragments {
                    let geometry = output.entry(fragment.owner).or_default();
                    let base = geometry.points.len();
                    geometry.points.extend(&fragment.points);
                    for i in 1..fragment.points.len() - 1 {
                        geometry.triangles.push([base, base + i, base + i + 1]);
                    }
                }
            }
        }
        Ok(output)
    }
}
