//! G1-4b (DR-123): the check before execution of one offered deploy (design
//! §6). It turns the linear cost estimate of DR-122 and the wallet snapshot at
//! R0 into holds, a resource bound and a bound source, or into a rejection
//! before any runtime entry, with no state change and no charge.
//!
//! G1-6 (DR-124) makes the check live in play and replay, and the check holds
//! only the client's demand: the demand of the client lanes, whose purses the
//! envelope's sources are located at (P1:2191-2200). The G1-6 fix adds the
//! lanes that the sources permit for at least one class: the client's purses
//! may pay them (P2 `prop:local-suff`, P2:1520-1527).

use std::collections::{BTreeMap, BTreeSet};

// Moved by dedup (task 11415) with the cursors to `rooted_cursors` in
// `family_selection.rs`.
// use std::num::NonZeroUsize;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
// Moved by dedup (task 11415) with the lane conversion to
// `lane_location_metered` in `producer.rs`, which uses these imports now.
// use prost::Message;
// use rholang::rust::interpreter::accounting::authority::{
//     cost_signature_to_sig_metered, funding_sig_channel_metered,
// };
use rholang::rust::interpreter::accounting::delta_sigma::estimate::{
    estimate_offered_term, location_lane_key, Completeness, CostEstimate, IncompleteReason,
};
// Moved by dedup (task 11415) with the cursors to `rooted_cursors` in
// `family_selection.rs`.
// use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource;
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_offered_phlo_controls, PhloControlsBinding, PhloOffer, PhloScheduleBinding,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, project_phlo_obligations, select_phlo_funding_family,
    CheckedPhloObligations, CountedPhloExecutionWitness, PhloExecutionError,
    PhloFamilyFundingInput, PhloFamilyFundingLimits, PhloFundingRequirement, PhloFundingSource,
    PhloObligationError, PhloOutcome, PhloResource, PhloResourceAmount,
};
use rholang::rust::interpreter::accounting::{funding_sig, Sig};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reduce::ReducerCore;

// Moved by dedup (task 11415) with the lane conversion to
// `lane_location_metered` in `producer.rs`.
// use shared::rust::clone_backing::{self, BackingError};

// Changed by dedup (task 11415, D1): the shared steps of the rooted funding
// setup.
// use super::family_selection::{source_permits, SourceAuthority};
use super::family_selection::{
    eligibility_row, rooted_cursors, rooted_source, RootedSetupError, SourceAuthority,
};
// Changed by dedup (task 11415, D2): the shared lane conversion.
// use super::producer::NativeOfferedFamilyLimits;
use super::producer::{lane_location, LaneLocationError, NativeOfferedFamilyLimits};
use crate::rust::util::rholang::acceptance::PreparedOfferedCandidate;
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::tools::Tools;

/// Whether the estimate covers all cost of the deploy. Only a complete
/// estimate binds the bound source `Certificate` (design §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfferedEstimateKind {
    Complete,
    Incomplete(IncompleteReason),
}

/// The outcome of the check: the estimate B in phlo, the resource bound and
/// its source, and the holds at R0 in canonical custody order. The holds are
/// fit checks and move no funds (DR-5, design §8.1). Only the gate builds it.
/// Since G1-6 (DR-124), B counts the client lanes only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedGate {
    kind: OfferedEstimateKind,
    estimate_phlo: u64,
    resource_bound: u64,
    bound_source: NativeBoundSource,
    holds: Vec<([u8; 32], u128)>,
}

impl OfferedGate {
    pub fn kind(&self) -> &OfferedEstimateKind { &self.kind }

    pub fn estimate_phlo(&self) -> u64 { self.estimate_phlo }

    pub fn resource_bound(&self) -> u64 { self.resource_bound }

    pub fn bound_source(&self) -> NativeBoundSource { self.bound_source }

    /// The hold of each purse in canonical custody order, the seam of the
    /// group check of DR-65 item 4.
    pub fn holds(&self) -> &[([u8; 32], u128)] { &self.holds }

    /// H: B·phloPrice + 1 for a complete estimate, phloLimit·phloPrice + 1
    /// otherwise.
    pub fn total_hold(&self) -> u128 {
        self.holds
            .iter()
            .fold(0u128, |total, (_, hold)| total.saturating_add(*hold))
    }

    /// Added by G1-6 (DR-124), test only: the binding of every live site
    /// before the check went live, the signed limit under `SignedLimit` with
    /// no hold and no estimate. The per-node hook that skips the check uses
    /// it, so a test can show that validators reject the block of a producer
    /// that skips the check.
    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn unchecked_signed_limit(limit: u64) -> Self {
        Self {
            kind: OfferedEstimateKind::Incomplete(IncompleteReason::UnsizedValue),
            estimate_phlo: 0,
            resource_bound: limit,
            bound_source: NativeBoundSource::SignedLimit,
            holds: Vec::new(),
        }
    }
}

