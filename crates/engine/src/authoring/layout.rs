//! Atlas layouts of a scene project's receivers, for a renderer that bakes materials in UV space
//! itself (the Studio's GPU preview). The layout is the one a CPU bake of the same project uses:
//! the same [`Atlas`], sized by the same rule, so a texel of a GPU bake samples the point the CPU
//! bake samples there and CPU-baked fields ([`Session::bake_project_fields`]) land on its grid.

use super::{
    Diagnostic, FieldBake, Project, Session,
    project::{contacts, material_request, receivers},
    receiver::{Atlas, GUTTER},
};
use serde::Serialize;
use std::{collections::BTreeSet, sync::Arc};

/// Numbers per triangle in [`ProjectLayouts::records`]:
/// - 0..4 the triangle's chart rectangle in atlas texels, rows from the upper edge: x, y, width,
///   height (gutters included);
/// - 4 the quarter turns from the triangle's legs to the atlas axes ([`Atlas::turn`]), 5..7 the
///   chart's distinct samples along its width and height, 7 the triangle's role in its chart
///   (0 alone, 1 the first of a pair, 2 the second);
/// - 8..17 world positions of the corner, the first leg's end and the second leg's end;
/// - 17..26 their unit normals; 26..32 their surface UVs; 32..38 their atlas UVs (V up).
pub const LAYOUT_RECORD: usize = 38;

/// One receiver's atlas: its size and where its triangle records start.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverLayout {
    /// The mesh identity in the composed scene.
    pub id: String,
    /// The material assignment that covers it.
    pub material: String,
    pub width: usize,
    pub height: usize,
    /// Texels per meter along every chart.
    pub density: f64,
    pub triangles: usize,
    /// Index of the receiver's first number in [`ProjectLayouts::records`].
    pub offset: usize,
}

/// The atlases of every assigned receiver of a scene project.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLayouts {
    /// Texels of chart margin on every side ([`GUTTER`]).
    pub gutter: usize,
    /// Numbers per triangle record ([`LAYOUT_RECORD`]).
    pub record: usize,
    pub receivers: Vec<ReceiverLayout>,
    /// Triangle records of every receiver, end to end.
    #[serde(skip)]
    pub records: Vec<f32>,
}

/// Appends an atlas's triangle records (see [`LAYOUT_RECORD`]) in triangle order.
fn write_records(atlas: &Atlas, records: &mut Vec<f32>) {
    let receiver = &atlas.receiver;
    let triangles = receiver.indices.len() / 3;
    let mut owner = vec![(0usize, 0f32); triangles];
    for (index, chart) in atlas.charts.iter().enumerate() {
        owner[chart.first] = (index, if chart.second.is_some() { 1. } else { 0. });
        if let Some(second) = chart.second {
            owner[second] = (index, 2.);
        }
    }
    records.reserve(triangles * LAYOUT_RECORD);
    for triangle in 0..triangles {
        let (chart, role) = owner[triangle];
        let chart = &atlas.charts[chart];
        records.extend([chart.x, chart.y, chart.width, chart.height].map(|texels| texels as f32));
        records.extend([
            f32::from(atlas.turn(triangle)),
            chart.samples[0] as f32,
            chart.samples[1] as f32,
            role,
        ]);
        let corners = atlas.corners(triangle).map(|index| index as usize);
        // The atlas mesh holds each triangle's corners in this order, with unit normals.
        for position in &atlas.mesh.positions[triangle * 3..triangle * 3 + 3] {
            records.extend(position.map(|v| v as f32));
        }
        for normal in &atlas.mesh.normals[triangle * 3..triangle * 3 + 3] {
            records.extend(normal.map(|v| v as f32));
        }
        for corner in corners {
            records.extend(
                receiver.uvs[corner * 2..corner * 2 + 2]
                    .iter()
                    .map(|v| *v as f32),
            );
        }
        for uv in &atlas.mesh.uvs[triangle * 3..triangle * 3 + 3] {
            records.extend(uv.map(|v| v as f32));
        }
    }
}

