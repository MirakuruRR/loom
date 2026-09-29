//! Everything the lifted packages declare, read before any of them is lifted, so each can name
//! and call the others: their Blueprints' functions, structs and enums.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::Value;

use super::input::{field_name, str_of, Package, Prop, PropTy, Ref};
use super::print::name;
use crate::syntax::{Expr, ExprKind, Span};
use crate::types::{TypeDb, TypeScope};

pub struct Program<'a> {
    pub db: &'a TypeDb,
    pub blueprints: Vec<Blueprint>,
    pub structs: Vec<UserStruct>,
    pub enums: Vec<UserEnum>,
    /// Packages that could not be read, each with why.
    pub unread: Vec<(String, anyhow::Error)>,
}

pub struct Blueprint {
    /// The asset's name, as loom names the Blueprint.
    pub name: String,
    /// Its package, `/Game/...`.
    pub path: String,
    /// Its generated class's path, `/Game/.../BP_X.BP_X_C`.
    pub class: String,
    /// The parent class's path.
    pub parent: String,
    pub widget: bool,
    /// A Blueprint Interface, whose functions are only declared.
    pub interface: bool,
    /// The interfaces its class implements itself, by their class paths.
    pub interfaces: Vec<String>,
    /// Every function the class compiled, by its cooked name.
    pub functions: HashMap<String, Function>,
    pub package: Package,
}

#[derive(Debug, Clone)]
pub struct Function {
    /// The name loom gives it, which is the cooked name unless that is not a loom name.
    pub name: String,
    pub params: Vec<Param>,
    pub pure: bool,
    pub is_static: bool,
    pub access: Option<&'static str>,
    /// An event or custom event's stub, which only jumps into the event graph: the graph's
    /// offset it enters at, and the graph variables it copies its parameters into.
    pub stub: Option<Stub>,
    /// It overrides a function or event of a parent class.
    pub overrides: bool,
}

#[derive(Debug, Clone)]
pub struct Stub {
    pub entry: usize,
    /// Graph variable to the parameter it is copied from.
    pub copies: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub cooked: String,
    pub name: String,
    pub ty: PropTy,
    pub dir: Dir,
    /// The function's return value, which a call gives as its value rather than through an
    /// argument.
    pub ret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    In,
    /// Passed by reference: the caller's own variable.
    Ref,
    Out,
}

pub struct UserStruct {
    pub name: String,
    pub path: String,
    /// The struct's path as a type, `/Game/.../S.S`.
    pub type_path: String,
    /// Each field's cooked name, its name in loom, and its type, in order.
    pub fields: Vec<(String, String, PropTy)>,
    pub defaults: Value,
}

pub struct UserEnum {
    pub name: String,
    pub path: String,
    pub type_path: String,
    /// Each value's cooked name, and its name in loom.
    pub values: Vec<(String, String)>,
}

