//! An Unreal project's Loom sources: every `.lm` under `Content`, each built into the Blueprint or
//! enum beside it, against the types the plugin last saved. The plugin asks for the sources with
//! `loom sources` and compiles them with `loom build`, and `loom check` and `loom types` read the
//! project the same way, without Unreal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;

use crate::{lower, ops, syntax, types};

pub struct Project {
    pub root: PathBuf,
}

pub struct Source {
    /// The full path.
    pub path: PathBuf,
    /// Relative to the project, as errors name it.
    pub file: String,
    /// Where its Blueprint or enum must be: the file's own place under /Game.
    pub package: String,
}

impl Source {
    /// The folders it uses, for whatever loads the types to load before building it. A source that
    /// doesn't parse uses none, and building it says why.
    pub fn uses(&self) -> Vec<String> {
        syntax::parse_file(&self.path).map(|source| source.uses()).unwrap_or_default()
    }
}

/// What is wrong with a project's sources, and where. The fields are those of the plugin's report.
#[derive(Serialize)]
pub struct Error {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub message: String,
}

impl Error {
    pub fn new(file: &str, message: impl Into<String>) -> Error {
        Error { file: file.to_string(), line: None, column: None, message: message.into() }
    }

    /// From the compiler's own error, whose message starts with its place, `4:18: ...`, when it
    /// has one.
    fn from_compiler(file: &str, error: &anyhow::Error) -> Error {
        let text = format!("{error:#}");
        let mut parts = text.splitn(3, ':');
        if let (Some(line), Some(column), Some(message)) = (parts.next(), parts.next(), parts.next()) {
            if let (Ok(line), Ok(column)) = (line.parse(), column.parse()) {
                return Error { file: file.to_string(), line: Some(line), column: Some(column), message: message.trim_start().to_string() };
            }
        }
        Error::new(file, text)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.file.is_empty() {
            write!(f, "{}:", self.file)?;
            if let (Some(line), Some(column)) = (self.line, self.column) {
                write!(f, "{line}:{column}:")?;
            }
            write!(f, " ")?;
        }
        write!(f, "{}", self.message)
    }
}

#[derive(Serialize)]
pub struct Check {
    pub ok: bool,
    pub sources: usize,
    pub errors: Vec<Error>,
    /// Names and paths the compile looked up as types and the types don't have, when it failed:
    /// what the plugin may load, save in the types and compile again with.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// A compile of every source: what is wrong, or when nothing is, each asset's ops.
pub struct Compiled {
    pub check: Check,
    /// Each Blueprint's or enum's name, its class or enum, and its ops.
    pub blueprints: Vec<(String, String, ops::Document)>,
}

impl Compiled {
    /// Writes each Blueprint's ops to a file named after its package, which no other Blueprint
    /// shares: `/Game/X/BP_A` to `Game/X/BP_A.json`. Then `build.json`: the version of Loom that
    /// made them, and those files in the order to apply them. Nothing is written when there is
    /// nothing to build.
    pub fn write(&self, out_dir: &Path) -> Result<()> {
        if self.blueprints.is_empty() {
            return Ok(());
        }
        let file_of = |class: &str| format!("{}.json", package_of(class).trim_start_matches('/'));
        let docs: Vec<(&str, String)> = self.blueprints.iter().map(|(_, class, doc)| (class.as_str(), doc.to_json())).collect();
        for (class, ops) in &docs {
            let out = out_dir.join(file_of(class));
            std::fs::create_dir_all(out.parent().unwrap())?;
            std::fs::write(&out, ops.clone() + "\n").with_context(|| format!("writing {}", out.display()))?;
        }

        // A Blueprint uses another, or an enum, when its ops name the other's class or the enum.
        // Each is applied after those it uses, since its nodes refer to them.
        let (order, cycles) = build_order(&docs);
        if let Some(cycle) = cycles.first() {
            bail!("{}", cycle_message(&docs, cycle));
        }
        let manifest = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "order": order.iter().map(|class| file_of(class)).collect::<Vec<_>>(),
        });
        std::fs::write(out_dir.join("build.json"), serde_json::to_string_pretty(&manifest)? + "\n")?;
        Ok(())
    }
}

/// The package a class path is in: `/Game/X/BP_A` from `/Game/X/BP_A.BP_A_C`.
fn package_of(class: &str) -> &str {
    class.split_once('.').map_or(class, |(package, _)| package)
}

