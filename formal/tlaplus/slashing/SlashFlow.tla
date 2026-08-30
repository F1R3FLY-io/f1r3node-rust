--------------------------- MODULE SlashFlow ---------------------------
(****************************************************************************)
(* End-to-end slashing pipeline:                                            *)
(*   equivocation → canonical evidence scan → propose-with-SlashDeploy →   *)
(*   PoS bond zero-out → fork-choice exclusion                              *)
(*                                                                          *)
(* Models a small DAG of bonded validators where one of them equivocates    *)
(* and an honest proposer issues a SlashDeploy.  Verifies:                  *)
(*   - bonds[offender] = 0 after slash                                      *)
(*   - the offender's pre-slash bond is quarantined for adjudication        *)
(*   - offender's latest message is filtered from fork-choice               *)
(*   - eventually slash fires given a fair proposer schedule                *)
(*                                                                          *)
(* Reference: docs/casper/theory/slashing/slashing-verification.md §6, §7.         *)
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
    coopVaultBalance,   \* Nat: stake portion of the canonical coop REV vault
    coopFuelBalance,    \* Nat: execution-balance portion of the same coop vault
    slashedSet,         \* SUBSET Validators

    \* Paper supply notation mapped to native SystemVault custody (DR-3 / DR-13):
    mintingHalted,      \* SUBSET Validators: the "mintingHalted" set (slash effect)
    quarantinedStake,   \* [Validators -> Nat]: the "quarantinedStake" earmark
                        \*   (per-offender pre-slash bond; 0 if not quarantined)
    burnedStake,        \* Nat: stake destroyed by a Burned redemption (the REV
                        \*   is removed from the PoS custody purse).
    supply,             \* [Validators -> Nat]: canonical SystemVault balance
    quarantinedFuel,    \* [Validators -> Nat]: slashed SystemVault balance
    burnedFuel,         \* Nat: quarantined execution balance destroyed on burn
    protocolMinted,     \* [Validators -> Nat]: cumulative authorized epoch mint
    mintedEpochs,       \* SUBSET (Validators \X {EpochIndex}): the "mintedEpochs" ledger

    \* DAG state:
    blocks,             \* [Validators -> [seq -> SUBSET BlockId]]
    invalidBlocks,      \* SUBSET BlockId
    equivocationRecords,\* SUBSET (Validators \X (0..MaxSeqNum))

    \* Pipeline state:
    pendingSlashDeploys,\* SUBSET BlockId: one authorized candidate per offender
    forkChoiceLatest    \* [Validators -> Nat]: latest seq considered by FC

vars == <<bonds, activeValidators, coopVaultBalance, coopFuelBalance, slashedSet,
          mintingHalted, quarantinedStake, burnedStake, supply,
          quarantinedFuel, burnedFuel, protocolMinted, mintedEpochs,
          blocks, invalidBlocks, equivocationRecords,
          pendingSlashDeploys,
          forkChoiceLatest>>

ASSUME MintAmountType == MintAmount \in Nat /\ MintAmount > 0

\* Constant-typing assumptions for InitialBonds and MaxSeqNum. These make
\* explicit the types already DOCUMENTED on the CONSTANT declarations above
\* (InitialBonds : "[Validators -> Nat]"; MaxSeqNum : a sequence-number bound,
\* used as 1..MaxSeqNum / 0..MaxSeqNum) and supplied by every model instance
\* (e.g. MC_InitialBonds, MC_MaxSeqNum). They are the well-formedness
\* preconditions under which TypeOK is an inductive invariant — Init sets
\* bonds = InitialBonds, so bonds \in [Validators -> Nat] holds iff InitialBonds
\* does; and SignEquivocating records <<v, s-1>> with s \in 1..MaxSeqNum, so
\* s-1 \in 0..MaxSeqNum needs MaxSeqNum \in Nat. They are exactly analogous to
\* the MintAmount typing assumption directly above. These are constant-typing
\* hypotheses, NOT property-altering axioms: each constrains only a model
\* parameter, never the reachable state space.
ASSUME InitialBondsType == InitialBonds \in [Validators -> Nat]
ASSUME MaxSeqNumType    == MaxSeqNum \in Nat

\* Block IDs are encoded as (validator, seqNum, blockNum) triples.
BlockId == Validators \X (1..MaxSeqNum) \X (1..2)

(****************************************************************************)
(* TypeOK                                                                   *)
(****************************************************************************)
TypeOK ==
    /\ bonds            \in [Validators -> Nat]
    /\ activeValidators \in SUBSET Validators
    /\ coopVaultBalance \in Nat
    /\ coopFuelBalance  \in Nat
    /\ slashedSet       \in SUBSET Validators
    /\ mintingHalted    \in SUBSET Validators
    /\ quarantinedStake \in [Validators -> Nat]
    /\ burnedStake      \in Nat
    /\ supply           \in [Validators -> Nat]
    /\ quarantinedFuel  \in [Validators -> Nat]
    /\ burnedFuel       \in Nat
    /\ protocolMinted   \in [Validators -> Nat]
    /\ mintedEpochs     \in SUBSET (Validators \X {EpochIndex})
    /\ blocks           \in [Validators -> [1..MaxSeqNum -> SUBSET (1..2)]]
    /\ invalidBlocks    \in SUBSET BlockId
    /\ equivocationRecords \in SUBSET (Validators \X (0..MaxSeqNum))
    /\ pendingSlashDeploys \in SUBSET BlockId
    /\ forkChoiceLatest \in [Validators -> Nat]

(****************************************************************************)
(* Init                                                                     *)
(****************************************************************************)
Init ==
    /\ bonds            = InitialBonds
    /\ activeValidators = {v \in Validators : InitialBonds[v] > 0}
    /\ coopVaultBalance = 0
    /\ coopFuelBalance  = 0
    /\ slashedSet       = {}
    /\ mintingHalted    = {}
    /\ quarantinedStake = [v \in Validators |-> 0]
    /\ burnedStake      = 0
    /\ supply           = [v \in Validators |-> 0]
    /\ quarantinedFuel  = [v \in Validators |-> 0]
    /\ burnedFuel       = 0
    /\ protocolMinted   = [v \in Validators |-> 0]
    /\ mintedEpochs     = {}
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
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, coopFuelBalance, slashedSet,
                    mintingHalted, quarantinedStake, burnedStake, supply,
                    quarantinedFuel, burnedFuel, protocolMinted, mintedEpochs,
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
    /\ pendingSlashDeploys' =
        IF \E h \in pendingSlashDeploys : h[1] = v
        THEN pendingSlashDeploys
        ELSE pendingSlashDeploys \cup {<<v, s, 2>>}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, coopFuelBalance, slashedSet,
                    mintingHalted, quarantinedStake, burnedStake, supply,
                    quarantinedFuel, burnedFuel, protocolMinted, mintedEpochs,
                    forkChoiceLatest>>

SlashSeedInput(proposer, seq, h) ==
    <<proposer, seq, h>>

(****************************************************************************)
(* Action: an honest proposer issues a SlashDeploy against invalid block h. *)
(****************************************************************************)
ExecuteSlash(h) ==
    /\ h \in pendingSlashDeploys
    /\ bonds[h[1]] > 0
    /\ LET o == h[1]
       IN LET valBond == bonds[o]
          IN  /\ bonds' = [bonds EXCEPT ![o] = 0]
                /\ activeValidators' = activeValidators \ {o}
                \* Cost-Accounted Rho Stage-C TWO-EFFECT slash (Decision 4):
                \* the legacy coop transfer is GONE — coop is UNCHANGED here;
                \* the pre-slash bond is EARMARKED on the per-offender
                \* quarantine pending redeemSlashed adjudication (coop grows
                \* ONLY in the Guilty redemption branch).
                /\ coopVaultBalance' = coopVaultBalance
                /\ coopFuelBalance' = coopFuelBalance
                /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = valBond]
                /\ burnedStake' = burnedStake
                /\ slashedSet' = slashedSet \cup {o}
                /\ pendingSlashDeploys' =
                    {d \in pendingSlashDeploys : d[1] # o}
                /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![o] = 0]
                \* Slash halts minting and atomically moves available canonical
                \* SystemVault custody into the offender's quarantine.
                /\ mintingHalted' = mintingHalted \cup {o}
                /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = supply[o]]
                /\ supply' = [supply EXCEPT ![o] = 0]
                /\ burnedFuel' = burnedFuel
                /\ protocolMinted' = protocolMinted
                /\ mintedEpochs' = mintedEpochs
    /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords>>

