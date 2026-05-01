(*
  Garbage.thy --- the garbage relation and the GC0 / GC1 algorithms.

  Definitions:
    is_garbage :: par => name => bool
    gc0        :: par => name set
    gc1        :: par => name set

  See docs/discoveries/rholang-gc-design.md sections 2.4 and 3 for the
  intended reading.
*)

theory Garbage
  imports FreeNames
begin

text \<open>
  Contexts are processes with a single hole.  Plugging substitutes the hole
  for the plugged process.  We capture the standard \<open>\<alpha>\<close>-convention by
  requiring that contexts are well-formed in Nominal2's sense; a refinement
  may switch to a Honda--Yoshida-style cofinite quantification later.
\<close>

typedecl ctx
consts ctx_plug :: "ctx \<Rightarrow> par \<Rightarrow> par"
consts ctx_atoms :: "ctx \<Rightarrow> atom set"
  \<comment> \<open>The atoms occurring free in the context, after \<open>\<alpha>\<close>-renaming the
      plugged process so its bound atoms are fresh w.r.t.\ \<open>K\<close>.\<close>

text \<open>
  The K-forgeability of a name relative to a context.
\<close>

definition ctx_forgeable :: "ctx \<Rightarrow> name \<Rightarrow> bool" where
  "ctx_forgeable K c \<longleftrightarrow> forgeable_by c (ctx_atoms K)"

text \<open>
  The garbage relation.  A name \<open>c\<close> is garbage with respect to \<open>P\<close> iff for
  every context \<open>K\<close> that cannot forge \<open>c\<close>, no future of \<open>K[P]\<close> records a
  COMM event on \<open>c\<close>.  ``Future'' is captured by the multi-step relation
  \<^const>\<open>steps\<close>; ``records a COMM on c'' is the existence of an
  \<^const>\<open>EvtComm\<close> label whose name (after stripping bundles) equals
  \<^term>\<open>strip_bundle c\<close>.
\<close>

definition is_garbage :: "par \<Rightarrow> name \<Rightarrow> bool" where
  "is_garbage P c \<longleftrightarrow>
     (\<forall>K cfg' es.
        \<not> ctx_forgeable K c \<longrightarrow>
        (init_config (ctx_plug K P)) \<rightarrow>*\<langle>es\<rangle> cfg' \<longrightarrow>
        (\<forall>e \<in> set es.
           case e of
             EvtComm c' \<Rightarrow> strip_bundle c' \<noteq> strip_bundle c
           | _          \<Rightarrow> True))"

subsection \<open>GC0: the coarse algorithm.\<close>

text \<open>
  \<^const>\<open>gc0\<close> reports as garbage every name whose atom set contains an atom
  that is neither in \<open>P\<close>'s syntax nor in the public ambient set nor bound
  by any \<open>new\<close> in \<open>P\<close>.  This is the workhorse of non-triviality: such atoms
  always exist by cardinality.
\<close>

definition gc0 :: "par \<Rightarrow> name set" where
  "gc0 P =
     {c. \<exists>a \<in> atoms_of_name c.
            a \<notin> atoms_of_par P
          \<and> a \<notin> pub
          \<and> a \<notin> bn_new_par P}"

subsection \<open>GC1: escape and one-sided analysis.\<close>

text \<open>
  An atom \<open>u\<close> bound by some \<open>new\<close> in \<open>P\<close> is \<^emph>\<open>retained-private\<close> iff it does
  not escape via any payload.  If retained-private and either (a) \<open>P\<close> only
  syncs on it as a sender, or (b) only as a receiver, or (c) not at all,
  then no COMM on a name carrying \<open>u\<close> can fire.
\<close>

definition retained_private :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "retained_private P u \<longleftrightarrow> u \<in> bn_new_par P \<and> \<not> escapes_in_par P u"

definition only_send_side :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "only_send_side P u \<longleftrightarrow>
     (\<forall>n \<in> sync_chans_recv P. u \<notin> atoms_of_name n)"

definition only_recv_side :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "only_recv_side P u \<longleftrightarrow>
     (\<forall>n \<in> sync_chans_send P. u \<notin> atoms_of_name n)"

text \<open>
  Bundle-aware refinement: if every occurrence of \<open>u\<close> in a sync-channel
  position is wrapped under \<open>bundle+\<close>, the holders cannot send to it; etc.
  We capture this with two predicates expressing the negation of the
  forbidden side after bundle effects.
\<close>

definition send_side_blocked_by_bundles :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "send_side_blocked_by_bundles P u \<longleftrightarrow>
     (\<forall>n \<in> sync_chans_send P. u \<in> atoms_of_name n
        \<longrightarrow> bundle_cap_of n \<in> {CapR, CapNone})"

definition recv_side_blocked_by_bundles :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "recv_side_blocked_by_bundles P u \<longleftrightarrow>
     (\<forall>n \<in> sync_chans_recv P. u \<in> atoms_of_name n
        \<longrightarrow> bundle_cap_of n \<in> {CapW, CapNone})"

definition gc1_atom :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "gc1_atom P u \<longleftrightarrow>
     retained_private P u
     \<and> ( only_send_side P u
       \<or> only_recv_side P u
       \<or> send_side_blocked_by_bundles P u
       \<or> recv_side_blocked_by_bundles P u )"

definition gc1 :: "par \<Rightarrow> name set" where
  "gc1 P = gc0 P
         \<union> {c. \<exists>u \<in> atoms_of_name c. gc1_atom P u}"

text \<open>
  Sanity: GC1 strictly extends GC0 (by construction, as a union).
\<close>

lemma gc0_subset_gc1: "gc0 P \<subseteq> gc1 P"
  unfolding gc1_def by blast

end
