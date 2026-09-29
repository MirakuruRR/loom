//! Reading a cooked package as CUE4Parse writes it to JSON: its exports, the objects they refer
//! to, and the types of their properties.

use anyhow::Result;
use serde_json::Value;

/// A package's exports, and the package path it was cooked at, as `/Game/...`.
pub struct Package {
    pub path: String,
    pub exports: Vec<Value>,
}

impl Package {
    pub fn parse(text: &str) -> Result<Package> {
        let exports: Vec<Value> = serde_json::from_str(text)?;
        let package = exports
            .iter()
            .find_map(|e| e.get("Package").and_then(Value::as_str))
            .ok_or_else(|| refusal!(Unexpected, "a package whose exports do not name it"))?;
        Ok(Package { path: content_path(package)?, exports })
    }

    /// The first export of a type.
    pub fn export(&self, ty: &str) -> Option<&Value> {
        self.exports.iter().find(|e| str_of(e, "Type") == Some(ty))
    }

    /// The export an object reference inside this package points at, by its name.
    pub fn export_named(&self, name: &str) -> Option<&Value> {
        self.exports.iter().find(|e| str_of(e, "Name") == Some(name))
    }
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// A cooked package path, `Game/Content/Mods/X/Y`, or an object path in one, `...Y.3`, as the
/// `/Game/Mods/X/Y` the editor names the package by.
pub fn content_path(cooked: &str) -> Result<String> {
    // An object path ends in the object's index in the package.
    let cooked = match cooked.rsplit_once('.') {
        Some((package, index)) if index.chars().all(|c| c.is_ascii_digit()) => package,
        _ => cooked,
    };
    // A path the loader already mounted, as an import names the game's own assets.
    if cooked.starts_with("/Game/") || cooked.starts_with("/Engine/") {
        return Ok(cooked.to_string());
    }
    let trimmed = cooked.trim_start_matches('/');
    let (_, rest) = trimmed.split_once("/Content/").ok_or_else(|| refusal!(Unexpected, "{cooked}, which is not under a Content folder"))?;
    let root = if trimmed.starts_with("Engine/") { "/Engine/" } else { "/Game/" };
    Ok(format!("{root}{rest}"))
}

/// A reference to an object, `{"ObjectName": "Class'Name'", "ObjectPath": "/Script/Engine"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    /// The object's class, as `Class` or `BlueprintGeneratedClass`.
    pub kind: String,
    /// Its name, which for a function or a subobject is `Outer:Name`.
    pub name: String,
    /// Its package: `/Script/Engine`, or `/Game/...`.
    pub package: String,
}

impl Ref {
    pub fn of(v: &Value) -> Result<Ref> {
        let object = str_of(v, "ObjectName").ok_or_else(|| refusal!(Unexpected, "{v} where an object reference should be"))?;
        let path = str_of(v, "ObjectPath").ok_or_else(|| refusal!(Unexpected, "{v} where an object reference should be"))?;
        let (kind, rest) = object.split_once('\'').ok_or_else(|| refusal!(Unexpected, "an unreadable object name {object}"))?;
        let name = rest.strip_suffix('\'').ok_or_else(|| refusal!(Unexpected, "an unreadable object name {object}"))?;
        let package = if path.starts_with("/Script/") { path.to_string() } else { content_path(path)? };
        Ok(Ref { kind: kind.to_string(), name: name.to_string(), package })
    }

    /// The object's full path, as loom writes a class: `/Script/Engine.Actor`, or
    /// `/Game/Mods/X/BP_Y.BP_Y_C`.
    pub fn path(&self) -> String {
        format!("{}.{}", self.package, self.name)
    }

    /// For a function, `Class:Function`: the class's path and the function's name.
    pub fn function(&self) -> Result<(String, String)> {
        let (class, function) = self.name.split_once(':').ok_or_else(|| refusal!(Unexpected, "{} where a function should be", self.name))?;
        Ok((format!("{}.{class}", self.package), function.to_string()))
    }

    pub fn from_script(&self) -> bool {
        self.package.starts_with("/Script/")
    }
}

/// A property as bytecode refers to it, `{"Owner": ..., "Property": {...}}` or
/// `{"Path": [...], "ResolvedOwner": ...}`: the class or struct that declares it, and its name.
pub fn prop_ref(v: &Value) -> Result<(Option<Ref>, String)> {
    if let Some(p) = v.get("Property") {
        let name = str_of(p, "Name").ok_or_else(|| refusal!(Unexpected, "a property with no name"))?;
        let owner = v.get("Owner").filter(|o| !o.is_null()).map(Ref::of).transpose()?;
        return Ok((owner, name.to_string()));
    }
    if let Some(path) = v.get("Path").and_then(Value::as_array) {
        let name = path.last().and_then(Value::as_str).ok_or_else(|| refusal!(Unexpected, "a property path with no name"))?;
        let owner = v.get("ResolvedOwner").filter(|o| !o.is_null()).map(Ref::of).transpose()?;
        return Ok((owner, name.to_string()));
    }
    unexpected!("an unreadable property reference: {v}")
}

