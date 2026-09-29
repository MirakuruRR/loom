//! Source text to a syntax tree.

use std::fmt;

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// A source file, which builds the one asset its header names: a Blueprint, or an enum.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Blueprint(File),
    Enum(Enum),
    Struct(Struct),
    Interface(Interface),
}

impl Source {
    pub fn name(&self) -> &str {
        match self {
            Source::Blueprint(file) => &file.name,
            Source::Enum(e) => &e.name,
            Source::Struct(s) => &s.name,
            Source::Interface(i) => &i.name,
        }
    }

    /// What it builds, as a message names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Blueprint(_) => "Blueprint",
            Source::Enum(_) => "enum",
            Source::Struct(_) => "struct",
            Source::Interface(_) => "interface",
        }
    }

    /// The package of the asset it builds.
    pub fn path(&self) -> &str {
        match self {
            Source::Blueprint(file) => &file.path,
            Source::Enum(e) => &e.path,
            Source::Struct(s) => &s.path,
            Source::Interface(i) => &i.path,
        }
    }

    /// The folders it uses, in the order written.
    pub fn uses(&self) -> Vec<String> {
        match self {
            Source::Blueprint(file) => file.uses(),
            Source::Enum(_) | Source::Struct(_) | Source::Interface(_) => Vec::new(),
        }
    }
}

/// An enum's source, `enum Name at /Game/Path { ... }`: a user-defined enum, with its values in
/// the order written.
#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub name: String,
    pub path: String,
    pub values: Vec<EnumValue>,
    /// Its tooltip.
    pub doc: Option<String>,
    pub span: Span,
}

/// A value of an enum. `label` is what the editor shows for it, `Sword "Great Sword"`, when it is
/// not the name.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub name: String,
    pub label: Option<String>,
    pub doc: Option<String>,
    pub span: Span,
}

/// A struct's source, `struct Name at /Game/Path { ... }`: a user-defined struct, with its fields
/// in the order written.
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub name: String,
    pub path: String,
    pub fields: Vec<Field>,
    /// Its tooltip.
    pub doc: Option<String>,
    pub span: Span,
}

/// A field of a struct, `Name: type`, and its default, `= value`, when it isn't zero.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: TypeName,
    pub default: Option<Expr>,
    pub doc: Option<String>,
    pub span: Span,
}

/// An interface's source, `interface Name at /Game/Path { ... }`: a Blueprint interface, with the
/// functions a Blueprint that implements it handles, in the order written.
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    pub name: String,
    pub path: String,
    pub functions: Vec<Signature>,
    /// Its tooltip.
    pub doc: Option<String>,
    pub span: Span,
}

/// A function an interface declares, `fn Name(params) -> type`, with no body.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub name: String,
    pub params: Vec<ParamDecl>,
    pub ret: Option<TypeName>,
    pub doc: Option<String>,
    pub span: Span,
}

impl File {
    /// The folders it uses, in the order written.
    pub fn uses(&self) -> Vec<String> {
        self.items.iter().filter_map(|item| match item {
            Item::Use { folder, .. } => Some(folder.clone()),
            _ => None,
        }).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    /// An editor Blueprint, `editor blueprint ...`: one whose parent runs only in the editor, such
    /// as a validator. It may use the editor's classes, and the game never loads it.
    pub editor: bool,
    pub name: String,
    pub parent: String,
    pub path: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `use /Game/Libs/Inventory`: a folder the source takes the types it names by their short
    /// names from, after its own folder. It comes before the Blueprint's other items.
    Use { folder: String, span: Span },
    /// `implements SomeInterface`: an interface the Blueprint implements, whose functions its `on`
    /// handlers may implement. It comes after the header and any `use`, before the other items.
    Implements { interface: TypeName, span: Span },
    /// `editable` shows it in the editor's Details panel, where each instance can set it. `doc` is
    /// its tooltip there.
    /// `private var`, which only this Blueprint can read or set, is `private`.
    Var { name: String, ty: TypeName, default: Option<Expr>, editable: bool, private: bool, doc: Option<String>, span: Span },
    /// An event dispatcher: other objects bind to it, and `Name.broadcast(args)` calls them all.
    Dispatcher { name: String, params: Vec<ParamDecl>, doc: Option<String>, span: Span },
    /// `doc` is the run of `//` lines directly above it, which becomes its tooltip.
    ///
    /// `private` or `protected` before it limits who may call it, and `pure` makes it a pure node,
    /// with no exec pins, run each time its value is read. `pure mut` lets a pure function change
    /// what is outside it too.
    Fn {
        name: String,
        params: Vec<ParamDecl>,
        ret: Option<TypeName>,
        body: Vec<Stmt>,
        doc: Option<String>,
        access: Access,
        pure: Purity,
        span: Span,
    },
    /// The palette category every function of the Blueprint is listed under.
    Category { name: String, span: Span },
    /// A widget in a widget Blueprint's tree, `widget Name: Class(field: value, ...)`, with
    /// literal values for its fields as the Details panel sets them. With `at x y size w h` it is
    /// placed on the tree's root canvas; without, and outside any other widget, it is the tree's
    /// root. The widgets in its `{ ... }` block are its children, and come after it, each naming it
    /// as `parent`. `slot(field: value, ...)` sets the fields of the slot that holds it in its
    /// parent, such as its padding and alignment.
    Widget {
        name: String,
        class: TypeName,
        fields: Vec<(String, Expr)>,
        slot: Vec<(String, Expr)>,
        place: Option<[i64; 4]>,
        parent: Option<String>,
        span: Span,
    },
    /// A handler for an event the parent class declares, naming the event's parameters. An event
    /// that returns a value, such as a validator's, names its return type too. With `component`,
    /// `on Component.Dispatcher`, it handles a dispatcher of one of the Blueprint's components.
    On { component: Option<String>, event: String, params: Vec<ParamDecl>, ret: Option<TypeName>, body: Vec<Stmt>, span: Span },
    /// `event Name(params) { }`: a custom event of the Blueprint's own. It is called as a function
    /// is, but gives nothing back and may wait, since it runs in the event graph.
    Event { name: String, params: Vec<ParamDecl>, body: Vec<Stmt>, doc: Option<String>, span: Span },
    /// A component the Blueprint is built with, `component Name: Class(field: value, ...)`, with
    /// literal values for the fields of its template, as the Details panel sets them.
    Component { name: String, class: TypeName, fields: Vec<(String, Expr)>, span: Span },
    /// `default Name = value`: the Blueprint's own default for a property its parent declares, as
    /// the Class Defaults panel sets it. `path` may go on into the property's struct fields. The
    /// value is a literal, or `Class.Name`, another class's default for it, copied whole.
    Default { path: Vec<String>, value: Expr, span: Span },
}

/// Who may call a function, as Blueprint's access specifier says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    /// Anything.
    #[default]
    Public,
    /// The Blueprint that declares it, and those that derive from it.
    Protected,
    /// The Blueprint that declares it alone.
    Private,
}

/// Whether a function is a pure node, and whether its body may change what is outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Purity {
    #[default]
    Impure,
    /// `pure`: its body changes nothing outside it.
    Pure,
    /// `pure mut`: a pure node whose body may change what is outside it, each time it runs.
    PureMut,
}

