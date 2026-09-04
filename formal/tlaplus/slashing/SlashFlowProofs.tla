--------------------------- MODULE SlashFlowProofs ---------------------------
(****************************************************************************)
(* Deductive (TLAPS) proof of the redemption-un-halt safety invariant for   *)
(* SlashFlow.                                                               *)
(*                                                                          *)
(*   THEOREM Safety == Spec => []Inv_RedeemedValidatorUnhalted              *)
(*                                                                          *)
(* established for ALL parameter values via the standard inductive-invariant*)
(* pattern (Init establishes IndInv; [Next]_vars preserves it; IndInv       *)
(* implies the target), with NO state enumeration — the full MC_SlashFlow    *)
(* state space is too large to model-check, which is precisely why this is  *)
(* a deductive proof.                                                       *)
(*                                                                          *)
(* This module is SEPARATE from SlashFlow.tla because it must `EXTENDS       *)
(* TLAPS` (for the PTL temporal backend and SMT pragma), and the standalone  *)
(* tla2tools.jar that TLC uses does not bundle the TLAPS standard module —   *)
(* an `EXTENDS TLAPS` in SlashFlow.tla would break every TLC model that      *)
(* depends on it. SlashFlow.tla is also kept free of `RECURSIVE` operator    *)
(* definitions (which tlapm 1.5.0 cannot process — it aborts the whole       *)
(* module, and any module that EXTENDS it, before generating obligations);   *)
(* those live in the TLC-only leaf SlashFlowConservation.tla. This module    *)
(* therefore EXTENDS the RECURSIVE-free SlashFlow.tla, so tlapm can process  *)
(* it.                                                                      *)
(*                                                                          *)
(* Validate with:  tlapm SlashFlowProofs.tla                                *)
(* Reference: docs/casper/theory/slashing/slashing-verification.md §6, §7;         *)
(* cost-accounted-rho.tex (Slashing), Rocq ValidatorRedemption.             *)
(****************************************************************************)

EXTENDS SlashFlow, TLAPS

(****************************************************************************)
(* LEMMA InitIndInv: the initial predicate establishes the inductive        *)
(* invariant.                                                               *)
(****************************************************************************)
LEMMA InitIndInv == Init => IndInv
  BY InitialBondsType
     DEF Init, IndInv, TypeOK, Inv_ActiveImpliesBonded,
         Inv_RedeemedValidatorUnhalted, BlockId

