//! Automatic connections read finished visual geometry and canonical walking support separately.
use crate::worldspec::themes::kowloon::{
    building::{
        self,
        materials::{Appearance, Settings},
    },
    mesh::Mesh as Drawing,
    spans::{anchors, classify, draw, plan as connections, walking::Walking},
};
use std::collections::BTreeMap;
use superspec_geometry::obstructions::{Budget, Obstructions};
use superspec_types::architecture::{Node, V3};

pub(crate) struct Job {
    pub(crate) index: usize,
    pub(crate) appearance: Appearance,
    pub(crate) drawing: Drawing<'static>,
}

pub(crate) fn connect(
    jobs: &mut [Job],
    buildings: &building::Plan,
    settings: &Node,
    walking: &Walking,
    retained: &[V3],
    budget: &mut Budget<'_>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut span_rules = settings
        .children
        .iter()
        .filter(|child| child.kind == "spans");
    let span_rule = span_rules.next();
    if span_rules.next().is_some() {
        return Err("Only one spans rule is supported.".into());
    }
    let selected: Option<std::collections::BTreeSet<&str>> = span_rule
        .and_then(|rule| rule.text("hosts"))
        .map(|hosts| hosts.split_whitespace().collect());
    if let Some(names) = &selected {
        for name in names {
            if !jobs
                .iter()
                .any(|job| buildings.sites()[job.index].host == *name)
            {
                return Err(format!("Unknown span building host {name}."));
            }
        }
    }
    let hosts = jobs
        .iter()
        .map(|job| {
            let bindings = &job.appearance.bindings;
            classify::Host {
                id: buildings.sites()[job.index].host.clone(),
                walls: bindings
                    .iter()
                    .filter(|(slot, _)| {
                        *slot == "wall" || *slot == "concrete" || slot.starts_with("substrate-")
                    })
                    .map(|(_, id)| id.clone())
                    .collect(),
                glazing: ["pane", "interior-lit"]
                    .iter()
                    .map(|slot| bindings[*slot].clone())
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let objects = jobs
        .iter()
        .zip(&hosts)
        .map(|(job, host)| {
            job.drawing
                .surfaces
                .iter()
                .map(|s| format!("{}/$dressing/{}", host.id, s.part))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let faces = classify::faces(
        &hosts,
        jobs.iter()
            .zip(&hosts)
            .zip(&objects)
            .flat_map(|((job, host), objects)| {
                job.drawing
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .enumerate()
                    .map(move |(index, triangle)| {
                        let surface = job.drawing.triangle_surfaces[index];
                        classify::Facet {
                            surface: classify::Surface {
                                object: &objects[surface],
                                host: Some(&host.id),
                                material: job
                                    .drawing
                                    .source_material(&job.drawing.surfaces[surface].slot),
                                visible: true,
                                dynamic: false,
                            },
                            points: triangle.map(|v| job.drawing.vertices[v as usize].position),
                            normal: job.drawing.vertices[triangle[0] as usize].normal,
                        }
                    })
            }),
        budget,
    )?;
    let names = hosts.iter().map(|h| h.id.clone()).collect::<Vec<_>>();
    let mut planes = anchors::planes(&names, &faces, budget)?;
    if let Some(names) = &selected {
        planes.retain(|plane| names.contains(plane.host()));
    }
    let count = retained.len() / 3
        + jobs
            .iter()
            .map(|j| j.drawing.triangle_count())
            .sum::<usize>();
    if count > 1_048_576 {
        return Err("Finished span scene exceeds triangle allowance.".into());
    }
    budget.spend(count)?;
    let mut triangles = retained.as_chunks::<3>().0.to_vec();
    for job in jobs.iter() {
        triangles.extend(
            job.drawing
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|i| job.drawing.vertices[i as usize].position)),
        );
    }
    let obstacles = Obstructions::new(&triangles, budget)?;
    let seed = format!("{}/spans/0", settings.text("seed").unwrap());
    let surroundings = anchors::Surroundings {
        obstructions: &obstacles,
        floors: walking,
    };
    let mut utility = vec![];
    let mut windows = vec![];
    for (i, plane) in planes.iter().enumerate() {
        let identity = format!("{seed}/{}/{i}", plane.host());
        utility.extend(anchors::utility(
            plane,
            i,
            &identity,
            &surroundings,
            budget,
        )?);
        windows.extend(anchors::windows(
            plane,
            i,
            &identity,
            &surroundings,
            budget,
        )?);
        if utility.len() + windows.len() > 65_536 {
            return Err("Finished span scene exceeds fixing-point allowance.".into());
        }
    }
    let number = |key, default| span_rule.map_or(default, |rule| rule.number(key, default));
    let plan = connections::build(
        connections::Rule {
            cables: number("cables", 0.6),
            clotheslines: number("clotheslines", 0.35),
            signs: number("signs", 0.),
            clearance: number("clearance", 2.6),
            reach: number("reach", 14.),
        },
        &seed,
        &planes,
        &utility,
        &windows,
        walking,
        obstacles,
        budget,
    )?;
    let anchors = utility.into_iter().chain(windows).collect::<Vec<_>>();
    let identity = format!("{}/spans", settings.text("seed").unwrap());
    let look = Settings {
        seed: &identity,
        grime: settings.number("grime", 0.4),
        wear: settings.number("wear", 0.2),
        rust: settings.number("rust", settings.number("wear", 0.2)),
        density: 64.,
        detail: match settings.text("detail") {
            Some("low") => 0,
            Some("high") => 2,
            _ => 1,
        },
    };
    let kits = jobs
        .iter()
        .map(|j| j.appearance.kit(&look))
        .collect::<Result<Vec<_>, _>>()?;
    let repeats = jobs
        .iter()
        .map(|j| {
            j.appearance
                .inputs
                .iter()
                .map(|(id, m)| (id.clone(), m.repeat))
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let repeats = repeats
        .iter()
        .map(|map| move |slot: &str| map.get(slot).copied().unwrap_or(1.))
        .collect::<Vec<_>>();
    let outputs = jobs
        .iter_mut()
        .zip(&kits)
        .zip(&repeats)
        .map(|((job, kit), repeats)| draw::Output {
            host: &buildings.sites()[job.index].host,
            mesh: &mut job.drawing,
            kit,
            cloth: job.appearance.inputs["balcony-cloth"].texture.is_some(),
            repeats,
        })
        .collect();
    draw::draw_hosts(outputs, &plan, &seed, &planes, &anchors, budget)
}
