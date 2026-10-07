From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure StateImportCodec StateImportCursor.
Import ListNotations.

Definition ImportWireHistory := list nat -> option (list nat).
Definition ImportHistoryReader := list nat -> option (list ImportWireEdge).

Definition import_checked_wire_history (Hash : list nat -> list nat)
    (store : ImportWireHistory) (key : list nat) : option (list ImportWireEdge) :=
  if import_cursor_word_validb key then
    match store key with
    | None => None
    | Some bytes => if list_eq_dec Nat.eq_dec (Hash bytes) key
      then import_parse_wire_node bytes else None
    end
  else None.

Theorem import_checked_wire_history_has_exact_witness : forall Hash store key edges,
  import_checked_wire_history Hash store key = Some edges ->
  length key = 32 /\ Forall import_wire_byte key /\
  exists bytes, store key = Some bytes /\ Hash bytes = key /\
    import_parse_wire_node bytes = Some edges.
Proof.
  intros Hash store key edges Read. unfold import_checked_wire_history in Read.
  destruct (import_cursor_word_validb key) eqn:Valid; try discriminate.
  apply import_cursor_word_validity_characterization in Valid as [Width Domain].
  destruct (store key) as [bytes|] eqn:Found; try discriminate.
  destruct (list_eq_dec Nat.eq_dec (Hash bytes) key) as [Hashed|]; try discriminate.
  split; auto. split; auto. exists bytes. auto.
Qed.

Theorem import_checked_wire_history_rejects_missing_row : forall Hash store key,
  store key = None -> import_checked_wire_history Hash store key = None.
Proof.
  intros Hash store key Missing. unfold import_checked_wire_history.
  destruct (import_cursor_word_validb key); auto. now rewrite Missing.
Qed.

Definition import_project_wire_history (store : ImportWireHistory)
    (key : nat) : option (list ImportRadixEdge) :=
  match store (import_nat_to_key 32 key) with
  | None => None
  | Some bytes => match import_parse_wire_node bytes with
    | None => None
    | Some edges => Some (map import_codec_edge_to_closure edges)
    end
  end.

Definition import_project_wire_store (store : ImportWireHistory)
    (raw legacy : nat -> option ImportLeaf) : ImportStore :=
  {| import_history := import_project_wire_history store;
     import_cold_raw := raw; import_cold_legacy := legacy |}.

Definition import_project_wire_hash (Hash : list nat -> list nat)
    (edges : list ImportRadixEdge) : nat :=
  import_key_to_nat (Hash (import_closure_records_to_bytes edges)).

Theorem import_wire_history_projection_preserves_stored_record : forall store key bytes edges,
  import_cursor_word_validb key = true ->
  store key = Some bytes -> import_parse_wire_node bytes = Some edges ->
  import_project_wire_history store (import_key_to_nat key) =
    Some (map import_codec_edge_to_closure edges).
Proof.
  intros store key bytes edges Valid Stored Parsed.
  apply import_cursor_word_validity_characterization in Valid as [Width Bytes].
  pose proof (import_key_conversion_is_reversible _ Bytes) as Reversible.
  rewrite Width in Reversible.
  unfold import_project_wire_history. now rewrite Reversible, Stored, Parsed.
Qed.

Theorem import_checked_wire_and_closure_history_reads_correspond :
  forall Hash PayloadHash store raw legacy key context,
  (forall bytes, import_cursor_word_validb (Hash bytes) = true) ->
  import_cursor_word_validb key = true ->
  import_checked_read (import_project_wire_hash Hash) PayloadHash
    (import_project_wire_store store raw legacy) (ImportHistoryRef (import_key_to_nat key) context) =
  match import_checked_wire_history Hash store key with
  | None => None
  | Some edges => import_contextual_children context (map import_codec_edge_to_closure edges)
  end.
