use super::{
    compiled::{Builtin, Input, Kind, Node, Op},
    syntax::{Expr, Mark},
    *,
};
use superspec_geometry::math::{cos, sin};

/// A value during evaluation: up to three components, the filtering footprint estimate carried
/// with numbers, and what the components mean. Text is always a literal of the program, so values
/// are plain copies with no reference counting.
///
/// The numbers sit together at fixed offsets and the shape is one small enum beside them. As one
/// enum over the whole value, copies moved padding in odd-sized pieces and the per-node
/// finiteness check reloaded components across them, which stalled the processor on most nodes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Value<'a> {
    data: [f64; 3],
    width: f64,
    shape: Shape<'a>,
}
#[derive(Clone, Copy, Debug)]
enum Shape<'a> {
    /// A scalar or vector with this many components.
    Number(usize),
    Bool(bool),
    Text(&'a std::sync::Arc<str>),
}
#[allow(non_snake_case)]
impl<'a> Value<'a> {
    /// The initial content of a result slot before a node writes it.
    pub(super) const EMPTY: Self = Self {
        data: [0.; 3],
        width: 0.,
        shape: Shape::Bool(false),
    };
    #[inline(always)]
    pub(super) fn Number(data: [f64; 3], components: usize, width: f64) -> Self {
        Self {
            data,
            width,
            shape: Shape::Number(components),
        }
    }
    #[inline(always)]
    pub(super) fn Bool(value: bool) -> Self {
        Self {
            data: [0.; 3],
            width: 0.,
            shape: Shape::Bool(value),
        }
    }
    pub(super) fn Text(value: &'a std::sync::Arc<str>) -> Self {
        Self {
            data: [0.; 3],
            width: 0.,
            shape: Shape::Text(value),
        }
    }
    #[inline(always)]
    pub(super) fn scalar(v: f64) -> Self {
        Self::Number([v, 0., 0.], 1, 0.)
    }
    pub(super) fn vector(v: &[f64], width: f64) -> Self {
        let mut data = [0.; 3];
        data[..v.len()].copy_from_slice(v);
        Self::Number(data, v.len(), width)
    }
    #[inline(always)]
    pub(super) fn data(&self) -> ([f64; 3], usize, f64) {
        match self.shape {
            Shape::Number(n) => (self.data, n, self.width),
            _ => unreachable!("checked numeric type"),
        }
    }
    #[inline(always)]
    pub(super) fn first(&self) -> f64 {
        self.data().0[0]
    }
    #[inline(always)]
    pub(super) fn boolean(&self) -> bool {
        match self.shape {
            Shape::Bool(v) => v,
            _ => unreachable!("checked bool type"),
        }
    }
    fn text(&self) -> &'a str {
        match self.shape {
            Shape::Text(v) => v,
            _ => unreachable!("checked string type"),
        }
    }
    pub(super) fn field(&self) -> FieldValue {
        let v = self.data;
        match self.shape {
            Shape::Number(1) => FieldValue::Scalar(v[0]),
            Shape::Number(2) => FieldValue::Vec2([v[0], v[1]]),
            Shape::Number(_) => FieldValue::Vec3(v),
            Shape::Bool(v) => FieldValue::Bool(v),
            Shape::Text(_) => unreachable!("no string fields"),
        }
    }
    pub fn json(&self) -> serde_json::Value {
        let v = self.data;
        match self.shape {
            Shape::Number(1) => serde_json::json!(v[0]),
            Shape::Number(n) => serde_json::json!(&v[..n]),
            Shape::Bool(v) => serde_json::json!(v),
            Shape::Text(v) => serde_json::json!(&**v),
        }
    }
    /// Whether this is a number with a non-finite or over-large component or footprint estimate.
    #[inline(always)]
    pub(super) fn unsound(&self) -> bool {
        // A biased exponent below 1355 means a magnitude below 2^332, which is finite and under
        // the 1e100 bound; only larger values need the exact comparison. Testing the bits keeps
        // each component a separate eight-byte load.
        #[inline(always)]
        fn large(v: f64) -> bool {
            (v.to_bits() >> 52) & 0x7ff >= 1355 && !(v.is_finite() && v.abs() <= 1e100)
        }
        match self.shape {
            Shape::Number(n) => {
                large(self.data[0])
                    || (n > 1 && large(self.data[1]))
                    || (n > 2 && large(self.data[2]))
                    || !self.width.is_finite()
            }
            _ => false,
        }
    }
    fn from_json(value: &serde_json::Value) -> Self {
        if let Some(v) = value.as_bool() {
            Self::Bool(v)
        } else if let Some(v) = value.as_f64() {
            Self::scalar(v)
        } else {
            Self::vector(
                &value
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect::<Vec<_>>(),
                0.,
            )
        }
    }
}

/// Per-evaluator working state reused across samples, so a sample allocates nothing.
#[derive(Default)]
pub(super) struct Scratch<'a> {
    /// Fields evaluated for the current sample, by field index.
    fields: Vec<Option<Value<'a>>>,
    /// Source images already resolved for a naming literal, with their mip pyramid once a
    /// filtered sample needed it. Looking these up per sample costs a map search and a lock.
    images: Vec<ResolvedImage<'a>>,
}
struct ResolvedImage<'a> {
    id: &'a str,
    image: &'a std::sync::Arc<Image>,
    pyramid: Option<std::sync::Arc<images::Pyramid>>,
}

