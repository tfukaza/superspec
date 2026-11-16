use super::{ProgramDiagnostic, Sources, Type, diagnostic};

#[derive(Clone, Debug)]
pub(super) struct Mark {
    pub module: String,
    pub start: usize,
    pub end: usize,
}
impl Mark {
    pub fn root(module: &str) -> Self {
        Self {
            module: module.into(),
            start: 0,
            end: 0,
        }
    }
}
#[derive(Clone)]
pub(super) struct Expr {
    pub kind: Expression,
    pub mark: Mark,
    pub(super) depth: usize,
}
impl Expr {
    /// Build a node the way the parser does, tracking nesting depth.
    pub(super) fn make(kind: Expression, mark: Mark) -> Self {
        let depth = 1 + match &kind {
            Expression::Unary(_, a) | Expression::Member(a, _) => a.depth,
            Expression::Binary(_, a, b) => a.depth.max(b.depth),
            Expression::Call(_, args) => args.iter().map(|v| v.depth).max().unwrap_or(0),
            _ => 0,
        };
        Self { kind, mark, depth }
    }
    /// Point every node at one source span, for code expanded from a template.
    pub(super) fn remark(&mut self, mark: &Mark) {
        self.mark = mark.clone();
        match &mut self.kind {
            Expression::Unary(_, a) | Expression::Member(a, _) => a.remark(mark),
            Expression::Binary(_, a, b) => {
                a.remark(mark);
                b.remark(mark);
            }
            Expression::Call(_, args) => args.iter_mut().for_each(|a| a.remark(mark)),
            _ => {}
        }
    }
    /// Rename bindings (not call targets) for which the callback returns a replacement.
    pub(super) fn rename(&mut self, replace: &dyn Fn(&str) -> Option<String>) {
        match &mut self.kind {
            Expression::Name(name) => {
                if let Some(new) = replace(name) {
                    *name = new;
                }
            }
            Expression::Unary(_, a) | Expression::Member(a, _) => a.rename(replace),
            Expression::Binary(_, a, b) => {
                a.rename(replace);
                b.rename(replace);
            }
            Expression::Call(_, args) => args.iter_mut().for_each(|a| a.rename(replace)),
            _ => {}
        }
    }
}
#[derive(Clone)]
pub(super) enum Expression {
    Number(f64),
    Bool(bool),
    Text(String),
    Name(String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Member(Box<Expr>, String),
}
#[derive(Clone)]
pub(super) enum Definition {
    Parameter {
        kind: Type,
        value: Expr,
        range: Option<[f64; 2]>,
        unit: Option<String>,
        label: Option<String>,
        mark: Mark,
    },
    Field {
        value: Expr,
        kind: Option<Type>,
        mark: Mark,
    },
    Function {
        arguments: Vec<(String, Type)>,
        result: Type,
        value: Expr,
        mark: Mark,
    },
}
impl Definition {
    pub fn mark(&self) -> &Mark {
        match self {
            Self::Parameter { mark, .. }
            | Self::Field { mark, .. }
            | Self::Function { mark, .. } => mark,
        }
    }
    pub(super) fn remark(&mut self, at: &Mark) {
        match self {
            Self::Parameter { value, mark, .. }
            | Self::Field { value, mark, .. }
            | Self::Function { value, mark, .. } => {
                *mark = at.clone();
                value.remark(at);
            }
        }
    }
    pub(super) fn rename(&mut self, replace: &dyn Fn(&str) -> Option<String>) {
        match self {
            Self::Parameter { value, .. }
            | Self::Field { value, .. }
            | Self::Function { value, .. } => value.rename(replace),
        }
    }
    pub fn value(&self) -> &Expr {
        match self {
            Self::Parameter { value, .. }
            | Self::Field { value, .. }
            | Self::Function { value, .. } => value,
        }
    }
}
pub(super) struct Module {
    pub imports: Vec<(String, Mark)>,
    pub declarations: Vec<(String, Definition)>,
    pub outputs: Vec<(String, Expr)>,
    pub live: Vec<super::LiveOperation>,
    pub traits: super::Traits,
}
#[derive(Clone, Debug)]
enum Token {
    Name(String),
    Number(f64),
    Text(String),
    Symbol(String),
    End,
}
#[derive(Clone)]
struct Located {
    token: Token,
    mark: Mark,
}

pub(super) fn parse(sources: &Sources, module: &str) -> Result<Module, ProgramDiagnostic> {
    let text = &sources.modules[module];
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
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
                return Err(diagnostic(
                    sources,
                    "MS_SYNTAX",
                    "Unclosed comment.",
                    &Mark {
                        module: module.into(),
                        start: i,
                        end: text.len(),
                    },
                ));
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
            while i < bytes.len() && bytes[i].is_ascii_digit() {
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
            let value = text[start..i].parse::<f64>().ok().filter(|n| n.is_finite());
            let Some(value) = value else {
                return Err(diagnostic(
                    sources,
                    "MS_SYNTAX",
                    "Expected a finite number.",
                    &Mark {
                        module: module.into(),
                        start,
                        end: i,
                    },
                ));
            };
            Token::Number(value)
        } else if bytes[i] == b'"' {
            i += 1;
            let mut escaped = false;
            while i < bytes.len() {
                if bytes[i] == b'"' && !escaped {
                    i += 1;
                    break;
                }
                escaped = bytes[i] == b'\\' && !escaped;
                i += 1;
            }
            let value = serde_json::from_str::<String>(&text[start..i]).map_err(|_| {
                diagnostic(
                    sources,
                    "MS_SYNTAX",
                    "Invalid or unclosed string.",
                    &Mark {
                        module: module.into(),
                        start,
                        end: i,
                    },
                )
            })?;
            Token::Text(value)
        } else {
            let pair = bytes
                .get(i..i + 2)
                .and_then(|b| std::str::from_utf8(b).ok());
            if pair.is_some_and(|s| matches!(s, "->" | "<=" | ">=" | "==" | "!=" | "&&" | "||")) {
                i += 2;
                Token::Symbol(text[start..i].into())
            } else if b";:,()=+-*/%<>!.".contains(&bytes[i]) {
                i += 1;
                Token::Symbol(text[start..i].into())
            } else {
                let end = i + text[i..].chars().next().unwrap().len_utf8();
                return Err(diagnostic(
                    sources,
                    "MS_SYNTAX",
                    "Unexpected character.",
                    &Mark {
                        module: module.into(),
                        start: i,
                        end,
                    },
                ));
            }
        };
        tokens.push(Located {
            token,
            mark: Mark {
                module: module.into(),
                start,
                end: i,
            },
        });
        if tokens.len() > 65_536 {
            return Err(diagnostic(
                sources,
                "MS_LIMIT",
                "Module exceeds the token budget.",
                &Mark::root(module),
            ));
        }
    }
    tokens.push(Located {
        token: Token::End,
        mark: Mark {
            module: module.into(),
            start: i,
            end: i,
        },
    });
    Parser {
        sources,
        tokens,
        cursor: 0,
        depth: 0,
        nodes: 0,
    }
    .module()
}

