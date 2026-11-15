//! Floor-clipped components and shared-capacity physical entrance resources.
use super::*;
use crate::architecture::Context;
use superspec_geometry::clipping::half_plane;

struct Work<'a> {
    used: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl Work<'_> {
    fn spend(&mut self, n: usize) -> Result<(), String> {
        self.used = self.used.saturating_add(n);
        if (self.cancelled)() {
            Err("Physical route analysis cancelled.".into())
        } else if self.used > 8_000_000 {
            Err("Physical route analysis exceeds its work allowance.".into())
        } else {
            Ok(())
        }
    }
}
struct Fragment {
    area: String,
    points: Vec<V3>,
    component: usize,
    bounds: [f64; 4],
}
pub use superspec_model::validation::physical::{Component, Connection, Evidence, Resource};

pub struct EnabledLink {
    pub id: String,
    pub navigation: navigation::Link,
}
pub struct Network {
    fragments: Vec<Fragment>,
    pub links: Vec<EnabledLink>,
    pub evidence: Evidence,
    pub diagnostics: Vec<Diagnostic>,
}

fn bounds(points: &[V3]) -> [f64; 4] {
    points.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |mut b, p| {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
            b
        },
    )
}
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] + 1e-5 && a[2] + 1e-5 >= b[0] && a[1] <= b[3] + 1e-5 && a[3] + 1e-5 >= b[1]
}
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}
fn join(parent: &mut [usize], a: usize, b: usize) {
    let a = root(parent, a);
    let b = root(parent, b);
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}
fn height(points: &[V3], p: V3) -> Option<f64> {
    let n = cross(sub(points[1], points[0]), sub(points[2], points[0]));
    (n[2].abs() > 1e-12).then(|| {
        points[0][2] - (n[0] * (p[0] - points[0][0]) + n[1] * (p[1] - points[0][1])) / n[2]
    })
}
fn shared_side(a: V3, b: V3, c: V3, d: V3, tolerance: f64) -> bool {
    let edge = sub(b, a);
    let length = hypot(edge[0], edge[1]);
    if length < 1e-6 {
        return false;
    }
    if orient(a, b, c).abs() > 1e-5 * length || orient(a, b, d).abs() > 1e-5 * length {
        return false;
    }
    let station = |p: V3| ((p[0] - a[0]) * edge[0] + (p[1] - a[1]) * edge[1]) / length;
    let (u, v) = (station(c), station(d));
    let lo = 0f64.max(u.min(v));
    let hi = length.min(u.max(v));
    if hi - lo < 1e-5 || (v - u).abs() < 1e-9 {
        return false;
    }
    [lo, hi].iter().all(|s| {
        ((a[2] + edge[2] * s / length) - (c[2] + (d[2] - c[2]) * (s - u) / (v - u))).abs()
            <= tolerance
    })
}
fn locate(
    fragments: &[Fragment],
    p: V3,
    area: Option<&str>,
    snap: f64,
    vertical: f64,
) -> Option<(usize, V3)> {
    let mut best = f64::INFINITY;
    let mut result = None;
    for fragment in fragments {
        if area.is_some_and(|a| a != fragment.area) {
            continue;
        }
        if !overlaps(
            fragment.bounds,
            [p[0] - snap, p[1] - snap, p[0] + snap, p[1] + snap],
        ) {
            continue;
        }
        for i in 1..fragment.points.len() - 1 {
            let tri = [
                fragment.points[0],
                fragment.points[i],
                fragment.points[i + 1],
            ];
            let flat = tri.map(|p| [p[0], p[1], 0.]);
            let mut q = navigation::closest_triangle([p[0], p[1], 0.], flat[0], flat[1], flat[2]);
            let Some(z) = height(&tri, q) else { continue };
            q[2] = z;
            let horizontal = hypot(q[0] - p[0], q[1] - p[1]);
            let dz = (q[2] - p[2]).abs();
            if horizontal <= snap + 1e-5 && dz <= vertical && horizontal + dz < best {
                best = horizontal + dz;
                result = Some((fragment.component, q));
            }
        }
    }
    result
}

