//! A GLSL preview of a checked program.
//!
//! The baker evaluates a program per texel on the CPU; a renderer can instead evaluate the same
//! program per pixel, so an edit shows without a bake. This emits GLSL ES 3.00: one global per
//! field that the outputs reach and `ms_evaluate()`, which sets the fields in order (helpers are
//! expanded where they are called) and fills the channel globals. Parameters and named seeds are
//! uniforms, so changing them needs no new shader.
//!
//! The preview is close to a bake, not identical. Lattice hashing uses the same 32-bit integer
//! arithmetic, but values are 32-bit floats, filtering follows screen derivatives where the baker
//! propagates a footprint, both branches of a `select` are computed, and `runoff` reads no edges.
//! Bakes stay the authority for exported maps.
//!
//! The shape of the source follows what compiles fast. Direct3D's compiler (behind WebGL on
//! Windows) inlines every call, and its time grows with about the square of the code it ends up
//! with: a hundred inlined lattice noises took two seconds, the same hundred evaluated by one loop
//! over arrays of their arguments a fraction of that. So helpers are expanded here (the compiler
//! would anyway), every lattice noise becomes a slot of an array, and each entry point is a
//! sequence of rounds: straight-line statements that set fields and the slots' arguments, then one
//! loop per kind of noise that evaluates the slots those statements filled. A slot whose argument
//! reads another slot's result waits for a later round. The loops' bounds add `ms_loop`, a uniform
//! no host sets (so it is zero), which keeps a compiler from unrolling them back into copies.
//! Loops draw slower than the same noises inline, so the source holds that form too, behind
//! `MS_INLINE_NOISE` (see `build`). Measure a change to any of this with
//! `tests/shader-compile-bench.mjs` on a real GPU: what the compiler is slow at is not obvious.
use super::{
    Program, Type,
    compiled::{Builtin, Input, Kind, Node, Op},
    syntax::Definition,
};
use serde::Serialize;
use std::collections::BTreeMap;

const RUNTIME: &str = include_str!("glsl_runtime.glsl");

/// A program as fragment shader source, with what the host must bind.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shader {
    /// Uniform declarations, the runtime and `void ms_evaluate()`, in two forms: with noises in
    /// loops, and, where the host defines `MS_INLINE_NOISE` before the source, with each noise
    /// where it is used. Before calling it the host sets `ms_world`, `ms_object`, `ms_normal`, `ms_uv`,
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
    /// The feature names the program queries (`feature_distance`, `feature_tangent`), in the
    /// order of their kinds: a host puts the segments of `named_features[k]` in `ms_features`
    /// with kind `5 + k`, an angle of zero and no normal.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub named_features: Vec<String>,
    /// Fields the shader reads from textures because they use CPU-only operations (`runoff`), set
    /// only by [`Program::shader_plan`]. Everything they read upstream stays out of the shader;
    /// everything that reads them downstream stays in it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu_fields: Vec<CpuField>,
    /// Outputs that read a CPU field (through the shader's own math), so a host can say which channels of a preview rest on CPU data.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cpu_outputs: Vec<String>,
    /// An identity of `source`: equal sources have equal identities, so a host can key compiled
    /// programs by it instead of hashing the text.
    pub id: String,
    /// Every uniform `source` declares, in order, as GLSL declares it.
    pub uniforms: Vec<UniformDeclaration>,
    /// How many of `uniforms` are samplers (each takes a texture unit).
    pub samplers: usize,
    /// Entry points besides `ms_evaluate()`: each computes some of the outputs with only the code
    /// they need, for a pass that writes one texture. One program of `ms_evaluate` compiles in less
    /// time than its parts do together; the parts are for drawing less per pass.
    pub entries: Vec<EntryPoint>,
    /// What each output and each live argument reads, so a host that keeps one texture per
    /// output redraws only those an edit can change.
    pub reads: ShaderReads,
    /// Render-only inputs the source reads, which only a renderer has (`time`, `instance`,
    /// instance and vertex attributes): the uniform `msTime` it declares (`ms_time`), and the
    /// varyings the host must declare and set, `msInstanceValue` (`ms_instance`, a vec4),
    /// `msInstance_<name>` and `msVertex_<name>`. Never baked: a bake of a value that reads one
    /// fails, naming it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub render_inputs: Vec<RenderInput>,
    /// For a painted material, the GLSL expression holding the color that reaches the screen
    /// after `ms_evaluate()`: linear RGB, drawn without the light loop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub painted: Option<String>,
    /// The cutoff of a cutout (`alpha_cutout` as the opacity): the host draws the surface only
    /// where `ms_opacity` reaches it, in every look of the material, baked ones included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f64>,
}

/// See [`Shader::render_inputs`].
#[derive(Clone, Debug, Serialize)]
pub struct RenderInput {
    pub symbol: String,
    /// `uniform` (`msTime`, `msSunDirection`), `instance` (one value per drawn instance, from
    /// the batch: `msInstanceValue`, `msInstance_<name>`), `vertex` (`msVertex_<name>`),
    /// `derived` (varyings the host derives from what it draws: `msInstanceRoot`,
    /// `msInstanceTint`, `msInstanceRandom`, `msPlacedWorld`) or `field` (a scene field's
    /// texture `msField<k>` and its rectangle `msFieldRect<k>`, minimum then maximum corner).
    pub kind: &'static str,
    /// The GLSL type: `float`, `vec2`, `vec3`, `vec4` or `sampler2D`.
    pub glsl: &'static str,
    /// For a field, the scene field it is (`ground.color`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For a named attribute, the GLSL of the value its call is typed by: what the host reads
    /// where the drawn mesh has no such attribute (as the evaluator's default bindings do).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// What the outputs and live arguments of a [`Shader`] read.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ShaderReads {
    /// By output name, for every output the program sets.
    pub outputs: BTreeMap<String, Reads>,
    /// By live operation id, then argument name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub live: BTreeMap<String, BTreeMap<String, Reads>>,
    /// What a painted material's color reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub painted: Option<Reads>,
}

/// Everything one value reads, through the fields and helpers upstream of it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reads {
    /// Parameter uniforms (`Shader::parameters[..].uniform`).
    pub parameters: Vec<String>,
    /// Whether any named seed is read, so the request seed matters.
    pub seed: bool,
    /// Supplied images, by name.
    pub images: Vec<String>,
    /// Whether the supplied source edges are read.
    pub edges: bool,
    /// Whether receiver features are queried, derived or named.
    pub features: bool,
    /// The parts of a sample read: `uv`, `surface`, `object`, `world`, `normal`, `gravity`,
    /// `footprint`, `uv_footprint`, `bounds`. A position brings its footprint along.
    pub sample: Vec<&'static str>,
    /// CPU fields read (`Shader::cpu_fields[..].name`); empty outside a plan.
    pub cpu_fields: Vec<String>,
    /// External inputs read (`external(name, fallback)`), whose planes belong to one receiver.
    pub planes: Vec<String>,
    /// Whether the value is the same at a surface position on every receiver: it reads only
    /// `surface` and `footprint` of the sample, and no features, edges or planes.
    pub intrinsic: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct UniformDeclaration {
    pub name: String,
    /// The GLSL type: `float`, `vec3`, `bool`, `int`, `uint` or `sampler2D`.
    #[serde(rename = "type")]
    pub kind: String,
}

/// A function of the source that evaluates part of the program at the current inputs. It sets the
/// channel globals of `outputs` (to their defaults where the program has no such output) and
/// leaves the other channels alone. Entry points share the field globals, so the last one called
/// decides what a field holds.
#[derive(Clone, Debug, Serialize)]
pub struct EntryPoint {
    /// The GLSL function, called as `name()`.
    pub function: String,
    pub outputs: Vec<String>,
    /// Whether it also sets every field a live renderer operation reads.
    pub live: bool,
    /// The function's definition, to append to the shader's `source` in a program that calls it;
    /// empty when `source` already holds it (`ms_evaluate_height`).
    pub source: String,
}

