//! Checking and writing out: every declaration of every file, resolved
//! and type-checked, as Rust against the runtime's API.
//!
//! A component becomes a `Copy` struct of its root's handle and its
//! declared properties, with a `new(ui, parent)` that makes its items in
//! tree order, then gives every property its value or binding, then
//! anchors. Bindings are lazy, so a binding may read an item declared
//! after it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::parser::{Decl, Element, Expr, File, Ident, Member, Stmt, StrPart};
use crate::types::{Kind, Ty, builtin_constant, named_color, snake};
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
    pub(crate) fn gather(files: &[(String, File)]) -> Result<Self, Diagnostic> {
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
        for (file, parsed) in files {
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
                    Decl::Component { name, .. } => taken(name, file, &mut seen)?,
                }
            }
        }
        for (file, parsed) in files {
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
        for (file, parsed) in files {
            for decl in &parsed.decls {
                if let Decl::Component { name, root } = decl {
                    let mut props = Vec::new();
                    let mut functions = BTreeMap::new();
                    let mut signals = Vec::new();
                    for member in &root.members {
                        match member {
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
                                name, params, ret, ..
                            } => {
                                let mut typed = Vec::new();
                                for (param, ty) in params {
                                    typed.push((param.text.clone(), program.ty(file, ty)?));
                                }
                                let _ = functions
                                    .insert(name.text.clone(), (typed, program.ty(file, ret)?));
                            }
                            _ => {}
                        }
                    }
                    let base = match Kind::named(&root.kind.text) {
                        Some(kind) => ElemTy::Kind(kind),
                        None => ElemTy::Component(root.kind.text.clone()),
                    };
                    let component = Component {
                        file: file.clone(),
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
        Ok(program)
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
        for (name, variants) in &self.enums {
            let _ = writeln!(
                out,
                "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]\npub enum {name} {{"
            );
            for (index, variant) in variants.iter().enumerate() {
                let default = if index == 0 { "#[default] " } else { "" };
                let _ = writeln!(out, "    {default}{variant},");
            }
            out.push_str("}\n\n");
            out.push_str(&prop_type_impl(name));
        }
        for (name, fields) in &self.structs {
            let _ = writeln!(
                out,
                "#[derive(Clone, Debug, Default, PartialEq)]\npub struct {name} {{"
            );
            for (field, ty) in fields {
                let _ = writeln!(out, "    pub {}: {},", snake(field), ty.rust());
            }
            out.push_str("}\n\n");
            out.push_str(&prop_type_impl(name));
        }
        for (name, component) in &self.components {
            out.push_str(&self.component(name, component)?);
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
    fn component(&self, name: &str, c: &Component) -> Result<String, Diagnostic> {
        let file = c.file.as_str();
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
        out.push_str("}\n\n");

        // Every element in tree order, with its local and its parent's.
        let mut elements: Vec<(String, ElemTy, &Element, Option<Target>)> = Vec::new();
        let mut ids: HashMap<String, (String, ElemTy)> = HashMap::new();
        self.walk(
            file,
            &c.root,
            "this".to_owned(),
            ElemTy::Component(name.to_owned()),
            Some(Target::Dynamic("this".to_owned())),
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
        let _ = writeln!(out, "        let this = {name} {{\n            base,");
        for (prop, ty) in &c.props {
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
        out.push_str("        };\n");
        for (var, ty, _, parent) in elements.iter().skip(1) {
            let parent_id = match parent {
                Some(Target::Static(parent_var, _)) => {
                    format!("::tole::Element::id(&{parent_var})")
                }
                _ => "::tole::Element::id(&this)".to_owned(),
            };
            let _ = writeln!(
                out,
                "        let {var} = {}::new(ui, ::core::option::Option::Some({parent_id}))?;",
                Self::handle(ty)
            );
        }
        for (var, ty, element, parent) in &elements {
            let scope = Scope {
                component: c,
                this: "this",
                ids: &ids,
                current: Some((var.clone(), ty.clone())),
                parent: parent.clone(),
                params: &[],
                file,
            };
            out.push_str(&self.members(&scope, var, ty, element)?);
        }
        out.push_str("        ::core::option::Option::Some(this)\n    }\n");
        for member in &c.root.members {
            if let Member::Function {
                name: fname, body, ..
            } = member
            {
                let (typed, ret) = self
                    .components
                    .get(name)
                    .and_then(|c| c.functions.get(&fname.text))
                    .cloned()
                    .unwrap_or_default_fn();
                let scope = Scope {
                    component: c,
                    this: "self",
                    ids: &ids,
                    current: None,
                    parent: None,
                    params: &typed,
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
        let _ = writeln!(
            out,
            "impl ::tole::Element for {name} {{\n    fn id(&self) -> ::tole::ItemId {{\n        ::tole::Element::id(&self.base)\n    }}\n    fn common(&self) -> ::tole::Common {{\n        ::tole::Element::common(&self.base)\n    }}\n}}\n"
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
        parent: Option<Target>,
        out: &mut Vec<(String, ElemTy, &'e Element, Option<Target>)>,
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
        out.push((var.clone(), ty.clone(), element, parent));
        for member in &element.members {
            if let Member::Child(child) = member {
                let child_ty = match Kind::named(&child.kind.text) {
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
                let child_var = format!("e{}", out.len());
                self.walk(
                    file,
                    child,
                    child_var,
                    child_ty,
                    Some(Target::Static(var.clone(), ty.clone())),
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
        signal: &Ident,
        body: &[Stmt],
    ) -> Result<String, Diagnostic> {
        let file = scope.file;
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

    /// A handler's statements.
    fn statements(&self, scope: &Scope<'_>, body: &[Stmt]) -> Result<String, Diagnostic> {
        let file = scope.file;
        let mut out = String::new();
        for statement in body {
            match statement {
                Stmt::Assign(path, expr) => {
                    let (prop, want) = self.lvalue(scope, path)?;
                    let code = self.expr_want(scope, expr, Some(&want))?;
                    let text = coerce(file, expr.pos(), code, &want)?;
                    let _ = write!(
                        out,
                        "{{ let value = {text}; ::tole::Prop::set({prop}, ui, value); }} "
                    );
                }
                Stmt::Call(Expr::Call(path, args, pos)) => {
                    if let Some((signal, params)) = self.emitted(scope, path) {
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
                    (found, expr, pos)
                }
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
                    ((format!(".{}", snake(&name.text)), ty), expr, name.pos)
                }
                Member::Handler { signal, body } => {
                    out.push_str(&self.handler(scope, var, ty, signal, body)?);
                    continue;
                }
                _ => continue,
            };
            let ((rust, want), expr, pos) = (target, expr, pos);
            let code = self.expr_want(scope, expr, Some(&want))?;
            let reads = code.reads;
            let text = coerce(file, pos, code, &want)?;
            let prop = format!("{var}{rust}");
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
        Ok(out)
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
                let Some((params, ret)) = scope.component.functions.get(*name) else {
                    return Err(diag(file, pos, &format!("`{name}` is not a function here")));
                };
                if params.len() != codes.len() {
                    return Err(diag(
                        file,
                        pos,
                        &format!("`{name}` takes {} arguments", params.len()),
                    ));
                }
                let mut texts = Vec::new();
                for (code, (_, ty)) in codes.into_iter().zip(params) {
                    texts.push(text_of(code, ty)?);
                }
                let args: String = texts.iter().map(|t| format!(", {t}")).collect();
                Code {
                    text: format!("{}.{}(ui{args})", scope.this, snake(name)),
                    ty: ret.clone(),
                    reads: true,
                }
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
