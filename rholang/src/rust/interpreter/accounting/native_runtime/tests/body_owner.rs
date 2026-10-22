//! Added by G3 (DR-135): a fired body runs in the context of its seal (plan
//! S3, rules R1, A1 and R11).
//!
//! With an owner binding, a fired owner body opens the regions of its owner
//! at the owner's price. A system body runs for its caller: its residue
//! records the caller, and a process that it holds runs for the caller. An
//! owner on a client lane, and a run without a binding, keep the behavior of
//! dev: the body runs for the running deployment at its price.
//!
//! Each scenario is a sequence of runs on one tuple space. Each run has its
//! own signer and price, so a stored region tells which context opened it.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostCaller, CostRegion, CostSignature};
use models::rust::utils::new_gstring_par;
// Added by G3 (DR-135, S4).
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
// Changed by G3 (DR-135, S4): the owners of these tests have purses.
// use crate::rust::interpreter::accounting::authority::{
//     is_unit_cost_signature, sig_to_cost_signature,
// };
// use crate::rust::interpreter::accounting::owner_funding::{
//     OwnerFundingBinding, OwnerPurseSet, OwnerTable,
// };
use crate::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, is_unit_cost_signature, sig_to_cost_signature,
};
// Added by G3 (DR-135, S4): the refusals of held work and of a stack.
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt,
};
// Changed by G3 (DR-135, S4): the purses of the owners (the old import is
// above).
use crate::rust::interpreter::accounting::owner_funding::{
    OwnerFundingBinding, OwnerPurse, OwnerPurseSet, OwnerTable, OwnerTableEntry,
};
// Added by G3 (DR-135, S4).
use crate::rust::interpreter::accounting::SignatureChannel;
use crate::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;
use crate::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use crate::rust::interpreter::test_utils::resources::create_runtimes;

/// The price of the run that stores an owner's process.
const STORE_PRICE: u64 = 7;
/// The price of the run that fires a stored process.
const FIRE_PRICE: u64 = 11;
/// The price of a later run that fires residue.
const LATER_PRICE: u64 = 13;

fn storer() -> Sig { Sig::Ground(vec![1]) }

fn firer() -> Sig { Sig::Ground(vec![2]) }

fn later_firer() -> Sig { Sig::Ground(vec![3]) }

/// The signature that the surface signature `[ name ]` gives.
fn named(name: &str) -> CostSignature {
    sig_to_cost_signature(&Sig::Ground(canon_ground(name))).expect("a ground signature")
}

fn signature_of(signer: &Sig) -> CostSignature {
    sig_to_cost_signature(signer).expect("a ground signature")
}

// Changed by G3 (DR-135, S4): S4 refuses the work of an owner that cannot
// pay, so the owners of these runs have purses with budgets. S3 located no
// purse.
// /// The purse locator of these tests. S3 locates no purse.
// fn no_purse(_: &CostSignature) -> Option<OwnerPurseSet> { None }
/// The purse locator of these tests: each signature is one purse, whose
/// custody is the lane of the signature.
fn test_purse(signature: &CostSignature) -> Option<OwnerPurseSet> {
    let custody = cost_signature_to_sig(signature).ok()?.lane_hash();
    Some(OwnerPurseSet::Single(OwnerPurse {
        custody,
        signature: signature.clone(),
    }))
}

/// Added by G3 (DR-135, S4): the budget of a funded owner of these tests.
const FUNDED: u64 = u64::MAX / 4;

/// Added by G3 (DR-135, S4): every signature that owns or holds work in
/// these tests.
fn test_owners() -> Vec<CostSignature> {
    let mut owners: Vec<CostSignature> =
        ["owner", "sender", "other_sender", "caller", "w", "client"]
            .into_iter()
            .map(named)
            .collect();
    owners.extend(
        [storer(), firer(), later_firer(), fourth()]
            .iter()
            .map(signature_of),
    );
    owners
}

/// Added by G3 (DR-135, S4): a binding whose client lanes are the lanes of
/// `client` and whose full table lists `budgets`.
fn funded_binding(client: &[Sig], budgets: &[(CostSignature, u64)]) -> Arc<OwnerFundingBinding> {
    let mut entries: Vec<([u8; 32], OwnerTableEntry)> = budgets
        .iter()
        .map(|(signature, budget)| {
            let custody = cost_signature_to_sig(signature)
                .expect("a canonical signature")
                .lane_hash();
            (custody, OwnerTableEntry {
                signature: signature.clone(),
                budget: *budget,
            })
        })
        .collect();
    entries.sort_by_key(|(custody, _)| *custody);
    let room = entries.len();
    Arc::new(OwnerFundingBinding::new(
        client.iter().map(Sig::lane_hash).collect(),
        test_purse,
        OwnerTable::new(entries, room).expect("distinct custody keys"),
    ))
}

// Changed by G3 (DR-135, S4): every owner of these tests can pay.
// /// A binding whose client lanes are the lanes of `client`.
// fn binding(client: &[Sig]) -> Arc<OwnerFundingBinding> {
//     Arc::new(OwnerFundingBinding::new(
//         client.iter().map(Sig::lane_hash).collect(),
//         no_purse,
//         OwnerTable::empty(0),
//     ))
// }
/// A binding whose client lanes are the lanes of `client`. Every owner of
/// these tests has the budget `FUNDED`.
fn binding(client: &[Sig]) -> Arc<OwnerFundingBinding> {
    let budgets: Vec<(CostSignature, u64)> = test_owners()
        .into_iter()
        .map(|signature| (signature, FUNDED))
        .collect();
    funded_binding(client, &budgets)
}

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager.r_space_stores().await.expect("in-memory stores");
    let (runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    runtime
}

/// Plays `term` natively for `signer` at `price`, with `binding` when it is
/// set.
async fn play(
    runtime: &mut RhoRuntimeImpl,
    deploy: &[u8],
    signer: Sig,
    price: u64,
    binding: Option<Arc<OwnerFundingBinding>>,
    term: &str,
) {
    runtime.cost.set_deploy_signature_funded(deploy, signer);
    let config = priced_config(1_000_000, [1; 4], price);
    let config = match binding {
        Some(binding) => config.with_owner_funding(binding),
        None => config,
    };
    let evaluation = runtime
        .evaluate_with_native_phlo(
            term,
            HashMap::new(),
            Blake2b512Random::create_from_bytes(deploy),
            None,
            config,
        )
        .await
        .expect("the native evaluation runs");
    assert!(
        evaluation.errors.is_empty(),
        "{}: {:?}",
        String::from_utf8_lossy(deploy),
        evaluation.errors
    );
}

/// Evaluates `term` as system work, as genesis does: without a native
/// configuration and for the Unit signature. Its unsigned surfaces open Unit
/// regions, so its stored processes carry system seals (DR-101).
async fn evaluate_as_system(runtime: &mut RhoRuntimeImpl, deploy: &[u8], term: &str) {
    runtime.cost.set_deploy_signature_funded(deploy, Sig::Unit);
    let evaluation = runtime
        .evaluate(
            term,
            Cost::unsafe_max(),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(deploy),
        )
        .await
        .expect("the system evaluation runs");
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
}

fn channel(name: &str) -> models::rhoapi::Par {
    new_gstring_par(name.to_string(), Vec::new(), false)
}

/// The seal regions of the one datum on `name`.
async fn datum_seal(runtime: &RhoRuntimeImpl, name: &str) -> Vec<CostRegion> {
    let data = runtime.get_data(&channel(name)).await;
    assert_eq!(data.len(), 1, "one datum stays on {name}");
    data[0]
        .a
        .cost_authority
        .clone()
        .expect("the stored datum has a seal")
        .regions
}

/// The seal regions of the one continuation on `name`.
async fn continuation_seal(runtime: &RhoRuntimeImpl, name: &str) -> Vec<CostRegion> {
    let continuations = runtime.get_continuations(vec![channel(name)]).await;
    assert_eq!(continuations.len(), 1, "one continuation waits on {name}");
    continuations[0]
        .continuation
        .cost_authority
        .clone()
        .expect("the stored continuation has a seal")
        .regions
}

/// The signature and price of each region.
fn openers(regions: &[CostRegion]) -> Vec<(CostSignature, u64)> {
    regions
        .iter()
        .map(|region| {
            (
                region.signature.clone().expect("a region signature"),
                region.price,
            )
        })
        .collect()
}

fn is_system(regions: &[CostRegion]) -> bool {
    !regions.is_empty()
        && regions.iter().all(|region| {
            is_unit_cost_signature(region.signature.as_ref().expect("a region signature"))
        })
}

/// The binding of a run of `runner`: none when `extra` is none, else a
/// binding whose client lanes are the lanes of `runner` and of `extra`.
fn run_binding(runner: &Sig, extra: Option<&[Sig]>) -> Option<Arc<OwnerFundingBinding>> {
    extra.map(|extra| {
        let mut client = Vec::with_capacity(1 + extra.len());
        client.push(runner.clone());
        client.extend_from_slice(extra);
        binding(&client)
    })
}

