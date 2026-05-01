(*
  Patterns.thy --- spatial-matching oracle.

  Rholang patterns are themselves Pars (RhoTypes.proto:163-167 and the
  Receive.binds field).  We do not redefine the pattern grammar here; we
  introduce only the abstract matching judgment that the spatial matcher
  realizes at runtime
    (rholang/src/rust/interpreter/matcher/spatial_matcher.rs).

  Phase-0 abstraction: the matcher is treated as a non-deterministic oracle
  that may yield any binding map for any (pattern, target) pair.  This is a
  sound over-approximation for GC --- see
    docs/discoveries/rholang-gc-design.md, section 3.3.

  A future Phase-1' refinement can specialize the oracle to the actual
  spatial-matching rules; the soundness statements quantify universally
  over the oracle and so hold for any specialization.
*)

theory Patterns
  imports Names
begin

text \<open>
  Free-variable bindings produced by a successful match are finite maps from
  De Bruijn levels (modeled as naturals) to either a process or a name,
  matching the runtime's \<open>FreeMap\<close>.
\<close>

datatype binding_value
  = BVPar par
  | BVName name

type_synonym free_map = "nat \<rightharpoonup> binding_value"

text \<open>
  Spatial matching judgment.  \<^prop>\<open>matches pat tgt fm\<close> states that the
  pattern \<open>pat\<close> spatially matches the target \<open>tgt\<close> with the binding map
  \<open>fm\<close>.  Phase-0 leaves this an inductive parameter.
\<close>

consts matches :: "par \<Rightarrow> par \<Rightarrow> free_map \<Rightarrow> bool"

text \<open>
  Lifted matching for a list of binds in a join: each pattern list must
  match some datum on its source channel.  We expose this lifted form
  because the COMM rule for joins consumes a tuple of datums atomically.
\<close>

definition matches_join ::
  "(par list \<times> name) list \<Rightarrow> (name \<times> par list) list \<Rightarrow> free_map \<Rightarrow> bool"
where
  "matches_join binds datums fm \<longleftrightarrow>
     length binds = length datums \<and>
     (\<forall>i < length binds.
        let (ps, c)   = binds ! i;
            (c', ds') = datums ! i
        in strip_bundle c = strip_bundle c' \<and>
           length ps = length ds' \<and>
           (\<forall>j < length ps. matches (ps ! j) (ds' ! j) fm))"

text \<open>
  Pure-evaluation oracle for \<open>where\<close> guards and \<open>If\<close> conditions.
  Realized at runtime by the \<open>rho-pure-eval\<close> crate
  (\<^file>\<open>../../../rho-pure-eval/src/lib.rs\<close>).  Phase-0 abstracts to a partial
  function from a process and an environment to a boolean; ill-typed or
  effectful expressions are simply absent from the relation.
\<close>

consts pure_eval_bool :: "par \<Rightarrow> free_map \<Rightarrow> bool \<Rightarrow> bool"
  \<comment> \<open>\<^prop>\<open>pure_eval_bool g fm b\<close>: \<open>g\<close> evaluates to \<open>b\<close> under \<open>fm\<close>.\<close>

end
