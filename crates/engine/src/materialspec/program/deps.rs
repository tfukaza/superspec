//! Which inputs each output and field of a checked program reads.
//!
//! The analysis walks the resolved program once. A host that bakes the same program over and over
//! with a changed parameter, seed or image can tell from it which outputs the change cannot touch
//! and keep their baked maps. It is conservative: both branches of `select` count, and a helper
//! counts for everything its body reads.
use super::compiled::{Builtin, Compiled, Input, Kind, Node};
use super::{Inputs, Program, ProgramDiagnostic, eval};
use std::collections::{BTreeMap, BTreeSet};

/// The parts of a sample (see [`super::Sample`]) a value can depend on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleInputs(u16);

impl SampleInputs {
    pub const NONE: Self = Self(0);
    pub const UV: Self = Self(1);
    /// The position in the material's two-dimensional domain.
    pub const SURFACE: Self = Self(1 << 1);
    pub const OBJECT: Self = Self(1 << 2);
    pub const WORLD: Self = Self(1 << 3);
    pub const NORMAL: Self = Self(1 << 4);
    pub const GRAVITY: Self = Self(1 << 5);
    /// The sample's size in meters, which positions carry for filtering.
    pub const FOOTPRINT: Self = Self(1 << 6);
    pub const UV_FOOTPRINT: Self = Self(1 << 7);
    pub const BOUNDS: Self = Self(1 << 8);
    /// Render-only: the renderer's clock.
    pub const TIME: Self = Self(1 << 9);
    /// Render-only: the drawn instance's value and attributes.
    pub const INSTANCE: Self = Self(1 << 10);
    /// Render-only: the drawn vertex's attributes.
    pub const VERTEX: Self = Self(1 << 11);
    /// Render-only: the sun of the scene's lighting record.
    pub const SUN: Self = Self(1 << 12);
    /// Render-only: the scene's top-down fields (`sample_field`).
    pub const FIELD: Self = Self(1 << 13);
    /// What only a renderer supplies, which no bake can read.
    pub const RENDER_ONLY: Self =
        Self(Self::TIME.0 | Self::INSTANCE.0 | Self::VERTEX.0 | Self::SUN.0 | Self::FIELD.0);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// The names of these inputs as programs spell them, in a fixed order.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Self::UV, "uv"),
            (Self::SURFACE, "surface"),
            (Self::OBJECT, "object"),
            (Self::WORLD, "world"),
            (Self::NORMAL, "normal"),
            (Self::GRAVITY, "gravity"),
            (Self::FOOTPRINT, "footprint"),
            (Self::UV_FOOTPRINT, "uv_footprint"),
            (Self::BOUNDS, "bounds"),
            (Self::TIME, "time"),
            (Self::INSTANCE, "instance"),
            (Self::VERTEX, "vertex"),
            (Self::SUN, "sun"),
            (Self::FIELD, "field"),
        ]
        .into_iter()
        .filter(|(part, _)| self.contains(*part))
        .map(|(_, name)| name)
        .collect()
    }
    /// These inputs without `other`.
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// What evaluating one output or field can read. A bake's key needs only the request's own
/// inputs (parameters, seed, images, edges), since receiver geometry belongs to the layout a bake
/// is keyed by anyway; `sample` and `features` say which parts of that geometry a value depends
/// on at all, for hosts that share values between layouts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dependencies {
    /// Public parameters, by index into `Program::metadata().parameters`, ascending.
    pub parameters: BTreeSet<usize>,
    /// Whether any `seed(...)` is read, so the request seed matters.
    pub seed: bool,
    /// Names of supplied images that image sampling reads.
    pub images: BTreeSet<String>,
    /// Whether `runoff` reads the supplied source edges.
    pub edges: bool,
    /// Fields read directly or through other fields and helpers, by index into
    /// `Program::metadata().fields`.
    pub fields: BTreeSet<usize>,
    /// The parts of the sample read, directly or by `runoff` and the crease queries. A position
    /// brings its footprint along.
    pub sample: SampleInputs,
    /// Whether crease queries read the receiver's features.
    pub features: bool,
    /// Names of external inputs read with `external(name, fallback)`. A host supplies each as a
    /// plane over a receiver's texels, so what is baked from one is keyed by the plane's content.
    pub externals: BTreeSet<String>,
    /// Names of the receiver's features queried with `feature_distance` and `feature_tangent`.
    pub named_features: BTreeSet<String>,
}

