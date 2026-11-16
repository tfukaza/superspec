//! A checked program with every name and call resolved once, before any sample is evaluated.
//!
//! Inputs, parameters, fields and helper arguments become slots and helper calls become indices,
//! so evaluation never searches by string. Evaluation order, step accounting and diagnostics are
//! those of the syntax tree the nodes were resolved from.
use super::syntax::{Definition, Expr, Expression, Mark};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Input {
    Uv,
    Surface,
    Object,
    World,
    Normal,
    Gravity,
    Footprint,
    UvFootprint,
    BoundsMin,
    BoundsMax,
}

impl Input {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "uv" => Self::Uv,
            "surface" => Self::Surface,
            "object" => Self::Object,
            "world" => Self::World,
            "normal" => Self::Normal,
            "gravity" => Self::Gravity,
            "footprint" => Self::Footprint,
            "uv_footprint" => Self::UvFootprint,
            "bounds_min" => Self::BoundsMin,
            "bounds_max" => Self::BoundsMax,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) enum Kind {
    Number(f64),
    Bool(bool),
    Text(Arc<str>),
    Input(Input),
    /// Index into the program's parameters, in metadata order.
    Parameter(usize),
    /// Index into the program's fields, in metadata order.
    Field(usize),
    /// Index into the enclosing helper's arguments.
    Local(usize),
    Unary(Op, Box<Node>),
    Binary(Op, Box<Node>, Box<Node>),
    /// Selected component indices, in order.
    Member(Box<Node>, Vec<usize>),
    Select(Box<Node>, Box<Node>, Box<Node>),
    /// Index into the program's helpers.
    Function(usize, Vec<Node>),
    /// A catalog operation.
    Builtin(Builtin, Vec<Node>),
}

/// A unary or binary operator, resolved from its spelling once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Not,
}

impl Op {
    /// The checker admits only known operators; any other spelling reads as `+`, as before.
    fn named(op: &str) -> Self {
        match op {
            "-" => Self::Sub,
            "*" => Self::Mul,
            "/" => Self::Div,
            "%" => Self::Rem,
            "&&" => Self::And,
            "||" => Self::Or,
            "==" => Self::Equal,
            "!=" => Self::NotEqual,
            "<" => Self::Less,
            ">" => Self::Greater,
            "<=" => Self::LessEqual,
            ">=" => Self::GreaterEqual,
            "!" => Self::Not,
            _ => Self::Add,
        }
    }
}

/// A catalog operation the evaluator implements, resolved from its identifier once so samples
/// never compare strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Builtin {
    Vec2,
    Vec3,
    Mix,
    Clamp,
    Smoothstep,
    Abs,
    Floor,
    Fract,
    Sin,
    Cos,
    Sqrt,
    Min,
    Max,
    Step,
    ValueNoise,
    Hash,
    Dot,
    Cross,
    Length,
    Normalize,
    Pow,
    Exp,
    Seed,
    Noise,
    Fbm,
    Cellular,
    Scale,
    Offset,
    Warp,
    Repeat,
    Rotate,
    SampleColor,
    SampleData,
    SampleAlpha,
    Runoff,
    CreaseDistance,
    CreaseNormal,
    CreaseTangent,
    CreaseAdjacentNormal,
    AdjustNormal,
    TriplanarWeights,
    VariedColor,
    Remap,
    SoftThreshold,
    DistanceBand,
    SoftUnion,
    MaskIntersect,
    MaskSubtract,
    /// A name outside the catalog; the checker rejects these before evaluation.
    Unknown,
}

