------------------------- MODULE MCCompoundSettlement -------------------------
(****************************************************************************)
(* Model-checking harness for CompoundSettlement (#12 — the exact            *)
(* per-component Split/Join compound settlement debit).                      *)
(*                                                                          *)
(* Pair with CompoundSettlement.cfg, run via                                 *)
(*   tlc -deadlock -config CompoundSettlement.cfg MCCompoundSettlement.tla    *)
(* (the check script's WRAPPED_BY map performs this pairing).                 *)
(****************************************************************************)

EXTENDS CompoundSettlement, TLC

=============================================================================