impl<'a> Program<'a> {
    pub fn read(db: &'a TypeDb, packages: Vec<Package>) -> Result<Program<'a>> {
        let mut program = Program { db, blueprints: Vec::new(), structs: Vec::new(), enums: Vec::new(), unread: Vec::new() };
        for package in packages {
            let path = package.path.clone();
            if let Err(err) = program.add(package) {
                program.unread.push((path, err));
            }
        }
        Ok(program)
    }

    fn add(&mut self, package: Package) -> Result<()> {
        let name = package.path.rsplit('/').next().unwrap_or_default().to_string();
        if let Some(s) = package.export("UserDefinedStruct") {
            let mut fields = Vec::new();
            for p in s.get("ChildProperties").and_then(Value::as_array).into_iter().flatten() {
                let prop = Prop::of(p)?;
                fields.push((prop.name.clone(), own_name(field_name(&prop.name))?, prop.ty));
            }
            self.structs.push(UserStruct {
                type_path: format!("{}.{name}", package.path),
                name,
                path: package.path.clone(),
                fields,
                defaults: s.get("DefaultProperties").cloned().unwrap_or(Value::Null),
            });
            return Ok(());
        }
        if let Some(e) = package.export("UserDefinedEnum") {
            let names = e.get("Names").and_then(Value::as_object).ok_or_else(|| refusal!(Unexpected, "an enum that lists no values"))?;
            let labels: HashMap<&str, &str> = e
                .pointer("/Properties/DisplayNameMap")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| Some((str_of(m, "Key")?, m.pointer("/Value/SourceString")?.as_str()?)))
                .collect();
            let mut values: Vec<(i64, String, String)> = Vec::new();
            for (full, n) in names {
                let cooked = full.rsplit("::").next().unwrap_or(full);
                // The last value is the one the engine adds, _MAX, which is not declared.
                if cooked.ends_with("_MAX") {
                    continue;
                }
                let label = labels.get(cooked).copied().unwrap_or(cooked);
                values.push((n.as_i64().unwrap_or(0), cooked.to_string(), own_name(label)?));
            }
            values.sort_by_key(|v| v.0);
            self.enums.push(UserEnum {
                type_path: format!("{}.{name}", package.path),
                name,
                path: package.path.clone(),
                values: values.into_iter().map(|(_, c, n)| (c, n)).collect(),
            });
            return Ok(());
        }
        let class = package
            .exports
            .iter()
            .find(|e| matches!(str_of(e, "Type"), Some("BlueprintGeneratedClass" | "WidgetBlueprintGeneratedClass")))
            .ok_or_else(|| refusal!(Inexpressible, "an asset that is not a Blueprint, a struct or an enum"))?;
        let widget = str_of(class, "Type") == Some("WidgetBlueprintGeneratedClass");
        let class_name = str_of(class, "Name").ok_or_else(|| refusal!(Unexpected, "a class with no name"))?;
        let parent = Ref::of(class.get("SuperStruct").ok_or_else(|| refusal!(Unexpected, "a class with no parent"))?)?.path();
        let mut interfaces = Vec::new();
        for implemented in class.get("Interfaces").and_then(Value::as_array).into_iter().flatten() {
            let interface = implemented.get("Class").ok_or_else(|| refusal!(Unexpected, "an implemented interface with no class"))?;
            interfaces.push(Ref::of(interface)?.path());
        }
        let mut functions = HashMap::new();
        for f in package.exports.iter().filter(|e| str_of(e, "Type") == Some("Function")) {
            let cooked = str_of(f, "Name").unwrap_or_default().to_string();
            if cooked.starts_with("ExecuteUbergraph_") || cooked.ends_with("__DelegateSignature") {
                continue;
            }
            let function = read_function(f).with_context(|| format!("reading function {cooked}"))?;
            functions.insert(cooked, function);
        }
        self.blueprints.push(Blueprint {
            class: format!("{}.{class_name}", package.path),
            name,
            path: package.path.clone(),
            interface: parent == crate::types::INTERFACE,
            parent,
            widget,
            interfaces,
            functions,
            package,
        });
        Ok(())
    }

    pub fn blueprint(&self, class: &str) -> Option<&Blueprint> {
        self.blueprints.iter().find(|b| b.class == class)
    }

    pub fn user_struct(&self, path: &str) -> Option<&UserStruct> {
        self.structs.iter().find(|s| s.type_path == path)
    }

    pub fn user_enum(&self, path: &str) -> Option<&UserEnum> {
        self.enums.iter().find(|e| e.type_path == path)
    }

    /// A function of a class, found on the class or the first of its parents that declares it:
    /// one of the program's Blueprints, or the engine's or game's class.
    pub fn function(&self, class: &str, name: &str) -> Option<(String, Callee)> {
        let mut class = class.to_string();
        loop {
            if let Some(bp) = self.blueprint(&class) {
                if let Some(f) = bp.functions.get(name) {
                    return Some((class, Callee::Lifted(f.clone())));
                }
                class = bp.parent.clone();
                continue;
            }
            let sig = self.db.compiled_signature(&class, name)?;
            return Some((sig.owner.clone(), Callee::Native(sig)));
        }
    }

