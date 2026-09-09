From Stdlib Require Import Arith Bool Lia Lists.List.
Import ListNotations.

Definition owners := nat -> option nat.
Definition empty_owners : owners := fun _ => None.
Definition install (state : owners) (key token : nat) : owners :=
  fun other => if Nat.eq_dec other key then
    match state key with None => Some token | held => held end
  else state other.
Definition release (state : owners) (key token : nat) : owners :=
  fun other => if Nat.eq_dec other key then
    match state key with
    | Some held => if Nat.eq_dec held token then None else Some held
    | None => None
    end
  else state other.

Theorem vacant_claim_has_exact_owner : forall state key token,
  state key = None -> install state key token key = Some token.
Proof. intros. unfold install. destruct Nat.eq_dec; [now rewrite H|congruence]. Qed.

Theorem duplicate_claim_preserves_owner : forall state key old new,
  state key = Some old -> install state key new key = Some old.
Proof. intros. unfold install. destruct Nat.eq_dec; [now rewrite H|congruence]. Qed.

Theorem claim_preserves_other_keys : forall state key token other,
  other <> key -> install state key token other = state other.
Proof. intros. unfold install. destruct Nat.eq_dec; congruence. Qed.

Theorem exact_release_clears_owner : forall state key token,
  state key = Some token -> release state key token key = None.
Proof.
  intros. unfold release. destruct Nat.eq_dec; [rewrite H|congruence].
  destruct Nat.eq_dec; congruence.
Qed.

Theorem stale_release_preserves_replacement : forall state key old new,
  state key = Some new -> old <> new -> release state key old key = Some new.
Proof.
  intros. unfold release. destruct Nat.eq_dec; [rewrite H|congruence].
  destruct Nat.eq_dec; congruence.
Qed.

Theorem release_preserves_other_keys : forall state key token other,
  other <> key -> release state key token other = state other.
Proof. intros. unfold release. destruct Nat.eq_dec; congruence. Qed.

Theorem release_is_idempotent : forall state key token other,
  release (release state key token) key token other = release state key token other.
Proof.
  intros. unfold release. repeat destruct Nat.eq_dec; try congruence.
  all: destruct (state key); repeat destruct Nat.eq_dec; congruence.
Qed.

Theorem failed_admission_releases_new_claim : forall state key token,
  state key = None -> release (install state key token) key token key = None.
Proof. intros. apply exact_release_clears_owner. now apply vacant_claim_has_exact_owner. Qed.

Definition drop_all (state : owners) (tokens : list (nat * nat)) : owners :=
  fold_left (fun current pair => release current (fst pair) (snd pair)) tokens state.

Theorem arbitrary_other_releases_preserve_owner : forall tokens state key,
  Forall (fun pair => fst pair <> key) tokens -> drop_all state tokens key = state key.
Proof.
  induction tokens as [|[k t] rest IH]; intros state key H; [reflexivity|].
  inversion H; subst. unfold drop_all in *. simpl.
  rewrite IH by assumption. apply release_preserves_other_keys. simpl in *. congruence.
Qed.

Theorem arbitrary_stale_releases_preserve_owner : forall tokens state key token,
  state key = Some token ->
  Forall (fun pair => fst pair = key -> snd pair <> token) tokens ->
  drop_all state tokens key = Some token.
Proof.
  induction tokens as [|[k t] rest IH]; intros state key token H F; [exact H|].
  inversion F; subst. unfold drop_all in *. simpl.
  apply IH; [|assumption]. destruct (Nat.eq_dec key k).
  - subst k. apply stale_release_preserves_replacement; [exact H|]. simpl in *. auto.
  - rewrite release_preserves_other_keys by assumption. exact H.
Qed.

Definition release_after_body (state : owners) (key token : nat) (body_alive : bool) : owners :=
  if body_alive then state else release state key token.

Theorem live_body_preserves_its_identity : forall state key token other,
  release_after_body state key token true other = state other.
Proof. reflexivity. Qed.

Theorem dropped_body_permits_exact_release : forall state key token,
  state key = Some token -> release_after_body state key token false key = None.
Proof. intros. now apply exact_release_clears_owner. Qed.

Print Assumptions live_body_preserves_its_identity.
Print Assumptions dropped_body_permits_exact_release.
Print Assumptions vacant_claim_has_exact_owner.
Print Assumptions duplicate_claim_preserves_owner.
Print Assumptions claim_preserves_other_keys.
Print Assumptions exact_release_clears_owner.
Print Assumptions stale_release_preserves_replacement.
Print Assumptions release_preserves_other_keys.
Print Assumptions release_is_idempotent.
Print Assumptions failed_admission_releases_new_claim.
Print Assumptions arbitrary_other_releases_preserve_owner.
Print Assumptions arbitrary_stale_releases_preserve_owner.
