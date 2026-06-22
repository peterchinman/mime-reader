use crate::{
    DamerauLevenshtein, Dictionary, Line, MeterScheme, RhymeScheme,
    error::{MeterCheckError, PoemEditError, RhymeCheckError},
    meter::MeterMatchResult,
    rhyme::RhymeCheckResult,
};

pub struct PoemLine {
    line: Line,
    rhyme_check: Option<Result<RhymeCheckResult, RhymeCheckError>>,
    meter_check: Option<Result<MeterMatchResult, MeterCheckError>>,
}

pub struct Poem {
    lines: Vec<Line>,
    meter_scheme: Option<MeterScheme>,
    rhyme_scheme: Option<RhymeScheme>,
}

impl Poem {
    /// Updates the line at the given index with the given text.
    pub fn update_line(
        &mut self,
        index: usize,
        text: &str,
        analyzer: &Analyzer,
    ) -> Result<(), PoemEditError> {
        let Some(line) = self.lines.get_mut(index) else {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        };

        *line = Line::new(text, &analyzer.dict);
        Ok(())
    }
    /// Inserts a new line at the given index with the given text.
    pub fn insert_line(
        &mut self,
        index: usize,
        text: &str,
        analyzer: &Analyzer,
    ) -> Result<(), PoemEditError> {
        if index > self.lines.len() {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        }

        let inserted_line = Line::new(text, &analyzer.dict);
        self.lines.insert(index, inserted_line);
        Ok(())
    }
    /// Deletes the line at the given index.
    pub fn delete_line(&mut self, index: usize) -> Result<(), PoemEditError> {
        if index >= self.lines.len() {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        }

        self.lines.remove(index);
        Ok(())
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
