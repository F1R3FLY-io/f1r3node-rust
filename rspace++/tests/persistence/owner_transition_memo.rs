use std::cell::Cell;
use std::panic::catch_unwind;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::checkpoint_wal::fixture_for_profile;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_for_profile, limits};
use super::durable_rspace_authority::{Profile, Store, poll_once, publication_limits};
use super::owner_cross_base_checkpoint::{
    ARMED_FAILPOINT_REFUSAL, Contents, DIRECTORY_FAILPOINTS, Step, Trial, View, apply_all,
    continuation, directory_image, generated_steps, panic_text, read_contents, view_limits,
};
use super::owner_incarnation::journal_bags;
use super::support::trial_scratch;
use super::trusted_successor_production::{PublicationEffects, observed_effects};
use crate::rspace::hot_store::{DurableHotStoreBootstrapLimits, HotStoreInstances};
use crate::rspace::persistence::difference::StateDifferenceLimits;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableIoFailpointV1, DurableRSpaceAuthorityV1,
    DurableRSpaceTransactionError, DurableStateRootV1, GenerationCheckpointPublicationHookPointV1,
    ResolvedDurableStateV1, ViewBaseV1, ViewCheckpointLimitsV1, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, recover_read_authority,
};
use crate::rspace::persistence::{LogPackDecodeLimits, SemanticProfile};

type Owner = DurableRSpaceAuthorityV1<Profile>;
type Answered = Result<ResolvedDurableStateV1, String>;
type ArmedAnswer = (Option<DurableIoFailpointV1>, Answered, Quiet, Option<String>);

const PRODUCTION_CAPACITY: usize = 4_096;
const SMALL_CAPACITY: usize = 2;
const WALK_REFUSAL: &str = "Rebase(Format(ResourceLimit { resource: \"state difference object \
                            reads\", limit: 0, attempted: 1 }))";
const GENERATED_SEQUENCES: u32 = 24;
const MAX_BATCHES: usize = 2;
const MAX_JOURNALS: usize = 3;
const MAX_PICKS: usize = 3;
const MAX_STEPS: usize = 4;
const BASE_SELECTORS: usize = 6;

fn owner_of(directory: &Path, capacity: usize) -> (Store, Arc<RwLock<()>>, Owner) {
    let view = recover_read_authority::<Profile>(
        directory,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the store directory recovers");
    let hot: Store = Arc::new(
        HotStoreInstances::create_from_durable(&view, DurableHotStoreBootstrapLimits::default())
            .expect("the hot store loads the recovered state"),
    );
    let owner = Owner::from_view_with_transition_memo_for_test(view, &hot, capacity)
        .unwrap_or_else(|failure| {
            panic!("the recovered view hands over its authority: {:?}", failure.1)
        });
    (hot, Arc::new(RwLock::new(())), owner)
}

fn trial_with_capacity(capacity: usize) -> Trial {
    let scratch = trial_scratch("owner-transition-memo-");
    let image = checkpoint_image_for_profile::<Profile>(fixture_for_profile(Profile::ID));
    install_checkpoint_image(scratch.path(), &image);
    let (hot, gate, owner) = owner_of(scratch.path(), capacity);
    Trial::over(scratch, hot, gate, owner)
}

fn without_a_walk() -> ViewCheckpointLimitsV1 {
    let limits = view_limits();
    ViewCheckpointLimitsV1 {
        difference: StateDifferenceLimits {
            max_object_reads: 0,
            max_object_read_bytes: 0,
            ..limits.difference
        },
        ..limits
    }
}

fn view_at(trial: &Trial, state: usize, journal: &[Step]) -> View {
    let (root, contents) = trial.states[state].clone();
    let mut view = trial.view(root, contents);
    apply_all(&view.hot, &mut view.contents, journal);
    view
}

fn described(
    result: Result<ResolvedDurableStateV1, Box<DurableRSpaceTransactionError>>,
) -> Answered {
    result.map_err(|refusal| format!("{refusal:?}"))
}

fn probed(trial: &mut Trial, state: usize, journal: &[Step]) -> Answered {
    let view = view_at(trial, state, journal);
    described(trial.checkpoint_with(view.base, &view.hot, without_a_walk()))
}

fn checkpointed(trial: &mut Trial, state: usize, journal: &[Step]) -> ResolvedDurableStateV1 {
    let view = view_at(trial, state, journal);
    trial
        .checkpoint_with(view.base, &view.hot, view_limits())
        .expect("a view of a retained state checkpoints within the full limits")
}

fn checkpoint_at(
    owner: &mut Owner,
    gate: &Arc<RwLock<()>>,
    base: ViewBaseV1,
    hot: &Store,
    limits: ViewCheckpointLimitsV1,
) -> Answered {
    let permit = Arc::clone(gate)
        .try_write_owned()
        .expect("the view gate is free");
    poll_once(owner.checkpoint_view_from_base(permit, base, gate, hot, limits))
        .map_err(|refusal| format!("{refusal:?}"))
}

fn stored_values(hot: &Store, channel: &str) -> Vec<String> {
    hot.get_data(&channel.to_owned())
        .into_iter()
        .map(|stored| stored.a.as_str().to_owned())
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Quiet {
    effects: PublicationEffects,
    capture_hook_run: bool,
    directory_unchanged: bool,
    slot_unchanged: bool,
    current_state_unchanged: bool,
    journal_unchanged: bool,
    memo_unchanged: bool,
    gate_free: bool,
}

fn quiet() -> Quiet {
    Quiet {
        effects: PublicationEffects {
            trace: Vec::new(),
            cold_admissions: 0,
            index_constructions: 0,
        },
        capture_hook_run: false,
        directory_unchanged: true,
        slot_unchanged: true,
        current_state_unchanged: true,
        journal_unchanged: true,
        memo_unchanged: true,
        gate_free: true,
    }
}

fn answered_without_a_walk(trial: &mut Trial, view: &View) -> (Answered, Quiet) {
    let files = directory_image(trial.path());
    let slot = trial.slot();
    let current = trial.owner.current_root();
    let journals = journal_bags(&view.hot);
    let memo = trial.owner.transition_memo_lengths_for_test();
    let captured = Rc::new(Cell::new(false));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
        let captured = Rc::clone(&captured);
        move || captured.set(true)
    });
    let (result, effects) =
        observed_effects(|| trial.checkpoint_with(view.base, &view.hot, without_a_walk()));
    drop(hook);
    (described(result), Quiet {
        effects,
        capture_hook_run: captured.get(),
        directory_unchanged: directory_image(trial.path()) == files,
        slot_unchanged: trial.slot() == slot,
        current_state_unchanged: trial.owner.current_root() == current,
        journal_unchanged: journal_bags(&view.hot) == journals,
        memo_unchanged: trial.owner.transition_memo_lengths_for_test() == memo,
        gate_free: matches!(trial.gate.try_write(), Ok(_)),
    })
}