    /// Every interface an object of a class implements: its own, its parents', and those each
    /// of them extends.
    pub fn implemented(&self, class: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut class = class.to_string();
        while let Some(bp) = self.blueprint(&class) {
            for interface in &bp.interfaces {
                if !out.contains(interface) {
                    out.push(interface.clone());
                }
            }
            class = bp.parent.clone();
        }
        for interface in self.db.interfaces_of(&class) {
            if !out.contains(&interface) {
                out.push(interface);
            }
        }
        out
    }

    /// Whether an interface, one of the program's or the engine's, declares a function.
    pub fn interface_declares(&self, interface: &str, name: &str) -> bool {
        match self.blueprint(interface) {
            Some(bp) => bp.functions.contains_key(name),
            None => self.db.interface_declares(interface, name),
        }
    }

    /// Whether a class is, or derives from, another.
    pub fn is_subclass(&self, class: &str, of: &str) -> bool {
        let mut class = class.to_string();
        loop {
            if class == of {
                return true;
            }
            match self.blueprint(&class) {
                Some(bp) => class = bp.parent.clone(),
                None => return self.db.is_subclass(&class, of),
            }
        }
    }

    /// The first class that is not one of the program's Blueprints, from this one up.
    pub fn native_ancestor(&self, class: &str) -> String {
        let mut class = class.to_string();
        while let Some(bp) = self.blueprint(&class) {
            class = bp.parent.clone();
        }
        class
    }

    /// How a source names a class: the Blueprint's name for one of the program's, the short name
    /// for the engine's or the game's when that names only it, else its path.
    pub fn class_name(&self, path: &str) -> Result<String> {
        if let Some(bp) = self.blueprint(path) {
            return Ok(bp.name.clone());
        }
        let scope = TypeScope { folder: "", uses: &[] };
        let short = short(path);
        if matches!(self.db.class_in(short, false, &scope), Ok(Some(found)) if found == path) && !self.shadowed(short) {
            return Ok(short.to_string());
        }
        self.by_path(path, matches!(self.db.class_in(path, true, &scope), Ok(Some(_))))
    }

    pub fn struct_name(&self, path: &str) -> Result<String> {
        if let Some(s) = self.user_struct(path) {
            return Ok(s.name.clone());
        }
        let scope = TypeScope { folder: "", uses: &[] };
        let short = short(path);
        if matches!(self.db.structure(short, &scope), Ok(Some(found)) if found == path) && !self.shadowed(short) {
            return Ok(short.to_string());
        }
        self.by_path(path, matches!(self.db.structure(path, &scope), Ok(Some(_))))
    }

    pub fn enum_name(&self, path: &str) -> Result<String> {
        if let Some(e) = self.user_enum(path) {
            return Ok(e.name.clone());
        }
        let scope = TypeScope { folder: "", uses: &[] };
        let short = short(path);
        if matches!(self.db.enumeration(short, &scope), Ok(Some(found)) if found == path) && !self.shadowed(short) {
            return Ok(short.to_string());
        }
        self.by_path(path, matches!(self.db.enumeration(path, &scope), Ok(Some(_))))
    }

    /// A type written by its path. A native one the project's types do not have, such as one from
    /// a plugin the mod's project had and this one does not, is one no source could name.
    fn by_path(&self, path: &str, known: bool) -> Result<String> {
        if !known && path.starts_with("/Script/") {
            missing!("{path}, which the project's types do not have, so the project lacks the plugin or module that declares it");
        }
        Ok(path.to_string())
    }

    /// Whether one of the program's own assets has this name, so the engine's of the same name
    /// must be written by path.
    fn shadowed(&self, short: &str) -> bool {
        self.blueprints.iter().any(|b| b.name == short)
            || self.structs.iter().any(|s| s.name == short)
            || self.enums.iter().any(|e| e.name == short)
    }

