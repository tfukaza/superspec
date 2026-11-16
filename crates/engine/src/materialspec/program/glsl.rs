//! A GLSL preview of a checked program.
//!
//! The baker evaluates a program per texel on the CPU; a renderer can instead evaluate the same
//! program per pixel, so an edit shows without a bake. This emits GLSL ES 3.00: one function per
//! helper, one global per field that the outputs reach, and `ms_evaluate()`, which fills the
//! channel globals. Parameters and named seeds are uniforms, so changing them needs no new shader.
//!
//! The preview is close to a bake, not identical. Lattice hashing uses the same 32-bit integer
//! arithmetic, but values are 32-bit floats, filtering follows screen derivatives where the baker
//! propagates a footprint, both branches of a `select` are computed, and `runoff` reads no edges.
//! Bakes stay the authority for exported maps.
use super::{
    Program, Type,
    compiled::{Builtin, Compiled, Input, Kind, Node, Op},
    syntax::Definition,
};
use serde::Serialize;
use std::collections::BTreeMap;

const RUNTIME: &str = include_str!("glsl_runtime.glsl");

/// A program as fragment shader source, with what the host must bind.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shader {
    /// Uniform declarations, the runtime, the program's functions and `void ms_evaluate()`.
    /// Before calling it the host sets `ms_world`, `ms_object`, `ms_normal`, `ms_uv`,
    /// `ms_surface`, `ms_footprint` and `ms_uv_footprint`; afterwards it reads `ms_base_color`,
    /// `ms_roughness`, `ms_metalness`, `ms_height`, `ms_emissive`, `ms_opacity`,
    /// `ms_receiver_normal` and `ms_normal_offset`.
    pub source: String,
    pub parameters: Vec<Uniform>,
    /// Named seeds: each uniform is `seed(name)` for the request's seed, as an unsigned integer.
    pub seeds: Vec<Uniform>,
    pub images: Vec<ImageUniform>,
    /// Channels the program sets; the others keep their defaults.
    pub outputs: Vec<String>,
    pub live: Vec<LiveOperation>,
    /// Whether the program queries receiver features (`ms_features`, `ms_feature_count`).
    pub features: bool,
    /// Fields the shader reads from textures because they use CPU-only operations (`runoff`), set
    /// only by [`Program::shader_plan`]. Everything they read upstream stays out of the shader;
    /// everything that reads them downstream stays in it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu_fields: Vec<CpuField>,
    /// Outputs that read a CPU field (through the shader's own math), so a host can say which channels of a preview rest on CPU data.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu_outputs: Vec<String>,
}

/// A field the host bakes on the CPU into a float texture and binds to the shader.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuField {
    pub name: String,
    /// The `sampler2D` uniform. Texel `(x, y)` of the bake grid (rows from the upper edge) holds
    /// the field's value at that texel: `.x` for a scalar or bool, `.xy` and `.xyz` for vectors.
    pub uniform: String,
    #[serde(rename = "type")]
    pub kind: Type,
    /// What the field reads (through everything upstream of it), so a host knows when to bake
    /// it again: a change to none of these leaves the baked texture valid.
    pub parameters: Vec<String>,
    pub seed: bool,
    pub images: Vec<String>,
    /// Whether the field reads the supplied source edges.
    pub edges: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Uniform {
    pub name: String,
    pub uniform: String,
    #[serde(rename = "type")]
    pub kind: Type,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImageUniform {
    pub name: String,
    pub uniform: String,
    /// Linear byte values (`sample_data`, `sample_alpha`) rather than decoded sRGB color.
    pub linear: bool,
}

/// A live renderer operation with a GLSL expression for each argument.
#[derive(Clone, Debug, Serialize)]
pub struct LiveOperation {
    pub id: String,
    pub arguments: BTreeMap<String, String>,
}

fn glsl_type(kind: Type) -> &'static str {
    match kind {
        Type::Scalar => "float",
        Type::Vec2 => "vec2",
        Type::Vec3 => "vec3",
        Type::Bool => "bool",
        Type::String => "int",
    }
}

fn literal(value: f64) -> String {
    let text = format!("{value:?}");
    if value < 0. {
        format!("({text})")
    } else {
        text
    }
}

fn wider(a: Type, b: Type) -> Type {
    if a == Type::Scalar { b } else { a }
}

/// `text` of type `from` as a value of type `to` (a scalar splats; equal types pass through).
fn promote(text: &str, from: Type, to: Type) -> String {
    if from == to || from != Type::Scalar {
        text.to_owned()
    } else {
        format!("{}({text})", glsl_type(to))
    }
}

struct Function<'a> {
    arguments: &'a [(String, Type)],
    result: Type,
    body: &'a Node,
    /// Scalar arguments that every call passes a seed, so they stay exact unsigned integers.
    seeds: Vec<bool>,
    needed: bool,
}

struct Emitter<'a> {
    program: &'a Program,
    functions: Vec<Function<'a>>,
    field_seed: Vec<Option<bool>>,
    seeds: Vec<String>,
    images: Vec<(String, bool)>,
    features: bool,
    field_state: Vec<u8>,
    field_order: Vec<usize>,
    function_order: Vec<usize>,
    /// Fields baked on the CPU and read from textures (empty unless partitioning).
    cpu_only: Vec<bool>,
}