(****************************************************************************)
(* Authenticated PoS epoch mint. It credits MintAmount directly to an         *)
(* eligible validator's canonical SystemVault and records (v, EpochIndex).    *)
(* The mintedEpochs guard                                                     *)
(* makes a duplicated / multi-parent-merged mint a NO-OP (idempotency).      *)
(****************************************************************************)
EpochMint(v) ==
    /\ v \in activeValidators
    /\ v \notin mintingHalted
    /\ <<v, EpochIndex>> \notin mintedEpochs
    /\ supply' = [supply EXCEPT ![v] = supply[v] + MintAmount]
    /\ protocolMinted' = [protocolMinted EXCEPT ![v] = protocolMinted[v] + MintAmount]
    /\ mintedEpochs' = mintedEpochs \cup {<<v, EpochIndex>>}
    /\ UNCHANGED <<bonds, activeValidators, coopVaultBalance, coopFuelBalance, slashedSet,
                    mintingHalted, quarantinedStake, burnedStake,
                    quarantinedFuel, burnedFuel, blocks,
                    invalidBlocks, equivocationRecords,
                    pendingSlashDeploys, forkChoiceLatest>>

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
(* Redemption resolves quarantined SystemVault custody according to the       *)
(* verdict. Later protocol minting uses the same canonical vault.              *)
(****************************************************************************)
RedeemOutcomes == {"Vindicated", "Guilty", "Burned"}

MinNat(a, b) == IF a <= b THEN a ELSE b

\* Drop every slash-pipeline hash targeting offender o when redemption reverses
\* a slash. A name-keyed filter on h[1].
DropSlashArtifacts(S, o) == {h \in S : h[1] # o}

Redeem(o, outcome) ==
    /\ outcome \in RedeemOutcomes
    /\ quarantinedStake[o] > 0                       \* requires a quarantine
    /\ LET valBond == quarantinedStake[o]
       IN CASE outcome = "Vindicated" ->
                 /\ bonds' = [bonds EXCEPT ![o] = valBond]
                 /\ activeValidators' = activeValidators \cup {o}
                 /\ coopVaultBalance' = coopVaultBalance
                 /\ coopFuelBalance' = coopFuelBalance
                 /\ mintingHalted' = mintingHalted \ {o}
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ supply' = [supply EXCEPT ![o] = supply[o] + quarantinedFuel[o]]
                 /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake
                 /\ burnedFuel' = burnedFuel
                 /\ protocolMinted' = protocolMinted
                 /\ slashedSet' = slashedSet \ {o}
                 /\ mintedEpochs' = mintedEpochs
                 \* The slash is reversed ⇒ vacate o's pipeline bookkeeping.
                 /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
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
                        fuelPenalty == MinNat(penalty, quarantinedFuel[o])
                    IN /\ bonds' = [bonds EXCEPT ![o] = remainder]
                       /\ coopVaultBalance' = coopVaultBalance + penalty
                       /\ coopFuelBalance' = coopFuelBalance + fuelPenalty
                       /\ supply' = [supply EXCEPT ![o] = supply[o] + quarantinedFuel[o] - fuelPenalty]
                 /\ activeValidators' = activeValidators \cup {o}
                 /\ mintingHalted' = mintingHalted \ {o}
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake
                 /\ burnedFuel' = burnedFuel
                 /\ protocolMinted' = protocolMinted
                 /\ slashedSet' = slashedSet \ {o}
                 /\ mintedEpochs' = mintedEpochs
                 \* The slash is (partially) reversed ⇒ vacate o's bookkeeping.
                 /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
            [] outcome = "Burned" ->
                 \* Destroy the quarantined stake; STAY halted; clear ONLY the
                 \* quarantine record. (Stake leaves the tracked set — coop
                 \* unchanged, bond stays 0, still halted/slashed.) The validator
                 \* REMAINS slashed, so the slash-pipeline bookkeeping is kept.
                 /\ bonds' = bonds
                 /\ activeValidators' = activeValidators
                 /\ coopVaultBalance' = coopVaultBalance
                 /\ coopFuelBalance' = coopFuelBalance
                 /\ mintingHalted' = mintingHalted
                 /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                 /\ supply' = supply
                 /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = 0]
                 /\ burnedStake' = burnedStake + valBond
                 /\ burnedFuel' = burnedFuel + quarantinedFuel[o]
                 /\ protocolMinted' = protocolMinted
                 /\ slashedSet' = slashedSet
                 /\ mintedEpochs' = mintedEpochs
                 /\ pendingSlashDeploys' = pendingSlashDeploys
    /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords, forkChoiceLatest>>

(****************************************************************************)
(* Next                                                                     *)
(****************************************************************************)
Next ==
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignHonest(v, s)
    \/ \E v \in Validators, s \in 1..MaxSeqNum : SignEquivocating(v, s)
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

\* NOTE (TLAPS compatibility): the recursive set-sum operators
\* (SumInitialBonds / SumBonds / SumQuarantined) and the conservation invariant
\* Inv_StakeConservation that uses them have been MOVED OUT of this module, into
\* the TLC-only leaf module SlashFlowConservation.tla (which `EXTENDS SlashFlow`),
\* with their definitions, names and comments preserved verbatim. They are still
\* TLC-model-checked via MC_SlashFlow (which now `EXTENDS SlashFlowConservation`),
\* so no model coverage is lost. The relocation is forced by a hard limitation of
\* tlapm 1.5.0: it ABORTS THE ENTIRE MODULE at level-computation time on ANY
\* `RECURSIVE` operator definition (this happens before any proof obligation is
\* generated, so no proof step can work around it), and this abort propagates to
\* every module that `EXTENDS`/`INSTANCE`s such a module. Keeping SlashFlow.tla
\* RECURSIVE-free is therefore a prerequisite for the deductive TLAPS proof of
\* Inv_RedeemedValidatorUnhalted below to be checkable by `tlapm SlashFlow.tla`.

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

Inv_FuelInQuarantineAfterSlash ==
    \A v \in Validators : v \in mintingHalted => supply[v] = 0

\* T-10: Slashed validators are excluded from fork choice.
Inv_SlashedExcludedFromFC ==
    \A v \in slashedSet : forkChoiceLatest[v] = 0

\* Slashed implies removed from active.
Inv_SlashedRemoved ==
    \A v \in slashedSet : v \notin activeValidators

\* Bonds are non-negative.
Inv_BondsNonNegative ==
    \A v \in Validators : bonds[v] >= 0

\* (Inv_StakeConservation — the quarantine-inclusive total-stake conservation
\* invariant — lives in the TLC-only leaf module SlashFlowConservation.tla; see
\* the NOTE above. It is model-checked there via MC_SlashFlow.)

Inv_PendingSlashHasEvidence ==
    pendingSlashDeploys \subseteq invalidBlocks

Inv_PendingSlashTargetUnique ==
    \A h1 \in pendingSlashDeploys :
      \A h2 \in pendingSlashDeploys :
        h1[1] = h2[1] => h1 = h2

Inv_PendingSlashAuthorized ==
    \A h \in pendingSlashDeploys : bonds[h[1]] > 0

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
\* v in mintingHalted, and slash quarantines the offender's vault balance, so a halted
\* validator's (v, EpochIndex) record can be present ONLY if it was minted
\* BEFORE being halted; this invariant asserts the stronger post-slash shape:
\* a slashed/halted validator carries no available vault balance and the
\* halt blocks all further mints).
Inv_HaltedNotMinted ==
    \A v \in Validators : v \in mintingHalted => supply[v] = 0

