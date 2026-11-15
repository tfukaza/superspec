//! Continuous closed-solid mouth obstruction proofs remain separate from navigation sampling.
use super::context::MouthObstruction;
use super::*;

fn contains(point: V3, loops: &[Vec<V3>]) -> bool {
    loops
        .iter()
        .filter(|ring| inside(point, ring))
        .map(|ring| area(ring).signum())
        .sum::<f64>()
        > 0.
}

/// Returns no proof when a mouth lies on a solid boundary or any open interval remains clear.
fn covered(a: V3, b: V3, slices: &[(Vec<Vec<V3>>, Vec<String>)]) -> Option<Vec<String>> {
    let d = sub(b, a);
    let mut cuts = vec![0., 1.];
    for (loops, _) in slices {
        for ring in loops {
            for i in 0..ring.len() {
                let p = ring[i];
                let q = ring[(i + 1) % ring.len()];
                let e = sub(q, p);
                let denominator = d[0] * e[1] - d[1] * e[0];
                if denominator.abs() < 1e-12 {
                    if orient(a, b, p).abs() < 1e-8
                        && (0..2).all(|axis| {
                            a[axis].min(b[axis]) <= p[axis].max(q[axis]) + 1e-8
                                && p[axis].min(q[axis]) <= a[axis].max(b[axis]) + 1e-8
                        })
                    {
                        return None;
                    }
                    continue;
                }
                let delta = sub(p, a);
                let t = (delta[0] * e[1] - delta[1] * e[0]) / denominator;
                let u = (delta[0] * d[1] - delta[1] * d[0]) / denominator;
                if t > 0. && t < 1. && (-1e-8..=1. + 1e-8).contains(&u) {
                    cuts.push(t);
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    let mut blockers = BTreeSet::new();
    for interval in cuts.windows(2) {
        if interval[1] - interval[0] < 1e-10 {
            continue;
        }
        let p = lerp(a, b, (interval[0] + interval[1]) / 2.);
        let mut occupied = false;
        for (loops, owners) in slices {
            if contains(p, loops) {
                occupied = true;
                blockers.extend(owners.iter().cloned());
            }
        }
        if !occupied {
            return None;
        }
    }
    (!blockers.is_empty()).then(|| blockers.into_iter().collect())
}

impl Compiler<'_> {
    pub(super) fn connection_obstructions(
        &self,
        product: &Product,
    ) -> Result<BTreeMap<String, MouthObstruction>, Diagnostic> {
        let connections: Vec<_> = self
            .resolution
            .document
            .metadata
            .iter()
            .filter(|n| n.kind == "connection")
            .collect();
        if connections.is_empty() {
            return Ok(BTreeMap::new());
        }
        let player = self
            .resolution
            .document
            .metadata
            .iter()
            .find(|n| n.kind == "player");
        let height = player.map(|p| p.number("height", 1.8)).unwrap_or(1.8);
        let step = player.map(|p| p.number("step", 0.45)).unwrap_or(0.45);
        if step >= height {
            return Ok(BTreeMap::new());
        }
        let mut supports = vec![];
        for (owner, geometry) in owned_geometry(&product.solids, &product.patches) {
            if !self.surfaces[&owner].collidable {
                continue;
            }
            for t in geometry.triangles {
                let points = t.map(|i| geometry.points[i]);
                if cross(sub(points[1], points[0]), sub(points[2], points[0]))[2] > 1e-9 {
                    let bounds = points.iter().fold(
                        [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
                        |mut b, p| {
                            for axis in 0..3 {
                                b[0][axis] = b[0][axis].min(p[axis]);
                                b[1][axis] = b[1][axis].max(p[axis]);
                            }
                            b
                        },
                    );
                    supports.push(bounds);
                }
            }
        }
        supports.sort_by(|a, b| a[0][2].total_cmp(&b[0][2]));
        let mut result = BTreeMap::new();
        let mut work = 0;
        for node in connections {
            if (self.cancelled)() {
                return Err(node.fail("CANCELLED", "Connection analysis cancelled."));
            }
            let Some(path) = node
                .text("path")
                .and_then(|id| self.resolution.paths.get(id))
            else {
                continue;
            };
            let elevation = path[0][2];
            if path.iter().any(|p| (p[2] - elevation).abs() > 1e-6) {
                continue;
            }
            let bounds =
                path.iter()
                    .fold([[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]], |mut b, p| {
                        for axis in 0..2 {
                            b[0][axis] = b[0][axis].min(p[axis]);
                            b[1][axis] = b[1][axis].max(p[axis]);
                        }
                        b
                    });
            let mut reachable = elevation;
            for support in &supports {
                work += 1;
                if work > 8_000_000 {
                    return Err(node.fail(
                        "AML_COMPLEXITY",
                        "Connection proof work allowance exceeded.",
                    ));
                }
                if support[0][2] > reachable + step + 1e-5 {
                    break;
                }
                reachable = reachable.max(support[1][2]);
            }
            let mut lowest = elevation;
            let mut highest = elevation;
            for support in &supports {
                work += 1;
                if work > 8_000_000 {
                    return Err(node.fail(
                        "AML_COMPLEXITY",
                        "Connection proof work allowance exceeded.",
                    ));
                }
                if support[0][2] > reachable + 1e-5 {
                    break;
                }
                if (0..2).any(|axis| {
                    support[0][axis] > bounds[1][axis] + 1e-5
                        || support[1][axis] < bounds[0][axis] - 1e-5
                }) {
                    continue;
                }
                lowest = lowest.min(support[0][2]);
                highest = highest.max(support[1][2].min(reachable));
            }
            if highest >= lowest + height - 1e-5 {
                continue;
            }
            let plane = (highest + lowest + height) / 2.;
            let mut slices = vec![];
            for solid in &product.solids {
                if (self.cancelled)() {
                    return Err(node.fail("CANCELLED", "Connection analysis cancelled."));
                }
                let box3 = solid.bounds();
                if plane <= box3[0][2] + 1e-7 || plane >= box3[1][2] - 1e-7 {
                    continue;
                }
                if (0..2).any(|axis| {
                    box3[0][axis] > bounds[1][axis] + 1e-5 || box3[1][axis] < bounds[0][axis] - 1e-5
                }) {
                    continue;
                }
                let owners = solid
                    .mesh()
                    .run_original_id
                    .iter()
                    .filter_map(|owner| self.surfaces.get(owner))
                    .map(|s| self.resolution.nodes[s.node].id.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                slices.push((solid.slice_xy(plane), owners));
            }
            let mut blockers = BTreeSet::new();
            let mut all = true;
            for edge in path.windows(2) {
                match covered(edge[0], edge[1], &slices) {
                    Some(ids) => blockers.extend(ids),
                    None => {
                        all = false;
                        break;
                    }
                }
            }
            if all && !blockers.is_empty() {
                result.insert(
                    node.id.clone(),
                    MouthObstruction {
                        height: plane,
                        blockers: blockers.into_iter().collect(),
                    },
                );
            }
        }
        Ok(result)
    }
}