/// A Blueprint uses another, or an enum, when its ops name the other's class or the enum, and is
/// built after it, since its nodes refer to it. `docs` is each Blueprint's class and its ops as
/// JSON. The order is every class, each after those it uses. The cycles are the Blueprints that use
/// each other so that no such order exists: each a chain of classes where each uses the next, and
/// the last uses the first, which [`walk_cycle`] takes one at a time from what is left.
fn build_order<'a>(docs: &[(&'a str, String)]) -> (Vec<&'a str>, Vec<Vec<&'a str>>) {
    let mut order: Vec<&str> = Vec::new();
    // What a cycle already found holds: what uses it is ordered after it anyway.
    let mut cyclic: Vec<&str> = Vec::new();
    let mut cycles: Vec<Vec<&str>> = Vec::new();
    loop {
        let next = docs.iter().find(|(class, ops)| {
            !order.contains(class)
                && !cyclic.contains(class)
                && docs.iter().all(|(other, _)| {
                    other == class || order.contains(other) || cyclic.contains(other) || !ops.contains(*other)
                })
        });
        if let Some((class, _)) = next {
            order.push(class);
            continue;
        }
        let left: Vec<&str> = docs
            .iter()
            .map(|(class, _)| *class)
            .filter(|class| !order.contains(class) && !cyclic.contains(class))
            .collect();
        if left.is_empty() {
            return (order, cycles);
        }
        let cycle = walk_cycle(docs, &left);
        cyclic.extend(cycle.iter().copied());
        cycles.push(cycle);
    }
}

/// One cycle among the classes `left`, none of which can be ordered: from the first, each uses the
/// next, until the walk comes back onto a class it has walked through, which is the cycle. Every
/// class there uses one that is there too — one that used only ordered classes would be ordered.
fn walk_cycle<'a>(docs: &[(&'a str, String)], left: &[&'a str]) -> Vec<&'a str> {
    let uses = |class: &str, other: &str| docs.iter().any(|(c, ops)| *c == class && ops.contains(other));
    let mut path: Vec<&str> = vec![left[0]];
    loop {
        let current = *path.last().expect("the walk starts at a class");
        // Unreachable, and a class of its own is better named than none if it ever is.
        let Some(next) = left.iter().copied().find(|other| *other != current && uses(current, *other)) else {
            return path;
        };
        if let Some(at) = path.iter().position(|class| *class == next) {
            return path[at..].to_vec();
        }
        path.push(next);
    }
}

/// What to say about Blueprints that use each other in a cycle: which uses which, round it. A class
/// is named as source names it, by its short name, unless the project holds more than one of that
/// name, when its path is the only name that says which.
fn cycle_message(docs: &[(&str, String)], cycle: &[&str]) -> String {
    let name = |class: &str| {
        let short = short_name(class);
        let shared = docs.iter().filter(|(other, _)| short_name(other) == short).count() > 1;
        if shared { class.to_string() } else { short.to_string() }
    };
    let uses: Vec<String> = (0..cycle.len()).map(|i| format!("{} uses {}", name(cycle[i]), name(cycle[(i + 1) % cycle.len()]))).collect();
    let message = match uses.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    };
    format!("these Blueprints use each other in a cycle: {message}")
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The names a source that didn't parse would have declared, as far as they can be told without
/// parsing it: the name on its header line, and its file's name, which is the one a Blueprint,
/// struct or enum beside it has.
fn unparsed_names(source: &Source, unsaved: Option<&str>) -> Vec<String> {
    let text = match unsaved {
        Some(text) => Some(text.to_string()),
        None => std::fs::read_to_string(&source.path).ok(),
    };
    let mut names: Vec<String> = Vec::new();
    if let Some(name) = text.as_deref().and_then(header_name) {
        names.push(name);
    }
    if let Some(name) = source.package.rsplit('/').next() {
        if !names.iter().any(|held| held == name) {
            names.push(name.to_string());
        }
    }
    names
}

/// The name a source's header line gives, `blueprint BP_A : Actor at /Game/X/BP_A`, read without
/// parsing it: the word after its kind, and after `editor` for an editor Blueprint. None when the
/// line isn't there or isn't a header.
fn header_name(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with("//"))?;
    let mut words = line.split_whitespace();
    let mut kind = words.next()?;
    if kind == "editor" {
        kind = words.next()?;
    }
    if !matches!(kind, "blueprint" | "enum" | "struct" | "interface") {
        return None;
    }
    words.next().map(str::to_string)
}

/// Whether an error is only the knock-on of a source that didn't parse: it names a type that source
/// would have declared, which this compile never declared. Such an error says nothing about the
/// source it is in, and the syntax error that caused it is reported instead.
fn is_knock_on(error: &Error, unparsed: &[String]) -> bool {
    unparsed.iter().any(|name| error.message.contains(&format!("unknown type '{name}'")))
}

