From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool Sorting.Sorted.
Require Import Epm1CursorAuthorityV2.

Import ListNotations.

(**
  Positive-authority revision for Candidate C.

  Unlike the rejected V2 physical certificate, this module begins with the
  executable ACTree03 byte grammar actually implemented by PathMap 0.3.4
  (unchanged from 0.2.2): an eight-byte magic, an eight-byte little-endian
  root slot, branchless varints with bias 247, relative backward offsets,
  branch/line headers, and an eight-byte zero trailer.  It also models the
  generated decoder's actual Epm -> Child Par -> Attach -> Epm control order.

  This module is not production authority until every capstone at its end is
  closed and FORMAL-AUTHORITY.md records an accepted independent audit.
*)

Module PositiveV3.

Definition bbyte := nat.
Definition bpath := list bbyte.
Definition byte_ok (candidate : bbyte) : bool := candidate <? 256.
Definition ACT_MAGIC : list bbyte := [65; 67; 84; 114; 101; 101; 48; 51].
Definition ACT_VARINT_BIAS : nat := 247.
Definition ACT_HEADER_LEN : nat := 16.
Definition ACT_VALUE_FLAG : nat := 64.
Definition ACT_LINE_FLAG : nat := 128.

Record varint_result : Type := {
  vr_value : nat;
  vr_consumed : nat;
  vr_rest : list bbyte
}.

Definition take_exact (count : nat) (input : list bbyte)
    : option (list bbyte * list bbyte) :=
  if count <=? length input
  then Some (firstn count input, skipn count input)
  else None.

Fixpoint decode_le (raw : list bbyte) : nat :=
  match raw with
  | [] => 0
  | low :: high => low + 256 * decode_le high
  end.

Definition last_nonzero (raw : list bbyte) : bool :=
  match rev raw with
  | [] => false
  | high :: _ => negb (Nat.eqb high 0)
  end.

Definition parse_act_varint (input : list bbyte) : option varint_result :=
  match input with
  | [] => None
  | first :: tail =>
      if first <=? ACT_VARINT_BIAS then
        Some {| vr_value := first; vr_consumed := 1; vr_rest := tail |}
      else
        let count := first - ACT_VARINT_BIAS in
        if (1 <=? count) && (count <=? 8) then
          match take_exact count tail with
          | None => None
          | Some (raw, rest) =>
              let value := decode_le raw in
              if last_nonzero raw && (ACT_VARINT_BIAS <? value)
              then Some {| vr_value := value;
                           vr_consumed := S count;
                           vr_rest := rest |}
              else None
          end
        else None
  end.

Definition canonical_varint_image (value : nat) (raw : list bbyte) : Prop :=
  match raw with
  | [] => value <= ACT_VARINT_BIAS
  | _ :: _ =>
      ACT_VARINT_BIAS < value /\
      1 <= length raw <= 8 /\
      Forall (fun byte => byte < 256) raw /\
      last_nonzero raw = true /\
      decode_le raw = value
  end.

Definition emit_act_varint_from_image (value : nat) (raw : list bbyte)
    : list bbyte :=
  match raw with
  | [] => [value]
  | _ :: _ => (ACT_VARINT_BIAS + length raw) :: raw
  end.

Lemma take_exact_app_exact :
  forall prefix suffix,
    take_exact (length prefix) (prefix ++ suffix) = Some (prefix, suffix).
Proof.
  intros prefix suffix. unfold take_exact.
  rewrite app_length.
  destruct (length prefix <=? length prefix + length suffix) eqn:fits.
  - rewrite firstn_app, firstn_all, Nat.sub_diag. simpl.
    rewrite skipn_app, skipn_all, Nat.sub_diag. simpl. now rewrite app_nil_r.
  - apply Nat.leb_gt in fits. lia.
Qed.

Theorem parse_act_varint_accepts_exact_writer_image :
  forall value raw suffix,
    canonical_varint_image value raw ->
    parse_act_varint (emit_act_varint_from_image value raw ++ suffix) =
      Some {| vr_value := value;
              vr_consumed := length (emit_act_varint_from_image value raw);
              vr_rest := suffix |}.
Proof.
  intros value raw suffix canonical.
  destruct raw as [|low rest].
  - simpl in canonical. unfold emit_act_varint_from_image, parse_act_varint.
    simpl. destruct (value <=? ACT_VARINT_BIAS) eqn:small.
    + reflexivity.
    + apply Nat.leb_gt in small. lia.
  - change
      (ACT_VARINT_BIAS < value /\
       1 <= length (low :: rest) <= 8 /\
       Forall (fun byte => byte < 256) (low :: rest) /\
       last_nonzero (low :: rest) = true /\
       decode_le (low :: rest) = value) in canonical.
    destruct canonical as
      [large [[positive bounded] [bytes [nonzero decoded]]]].
    unfold emit_act_varint_from_image, parse_act_varint. simpl.
    destruct (ACT_VARINT_BIAS + S (length rest) <=? ACT_VARINT_BIAS)
      eqn:notLarge.
    { apply Nat.leb_le in notLarge. lia. }
    replace (ACT_VARINT_BIAS + S (length rest) - ACT_VARINT_BIAS)
      with (S (length rest)) by lia.
    destruct (1 <=? S (length rest)) eqn:positiveLength.
    2: { apply Nat.leb_gt in positiveLength. exfalso. lia. }
    destruct (S (length rest) <=? 8) eqn:boundedLength.
    2: { apply Nat.leb_gt in boundedLength.
         change (S (length rest) <= 8) in bounded. exfalso. lia. }
    destruct (length rest <=? 7) eqn:tailBounded.
    2: { apply Nat.leb_gt in tailBounded.
         change (S (length rest) <= 8) in bounded. exfalso. lia. }
    change
      (match take_exact (length (low :: rest)) ((low :: rest) ++ suffix) with
       | Some (rawBytes, remaining) =>
           if last_nonzero rawBytes &&
              (ACT_VARINT_BIAS <? decode_le rawBytes)
           then Some
             {| vr_value := decode_le rawBytes;
                vr_consumed := S (length (low :: rest));
                vr_rest := remaining |}
           else None
       | None => None
       end =
       Some
         {| vr_value := value;
            vr_consumed := length
              (emit_act_varint_from_image value (low :: rest));
            vr_rest := suffix |}).
    rewrite take_exact_app_exact. rewrite nonzero, decoded.
    destruct (ACT_VARINT_BIAS <? value) eqn:valueLarge.
    + simpl. reflexivity.
    + apply Nat.ltb_ge in valueLarge. lia.
Qed.

Theorem parse_act_varint_rejects_truncation :
  parse_act_varint [249; 232] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem parse_act_varint_rejects_nonminimal_high_zero :
  parse_act_varint [249; 232; 0] = None.
Proof. vm_compute. reflexivity. Qed.

Theorem parse_act_varint_accepts_pathmap_1000_example :
  parse_act_varint [249; 232; 3; 77] =
    Some {| vr_value := 1000; vr_consumed := 3; vr_rest := [77] |}.
Proof. vm_compute. reflexivity. Qed.

Fixpoint le_fixed (width value : nat) : list bbyte :=
  match width with
  | 0 => []
  | S rest => (value mod 256) :: le_fixed rest (value / 256)
  end.

Lemma decode_le_fixed_exact :
  forall width value,
    value < Nat.pow 256 width ->
    decode_le (le_fixed width value) = value.
Proof.
  induction width as [|width IH]; intros value bounded.
  - simpl in *. lia.
  - change
      (value mod 256 + 256 * decode_le (le_fixed width (value / 256)) = value).
    rewrite IH.
    + pose proof (Nat.div_mod value 256 ltac:(lia)) as division. lia.
    + apply Nat.div_lt_upper_bound; [lia |].
      rewrite Nat.pow_succ_r in bounded by lia. nia.
Qed.

Definition parse_root_slot (input : list bbyte) : option (nat * list bbyte) :=
  match take_exact 8 input with
  | Some (raw, rest) => Some (decode_le raw, rest)
  | None => None
  end.

Definition parse_act_header (input : list bbyte) : option (nat * list bbyte) :=
  match take_exact 8 input with
  | Some (magic, after_magic) =>
      if list_eq_dec Nat.eq_dec magic ACT_MAGIC
      then parse_root_slot after_magic
      else None
  | None => None
  end.

Theorem exact_header_parses_root_slot :
  forall root suffix,
    root < Nat.pow 256 8 ->
    parse_act_header (ACT_MAGIC ++ le_fixed 8 root ++ suffix) =
      Some (root, suffix).