    /// A type as a source writes it, its names in backticks where they need them.
    pub fn type_name(&self, ty: &PropTy) -> Result<String> {
        Ok(match ty {
            PropTy::Bool => "bool".into(),
            PropTy::Int => "int".into(),
            PropTy::Int64 => "int64".into(),
            PropTy::Float => "float".into(),
            PropTy::Str => "string".into(),
            PropTy::Name => "name".into(),
            PropTy::Text => "text".into(),
            PropTy::Byte => cannot!("a byte, which loom has no type for"),
            PropTy::Object(c) | PropTy::Interface(c) => name(&self.class_name(c)?),
            PropTy::Class(c) => format!("class<{}>", name(&self.class_name(c)?)),
            PropTy::SoftObject(c) => format!("soft<{}>", name(&self.class_name(c)?)),
            PropTy::SoftClass(c) => format!("soft<class<{}>>", name(&self.class_name(c)?)),
            PropTy::Struct(s) => name(&self.struct_name(s)?),
            PropTy::Enum(e) => name(&self.enum_name(e)?),
            PropTy::Array(t) => format!("array<{}>", self.type_name(t)?),
            PropTy::Set(t) => format!("set<{}>", self.type_name(t)?),
            PropTy::Map(k, v) => format!("map<{}, {}>", self.type_name(k)?, self.type_name(v)?),
            PropTy::Delegate | PropTy::MulticastDelegate => cannot!("a variable that holds a delegate"),
        })
    }

    /// A value of a type as a literal, from the JSON of a class's defaults. None is the type's
    /// zero, which a variable need not be given.
    pub fn literal(&self, ty: &PropTy, v: &Value) -> Result<Option<Expr>> {
        let e = |kind| Some(Expr { kind, span: Span::default() });
        Ok(match (ty, v) {
            (_, Value::Null) => None,
            // A zero is what a value starts as, so it need not be written.
            (PropTy::Bool, Value::Bool(false)) => None,
            (PropTy::Int | PropTy::Int64 | PropTy::Float, Value::Number(n)) if n.as_f64() == Some(0.0) => None,
            (PropTy::Str | PropTy::Name, Value::String(s)) if s.is_empty() || (matches!(ty, PropTy::Name) && s == "None") => None,
            (PropTy::Bool, Value::Bool(b)) => e(ExprKind::Bool(*b)),
            (PropTy::Int | PropTy::Int64, Value::Number(n)) => e(ExprKind::Int(n.as_i64().ok_or_else(|| refusal!(Unexpected, "{n} where a whole number should be"))?)),
            (PropTy::Float, Value::Number(n)) => e(ExprKind::Float(n.as_f64().unwrap_or(0.0))),
            (PropTy::Str | PropTy::Name, Value::String(s)) => e(ExprKind::Str(s.clone())),
            (PropTy::Text, Value::Object(_)) => {
                let s = str_of(v, "SourceString")
                    .or_else(|| str_of(v, "CultureInvariantString"))
                    .or_else(|| v.pointer("/TextHistory/SourceString").and_then(Value::as_str))
                    .ok_or_else(|| refusal!(NotYet, "a text whose string is kept another way: {v}"))?;
                e(ExprKind::Str(s.to_string()))
            }
            (PropTy::Enum(path), Value::String(s)) => e(self.enum_value(path, s.rsplit("::").next().unwrap_or(s))?.kind),
            (PropTy::Object(_) | PropTy::Class(_) | PropTy::SoftObject(_) | PropTy::SoftClass(_), Value::Object(_)) => {
                let r = Ref::of(v)?;
                e(ExprKind::Path(r.path()))
            }
            (PropTy::SoftObject(_) | PropTy::SoftClass(_), Value::String(s)) if s.is_empty() => None,
            (PropTy::Struct(path), Value::Object(fields)) => {
                let mut made = Vec::new();
                for (cooked, value) in fields {
                    // CUE4Parse writes a colour's hex beside its components, which say it all.
                    if cooked == "Hex" && matches!(short(path), "LinearColor" | "Color") {
                        continue;
                    }
                    // A field at its zero is as the struct starts, so it is not written, even
                    // one only the editor has.
                    if is_zero(value) {
                        continue;
                    }
                    let (name, fty) = self.struct_field(path, cooked)?;
                    if let Some(lit) = self.literal(&fty, value)? {
                        made.push((name, lit));
                    }
                }
                // Every field at its zero is the struct's zero.
                let name = self.struct_name(path)?;
                if made.is_empty() { None } else { e(ExprKind::Make(name, made)) }
            }
            (PropTy::Array(inner) | PropTy::Set(inner), Value::Array(items)) => {
                if items.is_empty() {
                    return Ok(None);
                }
                let mut out = Vec::new();
                for item in items {
                    out.push(self.literal(inner, item)?.map_or_else(|| self.zero(inner), Ok)?);
                }
                e(ExprKind::ArrayLiteral(out))
            }
            (PropTy::Map(k, val), Value::Array(entries)) => {
                if entries.is_empty() {
                    return Ok(None);
                }
                let mut out = Vec::new();
                for entry in entries {
                    let key = entry.get("Key").ok_or_else(|| refusal!(Unexpected, "a map entry with no key"))?;
                    let value = entry.get("Value").ok_or_else(|| refusal!(Unexpected, "a map entry with no value"))?;
                    out.push((
                        self.literal(k, key)?.map_or_else(|| self.zero(k), Ok)?,
                        self.literal(val, value)?.map_or_else(|| self.zero(val), Ok)?,
                    ));
                }
                e(ExprKind::MapLiteral(out))
            }
            (ty, v) => not_yet!("a default of {}: {v}", self.type_name(ty).unwrap_or_else(|_| format!("{ty:?}"))),
        })
    }

