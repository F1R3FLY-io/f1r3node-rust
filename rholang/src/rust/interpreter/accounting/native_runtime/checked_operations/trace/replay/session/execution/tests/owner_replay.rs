//! Added by G3 (DR-135, S6): native replay of runs with owner funding (plan
//! S6, rules R5 to R8, A5, SA4, SA5 and SA8).
//!
//! Each scenario stores owner state in a run without a binding, plays a run
//! of the client `firer` with an owner binding, and replays that run from the
//! stored state. An honest journal replays to the state and the outcome of
//! play. A journal that names a false refusal, drops a refusal, moves a
//! budget start past the first attempt of its row, or lists a false budget
//! is rejected: by the journal check, by the start of replay, or by the
//! replayed evaluation.

use std::sync::Arc;

use models::rhoapi::CostSignature;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use rspace_plus_plus::rspace::trace::event::{Event, Produce};

use super::*;
use crate::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, sig_to_cost_signature,
};
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::native_runtime::{
    NativeBudgetRecording, NativeCommSource, NativeObservationLink, NativeOperationRecord,
    NativeOperationSource, NativeProduceSource, NativeRefusal,
};
use crate::rust::interpreter::accounting::owner_funding::{
    OwnerFundingBinding, OwnerPurse, OwnerPurseSet, OwnerTable, OwnerTableEntry,
};
use crate::rust::interpreter::accounting::Sig;
use crate::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;

/// The price of the run that stores owner state.
const STORE_PRICE: u64 = 7;
/// The price of the client run that is replayed.
const FIRE_PRICE: u64 = 11;
/// The budget of a funded owner.
const FUNDED: u64 = u64::MAX / 4;
/// The bound of every run.
const LIMIT: u64 = 1_000_000;
/// The weights of every run: every byte class costs one unit.
const WEIGHTS: [u64; 4] = [1; 4];

type Outcome = (Result<(), InterpreterError>, EvaluationFailureSummary);
type PlaySpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

fn storer() -> Sig { Sig::Ground(vec![1]) }

fn firer() -> Sig { Sig::Ground(vec![2]) }

/// The signature that the surface signature `[ name ]` gives.
fn named(name: &str) -> CostSignature {
    sig_to_cost_signature(&Sig::Ground(canon_ground(name))).expect("a ground signature")
}

fn custody_of(signature: &CostSignature) -> [u8; 32] {
    cost_signature_to_sig(signature)
        .expect("a canonical signature")
        .lane_hash()
}

/// The purse locator of these tests: each signature is one purse, whose
/// custody is the lane of the signature.
fn test_purse(signature: &CostSignature) -> Option<OwnerPurseSet> {
    let custody = cost_signature_to_sig(signature).ok()?.lane_hash();
    Some(OwnerPurseSet::Single(OwnerPurse {
        custody,
        signature: signature.clone(),
    }))
}

/// The binding of a run of `firer`. Its full table lists every signature of
/// these tests with the budget `FUNDED`, except the names in `budgets`.
fn binding(budgets: &[(&str, u64)]) -> Arc<OwnerFundingBinding> {
    let mut signatures: Vec<CostSignature> = ["owner", "sender", "other_sender", "w"]
        .into_iter()
        .map(named)
        .collect();
    for signer in [storer(), firer()] {
        signatures.push(sig_to_cost_signature(&signer).expect("a ground signature"));
    }
    let mut entries: Vec<([u8; 32], OwnerTableEntry)> = signatures
        .into_iter()
        .map(|signature| {
            let budget = budgets
                .iter()
                .find(|(name, _)| named(name) == signature)
                .map_or(FUNDED, |(_, budget)| *budget);
            (custody_of(&signature), OwnerTableEntry {
                signature,
                budget,
            })
        })
        .collect();
    entries.sort_by_key(|(custody, _)| *custody);
    let room = entries.len();
    Arc::new(OwnerFundingBinding::new(
        [firer().lane_hash()].into_iter().collect(),
        test_purse,
        OwnerTable::new(entries, room).expect("distinct custody keys"),
    ))
}

fn channel(name: &str) -> Par {
    models::rust::utils::new_gstring_par(name.to_owned(), Vec::new(), false)
}

