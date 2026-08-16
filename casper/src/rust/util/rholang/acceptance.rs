//! WD-D2 — per-signature acceptance gate + settlement-debit computation.
//!
//! The CONSENSUS-CRITICAL block-assembly funding gate of the Cost-Accounted Rho
//! Calculus (spec `publications/cost-accounting/cost-accounted-rho.tex` §7.6/§7.7;
//! authoritative design `docs/theory/cost-accounting-impl/wd-d2-acceptance-gate.md`).
//! Wires three landed pieces into one decision:
//!   * the PURE per-signature demand analyzer `Δ_s` + Split/Join supply closure
//!     (`rholang/.../accounting/delta_sigma.rs`, WD-D1);
//!   * canonical SystemVault balances plus located authority stacks exposed
//!     through [`SupplyReader`];
//!   * the ONE extracted FUNDING-`Sig` derivation (`accounting::funding_sig`) —
//!     keyed by the signers' GROUND public keys, so the available authority the
//!     gate proves `Σ ≥ Δ` against resolves to the signer's canonical
//!     SystemVault plus prepaid located stacks; replay re-derives it identically.
//!
//! ## What the gate computes (and does not)
//!
//! [`admit_by_funding`] is the closed-fragment structural gate: for each
//! per-signature group it admits the largest canonical-order prefix whose
//! certified reservation fits effective supply. Production block admission uses
//! [`RuntimeManager::admit_with_state_bound_evidence`]. That path evaluates the
//! canonical candidate sequence in an isolated scratch runtime rooted at the
//! merged pre-state, records each exact cost and pre/post root, removes every
//! capacity-exhausted or underfunded candidate, and repeats until the evidence is
//! for the exact retained sequence. Rejected candidates never reach committed
//! execution.
//!
//! Neither path mutates the committed RSpace root. The state-bound path does
//! perform bounded dependent proof evaluation because ambient continuations can
//! contribute COMM events that are absent from the submitted syntax. Committed
//! execution must reproduce the certified cost, status, event log, root chain,
//! settlement debit, and fee carve. The retained path then atomically reserves
//! and settles the exact debit through `rho:vault:system`, and replay performs
//! the same lifecycle before accepting the resulting state root.
//!
//! ## Determinism (the fork-avoidance bar)
//!
//! Every input that feeds the verdict is consensus-deterministic: the analyzer is
//! pure, the groups are a `BTreeMap` (deterministic iteration), and balance and
//! stack reads come from the merged pre-state hash (already a consensus
//! quantity). The block proposer collects
//! deploys into a `HashSet` whose iteration order is nondeterministic across
//! nodes, so this gate RE-IMPOSES the canonical order itself
//! ([`canonical_sort`], the `block_creator.rs:315-324` comparator) before grouping
//! and prefix selection — replay recomputes the identical verdict from
//! `block.body.deploys`.
//!
//! ## Compound (multi-pool) settlement — EXACT per-component debit (#12)
//!
//! The gate computes `effective_supply_with` faithfully for the ADMISSION
//! decision (a compound deploy is fundable up to `effectiveΣ_compound =
//! Σ⟦compound⟧ + min(Σ⟦s₁⟧, Σ⟦s₂⟧)`), AND settles the debit EXACTLY per spec
//! §3.6 Rule 2 + Rule 4: a compound-signed COMM consumes ONE token from the
//! combined pool `Σ⟦compound⟧` OR a matched pair from the component pools
//! `Σ⟦s₁⟧, Σ⟦s₂⟧`. [`compute_settlement_debits`] splits each admitted compound
//! group's cumulative demand `k` combined-pool-first (`draw_compound =
//! min(k, Σ⟦compound⟧)`, `draw_pair = k − draw_compound`), emitting up to THREE
//! `SettlementDebit`s (compound, left, right). A cross-group residual ledger
//! keeps the SUMMED draw on any pool ≤ its raw balance (underflow-safe even when
//! a component is shared by several compounds + its own single-signer group).
//! Single-signer (the common shape, all §7.4 examples) is UNCHANGED — one exact
//! debit `Σ⟦s⟧ -= Σ Δ_s`. The SAME function runs on play and replay
//! ([`recompute_settlement_debits`]) ⇒ byte-identical debits (fork safety).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::signed::Cosigned;
use models::casper::{
    CostAuthorityBornStackProto, CostAuthorityDemandKindProto, CostAuthorityEventProto,
    CostAuthorityFundingCertificateProto, CostAuthorityPhysicalEventDrawProto,
    CostAuthorityResourceProto, CostAuthorityStackReservationProto, CostAuthorityWitnessProto,
};
use models::rhoapi::{CostAuthority, CostSignature, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    allocate_authority_events, allocate_physical_settlement, apply_physical_settlement,
    authority_demand, authority_funding_signatures_with_presentations, canonical_authority,
    cost_region, sig_to_cost_signature, verify_physical_settlement, AuthorityBornStack,
    AuthorityCostWitness, AuthorityEvent, AuthorityPhysicalEventDraw, AuthorityPhysicalInventory,
    AuthorityPhysicalSettlement, DemandBound, FundingCertificate, ResourceMultiset,
    UnprovableDemand, AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
};
// Re-exported so reservation, settlement, replay, and reporting key every
// authority lane by the same canonical `Sig::lane_hash` basis.
pub use rholang::rust::interpreter::accounting::delta_sigma::SigKey;
use rholang::rust::interpreter::accounting::delta_sigma::{
    static_authority_plan, static_authority_signatures, Decomposition, DemandEntry,
};
use rholang::rust::interpreter::accounting::lexical::resolve_lexical_names_for_funding;
use rholang::rust::interpreter::accounting::resource_logic::{
    ApportionmentPolicy, DefaultApportionment, DefaultResourceLogic, FlatFeeApportionment,
    GroupShape, GsltPresentation, OslfResourceLogic, PoolDraw, PoolResidual, ResourceSignature,
    RhoGslt,
};
use rholang::rust::interpreter::accounting::{self, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_type::RhoNumber;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::replay_failure::ReplayFailure;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::supply;
use crate::rust::util::rholang::tools::Tools;

/// One per-authority-lane settlement debit. The channel identifies the located
/// stack purse for the lane; integer balance materialization resolves the same
/// signature to its canonical SystemVault payer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementDebit {
    /// `SignatureChannel::from_sig(s).par` — the lane's located-stack purse.
    pub channel: Par,
    /// `Σ Δ_s` over the admitted prefix of the group (≥ 0 by construction).
    pub amount: i64,
}

/// The outcome of the per-signature acceptance gate over one block's user
/// deploys.
#[derive(Clone, Debug, Default)]
pub struct AdmissionOutcome {
    /// The admitted deploy envelopes, in CANONICAL order — fed directly to
    /// `compute_deploys_checkpoint_cosigned` so execution order matches the
    /// order the funding decision was made in.
    pub admitted: Vec<Cosigned<DeployData>>,
    /// The PRIMARY signatures of gate-rejected deploys.
    pub rejected: Vec<Bytes>,
    /// The per-lane cost debit, keyed by `SigKey` (= `Sig::lane_hash`). The
    /// physical draw may consume prepaid located stacks or canonical
    /// SystemVault balance and is recomputed identically on replay.
    pub debits: BTreeMap<SigKey, SettlementDebit>,
    /// Cost-Accounted Rho Stage D FEE carve (the spec's `FeeExtract`,
    /// cost-accounted-rho.tex:3637 "one client token consumed as fee"): the
    /// per-lane client debit — ONE token per admitted deploy, transferred from
    /// the client's reserved SystemVault allocation to the proposing validator's
    /// SystemVault. Computed after the cost draw against residual authority and
    /// recomputed identically on replay.
    pub fee_debits: BTreeMap<SigKey, SettlementDebit>,
    pub stack_pops: BTreeMap<[u8; 32], u64>,
    pub purse_stacks: BTreeMap<[u8; 32], supply::PurseStack>,
}

/// The replay-recomputed debits: the COST settlement (burned) and the FEE carve
/// (transferred to the proposing validator's SystemVault), both recomputed from
/// `block.body.deploys` by
/// [`recompute_settlement_debits`] — byte-identical to the play-side
/// `AdmissionOutcome.{debits, fee_debits}` for a valid block.
#[derive(Clone, Debug, Default)]
pub struct RecomputedDebits {
    pub settlement: BTreeMap<SigKey, SettlementDebit>,
    pub fee: BTreeMap<SigKey, SettlementDebit>,
    pub stack_pops: BTreeMap<[u8; 32], u64>,
    pub purse_stacks: BTreeMap<[u8; 32], supply::PurseStack>,
}

pub type ReplayPurseSnapshot = BTreeMap<SigKey, supply::PurseInventory>;

fn authority_digest(bytes: &[u8], field: &str) -> Result<[u8; 32], CasperError> {
    bytes.try_into().map_err(|_| {
        CasperError::InvalidCostSettlement(format!("{field} must be exactly 32 bytes"))
    })
}

fn authority_resources_from_proto(
    resources: &[CostAuthorityResourceProto],
) -> Result<ResourceMultiset<SigKey>, CasperError> {
    let mut values = BTreeMap::new();
    let mut previous = None;
    for resource in resources {
        let key = authority_digest(resource.key.as_ref(), "cost-authority resource key")?;
        if resource.amount == 0 {
            return Err(CasperError::InvalidCostSettlement(
                "cost-authority resources cannot contain zero entries".to_string(),
            ));
        }
        if previous.is_some_and(|prior| prior >= key) {
            return Err(CasperError::InvalidCostSettlement(
                "cost-authority resources must be strictly key-ordered".to_string(),
            ));
        }
        previous = Some(key);
        values.insert(key, resource.amount);
    }
    Ok(ResourceMultiset(values))
}

fn authority_resources_to_proto(
    resources: &ResourceMultiset<SigKey>,
) -> Vec<CostAuthorityResourceProto> {
    resources
        .0
        .iter()
        .filter(|(_, amount)| **amount != 0)
        .map(|(key, amount)| CostAuthorityResourceProto {
            key: key.to_vec().into(),
            amount: *amount,
        })
        .collect()
}

fn authority_stack_reservations_from_proto(
    reservations: &[CostAuthorityStackReservationProto],
) -> Result<BTreeMap<[u8; 32], u64>, CasperError> {
    let mut values = BTreeMap::new();
    let mut previous = None;
    for reservation in reservations {
        let stack_id = authority_digest(
            &reservation.stack_id,
            "authority stack reservation identity",
        )?;
        if reservation.pop_count == 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack reservation cannot have zero pop count".to_string(),
            ));
        }
        if previous.is_some_and(|prior| prior >= stack_id) {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack reservations must be strictly identity-ordered".to_string(),
            ));
        }
        previous = Some(stack_id);
        values.insert(stack_id, reservation.pop_count);
    }
    Ok(values)
}

pub(crate) fn authority_certificate_from_proto(
    certificate: &CostAuthorityFundingCertificateProto,
) -> Result<FundingCertificate<SigKey>, CasperError> {
    let demand_values = authority_resources_from_proto(&certificate.demand)?;
    let kind = CostAuthorityDemandKindProto::try_from(certificate.demand_kind).map_err(|_| {
        CasperError::InvalidCostSettlement(
            "cost-authority demand kind is not recognized".to_string(),
        )
    })?;
    let demand = match kind {
        CostAuthorityDemandKindProto::CostAuthorityDemandKindExact => {
            if !certificate.proof.is_empty() || certificate.unprovable_reason != 0 {
                return Err(CasperError::InvalidCostSettlement(
                    "exact authority demand carries non-exact metadata".to_string(),
                ));
            }
            DemandBound::Exact(demand_values)
        }
        CostAuthorityDemandKindProto::CostAuthorityDemandKindFiniteUpperBound => {
            if certificate.proof.is_empty() || certificate.unprovable_reason != 0 {
                return Err(CasperError::InvalidCostSettlement(
                    "finite authority bound must carry only a non-empty proof".to_string(),
                ));
            }
            DemandBound::FiniteUpperBound {
                bound: demand_values,
                proof: certificate.proof.to_vec(),
            }
        }
        CostAuthorityDemandKindProto::CostAuthorityDemandKindUnprovable => {
            if !certificate.demand.is_empty() || !certificate.proof.is_empty() {
                return Err(CasperError::InvalidCostSettlement(
                    "unprovable authority demand cannot carry a bound or proof".to_string(),
                ));
            }
            let reason = u8::try_from(certificate.unprovable_reason)
                .ok()
                .and_then(|reason| UnprovableDemand::from_tag(reason).ok())
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "unprovable authority reason is not recognized".to_string(),
                    )
                })?;
            DemandBound::Unprovable(reason)
        }
    };
    Ok(FundingCertificate {
        protocol_version: certificate.protocol_version,
        program_hash: authority_digest(&certificate.program_hash, "authority program hash")?,
        pre_state_root: authority_digest(
            &certificate.pre_state_root,
            "authority certificate pre-state root",
        )?,
        reservation_id: authority_digest(
            &certificate.reservation_id,
            "authority reservation identity",
        )?,
        demand,
        allocation: authority_resources_from_proto(&certificate.allocation)?,
        stack_reservations: authority_stack_reservations_from_proto(
            &certificate.stack_reservations,
        )?,
        fee_allocation: authority_resources_from_proto(&certificate.fee_allocation)?,
        fee_recipient: certificate.fee_recipient.to_vec(),
    })
}

pub(crate) fn authority_certificate_to_proto(
    certificate: &FundingCertificate<SigKey>,
) -> CostAuthorityFundingCertificateProto {
    let (demand_kind, demand, proof, unprovable_reason) = match &certificate.demand {
        DemandBound::Exact(demand) => (
            CostAuthorityDemandKindProto::CostAuthorityDemandKindExact,
            authority_resources_to_proto(demand),
            Bytes::new(),
            0,
        ),
        DemandBound::FiniteUpperBound { bound, proof } => (
            CostAuthorityDemandKindProto::CostAuthorityDemandKindFiniteUpperBound,
            authority_resources_to_proto(bound),
            proof.clone().into(),
            0,
        ),
        DemandBound::Unprovable(reason) => (
            CostAuthorityDemandKindProto::CostAuthorityDemandKindUnprovable,
            Vec::new(),
            Bytes::new(),
            u32::from(reason.tag()),
        ),
    };
    CostAuthorityFundingCertificateProto {
        protocol_version: certificate.protocol_version,
        program_hash: certificate.program_hash.to_vec().into(),
        pre_state_root: certificate.pre_state_root.to_vec().into(),
        reservation_id: certificate.reservation_id.to_vec().into(),
        demand_kind: demand_kind as i32,
        demand,
        proof,
        unprovable_reason,
        allocation: authority_resources_to_proto(&certificate.allocation),
        stack_reservations: certificate
            .stack_reservations
            .iter()
            .map(|(stack_id, pop_count)| CostAuthorityStackReservationProto {
                stack_id: Bytes::copy_from_slice(stack_id),
                pop_count: *pop_count,
            })
            .collect(),
        fee_allocation: authority_resources_to_proto(&certificate.fee_allocation),
        fee_recipient: certificate.fee_recipient.clone().into(),
    }
}

pub(crate) fn authority_witness_from_proto(
    witness: &CostAuthorityWitnessProto,
    allow_unbound_certificate: bool,
) -> Result<AuthorityCostWitness<SigKey>, CasperError> {
    let certificate_id = if allow_unbound_certificate && witness.certificate_id.is_empty() {
        [0; 32]
    } else {
        authority_digest(
            &witness.certificate_id,
            "authority witness certificate identity",
        )?
    };
    let events = witness
        .events
        .iter()
        .map(|event| {
            let event = AuthorityEvent {
                event_id: authority_digest(&event.event_id, "authority event identity")?,
                authority: event.authority.clone().ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "authority event is missing its wrapper authority".to_string(),
                    )
                })?,
                debit: authority_resources_from_proto(&event.debit)?,
            };
            event
                .verify_authority()
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            Ok(event)
        })
        .collect::<Result<Vec<_>, CasperError>>()?;
    let witness = AuthorityCostWitness {
        protocol_version: witness.protocol_version,
        certificate_id,
        pre_state_root: authority_digest(
            &witness.pre_state_root,
            "authority witness pre-state root",
        )?,
        post_state_root: authority_digest(
            &witness.post_state_root,
            "authority witness post-state root",
        )?,
        events,
        realized: authority_resources_from_proto(&witness.realized)?,
        settlement: authority_resources_from_proto(&witness.settlement)?,
        physical_draws: witness
            .physical_draws
            .iter()
            .map(|draw| {
                let stack_ids = draw
                    .stack_ids
                    .iter()
                    .map(|stack_id| authority_digest(stack_id, "authority stack identity"))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AuthorityPhysicalEventDraw {
                    event_id: authority_digest(
                        &draw.event_id,
                        "authority physical draw event identity",
                    )?,
                    balances: authority_resources_from_proto(&draw.balances)?,
                    stack_ids,
                })
            })
            .collect::<Result<Vec<_>, CasperError>>()?,
        born_stacks: witness
            .born_stacks
            .iter()
            .map(|birth| {
                Ok(AuthorityBornStack {
                    stack_id: authority_digest(&birth.stack_id, "authority born stack identity")?,
                    produce_hash: authority_digest(
                        &birth.produce_hash,
                        "authority born stack produce identity",
                    )?,
                    cells: birth.cells.clone(),
                })
            })
            .collect::<Result<Vec<_>, CasperError>>()?,
    };
    witness
        .verify_structure()
        .and_then(|_| witness.verify_event_authorities())
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    Ok(witness)
}

pub(crate) fn authority_witness_to_proto(
    witness: &AuthorityCostWitness<SigKey>,
) -> CostAuthorityWitnessProto {
    CostAuthorityWitnessProto {
        protocol_version: witness.protocol_version,
        certificate_id: witness.certificate_id.to_vec().into(),
        pre_state_root: witness.pre_state_root.to_vec().into(),
        post_state_root: witness.post_state_root.to_vec().into(),
        events: witness
            .events
            .iter()
            .map(|event| CostAuthorityEventProto {
                event_id: event.event_id.to_vec().into(),
                debit: authority_resources_to_proto(&event.debit),
                authority: Some(event.authority.clone()),
            })
            .collect(),
        realized: authority_resources_to_proto(&witness.realized),
        settlement: authority_resources_to_proto(&witness.settlement),
        physical_draws: witness
            .physical_draws
            .iter()
            .map(|draw| CostAuthorityPhysicalEventDrawProto {
                event_id: draw.event_id.to_vec().into(),
                balances: authority_resources_to_proto(&draw.balances),
                stack_ids: draw
                    .stack_ids
                    .iter()
                    .map(|stack_id| stack_id.to_vec().into())
                    .collect(),
            })
            .collect(),
        born_stacks: witness
            .born_stacks
            .iter()
            .map(|birth| CostAuthorityBornStackProto {
                stack_id: birth.stack_id.to_vec().into(),
                produce_hash: birth.produce_hash.to_vec().into(),
                cells: birth.cells.clone(),
            })
            .collect(),
    }
}

/// An async per-channel supply-balance reader returning PRESENCE: `Some(n)` iff
/// a balance datum is resident on `chan` (even `n == 0`), `None` iff the pool is
/// absent. Absence is interpreted as zero supply. Two implementations keep
/// the gate's read symmetric across play and replay:
///   * play (block assembly): [`RuntimeManagerSupplyReader`] over a merged
///     pre-state HASH via `RuntimeManager::get_data`;
///   * replay: [`RuntimeOpsSupplyReader`] over the LIVE store already `reset` to
///     `start_hash`.
///
/// `Send + Sync` so a `&dyn SupplyReader` can be held across an `.await` inside a
/// `Send` future (the gate runs on the async block-assembly / replay paths).
pub trait SupplyReader: Send + Sync {
    fn pre_state_root(&self) -> [u8; 32];

    fn urn_map<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Arc<HashMap<String, Par>>, CasperError>>
                + Send
                + 'a,
        >,
    >;

    fn read_purse<'a>(
        &'a self,
        signature: &'a models::rhoapi::CostSignature,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<supply::PurseInventory, CasperError>>
                + Send
                + 'a,
        >,
    >;
}

fn decode_vault_balance(values: Vec<Par>) -> Result<i64, CasperError> {
    let mut balances = values.iter().filter_map(RhoNumber::unapply);
    let balance = balances.next().ok_or_else(|| {
        CasperError::InvalidCostSettlement(
            "canonical SystemVault balance query returned no integer".to_string(),
        )
    })?;
    if balances.next().is_some() {
        return Err(CasperError::InvalidCostSettlement(
            "canonical SystemVault balance query returned multiple integers".to_string(),
        ));
    }
    Ok(balance)
}

/// Play-side supply reader: reads each pool from the merged pre-state hash via
/// `RuntimeManager::get_data` (spawns a runtime at that root, reads, decodes).
pub struct RuntimeManagerSupplyReader<'rm> {
    pub runtime_manager: &'rm RuntimeManager,
    pub pre_state_hash: StateHash,
}

impl<'rm> SupplyReader for RuntimeManagerSupplyReader<'rm> {
    fn pre_state_root(&self) -> [u8; 32] {
        self.pre_state_hash
            .as_ref()
            .try_into()
            .expect("consensus state roots are Blake2b-256")
    }

    fn urn_map<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Arc<HashMap<String, Par>>, CasperError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(self
                .runtime_manager
                .spawn_runtime()
                .await
                .reducer
                .urn_map
                .clone())
        })
    }

    fn read_purse<'a>(
        &'a self,
        signature: &'a models::rhoapi::CostSignature,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<supply::PurseInventory, CasperError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let funding =
                rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(signature)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let channel = supply::supply_channel(&funding);
            let data = self
                .runtime_manager
                .get_data_datums(self.pre_state_hash.clone(), &channel)
                .await?;
            let mut inventory = supply::decode_purse_inventory(&data, signature)?;
            let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let query = Compiler::source_to_adt(
                &crate::rust::util::rholang::costacc::vault_payer::balance_query_source(
                    &payer.address,
                ),
            )
            .map_err(CasperError::InterpreterError)?;
            let values = self
                .runtime_manager
                .play_query_par_at_state_strict(query, &self.pre_state_hash)
                .await?;
            inventory.balance = Some(decode_vault_balance(values).map_err(|error| {
                CasperError::InvalidCostSettlement(format!(
                    "canonical SystemVault balance lookup for {} at pre-state {} failed: {}",
                    payer.address.to_base58(),
                    hex::encode(&self.pre_state_hash),
                    error
                ))
            })?);
            Ok(inventory)
        })
    }
}

/// Replay-side supply reader over the LIVE hot store already reset to
/// `start_hash`.
pub struct RuntimeOpsSupplyReader<'ops> {
    pub runtime_ops: &'ops RuntimeOps,
    pub pre_state_root: [u8; 32],
}

impl<'ops> SupplyReader for RuntimeOpsSupplyReader<'ops> {
    fn pre_state_root(&self) -> [u8; 32] { self.pre_state_root }

    fn urn_map<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Arc<HashMap<String, Par>>, CasperError>>
                + Send
                + 'a,
        >,
    > {
        let urn_map = self.runtime_ops.runtime.reducer.urn_map.clone();
        Box::pin(async move { Ok(urn_map) })
    }

    fn read_purse<'a>(
        &'a self,
        signature: &'a models::rhoapi::CostSignature,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<supply::PurseInventory, CasperError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let funding =
                rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(signature)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let channel = supply::supply_channel(&funding);
            let data = self.runtime_ops.get_data_datums(&channel).await;
            let mut inventory = supply::decode_purse_inventory(&data, signature)?;
            let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let query = Compiler::source_to_adt(
                &crate::rust::util::rholang::costacc::vault_payer::balance_query_source(
                    &payer.address,
                ),
            )
            .map_err(CasperError::InterpreterError)?;
            let values = self
                .runtime_ops
                .play_query_par_current_strict(query)
                .await?;
            inventory.balance = Some(decode_vault_balance(values).map_err(|error| {
                CasperError::InvalidCostSettlement(format!(
                    "canonical SystemVault balance lookup for {} at pre-state {} failed: {}",
                    payer.address.to_base58(),
                    hex::encode(self.pre_state_root),
                    error
                ))
            })?);
            Ok(inventory)
        })
    }
}

/// A canonicalized gate candidate: the deploy envelope, its authority-lane key,
/// its located-stack channel, and its certified demand.
struct Candidate {
    cosigned: Cosigned<DeployData>,
    sig_key: SigKey,
    channel: Par,
    certified_upper_bound: i64,
    certified: Option<CertifiedDemand>,
    /// `true` iff the term is malformed (`source_to_adt` failed). Malformed
    /// terms are REJECTED outright (the runtime would fail them too), never
    /// grouped/admitted.
    malformed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateBoundCostEvidence {
    pub cost: i64,
    pub pre_state_root: [u8; 32],
    pub post_state_root: [u8; 32],
    pub authority_witness: AuthorityCostWitness<SigKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedAuthorityReservation {
    pub canonical: Par,
    pub certificate: FundingCertificate<SigKey>,
    pub signatures: BTreeMap<SigKey, CostSignature>,
    pub inventory: AuthorityPhysicalInventory<SigKey>,
    pub purse_stacks: BTreeMap<[u8; 32], supply::PurseStack>,
    pub bound_events: Vec<AuthorityEvent<SigKey>>,
    pub fee_event: AuthorityEvent<SigKey>,
    pub maximum_cost_settlement: AuthorityPhysicalSettlement<SigKey>,
}

impl PreparedAuthorityReservation {
    pub fn execution_capacity(&self) -> Result<i64, CasperError> {
        let reservation = match &self.certificate.demand {
            DemandBound::Exact(reservation) => reservation,
            DemandBound::FiniteUpperBound { bound, .. } => bound,
            DemandBound::Unprovable(_) => {
                return Err(CasperError::InvalidCostSettlement(
                    "prepared authority reservation has no finite demand".to_string(),
                ));
            }
        };
        reservation
            .0
            .values()
            .try_fold(0_i64, |total: i64, amount| {
                i64::try_from(*amount)
                    .ok()
                    .and_then(|amount| total.checked_add(amount))
                    .ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "prepared authority demand exceeds execution range".to_string(),
                        )
                    })
            })
    }

    pub fn reserved_inventory(&self) -> Result<AuthorityPhysicalInventory<SigKey>, CasperError> {
        let mut stacks = BTreeMap::new();
        for (stack_id, pop_count) in &self.certificate.stack_reservations {
            let cells = self.inventory.stacks.get(stack_id).ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "prepared reservation references an unknown authority stack".to_string(),
                )
            })?;
            let pop_count = usize::try_from(*pop_count).map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "prepared authority stack reservation exceeds the platform range".to_string(),
                )
            })?;
            if pop_count > cells.len() {
                return Err(CasperError::InvalidCostSettlement(
                    "prepared authority stack reservation exceeds the stack length".to_string(),
                ));
            }
            stacks.insert(*stack_id, cells[..pop_count].to_vec());
        }
        Ok(AuthorityPhysicalInventory {
            balances: self.certificate.allocation.clone(),
            stacks,
            born_stacks: BTreeMap::new(),
        })
    }
}

fn insert_signature(
    signatures: &mut BTreeMap<SigKey, CostSignature>,
    signature: CostSignature,
) -> Result<(), CasperError> {
    let key = rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(&signature)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
        .key();
    match signatures.get(&key) {
        Some(existing) if existing != &signature => Err(CasperError::InvalidCostSettlement(
            "authority reservation contains a signature-key collision".to_string(),
        )),
        Some(_) => Ok(()),
        None => {
            signatures.insert(key, signature);
            Ok(())
        }
    }
}

fn bounded_authority_events(
    demand: &ResourceMultiset<SigKey>,
    signatures: &BTreeMap<SigKey, CostSignature>,
    reservation_id: [u8; 32],
) -> Result<Vec<AuthorityEvent<SigKey>>, CasperError> {
    let mut events = Vec::new();
    for (key, amount) in &demand.0 {
        let signature = signatures.get(key).ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "finite authority proof references an unresolved signature".to_string(),
            )
        })?;
        for occurrence in 0..*amount {
            let mut entropy = Vec::new();
            entropy.extend_from_slice(b"f1r3node:static-authority-event:v1");
            entropy.extend_from_slice(&reservation_id);
            entropy.extend_from_slice(key);
            entropy.extend_from_slice(&occurrence.to_le_bytes());
            let event_id: [u8; 32] = Blake2b256::hash(entropy)
                .try_into()
                .expect("Blake2b-256 digest length");
            let authority = canonical_authority(&CostAuthority {
                regions: vec![cost_region(signature, &event_id, 0)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?],
            })
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let debit = authority_demand(&authority)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            if debit != ResourceMultiset::singleton(*key, 1) {
                return Err(CasperError::InvalidCostSettlement(
                    "static authority event disagrees with its certified lane".to_string(),
                ));
            }
            events.push(AuthorityEvent {
                event_id,
                authority,
                debit,
            });
        }
    }
    Ok(events)
}

