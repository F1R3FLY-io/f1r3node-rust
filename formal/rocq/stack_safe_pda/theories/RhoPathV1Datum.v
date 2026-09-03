From Stdlib Require Import List Bool Arith Lia.

Import ListNotations.
Set Implicit Arguments.

(**
  Exact executable model of the production [RhoPathV1] datum-key language in
  [rspace++/src/rspace/rho_path.rs].

  A datum key is

    [R; H; P; 1] || datum-kind
      || frame(channel structure)
      || frame(channel identity)
      || frame(datum structure)
      || raw canonical datum identity.

  Frames escape a payload zero as [[0; 255]] and terminate with [[0; 0]].
  The canonical datum identity is deliberately *not* framed.  Bytes are
  represented by natural numbers in Rocq, so checked constructors and parsers
  reject values outside the physical [u8] range before producing a key or a
  view.  All traversals are structurally recursive over their input; there is
  no fuel or depth limit.
*)
Module RhoPathV1Datum.

Definition byte := nat.
Definition bytes := list byte.

Definition rho_path_v1_magic : bytes := [82; 72; 80; 1].
Definition rho_path_v1_datum_kind : byte := 1.
Definition frame_escape : byte := 0.
Definition frame_escaped_zero : byte := 255.
Definition frame_end : byte := 0.

Definition byte_valid (value : byte) : bool := value <? 256.
Definition bytes_valid (values : bytes) : bool := forallb byte_valid values.

Fixpoint escape_frame_payload (payload : bytes) : bytes :=
  match payload with
  | [] => []
  | value :: tail =>
      if value =? frame_escape
      then frame_escape :: frame_escaped_zero :: escape_frame_payload tail
      else value :: escape_frame_payload tail
  end.

Definition encode_frame (payload : bytes) : bytes :=
  escape_frame_payload payload ++ [frame_escape; frame_end].

Definition encode_frame_checked (payload : bytes) : option bytes :=
  if bytes_valid payload then Some (encode_frame payload) else None.

(** [parse_frame] is the executable counterpart of production [take_frame]
    plus [decode_frame].  Its result contains the decoded payload and the
    untouched suffix. *)
Fixpoint parse_frame (input : bytes) : option (bytes * bytes) :=
  match input with
  | [] => None
  | value :: tail =>
      if byte_valid value then
        if value =? frame_escape then
          match tail with
          | [] => None
          | escaped :: remaining =>
              if byte_valid escaped then
                if escaped =? frame_end then Some ([], remaining)
                else if escaped =? frame_escaped_zero then
                  match parse_frame remaining with
                  | Some (payload, suffix) => Some (frame_escape :: payload, suffix)
                  | None => None
                  end
                else None
              else None
          end
        else
          match parse_frame tail with
          | Some (payload, suffix) => Some (value :: payload, suffix)
          | None => None
          end
      else None
  end.

Lemma bytes_valid_cons : forall value tail,
  bytes_valid (value :: tail) = byte_valid value && bytes_valid tail.
Proof. reflexivity. Qed.

Lemma bytes_valid_app : forall left right,
  bytes_valid (left ++ right) = bytes_valid left && bytes_valid right.
Proof.
  induction left as [|value tail IH]; intro right; simpl; [reflexivity|].
  now rewrite IH, andb_assoc.
Qed.

Lemma escape_frame_payload_valid : forall payload,
  bytes_valid payload = true ->
  bytes_valid (escape_frame_payload payload) = true.
Proof.
  induction payload as [|value tail IH]; intro Hvalid; [reflexivity|].
  rewrite bytes_valid_cons, Bool.andb_true_iff in Hvalid.
  destruct Hvalid as [Hvalue Htail].
  destruct (value =? frame_escape) eqn:Hzero.
  - unfold escape_frame_payload; rewrite Hzero.
    repeat rewrite bytes_valid_cons. cbn [byte_valid].
    now apply IH.
  - unfold escape_frame_payload; rewrite Hzero.
    rewrite bytes_valid_cons, Hvalue. cbn.
    now apply IH.
