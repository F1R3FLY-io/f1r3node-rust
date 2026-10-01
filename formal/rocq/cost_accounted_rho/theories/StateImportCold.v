From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure StateImportStorage StateImportCodec.
Import ListNotations.

Definition import_read_fixed_word (width : nat) (input : list nat) : option (nat * list nat) :=
  if width <=? length input then
    Some (import_key_to_nat (firstn width input), skipn width input)
  else None.

Theorem import_fixed_word_has_exact_input_partition : forall width input count rest,
  import_read_fixed_word width input = Some (count, rest) ->
  exists header, length header = width /\ count = import_key_to_nat header /\ input = header ++ rest.
Proof.
  intros width input count rest Read. unfold import_read_fixed_word in Read.
  destruct (width <=? length input) eqn:Enough; [|discriminate].
  apply Nat.leb_le in Enough. inversion Read; subst count rest.
  exists (firstn width input). split.
  - rewrite length_firstn. lia.
  - split; auto. symmetry. apply firstn_skipn.
Qed.

Theorem import_fixed_word_reads_every_exact_header : forall width header rest,
  length header = width ->
  import_read_fixed_word width (header ++ rest) = Some (import_key_to_nat header, rest).
Proof.
  intros width header rest Width. unfold import_read_fixed_word.
  assert (width <=? length (header ++ rest) = true) as Enough by
    (apply Nat.leb_le; rewrite length_app, Width; lia).
  rewrite Enough, <- Width, firstn_app, firstn_all, Nat.sub_diag.
  rewrite skipn_app, skipn_all, Nat.sub_diag. cbn [firstn skipn app]. now rewrite app_nil_r.
Qed.

Definition import_read_byte_vector (input : list nat) : option (list nat * list nat) :=
  match import_read_fixed_word 8 input with
  | None => None
  | Some (count, body) => if count <=? length body
      then Some (firstn count body, skipn count body) else None
  end.

Definition import_byte_vector_frame (input payload rest : list nat) : Prop :=
  exists header, length header = 8 /\ import_key_to_nat header = length payload /\
    input = header ++ payload ++ rest.

Theorem import_byte_vector_parser_has_exact_framing : forall input payload rest,
  import_read_byte_vector input = Some (payload, rest) <-> import_byte_vector_frame input payload rest.
Proof.
  intros input payload rest. split.
  - intros Read. unfold import_read_byte_vector in Read.
    destruct (import_read_fixed_word 8 input) as [[count body]|] eqn:Word; [|discriminate].
    destruct (count <=? length body) eqn:Enough; [|discriminate].
    apply Nat.leb_le in Enough. inversion Read; subst payload rest.
    destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Word)
      as [header [Width [Count Partition]]].
    exists header. split; auto. split.
    + rewrite length_firstn. lia.
    + rewrite firstn_skipn. exact Partition.
  - intros [header [Width [Count Partition]]]. subst input.
    unfold import_read_byte_vector. rewrite import_fixed_word_reads_every_exact_header by assumption.
    rewrite Count. assert (length payload <=? length (payload ++ rest) = true) as Enough by
      (apply Nat.leb_le; rewrite length_app; lia).
    rewrite Enough, firstn_app, firstn_all, Nat.sub_diag, skipn_app, skipn_all, Nat.sub_diag.
    cbn [firstn skipn app]. now rewrite app_nil_r.
Qed.

Theorem import_byte_vector_read_consumes_header_and_payload : forall input payload rest,
  import_read_byte_vector input = Some (payload, rest) ->
  length input = 8 + length payload + length rest.
Proof.
  intros input payload rest Read. apply import_byte_vector_parser_has_exact_framing in Read
    as [header [Width [_ Partition]]]. subst input. rewrite !length_app, Width. lia.
Qed.

Fixpoint import_read_byte_vectors (count : nat) (input : list nat)
    : option (list (list nat) * list nat) :=
  match count with
  | 0 => Some ([], input)
  | S remaining => match import_read_byte_vector input with
    | None => None
    | Some (item, tail) => match import_read_byte_vectors remaining tail with
      | None => None
      | Some (items, rest) => Some (item :: items, rest)
      end
    end
  end.

Inductive import_byte_vectors_frame : nat -> list nat -> list (list nat) -> list nat -> Prop :=
| import_byte_vectors_frame_end : forall rest, import_byte_vectors_frame 0 rest [] rest
| import_byte_vectors_frame_next : forall count input item tail items rest,
    import_byte_vector_frame input item tail -> import_byte_vectors_frame count tail items rest ->
    import_byte_vectors_frame (S count) input (item :: items) rest.

Theorem import_byte_vectors_parser_has_exact_framing : forall count input items rest,
  import_read_byte_vectors count input = Some (items, rest) <->
    import_byte_vectors_frame count input items rest.