fn fragments(
    scene: &Scene,
    nav: &navigation::Mesh,
    agent: Agent,
    work: &mut Work<'_>,
) -> Result<(Vec<Fragment>, Vec<Component>), String> {
    let context = scene
        .manifest
        .architecture
        .as_ref()
        .ok_or("Missing floor context.")?;
    let physical_owners: BTreeSet<_> = scene
        .manifest
        .meshes
        .iter()
        .filter(|m| scene.manifest.colliders.contains(&m.id))
        .map(|m| m.object_id.as_str())
        .collect();
    let mut pieces = vec![];
    for floor in &context.floors {
        if !physical_owners.contains(floor.id.as_str()) {
            continue;
        }
        for tri in &floor.triangles {
            let tri = tri.map(|i| floor.points[i]);
            let box_floor = bounds(&tri);
            let sign = orient(tri[0], tri[1], tri[2]).signum();
            if sign == 0. {
                continue;
            }
            for polygon in &nav.polygons {
                work.spend(polygon.len())?;
                let points: Vec<_> = polygon.iter().map(|i| nav.vertices[*i]).collect();
                if !overlaps(box_floor, bounds(&points)) {
                    continue;
                }
                let mut clipped = points;
                for i in 0..3 {
                    clipped = half_plane(&clipped, |p| sign * orient(tri[i], tri[(i + 1) % 3], p));
                }
                if clipped.len() < 3 || area(&clipped).abs() < 1e-7 {
                    continue;
                }
                let center = scale(
                    clipped.iter().fold([0.; 3], |a, p| add(a, *p)),
                    1. / clipped.len() as f64,
                );
                if height(&tri, center)
                    .is_none_or(|z| (z - center[2]).abs() > agent.step + nav.cell_height * 2.)
                {
                    continue;
                }
                pieces.push(Fragment {
                    area: floor.area.clone(),
                    bounds: bounds(&clipped),
                    points: clipped,
                    component: 0,
                });
            }
        }
    }
    let mut parent: Vec<_> = (0..pieces.len()).collect();
    let mut bins: BTreeMap<(String, i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, piece) in pieces.iter().enumerate() {
        let b = piece.bounds;
        let mut candidates = BTreeSet::<usize>::new();
        let mut keys = vec![];
        for x in (b[0] - 1e-5).floor() as i64..=(b[2] + 1e-5).floor() as i64 {
            for y in (b[1] - 1e-5).floor() as i64..=(b[3] + 1e-5).floor() as i64 {
                work.spend(1)?;
                let key = (piece.area.clone(), x, y);
                if let Some(prior) = bins.get(&key) {
                    candidates.extend(prior.iter().copied());
                }
                keys.push(key);
            }
        }
        for &j in &candidates {
            let other = &pieces[j];
            if !overlaps(b, other.bounds) {
                continue;
            }
            let mut connected = false;
            for a in 0..piece.points.len() {
                for c in 0..other.points.len() {
                    work.spend(1)?;
                    connected |= shared_side(
                        piece.points[a],
                        piece.points[(a + 1) % piece.points.len()],
                        other.points[c],
                        other.points[(c + 1) % other.points.len()],
                        nav.cell_height * 2.,
                    );
                }
            }
            if connected {
                join(&mut parent, i, j);
            }
        }
        for key in keys {
            bins.entry(key).or_default().push(i);
        }
    }
    let mut roots = BTreeMap::new();
    let mut components: Vec<Component> = vec![];
    for (i, piece) in pieces.iter_mut().enumerate() {
        let r = root(&mut parent, i);
        let index = *roots.entry(r).or_insert_with(|| {
            components.push(Component {
                area: piece.area.clone(),
                bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
            });
            components.len() - 1
        });
        piece.component = index;
        for p in &piece.points {
            for axis in 0..3 {
                components[index].bounds[0][axis] = components[index].bounds[0][axis].min(p[axis]);
                components[index].bounds[1][axis] = components[index].bounds[1][axis].max(p[axis]);
            }
        }
    }
    Ok((pieces, components))
}

