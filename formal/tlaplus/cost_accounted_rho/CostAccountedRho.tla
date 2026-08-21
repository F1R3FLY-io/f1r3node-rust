--------------------------- MODULE CostAccountedRho ---------------------------
(****************************************************************************)
(* Finite-state model of the cost-accounted rho calculus token protocol.    *)
(*                                                                          *)
(* Models:                                                                  *)
(*   - Processes with signatures acquiring fuel tokens via COMM             *)
(*   - Multiple scheduling orders (permutations of COMM events)            *)
(*   - Token conservation: total fuel never increases                       *)
(*   - Cost commutativity: total cost is scheduling-independent             *)
(*   - Fuel-gate safety: no communication without fuel                      *)
(*                                                                          *)
(* Complements the Rocq mechanization at formal/rocq/cost_accounted_rho/    *)
(* which proves these properties for the general (unbounded) case.          *)
(* This TLA+ model exhaustively checks finite instances via TLC.            *)
(*                                                                          *)
(* Reference: L. G. Meredith, "Cost-Accounted Rho Calculus: A Spectral     *)
(* Decomposition of Phlogiston," F1R3FLY.io, May 2026.                     *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    Processes,      \* Set of process identifiers (e.g., {"p1", "p2", "p3"})
    Channels,       \* Set of channel identifiers (e.g., {"ch_a", "ch_b"})
    InitialTokens,  \* Function: Processes -> Nat (initial fuel per process)
    sigChannel      \* Function: Processes -> Channels (injective)

VARIABLES
    fuel,           \* Function: Processes -> Nat (remaining fuel tokens)
    gateOpen,       \* Function: Processes -> BOOLEAN (fuel gate has fired)
    commDone,       \* Function: Processes -> BOOLEAN (inner COMM completed)
    totalConsumed,  \* Nat: running total of tokens consumed (the "cost")
    pendingTokens,  \* Function: Channels -> Nat (token messages on channels)
    schedule        \* Sequence of process IDs: order of COMM firings so far

(*--------------------------------------------------------------------------*)
(* Type invariant: all state variables have the expected shapes.            *)
(*--------------------------------------------------------------------------*)
TypeOK ==
    /\ fuel         \in [Processes -> Nat]
    /\ gateOpen     \in [Processes -> BOOLEAN]
    /\ commDone     \in [Processes -> BOOLEAN]
    /\ totalConsumed \in Nat
    /\ pendingTokens \in [Channels -> Nat]
    /\ schedule     \in Seq(Processes)

(*--------------------------------------------------------------------------*)
(* Signature-to-channel mapping.                                            *)
(* In the full calculus, N[[s]] maps signatures to channels via quotation.  *)
(* Here we abstract this: each process has a designated signature channel.  *)
(* For simplicity, process p's signature channel is modeled as an element   *)
(* of Channels. The mapping is injective (distinct processes, distinct      *)
(* channels) — reflecting N_tr_signature_strict from the Rocq proofs.       *)
(*--------------------------------------------------------------------------*)
ASSUME Cardinality(Channels) >= Cardinality(Processes)

ASSUME sigChannel \in [Processes -> Channels]
ASSUME \A p1, p2 \in Processes : p1 # p2 => sigChannel[p1] # sigChannel[p2]

(*--------------------------------------------------------------------------*)
(* Initial state.                                                           *)
(*                                                                          *)
(* Each process starts with its gate closed (fuel not yet acquired),        *)
(* its COMM not yet done, and its initial fuel allocation as pending        *)
(* token messages on its signature channel.                                 *)
(*--------------------------------------------------------------------------*)
Init ==
    /\ fuel          = [p \in Processes |-> 0]
    /\ gateOpen      = [p \in Processes |-> FALSE]
    /\ commDone      = [p \in Processes |-> FALSE]
    /\ totalConsumed = 0
    /\ pendingTokens = [ch \in Channels |->
                          LET tokProcs == {p \in Processes : sigChannel[p] = ch}
                          IN  IF tokProcs # {}
                              THEN InitialTokens[CHOOSE p \in tokProcs : TRUE]
                              ELSE 0]
    /\ schedule      = << >>

(*--------------------------------------------------------------------------*)
(* Action: Fuel gate fires for process p.                                   *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - p's gate is not yet open                                             *)
(*   - There is at least one token on p's signature channel                 *)
(*                                                                          *)
(* Effect:                                                                  *)
(*   - Consumes one token from the channel                                  *)
(*   - Opens p's gate (allowing the inner COMM)                             *)
(*   - Increments p's local fuel counter                                    *)
(*   - Increments the global consumed counter by 1                          *)
(*                                                                          *)
(* This models the COMM rule:                                               *)
(*   for(t <- N[[s]])(P | *t)  |  N[[s]]!(T[[T]])  -->  P | *(@ T[[T]])    *)
(* The fuel gate (for-comprehension) consumes the token output.             *)
(*--------------------------------------------------------------------------*)
FuelGateFires(p) ==
    /\ gateOpen[p] = FALSE
    /\ pendingTokens[sigChannel[p]] > 0
    /\ fuel'          = [fuel EXCEPT ![p] = fuel[p] + 1]
    /\ gateOpen'      = [gateOpen EXCEPT ![p] = TRUE]
    /\ pendingTokens' = [pendingTokens EXCEPT ![sigChannel[p]] = @ - 1]
    /\ totalConsumed' = totalConsumed + 1
    /\ schedule'      = Append(schedule, p)
    /\ UNCHANGED commDone

(*--------------------------------------------------------------------------*)
(* Action: Inner COMM fires for process p.                                  *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - p's gate is open (fuel acquired)                                     *)
(*   - p's inner COMM has not yet fired                                     *)
(*                                                                          *)
(* Effect:                                                                  *)
(*   - Marks the inner COMM as done                                         *)
(*   - No additional cost (the COMM itself is free in the translated model) *)
(*                                                                          *)
(* This models the inner communication:                                     *)
(*   for(y <- x) P  |  x!(Q)  -->  P{@Q/y}                                *)
(* which fires AFTER the fuel gate has opened, at zero additional cost.     *)
(*--------------------------------------------------------------------------*)
InnerCommFires(p) ==
    /\ gateOpen[p] = TRUE
    /\ commDone[p] = FALSE
    /\ commDone' = [commDone EXCEPT ![p] = TRUE]
    /\ UNCHANGED <<fuel, gateOpen, totalConsumed, pendingTokens, schedule>>

(*--------------------------------------------------------------------------*)
(* Next-state relation: any enabled process can fire its gate or COMM.      *)
(* TLC explores ALL interleavings (every possible scheduling order).        *)
(*--------------------------------------------------------------------------*)
Next ==
    \E p \in Processes :
        \/ FuelGateFires(p)
        \/ InnerCommFires(p)

(*--------------------------------------------------------------------------*)
(* Fairness: every enabled action eventually fires.                         *)
(* This ensures liveness — all processes that CAN acquire fuel DO so.       *)
(*--------------------------------------------------------------------------*)
Fairness == WF_<<fuel, gateOpen, commDone, totalConsumed, pendingTokens, schedule>>(Next)

Spec == Init /\ [][Next]_<<fuel, gateOpen, commDone, totalConsumed, pendingTokens, schedule>> /\ Fairness

(*==========================================================================*)
(* INVARIANTS — checked by TLC across all reachable states.                 *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Token Conservation: total tokens in the system never increase.           *)
(* Tokens are either pending on channels or consumed (in totalConsumed).    *)
(* The sum is constant = sum of InitialTokens.                              *)
(*--------------------------------------------------------------------------*)
TotalInitialTokens ==
    LET S == CHOOSE f \in [Processes -> Nat] : f = InitialTokens
    IN  LET vals == {S[p] : p \in Processes}
        IN  LET Sum[V \in SUBSET vals] ==
                IF V = {} THEN 0
                ELSE LET v == CHOOSE x \in V : TRUE
                     IN  v + Sum[V \ {v}]
            IN Sum[vals]

\* Simpler: just sum pending + consumed and check it equals initial
TokensInSystem ==
    LET pendingSum == LET chSet == Channels
                      IN  LET SumCh[S \in SUBSET chSet] ==
                              IF S = {} THEN 0
                              ELSE LET ch == CHOOSE c \in S : TRUE
                                   IN  pendingTokens[ch] + SumCh[S \ {ch}]
                          IN SumCh[chSet]
    IN pendingSum + totalConsumed

InitialTotal ==
    LET pSet == Processes
    IN  LET SumP[S \in SUBSET pSet] ==
            IF S = {} THEN 0
            ELSE LET p == CHOOSE x \in S : TRUE
                 IN  InitialTokens[p] + SumP[S \ {p}]
        IN SumP[pSet]

TokenConservation == TokensInSystem = InitialTotal

(*--------------------------------------------------------------------------*)
(* No Negative Fuel: no channel ever has negative pending tokens.           *)
(* (Structural invariant — should hold by construction.)                    *)
(*--------------------------------------------------------------------------*)
NoNegativeFuel ==
    \A ch \in Channels : pendingTokens[ch] >= 0

(*--------------------------------------------------------------------------*)
(* Fuel-Gate Safety: a process can only fire its inner COMM if its gate     *)
(* is open (i.e., it has consumed a fuel token).                            *)
(*--------------------------------------------------------------------------*)
FuelGateSafety ==
    \A p \in Processes : commDone[p] => gateOpen[p]

(*--------------------------------------------------------------------------*)
(* Cost Monotonicity: totalConsumed never decreases across steps.           *)
(*--------------------------------------------------------------------------*)
CostMonotone == totalConsumed' >= totalConsumed

(*==========================================================================*)
(* TEMPORAL PROPERTIES                                                      *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Liveness: every process with available fuel eventually completes.        *)
(*--------------------------------------------------------------------------*)
AllComplete ==
    <>(\A p \in Processes :
        InitialTokens[p] > 0 => commDone[p])

(*--------------------------------------------------------------------------*)
(* Cost Commutativity (the key property for consensus):                     *)
(* In every terminal state (all enabled processes done), the total cost     *)
(* is the same. Since TLC checks ALL interleavings and we verify that       *)
(* the terminal totalConsumed is identical in each, this establishes        *)
(* scheduling-independence of cost.                                         *)
(*                                                                          *)
(* We express this as an invariant on terminal states: if all processes     *)
(* that could fire have fired, then totalConsumed equals the sum of         *)
(* min(1, InitialTokens[p]) for each process (each process consumes at     *)
(* most one token in this model).                                           *)
(*--------------------------------------------------------------------------*)
ExpectedCost ==
    LET pSet == Processes
    IN  LET SumExpected[S \in SUBSET pSet] ==
            IF S = {} THEN 0
            ELSE LET p == CHOOSE x \in S : TRUE
                 IN  (IF InitialTokens[p] > 0 THEN 1 ELSE 0)
                     + SumExpected[S \ {p}]
        IN SumExpected[pSet]

IsTerminal ==
    \A p \in Processes :
        \/ commDone[p]
        \/ (gateOpen[p] = FALSE /\ pendingTokens[sigChannel[p]] = 0)

CostDeterminism ==
    IsTerminal => totalConsumed = ExpectedCost

=============================================================================