fn answers_alone_and_under_every_failpoint(trial: &mut Trial, view: &View) -> Vec<ArmedAnswer> {
    let mut answers = Vec::with_capacity(DIRECTORY_FAILPOINTS.len() + 1);
    let (reached, unchanged) = answered_without_a_walk(trial, view);
    answers.push((None, reached, unchanged, None));
    for failpoint in DIRECTORY_FAILPOINTS {
        answers.push(std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    arm_durable_io_failpoint(failpoint);
                    let (reached, unchanged) = answered_without_a_walk(trial, view);
                    let rearmed = catch_unwind(move || arm_durable_io_failpoint(failpoint))
                        .err()
                        .map(|payload| panic_text(payload.as_ref()).to_owned());
                    (Some(failpoint), reached, unchanged, rearmed)
                })
                .join()
                .expect("the failpoint thread completes")
        }));
    }
    answers
}

fn assert_memoised_answers(answers: &[ArmedAnswer], memoised: ResolvedDurableStateV1) {
    assert_eq!(
        answers.len(),
        DIRECTORY_FAILPOINTS.len() + 1,
        "a memoised transition is repeated once alone and once under every directory failpoint"
    );
    for (failpoint, reached, unchanged, rearmed) in answers {
        assert_eq!(
            (reached, unchanged, rearmed.as_deref()),
            (&Ok(memoised), &quiet(), failpoint.map(|_| ARMED_FAILPOINT_REFUSAL)),
            "a memoised transition must be returned as the state of its first publication, within \
             a walk read budget of zero, without an effect, a write or a sync, with {failpoint:?} \
             armed and still armed afterwards"
        );
    }
}

