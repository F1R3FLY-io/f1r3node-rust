--------------------------- MODULE SlashFlow ---------------------------
(****************************************************************************)
(* End-to-end slashing pipeline:                                            *)
(*   equivocation → record → propose-with-SlashDeploy →                     *)
(*   PoS bond zero-out → fork-choice exclusion                              *)
(*                                                                          *)
(* Models a small DAG of bonded validators where one of them equivocates    *)
(* and an honest proposer issues a SlashDeploy.  Verifies:                  *)
(*   - bonds[offender] = 0 after slash                                      *)
(*   - coopVaultBalance gains exactly the offender's pre-slash bond         *)
(*   - offender's latest message is filtered from fork-choice               *)
(*   - eventually slash fires given a fair proposer schedule                *)
(*                                                                          *)
(* Reference: docs/theory/slashing/slashing-verification.md §6, §7.         *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Validators,         \* Set of validator IDs
    InitialBonds,       \* [Validators -> Nat]: initial bond per validator
    MaxSeqNum

VARIABLES
    \* On-chain state (PoS Rholang contract):
    bonds,              \* [Validators -> Nat]: current bond
    activeValidators,   \* SUBSET Validators: not-yet-slashed
    coopVaultBalance,   \* Nat: forfeited stake destination
    slashedSet,         \* SUBSET Validators

    \* DAG state:
    blocks,             \* [Validators -> [seq -> SUBSET BlockId]]
    invalidBlocks,      \* SUBSET BlockId
    equivocationRecords,\* SUBSET (Validators \X (0..MaxSeqNum))

    \* Pipeline state:
    pendingSlashDeploys,\* SUBSET Validators: slash deploys queued
    forkChoiceLatest    \* [Validators -> Nat]: latest seq considered by FC

vars == <<bonds, activeValidators, coopVaultBalance, slashedSet,
          blocks, invalidBlocks, equivocationRecords,
          pendingSlashDeploys, forkChoiceLatest>>

\* Block IDs are encoded as (validator, seqNum, blockNum) triples.
BlockId == Validators \X (1..MaxSeqNum) \X (1..2)

(****************************************************************************)
(* TypeOK                                                                   *)
(****************************************************************************)
TypeOK ==
    /\ bonds            \in [Validators -> Nat]
    /\ activeValidators \in SUBSET Validators
    /\ coopVaultBalance \in Nat
    /\ slashedSet       \in SUBSET Validators
    /\ blocks           \in [Validators -> [1..MaxSeqNum -> SUBSET (1..2)]]
    /\ invalidBlocks    \in SUBSET BlockId
    /\ equivocationRecords \in SUBSET (Validators \X (0..MaxSeqNum))
    /\ pendingSlashDeploys \in SUBSET Validators
    /\ forkChoiceLatest \in [Validators -> Nat]

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ bonds            = InitialBonds
    /\ activeValidators = {v \in Validators : InitialBonds[v] > 0}
    /\ coopVaultBalance = 0
    /\ slashedSet       = {}
    /\ blocks           = [v \in Validators |->
                              [s \in 1..MaxSeqNum |-> {}]]
    /\ invalidBlocks    = {}
    /\ equivocationRecords = {}
    /\ pendingSlashDeploys = {}
    /\ forkChoiceLatest = [v \in Validators |-> 0]

(****************************************************************************)
(* Action: validator v signs an honest block at seq s, num 1.               *)
(****************************************************************************)
SignHonest(v, s) ==
    /\ v \in activeValidators
    /\ s \in 1..MaxSeqNum
    /\ blocks[v][s] = {}
    /\ blocks' = [blocks EXCEPT ![v] = [@ EXCEPT ![s] = {1}]]
    /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![v] = s]
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    invalidBlocks, equivocationRecords, pendingSlashDeploys>>

(****************************************************************************)
(* Action: validator v equivocates by signing a SECOND block at seq s.      *)
(****************************************************************************)
SignEquivocating(v, s) ==
    /\ v \in activeValidators
    /\ s \in 1..MaxSeqNum
    /\ blocks[v][s] = {1}
    /\ blocks' = [blocks EXCEPT ![v] = [@ EXCEPT ![s] = {1, 2}]]
    /\ invalidBlocks' = invalidBlocks \cup {<<v, s, 2>>}
    /\ equivocationRecords' = equivocationRecords \cup {<<v, s - 1>>}
    /\ pendingSlashDeploys' = pendingSlashDeploys \cup {v}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    forkChoiceLatest>>

(****************************************************************************)
(* Action: an honest proposer issues a SlashDeploy against an offender o,   *)
(* and the PoS contract executes successfully (no transfer failure).        *)
(****************************************************************************)
ExecuteSlash(o) ==
    /\ o \in pendingSlashDeploys
    /\ o \in activeValidators
    /\ bonds[o] > 0
    /\ LET valBond == bonds[o]
       IN  /\ bonds' = [bonds EXCEPT ![o] = 0]
           /\ activeValidators' = activeValidators \ {o}
           /\ coopVaultBalance' = coopVaultBalance + valBond
           /\ slashedSet' = slashedSet \cup {o}
           /\ pendingSlashDeploys' = pendingSlashDeploys \ {o}
           /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![o] = 0]
    /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignHonest(v, s)
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignEquivocating(v, s)
    \/ \E o \in Validators                     : ExecuteSlash(o)

Spec == Init /\ [][Next]_vars /\ WF_vars(\E o \in Validators : ExecuteSlash(o))

(****************************************************************************)
(* Invariants                                                               *)
(****************************************************************************)

\* T-7: After slash, bond is zero.
Inv_BondsZeroAfterSlash ==
    \A v \in slashedSet : bonds[v] = 0

\* Recursive sum operator over a set, weighted by InitialBonds.
RECURSIVE SumInitialBonds(_)
SumInitialBonds(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  InitialBonds[v] + SumInitialBonds(S \ {v})

\* Recursive sum operator over a set, weighted by current bonds.
RECURSIVE SumBonds(_)
SumBonds(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  bonds[v] + SumBonds(S \ {v})

\* T-8: Coop vault gains exactly the sum of pre-slash bonds of slashed validators.
\* (Equivalent to T-8 from the verification doc.)
Inv_ForfeitedToCoopVault ==
    coopVaultBalance = SumInitialBonds(slashedSet)

\* T-10: Slashed validators are excluded from fork choice.
Inv_SlashedExcludedFromFC ==
    \A v \in slashedSet : forkChoiceLatest[v] = 0

\* Slashed implies removed from active.
Inv_SlashedRemoved ==
    \A v \in slashedSet : v \notin activeValidators

\* Bonds are non-negative.
Inv_BondsNonNegative ==
    \A v \in Validators : bonds[v] >= 0

\* Total stake conservation: bonds remaining + Coop vault = total initial stake.
Inv_StakeConservation ==
    SumBonds(Validators) + coopVaultBalance = SumInitialBonds(Validators)

(****************************************************************************)
(* Liveness: every detected equivocation eventually triggers slash.          *)
(****************************************************************************)
Live_SlashedEventually ==
    \A v \in Validators :
        v \in pendingSlashDeploys ~> v \in slashedSet

============================================================================