    /// The zero of a type, written out, for a local that must be declared before it is set.
    pub fn zero(&self, ty: &PropTy) -> Result<Expr> {
        let kind = match ty {
            PropTy::Bool => ExprKind::Bool(false),
            PropTy::Int | PropTy::Int64 | PropTy::Byte => ExprKind::Int(0),
            PropTy::Float => ExprKind::Float(0.0),
            PropTy::Str | PropTy::Name | PropTy::Text => ExprKind::Str(String::new()),
            PropTy::Object(_) | PropTy::Class(_) | PropTy::SoftObject(_) | PropTy::SoftClass(_) | PropTy::Interface(_) => ExprKind::Null,
            PropTy::Enum(path) => ExprKind::Call(self.enum_name(path)?, vec![Expr { kind: ExprKind::Int(0), span: Span::default() }], false),
            PropTy::Struct(path) => ExprKind::Make(self.struct_name(path)?, Vec::new()),
            PropTy::Array(_) | PropTy::Set(_) => ExprKind::EmptyArray,
            PropTy::Map(..) => ExprKind::MapLiteral(Vec::new()),
            PropTy::Delegate | PropTy::MulticastDelegate => cannot!("the zero of a delegate"),
        };
        Ok(Expr { kind, span: Span::default() })
    }

    /// `Enum.Value`, for a value named as cooked.
    pub fn enum_value(&self, path: &str, cooked: &str) -> Result<Expr> {
        let e = |kind| Expr { kind, span: Span::default() };
        let enum_name = self.enum_name(path)?;
        let value = match self.user_enum(path) {
            Some(u) => u
                .values
                .iter()
                .find(|(c, _)| c == cooked)
                .map(|(_, n)| n.clone())
                .ok_or_else(|| refusal!(Unexpected, "{cooked}, which {} does not have", u.name))?,
            None => cooked.to_string(),
        };
        Ok(e(ExprKind::Member(Box::new(e(ExprKind::Name(enum_name))), value)))
    }

    /// A native struct's field as the project's types spell it. The engine's names ignore case,
    /// so the bytecode may spell one as the mod's project did, `Count` for `count`.
    pub fn field_spelling(&self, path: &str, cooked: &str) -> String {
        self.db
            .field_order(path)
            .and_then(|order| order.iter().find(|f| f.eq_ignore_ascii_case(cooked)))
            .cloned()
            .unwrap_or_else(|| cooked.to_string())
    }

    /// The value of an enum that stands for a number.
    pub fn enum_number(&self, path: &str, n: i64) -> Result<Expr> {
        let e = |kind| Expr { kind, span: Span::default() };
        let name = match self.user_enum(path) {
            Some(u) => u.values.get(n as usize).map(|(_, v)| v.clone()),
            None => self.db.enum_values(path).get(n as usize).cloned(),
        };
        match name {
            Some(value) => Ok(e(ExprKind::Member(Box::new(e(ExprKind::Name(self.enum_name(path)?))), value))),
            None => Ok(e(ExprKind::Call(self.enum_name(path)?, vec![e(ExprKind::Int(n))], false))),
        }
    }

