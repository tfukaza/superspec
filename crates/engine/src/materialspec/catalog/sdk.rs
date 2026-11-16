//! Generated SDK artifacts: strict TypeScript declarations, port data and the AI reference.
use super::*;
use std::fmt::Write;

/// Files an author or agent workspace needs to type-check recipes.
pub struct SdkFile {
    pub name: &'static str,
    pub contents: String,
}

pub fn files() -> Vec<SdkFile> {
    vec![
        SdkFile {
            name: "index.d.ts",
            contents: declarations(),
        },
        SdkFile {
            name: "ports.json",
            contents: ports_json(),
        },
        SdkFile {
            name: "ai-reference.md",
            contents: reference(),
        },
        SdkFile {
            name: "tsconfig.json",
            contents: tsconfig(),
        },
    ]
}

fn input_type(kind: Type) -> &'static str {
    match kind {
        Type::Scalar => "ScalarInput",
        Type::Vec2 => "Vec2",
        Type::Vec3 => "Vec3",
        Type::Bool => "BoolInput",
        Type::String => "string",
    }
}

fn result_type(kind: Type) -> &'static str {
    match kind {
        Type::Scalar => "Scalar",
        Type::Vec2 => "Vec2",
        Type::Vec3 => "Vec3",
        Type::Bool => "Bool",
        Type::String => "string",
    }
}

fn parameter_names(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| ((b'a' + index as u8) as char).to_string())
        .collect()
}

/// Operations whose vector result is a color or weights rather than a value in the space of
/// their coordinate arguments.
const UNSPACED_RESULTS: [&str; 4] = [
    "sampleColor",
    "sampleData",
    "variedColor",
    "triplanarWeights",
];

/// The coordinate space each vector sample input is declared in.
fn input_space(ts_name: &str) -> &'static str {
    match ts_name {
        "uv" => "uv",
        "surface" => "surface",
        "object" => "object",
        "normal" | "gravity" | "sunDirection" => "direction",
        "instance" | "instanceTint" => "none",
        _ => "world",
    }
}

fn spaced(kind: Type, space: &str) -> String {
    match kind {
        Type::Vec2 | Type::Vec3 => format!("{}<{space}>", result_type(kind)),
        _ => result_type(kind).into(),
    }
}

