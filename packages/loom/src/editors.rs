//! Checks on the editor grammars in `editors/`, so they highlight the words the parser knows.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use crate::builtins::PLAIN_TYPES;
    use crate::syntax::{CONTEXTUAL, LITERAL_WORDS, RESERVED, TYPE_WORDS};

    /// The words of a TextMate rule whose match is a list of them, \b(a|b|c)\b.
    fn words(grammar: &serde_json::Value, rule: &str) -> BTreeSet<String> {
        let pattern = grammar["repository"][rule]["match"].as_str().unwrap_or_else(|| panic!("no rule {rule} with a match"));
        let list = pattern
            .strip_prefix("\\b(")
            .and_then(|p| p.strip_suffix(")\\b"))
            .unwrap_or_else(|| panic!("rule {rule} is not a list of words: {pattern}"));
        list.split('|').map(str::to_string).collect()
    }

    /// Every word a rule's patterns name in a group of plain words, (a|b|c) or (?:a|b|c).
    fn words_in(grammar: &serde_json::Value, rule: &str) -> BTreeSet<String> {
        let patterns = grammar["repository"][rule]["patterns"].as_array().unwrap_or_else(|| panic!("no rule {rule} with patterns"));
        let mut found = BTreeSet::new();
        for pattern in patterns.iter().filter_map(|p| p["match"].as_str()) {
            for group in pattern.split('(').skip(1) {
                let group = group.strip_prefix("?:").unwrap_or(group);
                let Some((list, _)) = group.split_once(')') else { continue };
                if !list.is_empty() && list.chars().all(|c| c.is_ascii_lowercase() || c == '|') {
                    found.extend(list.split('|').map(str::to_string));
                }
            }
        }
        found
    }

    fn set(words: impl IntoIterator<Item = &'static str>) -> BTreeSet<String> {
        words.into_iter().map(str::to_string).collect()
    }

    #[test]
    fn the_textmate_grammar_highlights_every_word_the_parser_knows() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/textmate/loom.tmLanguage.json");
        let grammar: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        // A reserved word is a keyword everywhere, and a contextual one only where the grammar's
        // patterns expect it.
        assert_eq!(words(&grammar, "control"), set(RESERVED.iter().copied()), "the grammar's control differs from syntax::RESERVED");
        let contextual: BTreeSet<String> =
            words_in(&grammar, "keywords").union(&words_in(&grammar, "declaration")).filter(|w| !RESERVED.contains(&w.as_str())).cloned().collect();
        assert_eq!(contextual, set(CONTEXTUAL.iter().copied()), "the grammar's keywords and declarations differ from syntax::CONTEXTUAL");
        assert_eq!(
            words(&grammar, "types"),
            set(TYPE_WORDS.iter().copied().chain(PLAIN_TYPES.iter().map(|t| t.name))),
            "the grammar's types differ from syntax::TYPE_WORDS and builtins::PLAIN_TYPES"
        );
        assert_eq!(words(&grammar, "constants"), set(LITERAL_WORDS.iter().copied()), "the grammar's constants differ from syntax::LITERAL_WORDS");
    }
}
