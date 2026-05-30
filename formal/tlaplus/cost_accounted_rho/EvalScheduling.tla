--------------------------- MODULE EvalScheduling -----------------------------
(****************************************************************************)
(* Models the eval loop scheduling problem that motivates the migration     *)
(* from two-phase dispatch to pure FuturesUnordered.                        *)
(*                                                                          *)
(* The key property: under the internalized cost model (fuel tokens),       *)
(* the total cost is identical regardless of which order COMM bodies        *)
(* are dispatched. This is what makes FuturesUnordered safe for consensus.  *)
(*                                                                          *)
(* We model N bodies, each requiring one fuel token to execute. The model   *)
(* explores all N! orderings and verifies that the terminal cost is the     *)
(* same in every case.                                                      *)
(*                                                                          *)
(* Contrast with the BROKEN externalized model (also modeled here),         *)
(* where produce-first vs consume-first yields different intermediate       *)
(* charges, making total cost order-dependent.                              *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    Bodies,             \* Set of body identifiers (e.g., {"b1", "b2", "b3"})
    CostPerToken        \* Nat: cost of consuming one fuel token (e.g., 1)

VARIABLES
    executed,           \* Set of Bodies that have completed execution
    totalCost,          \* Nat: running total cost under internalized model
    extCost,            \* Nat: running total cost under externalized model
    orderSoFar          \* Sequence of Bodies: execution order trace

(*--------------------------------------------------------------------------*)
(* In the internalized model, each body costs exactly CostPerToken          *)
(* (one fuel token consumed), regardless of which other bodies have         *)
(* already run.                                                             *)
(*                                                                          *)
(* In the externalized model, the cost of a body depends on whether         *)
(* it runs as a "producer" or "consumer" in its COMM interaction.           *)
(* We model this asymmetry: the first body to touch a shared channel        *)
(* pays StorageCostA; the second pays StorageCostB ≠ StorageCostA.          *)
(* This makes extCost order-dependent.                                      *)
(*--------------------------------------------------------------------------*)
CONSTANTS
    StorageCostA,       \* Cost when body stores first (e.g., produce-first)
    StorageCostB        \* Cost when body stores second (e.g., consume-first)

ASSUME StorageCostA # StorageCostB  \* The asymmetry that causes the bug
ASSUME StorageCostA \in Nat /\ StorageCostB \in Nat
ASSUME CostPerToken \in Nat

(*--------------------------------------------------------------------------*)
(* Shared channel state: tracks how many bodies have already interacted     *)
(* with the shared channel. Used only by the externalized cost model.       *)
(*--------------------------------------------------------------------------*)
VARIABLE channelTouches  \* Nat: number of bodies that have touched the channel

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho Stage B: the per-validator SUPPLY pool Σ⟦v⟧ written    *)
(* by the close-block epoch mint (CloseBlockDeploy::post_eval). We reuse     *)
(* Bodies as the validator set. [supply] is the per-validator balance; it is *)
(* written ONLY by the mint action (DR-13: Σ⟦v⟧ is reducer-unwritable, so a  *)
(* user ExecuteBody never touches it). [halted] is the "mintingHalted" set   *)
(* (Stage-C slash effect); [mintedThisEpoch] is the "mintedEpochs" guard for *)
(* the single epoch this model checks. MintAmount is the per-epoch credit.   *)
(*--------------------------------------------------------------------------*)
VARIABLE supply           \* [Bodies -> Nat]: per-validator Σ⟦v⟧ balance
VARIABLE halted           \* SUBSET Bodies: validators whose minting is halted
VARIABLE mintedThisEpoch  \* SUBSET Bodies: validators already minted this epoch

CONSTANT MintAmount       \* Nat: epochPhlogiston credited per eligible mint

