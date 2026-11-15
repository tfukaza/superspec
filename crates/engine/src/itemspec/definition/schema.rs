use super::{ItemDiagnostic, fail};
use crate::materialspec::program::MaterialRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use superspec_geometry::Matrix;

pub type Values = BTreeMap<String, Value>;
pub type Transform = [[f64; 3]; 4];
pub fn identity() -> Transform {
    Matrix::default().0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    pub format: String,
    pub version: u32,
    pub id: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Parameter>,
    pub generator: Generator,
    #[serde(default)]
    pub slots: BTreeMap<String, MaterialRequest>,
    #[serde(default = "identity")]
    pub transform: Transform,
    #[serde(default)]
    pub attachments: Vec<AttachmentDefinition>,
    #[serde(default)]
    pub collision: Collision,
    #[serde(skip)]
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    #[default]
    Geometry,
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterType {
    Number,
    Integer,
    Boolean,
    Color,
    Choice,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    #[serde(rename = "type")]
    pub kind: ParameterType,
    #[serde(default)]
    pub role: Role,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub unit: Option<String>,
    #[serde(default)]
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterReference {
    pub parameter: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Number(f64),
    Parameter(ParameterReference),
}
impl Default for Scalar {
    fn default() -> Self {
        Self::Number(0.)
    }
}
fn segments() -> Scalar {
    Scalar::Number(16.)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Switch {
    Boolean(bool),
    Parameter(ParameterReference),
}
impl Default for Switch {
    fn default() -> Self {
        Self::Boolean(true)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Generator {
    #[serde(rename_all = "camelCase")]
    Pipe {
        length: Scalar,
        radius: Scalar,
        #[serde(default)]
        wall_thickness: Scalar,
        #[serde(default = "segments")]
        segments: Scalar,
        #[serde(default)]
        capped: Switch,
    },
    #[serde(rename_all = "camelCase")]
    Duct {
        length: Scalar,
        width: Scalar,
        height: Scalar,
        #[serde(default)]
        wall_thickness: Scalar,
        #[serde(default)]
        capped: Switch,
    },
    Glb {
        asset: String,
        scene: Option<usize>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentDefinition {
    pub name: String,
    pub position: [Scalar; 3],
    pub direction: [f64; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollisionIntent {
    #[default]
    None,
    Bounds,
    Mesh,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CollisionShape {
    Box { center: [f64; 3], size: [f64; 3] },
    Sphere { center: [f64; 3], radius: f64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Collision {
    Intent(CollisionIntent),
    Shape(CollisionShape),
}
impl Default for Collision {
    fn default() -> Self {
        Self::Intent(CollisionIntent::None)
    }
}

pub(super) fn bounded_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-./".contains(&c))
}

impl Parameter {
    fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            ParameterType::Number | ParameterType::Integer => {
                value.as_f64().is_some_and(|number| {
                    number.is_finite()
                        && (self.kind != ParameterType::Integer || number.fract() == 0.)
                        && self.min.is_none_or(|min| number >= min)
                        && self.max.is_none_or(|max| number <= max)
                })
            }
            ParameterType::Boolean => value.is_boolean(),
            ParameterType::Color => {
                value.as_str().is_some_and(|color| {
                    color.len() == 7
                        && color.starts_with('#')
                        && color[1..].bytes().all(|c| c.is_ascii_hexdigit())
                }) || value.as_array().is_some_and(|rgb| {
                    rgb.len() == 3
                        && rgb.iter().all(|v| {
                            v.as_f64()
                                .is_some_and(|v| v.is_finite() && (0. ..=1.).contains(&v))
                        })
                })
            }
            ParameterType::Choice => value
                .as_str()
                .is_some_and(|v| self.choices.iter().any(|choice| choice == v)),
        }
    }
}

impl ItemDefinition {
    pub fn resolve_parameters(&self, overrides: &Values) -> Result<Values, ItemDiagnostic> {
        if self.format != "itemspec" || self.version != 1 {
            return Err(fail(
                &self.source,
                "/version",
                "ITEM_VERSION",
                "Expected itemspec version 1.",
            ));
        }
        if !bounded_name(&self.id) {
            return Err(fail(
                &self.source,
                "/id",
                "ITEM_ID",
                "Item ID must be a bounded nonempty identifier.",
            ));
        }
        if self.parameters.len() > 64 || self.slots.len() > 128 || self.attachments.len() > 256 {
            return Err(fail(
                &self.source,
                "/",
                "ITEM_LIMIT",
                "Item controls, slots or attachments exceed their limits.",
            ));
        }
        for name in overrides.keys() {
            if !self.parameters.contains_key(name) {
                return Err(fail(
                    &self.source,
                    &format!("/parameters/{}", super::pointer_key(name)),
                    "ITEM_PARAMETER",
                    "Unknown parameter override.",
                ));
            }
        }
        let mut values = Values::new();
        for (name, parameter) in &self.parameters {
            let pointer = format!("/parameters/{}", super::pointer_key(name));
            if !bounded_name(name)
                || parameter.unit.as_ref().is_some_and(|unit| unit.len() > 32)
                || parameter.choices.len() > 64
                || parameter.choices.iter().any(|v| v.len() > 128)
                || parameter.min.is_some_and(|v| !v.is_finite())
                || parameter.max.is_some_and(|v| !v.is_finite())
                || parameter.min.zip(parameter.max).is_some_and(|(a, b)| a > b)
                || !parameter.accepts(&parameter.default)
            {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "Invalid parameter declaration or default.",
                ));
            }
            let value = overrides.get(name).unwrap_or(&parameter.default);
            if !parameter.accepts(value) {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_PARAMETER",
                    "Parameter value violates its type, range or choices.",
                ));
            }
            values.insert(name.clone(), value.clone());
        }
        Matrix(self.transform)
            .checked()
            .and_then(Matrix::inverse)
            .map_err(|e| fail(&self.source, "/transform", "ITEM_TRANSFORM", e))?;
        if self.transform.iter().flatten().any(|v| v.abs() > 1e7) {
            return Err(fail(
                &self.source,
                "/transform",
                "ITEM_TRANSFORM",
                "Transform exceeds item coordinate limits.",
            ));
        }
        match &self.collision {
            Collision::Shape(CollisionShape::Box { center, size })
                if center
                    .iter()
                    .chain(size)
                    .any(|v| !v.is_finite() || v.abs() > 1e7)
                    || size.iter().any(|v| *v <= 0.) =>
            {
                return Err(fail(
                    &self.source,
                    "/collision",
                    "ITEM_COLLISION",
                    "Box collision needs finite center and positive finite size.",
                ));
            }
            Collision::Shape(CollisionShape::Sphere { center, radius })
                if center.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
                    || !radius.is_finite()
                    || *radius <= 0.
                    || *radius > 1e7 =>
            {
                return Err(fail(
                    &self.source,
                    "/collision",
                    "ITEM_COLLISION",
                    "Sphere collision needs a finite center and positive finite radius.",
                ));
            }
            _ => {}
        }
        Ok(values)
    }

    pub(super) fn parameter<'a>(
        &self,
        name: &str,
        values: &'a Values,
        pointer: &str,
        geometry: bool,
    ) -> Result<&'a Value, ItemDiagnostic> {
        let declaration = self.parameters.get(name).ok_or_else(|| {
            fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                format!("Unknown parameter {name}."),
            )
        })?;
        if geometry && declaration.role != Role::Geometry {
            return Err(fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Geometry cannot depend on a finish control.",
            ));
        }
        values.get(name).ok_or_else(|| {
            fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Missing resolved parameter.",
            )
        })
    }
    pub(super) fn scalar(
        &self,
        expression: &Scalar,
        values: &Values,
        pointer: &str,
    ) -> Result<f64, ItemDiagnostic> {
        let number = match expression {
            Scalar::Number(number) => *number,
            Scalar::Parameter(reference) => self
                .parameter(&reference.parameter, values, pointer, true)?
                .as_f64()
                .ok_or_else(|| {
                    fail(
                        &self.source,
                        pointer,
                        "ITEM_PARAMETER",
                        "Geometry requires a numeric control.",
                    )
                })?,
        };
        if !number.is_finite() {
            return Err(fail(
                &self.source,
                pointer,
                "ITEM_PARAMETER",
                "Geometry values must be finite.",
            ));
        }
        Ok(number)
    }
    pub(super) fn switch(
        &self,
        expression: &Switch,
        values: &Values,
        pointer: &str,
    ) -> Result<bool, ItemDiagnostic> {
        match expression {
            Switch::Boolean(value) => Ok(*value),
            Switch::Parameter(reference) => self
                .parameter(&reference.parameter, values, pointer, true)?
                .as_bool()
                .ok_or_else(|| {
                    fail(
                        &self.source,
                        pointer,
                        "ITEM_PARAMETER",
                        "Geometry requires a boolean control.",
                    )
                }),
        }
    }
    pub(super) fn materials(
        &self,
        values: &Values,
    ) -> Result<BTreeMap<String, MaterialRequest>, ItemDiagnostic> {
        let mut result = BTreeMap::new();
        for (slot, request) in &self.slots {
            let pointer = format!("/slots/{}", super::pointer_key(slot));
            if !bounded_name(slot)
                || request.entry.is_empty()
                || request.entry.len() > 1024
                || request.parameters.len() > 64
            {
                return Err(fail(
                    &self.source,
                    &pointer,
                    "ITEM_MATERIAL",
                    "Invalid material slot or entry.",
                ));
            }
            let mut request = request.clone();
            for (name, value) in &mut request.parameters {
                if let Some(reference) = value
                    .as_object()
                    .filter(|object| object.len() == 1)
                    .and_then(|object| object.get("parameter"))
                    .and_then(Value::as_str)
                {
                    let resolved = self.parameter(
                        reference,
                        values,
                        &format!("{pointer}/parameters/{}", super::pointer_key(name)),
                        false,
                    )?;
                    *value = if self.parameters[reference].kind == ParameterType::Color {
                        resolved
                            .as_str()
                            .map(|hex| serde_json::json!(superspec_types::color::srgb(hex)))
                            .unwrap_or_else(|| resolved.clone())
                    } else {
                        resolved.clone()
                    };
                }
                if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 4096) {
                    return Err(fail(
                        &self.source,
                        &pointer,
                        "ITEM_LIMIT",
                        "Material parameter exceeds its size limit.",
                    ));
                }
            }
            result.insert(slot.clone(), request);
        }
        Ok(result)
    }
}
