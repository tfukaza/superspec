//! Per-texel values an evaluation reads instead of computing them.
//!
//! A plane holds one value for each texel of an atlas and says what the values stand for
//! ([`Binding`]): a field of the program being evaluated, or a named external input the program
//! reads with `external(name, fallback)`. Both evaluators (one sample at a time, and a row of
//! samples together) read the same planes at the texel a sample is given with, and charge the
//! same steps for it:
//!
//! * A reference to a field whose plane holds the sample's texel costs the reference's one step;
//!   the field's body is not evaluated and not charged.
//! * `external(name, fallback)` costs its own step and its arguments' whether or not a plane
//!   supplies the value, so a program's cost never depends on what a host binds.
//!
//! A field's plane must hold what the program computes for the field at that texel (an earlier
//! evaluation of the same sample and inputs): it only changes what is computed, never a result.
//! An external's plane is an input like an image: its content decides the result, so hosts key
//! what they bake by it.
use super::eval::Value;
use std::sync::Arc;

/// What a plane's values stand for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    /// The field with this index in `Program::metadata().fields`.
    Field(usize),
    /// The external input of this name, read by `external(name, fallback)`.
    External(Arc<str>),
}

/// One value per texel of an atlas; see the module documentation.
///
/// A plane holds its values in one of two forms, and the other is empty. Reading the full form
/// is the path every kept field takes for every texel, so it is tried first and costs one range
/// check, as indexing the values directly would.
#[derive(Clone, Debug)]
pub struct Plane {
    binding: Binding,
    /// Components of a value, or 4 for a boolean.
    kind: u8,
    /// Full form, four numbers per texel: the components (padding included) and the footprint
    /// estimate. A texel without a value holds NaN as its estimate (an evaluated value never
    /// has one).
    full: Vec<f64>,
    /// Single form, one number per component and texel at single precision, with no footprint
    /// estimate. A texel without a value holds NaN as its first component.
    single: Vec<f32>,
    /// Texels holding a value.
    stored: usize,
    /// Texels given a value since the plane was made or last taken up for completion.
    added: usize,
    /// Samples evaluated while this plane was being filled that left no value in it: the field
    /// was not read there, or the row was evaluated one sample at a time.
    pub(super) missed: usize,
}

/// The values one field took at every texel of an atlas in an earlier evaluation: a [`Plane`]
/// bound to the field.
pub type FieldPlane = Plane;

/// Leading bytes of an encoded external plane (see [`Plane::encode_external`]).
pub const PLANE_MAGIC: &[u8; 4] = b"SPL1";
/// Bytes before the values of an encoded external plane.
const HEADER: usize = 16;

impl Plane {
    /// An empty plane for `field` (a value of `components` numbers, or a boolean when 0) over
    /// `texels` texels.
    pub fn new(field: usize, components: usize, texels: usize) -> Self {
        Self {
            binding: Binding::Field(field),
            kind: if components == 0 { 4 } else { components as u8 },
            full: [0., 0., 0., f64::NAN].repeat(texels),
            single: Vec::new(),
            stored: 0,
            added: 0,
            missed: 0,
        }
    }

    /// A plane for the external input `name`: `components` (1 to 3) single-precision numbers per
    /// texel, texel after texel. A texel whose first component is NaN has no value, and a program
    /// reads its fallback there; other components must be finite.
    pub fn external(name: &str, components: usize, values: Vec<f32>) -> Result<Self, String> {
        if !(1..=3).contains(&components) || !values.len().is_multiple_of(components) {
            return Err(format!(
                "External plane '{name}' needs 1 to 3 components for every texel."
            ));
        }
        let mut stored = 0;
        for texel in values.chunks_exact(components) {
            if texel[0].is_nan() {
                continue;
            }
            if texel.iter().any(|v| !v.is_finite()) {
                return Err(format!("External plane '{name}' holds a non-finite value."));
            }
            stored += 1;
        }
        Ok(Self {
            binding: Binding::External(Arc::from(name)),
            kind: components as u8,
            full: Vec::new(),
            single: values,
            stored,
            added: 0,
            missed: 0,
        })
    }

    /// This plane's values as the external input `name`, at full precision and with their
    /// footprint estimates: how one bake's field becomes another program's input. Booleans have
    /// no external form.
    pub fn into_external(mut self, name: &str) -> Result<Self, String> {
        if self.kind == 4 {
            return Err(format!("External plane '{name}' cannot hold booleans."));
        }
        self.binding = Binding::External(Arc::from(name));
        self.added = 0;
        self.missed = 0;
        Ok(self)
    }

