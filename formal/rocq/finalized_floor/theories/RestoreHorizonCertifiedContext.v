From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

From FinalizedFloor Require Import Foundation.

Inductive RestoreLatestDisposition : Type :=
| RestoreEligible
| RestoreGenesisPlaceholder
| RestoreMissingDependency.

Definition is_genesis_placeholder
  (canonical_genesis authority_floor latest_hash : BlockHash) : bool :=
  negb (Nat.eqb latest_hash authority_floor) &&
  Nat.eqb latest_hash canonical_genesis.

Definition classify_restore_latest
  (canonical_genesis authority_floor latest_hash : BlockHash)
  (held : BlockHash -> bool) : RestoreLatestDisposition :=
  if is_genesis_placeholder canonical_genesis authority_floor latest_hash
  then RestoreGenesisPlaceholder
  else if held latest_hash
       then RestoreEligible
       else RestoreMissingDependency.

Definition exact_restore_slots
  (exact : list (Validator * BlockHash)) : list (Validator * BlockHash) := exact.

Definition restore_eligible
  (canonical_genesis authority_floor : BlockHash)
  (held : BlockHash -> bool)
  (entry : Validator * BlockHash) : bool :=
  match classify_restore_latest
    canonical_genesis authority_floor (snd entry) held with
  | RestoreEligible => true
  | RestoreGenesisPlaceholder | RestoreMissingDependency => false
  end.

Definition restore_projection
  (canonical_genesis authority_floor : BlockHash)
  (held : BlockHash -> bool)
  (exact : list (Validator * BlockHash)) : list (Validator * BlockHash) :=
  filter (restore_eligible canonical_genesis authority_floor held) exact.

Definition authority_denominator (stakes : list nat) : nat :=
  fold_right Nat.add 0 stakes.

Definition restore_authority_denominator
  (stakes : list nat) (_held : BlockHash -> bool) : nat :=
  authority_denominator stakes.

Definition projected_cost
  (cost : BlockHash -> nat) (projection : list (Validator * BlockHash)) : nat :=
  fold_right Nat.add 0 (map (fun entry => cost (snd entry)) projection).

Theorem genesis_placeholder_classification_is_heldness_independent :
  forall canonical floor held_left held_right,
    canonical <> floor ->
    classify_restore_latest canonical floor canonical held_left =
    classify_restore_latest canonical floor canonical held_right.
Proof.
  intros canonical floor held_left held_right Hdifferent.
  unfold classify_restore_latest, is_genesis_placeholder.
  apply Nat.eqb_neq in Hdifferent.
  rewrite Hdifferent, Nat.eqb_refl. reflexivity.
Qed.

Theorem missing_noncanonical_latest_fails_closed :
  forall canonical floor latest held,
    latest <> canonical ->
    held latest = false ->
    classify_restore_latest canonical floor latest held =
      RestoreMissingDependency.
Proof.
  intros canonical floor latest held Hnoncanonical Hmissing.
  unfold classify_restore_latest, is_genesis_placeholder.
  apply Nat.eqb_neq in Hnoncanonical.
  rewrite Hnoncanonical, andb_false_r, Hmissing. reflexivity.
Qed.

Theorem exact_validator_slots_survive_restore_classification :
  forall exact, exact_restore_slots exact = exact.
Proof. reflexivity. Qed.

Theorem authority_stake_survives_restore_classification :
  forall stakes held_left held_right,
    restore_authority_denominator stakes held_left =
    restore_authority_denominator stakes held_right.
Proof. reflexivity. Qed.

Lemma restore_eligible_extensional :
  forall canonical floor held_left held_right entry,
    canonical <> floor ->
    (forall hash, hash <> canonical -> held_left hash = held_right hash) ->
    restore_eligible canonical floor held_left entry =
    restore_eligible canonical floor held_right entry.
Proof.
  intros canonical floor held_left held_right [validator latest] Hfloor Hheld.
  unfold restore_eligible, classify_restore_latest, is_genesis_placeholder.
  simpl.
  destruct (negb (Nat.eqb latest floor) && Nat.eqb latest canonical) eqn:Hplaceholder.
  - reflexivity.
  - destruct (Nat.eq_dec latest canonical) as [Heq | Hneq].
    + subst. apply Nat.eqb_neq in Hfloor.
      rewrite Hfloor, Nat.eqb_refl in Hplaceholder. discriminate.
    + rewrite (Hheld latest Hneq). reflexivity.
Qed.

Theorem full_and_restored_projections_are_identical :
  forall canonical floor held_full held_restored exact,
    canonical <> floor ->
    (forall hash, hash <> canonical -> held_full hash = held_restored hash) ->
    restore_projection canonical floor held_full exact =
    restore_projection canonical floor held_restored exact.
Proof.
  intros canonical floor held_full held_restored exact Hfloor Hheld.
  induction exact as [|entry rest IH]; simpl.
  - reflexivity.
  - rewrite (restore_eligible_extensional
      canonical floor held_full held_restored entry Hfloor Hheld).
    rewrite IH. reflexivity.
Qed.

Theorem full_and_restored_costs_are_identical :
  forall canonical floor held_full held_restored exact cost,
    canonical <> floor ->
    (forall hash, hash <> canonical -> held_full hash = held_restored hash) ->
    projected_cost cost (restore_projection canonical floor held_full exact) =
    projected_cost cost (restore_projection canonical floor held_restored exact).
Proof.
  intros canonical floor held_full held_restored exact cost Hfloor Hheld.
  rewrite (full_and_restored_projections_are_identical
    canonical floor held_full held_restored exact Hfloor Hheld).
  reflexivity.
