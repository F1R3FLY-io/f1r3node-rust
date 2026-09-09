From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportCodec.
Import ListNotations.

Definition ImportWirePath := list (list nat * option nat).

Inductive ImportCursor : Type :=
| ImportCursorStart : list nat -> ImportCursor
| ImportCursorResume : list nat -> list nat -> list nat -> ImportCursor.

Definition import_cursor_word_validb (word : list nat) : bool :=
  (length word =? 32) && forallb (fun byte => byte <? 256) word.

Definition import_cursor_validb (cursor : ImportCursor) : bool :=
  match cursor with
  | ImportCursorStart root => import_cursor_word_validb root
  | ImportCursorResume root prefix carrier =>
      import_cursor_word_validb root && import_cursor_word_validb carrier &&
      (length prefix <=? 128) && forallb (fun byte => byte <? 256) prefix
  end.

Definition import_cursor_pad (prefix : list nat) : list nat :=
  prefix ++ repeat 0 (128 - length prefix).

Definition import_cursor_encode_raw (cursor : ImportCursor) : ImportWirePath :=
  match cursor with
  | ImportCursorStart root => [(root, None)]
  | ImportCursorResume root prefix carrier =>
      let padded := import_cursor_pad prefix in
      let tail1 := skipn 32 padded in
      let tail2 := skipn 32 tail1 in
      [(root, None); (length prefix :: repeat 0 31, None);
       (firstn 32 padded, None); (firstn 32 tail1, None);
       (firstn 32 tail2, None); (skipn 32 tail2, None); (carrier, None)]
  end.

Definition import_cursor_encode (cursor : ImportCursor) : option ImportWirePath :=
  if import_cursor_validb cursor then Some (import_cursor_encode_raw cursor) else None.

Definition import_cursor_candidate (path : ImportWirePath) : option ImportCursor :=
  match path with
  | [(root, None)] => Some (ImportCursorStart root)
  | [(root, None); (size, None); (w0, None); (w1, None);
      (w2, None); (w3, None); (carrier, None)] =>
      Some (ImportCursorResume root (firstn (hd 0 size) (w0 ++ w1 ++ w2 ++ w3)) carrier)
  | _ => None
  end.

Definition import_cursor_path_eq_dec (first second : ImportWirePath)
    : {first = second} + {first <> second}.
Proof. repeat decide equality. Defined.

Definition import_cursor_decode (path : ImportWirePath) : option ImportCursor :=
  match import_cursor_candidate path with
  | None => None
  | Some cursor =>
      match import_cursor_encode cursor with
      | None => None
      | Some encoded => if import_cursor_path_eq_dec encoded path then Some cursor else None
      end
  end.

Lemma import_cursor_chunks_reassemble : forall bytes : list nat,
  firstn 32 bytes ++ firstn 32 (skipn 32 bytes) ++
    firstn 32 (skipn 32 (skipn 32 bytes)) ++ skipn 32 (skipn 32 (skipn 32 bytes)) = bytes.
Proof. intros bytes. now repeat rewrite firstn_skipn. Qed.

Theorem import_cursor_candidate_recovers_encoded_cursor : forall cursor,
  import_cursor_candidate (import_cursor_encode_raw cursor) = Some cursor.
Proof.
  intros [root|root prefix carrier]; [reflexivity|].
  cbn [import_cursor_encode_raw import_cursor_candidate hd].
  rewrite import_cursor_chunks_reassemble. unfold import_cursor_pad.
  rewrite firstn_app, firstn_all, Nat.sub_diag. simpl. now rewrite app_nil_r.
Qed.

Theorem import_cursor_encoder_requires_validity : forall cursor path,
  import_cursor_encode cursor = Some path ->
  import_cursor_validb cursor = true /\ path = import_cursor_encode_raw cursor.
Proof.
  intros cursor path Encoded. unfold import_cursor_encode in Encoded.
  destruct (import_cursor_validb cursor) eqn:Valid; inversion Encoded. now split.
Qed.

Theorem import_cursor_decoder_requires_canonical_encoding : forall path cursor,
  import_cursor_decode path = Some cursor -> import_cursor_encode cursor = Some path.
