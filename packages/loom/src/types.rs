//! The types a program can use: the language's own, and the classes, structs and enums of the
//! project it compiles for, read from the dump the plugin writes to `Intermediate/Loom/types.json`.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;

use anyhow::{bail, Result};
use serde::Deserialize;

use crate::ops;

/// The class every interface derives from.
pub const INTERFACE: &str = "/Script/CoreUObject.Interface";
/// The hidden parameter through which a latent function learns where to resume.
const LATENT_ACTION_INFO: &str = "struct:/Script/Engine.LatentActionInfo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Bool,
    Byte,
    Int,
    Int64,
    Float,
    Str,
    Name,
    Text,
    /// A reference to an object of this class or a subclass.
    Object(String),
    /// A class: this one or a subclass.
    Class(String),
    Struct(String),
    Enum(String),
    Array(Box<Ty>),
    /// Each value at most once, in no order.
    Set(Box<Ty>),
    /// A map from keys of the first type to values of the second.
    Map(Box<Ty>, Box<Ty>),
    /// A soft reference to an object of this class: the asset's path, loaded when asked.
    SoftObject(String),
    /// A soft reference to a class, this one or a subclass.
    SoftClass(String),
    /// An object that implements this interface, or none.
    Interface(String),
    /// A single-cast delegate: an object and one of its functions, which takes what this
    /// signature, a function of the engine's or of a class, says. Blueprint can make one, hold it
    /// in a parameter and pass it on, but not call it.
    Delegate(String),
    /// `none`, which any object or class reference can hold.
    Null,
    /// The outputs of a call that has more than one, read by name like members.
    Record(Vec<(String, Ty)>),
}

impl Ty {
    /// Reads a type as the dump spells it. Returns None for kinds the language cannot use yet,
    /// such as dispatchers, so a member of such a type is simply not available.
    pub fn from_dump(s: &str) -> Option<Ty> {
        Some(match s {
            "bool" => Ty::Bool,
            "byte" => Ty::Byte,
            "int" => Ty::Int,
            "int64" => Ty::Int64,
            "float" | "float32" => Ty::Float,
            "string" => Ty::Str,
            "name" => Ty::Name,
            "text" => Ty::Text,
            _ => {
                if let Some(inner) = s.strip_prefix("array<").and_then(|r| r.strip_suffix('>')) {
                    return Some(Ty::Array(Box::new(Ty::from_dump(inner)?)));
                }
                if let Some(inner) = s.strip_prefix("set<").and_then(|r| r.strip_suffix('>')) {
                    return Some(Ty::Set(Box::new(Ty::from_dump(inner)?)));
                }
                if let Some(inner) = s.strip_prefix("map<").and_then(|r| r.strip_suffix('>')) {
                    let (key, value) = split_pair(inner)?;
                    return Some(Ty::Map(Box::new(Ty::from_dump(key)?), Box::new(Ty::from_dump(value)?)));
                }
                let (kind, path) = s.split_once(':')?;
                let path = path.to_string();
                match kind {
                    "object" => Ty::Object(path),
                    "class" => Ty::Class(path),
                    "struct" => Ty::Struct(path),
                    "enum" => Ty::Enum(path),
                    "softobject" => Ty::SoftObject(path),
                    "softclass" => Ty::SoftClass(path),
                    "interface" => Ty::Interface(path),
                    "delegate" => Ty::Delegate(path),
                    _ => return None,
                }
            }
        })
    }

    /// Whether this is a delegate, or holds one.
    pub fn has_delegate(&self) -> bool {
        match self {
            Ty::Delegate(_) => true,
            Ty::Array(t) | Ty::Set(t) => t.has_delegate(),
            Ty::Map(k, v) => k.has_delegate() || v.has_delegate(),
            _ => false,
        }
    }

    /// The type as ops.json spells it for a variable, local or parameter.
    pub fn op_type(&self) -> Result<ops::Type> {
        Ok(match self {
            Ty::Bool => ops::Type::Bool,
            Ty::Int => ops::Type::Int,
            Ty::Int64 => ops::Type::Int64,
            Ty::Float => ops::Type::Float,
            Ty::Str => ops::Type::String,
            Ty::Name => ops::Type::Name,
            Ty::Text => ops::Type::Text,
            Ty::Object(path) => ops::Type::Object(path.clone()),
            Ty::Class(path) => ops::Type::Class(path.clone()),
            Ty::Struct(path) => ops::Type::Struct(path.clone()),
            Ty::Enum(path) => ops::Type::Enum(path.clone()),
            Ty::Array(inner) => ops::Type::Array(Box::new(inner.op_type()?)),
            Ty::Set(inner) => ops::Type::Set(Box::new(inner.op_type()?)),
            Ty::Map(key, value) => ops::Type::Map(Box::new(key.op_type()?), Box::new(value.op_type()?)),
            Ty::SoftObject(path) => ops::Type::SoftObject(path.clone()),
            Ty::SoftClass(path) => ops::Type::SoftClass(path.clone()),
            Ty::Interface(path) => ops::Type::Interface(path.clone()),
            Ty::Delegate(path) => ops::Type::Delegate(path.clone()),
            other => bail!("a variable cannot be {other} yet"),
        })
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short = |p: &str| p.rsplit(['.', '/']).next().unwrap_or(p).to_string();
        match self {
            Ty::Bool => f.write_str("bool"),
            Ty::Byte => f.write_str("byte"),
            Ty::Int => f.write_str("int"),
            Ty::Int64 => f.write_str("int64"),
            Ty::Float => f.write_str("float"),
            Ty::Str => f.write_str("string"),
            Ty::Name => f.write_str("name"),
            Ty::Text => f.write_str("text"),
            Ty::Object(p) => write!(f, "{}", short(p)),
            Ty::Class(p) => write!(f, "class of {}", short(p)),
            Ty::Struct(p) => write!(f, "{}", short(p)),
            Ty::Enum(p) => write!(f, "{}", short(p)),
            Ty::Array(t) => write!(f, "array of {t}"),
            Ty::Set(t) => write!(f, "set of {t}"),
            Ty::Map(k, v) => write!(f, "map from {k} to {v}"),
            Ty::SoftObject(p) => write!(f, "soft reference to {}", short(p)),
            Ty::SoftClass(p) => write!(f, "soft reference to class of {}", short(p)),
            Ty::Interface(p) => write!(f, "{}", short(p)),
            Ty::Delegate(p) => f.write_str(delegate_name(p)),
            Ty::Null => f.write_str("none"),
            Ty::Record(fields) => {
                let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                write!(f, "the outputs ({})", names.join(", "))
            }
        }
    }
}

