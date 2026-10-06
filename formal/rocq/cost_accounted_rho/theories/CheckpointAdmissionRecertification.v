From Stdlib Require Import Arith.PeanoNat Arith.Wf_nat Lists.List Lia.
Import ListNotations.

Inductive admission_class := Admit | Reject | Defer.

Section CheckpointAdmission.

Variable candidate : Type.
Variable classify : list candidate -> candidate -> admission_class.
Variable canonicalize : list candidate -> list candidate.

Definition has_class
  (window : list candidate)
  (wanted : admission_class)
  (value : candidate)
  : bool :=
  match wanted, classify window value with
  | Admit, Admit | Reject, Reject | Defer, Defer => true
  | _, _ => false
  end.

Definition admitted_candidates (values : list candidate) : list candidate :=
  filter (has_class values Admit) values.

Definition rejected_candidates (values : list candidate) : list candidate :=
  filter (has_class values Reject) values.

Definition deferred_candidates (values : list candidate) : list candidate :=
  filter (has_class values Defer) values.

Definition canonical_window
  (user_limit : nat)
  (canonical_users dummy_candidates : list candidate)
  : list candidate :=
  canonicalize (firstn user_limit canonical_users ++ dummy_candidates).

Record certified_checkpoint_attempt := {
  attempt_generation : nat;
  attempt_user_limit : nat;
  attempt_window : list candidate;
  attempt_admitted : list candidate;
  attempt_rejected : list candidate;
  attempt_deferred : list candidate;
  attempt_root_chain : list nat;
  attempt_admitted_exact :
    attempt_admitted = admitted_candidates attempt_window;
  attempt_rejected_exact :
    attempt_rejected = rejected_candidates attempt_window;
  attempt_deferred_exact :
    attempt_deferred = deferred_candidates attempt_window;
  attempt_root_chain_exact :
    attempt_root_chain = seq 0 (S (length attempt_admitted))
}.

Definition certify_checkpoint_attempt
  (generation user_limit : nat)
  (canonical_users dummy_candidates : list candidate)
  : certified_checkpoint_attempt :=
  let window := canonical_window user_limit canonical_users dummy_candidates in
  let admitted := admitted_candidates window in
  {| attempt_generation := generation;
     attempt_user_limit := user_limit;
     attempt_window := window;
     attempt_admitted := admitted;
     attempt_rejected := rejected_candidates window;
     attempt_deferred := deferred_candidates window;
     attempt_root_chain := seq 0 (S (length admitted));
     attempt_admitted_exact := eq_refl;
     attempt_rejected_exact := eq_refl;
     attempt_deferred_exact := eq_refl;
     attempt_root_chain_exact := eq_refl |}.

Lemma admission_partition_complete :
  forall values value,
    In value values <->
      In value (admitted_candidates values) \/
      In value (rejected_candidates values) \/
      In value (deferred_candidates values).
Proof.
  intros values value.
  unfold admitted_candidates, rejected_candidates, deferred_candidates.
  repeat rewrite filter_In.
  unfold has_class.
  destruct (classify values value); simpl; tauto.
Qed.

Lemma admitted_rejected_disjoint :
  forall values value,
    In value (admitted_candidates values) ->
    ~ In value (rejected_candidates values).
Proof.
  intros values value Hadmitted Hrejected.
  unfold admitted_candidates, rejected_candidates in *.
  apply filter_In in Hadmitted.
  apply filter_In in Hrejected.
  destruct Hadmitted as [_ Hadmitted].
  destruct Hrejected as [_ Hrejected].
  unfold has_class in Hadmitted, Hrejected.
  destruct (classify values value); discriminate.
Qed.

Lemma admitted_deferred_disjoint :
  forall values value,
    In value (admitted_candidates values) ->
    ~ In value (deferred_candidates values).
