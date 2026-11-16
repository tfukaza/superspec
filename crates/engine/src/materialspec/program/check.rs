use super::{
    super::catalog,
    syntax::{self, Definition, Expr, Expression, Mark},
    ts, *,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn compile(sources: &Sources) -> Result<Program, Vec<ProgramDiagnostic>> {
    let root = Mark::root(&sources.entry);
    if sources.modules.len() > 64
        || sources.modules.values().map(String::len).sum::<usize>() > 524_288
    {
        return Err(vec![diagnostic(
            sources,
            "MS_LIMIT",
            "Programs accept at most 64 modules and 512 KiB of source.",
            &root,
        )]);
    }
    if normalize("", &sources.entry).as_deref() != Some(sources.entry.as_str()) {
        return Err(vec![diagnostic(
            sources,
            "MS_IMPORT_PATH",
            "Entry must be a normalized relative module path.",
            &root,
        )]);
    }
    let mut program = Program {
        sources: sources.clone(),
        definitions: BTreeMap::new(),
        outputs: BTreeMap::new(),
        metadata: Metadata {
            version: 1,
            entry: sources.entry.clone(),
            dependencies: Vec::new(),
            parameters: Vec::new(),
            fields: Vec::new(),
            outputs: Vec::new(),
            required_inputs: Vec::new(),
            live: Vec::new(),
            externals: Vec::new(),
            named_features: Vec::new(),
            kind: RecipeKind::Material,
            double_sided: false,
            alpha_cutoff: None,
            size: None,
            blend: BTreeMap::new(),
            reads: Vec::new(),
        },
        compiled: Default::default(),
    };
    load(
        &mut program,
        &sources.entry,
        &root,
        &mut Vec::new(),
        &mut BTreeSet::new(),
    )?;
    if program.definitions.len() > 512 {
        return Err(vec![program.diagnostic(
            "MS_LIMIT",
            "Program exceeds 512 declarations.",
            &root,
        )]);
    }
    if program.metadata.kind == RecipeKind::Layer {
        // A layer changes some channels of the surface beneath and leaves the others alone.
        let foreign = program
            .outputs
            .iter()
            .find(|(name, _)| !LAYER_CHANNELS.contains(&name.as_str()));
        if let Some((name, expr)) = foreign {
            return Err(vec![program.diagnostic(
                "MS_OUTPUT",
                format!("A layer changes {}, not {name}.", LAYER_CHANNELS.join(", ")),
                &expr.mark,
            )]);
        }
        if program.outputs.is_empty() {
            return Err(vec![program.diagnostic(
                "MS_OUTPUT",
                "A layer must change at least one channel.",
                &root,
            )]);
        }
    } else if !program.outputs.contains_key("base_color") {
        return Err(vec![program.diagnostic(
            "MS_OUTPUT",
            "The entry module must declare base_color.",
            &root,
        )]);
    }
    let mut types = Checker {
        program: &program,
        stack: Vec::new(),
        types: BTreeMap::new(),
        externals: BTreeMap::new(),
        named_features: BTreeSet::new(),
        depth: 0,
    };
    let mut errors: Vec<ProgramDiagnostic> = Vec::new();
    let report = |errors: &mut Vec<ProgramDiagnostic>, error: ProgramDiagnostic| {
        let known = errors.iter().any(|known| {
            known.source.module == error.source.module
                && known.source.location.offset == error.source.location.offset
                && known.message == error.message
        });
        if !known && errors.len() < 64 {
            errors.push(error);
        }
    };
    for name in program.definitions.keys() {
        if let Err(error) = types.definition(name) {
            // A failed definition leaves partial traversal state behind.
            types.stack.clear();
            types.depth = 0;
            report(&mut errors, error);
        }
    }
    for (name, expr) in &program.outputs {
        let expected = match name.as_str() {
            "base_color" | "emissive" | "receiver_normal" | "normal_offset" => Type::Vec3,
            "roughness" | "metalness" | "height" | "opacity" => Type::Scalar,
            _ => {
                report(
                    &mut errors,
                    program.diagnostic("MS_OUTPUT", format!("Unknown output {name}."), &expr.mark),
                );
                continue;
            }
        };
        match types.expr(expr, &BTreeMap::new()) {
            Ok(kind) if kind == expected => {}
            Ok(_) => report(
                &mut errors,
                program.diagnostic(
                    "MS_TYPE",
                    format!("Output {name} requires {expected:?}."),
                    &expr.mark,
                ),
            ),
            Err(error) => {
                types.stack.clear();
                types.depth = 0;
                report(&mut errors, error);
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let externals: Vec<External> = types
        .externals
        .iter()
        .map(|(name, kind)| External {
            name: name.clone(),
            kind: *kind,
        })
        .collect();
    let named_features: Vec<String> = types.named_features.iter().cloned().collect();
    let inferred = types.types;
    let mut parameters = Vec::new();
    let mut fields = Vec::new();
    let mut required = BTreeSet::new();
    let mut helpers = BTreeSet::new();
    for (name, definition) in &program.definitions {
        match definition {
            Definition::Parameter {
                kind,
                value,
                range,
                unit,
                label,
                mark,
            } => {
                if !constant(value) {
                    return Err(vec![program.diagnostic(
                        "MS_PARAMETER",
                        "Parameter defaults must be constant numeric or boolean expressions.",
                        mark,
                    )]);
                }
                if range.is_some_and(|r| r[0] > r[1] || *kind == Type::Bool) {
                    return Err(vec![program.diagnostic(
                        "MS_PARAMETER",
                        "Parameter ranges require ordered numeric bounds.",
                        mark,
                    )]);
                }
                let default = eval::constant(&program, value).map_err(|error| vec![error])?;
                if !parameter_valid(*kind, &default, *range) {
                    return Err(vec![program.diagnostic(
                        "MS_PARAMETER",
                        format!("Default for {name} does not satisfy its type and range."),
                        mark,
                    )]);
                }
                parameters.push(Parameter {
                    name: name.clone(),
                    kind: *kind,
                    default,
                    min: range.map(|r| r[0]),
                    max: range.map(|r| r[1]),
                    unit: unit.clone(),
                    label: label.clone(),
                    source: program.diagnostic("", "", mark).source,
                });
            }
            Definition::Field { mark, .. } => fields.push(Field {
                name: name.clone(),
                kind: inferred[name],
                source: program.diagnostic("", "", mark).source,
            }),
            Definition::Function { .. } => {}
        }
        if matches!(definition, Definition::Field { .. }) {
            dependencies(&program, definition.value(), &mut required, &mut helpers);
        }
    }
    for expr in program.outputs.values() {
        dependencies(&program, expr, &mut required, &mut helpers);
    }
    program.metadata.alpha_cutoff = cutout(&program)?;
    if program.metadata.kind == RecipeKind::Layer {
        program.metadata.reads = externals
            .iter()
            .filter_map(|external| external.name.strip_prefix(BASE_PLANE))
            .filter(|channel| LAYER_CHANNELS.contains(channel))
            .map(str::to_owned)
            .collect();
    }
    program.metadata.parameters = parameters;
    program.metadata.fields = fields;
    program.metadata.outputs = program.outputs.keys().cloned().collect();
    program.metadata.required_inputs = required.into_iter().collect();
    program.metadata.externals = externals;
    program.metadata.named_features = named_features;
    program.metadata.dependencies.sort();
    program.compiled = std::sync::Arc::new(super::compiled::resolve(
        &program.definitions,
        &program.outputs,
    ));
    Ok(program)
}

/// The cutoff of the program's cutout, if its opacity is one. `alpha_cutout` declares how the
/// material is drawn, so it is valid only as the whole opacity channel (directly, or as the
/// field the channel names) with a constant cutoff in [0, 1].
fn cutout(program: &Program) -> Result<Option<f64>, Vec<ProgramDiagnostic>> {
    fn calls<'e>(expr: &'e Expr, found: &mut Vec<&'e Expr>) {
        match &expr.kind {
            Expression::Unary(_, a) | Expression::Member(a, _) => calls(a, found),
            Expression::Binary(_, a, b) => {
                calls(a, found);
                calls(b, found);
            }
            Expression::Call(name, args) => {
                if name == "alpha_cutout" {
                    found.push(expr);
                }
                for arg in args {
                    calls(arg, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for definition in program.definitions.values() {
        calls(definition.value(), &mut found);
    }
    for expr in program.outputs.values() {
        calls(expr, &mut found);
    }
    let Some(first) = found.first() else {
        return Ok(None);
    };
    // The opacity channel, through the fields that only name it.
    let mut opacity = program.outputs.get("opacity");
    for _ in 0..64 {
        match opacity.map(|expr| &expr.kind) {
            Some(Expression::Name(name)) => match program.definitions.get(name) {
                Some(Definition::Field { value, .. }) => opacity = Some(value),
                _ => break,
            },
            _ => break,
        }
    }
    let misplaced = |expr: &Expr| {
        vec![program.diagnostic(
            "MS_OUTPUT",
            "alpha_cutout declares how the material is drawn: it is valid only as the whole opacity channel.",
            &expr.mark,
        )]
    };
    let Some(opacity) = opacity else {
        return Err(misplaced(first));
    };
    if let Some(other) = found.iter().find(|call| !std::ptr::eq(**call, opacity)) {
        return Err(misplaced(other));
    }
    let Expression::Call(_, args) = &opacity.kind else {
        return Err(misplaced(first));
    };
    let out_of_range = || {
        vec![program.diagnostic(
            "MS_OUTPUT",
            "An alpha cutoff is a constant in [0, 1].",
            &args.get(1).unwrap_or(opacity).mark,
        )]
    };
    let Some(cutoff) = args.get(1).filter(|cutoff| constant(cutoff)) else {
        return Err(out_of_range());
    };
    let cutoff = eval::constant(program, cutoff)
        .map_err(|error| vec![error])?
        .as_f64()
        .filter(|cutoff| (0.0..=1.0).contains(cutoff))
        .ok_or_else(out_of_range)?;
    Ok(Some(cutoff))
}

pub(super) fn normalize(from: &str, path: &str) -> Option<String> {
    if path.is_empty() || path.len() > 1024 || path.starts_with('/') || path.contains(['\\', ':']) {
        return None;
    }
    let mut pieces: Vec<&str> = from
        .rsplit_once('/')
        .map_or(Vec::new(), |(p, _)| p.split('/').collect());
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                pieces.pop()?;
            }
            other => pieces.push(other),
        }
    }
    if pieces.is_empty() {
        None
    } else {
        Some(pieces.join("/"))
    }
}
fn load(
    program: &mut Program,
    module: &str,
    at: &Mark,
    stack: &mut Vec<String>,
    loaded: &mut BTreeSet<String>,
) -> Result<(), Vec<ProgramDiagnostic>> {
    if stack.iter().any(|p| p == module) {
        return Err(vec![program.diagnostic(
            "MS_IMPORT_CYCLE",
            format!("Import cycle: {} -> {module}.", stack.join(" -> ")),
            at,
        )]);
    }
    if loaded.contains(module) {
        return Ok(());
    }
    if !program.sources.modules.contains_key(module) {
        return Err(vec![program.diagnostic(
            "MS_IMPORT_MISSING",
            format!("Module '{module}' was not supplied."),
            at,
        )]);
    }
    stack.push(module.into());
    let parsed = if module.ends_with(".ts") {
        ts::parse(&program.sources, module)?
    } else {
        syntax::parse(&program.sources, module).map_err(|error| vec![error])?
    };
    for (path, mark) in parsed.imports {
        let resolved = normalize(module, &path).ok_or_else(|| {
            vec![program.diagnostic(
                "MS_IMPORT_PATH",
                "Import escapes the module root or is not relative.",
                &mark,
            )]
        })?;
        if module.ends_with(".ts") != resolved.ends_with(".ts") {
            return Err(vec![program.diagnostic(
                "MS_IMPORT_LANGUAGE",
                "Imports must use the same material language as their containing module.",
                &mark,
            )]);
        }
        load(program, &resolved, &mark, stack, loaded)?;
    }
    let mut errors = Vec::new();
    for (name, definition) in parsed.declarations {
        if builtin_name(&name)
            || variable(&name).is_some()
            || program.definitions.contains_key(&name)
        {
            errors.push(program.diagnostic(
                "MS_DUPLICATE",
                format!("Declaration '{name}' conflicts with an existing binding."),
                definition.mark(),
            ));
            continue;
        }
        program.definitions.insert(name, definition);
    }
    for (name, expr) in parsed.outputs {
        if module != program.sources.entry {
            errors.push(program.diagnostic("MS_OUTPUT","Imported modules may define helpers, fields and parameters, but only the entry declares outputs.",&expr.mark));
            continue;
        }
        if program.outputs.contains_key(&name) {
            errors.push(program.diagnostic(
                "MS_DUPLICATE",
                format!("Output '{name}' is declared twice."),
                &expr.mark,
            ));
            continue;
        }
        program.outputs.insert(name, expr);
    }
    if !parsed.live.is_empty() && module != program.sources.entry {
        errors.push(program.diagnostic(
            "MS_OUTPUT",
            "Only the entry module declares live renderer operations.",
            at,
        ));
    }
    program.metadata.live.extend(parsed.live);
    if module == program.sources.entry {
        let traits = parsed.traits;
        program.metadata.kind = traits.kind;
        program.metadata.double_sided = traits.double_sided;
        program.metadata.size = traits.size;
        program.metadata.blend = traits.blend;
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    stack.pop();
    loaded.insert(module.into());
    if module != program.sources.entry {
        program.metadata.dependencies.push(module.into());
    }
    Ok(())
}
pub(super) fn parameter_valid(
    kind: Type,
    value: &serde_json::Value,
    range: Option<[f64; 2]>,
) -> bool {
    let numeric = |v: &serde_json::Value| {
        v.as_f64()
            .is_some_and(|v| v.is_finite() && range.is_none_or(|r| v >= r[0] && v <= r[1]))
    };
    match kind {
        Type::Bool => value.is_boolean(),
        Type::Scalar => numeric(value),
        Type::Vec2 | Type::Vec3 => value.as_array().is_some_and(|v| {
            v.len() == if kind == Type::Vec2 { 2 } else { 3 } && v.iter().all(numeric)
        }),
        Type::String => false,
    }
}
pub(super) fn variable(name: &str) -> Option<Type> {
    match name {
        "uv" | "surface" => Some(Type::Vec2),
        "object" | "world" | "normal" | "gravity" | "bounds_min" | "bounds_max" => Some(Type::Vec3),
        "footprint" | "uv_footprint" => Some(Type::Scalar),
        // Render-only: what a renderer supplies where the material is drawn.
        "time" | "instance_w" => Some(Type::Scalar),
        "instance" => Some(Type::Vec3),
        _ => None,
    }
}
fn numeric(t: Type) -> bool {
    matches!(t, Type::Scalar | Type::Vec2 | Type::Vec3)
}
fn broadcast(a: Type, b: Type) -> Option<Type> {
    if numeric(a) && numeric(b) {
        if a == b || b == Type::Scalar {
            Some(a)
        } else if a == Type::Scalar {
            Some(b)
        } else {
            None
        }
    } else {
        None
    }
}
fn builtin_name(name: &str) -> bool {
    catalog::find(name).is_some()
}
fn builtin(name: &str, args: &[Type]) -> Option<Type> {
    catalog::resolve(name, args)
}
/// External inputs one program reads at most.
pub(super) const MAX_EXTERNALS: usize = 16;
/// External inputs a layered slot's composite reads at most: its channels and the base's live
/// inputs, which an author never writes.
pub(super) const MAX_COMPOSITE_EXTERNALS: usize = 64;
/// Feature names one program queries at most.
pub(super) const MAX_NAMED_QUERIES: usize = 32;
struct Checker<'a> {
    program: &'a Program,
    stack: Vec<String>,
    types: BTreeMap<String, Type>,
    /// The type each external input is read as.
    externals: BTreeMap<String, Type>,
    /// The feature names queried.
    named_features: BTreeSet<String>,
    depth: usize,
}
impl Checker<'_> {
    /// External inputs this program may read: a layered slot's composite reads more.
    fn most_externals(&self) -> usize {
        if self.program.metadata.entry == super::LAYERED_COMPOSITE {
            MAX_COMPOSITE_EXTERNALS
        } else {
            MAX_EXTERNALS
        }
    }

    /// Notes one `external(name, fallback)` of type `kind`. A name stands for one plane, so its
    /// uses agree on a type, and a program reads a bounded number of them.
    fn external(&mut self, name: &str, kind: Type, mark: &Mark) -> Result<(), ProgramDiagnostic> {
        if name.len() > 64
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        {
            return Err(self.program.diagnostic(
                "MS_TYPE",
                "External input names are at most 64 letters, digits, '.', '_' or '-'.",
                mark,
            ));
        }
        match self.externals.get(name) {
            Some(known) if *known != kind => Err(self.program.diagnostic(
                "MS_TYPE",
                format!(
                    "External input '{name}' is read as {known:?} elsewhere and as {kind:?} here."
                ),
                mark,
            )),
            Some(_) => Ok(()),
            None if self.externals.len() >= self.most_externals() => Err(self.program.diagnostic(
                "MS_LIMIT",
                format!(
                    "A program reads at most {} external inputs.",
                    self.most_externals()
                ),
                mark,
            )),
            None => {
                self.externals.insert(name.to_owned(), kind);
                Ok(())
            }
        }
    }
    fn definition(&mut self, name: &str) -> Result<Type, ProgramDiagnostic> {
        if let Some(kind) = self.types.get(name) {
            return Ok(*kind);
        }
        let definition = self.program.definitions[name].clone();
        if self.stack.iter().any(|n| n == name) {
            return Err(self.program.diagnostic(
                "MS_CYCLE",
                format!(
                    "Recursive field/helper dependency: {} -> {name}.",
                    self.stack.join(" -> ")
                ),
                definition.mark(),
            ));
        }
        if self.stack.len() >= 48 {
            return Err(self.program.diagnostic(
                "MS_LIMIT",
                "Helper dependency depth exceeds 48.",
                definition.mark(),
            ));
        }
        self.stack.push(name.into());
        let (value, expected, locals) = match &definition {
            Definition::Parameter { value, kind, .. } => (value, Some(*kind), BTreeMap::new()),
            Definition::Field { value, kind, .. } => (value, *kind, BTreeMap::new()),
            Definition::Function {
                arguments,
                result,
                value,
                mark,
            } => {
                let mut locals = BTreeMap::new();
                for (name, kind) in arguments {
                    if locals.insert(name.clone(), *kind).is_some() || variable(name).is_some() {
                        return Err(self.program.diagnostic(
                            "MS_DUPLICATE",
                            "Helper arguments must be unique and cannot shadow sample inputs.",
                            mark,
                        ));
                    }
                }
                (value, Some(*result), locals)
            }
        };
        let actual = self.expr(value, &locals)?;
        if actual == Type::String {
            return Err(self.program.diagnostic("MS_TYPE","Fields cannot store strings; image IDs and named seeds are literal call arguments.",&value.mark));
        }
        if expected.is_some_and(|t| t != actual) {
            return Err(self.program.diagnostic(
                "MS_TYPE",
                format!("Declaration '{name}' has type {actual:?}, expected {expected:?}."),
                &value.mark,
            ));
        }
        self.stack.pop();
        self.types.insert(name.into(), actual);
        Ok(actual)
    }
    fn expr(
        &mut self,
        expr: &Expr,
        locals: &BTreeMap<String, Type>,
    ) -> Result<Type, ProgramDiagnostic> {
        self.depth += 1;
        if self.depth > 128 {
            return Err(self.program.diagnostic(
                "MS_LIMIT",
                "Type checking exceeds its expression dependency depth.",
                &expr.mark,
            ));
        }
        let result = self.expression(expr, locals);
        self.depth -= 1;
        result
    }
    fn expression(
        &mut self,
        expr: &Expr,
        locals: &BTreeMap<String, Type>,
    ) -> Result<Type, ProgramDiagnostic> {
        let bad = || {
            self.program.diagnostic(
                "MS_TYPE",
                "Expression arguments have incompatible types.",
                &expr.mark,
            )
        };
        match &expr.kind {
            Expression::Number(_) => Ok(Type::Scalar),
            Expression::Bool(_) => Ok(Type::Bool),
            Expression::Text(_) => Ok(Type::String),
            Expression::Name(name) => {
                if let Some(kind) = locals.get(name).copied().or_else(|| variable(name)) {
                    return Ok(kind);
                }
                if let Some(definition) = self.program.definitions.get(name) {
                    if matches!(definition, Definition::Function { .. }) {
                        return Err(self.program.diagnostic(
                            "MS_TYPE",
                            "Helper functions must be called.",
                            &expr.mark,
                        ));
                    }
                    return self.definition(name);
                }
                Err(self.program.diagnostic(
                    "MS_NAME",
                    format!("Unknown binding '{name}'."),
                    &expr.mark,
                ))
            }
            Expression::Unary(op, value) => {
                let kind = self.expr(value, locals)?;
                if (op == "!" && kind == Type::Bool) || (op != "!" && numeric(kind)) {
                    Ok(kind)
                } else {
                    Err(bad())
                }
            }
            Expression::Binary(op, a, b) => {
                let a = self.expr(a, locals)?;
                let b = self.expr(b, locals)?;
                match op.as_str() {
                    "&&" | "||" if a == Type::Bool && b == Type::Bool => Ok(Type::Bool),
                    "==" | "!=" if a == b && a != Type::String => Ok(Type::Bool),
                    "<" | ">" | "<=" | ">=" if a == Type::Scalar && b == Type::Scalar => {
                        Ok(Type::Bool)
                    }
                    "+" | "-" | "*" | "/" | "%" => broadcast(a, b).ok_or_else(bad),
                    _ => Err(bad()),
                }
            }
            Expression::Member(value, member) => {
                let kind = self.expr(value, locals)?;
                let size = match kind {
                    Type::Vec2 => 2,
                    Type::Vec3 => 3,
                    _ => return Err(bad()),
                };
                if member.is_empty()
                    || member.len() > 3
                    || !member
                        .chars()
                        .all(|c| component(c).is_some_and(|i| i < size))
                {
                    return Err(bad());
                }
                Ok(match member.len() {
                    1 => Type::Scalar,
                    2 => Type::Vec2,
                    _ => Type::Vec3,
                })
            }
            Expression::Call(name, args) => {
                let kinds = args
                    .iter()
                    .map(|e| self.expr(e, locals))
                    .collect::<Result<Vec<_>, _>>()?;
                if builtin_name(name) {
                    if catalog::find(name).is_some_and(|operation| operation.literal_first)
                        && !matches!(args.first().map(|v|&v.kind),Some(Expression::Text(s)) if !s.is_empty()&&s.len()<=1024)
                    {
                        return Err(self.program.diagnostic("MS_TYPE","Image IDs and seed names require a nonempty literal string of at most 1024 bytes.",&expr.mark));
                    }
                    if let (Some(operation), Some(Expression::Text(word))) =
                        (catalog::find(name), args.first().map(|v| &v.kind))
                    {
                        if !operation.literal_values.is_empty()
                            && !operation.literal_values.contains(&word.as_str())
                        {
                            return Err(self.program.diagnostic(
                                "MS_TYPE",
                                format!(
                                    "{name} selects one of {}, not '{word}'.",
                                    operation.literal_values.join(", ")
                                ),
                                &expr.mark,
                            ));
                        }
                    }
                    let result = builtin(name, &kinds).ok_or_else(|| {
                        self.program.diagnostic(
                            "MS_TYPE",
                            format!("Invalid arguments for {name}: {kinds:?}."),
                            &expr.mark,
                        )
                    })?;
                    if let ("external", Some(Expression::Text(plane))) =
                        (name.as_str(), args.first().map(|v| &v.kind))
                    {
                        self.external(plane, result, &expr.mark)?;
                    }
                    if let (
                        "feature_distance" | "feature_tangent",
                        Some(Expression::Text(feature)),
                    ) = (name.as_str(), args.first().map(|v| &v.kind))
                    {
                        if !super::features::valid_feature_name(feature) {
                            return Err(self.program.diagnostic(
                                "MS_TYPE",
                                "Feature names are at most 64 letters, digits, '.', '_' or '-'.",
                                &expr.mark,
                            ));
                        }
                        if !self.named_features.contains(feature)
                            && self.named_features.len() >= MAX_NAMED_QUERIES
                        {
                            return Err(self.program.diagnostic(
                                "MS_LIMIT",
                                format!("A program queries features of at most {MAX_NAMED_QUERIES} names."),
                                &expr.mark,
                            ));
                        }
                        self.named_features.insert(feature.clone());
                    }
                    if let (
                        "instance_attribute" | "vertex_attribute",
                        Some(Expression::Text(attribute)),
                    ) = (name.as_str(), args.first().map(|v| &v.kind))
                    {
                        let bytes = attribute.as_bytes();
                        if bytes.len() > 64
                            || !bytes[0].is_ascii_alphabetic()
                            || !bytes.iter().all(|c| c.is_ascii_alphanumeric())
                        {
                            return Err(self.program.diagnostic(
                                "MS_TYPE",
                                "Attribute names are at most 64 letters and digits, starting with a letter.",
                                &expr.mark,
                            ));
                        }
                    }
                    return Ok(result);
                }
                let Some(Definition::Function {
                    arguments, result, ..
                }) = self.program.definitions.get(name)
                else {
                    return Err(self.program.diagnostic(
                        "MS_NAME",
                        format!("Unknown helper '{name}'."),
                        &expr.mark,
                    ));
                };
                if arguments.iter().map(|(_, t)| *t).collect::<Vec<_>>() != kinds {
                    return Err(bad());
                }
                let result = *result;
                self.definition(name)?;
                Ok(result)
            }
        }
    }
}
pub(super) fn component(c: char) -> Option<usize> {
    match c {
        'x' | 'r' => Some(0),
        'y' | 'g' => Some(1),
        'z' | 'b' => Some(2),
        _ => None,
    }
}
fn constant(expr: &Expr) -> bool {
    match &expr.kind {
        Expression::Number(_) | Expression::Bool(_) => true,
        Expression::Unary(_, a) | Expression::Member(a, _) => constant(a),
        Expression::Binary(_, a, b) => constant(a) && constant(b),
        Expression::Call(name, args) => {
            catalog::find(name).is_some_and(|operation| operation.constant)
                && args.iter().all(constant)
        }
        _ => false,
    }
}
fn dependencies(
    program: &Program,
    expr: &Expr,
    inputs: &mut BTreeSet<String>,
    helpers: &mut BTreeSet<String>,
) {
    match &expr.kind {
        Expression::Name(name) if variable(name).is_some() => {
            inputs.insert(name.clone());
        }
        Expression::Unary(_, a) | Expression::Member(a, _) => {
            dependencies(program, a, inputs, helpers)
        }
        Expression::Binary(_, a, b) => {
            dependencies(program, a, inputs, helpers);
            dependencies(program, b, inputs, helpers);
        }
        Expression::Call(name, args) => {
            if let Some(Definition::Function { value, .. }) = program.definitions.get(name) {
                if helpers.insert(name.clone()) {
                    dependencies(program, value, inputs, helpers);
                }
            }
            if let Some(operation) = catalog::find(name) {
                inputs.extend(operation.requires.iter().map(|input| (*input).to_owned()));
                if operation.image_input {
                    if let Some(Expr {
                        kind: Expression::Text(image),
                        ..
                    }) = args.first()
                    {
                        inputs.insert(format!("image:{image}"));
                    }
                }
                if matches!(operation.id, "feature_distance" | "feature_tangent") {
                    if let Some(Expr {
                        kind: Expression::Text(feature),
                        ..
                    }) = args.first()
                    {
                        inputs.insert(format!("feature:{feature}"));
                    }
                }
                if operation.id == "external" {
                    if let Some(Expr {
                        kind: Expression::Text(plane),
                        ..
                    }) = args.first()
                    {
                        inputs.insert(format!("plane:{plane}"));
                    }
                }
            }
            for arg in args {
                dependencies(program, arg, inputs, helpers);
            }
        }
        _ => {}
    }
}
