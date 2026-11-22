//! Binary glTF streams shared by exact value and accessor layout within one export.
use crate::Options;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::BuildHasher,
    ops::Range,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Layout {
    component_type: u32,
    components: usize,
    target: Option<u32>,
    bounds: bool,
}

struct Stored {
    range: Range<usize>,
    accessor: usize,
}

pub(super) struct Buffer<'a, 'cancel> {
    pub(super) bytes: Vec<u8>,
    pub(super) views: Vec<Value>,
    pub(super) accessors: Vec<Value>,
    pub(super) options: &'a Options<'cancel>,
    hash: RandomState,
    streams: HashMap<(Layout, u64), Vec<Stored>>,
}

impl<'a, 'cancel> Buffer<'a, 'cancel> {
    pub(super) fn new(options: &'a Options<'cancel>) -> Self {
        Self {
            bytes: vec![],
            views: vec![],
            accessors: vec![],
            options,
            hash: RandomState::new(),
            streams: HashMap::new(),
        }
    }

    pub(super) fn view(&mut self, bytes: &[u8], target: Option<u32>) -> Result<usize, String> {
        let padding = (4 - self.bytes.len() % 4) % 4;
        let length = self
            .bytes
            .len()
            .checked_add(padding)
            .and_then(|n| n.checked_add(bytes.len()))
            .ok_or("Export size overflow.")?;
        self.options.check(length)?;
        self.bytes.resize(self.bytes.len() + padding, 0);
        let mut view = json!({"buffer":0,"byteOffset":self.bytes.len(),"byteLength":bytes.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.views.push(view);
        self.bytes.extend_from_slice(bytes);
        Ok(self.views.len() - 1)
    }

    fn accessor(
        &mut self,
        layout: Layout,
        bytes: &[u8],
        count: usize,
        bounds: impl FnOnce() -> Option<(Vec<f32>, Vec<f32>)>,
    ) -> Result<usize, String> {
        self.options.check(0)?;
        let key = (layout, self.hash.hash_one(bytes));
        if let Some(existing) = self.streams.get(&key).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| &self.bytes[entry.range.clone()] == bytes)
        }) {
            return Ok(existing.accessor);
        }
        let view = self.view(bytes, layout.target)?;
        let end = self.bytes.len();
        let kind = match layout.components {
            1 => "SCALAR",
            2 => "VEC2",
            3 => "VEC3",
            4 => "VEC4",
            _ => unreachable!(),
        };
        let mut record = json!({
            "bufferView": view, "componentType": layout.component_type, "type": kind, "count": count,
        });
        if let Some((minimum, maximum)) = bounds() {
            record["min"] = json!(minimum);
            record["max"] = json!(maximum);
        }
        let accessor = self.accessors.len();
        self.accessors.push(record);
        self.streams.entry(key).or_default().push(Stored {
            range: end - bytes.len()..end,
            accessor,
        });
        Ok(accessor)
    }

    pub(super) fn floats(
        &mut self,
        values: &[f32],
        components: usize,
        bounds: bool,
    ) -> Result<usize, String> {
        self.attribute(values, components, bounds, Some(34962))
    }

    pub(super) fn attribute(
        &mut self,
        values: &[f32],
        components: usize,
        bounds: bool,
        target: Option<u32>,
    ) -> Result<usize, String> {
        #[cfg(target_endian = "little")]
        let bytes = bytemuck::cast_slice(values);
        #[cfg(target_endian = "big")]
        let bytes = &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.accessor(
            Layout {
                component_type: 5126,
                components,
                bounds,
                target,
            },
            bytes,
            values.len() / components,
            || {
                if !bounds {
                    return None;
                }
                let mut minimum = vec![f32::INFINITY; components];
                let mut maximum = vec![f32::NEG_INFINITY; components];
                for vertex in values.chunks_exact(components) {
                    for (axis, &v) in vertex.iter().enumerate() {
                        minimum[axis] = minimum[axis].min(v);
                        maximum[axis] = maximum[axis].max(v);
                    }
                }
                Some((minimum, maximum))
            },
        )
    }

    pub(super) fn indices(&mut self, values: &[u32]) -> Result<usize, String> {
        // glTF reserves the all-ones index of each component type for primitive restart.
        let layout = Layout {
            component_type: 5125,
            components: 1,
            bounds: false,
            target: Some(34963),
        };
        if values.iter().all(|&v| v < u32::from(u16::MAX)) {
            let bytes: Vec<_> = values
                .iter()
                .flat_map(|&v| (v as u16).to_le_bytes())
                .collect();
            return self.accessor(
                Layout {
                    component_type: 5123,
                    ..layout
                },
                &bytes,
                values.len(),
                || None,
            );
        }
        #[cfg(target_endian = "little")]
        let bytes = bytemuck::cast_slice(values);
        #[cfg(target_endian = "big")]
        let bytes = &values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.accessor(layout, bytes, values.len(), || None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_accessors_require_the_same_layout_and_exact_bits() {
        let options = Options::default();
        let mut output = Buffer::new(&options);
        let v = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let first = output.floats(&v, 3, false).unwrap();
        assert_eq!(output.floats(&v, 3, false).unwrap(), first);
        assert_ne!(output.floats(&v, 2, false).unwrap(), first);
        let position = output.floats(&v, 3, true).unwrap();
        assert_ne!(position, first);
        assert_eq!(output.accessors[position]["min"], json!([0.0, 0.0, 0.0]));
        assert_ne!(output.attribute(&v, 3, false, None).unwrap(), first);
        let mut alternate = v;
        alternate[0] = -0.0;
        assert_ne!(output.floats(&alternate, 3, false).unwrap(), first);
    }

    #[test]
    fn compact_indices_exclude_the_u16_primitive_restart_value() {
        let options = Options::default();
        let mut output = Buffer::new(&options);
        let compact = output.indices(&[0, 1, 65534]).unwrap();
        assert_eq!(output.accessors[compact]["componentType"], 5123);
        assert_eq!(output.bytes, [0, 0, 1, 0, 254, 255]);
        assert_eq!(output.indices(&[0, 1, 65534]).unwrap(), compact);
        let wide = output.indices(&[0, 65535, 65536]).unwrap();
        assert_eq!(output.accessors[wide]["componentType"], 5125);
        let view = output.accessors[wide]["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(output.views[view]["byteOffset"], 8);
        assert_eq!(output.bytes[8..], [0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 1, 0]);
    }
}
