From Stdlib Require Import Bool List.
From StackSafePDA Require Import Bag.

Import ListNotations.
Set Implicit Arguments.

(** Refinement model for the store-wide accepted-match transaction introduced
    by [cd6099f50].  [Epoch] is intentionally abstract: Rust represents it by
    [Arc] allocation identity, so the proof uses equality only and assumes no
    counter, ordering, width, or wraparound law. *)
Section RSpaceCommit.

Context {Key Epoch Metadata OtherState : Type}.

Record continuation_snapshot : Type := ContinuationSnapshot {
  current_epoch : Epoch;
  current_metadata : Metadata;
  current_persistent : bool;
  current_present : bool;
  other_continuation_present : bool
}.

Record continuation_ticket : Type := ContinuationTicket {
  ticket_epoch : Epoch;
  ticket_metadata : Metadata;
  ticket_persistent : bool
}.

Definition ticket_valid
    (current : continuation_snapshot) (ticket : continuation_ticket) : Prop :=
  current_present current = true /\
  current_epoch current = ticket_epoch ticket /\
  current_metadata current = ticket_metadata ticket /\
  current_persistent current = ticket_persistent ticket.

Record public_state : Type := PublicState {
  public_data : @bag Key;
  public_named_continuation : bool;
  public_join : bool;
  public_unrelated_state : OtherState;
  public_comm : bool
}.

Definition initial_state
    (subject : @bag Key)
    (current : continuation_snapshot)
    (unrelated : OtherState) : public_state :=
  PublicState subject (current_present current) true unrelated false.

Definition committed_state
    (subject additions removals : @bag Key)
    (current : continuation_snapshot)
    (unrelated : OtherState) : public_state :=
  PublicState
    (apply_delta subject additions removals)
    (current_persistent current)
    (orb (current_persistent current) (other_continuation_present current))
    unrelated
    false.

Definition acceptance_ready
    (subject removals : @bag Key)
    (current : continuation_snapshot)
    (ticket : continuation_ticket)
    (persistent_witnesses_present : Prop) : Prop :=
  ticket_valid current ticket /\
  persistent_witnesses_present /\
  delta_applicable subject removals.

Inductive match_decision : Type :=
| MatchRefused
| MatchCommitted.

Record match_outcome : Type := MatchOutcome {
  outcome_decision : match_decision;
  outcome_state : public_state
}.

Inductive accepted_match_transition
    (subject additions removals : @bag Key)
    (current : continuation_snapshot)
    (ticket : continuation_ticket)
    (persistent_witnesses_present : Prop)
    (unrelated : OtherState) : match_outcome -> Prop :=
| TransitionCommitted :
    acceptance_ready subject removals current ticket persistent_witnesses_present ->
    accepted_match_transition
      subject additions removals current ticket persistent_witnesses_present unrelated
      (MatchOutcome MatchCommitted
        (committed_state subject additions removals current unrelated))
| TransitionRefused :
    ~ acceptance_ready subject removals current ticket persistent_witnesses_present ->
    accepted_match_transition
      subject additions removals current ticket persistent_witnesses_present unrelated
      (MatchOutcome MatchRefused (initial_state subject current unrelated)).

Definition publish_comm (outcome : match_outcome) : match_outcome :=
  match outcome_decision outcome with
  | MatchRefused => outcome
  | MatchCommitted =>
      let state := outcome_state outcome in
      MatchOutcome MatchCommitted
        (PublicState
          (public_data state)
          (public_named_continuation state)
          (public_join state)
          (public_unrelated_state state)
          true)
  end.

Theorem transition_is_deterministic :
  forall subject additions removals current ticket witnesses unrelated left right,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated left ->
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated right ->
    left = right.
Proof.
  intros subject additions removals current ticket witnesses unrelated left right
    left_step right_step.
  inversion left_step; inversion right_step; subst; try contradiction; reflexivity.
Qed.

Theorem refusal_is_observably_nonmutating :
  forall subject additions removals current ticket witnesses unrelated outcome,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    outcome_decision outcome = MatchRefused ->
    outcome_state outcome = initial_state subject current unrelated.
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome step refused.
  inversion step; subst; simpl in refused; try discriminate; reflexivity.
Qed.

Theorem accepted_state_is_one_complete_proposal :
  forall subject additions removals current ticket witnesses unrelated outcome,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    outcome_decision outcome = MatchCommitted ->
    outcome_state outcome = committed_state subject additions removals current unrelated.
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome step committed.
  inversion step; subst; simpl in committed; try discriminate; reflexivity.
