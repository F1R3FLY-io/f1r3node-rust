-------------------------- MODULE CompoundProtocol ----------------------------
(****************************************************************************)
(* Extended model covering compound signatures, Split mediators, nested     *)
(* fuel gates, and recursive eval dispatch.                                 *)
(*                                                                          *)
(* This generalizes CostAccountedRho.tla from atomic-only signatures to     *)
(* the full cost-accounted rho calculus protocol:                           *)
(*   - Atomic signatures: one fuel gate, one token, one channel             *)
(*   - Compound signatures (s₁ & s₂): nested two-layer fuel gate,          *)
(*     requires Split mediator to decompose combined tokens                 *)
(*   - Recursive eval: COMM bodies can spawn sub-processes that             *)
(*     themselves require fuel                                              *)
(*                                                                          *)
(* The key property remains: total cost = total tokens consumed,            *)
(* independent of scheduling order.                                         *)
(*                                                                          *)
(* Reference: Rocq formalization at formal/rocq/cost_accounted_rho/         *)
(*   - Theorem 9.1 (Token Monotonicity)                                     *)
(*   - Lemma 9.3 (Split Fires)                                             *)
(*   - Lemma 9.4 (Compound Half Fires)                                     *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

(*--------------------------------------------------------------------------*)
(* CONSTANTS                                                                *)
(*--------------------------------------------------------------------------*)
CONSTANTS
    Procs,              \* Set of process identifiers
    Channels,           \* Set of channel identifiers
    AtomicProcs,        \* Subset of Procs with atomic (single-gate) signatures
    CompoundProcs,      \* Subset of Procs with compound (two-gate) signatures
    TokensPerProc,      \* Function: Procs -> Nat (tokens allocated per process)
    PrimaryChan,        \* Function: Procs -> Channels
                        \*   For atomic: the single signature channel
                        \*   For compound: the s₁ component channel
    SecondaryChan,      \* Function: CompoundProcs -> Channels
                        \*   The s₂ component channel (compound only)
    CompoundChan,       \* Function: CompoundProcs -> Channels
                        \*   The combined (s₁ & s₂) channel (compound only)
    SpawnedProcs,       \* Function: Procs -> SUBSET Procs
                        \*   Processes spawned by this process's COMM body
                        \*   (models recursive eval / nested Par)
    CostPerGate         \* Nat: cost of consuming one fuel token (e.g., 1)

\* Partition constraint
ASSUME AtomicProcs \cup CompoundProcs = Procs
ASSUME AtomicProcs \cap CompoundProcs = {}

\* Channel injectivity for primary channels
ASSUME PrimaryChan \in [Procs -> Channels]
ASSUME \A p1, p2 \in Procs : p1 # p2 => PrimaryChan[p1] # PrimaryChan[p2]

(*--------------------------------------------------------------------------*)
(* VARIABLES                                                                *)
(*--------------------------------------------------------------------------*)
VARIABLES
    tokens,         \* Function: Channels -> Nat (pending token messages)
    outerGateOpen,  \* Function: Procs -> BOOLEAN (outer/only fuel gate fired)
    innerGateOpen,  \* Function: CompoundProcs -> BOOLEAN (inner fuel gate fired)
    splitDone,      \* Function: CompoundProcs -> BOOLEAN (Split mediator fired)
    commDone,       \* Function: Procs -> BOOLEAN (inner COMM completed)
    spawned,        \* Function: Procs -> BOOLEAN (process has been spawned/activated)
    totalCost       \* Nat: running total of tokens consumed

vars == <<tokens, outerGateOpen, innerGateOpen, splitDone, commDone, spawned, totalCost>>

(*--------------------------------------------------------------------------*)
(* TYPE INVARIANT                                                           *)
(*--------------------------------------------------------------------------*)
TypeOK ==
    /\ tokens       \in [Channels -> Nat]
    /\ outerGateOpen \in [Procs -> BOOLEAN]
    /\ innerGateOpen \in [CompoundProcs -> BOOLEAN]
    /\ splitDone    \in [CompoundProcs -> BOOLEAN]
    /\ commDone     \in [Procs -> BOOLEAN]
    /\ spawned      \in [Procs -> BOOLEAN]
    /\ totalCost    \in Nat

(*--------------------------------------------------------------------------*)
(* Helper: sum a function f over a finite set S.                            *)
(*--------------------------------------------------------------------------*)
SumOver(f, S) ==
    LET SumRec[R \in SUBSET S] ==
        IF R = {} THEN 0
        ELSE LET x == CHOOSE y \in R : TRUE
             IN  f[x] + SumRec[R \ {x}]
    IN SumRec[S]

(*--------------------------------------------------------------------------*)
(* INITIAL STATE                                                            *)
(*                                                                          *)
(* Top-level processes are spawned; their children are not yet spawned.     *)
(* Tokens are placed on the appropriate channels:                           *)
(*   - Atomic process p: TokensPerProc[p] tokens on PrimaryChan[p]         *)
(*   - Compound process p: TokensPerProc[p] tokens on CompoundChan[p]      *)
(*     (the combined channel, awaiting Split)                               *)
(*--------------------------------------------------------------------------*)
TopLevelProcs == {p \in Procs : \A q \in Procs : p \notin SpawnedProcs[q]}

Init ==
    \* Tokens are placed on the appropriate channels for ALL processes
    \* (top-level and spawned). Atomic process tokens go on PrimaryChan;
    \* compound process tokens go on CompoundChan.
    /\ tokens = [ch \in Channels |->
        LET atomicOnCh == {p \in AtomicProcs : PrimaryChan[p] = ch}
            compoundOnCh == {p \in CompoundProcs : CompoundChan[p] = ch}
        IN  SumOver([p \in atomicOnCh |-> TokensPerProc[p]], atomicOnCh)
          + SumOver([p \in compoundOnCh |-> TokensPerProc[p]], compoundOnCh)]
    /\ outerGateOpen = [p \in Procs |-> FALSE]
    /\ innerGateOpen = [p \in CompoundProcs |-> FALSE]
    /\ splitDone     = [p \in CompoundProcs |-> FALSE]
    /\ commDone      = [p \in Procs |-> FALSE]
    /\ spawned       = [p \in Procs |-> p \in TopLevelProcs]
    /\ totalCost     = 0

(*--------------------------------------------------------------------------*)
(* ACTION: Split mediator fires for compound process p.                     *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - p is a compound process that has been spawned                        *)
(*   - Split has not yet fired for p                                        *)
(*   - There is a token on p's combined channel CompoundChan[p]             *)
(*                                                                          *)
(* Effect:                                                                  *)
(*   - Consumes one token from CompoundChan[p]                              *)
(*   - Places one token on PrimaryChan[p] (the s₁ channel)                 *)
(*   - Places one token on SecondaryChan[p] (the s₂ channel)               *)
(*   - Marks Split as done                                                  *)
(*   - Does NOT increment totalCost (Split is infrastructure, not fuel)     *)
(*                                                                          *)
(* Models Lemma 9.3 (Split Fires) from the Rocq formalization:             *)
(*   Split(s1, s2) | N[s1 & s2]!(M) ==> N[s1]!(0) | N[s2]!(deref(@M))   *)
(*--------------------------------------------------------------------------*)
SplitFires(p) ==
    /\ p \in CompoundProcs
    /\ spawned[p] = TRUE
    /\ splitDone[p] = FALSE
    /\ tokens[CompoundChan[p]] > 0
    /\ tokens' = [tokens EXCEPT
        ![CompoundChan[p]] = @ - 1,
        ![PrimaryChan[p]]  = @ + 1,
        ![SecondaryChan[p]] = @ + 1]
    /\ splitDone' = [splitDone EXCEPT ![p] = TRUE]
    /\ UNCHANGED <<outerGateOpen, innerGateOpen, commDone, spawned, totalCost>>

(*--------------------------------------------------------------------------*)
(* ACTION: Outer (or only) fuel gate fires for process p.                   *)
(*                                                                          *)
(* For atomic processes: this is the ONLY fuel gate.                        *)
(* For compound processes: this is the OUTER gate (on s₁ channel).          *)
(*   Requires Split to have already fired.                                  *)
(*                                                                          *)
(* Consumes one token from PrimaryChan[p]. Costs 1 gate.                   *)
(*--------------------------------------------------------------------------*)
OuterGateFires(p) ==
    /\ spawned[p] = TRUE
    /\ outerGateOpen[p] = FALSE
    /\ tokens[PrimaryChan[p]] > 0
    /\ (p \in AtomicProcs \/ (p \in CompoundProcs /\ splitDone[p] = TRUE))
    /\ tokens'       = [tokens EXCEPT ![PrimaryChan[p]] = @ - 1]
    /\ outerGateOpen' = [outerGateOpen EXCEPT ![p] = TRUE]
    /\ totalCost'    = totalCost + CostPerGate
    /\ UNCHANGED <<innerGateOpen, splitDone, commDone, spawned>>

(*--------------------------------------------------------------------------*)
(* ACTION: Inner fuel gate fires for compound process p.                    *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - p is compound, spawned, outer gate already open                      *)
(*   - Inner gate not yet open                                              *)
(*   - Token available on SecondaryChan[p]                                  *)
(*                                                                          *)
(* This models the second step of Lemma 9.4 (Compound Half Fires).         *)
(*--------------------------------------------------------------------------*)
InnerGateFires(p) ==
    /\ p \in CompoundProcs
    /\ spawned[p] = TRUE
    /\ outerGateOpen[p] = TRUE
    /\ innerGateOpen[p] = FALSE
    /\ tokens[SecondaryChan[p]] > 0
    /\ tokens'       = [tokens EXCEPT ![SecondaryChan[p]] = @ - 1]
    /\ innerGateOpen' = [innerGateOpen EXCEPT ![p] = TRUE]
    /\ totalCost'    = totalCost + CostPerGate
    /\ UNCHANGED <<outerGateOpen, splitDone, commDone, spawned>>

(*--------------------------------------------------------------------------*)
(* ACTION: Inner COMM fires for process p.                                  *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - All fuel gates for p are open (outer for atomic; outer+inner for     *)
(*     compound)                                                            *)
(*   - COMM not yet done                                                    *)
(*                                                                          *)
(* Effect:                                                                  *)
(*   - Marks COMM as done                                                   *)
(*   - Spawns any child processes (models recursive eval)                   *)
(*   - No additional cost (inner COMM is free)                              *)
(*--------------------------------------------------------------------------*)
AllGatesOpen(p) ==
    /\ outerGateOpen[p] = TRUE
    /\ (p \in AtomicProcs \/ (p \in CompoundProcs /\ innerGateOpen[p] = TRUE))

InnerCommFires(p) ==
    /\ spawned[p] = TRUE
    /\ AllGatesOpen(p)
    /\ commDone[p] = FALSE
    /\ commDone' = [commDone EXCEPT ![p] = TRUE]
    /\ spawned'  = [q \in Procs |->
        IF q \in SpawnedProcs[p] THEN TRUE ELSE spawned[q]]
    /\ UNCHANGED <<tokens, outerGateOpen, innerGateOpen, splitDone, totalCost>>

