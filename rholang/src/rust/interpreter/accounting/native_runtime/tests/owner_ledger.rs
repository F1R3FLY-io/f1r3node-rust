//! Added by G3 (DR-135): the owner ledger of the native recorder and the
//! owner fold of replay (plan S4, rules R3, R4, R7, R8, R9, SA5, SA6 and
//! SA8).
//!
//! The runs of these tests drive the recorder directly, as RSpace does: an
//! operation starts, its introduction is charged, a COMM may be charged, and
//! the operation finishes. Every owner region has the price `OWNER_PRICE`,
//! and the transfer weight is 1, so a charge of `n` transfer bytes draws
//! `n * OWNER_PRICE` from the purse of its owner.

// Changed by G3 (DR-135, S4 review): the fold keeps no restoration map.
// use std::collections::{BTreeMap, BTreeSet};
use std::collections::BTreeSet;

use futures::FutureExt;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use proptest::prelude::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::*;
use crate::rust::interpreter::accounting::authority::{
    canonical_regions, cost_signature_to_sig, priced_cost_region,
};
use crate::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use crate::rust::interpreter::accounting::native_phlo_rules::OwnerFoldInput;
use crate::rust::interpreter::accounting::owner_funding::{
    OwnerCommExtras, OwnerFundingBinding, OwnerPurse, OwnerPurseSet, OwnerTable, OwnerTableEntry,
};

/// The price of every owner region of these tests.
const OWNER_PRICE: u64 = 3;

fn owner(byte: u8) -> CostSignature {
    authority::sig_to_cost_signature(&Sig::Ground(vec![0xA0, byte])).expect("a ground signature")
}

fn custody(signature: &CostSignature) -> [u8; 32] {
    cost_signature_to_sig(signature)
        .expect("a canonical signature")
        .lane_hash()
}

/// The purse locator of these tests: each signature is one purse, whose
/// custody is the lane of the signature.
fn locate(signature: &CostSignature) -> Option<OwnerPurseSet> {
    let custody = cost_signature_to_sig(signature).ok()?.lane_hash();
    Some(OwnerPurseSet::Single(OwnerPurse {
        custody,
        signature: signature.clone(),
    }))
}

/// A binding that lists `budgets` in a table with `room`. The client lane is
/// the lane of the signer of `budget`, which owns no region of these tests.
fn binding(
    budget: &RuntimeBudget,
    budgets: &[(CostSignature, u64)],
    room: usize,
) -> Arc<OwnerFundingBinding> {
    let mut entries: Vec<([u8; 32], OwnerTableEntry)> = budgets
        .iter()
        .map(|(signature, budget)| {
            (custody(signature), OwnerTableEntry {
                signature: signature.clone(),
                budget: *budget,
            })
        })
        .collect();
    entries.sort_by_key(|(custody, _)| *custody);
    let table = OwnerTable::new(entries, room).expect("ascending custody within the room");
    Arc::new(OwnerFundingBinding::new(
        [budget.signature().lane_hash()].into_iter().collect(),
        locate,
        table,
    ))
}

/// Starts a native run of `budget` with `binding`, the bound `limit` and the
/// transfer weight 1.
fn start_run(budget: &RuntimeBudget, binding: &Arc<OwnerFundingBinding>, limit: u64) {
    budget
        .reset_for_native_execution(
            priced_config(limit, [0, 0, 1, 0], OWNER_PRICE).with_owner_funding(Arc::clone(binding)),
        )
        .expect("an idle budget starts a native run");
}

/// An authority with one region of `signature` at the owner price.
fn owned(signature: &CostSignature, entropy: &[u8]) -> CostAuthority {
    CostAuthority {
        regions: vec![
            priced_cost_region(signature, entropy, 0, OWNER_PRICE).expect("a priced owner region")
        ],
    }
}

fn transfer(bytes: u64) -> ByteCharge {
    ByteCharge {
        introduction_bytes: 0,
        transfer_bytes: bytes,
        trace_bytes: 0,
    }
}

/// Added by G3 (DR-135, S11): the signer of the client regions of these
/// tests (item 13).
fn client() -> CostSignature {
    authority::sig_to_cost_signature(&Sig::Ground(vec![0xC0, 1])).expect("a ground signature")
}

/// Added by G3 (DR-135, S11): `binding` whose client lane is the lane of
/// `client`.
fn client_binding(
    budget: &RuntimeBudget,
    budgets: &[(CostSignature, u64)],
    room: usize,
) -> Arc<OwnerFundingBinding> {
    Arc::new(OwnerFundingBinding::new(
        [custody(&client())].into_iter().collect(),
        locate,
        binding(budget, budgets, room).table().clone(),
    ))
}

/// Added by G3 (DR-135, S11): an authority with one region of `signature` at
/// the price `price`.
fn owned_at(signature: &CostSignature, entropy: &[u8], price: u64) -> CostAuthority {
    CostAuthority {
        regions: vec![
            priced_cost_region(signature, entropy, 0, price).expect("a priced owner region")
        ],
    }
}

/// Added by G3 (DR-135, S11): an authority with a region of `signature` at
/// the owner price and a region of `client`. Each region uses the whole
/// measurement, so the client part is half of the usage.
fn mixed(signature: &CostSignature, entropy: &[u8]) -> CostAuthority {
    canonical_regions(
        [
            priced_cost_region(signature, entropy, 0, OWNER_PRICE).expect("a priced owner region"),
            priced_cost_region(&client(), entropy, 1, OWNER_PRICE).expect("a priced client region"),
        ]
        .iter(),
    )
    .expect("a canonical mixed authority")
    .into_authority()
}

/// Added by G3 (DR-135, S11): an authority with one region of `client`.
fn client_only(entropy: &[u8]) -> CostAuthority { owned_at(&client(), entropy, OWNER_PRICE) }

/// Added by G3 (DR-135, S11): an introduction with `authority` that charges
/// `bytes`, in an operation of its own.
fn introduce(
    budget: &RuntimeBudget,
    index: usize,
    authority: &CostAuthority,
    bytes: u64,
) -> Result<(), InterpreterError> {
    with_operation(budget, || {
        budget.reserve_produce_introduction_measured(
            [index as u8; 32],
            authority,
            transfer(bytes),
            false,
        )
    })
}

fn channel(id: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(id)),
    }])
}

/// Runs `action` in the operation context of the outer index `index`, as
/// RSpace does for one locked operation.
fn scope<T>(budget: &RuntimeBudget, index: usize, action: impl FnOnce() -> T) -> T {
    operation_context::scope(
        OperationOrder {
            session: budget.deploy_id(),
            path: vec![(index as u64, 0)].into(),
        },
        async { action() },
    )
    .now_or_never()
    .expect("the operation scope completes at once")
}

/// Starts a recorded operation and returns the name of its row.
fn start_row(
    budget: &RuntimeBudget,
    source: RSpaceOperationSource<'_>,
    channels: &[Par],
) -> OperationOrder {
    // Changed by G3 (DR-135, S6): play starts every operation through
    // `start_native_owner_operation`.
    // budget
    //     .start_native_operation(source, channels, &[])
    //     .expect("the operation starts")
    //     .expect("native accounting names each row")
    budget
        .start_native_owner_operation(source, channels, &[])
        .expect("the operation starts")
        .0
        .expect("native accounting names each row")
}

