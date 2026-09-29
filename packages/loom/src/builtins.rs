//! What the language gives without a class to look in: its plain types, conversions, built-in
//! functions, the methods of containers and soft references, and what each operator does.
//!
//! Lowering finds each of these here and refuses anything that is not here, and the reference's
//! page of built-ins, `reference/builtins.md`, is written from these tables, with an example of
//! every entry that the reference's test compiles. `LOOM_BLESS=1 cargo test` rewrites the page.

// The descriptions and examples here are read only by the tests that write and check the reference.
#![cfg_attr(not(test), allow(dead_code))]

use crate::syntax::{BinaryOp, UnaryOp};
use crate::types::Ty;

pub const MATH: &str = "/Script/Engine.KismetMathLibrary";
pub const ARRAY: &str = "/Script/Engine.KismetArrayLibrary";
pub const MAP: &str = "/Script/Engine.BlueprintMapLibrary";
pub const SET: &str = "/Script/Engine.BlueprintSetLibrary";
pub const SYSTEM: &str = "/Script/Engine.KismetSystemLibrary";
pub const STRING: &str = "/Script/Engine.KismetStringLibrary";
pub const TEXT: &str = "/Script/Engine.KismetTextLibrary";

/// The kind of a value, as these tables tell types apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Byte,
    Int,
    Int64,
    Float,
    Str,
    Name,
    Text,
    Object,
    Class,
    Enum,
    SoftObject,
    SoftClass,
}

impl Kind {
    pub fn of(ty: &Ty) -> Option<Kind> {
        Some(match ty {
            Ty::Bool => Kind::Bool,
            Ty::Byte => Kind::Byte,
            Ty::Int => Kind::Int,
            Ty::Int64 => Kind::Int64,
            Ty::Float => Kind::Float,
            Ty::Str => Kind::Str,
            Ty::Name => Kind::Name,
            Ty::Text => Kind::Text,
            Ty::Object(_) => Kind::Object,
            Ty::Class(_) => Kind::Class,
            Ty::Enum(_) => Kind::Enum,
            Ty::SoftObject(_) => Kind::SoftObject,
            Ty::SoftClass(_) => Kind::SoftClass,
            Ty::Struct(_) | Ty::Array(_) | Ty::Set(_) | Ty::Map(..) | Ty::Interface(_) | Ty::Delegate(_) | Ty::Null | Ty::Record(_) => return None,
        })
    }

    /// The type of a value of this kind, for the kinds that are a whole type on their own.
    pub fn ty(self) -> Option<Ty> {
        Some(match self {
            Kind::Bool => Ty::Bool,
            Kind::Byte => Ty::Byte,
            Kind::Int => Ty::Int,
            Kind::Int64 => Ty::Int64,
            Kind::Float => Ty::Float,
            Kind::Str => Ty::Str,
            Kind::Name => Ty::Name,
            Kind::Text => Ty::Text,
            Kind::Object | Kind::Class | Kind::Enum | Kind::SoftObject | Kind::SoftClass => return None,
        })
    }

    /// How the reference names the kind.
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Bool => "`bool`",
            Kind::Byte => "a byte",
            Kind::Int => "`int`",
            Kind::Int64 => "`int64`",
            Kind::Float => "`float`",
            Kind::Str => "`string`",
            Kind::Name => "`name`",
            Kind::Text => "`text`",
            Kind::Object => "an object",
            Kind::Class => "`class<T>`",
            Kind::Enum => "an enum",
            Kind::SoftObject => "`soft<T>`",
            Kind::SoftClass => "`soft<class<T>>`",
        }
    }
}

/// The variables every example of this page may use.
const EXAMPLE_VARIABLES: &str = "var Alive: bool = true
var Lives: int = 3
var Seed: int64 = 0
var Speed: float = 2.5
var Title: string = \"Counter\"
var Tag: name = \"wood\"
var Label: text = \"Hello\"
var Target: Actor
var Kind: class<Actor>
var Group: ETickingGroup
var Icon: soft<Texture2D>
var Spawns: soft<class<Actor>>
var Items: array<int>
var Seen: set<name>
var Stock: map<string, int>
";

