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
        let file_of = |class: &str| {
            let package = class.split_once('.').map_or(class, |(package, _)| package);
            format!("{}.json", package.trim_start_matches('/'))
        };
        let docs: Vec<(&str, String)> = self.blueprints.iter().map(|(_, class, doc)| (class.as_str(), doc.to_json())).collect();
        for (class, ops) in &docs {
            let out = out_dir.join(file_of(class));
            std::fs::create_dir_all(out.parent().unwrap())?;
            std::fs::write(&out, ops.clone() + "\n").with_context(|| format!("writing {}", out.display()))?;
        }

        // A Blueprint uses another, or an enum, when its ops name the other's class or the enum.
        // Each is applied after those it uses, since its nodes refer to them.
        let mut order: Vec<&str> = Vec::new();
        while order.len() < docs.len() {
            let next = docs.iter().find(|(class, ops)| {
                !order.contains(class)
                    && docs.iter().all(|(other, _)| other == class || order.contains(other) || !ops.contains(*other))
            });
            match next {
                Some((class, _)) => order.push(class),
                None => {
                    let left: Vec<&str> = docs.iter().map(|(class, _)| *class).filter(|class| !order.contains(class)).collect();
                    bail!("these Blueprints use each other in a cycle: {}", left.join(", "))
                }
            }
        }
        let manifest = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "order": order.iter().map(|class| file_of(class)).collect::<Vec<_>>(),
        });
        std::fs::write(out_dir.join("build.json"), serde_json::to_string_pretty(&manifest)? + "\n")?;
        Ok(())
    }
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
                Err(error) => errors.push(Error::from_compiler(&source.file, &error)),
            }
        }
        // Each source is declared to the others, so all of them must have parsed.
        if !errors.is_empty() {
            return finish(errors, blueprints, db.missing());
        }
        let parsed: Vec<syntax::Source> = files.iter().map(|(_, f)| f.clone()).collect();
        if let Err(declare) = lower::declare(&parsed, &mut db) {
            return finish(vec![Error::from_compiler(&files[declare.file].0.file, &declare.error)], blueprints, db.missing());
        }
        for (source, file) in &files {
            match lower::lower_source(file, &source.file, &db).and_then(|doc| Ok((lower::asset_path(file)?, doc))) {
                Ok((class, doc)) => blueprints.push((file.name().to_string(), class, doc)),
                Err(error) => errors.push(Error::from_compiler(&source.file, &error)),
            }
        }
        let missing = db.missing();
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
        let root = std::env::temp_dir().join(format!("loom-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Intermediate/Loom")).unwrap();
        std::fs::write(root.join("Test.uproject"), "{}").unwrap();
        std::fs::write(root.join("Intermediate/Loom/types.json"), crate::fixtures::SMALL).unwrap();
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
}
