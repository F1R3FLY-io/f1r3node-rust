------------------------ MODULE MC_SlashFlowRedeem ------------------------
(****************************************************************************)
(* TINY model-checking instance for SlashFlow, sized for the redemption     *)
(* un-halt invariants (Inv_ActiveImpliesBonded + Inv_RedeemedValidatorUnhalted)*)
(* that are proved deductively (THEOREM Safety in SlashFlow.tla).            *)
(*                                                                          *)
(* TWO validators, each bonded with 100 stake; max seq number 1. This       *)
(* instance has a tiny state space and completes in seconds — it is a quick *)
(* cross-check of the inductive invariants and is NOT the exhaustive model   *)
(* (the full 3-validator / MaxSeqNum=2 MC_SlashFlow is intentionally left    *)
(* to the deductive proof, since its state space OOMs an enumerative check). *)
(*                                                                          *)
(* EXTENDS SlashFlow (not SlashFlowConservation): this instance does not     *)
(* check Inv_StakeConservation, so it has no need of the RECURSIVE sum       *)
(* operators that live in the conservation layer.                           *)
(****************************************************************************)

EXTENDS SlashFlow, TLC

CONSTANTS v1, v2

MC_Validators    == {v1, v2}
MC_InitialBonds  == (v1 :> 100 @@ v2 :> 100)
MC_MaxSeqNum     == 1
MC_MintAmount    == 1000   \* Cost-Accounted Rho: epochPhlogiston per mint
MC_EpochIndex    == 0      \* the single epoch index this model checks

============================================================================