/// `literals` narrows a string argument to the words the checker accepts. Every vector argument
/// gets its own space parameter; each after the first must combine with the ones before it, and
/// a vector result carries the combined space.
fn signature(name: &str, overload: &Overload, literals: &[&str]) -> String {
    let mut spaces = Vec::new();
    let mut combined = String::new();
    let parameters = overload
        .args
        .iter()
        .zip(parameter_names(overload.args.len()))
        .map(|(kind, label)| {
            if *kind == Type::String && !literals.is_empty() {
                let words: Vec<String> = literals.iter().map(|w| format!("\"{w}\"")).collect();
                format!("{label}: {}", words.join(" | "))
            } else if matches!(kind, Type::Vec2 | Type::Vec3) {
                let space = format!("S{}", spaces.len());
                spaces.push(format!("{space} extends Space"));
                let vector = spaced(*kind, &space);
                if combined.is_empty() {
                    combined = space;
                    format!("{label}: {vector}")
                } else {
                    let check = format!("{label}: {vector} & Check<{combined}, {space}>");
                    combined = format!("Combine<{combined}, {space}>");
                    check
                }
            } else {
                format!("{label}: {}", input_type(*kind))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let result = if combined.is_empty() || UNSPACED_RESULTS.contains(&name) {
        spaced(overload.result, "\"none\"")
    } else {
        spaced(overload.result, &combined)
    };
    let generics = if spaces.is_empty() {
        String::new()
    } else {
        format!("<{}>", spaces.join(", "))
    };
    format!("export declare function {name}{generics}({parameters}): {result};")
}

fn swizzles(size: usize, letters: &str) -> Vec<String> {
    let letters: Vec<char> = letters.chars().take(size).collect();
    let mut all = Vec::new();
    for length in 1..=3 {
        let mut combos = vec![String::new()];
        for _ in 0..length {
            combos = combos
                .into_iter()
                .flat_map(|prefix| {
                    letters.iter().map(move |letter| {
                        let mut next = prefix.clone();
                        next.push(*letter);
                        next
                    })
                })
                .collect();
        }
        all.extend(combos);
    }
    all
}

fn vector_interface(name: &str, size: usize) -> String {
    let mut out = format!(
        "/** A {size}-component vector in coordinate space `S`. */\nexport interface {name}<S extends Space = Space> {{\n  readonly [kind]: \"{}\";\n  readonly [space]: S;\n",
        name.to_lowercase()
    );
    for letters in ["xyz", "rgb"] {
        for member in swizzles(size, letters) {
            let kind = match member.len() {
                1 => Type::Scalar,
                2 => Type::Vec2,
                _ => Type::Vec3,
            };
            let _ = writeln!(out, "  readonly {member}: {};", spaced(kind, "S"));
        }
    }
    out.push_str("}\n");
    out
}

fn group_options(group: &Group) -> String {
    let fields = group
        .arguments
        .iter()
        .map(|argument| {
            let optional = if argument.default.is_some() { "?" } else { "" };
            format!(
                "    /** {}{} */\n    readonly {}{optional}: {};",
                argument.summary,
                if argument.unit.is_empty() {
                    String::new()
                } else {
                    format!(" Unit: {}.", argument.unit)
                },
                argument.name,
                input_type(argument.kind)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{{\n{fields}\n  }}")
}

pub(super) fn pascal(id: &str) -> String {
    let mut characters = id.chars();
    characters
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
        .unwrap_or_default()
}

pub fn declarations() -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated from the MaterialSpec operation catalog v{CATALOG_VERSION}. Do not edit.\n\
         // Recipes import from \"{SDK_MODULE}\". The compiler parses a documented TypeScript subset;\n\
         // this file only types it, so authored code never runs.\n\
         // `export {{}}` keeps the brand symbol private, so recipes cannot forge branded values.\n\
         export {{}};\n\
         declare const kind: unique symbol;\n\
         declare const space: unique symbol;\n\n\
         /**\n\
         \x20* Coordinate spaces of vectors. `\"none\"` marks literals, parameters and colors; `\"direction\"`\n\
         \x20* (normals, gravity) combines with any position space. The compiler ignores spaces; only\n\
         \x20* TypeScript checks them. A plain `Vec3` accepts every space but its result has none in\n\
         \x20* particular, so helpers that keep a space are generic: `<S extends Space>(p: Vec3<S>): Vec3<S>`.\n\
         \x20*/\n\
         export type Space = {spaces};\n\
         /** The space shared by two operands, or `never` when they conflict. */\n\
         type Combine<A extends Space, B extends Space> =\n\
         \x20 [A] extends [\"none\"] ? B : [B] extends [\"none\"] ? A :\n\
         \x20 [A] extends [\"direction\"] ? B : [B] extends [\"direction\"] ? A :\n\
         \x20 [A] extends [B] ? ([B] extends [A] ? A : never) : never;\n\
         /** Never satisfiable, so a conflicting operand reports both spaces. */\n\
         interface SpaceMismatch<A, B> {{\n\
         \x20 readonly spaceMismatch: [A, B];\n\
         }}\n\
         type Check<A extends Space, B extends Space> = [Combine<A, B>] extends [never] ? SpaceMismatch<A, B> : unknown;\n",
        spaces = SPACES
            .iter()
            .map(|space| format!("\"{space}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    out.push_str("export interface Scalar {\n  readonly [kind]: \"scalar\";\n}\n");
    out.push_str("export interface Bool {\n  readonly [kind]: \"bool\";\n}\n");
    out.push_str(&vector_interface("Vec2", 2));
    out.push_str(&vector_interface("Vec3", 3));
    out.push_str(
        "export type ScalarInput = Scalar | number;\nexport type BoolInput = Bool | boolean;\n\n",
    );
    out.push_str("// Sample inputs supplied by the host.\n");
    for input in inputs() {
        let _ = writeln!(
            out,
            "/** {} */\nexport declare const {}: {};",
            input.summary,
            input.ts_name,
            spaced(input.kind, &format!("\"{}\"", input_space(input.ts_name)))
        );
    }
    out.push_str("\n// Parameters. The constant name is the stable parameter ID.\n");
    out.push_str(
        "export declare const param: {\n\
         \x20 scalar(options: { readonly default: number; readonly range?: readonly [number, number]; readonly unit?: string; readonly label?: string }): Scalar;\n\
         \x20 vec2(options: { readonly default: Vec2; readonly range?: readonly [number, number]; readonly unit?: string; readonly label?: string }): Vec2<\"none\">;\n\
         \x20 vec3(options: { readonly default: Vec3; readonly range?: readonly [number, number]; readonly unit?: string; readonly label?: string }): Vec3<\"none\">;\n\
         \x20 bool(options: { readonly default: boolean; readonly label?: string }): Bool;\n\
         };\n\n",
    );
    out.push_str(
        "// Operators. Recipes call these because branded values do not support infix operators.\n",
    );
    for operator in operators() {
        let _ = writeln!(out, "/** {} */", operator.summary);
        for overload in &operator.overloads {
            let _ = writeln!(out, "{}", signature(operator.id, overload, &[]));
        }
    }
    out.push_str("\n// Primitives.\n");
    for operation in operations() {
        let name = ts_name(operation.id);
        let _ = writeln!(
            out,
            "/**\n * {}\n * @example {}\n */",
            operation.summary, operation.example
        );
        for overload in &operation.overloads {
            let _ = writeln!(
                out,
                "{}",
                signature(&name, overload, operation.literal_values)
            );
        }
    }
    out.push_str(
        "\n// Editable groups. Each exposes named fields that can be inspected and reused.\n",
    );
    for group in groups() {
        let name = pascal(group.id) + "Group";
        let members = group
            .outputs
            .iter()
            .map(|output| {
                format!(
                    "  /** {} */\n  readonly {}: {};",
                    output.summary,
                    output.name,
                    spaced(output.kind, "\"none\"")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = writeln!(
            out,
            "export interface {name} {{\n  readonly [kind]: \"{}\";\n{members}\n}}",
            group.id
        );
        let all_optional = group
            .arguments
            .iter()
            .all(|argument| argument.default.is_some());
        let unavailable = group
            .unavailable
            .map(|reason| format!(" Unavailable: {reason}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "/**\n * {}{unavailable}\n * @example {}\n */\nexport declare function {}(options{}: {}): {name};",
            group.summary,
            group.example,
            group.id,
            if all_optional { "?" } else { "" },
            group_options(group)
        );
    }
    out.push_str(
        "\n// Live renderer operations. Recorded in the material and consumed by the renderer.\n",
    );
    out.push_str("export interface LiveEffect {\n  readonly [kind]: \"live\";\n  readonly operation: string;\n}\n");
    for operation in renderer_operations() {
        let fields = operation
            .arguments
            .iter()
            .map(|argument| {
                let optional = if argument.default.is_some() { "?" } else { "" };
                format!(
                    "    readonly {}{optional}: {};",
                    argument.name,
                    input_type(argument.kind)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = writeln!(
            out,
            "/**\n * {} Stage: {}. Renderer supplies: {}.\n * @example {}\n */\nexport declare function {}(options: {{\n{fields}\n  }}): LiveEffect;",
            operation.summary,
            operation.stage,
            operation.requires.join(", "),
            operation.example,
            operation.id
        );
    }
    out.push_str(
        "\n// Material recipe.\n\
         export type HeightEdit =\n\
         \x20 | { readonly add: ScalarInput; readonly replace?: never }\n\
         \x20 | { readonly replace: ScalarInput; readonly add?: never };\n\
         export interface BaseChannels {\n\
         \x20 readonly baseColor: Vec3;\n\
         \x20 readonly roughness?: ScalarInput;\n\
         \x20 readonly metalness?: ScalarInput;\n\
         \x20 readonly height?: ScalarInput;\n\
         \x20 readonly emissive?: Vec3;\n\
         \x20 readonly opacity?: ScalarInput;\n\
         \x20 /** World-space shading normal the height relief is applied around; defaults to the receiver normal. */\n\
         \x20 readonly receiverNormal?: Vec3;\n\
         \x20 /** World-space vector added to the shaded normal after the height relief, before renormalizing. */\n\
         \x20 readonly normalOffset?: Vec3;\n\
         }\n\
         export interface Layer {\n\
         \x20 readonly [kind]: \"layer\";\n\
         }\n\
         export interface LayerOptions {\n\
         \x20 /** Stable identifier; names the layer's inspectable fields. */\n\
         \x20 readonly id: string;\n\
         \x20 /** Coverage, clamped to [0, 1]. */\n\
         \x20 readonly mask: ScalarInput;\n\
         \x20 readonly baseColor?: Vec3;\n\
         \x20 readonly roughness?: ScalarInput;\n\
         \x20 readonly metalness?: ScalarInput;\n\
         \x20 readonly emissive?: Vec3;\n\
         \x20 readonly opacity?: ScalarInput;\n\
         \x20 readonly height?: HeightEdit;\n\
         \x20 /** Turns the shading normal toward this direction by the layer's coverage. */\n\
         \x20 readonly receiverNormal?: Vec3;\n\
         \x20 /** Blends the post-relief normal offset toward this vector by the layer's coverage. */\n\
         \x20 readonly normalOffset?: Vec3;\n\
         }\n\
         /** An ordered masked layer; omitted channels pass through and ordinary channels blend in linear space. */\n\
         export declare function layer(options: LayerOptions): Layer;\n\
         export interface MaterialOptions {\n\
         \x20 readonly name?: string;\n\
         \x20 readonly base: BaseChannels;\n\
         \x20 readonly layers?: readonly Layer[];\n\
         \x20 /** Direct expressions that replace a channel's final value. */\n\
         \x20 readonly channels?: Partial<Omit<BaseChannels, \"baseColor\">> & { readonly baseColor?: Vec3 };\n\
         \x20 readonly live?: readonly LiveEffect[];\n\
         \x20 /** Draw the surface from both sides. */\n\
         \x20 readonly doubleSided?: boolean;\n\
         \x20 /** lit (the default) is shaded by the scene's lights; painted shows `paint` without the light loop, and the program is delivered with the material (its baked maps are the fallback). */\n\
         \x20 readonly shading?: \"lit\" | \"painted\";\n\
         \x20 /** The linear color a painted material shows, which may read the sun, instance and scene-field inputs; the base color without it. */\n\
         \x20 readonly paint?: Vec3;\n\
         }\n\
         export interface MaterialDefinition {\n\
         \x20 readonly [kind]: \"material\";\n\
         }\n\
         export declare function defineMaterial(options: MaterialOptions): MaterialDefinition;\n\
         \n// Decal appearance and weathering layers.\n\
         /** How a channel combines with the value beneath it, by coverage: mix(beneath, combined, coverage), where combined is the channel's value (mix), the sum (add), the product (multiply) or the larger or smaller of the two (max, min). */\n\
         export type BlendRule = \"mix\" | \"add\" | \"multiply\" | \"max\" | \"min\";\n\
         export interface DecalOptions {\n\
         \x20 readonly name?: string;\n\
         \x20 /** Width and height in meters, numeric literals. `uv` spans the rectangle; `surface` is meters from its lower left corner. */\n\
         \x20 readonly size: readonly [number, number];\n\
         \x20 readonly baseColor: Vec3;\n\
         \x20 /** Coverage of the decal; alphaCutout(...) makes it a cutout. */\n\
         \x20 readonly opacity?: ScalarInput;\n\
         \x20 readonly roughness?: ScalarInput;\n\
         \x20 readonly metalness?: ScalarInput;\n\
         \x20 readonly height?: ScalarInput;\n\
         \x20 readonly emissive?: Vec3;\n\
         \x20 /** How each channel given combines with the surface beneath, by opacity; mix where omitted. */\n\
         \x20 readonly blend?: { readonly baseColor?: BlendRule; readonly roughness?: BlendRule; readonly metalness?: BlendRule; readonly height?: BlendRule; readonly emissive?: BlendRule };\n\
         \x20 readonly doubleSided?: boolean;\n\
         }\n\
         export interface DecalDefinition {\n\
         \x20 readonly [kind]: \"decal\";\n\
         }\n\
         /** A bounded appearance sized in meters, baked without a receiver; its owner places it. */\n\
         export declare function defineDecal(options: DecalOptions): DecalDefinition;\n\
         /** The surface beneath a weathering layer: the base material with the layers under this one. */\n\
         export declare const below: {\n\
         \x20 readonly baseColor: Vec3<\"none\">;\n\
         \x20 readonly roughness: Scalar;\n\
         \x20 readonly metalness: Scalar;\n\
         \x20 readonly height: Scalar;\n\
         \x20 readonly opacity: Scalar;\n\
         \x20 readonly normalOffset: Vec3<\"direction\">;\n\
         };\n\
         export interface WeatheringOptions {\n\
         \x20 readonly name?: string;\n\
         \x20 /** Where the layer lies, clamped to [0, 1]. */\n\
         \x20 readonly coverage: ScalarInput;\n\
         \x20 readonly baseColor?: Vec3;\n\
         \x20 readonly roughness?: ScalarInput;\n\
         \x20 readonly metalness?: ScalarInput;\n\
         \x20 readonly height?: ScalarInput;\n\
         \x20 readonly opacity?: ScalarInput;\n\
         \x20 /** A world-space vector added to the shaded normal after the relief, as a material's normalOffset. */\n\
         \x20 readonly normalOffset?: Vec3;\n\
         \x20 /** How each channel given combines with the surface beneath, by coverage; mix where omitted. */\n\
         \x20 readonly blend?: { readonly baseColor?: BlendRule; readonly roughness?: BlendRule; readonly metalness?: BlendRule; readonly height?: BlendRule; readonly opacity?: BlendRule; readonly normalOffset?: BlendRule };\n\
         }\n\
         export interface WeatheringDefinition {\n\
         \x20 readonly [kind]: \"weathering\";\n\
         }\n\
         /** A weathering layer over a whole surface. It changes the channels it gives and passes the others through; it reads the surface beneath through `below` and follows geometry through named features and external inputs, never the receiver's bounds. */\n\
         export declare function defineLayer(options: WeatheringOptions): WeatheringDefinition;\n",
    );
    out
}

pub fn ports_json() -> String {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Ports<'a> {
        catalog_version: u32,
        module: &'static str,
        inputs: &'a [Input],
        operations: &'a [Operation],
        operators: &'a [Operator],
        groups: &'a [Group],
        renderer_operations: &'a [RendererOperation],
    }
    let mut text = serde_json::to_string_pretty(&Ports {
        catalog_version: CATALOG_VERSION,
        module: SDK_MODULE,
        inputs: inputs(),
        operations: operations(),
        operators: operators(),
        groups: groups(),
        renderer_operations: renderer_operations(),
    })
    .expect("catalog serializes");
    text.push('\n');
    text
}

fn overload_text(name: &str, overload: &Overload) -> String {
    let args = overload
        .args
        .iter()
        .map(|kind| format!("{kind:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{name}({args}) -> {}",
        format!("{:?}", overload.result).to_lowercase()
    )
}

pub fn reference() -> String {
    let mut out = format!(
        "# MaterialSpec SDK reference (catalog v{CATALOG_VERSION})\n\n\
         Write `.material.ts` recipes that import from `\"{SDK_MODULE}\"`. Recipes are never executed:\n\
         the compiler parses a subset (imports, `const`, pure functions with typed arguments,\n\
         `export default defineMaterial({{...}})`). Use operator functions (`add`, `mul`, ...) instead of\n\
         infix operators. Parameter IDs are the constant names. Fields are inspectable: every `const` that is not a parameter, helper, group or layer.\n\
         Vectors are typed by coordinate space (`Vec3<\"world\">`): `uv`, `surface`, `object` and `world` never mix;\n\
         literals, parameters and directions (`normal`, `gravity`) combine with any space. Keep a space\n\
         through a helper with `<S extends Space>(p: Vec3<S>): Vec3<S>`.\n\n\
         ## Sample inputs\n"
    );
    for input in inputs() {
        let _ = writeln!(
            out,
            "- `{}: {}` - {}",
            input.ts_name,
            result_type(input.kind),
            input.summary
        );
    }
    out.push_str("\n## Operators\n");
    for operator in operators() {
        let _ = writeln!(
            out,
            "- `{}` ({}) - {} e.g. `{}`",
            operator.id, operator.symbol, operator.summary, operator.example
        );
    }
    out.push_str("\n## Primitives\n");
    for operation in operations() {
        let _ = writeln!(
            out,
            "- `{}` - {} e.g. `{}`",
            ts_name(operation.id),
            operation.summary,
            operation.example
        );
        for overload in &operation.overloads {
            let _ = writeln!(
                out,
                "    - {}",
                overload_text(&ts_name(operation.id), overload)
            );
        }
    }
    out.push_str("\n## Groups\n");
    for group in groups() {
        let _ = writeln!(
            out,
            "- `{}` ({}) - {} e.g. `{}`",
            group.id, group.title, group.summary, group.example
        );
        for argument in &group.arguments {
            let _ = writeln!(
                out,
                "    - arg `{}: {}`{}{} - {}",
                argument.name,
                result_type(argument.kind),
                if argument.unit.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", argument.unit)
                },
                argument
                    .default
                    .map(|d| format!(" = {d}"))
                    .unwrap_or_default(),
                argument.summary
            );
        }
        for output in &group.outputs {
            let _ = writeln!(
                out,
                "    - field `.{}: {}` - {}",
                output.name,
                result_type(output.kind),
                output.summary
            );
        }
        if let Some(reason) = group.unavailable {
            let _ = writeln!(out, "    - unavailable: {reason}");
        }
    }
    out.push_str("\n## Live renderer operations\n");
    for operation in renderer_operations() {
        let _ = writeln!(
            out,
            "- `{}` - {} Stage `{}`; renderer supplies {}. e.g. `{}`",
            operation.id,
            operation.summary,
            operation.stage,
            operation.requires.join(", "),
            operation.example
        );
    }
    out.push_str(
        "\n## Recipe\n\
         `base` gives channel starting values; each `layer({ id, mask, ... })` blends its channels in order\n\
         (`mix` by clamped coverage; `height: { add }` adds `coverage * delta`, `height: { replace }` blends toward\n\
         the value); `channels` replace a channel's final value. Diagnostics carry the module and UTF-16 range.\n",
    );
    out
}

pub fn tsconfig() -> String {
    format!(
        "{{\n  \"compilerOptions\": {{\n    \"strict\": true,\n    \"noEmit\": true,\n    \"target\": \"ES2022\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"Bundler\",\n    \"types\": [],\n    \"allowImportingTsExtensions\": true,\n    \"paths\": {{ \"{SDK_MODULE}\": [\"./index.d.ts\"] }}\n  }},\n  \"include\": [\"index.d.ts\"]\n}}\n"
    )
}
