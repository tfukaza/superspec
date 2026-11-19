//! Materialize local ItemSpec resources, then place their instances in AML source order.
use crate::itemspec::definition::AssetReader;
use crate::{Scene, Session, authoring::Request};
use superspec_model::composition::WorldModel;
use superspec_types::{architecture::Node, diagnostic::Diagnostic};

pub(crate) fn place<'a>(
    scene: Scene,
    placements: impl Iterator<Item = &'a Node>,
    item: impl Fn(&Node) -> Result<Request, String>,
    assets: &AssetReader<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Scene, Diagnostic> {
    let revision = scene.manifest.revision;
    let mut placements = placements.peekable();
    let Some(first) = placements.peek() else {
        return Ok(scene);
    };
    let fail = |s: String| first.fail("AML_CITY", s);
    let mut model = WorldModel::from_scene(scene).map_err(fail)?;
    let mut session = Session::default();
    for node in placements {
        let fail = |s: String| node.fail("AML_CITY", s);
        let mut request = item(node).map_err(fail)?;
        let origin = node.vector("position", [0.; 3]);
        let step = node.vector("step", [0.; 3]);
        let (sin, cos) = node.number("heading", 0.).to_radians().sin_cos();
        request.placement_transform = Some([[cos, sin, 0.], [-sin, cos, 0.], [0., 0., 1.], origin]);
        let local = session
            .build(&request, revision.into(), assets, cancelled)
            .map_err(|errors| {
                fail(
                    errors
                        .into_iter()
                        .map(|e| format!("{} {}", e.source_name, e.message))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })?
            .model;
        let count = node.number("count", 1.) as usize;
        for i in 0..count {
            if cancelled() {
                return Err(node.fail("CANCELLED", "City placement cancelled."));
            }
            let mut placed = local.clone();
            let owner = superspec_profiles::city::placement_id(&node.id, count, i + 1);
            for instance in &mut placed.instances {
                instance.id = format!("{owner}/{}", instance.id);
                instance.object_id = owner.clone();
                instance.source = node.source.clone();
                for (axis, delta) in step.iter().enumerate() {
                    instance.transform[12 + axis] += i as f64 * delta;
                }
            }
            model.append(placed).map_err(fail)?;
        }
    }
    model.scene_cancellable(revision, cancelled).map_err(fail)
}