/// A property's type, as the JSON describes it.
#[derive(Debug, Clone, PartialEq)]
pub enum PropTy {
    Bool,
    Byte,
    Int,
    Int64,
    Float,
    Str,
    Name,
    Text,
    Object(String),
    Class(String),
    SoftObject(String),
    SoftClass(String),
    Interface(String),
    Struct(String),
    Enum(String),
    Array(Box<PropTy>),
    Set(Box<PropTy>),
    Map(Box<PropTy>, Box<PropTy>),
    Delegate,
    MulticastDelegate,
}

impl PropTy {
    pub fn of(p: &Value) -> Result<PropTy> {
        let ty = str_of(p, "Type").ok_or_else(|| refusal!(Unexpected, "a property with no type: {p}"))?;
        let class = |key: &str| -> Result<String> {
            let r = p.get(key).ok_or_else(|| refusal!(Unexpected, "{ty} {} with no {key}", str_of(p, "Name").unwrap_or("?")))?;
            Ok(Ref::of(r)?.path())
        };
        let inner = |key: &str| -> Result<Box<PropTy>> {
            Ok(Box::new(PropTy::of(p.get(key).ok_or_else(|| refusal!(Unexpected, "{ty} with no {key}"))?)?))
        };
        Ok(match ty {
            "BoolProperty" => PropTy::Bool,
            "ByteProperty" => match p.get("Enum") {
                Some(e) if !e.is_null() => PropTy::Enum(Ref::of(e)?.path()),
                _ => PropTy::Byte,
            },
            "EnumProperty" => PropTy::Enum(class("Enum")?),
            "IntProperty" => PropTy::Int,
            "Int64Property" => PropTy::Int64,
            "FloatProperty" | "DoubleProperty" => PropTy::Float,
            "StrProperty" => PropTy::Str,
            "NameProperty" => PropTy::Name,
            "TextProperty" => PropTy::Text,
            "ObjectProperty" | "WeakObjectProperty" => PropTy::Object(class("PropertyClass")?),
            "ClassProperty" => PropTy::Class(class("MetaClass")?),
            "SoftObjectProperty" => PropTy::SoftObject(class("PropertyClass")?),
            "SoftClassProperty" => PropTy::SoftClass(class("MetaClass")?),
            "InterfaceProperty" => PropTy::Interface(class("InterfaceClass")?),
            "StructProperty" => PropTy::Struct(class("Struct")?),
            "ArrayProperty" => PropTy::Array(inner("Inner")?),
            "SetProperty" => PropTy::Set(inner("ElementProp")?),
            "MapProperty" => PropTy::Map(inner("KeyProp")?, inner("ValueProp")?),
            "DelegateProperty" => PropTy::Delegate,
            "MulticastDelegateProperty" | "MulticastInlineDelegateProperty" | "MulticastSparseDelegateProperty" => {
                PropTy::MulticastDelegate
            }
            other => not_yet!("a property of type {other}"),
        })
    }
}

/// A property of a class or a function: its name, type and flags.
#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub ty: PropTy,
    pub flags: String,
    pub json: Value,
}

impl Prop {
    pub fn of(p: &Value) -> Result<Prop> {
        Ok(Prop {
            name: str_of(p, "Name").ok_or_else(|| refusal!(Unexpected, "a property with no name"))?.to_string(),
            ty: PropTy::of(p)?,
            flags: str_of(p, "PropertyFlags").unwrap_or("").to_string(),
            json: p.clone(),
        })
    }

    pub fn has(&self, flag: &str) -> bool {
        self.flags.split('|').any(|f| f.trim() == flag)
    }
}

/// The name a user-defined struct's field is shown by: `Id_2_<guid>` is `Id`.
pub fn field_name(cooked: &str) -> &str {
    let mut parts = cooked.rsplitn(3, '_');
    let (Some(guid), Some(index), Some(name)) = (parts.next(), parts.next(), parts.next()) else { return cooked };
    if guid.len() == 32 && guid.chars().all(|c| c.is_ascii_hexdigit()) && index.chars().all(|c| c.is_ascii_digit()) {
        name
    } else {
        cooked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_paths() {
        assert_eq!(content_path("Shop/Content/Mods/A/BP_B").unwrap(), "/Game/Mods/A/BP_B");
        assert_eq!(content_path("Shop/Content/Mods/A/BP_B.3").unwrap(), "/Game/Mods/A/BP_B");
        let r = Ref::of(&serde_json::json!({"ObjectName": "Function'KismetStringLibrary:Concat_StrStr'", "ObjectPath": "/Script/Engine"})).unwrap();
        assert_eq!(r.function().unwrap(), ("/Script/Engine.KismetStringLibrary".to_string(), "Concat_StrStr".to_string()));
        let r = Ref::of(&serde_json::json!({"ObjectName": "BlueprintGeneratedClass'BP_B_C'", "ObjectPath": "Game/Content/Mods/A/BP_B.0"})).unwrap();
        assert_eq!(r.path(), "/Game/Mods/A/BP_B.BP_B_C");
    }

    #[test]
    fn reads_field_names() {
        assert_eq!(field_name("Id_2_EACC536440B989E761A1A387817609BD"), "Id");
        assert_eq!(field_name("Max_Value_12_EACC536440B989E761A1A387817609BD"), "Max_Value");
        assert_eq!(field_name("Plain"), "Plain");
        assert_eq!(field_name("A_1_B"), "A_1_B");
    }
}
