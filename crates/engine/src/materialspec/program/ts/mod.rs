//! TypeScript recipe front end.
//!
//! A `.material.ts` file is valid strict TypeScript against the generated SDK declarations, but
//! it is never executed. This module parses the documented subset and lowers it to the same
//! definitions and outputs the `.ms` parser produces, so checking, evaluation and baking are
//! shared. Spans stay byte offsets into the recipe text; `diagnostic` converts them to UTF-16.
use super::{
    super::catalog::{self, Group},
    BASE_PLANE, Blend, LAYER_CHANNELS, LiveOperation, ProgramDiagnostic, RecipeKind, Source,
    Sources, Traits, Type, diagnostic,
    syntax::{self, Definition, Expr, Expression, Mark, Module},
};
use std::collections::{BTreeMap, BTreeSet};

mod edit;
mod scope;
pub use edit::{EditFailure, EditOp, Edited, Inspection, Patch, Span, apply, inspect};

const TOKEN_LIMIT: usize = 65_536;
const DEPTH_LIMIT: usize = 96;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Number(f64),
    Text(String),
    Symbol(&'static str),
    End,
}
#[derive(Clone, Debug)]
struct Located {
    token: Token,
    mark: Mark,
}

fn mark(module: &str, start: usize, end: usize) -> Mark {
    Mark {
        module: module.into(),
        start,
        end,
    }
}

fn tokenize(
    sources: &Sources,
    module: &str,
    text: &str,
) -> Result<Vec<Located>, ProgramDiagnostic> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let fail = |code, message: &str, start, end| {
        diagnostic(sources, code, message, &mark(module, start, end))
    };
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if text[i..].starts_with("//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if text[i..].starts_with("/*") {
            let Some(end) = text[i + 2..].find("*/") else {
                return Err(fail("MS_SYNTAX", "Unclosed comment.", i, text.len()));
            };
            i += end + 4;
            continue;
        }
        let start = i;
        let token = if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Token::Name(text[start..i].into())
        } else if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            if bytes[i] == b'.' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            if bytes.get(i) == Some(&b'.') {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if matches!(bytes.get(i), Some(b'e' | b'E')) {
                i += 1;
                if matches!(bytes.get(i), Some(b'+' | b'-')) {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let value = text[start..i]
                .replace('_', "")
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite());
            let Some(value) = value else {
                return Err(fail("MS_SYNTAX", "Expected a finite number.", start, i));
            };
            Token::Number(value)
        } else if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let mut escaped = false;
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == b'\n' {
                    break;
                }
                if bytes[i] == quote && !escaped {
                    i += 1;
                    closed = true;
                    break;
                }
                escaped = bytes[i] == b'\\' && !escaped;
                i += 1;
            }
            if !closed {
                return Err(fail("MS_SYNTAX", "Invalid or unclosed string.", start, i));
            }
            let inner = &text[start + 1..i - 1];
            let json = if quote == b'"' {
                format!("\"{inner}\"")
            } else {
                format!("\"{}\"", inner.replace("\\'", "'").replace('"', "\\\""))
            };
            let value = serde_json::from_str::<String>(&json)
                .map_err(|_| fail("MS_SYNTAX", "Invalid string escape.", start, i))?;
            Token::Text(value)
        } else if bytes[i] == b'`' {
            return Err(fail(
                "MT_SUBSET",
                "Template literals are outside the recipe subset; use a plain string.",
                i,
                i + 1,
            ));
        } else {
            let pair = text.get(i..i + 2).unwrap_or("");
            let symbol = ["=>", "==", "!=", "<=", ">=", "&&", "||"]
                .into_iter()
                .find(|s| *s == pair)
                .or_else(|| {
                    [
                        ";", ":", ",", "(", ")", "{", "}", "[", "]", "=", ".", "-", "+", "*", "/",
                        "%", "<", ">", "!", "&", "|", "?",
                    ]
                    .into_iter()
                    .find(|s| text[i..].starts_with(s))
                });
            let Some(symbol) = symbol else {
                let end = i + text[i..].chars().next().unwrap().len_utf8();
                return Err(fail("MS_SYNTAX", "Unexpected character.", i, end));
            };
            i += symbol.len();
            Token::Symbol(symbol)
        };
        tokens.push(Located {
            token,
            mark: mark(module, start, i),
        });
        if tokens.len() > TOKEN_LIMIT {
            return Err(fail("MS_LIMIT", "Module exceeds the token budget.", 0, 0));
        }
    }
    tokens.push(Located {
        token: Token::End,
        mark: mark(module, i, i),
    });
    Ok(tokens)
}

#[derive(Clone)]
struct Node {
    kind: NodeKind,
    mark: Mark,
    /// Byte offset just past the whole expression; mark.start is where it begins.
    end: usize,
}
#[derive(Clone)]
enum NodeKind {
    Number(f64),
    Bool(bool),
    Text(String),
    Name(String),
    Call(Box<Node>, Vec<Node>),
    Member(Box<Node>, String),
    Object(Vec<Property>),
    Array(Vec<Node>),
}
#[derive(Clone)]
struct Property {
    key: String,
    mark: Mark,
    value: Node,
}

enum Init {
    Expr(Node),
    Function {
        params: Vec<(String, Type)>,
        result: Type,
        body: Node,
    },
}
enum Statement {
    Import {
        names: Vec<(String, Mark)>,
        path: String,
        path_mark: Mark,
    },
    Const {
        name: String,
        mark: Mark,
        exported: bool,
        annotation: Option<Type>,
        init: Init,
    },
    Default(Node),
}