/// What a live argument is to a host that bakes: a number, an expression of scalar parameters
/// alone (the material's own shader can evaluate it from their uniforms), or a value that varies
/// over the surface and must be baked.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum LiveArgument {
    Constant {
        constant: f64,
    },
    Parameters {
        /// The parameter uniforms `glsl` reads.
        parameters: Vec<String>,
        glsl: String,
    },
    Varying {
        /// The GLSL symbol holding the value after an entry point that sets live fields ran.
        varying: String,
    },
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
    /// External inputs the field reads (`external(name, fallback)`): the field's texture also
    /// follows the planes a host binds to these names.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<String>,
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
    /// Each argument of `arguments`, classified.
    pub classes: BTreeMap<String, LiveArgument>,
}

/// A kind of lattice noise evaluated by a loop over slots rather than where it is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Batch {
    Value2,
    Value3,
    Gradient2,
    Gradient3,
    Cells2,
    Cells3,
}

impl Batch {
    const ALL: [Batch; 6] = [
        Batch::Value2,
        Batch::Value3,
        Batch::Gradient2,
        Batch::Gradient3,
        Batch::Cells2,
        Batch::Cells3,
    ];

    /// The arrays of a kind: `p` positions, `k` seeds, `s` spreads, `r` results and, for octave
    /// noises, `c` octave counts.
    fn prefix(self) -> &'static str {
        match self {
            Batch::Value2 => "ms_v2",
            Batch::Value3 => "ms_v3",
            Batch::Gradient2 => "ms_g2",
            Batch::Gradient3 => "ms_g3",
            Batch::Cells2 => "ms_c2",
            Batch::Cells3 => "ms_c3",
        }
    }

    fn point(self) -> &'static str {
        match self {
            Batch::Value2 | Batch::Gradient2 | Batch::Cells2 => "vec2",
            _ => "vec3",
        }
    }

    fn octaves(self) -> bool {
        !matches!(self, Batch::Value2 | Batch::Value3)
    }

    /// The noise of slot `i`, as the runtime's direct functions (`ms_value_noise`, `ms_noise`)
    /// compute it from the same arguments.
    fn call(self, i: &str) -> String {
        let p = self.prefix();
        match self {
            Batch::Value2 => format!("ms_fade_value(ms_value2({p}p{i}, {p}k{i}), {p}s{i})"),
            Batch::Value3 => format!("ms_fade_value(ms_value3({p}p{i}, {p}k{i}), {p}s{i})"),
            Batch::Gradient2 | Batch::Cells2 => format!(
                "ms_octaves(vec3({p}p{i}, 0.0), 2, {p}k{i}, int({p}c{i}), {}, {p}s{i})",
                self == Batch::Cells2
            ),
            Batch::Gradient3 | Batch::Cells3 => format!(
                "ms_octaves({p}p{i}, 3, {p}k{i}, int({p}c{i}), {}, {p}s{i})",
                self == Batch::Cells3
            ),
        }
    }
}

/// A round's slots of one kind are evaluated by a loop from this many; fewer, line by line.
const LOOP_SLOTS: usize = 3;

/// What a statement reads: an earlier statement's variable or a noise slot's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Statement(usize),
    Slot(usize),
}

/// One line of an entry point: it sets a field, a local or a slot's arguments, once.
struct Statement {
    text: String,
    uses: Vec<Use>,
    /// The round it runs in: one more than the round of the latest slot it reads.
    round: usize,
}

struct Slot {
    batch: Batch,
    /// The statement that sets its arguments; the slot is evaluated at the end of that round.
    statement: usize,
}

/// A helper argument while the helper's body is expanded.
struct Binding {
    text: String,
    uses: Vec<Use>,
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
    /// How often the body reads each argument.
    reads: Vec<usize>,
}

/// Counts a helper body's reads of each of its arguments.
fn count_reads(node: &Node, reads: &mut [usize]) {
    match &node.kind {
        Kind::Local(index) => reads[*index] += 1,
        Kind::Unary(_, a) | Kind::Member(a, _) => count_reads(a, reads),
        Kind::Binary(_, a, b) => {
            count_reads(a, reads);
            count_reads(b, reads);
        }
        Kind::Select(a, b, c) => {
            for part in [a, b, c] {
                count_reads(part, reads);
            }
        }
        Kind::Builtin(_, args) | Kind::Function(_, args) => {
            args.iter().for_each(|a| count_reads(a, reads))
        }
        _ => {}
    }
}

struct Emitter<'a> {
    program: &'a Program,
    functions: Vec<Function<'a>>,
    field_seed: Vec<Option<bool>>,
    seeds: Vec<String>,
    images: Vec<(String, bool)>,
    features: bool,
    /// Stop counts of the ramps emitted (each has a helper), and whether one is display-referred.
    ramps: std::collections::BTreeSet<usize>,
    display_ramp: bool,
    /// Render-only inputs read: GLSL symbol, kind (`uniform`, `instance`, `vertex`, `derived`)
    /// and type.
    render: std::collections::BTreeSet<(String, &'static str, &'static str)>,
    /// Whether `shade_band` is used, which needs its helper and `ms_cast_shadow`.
    shade_band: bool,
    /// Scene fields `sample_field` reads, in the order of their uniforms (`msField<k>`).
    fields: Vec<String>,
    /// Named attributes' fallbacks by symbol (see `RenderInput::fallback`).
    fallbacks: BTreeMap<String, String>,
    field_state: Vec<u8>,
    field_order: Vec<usize>,
    function_order: Vec<usize>,
    /// Fields baked on the CPU and read from textures (empty unless partitioning).
    cpu_only: Vec<bool>,
    /// Whether noises stay where they are used instead of becoming slots.
    inline: bool,
    statements: Vec<Statement>,
    slots: Vec<Slot>,
    /// What the expression being emitted has read so far.
    uses: Vec<Use>,
    /// The arguments of the helpers being expanded, innermost last.
    frames: Vec<Vec<Binding>>,
    /// The statement that sets each field.
    field_statement: Vec<usize>,
    /// Locals by declaration (`type expression`) and slots by kind and arguments, so equal
    /// expressions are computed once.
    locals: BTreeMap<String, (String, usize)>,
    slot_of: BTreeMap<(Batch, String), usize>,
}

/// Whether a node uses an operation only the CPU baker has, looking through arguments and helper
/// calls' arguments but not into other fields or helper bodies.
fn uses_cpu_only(node: &Node) -> bool {
    match &node.kind {
        // An external input is a plane over the receiver's texels, like a baked CPU field.
        Kind::Builtin(Builtin::Runoff | Builtin::External, _) => true,
        Kind::Unary(_, a) | Kind::Member(a, _) => uses_cpu_only(a),
        Kind::Binary(_, a, b) => uses_cpu_only(a) || uses_cpu_only(b),
        Kind::Select(a, b, c) => uses_cpu_only(a) || uses_cpu_only(b) || uses_cpu_only(c),
        Kind::Builtin(_, args) | Kind::Function(_, args) => args.iter().any(uses_cpu_only),
        _ => false,
    }
}

/// A slot's place in its arrays while the source is assembled; `settle` writes the final one.
fn slot_mark(slot: usize) -> String {
    format!("@{slot}@")
}

/// Replaces every slot mark of `text` by the slot's place: its round's arrays and its index.
fn settle(text: &str, place: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut parts = text.split('@');
    out.push_str(parts.next().unwrap_or(""));
    while let (Some(slot), Some(rest)) = (parts.next(), parts.next()) {
        let (round, index) = place[slot.parse::<usize>().expect("a slot mark")];
        out.push_str(&format!("{round}[{index}]"));
        out.push_str(rest);
    }
    out
}

