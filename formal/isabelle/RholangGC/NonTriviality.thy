(*
  NonTriviality.thy --- gc0(P) is countably infinite for every P.
*)

theory NonTriviality
  imports Garbage
begin

text \<open>Finiteness of atom occurrence sets.\<close>

lemma atoms_of_finite:
  fixes n :: name and p :: par
  shows "finite (atoms_of_name n) \<and> finite (atoms_of_par p)"
  by (induction rule: name_par.induct) auto

lemma atoms_of_name_finite [simp]: "finite (atoms_of_name n)"
  using atoms_of_finite by blast

lemma atoms_of_par_finite [simp]: "finite (atoms_of_par p)"
  using atoms_of_finite by blast

lemma bn_new_par_finite_aux:
  "\<forall>p \<in> set ps. finite (bn_new_par p) \<Longrightarrow> finite (\<Union> (set (map bn_new_par ps)))"
  by (induction ps) auto

lemma bn_new_par_finite [simp]: "finite (bn_new_par p)"
proof (induction p)
qed (auto intro: bn_new_par_finite_aux)

text \<open>Injectivity of the \<open>GPrivate\<close> embedding.\<close>

lemma inj_GPrivate: "inj GPrivate"
  by (simp add: inj_on_def)

text \<open>The witness set: atoms outside the finite ``known'' part.\<close>

lemma infinite_witness_atoms:
  "infinite (UNIV - (atoms_of_par P \<union> bn_new_par P \<union> pub))"
proof -
  have fin: "finite (atoms_of_par P \<union> bn_new_par P \<union> pub)"
    using atoms_of_par_finite bn_new_par_finite pub_finite by blast
  show ?thesis
    using infinite_atoms fin
    by (metis Diff_infinite_finite)
qed

text \<open>Each witness atom yields a distinct \<open>GPrivate\<close> in \<open>gc0 P\<close>.\<close>

lemma GPrivate_witness_in_gc0:
  assumes "a \<notin> atoms_of_par P" and "a \<notin> bn_new_par P" and "a \<notin> pub"
  shows "GPrivate a \<in> gc0 P"
  using assms by (auto simp: gc0_def)

theorem nontriviality_gc0:
  shows "infinite (gc0 P)"
proof -
  let ?W = "UNIV - (atoms_of_par P \<union> bn_new_par P \<union> pub)"
  have inj: "inj_on GPrivate ?W"
    using inj_GPrivate by (simp add: inj_on_def)
  have sub: "GPrivate ` ?W \<subseteq> gc0 P"
    by (auto intro: GPrivate_witness_in_gc0)
  have inf_img: "infinite (GPrivate ` ?W)"
    using infinite_witness_atoms inj
    by (simp add: finite_image_iff)
  show ?thesis
    using infinite_super[OF sub inf_img] .
qed

corollary gc0_nonempty:
  shows "gc0 P \<noteq> {}"
  using nontriviality_gc0 by (metis finite.emptyI)

end