/// Why the holds do not fit. The rejection text starts with "insufficient
/// phlogiston" (P1:2210).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InsufficientCause {
    #[error("no placement of the known demand and the fee fits the sources")]
    NoPlacement,
    #[error("{left} atomic units of the remainder fit no source")]
    RemainderLeft { left: u128 },
    #[error("the total hold {total} exceeds the total exposure {limit}")]
    ExposureExceeded { total: u128, limit: u128 },
}

/// A rejection before any runtime entry: "No state change occurs. No tokens
/// are consumed." (P1:2210-2212, design §6.3).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OfferedGateRejection {
    /// The estimate exceeds the signed limit (design §6.3 item 1).
    #[error("the estimate {estimate} exceeds the signed phlo limit {limit}")]
    EstimateAboveLimit { estimate: u64, limit: u64 },
    /// The holds do not fit (design §6.3 item 2).
    #[error("insufficient phlogiston: {0}")]
    InsufficientPhlogiston(InsufficientCause),
    /// The witness of the estimate, its obligations or its keys exceed the
    /// settlement limits, which settlement checks again (arbitration M3).
    #[error("the estimate exceeds the settlement limits: {0}")]
    SettlementLimits(String),
    /// The term does not parse or normalize, or an input is inconsistent.
    #[error("the check rejects the offer: {0}")]
    Invalid(String),
}

/// The inputs of the check. Every validator reads the same values: the
/// envelope, the prepared candidate, the selected schedule, the adopted
/// minimum price, the wallet snapshot at R0 and the URN map of the offered
/// runtime. The snapshot depends on the block time through its grant checks
/// (arbitration LOW 10).
pub struct OfferedGateInput<'a> {
    pub envelope: &'a DeployEnvelope,
    pub prepared: &'a PreparedOfferedCandidate<'a>,
    pub selected_terms: &'a [u8],
    pub schedule: &'a PhloScheduleBinding<'a>,
    pub minimum_price: u64,
    pub snapshot: &'a DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
    /// The reducer of the offered runtime: its URN map and its evaluator.
    pub evaluator: &'a ReducerCore,
    /// `offered_funded_v6_production_limits().family` (arbitration LOW 8).
    pub limits: NativeOfferedFamilyLimits,
}

/// The check of one offered deploy (design §6 and the chunks of §9 from
/// ⟨resolve the names of the deploy⟩ to ⟨place the holds or reject⟩).
pub fn gate_offered_candidate(
    input: OfferedGateInput<'_>,
) -> Result<OfferedGate, OfferedGateRejection> {
    check_with_estimate(input, std::convert::identity)
}

/// Added by G1-6 (DR-124), test only: the check of an estimate whose counts
/// and B are removed and whose completeness stays. A complete offer then
/// holds the fee alone and runs under the certified bound 0, so its run
/// passes the bound. This is the analyzer defect of design §7.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn gate_offered_candidate_without_counts(
    input: OfferedGateInput<'_>,
) -> Result<OfferedGate, OfferedGateRejection> {
    check_with_estimate(input, |estimate| CostEstimate {
        quantities: BTreeMap::new(),
        bound_phlo: 0,
        ..estimate
    })
}

fn check_with_estimate(
    input: OfferedGateInput<'_>,
    hook: impl FnOnce(CostEstimate) -> CostEstimate,
) -> Result<OfferedGate, OfferedGateRejection> {
    let DeployEnvelopeRef::OfferedFunded(signed) = input.envelope.view() else {
        return Err(OfferedGateRejection::Invalid(
            "the check needs an offered envelope".to_string(),
        ));
    };
    let funding_signature = funding_sig(signed);
    let estimate = hook(run_estimate(
        input.envelope,
        &funding_signature,
        input.schedule,
        input.evaluator,
    )?);
    let funding = GateFunding::from_snapshot(input.snapshot)?;
    // Added by G1-6 (DR-124): the check holds only the client's demand
    // (P1:2191-2200). The demand of every other lane leaves the estimate, and
    // the estimate is then incomplete.
    // Changed by the G1-6 fix (DR-124 correction): the client lanes are the
    // lanes that the sources may pay, so they include every lane that a
    // source permits.
    // let client = client_lane_set(
    //     funding_signature.lane_hash(),
    //     source_lanes(input.snapshot)?,
    //     &host,
    // )?;
    let client = client_lane_set(funding_signature.lane_hash(), source_lanes(input.snapshot)?);
    let estimate = estimate
        .retain_client_lanes(&client, input.schedule.descriptor())
        .map_err(analysis_rejection)?;
    let onchain = !input
        .snapshot
        .wallets()
        .authorization()
        .onchain_payers()
        .is_empty();
    decide(
        &estimate,
        onchain,
        &GateTerms {
            controls: &input.prepared.intent.base.controls,
            minimum_price: input.minimum_price,
            limit: input.prepared.phlo_limit,
            price: input.prepared.phlo_price,
            schedule: input.schedule,
            selected_terms: input.selected_terms,
        },
        &funding,
        input.limits,
    )
}

