use crate::{Dictionary, dictionary::Pronunciations};

pub struct WordEntry {
    pub word: String,
    // None here means an unrecognized word
    pub pronunciations: Option<Pronunciations>,
}

pub struct Line {
    pub words: Vec<WordEntry>,
}

impl Line {
    pub fn new(s: &str, dict: &Dictionary) -> Self {
        let words = s.split_whitespace()
            .map(|w| WordEntry {
                word: w.to_string(),
                pronunciations: dict.lookup(w).ok().cloned(),
            })
            .collect::<Vec<_>>();
        Self { words }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<Dictionary> = OnceLock::new();

    fn dict() -> &'static Dictionary {
        DICT.get_or_init(|| Dictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    #[test]
    fn known_words_have_pronunciations() {
        let line = Line::new("hello world", dict());
        assert_eq!(line.words.len(), 2);
        assert!(line.words[0].pronunciations.is_some());
        assert!(line.words[1].pronunciations.is_some());
    }

    #[test]
    fn unknown_word_has_no_pronunciations() {
        let line = Line::new("hello xyzzy", dict());
        assert!(line.words[0].pronunciations.is_some());
        assert!(line.words[1].pronunciations.is_none());
    }

    #[test]
    fn word_with_multiple_pronunciations() {
        // "hello" has two entries in CMUdict: HELLO and HELLO(1)
        let line = Line::new("hello", dict());
        let pronunciations = line.words[0].pronunciations.as_ref().unwrap();
        assert_eq!(pronunciations.len(), 2);
    }

    #[test]
    fn original_words_are_preserved() {
        let line = Line::new("Hello World", dict());
        assert_eq!(line.words[0].word, "Hello");
        assert_eq!(line.words[1].word, "World");
    }
}