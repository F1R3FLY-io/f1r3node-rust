(*
  Syntax.thy --- mutual datatype for Rholang names and processes.

  Rholang patterns are themselves Pars (see RhoTypes.proto:34-247 and the
  survey in docs/discoveries/rholang-gc-design.md), so we do not need a
  separate `pattern` datatype --- Patterns.thy is a thin wrapper that
  introduces the abstract spatial-matching oracle.

  Constructors track the protobuf schema in
    models/src/main/protobuf/RhoTypes.proto

  This is a Phase-0 skeleton: the syntax is faithful, but a number of
  expression-level subtleties (six numeric types, methods on values,
  PathMaps, Zippers, etc.) are folded into an opaque \<^typ>\<open>expr\<close> sort because
  they cannot allocate \<^const>\<open>GPrivate\<close> atoms and therefore do not influence
  garbage collection.  See \<^file>\<open>../../../docs/discoveries/rholang-gc-design.md\<close>
  \<section>5.1 for the conservative-abstraction argument.
*)

theory Syntax
  imports Atoms
begin

text \<open>
  Bundle capabilities, encoding the four meets of a sub-lattice of read/write
  permissions.  Corresponds to the Rholang \<open>bundle+\<close> / \<open>bundle-\<close> / \<open>bundle0\<close> /
  \<open>bundle\<close> surface forms (see \<^file>\<open>../../../docs/rholang/02-syntax-reference.md\<close>).
\<close>

datatype cap
  = CapR     \<comment> \<open>read-only by holders, i.e.\ \<open>bundle+\<close>\<close>
  | CapW     \<comment> \<open>write-only by holders, i.e.\ \<open>bundle-\<close>\<close>
  | CapRW    \<comment> \<open>both, i.e.\ unrestricted \<open>bundle\<close>\<close>
  | CapNone  \<comment> \<open>neither, i.e.\ \<open>bundle0\<close>\<close>

text \<open>
  Bundle composition: nested bundles intersect their capabilities.  This is
  the standard meet over the four-element lattice.  GC1 uses this in its
  bundle-aware refinement.
\<close>

fun cap_meet :: "cap \<Rightarrow> cap \<Rightarrow> cap" where
  "cap_meet CapNone _    = CapNone"
| "cap_meet _ CapNone    = CapNone"
| "cap_meet CapRW c      = c"
| "cap_meet c CapRW      = c"
| "cap_meet CapR CapR    = CapR"
| "cap_meet CapW CapW    = CapW"
| "cap_meet CapR CapW    = CapNone"
| "cap_meet CapW CapR    = CapNone"

text \<open>
  Opaque expression sort.  Stands for everything in \<^const>\<open>Expr\<close> --- ground
  values (six numeric types, strings, byte arrays, URIs, booleans),
  collections (lists, tuples, sets, maps, PathMaps, Zippers), arithmetic,
  comparison, boolean connectives, methods, string interpolation, and
  \<^const>\<open>EMatchExpr\<close>.  None of these allocate \<^const>\<open>GPrivate\<close> atoms; \<^const>\<open>eval_new\<close>
  in the Rust runtime is the only allocation site
  (\<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1168--1310).

  We track the free names that may appear inside an expression (e.g. via
  process-level subterms in collection elements or through \<open>EVar\<close> closures)
  through the abstract operator \<^const>\<open>expr_subterms\<close>; expressions are
  otherwise opaque to the model.
\<close>

typedecl expr

text \<open>
  Mutually recursive name and process datatypes.  See
  \<^file>\<open>../../../models/src/main/protobuf/RhoTypes.proto\<close> for the source.
\<close>

nominal_datatype
  name
  = GPrivate atom
  | GDeployId "8 word list"          \<comment> \<open>signature bytes; opaque\<close>
  | GDeployerId "8 word list"        \<comment> \<open>public key bytes; opaque\<close>
  | GSysAuthToken
  | GUri string                      \<comment> \<open>e.g.\ \<open>"rho:io:stdout"\<close>\<close>
  | Quote par                        \<comment> \<open>\<open>@P\<close>\<close>
  | Bundle cap name                  \<comment> \<open>\<open>bundle\<plusmn>{n}\<close>\<close>
and par
  = Nil
  | PPar par par                                                    (\<open>_ \<parallel> _\<close> [60,61] 60)
  | Send name "par list" bool                                       \<comment> \<open>channel, data, persistent\<close>
  | Recv "(par list \<times> name) list" par bool bool "par option"        \<comment> \<open>binds, body, persistent, peek, guard\<close>
  | NewN "atom set" par                                             \<comment> \<open>bound atoms, body\<close>
  | Match par "(par \<times> par option \<times> par) list"                       \<comment> \<open>target, cases (pattern, guard, body)\<close>
  | IfThenElse par par par                                          \<comment> \<open>condition, then, else\<close>
  | EvalQuote name                                                  \<comment> \<open>\<open>*x\<close>\<close>
  | EExpr expr                                                      \<comment> \<open>opaque value-level expression\<close>

text \<open>
  Conventions:

  \<^item> \<^const>\<open>Send\<close>: the third argument records persistence (\<open>chan!!(...)\<close> vs
    \<open>chan!(...)\<close>).
  \<^item> \<^const>\<open>Recv\<close>: each bind is a list of patterns paired with its source
    channel; the outer list is the join (\<open>&\<close>-separated binds inside one
    \<open>for\<close>); the body is the continuation; the booleans encode persistent
    (\<open><=\<close>) and peek (\<open><<-\<close>); the optional \<open>par\<close> is the \<open>where\<close> guard added in
    Phase 9 (\<^file>\<open>../../../docs/plans/where-clauses-and-match-guards-2026-04-29.md\<close>).
  \<^item> \<^const>\<open>NewN\<close>: binds the listed atoms in the body.  Nominal2's binder
    machinery handles \<open>\<alpha>\<close>-equivalence; freshness obligations are discharged
    by the standard \<open>fresh\<close>/\<open>fresh_star\<close> apparatus.
  \<^item> \<^const>\<open>Match\<close>: the case list mirrors the runtime's fall-through
    semantics --- the head case is tried first, and a guard returning
    \<open>false\<close> falls through to the next case
    (\<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1053--1135).
  \<^item> \<^const>\<open>IfThenElse\<close>: first-class \<^const>\<open>If\<close> with synchronous type-check
    semantics from Phase 3.10
    (\<^file>\<open>../../../rholang/src/rust/interpreter/reduce.rs\<close>:1136--1167).
  \<^item> \<^const>\<open>Quote\<close> / \<^const>\<open>EvalQuote\<close>: the \<open>@\<close>/\<open>*\<close> reflection pair.
\<close>

text \<open>
  Free names embedded in opaque expressions.  Stated as a parameter; any
  refinement that wants to see inside expressions can specialize it.
\<close>

consts expr_subterm_pars :: "expr \<Rightarrow> par set"
consts expr_subterm_names :: "expr \<Rightarrow> name set"

axiomatization where
  expr_subterm_pars_finite: "finite (expr_subterm_pars e)" and
  expr_subterm_names_finite: "finite (expr_subterm_names e)"

end
