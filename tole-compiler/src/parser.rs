//! The markup's grammar, into a tree.
//!
//! ```text
//! file      := { import | struct | enum | component }
//! import    := "import" name { "." name } [ version ]
//! struct    := "struct" Name "{" { field (";" | "," | newline) } "}"
//! enum      := "enum" Name "{" Name { "," Name } "}"
//! component := ["export"] "component" Name ":" Base "{" members "}"
//! member    := ["readonly" | "required"] "property" type name [":" expr]
//!            | "id" ":" name
//!            | path ":" expr                 (a binding; path is a.b.c)
//!            | group "{" { name ":" expr } "}"   (border { width: 1 })
//!            | Type "{" members "}"          (a child: its type starts upper case)
//!            | "function" name "(" params ")" ":" type "{" "return" expr "}"
//!            | "signal" name "(" params ")"
//! ```

use crate::lexer::{StrPiece, Tok};
use crate::{Diagnostic, Pos};

/// A name and where it was written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ident {
    /// The name.
    pub text: String,
    /// Where.
    pub pos: Pos,
}

/// A file.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct File {
    /// Its declarations.
    pub decls: Vec<Decl>,
}

/// A top-level declaration.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Decl {
    /// `struct Name { field: type, ... }`.
    Struct {
        name: Ident,
        fields: Vec<(Ident, Ident)>,
    },
    /// `enum Name { A, B }`.
    Enum { name: Ident, variants: Vec<Ident> },
    /// `component Name : Base { ... }`.
    Component { name: Ident, root: Element },
}

/// An element: a type and its members.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Element {
    /// Its type: a kind of item or a component.
    pub kind: Ident,
    /// What it says.
    pub members: Vec<Member>,
}

/// One member of an element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Member {
    /// `property type name: expr`.
    Property {
        name: Ident,
        ty: Ident,
        init: Option<Expr>,
    },
    /// `id: name`.
    Id(Ident),
    /// `a.b: expr`.
    Binding { path: Vec<Ident>, expr: Expr },
    /// A child element.
    Child(Element),
    /// `function name(params): type { return expr }`.
    Function {
        name: Ident,
        params: Vec<(Ident, Ident)>,
        ret: Ident,
        body: Expr,
    },
    /// `signal name(params)`.
    Signal {
        name: Ident,
        params: Vec<(Ident, Ident)>,
    },
    /// `onName: statements`, or `Keys.onName: statements` for an attached
    /// object's signal.
    Handler {
        attached: Option<Ident>,
        signal: Ident,
        body: Vec<Stmt>,
    },
}

/// A statement of a handler.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Stmt {
    /// `a.b = expr`.
    Assign(Vec<Ident>, Expr),
    /// A call: a signal emitted or a function run.
    Call(Expr),
    /// `if (c) ... else ...`.
    If(Expr, Vec<Stmt>, Vec<Stmt>),
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    /// `true`, `false`.
    Bool(bool, Pos),
    /// `12`.
    Int(i64, Pos),
    /// `0.5`.
    Real(f64, Pos),
    /// `"text ${expr}"`.
    Str(Vec<StrPart>, Pos),
    /// `#rrggbbaa`, as RGBA.
    Color(u32, Pos),
    /// `a.b.c`.
    Path(Vec<Ident>),
    /// `-x`, `!x`.
    Unary(&'static str, Box<Expr>, Pos),
    /// `a + b`.
    Binary(&'static str, Box<Expr>, Box<Expr>, Pos),
    /// `c ? a : b`.
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>, Pos),
    /// `f(a, b)` or `a.f(b)`.
    Call(Vec<Ident>, Vec<Expr>, Pos),
}

/// A part of a string expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StrPart {
    /// Text.
    Text(String),
    /// A `${}` part.
    Expr(Expr),
}

impl Expr {
    /// Where it starts.
    pub(crate) fn pos(&self) -> Pos {
        match self {
            Expr::Bool(_, pos)
            | Expr::Int(_, pos)
            | Expr::Real(_, pos)
            | Expr::Str(_, pos)
            | Expr::Color(_, pos)
            | Expr::Unary(_, _, pos)
            | Expr::Binary(_, _, _, pos)
            | Expr::Ternary(_, _, _, pos)
            | Expr::Call(_, _, pos) => *pos,
            Expr::Path(path) => path
                .first()
                .map_or(Pos { line: 0, column: 0 }, |first| first.pos),
        }
    }
}

/// `tokens` as a file.
pub(crate) fn parse(file: &str, tokens: Vec<(Tok, Pos)>) -> Result<File, Diagnostic> {
    let mut parser = Parser {
        file,
        tokens,
        at: 0,
    };
    parser.file()
}

