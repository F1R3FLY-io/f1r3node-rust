(*
  SoundnessGC1.thy --- soundness of the escape + one-sided algorithm.

  States: every name in gc1(P) is garbage with respect to P.

  Phase-0 skeleton: proof body is `sorry`.

  Intended proof structure:
    1. Re-use soundness_gc0 for the gc0 component.
    2. For the gc1-only component (atoms u with gc1_atom P u):
       2a. Define an invariant on configurations: u is retained-private
           inside the configuration.  Show it is preserved by every
           reduction rule:
            - StructPar / IfTrue / IfFalse / MatchHit / MatchFallThrough /
              EvalQuoteUnquote: preservation is mechanical (no atom
              motion).
            - ProduceInstall / ConsumeInstall: u stays bound by the
              new in P, and the datum/cont reflects payload_names of P
              after substitution.
            - New: introduces fresh atoms, none equal to u (by the
              freshness side-condition).
            - Comm: would have to fire on a name n with u ∈
              atoms_of_name n; but only_send_side / only_recv_side /
              the bundle-blocked refinements forbid the missing side.
       2b. Conclude: across the whole future of K[P], no Comm event
           on a name with u ∈ atoms ever fires.
    3. Therefore is_garbage P c for any c carrying such a u.

  The key lemma is the preservation step at Comm, which depends on the
  spatial-matcher oracle being symmetric in the way the runtime is: the
  matcher cannot synthesize a name with a private atom that does not
  appear in the configuration.  This is captured (later) by an axiom on
  matches that ought to be stated alongside the oracle in Patterns.thy.
*)

theory SoundnessGC1
  imports SoundnessGC0
begin

theorem soundness_gc1:
  assumes "c \<in> gc1 P"
  shows "is_garbage P c"
  sorry

corollary soundness_gc1_extends_gc0:
  assumes "c \<in> gc0 P"
  shows "is_garbage P c"
  using assms gc0_subset_gc1 soundness_gc1 by blast

end
