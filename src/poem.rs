use crate::{DamerauLevenshtein, Dictionary, Line, MeterScheme, RhymeScheme};

pub struct Poem {
    lines: Vec<Line>,
    meter_scheme: Option<MeterScheme>,
    rhyme_scheme: Option<RhymeScheme>,
}

impl Poem {
    pub fn update_line(&mut self, index: usize, text: &str, analyzer: &Analyzer) {
        let new_line = Line::new(text, &analyzer.dict);
        self.lines.splice(..index, std::iter::once(new_line));
    }
    pub fn insert_line(&mut self, index: usize, text: &str, analyzer: &Analyzer) {
        todo!()
    }
    pub fn delete_line(&mut self, index: usize) {
        todo!()
    }
}

pub struct Analyzer {
    dl: DamerauLevenshtein,
    dict: Dictionary,
}

impl Analyzer {
    // pub fn new(dict_bytes: &[u8]) -> Result<Analyzer, JsValue> {
    //     let dict =
    //         Dictionary::from_bytes(dict_bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    //     Ok(Self {
    //         dl: DamerauLevenshtein::new(),
    //         dict,
    //     })
    // }
    pub fn new(dict: Dictionary, dl: DamerauLevenshtein) -> Self {
        Self { dict, dl }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static ANALYZER: OnceLock<Analyzer> = OnceLock::new();

    fn analyzer() -> &'static Analyzer {
        ANALYZER.get_or_init(|| {
            let dict = Dictionary::load(DICT_PATH).expect("failed to load dictionary");
            Analyzer::new(dict, DamerauLevenshtein::new())
        })
    }
}