Qed.

Lemma encode_frame_valid : forall payload,
  bytes_valid payload = true -> bytes_valid (encode_frame payload) = true.
Proof.
  intros payload Hvalid. unfold encode_frame.
  rewrite bytes_valid_app, escape_frame_payload_valid by exact Hvalid.
  reflexivity.
Qed.

Lemma encode_frame_cons_zero : forall tail,
  encode_frame (frame_escape :: tail) =
    frame_escape :: frame_escaped_zero :: encode_frame tail.
Proof. reflexivity. Qed.

Lemma encode_frame_cons_nonzero : forall value tail,
  value <> frame_escape ->
  encode_frame (value :: tail) = value :: encode_frame tail.
Proof.
  intros value tail Hnonzero. unfold frame_escape in Hnonzero.
  destruct value as [|value]; [contradiction|reflexivity].
Qed.

(** General round trip, including payloads containing any number and placement
    of zero bytes. *)
Theorem frame_encode_parse_round_trip : forall payload suffix,
  bytes_valid payload = true ->
  parse_frame (encode_frame payload ++ suffix) = Some (payload, suffix).
Proof.
  induction payload as [|value tail IH]; intros suffix Hpayload.
  - reflexivity.
  - rewrite bytes_valid_cons, Bool.andb_true_iff in Hpayload.
    destruct Hpayload as [Hvalue Htail].
    destruct (Nat.eq_dec value frame_escape) as [-> | Hnonzero].
    + rewrite encode_frame_cons_zero. simpl.
      now rewrite IH.
    + rewrite encode_frame_cons_nonzero by exact Hnonzero. simpl.
      rewrite Hvalue.
      apply Nat.eqb_neq in Hnonzero. rewrite Hnonzero.
      now rewrite IH.
Qed.

(** Successful frame parsing recognizes precisely one canonical encoded
    frame prefix; this is the key lemma that rules out ambiguous partitions. *)
Theorem parse_frame_sound : forall input payload suffix,
  parse_frame input = Some (payload, suffix) ->
  bytes_valid payload = true /\
  input = encode_frame payload ++ suffix.
Proof.
  fix IH 1.
  intros input payload suffix Hparse.
  destruct input as [|value tail]; simpl in Hparse; [discriminate|].
  destruct (byte_valid value) eqn:Hvalue; [|discriminate].
  destruct (value =? frame_escape) eqn:Hzero.
  - apply Nat.eqb_eq in Hzero. subst value.
    destruct tail as [|escaped remaining]; [discriminate|].
    destruct (byte_valid escaped) eqn:Hescaped; [|discriminate].
    destruct (escaped =? frame_end) eqn:Hend.
    + inversion Hparse; subst payload suffix; clear Hparse.
      apply Nat.eqb_eq in Hend. subst escaped.
      split; reflexivity.
    + destruct (escaped =? frame_escaped_zero) eqn:Hescaped_zero;
        [|discriminate].
      destruct (parse_frame remaining) as [[parsed rest]|] eqn:Hrecursive;
        [|discriminate].
      inversion Hparse; subst payload suffix; clear Hparse.
      apply Nat.eqb_eq in Hescaped_zero. subst escaped.
      specialize (IH remaining parsed rest Hrecursive).
      destruct IH as [Hparsed Hreconstruct].
      split.
      * simpl. exact Hparsed.
      * rewrite encode_frame_cons_zero, Hreconstruct. reflexivity.
  - destruct (parse_frame tail) as [[parsed rest]|] eqn:Hrecursive;
      [|discriminate].
    inversion Hparse; subst payload suffix; clear Hparse.
    specialize (IH tail parsed rest Hrecursive).
    destruct IH as [Hparsed Hreconstruct].
    apply Nat.eqb_neq in Hzero.
    split.
    + simpl. rewrite Hvalue. exact Hparsed.
    + rewrite encode_frame_cons_nonzero by exact Hzero.
      now rewrite Hreconstruct.
