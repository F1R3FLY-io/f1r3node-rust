(*
  Reduction.thy --- labelled small-step reduction.

  Each step is labelled by an event of type @{typ event}.  The COMM(c)
  observable, recorded as the \<^const>\<open>EvtComm\<close> label, is the unit of
  observation for the garbage-collection question: a name \<open>c\<close> is garbage
  with respect to \<open>P\<close> iff no \<^const>\<open>EvtComm\<close> step on \<open>c\<close> is reachable from
  \<open>(init_config P)\<close> under any context.

  The rules track the runtime more closely than a textbook \<pi>-calculus:
  persistent and peek receives, joins of multi-bind binders, where-guards,
  bundle-aware sync, and reflection are all modeled explicitly.
*)

theory Reduction
  imports RSpace
begin

text \<open>Observable labels.\<close>

datatype event
  = EvtTau                   \<comment> \<open>silent step (structural, expression eval, ...)\<close>
  | EvtNew "atom set"        \<comment> \<open>fresh allocation by \<^const>\<open>NewN\<close>\<close>
  | EvtComm name             \<comment> \<open>synchronization on the given name\<close>

text \<open>
  Substitution of an atom for an atom inside a process.  Stated abstractly;
  Nominal2 generates the actual recursive equations from the binder
  declarations in \<^file>\<open>Syntax.thy\<close>.
\<close>

consts subst_atom_par :: "atom \<Rightarrow> atom \<Rightarrow> par \<Rightarrow> par"

text \<open>
  Substitution of a binding-value list into a process body, used by
  \<^const>\<open>Match\<close> and \<^const>\<open>Recv\<close> after a successful match.
\<close>

consts subst_binding :: "free_map \<Rightarrow> par \<Rightarrow> par"

inductive
  step :: "config \<Rightarrow> event \<Rightarrow> config \<Rightarrow> bool"
    (\<open>_ \<rightarrow>\<langle>_\<rangle> _\<close> [55,0,55] 55)
where

  StructPar:
    "\<lbrakk> cfg_proc cfg = PPar p q \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := PPar q p \<rparr>)"
    \<comment> \<open>commutativity of parallel composition; associativity and \<^const>\<open>Nil\<close> absorption are
        baked into structural-congruence-up-to (omitted from this skeleton).\<close>

| ProduceInstall:
    "\<lbrakk> cfg_proc cfg = PPar (Send c ds persistent) cont \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := cont,
                cfg_datums := add_mset
                  \<lparr> d_chan = c, d_payload = ds, d_persistent = persistent \<rparr>
                  (cfg_datums cfg) \<rparr>)"
    \<comment> \<open>installs a datum in the tuple space; the residual is the continuation, which in
        Rholang's async send is just \<^const>\<open>Nil\<close> (so \<open>cont\<close> is degenerate here, but
        retained to match \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:912--954
        where the produce returns a continuation token).\<close>

| ConsumeInstall:
    "\<lbrakk> cfg_proc cfg = PPar (Recv binds body persistent peek guard) cont \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := cont,
                cfg_waiting := add_mset
                  \<lparr> w_binds = binds, w_body = body,
                    w_persistent = persistent, w_peek = peek,
                    w_guard = guard \<rparr>
                  (cfg_waiting cfg) \<rparr>)"
    \<comment> \<open>installs a waiting continuation; analogue of consume in
        \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:955--1052.\<close>