/// Whether a node uses an operation only the CPU baker has, looking through arguments and helper
/// calls' arguments but not into other fields or helper bodies.
fn uses_cpu_only(node: &Node) -> bool {
    match &node.kind {
        Kind::Builtin(Builtin::Runoff, _) => true,
        Kind::Unary(_, a) | Kind::Member(a, _) => uses_cpu_only(a),
        Kind::Binary(_, a, b) => uses_cpu_only(a) || uses_cpu_only(b),
        Kind::Select(a, b, c) => uses_cpu_only(a) || uses_cpu_only(b) || uses_cpu_only(c),
        Kind::Builtin(_, args) | Kind::Function(_, args) => args.iter().any(uses_cpu_only),
        _ => false,
    }
}

/// Marks every field a node reaches, through other fields and helper bodies. A CPU field is
/// reached but not entered: the shader reads it from its texture.
fn reach_fields(
    compiled: &Compiled,
    cpu_only: &[bool],
    node: &Node,
    fields: &mut [bool],
    helpers: &mut [bool],
) {
    match &node.kind {
        Kind::Field(index) => {
            if !std::mem::replace(&mut fields[*index], true) && !cpu_only[*index] {
                reach_fields(
                    compiled,
                    cpu_only,
                    &compiled.fields[*index],
                    fields,
                    helpers,
                );
            }
        }
        Kind::Unary(_, a) | Kind::Member(a, _) => {
            reach_fields(compiled, cpu_only, a, fields, helpers)
        }
        Kind::Binary(_, a, b) => {
            reach_fields(compiled, cpu_only, a, fields, helpers);
            reach_fields(compiled, cpu_only, b, fields, helpers);
        }
        Kind::Select(a, b, c) => {
            for part in [a, b, c] {
                reach_fields(compiled, cpu_only, part, fields, helpers);
            }
        }
        Kind::Builtin(_, args) => {
            for argument in args {
                reach_fields(compiled, cpu_only, argument, fields, helpers);
            }
        }
        Kind::Function(index, args) => {
            for argument in args {
                reach_fields(compiled, cpu_only, argument, fields, helpers);
            }
            if !std::mem::replace(&mut helpers[*index], true) {
                reach_fields(
                    compiled,
                    cpu_only,
                    &compiled.functions[*index].body,
                    fields,
                    helpers,
                );
            }
        }
        _ => {}
    }
}

impl<'a> Emitter<'a> {
    fn field_type(&self, index: usize) -> Type {
        self.program.metadata.fields[index].kind
    }

    fn kind(&self, node: &Node, scope: Option<usize>) -> Type {
        match &node.kind {
            Kind::Number(_) => Type::Scalar,
            Kind::Bool(_) => Type::Bool,
            Kind::Text(_) => Type::String,
            Kind::Input(input) => match input {
                Input::Uv | Input::Surface => Type::Vec2,
                Input::Footprint | Input::UvFootprint => Type::Scalar,
                _ => Type::Vec3,
            },
            Kind::Parameter(index) => self.program.metadata.parameters[*index].kind,
            Kind::Field(index) => self.field_type(*index),
            Kind::Local(index) => self.functions[scope.expect("a helper body")].arguments[*index].1,
            Kind::Unary(Op::Not, _) => Type::Bool,
            Kind::Unary(_, value) => self.kind(value, scope),
            Kind::Binary(op, a, b) => match op {
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Rem => {
                    wider(self.kind(a, scope), self.kind(b, scope))
                }
                _ => Type::Bool,
            },
            Kind::Member(_, components) => {
                [Type::Scalar, Type::Vec2, Type::Vec3][components.len() - 1]
            }
            Kind::Select(_, yes, _) => self.kind(yes, scope),
            Kind::Function(index, _) => self.functions[*index].result,
            Kind::Builtin(name, args) => {
                use Builtin as B;
                let of = |index: usize| self.kind(&args[index], scope);
                match name {
                    B::Vec2 | B::Rotate => Type::Vec2,
                    B::Vec3
                    | B::Cross
                    | B::CreaseNormal
                    | B::CreaseTangent
                    | B::CreaseAdjacentNormal
                    | B::AdjustNormal
                    | B::TriplanarWeights
                    | B::VariedColor
                    | B::SampleColor
                    | B::SampleData => Type::Vec3,
                    B::Mix | B::Min | B::Max | B::Step => wider(of(0), of(1)),
                    B::Smoothstep => wider(wider(of(0), of(1)), of(2)),
                    B::Remap => of(1),
                    B::Clamp
                    | B::Abs
                    | B::Floor
                    | B::Fract
                    | B::Sin
                    | B::Cos
                    | B::Sqrt
                    | B::Normalize
                    | B::Pow
                    | B::Exp
                    | B::Repeat
                    | B::Scale
                    | B::Offset
                    | B::Warp
                    | B::SoftThreshold => of(0),
                    _ => Type::Scalar,
                }
            }
        }
    }

    /// Whether a node is a named seed carried as an exact unsigned integer.
    fn seedlike(&mut self, node: &Node, scope: Option<usize>) -> bool {
        match &node.kind {
            Kind::Builtin(Builtin::Seed, _) => true,
            Kind::Local(index) => self.functions[scope.expect("a helper body")].seeds[*index],
            Kind::Field(index) => self.field_is_seed(*index),
            _ => false,
        }
    }

    fn field_is_seed(&mut self, index: usize) -> bool {
        if let Some(known) = self.field_seed[index] {
            return known;
        }
        // Fields cannot refer to themselves, so this terminates.
        self.field_seed[index] = Some(false);
        let body = &self.program.compiled.fields[index];
        let known = self.seedlike(body, None);
        self.field_seed[index] = Some(known);
        known
    }

