//! Lexical recipe bindings lower to identities in the shared MaterialSpec symbol table.
use super::{
    Definition, Expr, Expression, Init, Mark, Module, Node, NodeKind, ProgramDiagnostic, Sources,
    Statement, catalog, diagnostic, is_call_to, parse_statements,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn is_parameter(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Call(callee, _) if matches!(&callee.kind,
        NodeKind::Member(base, _) if matches!(&base.kind, NodeKind::Name(name) if name == "param")))
}

fn identity(module: &str, name: &str, sources: &Sources, public_parameter: bool) -> String {
    if module == sources.entry || public_parameter {
        name.into()
    } else {
        format!("{module}::{name}")
    }
}

pub(super) fn imports(
    sources: &Sources,
    module: &str,
    path: &str,
    names: &[(String, Mark)],
    at: &Mark,
) -> Result<Vec<(String, String)>, Vec<ProgramDiagnostic>> {
    let target = super::super::check::normalize(module, path).ok_or_else(|| {
        vec![diagnostic(
            sources,
            "MS_IMPORT_PATH",
            "Import escapes the module root or is not relative.",
            at,
        )]
    })?;
    if !sources.modules.contains_key(&target) {
        return Err(vec![diagnostic(
            sources,
            "MS_IMPORT_MISSING",
            format!("Module '{target}' was not supplied."),
            at,
        )]);
    }
    let statements = parse_statements(sources, &target).map_err(|error| vec![error])?;
    let mut result = Vec::new();
    let mut errors = Vec::new();
    for (name, at) in names {
        let exported = statements.iter().find_map(|statement| match statement {
            Statement::Const {
                name: found,
                exported: true,
                init,
                ..
            } if found == name => Some(init),
            _ => None,
        });
        let Some(init) = exported else {
            errors.push(diagnostic(
                sources,
                "MS_IMPORT_NAME",
                format!("Module '{target}' does not export '{name}'."),
                at,
            ));
            continue;
        };
        if let Init::Expr(node) = init {
            let group = matches!(&node.kind, NodeKind::Call(callee, _) if matches!(&callee.kind, NodeKind::Name(name) if catalog::find_group(name).is_some()));
            if is_call_to(node, "layer").is_some() || group {
                errors.push(diagnostic(sources, "MT_SUBSET", "Relative imports support fields, parameters and helpers; layer and group objects must be declared in the consuming module.", at));
                continue;
            }
        }
        let public_parameter = matches!(init, Init::Expr(node) if is_parameter(node));
        result.push((
            name.clone(),
            identity(&target, name, sources, public_parameter),
        ));
    }
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors)
    }
}

pub(super) fn qualify(
    module: &mut Module,
    name: &str,
    sources: &Sources,
    imports: &BTreeMap<String, String>,
) {
    let mut bindings = imports.clone();
    for (binding, definition) in &module.declarations {
        // Parameters are host controls, with globally unique IDs independent of TS visibility.
        bindings.insert(
            binding.clone(),
            identity(
                name,
                binding,
                sources,
                matches!(definition, Definition::Parameter { .. }),
            ),
        );
    }
    for (binding, definition) in &mut module.declarations {
        *binding = bindings[binding].clone();
        let (value, locals) = match definition {
            Definition::Function {
                value, arguments, ..
            } => (
                value,
                arguments.iter().map(|(name, _)| name.as_str()).collect(),
            ),
            Definition::Parameter { value, .. } | Definition::Field { value, .. } => {
                (value, BTreeSet::new())
            }
        };
        qualify_expression(value, &bindings, &locals);
    }
    for (_, expression) in &mut module.outputs {
        qualify_expression(expression, &bindings, &BTreeSet::new());
    }
}

fn qualify_expression(
    expression: &mut Expr,
    bindings: &BTreeMap<String, String>,
    locals: &BTreeSet<&str>,
) {
    match &mut expression.kind {
        Expression::Name(name) => {
            if !locals.contains(name.as_str()) {
                if let Some(replacement) = bindings.get(name) {
                    *name = replacement.clone();
                }
            }
        }
        Expression::Call(name, args) => {
            if let Some(replacement) = bindings.get(name) {
                *name = replacement.clone();
            }
            for argument in args {
                qualify_expression(argument, bindings, locals);
            }
        }
        Expression::Unary(_, argument) | Expression::Member(argument, _) => {
            qualify_expression(argument, bindings, locals)
        }
        Expression::Binary(_, left, right) => {
            qualify_expression(left, bindings, locals);
            qualify_expression(right, bindings, locals);
        }
        _ => {}
    }
}
