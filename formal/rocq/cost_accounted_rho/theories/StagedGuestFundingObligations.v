From Stdlib Require Import Lists.List Lists.ListDec Bool.Bool Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import NativeAttemptReplay ReplayAdmissionPublication.
Import ListNotations.

Module StagedGuestFundingObligations.

Record guest_owner := {
  owner_session : nat;
  owner_generation : nat;
  owner_root : nat
}.

Record invocation_identity := {
  invocation_owner : guest_owner;
  invocation_prefix : nat;
  invocation_operation : nat;
  invocation_ordinal : nat
}.

Definition guest_owner_eq_dec : forall left right : guest_owner,
    {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Definition invocation_identity_eq_dec : forall left right : invocation_identity,
    {left = right} + {left <> right}.
Proof. decide equality; try apply Nat.eq_dec; apply guest_owner_eq_dec. Defined.

Definition guest_candidate := (nat * nat)%type.

Definition guest_candidate_eq_dec : forall left right : guest_candidate,
    {left = right} + {left <> right}.
Proof. decide equality; apply Nat.eq_dec. Defined.

Record guest_record := {
  record_identity : invocation_identity;
  record_candidates : list guest_candidate
}.

Definition guest_record_eq_dec : forall left right : guest_record,
    {left = right} + {left <> right}.
Proof.
  decide equality; try apply invocation_identity_eq_dec.
  apply list_eq_dec. exact guest_candidate_eq_dec.
Defined.

Definition same_list {A : Type}
    (eq_dec : forall left right : A, {left = right} + {left <> right})
    (left right : list A) : bool :=
  if list_eq_dec eq_dec left right then true else false.

Lemma same_list_exact : forall A eq_dec (left right : list A),
  same_list eq_dec left right = true <-> left = right.
Proof.
  intros A eq_dec left right. unfold same_list.
  destruct (list_eq_dec eq_dec left right); split; intros; congruence.
Qed.

Record monetary_projection := {
  monetary_native_debit : nat;
  monetary_fee : nat;
  monetary_wallet_root : nat;
  monetary_case_identity : nat;
  monetary_case_count : nat
}.

Record guest_publication_state := {
  guest_durable_cache : replay_publication_state;
  guest_money : monetary_projection;
  guest_published_journal : list guest_record
}.

Inductive guest_publication_result :=
| GuestPublished (after : guest_publication_state)
| GuestRefused (before : guest_publication_state).

Definition result_state (result : guest_publication_result) :=
  match result with GuestPublished state | GuestRefused state => state end.

Section CheckerComposition.

Variable complete_roster : invocation_identity -> list guest_candidate.
Variable roster_is_complete : invocation_identity -> list guest_candidate -> Prop.
Hypothesis complete_roster_sound : forall identity,
  roster_is_complete identity (complete_roster identity).

Variable candidate_check : invocation_identity -> guest_candidate -> bool.
Variable candidate_authenticated_and_funded : invocation_identity -> guest_candidate -> Prop.
Hypothesis candidate_check_sound : forall identity candidate,
  candidate_check identity candidate = true ->
  candidate_authenticated_and_funded identity candidate.

Variable SupplyWitness : Type.
Variable authenticated_supply : invocation_identity -> SupplyWitness -> Prop.
Variable candidate_covered : invocation_identity -> SupplyWitness -> guest_candidate -> Prop.

Definition roster_has_common_supply identity candidates : Prop :=
  exists witness, authenticated_supply identity witness /\
    Forall (candidate_covered identity witness) candidates.

Variable roster_supply_check : invocation_identity -> list guest_candidate -> bool.
Hypothesis roster_supply_check_sound : forall identity candidates,
  roster_supply_check identity candidates = true ->
  roster_has_common_supply identity candidates.

Variable artifact_check : guest_owner -> list guest_record -> validated_replay_artifacts -> bool.
Variable artifact_binds_journal : guest_owner -> list guest_record -> validated_replay_artifacts -> Prop.
Hypothesis artifact_check_sound : forall owner journal artifacts,
  artifact_check owner journal artifacts = true ->
  artifact_binds_journal owner journal artifacts.

Definition record_check (owner : guest_owner) (row : guest_record) : bool :=
  (if guest_owner_eq_dec (invocation_owner (record_identity row)) owner
   then true else false) &&
  (match record_candidates row with [] => false | _ :: _ => true end) &&
  same_list guest_candidate_eq_dec (record_candidates row)
    (complete_roster (record_identity row)) &&
  forallb (candidate_check (record_identity row)) (record_candidates row) &&
  roster_supply_check (record_identity row) (record_candidates row).

Definition record_valid (owner : guest_owner) (row : guest_record) : Prop :=
  invocation_owner (record_identity row) = owner /\
  record_candidates row <> [] /\
  record_candidates row = complete_roster (record_identity row) /\
  roster_is_complete (record_identity row) (record_candidates row) /\
  Forall (candidate_authenticated_and_funded (record_identity row))
    (record_candidates row) /\
  roster_has_common_supply (record_identity row) (record_candidates row).

Theorem checked_record_has_complete_authenticated_roster : forall owner row,
  record_check owner row = true -> record_valid owner row.
Proof.
  intros owner row checked.
  unfold record_check in checked.
  repeat rewrite andb_true_iff in checked.
  destruct checked as [[[[Howner Hnonempty] Hroster] Hcandidates] Hsupply].
  destruct (guest_owner_eq_dec (invocation_owner (record_identity row)) owner)
    as [Hsame | Hdifferent]; [|discriminate].
  apply same_list_exact in Hroster.
  unfold record_valid. split; [exact Hsame|].
  split.
  - intros Hempty. rewrite Hempty in Hnonempty. discriminate.
  - split; [exact Hroster|]. split.
    + rewrite Hroster. apply complete_roster_sound.
    + split.
      * apply Forall_forall. intros candidate Hin.
        apply candidate_check_sound.
        rewrite forallb_forall in Hcandidates. now apply Hcandidates.
      * now apply roster_supply_check_sound.
Qed.

Theorem checked_record_retains_one_common_supply : forall owner row,
  record_check owner row = true ->
  exists witness, authenticated_supply (record_identity row) witness /\
    forall candidate, In candidate (record_candidates row) ->
      candidate_covered (record_identity row) witness candidate.
Proof.
  intros owner row checked.
  apply checked_record_has_complete_authenticated_roster in checked.
  destruct checked as [_ [_ [_ [_ [_ [witness [Hauth Hcovers]]]]]]].
  exists witness. split; [exact Hauth|].
  exact (proj1 (@Forall_forall guest_candidate
    (candidate_covered (record_identity row) witness) (record_candidates row)) Hcovers).
Qed.

Theorem failed_common_supply_refuses_record : forall owner row,
  roster_supply_check (record_identity row) (record_candidates row) = false ->
  record_check owner row = false.
Proof.
  intros owner row failed. unfold record_check.
  rewrite failed. apply andb_false_r.
Qed.

Definition journal_check (owner : guest_owner)
    (expected : list invocation_identity) (journal : list guest_record) : bool :=
  (if NoDup_dec invocation_identity_eq_dec expected then true else false) &&
  same_list invocation_identity_eq_dec (map record_identity journal) expected &&
  forallb (record_check owner) journal.

Theorem checked_journal_is_complete_ordered_and_unique : forall owner expected journal,
  journal_check owner expected journal = true ->
  NoDup expected /\ map record_identity journal = expected /\
  Forall (record_valid owner) journal.
Proof.
  intros owner expected journal checked.
  unfold journal_check in checked.
  repeat rewrite andb_true_iff in checked.
  destruct checked as [[Hunique Hsame] Hrows].
  destruct (NoDup_dec invocation_identity_eq_dec expected) as [unique | duplicate];
    [|discriminate].
  apply same_list_exact in Hsame.
  split; [exact unique|]. split; [exact Hsame|].
  apply Forall_forall. intros row Hin.
  apply checked_record_has_complete_authenticated_roster.
  rewrite forallb_forall in Hrows. now apply Hrows.
Qed.

Theorem omitted_reordered_or_duplicated_identity_refuses_journal :
  forall owner expected journal,
  map record_identity journal <> expected ->
  journal_check owner expected journal = false.
Proof.
  intros owner expected journal different.
  destruct (journal_check owner expected journal) eqn:checked; [|reflexivity].
  apply checked_journal_is_complete_ordered_and_unique in checked.
  tauto.
Qed.

Theorem checked_candidate_multiplicity_is_exact : forall owner row candidate,
  record_check owner row = true ->
  count_occ guest_candidate_eq_dec (record_candidates row) candidate =
  count_occ guest_candidate_eq_dec (complete_roster (record_identity row)) candidate.
Proof.
  intros owner row candidate checked.
  apply checked_record_has_complete_authenticated_roster in checked.
  destruct checked as [_ [_ [same _]]]. now rewrite same.
Qed.

Definition append_checked_record (owner : guest_owner)
    (expected : list invocation_identity) (journal : list guest_record)
    (row : guest_record) : option (list guest_record) :=
  match nth_error expected (length journal) with
  | None => None
  | Some next =>
      if invocation_identity_eq_dec (record_identity row) next then
        if record_check owner row then Some (journal ++ [row]) else None
      else None
  end.

Theorem successful_append_preserves_prefix_and_next_identity :
  forall owner expected journal row after,
  append_checked_record owner expected journal row = Some after ->
  after = journal ++ [row] /\
  nth_error expected (length journal) = Some (record_identity row) /\
  record_valid owner row.
Proof.
  intros owner expected journal row after appended.
  unfold append_checked_record in appended.
  destruct (nth_error expected (length journal)) as [next|] eqn:Hnext;
    [|discriminate].
  destruct (invocation_identity_eq_dec (record_identity row) next) as [same|different];
    [|discriminate].
  destruct (record_check owner row) eqn:Hchecked; [|discriminate].
  inversion appended; subst after.
  split; [reflexivity|]. split.
  - now rewrite same.
  - now apply checked_record_has_complete_authenticated_roster.
Qed.

Definition valid_journal_prefix (owner : guest_owner)
    (expected : list invocation_identity) (journal : list guest_record) : Prop :=
  (exists suffix, expected = map record_identity journal ++ suffix) /\
  Forall (record_valid owner) journal.

Theorem successful_append_preserves_valid_journal_prefix :
  forall owner expected journal row after,
  valid_journal_prefix owner expected journal ->
  append_checked_record owner expected journal row = Some after ->
  valid_journal_prefix owner expected after.
Proof.
  intros owner expected journal row after [[suffix Hprefix] Hrows] Happended.
  apply successful_append_preserves_prefix_and_next_identity in Happended
    as [Hafter [Hnext Hrow]].
  subst after. unfold valid_journal_prefix. split.
  - rewrite Hprefix in Hnext.
    rewrite nth_error_app2 in Hnext by (rewrite length_map; lia).
    rewrite length_map, Nat.sub_diag in Hnext.
    destruct suffix as [|head tail]; [discriminate|].
    simpl in Hnext. inversion Hnext; subst head.
    exists tail. rewrite map_app. simpl. rewrite <- app_assoc.
    exact Hprefix.
  - apply Forall_app. split; [exact Hrows|].
    constructor; [exact Hrow|constructor].
Qed.

Definition publish_guest_stage (owner : guest_owner)
    (expected : list invocation_identity) (journal replay : list guest_record)
    (live_at_commit cancelled refused : bool)
    (artifacts : validated_replay_artifacts) (before : guest_publication_state)
    : guest_publication_result :=
  if cancelled then GuestRefused before else
  if refused then GuestRefused before else
  if journal_check owner expected journal then
    if same_list guest_record_eq_dec replay journal then
      if artifact_check owner journal artifacts then
        if live_at_commit then
          match publish_validated artifacts (fst (guest_durable_cache before)) with
          | Published durable => GuestPublished {|
              guest_durable_cache :=
                (durable, cache_insert (replay_execution_id artifacts)
                  (replay_evidence artifacts) (snd (guest_durable_cache before)));
              guest_money := guest_money before;
              guest_published_journal := journal |}
          | PublicationConflict _ => GuestRefused before
          end
        else GuestRefused before
      else GuestRefused before
    else GuestRefused before
  else GuestRefused before.

Theorem accepted_publication_requires_all_obligations :
  forall owner expected journal replay live cancelled refused artifacts before after,
  publish_guest_stage owner expected journal replay live cancelled refused artifacts before =
    GuestPublished after ->
  cancelled = false /\ refused = false /\ live = true /\
  NoDup expected /\ map record_identity journal = expected /\
  Forall (record_valid owner) journal /\ replay = journal /\
  artifact_binds_journal owner journal artifacts /\
  guest_money after = guest_money before /\
  guest_published_journal after = journal.
Proof.
  intros owner expected journal replay live cancelled refused artifacts before after published.
  unfold publish_guest_stage in published.
  destruct cancelled; [discriminate|].
  destruct refused; [discriminate|].
  destruct (journal_check owner expected journal) eqn:Hjournal; [|discriminate].
  destruct (same_list guest_record_eq_dec replay journal) eqn:Hreplay; [|discriminate].
  destruct (artifact_check owner journal artifacts) eqn:Hartifact; [|discriminate].
  destruct live; [|discriminate].
  destruct (publish_validated artifacts (fst (guest_durable_cache before)))
    as [durable|conflict]; [|discriminate].
  inversion published; subst after; simpl.
  apply checked_journal_is_complete_ordered_and_unique in Hjournal.
  apply same_list_exact in Hreplay.
  apply artifact_check_sound in Hartifact.
  tauto.
Qed.

Theorem cancellation_or_refusal_restores_baseline :
  forall owner expected journal replay live cancelled refused artifacts before,
  cancelled = true \/ refused = true ->
  publish_guest_stage owner expected journal replay live cancelled refused artifacts before =
    GuestRefused before.
Proof.
  intros owner expected journal replay live cancelled refused artifacts before failed.
  unfold publish_guest_stage.
  destruct failed as [Hcancelled | Hrefused].
  - now rewrite Hcancelled.
  - rewrite Hrefused. now destruct cancelled.
Qed.

Theorem revoked_capability_cannot_publish :
  forall owner expected journal replay cancelled refused artifacts before,
  publish_guest_stage owner expected journal replay false cancelled refused artifacts before =
    GuestRefused before.
Proof.
  intros. unfold publish_guest_stage.
  destruct cancelled, refused; try reflexivity.
  destruct (journal_check owner expected journal); [|reflexivity].
  destruct (same_list guest_record_eq_dec replay journal); [|reflexivity].
  now destruct (artifact_check owner journal artifacts).
Qed.

Theorem replay_difference_cannot_publish :
  forall owner expected journal replay live cancelled refused artifacts before,
  replay <> journal ->
  publish_guest_stage owner expected journal replay live cancelled refused artifacts before =
    GuestRefused before.
Proof.
  intros owner expected journal replay live cancelled refused artifacts before different.
  unfold publish_guest_stage.
  destruct cancelled, refused; try reflexivity.
  destruct (journal_check owner expected journal); [|reflexivity].
  destruct (same_list guest_record_eq_dec replay journal) eqn:Hsame; [|reflexivity].
  apply same_list_exact in Hsame. contradiction.
Qed.

Theorem every_guest_outcome_preserves_monetary_projection :
  forall owner expected journal replay live cancelled refused artifacts before,
  guest_money (result_state
    (publish_guest_stage owner expected journal replay live cancelled refused artifacts before)) =
    guest_money before.
Proof.
  intros. unfold publish_guest_stage.
  destruct cancelled, refused; try reflexivity.
  destruct (journal_check owner expected journal); [|reflexivity].
  destruct (same_list guest_record_eq_dec replay journal); [|reflexivity].
  destruct (artifact_check owner journal artifacts); [|reflexivity].
  destruct live; [|reflexivity].
  destruct (publish_validated artifacts (fst (guest_durable_cache before))); reflexivity.
Qed.

Theorem one_existing_funding_case_remains_one :
  forall owner expected journal replay live cancelled refused artifacts before,
  monetary_case_count (guest_money before) = 1 ->
  monetary_case_count (guest_money (result_state
    (publish_guest_stage owner expected journal replay live cancelled refused artifacts before))) = 1.
Proof.
  intros. rewrite every_guest_outcome_preserves_monetary_projection. assumption.
Qed.

Theorem conflicting_durable_evidence_refuses_without_cache_change :
  forall owner expected journal replay live cancelled refused artifacts before existing,
  fst (guest_durable_cache before) (replay_execution_id artifacts) = Some existing ->
  existing <> replay_evidence artifacts ->
  publish_guest_stage owner expected journal replay live cancelled refused artifacts before =
    GuestRefused before.
Proof.
  intros owner expected journal replay live cancelled refused artifacts before existing present different.
  unfold publish_guest_stage.
  destruct cancelled, refused; try reflexivity.
  destruct (journal_check owner expected journal); [|reflexivity].
  destruct (same_list guest_record_eq_dec replay journal); [|reflexivity].
  destruct (artifact_check owner journal artifacts); [|reflexivity].
  destruct live; [|reflexivity].
  rewrite (conflicting_publication_preserves_existing_store artifacts
    (fst (guest_durable_cache before)) existing present different).
  reflexivity.
Qed.

Theorem empty_guest_journal_preserves_native_only_eligibility : forall owner,
  journal_check owner [] [] = true.
Proof.
  intros owner. unfold journal_check.
  destruct (NoDup_dec invocation_identity_eq_dec []) as [unique | duplicate].
  - reflexivity.
  - exfalso. apply duplicate. constructor.
Qed.

Theorem guest_extension_preserves_native_replay_usage_and_ceiling :
  forall bound used original native_replay
    owner expected journal replay live cancelled refused artifacts before,
  check_attempt_prefix bound used original = true ->
  Permutation original native_replay ->
  accepted_usage native_replay = accepted_usage original /\
  used + accepted_usage native_replay <= bound /\
  guest_money (result_state
    (publish_guest_stage owner expected journal replay live cancelled refused artifacts before)) =
    guest_money before.
Proof.
  intros bound used original native_replay owner expected journal replay live cancelled refused
    artifacts before checked permuted.
  split.
  - symmetry. now apply accepted_usage_permutation.
  - split.
    + eapply checked_replay_permutation_bounded; eauto.
    + apply every_guest_outcome_preserves_monetary_projection.
Qed.

End CheckerComposition.

Print Assumptions accepted_publication_requires_all_obligations.
Print Assumptions checked_record_has_complete_authenticated_roster.
Print Assumptions checked_record_retains_one_common_supply.
Print Assumptions failed_common_supply_refuses_record.
Print Assumptions guest_extension_preserves_native_replay_usage_and_ceiling.
Print Assumptions one_existing_funding_case_remains_one.

End StagedGuestFundingObligations.
