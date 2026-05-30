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

vars == <<executed, totalCost, extCost, orderSoFar, channelTouches,
          supply, halted, mintedThisEpoch>>

TypeOK ==
    /\ executed   \in SUBSET Bodies
    /\ totalCost  \in Nat
    /\ extCost    \in Nat
    /\ orderSoFar \in Seq(Bodies)
    /\ channelTouches \in Nat
    /\ supply \in [Bodies -> Nat]
    /\ halted \in SUBSET Bodies
    /\ mintedThisEpoch \in SUBSET Bodies

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
    \* A user reduction step NEVER touches the supply pool (DR-13).
    /\ UNCHANGED <<supply, halted, mintedThisEpoch>>

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
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches, halted>>

Next ==
    \/ \E b \in Bodies : ExecuteBody(b)
    \/ \E b \in Bodies : MintValidator(b)

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
(* SupplyOnlyIncreasedByMint (ACTION property): across any step, every       *)
(* validator's supply is non-decreasing, and it strictly increases ONLY on a *)
(* mint step (when the validator transitions into mintedThisEpoch). A user   *)
(* ExecuteBody leaves all supply UNCHANGED. This is the transition form of   *)
(* user_ca_step_does_not_increase_balance + epoch_mint being the sole        *)
(* producer.                                                                 *)
(*--------------------------------------------------------------------------*)
SupplyMonotoneStep ==
    [][ \A b \in Bodies :
          /\ supply'[b] >= supply[b]
          /\ (supply'[b] > supply[b] => b \notin mintedThisEpoch /\ b \in mintedThisEpoch')
      ]_vars

(*--------------------------------------------------------------------------*)
(* A halted validator's supply NEVER changes across any step (sticky halt).  *)
(*--------------------------------------------------------------------------*)
HaltedSupplyFrozenStep ==
    [][ \A b \in Bodies : b \in halted => supply'[b] = supply[b] ]_vars

(*--------------------------------------------------------------------------*)
(* Progress: every body eventually executes (with fairness).                *)
(*--------------------------------------------------------------------------*)
Fairness == \A b \in Bodies : WF_vars(ExecuteBody(b))
LiveSpec == Spec /\ Fairness
AllEventuallyDone == <>(executed = Bodies)

=============================================================================