/// ⟨read the inputs⟩, ⟨resolve the names of the deploy⟩ and ⟨run the linear
/// analysis⟩: the term, its normalizer environment and its seed come from the
/// envelope, and the funding signature from its signers (`runtime.rs:152`).
// Changed by G1-6 (DR-124): the caller derives the funding signature once,
// because the client lanes need its lane too.
// fn run_estimate(
//     envelope: &DeployEnvelope,
//     schedule: &PhloScheduleBinding<'_>,
//     evaluator: &ReducerCore,
//     host: &HostWorkBudget,
// ) -> Result<CostEstimate, OfferedGateRejection> {
//     let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
//         return Err(OfferedGateRejection::Invalid(
//             "the check needs an offered envelope".to_string(),
//         ));
//     };
//     estimate_term(
//         &envelope.body().term,
//         normalizer_env_from_envelope(envelope),
//         Tools::user_envelope_rng(envelope),
//         &funding_sig(signed),
//         schedule,
//         evaluator,
//         host,
//     )
// }
fn run_estimate(
    envelope: &DeployEnvelope,
    funding: &Sig,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, OfferedGateRejection> {
    estimate_term(
        &envelope.body().term,
        normalizer_env_from_envelope(envelope),
        Tools::user_envelope_rng(envelope),
        funding,
        schedule,
        evaluator,
    )
}

/// Added by G1-6 (DR-124): the lanes that the signed sources may pay. A
/// source is located at the purse of its payer: the supply channel of the
/// payer's signature is its custody location (`supply_reader.rs`).
/// `Sig::lane_hash` digests that supply channel, so the payer's `lane_key`
/// names the location. Changed by the G1-6 fix: the result also holds the
/// lane of each resource permission of each source (`permitted_lanes`).
pub(super) fn source_lanes(
    snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
) -> Result<Vec<[u8; 32]>, OfferedGateRejection> {
    let authorization = snapshot.wallets().authorization();
    let sources = &authorization.record().sources;
    // Changed by the G1-6 fix (DR-124 correction): the room also covers the
    // permitted lanes.
    // let mut lanes = Vec::with_capacity(sources.len());
    let permitted = permitted_lanes(sources)?;
    let mut lanes = Vec::with_capacity(sources.len().saturating_add(permitted.len()));
    for source in sources {
        let custody: [u8; 32] = source.custody().try_into().map_err(|_| {
            OfferedGateRejection::Invalid("a custody key is not 32 bytes".to_string())
        })?;
        let payer = authorization.payers().get(&custody).ok_or_else(|| {
            OfferedGateRejection::Invalid("a signed source has no payer".to_string())
        })?;
        lanes.push(payer.lane_key);
    }
    lanes.extend(permitted);
    Ok(lanes)
}

/// Added by the G1-6 fix (DR-124 correction): the lane of each resource
/// permission of each signed source. A permission for any class makes its
/// lane a client lane. The client's own purse may pay that lane, so the
/// lane's demand is the client's sponsored demand: P2 checks one local
/// linear proof per purse (`prop:local-suff`, P2:1520-1527). The lane key of
/// a permission is the digest of its location (`location_lane_key`).
pub(super) fn permitted_lanes(
    signed: &[PhloSourcePolicyV1<'_>],
) -> Result<Vec<[u8; 32]>, OfferedGateRejection> {
    let count = signed
        .iter()
        .try_fold(0usize, |total, source| {
            total.checked_add(source.resources().len())
        })
        .ok_or_else(|| {
            OfferedGateRejection::Invalid("the count of permitted lanes overflows".to_string())
        })?;
    let mut lanes = Vec::with_capacity(count);
    for source in signed {
        for permission in source.resources() {
            lanes.push(location_lane_key(permission.location));
        }
    }
    Ok(lanes)
}

/// Added by G1-6 (DR-124): the client lanes, the lanes that the sources of
/// the envelope may pay. The deploy lane is always a client lane. A lane is a
/// client lane when a source is located at its purse, that is, when the
/// supply channel of the lane's signature equals the custody location of the
/// source. Changed by the G1-6 fix: a lane is also a client lane when a
/// source permits it for at least one class (`source_lanes`). Fewer permitted
/// classes never exclude a lane that keeps one permission or a located
/// source. A class of a client lane with known demand and no permitting
/// source rejects the offer before execution, when the holds are placed.
pub(super) fn client_lane_set(
    deploy_lane: [u8; 32],
    source_lanes: Vec<[u8; 32]>,
) -> BTreeSet<[u8; 32]> {
    let mut lanes = BTreeSet::new();
    lanes.insert(deploy_lane);
    lanes.extend(source_lanes);
    lanes
}

/// The estimate of one term (`estimate_offered_term`, DR-122).
pub(super) fn estimate_term(
    term: &str,
    normalizer_env: std::collections::HashMap<String, models::rhoapi::Par>,
    rand: crypto::rust::hash::blake2b512_random::Blake2b512Random,
    funding: &Sig,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, OfferedGateRejection> {
    estimate_offered_term(
        term,
        normalizer_env,
        rand,
        funding,
        schedule.descriptor(),
        evaluator,
    )
    .map_err(analysis_rejection)
}

/// The estimate of a resolved term (`linear_cost_estimate`, DR-122), with the
/// error mapping of `estimate_term`.
#[cfg(test)]
pub(super) fn estimate_resolved(
    resolved: &models::rhoapi::Par,
    funding: &Sig,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
) -> Result<CostEstimate, OfferedGateRejection> {
    rholang::rust::interpreter::accounting::delta_sigma::estimate::linear_cost_estimate(
        resolved,
        funding,
        schedule.descriptor(),
        evaluator,
    )
    .map_err(analysis_rejection)
}

/// An error of the parser, the resolver or the analysis means that the term
/// does not parse or normalize, or that an input is malformed (DR-122).
fn analysis_rejection(error: InterpreterError) -> OfferedGateRejection {
    OfferedGateRejection::Invalid(format!("the analysis rejects the term: {error}"))
}

/// The funding side of the check, read from the wallet snapshot at R0.
/// Supply counts SystemVault balances only (design §6.2).
pub(super) struct GateFunding<'f> {
    /// One source for each signed source, in record order: the balance at
    /// R0, the hold cap and the debit cap (`snapshot.rs`).
    pub(super) sources: Vec<PhloFundingSource<'f>>,
    pub(super) signed: &'f [PhloSourcePolicyV1<'f>],
    /// Whether each source is an on-chain purse.
    pub(super) onchain: Vec<bool>,
    /// The record positions of the sources in canonical custody order.
    pub(super) fill_order: Vec<usize>,
    pub(super) total_exposure: u128,
    /// The canonical resource and fee cursors, as `family_selection.rs`
    /// computes them.
    pub(super) cursors: (usize, usize),
}

impl<'f> GateFunding<'f> {
    pub(super) fn from_snapshot(
        snapshot: &'f DirectWalletPolicySnapshot<'f, OfferedFundedDeploy>,
    ) -> Result<Self, OfferedGateRejection> {
        let wallets = snapshot.wallets();
        let record = wallets.authorization().record();
        let count = wallets.sources().len();
        if record.sources.len() != count || snapshot.canonical_custodies().len() != count {
            return Err(OfferedGateRejection::Invalid(
                "the wallet snapshot differs from the signed sources".to_string(),
            ));
        }
        let onchain_payers = wallets.authorization().onchain_payers();
        let mut sources = Vec::with_capacity(count);
        let mut onchain = Vec::with_capacity(count);
        let mut positions = BTreeMap::new();
        for (position, (physical, signed)) in
            wallets.sources().iter().zip(&record.sources).enumerate()
        {
            // Moved by dedup (task 11415) to `rooted_source` in
            // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
            // let source = physical.source();
            // if source.custody != signed.custody() {
            //     return Err(OfferedGateRejection::Invalid(
            //         "a snapshot source differs from its signed source".to_string(),
            //     ));
            // }
            // let custody: [u8; 32] = source.custody.try_into().map_err(|_| {
            //     OfferedGateRejection::Invalid("a custody key is not 32 bytes".to_string())
            // })?;
            let (source, custody) = rooted_source(physical, signed).map_err(setup_rejection)?;
            onchain.push(onchain_payers.contains(&custody));
            positions.insert(source.custody, position);
            sources.push(source);
        }
        let mut fill_order = Vec::with_capacity(count);
        for custody in snapshot.canonical_custodies() {
            fill_order.push(*positions.get(custody).ok_or_else(|| {
                OfferedGateRejection::Invalid(
                    "a canonical custody has no snapshot source".to_string(),
                )
            })?);
        }
        // Moved by dedup (task 11415) to `rooted_cursors` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
        // let payers = NonZeroUsize::new(count).ok_or_else(|| {
        //     OfferedGateRejection::Invalid("the offer has no funding source".to_string())
        // })?;
        // let cursor = |cursor: Option<MonetaryCursor>| {
        //     cursor
        //         .unwrap_or(MonetaryCursor::INITIAL)
        //         .position_index(payers)
        //         .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))
        // };
        // Ok(Self {
        //     sources,
        //     signed: &record.sources,
        //     onchain,
        //     fill_order,
        //     total_exposure: record.total_exposure,
        //     cursors: (
        //         cursor(snapshot.resource_cursor())?,
        //         cursor(snapshot.fee_cursor())?,
        //     ),
        // })
        let cursors = rooted_cursors(snapshot, count).map_err(setup_rejection)?;
        Ok(Self {
            sources,
            signed: &record.sources,
            onchain,
            fill_order,
            total_exposure: record.total_exposure,
            cursors,
        })
    }
}

/// Added by dedup (task 11415, D1): the rejection of a failed step of the
/// rooted funding setup (`family_selection.rs`), with the messages of the
/// check.
fn setup_rejection(error: RootedSetupError) -> OfferedGateRejection {
    OfferedGateRejection::Invalid(match error {
        RootedSetupError::SourceDiffers => {
            "a snapshot source differs from its signed source".to_string()
        }
        RootedSetupError::CustodyWidth => "a custody key is not 32 bytes".to_string(),
        RootedSetupError::NoSources => "the offer has no funding source".to_string(),
        RootedSetupError::Cursor(error) => error.to_string(),
    })
}

/// The signed terms that the controls check reads.
#[derive(Clone, Copy)]
pub(super) struct GateTerms<'t> {
    pub(super) controls: &'t PhloControlsV1<'t>,
    pub(super) minimum_price: u64,
    pub(super) limit: u64,
    pub(super) price: u64,
    pub(super) schedule: &'t PhloScheduleBinding<'t>,
    /// The bytes of the selected schedule, the acquisition terms of every
    /// resource (`producer.rs`).
    pub(super) selected_terms: &'t [u8],
}