/// One produce operation whose introduction is owned by `signature` and
/// charges `bytes`. It returns the result of the introduction, and the
/// operation completes as RSpace completes it.
// Changed by G3 (DR-135, S11): the body moved to `produce_operation_with`,
// which takes the authority of the introduction.
fn produce_operation(
    budget: &RuntimeBudget,
    index: usize,
    source: &Produce,
    channel: &Par,
    signature: &CostSignature,
    bytes: u64,
) -> Result<(), InterpreterError> {
    produce_operation_with(
        budget,
        index,
        source,
        channel,
        &owned(signature, &[index as u8]),
        bytes,
    )
}

/// Added by G3 (DR-135, S11): `produce_operation` with the authority
/// `authority`.
fn produce_operation_with(
    budget: &RuntimeBudget,
    index: usize,
    source: &Produce,
    channel: &Par,
    authority: &CostAuthority,
    bytes: u64,
) -> Result<(), InterpreterError> {
    scope(budget, index, || {
        let reference = RSpaceOperationSource::Produce(source);
        let name = start_row(budget, reference, std::slice::from_ref(channel));
        operation_context::sync_scope(name, || {
            // Changed by G3 (DR-135, S11): the caller gives the authority.
            // let result = budget.reserve_produce_introduction_measured(
            //     produce_introduction_identity(source),
            //     &owned(signature, &[index as u8]),
            //     transfer(bytes),
            //     source.persistent,
            // );
            let result = budget.reserve_produce_introduction_measured(
                produce_introduction_identity(source),
                authority,
                transfer(bytes),
                source.persistent,
            );
            let completion = match result {
                Ok(()) => RSpaceOperationCompletion::Stored,
                Err(_) => RSpaceOperationCompletion::Rejected,
            };
            budget.finish_native_operation(reference, completion);
            result
        })
    })
}

/// The fold of the recording of `budget` with the owner terms of `binding`
/// and no restorations, under the contract of `start_run`.
fn fold_accepts(budget: &RuntimeBudget, binding: &OwnerFundingBinding, limit: u64) -> bool {
    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    with_priced_contract(limit, [0, 0, 1, 0], OWNER_PRICE, |contract| {
        contract
            .check_budget_evidence(
                recording.session,
                Arc::clone(&recording.attempts),
                Arc::clone(&recording.transfers),
                config(limit, [0, 0, 1, 0]).limits,
                Some(OwnerFoldInput {
                    binding,
                    table: binding.table().clone(),
                    // Changed by G3 (DR-135, S4 review): the fold keeps no restoration map.
                    // restorations: BTreeMap::new(),
                }),
            )
            .is_ok()
    })
}

/// R8, SA5: an introduction whose owner cannot pay records an attempt with
/// the decision OwnerRefused and a `Rejected` row with no COMM. It uses no
/// budget, its owner keeps its remaining budget, and the run remembers the
/// refusal. The journal check with the binding accepts the evidence, and the
/// check without owner funding rejects it.
#[test]
fn introduction_refused_records_owner_refused_and_a_rejected_row() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let funded = owner(1);
    let binding = binding(&budget, &[(funded.clone(), 5)], 4);
    start_run(&budget, &binding, 100);
    let _accounting = budget.enter_comm_accounting_scope();
    let (first_channel, second_channel) = (channel(1), channel(2));
    let first = Produce::create(&first_channel, &7u8, false);
    let second = Produce::create(&second_channel, &7u8, false);
    produce_operation(&budget, 0, &first, &first_channel, &funded, 1)
        .expect("the first introduction draws 3 of 5");
    assert!(matches!(
        produce_operation(&budget, 1, &second, &second_channel, &funded, 1),
        Err(InterpreterError::RSpaceError(
            RSpaceError::IntroductionRefused
        ))
    ));
    assert!(budget.introduction_refused(&produce_introduction_identity(&second)));
    assert!(!budget.introduction_refused(&produce_introduction_identity(&first)));
    assert_eq!(budget.native_phlo_usage(), Some(1));
    assert!(budget.authority_frontier().is_empty());
    let ledger = budget
        .native_owner_ledger()
        .expect("the run has an owner ledger");
    assert_eq!(ledger.remaining(&custody(&funded)), Some(2));
    assert_eq!(ledger.debits().get(&custody(&funded)), 3);
    assert_eq!((ledger.usage(), ledger.rev()), (1, 3));

    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    let decisions: Vec<(bool, bool)> = recording
        .attempts
        .iter()
        .map(|attempt| (attempt.granted, attempt.owner_refused))
        .collect();
    assert_eq!(decisions, vec![(true, false), (false, true)]);
    assert_eq!(recording.owner_table.len(), 1);
    let rows = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    assert_eq!(rows[1].completion, RSpaceOperationCompletion::Rejected);
    assert!(rows[1].comm.is_none() && rows[1].refusals.is_empty());
    assert_eq!(rows[1].introduction, NativeObservationLink::Attempt(1));
    with_priced_contract(100, [0, 0, 1, 0], OWNER_PRICE, |contract| {
        let checked = contract
            .check_operation_journal_with_owners(
                recording.session,
                &recording,
                Arc::clone(&rows),
                journal_limits(),
                Some(&binding),
            )
            .expect("the journal check confirms the refusal");
        assert_eq!(checked.total(), 1);
        assert!(matches!(
            contract.check_operation_journal(
                recording.session,
                &recording,
                Arc::clone(&rows),
                journal_limits()
            ),
            Err(NativeOperationJournalError::OwnerTable)
        ));
    });
}

/// The COMM event of a consume on `channel` with the continuation tag `tag`
/// that matches `produce`.
fn comm_event(channel: &Par, tag: u8, produce: &Produce, peek: bool) -> COMM {
    COMM {
        consume: Consume::create(&vec![channel.clone()], &vec![0u8], &tag, false),
        produces: vec![produce.clone()],
        peeks: match peek {
            true => [0].into_iter().collect(),
            false => Default::default(),
        },
        times_repeated: Default::default(),
    }
}

fn comm_identity(event: &COMM) -> [u8; 32] {
    event
        .cost_identity()
        .0
        .as_slice()
        .try_into()
        .expect("a COMM identity has 32 bytes")
}