Proof.
  induction count as [|count IH]; intros input items rest; split; intros Read.
  - inversion Read; subst. constructor.
  - now inversion Read.
  - cbn [import_read_byte_vectors] in Read.
    destruct (import_read_byte_vector input) as [[item tail]|] eqn:Head; [|discriminate].
    destruct (import_read_byte_vectors count tail) as [[remaining ending]|] eqn:Tail; [|discriminate].
    inversion Read; subst items rest. eapply import_byte_vectors_frame_next with (tail := tail).
    + apply import_byte_vector_parser_has_exact_framing. exact Head.
    + apply IH. exact Tail.
  - inversion Read; subst. cbn [import_read_byte_vectors].
    match goal with
    | Head : import_byte_vector_frame input ?item ?tail,
      Tail : import_byte_vectors_frame count ?tail ?items rest |- _ =>
        apply import_byte_vector_parser_has_exact_framing in Head;
        apply IH in Tail; now rewrite Head, Tail
    end.
Qed.

Theorem import_byte_vectors_read_has_exact_size : forall count input items rest,
  import_read_byte_vectors count input = Some (items, rest) ->
  length items = count /\ length input = 8 * count + length (concat items) + length rest.
Proof.
  intros count input items rest Read. apply import_byte_vectors_parser_has_exact_framing in Read.
  induction Read as [rest|count input item tail items rest Head Tail [Count Size]].
  - simpl. auto.
  - apply import_byte_vector_parser_has_exact_framing in Head.
    apply import_byte_vector_read_consumes_header_and_payload in Head.
    cbn [length concat]. rewrite length_app. split; lia.
Qed.

Definition import_read_collection (input : list nat) : option (list (list nat) * list nat) :=
  match import_read_fixed_word 8 input with
  | None => None
  | Some (count, body) =>
      if 8 * count <=? length body then import_read_byte_vectors count body else None
  end.

Theorem import_collection_bound_preserves_the_decoder_language : forall input items rest,
  import_read_collection input = Some (items, rest) <->
  exists count body, import_read_fixed_word 8 input = Some (count, body) /\
    import_read_byte_vectors count body = Some (items, rest).
Proof.
  intros input items rest. unfold import_read_collection. split.
  - destruct (import_read_fixed_word 8 input) as [[count body]|] eqn:Word; [|discriminate].
    destruct (8 * count <=? length body); intros Read; [eauto|discriminate].
  - intros [count [body [Word Read]]]. rewrite Word.
    pose proof (import_byte_vectors_read_has_exact_size _ _ _ _ Read) as [_ Size].
    assert (8 * count <=? length body = true) as Enough by (apply Nat.leb_le; lia).
    now rewrite Enough.
Qed.

Theorem import_collection_success_bounds_count_by_input_size : forall input items rest,
  import_read_collection input = Some (items, rest) ->
  8 + 8 * length items + length (concat items) + length rest = length input.
Proof.
  intros input items rest Read. apply import_collection_bound_preserves_the_decoder_language in Read
    as [count [body [Word Read]]].
  destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Word)
    as [header [Width [_ Partition]]].
  pose proof (import_byte_vectors_read_has_exact_size _ _ _ _ Read) as [Count Size].
  subst input. rewrite length_app, Width. lia.
Qed.

Lemma import_byte_vector_frame_accepts_trailing_bytes : forall input item rest extra,
  import_byte_vector_frame input item rest ->
  import_byte_vector_frame (input ++ extra) item (rest ++ extra).
Proof.
  intros input item rest extra [header [Width [Count Partition]]].
  exists header. split; auto. split; auto. subst input. now rewrite !app_assoc.
Qed.

Theorem import_byte_vectors_preserve_trailing_bytes : forall count input items rest extra,
  import_read_byte_vectors count input = Some (items, rest) ->
  import_read_byte_vectors count (input ++ extra) = Some (items, rest ++ extra).
Proof.
  intros count input items rest extra Read. apply import_byte_vectors_parser_has_exact_framing in Read.
  apply import_byte_vectors_parser_has_exact_framing.
  induction Read as [rest|count input item tail items rest Head Tail IH].
  - apply import_byte_vectors_frame_end.
  - eapply import_byte_vectors_frame_next with (tail := tail ++ extra); auto.
    now apply import_byte_vector_frame_accepts_trailing_bytes.
Qed.

Theorem import_collection_preserves_existing_trailing_byte_acceptance : forall input items rest extra,
  import_read_collection input = Some (items, rest) ->
  import_read_collection (input ++ extra) = Some (items, rest ++ extra).
Proof.
  intros input items rest extra Read. apply import_collection_bound_preserves_the_decoder_language in Read
    as [count [body [Word Read]]].
  apply import_collection_bound_preserves_the_decoder_language.
  destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Word)
    as [header [Width [Count Partition]]].
  exists count, (body ++ extra). split.
  - rewrite Partition, <- app_assoc, import_fixed_word_reads_every_exact_header by assumption.
    now rewrite <- Count.
  - now apply import_byte_vectors_preserve_trailing_bytes.
Qed.

Definition import_cold_kind_from_tag (tag : nat) : option ImportLeafKind :=
  match tag with 0 => Some ImportJoins | 1 => Some ImportData | 2 => Some ImportContinuations | _ => None end.

Definition import_read_persisted_leaf (input : list nat) : option ImportLeaf :=
  match import_read_fixed_word 4 input with
  | None => None
  | Some (tag, body) => match import_cold_kind_from_tag tag, import_read_byte_vector body with
    | Some kind, Some (payload, _) => Some {| import_leaf_kind := kind; import_leaf_payload := payload |}
    | _, _ => None
    end
  end.