/// The decision of the check from the estimate and the funding side.
pub(super) fn decide(
    estimate: &CostEstimate,
    onchain: bool,
    terms: &GateTerms<'_>,
    funding: &GateFunding<'_>,
    limits: NativeOfferedFamilyLimits,
) -> Result<OfferedGate, OfferedGateRejection> {
    let kind = classify(&estimate.completeness, onchain);
    let estimate_phlo = estimate.bound_phlo;
    if estimate_phlo > terms.limit {
        return Err(OfferedGateRejection::EstimateAboveLimit {
            estimate: estimate_phlo,
            limit: terms.limit,
        });
    }
    let (resource_bound, bound_source) = match kind {
        OfferedEstimateKind::Complete => (estimate_phlo, NativeBoundSource::Certificate),
        OfferedEstimateKind::Incomplete(_) => (terms.limit, NativeBoundSource::SignedLimit),
    };
    let remainder = match kind {
        OfferedEstimateKind::Complete => 0,
        OfferedEstimateKind::Incomplete(_) => {
            u128::from(terms.limit - estimate_phlo) * u128::from(terms.price)
        }
    };
    let placed =
        with_estimated_obligations(estimate, terms, resource_bound, limits, |obligations| {
            let eligible = eligibility(obligations, funding, limits)?;
            place_holds(
                obligations,
                &funding.sources,
                &eligible,
                &funding.onchain,
                funding.total_exposure,
                funding.cursors,
                remainder,
                &funding.fill_order,
                limits.policy,
            )
        })?;
    let mut holds = Vec::with_capacity(funding.fill_order.len());
    for position in &funding.fill_order {
        let custody: [u8; 32] = funding.sources[*position].custody.try_into().map_err(|_| {
            OfferedGateRejection::Invalid("a custody key is not 32 bytes".to_string())
        })?;
        holds.push((
            custody,
            u128::from(placed.known[*position]) + placed.remainder[*position],
        ));
    }
    Ok(OfferedGate {
        kind,
        estimate_phlo,
        resource_bound,
        bound_source,
        holds,
    })
}

