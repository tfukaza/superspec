//! Evaluates a program for a row of samples at once.
//!
//! The per-sample evaluator walks the whole expression tree for every texel, so most of its time
//! goes to dispatching nodes, accounting for steps and copying values rather than to arithmetic.
//! This evaluator walks the tree once for up to [`LANES`] samples: each node computes its value for
//! every sample before its parent runs, and every value comes from the same scalar kernels the
//! per-sample evaluator uses, so each sample's results are bit for bit its own.
//!
//! * A node that depends on no sample input (literals, parameters, seeds and arithmetic on them)
//!   is computed once for the whole row.
//! * `select`, `&&`, `||` and fields are lazy per sample: a branch runs only for the samples that
//!   take it, and a field only for the samples that read it, as in the per-sample evaluator.
//! * Nothing here reports an error. A sample that would fail (an invalid input, a non-finite
//!   value, a domain error, an exhausted allowance) makes the whole call decline, and the caller
//!   evaluates the samples one at a time, which fails with the one error that order produces. The
//!   steps and nesting a sample can take are bounded when the program compiles
//!   ([`super::compiled::Cost`]); a program whose bound could reach a limit is never evaluated here.
use super::{
    Feature, FieldValue, Inputs, Outputs, Program, Sample,
    compiled::{Builtin, Compiled, Kind, Node, Op},
    eval::{Context, OutputSet, Scratch, Value, numeric_binary, store_output, validate_sample},
    features,
};
use std::collections::BTreeMap;

/// The values one field took at every texel of an atlas in an earlier evaluation, so a later
/// evaluation whose inputs cannot change the field reads them instead of computing the field. A
/// field's value depends only on the sample and the parameters, seed, images and edges it reads, so
/// the owner keys a plane by those. Texels the field was not computed for hold NaN as their
/// footprint estimate (an evaluated value never has one) and are computed when needed.
#[derive(Clone, Debug)]
pub struct FieldPlane {
    field: usize,
    /// Components of the field's value, or 4 for a boolean.
    kind: u8,
    /// Four numbers per texel: the components (padding included) and the footprint estimate.
    data: Vec<f64>,
}

impl FieldPlane {
    /// An empty plane for `field` (a value of `components` numbers, or a boolean when 0) over
    /// `texels` texels.
    pub fn new(field: usize, components: usize, texels: usize) -> Self {
        Self {
            field,
            kind: if components == 0 { 4 } else { components as u8 },
            data: [0., 0., 0., f64::NAN].repeat(texels),
        }
    }
    pub fn field(&self) -> usize {
        self.field
    }
    /// Bytes this plane holds.
    pub fn bytes(&self) -> usize {
        self.data.len() * 8 + 64
    }
    fn value<'a>(&self, texel: usize) -> Option<Value<'a>> {
        let at = &self.data[texel * 4..texel * 4 + 4];
        if at[3].is_nan() {
            return None;
        }
        Some(if self.kind == 4 {
            Value::Bool(at[0] != 0.)
        } else {
            Value::Number([at[0], at[1], at[2]], self.kind as usize, at[3])
        })
    }
    fn store(&mut self, texel: usize, value: &Value<'_>) {
        let at = &mut self.data[texel * 4..texel * 4 + 4];
        if self.kind == 4 {
            at.copy_from_slice(&[f64::from(u8::from(value.boolean())), 0., 0., 0.]);
        } else {
            let (data, _, width) = value.data();
            at.copy_from_slice(&[data[0], data[1], data[2], width]);
        }
    }
}

/// Samples evaluated together. More amortize dispatch further; fewer keep every node's values in
/// the first-level cache.
pub(super) const LANES: usize = 64;

type Buf<'a> = [Value<'a>; LANES];

/// The value of a node for the active lanes: the same for all of them, or one per lane.
#[derive(Clone, Copy)]
enum Src<'a> {
    One(Value<'a>),
    Buf(usize),
}

/// A node's value, and whether the buffer holding it belongs to the caller to release.
#[derive(Clone, Copy)]
struct Res<'a> {
    src: Src<'a>,
    owned: bool,
}

/// A lane evaluation gave up; the caller evaluates one sample at a time instead.
pub(super) struct Declined;
type Lane<T> = Result<T, Declined>;