    /// Clears the seed mark of every helper argument some call passes an ordinary number.
    fn settle_seeds(&mut self) {
        let compiled = &self.program.compiled;
        loop {
            let mut changed = false;
            let mut sites: Vec<(&Node, Option<usize>)> =
                compiled.fields.iter().map(|n| (n, None)).collect();
            sites.extend(compiled.outputs.iter().map(|(_, n)| (n, None)));
            for index in 0..self.functions.len() {
                sites.push((self.functions[index].body, Some(index)));
            }
            while let Some((node, scope)) = sites.pop() {
                match &node.kind {
                    Kind::Unary(_, a) | Kind::Member(a, _) => sites.push((a, scope)),
                    Kind::Binary(_, a, b) => sites.extend([(&**a, scope), (&**b, scope)]),
                    Kind::Select(a, b, c) => {
                        sites.extend([(&**a, scope), (&**b, scope), (&**c, scope)])
                    }
                    Kind::Builtin(_, args) => sites.extend(args.iter().map(|a| (a, scope))),
                    Kind::Function(index, args) => {
                        for (position, argument) in args.iter().enumerate() {
                            if self.functions[*index].seeds[position]
                                && !self.seedlike(argument, scope)
                            {
                                self.functions[*index].seeds[position] = false;
                                changed = true;
                            }
                            sites.push((argument, scope));
                        }
                    }
                    _ => {}
                }
            }
            if !changed {
                break;
            }
            // A field's mark may depend on an argument that just lost its own.
            self.field_seed.iter_mut().for_each(|known| *known = None);
        }
    }

    /// Orders the fields and helpers a node needs so each is defined before its first use.
    fn require(&mut self, node: &'a Node) {
        match &node.kind {
            Kind::Field(index) => self.require_field(*index),
            Kind::Unary(_, a) | Kind::Member(a, _) => self.require(a),
            Kind::Binary(_, a, b) => {
                self.require(a);
                self.require(b);
            }
            Kind::Select(a, b, c) => {
                self.require(a);
                self.require(b);
                self.require(c);
            }
            Kind::Builtin(_, args) => args.iter().for_each(|a| self.require(a)),
            Kind::Function(index, args) => {
                args.iter().for_each(|a| self.require(a));
                if !self.functions[*index].needed {
                    self.functions[*index].needed = true;
                    let body = self.functions[*index].body;
                    self.require(body);
                    self.function_order.push(*index);
                }
            }
            _ => {}
        }
    }

    fn require_field(&mut self, index: usize) {
        if self.field_state[index] != 0 {
            return;
        }
        self.field_state[index] = 1;
        let program = self.program;
        // A CPU field is read from its texture: nothing upstream of it is needed here.
        if !self.cpu_only[index] {
            self.require(&program.compiled.fields[index]);
        }
        self.field_order.push(index);
    }

    fn seed_uniform(&mut self, name: &str) -> String {
        let index = self
            .seeds
            .iter()
            .position(|seed| seed == name)
            .unwrap_or_else(|| {
                self.seeds.push(name.to_owned());
                self.seeds.len() - 1
            });
        format!("ms_seed{index}")
    }

    fn image_uniform(&mut self, name: &str, linear: bool) -> String {
        let index = self
            .images
            .iter()
            .position(|image| image.0 == name && image.1 == linear)
            .unwrap_or_else(|| {
                self.images.push((name.to_owned(), linear));
                self.images.len() - 1
            });
        format!("ms_image{index}")
    }

    fn text(node: &Node) -> &str {
        match &node.kind {
            Kind::Text(text) => text,
            _ => "",
        }
    }

    /// A node as an unsigned 32-bit seed, as the lattice hash reduces it.
    fn seed(&mut self, node: &Node, scope: Option<usize>) -> String {
        match &node.kind {
            Kind::Builtin(Builtin::Seed, args) => self.seed_uniform(Self::text(&args[0])),
            Kind::Local(index) if self.seedlike(node, scope) => format!("a{index}"),
            Kind::Field(index) if self.seedlike(node, scope) => format!("ms_f{index}"),
            Kind::Number(value) => format!("{}u", *value as i64 as u32),
            // Sums stay in integer arithmetic, which agrees with the baker modulo 2^32.
            Kind::Binary(op @ (Op::Add | Op::Sub), a, b) => {
                let sign = if *op == Op::Add { "+" } else { "-" };
                format!("({} {sign} {})", self.seed(a, scope), self.seed(b, scope))
            }
            _ => format!("ms_seed_of({})", self.emit(node, scope)),
        }
    }