/// What the types are missing, less the names of the sources that didn't parse: those are the
/// project's own, so a check does not ask the plugin to go looking for them in the game.
fn missing_names(db: &types::TypeDb, unparsed: &[String]) -> Vec<String> {
    db.missing().into_iter().filter(|name| !unparsed.contains(name)).collect()
}

impl Check {
    pub fn text(&self) -> String {
        let mut out: String = self.errors.iter().map(|e| format!("{e}\n")).collect();
        out += &match (self.ok, self.sources, self.errors.len()) {
            (true, 1, _) => "1 source, ok\n".to_string(),
            (true, n, _) => format!("{n} sources, ok\n"),
            (false, _, 1) => "1 error\n".to_string(),
            (false, _, n) => format!("{n} errors\n"),
        };
        out
    }
}

impl Project {
    /// The project holding a folder: the nearest folder up from it with a .uproject. The folder's
    /// symlinks are not followed, so a source a project's Content links to is that project's.
    pub fn find(from: &Path) -> Result<Project> {
        let from = std::path::absolute(from).with_context(|| format!("reading {}", from.display()))?;
        for dir in from.ancestors() {
            let has_uproject = std::fs::read_dir(dir)
                .with_context(|| format!("reading {}", dir.display()))?
                .any(|entry| entry.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "uproject")));
            if has_uproject {
                return Ok(Project { root: dir.to_path_buf() });
            }
        }
        bail!("{} is not in an Unreal project: no folder up from it has a .uproject", from.display())
    }

    /// The types the plugin saved at its last build: the project's classes, structs and enums,
    /// without the Blueprints its own sources build.
    pub fn types_path(&self) -> PathBuf {
        self.root.join("Intermediate/Loom/types.json")
    }

    /// The types the plugin saved, read.
    pub fn type_db(&self) -> Result<types::TypeDb> {
        types::TypeDb::parse(&self.types_text()?).with_context(|| format!("reading {}", self.types_path().display()))
    }

    fn types_text(&self) -> Result<String> {
        let path = self.types_path();
        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
                "no types yet: the Loom plugin saves them to {} when it builds, and this project hasn't built. \
                 Build it once: with the project open in the editor, choose Tools › Rebuild All Blueprints, \
                 or run UnrealEditor-Cmd \"{}\" -run=LoomBuild -unattended -nullrhi, from the engine the project uses",
                path.display(),
                self.uproject()?.display()
            ),
            read => read.with_context(|| format!("reading {}", path.display())),
        }
    }

    /// The project's .uproject file.
    fn uproject(&self) -> Result<PathBuf> {
        for entry in std::fs::read_dir(&self.root).with_context(|| format!("reading {}", self.root.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|x| x == "uproject") {
                return Ok(path);
            }
        }
        bail!("{} has no .uproject", self.root.display())
    }

    /// Every .lm under Content, in order.
    pub fn sources(&self) -> Result<Vec<Source>> {
        let content = self.root.join("Content");
        let mut found = Vec::new();
        if content.is_dir() {
            find_sources(&content, &mut found)?;
        }
        found.sort();
        Ok(found
            .into_iter()
            .map(|path| {
                let relative = path.strip_prefix(&content).unwrap().to_string_lossy().replace('\\', "/");
                let package = format!("/Game/{}", relative.strip_suffix(".lm").unwrap());
                Source { file: format!("Content/{relative}"), package, path }
            })
            .collect())
    }

    /// Compiles every source, writing nothing.
    pub fn check(&self) -> Result<Check> {
        Ok(self.compile()?.check)
    }

    /// Compiles every source as [`Project::compile_with`] does, writing nothing.
    pub fn check_with(&self, unsaved: &HashMap<PathBuf, String>) -> Result<Check> {
        Ok(self.compile_with(unsaved)?.check)
    }

    /// Compiles every source, each into the Blueprint beside it. The ops name each source as the
    /// project does, `Content/...`.
    pub fn compile(&self) -> Result<Compiled> {
        self.compile_with(&HashMap::new())
    }

    /// Compiles every source as [`Project::compile`] does, but takes the text of a source whose
    /// full path, as [`Project::sources`] gives it, is in `unsaved` from there rather than from
    /// the file: an editor's text that isn't saved yet.
    pub fn compile_with(&self, unsaved: &HashMap<PathBuf, String>) -> Result<Compiled> {
        let sources = self.sources()?;
        let mut errors = Vec::new();
        let mut blueprints = Vec::new();
        let finish = |errors: Vec<Error>, blueprints, missing: Vec<String>| {
            let ok = errors.is_empty();
            let check = Check { ok, sources: sources.len(), errors, missing: if ok { Vec::new() } else { missing } };
            Ok(Compiled { blueprints: if check.ok { blueprints } else { Vec::new() }, check })
        };
        if sources.is_empty() {
            return finish(errors, blueprints, Vec::new());
        }
        let mut db = types::TypeDb::parse(&self.types_text()?)
            .with_context(|| format!("reading {}", self.types_path().display()))?;

        let mut files = Vec::new();
        // The names the sources that didn't parse would have declared. An error that names one of
        // them is the syntax error's knock-on, not its own source's fault.
        let mut unparsed: Vec<String> = Vec::new();
        for source in &sources {
            let parsed = match unsaved.get(&source.path) {
                Some(text) => syntax::parse_text(text, &source.path),
                None => syntax::parse_file(&source.path),
            };
            match parsed {
                Ok(file) if file.path() != source.package => errors.push(Error {
                    line: Some(1),
                    column: Some(1),
                    ..Error::new(
                        &source.file,
                        format!(
                            "its header says the {} is at {}, but the file is at {}; a source must be beside what it builds",
                            file.kind(),
                            file.path(),
                            source.package
                        ),
                    )
                }),
                Ok(file) => files.push((source, file)),
                Err(error) => {
                    for name in unparsed_names(source, unsaved.get(&source.path).map(String::as_str)) {
                        if !unparsed.contains(&name) {
                            unparsed.push(name);
                        }
                    }
                    errors.push(Error::from_compiler(&source.file, &error));
                }
            }
        }
        // A source that didn't parse cannot be declared or lowered, but the others can, so they are,
        // and the check reports their errors beside the syntax error: stopping at the first one
        // hides every type error in the project until it is fixed.
        if files.is_empty() {
            return finish(errors, blueprints, missing_names(&db, &unparsed));
        }
        let parsed: Vec<syntax::Source> = files.iter().map(|(_, f)| f.clone()).collect();
        if let Err(declare) = lower::declare(&parsed, &mut db) {
            // Each source is declared to the others, so one that cannot be declared stops the
            // lowering: what the others would read of it isn't there to read.
            let error = Error::from_compiler(&files[declare.file].0.file, &declare.error);
            if !is_knock_on(&error, &unparsed) {
                errors.push(error);
            }
            return finish(errors, blueprints, missing_names(&db, &unparsed));
        }
        let mut lowering_failed = false;
        for (source, file) in &files {
            match lower::lower_source(file, &source.file, &db).and_then(|doc| Ok((lower::asset_path(file)?, doc))) {
                Ok((class, doc)) => blueprints.push((file.name().to_string(), class, doc)),
                Err(error) => {
                    let error = Error::from_compiler(&source.file, &error);
                    if is_knock_on(&error, &unparsed) {
                        // Whatever reads this source is told the syntax error instead.
                        continue;
                    }
                    lowering_failed = true;
                    errors.push(error);
                }
            }
        }
        // A Blueprint whose ops name another's class is built after it, so ones that name each other
        // leave no order to build them in: LoomBuild finds that out only when it applies them, and
        // the check says it first, without the editor.
        if !lowering_failed {
            let docs: Vec<(&str, String)> = blueprints.iter().map(|(_, class, doc)| (class.as_str(), doc.to_json())).collect();
            for cycle in build_order(&docs).1 {
                let file = files
                    .iter()
                    .find(|(source, _)| source.package == package_of(cycle[0]))
                    .map_or(String::new(), |(source, _)| source.file.clone());
                errors.push(Error::new(&file, cycle_message(&docs, &cycle)));
            }
        }
        let missing = missing_names(&db, &unparsed);
        finish(errors, blueprints, missing)
    }

    /// What the project's types say about a class, struct or enum, by its name or path, or a
    /// function, as `Class.Function`.
    pub fn types(&self, name: &str) -> Result<String> {
        lookup(&self.types_dump()?, name)
    }

    /// The types the plugin saved, read whole, for [`lookup`] to look up many names in.
    pub fn types_dump(&self) -> Result<Value> {
        serde_json::from_str(&self.types_text()?).with_context(|| format!("reading {}", self.types_path().display()))
    }
}

