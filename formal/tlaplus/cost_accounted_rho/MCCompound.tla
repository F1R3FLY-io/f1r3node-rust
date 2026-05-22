------------------------------ MODULE MCCompound ------------------------------
(****************************************************************************)
(* Model-checking instance for CompoundProtocol.                            *)
(*                                                                          *)
(* Configuration: 2 atomic + 1 compound process.                            *)
(* The compound process spawns 1 child (atomic) when its COMM fires.        *)
(* This tests: Split firing, nested gates, recursive eval, and              *)
(* cost determinism across all interleavings.                               *)
(****************************************************************************)

EXTENDS CompoundProtocol, TLC

CONSTANTS a1, a2, c1, child1
CONSTANTS ch_a1, ch_a2, ch_c1_combined, ch_c1_s1, ch_c1_s2, ch_child1

MC_Procs         == {a1, a2, c1, child1}
MC_Channels      == {ch_a1, ch_a2, ch_c1_combined, ch_c1_s1, ch_c1_s2, ch_child1}
MC_AtomicProcs   == {a1, a2, child1}
MC_CompoundProcs == {c1}

MC_TokensPerProc == (a1 :> 1 @@ a2 :> 1 @@ c1 :> 1 @@ child1 :> 1)

MC_PrimaryChan   == (a1 :> ch_a1 @@ a2 :> ch_a2 @@ c1 :> ch_c1_s1 @@ child1 :> ch_child1)
MC_SecondaryChan == (c1 :> ch_c1_s2)
MC_CompoundChan  == (c1 :> ch_c1_combined)

\* c1's COMM body spawns child1 (recursive eval)
\* All others spawn nothing
MC_SpawnedProcs  == (a1 :> {} @@ a2 :> {} @@ c1 :> {child1} @@ child1 :> {})

MC_CostPerGate   == 1

=============================================================================