/// R7, SA6: a COMM whose owner cannot pay records a refusal of its source at
/// the position of the next attempt. It records no attempt and no frontier
/// entry, and it leaves the COMM stage open, so the operation forms another
/// COMM. The journal check accepts the row.
#[test]
fn comm_refusal_records_position_and_reopens_the_comm_stage() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let payer = owner(1);
    let binding = binding(&budget, &[(payer.clone(), 4)], 4);
    start_run(&budget, &binding, 100);
    let _accounting = budget.enter_comm_accounting_scope();
    let channel = channel(1);
    let source = Produce::create(&channel, &7u8, false);
    let (refused, granted) = (
        comm_event(&channel, 1, &source, false),
        comm_event(&channel, 2, &source, false),
    );
    scope(&budget, 0, || {
        let reference = RSpaceOperationSource::Produce(&source);
        let name = start_row(&budget, reference, std::slice::from_ref(&channel));
        operation_context::sync_scope(name, || {
            budget
                .reserve_produce_introduction_measured(
                    produce_introduction_identity(&source),
                    &authority(1),
                    transfer(1),
                    false,
                )
                .expect("a price-0 introduction draws nothing");
            budget
                .observe_native_comm_source(&refused)
                .expect("the first COMM source is observed");
            assert!(matches!(
                budget.reserve_comm_authority_measured(
                    comm_identity(&refused),
                    &owned(&payer, b"refused"),
                    transfer(2),
                ),
                Err(InterpreterError::RSpaceError(RSpaceError::CommRefused))
            ));
            assert!(budget.authority_frontier().is_empty());
            budget
                .observe_native_comm_source(&granted)
                .expect("the COMM stage is open again");
            budget
                .reserve_comm_authority_measured(
                    comm_identity(&granted),
                    &owned(&payer, b"granted"),
                    transfer(1),
                )
                .expect("the second COMM draws 3 of 4");
            budget.finish_native_operation(reference, RSpaceOperationCompletion::Matched);
        });
    });
    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    assert_eq!(recording.attempts.len(), 2);
    let rows = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    let row = &rows[0];
    assert_eq!(row.refusals.len(), 1);
    assert_eq!(row.refusals[0].position, 1);
    assert!(row.refusals[0].source.matches(&refused));
    assert!(row
        .comm
        .as_ref()
        .is_some_and(|comm| comm.source.matches(&granted)
            && comm.observation == NativeObservationLink::Attempt(1)));
    assert_eq!(row.completion, RSpaceOperationCompletion::Matched);
    with_priced_contract(100, [0, 0, 1, 0], OWNER_PRICE, |contract| {
        contract
            .check_operation_journal_with_owners(
                recording.session,
                &recording,
                Arc::clone(&rows),
                journal_limits(),
                Some(&binding),
            )
            .expect("the journal check accepts the refusal");
    });
}

// Changed by G3 (DR-135, G-4): the COMM reserves the restoration, and the
// granted restoration bills its reservation.
// /// R9: a granted peek COMM pays for the restorations of its data, and the
// /// introductions of the restorations draw no owner budget again. The fold
// /// finds each restoration by the identity of its produce hash and charges
// /// it at the COMM, so its ledger equals the ledger of play.
/// R9, G-4: a granted peek COMM reserves the restorations of its data. It
/// draws their shares from the remaining budgets and bills only its own
/// shares. The granted introduction of a restoration bills its reservation
/// and draws no owner budget again. The fold finds each restoration by the
/// identity of its produce hash, so its ledger equals the ledger of play.
#[test]
fn peek_comm_reserves_restorations_and_the_restoration_consumes_them() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let (reader, writer) = (owner(1), owner(2));
    let binding = binding(&budget, &[(reader.clone(), 100), (writer.clone(), 100)], 4);
    start_run(&budget, &binding, 100);
    let _accounting = budget.enter_comm_accounting_scope();
    let channel = channel(1);
    let datum = Produce::create(&channel, &7u8, false);
    produce_operation(&budget, 0, &datum, &channel, &writer, 1).expect("the datum is introduced");
    let event = comm_event(&channel, 1, &datum, true);
    let restoration = Arc::new(ByteObservation {
        event_id: produce_introduction_identity(&datum),
        kind: authority::AuthorityByteEventKind::ProduceIntroduction,
        authority: canonical_regions(&owned(&writer, &[2]).regions)
            .expect("a canonical restoration authority")
            .into_authority(),
        measurement: Some(transfer(1)),
        legacy_amount: None,
    });
    scope(&budget, 1, || {
        let reference = RSpaceOperationSource::Consume(&event.consume);
        let name = start_row(&budget, reference, std::slice::from_ref(&channel));
        operation_context::sync_scope(name, || {
            budget
                .observe_native_consume_peeks(&[0].into_iter().collect())
                .expect("the consume observes its peeks");
            budget
                .reserve_consume_introduction_measured(
                    consume_introduction_identity(&event.consume),
                    &owned(&reader, &[1]),
                    transfer(1),
                    false,
                )
                .expect("the reader pays its introduction");
            budget
                .observe_native_comm_source(&event)
                .expect("the COMM source is observed");
            let witness = canonical_regions(
                owned(&reader, b"comm-reader")
                    .regions
                    .iter()
                    .chain(owned(&writer, b"comm-writer").regions.iter()),
            )
            .expect("a canonical COMM authority");
            budget
                .reserve_comm_authority_owned(
                    comm_identity(&event),
                    witness,
                    transfer(1),
                    OwnerCommExtras {
                        transferee: None,
                        restorations: vec![Arc::clone(&restoration)],
                    },
                )
                .expect("the owners pay the COMM and the restoration");
            budget.finish_native_operation(reference, RSpaceOperationCompletion::Matched);
        });
    });
    let after_comm = budget.native_owner_ledger().expect("an owner ledger");
    produce_operation(&budget, 2, &datum, &channel, &writer, 1)
        .expect("the restoration is introduced");
    let after_restoration = budget.native_owner_ledger().expect("an owner ledger");
    // Changed by G3 (DR-135, G-4): the COMM draws the restoration from the
    // remaining budget but bills only its own shares, and the granted
    // restoration bills its reservation.
    // assert_eq!(
    //     after_restoration.debits(),
    //     after_comm.debits(),
    //     "the restoration draws nothing again"
    // );
    // // The writer paid its datum, its COMM share and the restoration.
    // assert_eq!(after_comm.debits().get(&custody(&writer)), 3 + 3 + 3);
    // assert_eq!(after_comm.debits().get(&custody(&reader)), 3 + 3);
    for signature in [&reader, &writer] {
        assert_eq!(
            after_restoration.remaining(&custody(signature)),
            after_comm.remaining(&custody(signature)),
            "the restoration draws nothing again"
        );
    }
    // The budget of the writer paid its datum, its COMM share and the
    // reserved restoration.
    assert_eq!(after_comm.remaining(&custody(&writer)), Some(100 - 9));
    assert_eq!(after_comm.remaining(&custody(&reader)), Some(100 - 6));
    // The COMM bills the datum and the COMM share of the writer only.
    assert_eq!(after_comm.debits().get(&custody(&writer)), 3 + 3);
    assert_eq!(after_comm.debits().get(&custody(&reader)), 3 + 3);
    assert_eq!(
        (after_comm.usage(), after_comm.rev()),
        (4, 4 * OWNER_PRICE as u128)
    );
    // The granted restoration bills its reservation.
    assert_eq!(after_restoration.debits().get(&custody(&writer)), 3 + 3 + 3);
    assert_eq!(after_restoration.debits().get(&custody(&reader)), 3 + 3);
    assert_eq!(
        (after_restoration.usage(), after_restoration.rev()),
        (5, 5 * OWNER_PRICE as u128)
    );
    // Every region of the run is an owner region, so the owners bill the
    // whole usage of the run.
    assert_eq!(budget.native_phlo_usage(), Some(after_restoration.usage()));

    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    let rows = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    with_priced_contract(100, [0, 0, 1, 0], OWNER_PRICE, |contract| {
        contract
            .check_operation_journal_with_owners(
                recording.session,
                &recording,
                Arc::clone(&rows),
                journal_limits(),
                Some(&binding),
            )
            .expect("the journal check folds the restoration at its COMM");
        let evidence = contract
            .check_budget_evidence(
                recording.session,
                Arc::clone(&recording.attempts),
                Arc::clone(&recording.transfers),
                journal_limits().budget,
                // Changed by G3 (DR-135, S4 review): the fold reads the
                // restorations from the COMM attempt.
                // Some(OwnerFoldInput {
                //     binding: &binding,
                //     table: binding.table().clone(),
                //     restorations: [(2, vec![3])].into_iter().collect(),
                // }),
                Some(OwnerFoldInput {
                    binding: &binding,
                    table: binding.table().clone(),
                }),
            )
            .expect("the fold accepts the run");
        let fold = evidence.owner().expect("an owner fold");
        assert_eq!(fold.ledger().debits(), after_restoration.debits());
        assert_eq!(fold.ledger().usage(), after_restoration.usage());
        assert_eq!(fold.ledger().rev(), after_restoration.rev());
        // Changed by G3 (DR-135, G-4): the COMM reserves the restoration.
        // // The writer pays the restoration at the COMM (attempt 2), so its
        // // budget before the restoration (attempt 3) is already 100 - 9.
        // The COMM (attempt 2) reserves the restoration, so the budget of the
        // writer before the restoration (attempt 3) is already 100 - 9, and
        // the restoration draws nothing again.
        assert_eq!(fold.remaining_at(&custody(&writer), 3), Some(100 - 9));
        // Added by G3 (DR-135, G-4).
        assert_eq!(fold.remaining_at(&custody(&writer), 4), Some(100 - 9));
        // Changed by G3 (DR-135, S4 review): the fold has no restoration map
        // any more, so the fold that charged the restoration at its own
        // attempt is gone. The COMM attempt records the restoration.
        // let late = contract
        //     .check_budget_evidence(
        //         recording.session,
        //         Arc::clone(&recording.attempts),
        //         journal_limits().budget,
        //         Some(OwnerFoldInput {
        //             binding: &binding,
        //             table: binding.table().clone(),
        //             restorations: BTreeMap::new(),
        //         }),
        //     )
        //     .expect("a fold without the restoration also accepts the run");
        // assert_eq!(
        //     late.owner()
        //         .expect("an owner fold")
        //         .remaining_at(&custody(&writer), 3),
        //     Some(100 - 6),
        //     "without the map, the restoration is charged at its own attempt"
        // );
        assert_eq!(
            recording.attempts[2]
                .restorations
                .iter()
                .map(|restoration| restoration.event_id)
                .collect::<Vec<_>>(),
            vec![produce_introduction_identity(&datum)],
            "the COMM attempt records its restoration"
        );
    });
}