Definition import_leaf_hash_input (leaf : ImportLeaf) : list nat :=
  import_nat_to_key 8 (length (import_leaf_payload leaf)) ++ import_leaf_payload leaf.

Theorem import_persisted_leaf_parser_has_exact_components : forall input leaf,
  import_read_persisted_leaf input = Some leaf <->
  exists tag body tail,
    import_read_fixed_word 4 input = Some (tag, body) /\
    import_cold_kind_from_tag tag = Some (import_leaf_kind leaf) /\
    import_read_byte_vector body = Some (import_leaf_payload leaf, tail).
Proof.
  intros input leaf. split.
  - intros Parsed. unfold import_read_persisted_leaf in Parsed.
    destruct (import_read_fixed_word 4 input) as [[tag body]|] eqn:Word; [|discriminate].
    destruct (import_cold_kind_from_tag tag) as [kind|] eqn:Kind; [|discriminate].
    destruct (import_read_byte_vector body) as [[payload tail]|] eqn:Payload; [|discriminate].
    inversion Parsed; subst leaf. exists tag, body, tail. auto.
  - intros [tag [body [tail [Word [Kind Payload]]]]].
    unfold import_read_persisted_leaf. rewrite Word, Kind, Payload. destruct leaf. reflexivity.
Qed.

Theorem import_persisted_leaf_preserves_existing_trailing_bytes : forall input leaf extra,
  import_read_persisted_leaf input = Some leaf ->
  import_read_persisted_leaf (input ++ extra) = Some leaf.
Proof.
  intros input leaf extra Parsed.
  apply import_persisted_leaf_parser_has_exact_components in Parsed
    as [tag [body [tail [Word [Kind Payload]]]]].
  apply import_persisted_leaf_parser_has_exact_components.
  destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Word)
    as [header [Width [Tag Partition]]].
  exists tag, (body ++ extra), (tail ++ extra). split.
  - rewrite Partition, <- app_assoc, import_fixed_word_reads_every_exact_header by assumption.
    now rewrite <- Tag.
  - split; auto. apply import_byte_vector_parser_has_exact_framing.
    apply import_byte_vector_frame_accepts_trailing_bytes.
    now apply import_byte_vector_parser_has_exact_framing.
Qed.

Theorem import_existing_leaf_hash_does_not_authenticate_kind : forall first second payload,
  import_leaf_hash_input {| import_leaf_kind := first; import_leaf_payload := payload |} =
    import_leaf_hash_input {| import_leaf_kind := second; import_leaf_payload := payload |}.
Proof. reflexivity. Qed.

Theorem import_fixed_width_bytes_bound_the_decoded_value : forall bytes,
  Forall import_wire_byte bytes -> import_key_to_nat bytes < 256 ^ length bytes.
Proof.
  intros bytes Bytes. induction Bytes as [|byte tail Byte Tail IH].
  - simpl. lia.
  - change (byte + 256 * import_key_to_nat tail < 256 ^ S (length tail)).
    rewrite Nat.pow_succ_r by lia. unfold import_wire_byte in Byte. lia.
Qed.

Theorem import_fixed_word_preserves_byte_domain_and_bounds : forall width input count rest,
  Forall import_wire_byte input ->
  import_read_fixed_word width input = Some (count, rest) ->
  count < 256 ^ width /\ Forall import_wire_byte rest.
Proof.
  intros width input count rest Bytes Parsed.
  destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Parsed)
    as [header [Width [Count Partition]]].
  subst input. apply Forall_app in Bytes as [Header Rest]. split; auto.
  subst count. rewrite <- Width. now apply import_fixed_width_bytes_bound_the_decoded_value.
Qed.

Theorem import_byte_vector_preserves_byte_domain_and_length_bound : forall input payload rest,
  Forall import_wire_byte input ->
  import_read_byte_vector input = Some (payload, rest) ->
  length payload < 256 ^ 8 /\
  Forall import_wire_byte payload /\ Forall import_wire_byte rest.
Proof.
  intros input payload rest Bytes Parsed.
  apply import_byte_vector_parser_has_exact_framing in Parsed
    as [header [Width [Count Partition]]].
  subst input. apply Forall_app in Bytes as [Header Body].
  apply Forall_app in Body as [Payload Rest]. split; auto.
  pose proof (import_fixed_width_bytes_bound_the_decoded_value _ Header) as Bound.
  now rewrite Width, Count in Bound.
Qed.

Theorem import_parsed_leaf_hash_uses_the_exact_payload_frame : forall input leaf,
  Forall import_wire_byte input -> import_read_persisted_leaf input = Some leaf ->
  exists tag_header payload_header tail,
    length tag_header = 4 /\ length payload_header = 8 /\
    input = tag_header ++ payload_header ++ import_leaf_payload leaf ++ tail /\
    import_leaf_hash_input leaf = payload_header ++ import_leaf_payload leaf /\
    length (import_leaf_payload leaf) < 256 ^ 8 /\
    Forall import_wire_byte (import_leaf_payload leaf).