(*--------------------------------------------------------------------------*)
(* NEXT-STATE RELATION: any enabled action can fire.                        *)
(*--------------------------------------------------------------------------*)
Next ==
    \E p \in Procs :
        \/ SplitFires(p)
        \/ OuterGateFires(p)
        \/ InnerGateFires(p)
        \/ InnerCommFires(p)

Fairness ==
    /\ \A p \in Procs : WF_vars(OuterGateFires(p))
    /\ \A p \in Procs : WF_vars(InnerCommFires(p))
    /\ \A p \in CompoundProcs : WF_vars(SplitFires(p))
    /\ \A p \in CompoundProcs : WF_vars(InnerGateFires(p))

Spec == Init /\ [][Next]_vars /\ Fairness

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Token Conservation: tokens are redistributed by Split (conserved)        *)
(* and consumed by fuel gates (decreased). Total tokens in system           *)
(* (pending + consumed) equals the initial total.                           *)
(*--------------------------------------------------------------------------*)
TotalPending == SumOver(tokens, Channels)
InitialTotal == SumOver(TokensPerProc, Procs)

\* Count how many Splits have fired (each Split converts 1 token into 2,
\* adding 1 to the total token count in the system).
SplitsFired == Cardinality({p \in CompoundProcs : splitDone[p]})

