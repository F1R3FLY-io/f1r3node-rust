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

Definition datum_key (channel structure identity : bytes) : bytes :=
  v2_magic ++ [datum_kind] ++ encode_frame channel ++
  encode_frame structure ++ encode_frame identity.

Definition continuation_key (channels : list bytes) (identity : bytes) : bytes :=
  v2_magic ++ [continuation_kind; collection_version] ++
  count_byte (length channels) ++ flat_map encode_frame channels ++
  [0; 1] ++ encode_frame identity.

Definition join_key (channel identity : bytes) : bytes :=
  v2_magic ++ [join_kind] ++ encode_frame channel ++ [0; 1] ++
  encode_frame identity.

Definition parse_three (input : bytes) : option (bytes * bytes * bytes * bytes) :=
  match parse_frame input with
  | Some (a, rest) =>
      match parse_frame rest with
      | Some (b, rest') =>
          match parse_frame rest' with
          | Some (c, suffix) => Some (a, b, c, suffix)
          | None => None
          end
      | None => None
      end
  | None => None
  end.

Definition parse_datum (input : bytes) : option (bytes * bytes * bytes) :=
  match input with
  | 82 :: 72 :: 80 :: 2 :: 1 :: rest =>
      match parse_three rest with
      | Some (channel, structure, identity, suffix) =>
          match suffix with
          | [] => Some (channel, structure, identity)
          | _ => None
          end
      | None => None
      end
  | _ => None
  end.

Theorem datum_empty_round_trip :
  parse_datum (datum_key [] [] []) = Some ([], [], []).
Proof. reflexivity. Qed.

Theorem datum_empty_rejects_trailing_bytes :
  parse_datum (datum_key [] [] [] ++ [9]) = None.
Proof. reflexivity. Qed.

Theorem v1_v2_domains_are_disjoint : forall suffix,
  [82; 72; 80; 1] ++ suffix <> v2_magic ++ suffix.
Proof. intros suffix H; simpl in H; discriminate. Qed.

Theorem datum_kind_is_distinct :
  datum_kind <> continuation_kind /\ datum_kind <> join_kind.
Proof. split; discriminate. Qed.

Theorem continuation_count_is_eight_bytes : forall n,
  length (count_byte n) = 8.
Proof. intros; unfold count_byte; reflexivity. Qed.

Theorem empty_continuation_has_explicit_zero_count :
  continuation_key [] [] = v2_magic ++ [continuation_kind; collection_version;
    0; 0; 0; 0; 0; 0; 0; 0; 0; 1] ++ encode_frame [].
Proof. reflexivity. Qed.

Theorem join_identity_is_framed : forall channel identity,
  join_key channel identity =
    v2_magic ++ [join_kind] ++ encode_frame channel ++ [0; 1] ++
    encode_frame identity.
Proof. reflexivity. Qed.

Print Assumptions datum_empty_round_trip.
Print Assumptions datum_empty_rejects_trailing_bytes.
Print Assumptions v1_v2_domains_are_disjoint.
Print Assumptions continuation_count_is_eight_bytes.
End RhoPathV2.
