------------------------------- MODULE MC ---------------------------------
(****************************************************************************)
(* Model-checking instance for CostAccountedRho.                            *)
(* Defines concrete constants for TLC to enumerate.                         *)
(****************************************************************************)

EXTENDS CostAccountedRho, TLC

\* Concrete model values
CONSTANTS p1, p2, p3, ch_a, ch_b, ch_c

\* Override the abstract constants with concrete values
MC_Processes    == {p1, p2, p3}
MC_Channels     == {ch_a, ch_b, ch_c}
MC_InitialTokens == (p1 :> 1 @@ p2 :> 1 @@ p3 :> 1)
MC_sigChannel    == (p1 :> ch_a @@ p2 :> ch_b @@ p3 :> ch_c)

=============================================================================
