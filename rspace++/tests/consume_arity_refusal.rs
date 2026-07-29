//! # ★★ A malformed `consume`/`install` is REFUSED, not aborted
//!
//! ## The classification, and why it is the opposite of #127/#136's
//!
//! `ISpace::consume` and `ISpace::install` have always returned
//! `Result<_, RSpaceError>`. Four sites nevertheless answered an
//! **arity or emptiness fault in their own arguments** with `panic!`, spending
//! the process instead of the error channel that was already there.
//!
//! The question that decides the disposition of any such guard is:
//!
//! > **can the caller's own inputs express the fault?**
//!
//! | site | fault | expressible by the caller? | disposition |
//! |---|---|---|---|
//! | `SpatialMatcher` on an absent required child (#127/#136) | a `Par` with `EMinus.p1 = None` | no honest producer emits it; it is not a term | **invariant** — it stays fatal |
//! | `consume`/`install` arity | `channels.len() != patterns.len()` | **yes** — two independent repeated fields on the wire | **decidable negative** — it refuses |
//!
//! The two lengths are *arguments*. `ConsumeParams` and `InstallParams`
//! (`models/src/main/protobuf/RSpacePlusPlusTypes.proto`) carry `channels` and
//! `patterns` as two **independent** `repeated` fields, so every wire encoding
//! that exists can carry a mismatch, and `channels` may simply be absent. That
//! is #73's rule — *undecidable guards refuse loudly* — applied to a second
//! surface.
//!
//! ## ★ The four that moved, and the fifth that had already moved
//!
//! | # | function | faults | before | after |
//! |---|---|---|---|---|
//! | 1 | `RSpace::consume` | empty, arity | `panic!` ×2 | `Err` ×2 |
//! | 2 | `RSpace::locked_install_internal` | arity | `panic!` | `Err` |
//! | 3 | `ReplayRSpace::consume` | empty, arity | `panic!` ×2 | `Err` ×2 |
//! | 4 | `ReplayRSpace::locked_install_internal` | arity | **already `Err`** | `Err` (untouched) |
//!
//! ★★ Row 4 is the finding. The ReplaySpace half of the `install` pair was
//! **already** returning `RSpaceError::BugFoundError` with this exact text, so
//! before this change `RSpace::install` killed the node where
//! `ReplayRSpace::install` refused — **a play/replay asymmetry sitting in the
//! tree**. It was masked, not absent: both of `locked_install_internal`'s
//! in-tree callers (`restore_installs` and
//! `rho_runtime::introduce_system_process`) convert the `Err` straight back
//! into a panic, so nothing downstream could ever see the difference.
//! [`play_and_replay_agree_on_every_malformed_shape`] is the cell that pins the
//! agreement, and it is the reason all four had to move together rather than
//! one being "the bug".
//!
//! ## Anti-vacuity
//!
//! A space that refused *everything* would satisfy every RED cell here. Each
//! RED is therefore paired with a **CONTROL that succeeds in the same run, on
//! the same space, through the same method** — differing only in the length of
//! the `patterns` vector. [`the_refusal_does_not_half_write`] adds the other
//! half: a refusal that had already installed a continuation would be a worse
//! defect than the panic it replaced, because the panic at least wrote nothing.
//!
//! ⚠ This file proves the refusal EXISTS. It does not prove anyone can REACH
//! it — the arguments here are built in Rust, three lines from the call. The
//! reachability measurement, from bytes a foreign caller encodes through the
//! real exported `extern "C"` symbols, is
//! `rspace++/libs/rspace_rhotypes/tests/ffi_consume_arity_reachability.rs`.
//!
//! ## ⚠ Consensus
//!
//! This converts *"every node dies"* into *"this call fails"*, which is
//! consensus-visible: a validator that today aborts would instead reject. It
//! requires a coordinated `Validate::version` bump — `casper/src/rust/validate.rs`
//! compares versions for exact equality with no activation-height machinery —
//! and that bump is **F1r3node's act, not this change's**.