/// The obligations of the estimate under `resource_bound` (design §6.1 steps
/// 1 to 3): the raw counts fill a counted witness whose required and fresh
/// resources are those counts, the signed controls check the bound, the
/// witness is checked against the controls and the settlement limits, and
/// the projection gives one obligation for each lane and class plus the fee.
/// `then` receives the obligations.
pub(super) fn with_estimated_obligations<R>(
    estimate: &CostEstimate,
    terms: &GateTerms<'_>,
    resource_bound: u64,
    limits: NativeOfferedFamilyLimits,
    then: impl FnOnce(&CheckedPhloObligations<'_>) -> Result<R, OfferedGateRejection>,
) -> Result<R, OfferedGateRejection> {
    let resources = EstimatedResources::new(estimate)?;
    let amounts = resources.amounts(terms.selected_terms);
    let binding = PhloControlsBinding::new(terms.controls, limits.signed.intent.controls())
        .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))?;
    let view = binding
        .view()
        .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))?;
    let offer = PhloOffer {
        limit: i64::try_from(terms.limit).map_err(|_| {
            OfferedGateRejection::Invalid("the signed limit exceeds the native range".to_string())
        })?,
        price: i64::try_from(terms.price).map_err(|_| {
            OfferedGateRejection::Invalid("the signed price exceeds the native range".to_string())
        })?,
    };
    let controls = check_offered_phlo_controls(
        terms.schedule.schedule().environment,
        terms.minimum_price,
        i64::MAX as u64,
        offer,
        view.terms(),
        terms.schedule.schedule(),
        resource_bound,
    )
    .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))?
    .controls();
    let checked = check_counted_phlo_execution(
        controls,
        CountedPhloExecutionWitness {
            available: &[],
            required: &amounts,
            used: &[],
            unused: &[],
            fresh: &amounts,
        },
        limits.measured.settlement.matching.execution,
    )
    .map_err(witness_rejection)?;
    let obligations = project_phlo_obligations(
        checked,
        PhloOutcome::Accepted(&[]),
        limits.policy.funding.obligations,
    )
    .map_err(obligation_rejection)?;
    then(&obligations)
}

