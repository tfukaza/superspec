use super::{
    Program,
    syntax::{Definition, Expr, Expression},
};
use std::mem::size_of;

// Charge allocation slack as well as payload. A map entry is conservatively charged
// a complete BTree node, including its eleven key/value slots and child pointers.
fn text(value: &String) -> usize {
    value.capacity().saturating_add(32)
}
fn optional_text(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, text)
}
fn map_storage<K, V>(count: usize) -> usize {
    count.saturating_mul(
        11usize
            .saturating_mul(size_of::<(K, V)>())
            .saturating_add(128),
    )
}
fn vector<T>(value: &Vec<T>) -> usize {
    value
        .capacity()
        .saturating_mul(size_of::<T>())
        .saturating_add(32)
}
fn sum(values: impl IntoIterator<Item = usize>) -> usize {
    values.into_iter().fold(0, usize::saturating_add)
}
fn json(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    match value {
        Value::String(value) => text(value),
        Value::Array(values) => vector(values).saturating_add(sum(values.iter().map(json))),
        Value::Object(values) => {
            map_storage::<String, Value>(values.len()).saturating_add(sum(values
                .iter()
                .map(|(name, value)| text(name).saturating_add(json(value)))))
        }
        _ => 0,
    }
}
fn expression(value: &Expr) -> usize {
    let children = match &value.kind {
        Expression::Number(_) | Expression::Bool(_) => 0,
        Expression::Text(value) | Expression::Name(value) => text(value),
        Expression::Unary(name, child) | Expression::Member(child, name) => {
            text(name).saturating_add(expression(child))
        }
        Expression::Binary(name, a, b) => sum([text(name), expression(a), expression(b)]),
        Expression::Call(name, arguments) => sum([
            text(name),
            vector(arguments),
            sum(arguments.iter().map(expression)),
        ]),
    };
    sum([size_of::<Expr>(), 32, text(&value.mark.module), children])
}
fn definition(value: &Definition) -> usize {
    let arguments = match value {
        Definition::Function { arguments, .. } => {
            vector(arguments).saturating_add(sum(arguments.iter().map(|(name, _)| text(name))))
        }
        Definition::Parameter { unit, label, .. } => {
            sum([optional_text(unit), optional_text(label)])
        }
        _ => 0,
    };
    sum([
        text(&value.mark().module),
        expression(value.value()),
        arguments,
    ])
}
impl Program {
    /// Conservative retained allocation charge, including every owned source copy,
    /// expression/source mark, metadata string and container capacity. Shared owners
    /// may charge this independently; it is an admission bound, not an allocator metric.
    pub fn retained_bytes(&self) -> usize {
        let metadata = &self.metadata;
        sum([
            size_of::<Self>(),
            text(&self.sources.entry),
            map_storage::<String, String>(self.sources.modules.len()),
            sum(self
                .sources
                .modules
                .iter()
                .map(|(name, source)| text(name).saturating_add(text(source)))),
            map_storage::<String, Definition>(self.definitions.len()),
            sum(self
                .definitions
                .iter()
                .map(|(name, value)| text(name).saturating_add(definition(value)))),
            map_storage::<String, Expr>(self.outputs.len()),
            sum(self
                .outputs
                .iter()
                .map(|(name, value)| text(name).saturating_add(expression(value)))),
            text(&metadata.entry),
            vector(&metadata.dependencies),
            sum(metadata.dependencies.iter().map(text)),
            vector(&metadata.parameters),
            sum(metadata.parameters.iter().map(|parameter| {
                sum([
                    text(&parameter.name),
                    text(&parameter.source.module),
                    json(&parameter.default),
                    optional_text(&parameter.unit),
                    optional_text(&parameter.label),
                ])
            })),
            vector(&metadata.fields),
            sum(metadata
                .fields
                .iter()
                .map(|field| text(&field.name).saturating_add(text(&field.source.module)))),
            vector(&metadata.outputs),
            sum(metadata.outputs.iter().map(text)),
            vector(&metadata.required_inputs),
            sum(metadata.required_inputs.iter().map(text)),
            self.compiled.retained,
            vector(&metadata.live),
            sum(metadata.live.iter().map(|operation| {
                sum([
                    text(&operation.id),
                    text(&operation.stage),
                    map_storage::<String, String>(operation.inputs.len()),
                    sum(operation
                        .inputs
                        .iter()
                        .map(|(argument, field)| text(argument).saturating_add(text(field)))),
                    vector(&operation.requires),
                    sum(operation.requires.iter().map(text)),
                    text(&operation.source.module),
                ])
            })),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materialspec::program::{Sources, compile};
    use std::collections::BTreeMap;

    #[test]
    fn retained_bytes_includes_parameter_annotations_in_both_owners() {
        let unit = "physical-unit".repeat(80);
        let label = "Public control label ".repeat(60);
        let source = format!(
            "import {{ defineMaterial, param, vec3 }} from \"@superspec/material/v1\";\n\
             const width = param.scalar({{ default: 0.1, unit: {unit:?}, label: {label:?} }});\n\
             export default defineMaterial({{ base: {{ baseColor: vec3(width) }} }});"
        );
        let mut program = compile(&Sources {
            entry: "main.material.ts".into(),
            modules: BTreeMap::from([("main.material.ts".into(), source)]),
        })
        .unwrap();
        let retained = program.retained_bytes();
        let Definition::Parameter { unit, label, .. } =
            program.definitions.get_mut("width").unwrap()
        else {
            panic!("width must remain a parameter");
        };
        let minimum = unit.take().unwrap().len() + label.take().unwrap().len();
        let without_definition_annotations = program.retained_bytes();
        assert!(retained - without_definition_annotations >= minimum);
        let parameter = &mut program.metadata.parameters[0];
        assert_eq!(parameter.name, "width");
        let minimum = parameter.unit.take().unwrap().len() + parameter.label.take().unwrap().len();
        assert!(without_definition_annotations - program.retained_bytes() >= minimum);
    }

    #[test]
    fn retained_bytes_includes_live_renderer_descriptors_and_their_owned_data() {
        let entry = format!(
            "studies/{}finish.material.ts",
            "long-module-name/".repeat(12)
        );
        let effects = std::iter::repeat_n("modulateDiffuse({ factor: 0.9, mask: 0.4 })", 24)
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "import {{ defineMaterial, modulateDiffuse, vec3 }} from \"@superspec/material/v1\";\n\
             export default defineMaterial({{ base: {{ baseColor: vec3(0.5) }}, live: [{effects}] }});"
        );
        let mut program = compile(&Sources {
            entry: entry.clone(),
            modules: BTreeMap::from([(entry, source)]),
        })
        .unwrap();
        assert_eq!(program.metadata.live.len(), 24);
        assert_eq!(
            program
                .metadata
                .fields
                .iter()
                .filter(|field| field.name.starts_with("live."))
                .count(),
            48
        );
        let retained = program.retained_bytes();
        let live = std::mem::take(&mut program.metadata.live);
        // The source and checked input-field expressions are still retained. Only the
        // renderer descriptors left the program, and their owned bytes must be charged.
        let owned_text: usize = live
            .iter()
            .map(|operation| {
                operation.id.len()
                    + operation.stage.len()
                    + operation.source.module.len()
                    + operation
                        .inputs
                        .iter()
                        .map(|(argument, field)| argument.len() + field.len())
                        .sum::<usize>()
                    + operation.requires.iter().map(String::len).sum::<usize>()
            })
            .sum();
        let minimum =
            owned_text + live.len() * size_of::<crate::materialspec::program::LiveOperation>();
        assert!(
            retained - program.retained_bytes() >= minimum,
            "live descriptors and their owned source, input, capability and identifier strings must be retained"
        );
    }
}
