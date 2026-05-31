//! WD-D2 — per-signature acceptance gate + settlement-debit computation.
//!
//! The CONSENSUS-CRITICAL block-assembly funding gate of the Cost-Accounted Rho
//! Calculus (spec `publications/cost-accounting/cost-accounted-rho.tex` §7.6/§7.7;
//! authoritative design `docs/theory/cost-accounting-impl/wd-d2-acceptance-gate.md`).
//! Wires three landed pieces into one decision:
//!   * the PURE per-signature demand analyzer `Δ_s` + Split/Join supply closure
//!     (`rholang/.../accounting/delta_sigma.rs`, WD-D1);
//!   * the per-signature supply pool `Σ⟦s⟧` read helpers (`supply.rs`, StageB);
//!   * the ONE extracted envelope-`Sig` derivation (`accounting::envelope_sig`).
//!
//! ## What the gate computes (and does not)
//!
//! [`admit_by_funding`] decides, for each per-signature group (deploys sharing a
//! supply pool `Σ⟦s⟧`), the LARGEST canonical-order prefix whose cumulative
//! demand `Σ Δ_s` fits the EFFECTIVE supply (§7.7 reject-both / no-partial: on the
//! first unfunded candidate, reject it AND all after it in the group). It returns
//! the admitted envelopes (in canonical order, fed straight to execution), the
//! rejected primary signatures (unioned into the block's `rejected_deploys`), and
//! the per-pool SETTLEMENT DEBIT `Σ Δ_s` (the amount `CloseBlockDeploy` subtracts
//! from `Σ⟦s⟧` so `post = pre − Σ Δ_admitted`).
//!
//! It does NOT execute anything (it is a pure O(AST) static analysis) and it does
//! NOT mutate RSpace — the single consensus decrement is the settlement debit,
//! applied once by `CloseBlockDeploy::dual_write_supply` AFTER all user deploys
//! have executed (handoff Decision 4c).
//!
//! ## Determinism (the fork-avoidance bar)
//!
//! Every input that feeds the verdict is consensus-deterministic: the analyzer is
//! pure, the groups are a `BTreeMap` (deterministic iteration), the supply reads
//! come from the merged pre-state hash (already a consensus quantity), and the
//! genesis `margin` is on-chain (`min_phlo_price`). The block proposer collects
//! deploys into a `HashSet` whose iteration order is nondeterministic across
//! nodes, so this gate RE-IMPOSES the canonical order itself
//! ([`canonical_sort`], the `block_creator.rs:315-324` comparator) before grouping
//! and prefix selection — replay recomputes the identical verdict from
//! `block.body.deploys`.
//!
//! ## Compound (multi-pool) scope — D2 cap (tracked D2→D3 follow-on)
//!
//! D2 computes `effective_supply_with` faithfully for the ADMISSION decision
//! (so a compound deploy may be admitted on component-pair credit), but DEBITS
//! only directly-targeted pools and CAPS a compound's admission at its OWN pool
//! `Σ_compound` — keeping the settlement single-pool and underflow-free. The full
//! multi-pool draw-allocation is a funding-slot mechanism, out of the D2
//! consensus-gate scope. Single-signer (the only shape the pool carries today,
//! all §7.4 examples) is EXACT: `Σ⟦s⟧ -= Σ Δ_s`.

use std::collections::BTreeMap;