impl Builtin {
    fn named(id: &str) -> Self {
        match id {
            "vec2" => Self::Vec2,
            "vec3" => Self::Vec3,
            "mix" => Self::Mix,
            "clamp" => Self::Clamp,
            "smoothstep" => Self::Smoothstep,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "fract" => Self::Fract,
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "sqrt" => Self::Sqrt,
            "min" => Self::Min,
            "max" => Self::Max,
            "step" => Self::Step,
            "value_noise" => Self::ValueNoise,
            "hash" => Self::Hash,
            "dot" => Self::Dot,
            "cross" => Self::Cross,
            "length" => Self::Length,
            "normalize" => Self::Normalize,
            "pow" => Self::Pow,
            "exp" => Self::Exp,
            "seed" => Self::Seed,
            "noise" => Self::Noise,
            "fbm" => Self::Fbm,
            "cellular" => Self::Cellular,
            "scale" => Self::Scale,
            "offset" => Self::Offset,
            "warp" => Self::Warp,
            "repeat" => Self::Repeat,
            "rotate" => Self::Rotate,
            "sample_color" => Self::SampleColor,
            "sample_data" => Self::SampleData,
            "sample_alpha" => Self::SampleAlpha,
            "runoff" => Self::Runoff,
            "crease_distance" => Self::CreaseDistance,
            "crease_normal" => Self::CreaseNormal,
            "crease_tangent" => Self::CreaseTangent,
            "crease_adjacent_normal" => Self::CreaseAdjacentNormal,
            "adjust_normal" => Self::AdjustNormal,
            "triplanar_weights" => Self::TriplanarWeights,
            "varied_color" => Self::VariedColor,
            "remap" => Self::Remap,
            "soft_threshold" => Self::SoftThreshold,
            "distance_band" => Self::DistanceBand,
            "soft_union" => Self::SoftUnion,
            "mask_intersect" => Self::MaskIntersect,
            "mask_subtract" => Self::MaskSubtract,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Node {
    pub kind: Kind,
    pub mark: Mark,
}

#[derive(Clone, Debug)]
pub(super) struct Function {
    pub body: Node,
}

/// Resolved bodies of every field, helper and output of one checked program.
#[derive(Clone, Debug, Default)]
pub(super) struct Compiled {
    pub fields: Vec<Node>,
    pub functions: Vec<Function>,
    pub outputs: Vec<(String, Node)>,
    /// Conservative retained allocation charge of these nodes.
    pub retained: usize,
}

struct Resolver<'a> {
    parameters: BTreeMap<&'a str, usize>,
    fields: BTreeMap<&'a str, usize>,
    functions: BTreeMap<&'a str, usize>,
    retained: usize,
}

impl Resolver<'_> {
    fn node(&mut self, expr: &Expr, locals: &[&str]) -> Node {
        self.retained = self
            .retained
            .saturating_add(size_of::<Node>() + expr.mark.module.capacity());
        let kind = match &expr.kind {
            Expression::Number(n) => Kind::Number(*n),
            Expression::Bool(b) => Kind::Bool(*b),
            Expression::Text(s) => {
                self.retained = self.retained.saturating_add(s.len());
                Kind::Text(Arc::from(s.as_str()))
            }
            Expression::Name(name) => {
                if let Some(index) = locals.iter().position(|local| local == name) {
                    Kind::Local(index)
                } else if let Some(input) = Input::named(name) {
                    Kind::Input(input)
                } else if let Some(index) = self.parameters.get(name.as_str()) {
                    Kind::Parameter(*index)
                } else {
                    // The checker admits only declared names; an unknown one fails at use.
                    Kind::Field(
                        self.fields
                            .get(name.as_str())
                            .copied()
                            .unwrap_or(usize::MAX),
                    )
                }
            }
            Expression::Unary(op, a) => Kind::Unary(Op::named(op), Box::new(self.node(a, locals))),
            Expression::Binary(op, a, b) => Kind::Binary(
                Op::named(op),
                Box::new(self.node(a, locals)),
                Box::new(self.node(b, locals)),
            ),
            Expression::Member(value, member) => {
                let components = member
                    .chars()
                    .map(|c| super::check::component(c).unwrap_or(0))
                    .collect::<Vec<_>>();
                self.retained = self
                    .retained
                    .saturating_add(components.capacity() * size_of::<usize>());
                Kind::Member(Box::new(self.node(value, locals)), components)
            }
            Expression::Call(name, args) if name == "select" && args.len() == 3 => Kind::Select(
                Box::new(self.node(&args[0], locals)),
                Box::new(self.node(&args[1], locals)),
                Box::new(self.node(&args[2], locals)),
            ),
            Expression::Call(name, args) => {
                let resolved = args
                    .iter()
                    .map(|arg| self.node(arg, locals))
                    .collect::<Vec<_>>();
                self.retained = self
                    .retained
                    .saturating_add(resolved.capacity() * size_of::<Node>());
                match self.functions.get(name.as_str()) {
                    Some(index) => Kind::Function(*index, resolved),
                    None => Kind::Builtin(
                        super::super::catalog::find(name)
                            .map_or(Builtin::Unknown, |operation| Builtin::named(operation.id)),
                        resolved,
                    ),
                }
            }
        };
        Node {
            kind,
            mark: expr.mark.clone(),
        }
    }
}

/// Resolves a checked program. Parameters and fields are numbered in definition order, which is
/// the order of the program's metadata.
pub(super) fn resolve(
    definitions: &BTreeMap<String, Definition>,
    outputs: &BTreeMap<String, Expr>,
) -> Compiled {
    let mut resolver = Resolver {
        parameters: BTreeMap::new(),
        fields: BTreeMap::new(),
        functions: BTreeMap::new(),
        retained: 0,
    };
    for (name, definition) in definitions {
        let table = match definition {
            Definition::Parameter { .. } => &mut resolver.parameters,
            Definition::Field { .. } => &mut resolver.fields,
            Definition::Function { .. } => &mut resolver.functions,
        };
        let index = table.len();
        table.insert(name.as_str(), index);
    }
    let mut fields = Vec::new();
    let mut functions = Vec::new();
    for definition in definitions.values() {
        match definition {
            Definition::Field { value, .. } => fields.push(resolver.node(value, &[])),
            Definition::Function {
                arguments, value, ..
            } => {
                let locals = arguments
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                functions.push(Function {
                    body: resolver.node(value, &locals),
                });
            }
            Definition::Parameter { .. } => {}
        }
    }
    let outputs = outputs
        .iter()
        .map(|(name, expr)| (name.clone(), resolver.node(expr, &[])))
        .collect::<Vec<_>>();
    let retained = resolver.retained.saturating_add(
        size_of::<Compiled>()
            + fields.capacity() * size_of::<Node>()
            + functions.capacity() * size_of::<Function>()
            + outputs
                .iter()
                .map(|(name, _)| name.capacity() + size_of::<(String, Node)>())
                .sum::<usize>(),
    );
    Compiled {
        fields,
        functions,
        outputs,
        retained,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_operation_resolves_to_an_evaluator_builtin() {
        for operation in super::super::super::catalog::operations() {
            // `select` evaluates lazily as its own node kind.
            if operation.id != "select" {
                assert_ne!(
                    Builtin::named(operation.id),
                    Builtin::Unknown,
                    "{}",
                    operation.id
                );
            }
        }
    }
}

/// Resolves one constant expression (a parameter default), which names no bindings.
pub(super) fn constant(expr: &Expr) -> Node {
    Resolver {
        parameters: BTreeMap::new(),
        fields: BTreeMap::new(),
        functions: BTreeMap::new(),
        retained: 0,
    }
    .node(expr, &[])
}
