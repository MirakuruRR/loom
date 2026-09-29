//! Checks on the language reference in `reference/`, so it cannot drift from the compiler.
//!
//! Every ```loom block in a page is compiled against the engine's own types, as `crate::fixtures`
//! has them. A block is a whole source file; one that does not start with a header is given
//! `blueprint Example : Actor at /Game/Example`. Its info string may say more:
//!
//! - `error="text"`: the block must fail to compile, with an error containing the text.
//! - `program="name"`: the blocks of a page with the same name are one program, compiled
//!   together, in the order they appear.
//! - `prelude`: members every other block of the page without a header of its own starts with,
//!   such as variables its examples use. It is compiled on its own too.
//! - `refusals="name"`: the block is one row of a refusal list, which the site draws as one frame
//!   with the shared header once. The blocks of a list come one after another, three or more, each
//!   refused and starting with the same header line, written out. Each is still compiled on its own.
//!
//! A ```loom-outline block shows the shape of a source rather than an example of one, so what it
//! names needn't exist. It is only parsed.
//!
//! The pages together must show every construct in [`crate::syntax::CONSTRUCTS`], and every
//! construct the examples and the demos use must be in that list.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use anyhow::{anyhow, Result};

    use crate::lower::{declare, lower_source};
    use crate::syntax::{parse_source, resolve_includes, Source, CONSTRUCTS};
    use crate::types::TypeDb;

    const HEADER: &str = "blueprint Example : Actor at /Game/Example\n";

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A ```loom block: where it is, what it says, and what its info string asks.
    struct Block {
        at: String,
        source: String,
        error: Option<String>,
        program: Option<String>,
        refusals: Option<String>,
    }

    fn pages() -> Vec<PathBuf> {
        let mut pages: Vec<PathBuf> = std::fs::read_dir(repo().join("reference"))
            .expect("a reference/ folder")
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        pages.sort();
        assert!(!pages.is_empty(), "no pages in reference/");
        pages
    }

    fn blocks(page: &Path) -> Vec<Block> {
        let text = std::fs::read_to_string(page).unwrap();
        let mut prelude = String::new();
        let name = format!("reference/{}", page.file_name().unwrap().to_string_lossy());
        let mut out = Vec::new();
        let mut open: Option<(usize, String, Vec<&str>)> = None;
        for (i, line) in text.lines().enumerate() {
            match &mut open {
                None => {
                    if let Some(info) = line.strip_prefix("```") {
                        open = Some((i + 1, info.trim().to_string(), Vec::new()));
                    }
                }
                Some((start, info, lines)) => {
                    if line.trim_start() == "```" {
                        let (lang, attrs) = info.split_once(' ').unwrap_or((info.as_str(), ""));
                        if lang == "loom" {
                            let body = lines.join("\n") + "\n";
                            let first = body.lines().find(|l| !l.trim().is_empty() && !l.trim().starts_with("//")).unwrap_or("");
                            let headed = ["blueprint ", "editor blueprint ", "enum ", "struct ", "interface "].iter().any(|h| first.starts_with(h));
                            let is_prelude = attrs.split_whitespace().any(|w| w == "prelude");
                            if is_prelude {
                                prelude = body.clone();
                            }
                            let source = match (headed, is_prelude) {
                                (true, _) => body,
                                (false, true) => format!("{HEADER}{body}"),
                                (false, false) => format!("{HEADER}{prelude}{body}"),
                            };
                            out.push(Block {
                                at: format!("{name}:{start}"),
                                source,
                                error: attr(attrs, "error"),
                                program: attr(attrs, "program"),
                                refusals: attr(attrs, "refusals"),
                            });
                        }
                        open = None;
                    } else {
                        lines.push(line);
                    }
                }
            }
        }
        assert!(open.is_none(), "{name} ends inside a code block");
        out
    }

    /// The value of `key="value"` in an info string.
    fn attr(attrs: &str, key: &str) -> Option<String> {
        let start = attrs.find(&format!("{key}=\""))? + key.len() + 2;
        let len = attrs[start..].find('"')?;
        Some(attrs[start..start + len].to_string())
    }

    /// Compiles blocks as one program, as `loom build` does.
    fn compile(blocks: &[&Block], types: &str) -> Result<()> {
        let mut files = Vec::new();
        for block in blocks {
            let mut file = parse_source(&block.source).map_err(|e| anyhow!("{}: {e}", block.at))?;
            // an include is read from beside the pages, as from beside a source file
            if let Source::Blueprint(file) = &mut file {
                resolve_includes(file, &repo().join("reference")).map_err(|e| anyhow!("{}: {e}", block.at))?;
            }
            files.push(file);
        }
        let mut db = TypeDb::parse(types)?;
        declare(&files, &mut db)?;
        for (file, block) in files.iter().zip(blocks) {
            lower_source(file, &block.at, &db).map_err(|e| anyhow!("{}: {e}", block.at))?;
        }
        Ok(())
    }

    #[test]
    fn every_example_in_the_reference_does_what_it_says() {
        let types = crate::fixtures::engine();
        let mut failures = Vec::new();
        let mut checked = 0;
        for page in pages() {
            let blocks = blocks(&page);
            let mut programs: Vec<(Option<&str>, Vec<&Block>)> = Vec::new();
            for block in &blocks {
                match programs.iter_mut().find(|(p, _)| p.is_some() && *p == block.program.as_deref()) {
                    Some((_, members)) => members.push(block),
                    None => programs.push((block.program.as_deref(), vec![block])),
                }
            }
            for (_, program) in programs {
                checked += program.len();
                let expected = program.iter().find_map(|b| b.error.as_deref().map(|e| (b.at.as_str(), e)));
                match (compile(&program, types), expected) {
                    (Ok(()), None) => {}
                    (Err(err), None) => failures.push(format!("{err:#}")),
                    (Ok(()), Some((at, want))) => failures.push(format!("{at}: compiled, but should fail with \"{want}\"")),
                    (Err(err), Some((at, want))) => {
                        let err = format!("{err:#}");
                        if !err.contains(want) {
                            failures.push(format!("{at}: should fail with \"{want}\", but failed with: {err}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{} of {checked} reference examples are wrong:\n{}", failures.len(), failures.join("\n"));
    }

    #[test]
    fn every_refusal_list_is_three_or_more_refused_blocks_with_one_header() {
        let mut failures = Vec::new();
        for page in pages() {
            let blocks = blocks(&page);
            let mut i = 0;
            while i < blocks.len() {
                let Some(name) = blocks[i].refusals.as_deref() else {
                    i += 1;
                    continue;
                };
                let end = blocks[i..].iter().position(|b| b.refusals.as_deref() != Some(name)).map_or(blocks.len(), |n| i + n);
                let list = &blocks[i..end];
                let header = list[0].source.lines().next().unwrap_or("");
                if list.len() < 3 {
                    failures.push(format!("{}: the refusal list {name} has {} blocks, not three or more", list[0].at, list.len()));
                }
                // The site shows the header as the blocks have it, so they must write it.
                if header == HEADER.trim_end() {
                    failures.push(format!("{}: the refusal list {name} doesn't write its header", list[0].at));
                }
                for block in list {
                    if block.error.is_none() {
                        failures.push(format!("{}: a block of the refusal list {name} isn't refused", block.at));
                    }
                    if block.source.lines().next() != Some(header) {
                        failures.push(format!("{}: a block of the refusal list {name} doesn't start with {header}", block.at));
                    }
                }
                if blocks[end..].iter().any(|b| b.refusals.as_deref() == Some(name)) {
                    failures.push(format!("{}: the refusal list {name} is split by another block", list[0].at));
                }
                i = end;
            }
        }
        assert!(failures.is_empty(), "these refusal lists are wrong:\n{}", failures.join("\n"));
    }

    #[test]
    fn every_outline_in_the_reference_parses() {
        let mut failures = Vec::new();
        for page in pages() {
            let name = format!("reference/{}", page.file_name().unwrap().to_string_lossy());
            let text = std::fs::read_to_string(&page).unwrap();
            let mut lines = text.lines().enumerate();
            while let Some((start, line)) = lines.next() {
                if line.trim_end() != "```loom-outline" {
                    continue;
                }
                let body: Vec<&str> = lines.by_ref().map(|(_, l)| l).take_while(|l| l.trim_start() != "```").collect();
                if let Err(e) = parse_source(&(body.join("\n") + "\n")) {
                    failures.push(format!("{name}:{}: {e}", start + 1));
                }
            }
        }
        assert!(failures.is_empty(), "these outlines don't parse:\n{}", failures.join("\n"));
    }

    /// Every .lm file under a folder of the repo, parsed.
    fn sources(folder: &str) -> Vec<(String, Source)> {
        let mut out = Vec::new();
        let mut stack = vec![repo().join(folder)];
        while let Some(dir) = stack.pop() {
            for path in std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()) {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "lm") {
                    let name = path.strip_prefix(repo()).unwrap().display().to_string();
                    let file = parse_source(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
                    out.push((name, file));
                }
            }
        }
        out
    }

    #[test]
    fn the_reference_shows_every_construct() {
        let mut shown = BTreeSet::new();
        for page in pages() {
            for block in blocks(&page) {
                // An example of a mistake shows what not to write, so it covers nothing.
                if block.error.is_some() {
                    continue;
                }
                shown.extend(parse_source(&block.source).unwrap_or_else(|e| panic!("{}: {e}", block.at)).constructs());
            }
        }
        let missing: Vec<&str> = CONSTRUCTS.iter().copied().filter(|c| !shown.contains(c)).collect();
        assert!(missing.is_empty(), "the reference has no example of: {}", missing.join(", "));
    }

    #[test]
    fn every_construct_in_use_is_known() {
        let known: BTreeSet<&str> = CONSTRUCTS.iter().copied().collect();
        let mut unknown = Vec::new();
        for (name, file) in sources("examples").into_iter().chain(sources("demos")) {
            for construct in file.constructs() {
                if !known.contains(construct) {
                    unknown.push(format!("{name}: {construct}"));
                }
            }
        }
        assert!(unknown.is_empty(), "these are not in syntax::CONSTRUCTS:\n{}", unknown.join("\n"));
    }
}
