(*
  SoundnessGC0.thy --- soundness of the coarse GC0 algorithm.

  Strategy:
    - Define total_atoms cfg = atoms-anywhere U bn_new-anywhere.
    - Show single-step preservation: total_atoms cfg' \<subseteq> total_atoms cfg
      under subst_atom_safe + subst_bn_new_safe + matches_atom_safe +
      matches_bn_new_safe.
    - Lift to multi-step.
    - Apply with gc0 + ctx_wf + ctx_private to derive a contradiction.
*)

theory SoundnessGC0
  imports Garbage NonTriviality
begin

text \<open>The total atom budget of a configuration.\<close>

definition bn_new_in_config :: "config \<Rightarrow> atom set" where
  "bn_new_in_config cfg =
     bn_new_par (cfg_proc cfg)
     \<union> (\<Union>d \<in> set_mset (cfg_datums cfg). bn_new_par (d_payload d))
     \<union> (\<Union>w \<in> set_mset (cfg_waiting cfg).
          bn_new_par (w_pat w) \<union> bn_new_par (w_body w) \<union> bn_new_par (w_guard w))"

definition total_atoms :: "config \<Rightarrow> atom set" where
  "total_atoms cfg = atoms_in_config cfg \<union> bn_new_in_config cfg"

lemma atoms_in_init_config [simp]: "atoms_in_config (init_config Q) = atoms_of_par Q"
  by (simp add: init_config_def atoms_in_config_def)

lemma bn_new_in_init_config [simp]: "bn_new_in_config (init_config Q) = bn_new_par Q"
  by (simp add: init_config_def bn_new_in_config_def)

lemma total_atoms_init: "total_atoms (init_config Q) = atoms_of_par Q \<union> bn_new_par Q"
  by (simp add: total_atoms_def)

text \<open>Substitution + matching atom safety, packaged as one assumption block.\<close>

abbreviation
  "rholang_safe \<equiv> subst_atom_safe \<and> subst_bn_new_safe
                  \<and> matches_atom_safe \<and> matches_bn_new_safe"

text \<open>
  Atoms of a substituted body are bounded by the body's atoms plus the
  pattern's and target's atoms.
\<close>

lemma fm_atoms_match_bound:
  assumes match_safe: matches_atom_safe
  assumes "matches pat tgt fm"
  shows "fm_atoms fm \<subseteq> atoms_of_par pat \<union> atoms_of_par tgt"
proof
  fix x assume "x \<in> fm_atoms fm"
  then obtain v where v_in: "v \<in> ran fm" and x_v: "x \<in> bv_atoms v"
    by (auto simp: fm_atoms_def)
  from v_in obtain i where "fm i = Some v" by (auto simp: ran_def)
  with match_safe assms(2) have "bv_atoms v \<subseteq> atoms_of_par pat \<union> atoms_of_par tgt"
    by (auto simp: matches_atom_safe_def)
  with x_v show "x \<in> atoms_of_par pat \<union> atoms_of_par tgt" by blast
qed

lemma fm_bn_new_match_bound:
  assumes match_safe: matches_bn_new_safe
  assumes "matches pat tgt fm"
  shows "fm_bn_new fm \<subseteq> bn_new_par pat \<union> bn_new_par tgt"
proof
  fix x assume "x \<in> fm_bn_new fm"
  then obtain v where v_in: "v \<in> ran fm" and x_v: "x \<in> bv_bn_new v"
    by (auto simp: fm_bn_new_def)
  from v_in obtain i where "fm i = Some v" by (auto simp: ran_def)
  with match_safe assms(2) have "bv_bn_new v \<subseteq> bn_new_par pat \<union> bn_new_par tgt"
    by (auto simp: matches_bn_new_safe_def)
  with x_v show "x \<in> bn_new_par pat \<union> bn_new_par tgt" by blast
qed