impl Dependencies {
    fn merge(&mut self, other: &Dependencies) {
        self.named_features
            .extend(other.named_features.iter().cloned());
        self.externals.extend(other.externals.iter().cloned());
        self.parameters.extend(&other.parameters);
        self.seed |= other.seed;
        self.images.extend(other.images.iter().cloned());
        self.edges |= other.edges;
        self.fields.extend(&other.fields);
        self.sample = self.sample.union(other.sample);
        self.features |= other.features;
    }

    /// Whether the value is the same at a domain position on every receiver: it reads nothing of
    /// a sample but its `surface` position and footprint, and neither features, source edges nor
    /// external inputs (planes over one receiver's texels). Such a value still changes with the
    /// texel size where what it reads is filtered by footprint.
    pub fn intrinsic(&self) -> bool {
        self.sample
            .without(SampleInputs::SURFACE.union(SampleInputs::FOOTPRINT))
            .is_empty()
            && !self.features
            && !self.edges
            && self.externals.is_empty()
            && self.named_features.is_empty()
    }

    /// Everything either side reads.
    pub fn union(&self, other: &Dependencies) -> Dependencies {
        let mut merged = self.clone();
        merged.merge(other);
        merged
    }
}

/// Dependencies of every output and field of a program, by name.
#[derive(Clone, Debug, Default)]
pub struct DependencyTable {
    pub outputs: BTreeMap<String, Dependencies>,
    pub fields: BTreeMap<String, Dependencies>,
}

struct Walk<'a> {
    compiled: &'a Compiled,
    fields: Vec<Option<Dependencies>>,
    functions: Vec<Option<Dependencies>>,
    /// Fields and helpers being resolved, so a reference cycle (which the checker rejects) cannot
    /// recurse without end.
    open: BTreeSet<(bool, usize)>,
}

