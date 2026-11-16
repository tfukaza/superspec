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
    planes::{FieldPlane, Plane},
};
use std::collections::BTreeMap;

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
    /// A value no sample can change. Computing it costs every sample the same (each); the lanes
    /// in `paid` have been charged for it, the others are when they first read it.
    Uniform {
        value: Value<'a>,
        paid: u64,
        each: usize,
    },
    Lanes {
        buf: usize,
        done: u64,
    },
}

struct Lanes<'a, 'b> {
    program: &'a Program,
    compiled: &'a Compiled,
    inputs: &'a Inputs,
    features: &'b [Feature],
    index: &'b features::Index,
    parameters: &'b [Value<'a>],
    samples: &'b [Sample],
    /// The plane texel of each sample in `samples`; empty when no plane is read.
    texels: &'b [usize],
    /// Planes of external inputs and kept fields (see [`super::planes`]).
    planes: &'b [&'b Plane],
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
    /// Operations the per-sample evaluator would have charged the batch allowance for the work
    /// done so far: one per node visit of each active lane, plus the extra charges of builtins.
    consumed: usize,
    /// Steps each lane's feature queries have added to its sample's step budget for the features
    /// they visited, and how many such steps the program's other steps leave a sample.
    visits: [usize; LANES],
    room: usize,
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

impl<'a> Res<'a> {
    /// A value that is the same for every lane.
    fn one(value: Value<'a>) -> Self {
        Self {
            src: Src::One(value),
            owned: false,
        }
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
            | Builtin::FeatureDistance
            | Builtin::FeatureTangent
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
        self.visits = [0; LANES];
    }