/// The values of `data`, sorted.
fn values(data: &[Datum<ListParWithRandom>]) -> Vec<Vec<u8>> {
    let mut values: Vec<Vec<u8>> = data
        .iter()
        .map(|datum| bincode::serialize(&datum.a.pars).expect("pars serialize"))
        .collect();
    values.sort();
    values
}

/// A played run of `firer` with owner funding, with what its replay needs.
struct Played {
    space: PlaySpace,
    parsed: Par,
    seed: Vec<u8>,
    outcome: Outcome,
    recording: NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    log: Vec<Event>,
}

/// Stores `store` for `storer` without a binding, checkpoints, and plays
/// `run` for `firer` with `binding` and the deploy seed `seed`.
async fn play(store: &str, run: &str, seed: &[u8], binding: &Arc<OwnerFundingBinding>) -> Played {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("the space opens");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"owner-replay-store", storer());
    budget
        .reset_for_native_execution(priced_config(LIMIT, WEIGHTS, STORE_PRICE))
        .expect("the store run starts");
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        space.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .expect("the play environment");
    block_data.write().await.block_number = 123;
    deploy_data.write().await.timestamp = 456;
    let scope = budget.enter_comm_accounting_scope();
    let stored = reducer
        .inj_with_observation(
            Compiler::source_to_adt(store).expect("the store term parses"),
            Blake2b512Random::create_from_bytes(b"owner-replay-store"),
        )
        .await;
    drop(scope);
    assert!(stored.0.is_ok(), "the store run fails: {:?}", stored.0);
    space.create_checkpoint().await.expect("the stored root");
    budget.set_deploy_signature_funded(seed, firer());
    budget
        .reset_for_native_execution(
            priced_config(LIMIT, WEIGHTS, FIRE_PRICE).with_owner_funding(Arc::clone(binding)),
        )
        .expect("the run starts");
    let parsed = Compiler::source_to_adt(run).expect("the run parses");
    let scope = budget.enter_comm_accounting_scope();
    let outcome = reducer
        .inj_with_observation(parsed.clone(), Blake2b512Random::create_from_bytes(seed))
        .await;
    drop(scope);
    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    let operations = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    let log = space.create_soft_checkpoint().await.log;
    Played {
        space,
        parsed,
        seed: seed.to_vec(),
        outcome,
        recording,
        operations,
        log,
    }
}

/// A replay that ran to its end: its outcome, its completed usage and the
/// data that it left on `names`.
struct Replayed {
    outcome: Outcome,
    complete: Result<(), String>,
    usage: Option<u64>,
    data: Vec<Vec<Vec<u8>>>,
    /// The owner-refused introductions of the recording that the replay
    /// budget reports as refused (SA5).
    refused: usize,
}

/// Replays `played` with `recording`, `operations` and `binding`. An error
/// is a rejection before the replayed evaluation starts.
async fn replay(
    played: &Played,
    recording: &NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    binding: &Arc<OwnerFundingBinding>,
    names: &[&str],
) -> Result<Replayed, String> {
    let checked = with_priced_contract(LIMIT, WEIGHTS, FIRE_PRICE, |contract| {
        contract.check_operation_journal_with_owners(
            recording.session,
            recording,
            operations,
            journal_limits(),
            Some(binding),
        )
    })
    .map_err(|error| format!("journal: {error:?}"))?;
    let trace = checked
        .bind_trace(played.log.clone().into(), NativeOperationTraceLimits {
            events: 1_000,
            source_entries: 10_000,
            source_bytes: 10_000_000,
            telemetry_items: 1_000,
            telemetry_bytes: 1_000_000,
        })
        .map_err(|error| format!("trace: {error:?}"))?;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(&played.seed, firer());
    budget
        .reset_for_native_execution(
            priced_config(LIMIT, WEIGHTS, FIRE_PRICE).with_owner_funding(Arc::clone(binding)),
        )
        .map_err(|error| format!("budget: {error:?}"))?;
    let mut environment = create_native_replay_env(
        trace,
        played.space.get_history_repository(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        budget,
        ExternalServices::noop(),
    )
    .await
    .map_err(|error| format!("environment: {error:?}"))?;
    environment.block_data.write().await.block_number = 123;
    environment.deploy_data.write().await.timestamp = 456;
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        environment.evaluate_raw(
            played.parsed.clone(),
            Blake2b512Random::create_from_bytes(&played.seed),
        ),
    )
    .await
    .map_err(|_| "the replayed evaluation did not complete".to_owned())?;
    let complete = environment
        .check_complete()
        .await
        .map_err(|error| format!("{error:?}"));
    let usage = environment.completed_usage().await.ok();
    let mut data = Vec::with_capacity(names.len());
    for name in names {
        data.push(
            environment
                .get_data(&channel(name))
                .await
                .map(|data| values(&data))
                .unwrap_or_default(),
        );
    }
    let refused = recording
        .attempts
        .iter()
        .filter(|attempt| {
            attempt.owner_refused
                && environment
                    .accounting_budget()
                    .introduction_refused(&attempt.observation.event_id)
        })
        .count();
    Ok(Replayed {
        outcome,
        complete,
        usage,
        data,
        refused,
    })
}