\* Inv_NoDoubleCreditUnderMerge: a validator is credited AT MOST once per
\* epoch — its protocol-minted total is bounded by one MintAmount (the mintedEpochs
\* idempotency guard prevents a second credit, even under a duplicated /
\* multi-parent-merged epoch mint). Combined with Inv_HaltedNotMinted (a halt
\* quarantines available custody).
Inv_NoDoubleCreditUnderMerge ==
    \A v \in Validators : protocolMinted[v] <= MintAmount

\* Native protocol funding is created only by epoch mint: a vault is non-zero
\* only if it is recorded in mintedEpochs (and not subsequently zeroed by a
\* slash). Equivalently, an unminted, unslashed validator has zero supply.
Inv_SupplyOnlyFromMint ==
    \A v \in Validators :
        (supply[v] > 0) => (protocolMinted[v] > 0 /\ v \notin mintingHalted)

\* Redemption un-halts (the RESTORATIVE outcomes). A validator that is back in
\* activeValidators is NOT in mintingHalted, so the next-epoch mint can re-fund
\* it. This is the TLA image of the spec's "Upon redemption, phlogiston minting
\* resumes at the next epoch boundary" (cost-accounted-rho.tex, paragraph
\* Slashing, around l.3043-3056) and the Rocq anchor
\* ValidatorRedemption.redeem_vindicated_restores (Vindicated/Guilty clear
\* mintingHalted + re-activate with a positive bond). It FAILS if a
\* Vindicated/Guilty Redeem re-activates an offender (activeValidators \cup {o})
\* yet omits the un-halt mintingHalted' = mintingHalted \ {o}.
\*
\* Burned is the spec's TERMINAL, non-restorative case (stake destroyed; minting
\* "contingent on good behavior", tex l.2368-2369 / l.3108-3109): it leaves the
\* offender BOTH unbonded and OUT of activeValidators, so it is correctly
\* outside this invariant's scope. Soundness rests on the model's active=>bond>0
\* (Init bonds are all positive; ExecuteSlash zeros-the-bond-and-deactivates
\* atomically; Redeem restores a positive bond).
Inv_RedeemedValidatorUnhalted ==
    \A v \in activeValidators : v \notin mintingHalted