Qed.

Corollary parse_frame_sound_with_valid_suffix : forall input payload suffix,
  parse_frame input = Some (payload, suffix) ->
  bytes_valid suffix = true ->
  bytes_valid input = true /\
  input = encode_frame payload ++ suffix.
Proof.
  intros input payload suffix Hparse Hsuffix.
  destruct (@parse_frame_sound input payload suffix Hparse)
    as [Hpayload Hreconstruct].
  split; [|exact Hreconstruct]. rewrite Hreconstruct, bytes_valid_app.
  now rewrite encode_frame_valid by exact Hpayload.
Qed.

Record DatumPathView : Type := DatumPathViewState {
  datum_view_channel_structure : bytes;
  datum_view_channel_identity : bytes;
  datum_view_datum_structure : bytes;
  datum_view_datum_identity : bytes
}.

Definition datum_channel_marker
    (channel_structure channel_identity : bytes) : bytes :=
  rho_path_v1_magic ++ [rho_path_v1_datum_kind] ++
  encode_frame channel_structure ++ encode_frame channel_identity.

Definition relative_datum_key
    (datum_structure datum_identity : bytes) : bytes :=
  encode_frame datum_structure ++ datum_identity.

Definition absolute_datum_key
    (channel_structure channel_identity datum_structure datum_identity : bytes)
    : bytes :=
  datum_channel_marker channel_structure channel_identity ++
  relative_datum_key datum_structure datum_identity.

Definition absolute_datum_key_checked
    (channel_structure channel_identity datum_structure datum_identity : bytes)
    : option bytes :=
  if bytes_valid channel_structure && bytes_valid channel_identity &&
       bytes_valid datum_structure && bytes_valid datum_identity
  then Some (absolute_datum_key channel_structure channel_identity
    datum_structure datum_identity)
  else None.

Fixpoint strip_exact_prefix (expected input : bytes) : option bytes :=
  match expected with
  | [] => Some input
  | wanted :: expected_tail =>
      match input with
      | [] => None
      | actual :: input_tail =>
          if wanted =? actual
          then strip_exact_prefix expected_tail input_tail
          else None
      end
  end.

(** This staged decoder gives the same header priority as production
    [parse_channel]: an incomplete or unequal four-byte magic is rejected,
    then a missing/wrong datum kind is rejected, before any frame can yield a
    view.  This is intentionally only the syntactic [parse_datum] boundary.
    Production semantic admission subsequently decodes and canonically
    re-encodes channel/datum identities, compares their structure frames, and
    requires a multiplicity endpoint.  Those type-specific checks do not alter
    this byte partition and do not belong in this generic syntax parser. *)
Definition parse_absolute_datum (input : bytes) : option DatumPathView :=
  if bytes_valid input then
    match strip_exact_prefix rho_path_v1_magic input with
    | Some (kind :: framed) =>
      if kind =? rho_path_v1_datum_kind then
        match parse_frame framed with
        | Some (channel_structure, after_channel_structure) =>
            match parse_frame after_channel_structure with
            | Some (channel_identity, after_channel_identity) =>
                match parse_frame after_channel_identity with
                | Some (datum_structure, datum_identity) =>
                    Some (DatumPathViewState channel_structure channel_identity
                      datum_structure datum_identity)
                | None => None
                end
            | None => None
            end
        | None => None
        end
      else None
    | _ => None
    end
  else None.

Lemma strip_exact_prefix_round_trip : forall prefix suffix,
  strip_exact_prefix prefix (prefix ++ suffix) = Some suffix.
Proof.
  induction prefix as [|wanted tail IH]; intro suffix; simpl; [reflexivity|].
  rewrite Nat.eqb_refl. exact (IH suffix).
Qed.

Lemma strip_exact_prefix_sound : forall prefix input suffix,
  strip_exact_prefix prefix input = Some suffix -> input = prefix ++ suffix.