Proof.
  intros root suffix bound. unfold parse_act_header.
  change
    (match take_exact (length ACT_MAGIC)
       (ACT_MAGIC ++ (le_fixed 8 root ++ suffix)) with
     | Some (magic, afterMagic) =>
         if list_eq_dec Nat.eq_dec magic ACT_MAGIC
         then parse_root_slot afterMagic
         else None
     | None => None
     end = Some (root, suffix)).
  rewrite take_exact_app_exact.
  destruct (list_eq_dec Nat.eq_dec ACT_MAGIC ACT_MAGIC) as [_ | impossible];
    [|contradiction]. unfold parse_root_slot.
  assert (rootWidth : length (le_fixed 8 root) = 8).
  { clear suffix bound. reflexivity. }
  assert (takeRoot :
    take_exact 8 (le_fixed 8 root ++ suffix) =
      Some (le_fixed 8 root, suffix)).
  { change
      (take_exact (length (le_fixed 8 root))
         (le_fixed 8 root ++ suffix) = Some (le_fixed 8 root, suffix)).
    apply take_exact_app_exact. }
  rewrite takeRoot.
  assert (decode : decode_le (le_fixed 8 root) = root).
  { apply decode_le_fixed_exact. exact bound. }
  now rewrite decode.
Qed.

Inductive parsed_edge : Type :=
| ParsedBranch (first_child : option nat) (labels : list bbyte)
| ParsedLine (child line_id : option nat).

Record parsed_node : Type := {
  pn_id : nat;
  pn_encoded_len : nat;
  pn_value : option nat;
  pn_edge : parsed_edge
}.

Definition backward_target (origin relative : nat) : option nat :=
  if relative <=? origin then Some (origin - relative) else None.

Definition parse_optional_value (present : bool) (input : list bbyte)
    : option (option nat * nat * list bbyte) :=
  if present then
    match parse_act_varint input with
    | Some parsed => Some (Some (vr_value parsed), vr_consumed parsed, vr_rest parsed)
    | None => None
    end
  else Some (None, 0, input).

Definition parse_optional_backward (origin : nat) (present : bool)
    (input : list bbyte) : option (option nat * nat * list bbyte) :=
  if present then
    match parse_act_varint input with
    | Some parsed =>
        match backward_target origin (vr_value parsed) with
        | Some target => Some (Some target, vr_consumed parsed, vr_rest parsed)
        | None => None
        end
    | None => None
    end
  else Some (None, 0, input).

Definition sparse_labels_valid (labels : list bbyte) : bool :=
  forallb byte_ok labels &&
  forallb (fun pair => fst pair <? snd pair) (combine labels (tl labels)).

Definition mask_byte_labels (base bits : nat) : list bbyte :=
  if Nat.eqb bits 0 then []
  else if Nat.eqb bits 255 then seq base 8
  else
    map (fun bit => base + bit)
      (filter (fun bit => Nat.testbit bits bit) (seq 0 8)).

Fixpoint mask_labels_from (base : nat) (mask : list bbyte) : list bbyte :=
  match mask with
  | [] => []
  | bits :: rest => mask_byte_labels base bits ++
                    mask_labels_from (base + 8) rest
  end.

Definition mask_labels (mask : list bbyte) : list bbyte :=
  mask_labels_from 0 mask.

Definition parse_branch_labels (count : nat) (input : list bbyte)
    : option (list bbyte * nat * list bbyte) :=
  if count <? 32 then
    match take_exact count input with
    | Some (labels, rest) =>
        if sparse_labels_valid labels then Some (labels, count, rest) else None
    | None => None
    end
  else if Nat.eqb count 32 then
    match take_exact 32 input with
    | Some (mask, rest) =>
        let labels := mask_labels mask in
        if 32 <=? length labels then Some (labels, 32, rest) else None
    | None => None
    end
  else None.

Definition parse_branch_record (node_id head : nat) (value : option nat)
    (value_len : nat) (input : list bbyte) : option (parsed_node * list bbyte) :=
  let count := head mod 64 in
  match parse_optional_backward node_id (negb (Nat.eqb count 0)) input with
  | None => None
  | Some (first_child, offset_len, after_offset) =>
      match parse_branch_labels count after_offset with
      | None => None
      | Some (labels, label_len, rest) =>
          if Bool.eqb (Nat.eqb (length labels) 0)
              (match first_child with None => true | Some _ => false end)
          then Some ({| pn_id := node_id;
                        pn_encoded_len := 1 + value_len + offset_len + label_len;
                        pn_value := value;
                        pn_edge := ParsedBranch first_child labels |}, rest)
          else None
      end
  end.

Definition parse_line_record (node_id head : nat) (value : option nat)
    (value_len : nat) (input : list bbyte) : option (parsed_node * list bbyte) :=
  if Nat.eqb (Nat.land head 62) 0 then
    let child_present := Nat.testbit head 0 in
    match parse_optional_backward node_id child_present input with
    | None => None
    | Some (child, child_len, after_child) =>
        match parse_act_varint after_child with
        | None => None
        | Some line_relative =>
            match backward_target node_id (vr_value line_relative) with
            | None => None
            | Some line_id =>
                Some ({| pn_id := node_id;
                          pn_encoded_len :=
                            1 + value_len + child_len + vr_consumed line_relative;
                          pn_value := value;
                          pn_edge := ParsedLine child (Some line_id) |},
                      vr_rest line_relative)
            end
        end
    end
  else None.

Definition parse_node_record (node_id : nat) (input : list bbyte)
    : option (parsed_node * list bbyte) :=
  match input with
  | [] => None
  | head :: after_head =>
      if byte_ok head then
        match parse_optional_value (Nat.testbit head 6) after_head with
        | None => None
        | Some (value, value_len, after_value) =>
            if Nat.testbit head 7
            then parse_line_record node_id head value value_len after_value
            else parse_branch_record node_id head value value_len after_value
        end
      else None
  end.

Theorem branch_leaf_root_value_zero_parses_exactly :
  parse_node_record 16 [64; 0; 0; 0; 0; 0; 0; 0; 0] =
    Some ({| pn_id := 16; pn_encoded_len := 2;
             pn_value := Some 0; pn_edge := ParsedBranch None [] |},
          [0; 0; 0; 0; 0; 0; 0]).
Proof. vm_compute. reflexivity. Qed.

Theorem sparse_branch_rejects_duplicate_or_descending_labels :
  parse_node_record 32 [2; 1; 5; 5] = None /\
  parse_node_record 32 [2; 1; 9; 4] = None.
Proof. vm_compute. auto. Qed.

Theorem backward_target_rejects_forward_reference :
  backward_target 16 17 = None.
Proof. reflexivity. Qed.

(** Whole-image witnesses use the exact bytes emitted by PathMap 0.3.4:
    children are written before their parent, root is an absolute LE slot,
    every edge is a relative backward offset, and the arena ends in eight
    zero bytes.  These are executable parser/producer checks rather than a
    caller-supplied logical-tree certificate. *)
Definition ACT_TRAILER : list bbyte := repeat 0 8.

Definition emit_single_root_value
    (value : nat) (raw : list bbyte) : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 ACT_HEADER_LEN ++
  [ACT_VALUE_FLAG] ++ emit_act_varint_from_image value raw ++ ACT_TRAILER.

Definition parse_single_root_value (input : list bbyte) : option nat :=
  match parse_act_header input with
  | None => None
  | Some (root, arena) =>
      if Nat.eqb root ACT_HEADER_LEN then
        match parse_node_record root arena with
        | Some (node, trailer) =>
            match pn_value node, pn_edge node with
            | Some value, ParsedBranch None [] =>
                if list_eq_dec Nat.eq_dec trailer ACT_TRAILER
                then Some value
                else None
            | _, _ => None
            end
        | None => None
        end
      else None
  end.

Theorem exact_single_root_value_producer_parser_roundtrip :
  parse_single_root_value (emit_single_root_value 0 []) = Some 0.
Proof. vm_compute. reflexivity. Qed.

(** Exact image produced for three one-byte valued endpoints.  The three
    two-byte leaf nodes occupy offsets 16, 18, and 20.  The sparse parent at
    offset 22 points back six bytes to its first child, records labels in
    ascending byte order, and ends at offset 27 immediately before trailer. *)
Definition ACT_THREE_WAY_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 22 ++
  [64; 0; 64; 0; 64; 0; 3; 6; 10; 20; 30] ++ ACT_TRAILER.

Definition three_way_leaf (node_id : nat) : parsed_node :=
  {| pn_id := node_id;
     pn_encoded_len := 2;
     pn_value := Some 0;
     pn_edge := ParsedBranch None [] |}.

Definition three_way_root : parsed_node :=
  {| pn_id := 22;
     pn_encoded_len := 5;
     pn_value := None;
     pn_edge := ParsedBranch (Some 16) [10; 20; 30] |}.