/// A type the language names itself, rather than finding it among the project's types.
pub struct PlainType {
    pub name: &'static str,
    pub kind: Kind,
    pub doc: &'static str,
}

pub const PLAIN_TYPES: &[PlainType] = &[
    PlainType { name: "bool", kind: Kind::Bool, doc: "`true` or `false`." },
    PlainType { name: "int", kind: Kind::Int, doc: "A 32-bit whole number." },
    PlainType { name: "int64", kind: Kind::Int64, doc: "A 64-bit whole number." },
    PlainType { name: "float", kind: Kind::Float, doc: "A number with a fraction, held as a double, as Blueprint floats are." },
    PlainType { name: "string", kind: Kind::Str, doc: "Text to work with." },
    PlainType { name: "name", kind: Kind::Name, doc: "An identifier, such as a tag, compared quickly." },
    PlainType { name: "text", kind: Kind::Text, doc: "Text to show a player." },
];

pub fn plain_type(name: &str) -> Option<Ty> {
    PLAIN_TYPES.iter().find(|t| t.name == name).and_then(|t| t.kind.ty())
}

/// A library function a conversion calls. An internal one is one of the conversions the Blueprint
/// editor puts between two pins by itself, which cannot be called by name.
pub struct Step {
    pub class: &'static str,
    pub function: &'static str,
    pub param: &'static str,
    pub internal: bool,
}

const fn step(class: &'static str, function: &'static str, param: &'static str) -> Step {
    Step { class, function, param, internal: false }
}

/// `to(value)`, for a value of kind `from`: the calls it makes, in order. None is the value itself.
pub struct Conversion {
    pub to: &'static str,
    pub from: Kind,
    pub steps: &'static [Step],
}

pub const CONVERSIONS: &[Conversion] = &[
    Conversion { to: "float", from: Kind::Float, steps: &[] },
    Conversion { to: "float", from: Kind::Int, steps: &[step(MATH, "Conv_IntToDouble", "InInt")] },
    Conversion { to: "float", from: Kind::Str, steps: &[step(STRING, "Conv_StringToDouble", "InString")] },
    Conversion { to: "int", from: Kind::Int, steps: &[] },
    Conversion { to: "int", from: Kind::Float, steps: &[step(MATH, "FTrunc", "A")] },
    Conversion { to: "int", from: Kind::Byte, steps: &[step(MATH, "Conv_ByteToInt", "InByte")] },
    // An enum's value is the byte it is held in.
    Conversion { to: "int", from: Kind::Enum, steps: &[step(MATH, "Conv_ByteToInt", "InByte")] },
    Conversion { to: "int", from: Kind::Str, steps: &[step(STRING, "Conv_StringToInt", "InString")] },
    Conversion { to: "string", from: Kind::Str, steps: &[] },
    Conversion { to: "string", from: Kind::Int, steps: &[step(STRING, "Conv_IntToString", "InInt")] },
    Conversion { to: "string", from: Kind::Float, steps: &[step(STRING, "Conv_DoubleToString", "InDouble")] },
    Conversion { to: "string", from: Kind::Bool, steps: &[step(STRING, "Conv_BoolToString", "InBool")] },
    Conversion { to: "string", from: Kind::Name, steps: &[step(STRING, "Conv_NameToString", "InName")] },
    Conversion { to: "string", from: Kind::Text, steps: &[step(TEXT, "Conv_TextToString", "InText")] },
    Conversion { to: "string", from: Kind::Object, steps: &[step(STRING, "Conv_ObjectToString", "InObj")] },
    Conversion {
        to: "string",
        from: Kind::SoftObject,
        steps: &[step(SYSTEM, "Conv_SoftObjectReferenceToString", "SoftObjectReference")],
    },
    Conversion {
        to: "string",
        from: Kind::SoftClass,
        steps: &[step(SYSTEM, "Conv_SoftClassReferenceToString", "SoftClassReference")],
    },
    // A class's path, by way of the soft reference to it.
    Conversion {
        to: "string",
        from: Kind::Class,
        steps: &[
            Step { class: SYSTEM, function: "Conv_ClassToSoftClassReference", param: "Class", internal: true },
            step(SYSTEM, "Conv_SoftClassReferenceToString", "SoftClassReference"),
        ],
    },
    Conversion { to: "name", from: Kind::Name, steps: &[] },
    Conversion { to: "name", from: Kind::Str, steps: &[step(STRING, "Conv_StringToName", "InString")] },
    Conversion { to: "text", from: Kind::Text, steps: &[] },
    Conversion { to: "text", from: Kind::Str, steps: &[step(TEXT, "Conv_StringToText", "InString")] },
];