lemma subst_atoms_bound:
  assumes "subst_atom_safe" "matches_atom_safe"
  assumes "matches pat tgt fm"
  shows "atoms_of_par (subst_binding fm body)
         \<subseteq> atoms_of_par body \<union> atoms_of_par pat \<union> atoms_of_par tgt"
proof -
  have "atoms_of_par (subst_binding fm body) \<subseteq> atoms_of_par body \<union> fm_atoms fm"
    using assms(1) by (simp add: subst_atom_safe_def)
  also have "fm_atoms fm \<subseteq> atoms_of_par pat \<union> atoms_of_par tgt"
    using fm_atoms_match_bound[OF assms(2,3)] .
  finally show ?thesis by blast
qed

lemma subst_bn_new_bound:
  assumes "subst_bn_new_safe" "matches_bn_new_safe"
  assumes "matches pat tgt fm"
  shows "bn_new_par (subst_binding fm body)
         \<subseteq> bn_new_par body \<union> bn_new_par pat \<union> bn_new_par tgt"
proof -
  have "bn_new_par (subst_binding fm body) \<subseteq> bn_new_par body \<union> fm_bn_new fm"
    using assms(1) by (simp add: subst_bn_new_safe_def)
  also have "fm_bn_new fm \<subseteq> bn_new_par pat \<union> bn_new_par tgt"
    using fm_bn_new_match_bound[OF assms(2,3)] .
  finally show ?thesis by blast
qed

text \<open>
  The crux: total_atoms is non-increasing across one reduction step.
\<close>

lemma step_total_atoms_subset:
  assumes step: "cfg \<rightarrow>\<langle>e\<rangle> cfg'"
  assumes safe: rholang_safe
  shows "total_atoms cfg' \<subseteq> total_atoms cfg"
  using step
