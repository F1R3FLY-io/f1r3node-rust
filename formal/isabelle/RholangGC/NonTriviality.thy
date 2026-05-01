(*
  NonTriviality.thy --- gc0(P) is countably infinite for every P.

  Phase-0 skeleton: proof body is `sorry`.

  The intended proof:
    1. atoms_of_par P is finite (structural induction on par; uses
       expr_subterm_pars_finite for the EExpr case).
    2. bn_new_par P is finite (structural induction on par).
    3. pub is finite (axiom in Atoms.thy).
    4. The atom type is countably infinite (Nominal2 atom sort is
       infinite; explicitly: there exists an injection nat ==> atom).
    5. Therefore atoms - (atoms_of_par P U bn_new_par P U pub) is
       countably infinite.
    6. Each such atom a yields the name GPrivate a, which is in gc0 P:
       atoms_of_name (GPrivate a) = {a} witnesses the existential, with
       a ∉ atoms_of_par P, a ∉ pub, a ∉ bn_new_par P.
    7. The map a |-> GPrivate a is injective; so gc0 P contains a
       countably infinite subset.
*)

theory NonTriviality
  imports Garbage
begin

lemma atoms_of_par_finite: "finite (atoms_of_par P)"
  sorry

lemma atoms_of_name_finite: "finite (atoms_of_name n)"
  sorry

lemma bn_new_par_finite: "finite (bn_new_par P)"
  sorry

theorem nontriviality_gc0:
  shows "infinite (gc0 P)"
  sorry

corollary gc0_nonempty:
  shows "gc0 P \<noteq> {}"
  using nontriviality_gc0 by (auto dest: infinite_imp_nonempty)

end