#[test]
fn a_memoised_transition_is_returned_without_a_write_a_sync_or_a_walk() {
    let mut trial = Trial::open();
    trial.publish(&[Step::put("a", "v0")]);
    let journal = [Step::put("b", "v0")];
    let memoised = checkpointed(&mut trial, 1, &journal);
    let superseding = checkpointed(&mut trial, 1, &[Step::put("c", "v1")]);
    assert_eq!(
        (trial.owner.current_root(), trial.owner.transition_memo_lengths_for_test()),
        (superseding.root(), (2, 2)),
        "two views of the first state must publish one after the other, the second from a \
         superseded base, and leave two memoised transitions"
    );

    let detached = journal_bags(&view_at(&trial, 1, &[]).hot);
    let repeated = view_at(&trial, 1, &journal);
    let journals = journal_bags(&repeated.hot);
    let binding = trial.owner.state(&repeated.hot).ok();
    let superseded = answers_alone_and_under_every_failpoint(&mut trial, &repeated);
    assert_memoised_answers(&superseded, memoised);
    assert_eq!(
        (
            binding,
            trial.owner.state(&repeated.hot).ok(),
            journals != detached,
            journal_bags(&repeated.hot) == journals,
            stored_values(&repeated.hot, "b"),
        ),
        (None, None, true, true, vec!["v0".to_owned()]),
        "after a memoised answer from a superseded base the view's store must still hold its \
         journal, unacknowledged, and stay bound to no catalog slot"
    );

    let unmemoised = view_at(&trial, 1, &[Step::put("b", "v1")]);
    let starved = answered_without_a_walk(&mut trial, &unmemoised);
    assert_eq!(
        starved,
        (Err(WALK_REFUSAL.to_owned()), quiet()),
        "a transition that the memo does not hold must be refused by the first object read of the \
         difference walk under the same read budget of zero, changing nothing"
    );

    let lengths = trial.owner.transition_memo_lengths_for_test();
    let slot = trial.slot();
    let (tripped, cleared) = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
                let refusal = trial
                    .checkpoint_with(unmemoised.base, &unmemoised.hot, view_limits())
                    .err()
                    .map(|refusal| format!("{refusal:?}"));
                let cleared =
                    catch_unwind(|| arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite))
                        .is_ok();
                (refusal, cleared)
            })
            .join()
            .expect("the liveness control thread completes")
    });
    assert!(
        cleared &&
            tripped.as_deref().is_some_and(|refusal| {
                refusal.contains("injected durable I/O failure at PhysicalWrite")
            }) &&
            trial.owner.transition_memo_lengths_for_test() == lengths,
        "a publishing view checkpoint in a failpoint thread must trip and clear PhysicalWrite and \
         memoise nothing: {tripped:?} {cleared}"
    );
    let reconciled = trial
        .owner
        .reconcile(publication_limits())
        .expect("the owner reconciles the interrupted publication");
    let retried = trial
        .checkpoint_with(unmemoised.base, &unmemoised.hot, view_limits())
        .expect("the interrupted view checkpoint publishes when it is retried");
    assert_eq!(
        (
            reconciled,
            trial.slot().state().generation(),
            trial.owner.current_root(),
            trial.owner.transition_memo_lengths_for_test(),
        ),
        (slot.state(), slot.state().generation() + 1, retried.root(), (3, 3)),
        "the interrupted publication reconciles to the previous current state and its retry \
         publishes one generation later and is memoised"
    );

    let mut returning = trial.view(retried.root(), unmemoised.contents.clone());
    apply_all(&returning.hot, &mut returning.contents, &[Step::take("b", "v1")]);
    let returned = trial
        .checkpoint_with(returning.base, &returning.hot, view_limits())
        .expect("a view of the current state publishes through the same-base path");
    assert_eq!(
        (
            returned.root(),
            trial.owner.current_root(),
            trial.slot().state().generation(),
            trial.owner.transition_memo_lengths_for_test(),
        ),
        (trial.states[1].0, trial.states[1].0, slot.state().generation() + 2, (4, 4)),
        "taking the datum back must publish the contents of the first state again, at a later \
         catalog slot"
    );

    let again = view_at(&trial, 1, &journal);
    let journals = journal_bags(&again.hot);
    let binding = trial.owner.state(&again.hot).ok();
    let current = answers_alone_and_under_every_failpoint(&mut trial, &again);
    assert_memoised_answers(&current, memoised);
    assert_eq!(
        (
            binding,
            trial.owner.state(&again.hot).ok(),
            journals != detached,
            journal_bags(&again.hot) == journals,
            stored_values(&again.hot, "b"),
        ),
        (None, Some(trial.slot().state()), true, true, vec!["v0".to_owned()]),
        "after a memoised answer from the current state the view's store must still hold its \
         journal, unacknowledged, bound to the current catalog slot"
    );
    println!(
        "owner transition memo TM-01: {} memoised answers from a superseded base, {} from the \
         current state",
        superseded.len(),
        current.len()
    );
}

#[derive(Clone, Debug)]
struct Sequence {
    batches: Vec<Vec<Step>>,
    checkpoints: Vec<(usize, Vec<Step>)>,
    echoed: bool,
}

