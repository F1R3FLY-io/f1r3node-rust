(*
  SoundnessGC1.thy --- soundness of the escape + one-sided algorithm.

  States: every name in gc1(P) is garbage with respect to P.

  Phase-1 status:
    - gc0 component reuses soundness_gc0.
    - only_send_side and only_recv_side disjuncts: PROVED via a
      configuration-level "u-clean" preservation invariant.
    - send_side_blocked_by_bundles and recv_side_blocked_by_bundles
      disjuncts remain sorry; they need a refined Comm rule that
      consults bundle_cap_of at sync-time, beyond the strip_bundle
      semantics of the current model.
*)

theory SoundnessGC1
  imports SoundnessGC0
begin

text \<open>
  The gc1-only fragment: names with at least one atom flagged by
  \<open>gc1_atom\<close>.
\<close>

definition gc1_only :: "par \<Rightarrow> name set" where
  "gc1_only P = {c. \<exists>u \<in> atoms_of_name c. gc1_atom P u}"

lemma gc1_decomp: "gc1 P = gc0 P \<union> gc1_only P"
  by (auto simp: gc1_def gc1_only_def)

subsection \<open>Auxiliary: cleanness from atom-disjointness.\<close>

text \<open>
  If \<open>u\<close> doesn't occur in \<open>P\<close>'s atoms at all, then \<open>P\<close> is trivially
  u-send-clean and u-recv-clean.  These let the EvalQuote case lift
  \<open>u \<notin> atoms_of_name (Quote p) = u \<notin> atoms_of_par p\<close> into structural
  cleanliness of the unquoted body.
\<close>

lemma u_clean_of_full_disjoint:
  fixes a :: atom
  shows "(a \<notin> atoms_of_name n \<and> a \<notin> bn_new_name n
            \<longrightarrow> True)
       \<and> (a \<notin> atoms_of_par p \<and> a \<notin> bn_new_par p
            \<longrightarrow> u_send_clean_par p a \<and> u_recv_clean_par p a)"
  by (induction rule: name_par.induct) auto

lemma u_send_clean_of_atoms_disjoint:
  fixes a :: atom
  assumes "a \<notin> atoms_of_par p" "a \<notin> bn_new_par p"
  shows "u_send_clean_par p a"
  using u_clean_of_full_disjoint assms by blast

lemma u_recv_clean_of_atoms_disjoint:
  fixes a :: atom
  assumes "a \<notin> atoms_of_par p" "a \<notin> bn_new_par p"
  shows "u_recv_clean_par p a"
  using u_clean_of_full_disjoint assms by blast

subsection \<open>Auxiliary: u-clean configurations for send-side.\<close>

text \<open>
  A configuration is "u-send-clean" iff (a) the active process is
  \<open>u_send_clean_par\<close>, (b) every datum's payload is u-free in atoms,
  and (c) every waiting continuation has a u-free recv channel,
  u-free pattern and guard, and a \<open>u_send_clean_par\<close> body.
\<close>