    fn emit(&mut self, node: &Node, scope: Option<usize>) -> String {
        match &node.kind {
            Kind::Number(value) => literal(*value),
            Kind::Bool(value) => value.to_string(),
            Kind::Text(_) => "0".into(),
            Kind::Input(input) => match input {
                Input::Uv => "ms_uv",
                Input::Surface => "ms_surface",
                Input::Object => "ms_object",
                Input::World => "ms_world",
                Input::Normal => "ms_normal",
                Input::Gravity => "ms_gravity",
                Input::Footprint => "ms_footprint",
                Input::UvFootprint => "ms_uv_footprint",
                Input::BoundsMin => "ms_bounds_min",
                Input::BoundsMax => "ms_bounds_max",
            }
            .into(),
            Kind::Parameter(index) => format!("ms_p{index}"),
            Kind::Field(index) if self.seedlike(node, scope) => format!("float(ms_f{index})"),
            Kind::Field(index) => format!("ms_f{index}"),
            Kind::Local(index) if self.seedlike(node, scope) => format!("float(a{index})"),
            Kind::Local(index) => format!("a{index}"),
            Kind::Unary(op, value) => {
                let value = self.emit(value, scope);
                match op {
                    Op::Not => format!("(!{value})"),
                    Op::Sub => format!("(-{value})"),
                    _ => value,
                }
            }
            Kind::Binary(op, a, b) => {
                let (ta, tb) = (self.kind(a, scope), self.kind(b, scope));
                let (x, y) = (self.emit(a, scope), self.emit(b, scope));
                let both = wider(ta, tb);
                match op {
                    Op::Add => format!("({x} + {y})"),
                    Op::Sub => format!("({x} - {y})"),
                    Op::Mul => format!("({x} * {y})"),
                    Op::Div => format!("({x} / {y})"),
                    Op::Rem => format!(
                        "ms_rem({}, {})",
                        promote(&x, ta, both),
                        promote(&y, tb, both)
                    ),
                    Op::And => format!("({x} && {y})"),
                    Op::Or => format!("({x} || {y})"),
                    Op::Equal | Op::NotEqual => {
                        let equal = if matches!(ta, Type::Vec2 | Type::Vec3) {
                            format!("all(equal({x}, {y}))")
                        } else {
                            format!("({x} == {y})")
                        };
                        if *op == Op::Equal {
                            equal
                        } else {
                            format!("(!{equal})")
                        }
                    }
                    Op::Less => format!("({x} < {y})"),
                    Op::Greater => format!("({x} > {y})"),
                    Op::LessEqual => format!("({x} <= {y})"),
                    Op::GreaterEqual => format!("({x} >= {y})"),
                    Op::Not => x,
                }
            }
            Kind::Member(value, components) => {
                let swizzle: String = components.iter().map(|c| ['x', 'y', 'z'][*c]).collect();
                format!("{}.{swizzle}", self.emit(value, scope))
            }
            Kind::Select(condition, yes, no) => format!(
                "({} ? {} : {})",
                self.emit(condition, scope),
                self.emit(yes, scope),
                self.emit(no, scope)
            ),
            Kind::Function(index, args) => {
                let values: Vec<String> = args
                    .iter()
                    .enumerate()
                    .map(|(position, argument)| {
                        if self.functions[*index].seeds[position] {
                            self.seed(argument, scope)
                        } else {
                            self.emit(argument, scope)
                        }
                    })
                    .collect();
                format!("ms_h{index}({})", values.join(", "))
            }
            Kind::Builtin(name, args) => self.builtin(*name, args, node, scope),
        }
    }

    fn builtin(
        &mut self,
        name: Builtin,
        args: &[Node],
        node: &Node,
        scope: Option<usize>,
    ) -> String {
        use Builtin as B;
        let result = self.kind(node, scope);
        let types: Vec<Type> = args.iter().map(|a| self.kind(a, scope)).collect();
        // Strings and seeds are read where an operation names them, never as numbers.
        let a: Vec<String> = args
            .iter()
            .zip(&types)
            .map(|(a, kind)| {
                if *kind == Type::String {
                    String::new()
                } else {
                    self.emit(a, scope)
                }
            })
            .collect();
        let raised = |index: usize| promote(&a[index], types[index], result);
        let selector = |node: &Node| match Self::text(node) {
            "convex" => 0,
            "concave" => 1,
            "boundary" => 2,
            "exposed" => 3,
            "any" => 4,
            "seam" => 5,
            "contact" => 6,
            _ => 7,
        };
        match name {
            B::Vec2 | B::Vec3 if args.len() == 1 => raised(0),
            B::Vec2 | B::Vec3 => format!("{}({})", glsl_type(result), a.join(", ")),
            B::Mix => format!("mix({}, {}, {})", raised(0), raised(1), a[2]),
            B::Clamp if types[1] == Type::Scalar && types[2] == Type::Scalar => {
                format!("clamp({}, {}, {})", a[0], a[1], a[2])
            }
            B::Clamp => format!("clamp({}, {}, {})", a[0], raised(1), raised(2)),
            B::Smoothstep if types[0] == Type::Scalar && types[1] == Type::Scalar => {
                format!("smoothstep({}, {}, {})", a[0], a[1], a[2])
            }
            B::Smoothstep => format!("smoothstep({}, {}, {})", raised(0), raised(1), raised(2)),
            B::Abs => format!("abs({})", a[0]),
            B::Floor => format!("floor({})", a[0]),
            B::Fract => format!("fract({})", a[0]),
            B::Sin => format!("sin({})", a[0]),
            B::Cos => format!("cos({})", a[0]),
            B::Sqrt => format!("sqrt({})", a[0]),
            B::Min => format!("min({}, {})", raised(0), raised(1)),
            B::Max => format!("max({}, {})", raised(0), raised(1)),
            B::Step => format!("step({}, {})", raised(0), raised(1)),
            B::ValueNoise => format!("ms_value_noise({}, {})", a[0], self.seed(&args[1], scope)),
            B::Hash => format!("ms_hash({}, {})", a[0], self.seed(&args[1], scope)),
            B::Dot => format!("dot({}, {})", a[0], a[1]),
            B::Cross => format!("cross({}, {})", a[0], a[1]),
            B::Length => format!("length({})", a[0]),
            B::Normalize => format!("normalize({})", a[0]),
            B::Pow => format!(
                "pow(max({}, 0.0), {})",
                a[0],
                promote(&a[1], Type::Scalar, result)
            ),
            B::Exp => format!("exp({})", a[0]),
            B::Seed => format!("float({})", self.seed(node, scope)),
            B::Noise | B::Fbm | B::Cellular => format!(
                "ms_noise({}, {}, {}, {})",
                a[0],
                self.seed(&args[1], scope),
                if name == B::Fbm { a[2].as_str() } else { "1.0" },
                name == B::Cellular
            ),
            B::Scale => format!("({} * {})", a[0], a[1]),
            B::Offset | B::Warp => format!("({} + {})", a[0], a[1]),
            B::Repeat => format!("ms_repeat({}, {})", a[0], raised(1)),
            B::Rotate => format!("ms_rotate({}, {})", a[0], a[1]),
            B::SampleColor => format!(
                "texture({}, {}).rgb",
                self.image_uniform(Self::text(&args[0]), false),
                a[1]
            ),
            B::SampleData => format!(
                "texture({}, {}).rgb",
                self.image_uniform(Self::text(&args[0]), true),
                a[1]
            ),
            B::SampleAlpha => format!(
                "texture({}, {}).a",
                self.image_uniform(Self::text(&args[0]), true),
                a[1]
            ),
            // The preview reads no source edges.
            B::Runoff => "0.0".into(),
            B::CreaseDistance | B::CreaseNormal | B::CreaseTangent | B::CreaseAdjacentNormal => {
                self.features = true;
                let query = match name {
                    B::CreaseDistance => "distance",
                    B::CreaseNormal => "normal",
                    B::CreaseTangent => "tangent",
                    _ => "adjacent",
                };
                format!(
                    "ms_crease_{query}({}, {}, {})",
                    selector(&args[0]),
                    a[1],
                    a[2]
                )
            }
            B::AdjustNormal => format!("ms_adjust_normal({}, {}, {})", a[0], a[1], a[2]),
            B::TriplanarWeights => format!("ms_triplanar({}, {})", a[0], a[1]),
            B::VariedColor => format!(
                "ms_varied({}, {}, {}, {}, {})",
                self.image_uniform(Self::text(&args[0]), false),
                a[1],
                self.seed(&args[2], scope),
                a[3],
                a[4]
            ),
            B::Remap => format!(
                "ms_remap({}, {}, {}, {}, {}, {})",
                a[1],
                a[2],
                a[3],
                a[4],
                a[5],
                Self::text(&args[0]) == "clamp"
            ),
            B::SoftThreshold => format!("ms_soft_threshold({}, {}, {})", a[0], a[1], a[2]),
            B::DistanceBand => format!("ms_distance_band({}, {}, {})", a[0], a[1], a[2]),
            B::SoftUnion => format!("ms_mask(0, {}, {})", a[0], a[1]),
            B::MaskIntersect => format!("ms_mask(1, {}, {})", a[0], a[1]),
            B::MaskSubtract => format!("ms_mask(2, {}, {})", a[0], a[1]),
            B::Unknown => "0.0".into(),
        }
    }
}

