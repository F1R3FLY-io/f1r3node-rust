From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA EPathMap.

Import ListNotations.
Set Implicit Arguments.

(**
  A structural model of the canonical EPM1 envelope used by the Rust codec.

  The ACTree03 arena is deliberately represented as bytes: PathMap owns that
  format and its executable parser.  This model proves the surrounding EPM1
  contract instead of treating the whole suffix as one opaque payload:

    magic | version | mode
      | varint(arena length) | ACTree03 arena
      | varint(value count)
      | repeated(varint(value length) | generated-PDA protobuf value)

  Bytes are natural numbers here.  [decode_varint] additionally enforces the
  physical byte range and rejects non-canonical redundant continuation groups.
*)

Definition byte := nat.
Definition bytes := list byte.

Definition epm_magic : bytes := [69; 80; 77; 49].
Definition epm_version : byte := 1.

Definition mode_code (storage_mode : mode) : byte :=
  match storage_mode with Neutral => 0 | SetMode => 1 | MapMode => 2 end.

Fixpoint encode_varint_fuel (fuel value : nat) : bytes :=
  match fuel with
  | 0 => []
  | S remaining =>
      if value <? 128 then [value]
      else (128 + value mod 128) :: encode_varint_fuel remaining (value / 128)
  end.

Definition encode_varint (value : nat) : bytes :=
  encode_varint_fuel (S value) value.

Fixpoint decode_varint (input : bytes) : option (nat * bytes) :=
  match input with
  | [] => None
  | current :: rest =>
      if current <? 128 then Some (current, rest)
      else if current <? 256 then
        match decode_varint rest with
        | Some (quotient, suffix) =>
            if quotient =? 0 then None
            else Some ((current - 128) + 128 * quotient, suffix)
        | None => None
        end
      else None
  end.

Lemma decode_varint_cons :
  forall current rest,
    decode_varint (current :: rest) =
      if current <? 128 then Some (current, rest)
      else if current <? 256 then
        match decode_varint rest with
        | Some (quotient, suffix) =>
            if quotient =? 0 then None
            else Some ((current - 128) + 128 * quotient, suffix)
        | None => None
        end
      else None.
Proof. reflexivity. Qed.

Lemma decode_varint_nil : decode_varint [] = None.
Proof. reflexivity. Qed.

Global Opaque decode_varint.

Lemma decode_encode_varint_fuel :
  forall fuel value suffix,
    value < fuel ->
    decode_varint (encode_varint_fuel fuel value ++ suffix) =
      Some (value, suffix).
Proof.
  induction fuel as [|fuel induction]; intros value suffix bounded; [lia|].
  unfold encode_varint_fuel at 1.
  destruct (value <? 128) eqn:small.
  - change (decode_varint (value :: suffix) = Some (value, suffix)).
    rewrite decode_varint_cons, small. reflexivity.
  - apply Nat.ltb_ge in small.
    assert (remainder_bound : value mod 128 < 128).
    { apply Nat.mod_upper_bound. lia. }
    assert (head_not_small : (128 + value mod 128 <? 128) = false).
    { apply Nat.ltb_ge. lia. }
    assert (head_is_byte : (128 + value mod 128 <? 256) = true).
    { apply Nat.ltb_lt. lia. }
    assert (value_positive : 0 < value) by lia.
    assert (quotient_bounded : value / 128 < fuel).
    {
      pose proof (Nat.div_lt value 128 value_positive ltac:(lia)).
      lia.
    }
    assert (quotient_positive : 0 < value / 128).
    { apply Nat.div_str_pos. lia. }
    assert (quotient_nonzero : (value / 128 =? 0) = false).
    { apply Nat.eqb_neq. lia. }
    change
      (decode_varint
        ((128 + value mod 128) ::
          (encode_varint_fuel fuel (value / 128) ++ suffix)) =
       Some (value, suffix)).
    rewrite decode_varint_cons, head_not_small, head_is_byte.
    rewrite induction by exact quotient_bounded.
    rewrite quotient_nonzero.
    pose proof (Nat.div_mod value 128 ltac:(lia)) as decomposition.
    assert (head_remainder : 128 + value mod 128 - 128 = value mod 128) by lia.
    rewrite head_remainder.
    assert (reconstructed : value mod 128 + 128 * (value / 128) = value) by lia.
    now rewrite reconstructed.
Qed.

Theorem canonical_varint_round_trip :
  forall value suffix,
    decode_varint (encode_varint value ++ suffix) = Some (value, suffix).
Proof.
  intros value suffix. unfold encode_varint.
  apply decode_encode_varint_fuel. lia.
Qed.

Theorem canonical_varint_rejects_redundant_continuation :
  decode_varint [128; 0] = None.