/// A parameter. One marked `ref` is the caller's own variable, not a copy of its value.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub ty: TypeName,
    pub by_ref: bool,
    /// `out`: an output of the function rather than an input. The body assigns it as it would a
    /// local, and a call gives it as a member of the call's outputs.
    pub out: bool,
    /// What the parameter is when a call leaves it out: only a function's has one, and every
    /// parameter after one with a default has one too.
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeName {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, ty: Option<TypeName>, value: Expr, span: Span },
    /// `target` is a name or a member.
    Assign { target: Expr, value: Expr, span: Span },
    Return { value: Option<Expr>, span: Span },
    While { cond: Expr, body: Vec<Stmt>, span: Span },
    /// Leaves the innermost loop, or the one its label names.
    Break { label: Option<String>, span: Span },
    /// Starts the next pass of the innermost loop, or of the one its label names.
    Continue { label: Option<String>, span: Span },
    /// `label: for ...` or `label: while ...`: a loop with a name, which a `break` or `continue`
    /// inside it can give to leave or go on with it rather than the innermost loop.
    Labelled { label: String, looped: Box<Stmt>, span: Span },
    /// `for item in array { }`, or `for key, value in map { }`.
    For { names: Vec<String>, items: Expr, body: Vec<Stmt>, span: Span },
    /// `target op= value`, as `target = target op value`, with what picks out the target, an index
    /// or the object whose member it is, computed once.
    Compound { target: Expr, op: BinaryOp, value: Expr, span: Span },
    /// `for i in first..last`: a count from first to last, both included, as Blueprint's ForLoop
    /// counts. `first` is computed once, before the first pass, and `last` before every pass.
    Count { name: String, first: Expr, last: Expr, body: Vec<Stmt>, span: Span },
    If { cond: Expr, then: Vec<Stmt>, otherwise: Vec<Stmt>, span: Span },
    /// `match value { A, B { } C { } else { } }`: the body of the first arm that lists the value,
    /// or `otherwise` when none does.
    Match { value: Expr, arms: Vec<(Vec<Expr>, Vec<Stmt>)>, otherwise: Option<Vec<Stmt>>, span: Span },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// `none`: no object.
    Null,
    Name(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `name(args)`. The flag says the name was written in backticks, which makes it never a
    /// built-in function but always the Blueprint's own.
    Call(String, Vec<Expr>, bool),
    /// `Struct(field: value, ...)`: a struct with the fields given, the rest zero.
    Make(String, Vec<(String, Expr)>),
    /// `array[index]`.
    Index(Box<Expr>, Box<Expr>),
    /// `[]`: an empty array.
    EmptyArray,
    /// `[a, b, c]`: an array of these items, in order.
    ArrayLiteral(Vec<Expr>),
    /// `{key: value, ...}`: a map of these entries. `{}` is an empty map.
    MapLiteral(Vec<(Expr, Expr)>),
    /// An asset or class path, such as /Game/Props/Barrel.Barrel, as a soft reference's value.
    Path(String),
    /// `object.name`, or `Class.name` for a static, where the object is a class name.
    Member(Box<Expr>, String),
    /// `object.name(args)`, or `Class.name(args)` for a static function.
    Method(Box<Expr>, String, Vec<Expr>),
    /// `if condition { a } else { b }`: `a` when the condition holds, else `b`. Only the side
    /// chosen is computed.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `match value { A, B { a } else { b } }`: the value of the first arm that lists the value, or
    /// the last one when none does. Only the arm chosen is computed.
    Match(Box<Expr>, Vec<(Vec<Expr>, Expr)>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    BitAnd,
    BitOr,
}

/// The binary operators: how each is written, how tightly it binds (higher binds tighter), and
/// what it does, as the reference describes it. The parser reads its operators from here.
pub const BINARY_OPERATORS: &[(&str, BinaryOp, u8, &str)] = &[
    ("||", BinaryOp::Or, 1, "or"),
    ("&&", BinaryOp::And, 2, "and"),
    ("==", BinaryOp::Eq, 3, "equal"),
    ("!=", BinaryOp::Ne, 3, "not equal"),
    ("<", BinaryOp::Lt, 4, "less"),
    ("<=", BinaryOp::Le, 4, "less or equal"),
    (">", BinaryOp::Gt, 4, "greater"),
    (">=", BinaryOp::Ge, 4, "greater or equal"),
    // Bitwise operators bind tighter than comparisons, so flags & 32 != 0 reads as meant.
    ("|", BinaryOp::BitOr, 5, "bitwise or"),
    ("&", BinaryOp::BitAnd, 6, "bitwise and"),
    ("+", BinaryOp::Add, 7, "add"),
    ("-", BinaryOp::Sub, 7, "subtract"),
    ("*", BinaryOp::Mul, 8, "multiply"),
    ("/", BinaryOp::Div, 8, "divide"),
    ("%", BinaryOp::Rem, 8, "remainder"),
];

impl BinaryOp {
    pub fn symbol(self) -> &'static str {
        BINARY_OPERATORS.iter().find(|(_, op, _, _)| *op == self).expect("every operator is in BINARY_OPERATORS").0
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
impl UnaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Words

/// The words that start a statement or a value, which the parser could not tell from a name
/// there. None of them is a name unless it is written in backticks, as `` `if` ``.
pub const RESERVED: &[&str] = &["break", "continue", "else", "for", "if", "let", "match", "return", "while"];

/// The words that mean something only where the parser expects them, such as `var` at the start
/// of an item or `out` before a parameter's name. Anywhere else each is a name like any other.
pub const CONTEXTUAL: &[&str] = &[
    "at", "blueprint", "category", "component", "default", "dispatcher", "editable", "editor", "enum", "event", "fn", "implements", "in",
    "interface", "mut", "on", "out", "private", "protected", "pure", "ref", "size", "slot", "struct", "use", "var", "widget",
];

/// The words that are this object and its parent's version of it.
pub const SELF_WORDS: &[&str] = &["self", "super"];

/// Whether a word is a name only in backticks: a reserved word, a literal, `self` or `super`.
pub fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word) || LITERAL_WORDS.contains(&word) || SELF_WORDS.contains(&word)
}

/// A name as a source writes it: as it is, or in backticks when it is reserved or is not made of
/// letters, digits and underscores, such as a function called `Scale Sample Count`. A path is
/// written as it is. None is a name no source can write, one holding a backtick or a line break.
pub fn written_name(name: &str) -> Option<String> {
    if name.starts_with('/') {
        return Some(name.to_string());
    }
    if name.is_empty() || name.contains(['`', '\n', '\r']) {
        return None;
    }
    let plain = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    Some(if plain && !is_reserved(name) { name.to_string() } else { format!("`{name}`") })
}

/// The words that make a type of other types: array<int>, map<string, int>.
pub const TYPE_WORDS: &[&str] = &["array", "class", "map", "set", "soft"];

/// The start of every name loom makes itself, which a declared name may not have.
pub const RESERVED_PREFIX: &str = "Loom_";

/// The words that are values.
#[cfg_attr(not(test), allow(dead_code))] // read only by the grammar's test
pub const LITERAL_WORDS: &[&str] = &["false", "none", "true"];

// Constructs