Proof.
  intros values value Hadmitted Hdeferred.
  unfold admitted_candidates, deferred_candidates in *.
  apply filter_In in Hadmitted.
  apply filter_In in Hdeferred.
  destruct Hadmitted as [_ Hadmitted].
  destruct Hdeferred as [_ Hdeferred].
  unfold has_class in Hadmitted, Hdeferred.
  destruct (classify values value); discriminate.
Qed.

Lemma rejected_deferred_disjoint :
  forall values value,
    In value (rejected_candidates values) ->
    ~ In value (deferred_candidates values).
Proof.
  intros values value Hrejected Hdeferred.
  unfold rejected_candidates, deferred_candidates in *.
  apply filter_In in Hrejected.
  apply filter_In in Hdeferred.
  destruct Hrejected as [_ Hrejected].
  destruct Hdeferred as [_ Hdeferred].
  unfold has_class in Hrejected, Hdeferred.
  destruct (classify values value); discriminate.
Qed.

Inductive ordered_subsequence : list candidate -> list candidate -> Prop :=
| OrderedSubsequenceNil : forall values, ordered_subsequence [] values
| OrderedSubsequenceKeep :
    forall value selected values,
      ordered_subsequence selected values ->
      ordered_subsequence (value :: selected) (value :: values)
| OrderedSubsequenceDrop :
    forall value selected values,
      ordered_subsequence selected values ->
      ordered_subsequence selected (value :: values).

Lemma filter_is_ordered_subsequence :
  forall predicate values,
    ordered_subsequence (filter predicate values) values.
Proof.
  intros predicate values.
  induction values as [|value values IH]; simpl.
  - apply OrderedSubsequenceNil.
  - destruct (predicate value).
    + now apply OrderedSubsequenceKeep.
    + now apply OrderedSubsequenceDrop.
Qed.

Theorem certified_partition_is_complete :
  forall attempt value,
    In value (attempt_window attempt) <->
      In value (attempt_admitted attempt) \/
      In value (attempt_rejected attempt) \/
      In value (attempt_deferred attempt).
Proof.
  intros attempt value.
  rewrite (attempt_admitted_exact attempt).
  rewrite (attempt_rejected_exact attempt).
  rewrite (attempt_deferred_exact attempt).
  apply admission_partition_complete.
Qed.

Theorem certified_partition_is_pairwise_disjoint :
  forall attempt value,
    (In value (attempt_admitted attempt) ->
       ~ In value (attempt_rejected attempt)) /\
    (In value (attempt_admitted attempt) ->
       ~ In value (attempt_deferred attempt)) /\
    (In value (attempt_rejected attempt) ->
       ~ In value (attempt_deferred attempt)).
Proof.
  intros attempt value.
  rewrite (attempt_admitted_exact attempt).
  rewrite (attempt_rejected_exact attempt).
  rewrite (attempt_deferred_exact attempt).
  repeat split.
  - apply admitted_rejected_disjoint.
  - apply admitted_deferred_disjoint.
  - apply rejected_deferred_disjoint.
Qed.

Theorem certified_classes_preserve_canonical_order :
  forall attempt,
    ordered_subsequence (attempt_admitted attempt) (attempt_window attempt) /\
    ordered_subsequence (attempt_rejected attempt) (attempt_window attempt) /\
    ordered_subsequence (attempt_deferred attempt) (attempt_window attempt).
Proof.
  intro attempt.
  rewrite (attempt_admitted_exact attempt).
  rewrite (attempt_rejected_exact attempt).
  rewrite (attempt_deferred_exact attempt).
  repeat split; apply filter_is_ordered_subsequence.
Qed.

Theorem certified_root_chain_matches_admitted_order :
  forall attempt,
    length (attempt_root_chain attempt) = S (length (attempt_admitted attempt)).
Proof.
  intro attempt.
  rewrite (attempt_root_chain_exact attempt).
  apply length_seq.
Qed.

Theorem canonical_window_uses_raw_user_prefix :
  forall generation user_limit canonical_users dummy_candidates,
    attempt_window
      (certify_checkpoint_attempt
        generation user_limit canonical_users dummy_candidates) =
      canonicalize (firstn user_limit canonical_users ++ dummy_candidates).
