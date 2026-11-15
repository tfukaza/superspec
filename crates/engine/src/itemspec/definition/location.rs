use super::ItemDiagnostic;

/// Resolves a JSON pointer to one-based line/column and zero-based UTF-16 offsets.
/// Missing fields point at their nearest existing ancestor. Call only with the
/// text belonging to `diagnostic.source`; GLB diagnostics name their own asset.
pub fn locate_diagnostic(text: &str, diagnostic: &mut ItemDiagnostic) {
    if diagnostic.offset.is_some() || text.len() > 1024 * 1024 {
        return;
    }
    let (start, end) = if let Some(line) = diagnostic.line {
        let line_start = text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            .min(text.len());
        let mut start = line_start
            .saturating_add(diagnostic.column.unwrap_or(1).saturating_sub(1))
            .min(text.len());
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let end = start + text[start..].chars().next().map_or(0, char::len_utf8);
        (start, end)
    } else {
        let tokens: Vec<String> = if diagnostic.pointer == "/" || diagnostic.pointer.is_empty() {
            vec![]
        } else {
            diagnostic
                .pointer
                .strip_prefix('/')
                .unwrap_or(&diagnostic.pointer)
                .split('/')
                .map(|part| part.replace("~1", "/").replace("~0", "~"))
                .collect()
        };
        let mut scanner = Scanner {
            text,
            cursor: 0,
            target: &tokens,
            best: None,
        };
        if scanner.value(&mut Vec::new(), 0).is_err() {
            return;
        }
        scanner.best.unwrap_or((0, text.len()))
    };
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    diagnostic.line = Some(text[..start].bytes().filter(|b| *b == b'\n').count() + 1);
    diagnostic.column = Some(text[line_start..start].encode_utf16().count() + 1);
    diagnostic.offset = Some(text[..start].encode_utf16().count());
    diagnostic.end = Some(text[..end].encode_utf16().count());
}

struct Scanner<'a> {
    text: &'a str,
    cursor: usize,
    target: &'a [String],
    best: Option<(usize, usize)>,
}
impl Scanner<'_> {
    fn whitespace(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }
    fn string(&mut self) -> Result<String, ()> {
        let start = self.cursor;
        if self.text.as_bytes().get(self.cursor) != Some(&b'"') {
            return Err(());
        }
        self.cursor += 1;
        while let Some(&byte) = self.text.as_bytes().get(self.cursor) {
            self.cursor += 1;
            if byte == b'\\' {
                self.cursor += 1;
            } else if byte == b'"' {
                return serde_json::from_str(&self.text[start..self.cursor]).map_err(|_| ());
            }
        }
        Err(())
    }
    fn value(&mut self, path: &mut Vec<String>, depth: usize) -> Result<(), ()> {
        if depth > 128 {
            return Err(());
        }
        self.whitespace();
        let start = self.cursor;
        match self.text.as_bytes().get(self.cursor) {
            Some(b'{') => {
                self.cursor += 1;
                self.whitespace();
                if self.text.as_bytes().get(self.cursor) != Some(&b'}') {
                    loop {
                        self.whitespace();
                        let key = self.string()?;
                        self.whitespace();
                        if self.text.as_bytes().get(self.cursor) != Some(&b':') {
                            return Err(());
                        }
                        self.cursor += 1;
                        path.push(key);
                        self.value(path, depth + 1)?;
                        path.pop();
                        self.whitespace();
                        if self.text.as_bytes().get(self.cursor) != Some(&b',') {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                if self.text.as_bytes().get(self.cursor) != Some(&b'}') {
                    return Err(());
                }
                self.cursor += 1;
            }
            Some(b'[') => {
                self.cursor += 1;
                self.whitespace();
                let mut index = 0;
                if self.text.as_bytes().get(self.cursor) != Some(&b']') {
                    loop {
                        path.push(index.to_string());
                        self.value(path, depth + 1)?;
                        path.pop();
                        index += 1;
                        self.whitespace();
                        if self.text.as_bytes().get(self.cursor) != Some(&b',') {
                            break;
                        }
                        self.cursor += 1;
                    }
                }
                if self.text.as_bytes().get(self.cursor) != Some(&b']') {
                    return Err(());
                }
                self.cursor += 1;
            }
            Some(b'"') => {
                self.string()?;
            }
            Some(_) => {
                while self
                    .text
                    .as_bytes()
                    .get(self.cursor)
                    .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
                {
                    self.cursor += 1;
                }
                if self.cursor == start {
                    return Err(());
                }
            }
            None => return Err(()),
        }
        if self.target.starts_with(path) && self.best.is_none() {
            self.best = Some((start, self.cursor));
        }
        Ok(())
    }
}
