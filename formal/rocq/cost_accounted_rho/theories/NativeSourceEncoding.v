From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Fixpoint encode_chunks (chunks : list (list nat)) (used limit : nat)
  : option (list nat * nat) :=
  match chunks with
  | [] => Some ([], used)
  | chunk :: rest =>
      let next := used + length chunk in
      if next <=? limit then
        match encode_chunks rest next limit with
        | Some (bytes, final) => Some (chunk ++ bytes, final)
        | None => None
        end
      else None
  end.

Theorem successful_encoding_preserves_bytes : forall chunks used limit bytes final,
  encode_chunks chunks used limit = Some (bytes, final) ->
  bytes = concat chunks /\ final = used + length bytes.
Proof.
  induction chunks as [|chunk rest IH]; intros used limit bytes final result.
  - simpl in result. inversion result. simpl. auto.
  - simpl in result. destruct (used + length chunk <=? limit); [|discriminate].
    destruct (encode_chunks rest (used + length chunk) limit) as [[tail total]|] eqn:next;
      [|discriminate].
    inversion result; subst. specialize (IH _ _ _ _ next).
    destruct IH as [same counted]. split; [simpl; now rewrite same|].
    rewrite length_app. lia.
Qed.

Theorem successful_encoding_respects_limit : forall chunks used limit bytes final,
  used <= limit -> encode_chunks chunks used limit = Some (bytes, final) -> final <= limit.
Proof.
  induction chunks as [|chunk rest IH]; intros used limit bytes final fits result.
  - simpl in result. inversion result; subst. exact fits.
  - simpl in result. destruct (used + length chunk <=? limit) eqn:fits_next;
      [|discriminate].
    apply Nat.leb_le in fits_next.
    destruct (encode_chunks rest (used + length chunk) limit) as [[tail total]|] eqn:next;
      [|discriminate].
    inversion result; subst. eapply IH; eauto.
Qed.

Theorem rejected_chunk_emits_no_result : forall chunk rest used limit,
  limit < used + length chunk -> encode_chunks (chunk :: rest) used limit = None.
Proof.
  intros. simpl. destruct (used + length chunk <=? limit) eqn:fits; [|reflexivity].
  apply Nat.leb_le in fits. lia.
Qed.

Theorem equal_encodings_preserve_identity : forall (Hash : list nat -> nat)
  chunks used limit bytes final,
  encode_chunks chunks used limit = Some (bytes, final) -> Hash bytes = Hash (concat chunks).
Proof.
  intros. pose proof (successful_encoding_preserves_bytes _ _ _ _ _ H) as [same _].
  now rewrite same.
Qed.

Definition growth_charge (old additional : nat) : nat := old + additional.

Theorem growth_reserves_new_allocation_before_copy : forall old additional limit used,
  used + growth_charge old additional <= limit ->
  used + old <= limit /\ used + additional <= limit.
Proof. unfold growth_charge. intros. lia. Qed.

Example payload_only_budget_omits_framing :
  encode_chunks [[1; 0; 0; 0; 0; 0; 0; 0]; [42]] 0 1 = None.
Proof. reflexivity. Qed.
