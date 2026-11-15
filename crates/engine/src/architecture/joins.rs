use super::*;

#[derive(Clone)]
pub(super) struct Incident {
    pub node: usize,
    pub point: V3,
    pub direction: V3,
    pub thickness: f64,
    pub bottom: f64,
    pub top: f64,
}

impl Compiler<'_> {
    /// Terrain and moving objects retain independent physical boundaries inside assemblies.
    pub(super) fn compose(
        &self,
        solids: Vec<Solid>,
        source: &Node,
    ) -> Result<Vec<Solid>, Diagnostic> {
        let mut terrain = vec![];
        let mut fixed = vec![];
        let mut dynamic = vec![];
        for solid in solids {
            let mesh = solid.mesh();
            let nodes = mesh
                .run_original_id
                .iter()
                .filter_map(|id| self.surfaces.get(id))
                .map(|surface| &self.resolution.nodes[surface.node].source)
                .collect::<Vec<_>>();
            if nodes.iter().all(|n| n.kind == "terrain") {
                terrain.push(solid);
            } else if nodes.iter().any(|n| n.boolean("dynamic", false)) {
                dynamic.push(solid);
            } else {
                fixed.push(solid);
            }
        }
        if !fixed.is_empty() {
            terrain.push(Solid::union(&fixed).at(source)?);
        }
        terrain.extend(dynamic);
        Ok(terrain)
    }

    /// Fills only exterior endpoint wedges within an explicit assembly and shared height span.
    pub(super) fn joins(&mut self, assembly: usize) -> Result<Product, Diagnostic> {
        let mut endpoints: BTreeMap<[i64; 2], Vec<Incident>> = BTreeMap::new();
        for incident in &self.incidents {
            if !self.resolution.nodes[incident.node]
                .assemblies
                .contains(&self.resolution.nodes[assembly].id)
            {
                continue;
            }
            let key = [incident.point[0], incident.point[1]].map(|n| (n * 1e7).round() as i64);
            endpoints.entry(key).or_default().push(incident.clone());
        }
        let mut product = Product::default();
        for entries in endpoints.into_values() {
            let mut heights = entries
                .iter()
                .flat_map(|e| [e.bottom, e.top])
                .collect::<Vec<_>>();
            heights.sort_by(f64::total_cmp);
            heights.dedup();
            for span in heights.windows(2) {
                let (bottom, top) = (span[0], span[1]);
                if top - bottom < 1e-7 {
                    continue;
                }
                let mut directions: BTreeMap<[i64; 2], &Incident> = BTreeMap::new();
                for e in &entries {
                    if e.bottom >= top - 1e-7 || e.top <= bottom + 1e-7 {
                        continue;
                    }
                    let key = [e.direction[0], e.direction[1]].map(|n| (n * 1e7).round() as i64);
                    if directions
                        .get(&key)
                        .is_none_or(|old| old.thickness < e.thickness)
                    {
                        directions.insert(key, e);
                    }
                }
                let mut active = directions.into_values().collect::<Vec<_>>();
                active.sort_by(|a, b| {
                    atan2(a.direction[1], a.direction[0])
                        .total_cmp(&atan2(b.direction[1], b.direction[0]))
                });
                if active.len() < 2 {
                    continue;
                }
                for i in 0..active.len() {
                    let (a, b) = (active[i], active[(i + 1) % active.len()]);
                    let (u, v) = (a.direction, b.direction);
                    let mut angle = atan2(v[1], v[0]) - atan2(u[1], u[0]);
                    if angle <= 0. {
                        angle += std::f64::consts::TAU;
                    }
                    if angle < std::f64::consts::PI + 1e-7 {
                        continue;
                    }
                    let p = [a.point[0], a.point[1], top];
                    let aa = [
                        p[0] - u[1] * a.thickness / 2.,
                        p[1] + u[0] * a.thickness / 2.,
                        top,
                    ];
                    let bb = [
                        p[0] + v[1] * b.thickness / 2.,
                        p[1] - v[0] * b.thickness / 2.,
                        top,
                    ];
                    let den = u[0] * v[1] - u[1] * v[0];
                    if den.abs() < 1e-7 {
                        continue;
                    }
                    let t = ((bb[0] - aa[0]) * v[1] - (bb[1] - aa[1]) * v[0]) / den;
                    let intersection = add(aa, scale(u, t));
                    let mut points = vec![p, aa];
                    if length(sub(intersection, p)) <= 2. * a.thickness.max(b.thickness) {
                        points.push(intersection);
                    }
                    points.push(bb);
                    if area(&points) < 0. {
                        points.reverse();
                    }
                    let profile = Profile {
                        triangles: (1..points.len() - 1).map(|j| [0, j, j + 1]).collect(),
                        loops: vec![points.clone()],
                        points,
                    };
                    let geometry = Geometry::surface(&profile, |p| [p[0], p[1], bottom]);
                    let owner = self.owner(a.node, None)?;
                    let source = &self.resolution.nodes[a.node].source;
                    let solid = Solid::from_geometry(&geometry, owner).at(source)?;
                    for t in &geometry.triangles {
                        self.authored.push((t.map(|j| geometry.points[j]), owner));
                    }
                    if source.boolean("visible", true) {
                        product.render.push(solid.clone());
                    }
                    if source.boolean("collidable", true) {
                        product.solids.push(solid);
                    }
                }
            }
        }
        Ok(product)
    }
}
