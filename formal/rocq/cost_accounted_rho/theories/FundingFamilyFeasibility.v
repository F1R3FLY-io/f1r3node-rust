From Stdlib Require Import Arith.PeanoNat Arith.Compare_dec Lists.List Lia.
Import ListNotations.

Definition family_hold_sum count (hold : nat -> nat) :=
  fold_right Nat.add 0 (map hold (seq 0 count)).

Lemma family_sum_on_monotone : forall indices (left right : nat -> nat),
  (forall i, In i indices -> left i <= right i) ->
  fold_right Nat.add 0 (map left indices) <= fold_right Nat.add 0 (map right indices).
Proof.
  induction indices as [|head tail IH]; intros left right bounded; simpl; [lia|].
  assert (head_bound : left head <= right head) by (apply bounded; now left).
  assert (tail_bound : fold_right Nat.add 0 (map left tail) <= fold_right Nat.add 0 (map right tail)).
  { apply IH. intros i inside. apply bounded. now right. }
  lia.
Qed.

Lemma family_hold_sum_monotone : forall count left right,
  (forall i, i < count -> left i <= right i) ->
  family_hold_sum count left <= family_hold_sum count right.
Proof.
  intros count left right bounded. unfold family_hold_sum.
  apply family_sum_on_monotone. intros i inside. apply in_seq in inside. apply bounded. lia.
Qed.

Definition family_set_bound (bounds : nat -> nat) source value i :=
  if Nat.eq_dec i source then value else bounds i.

Definition family_source_hold (draws : list (nat -> nat)) source :=
  fold_right Nat.max 0 (map (fun draw => draw source) draws).

Lemma family_source_hold_bound : forall draws source bound,
  family_source_hold draws source <= bound <->
  forall draw, In draw draws -> draw source <= bound.
Proof.
  induction draws as [|head tail IH]; intros source bound; simpl.
  - unfold family_source_hold. simpl. split; [intros _ draw absent; contradiction|lia].
  - change (Nat.max (head source) (family_source_hold tail source) <= bound <->
      forall draw, head = draw \/ In draw tail -> draw source <= bound).
    rewrite Nat.max_lub_iff, IH. split.
    + intros [first rest] draw [same|inside]; [subst; exact first|now apply rest].
    + intro bounded. split; [apply bounded; now left|].
      intros draw inside. apply bounded. now right.
Qed.

Lemma family_choose_bounded_draws : forall count (hold : nat -> nat) domains,
  (forall domain, In domain domains -> exists draw, domain draw /\ forall i, i < count -> draw i <= hold i) ->
  exists draws, Forall2 (fun domain draw => domain draw) domains draws /\
    forall draw, In draw draws -> forall i, i < count -> draw i <= hold i.
Proof.
  intros count hold domains. induction domains as [|head tail IH]; intro available.
  - exists []. split; [constructor|intros draw absent; contradiction].
  - destruct (available head (or_introl eq_refl)) as [first [valid bounded]].
    destruct IH as [rest [valid_rest bounded_rest]].
    { intros domain inside. apply available. now right. }
    exists (first :: rest). split; [constructor; assumption|].
    intros draw [same|inside]; [subst; exact bounded|now apply bounded_rest].
Qed.

Theorem family_shared_hold_completion_exact : forall count (capacity : nat -> nat) exposure domains,
  (exists draws, Forall2 (fun domain draw => domain draw) domains draws /\
    (forall i, i < count -> family_source_hold draws i <= capacity i) /\
    family_hold_sum count (family_source_hold draws) <= exposure) <->
  (exists hold, (forall i, i < count -> hold i <= capacity i) /\
    family_hold_sum count hold <= exposure /\
    forall domain, In domain domains -> exists draw, domain draw /\ forall i, i < count -> draw i <= hold i).
Proof.
  intros count capacity exposure domains. split.
  - intros [draws [paired [bounded fits]]]. exists (family_source_hold draws).
    split; [exact bounded|]. split; [exact fits|].
    intros domain inside.
    assert (member : exists draw, In draw draws /\ domain draw).
    { clear bounded fits. induction paired as [|head draw tail rest valid paired IH]; [contradiction|].
      destruct inside as [same|inside].
      - subst. exists draw. split; [now left|exact valid].
      - destruct (IH inside) as [found [present valid_found]]. exists found. split; [now right|exact valid_found]. }
    destruct member as [draw [present valid]]. exists draw. split; [exact valid|].
    intros i inside_i. exact (proj1 (family_source_hold_bound draws i (family_source_hold draws i)) (Nat.le_refl _) draw present).
  - intros [hold [bounded [fits available]]].
    destruct (family_choose_bounded_draws count hold domains available) as [draws [paired draws_bounded]].
    assert (max_bounded : forall i, i < count -> family_source_hold draws i <= hold i).
    { intros i inside. apply family_source_hold_bound. intros draw present. now apply draws_bounded. }
    exists draws. split; [exact paired|]. split.
    + intros i inside. specialize (max_bounded i inside). specialize (bounded i inside). lia.
    + pose proof (family_hold_sum_monotone count _ _ max_bounded). lia.
Qed.

