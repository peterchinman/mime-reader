use std::collections::{HashMap, HashSet};

use crate::phoneme::{Vowel, VowelPhoneme};
use VowelPhoneme::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VowelHexGraphConfig {
    pub vowel_stress_penalty: u32,
    // How much to weight raw vowel phoneme distance vs the vowel stress penalty.
    pub vowel_distance_coefficient: u32,
    /// Graph edge distance between ER as in BIRD and AH as in BUT.
    pub er_ah_distance: u32,
}

impl Default for VowelHexGraphConfig {
    fn default() -> Self {
        Self {
            vowel_stress_penalty: 1,
            vowel_distance_coefficient: 3,
            er_ah_distance: 3,
        }
    }
}

/// The Vowel Hexagonal Graph is a rendering of vowel space that I invented, by arranging the CMU
/// Pronouncing Dictionary monophthong vowels into a hexagonal grid. Imagine the vowels dripping
/// with honey.
///
/// ```text
///      ..         .          .         ..
///   .::  .=.   .=.  ::.  .::  .=.   .=.  ::.
/// .:.       .=.       .::.       .=.       .:
/// ..   IY    =    IH   .    UH    =    UW   .
/// ..         =         .          =         .
/// ..        .=.        ..        .=.        .
///    =.  .-.   ::   .=    +.   -:   .-.  .=
///      .-         -.        .-         -.
///       :   EH    :    AH    :    AO   -
///       :         :          :         -
///       -.       .-.        .-.        -
///        ..=   +..  .-.  .-.  ..=   =..
///           .=.        ..         =
///            =    AE   .   AA    =
///            =         .          =
///            =:.      .::.      ..=
///              .:: .-.    .=. ::.
/// ```
///
///
/// DIPHTHONGS:
///
///   "AW", "AY", "EY", "OW", "OY" — 5 diphthongs
///   bout, bite, bait, boat, boy
///
/// diphthongs mapping:
///   AW  aʊ  ->  AU* UH
///   AY  aɪ  ->  AU* IH
///   EY  eɪ  ->  EE* IH
///   OW  oʊ  ->  OH* UH
///   OY  ɔɪ  ->  OH* IH
///
/// Where AU is IPA a, between AE and AA
///       EE is IPA e, between EH and IY
///       OH is IPA o, between AO and UW
///
/// NOTE: some of the diphthongs start in in-between locations on the hex-graph, but end in either UH or IH.
#[derive(Clone)]
pub struct VowelHexGraph {
    distances: HashMap<(VowelPhoneme, VowelPhoneme), u32>,
    config: VowelHexGraphConfig,
}

pub trait VowelDistanceScorer {
    fn distance(&self, v1: &Vowel, v2: &Vowel) -> u32;
}

type VowelAdjacency = HashMap<VowelPhoneme, Vec<(VowelPhoneme, u32)>>;

fn add_edge(adj: &mut VowelAdjacency, a: VowelPhoneme, b: VowelPhoneme, distance: u32) {
    adj.entry(a).or_default().push((b, distance));
    adj.entry(b).or_default().push((a, distance));
}

fn build_adjacency(config: VowelHexGraphConfig) -> VowelAdjacency {
    let mut adj = HashMap::new();

    // Monophthong edges
    add_edge(&mut adj, AE, AA, 1);
    add_edge(&mut adj, AE, AH, 1);
    add_edge(&mut adj, AE, EH, 1);
    add_edge(&mut adj, AA, AO, 1);
    add_edge(&mut adj, AA, AH, 1);
    add_edge(&mut adj, EH, AH, 1);
    add_edge(&mut adj, EH, IH, 1);
    add_edge(&mut adj, EH, IY, 1);
    add_edge(&mut adj, AH, AO, 1);
    add_edge(&mut adj, AH, UH, 1);
    add_edge(&mut adj, AH, IH, 1);
    add_edge(&mut adj, AO, UW, 1);
    add_edge(&mut adj, AO, UH, 1);
    add_edge(&mut adj, IY, IH, 1);
    add_edge(&mut adj, IH, UH, 1);
    add_edge(&mut adj, UH, UW, 1);

    // OPINIONATED /ER/ EDGE:
    // ER as in BIRD has a configurable edge to AH as in BUT.
    add_edge(&mut adj, ER, AH, config.er_ah_distance);

    // DIPHTHONG ADJACENCIES
    // "AW", "AY", "EY", "OW", "OY" — 5 diphthongs
    //  bout, bite, bait, boat, boy
    //
    // I am making some extremely opinionated decisions here:

    // 1. AW as in BOUT is adjacent to:
    //    UH as in BUSH
    //    OW as in BOAT
    //    AH as in BUT
    //    AA : 2 (satisfied by AH adjacency)
    //    AE : 2 (satisfied by AH adjacency)
    add_edge(&mut adj, AW, UH, 1);
    add_edge(&mut adj, AW, OW, 1);
    add_edge(&mut adj, AW, AH, 1);

    // 2. AY as in BITE is adjacent to:
    //    IH as in BIT
    //    EY as in BAIT
    //    AH as in BUT
    //    AA : 2 (satisfied by AH adjacency)
    //    AE : 2 (satisfied by AH adjacency)
    add_edge(&mut adj, AY, IH, 1);
    add_edge(&mut adj, AY, EY, 1);
    add_edge(&mut adj, AY, AH, 1);

    // 3. EY as in BAIT is adjacent to:
    //    AY as in BITE *redundant
    //    IH as in BIT
    //    EH as in BET
    //    IY as in BEAT
    add_edge(&mut adj, EY, IH, 1);
    add_edge(&mut adj, EY, EH, 1);
    add_edge(&mut adj, EY, IY, 1);

    // 4. OW as in BOAT is adjacent to:
    //    OY as in BOY
    //    AW as in BOUT *redundant
    //    UH as in BUSH
    //    UW as in BOOT
    //    AO as in BOMB
    add_edge(&mut adj, OW, OY, 1);
    add_edge(&mut adj, OW, UH, 1);
    add_edge(&mut adj, OW, UW, 1);
    add_edge(&mut adj, OW, AO, 1);

    // 5. OY as in BOY is adjacent to:
    //    IH as in BIT
    //    OW as in BOAT *redundant
    add_edge(&mut adj, OY, IH, 1);

    adj
}

