use std::collections::{HashMap, HashSet, VecDeque};

use crate::phoneme::{Vowel, VowelPhoneme};
use VowelPhoneme::*;

pub const VOWEL_STRESS_PENALTY: u32 = 1;
pub const VOWEL_DISTANCE_COEFFICIENT: u32 = 2;

const VOWELS: [VowelPhoneme; 15] = [AE, AA, EH, AH, AO, IY, IH, UH, UW, ER, AW, AY, EY, OW, OY];

// The Vowel Hexagonal Graph is a rendering of vowel space that I invented, by arranging the CMU
// Pronouncing Dictionary monophthong vowels into a hexagonal grid. Imagine the vowels dripping
// with honey.
//
//      ..         .          .         ..
//   .::  .=.   .=.  ::.  .::  .=.   .=.  ::.
// .:.       .=.       .::.       .=.       .:
// ..   IY    =    IH   .    UH    =    UW   .
// ..         =         .          =         .
// ..        .=.        ..        .=.        .
//    =.  .-.   ::   .=    +.   -:   .-.  .=
//      .-         -.        .-         -.
//       :   EH    :    AH    :    AO   -
//       :         :          :         -
//       -.       .-.        .-.        -
//        ..=   +..  .-.  .-.  ..=   =..
//           .=.        ..         =
//            =    AE   .   AA    =
//            =         .          =
//            =:.      .::.      ..=
//              .:: .-.    .=. ::.
//
//
// DIPHTHONGS:
//
//   "AW", "AY", "EY", "OW", "OY" — 5 diphthongs
//   bout, bite, bait, boat, boy
//
// diphthongs mapping:
//   AW  aʊ  ->  AU* UH
//   AY  aɪ  ->  AU* IH
//   EY  eɪ  ->  EE* IH
//   OW  oʊ  ->  OH* UH
//   OY  ɔɪ  ->  OH* IH
//
// Where AU is IPA a, between AE and AA
//       EE is IPA e, between EH and IY
//       OH is IPA o, between AO and UW
//
// NOTE: some of the diphthongs start in in-between locations on the hex-graph, but end in either
// UH or IH.

pub struct VowelHexGraph {
    distances: HashMap<(VowelPhoneme, VowelPhoneme), u32>,
}

fn build_adjacency() -> HashMap<VowelPhoneme, Vec<VowelPhoneme>> {
    let mut adj: HashMap<VowelPhoneme, Vec<VowelPhoneme>> = HashMap::new();

    let mut add_edge = |a: VowelPhoneme, b: VowelPhoneme| {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    };

    // Monophthong edges
    add_edge(AE, AA);
    add_edge(AE, AH);
    add_edge(AE, EH);
    add_edge(AA, AO);
    add_edge(AA, AH);
    add_edge(EH, AH);
    add_edge(EH, IH);
    add_edge(EH, IY);
    add_edge(AH, AO);
    add_edge(AH, UH);
    add_edge(AH, IH);
    add_edge(AO, UW);
    add_edge(AO, UH);
    add_edge(IY, IH);
    add_edge(IH, UH);
    add_edge(UH, UW);

    // OPINIONATED /ER/ ADJACENCY:
    // ER as in BIRD is adjacent to:
    //   AH as in BUT
    add_edge(ER, AH);

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
    add_edge(AW, UH);
    add_edge(AW, OW);
    add_edge(AW, AH);

    // 2. AY as in BITE is adjacent to:
    //    IH as in BIT
    //    EY as in BAIT
    //    AH as in BUT
    //    AA : 2 (satisfied by AH adjacency)
    //    AE : 2 (satisfied by AH adjacency)
    add_edge(AY, IH);
    add_edge(AY, EY);
    add_edge(AY, AH);

    // 3. EY as in BAIT is adjacent to:
    //    AY as in BITE *redundant
    //    IH as in BIT
    //    EH as in BET
    //    IY as in BEAT
    add_edge(EY, IH);
    add_edge(EY, EH);
    add_edge(EY, IY);

    // 4. OW as in BOAT is adjacent to:
    //    OY as in BOY
    //    AW as in BOUT *redundant
    //    UH as in BUSH
    //    UW as in BOOT
    //    AO as in BOMB
    add_edge(OW, OY);
    add_edge(OW, UH);
    add_edge(OW, UW);
    add_edge(OW, AO);

    // 5. OY as in BOY is adjacent to:
    //    IH as in BIT
    //    OW as in BOAT *redundant
    add_edge(OY, IH);

    adj
}

fn bfs_distance(
    adj: &HashMap<VowelPhoneme, Vec<VowelPhoneme>>,
    start: VowelPhoneme,
    end: VowelPhoneme,
) -> u32 {
    // If both vowels are the same, the distance is 0
    if start == end {
        return 0;
    }

    // Set up a queue for BFS and a set to track visited nodes
    let mut queue: VecDeque<(VowelPhoneme, u32)> = VecDeque::new();
    let mut visited: HashSet<VowelPhoneme> = HashSet::new();

    // Initialize BFS
    queue.push_back((start, 0));
    visited.insert(start);

    // Perform BFS
    while let Some((current, dist)) = queue.pop_front() {
        // Check all connected vowels
        let Some(neighbors) = adj.get(&current) else {
            continue;
        };
        for &neighbor in neighbors {
            if neighbor == end {
                return dist + 1; // Found the target vowel
            }
            if visited.insert(neighbor) {
                queue.push_back((neighbor, dist + 1));
            }
        }
    }

    panic!(
        "no path between {:?} and {:?} — graph is disconnected",
        start, end
    );
}

