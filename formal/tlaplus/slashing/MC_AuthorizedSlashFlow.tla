---------------------- MODULE MC_AuthorizedSlashFlow ----------------------
EXTENDS AuthorizedSlashFlow, TLC

CONSTANT
    \* @type: Str;
    v1,
    \* @type: Str;
    v2,
    \* @type: Str;
    h1,
    \* @type: Str;
    h2,
    \* @type: Str;
    e0,
    \* @type: Str;
    e1

MC_Validators == {v1, v2}
MC_Hashes == {h1, h2}
MC_Epochs == {e0, e1}
MC_HashRank == (h1 :> 0 @@ h2 :> 1)
MC_InitialBonds == (v1 :> 100 @@ v2 :> 100)
MC_MaxGeneration == 1
SymmetryV == Permutations(MC_Validators)

============================================================================