// Changed by G3 (DR-135, G-4): the COMM reserves the restoration.
// /// Added by G3 (DR-135, S4 review): one peek consume on `channel` that
// /// matches `datum`. The reader pays its introduction, the reader and the
// /// writer share the COMM, and the COMM pays for the restoration of the datum
// /// of the writer (R9). It returns the result of the COMM.
/// Added by G3 (DR-135, S4 review): one peek consume on `channel` that
/// matches `datum`. The reader pays its introduction, the reader and the
/// writer share the COMM, and the COMM reserves the restoration of the datum
/// of the writer (R9, G-4). It returns the result of the COMM.
// Changed by G3 (DR-135, S11): the body moved to `peek_operation_restoring`,
// which takes the authority of the restoration.
fn peek_operation(
    budget: &RuntimeBudget,
    index: usize,
    channel: &Par,
    datum: &Produce,
    reader: &CostSignature,
    writer: &CostSignature,
) -> Result<(), InterpreterError> {
    peek_operation_restoring(
        budget,
        index,
        channel,
        datum,
        reader,
        writer,
        &owned(writer, &[2]),
    )
}

/// Added by G3 (DR-135, S11): `peek_operation` whose restoration has the
/// authority `restored`.
fn peek_operation_restoring(
    budget: &RuntimeBudget,
    index: usize,
    channel: &Par,
    datum: &Produce,
    reader: &CostSignature,
    writer: &CostSignature,
    restored: &CostAuthority,
) -> Result<(), InterpreterError> {
    let event = comm_event(channel, 1, datum, true);
    // Changed by G3 (DR-135, S11): the caller gives the restored authority.
    // let restoration = Arc::new(ByteObservation {
    //     event_id: produce_introduction_identity(datum),
    //     kind: authority::AuthorityByteEventKind::ProduceIntroduction,
    //     authority: canonical_regions(&owned(writer, &[2]).regions)
    //         .expect("a canonical restoration authority")
    //         .into_authority(),
    //     measurement: Some(transfer(1)),
    //     legacy_amount: None,
    // });
    let restoration = Arc::new(ByteObservation {
        event_id: produce_introduction_identity(datum),
        kind: authority::AuthorityByteEventKind::ProduceIntroduction,
        authority: canonical_regions(&restored.regions)
            .expect("a canonical restoration authority")
            .into_authority(),
        measurement: Some(transfer(1)),
        legacy_amount: None,
    });
    scope(budget, index, || {
        let reference = RSpaceOperationSource::Consume(&event.consume);
        let name = start_row(budget, reference, std::slice::from_ref(channel));
        operation_context::sync_scope(name, || {
            budget
                .observe_native_consume_peeks(&[0].into_iter().collect())
                .expect("the consume observes its peeks");
            budget
                .reserve_consume_introduction_measured(
                    consume_introduction_identity(&event.consume),
                    &owned(reader, &[1]),
                    transfer(1),
                    false,
                )
                .expect("the reader pays its introduction");
            budget
                .observe_native_comm_source(&event)
                .expect("the COMM source is observed");
            let witness = canonical_regions(
                owned(reader, b"comm-reader")
                    .regions
                    .iter()
                    .chain(owned(writer, b"comm-writer").regions.iter()),
            )
            .expect("a canonical COMM authority");
            let result = budget.reserve_comm_authority_owned(
                comm_identity(&event),
                witness,
                transfer(1),
                OwnerCommExtras {
                    transferee: None,
                    restorations: vec![restoration],
                },
            );
            let completion = match result {
                Ok(()) => RSpaceOperationCompletion::Matched,
                Err(_) => RSpaceOperationCompletion::Rejected,
            };
            budget.finish_native_operation(reference, completion);
            result
        })
    })
}

