---------------------------- MODULE FullProtocol -----------------------------
(****************************************************************************)
(* Fully generalized model of the cost-accounted rho calculus protocol.     *)
(*                                                                          *)
(* Extends CompoundProtocol.tla with three generalizations:                 *)
(*                                                                          *)
(* 1. SHARED CHANNELS: Multiple processes can listen on the same signature  *)
(*    channel. When two processes compete for the same token, only one      *)
(*    wins non-deterministically, but total cost remains deterministic      *)
(*    (the total number of gate firings is fixed by the token supply).      *)
(*                                                                          *)
(* 2. ARBITRARY SIGNATURE NESTING: Processes can have nesting depth > 1.   *)
(*    A depth-k process requires k cascading Splits and (k+1) gate layers  *)
(*    to fire before its inner COMM can execute.                            *)
(*      Depth 0 (atomic):  1 gate, 0 Splits                                *)
(*      Depth 1 (compound): 2 gates, 1 Split                               *)
(*      Depth 2 (doubly-compound): 3 gates, 2 cascading Splits             *)
(*                                                                          *)
(* 3. JOIN MEDIATOR: The inverse of Split. Combines one token from each of  *)
(*    two input channels into one compound token on an output channel.      *)
(*    This is infrastructure (no cost), but reduces the token count by 1    *)
(*    (2 in -> 1 out), complementing Split (1 in -> 2 out).                *)
(*                                                                          *)
(* Key conservation law:                                                    *)
(*   TotalPending + totalCost - TotalSplitsFired + TotalJoinsFired          *)
(*     = InitialTotal                                                       *)
(*                                                                          *)
(* Reference: Rocq formalization at formal/rocq/cost_accounted_rho/         *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

(*--------------------------------------------------------------------------*)
(* CONSTANTS                                                                *)
(*--------------------------------------------------------------------------*)
CONSTANTS
    Procs,              \* Set of all process identifiers
    Channels,           \* Set of all channel identifiers
    NestingDepth,       \* Function: Procs -> Nat
                        \*   0 = atomic (1 gate, 0 splits)
                        \*   1 = compound (2 gates, 1 split)
                        \*   k = k splits, k+1 gates
    TokensPerProc,      \* Function: Procs -> Nat (tokens allocated per proc)
    SpawnedProcs,       \* Function: Procs -> SUBSET Procs
    CostPerGate,        \* Nat: cost of consuming one fuel token
    ExpectedTerminalCost, \* Nat: the expected totalCost in terminal states

    \* --- Channel layout for each process ---
    \* For a depth-k process p, there are k Split levels and k+1 gate layers.
    \* We use separate constant functions to map (proc, level/layer) to channels.

    \* Gate channels: GateChans[p] is a sequence of length (NestingDepth[p]+1)
    \* GateChans[p][j] is the channel consumed by gate layer j.
    \* Gates fire in order: layer 1 first, then 2, ..., then NestingDepth[p]+1.
    GateChans,

    \* Split input/output channels: for depth-k process p, split level i in 1..k:
    \*   SplitIn[p][i]     = channel consumed by split level i
    \*   SplitPrimOut[p][i] = primary output channel of split level i
    \*   SplitSecOut[p][i]  = secondary output channel of split level i
    \*
    \* Convention:
    \*   SplitIn[p][1] is the outermost compound channel (where initial tokens go)
    \*   SplitPrimOut[p][i] = SplitIn[p][i+1] (cascading: primary feeds next split)
    \*   SplitSecOut[p][i]  = GateChans[p][i]  (secondary feeds gate layer i)
    \*   SplitPrimOut[p][k] = GateChans[p][k+1] (last primary feeds last gate)
    \*
    \* These conventions are NOT enforced as ASSUMEs (to allow flexibility),
    \* but the model instance should follow them for correct behavior.
    SplitIn,
    SplitPrimOut,
    SplitSecOut,

    \* --- Join mediator configuration ---
    JoinProcs,          \* Subset of Procs that act as Join mediators
    JoinPrimCh,         \* Function: JoinProcs -> Channels (first input)
    JoinSecCh,          \* Function: JoinProcs -> Channels (second input)
    JoinOutCh           \* Function: JoinProcs -> Channels (output)

(*--------------------------------------------------------------------------*)
(* Derived sets                                                             *)
(*--------------------------------------------------------------------------*)
AtomicProcs   == {p \in Procs : NestingDepth[p] = 0}
CompoundProcs == {p \in Procs : NestingDepth[p] > 0}

(*--------------------------------------------------------------------------*)
(* Layer/level ranges                                                       *)
(*--------------------------------------------------------------------------*)
GateLayers(p)  == 1 .. (NestingDepth[p] + 1)
SplitLevels(p) == 1 .. NestingDepth[p]

(*--------------------------------------------------------------------------*)
(* ASSUMPTIONS                                                              *)
(*--------------------------------------------------------------------------*)
ASSUME NestingDepth  \in [Procs -> Nat]
ASSUME TokensPerProc \in [Procs -> Nat]
ASSUME CostPerGate   \in Nat
ASSUME ExpectedTerminalCost \in Nat

ASSUME GateChans \in [Procs -> Seq(Channels)]
ASSUME \A p \in Procs : Len(GateChans[p]) = NestingDepth[p] + 1

ASSUME JoinProcs \subseteq Procs

\* No channel injectivity assumption -- shared channels are allowed!

(*--------------------------------------------------------------------------*)
(* Outermost channel: where initial tokens are placed.                      *)
(*   Atomic: GateChans[p][1]                                                *)
(*   Compound: SplitIn[p][1]                                                *)
(*--------------------------------------------------------------------------*)
OutermostChan(p) ==
    IF NestingDepth[p] = 0 THEN GateChans[p][1]
    ELSE SplitIn[p][1]

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
(* Helper: maximum nesting depth across all processes.                      *)
(*--------------------------------------------------------------------------*)
MaxDepth ==
    LET MaxRec[S \in SUBSET Procs] ==
        IF S = {} THEN 0
        ELSE LET p == CHOOSE x \in S : TRUE
                 rest == MaxRec[S \ {p}]
             IN  IF NestingDepth[p] > rest THEN NestingDepth[p] ELSE rest
    IN MaxRec[Procs]

(*--------------------------------------------------------------------------*)
(* VARIABLES                                                                *)
(*--------------------------------------------------------------------------*)
VARIABLES
    tokens,         \* Function: Channels -> Nat (pending token messages)
    gateOpen,       \* Function: Procs -> Seq(BOOLEAN)
                    \*   gateOpen[p][j] = TRUE iff gate layer j has fired
    splitDone,      \* Function: CompoundProcs -> Seq(BOOLEAN)
                    \*   splitDone[p][i] = TRUE iff split level i has fired
    commDone,       \* Function: Procs -> BOOLEAN (inner COMM completed)
    spawned,        \* Function: Procs -> BOOLEAN
    joinDone,       \* Function: JoinProcs -> BOOLEAN
    totalCost,      \* Nat: running total of tokens consumed by gates
    totalJoinsFired \* Nat: count of Join mediator firings

vars == <<tokens, gateOpen, splitDone, commDone, spawned,
          joinDone, totalCost, totalJoinsFired>>

(*--------------------------------------------------------------------------*)
(* TYPE INVARIANT                                                           *)
(*--------------------------------------------------------------------------*)
TypeOK ==
    /\ tokens    \in [Channels -> Nat]
    /\ commDone  \in [Procs -> BOOLEAN]
    /\ spawned   \in [Procs -> BOOLEAN]
    /\ joinDone  \in [JoinProcs -> BOOLEAN]
    /\ totalCost \in Nat
    /\ totalJoinsFired \in Nat
    /\ \A p \in Procs :
        /\ Len(gateOpen[p]) = NestingDepth[p] + 1
        /\ \A j \in GateLayers(p) : gateOpen[p][j] \in BOOLEAN
    /\ \A p \in CompoundProcs :
        /\ Len(splitDone[p]) = NestingDepth[p]
        /\ \A i \in SplitLevels(p) : splitDone[p][i] \in BOOLEAN

(*--------------------------------------------------------------------------*)
(* INITIAL STATE                                                            *)
(*                                                                          *)
(* Tokens placed on each process's outermost channel.                       *)
(* Multiple processes may share the same outermost channel (shared          *)
(* channels) -- their tokens accumulate on that channel.                    *)
(*--------------------------------------------------------------------------*)
TopLevelProcs == {p \in Procs : \A q \in Procs : p \notin SpawnedProcs[q]}

Init ==
    /\ tokens = [ch \in Channels |->
        LET procsOnCh == {p \in Procs : OutermostChan(p) = ch}
        IN  SumOver([p \in procsOnCh |-> TokensPerProc[p]], procsOnCh)]
    /\ gateOpen = [p \in Procs |->
        [j \in GateLayers(p) |-> FALSE]]
    /\ splitDone = [p \in CompoundProcs |->
        [i \in SplitLevels(p) |-> FALSE]]
    /\ commDone = [p \in Procs |-> FALSE]
    /\ spawned  = [p \in Procs |-> p \in TopLevelProcs]
    /\ joinDone = [j \in JoinProcs |-> FALSE]
    /\ totalCost = 0
    /\ totalJoinsFired = 0

(*--------------------------------------------------------------------------*)
(* ACTION: Split at level i fires for compound process p.                   *)
(*                                                                          *)
(* Consumes 1 token from SplitIn[p][i].                                    *)
(* Produces 1 token on SplitPrimOut[p][i] and 1 on SplitSecOut[p][i].      *)
(* Net effect: +1 token in the system (1 consumed, 2 produced).             *)
(*                                                                          *)
(* Split does NOT increment totalCost (infrastructure, not fuel).           *)
(*--------------------------------------------------------------------------*)
SplitFires(p, i) ==
    /\ p \in CompoundProcs
    /\ i \in SplitLevels(p)
    /\ spawned[p] = TRUE
    /\ splitDone[p][i] = FALSE
    /\ IF i = 1 THEN TRUE ELSE splitDone[p][i - 1] = TRUE
    /\ tokens[SplitIn[p][i]] > 0
    /\ LET inCh   == SplitIn[p][i]
           primCh  == SplitPrimOut[p][i]
           secCh   == SplitSecOut[p][i]
       IN tokens' = [ch \in Channels |->
            LET delta ==
                (IF ch = inCh   THEN -1 ELSE 0) +
                (IF ch = primCh THEN  1 ELSE 0) +
                (IF ch = secCh  THEN  1 ELSE 0)
            IN tokens[ch] + delta]
    /\ splitDone' = [splitDone EXCEPT ![p][i] = TRUE]
    /\ UNCHANGED <<gateOpen, commDone, spawned, joinDone, totalCost, totalJoinsFired>>

(*--------------------------------------------------------------------------*)
(* ACTION: Gate at layer j fires for process p.                             *)
(*                                                                          *)
(* Consumes 1 token from GateChans[p][j]. Costs CostPerGate.               *)
(*                                                                          *)
(* Preconditions:                                                           *)
(*   - p is spawned, gate j not yet open                                    *)
(*   - All lower gates (1..j-1) already open (ordered firing)               *)
(*   - For atomic procs (depth 0): no split prerequisite                    *)
(*   - For compound procs (depth k > 0):                                    *)
(*     Gate layer j requires that the split producing tokens on             *)
(*     GateChans[p][j] has already fired.                                   *)
(*     By convention: GateChans[p][j] = SplitSecOut[p][j] for j <= k,      *)
(*                    GateChans[p][k+1] = SplitPrimOut[p][k].               *)
(*     So gate j (j <= k) needs split j done; gate k+1 needs split k done. *)
(*--------------------------------------------------------------------------*)
SplitPrereqForGate(p, j) ==
    IF NestingDepth[p] = 0 THEN TRUE
    ELSE IF j <= NestingDepth[p] THEN splitDone[p][j] = TRUE
    ELSE splitDone[p][NestingDepth[p]] = TRUE

GateFires(p, j) ==
    /\ j \in GateLayers(p)
    /\ spawned[p] = TRUE
    /\ gateOpen[p][j] = FALSE
    /\ \A jj \in 1 .. (j - 1) : gateOpen[p][jj] = TRUE
    /\ SplitPrereqForGate(p, j)
    /\ tokens[GateChans[p][j]] > 0
    /\ tokens'    = [tokens EXCEPT ![GateChans[p][j]] = @ - 1]
    /\ gateOpen'  = [gateOpen EXCEPT ![p][j] = TRUE]
    /\ totalCost' = totalCost + CostPerGate
    /\ UNCHANGED <<splitDone, commDone, spawned, joinDone, totalJoinsFired>>

(*--------------------------------------------------------------------------*)
(* ACTION: Inner COMM fires for process p.                                  *)
(*                                                                          *)
(* All gates must be open. COMM is free (no additional cost).               *)
(* Spawns child processes (models recursive eval).                          *)
(*--------------------------------------------------------------------------*)
AllGatesOpen(p) == \A j \in GateLayers(p) : gateOpen[p][j] = TRUE

InnerCommFires(p) ==
    /\ spawned[p] = TRUE
    /\ AllGatesOpen(p)
    /\ commDone[p] = FALSE
    /\ commDone' = [commDone EXCEPT ![p] = TRUE]
    /\ spawned'  = [q \in Procs |->
        IF q \in SpawnedProcs[p] THEN TRUE ELSE spawned[q]]
    /\ UNCHANGED <<tokens, gateOpen, splitDone, joinDone, totalCost, totalJoinsFired>>

