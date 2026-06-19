From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Slashing Require Import Validator.

Set Implicit Arguments.

Definition Epoch := nat.

Record ValidatorLifetimeId : Type := mkValidatorLifetimeId {
  vl_validator : Validator;
  vl_epoch : Epoch
}.

Definition same_lifetime (a b : ValidatorLifetimeId) : Prop :=
  vl_validator a = vl_validator b /\ vl_epoch a = vl_epoch b.

Definition evidence_authorizes_lifetime
  (evidence target : ValidatorLifetimeId) : bool :=
  if validator_eq_dec (vl_validator evidence) (vl_validator target) then
    Nat.eqb (vl_epoch evidence) (vl_epoch target)
  else false.

Theorem same_key_different_epoch_distinct :
  forall v e1 e2,
    e1 <> e2 ->
    ~ same_lifetime (mkValidatorLifetimeId v e1) (mkValidatorLifetimeId v e2).
Proof.
  intros v e1 e2 Hneq [_ Hep]. contradiction.
Qed.

Theorem stale_evidence_not_authorized :
  forall v e_old e_new,
    e_old <> e_new ->
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId v e_old)
      (mkValidatorLifetimeId v e_new) = false.
Proof.
  intros v e_old e_new Hneq.
  unfold evidence_authorizes_lifetime; simpl.
  destruct (validator_eq_dec v v) as [_ | Hbad]; [|contradiction].
  apply Nat.eqb_neq. assumption.
Qed.

Theorem matching_lifetime_authorized :
  forall v e,
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId v e)
      (mkValidatorLifetimeId v e) = true.
Proof.
  intros v e.
  unfold evidence_authorizes_lifetime; simpl.
  destruct (validator_eq_dec v v) as [_ | Hbad]; [|contradiction].
  apply Nat.eqb_refl.
Qed.
