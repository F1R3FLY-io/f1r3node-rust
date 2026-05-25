(* ═══════════════════════════════════════════════════════════════════════════
   LLIdentities.v — Phase 2 + Phase 3 linear-logic algebraic identities

   Mechanizes the canonical algebraic identities of intuitionistic linear
   logic with exponentials (ILLE) — the fragment of LL that the Phase 3
   substrate implements via the `Sig` enum extensions (Tensor / And, Plus,
   With, Bang, WhyNot, Lolly) plus the Phase 2 Threshold primitive.

   Models each connective abstractly as a finite multiset of atomic
   propositions (the "carrier" of the SignatureChannel reflection — the
   substrate's `ParSortMatcher::sort_match` post-step makes the channel
   shape invariant under permutation, so multiset semantics faithfully
   capture the reflection-layer behavior).

   Three families of identities established Qed-closed:

     1. Multiplicative laws (Tensor / And `⊗`): commutativity,
        associativity. Already covered for the Sig::And case by
        MultiSignerRefinement; this file extends to Plus / With.

     2. Additive laws (Plus `⊕`, With `&`): commutativity, associativity
        at the reflection layer (signer/verifier choice symmetry).

     3. Exponential laws (Bang `!`, WhyNot `?`): idempotence at the
        reflection layer (Bang/WhyNot reflect to the inner channel
        per `SignatureChannel::from_sig` for the unary case).

   No Axiom, no Admitted. All theorems Qed-closed.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia
  Sorting.Permutation.
Import ListNotations.

(* ─────────────────────────────────────────────────────────────────────────
   §1: Abstract channel model — multiset of atomic propositions.
   The reflection layer's ParSortMatcher::sort_match makes the resulting
   Par invariant under permutation of constituent channels, so a multiset
   (modeled as a list quotiented by Permutation) faithfully captures the
   reflection-shape equivalence relation.
   ─────────────────────────────────────────────────────────────────────── *)

Definition channel := list nat.
(** Each atomic signature contributes a nat-id to the channel multiset. *)

(** Channel equivalence: two channels are equivalent if their multisets agree.
    Modeled via `Permutation` — the closest standard-library multiset notion. *)
Definition channel_equiv (c1 c2 : channel) : Prop := Permutation c1 c2.

Lemma channel_equiv_refl : forall c, channel_equiv c c.
Proof. intros c. apply Permutation_refl. Qed.

Lemma channel_equiv_sym : forall c1 c2,
  channel_equiv c1 c2 -> channel_equiv c2 c1.
Proof. intros c1 c2 H. apply Permutation_sym. exact H. Qed.

Lemma channel_equiv_trans : forall c1 c2 c3,
  channel_equiv c1 c2 -> channel_equiv c2 c3 -> channel_equiv c1 c3.
Proof. intros c1 c2 c3 H1 H2. eapply Permutation_trans; eauto. Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §2: Connective semantics at the channel layer.

   - tensor (And) / plus / with / lolly: concatenate constituent channels
     (the substrate's `concatenate_pars` after ParSortMatcher::sort_match).
   - bang / whynot: reflect to the inner channel (the Phase 3 substrate
     decision — replication/optionality semantics enforced by the
     capability-registry contract layer, not the reflection layer).
   - threshold: concatenate ALL member channels (the substrate's Phase 2
     reflection — k-of-N semantic enforced at verifier layer).
   ─────────────────────────────────────────────────────────────────────── *)

Definition tensor_channel (c1 c2 : channel) : channel := c1 ++ c2.
Definition plus_channel   (c1 c2 : channel) : channel := c1 ++ c2.
Definition with_channel   (c1 c2 : channel) : channel := c1 ++ c2.
Definition lolly_channel  (c1 c2 : channel) : channel := c1 ++ c2.
Definition bang_channel   (c : channel)     : channel := c.
Definition whynot_channel (c : channel)     : channel := c.

Fixpoint threshold_channel (members : list channel) : channel :=
  match members with
  | [] => []
  | h :: t => h ++ threshold_channel t
  end.

(* ─────────────────────────────────────────────────────────────────────────
   §3: Multiplicative laws (Tensor / And)
   ─────────────────────────────────────────────────────────────────────── *)

Theorem tensor_commutative : forall c1 c2,
  channel_equiv (tensor_channel c1 c2) (tensor_channel c2 c1).
Proof.
  intros c1 c2. unfold tensor_channel, channel_equiv.
  apply Permutation_app_comm.
Qed.

Theorem tensor_associative : forall c1 c2 c3,
  channel_equiv (tensor_channel (tensor_channel c1 c2) c3)
                (tensor_channel c1 (tensor_channel c2 c3)).
Proof.
  intros c1 c2 c3. unfold tensor_channel, channel_equiv.
  rewrite <- app_assoc. apply Permutation_refl.
Qed.

Theorem tensor_unit_left : forall c,
  channel_equiv (tensor_channel [] c) c.
Proof.
  intros c. unfold tensor_channel, channel_equiv. cbn.
  apply Permutation_refl.
Qed.

Theorem tensor_unit_right : forall c,
  channel_equiv (tensor_channel c []) c.
Proof.
  intros c. unfold tensor_channel, channel_equiv.
  rewrite app_nil_r. apply Permutation_refl.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §4: Additive laws (Plus, With)
   Same structural shape as Tensor at the channel layer; semantic
   distinction (signer-choice vs verifier-choice) is enforced at the
   verifier dispatch layer (Phase 2 / Phase 3 envelope construction).
   ─────────────────────────────────────────────────────────────────────── *)

Theorem plus_commutative : forall c1 c2,
  channel_equiv (plus_channel c1 c2) (plus_channel c2 c1).