pub fn conversion(to: &str, from: &Ty) -> Option<&'static Conversion> {
    let from = Kind::of(from)?;
    CONVERSIONS.iter().find(|c| c.to == to && c.from == from)
}

/// A function the language gives, called by name.
pub struct Function {
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
    /// A whole source file that calls it.
    pub example: &'static str,
}

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "float",
        signature: "float(value) -> float",
        doc: "The value as a `float`. See [Conversions](#conversions) for what it converts.",
        example: "fn F() -> float {\n    return float(Lives) * Speed\n}",
    },
    Function {
        name: "int",
        signature: "int(value) -> int",
        doc: "The value as an `int`. A `float` loses its fraction, a `string` that does not start with a number gives 0, and an enum gives the number that stands for its value.",
        example: "fn F() -> int {\n    return int(Speed)\n}",
    },
    Function {
        name: "string",
        signature: "string(value) -> string",
        doc: "The value as a `string`.",
        example: "fn F() -> string {\n    return \"lives: \" + string(Lives)\n}",
    },
    Function {
        name: "name",
        signature: "name(value) -> name",
        doc: "The value as a `name`, such as a key made up from strings.",
        example: "fn F(id: string) -> name {\n    return name(\"option.\" + id)\n}",
    },
    Function {
        name: "text",
        signature: "text(value) -> text",
        doc: "The value as `text`, to show a player.",
        example: "fn F() -> text {\n    return text(Title)\n}",
    },
    Function {
        name: "cast",
        signature: "cast(value, Class or Interface) -> Class or Interface",
        doc: "The object if it is of the class, and `none` otherwise. With a `class<T>`, the class if it \
              is the class or a subclass of it, and `none` otherwise.\n\n\
              With an interface, the object as that interface, or `none` if it doesn't implement it. \
              Call the interface's functions on the result, as \
              [Calls through an interface](/reference/expressions/#calls-through-an-interface) shows.",
        example: "fn F(thing: Object) -> Actor {\n    return cast(thing, Actor)\n}",
    },
    Function {
        name: "implements",
        signature: "implements(value, Interface) -> bool",
        doc: "Whether the object implements the interface, itself or through its parents. `false` for \
              `none`.",
        example: "fn F() -> bool {\n    return implements(Target, MovieSceneEasingFunction)\n}",
    },
    Function {
        name: "create_widget",
        signature: "create_widget(Class) -> Class",
        doc: "A new widget of a widget Blueprint's class.",
        example: "blueprint WBP_Panel : UserWidget at /Game/UI/WBP_Panel\n\nfn Another() {\n    \
                  let panel = create_widget(WBP_Panel)\n    panel.AddToViewport(0)\n}",
    },
    Function {
        name: "construct",
        signature: "construct(Class, outer) -> Class",
        doc: "A new object of the class, held by `outer`, which it lives as long as. Not for an actor, \
              which is spawned, or a widget Blueprint, which `create_widget` makes.",
        example: "fn F(owner: Object) -> HorizontalBox {\n    return construct(HorizontalBox, owner)\n}",
    },
    Function {
        name: "include",
        signature: "include(\"file.json\")",
        doc: "A variable's default, read from a JSON file when the Blueprint is compiled: an object \
              as a map, an array as an array. The path is relative to the source file. Only a \
              variable's default can be included.",
        example: "var Strings: map<name, string> = include(\"strings.json\")",
    },
    Function {
        name: "subsystem",
        signature: "subsystem(Class) -> Class",
        doc: "The engine's subsystem of the class, or in an editor Blueprint, the editor's.",
        example: "fn F() -> bool {\n    return subsystem(DataDrivenCVarEngineSubsystem) != none\n}",
    },
];

pub fn function(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

/// What a method is called on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    Soft,
}

impl On {
    /// How an error message names it.
    pub fn a(self) -> &'static str {
        match self {
            On::Soft => "a soft reference",
        }
    }
}