fn seed_sequences() -> [Sequence; 4] {
    [
        Sequence {
            batches: vec![vec![Step::put("a", "v0")]],
            checkpoints: vec![
                (0, vec![Step::put("b", "v0")]),
                (1, vec![Step::put("b", "v0")]),
                (0, vec![Step::put("b", "v0")]),
                (1, vec![Step::put("b", "v0")]),
            ],
            echoed: false,
        },
        Sequence {
            batches: vec![vec![Step::put("a", "v0")]],
            checkpoints: vec![
                (1, vec![Step::put("b", "v0")]),
                (2, vec![Step::take("b", "v0")]),
                (1, vec![Step::put("b", "v0")]),
                (2, vec![Step::take("b", "v0")]),
                (1, vec![Step::put("b", "v0"), Step::put("c", "v1"), Step::take("c", "v1")]),
            ],
            echoed: false,
        },
        Sequence {
            batches: vec![vec![Step::put("a", "v0"), Step::install(&["a", "b"], "k0")]],
            checkpoints: vec![
                (1, vec![Step::install(&["a"], "k1"), Step::withdraw(&["a", "b"], "k0")]),
                (0, vec![Step::put("b", "v1")]),
                (1, vec![Step::install(&["a"], "k1"), Step::withdraw(&["a", "b"], "k0")]),
                (1, vec![Step::withdraw(&["a", "b"], "k0"), Step::install(&["a"], "k1")]),
            ],
            echoed: false,
        },
        Sequence {
            batches: vec![vec![Step::put("a", "v0")], vec![Step::put("b", "v0")]],
            checkpoints: vec![
                (1, vec![Step::put("b", "v0")]),
                (1, vec![Step::put("b", "v0")]),
                (0, vec![Step::put("a", "v0"), Step::put("b", "v0")]),
                (0, vec![Step::put("b", "v0"), Step::put("a", "v0")]),
                (1, vec![Step::put("c", "v0"), Step::take("c", "v0")]),
            ],
            echoed: false,
        },
    ]
}

