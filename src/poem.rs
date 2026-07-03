use crate::{
    DamerauLevenshtein, Dictionary, Line, MeterScheme, RhymeScheme,
    error::{MeterCheckError, PoemEditError, RhymeCheckError},
    meter::MeterMatchResult,
    rhyme::{LineRole, ResolvedLineRole, RhymeCheckResult},
};

#[derive(Debug)]
pub enum MeterAnalysis<'a> {
    NotApplicable,
    Ready(&'a Result<MeterMatchResult, MeterCheckError>),
}

#[derive(Debug)]
pub enum RhymeAnalysis<'a> {
    NotApplicable,
    Ready(&'a Result<RhymeCheckResult, RhymeCheckError>),
}

#[derive(Debug)]
pub struct PoemLineAnalysis<'a> {
    pub meter: MeterAnalysis<'a>,
    pub rhyme: RhymeAnalysis<'a>,
}

struct PoemLine {
    line: Line,
}

#[derive(Debug)]
struct LineAnalysis {
    meter: AnalysisState<MeterMatchResult, MeterCheckError>,
    rhyme: AnalysisState<RhymeCheckResult, RhymeCheckError>,
}

#[derive(Debug)]
enum AnalysisState<T, E> {
    Dirty,
    Ready(Result<T, E>),
    NotApplicable,
}

pub struct Poem {
    lines: Vec<PoemLine>,
    analyses: Vec<LineAnalysis>,
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