/// Two stored bodies that `SIGNER` seals. The first receive has a signed
/// bind, so its body is unsigned. The second receive is signed, so uniform
/// signing signs its body too.
const STORED_BODIES: &str = r#"
for ({% @n <- @"g3-bind-in" %}[ SIGNER ]) { @"g3-bind-out"!(n) } |
{% for (@n <- @"g3-signed-in") { @"g3-signed-out"!(n) } %}[ SIGNER ]
"#;

/// What the two fired bodies of `STORED_BODIES` store: the signature and
/// price of the region of each datum that a body sends.
struct OwnerBodyOutcome {
    /// The unsigned send of the body of the signed bind. It opens a region
    /// of the context in which the body runs.
    bind_body: Vec<(CostSignature, u64)>,
    /// The signed send of the body of the signed receive. Its region has the
    /// signature of the term and the price of the context.
    signed_body: Vec<(CostSignature, u64)>,
}

/// `STORED_BODIES` for `name` are stored at `STORE_PRICE` and fired by
/// another signer at `FIRE_PRICE`, with the binding of `extra`.
async fn owner_body_outcome(name: &str, extra: Option<&[Sig]>) -> OwnerBodyOutcome {
    let mut runtime = fresh_runtime().await;
    play(
        &mut runtime,
        b"g3-owner-store",
        storer(),
        STORE_PRICE,
        None,
        &STORED_BODIES.replace("SIGNER", name),
    )
    .await;
    for name_in in ["g3-bind-in", "g3-signed-in"] {
        assert_eq!(
            openers(&continuation_seal(&runtime, name_in).await),
            vec![(named(name), STORE_PRICE)],
            "the body on {name_in} is sealed by {name} at the storing price"
        );
    }
    play(
        &mut runtime,
        b"g3-owner-fire",
        firer(),
        FIRE_PRICE,
        run_binding(&firer(), extra),
        r#"@"g3-bind-in"!(5) | @"g3-signed-in"!(6)"#,
    )
    .await;
    OwnerBodyOutcome {
        bind_body: openers(&datum_seal(&runtime, "g3-bind-out").await),
        signed_body: openers(&datum_seal(&runtime, "g3-signed-out").await),
    }
}

/// What a scenario with a system body observes.
struct SystemBodyOutcome {
    /// The regions of the datum that a held process stored in the run that
    /// fired the system body.
    held_now: Vec<(CostSignature, u64)>,
    /// The seal of the residue continuation that the system body stored.
    residue: Vec<CostRegion>,
    /// The regions of the datum that a held process stored when a later run
    /// fired the residue.
    held_later: Vec<(CostSignature, u64)>,
}

/// A system contract fires for `name` in one run, and its residue fires in
/// a later run of a third signer. Each run has the binding of `extra`.
///
/// - The system work stores the contract with a system seal.
/// - The firing run sends a process to the contract under the signature
///   `name`. No region records a caller yet, so the single owner of the data
///   seals is the caller. The body runs the process that it holds, and it
///   stores residue that waits for a second process.
/// - The later run sends the second process to the residue. The residue
///   records the caller, so the held second process runs for the caller.
async fn system_body_outcome(name: &str, extra: Option<&[Sig]>) -> SystemBodyOutcome {
    let mut runtime = fresh_runtime().await;
    evaluate_as_system(
        &mut runtime,
        b"g3-system-store",
        r#"for (p <= @"g3-system") { *p | for (q <- @"g3-system-later") { *q } }"#,
    )
    .await;
    assert!(
        is_system(&continuation_seal(&runtime, "g3-system").await),
        "the contract carries a system seal"
    );
    play(
        &mut runtime,
        b"g3-system-fire",
        firer(),
        FIRE_PRICE,
        run_binding(&firer(), extra),
        &r#"{% @"g3-system"!({ @"g3-held-now"!(2) }) %}[ SIGNER ]"#.replace("SIGNER", name),
    )
    .await;
    let held_now = openers(&datum_seal(&runtime, "g3-held-now").await);
    let residue = continuation_seal(&runtime, "g3-system-later").await;
    assert!(is_system(&residue), "the residue carries a system seal");
    play(
        &mut runtime,
        b"g3-system-later",
        later_firer(),
        LATER_PRICE,
        run_binding(&later_firer(), extra),
        r#"@"g3-system-later"!({ @"g3-held-later"!(3) })"#,
    )
    .await;
    let held_later = openers(&datum_seal(&runtime, "g3-held-later").await);
    SystemBodyOutcome {
        held_now,
        residue,
        held_later,
    }
}

/// R1: a fired owner body runs for its owner, at the price that the owner's
/// stored seal recorded, and not for the running deployment at its price.
/// An unsigned surface of the body opens a region of the owner, and a
/// signed surface takes the owner's price.
#[tokio::test]
async fn a_fired_body_opens_regions_of_its_owner() {
    let outcome = owner_body_outcome("owner", Some(&[])).await;
    let owner = vec![(named("owner"), STORE_PRICE)];
    assert_eq!(
        outcome.bind_body, owner,
        "an unsigned surface opens a region of the owner"
    );
    assert_eq!(
        outcome.signed_body, owner,
        "a signed surface takes the price of the owner"
    );
}

/// A1: a system body runs for its caller. A process that it holds runs for
/// the caller, and its residue records the caller.
///
/// Changed by G3 (DR-135, S3b T8): a later run that fires the residue sends
/// the second process itself. K then holds the recorded caller `owner` and
/// the holder `later_firer` of the second process, so the residue body has
/// no caller, and the second process runs for its own sender `later_firer`
/// at LATER_PRICE. S3 charged `owner` for the process of `later_firer`, and
/// Rule H fixes this.
#[tokio::test]
async fn a_held_process_in_a_system_body_runs_for_its_recorded_caller() {
    let outcome = system_body_outcome("owner", Some(&[])).await;
    let caller = (named("owner"), FIRE_PRICE);
    assert_eq!(outcome.held_now, vec![caller.clone()]);
    assert!(outcome.residue.iter().all(|region| region.caller
        == Some(CostCaller {
            signature: Some(caller.0.clone()),
            price: caller.1,
        })));
    // assert_eq!(outcome.held_later, vec![caller]);
    assert_eq!(outcome.held_later, vec![(
        signature_of(&later_firer()),
        LATER_PRICE
    )]);
}

/// An owner on a client lane is no owner: the running deployment pays for
/// its own lanes, as on dev. The same holds for the caller of a system
/// body, so its residue records no caller.
#[tokio::test]
async fn a_client_lane_owner_runs_for_the_running_deployment() {
    let client = [Sig::Ground(canon_ground("client"))];
    let outcome = owner_body_outcome("client", Some(&client)).await;
    assert_eq!(
        outcome.bind_body,
        vec![(signature_of(&firer()), FIRE_PRICE)],
        "an unsigned surface opens a region of the running deployment"
    );
    assert_eq!(
        outcome.signed_body,
        vec![(named("client"), FIRE_PRICE)],
        "a signed surface takes the price of the running deployment"
    );
    let outcome = system_body_outcome("client", Some(&client)).await;
    assert_eq!(outcome.held_now, vec![(signature_of(&firer()), FIRE_PRICE)]);
    // Changed by G3 (DR-135, S3b T8): H6 keeps a caller on a client lane, so
    // the residue records the client caller. Its held process still runs for
    // the running deployment (T1).
    // assert!(outcome.residue.iter().all(|region| region.caller.is_none()));
    assert!(outcome.residue.iter().all(|region| region.caller
        == Some(CostCaller {
            signature: Some(named("client")),
            price: FIRE_PRICE,
        })));
    assert_eq!(outcome.held_later, vec![(
        signature_of(&later_firer()),
        LATER_PRICE
    )]);
}

/// R11: without a binding every fired body runs as on dev. An owner body
/// runs for the running deployment at its price, a held process runs
/// outside the system seal for the running deployment, and residue records
/// no caller.
#[tokio::test]
async fn no_binding_keeps_dev_dispatch() {
    let outcome = owner_body_outcome("owner", None).await;
    assert_eq!(outcome.bind_body, vec![(
        signature_of(&firer()),
        FIRE_PRICE
    )]);
    assert_eq!(outcome.signed_body, vec![(named("owner"), FIRE_PRICE)]);
    let outcome = system_body_outcome("owner", None).await;
    assert_eq!(outcome.held_now, vec![(signature_of(&firer()), FIRE_PRICE)]);
    assert!(outcome.residue.iter().all(|region| region.caller.is_none()));
    assert_eq!(outcome.held_later, vec![(
        signature_of(&later_firer()),
        LATER_PRICE
    )]);
}