/// `value.name(params)` on an array, a set, a map or a soft reference.
pub struct Method {
    pub on: On,
    pub name: &'static str,
    pub params: &'static [&'static str],
    /// What it gives back, as the reference says it.
    pub returns: Option<&'static str>,
    /// The library function it calls, and for a soft reference to a class, the one it calls then.
    pub function: &'static str,
    pub class_function: Option<&'static str>,
    /// Whether it only computes a value, so it cannot stand on its own.
    pub pure: bool,
    pub doc: &'static str,
}

pub const METHODS: &[Method] = &[
    Method {
        on: On::Soft,
        name: "load",
        params: &[],
        returns: Some("the object, or the class"),
        function: "LoadAsset_Blocking",
        class_function: Some("LoadClassAsset_Blocking"),
        pure: false,
        doc: "Loads what it refers to, waits until it has, and gives it.",
    },
    Method {
        on: On::Soft,
        name: "valid",
        params: &[],
        returns: Some("`bool`"),
        function: "IsValidSoftObjectReference",
        class_function: Some("IsValidSoftClassReference"),
        pure: true,
        doc: "Whether it refers to anything.",
    },
];

pub fn method(on: On, name: &str, args: usize) -> Option<&'static Method> {
    METHODS.iter().find(|m| m.on == on && m.name == name && m.params.len() == args)
}

/// Everything `on` has, as an error message lists it: "add(item), length() and clear()".
pub fn methods_of(on: On) -> String {
    let all: Vec<String> = METHODS.iter().filter(|m| m.on == on).map(|m| format!("{}({})", m.name, m.params.join(", "))).collect();
    match all.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// How an operator is lowered for one kind of value.
pub enum Lowering {
    /// `function(A, B)`, which gives a value of the kind.
    Library { class: &'static str, function: &'static str, returns: Kind },
    /// The Blueprint editor's equality node for enums, negated for `!=`.
    EnumEqual,
}

pub struct Operator {
    pub op: BinaryOp,
    pub on: Kind,
    pub lowering: Lowering,
}

const fn math(op: BinaryOp, on: Kind, function: &'static str, returns: Kind) -> Operator {
    Operator { op, on, lowering: Lowering::Library { class: MATH, function, returns } }
}

const fn string(op: BinaryOp, function: &'static str, returns: Kind) -> Operator {
    Operator { op, on: Kind::Str, lowering: Lowering::Library { class: STRING, function, returns } }
}

pub const OPERATORS: &[Operator] = {
    use BinaryOp::*;
    use Kind::{Bool, Byte, Enum, Float, Int, Name, Object, Str};
    &[
        math(Add, Int, "Add_IntInt", Int),
        math(Sub, Int, "Subtract_IntInt", Int),
        math(Mul, Int, "Multiply_IntInt", Int),
        math(Div, Int, "Divide_IntInt", Int),
        math(Rem, Int, "Percent_IntInt", Int),
        math(Lt, Int, "Less_IntInt", Bool),
        math(Le, Int, "LessEqual_IntInt", Bool),
        math(Gt, Int, "Greater_IntInt", Bool),
        math(Ge, Int, "GreaterEqual_IntInt", Bool),
        math(Eq, Int, "EqualEqual_IntInt", Bool),
        math(Ne, Int, "NotEqual_IntInt", Bool),
        math(BitAnd, Int, "And_IntInt", Int),
        math(BitOr, Int, "Or_IntInt", Int),
        math(Add, Float, "Add_DoubleDouble", Float),
        math(Sub, Float, "Subtract_DoubleDouble", Float),
        math(Mul, Float, "Multiply_DoubleDouble", Float),
        math(Div, Float, "Divide_DoubleDouble", Float),
        math(Rem, Float, "Percent_FloatFloat", Float),
        math(Lt, Float, "Less_DoubleDouble", Bool),
        math(Le, Float, "LessEqual_DoubleDouble", Bool),
        math(Gt, Float, "Greater_DoubleDouble", Bool),
        math(Ge, Float, "GreaterEqual_DoubleDouble", Bool),
        math(Eq, Float, "EqualEqual_DoubleDouble", Bool),
        math(Ne, Float, "NotEqual_DoubleDouble", Bool),
        math(And, Bool, "BooleanAND", Bool),
        math(Or, Bool, "BooleanOR", Bool),
        math(Eq, Bool, "EqualEqual_BoolBool", Bool),
        math(Ne, Bool, "NotEqual_BoolBool", Bool),
        string(Add, "Concat_StrStr", Str),
        string(Eq, "EqualEqual_StrStr", Bool),
        string(Ne, "NotEqual_StrStr", Bool),
        math(Eq, Name, "EqualEqual_NameName", Bool),
        math(Ne, Name, "NotEqual_NameName", Bool),
        math(Eq, Byte, "EqualEqual_ByteByte", Bool),
        math(Ne, Byte, "NotEqual_ByteByte", Bool),
        math(Eq, Object, "EqualEqual_ObjectObject", Bool),
        math(Ne, Object, "NotEqual_ObjectObject", Bool),
        Operator { op: Eq, on: Enum, lowering: Lowering::EnumEqual },
        Operator { op: Ne, on: Enum, lowering: Lowering::EnumEqual },
    ]
};

pub fn operator(op: BinaryOp, on: &Ty) -> Option<&'static Operator> {
    let on = Kind::of(on)?;
    OPERATORS.iter().find(|o| o.op == op && o.on == on)
}