Proof.
  intros Hash PayloadHash store raw legacy key context HashValid KeyValid.
  pose proof (import_cursor_word_validity_characterization key) as KeyCharacterization.
  apply KeyCharacterization in KeyValid as [Width Bytes].
  assert (import_cursor_word_validb key = true) as Valid by (apply KeyCharacterization; auto).
  pose proof (import_key_conversion_is_reversible _ Bytes) as Reversible.
  rewrite Width in Reversible.
  cbn [import_checked_read import_project_wire_store import_history].
  unfold import_project_wire_history, import_checked_wire_history.
  rewrite Reversible, Valid. destruct (store key) as [bytes|]; [|reflexivity].
  destruct (import_parse_wire_node bytes) as [edges|] eqn:Parsed.
  - unfold import_project_wire_hash.
    rewrite (import_closure_mapping_retains_original_input _ _ Parsed).
    destruct (list_eq_dec Nat.eq_dec (Hash bytes) key) as [Equal|Different].
    + rewrite Equal, Nat.eqb_refl. reflexivity.
    + assert (import_key_to_nat (Hash bytes) =? import_key_to_nat key = false) as NotEqual.
      { apply Nat.eqb_neq. intros Same. apply Different.
        pose proof (HashValid bytes) as Hashed.
        apply import_cursor_word_validity_characterization in Hashed as [HashWidth HashBytes].
        eapply import_32_byte_storage_keys_do_not_alias; eauto. }
      now rewrite NotEqual.
  - destruct (list_eq_dec Nat.eq_dec (Hash bytes) key); reflexivity.
Qed.

Fixpoint import_strip_exact_prefix (prefix input : list nat) : option (list nat) :=
  match prefix, input with
  | [], _ => Some input
  | expected :: prefix_tail, actual :: input_tail =>
      if expected =? actual then import_strip_exact_prefix prefix_tail input_tail else None
  | _, _ => None
  end.

Theorem import_prefix_strip_is_exact : forall prefix input suffix,
  import_strip_exact_prefix prefix input = Some suffix <-> input = prefix ++ suffix.
Proof.
  induction prefix as [|expected tail IH]; intros input suffix.
  - simpl. split; congruence.
  - destruct input as [|actual rest]; simpl; [split; discriminate|].
    destruct (expected =? actual) eqn:Equal.
    + apply Nat.eqb_eq in Equal. subst actual. rewrite IH. split; congruence.
    + apply Nat.eqb_neq in Equal. split; congruence.
Qed.

Fixpoint import_resolve_history_path (fuel : nat) (read : ImportHistoryReader)
    (key path : list nat) : option (list nat) :=
  match fuel with
  | 0 => None
  | S remaining =>
      match read key with
      | None => None
      | Some edges =>
          match path with
          | [] => Some key
          | slot :: tail =>
              match import_wire_slot_lookup slot edges with
              | None => None
              | Some edge =>
                  if import_wire_header edge <? 128 then None else
                    match import_strip_exact_prefix (import_wire_prefix edge) tail with
                    | None => None
                    | Some suffix =>
                        import_resolve_history_path remaining read (import_wire_hash edge) suffix
                    end
              end
          end
      end
  end.

Inductive import_history_path (read : ImportHistoryReader)
    : list nat -> list nat -> list nat -> Prop :=
| import_history_path_here : forall key edges,
    read key = Some edges -> import_history_path read key [] key
| import_history_path_child : forall key edges edge suffix target,
    read key = Some edges ->
    import_wire_slot_lookup (import_wire_slot edge) edges = Some edge ->
    128 <= import_wire_header edge ->
    import_history_path read (import_wire_hash edge) suffix target ->
    import_history_path read key
      (import_wire_slot edge :: import_wire_prefix edge ++ suffix) target.

Theorem import_path_resolution_has_exact_history_path : forall fuel read key path target,
  import_resolve_history_path fuel read key path = Some target ->
  import_history_path read key path target.