/// The data that play left on `names`.
async fn played_data(played: &Played, names: &[&str]) -> Vec<Vec<Vec<u8>>> {
    let mut data = Vec::with_capacity(names.len());
    for name in names {
        data.push(values(&played.space.get_data(&channel(name)).await));
    }
    data
}

/// Replay reproduces play: the outcome, the completed usage and the data on
/// `names`.
async fn assert_replays(
    played: &Played,
    replayed: Result<Replayed, String>,
    names: &[&str],
    context: &str,
) {
    let replayed = replayed.unwrap_or_else(|error| panic!("{context}: replay rejected: {error}"));
    assert_eq!(
        replayed.outcome.0.is_ok(),
        played.outcome.0.is_ok(),
        "{context}: {:?} differs from {:?}",
        replayed.outcome.0,
        played.outcome.0
    );
    assert_eq!(replayed.outcome.1, played.outcome.1, "{context}");
    assert_eq!(replayed.complete, Ok(()), "{context}");
    assert_eq!(replayed.usage, Some(played.recording.used), "{context}");
    assert_eq!(replayed.data, played_data(played, names).await, "{context}");
}

/// Replay rejects the journal: before the evaluation, in the evaluation, or
/// at its completion.
fn assert_rejected(played: &Played, replayed: Result<Replayed, String>, context: &str) {
    assert!(played.outcome.0.is_ok(), "{context}: play fails");
    if let Ok(replayed) = replayed {
        assert!(
            replayed.outcome.0.is_err() || replayed.complete.is_err(),
            "{context}: replay accepted the journal"
        );
    }
}

/// The row whose source is the consume of `source` or the produce of
/// `source`, by the first attempt that it links.
fn row_position(
    operations: &[NativeOperationRecord],
    matches: impl Fn(&NativeOperationRecord) -> bool,
) -> usize {
    let mut found = operations
        .iter()
        .enumerate()
        .filter(|(_, row)| matches(row));
    let (position, _) = found.next().expect("the row exists");
    assert!(found.next().is_none(), "one row matches");
    position
}

/// The attempt index that the introduction of `row` links.
fn introduction_attempt(row: &NativeOperationRecord) -> usize {
    match row.introduction {
        NativeObservationLink::Attempt(index) => index,
        NativeObservationLink::Retry(_) => panic!("the introduction is an attempt"),
    }
}

/// The owner state of the refusal scenarios: D1 of `sender` and D2 of
/// `other_sender` on `t13-in`, and a continuation of `owner` on `t13-x`.
const T13_STORE: &str = r#"
    {% @"t13-in"!(1) %}[ sender ] |
    {% @"t13-in"!(2) %}[ other_sender ] |
    {% for (@y <- @"t13-x") { @"t13-done"!(y) } %}[ owner ]
"#;

