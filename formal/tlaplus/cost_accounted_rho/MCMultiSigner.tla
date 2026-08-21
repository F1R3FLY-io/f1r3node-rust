------------------------------ MODULE MCMultiSigner ------------------------------
(****************************************************************************)
(* Model-checking harness for MultiSignerProtocol (Phase 1.10 / 4.6).       *)
(*                                                                          *)
(* Concrete constants for TLC: 3 cosigners, 4-phlo-per-signer cap. Tests    *)
(* the Map-in-MVar refinement's per-cosigner attribution, FIFO drain        *)
(* ordering, and outer soft-checkpoint atomicity under partial pre-charge   *)
(* failure.                                                                 *)
(*                                                                          *)
(* Pair with MCMultiSigner.cfg, run via                                     *)
(*   tlc -deadlock -config MCMultiSigner.cfg MCMultiSigner.tla              *)
(****************************************************************************)

EXTENDS MultiSignerProtocol, TLC

=============================================================================
