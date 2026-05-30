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
    MaxSeqNum,
    MintAmount,         \* Cost-Accounted Rho: epochPhlogiston per eligible mint
    EpochIndex          \* the single epoch index this model checks

VARIABLES
    \* On-chain state (PoS Rholang contract):
    bonds,              \* [Validators -> Nat]: current bond
    activeValidators,   \* SUBSET Validators: not-yet-slashed
    coopVaultBalance,   \* Nat: forfeited stake destination
    slashedSet,         \* SUBSET Validators

    \* Cost-Accounted Rho Stage B/C supply + halt state (DR-3 / DR-13):
    mintingHalted,      \* SUBSET Validators: the "mintingHalted" set (slash effect)
    supply,             \* [Validators -> Nat]: per-validator Σ⟦v⟧ supply pool
    mintedEpochs,       \* SUBSET (Validators \X {EpochIndex}): the "mintedEpochs" ledger

    \* DAG state:
    blocks,             \* [Validators -> [seq -> SUBSET BlockId]]
    invalidBlocks,      \* SUBSET BlockId
    equivocationRecords,\* SUBSET (Validators \X (0..MaxSeqNum))

    \* Pipeline state:
    pendingSlashDeploys,\* SUBSET BlockId: slash deploys queued by invalid hash
    rejectedSlashDeploys,
    recoveredSlashDeploys,
    noopSlashHashes,
    forkChoiceLatest    \* [Validators -> Nat]: latest seq considered by FC

vars == <<bonds, activeValidators, coopVaultBalance, slashedSet,
          mintingHalted, supply, mintedEpochs,
          blocks, invalidBlocks, equivocationRecords,
          pendingSlashDeploys, rejectedSlashDeploys, recoveredSlashDeploys,
          noopSlashHashes,
          forkChoiceLatest>>

ASSUME MintAmount \in Nat /\ MintAmount > 0

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
    /\ mintingHalted    \in SUBSET Validators
    /\ supply           \in [Validators -> Nat]
    /\ mintedEpochs     \in SUBSET (Validators \X {EpochIndex})
    /\ blocks           \in [Validators -> [1..MaxSeqNum -> SUBSET (1..2)]]
    /\ invalidBlocks    \in SUBSET BlockId
    /\ equivocationRecords \in SUBSET (Validators \X (0..MaxSeqNum))
    /\ pendingSlashDeploys \in SUBSET BlockId
    /\ rejectedSlashDeploys \in SUBSET BlockId
    /\ recoveredSlashDeploys \in SUBSET BlockId
    /\ noopSlashHashes \in SUBSET BlockId
    /\ forkChoiceLatest \in [Validators -> Nat]

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ bonds            = InitialBonds
    /\ activeValidators = {v \in Validators : InitialBonds[v] > 0}
    /\ coopVaultBalance = 0
    /\ slashedSet       = {}
    /\ mintingHalted    = {}
    /\ supply           = [v \in Validators |-> 0]
    /\ mintedEpochs     = {}
    /\ blocks           = [v \in Validators |->
                              [s \in 1..MaxSeqNum |-> {}]]
    /\ invalidBlocks    = {}
    /\ equivocationRecords = {}
    /\ pendingSlashDeploys = {}
    /\ rejectedSlashDeploys = {}
    /\ recoveredSlashDeploys = {}
    /\ noopSlashHashes = {}
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
                    mintingHalted, supply, mintedEpochs,
                    invalidBlocks, equivocationRecords, pendingSlashDeploys,
                    rejectedSlashDeploys, recoveredSlashDeploys, noopSlashHashes>>

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
    /\ pendingSlashDeploys' = pendingSlashDeploys \cup {<<v, s, 2>>}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    mintingHalted, supply, mintedEpochs,
                    rejectedSlashDeploys, recoveredSlashDeploys, noopSlashHashes, forkChoiceLatest>>

