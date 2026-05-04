--------------------------- MODULE MC_ConcurrentTracker ---------------------------
(****************************************************************************)
(* Model-checking instance for ConcurrentTracker.                           *)
(*                                                                          *)
(* Two threads, one validator key, two distinct block hashes.  This is the  *)
(* minimal configuration that exhibits the lock-free overwrite race.        *)
(*                                                                          *)
(* TLC must be run TWICE:                                                   *)
(*   1. With Locked = FALSE  → must FAIL (counter-example for the bug)      *)
(*   2. With Locked = TRUE   → must PASS (validates fix #2 / T-9.2)         *)
(****************************************************************************)

EXTENDS ConcurrentTracker, TLC

CONSTANTS t1, t2, v1, h1, h2

MC_Threads     == {t1, t2}
MC_Validators  == {v1}
MC_BlockHashes == {h1, h2}

\* Locked is overridden in the .cfg file (TRUE or FALSE)

============================================================================
