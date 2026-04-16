//! Thin PostScript tokeniser sufficient for dvips output.
//!
//! We don't execute PostScript — we only identify tokens that matter for
//! text extraction: literal strings `(…)` (their byte contents are glyph
//! codes), hex strings `<…>` (we skip payload), names (`/foo`), bare
//! names (operators + font selectors), numbers, and procedure braces
//! (tracked so we can skip over the procset).

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token<'a> {
    /// A literal string `(abc)`. Contents are raw bytes after backslash
    /// escape resolution.
    LitString(Vec<u8>),
    /// A hex string `<deadbeef>`. We keep the payload bytes decoded.
    HexString(Vec<u8>),
    /// A name `/foo`.
    Name(&'a str),
    /// A bare word — operator or executable name like `def`, `bop`, `Fa`, `a`.
    Word(&'a str),
    /// A numeric literal (integer or real). We keep the raw text and let
    /// the caller parse to f64 as needed.
    Number(f64),
    /// Open procedure brace `{`.
    ProcOpen,
    /// Close procedure brace `}`.
    ProcClose,
    /// Open array bracket `[`.
    ArrayOpen,
    /// Close array bracket `]`.
    ArrayClose,
    /// Dict-marker `<<` (not strictly needed for dvips but cheap to emit).
    DictOpen,
    /// Dict-end `>>`.
    DictClose,
}

pub(crate) struct Tokeniser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Tokeniser<'a> {
    pub fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Return the next token, skipping whitespace and comments. `None`
    /// indicates end-of-input.
    pub fn next(&mut self) -> Option<Token<'a>> {
        loop {
            self.skip_ws();
            let b = *self.src.get(self.pos)?;
            return Some(match b {
                b'%' => {
                    self.skip_line_comment();
                    continue;
                }
                b'(' => self.read_literal_string()?,
                b'<' => {
                    if self.peek1() == Some(b'<') {
                        self.pos += 2;
                        Token::DictOpen
                    } else {
                        self.read_hex_string()?
                    }
                }
                b'>' => {
                    if self.peek1() == Some(b'>') {
                        self.pos += 2;
                        Token::DictClose
                    } else {
                        // A bare '>' isn't valid PostScript; skip it.
                        self.pos += 1;
                        continue;
                    }
                }
                b'/' => self.read_name(),
                b'{' => {
                    self.pos += 1;
                    Token::ProcOpen
                }
                b'}' => {
                    self.pos += 1;
                    Token::ProcClose
                }
                b'[' => {
                    self.pos += 1;
                    Token::ArrayOpen
                }
                b']' => {
                    self.pos += 1;
                    Token::ArrayClose
                }
                _ => self.read_word_or_number(),
            });
        }
    }

    fn peek1(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x00) {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            self.pos += 1;
            if b == b'\n' {
                break;
            }
        }
    }

    fn read_literal_string(&mut self) -> Option<Token<'a>> {
        self.pos += 1; // consume '('
        let mut depth = 1usize;
        let mut out = Vec::new();
        while depth > 0 {
            let b = *self.src.get(self.pos)?;
            self.pos += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth > 0 {
                        out.push(b')');
                    }
                }
                b'\\' => {
                    // Handle PostScript escape sequences.
                    let esc = *self.src.get(self.pos)?;
                    self.pos += 1;
                    match esc {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'\\' => out.push(b'\\'),
                        b'(' => out.push(b'('),
                        b')' => out.push(b')'),
                        b'\n' => { /* line continuation — drop */ }
                        b'\r' => {
                            if self.src.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        d @ b'0'..=b'7' => {
                            // Octal escape, up to 3 digits total.
                            let mut v: u16 = (d - b'0') as u16;
                            for _ in 0..2 {
                                match self.src.get(self.pos).copied() {
                                    Some(d2 @ b'0'..=b'7') => {
                                        v = v * 8 + (d2 - b'0') as u16;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xFF) as u8);
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(b),
            }
        }
        Some(Token::LitString(out))
    }

    fn read_hex_string(&mut self) -> Option<Token<'a>> {
        self.pos += 1; // consume '<'
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;
        loop {
            let b = *self.src.get(self.pos)?;
            self.pos += 1;
            if b == b'>' {
                if let Some(h) = hi {
                    out.push(h << 4);
                }
                return Some(Token::HexString(out));
            }
            if let Some(n) = hex_val(b) {
                match hi {
                    None => hi = Some(n),
                    Some(h) => {
                        out.push((h << 4) | n);
                        hi = None;
                    }
                }
            }
            // Non-hex bytes inside <…> are whitespace; ignore.
        }
    }

    fn read_name(&mut self) -> Token<'a> {
        self.pos += 1; // consume '/'
        let start = self.pos;
        while let Some(&b) = self.src.get(self.pos) {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        Token::Name(s)
    }

    fn read_word_or_number(&mut self) -> Token<'a> {
        let start = self.pos;
        while let Some(&b) = self.src.get(self.pos) {
            if is_regular(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        if let Ok(n) = s.parse::<f64>() {
            return Token::Number(n);
        }
        // Accept integer with leading +, and negative numbers that start
        // with '-' and then digits.
        if let Some(stripped) = s.strip_prefix('+') {
            if let Ok(n) = stripped.parse::<f64>() {
                return Token::Number(n);
            }
        }
        Token::Word(s)
    }
}

fn is_regular(b: u8) -> bool {
    !matches!(
        b,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | 0x0C
            | 0x00
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
    )
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &[u8]) -> Vec<Token> {
        let mut t = Tokeniser::new(s);
        let mut out = Vec::new();
        while let Some(tok) = t.next() {
            out.push(tok);
        }
        out
    }

    #[test]
    fn simple_words_and_numbers() {
        let toks = tokens(b"1 2.5 -3 foo /Bar");
        assert_eq!(
            toks,
            vec![
                Token::Number(1.0),
                Token::Number(2.5),
                Token::Number(-3.0),
                Token::Word("foo"),
                Token::Name("Bar"),
            ]
        );
    }

    #[test]
    fn literal_string_with_escapes() {
        // "alt\014hought" with octal escape for 0x0C (form-feed in plain PS,
        // but for dvips/OT1 this is the ffi-ligature glyph code — the
        // tokeniser doesn't care about semantics, just that the byte
        // round-trips.)
        let toks = tokens(b"(alt\\014hought)");
        let Token::LitString(bytes) = &toks[0] else {
            panic!("expected literal string")
        };
        assert_eq!(bytes.as_slice(), b"alt\x0chought");
    }

    #[test]
    fn nested_parens() {
        let toks = tokens(b"(a(b)c)");
        let Token::LitString(bytes) = &toks[0] else {
            panic!()
        };
        assert_eq!(bytes.as_slice(), b"a(b)c");
    }

    #[test]
    fn hex_string() {
        let toks = tokens(b"<48 65 6C 6C 6F>");
        let Token::HexString(bytes) = &toks[0] else {
            panic!()
        };
        assert_eq!(bytes.as_slice(), b"Hello");
    }

    #[test]
    fn dict_vs_hex() {
        let toks = tokens(b"<< /Foo 1 >>");
        assert_eq!(
            toks,
            vec![
                Token::DictOpen,
                Token::Name("Foo"),
                Token::Number(1.0),
                Token::DictClose,
            ]
        );
    }

    #[test]
    fn comment_ignored() {
        let toks = tokens(b"1 % the answer is\n2");
        assert_eq!(toks, vec![Token::Number(1.0), Token::Number(2.0)]);
    }

    #[test]
    fn proc_braces() {
        let toks = tokens(b"{ pop add }");
        assert_eq!(
            toks,
            vec![
                Token::ProcOpen,
                Token::Word("pop"),
                Token::Word("add"),
                Token::ProcClose,
            ]
        );
    }

    #[test]
    fn dvips_snippet_round_trips() {
        // Actual CADE1996.ps content — font select + move + show.
        // Tokens: 621 194 a Fx (W) -6 b (alther) 22 b (Recursion) = 11.
        let toks = tokens(b"621 194 a Fx(W)-6 b(alther)22 b(Recursion)");
        assert_eq!(toks.len(), 11);
        assert!(matches!(toks[3], Token::Word("Fx")));
        if let Token::LitString(s) = &toks[4] {
            assert_eq!(s.as_slice(), b"W");
        } else {
            panic!("expected W literal at index 4")
        }
        if let Token::LitString(s) = &toks[7] {
            assert_eq!(s.as_slice(), b"alther");
        } else {
            panic!("expected alther literal at index 7")
        }
        if let Token::LitString(s) = &toks[10] {
            assert_eq!(s.as_slice(), b"Recursion");
        } else {
            panic!("expected Recursion literal at index 10")
        }
    }
}
