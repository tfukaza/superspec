//! Derived-value expressions: numbers, names, `+ - * / %`, comparisons
//! (`< <= > >= == !=`, which give 1 or 0), parentheses and `min`, `max`, `floor`,
//! `ceil`, `round`, `abs`, `clamp(x, low, high)`, `select(condition, a, b)` (`a` when
//! the condition is positive) and `random(...)`, a value in [0, 1) hashed from the
//! bits of its arguments. Every operation is IEEE arithmetic or integer hashing, so
//! native and Wasm results are bit-identical.

/// A failure inside one expression. `offset` is a zero-based byte offset into its text.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionError {
    pub offset: usize,
    pub message: String,
}
fn error<T>(offset: usize, message: impl Into<String>) -> Result<T, ExpressionError> {
    Err(ExpressionError {
        offset,
        message: message.into(),
    })
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Number(f64),
    Name(String, usize),
    Negate(Box<Node>),
    Binary(u8, Box<Node>, Box<Node>, usize),
    Call(Function, Vec<Node>, usize),
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Function {
    Min,
    Max,
    Floor,
    Ceil,
    Round,
    Abs,
    Clamp,
    Select,
    Random,
}

/// A value in [0, 1) from the exact bits of `values`: SplitMix64 steps folded over them.
pub fn random(values: &[f64]) -> f64 {
    let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ values.len() as u64;
    for value in values {
        // Treat -0 as 0 so equal numbers hash alike.
        let bits = if *value == 0. { 0 } else { value.to_bits() };
        state = state.wrapping_add(bits).wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        state = z ^ (z >> 31);
    }
    (state >> 11) as f64 / (1u64 << 53) as f64
}

/// A parsed expression. Parsing is separate from evaluation so a definition is checked once.
#[derive(Clone, Debug, PartialEq)]
pub struct Expression(Node);

const MAXIMUM_LENGTH: usize = 1024;
const MAXIMUM_DEPTH: usize = 32;

struct Parser<'a> {
    text: &'a [u8],
    cursor: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.text.get(self.cursor).is_some_and(|b| *b == b' ') {
            self.cursor += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.text.get(self.cursor).copied()
    }
    fn comparison(&mut self, depth: usize) -> Result<Node, ExpressionError> {
        let left = self.sum(depth)?;
        let at = self.cursor;
        let operator = match (self.peek(), self.text.get(self.cursor + 1).copied()) {
            (Some(b'<'), Some(b'=')) => b'l',
            (Some(b'>'), Some(b'=')) => b'g',
            (Some(b'='), Some(b'=')) => b'e',
            (Some(b'!'), Some(b'=')) => b'n',
            (Some(b'<'), _) => b'<',
            (Some(b'>'), _) => b'>',
            _ => return Ok(left),
        };
        self.cursor += if matches!(operator, b'<' | b'>') {
            1
        } else {
            2
        };
        let right = self.sum(depth)?;
        Ok(Node::Binary(operator, Box::new(left), Box::new(right), at))
    }
    fn sum(&mut self, depth: usize) -> Result<Node, ExpressionError> {
        let mut left = self.product(depth)?;
        while let Some(operator @ (b'+' | b'-')) = self.peek() {
            let at = self.cursor;
            self.cursor += 1;
            let right = self.product(depth)?;
            left = Node::Binary(operator, Box::new(left), Box::new(right), at);
        }
        Ok(left)
    }
    fn product(&mut self, depth: usize) -> Result<Node, ExpressionError> {
        let mut left = self.unary(depth)?;
        while let Some(operator @ (b'*' | b'/' | b'%')) = self.peek() {
            let at = self.cursor;
            self.cursor += 1;
            let right = self.unary(depth)?;
            left = Node::Binary(operator, Box::new(left), Box::new(right), at);
        }
        Ok(left)
    }
    fn unary(&mut self, depth: usize) -> Result<Node, ExpressionError> {
        if depth > MAXIMUM_DEPTH {
            return error(self.cursor, "Expression nesting exceeds 32 levels.");
        }
        match self.peek() {
            Some(b'-') => {
                self.cursor += 1;
                Ok(Node::Negate(Box::new(self.unary(depth + 1)?)))
            }
            Some(b'+') => {
                self.cursor += 1;
                self.unary(depth + 1)
            }
            _ => self.atom(depth),
        }
    }
    fn atom(&mut self, depth: usize) -> Result<Node, ExpressionError> {
        let start = self.cursor;
        match self.peek() {
            Some(b'(') => {
                self.cursor += 1;
                let inner = self.comparison(depth + 1)?;
                self.expect(b')')?;
                Ok(inner)
            }
            Some(byte) if byte.is_ascii_digit() || byte == b'.' => {
                let start = self.cursor;
                while self
                    .text
                    .get(self.cursor)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
                {
                    self.cursor += 1;
                }
                if self.text.get(self.cursor).is_some_and(|b| *b | 32 == b'e') {
                    let mark = self.cursor;
                    self.cursor += 1;
                    if self
                        .text
                        .get(self.cursor)
                        .is_some_and(|b| *b == b'+' || *b == b'-')
                    {
                        self.cursor += 1;
                    }
                    if !self.text.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                        return error(mark, "Exponent needs digits.");
                    }
                    while self.text.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                        self.cursor += 1;
                    }
                }
                let text = std::str::from_utf8(&self.text[start..self.cursor]).unwrap_or("");
                match text.parse::<f64>() {
                    Ok(number) if number.is_finite() => Ok(Node::Number(number)),
                    _ => error(start, format!("Invalid number {text}.")),
                }
            }
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = self.cursor;
                while self
                    .text
                    .get(self.cursor)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    self.cursor += 1;
                }
                let name = std::str::from_utf8(&self.text[start..self.cursor])
                    .unwrap_or("")
                    .to_owned();
                if self.peek() != Some(b'(') {
                    return Ok(Node::Name(name, start));
                }
                let function = match name.as_str() {
                    "min" => Function::Min,
                    "max" => Function::Max,
                    "floor" => Function::Floor,
                    "ceil" => Function::Ceil,
                    "round" => Function::Round,
                    "abs" => Function::Abs,
                    "clamp" => Function::Clamp,
                    "select" => Function::Select,
                    "random" => Function::Random,
                    _ => return error(start, format!("Unknown function {name}.")),
                };
                self.cursor += 1;
                let mut arguments = vec![self.comparison(depth + 1)?];
                while self.peek() == Some(b',') {
                    self.cursor += 1;
                    if arguments.len() >= 16 {
                        return error(self.cursor, "A function takes at most 16 arguments.");
                    }
                    arguments.push(self.comparison(depth + 1)?);
                }
                self.expect(b')')?;
                let valid = match function {
                    Function::Min | Function::Max => arguments.len() >= 2,
                    Function::Clamp | Function::Select => arguments.len() == 3,
                    Function::Random => !arguments.is_empty(),
                    _ => arguments.len() == 1,
                };
                if !valid {
                    return error(start, format!("Wrong number of arguments for {name}."));
                }
                Ok(Node::Call(function, arguments, start))
            }
            Some(_) => error(start.max(self.cursor), "Unexpected character."),
            None => error(self.cursor, "Expression ends early."),
        }
    }
    fn expect(&mut self, byte: u8) -> Result<(), ExpressionError> {
        if self.peek() == Some(byte) {
            self.cursor += 1;
            Ok(())
        } else {
            error(self.cursor, format!("Expected '{}'.", byte as char))
        }
    }
}

