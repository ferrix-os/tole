//! The markup's tokens.
//!
//! Newlines are tokens: a binding ends at the end of its line, as in QML,
//! unless a bracket is open. A string's `${expr}` parts are lexed here into
//! the pieces the parser reads back as an expression.

use crate::{Diagnostic, Pos};

/// One token.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    /// A name.
    Ident(String),
    /// A whole number.
    Int(i64),
    /// A number with a point.
    Real(f64),
    /// A string: its literal pieces and the source of its `${}` parts.
    Str(Vec<StrPiece>),
    /// `#rrggbb` or `#rrggbbaa`.
    Color(u32, bool),
    /// Punctuation or an operator.
    Punct(&'static str),
    /// The end of a line, outside brackets.
    Newline,
    /// The end of the file.
    End,
}

/// A piece of a string literal.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StrPiece {
    /// Text as written, escapes undone.
    Text(String),
    /// A `${...}` part: its tokens.
    Expr(Vec<(Tok, Pos)>),
}

/// Operators and punctuation, longest first.
const PUNCTS: [&str; 30] = [
    "===", "!==", "=>", "==", "!=", "<=", ">=", "&&", "||", "{", "}", "(", ")", "[", "]", ":", ";",
    ",", ".", "?", "+", "-", "*", "/", "%", "<", ">", "!", "=", "@",
];

/// `source`'s tokens, each with where it starts.
pub(crate) fn lex(file: &str, source: &str) -> Result<Vec<(Tok, Pos)>, Diagnostic> {
    let mut lexer = Lexer {
        file,
        chars: source.chars().collect(),
        at: 0,
        line: 1,
        column: 1,
        depth: 0,
    };
    lexer.all(false)
}

/// The state of the scan.
struct Lexer<'a> {
    /// The file's name, for diagnostics.
    file: &'a str,
    /// The source.
    chars: Vec<char>,
    /// The next character.
    at: usize,
    /// Its line.
    line: u32,
    /// Its column.
    column: u32,
    /// How many brackets are open.
    depth: u32,
}

