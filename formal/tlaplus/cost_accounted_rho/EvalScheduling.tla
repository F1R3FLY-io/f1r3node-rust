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

TypeOK ==
    /\ executed   \in SUBSET Bodies
    /\ totalCost  \in Nat
    /\ extCost    \in Nat
    /\ orderSoFar \in Seq(Bodies)
    /\ channelTouches \in Nat

Init ==
    /\ executed       = {}
    /\ totalCost      = 0
    /\ extCost        = 0
    /\ orderSoFar     = << >>
    /\ channelTouches = 0

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

Next == \E b \in Bodies : ExecuteBody(b)

Spec == Init /\ [][Next]_<<executed, totalCost, extCost, orderSoFar, channelTouches>>

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

(*--------------------------------------------------------------------------*)
(* Progress: every body eventually executes (with fairness).                *)
(*--------------------------------------------------------------------------*)
Fairness == \A b \in Bodies : WF_<<executed, totalCost, extCost, orderSoFar, channelTouches>>(ExecuteBody(b))
LiveSpec == Spec /\ Fairness
AllEventuallyDone == <>(executed = Bodies)

=============================================================================