impl Expression {
    pub fn parse(text: &str) -> Result<Self, ExpressionError> {
        if text.len() > MAXIMUM_LENGTH {
            return error(0, "Expression exceeds 1024 bytes.");
        }
        let mut parser = Parser {
            text: text.as_bytes(),
            cursor: 0,
        };
        let node = parser.comparison(0)?;
        if parser.peek().is_some() {
            return error(parser.cursor, "Unexpected character.");
        }
        Ok(Self(node))
    }

    /// Names in first-use order, with the offset of each first use.
    pub fn names(&self) -> Vec<(&str, usize)> {
        fn visit<'a>(node: &'a Node, names: &mut Vec<(&'a str, usize)>) {
            match node {
                Node::Number(_) => {}
                Node::Name(name, at) => {
                    if !names.iter().any(|(known, _)| known == name) {
                        names.push((name, *at));
                    }
                }
                Node::Negate(inner) => visit(inner, names),
                Node::Binary(_, left, right, _) => {
                    visit(left, names);
                    visit(right, names);
                }
                Node::Call(_, arguments, _) => arguments.iter().for_each(|a| visit(a, names)),
            }
        }
        let mut names = Vec::new();
        visit(&self.0, &mut names);
        names
    }

    /// `resolve` receives a name and the offset of its use.
    pub fn evaluate(
        &self,
        resolve: &mut dyn FnMut(&str, usize) -> Result<f64, ExpressionError>,
    ) -> Result<f64, ExpressionError> {
        fn value(
            node: &Node,
            resolve: &mut dyn FnMut(&str, usize) -> Result<f64, ExpressionError>,
        ) -> Result<f64, ExpressionError> {
            let (result, at) = match node {
                Node::Number(number) => return Ok(*number),
                Node::Name(name, at) => (resolve(name, *at)?, *at),
                Node::Negate(inner) => return Ok(-value(inner, resolve)?),
                Node::Binary(operator, left, right, at) => {
                    let (a, b) = (value(left, resolve)?, value(right, resolve)?);
                    (
                        match operator {
                            b'+' => a + b,
                            b'-' => a - b,
                            b'*' => a * b,
                            b'<' => f64::from(a < b),
                            b'>' => f64::from(a > b),
                            b'l' => f64::from(a <= b),
                            b'g' => f64::from(a >= b),
                            b'e' => f64::from(a == b),
                            b'n' => f64::from(a != b),
                            _ if b == 0. => return error(*at, "Division by zero."),
                            b'%' => a - b * (a / b).floor(),
                            _ => a / b,
                        },
                        *at,
                    )
                }
                Node::Call(function, arguments, at) => {
                    let mut values = Vec::with_capacity(arguments.len());
                    for argument in arguments {
                        values.push(value(argument, resolve)?);
                    }
                    (
                        match function {
                            Function::Min => values.into_iter().fold(f64::INFINITY, f64::min),
                            Function::Max => values.into_iter().fold(f64::NEG_INFINITY, f64::max),
                            Function::Floor => values[0].floor(),
                            Function::Ceil => values[0].ceil(),
                            // Half away from zero, as `f64::round` defines it.
                            Function::Round => values[0].round(),
                            Function::Abs => values[0].abs(),
                            Function::Clamp => values[0].max(values[1]).min(values[2]),
                            Function::Select => {
                                if values[0] > 0. {
                                    values[1]
                                } else {
                                    values[2]
                                }
                            }
                            Function::Random => random(&values),
                        },
                        *at,
                    )
                }
            };
            if result.is_finite() {
                Ok(result)
            } else {
                error(at, "Expression result is not finite.")
            }
        }
        value(&self.0, resolve)
    }
}
