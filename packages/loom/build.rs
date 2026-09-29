//! Embeds the language reference, every page of `reference/`, so the compiler says what the version
//! it is says, and the pages stay the one copy.

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut pages: Vec<_> = std::fs::read_dir(&dir)
        .expect("the repo's reference/ folder")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    pages.sort();
    let mut out = String::from("pub const PAGES: &[(&str, &str)] = &[\n");
    for path in pages {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let path = path.canonicalize().unwrap();
        out += &format!("    ({name:?}, include_str!({:?})),\n", path.display().to_string());
    }
    out += "];\n";
    let target = Path::new(&std::env::var("OUT_DIR").unwrap()).join("reference.rs");
    std::fs::write(target, out).unwrap();
}