impl Walk<'_> {
    fn field(&mut self, index: usize) -> Dependencies {
        if let Some(Some(known)) = self.fields.get(index) {
            return known.clone();
        }
        if index >= self.fields.len() || !self.open.insert((false, index)) {
            return Dependencies::default();
        }
        let compiled = self.compiled;
        let found = self.node(&compiled.fields[index]);
        self.open.remove(&(false, index));
        self.fields[index] = Some(found.clone());
        found
    }

    fn function(&mut self, index: usize) -> Dependencies {
        if let Some(Some(known)) = self.functions.get(index) {
            return known.clone();
        }
        if index >= self.functions.len() || !self.open.insert((true, index)) {
            return Dependencies::default();
        }
        let compiled = self.compiled;
        let found = self.node(&compiled.functions[index].body);
        self.open.remove(&(true, index));
        self.functions[index] = Some(found.clone());
        found
    }

    fn node(&mut self, node: &Node) -> Dependencies {
        let mut found = Dependencies::default();
        match &node.kind {
            Kind::Number(_) | Kind::Bool(_) | Kind::Text(_) | Kind::Local(_) => {}
            Kind::Input(input) => {
                use SampleInputs as S;
                found.sample = match input {
                    Input::Uv => S::UV.union(S::UV_FOOTPRINT),
                    Input::Surface => S::SURFACE.union(S::FOOTPRINT),
                    Input::Object => S::OBJECT.union(S::FOOTPRINT),
                    Input::World => S::WORLD.union(S::FOOTPRINT),
                    Input::Normal => S::NORMAL,
                    Input::Gravity => S::GRAVITY,
                    Input::Footprint => S::FOOTPRINT,
                    Input::UvFootprint => S::UV_FOOTPRINT,
                    Input::BoundsMin | Input::BoundsMax => S::BOUNDS,
                    Input::Time => S::TIME,
                    Input::Instance
                    | Input::InstanceW
                    | Input::InstanceRoot
                    | Input::InstanceTint
                    | Input::InstanceRandom => S::INSTANCE,
                    Input::SunDirection => S::SUN,
                    // Through the instance's placement, with the sample's own footprint.
                    Input::PlacedWorld => S::INSTANCE.union(S::WORLD).union(S::FOOTPRINT),
                };
            }
            Kind::Parameter(index) => {
                found.parameters.insert(*index);
            }
            Kind::Field(index) => {
                found.fields.insert(*index);
                found.merge(&self.field(*index));
            }
            Kind::Unary(_, a) | Kind::Member(a, _) => found.merge(&self.node(a)),
            Kind::Binary(_, a, b) => {
                found.merge(&self.node(a));
                found.merge(&self.node(b));
            }
            Kind::Select(a, b, c) => {
                found.merge(&self.node(a));
                found.merge(&self.node(b));
                found.merge(&self.node(c));
            }
            Kind::Function(index, args) => {
                found.merge(&self.function(*index));
                for arg in args {
                    found.merge(&self.node(arg));
                }
            }
            Kind::Builtin(builtin, args) => {
                for arg in args {
                    found.merge(&self.node(arg));
                }
                match builtin {
                    Builtin::Seed => found.seed = true,
                    Builtin::Runoff => {
                        use SampleInputs as S;
                        found.edges = true;
                        found.sample = found
                            .sample
                            .union(S::WORLD)
                            .union(S::NORMAL)
                            .union(S::GRAVITY)
                            .union(S::FOOTPRINT);
                    }
                    Builtin::CreaseDistance
                    | Builtin::CreaseNormal
                    | Builtin::CreaseTangent
                    | Builtin::CreaseAdjacentNormal => {
                        use SampleInputs as S;
                        found.features = true;
                        found.sample = found
                            .sample
                            .union(S::WORLD)
                            .union(S::NORMAL)
                            .union(S::FOOTPRINT);
                    }
                    Builtin::SampleColor
                    | Builtin::SampleData
                    | Builtin::SampleAlpha
                    | Builtin::AuthoredMask
                    | Builtin::VariedColor => {
                        if let Some(Kind::Text(name)) = args.first().map(|a| &a.kind) {
                            found.images.insert(name.to_string());
                        }
                    }
                    Builtin::FeatureDistance | Builtin::FeatureTangent => {
                        use SampleInputs as S;
                        if let Some(Kind::Text(name)) = args.first().map(|a| &a.kind) {
                            found.named_features.insert(name.to_string());
                        }
                        found.sample = found.sample.union(S::WORLD).union(S::FOOTPRINT);
                    }
                    Builtin::ZoneCoord | Builtin::ZoneExtent => {
                        use SampleInputs as S;
                        // A zone belongs to the receiver's owner, like its features.
                        found.features = true;
                        found.sample = found
                            .sample
                            .union(S::WORLD)
                            .union(S::NORMAL)
                            .union(S::FOOTPRINT);
                    }
                    Builtin::External => {
                        if let Some(Kind::Text(name)) = args.first().map(|a| &a.kind) {
                            found.externals.insert(name.to_string());
                        }
                    }
                    Builtin::InstanceAttribute => {
                        found.sample = found.sample.union(SampleInputs::INSTANCE);
                    }
                    Builtin::VertexAttribute => {
                        found.sample = found.sample.union(SampleInputs::VERTEX);
                    }
                    Builtin::SampleField => {
                        found.sample = found.sample.union(SampleInputs::FIELD);
                    }
                    _ => {}
                }
            }
        }
        found
    }
}

impl Program {
    /// What each output and field reads; see [`Dependencies`].
    pub fn dependency_table(&self) -> DependencyTable {
        let compiled: &Compiled = &self.compiled;
        let mut walk = Walk {
            compiled,
            fields: vec![None; compiled.fields.len()],
            functions: vec![None; compiled.functions.len()],
            open: BTreeSet::new(),
        };
        let mut table = DependencyTable::default();
        for (name, node) in &compiled.outputs {
            table.outputs.insert(name.clone(), walk.node(node));
        }
        for (index, field) in self.metadata.fields.iter().enumerate() {
            table.fields.insert(field.name.clone(), walk.field(index));
        }
        table
    }

