(*
  Patterns.thy --- spatial-matching oracle.

  Rholang patterns are themselves Pars (RhoTypes.proto:163-167 and the
  Receive.binds field).  We do not redefine the pattern grammar here; we
  introduce only the abstract matching judgment that the spatial matcher
  realizes at runtime
    (rholang/src/rust/interpreter/matcher/spatial_matcher.rs).

  Phase-1: `matches` and `pure_eval_bool` are abstract relations.  Any
  refinement of the spatial matcher or expression evaluator can specialize
  them; soundness statements quantify universally over compatible
  oracles.
*)

theory Patterns
  imports Names
begin

text \<open>
  A binding map produced by a successful match: a partial map from
  De Bruijn levels to a process or a name.
\<close>

datatype binding_value
  = BVPar par
  | BVName name

type_synonym free_map = "nat \<rightharpoonup> binding_value"

text \<open>
  Spatial matching judgment.  \<^prop>\<open>matches pat tgt fm\<close> states that the
  pattern \<open>pat\<close> spatially matches the target \<open>tgt\<close> with the binding map \<open>fm\<close>.
  Phase-1 leaves this an abstract relation.
\<close>

consts matches :: "par \<Rightarrow> par \<Rightarrow> free_map \<Rightarrow> bool"

text \<open>
  Pure-evaluation oracle for \<open>where\<close> guards and \<open>If\<close> conditions.
  Realized at runtime by the \<open>rho-pure-eval\<close> crate
  (\<^file>\<open>../../../rho-pure-eval/src/lib.rs\<close>).
\<close>

consts pure_eval_bool :: "par \<Rightarrow> free_map \<Rightarrow> bool \<Rightarrow> bool"

text \<open>
  Convention: a \<open>Nil\<close> guard means ``no guard''; we hard-code that the
  no-guard case always evaluates to \<open>True\<close>.  The Comm rule and Match rule
  consult \<open>guard_holds\<close> rather than \<open>pure_eval_bool\<close> directly.
\<close>

definition guard_holds :: "par \<Rightarrow> free_map \<Rightarrow> bool" where
  "guard_holds g fm \<longleftrightarrow> g = Nil \<or> pure_eval_bool g fm True"

text \<open>
  Properties the abstract \<open>matches\<close> oracle should satisfy: a successful
  match cannot synthesize atoms or bound atoms that are not present in
  either the pattern or the target.  These reflect what the actual
  spatial matcher does (it binds variables only to subterms that appear
  in the target), and are needed for the soundness arguments.
\<close>

primrec bv_atoms :: "binding_value \<Rightarrow> atom set" where
  "bv_atoms (BVPar p) = atoms_of_par p"
| "bv_atoms (BVName n) = atoms_of_name n"

primrec bv_bn_new :: "binding_value \<Rightarrow> atom set" where
  "bv_bn_new (BVPar p) = bn_new_par p"
| "bv_bn_new (BVName _) = {}"

definition fm_atoms :: "free_map \<Rightarrow> atom set" where
  "fm_atoms fm = (\<Union>v \<in> ran fm. bv_atoms v)"

definition fm_bn_new :: "free_map \<Rightarrow> atom set" where
  "fm_bn_new fm = (\<Union>v \<in> ran fm. bv_bn_new v)"

definition matches_atom_safe :: bool where
  "matches_atom_safe \<longleftrightarrow>
     (\<forall>pat tgt fm i v.
        matches pat tgt fm \<longrightarrow> fm i = Some v \<longrightarrow>
        bv_atoms v \<subseteq> atoms_of_par pat \<union> atoms_of_par tgt)"

text \<open>
  Bound-atom safety: matched values do not introduce \<open>new\<close>-binders beyond
  those in pattern or target.
\<close>

definition matches_bn_new_safe :: bool where
  "matches_bn_new_safe \<longleftrightarrow>
     (\<forall>pat tgt fm i v.
        matches pat tgt fm \<longrightarrow> fm i = Some v \<longrightarrow>
        bv_bn_new v \<subseteq> bn_new_par pat \<union> bn_new_par tgt)"

end
