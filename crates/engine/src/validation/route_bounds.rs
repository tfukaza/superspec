//! Optimistic capacity of declared entrances, independent of sampled navigation.
use super::{physical, *};
use crate::architecture::Context;

pub(super) struct Bounds {
    areas: Vec<String>,
    resources: Vec<physical::Resource>,
    complete: bool,
}

/// Ambiguous or absent floor ownership cannot establish a finite route bound.
fn endpoint(
    context: &Context,
    node: &Node,
    key: &str,
    area: &str,
    step: f64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>, String> {
    if let Some(id) = node.text(area) {
        let id = physical::area_id(context, id);
        return Ok(context
            .floors
            .iter()
            .any(|f| f.area == id)
            .then(|| id.to_owned()));
    }
    let p = node.vector(key, [0.; 3]);
    let mut matches = BTreeSet::new();
    let mut work = 0;
    for floor in &context.floors {
        if cancelled() {
            return Err("Route bound analysis cancelled.".into());
        }
        for t in &floor.triangles {
            work += 1;
            if work > 8_000_000 {
                return Err("Route endpoint work allowance exceeded.".into());
            }
            let tri = t.map(|i| floor.points[i]);
            let n = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
            if n[2].abs() < 1e-9 {
                continue;
            }
            let winding = n[2].signum();
            if (0..3).any(|i| winding * orient(tri[i], tri[(i + 1) % 3], p) < -1e-7) {
                continue;
            }
            let z = tri[0][2] - (n[0] * (p[0] - tri[0][0]) + n[1] * (p[1] - tri[0][1])) / n[2];
            if (z - p[2]).abs() <= step + 0.02 {
                matches.insert(floor.area.clone());
            }
        }
    }
    Ok((matches.len() == 1).then(|| matches.into_iter().next().unwrap()))
}

impl Bounds {
    pub(super) fn build(
        context: &Context,
        agent: Agent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        let areas: Vec<_> = context
            .floors
            .iter()
            .map(|f| f.area.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index = |id: &str| {
            areas
                .iter()
                .position(|a| a == physical::area_id(context, id))
        };
        let mut resources: Vec<physical::Resource> = vec![];
        let mut complete = true;
        let mut work = 0usize;
        // Group all aliases first. A blocked alias cannot erase a still-possible part of its mouth.
        let mut groups: Vec<(usize, usize, Vec<usize>)> = vec![];
        for (i, connection) in context.connections.iter().enumerate() {
            if cancelled() {
                return Err("Route bound analysis cancelled.".into());
            }
            let (Some(a), Some(b)) = (index(&connection.from), index(&connection.to)) else {
                complete = false;
                continue;
            };
            if connection.path.len() < 2 {
                complete = false;
                continue;
            }
            let pair = (a.min(b), a.max(b));
            let mut matching = vec![];
            for (j, (a, b, members)) in groups.iter().enumerate() {
                work += 1;
                if work > 8_000_000 {
                    return Err("Route bound work allowance exceeded.".into());
                }
                if (*a, *b) != pair {
                    continue;
                }
                for &member in members {
                    work = work.saturating_add(
                        connection
                            .path
                            .len()
                            .saturating_mul(context.connections[member].path.len()),
                    );
                    if work > 8_000_000 {
                        return Err("Route bound work allowance exceeded.".into());
                    }
                    if physical::touching(&connection.path, &context.connections[member].path) {
                        matching.push(j);
                        break;
                    }
                }
            }
            let mut members = vec![i];
            for j in matching.into_iter().rev() {
                members.extend(groups.remove(j).2);
            }
            groups.push((pair.0, pair.1, members));
        }
        for (a, b, members) in groups {
            if members.iter().all(|i| {
                context
                    .connection_obstructions
                    .contains_key(&context.connections[*i].id)
            }) {
                continue;
            }
            resources.push(physical::Resource {
                kind: "mouth",
                declarations: members
                    .iter()
                    .map(|i| context.connections[*i].id.clone())
                    .collect(),
                from: vec![a],
                to: vec![b],
                bidirectional: true,
                path: vec![],
            });
        }
        let player = context.metadata.iter().find(|n| n.kind == "player");
        for node in context
            .metadata
            .iter()
            .filter(|n| n.kind == "link" && n.text("movement").is_none_or(|m| m == "land"))
        {
            if cancelled() {
                return Err("Route bound analysis cancelled.".into());
            }
            if !node
                .text("ability")
                .is_some_and(|a| player.is_some_and(|p| p.boolean(a, false)))
            {
                continue;
            }
            let a = endpoint(context, node, "from", "from-area", agent.step, cancelled)?
                .and_then(|a| index(&a));
            let b = endpoint(context, node, "to", "to-area", agent.step, cancelled)?
                .and_then(|a| index(&a));
            let (Some(a), Some(b)) = (a, b) else {
                complete = false;
                continue;
            };
            let from = node.vector("from", [0.; 3]);
            let to = node.vector("to", [0.; 3]);
            let bidirectional = node.boolean("bidirectional", true);
            let mut prior = None;
            for (i, r) in resources
                .iter()
                .enumerate()
                .filter(|(_, r)| r.kind == "link")
            {
                work += 1;
                if work > 8_000_000 {
                    return Err("Route bound work allowance exceeded.".into());
                }
                let near = |p, q| length(sub(p, q)) <= agent.radius * 2.;
                if (r.from == [a] && r.to == [b] && near(r.path[0], from) && near(r.path[1], to))
                    || (r.from == [b]
                        && r.to == [a]
                        && near(r.path[0], to)
                        && near(r.path[1], from))
                {
                    prior = Some(i);
                    break;
                }
            }
            if let Some(i) = prior {
                let r = &mut resources[i];
                r.bidirectional |= bidirectional || r.from != [a];
                r.declarations.push(node.id.clone());
            } else {
                resources.push(physical::Resource {
                    kind: "link",
                    declarations: vec![node.id.clone()],
                    from: vec![a],
                    to: vec![b],
                    bidirectional,
                    path: vec![from, to],
                });
            }
        }
        Ok(Self {
            areas,
            resources,
            complete,
        })
    }

    /// Intra-area connectivity stays unrestricted. Same-area and incomplete graphs have no finite bound.
    pub(super) fn maximum(
        &self,
        context: &Context,
        from: &Node,
        to: &Node,
        agent: Agent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<usize>, String> {
        if !self.complete {
            return Ok(None);
        }
        let a = endpoint(context, from, "position", "element", agent.step, cancelled)?
            .and_then(|id| self.areas.iter().position(|a| a == &id));
        let b = endpoint(context, to, "position", "element", agent.step, cancelled)?
            .and_then(|id| self.areas.iter().position(|a| a == &id));
        let (Some(a), Some(b)) = (a, b) else {
            return Ok(None);
        };
        if a == b {
            return Ok(None);
        }
        physical::capacity(self.areas.len(), &self.resources, a, b, cancelled).map(Some)
    }
}