| Comm:
    "\<lbrakk> w \<in># cfg_waiting cfg;
       \<comment> \<open>pick a tuple of datums, one per bind, from the store\<close>
       length picked = length (w_binds w);
       \<forall>i < length picked. picked ! i \<in># cfg_datums cfg;
       matches_join (w_binds w)
                    (map (\<lambda>d. (d_chan d, d_payload d)) picked)
                    fm;
       case w_guard w of
         None \<Rightarrow> True
       | Some g \<Rightarrow> pure_eval_bool g fm True;
       \<comment> \<open>the channel actually fired on, after stripping bundles\<close>
       fired = strip_bundle (snd (w_binds w ! 0));
       \<comment> \<open>compute the new store: peek leaves datums alone; persistent leaves the continuation\<close>
       datums' = (if w_peek w then cfg_datums cfg
                  else fold (\<lambda>d acc. acc - {#d#}) picked (cfg_datums cfg)
                       + mset (filter d_persistent picked));
       waiting' = (if w_persistent w then cfg_waiting cfg
                   else cfg_waiting cfg - {#w#}) \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtComm fired\<rangle>
         (cfg \<lparr> cfg_proc := PPar (subst_binding fm (w_body w)) (cfg_proc cfg),
                cfg_datums := datums',
                cfg_waiting := waiting' \<rparr>)"
    \<comment> \<open>the COMM rule.  Tracks the runtime in
        \<^file>\<open>../../../rspace++/src/rspace/match.rs\<close>:71--83 (commit decision under
        cross-channel where-guards), and the fall-through and persistence
        behavior in
        \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:955--1052.\<close>

| New:
    "\<lbrakk> cfg_proc cfg = PPar (NewN bound body) cont;
       finite bound;
       \<comment> \<open>freshness: the bound atoms are disjoint from everything currently in scope\<close>
       bound \<inter> atoms_in_config cfg = {} \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtNew bound\<rangle>
         (cfg \<lparr> cfg_proc := PPar body cont \<rparr>)"
    \<comment> \<open>Ground rule for fresh-name allocation; corresponds to \<open>eval_new\<close>
        (\<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1168--1310).\<close>

| MatchHit:
    "\<lbrakk> cfg_proc cfg = PPar (Match tgt ((pat, gd, body) # rest)) cont;
       matches pat tgt fm;
       case gd of
         None \<Rightarrow> True
       | Some g \<Rightarrow> pure_eval_bool g fm True \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := PPar (subst_binding fm body) cont \<rparr>)"

| MatchFallThrough:
    "\<lbrakk> cfg_proc cfg = PPar (Match tgt ((pat, gd, body) # rest)) cont;
       \<comment> \<open>either pattern fails, or guard returns false\<close>
       (\<not>(\<exists>fm. matches pat tgt fm))
       \<or> (\<exists>fm. matches pat tgt fm
              \<and> (\<exists>g. gd = Some g \<and> pure_eval_bool g fm False)) \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle>
         (cfg \<lparr> cfg_proc := PPar (Match tgt rest) cont \<rparr>)"

| IfTrue:
    "\<lbrakk> cfg_proc cfg = PPar (IfThenElse c t e) cont;
       pure_eval_bool c (Map.empty) True \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar t cont \<rparr>)"

| IfFalse:
    "\<lbrakk> cfg_proc cfg = PPar (IfThenElse c t e) cont;
       pure_eval_bool c (Map.empty) False \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar e cont \<rparr>)"

| EvalQuoteUnquote:
    "\<lbrakk> cfg_proc cfg = PPar (EvalQuote (Quote p)) cont \<rbrakk> \<Longrightarrow>
       cfg \<rightarrow>\<langle>EvtTau\<rangle> (cfg \<lparr> cfg_proc := PPar p cont \<rparr>)"
    \<comment> \<open>\<open>*@P \<longrightarrow> P\<close>; reflection eliminator.  Other shapes of \<^const>\<open>EvalQuote\<close> are stuck.\<close>

text \<open>
  Atoms appearing anywhere in a configuration: in the active process, in
  any datum's channel or payload, and in any waiting continuation's binds,
  body, or guard.  Used by the freshness side-condition of the New rule.
\<close>

definition atoms_in_config :: "config \<Rightarrow> atom set" where
  "atoms_in_config cfg =
     atoms_of_par (cfg_proc cfg)
     \<union> (\<Union>d \<in> set_mset (cfg_datums cfg).
          atoms_of_name (d_chan d) \<union> (\<Union>p \<in> set (d_payload d). atoms_of_par p))
     \<union> (\<Union>w \<in> set_mset (cfg_waiting cfg).
          (\<Union>(ps, c) \<in> set (w_binds w).
              atoms_of_name c \<union> (\<Union>p \<in> set ps. atoms_of_par p))
          \<union> atoms_of_par (w_body w)
          \<union> (case w_guard w of None \<Rightarrow> {} | Some g \<Rightarrow> atoms_of_par g))"

text \<open>Reflexive-transitive closure with a trace of events.\<close>

inductive
  steps :: "config \<Rightarrow> event list \<Rightarrow> config \<Rightarrow> bool"
    (\<open>_ \<rightarrow>*\<langle>_\<rangle> _\<close> [55,0,55] 55)
where
  steps_refl: "cfg \<rightarrow>*\<langle>[]\<rangle> cfg"
| steps_step: "\<lbrakk> cfg\<^sub>0 \<rightarrow>\<langle>e\<rangle> cfg\<^sub>1; cfg\<^sub>1 \<rightarrow>*\<langle>es\<rangle> cfg\<^sub>2 \<rbrakk> \<Longrightarrow>
                cfg\<^sub>0 \<rightarrow>*\<langle>e # es\<rangle> cfg\<^sub>2"

end