use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::delta_sigma::{self, Decomposition, DemandEntry};
// Re-exported (NOT a private `use`) so settlement-debit consumers
// (`CloseBlockDeploy.settlement_debits`) key the map by the same canonical
// basis (`Sig::lane_hash`) without reaching into rholang internals.
pub use rholang::rust::interpreter::accounting::delta_sigma::SigKey;
use rholang::rust::interpreter::accounting::{self, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::supply;

/// One per-pool settlement debit: the amount `Σ Δ_s` to subtract from the
/// supply channel `Σ⟦s⟧` so `post = pre − Σ Δ_admitted` (handoff Decision 4c).
/// Carries the resolved channel so the settlement writer needs no `Sig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementDebit {
    /// `Σ⟦s⟧ = SignatureChannel::from_sig(s).par` — the pool to debit.
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
    /// The PRIMARY signatures of gate-rejected deploys, unioned into the
    /// block's `rejected_deploys` at packaging.
    pub rejected: Vec<Bytes>,
    /// The per-pool settlement debit, keyed by `SigKey` (= `Sig::lane_hash`).
    /// Threaded to `CloseBlockDeploy.settlement_debits` on the play path;
    /// RECOMPUTED identically from `block.body.deploys` on replay.
    pub debits: BTreeMap<SigKey, SettlementDebit>,
    /// Cost-Accounted Rho Stage D: the count of GATE-ADMITTED client deploy
    /// envelopes (= `admitted.len()`). The per-block fee (the spec's flat
    /// `FeeExtract` — ONE token per admitted client deploy, tex:2509-2521) is
    /// credited to the proposing validator's fee channel `F_v`. The proposer
    /// ADDS its own (gate-exempt) dummy-deploy count to this to obtain the
    /// total fee = `block.body.deploys.len()`, which is what [`recompute_fee_credits`]
    /// derives identically on replay from `terms.len()` (= `block.body.deploys`).
    /// This count does NOT affect the D2 gate decision (it is read-only metadata
    /// computed from the already-decided admitted set).
    pub admitted_client_count: usize,
}

/// Cost-Accounted Rho Stage D fee credit (the spec's `FeeExtract`): a flat ONE
/// token per admitted deploy in the block, collected into the PROPOSING
/// validator's fee channel `F_v`. Distinct from the COST (the D2
/// `SettlementDebit`, which burns from the signer's own `Σ⟦s⟧`): the fee is a
/// SEPARATE token TRANSFERRED to the validator (§7 funding model). Carries the
/// consensus-deterministic recipient (`block_data.sender`) and the amount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeCredit {
    /// The proposing validator's public-key bytes (`block_data.sender.bytes`) —
    /// the fee recipient. Used by `CloseBlockDeploy` to credit `F_v` for this pk.
    pub recipient_pk: Vec<u8>,
    /// The fee amount = `block.body.deploys.len() × 1` (flat 1-token per admitted
    /// deploy). Recomputed identically on replay from `terms.len()`.
    pub amount: i64,
}

/// An async per-channel supply-balance reader returning PRESENCE: `Some(n)` iff
/// a balance datum is resident on `chan` (even `n == 0`), `None` iff the pool is
/// absent. The presence distinction is the gate's per-pool ACTIVATION signal
/// (see [`read_balance_present`] / `admit_by_funding`). Two implementations keep
/// the gate's read symmetric across play and replay:
///   * play (block assembly): [`RuntimeManagerSupplyReader`] over a merged
///     pre-state HASH via `RuntimeManager::get_data`;
///   * replay: [`RuntimeOpsSupplyReader`] over the LIVE store already `reset` to
///     `start_hash`.
///
/// Both decode through the SAME `supply::decode_balance_present`, so the read is
/// byte-identical for a given state root.
///
/// `Send + Sync` so a `&dyn SupplyReader` can be held across an `.await` inside a
/// `Send` future (the gate runs on the async block-assembly / replay paths).
pub trait SupplyReader: Send + Sync {
    /// Read `supply(s)` from `chan`: `Some(n)` if a balance datum is present
    /// (including `Some(0)`), `None` if the pool is absent.
    fn read_balance<'a>(
        &'a self,
        chan: &'a Par,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<i64>, CasperError>> + Send + 'a>,
    >;
}

/// Play-side supply reader: reads each pool from the merged pre-state hash via
/// `RuntimeManager::get_data` (spawns a runtime at that root, reads, decodes).
pub struct RuntimeManagerSupplyReader<'rm> {
    pub runtime_manager: &'rm RuntimeManager,
    pub pre_state_hash: StateHash,
}

impl<'rm> SupplyReader for RuntimeManagerSupplyReader<'rm> {
    fn read_balance<'a>(
        &'a self,
        chan: &'a Par,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<i64>, CasperError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let data = self
                .runtime_manager
                .get_data(self.pre_state_hash.clone(), chan)
                .await?;
            Ok(supply::decode_balance_present(&data))
        })
    }
}