/// `-x` and `!x`: the library function, and whether it is given zero first, as `0 - x`.
pub struct Unary {
    pub op: UnaryOp,
    pub on: Kind,
    pub function: &'static str,
    pub zero_first: bool,
}

pub const UNARY: &[Unary] = &[
    Unary { op: UnaryOp::Neg, on: Kind::Int, function: "Subtract_IntInt", zero_first: true },
    Unary { op: UnaryOp::Neg, on: Kind::Float, function: "Subtract_DoubleDouble", zero_first: true },
    Unary { op: UnaryOp::Not, on: Kind::Bool, function: "Not_PreBool", zero_first: false },
];

pub fn unary(op: UnaryOp, on: &Ty) -> Option<&'static Unary> {
    let on = Kind::of(on)?;
    UNARY.iter().find(|u| u.op == op && u.on == on)
}

#[cfg(test)]
mod page {
    use std::fmt::Write;
    use std::path::Path;

    use super::*;
    use crate::syntax::BINARY_OPERATORS;

    fn short(path: &str) -> &str {
        path.rsplit('.').next().unwrap_or(path)
    }

    /// A ```loom block of one function doing each of `lines`.
    fn block(lines: &[&str]) -> String {
        let mut out = String::from("```loom\nfn Examples() {\n");
        for line in lines {
            writeln!(out, "    {line}").unwrap();
        }
        out.push_str("}\n```\n");
        out
    }

    /// A type as a source writes it.
    fn written(ty: &Ty) -> String {
        match ty {
            Ty::Bool => "bool".into(),
            Ty::Byte => "byte".into(),
            Ty::Int => "int".into(),
            Ty::Int64 => "int64".into(),
            Ty::Float => "float".into(),
            Ty::Str => "string".into(),
            Ty::Name => "name".into(),
            Ty::Text => "text".into(),
            Ty::Object(p) | Ty::Struct(p) | Ty::Enum(p) | Ty::Interface(p) => short(p).to_string(),
            Ty::Class(p) => format!("class<{}>", short(p)),
            Ty::SoftObject(p) => format!("soft<{}>", short(p)),
            Ty::SoftClass(p) => format!("soft<class<{}>>", short(p)),
            Ty::Array(t) => format!("array<{}>", written(t)),
            Ty::Set(t) => format!("set<{}>", written(t)),
            Ty::Map(k, v) => format!("map<{}, {}>", written(k), written(v)),
            Ty::Delegate(_) => ty.to_string(),
            Ty::Null | Ty::Record(_) => unreachable!("no parameter has this type"),
        }
    }

