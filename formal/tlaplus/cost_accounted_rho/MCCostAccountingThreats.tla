---------------------- MODULE MCCostAccountingThreats -----------------------
(****************************************************************************)
(* Model-checking instance for CostAccountingThreats.                        *)
(****************************************************************************)

EXTENDS CostAccountingThreats, TLC

CONSTANTS
    \* @type: Str;
    good_digest,
    \* @type: Str;
    bad_digest

MC_GoodDigest == good_digest
MC_BadDigest == bad_digest
MC_InitialFuel == 5

=============================================================================
