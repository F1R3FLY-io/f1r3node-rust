From Stdlib Require Import ZArith Lia.
From FinalizedFloor Require Import FtExact.

Open Scope Z_scope.

Definition candidate_floor_certificate := ft_exact_gt.
Definition durable_finalizer_certificate := ft_exact_gt.
Definition inclusive_candidate_control := ft_exact_ge.

Theorem candidate_floor_and_finalizer_equivalent :
  forall q S num den,
    candidate_floor_certificate q S num den <->
    durable_finalizer_certificate q S num den.
Proof. reflexivity. Qed.

Theorem four_validator_boundary_rejected_by_both :
  ~ candidate_floor_certificate 8 16 0 1000000 /\
  ~ durable_finalizer_certificate 8 16 0 1000000.
Proof. unfold candidate_floor_certificate, durable_finalizer_certificate, ft_exact_gt; lia. Qed.

Theorem inclusive_boundary_outpaces_durable_finalization :
  inclusive_candidate_control 8 16 0 1000000 /\
  ~ durable_finalizer_certificate 8 16 0 1000000.
Proof. unfold inclusive_candidate_control, durable_finalizer_certificate.
  unfold ft_exact_ge, ft_exact_gt. lia.
Qed.

Theorem aligned_threshold_contract :
  (forall q S num den,
    candidate_floor_certificate q S num den <->
    durable_finalizer_certificate q S num den)
  /\
  (~ candidate_floor_certificate 8 16 0 1000000 /\
   ~ durable_finalizer_certificate 8 16 0 1000000)
  /\
  (inclusive_candidate_control 8 16 0 1000000 /\
   ~ durable_finalizer_certificate 8 16 0 1000000).
Proof.
  exact (conj candidate_floor_and_finalizer_equivalent
    (conj four_validator_boundary_rejected_by_both
      inclusive_boundary_outpaces_durable_finalization)).
Qed.

Print Assumptions aligned_threshold_contract.

Close Scope Z_scope.