impl<'a> Emitter<'a> {
    /// The round a statement reading `uses` runs in.
    fn round(&self, uses: &[Use]) -> usize {
        uses.iter()
            .map(|used| match used {
                Use::Statement(statement) => self.statements[*statement].round,
                Use::Slot(slot) => self.statements[self.slots[*slot].statement].round + 1,
            })
            .max()
            .unwrap_or(0)
    }

    fn statement(&mut self, text: String, uses: Vec<Use>) -> usize {
        let round = self.round(&uses);
        self.statements.push(Statement { text, uses, round });
        self.statements.len() - 1
    }

    /// Emits with `emit` and returns what that read, apart from what the enclosing expression has.
    fn reading<T>(&mut self, emit: impl FnOnce(&mut Self) -> T) -> (T, Vec<Use>) {
        let outer = std::mem::take(&mut self.uses);
        let value = emit(self);
        (value, std::mem::replace(&mut self.uses, outer))
    }

    /// A lattice noise as a slot: its arguments are set by a statement, a loop evaluates it at the
    /// end of that round, and the expression reads the result. Equal arguments share a slot.
    fn slot(
        &mut self,
        batch: Batch,
        point: String,
        seed: String,
        count: &str,
        uses: Vec<Use>,
    ) -> String {
        if self.inline {
            self.uses.extend(uses);
            return match batch {
                Batch::Value2 | Batch::Value3 => format!("ms_value_noise({point}, {seed})"),
                _ => format!(
                    "ms_noise({point}, {seed}, {count}, {})",
                    matches!(batch, Batch::Cells2 | Batch::Cells3)
                ),
            };
        }
        let p = batch.prefix();
        let key = (batch, format!("{point}|{seed}|{count}"));
        let slot = match self.slot_of.get(&key) {
            Some(slot) => *slot,
            None => {
                let slot = self.slots.len();
                let i = slot_mark(slot);
                let mut text =
                    format!("  {p}p{i} = {point}; {p}k{i} = {seed}; {p}s{i} = ms_spread({p}p{i});");
                if batch.octaves() {
                    text.push_str(&format!(" {p}c{i} = {count};"));
                }
                text.push('\n');
                let statement = self.statement(text, uses);
                self.slots.push(Slot { batch, statement });
                self.slot_of.insert(key, slot);
                slot
            }
        };
        self.uses.push(Use::Slot(slot));
        format!("{p}r{}", slot_mark(slot))
    }

    /// A helper call as the helper's body with the arguments in place. An argument the body reads
    /// more than once is computed into a local first, unless it is a name or a constant.
    fn expand(&mut self, index: usize, args: &[Node], scope: Option<usize>) -> String {
        let mut frame = Vec::with_capacity(args.len());
        for (position, argument) in args.iter().enumerate() {
            let seed = self.functions[index].seeds[position];
            let (text, uses) = self.reading(|emitter| {
                if seed {
                    emitter.seed(argument, scope)
                } else {
                    emitter.emit(argument, scope)
                }
            });
            if self.functions[index].reads[position] < 2 || !text.contains(' ') {
                frame.push(Binding { text, uses });
                continue;
            }
            let kind = if seed {
                "uint"
            } else {
                glsl_type(self.functions[index].arguments[position].1)
            };
            let (name, statement) = self.named(kind, text, uses);
            frame.push(Binding {
                text: name,
                uses: vec![Use::Statement(statement)],
            });
        }
        self.frames.push(frame);
        let body = self.functions[index].body;
        let (value, uses) = self.reading(|emitter| emitter.emit(body, Some(index)));
        self.frames.pop();
        if !value.contains(' ') {
            self.uses.extend(uses);
            return value;
        }
        // The result is a local too, so a call made twice with the same arguments is computed once.
        let (name, statement) = self.named(glsl_type(self.functions[index].result), value, uses);
        self.uses.push(Use::Statement(statement));
        name
    }

    /// The local that holds `text`, declared by a statement the first time.
    fn named(&mut self, kind: &str, text: String, uses: Vec<Use>) -> (String, usize) {
        let declaration = format!("{kind} {text}");
        if let Some(found) = self.locals.get(&declaration) {
            return found.clone();
        }
        let name = format!("ms_t{}", self.locals.len());
        let statement = self.statement(format!("  {kind} {name} = {text};\n"), uses);
        self.locals.insert(declaration, (name.clone(), statement));
        (name, statement)
    }

    /// The argument a helper's body reads, noting what it read.
    fn local(&mut self, index: usize) -> String {
        let binding = &self.frames.last().expect("a helper body")[index];
        let text = binding.text.clone();
        let uses = binding.uses.clone();
        self.uses.extend(uses);
        text
    }

    fn field(&mut self, index: usize) -> String {
        self.uses.push(Use::Statement(self.field_statement[index]));
        format!("ms_f{index}")
    }

    /// Whether a node is arithmetic on numbers and scalar parameters alone, through scalar fields.
    fn pure(&mut self, node: &Node) -> bool {
        use Builtin as B;
        match &node.kind {
            Kind::Number(_) => true,
            Kind::Parameter(index) => self.program.metadata.parameters[*index].kind == Type::Scalar,
            Kind::Field(index) => {
                let program = self.program;
                let body = &program.compiled.fields[*index];
                !self.cpu_only[*index]
                    && self.field_type(*index) == Type::Scalar
                    && !self.field_is_seed(*index)
                    && self.pure(body)
            }
            Kind::Unary(Op::Sub, a) => self.pure(a),
            Kind::Binary(Op::Add | Op::Sub | Op::Mul | Op::Div, a, b) => {
                self.pure(a) && self.pure(b)
            }
            Kind::Builtin(
                B::Mix
                | B::Clamp
                | B::Smoothstep
                | B::Abs
                | B::Floor
                | B::Fract
                | B::Sin
                | B::Cos
                | B::Sqrt
                | B::Exp
                | B::Min
                | B::Max
                | B::Step
                | B::Pow,
                args,
            ) => args.iter().all(|a| self.pure(a)),
            _ => false,
        }
    }

    /// A pure node as GLSL that reads parameter uniforms only, with the uniforms it reads.
    fn pure_text(&self, node: &Node, parameters: &mut Vec<String>) -> String {
        use Builtin as B;
        match &node.kind {
            Kind::Number(value) => literal(*value),
            Kind::Parameter(index) => {
                let uniform = format!("ms_p{index}");
                if !parameters.contains(&uniform) {
                    parameters.push(uniform.clone());
                }
                uniform
            }
            Kind::Field(index) => self.pure_text(&self.program.compiled.fields[*index], parameters),
            Kind::Unary(_, a) => format!("(-{})", self.pure_text(a, parameters)),
            Kind::Binary(op, a, b) => {
                let sign = match op {
                    Op::Add => "+",
                    Op::Sub => "-",
                    Op::Mul => "*",
                    _ => "/",
                };
                format!(
                    "({} {sign} {})",
                    self.pure_text(a, parameters),
                    self.pure_text(b, parameters)
                )
            }
            Kind::Builtin(name, args) => {
                let a: Vec<String> = args.iter().map(|a| self.pure_text(a, parameters)).collect();
                let call = match name {
                    B::Mix => "mix",
                    B::Clamp => "clamp",
                    B::Smoothstep => "smoothstep",
                    B::Abs => "abs",
                    B::Floor => "floor",
                    B::Fract => "fract",
                    B::Sin => "sin",
                    B::Cos => "cos",
                    B::Sqrt => "sqrt",
                    B::Exp => "exp",
                    B::Min => "min",
                    B::Max => "max",
                    B::Step => "step",
                    // As `builtin` emits it: the base is held at zero or above.
                    _ => return format!("pow(max({}, 0.0), {})", a[0], a[1]),
                };
                format!("{call}({})", a.join(", "))
            }
            _ => "0.0".into(),
        }
    }