\* Token Conservation accounting for Split redistribution:
\* Each gate firing removes 1 token (adds 1 to totalCost).
\* Each Split firing adds 1 token (1 compound -> 2 atomic).
\* So: TotalPending + totalCost - SplitsFired = InitialTotal
TokenConservation == TotalPending + totalCost - SplitsFired = InitialTotal

(*--------------------------------------------------------------------------*)
(* No Negative Tokens                                                       *)
(*--------------------------------------------------------------------------*)
NoNegativeTokens == \A ch \in Channels : tokens[ch] >= 0

(*--------------------------------------------------------------------------*)
(* Fuel-Gate Safety: COMM only fires after all gates open.                  *)
(*--------------------------------------------------------------------------*)
FuelGateSafety == \A p \in Procs : commDone[p] => AllGatesOpen(p)

(*--------------------------------------------------------------------------*)
(* Split Ordering: for compound procs, outer gate only fires after Split.   *)
(*--------------------------------------------------------------------------*)
SplitOrdering == \A p \in CompoundProcs :
    outerGateOpen[p] => splitDone[p]

(*--------------------------------------------------------------------------*)
(* Inner Gate Ordering: inner gate only fires after outer gate.             *)
(*--------------------------------------------------------------------------*)
InnerGateOrdering == \A p \in CompoundProcs :
    innerGateOpen[p] => outerGateOpen[p]