Proof.
  intros c1 c2. unfold plus_channel, channel_equiv.
  apply Permutation_app_comm.
Qed.

Theorem plus_associative : forall c1 c2 c3,
  channel_equiv (plus_channel (plus_channel c1 c2) c3)
                (plus_channel c1 (plus_channel c2 c3)).
Proof.
  intros c1 c2 c3. unfold plus_channel, channel_equiv.
  rewrite <- app_assoc. apply Permutation_refl.
Qed.

Theorem with_commutative : forall c1 c2,
  channel_equiv (with_channel c1 c2) (with_channel c2 c1).
Proof.
  intros c1 c2. unfold with_channel, channel_equiv.
  apply Permutation_app_comm.
Qed.

Theorem with_associative : forall c1 c2 c3,
  channel_equiv (with_channel (with_channel c1 c2) c3)
                (with_channel c1 (with_channel c2 c3)).
Proof.
  intros c1 c2 c3. unfold with_channel, channel_equiv.
  rewrite <- app_assoc. apply Permutation_refl.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §5: Exponential laws (Bang, WhyNot)
   Reflection layer collapses to inner channel; idempotence is trivial.
   ─────────────────────────────────────────────────────────────────────── *)

Theorem bang_idempotent : forall c,
  channel_equiv (bang_channel (bang_channel c)) (bang_channel c).
Proof. intros c. apply channel_equiv_refl. Qed.

Theorem whynot_idempotent : forall c,
  channel_equiv (whynot_channel (whynot_channel c)) (whynot_channel c).
Proof. intros c. apply channel_equiv_refl. Qed.

Theorem bang_unit : forall c,
  channel_equiv (bang_channel c) c.
Proof. intros c. apply channel_equiv_refl. Qed.

Theorem whynot_unit : forall c,
  channel_equiv (whynot_channel c) c.
Proof. intros c. apply channel_equiv_refl. Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §6: Lolly (linear implication)
   Lolly reflects to (from ++ to) at the channel level. Algebraically
   compatible with the tensor laws since structurally identical.
   Semantic distinction is in the capability-registry transformer (Phase 3
   §3.5), not the channel-shape layer.
   ─────────────────────────────────────────────────────────────────────── *)

Theorem lolly_to_tensor_channel : forall c_from c_to,
  channel_equiv (lolly_channel c_from c_to) (tensor_channel c_from c_to).
Proof.
  intros c_from c_to. unfold lolly_channel, tensor_channel, channel_equiv.
  apply Permutation_refl.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §7: Threshold permutation-invariance (Phase 2)
   ─────────────────────────────────────────────────────────────────────── *)

Lemma threshold_channel_app : forall ms1 ms2,
  threshold_channel (ms1 ++ ms2) =
  threshold_channel ms1 ++ threshold_channel ms2.
Proof.
  induction ms1 as [|h t IH]; intros ms2; cbn.
  - reflexivity.
  - rewrite IH. rewrite app_assoc. reflexivity.
Qed.

(** Reordering the member list of a Threshold preserves the channel
    multiset. This is the formal counterpart to the substrate test
    `sig_threshold_reflection_permutation_invariant_in_members`. *)
Theorem threshold_permutation_invariant :
  forall ms1 ms2,
    Permutation ms1 ms2 ->
    channel_equiv (threshold_channel ms1) (threshold_channel ms2).
Proof.
  intros ms1 ms2 Hperm. unfold channel_equiv.
  induction Hperm.
  - cbn. apply Permutation_refl.
  - cbn. apply Permutation_app_head. exact IHHperm.
  - cbn. rewrite !app_assoc. apply Permutation_app_tail.
    apply Permutation_app_comm.
  - eapply Permutation_trans; eauto.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §8: Distributivity laws
   tensor distributes over plus (one of the canonical LL laws).
   Both reduce to the same concatenation at the channel level.
   ─────────────────────────────────────────────────────────────────────── *)

(** Note on tensor-distributes-over-plus:

    The canonical LL law `σ ⊗ (τ ⊕ ρ) ≡ (σ ⊗ τ) ⊕ (σ ⊗ ρ)` does NOT hold
    under our channel-as-multiset semantics because the right-hand side
    duplicates `σ` (counter-example: σ=[1], τ=[2], ρ=[3] gives LHS=[1,2,3]
    but RHS=[1,2,1,3]). The genuine LL distributivity is enforced at the
    *verifier-dispatch* layer — when a deploy presents `σ ⊗ (τ ⊕ ρ)` the
    verifier consumes σ once and exactly one of {τ, ρ} based on the
    chosen branch witness. The substrate channel reflection deliberately
    flattens to concatenation; correct branch-selection happens at the
    Phase 3 §3.7 verifier layer (`Cosigned::from_signed_data` extension).

    The provable weaker law at the channel layer: every atom appearing
    in LHS also appears in RHS (a containment, not an equivalence). *)
Theorem tensor_over_plus_subset_lhs_in_rhs :
  forall c1 c2 c3 a,
    In a (tensor_channel c1 (plus_channel c2 c3)) ->
    In a (plus_channel (tensor_channel c1 c2) (tensor_channel c1 c3)).
Proof.
  intros c1 c2 c3 a Hin.
  unfold tensor_channel, plus_channel in *.
  apply in_app_or in Hin. destruct Hin as [H_c1 | H_c2c3].
  - apply in_or_app. left. apply in_or_app. left. exact H_c1.
  - apply in_app_or in H_c2c3. destruct H_c2c3 as [H_c2 | H_c3].
    + apply in_or_app. left. apply in_or_app. right. exact H_c2.
    + apply in_or_app. right. apply in_or_app. right. exact H_c3.
Qed.