        line.line = Line::new(text, &analyzer.dict);
        // TODO: do we create analysis here? or is that separate?
        // TODO: only invalidate dependent lines
        self.invalidate_all();
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
        let inserted_poem_line = PoemLine {
            line: inserted_line,
        };
        self.lines.insert(index, inserted_poem_line);
        self.analyses.insert(
            index,
            Self::set_dirty_for(self.meter_scheme.is_some(), self.rhyme_scheme.is_some()),
        );
        // TODO optimize: only invalidate what we need to
        self.invalidate_all();
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
        self.analyses.remove(index);
        // TODO optimize
        self.invalidate_all();
        Ok(())
    }

    pub fn meter_analysis(&mut self, index: usize) -> Result<MeterAnalysis<'_>, PoemEditError> {
        self.ensure_meter_analysis(index)?;
        Ok(self.cached_meter_analysis_after_ensure(index))
    }

    fn ensure_meter_analysis(&mut self, index: usize) -> Result<(), PoemEditError> {
        if index >= self.lines.len() {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        }

        let Some(meter_scheme) = &self.meter_scheme else {
            self.analyses[index].meter = AnalysisState::NotApplicable;
            return Ok(());
        };

        if matches!(self.analyses[index].meter, AnalysisState::Dirty) {
            let result = meter_scheme.check_line(&self.lines[index].line, index);
            self.analyses[index].meter = AnalysisState::Ready(result);
        }

        Ok(())
    }

    fn cached_meter_analysis_after_ensure(&self, index: usize) -> MeterAnalysis<'_> {
        match &self.analyses[index].meter {
            AnalysisState::Ready(result) => MeterAnalysis::Ready(result),
            AnalysisState::NotApplicable => MeterAnalysis::NotApplicable,
            AnalysisState::Dirty => {
                unreachable!("ensure_meter_analysis should resolve dirty meter analysis")
            }
        }
    }

    fn resolved_line_role<'a>(
        &'a self,
        rhyme_scheme: &RhymeScheme,
        index: usize,
    ) -> Result<ResolvedLineRole<'a>, RhymeCheckError> {
        match rhyme_scheme.line_role(index) {
            LineRole::Leader => Ok(ResolvedLineRole::Leader),
            LineRole::Unconstrained => Ok(ResolvedLineRole::Unconstrained),
            LineRole::Follower { leader_index } => {
                let Some(leader) = self.lines.get(leader_index) else {
                    return Err(RhymeCheckError::TargetLineOutOfBounds {
                        target_index: leader_index,
                        line_count: self.lines.len(),
                    });
                };

                Ok(ResolvedLineRole::Follower {
                    leader_index,
                    leader: &leader.line,
                })
            }
        }
    }

    pub fn rhyme_analysis(
        &mut self,
        index: usize,
        analyzer: &Analyzer,
    ) -> Result<RhymeAnalysis<'_>, PoemEditError> {
        self.ensure_rhyme_analysis(index, analyzer)?;
        Ok(self.cached_rhyme_analysis_after_ensure(index))
    }

    fn ensure_rhyme_analysis(
        &mut self,
        index: usize,
        analyzer: &Analyzer,
    ) -> Result<(), PoemEditError> {
        if index >= self.lines.len() {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        }

        let Some(rhyme_scheme) = &self.rhyme_scheme else {
            self.analyses[index].rhyme = AnalysisState::NotApplicable;
            return Ok(());
        };

        if matches!(self.analyses[index].rhyme, AnalysisState::Dirty) {
            let role = self.resolved_line_role(rhyme_scheme, index);
            let result = role.and_then(|role| {
                rhyme_scheme.check_line(&self.lines[index].line, role, &analyzer.dl)
            });
            self.analyses[index].rhyme = AnalysisState::Ready(result);
        }

        Ok(())
    }

    fn cached_rhyme_analysis_after_ensure(&self, index: usize) -> RhymeAnalysis<'_> {
        match &self.analyses[index].rhyme {
            AnalysisState::Ready(result) => RhymeAnalysis::Ready(result),
            AnalysisState::NotApplicable => RhymeAnalysis::NotApplicable,
            AnalysisState::Dirty => {
                unreachable!("ensure_rhyme_analysis should resolve dirty rhyme analysis")
            }
        }
    }

    pub fn line_analysis(
        &mut self,
        index: usize,
        analyzer: &Analyzer,
    ) -> Result<PoemLineAnalysis<'_>, PoemEditError> {
        self.ensure_meter_analysis(index)?;
        self.ensure_rhyme_analysis(index, analyzer)?;

        Ok(PoemLineAnalysis {
            meter: self.cached_meter_analysis_after_ensure(index),
            rhyme: self.cached_rhyme_analysis_after_ensure(index),
        })
    }

    fn invalidate_all(&mut self) {
        let has_meter_scheme = self.meter_scheme.is_some();
        let has_rhyme_scheme = self.rhyme_scheme.is_some();

        for index in 0..self.analyses.len() {
            self.analyses[index] = Self::set_dirty_for(has_meter_scheme, has_rhyme_scheme);
        }
    }

    fn set_dirty_for(has_meter_scheme: bool, has_rhyme_scheme: bool) -> LineAnalysis {
        LineAnalysis {
            meter: if has_meter_scheme {
                AnalysisState::Dirty
            } else {
                AnalysisState::NotApplicable
            },
            rhyme: if has_rhyme_scheme {
                AnalysisState::Dirty
            } else {
                AnalysisState::NotApplicable
            },
        }
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

    fn poem_with_schemes(
        lines: &[&str],
        meter_scheme: Option<&str>,
        rhyme_scheme: Option<&str>,
    ) -> Poem {
        let meter_scheme = meter_scheme.map(|scheme| scheme.parse().unwrap());
        let rhyme_scheme = rhyme_scheme.map(|scheme| RhymeScheme::new(scheme, 0.5).unwrap());
        let has_meter_scheme = meter_scheme.is_some();
        let has_rhyme_scheme = rhyme_scheme.is_some();

        Poem {
            lines: lines
                .iter()
                .map(|text| PoemLine {
                    line: Line::new(text, &analyzer().dict),
                })
                .collect(),
            analyses: lines
                .iter()
                .map(|_| Poem::set_dirty_for(has_meter_scheme, has_rhyme_scheme))
                .collect(),
            meter_scheme,
            rhyme_scheme,
        }
    }

    fn poem_with_cached_analyses(lines: &[&str]) -> Poem {
        let mut poem = poem_with_schemes(lines, Some("x"), Some("AA"));
        poem.analyses = lines.iter().map(|_| cached_analysis()).collect();
        poem
    }

    fn cached_analysis() -> LineAnalysis {
        LineAnalysis {
            meter: AnalysisState::Ready(Ok(MeterMatchResult::Match)),
            rhyme: AnalysisState::Ready(Ok(RhymeCheckResult::Leader)),
        }
    }

    fn line_texts(poem: &Poem) -> Vec<String> {
        poem.lines
            .iter()
            .map(|poem_line| {
                poem_line
                    .line
                    .words
                    .iter()
                    .map(|word| word.word.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    fn assert_dirty_analyses_aligned_with_lines(poem: &Poem) {
        assert_eq!(poem.analyses.len(), poem.lines.len());

        for (index, (_line, analysis)) in poem.lines.iter().zip(&poem.analyses).enumerate() {
            assert!(
                matches!(&analysis.meter, AnalysisState::Dirty),
                "meter analysis at index {index} was not dirty"
            );
            assert!(
                matches!(&analysis.rhyme, AnalysisState::Dirty),
                "rhyme analysis at index {index} was not dirty"
            );
        }
    }

    fn assert_meter_ready_match(analysis: MeterAnalysis<'_>) {
        let MeterAnalysis::Ready(result) = analysis else {
            panic!("expected ready meter analysis, got {analysis:?}");
        };

        assert_eq!(result, &Ok(MeterMatchResult::Match));
    }

    fn assert_rhyme_ready_leader(analysis: RhymeAnalysis<'_>) {
        let RhymeAnalysis::Ready(result) = analysis else {
            panic!("expected ready rhyme analysis, got {analysis:?}");
        };

        assert_eq!(result, &Ok(RhymeCheckResult::Leader));
    }

    fn assert_rhyme_ready_unconstrained(analysis: RhymeAnalysis<'_>) {
        let RhymeAnalysis::Ready(result) = analysis else {
            panic!("expected ready rhyme analysis, got {analysis:?}");
        };

        assert_eq!(result, &Ok(RhymeCheckResult::Unconstrained));
    }

    fn assert_rhyme_ready_follower_passes(analysis: RhymeAnalysis<'_>, expected_leader: usize) {
        let RhymeAnalysis::Ready(result) = analysis else {
            panic!("expected ready rhyme analysis, got {analysis:?}");
        };

        let Ok(RhymeCheckResult::Follower {
            leader_index,
            distance,
            passed,
        }) = result
        else {
            panic!("expected passing follower rhyme result, got {result:?}");
        };

        assert_eq!(*leader_index, expected_leader);
        assert_eq!(distance.raw_distance, 0);
        assert_eq!(distance.syllable_count, 1);
        assert!(*passed);
    }

    fn assert_meter_state_ready(poem: &Poem, index: usize) {
        assert!(
            matches!(
                &poem.analyses[index].meter,
                AnalysisState::Ready(Ok(MeterMatchResult::Match))
            ),
            "meter analysis at index {index} was not ready: {:?}",
            poem.analyses[index]
        );
    }

    fn assert_rhyme_state_ready(poem: &Poem, index: usize) {
        assert!(
            matches!(&poem.analyses[index].rhyme, AnalysisState::Ready(_)),
            "rhyme analysis at index {index} was not ready: {:?}",
            poem.analyses[index]
        );
    }

    #[test]
    fn insert_line_keeps_lines_and_analyses_aligned() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog"]);

        poem.insert_line(1, "hat", analyzer()).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat", "dog"]);
        assert_dirty_analyses_aligned_with_lines(&poem);
    }

    #[test]
    fn delete_line_keeps_lines_and_analyses_aligned() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog", "hat"]);

        poem.delete_line(1).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat"]);
        assert_dirty_analyses_aligned_with_lines(&poem);
    }

    #[test]
    fn update_line_replaces_line_and_invalidates_existing_analyses() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog"]);

        poem.update_line(1, "hat", analyzer()).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat"]);
        assert_dirty_analyses_aligned_with_lines(&poem);
    }

    #[test]
    fn meter_analysis_computes_dirty_meter_cache() {
        let mut poem = poem_with_schemes(&["cat"], Some("/"), Some("A"));

        let analysis = poem.meter_analysis(0).unwrap();

        assert_meter_ready_match(analysis);
        assert_meter_state_ready(&poem, 0);
    }

    #[test]
    fn meter_analysis_only_warms_meter_cache() {
        let mut poem = poem_with_schemes(&["cat"], Some("/"), Some("A"));

        let analysis = poem.meter_analysis(0).unwrap();

        assert_meter_ready_match(analysis);
        assert_meter_state_ready(&poem, 0);
        assert!(
            matches!(&poem.analyses[0].rhyme, AnalysisState::Dirty),
            "rhyme analysis should remain dirty when only meter is requested"
        );
    }

    #[test]
    fn rhyme_analysis_only_warms_rhyme_cache() {
        let mut poem = poem_with_schemes(&["cat"], Some("/"), Some("A"));

        let analysis = poem.rhyme_analysis(0, analyzer()).unwrap();

        assert_rhyme_ready_leader(analysis);
        assert_rhyme_state_ready(&poem, 0);
        assert!(
            matches!(&poem.analyses[0].meter, AnalysisState::Dirty),
            "meter analysis should remain dirty when only rhyme is requested"
        );
    }

    #[test]
    fn rhyme_analysis_computes_leader_unconstrained_and_follower_roles() {
        let mut poem = poem_with_schemes(&["cat", "dog", "hat"], None, Some("A_A"));

        let leader = poem.rhyme_analysis(0, analyzer()).unwrap();
        assert_rhyme_ready_leader(leader);

        let unconstrained = poem.rhyme_analysis(1, analyzer()).unwrap();
        assert_rhyme_ready_unconstrained(unconstrained);

        let follower = poem.rhyme_analysis(2, analyzer()).unwrap();
        assert_rhyme_ready_follower_passes(follower, 0);

        for index in 0..poem.lines.len() {
            assert_rhyme_state_ready(&poem, index);
        }
    }

    #[test]
    fn rhyme_analysis_caches_check_errors_inside_ready_state() {
        let mut poem = poem_with_schemes(&["cat", "xyzzy"], None, Some("AA"));

        let analysis = poem.rhyme_analysis(1, analyzer()).unwrap();

        let RhymeAnalysis::Ready(Err(RhymeCheckError::UnknownWord(error))) = analysis else {
            panic!("expected cached unknown-word rhyme error, got {analysis:?}");
        };
        assert_eq!(error.word, "xyzzy");
        assert!(
            matches!(
                &poem.analyses[1].rhyme,
                AnalysisState::Ready(Err(RhymeCheckError::UnknownWord(error)))
                    if error.word == "xyzzy"
            ),
            "rhyme error was not cached as ready: {:?}",
            poem.analyses[1]
        );
    }

    #[test]
    fn line_analysis_warms_meter_and_rhyme_together() {
        let mut poem = poem_with_schemes(&["cat", "hat"], Some("/"), Some("AA"));

        let analysis = poem.line_analysis(1, analyzer()).unwrap();

        assert_meter_ready_match(analysis.meter);
        assert_rhyme_ready_follower_passes(analysis.rhyme, 0);
        assert_meter_state_ready(&poem, 1);
        assert_rhyme_state_ready(&poem, 1);
    }

    #[test]
    fn line_analysis_returns_not_applicable_for_missing_schemes() {
        let mut poem = poem_with_schemes(&["cat"], None, None);

        let analysis = poem.line_analysis(0, analyzer()).unwrap();

        assert!(matches!(analysis.meter, MeterAnalysis::NotApplicable));
        assert!(matches!(analysis.rhyme, RhymeAnalysis::NotApplicable));
        assert!(matches!(
            &poem.analyses[0].meter,
            AnalysisState::NotApplicable
        ));
        assert!(matches!(
            &poem.analyses[0].rhyme,
            AnalysisState::NotApplicable
        ));
    }

    #[test]
    fn analysis_accessors_return_line_index_errors() {
        let mut poem = poem_with_schemes(&["cat"], Some("/"), Some("A"));

        assert!(matches!(
            poem.meter_analysis(1),
            Err(PoemEditError::LineIndexOutOfBounds {
                index: 1,
                line_count: 1
            })
        ));
        assert!(matches!(
            poem.rhyme_analysis(1, analyzer()),
            Err(PoemEditError::LineIndexOutOfBounds {
                index: 1,
                line_count: 1
            })
        ));
        assert!(matches!(
            poem.line_analysis(1, analyzer()),
            Err(PoemEditError::LineIndexOutOfBounds {
                index: 1,
                line_count: 1
            })
        ));
    }
}