Proof.
  induction prefix as [|wanted tail IH]; intros input suffix Hstrip.
  - simpl in Hstrip. now inversion Hstrip.
  - destruct input as [|actual input_tail]; simpl in Hstrip; [discriminate|].
    destruct (wanted =? actual) eqn:Hequal; [|discriminate].
    apply Nat.eqb_eq in Hequal. subst actual.
    specialize (IH input_tail suffix Hstrip). simpl. now rewrite IH.
Qed.

Lemma datum_channel_marker_valid : forall channel_structure channel_identity,
  bytes_valid channel_structure = true ->
  bytes_valid channel_identity = true ->
  bytes_valid (datum_channel_marker channel_structure channel_identity) = true.
Proof.
  intros channel_structure channel_identity Hstructure Hidentity.
  unfold datum_channel_marker.
  repeat rewrite bytes_valid_app.
  rewrite (@encode_frame_valid channel_structure Hstructure),
    (@encode_frame_valid channel_identity Hidentity).
  reflexivity.
Qed.

Lemma relative_datum_key_valid : forall datum_structure datum_identity,
  bytes_valid datum_structure = true ->
  bytes_valid datum_identity = true ->
  bytes_valid (relative_datum_key datum_structure datum_identity) = true.
Proof.
  intros datum_structure datum_identity Hstructure Hidentity.
  unfold relative_datum_key. rewrite bytes_valid_app.
  now rewrite encode_frame_valid by exact Hstructure.
Qed.

Lemma absolute_datum_key_valid : forall
    channel_structure channel_identity datum_structure datum_identity,
  bytes_valid channel_structure = true ->
  bytes_valid channel_identity = true ->
  bytes_valid datum_structure = true ->
  bytes_valid datum_identity = true ->
  bytes_valid (absolute_datum_key channel_structure channel_identity
    datum_structure datum_identity) = true.
Proof.
  intros channel_structure channel_identity datum_structure datum_identity
    Hchannel_structure Hchannel_identity Hdatum_structure Hdatum_identity.
  unfold absolute_datum_key. rewrite bytes_valid_app.
  rewrite datum_channel_marker_valid by assumption.
  now rewrite relative_datum_key_valid by assumption.
Qed.

(** The three framed dimensions and raw identity suffix are recovered exactly. *)
Theorem datum_constructor_parser_round_trip : forall
    channel_structure channel_identity datum_structure datum_identity,
  bytes_valid channel_structure = true ->
  bytes_valid channel_identity = true ->
  bytes_valid datum_structure = true ->
  bytes_valid datum_identity = true ->
  parse_absolute_datum
    (absolute_datum_key channel_structure channel_identity
      datum_structure datum_identity) =
  Some (DatumPathViewState channel_structure channel_identity
    datum_structure datum_identity).
Proof.
  intros channel_structure channel_identity datum_structure datum_identity
    Hchannel_structure Hchannel_identity Hdatum_structure Hdatum_identity.
  unfold parse_absolute_datum.
  rewrite absolute_datum_key_valid by assumption.
  unfold absolute_datum_key, datum_channel_marker,
    relative_datum_key, rho_path_v1_magic, rho_path_v1_datum_kind.
  cbn.
  repeat rewrite <- app_assoc.
  rewrite frame_encode_parse_round_trip by assumption.
  rewrite frame_encode_parse_round_trip by assumption.
  now rewrite frame_encode_parse_round_trip by assumption.
Qed.

Theorem checked_datum_constructor_parser_round_trip : forall
    channel_structure channel_identity datum_structure datum_identity key,
  absolute_datum_key_checked channel_structure channel_identity
      datum_structure datum_identity = Some key ->
  parse_absolute_datum key =
    Some (DatumPathViewState channel_structure channel_identity
      datum_structure datum_identity).
