------------------------------ MODULE MCBang ------------------------------
(****************************************************************************)
(* Model-checking harness for BangProtocol (Phase 3 / 4.6).                 *)
(* Tighter bound (Bound=5, MaxInvocations=10) explores both unbounded and  *)
(* bounded-exhaustion paths.                                                *)
(****************************************************************************)

EXTENDS BangProtocol, TLC

=============================================================================