// Changed by G3 (DR-135, G-4): an unrun restoration stays reserved and
// unbilled.
// /// Added by G3 (DR-135, S4 review): §6.3 item 4. A granted peek COMM pays for
// /// its restoration at the COMM, in play and in the fold, and never again. A
// /// restoration that never runs (for example after a user failure between
// /// the COMM and the restoration) therefore leaves play and the fold equal:
// /// a later refusal that the restoration's draw caused folds, and the
// /// remaining budget of play equals `remaining_at` of the fold.
// #[test]
// fn an_unrun_restoration_stays_paid_at_its_comm_and_a_later_refusal_folds() {
/// Added by G3 (DR-135, S4 review), changed by G-4: §6.3 item 4. A granted
/// peek COMM reserves its restoration at the COMM, in play and in the fold:
/// it draws the share of the restoration from the remaining budget, and only
/// the granted introduction of the restoration bills it. A restoration that
/// never runs (for example after a user failure between the COMM and the
/// restoration) stays reserved and unbilled. A later refusal that the
/// reservation caused folds, play and the fold keep equal remaining budgets
/// and equal billed sums, and the owners bill only the metered usage.
#[test]
fn an_unrun_restoration_stays_reserved_unbilled_and_a_later_refusal_folds() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let (reader, writer) = (owner(1), owner(2));
    let binding = binding(&budget, &[(reader.clone(), 100), (writer.clone(), 9)], 4);
    start_run(&budget, &binding, 100);
    let _accounting = budget.enter_comm_accounting_scope();
    let (first, second) = (channel(1), channel(2));
    let datum = Produce::create(&first, &7u8, false);
    produce_operation(&budget, 0, &datum, &first, &writer, 1).expect("the writer pays 3 of 9");
    // Changed by G3 (DR-135, G-4): the COMM reserves the restoration.
    // peek_operation(&budget, 1, &first, &datum, &reader, &writer)
    //     .expect("the writer pays 3 for the COMM and 3 for the restoration");
    peek_operation(&budget, 1, &first, &datum, &reader, &writer)
        .expect("the writer pays 3 for the COMM and reserves 3 for the restoration");
    let later = Produce::create(&second, &8u8, false);
    assert!(matches!(
        produce_operation(&budget, 2, &later, &second, &writer, 1),
        Err(InterpreterError::RSpaceError(
            RSpaceError::IntroductionRefused
        ))
    ));
    let play = budget.native_owner_ledger().expect("an owner ledger");
    assert_eq!(play.remaining(&custody(&writer)), Some(0));
    // Added by G3 (DR-135, G-4): the writer is billed for its datum and its
    // COMM share only, and the owners bill exactly the metered usage, because
    // every region of the run is an owner region.
    assert_eq!(play.debits().get(&custody(&writer)), 3 + 3);
    assert_eq!(play.debits().get(&custody(&reader)), 3 + 3);
    assert_eq!((play.usage(), play.rev()), (4, 4 * OWNER_PRICE as u128));
    assert_eq!(budget.native_phlo_usage(), Some(play.usage()));
    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    let rows = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    with_priced_contract(100, [0, 0, 1, 0], OWNER_PRICE, |contract| {
        let checked = contract
            .check_operation_journal_with_owners(
                recording.session,
                &recording,
                Arc::clone(&rows),
                journal_limits(),
                Some(&binding),
            )
            .expect("the fold accepts the refusal after the unrun restoration");
        let fold = checked.budget_evidence().owner().expect("an owner fold");
        for signature in [&reader, &writer] {
            assert_eq!(
                fold.remaining_at(&custody(signature), recording.attempts.len()),
                play.remaining(&custody(signature))
            );
        }
        assert_eq!(fold.ledger().debits(), play.debits());
        // Added by G3 (DR-135, G-4): play and the fold bill equal sums.
        assert_eq!(fold.ledger().usage(), play.usage());
        assert_eq!(fold.ledger().rev(), play.rev());
    });
}

/// Added by G3 (DR-135, G-4): a peek COMM whose restoration the signed
/// limit of the client denies. The COMM reserves the restoration, and the
/// denial is the charged user failure `SignedLimitExhausted`. The
/// reservation stays drawn and unbilled: each owner is billed for its own
/// introduction and its COMM share only, the owners bill exactly the metered
/// usage, and the fold rebuilds the remaining budgets and the billed sums of
/// play.
#[test]
fn a_bound_denied_restoration_stays_unbilled_and_the_member_fails_charged() {
    use crate::rust::interpreter::accounting::economic_failure::classify_errors;
    use crate::rust::interpreter::accounting::phlo_execution::PhloFailure;

    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let (reader, writer) = (owner(1), owner(2));
    // Changed by G3 (DR-135, S11): the bound counts no owner share at a
    // price of 1 or more, so the datum has a mixed seal: a region of the
    // writer and a region of the client. The client part of the datum fills
    // the limit, so the client part of the restoration exceeds it (item 13).
    // The replaced lines follow.
    // let binding = binding(&budget, &[(reader.clone(), 100), (writer.clone(), 100)], 4);
    // // The datum, the consume and the two regions of the COMM use 1 + 1 + 2,
    // // the whole limit, so the restoration exceeds it.
    // let limit = 4;
    let binding = client_binding(&budget, &[(reader.clone(), 100), (writer.clone(), 100)], 4);
    let limit = 1;
    budget
        .reset_for_native_execution(
            priced_config_from(
                limit,
                [0, 0, 1, 0],
                OWNER_PRICE,
                NativeBoundSource::SignedLimit,
            )
            .with_owner_funding(Arc::clone(&binding)),
        )
        .expect("an idle budget starts a native run");
    let _accounting = budget.enter_comm_accounting_scope();
    let channel = channel(1);
    let datum = Produce::create(&channel, &7u8, false);
    // Changed by G3 (DR-135, S11): the datum and its restoration have the
    // mixed seal.
    // produce_operation(&budget, 0, &datum, &channel, &writer, 1).expect("the datum is introduced");
    // peek_operation(&budget, 1, &channel, &datum, &reader, &writer)
    //     .expect("the COMM reserves the restoration");
    produce_operation_with(&budget, 0, &datum, &channel, &mixed(&writer, &[0]), 1)
        .expect("the datum is introduced");
    peek_operation_restoring(
        &budget,
        1,
        &channel,
        &datum,
        &reader,
        &writer,
        &mixed(&writer, &[2]),
    )
    .expect("the COMM reserves the restoration");
    let after_comm = budget.native_owner_ledger().expect("an owner ledger");
    // let error = produce_operation(&budget, 2, &datum, &channel, &writer, 1)
    //     .expect_err("the signed limit denies the restoration");
    let error = produce_operation_with(&budget, 2, &datum, &channel, &mixed(&writer, &[2]), 1)
        .expect_err("the signed limit denies the restoration");
    assert!(matches!(error, InterpreterError::SignedLimitExhausted));
    let summary = classify_errors(std::slice::from_ref(&error)).expect("a classified failure");
    assert!(summary.contains(PhloFailure::User));
    assert!(summary.permits_retained_charge());

    let play = budget.native_owner_ledger().expect("an owner ledger");
    for signature in [&reader, &writer] {
        assert_eq!(
            play.remaining(&custody(signature)),
            after_comm.remaining(&custody(signature)),
            "the denial changes no budget"
        );
        assert_eq!(
            play.debits().get(&custody(signature)),
            3 + 3,
            "each owner is billed for its introduction and its COMM share only"
        );
    }
    assert_eq!(
        play.remaining(&custody(&writer)),
        Some(100 - 9),
        "the reservation stays drawn until the end of the run"
    );
    assert_eq!((play.usage(), play.rev()), (4, 4 * OWNER_PRICE as u128));
    // Changed by G3 (DR-135, S11): the total holds the client region of the
    // datum besides the billed owner usage.
    // assert_eq!(budget.native_phlo_usage(), Some(limit));
    // assert_eq!(budget.native_phlo_usage(), Some(play.usage()));
    assert_eq!(budget.native_phlo_usage(), Some(5));
    assert_eq!(budget.native_phlo_usage(), Some(play.usage() + limit));

    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    assert_eq!(
        recording
            .attempts
            .iter()
            .map(|attempt| (attempt.granted, attempt.owner_refused))
            .collect::<Vec<_>>(),
        vec![(true, false), (true, false), (true, false), (false, false)],
        "the restoration is a bound denial, not an owner refusal"
    );
    let rows = budget
        .native_operation_recording()
        .expect("the journal is complete")
        .expect("a native run journals");
    with_priced_contract_from(
        limit,
        limit,
        [0, 0, 1, 0],
        OWNER_PRICE,
        NativeBoundSource::SignedLimit,
        |contract| {
            let contract = contract.expect("a signed-limit bound equal to the limit");
            let checked = contract
                .check_operation_journal_with_owners(
                    recording.session,
                    &recording,
                    Arc::clone(&rows),
                    journal_limits(),
                    Some(&binding),
                )
                .expect("the journal check folds the denied restoration");
            // Changed by G3 (DR-135, S11): the total holds every granted
            // charge, and the client total is the limit.
            // assert_eq!(checked.total(), limit);
            assert_eq!(checked.total(), 5);
            assert_eq!(checked.budget_evidence().client_total(), limit);
            let fold = checked.budget_evidence().owner().expect("an owner fold");
            for signature in [&reader, &writer] {
                assert_eq!(
                    fold.remaining_at(&custody(signature), recording.attempts.len()),
                    play.remaining(&custody(signature))
                );
            }
            assert_eq!(fold.ledger().debits(), play.debits());
            assert_eq!(fold.ledger().usage(), play.usage());
            assert_eq!(fold.ledger().rev(), play.rev());
        },
    );
}