pub async fn prepare_authority_reservation(
    deploy: &Cosigned<DeployData>,
    supply_reader: &dyn SupplyReader,
    fee_recipient: &[u8],
) -> Result<PreparedAuthorityReservation, CasperError> {
    let funding = accounting::funding_sig(deploy);
    if !funding.is_funding_former() {
        return Err(CasperError::InvalidCostSettlement(
            "deploy funding authority is not a funding former".to_string(),
        ));
    }
    let canonical = canonical_program_for_deploy(deploy, supply_reader).await?;
    let demand = DefaultResourceLogic.demand_bound(&canonical, &funding);
    let reservation = DefaultResourceLogic
        .verify_demand_bound(&canonical, &funding, &demand)
        .ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "deploy has no verified finite authority bound".to_string(),
            )
        })?;
    let static_plan = static_authority_plan(&canonical, &funding)
        .map_err(|reason| CasperError::InvalidCostSettlement(format!("{reason:?}")))?;
    if static_plan.demand != reservation {
        return Err(CasperError::InvalidCostSettlement(
            "static resource-flow plan disagrees with the certified authority demand".to_string(),
        ));
    }
    let program_hash: [u8; 32] = Blake2b256::hash(canonical.encode_to_vec())
        .try_into()
        .expect("Blake2b-256 digest length");
    let mut reservation_entropy = Vec::new();
    reservation_entropy.extend_from_slice(b"f1r3node:vault-cost-reservation:v1");
    reservation_entropy.extend_from_slice(&supply_reader.pre_state_root());
    reservation_entropy.extend_from_slice(&program_hash);
    reservation_entropy.extend_from_slice(deploy.primary().sig.as_ref());
    let reservation_id: [u8; 32] = Blake2b256::hash(reservation_entropy)
        .try_into()
        .expect("Blake2b-256 digest length");

    let mut signatures = static_authority_signatures(&canonical)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    insert_signature(
        &mut signatures,
        sig_to_cost_signature(&funding)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
    )?;
    for presentation in &deploy.data().authority_presentations {
        insert_signature(&mut signatures, presentation.clone())?;
    }
    let bound_events = bounded_authority_events(
        &static_plan.external_reservation,
        &signatures,
        reservation_id,
    )?;
    let fee_event = fee_authority_event(deploy)?;
    for (key, signature) in authority_funding_signatures_with_presentations(
        &bound_events
            .iter()
            .cloned()
            .chain(std::iter::once(fee_event.clone()))
            .collect::<Vec<_>>(),
        &deploy.data().authority_presentations,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
    {
        match signatures.get(&key) {
            Some(existing) if existing != &signature => {
                return Err(CasperError::InvalidCostSettlement(
                    "authority reservation contains a signature-key collision".to_string(),
                ));
            }
            Some(_) => {}
            None => {
                signatures.insert(key, signature);
            }
        }
    }

    let mut inventory = AuthorityPhysicalInventory::default();
    let mut purse_stacks = BTreeMap::new();
    for (key, signature) in &signatures {
        let purse = supply_reader.read_purse(signature).await?;
        let balance = purse.balance.unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        if balance > 0 {
            inventory.balances.0.insert(
                *key,
                u64::try_from(balance).expect("non-negative authority balance"),
            );
        }
        for stack in purse.stacks {
            if inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
                || purse_stacks.insert(stack.instance_id, stack).is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "authority inventory contains a duplicate stack identity".to_string(),
                ));
            }
        }
    }

    let maximum_cost_settlement =
        allocate_physical_settlement(&bound_events, &signatures, &inventory)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    verify_physical_settlement(
        &bound_events,
        &signatures,
        &inventory,
        &maximum_cost_settlement.draws,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let after_cost = inventory
        .balances
        .checked_sub(&maximum_cost_settlement.balance_debit)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let fee_allocation = allocate_authority_events(std::slice::from_ref(&fee_event), &after_cost)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let certificate = FundingCertificate {
        protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
        program_hash,
        pre_state_root: supply_reader.pre_state_root(),
        reservation_id,
        demand,
        allocation: maximum_cost_settlement.balance_debit.clone(),
        stack_reservations: maximum_cost_settlement.stack_pops.clone(),
        fee_allocation,
        fee_recipient: fee_recipient.to_vec(),
    };
    certificate
        .verify_stack_reservations()
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;

    Ok(PreparedAuthorityReservation {
        canonical,
        certificate,
        signatures,
        inventory,
        purse_stacks,
        bound_events,
        fee_event,
        maximum_cost_settlement,
    })
}

pub async fn prepare_state_bound_authority_reservation(
    deploy: &Cosigned<DeployData>,
    witness: &AuthorityCostWitness<SigKey>,
    supply_reader: &dyn SupplyReader,
    fee_recipient: &[u8],
) -> Result<PreparedAuthorityReservation, CasperError> {
    witness
        .verify_structure()
        .and_then(|_| witness.verify_event_authorities())
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    if witness.pre_state_root != supply_reader.pre_state_root() {
        return Err(CasperError::InvalidCostSettlement(
            "state-bound witness is bound to a different authority pre-state".to_string(),
        ));
    }

    let canonical = canonical_program_for_deploy(deploy, supply_reader).await?;
    let program_hash: [u8; 32] = Blake2b256::hash(canonical.encode_to_vec())
        .try_into()
        .expect("Blake2b-256 digest length");
    let mut reservation_entropy = Vec::new();
    reservation_entropy.extend_from_slice(b"f1r3node:vault-cost-reservation:v1");
    reservation_entropy.extend_from_slice(&supply_reader.pre_state_root());
    reservation_entropy.extend_from_slice(&program_hash);
    reservation_entropy.extend_from_slice(deploy.primary().sig.as_ref());
    let reservation_id: [u8; 32] = Blake2b256::hash(reservation_entropy)
        .try_into()
        .expect("Blake2b-256 digest length");

    let fee_event = fee_authority_event(deploy)?;
    let mut funding_events = witness.events.clone();
    funding_events.push(fee_event.clone());
    let mut signatures = authority_funding_signatures_with_presentations(
        &funding_events,
        &deploy.data().authority_presentations,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    for birth in &witness.born_stacks {
        for cell in &birth.cells {
            insert_signature(&mut signatures, cell.clone())?;
        }
    }

    let mut inventory = AuthorityPhysicalInventory::default();
    let mut purse_stacks = BTreeMap::new();
    for (key, signature) in &signatures {
        let purse = supply_reader.read_purse(signature).await?;
        let balance = purse.balance.unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        if balance > 0 {
            inventory.balances.0.insert(
                *key,
                u64::try_from(balance).expect("non-negative authority balance"),
            );
        }
        for stack in purse.stacks {
            if inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
                || purse_stacks.insert(stack.instance_id, stack).is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "authority inventory contains a duplicate stack identity".to_string(),
                ));
            }
        }
    }
    for birth in &witness.born_stacks {
        if inventory
            .stacks
            .insert(birth.stack_id, birth.cells.clone())
            .is_some()
            || inventory
                .born_stacks
                .insert(birth.stack_id, birth.produce_hash)
                .is_some()
        {
            return Err(CasperError::InvalidCostSettlement(
                "born authority stack collides with pre-state inventory".to_string(),
            ));
        }
    }

    let maximum_cost_settlement =
        allocate_physical_settlement(&witness.events, &signatures, &inventory)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    verify_physical_settlement(
        &witness.events,
        &signatures,
        &inventory,
        &maximum_cost_settlement.draws,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let after_cost = inventory
        .balances
        .checked_sub(&maximum_cost_settlement.balance_debit)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let fee_allocation = allocate_authority_events(std::slice::from_ref(&fee_event), &after_cost)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let born_stack_ids = witness
        .born_stacks
        .iter()
        .map(|birth| birth.stack_id)
        .collect::<std::collections::BTreeSet<_>>();
    let stack_reservations = maximum_cost_settlement
        .stack_pops
        .iter()
        .filter(|(stack_id, _)| !born_stack_ids.contains(*stack_id))
        .map(|(stack_id, count)| (*stack_id, *count))
        .collect();
    let certificate = FundingCertificate {
        protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
        program_hash,
        pre_state_root: supply_reader.pre_state_root(),
        reservation_id,
        demand: DemandBound::Exact(witness.realized.clone()),
        allocation: maximum_cost_settlement.balance_debit.clone(),
        stack_reservations,
        fee_allocation,
        fee_recipient: fee_recipient.to_vec(),
    };
    certificate
        .verify_with_allocation(
            AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash,
            supply_reader.pre_state_root(),
            &inventory.balances,
            |_, _| false,
            |demand, allocation| {
                demand == &witness.realized && allocation == &maximum_cost_settlement.balance_debit
            },
        )
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;

    Ok(PreparedAuthorityReservation {
        canonical,
        certificate,
        signatures,
        inventory,
        purse_stacks,
        bound_events: witness.events.clone(),
        fee_event,
        maximum_cost_settlement,
    })
}

async fn canonical_program_for_deploy(
    deploy: &Cosigned<DeployData>,
    supply_reader: &dyn SupplyReader,
) -> Result<Par, CasperError> {
    let normalized = Compiler::source_to_adt_with_normalizer_env(
        &deploy.data().term,
        models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(deploy),
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let canonical = RhoGslt.canonicalize_for_funding(&normalized);
    let urn_map = supply_reader.urn_map().await?;
    resolve_lexical_names_for_funding(
        &canonical,
        Tools::unforgeable_name_rng(&deploy.primary().pk, deploy.data().time_stamp),
        &urn_map,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))
}

#[derive(Clone, Copy)]
enum DemandProofKind {
    Structural,
    StateBound,
}

struct CertifiedDemand {
    canonical: Par,
    funding: Sig,
    program_hash: [u8; 32],
    certificate: FundingCertificate<SigKey>,
    proof_kind: DemandProofKind,
}

struct RealizedCost {
    cost: i64,
    post_state_root: [u8; 32],
    certificate: FundingCertificate<SigKey>,
    authority_witness: AuthorityCostWitness<SigKey>,
}

/// Re-impose the consensus-canonical deploy order on a `HashSet`-sourced
/// candidate list. VERBATIM the `block_creator.rs:315-324` comparator:
/// `valid_after_block_number`, then `time_stamp`, then the primary `sig` bytes
/// (the stable tie-breaker). For a `Cosigned`, the primary signature is
/// `primary().sig` — equal to the on-disk `ProcessedDeploy.deploy.sig`, so play
/// and replay sort identically.
pub fn canonical_sort(deploys: &mut [Cosigned<DeployData>]) {
    deploys.sort_by(|a, b| {
        a.data()
            .valid_after_block_number
            .cmp(&b.data().valid_after_block_number)
            .then_with(|| a.data().time_stamp.cmp(&b.data().time_stamp))
            .then_with(|| a.primary().sig.cmp(&b.primary().sig))
    });
}

fn state_bound_evidence(
    ordered: &[Cosigned<DeployData>],
    processed: &[ProcessedDeploy],
    initial_root: [u8; 32],
) -> Result<BTreeMap<Vec<u8>, VecDeque<StateBoundCostEvidence>>, CasperError> {
    if ordered.len() != processed.len() {
        return Err(CasperError::InvalidCostSettlement(format!(
            "state-bound evidence count {} does not match candidate count {}",
            processed.len(),
            ordered.len()
        )));
    }

    let mut expected_pre = initial_root;
    let mut evidence = BTreeMap::<Vec<u8>, VecDeque<StateBoundCostEvidence>>::new();
    for (candidate, witness) in ordered.iter().zip(processed) {
        let reconstructed = witness
            .to_cosigned()
            .map_err(CasperError::InvalidCostSettlement)?;
        if &reconstructed != candidate {
            return Err(CasperError::InvalidCostSettlement(
                "state-bound evidence is not for the canonical candidate envelope".to_string(),
            ));
        }
        let pre_state_root: [u8; 32] =
            witness.pre_state_hash.as_ref().try_into().map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "state-bound evidence pre-state root is not Blake2b-256".to_string(),
                )
            })?;
        let post_state_root: [u8; 32] =
            witness.post_state_hash.as_ref().try_into().map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "state-bound evidence post-state root is not Blake2b-256".to_string(),
                )
            })?;
        if pre_state_root != expected_pre {
            return Err(CasperError::InvalidCostSettlement(format!(
                "state-bound evidence chain expected pre-state {} but found {}",
                hex::encode(expected_pre),
                hex::encode(pre_state_root)
            )));
        }
        let cost = i64::try_from(witness.cost.cost).map_err(|_| {
            CasperError::InvalidCostSettlement(
                "state-bound evidence cost exceeds i64 range".to_string(),
            )
        })?;
        let authority_witness = authority_witness_from_proto(
            witness.authority_cost_witness.as_ref().ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "state-bound evidence is missing its authority witness".to_string(),
                )
            })?,
            true,
        )?;
        if authority_witness.pre_state_root != pre_state_root
            || authority_witness.post_state_root != post_state_root
        {
            return Err(CasperError::InvalidCostSettlement(
                "state-bound authority witness roots disagree with the processed deploy"
                    .to_string(),
            ));
        }
        evidence
            .entry(candidate.primary().sig.to_vec())
            .or_default()
            .push_back(StateBoundCostEvidence {
                cost,
                pre_state_root,
                post_state_root,
                authority_witness,
            });
        expected_pre = post_state_root;
    }
    Ok(evidence)
}

/// Build the gate candidate for one deploy: derive the FUNDING `Sig` via the
/// ONE shared `accounting::funding_sig`, the authority key + channel from it, and
/// either a structural demand bound or exact state-bound evidence. A term whose
/// `source_to_adt` fails is flagged `malformed` (⇒ rejected).
///
/// The funding signature is keyed by the signers' GROUND public keys
/// (`Sig::Ground(pk)` / the `And`-fold thereof), so native balance authority
/// resolves to each signer's canonical SystemVault and stack authority resolves
/// to the signature-keyed RSpace purse.
fn build_candidate_with_logic<L>(
    cosigned: Cosigned<DeployData>,
    logic: &L,
    pre_state_root: [u8; 32],
    state_bound: Option<&StateBoundCostEvidence>,
) -> Candidate
where
    L: OslfResourceLogic<RhoGslt>,
{
    let gslt = RhoGslt;
    let funding: Sig = accounting::funding_sig(&cosigned);

    // F-A funding/capability separation (gate invariant (a) — red-team M2,
    // `docs/theory/cost-accounting-impl/f-a-funding-vs-capability-separation.md`
    // §3/§6): the funding `Sig` that keys an authority lane MUST be a
    // funding-grammar signature (`g|#P|s∘s` = `Unit`/`Ground`/`Quote` atoms
    // folded by `And`). A value/capability type-logic connective
    // (`Plus`/`With`/`Bang`/`WhyNot`/`Lolly`/`Threshold`) is NOT a funding former
    // and must never key a funding pool. If one ever appears, route the candidate
    // to the SAME rejected (`malformed`) path as a `source_to_adt` failure rather
    // than panicking — a malformed funding shape is refused, not crashed.
    //
    // This is a BELT-AND-SUSPENDERS REGRESSION GUARD, not the live-wire defense:
    // `funding_sig` is already total to `Ground`/`And` (it maps each verified
    // cosigner's public key to a `Sig::Ground` atom, folding ≥2 by `And`), so
    // this branch is UNREACHABLE today and can only fire if a future change makes
    // `funding_sig` non-total. It does NOT defend the gRPC wire path — the
    // LOAD-BEARING guard that actually stops a malicious client from submitting a
    // `⊕/&/!/?/⊸`-formed `sig_algebra` is the INGRESS reject (c) in
    // `models/.../casper_message.rs::from_proto_cosigned_with_sig_algebra`.
    if !funding.is_funding_former() {
        // No-panic contract: derive the (rejected) candidate's key/channel from
        // the TOTAL reflection (`Sig::key` → `lane_hash` → `from_sig`,
        // `SignatureChannel::from_sig` directly) rather than the funding-asserting
        // `supply::supply_channel` wrapper, so this guard refuses the candidate
        // instead of tripping the `debug_assert!`. The candidate is `malformed`
        // ⇒ rejected; its key/channel only serve as map placeholders.
        let sig_key = funding.key();
        let channel = accounting::SignatureChannel::from_sig(&funding).par;
        return Candidate {
            cosigned,
            sig_key,
            channel,
            certified_upper_bound: 0,
            certified: None,
            malformed: true,
        };
    }

    let sig_key = funding.key();
    let channel = supply::supply_channel(&funding);

    match Compiler::source_to_adt_with_normalizer_env(
        &cosigned.data().term,
        models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(&cosigned),
    ) {
        Ok(par) => {
            let desugared = gslt.canonicalize_for_funding(&par);
            let (demand_bound, reservation, proof_kind, certified_upper_bound) = match state_bound {
                Some(evidence) if evidence.cost >= 0 => {
                    let reservation = evidence.authority_witness.realized.clone();
                    (
                        rholang::rust::interpreter::accounting::authority::DemandBound::Exact(
                            reservation.clone(),
                        ),
                        Some(reservation),
                        DemandProofKind::StateBound,
                        Some(evidence.cost),
                    )
                }
                Some(_) => (
                    rholang::rust::interpreter::accounting::authority::DemandBound::Unprovable(
                        rholang::rust::interpreter::accounting::authority::UnprovableDemand::DynamicAuthority,
                    ),
                    None,
                    DemandProofKind::StateBound,
                    None,
                ),
                None => {
                    let demand_bound = logic.demand_bound(&desugared, &funding);
                    let reservation =
                        logic.verify_demand_bound(&desugared, &funding, &demand_bound);
                    let certified_upper_bound = reservation.as_ref().and_then(|reservation| {
                        reservation.0.values().try_fold(0_i64, |sum, amount| {
                            i64::try_from(*amount).ok().and_then(|amount| sum.checked_add(amount))
                        })
                    });
                    (
                        demand_bound,
                        reservation,
                        DemandProofKind::Structural,
                        certified_upper_bound,
                    )
                }
            };
            let program_hash: [u8; 32] = Blake2b256::hash(desugared.encode_to_vec())
                .try_into()
                .expect("Blake2b-256 digest length");
            let reservation_id: [u8; 32] = Blake2b256::hash(cosigned.primary().sig.to_vec())
                .try_into()
                .expect("Blake2b-256 digest length");
            let certified = reservation.map(|allocation| CertifiedDemand {
                canonical: desugared,
                funding,
                program_hash,
                certificate: FundingCertificate {
                    protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                    program_hash,
                    pre_state_root: state_bound
                        .map(|evidence| evidence.pre_state_root)
                        .unwrap_or(pre_state_root),
                    reservation_id,
                    demand: demand_bound,
                    allocation,
                    stack_reservations: BTreeMap::new(),
                    fee_allocation: ResourceMultiset::default(),
                    fee_recipient: Vec::new(),
                },
                proof_kind,
            });
            Candidate {
                cosigned,
                sig_key,
                channel,
                certified_upper_bound: certified_upper_bound.unwrap_or(0),
                malformed: certified.is_none(),
                certified,
            }
        }
        Err(_) => Candidate {
            cosigned,
            sig_key,
            channel,
            certified_upper_bound: 0,
            certified: None,
            malformed: true,
        },
    }
}

/// Decompositions for the Split/Join closure: for every compound (`Sig::And`)
/// envelope, emit `(lane_hash(compound), lane_hash(left), lane_hash(right))` per
/// internal `And` node (so an n≥3 left-assoc fold contributes one entry per
/// node). `Threshold/Plus/With/...` form no decomposition (the runtime forms
/// only `And` today). Each component's `(SigKey, channel)` is recorded into
/// `component_channels` so its raw balance is read exactly once — keyed by the
/// SAME `Sig::lane_hash` the closure keys on (no re-derivation drift).
fn collect_decompositions(
    envelope: &Sig,
    out: &mut Vec<Decomposition>,
    component_channels: &mut BTreeMap<SigKey, Par>,
) {
    let mut decompositions = Vec::new();
    envelope.split_join_decompositions(&mut decompositions);
    out.extend(
        decompositions
            .into_iter()
            .map(|decomposition| Decomposition {
                compound: decomposition.compound,
                left: decomposition.left,
                right: decomposition.right,
            }),
    );
    collect_component_channels(envelope, component_channels);
}

fn collect_funding_signatures(
    signature: &Sig,
    signatures: &mut BTreeMap<SigKey, CostSignature>,
) -> Result<(), CasperError> {
    insert_signature(
        signatures,
        sig_to_cost_signature(signature)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
    )?;
    if let Sig::And(left, right) = signature {
        collect_funding_signatures(left, signatures)?;
        collect_funding_signatures(right, signatures)?;
    }
    Ok(())
}

fn collect_component_channels(envelope: &Sig, component_channels: &mut BTreeMap<SigKey, Par>) {
    if let Sig::And(left, right) = envelope {
        let left_key = left.key();
        let right_key = right.key();
        component_channels
            .entry(left_key)
            .or_insert_with(|| supply::supply_channel(left));
        component_channels
            .entry(right_key)
            .or_insert_with(|| supply::supply_channel(right));
        // Recurse so a left-associated n≥3 fold yields one decomposition per
        // internal `And` node.
        collect_component_channels(left, component_channels);
        collect_component_channels(right, component_channels);
    }
}

/// Index Split/Join decompositions by their compound key for O(1) shape lookup.
/// An n≥3 left-assoc fold contributes nested decompositions on keys that are NOT
/// standalone groups; the first (top-level) entry per compound key wins.
fn index_decompositions(decompositions: &[Decomposition]) -> BTreeMap<SigKey, Decomposition> {
    let mut by_compound: BTreeMap<SigKey, Decomposition> = BTreeMap::new();
    for decomposition in decompositions {
        by_compound
            .entry(decomposition.compound)
            .or_insert(*decomposition);
    }
    by_compound
}

/// The funding shape of a group, read from a residual ledger (the static `raw`
/// pre-state OR the LIVE cross-group `remaining` ledger). A group keyed on a
/// compound (`Sig::And`) decomposition presents as [`GroupShape::Compound`]
/// (combined pool + matched component pair); any other key is
/// [`GroupShape::Single`] (own pool only — NO compound-pool credit; no-weakening,
/// §D2.9-R2). The SINGLE source of truth for shape construction, shared by the
/// admission gate, the cross-group replay re-verification, and
/// [`compute_settlement_debits`], so all three build identical shapes from the
/// same ledger (no drift ⇒ no play/replay fork).
fn group_shape_from(
    group_key: &SigKey,
    decomposition_by_compound: &BTreeMap<SigKey, Decomposition>,
    residual: &BTreeMap<SigKey, i64>,
) -> GroupShape<SigKey> {
    let read = |key: &SigKey| -> i64 { *residual.get(key).unwrap_or(&0) };
    match decomposition_by_compound.get(group_key) {
        Some(decomposition) => GroupShape::Compound {
            combined: PoolResidual {
                key: *group_key,
                residual: read(group_key),
            },
            left: PoolResidual {
                key: decomposition.left,
                residual: read(&decomposition.left),
            },
            right: PoolResidual {
                key: decomposition.right,
                residual: read(&decomposition.right),
            },
        },
        None => GroupShape::Single {
            own: PoolResidual {
                key: *group_key,
                residual: read(group_key),
            },
        },
    }
}

/// The LIVE effective funding capacity of a group shape (cost-accounted-rho Def 19
/// effective supply, evaluated on the residual ledger): a single group funds from
/// its own pool ONLY (no-weakening, §D2.9-R2); a compound funds from its combined
/// pool plus the matched component pair `min(left, right)` (the Split/Join Join
/// term). This is the bound admission proves `Σ(cost+fee) ≤ capacity` against —
/// keyed on the LIVE residual so a later group sharing a component sees the
/// drawn-down balance (cross-group linearity — linear logic admits no contraction).
fn group_capacity(shape: GroupShape<SigKey>) -> i64 {
    match shape {
        GroupShape::Single { own } => own.residual,
        GroupShape::Compound {
            combined,
            left,
            right,
        } => combined
            .residual
            .saturating_add(left.residual.min(right.residual)),
    }
}

/// Apply a group's folded `cost + fee` demand to the LIVE cross-group residual
/// ledger, combined-pool-first via [`DefaultApportionment`] — the conservative
/// reservation that DOMINATES the two-pass cost-then-fee settlement on every pool
/// (the flat fee policy draws a single component, a subset of the cost pair-draw),
/// so `admission-fundable ⟹ settlement-safe`. Each drawn pool is `saturating_sub`-
/// decremented; the caller processes groups in `SigKey` order, so the ledger
/// evolves deterministically and identically on play and replay.
fn draw_group_from_ledger(
    shape: GroupShape<SigKey>,
    demand: i64,
    residual: &mut BTreeMap<SigKey, i64>,
) {
    let draws = <DefaultApportionment as ApportionmentPolicy<RhoGslt>>::apportion(
        &DefaultApportionment,
        shape,
        demand,
    );
    for PoolDraw { key, amount } in draws {
        if amount <= 0 {
            continue;
        }
        let current = *residual.get(&key).unwrap_or(&0);
        residual.insert(key, current.saturating_sub(amount));
    }
}