struct Parser<'a> {
    sources: &'a Sources,
    module: &'a str,
    tokens: Vec<Located>,
    cursor: usize,
    depth: usize,
    last_end: usize,
}
impl Parser<'_> {
    fn current(&self) -> &Located {
        &self.tokens[self.cursor]
    }
    fn advance(&mut self) -> Located {
        let token = self.current().clone();
        if token.token != Token::End {
            self.cursor += 1;
            self.last_end = token.mark.end;
        }
        token
    }
    fn error_at(
        &self,
        code: &'static str,
        message: impl Into<String>,
        at: &Mark,
    ) -> ProgramDiagnostic {
        diagnostic(self.sources, code, message, at)
    }
    fn error(&self, message: impl Into<String>) -> ProgramDiagnostic {
        self.error_at("MS_SYNTAX", message, &self.current().mark)
    }
    fn symbol(&mut self, s: &str) -> bool {
        if matches!(&self.current().token, Token::Symbol(v) if *v == s) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn is_symbol(&self, s: &str) -> bool {
        matches!(&self.current().token, Token::Symbol(v) if *v == s)
    }
    fn is_name(&self, s: &str) -> bool {
        matches!(&self.current().token, Token::Name(v) if v == s)
    }
    fn expect(&mut self, s: &str) -> Result<Located, ProgramDiagnostic> {
        if self.is_symbol(s) {
            return Ok(self.advance());
        }
        if let Token::Symbol(op) = &self.current().token {
            if matches!(
                *op,
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | "<"
                    | ">"
                    | "<="
                    | ">="
                    | "=="
                    | "!="
                    | "&&"
                    | "||"
                    | "!"
                    | "&"
                    | "|"
                    | "?"
            ) {
                return Err(self.error_at(
                    "MT_SUBSET",
                    format!("Operator '{op}' is outside the recipe subset; call add(), sub(), mul(), div(), rem(), neg(), lt(), le(), gt(), ge(), eq(), ne(), and(), or() or not()."),
                    &self.current().mark,
                ));
            }
        }
        Err(self.error(format!("Expected '{s}'.")))
    }
    fn name(&mut self) -> Result<(String, Mark), ProgramDiagnostic> {
        let located = self.advance();
        match located.token {
            Token::Name(name) => Ok((name, located.mark)),
            _ => Err(self.error_at("MS_SYNTAX", "Expected a name.", &located.mark)),
        }
    }
    /// Coordinate spaces only type-check in TypeScript; the compiler accepts and ignores them.
    fn space_argument(&mut self) -> Result<(), ProgramDiagnostic> {
        if !self.symbol("<") {
            return Ok(());
        }
        let located = self.advance();
        match located.token {
            Token::Name(_) => {}
            Token::Text(space) if catalog::SPACES.contains(&space.as_str()) => {}
            _ => {
                return Err(self.error_at(
                    "MS_TYPE",
                    format!(
                        "Expected a coordinate space ({}) or a type parameter.",
                        catalog::SPACES.join(", ")
                    ),
                    &located.mark,
                ));
            }
        }
        self.expect(">").map(|_| ())
    }
    /// `<S extends Space, T extends Space>` before a helper's parameters.
    fn type_parameters(&mut self) -> Result<(), ProgramDiagnostic> {
        if !self.symbol("<") {
            return Ok(());
        }
        loop {
            self.name()?;
            if !self.is_name("extends") {
                return Err(self.error("Expected 'extends Space'."));
            }
            self.advance();
            let (bound, at) = self.name()?;
            if bound != "Space" {
                return Err(self.error_at(
                    "MT_SUBSET",
                    "Type parameters are coordinate spaces: 'extends Space'.",
                    &at,
                ));
            }
            if !self.symbol(",") {
                break;
            }
        }
        self.expect(">").map(|_| ())
    }
    fn type_name(&mut self) -> Result<Type, ProgramDiagnostic> {
        let (name, at) = self.name()?;
        match name.as_str() {
            // The *Input aliases also accept literals, so helpers can be called with 0.08 or true.
            "Scalar" | "ScalarInput" => Ok(Type::Scalar),
            "Vec2" => self.space_argument().map(|()| Type::Vec2),
            "Vec3" => self.space_argument().map(|()| Type::Vec3),
            "Bool" | "BoolInput" => Ok(Type::Bool),
            _ => Err(self.error_at(
                "MS_TYPE",
                "Expected the SDK type Scalar, ScalarInput, Vec2, Vec3, Bool or BoolInput.",
                &at,
            )),
        }
    }

    fn statements(&mut self) -> Result<Vec<Statement>, ProgramDiagnostic> {
        let mut statements = Vec::new();
        while self.current().token != Token::End {
            statements.push(self.statement()?);
            if statements.len() > 512 {
                return Err(self.error("Module exceeds its declaration budget."));
            }
        }
        Ok(statements)
    }
    fn statement(&mut self) -> Result<Statement, ProgramDiagnostic> {
        let (keyword, at) = self.name()?;
        match keyword.as_str() {
            "import" => self.import(),
            "export" => {
                if self.is_name("default") {
                    self.advance();
                    let node = self.expr()?;
                    self.expect(";")?;
                    return Ok(Statement::Default(node));
                }
                let (next, at) = self.name()?;
                match next.as_str() {
                    "const" => self.constant(true),
                    "function" => self.function(true),
                    _ => Err(self.error_at(
                        "MT_SUBSET",
                        "Expected export const, export function or export default.",
                        &at,
                    )),
                }
            }
            "const" => self.constant(false),
            "function" => self.function(false),
            "let" | "var" => Err(self.error_at(
                "MT_SUBSET",
                "Use const; recipes are pure and bindings never change.",
                &at,
            )),
            _ => Err(self.error_at(
                "MT_SUBSET",
                "Expected import, const, function or export default.",
                &at,
            )),
        }
    }
    fn import(&mut self) -> Result<Statement, ProgramDiagnostic> {
        if self.is_name("type") {
            self.advance();
        }
        self.expect("{")?;
        let mut names = Vec::new();
        while !self.is_symbol("}") {
            // `type Vec3` imports a declaration only; types are names like any other import.
            if self.is_name("type")
                && matches!(
                    self.tokens.get(self.cursor + 1).map(|t| &t.token),
                    Some(Token::Name(_))
                )
            {
                self.advance();
            }
            let (name, at) = self.name()?;
            if self.is_name("as") {
                return Err(self.error_at(
                    "MT_SUBSET",
                    "Import aliases are outside the recipe subset.",
                    &self.current().mark,
                ));
            }
            names.push((name, at));
            if !self.symbol(",") {
                break;
            }
        }
        self.expect("}")?;
        if !self.is_name("from") {
            return Err(self.error("Expected 'from'."));
        }
        self.advance();
        let located = self.advance();
        let Token::Text(path) = located.token else {
            return Err(self.error_at(
                "MS_SYNTAX",
                "Expected a quoted module path.",
                &located.mark,
            ));
        };
        self.expect(";")?;
        Ok(Statement::Import {
            names,
            path,
            path_mark: located.mark,
        })
    }
    fn params(&mut self) -> Result<Vec<(String, Type)>, ProgramDiagnostic> {
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.is_symbol(")") {
            let (name, _) = self.name()?;
            self.expect(":")?;
            params.push((name, self.type_name()?));
            if !self.symbol(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(params)
    }
    fn body(&mut self) -> Result<Node, ProgramDiagnostic> {
        if self.symbol("{") {
            if !self.is_name("return") {
                return Err(self.error_at(
                    "MT_SUBSET",
                    "A helper body is one return statement.",
                    &self.current().mark,
                ));
            }
            self.advance();
            let value = self.expr()?;
            self.expect(";")?;
            self.expect("}")?;
            Ok(value)
        } else {
            self.expr()
        }
    }
    fn function(&mut self, exported: bool) -> Result<Statement, ProgramDiagnostic> {
        let (name, at) = self.name()?;
        self.type_parameters()?;
        let params = self.params()?;
        self.expect(":")?;
        let result = self.type_name()?;
        if !self.is_symbol("{") {
            return Err(self.error("Expected '{'."));
        }
        let body = self.body()?;
        Ok(Statement::Const {
            name,
            mark: at,
            exported,
            annotation: None,
            init: Init::Function {
                params,
                result,
                body,
            },
        })
    }
    fn arrow_ahead(&self) -> bool {
        if !self.is_symbol("(") {
            return false;
        }
        let mut depth = 0;
        for (offset, located) in self.tokens[self.cursor..].iter().enumerate() {
            match &located.token {
                Token::Symbol("(") => depth += 1,
                Token::Symbol(")") => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(self.cursor + offset + 1).is_some_and(|t| {
                            matches!(t.token, Token::Symbol(":") | Token::Symbol("=>"))
                        });
                    }
                }
                Token::End => return false,
                _ => {}
            }
        }
        false
    }
    fn constant(&mut self, exported: bool) -> Result<Statement, ProgramDiagnostic> {
        let (name, at) = self.name()?;
        let annotation = if self.symbol(":") {
            Some(self.type_name()?)
        } else {
            None
        };
        self.expect("=")?;
        let generic = self.is_symbol("<");
        if generic {
            self.type_parameters()?;
            if !self.arrow_ahead() {
                return Err(self.error("Expected a helper after its type parameters."));
            }
        }
        let init = if generic || self.arrow_ahead() {
            let params = self.params()?;
            if !self.symbol(":") {
                return Err(self.error_at(
                    "MT_SUBSET",
                    "Helpers need a return type annotation.",
                    &self.current().mark,
                ));
            }
            let result = self.type_name()?;
            self.expect("=>")?;
            Init::Function {
                params,
                result,
                body: self.body()?,
            }
        } else {
            Init::Expr(self.expr()?)
        };
        self.expect(";")?;
        Ok(Statement::Const {
            name,
            mark: at,
            exported,
            annotation,
            init,
        })
    }

    fn node(&self, kind: NodeKind, mark: Mark) -> Node {
        Node {
            kind,
            mark,
            end: self.last_end,
        }
    }
    fn expr(&mut self) -> Result<Node, ProgramDiagnostic> {
        self.depth += 1;
        if self.depth > DEPTH_LIMIT {
            return Err(self.error_at(
                "MS_LIMIT",
                "Expression nesting exceeds 96 levels.",
                &self.current().mark,
            ));
        }
        let mut node = self.primary()?;
        loop {
            if self.is_symbol("(") {
                let open = self.advance();
                let mut args = Vec::new();
                while !self.is_symbol(")") {
                    args.push(self.expr()?);
                    if !self.symbol(",") {
                        break;
                    }
                    if args.len() > 16 {
                        return Err(self.error_at(
                            "MS_LIMIT",
                            "Calls accept at most 16 arguments.",
                            &open.mark,
                        ));
                    }
                }
                self.expect(")")?;
                let at = node.mark.clone();
                node = self.node(NodeKind::Call(Box::new(node), args), at);
            } else if self.is_symbol(".") {
                self.advance();
                let (member, at) = self.name()?;
                node = self.node(NodeKind::Member(Box::new(node), member), at);
            } else {
                break;
            }
        }
        self.depth -= 1;
        Ok(node)
    }
    fn primary(&mut self) -> Result<Node, ProgramDiagnostic> {
        let located = self.advance();
        let at = located.mark.clone();
        match located.token {
            Token::Number(value) => Ok(self.node(NodeKind::Number(value), at)),
            Token::Text(value) => Ok(self.node(NodeKind::Text(value), at)),
            Token::Name(name) => Ok(self.node(
                match name.as_str() {
                    "true" => NodeKind::Bool(true),
                    "false" => NodeKind::Bool(false),
                    _ => NodeKind::Name(name),
                },
                at,
            )),
            Token::Symbol("-") => {
                let next = self.advance();
                match next.token {
                    Token::Number(value) => Ok(self.node(
                        NodeKind::Number(-value),
                        mark(self.module, at.start, next.mark.end),
                    )),
                    _ => Err(self.error_at(
                        "MT_SUBSET",
                        "Only numeric literals take a minus sign; call neg() or sub() otherwise.",
                        &at,
                    )),
                }
            }
            Token::Symbol("(") => {
                let node = self.expr()?;
                self.expect(")")?;
                Ok(node)
            }
            Token::Symbol("{") => {
                let mut properties = Vec::new();
                while !self.is_symbol("}") {
                    let key = self.advance();
                    let name = match key.token {
                        Token::Name(name) | Token::Text(name) => name,
                        _ => {
                            return Err(self.error_at(
                                "MS_SYNTAX",
                                "Expected a property name.",
                                &key.mark,
                            ));
                        }
                    };
                    let value = if self.symbol(":") {
                        self.expr()?
                    } else {
                        self.node(NodeKind::Name(name.clone()), key.mark.clone())
                    };
                    properties.push(Property {
                        key: name,
                        mark: key.mark,
                        value,
                    });
                    if !self.symbol(",") {
                        break;
                    }
                }
                self.expect("}")?;
                Ok(self.node(NodeKind::Object(properties), at))
            }
            Token::Symbol("[") => {
                let mut items = Vec::new();
                while !self.is_symbol("]") {
                    items.push(self.expr()?);
                    if !self.symbol(",") {
                        break;
                    }
                }
                self.expect("]")?;
                Ok(self.node(NodeKind::Array(items), at))
            }
            _ => Err(self.error_at("MS_SYNTAX", "Expected an expression.", &at)),
        }
    }
}

