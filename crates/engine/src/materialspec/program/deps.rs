//! Which inputs each output and field of a checked program reads.
//!
//! The analysis walks the resolved program once. A host that bakes the same program over and over
//! with a changed parameter, seed or image can tell from it which outputs the change cannot touch
//! and keep their baked maps. It is conservative: both branches of `select` count, and a helper
//! counts for everything its body reads.
use super::compiled::{Builtin, Compiled, Kind, Node};
use super::{Inputs, Program, ProgramDiagnostic, eval};
use std::collections::{BTreeMap, BTreeSet};

/// What evaluating one output or field can read from the request's inputs. Receiver geometry
/// (coordinates, normals, creases, bounds, gravity) is not listed: it belongs to the layout a bake
/// is keyed by anyway.
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
}

impl Dependencies {
    fn merge(&mut self, other: &Dependencies) {
        self.parameters.extend(&other.parameters);
        self.seed |= other.seed;
        self.images.extend(other.images.iter().cloned());
        self.edges |= other.edges;
        self.fields.extend(&other.fields);
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
            Kind::Number(_) | Kind::Bool(_) | Kind::Text(_) | Kind::Input(_) | Kind::Local(_) => {}
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
                    Builtin::Runoff => found.edges = true,
                    Builtin::SampleColor
                    | Builtin::SampleData
                    | Builtin::SampleAlpha
                    | Builtin::VariedColor => {
                        if let Some(Kind::Text(name)) = args.first().map(|a| &a.kind) {
                            found.images.insert(name.to_string());
                        }
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
}