Proof.
  intros input leaf Bytes Parsed.
  apply import_persisted_leaf_parser_has_exact_components in Parsed
    as [tag [body [tail [Word [Kind Payload]]]]].
  destruct (import_fixed_word_has_exact_input_partition _ _ _ _ Word)
    as [tag_header [TagWidth [Tag Input]]].
  assert (Forall import_wire_byte body) as BodyBytes.
  { rewrite Input in Bytes. now apply Forall_app in Bytes as [_ BodyBytes]. }
  pose proof (import_byte_vector_preserves_byte_domain_and_length_bound _ _ _ BodyBytes Payload)
    as [Bound [PayloadBytes _]].
  apply import_byte_vector_parser_has_exact_framing in Payload
    as [payload_header [PayloadWidth [Count Body]]].
  assert (Forall import_wire_byte payload_header) as HeaderBytes.
  { rewrite Body in BodyBytes. now apply Forall_app in BodyBytes as [HeaderBytes _]. }
  exists tag_header, payload_header, tail. split; auto. split; auto. split.
  - now rewrite Input, Body.
  - split; auto. unfold import_leaf_hash_input.
    pose proof (import_key_conversion_is_reversible _ HeaderBytes) as Encoded.
    rewrite PayloadWidth, Count in Encoded. now rewrite Encoded.
Qed.

Section TypedColdConsumer.

Variable Value : ImportLeafKind -> Type.
Variable decode_item : forall kind, list nat -> option (Value kind).

Fixpoint import_decode_cold_items (kind : ImportLeafKind) (items : list (list nat))
    : option (list (Value kind)) :=
  match items with
  | [] => Some []
  | item :: tail => match decode_item kind item, import_decode_cold_items kind tail with
    | Some value, Some values => Some (value :: values)
    | _, _ => None
    end
  end.

Definition import_consume_cold_leaf (expected : ImportLeafKind) (leaf : ImportLeaf)
    : option (list (Value expected)) :=
  if import_leaf_kind_eq_dec (import_leaf_kind leaf) expected then
    match import_read_collection (import_leaf_payload leaf) with
    | None => None
    | Some (items, _) => import_decode_cold_items expected items
    end
  else None.

Definition import_validate_cold_leaf (expected : ImportLeafKind) (leaf : ImportLeaf) : bool :=
  match import_consume_cold_leaf expected leaf with Some _ => true | None => false end.

Theorem import_cold_validation_iff_successful_consumption : forall expected leaf,
  import_validate_cold_leaf expected leaf = true <->
    exists values, import_consume_cold_leaf expected leaf = Some values.
Proof.
  intros expected leaf. unfold import_validate_cold_leaf.
  destruct (import_consume_cold_leaf expected leaf); split; intros Result; eauto; try discriminate.
  destruct Result as [values Impossible]. discriminate.
Qed.

Theorem import_item_decoding_preserves_exact_correspondence : forall kind items values,
  import_decode_cold_items kind items = Some values <->
    Forall2 (fun item value => decode_item kind item = Some value) items values.
Proof.
  intros kind items. induction items as [|item tail IH]; intros values; split; intros Decoded.
  - inversion Decoded; subst values. constructor.
  - now inversion Decoded.
  - cbn [import_decode_cold_items] in Decoded.
    destruct (decode_item kind item) as [value|] eqn:Head; [|discriminate].
    destruct (import_decode_cold_items kind tail) as [rest|] eqn:Tail; [|discriminate].
    inversion Decoded; subst values. constructor; auto. now apply IH.
  - inversion Decoded; subst. cbn [import_decode_cold_items].
    match goal with
    | Head : decode_item kind item = Some ?value,
      Tail : Forall2 _ tail ?rest |- _ => apply IH in Tail; now rewrite Head, Tail
    end.
Qed.

Theorem import_successful_consumer_has_every_typed_item : forall expected leaf values,
  import_consume_cold_leaf expected leaf = Some values ->
  import_leaf_kind leaf = expected /\
  exists items rest, import_read_collection (import_leaf_payload leaf) = Some (items, rest) /\
    Forall2 (fun item value => decode_item expected item = Some value) items values.
Proof.
  intros expected leaf values Consumed. unfold import_consume_cold_leaf in Consumed.
  destruct (import_leaf_kind_eq_dec (import_leaf_kind leaf) expected) as [Kind|Wrong]; [|discriminate].
  destruct (import_read_collection (import_leaf_payload leaf)) as [[items rest]|] eqn:Read;
    [|discriminate]. split; auto. exists items, rest. split; auto.
  now apply import_item_decoding_preserves_exact_correspondence.
Qed.

Definition import_authenticate_cold_bytes (Hash : list nat -> nat)
    (expected : ImportLeafKind) (key : nat) (input : list nat) : option ImportLeaf :=
  match import_read_persisted_leaf input with
  | None => None
  | Some leaf => if (Hash (import_leaf_hash_input leaf) =? key) && import_validate_cold_leaf expected leaf
      then Some leaf else None
  end.

Theorem import_authenticated_cold_bytes_have_hash_kind_and_typed_values :
  forall Hash expected key input leaf,
  import_authenticate_cold_bytes Hash expected key input = Some leaf ->
  import_read_persisted_leaf input = Some leaf /\
  Hash (import_leaf_hash_input leaf) = key /\ import_leaf_kind leaf = expected /\
  exists values items rest,
    import_consume_cold_leaf expected leaf = Some values /\
    import_read_collection (import_leaf_payload leaf) = Some (items, rest) /\
    Forall2 (fun item value => decode_item expected item = Some value) items values.