fn shortest_path_distance(adj: &VowelAdjacency, start: VowelPhoneme, end: VowelPhoneme) -> u32 {
    if start == end {
        return 0;
    }

    let mut distances = HashMap::from([(start, 0)]);
    let mut visited: HashSet<VowelPhoneme> = HashSet::new();

    loop {
        let Some((current, current_distance)) = distances
            .iter()
            .filter(|(phoneme, _)| !visited.contains(phoneme))
            .min_by_key(|(_, distance)| *distance)
            .map(|(phoneme, distance)| (*phoneme, *distance))
        else {
            panic!(
                "no path between {:?} and {:?} — graph is disconnected",
                start, end
            );
        };

        if current == end {
            return current_distance;
        }
        visited.insert(current);

        let Some(neighbors) = adj.get(&current) else {
            continue;
        };

        for &(neighbor, edge_distance) in neighbors {
            if visited.contains(&neighbor) {
                continue;
            }

            let candidate = current_distance
                .checked_add(edge_distance)
                .expect("vowel path distance overflow");
            distances
                .entry(neighbor)
                .and_modify(|distance| *distance = (*distance).min(candidate))
                .or_insert(candidate);
        }
    }
}

impl VowelHexGraph {
    fn build(config: VowelHexGraphConfig) -> Self {
        let adj = build_adjacency(config);
        let mut distances = HashMap::new();
        for a in VowelPhoneme::ALL {
            for b in VowelPhoneme::ALL {
                distances.insert((a, b), shortest_path_distance(&adj, a, b));
            }
        }
        VowelHexGraph { distances, config }
    }

    pub fn new() -> Self {
        Self::with_config(VowelHexGraphConfig::default())
    }

    pub fn with_config(config: VowelHexGraphConfig) -> Self {
        Self::build(config)
    }

    pub fn config(&self) -> VowelHexGraphConfig {
        self.config
    }

    fn phoneme_distance(&self, v1: VowelPhoneme, v2: VowelPhoneme) -> u32 {
        self.distances[&(v1, v2)]
    }

    pub fn distance(&self, v1: &Vowel, v2: &Vowel) -> u32 {
        if v1 == v2 {
            return 0;
        }

        let stress_penalty = if v1.stress != v2.stress {
            self.config.vowel_stress_penalty
        } else {
            0
        };

        if v1.phoneme == v2.phoneme {
            stress_penalty
        } else {
            self.phoneme_distance(v1.phoneme, v2.phoneme) * self.config.vowel_distance_coefficient
                + stress_penalty
        }
    }
}

impl VowelDistanceScorer for VowelHexGraph {
    fn distance(&self, v1: &Vowel, v2: &Vowel) -> u32 {
        VowelHexGraph::distance(self, v1, v2)
    }
}