/// The client run of the refusal scenarios. The consume's body runs for `w`
/// (A5). With `w` unfunded, its greedy tuple, which skips D1 whose owner
/// cannot pay (R5) and takes D2, is refused, and the consume is stored with
/// one refusal (R7). The produce on `t13-x` matches the continuation of
/// `owner`.
const T13_RUN: &str = r#"
    for (@x <- @"t13-in") { {% @"t13-out"!(x) %}[ w ] } |
    @"t13-x"!(5)
"#;

const T13_NAMES: [&str; 4] = ["t13-in", "t13-out", "t13-x", "t13-done"];

fn t13_binding() -> Arc<OwnerFundingBinding> { binding(&[("sender", 0), ("w", 0)]) }

/// The row of the consume of the refusal scenarios: a Stored row with one
/// refusal.
fn refused_consume(operations: &[NativeOperationRecord]) -> usize {
    row_position(operations, |row| {
        matches!(row.source, NativeOperationSource::Consume(_)) && !row.refusals.is_empty()
    })
}

/// One round of T13 under the current runtime.
async fn t13_round(seed: &[u8], context: &str) {
    let binding = t13_binding();
    let played = play(T13_STORE, T13_RUN, seed, &binding).await;
    assert!(
        played.outcome.0.is_ok(),
        "{context}: play fails: {:?}",
        played.outcome.0
    );
    let consume = refused_consume(&played.operations);
    let consume_row = &played.operations[consume];
    assert_eq!(consume_row.completion, RSpaceOperationCompletion::Stored);
    assert_eq!(consume_row.refusals.len(), 1);

    // The honest journal replays.
    let honest = replay(
        &played,
        &played.recording,
        Arc::clone(&played.operations),
        &binding,
        &T13_NAMES,
    )
    .await;
    assert_replays(&played, honest, &T13_NAMES, &format!("{context}: honest")).await;

    // A removed refusal: the Stored rule finds the fundable tuple.
    let mut removed = played.operations.to_vec();
    removed[consume].refusals = Arc::from([]);
    let replayed = replay(
        &played,
        &played.recording,
        removed.into(),
        &binding,
        &T13_NAMES,
    )
    .await;
    assert_rejected(&played, replayed, &format!("{context}: removed refusal"));

    // A fake refusal of the matched produce: its COMM shows no shortfall.
    let produce = row_position(&played.operations, |row| {
        matches!(&row.source, NativeOperationSource::Produce(source)
            if source.channel == channel_hash(&channel("t13-x")))
            && row.completion == RSpaceOperationCompletion::Matched
    });
    let mut fake = played.operations.to_vec();
    let comm = fake[produce]
        .comm
        .as_ref()
        .expect("a matched COMM")
        .source
        .clone();
    let position = fake[produce].budget_start;
    fake[produce].refusals = Arc::from([NativeRefusal {
        source: comm,
        position,
    }]);
    let replayed = replay(
        &played,
        &played.recording,
        fake.into(),
        &binding,
        &T13_NAMES,
    )
    .await;
    assert_rejected(&played, replayed, &format!("{context}: fake refusal"));

    // A budget start past the first attempt of its row.
    let mut late = played.operations.to_vec();
    late[consume].budget_start = introduction_attempt(&late[consume]) + 1;
    let replayed = replay(
        &played,
        &played.recording,
        late.into(),
        &binding,
        &T13_NAMES,
    )
    .await;
    assert_rejected(&played, replayed, &format!("{context}: late budget start"));
}

/// The digest of the channel `channel`, as a COMM source records it.
fn channel_hash(channel: &Par) -> [u8; 32] {
    Produce::create(channel, &0u8, false)
        .channel_hash
        .0
        .as_slice()
        .try_into()
        .expect("a channel digest has 32 bytes")
}

/// A runtime for `workers` worker threads, or the current thread for 0.
fn runtime(workers: usize) -> tokio::runtime::Runtime {
    let mut builder = match workers {
        0 => tokio::runtime::Builder::new_current_thread(),
        workers => {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            builder.worker_threads(workers);
            builder
        }
    };
    builder
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
        .expect("a tokio runtime")
}