    /// What a live argument read from field `index` (as `symbol`) is to a host that bakes.
    fn classify(&mut self, index: usize, symbol: &str) -> LiveArgument {
        let program = self.program;
        let node = &program.compiled.fields[index];
        if self.cpu_only[index]
            || self.field_type(index) != Type::Scalar
            || self.field_is_seed(index)
            || !self.pure(node)
        {
            return LiveArgument::Varying {
                varying: symbol.to_owned(),
            };
        }
        let mut parameters = Vec::new();
        let glsl = self.pure_text(node, &mut parameters);
        // A field that only repeats a number, through other fields, is that number.
        let mut value = node;
        while let Kind::Field(next) = &value.kind {
            value = &program.compiled.fields[*next];
        }
        match &value.kind {
            Kind::Number(constant) => LiveArgument::Constant {
                constant: *constant,
            },
            _ => LiveArgument::Parameters { parameters, glsl },
        }
    }

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
                Input::Footprint
                | Input::UvFootprint
                | Input::Time
                | Input::InstanceW
                | Input::InstanceRandom => Type::Scalar,
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
                    | B::FeatureTangent
                    | B::AdjustNormal
                    | B::TriplanarWeights
                    | B::VariedColor
                    | B::SampleColor
                    | B::Ramp
                    | B::SampleField
                    | B::SampleData => Type::Vec3,
                    B::Mix | B::Min | B::Max | B::Step => wider(of(0), of(1)),
                    B::Smoothstep => wider(wider(of(0), of(1)), of(2)),
                    B::Remap | B::External | B::InstanceAttribute | B::VertexAttribute => of(1),
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
            Kind::Local(index) if self.seedlike(node, scope) => self.local(*index),
            Kind::Field(index) if self.seedlike(node, scope) => self.field(*index),
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
                Input::Time => {
                    self.render.insert(("msTime".into(), "uniform", "float"));
                    "ms_time"
                }
                Input::Instance | Input::InstanceW => {
                    self.render
                        .insert(("msInstanceValue".into(), "instance", "vec4"));
                    if *input == Input::Instance {
                        "ms_instance.xyz"
                    } else {
                        "ms_instance.w"
                    }
                }
                Input::SunDirection => {
                    self.render
                        .insert(("msSunDirection".into(), "uniform", "vec3"));
                    "ms_sun_direction"
                }
                // Varyings the host derives from what it draws: the instance's placement and
                // tint, a number per instance, the placed position.
                Input::InstanceRoot => {
                    self.render
                        .insert(("msInstanceRoot".into(), "derived", "vec3"));
                    "msInstanceRoot"
                }
                Input::InstanceTint => {
                    self.render
                        .insert(("msInstanceTint".into(), "derived", "vec3"));
                    "msInstanceTint"
                }
                Input::InstanceRandom => {
                    self.render
                        .insert(("msInstanceRandom".into(), "derived", "float"));
                    "msInstanceRandom"
                }
                Input::PlacedWorld => {
                    self.render
                        .insert(("msPlacedWorld".into(), "derived", "vec3"));
                    "msPlacedWorld"
                }
            }
            .into(),
            Kind::Parameter(index) => format!("ms_p{index}"),
            Kind::Field(index) if self.seedlike(node, scope) => {
                format!("float({})", self.field(*index))
            }
            Kind::Field(index) => self.field(*index),
            Kind::Local(index) if self.seedlike(node, scope) => {
                format!("float({})", self.local(*index))
            }
            Kind::Local(index) => self.local(*index),
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
            Kind::Function(index, args) => self.expand(*index, args, scope),
            Kind::Builtin(name, args) => match self.batch(*name, args, scope) {
                Some(batch) => {
                    let ((point, seed, count), uses) = self.reading(|emitter| {
                        let point = emitter.emit(&args[0], scope);
                        let seed = emitter.seed(&args[1], scope);
                        let count = match name {
                            Builtin::Fbm => emitter.emit(&args[2], scope),
                            _ => "1.0".into(),
                        };
                        (point, seed, count)
                    });
                    self.slot(batch, point, seed, &count, uses)
                }
                None => self.builtin(*name, args, node, scope),
            },
        }
    }

    /// The kind of slot a lattice noise call takes, if it is one.
    fn batch(&self, name: Builtin, args: &[Node], scope: Option<usize>) -> Option<Batch> {
        let vec3 = match self.kind(args.first()?, scope) {
            Type::Vec2 => false,
            Type::Vec3 => true,
            _ => return None,
        };
        Some(match (name, vec3) {
            (Builtin::ValueNoise, false) => Batch::Value2,
            (Builtin::ValueNoise, true) => Batch::Value3,
            (Builtin::Noise | Builtin::Fbm, false) => Batch::Gradient2,
            (Builtin::Noise | Builtin::Fbm, true) => Batch::Gradient3,
            (Builtin::Cellular, false) => Batch::Cells2,
            (Builtin::Cellular, true) => Batch::Cells3,
            _ => return None,
        })
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
            // Outside a CPU field the shader has no plane to read: the fallback.
            B::External => a[1].clone(),
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
            B::FeatureDistance | B::FeatureTangent => {
                // A name is a selector of its own: 8 plus its place in `Shader::named_features`,
                // which selects the features a host uploads with kind 5 plus that place.
                self.features = true;
                let feature = Self::text(&args[0]);
                let names = &self.program.metadata.named_features;
                let place = names.iter().position(|known| known == feature);
                let query = match name {
                    B::FeatureTangent => "tangent",
                    _ => "distance",
                };
                format!(
                    "ms_crease_{query}({}, 0.0, {})",
                    8 + place.unwrap_or(0),
                    a[1]
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
            // The host draws the cutout: it tests this coverage against the declared cutoff.
            B::AlphaCutout => format!("clamp({}, 0.0, 1.0)", a[0]),
            // Render-only attributes are varyings the renderer declares and sets.
            B::InstanceAttribute | B::VertexAttribute => {
                let (prefix, kind) = if name == B::InstanceAttribute {
                    ("msInstance_", "instance")
                } else {
                    ("msVertex_", "vertex")
                };
                let symbol = format!("{prefix}{}", Self::text(&args[0]));
                let glsl = match result {
                    Type::Vec2 => "vec2",
                    Type::Vec3 => "vec3",
                    _ => "float",
                };
                self.render.insert((symbol.clone(), kind, glsl));
                // What a host reads where the drawn mesh has no such attribute: the value the
                // call is typed by, as the evaluator's bindings read it. The first call's wins.
                self.fallbacks
                    .entry(symbol.clone())
                    .or_insert_with(|| a[1].clone());
                symbol
            }
            B::ShadeBand => {
                self.shade_band = true;
                format!(
                    "ms_shade_band({}, {}, {}, {}, {})",
                    a[0], a[1], a[2], a[3], a[4]
                )
            }
            B::SampleField => {
                let field = Self::text(&args[0]);
                let index = self
                    .fields
                    .iter()
                    .position(|known| known == field)
                    .unwrap_or_else(|| {
                        self.fields.push(field.to_owned());
                        self.fields.len() - 1
                    });
                format!(
                    "ms_sample_field(msField{index}, msFieldRect{index}, {})",
                    a[1]
                )
            }
            B::AuthoredMask => format!(
                "ms_authored_mask({}, {})",
                self.image_uniform(Self::text(&args[0]), true),
                a[1]
            ),
            B::Ramp => {
                let mode = Self::text(&args[0]);
                let stops = (args.len() - 2) / 2;
                self.ramps.insert(stops);
                let ramp = format!(
                    "ms_ramp{stops}({}, {})",
                    a[1..].join(", "),
                    mode.ends_with("constant")
                );
                if mode.starts_with("display") {
                    self.display_ramp = true;
                    format!("ms_srgb_to_linear({ramp})")
                } else {
                    ramp
                }
            }
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

/// `shade_band`: as the evaluator computes it (`SHADE_BAND_FLOOR` is its narrowest half-width),
/// widened by `softness` pixels of the difference's screen derivative, which a sample has not.
/// `ms_cast_shadow` is the sun's cast shadow at the drawn point in the scene's shadow style; a
/// host that draws shadows sets it before `ms_evaluate()`, and without one it stays lit.
const SHADE_BAND: &str = "float ms_cast_shadow = 1.0;
float ms_shade_band(vec3 n, vec3 s, float threshold, float softness, float range) {
  float d = dot(n, s) - threshold;
  float w = max(max(range, 1e-6), softness * fwidth(d));
  return min(smoothstep(-w, w, d), clamp(ms_cast_shadow, 0.0, 1.0));
}
";

/// `sample_field`: bilinear between texel centers of a float texture over a rectangle of the
/// world's XY plane (`rect` is its minimum and maximum corner), edge texels continuing outside, as
/// `SceneField::sample` reads it. Row 0 is the rectangle's lower edge.
const SAMPLE_FIELD: &str = "vec3 ms_sample_field(highp sampler2D field, vec4 rect, vec2 p) {
  ivec2 size = textureSize(field, 0);
  vec2 x = clamp((p - rect.xy) / (rect.zw - rect.xy) * vec2(size) - 0.5, vec2(0.0), vec2(size - 1));
  vec2 base = floor(x), t = x - base;
  ivec2 c = ivec2(base), n = min(c + 1, size - 1);
  vec3 low = mix(texelFetch(field, c, 0).xyz, texelFetch(field, ivec2(n.x, c.y), 0).xyz, t.x);
  vec3 high = mix(texelFetch(field, ivec2(c.x, n.y), 0).xyz, texelFetch(field, n, 0).xyz, t.x);
  return mix(low, high, t.y);
}
";

/// `authored_mask`: the red channel over the unit square, edge texels continuing to its edge,
/// zero outside, whatever wrapping the host gave the texture.
const AUTHORED_MASK: &str = "float ms_authored_mask(sampler2D image, vec2 uv) {
  if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) return 0.0;
  vec2 half_texel = 0.5 / vec2(textureSize(image, 0));
  return texture(image, clamp(uv, half_texel, 1.0 - half_texel)).r;
}
";

