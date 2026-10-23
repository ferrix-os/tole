//! Checking and writing out: every declaration of every file, resolved
//! and type-checked, as Rust against the runtime's API.
//!
//! A component becomes a `Copy` struct of its root's handle and its
//! declared properties, with a `new(ui, parent)` that makes its items in
//! tree order, then gives every property its value or binding, then
//! anchors. Bindings are lazy, so a binding may read an item declared
//! after it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::parser::{Decl, Element, Expr, File, Ident, Member, Stmt, StrPart};
use crate::types::{
    Kind, Ty, builtin_constant, keys_handler_keysym, layout_attached, named_color, snake,
};
use crate::{Diagnostic, Pos};

/// What an element is: a kind of item or a component.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ElemTy {
    /// A kind the runtime makes.
    Kind(Kind),
    /// A component.
    Component(String),
}

/// A component, as the rest of the program sees it.
#[derive(Clone, Debug)]
struct Component {
    /// The file it is in.
    file: String,
    /// The Rust module it is written into: `""` for the program's own.
    module: String,
    /// Whether it is a `singleton`: properties and functions, no item.
    singleton: bool,
    /// Its `property alias`es: name and the path they stand for, until
    /// they are resolved into `props`.
    aliases: Vec<(String, Vec<Ident>)>,
    /// Its `default property alias`: the field's name and the id where a
    /// use's children go.
    content: Option<(String, Ident)>,
    /// Its functions' parameters' defaults.
    defaults: BTreeMap<String, Vec<Option<Expr>>>,
    /// Its root element.
    root: Element,
    /// Its base: what its root is.
    base: ElemTy,
    /// Its declared properties, in order.
    props: Vec<(String, Ty)>,
    /// Its functions: parameters and result.
    functions: BTreeMap<String, (Vec<(String, Ty)>, Ty)>,
    /// Its signals and their parameters, in order.
    signals: Vec<(String, Vec<(String, Ty)>)>,
}

/// The whole program.
pub(crate) struct Program {
    /// Declared structs: their fields.
    structs: BTreeMap<String, Vec<(String, Ty)>>,
    /// Declared enums: their variants.
    enums: BTreeMap<String, Vec<String>>,
    /// Components, by name.
    components: BTreeMap<String, Component>,
}

/// An item an expression can name, and how to reach it.
#[derive(Clone, Debug)]
enum Target {
    /// A handle held in a local of the generated code.
    Static(String, ElemTy),
    /// The parent of the component's root, known only when it runs: only
    /// its common properties.
    Dynamic(String),
}

/// What names mean where an expression is.
struct Scope<'a> {
    /// The component.
    component: &'a Component,
    /// The variable its value is in: `this` in `new`, `self` in a function.
    this: &'a str,
    /// Its ids.
    ids: &'a HashMap<String, (String, ElemTy)>,
    /// The element the expression is on.
    current: Option<(String, ElemTy)>,
    /// That element's parent.
    parent: Option<Target>,
    /// A function's parameters.
    params: &'a [(String, Ty)],
    /// Names a delegate gives (`modelData`, `index`): the name, the Rust
    /// that reads it, and its type.
    locals: &'a [(String, String, Ty)],
    /// How deep in delegates: names the factories' locals apart.
    depth: usize,
    /// Code that runs once every element is made and given its members:
    /// states first, then animations.
    deferred: &'a RefCell<(String, String)>,
    /// The file, for diagnostics.
    file: &'a str,
}

/// An expression written out: its code, its type, and whether it reads
/// anything (a constant is set once rather than bound).
struct Code {
    /// Rust.
    text: String,
    /// Its type.
    ty: Ty,
    /// Whether it reads a property or calls a function.
    reads: bool,
}

impl Program {
    /// Every file's declarations gathered and checked for clashes.
    pub(crate) fn gather(files: &[(String, File, String)]) -> Result<Self, Diagnostic> {
        let mut program = Program {
            structs: BTreeMap::new(),
            enums: BTreeMap::new(),
            components: BTreeMap::new(),
        };
        let mut seen: HashSet<String> = HashSet::new();
        let taken = |name: &Ident, file: &str, seen: &mut HashSet<String>| {
            if Kind::named(&name.text).is_some() || !seen.insert(name.text.clone()) {
                Err(diag(
                    file,
                    name.pos,
                    &format!("`{}` is declared twice, or is a kind of item", name.text),
                ))
            } else {
                Ok(())
            }
        };
        // Names first, so a struct's field may be an enum declared later.
        for (file, parsed, _) in files {
            for decl in &parsed.decls {
                match decl {
                    Decl::Enum { name, variants } => {
                        taken(name, file, &mut seen)?;
                        if variants.is_empty() {
                            return Err(diag(file, name.pos, "an enum needs a variant"));
                        }
                        let _ = program.enums.insert(
                            name.text.clone(),
                            variants.iter().map(|v| v.text.clone()).collect(),
                        );
                    }
                    Decl::Struct { name, .. } => {
                        taken(name, file, &mut seen)?;
                        let _ = program.structs.insert(name.text.clone(), Vec::new());
                    }
                    Decl::Component { name, .. } | Decl::Singleton { name, .. } => {
                        taken(name, file, &mut seen)?;
                    }
                }
            }
        }
        for (file, parsed, _) in files {
            for decl in &parsed.decls {
                if let Decl::Struct { name, fields } = decl {
                    let mut typed = Vec::new();
                    for (field, ty) in fields {
                        let ty = program.ty(file, ty)?;
                        if ty == Ty::Struct(name.text.clone()) {
                            return Err(diag(file, field.pos, "a struct cannot hold itself"));
                        }
                        typed.push((field.text.clone(), ty));
                    }
                    let _ = program.structs.insert(name.text.clone(), typed);
                }
            }
        }
        for (file, parsed, module) in files {
            for decl in &parsed.decls {
                let (name, root, singleton) = match decl {
                    Decl::Component { name, root } => (name, root, false),
                    Decl::Singleton { name, root } => (name, root, true),
                    _ => continue,
                };
                {
                    let mut props = Vec::new();
                    let mut functions = BTreeMap::new();
                    let mut defaults = BTreeMap::new();
                    let mut signals = Vec::new();
                    let mut aliases = Vec::new();
                    let mut content = None;
                    for member in &root.members {
                        if singleton
                            && !matches!(member, Member::Property { .. } | Member::Function { .. })
                        {
                            return Err(diag(
                                file,
                                member_pos(member).unwrap_or(name.pos),
                                "a singleton holds properties and functions",
                            ));
                        }
                        match member {
                            Member::Property {
                                name,
                                ty,
                                init,
                                default,
                            } if ty.text == "alias" || *default => {
                                let target = match init {
                                    Some(Expr::Path(path)) if ty.text == "alias" => path.clone(),
                                    _ => {
                                        return Err(diag(
                                            file,
                                            name.pos,
                                            "an alias names what it stands for: `property alias text: label.text`, `default property alias content: box`",
                                        ));
                                    }
                                };
                                if singleton {
                                    return Err(diag(file, name.pos, "a singleton has no aliases"));
                                }
                                if *default {
                                    let [id] = target.as_slice() else {
                                        return Err(diag(
                                            file,
                                            name.pos,
                                            "a default alias names the id of the item a use's children go in",
                                        ));
                                    };
                                    if content.is_some() {
                                        return Err(diag(file, name.pos, "one default alias"));
                                    }
                                    content = Some((name.text.clone(), id.clone()));
                                } else {
                                    if target.len() < 2 {
                                        return Err(diag(
                                            file,
                                            name.pos,
                                            "an alias names an item's property: `label.text`",
                                        ));
                                    }
                                    aliases.push((name.text.clone(), target));
                                }
                            }
                            Member::Signal { name, params } => {
                                let mut typed = Vec::new();
                                for (param, ty) in params {
                                    typed.push((param.text.clone(), program.ty(file, ty)?));
                                }
                                signals.push((name.text.clone(), typed));
                            }
                            Member::Property { name, ty, .. } => {
                                if props.iter().any(|(n, _): &(String, Ty)| *n == name.text) {
                                    return Err(diag(
                                        file,
                                        name.pos,
                                        &format!("`{}` is declared twice", name.text),
                                    ));
                                }
                                props.push((name.text.clone(), program.ty(file, ty)?));
                            }
                            Member::Function {
                                name,
                                params,
                                defaults: given,
                                ret,
                                ..
                            } => {
                                let mut typed = Vec::new();
                                for (param, ty) in params {
                                    typed.push((param.text.clone(), program.ty(file, ty)?));
                                }
                                let _ = functions
                                    .insert(name.text.clone(), (typed, program.ty(file, ret)?));
                                let _ = defaults.insert(name.text.clone(), given.clone());
                            }
                            _ => {}
                        }
                    }
                    let base = match kind_of(root) {
                        Some(kind) => ElemTy::Kind(kind),
                        None if singleton => ElemTy::Kind(Kind::Item),
                        None => ElemTy::Component(root.kind.text.clone()),
                    };
                    let component = Component {
                        file: file.clone(),
                        module: module.clone(),
                        singleton,
                        aliases,
                        content,
                        defaults,
                        root: root.clone(),
                        base,
                        props,
                        functions,
                        signals,
                    };
                    let _ = program.components.insert(name.text.clone(), component);
                }
            }
        }
        program.no_cycles()?;
        program.resolve_aliases()?;
        Ok(program)
    }