/// The kind of an estimate. An offer with an on-chain payer is incomplete
/// (design §5.3), and the analysis's own reason comes first.
pub(super) fn classify(completeness: &Completeness, onchain: bool) -> OfferedEstimateKind {
    match (completeness, onchain) {
        (Completeness::Complete, false) => OfferedEstimateKind::Complete,
        (Completeness::Complete, true) => {
            OfferedEstimateKind::Incomplete(IncompleteReason::OnChainPayer)
        }
        (Completeness::Incomplete(reason), _) => OfferedEstimateKind::Incomplete(reason.clone()),
    }
}

fn witness_rejection(error: PhloExecutionError) -> OfferedGateRejection {
    match error {
        PhloExecutionError::TooManyResourceEntries
        | PhloExecutionError::TooManyAuthorityNodes
        | PhloExecutionError::TooManyKeyBytes => {
            OfferedGateRejection::SettlementLimits(error.to_string())
        }
        other => OfferedGateRejection::Invalid(other.to_string()),
    }
}

fn obligation_rejection(error: PhloObligationError) -> OfferedGateRejection {
    match error {
        PhloObligationError::TooManyObligations | PhloObligationError::KeyEncoding(_) => {
            OfferedGateRejection::SettlementLimits(error.to_string())
        }
        PhloObligationError::Execution(inner) => witness_rejection(inner),
        other => OfferedGateRejection::Invalid(other.to_string()),
    }
}

/// The resources of the witness, keyed as the measured path keys them: the
/// location is the encoded supply channel of the lane's authority
/// (`purses.rs`), the class is the schedule class, and the authority is the
/// lane's signature as a `Sig` (`reduce.rs`, `purses.rs`).
struct EstimatedResources {
    /// The authority and the location of each lane, built once per lane.
    lanes: BTreeMap<[u8; 32], (Sig, Vec<u8>)>,
    /// The lane, the class and the count of each resource.
    entries: Vec<([u8; 32], usize, u64)>,
}

impl EstimatedResources {
    /// Changed by dedup (task 11415, D2): the conversion is `lane_location`
    /// (`producer.rs`), which `born_resource` also calls. Its sort runs on a
    /// heap work stack (DR-131 P1), so a deep lane signature converts without
    /// recursion.
    fn new(estimate: &CostEstimate) -> Result<Self, OfferedGateRejection> {
        let count = estimate.quantities.len();
        // Moved by dedup (task 11415) to `lane_location_metered` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/producer.rs`.
        // let backing = |operations: usize, scanned: usize, allocation: usize| {
        //     let doubled = operations.checked_mul(2).ok_or(BackingError::Overflow)?;
        //     reserve(host, HostWorkDimension::VerificationOperations, doubled)
        //         .map_err(|_| BackingError::Rejected)?;
        //     reserve(host, HostWorkDimension::VerificationBytes, scanned)
        //         .map_err(|_| BackingError::Rejected)?;
        //     reserve(host, HostWorkDimension::SearchStateBytes, allocation)
        //         .map_err(|_| BackingError::Rejected)
        // };
        let conversion = |error: String| {
            OfferedGateRejection::Invalid(format!("a lane signature does not convert: {error}"))
        };
        let mut lanes = BTreeMap::new();
        let mut entries = Vec::with_capacity(count);
        for ((lane, class), quantity) in &estimate.quantities {
            if !lanes.contains_key(lane) {
                let signature = estimate.signatures.get(lane).ok_or_else(|| {
                    OfferedGateRejection::Invalid(
                        "the estimate counted a lane without a signature".to_string(),
                    )
                })?;
                // Moved by dedup (task 11415) to `lane_location_metered` in
                // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/producer.rs`.
                // let authority = cost_signature_to_sig_metered(signature, &backing)
                //     .map_err(|error| conversion(error.to_string()))?;
                // if !authority.is_funding_former() {
                //     return Err(OfferedGateRejection::Invalid(
                //         "a lane signature is not a funding former".to_string(),
                //     ));
                // }
                // let channel = funding_sig_channel_metered(&authority, &backing)
                //     .map_err(|error| conversion(error.to_string()))?;
                // clone_backing::inspect_blocks(&channel, &backing)
                //     .map_err(|error| conversion(format!("{error:?}")))?;
                // let size = channel.encoded_len();
                // reserve(
                //     host,
                //     HostWorkDimension::VerificationOperations,
                //     size.checked_mul(2).ok_or(OfferedGateRejection::HostWork)?,
                // )?;
                // reserve(host, HostWorkDimension::SearchStateBytes, size)?;
                // let mut location = Vec::new();
                // location.try_reserve_exact(size).map_err(|_| {
                //     OfferedGateRejection::Invalid(
                //         "the location of a lane does not fit in memory".to_string(),
                //     )
                // })?;
                // channel
                //     .encode(&mut location)
                //     .map_err(|error| conversion(error.to_string()))?;
                let (authority, location) =
                    lane_location(signature).map_err(|error| match error {
                        LaneLocationError::Authority(error) | LaneLocationError::Channel(error) => {
                            conversion(error.to_string())
                        }
                        LaneLocationError::NotFundingFormer => OfferedGateRejection::Invalid(
                            "a lane signature is not a funding former".to_string(),
                        ),
                        LaneLocationError::Allocation => OfferedGateRejection::Invalid(
                            "the location of a lane does not fit in memory".to_string(),
                        ),
                        LaneLocationError::Encode(error) => conversion(error.to_string()),
                    })?;
                lanes.insert(*lane, (authority, location));
            }
            entries.push((*lane, *class, *quantity));
        }
        Ok(Self { lanes, entries })
    }