    /// Canonical bytes of the parts of `inputs` that `dependencies` reads: the effective value of
    /// each listed parameter (the declared default where the request names none), the seed and
    /// the source edges. Equal bytes mean the program computes equal values; images are the
    /// caller's to identify by content. Validates `inputs` exactly like an evaluator does, so
    /// the same invalid request fails with the same diagnostic.
    pub fn input_fingerprint(
        &self,
        inputs: &Inputs,
        dependencies: &Dependencies,
    ) -> Result<Vec<u8>, ProgramDiagnostic> {
        eval::fingerprint(self, inputs, dependencies)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Sources, compile};

    fn program(source: &str) -> super::Program {
        compile(&Sources {
            entry: "main.ms".into(),
            modules: [("main.ms".to_owned(), source.to_owned())].into(),
        })
        .unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn outputs_report_the_parameters_seeds_and_images_they_read_through_fields_and_helpers() {
        let program = program(
            "material 1;
             param tint: scalar = 0.5;
             param shine: scalar = 0.2;
             param unused: scalar = 1;
             fn lift(x: scalar) -> scalar = x + shine;
             let grain = noise(uv * 8, seed(\"grain\"));
             let shade = lift(tint) * grain;
             output base_color = vec3(shade);
             output roughness = shine;
             output metalness = 0;",
        );
        let table = program.dependency_table();
        let index = |name: &str| {
            program
                .metadata()
                .parameters
                .iter()
                .position(|p| p.name == name)
                .unwrap()
        };
        let color = &table.outputs["base_color"];
        assert_eq!(
            color.parameters,
            [index("tint"), index("shine")].into_iter().collect()
        );
        assert!(color.seed);
        let roughness = &table.outputs["roughness"];
        assert_eq!(roughness.parameters.len(), 1);
        assert!(roughness.parameters.contains(&index("shine")));
        assert!(!roughness.seed && !roughness.edges && roughness.images.is_empty());
        assert!(table.outputs["metalness"].parameters.is_empty());
        assert!(!table.fields["shade"].parameters.contains(&index("unused")));
        assert!(table.fields["grain"].seed);
    }

    #[test]
    fn values_report_the_parts_of_a_sample_and_the_features_they_read() {
        use super::SampleInputs as S;
        let program = program(
            "material 1;
             param tint: scalar = 0.5;
             fn lift(p: vec2) -> scalar = noise(p * 8, seed(\"grain\"));
             let grain = lift(surface);
             let facing = normal.z * tint;
             let worn = crease_distance(\"convex\", 30.0, 0.05);
             let wet = runoff(0.1, 0.5);
             let both = grain * facing;
             let size = footprint + bounds_max.x - bounds_min.x;
             output base_color = vec3(grain);
             output roughness = worn;
             output metalness = 0;
             output height = wet + both + uv.x + object.y + gravity.z + size;",
        );
        let table = program.dependency_table();
        let grain = &table.fields["grain"];
        assert_eq!(grain.sample, S::SURFACE.union(S::FOOTPRINT));
        assert!(grain.intrinsic() && !grain.features);
        assert!(table.outputs["base_color"].intrinsic());
        let facing = &table.fields["facing"];
        assert_eq!(facing.sample, S::NORMAL);
        assert!(!facing.intrinsic());
        let worn = &table.fields["worn"];
        assert!(worn.features && !worn.intrinsic());
        assert_eq!(worn.sample, S::WORLD.union(S::NORMAL).union(S::FOOTPRINT));
        assert!(table.outputs["roughness"].features);
        let wet = &table.fields["wet"];
        assert!(wet.edges && !wet.features && !wet.intrinsic());
        assert!(wet.sample.contains(S::WORLD.union(S::GRAVITY)));
        assert_eq!(
            table.fields["both"].sample,
            S::SURFACE.union(S::FOOTPRINT).union(S::NORMAL)
        );
        assert_eq!(table.fields["size"].sample, S::FOOTPRINT.union(S::BOUNDS));
        let height = &table.outputs["height"];
        for part in [
            S::UV,
            S::UV_FOOTPRINT,
            S::SURFACE,
            S::OBJECT,
            S::WORLD,
            S::NORMAL,
            S::GRAVITY,
            S::FOOTPRINT,
            S::BOUNDS,
        ] {
            assert!(height.sample.contains(part), "{part:?}");
        }
        assert!(table.outputs["metalness"].sample.is_empty());
        assert!(table.outputs["metalness"].intrinsic());
    }
}
