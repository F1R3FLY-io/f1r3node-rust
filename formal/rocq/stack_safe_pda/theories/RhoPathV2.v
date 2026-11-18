From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import RhoPathV1Datum.
Import ListNotations.
Set Implicit Arguments.

(** Schema-level model of the proposed RhoPathV2 outer grammar.  Payloads are
    opaque canonical byte strings at this layer; RCI2/RMP2 validity is proved
    by their respective models.  The model deliberately exposes framing and
    cursor boundaries so the Rust adapter can be checked independently. *)
Module RhoPathV2.
Import RhoPathV1Datum.

Definition v2_magic : bytes := [82; 72; 80; 2].
Definition datum_kind : byte := 1.
Definition continuation_kind : byte := 2.
Definition join_kind : byte := 3.
Definition collection_version : byte := 1.
Definition count_byte (n : nat) : bytes :=
  [ (n / 72057594037927936) mod 256; (n / 281474976710656) mod 256;
    (n / 1099511627776) mod 256; (n / 4294967296) mod 256;
    (n / 16777216) mod 256; (n / 65536) mod 256;
    (n / 256) mod 256; n mod 256 ].

(** A datum key is the channel prefix [RHP 02 01 || frame(channel image)]
    followed by the V1 datum-relative bytes of [relative_datum_key]: the
    frame of the datum structure, then the raw canonical datum identity.
    Only the channel prefix differs from a V1 datum key with the same datum
    structure and identity. *)
Definition datum_prefix (channel : bytes) : bytes :=
  v2_magic ++ [datum_kind] ++ encode_frame channel.

Definition datum_key (channel structure ident : bytes) : bytes :=
  datum_prefix channel ++ relative_datum_key structure ident.

Definition continuation_key_unchecked (channels : list bytes) (ident : bytes) : bytes :=
  v2_magic ++ [continuation_kind; collection_version] ++
  count_byte (length channels) ++ flat_map encode_frame channels ++
  [0; 1] ++ encode_frame ident.

(** The production constructor rejects an empty ordered channel group.  Keep
    that precondition explicit in the formal model rather than admitting a
    byte string that the Rust parser will refuse. *)
Definition continuation_key (channels : list bytes) (ident : bytes) : option bytes :=
  match channels with
  | [] => None
  | _ => Some (continuation_key_unchecked channels ident)
  end.

Definition join_key (channel ident : bytes) : bytes :=
  v2_magic ++ [join_kind] ++ encode_frame channel ++ [0; 1] ++
  encode_frame ident.

(** [parse_three] splits the body of a datum key into its three fields: the
    channel-image frame, the datum-structure frame, and the raw identity
    suffix.  Like [parse_absolute_datum] of [RhoPathV1Datum] for its whole
    input, it accepts only byte values below 256.  The identity is not a
    frame, so no suffix is left over to refuse. *)
Definition parse_three (input : bytes) : option (bytes * bytes * bytes) :=
  match parse_frame input with
  | Some (channel, relative) =>
      match parse_frame relative with
      | Some (structure, ident) =>
          if bytes_valid ident then Some (channel, structure, ident) else None
      | None => None
      end
  | None => None
  end.

Definition parse_datum (input : bytes) : option (bytes * bytes * bytes) :=
  match input with
  | 82 :: 72 :: 80 :: 2 :: 1 :: rest => parse_three rest
  | _ => None
  end.