fn find_sources(dir: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            find_sources(&path, found)?;
        } else if path.extension().is_some_and(|e| e == "lm") {
            found.push(path);
        }
    }
    Ok(())
}

/// The name source calls a type by: a Blueprint's class, /Game/X/Name.Name_C, by its Blueprint's.
fn short_name(path: &str) -> &str {
    let Some((package, name)) = path.rsplit_once('.') else { return path };
    let asset = package.rsplit('/').next().unwrap_or(package);
    name.strip_suffix("_C").filter(|stem| *stem == asset).unwrap_or(name)
}

const KINDS: [(&str, &str); 3] = [("classes", "class"), ("structs", "struct"), ("enums", "enum")];

/// What [`Project::types`] says about a name, in types already read.
pub fn lookup(dump: &Value, name: &str) -> Result<String> {
    let entries = |kind: &str| dump[kind].as_object().into_iter().flatten();
    let matches = |path: &str| path == name || short_name(path).eq_ignore_ascii_case(name);

    let mut out = String::new();
    for (kind, label) in KINDS {
        for (path, entry) in entries(kind).filter(|(path, _)| matches(path)) {
            out += &format!("{label} {path}");
            if kind == "classes" {
                let mut chain = Vec::new();
                let mut next = entry["super"].as_str();
                while let Some(parent) = next {
                    chain.push(short_name(parent));
                    next = dump["classes"][parent]["super"].as_str();
                }
                if !chain.is_empty() {
                    out += &format!(" : {}", chain.join(" : "));
                }
            }
            out += &format!("\n{}\n\n", serde_json::to_string_pretty(entry)?);
            if kind == "structs" {
                let hint = native_hint(dump, entry);
                if !hint.is_empty() {
                    out += &format!("{hint}\n\n");
                }
            }
        }
    }

    // Class.Function, looked for in the class and then each class it derives from.
    if let Some((class, function)) = name.rsplit_once('.').filter(|_| !name.starts_with('/')) {
        for (path, _) in entries("classes").filter(|(path, _)| short_name(path).eq_ignore_ascii_case(class)) {
            let mut next = Some(path.as_str());
            while let Some(owner) = next {
                let found = dump["classes"][owner]["functions"].as_object().into_iter().flatten()
                    .find(|(name, _)| name.eq_ignore_ascii_case(function));
                if let Some((function, entry)) = found {
                    out += &format!("function {owner}.{function}\n{}\n\n", serde_json::to_string_pretty(entry)?);
                    break;
                }
                next = dump["classes"][owner]["super"].as_str();
            }
        }
    }

    if out.is_empty() {
        let wanted = name.to_lowercase();
        let mut similar: Vec<&str> = KINDS
            .iter()
            .flat_map(|(kind, _)| entries(kind).map(|(path, _)| short_name(path)))
            .filter(|short| short.to_lowercase().contains(&wanted))
            .collect();
        similar.sort();
        similar.dedup();
        if similar.is_empty() {
            bail!("the project's types have nothing named {name}");
        }
        let more = similar.len().saturating_sub(20);
        similar.truncate(20);
        let mut list = similar.join(", ");
        if more > 0 {
            list += &format!(", and {more} more");
        }
        bail!("the project's types have nothing named {name}; names that hold it: {list}");
    }
    Ok(out.trim_end().to_string() + "\n")
}

