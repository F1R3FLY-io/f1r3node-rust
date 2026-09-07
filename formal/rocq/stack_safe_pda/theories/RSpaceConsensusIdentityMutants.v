From Stdlib Require Import Bool Arith PeanoNat Lia Lists.List.
From StackSafePDA Require Import RSpaceConsensusIdentity.
Import ListNotations RSpaceConsensusIdentity.RSpaceConsensusIdentity.

Module RSpaceConsensusIdentityMutants.

Definition original := witness_binding 31 1.
Definition initial := Authority.Owner witness_domain 4 20 7 true true [20] [(1, 20)].
Definition lookup (_ : Authority.owner) (reference : nat) :=
  if Nat.eqb reference 20 then Some original else None.
Definition root_of := @hd nat 0.
Definition refined (_ : bridge_binding) := True.

Example checked_fixture_is_nonvacuous : checked lookup refined root_of initial original.
Proof. repeat split. Qed.

Example completed_publication_is_nonvacuous :
  publish lookup refined root_of (fun _ _ => True)
    initial (Authority.view_permit initial) original original
    (installed_catalog initial original [20] [(1, 20)]) 1.
Proof.
  eapply Publish with (representative := original).
  - repeat split.
  - repeat split.
  - exact I.
  - repeat split.
  - simpl; auto.
  - constructor; try reflexivity.
    + repeat split.
    + unfold Authority.resolves_alias; repeat split.
    + simpl; auto.
  - repeat split.
Qed.

Example checked_recovery_is_nonvacuous :
  recover lookup refined root_of initial original [4] 5
    (Authority.reopen (Authority.close initial) 5) original.
Proof.
  eapply Recover with (representative := original).
  - repeat split.
  - constructor; simpl; intuition discriminate.
  - repeat split.
  - constructor; try reflexivity.
    + repeat split.
    + unfold Authority.resolves_alias; repeat split.
    + simpl; auto.
  - repeat split.
Qed.

Example same_reference_does_not_authorize_different_facet :
  binding_reference original = binding_reference (witness_binding 32 2) /\
  ~ checked lookup refined root_of initial (witness_binding 32 2).
Proof.
  split; [reflexivity|]. apply absent_exact_authentication_refuses.
  unfold authenticated_binding, lookup; simpl; discriminate.
Qed.

Definition manifest_as_root := BridgeBinding 20 witness_domain PreservedLegacyV1
  witness_semantic (CompatibilityFacet 31 [1]) (manifest_reference witness_semantic).

Example authenticated_manifest_digest_is_not_legacy_root :
  ~ checked (fun _ _ => Some manifest_as_root) refined root_of initial manifest_as_root.
Proof. unfold checked; simpl; intuition discriminate. Qed.

Example semantic_bytes_without_refinement_do_not_make_a_bridge :
  ~ checked lookup (fun _ => False) root_of initial original.
Proof. apply absent_refinement_refuses; simpl; auto. Qed.

Definition before_only_lookup (o : Authority.owner) (reference : nat) :=
  if Nat.eqb (Authority.publication_generation o) 7 then lookup o reference else None.

Example prepublication_lookup_does_not_certify_completed_publication :
  checked before_only_lookup refined root_of initial original /\
  ~ exists next exposed,
    publish before_only_lookup refined root_of (fun _ _ => True)
      initial (Authority.view_permit initial) original original next exposed.
Proof.
  split; [repeat split|]. intros [next [exposed step]].
  inversion step; subst.
  unfold checked, authenticated_binding, before_only_lookup, installed_catalog, initial in *.
  simpl in *; intuition discriminate.
Qed.

Example missing_facet_does_not_recover_from_a_retained_reference :
  In (binding_reference original) (Authority.retained_roots initial) /\
  ~ exists reopened recovered,
    recover (fun _ _ => None) refined root_of initial original [4] 5 reopened recovered.
Proof.
  split; [simpl; auto|].
  apply missing_recovery_binding_cannot_fall_back.
  apply absent_exact_authentication_refuses; unfold authenticated_binding; discriminate.
Qed.

Example raw_reopen_reuses_permit_but_checked_recovery_refuses :
  Authority.authorizes (Authority.reopen (Authority.close initial) 4)
    (Authority.view_permit initial) /\
  ~ exists reopened recovered,
    recover lookup refined root_of initial original [4] 4 reopened recovered.
Proof.
  split; [repeat split|]. intros [reopened [recovered step]].
  inversion step; subst.
  match goal with transition : Authority.reopen_transition _ _ _ _ |- _ =>
    inversion transition; subst end.
  intuition.
Qed.

Definition before_restart_lookup (o : Authority.owner) (reference : nat) :=
  if Nat.eqb (Authority.incarnation o) 4 then lookup o reference else None.

Example previously_valid_binding_still_requires_restart_reauthentication :
  checked before_restart_lookup refined root_of initial original /\
  ~ exists reopened recovered,
    recover before_restart_lookup refined root_of initial original [4] 5 reopened recovered.
Proof.
  split; [repeat split|].
  apply missing_recovery_binding_cannot_fall_back.
  apply absent_exact_authentication_refuses.
  unfold authenticated_binding, before_restart_lookup; simpl; discriminate.
Qed.

Print Assumptions checked_fixture_is_nonvacuous.
Print Assumptions completed_publication_is_nonvacuous.
Print Assumptions checked_recovery_is_nonvacuous.
Print Assumptions same_reference_does_not_authorize_different_facet.
Print Assumptions authenticated_manifest_digest_is_not_legacy_root.
Print Assumptions semantic_bytes_without_refinement_do_not_make_a_bridge.
Print Assumptions prepublication_lookup_does_not_certify_completed_publication.
Print Assumptions missing_facet_does_not_recover_from_a_retained_reference.
Print Assumptions raw_reopen_reuses_permit_but_checked_recovery_refuses.
Print Assumptions previously_valid_binding_still_requires_restart_reauthentication.

End RSpaceConsensusIdentityMutants.