impl Lexer<'_> {
    /// Where the next character is.
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            column: self.column,
        }
    }

    /// The character `ahead` places on.
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    /// Take a character.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.at += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    /// A diagnostic at `pos`.
    fn error(&self, pos: Pos, message: &str) -> Diagnostic {
        Diagnostic {
            file: self.file.to_owned(),
            pos,
            message: message.to_owned(),
        }
    }

    /// Every token to the end, or, `inside` a `${}`, to its `}`.
    fn all(&mut self, inside: bool) -> Result<Vec<(Tok, Pos)>, Diagnostic> {
        let mut out = Vec::new();
        let mut braces = 0_u32;
        loop {
            self.skip_space()?;
            let pos = self.pos();
            let Some(c) = self.peek(0) else {
                if inside {
                    return Err(self.error(pos, "a `${` in a string is not closed"));
                }
                out.push((Tok::End, pos));
                return Ok(out);
            };
            if inside && c == '}' && braces == 0 {
                let _ = self.bump();
                out.push((Tok::End, pos));
                return Ok(out);
            }
            let tok = if c == '\n' {
                let _ = self.bump();
                if self.depth > 0 || inside {
                    continue;
                }
                Tok::Newline
            } else if c.is_ascii_digit() {
                self.number()?
            } else if c.is_alphabetic() || c == '_' {
                let mut name = String::new();
                while let Some(c) = self.peek(0).filter(|c| c.is_alphanumeric() || *c == '_') {
                    name.push(c);
                    let _ = self.bump();
                }
                Tok::Ident(name)
            } else if c == '"' {
                self.string()?
            } else if c == '#' {
                self.color()?
            } else {
                let Some(punct) = PUNCTS.iter().find(|p| {
                    p.chars()
                        .enumerate()
                        .all(|(i, pc)| self.peek(i) == Some(pc))
                }) else {
                    return Err(self.error(pos, &format!("`{c}` is not part of the language")));
                };
                for _ in 0..punct.len() {
                    let _ = self.bump();
                }
                match *punct {
                    "(" | "[" => self.depth += 1,
                    ")" | "]" => self.depth = self.depth.saturating_sub(1),
                    "{" => braces += 1,
                    "}" => braces = braces.saturating_sub(1),
                    _ => {}
                }
                Tok::Punct(punct)
            };
            out.push((tok, pos));
        }
    }

    /// Spaces, tabs, carriage returns and comments; not newlines.
    fn skip_space(&mut self) -> Result<(), Diagnostic> {
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(' ' | '\t' | '\r'), _) => {
                    let _ = self.bump();
                }
                (Some('/'), Some('/')) => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        let _ = self.bump();
                    }
                }
                (Some('/'), Some('*')) => {
                    let pos = self.pos();
                    let _ = (self.bump(), self.bump());
                    loop {
                        match (self.peek(0), self.peek(1)) {
                            (Some('*'), Some('/')) => {
                                let _ = (self.bump(), self.bump());
                                break;
                            }
                            (Some(_), _) => {
                                let _ = self.bump();
                            }
                            (None, _) => {
                                return Err(self.error(pos, "a `/*` comment is not closed"));
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// A number, whole or with a point, and an optional unit suffix (`ms`).
    fn number(&mut self) -> Result<Tok, Diagnostic> {
        let pos = self.pos();
        let mut text = String::new();
        let mut real = false;
        while let Some(c) = self.peek(0) {
            if c.is_ascii_digit() || c == '_' {
                if c != '_' {
                    text.push(c);
                }
            } else if c == '.' && !real && self.peek(1).is_some_and(|d| d.is_ascii_digit()) {
                real = true;
                text.push(c);
            } else {
                break;
            }
            let _ = self.bump();
        }
        if real {
            text.parse()
                .map(Tok::Real)
                .map_err(|_| self.error(pos, "this number cannot be read"))
        } else {
            text.parse()
                .map(Tok::Int)
                .map_err(|_| self.error(pos, "this number is too large"))
        }
    }

    /// A string literal, with its escapes and `${}` parts.
    fn string(&mut self) -> Result<Tok, Diagnostic> {
        let pos = self.pos();
        let _ = self.bump();
        let mut pieces = Vec::new();
        let mut text = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => {
                    return Err(self.error(pos, "a string is not closed on its line"));
                }
                Some('"') => break,
                Some('\\') => match self.bump() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(c @ ('\\' | '"' | '$')) => text.push(c),
                    _ => {
                        return Err(
                            self.error(self.pos(), "an escape is \\n, \\t, \\\\, \\\" or \\$")
                        );
                    }
                },
                Some('$') if self.peek(0) == Some('{') => {
                    let _ = self.bump();
                    if !text.is_empty() {
                        pieces.push(StrPiece::Text(core::mem::take(&mut text)));
                    }
                    let tokens = self.all(true)?;
                    pieces.push(StrPiece::Expr(tokens));
                }
                Some(c) => text.push(c),
            }
        }
        if !text.is_empty() || pieces.is_empty() {
            pieces.push(StrPiece::Text(text));
        }
        Ok(Tok::Str(pieces))
    }

    /// `#rrggbb` or `#rrggbbaa`.
    fn color(&mut self) -> Result<Tok, Diagnostic> {
        let pos = self.pos();
        let _ = self.bump();
        let mut digits = String::new();
        while let Some(c) = self.peek(0).filter(char::is_ascii_hexdigit) {
            digits.push(c);
            let _ = self.bump();
        }
        let value = u32::from_str_radix(&digits, 16)
            .map_err(|_| self.error(pos, "a colour is #rrggbb or #rrggbbaa"))?;
        match digits.len() {
            6 => Ok(Tok::Color(value, false)),
            8 => Ok(Tok::Color(value, true)),
            _ => Err(self.error(pos, "a colour is #rrggbb or #rrggbbaa")),
        }
    }
}