(****************************************************************************)
(* Action: a merge rejects a slash branch carrying invalid block h.         *)
(****************************************************************************)
ObserveRejectedSlash(h) ==
    /\ h \in invalidBlocks
    /\ h \notin rejectedSlashDeploys
    /\ rejectedSlashDeploys' = rejectedSlashDeploys \cup {h}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    mintingHalted, supply, mintedEpochs,
                    blocks, invalidBlocks, equivocationRecords,
                    pendingSlashDeploys, recoveredSlashDeploys, noopSlashHashes, forkChoiceLatest>>

RecoverRejectedSlash(h) ==
    /\ h \in rejectedSlashDeploys
    /\ h \in invalidBlocks
    /\ h \notin recoveredSlashDeploys
    /\ recoveredSlashDeploys' = recoveredSlashDeploys \cup {h}
    /\ pendingSlashDeploys' =
        IF h \in pendingSlashDeploys \/ h[1] \in slashedSet
        THEN pendingSlashDeploys
        ELSE pendingSlashDeploys \cup {h}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    mintingHalted, supply, mintedEpochs,
                    blocks, invalidBlocks, equivocationRecords,
                    rejectedSlashDeploys, noopSlashHashes, forkChoiceLatest>>

SlashSeedInput(proposer, seq, h) ==
    <<proposer, seq, h>>

(****************************************************************************)
(* Action: an honest proposer issues a SlashDeploy against invalid block h. *)
(* A duplicate slash against a zero-bond offender succeeds as a no-op.       *)
(****************************************************************************)
ExecuteSlash(h) ==
    /\ h \in pendingSlashDeploys
    /\ LET o == h[1]
       IN IF bonds[o] > 0
          THEN
            LET valBond == bonds[o]
            IN  /\ bonds' = [bonds EXCEPT ![o] = 0]
                /\ activeValidators' = activeValidators \ {o}
                /\ coopVaultBalance' = coopVaultBalance + valBond
                /\ slashedSet' = slashedSet \cup {o}
                /\ pendingSlashDeploys' =
                    {d \in pendingSlashDeploys : d[1] # o}
                /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![o] = 0]
                /\ noopSlashHashes' = noopSlashHashes
                \* Cost-Accounted Rho Stage-C slash effect (Decision 4):
                \* halt minting + zero Σ⟦v⟧ (drain @W_v is the bond zero-out
                \* above). mintingHalted is idempotent (already-halted stays).
                /\ mintingHalted' = mintingHalted \cup {o}
                /\ supply' = [supply EXCEPT ![o] = 0]
                /\ mintedEpochs' = mintedEpochs
          ELSE
            /\ bonds' = bonds
            /\ activeValidators' = activeValidators
            /\ coopVaultBalance' = coopVaultBalance
            /\ slashedSet' = slashedSet
            /\ pendingSlashDeploys' = pendingSlashDeploys \ {h}
            /\ forkChoiceLatest' = forkChoiceLatest
            /\ noopSlashHashes' = noopSlashHashes \cup {h}
            \* A duplicate slash of an already-zero-bond offender keeps the
            \* validator halted with zero supply (idempotent slash).
            /\ mintingHalted' = mintingHalted \cup {h[1]}
            /\ supply' = supply
            /\ mintedEpochs' = mintedEpochs
    /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords,
                    rejectedSlashDeploys, recoveredSlashDeploys>>