    /// An external plane as bytes a host can store or hand to another session: the magic, the
    /// atlas width and height, the component count (all little-endian 32-bit), then the values as
    /// little-endian `f32`, rows from the atlas's upper edge.
    pub fn encode_external(
        width: usize,
        height: usize,
        components: usize,
        values: &[f32],
    ) -> Result<Vec<u8>, String> {
        if !(1..=3).contains(&components)
            || width
                .checked_mul(height)
                .and_then(|n| n.checked_mul(components))
                != Some(values.len())
            || width > u32::MAX as usize
            || height > u32::MAX as usize
        {
            return Err("An external plane needs 1 to 3 components for every texel.".into());
        }
        let mut bytes = Vec::with_capacity(HEADER + values.len() * 4);
        bytes.extend(PLANE_MAGIC);
        for word in [width, height, components] {
            bytes.extend((word as u32).to_le_bytes());
        }
        for value in values {
            bytes.extend(value.to_le_bytes());
        }
        Ok(bytes)
    }

    /// Reads [`Self::encode_external`]'s bytes as the external input `name` of an atlas of
    /// `width` by `height` texels. Bytes of another size, shape or with non-finite values fail.
    pub fn decode_external(
        name: &str,
        bytes: &[u8],
        width: usize,
        height: usize,
    ) -> Result<Self, String> {
        let invalid = |what: &str| format!("External plane '{name}' {what}.");
        if bytes.len() < HEADER || &bytes[..4] != PLANE_MAGIC {
            return Err(invalid("is not an encoded plane"));
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let (have_width, have_height, components) = (word(4), word(8), word(12));
        if (have_width, have_height) != (width, height) {
            return Err(invalid(&format!(
                "covers {have_width} by {have_height} texels where the receiver's atlas has {width} by {height}"
            )));
        }
        let count = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(components))
            .filter(|_| (1..=3).contains(&components));
        if count.and_then(|n| n.checked_mul(4)) != Some(bytes.len() - HEADER) {
            return Err(invalid("does not hold 1 to 3 components for every texel"));
        }
        let values = bytes[HEADER..]
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();
        Self::external(name, components, values)
    }