/// Added by G3 (DR-135, S11): whether `error` is the exhaustion of a bound,
/// not an owner refusal.
fn is_bound_denial(error: &InterpreterError) -> bool {
    !matches!(
        error,
        InterpreterError::RSpaceError(RSpaceError::IntroductionRefused)
    )
}

/// Added by G3 (DR-135, S11): item 13. The owner pays an introduction at a
/// price of 1 or more from its own budget, so the caller's limit does not
/// count it. Two introductions of 5 are granted under the limit 4, and the
/// fold accepts both grants.
#[test]
fn an_owner_introduction_beyond_the_signed_limit_is_granted() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let payer = owner(1);
    let binding = binding(&budget, &[(payer.clone(), 100)], 1);
    start_run(&budget, &binding, 4);
    let accounting = budget.enter_comm_accounting_scope();
    introduce(&budget, 0, &owned(&payer, &[0]), 5)
        .expect("the owner pays an introduction of 5 past the limit 4");
    introduce(&budget, 1, &owned(&payer, &[1]), 5).expect("and a second one");
    assert_eq!(budget.native_phlo_usage(), Some(10));
    let ledger = budget.native_owner_ledger().expect("an owner ledger");
    assert_eq!(ledger.debits().get(&custody(&payer)), 10 * OWNER_PRICE);
    assert_eq!(
        (ledger.usage(), ledger.rev()),
        (10, 10 * OWNER_PRICE as u128)
    );
    assert!(fold_accepts(&budget, &binding, 4));
    drop(accounting);
}

/// Added by G3 (DR-135, S11): item 13. The client region of a mixed
/// introduction still counts against the caller's limit. Under the limit
/// 4, a mixed introduction of 3 counts 3, and a second one exceeds the limit
/// and is denied, not owner-refused. The fold accepts both decisions.
#[test]
fn a_client_region_still_exhausts_the_signed_limit() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let payer = owner(1);
    let binding = client_binding(&budget, &[(payer.clone(), 100)], 1);
    start_run(&budget, &binding, 4);
    let accounting = budget.enter_comm_accounting_scope();
    introduce(&budget, 0, &mixed(&payer, &[0]), 3).expect("the client part 3 fits the limit 4");
    let error = introduce(&budget, 1, &mixed(&payer, &[1]), 3)
        .expect_err("the client part 6 exceeds the limit 4");
    assert!(is_bound_denial(&error), "{error:?}");
    assert_eq!(budget.native_phlo_usage(), Some(6));
    let recording = budget
        .native_budget_recording()
        .expect("the recording is complete")
        .expect("a native run records");
    assert_eq!(
        recording
            .attempts
            .iter()
            .map(|attempt| (attempt.granted, attempt.owner_refused))
            .collect::<Vec<_>>(),
        vec![(true, false), (false, false)]
    );
    assert!(fold_accepts(&budget, &binding, 4));
    drop(accounting);
}

/// Added by G3 (DR-135, S11): decision 2. An owner share at price 0 keeps
/// counting against the caller's limit, and an owner share at a price of 1
/// or more does not. A price-0 introduction of 4 fills the limit 4, so a
/// second price-0 introduction is denied. A priced introduction of 5 is
/// still granted. The owner bills the price-0 usage at no REV.
#[test]
fn a_price_zero_owner_share_counts_against_the_callers_limit() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    let payer = owner(1);
    let binding = binding(&budget, &[(payer.clone(), 100)], 1);
    start_run(&budget, &binding, 4);
    let accounting = budget.enter_comm_accounting_scope();
    introduce(&budget, 0, &owned_at(&payer, &[0], 0), 4)
        .expect("a price-0 owner share of 4 fills the limit 4");
    let error = introduce(&budget, 1, &owned_at(&payer, &[1], 0), 1)
        .expect_err("the limit counts the price-0 share");
    assert!(is_bound_denial(&error), "{error:?}");
    introduce(&budget, 2, &owned(&payer, &[2]), 5)
        .expect("the limit does not count a priced owner share");
    assert_eq!(budget.native_phlo_usage(), Some(9));
    let ledger = budget.native_owner_ledger().expect("an owner ledger");
    assert_eq!((ledger.usage(), ledger.rev()), (9, 5 * OWNER_PRICE as u128));
    assert_eq!(ledger.debits().get(&custody(&payer)), 5 * OWNER_PRICE);
    assert!(fold_accepts(&budget, &binding, 4));
    drop(accounting);
}

/// Added by G3 (DR-135, S11): one introduction: its kind (0 owner-only at
/// the owner price, 1 owner-only at price 0, 2 mixed, 3 client-only), the
/// owner index and the transfer bytes.
fn arb_kinds() -> impl Strategy<Value = Vec<(usize, usize, u64)>> {
    prop::collection::vec((0_usize..4, 0_usize..2, 0_u64..5), 1..20)
}

