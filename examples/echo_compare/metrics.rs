//! Separation metrics for scored pairs.

use crate::corpus::Label;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub min: f64,
    pub max: f64,
}

pub fn summary(xs: &[f32]) -> Option<Summary> {
    if xs.is_empty() {
        return None;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let var = xs
        .iter()
        .map(|&x| (f64::from(x) - mean).powi(2))
        .sum::<f64>()
        / n;
    Some(Summary {
        n: xs.len(),
        mean,
        sd: var.sqrt(),
        min: xs.iter().copied().fold(f32::INFINITY, f32::min).into(),
        max: xs.iter().copied().fold(f32::NEG_INFINITY, f32::max).into(),
    })
}

/// ROC AUC: the probability a random echo outscores a random non-echo
/// (ties count half), by mid-ranks.
pub fn auc(positives: &[f32], negatives: &[f32]) -> Option<f64> {
    if positives.is_empty() || negatives.is_empty() {
        return None;
    }
    let mut all: Vec<(f32, bool)> = positives
        .iter()
        .map(|&s| (s, true))
        .chain(negatives.iter().map(|&s| (s, false)))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut rank_sum = 0.0;
    let mut start = 0;
    while start < all.len() {
        let mut end = start;
        while end < all.len() && all[end].0 == all[start].0 {
            end += 1;
        }
        // Ranks start..end (1-based start+1..=end) share their mean.
        let mid_rank = (start + 1 + end) as f64 / 2.0;
        rank_sum += mid_rank * all[start..end].iter().filter(|(_, p)| *p).count() as f64;
        start = end;
    }
    let (p, n) = (positives.len() as f64, negatives.len() as f64);
    Some((rank_sum - p * (p + 1.0) / 2.0) / (p * n))
}

/// A decision rule `similarity ≥ threshold ⇒ echo`, and how it does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Operating {
    pub threshold: f32,
    pub tpr: f64,
    pub tnr: f64,
    pub balanced_accuracy: f64,
    pub f1: f64,
}

pub fn at_threshold(positives: &[f32], negatives: &[f32], threshold: f32) -> Operating {
    let tp = positives.iter().filter(|&&s| s >= threshold).count() as f64;
    let tn = negatives.iter().filter(|&&s| s < threshold).count() as f64;
    let fp = negatives.len() as f64 - tn;
    let tpr = if positives.is_empty() {
        0.0
    } else {
        tp / positives.len() as f64
    };
    let tnr = if negatives.is_empty() {
        0.0
    } else {
        tn / negatives.len() as f64
    };
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let f1 = if precision + tpr > 0.0 {
        2.0 * precision * tpr / (precision + tpr)
    } else {
        0.0
    };
    Operating {
        threshold,
        tpr,
        tnr,
        balanced_accuracy: (tpr + tnr) / 2.0,
        f1,
    }
}

/// The threshold maximizing balanced accuracy, placed midway between the
/// two scores it separates. Ties keep the widest gap.
pub fn best_threshold(positives: &[f32], negatives: &[f32]) -> Option<Operating> {
    if positives.is_empty() || negatives.is_empty() {
        return None;
    }
    let mut scores: Vec<f32> = positives.iter().chain(negatives).copied().collect();
    scores.sort_by(f32::total_cmp);
    scores.dedup();
    let mut candidates = vec![scores[0] - 0.01];
    candidates.extend(scores.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    candidates.push(scores[scores.len() - 1] + 0.01);
    let gap = |t: f32| {
        let above = scores.iter().copied().find(|&s| s >= t).unwrap_or(t);
        let below = scores.iter().copied().rev().find(|&s| s < t).unwrap_or(t);
        above - below
    };
    candidates
        .into_iter()
        .map(|t| at_threshold(positives, negatives, t))
        .max_by(|a, b| {
            a.balanced_accuracy
                .total_cmp(&b.balanced_accuracy)
                .then(gap(a.threshold).total_cmp(&gap(b.threshold)))
        })
}

/// `min(echoes) − max(non-echoes)`: positive when perfectly separated.
pub fn margin(positives: &[f32], negatives: &[f32]) -> Option<f32> {
    let min = positives.iter().copied().reduce(f32::min)?;
    let max = negatives.iter().copied().reduce(f32::max)?;
    Some(min - max)
}

/// How well one poem's echoes rank among its own line pairs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoemRank {
    /// The number of echo pairs.
    pub k: usize,
    /// The share of echo pairs among the poem's top-k pairs.
    pub precision_at_k: f64,
    /// Mean reciprocal rank of each echo pair among the pairs sharing its
    /// later line (other echoes filtered out).
    pub mrr: f64,
    /// Every echo outscores every non-echo.
    pub separated: bool,
}