/// Replay-side supply reader: reads each pool from the LIVE hot store (already
/// `reset` to `start_hash`) via `supply::read_balance_present`. Same decoder,
/// same root ⇒ byte-identical presence/balances to the play-side read.
pub struct RuntimeOpsSupplyReader<'ops> {
    pub runtime_ops: &'ops RuntimeOps,
}

impl<'ops> SupplyReader for RuntimeOpsSupplyReader<'ops> {
    fn read_balance<'a>(
        &'a self,
        chan: &'a Par,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<i64>, CasperError>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(supply::read_balance_present(self.runtime_ops, chan).await) })
    }
}

/// A canonicalized gate candidate: the deploy envelope, its supply key, its
/// resolved supply channel, and its static demand.
struct Candidate {
    cosigned: Cosigned<DeployData>,
    sig_key: SigKey,
    channel: Par,
    demand: DemandEntry,
    /// `true` iff the term is malformed (`source_to_adt` failed). Malformed
    /// terms are REJECTED outright (the runtime would fail them too), never
    /// grouped/admitted.
    malformed: bool,
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

/// Build the gate candidate for one deploy: derive the envelope `Sig` via the
/// ONE shared `accounting::envelope_sig`, the supply key + channel from it, and
/// the static demand `Δ_s` from the desugared term. A term whose
/// `source_to_adt` fails is flagged `malformed` (⇒ rejected).
fn build_candidate(cosigned: Cosigned<DeployData>) -> Candidate {
    let envelope: Sig = accounting::envelope_sig(&cosigned);
    let sig_key = delta_sigma::sig_key(&envelope);
    let channel = supply::supply_channel(&envelope);

    match Compiler::source_to_adt(&cosigned.data().term) {
        Ok(par) => {
            let desugared = delta_sigma::desugar_for_funding(&par);
            // D3 (DR-9): `demand` is now the per-COMM count (send/receive only;
            // new/match/if are diagnostic Reductions). `known_lower_bound`
            // therefore equals the runtime's consumed per-COMM `total_cost()`,
            // so gate demand == runtime consumed == settlement debit, all
            // per-COMM (the D1→D3 handoff completed in lockstep).
            let demand = delta_sigma::demand(&desugared, &envelope);
            Candidate {
                cosigned,
                sig_key,
                channel,
                demand,
                malformed: false,
            }
        }
        Err(_) => Candidate {
            cosigned,
            sig_key,
            channel,
            demand: DemandEntry::ZERO,
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
    if let Sig::And(left, right) = envelope {
        let compound = delta_sigma::sig_key(envelope);
        let left_key = delta_sigma::sig_key(left);
        let right_key = delta_sigma::sig_key(right);
        out.push(Decomposition {
            compound,
            left: left_key,
            right: right_key,
        });
        component_channels
            .entry(left_key)
            .or_insert_with(|| supply::supply_channel(left));
        component_channels
            .entry(right_key)
            .or_insert_with(|| supply::supply_channel(right));
        // Recurse so a left-associated n≥3 fold yields one decomposition per
        // internal `And` node.
        collect_decompositions(left, out, component_channels);
        collect_decompositions(right, out, component_channels);
    }
}

/// The per-signature acceptance gate (cost-accounted-rho §7.6/§7.7). See the
/// module docs and `wd-d2-acceptance-gate.md` §D2.2 for the binding algorithm.
///
/// `deploys` are the candidate user deploy envelopes (`HashSet`-sourced, hence
/// nondeterministically ordered — this function re-sorts canonically).
/// `supply_reader` reads each pool `Σ⟦s⟧` from the consensus pre-state (play:
/// merged pre-state hash; replay: live store reset to `start_hash`). `margin` is
/// the on-chain genesis safety buffer (`min_phlo_price`).
///
/// Returns the [`AdmissionOutcome`]: admitted envelopes in canonical order, the
/// rejected primary sigs, and the per-pool settlement debits.
pub async fn admit_by_funding(
    deploys: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
    margin: i64,
) -> Result<AdmissionOutcome, CasperError> {
    // 1. Canonicalize the (nondeterministically-ordered) input.
    let mut ordered = deploys;
    canonical_sort(&mut ordered);

    // 2. Build candidates (envelope Sig → key/channel/demand). Malformed terms
    //    are split off as rejected immediately (never grouped).
    let mut outcome = AdmissionOutcome::default();
    let mut candidates: Vec<Candidate> = Vec::with_capacity(ordered.len());
    for cosigned in ordered {
        let candidate = build_candidate(cosigned);
        if candidate.malformed {
            outcome.rejected.push(candidate.cosigned.primary().sig.clone());
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
    for group in groups.values() {
        // All candidates in a group share the same envelope key/channel; the
        // first is the representative.
        if let Some(repr) = group.first() {
            channels_by_key
                .entry(repr.sig_key)
                .or_insert_with(|| repr.channel.clone());
            let envelope = accounting::envelope_sig(&repr.cosigned);
            collect_decompositions(&envelope, &mut decompositions, &mut channels_by_key);
        }
    }

    // 5. Read each distinct channel's PRESENCE + balance exactly once.
    //    `present` records which pools exist (the per-pool ACTIVATION signal);
    //    `raw` holds balances with absent ⇒ 0 (the Split/Join closure math).
    let mut present: std::collections::BTreeSet<SigKey> = std::collections::BTreeSet::new();
    let mut raw: BTreeMap<SigKey, i64> = BTreeMap::new();
    for (key, chan) in &channels_by_key {
        match supply_reader.read_balance(chan).await? {
            Some(balance) => {
                present.insert(*key);
                raw.insert(*key, balance);
            }
            None => {
                raw.insert(*key, 0);
            }
        }
    }

    // 6. Apply the Split/Join closure to get the EFFECTIVE supplies.
    let effective = delta_sigma::effective_supply_with(&raw, &decompositions);

    // The set of compound keys (a compound's admission is capped at its OWN
    // pool `Σ_compound` in D2 — component-pair credit is non-spendable here).
    let compound_keys: std::collections::BTreeSet<SigKey> =
        decompositions.iter().map(|d| d.compound).collect();

    // 7. Per-group prefix admission (reject-both), accumulating Σ Δ_s.
    for (sig_key, group) in groups {
        let channel = group
            .first()
            .map(|c| c.channel.clone())
            .unwrap_or_default();

        // ACTIVATION (reported grounding refinement — see `supply::read_balance_present`):
        // a group whose pool is ABSENT is not yet under cost-accounting funding
        // (the Workstream-C economic producer has not provisioned it) ⇒ admit
        // the whole group with NO enforcement and NO debit (pre-C /
        // non-cost-accounted behavior, bit-for-bit). A PRESENT pool (including a
        // drained `Some(0)`) IS under cost-accounting ⇒ enforce the funding
        // obligation + §7.7 reject-both.
        if !present.contains(&sig_key) {
            for candidate in group {
                outcome.admitted.push(candidate.cosigned);
            }
            continue;
        }

        // The admission residual starts from the EFFECTIVE supply, EXCEPT for a
        // compound group where it is capped at the RAW compound pool
        // `Σ_compound` (D2 single-pool debit safety — see module docs).
        let effective_supply = *effective.get(&sig_key).unwrap_or(&0);
        let mut residual: i64 = if compound_keys.contains(&sig_key) {
            // Cap at the compound's own pool: admit no more than `Σ_compound`
            // can fund directly, so the single-pool debit never underflows.
            effective_supply.min(*raw.get(&sig_key).unwrap_or(&0))
        } else {
            effective_supply
        };

        let mut group_debit: i64 = 0;
        let mut prefix_open = true;
        for candidate in group {
            if prefix_open
                && delta_sigma::is_funded(&candidate.demand, residual, margin)
            {
                // Admit: consume the known lower bound from the residual and
                // accumulate the debit. (`is_funded` already folded margin +
                // the `unknown` over-approximation into the decision.)
                residual = residual.saturating_sub(candidate.demand.known_lower_bound);
                group_debit = group_debit.saturating_add(candidate.demand.known_lower_bound);
                outcome.admitted.push(candidate.cosigned);
            } else {
                // §7.7 reject-both: the FIRST unfunded candidate and ALL after
                // it in the group are rejected.
                prefix_open = false;
                outcome.rejected.push(candidate.cosigned.primary().sig.clone());
            }
        }

        if group_debit > 0 {
            outcome.debits.insert(
                sig_key,
                SettlementDebit {
                    channel,
                    amount: group_debit,
                },
            );
        }
    }

    // Re-impose canonical order on the admitted set: the per-group walk emits
    // each group's prefix in canonical order, but group iteration is by SigKey,
    // so a final canonical sort restores the global execution order.
    canonical_sort(&mut outcome.admitted);

    // Stage D (additive; does NOT touch the gate decision above): record the
    // admitted client-deploy count for the fee credit. The proposer adds its
    // own dummy-deploy count to reach `block.body.deploys.len()`.
    outcome.admitted_client_count = outcome.admitted.len();

    Ok(outcome)
}

/// REPLAY recompute of the WD-D2 settlement-debit map from the block's ADMITTED
/// deploys (`block.body.deploys`), for the replay-symmetric settlement debit.
///
/// `block.body.deploys` contains EXACTLY the gate-admitted envelopes (rejected
/// deploys carry only a sig in `rejected_deploys`, not a body). So the per-pool
/// settlement debit is simply `Σ Δ_s` over the admitted deploys in each PRESENT
/// pool — the SAME quantity `admit_by_funding` accumulated on the play path
/// (where `debits[pool].amount = Σ Δ_s over the admitted prefix`), recomputed
/// here from the block alone. This recompute is MARGIN-FREE: the admission
/// decision (which uses the margin) already happened on the play side and is
/// fixed by the block's contents; replay only needs to reproduce the debit
/// AMOUNTS. A PRESENT pool's debit is enforced by the settlement `checked_sub`
/// (an over-admitting proposer ⇒ `ΣΔ_s > Σ_s` ⇒ underflow ⇒ a detectable invalid
/// block — TM-CA-153 double-spend); an ABSENT pool is unenforced (no debit),
/// matching the play-side presence gate.
///
/// Returns the recomputed map (identical to the play-side `AdmissionOutcome.debits`
/// for a valid block) AND the count of admitted deploys (for the
/// `ReplayAdmissionMismatch` diagnostic).
pub async fn recompute_settlement_debits(
    admitted: Vec<Cosigned<DeployData>>,
    supply_reader: &dyn SupplyReader,
) -> Result<BTreeMap<SigKey, SettlementDebit>, CasperError> {
    // Group admitted deploys by pool, summing Δ_s. A malformed term among the
    // ADMITTED set cannot occur for a valid block (the proposer never admits a
    // malformed deploy), but treat it defensively as zero demand so the recompute
    // is total — the post-state root check backstops any such anomaly.
    let mut demand_by_key: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    for cosigned in admitted {
        let candidate = build_candidate(cosigned);
        if candidate.malformed {
            continue;
        }
        let entry = demand_by_key
            .entry(candidate.sig_key)
            .or_insert_with(|| (candidate.channel.clone(), 0));
        entry.1 = entry.1.saturating_add(candidate.demand.known_lower_bound);
    }

    // Restrict to PRESENT pools (absent ⇒ unenforced ⇒ no debit), mirroring the
    // play-side presence gate. Read each pool's presence exactly once.
    let mut debits: BTreeMap<SigKey, SettlementDebit> = BTreeMap::new();
    for (key, (channel, amount)) in demand_by_key {
        if amount == 0 {
            continue;
        }
        match supply_reader.read_balance(&channel).await? {
            Some(_present) => {
                debits.insert(key, SettlementDebit { channel, amount });
            }
            None => {
                // Absent pool: the deploy was admitted unenforced on the play
                // side (no debit). Skip.
            }
        }
    }
    Ok(debits)
}

/// Cost-Accounted Rho Stage D: REPLAY recompute of the per-block fee credit from
/// the block's deploys (`block.body.deploys`) + the proposing validator
/// (`block_data.sender`), MIRRORING [`recompute_settlement_debits`] in
/// discipline (recomputed-from-the-block, never serialized into it).
///
/// The fee is the spec's flat `FeeExtract`: ONE token per deploy the validator
/// processed in the block (tex:2509-2521), collected into the proposing
/// validator's fee channel `F_v`. The amount is therefore EXACTLY the number of
/// deploys recorded in `block.body.deploys` — a quantity that is byte-identical
/// on the play side (`admitted_client_count + dummy_count = block.body.deploys.len()`)
/// and the replay side (`deploy_count = terms.len()`, where `terms` IS
/// `block.body.deploys`), INCLUDING failed and dummy deploys (every fed deploy is
/// recorded as a `ProcessedDeploy`, failed or not — runtime.rs:881
/// `is_failed: !eval_succeeded`). This is the same `terms.len()` identity that
/// makes the settlement-debit recompute byte-identical; it is the StageD
/// fork-safety bar (TM-CA-160 fee-credit play/replay divergence).
///
/// Returns `None` for an empty block (no deploys ⇒ no fee), so callers thread no
/// fee credit through closeBlock in that case (a genesis / heartbeat-only block).
/// `recipient_pk` is the consensus-deterministic `block_data.sender.bytes`.
pub fn recompute_fee_credits(deploy_count: usize, recipient_pk: Vec<u8>) -> Option<FeeCredit> {
    if deploy_count == 0 {
        return None;
    }
    // Flat ONE token per admitted deploy (the spec's `c:()` FeeExtract; NO
    // configurable rate — spec-literal). `deploy_count` is bounded by the
    // block's deploy slot, far below i64::MAX, so the cast is exact.
    Some(FeeCredit {
        recipient_pk,
        amount: deploy_count as i64,
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
    use super::*;
    use crypto::rust::public_key::PublicKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use std::collections::HashMap;

    /// A canned per-channel supply reader keyed by the channel's wire encoding.
    struct MockSupplyReader {
        balances: HashMap<Vec<u8>, i64>,
    }

    impl MockSupplyReader {
        fn new() -> Self {
            Self {
                balances: HashMap::new(),
            }
        }

        /// Set the balance of the pool a given envelope `sig` maps to.
        fn set(&mut self, sig: &[u8], balance: i64) {
            use prost::Message;
            let envelope = accounting::envelope_sig_single(sig);
            let chan = supply::supply_channel(&envelope);
            self.balances.insert(chan.encode_to_vec(), balance);
        }
    }

    impl SupplyReader for MockSupplyReader {
        fn read_balance<'a>(
            &'a self,
            chan: &'a Par,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<i64>, CasperError>> + Send + 'a>,
        > {
            use prost::Message;
            let key = chan.encode_to_vec();
            // A `set` pool is PRESENT (`Some`); an unset pool is ABSENT (`None`).
            // The gate enforces funding only for present pools (activation).
            let balance = self.balances.get(&key).copied();
            Box::pin(async move { Ok(balance) })
        }
    }

    /// Build a `Cosigned<DeployData>` with the given Rholang `term`, primary
    /// signature bytes `sig` (controls the supply-pool group), and ordering
    /// fields. The gate does not verify signatures, so an arbitrary `sig` byte
    /// string is sufficient to place the deploy into a chosen group — two
    /// deploys sharing `sig` share a group (the s₀-collapse double-spend shape).
    fn cosigned(term: &str, sig: &[u8], vabn: i64, ts: i64) -> Cosigned<DeployData> {
        let data = DeployData {
            term: term.to_string(),
            time_stamp: ts,
            valid_after_block_number: vabn,
            shard_id: String::new(),
            expiration_timestamp: None,
        };
        let signed = Signed {
            data,
            // A deterministic 33-byte secp256k1-shaped public key placeholder;
            // unused by the gate (it keys on the envelope sig, not the pk).
            pk: PublicKey::from_bytes(&[2u8; 33]),
            sig: Bytes::copy_from_slice(sig),
            sig_algorithm: Box::new(Secp256k1),
        };
        Cosigned::from_single_signer(signed).expect("from_single_signer is infallible")
    }

    /// `n` parallel sends `@0!(0) | … | @0!(0)` ⇒ Δ = n (each send is one
    /// token-consuming COMM; see `delta_sigma::demand`).
    fn n_sends(n: usize) -> String {
        let one = "@0!(0)";
        std::iter::repeat(one).take(n).collect::<Vec<_>>().join(" | ")
    }

    /// Per-signature grouping + independence: two deploys SHARING a signature
    /// form ONE group whose pool funds exactly one; a third deploy with a
    /// DIFFERENT signature is an independent group, funded on its own pool.
    #[tokio::test]
    async fn per_signature_group_gate() {
        // Group A (sig = "alice"): two deploys, each Δ=3; pool funds exactly 3.
        // Group B (sig = "bob"):   one deploy,  Δ=2; pool funds 2.
        let a0 = cosigned(&n_sends(3), b"alice", 0, 10);
        let a1 = cosigned(&n_sends(3), b"alice", 0, 20);
        let b0 = cosigned(&n_sends(2), b"bob", 0, 30);

        let mut reader = MockSupplyReader::new();
        reader.set(b"alice", 3); // exactly one Δ=3 deploy fits
        reader.set(b"bob", 2); // the Δ=2 deploy fits

        let outcome = admit_by_funding(vec![a1.clone(), b0.clone(), a0.clone()], &reader, 0)
            .await
            .expect("gate must not error");

        // Group A: canonical order is a0 (ts=10) before a1 (ts=20); a0 admitted,
        // a1 rejected (pool exhausted). Group B: b0 admitted independently.
        let admitted_sigs: Vec<&[u8]> =
            outcome.admitted.iter().map(|c| c.primary().sig.as_ref()).collect();
        assert!(admitted_sigs.contains(&b"alice".as_ref()), "alice's first fits");
        assert!(admitted_sigs.contains(&b"bob".as_ref()), "bob is independent");
        assert_eq!(outcome.admitted.len(), 2, "a0 + b0 admitted");
        assert_eq!(outcome.rejected.len(), 1, "a1 rejected (pool exhausted)");
        // Debits: alice pool -= 3, bob pool -= 2.
        let alice_key = delta_sigma::sig_key(&accounting::envelope_sig_single(b"alice"));
        let bob_key = delta_sigma::sig_key(&accounting::envelope_sig_single(b"bob"));
        assert_eq!(outcome.debits.get(&alice_key).map(|d| d.amount), Some(3));
        assert_eq!(outcome.debits.get(&bob_key).map(|d| d.amount), Some(2));
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

        let outcome = admit_by_funding(vec![c0.clone(), c1.clone()], &reader, 0)
            .await
            .expect("gate must not error");

        assert!(outcome.admitted.is_empty(), "first unfunded ⇒ reject both");
        assert_eq!(outcome.rejected.len(), 2, "both deploys rejected");
        let carol_key = delta_sigma::sig_key(&accounting::envelope_sig_single(b"carol"));
        assert!(
            outcome.debits.get(&carol_key).is_none(),
            "no admitted deploys ⇒ no debit on the pool"
        );
    }

    /// §7.4 funded / unfunded boundary: with margin `m`, a single deploy of
    /// demand Δ is admitted iff `Σ ≥ Δ + m`. Pin the exact boundary pair
    /// (Σ = Δ+m accepts; Σ = Δ+m−1 rejects).
    #[tokio::test]
    async fn funded_unfunded_boundary_at_margin() {
        // Δ = 4 (four parallel sends), margin = 2 ⇒ need Σ ≥ 6.
        let demand = 4;
        let margin = 2;

        // Σ = 6 ⇒ accepted.
        let d = cosigned(&n_sends(demand), b"dave", 0, 10);
        let mut reader_ok = MockSupplyReader::new();
        reader_ok.set(b"dave", (demand as i64) + margin);
        let accepted = admit_by_funding(vec![d.clone()], &reader_ok, margin)
            .await
            .expect("gate must not error");
        assert_eq!(accepted.admitted.len(), 1, "Σ = Δ+margin ⇒ accepted");
        assert!(accepted.rejected.is_empty());
        let dave_key = delta_sigma::sig_key(&accounting::envelope_sig_single(b"dave"));
        assert_eq!(
            accepted.debits.get(&dave_key).map(|x| x.amount),
            Some(demand as i64)
        );

        // Σ = 5 (= Δ+margin−1) ⇒ rejected.
        let mut reader_short = MockSupplyReader::new();
        reader_short.set(b"dave", (demand as i64) + margin - 1);
        let rejected = admit_by_funding(vec![d.clone()], &reader_short, margin)
            .await
            .expect("gate must not error");
        assert!(rejected.admitted.is_empty(), "Σ = Δ+margin−1 ⇒ rejected");
        assert_eq!(rejected.rejected.len(), 1);
        assert!(rejected.debits.is_empty(), "nothing admitted ⇒ no debit");
    }

    /// A malformed term (one that fails to parse) is rejected outright — the
    /// runtime would fail it too — and never grouped or debited.
    #[tokio::test]
    async fn malformed_term_is_rejected() {
        // Unbalanced braces ⇒ `source_to_adt` fails.
        let bad = cosigned("for(x <- @0){ ", b"erin", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"erin", 1_000);
        let outcome = admit_by_funding(vec![bad], &reader, 0)
            .await
            .expect("gate must not error");
        assert!(outcome.admitted.is_empty(), "malformed ⇒ not admitted");
        assert_eq!(outcome.rejected.len(), 1, "malformed ⇒ rejected");
        assert!(outcome.debits.is_empty());
    }

    /// ACTIVATION: a deploy whose supply pool is ABSENT (never provisioned by
    /// the cost-accounting economic producer) is admitted WITHOUT funding
    /// enforcement and WITHOUT a debit — even though its Δ ≫ 0 and the supply
    /// is (implicitly) 0. This is the pre-Workstream-C / non-cost-accounted
    /// path that keeps existing blocks valid. Contrast `funded_unfunded_*`,
    /// where the pool is PRESENT and the same Δ-vs-Σ shortfall rejects.
    #[tokio::test]
    async fn absent_pool_admits_without_enforcement() {
        // Δ = 5, but NO pool is set for "frank" ⇒ pool absent ⇒ admit, no debit.
        let f = cosigned(&n_sends(5), b"frank", 0, 10);
        let reader = MockSupplyReader::new(); // empty: every pool absent
        let outcome = admit_by_funding(vec![f], &reader, /* margin */ 1)
            .await
            .expect("gate must not error");
        assert_eq!(outcome.admitted.len(), 1, "absent pool ⇒ admitted unenforced");
        assert!(outcome.rejected.is_empty(), "absent pool ⇒ never rejected");
        assert!(
            outcome.debits.is_empty(),
            "absent pool ⇒ no settlement debit (not under cost-accounting)"
        );
    }

    /// A PRESENT but DRAINED pool (`Some(0)`) correctly REJECTS a further spend
    /// — the §7.7 duplicate-deploy example (tex 1677-1687): once a signer's
    /// supply is committed to 0, the next deploy sees Σ = 0 < Δ and is rejected.
    /// This is the case `read_balance_present` exists to distinguish from an
    /// absent pool (which would instead admit).
    #[tokio::test]
    async fn drained_present_pool_rejects() {
        let g = cosigned(&n_sends(3), b"grace", 0, 10);
        let mut reader = MockSupplyReader::new();
        reader.set(b"grace", 0); // PRESENT, drained to zero
        let outcome = admit_by_funding(vec![g], &reader, 0)
            .await
            .expect("gate must not error");
        assert!(
            outcome.admitted.is_empty(),
            "present drained pool (Σ=0) ⇒ Δ=3 rejected"
        );
        assert_eq!(outcome.rejected.len(), 1);
        assert!(outcome.debits.is_empty());
    }
}