Proof.
  rewrite !decode_varint_cons. reflexivity.
Qed.

Theorem canonical_varint_rejects_truncated_continuation :
  decode_varint [128] = None.
Proof.
  rewrite decode_varint_cons. reflexivity.
Qed.

Theorem canonical_varint_rejects_out_of_range_byte :
  decode_varint [256] = None.
Proof.
  rewrite decode_varint_cons. reflexivity.
Qed.

Lemma firstn_length_app :
  forall (prefix suffix : bytes),
    firstn (length prefix) (prefix ++ suffix) = prefix.
Proof.
  induction prefix; intros suffix; simpl; auto.
  now rewrite IHprefix.
Qed.

Lemma skipn_length_app :
  forall (prefix suffix : bytes),
    skipn (length prefix) (prefix ++ suffix) = suffix.
Proof.
  induction prefix; intros suffix; simpl; auto.
Qed.

Definition encode_frame (payload : bytes) : bytes :=
  encode_varint (length payload) ++ payload.

Definition decode_frame (input : bytes) : option (bytes * bytes) :=
  match decode_varint input with
  | Some (payload_length, rest) =>
      if payload_length <=? length rest then
        Some (firstn payload_length rest, skipn payload_length rest)
      else None
  | None => None
  end.

Theorem canonical_frame_round_trip :
  forall payload suffix,
    decode_frame (encode_frame payload ++ suffix) = Some (payload, suffix).
Proof.
  intros payload suffix. unfold decode_frame, encode_frame.
  assert (association :
    (encode_varint (length payload) ++ payload) ++ suffix =
    encode_varint (length payload) ++ (payload ++ suffix)).
  { symmetry. apply app_assoc. }
  rewrite association, canonical_varint_round_trip.
  assert (enough : (length payload <=? length (payload ++ suffix)) = true).
  { apply Nat.leb_le. rewrite length_app. lia. }
  rewrite enough.
  rewrite firstn_length_app, skipn_length_app. reflexivity.
Qed.

Theorem length_frame_rejects_truncated_payload :
  decode_frame [2; 42] = None.
Proof.
  unfold decode_frame. rewrite decode_varint_cons. reflexivity.
Qed.

Fixpoint encode_frames (payloads : list bytes) : bytes :=
  match payloads with
  | [] => []
  | payload :: rest => encode_frame payload ++ encode_frames rest
  end.

Fixpoint decode_frames (count : nat) (input : bytes)
  : option (list bytes * bytes) :=
  match count with
  | 0 => Some ([], input)
  | S remaining =>
      match decode_frame input with
      | Some (payload, rest) =>
          match decode_frames remaining rest with
          | Some (payloads, suffix) => Some (payload :: payloads, suffix)
          | None => None
          end
      | None => None
      end
  end.

Theorem canonical_frames_round_trip :
  forall payloads suffix,
    decode_frames (length payloads) (encode_frames payloads ++ suffix) =
      Some (payloads, suffix).
Proof.
  induction payloads as [|payload payloads induction]; intros suffix; simpl.
  - reflexivity.
  - assert (association :
      (encode_frame payload ++ encode_frames payloads) ++ suffix =
      encode_frame payload ++ (encode_frames payloads ++ suffix)).
    { symmetry. apply app_assoc. }
    rewrite association, canonical_frame_round_trip, induction. reflexivity.
Qed.

Record epm1_snapshot : Type := Epm1Snapshot {
  snapshot_mode : mode;
  topology_arena : bytes;
  value_bodies : list bytes
}.

Definition mode_well_formedb (snapshot : epm1_snapshot) : bool :=
  match snapshot_mode snapshot with
  | Neutral =>
      (length (topology_arena snapshot) =? 0) &&
      (length (value_bodies snapshot) =? 0)
  | SetMode =>
      (0 <? length (topology_arena snapshot)) &&
      (length (value_bodies snapshot) =? 0)
  | MapMode =>
      (0 <? length (topology_arena snapshot)) &&
      (0 <? length (value_bodies snapshot))
  end.

Definition encode_epm1 (snapshot : epm1_snapshot) : bytes :=
  epm_magic ++ [epm_version; mode_code (snapshot_mode snapshot)] ++
  encode_frame (topology_arena snapshot) ++
  encode_varint (length (value_bodies snapshot)) ++
  encode_frames (value_bodies snapshot).