use std::collections::BTreeSet;
use std::sync::Arc;

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════
// The test doubles — the same `String`/`Pattern` pair the rest of the rspace
// suite uses (`storage_actions_test.rs`, `replay_rspace_tests.rs`), so nothing
// here depends on a shape invented for this file.
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
enum Pattern {
    #[default]
    Wildcard,
}

impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

#[derive(Clone)]
struct StringMatch;

impl Match<Pattern, String, String> for StringMatch {
    fn get(&self, p: &Pattern, a: &String) -> Option<String> {
        match p {
            Pattern::Wildcard => Some(a.clone()),
        }
    }
}

impl rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode for Pattern {
    fn cold_decode_prefix(
        bytes: &[u8],
    ) -> Result<
        (Self, usize),
        rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecodeError,
    > {
        rspace_plus_plus::rspace::serializers::cold_store_decode::legacy_prefix(bytes)
    }
}

type Play = RSpace<String, Pattern, String, String>;
type Replay = ReplayRSpace<String, Pattern, String, String>;

/// Both halves of the pair, over ONE history repository — the same construction
/// `replay_rspace_tests.rs::fixture` uses. Building them together is what makes
/// [`play_and_replay_agree_on_every_malformed_shape`] a comparison rather than
/// two unrelated observations.
async fn play_and_replay() -> (Play, Replay) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("the in-memory store manager yields rspace stores");

    let history_repo = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::lmdb_repository(
            store.history.clone(),
            store.roots.clone(),
            store.cold.clone(),
        )
        .expect("the history repository is constructible over in-memory stores"),
    );

    let history_reader = history_repo
        .get_history_reader(&history_repo.root())
        .expect("the empty root has a history reader");

    let play_store = {
        let hr = history_reader.base();
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), hr)
    };
    let play = RSpace::apply(history_repo.clone(), play_store, Arc::new(Box::new(StringMatch)));

    let replay_store = {
        let hr = history_reader.base();
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), hr)
    };
    let replay =
        ReplayRSpace::apply(history_repo, Arc::new(replay_store), Arc::new(Box::new(StringMatch)));

    (play, replay)
}

/// ★ The three argument shapes, named once so every cell below quantifies over
/// the SAME set and none can silently test fewer than the others.
///
/// `Wellformed` is the control; the other two are the two decidable negatives
/// the four sites discriminate on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// 2 channels, 2 patterns — must be accepted.
    Wellformed,
    /// 2 channels, 1 pattern — `channels.len() != patterns.len()`.
    ArityMismatch,
    /// 0 channels, 0 patterns — `channels.is_empty()`.
    NoChannels,
}

impl Shape {
    const ALL: [Shape; 3] = [Shape::Wellformed, Shape::ArityMismatch, Shape::NoChannels];

    fn channels(self) -> Vec<String> {
        match self {
            Shape::Wellformed | Shape::ArityMismatch => {
                let mut v = Vec::with_capacity(2);
                v.push("ch1".to_string());
                v.push("ch2".to_string());
                v
            }
            Shape::NoChannels => Vec::new(),
        }
    }

    fn patterns(self) -> Vec<Pattern> {
        match self {
            Shape::Wellformed => {
                let mut v = Vec::with_capacity(2);
                v.push(Pattern::Wildcard);
                v.push(Pattern::Wildcard);
                v
            }
            Shape::ArityMismatch => {
                let mut v = Vec::with_capacity(1);
                v.push(Pattern::Wildcard);
                v
            }
            Shape::NoChannels => Vec::new(),
        }
    }

    /// The exact message the site must produce, or `None` for the control.
    ///
    /// ⚠ Held to the LITERAL text rather than merely to "some error": the four
    /// sites are required to agree with each other and with the one that was
    /// already converted, and only the literal can show that.
    fn expected_message(self) -> Option<&'static str> {
        match self {
            Shape::Wellformed => None,
            Shape::ArityMismatch => {
                Some("RUST ERROR: channels.length must equal patterns.length")
            }
            Shape::NoChannels => Some("RUST ERROR: channels can't be empty"),
        }
    }
}

