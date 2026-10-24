//! Modules and names: which declarations a file sees, and the one name
//! each declaration is known by everywhere after this.
//!
//! A program's own files are one module, whose declarations every one of
//! them sees, in any order and across files. A framework module
//! (`Fx.Controls`) is a set of `.tole` files inside this crate, seen by a
//! file that imports it; its declarations are written out as a Rust
//! module of their own (`fx_controls`), so a program's `Button` and the
//! controls' never meet. Where two have one name, a file sees its own
//! declaration first, then what it imports, then the rest of the
//! program's: what a file names explicitly wins.
//!
//! Resolving rewrites the parsed files: every name of a declared type,
//! component or singleton becomes its key, which is its Rust path from
//! the generated module's top (`Button`, `fx_controls::Button`). What
//! follows (gathering, checking, writing out) knows nothing of modules.

use std::collections::{BTreeMap, HashMap};

use crate::Diagnostic;
use crate::parser::{Decl, Element, Expr, File, Ident, Member, Stmt, StrPart};

/// The framework's modules: the name a file imports, the Rust module its
/// declarations are written into, and its files.
pub(crate) const MODULES: &[(&str, &str, &[Source])] = &[
    ("Fx", "", &[]),
    (
        "Fx.Controls",
        "fx_controls",
        &[
            (
                "Theme.tole",
                include_str!("../modules/Fx/Controls/Theme.tole"),
            ),
            (
                "Button.tole",
                include_str!("../modules/Fx/Controls/Button.tole"),
            ),
            (
                "CheckBox.tole",
                include_str!("../modules/Fx/Controls/CheckBox.tole"),
            ),
            (
                "RadioButton.tole",
                include_str!("../modules/Fx/Controls/RadioButton.tole"),
            ),
            (
                "Switch.tole",
                include_str!("../modules/Fx/Controls/Switch.tole"),
            ),
            (
                "Label.tole",
                include_str!("../modules/Fx/Controls/Label.tole"),
            ),
            (
                "TabBar.tole",
                include_str!("../modules/Fx/Controls/TabBar.tole"),
            ),
            (
                "ProgressBar.tole",
                include_str!("../modules/Fx/Controls/ProgressBar.tole"),
            ),
            (
                "GroupBox.tole",
                include_str!("../modules/Fx/Controls/GroupBox.tole"),
            ),
            (
                "TextField.tole",
                include_str!("../modules/Fx/Controls/TextField.tole"),
            ),
            (
                "TextArea.tole",
                include_str!("../modules/Fx/Controls/TextArea.tole"),
            ),
            (
                "ComboBox.tole",
                include_str!("../modules/Fx/Controls/ComboBox.tole"),
            ),
            (
                "Menu.tole",
                include_str!("../modules/Fx/Controls/Menu.tole"),
            ),
            (
                "MenuItem.tole",
                include_str!("../modules/Fx/Controls/MenuItem.tole"),
            ),
            (
                "MenuSeparator.tole",
                include_str!("../modules/Fx/Controls/MenuSeparator.tole"),
            ),
            (
                "ScrollBar.tole",
                include_str!("../modules/Fx/Controls/ScrollBar.tole"),
            ),
            (
                "FileIcon.tole",
                include_str!("../modules/Fx/Controls/FileIcon.tole"),
            ),
            (
                "CloseButton.tole",
                include_str!("../modules/Fx/Controls/CloseButton.tole"),
            ),
            (
                "UpButton.tole",
                include_str!("../modules/Fx/Controls/UpButton.tole"),
            ),
            (
                "ViewSwitch.tole",
                include_str!("../modules/Fx/Controls/ViewSwitch.tole"),
            ),
            (
                "SegmentedBar.tole",
                include_str!("../modules/Fx/Controls/SegmentedBar.tole"),
            ),
            (
                "SearchField.tole",
                include_str!("../modules/Fx/Controls/SearchField.tole"),
            ),
            (
                "Breadcrumbs.tole",
                include_str!("../modules/Fx/Controls/Breadcrumbs.tole"),
            ),
            (
                "PlaceRow.tole",
                include_str!("../modules/Fx/Controls/PlaceRow.tole"),
            ),
            (
                "PlacesList.tole",
                include_str!("../modules/Fx/Controls/PlacesList.tole"),
            ),
            (
                "FileListRow.tole",
                include_str!("../modules/Fx/Controls/FileListRow.tole"),
            ),
            (
                "FileTile.tole",
                include_str!("../modules/Fx/Controls/FileTile.tole"),
            ),
            (
                "FilePreview.tole",
                include_str!("../modules/Fx/Controls/FilePreview.tole"),
            ),
            (
                "PatternHelp.tole",
                include_str!("../modules/Fx/Controls/PatternHelp.tole"),
            ),
            (
                "FileDialog.tole",
                include_str!("../modules/Fx/Controls/FileDialog.tole"),
            ),
        ],
    ),
];