Proof.
  induction fuel as [|fuel IH]; intros read key path target Resolved; [discriminate|].
  cbn [import_resolve_history_path] in Resolved.
  destruct (read key) as [edges|] eqn:Read; try discriminate.
  destruct path as [|slot tail].
  - inversion Resolved; subst target. now apply import_history_path_here with (edges := edges).
  - destruct (import_wire_slot_lookup slot edges) as [edge|] eqn:Lookup; try discriminate.
    destruct (import_wire_header edge <? 128) eqn:Kind; try discriminate.
    destruct (import_strip_exact_prefix (import_wire_prefix edge) tail) as [suffix|] eqn:Prefix;
      try discriminate.
    apply import_prefix_strip_is_exact in Prefix. subst tail.
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Lookup) as [_ Slot].
    rewrite <- Slot. eapply import_history_path_child; eauto.
    + now rewrite Slot.
    + now apply Nat.ltb_ge.
Qed.

Theorem import_path_resolution_is_complete : forall read key path target,
  import_history_path read key path target -> forall fuel,
  length path < fuel -> import_resolve_history_path fuel read key path = Some target.
Proof.
  intros read key path target Path.
  induction Path as [key edges Read|
    key edges edge suffix target Read Lookup Kind Child IH]; intros fuel Bound;
    destruct fuel as [|fuel]; [simpl in Bound; lia| |simpl in Bound; lia|].
  - cbn [import_resolve_history_path]. now rewrite Read.
  - cbn [import_resolve_history_path]. rewrite Read, Lookup.
    assert (import_wire_header edge <? 128 = false) as Header by now apply Nat.ltb_ge.
    rewrite Header.
    assert (import_strip_exact_prefix (import_wire_prefix edge)
      (import_wire_prefix edge ++ suffix) = Some suffix) as Prefix.
    { apply import_prefix_strip_is_exact. reflexivity. }
    rewrite Prefix. apply IH. cbn [length] in Bound. rewrite length_app in Bound. lia.
Qed.

Theorem import_path_resolution_characterization : forall read key path target,
  import_resolve_history_path (S (length path)) read key path = Some target <->
  import_history_path read key path target.
Proof.
  split; [apply import_path_resolution_has_exact_history_path|].
  intros Path. apply import_path_resolution_is_complete; auto.
Qed.

Theorem import_resolved_history_path_has_readable_target : forall read key path target,
  import_history_path read key path target -> exists edges, read target = Some edges.
Proof. intros read key path target Path. induction Path; eauto. Qed.

Theorem import_empty_history_path_resolves_only_start_key : forall read key target,
  import_history_path read key [] target -> target = key.
Proof. intros read key target Path. inversion Path. reflexivity. Qed.

Theorem import_history_path_is_deterministic : forall read key path first second,
  import_history_path read key path first -> import_history_path read key path second -> first = second.
Proof.
  intros read key path first second First Second.
  apply import_path_resolution_characterization in First.
  apply import_path_resolution_characterization in Second. congruence.
Qed.

Definition import_history_reader_extension (first second : ImportHistoryReader) : Prop :=
  forall key edges, first key = Some edges -> second key = Some edges.

Theorem import_compatible_wire_writes_extend_checked_reader : forall Hash first second,
  (forall key bytes, first key = Some bytes -> second key = Some bytes) ->
  import_history_reader_extension
    (import_checked_wire_history Hash first) (import_checked_wire_history Hash second).
Proof.
  intros Hash first second Extension key edges Read.
  unfold import_checked_wire_history in *.
  destruct (import_cursor_word_validb key); try discriminate.
  destruct (first key) as [bytes|] eqn:Found; try discriminate.
  now rewrite (Extension key bytes Found).
Qed.

Theorem import_history_reader_extension_is_transitive : forall first middle last,
  import_history_reader_extension first middle -> import_history_reader_extension middle last ->
  import_history_reader_extension first last.
Proof. intros first middle last First Second key edges Read. apply Second. now apply First. Qed.

