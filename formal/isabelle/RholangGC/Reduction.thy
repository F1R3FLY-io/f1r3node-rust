(*
  Reduction.thy --- labelled small-step reduction.

  Each step is labelled by an event of type \<open>event\<close>.  The COMM(c)
  observable, recorded as \<open>EvtComm\<close>, is the unit of observation for the
  garbage-collection question.

  Phase-1 simplification: single-payload sends, single-pattern receives,
  single-arm matches with explicit fall-through.  This is enough to
  faithfully model the GC-relevant reductions.  Multi-bind joins are not
  modeled directly in the core; they reduce to repeated atomic Comm
  events on a series of names, which is the relevant abstraction for GC.

  Substitution under bindings is left abstract (\<open>subst_binding\<close>) and is a
  parameter of the model: once a match yields a free_map, the body is
  substituted accordingly.  The atom-conservation property required by
  the soundness arguments holds independently of which substitution is
  used, provided that substitution does not introduce private atoms.
*)

theory Reduction
  imports RSpace
begin

text \<open>Observable labels.\<close>

datatype event
  = EvtTau                   \<comment> \<open>silent step (structural, expression eval, ...)\<close>
  | EvtNew "atom set"        \<comment> \<open>fresh allocation by \<open>NewN\<close>\<close>
  | EvtComm name             \<comment> \<open>synchronization on the given name\<close>

text \<open>
  Substitution of a free-variable map into a process body, used after a
  successful pattern match.  Phase-1 leaves this an abstract operator;
  any concrete substitution that satisfies the atom-conservation lemma
  below is admissible.
\<close>

consts subst_binding :: "free_map \<Rightarrow> par \<Rightarrow> par"

text \<open>
  Atom-conservation properties required of \<open>subst_binding\<close>: substituting
  a free_map into a process can only introduce atoms (resp.\ \<open>new\<close>-bound
  atoms) that already appear in the body or in some bound value.  See
  \<^file>\<open>Patterns.thy\<close> for the auxiliaries \<open>fm_atoms\<close> and \<open>fm_bn_new\<close>.
\<close>

definition subst_atom_safe :: bool where
  "subst_atom_safe \<longleftrightarrow>
     (\<forall>fm body.
        atoms_of_par (subst_binding fm body)
        \<subseteq> atoms_of_par body \<union> fm_atoms fm)"

definition subst_bn_new_safe :: bool where
  "subst_bn_new_safe \<longleftrightarrow>
     (\<forall>fm body.
        bn_new_par (subst_binding fm body)
        \<subseteq> bn_new_par body \<union> fm_bn_new fm)"

text \<open>
  The set of atoms occurring anywhere in a configuration: in the active
  process, in any datum, or in any waiting continuation.  Used by the
  freshness side-condition of the New rule.
\<close>

definition atoms_in_config :: "config \<Rightarrow> atom set" where
  "atoms_in_config cfg =
     atoms_of_par (cfg_proc cfg)
     \<union> (\<Union>d \<in> set_mset (cfg_datums cfg).
          atoms_of_name (d_chan d) \<union> atoms_of_par (d_payload d))
     \<union> (\<Union>w \<in> set_mset (cfg_waiting cfg).
          atoms_of_par (w_pat w) \<union> atoms_of_name (w_chan w)
          \<union> atoms_of_par (w_body w) \<union> atoms_of_par (w_guard w))"

inductive
  step :: "config \<Rightarrow> event \<Rightarrow> config \<Rightarrow> bool"
    (\<open>_ \<rightarrow>\<langle>_\<rangle> _\<close> [55,0,55] 55)
where

  ProduceInstall:
    "cfg_proc cfg = PPar (Send c d persistent) cont \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := cont,
                cfg_datums := add_mset
                  \<lparr> d_chan = c, d_payload = d, d_persistent = persistent \<rparr>
                  (cfg_datums cfg) \<rparr>)"

| ConsumeInstall:
    "cfg_proc cfg = PPar (Recv pat c body persistent peek guard) cont \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := cont,
                cfg_waiting := add_mset
                  \<lparr> w_pat = pat, w_chan = c, w_body = body,
                    w_persistent = persistent, w_peek = peek,
                    w_guard = guard \<rparr>
                  (cfg_waiting cfg) \<rparr>)"

| Comm:
    "w \<in># cfg_waiting cfg \<Longrightarrow>
     d \<in># cfg_datums cfg \<Longrightarrow>
     strip_bundle (w_chan w) = strip_bundle (d_chan d) \<Longrightarrow>
     matches (w_pat w) (d_payload d) fm \<Longrightarrow>
     guard_holds (w_guard w) fm \<Longrightarrow>
     fired = strip_bundle (w_chan w) \<Longrightarrow>
     datums' = (if w_peek w then cfg_datums cfg
                else if d_persistent d then cfg_datums cfg
                else cfg_datums cfg - {#d#}) \<Longrightarrow>
     waiting' = (if w_persistent w then cfg_waiting cfg
                 else cfg_waiting cfg - {#w#}) \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtComm fired\<rangle>
         (cfg \<lparr> cfg_proc := PPar (subst_binding fm (w_body w)) (cfg_proc cfg),
                cfg_datums := datums',
                cfg_waiting := waiting' \<rparr>)"

| New:
    "cfg_proc cfg = PPar (NewN bound body) cont \<Longrightarrow>
     set bound \<inter> atoms_in_config cfg = {} \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtNew (set bound)\<rangle>
         (cfg \<lparr> cfg_proc := PPar body cont \<rparr>)"

| MatchHit:
    "cfg_proc cfg = PPar (MatchOne tgt pat gd body fall) cont \<Longrightarrow>
     matches pat tgt fm \<Longrightarrow>
     guard_holds gd fm \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := PPar (subst_binding fm body) cont \<rparr>)"

| MatchFallThrough:
    "cfg_proc cfg = PPar (MatchOne tgt pat gd body fall) cont \<Longrightarrow>
     (\<not>(\<exists>fm. matches pat tgt fm \<and> guard_holds gd fm)) \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := PPar fall cont \<rparr>)"

| IfTrue:
    "cfg_proc cfg = PPar (IfThenElse c t e) cont \<Longrightarrow>
     pure_eval_bool c Map.empty True \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar t cont \<rparr>)"

| IfFalse:
    "cfg_proc cfg = PPar (IfThenElse c t e) cont \<Longrightarrow>
     pure_eval_bool c Map.empty False \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar e cont \<rparr>)"

| EvalQuoteUnquote:
    "cfg_proc cfg = PPar (EvalQuote (Quote p)) cont \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar p cont \<rparr>)"

| StructComm:
    "cfg_proc cfg = PPar p q \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar q p \<rparr>)"

text \<open>Reflexive-transitive closure with a trace of events.\<close>

inductive
  steps :: "config \<Rightarrow> event list \<Rightarrow> config \<Rightarrow> bool"
    (\<open>_ \<rightarrow>*\<langle>_\<rangle> _\<close> [55,0,55] 55)
where
  steps_refl: "cfg \<rightarrow>*\<langle>[]\<rangle> cfg"
| steps_step: "cfg\<^sub>0 \<rightarrow>\<langle>e\<rangle> cfg\<^sub>1 \<Longrightarrow> cfg\<^sub>1 \<rightarrow>*\<langle>es\<rangle> cfg\<^sub>2 \<Longrightarrow>
                cfg\<^sub>0 \<rightarrow>*\<langle>e # es\<rangle> cfg\<^sub>2"

end