Definition decode_epm1_prefix (encoded : bytes)
  : option (epm1_snapshot * bytes) :=
  match encoded with
  | 69 :: 80 :: 77 :: 49 :: 1 :: mode_byte :: payload =>
      match mode_byte with
      | 0 =>
          match decode_frame payload with
          | Some (arena, after_arena) =>
              match decode_varint after_arena with
              | Some (value_count, framed_values) =>
                  match decode_frames value_count framed_values with
                  | Some (values, suffix) =>
                      Some (Epm1Snapshot Neutral arena values, suffix)
                  | None => None
                  end
              | None => None
              end
          | None => None
          end
      | 1 =>
          match decode_frame payload with
          | Some (arena, after_arena) =>
              match decode_varint after_arena with
              | Some (value_count, framed_values) =>
                  match decode_frames value_count framed_values with
                  | Some (values, suffix) =>
                      Some (Epm1Snapshot SetMode arena values, suffix)
                  | None => None
                  end
              | None => None
              end
          | None => None
          end
      | 2 =>
          match decode_frame payload with
          | Some (arena, after_arena) =>
              match decode_varint after_arena with
              | Some (value_count, framed_values) =>
                  match decode_frames value_count framed_values with
                  | Some (values, suffix) =>
                      Some (Epm1Snapshot MapMode arena values, suffix)
                  | None => None
                  end
              | None => None
              end
          | None => None
          end
      | _ => None
      end
  | _ => None
  end.

Definition decode_epm1 (encoded : bytes) : option epm1_snapshot :=
  match decode_epm1_prefix encoded with
  | Some (snapshot, []) =>
      if mode_well_formedb snapshot then Some snapshot else None
  | _ => None
  end.

Theorem epm1_structural_prefix_round_trip :
  forall snapshot suffix,
    decode_epm1_prefix (encode_epm1 snapshot ++ suffix) =
      Some (snapshot, suffix).
Proof.
  intros [storage_mode arena values] suffix. destruct storage_mode; simpl.
  all: assert (arena_association :
    (encode_frame arena ++
      (encode_varint (length values) ++ encode_frames values)) ++ suffix =
    encode_frame arena ++
      ((encode_varint (length values) ++ encode_frames values) ++ suffix)).
  all: try (symmetry; apply app_assoc).
  all: rewrite arena_association, canonical_frame_round_trip.
  all: assert (value_association :
    (encode_varint (length values) ++ encode_frames values) ++ suffix =
    encode_varint (length values) ++ (encode_frames values ++ suffix)).
  all: try (symmetry; apply app_assoc).
  all: rewrite value_association, canonical_varint_round_trip.
  all: rewrite canonical_frames_round_trip.
  all: reflexivity.
Qed.

Theorem epm1_framed_decode_encode_identity :
  forall snapshot,
    mode_well_formedb snapshot = true ->
    decode_epm1 (encode_epm1 snapshot) = Some snapshot.
Proof.
  intros snapshot valid. unfold decode_epm1.
  pose proof (epm1_structural_prefix_round_trip snapshot []) as round_trip.
  rewrite app_nil_r in round_trip. rewrite round_trip, valid. reflexivity.
Qed.

Theorem epm1_preserves_topology_and_ordered_value_table :
  forall snapshot decoded,
    mode_well_formedb snapshot = true ->
    decode_epm1 (encode_epm1 snapshot) = Some decoded ->
    topology_arena decoded = topology_arena snapshot /\
    value_bodies decoded = value_bodies snapshot.
Proof.
  intros snapshot decoded valid decoded_equal.
  rewrite (epm1_framed_decode_encode_identity snapshot valid) in decoded_equal.
  inversion decoded_equal. auto.
Qed.

Theorem epm1_rejects_trailing_bytes :
  forall snapshot trailing_byte,
    decode_epm1 (encode_epm1 snapshot ++ [trailing_byte]) = None.
Proof.
  intros snapshot trailing_byte. unfold decode_epm1.
  rewrite epm1_structural_prefix_round_trip. reflexivity.
Qed.

Theorem epm1_rejects_truncated_header :
  decode_epm1 [] = None /\
  decode_epm1 [69] = None /\
  decode_epm1 [69; 80] = None /\
  decode_epm1 [69; 80; 77] = None /\
  decode_epm1 [69; 80; 77; 49] = None /\
  decode_epm1 [69; 80; 77; 49; 1] = None.
Proof. repeat split; reflexivity. Qed.

Theorem epm1_rejects_invalid_mode :
  forall payload, decode_epm1 (epm_magic ++ [epm_version; 3] ++ payload) = None.
Proof. reflexivity. Qed.

Definition canonical_ordinals (values : list bytes) : list nat :=
  seq 0 (length values).

Theorem canonical_value_ordinals_are_bijective :
  forall values,
    NoDup (canonical_ordinals values) /\
    forall ordinal,
      In ordinal (canonical_ordinals values) <-> ordinal < length values.
Proof.
  intro values. split.
  - apply seq_NoDup.
  - intro ordinal. unfold canonical_ordinals. rewrite in_seq. lia.
