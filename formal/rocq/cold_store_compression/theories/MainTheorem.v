From Coq Require Import List Arith Bool Lia.
Import ListNotations.

Definition Bytes := list nat.

Inductive Physical :=
| Raw (canonical : Bytes)
| Compressed (canonical : Bytes) (verified : bool)
| Unsupported.

Definition decode (record : Physical) : option Bytes :=
  match record with
  | Raw value => Some value
  | Compressed value true => Some value
  | _ => None
  end.

Definition encode (compress : bool) (value : Bytes) : Physical :=
  if compress then Compressed value true else Raw value.

Theorem canonical_roundtrip : forall compress value,
  decode (encode compress value) = Some value.
Proof. intros [] value; reflexivity. Qed.

Theorem corrupt_frame_refused : forall value,
  decode (Compressed value false) = None.
Proof. reflexivity. Qed.

Theorem unknown_format_refused : decode Unsupported = None.
Proof. reflexivity. Qed.

Theorem successful_compressed_read_verified : forall value verified result,
  decode (Compressed value verified) = Some result ->
  verified = true /\ result = value.
Proof. intros value [] result H; simpl in H; inversion H; auto. Qed.

Definition read (record : option Physical) : option Bytes :=
  match record with Some p => decode p | None => None end.

Definition Store := nat -> option Physical.
Definition logical (store : Store) : nat -> option Bytes := fun key => read (store key).
Definition write (store : Store) (key : nat) (value : Bytes) (compress : bool) : Store :=
  fun queried => if Nat.eqb queried key then Some (encode compress value) else store queried.

Theorem physical_choice_preserves_state : forall store key value left right queried,
  logical (write store key value left) queried = logical (write store key value right) queried.
Proof.
  intros. unfold logical, write.
  destruct (Nat.eqb queried key); simpl; auto.
  rewrite !canonical_roundtrip. reflexivity.
Qed.

Theorem write_preserves_other_keys : forall store key value compress queried,
  queried <> key -> write store key value compress queried = store queried.
Proof.
  intros. unfold write. apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

Theorem canonical_export_independent : forall values left right,
  map (fun value => decode (encode left value)) values =
  map (fun value => decode (encode right value)) values.
Proof.
  induction values; intros; simpl; auto.
  rewrite !canonical_roundtrip. f_equal. apply IHvalues.
Qed.

Theorem canonical_observer_independent : forall values left right (observer : list (option Bytes) -> nat),
  observer (map (fun value => decode (encode left value)) values) =
  observer (map (fun value => decode (encode right value)) values).
Proof. intros. rewrite (canonical_export_independent values left right). reflexivity. Qed.

Definition bytes_eq_dec := list_eq_dec Nat.eq_dec.
Definition physical_eq_dec : forall a b : Physical, {a = b} + {a <> b}.
Proof. decide equality; try apply Bool.bool_dec; apply bytes_eq_dec. Defined.
Definition row_eq_dec : forall a b : option Physical, {a = b} + {a <> b}.
Proof. decide equality; apply physical_eq_dec. Defined.
Definition value_eq_dec : forall a b : option Bytes, {a = b} + {a <> b}.
Proof. decide equality; apply bytes_eq_dec. Defined.

Definition compare_delete (current snapshot : option Physical) (expected : Bytes) : option Physical :=
  if value_eq_dec (read snapshot) (Some expected) then
    if row_eq_dec current snapshot then None else current
  else current.

Theorem changed_row_preserved : forall current snapshot expected,
  current <> snapshot -> compare_delete current snapshot expected = current.
Proof.
  intros. unfold compare_delete.
  destruct (value_eq_dec (read snapshot) (Some expected)); auto.
  destruct (row_eq_dec current snapshot); congruence.
Qed.

Theorem different_value_preserved : forall current snapshot expected,
  read current <> Some expected -> compare_delete current snapshot expected = current.
Proof.
  intros. unfold compare_delete.
  destruct (value_eq_dec (read snapshot) (Some expected)); auto.
  destruct (row_eq_dec current snapshot); congruence.
Qed.

Theorem deletion_requires_matching_value : forall current snapshot expected,
  current <> None -> compare_delete current snapshot expected = None ->
  read current = Some expected.
Proof.
  intros current snapshot expected Hpresent Hdelete. unfold compare_delete in Hdelete.
  destruct (value_eq_dec (read snapshot) (Some expected)); try congruence.
  destruct (row_eq_dec current snapshot); congruence.
Qed.

Theorem stable_matching_row_deleted : forall current expected,
  read current = Some expected -> compare_delete current current expected = None.
Proof.
  intros. unfold compare_delete.
  destruct (value_eq_dec (read current) (Some expected)); try congruence.
  destruct (row_eq_dec current current); congruence.
Qed.

Record Budget := { active : nat; reserved : nat }.
Definition bounded (max_active max_bytes : nat) (state : Budget) : Prop :=
  active state <= max_active /\ reserved state <= max_bytes.
Definition acquire (max_active max_bytes bytes : nat) (state : Budget) : option Budget :=
  if (active state <? max_active) && (reserved state + bytes <=? max_bytes)
  then Some {| active := S (active state); reserved := reserved state + bytes |}
  else None.
Definition release (bytes : nat) (state : Budget) : Budget :=
  {| active := pred (active state); reserved := reserved state - bytes |}.

Theorem acquire_preserves_bounds : forall max_active max_bytes bytes state next,
  acquire max_active max_bytes bytes state = Some next -> bounded max_active max_bytes next.
Proof.
  intros. unfold acquire in H.
  destruct ((active state <? max_active) && (reserved state + bytes <=? max_bytes)) eqn:E; try discriminate.
  apply andb_true_iff in E as [EA EB].
  apply Nat.ltb_lt in EA. apply Nat.leb_le in EB.
  inversion H; subst. unfold bounded; simpl. lia.
Qed.

Theorem release_preserves_bounds : forall max_active max_bytes bytes state,
  bounded max_active max_bytes state -> bounded max_active max_bytes (release bytes state).
Proof. intros; unfold bounded, release in *; simpl in *; lia. Qed.

Theorem acquire_release_exact : forall max_active max_bytes bytes state next,
  acquire max_active max_bytes bytes state = Some next -> release bytes next = state.
Proof.
  intros. unfold acquire in H.
  destruct ((active state <? max_active) && (reserved state + bytes <=? max_bytes)); try discriminate.
  inversion H; subst. destruct state. unfold release; simpl. f_equal. lia.
Qed.

Inductive BudgetStep (max_active max_bytes : nat) : Budget -> Budget -> Prop :=
| Reserved : forall state next bytes, acquire max_active max_bytes bytes state = Some next ->
    BudgetStep max_active max_bytes state next
| Released : forall state bytes, BudgetStep max_active max_bytes state (release bytes state).

Inductive BudgetTrace (max_active max_bytes : nat) : Budget -> Budget -> Prop :=
| EmptyTrace : forall state, BudgetTrace max_active max_bytes state state
| NextTrace : forall initial current next,
    BudgetTrace max_active max_bytes initial current ->
    BudgetStep max_active max_bytes current next ->
    BudgetTrace max_active max_bytes initial next.

Theorem arbitrary_trace_preserves_bounds : forall max_active max_bytes initial final,
  BudgetTrace max_active max_bytes initial final ->
  bounded max_active max_bytes initial -> bounded max_active max_bytes final.
Proof.
  intros max_active max_bytes initial final Htrace. induction Htrace; intros Hinitial; auto.
  inversion H; subst.
  - eapply acquire_preserves_bounds; eauto.
  - apply release_preserves_bounds. auto.
Qed.