const CHANNELS: [(&str, &str, Type); 8] = [
    ("baseColor", "base_color", Type::Vec3),
    ("roughness", "roughness", Type::Scalar),
    ("metalness", "metalness", Type::Scalar),
    ("height", "height", Type::Scalar),
    ("emissive", "emissive", Type::Vec3),
    ("opacity", "opacity", Type::Scalar),
    ("receiverNormal", "receiver_normal", Type::Vec3),
    ("normalOffset", "normal_offset", Type::Vec3),
];

struct Lower<'a> {
    sources: &'a Sources,
    module: &'a str,
    imported: BTreeSet<String>,
    bindings: BTreeMap<String, String>,
    sdk: BTreeSet<String>,
    declared: BTreeSet<String>,
    groups: BTreeMap<String, &'static Group>,
    layer_nodes: BTreeMap<String, Node>,
    /// Channels of the surface beneath that the recipe reads (`below.<channel>`), with where.
    below: BTreeMap<&'static str, Mark>,
    errors: Vec<ProgramDiagnostic>,
    out: Module,
}

fn parse_statements(sources: &Sources, module: &str) -> Result<Vec<Statement>, ProgramDiagnostic> {
    let text = &sources.modules[module];
    let tokens = tokenize(sources, module, text)?;
    Parser {
        sources,
        module,
        tokens,
        cursor: 0,
        depth: 0,
        last_end: 0,
    }
    .statements()
}

pub(super) fn parse(sources: &Sources, module: &str) -> Result<Module, Vec<ProgramDiagnostic>> {
    let statements = parse_statements(sources, module).map_err(|e| vec![e])?;
    Lower {
        sources,
        module,
        imported: BTreeSet::new(),
        bindings: BTreeMap::new(),
        sdk: catalog::sdk_exports().into_iter().collect(),
        declared: BTreeSet::new(),
        groups: BTreeMap::new(),
        layer_nodes: BTreeMap::new(),
        below: BTreeMap::new(),
        errors: Vec::new(),
        out: Module {
            imports: Vec::new(),
            declarations: Vec::new(),
            outputs: Vec::new(),
            live: Vec::new(),
            traits: Default::default(),
        },
    }
    .run(statements)
}

fn is_call_to<'n>(node: &'n Node, name: &str) -> Option<&'n Vec<Node>> {
    match &node.kind {
        NodeKind::Call(callee, args) if matches!(&callee.kind, NodeKind::Name(n) if n == name) => {
            Some(args)
        }
        _ => None,
    }
}