    /// What each array, set and map method does, by its engine name with its library's prefix. The
    /// methods come from the engine's types; these lines are the page's own, and a method without
    /// one stops the page being written.
    const CONTAINER_DOES: &[(&str, &str)] = &[
        ("Array_Add", "Adds the item at the end, and gives its index."),
        ("Array_AddUnique", "Adds the item at the end unless it's there already, and gives its index, or -1."),
        ("Array_Append", "Adds every item of the other array at the end, in order."),
        ("Array_Clear", "Removes every item."),
        ("Array_Contains", "Whether the item is in it."),
        ("Array_Find", "The index of the item's first place, or -1."),
        ("Array_Identical", "Whether the other array holds the same items in the same order."),
        ("Array_Insert", "Puts the item at the index, moving the ones after it along."),
        ("Array_IsEmpty", "Whether it holds nothing."),
        ("Array_IsNotEmpty", "Whether it holds anything."),
        ("Array_IsValidIndex", "Whether the index is one of its items'."),
        ("Array_LastIndex", "The index of its last item, or -1 if it's empty."),
        ("Array_Length", "How many items it holds."),
        ("Array_Random", "A random item, and its index."),
        ("Array_RandomFromStream", "A random item, and its index, drawn from the stream."),
        ("Array_Remove", "Removes the item at the index, moving the ones after it back."),
        ("Array_RemoveItem", "Removes every copy of the item, and gives whether there was one."),
        ("Array_Resize", "Makes it that long, adding zeroed items or dropping the last ones."),
        ("Array_Reverse", "Reverses its order."),
        ("Array_Set", "Sets the item at the index; with bSizeToFit, grows it to reach an index past its end."),
        ("Array_Shuffle", "Puts its items in a random order."),
        ("Array_ShuffleFromStream", "Puts its items in a random order, drawn from the stream."),
        ("Array_Swap", "Swaps the items at the two indices."),
        ("Set_Add", "Adds the item, unless it's there already."),
        ("Set_AddItems", "Adds each item of the array that isn't there already."),
        ("Set_Clear", "Removes every item."),
        ("Set_Contains", "Whether the item is in it."),
        ("Set_Difference", "The items of this set that aren't in the other."),
        ("Set_GetItemByIndex", "The item at the index, in the set's own order."),
        ("Set_GetLastIndex", "The index of its last item, or -1 if it's empty."),
        ("Set_Intersection", "The items in both sets."),
        ("Set_IsEmpty", "Whether it holds nothing."),
        ("Set_IsNotEmpty", "Whether it holds anything."),
        ("Set_Length", "How many items it holds."),
        ("Set_Remove", "Removes the item, and gives whether it was there."),
        ("Set_RemoveItems", "Removes each item of the array."),
        ("Set_ToArray", "Its items, as an array."),
        ("Set_Union", "The items in either set."),
        ("Map_Add", "Sets the key's value, adding the key if it's new."),
        ("Map_Clear", "Removes every key."),
        ("Map_Contains", "Whether the key is in it."),
        ("Map_Find", "The key's value, and whether the key was there."),
        ("Map_GetKeyValueByIndex", "The key and value at the index, in the map's own order."),
        ("Map_GetLastIndex", "The index of its last key, or -1 if it's empty."),
        ("Map_IsEmpty", "Whether it holds nothing."),
        ("Map_IsNotEmpty", "Whether it holds anything."),
        ("Map_Keys", "Its keys, as an array."),
        ("Map_Length", "How many keys it holds."),
        ("Map_Remove", "Removes the key and its value, and gives whether it was there."),
        ("Map_Values", "Its values, as an array."),
    ];