// ---- Added by G3 (DR-135, S3b): Rule H, a held process runs for the sender
// of its value (design §13). Each scenario stores an owner body or a system
// contract, fires it with a value of a known sender, and reads the region of
// the datum that the held code sends. The sender `sender` signs its sends, so
// its regions never sit on a client lane.

/// Added by G3 (DR-135, S3b): the signer and price of a fourth run.
fn fourth() -> Sig { Sig::Ground(vec![4]) }

/// Added by G3 (DR-135, S3b).
const FOURTH_PRICE: u64 = 17;

/// Added by G3 (DR-135, S3b): `play`, keeping the evaluation for its
/// observations.
async fn play_observed(
    runtime: &mut RhoRuntimeImpl,
    deploy: &[u8],
    signer: Sig,
    price: u64,
    binding: Option<Arc<OwnerFundingBinding>>,
    term: &str,
) -> crate::rust::interpreter::interpreter::EvaluateResult {
    runtime.cost.set_deploy_signature_funded(deploy, signer);
    let config = priced_config(1_000_000, [1; 4], price);
    let config = match binding {
        Some(binding) => config.with_owner_funding(binding),
        None => config,
    };
    let evaluation = runtime
        .evaluate_with_native_phlo(
            term,
            HashMap::new(),
            Blake2b512Random::create_from_bytes(deploy),
            None,
            config,
        )
        .await
        .expect("the native evaluation runs");
    assert!(
        evaluation.errors.is_empty(),
        "{}: {:?}",
        String::from_utf8_lossy(deploy),
        evaluation.errors
    );
    evaluation
}

/// Added by G3 (DR-135, S3b): stores `term` as the owner `owner` at
/// STORE_PRICE, without a binding.
async fn store(runtime: &mut RhoRuntimeImpl, deploy: &[u8], term: &str) {
    play(runtime, deploy, storer(), STORE_PRICE, None, term).await;
}

/// Added by G3 (DR-135, S3b): a run of `signer` at `price`, with the binding
/// whose client lane is the signer's lane when `bound` holds.
async fn run_as(
    runtime: &mut RhoRuntimeImpl,
    deploy: &[u8],
    signer: Sig,
    price: u64,
    bound: bool,
    term: &str,
) {
    let no_extra: &[Sig] = &[];
    let binding = run_binding(&signer, bound.then_some(no_extra));
    play(runtime, deploy, signer, price, binding, term).await;
}

/// Added by G3 (DR-135, S3b): the openers of every datum on `name`, sorted.
async fn data_openers(runtime: &RhoRuntimeImpl, name: &str) -> Vec<Vec<(CostSignature, u64)>> {
    let mut openers_of: Vec<Vec<(CostSignature, u64)>> = runtime
        .get_data(&channel(name))
        .await
        .into_iter()
        .map(|datum| {
            openers(
                &datum
                    .a
                    .cost_authority
                    .expect("a stored datum has a seal")
                    .regions,
            )
        })
        .collect();
    openers_of.sort_by_key(|openers| format!("{openers:?}"));
    openers_of
}

/// Added by G3 (DR-135, S3b): the callers that the regions of the one datum
/// on `name` record.
async fn datum_callers(runtime: &RhoRuntimeImpl, name: &str) -> Vec<Option<CostCaller>> {
    datum_seal(runtime, name)
        .await
        .into_iter()
        .map(|region| region.caller)
        .collect()
}

/// Added by G3 (DR-135, S3b): the stored body of the one continuation on
/// `name`.
async fn continuation_body(runtime: &RhoRuntimeImpl, name: &str) -> models::rhoapi::Par {
    let continuations = runtime.get_continuations(vec![channel(name)]).await;
    assert_eq!(continuations.len(), 1, "one continuation waits on {name}");
    match continuations[0].continuation.tagged_cont.as_ref() {
        Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(body)) => {
            body.body.clone().expect("a stored body")
        }
        other => panic!("a stored Rholang body on {name}, found {other:?}"),
    }
}

/// Added by G3 (DR-135, S3b): whether `value` holds a held term anywhere.
/// The derived `Debug` prints every field, and a surface signed term prints
/// `held_price: None`.
fn holds_held_term<T: std::fmt::Debug>(value: &T) -> bool {
    format!("{value:?}").contains("held_price: Some(")
}

fn sender_at(price: u64) -> Vec<(CostSignature, u64)> { vec![(named("sender"), price)] }

fn owner_at_store() -> Vec<(CostSignature, u64)> { vec![(named("owner"), STORE_PRICE)] }

fn signer_at(signer: &Sig, price: u64) -> Vec<(CostSignature, u64)> {
    vec![(signature_of(signer), price)]
}

/// Added by G3 (DR-135, S3b): a recorded caller of `name` at `price`.
fn caller_named(name: &str, price: u64) -> Option<CostCaller> {
    Some(CostCaller {
        signature: Some(named(name)),
        price,
    })
}

/// Added by G3 (DR-135, S3b): the Unit sentinel of T12.
fn sentinel() -> Option<CostCaller> {
    Some(CostCaller {
        signature: Some(sig_to_cost_signature(&Sig::Unit).expect("the Unit signature")),
        price: 0,
    })
}

/// Case 1 (H0, H1): an owner body runs a process that it binds for the
/// sender of the process, not for the owner. The process of a client sender
/// runs for the running deployment, whose lane it is.
#[tokio::test]
async fn a_process_held_in_an_owner_body_runs_for_its_sender() {
    for bound in [true, false] {
        let mut runtime = fresh_runtime().await;
        store(
            &mut runtime,
            b"r1-store",
            r#"for ({% @code <- @"r1-in" %}[ owner ]) { code } |
               for ({% @code <- @"r1-client-in" %}[ owner ]) { code }"#,
        )
        .await;
        run_as(
            &mut runtime,
            b"r1-fire",
            firer(),
            FIRE_PRICE,
            bound,
            r#"{% @"r1-in"!({ @"r1-out"!(1) }) %}[ sender ] |
               @"r1-client-in"!({ @"r1-client-out"!(2) })"#,
        )
        .await;
        let (out, client) = (
            datum_seal(&runtime, "r1-out").await,
            datum_seal(&runtime, "r1-client-out").await,
        );
        match bound {
            true => {
                assert_eq!(openers(&out), sender_at(FIRE_PRICE));
                assert_eq!(openers(&client), signer_at(&firer(), FIRE_PRICE));
            }
            false => {
                assert_eq!(openers(&out), signer_at(&firer(), FIRE_PRICE));
                assert_eq!(openers(&client), signer_at(&firer(), FIRE_PRICE));
            }
        }
    }
}

/// Case 2 (H2, H4): a stored body that captures a held process keeps the
/// process's sender and price in a held term, and a later firing in another
/// deployment runs it for the sender at the sender's price.
#[tokio::test]
async fn a_held_process_that_substitution_stores_keeps_its_sender_and_price() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r2-store",
        r#"for ({% @code <- @"r2-in" %}[ owner ]) { for (_ <- @"r2-tick") { code } }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r2-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r2-in"!({ @"r2-out"!(1) }) %}[ sender ]"#,
    )
    .await;
    let stored = continuation_body(&runtime, "r2-tick").await;
    assert_eq!(stored.cost_signed_terms.len(), 1, "{stored:?}");
    assert_eq!(stored.cost_signed_terms[0].held_price, Some(FIRE_PRICE));
    assert_eq!(stored.cost_signed_terms[0].signature, Some(named("sender")));
    run_as(
        &mut runtime,
        b"r2-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"@"r2-tick"!(Nil)"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r2-out").await),
        sender_at(FIRE_PRICE)
    );
}

/// Case 3 (H5): an owner body that re-sends a held process records its
/// sender on the datum, so a later run of the process runs it for the
/// sender.
#[tokio::test]
async fn a_resent_held_process_records_its_holder() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r3-store",
        r#"for ({% @code <- @"r3-in" %}[ owner ]) { @"r3-out"!(code) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r3-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r3-in"!({ @"r3-run"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r3-out").await),
        owner_at_store()
    );
    assert_eq!(datum_callers(&runtime, "r3-out").await, vec![caller_named(
        "sender", FIRE_PRICE
    )]);
    run_as(
        &mut runtime,
        b"r3-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"for (@p <- @"r3-out") { p }"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r3-run").await),
        sender_at(FIRE_PRICE)
    );
}

/// Case 3 (H5, T12): a datum that mixes the sender's code with the owner's
/// own code records the Unit sentinel, which names no holder.
#[tokio::test]
async fn a_mixed_resend_records_no_holder() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r4-store",
        r#"for ({% @code <- @"r4-in" %}[ owner ]) { @"r4-out"!(code, { @"r4-own"!(0) }) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r4-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r4-in"!({ @"r4-run"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r4-out").await, vec![sentinel()]);
}