/// `pairs`: (i, j, label, similarity) for one poem.
pub fn poem_rank(pairs: &[(usize, usize, Label, f32)]) -> Option<PoemRank> {
    let positives: Vec<&(usize, usize, Label, f32)> =
        pairs.iter().filter(|p| p.2 == Label::Echo).collect();
    if positives.is_empty() {
        return None;
    }
    let k = positives.len();
    let mut sorted: Vec<&(usize, usize, Label, f32)> = pairs.iter().collect();
    sorted.sort_by(|a, b| b.3.total_cmp(&a.3));
    let precision_at_k = sorted[..k.min(sorted.len())]
        .iter()
        .filter(|p| p.2 == Label::Echo)
        .count() as f64
        / k as f64;
    let mrr = positives
        .iter()
        .map(|&&(_, j, _, score)| {
            let better = pairs
                .iter()
                .filter(|p| (p.0 == j || p.1 == j) && p.2 == Label::Not && p.3 > score)
                .count();
            1.0 / (better + 1) as f64
        })
        .sum::<f64>()
        / k as f64;
    let min_positive = positives.iter().map(|p| p.3).fold(f32::INFINITY, f32::min);
    let max_negative = pairs
        .iter()
        .filter(|p| p.2 == Label::Not)
        .map(|p| p.3)
        .fold(f32::NEG_INFINITY, f32::max);
    Some(PoemRank {
        k,
        precision_at_k,
        mrr,
        separated: min_positive > max_negative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auc_counts_ties_half() {
        assert_eq!(auc(&[0.9, 0.8], &[0.1, 0.2]), Some(1.0));
        assert_eq!(auc(&[0.1], &[0.9]), Some(0.0));
        assert_eq!(auc(&[0.5], &[0.5]), Some(0.5));
        assert_eq!(auc(&[0.9, 0.4], &[0.5, 0.1]), Some(0.75));
        assert_eq!(auc(&[], &[0.5]), None);
    }

    #[test]
    fn best_threshold_separates_when_possible() {
        let best = best_threshold(&[0.9, 0.8, 0.7], &[0.1, 0.3]).unwrap();
        assert_eq!(best.balanced_accuracy, 1.0);
        assert!(
            (best.threshold - 0.5).abs() < 1e-6,
            "midway: {}",
            best.threshold
        );
        assert_eq!(best.f1, 1.0);
        let overlapping = best_threshold(&[0.9, 0.2], &[0.3, 0.1]).unwrap();
        assert_eq!(overlapping.balanced_accuracy, 0.75);
    }

    #[test]
    fn margin_and_summary() {
        assert!((margin(&[0.9, 0.7], &[0.2, 0.4]).unwrap() - 0.3).abs() < 1e-6);
        assert!(margin(&[0.3], &[0.4]).unwrap() < 0.0);
        let s = summary(&[1.0, 3.0]).unwrap();
        assert_eq!((s.n, s.mean, s.sd, s.min, s.max), (2, 2.0, 1.0, 1.0, 3.0));
    }

    #[test]
    fn poem_rank_filters_other_echoes() {
        use Label::{Echo, Not};
        // Lines 0, 1, 2 all echo; line 3 is free.
        let pairs = [
            (0, 1, Echo, 0.9),
            (0, 2, Echo, 0.8),
            (1, 2, Echo, 0.95),
            (0, 3, Not, 0.85),
            (1, 3, Not, 0.2),
            (2, 3, Not, 0.1),
        ];
        let rank = poem_rank(&pairs).unwrap();
        assert_eq!(rank.k, 3);
        assert!((rank.precision_at_k - 2.0 / 3.0).abs() < 1e-9);
        assert!(!rank.separated);
        assert_eq!(
            rank.mrr, 1.0,
            "no non-echo shares a later line and outscores"
        );
    }
}