Proof.
  intros path cursor Decoded. unfold import_cursor_decode in Decoded.
  destruct (import_cursor_candidate path) as [candidate|]; try discriminate.
  destruct (import_cursor_encode candidate) as [encoded|] eqn:Encoded; try discriminate.
  destruct (import_cursor_path_eq_dec encoded path) as [Same|]; try discriminate.
  inversion Decoded; subst. assumption.
Qed.

Theorem import_cursor_decode_encode_round_trip : forall cursor path,
  import_cursor_encode cursor = Some path -> import_cursor_decode path = Some cursor.
Proof.
  intros cursor path Encoded.
  pose proof (import_cursor_encoder_requires_validity _ _ Encoded) as [_ Same].
  subst path. unfold import_cursor_decode.
  rewrite import_cursor_candidate_recovers_encoded_cursor, Encoded.
  destruct (import_cursor_path_eq_dec (import_cursor_encode_raw cursor)
    (import_cursor_encode_raw cursor)); congruence.
Qed.

Theorem import_cursor_acceptance_characterization : forall path cursor,
  import_cursor_decode path = Some cursor <-> import_cursor_encode cursor = Some path.
Proof.
  split; [apply import_cursor_decoder_requires_canonical_encoding|
    apply import_cursor_decode_encode_round_trip].
Qed.

Theorem import_cursor_encoding_is_injective : forall first second path,
  import_cursor_encode first = Some path -> import_cursor_encode second = Some path -> first = second.
Proof.
  intros first second path First Second.
  apply import_cursor_decode_encode_round_trip in First.
  apply import_cursor_decode_encode_round_trip in Second. congruence.
Qed.

Theorem import_cursor_decode_rejects_invalid_candidate : forall path cursor,
  import_cursor_candidate path = Some cursor -> import_cursor_validb cursor = false ->
  import_cursor_decode path = None.
Proof.
  intros path cursor Candidate Invalid.
  unfold import_cursor_decode. rewrite Candidate. unfold import_cursor_encode. now rewrite Invalid.
Qed.

Theorem import_cursor_decode_rejects_noncanonical_candidate : forall path cursor,
  import_cursor_candidate path = Some cursor -> import_cursor_encode_raw cursor <> path ->
  import_cursor_decode path = None.
Proof.
  intros path cursor Candidate Different.
  unfold import_cursor_decode. rewrite Candidate. unfold import_cursor_encode.
  destruct (import_cursor_validb cursor); auto.
  destruct (import_cursor_path_eq_dec (import_cursor_encode_raw cursor) path); congruence.
Qed.

Lemma import_cursor_word_validity_characterization : forall word,
  import_cursor_word_validb word = true <-> length word = 32 /\ Forall import_wire_byte word.
Proof.
  intros word. unfold import_cursor_word_validb.
  rewrite Bool.andb_true_iff, Nat.eqb_eq, forallb_forall, Forall_forall.
  unfold import_wire_byte. split; intros [Width Bytes]; split; auto.
  - intros byte Present. apply Nat.ltb_lt. now apply Bytes.
  - intros byte Present. apply Nat.ltb_lt. now apply Bytes.
Qed.

Theorem import_resume_cursor_validity_characterization : forall root prefix carrier,
  import_cursor_validb (ImportCursorResume root prefix carrier) = true <->
  (length root = 32 /\ Forall import_wire_byte root) /\
  (length carrier = 32 /\ Forall import_wire_byte carrier) /\
  length prefix <= 128 /\ Forall import_wire_byte prefix.
Proof.
  intros root prefix carrier. cbn [import_cursor_validb].
  rewrite !Bool.andb_true_iff, !import_cursor_word_validity_characterization,
    Nat.leb_le, forallb_forall, !Forall_forall.
  setoid_rewrite Nat.ltb_lt. unfold import_wire_byte. tauto.
Qed.

Theorem import_cursor_padding_has_exact_capacity : forall prefix,
  length prefix <= 128 -> length (import_cursor_pad prefix) = 128.