    /// Give each `property alias` the type of what it stands for. An alias
    /// may stand for another component's alias, so this goes round until
    /// nothing more is learned.
    fn resolve_aliases(&mut self) -> Result<(), Diagnostic> {
        loop {
            let mut learned = Vec::new();
            for (name, c) in &self.components {
                for (alias, target) in &c.aliases {
                    if c.props.iter().any(|(p, _)| p == alias) {
                        continue;
                    }
                    if let Some((_, ty)) = self.alias_target(name, c, target) {
                        learned.push((name.clone(), alias.clone(), ty));
                    }
                }
            }
            if learned.is_empty() {
                break;
            }
            for (name, alias, ty) in learned {
                if let Some(c) = self.components.get_mut(&name) {
                    c.props.push((alias, ty));
                }
            }
        }
        for c in self.components.values() {
            for (alias, target) in &c.aliases {
                if !c.props.iter().any(|(p, _)| p == alias) {
                    let at = target.first().map_or(Pos { line: 0, column: 0 }, |t| t.pos);
                    return Err(diag(
                        &c.file,
                        at,
                        &format!(
                            "`{}` names no property this alias can stand for",
                            target
                                .iter()
                                .map(|t| t.text.as_str())
                                .collect::<Vec<_>>()
                                .join(".")
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// What `target` (`id.prop`) names in component `name`: the element's
    /// type and the property's Rust path from its handle, and its type.
    fn alias_target(
        &self,
        name: &str,
        c: &Component,
        target: &[Ident],
    ) -> Option<((ElemTy, String), Ty)> {
        let (id, rest) = target.split_first()?;
        let ty = find_id(&c.root, &id.text, true).map(|element| {
            if std::ptr::eq(element, &c.root) {
                ElemTy::Component(name.to_owned())
            } else {
                match kind_of(element) {
                    Some(kind) => ElemTy::Kind(kind),
                    None => ElemTy::Component(element.kind.text.clone()),
                }
            }
        })?;
        let names: Vec<&str> = rest.iter().map(|p| p.text.as_str()).collect();
        // The root's own: its base's, since its aliases are what is being
        // worked out.
        let (rust, pty) = match &ty {
            ElemTy::Component(own) if own == name => self
                .property(&c.base, &names)
                .map(|(rust, pty)| (format!(".base{rust}"), pty))?,
            other => self.property(other, &names)?,
        };
        Some(((ty, rust), pty))
    }

    /// Whether a use of an element of `ty` puts its children somewhere
    /// other than its root item: a component with a default alias, or one
    /// based on such a component.
    fn has_content(&self, ty: &ElemTy) -> bool {
        match ty {
            ElemTy::Kind(_) => false,
            ElemTy::Component(name) => self
                .components
                .get(name)
                .is_some_and(|c| c.content.is_some() || self.has_content(&c.base)),
        }
    }

    /// A type a declaration names.
    fn ty(&self, file: &str, name: &Ident) -> Result<Ty, Diagnostic> {
        Ty::named(&name.text, &self.structs, &self.enums)
            .ok_or_else(|| diag(file, name.pos, &format!("`{}` is not a type", name.text)))
    }

    /// No component makes itself, directly or through others.
    fn no_cycles(&self) -> Result<(), Diagnostic> {
        fn uses(element: &Element, out: &mut Vec<(String, Pos)>) {
            out.push((element.kind.text.clone(), element.kind.pos));
            for member in &element.members {
                if let Member::Child(child) = member {
                    uses(child, out);
                }
            }
        }
        for name in self.components.keys() {
            let mut stack: Vec<String> = vec![name.clone()];
            let mut seen = HashSet::new();
            while let Some(next) = stack.pop() {
                let Some(c) = self.components.get(&next) else {
                    continue;
                };
                let mut used = Vec::new();
                uses(&c.root, &mut used);
                for (kind, pos) in used {
                    if kind == *name {
                        return Err(diag(
                            &c.file,
                            pos,
                            &format!("`{name}` makes itself, through `{next}`"),
                        ));
                    }
                    if seen.insert(kind.clone()) {
                        stack.push(kind);
                    }
                }
            }
        }
        Ok(())
    }

    /// The whole program as Rust.
    pub(crate) fn emit(&self) -> Result<String, Diagnostic> {
        let mut out = String::from(
            "// Written by tole-compiler from the program's .tole files. Not to be\n// edited: it is written again at each build.\n\n",
        );
        // The program's own declarations at the top, each imported
        // module's in a Rust module of its own, which sees the top's
        // names (its own keys are `module::Name`).
        let mut modules: Vec<&str> = vec![""];
        for c in self.components.values() {
            if !modules.contains(&c.module.as_str()) {
                modules.push(&c.module);
            }
        }
        for module in modules {
            let mut body = String::new();
            let mine = |key: &str| module_of(key) == module;
            for (key, variants) in self.enums.iter().filter(|(k, _)| mine(k)) {
                let name = short(key);
                let _ = writeln!(
                    body,
                    "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]\npub enum {name} {{"
                );
                for (index, variant) in variants.iter().enumerate() {
                    let default = if index == 0 { "#[default] " } else { "" };
                    let _ = writeln!(body, "    {default}{variant},");
                }
                body.push_str("}\n\n");
                body.push_str(&prop_type_impl(name));
            }
            for (key, fields) in self.structs.iter().filter(|(k, _)| mine(k)) {
                let name = short(key);
                let _ = writeln!(
                    body,
                    "#[derive(Clone, Debug, Default, PartialEq)]\npub struct {name} {{"
                );
                for (field, ty) in fields {
                    let _ = writeln!(body, "    pub {}: {},", snake(field), ty.rust());
                }
                body.push_str("}\n\n");
                body.push_str(&prop_type_impl(name));
            }
            for (key, component) in self.components.iter().filter(|(k, _)| mine(k)) {
                if component.singleton {
                    body.push_str(&self.singleton(key, component)?);
                } else {
                    body.push_str(&self.component(key, component)?);
                }
            }
            if module.is_empty() {
                out.push_str(&body);
            } else {
                let _ = write!(
                    out,
                    "/// The framework's module of the same name, as the program's files\n/// import it.\npub mod {module} {{\nuse super::*;\n\n{body}}}\n\n"
                );
            }
        }
        Ok(out)
    }

    /// A singleton: a struct of its properties, made once per `Ui` and
    /// reached by `instance`, and its functions.
    fn singleton(&self, key: &str, c: &Component) -> Result<String, Diagnostic> {
        let file = c.file.as_str();
        let name = short(key);
        let mut out = String::new();
        let _ = writeln!(
            out,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct {name} {{"
        );
        for (prop, ty) in &c.props {
            let _ = writeln!(out, "    pub {}: ::tole::Prop<{}>,", snake(prop), ty.rust());
        }
        out.push_str("}\n\n");
        let _ = writeln!(
            out,
            "impl {name} {{\n    /// The one `{name}` of `ui`, made the first time it is asked for.\n    pub fn instance(ui: &mut ::tole::Ui) -> Self {{\n        ui.global(Self::make)\n    }}\n    fn make(ui: &mut ::tole::Ui) -> Self {{"
        );
        let _ = writeln!(out, "        let this = {name} {{");
        for (prop, ty) in &c.props {
            let _ = writeln!(
                out,
                "            {}: ui.prop(<{} as ::tole::PropType>::fallback()),",
                snake(prop),
                ty.rust()
            );
        }
        out.push_str("        };\n");
        let ids = HashMap::new();
        let deferred = RefCell::new((String::new(), String::new()));
        let scope = Scope {
            component: c,
            this: "this",
            ids: &ids,
            current: None,
            parent: None,
            params: &[],
            locals: &[],
            depth: 0,
            deferred: &deferred,
            file,
        };
        out.push_str(&self.members(&scope, "this", &ElemTy::Component(key.to_owned()), &c.root)?);
        out.push_str("        this\n    }\n");
        out.push_str(&self.functions(key, c, &ids)?);
        out.push_str("}\n\n");
        Ok(out)
    }

    /// A component's functions, as methods.
    fn functions(
        &self,
        key: &str,
        c: &Component,
        ids: &HashMap<String, (String, ElemTy)>,
    ) -> Result<String, Diagnostic> {
        let file = c.file.as_str();
        let mut out = String::new();
        let no_deferred = RefCell::new((String::new(), String::new()));
        for member in &c.root.members {
            if let Member::Function {
                name: fname, body, ..
            } = member
            {
                let (typed, ret) = self
                    .components
                    .get(key)
                    .and_then(|c| c.functions.get(&fname.text))
                    .cloned()
                    .unwrap_or_default_fn();
                let scope = Scope {
                    component: c,
                    this: "self",
                    ids,
                    current: None,
                    parent: None,
                    params: &typed,
                    locals: &[],
                    depth: 0,
                    deferred: &no_deferred,
                    file,
                };
                let code = self.expr(&scope, body)?;
                let code = coerce(file, body.pos(), code, &ret)?;
                let args: String = typed
                    .iter()
                    .map(|(p, t)| format!(", {}: {}", snake(p), t.rust()))
                    .collect();
                let _ = writeln!(
                    out,
                    "    pub fn {}(&self, ui: &mut ::tole::Ui{args}) -> {} {{\n        {}\n    }}",
                    snake(&fname.text),
                    ret.rust(),
                    code
                );
            }
        }
        Ok(out)
    }

    /// The Rust type of an element's handle.
    fn handle(ty: &ElemTy) -> String {
        match ty {
            ElemTy::Kind(kind) => kind.handle().to_owned(),
            ElemTy::Component(name) => name.clone(),
        }
    }

    /// One component: its struct, `new`, functions and `Element`.
    fn component(&self, key: &str, c: &Component) -> Result<String, Diagnostic> {
        let file = c.file.as_str();
        let name = short(key);
        if let ElemTy::Component(base) = &c.base
            && !self.components.contains_key(base)
        {
            return Err(diag(
                file,
                c.root.kind.pos,
                &format!("`{base}` is neither a kind of item nor a component"),
            ));
        }
        let mut out = String::new();
        let _ = writeln!(
            out,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct {name} {{"
        );
        let _ = writeln!(out, "    pub base: {},", Self::handle(&c.base));
        for (prop, ty) in &c.props {
            let _ = writeln!(out, "    pub {}: ::tole::Prop<{}>,", snake(prop), ty.rust());
        }
        for (signal, _) in &c.signals {
            let _ = writeln!(out, "    pub {}: ::tole::SignalId,", snake(signal));
        }
        if let Some((field, _)) = &c.content {
            let _ = writeln!(
                out,
                "    /// Where a use's children go.\n    pub {}: ::tole::ItemId,",
                snake(field)
            );
        }
        out.push_str("}\n\n");

        // Every element in tree order, with its local and its parent's.
        let mut elements: Vec<Walked<'_>> = Vec::new();
        let mut ids: HashMap<String, (String, ElemTy)> = HashMap::new();
        self.walk(
            file,
            &c.root,
            "this".to_owned(),
            ElemTy::Component(key.to_owned()),
            (Some(Target::Dynamic("this".to_owned())), None),
            &mut elements,
            &mut ids,
        )?;

        let _ = writeln!(out, "impl {name} {{");
        let _ = writeln!(
            out,
            "    pub fn new(ui: &mut ::tole::Ui, parent: ::core::option::Option<::tole::ItemId>) -> ::core::option::Option<Self> {{"
        );
        let _ = writeln!(
            out,
            "        let base = {}::new(ui, parent)?;",
            Self::handle(&c.base)
        );
        // The items, then the component's own value, whose aliases are
        // its items' properties.
        for (var, ty, _, _, under) in elements.iter().skip(1) {
            let under = match under.as_deref() {
                Some("this") | None => "base",
                Some(other) => other,
            };
            let _ = writeln!(
                out,
                "        let {var} = {}::new(ui, ::core::option::Option::Some(::tole::Element::content(&{under})))?;",
                Self::handle(ty)
            );
        }
        let _ = writeln!(out, "        let this = {name} {{\n            base,");
        for (prop, ty) in &c.props {
            if let Some((_, target)) = c.aliases.iter().find(|(a, _)| a == prop) {
                let Some(((_, rust), _)) = self.alias_target(key, c, target) else {
                    continue;
                };
                let var = target
                    .first()
                    .and_then(|id| ids.get(&id.text))
                    .map_or("this", |(v, _)| v.as_str());
                let var = if var == "this" { "base" } else { var };
                let rust = if var == "base" {
                    rust.strip_prefix(".base").unwrap_or(&rust).to_owned()
                } else {
                    rust
                };
                let _ = writeln!(out, "            {}: {var}{rust},", snake(prop));
                continue;
            }
            let _ = writeln!(
                out,
                "            {}: ui.own_prop(::tole::Element::id(&base), <{} as ::tole::PropType>::fallback()),",
                snake(prop),
                ty.rust()
            );
        }
        for (signal, _) in &c.signals {
            let _ = writeln!(
                out,
                "            {}: ui.own_signal(::tole::Element::id(&base)),",
                snake(signal)
            );
        }
        if let Some((field, id)) = &c.content {
            let Some((var, _)) = ids.get(&id.text) else {
                return Err(diag(
                    file,
                    id.pos,
                    &format!("`{}` is not an id here", id.text),
                ));
            };
            let var = if var == "this" { "base" } else { var.as_str() };
            let _ = writeln!(
                out,
                "            {}: ::tole::Element::content(&{var}),",
                snake(field)
            );
        }
        out.push_str("        };\n");
        let deferred = RefCell::new((String::new(), String::new()));
        for (var, ty, element, parent, _) in &elements {
            let scope = Scope {
                component: c,
                this: "this",
                ids: &ids,
                current: Some((var.clone(), ty.clone())),
                parent: parent.clone(),
                params: &[],
                locals: &[],
                depth: 0,
                deferred: &deferred,
                file,
            };
            out.push_str(&self.members(&scope, var, ty, element)?);
        }
        let (states, animations) = deferred.into_inner();
        out.push_str(&states);
        out.push_str(&animations);
        out.push_str("        ui.update();\n        ::core::option::Option::Some(this)\n    }\n");
        out.push_str(&self.functions(key, c, &ids)?);
        if !c.signals.is_empty() {
            let _ = writeln!(
                out,
                "    /// Which of this component's signals `emitted` is, with its arguments.\n    pub fn signal(&self, emitted: &::tole::Emitted) -> ::core::option::Option<{name}Signal> {{"
            );
            for (signal, params) in &c.signals {
                let decode: Vec<String> = params
                    .iter()
                    .enumerate()
                    .map(|(index, (_, ty))| {
                        format!(
                            "<{} as ::tole::PropType>::from_value(emitted.args.get({index})?)?",
                            ty.rust()
                        )
                    })
                    .collect();
                let fields = if decode.is_empty() {
                    String::new()
                } else {
                    format!("({})", decode.join(", "))
                };
                let _ = writeln!(
                    out,
                    "        if emitted.signal == self.{} {{\n            return ::core::option::Option::Some({name}Signal::{}{fields});\n        }}",
                    snake(signal),
                    upper(signal)
                );
            }
            out.push_str("        ::core::option::Option::None\n    }\n");
        }
        out.push_str("}\n\n");
        if !c.signals.is_empty() {
            let _ = writeln!(
                out,
                "#[derive(Clone, Debug, PartialEq)]\npub enum {name}Signal {{"
            );
            for (signal, params) in &c.signals {
                let types: Vec<String> = params.iter().map(|(_, ty)| ty.rust()).collect();
                let fields = if types.is_empty() {
                    String::new()
                } else {
                    format!("({})", types.join(", "))
                };
                let _ = writeln!(out, "    {}{fields},", upper(signal));
            }
            out.push_str("}\n\n");
        }
        let content = match &c.content {
            Some((field, _)) => format!("self.{}", snake(field)),
            None => "::tole::Element::content(&self.base)".to_owned(),
        };
        let _ = writeln!(
            out,
            "impl ::tole::Element for {name} {{\n    fn id(&self) -> ::tole::ItemId {{\n        ::tole::Element::id(&self.base)\n    }}\n    fn common(&self) -> ::tole::Common {{\n        ::tole::Element::common(&self.base)\n    }}\n    fn content(&self) -> ::tole::ItemId {{\n        {content}\n    }}\n}}\n"
        );
        Ok(out)
    }

    /// Gather `element` and its children: a local each, their ids.
    #[allow(clippy::too_many_arguments, reason = "the walk's state, passed down")]
    fn walk<'e>(
        &self,
        file: &str,
        element: &'e Element,
        var: String,
        ty: ElemTy,
        (parent, under): (Option<Target>, Option<String>),
        out: &mut Vec<Walked<'e>>,
        ids: &mut HashMap<String, (String, ElemTy)>,
    ) -> Result<(), Diagnostic> {
        for member in &element.members {
            if let Member::Id(id) = member
                && ids
                    .insert(id.text.clone(), (var.clone(), ty.clone()))
                    .is_some()
            {
                return Err(diag(
                    file,
                    id.pos,
                    &format!("the id `{}` is given twice", id.text),
                ));
            }
        }
        out.push((var.clone(), ty.clone(), element, parent, under));
        if matches!(
            ty,
            ElemTy::Kind(Kind::Repeater | Kind::ListView | Kind::Shape)
        ) || kind_of(element) == Some(Kind::Shape)
        {
            // Its child is its delegate, made by a factory of its own; a
            // Shape's are its paths, values rather than items.
            return Ok(());
        }
        let prefix: String = match var.rfind('_') {
            Some(at) => var.get(..=at).unwrap_or("e").to_owned(),
            None => "e".to_owned(),
        };
        for member in &element.members {
            if let Member::Child(child) = member {
                let child_ty = match kind_of(child) {
                    Some(kind) => ElemTy::Kind(kind),
                    None if self.components.contains_key(&child.kind.text) => {
                        ElemTy::Component(child.kind.text.clone())
                    }
                    None => {
                        return Err(diag(
                            file,
                            child.kind.pos,
                            &format!(
                                "`{}` is neither a kind of item nor a component",
                                child.kind.text
                            ),
                        ));
                    }
                };
                for inner in &child.members {
                    if let Member::Property { name, .. }
                    | Member::Function { name, .. }
                    | Member::Signal { name, .. } = inner
                    {
                        return Err(diag(
                            file,
                            name.pos,
                            "properties, functions and signals are declared on a component's root",
                        ));
                    }
                }
                let child_var = format!("{prefix}{}", out.len());
                // Under a component that puts a use's children elsewhere,
                // `parent` is wherever that is, known when it runs.
                let parent = if self.has_content(&ty) {
                    Target::Dynamic(child_var.clone())
                } else {
                    Target::Static(var.clone(), ty.clone())
                };
                self.walk(
                    file,
                    child,
                    child_var,
                    child_ty,
                    (Some(parent), Some(var.clone())),
                    out,
                    ids,
                )?;
            }
        }
        Ok(())
    }

    /// The property `path` names on an element of type `ty`: the Rust
    /// path from its handle, and the type.
    fn property(&self, ty: &ElemTy, path: &[&str]) -> Option<(String, Ty)> {
        match ty {
            ElemTy::Kind(kind) => kind.property(path).map(|(rust, ty)| (rust.to_owned(), ty)),
            ElemTy::Component(name) => {
                let c = self.components.get(name)?;
                if let [one] = path
                    && let Some((_, ty)) = c.props.iter().find(|(p, _)| p == one)
                {
                    return Some((format!(".{}", snake(one)), ty.clone()));
                }
                if c.singleton {
                    return None;
                }
                self.property(&c.base, path)
                    .map(|(rust, ty)| (format!(".base{rust}"), ty))
            }
        }
    }

    /// The signal `name` on an element of type `ty`: the Rust path from
    /// its handle, and its parameters.
    fn signal(&self, ty: &ElemTy, name: &str) -> Option<(String, Vec<(String, Ty)>)> {
        match ty {
            ElemTy::Kind(kind) => kind.signal(name).map(|(rust, params)| {
                (
                    rust.to_owned(),
                    params
                        .iter()
                        .map(|(p, t)| ((*p).to_owned(), t.clone()))
                        .collect(),
                )
            }),
            ElemTy::Component(component) => {
                let c = self.components.get(component)?;
                if let Some((_, params)) = c.signals.iter().find(|(s, _)| s == name) {
                    return Some((format!(".{}", snake(name)), params.clone()));
                }
                self.signal(&c.base, name)
                    .map(|(rust, params)| (format!(".base{rust}"), params))
            }
        }
    }

    /// `onName: ...` on the element in `var`: a closure connected to the
    /// signal, its arguments as locals by their parameters' names.
    fn handler(
        &self,
        scope: &Scope<'_>,
        var: &str,
        ty: &ElemTy,
        attached: Option<&Ident>,
        signal: &Ident,
        body: &[Stmt],
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        if let Some(attached) = attached {
            return self.keys_handler(scope, var, attached, signal, body);
        }
        let (rust, params) = self.signal(ty, &signal.text).ok_or_else(|| {
            diag(
                file,
                signal.pos,
                &format!("{} has no signal `{}`", describe(ty), signal.text),
            )
        })?;
        let mut locals = String::new();
        for (index, (param, pty)) in params.iter().enumerate() {
            let _ = write!(
                locals,
                "let {}: {} = args.get({index}).and_then(<{} as ::tole::PropType>::from_value).unwrap_or_else(<{} as ::tole::PropType>::fallback); ",
                snake(param),
                pty.rust(),
                pty.rust(),
                pty.rust()
            );
        }
        let inner = Scope {
            component: scope.component,
            this: scope.this,
            ids: scope.ids,
            current: scope.current.clone(),
            parent: scope.parent.clone(),
            params: &params,
            locals: scope.locals,
            depth: scope.depth,
            deferred: scope.deferred,
            file,
        };
        if let Some(own) = body.iter().find_map(|statement| match statement {
            Stmt::Call(Expr::Call(path, _, pos)) => self
                .emitted(&inner, path)
                .filter(|(emits, _)| *emits == format!("{var}{rust}"))
                .map(|_| *pos),
            _ => None,
        }) {
            return Err(diag(
                file,
                own,
                &format!(
                    "this handler emits the `{}` it handles, again and again: name the component's root by its id (`root.{}()`)",
                    signal.text, signal.text
                ),
            ));
        }
        let statements = self.statements(&inner, body)?;
        Ok(format!(
            "        ui.connect({var}{rust}, ::std::rc::Rc::new(move |ui: &mut ::tole::Ui, args: &[::tole::Value]| {{ let _ = args; {locals}{statements}}}));\n"
        ))
    }

    /// `Keys.onPressed: ...` and the rest on the element in `var`: the
    /// signal made on first use, `event` its argument.
    fn keys_handler(
        &self,
        scope: &Scope<'_>,
        var: &str,
        attached: &Ident,
        signal: &Ident,
        body: &[Stmt],
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        if attached.text != "Keys" {
            return Err(diag(
                file,
                attached.pos,
                &format!("`{}` has no signals: `Keys` has", attached.text),
            ));
        }
        let id = format!("::tole::Element::id(&{var})");
        let make = match signal.text.as_str() {
            "pressed" => format!("ui.keys_pressed({id})"),
            "released" => format!("ui.keys_released({id})"),
            other => match keys_handler_keysym(other) {
                Some(keysym) => format!("ui.keys_key_pressed({id}, {keysym:?})"),
                None => {
                    return Err(diag(
                        file,
                        signal.pos,
                        &format!("`Keys` has no signal `{other}`"),
                    ));
                }
            },
        };
        let params = [("event".to_owned(), Ty::KeyEvent)];
        let inner = Scope {
            component: scope.component,
            this: scope.this,
            ids: scope.ids,
            current: scope.current.clone(),
            parent: scope.parent.clone(),
            params: &params,
            locals: scope.locals,
            depth: scope.depth,
            deferred: scope.deferred,
            file,
        };
        let statements = self.statements(&inner, body)?;
        Ok(format!(
            "        if let ::core::option::Option::Some(signal) = {make} {{ ui.connect(signal, ::std::rc::Rc::new(move |ui: &mut ::tole::Ui, args: &[::tole::Value]| {{ let event: ::tole::KeyEvent = args.first().and_then(<::tole::KeyEvent as ::tole::PropType>::from_value).unwrap_or_default(); let _ = &event; {statements}}})); }}\n"
        ))
    }

    /// `KeyNavigation.tab: other` on the element in `var`.
    fn key_navigation(
        &self,
        scope: &Scope<'_>,
        var: &str,
        path: &[Ident],
        expr: &Expr,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let at = path.first().map_or(expr.pos(), |p| p.pos);
        let nav = match path {
            [_, which] => match which.text.as_str() {
                "tab" => "Tab",
                "backtab" => "Backtab",
                "up" => "Up",
                "down" => "Down",
                "left" => "Left",
                "right" => "Right",
                _ => {
                    return Err(diag(
                        file,
                        which.pos,
                        "`KeyNavigation` has `tab`, `backtab`, `up`, `down`, `left` and `right`",
                    ));
                }
            },
            _ => return Err(diag(file, at, "`KeyNavigation.tab: other`")),
        };
        let target = match expr {
            Expr::Path(names) => match names.as_slice() {
                [one] => scope.ids.get(&one.text).map(|(v, _)| v.clone()),
                _ => None,
            },
            _ => None,
        }
        .ok_or_else(|| diag(file, expr.pos(), "`KeyNavigation` names an id"))?;
        Ok(format!(
            "        ui.set_key_navigation(::tole::Element::id(&{var}), ::tole::Nav::{nav}, ::tole::Element::id(&{target}));\n"
        ))
    }

    /// An item's method a call names: `forceActiveFocus()` or a text
    /// field's `selectAll()`, on the element, or an id or `parent` before
    /// it. Answers the item's local and the `Ui`'s method.
    fn focus_call(&self, scope: &Scope<'_>, path: &[Ident]) -> Option<(String, &'static str)> {
        let method = |name: &str| match name {
            "forceActiveFocus" => Some("force_active_focus"),
            "selectAll" => Some("select_all"),
            "open" => Some("open_popup"),
            "close" => Some("close_popup"),
            _ => None,
        };
        match path {
            [call] => {
                let method = method(&call.text)?;
                scope.current.as_ref().map(|(v, _)| (v.clone(), method))
            }
            [item, call] => {
                let method = method(&call.text)?;
                let var = if item.text == "parent" {
                    match &scope.parent {
                        Some(Target::Static(v, _)) => Some(v.clone()),
                        _ => None,
                    }
                } else {
                    scope.ids.get(&item.text).map(|(v, _)| v.clone())
                };
                var.map(|v| (v, method))
            }
            _ => None,
        }
    }

    /// A handler's statements.
    fn statements(&self, scope: &Scope<'_>, body: &[Stmt]) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut out = String::new();
        for statement in body {
            match statement {
                Stmt::Assign(path, expr)
                    if matches!(path.as_slice(), [event, field]
                        if field.text == "accepted"
                        && scope.params.iter().any(|(p, t)| *p == event.text && *t == Ty::KeyEvent)) =>
                {
                    let code = self.expr(scope, expr)?;
                    let text = coerce(file, expr.pos(), code, &Ty::Bool)?;
                    let _ = write!(out, "ui.accept_key({text}); ");
                }
                Stmt::Assign(path, expr) => {
                    let (prop, want) = self.lvalue(scope, path)?;
                    if let Some(item) = prop.strip_suffix(".common.focus") {
                        let code = self.expr(scope, expr)?;
                        let text = coerce(file, expr.pos(), code, &Ty::Bool)?;
                        let item = item.strip_suffix(".base").unwrap_or(item);
                        let _ = write!(
                            out,
                            "{{ let on = {text}; ui.set_focus(::tole::Element::id(&{item}), on); }} "
                        );
                        continue;
                    }
                    let code = self.expr_want(scope, expr, Some(&want))?;
                    let text = coerce(file, expr.pos(), code, &want)?;
                    let _ = write!(
                        out,
                        "{{ let value = {text}; ::tole::Prop::set({prop}, ui, value); }} "
                    );
                }
                Stmt::Call(Expr::Call(path, args, pos)) => {
                    if let Some((item, method)) = self.focus_call(scope, path) {
                        if !args.is_empty() {
                            return Err(diag(
                                file,
                                *pos,
                                "`forceActiveFocus`, `selectAll`, `open` and `close` take no arguments",
                            ));
                        }
                        let _ = write!(out, "ui.{method}(::tole::Element::id(&{item})); ");
                    } else if let Some((signal, params)) = self.emitted(scope, path) {
                        if params.len() != args.len() {
                            return Err(diag(
                                file,
                                *pos,
                                &format!("this signal takes {} arguments", params.len()),
                            ));
                        }
                        let mut values = Vec::new();
                        for (arg, (_, want)) in args.iter().zip(&params) {
                            let code = self.expr_want(scope, arg, Some(want))?;
                            let text = coerce(file, arg.pos(), code, want)?;
                            values.push(format!(
                                "<{} as ::tole::PropType>::into_value({text})",
                                want.rust()
                            ));
                        }
                        let _ = write!(
                            out,
                            "{{ let args = ::std::vec![{}]; ui.emit({signal}, args); }} ",
                            values.join(", ")
                        );
                    } else {
                        let code = self.call(scope, path, args, *pos)?;
                        let _ = write!(out, "let _ = {}; ", code.text);
                    }
                }
                Stmt::Call(other) => {
                    return Err(diag(
                        file,
                        other.pos(),
                        "a statement is an assignment, a call or an `if`",
                    ));
                }
                Stmt::If(condition, yes, no) => {
                    let code = self.expr(scope, condition)?;
                    if code.ty != Ty::Bool {
                        return Err(diag(file, condition.pos(), "a condition is a bool"));
                    }
                    let _ = write!(
                        out,
                        "if {} {{ {}}} else {{ {}}} ",
                        code.text,
                        self.statements(scope, yes)?,
                        self.statements(scope, no)?
                    );
                }
            }
        }
        Ok(out)
    }

    /// The signal a call statement emits, if it names one: `name()` on the
    /// element or the component, or `id.name()`, `parent.name()`.
    fn emitted(&self, scope: &Scope<'_>, path: &[Ident]) -> Option<(String, Vec<(String, Ty)>)> {
        match path {
            [name] => {
                let root = (
                    scope.this.to_owned(),
                    ElemTy::Component(component_name(self, scope.component)),
                );
                for (var, ty) in [scope.current.clone(), Some(root)].into_iter().flatten() {
                    if let Some((rust, params)) = self.signal(&ty, &name.text) {
                        return Some((format!("{var}{rust}"), params));
                    }
                }
                None
            }
            [item, name] => {
                let (var, ty) = if item.text == "parent" {
                    match &scope.parent {
                        Some(Target::Static(var, ty)) => (var.clone(), ty.clone()),
                        _ => return None,
                    }
                } else {
                    scope.ids.get(&item.text).cloned()?
                };
                self.signal(&ty, &name.text)
                    .map(|(rust, params)| (format!("{var}{rust}"), params))
            }
            _ => None,
        }
    }

    /// The property an assignment sets: the Rust path to its `Prop`, and
    /// its type.
    fn lvalue(&self, scope: &Scope<'_>, path: &[Ident]) -> Result<(String, Ty), Diagnostic> {
        let file = scope.file;
        let Some(first) = path.first() else {
            return Err(diag(file, Pos { line: 0, column: 0 }, "an empty name"));
        };
        let names: Vec<&str> = path.iter().map(|p| p.text.as_str()).collect();
        let rest = names.get(1..).unwrap_or(&[]);
        if let Some(c) = self.components.get(&first.text).filter(|c| c.singleton) {
            return match rest {
                [prop] => c
                    .props
                    .iter()
                    .find(|(p, _)| p == prop)
                    .map(|(p, ty)| {
                        (
                            format!("{}::instance(ui).{}", first.text, snake(p)),
                            ty.clone(),
                        )
                    })
                    .ok_or_else(|| {
                        diag(
                            file,
                            first.pos,
                            &format!("`{}` has no property `{prop}`", short(&first.text)),
                        )
                    }),
                _ => Err(diag(
                    file,
                    first.pos,
                    "a singleton's property is `Name.property`",
                )),
            };
        }
        let item = if first.text == "parent" {
            match &scope.parent {
                Some(Target::Static(var, ty)) => Some((var.clone(), ty.clone())),
                _ => {
                    return Err(diag(
                        file,
                        first.pos,
                        "a component's root cannot set its parent's properties",
                    ));
                }
            }
        } else {
            scope.ids.get(&first.text).cloned()
        };
        let found = match item {
            Some((var, ty)) => self
                .property(&ty, rest)
                .map(|(rust, pty)| (format!("{var}{rust}"), pty)),
            None => {
                let root = (
                    scope.this.to_owned(),
                    ElemTy::Component(component_name(self, scope.component)),
                );
                [scope.current.clone(), Some(root)]
                    .into_iter()
                    .flatten()
                    .find_map(|(var, ty)| {
                        self.property(&ty, &names)
                            .map(|(rust, pty)| (format!("{var}{rust}"), pty))
                    })
            }
        };
        found.ok_or_else(|| {
            diag(
                file,
                first.pos,
                &format!(
                    "`{}` is not a property that can be set here",
                    names.join(".")
                ),
            )
        })
    }

    /// An element's members: values and bindings, then anchors.
    fn members(
        &self,
        scope: &Scope<'_>,
        var: &str,
        ty: &ElemTy,
        element: &Element,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut out = String::new();
        let mut anchors: Vec<(&[Ident], &Expr)> = Vec::new();
        for member in &element.members {
            let (target, expr, pos) = match member {
                Member::Binding { path, expr }
                    if path.first().is_some_and(|p| p.text == "anchors") =>
                {
                    anchors.push((path.as_slice(), expr));
                    continue;
                }
                Member::Binding { path, expr }
                    if path.first().is_some_and(|p| p.text == "KeyNavigation") =>
                {
                    out.push_str(&self.key_navigation(scope, var, path, expr)?);
                    continue;
                }
                Member::Binding { path, expr }
                    if path.first().is_some_and(|p| p.text == "Exclusive") =>
                {
                    let at = path.first().map_or(expr.pos(), |p| p.pos);
                    let (Expr::Path(names), [_, which]) = (expr, path.as_slice()) else {
                        return Err(diag(
                            file,
                            at,
                            "`Exclusive.checked: checked` names a bool property",
                        ));
                    };
                    if which.text != "checked" {
                        return Err(diag(file, which.pos, "`Exclusive` has `checked`"));
                    }
                    let (prop, pty) = self.lvalue(scope, names)?;
                    if pty != Ty::Bool {
                        return Err(diag(file, expr.pos(), "`Exclusive.checked` names a bool"));
                    }
                    let _ = writeln!(
                        out,
                        "        ui.exclusive(::tole::Element::id(&{var}), {prop});"
                    );
                    continue;
                }
                Member::Binding { path, expr }
                    if matches!(path.as_slice(), [one] if one.text == "orientation")
                        && matches!(
                            ty,
                            ElemTy::Kind(Kind::SplitView | Kind::SplitViewVertical)
                        ) =>
                {
                    // Chose the kind when it was made.
                    if !matches!(expr, Expr::Path(v) if matches!(v.as_slice(), [q, _] if q.text == "Qt"))
                    {
                        return Err(diag(
                            file,
                            expr.pos(),
                            "a SplitView's `orientation` is `Qt.Horizontal` or `Qt.Vertical`, and stays",
                        ));
                    }
                    continue;
                }
                Member::Binding { path, expr } if matches!(path.as_slice(), [one] if one.text == "focus") =>
                {
                    let Expr::Bool(on, _) = expr else {
                        return Err(diag(
                            file,
                            expr.pos(),
                            "`focus` is given as `true` or `false`; a handler moves it (`other.focus = true`, `forceActiveFocus()`)",
                        ));
                    };
                    let _ = writeln!(
                        out,
                        "        ui.set_focus(::tole::Element::id(&{var}), {on});"
                    );
                    continue;
                }
                Member::Binding { path, expr }
                    if path.first().is_some_and(|p| p.text == "Layout") =>
                {
                    let pos = path.first().map_or(expr.pos(), |p| p.pos);
                    let found = match path.as_slice() {
                        [_, name] => layout_attached(&name.text),
                        _ => None,
                    };
                    let Some((field, want)) = found else {
                        return Err(diag(
                            file,
                            pos,
                            &format!(
                                "`{}` is not one of `Layout`'s",
                                path.iter()
                                    .map(|p| p.text.as_str())
                                    .collect::<Vec<_>>()
                                    .join(".")
                            ),
                        ));
                    };
                    let _ = writeln!(
                        out,
                        "        let layout_{var} = ui.layout_of(::tole::Element::id(&{var}))?;"
                    );
                    ((format!("layout_{var}.{field}"), want), expr, pos)
                }
                Member::Binding { path, expr } => {
                    let names: Vec<&str> = path.iter().map(|p| p.text.as_str()).collect();
                    let pos = path.first().map_or(expr.pos(), |p| p.pos);
                    let found = self.property(ty, &names).ok_or_else(|| {
                        diag(
                            file,
                            pos,
                            &format!("{} has no property `{}`", describe(ty), names.join(".")),
                        )
                    })?;
                    ((format!("{var}{}", found.0), found.1), expr, pos)
                }
                Member::Property { ty, default, .. } if ty.text == "alias" || *default => continue,
                Member::Property {
                    name,
                    init: Some(expr),
                    ..
                } => {
                    let ty = scope
                        .component
                        .props
                        .iter()
                        .find(|(p, _)| *p == name.text)
                        .map(|(_, t)| t.clone());
                    let Some(ty) = ty else { continue };
                    ((format!("{var}.{}", snake(&name.text)), ty), expr, name.pos)
                }
                Member::Handler {
                    attached,
                    signal,
                    body,
                } => {
                    out.push_str(&self.handler(scope, var, ty, attached.as_ref(), signal, body)?);
                    continue;
                }
                Member::On { target, element } => {
                    let code = self.on_animation(scope, var, ty, target, element)?;
                    scope.deferred.borrow_mut().1.push_str(&code);
                    continue;
                }
                Member::Elements { name, elements } if name.text == "states" => {
                    let transitions: Vec<&Element> = element
                        .members
                        .iter()
                        .filter_map(|m| match m {
                            Member::Elements { name, elements } if name.text == "transitions" => {
                                Some(elements.iter())
                            }
                            _ => None,
                        })
                        .flatten()
                        .collect();
                    let (states, animations) =
                        self.states(scope, var, ty, elements, &transitions)?;
                    let mut deferred = scope.deferred.borrow_mut();
                    deferred.0.push_str(&states);
                    deferred.1.push_str(&animations);
                    continue;
                }
                Member::Elements { name, .. } => {
                    if element.members.iter().any(
                        |m| matches!(m, Member::Elements { name, .. } if name.text == "states"),
                    ) {
                        continue;
                    }
                    return Err(diag(
                        file,
                        name.pos,
                        "`transitions` animate changes of `states`, and there are none here",
                    ));
                }
                _ => continue,
            };
            let ((prop, want), expr, pos) = (target, expr, pos);
            let code = self.expr_want(scope, expr, Some(&want))?;
            let reads = code.reads;
            let text = coerce(file, pos, code, &want)?;
            if reads {
                let _ = writeln!(
                    out,
                    "        ::tole::Prop::bind({prop}, ui, move |ui: &mut ::tole::Ui| -> {} {{ {text} }});",
                    want.rust()
                );
            } else {
                let _ = writeln!(out, "        ::tole::Prop::set({prop}, ui, {text});");
            }
        }
        if !anchors.is_empty() {
            out.push_str(&self.anchors(scope, var, &anchors)?);
        }
        if matches!(ty, ElemTy::Kind(Kind::Repeater | Kind::ListView)) {
            out.push_str(&self.delegate_factory(scope, var, ty, element)?);
        }
        if kind_of(element) == Some(Kind::Shape) {
            // A component's root is its handle's base.
            let handle = if matches!(ty, ElemTy::Component(_)) {
                format!("{var}.base")
            } else {
                var.to_owned()
            };
            out.push_str(&self.shape_paths(scope, &handle, element)?);
        }
        Ok(out)
    }

    /// A `Repeater`'s or `ListView`'s delegate as factories: closures
    /// making the delegate's tree under the parent they are given, with
    /// `modelData` and `index` in its scope. A view's may be a
    /// `DelegateChooser`, one factory per `DelegateChoice` and a chooser
    /// picking by the row's `role`.
    fn delegate_factory(
        &self,
        scope: &Scope<'_>,
        var: &str,
        ty: &ElemTy,
        element: &Element,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let view = *ty == ElemTy::Kind(Kind::ListView);
        let what = if view { "ListView" } else { "Repeater" };
        let children: Vec<&Element> = element
            .members
            .iter()
            .filter_map(|m| match m {
                Member::Child(child) => Some(child),
                _ => None,
            })
            .collect();
        let delegate = match children.as_slice() {
            [one] => *one,
            [] => return Ok(String::new()),
            [_, second, ..] => {
                return Err(diag(
                    file,
                    second.kind.pos,
                    &format!("a `{what}` has one delegate: put the rest inside it"),
                ));
            }
        };
        // The row's type, from what the model is.
        let row_ty = match element.members.iter().find_map(|m| match m {
            Member::Binding { path, expr } if matches!(path.as_slice(), [one] if one.text == "model") => {
                Some(expr)
            }
            _ => None,
        }) {
            Some(expr) => match self.expr(scope, expr)?.ty {
                Ty::Int => Ty::Int,
                Ty::List(inner) | Ty::Model(inner) => *inner,
                other => {
                    return Err(diag(
                        file,
                        expr.pos(),
                        &format!("a model is a count, a list or a model, and this is a {}", other.name()),
                    ));
                }
            },
            None => Ty::Int,
        };
        // A view's delegates are its own children; a Repeater's, its
        // parent's.
        let parent_target = if view {
            Some(Target::Static(var.to_owned(), ty.clone()))
        } else {
            scope.parent.clone()
        };
        let id = format!("::tole::Element::id(&{var})");
        if delegate.kind.text != "DelegateChooser" {
            let factory = self.one_factory(scope, delegate, &row_ty, parent_target)?;
            return Ok(if view {
                format!(
                    "        ui.set_view_delegates({id}, ::std::vec![{factory}], ::core::option::Option::None);\n"
                )
            } else {
                format!(
                    "        {{ let factory: ::tole::DelegateFactory = {factory};\n        ui.set_delegate({id}, factory); }}\n"
                )
            });
        }
        if !view {
            return Err(diag(
                file,
                delegate.kind.pos,
                "a `DelegateChooser` chooses a `ListView`'s delegates; a `Repeater` has one",
            ));
        }
        self.chooser(scope, &id, delegate, &row_ty, parent_target)
    }

    /// A `DelegateChooser`: its choices' factories, and the chooser.
    fn chooser(
        &self,
        scope: &Scope<'_>,
        id: &str,
        chooser: &Element,
        row_ty: &Ty,
        parent_target: Option<Target>,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut role: Option<(String, Ty)> = None;
        let mut factories = Vec::new();
        let mut tests = Vec::new();
        let mut fallback: Option<usize> = None;
        for member in &chooser.members {
            match member {
                Member::Binding { path, expr } if matches!(path.as_slice(), [one] if one.text == "role") =>
                {
                    let Expr::Str(parts, pos) = expr else {
                        return Err(diag(
                            file,
                            expr.pos(),
                            "`role` names a field of the rows: `role: \"kind\"`",
                        ));
                    };
                    let name = match parts.as_slice() {
                        [StrPart::Text(text)] => text.clone(),
                        _ => String::new(),
                    };
                    let field = match row_ty {
                        Ty::Struct(s) => self
                            .structs
                            .get(s)
                            .and_then(|fields| fields.iter().find(|(f, _)| *f == name))
                            .map(|(_, t)| t.clone()),
                        _ => None,
                    };
                    let Some(field) = field else {
                        return Err(diag(
                            file,
                            *pos,
                            &format!("the rows, a {}, have no field `{name}`", row_ty.name()),
                        ));
                    };
                    role = Some((snake(&name), field));
                }
                Member::Child(choice) if choice.kind.text == "DelegateChoice" => {
                    let mut value = None;
                    let mut inner = None;
                    for m in &choice.members {
                        match m {
                            Member::Binding { path, expr } if matches!(path.as_slice(), [one] if one.text == "roleValue") =>
                            {
                                value = Some(expr);
                            }
                            Member::Child(element) => inner = Some(element),
                            _ => {
                                return Err(diag(
                                    file,
                                    choice.kind.pos,
                                    "a `DelegateChoice` holds a `roleValue` and its delegate",
                                ));
                            }
                        }
                    }
                    let Some(element) = inner else {
                        return Err(diag(
                            file,
                            choice.kind.pos,
                            "a `DelegateChoice` holds its delegate",
                        ));
                    };
                    let index = factories.len();
                    factories.push(self.one_factory(
                        scope,
                        element,
                        row_ty,
                        parent_target.clone(),
                    )?);
                    match value {
                        None => {
                            if fallback.is_none() {
                                fallback = Some(index);
                            }
                        }
                        Some(expr) => {
                            let want = role.as_ref().map_or(row_ty, |(_, t)| t);
                            let code = self.expr_want(scope, expr, Some(want))?;
                            if code.reads {
                                return Err(diag(file, expr.pos(), "a `roleValue` is a constant"));
                            }
                            let text = coerce(file, expr.pos(), code, want)?;
                            tests.push(format!("if role == {text} {{ return {index}; }}"));
                        }
                    }
                }
                Member::Child(other) => {
                    return Err(diag(
                        file,
                        other.kind.pos,
                        "a `DelegateChooser` holds `DelegateChoice`s",
                    ));
                }
                _ => {
                    return Err(diag(
                        file,
                        chooser.kind.pos,
                        "a `DelegateChooser` has a `role` and `DelegateChoice`s",
                    ));
                }
            }
        }
        let role_read = match &role {
            Some((field, _)) => format!("row.{field}"),
            None => "row".to_owned(),
        };
        Ok(format!(
            "        {{ let factories: ::std::vec::Vec<::tole::DelegateFactory> = ::std::vec![{}];\n        let chooser: ::tole::Chooser = ::std::rc::Rc::new(move |value: &::tole::Value| -> usize {{ let row: {ty} = <{ty} as ::tole::PropType>::from_value(value).unwrap_or_else(<{ty} as ::tole::PropType>::fallback); let role = {role_read}; let _ = &role; {} {} }});\n        ui.set_view_delegates({id}, factories, ::core::option::Option::Some(chooser)); }}\n",
            factories.join(", "),
            tests.join(" "),
            fallback.unwrap_or(0),
            ty = row_ty.rust(),
        ))
    }

    /// One delegate's factory: a closure making `delegate`'s tree.
    fn one_factory(
        &self,
        scope: &Scope<'_>,
        delegate: &Element,
        row_ty: &Ty,
        parent_target: Option<Target>,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let row_ty = row_ty.clone();
        let depth = scope.depth + 1;
        let root_ty = match kind_of(delegate) {
            Some(kind) => ElemTy::Kind(kind),
            None if self.components.contains_key(&delegate.kind.text) => {
                ElemTy::Component(delegate.kind.text.clone())
            }
            None => {
                return Err(diag(
                    file,
                    delegate.kind.pos,
                    &format!(
                        "`{}` is neither a kind of item nor a component",
                        delegate.kind.text
                    ),
                ));
            }
        };
        let mut elements = Vec::new();
        let mut ids = scope.ids.clone();
        let root = format!("d{depth}_0");
        self.walk(
            file,
            delegate,
            root.clone(),
            root_ty.clone(),
            (parent_target, None),
            &mut elements,
            &mut ids,
        )?;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "::std::rc::Rc::new(move |ui: &mut ::tole::Ui, parent: ::tole::ItemId| -> ::core::option::Option<::tole::Delegate> {{"
        );
        for (evar, ety, _, _, under) in &elements {
            let parent_id = match under {
                None => "parent".to_owned(),
                Some(under) => format!("::tole::Element::content(&{under})"),
            };
            let _ = writeln!(
                out,
                "        let {evar} = {}::new(ui, ::core::option::Option::Some({parent_id}))?;",
                Self::handle(ety)
            );
        }
        let _ = writeln!(
            out,
            "        let (row_{depth}, index_{depth}) = ui.delegate_cells(::tole::Element::id(&{root}));"
        );
        if let ElemTy::Component(name) = &root_ty
            && let Some(c) = self.components.get(name)
        {
            for (prop, pty) in &c.props {
                let want = match prop.as_str() {
                    "modelData" => &row_ty,
                    "index" => &Ty::Int,
                    _ => continue,
                };
                if pty != want {
                    return Err(diag(
                        file,
                        delegate.kind.pos,
                        &format!(
                            "`{name}`'s `{prop}` is a {}, and the model's rows here are a {}",
                            pty.name(),
                            want.name()
                        ),
                    ));
                }
                if prop == "modelData" {
                    let _ = writeln!(out, "        let row_{depth} = {root}.model_data.id;");
                } else {
                    let _ = writeln!(out, "        let index_{depth} = {root}.index;");
                }
            }
        }
        let _ = writeln!(
            out,
            "        let model_data_{depth}: ::tole::Prop<{}> = ::tole::Prop::from_id(row_{depth});",
            row_ty.rust()
        );
        let mut locals: Vec<(String, String, Ty)> = scope.locals.to_vec();
        locals.push((
            "modelData".to_owned(),
            format!("ui.get(model_data_{depth})"),
            row_ty,
        ));
        locals.push((
            "index".to_owned(),
            format!("ui.get(index_{depth})"),
            Ty::Int,
        ));
        let deferred = RefCell::new((String::new(), String::new()));
        for (evar, ety, el, parent, _) in &elements {
            let inner = Scope {
                component: scope.component,
                this: scope.this,
                ids: &ids,
                current: Some((evar.clone(), ety.clone())),
                parent: parent.clone(),
                params: &[],
                locals: &locals,
                depth,
                deferred: &deferred,
                file,
            };
            out.push_str(&self.members(&inner, evar, ety, el)?);
        }
        let (states, animations) = deferred.into_inner();
        out.push_str(&states);
        out.push_str(&animations);
        let _ = writeln!(
            out,
            "        ::core::option::Option::Some(::tole::Delegate {{ root: ::tole::Element::id(&{root}), row: row_{depth}, index: index_{depth} }})\n        }})"
        );
        Ok(out)
    }

    /// The bindings of an animation element: name to expression.
    fn bindings_of(element: &Element) -> Vec<(String, &Expr, Pos)> {
        element
            .members
            .iter()
            .filter_map(|m| match m {
                Member::Binding { path, expr } => Some((
                    path.iter()
                        .map(|p| p.text.as_str())
                        .collect::<Vec<_>>()
                        .join("."),
                    expr,
                    path.first().map_or(expr.pos(), |p| p.pos),
                )),
                _ => None,
            })
            .collect()
    }

    /// An animation's `duration` and `easing.type` as a `tole::Tween`.
    fn tween(&self, scope: &Scope<'_>, element: &Element) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut duration = "250.0_f32".to_owned();
        let mut easing = "::tole::Easing::Linear".to_owned();
        for (name, expr, _) in Self::bindings_of(element) {
            match name.as_str() {
                "duration" => {
                    let code = self.expr(scope, expr)?;
                    duration = coerce(file, expr.pos(), code, &Ty::Real)?;
                }
                "easing.type" => {
                    let code = self.expr(scope, expr)?;
                    easing = coerce(file, expr.pos(), code, &Ty::Easing)?;
                }
                _ => {}
            }
        }
        Ok(format!(
            "::tole::Tween {{ duration: {duration}, easing: {easing} }}"
        ))
    }

    /// The property an animation's `target` and `property` name, or
    /// `default` (the property it is `on`).
    fn animated(
        &self,
        scope: &Scope<'_>,
        element: &Element,
        default: Option<&(String, Ty)>,
    ) -> Result<(String, Ty), Diagnostic> {
        let file = scope.file;
        let bindings = Self::bindings_of(element);
        let target = bindings.iter().find(|(n, _, _)| n == "target");
        let property = bindings.iter().find(|(n, _, _)| n == "property");
        match (target, property) {
            (Some((_, target, _)), Some((_, property, at))) => {
                let (var, ty) = match target {
                    Expr::Path(names) => match names.as_slice() {
                        [one] => scope.ids.get(&one.text).cloned(),
                        _ => None,
                    },
                    _ => None,
                }
                .ok_or_else(|| diag(file, target.pos(), "an animation's `target` names an id"))?;
                let name = match property {
                    Expr::Str(parts, _) => match parts.as_slice() {
                        [StrPart::Text(text)] => text.clone(),
                        _ => String::new(),
                    },
                    _ => String::new(),
                };
                let path: Vec<&str> = name.split('.').collect();
                let (rust, pty) = self.property(&ty, &path).ok_or_else(|| {
                    diag(
                        file,
                        *at,
                        &format!("{} has no property `{name}`", describe(&ty)),
                    )
                })?;
                Ok((format!("{var}{rust}"), pty))
            }
            _ => default.cloned().ok_or_else(|| {
                diag(
                    file,
                    element.kind.pos,
                    "this animation needs a `target` and a `property`",
                )
            }),
        }
    }

    /// An animation element as a `tole::Step`, its tweens on `default`
    /// unless they name their own target.
    fn step(
        &self,
        scope: &Scope<'_>,
        element: &Element,
        default: Option<&(String, Ty)>,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        match element.kind.text.as_str() {
            "NumberAnimation" | "ColorAnimation" => {
                let (prop, ty) = self.animated(scope, element, default)?;
                let want = if element.kind.text == "ColorAnimation" {
                    Ty::Color
                } else {
                    Ty::Real
                };
                if ty != want {
                    return Err(diag(
                        file,
                        element.kind.pos,
                        &format!(
                            "a {} animates a {}, and this is a {}",
                            element.kind.text,
                            want.name(),
                            ty.name()
                        ),
                    ));
                }
                let mut ends = [
                    "::core::option::Option::None".to_owned(),
                    "::core::option::Option::None".to_owned(),
                ];
                for (name, expr, _) in Self::bindings_of(element) {
                    let slot = match name.as_str() {
                        "from" => 0,
                        "to" => 1,
                        _ => continue,
                    };
                    let code = self.expr_want(scope, expr, Some(&ty))?;
                    let text = coerce(file, expr.pos(), code, &ty)?;
                    if let Some(end) = ends.get_mut(slot) {
                        *end = format!(
                            "::core::option::Option::Some(<{} as ::tole::PropType>::into_value({text}))",
                            ty.rust()
                        );
                    }
                }
                let [from, to] = ends;
                Ok(format!(
                    "::tole::Step::Tween {{ target: {prop}.id, from: {from}, to: {to}, tween: {} }}",
                    self.tween(scope, element)?
                ))
            }
            "PauseAnimation" => {
                let tween = self.tween(scope, element)?;
                Ok(format!("::tole::Step::Pause(({tween}).duration)"))
            }
            "SequentialAnimation" | "ParallelAnimation" => {
                let mut steps = Vec::new();
                for member in &element.members {
                    if let Member::Child(child) = member {
                        steps.push(self.step(scope, child, default)?);
                    }
                }
                let kind = if element.kind.text == "SequentialAnimation" {
                    "Sequential"
                } else {
                    "Parallel"
                };
                Ok(format!(
                    "::tole::Step::{kind}(::std::vec![{}])",
                    steps.join(", ")
                ))
            }
            other => Err(diag(
                file,
                element.kind.pos,
                &format!(
                    "`{other}` is not an animation: NumberAnimation, ColorAnimation, PauseAnimation, SequentialAnimation, ParallelAnimation"
                ),
            )),
        }
    }

    /// `Behavior on x { ... }`, or an animation `on x`.
    fn on_animation(
        &self,
        scope: &Scope<'_>,
        var: &str,
        ty: &ElemTy,
        target: &[Ident],
        element: &Element,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let names: Vec<&str> = target.iter().map(|p| p.text.as_str()).collect();
        let at = target.first().map_or(element.kind.pos, |p| p.pos);
        let (rust, pty) = self.property(ty, &names).ok_or_else(|| {
            diag(
                file,
                at,
                &format!("{} has no property `{}`", describe(ty), names.join(".")),
            )
        })?;
        if !matches!(pty, Ty::Real | Ty::Color) {
            return Err(diag(
                file,
                at,
                &format!("a {} does not animate: reals and colours do", pty.name()),
            ));
        }
        let prop = (format!("{var}{rust}"), pty);
        let id = format!("::tole::Element::id(&{var})");
        if element.kind.text == "Behavior" {
            let inner: Vec<&Element> = element
                .members
                .iter()
                .filter_map(|m| match m {
                    Member::Child(child) => Some(child),
                    _ => None,
                })
                .collect();
            let [animation] = inner.as_slice() else {
                return Err(diag(
                    file,
                    element.kind.pos,
                    "a `Behavior` holds one animation",
                ));
            };
            let _ = self.step(scope, animation, Some(&prop))?;
            let tween = self.tween(scope, animation)?;
            return Ok(format!(
                "        ui.behavior({id}, {}.id, ::tole::Animate::Always({tween}));\n",
                prop.0
            ));
        }
        let step = self.step(scope, element, Some(&prop))?;
        let mut loops = "1_i32".to_owned();
        let mut running = "true".to_owned();
        let mut running_reads = false;
        for (name, expr, _) in Self::bindings_of(element) {
            match name.as_str() {
                "loops" => {
                    let code = self.expr(scope, expr)?;
                    loops = coerce(file, expr.pos(), code, &Ty::Int)?;
                }
                "running" => {
                    let code = self.expr(scope, expr)?;
                    running_reads = code.reads;
                    running = coerce(file, expr.pos(), code, &Ty::Bool)?;
                }
                _ => {}
            }
        }
        let set_running = if running_reads {
            format!(
                "::tole::Prop::bind(running, ui, move |ui: &mut ::tole::Ui| -> bool {{ {running} }});"
            )
        } else {
            format!("::tole::Prop::set(running, ui, {running});")
        };
        Ok(format!(
            "        {{ let running = ui.own_prop({id}, true); {set_running} let step = {step}; ui.animation_on({id}, step, {loops}, running); }}\n"
        ))
    }

    /// `states` (and the `transitions` beside them) on the element in
    /// `var`: the code choosing the active state and each changed
    /// property's value, and the code animating changes of state.
    fn states(
        &self,
        scope: &Scope<'_>,
        var: &str,
        ty: &ElemTy,
        states: &[Element],
        transitions: &[&Element],
    ) -> Result<(String, String), Diagnostic> {
        let file = scope.file;
        let id = format!("::tole::Element::id(&{var})");
        let rc = "::std::rc::Rc";
        let mut whens = Vec::new();
        // (target var, property as written) -> (rust prop, type, [(state, value code)])
        let mut changes: Vec<(ChangeKey, Changed)> = Vec::new();
        for state in states {
            if state.kind.text != "State" {
                return Err(diag(file, state.kind.pos, "`states` holds `State`s"));
            }
            let mut name = None;
            for member in &state.members {
                match member {
                    Member::Binding { path, expr } if matches!(path.as_slice(), [p] if p.text == "name") =>
                    {
                        name = match expr {
                            Expr::Str(parts, _) => match parts.as_slice() {
                                [StrPart::Text(text)] => Some(text.clone()),
                                _ => None,
                            },
                            _ => None,
                        };
                        if name.is_none() {
                            return Err(diag(file, expr.pos(), "a state's `name` is a string"));
                        }
                    }
                    _ => {}
                }
            }
            let Some(name) = name else {
                return Err(diag(file, state.kind.pos, "a `State` has a `name`"));
            };
            for member in &state.members {
                match member {
                    Member::Binding { path, expr } if matches!(path.as_slice(), [p] if p.text == "when") =>
                    {
                        let code = self.expr(scope, expr)?;
                        let cond = coerce(file, expr.pos(), code, &Ty::Bool)?;
                        whens.push(format!(
                            "({rc}::<str>::from({name:?}), {rc}::new(move |ui: &mut ::tole::Ui| -> bool {{ {cond} }}) as {rc}<dyn Fn(&mut ::tole::Ui) -> bool>)"
                        ));
                    }
                    Member::Binding { .. } => {}
                    Member::Child(changes_el) if changes_el.kind.text == "PropertyChanges" => {
                        let bindings = Self::bindings_of(changes_el);
                        let (tvar, tty) = match bindings.iter().find(|(n, _, _)| n == "target") {
                            Some((_, Expr::Path(names), _)) => match names.as_slice() {
                                [one] => scope.ids.get(&one.text).cloned(),
                                _ => None,
                            }
                            .ok_or_else(|| {
                                diag(file, changes_el.kind.pos, "`target` names an id")
                            })?,
                            Some((_, other, _)) => {
                                return Err(diag(file, other.pos(), "`target` names an id"));
                            }
                            None => (var.to_owned(), ty.clone()),
                        };
                        for (prop, expr, at) in bindings {
                            if prop == "target" {
                                continue;
                            }
                            let path: Vec<&str> = prop.split('.').collect();
                            let (rust, pty) = self.property(&tty, &path).ok_or_else(|| {
                                diag(
                                    file,
                                    at,
                                    &format!("{} has no property `{prop}`", describe(&tty)),
                                )
                            })?;
                            let code = self.expr_want(scope, expr, Some(&pty))?;
                            let text = coerce(file, expr.pos(), code, &pty)?;
                            let value = format!(
                                "({rc}::<str>::from({name:?}), {rc}::new(move |ui: &mut ::tole::Ui| -> ::tole::Value {{ let _ = &ui; <{} as ::tole::PropType>::into_value({text}) }}) as {rc}<dyn Fn(&mut ::tole::Ui) -> ::tole::Value>)",
                                pty.rust()
                            );
                            let key = (tvar.clone(), prop.clone());
                            match changes.iter_mut().find(|(k, _)| *k == key) {
                                Some((_, (_, _, values))) => values.push((name.clone(), value)),
                                None => changes.push((
                                    key,
                                    (format!("{tvar}{rust}"), pty, vec![(name.clone(), value)]),
                                )),
                            }
                        }
                    }
                    Member::Child(other) => {
                        return Err(diag(
                            file,
                            other.kind.pos,
                            "a `State` holds `PropertyChanges`",
                        ));
                    }
                    _ => {}
                }
            }
        }
        let active = format!("active_{var}");
        let mut out = format!(
            "        let {active} = ui.active_state({id}, ::std::vec![{}])?;\n",
            whens.join(", ")
        );
        for (_, (prop, _, values)) in &changes {
            let values: Vec<&str> = values.iter().map(|(_, v)| v.as_str()).collect();
            let _ = writeln!(
                out,
                "        ui.bind_states({id}, {prop}.id, {active}, ::std::vec![{}]);",
                values.join(", ")
            );
        }
        // Transitions: per changed property, the rules naming it.
        let mut animations = String::new();
        let mut rules: Vec<(usize, String)> = Vec::new();
        for transition in transitions {
            if transition.kind.text != "Transition" {
                return Err(diag(
                    file,
                    transition.kind.pos,
                    "`transitions` holds `Transition`s",
                ));
            }
            let mut ends = ["*".to_owned(), "*".to_owned()];
            let mut anims: Vec<&Element> = Vec::new();
            for member in &transition.members {
                match member {
                    Member::Binding { path, expr } => {
                        let slot = match path.as_slice() {
                            [p] if p.text == "from" => 0,
                            [p] if p.text == "to" => 1,
                            _ => {
                                return Err(diag(
                                    file,
                                    expr.pos(),
                                    "a `Transition` has `from`, `to` and animations",
                                ));
                            }
                        };
                        let text = match expr {
                            Expr::Str(parts, _) => match parts.as_slice() {
                                [StrPart::Text(text)] => text.clone(),
                                _ => String::new(),
                            },
                            _ => return Err(diag(file, expr.pos(), "a state's name is a string")),
                        };
                        if let Some(end) = ends.get_mut(slot) {
                            *end = text;
                        }
                    }
                    Member::Child(child) if child.kind.text == "ParallelAnimation" => {
                        anims.extend(child.members.iter().filter_map(|m| match m {
                            Member::Child(c) => Some(c),
                            _ => None,
                        }));
                    }
                    Member::Child(child) => anims.push(child),
                    _ => {}
                }
            }
            let [from, to] = ends;
            for anim in anims {
                if !matches!(
                    anim.kind.text.as_str(),
                    "NumberAnimation" | "ColorAnimation"
                ) {
                    return Err(diag(
                        file,
                        anim.kind.pos,
                        "a `Transition` runs `NumberAnimation`s and `ColorAnimation`s, together",
                    ));
                }
                let bindings = Self::bindings_of(anim);
                let target = match bindings.iter().find(|(n, _, _)| n == "target") {
                    Some((_, Expr::Path(names), _)) => match names.as_slice() {
                        [one] => scope.ids.get(&one.text).map(|(v, _)| v.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let mut names: Vec<String> = Vec::new();
                for (n, expr, _) in &bindings {
                    if (n == "property" || n == "properties")
                        && let Expr::Str(parts, _) = expr
                        && let [StrPart::Text(text)] = parts.as_slice()
                    {
                        names.extend(text.split(',').map(|p| p.trim().to_owned()));
                    }
                }
                let tween = self.tween(scope, anim)?;
                for (index, ((tvar, prop), _)) in changes.iter().enumerate() {
                    let named = names.is_empty() || names.contains(prop);
                    let aimed = target.as_ref().is_none_or(|t| t == tvar);
                    if named && aimed {
                        rules.push((
                            index,
                            format!(
                                "::tole::Transition {{ from: {rc}::from({from:?}), to: {rc}::from({to:?}), tween: {tween} }}"
                            ),
                        ));
                    }
                }
            }
        }
        for (index, (_, (prop, pty, _))) in changes.iter().enumerate() {
            let mine: Vec<&str> = rules
                .iter()
                .filter(|(i, _)| *i == index)
                .map(|(_, r)| r.as_str())
                .collect();
            if mine.is_empty() || !matches!(pty, Ty::Real | Ty::Color) {
                continue;
            }
            let _ = writeln!(
                animations,
                "        ui.behavior({id}, {prop}.id, ::tole::Animate::Transitions {{ active: {active}, rules: ::std::vec![{}] }});",
                mine.join(", ")
            );
        }
        Ok((out, animations))
    }

    /// `anchors.*` as one `Ui::anchor` call.
    fn anchors(
        &self,
        scope: &Scope<'_>,
        var: &str,
        anchors: &[(&[Ident], &Expr)],
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut fields = Vec::new();
        for (path, expr) in anchors {
            let at = path.first().map_or(expr.pos(), |p| p.pos);
            let [_, name] = path else {
                return Err(diag(
                    file,
                    at,
                    "anchors are `anchors.fill: parent`, `anchors.left: other.right`, `anchors.margins: 4`, ...",
                ));
            };
            let item = |names: &[Ident]| -> Result<String, Diagnostic> {
                match names {
                    [one] if one.text == "parent" => match &scope.parent {
                        Some(Target::Static(parent, _)) => Ok(format!(
                            "::core::option::Option::Some(::tole::Element::id(&{parent}))"
                        )),
                        Some(Target::Dynamic(child)) => {
                            Ok(format!("ui.parent(::tole::Element::id(&{child}))"))
                        }
                        None => Err(diag(file, one.pos, "nothing is `parent` here")),
                    },
                    [one] => match scope.ids.get(&one.text) {
                        Some((other, _)) => Ok(format!(
                            "::core::option::Option::Some(::tole::Element::id(&{other}))"
                        )),
                        None => Err(diag(file, one.pos, &format!("`{}` is not an id", one.text))),
                    },
                    _ => Err(diag(file, at, "an anchor names `parent` or an id")),
                }
            };
            match name.text.as_str() {
                "fill" | "centerIn" => {
                    let Expr::Path(names) = expr else {
                        return Err(diag(
                            file,
                            expr.pos(),
                            "`fill` and `centerIn` take `parent` or an id",
                        ));
                    };
                    let field = if name.text == "fill" {
                        "fill"
                    } else {
                        "center_in"
                    };
                    fields.push(format!("{field}: {}", item(names)?));
                }
                "left" | "right" | "horizontalCenter" | "top" | "bottom" | "verticalCenter" => {
                    let Expr::Path(names) = expr else {
                        return Err(diag(
                            file,
                            expr.pos(),
                            "an edge anchor takes a line: `parent.left`, `other.bottom`",
                        ));
                    };
                    let Some((line, target)) = names.split_last() else {
                        return Err(diag(file, expr.pos(), "an edge anchor takes a line"));
                    };
                    let horizontal =
                        matches!(name.text.as_str(), "left" | "right" | "horizontalCenter");
                    let line_code = match (line.text.as_str(), horizontal) {
                        ("left", true) => "Left",
                        ("right", true) => "Right",
                        ("horizontalCenter", true) => "HorizontalCenter",
                        ("top", false) => "Top",
                        ("bottom", false) => "Bottom",
                        ("verticalCenter", false) => "VerticalCenter",
                        _ => {
                            return Err(diag(
                                file,
                                line.pos,
                                &format!(
                                    "`{}` is not a line `{}` can attach to",
                                    line.text, name.text
                                ),
                            ));
                        }
                    };
                    fields.push(format!(
                        "{}: {}.map(|item| (item, ::tole::Line::{line_code}))",
                        snake(&name.text),
                        item(target)?
                    ));
                }
                "margins" | "leftMargin" | "rightMargin" | "topMargin" | "bottomMargin" => {
                    let value = constant_number(expr)
                        .ok_or_else(|| diag(file, expr.pos(), "a margin is a number, for now: margins that follow bindings come later"))?;
                    let value = format_real(value);
                    if name.text == "margins" {
                        fields.push(format!("margins: {value}"));
                    } else {
                        fields.push(format!(
                            "{}: ::core::option::Option::Some({value})",
                            snake(&name.text)
                        ));
                    }
                }
                other => {
                    return Err(diag(
                        file,
                        name.pos,
                        &format!("`anchors.{other}` is not an anchor"),
                    ));
                }
            }
        }
        Ok(format!(
            "        let _ = ui.anchor(::tole::Element::id(&{var}), &::tole::Anchors {{ {}, ..::tole::Anchors::default() }});\n",
            fields.join(", ")
        ))
    }

    /// An expression, written out and typed.
    fn expr(&self, scope: &Scope<'_>, expr: &Expr) -> Result<Code, Diagnostic> {
        self.expr_want(scope, expr, None)
    }

    /// An expression where a `want` is expected: a string literal is a
    /// colour where a colour is (`"transparent"`, `"#fff"`), also in either
    /// side of a `? :`.
    fn expr_want(
        &self,
        scope: &Scope<'_>,
        expr: &Expr,
        want: Option<&Ty>,
    ) -> Result<Code, Diagnostic> {
        let file = scope.file;
        Ok(match expr {
            Expr::Str(parts, at)
                if want == Some(&Ty::Color) && matches!(parts.as_slice(), [StrPart::Text(_)]) =>
            {
                let text = match parts.as_slice() {
                    [StrPart::Text(text)] => text.as_str(),
                    _ => "",
                };
                let rgba = named_color(text)
                    .ok_or_else(|| diag(file, *at, &format!("`{text}` is not a colour")))?;
                Code {
                    text: color_code(rgba),
                    ty: Ty::Color,
                    reads: false,
                }
            }
            Expr::Bool(value, _) => Code {
                text: value.to_string(),
                ty: Ty::Bool,
                reads: false,
            },
            Expr::Int(value, pos) => {
                let value = i32::try_from(*value)
                    .map_err(|_| diag(file, *pos, "this number is larger than an int"))?;
                Code {
                    text: format!("{value}_i32"),
                    ty: Ty::Int,
                    reads: false,
                }
            }
            Expr::Real(value, _) => Code {
                text: format_real(*value),
                ty: Ty::Real,
                reads: false,
            },
            Expr::Color(rgba, _) => Code {
                text: color_code(*rgba),
                ty: Ty::Color,
                reads: false,
            },
            Expr::Str(parts, pos) => {
                let mut pattern = String::new();
                let mut plain = String::new();
                let mut args = Vec::new();
                let mut reads = false;
                for part in parts {
                    match part {
                        StrPart::Text(text) => {
                            plain.push_str(text);
                            pattern.push_str(&text.replace('{', "{{").replace('}', "}}"));
                        }
                        StrPart::Expr(inner) => {
                            let code = self.expr(scope, inner)?;
                            if !code.ty.shows() {
                                return Err(diag(
                                    file,
                                    *pos,
                                    &format!("a {} cannot be put in a string", code.ty.name()),
                                ));
                            }
                            reads |= code.reads;
                            pattern.push_str("{}");
                            args.push(code.text);
                        }
                    }
                }
                let text = if args.is_empty() {
                    format!("::std::rc::Rc::<str>::from({plain:?})")
                } else {
                    format!(
                        "::std::rc::Rc::<str>::from(::std::format!({pattern:?}, {}))",
                        args.join(", ")
                    )
                };
                Code {
                    text,
                    ty: Ty::Str,
                    reads,
                }
            }
            Expr::Path(path) => self.path(scope, path)?,
            Expr::Unary(op, inner, pos) => {
                let code = self.expr(scope, inner)?;
                match (*op, &code.ty) {
                    ("-", Ty::Int) => Code {
                        text: format!("({}).wrapping_neg()", code.text),
                        ..code
                    },
                    ("-", Ty::Real) => Code {
                        text: format!("(-({}))", code.text),
                        ..code
                    },
                    ("!", Ty::Bool) => Code {
                        text: format!("(!({}))", code.text),
                        ..code
                    },
                    _ => {
                        return Err(diag(
                            file,
                            *pos,
                            &format!("`{op}` does not apply to a {}", code.ty.name()),
                        ));
                    }
                }
            }
            Expr::Binary(op, left, right, pos) => {
                let (a, b) = (self.expr(scope, left)?, self.expr(scope, right)?);
                binary(file, *pos, op, a, b)?
            }
            Expr::Ternary(condition, yes, no, pos) => {
                let condition = self.expr(scope, condition)?;
                if condition.ty != Ty::Bool {
                    return Err(diag(file, *pos, "a condition is a bool"));
                }
                let (yes, no) = (
                    self.expr_want(scope, yes, want)?,
                    self.expr_want(scope, no, want)?,
                );
                let ty = unify(&yes.ty, &no.ty).ok_or_else(|| {
                    diag(
                        file,
                        *pos,
                        &format!(
                            "the two sides are a {} and a {}",
                            yes.ty.name(),
                            no.ty.name()
                        ),
                    )
                })?;
                let reads = condition.reads || yes.reads || no.reads;
                let (yes, no) = (coerce(file, *pos, yes, &ty)?, coerce(file, *pos, no, &ty)?);
                Code {
                    text: format!("(if {} {{ {yes} }} else {{ {no} }})", condition.text),
                    ty,
                    reads,
                }
            }
            Expr::Call(path, args, pos) => self.call(scope, path, args, *pos)?,
            Expr::List(items, pos) => {
                let mut codes = Vec::new();
                for item in items {
                    let inner_want = match want {
                        Some(Ty::List(inner)) => Some(&**inner),
                        _ => None,
                    };
                    codes.push(self.expr_want(scope, item, inner_want)?);
                }
                let elem = match (codes.first(), want) {
                    (Some(first), _) => codes
                        .iter()
                        .try_fold(first.ty.clone(), |ty, c| unify(&ty, &c.ty))
                        .ok_or_else(|| diag(file, *pos, "a list's items are of one type"))?,
                    (None, Some(Ty::List(inner))) => (**inner).clone(),
                    (None, _) => {
                        return Err(diag(file, *pos, "an empty list here has no type"));
                    }
                };
                let reads = codes.iter().any(|c| c.reads);
                let mut texts = Vec::new();
                for code in codes {
                    texts.push(coerce(file, *pos, code, &elem)?);
                }
                Code {
                    text: format!("::std::vec![{}]", texts.join(", ")),
                    ty: Ty::List(Box::new(elem)),
                    reads,
                }
            }
            Expr::Index(list, index, fields, pos) => {
                let list = self.expr(scope, list)?;
                let Ty::List(elem) = list.ty.clone() else {
                    return Err(diag(
                        file,
                        *pos,
                        &format!("a {} cannot be indexed", list.ty.name()),
                    ));
                };
                let index = self.expr(scope, index)?;
                let reads = list.reads || index.reads;
                let index = coerce(file, *pos, index, &Ty::Int)?;
                let text = format!(
                    "({}).get(usize::try_from({index}).unwrap_or(usize::MAX)).cloned().unwrap_or_else(<{} as ::tole::PropType>::fallback)",
                    list.text,
                    elem.rust()
                );
                let names: Vec<&str> = fields.iter().map(|f| f.text.as_str()).collect();
                let code = self.fields(file, *pos, text, *elem, &names)?;
                Code { reads, ..code }
            }
        })
    }

    /// A name or a path: an id's or the parent's property, a property of
    /// the element or the component, a parameter, an enum's value, or a
    /// struct's field.
    fn path(&self, scope: &Scope<'_>, path: &[Ident]) -> Result<Code, Diagnostic> {
        let file = scope.file;
        let Some(first) = path.first() else {
            return Err(diag(file, Pos { line: 0, column: 0 }, "an empty name"));
        };
        let names: Vec<&str> = path.iter().map(|p| p.text.as_str()).collect();
        let rest = names.get(1..).unwrap_or(&[]);
        // A parameter, then its fields.
        if let Some((_, ty)) = scope.params.iter().find(|(p, _)| *p == first.text) {
            return self.fields(file, first.pos, snake(&first.text), ty.clone(), rest);
        }
        // A delegate's `modelData` or `index`.
        if let Some((_, code, ty)) = scope.locals.iter().rev().find(|(n, _, _)| *n == first.text) {
            return self.fields(file, first.pos, code.clone(), ty.clone(), rest);
        }
        // An enum's value.
        if let (Some(variants), [variant]) = (self.enums.get(&first.text), rest) {
            if variants.iter().any(|v| v == variant) {
                return Ok(Code {
                    text: format!("{}::{variant}", first.text),
                    ty: Ty::Enum(first.text.clone()),
                    reads: false,
                });
            }
            return Err(diag(
                file,
                first.pos,
                &format!("`{}` has no value `{variant}`", first.text),
            ));
        }
        if let [space, name] = names.as_slice()
            && let Some((text, ty)) = builtin_constant(space, name)
        {
            return Ok(Code {
                text: text.to_owned(),
                ty,
                reads: false,
            });
        }
        // A singleton's property.
        if let Some(c) = self.components.get(&first.text).filter(|c| c.singleton) {
            let found = rest
                .first()
                .and_then(|prop| c.props.iter().find(|(p, _)| p == prop));
            let Some((prop, pty)) = found else {
                return Err(diag(
                    file,
                    first.pos,
                    &format!(
                        "`{}` has no property `{}`",
                        short(&first.text),
                        rest.join(".")
                    ),
                ));
            };
            let text = format!(
                "::tole::Prop::get({}::instance(ui).{}, ui)",
                first.text,
                snake(prop)
            );
            return self.fields(
                file,
                first.pos,
                text,
                pty.clone(),
                rest.get(1..).unwrap_or(&[]),
            );
        }
        // An item: `parent` or an id, then a property of it.
        let item = if first.text == "parent" {
            match &scope.parent {
                Some(target) => Some(target.clone()),
                None => return Err(diag(file, first.pos, "nothing is `parent` here")),
            }
        } else {
            scope
                .ids
                .get(&first.text)
                .map(|(var, ty)| Target::Static(var.clone(), ty.clone()))
        };
        if let Some(item) = item {
            if rest.is_empty() {
                return Err(diag(
                    file,
                    first.pos,
                    "an item is not a value: name one of its properties",
                ));
            }
            return self.item_property(scope, &item, rest, first.pos);
        }
        // A property of the element the expression is on, then of the
        // component's root.
        let candidates = [
            scope.current.clone(),
            Some((
                scope.this.to_owned(),
                ElemTy::Component(component_name(self, scope.component)),
            )),
        ];
        for (var, ty) in candidates.into_iter().flatten() {
            for take in [2, 1] {
                let Some(head) = names.get(..take) else {
                    continue;
                };
                if let Some((rust, pty)) = self.property(&ty, head) {
                    let read = Code {
                        text: format!("ui.get({var}{rust})"),
                        ty: pty,
                        reads: true,
                    };
                    return self.fields(
                        file,
                        first.pos,
                        read.text,
                        read.ty,
                        names.get(take..).unwrap_or(&[]),
                    );
                }
            }
        }
        Err(diag(
            file,
            first.pos,
            &format!("`{}` is not an id, a property or a type here", first.text),
        ))
    }

    /// A property of `item`.
    fn item_property(
        &self,
        scope: &Scope<'_>,
        item: &Target,
        rest: &[&str],
        pos: Pos,
    ) -> Result<Code, Diagnostic> {
        let file = scope.file;
        for take in [2, 1] {
            let Some(head) = rest.get(..take) else {
                continue;
            };
            let tail = rest.get(take..).unwrap_or(&[]);
            match item {
                Target::Static(var, ty) => {
                    if let Some((rust, pty)) = self.property(ty, head) {
                        return self.fields(file, pos, format!("ui.get({var}{rust})"), pty, tail);
                    }
                }
                Target::Dynamic(child) => {
                    if let Some((rust, pty)) = Kind::common_property(head) {
                        let fallback = match pty {
                            Ty::Bool => "false",
                            _ => "0.0",
                        };
                        let text = format!(
                            "(match ui.parent(::tole::Element::id(&{child})).and_then(|item| ui.common(item)) {{ ::core::option::Option::Some(common) => ui.get(common{}), ::core::option::Option::None => {fallback} }})",
                            rust.trim_start_matches(".common")
                        );
                        return self.fields(file, pos, text, pty, tail);
                    }
                }
            }
        }
        Err(diag(
            file,
            pos,
            &format!("there is no property `{}` there", rest.join(".")),
        ))
    }

    /// `base`'s fields `fields`, one after another.
    fn fields(
        &self,
        file: &str,
        pos: Pos,
        base: String,
        ty: Ty,
        fields: &[&str],
    ) -> Result<Code, Diagnostic> {
        let mut code = Code {
            text: base,
            ty,
            reads: true,
        };
        for field in fields {
            if matches!(code.ty, Ty::List(_)) && *field == "length" {
                code = Code {
                    text: format!("(i32::try_from(({}).len()).unwrap_or(i32::MAX))", code.text),
                    ty: Ty::Int,
                    reads: code.reads,
                };
                continue;
            }
            if let Some((rust, ty)) = code.ty.builtin_field(field) {
                code = Code {
                    text: format!("({}).{rust}", code.text),
                    ty,
                    reads: code.reads,
                };
                continue;
            }
            let Ty::Struct(name) = &code.ty else {
                return Err(diag(
                    file,
                    pos,
                    &format!("a {} has no field `{field}`", code.ty.name()),
                ));
            };
            let found = self
                .structs
                .get(name)
                .and_then(|fields| fields.iter().find(|(f, _)| f == field))
                .map(|(_, t)| t.clone());
            let Some(ty) = found else {
                return Err(diag(file, pos, &format!("`{name}` has no field `{field}`")));
            };
            code = Code {
                text: format!("({}).{}", code.text, snake(field)),
                ty,
                reads: code.reads,
            };
        }
        Ok(code)
    }

    /// A call: a built-in function or one of the component's.
    fn call(
        &self,
        scope: &Scope<'_>,
        path: &[Ident],
        args: &[Expr],
        pos: Pos,
    ) -> Result<Code, Diagnostic> {
        let file = scope.file;
        let mut codes = Vec::new();
        for arg in args {
            codes.push(self.expr(scope, arg)?);
        }
        let reads = codes.iter().any(|c| c.reads);
        let names: Vec<&str> = path.iter().map(|p| p.text.as_str()).collect();
        let arity = |n: usize| -> Result<(), Diagnostic> {
            if codes.len() == n {
                Ok(())
            } else {
                Err(diag(
                    file,
                    pos,
                    &format!("`{}` takes {n} arguments", names.join(".")),
                ))
            }
        };
        let real = |code: Code| coerce(file, pos, code, &Ty::Real);
        let text_of = |code: Code, ty: &Ty| coerce(file, pos, code, ty);
        let code = match names.as_slice() {
            ["min" | "max"] => {
                arity(2)?;
                let mut it = codes.into_iter();
                let (Some(a), Some(b)) = (it.next(), it.next()) else {
                    return Err(diag(file, pos, "two arguments"));
                };
                let ty = unify(&a.ty, &b.ty)
                    .filter(|t| matches!(t, Ty::Int | Ty::Real))
                    .ok_or_else(|| diag(file, pos, "min and max take numbers"))?;
                let f = names.first().copied().unwrap_or("min");
                let text = format!("({}).{f}({})", text_of(a, &ty)?, text_of(b, &ty)?);
                Code { text, ty, reads }
            }
            ["clamp"] => {
                arity(3)?;
                let mut it = codes.into_iter();
                let (Some(v), Some(lo), Some(hi)) = (it.next(), it.next(), it.next()) else {
                    return Err(diag(file, pos, "three arguments"));
                };
                Code {
                    text: format!("({}).max({}).min({})", real(v)?, real(lo)?, real(hi)?),
                    ty: Ty::Real,
                    reads,
                }
            }
            ["round" | "floor" | "ceil"] => {
                arity(1)?;
                let f = names.first().copied().unwrap_or("round");
                let v = codes
                    .into_iter()
                    .next()
                    .ok_or_else(|| diag(file, pos, "one argument"))?;
                Code {
                    text: format!("(({}).{f}() as i32)", real(v)?),
                    ty: Ty::Int,
                    reads,
                }
            }
            ["abs"] => {
                arity(1)?;
                let v = codes
                    .into_iter()
                    .next()
                    .ok_or_else(|| diag(file, pos, "one argument"))?;
                let ty = v.ty.clone();
                match ty {
                    Ty::Int => Code {
                        text: format!("({}).wrapping_abs()", v.text),
                        ty,
                        reads,
                    },
                    Ty::Real => Code {
                        text: format!("({}).abs()", v.text),
                        ty,
                        reads,
                    },
                    _ => return Err(diag(file, pos, "abs takes a number")),
                }
            }
            ["font"] => {
                arity(3)?;
                let mut it = codes.into_iter();
                let (Some(family), Some(px), Some(weight)) = (it.next(), it.next(), it.next())
                else {
                    return Err(diag(file, pos, "three arguments"));
                };
                let family = text_of(family, &Ty::Str)?;
                let weight = text_of(weight, &Ty::Int)?;
                Code {
                    text: format!(
                        "::tole::Font::new(&{family}, {}, ({weight}).clamp(1, 1000) as u16)",
                        real(px)?
                    ),
                    ty: Ty::Font,
                    reads,
                }
            }
            ["group"] => {
                arity(1)?;
                let v = codes
                    .into_iter()
                    .next()
                    .ok_or_else(|| diag(file, pos, "one argument"))?;
                Code {
                    text: format!("::tole::group({})", text_of(v, &Ty::Int)?),
                    ty: Ty::Str,
                    reads,
                }
            }
            ["rgba"] | ["Qt", "rgba"] => {
                arity(4)?;
                let mut parts = Vec::new();
                for code in codes {
                    parts.push(format!(
                        "(({}).clamp(0.0, 1.0) * 255.0).round() as u8",
                        real(code)?
                    ));
                }
                Code {
                    text: format!("::tole::Color::rgba({})", parts.join(", ")),
                    ty: Ty::Color,
                    reads,
                }
            }
            ["alpha"] => {
                arity(2)?;
                let mut it = codes.into_iter();
                let (Some(color), Some(alpha)) = (it.next(), it.next()) else {
                    return Err(diag(file, pos, "two arguments"));
                };
                Code {
                    text: format!("({}).alpha({})", text_of(color, &Ty::Color)?, real(alpha)?),
                    ty: Ty::Color,
                    reads,
                }
            }
            [name] => {
                if !scope.component.functions.contains_key(*name) {
                    return Err(diag(file, pos, &format!("`{name}` is not a function here")));
                }
                self.function_call(scope, scope.component, name, codes, scope.this, pos)?
            }
            [id, name]
                if scope.ids.get(*id).is_some_and(|(_, ty)| {
                    matches!(ty, ElemTy::Component(c) if self.components.get(c).is_some_and(|c| c.functions.contains_key(*name)))
                }) =>
            {
                let Some((var, ElemTy::Component(key))) = scope.ids.get(*id) else {
                    return Err(diag(file, pos, "no component"));
                };
                let Some(c) = self.components.get(key) else {
                    return Err(diag(file, pos, "no component"));
                };
                let receiver = if var == "this" { scope.this } else { var.as_str() };
                self.function_call(scope, c, name, codes, receiver, pos)?
            }
            [space, name]
                if self
                    .components
                    .get(*space)
                    .is_some_and(|c| c.singleton && c.functions.contains_key(*name)) =>
            {
                let Some(c) = self.components.get(*space) else {
                    return Err(diag(file, pos, "no singleton"));
                };
                let receiver = format!("{space}::instance(ui)");
                self.function_call(scope, c, name, codes, &receiver, pos)?
            }
            _ => {
                return Err(diag(
                    file,
                    pos,
                    &format!("`{}` is not a function", names.join(".")),
                ));
            }
        };
        Ok(code)
    }
}

impl Program {
    /// A `Shape`'s `ShapePath`s as one binding of its `paths`: each a
    /// path from `startX`, `startY` through its `PathMove`, `PathLine`,
    /// `PathQuad` and `PathCubic` steps, its fill, stroke and width.
    fn shape_paths(
        &self,
        scope: &Scope<'_>,
        var: &str,
        element: &Element,
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
        let value =
            |name: &str, el: &Element, ty: Ty, default: &str| -> Result<String, Diagnostic> {
                match Self::bindings_of(el)
                    .into_iter()
                    .find(|(n, _, _)| n == name)
                {
                    Some((_, expr, _)) => {
                        let code = self.expr_want(scope, expr, Some(&ty))?;
                        coerce(file, expr.pos(), code, &ty)
                    }
                    None => Ok(default.to_owned()),
                }
            };
        let mut body = String::new();
        for member in &element.members {
            let Member::Child(path) = member else {
                continue;
            };
            if path.kind.text != "ShapePath" {
                return Err(diag(file, path.kind.pos, "a `Shape` holds `ShapePath`s"));
            }
            for m in &path.members {
                if let Member::Binding { path: p, expr } = m
                    && !matches!(
                        p.first().map(|i| i.text.as_str()),
                        Some(
                            "strokeColor"
                                | "fillColor"
                                | "strokeWidth"
                                | "startX"
                                | "startY"
                                | "closed"
                                | "capStyle"
                                | "joinStyle"
                        )
                    )
                {
                    return Err(diag(
                        file,
                        p.first().map_or(expr.pos(), |i| i.pos),
                        "a `ShapePath` has `strokeColor`, `fillColor`, `strokeWidth`, `capStyle`, `joinStyle`, `startX`, `startY` and `closed`",
                    ));
                }
            }
            let _ = write!(
                body,
                "{{ let mut path = ::tole::Path::new(); let _ = path.move_to({}, {}); ",
                value("startX", path, Ty::Real, "0.0_f32")?,
                value("startY", path, Ty::Real, "0.0_f32")?
            );
            for step in &path.members {
                let Member::Child(step) = step else {
                    continue;
                };
                let real = |name: &str| value(name, step, Ty::Real, "0.0_f32");
                let call = match step.kind.text.as_str() {
                    "PathMove" => format!("path.move_to({}, {})", real("x")?, real("y")?),
                    "PathLine" => format!("path.line_to({}, {})", real("x")?, real("y")?),
                    "PathQuad" => format!(
                        "path.quad_to({}, {}, {}, {})",
                        real("controlX")?,
                        real("controlY")?,
                        real("x")?,
                        real("y")?
                    ),
                    "PathCircle" => format!(
                        "path.circle({}, {}, {})",
                        real("centerX")?,
                        real("centerY")?,
                        real("radius")?
                    ),
                    "PathCubic" => format!(
                        "path.cubic_to(({}, {}), ({}, {}), {}, {})",
                        real("control1X")?,
                        real("control1Y")?,
                        real("control2X")?,
                        real("control2Y")?,
                        real("x")?,
                        real("y")?
                    ),
                    _ => {
                        return Err(diag(
                            file,
                            step.kind.pos,
                            "a `ShapePath` holds `PathMove`, `PathLine`, `PathQuad`, `PathCubic` and `PathCircle`",
                        ));
                    }
                };
                let _ = write!(body, "let _ = {call}; ");
            }
            let _ = write!(
                body,
                "let _ = path.set_style(::tole::shape_style({}, {})); ",
                value("capStyle", path, Ty::Int, "2_i32")?,
                value("joinStyle", path, Ty::Int, "128_i32")?
            );
            let _ = write!(
                body,
                "if {} {{ let _ = path.close(); }} paths.push(::tole::ShapePathValue {{ path, fill: {}, stroke: {}, width: {} }}); }} ",
                value("closed", path, Ty::Bool, "false")?,
                value("fillColor", path, Ty::Color, "::tole::Color::TRANSPARENT")?,
                value(
                    "strokeColor",
                    path,
                    Ty::Color,
                    "::tole::Color::rgba(0, 0, 0, 255)"
                )?,
                value("strokeWidth", path, Ty::Real, "1.0_f32")?
            );
        }
        Ok(format!(
            "        ::tole::Prop::bind({var}.props.paths, ui, move |ui: &mut ::tole::Ui| -> ::tole::ShapePaths {{ let _ = &ui; let mut paths = ::std::vec::Vec::new(); {body}::tole::ShapePaths::new(paths) }});\n"
        ))
    }

    /// A call of `owner`'s function `name` on `receiver`, its missing
    /// arguments given their defaults.
    fn function_call(
        &self,
        scope: &Scope<'_>,
        owner: &Component,
        name: &str,
        mut codes: Vec<Code>,
        receiver: &str,
        pos: Pos,
    ) -> Result<Code, Diagnostic> {
        let file = scope.file;
        let Some((params, ret)) = owner.functions.get(name) else {
            return Err(diag(file, pos, &format!("`{name}` is not a function here")));
        };
        let defaults = owner.defaults.get(name).cloned().unwrap_or_default();
        if codes.len() > params.len() {
            return Err(diag(
                file,
                pos,
                &format!("`{name}` takes {} arguments", params.len()),
            ));
        }
        for index in codes.len()..params.len() {
            let Some(Some(default)) = defaults.get(index) else {
                return Err(diag(
                    file,
                    pos,
                    &format!("`{name}` takes {} arguments", params.len()),
                ));
            };
            let code = self.expr(scope, default)?;
            if code.reads {
                return Err(diag(file, default.pos(), "a default is a constant"));
            }
            codes.push(code);
        }
        // The arguments are worked out first, each of them may read the
        // `Ui` the call is then lent.
        let mut lets = format!("let receiver = {receiver}; ");
        let mut args = String::new();
        for (index, (code, (_, ty))) in codes.into_iter().zip(params).enumerate() {
            let text = coerce(file, pos, code, ty)?;
            let _ = write!(lets, "let arg{index}: {} = {text}; ", ty.rust());
            let _ = write!(args, ", arg{index}");
        }
        Ok(Code {
            text: format!("{{ {lets}receiver.{}(ui{args}) }}", snake(name)),
            ty: ret.clone(),
            reads: true,
        })
    }
}

/// A property a state changes: the target's local and the property as
/// the markup writes it.
type ChangeKey = (String, String);

/// What the states make of it: its Rust path, its type, and each state's
/// value as code.
type Changed = (String, Ty, Vec<(String, String)>);

/// The name of the component `component` is.
fn component_name(program: &Program, component: &Component) -> String {
    program
        .components
        .iter()
        .find(|(_, c)| std::ptr::eq(*c, component))
        .map(|(name, _)| name.clone())
        .unwrap_or_default()
}

/// A diagnostic.
fn diag(file: &str, pos: Pos, message: &str) -> Diagnostic {
    Diagnostic {
        file: file.to_owned(),
        pos,
        message: message.to_owned(),
    }
}

/// How a diagnostic names an element's type.
fn describe(ty: &ElemTy) -> String {
    match ty {
        ElemTy::Kind(kind) => format!("{kind:?}"),
        ElemTy::Component(name) => name.clone(),
    }
}

/// `PropType` for a declared type.
fn prop_type_impl(name: &str) -> String {
    format!(
        "impl ::tole::PropType for {name} {{\n    fn into_value(self) -> ::tole::Value {{\n        ::tole::Value::custom(self)\n    }}\n    fn from_value(value: &::tole::Value) -> ::core::option::Option<Self> {{\n        value.downcast::<Self>().cloned()\n    }}\n    fn fallback() -> Self {{\n        <Self as ::core::default::Default>::default()\n    }}\n}}\n\n"
    )
}

/// A colour as Rust.
fn color_code(rgba: u32) -> String {
    format!(
        "::tole::Color::rgba({}, {}, {}, {})",
        rgba >> 24,
        (rgba >> 16) & 0xff,
        (rgba >> 8) & 0xff,
        rgba & 0xff
    )
}

/// A real as a Rust `f32` literal.
fn format_real(value: f64) -> String {
    let text = format!("{value:?}");
    if text.contains('.') || text.contains('e') {
        format!("{text}_f32")
    } else {
        format!("{text}.0_f32")
    }
}

/// A number written as a literal, maybe negated.
fn constant_number(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Int(value, _) => Some(*value as f64),
        Expr::Real(value, _) => Some(*value),
        Expr::Unary("-", inner, _) => constant_number(inner).map(|v| -v),
        _ => None,
    }
}

/// The type both of two types can be: the same, or `real` for an `int`
/// and a `real`.
fn unify(a: &Ty, b: &Ty) -> Option<Ty> {
    match (a, b) {
        _ if a == b => Some(a.clone()),
        (Ty::Int, Ty::Real) | (Ty::Real, Ty::Int) => Some(Ty::Real),
        _ => None,
    }
}

/// `code` as a `want`: as it is, or an `int` widened.
fn coerce(file: &str, pos: Pos, code: Code, want: &Ty) -> Result<String, Diagnostic> {
    match (&code.ty, want) {
        (have, want) if have == want => Ok(code.text),
        (Ty::Int, Ty::Real) => Ok(format!("(({}) as f32)", code.text)),
        (Ty::Int, Ty::ModelSource) => Ok(format!("::tole::ModelSource::count({})", code.text)),
        (Ty::List(_), Ty::ModelSource) => Ok(format!("::tole::ModelSource::list({})", code.text)),
        (Ty::Model(_), Ty::ModelSource) => Ok(format!("::tole::ModelSource::model({})", code.text)),
        _ => Err(diag(
            file,
            pos,
            &format!(
                "a {} was expected, and this is a {}",
                want.name(),
                code.ty.name()
            ),
        )),
    }
}

/// A binary operator.
fn binary(file: &str, pos: Pos, op: &str, a: Code, b: Code) -> Result<Code, Diagnostic> {
    let reads = a.reads || b.reads;
    let mismatch = |a: &Code, b: &Code| {
        diag(
            file,
            pos,
            &format!(
                "`{op}` does not join a {} and a {}",
                a.ty.name(),
                b.ty.name()
            ),
        )
    };
    match op {
        "+" if a.ty == Ty::Str || b.ty == Ty::Str => {
            if !a.ty.shows() || !b.ty.shows() {
                return Err(mismatch(&a, &b));
            }
            Ok(Code {
                text: format!(
                    "::std::rc::Rc::<str>::from(::std::format!(\"{{}}{{}}\", {}, {}))",
                    a.text, b.text
                ),
                ty: Ty::Str,
                reads,
            })
        }
        "+" | "-" | "*" | "/" | "%" => {
            let ty = unify(&a.ty, &b.ty)
                .filter(|t| matches!(t, Ty::Int | Ty::Real))
                .ok_or_else(|| mismatch(&a, &b))?;
            // Division gives a real, as in QML, whatever it divides.
            let ty = if op == "/" { Ty::Real } else { ty };
            let (x, y) = (coerce(file, pos, a, &ty)?, coerce(file, pos, b, &ty)?);
            let text = match (op, &ty) {
                ("+", Ty::Int) => format!("({x}).wrapping_add({y})"),
                ("-", Ty::Int) => format!("({x}).wrapping_sub({y})"),
                ("*", Ty::Int) => format!("({x}).wrapping_mul({y})"),
                ("%", Ty::Int) => format!("({x}).checked_rem({y}).unwrap_or(0)"),
                _ => format!("({x} {op} {y})"),
            };
            Ok(Code { text, ty, reads })
        }
        "<" | "<=" | ">" | ">=" => {
            let ty = unify(&a.ty, &b.ty)
                .filter(|t| matches!(t, Ty::Int | Ty::Real))
                .ok_or_else(|| mismatch(&a, &b))?;
            let (x, y) = (coerce(file, pos, a, &ty)?, coerce(file, pos, b, &ty)?);
            Ok(Code {
                text: format!("({x} {op} {y})"),
                ty: Ty::Bool,
                reads,
            })
        }
        "==" | "!=" => {
            let ty = unify(&a.ty, &b.ty).ok_or_else(|| mismatch(&a, &b))?;
            let (x, y) = (coerce(file, pos, a, &ty)?, coerce(file, pos, b, &ty)?);
            Ok(Code {
                text: format!("({x} {op} {y})"),
                ty: Ty::Bool,
                reads,
            })
        }
        "|" => {
            if a.ty != Ty::Int || b.ty != Ty::Int {
                return Err(mismatch(&a, &b));
            }
            Ok(Code {
                text: format!("({} | {})", a.text, b.text),
                ty: Ty::Int,
                reads,
            })
        }
        "&&" | "||" => {
            if a.ty != Ty::Bool || b.ty != Ty::Bool {
                return Err(mismatch(&a, &b));
            }
            Ok(Code {
                text: format!("({} {op} {})", a.text, b.text),
                ty: Ty::Bool,
                reads,
            })
        }
        _ => Err(diag(file, pos, &format!("`{op}` is not an operator here"))),
    }
}

/// What a function lookup falls back to.
trait OrDefaultFn {
    /// The function's parameters and result, or none and `bool`.
    fn unwrap_or_default_fn(self) -> (Vec<(String, Ty)>, Ty);
}

impl OrDefaultFn for Option<(Vec<(String, Ty)>, Ty)> {
    fn unwrap_or_default_fn(self) -> (Vec<(String, Ty)>, Ty) {
        self.unwrap_or((Vec::new(), Ty::Bool))
    }
}

/// `lineActivated` as `LineActivated`.
fn upper(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// An element as the walk gathers it: its local, its type, the element,
/// what `parent` is in it, and the local of the element it is made under
/// (`None` for the root).
type Walked<'e> = (String, ElemTy, &'e Element, Option<Target>, Option<String>);

/// The kind of item `element` is, if it is one: a `SplitView` whose
/// `orientation` is `Qt.Vertical` is the kind that places down.
fn kind_of(element: &Element) -> Option<Kind> {
    let kind = Kind::named(&element.kind.text)?;
    let vertical = element.members.iter().any(|m| {
        matches!(m, Member::Binding { path, expr: Expr::Path(value) }
            if matches!(path.as_slice(), [p] if p.text == "orientation")
                && matches!(value.as_slice(), [q, v] if q.text == "Qt" && v.text == "Vertical"))
    });
    Some(if kind == Kind::SplitView && vertical {
        Kind::SplitViewVertical
    } else {
        kind
    })
}

/// The last part of a key: the name its declaration is written with.
fn short(key: &str) -> &str {
    key.rsplit("::").next().unwrap_or(key)
}

/// The Rust module a key's declaration is written in: `""` at the top.
fn module_of(key: &str) -> &str {
    key.rsplit_once("::").map_or("", |(module, _)| module)
}

/// Where a member starts, for a diagnostic.
fn member_pos(member: &Member) -> Option<Pos> {
    match member {
        Member::Property { name, .. }
        | Member::Function { name, .. }
        | Member::Signal { name, .. }
        | Member::Elements { name, .. } => Some(name.pos),
        Member::Id(id) => Some(id.pos),
        Member::Binding { path, .. } => path.first().map(|p| p.pos),
        Member::Child(child) | Member::On { element: child, .. } => Some(child.kind.pos),
        Member::Handler { signal, .. } => Some(signal.pos),
    }
}

/// The element in `element`'s tree whose id is `id`, not looking into a
/// `Repeater`'s or view's delegate, which is a tree of its own.
fn find_id<'e>(element: &'e Element, id: &str, top: bool) -> Option<&'e Element> {
    if element
        .members
        .iter()
        .any(|m| matches!(m, Member::Id(name) if name.text == id))
    {
        return Some(element);
    }
    if !top
        && matches!(
            element.kind.text.as_str(),
            "Repeater" | "ListView" | "Shape"
        )
    {
        return None;
    }
    element.members.iter().find_map(|m| match m {
        Member::Child(child) => find_id(child, id, false),
        _ => None,
    })
}