impl Ty {
    /// The type as the dump spells it, for classes this compile declares itself.
    fn to_dump(&self) -> String {
        match self {
            Ty::Bool => "bool".into(),
            Ty::Byte => "byte".into(),
            Ty::Int => "int".into(),
            Ty::Int64 => "int64".into(),
            Ty::Float => "float".into(),
            Ty::Str => "string".into(),
            Ty::Name => "name".into(),
            Ty::Text => "text".into(),
            Ty::Object(p) => format!("object:{p}"),
            Ty::Class(p) => format!("class:{p}"),
            Ty::Struct(p) => format!("struct:{p}"),
            Ty::Enum(p) => format!("enum:{p}"),
            Ty::Array(t) => format!("array<{}>", t.to_dump()),
            Ty::Set(t) => format!("set<{}>", t.to_dump()),
            Ty::Map(k, v) => format!("map<{},{}>", k.to_dump(), v.to_dump()),
            Ty::SoftObject(p) => format!("softobject:{p}"),
            Ty::SoftClass(p) => format!("softclass:{p}"),
            Ty::Interface(p) => format!("interface:{p}"),
            Ty::Delegate(p) => format!("delegate:{p}"),
            Ty::Null | Ty::Record(_) => unreachable!("not the type of a member"),
        }
    }
}

/// The name a delegate's signature goes by, as source names it: TimerDynamicDelegate for
/// /Script/Engine.TimerDynamicDelegate__DelegateSignature, which C++ calls FTimerDynamicDelegate.
pub fn delegate_name(path: &str) -> &str {
    let name = path.rsplit(['.', ':']).next().unwrap_or(path);
    name.strip_suffix("__DelegateSignature").unwrap_or(name)
}

/// How a function's parameter is passed, which a function bound to a delegate must match for
/// each: a value, a reference (const or not, which Blueprint does not tell apart here), an output,
/// or the return value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passing {
    Value,
    Ref,
    Out,
    Return,
}

/// Whether a parameter's floats are 32-bit, as many of the engine's are, where loom's and
/// Blueprint's own are 64-bit. Its type is `float` for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Floats {
    /// No 32-bit float: a 64-bit one, or none at all.
    Double,
    /// A 32-bit float, the parameter itself.
    Single,
    /// A 32-bit float inside a container.
    SingleInside,
}

impl Floats {
    fn of_dump(ty: &str) -> Floats {
        match ty {
            "float32" => Floats::Single,
            _ if ty.contains("float32") => Floats::SingleInside,
            _ => Floats::Double,
        }
    }
}

/// A parameter of a function or of a delegate's signature, as a match between them compares it.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub name: String,
    pub ty: Ty,
    pub passing: Passing,
    pub floats: Floats,
}

impl Passing {
    /// How the dump's `dir` and `const_ref` say a parameter is passed.
    fn from_dump(dir: &str, const_ref: bool) -> Passing {
        match dir {
            "return" => Passing::Return,
            "ref" => Passing::Ref,
            "out" => Passing::Out,
            _ if const_ref => Passing::Ref,
            _ => Passing::Value,
        }
    }

    /// How a Blueprint passes a parameter it declares: an array always by reference, as its
    /// compiler makes one, and anything else by reference only when asked.
    pub fn of_blueprint(ty: &Ty, by_ref: bool) -> Passing {
        if by_ref || matches!(ty, Ty::Array(_)) { Passing::Ref } else { Passing::Value }
    }
}

#[derive(Debug, Deserialize)]
struct Dump {
    classes: HashMap<String, ClassDump>,
    #[serde(default)]
    structs: HashMap<String, StructDump>,
    /// Each enum's value names.
    #[serde(default)]
    enums: HashMap<String, Vec<String>>,
    /// The project's Loom settings. A dump without them, from an older plugin, has the defaults.
    #[serde(default)]
    settings: Settings,
}

/// Where a source looks for a type it names by its short name: its own folder, and the folders it
/// uses, in the order written.
pub struct TypeScope<'a> {
    pub folder: &'a str,
    pub uses: &'a [String],
}

