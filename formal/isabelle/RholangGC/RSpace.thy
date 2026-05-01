(*
  RSpace.thy --- runtime configuration (sigma, P).

  Models the rspace++ tuple space (rspace++/src/rspace/internal.rs):
    - datums:                pending sends, possibly persistent
    - waiting_continuations: pending receives, possibly persistent / peek
                             with optional where-guard.

  Phase-1 simplification: single-payload sends and single-pattern receives,
  matching the simplified AST in Syntax.thy.  Multi-bind joins are encoded
  outside the core (see Reduction.thy notes).
*)

theory RSpace
  imports Patterns "HOL-Library.Multiset"
begin

text \<open>A datum: \<open>(channel, payload, persistent)\<close>.\<close>

record datum =
  d_chan       :: name
  d_payload    :: par
  d_persistent :: bool

text \<open>
  A waiting continuation: a single pattern on a single source channel, plus
  body, persistence and peek flags, and a where-guard process (\<open>Nil\<close>
  encodes ``no guard'').
\<close>

record wait_cont =
  w_pat        :: par
  w_chan       :: name
  w_body       :: par
  w_persistent :: bool
  w_peek       :: bool
  w_guard      :: par

text \<open>
  A configuration: a multiset of datums, a multiset of waiting
  continuations, and the active process pool.  Multisets reflect the
  runtime's distinction of duplicate sends/continuations.
\<close>

record config =
  cfg_datums  :: "datum multiset"
  cfg_waiting :: "wait_cont multiset"
  cfg_proc    :: par

definition init_config :: "par \<Rightarrow> config" where
  "init_config P = \<lparr> cfg_datums = {#}, cfg_waiting = {#}, cfg_proc = P \<rparr>"

end