/// One introduction of a random owner: the owner index and the transfer
/// bytes.
fn arb_charges() -> impl Strategy<Value = Vec<(usize, u64)>> {
    prop::collection::vec((0_usize..3, 0_u64..4), 1..24)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// §6.3 item 1: concurrent introductions decide their owner part and
    /// their bound in one critical section, so the fold of the attempts in
    /// their recorded order reproduces every decision and the final ledger.
    #[test]
    fn concurrent_grants_fold_in_recorded_order(
        budgets in prop::collection::vec(0_u64..20, 3),
        charges in arb_charges(),
        workers in 2_usize..6,
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let owners: Vec<CostSignature> = (0..3).map(owner).collect();
        let listed: Vec<(CostSignature, u64)> =
            owners.iter().cloned().zip(budgets.iter().copied()).collect();
        let binding = binding(&budget, &listed, 3);
        start_run(&budget, &binding, 1_000);
        let accounting = budget.enter_comm_accounting_scope();
        let refused = std::sync::atomic::AtomicUsize::new(0);
        let barrier = std::sync::Barrier::new(workers);
        std::thread::scope(|threads| {
            for worker in 0..workers {
                let (budget, owners, charges, refused, barrier) =
                    (&budget, &owners, &charges, &refused, &barrier);
                threads.spawn(move || {
                    barrier.wait();
                    for (index, (owner, bytes)) in
                        charges.iter().enumerate().filter(|(index, _)| index % workers == worker)
                    {
                        let result = with_operation(budget, || {
                            budget.reserve_produce_introduction_measured(
                                [index as u8; 32],
                                &owned(&owners[*owner], &[index as u8]),
                                transfer(*bytes),
                                false,
                            )
                        });
                        if matches!(
                            result,
                            Err(InterpreterError::RSpaceError(RSpaceError::IntroductionRefused))
                        ) {
                            refused.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let recording = budget.native_budget_recording().unwrap().unwrap();
        prop_assert_eq!(
            recording.attempts.iter().filter(|attempt| attempt.owner_refused).count(),
            refused.load(std::sync::atomic::Ordering::Relaxed)
        );
        let play = budget.native_owner_ledger().expect("an owner ledger");
        with_priced_contract(1_000, [0, 0, 1, 0], OWNER_PRICE, |contract| {
            let evidence = contract
                .check_budget_evidence(
                    recording.session,
                    Arc::clone(&recording.attempts),
                    Arc::clone(&recording.transfers),
                    config(1_000, [0, 0, 1, 0]).limits,
                    Some(OwnerFoldInput {
                        binding: &binding,
                        table: binding.table().clone(),
                        // Changed by G3 (DR-135, S4 review): the fold keeps no restoration map.
                        // restorations: BTreeMap::new(),
                    }),
                )
                .expect("the fold reproduces every decision");
            let fold = evidence.owner().expect("an owner fold");
            assert_eq!(fold.ledger().debits(), play.debits());
            assert_eq!(fold.ledger().usage(), play.usage());
            assert_eq!(fold.ledger().rev(), play.rev());
            for signature in &owners {
                assert_eq!(
                    fold.remaining_at(&custody(signature), recording.attempts.len()),
                    play.remaining(&custody(signature))
                );
            }
        });
        drop(accounting);
    }

    /// §6.3 item 3: the budgets S_r that an operation captures at its start
    /// equal the budgets of the fold before the first attempt of the
    /// operation, `remaining_at(budget_start)`, also when operations run on
    /// several threads.
    #[test]
    fn snapshot_equals_fold_prefix(
        budgets in prop::collection::vec(0_u64..20, 3),
        charges in arb_charges(),
        workers in 1_usize..4,
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let owners: Vec<CostSignature> = (0..3).map(owner).collect();
        let listed: Vec<(CostSignature, u64)> =
            owners.iter().cloned().zip(budgets.iter().copied()).collect();
        let binding = binding(&budget, &listed, 3);
        start_run(&budget, &binding, 1_000);
        let accounting = budget.enter_comm_accounting_scope();
        std::thread::scope(|threads| {
            for worker in 0..workers {
                let (budget, owners, charges) = (&budget, &owners, &charges);
                threads.spawn(move || {
                    for (index, (owner, bytes)) in
                        charges.iter().enumerate().filter(|(index, _)| index % workers == worker)
                    {
                        let channel = channel(index as i64);
                        let source = Produce::create(&channel, &7u8, false);
                        let _ = produce_operation(
                            budget,
                            index,
                            &source,
                            &channel,
                            &owners[*owner],
                            *bytes,
                        );
                    }
                });
            }
        });
        let recording = budget.native_budget_recording().unwrap().unwrap();
        let rows = budget.native_operation_recording().unwrap().unwrap();
        let snapshots = budget.native_operation_snapshots();
        prop_assert_eq!(snapshots.len(), rows.len());
        with_priced_contract(1_000, [0, 0, 1, 0], OWNER_PRICE, |contract| {
            let evidence = contract
                .check_budget_evidence(
                    recording.session,
                    Arc::clone(&recording.attempts),
                    Arc::clone(&recording.transfers),
                    config(1_000, [0, 0, 1, 0]).limits,
                    Some(OwnerFoldInput {
                        binding: &binding,
                        table: binding.table().clone(),
                        // Changed by G3 (DR-135, S4 review): the fold keeps no restoration map.
                        // restorations: BTreeMap::new(),
                    }),
                )
                .expect("the fold reproduces every decision");
            let fold = evidence.owner().expect("an owner fold");
            for (row, snapshot) in rows.iter().zip(&snapshots) {
                let snapshot = snapshot.as_ref().expect("an owner run captures S_r");
                for signature in &owners {
                    let purse = custody(signature);
                    assert_eq!(
                        fold.remaining_at(&purse, row.budget_start),
                        Some(snapshot.get(&purse)),
                        "row starting at {}",
                        row.budget_start
                    );
                }
            }
            contract
                .check_operation_journal_with_owners(
                    recording.session,
                    &recording,
                    Arc::clone(&rows),
                    journal_limits(),
                    Some(&binding),
                )
                .expect("the journal check accepts the run");
        });
        drop(accounting);
    }

    /// A4: a persistent introduction charges its owner once. Each re-issue
    /// is a retry, which takes no owner share and is never refused, also
    /// when the owner has no budget left.
    #[test]
    fn persistent_retry_has_zero_share(bytes in 1_u64..5, reissues in 1_usize..6) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let payer = owner(1);
        let binding = binding(&budget, &[(payer.clone(), bytes * OWNER_PRICE)], 1);
        start_run(&budget, &binding, 1_000);
        let accounting = budget.enter_comm_accounting_scope();
        let sealed = owned(&payer, b"persistent");
        for _ in 0..=reissues {
            with_operation(&budget, || {
                budget.reserve_produce_introduction_measured(
                    [9; 32],
                    &sealed,
                    transfer(bytes),
                    true,
                )
            })
            .expect("a re-issue is a retry");
        }
        let recording = budget.native_budget_recording().unwrap().unwrap();
        prop_assert_eq!(recording.attempts.len(), 1);
        prop_assert_eq!(recording.retries.len(), reissues);
        let ledger = budget.native_owner_ledger().expect("an owner ledger");
        prop_assert_eq!(ledger.debits().get(&custody(&payer)), bytes * OWNER_PRICE);
        prop_assert_eq!(ledger.remaining(&custody(&payer)), Some(0));
        prop_assert!(fold_accepts(&budget, &binding, 1_000));
        drop(accounting);
    }

    /// Added by G3 (DR-135, S11): item 13 and decision 2. Play decides each
    /// introduction as an oracle does: the owner part first, then the client
    /// part against the limit. An owner share at a price of 1 or more is not
    /// in the client part, and a price-0 share is. The total is the client
    /// usage plus the priced owner usage, the fold reproduces both totals,
    /// and the client usage stays within the limit.
    #[test]
    fn the_client_bound_never_counts_a_priced_owner_share(
        budgets in prop::collection::vec(0_u64..30, 2),
        limit in 1_u64..12,
        kinds in arb_kinds(),
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let owners: Vec<CostSignature> = (0..2).map(owner).collect();
        let listed: Vec<(CostSignature, u64)> =
            owners.iter().cloned().zip(budgets.iter().copied()).collect();
        let binding = client_binding(&budget, &listed, 2);
        start_run(&budget, &binding, limit);
        let accounting = budget.enter_comm_accounting_scope();
        let mut remaining = budgets.clone();
        let (mut used, mut client_used, mut priced) = (0_u64, 0_u64, 0_u64);
        for (index, (kind, owner_index, bytes)) in kinds.iter().enumerate() {
            let entropy = [index as u8];
            let (authority, usage, client, draw) = match kind {
                0 => (owned(&owners[*owner_index], &entropy), *bytes, 0, *bytes * OWNER_PRICE),
                1 => (owned_at(&owners[*owner_index], &entropy, 0), *bytes, *bytes, 0),
                2 => (mixed(&owners[*owner_index], &entropy), 2 * *bytes, *bytes, *bytes * OWNER_PRICE),
                _ => (client_only(&entropy), *bytes, *bytes, 0),
            };
            let result = introduce(&budget, index, &authority, *bytes);
            if *kind != 3 && draw > remaining[*owner_index] {
                prop_assert!(
                    matches!(
                        result,
                        Err(InterpreterError::RSpaceError(RSpaceError::IntroductionRefused))
                    ),
                    "introduction {index} is owner-refused"
                );
            } else if client_used + client <= limit {
                prop_assert!(result.is_ok(), "introduction {index} is granted");
                used += usage;
                client_used += client;
                if *kind == 0 || *kind == 2 {
                    priced += *bytes;
                }
                if *kind != 3 {
                    remaining[*owner_index] -= draw;
                }
            } else {
                prop_assert!(
                    result.as_ref().is_err_and(is_bound_denial),
                    "introduction {index} is a bound denial"
                );
            }
        }
        prop_assert_eq!(budget.native_phlo_usage(), Some(used));
        prop_assert_eq!(used, client_used + priced);
        prop_assert!(client_used <= limit);
        let recording = budget
            .native_budget_recording()
            .expect("the recording is complete")
            .expect("a native run records");
        with_priced_contract(limit, [0, 0, 1, 0], OWNER_PRICE, |contract| {
            let evidence = contract
                .check_budget_evidence(
                    recording.session,
                    Arc::clone(&recording.attempts),
                    Arc::clone(&recording.transfers),
                    config(limit, [0, 0, 1, 0]).limits,
                    Some(OwnerFoldInput {
                        binding: &binding,
                        table: binding.table().clone(),
                    }),
                )
                .expect("the fold reproduces every decision");
            assert_eq!(evidence.total(), used);
            assert_eq!(evidence.client_total(), client_used);
        });
        drop(accounting);
    }

    /// Added by G3 (DR-135, S11): item 13 and decision 2. Whatever the
    /// introductions, the client usage stays within the limit, the price-0
    /// owner usage counts against it, each owner pays at most its budget,
    /// and the total is the client usage plus the priced owner usage.
    #[test]
    fn total_work_stays_within_the_bound_and_the_owner_budgets(
        budgets in prop::collection::vec(0_u64..30, 2),
        limit in 1_u64..12,
        kinds in arb_kinds(),
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let owners: Vec<CostSignature> = (0..2).map(owner).collect();
        let listed: Vec<(CostSignature, u64)> =
            owners.iter().cloned().zip(budgets.iter().copied()).collect();
        let binding = client_binding(&budget, &listed, 2);
        start_run(&budget, &binding, limit);
        let accounting = budget.enter_comm_accounting_scope();
        for (index, (kind, owner_index, bytes)) in kinds.iter().enumerate() {
            let entropy = [index as u8];
            let authority = match kind {
                0 => owned(&owners[*owner_index], &entropy),
                1 => owned_at(&owners[*owner_index], &entropy, 0),
                2 => mixed(&owners[*owner_index], &entropy),
                _ => client_only(&entropy),
            };
            let _ = introduce(&budget, index, &authority, *bytes);
        }
        let ledger = budget.native_owner_ledger().expect("an owner ledger");
        for (signature, budgeted) in &listed {
            prop_assert!(ledger.debits().get(&custody(signature)) <= *budgeted);
        }
        let priced = u64::try_from(ledger.rev() / u128::from(OWNER_PRICE))
            .expect("a small priced usage");
        let free = ledger.usage() - priced;
        let recording = budget
            .native_budget_recording()
            .expect("the recording is complete")
            .expect("a native run records");
        let total = budget.native_phlo_usage().expect("a native run");
        with_priced_contract(limit, [0, 0, 1, 0], OWNER_PRICE, |contract| {
            let evidence = contract
                .check_budget_evidence(
                    recording.session,
                    Arc::clone(&recording.attempts),
                    Arc::clone(&recording.transfers),
                    config(limit, [0, 0, 1, 0]).limits,
                    Some(OwnerFoldInput {
                        binding: &binding,
                        table: binding.table().clone(),
                    }),
                )
                .expect("the fold accepts the run");
            let client = evidence.client_total();
            assert!(client <= limit, "the client usage stays within the limit");
            assert!(free <= client, "the price-0 owner usage counts against the limit");
            assert_eq!(total, client + priced);
        });
        drop(accounting);
    }

    /// SA8: play gives an unlisted purse budget 0. The fold gives it 0 for a
    /// grant and passes it for a refusal while the table has room. So the
    /// fold accepts a play recording exactly when the table is full or no
    /// refused introduction belongs to an unlisted purse.
    #[test]
    fn unlisted_rules_follow_sa8(
        budgets in prop::collection::vec(0_u64..20, 3),
        listed_count in 0_usize..4,
        full in any::<bool>(),
        charges in arb_charges(),
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let owners: Vec<CostSignature> = (0..3).map(owner).collect();
        let listed: Vec<(CostSignature, u64)> = owners
            .iter()
            .cloned()
            .zip(budgets.iter().copied())
            .take(listed_count)
            .collect();
        let room = match full {
            true => listed.len(),
            false => listed.len() + 1,
        };
        let binding = binding(&budget, &listed, room);
        start_run(&budget, &binding, 1_000);
        let accounting = budget.enter_comm_accounting_scope();
        let mut refused_unlisted = false;
        for (index, (owner, bytes)) in charges.iter().enumerate() {
            let result = with_operation(&budget, || {
                budget.reserve_produce_introduction_measured(
                    [index as u8; 32],
                    &owned(&owners[*owner], &[index as u8]),
                    transfer(*bytes),
                    false,
                )
            });
            if result.is_err() && *owner >= listed.len() {
                refused_unlisted = true;
            }
        }
        let noted: BTreeSet<[u8; 32]> =
            binding.take_notes().into_iter().map(|purse| purse.custody).collect();
        prop_assert!(full || !refused_unlisted || !noted.is_empty(), "play notes an unlisted purse");
        prop_assert!(!full || noted.is_empty(), "a full table notes nothing");
        prop_assert_eq!(fold_accepts(&budget, &binding, 1_000), full || !refused_unlisted);
        drop(accounting);
    }
}