/// What a struct's own functions are called and what their pins are, for a struct Blueprints can
/// only build and read through them: `Transform` holds `Translation`, `Rotation` and `Scale3D`,
/// while `MakeTransform` takes `Location`, `Rotation` and `Scale` and `BreakTransform` gives those
/// back, so its fields are not the names to write. Empty for a struct without such functions, and
/// the pins are left out when the functions aren't in the types to read them from.
fn native_hint(dump: &Value, entry: &Value) -> String {
    let mut lines = Vec::new();
    if let Some((class, function)) = entry["native_break"].as_str().and_then(function_of) {
        let pins = pins_of(dump, class, function, &["out", "return"]).map_or(String::new(), |pins| format!(", whose outputs are {pins}"));
        lines.push(format!("fields are read through {function}{pins}"));
    }
    if let Some((class, function)) = entry["native_make"].as_str().and_then(function_of) {
        let pins = pins_of(dump, class, function, &["in"]).map_or(String::new(), |pins| format!(", which takes {pins}"));
        lines.push(format!("built by {function}{pins}"));
    }
    lines.join("\n")
}

/// A dump path of a function, `/Script/Engine.KismetMathLibrary.MakeTransform`, as the class it is
/// in and its name.
fn function_of(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once('.')
}

/// The pins a function takes or gives, as the types dump names them: `Location (Vector), Rotation
/// (Rotator) and Scale (Vector)`. `dirs` picks them by the direction they pass in, and the pins a
/// node fills itself are left out. None when the function isn't in the types.
fn pins_of(dump: &Value, class: &str, function: &str, dirs: &[&str]) -> Option<String> {
    let params = dump["classes"][class]["functions"][function]["params"].as_array()?;
    let names: Vec<String> = params
        .iter()
        .filter(|p| dirs.contains(&p["dir"].as_str().unwrap_or("")) && p["wired"].as_bool() != Some(true))
        .map(|p| {
            let name = p["name"].as_str().unwrap_or("");
            match types::Ty::from_dump(p["type"].as_str().unwrap_or("")) {
                Some(ty) => format!("{name} ({ty})"),
                None => name.to_string(),
            }
        })
        .collect();
    (!names.is_empty()).then(|| and_list(&names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump() -> Value {
        serde_json::from_str(crate::fixtures::engine()).unwrap()
    }

    #[test]
    fn looks_up_a_class_by_its_short_name() {
        let text = lookup(&dump(), "actor").unwrap();
        assert!(text.starts_with("class /Script/Engine.Actor : Object"), "{text}");
    }

    #[test]
    fn looks_up_an_inherited_function() {
        let text = lookup(&dump(), "Pawn.K2_DestroyActor").unwrap();
        assert!(text.starts_with("function /Script/Engine.Actor.K2_DestroyActor"), "{text}");
    }

    /// The lines [`lookup`] adds for a struct's own make and break functions.
    fn hints(text: &str) -> String {
        text.lines().filter(|line| line.starts_with("fields are read through") || line.starts_with("built by")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_struct_built_by_its_own_functions_names_their_pins() {
        let text = lookup(&dump(), "Transform").unwrap();
        let hints = hints(&text);
        assert!(
            hints.contains("fields are read through BreakTransform, whose outputs are Location (Vector), Rotation (Rotator) and Scale (Vector)"),
            "{hints}"
        );
        assert!(
            hints.contains("built by MakeTransform, which takes Location (Vector), Rotation (Rotator) and Scale (Vector)"),
            "{hints}"
        );
    }

    #[test]
    fn a_struct_whose_fields_are_its_pins_names_them_too() {
        let hints = hints(&lookup(&dump(), "Vector").unwrap());
        assert!(hints.contains("fields are read through BreakVector, whose outputs are X (float), Y (float) and Z (float)"), "{hints}");
        assert!(hints.contains("built by MakeVector, which takes X (float), Y (float) and Z (float)"), "{hints}");
    }

    #[test]
    fn a_struct_without_such_functions_says_nothing_extra() {
        let text = lookup(&dump(), "LinearColor").unwrap();
        assert!(hints(&text).is_empty(), "{text}");
    }

    #[test]
    fn suggests_names_that_hold_what_was_asked() {
        let error = lookup(&dump(), "DestroyAct").unwrap_err().to_string();
        assert!(error.contains("nothing named DestroyAct"), "{error}");
    }

    #[test]
    fn counts_errors() {
        let check = |n| Check { ok: n == 0, sources: 2, errors: (0..n).map(|_| Error::new("", "wrong")).collect(), missing: Vec::new() };
        assert!(check(1).text().ends_with("\n1 error\n"));
        assert!(check(2).text().ends_with("\n2 errors\n"));
        assert!(check(0).text().ends_with("2 sources, ok\n"));
    }

    #[test]
    fn a_project_without_types_says_how_to_build_it() {
        let project = project("untyped", &[]);
        std::fs::remove_file(project.types_path()).unwrap();
        let error = format!("{:#}", project.type_db().err().unwrap());
        let uproject = project.root.join("Test.uproject");
        assert!(error.contains("Tools › Rebuild All Blueprints"), "{error}");
        assert!(error.contains(&format!(r#"UnrealEditor-Cmd "{}" -run=LoomBuild"#, uproject.display())), "{error}");
        assert!(!error.contains("No such file"), "{error}");
    }

    /// A project on disk with the test types and these sources, by their paths under Content.
    fn project(name: &str, sources: &[(&str, &str)]) -> Project {
        project_with(name, crate::fixtures::SMALL, sources)
    }

    /// A project on disk with the engine's types, for sources that name a class only the engine has,
    /// such as a widget's UserWidget.
    fn engine_project(name: &str, sources: &[(&str, &str)]) -> Project {
        project_with(name, crate::fixtures::engine(), sources)
    }

    /// A project on disk with these types and sources, by their paths under Content.
    fn project_with(name: &str, types: &str, sources: &[(&str, &str)]) -> Project {
        let root = std::env::temp_dir().join(format!("loom-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Intermediate/Loom")).unwrap();
        std::fs::write(root.join("Test.uproject"), "{}").unwrap();
        std::fs::write(root.join("Intermediate/Loom/types.json"), types).unwrap();
        for (path, text) in sources {
            let file = root.join("Content").join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        Project { root }
    }

    #[cfg(unix)]
    #[test]
    fn a_source_linked_into_content_belongs_to_the_project() {
        let project = project("linked", &[]);
        let outside = std::env::temp_dir().join(format!("loom-test-linked-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("BP_A.lm"), "blueprint BP_A : Actor at /Game/Linked/BP_A\n").unwrap();
        std::fs::create_dir_all(project.root.join("Content")).unwrap();
        std::os::unix::fs::symlink(&outside, project.root.join("Content/Linked")).unwrap();
        let found = Project::find(&project.root.join("Content/Linked")).unwrap();
        assert_eq!(found.root, project.root);
        assert!(Project::find(&outside).is_err());
    }

    #[test]
    fn an_ambiguous_name_is_an_error_of_the_source_that_wrote_it() {
        let project = project("ambiguous", &[
            ("One/BP_A.lm", "blueprint BP_A : Actor at /Game/One/BP_A\n"),
            ("Two/BP_A.lm", "blueprint BP_A : Actor at /Game/Two/BP_A\n"),
            ("Three/BP_Other.lm", "blueprint BP_Other : Actor at /Game/Three/BP_Other\nvar Thing: BP_A\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        assert_eq!(check.errors[0].file, "Content/Three/BP_Other.lm", "{}", check.text());
        assert!(check.errors[0].message.contains("BP_A is ambiguous"), "{}", check.text());
    }

    #[test]
    fn a_source_lists_the_folders_it_uses() {
        let project = project("uses", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nuse /Game/Lib\nuse /Game/Shop/Items\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nvar\n"),
        ]);
        let uses: Vec<Vec<String>> = project.sources().unwrap().iter().map(Source::uses).collect();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(uses, [vec!["/Game/Lib".to_string(), "/Game/Shop/Items".to_string()], vec![]]);
    }

    #[test]
    fn a_failed_check_names_the_types_it_could_not_find() {
        let project = project("missing", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nvar Row: HudQuestEntry\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert!(!check.ok);
        assert!(check.missing.contains(&"HudQuestEntry".to_string()), "{:?}", check.missing);
    }

    #[test]
    fn blueprints_that_use_each_other_are_an_error_of_the_check() {
        let project = engine_project("cycle", &[
            ("Mod/BP_MapLoad.lm", "blueprint BP_MapLoad : Actor at /Game/Mod/BP_MapLoad\nvar Chains: WBP_Chains\n"),
            ("Mod/WBP_Chains.lm", "blueprint WBP_Chains : UserWidget at /Game/Mod/WBP_Chains\nvar Map: BP_MapLoad\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert!(!check.ok);
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        assert_eq!(check.errors[0].file, "Content/Mod/BP_MapLoad.lm", "{}", check.text());
        assert_eq!(check.errors[0].message, "these Blueprints use each other in a cycle: BP_MapLoad uses WBP_Chains, and WBP_Chains uses BP_MapLoad", "{}", check.text());
    }

    #[test]
    fn a_blueprint_that_uses_a_widget_that_uses_nothing_is_no_cycle() {
        let project = engine_project("no-cycle", &[
            ("Mod/BP_MapLoad.lm", "blueprint BP_MapLoad : Actor at /Game/Mod/BP_MapLoad\nvar Chains: WBP_Chains\n"),
            // Its parent is the engine's UserWidget, which no source builds, so nothing comes back.
            ("Mod/WBP_Chains.lm", "blueprint WBP_Chains : UserWidget at /Game/Mod/WBP_Chains\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert!(check.ok, "{}", check.text());
    }

    #[test]
    fn separate_cycles_are_separate_errors() {
        let project = project("cycles", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nvar B: BP_B\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nvar A: BP_A\n"),
            ("X/BP_C.lm", "blueprint BP_C : Actor at /Game/X/BP_C\nvar D: BP_D\n"),
            ("X/BP_D.lm", "blueprint BP_D : Actor at /Game/X/BP_D\nvar C: BP_C\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(check.errors.len(), 2, "{}", check.text());
        assert!(check.errors[0].message.contains("BP_A uses BP_B"), "{}", check.text());
        assert!(check.errors[1].message.contains("BP_C uses BP_D"), "{}", check.text());
    }

    #[test]
    fn a_cycle_names_the_class_when_its_short_name_is_shared() {
        let project = project("cycle-names", &[
            ("One/BP_A.lm", "blueprint BP_A : Actor at /Game/One/BP_A\nvar B: BP_B\n"),
            ("One/BP_B.lm", "blueprint BP_B : Actor at /Game/One/BP_B\n"),
            ("Two/BP_A.lm", "blueprint BP_A : Actor at /Game/Two/BP_A\nvar B: BP_B\n"),
            ("Two/BP_B.lm", "blueprint BP_B : Actor at /Game/Two/BP_B\nvar A: BP_A\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        let message = &check.errors[0].message;
        assert!(message.contains("/Game/Two/BP_A.BP_A_C uses /Game/Two/BP_B.BP_B_C"), "{message}");
        assert!(message.contains("/Game/Two/BP_B.BP_B_C uses /Game/Two/BP_A.BP_A_C"), "{message}");
    }

    #[test]
    fn blueprints_of_one_name_are_written_and_ordered_apart() {
        let project = project("write", &[
            ("One/BP_A.lm", "blueprint BP_A : Actor at /Game/One/BP_A\n"),
            ("One/BP_B.lm", "blueprint BP_B : Actor at /Game/One/BP_B\nvar Thing: BP_A\n"),
            ("Two/BP_A.lm", "blueprint BP_A : Actor at /Game/Two/BP_A\n"),
        ]);
        let compiled = project.compile().unwrap();
        assert!(compiled.check.ok, "{}", compiled.check.text());
        let out = project.root.join("ops");
        compiled.write(&out).unwrap();
        let manifest: Value = serde_json::from_str(&std::fs::read_to_string(out.join("build.json")).unwrap()).unwrap();
        let order: Vec<&str> = manifest["order"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        let files: Vec<bool> = order.iter().map(|f| out.join(f).exists()).collect();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(order.len(), 3, "{order:?}");
        assert!(files.iter().all(|f| *f), "{order:?}");
        let at = |file: &str| order.iter().position(|f| *f == file).unwrap_or_else(|| panic!("{file} not in {order:?}"));
        assert!(at("Game/One/BP_A.json") < at("Game/One/BP_B.json"), "{order:?}");
        at("Game/Two/BP_A.json");
    }

    #[test]
    fn unsaved_text_is_compiled_in_place_of_the_file() {
        let project = project("unsaved", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nvar Count: int\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nvar Other: BP_A\n"),
        ]);
        let a = project.root.join("Content/X/BP_A.lm");
        let broken = HashMap::from([(a.clone(), "blueprint BP_A : Actor at /Game/X/BP_A\nvar Count int\n".to_string())]);
        let with_broken = project.check_with(&broken);
        let fixed = HashMap::from([(a.clone(), "blueprint BP_A : Actor at /Game/X/BP_A\nvar Count: float\n".to_string())]);
        let with_fixed = project.check_with(&fixed);
        let on_disk = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);

        let check = with_broken.unwrap();
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        let error = &check.errors[0];
        assert_eq!((error.file.as_str(), error.line, error.column), ("Content/X/BP_A.lm", Some(2), Some(11)), "{}", check.text());
        assert!(with_fixed.unwrap().ok);
        assert!(on_disk.ok, "{}", on_disk.text());
    }

    #[test]
    fn reads_an_error_place() {
        let error = Error::from_compiler("Content/A.lm", &anyhow::anyhow!("4:18: expected a name, found '('"));
        assert_eq!(error.to_string(), "Content/A.lm:4:18: expected a name, found '('");
    }

    #[test]
    fn a_syntax_error_leaves_the_other_sources_lowered() {
        let project = project("parse-then-lower", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nvar Count int\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nvar Nope: NotAType\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        let files: Vec<&str> = check.errors.iter().map(|error| error.file.as_str()).collect();
        assert_eq!(check.errors.len(), 2, "{}", check.text());
        assert!(files.contains(&"Content/X/BP_A.lm"), "{}", check.text());
        assert!(files.contains(&"Content/X/BP_B.lm"), "{}", check.text());
        assert!(check.errors.iter().any(|error| error.message.contains("unknown type 'NotAType'")), "{}", check.text());
    }

    #[test]
    fn a_syntax_error_is_not_laid_at_the_source_that_reads_its_type() {
        let project = project("parse-knock-on", &[
            ("X/BP_A.lm", "blueprint BP_A : Actor at /Game/X/BP_A\nvar Count int\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nfn F() {\nlet a: BP_A = none\n}\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        assert_eq!(check.errors[0].file, "Content/X/BP_A.lm", "{}", check.text());
        assert!(!check.missing.contains(&"BP_A".to_string()), "{:?}", check.missing);
    }

    #[test]
    fn a_header_that_names_another_type_still_holds_that_name_back() {
        // The file is BP_A.lm, but the header it was going to write is BP_Real's: the error in the
        // file that reads BP_Real is its knock-on, and BP_Real is not a type to look for in the game.
        let project = project("parse-header-name", &[
            ("X/BP_A.lm", "blueprint BP_Real : Actor at /Game/X/BP_A\nvar Count int\n"),
            ("X/BP_B.lm", "blueprint BP_B : Actor at /Game/X/BP_B\nfn F() {\nlet a: BP_Real = none\n}\n"),
        ]);
        let check = project.check().unwrap();
        let _ = std::fs::remove_dir_all(&project.root);
        assert_eq!(check.errors.len(), 1, "{}", check.text());
        assert_eq!(check.errors[0].file, "Content/X/BP_A.lm", "{}", check.text());
        assert!(!check.missing.contains(&"BP_Real".to_string()), "{:?}", check.missing);
    }
}