(****************************************************************************)
(* Action: the Cost-Accounted Rho Stage-B epoch mint (closeBlock fold +     *)
(* CloseBlockDeploy::post_eval). Credits MintAmount to an ELIGIBLE validator *)
(* v — active AND NOT halted AND NOT already minted this epoch — on its      *)
(* Σ⟦v⟧ supply pool, and records (v, EpochIndex) in mintedEpochs. The        *)
(* eligibility guards mirror the Rholang predicate + the Rust post_eval      *)
(* recompute + mint_eligible (MintingInjection.v). The mintedEpochs guard    *)
(* makes a duplicated / multi-parent-merged mint a NO-OP (idempotency).      *)
(****************************************************************************)
EpochMint(v) ==
    /\ v \in activeValidators
    /\ v \notin mintingHalted
    /\ <<v, EpochIndex>> \notin mintedEpochs
    /\ supply' = [supply EXCEPT ![v] = supply[v] + MintAmount]
    /\ mintedEpochs' = mintedEpochs \cup {<<v, EpochIndex>>}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    mintingHalted, blocks, invalidBlocks, equivocationRecords,
                    pendingSlashDeploys, rejectedSlashDeploys,
                    recoveredSlashDeploys, noopSlashHashes, forkChoiceLatest>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignHonest(v, s)
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignEquivocating(v, s)
    \/ \E h \in BlockId                         : ObserveRejectedSlash(h)
    \/ \E h \in BlockId                         : RecoverRejectedSlash(h)
    \/ \E h \in BlockId                         : ExecuteSlash(h)
    \/ \E v \in Validators                      : EpochMint(v)

Spec == Init /\ [][Next]_vars /\ WF_vars(\E h \in BlockId : ExecuteSlash(h))

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

Inv_PendingSlashHasEvidence ==
    pendingSlashDeploys \subseteq invalidBlocks

Inv_RecoveredSlashHasEvidence ==
    recoveredSlashDeploys \subseteq invalidBlocks

Inv_RecoveredSlashCovered ==
    \A h \in recoveredSlashDeploys :
        h \in pendingSlashDeploys \/ h[1] \in slashedSet

Inv_ZeroBondSlashNoTransfer ==
    \A h \in noopSlashHashes :
        /\ bonds[h[1]] = 0
        /\ h \notin pendingSlashDeploys
        /\ coopVaultBalance = SumInitialBonds(slashedSet)

Inv_SlashSeedInputInjectiveByHash ==
    \A p \in Validators :
      \A s \in 1..MaxSeqNum :
        \A h1 \in BlockId :
          \A h2 \in BlockId :
            SlashSeedInput(p, s, h1) = SlashSeedInput(p, s, h2) => h1 = h2

(****************************************************************************)
(* Cost-Accounted Rho Stage B/C halt-interface invariants (DR-3 / DR-13).   *)
(****************************************************************************)

\* Inv_HaltedNotMinted: a halted validator is NEVER recorded in the mint
\* ledger for the current epoch — so it never receives an epoch credit while
\* halted (the cross-epoch halt). The EpochMint eligibility guard refuses any
\* v in mintingHalted, and slash zeros the offender's supply, so a halted
\* validator's (v, EpochIndex) record can be present ONLY if it was minted
\* BEFORE being halted; this invariant asserts the stronger post-slash shape:
\* a slashed/halted validator carries no supply (its Σ⟦v⟧ was zeroed and the
\* halt blocks all further mints).
Inv_HaltedNotMinted ==
    \A v \in Validators : v \in mintingHalted => supply[v] = 0

\* Inv_NoDoubleCreditUnderMerge: a validator is credited AT MOST once per
\* epoch — its supply is bounded by a single MintAmount (the mintedEpochs
\* idempotency guard prevents a second credit, even under a duplicated /
\* multi-parent-merged epoch mint). Combined with Inv_HaltedNotMinted (a halt
\* zeros it), supply[v] is always 0 or MintAmount.
Inv_NoDoubleCreditUnderMerge ==
    \A v \in Validators : supply[v] <= MintAmount

\* Supply is created ONLY by the epoch mint: a validator's supply is non-zero
\* only if it is recorded in mintedEpochs (and not subsequently zeroed by a
\* slash). Equivalently, an unminted, unslashed validator has zero supply.
Inv_SupplyOnlyFromMint ==
    \A v \in Validators :
        (supply[v] > 0) => (<<v, EpochIndex>> \in mintedEpochs /\ v \notin mintingHalted)

(****************************************************************************)
(* Liveness: every detected equivocation eventually triggers slash.          *)
(****************************************************************************)
Live_SlashedEventually ==
    \A h \in BlockId :
        h \in pendingSlashDeploys ~> h[1] \in slashedSet

============================================================================