Proof.
  intros channel_structure channel_identity datum_structure datum_identity key
    Hchecked.
  unfold absolute_datum_key_checked in Hchecked.
  destruct (bytes_valid channel_structure && bytes_valid channel_identity &&
    bytes_valid datum_structure && bytes_valid datum_identity) eqn:Hvalid;
    [|discriminate].
  inversion Hchecked; subst key; clear Hchecked.
  repeat rewrite Bool.andb_true_iff in Hvalid.
  destruct Hvalid as [[[Hchannel_structure Hchannel_identity]
    Hdatum_structure] Hdatum_identity].
  now apply datum_constructor_parser_round_trip.
Qed.

Theorem parse_absolute_datum_sound : forall input view,
  parse_absolute_datum input = Some view ->
  bytes_valid input = true /\
  input = absolute_datum_key
    (datum_view_channel_structure view)
    (datum_view_channel_identity view)
    (datum_view_datum_structure view)
    (datum_view_datum_identity view).
Proof.
  intros input view Hparse.
  unfold parse_absolute_datum in Hparse.
  destruct (bytes_valid input) eqn:Hvalid; [|discriminate].
  destruct (strip_exact_prefix rho_path_v1_magic input) as [after_magic|]
    eqn:Hmagic; [|discriminate].
  destruct after_magic as [|kind framed]; [discriminate|].
  destruct (kind =? rho_path_v1_datum_kind) eqn:Hkind; [|discriminate].
  destruct (parse_frame framed) as [[channel_structure after_channel_structure]|]
    eqn:Hchannel_structure; [|discriminate].
  destruct (parse_frame after_channel_structure)
    as [[channel_identity after_channel_identity]|] eqn:Hchannel_identity;
    [|discriminate].
  destruct (parse_frame after_channel_identity)
    as [[datum_structure datum_identity]|] eqn:Hdatum_structure;
    [|discriminate].
  inversion Hparse; subst view; clear Hparse.
  apply Nat.eqb_eq in Hkind. subst kind.
  pose proof (@strip_exact_prefix_sound rho_path_v1_magic input
    (rho_path_v1_datum_kind :: framed) Hmagic) as Hheader.
  pose proof (@parse_frame_sound framed channel_structure
    after_channel_structure Hchannel_structure) as Hchannel_structure_sound.
  pose proof (@parse_frame_sound after_channel_structure channel_identity
    after_channel_identity Hchannel_identity) as Hchannel_identity_sound.
  pose proof (@parse_frame_sound after_channel_identity datum_structure
    datum_identity Hdatum_structure) as Hdatum_structure_sound.
  destruct Hchannel_structure_sound as [_ Hframed].
  destruct Hchannel_identity_sound as [_ Hafter_channel_structure].
  destruct Hdatum_structure_sound as [_ Hafter_channel_identity].
  split; [reflexivity|].
  unfold absolute_datum_key, datum_channel_marker, relative_datum_key.
  rewrite Hheader, Hframed, Hafter_channel_structure, Hafter_channel_identity.
  repeat rewrite app_assoc. reflexivity.
Qed.

(** Exact split used by the two-level authority.  The marker ends immediately
    after the channel-identity terminator, exactly at production
    [DatumPathView::channel_prefix_len]; the relative key starts with the
    datum-structure frame and owns the complete raw identity suffix. *)
Record DatumKeyPartition : Type := DatumKeyPartitionState {
  partition_channel_marker : bytes;
  partition_relative_key : bytes
}.

Definition partition_absolute_datum (input : bytes)
    : option DatumKeyPartition :=
  match parse_absolute_datum input with
  | Some view =>
      Some (DatumKeyPartitionState
        (datum_channel_marker
          (datum_view_channel_structure view)
          (datum_view_channel_identity view))
        (relative_datum_key
          (datum_view_datum_structure view)
          (datum_view_datum_identity view)))
  | None => None
  end.

Definition reconstruct_datum_partition (partition : DatumKeyPartition)
    : bytes :=
  partition_channel_marker partition ++ partition_relative_key partition.

Theorem datum_partition_reconstructs_absolute_key : forall input partition,
  partition_absolute_datum input = Some partition ->
  reconstruct_datum_partition partition = input.
