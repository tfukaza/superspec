use super::{
    compiled::{Input, Kind, Node},
    syntax::{Expr, Mark},
    *,
};
use superspec_geometry::math::{cos, sin};

#[derive(Clone, Debug)]
pub(super) enum Value {
    Number([f64; 3], usize, f64),
    Bool(bool),
    Text(std::sync::Arc<str>),
}
impl Value {
    fn scalar(v: f64) -> Self {
        Self::Number([v, 0., 0.], 1, 0.)
    }
    fn vector(v: &[f64], width: f64) -> Self {
        let mut data = [0.; 3];
        data[..v.len()].copy_from_slice(v);
        Self::Number(data, v.len(), width)
    }
    fn data(&self) -> ([f64; 3], usize, f64) {
        match self {
            Self::Number(v, n, w) => (*v, *n, *w),
            _ => unreachable!("checked numeric type"),
        }
    }
    fn first(&self) -> f64 {
        self.data().0[0]
    }
    fn boolean(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            _ => unreachable!("checked bool type"),
        }
    }
    fn text(&self) -> &str {
        match self {
            Self::Text(v) => v,
            _ => unreachable!("checked string type"),
        }
    }
    fn field(&self) -> FieldValue {
        match self {
            Self::Number(v, 1, _) => FieldValue::Scalar(v[0]),
            Self::Number(v, 2, _) => FieldValue::Vec2([v[0], v[1]]),
            Self::Number(v, _, _) => FieldValue::Vec3(*v),
            Self::Bool(v) => FieldValue::Bool(*v),
            Self::Text(_) => unreachable!("no string fields"),
        }
    }
    pub fn json(&self) -> serde_json::Value {
        match self {
            Self::Number(v, 1, _) => serde_json::json!(v[0]),
            Self::Number(v, n, _) => serde_json::json!(&v[..*n]),
            Self::Bool(v) => serde_json::json!(v),
            Self::Text(v) => serde_json::json!(v.as_ref()),
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

pub(super) fn validate_inputs(
    program: &Program,
    inputs: &Inputs,
) -> Result<Vec<Value>, ProgramDiagnostic> {
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
pub(super) fn constant(program: &Program, expr: &Expr) -> Result<Value, ProgramDiagnostic> {
    let inputs = Inputs::default();
    let sample = Sample::default();
    let mut remaining = 16_384;
    let node = super::compiled::constant(expr);
    let compiled = super::compiled::Compiled::default();
    Context {
        program,
        compiled: &compiled,
        inputs: &inputs,
        features: &[],
        sample: &sample,
        parameters: &[],
        fields: Vec::new(),
        steps: 0,
        depth: 0,
        remaining: &mut remaining,
    }
    .expression(&node, &[])
    .map_err(|error| *error)
}
pub(super) fn evaluate(
    program: &Program,
    inputs: &Inputs,
    sample: &Sample,
) -> Result<Outputs, ProgramDiagnostic> {
    let parameters = validate_inputs(program, inputs)?;
    evaluate_validated(
        program,
        inputs,
        &inputs.features,
        sample,
        &parameters,
        &mut 16_384,
    )
}
/// Which results one evaluation needs. Every mode computes identical values for what it returns.
#[derive(Clone, Copy)]
pub(super) enum Want<'a> {
    /// Every output and every inspectable field.
    All,
    /// Every output, plus the named fields.
    Channels(&'a [&'a str]),
    /// Only the height output; other channels keep their defaults.
    Height,
}
pub(super) fn evaluate_validated(
    program: &Program,
    inputs: &Inputs,
    features: &[Feature],
    sample: &Sample,
    parameters: &[Value],
    remaining: &mut usize,
) -> Result<Outputs, ProgramDiagnostic> {
    evaluate_wanted(
        program,
        &program.compiled,
        inputs,
        features,
        sample,
        parameters,
        remaining,
        Want::All,
    )
}
pub(super) fn evaluate_wanted(
    program: &Program,
    compiled: &super::compiled::Compiled,
    inputs: &Inputs,
    features: &[Feature],
    sample: &Sample,
    parameters: &[Value],
    remaining: &mut usize,
    want: Want<'_>,
) -> Result<Outputs, ProgramDiagnostic> {
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
    let mut context = Context {
        program,
        compiled,
        inputs,
        features,
        sample,
        parameters,
        fields: vec![None; compiled.fields.len()],
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
        if matches!(want, Want::Height) && name != "height" {
            continue;
        }
        let value = context.expression(node, &[]).map_err(|error| *error)?;
        let expr = node;
        if name == "receiver_normal" {
            let v = value.data().0;
            let length = v.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !(length.is_finite() && length > 1e-12) {
                return Err(program.diagnostic(
                    "MS_OUTPUT",
                    "Output receiver_normal must be a finite nonzero direction.",
                    &expr.mark,
                ));
            }
            result.receiver_normal = Some(v.map(|v| v / length));
            continue;
        }
        if name == "normal_offset" {
            let v = value.data().0;
            if v.iter().any(|v| !v.is_finite() || v.abs() > 1e3) {
                return Err(program.diagnostic(
                    "MS_OUTPUT",
                    "Output normal_offset must be a finite vector of moderate length.",
                    &expr.mark,
                ));
            }
            result.normal_offset = Some(v);
            continue;
        }
        match name.as_str() {
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
                &expr.mark,
            ));
        }
    }
    for (index, field) in program.metadata.fields.iter().enumerate() {
        let wanted = match want {
            Want::All => true,
            Want::Channels(names) => names.contains(&field.name.as_str()),
            Want::Height => false,
        };
        if wanted {
            let value = context.field(index).map_err(|error| *error)?;
            result.fields.insert(field.name.clone(), value.field());
        }
    }
    Ok(result)
}
struct Context<'a> {
    program: &'a Program,
    compiled: &'a super::compiled::Compiled,
    inputs: &'a Inputs,
    features: &'a [Feature],
    sample: &'a Sample,
    /// Validated public parameters in metadata order, shared by every sample of a batch.
    parameters: &'a [Value],
    /// Fields evaluated for this sample, by field index.
    fields: Vec<Option<Value>>,
    steps: usize,
    depth: usize,
    remaining: &'a mut usize,
}
/// Evaluation results keep the rare error boxed, so every successful node returns a small value.
type Evaluated = Result<Value, Box<ProgramDiagnostic>>;
impl<'a> Context<'a> {
    /// Evaluates call arguments in order without a heap allocation for the common small arity.
    fn with_arguments(
        &mut self,
        args: &[Node],
        locals: &[Value],
        call: impl FnOnce(&mut Self, &[Value]) -> Evaluated,
    ) -> Evaluated {
        const INLINE: usize = 8;
        if args.len() <= INLINE {
            let mut values: [Value; INLINE] = std::array::from_fn(|_| Value::Bool(false));
            for (slot, arg) in values.iter_mut().zip(args) {
                *slot = self.expression(arg, locals)?;
            }
            call(self, &values[..args.len()])
        } else {
            let values = args
                .iter()
                .map(|arg| self.expression(arg, locals))
                .collect::<Result<Vec<_>, _>>()?;
            call(self, &values)
        }
    }
    fn field(&mut self, index: usize) -> Evaluated {
        if let Some(Some(value)) = self.fields.get(index) {
            return Ok(value.clone());
        }
        let compiled = self.compiled;
        let result = self.expression(&compiled.fields[index], &[])?;
        self.fields[index] = Some(result.clone());
        Ok(result)
    }
    fn expression(&mut self, node: &Node, locals: &[Value]) -> Evaluated {
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
        let value = match &node.kind {
            Kind::Number(n) => Value::scalar(*n),
            Kind::Bool(b) => Value::Bool(*b),
            Kind::Text(s) => Value::Text(s.clone()),
            Kind::Local(index) => locals[*index].clone(),
            Kind::Input(input) => {
                let sample = self.sample;
                match input {
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
            Kind::Parameter(index) => self.parameters[*index].clone(),
            Kind::Field(index) => self.field(*index)?,
            Kind::Unary(op, a) => {
                let a = self.expression(a, locals)?;
                if *op == "!" {
                    Value::Bool(!a.boolean())
                } else {
                    let (v, n, w) = a.data();
                    Value::Number(v.map(|v| if *op == "-" { -v } else { v }), n, w)
                }
            }
            Kind::Binary(op, a, b) => {
                let a = self.expression(a, locals)?;
                if *op == "&&" && !a.boolean() {
                    Value::Bool(false)
                } else if *op == "||" && a.boolean() {
                    Value::Bool(true)
                } else {
                    let b = self.expression(b, locals)?;
                    match *op {
                        "&&" => Value::Bool(a.boolean() && b.boolean()),
                        "||" => Value::Bool(a.boolean() || b.boolean()),
                        "==" | "!=" => {
                            let equal = match (&a, &b) {
                                (Value::Bool(a), Value::Bool(b)) => a == b,
                                _ => {
                                    let (a, an, _) = a.data();
                                    let (b, bn, _) = b.data();
                                    an == bn && a[..an] == b[..bn]
                                }
                            };
                            Value::Bool(if *op == "==" { equal } else { !equal })
                        }
                        "<" => Value::Bool(a.first() < b.first()),
                        ">" => Value::Bool(a.first() > b.first()),
                        "<=" => Value::Bool(a.first() <= b.first()),
                        ">=" => Value::Bool(a.first() >= b.first()),
                        _ => numeric_binary(op, &a, &b),
                    }
                }
            }
            Kind::Member(value, components) => {
                let (value, _, w) = self.expression(value, locals)?.data();
                let mut picked = [0.; 3];
                for (slot, component) in picked.iter_mut().zip(components) {
                    *slot = value[*component];
                }
                Value::Number(picked, components.len(), w)
            }
            Kind::Select(condition, yes, no) => {
                let condition = self.expression(condition, locals)?.boolean();
                self.expression(if condition { yes } else { no }, locals)?
            }
            Kind::Function(index, args) => {
                let compiled = self.compiled;
                let body = &compiled.functions[*index].body;
                self.with_arguments(args, locals, |context, values| {
                    context.expression(body, values)
                })?
            }
            Kind::Builtin(name, args) => self.with_arguments(args, locals, |context, values| {
                context.builtin(name, values, &node.mark)
            })?,
        };
        self.depth -= 1;
        if self.steps > 16_384 {
            Err(self.program.diagnostic(
                "MS_LIMIT",
                "Material evaluation exceeds its operation budget.",
                &node.mark,
            ))?;
        }
        if let Value::Number(v, n, w) = &value {
            if v[..*n].iter().any(|v| !v.is_finite() || v.abs() > 1e100) || !w.is_finite() {
                Err(self.program.diagnostic(
                    "MS_NONFINITE",
                    "Expression produced a non-finite value or invalid arithmetic.",
                    &node.mark,
                ))?;
            }
        }
        Ok(value)
    }
    fn builtin(&mut self, name: &str, a: &[Value], mark: &Mark) -> Evaluated {
        let error = |message: &str| self.program.diagnostic("MS_INPUT", message, mark);
        let value = match name {
            "vec2" | "vec3" => {
                let size = if name == "vec2" { 2 } else { 3 };
                if a.len() == 1 {
                    let (v, n, w) = a[0].data();
                    if n == 1 {
                        Value::Number([v[0]; 3], size, w)
                    } else {
                        a[0].clone()
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
                    Value::Number(data, count, width)
                }
            }
            // The most frequent operations come first: string arms are tested in order.
            "mix" | "clamp" | "smoothstep" => {
                let datas = [a[0].data(), a[1].data(), a[2].data()];
                let n = if name == "smoothstep" {
                    datas[2].1
                } else {
                    datas[0].1
                };
                let mut out = [0.; 3];
                for i in 0..n {
                    let get = |j: usize| datas[j].0[if datas[j].1 == 1 { 0 } else { i }];
                    let (x, y, z) = (get(0), get(1), get(2));
                    out[i] = match name {
                        "mix" => x + (y - x) * z,
                        "clamp" => {
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
                Value::Number(out, n, datas.iter().map(|d| d.2).sum())
            }
            "abs" | "floor" | "fract" | "sin" | "cos" | "sqrt" => {
                let (v, n, w) = a[0].data();
                let apply: fn(f64) -> f64 = match name {
                    "abs" => f64::abs,
                    "floor" => f64::floor,
                    "fract" => |x| x - x.floor(),
                    "sin" => sin,
                    "cos" => cos,
                    "sqrt" => f64::sqrt,
                    _ => unreachable!(),
                };
                Value::Number(v.map(apply), n, w)
            }
            "min" | "max" | "step" => {
                let (v, n, w) = a[0].data();
                let (b, bn, bw) = a[1].data();
                let size = n.max(bn);
                let mut out = [0.; 3];
                for i in 0..size {
                    let x = v[if n == 1 { 0 } else { i }];
                    let y = b[if bn == 1 { 0 } else { i }];
                    out[i] = match name {
                        "min" => x.min(y),
                        "max" => x.max(y),
                        _ => {
                            if y < x {
                                0.
                            } else {
                                1.
                            }
                        }
                    };
                }
                Value::Number(out, size, w + bw)
            }
            "value_noise" => {
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
                Value::Number(
                    [raw * (1. - filter) + 0.5 * filter, 0., 0.],
                    1,
                    (1. - filter) * w * 1.5,
                )
            }
            "hash" => {
                let (p, n, _) = a[0].data();
                let seed = a[1].first();
                let cell = p.map(f64::floor);
                let z = if n == 3 { cell[2] } else { 0. };
                let y = if n >= 2 { cell[1] } else { 0. };
                Value::scalar(hash(cell[0], y, z, seed))
            }
            "dot" => {
                let (v, n, w) = a[0].data();
                let (b, _, bw) = a[1].data();
                Value::Number(
                    [v[..n].iter().zip(b).map(|(a, b)| a * b).sum(), 0., 0.],
                    1,
                    w * max_abs(b, n) + bw * max_abs(v, n),
                )
            }
            "cross" => {
                let (u, _, uw) = a[0].data();
                let (v, _, vw) = a[1].data();
                let product = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                // Each component changes by at most |u| times v's variation plus |v| times u's.
                Value::Number(product, 3, uw * length(v) + vw * length(u))
            }
            "length" | "normalize" => {
                let (v, n, w) = a[0].data();
                let len = v[..n].iter().map(|v| v * v).sum::<f64>().sqrt();
                if name == "length" {
                    Value::Number([len, 0., 0.], 1, w)
                } else {
                    if len < 1e-12 {
                        Err(error("Cannot normalize a zero vector."))?;
                    }
                    Value::Number(v.map(|v| v / len), n, w / len)
                }
            }
            "pow" => {
                let (v, n, w) = a[0].data();
                let exponent = a[1].first();
                if v[..n].iter().any(|v| *v < 0.) || !exponent.is_finite() {
                    Err(error("pow needs nonnegative bases and a finite exponent."))?;
                }
                let slope = v[..n]
                    .iter()
                    .map(|v| (exponent * super::numeric::pow(v.max(1e-9), exponent - 1.)).abs())
                    .fold(0., f64::max);
                Value::Number(
                    v.map(|v| super::numeric::pow(v, exponent)),
                    n,
                    (w * slope).min(1e9),
                )
            }
            "exp" => {
                let (v, n, w) = a[0].data();
                if v[..n].iter().any(|v| *v > 200.) {
                    Err(error("exp arguments above 200 overflow."))?;
                }
                let largest = v[..n]
                    .iter()
                    .map(|v| super::numeric::exp(*v))
                    .fold(0., f64::max);
                Value::Number(v.map(super::numeric::exp), n, (w * largest).min(1e9))
            }
            "seed" => {
                let mut hash = 2_166_136_261u32 ^ self.inputs.seed;
                for byte in a[0].text().bytes() {
                    hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
                }
                Value::scalar(f64::from(hash))
            }
            "noise" | "fbm" | "cellular" => {
                let (v, n, w) = a[0].data();
                let seed = a[1].first();
                let count = if name == "fbm" {
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
                    let work = if name == "cellular" { 27 } else { 8 };
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
                    let raw = if name == "cellular" {
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
                Value::Number([sum / total, 0., 0.], 1, variation / total)
            }
            "scale" => numeric_binary("*", &a[0], &a[1]),
            "offset" | "warp" => numeric_binary("+", &a[0], &a[1]),
            "repeat" => {
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
                Value::Number(result, n, w)
            }
            "rotate" => {
                let (v, _, w) = a[0].data();
                let angle = a[1].first();
                let (s, c) = (sin(angle), cos(angle));
                Value::vector(
                    &[v[0] * c - v[1] * s, v[0] * s + v[1] * c],
                    w + a[1].data().2 * (v[0].abs() + v[1].abs()),
                )
            }
            "sample_color" | "sample_data" | "sample_alpha" => {
                let id = a[0].text();
                let image = self
                    .inputs
                    .images
                    .get(id)
                    .ok_or_else(|| error(&format!("Source image '{id}' was not supplied.")))?;
                let uv = a[1].data().0;
                let sampled = image_sample(image, [uv[0], uv[1]], name == "sample_color");
                if name == "sample_alpha" {
                    Value::scalar(sampled[3])
                } else {
                    Value::vector(&sampled[..3], 0.)
                }
            }
            "runoff" => {
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
                Value::scalar(runoff(self.sample, &self.inputs.edges, width, extent))
            }
            "crease_distance" => {
                let work = self.features.len() * 8;
                self.steps += work;
                if *self.remaining < work {
                    Err(self.program.diagnostic(
                        "MS_LIMIT",
                        "Material sample batch exceeds its operation allowance.",
                        mark,
                    ))?;
                }
                *self.remaining -= work;
                let selector = a[0].text();
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let distance = self
                    .features
                    .iter()
                    .filter(|feature| features::selected(feature, selector, min_angle))
                    .map(|feature| {
                        features::segment_distance(self.sample.world, feature.start, feature.end)
                    })
                    .fold(limit, f64::min);
                // Distance changes one meter per meter, so it carries the sample's footprint.
                Value::Number([distance, 0., 0.], 1, self.sample.footprint)
            }
            "crease_normal" => {
                let work = self.features.len() * 8;
                self.steps += work;
                if *self.remaining < work {
                    Err(self.program.diagnostic(
                        "MS_LIMIT",
                        "Material sample batch exceeds its operation allowance.",
                        mark,
                    ))?;
                }
                *self.remaining -= work;
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let normal = features::crease_normal(
                    self.features,
                    self.sample.world,
                    self.sample.normal,
                    a[0].text(),
                    min_angle,
                    limit,
                );
                Value::vector(&normal, 0.)
            }
            "crease_tangent" | "crease_adjacent_normal" => {
                let work = self.features.len() * 8;
                self.steps += work;
                if *self.remaining < work {
                    Err(self.program.diagnostic(
                        "MS_LIMIT",
                        "Material sample batch exceeds its operation allowance.",
                        mark,
                    ))?;
                }
                *self.remaining -= work;
                let min_angle = a[1].first();
                let limit = a[2].first();
                if !(limit > 0.) || !min_angle.is_finite() {
                    Err(error(
                        "Crease queries need a finite minimum angle in degrees and a positive maximum distance in meters.",
                    ))?;
                }
                let direction = if name == "crease_tangent" {
                    features::crease_tangent(
                        self.features,
                        self.sample.world,
                        a[0].text(),
                        min_angle,
                        limit,
                    )
                } else {
                    features::crease_adjacent_normal(
                        self.features,
                        self.sample.world,
                        self.sample.normal,
                        a[0].text(),
                        min_angle,
                        limit,
                    )
                };
                Value::vector(&direction, 0.)
            }
            "adjust_normal" => {
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
                Value::vector(&turned, weight_width.min(2.))
            }
            "triplanar_weights" => {
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
                Value::vector(&weights.map(|v| v / total), 0.)
            }
            "varied_color" => {
                let id = a[0].text();
                let image = self
                    .inputs
                    .images
                    .get(id)
                    .ok_or_else(|| error(&format!("Source image '{id}' was not supplied.")))?;
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
                let pyramid = self.inputs.mips.pyramid(id, image);
                let color = super::images::varied(
                    &pyramid,
                    image,
                    [uv[0], uv[1]],
                    width,
                    seed,
                    rotation,
                    correction,
                );
                Value::vector(&color, 0.)
            }
            "remap" => {
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
                let mut out = [0.; 3];
                for i in 0..n {
                    let mut t = (v[i] - in_lo) / span;
                    if clamp {
                        t = t.clamp(0., 1.);
                    }
                    out[i] = out_lo + t * (out_hi - out_lo);
                }
                // The variation estimate scales by the slope; clamping can only reduce it.
                Value::Number(out, n, w * ((out_hi - out_lo) / span).abs())
            }
            "soft_threshold" => {
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
                let mut out = [0.; 3];
                for i in 0..n {
                    let t = ((v[i] - threshold) / width + 0.5).clamp(0., 1.);
                    out[i] = t * t * (3. - 2. * t);
                }
                Value::Number(out, n, (w * 1.5 / width).min(1.))
            }
            "distance_band" => {
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
                Value::Number(
                    [1. - t * t * (3. - 2. * t), 0., 0.],
                    1,
                    (w * 1.5 / (hi - lo)).min(1.),
                )
            }
            "soft_union" | "mask_intersect" | "mask_subtract" => {
                let (x, _, xw) = a[0].data();
                let (y, _, yw) = a[1].data();
                let (x, y) = (x[0].clamp(0., 1.), y[0].clamp(0., 1.));
                let combined = match name {
                    "soft_union" => 1. - (1. - x) * (1. - y),
                    "mask_intersect" => x * y,
                    _ => x * (1. - y),
                };
                Value::Number([combined, 0., 0.], 1, (xw + yw).min(1.))
            }
            _ => unreachable!("typechecked builtin"),
        };
        Ok(value)
    }
}
fn numeric_binary(op: &str, a: &Value, b: &Value) -> Value {
    let (a, an, aw) = a.data();
    let (b, bn, bw) = b.data();
    let n = an.max(bn);
    let mut v = [0.; 3];
    for i in 0..n {
        let x = a[if an == 1 { 0 } else { i }];
        let y = b[if bn == 1 { 0 } else { i }];
        v[i] = match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => x / y,
            "%" => x % y,
            _ => unreachable!(),
        };
    }
    let width = match op {
        "*" => aw * max_abs(b, bn) + bw * max_abs(a, an) + aw * bw,
        "/" => {
            let denominator = b[..bn]
                .iter()
                .map(|v| v.abs())
                .fold(f64::INFINITY, f64::min);
            aw / denominator + bw * max_abs(a, an) / (denominator * denominator)
        }
        _ => aw + bw,
    };
    Value::Number(v, n, width)
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
