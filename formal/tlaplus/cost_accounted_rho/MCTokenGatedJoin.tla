------------------------- MODULE MCTokenGatedJoin ----------------------------
(****************************************************************************)
(* Model-checking harness for TokenGatedJoin (MAJOR-5 — the token-gated-join *)
(* sequential-fuel griefing / atomicity obligation).                         *)
(*                                                                          *)
(* Pair with TokenGatedJoin.cfg, run via                                     *)
(*   tlc -deadlock -config TokenGatedJoin.cfg MCTokenGatedJoin.tla            *)
(* (the check script's WRAPPED_BY map performs this pairing).                 *)
(*                                                                          *)
(* The DEFAULT .cfg (TokenGatedJoin.cfg) checks the NATIVE-model safety suite *)
(* (P1 funded-path equivalence, P2a/P2b native no-griefing, P3 conservation/  *)
(* no-underflow/no-theft, P4 conservation of authority) — all must HOLD.      *)
(*                                                                          *)
(* The companion .cfg (TokenGatedJoinM2Grief.cfg) checks                      *)
(* Inv_M2_NoVictimDrainWithoutFire — which TLC REFUTES with a counterexample, *)
(* CONFIRMING the griefing vector is real for the TRANSPILER runtime-gate     *)
(* model (and ONLY there). That refutation is the EXPECTED, intended outcome   *)
(* of that run.                                                               *)
(****************************************************************************)

EXTENDS TokenGatedJoin, TLC

=============================================================================