ASSUME MintAmount \in Nat /\ MintAmount > 0

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho Stage D: the per-validator FEE pool F_v and the per-   *)
(* epoch fee→v CONVERSION (the economic loop, spec tex:3061-3100). Rust holds *)
(* F_v as a reducer-unwritable, content-addressed pool (distinct from Σ⟦v⟧).  *)
(* COLLECTION credits feeCollected[v] (the FeeExtract — one token per         *)
(* processed deploy). At an epoch boundary, FeeConvert moves an ELIGIBLE      *)
(* validator's WHOLE feeCollected[v] into supply[v] (Σ⟦v⟧) 1:1 and zeroes the *)
(* fee pool, recording convertedThisEpoch (the convertedEpochs idempotency    *)
(* guard). DR-4: an eligible validator with feeCollected = 0 gets NO Σ⟦v⟧     *)
(* credit (no one-sided mint). cost ≠ fee: the fee is a SEPARATE token, never  *)
(* the burned settlement debit (poolBalance).                                  *)
(*--------------------------------------------------------------------------*)
VARIABLE feeCollected      \* [Bodies -> Nat]: per-validator F_v fee pool
VARIABLE convertedThisEpoch \* SUBSET Bodies: validators already fee-converted this epoch

CONSTANT FeeAmount         \* Nat: per-COLLECTION fee credit (the flat FeeExtract)

ASSUME FeeAmount \in Nat

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho WD-D2: the per-signature ACCEPTANCE GATE + settlement  *)
(* debit at block assembly (cost-accounted-rho §7.6/§7.7;                     *)
(* casper/.../util/rholang/acceptance.rs). We reuse Bodies as the per-block   *)
(* candidate deployments sharing ONE signature supply pool Σ⟦s⟧. CanonOrder   *)
(* is the consensus-canonical deploy order (block_creator.rs:315-324) the gate *)
(* re-imposes on the nondeterministic HashSet; Demand[b] is the deployment's  *)
(* static Δ_s. The gate admits the LARGEST canonical PREFIX whose cumulative  *)
(* Δ_s fits the pool, rejecting the first non-fitting deploy and ALL after it *)
(* (§7.7 reject-both). Only admitted deploys execute (gate-before-execute),   *)
(* and the pool is debited ΣΔ_admitted exactly once at settlement             *)
(* (post = pre − ΣΔ).                                                          *)
(*--------------------------------------------------------------------------*)
CONSTANT CanonOrder       \* Seq(Bodies): canonical deploy order (a permutation)
CONSTANT Demand           \* [Bodies -> Nat]: per-deploy static Δ_s
CONSTANT PoolSupply       \* Nat: the shared signature pool's pre-state Σ⟦s⟧

ASSUME PoolSupply \in Nat
ASSUME Demand \in [Bodies -> Nat]

VARIABLE gatePhase        \* {"pregate","executing","settled"}: block-assembly phase
VARIABLE admittedLen      \* Nat: length of the admitted canonical prefix
VARIABLE poolBalance      \* Nat: current Σ⟦s⟧ balance (pre, then post-settle)
VARIABLE gateExecuted     \* SUBSET Bodies: admitted deploys that have executed

(* Cumulative demand of the first k deploys in canonical order. *)
RECURSIVE CumDemand(_)
CumDemand(k) ==
    IF k = 0 THEN 0
    ELSE Demand[CanonOrder[k]] + CumDemand(k - 1)

(* The admitted prefix LENGTH: the largest k (0..Len(CanonOrder)) whose
   cumulative demand fits PoolSupply. Computed by choosing the maximal fitting
   prefix — the spec analogue of the Rust residual walk. *)
FittingLens == { k \in 0..Len(CanonOrder) : CumDemand(k) <= PoolSupply }
AdmittedPrefixLen == CHOOSE k \in FittingLens :
                        \A j \in FittingLens : j <= k

(* The admitted deploy set: the first AdmittedPrefixLen deploys in canon order. *)
AdmittedSet(len) == { CanonOrder[i] : i \in 1..len }

vars == <<executed, totalCost, extCost, orderSoFar, channelTouches,
          supply, halted, mintedThisEpoch,
          gatePhase, admittedLen, poolBalance, gateExecuted,
          feeCollected, convertedThisEpoch>>

TypeOK ==
    /\ executed   \in SUBSET Bodies
    /\ totalCost  \in Nat
    /\ extCost    \in Nat
    /\ orderSoFar \in Seq(Bodies)
    /\ channelTouches \in Nat
    /\ supply \in [Bodies -> Nat]
    /\ halted \in SUBSET Bodies
    /\ mintedThisEpoch \in SUBSET Bodies
    /\ gatePhase \in {"pregate", "executing", "settled"}
    /\ admittedLen \in 0..Len(CanonOrder)
    /\ poolBalance \in Nat
    /\ gateExecuted \in SUBSET Bodies
    /\ feeCollected \in [Bodies -> Nat]
    /\ convertedThisEpoch \in SUBSET Bodies

Init ==
    /\ executed       = {}
    /\ totalCost      = 0
    /\ extCost        = 0
    /\ orderSoFar     = << >>
    /\ channelTouches = 0
    /\ supply          = [b \in Bodies |-> 0]
    /\ mintedThisEpoch = {}
    \* A nondeterministic initial halt set lets TLC explore halted AND
    \* unhalted validators (the slash having already halted some validators).
    /\ halted \in SUBSET Bodies
    \* WD-D2: the block starts BEFORE the gate; the pool carries its pre-state
    \* Σ⟦s⟧ = PoolSupply; nothing admitted or executed yet.
    /\ gatePhase    = "pregate"
    /\ admittedLen  = 0
    /\ poolBalance  = PoolSupply
    /\ gateExecuted = {}
    \* Stage D: no fees collected or converted yet.
    /\ feeCollected      = [b \in Bodies |-> 0]
    /\ convertedThisEpoch = {}

(*--------------------------------------------------------------------------*)
(* Action: Execute body b.                                                  *)
(*                                                                          *)
(* Internalized cost: always CostPerToken (one token consumed).             *)
(* Externalized cost: StorageCostA if first to touch channel,               *)
(*                    StorageCostB if second, etc.                           *)
(*--------------------------------------------------------------------------*)
ExecuteBody(b) ==
    /\ b \notin executed
    /\ executed'       = executed \cup {b}
    /\ totalCost'      = totalCost + CostPerToken
    /\ extCost'        = extCost + (IF channelTouches = 0
                                     THEN StorageCostA
                                     ELSE StorageCostB)
    /\ channelTouches' = channelTouches + 1
    /\ orderSoFar'     = Append(orderSoFar, b)
    \* A user reduction step NEVER touches the supply pool (DR-13) nor the
    \* WD-D2 gate state nor the Stage-D fee pool (orthogonal dynamics).
    /\ UNCHANGED <<supply, halted, mintedThisEpoch,
                   gatePhase, admittedLen, poolBalance, gateExecuted,
                   feeCollected, convertedThisEpoch>>

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho Stage B: the epoch mint. An ELIGIBLE validator        *)
(* (active is implicit here; NOT halted AND NOT already minted this epoch)   *)
(* is credited MintAmount on its Σ⟦v⟧ and recorded in mintedThisEpoch. This  *)
(* is the SOLE supply-increasing action — the model's analogue of the        *)
(* closeBlock fold + post_eval produce_balance. The eligibility guards       *)
(* mirror mint_eligible (MintingInjection.v) and the Rholang predicate.      *)
(*--------------------------------------------------------------------------*)
MintValidator(b) ==
    /\ b \notin halted
    /\ b \notin mintedThisEpoch
    /\ supply'          = [supply EXCEPT ![b] = supply[b] + MintAmount]
    /\ mintedThisEpoch' = mintedThisEpoch \cup {b}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches, halted,
                   gatePhase, admittedLen, poolBalance, gateExecuted,
                   feeCollected, convertedThisEpoch>>

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho Stage D: the per-block fee COLLECTION. The proposing    *)
(* validator's fee pool feeCollected[b] is credited FeeAmount (the FeeExtract).*)
(* Modeled as a free action on any validator (any may propose a block). This   *)
(* is the only fee-pool-increasing action; it NEVER touches supply (Σ⟦v⟧) —    *)
(* the fee reaches Σ⟦v⟧ only via FeeConvert (backed conversion). cost ≠ fee.    *)
(*--------------------------------------------------------------------------*)
CollectFee(b) ==
    \* Bound the single-epoch model: a validator holds at most ONE outstanding
    \* collection in its fee pool at a time (it accrues FeeAmount, then the epoch
    \* convert drains it back to 0 before the next collection). This keeps the
    \* state space finite while exercising the collect → convert → re-collect
    \* loop; the per-block FeeExtract is the same flat FeeAmount.
    /\ feeCollected[b] = 0
    /\ feeCollected'   = [feeCollected EXCEPT ![b] = FeeAmount]
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, gatePhase, admittedLen,
                   poolBalance, gateExecuted, convertedThisEpoch>>

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho Stage D: the per-epoch fee→v CONVERSION (the economic    *)
(* loop). An ELIGIBLE validator (NOT halted AND NOT already converted this      *)
(* epoch) moves its WHOLE feeCollected[b] into supply[b] (Σ⟦v⟧) 1:1 and zeroes  *)
(* its fee pool, recording convertedThisEpoch (the convertedEpochs idempotency  *)
(* guard, sibling of mintedThisEpoch). The Σ⟦v⟧ credit equals EXACTLY the fees   *)
(* that leave feeCollected — it is BACKED, not minted (Rocq                     *)
(* fee_convert_credit_is_backed). DR-4: a validator with feeCollected = 0 still  *)
(* records the epoch (idempotency) but credits NOTHING (no one-sided mint).     *)
(*--------------------------------------------------------------------------*)
FeeConvert(b) ==
    /\ b \notin halted
    /\ b \notin convertedThisEpoch
    /\ supply'             = [supply EXCEPT ![b] = supply[b] + feeCollected[b]]
    /\ feeCollected'       = [feeCollected EXCEPT ![b] = 0]
    /\ convertedThisEpoch' = convertedThisEpoch \cup {b}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   halted, mintedThisEpoch, gatePhase, admittedLen,
                   poolBalance, gateExecuted>>

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: the ACCEPTANCE GATE. From the "pregate" phase, compute the   *)
(* admitted canonical prefix (the largest prefix whose cumulative Δ_s fits     *)
(* PoolSupply) and transition to "executing". This is the O(AST) gate that     *)
(* runs at block assembly BEFORE any admitted deploy executes (tex 1726-1729). *)
(* The pool balance is untouched here — the DEBIT happens at settlement, after *)
(* execution.                                                                  *)
(*--------------------------------------------------------------------------*)
AcceptanceGate ==
    /\ gatePhase = "pregate"
    /\ admittedLen' = AdmittedPrefixLen
    /\ gatePhase'   = "executing"
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, poolBalance, gateExecuted,
                   feeCollected, convertedThisEpoch>>

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: execute an ADMITTED deploy. Only possible AFTER the gate     *)
(* ("executing" phase) and only for deploys in the admitted prefix — the       *)
(* gate-before-execute discipline (a rejected deploy NEVER executes). Order     *)
(* among admitted deploys is nondeterministic (the gate's funding decision is   *)
(* schedule-independent).                                                       *)
(*--------------------------------------------------------------------------*)
ExecuteAdmitted(b) ==
    /\ gatePhase = "executing"
    /\ b \in AdmittedSet(admittedLen)
    /\ b \notin gateExecuted
    /\ gateExecuted' = gateExecuted \cup {b}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, gatePhase, admittedLen,
                   poolBalance, feeCollected, convertedThisEpoch>>

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: SETTLE the block. Once every admitted deploy has executed,    *)
(* debit the pool by ΣΔ_admitted exactly once (post Σ⟦s⟧ = pre − ΣΔ). This is  *)
(* the close-block settlement debit (dual_write_supply). The admitted prefix    *)
(* fits PoolSupply by construction, so the debit never underflows.             *)
(*--------------------------------------------------------------------------*)
SettleBlock ==
    /\ gatePhase = "executing"
    /\ gateExecuted = AdmittedSet(admittedLen)
    /\ poolBalance' = PoolSupply - CumDemand(admittedLen)
    /\ gatePhase'   = "settled"
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, admittedLen, gateExecuted,
                   feeCollected, convertedThisEpoch>>