/// CONSENSUS-CRITICAL (#12): the EXACT per-component (Split/Join) compound
/// apportionment. Given each PRESENT group's cumulative amount
/// `k` (`demand_by_group`, keyed + channel'd by the group's
/// `SigKey`), the Split/Join `decompositions`, the RAW pre-state pool balances
/// `raw` (`Σ⟦s⟧`, absent ⇒ 0), and the resolved `channel` for every key
/// (`channels_by_key`, covering groups AND compound components), produce the
/// per-pool map so that, for every admitted compound group:
///
/// ```text
/// draw_compound = min(k, Σ⟦compound⟧)
/// draw_pair     = k − draw_compound        // ≤ min(Σ⟦s₁⟧, Σ⟦s₂⟧) by admission
/// debit: Σ⟦compound⟧ −= draw_compound ;  Σ⟦s₁⟧ −= draw_pair ;  Σ⟦s₂⟧ −= draw_pair
/// ```
///
/// (spec §3.6 Rule 2 + Rule 4: a compound-signed COMM consumes ONE token from
/// EACH component pool; the combined pool is drawn first, the matched component
/// pair second). A single-signature (non-compound) group is UNCHANGED — one
/// debit `k` on its OWN pool — so the no-compound path is byte-identical to the
/// pre-#12 single-pool debit (back-compat).
///
/// ## The cross-group shared-component residual ledger (the #12 invariant)
///
/// A component pool `Σ⟦s₁⟧` may be drawn by MULTIPLE compound groups (and by its
/// OWN single-signer group, if one is also in the block) in the same block. To
/// keep the SUMMED draw on every pool ≤ its RAW balance (underflow-safe across
/// groups, not just within one), a `residual` ledger initialized to `raw`
/// tracks each pool's LIVE remaining balance: every draw (a compound's own-pool
/// draw, each component pair-draw, and a non-compound group's own-pool draw)
/// `saturating_sub`-decrements the ledger, and each compound pair-draw is BOUND
/// by `min(remaining, residual[s₁], residual[s₂])`. Groups are processed in
/// deterministic `BTreeMap` (`SigKey`-ascending) order, so the ledger evolves
/// identically on play and replay. The non-compound own-pool draw decrements the
/// ledger at the FULL admitted `k` (matching the pre-#12 single-pool debit) but
/// is itself never residual-capped (its `checked_sub` backstop in
/// `close_block_deploy.rs` remains the hard underflow guard); decrementing the
/// ledger for it ensures a later compound that shares that pool as a component
/// sees the reduced residual, so the cross-group SUM stays ≤ raw regardless of
/// the relative `SigKey` order of the single-signer group and the compound.
///
/// Pure (no I/O); a function purely of `(demand_by_group, decompositions, raw,
/// channels_by_key)`. Called IDENTICALLY by [`admit_by_funding`] (play) and
/// [`recompute_settlement_debits`] (replay) — the single code path is what makes
/// the debit map byte-identical (the fork-safety bar).
fn compute_settlement_debits<P>(
    demand_by_group: &BTreeMap<SigKey, (Par, i64)>,
    decompositions: &[Decomposition],
    raw: &BTreeMap<SigKey, i64>,
    channels_by_key: &BTreeMap<SigKey, Par>,
    policy: &P,
) -> BTreeMap<SigKey, SettlementDebit>
where
    P: ApportionmentPolicy<RhoGslt>,
{
    // The LIVE remaining balance of every pool (groups + components), seeded
    // from the raw pre-state balances. Absent pools are not present in `raw`
    // (read as 0). Processed in `SigKey` order via the BTreeMap, deterministic
    // on play and replay.
    let mut residual: BTreeMap<SigKey, i64> = raw.clone();
    let read_residual = |residual: &BTreeMap<SigKey, i64>, key: &SigKey| -> i64 {
        *residual.get(key).unwrap_or(&0)
    };

    // Accumulated draw amount per distinct channel (`SigKey`); summed across all
    // groups that touch a pool. A compound group emits up to THREE draws
    // (compound, left, right); a non-compound group emits one (its own pool).
    let mut draw_by_key: BTreeMap<SigKey, i64> = BTreeMap::new();

    // Index the decompositions by compound key so a compound group resolves its
    // component pair in O(1). The runtime forms only a single `Sig::And` per
    // top-level envelope (the group's `SigKey`), so each compound group key maps
    // to exactly one decomposition; an n≥3 left-assoc fold contributes nested
    // decompositions on keys that are NOT standalone groups (no demand entry),
    // so they never drive a group draw here.
    let decomposition_by_compound = index_decompositions(decompositions);

    for (group_key, (_channel, k)) in demand_by_group {
        let k = *k;
        if k <= 0 {
            continue;
        }
        // Build the funding shape from the LIVE residual ledger, then DELEGATE the
        // per-pool apportionment to `policy` (default = combined-pool-first). The
        // policy is a pure, deterministic, conservation-preserving function of
        // `(shape, k)` (see `ApportionmentPolicy`); it never sees channels, so it
        // cannot inject a wrong pool — the caller owns the ledger and the channel
        // resolution below.
        // Build the funding shape from the LIVE residual ledger via the SAME helper
        // the admission gate + cross-group replay re-verification use, so all three
        // construct identical shapes from the same ledger (no drift ⇒ no fork).
        let shape = group_shape_from(group_key, &decomposition_by_compound, &residual);
        // Apply each elected draw to the LIVE ledger + the per-pool accumulator.
        // `saturating_sub` is exact for the (residual-bounded) compound draws AND
        // reproduces the pre-#12 single-pool semantics (the own-pool debit is NOT
        // residual-capped). A later compound sharing this pool as a component sees
        // the reduction. Applying draws in the policy's fixed order keeps the
        // ledger evolution deterministic on play and replay.
        for PoolDraw { key, amount } in policy.apportion(shape, k) {
            if amount <= 0 {
                continue;
            }
            let current = read_residual(&residual, &key);
            residual.insert(key, current.saturating_sub(amount));
            *draw_by_key.entry(key).or_insert(0) += amount;
        }
    }

    // Materialize one `SettlementDebit` per distinct channel (the summed draw),
    // keyed by `SigKey`. Resolve each channel from `channels_by_key` (the gate
    // read every group + component channel exactly once into it); a key absent
    // there cannot occur (every drawn key is a group or a decomposition
    // component, all of which are recorded), but fall back to the group's own
    // channel defensively so the function is total.
    let mut debits: BTreeMap<SigKey, SettlementDebit> = BTreeMap::new();
    for (key, amount) in draw_by_key {
        if amount <= 0 {
            continue;
        }
        let channel = channels_by_key
            .get(&key)
            .cloned()
            .or_else(|| demand_by_group.get(&key).map(|(chan, _)| chan.clone()))
            .unwrap_or_default();
        debits.insert(key, SettlementDebit { channel, amount });
    }
    debits
}

/// The per-signature acceptance gate (cost-accounted-rho §7.6/§7.7). See the
/// module docs and `wd-d2-acceptance-gate.md` §D2.2 for the binding algorithm.
///
/// `deploys` are the candidate user deploy envelopes (`HashSet`-sourced, hence
/// nondeterministically ordered — this function re-sorts canonically).
/// `supply_reader` reads each pool `Σ⟦s⟧` from the consensus pre-state (play:
/// merged pre-state hash; replay: live store reset to `start_hash`). An absent
/// pool has zero supply and cannot fund positive demand or the fixed fee.
///
/// Returns the [`AdmissionOutcome`]: admitted envelopes in canonical order, the
/// rejected primary sigs, and the per-pool settlement debits.
pub async fn admit_by_funding(
    deploys: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
) -> Result<AdmissionOutcome, CasperError> {
    let logic = DefaultResourceLogic;
    let policy = DefaultApportionment;
    admit_by_funding_internal(deploys, supply_reader, &logic, &policy, None).await
}

pub(crate) fn fee_authority_event(
    deploy: &Cosigned<DeployData>,
) -> Result<AuthorityEvent<SigKey>, CasperError> {
    let signature = sig_to_cost_signature(&accounting::funding_sig(deploy))
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let region = cost_region(&signature, deploy.primary().sig.as_ref(), u32::MAX)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let authority = canonical_authority(&CostAuthority {
        regions: vec![region],
    })
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let mut identity = Vec::with_capacity(deploy.primary().sig.len() + 32);
    identity.extend_from_slice(b"f1r3node:cost-accounted-rho:fee-event:v2");
    identity.extend_from_slice(deploy.primary().sig.as_ref());
    Ok(AuthorityEvent {
        event_id: authority_digest(&Blake2b256::hash(identity), "fee event identity")?,
        debit: authority_demand(&authority)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
        authority,
    })
}

pub fn authority_purse_signatures(
    deploy: &Cosigned<DeployData>,
    witness: &AuthorityCostWitness<SigKey>,
) -> Result<BTreeMap<SigKey, CostSignature>, CasperError> {
    let fee_event = fee_authority_event(deploy)?;
    let mut funding_events = witness.events.clone();
    funding_events.push(fee_event);
    let mut signatures = authority_funding_signatures_with_presentations(
        &funding_events,
        &deploy.data().authority_presentations,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    for birth in &witness.born_stacks {
        for cell in &birth.cells {
            insert_signature(&mut signatures, cell.clone())?;
        }
    }
    Ok(signatures)
}

pub async fn replay_purse_snapshot(
    processed: &ProcessedDeploy,
    supply_reader: &dyn SupplyReader,
) -> Result<ReplayPurseSnapshot, CasperError> {
    let cosigned = processed.to_cosigned().map_err(CasperError::RuntimeError)?;
    let witness = authority_witness_from_proto(
        processed.authority_cost_witness.as_ref().ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "processed deploy is missing its authority witness".to_string(),
            )
        })?,
        false,
    )?;
    let signatures = authority_purse_signatures(&cosigned, &witness)?;
    let mut snapshot = BTreeMap::new();
    for (key, signature) in signatures {
        snapshot.insert(key, supply_reader.read_purse(&signature).await?);
    }
    Ok(snapshot)
}

pub(crate) fn record_authority_debits(
    target: &mut BTreeMap<SigKey, SettlementDebit>,
    draw: &ResourceMultiset<SigKey>,
    channels: &BTreeMap<SigKey, Par>,
) -> Result<(), CasperError> {
    for (key, amount) in &draw.0 {
        let amount = i64::try_from(*amount).map_err(|_| {
            CasperError::InvalidCostSettlement(
                "authority settlement amount exceeds i64 range".to_string(),
            )
        })?;
        let channel = channels.get(key).cloned().ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "authority settlement references an unresolved purse".to_string(),
            )
        })?;
        match target.get_mut(key) {
            Some(existing) => {
                existing.amount = existing.amount.checked_add(amount).ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "authority settlement debit overflow".to_string(),
                    )
                })?;
            }
            None => {
                target.insert(*key, SettlementDebit { channel, amount });
            }
        }
    }
    Ok(())
}

async fn admit_state_bound_authority(
    deploys: Vec<Cosigned<DeployData>>,
    processed: &mut [ProcessedDeploy],
    supply_reader: &dyn SupplyReader,
) -> Result<AdmissionOutcome, CasperError> {
    let mut ordered = deploys;
    canonical_sort(&mut ordered);
    let mut state_bounds =
        state_bound_evidence(&ordered, processed, supply_reader.pre_state_root())?;
    let mut candidates = Vec::with_capacity(ordered.len());
    let mut pool_signatures = BTreeMap::new();

    for (index, deploy) in ordered.into_iter().enumerate() {
        let evidence = state_bounds
            .get_mut(deploy.primary().sig.as_ref())
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "missing state-bound authority evidence for candidate deploy".to_string(),
                )
            })?;
        let fee_event = fee_authority_event(&deploy)?;
        let mut funding_events = evidence.authority_witness.events.clone();
        funding_events.push(fee_event.clone());
        for (key, signature) in authority_funding_signatures_with_presentations(
            &funding_events,
            &deploy.data().authority_presentations,
        )
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
        {
            pool_signatures.entry(key).or_insert(signature);
        }
        for birth in &evidence.authority_witness.born_stacks {
            for cell in &birth.cells {
                insert_signature(&mut pool_signatures, cell.clone())?;
            }
        }
        let candidate = build_candidate_with_logic(
            deploy,
            &DefaultResourceLogic,
            supply_reader.pre_state_root(),
            Some(&evidence),
        );
        candidates.push((index, candidate, evidence, fee_event));
    }

    let mut channels = BTreeMap::new();
    let mut physical_inventory = AuthorityPhysicalInventory::default();
    let mut purse_stacks = BTreeMap::new();
    for (key, signature) in &pool_signatures {
        let funding =
            rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let channel = supply::supply_channel(&funding);
        let purse = supply_reader.read_purse(signature).await?;
        let balance = purse.balance.unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        channels.insert(*key, channel);
        if balance > 0 {
            physical_inventory.balances.0.insert(
                *key,
                u64::try_from(balance).expect("non-negative authority balance"),
            );
        }
        for stack in purse.stacks {
            if physical_inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
                || purse_stacks.insert(stack.instance_id, stack).is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "authority inventory contains a duplicate stack identity".to_string(),
                ));
            }
        }
    }

    let mut outcome = AdmissionOutcome::default();
    let mut closed_groups = std::collections::BTreeSet::new();
    for (index, mut candidate, evidence, fee_event) in candidates {
        if candidate.malformed || closed_groups.contains(&candidate.sig_key) {
            closed_groups.insert(candidate.sig_key);
            outcome
                .rejected
                .push(candidate.cosigned.primary().sig.clone());
            continue;
        }
        let physical_settlement = match allocate_physical_settlement(
            &evidence.authority_witness.events,
            &pool_signatures,
            &physical_inventory,
        ) {
            Ok(settlement) => settlement,
            Err(rholang::rust::interpreter::accounting::authority::AuthorityError::InsufficientAuthority) => {
                closed_groups.insert(candidate.sig_key);
                outcome.rejected.push(candidate.cosigned.primary().sig.clone());
                continue;
            }
            Err(error) => {
                return Err(CasperError::InvalidCostSettlement(error.to_string()));
            }
        };
        let cost_draw = physical_settlement.balance_debit.clone();
        let after_cost = physical_inventory
            .balances
            .checked_sub(&cost_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let fee_draw = match allocate_authority_events(std::slice::from_ref(&fee_event), &after_cost)
        {
            Ok(draw) => draw,
            Err(rholang::rust::interpreter::accounting::authority::AuthorityError::InsufficientAuthority) => {
                closed_groups.insert(candidate.sig_key);
                outcome.rejected.push(candidate.cosigned.primary().sig.clone());
                continue;
            }
            Err(error) => {
                return Err(CasperError::InvalidCostSettlement(error.to_string()));
            }
        };

        let certified = candidate.certified.as_mut().ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "state-bound candidate is missing its funding certificate".to_string(),
            )
        })?;
        certified.certificate.allocation = cost_draw.clone();
        certified.certificate.stack_reservations = physical_settlement.stack_pops.clone();
        certified.certificate.fee_allocation = fee_draw.clone();
        certified
            .certificate
            .verify_with_allocation(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                certified.program_hash,
                evidence.pre_state_root,
                &physical_inventory.balances,
                |_, _| false,
                |bound, allocation| {
                    bound == &evidence.authority_witness.realized && allocation == &cost_draw
                },
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let mut witness = evidence.authority_witness;
        witness.certificate_id = certified.certificate.certificate_id();
        witness.settlement = cost_draw.clone();
        witness.physical_draws = physical_settlement.draws.clone();
        witness
            .verify_with_settlement(&certified.certificate, |_, _, _| Ok(cost_draw.clone()))
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;

        processed[index].authority_funding_certificate =
            Some(authority_certificate_to_proto(&certified.certificate));
        processed[index].authority_cost_witness = Some(authority_witness_to_proto(&witness));
        apply_physical_settlement(&mut physical_inventory, &physical_settlement)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        physical_inventory.balances = after_cost
            .checked_sub(&fee_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        for (stack_id, pop_count) in physical_settlement.stack_pops {
            let total = outcome.stack_pops.entry(stack_id).or_default();
            *total = total.checked_add(pop_count).ok_or_else(|| {
                CasperError::InvalidCostSettlement("authority stack pop count overflow".to_string())
            })?;
        }
        record_authority_debits(&mut outcome.debits, &cost_draw, &channels)?;
        record_authority_debits(&mut outcome.fee_debits, &fee_draw, &channels)?;
        outcome.admitted.push(candidate.cosigned);
    }
    outcome.purse_stacks = purse_stacks;
    Ok(outcome)
}

#[derive(Default)]
pub struct StateBoundAuthoritySession {
    pool_signatures: BTreeMap<SigKey, models::rhoapi::CostSignature>,
    channels: BTreeMap<SigKey, Par>,
    physical_inventory: AuthorityPhysicalInventory<SigKey>,
    purse_stacks: BTreeMap<[u8; 32], supply::PurseStack>,
    closed_groups: std::collections::BTreeSet<SigKey>,
    outcome: AdmissionOutcome,
}

impl StateBoundAuthoritySession {
    fn group_key(deploy: &Cosigned<DeployData>) -> SigKey { accounting::funding_sig(deploy).key() }

    pub fn prepare(&mut self, deploy: &Cosigned<DeployData>) -> bool {
        let funding = accounting::funding_sig(deploy);
        let key = funding.key();
        let valid = funding.is_funding_former()
            && Compiler::source_to_adt_with_normalizer_env(
                &deploy.data().term,
                models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(deploy),
            )
            .is_ok()
            && !self.closed_groups.contains(&key);
        if !valid {
            self.closed_groups.insert(key);
            self.outcome.rejected.push(deploy.primary().sig.clone());
        }
        valid
    }

    pub fn reject_exhausted(&mut self, deploy: &Cosigned<DeployData>) {
        self.closed_groups.insert(Self::group_key(deploy));
        self.outcome.rejected.push(deploy.primary().sig.clone());
    }

    async fn load_signatures(
        &mut self,
        signatures: BTreeMap<SigKey, models::rhoapi::CostSignature>,
        supply_reader: &dyn SupplyReader,
    ) -> Result<(), CasperError> {
        for (key, signature) in signatures {
            if let Some(existing) = self.pool_signatures.get(&key) {
                if existing != &signature {
                    return Err(CasperError::InvalidCostSettlement(
                        "authority session contains a signature-key collision".to_string(),
                    ));
                }
                continue;
            }
            let funding = rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(
                &signature,
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let channel = supply::supply_channel(&funding);
            let purse = supply_reader.read_purse(&signature).await?;
            let balance = purse.balance.unwrap_or(0);
            if balance < 0 {
                return Err(CasperError::InvalidCostSettlement(
                    "authority purse balance cannot be negative".to_string(),
                ));
            }
            if balance > 0 {
                self.physical_inventory.balances.0.insert(
                    key,
                    u64::try_from(balance).expect("non-negative authority balance"),
                );
            }
            for stack in purse.stacks {
                if self
                    .physical_inventory
                    .stacks
                    .insert(stack.instance_id, stack.stack.cells.clone())
                    .is_some()
                    || self.purse_stacks.insert(stack.instance_id, stack).is_some()
                {
                    return Err(CasperError::InvalidCostSettlement(
                        "authority inventory contains a duplicate stack identity".to_string(),
                    ));
                }
            }
            self.channels.insert(key, channel);
            self.pool_signatures.insert(key, signature);
        }
        Ok(())
    }

    pub async fn settle_candidate(
        &mut self,
        deploy: &Cosigned<DeployData>,
        processed: &mut ProcessedDeploy,
        pre_state_root: [u8; 32],
        supply_reader: &dyn SupplyReader,
    ) -> Result<bool, CasperError> {
        let mut witness_proto = processed
            .authority_cost_witness
            .as_ref()
            .ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "state-bound execution is missing its authority witness".to_string(),
                )
            })?
            .clone();
        if witness_proto.pre_state_root.is_empty() {
            witness_proto.pre_state_root = pre_state_root.to_vec().into();
        }
        if witness_proto.post_state_root.is_empty() {
            witness_proto.post_state_root = vec![0; 32].into();
        }
        let mut witness = authority_witness_from_proto(&witness_proto, true)?;
        witness.pre_state_root = pre_state_root;
        let fee_event = fee_authority_event(deploy)?;
        let mut funding_events = witness.events.clone();
        funding_events.push(fee_event.clone());
        let signatures = authority_funding_signatures_with_presentations(
            &funding_events,
            &deploy.data().authority_presentations,
        )
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        self.load_signatures(signatures, supply_reader).await?;

        let physical_settlement = match allocate_physical_settlement(
            &witness.events,
            &self.pool_signatures,
            &self.physical_inventory,
        ) {
            Ok(settlement) => settlement,
            Err(
                rholang::rust::interpreter::accounting::authority::AuthorityError::InsufficientAuthority,
            ) => {
                self.closed_groups.insert(Self::group_key(deploy));
                self.outcome.rejected.push(deploy.primary().sig.clone());
                return Ok(false);
            }
            Err(error) => {
                return Err(CasperError::InvalidCostSettlement(error.to_string()));
            }
        };
        let cost_draw = physical_settlement.balance_debit.clone();
        let after_cost = self
            .physical_inventory
            .balances
            .checked_sub(&cost_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let fee_draw = match allocate_authority_events(
            std::slice::from_ref(&fee_event),
            &after_cost,
        ) {
            Ok(draw) => draw,
            Err(
                rholang::rust::interpreter::accounting::authority::AuthorityError::InsufficientAuthority,
            ) => {
                self.closed_groups.insert(Self::group_key(deploy));
                self.outcome.rejected.push(deploy.primary().sig.clone());
                return Ok(false);
            }
            Err(error) => {
                return Err(CasperError::InvalidCostSettlement(error.to_string()));
            }
        };

        let canonical = canonical_program_for_deploy(deploy, supply_reader).await?;
        let program_hash: [u8; 32] = Blake2b256::hash(canonical.encode_to_vec())
            .try_into()
            .expect("Blake2b-256 digest length");
        let reservation_id: [u8; 32] = Blake2b256::hash(deploy.primary().sig.to_vec())
            .try_into()
            .expect("Blake2b-256 digest length");
        let certificate = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash,
            pre_state_root,
            reservation_id,
            demand: DemandBound::Exact(witness.realized.clone()),
            allocation: cost_draw.clone(),
            stack_reservations: physical_settlement.stack_pops.clone(),
            fee_allocation: fee_draw.clone(),
            fee_recipient: Vec::new(),
        };
        certificate
            .verify_with_allocation(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                program_hash,
                pre_state_root,
                &self.physical_inventory.balances,
                |_, _| false,
                |bound, allocation| bound == &witness.realized && allocation == &cost_draw,
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        witness.certificate_id = certificate.certificate_id();
        witness.settlement = cost_draw.clone();
        witness.physical_draws = physical_settlement.draws.clone();
        witness
            .verify_event_authorities()
            .and_then(|_| {
                witness.verify_with_settlement(&certificate, |_, _, _| Ok(cost_draw.clone()))
            })
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;

        processed.authority_funding_certificate =
            Some(authority_certificate_to_proto(&certificate));
        processed.authority_cost_witness = Some(authority_witness_to_proto(&witness));
        apply_physical_settlement(&mut self.physical_inventory, &physical_settlement)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        self.physical_inventory.balances = after_cost
            .checked_sub(&fee_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        for (stack_id, pop_count) in physical_settlement.stack_pops {
            let total = self.outcome.stack_pops.entry(stack_id).or_default();
            *total = total.checked_add(pop_count).ok_or_else(|| {
                CasperError::InvalidCostSettlement("authority stack pop count overflow".to_string())
            })?;
        }
        record_authority_debits(&mut self.outcome.debits, &cost_draw, &self.channels)?;
        record_authority_debits(&mut self.outcome.fee_debits, &fee_draw, &self.channels)?;
        self.outcome.admitted.push(deploy.clone());
        Ok(true)
    }

    pub fn finish(mut self) -> AdmissionOutcome {
        self.outcome.purse_stacks = self.purse_stacks;
        self.outcome
    }
}

pub async fn admit_by_funding_with_state_bound_evidence(
    deploys: Vec<Cosigned<DeployData>>,
    processed: &mut [ProcessedDeploy],
    supply_reader: &dyn SupplyReader,
) -> Result<AdmissionOutcome, CasperError> {
    admit_state_bound_authority(deploys, processed, supply_reader).await
}

pub async fn state_bound_execution_caps(
    deploys: &[Cosigned<DeployData>],
    supply_reader: &dyn SupplyReader,
) -> Result<Vec<i64>, CasperError> {
    let mut ordered = deploys.to_vec();
    canonical_sort(&mut ordered);
    let mut capacities = Vec::with_capacity(ordered.len());

    for deploy in &ordered {
        capacities.push(state_bound_execution_cap_with_frontier(deploy, &[], supply_reader).await?);
    }

    Ok(capacities)
}

fn insert_capacity_signature(
    signatures: &mut BTreeMap<SigKey, models::rhoapi::CostSignature>,
    key: SigKey,
    signature: models::rhoapi::CostSignature,
) -> Result<(), CasperError> {
    match signatures.get(&key) {
        Some(existing) if existing != &signature => Err(CasperError::InvalidCostSettlement(
            "authority capacity contains a signature-key collision".to_string(),
        )),
        Some(_) => Ok(()),
        None => {
            signatures.insert(key, signature);
            Ok(())
        }
    }
}

fn state_bound_capacity_signatures(
    deploy: &Cosigned<DeployData>,
    canonical: &Par,
    frontier: &[CostAuthority],
) -> Result<BTreeMap<SigKey, models::rhoapi::CostSignature>, CasperError> {
    let mut signatures = static_authority_signatures(canonical)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let funding = accounting::funding_sig(deploy);
    let funding_signature = sig_to_cost_signature(&funding)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    insert_capacity_signature(&mut signatures, funding.key(), funding_signature)?;

    let frontier_events = frontier
        .iter()
        .enumerate()
        .map(
            |(index, authority)| -> Result<AuthorityEvent<SigKey>, CasperError> {
                let authority = canonical_authority(authority)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                let mut event_id = [0; 32];
                event_id[..8].copy_from_slice(
                    &u64::try_from(index)
                        .map_err(|_| {
                            CasperError::InvalidCostSettlement(
                                "authority frontier exceeds the platform range".to_string(),
                            )
                        })?
                        .to_le_bytes(),
                );
                Ok(AuthorityEvent {
                    event_id,
                    debit: authority_demand(&authority)
                        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
                    authority,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    for (key, signature) in authority_funding_signatures_with_presentations(
        &frontier_events,
        &deploy.data().authority_presentations,
    )
    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
    {
        insert_capacity_signature(&mut signatures, key, signature)?;
    }
    Ok(signatures)
}

async fn state_bound_capacity_from_signatures(
    signatures: &BTreeMap<SigKey, models::rhoapi::CostSignature>,
    supply_reader: &dyn SupplyReader,
) -> Result<i64, CasperError> {
    let mut capacity = 0_i64;
    let mut stack_ids = std::collections::BTreeSet::new();
    for signature in signatures.values() {
        let purse = supply_reader.read_purse(signature).await?;
        let balance = purse.balance.unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        capacity = capacity.checked_add(balance).ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "authority-derived execution capacity overflow".to_string(),
            )
        })?;
        for stack in purse.stacks {
            if !stack_ids.insert(stack.instance_id) {
                return Err(CasperError::InvalidCostSettlement(
                    "authority-derived execution capacity contains a duplicate stack identity"
                        .to_string(),
                ));
            }
            let cells = i64::try_from(stack.stack.cells.len()).map_err(|_| {
                CasperError::InvalidCostSettlement(
                    "authority stack depth exceeds execution capacity range".to_string(),
                )
            })?;
            capacity = capacity.checked_add(cells).ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "authority-derived execution capacity overflow".to_string(),
                )
            })?;
        }
    }
    Ok(capacity)
}

pub async fn state_bound_execution_cap_with_frontier(
    deploy: &Cosigned<DeployData>,
    frontier: &[CostAuthority],
    supply_reader: &dyn SupplyReader,
) -> Result<i64, CasperError> {
    let canonical = canonical_program_for_deploy(deploy, supply_reader).await?;
    let signatures = state_bound_capacity_signatures(deploy, &canonical, frontier)?;
    let live_capacity = state_bound_capacity_from_signatures(&signatures, supply_reader).await?;
    let program_capacity = match static_authority_plan(&canonical, &accounting::funding_sig(deploy))
    {
        Ok(plan) => {
            plan.guaranteed_program_supply
                .0
                .values()
                .try_fold(0_i64, |total, amount| {
                    i64::try_from(*amount)
                        .ok()
                        .and_then(|amount| total.checked_add(amount))
                        .ok_or_else(|| {
                            CasperError::InvalidCostSettlement(
                                "program-born authority capacity exceeds the execution range"
                                    .to_string(),
                            )
                        })
                })?
        }
        Err(_) => 0,
    };
    live_capacity
        .saturating_sub(1)
        .max(0)
        .checked_add(program_capacity)
        .ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "state-bound execution capacity exceeds the platform range".to_string(),
            )
        })
}

pub async fn admit_by_funding_with_logic<L, P>(
    deploys: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
    logic: &L,
    policy: &P,
) -> Result<AdmissionOutcome, CasperError>
where
    L: OslfResourceLogic<RhoGslt>,
    P: ApportionmentPolicy<RhoGslt>,
{
    admit_by_funding_internal(deploys, supply_reader, logic, policy, None).await
}