    fn eval(&mut self, node: &'a Node, mask: u64) -> Lane<Res<'a>> {
        // The per-sample evaluator visits this node once for every active lane.
        self.consumed = self.consumed.saturating_add(mask.count_ones() as usize);
        match &node.kind {
            Kind::Number(n) => {
                let value = Value::scalar(*n);
                if value.unsound() {
                    return Err(Declined);
                }
                Ok(Res::one(value))
            }
            Kind::Bool(b) => Ok(Res::one(Value::Bool(*b))),
            Kind::Text(s) => Ok(Res::one(Value::Text(s))),
            Kind::Local(index) => Ok(Res {
                src: self.locals[self.base + *index],
                owned: false,
            }),
            Kind::Parameter(index) => {
                let value = self.parameters[*index];
                if value.unsound() {
                    return Err(Declined);
                }
                Ok(Res::one(value))
            }
            Kind::Input(input) => {
                use super::compiled::Input as I;
                // A render-only input fails each sample alone, with the error that order gives.
                if input.render_only() {
                    return Err(Declined);
                }
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
                        I::Time | I::Instance | I::InstanceW => unreachable!("declined above"),
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
                let invariant = matches!(chosen.src, Src::One(_));
                self.release(&chosen);
                let (yes_mask, no_mask) = (taken, mask & !taken);
                // One side for every active lane: that side's value. `Src::One` promises a value
                // that no sample can change, and callers (fields above all) keep it for lanes
                // outside this mask. A condition that varies by sample only happened to agree on
                // these lanes, so a branch value that is the same everywhere becomes one per lane.
                if no_mask == 0 || yes_mask == 0 {
                    let value = self.eval(if no_mask == 0 { yes } else { no }, mask)?;
                    return Ok(match value.src {
                        Src::One(same) if !invariant => {
                            let out = self.alloc();
                            let mut buf = self.bufs[out].take().expect("a free buffer");
                            for lane in active(mask) {
                                buf[lane] = same;
                            }
                            self.bufs[out] = Some(buf);
                            Res {
                                src: Src::Buf(out),
                                owned: true,
                            }
                        }
                        _ => value,
                    });
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
            return Ok(Res::one(value));
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
                Res::one(Value::Bool(false))
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
            return Ok(Res::one(value));
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
        if args.len() > 16
            || matches!(
                name,
                Builtin::Unknown | Builtin::InstanceAttribute | Builtin::VertexAttribute
            )
        {
            return Err(Declined);
        }
        // Only `external` reads a plane; it has its own path, so no other operation pays for one.
        if name == Builtin::External {
            return self.external(args, mask);
        }
        let mut operands = [Res::one(Value::EMPTY); 16];
        for (slot, arg) in operands.iter_mut().zip(args) {
            *slot = self.eval(arg, mask)?;
        }
        let operands = &operands[..args.len()];
        let first = mask.trailing_zeros() as usize;
        let uniform = !reads_sample(name) && operands.iter().all(|r| matches!(r.src, Src::One(_)));
        const AMPLE: usize = usize::MAX / 2;
        let mut remaining = AMPLE;
        let mut gathered = [Value::EMPTY; 16];
        let (program, compiled, inputs) = (self.program, self.compiled, self.inputs);
        let (features, index, parameters, samples) =
            (self.features, self.index, self.parameters, self.samples);
        let query = matches!(
            name,
            Builtin::CreaseDistance
                | Builtin::CreaseNormal
                | Builtin::CreaseTangent
                | Builtin::CreaseAdjacentNormal
                | Builtin::FeatureDistance
                | Builtin::FeatureTangent
        );
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
            // Computed once here; charged for every lane that would have computed it.
            self.consumed = self
                .consumed
                .saturating_add((AMPLE - remaining).saturating_mul(mask.count_ones() as usize));
            Res::one(value)
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
                    let before = context.steps();
                    if context
                        .builtin(name, &gathered[..args.len()], &node.mark, &mut value)
                        .is_err()
                        || value.unsound()
                    {
                        failed = true;
                        break;
                    }
                    if query {
                        // The query itself is one of the program's counted steps; the rest is
                        // what the features it visited cost this sample.
                        let visits = &mut self.visits[lane];
                        *visits += (context.steps() - before).saturating_sub(1);
                        if *visits > self.room {
                            failed = true;
                            break;
                        }
                    }
                    buf[lane] = value;
                }
            }
            self.bufs[out] = Some(buf);
            if failed {
                return Err(Declined);
            }
            self.consumed = self.consumed.saturating_add(AMPLE - remaining);
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

    /// `external(name, fallback)` for the active lanes: each lane's value of the plane bound to
    /// the name, at the lane's texel, and the fallback where the plane holds none; the fallback
    /// itself, untouched, when no plane is bound. The arguments are evaluated and charged like
    /// any operation's, and a plane of another shape declines, so the sample fails one at a time
    /// with the error that names it (see `Context::external`).
    #[inline(never)]
    fn external(&mut self, args: &'a [Node], mask: u64) -> Lane<Res<'a>> {
        let [name, fallback] = args else {
            return Err(Declined);
        };
        let Src::One(name) = self.eval(name, mask)?.src else {
            return Err(Declined);
        };
        let fallback = self.eval(fallback, mask)?;
        let name = name.text();
        let (planes, texels) = (self.planes, self.texels);
        let Some(plane) = planes.iter().find(|plane| plane.supplies(name)) else {
            return Ok(fallback);
        };
        let out = self.alloc();
        let mut buf = self.bufs[out].take().expect("a free buffer");
        let mut fits = true;
        for lane in active(mask) {
            let default = self.value(&fallback.src, lane);
            let texel = texels.get(lane).copied().unwrap_or(usize::MAX);
            buf[lane] = match plane.value(texel) {
                Some(value) if value.data().1 == default.data().1 => value,
                Some(_) => {
                    fits = false;
                    break;
                }
                None => default,
            };
        }
        self.bufs[out] = Some(buf);
        self.release(&fallback);
        if !fits {
            self.free.push(out);
            return Err(Declined);
        }
        Ok(Res {
            src: Src::Buf(out),
            owned: true,
        })
    }

    fn field(&mut self, index: usize, mask: u64) -> Lane<Res<'a>> {
        if index >= self.fields.len() {
            return Err(Declined);
        }
        let need = match self.fields[index] {
            FieldLanes::Uniform { value, paid, each } => {
                // A lane reading it for the first time computes it, as far as the allowance goes.
                let unpaid = mask & !paid;
                self.consumed = self
                    .consumed
                    .saturating_add(each.saturating_mul(unpaid.count_ones() as usize));
                self.fields[index] = FieldLanes::Uniform {
                    value,
                    paid: paid | mask,
                    each,
                };
                return Ok(Res::one(value));
            }
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
        let before = self.consumed;
        let value = self.eval(&compiled.fields[index], need)?;
        match value.src {
            Src::One(one_value) => {
                // `Src::One` is a value no sample can change (see `Kind::Select`), so it stands
                // for every lane, including lanes a kept plane already supplied.
                self.fields[index] = FieldLanes::Uniform {
                    value: one_value,
                    paid: need,
                    each: (self.consumed - before) / (need.count_ones() as usize).max(1),
                };
                Ok(Res::one(one_value))
            }
            Src::Buf(id) => {
                let (buf, done) = match self.fields[index] {
                    FieldLanes::Lanes { buf, done } => (buf, done),
                    FieldLanes::Uniform { .. } => return Err(Declined),
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
/// sample-major order. `remaining` is the batch allowance the caller has left. A finished call
/// takes from it exactly what evaluating the samples one at a time takes (see `Lanes::consumed`),
/// and a call that would take more declines, so where an allowance runs out never depends on how
/// rows are grouped. Fields read from `injected` planes are not computed and so not charged, and
/// `external` reads its plane at each sample's texel, exactly as evaluating one sample at a time
/// with the same planes does. This is [`evaluate_together`], noting on the planes being filled
/// the samples a declined call leaves them without.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate<'a>(
    program: &'a Program,
    compiled: &'a Compiled,
    inputs: &'a Inputs,
    features: &[Feature],
    index: &features::Index,
    samples: &[Sample],
    parameters: &[Value<'a>],
    remaining: &mut usize,
    scratch: &mut Scratch<'a>,
    outputs: OutputSet,
    selected: &[(usize, usize)],
    fields: &mut [FieldValue],
    texels: &[usize],
    injected: &[&FieldPlane],
    capture: &mut [FieldPlane],
) -> Option<Vec<Outputs>> {
    let done = evaluate_together(
        program, compiled, inputs, features, index, samples, parameters, remaining, scratch,
        outputs, selected, fields, texels, injected, capture,
    );
    if done.is_none() {
        for plane in capture.iter_mut() {
            plane.missed += samples.len();
        }
    }
    done
}

/// Steps a sample may take when its row is evaluated together, under the per-sample limit.
const STEPS: usize = 16_000;

/// Whether a program's cost bounds keep every sample under the per-sample limits whatever its
/// feature queries visit, given how many source edges and receiver features there are. Rows of
/// such a program are evaluated together unless a sample fails, and may read kept field planes.
pub(super) fn bounded(compiled: &Compiled, edges: usize, features: usize) -> bool {
    room(compiled, edges)
        .is_some_and(|room| compiled.cost.queries.saturating_mul(features / 16) <= room)
}

/// The steps a sample's feature queries may add for the features they visit, beyond one step
/// each, before the program's bound reaches the per-sample limit; `None` for a program whose
/// other steps or nesting already could. A program [`bounded`] does not cover is still evaluated
/// together while every sample's queries, which mostly visit only nearby features, stay within
/// this.
fn room(compiled: &Compiled, edges: usize) -> Option<usize> {
    let cost = &compiled.cost;
    let steps = cost
        .steps
        .saturating_add(cost.runoffs.saturating_mul(edges).saturating_mul(16))
        .saturating_add(cost.queries);
    (cost.depth <= 120)
        .then(|| STEPS.checked_sub(steps))
        .flatten()
}

#[allow(clippy::too_many_arguments)]
fn evaluate_together<'a>(
    program: &'a Program,
    compiled: &'a Compiled,
    inputs: &'a Inputs,
    features: &[Feature],
    index: &features::Index,
    samples: &[Sample],
    parameters: &[Value<'a>],
    remaining: &mut usize,
    scratch: &mut Scratch<'a>,
    outputs: OutputSet,
    selected: &[(usize, usize)],
    fields: &mut [FieldValue],
    texels: &[usize],
    injected: &[&FieldPlane],
    capture: &mut [FieldPlane],
) -> Option<Vec<Outputs>> {
    let room = room(compiled, inputs.edges.len())?;
    // Planes are read by texel: without one for every sample there is nothing to read them at.
    let (texels, injected) = if texels.len() == samples.len() {
        (texels, injected)
    } else {
        (&[][..], &[][..])
    };
    let mut lanes = Lanes {
        program,
        compiled,
        inputs,
        features,
        index,
        parameters,
        samples,
        texels,
        planes: injected,
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
        consumed: 0,
        visits: [0; LANES],
        room,
    };
    let mut results = Vec::with_capacity(samples.len());
    for (start, chunk) in samples
        .chunks(LANES)
        .enumerate()
        .map(|(i, c)| (i * LANES, c))
    {
        lanes.samples = chunk;
        lanes.texels = if texels.is_empty() {
            texels
        } else {
            &texels[start..start + chunk.len()]
        };
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
            let Some(field) = plane.field().filter(|field| *field < lanes.fields.len()) else {
                continue;
            };
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
                lanes.fields[field] = FieldLanes::Lanes { buf, done: known };
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
            let Some(field) = plane.field() else {
                continue;
            };
            let before = plane.stored();
            let had = (0..chunk.len())
                .filter(|lane| plane.has(texels[start + lane]))
                .count();
            match lanes.fields.get(field) {
                Some(FieldLanes::Uniform { value, .. }) => {
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
            plane.missed += chunk.len() - had - (plane.stored() - before);
        }
        results.extend(done);
        // More than the allowance: one sample at a time fails at the sample that exhausts it.
        if lanes.consumed > *remaining {
            return None;
        }
    }
    *remaining -= lanes.consumed;
    Some(results)
}