    fn amounts<'r>(&'r self, terms: &'r [u8]) -> Vec<PhloResourceAmount<'r>> {
        self.entries
            .iter()
            .filter_map(|(lane, class, quantity)| {
                self.lanes
                    .get(lane)
                    .map(|(authority, location)| PhloResourceAmount {
                        resource: PhloResource {
                            location,
                            class: *class,
                            acquisition_terms: terms,
                            authority,
                        },
                        quantity: *quantity,
                    })
            })
            .collect()
    }
}

/// The eligibility matrix of the selection. An on-chain purse has no
/// observed authority before execution, so it funds the fee only (map gap
/// fill G-5). That rule is conservative: after execution the run can show
/// the purse's authority (arbitration LOW 5).
fn eligibility(
    obligations: &CheckedPhloObligations<'_>,
    funding: &GateFunding<'_>,
    limits: NativeOfferedFamilyLimits,
) -> Result<Vec<Vec<bool>>, OfferedGateRejection> {
    let columns = obligations.keys().len();
    let mut eligible = Vec::with_capacity(funding.sources.len());
    for (signed, onchain) in funding.signed.iter().zip(&funding.onchain) {
        let authority = match onchain {
            true => SourceAuthority::Unobserved,
            false => SourceAuthority::Signed,
        };
        let mut row = Vec::with_capacity(columns);
        // Moved by dedup (task 11415) to `eligibility_row` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
        // for key in obligations.keys() {
        //     row.push(
        //         source_permits(key, signed, authority, limits).map_err(|error| {
        //             host_work_or(host, || OfferedGateRejection::Invalid(error.to_string()))
        //         })?,
        //     );
        // }
        eligibility_row(&mut row, obligations, signed, authority, limits)
            .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))?;
        eligible.push(row);
    }
    Ok(eligible)
}

/// The holds of the check, in record order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlacedHolds {
    /// The known demand and the fee that the selection places on each source.
    pub(super) known: Vec<u64>,
    /// The selection's amount of each obligation on each source.
    #[cfg(test)]
    pub(super) assignment: Vec<Vec<u64>>,
    /// The remainder of each source.
    pub(super) remainder: Vec<u128>,
}

