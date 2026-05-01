(*
  SoundnessGC1.thy --- soundness of the escape + one-sided algorithm.

  States: every name in gc1(P) is garbage with respect to P.

  STATUS: Phase-1 partial.  The gc0 component reuses soundness_gc0 (proved
  in SoundnessGC0.thy).  The gc1-only component is split into four
  sub-lemmas matching the four disjuncts of gc1_atom, each carrying its
  own sorry:

    soundness_gc1_only_send_side    (only_send_side disjunct)
    soundness_gc1_only_recv_side    (only_recv_side disjunct)
    soundness_gc1_send_blocked      (send_side_blocked_by_bundles disjunct)
    soundness_gc1_recv_blocked      (recv_side_blocked_by_bundles disjunct)

  Each requires its own preservation invariant on the reduction relation,
  characterizing which sync-channel positions can carry the witness atom
  u under that specific disjunct.  See per-lemma comments for the
  intended invariant and proof strategy.

  The motivating example `new x in { x!(0) }` is captured by
  soundness_gc1_only_send_side: x is retained-private (doesn't escape,
  bound by P's new), only appears in send-channel positions, so no
  reachable configuration ever has a continuation listening on a name
  with x as one of its atoms.
*)

theory SoundnessGC1
  imports SoundnessGC0
begin

text \<open>
  The gc1-only fragment: names with at least one atom flagged by
  \<open>gc1_atom\<close> --- i.e.\ a P-bound atom that is retained-private and has
  one-sided usage (or bundle-blocked refinements).
\<close>

definition gc1_only :: "par \<Rightarrow> name set" where
  "gc1_only P = {c. \<exists>u \<in> atoms_of_name c. gc1_atom P u}"

lemma gc1_decomp: "gc1 P = gc0 P \<union> gc1_only P"
  by (auto simp: gc1_def gc1_only_def)

subsection \<open>Per-disjunct soundness lemmas (each with its own sorry).\<close>

text \<open>
  \<^bold>\<open>only_send_side disjunct.\<close>  When P never syntactically receives on u
  (\<open>u \<notin> sync_chans_recv P\<close>), no continuation in any reachable
  configuration has u in its sync channel.  The argument:
  \<^enum> initially, ctx_plug K P contains only K's recv-channels (which lack
    u by ctx_private) and P's recv-channels (which lack u by
    only_send_side).
  \<^enum> ProduceInstall and ConsumeInstall transfer syntax intact; if the
    proc had no u-recv, neither do datums (no recv-channels) nor the new
    continuation.
  \<^enum> Comm substitutes a free_map into a body.  The resulting body's
    recv-channels come from either: (a) the body's existing recv-
    channels (no u, by induction), or (b) variables substituted by
    fm-values.  fm-values come from matching the pattern against the
    target's payload; the target is a datum's payload, and by escape
    analysis (retained_private) u never appears in any datum payload.
  \<^enum> All other rules don't change recv-channels at all.
  \<^enum> Therefore no Comm step fires on a name with u, since the missing
    receive side simply doesn't exist anywhere.
\<close>

lemma soundness_gc1_only_send_side:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "only_send_side P u"
  shows "is_garbage P c"
  sorry

text \<open>
  \<^bold>\<open>only_recv_side disjunct.\<close>  Symmetric to the previous: P never sends
  on u, so no datum in any reachable configuration has u as its channel
  --- the missing send side cannot come from K (lacks u) or from P
  (only_recv_side).  Escape analysis is again the key for substitution:
  even if a continuation has u in its channel, the matching datum
  cannot exist.
\<close>

lemma soundness_gc1_only_recv_side:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "only_recv_side P u"
  shows "is_garbage P c"
  sorry

text \<open>
  \<^bold>\<open>send_side_blocked_by_bundles disjunct.\<close>  When every send-channel
  position with u is wrapped under a bundle whose write capability is
  blocked (\<open>CapR\<close> or \<open>CapNone\<close>), holders cannot send to that name.  K is
  a holder (has the name as a free name once extruded); P is also bound
  by the same wrapping at the syntactic level (the bundle decoration
  applies to all uses of the name, not just K's).
  This disjunct is the most subtle: it requires reasoning about the
  bundle-capability semantics of the Comm rule, which the current model
  encodes only via \<open>strip_bundle\<close> at channel comparison time.  A
  refined model that consults \<open>bundle_cap_of\<close> at Comm-time is needed
  for a clean discharge; an approximation under the current model may
  still go through with extra side-conditions on \<open>ctx_wf\<close>.
\<close>

lemma soundness_gc1_send_blocked:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "send_side_blocked_by_bundles P u"
  shows "is_garbage P c"
  sorry

text \<open>
  \<^bold>\<open>recv_side_blocked_by_bundles disjunct.\<close>  Symmetric.
\<close>

lemma soundness_gc1_recv_blocked:
  assumes c_in: "c \<in> gc1_only P"
  assumes safe: rholang_safe
  assumes pick: "u \<in> atoms_of_name c"
                "retained_private P u" "recv_side_blocked_by_bundles P u"
  shows "is_garbage P c"
  sorry

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

text \<open>
  When the user's gc1 atom witness is itself a gc0 atom, soundness
  follows from \<open>soundness_gc0\<close>.  This corollary documents that
  containment.
\<close>

corollary soundness_gc1_via_gc0:
  assumes "c \<in> gc0 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
  using assms soundness_gc0 by blast

end