Qed.

Theorem stale_epoch_cannot_commit :
  forall subject additions removals current ticket witnesses unrelated outcome,
    current_epoch current <> ticket_epoch ticket ->
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    outcome = MatchOutcome MatchRefused (initial_state subject current unrelated).
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome stale step.
  inversion step as [ready | not_ready]; subst.
  - destruct ready as [[_ [epoch_equal _]] _]. contradiction.
  - reflexivity.
Qed.

Theorem missing_persistent_witness_cannot_commit :
  forall subject additions removals current ticket unrelated outcome,
    accepted_match_transition
      subject additions removals current ticket False unrelated outcome ->
    outcome = MatchOutcome MatchRefused (initial_state subject current unrelated).
Proof.
  intros subject additions removals current ticket unrelated outcome step.
  inversion step as [ready | not_ready]; subst.
  - destruct ready as [_ [witness _]]. contradiction.
  - reflexivity.
Qed.

Theorem inapplicable_delta_cannot_commit :
  forall subject additions removals current ticket witnesses unrelated outcome,
    ~ delta_applicable subject removals ->
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    outcome = MatchOutcome MatchRefused (initial_state subject current unrelated).
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome
    inapplicable step.
  inversion step as [ready | not_ready]; subst.
  - destruct ready as [_ [_ applicable]]. contradiction.
  - reflexivity.
Qed.

Theorem comm_publication_follows_a_complete_commit :
  forall subject additions removals current ticket witnesses unrelated outcome,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    public_comm (outcome_state (publish_comm outcome)) = true ->
    outcome_decision outcome = MatchCommitted /\
    public_data (outcome_state (publish_comm outcome)) =
      apply_delta subject additions removals /\
    public_named_continuation (outcome_state (publish_comm outcome)) =
      current_persistent current /\
    public_join (outcome_state (publish_comm outcome)) =
      orb (current_persistent current) (other_continuation_present current).
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome step logged.
  inversion step; subst; simpl in *; try discriminate; repeat split; reflexivity.
Qed.

Theorem unrelated_group_is_preserved :
  forall subject additions removals current ticket witnesses unrelated outcome,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated outcome ->
    public_unrelated_state (outcome_state outcome) = unrelated /\
    public_unrelated_state (outcome_state (publish_comm outcome)) = unrelated.
Proof.
  intros subject additions removals current ticket witnesses unrelated outcome step.
  inversion step; subst; simpl; split; reflexivity.
Qed.

Theorem play_replay_same_ticket_same_result :
  forall subject additions removals current ticket witnesses unrelated play replay,
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated play ->
    accepted_match_transition
      subject additions removals current ticket witnesses unrelated replay ->
    publish_comm play = publish_comm replay.
Proof.
  intros subject additions removals current ticket witnesses unrelated play replay
    play_step replay_step.
  rewrite (transition_is_deterministic play_step replay_step).
  reflexivity.
Qed.

End RSpaceCommit.

Section MatchingLifecycle.

Context {Datum Binding : Type}.

Definition matched_candidate := (Datum * Binding)%type.

Variable admissible : matched_candidate -> bool.

Fixpoint first_admissible (candidates : list matched_candidate)
    : option matched_candidate :=
  match candidates with
  | [] => None
  | candidate :: remaining =>
      if admissible candidate
      then Some candidate
      else first_admissible remaining
  end.

Lemma replay_subsequence_preserves_first_admissible_bindings :
  forall (keep : matched_candidate -> bool) candidates selected,
    first_admissible candidates = Some selected ->
    keep selected = true ->
    first_admissible (filter keep candidates) = Some selected.
Proof.
  intros keep candidates.
  induction candidates as [|candidate remaining induction_hypothesis];
    intros selected first kept; simpl in *.
  - discriminate.
  - destruct (admissible candidate) eqn:eligible.
    + inversion first; subst selected.
      rewrite kept. simpl. rewrite eligible. reflexivity.
    + destruct (keep candidate) eqn:retained; simpl.
      * rewrite eligible. apply induction_hypothesis; assumption.
      * apply induction_hypothesis; assumption.
Qed.

End MatchingLifecycle.

Print Assumptions refusal_is_observably_nonmutating.
Print Assumptions stale_epoch_cannot_commit.
Print Assumptions missing_persistent_witness_cannot_commit.
Print Assumptions inapplicable_delta_cannot_commit.
Print Assumptions comm_publication_follows_a_complete_commit.
Print Assumptions play_replay_same_ticket_same_result.