Proof.
  intros Hash expected key input leaf Authenticated.
  unfold import_authenticate_cold_bytes in Authenticated.
  destruct (import_read_persisted_leaf input) as [parsed|] eqn:Parsed; [|discriminate].
  destruct ((Hash (import_leaf_hash_input parsed) =? key) && import_validate_cold_leaf expected parsed)
    eqn:Accepted; [|discriminate]. inversion Authenticated; subst parsed.
  apply andb_true_iff in Accepted as [HashEq Valid]. apply Nat.eqb_eq in HashEq.
  apply import_cold_validation_iff_successful_consumption in Valid as [values Consumed].
  destruct (import_successful_consumer_has_every_typed_item _ _ _ Consumed)
    as [Kind [items [rest [Collection Items]]]].
  repeat split; auto. exists values, items, rest. auto.
Qed.

Theorem import_cold_authentication_preserves_envelope_trailing_bytes :
  forall Hash expected key input leaf extra,
  import_authenticate_cold_bytes Hash expected key input = Some leaf ->
  import_authenticate_cold_bytes Hash expected key (input ++ extra) = Some leaf.
Proof.
  intros Hash expected key input leaf extra Authenticated.
  pose proof (import_authenticated_cold_bytes_have_hash_kind_and_typed_values _ _ _ _ _ Authenticated)
    as [Parsed [Hashed [_ [values [items [rest [Consumed _]]]]]]].
  unfold import_authenticate_cold_bytes.
  rewrite (import_persisted_leaf_preserves_existing_trailing_bytes _ _ _ Parsed), Hashed, Nat.eqb_refl.
  unfold import_validate_cold_leaf. now rewrite Consumed.
Qed.

Theorem import_wrong_kind_cannot_be_consumed : forall expected leaf,
  import_leaf_kind leaf <> expected -> import_consume_cold_leaf expected leaf = None.
Proof.
  intros expected leaf Wrong. unfold import_consume_cold_leaf.
  destruct (import_leaf_kind_eq_dec (import_leaf_kind leaf) expected); congruence.
Qed.

Theorem import_malformed_collection_cannot_be_consumed : forall expected leaf,
  import_read_collection (import_leaf_payload leaf) = None ->
  import_consume_cold_leaf expected leaf = None.
Proof.
  intros expected leaf Invalid. unfold import_consume_cold_leaf.
  destruct (import_leaf_kind_eq_dec (import_leaf_kind leaf) expected); auto. now rewrite Invalid.
Qed.

Theorem import_failed_nested_item_cannot_be_consumed : forall expected leaf items rest item,
  import_read_collection (import_leaf_payload leaf) = Some (items, rest) ->
  In item items -> decode_item expected item = None -> import_consume_cold_leaf expected leaf = None.
Proof.
  intros expected leaf items rest item Read Present Invalid.
  destruct (import_consume_cold_leaf expected leaf) as [values|] eqn:Consumed; auto.
  apply import_successful_consumer_has_every_typed_item in Consumed
    as [_ [decoded [ending [Parsed All]]]].
  rewrite Read in Parsed. inversion Parsed; subst decoded ending.
  exfalso. clear Read Parsed leaf rest. induction All; cbn [In] in Present; try contradiction.
  destruct Present as [Same|Later].
  - subst x. congruence.
  - now apply IHAll.
Qed.

Definition import_consume_stored_cold (store : ImportStore) (key : nat) (expected : ImportLeafKind)
    : option (list (Value expected)) :=
  match import_resolve_cold store key with
  | None => None
  | Some leaf => import_consume_cold_leaf expected leaf
  end.

Theorem import_storage_binding_extension_preserves_typed_consumption :
  forall old new key expected values,
  import_binding_extension old new ->
  import_consume_stored_cold old key expected = Some values ->
  import_consume_stored_cold new key expected = Some values.
Proof.
  intros old new key expected values [_ Cold] Consumed.
  unfold import_consume_stored_cold in *.
  destruct (import_resolve_cold old key) as [leaf|] eqn:Read; [|discriminate].
  now rewrite (Cold key leaf Read).
Qed.

Definition import_consumable_closed_root
    (history_hash : list ImportRadixEdge -> nat) (payload_hash : list nat -> nat)
    (store : ImportStore) (root : ImportReference) : Prop :=
  import_closed history_hash payload_hash store root /\
  forall key expected,
    import_reachable history_hash payload_hash store root (ImportColdRef key expected) ->
    exists values, import_consume_stored_cold store key expected = Some values.

Theorem import_binding_extension_preserves_consumable_closed_root :
  forall history_hash payload_hash old new root,
  import_binding_extension old new ->
  import_consumable_closed_root history_hash payload_hash old root ->
  import_consumable_closed_root history_hash payload_hash new root.
