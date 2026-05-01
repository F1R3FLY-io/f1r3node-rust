(*
  Atoms.thy --- Unforgeable atoms.

  Models the countably-infinite seed set from which `new` allocates fresh
  GPrivate atoms.  Also records the publicly-known unforgeables `pub` (the
  system URIs and deploy-time ambients that every adversary knows by
  construction).

  Rust correspondences:
    - `new` allocation:  rholang/src/rust/interpreter/reduce.rs:1168-1310
      (eval_new, draws bytes from Blake2b512Random).
    - System channels:   rholang/src/rust/interpreter/system_processes.rs:86-144
      (FixedChannels: rho:io:stdout, rho:crypto:*, rho:registry:*, ...).
*)

theory Atoms
  imports "HOL-Nominal2.Nominal2"
begin

text \<open>
  The atom seed type.  In the Rust runtime each \<^const>\<open>GPrivate\<close> atom is a
  32-byte Blake2b512Random output; here we abstract over an arbitrary
  countably-infinite atom sort and rely on Nominal2's freshness machinery.
\<close>

atom_decl atom

text \<open>
  Public unforgeables: system channels and deploy-time ambients that every
  context can be assumed to know.  Treated as a model parameter --- a fixed
  but arbitrary finite set.
\<close>

consts pub :: "atom set"

axiomatization where
  pub_finite: "finite pub"

text \<open>
  We do not commit to whether \<^const>\<open>pub\<close> is empty; downstream theories
  only use that it is finite.
\<close>

end
