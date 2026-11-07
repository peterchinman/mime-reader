/// One-syllable function words, which can go unstressed in a line even where
/// the dictionary only stresses them (AS, BY, FROM, MY).
///
/// The list holds the closed classes of words that go unstressed in running
/// speech: articles and determiners, prepositions, conjunctions, pronouns and
/// auxiliaries, with their archaic forms and contractions, and the vocative
/// O. It leaves out homographs that are as often content words, like ART and
/// WILT, and adverbs that often carry stress, like NOT, THEN and THERE.
#[rustfmt::skip]
const FUNCTION_WORDS: &[&str] = &[
    // Articles and determiners
    "a", "an", "the", "this", "that", "these", "those",
    // Prepositions
    "as", "at", "by", "ere", "for", "from", "in", "of", "on", "than", "through", "till", "to",
    "with",
    // Conjunctions
    "and", "but", "if", "nor", "or", "so", "though", "when", "where",
    // Pronouns and possessives
    "i", "me", "my", "we", "us", "our", "you", "your", "ye", "thou", "thee", "thy", "thine",
    "he", "him", "his", "she", "her", "it", "its", "they", "them", "their",
    // Relative pronouns
    "who", "whom", "whose", "which", "what",
    // Auxiliaries
    "am", "is", "are", "was", "wast", "were", "wert", "be", "been",
    "has", "have", "hast", "had", "hadst", "hath",
    "do", "does", "dost", "doth", "did", "didst",
    "can", "canst", "could", "couldst", "may", "mayst", "might", "must",
    "shall", "shalt", "should", "shouldst", "will", "would", "wouldst",
    // Contractions
    "'tis", "'twas",
    // The vocative O
    "o",
];

/// Whether `word` is a one-syllable function word, in any letter case, and
/// with a curly apostrophe (’tis) or a straight one ('tis).
pub(crate) fn is_function_word(word: &str) -> bool {
    if word.contains('\u{2019}') {
        return is_function_word(&word.replace('\u{2019}', "'"));
    }
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
    fn recognizes_contractions_with_either_apostrophe() {
        assert!(is_function_word("'tis"));
        assert!(is_function_word("’Tis"));
        assert!(is_function_word("’twas"));
    }

    #[test]
    fn rejects_content_words_and_their_homographs() {
        assert!(!is_function_word("dream"));
        assert!(!is_function_word("ask"));
        assert!(!is_function_word("art"));
        assert!(!is_function_word("wilt"));
    }
}