enum FieldLanes<'a> {
    Pending,
    Uniform(Value<'a>),
    Lanes { buf: usize, done: u64 },
}

struct Lanes<'a, 'b> {
    program: &'a Program,
    compiled: &'a Compiled,
    inputs: &'a Inputs,
    features: &'b [Feature],
    index: &'b features::Index,
    parameters: &'b [Value<'a>],
    samples: &'b [Sample],
    scratch: &'b mut Scratch<'a>,
    bufs: Vec<Option<Box<Buf<'a>>>>,
    free: Vec<usize>,
    fields: Vec<FieldLanes<'a>>,
    /// Buffers fields keep for the current samples, released when the next samples start.
    field_bufs: Vec<usize>,
    /// Buffers of helper arguments being evaluated, released when their call returns.
    owned: Vec<usize>,
    /// Helper arguments of the calls being evaluated; a helper's own arguments start at `base`.
    locals: Vec<Src<'a>>,
    base: usize,
}

/// Read access to a node's values by lane.
enum View<'s, 'a> {
    One(&'s Value<'a>),
    Many(&'s Buf<'a>),
}

impl<'s, 'a> View<'s, 'a> {
    #[inline(always)]
    fn get(&self, lane: usize) -> &'s Value<'a> {
        match self {
            View::One(value) => value,
            View::Many(buf) => &buf[lane],
        }
    }
}

fn active(mask: u64) -> impl Iterator<Item = usize> {
    let mut rest = mask;
    std::iter::from_fn(move || {
        (rest != 0).then(|| {
            let lane = rest.trailing_zeros() as usize;
            rest &= rest - 1;
            lane
        })
    })
}

fn one(value: Value<'_>) -> Res<'_> {
    Res {
        src: Src::One(value),
        owned: false,
    }
}

/// Builtins that read the sample being evaluated, beyond their arguments.
fn reads_sample(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Runoff
            | Builtin::CreaseDistance
            | Builtin::CreaseNormal
            | Builtin::CreaseTangent
            | Builtin::CreaseAdjacentNormal
    )
}

impl<'a, 'b> Lanes<'a, 'b> {
    fn alloc(&mut self) -> usize {
        if let Some(id) = self.free.pop() {
            return id;
        }
        self.bufs.push(Some(Box::new([Value::EMPTY; LANES])));
        self.bufs.len() - 1
    }

    fn release(&mut self, res: &Res<'a>) {
        if let (true, Src::Buf(id)) = (res.owned, res.src) {
            self.free.push(id);
        }
    }

