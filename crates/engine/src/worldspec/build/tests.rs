use super::*;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};
use superspec_model::visual::{Surface, Vertex, VisualInput};

const SOURCE: &str = "<architecture version=\"1\" id=\"stage\" name=\"😀\"><box id=\"host\" width=\"2\" depth=\"3\" height=\"0.2\"/></architecture>";

struct NoTheme;
impl ThemeAdapter for NoTheme {}

fn compile(
    theme: &mut dyn ThemeAdapter,
    cancelled: &AtomicBool,
    phases: &RefCell<Vec<String>>,
) -> Result<Scene, Diagnostic> {
    compile_with_assets(theme, cancelled, phases, &|path| {
        Err(format!("Missing {path}"))
    })
}
fn compile_with_assets(
    theme: &mut dyn ThemeAdapter,
    cancelled: &AtomicBool,
    phases: &RefCell<Vec<String>>,
    assets: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Scene, Diagnostic> {
    let prepared =
        crate::source::prepare(SOURCE, &superspec_aml::extensions::Registry::default()).unwrap();
    compile_prepared(
        prepared,
        7,
        assets,
        true,
        &crate::assets::image_admission::validate_image,
        &|| cancelled.load(Ordering::SeqCst),
        false,
        &|phase| phases.borrow_mut().push(phase.into()),
        theme,
    )
}

fn physical(scene: &Scene) -> serde_json::Value {
    let triangles: Vec<_> = scene
        .manifest
        .colliders
        .iter()
        .map(|id| {
            let mesh = scene
                .manifest
                .meshes
                .iter()
                .find(|mesh| &mesh.id == id)
                .unwrap();
            let points: Vec<_> = scene.indices
                [mesh.index_start..mesh.index_start + mesh.index_count]
                .iter()
                .map(|index| {
                    let i = (mesh.vertex_start + *index as usize) * 3;
                    scene.positions[i..i + 3].to_vec()
                })
                .collect();
            serde_json::json!({"mesh":mesh,"points":points})
        })
        .collect();
    let context = scene.manifest.architecture.as_ref().unwrap();
    serde_json::json!({
        "triangles":triangles,"source":context.source("host"),
        "geometryValidation":scene.manifest.geometry_validation,
        "navigationValidation":scene.manifest.navigation_validation,
        "validation":scene.manifest.validation,"diagnostics":scene.manifest.diagnostics,
        "reviews":scene.manifest.reviews,"openings":scene.manifest.openings,
        "floors":context.floors,"connections":context.connections
    })
}

#[test]
fn no_theme_matches_plain_shipped_packet_and_physical_contract() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let scene = compile(&mut NoTheme, &cancelled, &phases).unwrap();
    let expected = crate::worldspec::shipped::compile(SOURCE, 7).unwrap();
    assert_eq!(scene.packet(), expected.packet());
    assert!(!scene.manifest.colliders.is_empty());
    assert_eq!(scene.manifest.geometry_validation, "not-run");
    assert_eq!(scene.manifest.navigation_validation, "not-run");
    assert!(scene.manifest.validation.is_none());
    assert_eq!(
        *phases.borrow(),
        [
            "physical-solids",
            "structural-validation",
            "face-materials",
            "architectural-render",
            "physical-context",
            "architectural-review",
            "asset-publication",
            "complete"
        ]
    );
}

struct Triangle {
    vertices: [Vertex; 3],
    surfaces: [Surface; 1],
}
impl Default for Triangle {
    fn default() -> Self {
        Self {
            vertices: [[0., 0., 1.], [1., 0., 1.], [0., 1., 1.]].map(|position| Vertex {
                position,
                normal: [0., 0., 1.],
                uv: [position[0], position[1]],
                color: [1.; 3],
            }),
            surfaces: [Surface {
                part: "marker".into(),
                slot: "finish".into(),
            }],
        }
    }
}
impl VisualInput for Triangle {
    fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    fn indices(&self) -> &[u32] {
        &[0, 1, 2]
    }
    fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }
    fn triangle_surfaces(&self) -> &[usize] {
        &[0]
    }
    fn surveying(&self) -> bool {
        false
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Failure {
    None,
    Prepare,
    Publish,
    CancelPublish,
}
struct MarkerTheme<'a> {
    events: &'a RefCell<Vec<&'static str>>,
    cancelled: &'a AtomicBool,
    failure: Failure,
    source: Option<Node>,
    textured: bool,
}
impl<'a> MarkerTheme<'a> {
    fn new(events: &'a RefCell<Vec<&'static str>>, cancelled: &'a AtomicBool) -> Self {
        Self {
            events,
            cancelled,
            failure: Failure::None,
            source: None,
            textured: false,
        }
    }
}
impl ThemeAdapter for MarkerTheme<'_> {
    fn prepare(&mut self, physical: &PhysicalBuild<'_>, _: &[Material]) -> Result<(), Diagnostic> {
        self.events.borrow_mut().push("prepare");
        self.source = Some(physical.root().clone());
        if self.failure == Failure::Prepare {
            return Err(physical
                .root()
                .fail("MARKER_PREPARE", "Marker preparation failed."));
        }
        Ok(())
    }
    fn plan(
        &mut self,
        _: &PhysicalBuild<'_>,
        _: &mut Vec<Diagnostic>,
        _: &PhysicalOutput,
        _: &RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        self.events.borrow_mut().push("plan");
        Ok(())
    }
    fn render_structure(
        &mut self,
        physical: &mut PhysicalBuild<'_>,
        scene: &mut Scene,
        render: RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        self.events.borrow_mut().push("structure");
        physical.render_structure(scene, render)
    }
    fn publish(
        &mut self,
        physical: &PhysicalBuild<'_>,
        visuals: &mut VisualOutput<'_>,
    ) -> Result<(), Diagnostic> {
        self.events.borrow_mut().push("publish");
        let material = Material {
            id: "marker-finish".into(),
            color: "#44aaee".into(),
            texture: self.textured.then(|| "marker.png".into()),
            ..Default::default()
        };
        visuals
            .ensure_material(&material)
            .map_err(|e| physical.root().fail("MARKER_VISUAL", e))?;
        visuals
            .append(
                "host",
                &Triangle::default(),
                &BTreeMap::from([("finish".into(), "marker-finish".into())]),
                &[] as &[crate::materialspec::bake::BakedMaterial],
            )
            .map_err(|e| physical.root().fail("MARKER_VISUAL", e))?;
        if self.failure == Failure::Publish {
            return Err(physical
                .root()
                .fail("MARKER_PUBLISH", "Marker publication failed."));
        }
        if self.failure == Failure::CancelPublish {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        Ok(())
    }
    fn fallback_source(&self) -> Option<&Node> {
        self.source.as_ref()
    }
}