/// The `Err` a refusal must be, unpacked to its text. Any other variant, or an
/// `Ok`, is a failure with the whole outcome printed.
fn refusal_text<T: std::fmt::Debug>(
    outcome: &Result<T, RSpaceError>,
    what: &str,
) -> String {
    match outcome {
        Err(RSpaceError::BugFoundError(msg)) => msg.clone(),
        Err(other) => panic!(
            "★ {what} refused, but with the wrong variant: {other:?}. The four sites must all \
             answer with `RSpaceError::BugFoundError`, which is the variant \
             `ReplayRSpace::locked_install_internal` already used before this change."
        ),
        Ok(value) => panic!(
            "★★ {what} did NOT refuse — it returned Ok({value:?}). Either the guard was removed \
             or this cell is no longer reaching it."
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// A — `RSpace::consume`
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn play_consume_refuses_the_malformed_and_accepts_the_control() {
    let (play, _replay) = play_and_replay().await;

    for shape in Shape::ALL {
        let outcome = play
            .consume(
                shape.channels(),
                shape.patterns(),
                "k".to_string(),
                false,
                BTreeSet::new(),
            )
            .await;

        match shape.expected_message() {
            // ★ THE CONTROL, in the same run and on the same space as the REDs.
            // Without it, a space that refused everything would pass this file.
            None => {
                let accepted = outcome.unwrap_or_else(|e| {
                    panic!(
                        "★ CONTROL: a 2-channel/2-pattern consume was refused with {e:?}. With \
                         this cell red, the refusals below are not attributable to the malformed \
                         arity — they would be attributable to calling `consume` at all."
                    )
                });
                assert!(
                    accepted.is_none(),
                    "the control consume found no resting data, so it must install and return None"
                );
            }
            Some(expected) => {
                let text = refusal_text(&outcome, &format!("RSpace::consume on {shape:?}"));
                assert_eq!(
                    text, expected,
                    "★ RSpace::consume on {shape:?} refused with the wrong text"
                );
            }
        }
    }
}

/// ★ A refusal that had already written would be WORSE than the abort it
/// replaced: the panic at least left the store untouched. This is the cell that
/// says the new `Err` is a refusal and not a half-completed consume.
#[tokio::test]
async fn the_refusal_does_not_half_write() {
    let (play, _replay) = play_and_replay().await;

    let channels = Shape::ArityMismatch.channels();
    let before = play.get_store().changes().len();

    let outcome = play
        .consume(
            channels.clone(),
            Shape::ArityMismatch.patterns(),
            "k".to_string(),
            false,
            BTreeSet::new(),
        )
        .await;
    let _ = refusal_text(&outcome, "RSpace::consume on ArityMismatch");

    assert_eq!(
        play.get_store().changes().len(),
        before,
        "★★ the refused consume MUTATED the hot store. A refusal must be a no-op; the guard sits \
         before `Consume::create` and before any lock is taken precisely so that nothing has to \
         be rolled back."
    );
    assert!(
        play.get_store().get_continuations(&channels).is_empty(),
        "★★ the refused consume installed a waiting continuation on {channels:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// B — `RSpace::install` (i.e. `locked_install_internal`)
//
// `install` has no empty-channels guard — only the arity one — so `NoChannels`
// is ACCEPTED here. That asymmetry is pre-existing and deliberate: this cell
// pins it rather than pretending the two functions are the same function.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn play_install_refuses_the_arity_mismatch_and_accepts_the_control() {
    let (play, _replay) = play_and_replay().await;

    let control = play
        .install(
            Shape::Wellformed.channels(),
            Shape::Wellformed.patterns(),
            "k".to_string(),
        )
        .await;
    assert!(
        control.expect("★ CONTROL: a 2-channel/2-pattern install must succeed").is_none(),
        "the control install found no resting data, so it must install and return None"
    );

    let outcome = play
        .install(
            Shape::ArityMismatch.channels(),
            Shape::ArityMismatch.patterns(),
            "k".to_string(),
        )
        .await;
    assert_eq!(
        refusal_text(&outcome, "RSpace::install on ArityMismatch"),
        Shape::ArityMismatch
            .expected_message()
            .expect("ArityMismatch has an expected message"),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// C — `ReplayRSpace::consume`, the sharper half
//
// ⚠ A refusal that happens during REPLAY but not during PLAY (or the reverse)
// is not an inconsistency, it is the divergence class itself: replay would
// report a mismatch against a play run that never saw one. That is why this
// cell exists as its own quantification and not as a footnote to A.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn replay_consume_refuses_the_malformed_and_accepts_the_control() {
    let (_play, replay) = play_and_replay().await;

    for shape in Shape::ALL {
        let outcome = replay
            .consume(
                shape.channels(),
                shape.patterns(),
                "k".to_string(),
                false,
                BTreeSet::new(),
            )
            .await;

        match shape.expected_message() {
            None => {
                let accepted = outcome.unwrap_or_else(|e| {
                    panic!(
                        "★ CONTROL: a 2-channel/2-pattern replay consume was refused with {e:?}"
                    )
                });
                assert!(
                    accepted.is_none(),
                    "the control replay consume found no resting data, so it must return None"
                );
            }
            Some(expected) => {
                let text =
                    refusal_text(&outcome, &format!("ReplayRSpace::consume on {shape:?}"));
                assert_eq!(
                    text, expected,
                    "★ ReplayRSpace::consume on {shape:?} refused with the wrong text"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// D — ★★ THE PAIR AGREES
// ═══════════════════════════════════════════════════════════════════════════

/// Play and replay must reach the SAME verdict on the SAME arguments, for
/// `consume` and for `install` alike.
///
/// ★ Before this change they did **not**, for `install`: `RSpace::install`
/// aborted the process while `ReplayRSpace::install` returned
/// `Err(BugFoundError(..))` — the same fault, two dispositions, in the two
/// halves of the pair whose whole job is to agree. It was invisible only
/// because both in-tree callers re-raise the `Err` as a panic, so the two
/// observable behaviours coincided by accident downstream.
///
/// This cell would have been RED before the change on `install`/`ArityMismatch`
/// in the strongest possible way — by killing the test process — and it is
/// green after it.
#[tokio::test]
async fn play_and_replay_agree_on_every_malformed_shape() {
    let (play, replay) = play_and_replay().await;

    // `install` — the row that used to disagree. `NoChannels` is omitted
    // because `install` has no emptiness guard on either side.
    for shape in [Shape::Wellformed, Shape::ArityMismatch] {
        let play_outcome = play
            .install(shape.channels(), shape.patterns(), "k".to_string())
            .await;
        let replay_outcome = replay
            .install(shape.channels(), shape.patterns(), "k".to_string())
            .await;

        assert_eq!(
            play_outcome.is_err(),
            replay_outcome.is_err(),
            "★★ install on {shape:?}: play returned {play_outcome:?} and replay returned \
             {replay_outcome:?}. A fault one half refuses and the other accepts IS the \
             replay-divergence class."
        );
        if let Some(expected) = shape.expected_message() {
            assert_eq!(refusal_text(&play_outcome, "play install"), expected);
            assert_eq!(refusal_text(&replay_outcome, "replay install"), expected);
        }
    }

    // `consume` — symmetric before the change (both panicked) and symmetric
    // after it (both refuse). Asserted so the pair cannot drift apart later in
    // the direction `install` had already drifted.
    for shape in Shape::ALL {
        let play_outcome = play
            .consume(shape.channels(), shape.patterns(), "k".to_string(), false, BTreeSet::new())
            .await;
        let replay_outcome = replay
            .consume(shape.channels(), shape.patterns(), "k".to_string(), false, BTreeSet::new())
            .await;

        assert_eq!(
            play_outcome.is_err(),
            replay_outcome.is_err(),
            "★★ consume on {shape:?}: play returned {play_outcome:?} and replay returned \
             {replay_outcome:?}"
        );
        if let Some(expected) = shape.expected_message() {
            assert_eq!(refusal_text(&play_outcome, "play consume"), expected);
            assert_eq!(refusal_text(&replay_outcome, "replay consume"), expected);
        }
    }
}