(*--------------------------------------------------------------------------*)
(* Cost Determinism: in terminal states, totalCost is determined by         *)
(* the initial configuration, not by scheduling order.                      *)
(*                                                                          *)
(* Expected cost = sum of gates that CAN fire. For each spawned process     *)
(* with available fuel:                                                     *)
(*   - Atomic: 1 gate (cost CostPerGate)                                   *)
(*   - Compound: 2 gates (cost 2 * CostPerGate)                            *)
(*--------------------------------------------------------------------------*)
GatesPerProc(p) ==
    IF p \in AtomicProcs THEN 1 ELSE 2

ExpectedCost ==
    LET CostRec[S \in SUBSET Procs] ==
        IF S = {} THEN 0
        ELSE LET p == CHOOSE x \in S : TRUE
             IN  (IF TokensPerProc[p] > 0
                  THEN GatesPerProc(p) * CostPerGate
                  ELSE 0)
                 + CostRec[S \ {p}]
    IN CostRec[Procs]

IsTerminal ==
    \A p \in Procs :
        \/ commDone[p]
        \/ spawned[p] = FALSE
        \/ (p \in AtomicProcs /\ tokens[PrimaryChan[p]] = 0 /\ outerGateOpen[p] = FALSE)
        \/ (p \in CompoundProcs /\ tokens[CompoundChan[p]] = 0 /\ splitDone[p] = FALSE)

CostDeterminism == IsTerminal => totalCost = ExpectedCost

(*--------------------------------------------------------------------------*)
(* Liveness: all spawned processes with fuel eventually complete.            *)
(*--------------------------------------------------------------------------*)
AllSpawnedComplete ==
    <>(\A p \in Procs :
        (spawned[p] /\ TokensPerProc[p] > 0) => commDone[p])

=============================================================================