Proof.
  intros prefix Width. unfold import_cursor_pad. rewrite length_app, repeat_length. lia.
Qed.

Theorem import_accepted_cursor_has_exact_shape : forall path cursor,
  import_cursor_decode path = Some cursor ->
  length path = match cursor with ImportCursorStart _ => 1 | ImportCursorResume _ _ _ => 7 end /\
  Forall (fun entry => snd entry = None) path.
Proof.
  intros path cursor Decoded.
  apply import_cursor_decoder_requires_canonical_encoding in Decoded.
  apply import_cursor_encoder_requires_validity in Decoded as [_ Same].
  subst path. destruct cursor; cbn [import_cursor_encode_raw]; split; repeat constructor.
Qed.

Lemma import_cursor_zero_padding_has_valid_bytes : forall count,
  Forall import_wire_byte (repeat 0 count).
Proof. induction count; constructor; auto. unfold import_wire_byte. lia. Qed.

Lemma import_cursor_split_preserves_valid_bytes : forall count bytes,
  Forall import_wire_byte bytes ->
  Forall import_wire_byte (firstn count bytes) /\ Forall import_wire_byte (skipn count bytes).
Proof.
  intros count bytes Valid. apply Forall_app.
  now rewrite firstn_skipn.
Qed.

Theorem import_cursor_prefix_words_have_exact_width : forall prefix,
  length prefix <= 128 ->
  let padded := import_cursor_pad prefix in
  length (firstn 32 padded) = 32 /\
  length (firstn 32 (skipn 32 padded)) = 32 /\
  length (firstn 32 (skipn 32 (skipn 32 padded))) = 32 /\
  length (skipn 32 (skipn 32 (skipn 32 padded))) = 32.
Proof.
  intros prefix Width. cbv zeta.
  rewrite !length_firstn, !length_skipn, import_cursor_padding_has_exact_capacity by assumption.
  repeat split; reflexivity.
Qed.

Theorem import_cursor_prefix_words_have_valid_bytes : forall prefix,
  Forall import_wire_byte prefix ->
  let padded := import_cursor_pad prefix in
  Forall import_wire_byte (firstn 32 padded) /\
  Forall import_wire_byte (firstn 32 (skipn 32 padded)) /\
  Forall import_wire_byte (firstn 32 (skipn 32 (skipn 32 padded))) /\
  Forall import_wire_byte (skipn 32 (skipn 32 (skipn 32 padded))).
Proof.
  intros prefix Valid. cbv zeta.
  assert (Forall import_wire_byte (import_cursor_pad prefix)) as Padded.
  { unfold import_cursor_pad. apply Forall_app. split; auto.
    apply import_cursor_zero_padding_has_valid_bytes. }
  destruct (import_cursor_split_preserves_valid_bytes 32 _ Padded) as [First Tail1].
  destruct (import_cursor_split_preserves_valid_bytes 32 _ Tail1) as [Second Tail2].
  destruct (import_cursor_split_preserves_valid_bytes 32 _ Tail2) as [Third Fourth].
  tauto.
Qed.

Theorem import_valid_cursor_encoding_has_fixed_width_words : forall cursor,
  import_cursor_validb cursor = true ->
  Forall (fun entry => length (fst entry) = 32 /\ Forall import_wire_byte (fst entry))
    (import_cursor_encode_raw cursor).
Proof.
  intros [root|root prefix carrier] Valid.
  - cbn [import_cursor_validb] in Valid.
    apply import_cursor_word_validity_characterization in Valid.
    cbn [import_cursor_encode_raw]. constructor; auto.
  - apply import_resume_cursor_validity_characterization in Valid.
    destruct Valid as [Root [Carrier [Width Bytes]]].
    pose proof (import_cursor_prefix_words_have_exact_width _ Width) as [W0 [W1 [W2 W3]]].
    pose proof (import_cursor_prefix_words_have_valid_bytes _ Bytes) as [B0 [B1 [B2 B3]]].
    assert (length (length prefix :: repeat 0 31) = 32 /\
      Forall import_wire_byte (length prefix :: repeat 0 31)) as Size.
    { split; [reflexivity|]. constructor.
      - unfold import_wire_byte. lia.
      - apply import_cursor_zero_padding_has_valid_bytes. }
    cbn [import_cursor_encode_raw].
    constructor; [exact Root|]. constructor; [exact Size|].
    constructor; [now split|]. constructor; [now split|].
    constructor; [now split|]. constructor; [now split|].
    constructor; [exact Carrier|constructor].