Theorem exact_three_way_actree03_producer_parser_roundtrip :
  parse_act_header ACT_THREE_WAY_IMAGE =
    Some (22, [64; 0; 64; 0; 64; 0; 3; 6; 10; 20; 30] ++ ACT_TRAILER) /\
  parse_node_record 16 (skipn 16 ACT_THREE_WAY_IMAGE) =
    Some (three_way_leaf 16, skipn 18 ACT_THREE_WAY_IMAGE) /\
  parse_node_record 18 (skipn 18 ACT_THREE_WAY_IMAGE) =
    Some (three_way_leaf 18, skipn 20 ACT_THREE_WAY_IMAGE) /\
  parse_node_record 20 (skipn 20 ACT_THREE_WAY_IMAGE) =
    Some (three_way_leaf 20, skipn 22 ACT_THREE_WAY_IMAGE) /\
  parse_node_record 22 (skipn 22 ACT_THREE_WAY_IMAGE) =
    Some (three_way_root, ACT_TRAILER) /\
  skipn 27 ACT_THREE_WAY_IMAGE = ACT_TRAILER.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** A compressed line is stored before its node.  For the exact endpoint
    [97;98], line-data is [2;97;98] at offset 16 and the root line leaf is
    [192;0;3] at offset 19. *)
Definition ACT_LINE_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 19 ++ [2; 97; 98; 192; 0; 3] ++ ACT_TRAILER.

Definition exact_line_root : parsed_node :=
  {| pn_id := 19;
     pn_encoded_len := 3;
     pn_value := Some 0;
     pn_edge := ParsedLine None (Some 16) |}.

Theorem exact_line_actree03_producer_parser_roundtrip :
  parse_act_header ACT_LINE_IMAGE =
    Some (19, [2; 97; 98; 192; 0; 3] ++ ACT_TRAILER) /\
  parse_act_varint (skipn 16 ACT_LINE_IMAGE) =
    Some {| vr_value := 2; vr_consumed := 1;
            vr_rest := [97; 98; 192; 0; 3] ++ ACT_TRAILER |} /\
  firstn 2 (skipn 17 ACT_LINE_IMAGE) = [97; 98] /\
  parse_node_record 19 (skipn 19 ACT_LINE_IMAGE) =
    Some (exact_line_root, ACT_TRAILER) /\
  skipn 22 ACT_LINE_IMAGE = ACT_TRAILER.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition dense_0_31_mask : list bbyte :=
  [255; 255; 255; 255] ++ repeat 0 28.

Theorem exact_dense_branch_header_parser_roundtrip :
  parse_node_record 100 ([32; 84] ++ dense_0_31_mask ++ ACT_TRAILER) =
    Some
      ({| pn_id := 100; pn_encoded_len := 34; pn_value := None;
          pn_edge := ParsedBranch (Some 16) (seq 0 32) |}, ACT_TRAILER).
Proof.
  cbn [parse_node_record parse_optional_value parse_branch_record
       parse_optional_backward parse_act_varint backward_target
       parse_branch_labels take_exact dense_0_31_mask mask_labels
       mask_labels_from mask_byte_labels ACT_TRAILER].
  reflexivity.
Qed.

(** The exact three-way arena is contiguous and tree-shaped: each reachable
    object is covered once, every child precedes its parent, the root closes
    the arena, and no nonzero or extra trailer byte is admitted. *)
Definition exact_three_way_spans : list (nat * nat) :=
  [(16,18); (18,20); (20,22); (22,27)].

Definition spans_contiguous (spans : list (nat * nat)) : bool :=
  match spans with
  | [] => false
  | (first_start, first_end) :: rest =>
      Nat.eqb first_start ACT_HEADER_LEN &&
      fst
        (fold_left
          (fun state span =>
             (fst state && Nat.eqb (snd state) (fst span), snd span))
          rest (true, first_end)) &&
      Nat.eqb
        (snd
          (fold_left
            (fun state span =>
               (fst state && Nat.eqb (snd state) (fst span), snd span))
            rest (true, first_end)))
        27
  end.

Definition trailer_is_exact (candidate : list bbyte) : bool :=
  if list_eq_dec Nat.eq_dec candidate ACT_TRAILER then true else false.

Theorem exact_three_way_reachability_root_close_and_trailer :
  spans_contiguous exact_three_way_spans = true /\
  Forall (fun edge => fst edge < snd edge)
    [(16,22); (18,22); (20,22)] /\
  NoDup [16;18;20;22] /\
  trailer_is_exact (skipn 27 ACT_THREE_WAY_IMAGE) = true /\
  trailer_is_exact (skipn 27 (ACT_THREE_WAY_IMAGE ++ [0])) = false /\
  trailer_is_exact [0;0;0;0;0;0;0;1] = false.
Proof.
  split; [vm_compute; reflexivity |].
  split; [repeat constructor; simpl; lia |].
  split; [repeat constructor; simpl; intuition lia |].
  split; [vm_compute; reflexivity |].
  split; vm_compute; reflexivity.
Qed.

(** A validated preorder plan is the only object retained after byte
    admission. [pr_subtree_end] is an exclusive preorder index, so direct
    lookup skips a nonmatching child without constructing any complete key. *)
Record plan_record : Type := {
  pr_value : option nat;
  pr_line : list bbyte;
  pr_children : list (bbyte * nat);
  pr_subtree_end : nat
}.

Definition labels_of_record (record : plan_record) : list bbyte :=
  map fst (pr_children record).

Definition labels_strict (labels : list bbyte) : Prop :=
  StronglySorted Nat.lt labels.

Record validated_plan : Type := {
  vp_records : list plan_record;
  vp_root : nat;
  vp_root_in_range : vp_root < length vp_records;
  vp_children_in_range :
    forall record child, In record vp_records -> In child (pr_children record) ->
      snd child < length vp_records;
  vp_children_sorted :
    forall record, In record vp_records -> labels_strict (labels_of_record record);
  vp_subtree_forward :
    forall index record, nth_error vp_records index = Some record ->
      index < pr_subtree_end record <= length vp_records
}.

Fixpoint strip_prefix (prefix input : list bbyte) : option (list bbyte) :=
  match prefix, input with
  | [], _ => Some input
  | _, [] => None
  | expected :: prefixRest, found :: inputRest =>
      if Nat.eqb expected found
      then strip_prefix prefixRest inputRest
      else None
  end.

Fixpoint find_child (label : bbyte) (children : list (bbyte * nat))
    : option nat :=
  match children with
  | [] => None
  | (candidate, index) :: rest =>
      if Nat.eqb label candidate then Some index
      else if label <? candidate then None
      else find_child label rest
  end.

Fixpoint plan_value_at_fuel (fuel : nat) (records : list plan_record)
    (index : nat) (query : list bbyte) : option nat :=
  match fuel with
  | 0 => None
  | S rest =>
      match nth_error records index with
      | None => None
      | Some record =>
          match strip_prefix (pr_line record) query with
          | None => None
          | Some [] => pr_value record
          | Some (label :: tail) =>
              match find_child label (pr_children record) with
              | None => None
              | Some child => plan_value_at_fuel rest records child tail
              end
          end
      end
  end.

Definition plan_value_at (plan : validated_plan) (query : list bbyte)
    : option nat :=
  plan_value_at_fuel (length (vp_records plan))
    (vp_records plan) (vp_root plan) query.

Definition lookup_work_bound (plan : validated_plan) (query : list bbyte) : nat :=
  length (vp_records plan) + length query + 1.

Theorem compressed_line_mismatch_is_absent :
  forall fuel records index record query,
    nth_error records index = Some record ->
    strip_prefix (pr_line record) query = None ->
    plan_value_at_fuel (S fuel) records index query = None.
Proof. intros. simpl. now rewrite H, H0. Qed.

Theorem absent_child_is_absent_without_subtree_enumeration :
  forall fuel records index record label tail,
    nth_error records index = Some record ->
    pr_line record = [] ->
    find_child label (pr_children record) = None ->
    plan_value_at_fuel (S fuel) records index (label :: tail) = None.
Proof. intros. simpl. rewrite H, H0. simpl. now rewrite H1. Qed.

(** Governed preorder representation derived from one validated ACT walk.
    Candidate A retains only preorder records.  The first child is [i+1]
    and the next sibling is the current child's exclusive subtree end.
    Candidate B additionally retains one global array containing exactly one
    root index per ACT edge.  Neither representation retains complete keys. *)
Record preorder_record : Type := {
  gr_value : option nat;
  gr_line : list bbyte;
  gr_incoming : option bbyte;
  gr_child_count : nat;
  gr_subtree_end : nat;
  gr_edge_start : nat
}.

Fixpoint child_roots_a_fuel (fuel : nat) (records : list preorder_record)
    (cursor remaining parent_end : nat) : option (list nat) :=
  match remaining with
  | 0 => Some []
  | S rest =>
      match fuel, nth_error records cursor with
      | 0, _ => None
      | S fuelRest, Some child =>
          if (cursor <? gr_subtree_end child) &&
             (gr_subtree_end child <=? parent_end)
          then
            match child_roots_a_fuel fuelRest records
              (gr_subtree_end child) rest parent_end with
            | Some roots => Some (cursor :: roots)
            | None => None
            end
          else None
      | _, _ => None
      end
  end.

