From Stdlib Require Import Bool.Bool.

Inductive candidate_self_chain_disposition : Type :=
| NotOnSelfChain
| ActiveDuplicate
| ExcludedBranchRehome
| SelectedRecovery.

Definition classify_candidate_self_chain
  (on_self_chain active_in_candidate_scope selected_recovery : bool)
  : candidate_self_chain_disposition :=
  if negb on_self_chain then NotOnSelfChain
  else if selected_recovery then SelectedRecovery
  else if active_in_candidate_scope then ActiveDuplicate
  else ExcludedBranchRehome.

Definition should_package_candidate
  (disposition : candidate_self_chain_disposition) : bool :=
  match disposition with
  | ActiveDuplicate => false
  | _ => true
  end.

Theorem active_candidate_duplicate_is_suppressed :
  classify_candidate_self_chain true true false = ActiveDuplicate /\
  should_package_candidate ActiveDuplicate = false.
Proof.
  split; reflexivity.
Qed.

Theorem excluded_branch_occurrence_is_rehomed :
  classify_candidate_self_chain true false false = ExcludedBranchRehome /\
  should_package_candidate ExcludedBranchRehome = true.
Proof.
  split; reflexivity.
Qed.

Theorem selected_recovery_preserves_authorization :
  forall active_in_candidate_scope,
    classify_candidate_self_chain true active_in_candidate_scope true = SelectedRecovery /\
    should_package_candidate SelectedRecovery = true.
Proof.
  intros active_in_candidate_scope.
  split; reflexivity.
Qed.

Theorem non_self_chain_candidate_is_packaged :
  forall active_in_candidate_scope selected_recovery,
    classify_candidate_self_chain false active_in_candidate_scope selected_recovery =
      NotOnSelfChain /\
    should_package_candidate NotOnSelfChain = true.
Proof.
  intros active_in_candidate_scope selected_recovery.
  split; reflexivity.
Qed.

Theorem only_active_candidate_duplicate_is_suppressed :
  forall on_self_chain active_in_candidate_scope selected_recovery,
    should_package_candidate
      (classify_candidate_self_chain
        on_self_chain active_in_candidate_scope selected_recovery) = false <->
    on_self_chain = true /\
    active_in_candidate_scope = true /\
    selected_recovery = false.
Proof.
  intros on_self_chain active_in_candidate_scope selected_recovery.
  destruct on_self_chain, active_in_candidate_scope, selected_recovery;
    simpl; intuition congruence.
Qed.

Theorem candidate_scope_authorization_is_total :
  forall on_self_chain active_in_candidate_scope selected_recovery,
    should_package_candidate
      (classify_candidate_self_chain
        on_self_chain active_in_candidate_scope selected_recovery) = true \/
    should_package_candidate
      (classify_candidate_self_chain
        on_self_chain active_in_candidate_scope selected_recovery) = false.
Proof.
  intros on_self_chain active_in_candidate_scope selected_recovery.
  destruct on_self_chain, active_in_candidate_scope, selected_recovery;
    simpl; auto.
Qed.
