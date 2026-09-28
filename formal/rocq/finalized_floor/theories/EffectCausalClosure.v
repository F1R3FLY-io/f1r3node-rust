From Stdlib Require Import Lists.List.
Import ListNotations.

Inductive resource_kind : Type :=
| OrdinaryDatum
| OrdinaryContinuation
| MergeableDatum.

Record resource : Type := {
  resource_kind_of : resource_kind;
  resource_identity : nat
}.

Inductive effect : Type :=
| BaseClose
| StaleClose
| CausalChild
| TransitiveChild
| MergeChild
| UserEffect
| MergeableProducer
| MergeableConsumer.

Definition base_cell :=
  {| resource_kind_of := OrdinaryDatum; resource_identity := 0 |}.

Definition stale_cell :=
  {| resource_kind_of := OrdinaryDatum; resource_identity := 1 |}.

Definition causal_cell :=
  {| resource_kind_of := OrdinaryContinuation; resource_identity := 2 |}.

Definition user_cell :=
  {| resource_kind_of := OrdinaryDatum; resource_identity := 3 |}.

Definition mergeable_cell :=
  {| resource_kind_of := MergeableDatum; resource_identity := 4 |}.

Definition adds (e : effect) : list resource :=
  match e with
  | BaseClose => [base_cell]
  | StaleClose => [stale_cell]
  | CausalChild => [causal_cell]
  | UserEffect => [user_cell]
  | MergeableProducer => [mergeable_cell]
  | _ => []
  end.

Definition removes (e : effect) : list resource :=
  match e with
  | CausalChild => [stale_cell]
  | TransitiveChild => [causal_cell]
  | MergeChild => [base_cell]
  | MergeableConsumer => [mergeable_cell]
  | _ => []
  end.

Definition ordinary (r : resource) : Prop :=
  resource_kind_of r <> MergeableDatum.

Definition physical_depends (target source : effect) : Prop :=
  exists r,
    In r (removes target) /\
    In r (adds source) /\
    ordinary r.

Inductive seed_rejected : effect -> Prop :=
| stale_close_rejected : seed_rejected StaleClose.

Inductive causal_rejected : effect -> Prop :=
| rejected_seed : forall e,
    seed_rejected e ->
    causal_rejected e
| rejected_dependent : forall target source,
    physical_depends target source ->
    causal_rejected source ->
    causal_rejected target.

Definition accepted (e : effect) : Prop := ~ causal_rejected e.

Definition rejection_closed (rejected : effect -> Prop) : Prop :=
  (forall e, seed_rejected e -> rejected e) /\
  (forall target source,
    physical_depends target source ->
    rejected source ->
    rejected target).

Lemma causal_rejected_is_closed : rejection_closed causal_rejected.
Proof.
  split.
  - intros e Hseed. exact (rejected_seed e Hseed).
  - intros target source Hdepends Hsource.
    exact (rejected_dependent target source Hdepends Hsource).
Qed.

Lemma causal_rejected_is_least :
  forall rejected,
    rejection_closed rejected ->
    forall e, causal_rejected e -> rejected e.
Proof.
  intros rejected [Hseed Hdependent] e Hrejected.
  induction Hrejected.
  - exact (Hseed e H).
  - exact (Hdependent target source H IHHrejected).
Qed.

Lemma accepted_has_no_rejected_dependency :
  forall target source,
    accepted target ->
    physical_depends target source ->
    causal_rejected source ->
    False.
Proof.
  intros target source Haccepted Hdepends Hsource.
  apply Haccepted.
  exact (rejected_dependent target source Hdepends Hsource).
Qed.

Lemma stale_close_is_rejected : causal_rejected StaleClose.
Proof.
  exact (rejected_seed StaleClose stale_close_rejected).
Qed.

Lemma causal_child_is_rejected : causal_rejected CausalChild.
Proof.
  apply (rejected_dependent CausalChild StaleClose).
  - exists stale_cell. repeat split; simpl; auto. discriminate.
  - exact stale_close_is_rejected.
Qed.

Lemma transitive_child_is_rejected : causal_rejected TransitiveChild.
Proof.
  apply (rejected_dependent TransitiveChild CausalChild).
  - exists causal_cell. repeat split; simpl; auto. discriminate.
  - exact causal_child_is_rejected.
Qed.

Lemma base_close_survives : accepted BaseClose.
Proof.
  unfold accepted.
  intros Hrejected.
  inversion Hrejected; subst.
  - inversion H.
  - destruct H as [r [Hremoved _]]. simpl in Hremoved. contradiction.
Qed.

Lemma merge_child_depends_only_on_base :
  forall source,
    physical_depends MergeChild source ->
    source = BaseClose.
Proof.
  intros source [r [Hremoved [Hadded Hordinary]]].
  simpl in Hremoved. destruct Hremoved as [Hr | Hfalse].
  - subst r. destruct source; simpl in Hadded; intuition discriminate.
  - contradiction.
Qed.

Lemma merge_child_survives : accepted MergeChild.
Proof.
  unfold accepted.
  intros Hrejected.
  inversion Hrejected; subst.
  - inversion H.
  - apply merge_child_depends_only_on_base in H. subst source.
    exact (base_close_survives H0).
Qed.

Lemma user_effect_survives : accepted UserEffect.
Proof.
  unfold accepted.
  intros Hrejected.
  inversion Hrejected; subst.
  - inversion H.
  - destruct H as [r [Hremoved _]]. simpl in Hremoved. contradiction.
Qed.

Lemma mergeable_materialization_is_not_dependency :
  ~ physical_depends MergeableConsumer MergeableProducer.
Proof.
  intros [r [Hremoved [Hadded Hordinary]]].
  simpl in Hremoved, Hadded.
  destruct Hremoved as [Hr | Hfalse].
  - subst r. apply Hordinary. reflexivity.
  - contradiction.
Qed.

Definition exact_effect_causal_closure_contract : Prop :=
  causal_rejected StaleClose /\
  causal_rejected CausalChild /\
  causal_rejected TransitiveChild /\
  accepted MergeChild /\
  accepted UserEffect /\
  ~ physical_depends MergeableConsumer MergeableProducer /\
  (forall target source,
    accepted target ->
    physical_depends target source ->
    causal_rejected source ->
    False) /\
  (forall rejected,
    rejection_closed rejected ->
    forall e, causal_rejected e -> rejected e).

Theorem exact_effect_causal_closure_correct :
  exact_effect_causal_closure_contract.
Proof.
  repeat split.
  - exact stale_close_is_rejected.
  - exact causal_child_is_rejected.
  - exact transitive_child_is_rejected.
  - exact merge_child_survives.
  - exact user_effect_survives.
  - exact mergeable_materialization_is_not_dependency.
  - exact accepted_has_no_rejected_dependency.
  - exact causal_rejected_is_least.
Qed.

Print Assumptions exact_effect_causal_closure_correct.