    /// What the values stand for.
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    /// The field this plane is bound to, if it is bound to one.
    #[inline(always)]
    pub fn field(&self) -> Option<usize> {
        match self.binding {
            Binding::Field(index) => Some(index),
            Binding::External(_) => None,
        }
    }
    /// Whether this plane supplies the external input `name`.
    #[inline]
    pub(super) fn supplies(&self, name: &str) -> bool {
        matches!(&self.binding, Binding::External(own) if **own == *name)
    }
    /// Components of a value; 0 for a boolean.
    pub fn components(&self) -> usize {
        if self.kind == 4 {
            0
        } else {
            self.kind as usize
        }
    }
    /// Texels that hold a value.
    #[inline(always)]
    pub fn stored(&self) -> usize {
        self.stored
    }
    /// Texels given a value since the plane was made or copied by [Self::completing].
    pub fn added(&self) -> usize {
        self.added
    }
    /// Whether some evaluated sample left no value, so a later evaluation may add texels.
    pub fn incomplete(&self) -> bool {
        self.missed > 0
    }
    /// A copy to fill further: an evaluation that computes the field where this plane lacks it
    /// adds those texels to the copy.
    pub fn completing(&self) -> Self {
        Self {
            added: 0,
            missed: 0,
            ..self.clone()
        }
    }
    /// Bytes this plane holds.
    pub fn bytes(&self) -> usize {
        self.full.len() * 8 + self.single.len() * 4 + 64
    }
    /// Texels this plane covers.
    pub fn texels(&self) -> usize {
        self.full.len() / 4 + self.single.len() / (self.kind.clamp(1, 3) as usize)
    }
    /// The four numbers of `texel` in the full form; `None` outside it, and for every texel of
    /// a plane in the single form.
    #[inline(always)]
    fn full_at(&self, texel: usize) -> Option<&[f64]> {
        if texel >= usize::MAX / 4 {
            return None;
        }
        self.full.get(texel * 4..texel * 4 + 4)
    }
    /// Whether `texel` holds a value. Texels outside the plane hold none.
    #[inline(always)]
    pub(super) fn has(&self, texel: usize) -> bool {
        match self.full_at(texel) {
            Some(at) => !at[3].is_nan(),
            None => self.single_at(texel).is_some(),
        }
    }
    /// The value at `texel`, if it holds one. Texels outside the plane hold none.
    #[inline(always)]
    pub(super) fn value<'a>(&self, texel: usize) -> Option<Value<'a>> {
        let Some(at) = self.full_at(texel) else {
            return self.single_at(texel);
        };
        if at[3].is_nan() {
            return None;
        }
        Some(if self.kind == 4 {
            Value::Bool(at[0] != 0.)
        } else {
            Value::Number([at[0], at[1], at[2]], self.kind as usize, at[3])
        })
    }
    /// The value at `texel` of a plane in the single form.
    #[inline(never)]
    fn single_at<'a>(&self, texel: usize) -> Option<Value<'a>> {
        let components = self.kind.clamp(1, 3) as usize;
        let start = texel.checked_mul(components)?;
        let at = self.single.get(start..start.checked_add(components)?)?;
        if at[0].is_nan() {
            return None;
        }
        let mut value = [0.; 3];
        for (slot, component) in value.iter_mut().zip(at) {
            *slot = f64::from(*component);
        }
        Some(Value::Number(value, components, 0.))
    }
    /// Stores an evaluated value at `texel` of a plane made by [`Self::new`], the only planes
    /// that are filled.
    #[inline(always)]
    pub(super) fn store(&mut self, texel: usize, value: &Value<'_>) {
        let at = &mut self.full[texel * 4..texel * 4 + 4];
        if at[3].is_nan() {
            self.stored += 1;
            self.added += 1;
        }
        if self.kind == 4 {
            at.copy_from_slice(&[f64::from(u8::from(value.boolean())), 0., 0., 0.]);
        } else {
            let (data, _, width) = value.data();
            at.copy_from_slice(&[data[0], data[1], data[2], width]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_planes_round_trip_and_reject_what_does_not_fit_the_atlas() {
        let values = [0.25f32, f32::NAN, 1., 0.5, 0.125, 0.75];
        let bytes = Plane::encode_external(3, 2, 1, &values).unwrap();
        let plane = Plane::decode_external("shade", &bytes, 3, 2).unwrap();
        assert_eq!(plane.binding(), &Binding::External("shade".into()));
        assert_eq!(
            (plane.texels(), plane.stored(), plane.components()),
            (6, 5, 1)
        );
        assert!(plane.supplies("shade") && !plane.supplies("shad") && plane.field().is_none());
        assert_eq!(plane.value(0).unwrap().data(), ([0.25, 0., 0.], 1, 0.));
        assert!(
            plane.value(1).is_none() && !plane.has(1),
            "NaN marks no value"
        );
        assert!(plane.value(6).is_none() && plane.value(usize::MAX).is_none());
        let vectors =
            Plane::encode_external(1, 2, 3, &[f32::NAN, 0., 0., 0.5, 0.125, 0.75]).unwrap();
        let plane = Plane::decode_external("tint", &vectors, 1, 2).unwrap();
        assert_eq!(plane.value(1).unwrap().data(), ([0.5, 0.125, 0.75], 3, 0.));
        assert!(
            plane.value(0).is_none(),
            "a NaN first component marks the texel"
        );
        for (bytes, width, height, fragment) in [
            (
                &bytes[..],
                2,
                3,
                "3 by 2 texels where the receiver's atlas has 2 by 3",
            ),
            (&bytes[..bytes.len() - 1], 3, 2, "every texel"),
            (&bytes[..8], 3, 2, "not an encoded plane"),
            (&b"SBR2............"[..], 3, 2, "not an encoded plane"),
        ] {
            let error = Plane::decode_external("shade", bytes, width, height).unwrap_err();
            assert!(
                error.contains(fragment) && error.contains("'shade'"),
                "{error}"
            );
        }
        let mut four = bytes.clone();
        four[12] = 4;
        assert!(Plane::decode_external("shade", &four, 3, 2).is_err());
        assert!(Plane::encode_external(3, 2, 1, &values[..5]).is_err());
        assert!(Plane::external("shade", 1, vec![f32::INFINITY]).is_err());
        assert!(Plane::external("shade", 2, vec![0.5, f32::NAN]).is_err());
        assert!(Plane::external("shade", 4, vec![0.; 4]).is_err());
    }

    #[test]
    fn a_filled_field_plane_becomes_an_external_input_with_its_exact_values() {
        let mut plane = Plane::new(3, 2, 4);
        assert_eq!(plane.field(), Some(3));
        plane.store(2, &Value::Number([0.1, 0.2, 0.], 2, 0.03125));
        assert_eq!((plane.stored(), plane.added()), (1, 1));
        let external = plane.into_external("base.grain").unwrap();
        assert!(external.supplies("base.grain") && external.field().is_none());
        assert_eq!(
            external.value(2).unwrap().data(),
            ([0.1, 0.2, 0.], 2, 0.03125)
        );
        assert!(external.value(0).is_none());
        // No texel index, however large, reads another texel's value.
        for outside in [
            4,
            usize::MAX / 4 - 1,
            usize::MAX / 4,
            1 << 62,
            (1 << 62) + 2,
            usize::MAX,
        ] {
            assert!(
                external.value(outside).is_none() && !external.has(outside),
                "{outside}"
            );
        }
        assert!(Plane::new(0, 0, 1).into_external("flag").is_err());
    }
}