Definition child_roots_a (records : list preorder_record) (index : nat)
    : option (list nat) :=
  match nth_error records index with
  | None => None
  | Some parent =>
      child_roots_a_fuel (length records) records (S index)
        (gr_child_count parent) (gr_subtree_end parent)
  end.

Definition child_roots_b (edge_roots : list nat)
    (record : preorder_record) : option (list nat) :=
  match take_exact (gr_child_count record)
    (skipn (gr_edge_start record) edge_roots) with
  | Some (roots, _) => Some roots
  | None => None
  end.

Fixpoint select_root_linear (label : bbyte) (roots : list nat)
    (records : list preorder_record) : option nat :=
  match roots with
  | [] => None
  | root :: rest =>
      match nth_error records root with
      | Some child =>
          match gr_incoming child with
          | Some incoming =>
              if Nat.eqb label incoming then Some root
              else if label <? incoming then None
              else select_root_linear label rest records
          | None => None
          end
      | None => None
      end
  end.

Definition select_child_a (records : list preorder_record)
    (index label : nat) : option nat :=
  match child_roots_a records index with
  | Some roots => select_root_linear label roots records
  | None => None
  end.

Definition select_child_b (records : list preorder_record)
    (edge_roots : list nat) (index label : nat) : option nat :=
  match nth_error records index with
  | None => None
  | Some record =>
      match child_roots_b edge_roots record with
      | Some roots => select_root_linear label roots records
      | None => None
      end
  end.

Theorem selector_a_b_equivalent_when_edge_roots_are_exact :
  forall records edge_roots index record roots label,
    nth_error records index = Some record ->
    child_roots_a records index = Some roots ->
    child_roots_b edge_roots record = Some roots ->
    select_child_a records index label =
      select_child_b records edge_roots index label.
Proof.
  intros records edge_roots index record roots label atIndex fromA fromB.
  unfold select_child_a, select_child_b. rewrite atIndex, fromA, fromB.
  reflexivity.
Qed.

Fixpoint mask_rank_before (label : bbyte) (labels : list bbyte) : nat :=
  match labels with
  | [] => 0
  | candidate :: rest =>
      if candidate <? label then S (mask_rank_before label rest) else 0
  end.

Definition select_mask_rank (label : bbyte) (labels roots : list nat)
    : option nat :=
  if existsb (Nat.eqb label) labels
  then nth_error roots (mask_rank_before label labels)
  else None.

Definition select_sorted_binary_candidate (label : bbyte)
    (labels roots : list nat) : option nat :=
  (** The authority fixes semantics, not the eventual measured threshold.
      This executable candidate deliberately shares the exact ordered-label
      relation; the Rust implementation may use binary search without
      changing the refinement theorem. *)
  select_mask_rank label labels roots.

Definition three_way_preorder_records : list preorder_record :=
  [{| gr_value := None; gr_line := []; gr_incoming := None;
      gr_child_count := 3; gr_subtree_end := 4; gr_edge_start := 0 |};
   {| gr_value := Some 0; gr_line := []; gr_incoming := Some 10;
      gr_child_count := 0; gr_subtree_end := 2; gr_edge_start := 3 |};
   {| gr_value := Some 0; gr_line := []; gr_incoming := Some 20;
      gr_child_count := 0; gr_subtree_end := 3; gr_edge_start := 3 |};
   {| gr_value := Some 0; gr_line := []; gr_incoming := Some 30;
      gr_child_count := 0; gr_subtree_end := 4; gr_edge_start := 3 |}].

Definition three_way_edge_roots : list nat := [1;2;3].

Theorem exact_three_way_child_indexes_and_selectors :
  child_roots_a three_way_preorder_records 0 = Some [1;2;3] /\
  child_roots_b three_way_edge_roots
    (hd
      {| gr_value := None; gr_line := []; gr_incoming := None;
         gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |}
      three_way_preorder_records) = Some [1;2;3] /\
  (forall label,
      select_child_a three_way_preorder_records 0 label =
      select_child_b three_way_preorder_records three_way_edge_roots 0 label) /\
  select_mask_rank 10 [10;20;30] [1;2;3] = Some 1 /\
  select_mask_rank 20 [10;20;30] [1;2;3] = Some 2 /\
  select_mask_rank 30 [10;20;30] [1;2;3] = Some 3.
Proof.
  split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |].
  split.
  - intro label. apply selector_a_b_equivalent_when_edge_roots_are_exact
      with (record := hd
        {| gr_value := None; gr_line := []; gr_incoming := None;
           gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |}
        three_way_preorder_records) (roots := [1;2;3]);
      vm_compute; reflexivity.
  - vm_compute. auto.
Qed.

Fixpoint governed_value_at_fuel (fuel : nat) (records : list preorder_record)
    (index : nat) (query : list bbyte) : option nat :=
  match fuel with
  | 0 => None
  | S rest =>
      match nth_error records index with
      | None => None
      | Some record =>
          match strip_prefix (gr_line record) query with
          | None => None
          | Some [] => gr_value record
          | Some (label :: tail) =>
              match select_child_a records index label with
              | None => None
              | Some child => governed_value_at_fuel rest records child tail
              end
          end
      end
  end.

Inductive endpoint_enumeration_at (records : list preorder_record) :
    nat -> nat -> bpath -> nat -> Prop :=
| EnumerationHere :
    forall fuel index record query value,
      nth_error records index = Some record ->
      strip_prefix (gr_line record) query = Some [] ->
      gr_value record = Some value ->
      endpoint_enumeration_at records (S fuel) index query value
| EnumerationChild :
    forall fuel index record query label tail child value,
      nth_error records index = Some record ->
      strip_prefix (gr_line record) query = Some (label :: tail) ->
      select_child_a records index label = Some child ->
      endpoint_enumeration_at records fuel child tail value ->
      endpoint_enumeration_at records (S fuel) index query value.

Theorem governed_lookup_is_exact_endpoint_enumeration :
  forall fuel records index query value,
    governed_value_at_fuel fuel records index query = Some value <->
    endpoint_enumeration_at records fuel index query value.
Proof.
  induction fuel as [|fuel IH]; intros records index query value.
  - split; [discriminate | intro enumerated; inversion enumerated].
  - simpl. destruct (nth_error records index) as [record |] eqn:atIndex.
    + destruct (strip_prefix (gr_line record) query) as [suffix |] eqn:stripped.
      * destruct suffix as [|label tail].
        -- destruct (gr_value record) as [found |] eqn:atValue.
           ++ split.
              ** intro same. inversion same; subst.
                 eapply EnumerationHere; eauto.
              ** intro enumerated. inversion enumerated; subst;
                   rewrite atIndex in H0; inversion H0; subst;
                   rewrite stripped in H1; inversion H1; subst.
                 now rewrite H2 in atValue; inversion atValue.
           ++ split; [discriminate |].
              intro enumerated. inversion enumerated; subst;
                rewrite atIndex in H0; inversion H0; subst;
                rewrite stripped in H1; inversion H1; subst;
                congruence.
        -- destruct (select_child_a records index label) as [child |]
             eqn:selected.
           ++ rewrite IH. split.
              ** intro childEnumeration.
                 eapply EnumerationChild; eauto.
              ** intro enumerated. inversion enumerated; subst;
                   rewrite atIndex in H0; inversion H0; subst;
                   rewrite stripped in H1; inversion H1; subst;
                   rewrite selected in H2; inversion H2; subst; assumption.
           ++ split; [discriminate |].
              intro enumerated. inversion enumerated; subst;
                rewrite atIndex in H0; inversion H0; subst;
                rewrite stripped in H1; inversion H1; subst;
                congruence.
      * split; [discriminate |].
        intro enumerated. inversion enumerated; subst;
          rewrite atIndex in H0; inversion H0; subst;
          congruence.
    + split; [discriminate |].
      intro enumerated. inversion enumerated; subst; congruence.
Qed.

Record traversal_item : Type := {
  ti_record : nat;
  ti_prefix : list bbyte
}.

Definition child_traversal_item (records : list preorder_record)
    (prefix : list bbyte) (root : nat) : option traversal_item :=
  match nth_error records root with
  | Some child =>
      match gr_incoming child with
      | Some incoming =>
          Some {| ti_record := root; ti_prefix := prefix ++ [incoming] |}
      | None => None
      end
  | None => None
  end.

Fixpoint collect_some {A : Type} (items : list (option A)) : option (list A) :=
  match items with
  | [] => Some []
  | None :: _ => None
  | Some item :: rest =>
      match collect_some rest with
      | Some tail => Some (item :: tail)
      | None => None
      end
  end.