    /// A field of a struct by its cooked name: its name in loom and its type.
    pub fn struct_field(&self, path: &str, cooked: &str) -> Result<(String, PropTy)> {
        if let Some(s) = self.user_struct(path) {
            return s
                .fields
                .iter()
                .find(|(c, _, _)| c == cooked)
                .map(|(_, n, t)| (n.clone(), t.clone()))
                .ok_or_else(|| refusal!(Unexpected, "{cooked}, which {} does not have", s.name));
        }
        let ty = self.db.editable_field(path, cooked).ok_or_else(|| refusal!(Unexpected, "{cooked}, which {} does not have", short(path)))?;
        Ok((cooked.to_string(), PropTy::from_ty(&ty)?))
    }

    /// A struct's fields' names in loom and types, in the order its compiled constants give them.
    pub fn struct_fields(&self, path: &str) -> Result<Vec<(String, PropTy)>> {
        if let Some(s) = self.user_struct(path) {
            return Ok(s.fields.iter().map(|(_, n, t)| (n.clone(), t.clone())).collect());
        }
        // A struct the project's types lack says so, rather than that there is no such struct.
        self.struct_name(path)?;
        let order = self.db.field_order(path).ok_or_else(|| refusal!(Unexpected, "{path} where a struct should be"))?;
        order
            .iter()
            .map(|name| {
                let ty = self.db.editable_field(path, name).ok_or_else(|| refusal!(Inexpressible, "{}'s field {name}", short(path)))?;
                Ok((name.clone(), PropTy::from_ty(&ty)?))
            })
            .collect()
    }
}

/// A function a call reaches.
#[derive(Debug, Clone)]
pub enum Callee {
    Lifted(Function),
    Native(crate::types::CompiledSignature),
}

impl PropTy {
    pub fn from_ty(ty: &crate::types::Ty) -> Result<PropTy> {
        use crate::types::Ty;
        Ok(match ty {
            Ty::Bool => PropTy::Bool,
            Ty::Byte => PropTy::Byte,
            Ty::Int => PropTy::Int,
            Ty::Int64 => PropTy::Int64,
            Ty::Float => PropTy::Float,
            Ty::Str => PropTy::Str,
            Ty::Name => PropTy::Name,
            Ty::Text => PropTy::Text,
            Ty::Object(p) => PropTy::Object(p.clone()),
            Ty::Class(p) => PropTy::Class(p.clone()),
            Ty::Struct(p) => PropTy::Struct(p.clone()),
            Ty::Enum(p) => PropTy::Enum(p.clone()),
            Ty::Array(t) => PropTy::Array(Box::new(PropTy::from_ty(t)?)),
            Ty::Set(t) => PropTy::Set(Box::new(PropTy::from_ty(t)?)),
            Ty::Map(k, v) => PropTy::Map(Box::new(PropTy::from_ty(k)?), Box::new(PropTy::from_ty(v)?)),
            Ty::SoftObject(p) => PropTy::SoftObject(p.clone()),
            Ty::SoftClass(p) => PropTy::SoftClass(p.clone()),
            Ty::Interface(p) => PropTy::Interface(p.clone()),
            Ty::Delegate(_) => PropTy::Delegate,
            Ty::Null | Ty::Record(_) => unexpected!("{ty} as a property's type"),
        })
    }

    /// A type as the type dump spells it.
    pub fn from_dump(s: &str) -> Result<PropTy> {
        let ty = crate::types::Ty::from_dump(s).ok_or_else(|| refusal!(NotYet, "a parameter of type {s}"))?;
        PropTy::from_ty(&ty)
    }
}

