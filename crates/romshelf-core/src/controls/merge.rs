//! Combining an emulator's default with a game's differences, and the reverse.

use super::model::Layout;
use crate::model::Binding;

/// The layout a game plays with: the default, with the game's differences on top. An input the
/// game cleared stays cleared; one it doesn't mention follows the default.
pub fn merge(default: &Layout, game: &Layout) -> Layout {
    let mut out = default.clone();
    for (input, binding) in game {
        out.insert(input.clone(), binding.clone());
    }
    out
}

/// What a full layout changes from the default: the entries to keep as a game's differences.
/// An input missing from `full` counts as cleared.
pub fn difference(full: &Layout, default: &Layout) -> Layout {
    let inputs = full.keys().chain(default.keys());
    let mut out = Layout::new();
    for input in inputs {
        let mine = full.get(input).unwrap_or(&Binding::Cleared);
        let theirs = default.get(input).unwrap_or(&Binding::Cleared);
        if mine != theirs {
            out.insert(input.clone(), mine.clone());
        }
    }
    out
}

/// "Make this the default": the game's merged layout becomes the default, and the game keeps
/// no differences. Other games keep theirs, so they change only where they had none.
pub fn promote(default: &Layout, game: &Layout) -> (Layout, Layout) {
    (merge(default, game), Layout::new())
}
