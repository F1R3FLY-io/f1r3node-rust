------------------------------ MODULE MCLolly ------------------------------
(****************************************************************************)
(* Model-checking harness for LollyProtocol (Phase 3 / 4.6).                *)
(* MaxInvocations=6 exercises capability registration, invocation,         *)
(* revocation, and exhaustion paths.                                        *)
(****************************************************************************)

EXTENDS LollyProtocol, TLC

=============================================================================
