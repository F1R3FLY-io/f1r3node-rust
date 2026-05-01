(*
  Adequacy.thy --- correspondence between Isabelle rules and Rust source.

  This theory contains no theorems.  It is a navigable record of which
  Rust code each Isabelle definition is meant to model.  Phase 2
  (differential testing) and any future formal-adequacy work should pin
  obligations to entries in this table.

  Rust workspace root: this repository.
*)

theory Adequacy
  imports Garbage
begin

text \<open>

  \<^bold>\<open>Correspondence table\<close> (also reproduced in
  \<^file>\<open>../../../docs/discoveries/rholang-gc-design.md\<close> \<section>5):

  \<^item> Surface syntax of \<open>Par\<close>:
    \<^file>\<open>../../../models/src/main/protobuf/RhoTypes.proto\<close>:34--247
    corresponds to \<^typ>\<open>par\<close> in \<^file>\<open>Syntax.thy\<close>.

  \<^item> \<open>GUnforgeable\<close> oneof (the four runtime unforgeable shapes plus
    \<open>GUri\<close>, \<open>Quote\<close>, \<open>Bundle\<close> from the schema):
    \<^file>\<open>../../../models/src/main/protobuf/RhoTypes.proto\<close>:528--552
    corresponds to \<^typ>\<open>name\<close> in \<^file>\<open>Syntax.thy\<close>.

  \<^item> \<open>new\<close> allocates fresh \<open>GPrivate\<close>:
    \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1168--1310
    corresponds to \<open>New\<close> rule in \<^file>\<open>Reduction.thy\<close> (freshness side-condition delegated
      to Nominal2's binder machinery and \<^const>\<open>atoms_in_config\<close>).

  \<^item> SEND:
    \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:912--954
    corresponds to \<open>ProduceInstall\<close> in \<^file>\<open>Reduction.thy\<close>.

  \<^item> RECEIVE (linear, persistent, peek; multi-bind joins):
    \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:955--1052
    corresponds to \<open>ConsumeInstall\<close> + \<open>Comm\<close> in \<^file>\<open>Reduction.thy\<close>.

  \<^item> Spatial matching:
    \<^file>\<open>../../../rholang/src/rust/interpreter/matcher/spatial_matcher.rs\<close>
    corresponds to abstract \<^const>\<open>matches\<close> oracle in \<^file>\<open>Patterns.thy\<close>.

  \<^item> Where-guard commit (cross-channel, Phase 9):
    \<^file>\<open>../../../rspace++/src/rspace/match.rs\<close>:71--83 (\<open>check_commit\<close>)
    corresponds to the \<open>w_guard\<close>/\<^const>\<open>pure_eval_bool\<close> branch of \<open>Comm\<close> in
       \<^file>\<open>Reduction.thy\<close>.

  \<^item> Pure expression evaluator:
    \<^file>\<open>../../../rho-pure-eval/src/lib.rs\<close>
    corresponds to abstract \<^const>\<open>pure_eval_bool\<close> oracle in \<^file>\<open>Patterns.thy\<close>.

  \<^item> MATCH (fall-through on guard failure):
    \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1053--1135
    corresponds to \<open>MatchHit\<close> + \<open>MatchFallThrough\<close> in \<^file>\<open>Reduction.thy\<close>.

  \<^item> First-class \<open>If\<close> with synchronous type-error semantics
    (Phase 3.10):
    \<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1136--1167
    corresponds to \<open>IfTrue\<close>/\<open>IfFalse\<close> in \<^file>\<open>Reduction.thy\<close>; the type-error case is
       modeled by absence of a step (the configuration is stuck).

  \<^item> Tuple-space hot store:
    \<^file>\<open>../../../rspace++/src/rspace/internal.rs\<close> (\<open>Datum\<close>, \<open>WaitingContinuation\<close>)
    corresponds to records \<^typ>\<open>datum\<close>, \<^typ>\<open>wait_cont\<close>, \<^typ>\<open>config\<close> in \<^file>\<open>RSpace.thy\<close>.

  \<^item> COMM observable:
    \<^file>\<open>../../../models/src/main/protobuf/RSpacePlusPlusTypes.proto\<close>:258--263
    (\<open>CommProto\<close>)
    corresponds to \<^const>\<open>EvtComm\<close> labels in \<^file>\<open>Reduction.thy\<close>.

  \<^item> System channels (\<open>pub\<close>):
    \<^file>\<open>../../../rholang/src/rust/interpreter/system_processes.rs\<close>:86--144
    (\<open>FixedChannels\<close>)
    corresponds to abstract \<^const>\<open>pub\<close> set in \<^file>\<open>Atoms.thy\<close>.

  \<^bold>\<open>Conservative abstractions made by the model\<close> (cf.\
  \<^file>\<open>../../../docs/discoveries/rholang-gc-design.md\<close> \<section>5.1):

  \<^enum> \<^const>\<open>matches\<close> is left an oracle.
  \<^enum> \<^const>\<open>pure_eval_bool\<close> is left an oracle.
  \<^enum> Numerics, methods, string ops, collections, PathMaps, Zippers are
    folded into the opaque \<open>EExpr ps ns\<close> constructor, which records the
    name and process subterms but otherwise has no operational meaning.
    These constructs cannot allocate \<^const>\<open>GPrivate\<close> atoms in the
    runtime, so opacity is sound for GC.
  \<^enum> Replay determinism (\<open>Blake2b512Random\<close>) is not modeled --- only the
    weaker fact that fresh atoms are distinct from currently-visible
    atoms is needed.
\<close>

end
