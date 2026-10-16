From Coq Require Import List Arith Bool Lia.
Import ListNotations.

Definition rows_complete (held requested : list nat) (metadata body : nat -> bool) : bool :=
  forallb metadata held && forallb (fun h => if existsb (Nat.eqb h) held then body h else true) requested.

Theorem complete_metadata : forall held requested metadata body,
  rows_complete held requested metadata body = true ->
  forall h, In h held -> metadata h = true.
Proof.
  intros held requested metadata body H h Hin. apply andb_true_iff in H as [H _].
  apply forallb_forall with (x := h) in H; assumption.
Qed.

Theorem complete_requested_bodies : forall held requested metadata body,
  rows_complete held requested metadata body = true ->
  forall h, In h held -> In h requested -> body h = true.
Proof.
  intros held requested metadata body H h Hheld Hreq.
  apply andb_true_iff in H as [_ H].
  apply forallb_forall with (x := h) in H; [|exact Hreq].
  assert (existsb (Nat.eqb h) held = true) as He.
  { apply existsb_exists. exists h. split; [exact Hheld|apply Nat.eqb_refl]. }
  rewrite He in H. exact H.
Qed.

Fixpoint fields_encode (fields : list (list nat)) : list nat :=
  match fields with
  | [] => []
  | field :: rest => length field :: field ++ fields_encode rest
  end.

Fixpoint fields_decode (count : nat) (wire : list nat)
  : option (list (list nat) * list nat) :=
  match count with
  | 0 => Some ([], wire)
  | S n => match wire with
           | [] => None
           | size :: tail =>
               if size <=? length tail then
                 match fields_decode n (skipn size tail) with
                 | Some (fields, rest) => Some (firstn size tail :: fields, rest)
                 | None => None
                 end
               else None
           end
  end.

Theorem fields_roundtrip_suffix : forall fields suffix,
  fields_decode (length fields) (fields_encode fields ++ suffix) = Some (fields, suffix).
Proof.
  induction fields as [|field rest IH]; intros suffix; simpl; [reflexivity|].
  rewrite <- app_assoc.
  assert (length field <=? length (field ++ (fields_encode rest ++ suffix)) = true) as Hlen.
  { apply Nat.leb_le. rewrite app_length. lia. }
  rewrite Hlen.
  rewrite skipn_app, firstn_app, skipn_all, firstn_all, Nat.sub_diag.
  simpl. rewrite app_nil_r. rewrite IH. reflexivity.
Qed.

Definition canonical_record (fields : list (list nat)) : list nat :=
  length fields :: fields_encode fields.

Theorem canonical_record_injective : forall left right,
  canonical_record left = canonical_record right -> left = right.
Proof.
  intros left right H. unfold canonical_record in H.
  injection H as Hcount Hfields.
  pose proof (fields_roundtrip_suffix left []) as Hl.
  pose proof (fields_roundtrip_suffix right []) as Hr.
  rewrite !app_nil_r in Hl, Hr. rewrite Hcount, Hfields in Hl.
  rewrite Hr in Hl. inversion Hl. reflexivity.
Qed.

Definition heap := nat -> list nat.
Definition put (h : heap) (address : nat) (value : list nat) : heap :=
  fun other => if other =? address then value else h other.

Fixpoint populate (h : heap) (rows : list (nat * list nat)) : heap :=
  match rows with
  | [] => h
  | (address, value) :: rest => populate (put h address value) rest
  end.

Theorem populate_preserves_other_stores : forall rows h address,
  ~ In address (map fst rows) -> populate h rows address = h address.
Proof.
  induction rows as [|[key value] rest IH]; intros h address H; simpl; [reflexivity|].
  rewrite IH.
  - unfold put. destruct (address =? key) eqn:He; [|reflexivity].
    apply Nat.eqb_eq in He. subst. exfalso. apply H. simpl. auto.
  - intros Hin. apply H. simpl. auto.
Qed.

Definition addresses_fresh (next : nat) (rows : list (nat * list nat)) : Prop :=
  Forall (fun row => next <= fst row) rows.

Theorem fresh_scratch_preserves_production : forall rows h next address,
  addresses_fresh next rows -> address < next -> populate h rows address = h address.
Proof.
  intros rows h next address Hfresh Hlt.
  apply populate_preserves_other_stores. intros Hin.
  apply in_map_iff in Hin as [[key value] [Heq Hin]]. simpl in Heq. subst key.
  unfold addresses_fresh in Hfresh. rewrite Forall_forall in Hfresh.
  specialize (Hfresh _ Hin). simpl in Hfresh. lia.
Qed.

Theorem scratch_writes_preserve_sibling : forall h first second value,
  first <> second -> put h first value second = h second.
Proof.
  intros h first second value H. unfold put.
  destruct (second =? first) eqn:He; [|reflexivity].
  apply Nat.eqb_eq in He. congruence.
Qed.