/// A file of a module: its name and its text.
pub(crate) type Source = (&'static str, &'static str);

/// The Rust module an imported module's declarations go in; `""` for the
/// program's own.
pub(crate) fn rust_module(module: &str) -> Option<&'static str> {
    MODULES
        .iter()
        .find(|(name, _, _)| *name == module)
        .map(|(_, rust, _)| *rust)
}

/// A parsed file, with the module it belongs to (`""`: the program's).
pub(crate) struct Unit {
    /// Its name, for diagnostics.
    pub name: String,
    /// Its module.
    pub module: String,
    /// Its text, parsed.
    pub file: File,
}

/// Rewrite every unit's names to keys. Answers the units, and the Rust
/// module each key's declaration goes in.
pub(crate) fn resolve(mut units: Vec<Unit>) -> Result<Vec<(String, File, String)>, Diagnostic> {
    // Every module's declarations: name to key.
    let mut declared: BTreeMap<String, HashMap<String, String>> = BTreeMap::new();
    for unit in &units {
        let rust = rust_module(&unit.module).unwrap_or("");
        let names = declared.entry(unit.module.clone()).or_default();
        for decl in &unit.file.decls {
            let name = decl_name(decl);
            let key = if rust.is_empty() {
                name.text.clone()
            } else {
                format!("{rust}::{}", name.text)
            };
            let _ = names.insert(name.text.clone(), key);
        }
    }
    let mut out = Vec::new();
    for unit in &mut units {
        // What this file sees: its module's names, its imports' over
        // them, and its own declarations over those.
        let mut seen: HashMap<String, String> = HashMap::new();
        if let Some(names) = declared.get(&unit.module) {
            seen.extend(names.iter().map(|(n, k)| (n.clone(), k.clone())));
        }
        for import in &unit.file.imports {
            if rust_module(&import.text).is_none() {
                return Err(Diagnostic {
                    file: unit.name.clone(),
                    pos: import.pos,
                    message: format!(
                        "there is no module `{}`: `Fx` and `Fx.Controls` are",
                        import.text
                    ),
                });
            }
            if let Some(names) = declared.get(&import.text) {
                seen.extend(names.iter().map(|(n, k)| (n.clone(), k.clone())));
            }
        }
        let rust = rust_module(&unit.module).unwrap_or("");
        for decl in &unit.file.decls {
            let name = &decl_name(decl).text;
            let key = if rust.is_empty() {
                name.clone()
            } else {
                format!("{rust}::{name}")
            };
            let _ = seen.insert(name.clone(), key);
        }
        let names = Names { seen: &seen };
        for decl in &mut unit.file.decls {
            names.decl(decl);
        }
        let rust = rust_module(&unit.module).unwrap_or("").to_owned();
        out.push((
            unit.name.clone(),
            File {
                imports: unit.file.imports.clone(),
                decls: core::mem::take(&mut unit.file.decls),
            },
            rust,
        ));
    }
    Ok(out)
}

/// A declaration's name.
fn decl_name(decl: &Decl) -> &Ident {
    match decl {
        Decl::Struct { name, .. }
        | Decl::Enum { name, .. }
        | Decl::Component { name, .. }
        | Decl::Singleton { name, .. } => name,
    }
}

