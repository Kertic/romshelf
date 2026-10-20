//! Stage 7: the control model and merging.

use romshelf_core::controls::model::{from_pairs, validate};
use romshelf_core::controls::{difference, merge, promote, Layout, Scope, INPUTS};
use romshelf_core::model::{Binding, System};

fn bound(t: &str) -> Binding {
    Binding::Bound(t.into())
}

/// A GBA default in RetroArch, as the reference screen shows it.
fn gba_default() -> Layout {
    from_pairs(&[
        ("dpad", Some("dpad")),
        ("left_stick", Some("dpad")),
        ("right_stick", None),
        ("a", Some("a")),
        ("b", Some("b")),
        ("x", None),
        ("y", None),
        ("l1", Some("l")),
        ("r1", Some("r")),
        ("l2", None),
        ("r2", None),
        ("l3", None),
        ("r3", None),
        ("menu", Some("start")),
        ("view", Some("select")),
        ("l4", None),
        ("r4", None),
        ("l5", Some("save_state")),
        ("r5", Some("load_state")),
    ])
    .unwrap()
}

fn gba() -> Scope {
    Scope::new("retroarch", System::Gba)
}

#[test]
fn the_default_is_complete_and_valid() {
    let d = gba_default();
    assert_eq!(d.len(), INPUTS.len());
    validate(&gba(), &d).unwrap();
}

#[test]
fn merge_override_wins_cleared_stays_cleared_rest_follows() {
    let d = gba_default();
    let game = from_pairs(&[
        ("a", Some("b")),
        ("b", Some("a")),
        ("l5", None),
        ("l4", Some("fast_forward")),
    ])
    .unwrap();
    let m = merge(&d, &game);
    assert_eq!(m["a"], bound("b"));
    assert_eq!(m["b"], bound("a"));
    assert_eq!(
        m["l5"],
        Binding::Cleared,
        "explicitly unbound stays unbound"
    );
    assert_eq!(m["l4"], bound("fast_forward"));
    assert_eq!(m["menu"], bound("start"), "untouched follows the default");
    assert_eq!(m.len(), d.len());
}

#[test]
fn changing_the_default_moves_only_what_the_game_did_not_override() {
    let mut d = gba_default();
    let game = from_pairs(&[("a", Some("b"))]).unwrap();
    d.insert("a".into(), bound("select"));
    d.insert("menu".into(), bound("select"));
    let m = merge(&d, &game);
    assert_eq!(m["a"], bound("b"), "overridden: unchanged");
    assert_eq!(m["menu"], bound("select"), "not overridden: follows");
}

/// xorshift, so the "random" layouts are the same on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

fn random_layout(rng: &mut Rng, partial: bool) -> Layout {
    let targets = [
        "a",
        "b",
        "l",
        "r",
        "start",
        "select",
        "fast_forward",
        "rewind",
    ];
    let mut out = Layout::new();
    for input in INPUTS {
        if partial && rng.next() % 2 == 0 {
            continue;
        }
        let b = match rng.next() % 4 {
            0 => Binding::Cleared,
            _ => bound(rng.pick(&targets)),
        };
        out.insert(input.id.into(), b);
    }
    out
}

#[test]
fn difference_then_merge_gives_back_the_layout() {
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    for _ in 0..500 {
        let default = random_layout(&mut rng, false);
        let full = random_layout(&mut rng, false);
        let diff = difference(&full, &default);
        assert_eq!(merge(&default, &diff), full);
        // Differences hold only what actually differs.
        for (input, b) in &diff {
            assert_ne!(default.get(input), Some(b));
        }
        // And a game's differences survive the trip.
        let game = random_layout(&mut rng, true);
        let merged = merge(&default, &game);
        assert_eq!(merge(&default, &difference(&merged, &default)), merged);
    }
}

#[test]
fn promoting_leaves_the_game_with_no_differences_and_others_keep_theirs() {
    let d = gba_default();
    let this = from_pairs(&[("a", Some("b")), ("l4", Some("fast_forward"))]).unwrap();
    let other = from_pairs(&[("a", Some("select"))]).unwrap();
    let before_other = merge(&d, &other);
    let (new_default, this_after) = promote(&d, &this);
    assert!(this_after.is_empty());
    assert_eq!(merge(&new_default, &this_after), merge(&d, &this));
    let after_other = merge(&new_default, &other);
    assert_eq!(after_other["a"], bound("select"), "its own override kept");
    assert_eq!(
        after_other["l4"],
        bound("fast_forward"),
        "no override: follows"
    );
    for (input, b) in &before_other {
        if input != "l4" {
            assert_eq!(&after_other[input], b, "{input} changed");
        }
    }
}

#[test]
fn resetting_a_game_is_zero_differences() {
    let d = gba_default();
    let full = merge(&d, &Layout::new());
    assert!(difference(&full, &d).is_empty());
}

#[test]
fn two_inputs_may_share_a_target_but_one_input_cannot_have_two() {
    let shared = from_pairs(&[("dpad", Some("dpad")), ("left_stick", Some("dpad"))]).unwrap();
    validate(&gba(), &shared).unwrap();
    let err = from_pairs(&[("a", Some("a")), ("a", Some("b"))]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "A is bound twice. An input can do only one thing."
    );
}

#[test]
fn validation_names_what_is_wrong() {
    let bad = |pairs: &[(&str, Option<&str>)]| {
        validate(&gba(), &from_pairs(pairs).unwrap())
            .unwrap_err()
            .to_string()
    };
    assert_eq!(bad(&[("q", Some("a"))]), "q isn't a Deck input.");
    assert_eq!(
        bad(&[("a", Some("x"))]),
        "x isn't something retroarch can do for GBA."
    );
    assert_eq!(
        bad(&[("a", Some("dpad"))]),
        "D-pad needs the D-pad or a stick, not A."
    );
    assert_eq!(
        bad(&[("dpad", Some("a"))]),
        "A is a button; bind it to a button, not D-pad."
    );
    // Dolphin has no rewind.
    let gc = Scope::new("dolphin", System::Gamecube);
    let err = validate(&gc, &from_pairs(&[("l4", Some("rewind"))]).unwrap()).unwrap_err();
    assert!(err.to_string().contains("rewind"));
}

#[test]
fn scopes_are_stored_as_emulator_and_system() {
    assert_eq!(gba().key(), "retroarch/gba");
    assert_eq!(
        Scope::from_key("dolphin/gamecube"),
        Some(Scope::new("dolphin", System::Gamecube))
    );
    assert_eq!(Scope::from_key("retroarch"), None);
}
