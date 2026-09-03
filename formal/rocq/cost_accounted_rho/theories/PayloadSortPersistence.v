From Stdlib Require Import Bool.Bool Lists.List.
Import ListNotations.

Record complete_payload : Type := {
  payload_authority : nat;
  payload_stack : list nat;
  payload_conditionals : list bool
}.

Definition store_payload (payload : complete_payload) : complete_payload := payload.

Definition capture_free_variable
  (payload : complete_payload)
  : complete_payload := payload.

Definition execute_payload
  (payload : complete_payload)
  : complete_payload := payload.

Definition replay_payload
  (payload : complete_payload)
  : complete_payload := payload.

Theorem storage_preserves_complete_payload :
  forall payload,
    store_payload payload = payload.
Proof.
  reflexivity.
Qed.

Theorem free_capture_preserves_complete_payload :
  forall payload,
    capture_free_variable payload = payload.
Proof.
  reflexivity.
Qed.

Theorem execution_preserves_complete_payload :
  forall payload,
    execute_payload (capture_free_variable (store_payload payload)) = payload.
Proof.
  reflexivity.
Qed.

Theorem replay_preserves_complete_payload :
  forall payload,
    replay_payload
      (execute_payload (capture_free_variable (store_payload payload))) = payload.
Proof.
  reflexivity.
Qed.

Theorem free_capture_preserves_authority :
  forall payload,
    payload_authority (capture_free_variable payload) = payload_authority payload.
Proof.
  reflexivity.
Qed.

Theorem free_capture_preserves_stack_order :
  forall payload,
    payload_stack (capture_free_variable payload) = payload_stack payload.
Proof.
  reflexivity.
Qed.

Theorem free_capture_preserves_conditionals :
  forall payload,
    payload_conditionals (capture_free_variable payload) =
    payload_conditionals payload.
Proof.
  reflexivity.
Qed.

Print Assumptions storage_preserves_complete_payload.
Print Assumptions free_capture_preserves_complete_payload.
Print Assumptions execution_preserves_complete_payload.
Print Assumptions replay_preserves_complete_payload.
Print Assumptions free_capture_preserves_authority.
Print Assumptions free_capture_preserves_stack_order.
Print Assumptions free_capture_preserves_conditionals.