definition cfg_send_clean :: "config \<Rightarrow> atom \<Rightarrow> bool" where
  "cfg_send_clean cfg u \<longleftrightarrow>
     u_send_clean_par (cfg_proc cfg) u
     \<and> (\<forall>d \<in># cfg_datums cfg. u \<notin> atoms_of_par (d_payload d))
     \<and> (\<forall>w \<in># cfg_waiting cfg.
            u \<notin> atoms_of_name (w_chan w)
          \<and> u \<notin> atoms_of_par (w_pat w)
          \<and> u \<notin> atoms_of_par (w_guard w)
          \<and> u_send_clean_par (w_body w) u)"

text \<open>Symmetric predicate for the recv-side disjunct.\<close>

definition cfg_recv_clean :: "config \<Rightarrow> atom \<Rightarrow> bool" where
  "cfg_recv_clean cfg u \<longleftrightarrow>
     u_recv_clean_par (cfg_proc cfg) u
     \<and> (\<forall>d \<in># cfg_datums cfg.
            u \<notin> atoms_of_name (d_chan d)
          \<and> u \<notin> atoms_of_par (d_payload d))
     \<and> (\<forall>w \<in># cfg_waiting cfg.
            u \<notin> atoms_of_par (w_pat w)
          \<and> u \<notin> atoms_of_par (w_guard w)
          \<and> u_recv_clean_par (w_body w) u)"

subsection \<open>u-clean implies no Comm on u-channel.\<close>

text \<open>
  If a configuration is u-send-clean, every waiting continuation has a
  u-free recv channel.  A Comm step fires on \<open>strip_bundle (w_chan w)\<close>
  for some waiting w; since strip_bundle preserves atoms, the fired
  channel has no u atom either.
\<close>

lemma cfg_send_clean_no_comm_on_u:
  assumes inv: "cfg_send_clean cfg u"
  assumes step: "cfg \<rightarrow>\<langle>EvtComm c'\<rangle> cfg'"
  shows "u \<notin> atoms_of_name c'"
proof -
  from step obtain w where w_in: "w \<in># cfg_waiting cfg"
                       and c'_eq: "c' = strip_bundle (w_chan w)"
    by (cases rule: step.cases) auto
  from inv w_in have "u \<notin> atoms_of_name (w_chan w)"
    by (auto simp: cfg_send_clean_def)
  thus ?thesis using c'_eq atoms_of_strip_bundle by simp
qed

lemma cfg_recv_clean_no_comm_on_u:
  assumes inv: "cfg_recv_clean cfg u"
  assumes step: "cfg \<rightarrow>\<langle>EvtComm c'\<rangle> cfg'"
  shows "u \<notin> atoms_of_name c'"
proof -
  from step obtain d where d_in: "d \<in># cfg_datums cfg"
                       and c'_eq: "c' = strip_bundle (d_chan d)"
    by (cases rule: step.cases) auto
  from inv d_in have "u \<notin> atoms_of_name (d_chan d)"
    by (auto simp: cfg_recv_clean_def)
  thus ?thesis using c'_eq atoms_of_strip_bundle by simp
qed

subsection \<open>u-clean is preserved by every reduction step.\<close>

text \<open>
  Auxiliary: if a process is u_send_clean and its atom-content does not
  include u in non-channel positions, then sub-processes inherit the
  cleanliness.  The structural primrec equations below give us the
  individual elimination steps; the general preservation lemma follows
  by case analysis on the reduction rule.
\<close>

text \<open>
  The Comm rule introduces a substituted body.  The body satisfied
  u_send_clean before the consume, so the substituted body must also be
  u_send_clean.  This requires constraints on \<open>matches\<close>: any value bound
  by the matcher, when substituted into a u_send_clean body, must
  preserve cleanliness.  Under the assumptions \<open>matches_atom_safe\<close> and
  \<open>subst_atom_safe\<close>, plus the configuration-level invariant that no datum
  payload contains u, no fm-value contains u; substitution into a
  u-clean body therefore yields a u-clean body.

  We capture this as an axiom on substitution: since fm-values contain
  no u (by atom safety + clean datums), substituting them does not
  introduce u anywhere.  Phase-1' could discharge this from a more
  detailed substitution model.
\<close>

axiomatization where
  subst_preserves_send_clean:
    "u \<notin> fm_atoms fm \<Longrightarrow> u_send_clean_par body u
     \<Longrightarrow> u_send_clean_par (subst_binding fm body) u" and
  subst_preserves_recv_clean:
    "u \<notin> fm_atoms fm \<Longrightarrow> u_recv_clean_par body u
     \<Longrightarrow> u_recv_clean_par (subst_binding fm body) u" and
  subst_preserves_atom_absence:
    "subst_atom_safe \<Longrightarrow> u \<notin> atoms_of_par body \<Longrightarrow> u \<notin> fm_atoms fm
     \<Longrightarrow> u \<notin> atoms_of_par (subst_binding fm body)"

lemma fm_atoms_no_u:
  assumes match_safe: matches_atom_safe
  assumes match: "matches pat tgt fm"
  assumes "u \<notin> atoms_of_par pat" "u \<notin> atoms_of_par tgt"
  shows "u \<notin> fm_atoms fm"
  using fm_atoms_match_bound[OF match_safe match] assms by blast

lemma cfg_send_clean_step_preserved:
  assumes safe: rholang_safe
  assumes inv: "cfg_send_clean cfg u"
  assumes step: "cfg \<rightarrow>\<langle>e\<rangle> cfg'"
  shows "cfg_send_clean cfg' u"
  using step
proof (cases rule: step.cases)
  case (ProduceInstall c d persistent cont)
  \<comment> \<open>Send becomes datum.  Original \<open>Send c d _\<close> was inside cfg_proc and
      thus u_send_clean: u_send_clean_par d u and u \<notin> atoms_of_par d.\<close>
  from ProduceInstall inv have proc_clean: "u_send_clean_par (PPar (Send c d persistent) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par (Send c d persistent) u" and "u_send_clean_par cont u"
    by simp_all
  hence d_no_u: "u \<notin> atoms_of_par d"
    by simp
  show ?thesis
    using ProduceInstall inv proc_clean d_no_u
    by (auto simp: cfg_send_clean_def)
next
  case (ConsumeInstall pat c body persistent peek guard cont)
  \<comment> \<open>Recv becomes waiting continuation.\<close>
  from ConsumeInstall inv have proc_clean:
      "u_send_clean_par (PPar (Recv pat c body persistent peek guard) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par (Recv pat c body persistent peek guard) u"
    and cont_clean: "u_send_clean_par cont u" by simp_all
  hence c_no_u: "u \<notin> atoms_of_name c"
    and pat_no_u: "u \<notin> atoms_of_par pat"
    and guard_no_u: "u \<notin> atoms_of_par guard"
    and body_clean: "u_send_clean_par body u" by simp_all
  show ?thesis
    using ConsumeInstall inv cont_clean c_no_u pat_no_u guard_no_u body_clean
    by (auto simp: cfg_send_clean_def)
next
  case (Comm w d frmap fired datums' waiting')
  from Comm inv have w_no_u: "u \<notin> atoms_of_name (w_chan w)"
                 and pat_no_u: "u \<notin> atoms_of_par (w_pat w)"
                 and guard_no_u: "u \<notin> atoms_of_par (w_guard w)"
                 and body_clean: "u_send_clean_par (w_body w) u"
                 and proc_clean: "u_send_clean_par (cfg_proc cfg) u"
    by (auto simp: cfg_send_clean_def)
  from Comm inv have d_no_u: "u \<notin> atoms_of_par (d_payload d)"
    by (auto simp: cfg_send_clean_def)
  have match: "matches (w_pat w) (d_payload d) frmap"
    using Comm by simp
  have fm_no_u: "u \<notin> fm_atoms frmap"
    using fm_atoms_no_u[OF _ match pat_no_u d_no_u] safe by simp
  hence subst_clean: "u_send_clean_par (subst_binding frmap (w_body w)) u"
    using subst_preserves_send_clean body_clean by blast
  have new_proc_clean:
      "u_send_clean_par (PPar (subst_binding frmap (w_body w)) (cfg_proc cfg)) u"
    using subst_clean proc_clean by simp
  \<comment> \<open>datums' is a sub-multiset of cfg_datums; waiting' a sub-multiset of cfg_waiting.\<close>
  have d'_sub: "datums' \<subseteq># cfg_datums cfg" using Comm by auto
  have w'_sub: "waiting' \<subseteq># cfg_waiting cfg" using Comm by auto
  show ?thesis
    using Comm inv new_proc_clean d'_sub w'_sub
    by (auto simp: cfg_send_clean_def dest: mset_subset_eqD)
next
  case (New bound body cont)
  \<comment> \<open>NewN fires.  cfg_proc was \<open>PPar (NewN bound body) cont\<close>, becomes
      \<open>PPar body cont\<close>.  By definition u_send_clean_par (NewN _ body) =
      u_send_clean_par body, so u_send_clean is preserved.\<close>
  from New inv have "u_send_clean_par (PPar (NewN bound body) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par body u" "u_send_clean_par cont u" by simp_all
  thus ?thesis using New inv by (auto simp: cfg_send_clean_def)
next
  case (MatchHit tgt pat gd body fall cont frmap)
  from MatchHit inv have proc_clean:
      "u_send_clean_par (PPar (MatchOne tgt pat gd body fall) cont) u"
    by (simp add: cfg_send_clean_def)
  hence tgt_no_u: "u \<notin> atoms_of_par tgt"
    and pat_no_u: "u \<notin> atoms_of_par pat"
    and gd_no_u: "u \<notin> atoms_of_par gd"
    and body_clean: "u_send_clean_par body u"
    and fall_clean: "u_send_clean_par fall u"
    and cont_clean: "u_send_clean_par cont u" by simp_all
  have match: "matches pat tgt frmap" using MatchHit by simp
  have fm_no_u: "u \<notin> fm_atoms frmap"
    using fm_atoms_no_u[OF _ match pat_no_u tgt_no_u] safe by simp
  hence subst_clean: "u_send_clean_par (subst_binding frmap body) u"
    using subst_preserves_send_clean body_clean by blast
  show ?thesis
    using MatchHit inv subst_clean cont_clean
    by (auto simp: cfg_send_clean_def)
next
  case (MatchFallThrough tgt pat gd body fall cont)
  from MatchFallThrough inv have
      "u_send_clean_par (PPar (MatchOne tgt pat gd body fall) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par fall u" "u_send_clean_par cont u" by simp_all
  thus ?thesis using MatchFallThrough inv by (auto simp: cfg_send_clean_def)
next
  case (IfTrue c t e cont)
  from IfTrue inv have "u_send_clean_par (PPar (IfThenElse c t e) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par t u" "u_send_clean_par cont u" by simp_all
  thus ?thesis using IfTrue inv by (auto simp: cfg_send_clean_def)
next
  case (IfFalse c t e cont)
  from IfFalse inv have "u_send_clean_par (PPar (IfThenElse c t e) cont) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par e u" "u_send_clean_par cont u" by simp_all
  thus ?thesis using IfFalse inv by (auto simp: cfg_send_clean_def)
next
  case (EvalQuoteUnquote p cont)
  from EvalQuoteUnquote inv have
      "u_send_clean_par (PPar (EvalQuote (Quote p)) cont) u"
    by (simp add: cfg_send_clean_def)
  hence p_no_u: "u \<notin> atoms_of_par p" and p_no_bn: "u \<notin> bn_new_par p"
    and cont_clean: "u_send_clean_par cont u"
    by simp_all
  have "u_send_clean_par p u"
    using u_send_clean_of_atoms_disjoint[OF p_no_u p_no_bn] .
  thus ?thesis using EvalQuoteUnquote inv cont_clean
    by (auto simp: cfg_send_clean_def)
next
  case (StructComm p q)
  from StructComm inv have "u_send_clean_par (PPar p q) u"
    by (simp add: cfg_send_clean_def)
  hence "u_send_clean_par p u" "u_send_clean_par q u" by simp_all
  thus ?thesis using StructComm inv by (auto simp: cfg_send_clean_def)
qed

lemma cfg_recv_clean_step_preserved:
  assumes safe: rholang_safe
  assumes inv: "cfg_recv_clean cfg u"
  assumes step: "cfg \<rightarrow>\<langle>e\<rangle> cfg'"
  shows "cfg_recv_clean cfg' u"
  using step
proof (cases rule: step.cases)
  case (ProduceInstall c d persistent cont)
  from ProduceInstall inv have proc_clean: "u_recv_clean_par (PPar (Send c d persistent) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par (Send c d persistent) u" "u_recv_clean_par cont u" by simp_all
  hence c_no_u: "u \<notin> atoms_of_name c" and d_no_u: "u \<notin> atoms_of_par d"
    by simp_all
  show ?thesis
    using ProduceInstall inv proc_clean c_no_u d_no_u
    by (auto simp: cfg_recv_clean_def)
next
  case (ConsumeInstall pat c body persistent peek guard cont)
  from ConsumeInstall inv have proc_clean:
      "u_recv_clean_par (PPar (Recv pat c body persistent peek guard) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par (Recv pat c body persistent peek guard) u"
    and cont_clean: "u_recv_clean_par cont u" by simp_all
  hence pat_no_u: "u \<notin> atoms_of_par pat"
    and guard_no_u: "u \<notin> atoms_of_par guard"
    and body_clean: "u_recv_clean_par body u" by simp_all
  show ?thesis
    using ConsumeInstall inv cont_clean pat_no_u guard_no_u body_clean
    by (auto simp: cfg_recv_clean_def)
next
  case (Comm w d frmap fired datums' waiting')
  from Comm inv have pat_no_u: "u \<notin> atoms_of_par (w_pat w)"
                 and guard_no_u: "u \<notin> atoms_of_par (w_guard w)"
                 and body_clean: "u_recv_clean_par (w_body w) u"
                 and proc_clean: "u_recv_clean_par (cfg_proc cfg) u"
                 and d_chan_no_u: "u \<notin> atoms_of_name (d_chan d)"
                 and d_no_u: "u \<notin> atoms_of_par (d_payload d)"
    by (auto simp: cfg_recv_clean_def)
  have match: "matches (w_pat w) (d_payload d) frmap" using Comm by simp
  have fm_no_u: "u \<notin> fm_atoms frmap"
    using fm_atoms_no_u[OF _ match pat_no_u d_no_u] safe by simp
  hence subst_clean: "u_recv_clean_par (subst_binding frmap (w_body w)) u"
    using subst_preserves_recv_clean body_clean by blast
  have new_proc_clean:
      "u_recv_clean_par (PPar (subst_binding frmap (w_body w)) (cfg_proc cfg)) u"
    using subst_clean proc_clean by simp
  have d'_sub: "datums' \<subseteq># cfg_datums cfg" using Comm by auto
  have w'_sub: "waiting' \<subseteq># cfg_waiting cfg" using Comm by auto
  show ?thesis
    using Comm inv new_proc_clean d'_sub w'_sub
    by (auto simp: cfg_recv_clean_def dest: mset_subset_eqD)
next
  case (New bound body cont)
  from New inv have "u_recv_clean_par (PPar (NewN bound body) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par body u" "u_recv_clean_par cont u" by simp_all
  thus ?thesis using New inv by (auto simp: cfg_recv_clean_def)
next
  case (MatchHit tgt pat gd body fall cont frmap)
  from MatchHit inv have proc_clean:
      "u_recv_clean_par (PPar (MatchOne tgt pat gd body fall) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence tgt_no_u: "u \<notin> atoms_of_par tgt"
    and pat_no_u: "u \<notin> atoms_of_par pat"
    and body_clean: "u_recv_clean_par body u"
    and cont_clean: "u_recv_clean_par cont u" by simp_all
  have match: "matches pat tgt frmap" using MatchHit by simp
  have fm_no_u: "u \<notin> fm_atoms frmap"
    using fm_atoms_no_u[OF _ match pat_no_u tgt_no_u] safe by simp
  hence subst_clean: "u_recv_clean_par (subst_binding frmap body) u"
    using subst_preserves_recv_clean body_clean by blast
  show ?thesis
    using MatchHit inv subst_clean cont_clean
    by (auto simp: cfg_recv_clean_def)
next
  case (MatchFallThrough tgt pat gd body fall cont)
  from MatchFallThrough inv have
      "u_recv_clean_par (PPar (MatchOne tgt pat gd body fall) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par fall u" "u_recv_clean_par cont u" by simp_all
  thus ?thesis using MatchFallThrough inv by (auto simp: cfg_recv_clean_def)
next
  case (IfTrue c t e cont)
  from IfTrue inv have "u_recv_clean_par (PPar (IfThenElse c t e) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par t u" "u_recv_clean_par cont u" by simp_all
  thus ?thesis using IfTrue inv by (auto simp: cfg_recv_clean_def)
next
  case (IfFalse c t e cont)
  from IfFalse inv have "u_recv_clean_par (PPar (IfThenElse c t e) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par e u" "u_recv_clean_par cont u" by simp_all
  thus ?thesis using IfFalse inv by (auto simp: cfg_recv_clean_def)
next
  case (EvalQuoteUnquote p cont)
  from EvalQuoteUnquote inv have
      "u_recv_clean_par (PPar (EvalQuote (Quote p)) cont) u"
    by (simp add: cfg_recv_clean_def)
  hence p_no_u: "u \<notin> atoms_of_par p" and p_no_bn: "u \<notin> bn_new_par p"
    and cont_clean: "u_recv_clean_par cont u"
    by simp_all
  have "u_recv_clean_par p u"
    using u_recv_clean_of_atoms_disjoint[OF p_no_u p_no_bn] .
  thus ?thesis using EvalQuoteUnquote inv cont_clean
    by (auto simp: cfg_recv_clean_def)
next
  case (StructComm p q)
  from StructComm inv have "u_recv_clean_par (PPar p q) u"
    by (simp add: cfg_recv_clean_def)
  hence "u_recv_clean_par p u" "u_recv_clean_par q u" by simp_all
  thus ?thesis using StructComm inv by (auto simp: cfg_recv_clean_def)
qed

text \<open>Lift preservation to multi-step.\<close>

lemma cfg_send_clean_steps_preserved:
  assumes "cfg \<rightarrow>*\<langle>es\<rangle> cfg'"
  assumes safe: rholang_safe
  assumes inv: "cfg_send_clean cfg u"
  shows "cfg_send_clean cfg' u"
  using assms(1) inv
proof (induction rule: steps.induct)
  case (steps_refl cfg) thus ?case by simp
next
  case (steps_step cfg0 e cfg1 es cfg2)
  have "cfg_send_clean cfg1 u"
    using cfg_send_clean_step_preserved[OF safe steps_step.prems steps_step.hyps(1)] .
  thus ?case using steps_step.IH by blast
qed

lemma cfg_recv_clean_steps_preserved:
  assumes "cfg \<rightarrow>*\<langle>es\<rangle> cfg'"
  assumes safe: rholang_safe
  assumes inv: "cfg_recv_clean cfg u"
  shows "cfg_recv_clean cfg' u"
  using assms(1) inv
proof (induction rule: steps.induct)
  case (steps_refl cfg) thus ?case by simp
next
  case (steps_step cfg0 e cfg1 es cfg2)
  have "cfg_recv_clean cfg1 u"
    using cfg_recv_clean_step_preserved[OF safe steps_step.prems steps_step.hyps(1)] .
  thus ?case using steps_step.IH by blast
qed

subsection \<open>Initial-config cleanliness from \<open>ctx_wf\<close> + \<open>ctx_private\<close>.\<close>

text \<open>
  When K is well-formed and keeps c private, the plugged process is
  u-send-clean (resp.\ u-recv-clean) provided P itself is.  Captured as
  axioms relating the abstract \<open>ctx_plug\<close> to the cleanliness predicates;
  any concrete context implementation must satisfy these for the
  soundness theorems to apply.
\<close>

axiomatization where
  ctx_plug_send_clean:
    "ctx_wf K P \<Longrightarrow> ctx_private K c \<Longrightarrow>
     u \<in> atoms_of_name c \<Longrightarrow> u \<notin> pub \<Longrightarrow>
     u_send_clean_par P u \<Longrightarrow>
     u_send_clean_par (ctx_plug K P) u" and
  ctx_plug_recv_clean:
    "ctx_wf K P \<Longrightarrow> ctx_private K c \<Longrightarrow>
     u \<in> atoms_of_name c \<Longrightarrow> u \<notin> pub \<Longrightarrow>
     u_recv_clean_par P u \<Longrightarrow>
     u_recv_clean_par (ctx_plug K P) u"

subsection \<open>List-decomposition: a step in a trace splits into prefix + step + suffix.\<close>

lemma steps_split_at_event_ex:
  assumes "cfg \<rightarrow>*\<langle>es\<rangle> cfg'"
  assumes "e \<in> set es"
  shows "\<exists>cfg0 cfg1 es1 es2.
           cfg \<rightarrow>*\<langle>es1\<rangle> cfg0 \<and> cfg0 \<rightarrow>\<langle>e\<rangle> cfg1 \<and> cfg1 \<rightarrow>*\<langle>es2\<rangle> cfg' \<and> es = es1 @ e # es2"
  using assms
proof (induction rule: steps.induct)
  case (steps_refl cfg) thus ?case by simp
next
  case (steps_step cfg0 e' cfg1 es' cfg2)
  show ?case
  proof (cases "e = e'")
    case True
    have "cfg0 \<rightarrow>*\<langle>[]\<rangle> cfg0" by (rule steps.steps_refl)
    moreover have "cfg0 \<rightarrow>\<langle>e\<rangle> cfg1" using steps_step.hyps(1) True by simp
    moreover have "cfg1 \<rightarrow>*\<langle>es'\<rangle> cfg2" using steps_step.hyps(2) by simp
    moreover have "e' # es' = [] @ e # es'" using True by simp
    ultimately show ?thesis by blast
  next
    case False
    hence e_in_es': "e \<in> set es'" using steps_step.prems by simp
    obtain cfg_a cfg_b es_a es_b where
        prefix_es': "cfg1 \<rightarrow>*\<langle>es_a\<rangle> cfg_a"
        and step_e: "cfg_a \<rightarrow>\<langle>e\<rangle> cfg_b"
        and suffix_es': "cfg_b \<rightarrow>*\<langle>es_b\<rangle> cfg2"
        and split_es': "es' = es_a @ e # es_b"
      using steps_step.IH[OF e_in_es'] by blast
    have prefix: "cfg0 \<rightarrow>*\<langle>e' # es_a\<rangle> cfg_a"
      using steps.steps_step[OF steps_step.hyps(1) prefix_es'] .
    have "e' # es' = (e' # es_a) @ e # es_b"
      using split_es' by simp
    thus ?thesis using prefix step_e suffix_es' by blast
  qed
qed

subsection \<open>Per-disjunct soundness lemmas.\<close>

lemma soundness_gc1_only_send_side:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "only_send_side P u"
  shows "is_garbage P c"
  unfolding is_garbage_def
proof (intro allI impI ballI)
  fix K cfg' es e
  assume wf: "ctx_wf K P"
  assume priv: "ctx_private K c"
  assume reach: "init_config (ctx_plug K P) \<rightarrow>*\<langle>es\<rangle> cfg'"
  assume e_in: "e \<in> set es"
  from pick have u_no_pub: "u \<notin> pub" by (simp add: retained_private_def)
  from pick have only_send: "u_send_clean_par P u"
    by (simp add: only_send_side_def)
  have plug_clean: "u_send_clean_par (ctx_plug K P) u"
    using ctx_plug_send_clean wf priv pick(1) u_no_pub only_send by blast
  have init_inv: "cfg_send_clean (init_config (ctx_plug K P)) u"
    by (simp add: cfg_send_clean_def init_config_def plug_clean)
  have cfg'_inv: "cfg_send_clean cfg' u"
    using cfg_send_clean_steps_preserved[OF reach safe init_inv] .
  show "case e of EvtComm c' \<Rightarrow> strip_bundle c' \<noteq> strip_bundle c | _ \<Rightarrow> True"
  proof (cases e)
    case (EvtComm c'')
    obtain cfg0 cfg1 es1 es2 where
        prefix: "init_config (ctx_plug K P) \<rightarrow>*\<langle>es1\<rangle> cfg0"
       and step_e: "cfg0 \<rightarrow>\<langle>e\<rangle> cfg1"
       and suffix: "cfg1 \<rightarrow>*\<langle>es2\<rangle> cfg'"
      using steps_split_at_event_ex[OF reach e_in] by blast
    have cfg0_inv: "cfg_send_clean cfg0 u"
      using cfg_send_clean_steps_preserved[OF prefix safe init_inv] .
    have "u \<notin> atoms_of_name c''"
      using cfg_send_clean_no_comm_on_u[OF cfg0_inv]
      using step_e EvtComm by simp
    moreover have "u \<in> atoms_of_name c" using pick(1) .
    ultimately have "atoms_of_name c'' \<noteq> atoms_of_name c" by blast
    hence "strip_bundle c'' \<noteq> strip_bundle c"
      using strip_bundle_atoms_eq by metis
    thus ?thesis using EvtComm by simp
  qed (auto)
qed

lemma soundness_gc1_only_recv_side:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "only_recv_side P u"
  shows "is_garbage P c"
  unfolding is_garbage_def
proof (intro allI impI ballI)
  fix K cfg' es e
  assume wf: "ctx_wf K P"
  assume priv: "ctx_private K c"
  assume reach: "init_config (ctx_plug K P) \<rightarrow>*\<langle>es\<rangle> cfg'"
  assume e_in: "e \<in> set es"
  from pick have u_no_pub: "u \<notin> pub" by (simp add: retained_private_def)
  from pick have only_recv: "u_recv_clean_par P u"
    by (simp add: only_recv_side_def)
  have plug_clean: "u_recv_clean_par (ctx_plug K P) u"
    using ctx_plug_recv_clean wf priv pick(1) u_no_pub only_recv by blast
  have init_inv: "cfg_recv_clean (init_config (ctx_plug K P)) u"
    by (simp add: cfg_recv_clean_def init_config_def plug_clean)
  show "case e of EvtComm c' \<Rightarrow> strip_bundle c' \<noteq> strip_bundle c | _ \<Rightarrow> True"
  proof (cases e)
    case (EvtComm c'')
    obtain cfg0 cfg1 es1 es2 where
        prefix: "init_config (ctx_plug K P) \<rightarrow>*\<langle>es1\<rangle> cfg0"
       and step_e: "cfg0 \<rightarrow>\<langle>e\<rangle> cfg1"
       and suffix: "cfg1 \<rightarrow>*\<langle>es2\<rangle> cfg'"
      using steps_split_at_event_ex[OF reach e_in] by blast
    have cfg0_inv: "cfg_recv_clean cfg0 u"
      using cfg_recv_clean_steps_preserved[OF prefix safe init_inv] .
    have "u \<notin> atoms_of_name c''"
      using cfg_recv_clean_no_comm_on_u[OF cfg0_inv]
      using step_e EvtComm by simp
    moreover have "u \<in> atoms_of_name c" using pick(1) .
    ultimately have "atoms_of_name c'' \<noteq> atoms_of_name c" by blast
    hence "strip_bundle c'' \<noteq> strip_bundle c"
      using strip_bundle_atoms_eq by metis
    thus ?thesis using EvtComm by simp
  qed (auto)
qed

lemma soundness_gc1_send_blocked:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "send_side_blocked_by_bundles P u"
  shows "is_garbage P c"
  sorry  \<comment> \<open>Bundle-aware Comm rule needed; deferred to Phase-1'.\<close>

lemma soundness_gc1_recv_blocked:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "recv_side_blocked_by_bundles P u"
  shows "is_garbage P c"
  sorry  \<comment> \<open>Bundle-aware Comm rule needed; deferred to Phase-1'.\<close>

subsection \<open>Combining the disjuncts.\<close>

lemma soundness_gc1_only:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
proof -
  from c_in obtain u where u_in: "u \<in> atoms_of_name c"
                       and gc1u: "gc1_atom P u"
    by (auto simp: gc1_only_def)
  from gc1u have priv: "retained_private P u"
    and side: "only_send_side P u \<or> only_recv_side P u
               \<or> send_side_blocked_by_bundles P u
               \<or> recv_side_blocked_by_bundles P u"
    by (auto simp: gc1_atom_def)
  from side show ?thesis
  proof (elim disjE)
    assume "only_send_side P u"
    thus ?thesis
      using c_in safe u_in priv soundness_gc1_only_send_side by blast
  next
    assume "only_recv_side P u"
    thus ?thesis
      using c_in safe u_in priv soundness_gc1_only_recv_side by blast
  next
    assume "send_side_blocked_by_bundles P u"
    thus ?thesis
      using c_in safe u_in priv soundness_gc1_send_blocked by blast
  next
    assume "recv_side_blocked_by_bundles P u"
    thus ?thesis
      using c_in safe u_in priv soundness_gc1_recv_blocked by blast
  qed
qed

theorem soundness_gc1:
  assumes c_in_gc1: "c \<in> gc1 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
proof -
  from c_in_gc1 have "c \<in> gc0 P \<or> c \<in> gc1_only P"
    using gc1_decomp by blast
  thus ?thesis
  proof
    assume "c \<in> gc0 P"
    thus ?thesis using safe soundness_gc0 by blast
  next
    assume "c \<in> gc1_only P"
    thus ?thesis using safe soundness_gc1_only by blast
  qed
qed

corollary soundness_gc1_via_gc0:
  assumes "c \<in> gc0 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
  using assms soundness_gc0 by blast

end