/// Whether any node of the program is an `authored_mask`.
fn source_reads_mask(compiled: &super::compiled::Compiled) -> bool {
    fn within(node: &Node) -> bool {
        match &node.kind {
            Kind::Builtin(Builtin::AuthoredMask, _) => true,
            Kind::Unary(_, a) | Kind::Member(a, _) => within(a),
            Kind::Binary(_, a, b) => within(a) || within(b),
            Kind::Select(a, b, c) => within(a) || within(b) || within(c),
            Kind::Function(_, args) | Kind::Builtin(_, args) => args.iter().any(within),
            _ => false,
        }
    }
    compiled.fields.iter().any(within)
        || compiled
            .functions
            .iter()
            .any(|function| within(&function.body))
        || compiled.outputs.iter().any(|(_, node)| within(node))
}

/// The helper of a ramp of `stops` stops: each stop after the first takes over from the color
/// so far, at once (`hold`) or across the stretch from the stop before it.
fn ramp_helper(stops: usize) -> String {
    let mut text = format!("vec3 ms_ramp{stops}(float t");
    for stop in 0..stops {
        text.push_str(&format!(", float p{stop}, vec3 c{stop}"));
    }
    text.push_str(", bool hold) {\n  vec3 c = c0;\n");
    for stop in 1..stops {
        let before = stop - 1;
        text.push_str(&format!(
            "  c = mix(c, c{stop}, hold ? step(p{stop}, t) : clamp((t - p{before}) / max(p{stop} - p{before}, 1e-30), 0.0, 1.0));\n"
        ));
    }
    text.push_str("  return c;\n}\n");
    text
}

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

/// The define that selects the entry points with every noise where it is used.
const INLINE: &str = "MS_INLINE_NOISE";

/// Both forms of the entry points over one set of declarations. Looping over slots compiles fast
/// and draws slower (a program of a hundred noises compiled in a third of the time and drew in
/// seven times the time on the GPU this was measured on): right for a bake, which draws once, and
/// for a first look. A renderer that shades every frame with the program can compile the other
/// form behind it by defining `MS_INLINE_NOISE` before the source: every noise where it is used,
/// which draws as fast as the program can and compiles slowly.
fn build(program: &Program, partition: bool) -> Result<Shader, String> {
    let (mut shader, at) = emit(program, partition, false)?;
    let (inline, inline_at) = emit(program, partition, true)?;
    debug_assert_eq!(shader.source[..at], inline.source[..inline_at]);
    let looped = shader.source.split_off(at);
    shader.source.push_str(&format!(
        "#ifdef {INLINE}\n{}#else\n{looped}#endif\n",
        &inline.source[inline_at..]
    ));
    // FNV-1a over the source.
    shader.id = format!(
        "{:016x}",
        shader
            .source
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            })
    );
    Ok(shader)
}

