From Stdlib Require Import Arith.PeanoNat Lia Lists.List.

Import ListNotations.

Section ForcedRedexAccounting.

Context {region cell : Type}.

Definition consume_forces
  (occurrences : list region)
  (stack : list cell)
  : option (list cell) :=
  if Nat.leb (length occurrences) (length stack)
  then Some (skipn (length occurrences) stack)
  else None.

Theorem every_forced_redex_consumes_one_cell :
  forall occurrences stack tail,
    consume_forces occurrences stack = Some tail ->
    length stack = length occurrences + length tail.
Proof.
  intros occurrences stack tail Hconsume.
  unfold consume_forces in Hconsume.
  destruct (Nat.leb (length occurrences) (length stack)) eqn:Hfits;
    try discriminate.
  inversion Hconsume; subst tail.
  apply Nat.leb_le in Hfits.
  rewrite length_skipn.
  lia.
Qed.

Theorem repeated_region_occurrences_preserve_multiplicity :
  forall region_id stack tail,
    consume_forces [region_id; region_id] stack = Some tail ->
    length stack = 2 + length tail.
Proof.
  intros region_id stack tail Hconsume.
  pose proof (every_forced_redex_consumes_one_cell
    [region_id; region_id] stack tail Hconsume) as Hlength.
  simpl in Hlength.
  exact Hlength.
Qed.

Theorem insufficient_stack_rejects_without_tail :
  forall occurrences stack,
    length stack < length occurrences ->
    consume_forces occurrences stack = None.
Proof.
  intros occurrences stack Hinsufficient.
  unfold consume_forces.
  destruct (Nat.leb (length occurrences) (length stack)) eqn:Hfits.
  - apply Nat.leb_le in Hfits.
    lia.
  - reflexivity.
Qed.

Theorem certified_replay_consumption_is_deterministic :
  forall occurrences stack committed replayed,
    consume_forces occurrences stack = Some committed ->
    consume_forces occurrences stack = Some replayed ->
    committed = replayed.
Proof.
  intros occurrences stack committed replayed Hcommitted Hreplayed.
  congruence.
Qed.

End ForcedRedexAccounting.