pub(super) fn area_id<'a>(context: &'a Context, id: &'a str) -> &'a str {
    context
        .floors
        .iter()
        .find(|f| f.id == id)
        .map(|f| f.area.as_str())
        .unwrap_or(id)
}
pub(super) fn touching(a: &[V3], b: &[V3]) -> bool {
    for edge in a.windows(2) {
        for other in b.windows(2) {
            if edge
                .iter()
                .any(|p| other.iter().any(|q| length(sub(*p, *q)) < 1e-6))
            {
                return true;
            }
            let v = sub(edge[1], edge[0]);
            let size = dot(v, v);
            if size < 1e-12 {
                continue;
            }
            let t = |p: V3| dot(sub(p, edge[0]), v) / size;
            if other
                .iter()
                .all(|p| length(sub(*p, add(edge[0], scale(v, t(*p))))) < 1e-6)
                && t(other[0]).min(t(other[1])).max(0.)
                    <= t(other[0]).max(t(other[1])).min(1.) + 1e-7
            {
                return true;
            }
        }
    }
    false
}
impl Network {
    pub fn build(
        scene: &Scene,
        nav: &navigation::Mesh,
        agent: Agent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        let context = scene
            .manifest
            .architecture
            .as_ref()
            .ok_or("Missing context.")?;
        let collision_triangles = scene
            .manifest
            .meshes
            .iter()
            .filter(|m| scene.manifest.colliders.contains(&m.id))
            .map(|m| m.index_count / 3)
            .sum::<usize>();
        let mut work = Work { used: 0, cancelled };
        let (fragments, components) = fragments(scene, nav, agent, &mut work)?;
        let mut result = Self {
            fragments,
            links: vec![],
            evidence: Evidence {
                components,
                ..Default::default()
            },
            diagnostics: vec![],
        };
        let mut parent: Vec<_> = (0..context.connections.len()).collect();
        for (i, a) in context.connections.iter().enumerate() {
            for (j, b) in context.connections[..i].iter().enumerate() {
                work.spend(a.path.len() * b.path.len() + 1)?;
                let mut aa = [area_id(context, &a.from), area_id(context, &a.to)];
                let mut bb = [area_id(context, &b.from), area_id(context, &b.to)];
                aa.sort();
                bb.sort();
                if aa == bb && touching(&a.path, &b.path) {
                    join(&mut parent, i, j);
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..parent.len() {
            let r = root(&mut parent, i);
            groups.entry(r).or_default().push(i);
        }
        for group in groups.values() {
            let mut pairs = vec![];
            let mut path = vec![];
            for &i in group {
                let connection = &context.connections[i];
                let mut areas = [
                    area_id(context, &connection.from),
                    area_id(context, &connection.to),
                ];
                areas.sort();
                path.extend_from_slice(&connection.path);
                if let Some(obstruction) = context.connection_obstructions.get(&connection.id) {
                    result.evidence.connections.push(Connection {
                        id: connection.id.clone(),
                        status: "blocked",
                        path: connection.path.clone(),
                    });
                    let mut objects = vec![
                        connection.id.clone(),
                        connection.from.clone(),
                        connection.to.clone(),
                    ];
                    objects.extend(obstruction.blockers.iter().cloned());
                    let points: Vec<_> = connection
                        .path
                        .iter()
                        .map(|p| [p[0], p[1], obstruction.height])
                        .collect();
                    result.diagnostics.push(super::finding(connection.source.clone(),"CONNECTION_BLOCKED","Closed collision covers the entire mouth at a common standing-body height.".into(),objects,&points,"error"));
                    continue;
                }
                let mut found = false;
                for edge in connection.path.windows(2) {
                    let v = sub(edge[1], edge[0]);
                    let length = hypot(v[0], v[1]);
                    if length < 1e-8 {
                        continue;
                    }
                    for fraction in [0.5, 0.25, 0.75, 0.125, 0.875] {
                        work.spend(result.fragments.len() * 2 + 1)?;
                        let p = lerp(edge[0], edge[1], fraction);
                        let side = scale([-v[1] / length, v[0] / length, 0.], 0.025);
                        for direction in [1., -1.] {
                            let a = locate(
                                &result.fragments,
                                add(p, scale(side, direction)),
                                Some(areas[0]),
                                0.,
                                agent.step + nav.cell_height * 2.,
                            );
                            let b = locate(
                                &result.fragments,
                                sub(p, scale(side, direction)),
                                Some(areas[1]),
                                0.,
                                agent.step + nav.cell_height * 2.,
                            );
                            if let (Some((a, pa)), Some((b, pb))) = (a, b) {
                                work.spend(collision_triangles)?;
                                if !super::ray_blocked(
                                    scene,
                                    add(pa, [0., 0., agent.height / 2.]),
                                    add(pb, [0., 0., agent.height / 2.]),
                                ) {
                                    pairs.push((a, b));
                                    found = true;
                                }
                            }
                        }
                    }
                }
                result.evidence.connections.push(Connection {
                    id: connection.id.clone(),
                    status: if found { "verified" } else { "unverified" },
                    path: connection.path.clone(),
                });
                if !found {
                    result.diagnostics.push(super::finding(connection.source.clone(),"CONNECTION_UNVERIFIED","No standing crossing was witnessed through the declared mouth. Inspect its support and clearance.".into(),vec![connection.id.clone(),connection.from.clone(),connection.to.clone()],&connection.path,"warning"));
                }
            }
            if let Some(&(a, b)) = pairs.first() {
                let mut cluster = BTreeSet::from([a, b]);
                let mut changed = true;
                while changed {
                    changed = false;
                    for &(a, b) in &pairs {
                        if cluster.contains(&a) || cluster.contains(&b) {
                            changed |= cluster.insert(a);
                            changed |= cluster.insert(b);
                        }
                    }
                }
                let pairs: Vec<_> = pairs
                    .into_iter()
                    .filter(|(a, b)| cluster.contains(a) && cluster.contains(b))
                    .collect();
                result.evidence.resources.push(Resource {
                    kind: "mouth",
                    declarations: group
                        .iter()
                        .map(|i| context.connections[*i].id.clone())
                        .collect(),
                    from: pairs
                        .iter()
                        .map(|p| p.0)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    to: pairs
                        .iter()
                        .map(|p| p.1)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    bidirectional: true,
                    path,
                });
            }
        }
        let player = context.metadata.iter().find(|n| n.kind == "player");
        for node in context
            .metadata
            .iter()
            .filter(|n| n.kind == "link" && n.text("movement").is_none_or(|m| m == "land"))
        {
            let enabled = node
                .text("ability")
                .is_some_and(|ability| player.is_some_and(|p| p.boolean(ability, false)));
            if !enabled {
                continue;
            }
            let from = node.vector("from", [0.; 3]);
            let to = node.vector("to", [0.; 3]);
            let a = nav.nearest(from, agent.radius, agent.step + nav.cell_height * 2.);
            let b = nav.nearest(to, agent.radius, agent.step + nav.cell_height * 2.);
            let (Some((_, a)), Some((_, b))) = (a, b) else {
                result.diagnostics.push(super::node_finding(
                    node,
                    "LINK_ENDPOINT_UNVERIFIED",
                    "An enabled traversal endpoint lacks a nearby standing navigation surface.",
                    &[from, to],
                    "warning",
                ));
                continue;
            };
            let bidirectional = node.boolean("bidirectional", true);
            let ca = locate(
                &result.fragments,
                a,
                node.text("from-area").map(|id| area_id(context, id)),
                agent.radius,
                agent.step + nav.cell_height * 2.,
            );
            let cb = locate(
                &result.fragments,
                b,
                node.text("to-area").map(|id| area_id(context, id)),
                agent.radius,
                agent.step + nav.cell_height * 2.,
            );
            if (node.text("from-area").is_some() && ca.is_none())
                || (node.text("to-area").is_some() && cb.is_none())
            {
                result.diagnostics.push(super::node_finding(
                    node,
                    "LINK_ENDPOINT_UNVERIFIED",
                    "A traversal endpoint does not lie on its declared floor area.",
                    &[from, to],
                    "warning",
                ));
                continue;
            }
            result.links.push(EnabledLink {
                id: node.id.clone(),
                navigation: navigation::Link {
                    from: a,
                    to: b,
                    bidirectional,
                },
            });
            if let (Some((ca, _)), Some((cb, _))) = (ca, cb) {
                let prior = result.evidence.resources.iter_mut().find(|r| {
                    r.kind == "link"
                        && ((r.from == [ca]
                            && r.to == [cb]
                            && length(sub(r.path[0], a)) <= 2. * agent.radius
                            && length(sub(r.path[1], b)) <= 2. * agent.radius)
                            || (r.from == [cb]
                                && r.to == [ca]
                                && length(sub(r.path[0], b)) <= 2. * agent.radius
                                && length(sub(r.path[1], a)) <= 2. * agent.radius))
                });
                if let Some(prior) = prior {
                    prior.bidirectional |= bidirectional || prior.from != [ca];
                    prior.declarations.push(node.id.clone());
                } else {
                    result.evidence.resources.push(Resource {
                        kind: "link",
                        declarations: vec![node.id.clone()],
                        from: vec![ca],
                        to: vec![cb],
                        bidirectional,
                        path: vec![a, b],
                    });
                }
            }
        }
        Ok(result)
    }
    pub fn independent(
        &self,
        context: &Context,
        from: &Node,
        to: &Node,
        agent: Agent,
        vertical: f64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<usize, String> {
        let a = locate(
            &self.fragments,
            from.vector("position", [0.; 3]),
            from.text("element").map(|id| area_id(context, id)),
            agent.radius,
            vertical,
        );
        let b = locate(
            &self.fragments,
            to.vector("position", [0.; 3]),
            to.text("element").map(|id| area_id(context, id)),
            agent.radius,
            vertical,
        );
        let (Some((a, _)), Some((b, _))) = (a, b) else {
            return Ok(0);
        };
        if a == b {
            return Ok(1);
        }
        capacity(
            self.evidence.components.len(),
            &self.evidence.resources,
            a,
            b,
            cancelled,
        )
    }
}

/// Each mouth or traversal has one shared bottleneck, including bidirectional resources.
pub(super) fn capacity(
    components: usize,
    resources: &[Resource],
    a: usize,
    b: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<usize, String> {
    let mut work = Work { used: 0, cancelled };
    work.spend(components + resources.len() * 2)?;
    let count = components + resources.len() * 2;
    let mut graph = vec![BTreeMap::<usize, usize>::new(); count];
    let capacity = resources.len() + 1;
    fn edge(g: &mut [BTreeMap<usize, usize>], a: usize, b: usize, n: usize) {
        *g[a].entry(b).or_default() += n;
        g[b].entry(a).or_default();
    }
    for (i, r) in resources.iter().enumerate() {
        work.spend(r.from.len() + r.to.len() + 1)?;
        let x = components + i * 2;
        let y = x + 1;
        edge(&mut graph, x, y, 1);
        for &a in &r.from {
            edge(&mut graph, a, x, capacity);
            if r.bidirectional {
                edge(&mut graph, y, a, capacity);
            }
        }
        for &b in &r.to {
            edge(&mut graph, y, b, capacity);
            if r.bidirectional {
                edge(&mut graph, b, x, capacity);
            }
        }
    }
    let mut flow = 0;
    loop {
        let mut parent = vec![usize::MAX; count];
        parent[a] = a;
        let mut queue = vec![a];
        let mut cursor = 0;
        while cursor < queue.len() && parent[b] == usize::MAX {
            for (&n, &capacity) in &graph[queue[cursor]] {
                work.spend(1)?;
                if capacity > 0 && parent[n] == usize::MAX {
                    parent[n] = queue[cursor];
                    queue.push(n);
                }
            }
            cursor += 1;
        }
        if parent[b] == usize::MAX {
            return Ok(flow);
        }
        let mut at = b;
        while at != a {
            let p = parent[at];
            *graph[p].get_mut(&at).unwrap() -= 1;
            *graph[at].get_mut(&p).unwrap() += 1;
            at = p;
        }
        flow += 1;
    }
}