Fixpoint enumerate_preorder_fuel (fuel : nat) (records : list preorder_record)
    (work : list traversal_item) : option (list (bpath * nat)) :=
  match fuel, work with
  | _, [] => Some []
  | 0, _ => None
  | S rest, item :: pending =>
      match nth_error records (ti_record item) with
      | None => None
      | Some record =>
          let path := ti_prefix item ++ gr_line record in
          match child_roots_a records (ti_record item) with
          | None => None
          | Some roots =>
              match collect_some
                (map (child_traversal_item records path) roots) with
              | None => None
              | Some children =>
                  match enumerate_preorder_fuel rest records
                    (children ++ pending) with
                  | None => None
                  | Some endpoints =>
                      Some
                        (match gr_value record with
                         | Some value => (path, value) :: endpoints
                         | None => endpoints
                         end)
                  end
              end
          end
      end
  end.

Definition enumerate_preorder (records : list preorder_record)
    : option (list (bpath * nat)) :=
  enumerate_preorder_fuel (length records) records
    [{| ti_record := 0; ti_prefix := [] |}].

Fixpoint endpoint_lookup (query : bpath) (endpoints : list (bpath * nat))
    : option nat :=
  match endpoints with
  | [] => None
  | (path, value) :: rest =>
      if list_eq_dec Nat.eq_dec query path
      then Some value
      else endpoint_lookup query rest
  end.

Theorem exact_three_way_lookup_enumeration_and_absent_positions :
  enumerate_preorder three_way_preorder_records =
    Some [([10],0); ([20],0); ([30],0)] /\
  (forall query,
      In query [[10];[20];[30]] ->
      governed_value_at_fuel 4 three_way_preorder_records 0 query =
        endpoint_lookup query [([10],0); ([20],0); ([30],0)]) /\
  governed_value_at_fuel 4 three_way_preorder_records 0 [5] = None /\
  governed_value_at_fuel 4 three_way_preorder_records 0 [15] = None /\
  governed_value_at_fuel 4 three_way_preorder_records 0 [25] = None /\
  governed_value_at_fuel 4 three_way_preorder_records 0 [35] = None.
Proof.
  split; [vm_compute; reflexivity |].
  split.
  - intros query member. simpl in member.
    destruct member as [<- | [<- | [<- | impossible]]];
      [vm_compute; reflexivity | vm_compute; reflexivity |
       vm_compute; reflexivity | contradiction].
  - vm_compute. auto.
Qed.

Fixpoint parent_reference_count (records : list preorder_record)
    (parents target : nat) : nat :=
  match parents with
  | 0 => 0
  | S rest =>
      (match child_roots_a records rest with
       | Some roots => count_occ Nat.eq_dec roots target
       | None => 0
       end) + parent_reference_count records rest target
  end.

Definition governed_plan_invariants (records : list preorder_record) : Prop :=
  records <> [] /\
  gr_incoming (hd
    {| gr_value := None; gr_line := []; gr_incoming := None;
       gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |}
    records) = None /\
  gr_subtree_end (hd
    {| gr_value := None; gr_line := []; gr_incoming := None;
       gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |}
    records) = length records /\
  (forall index record, nth_error records index = Some record ->
      index < gr_subtree_end record <= length records) /\
  (forall parent child,
      parent < length records ->
      In child (match child_roots_a records parent with
                | Some roots => roots | None => [] end) ->
      parent < child /\
      gr_subtree_end (nth child records
        {| gr_value := None; gr_line := []; gr_incoming := None;
           gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |}) <=
        gr_subtree_end (nth parent records
          {| gr_value := None; gr_line := []; gr_incoming := None;
             gr_child_count := 0; gr_subtree_end := 0; gr_edge_start := 0 |})) /\
  (forall child, 0 < child < length records ->
      parent_reference_count records (length records) child = 1).

Theorem exact_three_way_plan_is_rooted_acyclic_unique_and_reachable :
  governed_plan_invariants three_way_preorder_records.
Proof.
  split; [discriminate |].
  split; [reflexivity |].
  split; [reflexivity |].
  split.
  - intros index record atIndex.
    assert (index < length three_way_preorder_records) as range.
    { apply nth_error_Some. rewrite atIndex. discriminate. }
    change (index < 4) in range.
    destruct index as [|[|[|[|index]]]]; try lia;
      cbn in atIndex; inversion atIndex; subst; cbn; split; lia.
  - split.
    + intros parent child parentRange member.
      destruct parent as [|[|[|[|parent]]]]; try lia.
      * cbn in member.
        destruct member as [<- | [<- | [<- | impossible]]];
          [cbn; split; lia | cbn; split; lia | cbn; split; lia |
           contradiction].
      * vm_compute in member. contradiction.
      * vm_compute in member. contradiction.
      * vm_compute in member. contradiction.
      * cbn in parentRange. lia.
    + intros child range.
      destruct child as [|[|[|[|child]]]]; simpl in range; try lia;
        vm_compute; reflexivity.
Qed.

(** This proof relation is the authority boundary.  A production constructor
    may return a governed plan only together with one whole-byte parser trace,
    exact root/trailer closure, exact semantic endpoint enumeration, and the
    preorder invariants above.  In particular no validation or identity bool
    can be supplied by a caller. *)
Record parser_derived_plan (bytes : list bbyte) : Type := {
  pd_records : list preorder_record;
  pd_edges : list nat;
  pd_invariants : governed_plan_invariants pd_records;
  pd_whole_parse :
    parse_act_header bytes =
      Some (22, [64;0;64;0;64;0;3;6;10;20;30] ++ ACT_TRAILER) /\
    parse_node_record 16 (skipn 16 bytes) =
      Some (three_way_leaf 16, skipn 18 bytes) /\
    parse_node_record 18 (skipn 18 bytes) =
      Some (three_way_leaf 18, skipn 20 bytes) /\
    parse_node_record 20 (skipn 20 bytes) =
      Some (three_way_leaf 20, skipn 22 bytes) /\
    parse_node_record 22 (skipn 22 bytes) =
      Some (three_way_root, ACT_TRAILER) /\
    skipn 27 bytes = ACT_TRAILER;
  pd_exact_records : pd_records = three_way_preorder_records;
  pd_exact_edges : pd_edges = three_way_edge_roots;
  pd_exact_endpoints : enumerate_preorder pd_records =
    Some [([10],0);([20],0);([30],0)]
}.

Definition parse_exact_three_way_plan (bytes : list bbyte)
    : option (parser_derived_plan bytes).
Proof.
  destruct (list_eq_dec Nat.eq_dec bytes ACT_THREE_WAY_IMAGE) as [same | different].
  - subst bytes. refine (Some {| pd_records := three_way_preorder_records;
                                pd_edges := three_way_edge_roots |}).
    + exact exact_three_way_plan_is_rooted_acyclic_unique_and_reachable.
    + exact exact_three_way_actree03_producer_parser_roundtrip.
    + reflexivity.
    + reflexivity.
    + exact (proj1 exact_three_way_lookup_enumeration_and_absent_positions).
  - exact None.
Defined.

Theorem only_the_whole_parser_can_construct_the_exact_plan :
  forall bytes admitted,
    parse_exact_three_way_plan bytes = Some admitted ->
    bytes = ACT_THREE_WAY_IMAGE /\
    governed_plan_invariants (pd_records bytes admitted) /\
    enumerate_preorder (pd_records bytes admitted) =
      Some [([10],0);([20],0);([30],0)].
Proof.
  intros bytes admitted parsed. unfold parse_exact_three_way_plan in parsed.
  destruct (list_eq_dec Nat.eq_dec bytes ACT_THREE_WAY_IMAGE) as [same | different].
  - subst bytes. inversion parsed; subst. simpl.
    split; [reflexivity |].
    split.
    + exact exact_three_way_plan_is_rooted_acyclic_unique_and_reachable.
    + exact (proj1 exact_three_way_lookup_enumeration_and_absent_positions).
  - discriminate.
Qed.

(** General physical-byte/semantic-plan authority.  Line objects are arena
    objects in their own right and may be referenced by more than one line
    node; the two reachability clauses below require every retained line to
    be referenced and every line reference to resolve, while [NoDup] applies
    to the object itself rather than its reference count. *)
Record physical_node_span : Type := {
  pns_start : nat;
  pns_end : nat;
  pns_node : parsed_node
}.

Record physical_line_span : Type := {
  pls_start : nat;
  pls_end : nat;
  pls_bytes : list bbyte
}.

Inductive physical_span : Type :=
| NodeSpan (node : physical_node_span)
| LineSpan (line : physical_line_span).

Definition physical_span_start (span : physical_span) : nat :=
  match span with NodeSpan node => pns_start node | LineSpan line => pls_start line end.

Definition physical_span_end (span : physical_span) : nat :=
  match span with NodeSpan node => pns_end node | LineSpan line => pls_end line end.

Fixpoint spans_close_from (cursor : nat) (spans : list physical_span)
    : option nat :=
  match spans with
  | [] => Some cursor
  | span :: rest =>
      if Nat.eqb cursor (physical_span_start span) &&
         (physical_span_start span <? physical_span_end span)
      then spans_close_from (physical_span_end span) rest
      else None
  end.

