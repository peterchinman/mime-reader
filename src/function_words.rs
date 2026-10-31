/// One-syllable function words, which can go unstressed in a line even where
/// the dictionary only stresses them (AS, BY, FROM, MY).
#[rustfmt::skip]
const FUNCTION_WORDS: &[&str] = &[
    // Articles and determiners
    "a", "an", "the", "this", "that", "these", "those",
    // Prepositions
    "as", "at", "by", "for", "from", "in", "of", "on", "than", "through", "till", "to", "with",
    // Conjunctions
    "and", "but", "if", "nor", "or", "so", "though", "when", "where",
    // Pronouns and possessives
    "i", "me", "my", "we", "us", "our", "you", "your", "ye", "thou", "thee", "thy",
    "he", "him", "his", "she", "her", "it", "its", "they", "them", "their",
    // Relative pronouns
    "who", "whom", "whose", "which",
    // Auxiliaries
    "am", "is", "are", "was", "were", "be", "been",
    "has", "have", "had", "hast", "hath", "do", "does", "did", "doth",
    "can", "could", "may", "might", "must", "shall", "should", "will", "would",
];

/// Whether `word` is a one-syllable function word, in any letter case.
pub(crate) fn is_function_word(word: &str) -> bool {
    FUNCTION_WORDS
        .iter()
        .any(|function_word| function_word.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_function_words_in_any_case() {
        assert!(is_function_word("as"));
        assert!(is_function_word("As"));
        assert!(is_function_word("THY"));
    }

    #[test]
    fn rejects_content_words() {
        assert!(!is_function_word("dream"));
        assert!(!is_function_word("ask"));
    }
}