Next ==
    \/ \E b \in Bodies : ExecuteBody(b)
    \/ \E b \in Bodies : MintValidator(b)
    \/ \E b \in Bodies : CollectFee(b)
    \/ \E b \in Bodies : FeeConvert(b)
    \/ AcceptanceGate
    \/ \E b \in Bodies : ExecuteAdmitted(b)
    \/ SettleBlock

Spec == Init /\ [][Next]_vars

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Internalized cost is always deterministic: at termination, totalCost     *)
(* equals |Bodies| * CostPerToken regardless of execution order.            *)
(*--------------------------------------------------------------------------*)
AllDone == executed = Bodies

InternalizedCostDeterministic ==
    AllDone => totalCost = Cardinality(Bodies) * CostPerToken

(*--------------------------------------------------------------------------*)
(* Externalized cost is NOT deterministic when |Bodies| >= 2 and the        *)
(* bodies share a channel. We don't assert this as an invariant (it would   *)
(* fail); instead we track extCost for observational comparison.            *)
(* The ABSENCE of a similar invariant for extCost is the bug.               *)
(*--------------------------------------------------------------------------*)

(*--------------------------------------------------------------------------*)
(* Token conservation (internalized): cost increases by exactly             *)
(* CostPerToken per body, never more.                                       *)
(*--------------------------------------------------------------------------*)
InternalizedCostBounded ==
    totalCost <= Cardinality(Bodies) * CostPerToken