/// T12 (the mailbox `ret!((0, msg))`): a tagged forward of a held process
/// records its sender, because the classification looks into the tuple. A
/// later run of the process runs it for the sender.
#[tokio::test]
async fn a_tagged_forward_records_its_sender() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r5-store",
        r#"for ({% @msg <- @"r5-in" %}[ owner ]) { @"r5-ret"!((0, msg)) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r5-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r5-in"!({ @"r5-run"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r5-ret").await, vec![caller_named(
        "sender", FIRE_PRICE
    )]);
    run_as(
        &mut runtime,
        b"r5-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"for (@(_, m) <- @"r5-ret") { m }"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r5-run").await),
        sender_at(FIRE_PRICE)
    );
}

/// T12: a forward that mixes the sender's process with the owner's own code
/// records the Unit sentinel, so a later run of both charges the runner, and
/// neither the owner nor the sender.
#[tokio::test]
async fn a_mixed_forward_charges_the_runner() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r6-store",
        r#"for ({% @msg <- @"r6-in" %}[ owner ]) { @"r6-ret"!((msg, { @"r6-own"!(0) })) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r6-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r6-in"!({ @"r6-run"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r6-ret").await, vec![sentinel()]);
    run_as(
        &mut runtime,
        b"r6-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"for (@(m, own) <- @"r6-ret") { m | own }"#,
    )
    .await;
    let runner = signer_at(&later_firer(), LATER_PRICE);
    assert_eq!(openers(&datum_seal(&runtime, "r6-run").await), runner);
    assert_eq!(openers(&datum_seal(&runtime, "r6-own").await), runner);
}

/// Added by G3 (DR-135, S3b review): a forward that carries the processes of
/// two senders records the Unit sentinel, because its holders do not reduce
/// to one agreed holder. A later run of both processes then charges the
/// runner, and never the owner that forwarded them (T12, the forwarder-pays
/// case).
#[tokio::test]
async fn a_forward_of_two_senders_code_charges_the_runner() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r22-store",
        r#"for ({% @a <- @"r22-x" %}[ owner ] & {% @b <- @"r22-y" %}[ owner ]) {
             @"r22-out"!((a, b))
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r22-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r22-x"!({ @"r22-a"!(1) }) %}[ sender ] |
           {% @"r22-y"!({ @"r22-b"!(1) }) %}[ other_sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r22-out").await, vec![sentinel()]);
    run_as(
        &mut runtime,
        b"r22-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"for (@(a, b) <- @"r22-out") { a | b }"#,
    )
    .await;
    let runner = signer_at(&later_firer(), LATER_PRICE);
    assert_eq!(openers(&datum_seal(&runtime, "r22-a").await), runner);
    assert_eq!(openers(&datum_seal(&runtime, "r22-b").await), runner);
}

/// Case 4: a pattern that binds parts of one datum gives each part the
/// datum's sender.
#[tokio::test]
async fn a_pattern_bound_subterm_runs_for_its_sender() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r7-store",
        r#"for ({% @(a, b) <- @"r7-in" %}[ owner ]) { a }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r7-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r7-in"!(({ @"r7-out"!(1) }, 7)) %}[ sender ]"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r7-out").await),
        sender_at(FIRE_PRICE)
    );
}

/// Case 5 (H3): the stored match scenario of `a_match_extract_runs_for_its_sender`
/// and `a_matched_literal_runs_for_the_owner`.
async fn match_scenario() -> RhoRuntimeImpl {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r8-store",
        r#"for ({% @data <- @"r8-in" %}[ owner ]) {
             match data { (p, _) => p } |
             match ({ @"r8-own"!(1) }, 0) { (q, _) => q }
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r8-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r8-in"!(({ @"r8-out"!(1) }, 0)) %}[ sender ]"#,
    )
    .await;
    runtime
}

/// Case 5 (H3): a match on a held variable binds its parts for the
/// variable's holder.
#[tokio::test]
async fn a_match_extract_runs_for_its_sender() {
    let runtime = match_scenario().await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r8-out").await),
        sender_at(FIRE_PRICE)
    );
}

/// Case 5 (D2): a match on a literal of the owner gives its parts no holder,
/// so they run for the owner.
#[tokio::test]
async fn a_matched_literal_runs_for_the_owner() {
    let runtime = match_scenario().await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r8-own").await),
        owner_at_store()
    );
}

/// Case 6 (H1, M5): the method scenario of
/// `a_method_on_a_held_target_runs_for_its_holder` and
/// `a_method_with_owner_code_runs_for_the_owner`.
async fn method_scenario() -> RhoRuntimeImpl {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r9-store",
        r#"for ({% @list <- @"r9-in" %}[ owner ]) {
             list.nth(0) | (list ++ [{ @"r9-own"!(1) }]).nth(1)
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r9-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r9-in"!([{ @"r9-out"!(1) }]) %}[ sender ]"#,
    )
    .await;
    runtime
}

/// Case 6 (H1): a method on a held target with code-free arguments runs for
/// the target's holder.
#[tokio::test]
async fn a_method_on_a_held_target_runs_for_its_holder() {
    let runtime = method_scenario().await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r9-out").await),
        sender_at(FIRE_PRICE)
    );
}

/// Case 6 (M5): a method whose target mixes in the owner's code has no
/// holder, so the owner's code runs for the owner.
#[tokio::test]
async fn a_method_with_owner_code_runs_for_the_owner() {
    let runtime = method_scenario().await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r9-own").await),
        owner_at_store()
    );
}

/// Case 7 (H0): in a join, each value runs for the sender of its own datum.
#[tokio::test]
async fn a_join_runs_each_held_process_for_its_own_sender() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r10-store",
        r#"for ({% @a <- @"r10-x" %}[ owner ] & {% @b <- @"r10-y" %}[ owner ]) { a | b }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r10-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r10-x"!({ @"r10-a"!(1) }) %}[ sender ] | @"r10-y"!({ @"r10-b"!(1) })"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r10-a").await),
        sender_at(FIRE_PRICE)
    );
    assert_eq!(
        openers(&datum_seal(&runtime, "r10-b").await),
        signer_at(&firer(), FIRE_PRICE)
    );
}

/// Added by G3 (DR-135, S3b): the shape of `Either.map2` and
/// `Either."map2 <-"` (Either.rho:99-157), as system work on `NAME`.
const EITHER: &str = r#"
contract @"NAME"(@"map2", @a, @b, @f, return) = {
  match (a, b) {
    ((true, va), (true, vb)) => {
      new ret in {
        match f {
          { for (x, y, r <- fName) { _ } } => {
            f | fName!(va, vb, *ret) | for (@v <- ret) { return!((true, v)) }
          }
        }
      }
    }
  }
} |
contract @"NAME"(@"map2 <-", aCh, bCh, @f, return) = {
  for (@a <- aCh) { for (@b <- bCh) { @"NAME"!("map2", a, b, f, *return) } }
}
"#;

async fn either_runtime(name: &str) -> RhoRuntimeImpl {
    let mut runtime = fresh_runtime().await;
    evaluate_as_system(&mut runtime, b"either-store", &EITHER.replace("NAME", name)).await;
    runtime
}

/// Case 8 (H6, H3): map2 runs the function of its caller for that caller.
#[tokio::test]
async fn either_map2_runs_f_for_the_caller_that_passed_it() {
    let mut runtime = either_runtime("r11-either").await;
    run_as(
        &mut runtime,
        b"r11-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"new ret in {
             {% @"r11-either"!("map2", (true, 1), (true, 2),
                  for (@x, @y, r <- @"r11-f") { r!(y) | @"r11-ran"!(1) }, *ret) %}[ caller ]
           }"#,
    )
    .await;
    assert_eq!(openers(&datum_seal(&runtime, "r11-ran").await), vec![(
        named("caller"),
        FIRE_PRICE
    )]);
}

/// C8 (H2, H5, H6): a client puts its function into the "map2 <-" chain,
/// and an owner feeds the chain in a later deployment. The function runs for
/// the client that passed it, never for the owner that feeds the chain.
#[tokio::test]
async fn a_client_map2_chain_never_charges_the_owner_that_feeds_it() {
    let mut runtime = either_runtime("r12-either").await;
    run_as(
        &mut runtime,
        b"r12-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"@"r12-either"!("map2 <-", "r12-a", "r12-b",
             for (@x, @y, r <- @"r12-f") { r!(y) | @"r12-ran"!(1) }, "r12-ret")"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r12-feed",
        later_firer(),
        LATER_PRICE,
        true,
        r#"@"r12-a"!((true, 1)) | @"r12-b"!((true, 2))"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r12-ran").await),
        signer_at(&firer(), FIRE_PRICE)
    );
}

