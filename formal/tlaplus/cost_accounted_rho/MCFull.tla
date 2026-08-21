------------------------------- MODULE MCFull ---------------------------------
(****************************************************************************)
(* Model-checking instance for FullProtocol.                                *)
(*                                                                          *)
(* Configuration:                                                           *)
(*   - a1, a2: atomic (depth 0), sharing channel ch_s (1 token each)        *)
(*   - c1: compound (depth 1), 1 token                                      *)
(*   - d1: doubly-compound (depth 2), 1 token                               *)
(*   - js1, js2: atomic fuel sources for Join (2 tokens each)               *)
(*   - jm: Join mediator (depth 0, 0 initial tokens)                        *)
(*     JoinPrimCh = ch_j1, JoinSecCh = ch_j2, JoinOutCh = ch_jm            *)
(*     After Join fires, jm gets 1 token on ch_jm and can fire its gate.    *)
(*                                                                          *)
(* Channel layout:                                                          *)
(*   ch_s       -- shared by a1 and a2 (2 tokens total)                     *)
(*   ch_c1_top  -- c1 split input (compound channel)                        *)
(*   ch_c1_g1   -- c1 gate 1 (split secondary output)                      *)
(*   ch_c1_g2   -- c1 gate 2 (split primary output)                        *)
(*   ch_d1_top  -- d1 split 1 input (outermost compound)                   *)
(*   ch_d1_mid  -- d1 split 1 primary out = split 2 input                  *)
(*   ch_d1_g1   -- d1 gate 1 (split 1 secondary output)                    *)
(*   ch_d1_g2   -- d1 gate 2 (split 2 secondary output)                    *)
(*   ch_d1_g3   -- d1 gate 3 (split 2 primary output)                      *)
(*   ch_j1      -- js1 gate channel AND join primary input                  *)
(*   ch_j2      -- js2 gate channel AND join secondary input                *)
(*   ch_jm      -- jm gate channel AND join output                          *)
(*                                                                          *)
(* Expected terminal cost:                                                  *)
(*   a1: 1 gate  = 1    a2: 1 gate  = 1                                    *)
(*   c1: 2 gates = 2    d1: 3 gates = 3                                    *)
(*   js1: 1 gate = 1    js2: 1 gate = 1                                    *)
(*   jm: 1 gate  = 1                                                        *)
(*   Total = 10                                                              *)
(****************************************************************************)

EXTENDS FullProtocol, TLC

\* ---- Process identifiers ----
CONSTANTS a1, a2, c1, d1, js1, js2, jm

\* ---- Channel identifiers ----
CONSTANTS ch_s,
          ch_c1_top, ch_c1_g1, ch_c1_g2,
          ch_d1_top, ch_d1_mid, ch_d1_g1, ch_d1_g2, ch_d1_g3,
          ch_j1, ch_j2, ch_jm

\* ---- Procs ----
MC_Procs == {a1, a2, c1, d1, js1, js2, jm}

\* ---- Channels ----
MC_Channels == {ch_s, ch_c1_top, ch_c1_g1, ch_c1_g2,
                ch_d1_top, ch_d1_mid, ch_d1_g1, ch_d1_g2, ch_d1_g3,
                ch_j1, ch_j2, ch_jm}

\* ---- Nesting depths ----
MC_NestingDepth ==
    a1 :> 0 @@ a2 :> 0 @@ c1 :> 1 @@ d1 :> 2 @@
    js1 :> 0 @@ js2 :> 0 @@ jm :> 0

\* ---- Tokens per process ----
\* a1, a2: 1 each on shared ch_s (total 2 on ch_s)
\* c1: 1 on ch_c1_top
\* d1: 1 on ch_d1_top
\* js1: 2 on ch_j1 (1 for own gate, 1 for Join input)
\* js2: 2 on ch_j2 (1 for own gate, 1 for Join input)
\* jm: 0 (gets token from Join output)
MC_TokensPerProc ==
    a1 :> 1 @@ a2 :> 1 @@ c1 :> 1 @@ d1 :> 1 @@
    js1 :> 2 @@ js2 :> 2 @@ jm :> 0

\* ---- Gate channels ----
\* a1 (depth 0): 1 gate on ch_s
\* a2 (depth 0): 1 gate on ch_s (shared!)
\* c1 (depth 1): 2 gates on ch_c1_g1 (layer 1), ch_c1_g2 (layer 2)
\* d1 (depth 2): 3 gates on ch_d1_g1 (layer 1), ch_d1_g2 (layer 2), ch_d1_g3 (layer 3)
\* js1 (depth 0): 1 gate on ch_j1
\* js2 (depth 0): 1 gate on ch_j2
\* jm (depth 0): 1 gate on ch_jm
MC_GateChans ==
    a1  :> <<ch_s>> @@
    a2  :> <<ch_s>> @@
    c1  :> <<ch_c1_g1, ch_c1_g2>> @@
    d1  :> <<ch_d1_g1, ch_d1_g2, ch_d1_g3>> @@
    js1 :> <<ch_j1>> @@
    js2 :> <<ch_j2>> @@
    jm  :> <<ch_jm>>

\* ---- Split channels ----
\* c1 (depth 1): 1 split level
\*   Split 1: in=ch_c1_top, primOut=ch_c1_g2, secOut=ch_c1_g1
MC_SplitIn ==
    c1 :> <<ch_c1_top>> @@
    d1 :> <<ch_d1_top, ch_d1_mid>>

MC_SplitPrimOut ==
    c1 :> <<ch_c1_g2>> @@
    d1 :> <<ch_d1_mid, ch_d1_g3>>

MC_SplitSecOut ==
    c1 :> <<ch_c1_g1>> @@
    d1 :> <<ch_d1_g1, ch_d1_g2>>

\* ---- Spawned processes (no spawning in this model) ----
MC_SpawnedProcs ==
    a1 :> {} @@ a2 :> {} @@ c1 :> {} @@ d1 :> {} @@
    js1 :> {} @@ js2 :> {} @@ jm :> {}

\* ---- Cost per gate ----
MC_CostPerGate == 1

\* ---- Expected terminal cost ----
\* a1(1) + a2(1) + c1(2) + d1(3) + js1(1) + js2(1) + jm(1) = 10
MC_ExpectedTerminalCost == 10

\* ---- Join mediator configuration ----
MC_JoinProcs == {jm}

MC_JoinPrimCh == jm :> ch_j1
MC_JoinSecCh  == jm :> ch_j2
MC_JoinOutCh  == jm :> ch_jm

=============================================================================