#[test]
fn alternate_adapter_adds_only_its_visual_and_preserves_phase_order() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let events = RefCell::new(vec![]);
    let plain = compile(&mut NoTheme, &cancelled, &phases).unwrap();
    let scene = compile(
        &mut MarkerTheme::new(&events, &cancelled),
        &cancelled,
        &phases,
    )
    .unwrap();
    assert_eq!(
        *events.borrow(),
        ["prepare", "plan", "structure", "publish"]
    );
    assert_eq!(physical(&plain), physical(&scene));
    assert_eq!(scene.manifest.meshes.len(), plain.manifest.meshes.len() + 1);
    assert_eq!(scene.manifest.vertex_count, plain.manifest.vertex_count + 3);
    let marker = scene.manifest.meshes.last().unwrap();
    assert_eq!(marker.id, "host/$dressing/marker:finish");
    assert_eq!(marker.material.as_deref(), Some("marker-finish"));
    assert_eq!(
        marker.source,
        plain
            .manifest
            .architecture
            .as_ref()
            .unwrap()
            .source("host")
            .unwrap()
            .clone()
    );
    assert_eq!(
        &scene.positions[marker.vertex_start * 3..],
        [0., 0., 1., 1., 0., 1., 0., 1., 1.]
    );
}

#[test]
fn adapter_failure_and_cancellation_never_publish_a_partial_scene_and_recover() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let events = RefCell::new(vec![]);
    let mut theme = MarkerTheme::new(&events, &cancelled);
    let expected = compile(&mut theme, &cancelled, &phases).unwrap().packet();
    let source = theme.source.as_ref().unwrap().source.clone();
    for (failure, code) in [
        (Failure::Prepare, "MARKER_PREPARE"),
        (Failure::Publish, "MARKER_PUBLISH"),
        (Failure::CancelPublish, "CANCELLED"),
    ] {
        theme.failure = failure;
        phases.borrow_mut().clear();
        let error = compile(&mut theme, &cancelled, &phases).unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.source, source);
        assert!(!phases.borrow().iter().any(|phase| phase == "complete"));
        cancelled.store(false, Ordering::SeqCst);
        theme.failure = Failure::None;
        assert_eq!(
            compile(&mut theme, &cancelled, &phases).unwrap().packet(),
            expected
        );
    }
}

#[test]
fn adapter_asset_errors_use_its_source_and_cancelled_asset_loads_cannot_publish() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let events = RefCell::new(vec![]);
    let mut theme = MarkerTheme::new(&events, &cancelled);
    theme.textured = true;
    let error = compile(&mut theme, &cancelled, &phases).unwrap_err();
    assert_eq!(error.code, "AML_ASSET");
    assert_eq!(error.message.as_ref(), "Missing marker.png");
    assert_eq!(error.source, theme.source.as_ref().unwrap().source);
    let png = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 1,
        height: 1,
        rgba: vec![20, 80, 120, 255],
        mime: "image/png",
    })
    .unwrap();
    let error = compile_with_assets(&mut theme, &cancelled, &phases, &|name| {
        assert_eq!(name, "marker.png");
        cancelled.store(true, Ordering::SeqCst);
        Ok(png.clone())
    })
    .unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    cancelled.store(false, Ordering::SeqCst);
    let recovered =
        compile_with_assets(&mut theme, &cancelled, &phases, &|_| Ok(png.clone())).unwrap();
    assert_eq!(recovered.manifest.assets.len(), 1);
    assert_eq!(recovered.asset_bytes, png);
}