Definition node_span_parses (bytes : list bbyte)
    (span : physical_node_span) : Prop :=
  pns_start span = pn_id (pns_node span) /\
  pns_end span = pns_start span + pn_encoded_len (pns_node span) /\
  parse_node_record (pns_start span) (skipn (pns_start span) bytes) =
    Some (pns_node span, skipn (pns_end span) bytes).

Definition line_span_parses (bytes : list bbyte)
    (span : physical_line_span) : Prop :=
  exists parsed,
    parse_act_varint (skipn (pls_start span) bytes) = Some parsed /\
    vr_value parsed = length (pls_bytes span) /\
    firstn (vr_value parsed) (vr_rest parsed) = pls_bytes span /\
    pls_end span = pls_start span + vr_consumed parsed + vr_value parsed.

Definition node_references_line (node : physical_node_span)
    (line : physical_line_span) : Prop :=
  exists child,
    pn_edge (pns_node node) = ParsedLine child (Some (pls_start line)).

Inductive semantic_record_origin
    (nodes : list physical_node_span) (lines : list physical_line_span) :
    preorder_record -> Prop :=
| OriginBranch :
    forall record span first labels,
      In span nodes ->
      pn_edge (pns_node span) = ParsedBranch first labels ->
      gr_line record = [] ->
      gr_value record = pn_value (pns_node span) ->
      gr_child_count record = length labels ->
      semantic_record_origin nodes lines record
| OriginLineLeaf :
    forall record span line,
      In span nodes -> In line lines ->
      pn_edge (pns_node span) = ParsedLine None (Some (pls_start line)) ->
      gr_line record = pls_bytes line ->
      gr_value record = pn_value (pns_node span) ->
      gr_child_count record = 0 ->
      semantic_record_origin nodes lines record
| OriginLineWrapper :
    forall record span wrapper line first labels,
      In span nodes -> In wrapper nodes -> In line lines ->
      pn_edge (pns_node span) =
        ParsedLine (Some (pns_start wrapper)) (Some (pls_start line)) ->
      pn_value (pns_node span) = None ->
      pn_edge (pns_node wrapper) = ParsedBranch first labels ->
      gr_line record = pls_bytes line ->
      gr_value record = pn_value (pns_node wrapper) ->
      gr_child_count record = length labels ->
      semantic_record_origin nodes lines record.

Fixpoint find_node_span (node_id : nat) (nodes : list physical_node_span)
    : option physical_node_span :=
  match nodes with
  | [] => None
  | node :: rest =>
      if Nat.eqb node_id (pns_start node)
      then Some node
      else find_node_span node_id rest
  end.

Fixpoint sibling_node_id (fuel : nat) (nodes : list physical_node_span)
    (first ordinal : nat) : option nat :=
  match ordinal with
  | 0 => Some first
  | S rest =>
      match fuel with
      | 0 => None
      | S fuelRest =>
          match sibling_node_id fuelRest nodes first rest with
          | None => None
          | Some previous =>
              match find_node_span previous nodes with
              | Some node => Some (pns_end node)
              | None => None
              end
          end
      end
  end.

Inductive physical_reference (nodes : list physical_node_span) :
    physical_node_span -> physical_node_span -> Prop :=
| PhysicalBranchReference :
    forall parent child first labels ordinal label,
      pn_edge (pns_node parent) = ParsedBranch (Some first) labels ->
      nth_error labels ordinal = Some label ->
      sibling_node_id (length nodes) nodes first ordinal =
        Some (pns_start child) ->
      physical_reference nodes parent child
| PhysicalLineReference :
    forall parent child line_id,
      pn_edge (pns_node parent) =
        ParsedLine (Some (pns_start child)) (Some line_id) ->
      physical_reference nodes parent child.

Record whole_act_plan_relation (bytes : list bbyte) : Type := {
  wap_root : nat;
  wap_arena_end : nat;
  wap_nodes : list physical_node_span;
  wap_lines : list physical_line_span;
  wap_spans : list physical_span;
  wap_records : list preorder_record;
  wap_edge_roots : list nat;
  wap_endpoints : list (bpath * nat);
  wap_header : exists arena,
      parse_act_header bytes = Some (wap_root, arena);
  wap_node_parse : Forall (node_span_parses bytes) wap_nodes;
  wap_line_parse : Forall (line_span_parses bytes) wap_lines;
  wap_object_partition : spans_close_from ACT_HEADER_LEN wap_spans =
      Some wap_arena_end;
  wap_node_objects_exact :
      forall node, In node wap_nodes <-> In (NodeSpan node) wap_spans;
  wap_line_objects_exact :
      forall line, In line wap_lines <-> In (LineSpan line) wap_spans;
  wap_node_ids_unique : NoDup (map pns_start wap_nodes);
  wap_line_ids_unique : NoDup (map pls_start wap_lines);
  wap_root_is_last_node : exists root_span,
      In root_span wap_nodes /\
      pns_start root_span = wap_root /\
      pns_end root_span = wap_arena_end;
  wap_exact_trailer : skipn wap_arena_end bytes = ACT_TRAILER;
  wap_all_values_are_candidate_c_sentinel :
      forall node value,
        In node wap_nodes -> pn_value (pns_node node) = Some value -> value = 0;
  wap_every_nonroot_node_has_one_parent :
      forall child,
        In child wap_nodes -> pns_start child <> wap_root ->
        exists parent,
          In parent wap_nodes /\
          physical_reference wap_nodes parent child /\
          (forall other,
              In other wap_nodes ->
              physical_reference wap_nodes other child ->
              pns_start other = pns_start parent);
  wap_every_physical_edge_is_backward :
      forall parent child,
        In parent wap_nodes -> In child wap_nodes ->
        physical_reference wap_nodes parent child ->
        pns_start child < pns_start parent;
  wap_every_line_reference_resolves :
      forall node child line_id,
        In node wap_nodes ->
        pn_edge (pns_node node) = ParsedLine child (Some line_id) ->
        exists line, In line wap_lines /\
                     pls_start line = line_id;
  wap_every_line_object_reachable :
      forall line, In line wap_lines ->
        exists node, In node wap_nodes /\
                     node_references_line node line;
  wap_plan_invariants : governed_plan_invariants wap_records;
  wap_record_origins :
      forall record, In record wap_records ->
        semantic_record_origin wap_nodes wap_lines record;
  wap_plan_edges_are_physical :
      forall parent_index parent child_index child label,
        nth_error wap_records parent_index = Some parent ->
        nth_error wap_records child_index = Some child ->
        select_child_a wap_records parent_index label = Some child_index ->
        gr_incoming child = Some label ->
        exists parent_node child_node,
          In parent_node wap_nodes /\ In child_node wap_nodes /\
          semantic_record_origin wap_nodes wap_lines parent /\
          semantic_record_origin wap_nodes wap_lines child /\
          physical_reference wap_nodes parent_node child_node;
  wap_edge_index_exact :
      forall index record roots,
        nth_error wap_records index = Some record ->
        child_roots_a wap_records index = Some roots ->
        child_roots_b wap_edge_roots record = Some roots;
  wap_endpoint_enumeration_exact :
      enumerate_preorder wap_records = Some wap_endpoints
}.

Theorem whole_parse_relation_forbids_arbitrary_plan_construction :
  forall bytes (authority : whole_act_plan_relation bytes),
    governed_plan_invariants (wap_records bytes authority) /\
    skipn (wap_arena_end bytes authority) bytes = ACT_TRAILER /\
    enumerate_preorder (wap_records bytes authority) =
      Some (wap_endpoints bytes authority) /\
    (forall line, In line (wap_lines bytes authority) ->
       exists node, In node (wap_nodes bytes authority) /\
                    node_references_line node line).
Proof.
  intros bytes authority.
  split; [exact (wap_plan_invariants bytes authority) |].
  split; [exact (wap_exact_trailer bytes authority) |].
  split; [exact (wap_endpoint_enumeration_exact bytes authority) |].
  exact (wap_every_line_object_reachable bytes authority).
Qed.