Definition parse_join (input : bytes) : option (bytes * bytes) :=
  match input with
  | 82 :: 72 :: 80 :: 2 :: 3 :: rest =>
      match parse_frame rest with
      | Some (channel, suffix) =>
          match suffix with
          | 0 :: 1 :: identity_input =>
              match parse_frame identity_input with
              | Some (ident, suffix') =>
                  match suffix' with
                  | [] => Some (channel, ident)
                  | _ => None
                  end
              | None => None
              end
          | _ => None
          end
      | None => None
      end
  | _ => None
  end.

Fixpoint bytes_value (input : bytes) : nat :=
  match input with
  | [] => 0
  | value :: tail => value + 256 * bytes_value tail
  end.

Fixpoint take_frames (count : nat) (input : bytes)
    : option (list bytes * bytes) :=
  match count with
  | 0 => Some ([], input)
  | S remaining =>
      match parse_frame input with
      | Some (frame, suffix) =>
          match take_frames remaining suffix with
          | Some (frames, rest) => Some (frame :: frames, rest)
          | None => None
          end
      | None => None
      end
  end.

Definition parse_continuation (input : bytes)
    : option (list bytes * bytes) :=
  match input with
  | 82 :: 72 :: 80 :: 2 :: 2 :: 1 :: b0 :: b1 :: b2 :: b3 :: b4 :: b5 :: b6 :: b7 :: rest =>
      if bytes_valid [b0; b1; b2; b3; b4; b5; b6; b7] then
        let count := bytes_value [b7; b6; b5; b4; b3; b2; b1; b0] in
        match count with
        | 0 => None
        | S _ =>
            match take_frames count rest with
            | Some (channels, suffix) =>
                match suffix with
                | 0 :: 1 :: identity_input =>
                    match parse_frame identity_input with
                    | Some (ident, suffix') =>
                        match suffix' with
                        | [] => Some (channels, ident)
                        | _ => None
                        end
                    | None => None
                    end
                | _ => None
                end
            | None => None
            end
        end
      else None
  | _ => None
  end.

(** Mirrors [datum_constructor_parser_round_trip] of [RhoPathV1Datum]: the
    channel frame, the structure frame and the raw identity of the V1
    datum-relative bytes are recovered exactly. *)
Lemma parse_three_round_trip : forall a b c,
  bytes_valid a = true -> bytes_valid b = true -> bytes_valid c = true ->
  parse_three (encode_frame a ++ relative_datum_key b c) = Some (a, b, c).
Proof.
  intros a b c Ha Hb Hc.
  unfold parse_three, relative_datum_key.
  rewrite (frame_encode_parse_round_trip a (encode_frame b ++ c) Ha).
  rewrite (frame_encode_parse_round_trip b c Hb).
  now rewrite Hc.
Qed.

Theorem datum_empty_round_trip :
  parse_datum (datum_key [] [] []) = Some ([], [], []).
Proof. reflexivity. Qed.

(** The identity is the raw suffix of a datum key: the key extended by any
    bytes is the key of the longer identity, and the parser returns that
    identity instead of refusing trailing bytes.  With [suffix = []] this is
    the general constructor and parser round trip. *)
Theorem datum_identity_is_the_raw_suffix : forall channel structure ident suffix,
  bytes_valid channel = true -> bytes_valid structure = true ->
  bytes_valid ident = true -> bytes_valid suffix = true ->
  datum_key channel structure ident ++ suffix =
    datum_key channel structure (ident ++ suffix) /\
  parse_datum (datum_key channel structure ident ++ suffix) =
    Some (channel, structure, ident ++ suffix).
Proof.
  intros channel structure ident suffix Hchannel Hstructure Hident Hsuffix.
  assert (Hlonger : datum_key channel structure ident ++ suffix =
    datum_key channel structure (ident ++ suffix)).
  { unfold datum_key, relative_datum_key. now rewrite <- !app_assoc. }
  split; [exact Hlonger|].
  rewrite Hlonger.
  change (parse_three (encode_frame channel ++
    relative_datum_key structure (ident ++ suffix)) =
    Some (channel, structure, ident ++ suffix)).
  apply parse_three_round_trip; [exact Hchannel|exact Hstructure|].
  rewrite bytes_valid_app, Hident, Hsuffix. reflexivity.
Qed.

Theorem join_empty_round_trip :
  parse_join (join_key [] []) = Some ([], []).
Proof. reflexivity. Qed.

Theorem continuation_zero_count_bytes_are_rejected :
  parse_continuation
    (v2_magic ++ [continuation_kind; collection_version;
      0; 0; 0; 0; 0; 0; 0; 0] ++ [0; 1] ++ encode_frame []) = None.
Proof. reflexivity. Qed.

Theorem continuation_count_mismatch_is_rejected :
  parse_continuation
    (v2_magic ++ [continuation_kind; collection_version;
      0; 0; 0; 0; 0; 0; 0; 1] ++ encode_frame []) = None.
Proof. reflexivity. Qed.

Theorem join_trailing_bytes_are_rejected :
  parse_join (join_key [] [] ++ [9]) = None.
Proof. reflexivity. Qed.

Theorem v1_v2_domains_are_prefix_disjoint : forall v1_suffix v2_suffix,
  [82; 72; 80; 1] ++ v1_suffix <> v2_magic ++ v2_suffix.
Proof. intros v1_suffix v2_suffix H; simpl in H; discriminate. Qed.

Theorem datum_kind_is_distinct :
  datum_kind <> continuation_kind /\ datum_kind <> join_kind.
Proof. split; discriminate. Qed.

Theorem continuation_count_is_eight_bytes : forall n,
  length (count_byte n) = 8.
Proof. intros; unfold count_byte; reflexivity. Qed.

Theorem empty_continuation_is_rejected :
  continuation_key [] [] = None.
Proof. reflexivity. Qed.

Theorem nonempty_continuation_has_explicit_count : forall channel ident,
  continuation_key [channel] ident =
    Some (continuation_key_unchecked [channel] ident).
Proof. reflexivity. Qed.

Theorem join_identity_is_framed : forall channel ident,
  join_key channel ident =
    v2_magic ++ [join_kind] ++ encode_frame channel ++ [0; 1] ++
    encode_frame ident.
Proof. reflexivity. Qed.

Print Assumptions datum_empty_round_trip.
Print Assumptions datum_identity_is_the_raw_suffix.
Print Assumptions v1_v2_domains_are_prefix_disjoint.
Print Assumptions continuation_count_is_eight_bytes.
End RhoPathV2.