(*--------------------------------------------------------------------------*)
(* ACTION: Join mediator fires for process j.                               *)
(*                                                                          *)
(* The inverse of Split: consumes 1 token from JoinPrimCh[j] and 1 from    *)
(* JoinSecCh[j], produces 1 token on JoinOutCh[j].                         *)
(* Net effect: -1 token in the system (2 consumed, 1 produced).             *)
(* Infrastructure only -- no cost increment.                                *)
(*--------------------------------------------------------------------------*)
JoinFires(j) ==
    /\ j \in JoinProcs
    /\ spawned[j] = TRUE
    /\ joinDone[j] = FALSE
    /\ tokens[JoinPrimCh[j]] > 0
    /\ tokens[JoinSecCh[j]] > 0
    /\ LET primCh == JoinPrimCh[j]
           secCh  == JoinSecCh[j]
           outCh  == JoinOutCh[j]
       IN tokens' = [ch \in Channels |->
            LET delta ==
                (IF ch = primCh THEN -1 ELSE 0) +
                (IF ch = secCh  THEN -1 ELSE 0) +
                (IF ch = outCh  THEN  1 ELSE 0)
            IN tokens[ch] + delta]
    /\ joinDone' = [joinDone EXCEPT ![j] = TRUE]
    /\ totalJoinsFired' = totalJoinsFired + 1
    /\ UNCHANGED <<gateOpen, splitDone, commDone, spawned, totalCost>>