struct Parser<'a> {
    sources: &'a Sources,
    tokens: Vec<Located>,
    cursor: usize,
    depth: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn node(&mut self, kind: Expression, mark: Mark) -> Result<Expr, ProgramDiagnostic> {
        self.nodes += 1;
        let depth = 1 + match &kind {
            Expression::Unary(_, a) | Expression::Member(a, _) => a.depth,
            Expression::Binary(_, a, b) => a.depth.max(b.depth),
            Expression::Call(_, args) => args.iter().map(|v| v.depth).max().unwrap_or(0),
            _ => 0,
        };
        if depth > 96 || self.nodes > 8192 {
            return Err(diagnostic(
                self.sources,
                "MS_LIMIT",
                "Expression exceeds the syntax depth or node budget.",
                &mark,
            ));
        }
        Ok(Expr { kind, mark, depth })
    }
    fn current(&self) -> &Located {
        &self.tokens[self.cursor]
    }
    fn advance(&mut self) -> Located {
        let token = self.current().clone();
        if !matches!(token.token, Token::End) {
            self.cursor += 1;
        }
        token
    }
    fn error(&self, message: impl Into<String>) -> ProgramDiagnostic {
        diagnostic(self.sources, "MS_SYNTAX", message, &self.current().mark)
    }
    fn symbol(&mut self, s: &str) -> bool {
        if matches!(&self.current().token,Token::Symbol(v) if v==s) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<(), ProgramDiagnostic> {
        if self.symbol(s) {
            Ok(())
        } else {
            Err(self.error(format!("Expected '{s}'.")))
        }
    }
    fn name(&mut self) -> Result<(String, Mark), ProgramDiagnostic> {
        let t = self.advance();
        if let Token::Name(s) = t.token {
            Ok((s, t.mark))
        } else {
            Err(diagnostic(
                self.sources,
                "MS_SYNTAX",
                "Expected a name.",
                &t.mark,
            ))
        }
    }
    fn kind(&mut self) -> Result<Type, ProgramDiagnostic> {
        let (name, mark) = self.name()?;
        match name.as_str() {
            "scalar" => Ok(Type::Scalar),
            "vec2" => Ok(Type::Vec2),
            "vec3" => Ok(Type::Vec3),
            "bool" => Ok(Type::Bool),
            _ => Err(diagnostic(
                self.sources,
                "MS_TYPE",
                "Expected scalar, vec2, vec3 or bool.",
                &mark,
            )),
        }
    }
    fn number(&mut self) -> Result<f64, ProgramDiagnostic> {
        let sign = if self.symbol("-") {
            -1.
        } else {
            self.symbol("+");
            1.
        };
        let t = self.advance();
        match t.token {
            Token::Number(v) => Ok(v * sign),
            _ => Err(diagnostic(
                self.sources,
                "MS_SYNTAX",
                "Expected a numeric range bound.",
                &t.mark,
            )),
        }
    }
    fn module(&mut self) -> Result<Module, ProgramDiagnostic> {
        let (header, mark) = self.name()?;
        if header != "material" {
            return Err(diagnostic(
                self.sources,
                "MS_VERSION",
                "A module must begin with 'material 1;'.",
                &mark,
            ));
        }
        let version = self.advance();
        if !matches!(version.token, Token::Number(1.)) {
            return Err(diagnostic(
                self.sources,
                "MS_VERSION",
                "Only material language version 1 is supported.",
                &version.mark,
            ));
        }
        self.expect(";")?;
        let mut module = Module {
            imports: Vec::new(),
            declarations: Vec::new(),
            outputs: Vec::new(),
            live: Vec::new(),
            traits: Default::default(),
        };
        while !matches!(self.current().token, Token::End) {
            let (keyword, mark) = self.name()?;
            match keyword.as_str() {
                "import" => {
                    let token = self.advance();
                    let Token::Text(path) = token.token else {
                        return Err(diagnostic(
                            self.sources,
                            "MS_SYNTAX",
                            "Expected a quoted relative module path.",
                            &token.mark,
                        ));
                    };
                    module.imports.push((path, token.mark));
                }
                "param" => {
                    let (name, mark) = self.name()?;
                    self.expect(":")?;
                    let kind = self.kind()?;
                    self.expect("=")?;
                    let value = self.expr(0)?;
                    let range = if matches!(&self.current().token,Token::Name(n) if n=="range") {
                        self.advance();
                        self.expect("(")?;
                        let lo = self.number()?;
                        self.expect(",")?;
                        let hi = self.number()?;
                        self.expect(")")?;
                        Some([lo, hi])
                    } else {
                        None
                    };
                    module.declarations.push((
                        name,
                        Definition::Parameter {
                            kind,
                            value,
                            range,
                            unit: None,
                            label: None,
                            mark,
                        },
                    ));
                }
                "let" => {
                    let (name, mark) = self.name()?;
                    let kind = if self.symbol(":") {
                        Some(self.kind()?)
                    } else {
                        None
                    };
                    self.expect("=")?;
                    let value = self.expr(0)?;
                    module
                        .declarations
                        .push((name, Definition::Field { value, kind, mark }));
                }
                "fn" => {
                    let (name, mark) = self.name()?;
                    self.expect("(")?;
                    let mut arguments = Vec::new();
                    if !self.symbol(")") {
                        loop {
                            let (name, _) = self.name()?;
                            self.expect(":")?;
                            arguments.push((name, self.kind()?));
                            if self.symbol(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    self.expect("->")?;
                    let result = self.kind()?;
                    self.expect("=")?;
                    let value = self.expr(0)?;
                    module.declarations.push((
                        name,
                        Definition::Function {
                            arguments,
                            result,
                            value,
                            mark,
                        },
                    ));
                }
                "output" => {
                    let (name, _) = self.name()?;
                    self.expect("=")?;
                    module.outputs.push((name, self.expr(0)?));
                }
                _ => {
                    return Err(diagnostic(
                        self.sources,
                        "MS_SYNTAX",
                        "Expected import, param, let, fn or output.",
                        &mark,
                    ));
                }
            }
            self.expect(";")?;
            if module.declarations.len() + module.outputs.len() > 512 {
                return Err(self.error("Module exceeds its declaration budget."));
            }
        }
        Ok(module)
    }
    fn expr(&mut self, min: u8) -> Result<Expr, ProgramDiagnostic> {
        self.depth += 1;
        if self.depth > 96 {
            return Err(self.error("Expression nesting exceeds 96 levels."));
        }
        let token = self.advance();
        let mut left = match token.token {
            Token::Number(value) => self.node(Expression::Number(value), token.mark)?,
            Token::Text(value) => self.node(Expression::Text(value), token.mark)?,
            Token::Name(name) => {
                let kind = if name == "true" {
                    Expression::Bool(true)
                } else if name == "false" {
                    Expression::Bool(false)
                } else if self.symbol("(") {
                    let mut arguments = Vec::new();
                    if !self.symbol(")") {
                        loop {
                            arguments.push(self.expr(0)?);
                            if self.symbol(")") {
                                break;
                            }
                            self.expect(",")?;
                            if arguments.len() > 16 {
                                return Err(self.error("Functions accept at most 16 arguments."));
                            }
                        }
                    }
                    Expression::Call(name, arguments)
                } else {
                    Expression::Name(name)
                };
                self.node(kind, token.mark)?
            }
            Token::Symbol(ref s) if s == "(" => {
                let expr = self.expr(0)?;
                self.expect(")")?;
                expr
            }
            Token::Symbol(s) if matches!(s.as_str(), "-" | "+" | "!") => {
                let value = self.expr(8)?;
                self.node(Expression::Unary(s, Box::new(value)), token.mark)?
            }
            _ => {
                return Err(diagnostic(
                    self.sources,
                    "MS_SYNTAX",
                    "Expected an expression.",
                    &token.mark,
                ));
            }
        };
        loop {
            if self.symbol(".") {
                let (field, mark) = self.name()?;
                left = self.node(Expression::Member(Box::new(left), field), mark)?;
                continue;
            }
            let Token::Symbol(operator) = &self.current().token else {
                break;
            };
            let precedence = match operator.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | ">" | "<=" | ">=" => 4,
                "+" | "-" => 5,
                "*" | "/" | "%" => 6,
                _ => 0,
            };
            if precedence == 0 || precedence < min {
                break;
            }
            let operator = self.advance();
            let Token::Symbol(op) = operator.token else {
                unreachable!()
            };
            let right = self.expr(precedence + 1)?;
            left = self.node(
                Expression::Binary(op, Box::new(left), Box::new(right)),
                operator.mark,
            )?;
        }
        self.depth -= 1;
        Ok(left)
    }
}
