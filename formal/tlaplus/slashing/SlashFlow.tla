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
    coopVaultBalance,   \* Nat: coop multisig vault (grows ONLY on Guilty redeem)
    slashedSet,         \* SUBSET Validators

    \* Cost-Accounted Rho Stage B/C supply + halt state (DR-3 / DR-13):
    mintingHalted,      \* SUBSET Validators: the "mintingHalted" set (slash effect)
    quarantinedStake,   \* [Validators -> Nat]: the "quarantinedStake" earmark
                        \*   (per-offender pre-slash bond; 0 if not quarantined)
    burnedStake,        \* Nat: stake destroyed by a Burned redemption (the REV
                        \*   becomes posVault protocol surplus). Tracked so the
                        \*   conservation invariant is exact across all outcomes.
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
          mintingHalted, quarantinedStake, burnedStake, supply, mintedEpochs,
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
    /\ quarantinedStake \in [Validators -> Nat]
    /\ burnedStake      \in Nat
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
    /\ quarantinedStake = [v \in Validators |-> 0]
    /\ burnedStake      = 0
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
                    mintingHalted, quarantinedStake, burnedStake, supply, mintedEpochs,
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
                    mintingHalted, quarantinedStake, burnedStake, supply, mintedEpochs,
                    rejectedSlashDeploys, recoveredSlashDeploys, noopSlashHashes, forkChoiceLatest>>

(****************************************************************************)
(* Action: a merge rejects a slash branch carrying invalid block h.         *)
(****************************************************************************)
ObserveRejectedSlash(h) ==
    /\ h \in invalidBlocks
    /\ h \notin rejectedSlashDeploys
    /\ rejectedSlashDeploys' = rejectedSlashDeploys \cup {h}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, slashedSet,
                    mintingHalted, quarantinedStake, burnedStake, supply, mintedEpochs,
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
                    mintingHalted, quarantinedStake, burnedStake, supply, mintedEpochs,
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
                \* Cost-Accounted Rho Stage-C TWO-EFFECT slash (Decision 4):
                \* the legacy coop transfer is GONE — coop is UNCHANGED here;
                \* the pre-slash bond is EARMARKED on the per-offender
                \* quarantine pending redeemSlashed adjudication (coop grows
                \* ONLY in the Guilty redemption branch).
                /\ coopVaultBalance' = coopVaultBalance
                /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = valBond]
                /\ burnedStake' = burnedStake
                /\ slashedSet' = slashedSet \cup {o}
                /\ pendingSlashDeploys' =
                    {d \in pendingSlashDeploys : d[1] # o}
                /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![o] = 0]
                /\ noopSlashHashes' = noopSlashHashes
                \* The other two slash effects: halt minting + zero Σ⟦v⟧ (the
                \* @W_v drain is the bond zero-out above). mintingHalted is
                \* idempotent (already-halted stays).
                /\ mintingHalted' = mintingHalted \cup {o}
                /\ supply' = [supply EXCEPT ![o] = 0]
                /\ mintedEpochs' = mintedEpochs
          ELSE
            /\ bonds' = bonds
            /\ activeValidators' = activeValidators
            /\ coopVaultBalance' = coopVaultBalance
            /\ quarantinedStake' = quarantinedStake
            /\ burnedStake' = burnedStake
            /\ slashedSet' = slashedSet
            /\ pendingSlashDeploys' = pendingSlashDeploys \ {h}
            /\ forkChoiceLatest' = forkChoiceLatest
            /\ noopSlashHashes' = noopSlashHashes \cup {h}
            \* A duplicate slash of an already-zero-bond offender keeps the
            \* validator halted with zero supply (idempotent slash); the
            \* quarantine earmark is untouched (first slash recorded it).
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
                    mintingHalted, quarantinedStake, burnedStake, blocks,
                    invalidBlocks, equivocationRecords,
                    pendingSlashDeploys, rejectedSlashDeploys,
                    recoveredSlashDeploys, noopSlashHashes, forkChoiceLatest>>

