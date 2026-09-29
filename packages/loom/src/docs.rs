//! The language reference, as the compiler carries it: `loom docs`, and the MCP server's `docs`
//! and `docs_search`.

use anyhow::{bail, Result};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/reference.rs"));
}

pub struct Page {
    pub name: &'static str,
    pub title: String,
    pub description: String,
    order: u32,
    body: &'static str,
}

impl Page {
    fn parse(name: &'static str, text: &'static str) -> Page {
        let (front, body) = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .unwrap_or_else(|| panic!("reference/{name}.md has no front matter"));
        let field = |key: &str| {
            front.lines().find_map(|line| line.trim().strip_prefix(key)?.strip_prefix(':')).map(|v| v.trim().to_string())
        };
        Page {
            name,
            title: field("title").unwrap_or_else(|| panic!("reference/{name}.md has no title")),
            description: field("description").unwrap_or_default(),
            order: field("order").and_then(|o| o.parse().ok()).unwrap_or(u32::MAX),
            body: body.trim_start(),
        }
    }

    /// The page as Markdown, headed by its title and description.
    pub fn markdown(&self) -> String {
        let mut body = String::new();
        let mut rest = self.body;
        // Comments are notes to whoever edits the page.
        while let Some((before, after)) = rest.split_once("<!--") {
            body += before;
            rest = after.split_once("-->").map_or("", |(_, after)| after.trim_start_matches('\n'));
        }
        body += rest;
        format!("# {}\n\n> {}\n\n{}", self.title, self.description, body.trim_start())
    }
}

/// The reference's pages, in the site's order.
pub fn pages() -> Vec<Page> {
    let mut pages: Vec<Page> = embedded::PAGES.iter().map(|(name, text)| Page::parse(name, text)).collect();
    pages.sort_by_key(|page| page.order);
    pages
}

/// Every page, a line each.
pub fn list() -> String {
    let pages = pages();
    let width = pages.iter().map(|p| p.name.len()).max().unwrap_or(0);
    pages.iter().map(|p| format!("{:width$}  {}: {}\n", p.name, p.title, p.description)).collect()
}

pub fn page(name: &str) -> Result<String> {
    let pages = pages();
    match pages.iter().find(|p| p.name == name) {
        Some(page) => Ok(page.markdown()),
        None => bail!(
            "no page {name}; the pages are {}",
            pages.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The sections, a page's `##` headings, that hold every word, with the lines that hold any.
pub fn search(words: &str) -> String {
    let words: Vec<String> = words.split_whitespace().map(str::to_lowercase).collect();
    let mut out = String::new();
    for page in pages() {
        let mut sections: Vec<(Option<&str>, Vec<&str>)> = vec![(None, Vec::new())];
        let mut fenced = false;
        for line in page.body.lines() {
            if line.starts_with("```") {
                fenced = !fenced;
            }
            match line.strip_prefix("## ") {
                Some(heading) if !fenced => sections.push((Some(heading), Vec::new())),
                _ => sections.last_mut().unwrap().1.push(line),
            }
        }
        for (heading, lines) in sections {
            let text = lines.join("\n").to_lowercase() + "\n" + &heading.unwrap_or("").to_lowercase();
            if words.is_empty() || !words.iter().all(|w| text.contains(w.as_str())) {
                continue;
            }
            out += &match heading {
                Some(heading) => format!("{}: {} > {heading}\n", page.name, page.title),
                None => format!("{}: {}\n", page.name, page.title),
            };
            let hits = lines.iter().filter(|l| words.iter().any(|w| l.to_lowercase().contains(w.as_str())));
            for line in hits.take(3) {
                out += &format!("    {}\n", line.trim());
            }
        }
    }
    if out.is_empty() {
        out = format!("no section of the reference mentions all of: {}\n", words.join(" "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_embedded_and_readable() {
        let pages = pages();
        assert!(pages.iter().any(|p| p.name == "index"));
        for page in &pages {
            assert!(!page.description.is_empty(), "reference/{}.md has no description", page.name);
            assert!(!page.markdown().contains("<!--"));
        }
    }

    #[test]
    fn search_finds_a_section() {
        assert!(search("dispatcher").contains("members"));
    }
}