(*--------------------------------------------------------------------------*)
(* NEXT-STATE RELATION                                                      *)
(*--------------------------------------------------------------------------*)
Next ==
    \/ \E p \in CompoundProcs :
        \E i \in SplitLevels(p) :
            SplitFires(p, i)
    \/ \E p \in Procs :
        \E j \in GateLayers(p) :
            GateFires(p, j)
    \/ \E p \in Procs :
        InnerCommFires(p)
    \/ \E j \in JoinProcs :
        JoinFires(j)

Fairness ==
    /\ \A p \in CompoundProcs :
        \A i \in SplitLevels(p) :
            WF_vars(SplitFires(p, i))
    /\ \A p \in Procs :
        \A j \in GateLayers(p) :
            WF_vars(GateFires(p, j))
    /\ \A p \in Procs :
        WF_vars(InnerCommFires(p))
    /\ \A j \in JoinProcs :
        WF_vars(JoinFires(j))

Spec == Init /\ [][Next]_vars /\ Fairness

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Token Conservation:                                                      *)
(*   TotalPending + totalCost - TotalSplitsFired + totalJoinsFired          *)
(*     = InitialTotal                                                       *)
(*                                                                          *)
(* Each Split adds +1 net token (1 in -> 2 out).                            *)
(* Each Join removes -1 net token (2 in -> 1 out).                          *)
(* Each gate removes -1 token (tracked by totalCost).                       *)
(*--------------------------------------------------------------------------*)
TotalPending == SumOver(tokens, Channels)
InitialTotal == SumOver(TokensPerProc, Procs)