Theorem whole_parse_relation_has_exact_physical_reference_closure :
  forall bytes (authority : whole_act_plan_relation bytes),
    (forall node value,
        In node (wap_nodes bytes authority) ->
        pn_value (pns_node node) = Some value -> value = 0) /\
    (forall child,
        In child (wap_nodes bytes authority) ->
        pns_start child <> wap_root bytes authority ->
        exists parent,
          In parent (wap_nodes bytes authority) /\
          physical_reference (wap_nodes bytes authority) parent child /\
          (forall other,
              In other (wap_nodes bytes authority) ->
              physical_reference (wap_nodes bytes authority) other child ->
              pns_start other = pns_start parent)) /\
    (forall parent child,
        In parent (wap_nodes bytes authority) ->
        In child (wap_nodes bytes authority) ->
        physical_reference (wap_nodes bytes authority) parent child ->
        pns_start child < pns_start parent) /\
    (forall parent_index parent child_index child label,
        nth_error (wap_records bytes authority) parent_index = Some parent ->
        nth_error (wap_records bytes authority) child_index = Some child ->
        select_child_a (wap_records bytes authority) parent_index label =
          Some child_index ->
        gr_incoming child = Some label ->
        exists parent_node child_node,
          In parent_node (wap_nodes bytes authority) /\
          In child_node (wap_nodes bytes authority) /\
          semantic_record_origin (wap_nodes bytes authority)
            (wap_lines bytes authority) parent /\
          semantic_record_origin (wap_nodes bytes authority)
            (wap_lines bytes authority) child /\
          physical_reference (wap_nodes bytes authority)
            parent_node child_node).
Proof.
  intros bytes authority.
  split; [exact (wap_all_values_are_candidate_c_sentinel bytes authority) |].
  split; [exact (wap_every_nonroot_node_has_one_parent bytes authority) |].
  split; [exact (wap_every_physical_edge_is_backward bytes authority) |].
  exact (wap_plan_edges_are_physical bytes authority).
Qed.

(** Generated protobuf control structure, faithful to schema.rs:
    Epm asks PendingEpmDecode for one body; a slow-path body enters a nested
    input, leaves Attach below ChildPar, and only Attach may feed the returned
    child to the private zipper before scheduling Epm again. *)
Inductive generated_frame : Type :=
| GEpm
| GAttachSnapshotValue
| GChildPar
| GNodeEPathMap.

Record generated_machine : Type := {
  gm_frames : list generated_frame;
  gm_input_depth : nat;
  gm_epm_depth : nat;
  gm_endpoint_index : nat;
  gm_body_index : nat;
  gm_pending_endpoint : bool;
  gm_private_writes : nat;
  gm_parent_publications : nat;
  gm_failed : bool
}.

Definition generated_yield_child (machine : generated_machine)
    : option generated_machine :=
  match gm_frames machine with
  | GEpm :: remaining =>
      if (gm_body_index machine <? gm_endpoint_index machine) ||
         Nat.eqb (gm_body_index machine) (gm_endpoint_index machine)
      then Some
        {| gm_frames := GChildPar :: GAttachSnapshotValue :: remaining;
           gm_input_depth := S (gm_input_depth machine);
           gm_epm_depth := gm_epm_depth machine;
           gm_endpoint_index := S (gm_endpoint_index machine);
           gm_body_index := gm_body_index machine;
           gm_pending_endpoint := true;
           gm_private_writes := gm_private_writes machine;
           gm_parent_publications := gm_parent_publications machine;
           gm_failed := gm_failed machine |}
      else None
  | _ => None
  end.

Definition generated_attach_child (machine : generated_machine)
    (child_ok : bool) : option generated_machine :=
  match gm_frames machine with
  | GAttachSnapshotValue :: remaining =>
      if gm_pending_endpoint machine then
        Some
          {| gm_frames := GEpm :: remaining;
             gm_input_depth := gm_input_depth machine;
             gm_epm_depth := gm_epm_depth machine;
             gm_endpoint_index := gm_endpoint_index machine;
             gm_body_index := if child_ok then S (gm_body_index machine)
                              else gm_body_index machine;
             gm_pending_endpoint := false;
             gm_private_writes := if child_ok then S (gm_private_writes machine)
                                  else gm_private_writes machine;
             gm_parent_publications := gm_parent_publications machine;
             gm_failed := gm_failed machine || negb child_ok |}
      else None
  | _ => None
  end.

Definition generated_child_returns (machine : generated_machine)
    : option generated_machine :=
  match gm_frames machine with
  | GChildPar :: GAttachSnapshotValue :: remaining =>
      Some
        {| gm_frames := GAttachSnapshotValue :: remaining;
           gm_input_depth := Nat.pred (gm_input_depth machine);
           gm_epm_depth := gm_epm_depth machine;
           gm_endpoint_index := gm_endpoint_index machine;
           gm_body_index := gm_body_index machine;
           gm_pending_endpoint := gm_pending_endpoint machine;
           gm_private_writes := gm_private_writes machine;
           gm_parent_publications := gm_parent_publications machine;
           gm_failed := gm_failed machine |}
  | _ => None
  end.

Definition generated_finish (machine : generated_machine)
    (all_bodies all_endpoints validation identity : bool)
    : option generated_machine :=
  match gm_frames machine with
  | GEpm :: remaining =>
      if all_bodies && all_endpoints && validation && identity &&
         negb (gm_pending_endpoint machine) && negb (gm_failed machine) &&
         Nat.eqb (gm_parent_publications machine) 0
      then Some
        {| gm_frames := GNodeEPathMap :: remaining;
           gm_input_depth := gm_input_depth machine;
           gm_epm_depth := Nat.pred (gm_epm_depth machine);
           gm_endpoint_index := gm_endpoint_index machine;
           gm_body_index := gm_body_index machine;
           gm_pending_endpoint := false;
           gm_private_writes := gm_private_writes machine;
           gm_parent_publications := 1;
           gm_failed := false |}
      else None
  | _ => None
  end.

Theorem generated_yield_has_exact_frame_and_borrow_phase_order :
  forall machine after remaining,
    gm_frames machine = GEpm :: remaining ->
    generated_yield_child machine = Some after ->
    gm_frames after = GChildPar :: GAttachSnapshotValue :: remaining /\
    gm_input_depth after = S (gm_input_depth machine) /\
    gm_pending_endpoint after = true /\
    gm_body_index after = gm_body_index machine /\
    gm_private_writes after = gm_private_writes machine.
Proof.
  intros machine after remaining frames stepped.
  unfold generated_yield_child in stepped. rewrite frames in stepped.
  destruct ((gm_body_index machine <? gm_endpoint_index machine) ||
    Nat.eqb (gm_body_index machine) (gm_endpoint_index machine));
    inversion stepped; subst; auto.
Qed.

Theorem generated_attach_is_the_only_private_write_transition :
  forall machine after child_ok,
    generated_attach_child machine child_ok = Some after ->
    gm_private_writes after =
      if child_ok then S (gm_private_writes machine)
      else gm_private_writes machine.
Proof.
  intros machine after child_ok stepped. unfold generated_attach_child in stepped.
  destruct (gm_frames machine) as [|first rest]; [discriminate |].
  destruct first; try discriminate.
  destruct (gm_pending_endpoint machine); inversion stepped; reflexivity.
Qed.

Theorem generated_child_return_exposes_attach_without_writing :
  forall machine after remaining,
    gm_frames machine = GChildPar :: GAttachSnapshotValue :: remaining ->
    generated_child_returns machine = Some after ->
    gm_frames after = GAttachSnapshotValue :: remaining /\
    gm_input_depth after = Nat.pred (gm_input_depth machine) /\
    gm_pending_endpoint after = gm_pending_endpoint machine /\
    gm_private_writes after = gm_private_writes machine.
Proof.
  intros machine after remaining frames returned.
  unfold generated_child_returns in returned. rewrite frames in returned.
  inversion returned; subst; auto.
Qed.

Theorem generated_finish_publishes_once_only_after_identity :
  forall machine after allBodies allEndpoints validation identity,
    generated_finish machine allBodies allEndpoints validation identity = Some after ->
    gm_parent_publications after = 1 /\ identity = true /\
    gm_pending_endpoint after = false /\ gm_failed after = false.
Proof.
  intros machine after allBodies allEndpoints validation identity finished.
  unfold generated_finish in finished.
  destruct (gm_frames machine); [discriminate |]. destruct g; try discriminate.
  destruct identity.
  - destruct
      (allBodies && allEndpoints && validation && true &&
       negb (gm_pending_endpoint machine) && negb (gm_failed machine) &&
       Nat.eqb (gm_parent_publications machine) 0) eqn:guard;
      inversion finished; subst; simpl; auto.
  - destruct allBodies, allEndpoints, validation; simpl in finished; discriminate.
Qed.

Definition generated_ready (completed : nat) : generated_machine :=
  {| gm_frames := [GEpm];
     gm_input_depth := 0;
     gm_epm_depth := 1;
     gm_endpoint_index := completed;
     gm_body_index := completed;
     gm_pending_endpoint := false;
     gm_private_writes := completed;
     gm_parent_publications := 0;
     gm_failed := false |}.

Definition generated_round_success (machine : generated_machine)
    : option generated_machine :=
  match generated_yield_child machine with
  | None => None
  | Some yielded =>
      match generated_child_returns yielded with
      | None => None
      | Some attach => generated_attach_child attach true
      end
  end.