/// ⟨place the holds or reject⟩. The selection places the known demand and the
/// fee on eligible sources at R0 with one requirement (`family_policy.rs`).
/// The remainder of an incomplete estimate then fills the sources in canonical
/// custody order. Each source takes at most its balance, its hold cap and its
/// debit cap, minus its known hold. An on-chain purse takes no part of the
/// remainder, because before execution it funds the fee only (map gap fill
/// G-5). Supply covers all of the hold or none of it (P1:1990-1992).
#[allow(clippy::too_many_arguments)]
pub(super) fn place_holds(
    obligations: &CheckedPhloObligations<'_>,
    sources: &[PhloFundingSource<'_>],
    eligible: &[Vec<bool>],
    onchain: &[bool],
    total_exposure: u128,
    cursors: (usize, usize),
    remainder: u128,
    fill_order: &[usize],
    limits: PhloFamilyFundingLimits,
) -> Result<PlacedHolds, OfferedGateRejection> {
    let count = sources.len();
    if onchain.len() != count {
        return Err(OfferedGateRejection::Invalid(
            "the on-chain flags differ from the sources".to_string(),
        ));
    }
    let mut seen = vec![false; count];
    if fill_order.len() != count
        || fill_order
            .iter()
            .any(|position| *position >= count || std::mem::replace(&mut seen[*position], true))
    {
        return Err(OfferedGateRejection::Invalid(
            "the custody order is not a permutation of the sources".to_string(),
        ));
    }
    let requirements = [PhloFundingRequirement {
        obligations,
        eligible,
    }];
    let selected = select_phlo_funding_family(
        PhloFamilyFundingInput {
            sources,
            outcomes: &requirements,
            total_exposure_limit: total_exposure,
            canonical_resource_cursor: cursors.0,
            canonical_fee_cursor: cursors.1,
        },
        limits,
    )
    .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))?
    .ok_or(OfferedGateRejection::InsufficientPhlogiston(
        InsufficientCause::NoPlacement,
    ))?;
    let known = selected.source_holds().to_vec();
    #[cfg(test)]
    let assignment =
        selected.assignments().first().cloned().ok_or_else(|| {
            OfferedGateRejection::Invalid("the selection has no case".to_string())
        })?;
    let mut parts = vec![0u128; count];
    let mut left = remainder;
    for position in fill_order {
        if left == 0 {
            break;
        }
        if onchain[*position] {
            continue;
        }
        let source = sources[*position];
        let room = u128::from(
            source
                .capacity
                .min(source.exposure_limit)
                .min(source.debit_limit),
        )
        .saturating_sub(u128::from(known[*position]));
        let take = room.min(left);
        parts[*position] = take;
        left -= take;
    }
    if left > 0 {
        return Err(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::RemainderLeft { left },
        ));
    }
    let total = known
        .iter()
        .try_fold(remainder, |total, hold| {
            total.checked_add(u128::from(*hold))
        })
        .ok_or(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::ExposureExceeded {
                total: u128::MAX,
                limit: total_exposure,
            },
        ))?;
    // Kept by dedup (task 11415, D3) as a defensive check. Production does not
    // reach it. The ceiling of an offer is the signed limit times the signed
    // price plus the fee. `prepare_offered_candidate` already rejects an offer
    // whose ceiling exceeds the total exposure (`acceptance.rs:289-295`). The
    // holds never exceed that ceiling.
    if total > total_exposure {
        return Err(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::ExposureExceeded {
                total,
                limit: total_exposure,
            },
        ));
    }
    Ok(PlacedHolds {
        known,
        #[cfg(test)]
        assignment,
        remainder: parts,
    })
}

/// Added by G1-7 (DR-65 item 5, design §8.3): the held-capacity guard of one
/// member at its settlement. Every purse in the debits or in the later holds
/// must satisfy balance_after(p) ≥ debit(p) + later(p), where balance_after
/// is the balance at the settlement root of the member. A purse that the
/// balances do not list has balance 0, and a sum that overflows fails its
/// purse. One offer per block (G1) passes no later holds, so the guard is
/// the condition under which SystemVault reserves each debit. The error
/// lists the failing purses in ascending order. The walk merges the two
/// sorted key sets in one pass, with no recursion.
pub(crate) fn held_capacity_guard(
    balance_after: &BTreeMap<[u8; 32], u128>,
    debits: &BTreeMap<[u8; 32], u128>,
    later_holds: &BTreeMap<[u8; 32], u128>,
) -> Result<(), Vec<[u8; 32]>> {
    let mut failing = Vec::new();
    let mut debit_entries = debits.iter().peekable();
    let mut later_entries = later_holds.iter().peekable();
    loop {
        let (purse, debit, later) = match (debit_entries.peek(), later_entries.peek()) {
            (None, None) => break,
            (Some((purse, debit)), None) => {
                let entry = (**purse, **debit, 0);
                debit_entries.next();
                entry
            }
            (None, Some((purse, later))) => {
                let entry = (**purse, 0, **later);
                later_entries.next();
                entry
            }
            (Some((debit_purse, debit)), Some((later_purse, later))) => {
                match debit_purse.cmp(later_purse) {
                    std::cmp::Ordering::Less => {
                        let entry = (**debit_purse, **debit, 0);
                        debit_entries.next();
                        entry
                    }
                    std::cmp::Ordering::Greater => {
                        let entry = (**later_purse, 0, **later);
                        later_entries.next();
                        entry
                    }
                    std::cmp::Ordering::Equal => {
                        let entry = (**debit_purse, **debit, **later);
                        debit_entries.next();
                        later_entries.next();
                        entry
                    }
                }
            }
        };
        let balance = balance_after.get(&purse).copied().unwrap_or(0);
        match debit.checked_add(later) {
            Some(required) if required <= balance => {}
            _ => failing.push(purse),
        }
    }
    match failing.is_empty() {
        true => Ok(()),
        false => Err(failing),
    }
}

#[cfg(test)]
mod tests;
