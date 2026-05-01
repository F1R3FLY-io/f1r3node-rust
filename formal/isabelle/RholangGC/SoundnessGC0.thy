(*
  SoundnessGC0.thy --- soundness of the coarse GC0 algorithm.

  States: every name in gc0(P) is garbage with respect to P.

  Phase-0 skeleton: proof body is `sorry`.  Phase-1 will discharge it.

  The intended proof structure:
    1. Prove a lemma "atom-introduction is the only source of new private
       atoms in a configuration": across the whole reduction, the set of
       atoms occurring anywhere in (sigma, P') comes from atoms_in_config
       at step 0 plus the atoms allocated by EvtNew labels along the way.
    2. Prove that an EvtNew step only allocates fresh atoms (already
       enforced by the New rule's freshness side-condition).
    3. Conclude: an EvtComm(c) step requires `atoms_of_name c'` for the
       fired name c' to be a subset of (atoms_in_config initial) U (atoms
       allocated by previous EvtNew steps); and (initial atoms) is a
       subset of atoms(K) U atoms(P) U pub.
    4. If c has an atom outside atoms(P) U pub U bn_new(P), and K cannot
       forge c (so atoms(c) is not subset of atoms(K) U pub), then the
       offending atom must be allocated by some EvtNew --- contradicting
       that it is also outside bn_new(P).
*)

theory SoundnessGC0
  imports Garbage
begin

theorem soundness_gc0:
  assumes "c \<in> gc0 P"
  shows "is_garbage P c"
  sorry

end