/// 6c (H2, H5, H6): the owner `w` feeds the chain with a value that carries
/// code, and the last COMM fires in another deployment. The body that runs
/// the function then has two code holders, so the function runs for the
/// deployment of the last COMM (D5), and never for the feeding owner.
#[tokio::test]
async fn a_code_carrying_map2_chain_never_draws_the_feeding_owner() {
    let mut runtime = either_runtime("r13-either").await;
    run_as(
        &mut runtime,
        b"r13-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"@"r13-either"!("map2 <-", "r13-a", "r13-b",
             for (@x, @y, r <- @"r13-f") { r!(y) | @"r13-ran"!(1) }, "r13-ret")"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r13-feed",
        later_firer(),
        LATER_PRICE,
        true,
        r#"{% @"r13-a"!((true, { @"r13-w"!(1) })) %}[ w ]"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r13-last",
        fourth(),
        FOURTH_PRICE,
        true,
        r#"@"r13-b"!((true, 2))"#,
    )
    .await;
    let ran = openers(&datum_seal(&runtime, "r13-ran").await);
    assert_eq!(ran, signer_at(&fourth(), FOURTH_PRICE));
    assert!(ran.iter().all(|(signature, _)| *signature != named("w")));
}

/// Case 9: a client's process runs for the running deployment while the
/// client runs, and for the client as an owner when it runs later in another
/// deployment, both at the client's price.
#[tokio::test]
async fn a_client_sender_pays_now_and_as_an_owner_later() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r14-store",
        r#"for ({% @code <- @"r14-in" %}[ owner ]) { code | for (_ <- @"r14-tick") { code } }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r14-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"@"r14-in"!({ @"r14-out"!(1) })"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r14-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"@"r14-tick"!(Nil)"#,
    )
    .await;
    let client = signer_at(&firer(), FIRE_PRICE);
    assert_eq!(data_openers(&runtime, "r14-out").await, vec![
        client.clone(),
        client
    ]);
}

/// Case 10 (H5, H0): a system body that forwards a held process records its
/// sender on the residue, so a later run of the residue datum runs the
/// process for the sender.
#[tokio::test]
async fn a_residue_caller_is_the_holder_of_its_datum() {
    let mut runtime = fresh_runtime().await;
    evaluate_as_system(
        &mut runtime,
        b"r15-system",
        r#"for (@v <= @"r15-sys") { @"r15-res"!(v) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r15-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r15-sys"!({ @"r15-out"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r15-res").await, vec![
        caller_named("sender", FIRE_PRICE)
    ]);
    run_as(
        &mut runtime,
        b"r15-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"for (@p <- @"r15-res") { p }"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r15-out").await),
        sender_at(FIRE_PRICE)
    );
}

/// H4, H7: a held term that a binding run stored runs as the surface term
/// would, when a run without a binding fires it.
#[tokio::test]
async fn a_held_term_is_transparent_without_a_binding() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r16-store",
        r#"for ({% @code <- @"r16-in" %}[ owner ]) { for (_ <- @"r16-tick") { code } }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r16-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r16-in"!({ @"r16-out"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert!(holds_held_term(
        &continuation_body(&runtime, "r16-tick").await
    ));
    run_as(
        &mut runtime,
        b"r16-later",
        later_firer(),
        LATER_PRICE,
        false,
        r#"@"r16-tick"!(Nil)"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r16-out").await),
        signer_at(&later_firer(), LATER_PRICE)
    );
}

/// R11 and H7: without a binding, held processes run as on dev: for the
/// running deployment, with no holder recorded and no held term stored.
#[tokio::test]
async fn no_binding_keeps_dev_held_processes() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r17-store",
        r#"for ({% @code <- @"r17-in" %}[ owner ]) {
             code | @"r17-out"!(code) | for (_ <- @"r17-tick") { code }
           } |
           for ({% @data <- @"r17-match" %}[ owner ]) { match data { (p, _) => p } } |
           for ({% @list <- @"r17-list" %}[ owner ]) { list.nth(0) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r17-fire",
        firer(),
        FIRE_PRICE,
        false,
        r#"{% @"r17-in"!({ @"r17-run"!(1) }) %}[ sender ] |
           {% @"r17-match"!(({ @"r17-matched"!(1) }, 0)) %}[ sender ] |
           {% @"r17-list"!([{ @"r17-listed"!(1) }]) %}[ sender ]"#,
    )
    .await;
    let running = signer_at(&firer(), FIRE_PRICE);
    assert_eq!(openers(&datum_seal(&runtime, "r17-run").await), running);
    assert_eq!(openers(&datum_seal(&runtime, "r17-matched").await), running);
    assert_eq!(openers(&datum_seal(&runtime, "r17-listed").await), running);
    assert_eq!(datum_callers(&runtime, "r17-out").await, vec![None]);
    assert!(!holds_held_term(
        &continuation_body(&runtime, "r17-tick").await
    ));
}

/// M2: a stored body that re-sends a captured process later keeps the
/// process's sender in a held marker. The send strips the marker and records
/// the sender on the datum, so the worker runs the process for the sender.
#[tokio::test]
async fn a_deferred_resend_from_a_stored_body_keeps_its_holder() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r18-store",
        r#"for ({% @job <- @"r18-jobs" %}[ owner ]) {
             for (_ <- @"r18-tick") { @"r18-worker"!(job) }
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r18-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r18-jobs"!({ @"r18-ran"!(1) }) %}[ sender ]"#,
    )
    .await;
    let stored = continuation_body(&runtime, "r18-tick").await;
    assert!(holds_held_term(&stored.sends), "the job is a held marker");
    run_as(
        &mut runtime,
        b"r18-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"@"r18-tick"!(Nil) | for (@j <- @"r18-worker") { j }"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r18-ran").await),
        sender_at(FIRE_PRICE)
    );
}

/// T10: a stored body that forwards a part of a held message, through a
/// match inside the stored body or around it, keeps the sender in a held
/// marker.
#[tokio::test]
async fn a_deferred_forward_of_a_match_part_keeps_its_holder() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r19-store",
        r#"for ({% @msg <- @"r19-in" %}[ owner ]) {
             for (_ <- @"r19-tick") { match msg { (_, payload) => @"r19-out"!(payload) } }
           } |
           for ({% @msg <- @"r19b-in" %}[ owner ]) {
             match msg { (_, payload) => for (_ <- @"r19b-tick") { @"r19b-out"!(payload) } }
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r19-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r19-in"!((0, { @"r19-ran"!(1) })) %}[ sender ] |
           {% @"r19b-in"!((0, { @"r19b-ran"!(1) })) %}[ sender ]"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r19-later",
        later_firer(),
        LATER_PRICE,
        true,
        r#"@"r19-tick"!(Nil) | for (@p <- @"r19-out") { p } |
           @"r19b-tick"!(Nil) | for (@p <- @"r19b-out") { p }"#,
    )
    .await;
    assert_eq!(
        openers(&datum_seal(&runtime, "r19-ran").await),
        sender_at(FIRE_PRICE),
        "a match inside the stored body"
    );
    assert_eq!(
        openers(&datum_seal(&runtime, "r19b-ran").await),
        sender_at(FIRE_PRICE),
        "a match around the stored body"
    );
}

/// The state of `data_only_system_calls_keep_dev_bytes` with or without a
/// binding: the seals of the forwarded data and the stored batch body.
async fn data_only_state(bound: bool) -> (String, String, String) {
    let mut runtime = fresh_runtime().await;
    evaluate_as_system(
        &mut runtime,
        b"r20-system",
        r#"for (@x <= @"r20-sys") { @"r20-out"!(x) } |
           for (@transfers <= @"r20-batch") { for (_ <- @"r20-tick") { @"r20-out2"!(transfers) } }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r20-fire",
        firer(),
        FIRE_PRICE,
        bound,
        r#"{% @"r20-sys"!(42) %}[ sender ] | {% @"r20-batch"!([1, 2, 3]) %}[ sender ]"#,
    )
    .await;
    let stored = format!(
        "{:?}",
        runtime.get_continuations(vec![channel("r20-tick")]).await
    );
    run_as(
        &mut runtime,
        b"r20-later",
        later_firer(),
        LATER_PRICE,
        bound,
        r#"@"r20-tick"!(Nil)"#,
    )
    .await;
    (
        format!("{:?}", runtime.get_data(&channel("r20-out")).await),
        format!("{:?}", runtime.get_data(&channel("r20-out2")).await),
        stored,
    )
}

/// 5d (H6, M2): system calls whose data carry no code keep the bytes of
/// dev: no caller on their residue and no held marker in their stored
/// bodies, as in SystemVault's `transferBatch`.
#[tokio::test]
async fn data_only_system_calls_keep_dev_bytes() {
    assert_eq!(data_only_state(true).await, data_only_state(false).await);
}