    fn buf(&self, id: usize) -> &Buf<'a> {
        self.bufs[id].as_ref().expect("a buffer in use")
    }

    fn value(&self, src: &Src<'a>, lane: usize) -> Value<'a> {
        match src {
            Src::One(value) => *value,
            Src::Buf(id) => self.buf(*id)[lane],
        }
    }

    /// Starts a new row of samples: every field is computed afresh.
    fn reset(&mut self) {
        for field in &mut self.fields {
            *field = FieldLanes::Pending;
        }
        self.free.append(&mut self.field_bufs);
        self.locals.clear();
        self.owned.clear();
        self.base = 0;
    }

    fn eval(&mut self, node: &'a Node, mask: u64) -> Lane<Res<'a>> {
        match &node.kind {
            Kind::Number(n) => {
                let value = Value::scalar(*n);
                if value.unsound() {
                    return Err(Declined);
                }
                Ok(one(value))
            }
            Kind::Bool(b) => Ok(one(Value::Bool(*b))),
            Kind::Text(s) => Ok(one(Value::Text(s))),
            Kind::Local(index) => Ok(Res {
                src: self.locals[self.base + *index],
                owned: false,
            }),
            Kind::Parameter(index) => {
                let value = self.parameters[*index];
                if value.unsound() {
                    return Err(Declined);
                }
                Ok(one(value))
            }
            Kind::Input(input) => {
                use super::compiled::Input as I;
                let out = self.alloc();
                let mut buf = self.bufs[out].take().expect("a free buffer");
                for lane in active(mask) {
                    let sample = &self.samples[lane];
                    let value = match input {
                        I::Uv => Value::vector(&sample.uv, sample.uv_footprint.unwrap_or(0.)),
                        I::Surface => Value::vector(&sample.surface, sample.footprint),
                        I::Object => Value::vector(&sample.object, sample.footprint),
                        I::World => Value::vector(&sample.world, sample.footprint),
                        I::Normal => Value::vector(&sample.normal, 0.),
                        I::Gravity => Value::vector(&sample.gravity, 0.),
                        I::BoundsMin => Value::vector(&sample.bounds_min, 0.),
                        I::BoundsMax => Value::vector(&sample.bounds_max, 0.),
                        I::Footprint => Value::scalar(sample.footprint),
                        I::UvFootprint => Value::scalar(sample.uv_footprint.unwrap_or(0.)),
                    };
                    buf[lane] = value;
                }
                self.bufs[out] = Some(buf);
                Ok(Res {
                    src: Src::Buf(out),
                    owned: true,
                })
            }
            Kind::Field(index) => self.field(*index, mask),
            Kind::Unary(op, operand) => {
                let a = self.eval(operand, mask)?;
                let apply = |a: &Value<'a>| -> Value<'a> {
                    if *op == Op::Not {
                        Value::Bool(!a.boolean())
                    } else {
                        let (v, n, w) = a.data();
                        Value::Number(v.map(|v| if *op == Op::Sub { -v } else { v }), n, w)
                    }
                };
                self.map1(a, mask, apply)
            }
            Kind::Binary(op, left, right) => self.binary(*op, left, right, mask),
            Kind::Member(value, components) => {
                let a = self.eval(value, mask)?;
                self.map1(a, mask, |a| {
                    let (value, _, w) = a.data();
                    let mut picked = [0.; 3];
                    for (slot, component) in picked.iter_mut().zip(components) {
                        *slot = value[*component];
                    }
                    Value::Number(picked, components.len(), w)
                })
            }
            Kind::Select(condition, yes, no) => {
                let chosen = self.eval(condition, mask)?;
                let mut taken = 0u64;
                for lane in active(mask) {
                    if self.value(&chosen.src, lane).boolean() {
                        taken |= 1 << lane;
                    }
                }
                self.release(&chosen);
                let (yes_mask, no_mask) = (taken, mask & !taken);
                // One side everywhere: that side's value as it is.
                if no_mask == 0 {
                    return self.eval(yes, yes_mask);
                }
                if yes_mask == 0 {
                    return self.eval(no, no_mask);
                }
                let out = self.alloc();
                for (branch, branch_mask) in [(yes, yes_mask), (no, no_mask)] {
                    let value = self.eval(branch, branch_mask)?;
                    let mut buf = self.bufs[out].take().expect("a free buffer");
                    for lane in active(branch_mask) {
                        buf[lane] = self.value(&value.src, lane);
                    }
                    self.bufs[out] = Some(buf);
                    self.release(&value);
                }
                Ok(Res {
                    src: Src::Buf(out),
                    owned: true,
                })
            }
            Kind::Function(index, args) => {
                let start = self.locals.len();
                let owned_start = self.owned.len();
                for arg in args {
                    let value = self.eval(arg, mask)?;
                    if let (true, Src::Buf(id)) = (value.owned, value.src) {
                        self.owned.push(id);
                    }
                    self.locals.push(value.src);
                }
                let compiled = self.compiled;
                let saved = std::mem::replace(&mut self.base, start);
                let result = self.eval(&compiled.functions[*index].body, mask);
                self.base = saved;
                self.locals.truncate(start);
                let mut result = result?;
                // A result that is one of the arguments' buffers must outlive them.
                if let (false, Src::Buf(id)) = (result.owned, result.src) {
                    if self.owned[owned_start..].contains(&id) {
                        let copy = self.alloc();
                        let mut buf = self.bufs[copy].take().expect("a free buffer");
                        for lane in active(mask) {
                            buf[lane] = self.buf(id)[lane];
                        }
                        self.bufs[copy] = Some(buf);
                        result = Res {
                            src: Src::Buf(copy),
                            owned: true,
                        };
                    }
                }
                self.free.extend(self.owned.drain(owned_start..));
                Ok(result)
            }
            Kind::Builtin(name, args) => self.builtin(*name, args, node, mask),
        }
    }

    /// `f` for every active lane over `a`'s values; once, for a value that is the same everywhere.
    fn map1(
        &mut self,
        a: Res<'a>,
        mask: u64,
        f: impl Fn(&Value<'a>) -> Value<'a>,
    ) -> Lane<Res<'a>> {
        if let Src::One(x) = a.src {
            let value = f(&x);
            if value.unsound() {
                return Err(Declined);
            }
            return Ok(one(value));
        }
        let out = self.alloc();
        let mut buf = self.bufs[out].take().expect("a free buffer");
        let mut sound = true;
        {
            let x = self.view(&a.src);
            for lane in active(mask) {
                let value = f(x.get(lane));
                if value.unsound() {
                    sound = false;
                    break;
                }
                buf[lane] = value;
            }
        }
        self.bufs[out] = Some(buf);
        self.release(&a);
        if !sound {
            return Err(Declined);
        }
        Ok(Res {
            src: Src::Buf(out),
            owned: true,
        })
    }
    fn binary(&mut self, op: Op, left: &'a Node, right: &'a Node, mask: u64) -> Lane<Res<'a>> {
        let a = self.eval(left, mask)?;
        // `&&` and `||` evaluate their right side only for the samples that need it.
        if matches!(op, Op::And | Op::Or) {
            let mut needs = 0u64;
            for lane in active(mask) {
                let left_true = self.value(&a.src, lane).boolean();
                if (op == Op::And) == left_true {
                    needs |= 1 << lane;
                }
            }
            let b = if needs == 0 {
                one(Value::Bool(false))
            } else {
                self.eval(right, needs)?
            };
            let out = self.alloc();
            let mut buf = self.bufs[out].take().expect("a free buffer");
            for lane in active(mask) {
                let left_true = self.value(&a.src, lane).boolean();
                buf[lane] = Value::Bool(if needs >> lane & 1 == 1 {
                    let right_true = self.value(&b.src, lane).boolean();
                    if op == Op::And {
                        left_true && right_true
                    } else {
                        left_true || right_true
                    }
                } else {
                    // `&&` with a false left side, or `||` with a true one.
                    op == Op::Or
                });
            }
            self.bufs[out] = Some(buf);
            self.release(&a);
            self.release(&b);
            return Ok(Res {
                src: Src::Buf(out),
                owned: true,
            });
        }
        let b = self.eval(right, mask)?;
        let apply = |x: &Value<'a>, y: &Value<'a>, out: &mut Value<'a>| match op {
            Op::Equal | Op::NotEqual => {
                let equal = super::eval::equal(x, y);
                *out = Value::Bool(if op == Op::Equal { equal } else { !equal });
            }
            Op::Less => *out = Value::Bool(x.first() < y.first()),
            Op::Greater => *out = Value::Bool(x.first() > y.first()),
            Op::LessEqual => *out = Value::Bool(x.first() <= y.first()),
            Op::GreaterEqual => *out = Value::Bool(x.first() >= y.first()),
            _ => numeric_binary(op, x, y, out),
        };
        // Operands that are the same everywhere give a result that is too.
        if let (Src::One(x), Src::One(y)) = (a.src, b.src) {
            let mut value = Value::EMPTY;
            apply(&x, &y, &mut value);
            if value.unsound() {
                return Err(Declined);
            }
            return Ok(one(value));
        }
        let out = self.alloc();
        let mut buf = self.bufs[out].take().expect("a free buffer");
        let mut sound = true;
        {
            let (x, y) = (self.view(&a.src), self.view(&b.src));
            for lane in active(mask) {
                let slot = &mut buf[lane];
                apply(x.get(lane), y.get(lane), slot);
                if slot.unsound() {
                    sound = false;
                    break;
                }
            }
        }
        self.bufs[out] = Some(buf);
        self.release(&a);
        self.release(&b);
        if !sound {
            return Err(Declined);
        }
        Ok(Res {
            src: Src::Buf(out),
            owned: true,
        })
    }

    fn view<'s>(&'s self, src: &'s Src<'a>) -> View<'s, 'a> {
        match src {
            Src::One(value) => View::One(value),
            Src::Buf(id) => View::Many(self.buf(*id)),
        }
    }

    fn builtin(
        &mut self,
        name: Builtin,
        args: &'a [Node],
        node: &'a Node,
        mask: u64,
    ) -> Lane<Res<'a>> {
        if args.len() > 8 || name == Builtin::Unknown {
            return Err(Declined);
        }
        let mut operands = [one(Value::EMPTY); 8];
        for (slot, arg) in operands.iter_mut().zip(args) {
            *slot = self.eval(arg, mask)?;
        }
        let operands = &operands[..args.len()];
        let first = mask.trailing_zeros() as usize;
        let uniform = !reads_sample(name) && operands.iter().all(|r| matches!(r.src, Src::One(_)));
        let mut remaining = usize::MAX / 2;
        let mut gathered = [Value::EMPTY; 8];
        let (program, compiled, inputs) = (self.program, self.compiled, self.inputs);
        let (features, index, parameters, samples) =
            (self.features, self.index, self.parameters, self.samples);
        let result = if uniform {
            for (slot, operand) in gathered.iter_mut().zip(operands) {
                if let Src::One(value) = operand.src {
                    *slot = value;
                }
            }
            let mut context = Context::lane(
                program,
                compiled,
                inputs,
                features,
                index,
                &samples[first],
                parameters,
                &mut *self.scratch,
                &mut remaining,
            );
            let mut value = Value::EMPTY;
            context
                .builtin(name, &gathered[..args.len()], &node.mark, &mut value)
                .map_err(|_| Declined)?;
            if value.unsound() {
                return Err(Declined);
            }
            one(value)
        } else {
            let out = self.alloc();
            let mut buf = self.bufs[out].take().expect("a free buffer");
            let mut failed = false;
            {
                let mut context = Context::lane(
                    program,
                    compiled,
                    inputs,
                    features,
                    index,
                    &samples[first],
                    parameters,
                    &mut *self.scratch,
                    &mut remaining,
                );
                for lane in active(mask) {
                    for (slot, operand) in gathered.iter_mut().zip(operands) {
                        *slot = match operand.src {
                            Src::One(value) => value,
                            Src::Buf(id) => self.bufs[id].as_ref().expect("a buffer in use")[lane],
                        };
                    }
                    context.set_sample(&samples[lane]);
                    let mut value = Value::EMPTY;
                    if context
                        .builtin(name, &gathered[..args.len()], &node.mark, &mut value)
                        .is_err()
                        || value.unsound()
                    {
                        failed = true;
                        break;
                    }
                    buf[lane] = value;
                }
            }
            self.bufs[out] = Some(buf);
            if failed {
                return Err(Declined);
            }
            Res {
                src: Src::Buf(out),
                owned: true,
            }
        };
        for operand in operands {
            self.release(operand);
        }
        Ok(result)
    }

    fn field(&mut self, index: usize, mask: u64) -> Lane<Res<'a>> {
        if index >= self.fields.len() {
            return Err(Declined);
        }
        let need = match self.fields[index] {
            FieldLanes::Uniform(value) => return Ok(one(value)),
            FieldLanes::Lanes { buf, done } => {
                if mask & !done == 0 {
                    return Ok(Res {
                        src: Src::Buf(buf),
                        owned: false,
                    });
                }
                mask & !done
            }
            FieldLanes::Pending => mask,
        };
        let compiled = self.compiled;
        let value = self.eval(&compiled.fields[index], need)?;
        match value.src {
            Src::One(one_value) => {
                if matches!(self.fields[index], FieldLanes::Lanes { .. }) {
                    return Err(Declined);
                }
                self.fields[index] = FieldLanes::Uniform(one_value);
                Ok(one(one_value))
            }
            Src::Buf(id) => {
                let (buf, done) = match self.fields[index] {
                    FieldLanes::Lanes { buf, done } => (buf, done),
                    FieldLanes::Uniform(_) => return Err(Declined),
                    FieldLanes::Pending => {
                        let buf = self.alloc();
                        self.field_bufs.push(buf);
                        (buf, 0)
                    }
                };
                if buf != id {
                    let mut target = self.bufs[buf].take().expect("a field buffer");
                    for lane in active(need) {
                        target[lane] = self.buf(id)[lane];
                    }
                    self.bufs[buf] = Some(target);
                    self.release(&value);
                }
                self.fields[index] = FieldLanes::Lanes {
                    buf,
                    done: done | need,
                };
                Ok(Res {
                    src: Src::Buf(buf),
                    owned: false,
                })
            }
        }
    }
}