Proof.
  reflexivity.
Qed.

Record published_checkpoint := {
  published_generation : nat;
  published_window : list candidate;
  published_admitted : list candidate;
  published_rejected : list candidate;
  published_deferred : list candidate;
  published_root_chain : list nat
}.

Definition publish_checkpoint
  (attempt : certified_checkpoint_attempt)
  : published_checkpoint :=
  {| published_generation := attempt_generation attempt;
     published_window := attempt_window attempt;
     published_admitted := attempt_admitted attempt;
     published_rejected := attempt_rejected attempt;
     published_deferred := attempt_deferred attempt;
     published_root_chain := attempt_root_chain attempt |}.

Theorem publication_uses_one_certified_generation :
  forall attempt,
    let publication := publish_checkpoint attempt in
    published_generation publication = attempt_generation attempt /\
    published_window publication = attempt_window attempt /\
    published_admitted publication = attempt_admitted attempt /\
    published_rejected publication = attempt_rejected attempt /\
    published_deferred publication = attempt_deferred attempt /\
    published_root_chain publication = attempt_root_chain attempt.
Proof.
  intro attempt.
  repeat split; reflexivity.
Qed.

Inductive checkpoint_result :=
| CheckpointFailed
| CheckpointSucceeded (publication : published_checkpoint).

Definition finish_checkpoint
  (success : bool)
  (attempt : certified_checkpoint_attempt)
  : checkpoint_result :=
  if success
  then CheckpointSucceeded (publish_checkpoint attempt)
  else CheckpointFailed.

Definition settled_candidates (result : checkpoint_result) : list candidate :=
  match result with
  | CheckpointFailed => []
  | CheckpointSucceeded publication => published_admitted publication
  end.

Definition terminal_candidates (result : checkpoint_result) : list candidate :=
  match result with
  | CheckpointFailed => []
  | CheckpointSucceeded publication =>
      published_admitted publication ++ published_rejected publication
  end.

Theorem failed_checkpoint_publishes_nothing :
  forall attempt,
    finish_checkpoint false attempt = CheckpointFailed.
Proof.
  reflexivity.
Qed.

Theorem failed_checkpoint_settles_nothing :
  forall attempt,
    settled_candidates (finish_checkpoint false attempt) = [].
Proof.
  reflexivity.
Qed.

Theorem failed_checkpoint_drains_nothing :
  forall attempt,
    terminal_candidates (finish_checkpoint false attempt) = [].
Proof.
  reflexivity.
Qed.

Theorem deferred_candidates_are_not_terminal :
  forall attempt value,
    In value (attempt_deferred attempt) ->
    ~ In value
        (terminal_candidates
          (finish_checkpoint true attempt)).
Proof.
  intros attempt value Hdeferred.
  simpl.
  intro Hterminal.
  apply in_app_iff in Hterminal.
  destruct Hterminal as [Hadmitted | Hrejected].
  - pose proof (certified_partition_is_pairwise_disjoint attempt value) as [_ [H _]].
    exact (H Hadmitted Hdeferred).
  - pose proof (certified_partition_is_pairwise_disjoint attempt value) as [_ [_ H]].
    exact (H Hrejected Hdeferred).
Qed.

Definition retry_step (current next : nat) : Prop :=
  1 < current /\ next = current / 2.

Theorem retry_step_strictly_decreases :
  forall current next,
    retry_step current next -> next < current.
Proof.
  intros current next [Hcurrent ->].
  apply Nat.div_lt; lia.
Qed.

Theorem retry_relation_is_well_founded :
  well_founded (fun next current => retry_step current next).
Proof.
  unfold well_founded.
  intro current.
  pattern current.
  apply lt_wf_ind.
  intros value IH.
  constructor.
  intros next Hstep.
  apply IH.
  now apply retry_step_strictly_decreases in Hstep.
Qed.

End CheckpointAdmission.