/// The datum of `markers_are_stripped_without_a_binding`: its pars, after a
/// binding run stored the job (as a marker when `bound_store`) and a run
/// without a binding fired it.
async fn stripped_datum(bound_store: bool) -> Vec<models::rhoapi::Par> {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r21-store",
        r#"for ({% @job <- @"r21-jobs" %}[ owner ]) {
             for (_ <- @"r21-tick") { @"r21-out"!(job) }
           }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r21-fire",
        firer(),
        FIRE_PRICE,
        bound_store,
        r#"{% @"r21-jobs"!({ @"r21-x"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(
        holds_held_term(&continuation_body(&runtime, "r21-tick").await),
        bound_store,
        "a binding run stores a held marker"
    );
    run_as(
        &mut runtime,
        b"r21-later",
        later_firer(),
        LATER_PRICE,
        false,
        r#"@"r21-tick"!(Nil)"#,
    )
    .await;
    let data = runtime.get_data(&channel("r21-out")).await;
    assert_eq!(data.len(), 1);
    data[0].a.pars.clone()
}

/// H5, H7: every run strips held markers, also without a binding, so no held
/// term reaches a datum.
#[tokio::test]
async fn markers_are_stripped_without_a_binding() {
    let stripped = stripped_datum(true).await;
    assert!(!holds_held_term(&stripped), "{stripped:?}");
    assert_eq!(stripped, stripped_datum(false).await);
}

/// M6 (T13): every non-Unit region that native execution creates or stores
/// has a price above 0 and at least the smallest checked price of the runs,
/// which the policy minimum bounds. Held work and residue callers keep the
/// prices of earlier runs.
#[tokio::test]
async fn every_native_region_has_a_price_of_at_least_the_policy_minimum() {
    let mut runtime = fresh_runtime().await;
    evaluate_as_system(
        &mut runtime,
        b"m6-system",
        r#"for (@v <= @"m6-sys") { @"m6-res"!(v) }"#,
    )
    .await;
    let mut evaluations = Vec::new();
    for (deploy, signer, price, term) in [
        (
            b"m6-store".as_slice(),
            storer(),
            STORE_PRICE,
            r#"for ({% @code <- @"m6-in" %}[ owner ]) { code | for (_ <- @"m6-tick") { code } }"#,
        ),
        (
            b"m6-fire",
            firer(),
            FIRE_PRICE,
            r#"{% @"m6-in"!({ @"m6-out"!(1) }) %}[ sender ] | {% @"m6-sys"!({ @"m6-out2"!(2) }) %}[ sender ]"#,
        ),
        (
            b"m6-later",
            later_firer(),
            LATER_PRICE,
            r#"@"m6-tick"!(Nil) | for (@p <- @"m6-res") { p }"#,
        ),
    ] {
        let binding = run_binding(&signer, Some(&[]));
        evaluations.push(play_observed(&mut runtime, deploy, signer, price, binding, term).await);
    }
    let mut regions: Vec<CostRegion> = evaluations
        .iter()
        .flat_map(|evaluation| evaluation.byte_observations.rows.iter())
        .flat_map(|row| row.authority.regions.iter().cloned())
        .collect();
    for name in ["m6-out", "m6-out2"] {
        for datum in runtime.get_data(&channel(name)).await {
            regions.extend(datum.a.cost_authority.expect("a seal").regions);
        }
    }
    let priced: Vec<&CostRegion> = regions
        .iter()
        .filter(|region| {
            !is_unit_cost_signature(region.signature.as_ref().expect("a region signature"))
        })
        .collect();
    assert!(priced.len() >= 6, "the runs observe their regions");
    // STORE_PRICE is the smallest checked price of the runs, and it is above
    // 0, so a region at or above it is above 0 too.
    const { assert!(STORE_PRICE > 0) };
    assert!(
        priced.iter().all(|region| region.price >= STORE_PRICE),
        "{priced:?}"
    );
}

/// Added by G3 (DR-135, S3b): one random held scenario of the properties: a
/// sender (named or the client) sends a process to an owner body, which runs
/// it now, stores it, and re-sends it. A later run fires the stored body and
/// runs the re-sent process. The result is the runtime and the holder that
/// the held work must run for.
async fn held_scenario(
    named_sender: bool,
    price: u64,
    bound: bool,
) -> (RhoRuntimeImpl, Vec<(CostSignature, u64)>) {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"p-store",
        r#"for ({% @code <- @"p-in" %}[ owner ]) {
             code | for (_ <- @"p-tick") { code } | @"p-resend"!(code)
           }"#,
    )
    .await;
    let send = match named_sender {
        true => r#"{% @"p-in"!({ @"p-out"!(1) }) %}[ sender ]"#,
        false => r#"@"p-in"!({ @"p-out"!(1) })"#,
    };
    run_as(&mut runtime, b"p-fire", firer(), price, bound, send).await;
    run_as(
        &mut runtime,
        b"p-later",
        later_firer(),
        LATER_PRICE,
        bound,
        r#"@"p-tick"!(Nil) | for (@p <- @"p-resend") { p }"#,
    )
    .await;
    let holder = match named_sender {
        true => sender_at(price),
        false => signer_at(&firer(), price),
    };
    (runtime, holder)
}

/// Added by G3 (DR-135, S3b): a small tokio runtime for one case of a
/// property.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(6))]

    /// Every region that held work opens is a region of the holder.
    #[test]
    fn held_work_opens_only_holder_regions(named_sender in proptest::bool::ANY, price in 1_u64..40) {
        let (runtime, holder) = block_on(held_scenario(named_sender, price, true));
        let out = block_on(data_openers(&runtime, "p-out"));
        proptest::prop_assert_eq!(out.len(), 3);
        for openers in &out {
            proptest::prop_assert!(openers.iter().all(|(signature, _)| *signature == holder[0].0));
        }
    }

    /// Every region that held work opens records the holder's price, also
    /// when the work runs in a later deployment at another price.
    #[test]
    fn held_regions_record_the_holder_price(named_sender in proptest::bool::ANY, price in 1_u64..40) {
        let (runtime, holder) = block_on(held_scenario(named_sender, price, true));
        let out = block_on(data_openers(&runtime, "p-out"));
        proptest::prop_assert_eq!(out, vec![holder.clone(), holder.clone(), holder]);
    }

    /// After runs with a binding, no stored datum holds a held term: the
    /// sends strip every marker.
    #[test]
    fn no_value_or_datum_contains_a_held_term(named_sender in proptest::bool::ANY, price in 1_u64..40) {
        let (runtime, _) = block_on(held_scenario(named_sender, price, true));
        for name in ["p-out", "p-resend", "p-in", "p-tick"] {
            let data = block_on(runtime.get_data(&channel(name)));
            proptest::prop_assert!(!holds_held_term(&data), "{}: {:?}", name, data);
        }
    }

    /// Without a binding, no stored body holds a held term, no seal records
    /// a caller, and the held work runs for the running deployments, as on
    /// dev.
    #[test]
    fn without_a_binding_bodies_and_seals_equal_dev(named_sender in proptest::bool::ANY, price in 1_u64..40) {
        let (runtime, _) = block_on(held_scenario(named_sender, price, false));
        let out = block_on(data_openers(&runtime, "p-out"));
        let mut expected = vec![
            signer_at(&firer(), price),
            signer_at(&later_firer(), LATER_PRICE),
            signer_at(&later_firer(), LATER_PRICE),
        ];
        expected.sort_by_key(|openers| format!("{openers:?}"));
        proptest::prop_assert_eq!(out, expected);
        for name in ["p-out", "p-resend"] {
            let data = block_on(runtime.get_data(&channel(name)));
            proptest::prop_assert!(data.iter().all(|datum| datum
                .a
                .cost_authority
                .as_ref()
                .is_none_or(|seal| seal.regions.iter().all(|region| region.caller.is_none()))));
        }
        let continuations = block_on(runtime.get_continuations(vec![channel("p-tick")]));
        proptest::prop_assert!(!holds_held_term(&continuations));
    }
}

// ---- Added by G3 (DR-135, S4): held work whose holder cannot pay (design
// §13.5 of Rule H, plan S4 SA5). A stored owner body keeps the process of
// `sender` for a later tick (case 2). The later run lists the purse of
// `sender` with the budget `held_budget`, so the held work of `sender` is
// refused when the purse is empty.