impl VowelHexGraph {
    fn build() -> Self {
        let adj = build_adjacency();
        let mut distances = HashMap::new();
        for a in VOWELS {
            for b in VOWELS {
                distances.insert((a, b), bfs_distance(&adj, a, b));
            }
        }
        VowelHexGraph { distances }
    }

    pub fn new() -> Self {
        Self::build()
    }

    pub fn get_distance(&self, v1: VowelPhoneme, v2: VowelPhoneme) -> u32 {
        self.distances[&(v1, v2)]
    }
}

impl Vowel {
    pub fn distance(&self, other: &Vowel, graph: &VowelHexGraph) -> u32 {
        if self == other {
            return 0;
        }
        let stress_penalty = if self.stress != other.stress {
            VOWEL_STRESS_PENALTY
        } else {
            0
        };
        if self.phoneme == other.phoneme {
            stress_penalty
        } else {
            graph.get_distance(self.phoneme, other.phoneme) * VOWEL_DISTANCE_COEFFICIENT
                + stress_penalty
        }
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

    fn v(s: &str) -> Vowel {
        s.parse().unwrap()
    }

    // --- Vowel::distance ---

    #[test]
    fn vowel_distance_same_vowel_same_stress() {
        assert_eq!(v("AO1").distance(&v("AO1"), graph()), 0);
    }

    #[test]
    fn vowel_distance_same_vowel_different_stress() {
        assert_eq!(v("AO1").distance(&v("AO0"), graph()), VOWEL_STRESS_PENALTY);
    }

    #[test]
    fn vowel_distance_adjacent_same_stress() {
        // AO and OW are adjacent on the hex-graph (dist=1)
        assert_eq!(
            v("AO1").distance(&v("OW1"), graph()),
            VOWEL_DISTANCE_COEFFICIENT
        );
    }

    #[test]
    fn vowel_distance_adjacent_different_stress() {
        assert_eq!(
            v("AO1").distance(&v("OW0"), graph()),
            VOWEL_DISTANCE_COEFFICIENT + VOWEL_STRESS_PENALTY
        );
    }

    #[test]
    fn vowel_distance_two_steps() {
        // UW and IH are distance 2 on the hex-graph
        assert_eq!(
            v("UW1").distance(&v("IH1"), graph()),
            2 * VOWEL_DISTANCE_COEFFICIENT
        );
    }

    #[test]
    fn vowel_distance_is_symmetric() {
        assert_eq!(
            v("AO1").distance(&v("IY2"), graph()),
            v("IY2").distance(&v("AO1"), graph())
        );
    }

    #[test]
    fn same_vowel_distance_is_zero() {
        assert_eq!(graph().get_distance(AE, AE), 0);
        assert_eq!(graph().get_distance(OY, OY), 0);
    }

    #[test]
    fn adjacent_vowels_distance_is_one() {
        assert_eq!(graph().get_distance(AE, AH), 1);
    }

    #[test]
    fn distance_two() {
        assert_eq!(graph().get_distance(UW, IH), 2);
    }

    #[test]
    fn distance_three() {
        assert_eq!(graph().get_distance(AO, IY), 3);
    }

    #[test]
    fn distances_are_symmetric() {
        assert_eq!(graph().get_distance(AH, AE), graph().get_distance(AE, AH));
        assert_eq!(graph().get_distance(IY, AO), graph().get_distance(AO, IY));
    }

    // --- diphthong adjacencies ---

    #[test]
    fn aw_adjacencies() {
        assert_eq!(graph().get_distance(AW, UH), 1);
        assert_eq!(graph().get_distance(AW, OW), 1);
        assert_eq!(graph().get_distance(AW, AH), 1);
    }

    #[test]
    fn ay_adjacencies() {
        assert_eq!(graph().get_distance(AY, IH), 1);
        assert_eq!(graph().get_distance(AY, EY), 1);
        assert_eq!(graph().get_distance(AY, AH), 1);
    }

    #[test]
    fn ey_adjacencies() {
        assert_eq!(graph().get_distance(EY, IH), 1);
        assert_eq!(graph().get_distance(EY, EH), 1);
        assert_eq!(graph().get_distance(EY, IY), 1);
    }

    #[test]
    fn ow_adjacencies() {
        assert_eq!(graph().get_distance(OW, OY), 1);
        assert_eq!(graph().get_distance(OW, UH), 1);
        assert_eq!(graph().get_distance(OW, UW), 1);
        assert_eq!(graph().get_distance(OW, AO), 1);
    }

    #[test]
    fn oy_adjacencies() {
        assert_eq!(graph().get_distance(OY, IH), 1);
    }
}