/// Helpers that take the program's own types.
const HELPERS: &str = "
float ms_repeat(float v, float p) { return v - p * floor(v / p); }
vec2 ms_repeat(vec2 v, vec2 p) { return v - p * floor(v / p); }
vec3 ms_repeat(vec3 v, vec3 p) { return v - p * floor(v / p); }
float ms_remap(float v, float a, float b, float c, float d, bool clamped) { float t = (v - a) / (b - a); return c + (clamped ? clamp(t, 0.0, 1.0) : t) * (d - c); }
vec2 ms_remap(vec2 v, float a, float b, float c, float d, bool clamped) { vec2 t = (v - a) / (b - a); return c + (clamped ? clamp(t, 0.0, 1.0) : t) * (d - c); }
vec3 ms_remap(vec3 v, float a, float b, float c, float d, bool clamped) { vec3 t = (v - a) / (b - a); return c + (clamped ? clamp(t, 0.0, 1.0) : t) * (d - c); }
float ms_mask(int mode, float x, float y) {
  x = clamp(x, 0.0, 1.0); y = clamp(y, 0.0, 1.0);
  return mode == 0 ? 1.0 - (1.0 - x) * (1.0 - y) : mode == 1 ? x * y : x * (1.0 - y);
}
";

/// The program as one shader: `runoff` reads no edges (it is zero), as a preview approximates it.
pub(super) fn shader(program: &Program) -> Result<Shader, String> {
    build(program, false)
}

/// The program split between the GPU and the CPU: fields that use CPU-only operations become
/// textures the host bakes, and the shader holds everything else. Data flows from the CPU to the
/// GPU only, so everything upstream of a CPU field is evaluated by the CPU with it (and absent
/// from the shader unless something else needs it), and everything downstream stays on the GPU.
/// Refuses what that rule cannot place, naming why: a CPU-only operation inside a helper function
/// or directly inside an output (it has no field to bake), or one in a seed.
pub(super) fn plan(program: &Program) -> Result<Shader, String> {
    build(program, true)
}