Proof.
  intros input partition Hpartition.
  unfold partition_absolute_datum in Hpartition.
  destruct (parse_absolute_datum input) as [view|] eqn:Hparse;
    [|discriminate].
  inversion Hpartition; subst partition; clear Hpartition.
  destruct (@parse_absolute_datum_sound input view Hparse)
    as [_ Hreconstruct].
  unfold reconstruct_datum_partition, absolute_datum_key in *; simpl.
  exact (eq_sym Hreconstruct).
Qed.

Theorem parsed_partition_is_unique_and_deterministic : forall input left right,
  partition_absolute_datum input = Some left ->
  partition_absolute_datum input = Some right ->
  left = right.
Proof.
  intros input left right Hleft Hright.
  rewrite Hleft in Hright. now inversion Hright.
Qed.

Theorem datum_partition_returns_exact_marker_and_relative : forall
    channel_structure channel_identity datum_structure datum_identity,
  bytes_valid channel_structure = true ->
  bytes_valid channel_identity = true ->
  bytes_valid datum_structure = true ->
  bytes_valid datum_identity = true ->
  partition_absolute_datum
    (absolute_datum_key channel_structure channel_identity
      datum_structure datum_identity) =
  Some (DatumKeyPartitionState
    (datum_channel_marker channel_structure channel_identity)
    (relative_datum_key datum_structure datum_identity)).
Proof.
  intros channel_structure channel_identity datum_structure datum_identity
    Hchannel_structure Hchannel_identity Hdatum_structure Hdatum_identity.
  unfold partition_absolute_datum.
  rewrite datum_constructor_parser_round_trip by assumption.
  reflexivity.
Qed.

(** Consequently no two byte-valid source tuples can name the same absolute
    key while inducing different channel or relative partitions. *)
Theorem absolute_datum_key_is_injective : forall
    channel_structure_a channel_identity_a datum_structure_a datum_identity_a
    channel_structure_b channel_identity_b datum_structure_b datum_identity_b,
  bytes_valid channel_structure_a = true ->
  bytes_valid channel_identity_a = true ->
  bytes_valid datum_structure_a = true ->
  bytes_valid datum_identity_a = true ->
  bytes_valid channel_structure_b = true ->
  bytes_valid channel_identity_b = true ->
  bytes_valid datum_structure_b = true ->
  bytes_valid datum_identity_b = true ->
  absolute_datum_key channel_structure_a channel_identity_a
      datum_structure_a datum_identity_a =
    absolute_datum_key channel_structure_b channel_identity_b
      datum_structure_b datum_identity_b ->
  channel_structure_a = channel_structure_b /\
  channel_identity_a = channel_identity_b /\
  datum_structure_a = datum_structure_b /\
  datum_identity_a = datum_identity_b.
Proof.
  intros channel_structure_a channel_identity_a datum_structure_a datum_identity_a
    channel_structure_b channel_identity_b datum_structure_b datum_identity_b
    Hchannel_structure_a Hchannel_identity_a Hdatum_structure_a Hdatum_identity_a
    Hchannel_structure_b Hchannel_identity_b Hdatum_structure_b Hdatum_identity_b
    Hkeys.
  pose proof (@datum_constructor_parser_round_trip
    channel_structure_a channel_identity_a datum_structure_a datum_identity_a
    Hchannel_structure_a Hchannel_identity_a Hdatum_structure_a Hdatum_identity_a)
    as Hparse_a.
  pose proof (@datum_constructor_parser_round_trip
    channel_structure_b channel_identity_b datum_structure_b datum_identity_b
    Hchannel_structure_b Hchannel_identity_b Hdatum_structure_b Hdatum_identity_b)
    as Hparse_b.
  rewrite Hkeys in Hparse_a. rewrite Hparse_b in Hparse_a.
  now inversion Hparse_a.
Qed.

(** Executable causal controls ------------------------------------------ *)

Theorem frame_zero_escape_and_terminator_are_exact :
  encode_frame [0; 7; 0; 255; 0] =
    [0; 255; 7; 0; 255; 255; 0; 255; 0; 0].