fn generated_sequences() -> impl Strategy<Value = Sequence> {
    let journals = || prop::collection::vec(generated_steps(), 0..=MAX_STEPS);
    (
        prop::collection::vec(journals(), 1..=MAX_BATCHES),
        prop::collection::vec(journals(), 1..=MAX_JOURNALS),
        prop::collection::vec((0..BASE_SELECTORS, 0..MAX_JOURNALS), 1..=MAX_PICKS),
    )
        .prop_map(|(batches, pool, picks)| Sequence {
            batches,
            checkpoints: picks
                .into_iter()
                .map(|(selector, pick)| (selector, pool[pick % pool.len()].clone()))
                .collect(),
            echoed: true,
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    JournalWithoutEffect,
    SameBasePublication,
    SameBaseMemo,
    CrossBasePublication,
    CrossBaseBatchWithoutEffect,
    CrossBaseMemo,
}

const fn predicted(effect: bool, memoised: bool, same_state: bool, current: bool) -> Answer {
    match (effect, memoised, same_state, current) {
        (false, _, _, _) => Answer::JournalWithoutEffect,
        (true, true, true, _) => Answer::SameBaseMemo,
        (true, true, false, _) => Answer::CrossBaseMemo,
        (true, false, true, _) => Answer::SameBasePublication,
        (true, false, false, true) => Answer::CrossBaseBatchWithoutEffect,
        (true, false, false, false) => Answer::CrossBasePublication,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Answers {
    sequences: u32,
    checkpoints: u32,
    journals_without_effect: u32,
    same_base_publications: u32,
    same_base_memo_answers: u32,
    cross_base_publications: u32,
    cross_base_batches_without_effect: u32,
    cross_base_memo_answers: u32,
    sequences_with_a_repeated_base: u32,
    sequences_with_distinct_bases: u32,
}

impl Answers {
    fn count(&mut self, answer: Answer) {
        self.checkpoints += 1;
        *match answer {
            Answer::JournalWithoutEffect => &mut self.journals_without_effect,
            Answer::SameBasePublication => &mut self.same_base_publications,
            Answer::SameBaseMemo => &mut self.same_base_memo_answers,
            Answer::CrossBasePublication => &mut self.cross_base_publications,
            Answer::CrossBaseBatchWithoutEffect => &mut self.cross_base_batches_without_effect,
            Answer::CrossBaseMemo => &mut self.cross_base_memo_answers,
        } += 1;
    }

    fn merge(&mut self, other: Self) {
        let Self {
            sequences,
            checkpoints,
            journals_without_effect,
            same_base_publications,
            same_base_memo_answers,
            cross_base_publications,
            cross_base_batches_without_effect,
            cross_base_memo_answers,
            sequences_with_a_repeated_base,
            sequences_with_distinct_bases,
        } = other;
        self.sequences += sequences;
        self.checkpoints += checkpoints;
        self.journals_without_effect += journals_without_effect;
        self.same_base_publications += same_base_publications;
        self.same_base_memo_answers += same_base_memo_answers;
        self.cross_base_publications += cross_base_publications;
        self.cross_base_batches_without_effect += cross_base_batches_without_effect;
        self.cross_base_memo_answers += cross_base_memo_answers;
        self.sequences_with_a_repeated_base += sequences_with_a_repeated_base;
        self.sequences_with_distinct_bases += sequences_with_distinct_bases;
    }
}

fn observed_answer(
    trial: &mut Trial,
    view: &View,
    effect: bool,
) -> Result<(ResolvedDurableStateV1, Answer), TestCaseError> {
    let same_state = view.base.state().root() == trial.owner.current_root();
    let before = trial.slot();
    let within_the_full_limits = |trial: &mut Trial| {
        trial
            .checkpoint_with(view.base, &view.hot, view_limits())
            .map_err(|refusal| {
                TestCaseError::fail(format!(
                    "the owner refused a view checkpoint from a retained base: {refusal:?}"
                ))
            })
    };
    let (reached, memoised) = match (effect, same_state) {
        (false, _) | (true, true) => (within_the_full_limits(trial)?, false),
        (true, false) => {
            match described(trial.checkpoint_with(view.base, &view.hot, without_a_walk())) {
                Ok(reached) => (reached, true),
                Err(refusal) => {
                    prop_assert_eq!(
                        (refusal.as_str(), trial.slot()),
                        (WALK_REFUSAL, before),
                        "a view checkpoint from a superseded base that the memo does not answer \
                         must be refused by the first object read of the difference walk when the \
                         walk may read nothing, changing nothing"
                    );
                    (within_the_full_limits(trial)?, false)
                }
            }
        }
    };
    let after = trial.slot();
    let moved = match after == before {
        true => false,
        false => {
            prop_assert_eq!(
                (after.state().generation(), after.state().manifest()),
                (before.state().generation() + 1, reached.manifest()),
                "a view checkpoint that moves the catalog slot must publish the returned state \
                 one generation later"
            );
            true
        }
    };
    let answer = match (effect, same_state, memoised, moved) {
        (false, _, false, false) => Answer::JournalWithoutEffect,
        (true, true, false, true) => Answer::SameBasePublication,
        (true, true, false, false) => Answer::SameBaseMemo,
        (true, false, false, true) => Answer::CrossBasePublication,
        (true, false, false, false) => Answer::CrossBaseBatchWithoutEffect,
        (true, false, true, false) => Answer::CrossBaseMemo,
        (false, _, _, true) |
        (false, _, true, false) |
        (true, _, true, true) |
        (true, true, true, false) => {
            return Err(TestCaseError::fail(format!(
                "a view checkpoint answered in a way no path of the owner allows: journal with \
                 effect {effect}, base equal to the current state {same_state}, answered within a \
                 walk read budget of zero {memoised}, catalog slot moved {moved}"
            )));
        }
    };
    Ok((reached, answer))
}

struct Pair {
    memoising: Trial,
    plain: Trial,
    transitions: Vec<(DurableStateRootV1, Contents)>,
    bases: Vec<DurableStateRootV1>,
}

impl Pair {
    fn check(&mut self, state: usize, journal: &[Step]) -> Result<Answer, TestCaseError> {
        let (root, base_contents) = self.memoising.states[state].clone();
        let mut memoising_view = self.memoising.view(root, base_contents.clone());
        let mut plain_view = self.plain.view(root, base_contents.clone());
        apply_all(&memoising_view.hot, &mut memoising_view.contents, journal);
        apply_all(&plain_view.hot, &mut plain_view.contents, journal);
        let expected = memoising_view.contents.clone();
        let effect = expected != base_contents;
        let memoised = self
            .transitions
            .iter()
            .any(|(base, contents)| *base == root && *contents == expected);
        let [with_memo, without_memo] = [&self.memoising, &self.plain].map(|trial| {
            let current = trial.owner.current_root();
            (current == root, read_contents(&trial.owner, current) == expected)
        });
        let (reached, answer) = observed_answer(&mut self.memoising, &memoising_view, effect)?;
        let (reference, unmemoised) = observed_answer(&mut self.plain, &plain_view, effect)?;
        prop_assert_eq!(
            reached,
            reference,
            "the owner with the memo and the owner without one must return the same state"
        );
        prop_assert_eq!(
            (
                read_contents(&self.memoising.owner, reached.root()),
                read_contents(&self.plain.owner, reference.root()),
            ),
            (expected.clone(), expected.clone()),
            "the returned state must hold the base's contents with the journal applied in both \
             stores"
        );
        if effect && !memoised {
            self.transitions.push((root, expected.clone()));
        }
        let recorded = self.transitions.len();
        prop_assert_eq!(
            (
                answer,
                unmemoised,
                self.memoising.owner.transition_memo_lengths_for_test(),
                self.plain.owner.transition_memo_lengths_for_test(),
            ),
            (
                predicted(effect, memoised, with_memo.0, with_memo.1),
                predicted(effect, false, without_memo.0, without_memo.1),
                (recorded, recorded),
                (0, 0),
            ),
            "the owner with the memo must answer from it exactly when it has completed the same \
             journal effect from the same base before and must hold one transition for each such \
             pair, and the owner whose memo has capacity zero must never answer from a memo and \
             hold nothing"
        );
        if !self
            .memoising
            .states
            .iter()
            .any(|(known, _)| *known == reached.root())
        {
            self.memoising.states.push((reached.root(), expected));
        }
        self.bases.push(root);
        Ok(answer)
    }
}

fn check_sequence(sequence: &Sequence) -> Result<Answers, TestCaseError> {
    let calls = sequence.checkpoints.len() * (1 + usize::from(sequence.echoed));
    let mut pair = Pair {
        memoising: Trial::open(),
        plain: trial_with_capacity(0),
        transitions: Vec::with_capacity(calls),
        bases: Vec::with_capacity(calls),
    };
    prop_assert_eq!(
        (
            pair.memoising.owner.transition_memo_capacity_for_test(),
            pair.plain.owner.transition_memo_capacity_for_test(),
        ),
        (PRODUCTION_CAPACITY, 0),
        "the first owner is built by the production constructor and the second with a memo of \
         capacity zero"
    );
    for batch in &sequence.batches {
        prop_assert_eq!(
            pair.memoising.publish(batch),
            pair.plain.publish(batch),
            "the two stores must publish the same state for the same batch"
        );
    }
    let mut answers = Answers {
        sequences: 1,
        ..Answers::default()
    };
    let mut echoes = Vec::with_capacity(sequence.checkpoints.len());
    for (selector, journal) in &sequence.checkpoints {
        let state = selector % pair.memoising.states.len();
        answers.count(pair.check(state, journal)?);
        echoes.push((state, journal));
    }
    if sequence.echoed {
        for (state, journal) in echoes {
            answers.count(pair.check(state, journal)?);
        }
    }
    let mut distinct = pair.bases.clone();
    distinct.sort_unstable();
    distinct.dedup();
    answers.sequences_with_a_repeated_base = u32::from(distinct.len() < pair.bases.len());
    answers.sequences_with_distinct_bases = u32::from(distinct.len() > 1);
    Ok(answers)
}

#[test]
fn every_checkpoint_result_equals_that_of_an_owner_without_a_memo() {
    let mut seeded = Answers::default();
    for sequence in seed_sequences() {
        seeded.merge(check_sequence(&sequence).expect("a seeded sequence satisfies TM-02"));
    }
    println!("owner transition memo TM-02 seeds: {seeded:?}");
    assert_eq!(
        seeded,
        Answers {
            sequences: 4,
            checkpoints: 18,
            journals_without_effect: 1,
            same_base_publications: 3,
            same_base_memo_answers: 2,
            cross_base_publications: 3,
            cross_base_batches_without_effect: 2,
            cross_base_memo_answers: 7,
            sequences_with_a_repeated_base: 4,
            sequences_with_distinct_bases: 4,
        },
        "the seeded sequences must be answered from the memo twice from the current state and \
         seven times from a superseded base, and take every other exit of a view checkpoint"
    );
    let generated = Cell::new(Answers::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_SEQUENCES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_sequences(), |sequence| {
        let mut total = generated.get();
        total.merge(check_sequence(&sequence)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated sequences satisfy TM-02");
    let generated = generated.get();
    println!("owner transition memo TM-02 generated sequences: {generated:?}");
    assert!(
        generated.sequences >= GENERATED_SEQUENCES,
        "every generated sequence must be checked: {generated:?}"
    );
}

#[test]
fn the_memo_is_bounded_evicts_its_oldest_transition_and_is_empty_after_close_and_a_reopen() {
    let production = Trial::open();
    assert_eq!(
        (
            production.owner.transition_memo_capacity_for_test(),
            production.owner.transition_memo_lengths_for_test(),
            production.owner.transition_memo_reserved_for_test() >= PRODUCTION_CAPACITY,
        ),
        (PRODUCTION_CAPACITY, (0, 0), true),
        "an owner built by the production constructor must reserve room for 4,096 transitions and \
         hold none"
    );
    drop(production);

    let mut trial = trial_with_capacity(SMALL_CAPACITY);
    trial.publish(&[Step::put("a", "v0")]);
    let journals = [[Step::put("b", "v0")], [Step::put("c", "v0")], [Step::put("b", "v1")]];
    let lengths = |trial: &Trial| trial.owner.transition_memo_lengths_for_test();
    let first = checkpointed(&mut trial, 1, &journals[0]);
    let one = lengths(&trial);
    let second = checkpointed(&mut trial, 1, &journals[1]);
    let two = lengths(&trial);
    let touched = probed(&mut trial, 1, &journals[0]);
    let third = checkpointed(&mut trial, 1, &journals[2]);
    let three = lengths(&trial);
    let answers = [0, 1, 2].map(|index| probed(&mut trial, 1, &journals[index]));
    assert_eq!(
        (
            trial.owner.transition_memo_capacity_for_test(),
            one,
            two,
            touched,
            three,
            answers,
            lengths(&trial),
        ),
        (
            SMALL_CAPACITY,
            (1, 1),
            (2, 2),
            Ok(first),
            (2, 2),
            [Err(WALK_REFUSAL.to_owned()), Ok(second), Ok(third)],
            (2, 2),
        ),
        "a memo of capacity two must drop the transition it recorded first when it records a \
         third, although that transition was the last one it answered, and must keep the other two"
    );
    let republished = checkpointed(&mut trial, 1, &journals[0]);
    let answers = [0, 1, 2].map(|index| probed(&mut trial, 1, &journals[index]));
    assert_eq!(
        (republished, answers, lengths(&trial)),
        (first, [Ok(first), Err(WALK_REFUSAL.to_owned()), Ok(third)], (2, 2)),
        "publishing the dropped transition again must record it as the newest and drop the older \
         of the two transitions that were held"
    );

    let late = view_at(&trial, 1, &journals[2]);
    let held = described(trial.checkpoint_with(late.base, &late.hot, without_a_walk()));
    trial.owner.close().expect("the quiescent owner closes");
    assert_eq!(
        (held, lengths(&trial)),
        (Ok(third), (0, 0)),
        "a closed owner must hold no memoised transition"
    );
    assert_eq!(
        (
            trial.owner.transition_memo_capacity_for_test(),
            trial.owner.transition_memo_reserved_for_test(),
        ),
        (0, 0),
        "a closed owner must release the storage of its memo"
    );

    let (reopened_hot, reopened_gate, mut reopened) = owner_of(trial.path(), SMALL_CAPACITY);
    let fresh = reopened
        .attach(&reopened_hot)
        .expect("the reopened owner's store attaches")
        .incarnation();
    let opened = reopened.transition_memo_lengths_for_test();
    let base = ViewBaseV1::new(fresh, late.base.state());
    let missed = checkpoint_at(&mut reopened, &reopened_gate, base, &late.hot, without_a_walk());
    let walked = checkpoint_at(&mut reopened, &reopened_gate, base, &late.hot, view_limits());
    assert_eq!(
        (opened, missed, walked, reopened.transition_memo_lengths_for_test()),
        ((0, 0), Err(WALK_REFUSAL.to_owned()), Ok(third), (1, 1)),
        "an owner reopened on the same directory must start without a memoised transition: the \
         transition that the closed owner held must take the difference walk again"
    );
}

#[test]
fn equal_after_values_share_a_key_and_one_value_path_or_domain_apart_do_not() {
    let mut trial = Trial::open();
    trial.publish(&[Step::put("a", "v0")]);
    let datum = checkpointed(&mut trial, 1, &[Step::put("b", "v0")]);
    let installed = checkpointed(&mut trial, 1, &[Step::install(&["a", "b"], "k0")]);
    let answers = [
        probed(&mut trial, 1, &[Step::put("a", "v1"), Step::take("a", "v1"), Step::put("b", "v0")]),
        probed(&mut trial, 1, &[
            Step::install(&["a"], "k1"),
            Step::install(&["a", "b"], "k0"),
            Step::withdraw(&["a"], "k1"),
        ]),
        probed(&mut trial, 1, &[Step::put("b", "v0"), Step::put("b", "v0")]),
        probed(&mut trial, 1, &[Step::put("b", "v1")]),
        probed(&mut trial, 1, &[Step::put("c", "v0")]),
        probed(&mut trial, 1, &[Step::put("b", "v0"), Step::put("c", "v0")]),
        probed(&mut trial, 1, &[Step::install(&["a", "b"], "k1")]),
        probed(&mut trial, 1, &[Step::install(&["b", "a"], "k0")]),
    ];
    let refused = || Err(WALK_REFUSAL.to_owned());
    assert_eq!(
        answers,
        [
            Ok(datum),
            Ok(installed),
            refused(),
            refused(),
            refused(),
            refused(),
            refused(),
            refused(),
        ],
        "journals that leave the same after values must be answered from the memo whatever their \
         steps were, and a journal one count, one path, one further entry, one continuation or \
         one channel group apart must not"
    );

    let channels = vec!["a".to_owned(), "b".to_owned()];
    let continuation_alone = view_at(&trial, 1, &[]);
    assert_eq!(
        continuation_alone
            .hot
            .put_continuation(&channels, continuation(&["a", "b"], "k0")),
        Some(true),
        "the hot store takes a continuation it does not hold"
    );
    let joins_alone = view_at(&trial, 1, &[]);
    for member in &channels {
        joins_alone
            .hot
            .put_join(member, &channels)
            .expect("the hot store takes the join");
    }
    let complete = probed(&mut trial, 1, &[Step::install(&["a", "b"], "k0")]);
    assert_eq!(
        [&continuation_alone, &joins_alone].map(|view| {
            described(trial.checkpoint_with(view.base, &view.hot, without_a_walk()))
        }),
        [refused(), refused()],
        "a journal that holds only the continuation entry or only the join entries of a memoised \
         transition must not be answered from the memo"
    );
    assert_eq!(
        (complete, trial.owner.transition_memo_lengths_for_test()),
        (Ok(installed), (2, 2)),
        "the journal with the continuation entry and the join entries is answered from the memo, \
         and the refused journals are not recorded"
    );
}

#[test]
fn a_memoised_state_that_the_store_does_not_retain_is_not_returned() {
    let mut trial = Trial::open();
    trial.publish(&[Step::put("a", "v0")]);
    let journal = [Step::put("b", "v0")];
    let memoised = checkpointed(&mut trial, 1, &journal);
    checkpointed(&mut trial, 1, &[Step::put("c", "v1")]);
    let held = probed(&mut trial, 1, &journal);
    let stranger = {
        let mut other = Trial::open();
        other.publish(&[Step::put("c", "v1"), Step::put("c", "v1")]);
        view_at(&other, 1, &[]).base.state()
    };
    trial.owner.replace_memoised_successors_for_test(stranger);
    let unretained = view_at(&trial, 1, &journal);
    let refused = answered_without_a_walk(&mut trial, &unretained);
    let republished =
        described(trial.checkpoint_with(unretained.base, &unretained.hot, view_limits()));
    let restored = probed(&mut trial, 1, &journal);
    assert_eq!(
        (held, refused, republished, restored, trial.owner.transition_memo_lengths_for_test()),
        (Ok(memoised), (Err(WALK_REFUSAL.to_owned()), quiet()), Ok(memoised), Ok(memoised), (2, 2)),
        "a memoised state that the store does not retain must not be returned: the transition \
         takes the difference walk, publishes its successor again, replaces the state in the memo \
         and is answered from the memo afterwards"
    );
}

#[test]
fn a_publication_completed_by_reconcile_is_not_memoised() {
    let mut trial = Trial::open();
    trial.publish(&[Step::put("a", "v0")]);
    trial.publish(&[Step::put("c", "v1")]);
    let journal = [Step::put("b", "v0")];
    let interrupted = view_at(&trial, 1, &journal);
    arm_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    );
    let uncertain = trial
        .checkpoint_with(interrupted.base, &interrupted.hot, view_limits())
        .is_err();
    let terminal = trial.owner.pending_terminal();
    let pending = trial.owner.transition_memo_lengths_for_test();
    let reconciled = trial.owner.reconcile(publication_limits()).ok();
    let completed = trial.owner.transition_memo_lengths_for_test();
    assert!(
        uncertain && terminal.is_some() && reconciled == terminal.map(|slot| slot.state()),
        "a view checkpoint interrupted after the catalog sync must leave a pending record that \
         reconciliation completes at its terminal: {terminal:?} {reconciled:?}"
    );
    let missed = probed(&mut trial, 1, &journal);
    let walked = checkpointed(&mut trial, 1, &journal);
    let recorded = trial.owner.transition_memo_lengths_for_test();
    let answered = probed(&mut trial, 1, &journal);
    assert_eq!(
        (pending, completed, missed, walked.root(), recorded, answered),
        (
            (0, 0),
            (0, 0),
            Err(WALK_REFUSAL.to_owned()),
            trial.owner.current_root(),
            (1, 1),
            Ok(walked),
        ),
        "neither the interrupted call nor the reconciliation that completes its publication may \
         memoise the transition: its repetition takes the difference walk, finds the current \
         state unchanged, is memoised then and answered from the memo afterwards"
    );
}
