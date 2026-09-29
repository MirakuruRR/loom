//! ops.json: the flat list of editor operations the LoomEditor plugin applies. This is the
//! compiler's output and the plugin's input, so the two must agree on every field here.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const VERSION: u32 = 0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Blueprint {
        path: String,
        parent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// A widget Blueprint, which has a widget tree as well as graphs.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        widget: bool,
        /// The name of a widget Blueprint's root canvas, which the tree starts out as.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        canvas: Option<String>,
    },
    /// A user-defined enum, with these values in order, made or remade whole. A document that makes
    /// one holds this op and "save", and no other.
    Enum {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        values: Vec<Enumerator>,
        /// Its tooltip in the editor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// A user-defined struct, with these fields in order, made or brought up to date: a field is
    /// kept by its name, so what holds one keeps its value. A document that makes one holds this
    /// op and "save", and no other.
    Struct {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        fields: Vec<StructField>,
        /// Its tooltip in the editor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// An interface the Blueprint implements, as the Class Settings panel adds one: each of its
    /// functions that returns a value gets a graph, which a "function" op overriding it then fills.
    /// It comes before every function and node.
    Implement { interface: String },
    /// A widget of a widget Blueprint's tree, which becomes a variable of it, with its fields set
    /// from text as the Details panel sets them. With a parent, an earlier widget, it is that
    /// widget's next child. With a place, `[x, y, width, height]`, it goes on the tree's root
    /// canvas. With neither, it is the tree's root. `slot` sets the fields of the slot that holds
    /// it, the same way.
    Widget {
        name: String,
        class: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fields: Vec<ComponentField>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<[i64; 4]>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        slot: Vec<ComponentField>,
    },
    Variable {
        name: String,
        #[serde(rename = "type")]
        ty: Type,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        /// Shown in the Details panel, where each instance of the class can set its own value.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        editable: bool,
        /// Only this Blueprint can read or set it, as the editor's Private checkbox says.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        private: bool,
        /// Its tooltip in the editor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// The Details panel category it is listed under.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
    },
    /// A component in the Blueprint's Components tree, with values for its template's fields.
    Component {
        name: String,
        class: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fields: Vec<ComponentField>,
    },
    /// An event dispatcher: a multicast delegate property, with the signature its binders take.
    Dispatcher {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        params: Vec<Param>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
    },
    Function {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        params: Vec<Param>,
        /// The function's outputs. A "result" node in its graph returns through them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        returns: Vec<Param>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locals: Vec<Local>,
        /// Its tooltip in the editor.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// The palette category it is listed under.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        /// The class that declares the event this function overrides, which gives it its
        /// signature: a parent class, or an interface. None for the Blueprint's own functions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        overrides: Option<String>,
        /// A pure node, with no exec pins, run each time its value is read.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pure: bool,
        /// "private" or "protected", as the function's access specifier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access: Option<String>,
    },
    Node {
        graph: String,
        id: String,
        #[serde(flatten)]
        kind: NodeKind,
    },
    Link {
        from: PinRef,
        to: PinRef,
    },
    Default {
        pin: PinRef,
        value: String,
    },
    Compile,
    /// The compiled class's default for a property its parent declares, or a field inside one
    /// (`a.b`): set from text as the Class Defaults panel sets it, or copied from the same
    /// property's default in `from`, another class.
    ClassDefault {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    Save,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Type,
    /// Passed by reference: the function works on the caller's variable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_ref: bool,
    /// A function's parameter's default, as text, which a call that leaves it out gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// A value of an enum. `label` is what the editor shows for it when that is not its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enumerator {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its tooltip in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A field of a struct, with its default as text, as the struct editor sets it, when it isn't zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Its tooltip in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A field of a component's template and its value, as the Details panel would set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Local {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// The function graph's existing entry node.
    Entry,
    /// An override of an event declared on `class`, e.g. ReceiveBeginPlay on Actor.
    Event { event: String, class: String },
    /// A custom event of the Blueprint's own, with an output pin for each parameter.
    CustomEvent { name: String, params: Vec<Param> },
    /// `function` is `<class path>:<name>`, or `self:<name>` for the Blueprint's own.
    Call { function: String },
    /// Calls an interface's `function`, `<interface path>:<name>`, on the object on its "self" pin.
    /// On an object that does not implement the interface it does nothing, and each output is its
    /// type's zero. Never pure.
    Message { function: String },
    /// The parent class's own version of `function`, `<class path>:<name>`, called from the
    /// override of it.
    CallParent { function: String },
    Branch,
    /// Reads a variable. An external or inherited one is a property of `class`.
    Get {
        var: String,
        scope: Scope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
    },
    Set {
        var: String,
        scope: Scope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
    },
    /// Returns from the function, through the outputs its "function" op declared.
    Result,
    /// Splits a struct into a pin per field. Its input pin is named after the struct.
    Break { r#struct: String },
    /// Makes a struct from a pin per field. Its output pin is named after the struct.
    Make { r#struct: String },
    /// Whether the values of `enum` on "A" and "B" are equal, on "ReturnValue". Pure.
    EnumEqual { r#enum: String },
    /// A value of `enum`, given on its "Enum" pin, on "ReturnValue". Pure.
    EnumLiteral { r#enum: String },
    /// The value of `enum` a byte on its "Byte" pin stands for, on "ReturnValue", whether the enum
    /// names it or not. Pure.
    ByteToEnum { r#enum: String },
    /// Sets `fields` of the struct linked to its "StructRef" pin, in place, from a pin per field.
    SetFields { r#struct: String, fields: Vec<String> },
    /// Reads an array's item: pins "Array", "Dimension 1" and "Output".
    ArrayGet,
    /// Creates a widget of `class`, a widget Blueprint's class.
    CreateWidget { class: String },
    /// Makes an object of `class`, held by the object on its outer pin, which is named "self", on "ReturnValue".
    Construct { class: String },
    /// `class` or none, whichever the object on "Object" is, on its output pin "Result". Pure.
    Cast { class: String },
    /// `class` or a subclass, or none, whichever the class on "Class" is, on its output pin
    /// "Result". Pure.
    ClassCast { class: String },
    /// The engine's or the editor's subsystem of `class`, on its output pin "ReturnValue". Pure.
    Subsystem { class: String },
    /// An event bound to a dispatcher of one of this Blueprint's components, with an output pin
    /// per parameter.
    BoundEvent { component: String, dispatcher: String },
    /// A delegate to `function`, on its output pin "OutputDelegate", which must be linked: it takes
    /// its signature from the pin it feeds. The function is this Blueprint's, or with an object on
    /// its "self" pin, that object's, found on the pin's class. Pure.
    Delegate { function: String },
    /// Binds the delegate on its "Delegate" pin to `dispatcher`, which `class` declares, on the
    /// object on its "self" pin, so that the object's broadcasts call it. Without a class, it is
    /// this Blueprint's own dispatcher, on this object.
    AddDelegate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
        dispatcher: String,
    },
    /// Unbinds the delegate on its "Delegate" pin, the same object's same function, from a
    /// dispatcher, named as `add_delegate` names it.
    RemoveDelegate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
        dispatcher: String,
    },
    /// Unbinds everything bound to a dispatcher, named as `add_delegate` names it.
    ClearDelegate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
        dispatcher: String,
    },
    /// Calls everything bound to this Blueprint's dispatcher, with a pin per parameter.
    Broadcast { dispatcher: String },
    /// This object, on its output pin "self".
    #[serde(rename = "self")]
    SelfRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[serde(rename = "self")]
    SelfMember,
    Local,
    /// A property of another object's class, read or written through the node's "self" pin.
    External,
    /// A property this Blueprint inherits from its parent classes, on this object.
    Inherited,
}

/// A pin type, written as `int`, `float`, `object:/Script/Engine.Actor` and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Int,
    Int64,
    Float,
    /// A 32-bit float, which a function bound to an engine delegate that passes one takes, where
    /// Blueprint's own floats are 64-bit.
    Float32,
    String,
    Name,
    Text,
    Object(String),
    Class(String),
    Struct(String),
    Enum(String),
    Array(Box<Type>),
    Set(Box<Type>),
    Map(Box<Type>, Box<Type>),
    SoftObject(String),
    SoftClass(String),
    /// An object that implements this interface, or none.
    Interface(String),
    /// A single-cast delegate, named by its signature function's path.
    Delegate(String),
}

impl FromStr for Type {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "int64" => Type::Int64,
            "float" => Type::Float,
            "float32" => Type::Float32,
            "string" => Type::String,
            "name" => Type::Name,
            "text" => Type::Text,
            _ if s.starts_with("array<") && s.ends_with('>') => Type::Array(Box::new(s[6..s.len() - 1].parse()?)),
            _ if s.starts_with("set<") && s.ends_with('>') => Type::Set(Box::new(s[4..s.len() - 1].parse()?)),
            _ if s.starts_with("map<") && s.ends_with('>') => {
                let Some((key, value)) = crate::types::split_pair(&s[4..s.len() - 1]) else { bail!("unknown type '{s}'") };
                Type::Map(Box::new(key.parse()?), Box::new(value.parse()?))
            }
            _ => match s.split_once(':') {
                Some(("object", path)) if !path.is_empty() => Type::Object(path.to_string()),
                Some(("class", path)) if !path.is_empty() => Type::Class(path.to_string()),
                Some(("struct", path)) if !path.is_empty() => Type::Struct(path.to_string()),
                Some(("enum", path)) if !path.is_empty() => Type::Enum(path.to_string()),
                Some(("softobject", path)) if !path.is_empty() => Type::SoftObject(path.to_string()),
                Some(("softclass", path)) if !path.is_empty() => Type::SoftClass(path.to_string()),
                Some(("interface", path)) if !path.is_empty() => Type::Interface(path.to_string()),
                Some(("delegate", path)) if !path.is_empty() => Type::Delegate(path.to_string()),
                _ => bail!("unknown type '{s}'"),
            },
        })
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => f.write_str("bool"),
            Type::Int => f.write_str("int"),
            Type::Int64 => f.write_str("int64"),
            Type::Float => f.write_str("float"),
            Type::Float32 => f.write_str("float32"),
            Type::String => f.write_str("string"),
            Type::Name => f.write_str("name"),
            Type::Text => f.write_str("text"),
            Type::Object(class) => write!(f, "object:{class}"),
            Type::Class(class) => write!(f, "class:{class}"),
            Type::Struct(path) => write!(f, "struct:{path}"),
            Type::Enum(path) => write!(f, "enum:{path}"),
            Type::Array(inner) => write!(f, "array<{inner}>"),
            Type::Set(inner) => write!(f, "set<{inner}>"),
            Type::Map(key, value) => write!(f, "map<{key},{value}>"),
            Type::SoftObject(class) => write!(f, "softobject:{class}"),
            Type::SoftClass(class) => write!(f, "softclass:{class}"),
            Type::Interface(class) => write!(f, "interface:{class}"),
            Type::Delegate(signature) => write!(f, "delegate:{signature}"),
        }
    }
}