Qed.

Theorem canonical_ordinal_indexes_its_value :
  forall values ordinal,
    ordinal < length values ->
    nth_error (canonical_ordinals values) ordinal = Some ordinal /\
    exists value, nth_error values ordinal = Some value.
Proof.
  intros values ordinal bounded. split.
  - unfold canonical_ordinals. rewrite nth_error_seq.
    assert (in_range : (ordinal <? length values) = true).
    { apply Nat.ltb_lt. exact bounded. }
    rewrite in_range. reflexivity.
  - apply nth_error_Some in bounded.
    destruct (nth_error values ordinal) eqn:found; [eauto|congruence].
Qed.

Theorem epm1_round_trip_preserves_ordinal_value_association :
  forall snapshot decoded ordinal value,
    mode_well_formedb snapshot = true ->
    decode_epm1 (encode_epm1 snapshot) = Some decoded ->
    nth_error (value_bodies snapshot) ordinal = Some value ->
    nth_error (value_bodies decoded) ordinal = Some value.
Proof.
  intros snapshot decoded ordinal value valid decoded_equal association.
  rewrite (epm1_framed_decode_encode_identity snapshot valid) in decoded_equal.
  injection decoded_equal as same. subst decoded. exact association.
Qed.

Section TopologyOrdinalAssociation.

Variable extract_value_ordinals : bytes -> option (list nat).

Definition topology_value_association (snapshot : epm1_snapshot) : Prop :=
  match snapshot_mode snapshot with
  | MapMode =>
      extract_value_ordinals (topology_arena snapshot) =
        Some (canonical_ordinals (value_bodies snapshot))
  | Neutral | SetMode =>
      extract_value_ordinals (topology_arena snapshot) = Some []
  end.

Theorem epm1_round_trip_preserves_topology_value_association :
  forall snapshot decoded,
    mode_well_formedb snapshot = true ->
    topology_value_association snapshot ->
    decode_epm1 (encode_epm1 snapshot) = Some decoded ->
    topology_value_association decoded.
Proof.
  intros snapshot decoded valid association decoded_equal.
  rewrite (epm1_framed_decode_encode_identity snapshot valid) in decoded_equal.
  injection decoded_equal as same. now subst decoded.
Qed.

Theorem associated_map_ordinals_are_in_range_and_unique :
  forall snapshot,
    snapshot_mode snapshot = MapMode ->
    topology_value_association snapshot ->
    exists ordinals,
      extract_value_ordinals (topology_arena snapshot) = Some ordinals /\
      NoDup ordinals /\
      forall ordinal, In ordinal ordinals <-> ordinal < length (value_bodies snapshot).
Proof.
  intros snapshot map_mode association. unfold topology_value_association in association.
  rewrite map_mode in association.
  exists (canonical_ordinals (value_bodies snapshot)).
  split; [exact association|].
  apply canonical_value_ordinals_are_bijective.
Qed.

End TopologyOrdinalAssociation.

Section GeneratedValueCodec.

Context {Label : Type}.
Variable encode_node : Label -> list bytes -> bytes.

Definition recursive_value_body (value : @tree Label) : bytes :=
  @fold_tree Label bytes encode_node value.

Definition pda_value_body (value : @tree Label) : bytes :=
  match @run Label bytes encode_node (@compile_tree Label value) [] with
  | Some [body] => body
  | _ => []
  end.

Theorem generated_pda_value_body_equals_recursive_body :
  forall value, pda_value_body value = recursive_value_body value.
Proof.
  intro value. unfold pda_value_body, recursive_value_body.
  rewrite (@pda_fold_equivalent_to_recursive_fold Label bytes encode_node value).
  reflexivity.
Qed.

Definition recursive_value_table (values : list (@tree Label)) : list bytes :=
  map recursive_value_body values.

Definition pda_value_table (values : list (@tree Label)) : list bytes :=
  map pda_value_body values.

Theorem generated_pda_value_table_equals_recursive_table :
  forall values, pda_value_table values = recursive_value_table values.
Proof.
  intro values. unfold pda_value_table, recursive_value_table.
  apply map_ext. intro value.
  apply generated_pda_value_body_equals_recursive_body.
Qed.

Theorem epm1_generated_pda_values_equal_recursive_encoding :
  forall storage_mode arena values,
    encode_epm1 (Epm1Snapshot storage_mode arena (pda_value_table values)) =
    encode_epm1 (Epm1Snapshot storage_mode arena (recursive_value_table values)).
Proof.
  intros storage_mode arena values.
  rewrite generated_pda_value_table_equals_recursive_table. reflexivity.
Qed.

End GeneratedValueCodec.