/// Evaluates `outputs` (and the fields chosen by `selection`) for every sample, or declines.
/// Results are those of evaluating each sample alone: `fields` receives the selected fields in
/// sample-major order. `remaining` is the allowance the caller has left; the lanes leave it as
/// they found it.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate<'a>(
    program: &'a Program,
    compiled: &'a Compiled,
    inputs: &'a Inputs,
    features: &[Feature],
    index: &features::Index,
    samples: &[Sample],
    parameters: &[Value<'a>],
    remaining: usize,
    scratch: &mut Scratch<'a>,
    outputs: OutputSet,
    selected: &[(usize, usize)],
    fields: &mut [FieldValue],
    texels: &[usize],
    injected: &[&FieldPlane],
    capture: &mut [FieldPlane],
) -> Option<Vec<Outputs>> {
    let cost = &compiled.cost;
    let queries = cost.queries * (1 + features.len() / 16);
    let steps = cost.steps + cost.runoffs * inputs.edges.len() * 16 + queries;
    let allowance =
        cost.steps + cost.runoffs * inputs.edges.len() * 16 + cost.queries * features.len() * 8;
    if steps > 16_000 || cost.depth > 120 || allowance.saturating_mul(samples.len()) > remaining {
        return None;
    }
    let mut lanes = Lanes {
        program,
        compiled,
        inputs,
        features,
        index,
        parameters,
        samples,
        scratch,
        bufs: Vec::new(),
        free: Vec::new(),
        fields: (0..compiled.fields.len())
            .map(|_| FieldLanes::Pending)
            .collect(),
        field_bufs: Vec::new(),
        owned: Vec::new(),
        locals: Vec::new(),
        base: 0,
    };
    let mut results = Vec::with_capacity(samples.len());
    for (start, chunk) in samples
        .chunks(LANES)
        .enumerate()
        .map(|(i, c)| (i * LANES, c))
    {
        lanes.samples = chunk;
        lanes.reset();
        for sample in chunk {
            validate_sample(program, sample).ok()?;
        }
        let mask = if chunk.len() == LANES {
            !0u64
        } else {
            (1u64 << chunk.len()) - 1
        };
        // Fields an earlier evaluation kept: their values at these samples are already known.
        for plane in injected {
            if plane.field >= lanes.fields.len() {
                continue;
            }
            let mut known = 0u64;
            let mut buf = None;
            for lane in 0..chunk.len() {
                let Some(value) = plane.value(texels[start + lane]) else {
                    continue;
                };
                let id = *buf.get_or_insert_with(|| {
                    let id = lanes.alloc();
                    lanes.field_bufs.push(id);
                    id
                });
                lanes.bufs[id].as_mut().expect("a field buffer")[lane] = value;
                known |= 1 << lane;
            }
            if let Some(buf) = buf {
                lanes.fields[plane.field] = FieldLanes::Lanes { buf, done: known };
            }
        }
        let mut done: Vec<Outputs> = (0..chunk.len())
            .map(|_| Outputs {
                base_color: [0.; 3],
                roughness: 1.,
                metalness: 0.,
                height: 0.,
                emissive: [0.; 3],
                opacity: 1.,
                receiver_normal: None,
                normal_offset: None,
                fields: BTreeMap::new(),
            })
            .collect();
        for (name, node) in &compiled.outputs {
            if !outputs.contains(OutputSet::named(name)) {
                continue;
            }
            let value = lanes.eval(node, mask).ok()?;
            for (lane, result) in done.iter_mut().enumerate() {
                let sample_value = lanes.value(&value.src, lane);
                store_output(program, name, &node.mark, &sample_value, result).ok()?;
            }
            lanes.release(&value);
        }
        for &(field, slot) in selected {
            let value = lanes.field(field, mask).ok()?;
            for lane in 0..chunk.len() {
                fields[(start + lane) * selected.len() + slot] =
                    lanes.value(&value.src, lane).field();
            }
        }
        for plane in capture.iter_mut() {
            match lanes.fields.get(plane.field) {
                Some(FieldLanes::Uniform(value)) => {
                    for lane in 0..chunk.len() {
                        plane.store(texels[start + lane], value);
                    }
                }
                Some(FieldLanes::Lanes { buf, done }) => {
                    let (buf, done) = (*buf, *done);
                    for lane in active(done) {
                        let value = lanes.buf(buf)[lane];
                        plane.store(texels[start + lane], &value);
                    }
                }
                _ => {}
            }
        }
        results.extend(done);
    }
    Some(results)
}