/// The state of the parse.
struct Parser<'a> {
    /// The file's name.
    file: &'a str,
    /// The tokens, ending in [`Tok::End`].
    tokens: Vec<(Tok, Pos)>,
    /// The next one.
    at: usize,
}

/// Binary operators by binding power, loosest first.
const LEVELS: [&[&str]; 6] = [
    &["||"],
    &["&&"],
    &["==", "!=", "===", "!=="],
    &["<", "<=", ">", ">="],
    &["+", "-"],
    &["*", "/", "%"],
];

impl Parser<'_> {
    /// The next token.
    fn peek(&self) -> &Tok {
        self.tokens.get(self.at).map_or(&Tok::End, |(tok, _)| tok)
    }

    /// The token after it.
    fn peek2(&self) -> &Tok {
        self.tokens
            .get(self.at + 1)
            .map_or(&Tok::End, |(tok, _)| tok)
    }

    /// Where the next token is.
    fn pos(&self) -> Pos {
        self.tokens
            .get(self.at)
            .map_or(Pos { line: 0, column: 0 }, |(_, pos)| *pos)
    }

    /// Take the next token.
    fn bump(&mut self) -> Tok {
        let tok = self.peek().clone();
        if self.at < self.tokens.len() {
            self.at += 1;
        }
        tok
    }

    /// A diagnostic at the next token.
    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic {
            file: self.file.to_owned(),
            pos: self.pos(),
            message: message.to_owned(),
        }
    }

    /// Whether the next token is `punct`; taken if so.
    fn eat(&mut self, punct: &str) -> bool {
        if matches!(self.peek(), Tok::Punct(p) if *p == punct) {
            let _ = self.bump();
            true
        } else {
            false
        }
    }

    /// `punct`, or an error.
    fn expect(&mut self, punct: &str) -> Result<(), Diagnostic> {
        if self.eat(punct) {
            Ok(())
        } else {
            Err(self.error(&format!("`{punct}` was expected here")))
        }
    }

    /// Skip newlines and semicolons.
    fn separators(&mut self) {
        while matches!(self.peek(), Tok::Newline | Tok::Punct(";")) {
            let _ = self.bump();
        }
    }

    /// Skip newlines.
    fn newlines(&mut self) {
        while matches!(self.peek(), Tok::Newline) {
            let _ = self.bump();
        }
    }

    /// A name.
    fn ident(&mut self) -> Result<Ident, Diagnostic> {
        let pos = self.pos();
        match self.bump() {
            Tok::Ident(text) => Ok(Ident { text, pos }),
            _ => {
                self.at = self.at.saturating_sub(1);
                Err(self.error("a name was expected here"))
            }
        }
    }

    /// Whether the next token is the word `word`.
    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Ident(text) if text == word)
    }

    /// The whole file.
    fn file(&mut self) -> Result<File, Diagnostic> {
        let mut decls = Vec::new();
        loop {
            self.separators();
            if matches!(self.peek(), Tok::End) {
                return Ok(File { decls });
            }
            if self.at_word("import") {
                while !matches!(self.peek(), Tok::Newline | Tok::End | Tok::Punct(";")) {
                    let _ = self.bump();
                }
            } else if self.at_word("struct") {
                let _ = self.bump();
                decls.push(self.structure()?);
            } else if self.at_word("enum") {
                let _ = self.bump();
                decls.push(self.enumeration()?);
            } else {
                if self.at_word("export") {
                    let _ = self.bump();
                }
                if !self.at_word("component") {
                    return Err(self.error(
                        "a file holds `import`, `struct`, `enum` and `component` declarations",
                    ));
                }
                let _ = self.bump();
                let name = self.ident()?;
                self.expect(":")?;
                let kind = self.ident()?;
                let root = self.element_body(kind)?;
                decls.push(Decl::Component { name, root });
            }
        }
    }

    /// A struct's name and fields.
    fn structure(&mut self) -> Result<Decl, Diagnostic> {
        let name = self.ident()?;
        self.newlines();
        self.expect("{")?;
        let mut fields = Vec::new();
        loop {
            while matches!(self.peek(), Tok::Newline | Tok::Punct(";" | ",")) {
                let _ = self.bump();
            }
            if self.eat("}") {
                return Ok(Decl::Struct { name, fields });
            }
            let field = self.ident()?;
            self.expect(":")?;
            fields.push((field, self.ident()?));
        }
    }

    /// An enum's name and variants.
    fn enumeration(&mut self) -> Result<Decl, Diagnostic> {
        let name = self.ident()?;
        self.newlines();
        self.expect("{")?;
        let mut variants = Vec::new();
        loop {
            while matches!(self.peek(), Tok::Newline | Tok::Punct(",")) {
                let _ = self.bump();
            }
            if self.eat("}") {
                return Ok(Decl::Enum { name, variants });
            }
            variants.push(self.ident()?);
        }
    }

    /// `{ members }` of an element of `kind`.
    fn element_body(&mut self, kind: Ident) -> Result<Element, Diagnostic> {
        self.newlines();
        self.expect("{")?;
        let mut members = Vec::new();
        loop {
            self.separators();
            if self.eat("}") {
                return Ok(Element { kind, members });
            }
            members.extend(self.member()?);
        }
    }

    /// One member, or a group's several.
    fn member(&mut self) -> Result<Vec<Member>, Diagnostic> {
        if self.at_word("readonly") || self.at_word("required") || self.at_word("default") {
            let _ = self.bump();
        }
        if self.at_word("property") && matches!(self.peek2(), Tok::Ident(_)) {
            let _ = self.bump();
            let ty = self.ident()?;
            let name = self.ident()?;
            let init = if self.eat(":") {
                Some(self.expr()?)
            } else {
                None
            };
            return Ok(vec![Member::Property { name, ty, init }]);
        }
        if self.at_word("function") && matches!(self.peek2(), Tok::Ident(_)) {
            let _ = self.bump();
            let name = self.ident()?;
            let params = self.params()?;
            self.expect(":")?;
            let ret = self.ident()?;
            self.newlines();
            self.expect("{")?;
            self.separators();
            if !self.at_word("return") {
                return Err(self.error("a function's body is `return` and an expression"));
            }
            let _ = self.bump();
            let body = self.expr()?;
            self.separators();
            self.expect("}")?;
            return Ok(vec![Member::Function {
                name,
                params,
                ret,
                body,
            }]);
        }
        if self.at_word("signal") && matches!(self.peek2(), Tok::Ident(_)) {
            let _ = self.bump();
            let name = self.ident()?;
            let params = if matches!(self.peek(), Tok::Punct("(")) {
                self.params()?
            } else {
                Vec::new()
            };
            return Ok(vec![Member::Signal { name, params }]);
        }
        let first = self.ident()?;
        if first.text == "id" && self.eat(":") {
            return Ok(vec![Member::Id(self.ident()?)]);
        }
        if let Some(signal) = handler_name(&first)
            && self.eat(":")
        {
            let body = self.block()?;
            return Ok(vec![Member::Handler {
                attached: None,
                signal,
                body,
            }]);
        }
        let starts_upper = first.text.chars().next().is_some_and(char::is_uppercase);
        if starts_upper && matches!(self.peek(), Tok::Punct("{") | Tok::Newline) {
            return Ok(vec![Member::Child(self.element_body(first)?)]);
        }
        let mut path = vec![first];
        while self.eat(".") {
            path.push(self.ident()?);
        }
        if let [attached, second] = path.as_slice()
            && attached.text.chars().next().is_some_and(char::is_uppercase)
            && let Some(signal) = handler_name(second)
            && self.eat(":")
        {
            let attached = Some(attached.clone());
            let body = self.block()?;
            return Ok(vec![Member::Handler {
                attached,
                signal,
                body,
            }]);
        }
        if matches!(self.peek(), Tok::Punct("{")) {
            // A group: border { width: 1; color: ... }.
            let _ = self.bump();
            let mut members = Vec::new();
            loop {
                self.separators();
                if self.eat("}") {
                    return Ok(members);
                }
                let mut inner = path.clone();
                inner.push(self.ident()?);
                self.expect(":")?;
                members.push(Member::Binding {
                    path: inner,
                    expr: self.expr()?,
                });
            }
        }
        self.expect(":")?;
        Ok(vec![Member::Binding {
            path,
            expr: self.expr()?,
        }])
    }

    /// A handler's body: `{ statements }` or one statement.
    fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if !self.eat("{") {
            return Ok(vec![self.statement()?]);
        }
        let mut body = Vec::new();
        loop {
            self.separators();
            if self.eat("}") {
                return Ok(body);
            }
            body.push(self.statement()?);
        }
    }

    /// One statement.
    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        if self.at_word("if") {
            let _ = self.bump();
            self.expect("(")?;
            let condition = self.expr()?;
            self.expect(")")?;
            self.newlines();
            let yes = self.block()?;
            let save = self.at;
            self.separators();
            let no = if self.at_word("else") {
                let _ = self.bump();
                self.newlines();
                if self.at_word("if") {
                    vec![self.statement()?]
                } else {
                    self.block()?
                }
            } else {
                self.at = save;
                Vec::new()
            };
            return Ok(Stmt::If(condition, yes, no));
        }
        let pos = self.pos();
        let expr = self.expr()?;
        if self.eat("=") {
            let Expr::Path(path) = expr else {
                return Err(Diagnostic {
                    file: self.file.to_owned(),
                    pos,
                    message: "only a property can be assigned to".to_owned(),
                });
            };
            return Ok(Stmt::Assign(path, self.expr()?));
        }
        if !matches!(expr, Expr::Call(..)) {
            return Err(Diagnostic {
                file: self.file.to_owned(),
                pos,
                message: "a statement is an assignment, a call or an `if`".to_owned(),
            });
        }
        Ok(Stmt::Call(expr))
    }

    /// `(name: type, ...)`, or `(type name, ...)` as a QML signal writes it.
    fn params(&mut self) -> Result<Vec<(Ident, Ident)>, Diagnostic> {
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            let first = self.ident()?;
            if self.eat(":") {
                params.push((first, self.ident()?));
            } else {
                let name = self.ident()?;
                params.push((name, first));
            }
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(params)
    }

    /// An expression.
    pub(crate) fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let condition = self.binary(0)?;
        if self.continues_with("?") {
            let pos = self.pos();
            let _ = self.bump();
            self.newlines();
            let yes = self.expr()?;
            if !self.continues_with(":") {
                return Err(self.error("`:` was expected: `condition ? this : that`"));
            }
            let _ = self.bump();
            self.newlines();
            let no = self.expr()?;
            return Ok(Expr::Ternary(
                Box::new(condition),
                Box::new(yes),
                Box::new(no),
                pos,
            ));
        }
        Ok(condition)
    }

    /// Whether the expression goes on with `punct`, on this line or at the
    /// start of the next; the newline is skipped if so.
    fn continues_with(&mut self, punct: &str) -> bool {
        if matches!(self.peek(), Tok::Punct(p) if *p == punct) {
            return true;
        }
        if matches!(self.peek(), Tok::Newline)
            && matches!(self.peek2(), Tok::Punct(p) if *p == punct)
        {
            let _ = self.bump();
            return true;
        }
        false
    }

    /// Binary operators from `level` on.
    fn binary(&mut self, level: usize) -> Result<Expr, Diagnostic> {
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        loop {
            let Some(&op) = ops.iter().find(|op| self.continues_with(op)) else {
                return Ok(left);
            };
            let pos = self.pos();
            let _ = self.bump();
            self.newlines();
            let right = self.binary(level + 1)?;
            let op = match op {
                "===" => "==",
                "!==" => "!=",
                other => other,
            };
            left = Expr::Binary(op, Box::new(left), Box::new(right), pos);
        }
    }

    /// `-x`, `!x`, or a primary.
    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        let pos = self.pos();
        if self.eat("-") {
            return Ok(Expr::Unary("-", Box::new(self.unary()?), pos));
        }
        if self.eat("!") {
            return Ok(Expr::Unary("!", Box::new(self.unary()?), pos));
        }
        self.primary()
    }

    /// A literal, a path, a call, or a bracketed expression.
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let pos = self.pos();
        match self.bump() {
            Tok::Int(value) => Ok(Expr::Int(value, pos)),
            Tok::Real(value) => Ok(Expr::Real(value, pos)),
            Tok::Color(value, alpha) => Ok(Expr::Color(
                if alpha { value } else { (value << 8) | 0xff },
                pos,
            )),
            Tok::Str(pieces) => {
                let mut parts = Vec::new();
                for piece in pieces {
                    match piece {
                        StrPiece::Text(text) => parts.push(StrPart::Text(text)),
                        StrPiece::Expr(tokens) => {
                            let mut inner = Parser {
                                file: self.file,
                                tokens,
                                at: 0,
                            };
                            let expr = inner.expr()?;
                            if !matches!(inner.peek(), Tok::End) {
                                return Err(inner.error("a `${}` holds one expression"));
                            }
                            parts.push(StrPart::Expr(expr));
                        }
                    }
                }
                Ok(Expr::Str(parts, pos))
            }
            Tok::Punct("(") => {
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::Ident(word) if word == "true" || word == "false" => {
                Ok(Expr::Bool(word == "true", pos))
            }
            Tok::Ident(text) => {
                let mut path = vec![Ident { text, pos }];
                while self.eat(".") {
                    path.push(self.ident()?);
                }
                if self.eat("(") {
                    let mut args = Vec::new();
                    while !self.eat(")") {
                        args.push(self.expr()?);
                        if !self.eat(",") {
                            self.expect(")")?;
                            break;
                        }
                    }
                    return Ok(Expr::Call(path, args, pos));
                }
                Ok(Expr::Path(path))
            }
            _ => {
                self.at = self.at.saturating_sub(1);
                Err(self.error("an expression was expected here"))
            }
        }
    }
}

/// The signal `onName` handles: `name` with its first letter lowered.
fn handler_name(word: &Ident) -> Option<Ident> {
    let rest = word.text.strip_prefix("on")?;
    let mut chars = rest.chars();
    let first = chars.next().filter(|c| c.is_uppercase())?;
    Some(Ident {
        text: first.to_lowercase().chain(chars).collect(),
        pos: word.pos,
    })
}