struct HostedItem {
    collidable: bool,
    texture: Option<&'static str>,
}

impl ThemeAdapter for HostedItem {
    fn place_items<'build>(
        &self,
        physical: &'build PhysicalBuild<'_>,
        items: ItemOutput<'build>,
    ) -> Result<ItemOutput<'build>, Diagnostic> {
        use superspec_model::instances::{IDENTITY, Instance, LocalMesh, Model};
        let triangle = Triangle::default().vertices;
        let model = Model {
            meshes: BTreeMap::from([(
                "triangle".into(),
                std::sync::Arc::new(LocalMesh {
                    positions: triangle.iter().map(|v| v.position).collect(),
                    normals: triangle.iter().map(|v| v.normal).collect(),
                    uvs: triangle.iter().map(|v| v.uv).collect(),
                    colors: triangle.iter().map(|v| v.color).collect(),
                    indices: vec![0, 1, 2],
                    tangents: Vec::new(),
                }),
            )]),
            instances: vec![Instance {
                id: "host/finish".into(),
                object_id: "host".into(),
                mesh: "triangle".into(),
                material: None,
                transform: IDENTITY,
                source: physical.root().source.clone(),
                visible: true,
                collidable: self.collidable,
            }],
            ..Default::default()
        };
        let fail = |e: String| physical.root().fail("TEST_ITEM", e);
        let mut items = items.attach_visual("host", model).map_err(fail)?;
        let owners = ["host".into()].into();
        let source = physical.root();
        items
            .bind_slots(
                source,
                &owners,
                |_| Some("item-finish".into()),
                |id| {
                    Some(Material {
                        id: id.into(),
                        texture: self.texture.map(str::to_owned),
                        ..Default::default()
                    })
                },
            )
            .map_err(fail)?;
        // Swallow a failed lookup deliberately: it must leave every previous binding usable.
        items
            .bind_slots(source, &owners, |_| Some("absent".into()), |_| None)
            .unwrap_err();
        Ok(items)
    }
}

#[test]
fn item_bindings_are_scoped_to_new_meshes_and_hosted_parts_preserve_physical_review() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let base = compile(&mut NoTheme, &cancelled, &phases).unwrap();
    let protected = serde_json::to_value(&base.manifest.meshes).unwrap();
    let scene = compile(
        &mut HostedItem {
            collidable: false,
            texture: None,
        },
        &cancelled,
        &phases,
    )
    .unwrap();
    assert_eq!(physical(&base), physical(&scene));
    assert_eq!(
        protected,
        serde_json::to_value(&scene.manifest.meshes[..base.manifest.meshes.len()]).unwrap()
    );
    assert_eq!(
        scene.manifest.meshes.last().unwrap().material.as_deref(),
        Some("item-finish")
    );
    assert_eq!(
        scene
            .manifest
            .materials
            .iter()
            .map(|m| m.id.as_str())
            .filter(|id| *id == "item-finish")
            .count(),
        1
    );
    let error = compile(
        &mut HostedItem {
            collidable: true,
            texture: None,
        },
        &cancelled,
        &phases,
    )
    .unwrap_err();
    assert_eq!(error.code, "TEST_ITEM");
    assert!(error.message.contains("render-only"));
}

#[test]
fn item_slot_companions_are_published_after_placement() {
    let cancelled = AtomicBool::new(false);
    let phases = RefCell::new(vec![]);
    let mut theme = HostedItem {
        collidable: false,
        texture: Some("fixture.png"),
    };
    let missing = compile(&mut theme, &cancelled, &phases).unwrap_err();
    assert_eq!(missing.code, "AML_ASSET");
    assert!(missing.message.contains("fixture.png"));
    let bytes = superspec_geometry::image::encode(&superspec_geometry::image::Image {
        width: 1,
        height: 1,
        rgba: vec![130, 140, 150, 255],
        mime: "image/png",
    })
    .unwrap();
    let scene = compile_with_assets(&mut theme, &cancelled, &phases, &|name| {
        assert_eq!(name, "fixture.png");
        Ok(bytes.clone())
    })
    .unwrap();
    assert_eq!(scene.manifest.assets.len(), 1);
    assert_eq!(scene.manifest.assets[0].id, "fixture.png");
    assert_eq!(scene.asset_bytes, bytes);
}

#[test]
fn visual_material_reuse_requires_the_same_definition() {
    let mut scene = Scene::empty(1, "fixture");
    let material = Material {
        id: "fixture".into(),
        ..Default::default()
    };
    let mut visuals = VisualOutput(&mut scene);
    visuals.ensure_material(&material).unwrap();
    visuals.ensure_material(&material).unwrap();
    let mut conflict = material.clone();
    conflict.color = "#ff00aa".into();
    assert!(
        visuals
            .ensure_material(&conflict)
            .unwrap_err()
            .contains("fixture")
    );
    assert_eq!(scene.manifest.materials, [material]);
}