/// One form of the program: the shader, and where in its source the entry points begin.
fn emit(program: &Program, partition: bool, inline: bool) -> Result<(Shader, usize), String> {
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
            let mut reads = vec![0; arguments.len()];
            count_reads(&function.body, &mut reads);
            Function {
                arguments,
                result: *result,
                body: &function.body,
                seeds: arguments
                    .iter()
                    .map(|(_, kind)| *kind == Type::Scalar)
                    .collect(),
                needed: false,
                reads,
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
        ramps: Default::default(),
        display_ramp: false,
        render: Default::default(),
        shade_band: false,
        fields: Vec::new(),
        fallbacks: BTreeMap::new(),
        field_state: vec![0; compiled.fields.len()],
        field_order: Vec::new(),
        function_order: Vec::new(),
        cpu_only: compiled
            .fields
            .iter()
            .map(|field| partition && uses_cpu_only(field))
            .collect(),
        inline,
        statements: Vec::new(),
        slots: Vec::new(),
        uses: Vec::new(),
        frames: Vec::new(),
        field_statement: vec![usize::MAX; compiled.fields.len()],
        locals: BTreeMap::new(),
        slot_of: BTreeMap::new(),
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
    let mut live_fields = Vec::new();
    for operation in &program.metadata.live {
        let mut arguments = BTreeMap::new();
        let mut classes = BTreeMap::new();
        for (argument, field) in &operation.inputs {
            let index = field_index(field)?;
            emitter.require_field(index);
            let symbol = match (emitter.field_type(index), emitter.field_is_seed(index)) {
                (_, true) | (Type::Bool, _) => format!("float(ms_f{index})"),
                (Type::Scalar, _) => format!("ms_f{index}"),
                _ => format!("ms_f{index}.x"),
            };
            classes.insert(argument.clone(), emitter.classify(index, &symbol));
            arguments.insert(argument.clone(), symbol);
            live_fields.push(index);
        }
        live.push(LiveOperation {
            id: operation.id.clone(),
            arguments,
            classes,
        });
    }
    // A painted material's color: its paint field, which `ms_evaluate` computes, or the base color.
    let mut paint_field = None;
    let painted = match (program.metadata.shading, &program.metadata.paint) {
        (super::Shading::Lit, _) => None,
        (super::Shading::Painted, Some(name)) => {
            let index = field_index(name)?;
            emitter.require_field(index);
            paint_field = Some(index);
            Some(format!("ms_f{index}"))
        }
        (super::Shading::Painted, None) => Some("ms_base_color".to_owned()),
    };

    if partition {
        for index in emitter.function_order.clone() {
            if uses_cpu_only(emitter.functions[index].body) {
                return Err("A helper function that the GPU part needs uses runoff, which cannot be partitioned.".into());
            }
        }
    }
    let mut declarations = String::new();
    let mut cpu_fields: Vec<CpuField> = Vec::new();
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
            emitter.field_statement[index] =
                emitter.statement(format!("  ms_f{index} = {value};\n"), Vec::new());
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
                planes: found.externals.iter().cloned().collect(),
            });
            continue;
        }
        let seed = emitter.field_is_seed(index);
        let (value, uses) = emitter.reading(|emitter| {
            if seed {
                emitter.seed(node, None)
            } else {
                emitter.emit(node, None)
            }
        });
        emitter.field_statement[index] =
            emitter.statement(format!("  ms_f{index} = {value};\n"), uses);
    }
    // Each output's value and what it reads.
    let mut outputs = Vec::new();
    let mut values: BTreeMap<&str, (String, Vec<Use>)> = BTreeMap::new();
    for (name, node) in &compiled.outputs {
        values.insert(
            name.as_str(),
            emitter.reading(|emitter| emitter.emit(node, None)),
        );
        outputs.push(name.clone());
    }
    let reach = |roots: &[Use]| {
        let mut statements = vec![false; emitter.statements.len()];
        let mut slots = vec![false; emitter.slots.len()];
        let mut pending = roots.to_vec();
        while let Some(used) = pending.pop() {
            let statement = match used {
                Use::Statement(statement) => statement,
                Use::Slot(slot) => {
                    slots[slot] = true;
                    emitter.slots[slot].statement
                }
            };
            if !std::mem::replace(&mut statements[statement], true) {
                pending.extend(&emitter.statements[statement].uses);
            }
        }
        (statements, slots)
    };
    let slot_round = |slot: usize| emitter.statements[emitter.slots[slot].statement].round;
    let rounds = emitter
        .statements
        .iter()
        .map(|statement| statement.round + 1)
        .max()
        .unwrap_or(0);
    // An entry point: the statements `roots` reach, round by round, each round followed by the
    // loops of the slots it filled. Each round of a kind has arrays of its own, local to the entry
    // point and exactly as long as the slots it reaches: a compiler follows an array that is
    // filled whole, read by one loop and then only read far faster than one that is filled in
    // part, filled again after a loop has read it, or read by two loops.
    let entry = |name: &str, defaults: &str, roots: &[Use], results: &[&str]| {
        let (statements, slots) = reach(roots);
        let mut place = vec![(0, 0); emitter.slots.len()];
        let mut counts: BTreeMap<(Batch, usize), usize> = BTreeMap::new();
        for slot in (0..emitter.slots.len()).filter(|slot| slots[*slot]) {
            let round = slot_round(slot);
            let count = counts
                .entry((emitter.slots[slot].batch, round))
                .or_default();
            place[slot] = (round, *count);
            *count += 1;
        }
        let mut text = format!("void {name}() {{\n{defaults}");
        for ((batch, round), count) in &counts {
            let p = batch.prefix();
            text.push_str(&format!(
                "  {} {p}p{round}[{count}]; uint {p}k{round}[{count}]; float {p}s{round}[{count}]; float {p}r{round}[{count}];",
                batch.point()
            ));
            if batch.octaves() {
                text.push_str(&format!(" float {p}c{round}[{count}];"));
            }
            text.push('\n');
        }
        for round in 0..rounds {
            for (statement, reached) in emitter.statements.iter().zip(&statements) {
                if *reached && statement.round == round {
                    text.push_str(&settle(&statement.text, &place));
                }
            }
            for batch in Batch::ALL {
                let Some(count) = counts.get(&(batch, round)) else {
                    continue;
                };
                let p = batch.prefix();
                if *count >= LOOP_SLOTS {
                    text.push_str(&format!(
                        "  for (int i = 0; i < {count} + ms_loop; i++) {p}r{round}[i] = {};\n",
                        batch.call(&format!("{round}[i]"))
                    ));
                } else {
                    for index in 0..*count {
                        text.push_str(&format!(
                            "  {p}r{round}[{index}] = {};\n",
                            batch.call(&format!("{round}[{index}]"))
                        ));
                    }
                }
            }
        }
        for name in results {
            if let Some((value, _)) = values.get(name) {
                text.push_str(&format!("  ms_{name} = {};\n", settle(value, &place)));
            }
        }
        text.push_str("}\n");
        text
    };
    let default_of = |name: &str| match name {
        "base_color" => "ms_base_color = vec3(0.0);",
        "roughness" => "ms_roughness = 1.0;",
        "metalness" => "ms_metalness = 0.0;",
        "height" => "ms_height = 0.0;",
        "emissive" => "ms_emissive = vec3(0.0);",
        "opacity" => "ms_opacity = 1.0;",
        "receiver_normal" => "ms_receiver_normal = ms_normal;",
        _ => "ms_normal_offset = vec3(0.0);",
    };
    let live_roots: Vec<Use> = live_fields
        .iter()
        .map(|index| Use::Statement(emitter.field_statement[*index]))
        .collect();
    // A part of the program: the named outputs, and the live fields when `live`.
    let part = |name: &str, results: &[&str], live: bool| {
        let mut roots: Vec<Use> = results
            .iter()
            .filter_map(|name| values.get(name))
            .flat_map(|(_, uses)| uses.iter().copied())
            .collect();
        if live {
            roots.extend(&live_roots);
        }
        let defaults: Vec<&str> = results.iter().map(|name| default_of(name)).collect();
        let defaults = if defaults.is_empty() {
            String::new()
        } else {
            format!("  {}\n", defaults.join(" "))
        };
        entry(name, &defaults, &roots, results)
    };

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
    if source_reads_mask(compiled) {
        source.push_str(AUTHORED_MASK);
    }
    // Render-only inputs: the renderer binds `msTime` and declares and sets the varyings.
    for (symbol, _, _) in &emitter.render {
        match symbol.as_str() {
            "msTime" => source.push_str("uniform float msTime;\n#define ms_time msTime\n"),
            "msInstanceValue" => source.push_str("#define ms_instance msInstanceValue\n"),
            "msSunDirection" => source.push_str(
                "uniform vec3 msSunDirection;\n#define ms_sun_direction msSunDirection\n",
            ),
            _ => {}
        }
    }
    if emitter.shade_band {
        source.push_str(SHADE_BAND);
    }
    if !emitter.fields.is_empty() {
        for index in 0..emitter.fields.len() {
            source.push_str(&format!(
                "uniform highp sampler2D msField{index};\nuniform vec4 msFieldRect{index};\n"
            ));
        }
        source.push_str(SAMPLE_FIELD);
    }
    for stops in &emitter.ramps {
        source.push_str(&ramp_helper(*stops));
    }
    if emitter.display_ramp {
        source.push_str(
            "vec3 ms_srgb_to_linear(vec3 c) { return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), c)); }\n",
        );
    }
    source.push_str(
        "vec3 ms_base_color; float ms_roughness; float ms_metalness; float ms_height;\n\
         vec3 ms_emissive; float ms_opacity; vec3 ms_receiver_normal; vec3 ms_normal_offset;\n",
    );
    source.push_str(&declarations);
    let functions = source.len();
    let names: Vec<&str> = outputs.iter().map(String::as_str).collect();
    let mut all_roots = live_roots.clone();
    all_roots.extend(values.values().flat_map(|(_, uses)| uses.iter().copied()));
    if let Some(index) = paint_field {
        all_roots.push(Use::Statement(emitter.field_statement[index]));
    }
    source.push_str(&entry(
        "ms_evaluate",
        "  ms_base_color = vec3(0.0); ms_roughness = 1.0; ms_metalness = 0.0; ms_height = 0.0;\n  \
         ms_emissive = vec3(0.0); ms_opacity = 1.0; ms_receiver_normal = ms_normal; ms_normal_offset = vec3(0.0);\n",
        &all_roots,
        &names,
    ));
    // Parts of the program, each with only the statements its outputs reach, in the order
    // `ms_evaluate` has them. The height alone is what a renderer that shades per pixel calls at
    // neighboring surface points, to take the relief's slope by finite differences, which screen
    // derivatives of `ms_height` give at half resolution; it is part of the source. The others are
    // a bake's textures (color, properties, relief as the height with the receiver normal that
    // turns it, emissive, the normal offset and the live arguments) and stay beside the source,
    // for a host that wants a program per texture. Every part writes the same field globals as
    // `ms_evaluate`, so a host that calls several calls `ms_evaluate` last.
    let mut entries = Vec::new();
    if values.contains_key("height") {
        source.push_str(&part("ms_evaluate_height", &["height"], false));
        entries.push(EntryPoint {
            function: "ms_evaluate_height".into(),
            outputs: vec!["height".into()],
            live: false,
            source: String::new(),
        });
    }
    let mut add = |function: &str, results: &[&str], live: bool| {
        if inline {
            return;
        }
        entries.push(EntryPoint {
            function: function.to_owned(),
            outputs: results.iter().map(|name| (*name).to_owned()).collect(),
            live,
            source: part(function, results, live),
        });
    };
    add("ms_evaluate_base", &["base_color", "opacity"], false);
    add("ms_evaluate_properties", &["roughness", "metalness"], false);
    add("ms_evaluate_relief", &["height", "receiver_normal"], false);
    if values.contains_key("emissive") {
        add("ms_evaluate_emissive", &["emissive"], false);
    }
    if values.contains_key("normal_offset") {
        add("ms_evaluate_offset", &["normal_offset"], false);
    }
    if !live_roots.is_empty() {
        add("ms_evaluate_live", &[], true);
    }
    // The outputs that are the same at a surface position on every receiver, and the others:
    // a host bakes the first once for a stretch of the material's domain (they read only
    // `ms_surface` and `ms_footprint`) and maps the result onto receivers, and draws the second
    // for each receiver. `Shader::reads` says which is which.
    let all = program.dependency_table();
    let (intrinsic, remainder): (Vec<&str>, Vec<&str>) = names
        .iter()
        .partition(|name| all.outputs.get(**name).is_some_and(|read| read.intrinsic()));
    // A program whose outputs are all of one kind has nothing to split: `ms_evaluate` is it.
    if !intrinsic.is_empty() && !remainder.is_empty() {
        add("ms_evaluate_intrinsic", &intrinsic, false);
        add("ms_evaluate_remainder", &remainder, false);
    }
    let uniforms: Vec<UniformDeclaration> = source
        .lines()
        .filter_map(|line| {
            let mut words = line
                .strip_prefix("uniform ")?
                .trim_end_matches(';')
                .split(' ');
            let kind = match words.next()? {
                "highp" => words.next()?,
                kind => kind,
            };
            Some(UniformDeclaration {
                name: words.next()?.to_owned(),
                kind: kind.to_owned(),
            })
        })
        .collect();
    let samplers = uniforms
        .iter()
        .filter(|uniform| uniform.kind.starts_with("sampler"))
        .count();
    let cpu_outputs: Vec<String> = table
        .as_ref()
        .filter(|_| !cpu_fields.is_empty())
        .map(|table| {
            outputs
                .iter()
                .filter(|name| {
                    table
                        .outputs
                        .get(*name)
                        .is_some_and(|found| found.edges || !found.externals.is_empty())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let reads_of = |found: &super::Dependencies| Reads {
        parameters: found
            .parameters
            .iter()
            .map(|i| format!("ms_p{i}"))
            .collect(),
        seed: found.seed,
        images: found.images.iter().cloned().collect(),
        edges: found.edges,
        features: found.features || !found.named_features.is_empty(),
        sample: found.sample.names(),
        cpu_fields: cpu_fields
            .iter()
            .filter(|field| {
                let index = program
                    .metadata
                    .fields
                    .iter()
                    .position(|known| known.name == field.name);
                index.is_some_and(|index| found.fields.contains(&index))
            })
            .map(|field| field.name.clone())
            .collect(),
        planes: found.externals.iter().cloned().collect(),
        intrinsic: found.intrinsic(),
    };
    let reads = ShaderReads {
        outputs: outputs
            .iter()
            .filter_map(|name| Some((name.clone(), reads_of(all.outputs.get(name)?))))
            .collect(),
        live: program
            .metadata
            .live
            .iter()
            .map(|operation| {
                let arguments = operation
                    .inputs
                    .iter()
                    .filter_map(|(argument, field)| {
                        Some((argument.clone(), reads_of(all.fields.get(field)?)))
                    })
                    .collect();
                (operation.id.clone(), arguments)
            })
            .collect(),
        painted: match (&painted, &program.metadata.paint) {
            (None, _) => None,
            (Some(_), Some(field)) => all.fields.get(field).map(reads_of),
            (Some(_), None) => all.outputs.get("base_color").map(reads_of),
        },
    };
    let mut render_inputs: Vec<RenderInput> = emitter
        .render
        .iter()
        .map(|(symbol, kind, glsl)| RenderInput {
            symbol: symbol.clone(),
            kind,
            glsl,
            name: None,
            fallback: emitter.fallbacks.get(symbol).cloned(),
        })
        .collect();
    for (index, name) in emitter.fields.iter().enumerate() {
        render_inputs.push(RenderInput {
            symbol: format!("msField{index}"),
            kind: "field",
            glsl: "sampler2D",
            name: Some(name.clone()),
            fallback: None,
        });
        render_inputs.push(RenderInput {
            symbol: format!("msFieldRect{index}"),
            kind: "field",
            glsl: "vec4",
            name: Some(name.clone()),
            fallback: None,
        });
    }
    Ok((
        Shader {
            source,
            parameters,
            seeds,
            images,
            outputs,
            live,
            features: emitter.features,
            named_features: program.metadata.named_features.clone(),
            cpu_fields,
            cpu_outputs,
            id: String::new(),
            uniforms,
            samplers,
            entries,
            reads,
            render_inputs,
            painted,
            alpha_cutoff: program.metadata.alpha_cutoff,
        },
        functions,
    ))
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
        for form in forms(&shader.source) {
            let entry = function(form, "ms_evaluate_height").expect("a height entry point");
            // `bump` and, through the helper's body, `relief`; not `tone`, which only the color reads.
            assert_eq!(entry.matches("  ms_f").count(), 2, "{entry}");
            assert!(entry.contains("ms_height = "));
            let whole = function(form, "ms_evaluate").unwrap();
            assert_eq!(whole.matches("  ms_f").count(), 3);
        }
        assert!(
            !plan_of(&source(""))
                .unwrap()
                .source
                .contains("ms_evaluate_height")
        );
    }

    /// The two forms of the entry points: with `MS_INLINE_NOISE` defined, and without.
    fn forms(source: &str) -> [&str; 2] {
        let (_, rest) = source.split_once("#ifdef MS_INLINE_NOISE\n").unwrap();
        let (inline, rest) = rest.split_once("#else\n").unwrap();
        let (looped, rest) = rest.split_once("#endif\n").unwrap();
        assert_eq!(rest, "", "nothing follows the entry points");
        [inline, looped]
    }

    /// The body of an entry point.
    fn function<'a>(source: &'a str, name: &str) -> Option<&'a str> {
        let (_, rest) = source.split_once(&format!("void {name}() {{\n"))?;
        Some(rest.split_once("\n}\n")?.0)
    }

    /// Without the define every lattice noise is a slot: a statement sets its arguments and one
    /// loop per round evaluates the round's slots. A noise whose position reads another noise
    /// waits for the next round, a call made twice is one slot, and helpers are expanded so that
    /// the noises inside them are slots too. With the define each noise is where it is used.
    #[test]
    fn noises_are_slots_evaluated_round_by_round() {
        let shader = plan_of(
            "material 1;
             fn grain(p: vec2) -> scalar = noise(p * 5.0, seed(\"grain\")) * noise(p * 11.0, seed(\"grain\"));
             let a = grain(surface);
             let b = grain(surface) + noise(surface * 2.0, seed(\"wide\"));
             let warped = value_noise(surface * 3.0 + a, seed(\"warp\"));
             let c = fbm(surface, seed(\"fbm\"), 4.0);
             output base_color = vec3(a + b + warped + c);
             output roughness = 1;
             output metalness = 0;",
        )
        .unwrap();
        assert!(!shader.source.contains('@'), "every slot has its place");
        let [inline, looped] = forms(&shader.source);
        let body = function(looped, "ms_evaluate").unwrap();
        // The helper's two noises (once, though it is called twice), the wide one and the fbm: one loop.
        assert_eq!(body.matches("ms_g2k0[").count() - 1, 4 + 1, "{body}");
        assert!(body.contains("vec2 ms_g2p0[4]; uint ms_g2k0[4]; float ms_g2s0[4]; float ms_g2r0[4]; float ms_g2c0[4];"));
        assert!(body.contains("for (int i = 0; i < 4 + ms_loop; i++) ms_g2r0[i] = ms_octaves(vec3(ms_g2p0[i], 0.0), 2, ms_g2k0[i], int(ms_g2c0[i]), false, ms_g2s0[i]);"));
        assert!(
            body.contains("ms_g2c0[3] = 4.0;"),
            "the fbm keeps its octaves: {body}"
        );
        // The warped noise reads `a`, which reads slots of the first round: it is filled after their loop.
        let (first, second) = body.split_once("for (int i = 0; i < 4 + ms_loop").unwrap();
        assert!(!first.contains("ms_v2p1[0] =") && second.contains("ms_v2p1[0] ="));
        // A round of fewer than three slots is evaluated line by line.
        assert!(second.contains(
            "  ms_v2r1[0] = ms_fade_value(ms_value2(ms_v2p1[0], ms_v2k1[0]), ms_v2s1[0]);"
        ));
        assert!(!looped.contains("ms_noise(") && !looped.contains("ms_value_noise("));
        let body = function(inline, "ms_evaluate").unwrap();
        assert!(!body.contains("for (") && !body.contains("ms_g2"));
        assert!(body.contains("ms_noise(") && body.contains("ms_value_noise("));
        // Both forms set the same fields from the same uniforms.
        for form in [inline, looped] {
            assert_eq!(
                function(form, "ms_evaluate")
                    .unwrap()
                    .matches("  ms_f")
                    .count(),
                4
            );
        }
        assert_eq!(shader.seeds.len(), 4);
    }

    /// The parts of a bake: each sets its outputs (defaults where the program has none) from only
    /// the statements they reach, with slots numbered from zero for it.
    #[test]
    fn parts_hold_only_what_their_outputs_reach() {
        let shader = plan_of(
            "material 1;
             let tone = noise(surface * 3.0, seed(\"tone\")) + noise(surface * 5.0, seed(\"tone\")) + noise(surface * 7.0, seed(\"tone\"));
             let relief = value_noise(surface * 9.0, seed(\"relief\"));
             output base_color = vec3(tone);
             output roughness = 0.5;
             output metalness = 0;
             output height = relief * 0.01;",
        )
        .unwrap();
        let names: Vec<_> = shader
            .entries
            .iter()
            .map(|entry| entry.function.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "ms_evaluate_height",
                "ms_evaluate_base",
                "ms_evaluate_properties",
                "ms_evaluate_relief"
            ]
        );
        let part = |name: &str| {
            let entry = shader
                .entries
                .iter()
                .find(|entry| entry.function == name)
                .unwrap();
            function(&entry.source, name).unwrap().to_owned()
        };
        // The height is in the source itself, in both forms.
        assert!(shader.entries[0].source.is_empty());
        let base = part("ms_evaluate_base");
        assert!(
            base.contains("ms_base_color = vec3(0.0); ms_opacity = 1.0;")
                && base.contains("ms_g2r0[i]")
        );
        assert!(
            !base.contains("ms_v2") && !base.contains("ms_height"),
            "{base}"
        );
        let properties = part("ms_evaluate_properties");
        assert!(
            properties.contains("ms_roughness = 0.5;")
                && !properties.contains("ms_g2")
                && !properties.contains("ms_f")
        );
        let relief = part("ms_evaluate_relief");
        assert!(relief.contains("ms_height = 0.0; ms_receiver_normal = ms_normal;"));
        assert!(
            relief.contains("ms_v2r0[0] = ") && !relief.contains("ms_g2"),
            "{relief}"
        );
        assert_eq!(shader.entries[3].outputs, ["height", "receiver_normal"]);
    }

    /// What a host needs besides the text: the uniforms the source declares, how many are
    /// samplers, an identity that follows the source, and each live argument as a number, an
    /// expression of parameters or a value that varies.
    #[test]
    fn the_record_states_uniforms_identity_and_live_argument_classes() {
        use super::LiveArgument;
        let steel = examples("kowloon-rusted-steel.material.ts");
        let declared = |name: &str| {
            steel
                .uniforms
                .iter()
                .find(|uniform| uniform.name == name)
                .map(|uniform| uniform.kind.as_str())
        };
        assert_eq!(declared("ms_loop"), Some("int"));
        assert_eq!(declared("ms_features"), Some("sampler2D"));
        assert_eq!(declared(&steel.seeds[0].uniform), Some("uint"));
        assert_eq!(
            steel.uniforms.len(),
            steel.source.matches("\nuniform ").count()
                + usize::from(steel.source.starts_with("uniform "))
        );
        assert_eq!(steel.samplers, steel.images.len() + 1);
        assert_eq!(steel.id.len(), 16);
        assert_eq!(steel.id, examples("kowloon-rusted-steel.material.ts").id);
        assert_ne!(steel.id, examples("kowloon-concrete.material.ts").id);
        let (mut constants, mut parameters, mut varying) = (0, 0, 0);
        for operation in &steel.live {
            assert_eq!(operation.classes.len(), operation.arguments.len());
            for (name, class) in &operation.classes {
                match class {
                    LiveArgument::Constant { constant } => {
                        constants += 1;
                        assert!(constant.is_finite());
                    }
                    LiveArgument::Parameters {
                        parameters: read,
                        glsl,
                    } => {
                        parameters += 1;
                        assert!(!glsl.contains("ms_f"), "{glsl} reads no field");
                        assert!(read.iter().all(|uniform| glsl.contains(uniform.as_str())));
                    }
                    LiveArgument::Varying { varying: symbol } => {
                        varying += 1;
                        assert_eq!(symbol, &operation.arguments[name]);
                    }
                }
            }
        }
        // The study's lighting has fixed widths, strengths that follow parameters and masks that vary.
        assert!(
            constants > 0 && parameters > 0 && varying > 0,
            "{constants} {parameters} {varying}"
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
