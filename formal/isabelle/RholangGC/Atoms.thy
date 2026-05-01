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

  Phase-1 simplification: we pick \<^typ>\<open>nat\<close> as the atom carrier instead of an
  abstract type.  Naturals are countably infinite, fully decidable, and
  give us the freshness witnesses we need without dragging in Nominal2.
*)

theory Atoms
  imports Main
begin

type_synonym atom = nat

lemma infinite_atoms: "infinite (UNIV :: atom set)"
  by (simp add: infinite_UNIV_nat)

text \<open>
  The set of public unforgeables \<open>pub\<close>: system channel URIs and deploy-time
  ambients that every adversarial context can be assumed to know.  Treated
  as a model parameter --- a fixed but finite set of atoms.

  At the model level, the only thing that matters about a public atom is
  that it cannot be the witness for the GC0 non-triviality argument; we
  axiomatize finiteness and leave the contents abstract so different
  deploys can instantiate \<open>pub\<close> independently.
\<close>

axiomatization
  pub :: "atom set"
where
  pub_finite: "finite pub"

end