impl Default for VowelHexGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phoneme::Vowel;
    use std::sync::OnceLock;

    static GRAPH: OnceLock<VowelHexGraph> = OnceLock::new();

    fn graph() -> &'static VowelHexGraph {
        GRAPH.get_or_init(VowelHexGraph::new)
    }

    fn config() -> VowelHexGraphConfig {
        VowelHexGraphConfig::default()
    }

    fn v(s: &str) -> Vowel {
        s.parse().unwrap()
    }

    // --- VowelHexGraph::distance ---

    #[test]
    fn vowel_distance_same_vowel_same_stress() {
        assert_eq!(graph().distance(&v("AO1"), &v("AO1")), 0);
    }

    #[test]
    fn vowel_distance_same_vowel_different_stress() {
        assert_eq!(
            graph().distance(&v("AO1"), &v("AO0")),
            config().vowel_stress_penalty
        );
    }

    #[test]
    fn vowel_distance_adjacent_same_stress() {
        // AO and OW are adjacent on the hex-graph (dist=1)
        assert_eq!(
            graph().distance(&v("AO1"), &v("OW1")),
            config().vowel_distance_coefficient
        );
    }

    #[test]
    fn vowel_distance_adjacent_different_stress() {
        assert_eq!(
            graph().distance(&v("AO1"), &v("OW0")),
            config().vowel_distance_coefficient + config().vowel_stress_penalty
        );
    }

    #[test]
    fn vowel_distance_two_steps() {
        // UW and IH are distance 2 on the hex-graph
        assert_eq!(
            graph().distance(&v("UW1"), &v("IH1")),
            2 * config().vowel_distance_coefficient
        );
    }

    #[test]
    fn vowel_distance_weighted_edge() {
        assert_eq!(
            graph().distance(&v("ER1"), &v("AH1")),
            config().er_ah_distance * config().vowel_distance_coefficient
        );
    }

    #[test]
    fn vowel_distance_is_symmetric() {
        assert_eq!(
            graph().distance(&v("AO1"), &v("IY2")),
            graph().distance(&v("IY2"), &v("AO1"))
        );
    }

    #[test]
    fn graph_distance_includes_stress_penalty() {
        assert_eq!(
            graph().distance(&v("AO1"), &v("OW0")),
            config().vowel_distance_coefficient + config().vowel_stress_penalty
        );
    }

    #[test]
    fn custom_config_controls_vowel_distance_penalties() {
        let graph = VowelHexGraph::with_config(VowelHexGraphConfig {
            vowel_stress_penalty: 5,
            vowel_distance_coefficient: 7,
            er_ah_distance: 3,
        });

        assert_eq!(graph.distance(&v("AO1"), &v("AO0")), 5);
        assert_eq!(graph.distance(&v("AO1"), &v("OW1")), 7);
        assert_eq!(graph.distance(&v("AO1"), &v("OW0")), 12);
        assert_eq!(graph.distance(&v("ER1"), &v("AH1")), 21);
    }

    #[test]
    fn same_vowel_phoneme_distance_is_zero() {
        assert_eq!(graph().phoneme_distance(AE, AE), 0);
        assert_eq!(graph().phoneme_distance(OY, OY), 0);
    }

    #[test]
    fn adjacent_vowel_phoneme_distance_is_one() {
        assert_eq!(graph().phoneme_distance(AE, AH), 1);
    }

    #[test]
    fn phoneme_distance_two_steps() {
        assert_eq!(graph().phoneme_distance(UW, IH), 2);
    }

    #[test]
    fn er_ah_phoneme_distance_uses_configured_edge_weight() {
        assert_eq!(graph().phoneme_distance(ER, AH), config().er_ah_distance);
    }

    #[test]
    fn weighted_shortest_path_can_beat_direct_edge() {
        let mut adj = VowelAdjacency::new();
        add_edge(&mut adj, AE, AA, 5);
        add_edge(&mut adj, AE, AH, 1);
        add_edge(&mut adj, AH, AA, 1);

        assert_eq!(shortest_path_distance(&adj, AE, AA), 2);
    }

    #[test]
    fn phoneme_distance_three() {
        assert_eq!(graph().phoneme_distance(AO, IY), 3);
    }

    #[test]
    fn phoneme_distances_are_symmetric() {
        assert_eq!(
            graph().phoneme_distance(AH, AE),
            graph().phoneme_distance(AE, AH)
        );
        assert_eq!(
            graph().phoneme_distance(IY, AO),
            graph().phoneme_distance(AO, IY)
        );
    }

    // --- diphthong adjacencies ---

    #[test]
    fn aw_adjacencies() {
        assert_eq!(graph().phoneme_distance(AW, UH), 1);
        assert_eq!(graph().phoneme_distance(AW, OW), 1);
        assert_eq!(graph().phoneme_distance(AW, AH), 1);
    }

    #[test]
    fn ay_adjacencies() {
        assert_eq!(graph().phoneme_distance(AY, IH), 1);
        assert_eq!(graph().phoneme_distance(AY, EY), 1);
        assert_eq!(graph().phoneme_distance(AY, AH), 1);
    }

    #[test]
    fn ey_adjacencies() {
        assert_eq!(graph().phoneme_distance(EY, IH), 1);
        assert_eq!(graph().phoneme_distance(EY, EH), 1);
        assert_eq!(graph().phoneme_distance(EY, IY), 1);
    }

    #[test]
    fn ow_adjacencies() {
        assert_eq!(graph().phoneme_distance(OW, OY), 1);
        assert_eq!(graph().phoneme_distance(OW, UH), 1);
        assert_eq!(graph().phoneme_distance(OW, UW), 1);
        assert_eq!(graph().phoneme_distance(OW, AO), 1);
    }

    #[test]
    fn oy_adjacencies() {
        assert_eq!(graph().phoneme_distance(OY, IH), 1);
    }
}
