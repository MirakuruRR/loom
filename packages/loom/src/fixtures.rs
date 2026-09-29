//! The types the tests compile against without Unreal, in `tests/types/`: `small.json`, a few
//! classes written for the tests, and `engine-<version>.json`, every class, struct and enum of the
//! engine `tests/project` names, which `cargo xtask test-types` remakes from it.

use std::path::PathBuf;
use std::sync::OnceLock;

pub const SMALL: &str = include_str!("../tests/types/small.json");

/// The engine `tests/project` names, such as `5.6`.
fn test_project_engine() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/project/LoomTests.uproject");
    let project: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    project["EngineAssociation"].as_str().expect("tests/project names its engine").to_string()
}

pub fn engine_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/types/engine-{}.json", test_project_engine()))
}

/// The engine's types, read once.
pub fn engine() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let path = engine_path();
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; make it with cargo xtask test-types", path.display()))
    })
}

/// The engine's types were dumped from what the repository holds now: the engine `tests/project`
/// names, and each file they were dumped with, as `inputs` says, unchanged since.
#[test]
fn the_engine_types_are_up_to_date() {
    use sha2::Digest;
    let types: serde_json::Value = serde_json::from_str(engine()).unwrap();
    let inputs = &types["inputs"];
    let mut stale = Vec::new();
    let engine = test_project_engine();
    let dumped = inputs["engine"].as_str().unwrap_or("no engine");
    if !dumped.starts_with(&format!("{engine}.")) {
        stale.push(format!("they're {dumped}'s, and tests/project names {engine}"));
    }
    let files = inputs["files"].as_object().map(|f| f.iter().collect::<Vec<_>>()).unwrap_or_default();
    if files.is_empty() {
        stale.push("they list no files they were dumped with".to_string());
    }
    let repo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    for (file, hash) in files {
        let now = std::fs::read(repo.join(file))
            .map(|bytes| sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect::<String>());
        match now {
            Ok(now) if Some(now.as_str()) == hash.as_str() => {}
            Ok(_) => stale.push(format!("{file} has changed since")),
            Err(e) => stale.push(format!("{file}: {e}")),
        }
    }
    assert!(
        stale.is_empty(),
        "{} is out of date: {}. Remake it with cargo xtask test-types",
        engine_path().display(),
        stale.join("; ")
    );
}
