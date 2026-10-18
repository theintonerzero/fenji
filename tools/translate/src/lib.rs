//! The retranslation's tools (retranslation spec §6.5). They read the pinned copy of the game,
//! and write only under `re/retranslation/` (ground rule 3): nothing derived from the copy is
//! committed, and only the new English, keyed by fingerprints, leaves for `fenji-content`.
pub mod area;
pub mod check;
pub mod extract;
pub mod lines;
pub mod markup;
