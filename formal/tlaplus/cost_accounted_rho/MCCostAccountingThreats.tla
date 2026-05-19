---------------------- MODULE MCCostAccountingThreats -----------------------
(****************************************************************************)
(* Model-checking instance for CostAccountingThreats.                        *)
(****************************************************************************)

EXTENDS CostAccountingThreats, TLC

CONSTANTS good_digest, bad_digest

MC_GoodDigest == good_digest
MC_BadDigest == bad_digest
MC_InitialFuel == 5

=============================================================================