(****************************************************************************)
(* Action: the Cost-Accounted Rho Stage-C validator redemption              *)
(* (redeemSlashed; DR-3/DR-7/DR-12). Governance-triggered, PoS-multisig-     *)
(* quorum gated (the `authorized` verdict is the Rust DR-12 platform         *)
(* obligation — here a redemption only fires for a quarantined offender, and *)
(* the model treats authorization as a fired-only-when-authorized guard).    *)
(* REQUIRES an active quarantine record (quarantinedStake[o] > 0). Three     *)
(* outcomes:                                                                 *)
(*   "Vindicated" — restore the full quarantined bond, UN-HALT, clear        *)
(*                  quarantine + stale mintedEpochs; coop unchanged.         *)
(*   "Guilty"     — move the quarantined bond to coopMultiVault (the ONLY    *)
(*                  coop-growth path; modeled at the maximal penalty = bond, *)
(*                  remainder 0), UN-HALT, clear quarantine + stale epochs.  *)
(*   "Burned"     — destroy the quarantined stake (bond stays 0); STAYS      *)
(*                  halted; clear ONLY the quarantine record.                *)
(* Redemption writes NEITHER Σ⟦v⟧ NOR @W_v directly — a restored validator   *)
(* is re-funded by the normal next-epoch mint (so clearing stale mintedEpochs*)
(* re-enables EpochMint for v). coopVaultBalance grows ONLY here (Guilty).   *)
(****************************************************************************)
RedeemOutcomes == {"Vindicated", "Guilty", "Burned"}