Proof.
  intros history_hash payload_hash old new root Bindings [Closed Consumable].
  pose proof (import_binding_extension_preserves_checked_reads history_hash payload_hash old new Bindings)
    as Checked. split.
  - eapply import_compatible_insertion_preserves_closed_root; eauto.
  - intros key expected Reached.
    apply (import_closed_extension_reachability history_hash payload_hash old new root Closed Checked)
      in Reached.
    destruct (Consumable key expected Reached) as [values Consumed]. exists values.
    eapply import_storage_binding_extension_preserves_typed_consumption; eauto.
Qed.

Theorem import_arbitrary_batches_preserve_all_consumable_roots :
  forall history_hash payload_hash first last roots,
  Forall (import_consumable_closed_root history_hash payload_hash first) roots ->
  import_batch_run first last ->
  Forall (import_consumable_closed_root history_hash payload_hash last) roots.
Proof.
  intros history_hash payload_hash first last roots Closed Run.
  destruct (import_arbitrary_batches_preserve_exact_bindings_without_global_agreement _ _ Run)
    as [_ Bindings].
  induction Closed; constructor; auto.
  eapply import_binding_extension_preserves_consumable_closed_root; eauto.
Qed.

Definition import_consume_split_cold (first second : ImportStore) (key : nat)
    (expected : ImportLeafKind) : option (list (Value expected)) :=
  match import_split_alias_read first second key with
  | Some leaf => import_consume_cold_leaf expected leaf
  | None => None
  end.

Theorem import_guarded_batches_preserve_typed_split_readers :
  forall initial first second key expected values,
  import_consume_stored_cold initial key expected = Some values ->
  import_batch_run initial first -> import_batch_run first second ->
  import_consume_split_cold first second key expected = Some values.
Proof.
  intros initial first second key expected values Consumed First Second.
  unfold import_consume_stored_cold in Consumed.
  destruct (import_resolve_cold initial key) as [leaf|] eqn:Resolved; [|discriminate].
  unfold import_consume_split_cold.
  rewrite (import_arbitrary_batches_preserve_split_readers_without_global_agreement
    _ _ _ _ _ Resolved First Second). exact Consumed.
Qed.

Definition import_reference_consumable (store : ImportStore) (reference : ImportReference) : Prop :=
  match reference with
  | ImportHistoryRef _ _ => True
  | ImportColdRef key expected =>
      exists values, import_consume_stored_cold store key expected = Some values
  end.

Definition import_consumer_checked_read
    (history_hash : list ImportRadixEdge -> nat) (payload_hash : list nat -> nat)
    (store : ImportStore) (reference : ImportReference) : option (list ImportReference) :=
  match import_checked_read history_hash payload_hash store reference with
  | None => None
  | Some children => match reference with
    | ImportHistoryRef _ _ => Some children
    | ImportColdRef key expected => match import_consume_stored_cold store key expected with
      | Some _ => Some children
      | None => None
      end
    end
  end.

Theorem import_consumer_checked_read_binds_structure_and_consumption :
  forall history_hash payload_hash store reference children,
  import_consumer_checked_read history_hash payload_hash store reference = Some children ->
  import_checked_read history_hash payload_hash store reference = Some children /\
    import_reference_consumable store reference.
Proof.
  intros history_hash payload_hash store reference children Read.
  unfold import_consumer_checked_read in Read.
  destruct (import_checked_read history_hash payload_hash store reference) as [found|] eqn:Found;
    [|discriminate].
  destruct reference as [key context|key expected].
  - inversion Read; subst. split; auto. exact I.
  - destruct (import_consume_stored_cold store key expected) as [values|] eqn:Consumed;
      [|discriminate]. inversion Read; subst. split; auto. now exists values.
Qed.

Theorem import_consumer_checked_read_accepts_exactly_structural_and_typed_reads :
  forall history_hash payload_hash store reference children,
  import_checked_read history_hash payload_hash store reference = Some children ->
  import_reference_consumable store reference ->
  import_consumer_checked_read history_hash payload_hash store reference = Some children.
Proof.
  intros history_hash payload_hash store reference children Read Consumed.
  unfold import_consumer_checked_read. rewrite Read.
  destruct reference as [key context|key expected]; auto.
  destruct Consumed as [values Consumed]. now rewrite Consumed.
Qed.

Theorem import_authenticated_stored_leaf_passes_the_consuming_scan :
  forall history_hash Hash store expected key input leaf,
  import_authenticate_cold_bytes Hash expected key input = Some leaf ->
  import_resolve_cold store key = Some leaf ->
  import_consumer_checked_read history_hash
    (fun payload => Hash (import_nat_to_key 8 (length payload) ++ payload))
    store (ImportColdRef key expected) = Some [].
Proof.
  intros history_hash Hash store expected key input leaf Authenticated Stored.
  destruct (import_authenticated_cold_bytes_have_hash_kind_and_typed_values _ _ _ _ _ Authenticated)
    as [_ [Hashed [Kind [values [items [rest [Consumed _]]]]]]].
  apply import_consumer_checked_read_accepts_exactly_structural_and_typed_reads.
  - cbn [import_checked_read]. rewrite Stored.
    unfold import_leaf_hash_input in Hashed. rewrite Hashed, Nat.eqb_refl, Kind.
    destruct (import_leaf_kind_eq_dec expected expected); congruence.
  - exists values. unfold import_consume_stored_cold. now rewrite Stored.
Qed.