    /// The array, set and map methods: every function of their libraries a Blueprint may call on
    /// them, from the reference's types, with its types written for what the container holds.
    fn container_sections() -> String {
        let db = crate::types::TypeDb::parse(crate::fixtures::engine()).unwrap();
        let held = |n: &str| Box::new(Ty::Struct(format!("/Script/Loom.{n}")));
        let mut out = String::new();
        for (on, example, library, prefix, title, intro, lines) in [
            (
                Ty::Array(held("T")),
                Ty::Array(Box::new(Ty::Int)),
                ARRAY,
                "Array_",
                "Array methods",
                "Items are also read and set with `array[index]`, and walked with `for`. `T` is what the array holds.",
                &["Items.Add(4)", "let count = Items.Length()", "let has_four = Items.Contains(4)", "Items.RemoveItem(4)"][..],
            ),
            (
                Ty::Set(held("T")),
                Ty::Set(Box::new(Ty::Name)),
                SET,
                "Set_",
                "Set methods",
                "A set is walked with `for`. `T` is what the set holds.",
                &["Seen.Add(\"wood\")", "let seen_wood = Seen.Contains(\"wood\")", "let kinds = Seen.Length()"][..],
            ),
            (
                Ty::Map(held("K"), held("V")),
                Ty::Map(Box::new(Ty::Str), Box::new(Ty::Int)),
                MAP,
                "Map_",
                "Map methods",
                "A value is also read and set with `map[key]`, and a map walked with `for key, value in map`. `K` and `V` are the map's keys' and values' types.",
                &["Stock.Add(\"wood\", 3)", "let wood = Stock.Find(\"wood\")", "let has_wood = wood.ReturnValue", "let wood_count = wood.Value"][..],
            ),
        ] {
            writeln!(
                out,
                "\n## {title}\n\n{intro} Each method is a function of `{}`, called by its name without `{prefix}`. \
                 One that gives outputs gives them as a call to any function does.\n\n| Method | Gives | Does | Calls |\n| --- | --- | --- | --- |",
                short(library)
            )
            .unwrap();
            for name in db.container_methods(&example) {
                let (f, _) = db.container_function(&on, &name).unwrap().expect("a method it lists");
                let params: Vec<String> = f.params.iter().map(|p| format!("{}: {}", p.name, written(&p.ty))).collect();
                let mut gives: Vec<String> = f.outputs.iter().map(|(n, t)| format!("{n}: {}", written(t))).collect();
                let gives = match (&f.ret, gives.is_empty()) {
                    (None, true) => "nothing".to_string(),
                    (Some(t), true) => format!("`{}`", written(t)),
                    (ret, false) => {
                        gives.extend(ret.iter().map(|t| format!("ReturnValue: {}", written(t))));
                        format!("the outputs `{}`", gives.join(", "))
                    }
                };
                let pure = if f.pure { ", pure" } else { "" };
                let engine = format!("{prefix}{name}");
                let does = CONTAINER_DOES
                    .iter()
                    .find(|(n, _)| *n == engine)
                    .unwrap_or_else(|| panic!("{engine} has no line saying what it does; add one to CONTAINER_DOES"))
                    .1;
                writeln!(out, "| `{name}({})` | {gives} | {does} | `{}.{engine}`{pure} |", params.join(", "), short(library)).unwrap();
            }
            out.push('\n');
            out.push_str(&block(lines));
        }
        out
    }