proof (cases rule: step.cases)
  case (ProduceInstall c d persistent cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (ConsumeInstall pat c body persistent peek guard cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (Comm w d frmap fired datums' waiting')
  have w_in: "w \<in># cfg_waiting cfg" and d_in: "d \<in># cfg_datums cfg"
    using Comm by auto
  have match_used: "matches (w_pat w) (d_payload d) frmap"
    using Comm by simp
  have body_atoms:
      "atoms_of_par (subst_binding frmap (w_body w))
       \<subseteq> atoms_of_par (w_body w) \<union> atoms_of_par (w_pat w) \<union> atoms_of_par (d_payload d)"
    using subst_atoms_bound[where pat = "w_pat w" and tgt = "d_payload d"
                                and fm = frmap and body = "w_body w"]
          safe match_used by simp
  have body_bn:
      "bn_new_par (subst_binding frmap (w_body w))
       \<subseteq> bn_new_par (w_body w) \<union> bn_new_par (w_pat w) \<union> bn_new_par (d_payload d)"
    using subst_bn_new_bound[where pat = "w_pat w" and tgt = "d_payload d"
                                 and fm = frmap and body = "w_body w"]
          safe match_used by simp
  have w_atoms: "atoms_of_par (w_body w) \<union> atoms_of_par (w_pat w)
                 \<subseteq> atoms_in_config cfg"
    using w_in by (auto simp: atoms_in_config_def)
  have d_atoms: "atoms_of_par (d_payload d) \<subseteq> atoms_in_config cfg"
    using d_in by (auto simp: atoms_in_config_def)
  have w_bn: "bn_new_par (w_body w) \<union> bn_new_par (w_pat w)
              \<subseteq> bn_new_in_config cfg"
    using w_in by (auto simp: bn_new_in_config_def)
  have d_bn: "bn_new_par (d_payload d) \<subseteq> bn_new_in_config cfg"
    using d_in by (auto simp: bn_new_in_config_def)
  have datums'_sub: "datums' \<subseteq># cfg_datums cfg" using Comm by auto
  have waiting'_sub: "waiting' \<subseteq># cfg_waiting cfg" using Comm by auto
  show ?thesis
    using Comm body_atoms body_bn w_atoms d_atoms w_bn d_bn
          datums'_sub waiting'_sub
    apply (simp add: total_atoms_def atoms_in_config_def bn_new_in_config_def)
    apply (intro conjI subsetI)
    apply (auto dest: mset_subset_eqD)
    done
next
  case (New bound body cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (MatchHit tgt pat gd body fall cont frmap)
  have match_used: "matches pat tgt frmap" using MatchHit by simp
  have body_atoms:
      "atoms_of_par (subst_binding frmap body)
       \<subseteq> atoms_of_par body \<union> atoms_of_par pat \<union> atoms_of_par tgt"
    using subst_atoms_bound[where pat = pat and tgt = tgt and fm = frmap and body = body]
          safe match_used by simp
  have body_bn:
      "bn_new_par (subst_binding frmap body)
       \<subseteq> bn_new_par body \<union> bn_new_par pat \<union> bn_new_par tgt"
    using subst_bn_new_bound[where pat = pat and tgt = tgt and fm = frmap and body = body]
          safe match_used by simp
  show ?thesis
    using MatchHit body_atoms body_bn
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (MatchFallThrough tgt pat gd body fall cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (IfTrue c t e cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (IfFalse c t e cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (EvalQuoteUnquote p cont)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
next
  case (StructComm p q)
  thus ?thesis
    by (auto simp: total_atoms_def atoms_in_config_def bn_new_in_config_def)
qed

text \<open>An EvtComm step records a name whose atoms are within the pre-step total budget.\<close>

lemma step_evt_comm_atoms:
  assumes "cfg \<rightarrow>\<langle>EvtComm c'\<rangle> cfg'"
  shows "atoms_of_name c' \<subseteq> total_atoms cfg"
proof -
  from assms obtain w where w_in: "w \<in># cfg_waiting cfg"
                       and c'_eq: "c' = strip_bundle (w_chan w)"
    by (cases rule: step.cases) auto
  hence "atoms_of_name c' = atoms_of_name (w_chan w)"
    using atoms_of_strip_bundle by simp
  also have "atoms_of_name (w_chan w) \<subseteq> atoms_in_config cfg"
    using w_in by (auto simp: atoms_in_config_def)
  also have "\<dots> \<subseteq> total_atoms cfg"
    by (simp add: total_atoms_def)
  finally show ?thesis .
qed

text \<open>Lift the single-step results to multi-step.\<close>

lemma steps_total_atoms_subset:
  assumes "cfg \<rightarrow>*\<langle>es\<rangle> cfg'"
  assumes safe: rholang_safe
  shows "total_atoms cfg' \<subseteq> total_atoms cfg"
  using assms(1)
proof (induction rule: steps.induct)
  case (steps_refl cfg) thus ?case by simp
next
  case (steps_step cfg0 e cfg1 es cfg2)
  have "total_atoms cfg1 \<subseteq> total_atoms cfg0"
    using steps_step.hyps(1) safe step_total_atoms_subset by blast
  thus ?case using steps_step.IH by blast
qed

lemma steps_evt_comm_atoms:
  assumes "cfg \<rightarrow>*\<langle>es\<rangle> cfg'"
  assumes safe: rholang_safe
  assumes "EvtComm c' \<in> set es"
  shows "atoms_of_name c' \<subseteq> total_atoms cfg"
  using assms(1,3)
proof (induction arbitrary: c' rule: steps.induct)
  case (steps_refl cfg) thus ?case by simp
next
  case (steps_step cfg0 e cfg1 es cfg2)
  consider (here) "e = EvtComm c'" | (later) "EvtComm c' \<in> set es"
    using steps_step.prems by auto
  thus ?case
  proof cases
    case here
    hence "cfg0 \<rightarrow>\<langle>EvtComm c'\<rangle> cfg1" using steps_step.hyps(1) by simp
    thus ?thesis using step_evt_comm_atoms by blast
  next
    case later
    have "atoms_of_name c' \<subseteq> total_atoms cfg1"
      using steps_step.IH[OF later] .
    also have "total_atoms cfg1 \<subseteq> total_atoms cfg0"
      using steps_step.hyps(1) safe step_total_atoms_subset by blast
    finally show ?thesis .
  qed
qed

text \<open>The main soundness theorem for GC0.\<close>

theorem soundness_gc0:
  assumes c_in_gc0: "c \<in> gc0 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
  unfolding is_garbage_def
proof (intro allI impI ballI)
  fix K cfg' es e
  assume wf: "ctx_wf K P"
  assume priv: "ctx_private K c"
  assume reach: "init_config (ctx_plug K P) \<rightarrow>*\<langle>es\<rangle> cfg'"
  assume e_in: "e \<in> set es"
  show "case e of EvtComm c' \<Rightarrow> strip_bundle c' \<noteq> strip_bundle c | _ \<Rightarrow> True"
  proof (cases e)
    case (EvtComm c'')
    have c''_atoms: "atoms_of_name c'' \<subseteq> total_atoms (init_config (ctx_plug K P))"
      using steps_evt_comm_atoms[OF reach safe] e_in EvtComm by auto
    hence c''_in_KP:
        "atoms_of_name c'' \<subseteq> atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)"
      by (simp add: total_atoms_init)
    have ctx_bound: "atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)
                     \<subseteq> ctx_free_atoms K \<union> ctx_bound_atoms K
                       \<union> atoms_of_par P \<union> bn_new_par P"
      using wf by (auto simp: ctx_wf_def)
    from c_in_gc0 have c_disjoint_P:
        "atoms_of_name c \<inter> (atoms_of_par P \<union> bn_new_par P \<union> pub) = {}"
      and c_nonempty: "atoms_of_name c \<noteq> {}"
      by (auto simp: gc0_def)
    from priv have c_disjoint_K:
        "(atoms_of_name c - pub) \<inter> (ctx_free_atoms K \<union> ctx_bound_atoms K) = {}"
      by (simp add: ctx_private_def)
    from c_disjoint_P have c_no_pub: "atoms_of_name c \<inter> pub = {}" by blast
    have c_disjoint_initial:
        "atoms_of_name c \<inter> (atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)) = {}"
    proof (rule ccontr)
      assume "\<not> atoms_of_name c \<inter> (atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)) = {}"
      then obtain a where a_in_c: "a \<in> atoms_of_name c"
                      and a_in_KP: "a \<in> atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)"
        by blast
      from a_in_KP ctx_bound have
          "a \<in> ctx_free_atoms K \<union> ctx_bound_atoms K \<union> atoms_of_par P \<union> bn_new_par P"
        by blast
      moreover from c_disjoint_P a_in_c have a_not_P:
          "a \<notin> atoms_of_par P \<union> bn_new_par P \<union> pub" by blast
      moreover from c_disjoint_K a_in_c c_no_pub have a_not_K:
          "a \<notin> ctx_free_atoms K \<union> ctx_bound_atoms K" by blast
      ultimately show False by blast
    qed
    have "strip_bundle c'' \<noteq> strip_bundle c"
    proof
      assume eq: "strip_bundle c'' = strip_bundle c"
      hence "atoms_of_name c'' = atoms_of_name c"
        using strip_bundle_atoms_eq by blast
      with c''_in_KP have "atoms_of_name c \<subseteq> atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P)"
        by simp
      hence "atoms_of_name c \<inter> (atoms_of_par (ctx_plug K P) \<union> bn_new_par (ctx_plug K P))
             = atoms_of_name c"
        by blast
      with c_disjoint_initial have "atoms_of_name c = {}" by simp
      with c_nonempty show False by simp
    qed
    thus ?thesis using EvtComm by simp
  qed (simp_all)
qed

end
