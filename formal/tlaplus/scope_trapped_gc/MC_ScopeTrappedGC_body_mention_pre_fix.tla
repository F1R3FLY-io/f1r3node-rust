--------------- MODULE MC_ScopeTrappedGC_body_mention_pre_fix ---------------
EXTENDS ScopeTrappedGC

\* Pre-fix control #2: classifier weakened to allow x anywhere inside a
\* Receive body. Expected violation: Inv_N_NeverInParkedBodyOnForgeable
\* (an external deploy can park a receive on a forgeable channel whose
\* body textually contains N; firing fires the body and N escapes).
============================================================================