TotalSplitsFired ==
    LET CountRec[S \in SUBSET CompoundProcs] ==
        IF S = {} THEN 0
        ELSE LET p == CHOOSE x \in S : TRUE
             IN  Cardinality({i \in SplitLevels(p) : splitDone[p][i]})
                 + CountRec[S \ {p}]
    IN CountRec[CompoundProcs]

TokenConservation ==
    TotalPending + totalCost - TotalSplitsFired + totalJoinsFired = InitialTotal

(*--------------------------------------------------------------------------*)
(* No Negative Tokens                                                       *)
(*--------------------------------------------------------------------------*)
NoNegativeTokens == \A ch \in Channels : tokens[ch] >= 0

(*--------------------------------------------------------------------------*)
(* Fuel-Gate Safety: COMM only fires after all gates open.                  *)
(*--------------------------------------------------------------------------*)
FuelGateSafety == \A p \in Procs : commDone[p] => AllGatesOpen(p)

(*--------------------------------------------------------------------------*)
(* Gate Ordering: gate j implies all gates 1..j-1 are open.                 *)
(* Also implies the prerequisite splits have fired.                         *)
(*--------------------------------------------------------------------------*)
GateOrdering ==
    \A p \in Procs :
        \A j \in GateLayers(p) :
            gateOpen[p][j] =>
                /\ \A jj \in 1 .. (j - 1) : gateOpen[p][jj]
                /\ SplitPrereqForGate(p, j)

