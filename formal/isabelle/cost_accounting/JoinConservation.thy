(* Cost-Accounted Rho — N-ary join authority conservation (spec §4.8 Prop 4.7 /
   §4.8.5), Isabelle/HOL. Signatures are the free SAnd tensor over atoms;
   combined_key mirrors the Rocq CAJoinConservation.combined_key fold. Proves the
   FULL multiset conservation (join_authority_conserved: the fused key's atoms are
   exactly the receiver atoms plus every sender's atoms, as a multiset, invariant
   under grouping) and the no-weakening corollary — an independent HOL corroboration
   of the Rocq CAJoinConservation development. *)
theory JoinConservation
  imports "HOL-Library.Multiset"
begin

datatype sg = Leaf nat | And sg sg

fun sig_size :: "sg \<Rightarrow> nat" where
  "sig_size (Leaf _) = 1"
| "sig_size (And a b) = sig_size a + sig_size b"

lemma sig_size_pos: "sig_size s \<ge> 1"
  by (induction s) auto

fun combined_key :: "sg \<Rightarrow> sg list \<Rightarrow> sg" where
  "combined_key s1 [] = s1"
| "combined_key s1 (t # ts) = And (combined_key s1 ts) t"

lemma key_ge: "sig_size (combined_key s1 ts) \<ge> sig_size s1"
  by (induction ts) auto

theorem join_no_weakening: "sig_size s1 < sig_size (combined_key s1 (t # ts))"
  using key_ge[of s1 ts] sig_size_pos[of t] by simp

fun sig_atoms :: "sg \<Rightarrow> nat multiset" where
  "sig_atoms (Leaf a) = {# a #}"
| "sig_atoms (And a b) = sig_atoms a + sig_atoms b"

fun atoms_of :: "sg list \<Rightarrow> nat multiset" where
  "atoms_of [] = {#}"
| "atoms_of (t # ts) = sig_atoms t + atoms_of ts"

theorem join_authority_conserved:
  "sig_atoms (combined_key s1 ts) = sig_atoms s1 + atoms_of ts"
  by (induction ts) (simp_all add: add.assoc add.left_commute add.commute)

end