(*==========================================================================*)
(* Cost-Accounted Rho Stage B SUPPLY INVARIANTS                             *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* HaltedValidatorSupplyNonIncreasing: a validator that is halted (the      *)
(* "mintingHalted" set) never accrues supply — its Σ⟦v⟧ stays at its initial *)
(* 0, because the mint action skips halted validators. State invariant; the  *)
(* TLA+ analogue of halted_validator_supply_not_increased (MintingHalt.v).   *)
(*--------------------------------------------------------------------------*)
HaltedValidatorSupplyNonIncreasing ==
    \A b \in Bodies : b \in halted => supply[b] = 0

(*--------------------------------------------------------------------------*)
(* Supply is bounded by the mint accounting: a validator's Σ⟦v⟧ is 0 unless  *)
(* it was minted this epoch, in which case it is exactly MintAmount. So the  *)
(* supply is created ONLY by a mint and is precisely accountable — the state *)
(* form of "minting is the sole producer of supply" (DR-13). The TLA+        *)
(* analogue of epoch_mint crediting exactly MintAmount to an eligible        *)
(* validator and the identity otherwise.                                     *)
(*--------------------------------------------------------------------------*)
SupplyOnlyFromMint ==
    \A b \in Bodies :
        \/ /\ b \notin mintedThisEpoch
           /\ supply[b] = 0
        \/ /\ b \in mintedThisEpoch
           /\ supply[b] = MintAmount

(*--------------------------------------------------------------------------*)
(* SupplyOnlyFromMintOrBackedFeeConvert (Stage-D generalization of            *)
(* SupplyOnlyFromMint): with the fee→v conversion added, Σ⟦v⟧ is produced by   *)
(* EXACTLY TWO sources — the epoch MINT (MintAmount) and the BACKED fee        *)
(* convert (≤ the fees that were collected). So a validator's supply is        *)
(* bounded above by `(minted ? MintAmount : 0) + TotalFeesEverCollected[b]`,    *)
(* and in particular is 0 unless it was minted OR fee-converted. We pin the     *)
(* upper bound MintAmount + (FeeAmount * |Bodies|) (a loose but sound cap: at   *)
(* most that many fee tokens can have been collected then converted in this     *)
(* single-epoch model), so supply is never inflated beyond mint + collectible   *)
(* fees — "minting + backed conversion are the sole producers of supply".       *)
(*--------------------------------------------------------------------------*)
SupplyOnlyFromMintOrBackedFeeConvert ==
    \A b \in Bodies :
        \* A validator NEITHER minted NOR fee-converted has 0 supply (the two
        \* sources are the ONLY producers).
        /\ (b \notin mintedThisEpoch /\ b \notin convertedThisEpoch => supply[b] = 0)
        \* Supply is bounded above by the mint plus the BACKED converted fees
        \* (≤ all fees ever collectible in this single-epoch model) — never
        \* inflated beyond mint + collected fees.
        /\ supply[b] <= MintAmount + FeeAmount * Cardinality(Bodies)

(*--------------------------------------------------------------------------*)
(* Inv_FeeConvertConserves: the fee conversion CONSERVES the validator's        *)
(* total holding — it MOVES fees from feeCollected into supply (1:1), it never   *)
(* mints or destroys. So the combined per-validator total                       *)
(* feeCollected[b] + supply[b] is bounded above by the mint plus ALL collectible *)
(* fees (FeeAmount per validator in this single-epoch model): the convert can     *)
(* not inflate the combined holding beyond what was minted + collected. (A        *)
(* convert that drains f from feeCollected adds exactly f to supply — the total   *)
(* is unchanged by the convert itself; subsequent collections add NEW            *)
(* next-epoch fees, still within the bound.) TLA+ analogue of Rocq               *)
(* fee_collection_conserves / fee_convert_conserves_holding.                     *)
(*--------------------------------------------------------------------------*)
Inv_FeeConvertConserves ==
    \A b \in Bodies :
        feeCollected[b] + supply[b] <= MintAmount + FeeAmount * Cardinality(Bodies)

(*--------------------------------------------------------------------------*)
(* Inv_FeeConvertNotFromEmpty (DR-4): the fee convert never credits Σ⟦v⟧ from   *)
(* nothing — a HALTED validator (whose fee convert is blocked, like its mint)   *)
(* never has its fees converted, so a halted validator's supply stays 0 and its *)
(* fees, if any, are never moved into Σ⟦v⟧. Combined with                       *)
(* HaltedValidatorSupplyNonIncreasing this is the "no one-sided / unauthorized  *)
(* supply from the fee loop" guarantee.                                         *)
(*--------------------------------------------------------------------------*)
Inv_FeeConvertNotFromEmpty ==
    \A b \in Bodies : b \in halted => b \notin convertedThisEpoch

(*--------------------------------------------------------------------------*)
(* SupplyMonotoneStep (ACTION property): across any step, every validator's   *)
(* supply is non-decreasing, and it strictly increases ONLY on a MINT step    *)
(* (transition into mintedThisEpoch) OR a Stage-D fee-CONVERT step (transition *)
(* into convertedThisEpoch). A user ExecuteBody, a CollectFee (credits the fee *)
(* pool, not supply), and the gate steps all leave supply UNCHANGED. This is   *)
(* the transition form of "the epoch mint and the BACKED fee convert are the   *)
(* sole producers of supply".                                                  *)
(*--------------------------------------------------------------------------*)
SupplyMonotoneStep ==
    [][ \A b \in Bodies :
          /\ supply'[b] >= supply[b]
          /\ (supply'[b] > supply[b] =>
                \/ (b \notin mintedThisEpoch /\ b \in mintedThisEpoch')
                \/ (b \notin convertedThisEpoch /\ b \in convertedThisEpoch'))
      ]_vars

(*--------------------------------------------------------------------------*)
(* A halted validator's supply NEVER changes across any step (sticky halt).  *)
(*--------------------------------------------------------------------------*)
HaltedSupplyFrozenStep ==
    [][ \A b \in Bodies : b \in halted => supply'[b] = supply[b] ]_vars

(*==========================================================================*)
(* Cost-Accounted Rho WD-D2 ACCEPTANCE-GATE INVARIANTS                       *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* NoDoubleSpendAtBlock: the signature pool Σ⟦s⟧ never goes negative. The     *)
(* gate admits only a prefix whose cumulative Δ_s fits the pool, so the        *)
(* settlement debit (post = pre − ΣΔ) lands on a non-negative balance — no     *)
(* deploy spends supply that a previously-admitted deploy already committed    *)
(* (cost-accounted-rho §7.7 duplicate-deploy / TM-CA-153 double-spend).        *)
(* [poolBalance] is [Nat]-typed, so this is the substantive statement that the *)
(* admitted demand never exceeds the pre-state supply at settlement.           *)
(*--------------------------------------------------------------------------*)
NoDoubleSpendAtBlock ==
    gatePhase = "settled" => poolBalance = PoolSupply - CumDemand(admittedLen)
                             /\ CumDemand(admittedLen) <= PoolSupply

(*--------------------------------------------------------------------------*)
(* RejectBothOnOversubscription: the admitted set is exactly a canonical      *)
(* PREFIX — if a deploy at canonical index i is rejected (i > admittedLen),    *)
(* then EVERY deploy at a later index is also rejected (no admitted deploy      *)
(* follows a rejected one). This is the §7.7 reject-both / no-partial          *)
(* discipline. Stated as: the admitted set equals the first [admittedLen]      *)
(* deploys in canon order, AND that prefix is maximal-fitting (admitting one    *)
(* more would oversubscribe).                                                   *)
(*--------------------------------------------------------------------------*)
RejectBothOnOversubscription ==
    (gatePhase \in {"executing", "settled"}) =>
        /\ AdmittedSet(admittedLen) = { CanonOrder[i] : i \in 1..admittedLen }
        /\ (admittedLen < Len(CanonOrder) =>
              CumDemand(admittedLen + 1) > PoolSupply)

(*--------------------------------------------------------------------------*)
(* GateBeforeExecute: no deploy executes before the gate has run, and only    *)
(* admitted deploys ever execute. So [gateExecuted] is empty in "pregate" and  *)
(* is always a subset of the admitted prefix (tex 1726-1729 accept-then-       *)
(* execute; rejected deploys never execute).                                   *)
(*--------------------------------------------------------------------------*)
GateBeforeExecute ==
    /\ (gatePhase = "pregate" => gateExecuted = {})
    /\ gateExecuted \subseteq AdmittedSet(admittedLen)

(*--------------------------------------------------------------------------*)
(* SupplyConservation: at settlement, the post-state pool plus the debited     *)
(* admitted demand EQUALS the pre-state pool — the settlement neither creates  *)
(* nor destroys supply (post + ΣΔ = pre). The TLA+ analogue of the Rocq        *)
(* [settlement_conserves] / [accept_commit_conserves].                         *)
(*--------------------------------------------------------------------------*)
SupplyConservation ==
    gatePhase = "settled" => poolBalance + CumDemand(admittedLen) = PoolSupply

(*--------------------------------------------------------------------------*)
(* SupplyOnlyWrittenByMintOrFeeConvert (ACTION property): the per-validator    *)
(* supply Σ⟦v⟧ is written ONLY by a mint step OR a Stage-D fee-convert step,    *)
(* and the signature pool Σ⟦s⟧ ([poolBalance]) is written ONLY by the          *)
(* settlement step (the gate transition to "settled"). No user execution       *)
(* (ExecuteBody / ExecuteAdmitted), no gate-admission step, and no CollectFee   *)
(* (which writes the fee pool, NOT supply) mutates Σ⟦v⟧ — DR-13 (Σ is reducer-  *)
(* unwritable; the only writers are the Rust mint, the Rust fee-convert mirror, *)
(* and the Rust settlement debit).                                              *)
(*--------------------------------------------------------------------------*)
SupplyOnlyWrittenByMintOrFeeConvert ==
    [][ /\ (\A b \in Bodies : supply'[b] # supply[b] =>
              ~ UNCHANGED mintedThisEpoch \/ ~ UNCHANGED convertedThisEpoch)
        /\ (poolBalance' # poolBalance => gatePhase' = "settled" /\ gatePhase = "executing")
      ]_vars

(*--------------------------------------------------------------------------*)
(* Progress: every body eventually executes (with fairness).                *)
(*--------------------------------------------------------------------------*)
Fairness == \A b \in Bodies : WF_vars(ExecuteBody(b))
LiveSpec == Spec /\ Fairness
AllEventuallyDone == <>(executed = Bodies)

=============================================================================