    /// The page, from the tables.
    fn page() -> String {
        let mut out = String::from(
            "---\ntitle: Built-ins\ndescription: The plain types, conversions, functions, methods and operators Loom gives.\n\
             sidebar:\n  order: 10\ntableOfContents:\n  maxHeadingLevel: 2\n---\n\n\
             <!-- Written by the compiler's tests from packages/loom/src/builtins.rs. Change that, not this. -->\n\n\
             What Loom gives without a class to look in. Each is written from the compiler's own \
             tables, and each example below compiles. The examples use these variables:\n\n",
        );
        writeln!(out, "```loom prelude\n{EXAMPLE_VARIABLES}```\n").unwrap();

        out.push_str("## Plain types\n\n| Type | Is |\n| --- | --- |\n");
        for t in PLAIN_TYPES {
            writeln!(out, "| `{}` | {} |", t.name, t.doc).unwrap();
        }

        out.push_str("\n## Functions\n\n");
        for f in FUNCTIONS {
            writeln!(out, "### {}\n\n`{}`\n\n{}\n", f.name, f.signature, f.doc).unwrap();
            writeln!(out, "```loom\n{}\n```\n", f.example).unwrap();
        }

        out.push_str("## Conversions\n\nWhat each conversion takes, and the Blueprint function it calls.\n\n");
        out.push_str("| Conversion | Of | Calls |\n| --- | --- | --- |\n");
        for c in CONVERSIONS {
            let calls = if c.steps.is_empty() {
                "nothing: it is the value itself".to_string()
            } else {
                c.steps
                    .iter()
                    .map(|s| format!("`{}.{}`{}", short(s.class), s.function, if s.internal { " (internal)" } else { "" }))
                    .collect::<Vec<_>>()
                    .join(", then ")
            };
            writeln!(out, "| `{}()` | {} | {calls} |", c.to, c.from.describe()).unwrap();
        }
        out.push('\n');
        out.push_str(&block(&["let shown = string(Lives)", "let rounded = int(Speed)", "let exact = float(Lives)", "let heading = text(Title)"]));

        out.push_str(&container_sections());

        for (on, title, intro) in [(On::Soft, "Soft reference methods", "`load()` on a `soft<class<T>>` gives the class, as a `class<T>`.")] {
            writeln!(out, "\n## {title}\n\n{intro}\n\n| Method | Gives | Does | Calls |\n| --- | --- | --- | --- |").unwrap();
            for m in METHODS.iter().filter(|m| m.on == on) {
                let pure = if m.pure { " Pure, so it cannot stand on its own." } else { "" };
                let calls = match m.class_function {
                    Some(class) => format!("`{}`, or for a class, `{class}`", m.function),
                    None => format!("`{}`", m.function),
                };
                writeln!(out, "| `{}({})` | {} | {}{pure} | {calls} |", m.name, m.params.join(", "), m.returns.unwrap_or("nothing"), m.doc)
                    .unwrap();
            }
            out.push('\n');
            out.push_str(&block(&["let spawned = Spawns.load()", "let has_icon = Icon.valid()"]));
        }

        // The levels in order, numbered, as the order is what they say.
        out.push_str("\n## Operators\n\nFrom loosest to tightest. Parentheses group as usual.\n\n");
        let mut levels: Vec<u8> = BINARY_OPERATORS.iter().map(|o| o.2).collect();
        levels.dedup();
        let mut number = 0;
        for level in levels {
            let ops: Vec<_> = BINARY_OPERATORS.iter().filter(|o| o.2 == level).collect();
            let symbols: Vec<String> = ops.iter().map(|o| format!("`{}`", o.0)).collect();
            let names: Vec<&str> = ops.iter().map(|o| o.3).collect();
            number += 1;
            writeln!(out, "{number}. {}: {}", symbols.join(" "), names.join(", ")).unwrap();
        }
        let unary: Vec<String> = [UnaryOp::Neg, UnaryOp::Not].iter().map(|op| format!("`{}x`", op.symbol())).collect();
        writeln!(out, "{}. {}: negate, not", number + 1, unary.join(" ")).unwrap();
        out.push_str(
            "\n`+=`, `-=` and the other compound assignments aren't on this list: each is a statement that \
             applies an operator and assigns the result. See \
             [Compound assignment](/reference/statements/#compound-assignment).\n",
        );

        out.push_str(
            "\nBoth sides of an operator have the same type, and what each operator takes is below. \
             `&&` and `||` compute both sides, so neither may contain a call that does something.\n\n\
             | Operator | Takes | Gives | Calls |\n| --- | --- | --- | --- |\n",
        );
        for (symbol, op, ..) in BINARY_OPERATORS {
            for o in OPERATORS.iter().filter(|o| o.op == *op) {
                let (gives, calls) = match &o.lowering {
                    Lowering::Library { class, function, returns } => (returns.describe(), format!("`{}.{function}`", short(class))),
                    Lowering::EnumEqual => ("`bool`", "the editor's enum equality node".to_string()),
                };
                writeln!(out, "| `{}` | {} | {gives} | {calls} |", symbol.replace('|', "\\|"), o.on.describe()).unwrap();
            }
        }
        for u in UNARY {
            writeln!(out, "| `{}x` | {} | {} | `KismetMathLibrary.{}`{} |", u.op.symbol(), u.on.describe(), u.on.describe(), u.function, if u.zero_first { ", as `0 - x`" } else { "" }).unwrap();
        }
        out.push('\n');
        out.push_str(&block(&["let playing = Alive && Lives > 0", "let doubled = Speed * 2.0", "let greeting = Title + \"!\"", "let aimed = Target != none"]));
        out
    }

    #[test]
    fn the_builtins_page_is_written_from_the_tables() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/builtins.md");
        let got = page();
        if std::env::var_os("LOOM_BLESS").is_some() {
            std::fs::write(&path, &got).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(got == want, "reference/builtins.md is not what packages/loom/src/builtins.rs says; run LOOM_BLESS=1 cargo test");
    }
}