(*--------------------------------------------------------------------------*)
(* Split Ordering: split i implies all splits 1..i-1 are done.             *)
(*--------------------------------------------------------------------------*)
SplitOrdering ==
    \A p \in CompoundProcs :
        \A i \in SplitLevels(p) :
            splitDone[p][i] =>
                \A ii \in 1 .. (i - 1) : splitDone[p][ii]

(*--------------------------------------------------------------------------*)
(* Cost Determinism: in terminal states, totalCost = ExpectedTerminalCost.  *)
(*                                                                          *)
(* With shared channels, the expected cost depends on the token supply      *)
(* reaching all processes. The model instance must specify the correct      *)
(* ExpectedTerminalCost as a constant (since it depends on the specific     *)
(* channel sharing and token allocation configuration).                     *)
(*--------------------------------------------------------------------------*)
IsTerminal ==
    /\ \A p \in Procs :
        \/ commDone[p]
        \/ spawned[p] = FALSE
        \/ (NestingDepth[p] = 0
            /\ tokens[GateChans[p][1]] = 0
            /\ gateOpen[p][1] = FALSE)
        \/ (NestingDepth[p] > 0
            /\ tokens[OutermostChan(p)] = 0
            /\ splitDone[p][1] = FALSE)
    /\ \A j \in JoinProcs :
        \/ joinDone[j]
        \/ tokens[JoinPrimCh[j]] = 0
        \/ tokens[JoinSecCh[j]] = 0

CostDeterminism == IsTerminal => totalCost = ExpectedTerminalCost

(*--------------------------------------------------------------------------*)
(* Liveness: all spawned processes with fuel eventually complete.           *)
(*--------------------------------------------------------------------------*)
AllComplete ==
    <>(\A p \in Procs :
        (spawned[p] /\ TokensPerProc[p] > 0) => commDone[p])

=============================================================================