Proof. reflexivity. Qed.

Theorem frame_with_arbitrary_zeros_parses_exactly :
  parse_frame
    ([0; 255; 7; 0; 255; 255; 0; 255; 0; 0] ++ [19; 0; 42]) =
  Some ([0; 7; 0; 255; 0], [19; 0; 42]).
Proof. vm_compute. reflexivity. Qed.

Theorem absolute_layout_has_three_frames_and_raw_identity :
  absolute_datum_key [0; 9] [8; 0] [0; 7] [0; 42] =
  [82; 72; 80; 1; 1;
   0; 255; 9; 0; 0;
   8; 0; 255; 0; 0;
   0; 255; 7; 0; 0;
   0; 42].
Proof. reflexivity. Qed.

Theorem syntactic_parser_accepts_empty_datum_identity :
  parse_absolute_datum
    (absolute_datum_key [0] [3] [0; 4] []) =
  Some (DatumPathViewState [0] [3] [0; 4] []).
Proof. vm_compute. reflexivity. Qed.

(** [[0;42]] is malformed inside any frame but is ordinary raw data after
    the third frame terminator. *)
Theorem raw_datum_identity_does_not_interpret_escape_pairs :
  parse_absolute_datum
    (absolute_datum_key [1] [2] [3] [0; 42]) =
  Some (DatumPathViewState [1] [2] [3] [0; 42]).
Proof. vm_compute. reflexivity. Qed.

Theorem checked_frame_refuses_out_of_range_before_output :
  encode_frame_checked [7; 256; 8] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem checked_datum_refuses_out_of_range_before_output :
  absolute_datum_key_checked [1] [2] [3] [256] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem malformed_magic_is_refused_before_view :
  parse_absolute_datum
    [81; 72; 80; 1; 1; 0; 0; 0; 0; 0; 0] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem malformed_kind_is_refused_before_view :
  parse_absolute_datum
    [82; 72; 80; 1; 2; 0; 0; 0; 0; 0; 0] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem malformed_escape_is_refused_before_view :
  parse_absolute_datum
    [82; 72; 80; 1; 1; 0; 42; 0; 0; 0; 0] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem truncated_frame_is_refused_before_view :
  parse_absolute_datum [82; 72; 80; 1; 1; 7] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem truncated_escape_is_refused_before_view :
  parse_absolute_datum [82; 72; 80; 1; 1; 0] = None.
Proof. vm_compute. reflexivity. Qed.

(** CI counts these reports and the kernel checker independently replays the
    compiled module. *)
Print Assumptions frame_encode_parse_round_trip.
Print Assumptions parse_frame_sound.
Print Assumptions datum_constructor_parser_round_trip.
Print Assumptions checked_datum_constructor_parser_round_trip.
Print Assumptions parse_absolute_datum_sound.
Print Assumptions datum_partition_reconstructs_absolute_key.
Print Assumptions parsed_partition_is_unique_and_deterministic.
Print Assumptions datum_partition_returns_exact_marker_and_relative.
Print Assumptions absolute_datum_key_is_injective.
Print Assumptions frame_zero_escape_and_terminator_are_exact.
Print Assumptions frame_with_arbitrary_zeros_parses_exactly.
Print Assumptions absolute_layout_has_three_frames_and_raw_identity.
Print Assumptions syntactic_parser_accepts_empty_datum_identity.
Print Assumptions raw_datum_identity_does_not_interpret_escape_pairs.
Print Assumptions checked_frame_refuses_out_of_range_before_output.
Print Assumptions checked_datum_refuses_out_of_range_before_output.
Print Assumptions malformed_magic_is_refused_before_view.
Print Assumptions malformed_kind_is_refused_before_view.
Print Assumptions malformed_escape_is_refused_before_view.
Print Assumptions truncated_frame_is_refused_before_view.
Print Assumptions truncated_escape_is_refused_before_view.

End RhoPathV1Datum.