fn read_function(f: &Value) -> Result<Function> {
    let flags = str_of(f, "FunctionFlags").unwrap_or("");
    let flag = |x: &str| flags.split('|').any(|f| f.trim() == x);
    let mut params = Vec::new();
    for p in f.get("ChildProperties").and_then(Value::as_array).into_iter().flatten() {
        let prop = Prop::of(p)?;
        if !prop.has("Parm") {
            continue;
        }
        let dir = if prop.has("ReturnParm") || (prop.has("OutParm") && !prop.has("ReferenceParm")) {
            Dir::Out
        } else if prop.has("ReferenceParm") {
            Dir::Ref
        } else {
            Dir::In
        };
        let ret = prop.has("ReturnParm");
        params.push(Param { name: own_name(&prop.name)?, cooked: prop.name, ty: prop.ty, dir, ret });
    }
    let stub = read_stub(f)?;
    Ok(Function {
        name: own_name(str_of(f, "Name").unwrap_or_default())?,
        params,
        pure: flag("FUNC_BlueprintPure"),
        is_static: flag("FUNC_Static"),
        access: if flag("FUNC_Private") {
            Some("private")
        } else if flag("FUNC_Protected") && !flag("FUNC_Event") {
            Some("protected")
        } else {
            None
        },
        stub,
        overrides: f.get("SuperStruct").is_some_and(|s| !s.is_null()),
    })
}

/// An event's stub: parameters copied into the event graph's variables, then a call of the graph
/// at the event's entry point, then the end.
fn read_stub(f: &Value) -> Result<Option<Stub>> {
    let Some(code) = f.get("ScriptBytecode").and_then(Value::as_array) else { return Ok(None) };
    let mut copies = Vec::new();
    for s in code {
        match str_of(s, "Token") {
            Some("EX_LetValueOnPersistentFrame") => {
                let dest = s.pointer("/DestinationProperty/Property/Name").and_then(Value::as_str);
                let src = s.pointer("/AssignmentExpression/Variable/Property/Name").and_then(Value::as_str);
                match (dest, src) {
                    (Some(d), Some(p)) => copies.push((d.to_string(), p.to_string())),
                    _ => return Ok(None),
                }
            }
            Some("EX_LocalFinalFunction" | "EX_FinalFunction") => {
                let (_, name) = Ref::of(s.get("Function").unwrap_or(&Value::Null))?.function()?;
                if !name.starts_with("ExecuteUbergraph_") {
                    return Ok(None);
                }
                let entry = s
                    .pointer("/Parameters/0/Value")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| refusal!(Unexpected, "a call of the event graph with no entry point"))?;
                return Ok(Some(Stub { entry: entry as usize, copies }));
            }
            _ => return Ok(None),
        }
    }
    Ok(None)
}

/// Whether a cooked value is its type's zero: nothing, false, 0, an empty string or name, or a
/// struct, array or map of zeroes.
fn is_zero(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty() || s == "None",
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.values().all(is_zero),
    }
}

/// The last part of a path: `/Script/Engine.Actor` is `Actor`.
pub fn short(path: &str) -> &str {
    path.rsplit(['.', '/']).next().unwrap_or(path)
}

/// A name the asset has, which a source keeps as it is, in backticks where it needs them. It fails
/// for one no source can write, rather than renaming what other assets find by that name.
pub fn own_name(name: &str) -> Result<String> {
    if crate::syntax::written_name(name).is_none() {
        cannot!("the name {name:?}");
    }
    if name.starts_with(crate::syntax::RESERVED_PREFIX) {
        cannot!("the name {name}, since loom keeps names that start with {} for names it makes itself", crate::syntax::RESERVED_PREFIX);
    }
    Ok(name.to_string())
}

/// A name the lifter makes up, for one of the compiler's locals, from words: letters, digits and
/// underscores, not starting with a digit, and not a reserved word. `Get Localized Option` is
/// `GetLocalizedOption`.
pub fn invented(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(if upper { c.to_ascii_uppercase() } else { c });
            upper = false;
        } else {
            upper = !out.is_empty();
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.starts_with(crate::syntax::RESERVED_PREFIX) {
        out.insert(0, '_');
    }
    if crate::syntax::is_reserved(&out) {
        out.push('_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_up_names() {
        assert_eq!(invented("Get Localized Option"), "GetLocalizedOption");
        assert_eq!(invented("Options Values"), "OptionsValues");
        assert_eq!(invented("if"), "if_");
        assert_eq!(invented("in"), "in");
        assert_eq!(invented("2x"), "_2x");
        assert_eq!(invented("a-b"), "aB");
    }
}