impl Lower<'_> {
    fn error(&mut self, code: &'static str, message: impl Into<String>, at: &Mark) {
        self.errors
            .push(diagnostic(self.sources, code, message, at));
    }
    fn num(&self, value: f64, at: &Mark) -> Expr {
        Expr::make(Expression::Number(value), at.clone())
    }
    fn name_expr(&self, name: &str, at: &Mark) -> Expr {
        Expr::make(Expression::Name(name.into()), at.clone())
    }
    fn call_expr(&self, name: &str, args: Vec<Expr>, at: &Mark) -> Expr {
        Expr::make(Expression::Call(name.into(), args), at.clone())
    }

    fn run(mut self, statements: Vec<Statement>) -> Result<Module, Vec<ProgramDiagnostic>> {
        for statement in &statements {
            if let Statement::Const {
                name, init, mark, ..
            } = statement
            {
                if !self.declared.insert(name.clone()) {
                    self.error(
                        "MS_DUPLICATE",
                        format!("Declaration '{name}' is repeated in this module."),
                        mark,
                    );
                }
                if let Init::Expr(node) = init {
                    if let NodeKind::Call(callee, _) = &node.kind {
                        if let NodeKind::Name(callee) = &callee.kind {
                            if let Some(group) = catalog::find_group(callee) {
                                self.groups.insert(name.clone(), group);
                            } else if callee == "layer" {
                                self.layer_nodes.insert(name.clone(), node.clone());
                            }
                        }
                    }
                }
            }
        }
        // Imports introduce lexical bindings regardless of where their declaration appears.
        for statement in &statements {
            if let Statement::Import {
                names,
                path,
                path_mark,
            } = statement
            {
                self.import(names.clone(), path.clone(), path_mark);
            }
        }
        for statement in statements {
            self.statement(statement);
        }
        self.below_fields();
        if self.errors.is_empty() {
            scope::qualify(&mut self.out, self.module, self.sources, &self.bindings);
            Ok(self.out)
        } else {
            Err(self.errors)
        }
    }

    fn statement(&mut self, statement: Statement) {
        match statement {
            Statement::Import { .. } => {}
            Statement::Const {
                name,
                mark: at,
                annotation,
                init,
                ..
            } => self.constant(name, &at, annotation, init),
            Statement::Default(node) => self.material(&node),
        }
    }

    fn import(&mut self, names: Vec<(String, Mark)>, path: String, at: &Mark) {
        if path == catalog::SDK_MODULE {
            for (name, name_mark) in names {
                if self.sdk.contains(&name) {
                    if !self.imported.insert(name.clone()) || self.bindings.contains_key(&name) {
                        self.error(
                            "MS_DUPLICATE",
                            format!("Import '{name}' is repeated."),
                            &name_mark,
                        );
                    }
                } else {
                    self.error(
                        "MS_NAME",
                        format!("'{name}' is not exported by {}.", catalog::SDK_MODULE),
                        &name_mark,
                    );
                }
            }
        } else if path.starts_with("@superspec/material/") {
            self.error(
                "MS_VERSION",
                format!("Only {} is supported.", catalog::SDK_MODULE),
                at,
            );
        } else if path.starts_with("./") || path.starts_with("../") {
            if path.ends_with(".ms") {
                self.error("MS_IMPORT_LANGUAGE", "TypeScript recipes import TypeScript modules; .ms imports use a separate shared namespace.", at);
                return;
            }
            let target = if path.ends_with(".ts") {
                path
            } else {
                format!("{path}.ts")
            };
            match scope::imports(self.sources, self.module, &target, &names, at) {
                Ok(bindings) => {
                    for ((name, name_mark), (_, identity)) in names.into_iter().zip(bindings) {
                        if self.declared.contains(&name)
                            || self.imported.contains(&name)
                            || self.name_conflicts(&name)
                            || self.bindings.contains_key(&name)
                        {
                            self.error(
                                "MS_DUPLICATE",
                                format!("Import '{name}' conflicts with an existing binding."),
                                &name_mark,
                            );
                        } else {
                            self.bindings.insert(name, identity);
                        }
                    }
                }
                Err(errors) => self.errors.extend(errors),
            }
            self.out.imports.push((target, at.clone()));
        } else {
            self.error(
                "MS_IMPORT_PATH",
                "Recipes import relative modules or the MaterialSpec SDK only.",
                at,
            );
        }
    }

    fn name_conflicts(&self, name: &str) -> bool {
        self.sdk.contains(name)
            || catalog::find(&catalog::ms_name(name)).is_some()
            || super::check::variable(&catalog::ms_name(name)).is_some()
    }

    fn constant(&mut self, name: String, at: &Mark, annotation: Option<Type>, init: Init) {
        if self.name_conflicts(&name) {
            self.error(
                "MS_DUPLICATE",
                format!("Declaration '{name}' conflicts with an SDK name."),
                at,
            );
            return;
        }
        match init {
            Init::Function {
                params,
                result,
                body,
            } => {
                let locals = params.iter().map(|(n, _)| n.clone()).collect();
                if let Some(value) = self.expr(&body, &locals) {
                    self.out.declarations.push((
                        name,
                        Definition::Function {
                            arguments: params,
                            result,
                            value,
                            mark: at.clone(),
                        },
                    ));
                }
            }
            Init::Expr(node) => {
                if self.layer_nodes.contains_key(&name) {
                    return;
                }
                if let NodeKind::Call(callee, args) = &node.kind {
                    if let NodeKind::Member(base, kind) = &callee.kind {
                        if matches!(&base.kind, NodeKind::Name(n) if n == "param") {
                            self.parameter(name, at, kind, args, &node.mark);
                            return;
                        }
                    }
                }
                if let Some(group) = self.groups.get(&name).copied() {
                    if let NodeKind::Call(_, args) = &node.kind {
                        self.group(&name, group, args, &node.mark);
                    }
                    return;
                }
                if let Some(value) = self.expr(&node, &BTreeSet::new()) {
                    self.out.declarations.push((
                        name,
                        Definition::Field {
                            value,
                            kind: annotation,
                            mark: at.clone(),
                        },
                    ));
                }
            }
        }
    }

    /// Validate an options object: keys must be listed and unique.
    fn properties<'n>(
        &mut self,
        node: &'n Node,
        what: &str,
        allowed: &[&str],
    ) -> Option<&'n [Property]> {
        let NodeKind::Object(properties) = &node.kind else {
            self.error(
                "MT_SUBSET",
                format!("{what} takes an object literal."),
                &node.mark,
            );
            return None;
        };
        let mut seen = BTreeSet::new();
        let mut valid = true;
        for property in properties {
            if !allowed.contains(&property.key.as_str()) {
                self.error(
                    "MT_PROPERTY",
                    format!(
                        "Unknown property '{}' for {what}; expected {}.",
                        property.key,
                        allowed.join(", ")
                    ),
                    &property.mark,
                );
                valid = false;
            } else if !seen.insert(property.key.clone()) {
                self.error(
                    "MT_PROPERTY",
                    format!("Property '{}' is repeated.", property.key),
                    &property.mark,
                );
                valid = false;
            }
        }
        valid.then_some(properties.as_slice())
    }

    fn require_import(&mut self, name: &str, at: &Mark) -> bool {
        if self.imported.contains(name) {
            true
        } else {
            self.error(
                "MS_NAME",
                format!(
                    "'{name}' is part of the MaterialSpec SDK; import it from \"{}\".",
                    catalog::SDK_MODULE
                ),
                at,
            );
            false
        }
    }

    fn parameter(&mut self, name: String, at: &Mark, kind: &str, args: &[Node], call: &Mark) {
        if !self.require_import("param", call) {
            return;
        }
        let kind = match kind {
            "scalar" => Type::Scalar,
            "vec2" => Type::Vec2,
            "vec3" => Type::Vec3,
            "bool" => Type::Bool,
            _ => {
                self.error(
                    "MS_NAME",
                    format!("param.{kind} is not an SDK parameter kind."),
                    call,
                );
                return;
            }
        };
        let [options] = args else {
            self.error("MT_SUBSET", "param calls take one options object.", call);
            return;
        };
        let Some(properties) =
            self.properties(options, "param", &["default", "range", "unit", "label"])
        else {
            return;
        };
        let find = |key: &str| properties.iter().find(|p| p.key == key);
        let Some(default) = find("default") else {
            self.error("MT_PROPERTY", "A parameter needs a default.", &options.mark);
            return;
        };
        let mut range = None;
        if let Some(property) = find("range") {
            match &property.value.kind {
                NodeKind::Array(items) => match items.as_slice() {
                    [
                        Node {
                            kind: NodeKind::Number(lo),
                            ..
                        },
                        Node {
                            kind: NodeKind::Number(hi),
                            ..
                        },
                    ] => {
                        range = Some([*lo, *hi]);
                    }
                    _ => self.error(
                        "MT_PROPERTY",
                        "A range is [min, max] numeric literals.",
                        &property.value.mark,
                    ),
                },
                _ => self.error(
                    "MT_PROPERTY",
                    "A range is [min, max] numeric literals.",
                    &property.value.mark,
                ),
            }
        }
        for key in ["unit", "label"] {
            if let Some(property) = find(key) {
                if !matches!(property.value.kind, NodeKind::Text(_)) {
                    self.error(
                        "MT_PROPERTY",
                        format!("'{key}' is a string literal."),
                        &property.value.mark,
                    );
                }
            }
        }
        if let Some(value) = self.expr(&default.value, &BTreeSet::new()) {
            self.out.declarations.push((
                name,
                Definition::Parameter {
                    kind,
                    value,
                    range,
                    unit: find("unit").and_then(|property| match &property.value.kind {
                        NodeKind::Text(value) => Some(value.clone()),
                        _ => None,
                    }),
                    label: find("label").and_then(|property| match &property.value.kind {
                        NodeKind::Text(value) => Some(value.clone()),
                        _ => None,
                    }),
                    mark: at.clone(),
                },
            ));
        }
    }

    fn text_expr(&self, text: &str) -> Result<Expr, ProgramDiagnostic> {
        let template = Sources {
            entry: "template".into(),
            modules: BTreeMap::from([(
                "template".into(),
                format!("material 1;\nlet value = {text};"),
            )]),
        };
        let mut parsed = syntax::parse(&template, "template")?;
        let (_, definition) = parsed.declarations.remove(0);
        Ok(definition.value().clone())
    }

    fn group(&mut self, instance: &str, group: &'static Group, args: &[Node], call: &Mark) {
        if !self.require_import(group.id, call) {
            return;
        }
        if let Some(reason) = group.unavailable {
            self.error(
                "MT_CAPABILITY",
                format!("{} is unavailable: {reason}", group.title),
                call,
            );
            return;
        }
        let allowed: Vec<&str> = group.arguments.iter().map(|a| a.name).collect();
        let properties: &[Property] = match args {
            [] => &[],
            [options] => match self.properties(options, group.id, &allowed) {
                Some(properties) => properties,
                None => return,
            },
            _ => {
                self.error(
                    "MT_SUBSET",
                    format!("{} takes one options object.", group.id),
                    call,
                );
                return;
            }
        };
        let mut declarations = Vec::new();
        for argument in &group.arguments {
            let field = format!("{instance}.{}", argument.name);
            let (value, at) = match properties.iter().find(|p| p.key == argument.name) {
                Some(property) => match self.expr(&property.value, &BTreeSet::new()) {
                    Some(value) => (value, property.value.mark.clone()),
                    None => return,
                },
                None => match argument.default {
                    Some(text) => match self.text_expr(text) {
                        Ok(mut value) => {
                            value.remark(call);
                            (value, call.clone())
                        }
                        Err(error) => {
                            self.errors.push(error);
                            return;
                        }
                    },
                    None => {
                        self.error(
                            "MT_ARGUMENT",
                            format!("{} requires '{}'.", group.id, argument.name),
                            call,
                        );
                        return;
                    }
                },
            };
            declarations.push((
                field,
                Definition::Field {
                    value,
                    kind: Some(argument.kind),
                    mark: at,
                },
            ));
        }
        let template = Sources {
            entry: "template".into(),
            modules: BTreeMap::from([(
                "template".into(),
                format!("material 1;\n{}", group.template.replace("%ID%", instance)),
            )]),
        };
        let parsed = match syntax::parse(&template, "template") {
            Ok(parsed) => parsed,
            Err(error) => {
                self.errors.push(error);
                return;
            }
        };
        let mut renames: BTreeMap<String, String> = group
            .arguments
            .iter()
            .map(|a| (a.name.to_owned(), format!("{instance}.{}", a.name)))
            .collect();
        for (name, _) in &parsed.declarations {
            renames.insert(name.clone(), format!("{instance}.{name}"));
        }
        for (name, mut definition) in parsed.declarations {
            definition.rename(&|name| renames.get(name).cloned());
            definition.remark(call);
            declarations.push((format!("{instance}.{name}"), definition));
        }
        self.out.declarations.extend(declarations);
    }

    /// A literal `true` or `false` property, such as `doubleSided`.
    fn flag(&mut self, property: Option<&Property>) -> bool {
        match property.map(|property| (&property.value.kind, &property.value.mark)) {
            None => false,
            Some((NodeKind::Bool(value), _)) => *value,
            Some((_, at)) => {
                let at = at.clone();
                self.error("MT_PROPERTY", "Expected the literal true or false.", &at);
                false
            }
        }
    }

    /// The `blend` property of a decal or layer: a rule for some of the channels it outputs.
    fn blend_rules(
        &mut self,
        property: Option<&Property>,
        channels: &[(&'static str, &'static str)],
    ) -> BTreeMap<&'static str, Blend> {
        let mut rules = BTreeMap::new();
        let Some(property) = property else {
            return rules;
        };
        let keys: Vec<&str> = channels.iter().map(|(key, _)| *key).collect();
        let Some(entries) = self.properties(&property.value, "blend", &keys) else {
            return rules;
        };
        for entry in entries {
            let rule = match &entry.value.kind {
                NodeKind::Text(name) => Blend::named(name),
                _ => None,
            };
            let Some(rule) = rule else {
                self.error(
                    "MT_PROPERTY",
                    format!("A blend rule is one of {}.", Blend::NAMES.join(", ")),
                    &entry.value.mark,
                );
                continue;
            };
            let (_, ms) = channels
                .iter()
                .find(|(key, _)| *key == entry.key)
                .expect("a listed key");
            rules.insert(*ms, rule);
        }
        rules
    }

    /// `defineDecal({ size, baseColor, opacity, ..., blend })`: a bounded appearance sized in
    /// meters. Its channels are its own values; `blend` says how whoever places it combines
    /// each with the surface beneath, by the decal's opacity.
    fn decal(&mut self, node: &Node, args: &[Node]) {
        if !self.require_import("defineDecal", &node.mark) {
            return;
        }
        let [options] = args else {
            self.error(
                "MT_SUBSET",
                "defineDecal takes one options object.",
                &node.mark,
            );
            return;
        };
        const DECAL: [&str; 6] = [
            "baseColor",
            "roughness",
            "metalness",
            "height",
            "emissive",
            "opacity",
        ];
        let mut allowed = vec!["name", "size", "blend", "doubleSided"];
        allowed.extend(DECAL);
        let Some(properties) = self.properties(options, "defineDecal", &allowed) else {
            return;
        };
        let find = |key: &str| properties.iter().find(|p| p.key == key);
        let size = match find("size").map(|property| &property.value.kind) {
            Some(NodeKind::Array(items)) => match items.as_slice() {
                [
                    Node {
                        kind: NodeKind::Number(width),
                        ..
                    },
                    Node {
                        kind: NodeKind::Number(height),
                        ..
                    },
                ] if *width > 0. && *height > 0. && width.is_finite() && height.is_finite() => {
                    Some([*width, *height])
                }
                _ => None,
            },
            _ => None,
        };
        if size.is_none() {
            self.error(
                "MT_PROPERTY",
                "A decal needs size: [width, height], positive numeric literals in meters.",
                find("size").map_or(&options.mark, |property| &property.value.mark),
            );
        }
        if find("baseColor").is_none() {
            self.error("MT_BASE", "A decal needs a baseColor.", &options.mark);
        }
        let mut blended = Vec::new();
        for (key, ms, kind) in CHANNELS {
            let Some(entry) = find(key).filter(|_| DECAL.contains(&key)) else {
                continue;
            };
            let Some(value) = self.expr(&entry.value, &BTreeSet::new()) else {
                continue;
            };
            let field = format!("decal.{ms}");
            self.out.declarations.push((
                field.clone(),
                Definition::Field {
                    value,
                    kind: Some(kind),
                    mark: entry.mark.clone(),
                },
            ));
            self.out
                .outputs
                .push((ms.into(), self.name_expr(&field, &entry.mark)));
            if ms != "opacity" {
                blended.push((key, ms));
            }
        }
        let rules = self.blend_rules(find("blend"), &blended);
        self.out.traits = Traits {
            kind: RecipeKind::Decal,
            double_sided: self.flag(find("doubleSided")),
            size,
            blend: blended
                .iter()
                .map(|(_, ms)| {
                    (
                        (*ms).to_owned(),
                        rules.get(ms).copied().unwrap_or(Blend::Mix),
                    )
                })
                .collect(),
        };
    }

    /// The field that reads a channel of the surface beneath a layer.
    fn below_field(ms: &str) -> String {
        format!("below.{ms}")
    }

    /// `defineLayer({ coverage, baseColor, ..., blend })`: a weathering layer. Each channel it
    /// gives becomes the surface beneath combined with it by the layer's coverage, under the
    /// channel's blend rule; the other channels are not outputs, and pass through.
    fn weathering(&mut self, node: &Node, args: &[Node]) {
        if !self.require_import("defineLayer", &node.mark) {
            return;
        }
        let [options] = args else {
            self.error(
                "MT_SUBSET",
                "defineLayer takes one options object.",
                &node.mark,
            );
            return;
        };
        let channels: Vec<(&'static str, &'static str, Type)> = CHANNELS
            .into_iter()
            .filter(|(_, ms, _)| LAYER_CHANNELS.contains(ms))
            .collect();
        let mut allowed = vec!["name", "coverage", "blend"];
        allowed.extend(channels.iter().map(|(key, _, _)| *key));
        let Some(properties) = self.properties(options, "defineLayer", &allowed) else {
            return;
        };
        let find = |key: &str| properties.iter().find(|p| p.key == key);
        let Some(coverage) = find("coverage") else {
            self.error("MT_PROPERTY", "A layer needs a coverage.", &options.mark);
            return;
        };
        let Some(value) = self.expr(&coverage.value, &BTreeSet::new()) else {
            return;
        };
        let at = &coverage.value.mark;
        let clamped = self.call_expr("clamp", vec![value, self.num(0., at), self.num(1., at)], at);
        self.out.declarations.push((
            "layer.coverage".into(),
            Definition::Field {
                value: clamped,
                kind: Some(Type::Scalar),
                mark: at.clone(),
            },
        ));
        let given: Vec<(&'static str, &'static str)> = channels
            .iter()
            .filter(|(key, _, _)| find(key).is_some())
            .map(|(key, ms, _)| (*key, *ms))
            .collect();
        let rules = self.blend_rules(find("blend"), &given);
        let mut blend = BTreeMap::new();
        for (key, ms, kind) in channels {
            let Some(entry) = find(key) else { continue };
            let Some(value) = self.expr(&entry.value, &BTreeSet::new()) else {
                continue;
            };
            let at = &entry.value.mark;
            let field = format!("layer.{ms}");
            self.out.declarations.push((
                field.clone(),
                Definition::Field {
                    value,
                    kind: Some(kind),
                    mark: entry.mark.clone(),
                },
            ));
            self.below.entry(ms).or_insert_with(|| at.clone());
            let beneath = || self.name_expr(&Self::below_field(ms), at);
            let own = self.name_expr(&field, at);
            let rule = rules.get(ms).copied().unwrap_or(Blend::Mix);
            let binary = |symbol: &str| {
                Expr::make(
                    Expression::Binary(symbol.into(), Box::new(beneath()), Box::new(own.clone())),
                    at.clone(),
                )
            };
            let combined = match rule {
                Blend::Mix => own.clone(),
                Blend::Add => binary("+"),
                Blend::Multiply => binary("*"),
                Blend::Max => self.call_expr("max", vec![beneath(), own.clone()], at),
                Blend::Min => self.call_expr("min", vec![beneath(), own.clone()], at),
            };
            let cover = self.name_expr("layer.coverage", at);
            self.out.outputs.push((
                ms.into(),
                self.call_expr("mix", vec![beneath(), combined, cover], at),
            ));
            blend.insert(ms.to_owned(), rule);
        }
        self.out.traits = Traits {
            kind: RecipeKind::Layer,
            blend,
            ..Traits::default()
        };
    }

    /// The fields that read the surface beneath, for every channel a layer reads or changes.
    fn below_fields(&mut self) {
        let below = std::mem::take(&mut self.below);
        if self.out.traits.kind != RecipeKind::Layer {
            if let Some(at) = below.values().next() {
                self.error(
                    "MT_SUBSET",
                    "Only a layer (defineLayer) reads the surface beneath it.",
                    at,
                );
            }
            return;
        }
        for (ms, at) in below {
            let fallback = match ms {
                "base_color" => self.call_expr("vec3", vec![self.num(0.5, &at)], &at),
                _ => self.default_channel(ms, &at),
            };
            let name = Expr::make(Expression::Text(format!("{BASE_PLANE}{ms}")), at.clone());
            let kind = CHANNELS
                .iter()
                .find(|(_, known, _)| *known == ms)
                .map(|(_, _, kind)| *kind);
            self.out.declarations.push((
                Self::below_field(ms),
                Definition::Field {
                    value: self.call_expr("external", vec![name, fallback], &at),
                    kind,
                    mark: at,
                },
            ));
        }
    }

    fn material(&mut self, node: &Node) {
        if let Some(args) = is_call_to(node, "defineDecal") {
            return self.decal(node, args);
        }
        if let Some(args) = is_call_to(node, "defineLayer") {
            return self.weathering(node, args);
        }
        let Some(args) = is_call_to(node, "defineMaterial") else {
            self.error(
                "MT_SUBSET",
                "The default export must be defineMaterial({ ... }), defineDecal({ ... }) or defineLayer({ ... }).",
                &node.mark,
            );
            return;
        };
        if !self.require_import("defineMaterial", &node.mark) {
            return;
        }
        let [options] = args.as_slice() else {
            self.error(
                "MT_SUBSET",
                "defineMaterial takes one options object.",
                &node.mark,
            );
            return;
        };
        let Some(properties) = self.properties(
            options,
            "defineMaterial",
            &["name", "base", "layers", "channels", "live", "doubleSided"],
        ) else {
            return;
        };
        let find = |key: &str| properties.iter().find(|p| p.key == key);
        self.out.traits.double_sided = self.flag(find("doubleSided"));
        let channel_keys: Vec<&str> = CHANNELS.iter().map(|c| c.0).collect();
        let mut current: BTreeMap<&'static str, String> = BTreeMap::new();

        match find("base") {
            None => self.error("MT_BASE", "defineMaterial requires a base.", &node.mark),
            Some(base) => {
                if let Some(entries) = self.properties(&base.value, "base", &channel_keys) {
                    for (key, ms, kind) in CHANNELS {
                        if let Some(entry) = entries.iter().find(|e| e.key == key) {
                            if let Some(value) = self.expr(&entry.value, &BTreeSet::new()) {
                                let field = format!("base.{ms}");
                                self.out.declarations.push((
                                    field.clone(),
                                    Definition::Field {
                                        value,
                                        kind: Some(kind),
                                        mark: entry.mark.clone(),
                                    },
                                ));
                                current.insert(ms, field);
                            }
                        }
                    }
                }
            }
        }

        let mut overrides: BTreeMap<&'static str, Expr> = BTreeMap::new();
        if let Some(channels) = find("channels") {
            if let Some(entries) = self.properties(&channels.value, "channels", &channel_keys) {
                for (key, ms, _) in CHANNELS {
                    if let Some(entry) = entries.iter().find(|e| e.key == key) {
                        if let Some(value) = self.expr(&entry.value, &BTreeSet::new()) {
                            overrides.insert(ms, value);
                        }
                    }
                }
            }
        }
        if !current.contains_key("base_color")
            && !overrides.contains_key("base_color")
            && find("base").is_some()
        {
            self.error(
                "MT_BASE",
                "base.baseColor is required.",
                &find("base").unwrap().mark,
            );
        }

        if let Some(layers) = find("layers") {
            let NodeKind::Array(items) = &layers.value.kind else {
                self.error(
                    "MT_SUBSET",
                    "layers is an array of layer({ ... }) calls.",
                    &layers.value.mark,
                );
                return;
            };
            let mut ids = BTreeSet::new();
            for item in items {
                let resolved = match &item.kind {
                    NodeKind::Name(name) => match self.layer_nodes.get(name) {
                        Some(node) => node.clone(),
                        None => {
                            self.error(
                                "MT_SUBSET",
                                format!("'{name}' is not a layer."),
                                &item.mark,
                            );
                            continue;
                        }
                    },
                    _ => item.clone(),
                };
                self.layer(&resolved, &mut ids, &mut current);
            }
        }

        for (_, ms, _) in CHANNELS {
            let expr = match (overrides.remove(ms), current.get(ms)) {
                (Some(expr), _) => expr,
                (None, Some(field)) => self.name_expr(field, &node.mark),
                (None, None) => continue,
            };
            self.out.outputs.push((ms.into(), expr));
        }

        if let Some(live) = find("live") {
            self.live(&live.value);
        }
    }

    fn default_channel(&self, ms: &str, at: &Mark) -> Expr {
        match ms {
            "roughness" | "opacity" => self.num(1.0, at),
            "emissive" | "normal_offset" => self.call_expr("vec3", vec![self.num(0.0, at)], at),
            "receiver_normal" => self.name_expr("normal", at),
            _ => self.num(0.0, at),
        }
    }

    fn layer(
        &mut self,
        node: &Node,
        ids: &mut BTreeSet<String>,
        current: &mut BTreeMap<&'static str, String>,
    ) {
        let Some(args) = is_call_to(node, "layer") else {
            self.error("MT_SUBSET", "Expected a layer({ ... }) call.", &node.mark);
            return;
        };
        if !self.require_import("layer", &node.mark) {
            return;
        }
        let [options] = args.as_slice() else {
            self.error("MT_SUBSET", "layer takes one options object.", &node.mark);
            return;
        };
        let mut allowed = vec!["id", "mask"];
        allowed.extend(CHANNELS.iter().map(|c| c.0));
        let Some(properties) = self.properties(options, "layer", &allowed) else {
            return;
        };
        let find = |key: &str| properties.iter().find(|p| p.key == key);
        let id = match find("id").map(|p| (&p.value.kind, &p.value.mark)) {
            Some((NodeKind::Text(id), at)) => {
                let valid = id
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !valid {
                    self.error("MT_PROPERTY", "A layer id is an identifier.", at);
                    return;
                }
                if !ids.insert(id.clone()) {
                    self.error(
                        "MS_DUPLICATE",
                        format!("Layer id '{id}' is used twice."),
                        at,
                    );
                    return;
                }
                id.clone()
            }
            _ => {
                self.error(
                    "MT_PROPERTY",
                    "A layer needs a literal string id.",
                    &options.mark,
                );
                return;
            }
        };
        let Some(mask) = find("mask") else {
            self.error("MT_PROPERTY", "A layer needs a mask.", &options.mark);
            return;
        };
        let Some(mask_value) = self.expr(&mask.value, &BTreeSet::new()) else {
            return;
        };
        let at = &mask.value.mark;
        let coverage = format!("layers.{id}.coverage");
        let clamped = self.call_expr(
            "clamp",
            vec![mask_value, self.num(0.0, at), self.num(1.0, at)],
            at,
        );
        self.out.declarations.push((
            coverage.clone(),
            Definition::Field {
                value: clamped,
                kind: Some(Type::Scalar),
                mark: at.clone(),
            },
        ));
        for (key, ms, kind) in CHANNELS {
            let Some(entry) = find(key) else { continue };
            if ms == "base_color" && !current.contains_key(ms) {
                self.error(
                    "MT_BASE",
                    "A layer cannot blend baseColor without base.baseColor.",
                    &entry.mark,
                );
                continue;
            }
            let at = &entry.value.mark;
            let previous = match current.get(ms) {
                Some(field) => self.name_expr(field, at),
                None => self.default_channel(ms, at),
            };
            let cover = self.name_expr(&coverage, at);
            let value = if ms == "height" && matches!(entry.value.kind, NodeKind::Object(_)) {
                let Some(edit) = self.properties(&entry.value, "height", &["add", "replace"])
                else {
                    continue;
                };
                let [edit] = edit else {
                    self.error(
                        "MT_PROPERTY",
                        "height takes exactly one of add or replace.",
                        at,
                    );
                    continue;
                };
                let Some(delta) = self.expr(&edit.value, &BTreeSet::new()) else {
                    continue;
                };
                if edit.key == "add" {
                    let scaled = Expr::make(
                        Expression::Binary("*".into(), Box::new(cover), Box::new(delta)),
                        at.clone(),
                    );
                    Expr::make(
                        Expression::Binary("+".into(), Box::new(previous), Box::new(scaled)),
                        at.clone(),
                    )
                } else {
                    self.call_expr("mix", vec![previous, delta, cover], at)
                }
            } else {
                let Some(value) = self.expr(&entry.value, &BTreeSet::new()) else {
                    continue;
                };
                // A normal turns toward the layer's direction by coverage rather than averaging.
                let blend = if ms == "receiver_normal" {
                    "adjust_normal"
                } else {
                    "mix"
                };
                self.call_expr(blend, vec![previous, value, cover], at)
            };
            let field = format!("layers.{id}.{ms}");
            self.out.declarations.push((
                field.clone(),
                Definition::Field {
                    value,
                    kind: Some(kind),
                    mark: entry.mark.clone(),
                },
            ));
            current.insert(ms, field);
        }
    }

    fn live(&mut self, node: &Node) {
        let NodeKind::Array(items) = &node.kind else {
            self.error(
                "MT_SUBSET",
                "live is an array of renderer operations.",
                &node.mark,
            );
            return;
        };
        for (index, item) in items.iter().enumerate() {
            let NodeKind::Call(callee, args) = &item.kind else {
                self.error(
                    "MT_SUBSET",
                    "Expected a renderer operation call.",
                    &item.mark,
                );
                continue;
            };
            let NodeKind::Name(id) = &callee.kind else {
                self.error(
                    "MT_SUBSET",
                    "Expected a renderer operation call.",
                    &item.mark,
                );
                continue;
            };
            let Some(operation) = catalog::find_renderer_operation(id) else {
                self.error(
                    "MS_NAME",
                    format!("'{id}' is not a renderer operation."),
                    &callee.mark,
                );
                continue;
            };
            if !self.require_import(id, &callee.mark) {
                continue;
            }
            let allowed: Vec<&str> = operation.arguments.iter().map(|a| a.name).collect();
            let [options] = args.as_slice() else {
                self.error(
                    "MT_SUBSET",
                    format!("{id} takes one options object."),
                    &item.mark,
                );
                continue;
            };
            let Some(properties) = self.properties(options, id, &allowed) else {
                continue;
            };
            let mut inputs = BTreeMap::new();
            for argument in &operation.arguments {
                let field = format!("live.{index}.{}", argument.name);
                let (value, at) = match properties.iter().find(|p| p.key == argument.name) {
                    Some(property) => match self.expr(&property.value, &BTreeSet::new()) {
                        Some(value) => (value, property.value.mark.clone()),
                        None => continue,
                    },
                    None => match argument.default {
                        Some(text) => match self.text_expr(text) {
                            Ok(mut value) => {
                                value.remark(&item.mark);
                                (value, item.mark.clone())
                            }
                            Err(error) => {
                                self.errors.push(error);
                                continue;
                            }
                        },
                        None => {
                            self.error(
                                "MT_ARGUMENT",
                                format!("{id} requires '{}'.", argument.name),
                                &item.mark,
                            );
                            continue;
                        }
                    },
                };
                self.out.declarations.push((
                    field.clone(),
                    Definition::Field {
                        value,
                        kind: Some(argument.kind),
                        mark: at,
                    },
                ));
                inputs.insert(argument.name.to_owned(), field);
            }
            let source: Source = diagnostic(self.sources, "", "", &item.mark).source;
            self.out.live.push(LiveOperation {
                id: operation.id.into(),
                version: operation.version,
                stage: operation.stage.into(),
                inputs,
                requires: operation.requires.iter().map(|r| (*r).to_owned()).collect(),
                source,
            });
        }
    }

    fn expr(&mut self, node: &Node, locals: &BTreeSet<String>) -> Option<Expr> {
        let expr = match &node.kind {
            NodeKind::Number(value) => Expr::make(Expression::Number(*value), node.mark.clone()),
            NodeKind::Bool(value) => Expr::make(Expression::Bool(*value), node.mark.clone()),
            NodeKind::Text(value) => Expr::make(Expression::Text(value.clone()), node.mark.clone()),
            NodeKind::Name(name) => self.binding(name, &node.mark, locals)?,
            NodeKind::Member(base, member) => {
                if let NodeKind::Name(instance) = &base.kind {
                    // A channel of the surface beneath a layer.
                    if instance == "below"
                        && !locals.contains(instance)
                        && !self.declared.contains(instance)
                        && !self.bindings.contains_key(instance)
                    {
                        if !self.require_import("below", &node.mark) {
                            return None;
                        }
                        let channel = CHANNELS
                            .iter()
                            .find(|(key, ms, _)| key == member && LAYER_CHANNELS.contains(ms));
                        let Some((_, ms, _)) = channel else {
                            self.error(
                                "MS_NAME",
                                "The surface beneath has baseColor, roughness, metalness, height, opacity and normalOffset.",
                                &node.mark,
                            );
                            return None;
                        };
                        self.below.entry(ms).or_insert_with(|| node.mark.clone());
                        return Some(self.name_expr(&Self::below_field(ms), &node.mark));
                    }
                    if !locals.contains(instance) {
                        if let Some(group) = self.groups.get(instance).copied() {
                            if group.outputs.iter().any(|o| o.name == member) {
                                return Some(
                                    self.name_expr(&format!("{instance}.{member}"), &node.mark),
                                );
                            }
                            self.error(
                                "MS_NAME",
                                format!(
                                    "{} has no field '{member}'; it exposes {}.",
                                    group.id,
                                    group
                                        .outputs
                                        .iter()
                                        .map(|o| o.name)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                &node.mark,
                            );
                            return None;
                        }
                    }
                }
                let base = self.expr(base, locals)?;
                Expr::make(
                    Expression::Member(Box::new(base), member.clone()),
                    node.mark.clone(),
                )
            }
            NodeKind::Call(callee, args) => self.call(node, callee, args, locals)?,
            NodeKind::Object(_) | NodeKind::Array(_) => {
                self.error(
                    "MT_SUBSET",
                    "Objects and arrays are only valid as arguments to defineMaterial, layer, param and group calls.",
                    &node.mark,
                );
                return None;
            }
        };
        if expr.depth > DEPTH_LIMIT {
            self.error(
                "MS_LIMIT",
                "Expression exceeds the syntax depth budget.",
                &node.mark,
            );
            return None;
        }
        Some(expr)
    }

    fn binding(&mut self, name: &str, at: &Mark, locals: &BTreeSet<String>) -> Option<Expr> {
        if locals.contains(name) {
            return Some(self.name_expr(name, at));
        }
        if self.declared.contains(name) {
            if self.groups.contains_key(name) {
                self.error(
                    "MT_SUBSET",
                    format!(
                        "Use a field of '{name}', such as {name}.{}.",
                        self.groups[name].outputs[0].name
                    ),
                    at,
                );
                return None;
            }
            if self.layer_nodes.contains_key(name) {
                self.error(
                    "MT_SUBSET",
                    format!("'{name}' is a layer; list it in layers."),
                    at,
                );
                return None;
            }
            return Some(self.name_expr(name, at));
        }
        if self.bindings.contains_key(name) {
            return Some(self.name_expr(name, at));
        }
        if let Some(input) = catalog::inputs().iter().find(|i| i.ts_name == name) {
            if !self.require_import(name, at) {
                return None;
            }
            return Some(self.name_expr(input.id, at));
        }
        if self.sdk.contains(name) {
            self.error(
                "MT_SUBSET",
                format!("'{name}' is an SDK function and must be called."),
                at,
            );
            return None;
        }
        if catalog::inputs().iter().any(|i| i.id == name) {
            self.error(
                "MS_NAME",
                format!("Unknown SDK name '{name}'; use {}.", catalog::ts_name(name)),
                at,
            );
            return None;
        }
        self.error(
            "MS_NAME",
            format!("'{name}' is neither declared nor imported in this module."),
            at,
        );
        None
    }

    fn call(
        &mut self,
        node: &Node,
        callee: &Node,
        args: &[Node],
        locals: &BTreeSet<String>,
    ) -> Option<Expr> {
        let mut failed = false;
        let result = self.call_inner(node, callee, args, locals, &mut failed);
        if failed { None } else { result }
    }

    fn call_inner(
        &mut self,
        node: &Node,
        callee: &Node,
        args: &[Node],
        locals: &BTreeSet<String>,
        failed: &mut bool,
    ) -> Option<Expr> {
        let NodeKind::Name(name) = &callee.kind else {
            self.error(
                "MT_SUBSET",
                "Only named operations and helpers can be called.",
                &node.mark,
            );
            return None;
        };
        // Failed arguments already reported their errors; keep checking the callee for more.
        let mut lowered = Vec::with_capacity(args.len());
        for argument in args {
            match self.expr(argument, locals) {
                Some(expr) => lowered.push(expr),
                None => {
                    *failed = true;
                    lowered.push(self.num(0.0, &argument.mark));
                }
            }
        }
        let args = lowered;
        if args.len() > 16 {
            self.error(
                "MS_LIMIT",
                "Calls accept at most 16 arguments.",
                &callee.mark,
            );
            return None;
        }
        let at = &callee.mark;
        if locals.contains(name) {
            self.error(
                "MS_TYPE",
                format!("Parameter '{name}' is not callable."),
                at,
            );
            return None;
        }
        if self.declared.contains(name) || self.bindings.contains_key(name) {
            return Some(Expr::make(Expression::Call(name.clone(), args), at.clone()));
        }
        if self.sdk.contains(name) {
            if !self.require_import(name, at) {
                return None;
            }
            if let Some(operator) = catalog::find_operator(name) {
                let expected = if matches!(operator.id, "neg" | "not") {
                    1
                } else {
                    2
                };
                let mut parts = args.into_iter();
                let (first, second, extra) = (parts.next(), parts.next(), parts.next().is_some());
                return match (first, second, extra, expected) {
                    (Some(a), None, false, 1) => Some(Expr::make(
                        Expression::Unary(operator.symbol.into(), Box::new(a)),
                        at.clone(),
                    )),
                    (Some(a), Some(b), false, 2) => Some(Expr::make(
                        Expression::Binary(operator.symbol.into(), Box::new(a), Box::new(b)),
                        at.clone(),
                    )),
                    _ => {
                        self.error(
                            "MS_TYPE",
                            format!("{name} takes {expected} argument(s)."),
                            at,
                        );
                        None
                    }
                };
            }
            let id = catalog::ms_name(name);
            if catalog::find(&id).is_some() && catalog::ts_name(&id) == *name {
                return Some(Expr::make(Expression::Call(id, args), at.clone()));
            }
            self.error(
                "MT_SUBSET",
                format!("'{name}' is only valid as a parameter, layer, group, renderer operation or default export."),
                at,
            );
            return None;
        }
        if catalog::find(name).is_some() {
            self.error(
                "MS_NAME",
                format!("Unknown SDK name '{name}'; use {}.", catalog::ts_name(name)),
                at,
            );
            return None;
        }
        self.error(
            "MS_NAME",
            format!("Helper '{name}' is neither declared nor imported in this module."),
            at,
        );
        None
    }
}