/// T13 (plan S6): replay requires and verifies each recorded refusal. Over
/// 64 seeds, on the current thread and on 2 and 8 workers, the honest
/// journal replays to the state of play. A journal without the refusal, with
/// a fake refusal, or with a budget start past the first attempt of the
/// refusing row is rejected.
#[test]
fn replay_requires_and_verifies_a_recorded_refusal() {
    for workers in [0, 2, 8] {
        let runtime = runtime(workers);
        for seed in 0..64_u8 {
            let context = format!("workers {workers}, seed {seed}");
            runtime.block_on(t13_round(&[b't', 13, seed], &context));
        }
    }
}

/// SA4 (plan S6): a refusal must name a tuple whose items pass at S_r. A
/// producer that names the tuple of D1, whose owner cannot pay its item,
/// instead of the refused tuple of D2 would hide the fundable tuple of the
/// consume from the Stored rule. The charge of the false tuple shows a
/// shortfall too, so only the strict item test rejects it.
#[tokio::test]
async fn replay_rejects_a_refusal_whose_items_fail_at_s_r() {
    let binding = t13_binding();
    let played = play(T13_STORE, T13_RUN, b"sa4", &binding).await;
    assert!(played.outcome.0.is_ok(), "{:?}", played.outcome.0);
    let consume = refused_consume(&played.operations);
    let honest = &played.operations[consume].refusals[0];
    let stored = played.space.get_data(&channel("t13-in")).await;
    let d1 = stored
        .iter()
        .find(|datum| {
            datum.a.cost_authority.as_ref().is_some_and(|seal| {
                seal.regions
                    .iter()
                    .any(|region| region.signature == Some(named("sender")))
            })
        })
        .expect("D1 of sender is stored")
        .source
        .clone();
    let d1 = NativeProduceSource {
        channel: d1
            .channel_hash
            .0
            .as_slice()
            .try_into()
            .expect("a 32-byte digest"),
        hash: d1.hash.0.as_slice().try_into().expect("a 32-byte digest"),
        persistent: d1.persistent,
    };
    assert_eq!(honest.source.produces.len(), 1);
    assert_ne!(
        honest.source.produces[0], d1,
        "play refused the tuple of D2"
    );
    let count = honest.source.repetitions[0].1;
    let forged = NativeCommSource {
        consume: honest.source.consume.clone(),
        produces: Arc::from([d1.clone()]),
        peeks: Arc::clone(&honest.source.peeks),
        repetitions: Arc::from([(d1, count)]),
    };
    let mut operations = played.operations.to_vec();
    let position = operations[consume].refusals[0].position;
    operations[consume].refusals = Arc::from([NativeRefusal {
        source: Arc::new(forged),
        position,
    }]);
    let replayed = replay(
        &played,
        &played.recording,
        operations.into(),
        &binding,
        &T13_NAMES,
    )
    .await;
    assert_rejected(&played, replayed, "a refusal of the tuple of D1");
}

/// A5 (plan S6): the body of a stored continuation is one term signed by
/// `w`, so it runs for `w`, and its COMM needs `w` to keep a positive budget
/// after the COMM. The body does no work, so no attempt of the run draws from
/// `w`. With `w` unfunded play refuses the COMM, and with `w` funded play
/// grants it. A journal of the granted run whose owner table lists `w` with
/// budget 0 still folds, because nothing draws from `w`, but replay rejects
/// its Matched row, because `w` has no budget after the COMM.
#[tokio::test]
async fn a_matched_comm_leaves_its_transferee_funded() {
    const STORE: &str = r#"for (@x <- @"a5-in") { {% Nil %}[ w ] }"#;
    const RUN: &str = r#"@"a5-in"!(1)"#;
    const NAMES: [&str; 1] = ["a5-in"];
    let unfunded = binding(&[("w", 0)]);
    let refused = play(STORE, RUN, b"a5", &unfunded).await;
    assert!(refused.outcome.0.is_ok(), "{:?}", refused.outcome.0);
    assert_eq!(
        values(&refused.space.get_data(&channel("a5-in")).await).len(),
        1,
        "an unfunded transferee refuses the COMM"
    );
    assert_eq!(
        refused
            .operations
            .iter()
            .map(|row| row.refusals.len())
            .sum::<usize>(),
        1
    );
    let funded = binding(&[]);
    let played = play(STORE, RUN, b"a5", &funded).await;
    assert!(played.outcome.0.is_ok(), "{:?}", played.outcome.0);
    assert!(
        played.space.get_data(&channel("a5-in")).await.is_empty(),
        "a funded transferee lets the COMM fire"
    );
    let honest = replay(
        &played,
        &played.recording,
        Arc::clone(&played.operations),
        &funded,
        &NAMES,
    )
    .await;
    assert_replays(&played, honest, &NAMES, "the funded transferee").await;

    let mut recording = played.recording.clone();
    let mut entries: Vec<OwnerTableEntry> = recording.owner_table.to_vec();
    for entry in &mut entries {
        if entry.signature == named("w") {
            entry.budget = 0;
        }
    }
    recording.owner_table = entries.into();
    let replayed = replay(
        &played,
        &recording,
        Arc::clone(&played.operations),
        &unfunded,
        &NAMES,
    )
    .await;
    assert_rejected(&played, replayed, "a transferee without budget");
}