/// Added by G3 (DR-135, S4): stores the owner body, fires it with the
/// process `code` of `sender`, and ticks it in a later run whose binding
/// gives `sender` the budget `held_budget`. The result is the runtime, the
/// evaluation of the later run and its binding.
async fn held_tick(
    tag: &str,
    code: &str,
    held_budget: u64,
) -> (
    RhoRuntimeImpl,
    crate::rust::interpreter::interpreter::EvaluateResult,
    Arc<OwnerFundingBinding>,
) {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        format!("{tag}-store").as_bytes(),
        &r#"for ({% @code <- @"TAG-in" %}[ owner ]) {
               for (_ <- @"TAG-tick") { code | @"TAG-after"!(1) }
             }"#
        .replace("TAG", tag),
    )
    .await;
    run_as(
        &mut runtime,
        format!("{tag}-fire").as_bytes(),
        firer(),
        FIRE_PRICE,
        true,
        &r#"{% @"TAG-in"!({ CODE }) %}[ sender ]"#
            .replace("TAG", tag)
            .replace("CODE", code),
    )
    .await;
    let budgets: Vec<(CostSignature, u64)> = test_owners()
        .into_iter()
        .map(|signature| match signature == named("sender") {
            true => (signature, held_budget),
            false => (signature, FUNDED),
        })
        .collect();
    let binding = funded_binding(&[later_firer()], &budgets);
    let evaluation = play_observed(
        &mut runtime,
        format!("{tag}-later").as_bytes(),
        later_firer(),
        LATER_PRICE,
        Some(Arc::clone(&binding)),
        &r#"@"TAG-tick"!(Nil)"#.replace("TAG", tag),
    )
    .await;
    (runtime, evaluation, binding)
}

/// Added by G3 (DR-135, S4, Rule H §13.5): held work whose holder cannot pay
/// records the decision OwnerRefused for its introduction and stores
/// nothing, and the run continues: it has no error, and the owner's own send
/// in the same body is stored. The journal check with the binding of the run
/// confirms the refusal, because the listed purse of the holder is empty.
#[tokio::test]
async fn held_work_whose_holder_cannot_pay_records_owner_refused_and_the_run_continues() {
    let (runtime, evaluation, binding) = held_tick("r45", r#"@"r45-out"!(1)"#, 0).await;
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
    assert_eq!(
        openers(&datum_seal(&runtime, "r45-after").await),
        owner_at_store()
    );
    assert!(runtime.get_data(&channel("r45-out")).await.is_empty());
    let recording = evaluation
        .native_budget_recording
        .expect("a native run records");
    let refused: Vec<&NativeBudgetAttempt> = recording
        .attempts
        .iter()
        .filter(|attempt| attempt.owner_refused)
        .collect();
    assert_eq!(refused.len(), 1);
    assert!(!refused[0].granted);
    assert_eq!(
        refused[0].occurrence.stage,
        NativeAttemptStage::ProduceIntroduction
    );
    assert_eq!(
        openers(&refused[0].observation.authority.regions),
        sender_at(FIRE_PRICE)
    );
    let rows = evaluation
        .native_operation_recording
        .expect("a native run journals");
    assert!(rows.iter().any(|row| {
        row.completion == RSpaceOperationCompletion::Rejected
            && row.comm.is_none()
            && matches!(row.introduction, NativeObservationLink::Attempt(index)
                if recording.attempts[index].owner_refused)
    }));
    with_priced_contract(1_000_000, [1; 4], LATER_PRICE, |contract| {
        contract
            .check_operation_journal_with_owners(
                recording.session,
                &recording,
                rows,
                journal_limits(),
                Some(&binding),
            )
            .expect("the journal check confirms the refusal");
    });
}

/// Added by G3 (DR-135, S4, SA5): a token stack that held work produces is
/// refused at its introduction when its holder cannot pay. The stack
/// transfer is then aborted, so the run records no birth whose stack was
/// never stored. A funded holder stores the stack and records its birth.
#[tokio::test]
async fn a_refused_stack_produce_aborts_its_transfer() {
    let stack = SignatureChannel::from_sig(&Sig::Ground(canon_ground("sender"))).par;
    let (runtime, evaluation, _) = held_tick("r46", "sender :: ()", FUNDED).await;
    assert_eq!(evaluation.authority_stack_births.len(), 1);
    assert_eq!(runtime.get_data(&stack).await.len(), 1);
    let (runtime, evaluation, _) = held_tick("r47", "sender :: ()", 0).await;
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
    assert!(evaluation
        .native_budget_recording
        .expect("a native run records")
        .attempts
        .iter()
        .any(|attempt| attempt.owner_refused));
    assert!(evaluation.authority_stack_births.is_empty());
    assert!(runtime.get_data(&stack).await.is_empty());
    assert_eq!(
        openers(&datum_seal(&runtime, "r47-after").await),
        owner_at_store()
    );
}

// ---- Added by G3 (DR-135, S5): the owner checks of the play matcher with
// owner funding (R5, A5, R9).

/// Added by G3 (DR-135, S5): a binding whose client lanes are the lanes of
/// `client` and whose full table lists every test owner at `FUNDED`, except
/// the named owners in `budgets`, which have their budgets.
fn binding_with(client: &[Sig], budgets: &[(&str, u64)]) -> Arc<OwnerFundingBinding> {
    let listed: Vec<(CostSignature, u64)> = test_owners()
        .into_iter()
        .map(
            |signature| match budgets.iter().find(|(name, _)| named(name) == signature) {
                Some((_, budget)) => (signature, *budget),
                None => (signature, FUNDED),
            },
        )
        .collect();
    funded_binding(client, &listed)
}

/// Added by G3 (DR-135, S5): the custody of the purse of the named owner.
fn custody_of(name: &str) -> [u8; 32] {
    cost_signature_to_sig(&named(name))
        .expect("a canonical signature")
        .lane_hash()
}

/// Added by G3 (DR-135, S5): the owner debits that the fold of the journal of
/// `evaluation` rebuilds. The journal check with `binding` must accept the
/// run, under the contract of `play_observed` at `price`.
fn fold_debits(
    evaluation: &crate::rust::interpreter::interpreter::EvaluateResult,
    binding: &OwnerFundingBinding,
    price: u64,
) -> ResourceMultiset<[u8; 32]> {
    let recording = evaluation
        .native_budget_recording
        .as_ref()
        .expect("a native run records");
    let rows = evaluation
        .native_operation_recording
        .clone()
        .expect("a native run journals");
    with_priced_contract(1_000_000, [1; 4], price, |contract| {
        contract
            .check_operation_journal_with_owners(
                recording.session,
                recording,
                rows,
                journal_limits(),
                Some(binding),
            )
            .expect("the journal check accepts the run")
            .budget_evidence()
            .owner()
            .expect("an owner fold")
            .ledger()
            .debits()
            .clone()
    })
}

fn integers(
    data: &[rspace_plus_plus::rspace::internal::Datum<models::rhoapi::ListParWithRandom>],
) -> Vec<i64> {
    let mut values: Vec<i64> = data
        .iter()
        .flat_map(|datum| datum.a.pars.iter())
        .flat_map(|par| par.exprs.iter())
        .filter_map(|expr| match expr.expr_instance {
            Some(models::rhoapi::expr::ExprInstance::GInt(value)) => Some(value),
            _ => None,
        })
        .collect();
    values.sort_unstable();
    values
}

/// Added by G3 (DR-135, S5, R5): a datum whose owner cannot pay its share of
/// a COMM fails the item test, so it is skipped like a pattern miss, and the
/// receive takes the datum whose owner can pay. Over fresh runs, whose data
/// orders are random, the receive never takes the unfunded datum. The fold
/// draws nothing from the unfunded owner.
#[tokio::test]
async fn a_datum_whose_owner_cannot_pay_is_skipped_like_a_pattern_miss() {
    for run in 0..8_u8 {
        let mut runtime = fresh_runtime().await;
        store(
            &mut runtime,
            &[b'r', 51, run],
            r#"{% @"r51-in"!(1) %}[ sender ] | {% @"r51-in"!(2) %}[ other_sender ]"#,
        )
        .await;
        let binding = binding_with(&[firer()], &[("sender", 0)]);
        let evaluation = play_observed(
            &mut runtime,
            &[b'f', 51, run],
            firer(),
            FIRE_PRICE,
            Some(Arc::clone(&binding)),
            r#"for (@x <- @"r51-in") { @"r51-out"!(x) }"#,
        )
        .await;
        assert_eq!(
            integers(&runtime.get_data(&channel("r51-out")).await),
            vec![2]
        );
        assert_eq!(integers(&runtime.get_data(&channel("r51-in")).await), vec![
            1
        ]);
        let debits = fold_debits(&evaluation, &binding, FIRE_PRICE);
        assert_eq!(debits.get(&custody_of("sender")), 0);
        assert!(debits.get(&custody_of("other_sender")) > 0);
    }
}

/// Added by G3 (DR-135, S5, Rule H §13.5 item 2): the region of a forwarded
/// datum records the holder of its code as its caller (H5). At a COMM the
/// signer of the region pays, never the caller: with the caller's purse
/// empty, the item test passes, the COMM fires, and the fold draws nothing
/// from the caller.
#[tokio::test]
async fn a_recorded_caller_is_never_charged_at_the_comm() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r52-store",
        r#"for ({% @code <- @"r52-in" %}[ owner ]) { @"r52-fwd"!(code) }"#,
    )
    .await;
    run_as(
        &mut runtime,
        b"r52-fire",
        firer(),
        FIRE_PRICE,
        true,
        r#"{% @"r52-in"!({ @"r52-out"!(1) }) %}[ sender ]"#,
    )
    .await;
    assert_eq!(datum_callers(&runtime, "r52-fwd").await, vec![
        caller_named("sender", FIRE_PRICE)
    ]);
    let binding = binding_with(&[later_firer()], &[("sender", 0)]);
    let evaluation = play_observed(
        &mut runtime,
        b"r52-later",
        later_firer(),
        LATER_PRICE,
        Some(Arc::clone(&binding)),
        r#"for (_ <- @"r52-fwd") { Nil }"#,
    )
    .await;
    assert!(runtime.get_data(&channel("r52-fwd")).await.is_empty());
    let debits = fold_debits(&evaluation, &binding, LATER_PRICE);
    assert_eq!(debits.get(&custody_of("sender")), 0);
    assert!(debits.get(&custody_of("owner")) > 0);
}