fn build(program: &Program, partition: bool) -> Result<Shader, String> {
    let compiled = &program.compiled;
    let definitions: Vec<&Definition> = program
        .definitions
        .values()
        .filter(|definition| matches!(definition, Definition::Function { .. }))
        .collect();
    let functions = definitions
        .iter()
        .zip(&compiled.functions)
        .map(|(definition, function)| {
            let Definition::Function {
                arguments, result, ..
            } = definition
            else {
                unreachable!()
            };
            Function {
                arguments,
                result: *result,
                body: &function.body,
                seeds: arguments
                    .iter()
                    .map(|(_, kind)| *kind == Type::Scalar)
                    .collect(),
                needed: false,
            }
        })
        .collect();
    let mut emitter = Emitter {
        program,
        functions,
        field_seed: vec![None; compiled.fields.len()],
        seeds: Vec::new(),
        images: Vec::new(),
        features: false,
        field_state: vec![0; compiled.fields.len()],
        field_order: Vec::new(),
        function_order: Vec::new(),
        cpu_only: compiled
            .fields
            .iter()
            .map(|field| partition && uses_cpu_only(field))
            .collect(),
    };
    if partition {
        for (name, node) in &compiled.outputs {
            if uses_cpu_only(node) {
                return Err(format!(
                    "The {name} output uses runoff directly; name it with a field so the CPU can bake it."
                ));
            }
        }
    }
    emitter.settle_seeds();
    let field_index = |name: &str| {
        program
            .metadata
            .fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| format!("Unknown field {name}."))
    };
    for (_, node) in &compiled.outputs {
        emitter.require(node);
    }
    let mut live = Vec::new();
    for operation in &program.metadata.live {
        let mut arguments = BTreeMap::new();
        for (argument, field) in &operation.inputs {
            let index = field_index(field)?;
            emitter.require_field(index);
            let symbol = match (emitter.field_type(index), emitter.field_is_seed(index)) {
                (_, true) | (Type::Bool, _) => format!("float(ms_f{index})"),
                (Type::Scalar, _) => format!("ms_f{index}"),
                _ => format!("ms_f{index}.x"),
            };
            arguments.insert(argument.clone(), symbol);
        }
        live.push(LiveOperation {
            id: operation.id.clone(),
            arguments,
        });
    }

    if partition {
        for index in emitter.function_order.clone() {
            if uses_cpu_only(emitter.functions[index].body) {
                return Err("A helper function that the GPU part needs uses runoff, which cannot be partitioned.".into());
            }
        }
    }
    let mut body = String::new();
    let mut declarations = String::new();
    let mut cpu_fields: Vec<CpuField> = Vec::new();
    let mut field_lines: Vec<(usize, String)> = Vec::new();
    let table = partition.then(|| program.dependency_table());
    for index in emitter.field_order.clone() {
        let kind = if emitter.field_is_seed(index) {
            "uint"
        } else {
            glsl_type(emitter.field_type(index))
        };
        declarations.push_str(&format!("{kind} ms_f{index};\n"));
        let node = &compiled.fields[index];
        if emitter.cpu_only[index] {
            let field = &program.metadata.fields[index];
            if field.kind == Type::String || emitter.field_is_seed(index) {
                return Err(format!(
                    "The field {} cannot be baked as a texture.",
                    field.name
                ));
            }
            let uniform = format!("ms_cpu{}", cpu_fields.len());
            let fetch = format!("texelFetch({uniform}, ms_texel, 0)");
            let value = match field.kind {
                Type::Scalar => format!("{fetch}.x"),
                Type::Vec2 => format!("{fetch}.xy"),
                Type::Vec3 => format!("{fetch}.xyz"),
                _ => format!("({fetch}.x > 0.5)"),
            };
            let line = format!("  ms_f{index} = {value};\n");
            body.push_str(&line);
            field_lines.push((index, line));
            let found = table
                .as_ref()
                .and_then(|table| table.fields.get(&field.name))
                .cloned()
                .unwrap_or_default();
            cpu_fields.push(CpuField {
                name: field.name.clone(),
                uniform,
                kind: field.kind,
                parameters: found
                    .parameters
                    .iter()
                    .map(|i| program.metadata.parameters[*i].name.clone())
                    .collect(),
                seed: found.seed,
                images: found.images.iter().cloned().collect(),
                edges: found.edges,
            });
            continue;
        }
        let value = if emitter.field_is_seed(index) {
            emitter.seed(node, None)
        } else {
            emitter.emit(node, None)
        };
        let line = format!("  ms_f{index} = {value};\n");
        body.push_str(&line);
        field_lines.push((index, line));
    }
    let mut outputs = Vec::new();
    let mut height: Option<(&Node, String)> = None;
    for (name, node) in &compiled.outputs {
        let value = emitter.emit(node, None);
        if name == "height" {
            height = Some((node, value.clone()));
        }
        body.push_str(&format!("  ms_{name} = {value};\n"));
        outputs.push(name.clone());
    }
    let mut helpers = String::new();
    let mut prototypes = String::new();
    for index in emitter.function_order.clone() {
        let signature = {
            let function = &emitter.functions[index];
            let arguments: Vec<String> = function
                .arguments
                .iter()
                .enumerate()
                .map(|(position, (_, kind))| {
                    let kind = if function.seeds[position] {
                        "uint"
                    } else {
                        glsl_type(*kind)
                    };
                    format!("{kind} a{position}")
                })
                .collect();
            format!(
                "{} ms_h{index}({})",
                glsl_type(function.result),
                arguments.join(", ")
            )
        };
        let node = emitter.functions[index].body;
        let value = emitter.emit(node, Some(index));
        prototypes.push_str(&format!("{signature};\n"));
        helpers.push_str(&format!("{signature} {{ return {value}; }}\n"));
    }

    let mut source = String::new();
    let mut parameters = Vec::new();
    for (index, parameter) in program.metadata.parameters.iter().enumerate() {
        let uniform = format!("ms_p{index}");
        source.push_str(&format!(
            "uniform {} {uniform};\n",
            glsl_type(parameter.kind)
        ));
        parameters.push(Uniform {
            name: parameter.name.clone(),
            uniform,
            kind: parameter.kind,
        });
    }
    let seeds: Vec<Uniform> = emitter
        .seeds
        .iter()
        .enumerate()
        .map(|(index, name)| Uniform {
            name: name.clone(),
            uniform: format!("ms_seed{index}"),
            kind: Type::Scalar,
        })
        .collect();
    for seed in &seeds {
        source.push_str(&format!("uniform uint {};\n", seed.uniform));
    }
    let images: Vec<ImageUniform> = emitter
        .images
        .iter()
        .enumerate()
        .map(|(index, (name, linear))| ImageUniform {
            name: name.clone(),
            uniform: format!("ms_image{index}"),
            linear: *linear,
        })
        .collect();
    for image in &images {
        source.push_str(&format!("uniform sampler2D {};\n", image.uniform));
    }
    for field in &cpu_fields {
        source.push_str(&format!("uniform highp sampler2D {};\n", field.uniform));
    }
    source.push_str(RUNTIME);
    source.push_str(HELPERS);
    source.push_str(
        "vec3 ms_base_color; float ms_roughness; float ms_metalness; float ms_height;\n\
         vec3 ms_emissive; float ms_opacity; vec3 ms_receiver_normal; vec3 ms_normal_offset;\n",
    );
    source.push_str(&declarations);
    source.push_str(&prototypes);
    source.push_str(&helpers);
    source.push_str(
        "void ms_evaluate() {\n  ms_base_color = vec3(0.0); ms_roughness = 1.0; ms_metalness = 0.0; ms_height = 0.0;\n  \
         ms_emissive = vec3(0.0); ms_opacity = 1.0; ms_receiver_normal = ms_normal; ms_normal_offset = vec3(0.0);\n",
    );
    source.push_str(&body);
    source.push_str("}\n");
    // The height alone: only the fields the height output reaches, in the same order. A renderer
    // that shades per pixel calls it at neighboring surface points to take the relief's slope by
    // finite differences, which screen derivatives of `ms_height` give at half resolution. It
    // writes the same field globals as `ms_evaluate`, so hosts call it before `ms_evaluate`.
    if let Some((node, value)) = &height {
        let mut reached = vec![false; compiled.fields.len()];
        let mut helpers_seen = vec![false; compiled.functions.len()];
        reach_fields(
            compiled,
            &emitter.cpu_only,
            node,
            &mut reached,
            &mut helpers_seen,
        );
        source.push_str("void ms_evaluate_height() {\n  ms_height = 0.0;\n");
        for (index, line) in &field_lines {
            if reached[*index] {
                source.push_str(line);
            }
        }
        source.push_str(&format!("  ms_height = {value};\n}}\n"));
    }
    let cpu_outputs: Vec<String> = table
        .as_ref()
        .filter(|_| !cpu_fields.is_empty())
        .map(|table| {
            outputs
                .iter()
                .filter(|name| table.outputs.get(*name).is_some_and(|found| found.edges))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Ok(Shader {
        source,
        parameters,
        seeds,
        images,
        outputs,
        live,
        features: emitter.features,
        cpu_fields,
        cpu_outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{Sources, compile};

    fn examples(entry: &str) -> super::Shader {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/materialspec/program/examples");
        let modules = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(|file| {
                let path = file.unwrap().path();
                let name = path.file_name()?.to_str()?.to_owned();
                (name.ends_with(".material.ts") || name.ends_with(".ms"))
                    .then(|| (name, std::fs::read_to_string(&path).unwrap()))
            })
            .collect();
        let program = compile(&Sources {
            entry: entry.into(),
            modules,
        })
        .unwrap_or_else(|errors| panic!("{errors:?}"));
        program.shader().unwrap()
    }

    fn plan_of(source: &str) -> Result<super::Shader, String> {
        compile(&Sources {
            entry: "main.ms".into(),
            modules: [("main.ms".to_owned(), source.to_owned())].into(),
        })
        .unwrap_or_else(|errors| panic!("{errors:?}"))
        .shader_plan()
    }

    /// The chipped concrete's only CPU-only node is `runoff`; the plan bakes that one field on the
    /// CPU and leaves the rest, including everything that reads it, in the shader.
    #[test]
    fn chipped_concrete_plan_places_only_runoff_on_the_cpu() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/materialspec/program/examples");
        let modules = ["chipped-concrete.ms", "common.ms"]
            .map(|name| {
                (
                    name.to_owned(),
                    std::fs::read_to_string(directory.join(name)).unwrap(),
                )
            })
            .into();
        let program = compile(&Sources {
            entry: "chipped-concrete.ms".into(),
            modules,
        })
        .unwrap();
        let plan = program.shader_plan().unwrap();
        let names: Vec<_> = plan
            .cpu_fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        assert_eq!(names, ["runoff_mask"]);
        let field = &plan.cpu_fields[0];
        assert!(
            field.edges && field.parameters.is_empty() && !field.seed,
            "runoff reads edges, not parameters or seeds"
        );
        assert!(plan.source.contains("uniform highp sampler2D ms_cpu0;"));
        assert!(plan.source.contains("texelFetch(ms_cpu0, ms_texel, 0).x"));
        // The downstream color math stays on the GPU, still reading its parameters and seeds.
        assert!(plan.parameters.iter().any(|p| p.name == "tint") && !plan.seeds.is_empty());
        assert!(plan.outputs.contains(&"base_color".to_owned()));
        // Only the color rests on the CPU field; roughness, metalness and height are the GPU's alone.
        assert_eq!(plan.cpu_outputs, ["base_color"]);
        // The unpartitioned shader approximates runoff as zero, as scene previews always have.
        assert!(program.shader().unwrap().cpu_fields.is_empty());
    }

    /// Data flows from the CPU to the GPU only: what only a CPU field reads is evaluated with it
    /// and never appears in the shader; what the shader also reads stays in both.
    #[test]
    fn plan_closes_over_everything_upstream_of_a_cpu_field() {
        let plan = plan_of(
            "material 1;
             param reach: scalar = 0.5;
             param shade: scalar = 0.5;
             let upstream = noise(surface, seed(\"only-cpu\")) * reach;
             let shared = noise(surface * 3.0, seed(\"both\"));
             let flow = runoff(0.1, 0.5) * upstream * shared;
             let downstream = flow * shade + shared * 0.1;
             output base_color = vec3(downstream);
             output roughness = shared;
             output metalness = 0;",
        )
        .unwrap();
        let field = &plan.cpu_fields[0];
        assert_eq!(field.name, "flow");
        // The CPU field carries all it reads, upstream included: its seeds and parameters.
        assert!(field.edges && field.seed);
        assert_eq!(field.parameters, ["reach"]);
        // The shader has neither the upstream-only seed nor the upstream-only parameter's field.
        let seeds: Vec<_> = plan.seeds.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            seeds,
            ["both"],
            "the upstream-only noise is not in the shader"
        );
        assert!(plan.source.contains("ms_cpu0")); // Downstream of the CPU field the math stays on the GPU.
        assert!(plan.parameters.iter().any(|p| p.name == "shade"));
    }

    /// A program with a height output also gets a height-only entry point that computes just the
    /// fields the height reaches; a program without one gets none.
    #[test]
    fn height_entry_point_computes_only_what_the_height_reaches() {
        let source = |height: &str| {
            format!(
                "material 1;
                 fn lifted(x: scalar) -> scalar = x * relief;
                 let tone = noise(surface * 3.0, seed(\"tone\"));
                 let relief = noise(surface * 9.0, seed(\"relief\"));
                 let bump = lifted(0.5) + 0.1;
                 output base_color = vec3(tone);
                 output roughness = 0.5;
                 output metalness = 0;
                 {height}"
            )
        };
        let shader = plan_of(&source("output height = bump * 0.01;")).unwrap();
        let entry = shader
            .source
            .split("void ms_evaluate_height() {")
            .nth(1)
            .expect("a height entry point");
        // `bump` and, through the helper's body, `relief`; not `tone`, which only the color reads.
        assert_eq!(entry.matches("  ms_f").count(), 2, "{entry}");
        assert!(entry.contains("ms_height = "));
        let whole = shader
            .source
            .split("void ms_evaluate() {")
            .nth(1)
            .unwrap()
            .split("void ms_evaluate_height")
            .next()
            .unwrap();
        assert_eq!(whole.matches("  ms_f").count(), 3);
        assert!(
            !plan_of(&source(""))
                .unwrap()
                .source
                .contains("ms_evaluate_height")
        );
    }

    /// What the rule cannot place is refused with a reason, not approximated.
    #[test]
    fn plan_refuses_runoff_it_cannot_partition() {
        let in_output = plan_of(
            "material 1; output base_color = vec3(runoff(0.1, 0.5)); output roughness = 1; output metalness = 0;",
        );
        assert!(
            in_output
                .unwrap_err()
                .contains("output uses runoff directly")
        );
        let in_helper = plan_of(
            "material 1;
             fn wet(x: scalar) -> scalar = x * runoff(0.1, 0.5);
             let value = wet(0.5);
             output base_color = vec3(value);
             output roughness = 1;
             output metalness = 0;",
        );
        // A helper's body is not a field the CPU could bake, so a CPU-only operation inside one is refused.
        assert!(in_helper.unwrap_err().contains("helper function"));
    }

    /// Every shipped recipe emits a shader whose seeds stay integers and whose uniforms are listed.
    #[test]
    fn shipped_recipes_emit_preview_shaders() {
        for entry in [
            "chipped-concrete.ms",
            "painted-metal.material.ts",
            "tiled-courtyard.material.ts",
            "kowloon-concrete.material.ts",
            "kowloon-rusted-steel.material.ts",
        ] {
            let shader = examples(entry);
            assert!(shader.source.contains("void ms_evaluate()"), "{entry}");
            assert!(shader.outputs.contains(&"base_color".to_owned()), "{entry}");
            assert_eq!(
                shader.source.matches('{').count(),
                shader.source.matches('}').count(),
                "{entry}: balanced braces"
            );
            assert_eq!(
                shader.source.matches('(').count(),
                shader.source.matches(')').count(),
                "{entry}: balanced parentheses"
            );
            for uniform in shader
                .parameters
                .iter()
                .map(|p| &p.uniform)
                .chain(shader.seeds.iter().map(|s| &s.uniform))
            {
                assert!(
                    shader.source.contains(&format!(" {uniform};")),
                    "{entry}: {uniform}"
                );
            }
        }
        let concrete = examples("kowloon-concrete.material.ts");
        // Helper keys are seeds at every call, so they never pass through a float.
        // (The one match is the runtime's own definition.)
        assert_eq!(
            concrete.source.matches("ms_seed_of(").count(),
            1,
            "seeds stay exact"
        );
        if let Ok(path) = std::env::var("GLSL_DUMP") {
            std::fs::write(path, &concrete.source).unwrap();
        }
        assert!(concrete.features && concrete.images.len() >= 2);
        let steel = examples("kowloon-rusted-steel.material.ts");
        assert_eq!(steel.live.len(), 7);
        assert!(steel.live.iter().all(|operation| {
            operation
                .arguments
                .values()
                .all(|symbol| symbol.contains("ms_f"))
        }));
    }
}
