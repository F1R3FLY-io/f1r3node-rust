---------------- MODULE MC_ScopeTrappedGC_payload_leak_pre_fix ----------------
EXTENDS ScopeTrappedGC

\* Pre-fix control #1: classifier weakened to accept x used in a Send
\* payload. Expected violation: Inv_N_NeverInParkedData (an external send
\* on a forgeable channel can propagate N to any receiver).
============================================================================