ClearStaleEpochs(o) == {e \in mintedEpochs : e[1] # o}

\* Drop every slash-pipeline hash targeting offender o (used when a redemption
\* REVERSES a slash — Vindicated/Guilty — so the merge-recovery bookkeeping does
\* not dangle on a no-longer-slashed validator). A name-keyed filter on h[1].
DropSlashArtifacts(S, o) == {h \in S : h[1] # o}

Redeem(o, outcome) ==
    /\ outcome \in RedeemOutcomes
    /\ quarantinedStake[o] > 0                       \* requires a quarantine
    /\ LET valBond == quarantinedStake[o]
       IN CASE outcome = "Vindicated" ->
                 /\ bonds' = [bonds EXCEPT ![o] = valBond]
                 /\ activeValidators' = activeValidators \cup {o}
                 /\ coopVaultBalance' = coopVaultBalance
                 /\ mintingHalted' = mintingHalted \ {o}
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake
                 /\ slashedSet' = slashedSet \ {o}
                 /\ mintedEpochs' = ClearStaleEpochs(o)
                 \* The slash is reversed ⇒ vacate o's pipeline bookkeeping.
                 /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
                 /\ rejectedSlashDeploys' = DropSlashArtifacts(rejectedSlashDeploys, o)
                 /\ recoveredSlashDeploys' = DropSlashArtifacts(recoveredSlashDeploys, o)
                 /\ noopSlashHashes' = DropSlashArtifacts(noopSlashHashes, o)
            [] outcome = "Guilty" ->
                 \* Partial penalty: a PROPORTION (modeled as half, so the
                 \* remainder is positive for any positive bond) goes to the coop
                 \* multisig vault (the ONLY coop-growth path), and the REMAINDER
                 \* is restored to active stake. The validator is un-halted and
                 \* re-activated WITH a positive bond. Conservation: the
                 \* quarantined bond splits exactly into coop-penalty + restored
                 \* remainder. (A full-forfeit Guilty would coincide with Burned
                 \* on the bond; the partial case is the representative one and
                 \* keeps the re-bonded validator a genuine active validator.)
                 /\ LET penalty == valBond \div 2
                        remainder == valBond - penalty
                    IN /\ bonds' = [bonds EXCEPT ![o] = remainder]
                       /\ coopVaultBalance' = coopVaultBalance + penalty
                 /\ activeValidators' = activeValidators \cup {o}
                 /\ mintingHalted' = mintingHalted \ {o}
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake
                 /\ slashedSet' = slashedSet \ {o}
                 /\ mintedEpochs' = ClearStaleEpochs(o)
                 \* The slash is (partially) reversed ⇒ vacate o's bookkeeping.
                 /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
                 /\ rejectedSlashDeploys' = DropSlashArtifacts(rejectedSlashDeploys, o)
                 /\ recoveredSlashDeploys' = DropSlashArtifacts(recoveredSlashDeploys, o)
                 /\ noopSlashHashes' = DropSlashArtifacts(noopSlashHashes, o)
            [] outcome = "Burned" ->
                 \* Destroy the quarantined stake; STAY halted; clear ONLY the
                 \* quarantine record. (Stake leaves the tracked set — coop
                 \* unchanged, bond stays 0, still halted/slashed.) The validator
                 \* REMAINS slashed, so the slash-pipeline bookkeeping is kept.
                 /\ bonds' = bonds
                 /\ activeValidators' = activeValidators
                 /\ coopVaultBalance' = coopVaultBalance
                 /\ mintingHalted' = mintingHalted
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake + valBond
                 /\ slashedSet' = slashedSet
                 /\ mintedEpochs' = mintedEpochs
                 /\ pendingSlashDeploys' = pendingSlashDeploys
                 /\ rejectedSlashDeploys' = rejectedSlashDeploys
                 /\ recoveredSlashDeploys' = recoveredSlashDeploys
                 /\ noopSlashHashes' = noopSlashHashes
    /\ supply' = supply
    /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords, forkChoiceLatest>>

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
    \/ \E o \in Validators, oc \in RedeemOutcomes : Redeem(o, oc)

\* Fairness: every queued slash deploy is eventually serviced by an honest
\* proposer. We use PER-HASH weak fairness on ExecuteSlash (rather than a single
\* existential WF) because the Stage-C redemption lifecycle breaks the
\* monotonicity the existential form relied on: a Redeem can move an offender OUT
\* of slashedSet, so a perpetual slash↔redeem cycle on ONE offender could
\* otherwise starve a DIFFERENT offender's still-pending slash under the weaker
\* existential fairness. Per-hash WF guarantees each continuously-enabled
\* ExecuteSlash(h) eventually fires, so every pending slash reaches its target.
Spec == Init /\ [][Next]_vars /\ \A h \in BlockId : WF_vars(ExecuteSlash(h))

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

\* Recursive sum operator over a set, weighted by current quarantinedStake.
RECURSIVE SumQuarantined(_)
SumQuarantined(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN  quarantinedStake[v] + SumQuarantined(S \ {v})

\* T-8C (replaces the legacy Inv_ForfeitedToCoopVault): the Stage-C two-effect
\* slash does NOT transfer to the coop vault — it EARMARKS the offender's bond
\* on the per-offender quarantine. Every currently-quarantined validator (a
\* slashed validator that has not yet been redeemed) holds a positive earmark
\* and a zeroed bond; conversely the coop vault is NOT credited by the slash
\* itself. (Coop growth is exercised only by the Guilty Redeem branch.)
Inv_StakeInQuarantineAfterSlash ==
    \A v \in Validators :
        (quarantinedStake[v] > 0) =>
            /\ v \in slashedSet
            /\ bonds[v] = 0
            /\ v \in mintingHalted

\* T-10: Slashed validators are excluded from fork choice.
Inv_SlashedExcludedFromFC ==
    \A v \in slashedSet : forkChoiceLatest[v] = 0

\* Slashed implies removed from active.
Inv_SlashedRemoved ==
    \A v \in slashedSet : v \notin activeValidators

\* Bonds are non-negative.
Inv_BondsNonNegative ==
    \A v \in Validators : bonds[v] >= 0

\* Total stake conservation (quarantine-inclusive, Stage-C): the initial stake
\* is fully accounted across the four buckets it can occupy — currently bonded,
\* the coop vault (Guilty penalties), the per-offender quarantine (slashed but
\* not yet adjudicated), and burned (destroyed by a Burned redemption, now
\* posVault protocol surplus). A slash moves bond → quarantine; a Vindicated
\* redeem moves quarantine → bond; a Guilty redeem moves quarantine → coop; a
\* Burned redeem moves quarantine → burned. Every transition conserves the sum.
Inv_StakeConservation ==
    SumBonds(Validators) + coopVaultBalance
      + SumQuarantined(Validators) + burnedStake
    = SumInitialBonds(Validators)

Inv_PendingSlashHasEvidence ==
    pendingSlashDeploys \subseteq invalidBlocks

Inv_RecoveredSlashHasEvidence ==
    recoveredSlashDeploys \subseteq invalidBlocks

\* A recovered (merge-rejected-then-re-issued) slash is never "lost": its slash
\* effect is guaranteed to be applied or already-applied. Coverage holds iff the
\* hash is still actionable (pending), its target is currently slashed, OR the
\* target's bond is ALREADY 0 — i.e. the slash effect is already realized (the
\* validator was slashed and possibly subsequently REDEEMED to a 0-remainder
\* bond, or the recovered slash hit the idempotent no-op branch). The bond-0
\* disjunct is SOUND, not a weakening: a SlashDeploy against a bond-0 offender
\* is a no-op by design (idempotency), so coverage of such a hash is vacuous.
\* This disjunct is what admits the Stage-C redemption lifecycle (a Guilty/
\* Vindicated redeem moves the offender out of slashedSet) without losing the
\* recovery guarantee. (Pre-redemption, the first two disjuncts sufficed.)
Inv_RecoveredSlashCovered ==
    \A h \in recoveredSlashDeploys :
        \/ h \in pendingSlashDeploys
        \/ h[1] \in slashedSet
        \/ bonds[h[1]] = 0

\* A no-op (already-zero-bond) slash neither re-earmarks quarantine nor moves
\* funds: the offender's bond stays 0 and the deploy leaves the pending set.
\* (Coop is never credited by a slash at all under the two-effect model, so the
\* legacy "coop = SumInitialBonds(slashedSet)" clause is dropped.)
Inv_ZeroBondSlashNoTransfer ==
    \A h \in noopSlashHashes :
        /\ bonds[h[1]] = 0
        /\ h \notin pendingSlashDeploys

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
