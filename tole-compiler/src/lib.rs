//! tole-compiler: `.tole` files into Rust, at build time
//! (Ferrix's `docs/UI-FRAMEWORK.md` §3.3).
//!
//! A program's `build.rs` calls [`build`] with the directory its `.tole`
//! files are in; the Rust it writes to `OUT_DIR` is put in place by
//! `tole::include_ui!()`. Every file is parsed, every declaration of every
//! file gathered, and every expression type-checked before anything is
//! written: an error names its file, line and column, and the build
//! stops there. Nothing of this crate ends up in the program.
//!
//! What it knows today (U2): `struct`, `enum` and `component`
//! declarations; the kinds `Item`, `Rectangle`, `Text`, `Row` and
//! `Column`; ids and `parent`; declared properties; bindings in a typed
//! expression language (literals, `${}` strings, arithmetic, comparisons,
//! `? :`, enum values, struct fields, built-in and the component's own
//! functions); grouped properties (`border { width: 1 }`); anchors;
//! signals and handlers (U3a); focus, `Keys.on...` handlers with their
//! `event`, `KeyNavigation` and `forceActiveFocus()` (U3c); `list<T>` and
//! `model<T>`, list literals and indexing, `Layout.*`, and `Repeater`
//! delegates with `modelData` and `index` (U4a); `Flickable`,
//! `ListView`, `delegate:`, `DelegateChooser` and `group()` (U4b);
//! `states`, `transitions`, `Behavior on`, animations `on` a property,
//! `Easing.*` and `Animation.Infinite` (U5); `import`ed modules, the
//! framework's `Fx.Controls` among them, `singleton`s, property aliases
//! and a component's `default property alias` for where a use's children
//! go, parameters with defaults, and `Exclusive.checked` (U6).

mod emit;
mod lexer;
mod parser;
mod resolve;
mod types;

use std::fmt;
use std::path::{Path, PathBuf};

/// Where in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    /// From 1.
    pub line: u32,
    /// From 1, in characters.
    pub column: u32,
}

/// What is wrong, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file, as it was named to the compiler.
    pub file: String,
    /// Where in it.
    pub pos: Pos,
    /// What.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.pos.line, self.pos.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

/// `sources`, each a file's name and text, as one Rust module's text.
///
/// # Errors
///
/// The first thing wrong, with its place.
pub fn compile(sources: &[(&str, &str)]) -> Result<String, Diagnostic> {
    let mut units = Vec::new();
    for (name, text) in sources {
        let tokens = lexer::lex(name, text)?;
        units.push(resolve::Unit {
            name: (*name).to_owned(),
            module: String::new(),
            file: parser::parse(name, tokens)?,
        });
    }
    // The framework's modules the files import, and those they import.
    let mut loaded: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(unit) = units.get(at) {
        let wanted: Vec<String> = unit
            .file
            .imports
            .iter()
            .map(|import| import.text.clone())
            .filter(|name| !loaded.contains(name))
            .collect();
        for module in wanted {
            if let Some((_, _, files)) = resolve::MODULES.iter().find(|(m, _, _)| *m == module) {
                for (file, text) in *files {
                    let name = format!("<{module}>/{file}");
                    let tokens = lexer::lex(&name, text)?;
                    let file = parser::parse(&name, tokens)?;
                    units.push(resolve::Unit {
                        name,
                        module: module.clone(),
                        file,
                    });
                }
            }
            loaded.push(module);
        }
        at += 1;
    }
    let files = resolve::resolve(units)?;
    emit::Program::gather(&files)?.emit()
}

/// Compile every `.tole` file under `dir` (relative to the crate being
/// built) into `$OUT_DIR/tole_ui.rs`, and tell cargo to build again when
/// any of them changes. For a `build.rs`; it prints the diagnostic and
/// answers it on a failure, so `build("ui").unwrap_or_else(|_| exit(1))`
/// stops the build with the place shown.
///
/// # Errors
///
/// A file that cannot be read, an error in one, or an `OUT_DIR` that
/// cannot be written.
#[expect(
    clippy::print_stdout,
    reason = "cargo reads a build script's instructions from its standard output"
)]
pub fn build(dir: impl AsRef<Path>) -> Result<(), Diagnostic> {
    let dir = dir.as_ref();
    let fail = |message: String| Diagnostic {
        file: dir.display().to_string(),
        pos: Pos { line: 0, column: 0 },
        message,
    };
    let mut paths = Vec::new();
    collect(dir, &mut paths).map_err(|error| fail(format!("cannot be read: {error}")))?;
    paths.sort();
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut texts = Vec::new();
    for path in &paths {
        println!("cargo::rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(path)
            .map_err(|error| fail(format!("{}: {error}", path.display())))?;
        texts.push((path.display().to_string(), text));
    }
    let sources: Vec<(&str, &str)> = texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    let rust = compile(&sources).inspect_err(|diagnostic| {
        println!("cargo::error={diagnostic}");
    })?;
    let out = std::env::var_os("OUT_DIR")
        .ok_or_else(|| fail("OUT_DIR is not set: build() is for a build.rs".to_owned()))?;
    std::fs::write(PathBuf::from(out).join("tole_ui.rs"), rust)
        .map_err(|error| fail(format!("OUT_DIR: {error}")))
}

/// Every `.tole` file under `dir`.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "tole") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