/// Every construct of the language, by the name the language reference's coverage test knows it
/// by. The reference shows each one in an example that compiles.
#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
pub const CONSTRUCTS: &[&str] = &[
    "blueprint",
    "editor blueprint",
    "enum",
    "enum value with a label",
    "struct",
    "struct field with a default",
    "interface",
    "interface function returning a value",
    "use",
    "implements",
    "var",
    "var with a default",
    "editable var",
    "private var",
    "doc comment",
    "dispatcher",
    "fn",
    "event",
    "fn returning a value",
    "pure fn",
    "pure mut fn",
    "private fn",
    "protected fn",
    "ref parameter",
    "out parameter",
    "parameter with a default",
    "category",
    "widget",
    "widget placed on the canvas",
    "widget fields",
    "widget in a widget",
    "widget slot fields",
    "component",
    "component fields",
    "default",
    "on",
    "on with parameters",
    "on returning a value",
    "on a component's dispatcher",
    "bind",
    "unbind",
    "unbind_all",
    "super call",
    "let",
    "let with a type",
    "assign",
    "assign a member",
    "assign an item",
    "compound assignment",
    "return",
    "return a value",
    "while",
    "break",
    "continue",
    "labelled loop",
    "break a labelled loop",
    "continue a labelled loop",
    "if",
    "else",
    "else if",
    "if expression",
    "match",
    "match with else",
    "match expression",
    "for",
    "for with two names",
    "for over a count",
    "call as a statement",
    "integer",
    "float",
    "true",
    "false",
    "string",
    "none",
    "self",
    "name",
    "-x",
    "!x",
    "+",
    "-",
    "*",
    "/",
    "%",
    "<",
    "<=",
    ">",
    ">=",
    "==",
    "!=",
    "&&",
    "||",
    "&",
    "|",
    "call",
    "struct literal",
    "index",
    "empty array",
    "array literal",
    "map literal",
    "empty map",
    "path",
    "member",
    "method call",
    "array type",
    "set type",
    "map type",
    "class type",
    "soft type",
    "type by path",
];

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
impl Source {
    /// The constructs this source uses, by their names in [`CONSTRUCTS`].
    pub fn constructs(&self) -> std::collections::BTreeSet<&'static str> {
        match self {
            Source::Blueprint(file) => file.constructs(),
            Source::Enum(e) => {
                let mut out = std::collections::BTreeSet::from(["enum"]);
                if e.doc.is_some() || e.values.iter().any(|v| v.doc.is_some()) {
                    out.insert("doc comment");
                }
                if e.values.iter().any(|v| v.label.is_some()) {
                    out.insert("enum value with a label");
                }
                out
            }
            Source::Struct(s) => {
                let mut out = std::collections::BTreeSet::from(["struct"]);
                if s.doc.is_some() || s.fields.iter().any(|f| f.doc.is_some()) {
                    out.insert("doc comment");
                }
                for field in &s.fields {
                    type_constructs(&field.ty, &mut out);
                    if let Some(default) = &field.default {
                        out.insert("struct field with a default");
                        expr_constructs(default, &mut out);
                    }
                }
                out
            }
            Source::Interface(i) => {
                let mut out = std::collections::BTreeSet::from(["interface"]);
                if i.doc.is_some() || i.functions.iter().any(|f| f.doc.is_some()) {
                    out.insert("doc comment");
                }
                for function in &i.functions {
                    for param in &function.params {
                        type_constructs(&param.ty, &mut out);
                        if param.by_ref {
                            out.insert("ref parameter");
                        }
                    }
                    if let Some(ret) = &function.ret {
                        out.insert("interface function returning a value");
                        type_constructs(ret, &mut out);
                    }
                }
                out
            }
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
impl File {
    /// The constructs this file uses, by their names in [`CONSTRUCTS`]. Every match here names
    /// each variant, so a new one cannot be added without giving it a construct.
    pub fn constructs(&self) -> std::collections::BTreeSet<&'static str> {
        let mut out = std::collections::BTreeSet::new();
        out.insert(if self.editor { "editor blueprint" } else { "blueprint" });
        if self.parent.starts_with('/') {
            out.insert("type by path");
        }
        for item in &self.items {
            item_constructs(item, &mut out);
        }
        out
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
fn item_constructs(item: &Item, out: &mut std::collections::BTreeSet<&'static str>) {
    match item {
        Item::Var { ty, default, editable, private, doc, .. } => {
            out.insert("var");
            type_constructs(ty, out);
            if let Some(default) = default {
                out.insert("var with a default");
                expr_constructs(default, out);
            }
            if *editable {
                out.insert("editable var");
            }
            if *private {
                out.insert("private var");
            }
            if doc.is_some() {
                out.insert("doc comment");
            }
        }
        Item::Dispatcher { params, doc, .. } => {
            out.insert("dispatcher");
            params_constructs(params, out);
            if doc.is_some() {
                out.insert("doc comment");
            }
        }
        Item::Event { params, body, doc, .. } => {
            out.insert("event");
            params_constructs(params, out);
            block_constructs(body, out);
            if doc.is_some() {
                out.insert("doc comment");
            }
        }
        Item::Fn { params, ret, body, doc, access, pure, .. } => {
            out.insert("fn");
            match pure {
                Purity::Impure => {}
                Purity::Pure => {
                    out.insert("pure fn");
                }
                Purity::PureMut => {
                    out.insert("pure mut fn");
                }
            }
            match access {
                Access::Public => {}
                Access::Protected => {
                    out.insert("protected fn");
                }
                Access::Private => {
                    out.insert("private fn");
                }
            }
            params_constructs(params, out);
            if let Some(ret) = ret {
                out.insert("fn returning a value");
                type_constructs(ret, out);
            }
            if doc.is_some() {
                out.insert("doc comment");
            }
            block_constructs(body, out);
        }
        Item::Category { .. } => {
            out.insert("category");
        }
        Item::Use { .. } => {
            out.insert("use");
        }
        Item::Implements { interface, .. } => {
            out.insert("implements");
            type_constructs(interface, out);
        }
        Item::Widget { class, fields, slot, place, parent, .. } => {
            out.insert("widget");
            type_constructs(class, out);
            if place.is_some() {
                out.insert("widget placed on the canvas");
            }
            if !fields.is_empty() {
                out.insert("widget fields");
            }
            if parent.is_some() {
                out.insert("widget in a widget");
            }
            if !slot.is_empty() {
                out.insert("widget slot fields");
            }
            for (_, value) in fields.iter().chain(slot) {
                expr_constructs(value, out);
            }
        }
        Item::On { component, params, ret, body, .. } => {
            out.insert("on");
            if component.is_some() {
                out.insert("on a component's dispatcher");
            }
            if !params.is_empty() {
                out.insert("on with parameters");
            }
            params_constructs(params, out);
            if let Some(ret) = ret {
                out.insert("on returning a value");
                type_constructs(ret, out);
            }
            block_constructs(body, out);
        }
        Item::Component { class, fields, .. } => {
            out.insert("component");
            type_constructs(class, out);
            if !fields.is_empty() {
                out.insert("component fields");
            }
            for (_, value) in fields {
                expr_constructs(value, out);
            }
        }
        Item::Default { value, .. } => {
            out.insert("default");
            expr_constructs(value, out);
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
fn params_constructs(params: &[ParamDecl], out: &mut std::collections::BTreeSet<&'static str>) {
    for ParamDecl { ty, by_ref, out: is_out, default, .. } in params {
        type_constructs(ty, out);
        if *by_ref {
            out.insert("ref parameter");
        }
        if *is_out {
            out.insert("out parameter");
        }
        if let Some(default) = default {
            out.insert("parameter with a default");
            expr_constructs(default, out);
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
fn type_constructs(ty: &TypeName, out: &mut std::collections::BTreeSet<&'static str>) {
    // A type name is kept as text, such as map<string,array<int>>, so each part of it is looked for.
    for (prefix, construct) in
        [("array<", "array type"), ("set<", "set type"), ("map<", "map type"), ("class<", "class type"), ("soft<", "soft type")]
    {
        if ty.name.contains(prefix) {
            out.insert(construct);
        }
    }
    if ty.name.starts_with('/') || ty.name.contains("</") || ty.name.contains(",/") {
        out.insert("type by path");
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
fn block_constructs(body: &[Stmt], out: &mut std::collections::BTreeSet<&'static str>) {
    for stmt in body {
        match stmt {
            Stmt::Let { ty, value, .. } => {
                out.insert("let");
                if let Some(ty) = ty {
                    out.insert("let with a type");
                    type_constructs(ty, out);
                }
                expr_constructs(value, out);
            }
            Stmt::Assign { target, value, .. } => {
                out.insert(match target.kind {
                    ExprKind::Member(..) => "assign a member",
                    ExprKind::Index(..) => "assign an item",
                    _ => "assign",
                });
                expr_constructs(target, out);
                expr_constructs(value, out);
            }
            Stmt::Return { value, .. } => {
                out.insert("return");
                if let Some(value) = value {
                    out.insert("return a value");
                    expr_constructs(value, out);
                }
            }
            Stmt::While { cond, body, .. } => {
                out.insert("while");
                expr_constructs(cond, out);
                block_constructs(body, out);
            }
            Stmt::Break { label, .. } => {
                out.insert(if label.is_some() { "break a labelled loop" } else { "break" });
            }
            Stmt::Continue { label, .. } => {
                out.insert(if label.is_some() { "continue a labelled loop" } else { "continue" });
            }
            Stmt::Labelled { looped, .. } => {
                out.insert("labelled loop");
                block_constructs(std::slice::from_ref(looped), out);
            }
            Stmt::For { names, items, body, .. } => {
                // Two names over a map are its key and value, and over an array its item and index,
                // which only lowering, knowing the type, tells apart.
                out.insert(if names.len() == 2 { "for with two names" } else { "for" });
                expr_constructs(items, out);
                block_constructs(body, out);
            }
            Stmt::Compound { target, value, .. } => {
                out.insert("compound assignment");
                expr_constructs(target, out);
                expr_constructs(value, out);
            }
            Stmt::Count { first, last, body, .. } => {
                out.insert("for over a count");
                expr_constructs(first, out);
                expr_constructs(last, out);
                block_constructs(body, out);
            }
            Stmt::If { cond, then, otherwise, .. } => {
                out.insert("if");
                expr_constructs(cond, out);
                block_constructs(then, out);
                match otherwise.as_slice() {
                    [] => {}
                    [Stmt::If { .. }] => {
                        out.insert("else if");
                    }
                    _ => {
                        out.insert("else");
                    }
                }
                block_constructs(otherwise, out);
            }
            Stmt::Match { value, arms, otherwise, .. } => {
                out.insert("match");
                expr_constructs(value, out);
                for (values, body) in arms {
                    values.iter().for_each(|v| expr_constructs(v, out));
                    block_constructs(body, out);
                }
                if let Some(otherwise) = otherwise {
                    out.insert("match with else");
                    block_constructs(otherwise, out);
                }
            }
            Stmt::Expr(e) => {
                out.insert("call as a statement");
                expr_constructs(e, out);
            }
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))] // read only by the reference's tests
fn expr_constructs(e: &Expr, out: &mut std::collections::BTreeSet<&'static str>) {
    match &e.kind {
        ExprKind::Int(_) => {
            out.insert("integer");
        }
        ExprKind::Float(_) => {
            out.insert("float");
        }
        ExprKind::Bool(b) => {
            out.insert(if *b { "true" } else { "false" });
        }
        ExprKind::Str(_) => {
            out.insert("string");
        }
        ExprKind::Null => {
            out.insert("none");
        }
        ExprKind::Name(name) => {
            out.insert(if name == "self" { "self" } else { "name" });
        }
        ExprKind::Unary(op, operand) => {
            out.insert(match op {
                UnaryOp::Neg => "-x",
                UnaryOp::Not => "!x",
            });
            expr_constructs(operand, out);
        }
        ExprKind::Binary(op, a, b) => {
            out.insert(op.symbol());
            expr_constructs(a, out);
            expr_constructs(b, out);
        }
        ExprKind::Call(_, args, _) => {
            out.insert("call");
            args.iter().for_each(|a| expr_constructs(a, out));
        }
        ExprKind::Make(_, fields) => {
            out.insert("struct literal");
            fields.iter().for_each(|(_, v)| expr_constructs(v, out));
        }
        ExprKind::Index(a, i) => {
            out.insert("index");
            expr_constructs(a, out);
            expr_constructs(i, out);
        }
        ExprKind::EmptyArray => {
            out.insert("empty array");
        }
        ExprKind::ArrayLiteral(items) => {
            out.insert("array literal");
            items.iter().for_each(|i| expr_constructs(i, out));
        }
        ExprKind::MapLiteral(entries) => {
            out.insert(if entries.is_empty() { "empty map" } else { "map literal" });
            for (k, v) in entries {
                expr_constructs(k, out);
                expr_constructs(v, out);
            }
        }
        ExprKind::Path(_) => {
            out.insert("path");
        }
        ExprKind::Match(value, arms, otherwise) => {
            out.insert("match expression");
            expr_constructs(value, out);
            for (values, arm) in arms {
                values.iter().for_each(|v| expr_constructs(v, out));
                expr_constructs(arm, out);
            }
            expr_constructs(otherwise, out);
        }
        ExprKind::If(cond, a, b) => {
            out.insert("if expression");
            expr_constructs(cond, out);
            expr_constructs(a, out);
            expr_constructs(b, out);
        }
        ExprKind::Member(object, _) => {
            out.insert("member");
            expr_constructs(object, out);
        }
        ExprKind::Method(object, name, args) => {
            out.insert(if matches!(&object.kind, ExprKind::Name(n) if n == "super") { "super call" } else { "method call" });
            // The methods that bind a function to a dispatcher and unbind it.
            if let Some(method) = ["bind", "unbind", "unbind_all"].into_iter().find(|m| m == name) {
                out.insert(method);
            }
            expr_constructs(object, out);
            args.iter().for_each(|a| expr_constructs(a, out));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// A name in backticks, which is never a word of the language.
    Quoted(String),
    /// An asset or class path, such as /Script/Engine.Actor or /Game/Loom/BP_Sum.
    Path(String),
    Int(i64),
    Float(f64),
    Str(String),
    Punct(&'static str),
    Newline,
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "'{s}'"),
            Tok::Quoted(s) => write!(f, "'`{s}`'"),
            Tok::Path(s) => write!(f, "path {s}"),
            Tok::Int(n) => write!(f, "{n}"),
            Tok::Float(n) => write!(f, "{n}"),
            Tok::Str(s) => write!(f, "\"{s}\""),
            Tok::Punct(p) => write!(f, "'{p}'"),
            Tok::Newline => f.write_str("end of line"),
            Tok::Eof => f.write_str("end of file"),
        }
    }
}

const PUNCT: [&str; 35] = [
    "[", "]", "->", "..", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "<=", ">=", "==", "!=", "&&", "||", "&", "|", "+", "-", "*", "/", "%", "<", ">", "=", "!", "(", ")", "{", "}", ",",
    ":", ".",
];

fn lex(src: &str) -> Result<Vec<(Tok, Span)>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut col) = (0, 1u32, 1u32);
    while i < chars.len() {
        let c = chars[i];
        let span = Span { line, col };
        let start = i;
        // A line ends in \n, or in \r\n as a Windows checkout may have it, one ending either way.
        let ending = match (c, chars.get(i + 1)) {
            ('\n', _) => 1,
            ('\r', Some('\n')) => 2,
            _ => 0,
        };
        if ending > 0 {
            out.push((Tok::Newline, span));
            i += ending;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let tok = if c == '/' && chars.get(i + 1).is_some_and(|n| n.is_ascii_uppercase()) {
            // A path starts with a slash and a capital: /Script/..., /Game/...
            while i < chars.len() && (chars[i].is_alphanumeric() || "/_.-".contains(chars[i])) {
                i += 1;
            }
            Tok::Path(chars[start..i].iter().collect())
        } else if c.is_ascii_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            Tok::Ident(chars[start..i].iter().collect())
        } else if c == '0' && matches!(chars.get(i + 1), Some('x' | 'X')) {
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            let text: String = chars[start + 2..i].iter().collect();
            match i64::from_str_radix(&text, 16) {
                Ok(n) => Tok::Int(n),
                Err(_) => bail!("{span}: 0x{text} is not a hex number"),
            }
        } else if c.is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let is_float = chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
            if is_float {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().collect();
            if is_float {
                Tok::Float(text.parse()?)
            } else {
                match text.parse() {
                    Ok(n) => Tok::Int(n),
                    Err(_) => bail!("{span}: integer {text} is too large"),
                }
            }
        } else if c == '`' {
            i += 1;
            while i < chars.len() && !matches!(chars[i], '`' | '\n' | '\r') {
                i += 1;
            }
            if chars.get(i) != Some(&'`') {
                bail!("{span}: a name in backticks needs its closing backtick on the same line");
            }
            if i == start + 1 {
                bail!("{span}: a name in backticks cannot be empty");
            }
            i += 1;
            Tok::Quoted(chars[start + 1..i - 1].iter().collect())
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match chars.get(i) {
                    None | Some('\n') => bail!("{span}: unterminated string"),
                    Some('"') => break,
                    Some('\\') => {
                        match chars.get(i + 1) {
                            Some('n') => s.push('\n'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            _ => bail!("{span}: unknown escape in string"),
                        }
                        i += 2;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            i += 1;
            Tok::Str(s)
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            match PUNCT.iter().find(|p| rest.starts_with(**p)) {
                Some(p) => {
                    i += p.len();
                    Tok::Punct(p)
                }
                None => bail!("{span}: unexpected character '{c}'"),
            }
        };
        col += (i - start) as u32;
        out.push((tok, span));
    }
    out.push((Tok::Eof, Span { line, col }));
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Parser

/// A source file, parsed, with every `include("file")` in it replaced by what the file holds.
pub fn parse_file(path: &std::path::Path) -> Result<Source> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    parse_text(&text, path)
}

/// A source file's text, parsed as [`parse_file`] parses what the file holds: its includes are
/// read from beside `path`.
pub fn parse_text(text: &str, path: &std::path::Path) -> Result<Source> {
    let mut source = parse_source(text)?;
    if let Source::Blueprint(file) = &mut source {
        resolve_includes(file, path.parent().unwrap_or(std::path::Path::new(".")))?;
    }
    Ok(source)
}

/// Replaces every `include("file")` in a parsed source with what the file holds, reading paths
/// relative to `dir`, the source's folder.
pub fn resolve_includes(file: &mut File, dir: &std::path::Path) -> Result<()> {
    for item in &mut file.items {
        if let Item::Var { default: Some(value), .. } = item {
            if let ExprKind::Call(name, args, _) = &value.kind {
                if name == "include" {
                    *value = include(dir, args, value.span)?;
                }
            }
        }
    }
    Ok(())
}

/// A variable's default read from a JSON file at compile time, as the literal it would be written
/// as: an object as a map, an array as an array. Its path is relative to the source.
fn include(dir: &std::path::Path, args: &[Expr], span: Span) -> Result<Expr> {
    let [Expr { kind: ExprKind::Str(file), .. }] = args else {
        bail!("{span}: include() takes the path of a JSON file, as a string");
    };
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("{span}: reading {}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{span}: {} is not JSON: {e}", path.display()))?;
    json_literal(&json, span).map_err(|e| anyhow::anyhow!("{span}: in {}: {e}", path.display()))
}

fn json_literal(json: &serde_json::Value, span: Span) -> Result<Expr> {
    use serde_json::Value;
    let kind = match json {
        Value::Bool(b) => ExprKind::Bool(*b),
        Value::String(s) => ExprKind::Str(s.clone()),
        Value::Number(n) => match n.as_i64() {
            Some(i) => ExprKind::Int(i),
            None => ExprKind::Float(n.as_f64().ok_or_else(|| anyhow::anyhow!("{n} is out of range"))?),
        },
        Value::Array(items) if items.is_empty() => ExprKind::EmptyArray,
        Value::Array(items) => ExprKind::ArrayLiteral(items.iter().map(|i| json_literal(i, span)).collect::<Result<_>>()?),
        Value::Object(entries) => ExprKind::MapLiteral(
            entries
                .iter()
                .map(|(k, v)| Ok((Expr { kind: ExprKind::Str(k.clone()), span }, json_literal(v, span)?)))
                .collect::<Result<_>>()?,
        ),
        Value::Null => bail!("null has no literal; leave the entry out"),
    };
    Ok(Expr { kind, span })
}

/// A source: a Blueprint's or an enum's, as its header says.
pub fn parse_source(src: &str) -> Result<Source> {
    let mut source = Parser { toks: lex(src)?, pos: 0 }.source()?;
    let lines: Vec<&str> = src.lines().collect();
    match &mut source {
        Source::Blueprint(file) => {
            for item in &mut file.items {
                if let Item::Fn { doc, span, .. } | Item::Event { doc, span, .. } | Item::Var { doc, span, .. } | Item::Dispatcher { doc, span, .. } = item {
                    *doc = doc_above(&lines, span.line as usize);
                }
            }
        }
        Source::Enum(e) => {
            e.doc = doc_above(&lines, e.span.line as usize);
            for value in &mut e.values {
                value.doc = doc_above(&lines, value.span.line as usize);
            }
        }
        Source::Struct(s) => {
            s.doc = doc_above(&lines, s.span.line as usize);
            for field in &mut s.fields {
                field.doc = doc_above(&lines, field.span.line as usize);
            }
        }
        Source::Interface(i) => {
            i.doc = doc_above(&lines, i.span.line as usize);
            for function in &mut i.functions {
                function.doc = doc_above(&lines, function.span.line as usize);
            }
        }
    }
    Ok(source)
}

/// A Blueprint's source.
#[cfg_attr(not(test), allow(dead_code))] // read only by tests
pub fn parse(src: &str) -> Result<File> {
    match parse_source(src)? {
        Source::Blueprint(file) => Ok(file),
        Source::Enum(e) => bail!("{}: expected a Blueprint's source, found the enum {}", e.span, e.name),
        Source::Struct(s) => bail!("{}: expected a Blueprint's source, found the struct {}", s.span, s.name),
        Source::Interface(i) => bail!("{}: expected a Blueprint's source, found the interface {}", i.span, i.name),
    }
}

/// The comment lines directly above line `line` (counted from 1), with no blank line between,
/// as one text. A `//` line with nothing after it starts a new paragraph.
fn doc_above(lines: &[&str], line: usize) -> Option<String> {
    let mut found = Vec::new();
    for text in lines[..line.saturating_sub(1)].iter().rev() {
        let Some(comment) = text.trim().strip_prefix("//") else { break };
        found.push(comment.strip_prefix(' ').unwrap_or(comment).trim_end());
    }
    found.reverse();
    let mut doc = String::new();
    for text in found {
        if text.is_empty() {
            doc.push_str("\n\n");
        } else {
            if !doc.is_empty() && !doc.ends_with('\n') {
                doc.push(' ');
            }
            doc.push_str(text);
        }
    }
    let doc = doc.trim().to_string();
    (!doc.is_empty()).then_some(doc)
}

struct Parser {
    toks: Vec<(Tok, Span)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    /// The label a `break` or `continue` names, if it names one.
    fn label(&mut self) -> Result<Option<String>> {
        Ok(match self.peek() {
            Tok::Ident(_) | Tok::Quoted(_) => Some(self.ident()?.0),
            _ => None,
        })
    }

    /// The token after the next one.
    fn peek_second(&self) -> &Tok {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)].0
    }

    fn span(&self) -> Span {
        self.toks[self.pos].1
    }

    fn next(&mut self) -> (Tok, Span) {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn is_keyword(&self, k: &str) -> bool {
        debug_assert!(RESERVED.contains(&k) || CONTEXTUAL.contains(&k), "'{k}' is a word missing from syntax::RESERVED and syntax::CONTEXTUAL");
        matches!(self.peek(), Tok::Ident(s) if s == k)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        let found = self.is_punct(p);
        if found {
            self.next();
        }
        found
    }

    fn expect_punct(&mut self, p: &str) -> Result<()> {
        if !self.eat_punct(p) {
            bail!("{}: expected '{p}', found {}", self.span(), self.peek());
        }
        Ok(())
    }

    fn expect_keyword(&mut self, k: &str) -> Result<()> {
        if !self.is_keyword(k) {
            bail!("{}: expected '{k}', found {}", self.span(), self.peek());
        }
        self.next();
        Ok(())
    }

    fn ident(&mut self) -> Result<(String, Span)> {
        match self.next() {
            (Tok::Ident(s) | Tok::Quoted(s), span) => Ok((s, span)),
            (t, span) => bail!("{span}: expected a name, found {t}"),
        }
    }

    /// Fails where a function is declared with a built-in function's name not in backticks, which
    /// every call would take for the built-in.
    fn not_builtin(&self) -> Result<()> {
        if let Tok::Ident(word) = self.peek() {
            if crate::builtins::function(word).is_some() {
                bail!("{}: {word} is a built-in function; write `{word}` in backticks to declare a function of that name, and to call it", self.span());
            }
        }
        Ok(())
    }

    /// A name the source declares. Names starting with `Loom_` are kept for the ones loom makes
    /// itself, such as event handlers and hidden locals, so a declared one cannot clash with them.
    fn declared(&mut self) -> Result<(String, Span)> {
        if let Tok::Ident(word) = self.peek() {
            if is_reserved(word) {
                bail!("{}: {word} is a reserved word; write `{word}` in backticks for a name", self.span());
            }
        }
        let (name, span) = self.ident()?;
        if SELF_WORDS.contains(&name.as_str()) {
            bail!("{span}: {name} is always this object, so nothing can be declared {name}");
        }
        if name.starts_with(RESERVED_PREFIX) {
            bail!("{span}: {name} starts with {RESERVED_PREFIX}, which loom keeps for names it makes itself");
        }
        Ok((name, span))
    }

    fn int(&mut self) -> Result<i64> {
        match self.next() {
            (Tok::Int(n), _) => Ok(n),
            (t, span) => bail!("{span}: expected a whole number, found {t}"),
        }
    }

    fn path(&mut self) -> Result<String> {
        match self.next() {
            (Tok::Path(s), _) => Ok(s),
            (t, span) => bail!("{span}: expected a path such as /Script/Engine.Actor, found {t}"),
        }
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Tok::Newline {
            self.next();
        }
    }

    fn end_of_line(&mut self) -> Result<()> {
        match self.peek() {
            Tok::Newline => {
                self.skip_newlines();
                Ok(())
            }
            Tok::Eof => Ok(()),
            Tok::Punct("}") => Ok(()),
            t => bail!("{}: expected the end of the line, found {t}", self.span()),
        }
    }

    fn source(&mut self) -> Result<Source> {
        self.skip_newlines();
        if self.is_keyword("enum") {
            return Ok(Source::Enum(self.enumeration()?));
        }
        if self.is_keyword("struct") {
            return Ok(Source::Struct(self.structure()?));
        }
        if self.is_keyword("interface") {
            return Ok(Source::Interface(self.interface()?));
        }
        Ok(Source::Blueprint(self.file()?))
    }

    /// `enum Name at /Game/Path { ... }`, one value on each line, each a name and, when the editor
    /// shows it otherwise, a label.
    fn enumeration(&mut self) -> Result<Enum> {
        let span = self.span();
        self.expect_keyword("enum")?;
        let (name, _) = self.declared()?;
        self.expect_keyword("at")?;
        let path = self.path()?;
        self.expect_punct("{")?;
        self.skip_newlines();
        let mut values = Vec::new();
        while !self.eat_punct("}") {
            let (name, span) = self.declared()?;
            let label = match self.peek() {
                Tok::Str(_) => match self.next() {
                    (Tok::Str(label), _) => Some(label),
                    _ => unreachable!(),
                },
                _ => None,
            };
            values.push(EnumValue { name, label, doc: None, span });
            self.end_of_line()?;
        }
        self.skip_newlines();
        if *self.peek() != Tok::Eof {
            bail!("{}: an enum's source holds only the enum, found {}", self.span(), self.peek());
        }
        Ok(Enum { name, path, values, doc: None, span })
    }

    /// `struct Name at /Game/Path { ... }`, one field on each line.
    fn structure(&mut self) -> Result<Struct> {
        let span = self.span();
        self.expect_keyword("struct")?;
        let (name, _) = self.declared()?;
        self.expect_keyword("at")?;
        let path = self.path()?;
        self.expect_punct("{")?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !self.eat_punct("}") {
            let (name, span) = self.declared()?;
            self.expect_punct(":")?;
            let ty = self.type_name()?;
            let default = if self.eat_punct("=") { Some(self.expr()?) } else { None };
            fields.push(Field { name, ty, default, doc: None, span });
            self.end_of_line()?;
        }
        self.skip_newlines();
        if *self.peek() != Tok::Eof {
            bail!("{}: a struct's source holds only the struct, found {}", self.span(), self.peek());
        }
        Ok(Struct { name, path, fields, doc: None, span })
    }

    /// `interface Name at /Game/Path { ... }`, one function's signature on each line.
    fn interface(&mut self) -> Result<Interface> {
        let span = self.span();
        self.expect_keyword("interface")?;
        let (name, _) = self.declared()?;
        self.expect_keyword("at")?;
        let path = self.path()?;
        self.expect_punct("{")?;
        self.skip_newlines();
        let mut functions = Vec::new();
        while !self.eat_punct("}") {
            let span = self.span();
            self.expect_keyword("fn")?;
            let (name, _) = self.declared()?;
            let params = self.params(false)?;
            let ret = if self.eat_punct("->") { Some(self.type_name()?) } else { None };
            if self.is_punct("{") {
                bail!("{}: an interface's function has no body; the Blueprints that implement it give it one", self.span());
            }
            functions.push(Signature { name, params, ret, doc: None, span });
            self.end_of_line()?;
        }
        self.skip_newlines();
        if *self.peek() != Tok::Eof {
            bail!("{}: an interface's source holds only the interface, found {}", self.span(), self.peek());
        }
        Ok(Interface { name, path, functions, doc: None, span })
    }

    fn file(&mut self) -> Result<File> {
        let editor = self.is_keyword("editor");
        if editor {
            self.next();
        }
        self.expect_keyword("blueprint")?;
        let (name, _) = self.ident()?;
        self.expect_punct(":")?;
        let parent = self.type_name()?.name;
        self.expect_keyword("at")?;
        let path = self.path()?;
        self.end_of_line()?;
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            if self.is_keyword("widget") {
                // A widget's children follow it.
                let (widget, children) = self.widget(None)?;
                self.end_of_line()?;
                items.push(widget);
                items.extend(children);
            } else {
                let item = self.item()?;
                if let Item::Use { span, .. } = &item {
                    if items.iter().any(|i| !matches!(i, Item::Use { .. })) {
                        bail!("{span}: use comes before the Blueprint's other items");
                    }
                }
                if let Item::Implements { span, .. } = &item {
                    if items.iter().any(|i| !matches!(i, Item::Use { .. } | Item::Implements { .. })) {
                        bail!("{span}: implements comes after the header and any use, before the Blueprint's other items");
                    }
                }
                items.push(item);
            }
        }
        Ok(File { editor, name, parent, path, items })
    }

    fn type_name(&mut self) -> Result<TypeName> {
        match self.next() {
            (Tok::Ident(name), span) if name != "map" && TYPE_WORDS.contains(&name.as_str()) => {
                self.expect_punct("<")?;
                let inner = self.type_name()?;
                self.expect_punct(">")?;
                Ok(TypeName { name: format!("{name}<{}>", inner.name), span })
            }
            (Tok::Ident(name), span) if name == "map" => {
                self.expect_punct("<")?;
                let key = self.type_name()?;
                self.expect_punct(",")?;
                let value = self.type_name()?;
                self.expect_punct(">")?;
                Ok(TypeName { name: format!("map<{},{}>", key.name, value.name), span })
            }
            (Tok::Ident(name) | Tok::Quoted(name) | Tok::Path(name), span) => Ok(TypeName { name, span }),
            (t, span) => bail!("{span}: expected a type, found {t}"),
        }
    }

    /// Parameters in parentheses. With `defaults`, a function's, each may have a default, `= value`,
    /// which every one after it has too. `out` before a name makes it an output, which a call does
    /// not pass, so it is neither a ref nor given a default, and may come anywhere in the list.
    fn params(&mut self, defaults: bool) -> Result<Vec<ParamDecl>> {
        self.expect_punct("(")?;
        let mut params: Vec<ParamDecl> = Vec::new();
        while !self.is_punct(")") {
            // `out` is only a word here when a name follows it, so `out: int` is still a
            // parameter called out.
            let out = self.is_keyword("out") && matches!(self.peek_second(), Tok::Ident(_) | Tok::Quoted(_));
            if out {
                self.next();
            }
            let (name, span) = self.declared()?;
            self.expect_punct(":")?;
            let by_ref = self.is_keyword("ref");
            if by_ref {
                if out {
                    bail!("{}: {name} is an output, which the function gives, so it cannot be ref", self.span());
                }
                self.next();
            }
            let ty = self.type_name()?;
            let default = if self.is_punct("=") {
                let at = self.span();
                if !defaults {
                    bail!("{at}: only a function's parameters have defaults");
                }
                if by_ref {
                    bail!("{at}: {name} is a ref, the caller's own variable, so it has no default");
                }
                if out {
                    bail!("{at}: {name} is an output, which a call does not pass, so it has no default");
                }
                self.next();
                Some(self.expr()?)
            } else {
                if !out && params.iter().any(|p| p.default.is_some()) {
                    bail!("{span}: {name} comes after a parameter with a default, so it needs a default too");
                }
                None
            };
            params.push(ParamDecl { name, ty, by_ref, out, default });
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        Ok(params)
    }

    fn item(&mut self) -> Result<Item> {
        let span = self.span();
        let editable = self.is_keyword("editable");
        if editable {
            self.next();
            if !self.is_keyword("var") {
                bail!("{}: expected 'var' after 'editable', found {}", self.span(), self.peek());
            }
        }
        // A function's access, then whether it is pure, in that order, as `private pure fn`.
        let access = if self.is_keyword("private") {
            self.next();
            Access::Private
        } else if self.is_keyword("protected") {
            self.next();
            Access::Protected
        } else {
            Access::Public
        };
        let pure = if self.is_keyword("pure") {
            self.next();
            if self.is_keyword("mut") {
                self.next();
                Purity::PureMut
            } else {
                Purity::Pure
            }
        } else {
            Purity::Impure
        };
        // A variable may be private too, which only this Blueprint can read or set.
        let private_var = access == Access::Private && pure == Purity::Impure && !editable && self.is_keyword("var");
        if (access != Access::Public || pure != Purity::Impure) && !self.is_keyword("fn") && !private_var {
            if self.is_keyword("private") || self.is_keyword("protected") {
                bail!("{}: write the access first, then pure: private pure fn", self.span());
            }
            bail!("{}: expected 'fn', found {}", self.span(), self.peek());
        }
        let item = if self.is_keyword("var") {
            self.next();
            let (name, _) = self.declared()?;
            self.expect_punct(":")?;
            let ty = self.type_name()?;
            let default = if self.eat_punct("=") { Some(self.expr()?) } else { None };
            Item::Var { name, ty, default, editable, private: private_var, doc: None, span }
        } else if self.is_keyword("dispatcher") {
            self.next();
            let (name, _) = self.declared()?;
            let params = self.params(false)?;
            if let Some(p) = params.iter().find(|p| p.by_ref) {
                bail!("{span}: a dispatcher passes values, so {} cannot be ref", p.name);
            }
            if let Some(p) = params.iter().find(|p| p.out) {
                bail!("{span}: a dispatcher passes values and gives nothing back, so {} cannot be out", p.name);
            }
            Item::Dispatcher { name, params, doc: None, span }
        } else if self.is_keyword("fn") {
            self.next();
            self.not_builtin()?;
            let (name, _) = self.declared()?;
            let params = self.params(true)?;
            let ret = if self.eat_punct("->") { Some(self.type_name()?) } else { None };
            Item::Fn { name, params, ret, body: self.block()?, doc: None, access, pure, span }
        } else if self.is_keyword("use") {
            self.next();
            let folder = self.path()?;
            if folder.contains('.') {
                bail!("{span}: use takes a folder, such as /Game/Libs/Inventory, not {folder}");
            }
            Item::Use { folder: folder.trim_end_matches('/').to_string(), span }
        } else if self.is_keyword("implements") {
            self.next();
            Item::Implements { interface: self.type_name()?, span }
        } else if self.is_keyword("category") {
            self.next();
            let name = match self.next() {
                (Tok::Str(name), _) => name,
                (t, span) => bail!("{span}: expected the category in quotes, found {t}"),
            };
            Item::Category { name, span }
        } else if self.is_keyword("event") {
            self.next();
            self.not_builtin()?;
            let (name, _) = self.declared()?;
            let params = if self.is_punct("(") { self.params(false)? } else { Vec::new() };
            if let Some(p) = params.iter().find(|p| p.by_ref || p.out) {
                bail!("{span}: an event's parameters are values it is passed, so {} cannot be {}", p.name, if p.by_ref { "ref" } else { "out" });
            }
            if self.is_punct("->") {
                bail!("{}: an event gives nothing back; a `fn` returns a value", self.span());
            }
            Item::Event { name, params, body: self.block()?, doc: None, span }
        } else if self.is_keyword("on") {
            self.next();
            let (first, _) = self.ident()?;
            let (component, event) = if self.eat_punct(".") { (Some(first), self.ident()?.0) } else { (None, first) };
            let params = if self.is_punct("(") { self.params(false)? } else { Vec::new() };
            let ret = if self.eat_punct("->") { Some(self.type_name()?) } else { None };
            Item::On { component, event, params, ret, body: self.block()?, span }
        } else if self.is_keyword("component") {
            self.next();
            let (name, _) = self.declared()?;
            self.expect_punct(":")?;
            let class = self.type_name()?;
            let fields = if self.is_punct("(") { self.fields()? } else { Vec::new() };
            Item::Component { name, class, fields, span }
        } else if self.is_keyword("default") {
            self.next();
            let mut path = vec![self.ident()?.0];
            while self.eat_punct(".") {
                path.push(self.ident()?.0);
            }
            self.expect_punct("=")?;
            Item::Default { path, value: self.expr()?, span }
        } else {
            bail!("{span}: expected 'var', 'fn', 'event', 'dispatcher', 'component', 'default', 'category', 'widget' or 'on', found {}", self.peek());
        };
        self.end_of_line()?;
        Ok(item)
    }

    /// A widget, and after it every widget of its block, in the order they are written.
    fn widget(&mut self, parent: Option<String>) -> Result<(Item, Vec<Item>)> {
        let span = self.span();
        self.expect_keyword("widget")?;
        let (name, _) = self.declared()?;
        self.expect_punct(":")?;
        let class = self.type_name()?;
        let fields = if self.is_punct("(") { self.fields()? } else { Vec::new() };
        let place = if self.is_keyword("at") {
            self.next();
            let (x, y) = (self.int()?, self.int()?);
            self.expect_keyword("size")?;
            Some([x, y, self.int()?, self.int()?])
        } else {
            None
        };
        let slot = if self.is_keyword("slot") {
            self.next();
            self.fields()?
        } else {
            Vec::new()
        };
        let mut children = Vec::new();
        if self.eat_punct("{") {
            self.skip_newlines();
            while !self.eat_punct("}") {
                if !self.is_keyword("widget") {
                    bail!("{}: a widget's block holds only widgets, found {}", self.span(), self.peek());
                }
                let (child, mut descendants) = self.widget(Some(name.clone()))?;
                children.push(child);
                children.append(&mut descendants);
                self.end_of_line()?;
            }
        }
        Ok((Item::Widget { name, class, fields, slot, place, parent, span }, children))
    }

    /// `(field: value, ...)`, which may span lines.
    fn fields(&mut self) -> Result<Vec<(String, Expr)>> {
        self.expect_punct("(")?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !self.is_punct(")") {
            let (field, _) = self.ident()?;
            self.expect_punct(":")?;
            fields.push((field, self.expr()?));
            self.skip_newlines();
            if !self.eat_punct(",") {
                break;
            }
            self.skip_newlines();
        }
        self.expect_punct(")")?;
        Ok(fields)
    }

    fn block(&mut self) -> Result<Vec<Stmt>> {
        self.expect_punct("{")?;
        self.skip_newlines();
        let mut body = Vec::new();
        while !self.eat_punct("}") {
            body.push(self.stmt()?);
            self.end_of_line()?;
        }
        Ok(body)
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let span = self.span();
        // `label: for ...`: a name and a colon start only a labelled loop.
        if matches!(self.peek(), Tok::Ident(_) | Tok::Quoted(_)) && self.peek_second() == &Tok::Punct(":") {
            let (label, _) = self.declared()?;
            self.expect_punct(":")?;
            if !self.is_keyword("for") && !self.is_keyword("while") {
                bail!("{span}: a label names a loop, so a for or a while follows it, not {}", self.peek());
            }
            let looped = Box::new(self.stmt()?);
            return Ok(Stmt::Labelled { label, looped, span });
        }
        if self.is_keyword("let") {
            self.next();
            let (name, _) = self.declared()?;
            let ty = if self.eat_punct(":") { Some(self.type_name()?) } else { None };
            self.expect_punct("=")?;
            return Ok(Stmt::Let { name, ty, value: self.expr()?, span });
        }
        if self.is_keyword("while") {
            self.next();
            let cond = self.expr()?;
            return Ok(Stmt::While { cond, body: self.block()?, span });
        }
        if self.is_keyword("if") {
            return self.if_stmt();
        }
        if self.is_keyword("match") {
            self.next();
            let value = self.expr()?;
            let (arms, otherwise) = self.arms(Self::block)?;
            return Ok(Stmt::Match { value, arms, otherwise, span });
        }
        if self.is_keyword("for") {
            self.next();
            let mut names = vec![self.declared()?.0];
            if self.eat_punct(",") {
                names.push(self.declared()?.0);
            }
            self.expect_keyword("in")?;
            let items = self.expr()?;
            if self.eat_punct("..") {
                let [name] = &names[..] else {
                    bail!("{span}: a count names one number: for i in 1..10");
                };
                let last = self.expr()?;
                return Ok(Stmt::Count { name: name.clone(), first: items, last, body: self.block()?, span });
            }
            return Ok(Stmt::For { names, items, body: self.block()?, span });
        }
        if self.is_keyword("return") {
            self.next();
            let value = match self.peek() {
                Tok::Newline | Tok::Eof | Tok::Punct("}") => None,
                _ => Some(self.expr()?),
            };
            return Ok(Stmt::Return { value, span });
        }
        if self.is_keyword("break") {
            self.next();
            return Ok(Stmt::Break { label: self.label()?, span });
        }
        if self.is_keyword("continue") {
            self.next();
            return Ok(Stmt::Continue { label: self.label()?, span });
        }
        let expr = self.expr()?;
        let assignable = matches!(expr.kind, ExprKind::Name(_) | ExprKind::Member(..) | ExprKind::Index(..));
        if self.eat_punct("=") {
            if !assignable {
                bail!("{span}: only a variable or a member can be assigned");
            }
            return Ok(Stmt::Assign { target: expr, value: self.expr()?, span });
        }
        let compound = [
            ("+=", BinaryOp::Add),
            ("-=", BinaryOp::Sub),
            ("*=", BinaryOp::Mul),
            ("/=", BinaryOp::Div),
            ("%=", BinaryOp::Rem),
            ("&=", BinaryOp::BitAnd),
            ("|=", BinaryOp::BitOr),
        ];
        if let Some((_, op)) = compound.into_iter().find(|(p, _)| self.is_punct(p)) {
            self.next();
            if !assignable {
                bail!("{span}: only a variable or a member can be assigned");
            }
            return Ok(Stmt::Compound { target: expr, op, value: self.expr()?, span });
        }
        if !matches!(expr.kind, ExprKind::Call(..) | ExprKind::Method(..)) {
            bail!("{span}: an expression on its own does nothing; only a call can stand alone");
        }
        Ok(Stmt::Expr(expr))
    }

    fn if_stmt(&mut self) -> Result<Stmt> {
        let span = self.span();
        self.expect_keyword("if")?;
        let cond = self.expr()?;
        let then = self.block()?;
        let otherwise = if self.is_keyword("else") {
            self.next();
            if self.is_keyword("if") { vec![self.if_stmt()?] } else { self.block()? }
        } else {
            Vec::new()
        };
        Ok(Stmt::If { cond, then, otherwise, span })
    }

    fn expr(&mut self) -> Result<Expr> {
        self.binary(0)
    }

    /// The rest of `if condition { a } else { b }`, after the `if`, where a value is expected.
    fn if_expr(&mut self, span: Span) -> Result<Expr> {
        let cond = self.expr()?;
        let then = self.braced_expr()?;
        if !self.is_keyword("else") {
            bail!("{span}: an `if` that gives a value needs an `else`, for when the condition doesn't hold");
        }
        self.next();
        let otherwise = if self.is_keyword("if") {
            let span = self.span();
            self.next();
            self.if_expr(span)?
        } else {
            self.braced_expr()?
        };
        Ok(Expr { kind: ExprKind::If(Box::new(cond), Box::new(then), Box::new(otherwise)), span })
    }

    /// A match's arms, each the values it matches and what `side` reads after them, then an `else`
    /// arm when there is one, which comes last.
    #[allow(clippy::type_complexity)]
    fn arms<T>(&mut self, side: fn(&mut Self) -> Result<T>) -> Result<(Vec<(Vec<Expr>, T)>, Option<T>)> {
        self.expect_punct("{")?;
        self.skip_newlines();
        let mut arms = Vec::new();
        let mut otherwise = None;
        while !self.eat_punct("}") {
            if otherwise.is_some() {
                bail!("{}: the `else` arm comes last", self.span());
            }
            if self.is_keyword("else") {
                self.next();
                otherwise = Some(side(self)?);
            } else {
                let mut values = vec![self.expr()?];
                while self.eat_punct(",") {
                    values.push(self.expr()?);
                }
                arms.push((values, side(self)?));
            }
            self.skip_newlines();
        }
        Ok((arms, otherwise))
    }

    /// `{ value }`, a side of an `if` expression.
    fn braced_expr(&mut self) -> Result<Expr> {
        self.expect_punct("{")?;
        self.skip_newlines();
        let value = self.expr()?;
        self.skip_newlines();
        if !self.eat_punct("}") {
            bail!("{}: each side of an `if` that gives a value is one value, then `}}`", self.span());
        }
        Ok(value)
    }

    fn binary(&mut self, min: u8) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let found = match self.peek() {
                Tok::Punct(p) => BINARY_OPERATORS.iter().find(|(symbol, ..)| symbol == p),
                _ => None,
            };
            let Some(&(_, op, prec, _)) = found else { return Ok(left) };
            if prec < min {
                return Ok(left);
            }
            let span = self.span();
            self.next();
            let right = self.binary(prec + 1)?;
            left = Expr { kind: ExprKind::Binary(op, Box::new(left), Box::new(right)), span };
        }
    }

    fn unary(&mut self) -> Result<Expr> {
        let span = self.span();
        let op = if self.eat_punct("-") {
            UnaryOp::Neg
        } else if self.eat_punct("!") {
            UnaryOp::Not
        } else {
            return self.primary();
        };
        let operand = self.unary()?;
        Ok(Expr { kind: ExprKind::Unary(op, Box::new(operand)), span })
    }

    fn primary(&mut self) -> Result<Expr> {
        let mut expr = self.atom()?;
        while self.is_punct(".") || self.is_punct("[") {
            if self.eat_punct("[") {
                let span = expr.span;
                let index = self.expr()?;
                self.expect_punct("]")?;
                expr = Expr { kind: ExprKind::Index(Box::new(expr), Box::new(index)), span };
                continue;
            }
            self.next();
            let (name, span) = self.ident()?;
            expr = if self.is_punct("(") {
                let args = self.args()?;
                Expr { kind: ExprKind::Method(Box::new(expr), name, args), span }
            } else {
                Expr { kind: ExprKind::Member(Box::new(expr), name), span }
            };
        }
        Ok(expr)
    }

    /// What a name starts: a struct made with named fields, a call, or the name alone.
    fn named(&mut self, name: String, quoted: bool) -> Result<ExprKind> {
        // `Name(field: ...`, perhaps with the first field on the next line
        let mut first = self.pos + 1;
        while self.toks.get(first).is_some_and(|t| t.0 == Tok::Newline) {
            first += 1;
        }
        let named = self.is_punct("(")
            && matches!(self.toks.get(first).map(|t| &t.0), Some(Tok::Ident(_) | Tok::Quoted(_)))
            && self.toks.get(first + 1).map(|t| &t.0) == Some(&Tok::Punct(":"));
        Ok(if named {
            ExprKind::Make(name, self.named_args()?)
        } else if self.is_punct("(") {
            ExprKind::Call(name, self.args()?, quoted)
        } else {
            ExprKind::Name(name)
        })
    }

    fn args(&mut self) -> Result<Vec<Expr>> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        while !self.is_punct(")") {
            args.push(self.expr()?);
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    fn named_args(&mut self) -> Result<Vec<(String, Expr)>> {
        self.fields()
    }

    fn atom(&mut self) -> Result<Expr> {
        let (tok, span) = self.next();
        let kind = match tok {
            Tok::Int(n) => ExprKind::Int(n),
            Tok::Float(n) => ExprKind::Float(n),
            Tok::Str(s) => ExprKind::Str(s),
            Tok::Ident(s) if s == "true" => ExprKind::Bool(true),
            Tok::Ident(s) if s == "false" => ExprKind::Bool(false),
            Tok::Ident(s) if s == "none" => ExprKind::Null,
            Tok::Ident(s) if s == "if" => return self.if_expr(span),
            Tok::Ident(s) if s == "match" => {
                let value = self.expr()?;
                let (arms, otherwise) = self.arms(Self::braced_expr)?;
                let Some(otherwise) = otherwise else {
                    bail!("{span}: a `match` that gives a value needs an `else` arm, for when no arm lists the value");
                };
                ExprKind::Match(Box::new(value), arms, Box::new(otherwise))
            }
            Tok::Ident(name) if is_reserved(&name) && !SELF_WORDS.contains(&name.as_str()) => {
                bail!("{span}: {name} is a reserved word; write `{name}` in backticks for a name")
            }
            Tok::Quoted(name) if SELF_WORDS.contains(&name.as_str()) => {
                bail!("{span}: {name} is always this object, so no name can be {name}; write self.`{name}` for a member of that name")
            }
            Tok::Ident(name) => self.named(name, false)?,
            Tok::Quoted(name) => self.named(name, true)?,
            Tok::Punct("[") => {
                self.skip_newlines();
                let mut items = Vec::new();
                while !self.is_punct("]") {
                    items.push(self.expr()?);
                    self.skip_newlines();
                    if !self.eat_punct(",") {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect_punct("]")?;
                if items.is_empty() { ExprKind::EmptyArray } else { ExprKind::ArrayLiteral(items) }
            }
            Tok::Punct("{") => {
                self.skip_newlines();
                let mut entries = Vec::new();
                while !self.is_punct("}") {
                    let key = self.expr()?;
                    self.expect_punct(":")?;
                    entries.push((key, self.expr()?));
                    self.skip_newlines();
                    if !self.eat_punct(",") {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect_punct("}")?;
                ExprKind::MapLiteral(entries)
            }
            Tok::Path(path) => ExprKind::Path(path),
            Tok::Punct("(") => {
                let inner = self.expr()?;
                self.expect_punct(")")?;
                return Ok(inner);
            }
            t => bail!("{span}: expected an expression, found {t}"),
        };
        Ok(Expr { kind, span })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_name_may_not_start_with_the_prefix_loom_keeps() {
        let header = "blueprint B : Actor at /Game/B\n";
        for decl in [
            "var Loom_X: int",
            "dispatcher Loom_X()",
            "fn Loom_On_ReceiveTick() {\n}",
            "fn F(Loom_X: int) {\n}",
            "component Loom_X: AudioComponent",
            "widget Loom_Canvas: CanvasPanel",
            "fn F() {\n let Loom_Value1 = 1\n}",
            "fn F() {\n for Loom_X in [1] {\n }\n}",
        ] {
            let err = parse(&format!("{header}{decl}\n")).unwrap_err();
            assert!(err.to_string().contains("loom keeps for names it makes itself"), "{decl}: {err}");
        }
        // A name declared elsewhere, such as an engine member, may still be read.
        parse(&format!("{header}fn F() {{\n let x = self.Loom_X\n}}\n")).unwrap();
    }

    #[test]
    fn a_source_with_crlf_line_endings_lexes_as_with_lf() {
        let lf = include_str!("../../../examples/structs/BP_LoomStructs.lm");
        assert_eq!(lex(&lf.replace('\n', "\r\n")).unwrap(), lex(lf).unwrap());
        let err = parse("blueprint B : Actor at /Game/B\r\n\r\nfn F() {\r\n    let x = (\r\n}\r\n").unwrap_err();
        assert!(err.to_string().starts_with("4:14:"), "{err}");
    }

    #[test]
    fn parses_the_sum_example() {
        let file = parse(include_str!("../../../examples/sum/BP_LoomSum.lm")).unwrap();
        assert_eq!(file.name, "BP_LoomSum");
        assert_eq!(file.parent, "/Script/Engine.Actor");
        assert_eq!(file.items.len(), 3);
    }

    #[test]
    fn an_enums_source_holds_only_the_enum() {
        let Source::Enum(e) = parse_source("// Tools.\nenum E at /Game/E {\n    A\n\n    // The second.\n    B \"Bee\"\n}\n").unwrap() else { panic!() };
        assert_eq!(e.doc.as_deref(), Some("Tools."));
        assert_eq!(e.values.iter().map(|v| (v.name.as_str(), v.label.as_deref(), v.doc.as_deref())).collect::<Vec<_>>(), [
            ("A", None, None),
            ("B", Some("Bee"), Some("The second.")),
        ]);
        let err = parse_source("enum E at /Game/E {\n    A\n}\nvar X: int\n").unwrap_err();
        assert!(err.to_string().contains("holds only the enum"), "{err}");
        let err = parse_source("enum E at /Game/E {\n    Loom_A\n}\n").unwrap_err();
        assert!(err.to_string().contains("loom keeps"), "{err}");
    }

    #[test]
    fn a_structs_source_holds_only_the_struct() {
        let Source::Struct(s) = parse_source("struct S at /Game/S {\n    // Across.\n    X: int = 1\n    Items: array<string>\n}\n").unwrap() else { panic!() };
        assert_eq!(s.fields.iter().map(|f| (f.name.as_str(), f.ty.name.as_str(), f.default.is_some(), f.doc.as_deref())).collect::<Vec<_>>(), [
            ("X", "int", true, Some("Across.")),
            ("Items", "array<string>", false, None),
        ]);
        let err = parse_source("struct S at /Game/S {\n    X: int\n}\nfn F() {\n}\n").unwrap_err();
        assert!(err.to_string().contains("holds only the struct"), "{err}");
    }

    #[test]
    fn precedence_binds_multiplication_tighter() {
        let file = parse("blueprint B : /Script/Engine.Actor at /Game/B\nvar X: int = 1 + 2 * 3\n").unwrap();
        let Item::Var { default: Some(e), .. } = &file.items[0] else { panic!() };
        let ExprKind::Binary(BinaryOp::Add, _, right) = &e.kind else { panic!("{e:?}") };
        assert!(matches!(right.kind, ExprKind::Binary(BinaryOp::Mul, ..)));
    }

    #[test]
    fn a_reserved_word_is_a_name_only_in_backticks() {
        let header = "blueprint B : Actor at /Game/B\n";
        for decl in ["var if: int", "fn F(match: int) {\n}", "fn while() {\n}", "fn F() {\n    let return = 1\n}", "var true: bool"] {
            let err = parse(&format!("{header}{decl}\n")).unwrap_err().to_string();
            assert!(err.contains("is a reserved word; write"), "{decl}: {err}");
        }
        let err = parse(&format!("{header}var x: int\nfn F() -> int {{\n    return x + else\n}}\n")).unwrap_err().to_string();
        assert!(err.contains("else is a reserved word"), "{err}");

        let file = parse(&format!("{header}var `if`: int\nfn `Scale It`(`in`: int) -> int {{\n    `if` = `in`\n    return self.`Scale It`(`if`)\n}}\n")).unwrap();
        let Item::Var { name, .. } = &file.items[0] else { panic!() };
        assert_eq!(name, "if");
        let Item::Fn { name, params, body, .. } = &file.items[1] else { panic!() };
        assert_eq!((name.as_str(), params[0].name.as_str()), ("Scale It", "in"));
        let Stmt::Assign { target, value, .. } = &body[0] else { panic!() };
        assert!(matches!((&target.kind, &value.kind), (ExprKind::Name(t), ExprKind::Name(v)) if t == "if" && v == "in"));
        let Stmt::Return { value: Some(call), .. } = &body[1] else { panic!() };
        assert!(matches!(&call.kind, ExprKind::Method(_, m, _) if m == "Scale It"));

        for (src, expected) in [
            ("var `x: int\n", "closing backtick on the same line"),
            ("var ``: int\n", "cannot be empty"),
            ("fn F() {\n    `self`.X = 1\n}\n", "self is always this object"),
            ("var `Loom_X`: int\n", "loom keeps for names it makes itself"),
            ("var `self`: int\n", "nothing can be declared self"),
        ] {
            let err = parse(&format!("{header}{src}")).unwrap_err().to_string();
            assert!(err.contains(expected), "{src}: {err}");
        }
    }

    #[test]
    fn a_contextual_word_is_a_name_where_no_word_is_expected() {
        let header = "blueprint B : Actor at /Game/B\n";
        for word in CONTEXTUAL.iter().chain(TYPE_WORDS) {
            // A function named as a built-in one is declared in backticks, whatever the word.
            let function = crate::lift::print::function_name(word);
            let src = format!("{header}var {word}: int\nfn {function}({word}: int) -> int {{\n    let x = {word}\n    self.{word} = x\n    return {word}\n}}\n");
            parse(&src).unwrap_or_else(|e| panic!("{word}: {e}"));
        }
    }

    #[test]
    fn a_name_is_written_in_backticks_only_where_it_needs_them() {
        assert_eq!(written_name("Health").as_deref(), Some("Health"));
        assert_eq!(written_name("in").as_deref(), Some("in"));
        assert_eq!(written_name("if").as_deref(), Some("`if`"));
        assert_eq!(written_name("none").as_deref(), Some("`none`"));
        assert_eq!(written_name("Scale Sample Count").as_deref(), Some("`Scale Sample Count`"));
        assert_eq!(written_name("2x").as_deref(), Some("`2x`"));
        assert_eq!(written_name("/Script/Engine.Actor").as_deref(), Some("/Script/Engine.Actor"));
        assert_eq!(written_name("a`b"), None);
        assert_eq!(written_name(""), None);
    }

    #[test]
    fn a_parameter_may_be_called_out() {
        let header = "blueprint B : Actor at /Game/B\n";
        let file = parse(&format!("{header}fn F(out: int, out out: bool) {{\n}}\n")).unwrap();
        let Item::Fn { params, .. } = &file.items[0] else { panic!() };
        assert_eq!(params.iter().map(|p| (p.name.as_str(), p.out)).collect::<Vec<_>>(), [("out", false), ("out", true)]);
    }

    #[test]
    fn an_out_parameter_is_neither_a_ref_nor_given_a_default() {
        let header = "blueprint B : Actor at /Game/B\n";
        let file = parse(&format!("{header}fn F(a: int, out n: int, b: int = 2, out m: int) {{\n}}\n")).unwrap();
        let Item::Fn { params, .. } = &file.items[0] else { panic!() };
        assert_eq!(params.iter().map(|p| (p.name.as_str(), p.out, p.default.is_some())).collect::<Vec<_>>(), [
            ("a", false, false),
            ("n", true, false),
            ("b", false, true),
            ("m", true, false),
        ]);
        for (decl, expected) in [
            ("fn F(out xs: ref array<int>) {\n}", "cannot be ref"),
            ("fn F(out n: int = 1) {\n}", "has no default"),
            ("fn F(a: int = 1, out n: int, b: int) {\n}", "needs a default too"),
            ("dispatcher D(out n: int)", "cannot be out"),
            ("fn F(out Loom_N: int) {\n}", "loom keeps for names it makes itself"),
        ] {
            let err = parse(&format!("{header}{decl}\n")).unwrap_err();
            assert!(err.to_string().contains(expected), "{decl}: {err}");
        }
    }

    #[test]
    fn reports_where_it_failed() {
        let err = parse("blueprint B : /Script/Engine.Actor at /Game/B\nfn F( {\n").unwrap_err();
        assert!(err.to_string().starts_with("2:"), "{err}");
    }
}
