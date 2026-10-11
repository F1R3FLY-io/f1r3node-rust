//! G1-4b (DR-123): the check before execution of one offered deploy (design
//! §6). It turns the linear cost estimate of DR-122 and the wallet snapshot at
//! R0 into holds, a resource bound and a bound source, or into a rejection
//! before any runtime entry, with no state change and no charge.

use std::collections::BTreeMap;
use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};
use models::rust::host_work::{HostWorkDimension, HostWorkLimits, HostWorkUnits};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig_metered, funding_sig_channel_metered,
};
use rholang::rust::interpreter::accounting::delta_sigma::estimate::{
    estimate_offered_term, Completeness, CostEstimate, IncompleteReason,
};
use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
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
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::reduce::ReducerCore;
use shared::rust::clone_backing::{self, BackingError};

use super::family_selection::{source_permits, SourceAuthority};
use super::producer::NativeOfferedFamilyLimits;
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
    /// The host-work budget of the check ran out in the parser, the
    /// resolver, the analysis or the selection (design §6.3 item 3).
    #[error("the host-work budget of the check ran out")]
    HostWork,
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
    /// `offered_funded_v6_host_work_limits()`. Each call builds its own budget
    /// from them, so the outcome does not depend on earlier work (DR-102).
    pub host_limits: HostWorkLimits,
}

/// The check of one offered deploy (design §6 and the chunks of §9 from
/// ⟨resolve the names of the deploy⟩ to ⟨place the holds or reject⟩).
pub fn gate_offered_candidate(
    input: OfferedGateInput<'_>,
) -> Result<OfferedGate, OfferedGateRejection> {
    let host = HostWorkBudget::new(input.host_limits);
    let estimate = run_estimate(input.envelope, input.schedule, input.evaluator, &host)?;
    let funding = GateFunding::from_snapshot(input.snapshot, &host)?;
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
        &host,
    )
}

fn reserve(
    host: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), OfferedGateRejection> {
    let units = u64::try_from(amount).map_err(|_| OfferedGateRejection::HostWork)?;
    host.reserve(dimension, HostWorkUnits::new(units))
        .map(|_| ())
        .map_err(|_| OfferedGateRejection::HostWork)
}

/// A failure that the host-work budget caused is a host-work rejection.
fn host_work_or(
    host: &HostWorkBudget,
    otherwise: impl FnOnce() -> OfferedGateRejection,
) -> OfferedGateRejection {
    match host.rejection() {
        Some(_) => OfferedGateRejection::HostWork,
        None => otherwise(),
    }
}

/// ⟨read the inputs⟩, ⟨resolve the names of the deploy⟩ and ⟨run the linear
/// analysis⟩: the term, its normalizer environment and its seed come from the
/// envelope, and the funding signature from its signers (`runtime.rs:152`).
fn run_estimate(
    envelope: &DeployEnvelope,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
    host: &HostWorkBudget,
) -> Result<CostEstimate, OfferedGateRejection> {
    let DeployEnvelopeRef::OfferedFunded(signed) = envelope.view() else {
        return Err(OfferedGateRejection::Invalid(
            "the check needs an offered envelope".to_string(),
        ));
    };
    estimate_term(
        &envelope.body().term,
        normalizer_env_from_envelope(envelope),
        Tools::user_envelope_rng(envelope),
        &funding_sig(signed),
        schedule,
        evaluator,
        host,
    )
}

/// The estimate of one term (`estimate_offered_term`, DR-122).
pub(super) fn estimate_term(
    term: &str,
    normalizer_env: std::collections::HashMap<String, models::rhoapi::Par>,
    rand: crypto::rust::hash::blake2b512_random::Blake2b512Random,
    funding: &Sig,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
    host: &HostWorkBudget,
) -> Result<CostEstimate, OfferedGateRejection> {
    estimate_offered_term(
        term,
        normalizer_env,
        rand,
        funding,
        schedule.descriptor(),
        evaluator,
        host,
    )
    .map_err(|error| analysis_rejection(error, host))
}

