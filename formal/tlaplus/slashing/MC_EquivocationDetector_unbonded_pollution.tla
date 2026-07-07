------------------- MODULE MC_EquivocationDetector_unbonded_pollution -------------------
(****************************************************************************)
(* FV audit #6 — unbonded-window record pollution fork.                     *)
(*                                                                          *)
(* Small-bound (2 validators × 1 seqnum × 2 blocks) model-checking instance  *)
(* for the enriched EquivocationDetector, exercising offender bond-status    *)
(* dynamics (Unbond/Rebond) and the record witness set (recordWitness /      *)
(* StampWitness).  Two companion cfgs select post-fix vs pre-fix behaviour    *)
(* via the EnableStampWitness selector — both share this wrapper:            *)
(*                                                                          *)
(*   MC_EquivocationDetector_unbonded_pollution.cfg          POST-FIX        *)
(*     (EnableStampWitness = FALSE)  ⇒ recordWitness stays empty; MUST PASS   *)
(*     Inv_NoStampAgainstUnbonded AND Inv_NeglectNotFromUnbondedPollution.    *)
(*                                                                          *)
(*   MC_EquivocationDetector_unbonded_pollution_pre_fix.cfg  PRE-FIX         *)
(*     (EnableStampWitness = TRUE)   ⇒ StampWitness stamps the observer hash  *)
(*     into an UNBONDED offender's record; MUST REPRODUCE a counterexample    *)
(*     to Inv_NoStampAgainstUnbonded (the fork's root cause).                 *)
(*                                                                          *)
(* Reference: docs/theory/slashing/design/12-failure-modes.md §12.2.1a.      *)
(****************************************************************************)

EXTENDS EquivocationDetector, TLC

CONSTANTS v1, v2

\* NB: MaxSeqNum = 1 keeps the enriched model (bond-status toggling + witness
\* set) exhaustively checkable in seconds.  It is sufficient to exhibit BOTH
\* FV-audit-#6 invariant directions: an equivocation record <<v, 0>> is created
\* from an equivocation at seq 1, the offender may Unbond, and (pre-fix)
\* StampWitness pollutes the record while unbonded — the seq dimension is not
\* what the pollution invariants depend on.  The base detector's own soundness
\* invariants are exhausted separately at 2v/2s/2b (MC_EquivocationDetector_safety).
MC_Validators        == {v1, v2}
MC_MaxSeqNum         == 1
MC_MaxBlocksPerSeq   == 2

============================================================================