(****************************************************************************)
(* Inductive-invariant scaffold for the redemption-un-halt safety invariant.*)
(*                                                                          *)
(* Inv_RedeemedValidatorUnhalted is proved DEDUCTIVELY (TLAPS) — for ALL    *)
(* parameter values, with no state enumeration (the full MC_SlashFlow state *)
(* space is too large to model-check) — in the companion proof module       *)
(* SlashFlowProofs.tla (THEOREM Safety). The proof lives in a SEPARATE       *)
(* module because it must `EXTENDS TLAPS` (for the PTL temporal-logic        *)
(* backend etc.), and the standalone tla2tools.jar that TLC uses does NOT    *)
(* bundle the TLAPS standard module — so an `EXTENDS TLAPS` in THIS module   *)
(* would break every TLC model that depends on it (MC_SlashFlow, the CI      *)
(* invariant check, and the tiny MC_SlashFlowRedeem cross-check). Keeping    *)
(* this module TLAPS-free preserves all existing TLC machinery byte-for-byte.*)
(*                                                                          *)
(* The two definitions below (the auxiliary inductive invariant and the      *)
(* assembled IndInv) are plain TLA+ — TLC-checkable and shared by both the   *)
(* TLC models and the TLAPS proof. The auxiliary invariant                  *)
(* Inv_ActiveImpliesBonded supplies the authorization premise for every      *)
(* pending slash and is preserved by the atomic slash and redemption steps. *)
(****************************************************************************)

\* Auxiliary inductive invariant: every active validator carries a positive
\* bond. (Init bonds the active set with InitialBonds > 0; ExecuteSlash zeros
\* the bond AND deactivates atomically; Redeem re-activates only with a
\* positive restored/remainder bond. Burned leaves both unchanged.)
Inv_ActiveImpliesBonded ==
    \A v \in activeValidators : bonds[v] > 0

\* The full inductive invariant carried through the TLAPS proof
\* (SlashFlowProofs.tla). Also a sound TLC invariant in its own right.
IndInv == TypeOK /\ Inv_ActiveImpliesBonded /\ Inv_RedeemedValidatorUnhalted

(****************************************************************************)
(* Liveness: every detected equivocation eventually triggers slash.          *)
(****************************************************************************)
Live_SlashedEventually ==
    \A h \in BlockId :
        h \in pendingSlashDeploys ~> h[1] \in slashedSet

============================================================================