Section FamilySearch.
  Context {Plan : Type}.
  Variable count : nat.
  Variable valid : Plan -> Prop.
  Variable draw : Plan -> nat -> nat.
  Variable exposure : nat.

  Definition family_box_member lower upper plan :=
    valid plan /\ exists hold,
      (forall i, i < count -> lower i <= hold i /\ hold i <= upper i /\ draw plan i <= hold i) /\
      family_hold_sum count hold <= exposure.

  Theorem family_box_exposure_pruning : forall lower upper,
    exposure < family_hold_sum count lower ->
    forall plan, ~ family_box_member lower upper plan.
  Proof.
    intros lower upper over plan [_ [hold [bounded fits]]].
    assert (le : family_hold_sum count lower <= family_hold_sum count hold).
    { apply family_hold_sum_monotone. intros i inside. apply (bounded i inside). }
    lia.
  Qed.

  Theorem family_box_relaxed_pruning : forall lower upper,
    (forall plan, valid plan -> exists i, i < count /\ upper i < draw plan i) ->
    forall plan, ~ family_box_member lower upper plan.
  Proof.
    intros lower upper impossible plan [sound [hold [bounded fits]]].
    destruct (impossible plan sound) as [i [inside exceeds]].
    specialize (bounded i inside). lia.
  Qed.

  Theorem family_box_split_exact : forall lower upper source middle plan,
    source < count -> lower source <= middle -> middle < upper source ->
    (family_box_member lower upper plan <->
      family_box_member lower (family_set_bound upper source middle) plan \/
      family_box_member (family_set_bound lower source (S middle)) upper plan).
  Proof.
    intros lower upper source middle plan inside low high. split.
    - intros [sound [hold [bounded fits]]].
      destruct (le_dec (hold source) middle) as [left_half|right_half].
      + left. split; [exact sound|]. exists hold. split; [|exact fits].
        intros i valid_i. specialize (bounded i valid_i). unfold family_set_bound.
        destruct (Nat.eq_dec i source); subst; lia.
      + right. split; [exact sound|]. exists hold. split; [|exact fits].
        intros i valid_i. specialize (bounded i valid_i). unfold family_set_bound.
        destruct (Nat.eq_dec i source); subst; lia.
    - intros [[sound [hold [bounded fits]]]|[sound [hold [bounded fits]]]];
        split; try exact sound; exists hold; split; try exact fits;
        intros i valid_i; specialize (bounded i valid_i); unfold family_set_bound in bounded;
        destruct (Nat.eq_dec i source); subst; lia.
  Qed.

  Theorem family_box_singleton_complete : forall bounds plan,
    valid plan -> (forall i, i < count -> draw plan i <= bounds i) ->
    family_hold_sum count bounds <= exposure -> family_box_member bounds bounds plan.
  Proof.
    intros bounds plan sound bounded fits. split; [exact sound|].
    exists bounds. split; [intros i inside; specialize (bounded i inside); lia|exact fits].
  Qed.

  Theorem family_box_candidate_sound : forall lower upper plan,
    valid plan ->
    (forall i, i < count -> Nat.max (lower i) (draw plan i) <= upper i) ->
    family_hold_sum count (fun i => Nat.max (lower i) (draw plan i)) <= exposure ->
    family_box_member lower upper plan.
  Proof.
    intros lower upper plan sound bounded fits. split; [exact sound|].
    exists (fun i => Nat.max (lower i) (draw plan i)). split; [|exact fits].
    intros i inside. specialize (bounded i inside). lia.
  Qed.

  Theorem family_box_member_has_authorized_actual_exposure : forall lower upper plan,
    family_box_member lower upper plan -> family_hold_sum count (draw plan) <= exposure.
  Proof.
    intros lower upper plan [_ [hold [bounded fits]]].
    assert (le : family_hold_sum count (draw plan) <= family_hold_sum count hold).
    { apply family_hold_sum_monotone. intros i inside. apply (bounded i inside). }
    lia.
  Qed.

  Inductive family_empty_certificate : (nat -> nat) -> (nat -> nat) -> Prop :=
  | family_empty_exposure : forall lower upper,
      exposure < family_hold_sum count lower -> family_empty_certificate lower upper
  | family_empty_relaxed : forall lower upper,
      (forall plan, valid plan -> exists i, i < count /\ upper i < draw plan i) ->
      family_empty_certificate lower upper
  | family_empty_split : forall lower upper source middle,
      source < count -> lower source <= middle -> middle < upper source ->
      family_empty_certificate lower (family_set_bound upper source middle) ->
      family_empty_certificate (family_set_bound lower source (S middle)) upper ->
      family_empty_certificate lower upper.

  Theorem family_empty_certificate_sound : forall lower upper,
    family_empty_certificate lower upper -> forall plan, ~ family_box_member lower upper plan.
  Proof.
    intros lower upper certificate. induction certificate.
    - now apply family_box_exposure_pruning.
    - now apply family_box_relaxed_pruning.
    - intros plan member.
      apply (family_box_split_exact lower upper source middle plan H H0 H1) in member.
      destruct member as [left_half|right_half]; [eapply IHcertificate1|eapply IHcertificate2]; eauto.
  Qed.
End FamilySearch.

Theorem family_midpoint_split_progress : forall lower upper,
  lower < upper ->
  let middle := lower + (upper - lower) / 2 in
  lower <= middle /\ middle < upper /\
  middle - lower < upper - lower /\ upper - S middle < upper - lower.
Proof.
  intros lower upper positive. cbv zeta.
  assert (half : (upper - lower) / 2 < upper - lower) by (apply Nat.div_lt; lia).
  lia.
Qed.

Print Assumptions family_empty_certificate_sound.
Print Assumptions family_box_candidate_sound.
Print Assumptions family_box_member_has_authorized_actual_exposure.
Print Assumptions family_box_split_exact.
Print Assumptions family_midpoint_split_progress.
Print Assumptions family_shared_hold_completion_exact.