Qed.

Theorem import_accepted_cursor_has_fixed_width_words : forall path cursor,
  import_cursor_decode path = Some cursor ->
  Forall (fun entry => length (fst entry) = 32 /\ Forall import_wire_byte (fst entry)) path.
Proof.
  intros path cursor Decoded.
  apply import_cursor_decoder_requires_canonical_encoding in Decoded.
  apply import_cursor_encoder_requires_validity in Decoded as [Valid Same].
  subst path. now apply import_valid_cursor_encoding_has_fixed_width_words.
Qed.

Definition import_cursor_root (cursor : ImportCursor) : list nat :=
  match cursor with
  | ImportCursorStart root | ImportCursorResume root _ _ => root
  end.

Definition import_cursor_carrier (cursor : ImportCursor) : list nat :=
  match cursor with
  | ImportCursorStart root => root
  | ImportCursorResume _ _ carrier => carrier
  end.

Theorem import_accepted_cursor_preserves_root_and_carrier : forall path cursor,
  import_cursor_decode path = Some cursor ->
  hd_error path = Some (import_cursor_root cursor, None) /\
  last path ([], None) = (import_cursor_carrier cursor, None).
Proof.
  intros path cursor Decoded.
  apply import_cursor_decoder_requires_canonical_encoding in Decoded.
  apply import_cursor_encoder_requires_validity in Decoded as [_ Same].
  subst path. destruct cursor; split; reflexivity.
Qed.

Example import_cursor_accepts_valid_singleton :
  import_cursor_decode [(repeat 0 32, None)] = Some (ImportCursorStart (repeat 0 32)).
Proof. reflexivity. Qed.

Example import_cursor_accepts_empty_prefix :
  import_cursor_decode (import_cursor_encode_raw
    (ImportCursorResume (repeat 0 32) [] (repeat 1 32))) =
  Some (ImportCursorResume (repeat 0 32) [] (repeat 1 32)).
Proof. reflexivity. Qed.

Example import_cursor_accepts_maximum_prefix :
  import_cursor_decode (import_cursor_encode_raw
    (ImportCursorResume (repeat 0 32) (repeat 255 128) (repeat 1 32))) =
  Some (ImportCursorResume (repeat 0 32) (repeat 255 128) (repeat 1 32)).
Proof. reflexivity. Qed.

Example import_cursor_rejects_empty_path : import_cursor_decode [] = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_short_root : import_cursor_decode [(repeat 0 31, None)] = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_optional_index : import_cursor_decode [(repeat 0 32, Some 0)] = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_oversized_prefix :
  import_cursor_encode (ImportCursorResume (repeat 0 32) (repeat 0 129) (repeat 1 32)) = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_oversized_word_byte :
  import_cursor_decode [(256 :: repeat 0 31, None)] = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_extra_entry :
  import_cursor_decode (import_cursor_encode_raw
    (ImportCursorResume (repeat 0 32) [] (repeat 1 32)) ++ [(repeat 0 32, None)]) = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_nonzero_size_padding :
  import_cursor_decode [(repeat 0 32, None); (0 :: 1 :: repeat 0 30, None);
    (repeat 0 32, None); (repeat 0 32, None); (repeat 0 32, None);
    (repeat 0 32, None); (repeat 1 32, None)] = None.
Proof. reflexivity. Qed.

Example import_cursor_rejects_nonzero_unused_prefix_padding :
  import_cursor_decode [(repeat 0 32, None); (repeat 0 32, None);
    (1 :: repeat 0 31, None); (repeat 0 32, None); (repeat 0 32, None);
    (repeat 0 32, None); (repeat 1 32, None)] = None.
Proof. reflexivity. Qed.
