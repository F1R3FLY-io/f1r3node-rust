From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import EPM1.

Import ListNotations.

Module OpaqueCanonicalSegmentImport.

Definition bytes := list nat.

Inductive ground_kind :=
| BoolFalse | BoolTrue | Integer | StringAtom | UriAtom | ByteArray
| DoubleAtom | BigInteger | BigRational | FixedPoint.

Definition ground_tag (kind : ground_kind) : nat :=
  match kind with
  | BoolFalse => 1 | BoolTrue => 2 | Integer => 3 | StringAtom => 4
  | UriAtom => 5 | ByteArray => 6 | DoubleAtom => 7
  | BigInteger => 8 | BigRational => 9 | FixedPoint => 10
  end.

Inductive source_component :=
| StableGround : ground_kind -> bytes -> source_component
| StableList : list bytes -> source_component
| StableTuple : list bytes -> source_component
| StableSetMap : list bytes -> source_component
| StableValueMap : bytes -> source_component
| StablePrivate : bytes -> source_component
| UnstableComponent : source_component.

Definition stable (source : source_component) : bool :=
  match source with UnstableComponent => false | _ => true end.

Definition framed_path (path : bytes) : bytes :=
  EPM1.encode_varint (length path) ++ path.

Definition set_region (paths : list bytes) : bytes :=
  concat (map framed_path paths).

Definition escape_segment (protobuf : bytes) : bytes :=
  [15] ++ EPM1.encode_varint (length protobuf) ++ protobuf.

Definition borrowed_stable_segment (source : source_component) : option bytes :=
  match source with
  | StableGround kind payload => Some ([ground_tag kind] ++ payload)
  | StableList children =>
      Some ([11] ++ EPM1.encode_varint (length children) ++ concat children)
  | StableTuple children =>
      Some ([12] ++ EPM1.encode_varint (length children) ++ concat children)
  | StableSetMap paths =>
      let region := set_region paths in
      Some ([13] ++ EPM1.encode_varint (length region) ++ region)
  | StableValueMap snapshot =>
      Some ([16] ++ EPM1.encode_varint (length snapshot) ++ snapshot)
  | StablePrivate id =>
      Some ([14] ++ EPM1.encode_varint (length id) ++ id)
  | UnstableComponent => None
  end.

Definition borrowed_split_path (source : source_component) : option bytes :=
  match source with
  | StableList children => Some (concat children ++ [0])
  | _ => None
  end.

Record imported_component := Imported {
  imported_score : bytes;
  imported_protobuf : bytes;
  imported_stable : bool;
  imported_segment : option bytes;
  imported_split_path : option bytes
}.

Definition import_component (source : source_component)
    (score protobuf : bytes) : imported_component :=
  Imported score protobuf (stable source)
    (borrowed_stable_segment source) (borrowed_split_path source).

Definition emit_segment (component : imported_component) : option bytes :=
  if imported_stable component then imported_segment component
  else Some (escape_segment (imported_protobuf component)).

Definition emit_path (component : imported_component) : option bytes :=
  if imported_stable component then
    match imported_split_path component with
    | Some path => Some path
    | None => imported_segment component
    end
  else Some (escape_segment (imported_protobuf component)).

Definition authoritative_segment (source : source_component)
    (protobuf : bytes) : bytes :=
  match source with
  | StableGround kind payload => [ground_tag kind] ++ payload
  | StableList children =>
      [11] ++ EPM1.encode_varint (length children) ++ concat children
  | StableTuple children =>
      [12] ++ EPM1.encode_varint (length children) ++ concat children
  | StableSetMap paths =>
      let region := concat (map framed_path paths) in
      [13] ++ EPM1.encode_varint (length region) ++ region
  | StableValueMap snapshot =>
      [16] ++ EPM1.encode_varint (length snapshot) ++ snapshot
  | StablePrivate id =>
      [14] ++ EPM1.encode_varint (length id) ++ id
  | UnstableComponent => escape_segment protobuf
  end.

Definition authoritative_path (source : source_component)
    (protobuf : bytes) : bytes :=
  match source with
  | StableList children => concat children ++ [0]
  | _ => authoritative_segment source protobuf
  end.

Theorem stable_import_has_a_segment :
  forall source score protobuf,
    stable source = true ->
    exists segment,
      imported_segment (import_component source score protobuf) = Some segment.
Proof.
  intros source score protobuf stable_source.
  destruct source; simpl in *; try discriminate; eauto.
