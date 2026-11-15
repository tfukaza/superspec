use super::Package;
use crate::itemspec::definition::{ItemDiagnostic, check_cancelled};

pub(super) struct Data {
    pub values: Vec<[f64; 4]>,
    pub components: usize,
    pub kind: usize,
    pub normalized: bool,
}
impl Package {
    pub(super) fn accessor(
        &self,
        index: usize,
        maximum: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Data, ItemDiagnostic> {
        let pointer = format!("/accessors/{index}");
        let accessors = self.array("accessors", 16384)?;
        let accessor = accessors
            .get(index)
            .ok_or_else(|| self.error(&pointer, "Missing accessor."))?;
        let components = match accessor["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err(self.error(&pointer, "Expected a scalar or vector accessor.")),
        };
        let kind = self.index(&accessor["componentType"], &pointer)?;
        let size = match kind {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(self.error(&pointer, "Unsupported accessor component type.")),
        };
        let normalized = accessor
            .get("normalized")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| self.error(&pointer, "Accessor normalized must be boolean."))
            })
            .transpose()?
            .unwrap_or(false);
        if normalized && matches!(kind, 5125 | 5126) {
            return Err(self.error(
                &pointer,
                "Only byte and short integer accessors may be normalized.",
            ));
        }
        let count = self.index(&accessor["count"], &pointer)?;
        if count == 0 || count > maximum {
            return Err(self.error(
                &pointer,
                "Accessor count exceeds its allowance or is empty.",
            ));
        }
        let packed = components * size;
        let offset = accessor
            .get("byteOffset")
            .map(|v| self.index(v, &pointer))
            .transpose()?
            .unwrap_or(0);
        if !offset.is_multiple_of(size) {
            return Err(self.error(&pointer, "Accessor offset is not component-aligned."));
        }
        let mut values = vec![[0.; 4]; count];
        if let Some(view) = accessor.get("bufferView") {
            let view = self.index(view, &pointer)?;
            let views = self.array("bufferViews", 16384)?;
            let view_json = views
                .get(view)
                .ok_or_else(|| self.error(&pointer, "Missing buffer view."))?;
            let view_offset = view_json
                .get("byteOffset")
                .map(|v| self.index(v, &pointer))
                .transpose()?
                .unwrap_or(0);
            if !view_offset.is_multiple_of(size) {
                return Err(self.error(&pointer, "Buffer view is not component-aligned."));
            }
            let stride = view_json
                .get("byteStride")
                .map(|v| self.index(v, &pointer))
                .transpose()?
                .unwrap_or(packed);
            if stride < packed
                || !stride.is_multiple_of(size)
                || view_json
                    .get("byteStride")
                    .is_some_and(|_| stride > 252 || !stride.is_multiple_of(4))
            {
                return Err(self.error(&pointer, "Invalid accessor byte stride."));
            }
            let data = self.view(view, &pointer)?;
            let end = offset
                .checked_add(
                    (count - 1)
                        .checked_mul(stride)
                        .ok_or_else(|| self.error(&pointer, "Accessor stride overflow."))?,
                )
                .and_then(|v| v.checked_add(packed))
                .ok_or_else(|| self.error(&pointer, "Accessor range overflow."))?;
            if end > data.len() {
                return Err(self.error(&pointer, "Accessor exceeds its buffer view."));
            }
            for (row, value) in values.iter_mut().enumerate() {
                if row.is_multiple_of(4096) {
                    check_cancelled(&self.source, cancelled)?;
                }
                for (component, target) in value.iter_mut().enumerate().take(components) {
                    *target = self.component(
                        data,
                        offset + row * stride + component * size,
                        kind,
                        normalized,
                        &pointer,
                    )?;
                }
            }
        } else if offset != 0 || accessor.get("sparse").is_none() {
            return Err(self.error(
                &pointer,
                "An accessor without a buffer view requires sparse values and zero offset.",
            ));
        }
        if let Some(sparse) = accessor.get("sparse") {
            let sparse_count = self.index(&sparse["count"], &pointer)?;
            if sparse_count == 0 || sparse_count > count {
                return Err(self.error(&pointer, "Invalid sparse accessor count."));
            }
            let indices = &sparse["indices"];
            let index_kind = self.index(&indices["componentType"], &pointer)?;
            let index_size = match index_kind {
                5121 => 1,
                5123 => 2,
                5125 => 4,
                _ => {
                    return Err(self.error(
                        &pointer,
                        "Sparse indices require unsigned integer components.",
                    ));
                }
            };
            let index_view = self.index(&indices["bufferView"], &pointer)?;
            let index_offset = indices
                .get("byteOffset")
                .map(|v| self.index(v, &pointer))
                .transpose()?
                .unwrap_or(0);
            let index_bytes = self.view(index_view, &pointer)?;
            let value_view = self.index(&sparse["values"]["bufferView"], &pointer)?;
            let value_offset = sparse["values"]
                .get("byteOffset")
                .map(|v| self.index(v, &pointer))
                .transpose()?
                .unwrap_or(0);
            let value_bytes = self.view(value_view, &pointer)?;
            for (view, size) in [(index_view, index_size), (value_view, size)] {
                let value = &self.array("bufferViews", 16384)?[view];
                let start = value
                    .get("byteOffset")
                    .map(|v| self.index(v, &pointer))
                    .transpose()?
                    .unwrap_or(0);
                if value.get("byteStride").is_some() || !start.is_multiple_of(size) {
                    return Err(self.error(
                        &pointer,
                        "Sparse views must be tightly packed and component-aligned.",
                    ));
                }
            }
            if !index_offset.is_multiple_of(index_size) || !value_offset.is_multiple_of(size) {
                return Err(self.error(
                    &pointer,
                    "Sparse accessor offsets must be component-aligned.",
                ));
            }
            let mut previous = None;
            for row in 0..sparse_count {
                if row.is_multiple_of(4096) {
                    check_cancelled(&self.source, cancelled)?;
                }
                let offset = index_offset
                    .checked_add(row * index_size)
                    .ok_or_else(|| self.error(&pointer, "Sparse index overflow."))?;
                let target =
                    self.component(index_bytes, offset, index_kind, false, &pointer)? as usize;
                if target >= count || previous.is_some_and(|old| target <= old) {
                    return Err(self.error(
                        &pointer,
                        "Sparse indices must be unique, increasing and within the accessor.",
                    ));
                }
                previous = Some(target);
                for component in 0..components {
                    let offset = value_offset
                        .checked_add(row * packed + component * size)
                        .ok_or_else(|| self.error(&pointer, "Sparse value overflow."))?;
                    values[target][component] =
                        self.component(value_bytes, offset, kind, normalized, &pointer)?;
                }
            }
        }
        Ok(Data {
            values,
            components,
            kind,
            normalized,
        })
    }
    fn component(
        &self,
        bytes: &[u8],
        offset: usize,
        kind: usize,
        normalized: bool,
        pointer: &str,
    ) -> Result<f64, ItemDiagnostic> {
        let size = match kind {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            _ => 4,
        };
        let data = bytes
            .get(
                offset
                    ..offset
                        .checked_add(size)
                        .ok_or_else(|| self.error(pointer, "Component offset overflow."))?,
            )
            .ok_or_else(|| self.error(pointer, "Accessor component exceeds its view."))?;
        let mut value = match kind {
            5120 => (data[0] as i8) as f64,
            5121 => data[0] as f64,
            5122 => i16::from_le_bytes(data.try_into().unwrap()) as f64,
            5123 => u16::from_le_bytes(data.try_into().unwrap()) as f64,
            5125 => u32::from_le_bytes(data.try_into().unwrap()) as f64,
            5126 => f32::from_le_bytes(data.try_into().unwrap()) as f64,
            _ => unreachable!(),
        };
        if !value.is_finite() {
            return Err(self.error(pointer, "Accessor contains a nonfinite value."));
        }
        if normalized {
            value = match kind {
                5120 => (value / 127.).max(-1.),
                5121 => value / 255.,
                5122 => (value / 32767.).max(-1.),
                5123 => value / 65535.,
                _ => unreachable!(),
            };
        }
        Ok(value)
    }
}
