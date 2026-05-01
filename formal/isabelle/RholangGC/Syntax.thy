(*
  Syntax.thy --- mutual datatype for Rholang names and processes.

  Models a faithful core of Rholang.  This skeleton uses single-bind
  receives, single-arm matches, and binary parallel composition.  Multi-
  bind joins, multi-case matches, lists of patterns, and value-level
  expression machinery are encoded outside the core --- either as
  syntactic sugar (joins desugar to a primitive `RecvJoin` operator
  treated as an atomic-action ghost in the reduction relation) or as
  abstract operators on processes.  The garbage-collection theorems hold
  for the core; extending the AST to the full surface language is a
  conservative extension that does not invalidate the soundness arguments
  --- see docs/discoveries/rholang-gc-design.md \<section>5.1.

  Constructors track the protobuf schema in
    models/src/main/protobuf/RhoTypes.proto

  Phase-1 conversion of the original Nominal2 skeleton to plain HOL
  datatypes.  Binders are handled with explicit freshness side-conditions
  in Reduction.thy.
*)

theory Syntax
  imports Atoms
begin

text \<open>Bundle capabilities --- the four-element lattice of read/write rights.\<close>

datatype cap
  = CapR     \<comment> \<open>read-only by holders, i.e.\ \<open>bundle+\<close>\<close>
  | CapW     \<comment> \<open>write-only by holders, i.e.\ \<open>bundle-\<close>\<close>
  | CapRW    \<comment> \<open>both, i.e.\ unrestricted \<open>bundle\<close>\<close>
  | CapNone  \<comment> \<open>neither, i.e.\ \<open>bundle0\<close>\<close>

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
  Mutually recursive name and process datatypes.  See
  \<^file>\<open>../../../models/src/main/protobuf/RhoTypes.proto\<close>.

  Notes on simplifications relative to the protobuf schema:

  \<^item> \<open>Send\<close> carries a single payload \<open>par\<close> rather than a list, and a
    persistence flag.
  \<^item> \<open>Recv\<close> binds a single pattern \<open>par\<close> on a single source channel \<open>name\<close>,
    plus persistent and peek flags and an optional \<open>where\<close>-guard.  Multi-
    bind joins are tracked at the reduction level via a \<open>RecvJoin\<close> auxiliary
    operator (see \<^file>\<open>Reduction.thy\<close>).
  \<^item> \<open>Match\<close> takes a single pattern, optional guard, and continuation, with
    fall-through encoded by chaining \<open>Match\<close>'s in series.
  \<^item> Value-level expressions, six numeric types, methods, collections, etc.\
    are not modeled here; their only effect on GC is through the names
    they syntactically carry, captured by \<open>EExpr\<close> as an opaque \<open>par set\<close>.
\<close>

datatype name
  = GPrivate atom
  | GDeployId string
  | GDeployerId string
  | GSysAuthToken
  | GUri string
  | Quote par
  | Bundle cap name
and par
  = Nil
  | PPar par par
  | Send name par bool                       \<comment> \<open>channel, payload, persistent flag\<close>
  | Recv par name par bool bool par          \<comment> \<open>pattern, channel, body, persistent, peek, guard (Nil = no guard)\<close>
  | NewN "atom list" par                     \<comment> \<open>list of bound atoms (always finite), body\<close>
  | MatchOne par par par par par             \<comment> \<open>target, pattern, guard (Nil = no guard), body, fall-through\<close>
  | IfThenElse par par par
  | EvalQuote name
  | EExpr "par list" "name list"             \<comment> \<open>opaque value-level term carrying these subterms\<close>

end