/// A pin on a node, written `<node id>.<pin name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRef {
    pub node: String,
    pub pin: String,
}

impl FromStr for PinRef {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.split_once('.') {
            Some((node, pin)) if !node.is_empty() && !pin.is_empty() => Ok(PinRef {
                node: node.to_string(),
                pin: pin.to_string(),
            }),
            _ => bail!("pin reference must be <node id>.<pin>, not '{s}'"),
        }
    }
}

impl fmt::Display for PinRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.node, self.pin)
    }
}

macro_rules! string_serde {
    ($t:ty) => {
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}
string_serde!(Type);
string_serde!(PinRef);

impl Document {
    pub fn parse(text: &str) -> Result<Self> {
        let doc: Document = serde_json::from_str(text)?;
        doc.check()?;
        Ok(doc)
    }

    /// One op per line, so a change to what the compiler emits reads as a short diff.
    pub fn to_json(&self) -> String {
        let ops: Vec<String> = self
            .ops
            .iter()
            .map(|op| format!("    {}", serde_json::to_string(op).expect("ops always serialise")))
            .collect();
        format!("{{\n  \"version\": {},\n  \"ops\": [\n{}\n  ]\n}}", self.version, ops.join(",\n"))
    }

    /// Checks what can be checked without Unreal: ordering, names, and that every reference
    /// points at something declared before it. Pin names are the plugin's to check.
    pub fn check(&self) -> Result<()> {
        if self.version != VERSION {
            bail!("version is {}, expected {VERSION}", self.version);
        }
        let mut graphs: HashMap<&str, HashSet<&str>> = HashMap::new();
        let mut members = HashSet::new();
        let mut widgets = HashSet::new();
        let mut dispatchers = HashSet::new();
        let mut nodes = HashMap::new();
        let mut implemented = HashSet::new();
        // Each delegate node not yet linked to anything, which would have no signature.
        let mut unfed = HashSet::new();
        let mut compiled = false;
        if let Some(Op::Enum { values, .. }) = self.ops.first() {
            if !matches!(self.ops.as_slice(), [_, Op::Save]) {
                bail!("a document that makes an enum holds \"enum\" and \"save\", and no other op");
            }
            let names: HashSet<&str> = values.iter().map(|v| v.name.as_str()).collect();
            if values.is_empty() || names.len() != values.len() {
                bail!("op 0: an enum has at least one value, and each once");
            }
            return Ok(());
        }
        if let Some(Op::Struct { fields, .. }) = self.ops.first() {
            if !matches!(self.ops.as_slice(), [_, Op::Save]) {
                bail!("a document that makes a struct holds \"struct\" and \"save\", and no other op");
            }
            let names: HashSet<&str> = fields.iter().map(|f| f.name.as_str()).collect();
            if fields.is_empty() || names.len() != fields.len() {
                bail!("op 0: a struct has at least one field, and each once");
            }
            return Ok(());
        }
        for (index, op) in self.ops.iter().enumerate() {
            let at = || format!("op {index}");
            // A document builds one Blueprint, starting with "blueprint".
            match op {
                Op::Blueprint { widget, canvas, .. } if index == 0 => {
                    match (widget, canvas) {
                        (true, None) => bail!("op 0: a widget Blueprint names its canvas"),
                        (false, Some(_)) => bail!("op 0: only a widget Blueprint has a canvas"),
                        _ => {}
                    }
                    graphs.insert("EventGraph", HashSet::new());
                    continue;
                }
                _ if graphs.is_empty() => bail!("op {index}: a document starts with \"blueprint\", \"enum\" or \"struct\""),
                _ => {}
            }
            match op {
                Op::Widget { name, parent, place, .. } => {
                    let widget = matches!(self.ops[0], Op::Blueprint { widget: true, .. });
                    if !widget {
                        bail!("{}: widget {name} is in a Blueprint that is not a widget Blueprint", at());
                    }
                    if let Some(parent) = parent {
                        if place.is_some() {
                            bail!("{}: widget {name} has a parent and a place on the root canvas", at());
                        }
                        if !widgets.contains(parent.as_str()) {
                            bail!("{}: widget {name}'s parent {parent} is not a widget before it", at());
                        }
                    }
                    widgets.insert(name.as_str());
                    if !members.insert(name.as_str()) {
                        bail!("{}: {name} is declared twice", at());
                    }
                }
                Op::Blueprint { .. } => bail!("{}: one ops.json builds one Blueprint", at()),
                Op::Enum { .. } => bail!("{}: an enum is made by a document of its own", at()),
                Op::Struct { .. } => bail!("{}: a struct is made by a document of its own", at()),
                Op::Implement { interface } => {
                    if graphs.len() > 1 || !nodes.is_empty() {
                        bail!("{}: implement {interface} comes after a function or a node; it goes before them", at());
                    }
                    if !implemented.insert(interface.as_str()) {
                        bail!("{}: {interface} is implemented twice", at());
                    }
                }
                Op::Variable { name, .. } => {
                    if !members.insert(name.as_str()) {
                        bail!("{}: variable {name} is declared twice", at());
                    }
                }
                Op::Component { name, .. } => {
                    if !members.insert(name.as_str()) {
                        bail!("{}: {name} is declared twice", at());
                    }
                }
                Op::Dispatcher { name, params, .. } => {
                    if !members.insert(name.as_str()) || !dispatchers.insert(name.as_str()) {
                        bail!("{}: {name} is declared twice", at());
                    }
                    let names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
                    if names.len() != params.len() {
                        bail!("{}: dispatcher {name} declares a parameter twice", at());
                    }
                }
                Op::Function { name, params, locals, .. } => {
                    let names: HashSet<&str> = locals.iter().map(|l| l.name.as_str()).collect();
                    if names.len() != locals.len() {
                        bail!("{}: function {name} declares a local twice", at());
                    }
                    let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();
                    if param_names.len() != params.len() {
                        bail!("{}: function {name} declares a parameter twice", at());
                    }
                    if graphs.insert(name.as_str(), names).is_some() {
                        bail!("{}: graph {name} is declared twice", at());
                    }
                }
                Op::Node { graph, id, kind } => {
                    let locals = graphs
                        .get(graph.as_str())
                        .with_context(|| format!("{}: node {id} is in undeclared graph {graph}", at()))?;
                    match kind {
                        NodeKind::Entry if graph == "EventGraph" => {
                            bail!("{}: the event graph has no entry node", at())
                        }
                        NodeKind::Event { .. } if graph != "EventGraph" => {
                            bail!("{}: event {id} must be in the event graph", at())
                        }
                        NodeKind::Result if graph == "EventGraph" => {
                            bail!("{}: the event graph cannot return", at())
                        }
                        NodeKind::Broadcast { dispatcher }
                        | NodeKind::AddDelegate { class: None, dispatcher }
                        | NodeKind::RemoveDelegate { class: None, dispatcher }
                        | NodeKind::ClearDelegate { class: None, dispatcher }
                            if !dispatchers.contains(dispatcher.as_str()) =>
                        {
                            bail!("{}: no dispatcher {dispatcher}", at())
                        }
                        NodeKind::Get { class, scope, .. } | NodeKind::Set { class, scope, .. }
                            if matches!(scope, Scope::External | Scope::Inherited) != class.is_some() =>
                        {
                            bail!("{}: node {id} must name a class exactly when its scope is external or inherited", at())
                        }
                        NodeKind::Get { var, scope, .. } | NodeKind::Set { var, scope, .. } => match scope {
                            Scope::SelfMember if !members.contains(var.as_str()) => {
                                bail!("{}: no member variable {var}", at())
                            }
                            Scope::Local if !locals.contains(var.as_str()) => {
                                bail!("{}: no local {var} in {graph}", at())
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                    if nodes.insert(id.as_str(), graph.as_str()).is_some() {
                        bail!("{}: node id {id} is used twice", at());
                    }
                    if let NodeKind::Delegate { .. } = kind {
                        unfed.insert(id.as_str());
                    }
                }
                Op::Link { from, to } => {
                    unfed.remove(from.node.as_str());
                    let a = nodes.get(from.node.as_str()).with_context(|| format!("{}: no node {}", at(), from.node))?;
                    let b = nodes.get(to.node.as_str()).with_context(|| format!("{}: no node {}", at(), to.node))?;
                    if a != b {
                        bail!("{}: {from} and {to} are in different graphs", at());
                    }
                }
                Op::Default { pin, .. } => {
                    if !nodes.contains_key(pin.node.as_str()) {
                        bail!("{}: no node {}", at(), pin.node);
                    }
                }
                Op::Compile => compiled = true,
                Op::ClassDefault { name, value, from } => {
                    if !compiled {
                        bail!("{}: class_default {name} comes before compile, which makes the class", at());
                    }
                    if value.is_some() == from.is_some() {
                        bail!("{}: class_default {name} needs a value or a class to copy from, not both", at());
                    }
                }
                Op::Save => {
                    if !compiled {
                        bail!("{}: save comes before compile", at());
                    }
                }
            }
        }
        if let Some(id) = unfed.iter().min() {
            bail!("delegate node {id} feeds nothing, so it has no signature");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = include_str!("../../../examples/sum/compiled.json");

    #[test]
    fn sum_example_parses_and_checks() {
        Document::parse(SUM).unwrap();
    }

    #[test]
    fn round_trips() {
        let doc = Document::parse(SUM).unwrap();
        assert_eq!(Document::parse(&doc.to_json()).unwrap(), doc);
    }

    #[test]
    fn rejects_a_link_to_an_undeclared_node() {
        let text = r#"{"version": 0, "ops": [
            {"op": "blueprint", "path": "/Game/X", "parent": "/Script/Engine.Actor"},
            {"op": "link", "from": "a.then", "to": "b.execute"}
        ]}"#;
        assert!(Document::parse(text).unwrap_err().to_string().contains("no node a"));
    }

    #[test]
    fn a_widget_blueprint_names_its_canvas() {
        let text = r#"{"version": 0, "ops": [
            {"op": "blueprint", "path": "/Game/X", "parent": "/Script/UMG.UserWidget", "widget": true}
        ]}"#;
        assert!(Document::parse(text).unwrap_err().to_string().contains("names its canvas"));
        let text = r#"{"version": 0, "ops": [
            {"op": "blueprint", "path": "/Game/X", "parent": "/Script/Engine.Actor", "canvas": "Loom_Canvas"}
        ]}"#;
        assert!(Document::parse(text).unwrap_err().to_string().contains("only a widget Blueprint"));
    }
}