Theorem import_authenticated_guarded_insertion_establishes_consuming_read :
  forall history_hash Hash store alias expected key input leaf result,
  import_authenticate_cold_bytes Hash expected key input = Some leaf ->
  import_guarded_cold_insert store alias key leaf = Some result ->
  import_binding_extension store result /\
  import_consumer_checked_read history_hash
    (fun payload => Hash (import_nat_to_key 8 (length payload) ++ payload))
    result (ImportColdRef key expected) = Some [].
Proof.
  intros history_hash Hash store alias expected key input leaf result Authenticated Inserted.
  split.
  - eapply import_guarded_cold_insert_preserves_exact_bindings_without_global_agreement; eauto.
  - eapply import_authenticated_stored_leaf_passes_the_consuming_scan; eauto.
    eapply import_guarded_cold_insert_resolves_inserted_value; eauto.
Qed.

Theorem import_binding_extension_preserves_reference_consumption : forall old new reference,
  import_binding_extension old new ->
  import_reference_consumable old reference -> import_reference_consumable new reference.
Proof.
  intros old new reference Bindings Consumed.
  destruct reference as [key context|key expected]; simpl in *; auto.
  destruct Consumed as [values Consumed]. exists values.
  eapply import_storage_binding_extension_preserves_typed_consumption; eauto.
Qed.

Section ConsumingScan.

Variable history_hash : list ImportRadixEdge -> nat.
Variable payload_hash : list nat -> nat.

Definition import_consumer_scan_cut (root : ImportReference) (state : ImportScanState) : Prop :=
  import_scan_cut history_hash payload_hash root state /\
  forall reference, In reference (import_scan_checked state) ->
    import_reference_consumable (import_scan_store state) reference.

Inductive import_consumer_scan_step : ImportScanState -> ImportScanState -> Prop :=
| import_consumer_scan_reference : forall store checked frontier current children,
    import_consumer_checked_read history_hash payload_hash store current = Some children ->
    import_consumer_scan_step
      {| import_scan_store := store;
         import_scan_checked := checked;
         import_scan_frontier := current :: frontier |}
      {| import_scan_store := store;
         import_scan_checked := current :: checked;
         import_scan_frontier :=
           import_unchecked_frontier (current :: checked) (children ++ frontier) |}
| import_consumer_scan_concurrent_commit : forall old new checked frontier,
    import_binding_extension old new ->
    import_consumer_scan_step
      {| import_scan_store := old;
         import_scan_checked := checked;
         import_scan_frontier := frontier |}
      {| import_scan_store := new;
         import_scan_checked := checked;
         import_scan_frontier := frontier |}.

Inductive import_consumer_scan_run : ImportScanState -> ImportScanState -> Prop :=
| import_consumer_scan_run_refl : forall state, import_consumer_scan_run state state
| import_consumer_scan_run_cons : forall first middle last,
    import_consumer_scan_step first middle ->
    import_consumer_scan_run middle last -> import_consumer_scan_run first last.

Theorem import_consumer_scan_step_refines_the_structural_scan : forall first last,
  import_consumer_scan_step first last -> import_scan_step history_hash payload_hash first last.
Proof.
  intros first last Step. destruct Step.
  - apply import_scan_reference.
    eapply import_consumer_checked_read_binds_structure_and_consumption in H. tauto.
  - apply import_scan_concurrent_commit.
    now apply import_binding_extension_preserves_checked_reads.
Qed.

Theorem import_consumer_scan_step_preserves_checked_consumption : forall root first last,
  import_consumer_scan_step first last ->
  import_consumer_scan_cut root first -> import_consumer_scan_cut root last.
Proof.
  intros root first last Step [Cut Consumed]. split.
  - eapply import_scan_step_preserves_cut; eauto.
    now apply import_consumer_scan_step_refines_the_structural_scan.
  - destruct Step as [store checked frontier current children Read|old new checked frontier Bindings];
      cbn in *; intros reference Present.
    + destruct Present as [Same|Present].
      * subst reference.
        eapply import_consumer_checked_read_binds_structure_and_consumption in Read. tauto.
      * now apply Consumed.
    + eapply import_binding_extension_preserves_reference_consumption; eauto.
Qed.

Theorem import_consumer_scan_run_preserves_checked_consumption : forall root first last,
  import_consumer_scan_run first last ->
  import_consumer_scan_cut root first -> import_consumer_scan_cut root last.
Proof.
  intros root first last Run. induction Run; intros Cut; auto.
  apply IHRun. eapply import_consumer_scan_step_preserves_checked_consumption; eauto.
Qed.

Theorem import_completed_cut_contains_every_reachable_reference : forall store root checked reference,
  import_frontier_cut history_hash payload_hash store root checked [] ->
  import_reachable history_hash payload_hash store root reference -> In reference checked.
Proof.
  intros store root checked reference [Root Cut] Reached.
  induction Reached as [|parent child children Reached IH Read Present].
  - destruct Root as [Present|Impossible]; auto. inversion Impossible.
  - destruct (Cut parent IH) as [found [Found Covered]].
    rewrite Read in Found. inversion Found; subst found.
    destruct (Covered child Present) as [Done|Impossible]; auto. inversion Impossible.
Qed.

