--------------------- MODULE MC_JustificationProjection ---------------------
EXTENDS JustificationProjection, TLC

CONSTANTS
    v1,
    v2,
    v3

MC_Validators == {v1, v2, v3}
MC_MaxJustifications == 3

=============================================================================
