From Stdlib Require Import Arith.PeanoNat Lists.List.
Import ListNotations.

Section FinalizationCertificateRetrieval.

Context {Block Digest : Type}.
Variable block_eq_dec : forall left right : Block, {left = right} + {left <> right}.
Variable digest_eq_dec : forall left right : Digest, {left = right} + {left <> right}.

Inductive retrieval_dependency : Type :=
| BlockDependency : Block -> retrieval_dependency
| CertificateDependency : Digest -> retrieval_dependency.

Theorem typed_dependency_namespace_disjoint :
  forall block digest,
    BlockDependency block <> CertificateDependency digest.
Proof.
  discriminate.
Qed.

Record certificate_response : Type := {
  response_digest : Digest;
  response_content_digest : Digest;
  response_shape_valid : bool
}.

Definition valid_response (response : certificate_response) : Prop :=
  response_shape_valid response = true /\
  response_content_digest response = response_digest response.

Inductive response_result : Type :=
| ResponseIgnored
| ResponsePersisted : Digest -> response_result.

Inductive receive_response
  (tracked : list Digest)
  (response : certificate_response)
  : list Digest -> response_result -> Prop :=
| ReceiveExpectedValid :
    In (response_digest response) tracked ->
    valid_response response ->
    receive_response tracked response
      (remove digest_eq_dec (response_digest response) tracked)
      (ResponsePersisted (response_digest response))
| ReceiveUnexpected :
    ~ In (response_digest response) tracked ->
    receive_response tracked response tracked ResponseIgnored
| ReceiveInvalid :
    In (response_digest response) tracked ->
    ~ valid_response response ->
    receive_response tracked response tracked ResponseIgnored.

Theorem persisted_response_is_expected_and_content_addressed :
  forall tracked response next digest,
    receive_response tracked response next (ResponsePersisted digest) ->
    digest = response_digest response /\
    In digest tracked /\
    response_shape_valid response = true /\
    response_content_digest response = digest.
Proof.
  intros tracked response next digest received.
  inversion received as [expected valid | unexpected | expected invalid]; subst.
  destruct valid as [shape content].
  repeat split; assumption.
Qed.

Theorem unsolicited_response_is_ignored :
  forall tracked response,
    ~ In (response_digest response) tracked ->
    receive_response tracked response tracked ResponseIgnored.
Proof.
  intros tracked response absent.
  exact (ReceiveUnexpected tracked response absent).
Qed.

Theorem invalid_response_is_ignored :
  forall tracked response,
    In (response_digest response) tracked ->
    ~ valid_response response ->
    receive_response tracked response tracked ResponseIgnored.
Proof.
  intros tracked response expected invalid.
  exact (ReceiveInvalid tracked response expected invalid).
Qed.

Lemma remove_digest_absent :
  forall digest tracked,
    ~ In digest (remove digest_eq_dec digest tracked).
Proof.
  intros digest tracked.
  induction tracked as [| head tail induction].
  - simpl.
    tauto.
  - simpl.
    destruct (digest_eq_dec digest head) as [equal | different].
    + subst head.
      exact induction.
    + simpl.
      intros [equal | present].
      * symmetry in equal.
        contradiction.
      * apply induction.
        exact present.
Qed.

Theorem duplicate_response_cannot_persist_twice :
  forall tracked response next,
    receive_response tracked response next
      (ResponsePersisted (response_digest response)) ->
    receive_response next response next ResponseIgnored.
Proof.
  intros tracked response next accepted.
  inversion accepted; subst.
  apply ReceiveUnexpected.
  apply remove_digest_absent.
Qed.

Definition send_failure (tracked : list Digest) : list Digest := tracked.

Theorem failed_send_retains_live_request :
  forall tracked, send_failure tracked = tracked.
Proof.
  reflexivity.
Qed.

Definition rebuild_tracker
  (capacity : nat)
  (persistent_obligations : list Digest)
  : list Digest :=
  firstn capacity persistent_obligations.

Theorem rebuilt_tracker_is_bounded :
  forall capacity persistent_obligations,
    length (rebuild_tracker capacity persistent_obligations) <= capacity.
Proof.
  intros capacity persistent_obligations.
  unfold rebuild_tracker.
  apply firstn_le_length.
Qed.

Theorem bounded_persistent_obligations_are_rebuilt_exactly :
  forall capacity persistent_obligations,
    length persistent_obligations <= capacity ->
    rebuild_tracker capacity persistent_obligations = persistent_obligations.
Proof.
  intros capacity persistent_obligations bounded.
  unfold rebuild_tracker.
  apply firstn_all2.
  exact bounded.
Qed.

Definition enqueue_once (block : Block) (queue : list Block) : list Block :=
  if in_dec block_eq_dec block queue then queue else block :: queue.

Theorem enqueue_once_contains_block :
  forall block queue,
    In block (enqueue_once block queue).
Proof.
  intros block queue.
  unfold enqueue_once.
  destruct (in_dec block_eq_dec block queue) as [present | absent].
  - exact present.
  - simpl.
    tauto.
Qed.

Theorem enqueue_once_is_idempotent :
  forall block queue,
    enqueue_once block (enqueue_once block queue) = enqueue_once block queue.
Proof.
  intros block queue.
  unfold enqueue_once at 1.
  destruct (in_dec block_eq_dec block (enqueue_once block queue)) as
    [present | absent].
  - reflexivity.
  - exfalso.
    apply absent.
    apply enqueue_once_contains_block.
Qed.

Theorem finalization_certificate_retrieval_contract :
  (forall block digest,
    BlockDependency block <> CertificateDependency digest)
  /\
  (forall tracked response next digest,
    receive_response tracked response next (ResponsePersisted digest) ->
    digest = response_digest response /\
    In digest tracked /\
    response_shape_valid response = true /\
    response_content_digest response = digest)
  /\
  (forall tracked response,
    ~ In (response_digest response) tracked ->
    receive_response tracked response tracked ResponseIgnored)
  /\
  (forall tracked response next,
    receive_response tracked response next
      (ResponsePersisted (response_digest response)) ->
    receive_response next response next ResponseIgnored)
  /\
  (forall tracked, send_failure tracked = tracked)
  /\
  (forall capacity persistent_obligations,
    length (rebuild_tracker capacity persistent_obligations) <= capacity)
  /\
  (forall capacity persistent_obligations,
    length persistent_obligations <= capacity ->
    rebuild_tracker capacity persistent_obligations = persistent_obligations)
  /\
  (forall block queue,
    enqueue_once block (enqueue_once block queue) = enqueue_once block queue).
Proof.
  split.
  - exact typed_dependency_namespace_disjoint.
  - split.
    + exact persisted_response_is_expected_and_content_addressed.
    + split.
      * exact unsolicited_response_is_ignored.
      * split.
        -- exact duplicate_response_cannot_persist_twice.
        -- split.
           ++ exact failed_send_retains_live_request.
           ++ split.
              ** exact rebuilt_tracker_is_bounded.
              ** split.
                 --- exact bounded_persistent_obligations_are_rebuilt_exactly.
                 --- exact enqueue_once_is_idempotent.
Qed.

End FinalizationCertificateRetrieval.

Print Assumptions finalization_certificate_retrieval_contract.
