//! Mishearing, built on mime-reader: fuzzy phonetic search over a
//! pronunciation lexicon ([`search`]), phoneme deformations such as accents
//! ([`deform`]), and the two combined ([`engine`]).

pub mod accents;
pub mod bigrams;
pub mod clock;
pub mod cost;
pub mod deform;
pub mod engine;
pub mod knobs;
pub mod lexicon;
pub mod phone;
pub mod search;
pub mod spec;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use bigrams::BigramTable;
pub use cost::{CostConfig, CostModel};
pub use deform::{Action, Class, Context, Deformation, Rule, VowelChart, VowelMove};
pub use engine::{Engine, Heard, MishearConfig, Mishearing, Reading, Variety};
pub use lexicon::{Lexicon, WordId};
pub use phone::Ph;
pub use search::{Match, SearchConfig, SearchStats, best_matches, search};
