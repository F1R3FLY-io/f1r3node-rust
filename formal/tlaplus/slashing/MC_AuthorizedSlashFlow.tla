---------------------- MODULE MC_AuthorizedSlashFlow ----------------------
EXTENDS AuthorizedSlashFlow, TLC

CONSTANTS v1, v2, h1, h2, e0, e1

MC_Validators == {v1, v2}
MC_Hashes == {h1, h2}
MC_Epochs == {e0, e1}
MC_InitialBonds == (v1 :> 100 @@ v2 :> 100)

============================================================================