Qed.

Theorem imported_score_and_protobuf_are_opaque :
  forall source score protobuf,
    imported_score (import_component source score protobuf) = score /\
    imported_protobuf (import_component source score protobuf) = protobuf.
Proof. intros; split; reflexivity. Qed.

Theorem imported_segment_matches_authority :
  forall source score protobuf,
    emit_segment (import_component source score protobuf) =
      Some (authoritative_segment source protobuf).
Proof. intros; destruct source; reflexivity. Qed.

Theorem imported_path_matches_authority :
  forall source score protobuf,
    emit_path (import_component source score protobuf) =
      Some (authoritative_path source protobuf).
Proof. intros; destruct source; reflexivity. Qed.

Theorem imported_list_retains_both_key_forms :
  forall children score protobuf,
    imported_segment (import_component (StableList children) score protobuf) =
      Some ([11] ++ EPM1.encode_varint (length children) ++ concat children) /\
    imported_split_path (import_component (StableList children) score protobuf) =
      Some (concat children ++ [0]).
Proof. intros; split; reflexivity. Qed.

Theorem imported_set_uses_ordered_source_paths :
  forall first rest,
    set_region (first :: rest) = framed_path first ++ set_region rest.
Proof. reflexivity. Qed.

Theorem imported_value_map_uses_snapshot_bytes :
  forall snapshot score protobuf,
    emit_segment (import_component (StableValueMap snapshot) score protobuf) =
      Some ([16] ++ EPM1.encode_varint (length snapshot) ++ snapshot).
Proof. reflexivity. Qed.

Record planned_root := Root {
  root_component : imported_component;
  root_metadata_default : bool;
  root_protobuf : bytes
}.

Definition root_is_stable (root : planned_root) : bool :=
  root_metadata_default root && imported_stable (root_component root).

Definition emit_root_segment (root : planned_root) : option bytes :=
  if root_is_stable root then imported_segment (root_component root)
  else Some (escape_segment (root_protobuf root)).

Definition emit_root_path (root : planned_root) : option bytes :=
  if root_is_stable root then
    match imported_split_path (root_component root) with
    | Some path => Some path
    | None => imported_segment (root_component root)
    end
  else Some (escape_segment (root_protobuf root)).

Theorem stable_default_rewrap_keeps_exact_path :
  forall source score protobuf before after,
    stable source = true ->
    emit_root_path (Root (import_component source score protobuf) true before) =
      emit_root_path (Root (import_component source score protobuf) true after) /\
    emit_root_path (Root (import_component source score protobuf) true after) =
      Some (authoritative_path source protobuf).
Proof.
  intros source score protobuf before after Hstable.
  unfold emit_root_path, root_is_stable. simpl.
  rewrite Hstable. simpl.
  split; [reflexivity|].
  destruct source; simpl in *; try discriminate; reflexivity.
Qed.

Theorem nondefault_rewrap_escapes_exact_root_protobuf :
  forall source score protobuf root_protobuf,
    emit_root_path (Root (import_component source score protobuf)
      false root_protobuf) = Some (escape_segment root_protobuf) /\
    emit_root_segment (Root (import_component source score protobuf)
      false root_protobuf) = Some (escape_segment root_protobuf).
Proof. intros; split; reflexivity. Qed.

Definition dropped_segment_mutant (component : imported_component) :
    imported_component :=
  Imported (imported_score component) (imported_protobuf component)
    (imported_stable component) None (imported_split_path component).

Example dropped_stable_segment_mutant_cannot_emit_key :
  emit_segment (dropped_segment_mutant
    (import_component (StableGround Integer []) [] [])) = None /\
  emit_segment (import_component (StableGround Integer []) [] []) = Some [3].
Proof. split; reflexivity. Qed.

Print Assumptions stable_import_has_a_segment.
Print Assumptions imported_score_and_protobuf_are_opaque.
Print Assumptions imported_segment_matches_authority.
Print Assumptions imported_path_matches_authority.
Print Assumptions imported_list_retains_both_key_forms.
Print Assumptions imported_set_uses_ordered_source_paths.
Print Assumptions imported_value_map_uses_snapshot_bytes.
Print Assumptions stable_default_rewrap_keeps_exact_path.
Print Assumptions nondefault_rewrap_escapes_exact_root_protobuf.
Print Assumptions dropped_stable_segment_mutant_cannot_emit_key.

End OpaqueCanonicalSegmentImport.
