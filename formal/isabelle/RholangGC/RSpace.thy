(*
  RSpace.thy --- runtime configuration (sigma, P).

  Models the rspace++ tuple space (rspace++/src/rspace/internal.rs):
    - datums:                pending sends, possibly persistent
    - waiting_continuations: pending receives, possibly persistent / peek
                             with optional where-guard.
*)

theory RSpace
  imports Patterns
begin

text \<open>
  A datum is a tuple \<open>(channel, payload, persistent)\<close>: the data sent on the
  channel, with the persistence flag from \<open>chan!!(...)\<close> vs \<open>chan!(...)\<close>.
\<close>

record datum =
  d_chan        :: name
  d_payload     :: "par list"
  d_persistent  :: bool

text \<open>
  A waiting continuation is a list of binds (one per joined channel),
  the body, persistence and peek flags, and an optional where-guard.
\<close>

record wait_cont =
  w_binds       :: "(par list \<times> name) list"
  w_body        :: par
  w_persistent  :: bool
  w_peek        :: bool
  w_guard       :: "par option"

text \<open>
  A configuration is a multiset of datums and a multiset of waiting
  continuations together with the active process pool.  We use multisets
  rather than sets because the runtime distinguishes duplicate sends and
  duplicate continuations (relevant for \<open>persistent\<close> bookkeeping).
\<close>

record config =
  cfg_datums   :: "datum multiset"
  cfg_waiting  :: "wait_cont multiset"
  cfg_proc     :: par

text \<open>
  Initial configuration of a closed process: empty store, the process as
  the active pool.
\<close>

definition init_config :: "par \<Rightarrow> config" where
  "init_config P = \<lparr> cfg_datums = {#}, cfg_waiting = {#}, cfg_proc = P \<rparr>"

end
