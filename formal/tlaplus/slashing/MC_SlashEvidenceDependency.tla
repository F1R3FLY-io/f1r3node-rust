--------------------- MODULE MC_SlashEvidenceDependency ---------------------
EXTENDS SlashEvidenceDependency, TLC

CONSTANTS d1, d2, h1, h2

MC_Deploys == {d1, d2}
MC_Hashes == {h1, h2}
MC_SlashTarget == (d1 :> h1 @@ d2 :> h2)

=============================================================================