pub(super) fn validate_inputs(
    program: &Program,
    inputs: &Inputs,
) -> Result<Vec<Value<'static>>, ProgramDiagnostic> {
    let root = Mark::root(&program.sources.entry);
    for name in inputs.parameters.keys() {
        if !program.metadata.parameters.iter().any(|p| p.name == *name) {
            return Err(program.diagnostic(
                "MS_PARAMETER",
                format!("Unknown public parameter '{name}'."),
                &root,
            ));
        }
    }
    let mut values = Vec::with_capacity(program.metadata.parameters.len());
    for parameter in &program.metadata.parameters {
        let value = inputs
            .parameters
            .get(&parameter.name)
            .unwrap_or(&parameter.default);
        let range = parameter.min.zip(parameter.max).map(|(lo, hi)| [lo, hi]);
        if !check::parameter_valid(parameter.kind, value, range) {
            return Err(ProgramDiagnostic {
                code: "MS_PARAMETER",
                message: format!(
                    "Parameter '{}' requires {:?} within its declared range.",
                    parameter.name, parameter.kind
                ),
                source: parameter.source.clone(),
            });
        }
        values.push(Value::from_json(value));
    }
    let mut pixels = 0usize;
    if inputs.images.len() > 64 || inputs.edges.len() > 512 {
        return Err(program.diagnostic(
            "MS_LIMIT",
            "At most 64 images and 512 finite source edges may be supplied.",
            &root,
        ));
    }
    for (name, image) in &inputs.images {
        let count = image
            .width
            .checked_mul(image.height)
            .filter(|n| *n > 0 && *n <= 16_777_216);
        if name.len() > 1024 || count.and_then(|n| n.checked_mul(4)) != Some(image.rgba.len()) {
            return Err(program.diagnostic(
                "MS_INPUT",
                format!(
                    "Image '{name}' needs finite bounded dimensions and complete RGBA8 pixels."
                ),
                &root,
            ));
        }
        pixels = pixels.saturating_add(count.unwrap());
    }
    if pixels > 16_777_216 {
        return Err(program.diagnostic(
            "MS_LIMIT",
            "Source images exceed 16 million total pixels.",
            &root,
        ));
    }
    if inputs
        .edges
        .iter()
        .flat_map(|e| e.start.into_iter().chain(e.end))
        .any(|v| !v.is_finite() || v.abs() > 1e12)
    {
        return Err(program.diagnostic(
            "MS_INPUT",
            "Source edges must have finite world coordinates.",
            &root,
        ));
    }
    validate_features(program, &inputs.features)?;
    Ok(values)
}
/// Receiver features are bounded and finite, like source edges.
pub(super) fn validate_features(
    program: &Program,
    features: &[Feature],
) -> Result<(), ProgramDiagnostic> {
    let root = Mark::root(&program.sources.entry);
    if features.len() > super::MAX_FEATURES {
        return Err(program.diagnostic(
            "MS_LIMIT",
            format!(
                "At most {} receiver features may be supplied.",
                super::MAX_FEATURES
            ),
            &root,
        ));
    }
    if features.iter().any(|f| {
        f.start
            .into_iter()
            .chain(f.end)
            .any(|v| !v.is_finite() || v.abs() > 1e12)
            || !f.angle.is_finite()
    }) {
        return Err(program.diagnostic(
            "MS_INPUT",
            "Receiver features must have finite world coordinates and angles.",
            &root,
        ));
    }
    Ok(())
}
/// The JSON value of a constant expression, such as a parameter default.
pub(super) fn constant(
    program: &Program,
    expr: &Expr,
) -> Result<serde_json::Value, ProgramDiagnostic> {
    let inputs = Inputs::default();
    let sample = Sample::default();
    let mut remaining = 16_384;
    let node = super::compiled::constant(expr);
    let compiled = super::compiled::Compiled::default();
    let mut scratch = Scratch::default();
    Context {
        program,
        compiled: &compiled,
        inputs: &inputs,
        features: &[],
        index: &features::Index::default(),
        sample: &sample,
        parameters: &[],
        scratch: &mut scratch,
        steps: 0,
        depth: 0,
        remaining: &mut remaining,
    }
    .value(&node)
    .map(|value| value.json())
    .map_err(|error| *error)
}
pub(super) fn evaluate(
    program: &Program,
    inputs: &Inputs,
    sample: &Sample,
) -> Result<Outputs, ProgramDiagnostic> {
    let parameters = validate_inputs(program, inputs)?;
    evaluate_wanted(
        program,
        &program.compiled,
        inputs,
        &inputs.features,
        &features::Index::new(&inputs.features),
        sample,
        &parameters,
        &mut 16_384,
        &mut Scratch::default(),
        Want::All,
        OutputSet::ALL,
    )
}
/// Which outputs one evaluation computes. A bake of one map evaluates only the outputs that map
/// reads; the others keep their defaults in the returned [`Outputs`]. Every output computed has the
/// value it has when all are computed, and is checked identically.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputSet(u16);
impl OutputSet {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0xff);
    pub const BASE_COLOR: Self = Self(1);
    pub const ROUGHNESS: Self = Self(2);
    pub const METALNESS: Self = Self(4);
    pub const HEIGHT: Self = Self(8);
    pub const EMISSIVE: Self = Self(16);
    pub const OPACITY: Self = Self(32);
    pub const RECEIVER_NORMAL: Self = Self(64);
    pub const NORMAL_OFFSET: Self = Self(128);
    /// The set of one output by its declared name; unknown names are in no set.
    pub fn named(name: &str) -> Self {
        match name {
            "base_color" => Self::BASE_COLOR,
            "roughness" => Self::ROUGHNESS,
            "metalness" => Self::METALNESS,
            "height" => Self::HEIGHT,
            "emissive" => Self::EMISSIVE,
            "opacity" => Self::OPACITY,
            "receiver_normal" => Self::RECEIVER_NORMAL,
            "normal_offset" => Self::NORMAL_OFFSET,
            _ => Self::NONE,
        }
    }
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}
/// Canonical bytes of the inputs a set of dependencies reads; see [`Program::input_fingerprint`].
pub(super) fn fingerprint(
    program: &Program,
    inputs: &Inputs,
    dependencies: &super::deps::Dependencies,
) -> Result<Vec<u8>, ProgramDiagnostic> {
    let values = validate_inputs(program, inputs)?;
    let mut bytes = Vec::new();
    for &index in &dependencies.parameters {
        bytes.extend((index as u32).to_le_bytes());
        match &values[index].shape {
            Shape::Number(n) => {
                bytes.push(*n as u8);
                for component in &values[index].data[..*n] {
                    bytes.extend(component.to_bits().to_le_bytes());
                }
            }
            Shape::Bool(value) => bytes.extend([4, u8::from(*value)]),
            Shape::Text(_) => unreachable!("parameters are numbers or booleans"),
        }
    }
    if dependencies.seed {
        bytes.extend(inputs.seed.to_le_bytes());
    }
    if dependencies.edges {
        bytes.extend((inputs.edges.len() as u64).to_le_bytes());
        for edge in &inputs.edges {
            for component in edge.start.iter().chain(&edge.end) {
                bytes.extend(component.to_bits().to_le_bytes());
            }
        }
    }
    Ok(bytes)
}
/// Fields chosen once by name for many samples: `(field index, caller slot)` in field order,
/// which is the order the evaluator has always computed wanted fields in.
#[derive(Clone, Debug, Default)]
pub struct FieldSelection(pub(super) Vec<(usize, usize)>);
impl FieldSelection {
    /// Unknown names are an error, like indexing the named results would be.
    pub(super) fn new(program: &Program, names: &[&str]) -> Result<Self, String> {
        let mut picked = Vec::with_capacity(names.len());
        for (slot, name) in names.iter().enumerate() {
            let index = program
                .metadata
                .fields
                .iter()
                .position(|field| field.name == *name)
                .ok_or_else(|| format!("Unknown field {name}."))?;
            picked.push((index, slot));
        }
        picked.sort_unstable();
        Ok(Self(picked))
    }
    /// How many fields were selected, which is the length of the values slice to supply.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
/// Which results one evaluation needs. Every mode computes identical values for what it returns.
pub(super) enum Want<'w> {
    /// Every output and every inspectable field.
    All,
    /// Every output, plus the named fields.
    Channels(&'w [&'w str]),
    /// Every output, plus the selected fields written to `values` by caller slot instead of
    /// into the named map, so a sample allocates nothing.
    Selected(&'w FieldSelection, &'w mut [FieldValue]),
    /// Only the height output; other channels keep their defaults.
    Height,
}
/// The checks every sample passes before it is evaluated.
pub(super) fn validate_sample(program: &Program, sample: &Sample) -> Result<(), ProgramDiagnostic> {
    let root = Mark::root(&program.sources.entry);
    if sample
        .uv
        .iter()
        .chain(&sample.surface)
        .chain(&sample.object)
        .chain(&sample.world)
        .chain(&sample.normal)
        .chain(&sample.gravity)
        .chain(&sample.bounds_min)
        .chain(&sample.bounds_max)
        .any(|v| !v.is_finite() || v.abs() > 1e12)
        || !sample.footprint.is_finite()
        || sample.footprint < 0.
        || sample.footprint > 1e9
        || sample
            .uv_footprint
            .is_some_and(|v| !v.is_finite() || !(0. ..=1e9).contains(&v))
        || length(sample.normal) < 1e-12
        || length(sample.gravity) < 1e-12
    {
        return Err(program.diagnostic(
            "MS_INPUT",
            "Samples require finite coordinates, directions and a nonnegative footprint in meters.",
            &root,
        ));
    }
    Ok(())
}
/// Checks an evaluated output and stores it in `result`.
pub(super) fn store_output(
    program: &Program,
    name: &str,
    mark: &Mark,
    value: &Value<'_>,
    result: &mut Outputs,
) -> Result<(), ProgramDiagnostic> {
    if name == "receiver_normal" {
        let v = value.data().0;
        let length = v.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !(length.is_finite() && length > 1e-12) {
            return Err(program.diagnostic(
                "MS_OUTPUT",
                "Output receiver_normal must be a finite nonzero direction.",
                mark,
            ));
        }
        result.receiver_normal = Some(v.map(|v| v / length));
        return Ok(());
    }
    if name == "normal_offset" {
        let v = value.data().0;
        if v.iter().any(|v| !v.is_finite() || v.abs() > 1e3) {
            return Err(program.diagnostic(
                "MS_OUTPUT",
                "Output normal_offset must be a finite vector of moderate length.",
                mark,
            ));
        }
        result.normal_offset = Some(v);
        return Ok(());
    }
    match name {
        "base_color" => result.base_color = value.data().0,
        "emissive" => result.emissive = value.data().0,
        "roughness" => result.roughness = value.first(),
        "metalness" => result.metalness = value.first(),
        "height" => result.height = value.first(),
        "opacity" => result.opacity = value.first(),
        _ => unreachable!(),
    }
    let valid = if name == "height" {
        value.first().abs() <= 1e6
    } else if name == "emissive" {
        value.data().0.iter().all(|v| *v >= 0. && *v <= 1e6)
    } else {
        let (v, n, _) = value.data();
        v[..n].iter().all(|v| (0.0..=1.0).contains(v))
    };
    if !valid {
        return Err(program.diagnostic(
            "MS_OUTPUT",
            format!("Output {name} is outside its supported channel range."),
            mark,
        ));
    }
    Ok(())
}
/// Whether two evaluated values are equal: booleans by value, numbers by size and components.
pub(super) fn equal(a: &Value<'_>, b: &Value<'_>) -> bool {
    match (a.shape, b.shape) {
        (Shape::Bool(a), Shape::Bool(b)) => a == b,
        _ => {
            let (a, an, _) = a.data();
            let (b, bn, _) = b.data();
            an == bn && a[..an] == b[..bn]
        }
    }
}
pub(super) fn evaluate_wanted<'a>(
    program: &'a Program,
    compiled: &'a super::compiled::Compiled,
    inputs: &'a Inputs,
    features: &'a [Feature],
    index: &features::Index,
    sample: &Sample,
    parameters: &[Value<'a>],
    remaining: &mut usize,
    scratch: &mut Scratch<'a>,
    mut want: Want<'_>,
    outputs: OutputSet,
) -> Result<Outputs, ProgramDiagnostic> {
    validate_sample(program, sample)?;
    scratch.fields.clear();
    scratch.fields.resize(compiled.fields.len(), None);
    let mut context = Context {
        program,
        compiled,
        inputs,
        features,
        index,
        sample,
        parameters,
        scratch,
        steps: 0,
        depth: 0,
        remaining,
    };
    let mut result = Outputs {
        base_color: [0.; 3],
        roughness: 1.,
        metalness: 0.,
        height: 0.,
        emissive: [0.; 3],
        opacity: 1.,
        receiver_normal: None,
        normal_offset: None,
        fields: BTreeMap::new(),
    };
    for (name, node) in &compiled.outputs {
        if (matches!(want, Want::Height) && name != "height")
            || !outputs.contains(OutputSet::named(name))
        {
            continue;
        }
        let value = context.value(node).map_err(|error| *error)?;
        store_output(program, name, &node.mark, &value, &mut result)?;
    }
    match &mut want {
        Want::All => {
            for (index, field) in program.metadata.fields.iter().enumerate() {
                let value = context.field_value(index).map_err(|error| *error)?;
                result.fields.insert(field.name.clone(), value.field());
            }
        }
        Want::Channels(names) => {
            // Bakes usually want no field; skip comparing every field's name per sample.
            let fields = if names.is_empty() {
                &[][..]
            } else {
                &program.metadata.fields[..]
            };
            for (index, field) in fields.iter().enumerate() {
                if names.contains(&field.name.as_str()) {
                    let value = context.field_value(index).map_err(|error| *error)?;
                    result.fields.insert(field.name.clone(), value.field());
                }
            }
        }
        Want::Selected(selection, values) => {
            for &(index, slot) in &selection.0 {
                values[slot] = context.field_value(index).map_err(|error| *error)?.field();
            }
        }
        Want::Height => {}
    }
    Ok(result)
}
pub(super) struct Context<'a, 'b> {
    program: &'a Program,
    compiled: &'a super::compiled::Compiled,
    inputs: &'a Inputs,
    features: &'b [Feature],
    index: &'b features::Index,
    sample: &'b Sample,
    /// Validated public parameters in metadata order, shared by every sample of a batch.
    parameters: &'b [Value<'a>],
    scratch: &'b mut Scratch<'a>,
    steps: usize,
    depth: usize,
    remaining: &'b mut usize,
}
/// Nodes write their value into a slot the caller provides and return only success or the rare
/// boxed error. Returning the 48-byte value instead made every node build it in a temporary and
/// copy it out, and reading that copy back right after the narrower writes stalled the processor.
type Done = Result<(), Box<ProgramDiagnostic>>;
impl<'a, 'b> Context<'a, 'b> {
    /// A context for evaluating builtins of one sample at a time without step accounting, for the
    /// lane evaluator, which bounds steps before it runs.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lane(
        program: &'a Program,
        compiled: &'a super::compiled::Compiled,
        inputs: &'a Inputs,
        features: &'b [Feature],
        index: &'b features::Index,
        sample: &'b Sample,
        parameters: &'b [Value<'a>],
        scratch: &'b mut Scratch<'a>,
        remaining: &'b mut usize,
    ) -> Self {
        Self {
            program,
            compiled,
            inputs,
            features,
            index,
            sample,
            parameters,
            scratch,
            steps: 0,
            depth: 0,
            remaining,
        }
    }
    pub(super) fn set_sample(&mut self, sample: &'b Sample) {
        self.sample = sample;
    }
}
impl<'a> Context<'a, '_> {
    /// Charges a feature query for the features it visited: one operation per query plus one per
    /// 16 features toward the sample's step budget, and eight per feature toward the batch's.
    fn charge_features(&mut self, visited: usize, mark: &Mark) -> Done {
        let work = visited * 8;
        self.steps += 1 + visited / 16;
        if *self.remaining < work {
            Err(self.program.diagnostic(
                "MS_LIMIT",
                "Material sample batch exceeds its operation allowance.",
                mark,
            ))?;
        }
        *self.remaining -= work;
        Ok(())
    }
    /// Evaluates call arguments in order without a heap allocation for the common small arity.
    fn with_arguments(
        &mut self,
        args: &'a [Node],
        locals: &[Value<'a>],
        call: impl FnOnce(&mut Self, &[Value<'a>]) -> Done,
    ) -> Done {
        const INLINE: usize = 8;
        // Exact-size argument arrays for the common arities avoid initializing unused slots.
        match args {
            [a] => {
                let mut values = [Value::EMPTY; 1];
                self.expression(a, locals, &mut values[0])?;
                return call(self, &values);
            }
            [a, b] => {
                let mut values = [Value::EMPTY; 2];
                self.expression(a, locals, &mut values[0])?;
                self.expression(b, locals, &mut values[1])?;
                return call(self, &values);
            }
            [a, b, c] => {
                let mut values = [Value::EMPTY; 3];
                self.expression(a, locals, &mut values[0])?;
                self.expression(b, locals, &mut values[1])?;
                self.expression(c, locals, &mut values[2])?;
                return call(self, &values);
            }
            _ => {}
        }
        if args.len() <= INLINE {
            let mut values = [Value::EMPTY; INLINE];
            for (slot, arg) in values.iter_mut().zip(args) {
                self.expression(arg, locals, slot)?;
            }
            call(self, &values[..args.len()])
        } else {
            let mut values = vec![Value::EMPTY; args.len()];
            for (slot, arg) in values.iter_mut().zip(args) {
                self.expression(arg, locals, slot)?;
            }
            call(self, &values)
        }
    }
    /// One node's value, for callers outside the recursive evaluation.
    fn value(&mut self, node: &'a Node) -> Result<Value<'a>, Box<ProgramDiagnostic>> {
        let mut value = Value::EMPTY;
        self.expression(node, &[], &mut value)?;
        Ok(value)
    }
    fn field_value(&mut self, index: usize) -> Result<Value<'a>, Box<ProgramDiagnostic>> {
        let mut value = Value::EMPTY;
        self.field(index, &mut value)?;
        Ok(value)
    }
    fn field(&mut self, index: usize, out: &mut Value<'a>) -> Done {
        if let Some(Some(value)) = self.scratch.fields.get(index) {
            *out = *value;
            return Ok(());
        }
        let compiled = self.compiled;
        self.expression(&compiled.fields[index], &[], out)?;
        self.scratch.fields[index] = Some(*out);
        Ok(())
    }
    /// The image a literal names, and its mip pyramid when `filtered`; resolved once per evaluator.
    fn image(
        &mut self,
        id: &'a str,
        filtered: bool,
    ) -> Option<(&'a Image, Option<&images::Pyramid>)> {
        let found = self
            .scratch
            .images
            .iter()
            .position(|entry| std::ptr::eq(entry.id, id));
        let index = match found {
            Some(index) => index,
            None => {
                let image = self.inputs.images.get(id)?;
                self.scratch.images.push(ResolvedImage {
                    id,
                    image,
                    pyramid: None,
                });
                self.scratch.images.len() - 1
            }
        };
        let entry = &mut self.scratch.images[index];
        if filtered && entry.pyramid.is_none() {
            entry.pyramid = Some(self.inputs.mips.pyramid(entry.image));
        }
        Some((entry.image.as_ref(), entry.pyramid.as_deref()))
    }
    /// Evaluates one node. Literals, helper arguments and fields already computed for this sample
    /// are most of all nodes and need only the step accounting, so they return here without
    /// entering the general evaluator. The accounting and results are those of the general path,
    /// which also handles every budget failure.
    #[inline(always)]
    fn expression(&mut self, node: &'a Node, locals: &[Value<'a>], out: &mut Value<'a>) -> Done {
        if *self.remaining > 0 && self.steps < 16_384 && self.depth < 128 {
            let leaf = match &node.kind {
                // Larger or non-finite literals take the general path, which rejects them.
                Kind::Number(n) if n.abs() <= 1e100 => {
                    *out = Value::scalar(*n);
                    true
                }
                Kind::Local(index) => {
                    *out = locals[*index];
                    true
                }
                Kind::Field(index) => match self.scratch.fields.get(*index) {
                    Some(Some(value)) => {
                        *out = *value;
                        true
                    }
                    _ => false,
                },
                _ => false,
            };
            if leaf {
                self.steps += 1;
                *self.remaining -= 1;
                return Ok(());
            }
        }
        self.expression_general(node, locals, out)
    }
    #[inline(never)]
    fn expression_general(
        &mut self,
        node: &'a Node,
        locals: &[Value<'a>],
        out: &mut Value<'a>,
    ) -> Done {
        self.steps += 1;
        self.depth += 1;
        if *self.remaining == 0 {
            Err(self.program.diagnostic(
                "MS_LIMIT",
                "Material sample batch exceeds its operation allowance.",
                &node.mark,
            ))?;
        }
        *self.remaining -= 1;
        if self.steps > 16_384 || self.depth > 128 {
            Err(self.program.diagnostic(
                "MS_LIMIT",
                "Material evaluation exceeds its operation or call-depth budget.",
                &node.mark,
            ))?;
        }
        // Arms that forward another node's value have already written the slot and leave here.
        'written: {
            match &node.kind {
                Kind::Number(n) => *out = Value::scalar(*n),
                Kind::Bool(b) => *out = Value::Bool(*b),
                Kind::Text(s) => *out = Value::Text(s),
                Kind::Local(index) => *out = locals[*index],
                Kind::Input(input) => {
                    let sample = self.sample;
                    *out = match input {
                        Input::Uv => Value::vector(&sample.uv, sample.uv_footprint.unwrap_or(0.)),
                        Input::Surface => Value::vector(&sample.surface, sample.footprint),
                        Input::Object => Value::vector(&sample.object, sample.footprint),
                        Input::World => Value::vector(&sample.world, sample.footprint),
                        Input::Normal => Value::vector(&sample.normal, 0.),
                        Input::Gravity => Value::vector(&sample.gravity, 0.),
                        Input::BoundsMin => Value::vector(&sample.bounds_min, 0.),
                        Input::BoundsMax => Value::vector(&sample.bounds_max, 0.),
                        Input::Footprint => Value::scalar(sample.footprint),
                        Input::UvFootprint => Value::scalar(sample.uv_footprint.unwrap_or(0.)),
                    }
                }
                Kind::Parameter(index) => *out = self.parameters[*index],
                Kind::Field(index) => {
                    self.field(*index, out)?;
                    break 'written;
                }
                Kind::Unary(op, operand) => {
                    let mut a = Value::EMPTY;
                    self.expression(operand, locals, &mut a)?;
                    if *op == Op::Not {
                        *out = Value::Bool(!a.boolean());
                    } else {
                        let (v, n, w) = a.data();
                        *out = Value::Number(v.map(|v| if *op == Op::Sub { -v } else { v }), n, w);
                    }
                }
                Kind::Binary(op, left, right) => {
                    let mut a = Value::EMPTY;
                    self.expression(left, locals, &mut a)?;
                    if *op == Op::And && !a.boolean() {
                        *out = Value::Bool(false);
                    } else if *op == Op::Or && a.boolean() {
                        *out = Value::Bool(true);
                    } else {
                        let mut b = Value::EMPTY;
                        self.expression(right, locals, &mut b)?;
                        match *op {
                            Op::And => *out = Value::Bool(a.boolean() && b.boolean()),
                            Op::Or => *out = Value::Bool(a.boolean() || b.boolean()),
                            Op::Equal | Op::NotEqual => {
                                let equal = equal(&a, &b);
                                *out = Value::Bool(if *op == Op::Equal { equal } else { !equal });
                            }
                            Op::Less => *out = Value::Bool(a.first() < b.first()),
                            Op::Greater => *out = Value::Bool(a.first() > b.first()),
                            Op::LessEqual => *out = Value::Bool(a.first() <= b.first()),
                            Op::GreaterEqual => *out = Value::Bool(a.first() >= b.first()),
                            _ => numeric_binary(*op, &a, &b, out),
                        }
                    }
                }
                Kind::Member(value, components) => {
                    let mut whole = Value::EMPTY;
                    self.expression(value, locals, &mut whole)?;
                    let (value, _, w) = whole.data();
                    let mut picked = [0.; 3];
                    for (slot, component) in picked.iter_mut().zip(components) {
                        *slot = value[*component];
                    }
                    *out = Value::Number(picked, components.len(), w);
                }
                Kind::Select(condition, yes, no) => {
                    let mut chosen = Value::EMPTY;
                    self.expression(condition, locals, &mut chosen)?;
                    self.expression(if chosen.boolean() { yes } else { no }, locals, out)?;
                    break 'written;
                }
                Kind::Function(index, args) => {
                    let compiled = self.compiled;
                    let body = &compiled.functions[*index].body;
                    self.with_arguments(args, locals, |context, values| {
                        context.expression(body, values, out)
                    })?;
                    break 'written;
                }
                Kind::Builtin(name, args) => {
                    self.with_arguments(args, locals, |context, values| {
                        context.builtin(*name, values, &node.mark, out)
                    })?;
                    break 'written;
                }
            };
        }
        self.depth -= 1;
        if self.steps > 16_384 {
            Err(self.program.diagnostic(
                "MS_LIMIT",
                "Material evaluation exceeds its operation budget.",
                &node.mark,
            ))?;
        }
        if out.unsound() {
            Err(self.program.diagnostic(
                "MS_NONFINITE",
                "Expression produced a non-finite value or invalid arithmetic.",
                &node.mark,
            ))?;
        }
        Ok(())
    }
    /// Kept out of line: inlined into the recursive evaluator, its many locals would make every
    /// node pay for one large stack frame.
    #[inline(never)]
    pub(super) fn builtin(
        &mut self,
        name: Builtin,
        a: &[Value<'a>],
        mark: &Mark,
        out: &mut Value<'a>,
    ) -> Done {
        use Builtin as B;
        let error = |message: &str| self.program.diagnostic("MS_INPUT", message, mark);
        match name {
            B::Vec2 | B::Vec3 => {
                let size = if name == B::Vec2 { 2 } else { 3 };
                if a.len() == 1 {
                    let (v, n, w) = a[0].data();
                    if n == 1 {
                        *out = Value::Number([v[0]; 3], size, w);
                    } else {
                        *out = a[0];
                    }
                } else {
                    let mut data = [0.; 3];
                    let mut count = 0;
                    let mut width: f64 = 0.;
                    for arg in a {
                        let (v, n, w) = arg.data();
                        data[count..count + n].copy_from_slice(&v[..n]);
                        count += n;
                        width = width.max(w);
                    }
                    *out = Value::Number(data, count, width);
                }
            }
            B::Mix | B::Clamp | B::Smoothstep => {
                let datas = [a[0].data(), a[1].data(), a[2].data()];
                let n = if name == B::Smoothstep {
                    datas[2].1
                } else {
                    datas[0].1
                };
                let mut result = [0.; 3];
                for i in 0..n {
                    let get = |j: usize| datas[j].0[if datas[j].1 == 1 { 0 } else { i }];
                    let (x, y, z) = (get(0), get(1), get(2));
                    result[i] = match name {
                        B::Mix => x + (y - x) * z,
                        B::Clamp => {
                            if y > z {
                                Err(error("Clamp minimum exceeds maximum."))?;
                            }
                            x.clamp(y, z)
                        }
                        _ => {
                            if y <= x {
                                Err(error("Smoothstep needs increasing bounds."))?;
                            }
                            let t = ((z - x) / (y - x)).clamp(0., 1.);
                            t * t * (3. - 2. * t)
                        }
                    };
                }
                *out = Value::Number(result, n, datas.iter().map(|d| d.2).sum());
            }
            B::Abs | B::Floor | B::Fract | B::Sin | B::Cos | B::Sqrt => {
                let (v, n, w) = a[0].data();
                let apply: fn(f64) -> f64 = match name {
                    B::Abs => f64::abs,
                    B::Floor => f64::floor,
                    B::Fract => |x| x - x.floor(),
                    B::Sin => sin,
                    B::Cos => cos,
                    B::Sqrt => f64::sqrt,
                    _ => unreachable!(),
                };
                *out = Value::Number(v.map(apply), n, w);
            }
            B::Min | B::Max | B::Step => {
                let (v, n, w) = a[0].data();
                let (b, bn, bw) = a[1].data();
                let size = n.max(bn);
                let mut result = [0.; 3];
                for i in 0..size {
                    let x = v[if n == 1 { 0 } else { i }];
                    let y = b[if bn == 1 { 0 } else { i }];
                    result[i] = match name {
                        B::Min => x.min(y),
                        B::Max => x.max(y),
                        _ => {
                            if y < x {
                                0.
                            } else {
                                1.
                            }
                        }
                    };
                }
                *out = Value::Number(result, size, w + bw);
            }
            B::ValueNoise => {
                let (p, n, w) = a[0].data();
                let seed = a[1].first();
                let raw = if n == 3 {
                    super::super::noise::value_noise3(p[0], p[1], p[2], seed)
                } else {
                    super::super::noise::value_noise(p[0], if n == 2 { p[1] } else { 0. }, seed)
                };
                // Like gradient noise, detail at or below the footprint fades toward the mean. A
                // smoothstep-faded lattice value changes by at most 1.5 per unit along an axis, which
                // bounds its variation across the sample like a screen-space derivative would.
                let filter = ((w - 0.25) / 0.75).clamp(0., 1.);
                *out = Value::Number(
                    [raw * (1. - filter) + 0.5 * filter, 0., 0.],
                    1,
                    (1. - filter) * w * 1.5,
                );
            }
            B::Hash => {
                let (p, n, _) = a[0].data();
                let seed = a[1].first();
                let cell = p.map(f64::floor);
                let z = if n == 3 { cell[2] } else { 0. };
                let y = if n >= 2 { cell[1] } else { 0. };
                *out = Value::scalar(hash(cell[0], y, z, seed));
            }
            B::Dot => {
                let (v, n, w) = a[0].data();
                let (b, _, bw) = a[1].data();
                *out = Value::Number(
                    [v[..n].iter().zip(b).map(|(a, b)| a * b).sum(), 0., 0.],
                    1,
                    w * max_abs(b, n) + bw * max_abs(v, n),
                );
            }
            B::Cross => {
                let (u, _, uw) = a[0].data();
                let (v, _, vw) = a[1].data();
                let product = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                // Each component changes by at most |u| times v's variation plus |v| times u's.
                *out = Value::Number(product, 3, uw * length(v) + vw * length(u));
            }
            B::Length | B::Normalize => {
                let (v, n, w) = a[0].data();
                let len = v[..n].iter().map(|v| v * v).sum::<f64>().sqrt();
                if name == B::Length {
                    *out = Value::Number([len, 0., 0.], 1, w);
                } else {
                    if len < 1e-12 {
                        Err(error("Cannot normalize a zero vector."))?;
                    }
                    *out = Value::Number(v.map(|v| v / len), n, w / len);
                }
            }
            B::Pow => {
                let (v, n, w) = a[0].data();
                let exponent = a[1].first();
                if v[..n].iter().any(|v| *v < 0.) || !exponent.is_finite() {
                    Err(error("pow needs nonnegative bases and a finite exponent."))?;
                }
                let slope = v[..n]
                    .iter()
                    .map(|v| (exponent * super::numeric::pow(v.max(1e-9), exponent - 1.)).abs())
                    .fold(0., f64::max);
                *out = Value::Number(
                    v.map(|v| super::numeric::pow(v, exponent)),
                    n,
                    (w * slope).min(1e9),
                );
            }
            B::Exp => {
                let (v, n, w) = a[0].data();
                if v[..n].iter().any(|v| *v > 200.) {
                    Err(error("exp arguments above 200 overflow."))?;
                }
                let largest = v[..n]
                    .iter()
                    .map(|v| super::numeric::exp(*v))
                    .fold(0., f64::max);
                *out = Value::Number(v.map(super::numeric::exp), n, (w * largest).min(1e9));
            }
            B::Seed => {
                *out = Value::scalar(f64::from(seed_value(self.inputs.seed, a[0].text())));
            }
            B::Noise | B::Fbm | B::Cellular => {
                let (v, n, w) = a[0].data();
                let seed = a[1].first();
                let count = if name == B::Fbm {
                    let count = a[2].first();
                    if count.fract() != 0. || !(1.0..=12.0).contains(&count) {
                        Err(error(
                            "Fractal noise needs an integer octave count from 1 to 12.",
                        ))?;
                    }
                    count as usize
                } else {
                    1
                };
                let mut sum = 0.;
                let mut total = 0.;
                let mut frequency = 1.;
                let mut amplitude = 1.;
                let mut variation = 0.;
                for octave in 0..count {
                    let work = if name == B::Cellular { 27 } else { 8 };
                    self.steps += work;
                    if *self.remaining < work {
                        Err(self.program.diagnostic(
                            "MS_LIMIT",
                            "Material sample batch exceeds its operation allowance.",
                            mark,
                        ))?;
                    }
                    *self.remaining -= work;
                    let p = v.map(|v| v * frequency);
                    let seed = seed + octave as f64 * 131.;
                    let raw = if name == B::Cellular {
                        cellular(p, n, seed)
                    } else {
                        gradient(p, n, seed)
                    };
                    let filter = ((w * frequency - 0.25) / 0.75).clamp(0., 1.);
                    sum += amplitude * (raw * (1. - filter) + 0.5 * filter);
                    variation += amplitude * (1. - filter) * w * frequency * 6.;
                    total += amplitude;
                    amplitude *= 0.5;
                    frequency *= 2.;
                }
                *out = Value::Number([sum / total, 0., 0.], 1, variation / total);
            }
            B::Scale => numeric_binary(Op::Mul, &a[0], &a[1], out),
            B::Offset | B::Warp => numeric_binary(Op::Add, &a[0], &a[1], out),
            B::Repeat => {
                let (v, n, w) = a[0].data();
                let (period, pn, _) = a[1].data();
                let mut result = [0.; 3];
                for i in 0..n {
                    let size = period[if pn == 1 { 0 } else { i }];
                    if size <= 0. {
                        Err(error("Repeat periods must be positive."))?;
                    }
                    result[i] = v[i] - size * (v[i] / size).floor();
                }
                *out = Value::Number(result, n, w);
            }
            B::Rotate => {
                let (v, _, w) = a[0].data();
                let angle = a[1].first();
                let (s, c) = (sin(angle), cos(angle));
                *out = Value::vector(
                    &[v[0] * c - v[1] * s, v[0] * s + v[1] * c],
                    w + a[1].data().2 * (v[0].abs() + v[1].abs()),
                );
            }
            B::SampleColor | B::SampleData | B::SampleAlpha => {
                let id = a[0].text();
                let Some((image, _)) = self.image(id, false) else {
                    return Err(Box::new(error(&format!(
                        "Source image '{id}' was not supplied."
                    ))));
                };
                let uv = a[1].data().0;
                let sampled = image_sample(image, [uv[0], uv[1]], name == B::SampleColor);
                if name == B::SampleAlpha {
                    *out = Value::scalar(sampled[3]);
                } else {
                    *out = Value::vector(&sampled[..3], 0.);
                }
            }
            B::Runoff => {
                let work = self.inputs.edges.len() * 16;
                self.steps += work;
                if *self.remaining < work {
                    Err(self.program.diagnostic(
                        "MS_LIMIT",
                        "Material sample batch exceeds its operation allowance.",
                        mark,
                    ))?;
                }
                *self.remaining -= work;
                let width = a[0].first();
                let extent = a[1].first();
                if width <= 0. || extent <= 0. {
                    Err(error(
                        "Runoff width and downhill length must be positive meters.",
                    ))?;
                }
                *out = Value::scalar(runoff(self.sample, &self.inputs.edges, width, extent));
            }
            B::CreaseDistance => {
                let selector = features::Selector::named(a[0].text());
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let (distance, visited) = features::crease_distance(
                    self.index,
                    self.features,
                    self.sample.world,
                    selector,
                    min_angle,
                    limit,
                );
                self.charge_features(visited, mark)?;
                // Distance changes one meter per meter, so it carries the sample's footprint.
                *out = Value::Number([distance, 0., 0.], 1, self.sample.footprint);
            }
            B::CreaseNormal => {
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let (normal, visited) = features::crease_normal(
                    self.index,
                    self.features,
                    self.sample.world,
                    self.sample.normal,
                    features::Selector::named(a[0].text()),
                    min_angle,
                    limit,
                );
                self.charge_features(visited, mark)?;
                *out = Value::vector(&normal, 0.);
            }
            B::CreaseTangent | B::CreaseAdjacentNormal => {
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let selector = features::Selector::named(a[0].text());
                let (direction, visited) = if name == B::CreaseTangent {
                    features::crease_tangent(
                        self.index,
                        self.features,
                        self.sample.world,
                        selector,
                        min_angle,
                        limit,
                    )
                } else {
                    features::crease_adjacent_normal(
                        self.index,
                        self.features,
                        self.sample.world,
                        self.sample.normal,
                        selector,
                        min_angle,
                        limit,
                    )
                };
                self.charge_features(visited, mark)?;
                *out = Value::vector(&direction, 0.);
            }
            B::AdjustNormal => {
                let unit = |value: &Value| {
                    let v = value.data().0;
                    let length = v.iter().map(|v| v * v).sum::<f64>().sqrt();
                    (length > 1e-12).then(|| v.map(|v| v / length))
                };
                let (Some(from), Some(to)) = (unit(&a[0]), unit(&a[1])) else {
                    return Err(Box::new(error(
                        "Cannot adjust toward or from a zero normal.",
                    )));
                };
                let (weight, _, weight_width) = a[2].data();
                let turned = features::slerp(from, to, weight[0].clamp(0., 1.));
                // The weight's filtering estimate bounds how far the direction varies.
                *out = Value::vector(&turned, weight_width.min(2.));
            }
            B::TriplanarWeights => {
                let (normal, _, _) = a[0].data();
                let sharpness = a[1].first();
                if !(sharpness > 0.) || !sharpness.is_finite() {
                    Err(error(
                        "Triplanar sharpness must be a positive finite exponent.",
                    ))?;
                }
                let weights = normal.map(|v| super::numeric::pow(v.abs(), sharpness));
                let total: f64 = weights.iter().sum();
                if !(total > 1e-12) {
                    Err(error("Triplanar weights need a nonzero normal."))?;
                }
                *out = Value::vector(&weights.map(|v| v / total), 0.);
            }
            B::VariedColor => {
                let id = a[0].text();
                if self.image(id, false).is_none() {
                    Err(error(&format!("Source image '{id}' was not supplied.")))?;
                }
                let (uv, _, width) = a[1].data();
                let seed = a[2].first();
                let rotation = a[3].first();
                let correction = a[4].first();
                if !rotation.is_finite() || !correction.is_finite() {
                    Err(error(
                        "Varied sampling needs a finite rotation range and correction.",
                    ))?;
                }
                // Three lattice samples, each a trilinear lookup of up to eight texels.
                let work = 3 * 16;
                self.steps += work;
                if *self.remaining < work {
                    Err(self.program.diagnostic(
                        "MS_LIMIT",
                        "Material sample batch exceeds its operation allowance.",
                        mark,
                    ))?;
                }
                *self.remaining -= work;
                let Some((image, Some(pyramid))) = self.image(id, true) else {
                    unreachable!("the image was found above");
                };
                let color = super::images::varied(
                    pyramid,
                    image,
                    [uv[0], uv[1]],
                    width,
                    seed,
                    rotation,
                    correction,
                );
                *out = Value::vector(&color, 0.);
            }
            B::Remap => {
                let (v, n, w) = a[1].data();
                let [in_lo, in_hi, out_lo, out_hi] = [2, 3, 4, 5].map(|i| a[i].first());
                if !(in_hi > in_lo) || ![in_lo, in_hi, out_lo, out_hi].iter().all(|v| v.is_finite())
                {
                    Err(error(
                        "Remap needs a finite, strictly increasing input range and a finite output range.",
                    ))?;
                }
                let clamp = a[0].text() == "clamp";
                let span = in_hi - in_lo;
                let mut result = [0.; 3];
                for i in 0..n {
                    let mut t = (v[i] - in_lo) / span;
                    if clamp {
                        t = t.clamp(0., 1.);
                    }
                    result[i] = out_lo + t * (out_hi - out_lo);
                }
                // The variation estimate scales by the slope; clamping can only reduce it.
                *out = Value::Number(result, n, w * ((out_hi - out_lo) / span).abs());
            }
            B::SoftThreshold => {
                let (v, n, w) = a[0].data();
                let threshold = a[1].first();
                let softness = a[2].first();
                if !(softness > 0.) || !softness.is_finite() || !threshold.is_finite() {
                    Err(error(
                        "A soft threshold needs a finite threshold and a positive finite softness in the value's units.",
                    ))?;
                }
                // The value's variation across the sample widens the transition (feathering). The
                // propagated estimate bounds that variation from above; half of it approximates the
                // screen-space derivative a shader would use.
                let width = softness + w * 0.5;
                let mut result = [0.; 3];
                for i in 0..n {
                    let t = ((v[i] - threshold) / width + 0.5).clamp(0., 1.);
                    result[i] = t * t * (3. - 2. * t);
                }
                *out = Value::Number(result, n, (w * 1.5 / width).min(1.));
            }
            B::DistanceBand => {
                let (d, _, w) = a[0].data();
                let inner = a[1].first();
                let outer = a[2].first();
                if !(inner >= 0.) || !(outer > inner) || !outer.is_finite() {
                    Err(error(
                        "A distance band needs ordered bounds in meters: 0 <= inner < outer.",
                    ))?;
                }
                // Half the distance's footprint softens each side of the band.
                let (lo, hi) = (inner - w * 0.5, outer + w * 0.5);
                let t = ((d[0] - lo) / (hi - lo)).clamp(0., 1.);
                *out = Value::Number(
                    [1. - t * t * (3. - 2. * t), 0., 0.],
                    1,
                    (w * 1.5 / (hi - lo)).min(1.),
                );
            }
            B::SoftUnion | B::MaskIntersect | B::MaskSubtract => {
                let (x, _, xw) = a[0].data();
                let (y, _, yw) = a[1].data();
                let (x, y) = (x[0].clamp(0., 1.), y[0].clamp(0., 1.));
                let combined = match name {
                    B::SoftUnion => 1. - (1. - x) * (1. - y),
                    B::MaskIntersect => x * y,
                    _ => x * (1. - y),
                };
                *out = Value::Number([combined, 0., 0.], 1, (xw + yw).min(1.));
            }
            B::Unknown => unreachable!("typechecked builtin"),
        };
        Ok(())
    }
}
/// The value of `seed(name)` under a request seed: a stable 32-bit hash of both.
pub fn seed_value(seed: u32, name: &str) -> u32 {
    let mut hash = 2_166_136_261u32 ^ seed;
    for byte in name.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    hash
}
pub(super) fn numeric_binary<'a>(op: Op, a: &Value<'a>, b: &Value<'a>, out: &mut Value<'a>) {
    let (a, an, aw) = a.data();
    let (b, bn, bw) = b.data();
    // Scalar arithmetic is most of a program; this is the general case below for one component.
    if an == 1 && bn == 1 {
        let (x, y) = (a[0], b[0]);
        let (value, width) = match op {
            Op::Add => (x + y, aw + bw),
            Op::Sub => (x - y, aw + bw),
            Op::Mul => (x * y, aw * y.abs() + bw * x.abs() + aw * bw),
            Op::Div => {
                let denominator = y.abs();
                (
                    x / y,
                    aw / denominator + bw * x.abs() / (denominator * denominator),
                )
            }
            Op::Rem => (x % y, aw + bw),
            _ => unreachable!(),
        };
        *out = Value::Number([value, 0., 0.], 1, width);
        return;
    }
    let n = an.max(bn);
    let mut v = [0.; 3];
    for i in 0..n {
        let x = a[if an == 1 { 0 } else { i }];
        let y = b[if bn == 1 { 0 } else { i }];
        v[i] = match op {
            Op::Add => x + y,
            Op::Sub => x - y,
            Op::Mul => x * y,
            Op::Div => x / y,
            Op::Rem => x % y,
            _ => unreachable!(),
        };
    }
    let width = match op {
        Op::Mul => aw * max_abs(b, bn) + bw * max_abs(a, an) + aw * bw,
        Op::Div => {
            let denominator = b[..bn]
                .iter()
                .map(|v| v.abs())
                .fold(f64::INFINITY, f64::min);
            aw / denominator + bw * max_abs(a, an) / (denominator * denominator)
        }
        _ => aw + bw,
    };
    *out = Value::Number(v, n, width);
}
fn max_abs(v: [f64; 3], n: usize) -> f64 {
    v[..n].iter().map(|v| v.abs()).fold(0., f64::max)
}
fn length(v: [f64; 3]) -> f64 {
    v.iter().map(|v| v * v).sum::<f64>().sqrt()
}
fn hash(x: f64, y: f64, z: f64, seed: f64) -> f64 {
    super::super::noise::lattice_hash(x + z * 151., y + z * 269., seed)
}
/// Components past dimensions are padding; constructors may leave values there, so they are
/// cleared before hashing. Lower-dimensional noise depends only on the components it names.
fn unpadded(p: [f64; 3], dimensions: usize) -> [f64; 3] {
    std::array::from_fn(|i| if i < dimensions { p[i] } else { 0. })
}
fn gradient(p: [f64; 3], dimensions: usize, seed: f64) -> f64 {
    let p = unpadded(p, dimensions);
    let base = p.map(f64::floor);
    let t = std::array::from_fn::<_, 3, _>(|i| p[i] - base[i]);
    let fade = t.map(|v| v * v * v * (v * (v * 6. - 15.) + 10.));
    let mut sum = 0.;
    for corner in 0..(1 << dimensions) {
        let mut offset = [0.; 3];
        let mut coordinate = base;
        let mut weight = 1.;
        for axis in 0..dimensions {
            let high = (corner >> axis) & 1;
            coordinate[axis] += high as f64;
            offset[axis] = t[axis] - high as f64;
            weight *= if high == 1 {
                fade[axis]
            } else {
                1. - fade[axis]
            };
        }
        let index = (hash(coordinate[0], coordinate[1], coordinate[2], seed) * 12.) as usize;
        let gradients = [
            [1., 1., 0.],
            [-1., 1., 0.],
            [1., -1., 0.],
            [-1., -1., 0.],
            [1., 0., 1.],
            [-1., 0., 1.],
            [1., 0., -1.],
            [-1., 0., -1.],
            [0., 1., 1.],
            [0., -1., 1.],
            [0., 1., -1.],
            [0., -1., -1.],
        ];
        sum += weight
            * (offset[0] * gradients[index][0]
                + offset[1] * gradients[index][1]
                + offset[2] * gradients[index][2]);
    }
    (0.5 + sum * 0.5).clamp(0., 1.)
}
fn cellular(p: [f64; 3], dimensions: usize, seed: f64) -> f64 {
    let p = unpadded(p, dimensions);
    let base = p.map(f64::floor);
    let mut nearest = f64::INFINITY;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in if dimensions == 3 { -1..=1 } else { 0..=0 } {
                let cell = [base[0] + x as f64, base[1] + y as f64, base[2] + z as f64];
                let mut distance = 0.;
                for axis in 0..dimensions {
                    let point =
                        cell[axis] + hash(cell[0], cell[1], cell[2], seed + axis as f64 * 379.);
                    distance += (point - p[axis]).powi(2);
                }
                nearest = nearest.min(distance);
            }
        }
    }
    (nearest.sqrt() / (dimensions as f64).sqrt()).clamp(0., 1.)
}
fn image_sample(image: &Image, uv: [f64; 2], color: bool) -> [f64; 4] {
    let x = uv[0] * image.width as f64 - 0.5;
    let y = (1. - uv[1]) * image.height as f64 - 0.5;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let wrap = |v: f64, size: usize| v.rem_euclid(size as f64) as usize;
    let left = wrap(x.floor(), image.width);
    let right = wrap(x.floor() + 1., image.width);
    let top = wrap(y.floor(), image.height);
    let bottom = wrap(y.floor() + 1., image.height);
    let offsets = [
        top * image.width + left,
        top * image.width + right,
        bottom * image.width + left,
        bottom * image.width + right,
    ];
    let weights = [
        (1. - fx) * (1. - fy),
        fx * (1. - fy),
        (1. - fx) * fy,
        fx * fy,
    ];
    std::array::from_fn(|channel| {
        offsets
            .iter()
            .zip(weights)
            .map(|(offset, weight)| {
                let byte = image.rgba[offset * 4 + channel];
                weight
                    * if color && channel < 3 {
                        superspec_types::color::LINEAR[byte as usize]
                    } else {
                        f64::from(byte) / 255.
                    }
            })
            .sum()
    })
}
fn runoff(sample: &Sample, edges: &[Edge], width: f64, extent: f64) -> f64 {
    let normal = sample.normal.map(|v| v / length(sample.normal));
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let gn = dot(sample.gravity, normal);
    let projected = std::array::from_fn(|i| sample.gravity[i] - normal[i] * gn);
    let magnitude = length(projected);
    if magnitude < 1e-10 {
        return 0.;
    }
    let down = projected.map(|v| v / magnitude);
    let mut coverage: f64 = 0.;
    for edge in edges {
        let segment = std::array::from_fn(|i| edge.end[i] - edge.start[i]);
        let offset = std::array::from_fn(|i| sample.world[i] - edge.start[i]);
        let along = dot(segment, down);
        let across = std::array::from_fn(|i| segment[i] - down[i] * along);
        let across_squared = dot(across, across);
        let t = if across_squared > 1e-20 {
            (dot(offset, across) / across_squared).clamp(0., 1.)
        } else {
            0.
        };
        let from_edge = std::array::from_fn(|i| offset[i] - segment[i] * t);
        let distance = dot(from_edge, down);
        if distance < 0. || distance > extent {
            continue;
        }
        let lateral = length(std::array::from_fn(|i| from_edge[i] - down[i] * distance));
        let effective_width = width + sample.footprint * 0.5;
        let cross = (1. - lateral / effective_width).clamp(0., 1.);
        let falloff = (1. - distance / extent).clamp(0., 1.);
        coverage = coverage.max(cross * cross * (3. - 2. * cross) * falloff * falloff);
    }
    coverage
}