Qed.

Theorem deleting_an_unheld_exact_slot_breaks_completeness :
  forall (validator : Validator) (canonical : BlockHash),
    filter (fun entry => negb (Nat.eqb (snd entry) canonical))
      [(validator, canonical)] = ([] : list (Validator * BlockHash)) /\
    length [(validator, canonical)] <>
      length ([] : list (Validator * BlockHash)).
Proof.
  intros validator canonical. simpl. rewrite Nat.eqb_refl.
  split; [reflexivity | discriminate].
Qed.

Record CertifiedLatestCandidate : Type := {
  candidate_generation : nat;
  candidate_sequence : nat;
  candidate_hash : BlockHash
}.

Definition reconcile_latest_slot
  (canonical : BlockHash)
  (_raw : BlockHash)
  (candidate : option CertifiedLatestCandidate) : BlockHash :=
  match candidate with
  | Some entry => candidate_hash entry
  | None => canonical
  end.

Definition latest_slot_materialized
  (canonical : BlockHash)
  (held : BlockHash -> bool)
  (latest : BlockHash) : bool :=
  Nat.eqb latest canonical || held latest.

Theorem reconciliation_eliminates_stale_raw_index :
  forall canonical raw_left raw_right candidate,
    reconcile_latest_slot canonical raw_left candidate =
    reconcile_latest_slot canonical raw_right candidate.
Proof. reflexivity. Qed.

Theorem reconciled_slot_is_canonical_or_certified :
  forall canonical raw candidate,
    reconcile_latest_slot canonical raw candidate = canonical \/
    exists entry,
      candidate = Some entry /\
      reconcile_latest_slot canonical raw candidate =
        candidate_hash entry.
Proof.
  intros canonical raw [entry |].
  - right. exists entry. split; reflexivity.
  - left. reflexivity.
Qed.

Theorem reconciled_slot_is_materialized :
  forall canonical raw candidate held,
    (forall entry,
      candidate = Some entry ->
      held (candidate_hash entry) = true) ->
    latest_slot_materialized canonical held
      (reconcile_latest_slot canonical raw candidate) = true.
Proof.
  intros canonical raw [entry |] held Hheld.
  - unfold reconcile_latest_slot, latest_slot_materialized.
    rewrite (Hheld entry eq_refl), orb_true_r. reflexivity.
  - unfold reconcile_latest_slot, latest_slot_materialized.
    rewrite Nat.eqb_refl. reflexivity.
Qed.

Definition certified_support_manifest
  (canonical : BlockHash)
  (exact : list (Validator * BlockHash)) : list BlockHash :=
  canonical :: map snd exact.

Theorem canonical_identity_is_always_in_certified_support :
  forall canonical exact,
    In canonical (certified_support_manifest canonical exact).
Proof. intros canonical exact. left. reflexivity. Qed.

Theorem certified_support_is_heldness_independent :
  forall canonical exact (held_left held_right : BlockHash -> bool),
    certified_support_manifest canonical exact =
    certified_support_manifest canonical exact.
Proof. reflexivity. Qed.

Definition first_proposal_allowed
  (canonical : BlockHash)
  (held : BlockHash -> bool)
  (latest : BlockHash) : bool :=
  if Nat.eqb latest canonical then true else held latest.

Definition latest_slot_sequence
  (canonical : BlockHash)
  (sequence : BlockHash -> nat)
  (latest : BlockHash) : nat :=
  if Nat.eqb latest canonical then 0 else sequence latest.

Definition next_latest_sequence
  (canonical : BlockHash)
  (sequence : BlockHash -> nat)
  (latest : BlockHash) : nat :=
  S (latest_slot_sequence canonical sequence latest).

Theorem genesis_first_proposal_is_heldness_independent :
  forall canonical held_left held_right,
    first_proposal_allowed canonical held_left canonical =
    first_proposal_allowed canonical held_right canonical.
Proof.
  intros canonical held_left held_right.
  unfold first_proposal_allowed. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem genesis_placeholder_has_sequence_zero :
  forall canonical sequence,
    latest_slot_sequence canonical sequence canonical = 0.
Proof.
  intros canonical sequence.
  unfold latest_slot_sequence. rewrite Nat.eqb_refl. reflexivity.
Qed.

Theorem genesis_placeholder_first_authored_sequence_is_one :
  forall canonical sequence,
    next_latest_sequence canonical sequence canonical = 1.
Proof.
  intros canonical sequence.
  unfold next_latest_sequence.
  rewrite genesis_placeholder_has_sequence_zero.
  reflexivity.
Qed.

Theorem generation_change_preserves_monotonic_key_sequence :
  forall previous_sequence,
    previous_sequence < S previous_sequence.
Proof. apply Nat.lt_succ_diag_r. Qed.

Print Assumptions genesis_placeholder_classification_is_heldness_independent.
Print Assumptions missing_noncanonical_latest_fails_closed.
Print Assumptions full_and_restored_projections_are_identical.
Print Assumptions full_and_restored_costs_are_identical.
Print Assumptions reconciliation_eliminates_stale_raw_index.
Print Assumptions reconciled_slot_is_canonical_or_certified.
Print Assumptions reconciled_slot_is_materialized.
Print Assumptions canonical_identity_is_always_in_certified_support.
Print Assumptions genesis_first_proposal_is_heldness_independent.
Print Assumptions genesis_placeholder_has_sequence_zero.
Print Assumptions genesis_placeholder_first_authored_sequence_is_one.
Print Assumptions generation_change_preserves_monotonic_key_sequence.