/// The folder a type is in: /Game/X for /Game/X/BP_A.BP_A_C.
fn folder_of(path: &str) -> &str {
    let package = path.split_once('.').map_or(path, |(package, _)| package);
    package.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Whether a folder is another or one inside it, as whole folders: /Game/Lib/UI is under
/// /Game/Lib, and /Game/Library is not.
fn is_under(folder: &str, outer: &str) -> bool {
    folder == outer || folder.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/'))
}

/// The project's Loom settings, from Project Settings > Plugins > Loom.
#[derive(Debug, Default, Deserialize)]
struct Settings {
    /// The paths whose Blueprint classes a Blueprint may hold as components and widgets. Empty
    /// allows any class.
    #[serde(default)]
    instanced_class_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct StructDump {
    #[serde(default)]
    fields: Fields,
    native_make: Option<String>,
    native_break: Option<String>,
}

/// A struct's fields by name, and the order the dump lists them in, which is the order a struct's
/// compiled constant gives their values in.
#[derive(Debug, Default)]
struct Fields {
    by_name: HashMap<String, PropertyDump>,
    order: Vec<String>,
}

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Fields;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of fields")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Fields, A::Error> {
                let mut fields = Fields::default();
                while let Some((name, field)) = map.next_entry::<String, PropertyDump>()? {
                    fields.order.push(name.clone());
                    fields.by_name.insert(name, field);
                }
                Ok(fields)
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[derive(Debug, Default, Deserialize)]
struct ClassDump {
    #[serde(rename = "super")]
    super_class: Option<String>,
    #[serde(default)]
    editor_only: bool,
    /// A panel widget's: the class of the slot that holds each of its children.
    #[serde(default)]
    slot: Option<String>,
    /// A panel widget that holds at most one child, such as a button.
    #[serde(default)]
    one_child: bool,
    #[serde(default)]
    interface: bool,
    /// An interface a Blueprint cannot implement.
    #[serde(default)]
    not_implementable: bool,
    /// The interfaces the class implements itself, not counting its parents'.
    #[serde(default)]
    interfaces: Vec<String>,
    #[serde(default)]
    properties: HashMap<String, PropertyDump>,
    #[serde(default)]
    functions: HashMap<String, FunctionDump>,
}

#[derive(Debug, Deserialize)]
struct PropertyDump {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    read_only: bool,
    /// Set only in the Details panel, so a literal may give it a value but code cannot reach it.
    #[serde(default)]
    edit_only: bool,
    /// A dispatcher's or a delegate's parameters, which a function bound to it is passed.
    #[serde(default)]
    signature: Option<Vec<SignatureParam>>,
    /// Only its own class can read or set it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    private: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SignatureParam {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    /// How it is passed. A dump from an older plugin says nothing, which was only ever a
    /// dispatcher's, passed by value.
    #[serde(default = "dir_in")]
    dir: String,
    #[serde(default)]
    const_ref: bool,
}

fn dir_in() -> String {
    "in".into()
}

#[derive(Debug, Default, Deserialize)]
struct FunctionDump {
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    params: Vec<ParamDump>,
    world_context: Option<String>,
    default_to_self: Option<String>,
    hide_pin: Option<String>,
    determines_output_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ParamDump {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    dir: String,
    default: Option<String>,
    #[serde(default)]
    wired: bool,
    /// An input passed by const reference, which is an input like any other except to a delegate.
    #[serde(default)]
    const_ref: bool,
    /// A delegate's signature.
    #[serde(default)]
    signature: Option<Vec<SignatureParam>>,
    /// For a function of an array, a set or a map, what the engine's metadata ties the
    /// parameter's type to. The dump writes such a type with `int` standing for what it holds.
    #[serde(default)]
    generic: Option<Generic>,
}

/// What a container function's parameter is typed by, as its metadata says: `ArrayParm`,
/// `SetParam` or `MapParam` name the containers, `ArrayTypeDependentParams` and a `SetParam`'s
/// names after its `|` what they hold, and `MapKeyParam` and `MapValueParam` a map's keys and
/// values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Generic {
    Container,
    Item,
    Key,
    Value,
}

/// The library whose functions are a container's methods, and the prefix of their names.
fn container_library(on: &Ty) -> Option<(&'static str, &'static str)> {
    match on {
        Ty::Array(_) => Some((crate::builtins::ARRAY, "Array_")),
        Ty::Set(_) => Some((crate::builtins::SET, "Set_")),
        Ty::Map(..) => Some((crate::builtins::MAP, "Map_")),
        _ => None,
    }
}

/// A container function's type, for a container of `on`: the container itself, or, with the
/// `int` that stands for what it holds, what `on` holds.
fn fit(ty: &Ty, role: Generic, on: &Ty) -> Ty {
    let held = match (role, on) {
        (Generic::Container, _) => return on.clone(),
        (Generic::Item, Ty::Array(item) | Ty::Set(item)) | (Generic::Key, Ty::Map(item, _)) | (Generic::Value, Ty::Map(_, item)) => (**item).clone(),
        _ => return ty.clone(),
    };
    fn put(ty: &Ty, held: &Ty) -> Ty {
        match ty {
            Ty::Int => held.clone(),
            Ty::Array(inner) => Ty::Array(Box::new(put(inner, held))),
            Ty::Set(inner) => Ty::Set(Box::new(put(inner, held))),
            other => other.clone(),
        }
    }
    put(ty, &held)
}

/// A function as its compiled calls pass it arguments: every parameter but the return value, in
/// order.
#[derive(Debug, Clone)]
pub struct CompiledSignature {
    /// The class that declares it.
    pub owner: String,
    pub params: Vec<CompiledParam>,
    pub has_return: bool,
    pub pure: bool,
    pub is_static: bool,
    pub latent: bool,
}

#[derive(Debug, Clone)]
pub struct CompiledParam {
    pub name: String,
    /// "in", "ref" or "out".
    pub dir: String,
    /// The type as the dump spells it.
    pub ty: String,
    /// A parameter the node fills itself, which loom's calls leave out: the world context, a
    /// hidden pin, or a latent function's LatentActionInfo.
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub ty: Ty,
    pub read_only: bool,
    /// Set only in the Details panel: a literal may give it a value, code cannot reach it.
    pub edit_only: bool,
    /// Only its own class can read or set it.
    pub private: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    /// The pin fills itself when no argument is given: it has a default, or the node fills it.
    pub optional: bool,
    /// The function works on the caller's variable itself, so the argument must be one.
    pub by_ref: bool,
    /// The pin takes nothing written out on it: something must be wired to it.
    pub wired: bool,
}

/// A function a Blueprint in this compile declares.
#[derive(Debug, Clone)]
pub struct Declared {
    pub name: String,
    /// Each parameter's name, type, and whether it is passed by reference.
    pub params: Vec<(String, Ty, bool)>,
    /// Each parameter's default, as text, which makes it one a call may leave out.
    pub defaults: Vec<Option<String>>,
    pub ret: Option<Ty>,
    /// Its `out` parameters, which a call gives as a record, with the return value.
    pub outputs: Vec<(String, Ty)>,
    /// A function library's functions are static: they are called on the class, not an object.
    pub is_static: bool,
    /// A pure node, with no exec pins.
    pub pure: bool,
    /// "private" or "protected", when it is either.
    pub access: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Function {
    /// The class that declares it, as ops.json names it in a call.
    pub owner: String,
    pub name: String,
    pub pure: bool,
    pub is_static: bool,
    pub params: Vec<Param>,
    pub ret: Option<Ty>,
    /// Output parameters, which the call node shows as further output pins.
    pub outputs: Vec<(String, Ty)>,
    /// The parameter whose class argument decides the return type.
    pub determines_output_type: Option<String>,
    /// Finishes later, such as Delay, so it can only be called where the Blueprint can wait for
    /// it: in the event graph.
    pub latent: bool,
    /// "private" or "protected", when it is either: who may call it besides the class that
    /// declares it.
    pub access: Option<&'static str>,
    pub deprecated: bool,
    /// Every parameter, hidden or not, and the return value, in order, as a delegate it is bound
    /// to must pass them.
    pub shape: Vec<Slot>,
}

/// An event a class or an interface declares, which a Blueprint of that class, or one that
/// implements the interface, may handle.
pub struct Event {
    pub owner: String,
    pub params: Vec<(String, Ty)>,
    /// What a handler returns.
    pub ret: Option<Ty>,
    /// What a handler gives back besides, by name, as an interface's function's outputs.
    pub outputs: Vec<(String, Ty)>,
    /// Handled by overriding it as a function, as an event that returns a value or is const is,
    /// rather than by an event node.
    pub function: bool,
}

pub struct TypeDb {
    dump: Dump,
    /// Short class names to their paths, editor-only classes included.
    short: HashMap<String, Vec<String>>,
    /// The same for structs.
    short_structs: HashMap<String, Vec<String>>,
    /// The same for enums.
    short_enums: HashMap<String, Vec<String>>,
    /// Each delegate's signature by its path, from wherever the types name it.
    signatures: HashMap<String, Vec<SignatureParam>>,
    /// The same for delegates, by the names their signatures go by.
    short_delegates: HashMap<String, Vec<String>>,
    /// Every name or path looked up as a class, struct or enum and not found, for whatever loaded
    /// the types to load more.
    missing: RefCell<BTreeSet<String>>,
}

impl TypeDb {
    pub fn parse(text: &str) -> Result<TypeDb> {
        let dump: Dump = serde_json::from_str(text)?;
        let mut short: HashMap<String, Vec<String>> = HashMap::new();
        for path in dump.classes.keys() {
            if let Some((package, name)) = path.rsplit_once('.') {
                // A Blueprint's class, /Game/X/Name.Name_C, is called by the Blueprint's name.
                let asset = package.rsplit('/').next().unwrap_or(package);
                let name = name.strip_suffix("_C").filter(|stem| *stem == asset).unwrap_or(name);
                short.entry(name.to_string()).or_default().push(path.clone());
            }
        }
        let mut short_structs: HashMap<String, Vec<String>> = HashMap::new();
        for path in dump.structs.keys() {
            if let Some((_, name)) = path.rsplit_once('.') {
                short_structs.entry(name.to_string()).or_default().push(path.clone());
            }
        }
        let mut short_enums: HashMap<String, Vec<String>> = HashMap::new();
        for path in dump.enums.keys() {
            if let Some((_, name)) = path.rsplit_once('.') {
                short_enums.entry(name.to_string()).or_default().push(path.clone());
            }
        }
        // A delegate's signature is dumped wherever a property or a parameter has its type.
        let mut signatures: HashMap<String, Vec<SignatureParam>> = HashMap::new();
        let mut note = |ty: &str, signature: &Option<Vec<SignatureParam>>| {
            if let (Some(path), Some(signature)) = (ty.strip_prefix("delegate:"), signature) {
                signatures.entry(path.to_string()).or_insert_with(|| signature.clone());
            }
        };
        for class in dump.classes.values() {
            for property in class.properties.values() {
                note(&property.ty, &property.signature);
            }
            for function in class.functions.values() {
                for param in &function.params {
                    note(&param.ty, &param.signature);
                }
            }
        }
        for structure in dump.structs.values() {
            for field in structure.fields.by_name.values() {
                note(&field.ty, &field.signature);
            }
        }
        let mut short_delegates: HashMap<String, Vec<String>> = HashMap::new();
        for path in signatures.keys() {
            short_delegates.entry(delegate_name(path).to_string()).or_default().push(path.clone());
        }
        Ok(TypeDb { dump, short, short_structs, short_enums, signatures, short_delegates, missing: RefCell::default() })
    }

    /// A delegate by its signature's path, or by the name its signature goes by when that names
    /// exactly one.
    pub fn delegate(&self, name: &str, scope: &TypeScope) -> Result<Option<String>> {
        if name.starts_with('/') {
            return Ok(self.found(name, self.signatures.contains_key(name).then(|| name.to_string())));
        }
        let found: Vec<&String> = self.short_delegates.get(name).map(|paths| paths.iter().collect()).unwrap_or_default();
        self.pick(name, &found, scope)
    }

    /// What a delegate's signature passes a function bound to it, and how.
    pub fn signature(&self, path: &str) -> Result<Vec<Slot>> {
        let Some(signature) = self.signatures.get(path) else {
            bail!("the types have no signature for {}; is the dump from an older plugin?", delegate_name(path));
        };
        slots(delegate_name(path), signature)
    }

    /// Whether a Blueprint may hold an instance of a class, as a component or a widget: a native
    /// class always, and a Blueprint class when the project's InstancedClassPaths are empty or one
    /// of them holds it. A path holds what is under it as a whole folder: /Game/Lib holds
    /// /Game/Lib/X, not /Game/Library/X.
    pub fn may_instance(&self, class: &str) -> bool {
        let paths = &self.dump.settings.instanced_class_paths;
        class.starts_with("/Script/")
            || paths.is_empty()
            || paths.iter().any(|path| class.starts_with(&format!("{}/", path.trim_end_matches('/'))))
    }

    /// The project's InstancedClassPaths, as its settings list them.
    pub fn instanced_class_paths(&self) -> &[String] {
        &self.dump.settings.instanced_class_paths
    }

    /// Adds a Blueprint this compile declares, so other Blueprints in it can use it.
    pub fn add_class(
        &mut self,
        path: &str,
        super_class: &str,
        properties: &[(String, Ty, bool)],
        functions: &[Declared],
        dispatchers: &[(String, Vec<(String, Ty)>)],
        interfaces: &[String],
    ) {
        // A class whose parent runs only in the editor runs only in the editor too.
        let editor_only = self.is_editor_only(super_class);
        let interfaces = interfaces.to_vec();
        let mut class = ClassDump { super_class: Some(super_class.to_string()), editor_only, interfaces, ..Default::default() };
        for (name, ty, private) in properties {
            let dump = PropertyDump { ty: ty.to_dump(), read_only: false, edit_only: false, signature: None, private: *private };
            class.properties.insert(name.clone(), dump);
        }
        for (name, params) in dispatchers {
            let signature = params
                .iter()
                .map(|(n, t)| {
                    let dir = if Passing::of_blueprint(t, false) == Passing::Ref { "ref" } else { "in" };
                    SignatureParam { name: n.clone(), ty: t.to_dump(), dir: dir.into(), const_ref: false }
                })
                .collect();
            let dump = PropertyDump { ty: "multicast_delegate".into(), read_only: false, edit_only: false, signature: Some(signature), private: false };
            class.properties.insert(name.clone(), dump);
        }
        for function in functions {
            let mut dump = FunctionDump::default();
            if function.is_static {
                dump.flags.push("static".into());
            }
            if function.pure {
                dump.flags.push("pure".into());
            }
            dump.flags.extend(function.access.map(String::from));
            for ((param, ty, by_ref), default) in function.params.iter().zip(&function.defaults) {
                let dir = if *by_ref { "ref" } else { "in" };
                dump.params.push(ParamDump { name: param.clone(), ty: ty.to_dump(), dir: dir.into(), default: default.clone(), wired: false, const_ref: false, signature: None, generic: None });
            }
            for (name, ty) in &function.outputs {
                dump.params.push(ParamDump { name: name.clone(), ty: ty.to_dump(), dir: "out".into(), default: None, wired: false, const_ref: false, signature: None, generic: None });
            }
            if let Some(ret) = &function.ret {
                dump.params.push(ParamDump { name: "ReturnValue".into(), ty: ret.to_dump(), dir: "return".into(), default: None, wired: false, const_ref: false, signature: None, generic: None });
            }
            class.functions.insert(function.name.clone(), dump);
        }
        self.dump.classes.insert(path.to_string(), class);
        if let Some((_, name)) = path.rsplit_once('.') {
            // A Blueprint's class is Name_C, but source calls it by the Blueprint's name.
            let name = name.strip_suffix("_C").unwrap_or(name);
            // Added to the name's list, so two Blueprints of one name in different folders are both
            // seen, and a lookup can prefer the one in its own folder or refuse to pick.
            let paths = self.short.entry(name.to_string()).or_default();
            if !paths.iter().any(|p| p == path) {
                paths.push(path.to_string());
            }
        }
    }

    /// Adds an interface a source declares, with its functions, each an event a Blueprint that
    /// implements it may handle.
    pub fn add_interface(&mut self, path: &str, functions: &[Declared]) {
        self.add_class(path, INTERFACE, &[], functions, &[], &[]);
        let class = self.dump.classes.get_mut(path).expect("just added");
        class.interface = true;
        for function in class.functions.values_mut() {
            function.flags.push("event".into());
        }
    }

    /// Adds a struct a source declares, with its fields.
    pub fn add_struct(&mut self, path: &str, fields: &[(String, Ty)]) {
        let fields = Fields {
            by_name: fields
                .iter()
                .map(|(name, ty)| (name.clone(), PropertyDump { ty: ty.to_dump(), read_only: false, edit_only: false, signature: None, private: false }))
                .collect(),
            order: fields.iter().map(|(name, _)| name.clone()).collect(),
        };
        self.dump.structs.insert(path.to_string(), StructDump { fields, native_make: None, native_break: None });
        if let Some((_, name)) = path.rsplit_once('.') {
            let paths = self.short_structs.entry(name.to_string()).or_default();
            if !paths.iter().any(|p| p == path) {
                paths.push(path.to_string());
            }
        }
    }

    /// Adds an enum a source declares, with its values' names.
    pub fn add_enum(&mut self, path: &str, values: Vec<String>) {
        self.dump.enums.insert(path.to_string(), values);
        if let Some((_, name)) = path.rsplit_once('.') {
            let paths = self.short_enums.entry(name.to_string()).or_default();
            if !paths.iter().any(|p| p == path) {
                paths.push(path.to_string());
            }
        }
    }

    /// A struct by path, or by its short name when that names exactly one.
    pub fn structure(&self, name: &str, scope: &TypeScope) -> Result<Option<String>> {
        if name.starts_with('/') {
            return Ok(self.found(name, self.dump.structs.contains_key(name).then(|| name.to_string())));
        }
        let found: Vec<&String> = self.short_structs.get(name).map(|paths| paths.iter().collect()).unwrap_or_default();
        self.pick(name, &found, scope)
    }

    /// An enum's values, by name.
    pub fn enum_values(&self, path: &str) -> &[String] {
        self.dump.enums.get(path).map(Vec::as_slice).unwrap_or_default()
    }

    /// An enum by path, or by its short name when that names exactly one.
    pub fn enumeration(&self, name: &str, scope: &TypeScope) -> Result<Option<String>> {
        if name.starts_with('/') {
            return Ok(self.found(name, self.dump.enums.contains_key(name).then(|| name.to_string())));
        }
        let found: Vec<&String> = self.short_enums.get(name).map(|paths| paths.iter().collect()).unwrap_or_default();
        self.pick(name, &found, scope)
    }

    /// The functions that make and break a struct which refuses the generic nodes, such as
    /// Vector2D, as (class path, function name).
    pub fn native_make(&self, structure: &str) -> Option<(String, String)> {
        split_function(self.dump.structs.get(structure)?.native_make.as_deref()?)
    }

    pub fn native_break(&self, structure: &str) -> Option<(String, String)> {
        split_function(self.dump.structs.get(structure)?.native_break.as_deref()?)
    }

    /// A field of a struct that code can read or write.
    pub fn field(&self, structure: &str, name: &str) -> Option<Ty> {
        let field = self.dump.structs.get(structure)?.fields.by_name.get(name)?;
        if field.edit_only {
            return None;
        }
        Ty::from_dump(&field.ty)
    }

    /// Whether code may set a field of a struct: a Make Struct or a Set Members node has no pin
    /// for a read-only one.
    pub fn field_read_only(&self, structure: &str, name: &str) -> bool {
        self.dump.structs.get(structure).and_then(|s| s.fields.by_name.get(name)).is_some_and(|f| f.read_only)
    }

    /// A struct's fields, in the order they are declared.
    pub fn field_order(&self, structure: &str) -> Option<&[String]> {
        Some(&self.dump.structs.get(structure)?.fields.order)
    }

    /// A field a literal can set, including one only the Details panel shows.
    pub fn editable_field(&self, structure: &str, name: &str) -> Option<Ty> {
        Ty::from_dump(&self.dump.structs.get(structure)?.fields.by_name.get(name)?.ty)
    }

    /// A class by path, or by its short name, as `scope` finds it. Outside an editor Blueprint, a
    /// class that runs only in the editor is refused, since the game does not have it.
    pub fn class_in(&self, name: &str, editor: bool, scope: &TypeScope) -> Result<Option<String>> {
        let refused = |path: &str| -> Result<Option<String>> {
            bail!("{} runs only in the editor, so the game does not have it; only an `editor blueprint` can use it", path.rsplit('.').next().unwrap_or(path))
        };
        if name.starts_with('/') {
            return match self.dump.classes.get(name) {
                Some(_) if !editor && self.is_editor_only(name) => refused(name),
                Some(_) => Ok(Some(name.to_string())),
                None => Ok(self.found(name, None)),
            };
        }
        let found = self.short.get(name).map(Vec::as_slice).unwrap_or_default();
        let usable: Vec<&String> = found.iter().filter(|p| editor || !self.is_editor_only(p)).collect();
        match (usable.as_slice(), found.first()) {
            ([], Some(path)) => refused(path),
            _ => self.pick(name, &usable, scope),
        }
    }

    /// Which of the types of one short name a source means: the one in its own folder, then the
    /// one under the first folder it uses that has one, then the only one there is. Two at any of
    /// those steps is ambiguous.
    fn pick(&self, name: &str, found: &[&String], scope: &TypeScope) -> Result<Option<String>> {
        let ambiguous = |many: &[&String]| -> Result<Option<String>> {
            let paths: Vec<&str> = many.iter().map(|p| p.as_str()).collect();
            bail!("{name} is ambiguous; write its path, one of {}", paths.join(", "))
        };
        if let Some(own) = found.iter().find(|p| folder_of(p) == scope.folder) {
            return Ok(Some((*own).clone()));
        }
        for used in scope.uses {
            let under: Vec<&String> = found.iter().copied().filter(|p| is_under(folder_of(p), used)).collect();
            match under.as_slice() {
                [] => continue,
                [one] => return Ok(Some((*one).clone())),
                many => return ambiguous(many),
            }
        }
        match found {
            [] => Ok(self.found(name, None)),
            [one] => Ok(Some((*one).clone())),
            many => ambiguous(many),
        }
    }

    /// Whether any class, struct or enum the types have is under a folder.
    pub fn holds_anything_under(&self, folder: &str) -> bool {
        let dump = &self.dump;
        dump.classes.keys().chain(dump.structs.keys()).chain(dump.enums.keys()).any(|path| is_under(folder_of(path), folder))
    }

    /// Notes a folder of types a source needs and the types don't have, for whatever loaded them
    /// to load, written with a trailing slash.
    pub fn note_missing_folder(&self, folder: &str) {
        self.missing.borrow_mut().insert(format!("{folder}/"));
    }

    /// What a lookup found, noting a name that found nothing.
    fn found(&self, name: &str, found: Option<String>) -> Option<String> {
        if found.is_none() {
            self.missing.borrow_mut().insert(name.to_string());
        }
        found
    }

    /// Every name or path looked up as a class, struct or enum that the types don't have. Some are
    /// names that turned out to be something else, such as a variable.
    pub fn missing(&self) -> Vec<String> {
        self.missing.borrow().iter().cloned().collect()
    }

    /// Whether a class runs only in the editor, as any class does whose parent does.
    pub fn is_editor_only(&self, class: &str) -> bool {
        self.lineage(class).any(|(_, c)| c.editor_only)
    }

    /// A dispatcher of a class: the parameters a handler bound to it is passed.
    pub fn dispatcher(&self, class: &str, name: &str) -> Result<Option<Vec<(String, Ty)>>> {
        Ok(self.dispatcher_signature(class, name)?.map(|slots| slots.into_iter().map(|s| (s.name, s.ty)).collect()))
    }

    /// A dispatcher of a class: the parameters a function bound to it is passed, and how.
    pub fn dispatcher_signature(&self, class: &str, name: &str) -> Result<Option<Vec<Slot>>> {
        let found = self.lineage(class).find_map(|(_, c)| {
            let property = c.properties.get(name)?;
            (property.ty == "multicast_delegate").then_some(property.signature.as_ref()?)
        });
        found.map(|signature| slots(name, signature)).transpose()
    }

    /// The class and each of its ancestors, nearest first.
    fn lineage<'a>(&'a self, class: &'a str) -> impl Iterator<Item = (&'a str, &'a ClassDump)> + 'a {
        let mut next = Some(class);
        std::iter::from_fn(move || {
            let path = next?;
            let (path, dump) = self.dump.classes.get_key_value(path)?;
            next = dump.super_class.as_deref();
            Some((path.as_str(), dump))
        })
    }

    /// A panel widget's slot class, and whether it holds at most one child. None for a widget
    /// that is not a panel.
    pub fn panel<'a>(&'a self, class: &'a str) -> Option<(&'a str, bool)> {
        self.lineage(class).find_map(|(_, c)| Some((c.slot.as_deref()?, c.one_child)))
    }

    pub fn is_subclass(&self, class: &str, of: &str) -> bool {
        self.lineage(class).any(|(p, _)| p == of)
    }

    /// Whether a class is an interface, other than Interface itself, which every one derives from.
    pub fn is_interface(&self, class: &str) -> bool {
        class != INTERFACE && self.dump.classes.get(class).is_some_and(|c| c.interface)
    }

    /// Whether a Blueprint may implement an interface.
    pub fn is_implementable(&self, interface: &str) -> bool {
        self.dump.classes.get(interface).is_some_and(|c| !c.not_implementable)
    }

    /// Every interface a class implements, its own and its parents', each with the interfaces it
    /// derives from.
    pub fn interfaces_of(&self, class: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, dump) in self.lineage(class) {
            for interface in &dump.interfaces {
                for (path, _) in self.lineage(interface).take_while(|(p, _)| self.is_interface(p)) {
                    if !out.iter().any(|o| o == path) {
                        out.push(path.to_string());
                    }
                }
            }
        }
        out
    }

    /// Whether an interface, or one it derives from, declares a function of this name.
    pub fn interface_declares(&self, interface: &str, name: &str) -> bool {
        self.lineage(interface).take_while(|(p, _)| self.is_interface(p)).any(|(_, c)| c.functions.contains_key(name))
    }

    /// Whether a value of type `from` can go where `to` is expected.
    pub fn assignable(&self, from: &Ty, to: &Ty) -> bool {
        match (from, to) {
            (Ty::Object(a), Ty::Object(b))
            | (Ty::Class(a), Ty::Class(b))
            | (Ty::SoftObject(a), Ty::SoftObject(b))
            | (Ty::SoftClass(a), Ty::SoftClass(b)) => self.is_subclass(a, b),
            (Ty::Interface(a), Ty::Interface(b)) => self.is_subclass(a, b),
            (Ty::Array(a), Ty::Array(b)) | (Ty::Set(a), Ty::Set(b)) => a == b,
            (Ty::Map(ak, av), Ty::Map(bk, bv)) => ak == bk && av == bv,
            (
                Ty::Null,
                Ty::Object(_) | Ty::Class(_) | Ty::Array(_) | Ty::Set(_) | Ty::Map(..) | Ty::SoftObject(_) | Ty::SoftClass(_) | Ty::Interface(_),
            ) => true,
            _ => from == to,
        }
    }

    /// A property code can read or write.
    /// A property code can reach from another class: not one set only in the Details panel, nor
    /// a private one, which only its own class reaches.
    pub fn property(&self, class: &str, name: &str) -> Option<Property> {
        self.editable_property(class, name).filter(|p| !p.edit_only && !p.private)
    }

    /// A property a literal can set, as the Details panel does, including one code cannot reach.
    pub fn editable_property(&self, class: &str, name: &str) -> Option<Property> {
        self.lineage(class).find_map(|(_, c)| {
            let p = c.properties.get(name)?;
            Some(Property { ty: Ty::from_dump(&p.ty)?, read_only: p.read_only, edit_only: p.edit_only, private: p.private })
        })
    }

    /// The class that declares a property a class has, which is what a get or set node names.
    pub fn property_owner(&self, class: &str, name: &str) -> Option<String> {
        self.lineage(class).find(|(_, c)| c.properties.contains_key(name)).map(|(p, _)| p.to_string())
    }

    /// Whether a class declares any function itself, not counting its parents'.
    pub fn declares_functions(&self, class: &str) -> bool {
        self.dump.classes.get(class).is_some_and(|c| !c.functions.is_empty())
    }

    /// Whether a class has a function of this name, of any kind.
    pub fn has_function(&self, class: &str, name: &str) -> bool {
        self.lineage(class).any(|(_, c)| c.functions.contains_key(name))
    }

    pub fn function(&self, class: &str, name: &str) -> Result<Option<Function>> {
        self.function_in(class, name, &[])
    }

    /// A function of a container's library called on a container of `on`, as its method:
    /// `Items.RemoveItem(4)` calls `Array_RemoveItem`. Its types that the metadata ties to the
    /// container are `on`'s. The container it is called on is its first container parameter,
    /// which is left out of its parameters and named on its own.
    pub fn container_function(&self, on: &Ty, method: &str) -> Result<Option<(Function, String)>> {
        let Some((library, prefix)) = container_library(on) else { return Ok(None) };
        let name = format!("{prefix}{method}");
        let Some(dump) = self.dump.classes.get(library).and_then(|c| c.functions.get(&name)) else { return Ok(None) };
        let Some(receiver) = dump.params.iter().find(|p| p.generic == Some(Generic::Container)) else { return Ok(None) };
        let Some(mut function) = self.function(library, &name)? else { return Ok(None) };
        let role = |n: &str| dump.params.iter().find(|p| p.name == n).and_then(|p| p.generic);
        function.params.retain(|p| p.name != receiver.name);
        for p in &mut function.params {
            if let Some(r) = role(&p.name) {
                p.ty = fit(&p.ty, r, on);
            }
        }
        for (n, ty) in &mut function.outputs {
            if let Some(r) = role(n) {
                *ty = fit(ty, r, on);
            }
        }
        if let (Some(ty), Some(r)) = (&function.ret, role("ReturnValue")) {
            function.ret = Some(fit(ty, r, on));
        }
        Ok(Some((function, receiver.name.clone())))
    }

    /// The methods a container of `on` has: its library's functions, by their names without the
    /// prefix, that a Blueprint may call.
    pub fn container_methods(&self, on: &Ty) -> Vec<String> {
        let Some((library, prefix)) = container_library(on) else { return Vec::new() };
        let Some(class) = self.dump.classes.get(library) else { return Vec::new() };
        let mut names: Vec<String> = class
            .functions
            .iter()
            .filter(|(_, f)| f.params.iter().any(|p| p.generic == Some(Generic::Container)))
            .filter(|(_, f)| !f.flags.iter().any(|x| matches!(x.as_str(), "internal" | "not_callable" | "editor_only")))
            .filter_map(|(n, _)| n.strip_prefix(prefix).map(String::from))
            .collect();
        names.sort();
        names
    }

    /// A function a class has. One that is an event, internal or only runs in the editor is refused,
    /// unless its flag is one of `allowed`: an editor Blueprint allows "editor_only", and a
    /// conversion allows "internal" for the engine's own autocasts. A latent one is marked, for the
    /// caller to refuse where it can't wait.
    pub fn function_in(&self, class: &str, name: &str, allowed: &[&str]) -> Result<Option<Function>> {
        let Some((owner, dump)) = self.lineage(class).find_map(|(p, c)| c.functions.get(name).map(|f| (p, f)))
        else {
            return Ok(None);
        };
        let flag = |f: &str| dump.flags.iter().any(|x| x == f);
        for (f, why) in [
            ("not_callable", "is an event, not something to call"),
            ("internal", "is internal to the engine"),
            ("editor_only", "runs only in the editor, not in the game"),
        ] {
            if flag(f) && !allowed.contains(&f) {
                bail!("{name} {why}");
            }
        }
        let hidden: Vec<&str> = [&dump.world_context, &dump.hide_pin].into_iter().flatten().map(String::as_str).collect();
        let latent = flag("latent");
        let mut params = Vec::new();
        let mut outputs = Vec::new();
        let mut ret = None;
        let mut shape = Vec::new();
        for p in &dump.params {
            // The node fills a latent function's LatentActionInfo itself.
            if latent && p.dir == "in" && p.ty == LATENT_ACTION_INFO {
                continue;
            }
            let Some(ty) = Ty::from_dump(&p.ty) else {
                bail!("{name} has a parameter {} of type {}, which loom cannot use yet", p.name, p.ty);
            };
            // A Blueprint passes an array by reference whatever its source says.
            let passing = match Passing::from_dump(&p.dir, p.const_ref) {
                Passing::Value if !owner.starts_with("/Script/") => Passing::of_blueprint(&ty, false),
                passing => passing,
            };
            shape.push(Slot { name: p.name.clone(), ty: ty.clone(), passing, floats: Floats::of_dump(&p.ty) });
            match p.dir.as_str() {
                "return" => ret = Some(ty),
                "in" | "ref" if !hidden.contains(&p.name.as_str()) => params.push(Param {
                    name: p.name.clone(),
                    ty,
                    optional: p.default.is_some() || dump.default_to_self.as_deref() == Some(p.name.as_str()),
                    by_ref: p.dir == "ref",
                    wired: p.wired,
                }),
                "in" | "ref" => {}
                _ => outputs.push((p.name.clone(), ty)),
            }
        }
        Ok(Some(Function {
            owner: owner.to_string(),
            name: name.to_string(),
            pure: flag("pure"),
            is_static: flag("static"),
            params,
            ret,
            outputs,
            determines_output_type: dump.determines_output_type.clone(),
            latent,
            access: if flag("private") {
                Some("private")
            } else if flag("protected") {
                Some("protected")
            } else {
                None
            },
            deprecated: flag("deprecated"),
            shape,
        }))
    }

    /// Every parameter of a function, in the order a call's bytecode passes them, with the flags
    /// the dump gives the function, for the lifter to read a compiled call's arguments. None when
    /// the class has no such function.
    pub fn compiled_signature(&self, class: &str, name: &str) -> Option<CompiledSignature> {
        let (owner, dump) = self.lineage(class).find_map(|(p, c)| c.functions.get(name).map(|f| (p, f)))?;
        let flag = |f: &str| dump.flags.iter().any(|x| x == f);
        let latent = flag("latent");
        let hidden: Vec<&str> = [&dump.world_context, &dump.hide_pin].into_iter().flatten().map(String::as_str).collect();
        let params = dump
            .params
            .iter()
            .filter(|p| p.dir != "return")
            .map(|p| CompiledParam {
                name: p.name.clone(),
                dir: p.dir.clone(),
                ty: p.ty.clone(),
                hidden: hidden.contains(&p.name.as_str()) || (latent && p.dir == "in" && p.ty == LATENT_ACTION_INFO),
            })
            .collect();
        Some(CompiledSignature {
            owner: owner.to_string(),
            params,
            has_return: dump.params.iter().any(|p| p.dir == "return"),
            pure: flag("pure"),
            is_static: flag("static"),
            latent,
        })
    }

    /// An event a class declares, for `on` handlers: its parameters, what it returns, and the
    /// class that declares it.
    pub fn event(&self, class: &str, name: &str) -> Result<Event> {
        let Some((owner, dump)) = self.lineage(class).find_map(|(p, c)| c.functions.get(name).map(|f| (p, f))) else {
            bail!("{} has no event {name}", class.rsplit('.').next().unwrap_or(class));
        };
        if !dump.flags.iter().any(|f| f == "event") {
            bail!("{name} is not an event");
        }
        let mut event = Event { owner: owner.to_string(), params: Vec::new(), ret: None, outputs: Vec::new(), function: false };
        for p in &dump.params {
            match (Ty::from_dump(&p.ty), p.dir.as_str()) {
                (Some(ty), "in") => event.params.push((p.name.clone(), ty)),
                (Some(ty), "return") => event.ret = Some(ty),
                (Some(ty), "out") => event.outputs.push((p.name.clone(), ty)),
                _ => bail!("{name} has a parameter {} that loom cannot use yet", p.name),
            }
        }
        // As the editor tells them apart: an event with an output, or that is const, is a function.
        event.function = event.ret.is_some() || !event.outputs.is_empty() || dump.flags.iter().any(|f| f == "const");
        Ok(event)
    }
}

/// A signature's parameters, read, of the dispatcher or delegate `owner`.
fn slots(owner: &str, signature: &[SignatureParam]) -> Result<Vec<Slot>> {
    signature
        .iter()
        .map(|p| match Ty::from_dump(&p.ty) {
            Some(ty) => Ok(Slot { name: p.name.clone(), ty, passing: Passing::from_dump(&p.dir, p.const_ref), floats: Floats::of_dump(&p.ty) }),
            None => bail!("{owner} passes {} of type {}, which loom cannot use yet", p.name, p.ty),
        })
        .collect()
}

/// `/Script/Engine.KismetMathLibrary.MakeVector2D` as its class and function.
fn split_function(path: &str) -> Option<(String, String)> {
    let (class, function) = path.rsplit_once('.')?;
    Some((class.to_string(), function.to_string()))
}

/// Splits `a,b` at its one comma outside angle brackets, as in a map's key and value types.
pub fn split_pair(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => return Some((s[..i].trim(), s[i + 1..].trim())),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::TypeDb;

    fn db(settings: &str) -> TypeDb {
        TypeDb::parse(&format!(r#"{{"classes": {{}}{settings}}}"#)).unwrap()
    }

    #[test]
    fn structs_and_enums_are_found_in_used_folders_too() {
        let db = TypeDb::parse(r#"{"classes": {},
            "structs": {"/Game/A/F_Row.F_Row": {}, "/Game/B/F_Row.F_Row": {}},
            "enums": {"/Game/A/E_Kind.E_Kind": [], "/Game/B/E_Kind.E_Kind": []}}"#).unwrap();
        let uses = ["/Game/B".to_string()];
        let scope = super::TypeScope { folder: "/Game/Shop", uses: &uses };
        assert_eq!(db.structure("F_Row", &scope).unwrap().as_deref(), Some("/Game/B/F_Row.F_Row"));
        assert_eq!(db.enumeration("E_Kind", &scope).unwrap().as_deref(), Some("/Game/B/E_Kind.E_Kind"));
        let own = super::TypeScope { folder: "/Game/A", uses: &uses };
        assert_eq!(db.structure("F_Row", &own).unwrap().as_deref(), Some("/Game/A/F_Row.F_Row"));
        let nowhere = super::TypeScope { folder: "/Game/Shop", uses: &[] };
        assert!(db.structure("F_Row", &nowhere).unwrap_err().to_string().contains("ambiguous"));
    }

    #[test]
    fn any_class_may_be_instanced_without_instanced_class_paths() {
        for db in [db(""), db(r#", "settings": {}"#), db(r#", "settings": {"instanced_class_paths": []}"#)] {
            assert!(db.may_instance("/Game/Anywhere/BP_X.BP_X_C"));
        }
    }

    #[test]
    fn instanced_class_paths_hold_whole_folders() {
        let db = db(r#", "settings": {"instanced_class_paths": ["/Game/Lib", "/Game/Mine/"]}"#);
        assert!(db.may_instance("/Script/Engine.StaticMeshComponent"));
        assert!(db.may_instance("/Game/Lib/BP_X.BP_X_C"));
        assert!(db.may_instance("/Game/Mine/Deep/BP_X.BP_X_C"));
        assert!(!db.may_instance("/Game/Library/BP_X.BP_X_C"));
        assert!(!db.may_instance("/Game/BP_X.BP_X_C"));
    }
}