/// The names a file sees, and their keys.
struct Names<'a> {
    /// Name to key.
    seen: &'a HashMap<String, String>,
}

impl Names<'_> {
    /// `ident` as its key, if it names a declaration.
    fn ident(&self, ident: &mut Ident) {
        if let Some(key) = self.seen.get(&ident.text) {
            ident.text.clone_from(key);
        }
    }

    /// A type's name, `list<T>` and `model<T>` included.
    fn ty(&self, ty: &mut Ident) {
        for wrapper in ["list<", "model<"] {
            if let Some(inner) = ty
                .text
                .strip_prefix(wrapper)
                .and_then(|rest| rest.strip_suffix('>'))
            {
                let mut inner = Ident {
                    text: inner.to_owned(),
                    pos: ty.pos,
                };
                self.ty(&mut inner);
                ty.text = format!("{wrapper}{}>", inner.text);
                return;
            }
        }
        self.ident(ty);
    }

    /// A declaration.
    fn decl(&self, decl: &mut Decl) {
        match decl {
            Decl::Struct { name, fields } => {
                self.ident(name);
                for (_, ty) in fields {
                    self.ty(ty);
                }
            }
            Decl::Enum { name, .. } => self.ident(name),
            Decl::Component { name, root } | Decl::Singleton { name, root } => {
                self.ident(name);
                self.element(root);
            }
        }
    }

    /// An element and everything in it.
    fn element(&self, element: &mut Element) {
        self.ident(&mut element.kind);
        for member in &mut element.members {
            match member {
                Member::Property { ty, init, .. } => {
                    self.ty(ty);
                    if let Some(init) = init {
                        self.expr(init);
                    }
                }
                Member::Id(_) => {}
                Member::Binding { expr, .. } => self.expr(expr),
                Member::Child(child) => self.element(child),
                Member::Function {
                    params,
                    defaults,
                    ret,
                    body,
                    ..
                } => {
                    for (_, ty) in params {
                        self.ty(ty);
                    }
                    for default in defaults.iter_mut().flatten() {
                        self.expr(default);
                    }
                    self.ty(ret);
                    self.expr(body);
                }
                Member::Signal { params, .. } => {
                    for (_, ty) in params {
                        self.ty(ty);
                    }
                }
                Member::On { element, .. } => self.element(element),
                Member::Elements { elements, .. } => {
                    for element in elements {
                        self.element(element);
                    }
                }
                Member::Handler { body, .. } => self.statements(body),
            }
        }
    }

    /// A handler's statements.
    fn statements(&self, body: &mut [Stmt]) {
        for statement in body {
            match statement {
                Stmt::Assign(path, expr) => {
                    if let Some(first) = path.first_mut() {
                        self.ident(first);
                    }
                    self.expr(expr);
                }
                Stmt::Call(expr) => self.expr(expr),
                Stmt::If(condition, yes, no) => {
                    self.expr(condition);
                    self.statements(yes);
                    self.statements(no);
                }
            }
        }
    }

    /// An expression: a path or call's first name may be an enum or a
    /// singleton.
    fn expr(&self, expr: &mut Expr) {
        match expr {
            Expr::Bool(..) | Expr::Int(..) | Expr::Real(..) | Expr::Color(..) => {}
            Expr::Str(parts, _) => {
                for part in parts {
                    if let StrPart::Expr(inner) = part {
                        self.expr(inner);
                    }
                }
            }
            Expr::Path(path) => {
                if path.len() > 1
                    && let Some(first) = path.first_mut()
                {
                    self.ident(first);
                }
            }
            Expr::Unary(_, inner, _) => self.expr(inner),
            Expr::Binary(_, a, b, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Ternary(c, a, b, _) => {
                self.expr(c);
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(path, args, _) => {
                if path.len() > 1
                    && let Some(first) = path.first_mut()
                {
                    self.ident(first);
                }
                for arg in args {
                    self.expr(arg);
                }
            }
            Expr::List(items, _) => {
                for item in items {
                    self.expr(item);
                }
            }
            Expr::Index(list, index, _, _) => {
                self.expr(list);
                self.expr(index);
            }
        }
    }
}
