(* ════════════════════════════════════════════════════════════════════════
   CAJoinConservation.v — Conservation of authority for N-ary joins
   (spec §4.8, Proposition 4.7 + the §4.8.4 reverse-currying iso and the §4.8.5
   no-weakening corollary).

   The spec's Proposition 4.7 states that, across ALL token-presentation
   groupings of an N-ary join, the multiset of signatures consumed is invariant —
   exactly the receiver authority together with each sender authority. This is a
   property of the compound funding KEY (s1 ⊓ t1 ⊓ … ⊓ tN), independent of the
   exact reduction rule that fires the join: it is the algebra of the free `SAnd`
   tensor read as a multiset of atoms. Because `SAnd` is a FREE binary constructor
   (not quotiented), every statement is up to `Permutation` (a multiset equality),
   never Leibniz `=` — exactly the standing constraint. Axiom-free.

   This lands §4.8's central conservation guarantee; the native join REDUCTION
   rule (ca_join1, CAReduction) and its confluence/determinism metatheory are a
   separate concern (Risk R3/R4) and are not needed here.                       *)

From Stdlib Require Import Lists.List.
Import ListNotations.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
From CostAccountedRho Require Import CostAccountedSyntax.

(* The atoms of a signature: the leaves, flattening the free `SAnd` tensor. *)
Fixpoint sig_atoms (s : sig) : list sig :=
  match s with
  | SAnd a b => sig_atoms a ++ sig_atoms b
  | _        => s :: nil
  end.

(* The compound funding key of an N-ary join: s1 ⊓ t1 ⊓ … ⊓ tN. *)
Fixpoint combined_key (s1 : sig) (ts : list sig) : sig :=
  match ts with
  | nil       => s1
  | cons t ts' => SAnd (combined_key s1 ts') t
  end.

(* Every signature has at least one atom. *)
Lemma sig_atoms_nonempty : forall s, 1 <= length (sig_atoms s).
Proof.
  induction s; simpl; try lia.
  rewrite app_length. lia.
Qed.

(* ── Proposition 4.7: conservation of authority ─────────────────────────────
   The atoms of the compound key are, AS A MULTISET, exactly the receiver
   authority s1 together with each sender authority t_i — grouping along any axis
   never changes the consumed multiset. *)
Theorem join_authority_conserved : forall s1 ts,
  Permutation (sig_atoms (combined_key s1 ts))
              (sig_atoms s1 ++ concat (map sig_atoms ts)).
Proof.
  intros s1 ts. induction ts as [| t ts' IH]; simpl.
  - rewrite app_nil_r. apply Permutation_refl.
  - eapply Permutation_trans; [ apply Permutation_app_tail; exact IH | ].
    rewrite <- app_assoc. apply Permutation_app_head. apply Permutation_app_comm.
Qed.

(* ── §4.8.4 reverse-currying: Join/Split regrouping preserves the multiset ─── *)
Theorem reverse_curry_iso : forall s1 ts ts',
  Permutation (sig_atoms (combined_key s1 (ts ++ ts')))
              (sig_atoms (combined_key s1 ts) ++ concat (map sig_atoms ts')).
Proof.
  intros s1 ts ts'.
  eapply Permutation_trans; [ apply join_authority_conserved | ].
  rewrite map_app, concat_app.
  eapply Permutation_trans;
    [ | apply Permutation_app_tail; apply Permutation_sym; apply join_authority_conserved ].
  rewrite app_assoc. apply Permutation_refl.
Qed.

(* concat∘map is a Permutation-congruence (no such combined lemma in Stdlib). *)
Lemma Permutation_concat_map {A B} (f : A -> list B) : forall l l',
  Permutation l l' -> Permutation (concat (map f l)) (concat (map f l')).
Proof.
  intros l l' H. induction H; simpl.
  - apply Permutation_refl.
  - apply Permutation_app_head. exact IHPermutation.
  - rewrite !app_assoc. apply Permutation_app_tail. apply Permutation_app_comm.
  - eapply Permutation_trans; eassumption.
Qed.

(* The partition-invariance reading: any two orderings/groupings of the same
   sender authorities yield the same consumed multiset. *)
Corollary join_demand_partition_invariant : forall s1 ts ts',
  Permutation ts ts' ->
  Permutation (sig_atoms (combined_key s1 ts)) (sig_atoms (combined_key s1 ts')).
Proof.
  intros s1 ts ts' Hperm.
  eapply Permutation_trans; [ apply join_authority_conserved | ].
  eapply Permutation_trans; [ | apply Permutation_sym; apply join_authority_conserved ].
  apply Permutation_app_head. apply Permutation_concat_map. exact Hperm.
Qed.

(* ── §4.8.5 no-weakening: a compound key cannot be discharged as fewer atoms ──
   A non-trivial join key has strictly more atoms than its receiver authority
   alone, so it cannot be weakened to s1 (the sender authorities cannot be
   silently dropped). *)
Theorem join_no_weakening : forall s1 t ts,
  length (sig_atoms s1) < length (sig_atoms (combined_key s1 (t :: ts))).
Proof.
  intros s1 t ts.
  pose proof (join_authority_conserved s1 (t :: ts)) as HP.
  apply Permutation_length in HP. rewrite HP. simpl.
  rewrite app_length, app_length.
  pose proof (sig_atoms_nonempty t). lia.
Qed.
