From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat Lia.
Import ListNotations.

Inductive leaf_kind := Data | Continuations | Joins.

Definition lookup_prefix (kind : leaf_kind) : nat :=
  match kind with Data => 0 | Continuations => 1 | Joins => 2 end.

Definition stored_tag (kind : leaf_kind) : nat :=
  match kind with Joins => 0 | Data => 1 | Continuations => 2 end.

Theorem leaf_tags_are_not_lookup_prefixes : forall kind,
  lookup_prefix kind <> stored_tag kind.
Proof. destruct kind; simpl; lia. Qed.

Definition checked_end (offset width total : nat) : option nat :=
  if (offset <=? total) && (width <=? total - offset)
  then Some (offset + width) else None.

Theorem checked_span_bounds : forall offset width total ending,
  checked_end offset width total = Some ending ->
  ending = offset + width /\ offset <= ending /\ ending <= total.
Proof.
  intros offset width total ending.
  unfold checked_end. destruct (_ && _) eqn:bounds; [|discriminate].
  apply andb_true_iff in bounds. destruct bounds as [a b].
  apply Nat.leb_le in a. apply Nat.leb_le in b. intros result.
  inversion result. subst. lia.
Qed.

Theorem checked_span_machine_bound : forall offset width total ending maximum,
  total <= maximum -> checked_end offset width total = Some ending ->
  offset + width <= maximum.
Proof.
  intros. pose proof (checked_span_bounds _ _ _ _ H0). lia.
Qed.

Theorem child_step_strict : forall remaining prefix,
  prefix < remaining -> remaining - (1 + prefix) < remaining.
Proof. intros. lia. Qed.

Fixpoint node_visits (prefixes : list nat) (remaining : nat) : nat :=
  match remaining, prefixes with
  | 0, _ => 0
  | S _, [] => 1
  | S _, prefix :: tail =>
      if prefix <? remaining
      then 1 + node_visits tail (remaining - (1 + prefix)) else 1
  end.

Theorem node_visits_key_bound : forall prefixes remaining,
  node_visits prefixes remaining <= remaining.
Proof.
  induction prefixes as [|prefix tail IH]; intros [|remaining]; simpl; try lia.
  destruct (prefix <? S remaining) eqn:fits; [|lia].
  apply Nat.ltb_lt in fits.
  specialize (IH (S remaining - (1 + prefix))). simpl in IH. lia.
Qed.

Corollary hashed_projection_visits : forall prefixes,
  node_visits prefixes 33 <= 33.
Proof. intros. apply node_visits_key_bound. Qed.

Fixpoint rows_size (lengths : list nat) : nat :=
  match lengths with [] => 0 | n :: tail => 8 + n + rows_size tail end.

Theorem row_count_bound : forall lengths,
  8 * length lengths <= rows_size lengths.
Proof. induction lengths; simpl; lia. Qed.

Theorem row_spans_disjoint : forall before current after,
  rows_size before + 8 + current <= rows_size (before ++ current :: after).
Proof. induction before; intros; simpl; try lia. specialize (IHbefore current after). lia. Qed.

Theorem row_header_strict_progress : forall offset width total ending,
  checked_end offset (8 + width) total = Some ending -> offset < ending.
Proof. intros. pose proof (checked_span_bounds _ _ _ _ H). lia. Qed.

Definition authorized_consumer (reserved framed hashed typed : bool) : bool :=
  reserved && framed && hashed && typed.

Theorem consumer_requires_every_check : forall reserved framed hashed typed,
  authorized_consumer reserved framed hashed typed = true ->
  reserved = true /\ framed = true /\ hashed = true /\ typed = true.
Proof. intros [] [] [] []; simpl; intuition discriminate. Qed.

Theorem rejected_reservation_has_no_consumer : forall framed hashed typed,
  authorized_consumer false framed hashed typed = false.
Proof. reflexivity. Qed.

Definition leaf_hash_slice {A} (length_bytes payload trailer : list A) : list A :=
  firstn (length length_bytes + length payload) (length_bytes ++ payload ++ trailer).

Theorem leaf_hash_excludes_outer_trailer : forall A (length_bytes payload trailer : list A),
  leaf_hash_slice length_bytes payload trailer = length_bytes ++ payload.
Proof.
  intros. unfold leaf_hash_slice.
  replace (length length_bytes + length payload) with (length (length_bytes ++ payload))
    by (rewrite length_app; reflexivity).
  rewrite app_assoc, firstn_app, firstn_all, Nat.sub_diag. simpl.
  apply app_nil_r.
Qed.

Theorem inner_trailer_remains_hashed : forall A (length_bytes rows inner_trailer outer_trailer : list A),
  leaf_hash_slice length_bytes (rows ++ inner_trailer) outer_trailer =
    length_bytes ++ rows ++ inner_trailer.
Proof. intros. apply leaf_hash_excludes_outer_trailer. Qed.

Definition fallback_allowed {A} (raw : option A) : bool :=
  match raw with None => true | Some _ => false end.

Theorem present_raw_never_falls_back : forall A (raw : A),
  fallback_allowed (Some raw) = false.
Proof. reflexivity. Qed.

Theorem maximum_radix_frame : 256 * (2 + 127 + 32) = 41216.
Proof. reflexivity. Qed.