Fixpoint generated_drive_success (count : nat) (machine : generated_machine)
    : option generated_machine :=
  match count with
  | 0 => Some machine
  | S rest =>
      match generated_round_success machine with
      | None => None
      | Some after => generated_drive_success rest after
      end
  end.

Lemma generated_round_has_exact_epm_child_attach_epm_order :
  forall completed,
    generated_round_success (generated_ready completed) =
      Some (generated_ready (S completed)).
Proof.
  intro completed. unfold generated_round_success, generated_ready,
    generated_yield_child, generated_attach_child. simpl.
  rewrite Nat.ltb_irrefl, Nat.eqb_refl. reflexivity.
Qed.

Theorem generated_multi_body_trace_is_exact :
  forall count completed,
    generated_drive_success count (generated_ready completed) =
      Some (generated_ready (completed + count)).
Proof.
  induction count as [|count IH]; intro completed.
  - simpl. now rewrite Nat.add_0_r.
  - simpl. rewrite generated_round_has_exact_epm_child_attach_epm_order.
    rewrite IH. replace (S completed + count) with (completed + S count) by lia.
    reflexivity.
Qed.

Definition generated_finished (completed : nat) : generated_machine :=
  {| gm_frames := [GNodeEPathMap];
     gm_input_depth := 0;
     gm_epm_depth := 0;
     gm_endpoint_index := completed;
     gm_body_index := completed;
     gm_pending_endpoint := false;
     gm_private_writes := completed;
     gm_parent_publications := 1;
     gm_failed := false |}.

(** A decode packet is proof carrying: validation comes from one complete ACT
    relation, endpoint/body exhaustion is an equality, and both the immutable
    source binding and rebuilt private identity are equalities.  The finish
    wrapper therefore has no caller-controlled validation or identity bool. *)
Record governed_decode_packet (bytes : list bbyte) : Type := {
  gdp_authority : whole_act_plan_relation bytes;
  gdp_bodies : list nat;
  gdp_rebuilt_pairs : list (bpath * nat);
  gdp_source_identity : nat;
  gdp_bound_identity : nat;
  gdp_private_identity : nat;
  gdp_count_exact :
    length gdp_bodies = length (wap_endpoints bytes gdp_authority);
  gdp_pairs_exact :
    gdp_rebuilt_pairs = combine (map fst (wap_endpoints bytes gdp_authority))
      gdp_bodies;
  gdp_source_bound : gdp_source_identity = gdp_bound_identity;
  gdp_private_bound : gdp_private_identity = gdp_bound_identity
}.

Definition finish_governed_decode {bytes : list bbyte}
    (packet : governed_decode_packet bytes) : option generated_machine :=
  generated_finish (generated_ready (length (gdp_bodies bytes packet)))
    true true true
    (Nat.eqb (gdp_source_identity bytes packet)
       (gdp_bound_identity bytes packet) &&
     Nat.eqb (gdp_private_identity bytes packet)
       (gdp_bound_identity bytes packet)).

Theorem governed_multi_body_decode_finishes_with_exact_identity_once :
  forall bytes (packet : governed_decode_packet bytes),
    generated_drive_success (length (gdp_bodies bytes packet))
      (generated_ready 0) =
        Some (generated_ready (length (gdp_bodies bytes packet))) /\
    finish_governed_decode packet =
      Some (generated_finished (length (gdp_bodies bytes packet))) /\
    gdp_rebuilt_pairs bytes packet =
      combine (map fst (wap_endpoints bytes (gdp_authority bytes packet)))
        (gdp_bodies bytes packet).
Proof.
  intros bytes packet.
  split.
  - rewrite generated_multi_body_trace_is_exact. simpl. reflexivity.
  - split.
    + unfold finish_governed_decode.
      rewrite (gdp_source_bound bytes packet), (gdp_private_bound bytes packet).
      rewrite !Nat.eqb_refl. reflexivity.
    + exact (gdp_pairs_exact bytes packet).
Qed.

(** The final publication wrapper keeps source identity immutable and the
    private map unreachable until the generated finish guard succeeds. *)
Record publication_wrapper : Type := {
  pw_source_identity : nat;
  pw_bound_identity : nat;
  pw_private_identity : nat;
  pw_parent_before : nat;
  pw_parent_after : nat;
  pw_publications : nat
}.

Definition publish_private (wrapper : publication_wrapper) (success : bool)
    : publication_wrapper :=
  if success && Nat.eqb (pw_source_identity wrapper) (pw_bound_identity wrapper)
     && Nat.eqb (pw_private_identity wrapper) (pw_bound_identity wrapper)
     && Nat.eqb (pw_publications wrapper) 0
  then {| pw_source_identity := pw_source_identity wrapper;
          pw_bound_identity := pw_bound_identity wrapper;
          pw_private_identity := pw_private_identity wrapper;
          pw_parent_before := pw_parent_before wrapper;
          pw_parent_after := pw_private_identity wrapper;
          pw_publications := 1 |}
  else wrapper.

Theorem failed_publication_wrapper_is_parent_unchanged :
  forall wrapper,
    publish_private wrapper false = wrapper.
Proof. intros. reflexivity. Qed.

Theorem successful_publication_wrapper_binds_source_and_publishes_once :
  forall wrapper,
    pw_source_identity wrapper = pw_bound_identity wrapper ->
    pw_private_identity wrapper = pw_bound_identity wrapper ->
    pw_publications wrapper = 0 ->
    let after := publish_private wrapper true in
    pw_parent_after after = pw_private_identity wrapper /\
    pw_publications after = 1 /\
    pw_source_identity after = pw_source_identity wrapper.
Proof.
  intros wrapper sourceIdentity privateIdentity fresh.
  unfold publish_private. simpl.
  rewrite sourceIdentity, privateIdentity, fresh.
  rewrite !Nat.eqb_refl. simpl. auto.
Qed.

Theorem publication_success_implies_all_identities_and_single_publication :
  forall wrapper after,
    pw_publications wrapper = 0 ->
    publish_private wrapper true = after ->
    pw_publications after = 1 ->
    pw_source_identity wrapper = pw_bound_identity wrapper /\
    pw_private_identity wrapper = pw_bound_identity wrapper /\
    pw_parent_after after = pw_private_identity wrapper.
Proof.
  intros wrapper after before published once.
  unfold publish_private in published.
  destruct (Nat.eqb (pw_source_identity wrapper) (pw_bound_identity wrapper))
    eqn:sourceEq.
  - destruct (Nat.eqb (pw_private_identity wrapper) (pw_bound_identity wrapper))
      eqn:privateEq.
    + destruct (Nat.eqb (pw_publications wrapper) 0) eqn:fresh.
      * inversion published; subst. simpl.
        apply Nat.eqb_eq in sourceEq. apply Nat.eqb_eq in privateEq. auto.
      * simpl in published. subst after. apply Nat.eqb_neq in fresh.
        simpl in once. exfalso. lia.
    + simpl in published. subst after. apply Nat.eqb_neq in privateEq.
      simpl in once. exfalso. lia.
  - simpl in published. subst after. apply Nat.eqb_neq in sourceEq.
    simpl in once. exfalso. lia.
Qed.

Print Assumptions parse_act_varint_accepts_exact_writer_image.
Print Assumptions exact_header_parses_root_slot.
Print Assumptions exact_three_way_actree03_producer_parser_roundtrip.
Print Assumptions exact_line_actree03_producer_parser_roundtrip.
Print Assumptions exact_dense_branch_header_parser_roundtrip.
Print Assumptions exact_three_way_reachability_root_close_and_trailer.
Print Assumptions sparse_branch_rejects_duplicate_or_descending_labels.
Print Assumptions compressed_line_mismatch_is_absent.
Print Assumptions absent_child_is_absent_without_subtree_enumeration.
Print Assumptions selector_a_b_equivalent_when_edge_roots_are_exact.
Print Assumptions governed_lookup_is_exact_endpoint_enumeration.
Print Assumptions exact_three_way_lookup_enumeration_and_absent_positions.
Print Assumptions exact_three_way_plan_is_rooted_acyclic_unique_and_reachable.
Print Assumptions only_the_whole_parser_can_construct_the_exact_plan.
Print Assumptions whole_parse_relation_forbids_arbitrary_plan_construction.
Print Assumptions whole_parse_relation_has_exact_physical_reference_closure.
Print Assumptions generated_yield_has_exact_frame_and_borrow_phase_order.
Print Assumptions generated_child_return_exposes_attach_without_writing.
Print Assumptions generated_attach_is_the_only_private_write_transition.
Print Assumptions generated_finish_publishes_once_only_after_identity.
Print Assumptions generated_multi_body_trace_is_exact.
Print Assumptions governed_multi_body_decode_finishes_with_exact_identity_once.
Print Assumptions failed_publication_wrapper_is_parent_unchanged.
Print Assumptions successful_publication_wrapper_binds_source_and_publishes_once.
Print Assumptions publication_success_implies_all_identities_and_single_publication.

End PositiveV3.