Theorem import_completed_consuming_scan_establishes_consumable_closure : forall store root last,
  import_consumer_scan_run
    {| import_scan_store := store;
       import_scan_checked := [];
       import_scan_frontier := [root] |} last ->
  import_scan_frontier last = [] ->
  import_consumable_closed_root history_hash payload_hash (import_scan_store last) root.
Proof.
  intros store root last Run Empty.
  assert (Initial : import_consumer_scan_cut root
    {| import_scan_store := store;
       import_scan_checked := [];
       import_scan_frontier := [root] |}).
  { split.
    - apply import_initial_frontier_covers_root.
    - intros reference Impossible. inversion Impossible. }
  destruct (import_consumer_scan_run_preserves_checked_consumption _ _ _ Run Initial)
    as [Cut Consumed].
  unfold import_scan_cut in Cut. rewrite Empty in Cut. split.
  - eapply import_empty_frontier_establishes_typed_closure; eauto.
  - intros key expected Reached. apply (Consumed (ImportColdRef key expected)).
    eapply import_completed_cut_contains_every_reachable_reference; eauto.
Qed.

Theorem import_completion_cannot_hide_a_failed_nested_decoder : forall store root last key expected,
  import_consumer_scan_run
    {| import_scan_store := store;
       import_scan_checked := [];
       import_scan_frontier := [root] |} last ->
  import_reachable history_hash payload_hash (import_scan_store last) root (ImportColdRef key expected) ->
  import_consume_stored_cold (import_scan_store last) key expected = None ->
  import_scan_frontier last <> [].
Proof.
  intros store root last key expected Run Reached Failed Empty.
  destruct (import_completed_consuming_scan_establishes_consumable_closure _ _ _ Run Empty)
    as [_ Consumed].
  destruct (Consumed key expected Reached) as [values Success]. congruence.
Qed.

End ConsumingScan.

End TypedColdConsumer.

Definition import_nested_failure_leaf : ImportLeaf :=
  {| import_leaf_kind := ImportJoins;
     import_leaf_payload := [1; 0; 0; 0; 0; 0; 0; 0] ++ repeat 0 8 |}.

Definition import_nested_failure_edge : ImportRadixEdge :=
  {| import_edge_index := 2; import_edge_prefix := []; import_edge_target := ImportRadixLeaf 16 |}.

Definition import_nested_failure_store : ImportStore :=
  {| import_history := fun key => if key =? 1 then Some [import_nested_failure_edge] else None;
     import_cold_raw := fun key => if key =? 16 then Some import_nested_failure_leaf else None;
     import_cold_legacy := fun _ => None |}.

Definition import_nested_failure_decoder (_ : ImportLeafKind) (bytes : list nat) : option unit :=
  match bytes with [0] => Some tt | _ => None end.

Example import_skip_typed_consumption_allows_unsafe_completion :
  import_scan_run (@length ImportRadixEdge) (@length nat)
    {| import_scan_store := import_nested_failure_store;
       import_scan_checked := []; import_scan_frontier := [ImportHistoryRef 1 []] |}
    {| import_scan_store := import_nested_failure_store;
       import_scan_checked := [ImportColdRef 16 ImportJoins; ImportHistoryRef 1 []];
       import_scan_frontier := [] |} /\
  import_consume_stored_cold (fun _ => unit) import_nested_failure_decoder
    import_nested_failure_store 16 ImportJoins = None /\
  import_consumer_checked_read (fun _ => unit) import_nested_failure_decoder
    (@length ImportRadixEdge) (@length nat) import_nested_failure_store
    (ImportColdRef 16 ImportJoins) = None.
Proof.
  split.
  - eapply import_scan_run_cons with (middle :=
      {| import_scan_store := import_nested_failure_store;
         import_scan_checked := [ImportHistoryRef 1 []];
         import_scan_frontier := [ImportColdRef 16 ImportJoins] |}).
    + apply import_scan_reference with (children := [ImportColdRef 16 ImportJoins]). reflexivity.
    + eapply import_scan_run_cons.
      * apply import_scan_reference with (children := []). reflexivity.
      * constructor.
  - split; reflexivity.
Qed.

Example import_nested_failure_fixture_has_a_valid_outer_collection :
  import_read_collection (import_leaf_payload import_nested_failure_leaf) = Some ([[]], []) /\
  import_nested_failure_decoder ImportJoins [0] = Some tt.
Proof. split; reflexivity. Qed.

Example import_collection_declared_item_without_a_length_is_rejected :
  import_read_collection [1; 0; 0; 0; 0; 0; 0; 0] = None.
Proof. reflexivity. Qed.

Example import_collection_trailing_bytes_are_not_a_second_item :
  import_read_collection (repeat 0 8 ++ [91; 92]) = Some ([], [91; 92]).
Proof. reflexivity. Qed.

Example import_empty_persisted_envelopes_have_distinct_kinds :
  import_read_persisted_leaf (repeat 0 12) =
    Some {| import_leaf_kind := ImportJoins; import_leaf_payload := [] |} /\
  import_read_persisted_leaf (1 :: repeat 0 11) =
    Some {| import_leaf_kind := ImportData; import_leaf_payload := [] |}.
Proof. split; reflexivity. Qed.