Theorem import_history_path_survives_compatible_writes : forall first last key path target,
  import_history_reader_extension first last ->
  import_history_path first key path target -> import_history_path last key path target.
Proof.
  intros first last key path target Extension Path.
  induction Path as [key edges Read|key edges edge suffix target Read Lookup Kind Child IH].
  - apply import_history_path_here with (edges := edges). exact (Extension key edges Read).
  - apply import_history_path_child with (edges := edges).
    + exact (Extension key edges Read).
    + exact Lookup.
    + exact Kind.
    + exact IH.
Qed.

Inductive import_interleaved_history_path :
    ImportHistoryReader -> ImportHistoryReader -> list nat -> list nat -> list nat -> Prop :=
| import_interleaved_path_here : forall read key edges,
    read key = Some edges -> import_interleaved_history_path read read key [] key
| import_interleaved_path_child : forall first middle last key edges edge suffix target,
    first key = Some edges ->
    import_wire_slot_lookup (import_wire_slot edge) edges = Some edge ->
    128 <= import_wire_header edge ->
    import_history_reader_extension first middle ->
    import_interleaved_history_path middle last (import_wire_hash edge) suffix target ->
    import_interleaved_history_path first last key
      (import_wire_slot edge :: import_wire_prefix edge ++ suffix) target.

Theorem import_interleaved_path_preserves_reader_bindings : forall first last key path target,
  import_interleaved_history_path first last key path target ->
  import_history_reader_extension first last.
Proof.
  intros first last key path target Run. induction Run.
  - intros query stored Read. exact Read.
  - eapply import_history_reader_extension_is_transitive; eauto.
Qed.

Theorem import_interleaved_path_has_final_view_witness : forall first last key path target,
  import_interleaved_history_path first last key path target ->
  import_history_path last key path target.
Proof.
  intros first last key path target Run.
  induction Run as [read key edges Read|
    first middle last key edges edge suffix target Read Lookup Kind Extension Run IH].
  - now apply import_history_path_here with (edges := edges).
  - pose proof (import_interleaved_path_preserves_reader_bindings _ _ _ _ _ Run) as Later.
    apply import_history_path_child with (edges := edges).
    + exact (Later key edges (Extension key edges Read)).
    + exact Lookup.
    + exact Kind.
    + exact IH.
Qed.

Theorem import_concurrent_path_resolution_preserves_selected_target :
  forall initial final key path expected actual,
  import_history_path initial key path expected ->
  import_interleaved_history_path initial final key path actual -> actual = expected.
Proof.
  intros initial final key path expected actual Selected Run.
  pose proof (import_interleaved_path_preserves_reader_bindings _ _ _ _ _ Run) as Extension.
  pose proof (import_history_path_survives_compatible_writes _ _ _ _ _ Extension Selected) as Preserved.
  pose proof (import_interleaved_path_has_final_view_witness _ _ _ _ _ Run) as Actual.
  eapply import_history_path_is_deterministic; eauto.
Qed.

Theorem import_resolved_path_survives_writes_after_final_read :
  forall initial final later key path target,
  import_interleaved_history_path initial final key path target ->
  import_history_reader_extension final later -> import_history_path later key path target.
Proof.
  intros initial final later key path target Run Extension.
  eapply import_history_path_survives_compatible_writes; [exact Extension|].
  now apply import_interleaved_path_has_final_view_witness in Run.
Qed.

Definition import_cursor_prefix (cursor : ImportCursor) : list nat :=
  match cursor with ImportCursorStart _ => [] | ImportCursorResume _ prefix _ => prefix end.

Definition import_cursor_emits_anchor (cursor : ImportCursor) : bool :=
  match cursor with ImportCursorStart _ => true | ImportCursorResume _ _ _ => false end.

