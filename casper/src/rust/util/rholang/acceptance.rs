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

/// CONSENSUS-CRITICAL (#12): the EXACT per-component (Split/Join) compound
/// settlement debit. Given each PRESENT group's cumulative admitted demand
/// `k = ΣΔ_admitted` (`demand_by_group`, keyed + channel'd by the group's
/// `SigKey`), the Split/Join `decompositions`, the RAW pre-state pool balances
/// `raw` (`Σ⟦s⟧`, absent ⇒ 0), and the resolved `channel` for every key
/// (`channels_by_key`, covering groups AND compound components), produce the
/// per-pool settlement-debit map so that, for every admitted compound group:
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
fn compute_settlement_debits(
    demand_by_group: &BTreeMap<SigKey, (Par, i64)>,
    decompositions: &[Decomposition],
    raw: &BTreeMap<SigKey, i64>,
    channels_by_key: &BTreeMap<SigKey, Par>,
) -> BTreeMap<SigKey, SettlementDebit> {
    // The LIVE remaining balance of every pool (groups + components), seeded
    // from the raw pre-state balances. Absent pools are not present in `raw`
    // (read as 0). Processed in `SigKey` order via the BTreeMap, deterministic
    // on play and replay.
    let mut residual: BTreeMap<SigKey, i64> = raw.clone();
    let read_residual =
        |residual: &BTreeMap<SigKey, i64>, key: &SigKey| -> i64 { *residual.get(key).unwrap_or(&0) };

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
    let mut decomposition_by_compound: BTreeMap<SigKey, Decomposition> = BTreeMap::new();
    for decomposition in decompositions {
        decomposition_by_compound
            .entry(decomposition.compound)
            .or_insert(*decomposition);
    }

    for (group_key, (_channel, k)) in demand_by_group {
        let k = *k;
        if k <= 0 {
            continue;
        }
        match decomposition_by_compound.get(group_key) {
            Some(decomposition) => {
                // Compound group: combined pool first, then the component pair.
                let sigma_compound = read_residual(&residual, group_key);
                let draw_compound = k.min(sigma_compound);
                let remaining = k - draw_compound;

                let sigma_left = read_residual(&residual, &decomposition.left);
                let sigma_right = read_residual(&residual, &decomposition.right);
                // ≤ min(Σ⟦s₁⟧, Σ⟦s₂⟧) by the admission bound; further bound by the
                // LIVE residual of BOTH components for cross-group safety.
                let draw_pair = remaining.min(sigma_left).min(sigma_right).max(0);

                if draw_compound > 0 {
                    *residual.entry(*group_key).or_insert(0) -= draw_compound;
                    *draw_by_key.entry(*group_key).or_insert(0) += draw_compound;
                }
                if draw_pair > 0 {
                    *residual.entry(decomposition.left).or_insert(0) -= draw_pair;
                    *residual.entry(decomposition.right).or_insert(0) -= draw_pair;
                    *draw_by_key.entry(decomposition.left).or_insert(0) += draw_pair;
                    *draw_by_key.entry(decomposition.right).or_insert(0) += draw_pair;
                }
            }
            None => {
                // Single-signature group: one debit `k` on its own pool
                // (byte-identical to the pre-#12 single-pool path — NOT residual-
                // capped; the `checked_sub` backstop in close_block_deploy.rs is
                // the hard underflow guard). Decrement the ledger so a later
                // compound sharing this pool as a component sees the reduction.
                let current = read_residual(&residual, group_key);
                residual.insert(*group_key, current.saturating_sub(k));
                *draw_by_key.entry(*group_key).or_insert(0) += k;
            }
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

    // 7. Per-group prefix admission (reject-both), accumulating Σ Δ_s per group.
    //    The cumulative admitted demand of each PRESENT group is recorded into
    //    `demand_by_group` (channel + `k = ΣΔ_admitted`); the EXACT per-pool
    //    settlement debit (combined-pool-first split + shared-component residual
    //    ledger — #12) is computed AFTER the walk by [`compute_settlement_debits`],
    //    the SINGLE shared function replay also runs (byte-identity).
    let mut demand_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
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

        // The admission residual is the EFFECTIVE supply (#12: NO artificial
        // `min(Σ_compound)` cap — a compound group is fundable up to its full
        // `effectiveΣ_compound = Σ⟦compound⟧ + min(Σ⟦s₁⟧, Σ⟦s₂⟧)`; the exact
        // multi-pool draw is settled by `compute_settlement_debits`, which keeps
        // the per-pool debit underflow-safe).
        let mut residual: i64 = *effective.get(&sig_key).unwrap_or(&0);

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
            demand_by_group.insert(sig_key, (channel, group_debit));
        }
    }

    // 8. Settle the per-pool debit EXACTLY (#12): split each admitted compound
    //    group's cumulative demand `k` combined-pool-first into `(Σ⟦compound⟧,
    //    Σ⟦s₁⟧, Σ⟦s₂⟧)`, bounding the shared component draws by a cross-group
    //    residual ledger. The SAME function (over identically-derived inputs)
    //    runs on replay ⇒ byte-identical `BTreeMap<SigKey, SettlementDebit>`.
    outcome.debits =
        compute_settlement_debits(&demand_by_group, &decompositions, &raw, &channels_by_key);

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
    // 1. Group admitted deploys by pool, summing Δ_s, AND collect the Split/Join
    //    decompositions + every distinct channel (group + compound component) —
    //    EXACTLY as `admit_by_funding` does, from the same `Cosigned` envelopes
    //    (`build_candidate` → `envelope_sig` → the same `Sig::And`), so the
    //    inputs to `compute_settlement_debits` are byte-identical to the play
    //    side. A malformed term among the ADMITTED set cannot occur for a valid
    //    block (the proposer never admits a malformed deploy), but is treated
    //    defensively as zero demand so the recompute is total.
    let mut demand_by_group: BTreeMap<SigKey, (Par, i64)> = BTreeMap::new();
    let mut decompositions: Vec<Decomposition> = Vec::new();
    let mut channels_by_key: BTreeMap<SigKey, Par> = BTreeMap::new();
    let mut group_envelopes: BTreeMap<SigKey, Sig> = BTreeMap::new();
    for cosigned in admitted {
        let envelope = accounting::envelope_sig(&cosigned);
        let candidate = build_candidate(cosigned);
        if candidate.malformed {
            continue;
        }
        channels_by_key
            .entry(candidate.sig_key)
            .or_insert_with(|| candidate.channel.clone());
        // Record the envelope once per group so the decomposition collection
        // (below) walks each compound exactly once — identical to the play-side
        // per-group representative walk.
        group_envelopes
            .entry(candidate.sig_key)
            .or_insert(envelope);
        let entry = demand_by_group
            .entry(candidate.sig_key)
            .or_insert_with(|| (candidate.channel.clone(), 0));
        entry.1 = entry.1.saturating_add(candidate.demand.known_lower_bound);
    }
    for envelope in group_envelopes.values() {
        collect_decompositions(envelope, &mut decompositions, &mut channels_by_key);
    }

    // 2. Restrict the per-group demand to PRESENT pools (absent ⇒ unenforced ⇒
    //    no debit), mirroring the play-side presence gate, AND read the RAW
    //    balance of every distinct channel (group + component) exactly once —
    //    the same `raw` map (absent ⇒ 0) the play side fed to the closure. The
    //    group's OWN-pool presence governs whether it contributes demand; the
    //    component balances feed the Split/Join draw split.
    let mut raw: BTreeMap<SigKey, i64> = BTreeMap::new();
    let mut present: std::collections::BTreeSet<SigKey> = std::collections::BTreeSet::new();
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
    let demand_by_group: BTreeMap<SigKey, (Par, i64)> = demand_by_group
        .into_iter()
        .filter(|(key, (_chan, amount))| *amount > 0 && present.contains(key))
        .collect();

    // 3. Settle the per-pool debit via the SAME shared function the play side
    //    runs (combined-pool-first split + shared-component residual ledger).
    //    Same inputs + same function ⇒ a BYTE-IDENTICAL debit map (fork safety).
    Ok(compute_settlement_debits(
        &demand_by_group,
        &decompositions,
        &raw,
        &channels_by_key,
    ))
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

        /// Set the balance of the pool a structured `Sig` (e.g. a compound
        /// `Sig::And` or one of its components) maps to. Used by the #12
        /// compound play↔replay byte-identity test to seed the compound +
        /// component pools by the SAME `supply_channel` basis the gate reads.
        fn set_sig(&mut self, sig: &Sig, balance: i64) {
            use prost::Message;
            let chan = supply::supply_channel(sig);
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
        );

        // Components each debited k; compound NOT present (draw_compound = 0).
        assert_eq!(debits.get(&fx.a_key).map(|d| d.amount), Some(k), "Σ⟦a⟧ -= k");
        assert_eq!(debits.get(&fx.b_key).map(|d| d.amount), Some(k), "Σ⟦b⟧ -= k");
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
        );

        assert_eq!(
            debits.get(&fx.compound_key).map(|d| d.amount),
            Some(1),
            "combined pool drawn first: draw_compound = min(3,1) = 1"
        );
        assert_eq!(debits.get(&fx.a_key).map(|d| d.amount), Some(2), "draw_pair = 3-1 = 2");
        assert_eq!(debits.get(&fx.b_key).map(|d| d.amount), Some(2), "draw_pair = 3-1 = 2");
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
        assert_eq!(d_compound + d_a.min(d_b), k, "draw_compound + draw_pair = k");
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
            Decomposition { compound: ab_key, left: a_key, right: b_key },
            Decomposition { compound: ac_key, left: a_key, right: c_key },
        ];

        // Each compound group demands 2 (so combined demand 4 on a's residual of 3).
        let mut demand = BTreeMap::new();
        demand.insert(ab_key, (supply::supply_channel(&ab), 2));
        demand.insert(ac_key, (supply::supply_channel(&ac), 2));

        let debits = compute_settlement_debits(&demand, &decompositions, &raw, &channels_by_key);

        let a_draw = debits.get(&a_key).map(|d| d.amount).unwrap_or(0);
        assert!(
            a_draw <= 3,
            "summed shared-component draw {} must not exceed Σ⟦a⟧ = 3",
            a_draw
        );

        // play == replay: the same function over the same inputs is deterministic.
        let debits_again =
            compute_settlement_debits(&demand, &decompositions, &raw, &channels_by_key);
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
        let debits = compute_settlement_debits(&demand, &[], &raw, &channels_by_key);

        assert_eq!(debits.len(), 1, "exactly one debit for the single group");
        let d = debits.get(&key).expect("solo pool debited");
        assert_eq!(d.amount, k, "amount = ΣΔ_admitted = k (NOT residual-capped)");
        assert_eq!(d.channel, chan, "debit keyed to the group's own pool");
    }

    /// Build a REAL 2-signer compound `Cosigned<DeployData>` over `term` (two
    /// fresh Secp256k1 keypairs both signing the canonical message hash), so the
    /// gate's `envelope_sig` derives a genuine `Sig::And` (the compound shape).
    /// Mirrors `multi_sig_fanout_bench::build_n_signers`.
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
        let envelope = accounting::envelope_sig(&compound);
        let (left, right) = match &envelope {
            Sig::And(l, r) => ((**l).clone(), (**r).clone()),
            other => panic!("expected Sig::And from a 2-signer cosigned, got {:?}", other),
        };

        // Seed: Σ⟦compound⟧ = 1, Σ⟦left⟧ = Σ⟦right⟧ = 5. effectiveΣ_compound =
        // 1 + min(5,5) = 6 ≥ Δ=2 ⇒ admitted. Combined-first split:
        // draw_compound = min(2,1) = 1, draw_pair = 1 ⇒ compound-=1, left-=1, right-=1.
        let mut reader = MockSupplyReader::new();
        reader.set_sig(&envelope, 1);
        reader.set_sig(&left, 5);
        reader.set_sig(&right, 5);

        // ---- PLAY: the gate's threaded debit map ----
        let outcome = admit_by_funding(vec![compound.clone()], &reader, /* margin */ 0)
            .await
            .expect("gate must not error");
        assert_eq!(outcome.admitted.len(), 1, "compound deploy admitted on effectiveΣ");
        assert!(outcome.rejected.is_empty());

        // ---- REPLAY: recompute from the admitted set over the SAME pre-state ----
        let recomputed = recompute_settlement_debits(vec![compound.clone()], &reader)
            .await
            .expect("recompute must not error");

        // THE BAR: byte-identical debit maps.
        assert_eq!(
            outcome.debits, recomputed,
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
        assert_eq!(outcome.debits.get(&left_key).map(|d| d.amount), Some(1), "draw_pair = 1");
        assert_eq!(outcome.debits.get(&right_key).map(|d| d.amount), Some(1), "draw_pair = 1");
    }

    /// #12.5b — compound play↔replay byte-identity in the COMPONENT-PAIR-ONLY
    /// regime (`Σ⟦compound⟧ = 0`), so the whole demand is settled from the
    /// component pools and there is NO compound debit on either path.
    #[tokio::test]
    async fn compound_debit_play_replay_identical_pair_only() {
        let compound = compound_cosigned("@0!(0) | @0!(0) | @0!(0)", 0, 10); // Δ = 3
        let envelope = accounting::envelope_sig(&compound);
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

        let outcome = admit_by_funding(vec![compound.clone()], &reader, 0)
            .await
            .expect("gate must not error");
        assert_eq!(outcome.admitted.len(), 1, "admitted on component-pair credit");

        let recomputed = recompute_settlement_debits(vec![compound.clone()], &reader)
            .await
            .expect("recompute must not error");
        assert_eq!(
            outcome.debits, recomputed,
            "play == replay byte-identical (pair-only regime)"
        );

        let compound_key = delta_sigma::sig_key(&envelope);
        let left_key = delta_sigma::sig_key(&left);
        let right_key = delta_sigma::sig_key(&right);
        assert!(
            outcome.debits.get(&compound_key).is_none(),
            "empty combined pool ⇒ NO compound debit"
        );
        assert_eq!(outcome.debits.get(&left_key).map(|d| d.amount), Some(3), "Σ⟦left⟧ -= 3");
        assert_eq!(outcome.debits.get(&right_key).map(|d| d.amount), Some(3), "Σ⟦right⟧ -= 3");
    }
}