/// R8, SA5 (plan S6): the body of a stored receive runs for `owner`, whose
/// budget pays exactly its share of the COMM. The send of the body then
/// cannot be paid: play refuses its introduction, stores nothing and
/// continues. Replay skips the introduction, stores nothing, and reproduces
/// play.
#[tokio::test]
async fn an_owner_refused_introduction_replays_as_a_skip() {
    const STORE: &str = r#"for ({% @x <- @"r50-in" %}[ owner ]) { @"r50-out"!(x) }"#;
    const RUN: &str = r#"@"r50-in"!(1) | @"r50-side"!(2)"#;
    const NAMES: [&str; 3] = ["r50-in", "r50-out", "r50-side"];
    // A calibration run measures the owner's share of the COMM.
    let funded = binding(&[]);
    let calibration = play(STORE, RUN, b"r50", &funded).await;
    assert!(calibration.outcome.0.is_ok(), "{:?}", calibration.outcome.0);
    let owner = custody_of(&named("owner"));
    let fold = |played: &Played, binding: &OwnerFundingBinding| {
        with_priced_contract(LIMIT, WEIGHTS, FIRE_PRICE, |contract| {
            contract
                .check_operation_journal_with_owners(
                    played.recording.session,
                    &played.recording,
                    Arc::clone(&played.operations),
                    journal_limits(),
                    Some(binding),
                )
                .expect("the journal checks")
                .budget_evidence()
                .owner()
                .expect("an owner fold")
                .ledger()
                .debits()
                .get(&owner)
        })
    };
    let introduction = calibration
        .recording
        .attempts
        .iter()
        .filter(|attempt| {
            attempt.observation.kind == AuthorityByteEventKind::ProduceIntroduction
                && attempt.observation.authority.regions.iter().any(|region| {
                    region.signature == Some(named("owner")) && region.price == STORE_PRICE
                })
        })
        .map(|attempt| {
            attempt
                .observation
                .measurement
                .expect("a measured introduction")
                .introduction_bytes
                * STORE_PRICE
        })
        .next()
        .expect("the owner introduces its send");
    let comm_share = fold(&calibration, funded.as_ref()) - introduction;
    assert!(comm_share > 0 && introduction > 0);

    let calibrated = binding(&[("owner", comm_share)]);
    let played = play(STORE, RUN, b"r50", &calibrated).await;
    assert!(played.outcome.0.is_ok(), "{:?}", played.outcome.0);
    assert!(played.space.get_data(&channel("r50-out")).await.is_empty());
    assert_eq!(
        played
            .recording
            .attempts
            .iter()
            .filter(|attempt| attempt.owner_refused)
            .count(),
        1
    );
    let replayed = replay(
        &played,
        &played.recording,
        Arc::clone(&played.operations),
        &calibrated,
        &NAMES,
    )
    .await;
    // SA5: the replay budget reports the skipped introduction as refused, as
    // the play budget does, so the reducer drops the same stack transfers.
    let refused = replayed.as_ref().map_or(0, |replayed| replayed.refused);
    assert_replays(&played, replayed, &NAMES, "the skipped introduction").await;
    assert_eq!(refused, 1, "the replay budget records the refusal");
}