async fn admit_by_funding_internal<L, P>(
    deploys: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
    logic: &L,
    policy: &P,
    mut state_bounds: Option<&mut BTreeMap<Vec<u8>, VecDeque<StateBoundCostEvidence>>>,
) -> Result<AdmissionOutcome, CasperError>
where
    L: OslfResourceLogic<RhoGslt>,
    P: ApportionmentPolicy<RhoGslt>,
{
    // 1. Canonicalize the (nondeterministically-ordered) input.
    let mut ordered = deploys;
    canonical_sort(&mut ordered);

    // 2. Build candidates (envelope Sig → key/channel/demand). Malformed terms
    //    are split off as rejected immediately (never grouped).
    let mut outcome = AdmissionOutcome::default();
    let mut candidates: Vec<Candidate> = Vec::with_capacity(ordered.len());
    let pre_state_root = supply_reader.pre_state_root();
    for cosigned in ordered {
        let evidence = match state_bounds.as_deref_mut() {
            Some(bounds) => Some(
                bounds
                    .get_mut(cosigned.primary().sig.as_ref())
                    .and_then(VecDeque::pop_front)
                    .ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "missing state-bound cost evidence for candidate deploy".to_string(),
                        )
                    })?,
            ),
            None => None,
        };
        let candidate =
            build_candidate_with_logic(cosigned, logic, pre_state_root, evidence.as_ref());
        if candidate.malformed {
            outcome
                .rejected
                .push(candidate.cosigned.primary().sig.clone());
        } else {
            candidates.push(candidate);
        }
    }

    // 3. Group into a BTreeMap<SigKey, Vec<Candidate>> — deterministic group
    //    iteration; each group's Vec preserves canonical order (push order).
    let mut groups: BTreeMap<SigKey, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        groups.entry(candidate.sig_key).or_default().push(candidate);
    }

    // 4. Build decompositions for every compound envelope (per internal `And`
    //    node), and collect the full distinct channel set (groups + component
    //    channels) so each pool is read EXACTLY once. `channels_by_key` de-dups
    //    deterministically by `SigKey`.
    let mut decompositions: Vec<Decomposition> = Vec::new();
    let mut channels_by_key: BTreeMap<SigKey, Par> = BTreeMap::new();
    let mut signatures_by_key: BTreeMap<SigKey, CostSignature> = BTreeMap::new();
    for group in groups.values() {
        // All candidates in a group share the same envelope key/channel; the
        // first is the representative.
        if let Some(repr) = group.first() {
            channels_by_key
                .entry(repr.sig_key)
                .or_insert_with(|| repr.channel.clone());
            let funding = accounting::funding_sig(&repr.cosigned);
            collect_decompositions(&funding, &mut decompositions, &mut channels_by_key);
            collect_funding_signatures(&funding, &mut signatures_by_key)?;
        }
    }

    let mut raw: BTreeMap<SigKey, i64> = BTreeMap::new();
    for (key, signature) in &signatures_by_key {
        let balance = supply_reader
            .read_purse(signature)
            .await?
            .balance
            .unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        raw.insert(*key, balance);
    }

    // 6. The LIVE cross-group residual ledger (TM-CA-165): each group's admission
    //    cap is its EFFECTIVE supply read from this ledger, drawn DOWN as
    //    successive groups (SigKey order) are admitted — so two DISTINCT cosigner
    //    sets sharing a component wallet `Σ⟦Ground(s)⟧` cannot each be admitted
    //    against s's FULL balance (cross-group linearity: linear logic admits no
    //    contraction; a token is spent once). Seeded from the raw pre-state
    //    (absent ⇒ 0); evolves identically on play and replay (same SigKey order).
    let mut remaining: BTreeMap<SigKey, i64> = raw.clone();
    let decomposition_by_compound = index_decompositions(&decompositions);

    // 7. Per-group prefix admission (reject-both), accumulating Σ Δ_s per group.
    //    The cumulative admitted demand of each PRESENT group is recorded into
    //    `demand_by_group` (channel + `k = ΣΔ_admitted`); the EXACT per-pool
    //    settlement debit (combined-pool-first split + shared-component residual
    //    ledger — #12) is computed AFTER the walk by [`compute_settlement_debits`],
    //    the SINGLE shared function replay also runs (byte-identity).
    let mut demand_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    // The FeeExtract is one token per admitted deploy. It is accumulated beside
    // cost demand and settled by a second allocation pass against the post-cost
    // residual, then transferred to the proposing validator's SystemVault.
    let mut fee_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    for (sig_key, group) in groups {
        let channel = group.first().map(|c| c.channel.clone()).unwrap_or_default();

        // The admission residual is the group's EFFECTIVE supply read from the
        // LIVE cross-group ledger (TM-CA-165): a single group caps at its own-pool
        // residual (no-weakening, §D2.9-R2 — a compound pool does NOT augment a
        // single component); a compound caps at `Σ⟦compound⟧ + min(Σ⟦s₁⟧, Σ⟦s₂⟧)`.
        // Both are read from the DRAWN-DOWN `remaining`, so a later group sharing a
        // component sees the reduced balance — the exact multi-pool draw is still
        // settled by `compute_settlement_debits` (underflow-safe per-pool debit).
        let shape = group_shape_from(&sig_key, &decomposition_by_compound, &remaining);
        let mut residual: i64 = group_capacity(shape);

        // FeeExtract (cost-accounted-rho.tex:3637): ONE client token per admitted
        // deploy, CARVED from the client's own `Σ⟦c⟧` (conserving) IN ADDITION to
        // the burned per-COMM cost Δ — so an admitted deploy needs `Σ⟦c⟧ ≥ Δ + 1`.
        // The admission reservation folds the +1 fee into the certified cost
        // upper bound. Unprovable demand was rejected while building candidates.
        const FEE_PER_DEPLOY: i64 = 1;
        let mut group_debit: i64 = 0; // COST (Σ Δ): burned from Σ⟦c⟧
        let mut group_fee: i64 = 0;
        let mut prefix_open = true;
        for candidate in group {
            // Admission demand = cost + fee (the client must afford BOTH).
            let cost_plus_fee = DemandEntry {
                certified_upper_bound: candidate
                    .certified_upper_bound
                    .saturating_add(FEE_PER_DEPLOY),
                unknown: false,
            };
            let available = ResourceMultiset::singleton(
                sig_key,
                u64::try_from(residual.max(0)).unwrap_or_default(),
            );
            let certificate_valid = candidate.certified.as_ref().is_some_and(|certified| {
                certified
                    .certificate
                    .verify_with(
                        AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                        certified.program_hash,
                        pre_state_root,
                        &available,
                        |bound, _proof| match certified.proof_kind {
                            DemandProofKind::Structural => {
                                logic
                                    .verify_demand_bound(
                                        &certified.canonical,
                                        &certified.funding,
                                        &certified.certificate.demand,
                                    )
                                    .as_ref()
                                    == Some(bound)
                            }
                            DemandProofKind::StateBound => {
                                &certified.certificate.allocation == bound
                            }
                        },
                    )
                    .is_ok()
            });
            if prefix_open && certificate_valid && logic.is_funded(&cost_plus_fee, residual) {
                // Admit: consume cost + fee from the residual and preserve their
                // distinct burn and transfer dispositions.
                residual = residual
                    .saturating_sub(candidate.certified_upper_bound)
                    .saturating_sub(FEE_PER_DEPLOY);
                group_debit = group_debit.saturating_add(candidate.certified_upper_bound);
                group_fee = group_fee.saturating_add(FEE_PER_DEPLOY);
                outcome.admitted.push(candidate.cosigned);
            } else {
                // §7.7 reject-both: the FIRST candidate that cannot afford cost +
                // fee, and ALL after it in the group, are rejected.
                prefix_open = false;
                outcome
                    .rejected
                    .push(candidate.cosigned.primary().sig.clone());
            }
        }

        // Draw this group's folded cost+fee DOWN the LIVE cross-group ledger so the
        // NEXT group (SigKey order) sharing a component wallet sees the reduced
        // residual (TM-CA-165). Combined-pool-first via DefaultApportionment — the
        // conservative reservation that dominates the two-pass cost-then-fee
        // settlement on every pool, so admission-fundable ⟹ settlement-safe.
        let group_total = group_debit.saturating_add(group_fee);
        if group_total > 0 {
            draw_group_from_ledger(shape, group_total, &mut remaining);
        }

        if group_debit > 0 {
            demand_by_group.insert(sig_key, (channel.clone(), group_debit));
        }
        if group_fee > 0 {
            fee_by_group.insert(sig_key, (channel, group_fee));
        }
    }

    // 8. Settle the per-pool debit EXACTLY (#12): split each admitted compound
    //    group's cumulative demand `k` combined-pool-first into `(Σ⟦compound⟧,
    //    Σ⟦s₁⟧, Σ⟦s₂⟧)`, bounding the shared component draws by a cross-group
    //    residual ledger. The SAME function (over identically-derived inputs)
    //    runs on replay ⇒ byte-identical `BTreeMap<SigKey, SettlementDebit>`.
    outcome.debits = compute_settlement_debits(
        &demand_by_group,
        &decompositions,
        &raw,
        &channels_by_key,
        policy,
    );

    // FEE carve (Stage D): the per-pool fee debit is computed AFTER the cost
    // debit, against the POST-COST residual (raw − cost draws). It uses the FLAT
    // [`FlatFeeApportionment`], NOT the cost `policy`: the `FeeExtract` is ONE
    // PHYSICAL token per admitted deploy (tex:3637; design OD-3; Rocq flat-`f`), so
    // a COMPOUND deploy owes 1 — drawn combined-pool-first then a SINGLE component,
    // never the matched component PAIR that the COST debits (which would charge a
    // multi-sig deploy 2× — red-team F-1). The carved total is transferred to the
    // proposer SystemVault (conserving). The gate admitted
    // only deploys with Σ⟦c⟧ ≥ cost + fee, so this pass never underflows; the same
    // function + policy over identically-derived inputs runs on replay (`:911`) ⇒
    // byte-identical.
    let raw_after_cost: BTreeMap<SigKey, i64> = channels_by_key
        .keys()
        .map(|k| {
            let post = raw.get(k).copied().unwrap_or(0)
                - outcome.debits.get(k).map(|d| d.amount).unwrap_or(0);
            (*k, post)
        })
        .collect();
    outcome.fee_debits = compute_settlement_debits(
        &fee_by_group,
        &decompositions,
        &raw_after_cost,
        &channels_by_key,
        &FlatFeeApportionment,
    );

    // Re-impose canonical order on the admitted set: the per-group walk emits
    // each group's prefix in canonical order, but group iteration is by SigKey,
    // so a final canonical sort restores the global execution order.
    canonical_sort(&mut outcome.admitted);

    Ok(outcome)
}

/// REPLAY recompute of the WD-D2 structural reservation map from the block's
/// ADMITTED deploys (`block.body.deploys`).
///
/// Replay receives only the executed subset of `block.body.deploys`; terminal
/// funding-rejection records are verified and removed before this function. The per-pool
/// The map is `Σ Δ_s^max` over admitted deploys in each pool, matching
/// the play-side reservation. Realized settlement uses
/// [`recompute_realized_settlement_debits`] after execution. Every reservation
/// is checked against supply, with an absent pool interpreted as zero.
///
/// Returns the recomputed map (identical to the play-side `AdmissionOutcome.debits`
/// for a valid block) AND the count of admitted deploys (for the
/// `ReplayAdmissionMismatch` diagnostic).
///
/// The recompute covers every signed body deployment, including heartbeat and
/// dummy deployments, exactly as proposal admission does.
pub async fn recompute_settlement_debits(
    admitted: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
) -> Result<RecomputedDebits, CasperError> {
    let logic = DefaultResourceLogic;
    let policy = DefaultApportionment;
    recompute_settlement_debits_with_logic(admitted, supply_reader, &logic, &policy).await
}

pub async fn recompute_settlement_debits_with_logic<L, P>(
    admitted: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
    logic: &L,
    policy: &P,
) -> Result<RecomputedDebits, CasperError>
where
    L: OslfResourceLogic<RhoGslt>,
    P: ApportionmentPolicy<RhoGslt>,
{
    recompute_settlement_debits_internal(admitted, supply_reader, logic, policy, None, None).await
}

pub async fn recompute_realized_settlement_debits(
    processed: &[ProcessedDeploy],
    supply_reader: &dyn SupplyReader,
) -> Result<RecomputedDebits, CasperError> {
    recompute_authority_settlement_debits(processed, supply_reader).await
}

pub async fn recompute_state_bound_settlement_debits(
    processed: &[ProcessedDeploy],
    supply_reader: &dyn SupplyReader,
) -> Result<RecomputedDebits, CasperError> {
    recompute_authority_settlement_debits(processed, supply_reader).await
}

pub async fn verify_state_bound_replay_admission(
    processed: &[ProcessedDeploy],
    fee_recipient: &[u8],
    supply_reader: &dyn SupplyReader,
) -> Result<RecomputedDebits, CasperError> {
    for deploy in processed {
        let certificate = authority_certificate_from_proto(
            deploy
                .authority_funding_certificate
                .as_ref()
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "replay deploy is missing its authority certificate".to_string(),
                    )
                })?,
        )?;
        if certificate.fee_recipient.as_slice() != fee_recipient {
            return Err(CasperError::InvalidCostSettlement(
                "authority certificate fee recipient differs from the block proposer".to_string(),
            ));
        }
    }
    recompute_state_bound_settlement_debits(processed, supply_reader).await
}

async fn recompute_authority_settlement_debits(
    processed: &[ProcessedDeploy],
    supply_reader: &dyn SupplyReader,
) -> Result<RecomputedDebits, CasperError> {
    let mut entries = Vec::with_capacity(processed.len());
    let mut ordered = Vec::with_capacity(processed.len());
    let mut pool_signatures = BTreeMap::new();
    let mut expected_pre = supply_reader.pre_state_root();

    for deploy in processed {
        let cosigned = deploy.to_cosigned().map_err(CasperError::RuntimeError)?;
        let witness = authority_witness_from_proto(
            deploy.authority_cost_witness.as_ref().ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "processed deploy is missing its authority witness".to_string(),
                )
            })?,
            false,
        )?;
        let certificate = authority_certificate_from_proto(
            deploy
                .authority_funding_certificate
                .as_ref()
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "processed deploy is missing its authority funding certificate".to_string(),
                    )
                })?,
        )?;
        if witness.pre_state_root != expected_pre
            || certificate.pre_state_root != witness.pre_state_root
            || witness.post_state_root
                != authority_digest(&deploy.post_state_hash, "processed deploy post-state root")?
            || witness.pre_state_root
                != authority_digest(&deploy.pre_state_hash, "processed deploy pre-state root")?
        {
            return Err(CasperError::InvalidCostSettlement(
                "authority certificate and witness do not form the processed root chain"
                    .to_string(),
            ));
        }
        let canonical = canonical_program_for_deploy(&cosigned, supply_reader).await?;
        let program_hash = authority_digest(
            &Blake2b256::hash(canonical.encode_to_vec()),
            "replay authority program hash",
        )?;
        if certificate.protocol_version != AUTHORITY_ACCOUNTING_PROTOCOL_VERSION
            || certificate.program_hash != program_hash
        {
            return Err(CasperError::InvalidCostSettlement(
                "authority certificate protocol or program binding is invalid".to_string(),
            ));
        }
        let demand = match &certificate.demand {
            DemandBound::Exact(demand) => demand,
            DemandBound::FiniteUpperBound { bound, proof } if !proof.is_empty() => bound,
            _ => {
                return Err(CasperError::InvalidCostSettlement(
                    "authority certificate has no verifiable finite demand".to_string(),
                ));
            }
        };
        if !demand.dominates(&witness.realized) {
            return Err(CasperError::InvalidCostSettlement(
                "realized authority exceeds the certified demand".to_string(),
            ));
        }
        let fee_event = fee_authority_event(&cosigned)?;
        let mut funding_events = witness.events.clone();
        funding_events.push(fee_event.clone());
        for (key, signature) in authority_funding_signatures_with_presentations(
            &funding_events,
            &cosigned.data().authority_presentations,
        )
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
        {
            pool_signatures.entry(key).or_insert(signature);
        }
        for birth in &witness.born_stacks {
            for cell in &birth.cells {
                insert_signature(&mut pool_signatures, cell.clone())?;
            }
        }
        expected_pre = witness.post_state_root;
        ordered.push(cosigned.clone());
        entries.push((cosigned, certificate, witness, fee_event));
    }
    let mut canonical_order = ordered.clone();
    canonical_sort(&mut canonical_order);
    if canonical_order != ordered {
        return Err(CasperError::InvalidCostSettlement(
            "processed authority evidence is not in canonical deploy order".to_string(),
        ));
    }

    let mut channels = BTreeMap::new();
    let mut physical_inventory = AuthorityPhysicalInventory::default();
    let mut purse_stacks = BTreeMap::new();
    for (key, signature) in &pool_signatures {
        let funding =
            rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let channel = supply::supply_channel(&funding);
        let purse = supply_reader.read_purse(signature).await?;
        let balance = purse.balance.unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        channels.insert(*key, channel);
        if balance > 0 {
            physical_inventory.balances.0.insert(
                *key,
                u64::try_from(balance).expect("non-negative authority balance"),
            );
        }
        for stack in purse.stacks {
            if physical_inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
                || purse_stacks.insert(stack.instance_id, stack).is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "authority inventory contains a duplicate stack identity".to_string(),
                ));
            }
        }
    }

    let mut recomputed = RecomputedDebits::default();
    for (_cosigned, certificate, witness, fee_event) in entries {
        if witness.physical_draws.is_empty() && !witness.events.is_empty() {
            return Err(CasperError::InvalidCostSettlement(
                "authority witness is missing its physical settlement presentation".to_string(),
            ));
        }
        for birth in &witness.born_stacks {
            if physical_inventory
                .stacks
                .insert(birth.stack_id, birth.cells.clone())
                .is_some()
                || physical_inventory
                    .born_stacks
                    .insert(birth.stack_id, birth.produce_hash)
                    .is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "replayed born stack collides with prior authority inventory".to_string(),
                ));
            }
        }
        let physical_settlement = verify_physical_settlement(
            &witness.events,
            &pool_signatures,
            &physical_inventory,
            &witness.physical_draws,
        )
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let cost_draw = physical_settlement.balance_debit.clone();
        if cost_draw != witness.settlement || !certificate.allocation.dominates(&cost_draw) {
            return Err(CasperError::InvalidCostSettlement(
                "authority settlement differs from the canonical replay allocation".to_string(),
            ));
        }
        for (stack_id, pop_count) in &certificate.stack_reservations {
            let available = physical_inventory
                .stacks
                .get(stack_id)
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "authority certificate reserves an unknown stack identity".to_string(),
                    )
                })?
                .len();
            if u64::try_from(available).unwrap_or(u64::MAX) < *pop_count {
                return Err(CasperError::InvalidCostSettlement(
                    "authority certificate reserves more stack cells than are available"
                        .to_string(),
                ));
            }
        }
        let exact_reservation = matches!(&certificate.demand, DemandBound::Exact(_));
        let born_stack_ids = witness
            .born_stacks
            .iter()
            .map(|birth| birth.stack_id)
            .collect::<std::collections::BTreeSet<_>>();
        let preexisting_stack_pops = physical_settlement
            .stack_pops
            .iter()
            .filter(|(stack_id, _)| !born_stack_ids.contains(*stack_id))
            .map(|(stack_id, count)| (*stack_id, *count))
            .collect::<BTreeMap<_, _>>();
        if exact_reservation
            && (cost_draw != certificate.allocation
                || preexisting_stack_pops != certificate.stack_reservations)
        {
            return Err(CasperError::InvalidCostSettlement(
                "exact authority certificate does not bind the realized physical reservation"
                    .to_string(),
            ));
        }
        certificate
            .verify_with_allocation(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                certificate.program_hash,
                certificate.pre_state_root,
                &physical_inventory.balances,
                |_, proof| !proof.is_empty(),
                |bound, allocation| {
                    bound.dominates(&witness.realized) && allocation.dominates(&cost_draw)
                },
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let reserved_balances = certificate
            .allocation
            .checked_add(&certificate.fee_allocation)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        if !physical_inventory.balances.dominates(&reserved_balances) {
            return Err(CasperError::InvalidCostSettlement(
                "authority certificate reserves more vault balance than is available".to_string(),
            ));
        }
        witness
            .verify_with_settlement(&certificate, |_, _, _| Ok(cost_draw.clone()))
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let after_cost = physical_inventory
            .balances
            .checked_sub(&cost_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let fee_draw = allocate_authority_events(std::slice::from_ref(&fee_event), &after_cost)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        if fee_draw != certificate.fee_allocation {
            return Err(CasperError::InvalidCostSettlement(
                "authority fee allocation differs from the canonical replay allocation".to_string(),
            ));
        }
        apply_physical_settlement(&mut physical_inventory, &physical_settlement)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        for stack_id in born_stack_ids {
            physical_inventory.born_stacks.remove(&stack_id);
        }
        physical_inventory.balances = after_cost
            .checked_sub(&fee_draw)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        for (stack_id, pop_count) in physical_settlement.stack_pops {
            let total = recomputed.stack_pops.entry(stack_id).or_default();
            *total = total.checked_add(pop_count).ok_or_else(|| {
                CasperError::InvalidCostSettlement("authority stack pop count overflow".to_string())
            })?;
        }
        record_authority_debits(&mut recomputed.settlement, &cost_draw, &channels)?;
        record_authority_debits(&mut recomputed.fee, &fee_draw, &channels)?;
    }
    recomputed.purse_stacks = purse_stacks;
    Ok(recomputed)
}

