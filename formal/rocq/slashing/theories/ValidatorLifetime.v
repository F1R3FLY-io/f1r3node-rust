From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Slashing Require Import Validator.
Import ListNotations.

Set Implicit Arguments.

Definition Epoch := nat.
Definition BondGeneration := nat.
Definition max_bond_generation : BondGeneration := 9223372036854775807.

Record ValidatorLifetimeId : Type := mkValidatorLifetimeId {
  vl_validator : Validator;
  vl_generation : BondGeneration
}.

Definition same_lifetime (a b : ValidatorLifetimeId) : Prop :=
  vl_validator a = vl_validator b /\
  vl_generation a = vl_generation b.

Definition evidence_authorizes_lifetime
  (evidence target : ValidatorLifetimeId) : bool :=
  if validator_eq_dec (vl_validator evidence) (vl_validator target) then
    Nat.eqb (vl_generation evidence) (vl_generation target)
  else false.

Definition GenerationMap := list (Validator * BondGeneration).

Fixpoint gm_lookup
  (generations : GenerationMap) (validator : Validator)
  : option BondGeneration :=
  match generations with
  | [] => None
  | (key, generation) :: rest =>
      if validator_eq_dec key validator
      then Some generation
      else gm_lookup rest validator
  end.

Fixpoint gm_update
  (generations : GenerationMap)
  (validator : Validator)
  (generation : BondGeneration)
  : GenerationMap :=
  match generations with
  | [] => [(validator, generation)]
  | (key, current) :: rest =>
      if validator_eq_dec key validator
      then (validator, generation) :: rest
      else (key, current) :: gm_update rest validator generation
  end.

Definition checked_next_generation
  (generation : BondGeneration) : option BondGeneration :=
  if Nat.ltb generation max_bond_generation
  then Some (S generation)
  else None.

Theorem same_key_different_generation_distinct :
  forall validator first second,
    first <> second ->
    ~ same_lifetime
        (mkValidatorLifetimeId validator first)
        (mkValidatorLifetimeId validator second).
Proof.
  intros validator first second Hneq [_ Heq]. contradiction.
Qed.

Theorem stale_generation_evidence_not_authorized :
  forall validator evidence_generation target_generation,
    evidence_generation <> target_generation ->
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId validator evidence_generation)
      (mkValidatorLifetimeId validator target_generation) = false.
Proof.
  intros validator evidence_generation target_generation Hneq.
  unfold evidence_authorizes_lifetime; simpl.
  destruct (validator_eq_dec validator validator) as [_ | Hbad]; [|contradiction].
  apply Nat.eqb_neq. assumption.
Qed.

Theorem matching_generation_authorized :
  forall validator generation,
    evidence_authorizes_lifetime
      (mkValidatorLifetimeId validator generation)
      (mkValidatorLifetimeId validator generation) = true.
Proof.
  intros validator generation.
  unfold evidence_authorizes_lifetime; simpl.
  destruct (validator_eq_dec validator validator) as [_ | Hbad]; [|contradiction].
  apply Nat.eqb_refl.
Qed.

Theorem same_generation_different_epoch_same_lifetime :
  forall validator generation (first_epoch second_epoch : Epoch),
    same_lifetime
      (mkValidatorLifetimeId validator generation)
      (mkValidatorLifetimeId validator generation).
Proof.
  intros. split; reflexivity.
Qed.

Theorem same_epoch_different_generation_distinct :
  forall validator first_generation second_generation (epoch : Epoch),
    first_generation <> second_generation ->
    ~ same_lifetime
        (mkValidatorLifetimeId validator first_generation)
        (mkValidatorLifetimeId validator second_generation).
Proof.
  intros. apply same_key_different_generation_distinct. assumption.
Qed.

Theorem fresh_bond_generation_strictly_increases :
  forall generation next,
    checked_next_generation generation = Some next ->
    generation < next.
Proof.
  intros generation next Hnext.
  unfold checked_next_generation in Hnext.
  destruct (Nat.ltb generation max_bond_generation) eqn:Hbounded;
    inversion Hnext; lia.
Qed.

Theorem exhausted_generation_rejects_fresh_bond :
  checked_next_generation max_bond_generation = None.
Proof.
  unfold checked_next_generation.
  rewrite Nat.ltb_irrefl. reflexivity.
Qed.

Theorem gm_lookup_update_same :
  forall generations validator generation,
    gm_lookup (gm_update generations validator generation) validator =
    Some generation.
Proof.
  induction generations as [| [key current] rest IH];
    intros validator generation; simpl.
  - destruct (validator_eq_dec validator validator); [reflexivity | contradiction].
  - destruct (validator_eq_dec key validator) as [Heq | Hneq].
    + subst key. simpl.
      destruct (validator_eq_dec validator validator);
        [reflexivity | contradiction].
    + simpl. destruct (validator_eq_dec key validator);
        [contradiction | apply IH].
Qed.

Theorem gm_lookup_update_other :
  forall generations updated queried generation,
    updated <> queried ->
    gm_lookup (gm_update generations updated generation) queried =
    gm_lookup generations queried.
Proof.
  induction generations as [| [key current] rest IH];
    intros updated queried generation Hneq; simpl.
  - destruct (validator_eq_dec updated queried);
      [contradiction | reflexivity].
  - destruct (validator_eq_dec key updated) as [Heq | Hkey].
    + subst key. simpl.
      destruct (validator_eq_dec updated queried);
        [contradiction | reflexivity].
    + simpl. destruct (validator_eq_dec key queried);
        [reflexivity | apply IH; assumption].
Qed.
