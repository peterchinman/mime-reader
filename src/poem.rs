use crate::{
    Line, MeterScheme, PronouncingDictionary, RhymeDistanceScorer, RhymeScheme,
    consonant_distance::ConsonantDistanceScorer,
    error::{MeterCheckError, PoemEditError, RhymeCheckError},
    meter::MeterMatchResult,
    rhyme_scheme::{LineRole, ResolvedLineRole, RhymeCheckResult},
    vowel_distance::VowelDistanceScorer,
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
    /// Creates a poem from the given lines and optional schemes.
    ///
    /// Every line starts with a dirty analysis for whichever schemes are
    /// present, so the first `*_analysis` call computes and caches it.
    pub fn new<D: PronouncingDictionary + ?Sized>(
        lines: &[&str],
        meter_scheme: Option<MeterScheme>,
        rhyme_scheme: Option<RhymeScheme>,
        dict: &D,
    ) -> Self {
        let has_meter_scheme = meter_scheme.is_some();
        let has_rhyme_scheme = rhyme_scheme.is_some();

        Poem {
            lines: lines
                .iter()
                .map(|text| PoemLine {
                    line: Line::new(text, dict),
                })
                .collect(),
            analyses: lines
                .iter()
                .map(|_| Self::set_dirty_for(has_meter_scheme, has_rhyme_scheme))
                .collect(),
            meter_scheme,
            rhyme_scheme,
        }
    }

    /// Updates the line at the given index with the given text.
    pub fn update_line<D: PronouncingDictionary + ?Sized>(
        &mut self,
        index: usize,
        text: &str,
        dict: &D,
    ) -> Result<(), PoemEditError> {
        let Some(line) = self.lines.get_mut(index) else {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        };

        line.line = Line::new(text, dict);
        self.invalidate_after_update(index);
        Ok(())
    }
    /// Inserts a new line at the given index with the given text.
    pub fn insert_line<D: PronouncingDictionary + ?Sized>(
        &mut self,
        index: usize,
        text: &str,
        dict: &D,
    ) -> Result<(), PoemEditError> {
        if index > self.lines.len() {
            return Err(PoemEditError::LineIndexOutOfBounds {
                index,
                line_count: self.lines.len(),
            });
        }

        let inserted_line = Line::new(text, dict);
        let inserted_poem_line = PoemLine {
            line: inserted_line,
        };
        self.lines.insert(index, inserted_poem_line);
        self.analyses.insert(
            index,
            Self::set_dirty_for(self.meter_scheme.is_some(), self.rhyme_scheme.is_some()),
        );
        self.invalidate_after_insert_or_delete(index);
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
        self.invalidate_after_insert_or_delete(index);
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

    // TODO: one issue with current rhyme analysis is that, in a rhyme scheme like AAA, if the first line has two differnt pronunciations, we might not use the same pronunciation when evaluating lines 2 and 3. We should use something like Analyzer::score_consistent for evaluating leader + all followers
    pub fn rhyme_analysis<V, C>(
        &mut self,
        index: usize,
        scorer: &RhymeDistanceScorer<V, C>,
    ) -> Result<RhymeAnalysis<'_>, PoemEditError>
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        self.ensure_rhyme_analysis(index, scorer)?;
        Ok(self.cached_rhyme_analysis_after_ensure(index))
    }

    fn ensure_rhyme_analysis<V, C>(
        &mut self,
        index: usize,
        scorer: &RhymeDistanceScorer<V, C>,
    ) -> Result<(), PoemEditError>
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
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
            let result = role
                .and_then(|role| rhyme_scheme.check_line(&self.lines[index].line, role, scorer));
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

    pub fn line_analysis<V, C>(
        &mut self,
        index: usize,
        scorer: &RhymeDistanceScorer<V, C>,
    ) -> Result<PoemLineAnalysis<'_>, PoemEditError>
    where
        V: VowelDistanceScorer,
        C: ConsonantDistanceScorer,
    {
        self.ensure_meter_analysis(index)?;
        self.ensure_rhyme_analysis(index, scorer)?;

        Ok(PoemLineAnalysis {
            meter: self.cached_meter_analysis_after_ensure(index),
            rhyme: self.cached_rhyme_analysis_after_ensure(index),
        })
    }

    fn invalidate_after_update(&mut self, index: usize) {
        // Only the line itself has dirty meter
        if self.meter_scheme.is_some() {
            self.analyses[index].meter = AnalysisState::Dirty;
        }

        let Some(rhyme_scheme) = &self.rhyme_scheme else {
            return;
        };

        let dirty_rhyme_indices = match rhyme_scheme.line_role(index) {
            LineRole::Unconstrained => Vec::new(),
            // On leader update, dependent lines are dirty
            LineRole::Leader => rhyme_scheme.dependent_follower_indices(index, self.lines.len()),
            // On follower update, only line itself is dirty
            LineRole::Follower { .. } => vec![index],
        };

        for index in dirty_rhyme_indices {
            self.analyses[index].rhyme = AnalysisState::Dirty;
        }
    }

    fn invalidate_after_insert_or_delete(&mut self, index: usize) {
        if self.meter_scheme.is_some() {
            for analysis in &mut self.analyses[index..] {
                analysis.meter = AnalysisState::Dirty;
            }
        }

        if self.rhyme_scheme.is_some() {
            for analysis in &mut self.analyses[index..] {
                analysis.rhyme = AnalysisState::Dirty;
            }
        }
    }

    pub fn invalidate_rhyme_analysis(&mut self) {
        if self.rhyme_scheme.is_some() {
            for analysis in &mut self.analyses {
                analysis.rhyme = AnalysisState::Dirty;
            }
        }
    }

    pub fn invalidate_meter_analysis(&mut self) {
        if self.meter_scheme.is_some() {
            for analysis in &mut self.analyses {
                analysis.meter = AnalysisState::Dirty;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmuDictionary;
    use std::sync::OnceLock;

    const DICT_PATH: &str = "data/CMUdict/cmudict-0.7b";

    static DICT: OnceLock<CmuDictionary> = OnceLock::new();
    static SCORER: OnceLock<RhymeDistanceScorer> = OnceLock::new();

    fn dict() -> &'static CmuDictionary {
        DICT.get_or_init(|| CmuDictionary::load(DICT_PATH).expect("failed to load dictionary"))
    }

    fn scorer() -> &'static RhymeDistanceScorer {
        SCORER.get_or_init(RhymeDistanceScorer::new)
    }

    fn poem_with_schemes(
        lines: &[&str],
        meter_scheme: Option<&str>,
        rhyme_scheme: Option<&str>,
    ) -> Poem {
        let meter_scheme = meter_scheme.map(|scheme| scheme.parse().unwrap());
        let rhyme_scheme = rhyme_scheme.map(|scheme| RhymeScheme::new(scheme, 50.0).unwrap());
        Poem::new(lines, meter_scheme, rhyme_scheme, dict())
    }

    fn poem_with_cached_analyses(lines: &[&str]) -> Poem {
        poem_with_cached_analyses_and_rhyme_scheme(lines, "AA")
    }

    fn poem_with_cached_analyses_and_rhyme_scheme(lines: &[&str], rhyme_scheme: &str) -> Poem {
        let mut poem = poem_with_schemes(lines, Some("x"), Some(rhyme_scheme));
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

    fn assert_analyses_aligned_with_lines(poem: &Poem) {
        assert_eq!(poem.analyses.len(), poem.lines.len());
    }

    fn assert_meter_state_dirty(poem: &Poem, index: usize) {
        assert!(
            matches!(&poem.analyses[index].meter, AnalysisState::Dirty),
            "meter analysis at index {index} was not dirty: {:?}",
            poem.analyses[index]
        );
    }

    fn assert_rhyme_state_dirty(poem: &Poem, index: usize) {
        assert!(
            matches!(&poem.analyses[index].rhyme, AnalysisState::Dirty),
            "rhyme analysis at index {index} was not dirty: {:?}",
            poem.analyses[index]
        );
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
        assert_eq!(distance.score.raw_distance, 0);
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

    fn assert_analysis_ready(poem: &Poem, index: usize) {
        assert_meter_state_ready(poem, index);
        assert_rhyme_state_ready(poem, index);
    }

    fn assert_analysis_dirty(poem: &Poem, index: usize) {
        assert_meter_state_dirty(poem, index);
        assert_rhyme_state_dirty(poem, index);
    }

    #[test]
    fn insert_line_keeps_lines_and_analyses_aligned() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog"]);

        poem.insert_line(1, "hat", dict()).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat", "dog"]);
        assert_analyses_aligned_with_lines(&poem);
        assert_analysis_ready(&poem, 0);
        assert_analysis_dirty(&poem, 1);
        assert_analysis_dirty(&poem, 2);
    }

    #[test]
    fn delete_line_keeps_lines_and_analyses_aligned() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog", "hat"]);

        poem.delete_line(1).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat"]);
        assert_analyses_aligned_with_lines(&poem);
        assert_analysis_ready(&poem, 0);
        assert_analysis_dirty(&poem, 1);
    }

    #[test]
    fn update_line_replaces_line_and_invalidates_existing_analyses() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog"]);

        poem.update_line(1, "hat", dict()).unwrap();

        assert_eq!(line_texts(&poem), ["cat", "hat"]);
        assert_analyses_aligned_with_lines(&poem);
        assert_analysis_ready(&poem, 0);
        assert_analysis_dirty(&poem, 1);
    }

    #[test]
    fn invalidate_after_update_on_leader_dirties_meter_and_dependent_followers() {
        let mut poem = poem_with_cached_analyses_and_rhyme_scheme(&["cat", "dog", "hat"], "A_A");

        poem.invalidate_after_update(0);

        assert_meter_state_dirty(&poem, 0);
        assert_meter_state_ready(&poem, 1);
        assert_meter_state_ready(&poem, 2);
        assert_rhyme_state_ready(&poem, 0);
        assert_rhyme_state_ready(&poem, 1);
        assert_rhyme_state_dirty(&poem, 2);
    }

    #[test]
    fn invalidate_after_update_on_unconstrained_line_dirties_only_meter() {
        let mut poem = poem_with_cached_analyses_and_rhyme_scheme(&["cat", "dog", "hat"], "A_A");

        poem.invalidate_after_update(1);

        assert_meter_state_ready(&poem, 0);
        assert_meter_state_dirty(&poem, 1);
        assert_meter_state_ready(&poem, 2);
        for index in 0..poem.lines.len() {
            assert_rhyme_state_ready(&poem, index);
        }
    }

    #[test]
    fn invalidate_after_update_on_follower_dirties_meter_and_own_rhyme() {
        let mut poem = poem_with_cached_analyses_and_rhyme_scheme(&["cat", "dog", "hat"], "A_A");

        poem.invalidate_after_update(2);

        assert_meter_state_ready(&poem, 0);
        assert_meter_state_ready(&poem, 1);
        assert_meter_state_dirty(&poem, 2);
        assert_rhyme_state_ready(&poem, 0);
        assert_rhyme_state_ready(&poem, 1);
        assert_rhyme_state_dirty(&poem, 2);
    }

    #[test]
    fn invalidate_after_insert_or_delete_dirties_suffix_only() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog", "hat"]);

        poem.invalidate_after_insert_or_delete(1);

        assert_analysis_ready(&poem, 0);
        assert_analysis_dirty(&poem, 1);
        assert_analysis_dirty(&poem, 2);
    }

    #[test]
    fn invalidate_rhyme_analysis_dirties_only_rhyme_cache() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog", "hat"]);

        poem.invalidate_rhyme_analysis();

        for index in 0..poem.lines.len() {
            assert_meter_state_ready(&poem, index);
            assert_rhyme_state_dirty(&poem, index);
        }
    }

    #[test]
    fn invalidate_meter_analysis_dirties_only_meter_cache() {
        let mut poem = poem_with_cached_analyses(&["cat", "dog", "hat"]);

        poem.invalidate_meter_analysis();

        for index in 0..poem.lines.len() {
            assert_meter_state_dirty(&poem, index);
            assert_rhyme_state_ready(&poem, index);
        }
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

        let analysis = poem.rhyme_analysis(0, scorer()).unwrap();

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

        let leader = poem.rhyme_analysis(0, scorer()).unwrap();
        assert_rhyme_ready_leader(leader);

        let unconstrained = poem.rhyme_analysis(1, scorer()).unwrap();
        assert_rhyme_ready_unconstrained(unconstrained);

        let follower = poem.rhyme_analysis(2, scorer()).unwrap();
        assert_rhyme_ready_follower_passes(follower, 0);

        for index in 0..poem.lines.len() {
            assert_rhyme_state_ready(&poem, index);
        }
    }

    #[test]
    fn rhyme_analysis_caches_check_errors_inside_ready_state() {
        let mut poem = poem_with_schemes(&["cat", "xyzzy"], None, Some("AA"));

        let analysis = poem.rhyme_analysis(1, scorer()).unwrap();

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

        let analysis = poem.line_analysis(1, scorer()).unwrap();

        assert_meter_ready_match(analysis.meter);
        assert_rhyme_ready_follower_passes(analysis.rhyme, 0);
        assert_meter_state_ready(&poem, 1);
        assert_rhyme_state_ready(&poem, 1);
    }

    #[test]
    fn line_analysis_returns_not_applicable_for_missing_schemes() {
        let mut poem = poem_with_schemes(&["cat"], None, None);

        let analysis = poem.line_analysis(0, scorer()).unwrap();

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
            poem.rhyme_analysis(1, scorer()),
            Err(PoemEditError::LineIndexOutOfBounds {
                index: 1,
                line_count: 1
            })
        ));
        assert!(matches!(
            poem.line_analysis(1, scorer()),
            Err(PoemEditError::LineIndexOutOfBounds {
                index: 1,
                line_count: 1
            })
        ));
    }
}