Theorem import_empty_resume_and_start_have_distinct_export_behavior : forall root,
  import_cursor_prefix (ImportCursorStart root) =
    import_cursor_prefix (ImportCursorResume root [] root) /\
  import_cursor_emits_anchor (ImportCursorStart root) = true /\
  import_cursor_emits_anchor (ImportCursorResume root [] root) = false.
Proof. intros root. repeat split; reflexivity. Qed.

Definition import_cursor_has_valid_history_path (read : ImportHistoryReader)
    (cursor : ImportCursor) : bool :=
  match import_resolve_history_path (S (length (import_cursor_prefix cursor))) read
    (import_cursor_root cursor) (import_cursor_prefix cursor) with
  | None => false
  | Some found => if list_eq_dec Nat.eq_dec found (import_cursor_carrier cursor) then true else false
  end.

Theorem import_cursor_path_validation_characterization : forall read cursor,
  import_cursor_has_valid_history_path read cursor = true <->
  import_history_path read (import_cursor_root cursor) (import_cursor_prefix cursor)
    (import_cursor_carrier cursor).
Proof.
  intros read cursor. unfold import_cursor_has_valid_history_path. split.
  - destruct (import_resolve_history_path _ _ _ _) as [found|] eqn:Resolved; try discriminate.
    destruct (list_eq_dec Nat.eq_dec found (import_cursor_carrier cursor)) as [Same|];
      try discriminate.
    intros _. subst found. now apply import_path_resolution_characterization.
  - intros Path. apply import_path_resolution_characterization in Path. rewrite Path.
    destruct (list_eq_dec Nat.eq_dec (import_cursor_carrier cursor) (import_cursor_carrier cursor));
      congruence.
Qed.

Theorem import_valid_empty_resume_binds_carrier_to_root : forall read root carrier,
  import_cursor_has_valid_history_path read (ImportCursorResume root [] carrier) = true ->
  carrier = root.
Proof.
  intros read root carrier Valid. apply import_cursor_path_validation_characterization in Valid.
  now apply import_empty_history_path_resolves_only_start_key in Valid.
Qed.

Theorem import_valid_cursor_target_has_authenticated_bytes : forall Hash store cursor,
  import_cursor_has_valid_history_path (import_checked_wire_history Hash store) cursor = true ->
  exists bytes edges, store (import_cursor_carrier cursor) = Some bytes /\
    Hash bytes = import_cursor_carrier cursor /\ import_parse_wire_node bytes = Some edges.
Proof.
  intros Hash store cursor Valid. apply import_cursor_path_validation_characterization in Valid.
  apply import_resolved_history_path_has_readable_target in Valid as [edges Read].
  apply import_checked_wire_history_has_exact_witness in Read as [_ [_ [bytes Witness]]].
  exists bytes, edges. exact Witness.
Qed.

Example import_missing_singleton_is_not_valid_exhaustion :
  import_cursor_has_valid_history_path (fun _ => None) (ImportCursorStart (repeat 0 32)) = false.
Proof. reflexivity. Qed.

Example import_empty_prefix_with_different_carrier_is_rejected :
  import_cursor_has_valid_history_path (fun _ => Some [])
    (ImportCursorResume (repeat 0 32) [] (repeat 1 32)) = false.
Proof. reflexivity. Qed.

Example import_prefix_inside_compressed_edge_is_rejected :
  import_resolve_history_path 3 (fun _ => Some [import_boundary_wire_edge 1 130])
    (repeat 0 32) [1; 0] = None.
Proof. reflexivity. Qed.

Example import_leaf_is_not_a_cursor_history_node :
  import_resolve_history_path 2 (fun _ => Some [import_zero_prefix_wire_edge 1])
    (repeat 0 32) [1] = None.
Proof. reflexivity. Qed.

Example import_shared_singleton_does_not_require_an_original_root :
  import_cursor_has_valid_history_path (fun _ => Some [])
    (ImportCursorStart (repeat 1 32)) = true.
Proof. reflexivity. Qed.