impl Session {
    /// The atlas of every assigned receiver of a version 2 material project, as the CPU bake of
    /// that project lays it out. Nothing is compiled or baked.
    pub fn project_layouts(
        &mut self,
        project: &Project,
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProjectLayouts, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects have receiver layouts.".into(),
            ));
        }
        self.begin_authoring(revision, &project.entry)?;
        let (model, transforms) = self.compose_models(project, revision, assets, cancelled)?;
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        let mut layouts = ProjectLayouts {
            gutter: GUTTER,
            record: LAYOUT_RECORD,
            ..Default::default()
        };
        for assignment in &project.materials {
            let ids: BTreeSet<String> = assignment.meshes.iter().cloned().collect();
            // The atlas reads no source and no contacts: the request only carries the sizing inputs.
            let request = material_request(project, assignment, "", Vec::new());
            for receiver in receivers(&scene, &ids, &transforms) {
                if cancelled() {
                    return Err(fail("Project authoring cancelled.".into()));
                }
                let id = receiver.id.clone();
                let atlas = Atlas::for_request(receiver, &request)
                    .map_err(|message| fail(format!("{id}: {message}")))?;
                let offset = layouts.records.len();
                write_records(&atlas, &mut layouts.records);
                layouts.receivers.push(ReceiverLayout {
                    id,
                    material: assignment.id.clone(),
                    width: atlas.width,
                    height: atlas.height,
                    density: atlas.density,
                    triangles: atlas.receiver.indices.len() / 3,
                    offset,
                });
            }
        }
        Ok(layouts)
    }

    /// Evaluates `names` (fields of the material assigned to `mesh`) at every texel of that
    /// receiver's atlas, as [`Session::bake_fields`] does for a single request: the receiver, its
    /// contacts and the request are the ones `build_project` bakes that mesh with.
    pub fn bake_project_fields(
        &mut self,
        project: &Project,
        mesh: &str,
        names: &[String],
        revision: u64,
        assets: &dyn Fn(&str) -> Result<Arc<[u8]>, String>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<FieldBake, Vec<Diagnostic>> {
        let fail = |message: String| vec![Diagnostic::message(&project.entry, message)];
        if project.schema_version != 2 || project.kind != "material" {
            return Err(fail(
                "Only version 2 material projects bake fields per mesh.".into(),
            ));
        }
        self.begin_authoring(revision, &project.entry)?;
        let assignment = project
            .materials
            .iter()
            .find(|assignment| assignment.meshes.iter().any(|id| id == mesh))
            .ok_or_else(|| fail(format!("Mesh {mesh} has no material.")))?;
        let source = project.modules.get(&assignment.entry).ok_or_else(|| {
            fail(format!(
                "Material {} uses missing module {}.",
                assignment.id, assignment.entry
            ))
        })?;
        let (model, transforms) = self.compose_models(project, revision, assets, cancelled)?;
        let scene = model
            .scene_cancellable(revision as u32, cancelled)
            .map_err(&fail)?;
        let ids = BTreeSet::from([mesh.to_owned()]);
        let mut found = receivers(&scene, &ids, &transforms);
        let Some(mut receiver) = found.pop() else {
            return Err(fail(format!(
                "Mesh {mesh} is not a visible mesh of the project's models."
            )));
        };
        receiver.contacts = contacts(&scene, &receiver);
        let request = material_request(project, assignment, source, vec![receiver]);
        self.bake_fields(&request, revision, names, assets, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{Mapping, receiver};

    fn atlas(domain: [f64; 2], density: Option<f64>) -> Atlas {
        Atlas::new(
            receiver::swatch(domain),
            64,
            Mapping::Object,
            domain,
            [0., 0., -1.],
            512,
            density,
        )
        .unwrap()
    }

    /// Reads a record the way a renderer does and checks it against the atlas's own sampling.
    #[test]
    fn records_reproduce_the_atlas_texels() {
        for (domain, density) in [
            ([1., 1.], None),
            ([2., 0.5], None),
            ([0.25, 0.25], Some(64.)),
        ] {
            let atlas = atlas(domain, density);
            let mut records = Vec::new();
            write_records(&atlas, &mut records);
            assert_eq!(records.len(), 2 * LAYOUT_RECORD);
            for chart in &atlas.charts {
                for y in 0..chart.height {
                    for x in 0..chart.width {
                        let (triangle, u, v) = atlas.texel(chart, x, y);
                        let expected = atlas.sample(triangle, u, v).unwrap();
                        let record = &records[triangle * LAYOUT_RECORD..][..LAYOUT_RECORD];
                        assert_eq!(
                            &record[..4],
                            &[chart.x, chart.y, chart.width, chart.height].map(|n| n as f32)
                        );
                        // The renderer's texel-to-leg mapping (see atlas.ts), independent of `Atlas::texel`.
                        let inner = [record[2] - 4., record[3] - 4.];
                        let place = |texel: usize, axis: usize| {
                            let at = ((texel as f32 + 0.5 - 2.) / inner[axis]).clamp(0., 1.);
                            let count = record[5 + axis];
                            if count < inner[axis] {
                                ((at * count).floor().min(count - 1.) + 0.5) / count
                            } else {
                                at
                            }
                        };
                        let (s, t) = (place(x, 0), place(y, 1));
                        let turn = record[4] as u8;
                        let (mut a, mut b) = if turn & 1 == 1 { (t, 1. - s) } else { (s, t) };
                        // A texel on the diagonal belongs to either triangle of the pair.
                        let second = a + b > 1.;
                        if (a + b - 1.).abs() > 1e-5 {
                            assert_eq!(second, record[7] == 2., "role at {x},{y}");
                        }
                        if turn >= 2 {
                            (a, b) = (1. - a, 1. - b);
                        }
                        for axis in 0..3 {
                            let world = record[8 + axis] * (1. - a - b)
                                + record[11 + axis] * a
                                + record[14 + axis] * b;
                            assert!(
                                (f64::from(world) - expected.world[axis]).abs() < 1e-4,
                                "world at {x},{y}"
                            );
                        }
                    }
                }
            }
            // Atlas UVs are the baked mesh's own.
            for (triangle, uvs) in atlas.mesh.uvs.chunks_exact(3).enumerate() {
                let record = &records[triangle * LAYOUT_RECORD..][..LAYOUT_RECORD];
                for (corner, uv) in uvs.iter().enumerate() {
                    assert_eq!(record[32 + corner * 2], uv[0] as f32);
                    assert_eq!(record[33 + corner * 2], uv[1] as f32);
                }
            }
        }
    }
}