/// Added by G3 (DR-135, S5, A5): the lollipop `[ owner -o w ]` re-signs the
/// body of the receive with `w`, so the body runs for `w`. A COMM that fires
/// it needs `w` to keep a positive budget. An empty purse of `w` refuses the
/// COMM: the datum is stored, the continuation waits, and the journal records
/// the refusal on the produce row. A funded `w` lets the COMM fire.
#[tokio::test]
async fn an_unfunded_transferee_refuses_the_comm_and_both_items_stay() {
    for (budget, fires) in [(0, false), (FUNDED, true)] {
        let mut runtime = fresh_runtime().await;
        store(
            &mut runtime,
            b"r54-store",
            r#"{% for (@x <- @"r54-in") { @"r54-out"!(x) } %}[ owner -o w ]"#,
        )
        .await;
        let binding = binding_with(&[firer()], &[("w", budget)]);
        let evaluation = play_observed(
            &mut runtime,
            b"r54-fire",
            firer(),
            FIRE_PRICE,
            Some(Arc::clone(&binding)),
            r#"@"r54-in"!(1)"#,
        )
        .await;
        let refusals: usize = evaluation
            .native_operation_recording
            .as_ref()
            .expect("a native run journals")
            .iter()
            .map(|row| row.refusals.len())
            .sum();
        match fires {
            false => {
                assert_eq!(integers(&runtime.get_data(&channel("r54-in")).await), vec![
                    1
                ]);
                assert_eq!(
                    runtime
                        .get_continuations(vec![channel("r54-in")])
                        .await
                        .len(),
                    1
                );
                assert!(runtime.get_data(&channel("r54-out")).await.is_empty());
                assert_eq!(refusals, 1);
            }
            true => {
                assert_eq!(
                    integers(&runtime.get_data(&channel("r54-out")).await),
                    vec![1]
                );
                assert_eq!(refusals, 0);
            }
        }
        fold_debits(&evaluation, &binding, FIRE_PRICE);
    }
}

/// Added by G3 (DR-135, S5, R9): a peek COMM pays for the restoration of the
/// datum that it reads. The attempt of the COMM records the restoration's
/// introduction, with the identity of the datum's produce, the datum stays
/// stored, and the journal check accepts the run.
#[tokio::test]
async fn a_peek_records_its_restoration_and_the_journal_folds() {
    let mut runtime = fresh_runtime().await;
    store(
        &mut runtime,
        b"r53-store",
        r#"{% @"r53-in"!(7) %}[ sender ]"#,
    )
    .await;
    let binding = binding_with(&[firer()], &[]);
    let evaluation = play_observed(
        &mut runtime,
        b"r53-fire",
        firer(),
        FIRE_PRICE,
        Some(Arc::clone(&binding)),
        r#"for (@x <<- @"r53-in") { @"r53-out"!(x) }"#,
    )
    .await;
    assert_eq!(integers(&runtime.get_data(&channel("r53-in")).await), vec![
        7
    ]);
    assert_eq!(
        integers(&runtime.get_data(&channel("r53-out")).await),
        vec![7]
    );
    let recording = evaluation
        .native_budget_recording
        .as_ref()
        .expect("a native run records");
    let restorations: Vec<[u8; 32]> = recording
        .attempts
        .iter()
        .filter(|attempt| attempt.occurrence.stage == NativeAttemptStage::Comm)
        .flat_map(|attempt| {
            attempt
                .restorations
                .iter()
                .map(|restoration| restoration.event_id)
        })
        .collect();
    assert_eq!(restorations.len(), 1);
    assert!(recording.attempts.iter().any(|attempt| {
        attempt.occurrence.stage == NativeAttemptStage::ProduceIntroduction
            && attempt.observation.event_id == restorations[0]
            && attempt.granted
    }));
    assert!(fold_debits(&evaluation, &binding, FIRE_PRICE).get(&custody_of("sender")) > 0);
    // Added by G3 (DR-135, G-4): the play observation of the restoration has
    // the owner charge that the COMM reserved for it. So the granted
    // restoration bills its reservation and draws nothing again, and the run
    // bills every draw from the purse of the sender.
    let restoration = recording
        .attempts
        .iter()
        .position(|attempt| {
            attempt.occurrence.stage == NativeAttemptStage::ProduceIntroduction
                && attempt.observation.event_id == restorations[0]
                && attempt.granted
        })
        .expect("the restoration is introduced");
    let rows = evaluation
        .native_operation_recording
        .clone()
        .expect("a native run journals");
    with_priced_contract(1_000_000, [1; 4], FIRE_PRICE, |contract| {
        let checked = contract
            .check_operation_journal_with_owners(
                recording.session,
                recording,
                rows,
                journal_limits(),
                Some(&binding),
            )
            .expect("the journal check accepts the run");
        let fold = checked.budget_evidence().owner().expect("an owner fold");
        let sender = custody_of("sender");
        assert_eq!(
            fold.remaining_at(&sender, restoration),
            fold.remaining_at(&sender, restoration + 1),
            "the restoration draws nothing again"
        );
        let remaining = fold
            .remaining_at(&sender, recording.attempts.len())
            .expect("the sender is listed");
        assert_eq!(
            fold.ledger().debits().get(&sender),
            FUNDED - remaining,
            "the run bills every draw from the purse of the sender"
        );
    });
}

/// Added by G3 (DR-135, S5, R8): the introduction of an owner body's send is
/// refused when the owner has paid the COMM that fired the body and has no
/// budget left. The send stores nothing, and the run continues: it has no
/// error, and the other send of the run is stored. A calibration run measures
/// the owner's share of the COMM, which is the budget of the second run.
#[tokio::test]
async fn an_owner_refused_introduction_stores_nothing_and_the_run_continues() {
    let scenario = |budget: u64| async move {
        let mut runtime = fresh_runtime().await;
        store(
            &mut runtime,
            b"r50-store",
            r#"for ({% @x <- @"r50-in" %}[ owner ]) { @"r50-out"!(x) }"#,
        )
        .await;
        let binding = binding_with(&[firer()], &[("owner", budget)]);
        let evaluation = play_observed(
            &mut runtime,
            b"r50-fire",
            firer(),
            FIRE_PRICE,
            Some(Arc::clone(&binding)),
            r#"@"r50-in"!(1) | @"r50-side"!(2)"#,
        )
        .await;
        (runtime, evaluation, binding)
    };
    let (_, calibration, binding) = scenario(FUNDED).await;
    let recording = calibration
        .native_budget_recording
        .as_ref()
        .expect("a native run records");
    let introduction = recording
        .attempts
        .iter()
        .find(|attempt| {
            attempt.occurrence.stage == NativeAttemptStage::ProduceIntroduction
                && openers(&attempt.observation.authority.regions) == owner_at_store()
        })
        .expect("the owner introduces its send");
    let introduction_share = introduction
        .observation
        .measurement
        .expect("a measured introduction")
        .introduction_bytes
        * STORE_PRICE;
    let total = fold_debits(&calibration, &binding, FIRE_PRICE).get(&custody_of("owner"));
    let comm_share = total - introduction_share;
    assert!(comm_share > 0 && introduction_share > 0);

    let (runtime, evaluation, binding) = scenario(comm_share).await;
    assert!(evaluation.errors.is_empty(), "{:?}", evaluation.errors);
    assert!(runtime.get_data(&channel("r50-out")).await.is_empty());
    assert_eq!(
        integers(&runtime.get_data(&channel("r50-side")).await),
        vec![2]
    );
    let recording = evaluation
        .native_budget_recording
        .as_ref()
        .expect("a native run records");
    assert_eq!(
        recording
            .attempts
            .iter()
            .filter(|attempt| attempt.owner_refused)
            .count(),
        1
    );
    assert_eq!(
        fold_debits(&evaluation, &binding, FIRE_PRICE).get(&custody_of("owner")),
        comm_share
    );
}