(****************************************************************************)
(* LEMMA NextIndInv: the inductive step. Every Next-or-stutter transition   *)
(* preserves the inductive invariant.                                       *)
(****************************************************************************)
LEMMA NextIndInv == IndInv /\ [Next]_vars => IndInv'
  <1> SUFFICES ASSUME IndInv, [Next]_vars
               PROVE  IndInv'
      OBVIOUS
  <1> USE DEF IndInv, Inv_ActiveImpliesBonded, Inv_RedeemedValidatorUnhalted
  \* Disjuncts that leave bonds, activeValidators, mintingHalted UNCHANGED:
  \* the two carried invariants follow from the IH directly, TypeOK' from the
  \* per-action updates to the OTHER variables.
  <1>1. CASE UNCHANGED vars
        BY <1>1 DEF vars, TypeOK
  <1>2. ASSUME NEW v \in Validators, NEW s \in 1..MaxSeqNum, SignHonest(v, s)
        PROVE  IndInv'
        BY <1>2 DEF SignHonest, TypeOK, BlockId
  <1>3. ASSUME NEW v \in Validators, NEW s \in 1..MaxSeqNum, SignEquivocating(v, s)
        PROVE  IndInv'
    \* bonds, activeValidators, mintingHalted UNCHANGED ⇒ the two carried
    \* invariants from the IH; TypeOK' decomposed per-conjunct (the one-shot
    \* 17-conjunct goal is too large for the backend on the nested-EXCEPT
    \* blocks' update + the s-1 equivocation record).
    <2>a. s - 1 \in 0..MaxSeqNum
          BY <1>3, MaxSeqNumType, SMT
    <2>b. blocks' \in [Validators -> [1..MaxSeqNum -> SUBSET (1..2)]]
          BY <1>3 DEF SignEquivocating, TypeOK
    <2>c. equivocationRecords' \in SUBSET (Validators \X (0..MaxSeqNum))
          BY <1>3, <2>a DEF SignEquivocating, TypeOK
    <2>d0. <<v, s, 2>> \in BlockId
           BY <1>3 DEF BlockId
    <2>d1. invalidBlocks' \in SUBSET BlockId
           BY <1>3, <2>d0 DEF SignEquivocating, TypeOK
    <2>d2. pendingSlashDeploys' \in SUBSET BlockId
           BY <1>3, <2>d0 DEF SignEquivocating, TypeOK
    <2>1. TypeOK'
          BY <1>3, <2>b, <2>c, <2>d1, <2>d2 DEF SignEquivocating, TypeOK
    <2>2. Inv_ActiveImpliesBonded' /\ Inv_RedeemedValidatorUnhalted'
          BY <1>3 DEF SignEquivocating
    <2> QED BY <2>1, <2>2 DEF IndInv
  <1>6. ASSUME NEW v \in Validators, EpochMint(v)
        PROVE  IndInv'
    \* bonds, activeValidators, mintingHalted UNCHANGED ⇒ the two carried
    \* invariants from the IH; TypeOK' needs MintAmount \in Nat for the supply
    \* credit supply[v] + MintAmount \in Nat (decomposed off the 17-conjunct).
    <2>1. supply' \in [Validators -> Nat]
          BY <1>6, MintAmountType DEF EpochMint, TypeOK
    <2>2. protocolMinted' \in [Validators -> Nat]
          BY <1>6, MintAmountType DEF EpochMint, TypeOK
    <2>3. mintedEpochs' \in SUBSET (Validators \X {EpochIndex})
          BY <1>6 DEF EpochMint, TypeOK
    <2>4. TypeOK'
          BY <1>6, <2>1, <2>2, <2>3 DEF EpochMint, TypeOK
    <2>5. Inv_ActiveImpliesBonded' /\ Inv_RedeemedValidatorUnhalted'
          BY <1>6 DEF EpochMint
    <2> QED BY <2>4, <2>5 DEF IndInv
  \* ExecuteSlash(h): o == h[1] \in Validators with a positive bond.
  <1>7. ASSUME NEW h \in BlockId, ExecuteSlash(h)
        PROVE  IndInv'
    <2> DEFINE o == h[1]
    <2>o. o \in Validators
          BY <1>7 DEF ExecuteSlash, BlockId
    <2>1. TypeOK'
      <3>e1. /\ bonds' = [bonds EXCEPT ![o] = 0]
              /\ activeValidators' = activeValidators \ {o}
              /\ coopVaultBalance' = coopVaultBalance
              /\ coopFuelBalance' = coopFuelBalance
              /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = bonds[o]]
              /\ burnedStake' = burnedStake
              /\ slashedSet' = slashedSet \cup {o}
              BY <1>7 DEF ExecuteSlash
      <3>e2. /\ pendingSlashDeploys' = {d \in pendingSlashDeploys : d[1] # o}
              /\ forkChoiceLatest' = [forkChoiceLatest EXCEPT ![o] = 0]
              /\ mintingHalted' = mintingHalted \cup {o}
              /\ supply' = [supply EXCEPT ![o] = 0]
              /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = supply[o]]
              /\ burnedFuel' = burnedFuel
              /\ protocolMinted' = protocolMinted
              /\ mintedEpochs' = mintedEpochs
              /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords>>
              BY <1>7 DEF ExecuteSlash
      <3> QED BY <2>o, <3>e1, <3>e2 DEF TypeOK, BlockId
    <2>2. Inv_ActiveImpliesBonded'
          BY <1>7, <2>o DEF ExecuteSlash, TypeOK
    <2>3. Inv_RedeemedValidatorUnhalted'
          BY <1>7, <2>o DEF ExecuteSlash
    <2> QED BY <2>1, <2>2, <2>3
  \* Redeem(o, outcome): guard quarantinedStake[o] > 0; valBond > 0.
  <1>8. ASSUME NEW o \in Validators, NEW oc \in RedeemOutcomes, Redeem(o, oc)
        PROVE  IndInv'
    <2>v. quarantinedStake[o] > 0 /\ quarantinedStake[o] \in Nat
          BY <1>8 DEF Redeem, IndInv, TypeOK
    <2>1. CASE oc = "Vindicated"
      \* bonds'=[bonds EXCEPT ![o]=valBond]; active'=active\cup{o};
      \* halted'=halted\{o}; valBond=quarantinedStake[o]>0.
      <3>1. TypeOK'
            \* Expose the Vindicated arm's primed equalities (CASE/LET-folded),
            \* then close TypeOK' — the vault restoration is additive in Nat and
            \* DropSlashArtifacts keeps its carrier in BlockId.
            <4>e1. /\ bonds' = [bonds EXCEPT ![o] = quarantinedStake[o]]
                   /\ activeValidators' = activeValidators \cup {o}
                   /\ coopVaultBalance' = coopVaultBalance
                   /\ coopFuelBalance' = coopFuelBalance
                   /\ mintingHalted' = mintingHalted \ {o}
                   /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                   /\ burnedStake' = burnedStake
                   BY <1>8, <2>1 DEF Redeem
            <4>e2. /\ slashedSet' = slashedSet \ {o}
                   /\ mintedEpochs' = mintedEpochs
                   /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
                   /\ supply' = [supply EXCEPT ![o] = supply[o] + quarantinedFuel[o]]
                   /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = 0]
                   /\ burnedFuel' = burnedFuel
                   /\ protocolMinted' = protocolMinted
                   /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords, forkChoiceLatest>>
                   BY <1>8, <2>1 DEF Redeem
            <4>s. supply' \in [Validators -> Nat]
                  BY <4>e2 DEF TypeOK
            <4>p. pendingSlashDeploys' \in SUBSET BlockId
                  BY <4>e2 DEF TypeOK, DropSlashArtifacts
            <4> QED BY <2>v, <4>e1, <4>e2, <4>s, <4>p DEF TypeOK, BlockId
      <3>2. Inv_ActiveImpliesBonded'
            \* v=o ⇒ bonds'[o]=valBond>0; v # o in active ⇒
            \* bonds'[v]=bonds[v]>0 (IH).
            BY <1>8, <2>1, <2>v DEF Redeem, TypeOK
      <3>3. Inv_RedeemedValidatorUnhalted'
            \* o removed from halted ⇒ o \notin halted\{o}; v # o in active ⇒
            \* v \notin halted (IH) ⇒ v \notin halted\{o}.
            BY <1>8, <2>1 DEF Redeem
      <3> QED BY <3>1, <3>2, <3>3
    <2>2. CASE oc = "Guilty"
      \* penalty=valBond\div 2; remainder=valBond-penalty;
      \* bonds'=[bonds EXCEPT ![o]=remainder]; active'=active\cup{o};
      \* halted'=halted\{o}. Need remainder > 0.
      <3>r. quarantinedStake[o] - (quarantinedStake[o] \div 2) > 0
            BY <2>v
      <3>n. /\ quarantinedStake[o] \div 2 \in Nat
            /\ quarantinedStake[o] - (quarantinedStake[o] \div 2) \in Nat
            BY <2>v
      <3>t. /\ quarantinedFuel[o] \in Nat
            /\ supply[o] \in Nat
            /\ coopFuelBalance \in Nat
            BY <1>8 DEF IndInv, TypeOK
      <3>p. LET fuelPenalty == MinNat(quarantinedStake[o] \div 2, quarantinedFuel[o])
            IN /\ fuelPenalty \in Nat
               /\ fuelPenalty <= quarantinedFuel[o]
            BY <3>n, <3>t DEF MinNat
      <3>f. LET fuelPenalty == MinNat(quarantinedStake[o] \div 2, quarantinedFuel[o])
            IN /\ fuelPenalty \in Nat
               /\ coopFuelBalance + fuelPenalty \in Nat
               /\ supply[o] + quarantinedFuel[o] - fuelPenalty \in Nat
            BY <3>t, <3>p
      <3>1. TypeOK'
            \* Expose the Guilty arm's primed equalities (CASE + nested
            \* penalty/remainder LET-folded), then close TypeOK'. The split
            \* bond (remainder) and the credited coop (coop + penalty) stay in
            \* Nat by <3>n; the set-builder updates keep their carriers typed.
            <4>e1. /\ bonds' = [bonds EXCEPT ![o] = quarantinedStake[o] - (quarantinedStake[o] \div 2)]
                   /\ coopVaultBalance' = coopVaultBalance + (quarantinedStake[o] \div 2)
                   /\ coopFuelBalance' = coopFuelBalance + MinNat(quarantinedStake[o] \div 2, quarantinedFuel[o])
                   /\ activeValidators' = activeValidators \cup {o}
                   /\ mintingHalted' = mintingHalted \ {o}
                   /\ quarantinedStake' = [quarantinedStake EXCEPT ![o] = 0]
                   /\ burnedStake' = burnedStake
                   BY <1>8, <2>2 DEF Redeem
            <4>e2. /\ slashedSet' = slashedSet \ {o}
                   /\ mintedEpochs' = mintedEpochs
                   /\ pendingSlashDeploys' = DropSlashArtifacts(pendingSlashDeploys, o)
                   /\ supply' = [supply EXCEPT ![o] = supply[o] + quarantinedFuel[o] - MinNat(quarantinedStake[o] \div 2, quarantinedFuel[o])]
                   /\ quarantinedFuel' = [quarantinedFuel EXCEPT ![o] = 0]
                   /\ burnedFuel' = burnedFuel
                   /\ protocolMinted' = protocolMinted
                   /\ UNCHANGED <<blocks, invalidBlocks, equivocationRecords, forkChoiceLatest>>
                   BY <1>8, <2>2 DEF Redeem
            <4>b. bonds' \in [Validators -> Nat]
                  BY <4>e1, <3>n DEF TypeOK
            <4>c. coopVaultBalance' \in Nat
                  BY <4>e1, <3>n DEF TypeOK
            <4>f. /\ coopFuelBalance' \in Nat
                  /\ supply' \in [Validators -> Nat]
                  BY <4>e1, <4>e2, <3>f DEF TypeOK
            <4>p. pendingSlashDeploys' \in SUBSET BlockId
                  BY <4>e2 DEF TypeOK, DropSlashArtifacts
            <4> QED BY <4>e1, <4>e2, <4>b, <4>c, <4>f, <4>p DEF TypeOK, BlockId
      <3>2. Inv_ActiveImpliesBonded'
            \* v=o ⇒ bonds'[o]=remainder>0 (<3>r); v # o in active ⇒
            \* bonds'[v]=bonds[v]>0 (IH).
            BY <1>8, <2>2, <2>v, <3>r DEF Redeem, TypeOK
      <3>3. Inv_RedeemedValidatorUnhalted'
            \* identical to Vindicated: halted'=halted\{o}.
            BY <1>8, <2>2 DEF Redeem
      <3> QED BY <3>1, <3>2, <3>3
    <2>3. CASE oc = "Burned"
      \* bonds'=bonds; active'=active; halted'=halted (all UNCHANGED) ⇒
      \* both invariants from IH directly.
      <3>1. TypeOK'
            BY <1>8, <2>3, <2>v DEF Redeem, TypeOK
      <3>2. Inv_ActiveImpliesBonded'
            BY <1>8, <2>3 DEF Redeem, TypeOK
      <3>3. Inv_RedeemedValidatorUnhalted'
            BY <1>8, <2>3 DEF Redeem
      <3> QED BY <3>1, <3>2, <3>3
    <2> QED BY <2>1, <2>2, <2>3 DEF RedeemOutcomes
  <1> QED
      BY <1>1, <1>2, <1>3, <1>6, <1>7, <1>8 DEF Next

(****************************************************************************)
(* THEOREM Safety: Spec satisfies []Inv_RedeemedValidatorUnhalted.          *)
(* (Spec = Init /\ [][Next]_vars /\ <fairness>; the safety conjuncts        *)
(* Init /\ [][Next]_vars suffice for an invariant.)                         *)
(****************************************************************************)
THEOREM Safety == Spec => []Inv_RedeemedValidatorUnhalted
  <1>1. Init => IndInv
        BY InitIndInv
  <1>2. IndInv /\ [Next]_vars => IndInv'
        BY NextIndInv
  <1>3. IndInv => Inv_RedeemedValidatorUnhalted
        BY DEF IndInv
  <1> QED
      BY <1>1, <1>2, <1>3, PTL DEF Spec

============================================================================
