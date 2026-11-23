//! Typed stages over private physical lowering and its receiving-surface adapters.
use super::*;

pub(crate) struct PhysicalBuild<'a> {
    pub(super) compiler: Compiler<'a>,
}
pub(crate) struct PhysicalOutput {
    pub(super) product: Product,
}
pub(crate) struct RenderSurfaces {
    pub(super) geometry: BTreeMap<u32, Geometry>,
}
impl<'a> PhysicalBuild<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        prepared: crate::source::PreparedDocument,
        revision: u32,
        assets: &'a dyn Fn(&str) -> Result<Vec<u8>, String>,
        validate_image: &'a dyn Fn(
            &[u8],
        )
            -> Result<crate::assets::image_admission::ImageInfo, String>,
        cancelled: &'a (dyn Fn() -> bool + Sync),
    ) -> Result<(Self, Scene), Diagnostic> {
        let resolution = Resolution::new(prepared.document)?;
        let window_openings = windows::opening_references(&resolution)?;
        let compiler = Compiler {
            cancelled,
            validate_image,
            assets: crate::assets::Sources::new(assets, crate::MAX_SCENE_IMAGE_BYTES),
            resolution,
            cache: BTreeMap::new(),
            pending: BTreeSet::new(),
            surfaces: BTreeMap::new(),
            floors: BTreeMap::new(),
            water: BTreeMap::new(),
            floor_thickness: BTreeMap::new(),
            wall_paths: BTreeMap::new(),
            wall_offsets: BTreeMap::new(),
            wall_silhouettes: BTreeMap::new(),
            window_openings,
            window_fits: BTreeMap::new(),
            flights: BTreeMap::new(),
            volumes: vec![],
            authored: vec![],
            incidents: vec![],
            generated_triangles: 0,
        };
        let mut scene = Scene::empty(revision, "superspec-rust-architecture-v1");
        scene.manifest.id = compiler.resolution.document.root.id.clone();
        scene.manifest.name = compiler
            .resolution
            .document
            .root
            .string("name", &scene.manifest.id);
        scene.manifest.materials = compiler.materials()?;
        Ok((Self { compiler }, scene))
    }
    pub(crate) fn validate_relationships(&self) -> Result<(), Diagnostic> {
        self.compiler.validate_relationships()
    }
    pub(crate) fn lower(&mut self, scene: &mut Scene) -> Result<PhysicalOutput, Diagnostic> {
        let mut output = Product::default();
        for root in self.compiler.resolution.roots.clone() {
            if (self.compiler.cancelled)() {
                return Err(self.root().fail("CANCELLED", "Compilation cancelled."));
            }
            let product = self.compiler.build(root)?;
            output.solids.extend(product.solids);
            output.render.extend(product.render);
            output.patches.extend(product.patches);
        }
        self.compiler
            .emit_collision(scene, &output.solids, &output.patches)?;
        Ok(PhysicalOutput { product: output })
    }
    pub(crate) fn validate_structure(
        &self,
        scene: &Scene,
        output: &PhysicalOutput,
    ) -> Result<(), Diagnostic> {
        self.compiler.validate_structure(scene, &output.product)
    }
    pub(crate) fn surfaces(
        &mut self,
        output: &PhysicalOutput,
        needed: bool,
    ) -> Result<RenderSurfaces, Diagnostic> {
        let geometry = if needed {
            self.compiler.paint(owned_geometry(
                &output.product.render,
                &output.product.patches,
            ))?
        } else {
            BTreeMap::new()
        };
        Ok(RenderSurfaces { geometry })
    }
    pub(crate) fn retain_solids(
        &mut self,
        scene: &mut Scene,
        output: &PhysicalOutput,
    ) -> Result<(), Diagnostic> {
        scene.export_solids = self.compiler.export_solids(&output.product)?;
        Ok(())
    }
    pub(crate) fn render_structure(
        &mut self,
        scene: &mut Scene,
        render: RenderSurfaces,
    ) -> Result<(), Diagnostic> {
        self.compiler.append_owned(scene, render.geometry, false)
    }
    pub(crate) fn context(&self, output: &PhysicalOutput) -> Result<Context, Diagnostic> {
        self.compiler.context(&output.product)
    }
    pub(crate) fn root(&self) -> &Node {
        &self.compiler.resolution.document.root
    }
    pub(crate) fn check_cancelled(&self) -> Result<(), Diagnostic> {
        if (self.compiler.cancelled)() {
            Err(self.root().fail("CANCELLED", "Compilation cancelled."))
        } else {
            Ok(())
        }
    }
    pub(crate) fn publish_material_assets(
        &self,
        scene: &mut Scene,
        source: Option<&Node>,
    ) -> Result<(), Diagnostic> {
        self.compiler.load_material_assets(scene, source)
    }

    pub(crate) fn publish_assets(
        &self,
        scene: &mut Scene,
        fallback_source: Option<&Node>,
    ) -> Result<(), Diagnostic> {
        self.publish_material_assets(scene, fallback_source)?;
        scene.manifest.openings = self
            .compiler
            .resolution
            .nodes
            .iter()
            .filter(|n| n.source.kind == "opening")
            .map(|n| {
                let mut source = n.source.clone();
                source.id = n.id.clone();
                source
            })
            .collect();
        Ok(())
    }
}