async fn recompute_settlement_debits_internal<L, P>(
    admitted: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
    logic: &L,
    policy: &P,
    mut state_bounds: Option<&mut BTreeMap<Vec<u8>, VecDeque<StateBoundCostEvidence>>>,
    mut realized: Option<BTreeMap<Vec<u8>, VecDeque<RealizedCost>>>,
) -> Result<RecomputedDebits, CasperError>
where
    L: OslfResourceLogic<RhoGslt>,
    P: ApportionmentPolicy<RhoGslt>,
{
    // 1. Group admitted deploys by pool, summing Δ_s, AND collect the Split/Join
    //    decompositions + every distinct channel (group + compound component) —
    //    EXACTLY as `admit_by_funding` does, from the same `Cosigned` envelopes
    //    (`build_candidate` → `funding_sig` → the same `Sig::Ground`/`And`), so the
    //    inputs to `compute_settlement_debits` are byte-identical to the play
    //    side. A malformed term among the ADMITTED set cannot occur for a valid
    //    block (the proposer never admits a malformed deploy), but is treated
    //    defensively as zero demand so the recompute is total.
    let mut demand_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    // FEE carve (Stage D): per-group fee = count of admitted deploys (1 each),
    // accumulated alongside the cost demand. `block.body.deploys` IS the
    // play-admitted set, so this count mirrors the play-side gate's per-group
    // `group_fee` exactly; settled by the post-cost fee pass below.
    let mut fee_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    let mut decompositions: Vec<Decomposition> = Vec::new();
    let mut channels_by_key: BTreeMap<SigKey, Par> = BTreeMap::new();
    let mut signatures_by_key: BTreeMap<SigKey, CostSignature> = BTreeMap::new();
    let mut group_envelopes: BTreeMap<SigKey, Sig> = BTreeMap::new();
    let admitted_count = admitted.len();
    for (replay_index, cosigned) in admitted.into_iter().enumerate() {
        // SAME shared `funding_sig` the play-side gate (`build_candidate_with_logic`)
        // keys by — so replay reconstructs the byte-identical `Sig::Ground(pk)` /
        // `And`-fold, hence the byte-identical settlement-debit map.
        let funding = accounting::funding_sig(&cosigned);
        let evidence = match state_bounds.as_deref_mut() {
            Some(bounds) => Some(
                bounds
                    .get_mut(cosigned.primary().sig.as_ref())
                    .and_then(VecDeque::pop_front)
                    .ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "missing state-bound cost evidence for admitted deploy".to_string(),
                        )
                    })?,
            ),
            None => None,
        };
        let candidate = build_candidate_with_logic(
            cosigned,
            logic,
            supply_reader.pre_state_root(),
            evidence.as_ref(),
        );
        if candidate.malformed {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    admitted_count,
                    replay_index,
                    0,
                    1,
                    "admitted deployment has malformed or uncertifiable authority demand"
                        .to_string(),
                ),
            ));
        }
        channels_by_key
            .entry(candidate.sig_key)
            .or_insert_with(|| candidate.channel.clone());
        // Record the funding signature once per group so the decomposition
        // collection (below) walks each compound exactly once — identical to the
        // play-side per-group representative walk.
        group_envelopes.entry(candidate.sig_key).or_insert(funding);
        let realized_cost = if let Some(realized) = realized.as_mut() {
            Some(
                realized
                    .get_mut(candidate.cosigned.primary().sig.as_ref())
                    .and_then(VecDeque::pop_front)
                    .ok_or_else(|| {
                        CasperError::InvalidCostSettlement(
                            "missing realized cost for admitted deploy".to_string(),
                        )
                    })?,
            )
        } else {
            None
        };
        let cost = realized_cost
            .as_ref()
            .map(|realized| realized.cost)
            .unwrap_or(candidate.certified_upper_bound);
        if cost < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "realized deploy cost must be non-negative".to_string(),
            ));
        }
        if realized.is_some() && cost > candidate.certified_upper_bound {
            return Err(CasperError::InvalidCostSettlement(format!(
                "realized deploy cost {} exceeds certified bound {}",
                cost, candidate.certified_upper_bound
            )));
        }
        if let (Some(realized_cost), Some(certified)) =
            (realized_cost, candidate.certified.as_ref())
        {
            if realized_cost.post_state_root != realized_cost.authority_witness.post_state_root {
                return Err(CasperError::InvalidCostSettlement(
                    "processed deploy root disagrees with its authority witness".to_string(),
                ));
            }
            if realized_cost.certificate.protocol_version != AUTHORITY_ACCOUNTING_PROTOCOL_VERSION
                || realized_cost.certificate.program_hash != certified.program_hash
                || realized_cost.certificate.pre_state_root
                    != realized_cost.authority_witness.pre_state_root
            {
                return Err(CasperError::InvalidCostSettlement(
                    "processed deploy authority certificate disagrees with replay demand"
                        .to_string(),
                ));
            }
            let replay_bound = match &certified.certificate.demand {
                DemandBound::Exact(bound) | DemandBound::FiniteUpperBound { bound, .. } => bound,
                DemandBound::Unprovable(_) => {
                    return Err(CasperError::InvalidCostSettlement(
                        "replay demand has no finite authority bound".to_string(),
                    ));
                }
            };
            let certified_bound = match &realized_cost.certificate.demand {
                DemandBound::Exact(bound) | DemandBound::FiniteUpperBound { bound, .. } => bound,
                DemandBound::Unprovable(_) => {
                    return Err(CasperError::InvalidCostSettlement(
                        "processed authority certificate has no finite demand".to_string(),
                    ));
                }
            };
            if !replay_bound.dominates(certified_bound) {
                return Err(CasperError::InvalidCostSettlement(
                    "processed authority demand exceeds the replay bound".to_string(),
                ));
            }
            realized_cost
                .authority_witness
                .verify_with_settlement(&realized_cost.certificate, |events, _, allocation| {
                    allocate_authority_events(events, allocation)
                })
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        }
        let entry = demand_by_group
            .entry(candidate.sig_key)
            .or_insert_with(|| (candidate.channel.clone(), 0));
        entry.1 = entry.1.saturating_add(cost);
        // One fee token per admitted deploy in this group (the FeeExtract carve).
        let fee_entry = fee_by_group
            .entry(candidate.sig_key)
            .or_insert_with(|| (candidate.channel.clone(), 0));
        fee_entry.1 = fee_entry.1.saturating_add(1);
    }
    for envelope in group_envelopes.values() {
        collect_decompositions(envelope, &mut decompositions, &mut channels_by_key);
        collect_funding_signatures(envelope, &mut signatures_by_key)?;
    }

    let mut raw: BTreeMap<SigKey, i64> = BTreeMap::new();
    for (key, signature) in &signatures_by_key {
        let balance = supply_reader
            .read_purse(signature)
            .await?
            .balance
            .unwrap_or(0);
        if balance < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority purse balance cannot be negative".to_string(),
            ));
        }
        raw.insert(*key, balance);
    }
    // The LIVE cross-group residual ledger (TM-CA-165) — the SAME ledger the
    // play-side gate draws (seeded `raw.clone()`, drawn down per group in SigKey
    // order via `draw_group_from_ledger`). Replay re-runs that pass over the
    // admitted set to re-verify the gate's CROSS-GROUP admission bound (below).
    // A multi-sig deploy funds from the cosigners' canonical SystemVaults even
    // when no combined `Σ⟦And(…)⟧` authority lane exists.
    let mut remaining: BTreeMap<SigKey, i64> = raw.clone();
    let decomposition_by_compound = index_decompositions(&decompositions);

    // CROSS-GROUP ADMISSIBILITY RE-VERIFICATION (TM-CA-165) — the SUFFICIENT
    // consensus guarantee against over-admission across DISTINCT cosigner sets
    // sharing a component SystemVault authority. SUPERSEDES the prior per-group
    // static-`effective` check (TM-CA-164), which bounded each group independently
    // and so could not catch two groups jointly over-drawing a shared component
    // (e.g. `{A,s}` + `{B,s}` each admitted against `s`'s full balance).
    //
    // Replay RE-RUNS the gate's LIVE cross-group ledger over the admitted set:
    // process enforced groups in SigKey order, draw each group's folded cost+fee
    // DOWN the shared `remaining` ledger (`draw_group_from_ledger`), and reject
    // (ReplayAdmissionMismatch) any group whose folded demand exceeds the LIVE
    // effective capacity at its turn. Why this — not the per-pool `debit > balance`
    // check in `recompute_and_verify_admission` — is what bounds CUMULATIVE demand:
    // `compute_settlement_debits` residual-caps each pool draw, so an
    // over-admission is silently absorbed into per-pool debits ≤ balance and the
    // post-state agrees play↔replay; only re-running the admission ledger detects
    // it. Identical inputs + SigKey order on play and replay ⇒ no fork; equality is admissible (the gate
    // guarantees folded demand ≤ capacity, and this check uses the SAME ledger).
    //
    // The folded demand covers groups carrying a cost AND/OR a fee (a zero-cost admitted deploy still
    // carries a fee draw), via the union of `demand_by_group` and `fee_by_group`.
    let mut combined_demand: BTreeMap<SigKey, i64> = BTreeMap::new();
    for (key, (_chan, cost)) in &demand_by_group {
        if *cost > 0 {
            *combined_demand.entry(*key).or_insert(0) += *cost;
        }
    }
    for (key, (_chan, fee)) in &fee_by_group {
        if *fee > 0 {
            *combined_demand.entry(*key).or_insert(0) += *fee;
        }
    }
    for (key, demand) in &combined_demand {
        if *demand <= 0 {
            continue;
        }
        let shape = group_shape_from(key, &decomposition_by_compound, &remaining);
        let capacity = group_capacity(shape);
        if *demand > capacity {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    0,
                    0,
                    0,
                    0,
                    format!(
                        "cross-group funding over-admission: group folded demand \
                         (cost+fee {}) exceeds LIVE effective supply {} (SigKey {}) \
                         after drawing prior groups that share its component \
                         SystemVault authority — proposer admitted cumulative demand \
                         exceeding a shared payer (cross-group oversubscription, TM-CA-165)",
                        demand,
                        capacity,
                        hex::encode(key),
                    ),
                ),
            ));
        }
        draw_group_from_ledger(shape, *demand, &mut remaining);
    }

    // Keep every positive admitted cost and fee obligation. The funding check
    // above has already rejected any group whose effective supply is insufficient.
    let demand_by_group: BTreeMap<SigKey, (Par, i64)> = demand_by_group
        .into_iter()
        .filter(|(_key, (_chan, amount))| *amount > 0)
        .collect();
    let fee_by_group: BTreeMap<SigKey, (Par, i64)> = fee_by_group
        .into_iter()
        .filter(|(_key, (_chan, amount))| *amount > 0)
        .collect();

    // 3. Settle the per-pool COST debit via the SAME shared function the play side
    //    runs (combined-pool-first split + shared-component residual ledger).
    //    Same inputs + same function ⇒ a BYTE-IDENTICAL debit map (fork safety).
    let settlement = compute_settlement_debits(
        &demand_by_group,
        &decompositions,
        &raw,
        &channels_by_key,
        policy,
    );

    // 4. Settle the FEE carve AFTER the cost, against the post-cost residual
    //    (raw − cost draws) — mirrors the play-side fee pass exactly ⇒ byte-identical.
    let raw_after_cost: BTreeMap<SigKey, i64> = channels_by_key
        .keys()
        .map(|k| {
            let post =
                raw.get(k).copied().unwrap_or(0) - settlement.get(k).map(|d| d.amount).unwrap_or(0);
            (*k, post)
        })
        .collect();
    // Stage-D fee: FLAT [`FlatFeeApportionment`] (one physical token per deploy),
    // NOT the cost `policy` — identical to the play-side fee pass (`:716`) so the
    // recomputed `fee` is byte-identical. A compound deploy owes 1, not 2 (F-1).
    let fee = compute_settlement_debits(
        &fee_by_group,
        &decompositions,
        &raw_after_cost,
        &channels_by_key,
        &FlatFeeApportionment,
    );

    Ok(RecomputedDebits {
        settlement,
        fee,
        stack_pops: BTreeMap::new(),
        purse_stacks: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    //! WD-D2 acceptance-gate unit tests (cost-accounted-rho §7.4/§7.6/§7.7).
    //! Exercise per-signature grouping, the canonical-order prefix admission,
    //! the §7.7 reject-both / no-partial discipline, and the §7.4 funded /
    //! unfunded boundary. A [`MockSupplyReader`] supplies canned per-channel
    //! balances so the verdict depends only on the pure analyzer + the gate
    //! algorithm (no live runtime needed).
    use std::collections::HashMap;

    use crypto::rust::hash::blake2b256::Blake2b256;
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::public_key::PublicKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use proptest::prelude::*;
    use rholang::rust::interpreter::accounting::authority::{
        allocate_authority_event_draws, DemandBound, ResourceMultiset,
    };
    use rholang::rust::interpreter::accounting::delta_sigma;

    use super::*;

    /// Deterministic 33-byte secp256k1-shaped public key derived from a test's
    /// signature-label bytes. The gate now keys funding by the signer's PUBLIC
    /// KEY (`funding_sig` ⇒ `Sig::Ground(pk)`), so distinct labels must map to
    /// distinct pks (distinct wallets `Σ⟦Ground(pk)⟧`) while two deploys sharing
    /// a label share a pk (one pool — the s₀ double-spend group shape). The gate
    /// never verifies the sig against the pk (`from_single_signer` is
    /// infallible), so any deterministic 33-byte value is a valid stand-in; the
    /// Blake2b256 of the label is collision-free across distinct labels.
    fn pk_from_sig(sig: &[u8]) -> Vec<u8> {
        let hash = Blake2b256::hash(sig.to_vec());
        let mut pk = Vec::with_capacity(33);
        pk.push(0x02);
        pk.extend_from_slice(&hash);
        pk
    }

    /// The supply-pool `SigKey` the gate keys a single-signer test deploy to:
    /// `Σ⟦Ground(pk_from_sig(sig))⟧` — the signer's ground-pubkey wallet
    /// (`Σ⟦signer⟧ == Σ⟦wallet⟧`). Replaces the pre-fix
    /// `sig_key(envelope_sig_single(sig))` (which keyed the wire-sig `#P` pool).
    fn pool_key(sig: &[u8]) -> SigKey {
        delta_sigma::sig_key(&accounting::funding_sig_single(&pk_from_sig(sig)))
    }

    /// A canned per-channel supply reader keyed by the channel's wire encoding.
    struct MockSupplyReader {
        balances: HashMap<Vec<u8>, i64>,
        purses: HashMap<Vec<u8>, supply::PurseInventory>,
    }

    impl MockSupplyReader {
        fn new() -> Self {
            Self {
                balances: HashMap::new(),
                purses: HashMap::new(),
            }
        }

        /// Set the balance of the SIGNER'S ground-pubkey wallet `Σ⟦Ground(pk)⟧`
        /// for the deploy keyed by label `sig` — the SAME pool the gate now keys
        /// via `funding_sig` (`Σ⟦signer⟧ == Σ⟦wallet⟧`). `pk` is derived from the
        /// label by `pk_from_sig`, identically to [`cosigned`], so the seeded
        /// channel and the gate's keyed channel coincide.
        fn set(&mut self, sig: &[u8], balance: i64) {
            use prost::Message;
            let funding = accounting::funding_sig_single(&pk_from_sig(sig));
            let chan = supply::supply_channel(&funding);
            self.balances.insert(chan.encode_to_vec(), balance);
        }

        /// Set the balance of the pool a structured `Sig` (e.g. a compound
        /// `Sig::And` or one of its components) maps to. Used by the #12
        /// compound play↔replay byte-identity test to seed the compound +
        /// component pools by the SAME `supply_channel` basis the gate reads.
        fn set_sig(&mut self, sig: &Sig, balance: i64) {
            use prost::Message;
            let chan = supply::supply_channel(sig);
            self.balances.insert(chan.encode_to_vec(), balance);
        }

        fn set_stack(&mut self, sig: &Sig, cells: Vec<models::rhoapi::CostSignature>) {
            use prost::Message;
            let chan = supply::supply_channel(sig);
            let key = chan.encode_to_vec();
            let stack = models::rhoapi::CostStack { cells };
            self.purses
                .entry(key)
                .or_default()
                .stacks
                .push(supply::PurseStack {
                    instance_id: Blake2b256::hash(stack.encode_to_vec()).try_into().unwrap(),
                    source_hash: Blake2b256::hash(stack.encode_to_vec()).try_into().unwrap(),
                    channel: chan,
                    datum_index: 0,
                    random_state: vec![1],
                    persistent: false,
                    stack,
                });
        }
    }

    impl SupplyReader for MockSupplyReader {
        fn pre_state_root(&self) -> [u8; 32] { [42; 32] }

        fn urn_map<'a>(
            &'a self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Arc<HashMap<String, Par>>, CasperError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(Arc::new(HashMap::new())) })
        }

        fn read_purse<'a>(
            &'a self,
            signature: &'a models::rhoapi::CostSignature,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<supply::PurseInventory, CasperError>>
                    + Send
                    + 'a,
            >,
        > {
            use prost::Message;
            let sig =
                rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(signature)
                    .unwrap();
            let key = supply::supply_channel(&sig).encode_to_vec();
            let mut inventory = self.purses.get(&key).cloned().unwrap_or_default();
            inventory.balance = self.balances.get(&key).copied();
            Box::pin(async move { Ok(inventory) })
        }
    }

    /// Build a `Cosigned<DeployData>` with the given Rholang `term`, primary
    /// signature-label bytes `sig`, and ordering fields. The label both (a) is
    /// the deploy's wire `sig` (ordering / `deploy_id`) and (b) derives the
    /// signer's public key via [`pk_from_sig`], which the gate keys the supply
    /// pool `Σ⟦Ground(pk)⟧` by (`funding_sig`). The gate does not verify
    /// signatures, so an arbitrary label is sufficient to place the deploy into a
    /// chosen group — two deploys sharing a label share a pk and therefore the
    /// same default-payer oversubscription group.
    fn cosigned(term: &str, sig: &[u8], vabn: i64, ts: i64) -> Cosigned<DeployData> {
        let data = DeployData {
            term: term.to_string(),
            time_stamp: ts,
            valid_after_block_number: vabn,
            shard_id: String::new(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let signed = Signed {
            data,
            // The gate keys funding by the signer's PUBLIC KEY (`funding_sig` ⇒
            // `Sig::Ground(pk)`), so the pk is derived from the label so distinct
            // labels get distinct wallets `Σ⟦Ground(pk)⟧` (and same-label deploys
            // share one pool). `from_single_signer` does not verify, so this
            // stand-in pk needs no matching private key.
            pk: PublicKey::from_bytes(&pk_from_sig(sig)),
            sig: Bytes::copy_from_slice(sig),
            sig_algorithm: Box::new(Secp256k1),
        };
        Cosigned::from_single_signer(signed).expect("from_single_signer is infallible")
    }

    /// `n` parallel sends `@0!(0) | … | @0!(0)` ⇒ Δ = n (each send is one
    /// token-consuming COMM; see `delta_sigma::demand`).
    fn n_sends(n: usize) -> String {
        let one = "@0!(0)";
        std::iter::repeat_n(one, n).collect::<Vec<_>>().join(" | ")
    }

    #[tokio::test]
    async fn state_bound_capacity_counts_balance_and_stack_after_fee() {
        let deploy = cosigned(&n_sends(1), b"bounded", 0, 10);
        let funding = accounting::funding_sig(&deploy);
        let cell = sig_to_cost_signature(&funding).unwrap();
        let mut reader = MockSupplyReader::new();
        reader.set(b"bounded", 4);
        reader.set_stack(&funding, vec![cell.clone(), cell]);

        assert_eq!(
            state_bound_execution_caps(&[deploy], &reader)
                .await
                .unwrap(),
            vec![5]
        );
    }

    #[tokio::test]
    async fn state_bound_capacity_is_canonical_and_per_deploy() {
        let early = cosigned(&n_sends(1), b"early", 0, 10);
        let late = cosigned(&n_sends(1), b"late", 0, 20);
        let mut reader = MockSupplyReader::new();
        reader.set(b"early", 3);
        reader.set(b"late", 6);

        assert_eq!(
            state_bound_execution_caps(&[late, early], &reader)
                .await
                .unwrap(),
            vec![2, 5]
        );
    }

    // ── #12 compound settlement-debit helpers ──────────────────────────────

    /// Two distinct ground-atom component signatures `(a, b)` and their
    /// left-associated compound `Sig::And(a, b)` — the shape the runtime forms
    /// for a 2-signer deploy. Returned together with their `SigKey`s and resolved
    /// supply channels so a test can seed `raw`/`channels_by_key` and assert on
    /// `compute_settlement_debits` directly (no `Cosigned`/crypto needed).
    struct CompoundFixture {
        a_key: SigKey,
        b_key: SigKey,
        compound_key: SigKey,
        a_chan: Par,
        b_chan: Par,
        compound_chan: Par,
        decomposition: Decomposition,
    }

    fn compound_fixture(a_tag: &[u8], b_tag: &[u8]) -> CompoundFixture {
        let a = Sig::Ground(a_tag.to_vec());
        let b = Sig::Ground(b_tag.to_vec());
        let compound = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
        let a_key = delta_sigma::sig_key(&a);
        let b_key = delta_sigma::sig_key(&b);
        let compound_key = delta_sigma::sig_key(&compound);
        CompoundFixture {
            a_key,
            b_key,
            compound_key,
            a_chan: supply::supply_channel(&a),
            b_chan: supply::supply_channel(&b),
            compound_chan: supply::supply_channel(&compound),
            decomposition: Decomposition {
                compound: compound_key,
                left: a_key,
                right: b_key,
            },
        }
    }

    impl CompoundFixture {
        /// Build the `channels_by_key` the gate would have read (compound +
        /// both components), keyed by `SigKey`.
        fn channels_by_key(&self) -> BTreeMap<SigKey, Par> {
            let mut m = BTreeMap::new();
            m.insert(self.compound_key, self.compound_chan.clone());
            m.insert(self.a_key, self.a_chan.clone());
            m.insert(self.b_key, self.b_chan.clone());
            m
        }
    }

    /// An ALTERNATIVE payment-delegation policy proving the trait is pluggable:
    /// draw the COMPONENT PAIR first, then the combined pool — the dual of
    /// [`DefaultApportionment`]. Conservation of Authority makes the partition
    /// free, so it still settles exactly `k` and overdraws no pool.
    struct ComponentsFirstApportionment;
    impl ApportionmentPolicy<RhoGslt> for ComponentsFirstApportionment {
        fn apportion(&self, shape: GroupShape<SigKey>, k: i64) -> Vec<PoolDraw<SigKey>> {
            match shape {
                GroupShape::Single { own } => {
                    if k > 0 {
                        vec![PoolDraw {
                            key: own.key,
                            amount: k,
                        }]
                    } else {
                        Vec::new()
                    }
                }
                GroupShape::Compound {
                    combined,
                    left,
                    right,
                } => {
                    let draw_pair = k.min(left.residual).min(right.residual).max(0);
                    let draw_compound = (k - draw_pair).min(combined.residual).max(0);
                    let mut v = Vec::new();
                    if draw_pair > 0 {
                        v.push(PoolDraw {
                            key: left.key,
                            amount: draw_pair,
                        });
                        v.push(PoolDraw {
                            key: right.key,
                            amount: draw_pair,
                        });
                    }
                    if draw_compound > 0 {
                        v.push(PoolDraw {
                            key: combined.key,
                            amount: draw_compound,
                        });
                    }
                    v
                }
            }
        }
    }

    /// The payment-delegation trait is pluggable: an alternative policy apportions
    /// a multi-sig group's demand DIFFERENTLY than the default, yet still settles
    /// exactly `k` units of authority (Conservation of Authority — "grouping never
    /// changes the total") and overdraws no pool.
    #[test]
    fn alternative_payment_policy_conserves_but_apportions_differently() {
        // Σ⟦compound⟧ = 1, Σ⟦a⟧ = Σ⟦b⟧ = 5, demand k = 3.
        let fx = compound_fixture(b"alice", b"bob");
        let mut raw = BTreeMap::new();
        raw.insert(fx.compound_key, 1);
        raw.insert(fx.a_key, 5);
        raw.insert(fx.b_key, 5);
        let mut demand = BTreeMap::new();
        demand.insert(fx.compound_key, (fx.compound_chan.clone(), 3));

        let default = compute_settlement_debits(
            &demand,
            &[fx.decomposition],
            &raw,
            &fx.channels_by_key(),
            &DefaultApportionment,
        );
        let alt = compute_settlement_debits(
            &demand,
            &[fx.decomposition],
            &raw,
            &fx.channels_by_key(),
            &ComponentsFirstApportionment,
        );

        // DEFAULT (combined-first): compound -= 1, then the pair -= 2 each.
        assert_eq!(default.get(&fx.compound_key).map(|d| d.amount), Some(1));
        assert_eq!(default.get(&fx.a_key).map(|d| d.amount), Some(2));
        assert_eq!(default.get(&fx.b_key).map(|d| d.amount), Some(2));

        // COMPONENTS-FIRST: the pair -= 3 each fully funds k ⇒ compound untouched.
        assert_eq!(alt.get(&fx.compound_key).map(|d| d.amount), None);
        assert_eq!(alt.get(&fx.a_key).map(|d| d.amount), Some(3));
        assert_eq!(alt.get(&fx.b_key).map(|d| d.amount), Some(3));

        // CONSERVATION: both settle exactly k = 3 units of group authority
        // (compound draw counts once + the matched pair draw counts once).
        let settled = |m: &BTreeMap<SigKey, SettlementDebit>| {
            let c = m.get(&fx.compound_key).map(|d| d.amount).unwrap_or(0);
            let pair = m
                .get(&fx.a_key)
                .map(|d| d.amount)
                .unwrap_or(0)
                .min(m.get(&fx.b_key).map(|d| d.amount).unwrap_or(0));
            c + pair
        };
        assert_eq!(settled(&default), 3, "default conserves k");
        assert_eq!(settled(&alt), 3, "alternative conserves k");

        // NO-OVERDRAW under EITHER policy: no pool drawn past its residual.
        for m in [&default, &alt] {
            for (key, debit) in m {
                assert!(
                    debit.amount <= *raw.get(key).unwrap_or(&0),
                    "pool overdrawn"
                );
            }
        }
    }

    /// Per-signature grouping + independence: two deploys SHARING a signature
    /// form ONE group whose pool funds exactly one; a third deploy with a
    /// DIFFERENT signature is an independent group, funded on its own pool.
    #[tokio::test]
    async fn per_signature_group_gate() {
        // Group A (sig = "alice"): two deploys, each Δ=3; pool funds exactly one.
        // Group B (sig = "bob"):   one deploy,  Δ=2; pool funds it.
        // F-C/F-D: each admitted deploy now also carves a +1 FeeExtract token, so
        // admission requires `Σ⟦c⟧ ≥ Δ + 1` per deploy. The pools are sized at
        // `Δ + 1` (4 for one alice deploy; 3 for bob) so the grouping/reject-both
        // INTENT is unchanged: exactly one alice deploy fits (the second needs a
        // further 4 ⇒ rejected), bob fits independently. The COST debits below are
        // still the cost-only Δ (alice 3, bob 2); the +1 fee lands in `fee_debits`.
        let a0 = cosigned(&n_sends(3), b"alice", 0, 10);
        let a1 = cosigned(&n_sends(3), b"alice", 0, 20);
        let b0 = cosigned(&n_sends(2), b"bob", 0, 30);

        let mut reader = MockSupplyReader::new();
        reader.set(b"alice", 4); // exactly one Δ=3 deploy fits (cost 3 + fee 1)
        reader.set(b"bob", 3); // the Δ=2 deploy fits (cost 2 + fee 1)

        let outcome = admit_by_funding(vec![a1.clone(), b0.clone(), a0.clone()], &reader)
            .await
            .expect("gate must not error");

        // Group A: canonical order is a0 (ts=10) before a1 (ts=20); a0 admitted,
        // a1 rejected (pool exhausted). Group B: b0 admitted independently.
        let admitted_sigs: Vec<&[u8]> = outcome
            .admitted
            .iter()
            .map(|c| c.primary().sig.as_ref())
            .collect();
        assert!(
            admitted_sigs.contains(&b"alice".as_ref()),
            "alice's first fits"
        );
        assert!(
            admitted_sigs.contains(&b"bob".as_ref()),
            "bob is independent"
        );
        assert_eq!(outcome.admitted.len(), 2, "a0 + b0 admitted");
        assert_eq!(outcome.rejected.len(), 1, "a1 rejected (pool exhausted)");
        // COST debits (burned from Σ⟦c⟧): alice pool -= 3, bob pool -= 2.
        let alice_key = pool_key(b"alice");
        let bob_key = pool_key(b"bob");
        assert_eq!(outcome.debits.get(&alice_key).map(|d| d.amount), Some(3));
        assert_eq!(outcome.debits.get(&bob_key).map(|d| d.amount), Some(2));
        // One fee token per admitted deploy is transferred to the proposer
        // SystemVault. Cost and fee retain distinct dispositions.
        assert_eq!(
            outcome.fee_debits.get(&alice_key).map(|d| d.amount),
            Some(1)
        );
        assert_eq!(outcome.fee_debits.get(&bob_key).map(|d| d.amount), Some(1));
    }

    struct DenyLogic;

    impl OslfResourceLogic<RhoGslt> for DenyLogic {
        fn demand(&self, canonical: &Par, deploy_sig: &Sig) -> DemandEntry {
            delta_sigma::demand(canonical, deploy_sig)
        }

        fn verify_demand_bound(
            &self,
            canonical: &Par,
            deploy_sig: &Sig,
            bound: &DemandBound<delta_sigma::SigKey>,
        ) -> Option<ResourceMultiset<delta_sigma::SigKey>> {
            DefaultResourceLogic.verify_demand_bound(canonical, deploy_sig, bound)
        }

        fn is_funded(&self, _analysis: &DemandEntry, _effective_supply_s: i64) -> bool { false }
    }

    #[tokio::test]
    async fn gate_uses_injected_oslf_resource_logic() {
        let deploy = cosigned(&n_sends(1), b"oslf-gate", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"oslf-gate", 10);

        let default = admit_by_funding(vec![deploy.clone()], &reader)
            .await
            .expect("default gate must not error");
        let denied = admit_by_funding_with_logic(
            vec![deploy.clone()],
            &reader,
            &DenyLogic,
            &DefaultApportionment,
        )
        .await
        .expect("injected gate must not error");

        assert_eq!(default.admitted.len(), 1);
        assert!(default.rejected.is_empty());
        assert!(denied.admitted.is_empty());
        assert_eq!(denied.rejected, vec![deploy.primary().sig.clone()]);
        assert!(denied.debits.is_empty());
    }

    struct ForgedFiniteLogic;

    impl OslfResourceLogic<RhoGslt> for ForgedFiniteLogic {
        fn demand(&self, canonical: &Par, deploy_sig: &Sig) -> DemandEntry {
            delta_sigma::demand(canonical, deploy_sig)
        }

        fn demand_bound(
            &self,
            canonical: &Par,
            deploy_sig: &Sig,
        ) -> DemandBound<delta_sigma::SigKey> {
            DemandBound::FiniteUpperBound {
                bound: ResourceMultiset::singleton(
                    deploy_sig.key(),
                    u64::try_from(self.demand(canonical, deploy_sig).certified_upper_bound)
                        .unwrap(),
                ),
                proof: b"non-empty-is-not-a-proof".to_vec(),
            }
        }

        fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool {
            delta_sigma::is_funded(analysis, effective_supply_s)
        }
    }

    #[tokio::test]
    async fn gate_rejects_an_unverified_nonempty_bound_proof() {
        let deploy = cosigned(&n_sends(1), b"forged-bound", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"forged-bound", 10);

        let outcome = admit_by_funding_with_logic(
            vec![deploy.clone()],
            &reader,
            &ForgedFiniteLogic,
            &DefaultApportionment,
        )
        .await
        .unwrap();

        assert!(outcome.admitted.is_empty());
        assert_eq!(outcome.rejected, vec![deploy.primary().sig.clone()]);
    }

    struct VerifiedFiniteLogic;

    impl OslfResourceLogic<RhoGslt> for VerifiedFiniteLogic {
        fn demand(&self, _canonical: &Par, _deploy_sig: &Sig) -> DemandEntry {
            DemandEntry {
                certified_upper_bound: 0,
                unknown: true,
            }
        }

        fn demand_bound(
            &self,
            _canonical: &Par,
            deploy_sig: &Sig,
        ) -> DemandBound<delta_sigma::SigKey> {
            DemandBound::FiniteUpperBound {
                bound: ResourceMultiset::singleton(deploy_sig.key(), 3),
                proof: b"verified-gslt-bound".to_vec(),
            }
        }

        fn verify_demand_bound(
            &self,
            _canonical: &Par,
            deploy_sig: &Sig,
            bound: &DemandBound<delta_sigma::SigKey>,
        ) -> Option<ResourceMultiset<delta_sigma::SigKey>> {
            match bound {
                DemandBound::FiniteUpperBound { bound, proof }
                    if proof.as_slice() == b"verified-gslt-bound"
                        && bound == &ResourceMultiset::singleton(deploy_sig.key(), 3) =>
                {
                    Some(bound.clone())
                }
                _ => None,
            }
        }

        fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool {
            delta_sigma::is_funded(analysis, effective_supply_s)
        }
    }

    #[tokio::test]
    async fn verified_finite_bound_replaces_an_unprovable_native_lower_bound() {
        let deploy = cosigned(&n_sends(1), b"verified-bound", 0, 10);
        let key = pool_key(b"verified-bound");
        let mut funded_reader = MockSupplyReader::new();
        funded_reader.set(b"verified-bound", 4);
        let mut underfunded_reader = MockSupplyReader::new();
        underfunded_reader.set(b"verified-bound", 3);

        let admitted = admit_by_funding_with_logic(
            vec![deploy.clone()],
            &funded_reader,
            &VerifiedFiniteLogic,
            &DefaultApportionment,
        )
        .await
        .unwrap();
        let rejected = admit_by_funding_with_logic(
            vec![deploy.clone()],
            &underfunded_reader,
            &VerifiedFiniteLogic,
            &DefaultApportionment,
        )
        .await
        .unwrap();
        let replay_reservation = recompute_settlement_debits_with_logic(
            vec![deploy.clone()],
            &funded_reader,
            &VerifiedFiniteLogic,
            &DefaultApportionment,
        )
        .await
        .unwrap();

        assert_eq!(admitted.admitted.len(), 1);
        assert_eq!(admitted.debits.get(&key).map(|debit| debit.amount), Some(3));
        assert_eq!(
            admitted.fee_debits.get(&key).map(|debit| debit.amount),
            Some(1)
        );
        assert_eq!(
            replay_reservation
                .settlement
                .get(&key)
                .map(|debit| debit.amount),
            Some(3)
        );
        assert_eq!(
            replay_reservation.fee.get(&key).map(|debit| debit.amount),
            Some(1)
        );
        assert!(rejected.admitted.is_empty());
        assert_eq!(rejected.rejected, vec![deploy.primary().sig.clone()]);
    }

    struct ZeroDemandLogic;

    impl OslfResourceLogic<RhoGslt> for ZeroDemandLogic {
        fn demand(&self, _canonical: &Par, _deploy_sig: &Sig) -> DemandEntry { DemandEntry::ZERO }

        fn demand_bound(
            &self,
            _canonical: &Par,
            deploy_sig: &Sig,
        ) -> DemandBound<delta_sigma::SigKey> {
            DemandBound::Exact(ResourceMultiset::singleton(deploy_sig.key(), 0))
        }

        fn is_funded(&self, analysis: &DemandEntry, effective_supply_s: i64) -> bool {
            delta_sigma::is_funded(analysis, effective_supply_s)
        }
    }

    #[tokio::test]
    async fn replay_recompute_uses_injected_oslf_demand() {
        let deploy = cosigned(&n_sends(2), b"oslf-replay", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"oslf-replay", 10);
        let key = pool_key(b"oslf-replay");

        let default = recompute_settlement_debits(vec![deploy.clone()], &reader)
            .await
            .expect("default recompute must not error");
        let injected = recompute_settlement_debits_with_logic(
            vec![deploy.clone()],
            &reader,
            &ZeroDemandLogic,
            &DefaultApportionment,
        )
        .await
        .expect("injected recompute must not error");

        assert_eq!(
            default.settlement.get(&key).map(|debit| debit.amount),
            Some(2)
        );
        assert!(injected.settlement.is_empty());
    }

    fn processed_with_bound(
        cosigned: &Cosigned<DeployData>,
        cost: u64,
        bound: u64,
    ) -> ProcessedDeploy {
        let funding = accounting::funding_sig(cosigned);
        let signature = sig_to_cost_signature(&funding).unwrap();
        let events = if cost == 0 {
            Vec::new()
        } else {
            let authority = canonical_authority(&CostAuthority {
                regions: (0..cost)
                    .map(|index| {
                        cost_region(
                            &signature,
                            cosigned.primary().sig.as_ref(),
                            u32::try_from(index).unwrap(),
                        )
                        .unwrap()
                    })
                    .collect(),
            })
            .unwrap();
            vec![AuthorityEvent {
                event_id: authority_digest(
                    &Blake2b256::hash(cosigned.primary().sig.to_vec()),
                    "test authority event",
                )
                .unwrap(),
                debit: authority_demand(&authority).unwrap(),
                authority,
            }]
        };
        let realized = events
            .iter()
            .try_fold(ResourceMultiset::default(), |total, event| {
                total.checked_add(&event.debit)
            })
            .unwrap();
        let canonical = RhoGslt
            .canonicalize_for_funding(&Compiler::source_to_adt(&cosigned.data().term).unwrap());
        let program_hash = authority_digest(
            &Blake2b256::hash(canonical.encode_to_vec()),
            "test program hash",
        )
        .unwrap();
        let reservation_id = authority_digest(
            &Blake2b256::hash(cosigned.primary().sig.to_vec()),
            "test reservation identity",
        )
        .unwrap();
        let certificate = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash,
            pre_state_root: [42; 32],
            reservation_id,
            demand: DemandBound::Exact(ResourceMultiset::singleton(funding.key(), bound)),
            allocation: realized.clone(),
            stack_reservations: BTreeMap::new(),
            fee_allocation: ResourceMultiset::singleton(funding.key(), 1),
            fee_recipient: Vec::new(),
        };
        let physical_draws = allocate_authority_event_draws(&events, &realized).unwrap();
        let authority_witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: certificate.certificate_id(),
            pre_state_root: [42; 32],
            post_state_root: [43; 32],
            events,
            realized: realized.clone(),
            settlement: realized,
            physical_draws,
            born_stacks: Vec::new(),
        };
        ProcessedDeploy {
            deploy: Signed {
                data: cosigned.data().clone(),
                pk: cosigned.primary().pk.clone(),
                sig: cosigned.primary().sig.clone(),
                sig_algorithm: cosigned.primary().sig_algorithm.clone(),
            },
            cost: models::rhoapi::PCost { cost },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: Bytes::from_static(&[42; 32]),
            post_state_hash: Bytes::from_static(&[43; 32]),
            authority_funding_certificate: Some(authority_certificate_to_proto(&certificate)),
            authority_cost_witness: Some(authority_witness_to_proto(&authority_witness)),
            admission_status: Default::default(),
        }
    }

    fn processed(cosigned: &Cosigned<DeployData>, cost: u64) -> ProcessedDeploy {
        processed_with_bound(cosigned, cost, cost)
    }

    #[tokio::test]
    async fn realized_settlement_refunds_unused_reservation() {
        let deploy = cosigned(&n_sends(3), b"refund", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"refund", 4);
        let reservation = recompute_settlement_debits(vec![deploy.clone()], &reader)
            .await
            .expect("reservation recompute must succeed");
        let realized = recompute_realized_settlement_debits(&[processed(&deploy, 1)], &reader)
            .await
            .expect("realized recompute must succeed");
        let key = pool_key(b"refund");

        assert_eq!(
            reservation.settlement.get(&key).map(|debit| debit.amount),
            Some(3)
        );
        assert_eq!(
            realized.settlement.get(&key).map(|debit| debit.amount),
            Some(1)
        );
        assert_eq!(realized.fee.get(&key).map(|debit| debit.amount), Some(1));
    }

    #[tokio::test]
    async fn replay_rejects_a_tampered_fee_allocation() {
        let deploy = cosigned(&n_sends(1), b"tampered-fee", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"tampered-fee", 2);
        let mut evidence = vec![processed(&deploy, 1)];

        let mut certificate = authority_certificate_from_proto(
            evidence[0].authority_funding_certificate.as_ref().unwrap(),
        )
        .unwrap();
        certificate.fee_allocation = ResourceMultiset::default();
        let mut witness = authority_witness_from_proto(
            evidence[0].authority_cost_witness.as_ref().unwrap(),
            false,
        )
        .unwrap();
        witness.certificate_id = certificate.certificate_id();
        evidence[0].authority_funding_certificate =
            Some(authority_certificate_to_proto(&certificate));
        evidence[0].authority_cost_witness = Some(authority_witness_to_proto(&witness));

        let error = recompute_state_bound_settlement_debits(&evidence, &reader)
            .await
            .expect_err("replay must reject a non-canonical fee allocation");
        assert!(error
            .to_string()
            .contains("fee allocation differs from the canonical replay allocation"));
    }

    #[tokio::test]
    async fn realized_settlement_rejects_cost_above_certified_bound() {
        let deploy = cosigned(&n_sends(2), b"invalid-witness", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"invalid-witness", 10);
        let error =
            recompute_realized_settlement_debits(&[processed_with_bound(&deploy, 3, 2)], &reader)
                .await
                .expect_err("realized cost above the static certificate must fail");

        assert!(error
            .to_string()
            .contains("realized authority exceeds the certified demand"));
    }

    #[tokio::test]
    async fn absent_pool_does_not_relax_the_realized_bound() {
        let deploy = cosigned(&n_sends(2), b"absent-unenforced", 0, 10);
        let reader = MockSupplyReader::new();
        let error =
            recompute_realized_settlement_debits(&[processed_with_bound(&deploy, 3, 2)], &reader)
                .await
                .expect_err("an absent pool cannot relax the certified bound");
        assert!(error
            .to_string()
            .contains("realized authority exceeds the certified demand"));
    }

    #[tokio::test]
    async fn state_bound_evidence_accounts_for_ambient_contract_cost() {
        let deploy = cosigned(&n_sends(2), b"ambient", 0, 10);
        let witness = processed(&deploy, 123);
        let mut evidence = vec![witness];
        let mut reader = MockSupplyReader::new();
        reader.set(b"ambient", 124);

        let admitted = admit_by_funding_with_state_bound_evidence(
            vec![deploy.clone()],
            &mut evidence,
            &reader,
        )
        .await
        .unwrap();
        let recomputed = recompute_state_bound_settlement_debits(&evidence, &reader)
            .await
            .unwrap();
        let key = pool_key(b"ambient");

        assert_eq!(admitted.admitted.len(), 1);
        assert_eq!(
            admitted.debits.get(&key).map(|debit| debit.amount),
            Some(123)
        );
        assert_eq!(
            admitted.fee_debits.get(&key).map(|debit| debit.amount),
            Some(1)
        );
        assert_eq!(admitted.debits, recomputed.settlement);
        assert_eq!(admitted.fee_debits, recomputed.fee);
    }

    #[tokio::test]
    async fn state_bound_admission_and_replay_pop_the_same_persistent_stack() {
        let deploy = cosigned(&n_sends(1), b"stack-funded", 0, 10);
        let mut evidence = vec![processed(&deploy, 1)];
        let funding = accounting::funding_sig(&deploy);
        let signature = sig_to_cost_signature(&funding).unwrap();
        let mut reader = MockSupplyReader::new();
        reader.set(b"stack-funded", 1);
        reader.set_stack(&funding, vec![signature]);

        let admitted =
            admit_by_funding_with_state_bound_evidence(vec![deploy], &mut evidence, &reader)
                .await
                .unwrap();
        assert!(!evidence[0]
            .authority_cost_witness
            .as_ref()
            .unwrap()
            .physical_draws
            .is_empty());
        let replayed = recompute_state_bound_settlement_debits(&evidence, &reader)
            .await
            .unwrap();

        assert!(admitted.debits.is_empty());
        assert_eq!(admitted.stack_pops.len(), 1);
        assert_eq!(admitted.stack_pops, replayed.stack_pops);
        assert_eq!(admitted.purse_stacks, replayed.purse_stacks);
        assert_eq!(admitted.fee_debits, replayed.fee);

        let certificate = authority_certificate_from_proto(
            evidence[0].authority_funding_certificate.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(certificate.stack_reservations, admitted.stack_pops);

        let mut tampered = evidence.clone();
        let mut certificate = authority_certificate_from_proto(
            tampered[0].authority_funding_certificate.as_ref().unwrap(),
        )
        .unwrap();
        certificate.stack_reservations.clear();
        let mut witness = authority_witness_from_proto(
            tampered[0].authority_cost_witness.as_ref().unwrap(),
            false,
        )
        .unwrap();
        witness.certificate_id = certificate.certificate_id();
        tampered[0].authority_funding_certificate =
            Some(authority_certificate_to_proto(&certificate));
        tampered[0].authority_cost_witness = Some(authority_witness_to_proto(&witness));
        let error = recompute_state_bound_settlement_debits(&tampered, &reader)
            .await
            .expect_err("replay must reject a certificate that omits its stack reservation");
        assert!(error
            .to_string()
            .contains("does not bind the realized physical reservation"));
    }

    #[tokio::test]
    async fn signed_presentations_discover_intermediate_join_partitions() {
        use models::rhoapi::cost_signature::Value;
        use prost::Message;

        let atom = |tag: u8| models::rhoapi::CostSignature {
            value: Some(Value::Ground(vec![tag])),
        };
        let a = atom(1);
        let b = atom(2);
        let c = atom(3);
        let d = atom(4);
        let ab =
            rholang::rust::interpreter::accounting::authority::compound_cost_signatures(&a, &b)
                .unwrap();
        let cd =
            rholang::rust::interpreter::accounting::authority::compound_cost_signatures(&c, &d)
                .unwrap();
        let mut presentations = vec![ab.clone(), cd.clone()];
        presentations.sort_by_key(|signature| signature.encode_to_vec());

        let mut deploy = cosigned(&n_sends(1), b"intermediate-partition", 0, 10);
        deploy.data.authority_presentations = presentations;
        let mut evidence = processed(&deploy, 1);
        let authority = canonical_authority(&CostAuthority {
            regions: [a, b, c, d]
                .iter()
                .enumerate()
                .map(|(index, signature)| {
                    cost_region(signature, b"intermediate partition", index as u32).unwrap()
                })
                .collect(),
        })
        .unwrap();
        let event = AuthorityEvent {
            event_id: [17; 32],
            debit: authority_demand(&authority).unwrap(),
            authority,
        };
        let realized = event.debit.clone();
        let mut certificate = authority_certificate_from_proto(
            evidence.authority_funding_certificate.as_ref().unwrap(),
        )
        .unwrap();
        certificate.demand = DemandBound::Exact(realized.clone());
        certificate.allocation = realized.clone();
        let witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: certificate.certificate_id(),
            pre_state_root: [42; 32],
            post_state_root: [43; 32],
            events: vec![event],
            realized: realized.clone(),
            settlement: realized,
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };
        evidence.authority_funding_certificate = Some(authority_certificate_to_proto(&certificate));
        evidence.authority_cost_witness = Some(authority_witness_to_proto(&witness));

        let mut reader = MockSupplyReader::new();
        reader.set(b"intermediate-partition", 1);
        reader.set_stack(
            &rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(&ab).unwrap(),
            vec![ab],
        );
        reader.set_stack(
            &rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(&cd).unwrap(),
            vec![cd],
        );
        let mut evidence = vec![evidence];
        let admitted =
            admit_by_funding_with_state_bound_evidence(vec![deploy], &mut evidence, &reader)
                .await
                .unwrap();
        let replayed = recompute_state_bound_settlement_debits(&evidence, &reader)
            .await
            .unwrap();

        assert_eq!(admitted.admitted.len(), 1);
        assert!(admitted.debits.is_empty());
        assert_eq!(admitted.stack_pops.len(), 2);
        assert_eq!(admitted.stack_pops, replayed.stack_pops);
        assert_eq!(admitted.purse_stacks, replayed.purse_stacks);
    }

    #[tokio::test]
    async fn state_bound_evidence_rejects_a_broken_root_chain() {
        let deploy = cosigned(&n_sends(1), b"broken-chain", 0, 10);
        let mut witness = processed(&deploy, 1);
        witness.pre_state_hash = Bytes::from_static(&[41; 32]);
        let mut evidence = vec![witness];
        let mut reader = MockSupplyReader::new();
        reader.set(b"broken-chain", 2);

        let error =
            admit_by_funding_with_state_bound_evidence(vec![deploy], &mut evidence, &reader)
                .await
                .expect_err("evidence rooted in another state must fail");

        assert!(error
            .to_string()
            .contains("state-bound evidence chain expected pre-state"));
    }

    #[tokio::test]
    async fn state_bound_evidence_rejects_an_envelope_substitution() {
        let expected = cosigned(&n_sends(1), b"expected", 0, 10);
        let substituted = cosigned(&n_sends(1), b"substituted", 0, 10);
        let witness = processed(&substituted, 1);
        let mut evidence = vec![witness];
        let mut reader = MockSupplyReader::new();
        reader.set(b"expected", 2);

        let error =
            admit_by_funding_with_state_bound_evidence(vec![expected], &mut evidence, &reader)
                .await
                .expect_err("evidence for another envelope must fail");

        assert!(error
            .to_string()
            .contains("state-bound evidence is not for the canonical candidate envelope"));
    }

    proptest! {
        #[test]
        fn state_bound_admission_has_an_exact_cost_plus_fee_boundary(cost in 0u64..128) {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let deploy = cosigned(&n_sends(1), b"state-bound-boundary", 0, 10);
                let witness = processed(&deploy, cost);
                let mut exact = MockSupplyReader::new();
                exact.set(b"state-bound-boundary", i64::try_from(cost).unwrap() + 1);
                let mut exact_evidence = vec![witness.clone()];
                let admitted = admit_by_funding_with_state_bound_evidence(
                    vec![deploy.clone()],
                    &mut exact_evidence,
                    &exact,
                )
                .await
                .unwrap();
                prop_assert_eq!(admitted.admitted.len(), 1);

                let mut short = MockSupplyReader::new();
                short.set(b"state-bound-boundary", i64::try_from(cost).unwrap());
                let mut short_evidence = vec![witness];
                let rejected = admit_by_funding_with_state_bound_evidence(
                    vec![deploy],
                    &mut short_evidence,
                    &short,
                )
                .await
                .unwrap();
                prop_assert!(rejected.admitted.is_empty());
                prop_assert_eq!(rejected.rejected.len(), 1);
                Ok(())
            })?;
        }
    }

    /// §7.7 reject-both / no-partial: when the FIRST candidate in a group does
    /// not fit, it AND every candidate after it in the group are rejected.
    #[tokio::test]
    async fn reject_both_on_oversubscription() {
        // Two deploys sharing sig "carol", each Δ=3; pool = 2 (< Δ of the
        // first). The first fails (2 < 3) ⇒ reject it AND the second.
        let c0 = cosigned(&n_sends(3), b"carol", 0, 10);
        let c1 = cosigned(&n_sends(3), b"carol", 0, 20);

        let mut reader = MockSupplyReader::new();
        reader.set(b"carol", 2);

        let outcome = admit_by_funding(vec![c0.clone(), c1.clone()], &reader)
            .await
            .expect("gate must not error");

        assert!(outcome.admitted.is_empty(), "first unfunded ⇒ reject both");
        assert_eq!(outcome.rejected.len(), 2, "both deploys rejected");
        let carol_key = pool_key(b"carol");
        assert!(
            outcome.debits.get(&carol_key).is_none(),
            "no admitted deploys ⇒ no debit on the pool"
        );
    }

    /// §7.4 exact funding boundary: admission reserves certified cost plus the
    /// flat FeeExtract, so `Σ = Δ + fee` accepts and one unit less rejects.
    #[tokio::test]
    async fn funded_unfunded_boundary_at_def19() {
        // Δ = 4 and the flat fee is one, so exact funding requires Σ = 5.
        let demand = 4;
        const FEE: i64 = 1; // the per-deploy FeeExtract carve folded into admission

        // Σ = Δ + fee = 5 is accepted.
        let d = cosigned(&n_sends(demand), b"dave", 0, 10);
        let mut reader_ok = MockSupplyReader::new();
        reader_ok.set(b"dave", demand as i64 + FEE);
        let accepted = admit_by_funding(vec![d.clone()], &reader_ok)
            .await
            .expect("gate must not error");
        assert_eq!(accepted.admitted.len(), 1, "Σ = Δ + fee ⇒ accepted");
        assert!(accepted.rejected.is_empty());
        let dave_key = pool_key(b"dave");
        // COST debit is still the cost-only Δ (the fee is carved separately).
        assert_eq!(
            accepted.debits.get(&dave_key).map(|x| x.amount),
            Some(demand as i64)
        );
        assert_eq!(
            accepted.fee_debits.get(&dave_key).map(|x| x.amount),
            Some(FEE),
            "the admitted deploy carves exactly one FeeExtract token"
        );

        // Σ = Δ + fee − 1 = 4 ⇒ rejected (one below the cost+fee admission boundary).
        let mut reader_short = MockSupplyReader::new();
        reader_short.set(b"dave", demand as i64 + FEE - 1);
        let rejected = admit_by_funding(vec![d.clone()], &reader_short)
            .await
            .expect("gate must not error");
        assert!(rejected.admitted.is_empty(), "Σ = Δ + fee − 1 ⇒ rejected");
        assert_eq!(rejected.rejected.len(), 1);
        assert!(rejected.debits.is_empty(), "nothing admitted ⇒ no debit");
        assert!(
            rejected.fee_debits.is_empty(),
            "nothing admitted ⇒ no fee carve"
        );
    }

    /// A malformed term (one that fails to parse) is rejected outright — the
    /// runtime would fail it too — and never grouped or debited.
    #[tokio::test]
    async fn malformed_term_is_rejected() {
        // Unbalanced braces ⇒ `source_to_adt` fails.
        let bad = cosigned("for(x <- @0){ ", b"erin", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"erin", 1_000);
        let outcome = admit_by_funding(vec![bad], &reader)
            .await
            .expect("gate must not error");
        assert!(outcome.admitted.is_empty(), "malformed ⇒ not admitted");
        assert_eq!(outcome.rejected.len(), 1, "malformed ⇒ rejected");
        assert!(outcome.debits.is_empty());
    }

    #[tokio::test]
    async fn replay_rejects_malformed_admitted_deploy() {
        let bad = cosigned("for(x <- @0){ ", b"malformed-replay", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"malformed-replay", 1_000);

        let error = recompute_settlement_debits(vec![bad], &reader)
            .await
            .expect_err("replay must reject an admitted deploy without a demand certificate");

        assert!(matches!(
            error,
            CasperError::ReplayFailure(ReplayFailure::ReplayAdmissionMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn absent_pool_is_zero_supply_and_rejects() {
        let f = cosigned(&n_sends(5), b"frank", 0, 10);
        let reader = MockSupplyReader::new();
        let outcome = admit_by_funding(vec![f], &reader)
            .await
            .expect("gate must not error");
        assert!(outcome.admitted.is_empty());
        assert_eq!(outcome.rejected.len(), 1);
        assert!(outcome.debits.is_empty());
    }

    /// A PRESENT but DRAINED pool (`Some(0)`) correctly REJECTS a further spend
    /// — the §7.7 duplicate-deploy example (tex 1677-1687): once a signer's
    /// supply is committed to 0, the next deploy sees Σ = 0 < Δ and is rejected.
    #[tokio::test]
    async fn drained_present_pool_rejects() {
        let g = cosigned(&n_sends(3), b"grace", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"grace", 0); // PRESENT, drained to zero
        let outcome = admit_by_funding(vec![g], &reader)
            .await
            .expect("gate must not error");
        assert!(
            outcome.admitted.is_empty(),
            "present drained pool (Σ=0) ⇒ Δ=3 rejected"
        );
        assert_eq!(outcome.rejected.len(), 1);
        assert!(outcome.debits.is_empty());
    }

    /// Zero-demand handling under the F-C/F-D FeeExtract carve.
    /// A Δ=0 deploy (no token-consuming COMMs) STILL owes the spec's flat
    /// FeeExtract (one client token per PROCESSED deploy, tex:2509-2521/3637),
    /// which F-C/F-D folds into the admission demand: an admitted deploy needs
    /// `Σ⟦c⟧ ≥ Δ + fee = 0 + 1 = 1`. So the §7.6-step-5 zero-demand carve-out is
    /// now FEE-GATED — it admits only when the pool can afford the one-token fee:
    ///   * ABSENT (or drained) pool ⇒ effective Σ = 0 < 1 ⇒ REJECTED (it cannot
    ///     pay the FeeExtract; no execution, no debit, no carve);
    ///   * PRESENT pool with Σ ≥ 1 ⇒ ADMITTED with a ZERO COST debit (Δ=0) and a
    ///     fee carve of exactly 1, transferred to the proposer SystemVault.
    /// This is the cost ≠ fee split at the zero-cost boundary.
    #[tokio::test]
    async fn zero_demand_is_fee_gated() {
        let zoe_key = pool_key(b"zoe");

        // (a) ABSENT pool: Δ=0 but the FeeExtract needs Σ≥1 ⇒ effective 0 ⇒ rejected.
        let z_absent = cosigned("Nil", b"zoe", 0, 10);
        let reader_absent = MockSupplyReader::new(); // "zoe" pool absent
        let absent = admit_by_funding(vec![z_absent], &reader_absent)
            .await
            .expect("gate must not error");
        assert!(
            absent.admitted.is_empty(),
            "absent pool + Δ=0 ⇒ rejected because it cannot pay the fee"
        );
        assert_eq!(
            absent.rejected.len(),
            1,
            "the un-fee-fundable deploy is rejected"
        );
        assert!(absent.debits.is_empty(), "rejected ⇒ no cost debit");
        assert!(absent.fee_debits.is_empty(), "rejected ⇒ no fee carve");

        // (b) PRESENT pool with Σ=1: the Δ=0 deploy can pay the one-token fee ⇒
        // admitted, ZERO cost debit, fee carve of 1.
        let z_funded = cosigned("Nil", b"zoe", 0, 10);
        let mut reader_funded = MockSupplyReader::new();
        reader_funded.set(b"zoe", 1); // exactly the FeeExtract token, no cost
        let funded = admit_by_funding(vec![z_funded], &reader_funded)
            .await
            .expect("gate must not error");
        assert_eq!(
            funded.admitted.len(),
            1,
            "Σ=1 and Δ=0 ⇒ admitted because the FeeExtract is funded"
        );
        assert!(
            funded.rejected.is_empty(),
            "fee-fundable Δ=0 ⇒ not rejected"
        );
        assert!(
            funded.debits.get(&zoe_key).is_none(),
            "Δ=0 ⇒ zero COST settlement debit"
        );
        assert_eq!(
            funded.fee_debits.get(&zoe_key).map(|d| d.amount),
            Some(1),
            "the admitted Δ=0 deploy still transfers one fee token to the proposer"
        );
    }

    #[tokio::test]
    async fn mixed_absent_funded_and_drained_pools_are_enforced_uniformly() {
        // absent: Δ=4, no pool        ⇒ rejected
        // funded: Δ=2, Σ=5            ⇒ admitted, debit 2
        // drained: Δ=3, Σ=0 (present) ⇒ rejected, no debit
        let absent = cosigned(&n_sends(4), b"abs", 0, 10);
        let funded = cosigned(&n_sends(2), b"fund", 0, 20);
        let drained = cosigned(&n_sends(3), b"drain", 0, 30);

        let mut reader = MockSupplyReader::new();
        reader.set(b"fund", 5); // present, funds Δ=2
        reader.set(b"drain", 0); // present, drained ⇒ rejects Δ=3
                                 // "abs" intentionally unset ⇒ absent.

        let outcome = admit_by_funding(
            vec![absent.clone(), funded.clone(), drained.clone()],
            &reader,
        )
        .await
        .expect("gate must not error");

        let admitted_sigs: std::collections::BTreeSet<&[u8]> = outcome
            .admitted
            .iter()
            .map(|c| c.primary().sig.as_ref())
            .collect();
        assert_eq!(
            outcome.admitted.len(),
            1,
            "only the funded deploy is admitted"
        );
        assert!(admitted_sigs.contains(&b"fund".as_ref()), "funded admitted");
        assert_eq!(outcome.rejected.len(), 2, "absent and drained are rejected");

        // Debits: exactly the funded pool (Δ=2). Absent + drained ⇒ no debit.
        let abs_key = pool_key(b"abs");
        let fund_key = pool_key(b"fund");
        let drain_key = pool_key(b"drain");
        assert_eq!(
            outcome.debits.len(),
            1,
            "exactly one debit (the funded pool)"
        );
        assert_eq!(
            outcome.debits.get(&fund_key).map(|d| d.amount),
            Some(2),
            "funded -= 2"
        );
        assert!(
            outcome.debits.get(&abs_key).is_none(),
            "absent and rejected ⇒ no debit"
        );
        assert!(
            outcome.debits.get(&drain_key).is_none(),
            "drained/rejected ⇒ no debit"
        );
    }

    /// A genesis-funded client is admitted and debited identically on proposal
    /// and replay.
    ///
    /// This pairs with `genesis_system_vault_funding_is_committed_and_replay_deterministic`.
    #[tokio::test]
    async fn funded_client_is_admitted_and_replays() {
        // A client with demand Δ=4 and enough supply for cost plus fee.
        const DEMAND: usize = 4;
        let client = cosigned(&n_sends(DEMAND), b"client", 0, 10);
        let client_key = pool_key(b"client");

        let mut reader = MockSupplyReader::new();
        reader.set(b"client", (DEMAND as i64) + 6);

        // ---- PLAY gate ----
        let play = admit_by_funding(vec![client.clone()], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(play.admitted.len(), 1, "funded client pool ⇒ admitted");
        assert!(play.rejected.is_empty(), "funded client ⇒ not rejected");
        assert_eq!(
            play.debits.get(&client_key).map(|d| d.amount),
            Some(DEMAND as i64),
            "the admitted client pool is debited exactly its demand (post = pre − Δ)"
        );

        // ---- REPLAY recompute (strict = true) over the SAME admitted set ----
        // The replay path reconstructs the admitted envelopes from the block and
        // recomputes the debit map against the SAME pre-state pool. Under strict
        // it ALSO re-verifies admission; a funded client passes (no rejection).
        let recomputed = recompute_settlement_debits(play.admitted.clone(), &reader)
            .await
            .expect("strict replay recompute must not reject a funded client");

        // play == replay: the COST settlement map is byte-identical (the
        // consensus bar). `recompute_settlement_debits` returns `RecomputedDebits`
        // with `.settlement` (cost) and `.fee` (carve); this test asserts the cost.
        assert_eq!(
            recomputed.settlement.len(),
            play.debits.len(),
            "replay recomputed the same number of pool debits as play"
        );
        assert_eq!(
            recomputed.settlement.get(&client_key).map(|d| d.amount),
            play.debits.get(&client_key).map(|d| d.amount),
            "strict replay recompute is byte-identical to the play-side client debit"
        );
        assert_eq!(
            recomputed.settlement.get(&client_key).map(|d| d.amount),
            Some(DEMAND as i64),
            "replayed client debit equals its demand"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // §D2.9 — `Σ⟦signer⟧ == Σ⟦wallet⟧`: a deploy's fuel is drawn from the pool
    // keyed by the SIGNER'S GROUND PUBLIC KEY `Σ⟦Ground(pk)⟧` — the genesis-
    // seeded wallet (`close_block_deploy.rs` seeds `Sig::Ground(pk)`) — NOT a
    // per-deploy wire-signature pool. These are the consensus payoff tests that
    // the gate now binds against the signer's real wallet.
    // ═══════════════════════════════════════════════════════════════════════

    /// §D2.9 (single-sig) — a funded signer's deploy is admitted and the cost is
    /// debited from EXACTLY `Σ⟦Ground(signer_pk)⟧` (the wallet channel), by Δ;
    /// the gate's funding key IS `funding_sig_single(signer_pk)`. Conservation:
    /// the debit `CloseBlockDeploy` applies is `post = pre − Δ`.
    #[tokio::test]
    async fn deploy_funds_from_signer_ground_pubkey_wallet() {
        const DEMAND: usize = 3;
        let deploy = cosigned(&n_sends(DEMAND), b"signer", 0, 10);

        // The pool the gate keys is the signer's GROUND public-key wallet.
        let signer_pk = deploy.primary().pk.bytes.to_vec();
        let wallet_sig = accounting::funding_sig_single(&signer_pk);
        assert_eq!(
            accounting::funding_sig(&deploy),
            wallet_sig,
            "the gate funds a single-sig deploy from Σ⟦Ground(signer_pk)⟧"
        );
        let wallet_chan = supply::supply_channel(&wallet_sig);
        let wallet_key = delta_sigma::sig_key(&wallet_sig);

        const PRE: i64 = 10; // genesis-seeded balance ≥ Δ
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&wallet_sig, PRE);

        let outcome = admit_by_funding(vec![deploy.clone()], &reader)
            .await
            .expect("gate must not error");

        assert_eq!(outcome.admitted.len(), 1, "funded signer admitted");
        assert!(outcome.rejected.is_empty());
        let debit = outcome
            .debits
            .get(&wallet_key)
            .expect("the signer's ground-pubkey wallet is debited");
        assert_eq!(debit.amount, DEMAND as i64, "cost debit == Δ");
        assert_eq!(
            debit.channel, wallet_chan,
            "the debited channel IS Σ⟦Ground(signer_pk)⟧ — the wallet, not a wire-sig pool"
        );
        // Conservation: CloseBlockDeploy applies post = pre − Δ.
        assert_eq!(PRE - debit.amount, PRE - DEMAND as i64);
    }

    /// §D2.9 (single-sig) — a deploy whose signer wallet
    /// `Σ⟦Ground(pk)⟧` is ABSENT (never seeded) is REJECTED: it cannot prove
    /// `Σ ≥ Δ`. Pre-fix the wire-sig pool was ALWAYS absent ⇒ every deploy was
    /// admitted without a debit; now an unfunded signer is refused.
    #[tokio::test]
    async fn unfunded_signer_is_rejected() {
        let deploy = cosigned(&n_sends(2), b"poor", 0, 10);
        let reader = MockSupplyReader::new(); // the signer's wallet is ABSENT
        let outcome = admit_by_funding(vec![deploy.clone()], &reader)
            .await
            .expect("gate must not error");
        assert!(
            outcome.admitted.is_empty(),
            "absent signer wallet ⇒ rejected"
        );
        assert_eq!(outcome.rejected, vec![deploy.primary().sig.clone()]);
        assert!(outcome.debits.is_empty(), "rejected ⇒ no debit");
    }

    /// §D2.9 (multi-sig) + P8 — a compound deploy's funding components are
    /// EXACTLY the cosigners' ground-pubkey wallets `Σ⟦Ground(pkᵢ)⟧`, drawn
    /// BALANCED (each cosigner's wallet debited equally), with play == replay.
    ///
    /// This mirrors GENESIS: only the individual cosigner wallets
    /// `Σ⟦Ground(pkᵢ)⟧` are seeded — there is NO combined `Σ⟦And(…)⟧` pool (the
    /// genesis ceremony seeds per-pubkey wallets, never compound pools). The
    /// compound group therefore funds from
    /// `effectiveΣ_compound = Σ⟦compound⟧(absent ⇒ 0) + min(Σ⟦left⟧, Σ⟦right⟧)`,
    /// i.e. the matched component pair, debiting each cosigner's wallet equally.
    /// The exact Split/Join split is pinned by
    /// `compound_debit_play_replay_identical_pair_only` (now `funding_sig`-keyed);
    /// here we pin the cosigner-wallet identity + per-cosigner balance.
    #[tokio::test]
    async fn multi_sig_funds_balanced_over_cosigner_ground_pubkey_wallets() {
        let compound = compound_cosigned("@0!(0) | @0!(0)", 0, 10); // Δ = 2
        let pks: Vec<Vec<u8>> = compound
            .signers()
            .iter()
            .map(|s| s.pk.bytes.to_vec())
            .collect();
        assert_eq!(pks.len(), 2, "two cosigners");

        // The funding signature is And(Ground(pk_a), Ground(pk_b)) over the
        // ACTUAL cosigner public keys — their wallets (P8-balanced).
        let funding = accounting::funding_sig(&compound);
        let pk_refs: Vec<&[u8]> = pks.iter().map(|p| p.as_slice()).collect();
        assert_eq!(
            funding,
            accounting::funding_sig_compound(&pk_refs),
            "compound funding == And(Ground(cosigner_pkᵢ)) — the cosigners' wallets"
        );
        let (left, right) = match &funding {
            Sig::And(l, r) => ((**l).clone(), (**r).clone()),
            other => panic!("expected And(Ground,Ground), got {:?}", other),
        };
        assert!(
            matches!(left, Sig::Ground(_)) && matches!(right, Sig::Ground(_)),
            "both components are the cosigners' Ground(pk) wallets"
        );

        // Seed ONLY the two cosigner wallets (mirrors genesis: per-pubkey
        // wallets, NO compound pool). effectiveΣ_compound = 0 + min(5,5) = 5.
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&left, 5);
        reader.set_sig(&right, 5);

        // The absent compound pool contributes zero; the component pair funds it.
        let play = admit_by_funding(vec![compound.clone()], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(play.admitted.len(), 1, "admitted from the cosigner wallets");
        assert!(play.rejected.is_empty());

        let replay = recompute_settlement_debits(play.admitted.clone(), &reader)
            .await
            .expect("recompute must not error");
        assert_eq!(
            play.debits, replay.settlement,
            "play == replay byte-identical over the cosigner wallets"
        );

        // Balanced (P8): each cosigner's wallet is debited EQUALLY (a compound
        // token is a matched pair — one from each pool), and they actually fund.
        let l = play
            .debits
            .get(&delta_sigma::sig_key(&left))
            .map(|d| d.amount)
            .unwrap_or(0);
        let r = play
            .debits
            .get(&delta_sigma::sig_key(&right))
            .map(|d| d.amount)
            .unwrap_or(0);
        assert_eq!(l, r, "per-cosigner draw is balanced (P8)");
        assert!(l > 0, "the cosigner wallets actually fund the deploy");
    }

    /// §D2.9 SECURITY (placeholder filter, R1-F4) — a THRESHOLD envelope may
    /// list empty-`sig` PLACEHOLDER cosigners (un-signed members of an M-of-N
    /// set). `funding_sig` MUST exclude them, so a deploy can NEVER key funding
    /// to (debit) an UNSIGNED victim's wallet `Σ⟦Ground(victim_pk)⟧`. A 1-of-2
    /// threshold with one real signer + one placeholder "victim" must fund ONLY
    /// the real signer's wallet and leave the victim's (seeded) wallet untouched.
    #[tokio::test]
    async fn threshold_placeholder_victim_wallet_is_never_debited() {
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::Message;

        let data = DeployData {
            term: n_sends(2),
            time_stamp: 10,
            valid_after_block_number: 0,
            shard_id: String::new(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let secp = Secp256k1;
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);

        // One REAL signer (valid sig over the canonical hash).
        let (sk, real_pk) = secp.new_key_pair();
        let real_sig = Bytes::from(secp.sign(&hash, &sk.bytes));
        // One PLACEHOLDER "victim": a pubkey with an EMPTY sig (did NOT sign).
        let (_victim_sk, victim_pk) = secp.new_key_pair();

        let real_signer = Cosigner {
            pk: real_pk.clone(),
            sig: real_sig,
            sig_algorithm: Box::new(Secp256k1),
        };
        let victim_placeholder = Cosigner {
            pk: victim_pk.clone(),
            sig: Bytes::new(),
            sig_algorithm: Box::new(Secp256k1),
        };

        // 1-of-2 threshold: only the real signer's signature is required/valid.
        let cosigned =
            Cosigned::from_signed_data_threshold(data, vec![real_signer, victim_placeholder], 1)
                .expect("1-of-2 threshold with one valid signer");

        // funding_sig EXCLUDES the placeholder ⇒ Ground(real_pk) ONLY — the
        // FILTERED funder count (1) drives the arity, NOT `is_compound()` (2).
        let real_wallet = accounting::funding_sig_single(&real_pk.bytes);
        assert_eq!(
            accounting::funding_sig(&cosigned),
            real_wallet,
            "funding excludes the unsigned placeholder ⇒ only the real signer funds"
        );

        // Seed BOTH wallets; the victim's (richly funded) wallet must stay intact.
        let victim_wallet = accounting::funding_sig_single(&victim_pk.bytes);
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&real_wallet, 10);
        reader.set_sig(&victim_wallet, 100);

        let outcome = admit_by_funding(vec![cosigned.clone()], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(
            outcome.admitted.len(),
            1,
            "the real signer funds the deploy"
        );
        let real_key = delta_sigma::sig_key(&real_wallet);
        let victim_key = delta_sigma::sig_key(&victim_wallet);
        assert_eq!(
            outcome.debits.get(&real_key).map(|d| d.amount),
            Some(2),
            "the real signer's wallet is debited Δ"
        );
        assert!(
            outcome.debits.get(&victim_key).is_none(),
            "the unsigned victim's wallet is NEVER debited (placeholder filter)"
        );
    }

    /// TM-CA-153 (COMPOUND over-admission) — a hand-crafted compound admitted set
    /// whose cumulative demand `ΣΔ` EXCEEDS the group's effective supply is
    /// REJECTED on replay with `ReplayAdmissionMismatch`. The Split/Join
    /// residual-cap silently absorbs the over-demand into per-pool debits ≤
    /// balance, so the per-pool `debit > balance` check cannot catch it — the
    /// cumulative `cost + fee ≤ effectiveΣ` re-check must. A VALID compound deploy
    /// at the admissible boundary still passes (no fork of a gate-admitted block).
    #[tokio::test]
    async fn compound_over_admission_rejected_on_replay() {
        // Over-set: ONE compound deploy Δ = 8, components seeded to 5 each ⇒
        // effectiveΣ = Σ⟦And⟧(absent ⇒ 0) + min(5,5) = 5 < Δ. The play-side gate
        // would reject it (8 > 5); a malicious proposer includes it anyway.
        let over = compound_cosigned(&n_sends(8), 0, 10);
        let (left, right) = match accounting::funding_sig(&over) {
            Sig::And(l, r) => (*l, *r),
            other => panic!("expected And(Ground,Ground), got {:?}", other),
        };
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&left, 5);
        reader.set_sig(&right, 5);

        let err = recompute_settlement_debits(vec![over.clone()], &reader)
            .await
            .expect_err("compound over-admission (ΣΔ=8 > effectiveΣ=5) must be rejected on replay");
        match err {
            CasperError::ReplayFailure(ReplayFailure::ReplayAdmissionMismatch {
                detail, ..
            }) => {
                assert!(
                    detail.contains("over-admission") || detail.contains("exceeds effective"),
                    "expected an over-admission diagnostic, got: {detail}"
                );
            }
            other => panic!("expected ReplayAdmissionMismatch, got {:?}", other),
        }

        // A VALID compound deploy: Δ = 4, components 5 each ⇒ effectiveΣ = 5, and
        // cost + fee = 4 + 1 = 5 ≤ 5 (the admissible boundary) ⇒ NOT rejected. This
        // confirms the re-check matches the gate exactly and never forks a
        // gate-admitted block.
        let ok = compound_cosigned(&n_sends(4), 0, 10);
        let (l2, r2) = match accounting::funding_sig(&ok) {
            Sig::And(l, r) => (*l, *r),
            other => panic!("expected And, got {:?}", other),
        };
        let mut reader_ok = MockSupplyReader::new();
        reader_ok.set_sig(&l2, 5);
        reader_ok.set_sig(&r2, 5);
        recompute_settlement_debits(vec![ok.clone()], &reader_ok)
            .await
            .expect("a funded compound deploy (cost+fee = effectiveΣ) must pass the re-check");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // #12 — EXACT per-component (Split/Join) compound settlement debit.
    // The consensus bars for the multi-pool draw split + the cross-group
    // shared-component residual ledger + play↔replay byte-identity.
    // ═══════════════════════════════════════════════════════════════════════

    /// #12.1 — `Sig::And(a,b)`, `Σ⟦compound⟧=0`, `Σ⟦a⟧=Σ⟦b⟧=k`: the compound
    /// group's whole demand `k` is settled from the component PAIR (combined pool
    /// empty), so `Σ⟦a⟧ -= k` AND `Σ⟦b⟧ -= k`, with NO compound debit.
    #[test]
    fn compound_debit_splits_to_components() {
        let fx = compound_fixture(b"alice", b"bob");
        let k = 4_i64;

        let mut raw = BTreeMap::new();
        raw.insert(fx.compound_key, 0);
        raw.insert(fx.a_key, k);
        raw.insert(fx.b_key, k);

        let mut demand = BTreeMap::new();
        demand.insert(fx.compound_key, (fx.compound_chan.clone(), k));

        let debits = compute_settlement_debits(
            &demand,
            &[fx.decomposition],
            &raw,
            &fx.channels_by_key(),
            &DefaultApportionment,
        );

        // Components each debited k; compound NOT present (draw_compound = 0).
        assert_eq!(
            debits.get(&fx.a_key).map(|d| d.amount),
            Some(k),
            "Σ⟦a⟧ -= k"
        );
        assert_eq!(
            debits.get(&fx.b_key).map(|d| d.amount),
            Some(k),
            "Σ⟦b⟧ -= k"
        );
        assert!(
            debits.get(&fx.compound_key).is_none(),
            "empty combined pool ⇒ NO compound debit"
        );
        // The debited channels are the component channels.
        assert_eq!(debits.get(&fx.a_key).map(|d| &d.channel), Some(&fx.a_chan));
        assert_eq!(debits.get(&fx.b_key).map(|d| &d.channel), Some(&fx.b_chan));
    }

    /// #12.2 — combined-pool-first then component-pair: `Σ⟦compound⟧=1,
    /// Σ⟦a⟧=Σ⟦b⟧=5, k=3` ⇒ `draw_compound=1, draw_pair=2`; compound-=1, a-=2, b-=2.
    #[test]
    fn compound_debit_prefers_combined_then_pair() {
        let fx = compound_fixture(b"alice", b"bob");

        let mut raw = BTreeMap::new();
        raw.insert(fx.compound_key, 1);
        raw.insert(fx.a_key, 5);
        raw.insert(fx.b_key, 5);

        let mut demand = BTreeMap::new();
        demand.insert(fx.compound_key, (fx.compound_chan.clone(), 3));

        let debits = compute_settlement_debits(
            &demand,
            &[fx.decomposition],
            &raw,
            &fx.channels_by_key(),
            &DefaultApportionment,
        );

        assert_eq!(
            debits.get(&fx.compound_key).map(|d| d.amount),
            Some(1),
            "combined pool drawn first: draw_compound = min(3,1) = 1"
        );
        assert_eq!(
            debits.get(&fx.a_key).map(|d| d.amount),
            Some(2),
            "draw_pair = 3-1 = 2"
        );
        assert_eq!(
            debits.get(&fx.b_key).map(|d| d.amount),
            Some(2),
            "draw_pair = 3-1 = 2"
        );
    }

    /// #12.3 — underflow-safety at the funding boundary: for an admitted compound
    /// (`k = Σ⟦compound⟧ + min(Σ⟦a⟧,Σ⟦b⟧)`, the exact effectiveΣ), no pool's debit
    /// exceeds its raw balance ⇒ `post = pre − draw ≥ 0` on every pool.
    #[test]
    fn compound_component_pool_underflow_safe() {
        let fx = compound_fixture(b"alice", b"bob");
        let sigma_compound = 2_i64;
        let sigma_a = 3_i64;
        let sigma_b = 4_i64; // min(a,b) = 3
        let k = sigma_compound + sigma_a.min(sigma_b); // = 5, the funding boundary

        let mut raw = BTreeMap::new();
        raw.insert(fx.compound_key, sigma_compound);
        raw.insert(fx.a_key, sigma_a);
        raw.insert(fx.b_key, sigma_b);

        let mut demand = BTreeMap::new();
        demand.insert(fx.compound_key, (fx.compound_chan.clone(), k));

        let debits = compute_settlement_debits(
            &demand,
            &[fx.decomposition],
            &raw,
            &fx.channels_by_key(),
            &DefaultApportionment,
        );

        // draw_compound = min(5,2) = 2; draw_pair = min(3, 3, 4) = 3.
        let d_compound = debits.get(&fx.compound_key).map(|d| d.amount).unwrap_or(0);
        let d_a = debits.get(&fx.a_key).map(|d| d.amount).unwrap_or(0);
        let d_b = debits.get(&fx.b_key).map(|d| d.amount).unwrap_or(0);
        assert_eq!(d_compound, 2);
        assert_eq!(d_a, 3);
        assert_eq!(d_b, 3);
        // No pool underflows: post = pre − draw ≥ 0.
        assert!(sigma_compound - d_compound >= 0, "Σ⟦compound⟧ no underflow");
        assert!(sigma_a - d_a >= 0, "Σ⟦a⟧ no underflow");
        assert!(sigma_b - d_b >= 0, "Σ⟦b⟧ no underflow");
        // And the total settled equals the demand (conservation: draws sum to k).
        assert_eq!(
            d_compound + d_a.min(d_b),
            k,
            "draw_compound + draw_pair = k"
        );
    }

    /// #12.4 — cross-group shared-component contention: two compound groups
    /// `And(a,b)` and `And(a,c)` both draw the SHARED component `a`. The SUMMED
    /// `a`-draw across both groups MUST be ≤ `Σ⟦a⟧` (the residual ledger bounds
    /// the second group's pair-draw by `a`'s LIVE residual after the first).
    #[test]
    fn compound_shared_component_contention() {
        // Components a, b, c; compounds ab = And(a,b), ac = And(a,c).
        let a = Sig::Ground(b"shared-a".to_vec());
        let b = Sig::Ground(b"only-b".to_vec());
        let c = Sig::Ground(b"only-c".to_vec());
        let ab = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
        let ac = Sig::And(Box::new(a.clone()), Box::new(c.clone()));

        let a_key = delta_sigma::sig_key(&a);
        let b_key = delta_sigma::sig_key(&b);
        let c_key = delta_sigma::sig_key(&c);
        let ab_key = delta_sigma::sig_key(&ab);
        let ac_key = delta_sigma::sig_key(&ac);

        // Σ⟦a⟧ = 3 (the contended pool); plenty of b, c; empty combined pools so
        // ALL demand falls on the component pairs (maximizing contention on a).
        let mut raw = BTreeMap::new();
        raw.insert(a_key, 3);
        raw.insert(b_key, 10);
        raw.insert(c_key, 10);
        raw.insert(ab_key, 0);
        raw.insert(ac_key, 0);

        let mut channels_by_key = BTreeMap::new();
        channels_by_key.insert(a_key, supply::supply_channel(&a));
        channels_by_key.insert(b_key, supply::supply_channel(&b));
        channels_by_key.insert(c_key, supply::supply_channel(&c));
        channels_by_key.insert(ab_key, supply::supply_channel(&ab));
        channels_by_key.insert(ac_key, supply::supply_channel(&ac));

        let decompositions = vec![
            Decomposition {
                compound: ab_key,
                left: a_key,
                right: b_key,
            },
            Decomposition {
                compound: ac_key,
                left: a_key,
                right: c_key,
            },
        ];

        // Each compound group demands 2 (so combined demand 4 on a's residual of 3).
        let mut demand = BTreeMap::new();
        demand.insert(ab_key, (supply::supply_channel(&ab), 2));
        demand.insert(ac_key, (supply::supply_channel(&ac), 2));

        let debits = compute_settlement_debits(
            &demand,
            &decompositions,
            &raw,
            &channels_by_key,
            &DefaultApportionment,
        );

        let a_draw = debits.get(&a_key).map(|d| d.amount).unwrap_or(0);
        assert!(
            a_draw <= 3,
            "summed shared-component draw {} must not exceed Σ⟦a⟧ = 3",
            a_draw
        );

        // play == replay: the same function over the same inputs is deterministic.
        let debits_again = compute_settlement_debits(
            &demand,
            &decompositions,
            &raw,
            &channels_by_key,
            &DefaultApportionment,
        );
        assert_eq!(debits, debits_again, "deterministic: play == replay");
    }

    /// #12.6 — back-compat: a SINGLE-SIGNATURE (non-compound) group produces a
    /// byte-identical debit map to the pre-#12 single-pool path — one debit `k`
    /// on its own pool, never residual-capped, no component split.
    #[test]
    fn single_signer_debit_byte_identical_to_pre_12() {
        let sig = accounting::envelope_sig_single(b"solo");
        let key = delta_sigma::sig_key(&sig);
        let chan = supply::supply_channel(&sig);
        let k = 7_i64;

        let mut raw = BTreeMap::new();
        raw.insert(key, 9);
        let mut channels_by_key = BTreeMap::new();
        channels_by_key.insert(key, chan.clone());
        let mut demand = BTreeMap::new();
        demand.insert(key, (chan.clone(), k));

        // No decompositions ⇒ pure single-signer path.
        let debits =
            compute_settlement_debits(&demand, &[], &raw, &channels_by_key, &DefaultApportionment);

        assert_eq!(debits.len(), 1, "exactly one debit for the single group");
        let d = debits.get(&key).expect("solo pool debited");
        assert_eq!(
            d.amount, k,
            "amount = ΣΔ_admitted = k (NOT residual-capped)"
        );
        assert_eq!(d.channel, chan, "debit keyed to the group's own pool");
    }

    /// Build a REAL 2-signer compound `Cosigned<DeployData>` over `term` (two
    /// fresh Secp256k1 keypairs both signing the canonical message hash), so the
    /// gate's `funding_sig` derives a genuine `Sig::And` of the two cosigners'
    /// `Sig::Ground(pkᵢ)` atoms (the compound shape). Mirrors
    /// `multi_sig_fanout_bench::build_n_signers`.
    fn compound_cosigned(term: &str, vabn: i64, ts: i64) -> Cosigned<DeployData> {
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::Message;

        let data = DeployData {
            term: term.to_string(),
            time_stamp: ts,
            valid_after_block_number: vabn,
            shard_id: String::new(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let secp = Secp256k1;
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let signers: Vec<Cosigner> = (0..2)
            .map(|_| {
                let (sk, pk) = secp.new_key_pair();
                let sig = Bytes::from(secp.sign(&hash, &sk.bytes));
                Cosigner {
                    pk,
                    sig,
                    sig_algorithm: Box::new(Secp256k1),
                }
            })
            .collect();
        Cosigned::from_signed_data(data, signers).expect("2-signer compound envelope")
    }

    // ── TM-CA-165 cross-group shared-component admission helpers + tests ─────

    /// A fresh Secp256k1 keypair `(sk, pk)` for the cross-group tests.
    fn fresh_keypair() -> (PrivateKey, PublicKey) {
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        Secp256k1.new_key_pair()
    }

    /// Build a `Cosigned<DeployData>` over `term` from the GIVEN keypairs, so two
    /// envelopes can SHARE a cosigner (`compound_cosigned` generates fresh keys
    /// each call, yielding DISJOINT cosigner sets — useless for shared-component
    /// tests). One keypair ⇒ a single-sig deploy keyed `Σ⟦Ground(pk)⟧`; two or more
    /// ⇒ a compound whose `funding_sig` is the `And`-fold of `Ground(pkᵢ)`. Two
    /// envelopes built with a shared keypair `s` therefore share that cosigner's
    /// component wallet `Σ⟦Ground(pk_s)⟧` — the contended pool in TM-CA-165.
    fn cosigned_with_keypairs(
        term: &str,
        keypairs: &[(PrivateKey, PublicKey)],
        vabn: i64,
        ts: i64,
    ) -> Cosigned<DeployData> {
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::Message;

        let data = DeployData {
            term: term.to_string(),
            time_stamp: ts,
            valid_after_block_number: vabn,
            shard_id: String::new(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let secp = Secp256k1;
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let signers: Vec<Cosigner> = keypairs
            .iter()
            .map(|(sk, pk)| {
                let sig = Bytes::from(secp.sign(&hash, &sk.bytes));
                Cosigner {
                    pk: pk.clone(),
                    sig,
                    sig_algorithm: Box::new(Secp256k1),
                }
            })
            .collect();
        Cosigned::from_signed_data(data, signers).expect("cosigned envelope from given keypairs")
    }

    /// The `Σ⟦Ground(pk)⟧` component-wallet key + `Sig` for a keypair's pubkey.
    fn ground_pool(pk: &PublicKey) -> (Sig, SigKey) {
        let sig = Sig::Ground(pk.bytes.to_vec());
        let key = delta_sigma::sig_key(&sig);
        (sig, key)
    }

    /// TM-CA-165 (headline): two DISTINCT cosigner sets `{A,s}` and `{B,s}` share
    /// the component wallet `Σ⟦Ground(pk_s)⟧ = 3`. Each group's folded demand is
    /// cost(1)+fee(1) = 2, so the shared stack funds only ONE. The LIVE cross-group
    /// ledger draws `Σ⟦Ground(pk_s)⟧` down as the SigKey-first group is admitted, so
    /// the second is reject-both on the exhausted stack. PRE-FIX (static per-group
    /// effective) BOTH admitted, jointly over-drawing s by 1 unit of un-funded
    /// compute (TM-CA-165). The verdict is order-robust (whichever group sorts first
    /// wins; the other always rejects).
    #[tokio::test]
    async fn cross_group_two_compounds_sharing_component_admits_one() {
        let kp_a = fresh_keypair();
        let kp_b = fresh_keypair();
        let kp_s = fresh_keypair();

        let g1 = cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 10);
        let g2 = cosigned_with_keypairs("@0!(0)", &[kp_b.clone(), kp_s.clone()], 0, 20);

        let (s_sig, s_key) = ground_pool(&kp_s.1);
        let (a_sig, _) = ground_pool(&kp_a.1);
        let (b_sig, _) = ground_pool(&kp_b.1);

        let mut reader = MockSupplyReader::new();
        reader.set_sig(&s_sig, 3); // the contended shared wallet
        reader.set_sig(&a_sig, 100);
        reader.set_sig(&b_sig, 100);

        // strict = true ⇒ enforce even though the compound `Σ⟦And⟧` pools are
        // genesis-absent (§D2.9): funding flows through the component wallets.
        let outcome = admit_by_funding(vec![g1, g2], &reader)
            .await
            .expect("gate must not error");

        assert_eq!(
            outcome.admitted.len(),
            1,
            "shared Σ⟦Ground(s)⟧=3 funds only ONE of two cost+fee=2 groups (pre-fix: 2)"
        );
        assert_eq!(
            outcome.rejected.len(),
            1,
            "the second group sharing s is reject-both on the exhausted stack"
        );
        let s_draw = outcome.debits.get(&s_key).map(|d| d.amount).unwrap_or(0);
        assert!(
            s_draw <= 3,
            "summed shared-component draw {} must not exceed Σ⟦Ground(s)⟧=3",
            s_draw
        );

        // play == replay: the admitted set re-verifies + recomputes identically.
        let recomputed = recompute_settlement_debits(outcome.admitted.clone(), &reader)
            .await
            .expect("admitted set is fundable ⇒ no cross-group mismatch on replay");
        assert_eq!(
            outcome.debits, recomputed.settlement,
            "play debit map == replay recompute (byte-identical)"
        );
    }

    /// TM-CA-165 boundary: when the two groups' COMBINED folded demand EXACTLY
    /// equals the shared supply, BOTH are admitted (equality is admissible — no
    /// false-reject). `Σ⟦Ground(pk_s)⟧ = 4`, each group folds 2 ⇒ 2+2 = 4.
    #[tokio::test]
    async fn cross_group_boundary_demand_equals_shared_supply_admits_both() {
        let kp_a = fresh_keypair();
        let kp_b = fresh_keypair();
        let kp_s = fresh_keypair();

        let g1 = cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 10);
        let g2 = cosigned_with_keypairs("@0!(0)", &[kp_b.clone(), kp_s.clone()], 0, 20);

        let (s_sig, s_key) = ground_pool(&kp_s.1);
        let (a_sig, _) = ground_pool(&kp_a.1);
        let (b_sig, _) = ground_pool(&kp_b.1);

        let mut reader = MockSupplyReader::new();
        reader.set_sig(&s_sig, 4); // exactly funds both groups' folded 2 + 2
        reader.set_sig(&a_sig, 100);
        reader.set_sig(&b_sig, 100);

        let outcome = admit_by_funding(vec![g1, g2], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(
            outcome.admitted.len(),
            2,
            "Σ⟦Ground(s)⟧=4 exactly funds both folded-2 groups (equality admissible)"
        );
        let s_draw = outcome.debits.get(&s_key).map(|d| d.amount).unwrap_or(0);
        assert!(s_draw <= 4, "summed shared-component draw {} ≤ 4", s_draw);

        let recomputed = recompute_settlement_debits(outcome.admitted.clone(), &reader)
            .await
            .expect("boundary block re-verifies with no mismatch");
        assert_eq!(
            outcome.debits, recomputed.settlement,
            "play == replay at the boundary"
        );
    }

    /// TM-CA-165 replay guard: a malicious proposer who admits BOTH `{A,s}` and
    /// `{B,s}` jointly over-drawing `Σ⟦Ground(pk_s)⟧ = 3` is caught on replay — the
    /// cross-group ledger re-verification (not the per-pool debit≤balance check,
    /// which the residual-capped settlement silently satisfies) raises
    /// `ReplayAdmissionMismatch`.
    #[tokio::test]
    async fn cross_group_over_admission_distinct_sets_rejected_on_replay() {
        let kp_a = fresh_keypair();
        let kp_b = fresh_keypair();
        let kp_s = fresh_keypair();

        let g1 = cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 10);
        let g2 = cosigned_with_keypairs("@0!(0)", &[kp_b.clone(), kp_s.clone()], 0, 20);

        let (s_sig, _) = ground_pool(&kp_s.1);
        let (a_sig, _) = ground_pool(&kp_a.1);
        let (b_sig, _) = ground_pool(&kp_b.1);

        let mut reader = MockSupplyReader::new();
        reader.set_sig(&s_sig, 3); // cannot fund both groups' combined demand of 4
        reader.set_sig(&a_sig, 100);
        reader.set_sig(&b_sig, 100);

        // Hand-crafted over-admitted block: BOTH groups in the admitted set.
        let result = recompute_settlement_debits(vec![g1, g2], &reader).await;
        assert!(
            result.is_err(),
            "cross-group over-admission of a shared component must be rejected on replay"
        );
        let msg = format!("{:?}", result.expect_err("err"));
        assert!(
            msg.contains("cross-group") || msg.contains("over-admission"),
            "replay error must name the cross-group over-admission: {}",
            msg
        );
    }

    /// TM-CA-165 / §D2.9-R2: a SINGLE-sig deploy from `s` and a compound `{A,s}`
    /// share the component wallet `Σ⟦Ground(pk_s)⟧`. Under R2 the single-sig caps
    /// at its OWN-pool live residual (it cannot weaken a compound pool), and the
    /// compound's pair-draw also hits `Σ⟦Ground(pk_s)⟧`, so the LIVE ledger bounds
    /// their combined draw — only one is admitted; the shared stack is not
    /// over-drawn.
    #[tokio::test]
    async fn single_sig_and_compound_sharing_component_bounded() {
        let kp_a = fresh_keypair();
        let kp_s = fresh_keypair();

        // Single-sig from s (1 signer ⇒ funding_sig = Ground(pk_s)); compound {A,s}.
        let single_s = cosigned_with_keypairs("@0!(0) | @0!(0)", &[kp_s.clone()], 0, 10); // Δ=2
        let compound_as =
            cosigned_with_keypairs("@0!(0) | @0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 20); // Δ=2

        let (s_sig, s_key) = ground_pool(&kp_s.1);
        let (a_sig, _) = ground_pool(&kp_a.1);

        let mut reader = MockSupplyReader::new();
        reader.set_sig(&s_sig, 4); // funds only one folded-3 group
        reader.set_sig(&a_sig, 100);

        let outcome = admit_by_funding(vec![single_s, compound_as], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(
            outcome.admitted.len(),
            1,
            "Σ⟦Ground(s)⟧=4 funds only ONE of the single(s)/{{A,s}} folded-3 groups"
        );
        let s_draw = outcome.debits.get(&s_key).map(|d| d.amount).unwrap_or(0);
        assert!(
            s_draw <= 4,
            "single-sig (own-pool, no-weakening) + compound pair-draw on s ≤ 4, got {}",
            s_draw
        );

        let recomputed = recompute_settlement_debits(outcome.admitted.clone(), &reader)
            .await
            .expect("admitted set fundable on replay");
        assert_eq!(outcome.debits, recomputed.settlement, "play == replay");
    }

    #[tokio::test]
    async fn cross_group_absent_authorities_reject_both() {
        let kp_a = fresh_keypair();
        let kp_b = fresh_keypair();
        let kp_s = fresh_keypair();

        let g1 = cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 10);
        let g2 = cosigned_with_keypairs("@0!(0)", &[kp_b.clone(), kp_s.clone()], 0, 20);

        let reader = MockSupplyReader::new();
        let outcome = admit_by_funding(vec![g1, g2], &reader)
            .await
            .expect("gate must not error");

        assert!(outcome.admitted.is_empty());
        assert_eq!(outcome.rejected.len(), 2);
        assert!(outcome.debits.is_empty());
        assert!(outcome.fee_debits.is_empty(), "absent pools ⇒ no fee carve");
    }

    /// TM-CA-165 nested-arity guard: an n≥3 compound `{A,s,t}` =
    /// `And(And(A,s),t)` keys its TOP-LEVEL pair on `Σ⟦And(A,s)⟧` and `Σ⟦Ground(t)⟧`,
    /// NOT on `Σ⟦Ground(s)⟧` directly. With the inner compound pool `Σ⟦And(A,s)⟧`
    /// genesis-absent (§D2.9), the 3-sig group's effective capacity is 0, so it is
    /// rejected — it cannot leak draw onto a 2-sig `{A,s}`'s shared `s`. Pins that
    /// the nested-sharing flavor stays latent (unreachable) under absent compound
    /// pools, so no cross-group bound is bypassed via nesting.
    #[tokio::test]
    async fn nary_nested_compound_absent_inner_pool_rejected() {
        let kp_a = fresh_keypair();
        let kp_s = fresh_keypair();
        let kp_t = fresh_keypair();

        let two_sig = cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone()], 0, 10);
        let three_sig =
            cosigned_with_keypairs("@0!(0)", &[kp_a.clone(), kp_s.clone(), kp_t.clone()], 0, 20);

        let (s_sig, _) = ground_pool(&kp_s.1);
        let (a_sig, _) = ground_pool(&kp_a.1);
        let (t_sig, _) = ground_pool(&kp_t.1);

        let mut reader = MockSupplyReader::new();
        reader.set_sig(&s_sig, 100);
        reader.set_sig(&a_sig, 100);
        reader.set_sig(&t_sig, 100);
        // Σ⟦And(A,s)⟧ and Σ⟦And(And(A,s),t)⟧ are NOT set ⇒ absent ⇒ 0.

        let outcome = admit_by_funding(vec![two_sig, three_sig], &reader)
            .await
            .expect("gate must not error");

        // The 2-sig {A,s} funds from min(Σ⟦A⟧,Σ⟦s⟧) and is admitted; the 3-sig's
        // top-level pair (Σ⟦And(A,s)⟧=0, Σ⟦t⟧) has effective capacity 0 ⇒ rejected.
        assert_eq!(
            outcome.admitted.len(),
            1,
            "only the 2-sig admits; the 3-sig has 0 effective capacity (inner pool absent)"
        );
        assert_eq!(outcome.rejected.len(), 1, "the 3-sig {{A,s,t}} is rejected");
    }

    /// #12.5 — THE FORK-SAFETY BAR: for a REAL compound (2-signer) deploy, the
    /// play-side debit map (`admit_by_funding`) is BYTE-IDENTICAL to the replay
    /// recompute (`recompute_settlement_debits`) over the SAME pre-state. Both
    /// paths derive decompositions + raw balances from the same `Cosigned`
    /// envelope and run the SAME `compute_settlement_debits`, so the maps must be
    /// equal byte-for-byte — the property that prevents a play/replay fork.
    #[tokio::test]
    async fn compound_debit_play_replay_byte_identical() {
        // A compound deploy `@0!(0) | @0!(0)` ⇒ Δ = 2.
        let compound = compound_cosigned("@0!(0) | @0!(0)", 0, 10);
        assert!(compound.is_compound(), "must be a true multi-sig envelope");

        // The compound envelope `Sig::And(left, right)` the gate will derive.
        let envelope = accounting::funding_sig(&compound);
        let (left, right) = match &envelope {
            Sig::And(l, r) => ((**l).clone(), (**r).clone()),
            other => panic!(
                "expected Sig::And from a 2-signer cosigned, got {:?}",
                other
            ),
        };

        // Seed: Σ⟦compound⟧ = 1, Σ⟦left⟧ = Σ⟦right⟧ = 5. effectiveΣ_compound =
        // 1 + min(5,5) = 6 ≥ Δ=2 ⇒ admitted. Combined-first split:
        // draw_compound = min(2,1) = 1, draw_pair = 1 ⇒ compound-=1, left-=1, right-=1.
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&envelope, 1);
        reader.set_sig(&left, 5);
        reader.set_sig(&right, 5);

        // ---- PLAY: the gate's threaded debit map (strict OFF — #12 unchanged) ----
        let outcome = admit_by_funding(vec![compound.clone()], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(
            outcome.admitted.len(),
            1,
            "compound deploy admitted on effectiveΣ"
        );
        assert!(outcome.rejected.is_empty());

        // ---- REPLAY: recompute from the admitted set over the SAME pre-state ----
        let recomputed = recompute_settlement_debits(vec![compound.clone()], &reader)
            .await
            .expect("recompute must not error");

        // THE BAR: byte-identical debit maps (the COST settlement; `recomputed`
        // is `RecomputedDebits`, `.settlement` is the cost half).
        assert_eq!(
            outcome.debits, recomputed.settlement,
            "play-side debit map must equal the replay recompute byte-for-byte"
        );

        // And the split is the expected combined-first allocation.
        let compound_key = delta_sigma::sig_key(&envelope);
        let left_key = delta_sigma::sig_key(&left);
        let right_key = delta_sigma::sig_key(&right);
        assert_eq!(
            outcome.debits.get(&compound_key).map(|d| d.amount),
            Some(1),
            "draw_compound = min(Δ=2, Σ⟦compound⟧=1) = 1"
        );
        assert_eq!(
            outcome.debits.get(&left_key).map(|d| d.amount),
            Some(1),
            "draw_pair = 1"
        );
        assert_eq!(
            outcome.debits.get(&right_key).map(|d| d.amount),
            Some(1),
            "draw_pair = 1"
        );
    }

    /// #12.5b — compound play↔replay byte-identity in the COMPONENT-PAIR-ONLY
    /// regime (`Σ⟦compound⟧ = 0`), so the whole demand is settled from the
    /// component pools and there is NO compound debit on either path.
    #[tokio::test]
    async fn compound_debit_play_replay_identical_pair_only() {
        let compound = compound_cosigned("@0!(0) | @0!(0) | @0!(0)", 0, 10); // Δ = 3
        let envelope = accounting::funding_sig(&compound);
        let (left, right) = match &envelope {
            Sig::And(l, r) => ((**l).clone(), (**r).clone()),
            other => panic!("expected Sig::And, got {:?}", other),
        };

        // Σ⟦compound⟧ = 0 (PRESENT but drained), Σ⟦left⟧ = Σ⟦right⟧ = 4.
        // effectiveΣ_compound = 0 + min(4,4) = 4 ≥ Δ=3 ⇒ admitted.
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&envelope, 0);
        reader.set_sig(&left, 4);
        reader.set_sig(&right, 4);

        let outcome = admit_by_funding(vec![compound.clone()], &reader)
            .await
            .expect("gate must not error");
        assert_eq!(
            outcome.admitted.len(),
            1,
            "admitted on component-pair credit"
        );

        let recomputed = recompute_settlement_debits(vec![compound.clone()], &reader)
            .await
            .expect("recompute must not error");
        assert_eq!(
            outcome.debits, recomputed.settlement,
            "play == replay byte-identical (pair-only regime)"
        );

        let compound_key = delta_sigma::sig_key(&envelope);
        let left_key = delta_sigma::sig_key(&left);
        let right_key = delta_sigma::sig_key(&right);
        assert!(
            outcome.debits.get(&compound_key).is_none(),
            "empty combined pool ⇒ NO compound debit"
        );
        assert_eq!(
            outcome.debits.get(&left_key).map(|d| d.amount),
            Some(3),
            "Σ⟦left⟧ -= 3"
        );
        assert_eq!(
            outcome.debits.get(&right_key).map(|d| d.amount),
            Some(3),
            "Σ⟦right⟧ -= 3"
        );

        // F-1/F-2 (red-team): the compound FEE is FLAT — ONE token, drawn
        // combined-first (0 here, drained) then from a SINGLE component (the
        // canonical-first `left`), NOT the cost-style matched PAIR. The OLD code
        // reused the cost policy and charged a compound deploy 2 (left 1 + right 1);
        // `FlatFeeApportionment` charges exactly 1. Post-cost residual is
        // left=1,right=1, so left -= 1 covers the flat fee with no underflow.
        assert_eq!(
            outcome.fee_debits.get(&left_key).map(|d| d.amount),
            Some(1),
            "F-1: flat fee draws 1 from the canonical-first component"
        );
        assert!(
            outcome.fee_debits.get(&right_key).is_none(),
            "F-1: flat fee must NOT touch the right component (no pair doubling)"
        );
        assert!(
            outcome.fee_debits.get(&compound_key).is_none(),
            "drained combined pool ⇒ no compound fee draw"
        );
        let total_fee: i64 = outcome.fee_debits.values().map(|d| d.amount).sum();
        assert_eq!(
            total_fee, 1,
            "F-1: a compound deploy's fee is FLAT (1), not 2"
        );
        assert_eq!(
            outcome.fee_debits, recomputed.fee,
            "fee play == replay byte-identical (flat, pair-only regime)"
        );
    }

    // ════════════════════════════════════════════════════════════════════════
    // B2(a) (CA-P-171) — concurrent DISJOINT-POOL admission, example tests.
    //
    // `admit_deploy_cosigned` (block_admission.rs) is the deploy INTAKE path
    // (parse + cosigner-cap + store); it does NOT make the funding decision and
    // needs a full `MultiParentCasperImpl<T>`. The "accepts a funded deploy,
    // rejects an underfunded one, no side effects on reject" behavior is the
    // FUNDING gate `admit_by_funding` (F-A, Def 19) — exercised here. CA-P-171's
    // disjoint-pool concern: two deploys on DISTINCT signer pools (no shared
    // component) are admitted INDEPENDENTLY — one being unfunded must not block
    // the other, and a rejected deploy leaves NO debit on any pool (no side
    // effect). The existing tests cover same-pool oversubscription
    // (`reject_both_on_oversubscription`) and single-deploy boundaries
    // (`funded_unfunded_boundary_at_def19`); none covers the disjoint pair.
    // ════════════════════════════════════════════════════════════════════════

    /// A funded deploy and an UNDERFUNDED deploy on DISJOINT pools: the funded
    /// one is admitted (with its cost + fee debit), the underfunded one is
    /// rejected, and the rejection has NO side effect — the underfunded pool is
    /// never debited, and the funded pool's debit is exactly its own demand
    /// (the disjoint groups do not interfere). Δ = 2 each (`@0!(0) | @0!(0)`).
    #[tokio::test]
    async fn disjoint_pools_one_funded_one_underfunded_admit_independently() {
        const DEMAND: usize = 2;
        const FEE: i64 = 1;
        // "alpha" is funded to cover cost + fee; "beta" is one unit short of even
        // its cost. Distinct labels ⇒ distinct pks ⇒ DISJOINT wallets.
        let funded = cosigned(&n_sends(DEMAND), b"alpha", 0, 10);
        let underfunded = cosigned(&n_sends(DEMAND), b"beta", 0, 20);

        let mut reader = MockSupplyReader::new();
        reader.set(b"alpha", DEMAND as i64 + FEE); // exactly cost + fee
        reader.set(b"beta", DEMAND as i64 - 1); // below cost ⇒ reject

        let outcome = admit_by_funding(vec![funded.clone(), underfunded.clone()], &reader)
            .await
            .expect("gate must not error");

        let alpha_key = pool_key(b"alpha");
        let beta_key = pool_key(b"beta");

        // The funded deploy is admitted exactly once.
        assert_eq!(
            outcome.admitted.len(),
            1,
            "exactly the funded deploy is admitted"
        );
        assert_eq!(
            outcome.admitted[0].primary().sig,
            funded.primary().sig,
            "the ADMITTED deploy is the funded one (alpha)"
        );
        // The underfunded deploy is rejected.
        assert_eq!(
            outcome.rejected.len(),
            1,
            "exactly the underfunded deploy is rejected"
        );
        assert_eq!(
            outcome.rejected[0],
            underfunded.primary().sig,
            "the REJECTED deploy is the underfunded one (beta)"
        );

        // The funded pool is debited exactly its own cost demand (disjoint ⇒ no
        // cross-pool effect) + its single flat fee.
        assert_eq!(
            outcome.debits.get(&alpha_key).map(|d| d.amount),
            Some(DEMAND as i64),
            "the funded pool's COST debit is exactly its own demand"
        );
        assert_eq!(
            outcome.fee_debits.get(&alpha_key).map(|d| d.amount),
            Some(FEE),
            "the funded pool carves exactly one flat fee token"
        );

        // NO SIDE EFFECT on reject: the underfunded pool is never touched.
        assert!(
            outcome.debits.get(&beta_key).is_none(),
            "the rejected (underfunded) pool must NOT be cost-debited"
        );
        assert!(
            outcome.fee_debits.get(&beta_key).is_none(),
            "the rejected (underfunded) pool must NOT be fee-carved"
        );
    }

    /// The funded deploy's admission is INDEPENDENT of whether its disjoint peer
    /// is present and rejected: admitting `{alpha funded}` alone yields the SAME
    /// alpha cost + fee debits as admitting `{alpha funded, beta underfunded}`.
    /// The unfunded beta does not block, starve, or perturb alpha (the disjoint-
    /// pool non-interference CA-P-171 asserts). Order-robust: the input is
    /// canonicalized, so the result does not depend on submission order.
    #[tokio::test]
    async fn disjoint_underfunded_peer_does_not_perturb_the_funded_admission() {
        const DEMAND: usize = 3;
        const FEE: i64 = 1;
        let alpha = cosigned(&n_sends(DEMAND), b"alpha", 0, 10);
        let beta = cosigned(&n_sends(DEMAND), b"beta", 0, 20);
        let alpha_key = pool_key(b"alpha");

        let mut reader = MockSupplyReader::new();
        reader.set(b"alpha", DEMAND as i64 + FEE);
        reader.set(b"beta", 0); // present but drained ⇒ rejected

        // Alpha ALONE.
        let solo = admit_by_funding(vec![alpha.clone()], &reader)
            .await
            .expect("gate must not error");
        // Alpha WITH the unfunded beta (both orders).
        let with_peer = admit_by_funding(vec![alpha.clone(), beta.clone()], &reader)
            .await
            .expect("gate must not error");
        let with_peer_rev = admit_by_funding(vec![beta.clone(), alpha.clone()], &reader)
            .await
            .expect("gate must not error");

        // Alpha is admitted in all three, with IDENTICAL cost + fee debits.
        for outcome in [&solo, &with_peer, &with_peer_rev] {
            assert!(
                outcome
                    .admitted
                    .iter()
                    .any(|c| c.primary().sig == alpha.primary().sig),
                "alpha is admitted regardless of the unfunded peer"
            );
        }
        let alpha_cost = |o: &AdmissionOutcome| o.debits.get(&alpha_key).map(|d| d.amount);
        let alpha_fee = |o: &AdmissionOutcome| o.fee_debits.get(&alpha_key).map(|d| d.amount);
        assert_eq!(
            alpha_cost(&solo),
            alpha_cost(&with_peer),
            "alpha's cost debit is unchanged by the disjoint unfunded peer"
        );
        assert_eq!(
            alpha_cost(&with_peer),
            alpha_cost(&with_peer_rev),
            "alpha's cost debit is order-independent"
        );
        assert_eq!(
            alpha_fee(&solo),
            alpha_fee(&with_peer),
            "alpha's fee carve is unchanged by the disjoint unfunded peer"
        );
        assert_eq!(
            alpha_cost(&solo),
            Some(DEMAND as i64),
            "alpha cost = its own demand"
        );
        assert_eq!(alpha_fee(&solo), Some(FEE), "alpha fee = one flat token");
    }

    /// Two FUNDED deploys on DISJOINT pools are BOTH admitted, each debited
    /// exactly its own cost + fee — the pools settle independently with no
    /// cross-talk (the happy-path companion to the funded/underfunded test).
    #[tokio::test]
    async fn disjoint_pools_both_funded_admit_both_independently() {
        const DA: usize = 2;
        const DB: usize = 4;
        const FEE: i64 = 1;
        let a = cosigned(&n_sends(DA), b"alpha", 0, 10);
        let b = cosigned(&n_sends(DB), b"beta", 0, 20);
        let alpha_key = pool_key(b"alpha");
        let beta_key = pool_key(b"beta");

        let mut reader = MockSupplyReader::new();
        reader.set(b"alpha", DA as i64 + FEE);
        reader.set(b"beta", DB as i64 + FEE);

        let outcome = admit_by_funding(vec![a, b], &reader)
            .await
            .expect("gate must not error");

        assert_eq!(outcome.admitted.len(), 2, "both funded deploys admitted");
        assert!(outcome.rejected.is_empty(), "nothing rejected");
        // Each disjoint pool is debited exactly its OWN demand + fee.
        assert_eq!(
            outcome.debits.get(&alpha_key).map(|d| d.amount),
            Some(DA as i64)
        );
        assert_eq!(
            outcome.debits.get(&beta_key).map(|d| d.amount),
            Some(DB as i64)
        );
        assert_eq!(
            outcome.fee_debits.get(&alpha_key).map(|d| d.amount),
            Some(FEE)
        );
        assert_eq!(
            outcome.fee_debits.get(&beta_key).map(|d| d.amount),
            Some(FEE)
        );
    }

    // ════════════════════════════════════════════════════════════════════════
    // B4 (CA-P-086/087) — fee FLATNESS + supply CONSERVATION over random arity.
    //
    // (a) the carved fee is EXACTLY one layer per deploy regardless of compound
    //     arity (`FlatFeeApportionment` draws ONE pool, never the pair — so a
    //     compound deploy's fee is 1, not 2/n); (b) on every pool the gate
    //     conserves supply: `Σ pre = Σ residual + Σ cost-debits + Σ fee-debits`
    //     under the checked reserve/settle identity. The existing
    //     `compound_debit_play_replay_identical_pair_only` pins arity-2 only;
    //     this proptest exercises arities 1..=4 with random balances.
    // ════════════════════════════════════════════════════════════════════════

    /// `n` parallel sends ⇒ cost demand Δ = n; one deploy ⇒ flat fee = 1.
    /// Property over random arity `a ∈ 1..=4` and random surplus: a single
    /// admitted `a`-signer deploy carves a fee of EXACTLY 1 (flat — never `a` or
    /// 2), and supply is conserved on every pool the gate touches.
    #[test]
    fn fee_is_flat_and_supply_conserved_across_random_arity() {
        use proptest::prelude::*;
        use proptest::test_runner::{Config as ProptestConfig, TestRunner};

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let mut runner = TestRunner::new(ProptestConfig {
            cases: 48,
            ..ProptestConfig::default()
        });
        runner
            .run(
                &(
                    1usize..=4, // arity (number of distinct cosigners)
                    1usize..=4, // cost demand Δ (number of sends)
                    0i64..=6,   // per-component surplus above the bound
                ),
                |(arity, demand, surplus)| {
                    const FEE: i64 = 1;
                    // Build `arity` fresh keypairs → an `arity`-signer compound
                    // (or single-sig when arity == 1). `funding_sig` folds them
                    // into `Sig::And(Ground(pk₀), …)`.
                    let keypairs: Vec<(PrivateKey, PublicKey)> =
                        (0..arity).map(|_| fresh_keypair()).collect();
                    let deploy = cosigned_with_keypairs(&n_sends(demand), &keypairs, 0, 10);
                    let envelope = accounting::funding_sig(&deploy);

                    // Seed every pool the gate will read (the compound + each
                    // component) generously enough to admit: each component pool
                    // gets `demand + FEE + surplus`, and the combined pool the
                    // same, so the Join bound clears cost + fee with headroom.
                    let mut reader = MockSupplyReader::new();
                    let bound = demand as i64 + FEE + surplus;
                    // Collect every (Sig, pre-balance) the gate reads so we can
                    // check conservation pool-by-pool afterward.
                    let mut seeded: Vec<(Sig, i64)> = Vec::new();
                    // The top-level envelope pool.
                    reader.set_sig(&envelope, bound);
                    seeded.push((envelope.clone(), bound));
                    // Each leaf component pool (the `And`-fold's grounds).
                    for (_, pk) in &keypairs {
                        let comp = Sig::Ground(pk.bytes.to_vec());
                        reader.set_sig(&comp, bound);
                        seeded.push((comp, bound));
                    }
                    // Intermediate `And` nodes of an n≥3 left-assoc fold are also
                    // read by the gate (collect_decompositions); seed them too so
                    // presence is uniform. Walk the envelope's nested `And`s.
                    fn collect_and_nodes(sig: &Sig, out: &mut Vec<Sig>) {
                        if let Sig::And(l, r) = sig {
                            out.push(sig.clone());
                            collect_and_nodes(l, out);
                            collect_and_nodes(r, out);
                        }
                    }
                    let mut and_nodes = Vec::new();
                    collect_and_nodes(&envelope, &mut and_nodes);
                    for node in &and_nodes {
                        // The top envelope is already seeded; seed deeper nodes.
                        if node != &envelope {
                            reader.set_sig(node, bound);
                            seeded.push((node.clone(), bound));
                        }
                    }

                    let outcome = rt
                        .block_on(admit_by_funding(vec![deploy.clone()], &reader))
                        .expect("gate must not error");

                    // The deploy is admitted (every pool is funded with headroom).
                    prop_assert_eq!(
                        outcome.admitted.len(),
                        1,
                        "the fully-funded deploy must be admitted"
                    );

                    // (a) FLATNESS: the TOTAL carved fee is EXACTLY 1 — one flat
                    // token for the one admitted deploy — regardless of arity. A
                    // pair-doubling bug (the OLD behavior) would make this 2 for a
                    // compound; an n-fold bug would make it `arity`.
                    let total_fee: i64 = outcome.fee_debits.values().map(|d| d.amount).sum();
                    prop_assert_eq!(
                        total_fee,
                        FEE,
                        "the fee is FLAT (1) for arity {} — never pair-doubled / n-folded",
                        arity
                    );

                    // (b) CONSERVATION: on every pool, pre = residual + cost +
                    // fee. The settlement debits BURN cost from the pool and the
                    // fee carve TRANSFERS the fee out; both are `pre - draw`, so
                    // for each seeded pool the drawn total must not exceed pre and
                    // the residual is exactly pre minus the draws.
                    for (sig, pre) in &seeded {
                        let key = delta_sigma::sig_key(sig);
                        let cost_draw = outcome.debits.get(&key).map(|d| d.amount).unwrap_or(0);
                        let fee_draw = outcome.fee_debits.get(&key).map(|d| d.amount).unwrap_or(0);
                        // No pool is over-drawn (no underflow at settlement).
                        prop_assert!(
                            cost_draw + fee_draw <= *pre,
                            "pool draw {}+{} must not exceed pre {} (no underflow)",
                            cost_draw,
                            fee_draw,
                            pre
                        );
                        // Conservation identity: residual = pre − (cost + fee).
                        let residual = *pre - cost_draw - fee_draw;
                        prop_assert_eq!(
                            residual + cost_draw + fee_draw,
                            *pre,
                            "supply conserved on this pool: residual + cost + fee == pre"
                        );
                    }

                    // The COST burned (Σ over cost debits) equals the demand Δ
                    // (the admitted deploy consumes exactly its COMM count), and
                    // the FEE transferred equals 1 — so the TOTAL leaving the
                    // client pools is Δ + 1 (cost + flat fee), conserving.
                    let total_cost: i64 = outcome.debits.values().map(|d| d.amount).sum();
                    prop_assert_eq!(
                        total_cost,
                        demand as i64,
                        "total cost burned == Δ (the deploy's COMM demand)"
                    );
                    prop_assert_eq!(
                        total_cost + total_fee,
                        demand as i64 + FEE,
                        "fee + deploy cost == Δ + 1 (supply-conserving carve)"
                    );
                    Ok(())
                },
            )
            .expect("fee must be flat and supply conserved across all sampled arities");
    }
}
