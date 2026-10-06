From Stdlib Require Import Bool.Bool Lists.List.
Import ListNotations.

Inductive runtime_signature : Type :=
| RSigGround : nat -> runtime_signature
| RSigBound : nat -> runtime_signature
| RSigCompound : runtime_signature -> runtime_signature -> runtime_signature.

Fixpoint contains_bound (signature : runtime_signature) : bool :=
  match signature with
  | RSigGround _ => false
  | RSigBound _ => true
  | RSigCompound lhs rhs => contains_bound lhs || contains_bound rhs
  end.

Definition static_authority_projection
  (signatures : list runtime_signature)
  : list runtime_signature :=
  filter (fun signature => negb (contains_bound signature)) signatures.

Fixpoint resolve_signature
  (environment : nat -> nat)
  (signature : runtime_signature)
  : runtime_signature :=
  match signature with
  | RSigGround value => RSigGround value
  | RSigBound index => RSigGround (environment index)
  | RSigCompound lhs rhs =>
      RSigCompound
        (resolve_signature environment lhs)
        (resolve_signature environment rhs)
  end.

Theorem bound_authority_is_excluded_from_static_capacity :
  forall signatures index,
    ~ In (RSigBound index) (static_authority_projection signatures).
Proof.
  intros signatures index present.
  apply filter_In in present.
  destruct present as [_ selected].
  discriminate selected.
Qed.

Theorem resolved_authority_is_static :
  forall signatures value,
    In (RSigGround value) signatures ->
    In (RSigGround value) (static_authority_projection signatures).
Proof.
  intros signatures value present.
  apply filter_In.
  split.
  - exact present.
  - reflexivity.
Qed.

Theorem runtime_resolution_eliminates_bound_levels :
  forall environment signature,
    contains_bound (resolve_signature environment signature) = false.
Proof.
  intros environment signature.
  induction signature as [value | index | lhs IHlhs rhs IHrhs].
  - reflexivity.
  - reflexivity.
  - simpl.
    now rewrite IHlhs, IHrhs.
Qed.

Record persisted_continuation : Type := {
  persisted_outer : runtime_signature;
  persisted_inner : runtime_signature
}.

Definition instantiate_lollipop
  (environment : nat -> nat)
  (outer inner : runtime_signature)
  : persisted_continuation :=
  {|
    persisted_outer := resolve_signature environment outer;
    persisted_inner := resolve_signature environment inner
  |}.

Definition replay_persisted_continuation
  (continuation : persisted_continuation)
  : persisted_continuation := continuation.

Theorem new_bound_slot_becomes_the_persisted_continuation_authority :
  forall environment outer slot_index,
    persisted_inner
      (instantiate_lollipop environment outer (RSigBound slot_index)) =
    RSigGround (environment slot_index).
Proof.
  reflexivity.
Qed.

Theorem replay_preserves_the_resolved_slot_identity :
  forall environment outer slot_index,
    persisted_inner
      (replay_persisted_continuation
        (instantiate_lollipop environment outer (RSigBound slot_index))) =
    RSigGround (environment slot_index).
Proof.
  reflexivity.
Qed.

Definition creator_preflight_supply
  (authenticated_prestate_supply _candidate_stack_depth : nat)
  : nat := authenticated_prestate_supply.

Theorem candidate_stack_does_not_inflate_creator_preflight_capacity :
  forall authenticated_prestate_supply first_stack second_stack,
    creator_preflight_supply authenticated_prestate_supply first_stack =
    creator_preflight_supply authenticated_prestate_supply second_stack.
Proof.
  reflexivity.
Qed.

Record deploy_normalizer_environment : Type := {
  deployer_identity : nat;
  cosigner_identities : list nat
}.

Inductive deploy_system_reference : Type :=
| RefDeployerIdentity
| RefCosignerCount
| RefLiteral : nat -> deploy_system_reference.

Definition resolve_system_reference
  (environment : option deploy_normalizer_environment)
  (reference : deploy_system_reference)
  : option nat :=
  match reference with
  | RefDeployerIdentity =>
      match environment with
      | Some context => Some (deployer_identity context)
      | None => None
      end
  | RefCosignerCount =>
      match environment with
      | Some context => Some (length (cosigner_identities context))
      | None => None
      end
  | RefLiteral value => Some value
  end.

Definition normalize_deploy_program
  (environment : option deploy_normalizer_environment)
  (program : list deploy_system_reference)
  : list (option nat) :=
  map (resolve_system_reference environment) program.

Definition certify_normalized_program := normalize_deploy_program.
Definition execute_normalized_program := normalize_deploy_program.
Definition replay_normalized_program := normalize_deploy_program.

Theorem certification_execution_replay_share_authenticated_environment :
  forall environment program,
    certify_normalized_program (Some environment) program =
      execute_normalized_program (Some environment) program /\
    execute_normalized_program (Some environment) program =
      replay_normalized_program (Some environment) program.
Proof.
  intros environment program.
  split; reflexivity.
Qed.

Theorem empty_certification_environment_diverges_on_deployer_identity :
  forall environment,
    certify_normalized_program None [RefDeployerIdentity] <>
      execute_normalized_program (Some environment) [RefDeployerIdentity].
Proof.
  intros environment equality.
  discriminate equality.
Qed.

Print Assumptions bound_authority_is_excluded_from_static_capacity.
Print Assumptions resolved_authority_is_static.
Print Assumptions runtime_resolution_eliminates_bound_levels.
Print Assumptions new_bound_slot_becomes_the_persisted_continuation_authority.
Print Assumptions replay_preserves_the_resolved_slot_identity.
Print Assumptions candidate_stack_does_not_inflate_creator_preflight_capacity.
Print Assumptions certification_execution_replay_share_authenticated_environment.
Print Assumptions empty_certification_environment_diverges_on_deployer_identity.