/// The estimate of a resolved term (`linear_cost_estimate`, DR-122), with the
/// error mapping of `estimate_term`.
#[cfg(test)]
pub(super) fn estimate_resolved(
    resolved: &models::rhoapi::Par,
    funding: &Sig,
    schedule: &PhloScheduleBinding<'_>,
    evaluator: &ReducerCore,
    host: &HostWorkBudget,
) -> Result<CostEstimate, OfferedGateRejection> {
    rholang::rust::interpreter::accounting::delta_sigma::estimate::linear_cost_estimate(
        resolved,
        funding,
        schedule.descriptor(),
        evaluator,
        host,
    )
    .map_err(|error| analysis_rejection(error, host))
}

/// A host-work error of the parser, the resolver or the analysis is a
/// host-work rejection (design §6.3 item 3). Any other error means that the
/// term does not parse or normalize, or that an input is malformed (DR-122).
fn analysis_rejection(error: InterpreterError, host: &HostWorkBudget) -> OfferedGateRejection {
    match error {
        InterpreterError::HostWorkRejected => OfferedGateRejection::HostWork,
        other => host_work_or(host, || {
            OfferedGateRejection::Invalid(format!("the analysis rejects the term: {other}"))
        }),
    }
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
        host: &HostWorkBudget,
    ) -> Result<Self, OfferedGateRejection> {
        let wallets = snapshot.wallets();
        let record = wallets.authorization().record();
        let count = wallets.sources().len();
        if record.sources.len() != count || snapshot.canonical_custodies().len() != count {
            return Err(OfferedGateRejection::Invalid(
                "the wallet snapshot differs from the signed sources".to_string(),
            ));
        }
        reserve(
            host,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<PhloFundingSource<'_>>() + 2 * size_of::<usize>() + 1)
                .ok_or(OfferedGateRejection::HostWork)?,
        )?;
        reserve(
            host,
            HostWorkDimension::VerificationOperations,
            count.checked_mul(3).ok_or(OfferedGateRejection::HostWork)?,
        )?;
        let onchain_payers = wallets.authorization().onchain_payers();
        let mut sources = Vec::with_capacity(count);
        let mut onchain = Vec::with_capacity(count);
        let mut positions = BTreeMap::new();
        for (position, (physical, signed)) in
            wallets.sources().iter().zip(&record.sources).enumerate()
        {
            let source = physical.source();
            if source.custody != signed.custody() {
                return Err(OfferedGateRejection::Invalid(
                    "a snapshot source differs from its signed source".to_string(),
                ));
            }
            let custody: [u8; 32] = source.custody.try_into().map_err(|_| {
                OfferedGateRejection::Invalid("a custody key is not 32 bytes".to_string())
            })?;
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
        let payers = NonZeroUsize::new(count).ok_or_else(|| {
            OfferedGateRejection::Invalid("the offer has no funding source".to_string())
        })?;
        let cursor = |cursor: Option<MonetaryCursor>| {
            cursor
                .unwrap_or(MonetaryCursor::INITIAL)
                .position_index(payers)
                .map_err(|error| OfferedGateRejection::Invalid(error.to_string()))
        };
        Ok(Self {
            sources,
            signed: &record.sources,
            onchain,
            fill_order,
            total_exposure: record.total_exposure,
            cursors: (
                cursor(snapshot.resource_cursor())?,
                cursor(snapshot.fee_cursor())?,
            ),
        })
    }
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
    host: &HostWorkBudget,
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
    let placed = with_estimated_obligations(
        estimate,
        terms,
        resource_bound,
        limits,
        host,
        |obligations| {
            let eligible = eligibility(obligations, funding, limits, host)?;
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
                host,
            )
        },
    )?;
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
    host: &HostWorkBudget,
    then: impl FnOnce(&CheckedPhloObligations<'_>) -> Result<R, OfferedGateRejection>,
) -> Result<R, OfferedGateRejection> {
    let resources = EstimatedResources::new(estimate, host)?;
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
    /// The conversions are the metered ones of `born_resource`
    /// (`producer.rs`). Their sorter stops at 256 nesting levels, so a deeper
    /// lane signature fails here instead of recursing. The check then rejects
    /// the offer with no charge.
    fn new(estimate: &CostEstimate, host: &HostWorkBudget) -> Result<Self, OfferedGateRejection> {
        let count = estimate.quantities.len();
        reserve(
            host,
            HostWorkDimension::SearchStateBytes,
            count
                .checked_mul(size_of::<([u8; 32], usize, u64)>())
                .ok_or(OfferedGateRejection::HostWork)?,
        )?;
        let backing = |operations: usize, scanned: usize, allocation: usize| {
            let doubled = operations.checked_mul(2).ok_or(BackingError::Overflow)?;
            reserve(host, HostWorkDimension::VerificationOperations, doubled)
                .map_err(|_| BackingError::Rejected)?;
            reserve(host, HostWorkDimension::VerificationBytes, scanned)
                .map_err(|_| BackingError::Rejected)?;
            reserve(host, HostWorkDimension::SearchStateBytes, allocation)
                .map_err(|_| BackingError::Rejected)
        };
        let conversion = |error: String| {
            host_work_or(host, || {
                OfferedGateRejection::Invalid(format!("a lane signature does not convert: {error}"))
            })
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
                let authority = cost_signature_to_sig_metered(signature, &backing)
                    .map_err(|error| conversion(error.to_string()))?;
                if !authority.is_funding_former() {
                    return Err(OfferedGateRejection::Invalid(
                        "a lane signature is not a funding former".to_string(),
                    ));
                }
                let channel = funding_sig_channel_metered(&authority, &backing)
                    .map_err(|error| conversion(error.to_string()))?;
                clone_backing::inspect_blocks(&channel, &backing)
                    .map_err(|error| conversion(format!("{error:?}")))?;
                let size = channel.encoded_len();
                reserve(
                    host,
                    HostWorkDimension::VerificationOperations,
                    size.checked_mul(2).ok_or(OfferedGateRejection::HostWork)?,
                )?;
                reserve(host, HostWorkDimension::SearchStateBytes, size)?;
                let mut location = Vec::new();
                location.try_reserve_exact(size).map_err(|_| {
                    OfferedGateRejection::Invalid(
                        "the location of a lane does not fit in memory".to_string(),
                    )
                })?;
                channel
                    .encode(&mut location)
                    .map_err(|error| conversion(error.to_string()))?;
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
    host: &HostWorkBudget,
) -> Result<Vec<Vec<bool>>, OfferedGateRejection> {
    let columns = obligations.keys().len();
    let cells = funding
        .sources
        .len()
        .checked_mul(columns)
        .ok_or(OfferedGateRejection::HostWork)?;
    reserve(host, HostWorkDimension::SearchStateBytes, cells)?;
    reserve(host, HostWorkDimension::VerificationOperations, cells)?;
    let mut eligible = Vec::with_capacity(funding.sources.len());
    for (signed, onchain) in funding.signed.iter().zip(&funding.onchain) {
        let authority = match onchain {
            true => SourceAuthority::Unobserved,
            false => SourceAuthority::Signed,
        };
        let mut row = Vec::with_capacity(columns);
        for key in obligations.keys() {
            row.push(
                source_permits(key, signed, authority, limits, host).map_err(|error| {
                    host_work_or(host, || OfferedGateRejection::Invalid(error.to_string()))
                })?,
            );
        }
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
    host: &HostWorkBudget,
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
        host,
    )
    .map_err(|error| host_work_or(host, || OfferedGateRejection::Invalid(error.to_string())))?
    .ok_or(OfferedGateRejection::InsufficientPhlogiston(
        InsufficientCause::NoPlacement,
    ))?;
    reserve(
        host,
        HostWorkDimension::SearchStateBytes,
        count
            .checked_mul(size_of::<u128>() + size_of::<u64>())
            .ok_or(OfferedGateRejection::HostWork)?,
    )?;
    reserve(host, HostWorkDimension::VerificationOperations, count)?;
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

#[cfg(test)]
mod tests;
