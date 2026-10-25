From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool Sorting.Sorted
  Sorting.Permutation.
Require Import Epm1CursorPositiveV3 Epm1CursorPositiveV4.

Import ListNotations.
Import PositiveV3 PositiveV4.

(**
  Concrete ACTree03 authority, revision V5.

  V3 proves parsing for a caller-supplied canonical varint image and V4 proves
  equality with an abstract event stream.  Neither is an encoder/parser
  authority.  This module starts the replacement at the first physical byte:
  the writer computes the unique little-endian image itself, while the parser
  remains the executable V3 parser.  No proof caller supplies raw varint bytes.
*)
Module ActConcreteV5.

Fixpoint little_endian_digits (fuel value : nat) : list bbyte :=
  match fuel with
  | 0 => []
  | S remaining =>
      (value mod 256) ::
      if value <? 256 then []
      else little_endian_digits remaining (value / 256)
  end.

Definition encode_act_varint (value : nat) : list bbyte :=
  if value <=? ACT_VARINT_BIAS then [value]
  else
    let raw := little_endian_digits 8 value in
    (ACT_VARINT_BIAS + length raw) :: raw.

Lemma little_endian_digits_nonempty : forall fuel value,
  0 < fuel -> little_endian_digits fuel value <> [].
Proof.
  intros [|fuel] value positive; [lia|].
  simpl. discriminate.
Qed.

Lemma little_endian_digits_length_bounded : forall fuel value,
  length (little_endian_digits fuel value) <= fuel.
Proof.
  induction fuel as [|fuel IH]; intro value; [reflexivity|].
  simpl. destruct (value <? 256); simpl; [lia|].
  apply le_n_S. apply IH.
Qed.

Lemma little_endian_digits_bytes : forall fuel value,
  Forall (fun byte => byte < 256) (little_endian_digits fuel value).
Proof.
  induction fuel as [|fuel IH]; intro value; [constructor|].
  simpl. constructor.
  - change (value mod 256 < 256). apply Nat.mod_upper_bound. lia.
  - destruct (value <? 256); [constructor|apply IH].
Qed.

Lemma little_endian_digits_decode : forall fuel value,
  0 < fuel -> value < Nat.pow 256 fuel ->
  decode_le (little_endian_digits fuel value) = value.
Proof.
  induction fuel as [|fuel IH]; intros value positive bounded; [lia|].
  cbn [little_endian_digits]. destruct (value <? 256) eqn:small.
  - apply Nat.ltb_lt in small.
    cbn [decode_le].
    rewrite Nat.mul_0_r, Nat.add_0_r.
    apply Nat.mod_small. exact small.
  - apply Nat.ltb_ge in small.
    cbn [decode_le].
    rewrite IH.
    + pose proof (Nat.div_mod value 256 ltac:(lia)) as division. lia.
    + destruct fuel; [cbn in bounded; lia|lia].
    + apply Nat.Div0.div_lt_upper_bound.
      rewrite Nat.pow_succ_r in bounded by lia. nia.
Qed.

Lemma last_nonzero_cons_nonempty : forall low rest,
  rest <> [] -> last_nonzero (low :: rest) = last_nonzero rest.
Proof.
  intros low rest nonempty.
  unfold last_nonzero.
  cbn [rev].
  destruct (rev rest) as [|high tail] eqn:reversed.
  - apply (f_equal (@rev bbyte)) in reversed.
    rewrite rev_involutive in reversed. simpl in reversed. contradiction.
  - reflexivity.
Qed.

Lemma little_endian_digits_last_nonzero : forall fuel value,
  0 < fuel -> 0 < value -> value < Nat.pow 256 fuel ->
  last_nonzero (little_endian_digits fuel value) = true.
Proof.
  induction fuel as [|fuel IH]; intros value fuelPositive valuePositive bounded;
    [lia|].
  cbn [little_endian_digits]. destruct (value <? 256) eqn:small.
  - apply Nat.ltb_lt in small. unfold last_nonzero. cbn [rev].
    rewrite Nat.mod_small by exact small.
    cbn [app].
    destruct (Nat.eqb value 0) eqn:zero; [apply Nat.eqb_eq in zero; lia|].
    reflexivity.
  - apply Nat.ltb_ge in small.
    rewrite last_nonzero_cons_nonempty.
    + apply IH.
      * destruct fuel; [cbn in bounded; lia|lia].
      * apply Nat.div_str_pos; lia.
      * apply Nat.Div0.div_lt_upper_bound.
        rewrite Nat.pow_succ_r in bounded by lia. nia.
    + apply little_endian_digits_nonempty.
      destruct fuel; [cbn in bounded; lia|lia].
Qed.

Theorem encode_act_varint_is_canonical : forall value,
  value < Nat.pow 256 8 ->
  canonical_varint_image value
    (if value <=? ACT_VARINT_BIAS then []
     else little_endian_digits 8 value).
Proof.
  intros value bounded.
  destruct (value <=? ACT_VARINT_BIAS) eqn:small.
  - apply Nat.leb_le in small. exact small.
  - apply Nat.leb_gt in small.
    change
      (ACT_VARINT_BIAS < value /\
       1 <= length (little_endian_digits 8 value) <= 8 /\
       Forall (fun byte => byte < 256) (little_endian_digits 8 value) /\
       last_nonzero (little_endian_digits 8 value) = true /\
       decode_le (little_endian_digits 8 value) = value).
    repeat split.
    + exact small.
    + pose proof (little_endian_digits_nonempty 8 value ltac:(lia)).
      destruct (little_endian_digits 8 value); [contradiction|simpl; lia].
    + apply little_endian_digits_length_bounded.
    + apply little_endian_digits_bytes.
    + apply little_endian_digits_last_nonzero; lia.
    + apply little_endian_digits_decode; lia.
Qed.

Theorem encode_act_varint_parse_roundtrip : forall value suffix,
  value < Nat.pow 256 8 ->
  parse_act_varint (encode_act_varint value ++ suffix) =
    Some {| vr_value := value;
            vr_consumed := length (encode_act_varint value);
            vr_rest := suffix |}.
Proof.
  intros value suffix bounded.
  unfold encode_act_varint.
  destruct (value <=? ACT_VARINT_BIAS) eqn:small.
  - change
      (parse_act_varint (emit_act_varint_from_image value [] ++ suffix) =
       Some {| vr_value := value;
               vr_consumed := length (emit_act_varint_from_image value []);
               vr_rest := suffix |}).
    apply parse_act_varint_accepts_exact_writer_image.
    apply Nat.leb_le in small. exact small.
  - change
      (parse_act_varint
         (emit_act_varint_from_image value (little_endian_digits 8 value) ++ suffix) =
       Some {| vr_value := value;
               vr_consumed := length
                 (emit_act_varint_from_image value (little_endian_digits 8 value));
               vr_rest := suffix |}).
    apply parse_act_varint_accepts_exact_writer_image.
    pose proof (encode_act_varint_is_canonical value bounded) as canonical.
    rewrite small in canonical. exact canonical.
Qed.

Theorem encode_act_varint_has_exact_u64_bound : forall value,
  value < Nat.pow 256 8 ->
  length (encode_act_varint value) <= 9 /\
  forall suffix,
    parse_act_varint (encode_act_varint value ++ suffix) =
      Some {| vr_value := value;
              vr_consumed := length (encode_act_varint value);
              vr_rest := suffix |}.
Proof.
  intros value bounded.
  split.
  - unfold encode_act_varint.
    destruct (value <=? ACT_VARINT_BIAS).
    + cbn [length]. lia.
    + change (S (length (little_endian_digits 8 value)) <= 9).
      apply le_n_S. apply little_endian_digits_length_bounded.
  - intro suffix. apply encode_act_varint_parse_roundtrip. exact bounded.
Qed.

(** PathMap's Catamorphism does not immediately write the node returned by a
    child callback.  The callback first writes the child's descendants and
    line object, returns one pending root node, and the parent callback later
    writes all direct-child roots contiguously.  The following state machine
    preserves that distinction instead of pretending that ordinary recursive
    preorder or postorder emission matches the source. *)

Record line_cache_entry : Type := {
  lce_hash : nat;
  lce_bytes : list bbyte;
  lce_id : nat
}.

Record act_writer_state : Type := {
  aws_arena : list bbyte;
  aws_lines : list line_cache_entry
}.

Definition writer_position (state : act_writer_state) : nat :=
  ACT_HEADER_LEN + length (aws_arena state).

Fixpoint lookup_line_hash (hash : nat) (entries : list line_cache_entry)
    : option line_cache_entry :=
  match entries with
  | [] => None
  | entry :: rest =>
      if Nat.eqb hash (lce_hash entry) then Some entry
      else lookup_line_hash hash rest
  end.

Fixpoint remove_line_hash (hash : nat) (entries : list line_cache_entry)
    : list line_cache_entry :=
  match entries with
  | [] => []
  | entry :: rest =>
      if Nat.eqb hash (lce_hash entry)
      then remove_line_hash hash rest
      else entry :: remove_line_hash hash rest
  end.

Definition put_line_hash (entry : line_cache_entry)
    (entries : list line_cache_entry) : list line_cache_entry :=
  entry :: remove_line_hash (lce_hash entry) entries.

Definition append_arena (bytes : list bbyte) (state : act_writer_state)
    : act_writer_state :=
  {| aws_arena := aws_arena state ++ bytes;
     aws_lines := aws_lines state |}.

Definition add_or_reuse_line (line_hash : list bbyte -> nat)
    (line : list bbyte) (state : act_writer_state)
    : nat * act_writer_state :=
  let hash := line_hash line in
  match lookup_line_hash hash (aws_lines state) with
  | Some previous =>
      if list_eq_dec Nat.eq_dec (lce_bytes previous) line
      then (lce_id previous, state)
      else
        let id := writer_position state in
        let encoded := encode_act_varint (length line) ++ line in
        (id,
         {| aws_arena := aws_arena state ++ encoded;
            aws_lines := put_line_hash
              {| lce_hash := hash; lce_bytes := line; lce_id := id |}
              (aws_lines state) |})
  | None =>
      let id := writer_position state in
      let encoded := encode_act_varint (length line) ++ line in
      (id,
       {| aws_arena := aws_arena state ++ encoded;
          aws_lines := put_line_hash
            {| lce_hash := hash; lce_bytes := line; lce_id := id |}
            (aws_lines state) |})
  end.

Inductive pending_node : Type :=
| PendingBranch (value : option nat) (children : list (bbyte * nat))
| PendingLine (value : option nat) (child : option nat) (line_id : nat).

Definition optional_value_bytes (value : option nat) : list bbyte :=
  match value with None => [] | Some scalar => encode_act_varint scalar end.

Definition labels_of_children (children : list (bbyte * nat)) : list bbyte :=
  map fst children.

Definition first_child_id (children : list (bbyte * nat)) : option nat :=
  match children with [] => None | (_, id) :: _ => Some id end.

Definition mask_octet (labels : list bbyte) (octet : nat) : bbyte :=
  fold_left (fun mask bit =>
    if existsb (Nat.eqb (octet * 8 + bit)) labels
    then Nat.setbit mask bit
    else mask) (seq 0 8) 0.

Definition dense_label_mask (labels : list bbyte) : list bbyte :=
  map (mask_octet labels) (seq 0 32).

Definition encoded_branch_labels (labels : list bbyte) : list bbyte :=
  if length labels <? 32 then labels else dense_label_mask labels.

Definition branch_head (value : option nat) (labels : list bbyte) : bbyte :=
  Nat.min (length labels) 32 +
  match value with None => 0 | Some _ => ACT_VALUE_FLAG end.

Definition line_head (value : option nat) (child : option nat) : bbyte :=
  ACT_LINE_FLAG +
  (match value with None => 0 | Some _ => ACT_VALUE_FLAG end) +
  (match child with None => 0 | Some _ => 1 end).

Definition relative_bytes (origin target : nat) : list bbyte :=
  encode_act_varint (origin - target).

Definition encode_pending_node (origin : nat) (node : pending_node)
    : list bbyte :=
  match node with
  | PendingBranch value children =>
      let labels := labels_of_children children in
      [branch_head value labels] ++ optional_value_bytes value ++
      match first_child_id children with
      | None => encoded_branch_labels labels
      | Some first => relative_bytes origin first ++ encoded_branch_labels labels
      end
  | PendingLine value child line_id =>
      [line_head value child] ++ optional_value_bytes value ++
      (match child with None => [] | Some target => relative_bytes origin target end) ++
      relative_bytes origin line_id
  end.

Definition push_pending_node (node : pending_node) (state : act_writer_state)
    : nat * act_writer_state :=
  let id := writer_position state in
  (id, append_arena (encode_pending_node id node) state).

Fixpoint push_pending_children (labels : list bbyte)
    (nodes : list pending_node) (state : act_writer_state)
    : option (list (bbyte * nat) * act_writer_state) :=
  match labels, nodes with
  | [], [] => Some ([], state)
  | label :: label_rest, node :: node_rest =>
      let '(id, after_node) := push_pending_node node state in
      match push_pending_children label_rest node_rest after_node with
      | None => None
      | Some (children, final) => Some ((label, id) :: children, final)
      end
  | _, _ => None
  end.

Section SourceWriter.

Variable line_hash : list bbyte -> nat.

Fixpoint build_pending_tree (tree : writer_tree) (state : act_writer_state)
    : option (pending_node * act_writer_state) :=
  match tree with
  | WriterNode line value children =>
      match build_pending_forest children state with
      | None => None
      | Some (child_nodes, after_descendants) =>
          match push_pending_children (forest_labels children) child_nodes
                  after_descendants with
          | None => None
          | Some (child_ids, after_children) =>
              match line with
              | [] => Some (PendingBranch value child_ids, after_children)
              | _ :: _ =>
                  let '(line_id, after_line) :=
                    add_or_reuse_line line_hash line after_children in
                  match child_ids with
                  | [] => Some (PendingLine value None line_id, after_line)
                  | _ :: _ =>
                      let '(wrapper_id, after_wrapper) :=
                        push_pending_node (PendingBranch value child_ids) after_line in
                      Some (PendingLine None (Some wrapper_id) line_id, after_wrapper)
                  end
              end
          end
      end
  end
with build_pending_forest (forest : writer_forest) (state : act_writer_state)
    : option (list pending_node * act_writer_state) :=
  match forest with
  | WriterNil => Some ([], state)
  | WriterCons _ child rest =>
      match build_pending_tree child state with
      | None => None
      | Some (node, after_child) =>
          match build_pending_forest rest after_child with
          | None => None
          | Some (nodes, final) => Some (node :: nodes, final)
          end
      end
  end.

Definition empty_writer_state : act_writer_state :=
  {| aws_arena := []; aws_lines := [] |}.

Definition encode_writer_tree (tree : writer_tree) : option (list bbyte) :=
  match build_pending_tree tree empty_writer_state with
  | None => None
  | Some (root, before_root) =>
      let '(root_id, final) := push_pending_node root before_root in
      Some (ACT_MAGIC ++ le_fixed 8 root_id ++ aws_arena final ++ ACT_TRAILER)
  end.

End SourceWriter.

(** An executable producer certificate.  Objects are appended only by the two
    source-authoritative writer operations: a non-reused line allocation and a
    pending-node write.  Pending roots remain absent until the parent callback
    invokes [traced_push_pending_node], preserving PathMap's callback order. *)

Inductive physical_writer_object : Type :=
| WriterLineObject (id hash : nat) (line : list bbyte)
| WriterNodeObject (id : nat) (node : pending_node).

Definition physical_writer_object_start (object : physical_writer_object) : nat :=
  match object with
  | WriterLineObject id _ _ | WriterNodeObject id _ => id
  end.

Definition physical_writer_object_bytes (object : physical_writer_object)
    : list bbyte :=
  match object with
  | WriterLineObject _ _ line => encode_act_varint (length line) ++ line
  | WriterNodeObject id node => encode_pending_node id node
  end.

Definition physical_writer_bytes (objects : list physical_writer_object)
    : list bbyte := concat (map physical_writer_object_bytes objects).

Fixpoint replay_physical_line_cache (objects : list physical_writer_object)
    (cache : list line_cache_entry) : list line_cache_entry :=
  match objects with
  | [] => cache
  | WriterNodeObject _ _ :: rest => replay_physical_line_cache rest cache
  | WriterLineObject id hash line :: rest =>
      replay_physical_line_cache rest
        (put_line_hash
          {| lce_hash := hash; lce_bytes := line; lce_id := id |} cache)
  end.

Record traced_writer_state : Type := {
  tws_state : act_writer_state;
  tws_objects : list physical_writer_object
}.

Definition traced_writer_consistent (state : traced_writer_state) : Prop :=
  aws_arena (tws_state state) = physical_writer_bytes (tws_objects state).

Fixpoint physical_writer_objects_layout (position : nat)
    (objects : list physical_writer_object) : Prop :=
  match objects with
  | [] => True
  | object :: rest =>
      physical_writer_object_start object = position /\
      physical_writer_objects_layout
        (position + length (physical_writer_object_bytes object)) rest
  end.

Definition traced_writer_layout (state : traced_writer_state) : Prop :=
  physical_writer_objects_layout ACT_HEADER_LEN (tws_objects state).

Definition traced_writer_invariant (state : traced_writer_state) : Prop :=
  traced_writer_consistent state /\ traced_writer_layout state.

Definition traced_writer_cache_consistent (state : traced_writer_state) : Prop :=
  aws_lines (tws_state state) =
    replay_physical_line_cache (tws_objects state) [].

Definition traced_writer_full_invariant (state : traced_writer_state) : Prop :=
  traced_writer_invariant state /\ traced_writer_cache_consistent state.

Definition traced_push_pending_node (node : pending_node)
    (state : traced_writer_state) : nat * traced_writer_state :=
  let '(id, after) := push_pending_node node (tws_state state) in
  (id,
   {| tws_state := after;
      tws_objects := tws_objects state ++ [WriterNodeObject id node] |}).

Definition traced_add_or_reuse_line (line_hash : list bbyte -> nat)
    (line : list bbyte) (state : traced_writer_state)
    : nat * traced_writer_state :=
  let before := tws_state state in
  let '(id, after) := add_or_reuse_line line_hash line before in
  let objects :=
    if Nat.eqb (writer_position after) (writer_position before)
    then tws_objects state
    else tws_objects state ++ [WriterLineObject id (line_hash line) line] in
  (id, {| tws_state := after; tws_objects := objects |}).

Fixpoint traced_push_pending_children (labels : list bbyte)
    (nodes : list pending_node) (state : traced_writer_state)
    : option (list (bbyte * nat) * traced_writer_state) :=
  match labels, nodes with
  | [], [] => Some ([], state)
  | label :: label_rest, node :: node_rest =>
      let '(id, after_node) := traced_push_pending_node node state in
      match traced_push_pending_children label_rest node_rest after_node with
      | None => None
      | Some (children, final) => Some ((label, id) :: children, final)
      end
  | _, _ => None
  end.

Section TracedSourceWriter.

Variable traced_line_hash : list bbyte -> nat.

Fixpoint traced_build_pending_tree (tree : writer_tree)
    (state : traced_writer_state)
    : option (pending_node * traced_writer_state) :=
  match tree with
  | WriterNode line value children =>
      match traced_build_pending_forest children state with
      | None => None
      | Some (child_nodes, after_descendants) =>
          match traced_push_pending_children (forest_labels children) child_nodes
                  after_descendants with
          | None => None
          | Some (child_ids, after_children) =>
              match line with
              | [] => Some (PendingBranch value child_ids, after_children)
              | _ :: _ =>
                  let '(line_id, after_line) :=
                    traced_add_or_reuse_line traced_line_hash line after_children in
                  match child_ids with
                  | [] => Some (PendingLine value None line_id, after_line)
                  | _ :: _ =>
                      let '(wrapper_id, after_wrapper) :=
                        traced_push_pending_node
                          (PendingBranch value child_ids) after_line in
                      Some (PendingLine None (Some wrapper_id) line_id,
                            after_wrapper)
                  end
              end
          end
      end
  end
with traced_build_pending_forest (forest : writer_forest)
    (state : traced_writer_state)
    : option (list pending_node * traced_writer_state) :=
  match forest with
  | WriterNil => Some ([], state)
  | WriterCons _ child rest =>
      match traced_build_pending_tree child state with
      | None => None
      | Some (node, after_child) =>
          match traced_build_pending_forest rest after_child with
          | None => None
          | Some (nodes, final) => Some (node :: nodes, final)
          end
      end
  end.

Definition encode_writer_tree_with_certificate (tree : writer_tree)
    : option (list bbyte * list physical_writer_object) :=
  let initial := {| tws_state := empty_writer_state; tws_objects := [] |} in
  match traced_build_pending_tree tree initial with
  | None => None
  | Some (root, before_root) =>
      let '(root_id, final) := traced_push_pending_node root before_root in
      Some (ACT_MAGIC ++ le_fixed 8 root_id ++
            aws_arena (tws_state final) ++ ACT_TRAILER,
            tws_objects final)
  end.

End TracedSourceWriter.

Lemma traced_push_pending_node_erases : forall node state id after,
  traced_push_pending_node node state = (id, after) ->
  push_pending_node node (tws_state state) = (id, tws_state after).
Proof.
  intros node state id after traced.
  unfold traced_push_pending_node in traced.
  destruct (push_pending_node node (tws_state state)) as [actualId actualAfter]
    eqn:pushed.
  inversion traced; subst. reflexivity.
Qed.

Lemma traced_add_or_reuse_line_erases : forall line_hash line state id after,
  traced_add_or_reuse_line line_hash line state = (id, after) ->
  add_or_reuse_line line_hash line (tws_state state) =
    (id, tws_state after).
Proof.
  intros line_hash line state id after traced.
  unfold traced_add_or_reuse_line in traced.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [actualId actualAfter] eqn:added.
  inversion traced; subst. reflexivity.
Qed.

Lemma physical_writer_bytes_app : forall left right,
  physical_writer_bytes (left ++ right) =
  physical_writer_bytes left ++ physical_writer_bytes right.
Proof.
  intros left right. unfold physical_writer_bytes.
  rewrite map_app, concat_app. reflexivity.
Qed.

Lemma physical_writer_bytes_singleton : forall object,
  physical_writer_bytes [object] = physical_writer_object_bytes object.
Proof.
  intro object. unfold physical_writer_bytes. cbn. apply app_nil_r.
Qed.

Lemma physical_writer_bytes_cons : forall object rest,
  physical_writer_bytes (object :: rest) =
    physical_writer_object_bytes object ++ physical_writer_bytes rest.
Proof. reflexivity. Qed.

Lemma replay_physical_line_cache_app : forall left right cache,
  replay_physical_line_cache (left ++ right) cache =
  replay_physical_line_cache right
    (replay_physical_line_cache left cache).
Proof.
  induction left as [|object rest IH]; intros right cache; [reflexivity|].
  destruct object; cbn; apply IH.
Qed.

Lemma physical_writer_objects_layout_snoc : forall position objects object,
  physical_writer_objects_layout position objects ->
  physical_writer_object_start object =
    position + length (physical_writer_bytes objects) ->
  physical_writer_objects_layout position (objects ++ [object]).
Proof.
  intros position objects. revert position.
  induction objects as [|head rest IH]; intros position object layout objectAtEnd.
  - cbn [physical_writer_objects_layout physical_writer_bytes] in *.
    split; [now rewrite Nat.add_0_r in objectAtEnd|exact I].
  - cbn [physical_writer_objects_layout] in layout |- *.
    destruct layout as [headAtPosition restLayout]. split; [exact headAtPosition|].
    unfold physical_writer_bytes in objectAtEnd. cbn in objectAtEnd.
    rewrite length_app in objectAtEnd.
    eapply IH; [exact restLayout|].
    unfold physical_writer_bytes. lia.
Qed.

(** The canonical producer domain contains only facts available before
    parsing.  [writer_size_upper_bound] is deliberately conservative and
    hash-independent: every semantic tree node is charged enough space for a
    branch root, a possible line wrapper, two nine-byte ACT varints, one
    possible line object, and its literal line bytes. *)

Fixpoint writer_values_below_u64 (tree : writer_tree) : Prop :=
  match tree with
  | WriterNode _ value children =>
      (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) /\
      writer_forest_values_below_u64 children
  end
with writer_forest_values_below_u64 (forest : writer_forest) : Prop :=
  match forest with
  | WriterNil => True
  | WriterCons _ child rest =>
      writer_values_below_u64 child /\
      writer_forest_values_below_u64 rest
  end.

Fixpoint writer_has_no_value_free_unary (tree : writer_tree) : Prop :=
  match tree with
  | WriterNode _ value children =>
      ~ (value = None /\ length (forest_labels children) = 1) /\
      writer_forest_has_no_value_free_unary children
  end
with writer_forest_has_no_value_free_unary (forest : writer_forest) : Prop :=
  match forest with
  | WriterNil => True
  | WriterCons _ child rest =>
      writer_has_no_value_free_unary child /\
      writer_forest_has_no_value_free_unary rest
  end.

Fixpoint writer_size_upper_bound (tree : writer_tree) : nat :=
  match tree with
  | WriterNode line _ children =>
      96 + length line + writer_forest_size_upper_bound children
  end
with writer_forest_size_upper_bound (forest : writer_forest) : nat :=
  match forest with
  | WriterNil => 0
  | WriterCons _ child rest =>
      writer_size_upper_bound child + writer_forest_size_upper_bound rest
  end.

Definition canonical_writer_tree (tree : writer_tree) : Prop :=
  writer_well_formed tree /\
  writer_values_below_u64 tree /\
  writer_has_no_value_free_unary tree /\
  ACT_HEADER_LEN + writer_size_upper_bound tree + length ACT_TRAILER <
    Nat.pow 256 8.

(** Shape and byte-domain obligations are separated from the writer's
    conservative size precondition.  Forward production uses the stronger
    [canonical_writer_tree]; an accepted-image inverse must not pretend that
    the conservative upper bound is a necessary property of canonical ACT. *)
Definition canonical_writer_shape (tree : writer_tree) : Prop :=
  writer_well_formed tree /\
  writer_values_below_u64 tree /\
  writer_has_no_value_free_unary tree.

Definition byte_image (image : list bbyte) : Prop :=
  Forall (fun byte => byte < 256) image.

Lemma canonical_writer_tree_has_canonical_shape : forall tree,
  canonical_writer_tree tree -> canonical_writer_shape tree.
Proof. intros tree [wellFormed [values [noUnary _]]]. repeat split; assumption. Qed.

Lemma strongly_sorted_nat_lt_nodup : forall values,
  StronglySorted Nat.lt values -> NoDup values.
Proof.
  intros values sorted. induction sorted as [|head tail tailSorted IH allGreater].
  - constructor.
  - constructor; [|exact IH].
    intro present.
    apply Forall_forall with (x := head) in allGreater; [lia|exact present].
Qed.

Lemma bounded_bytes_in_full_alphabet : forall values,
  Forall (fun byte => byte < 256) values -> incl values (seq 0 256).
Proof.
  intros values bounded byte present.
  apply Forall_forall with (x := byte) in bounded; [|exact present].
  apply in_seq. lia.
Qed.

Theorem canonical_child_labels_have_at_most_256_members : forall line value children,
  canonical_writer_tree (WriterNode line value children) ->
  length (forest_labels children) <= 256.
Proof.
  intros line value children canonical.
  destruct canonical as [[_ [labelsSorted childrenWellFormed]] _].
  apply NoDup_incl_length with (l' := seq 0 256).
  - apply strongly_sorted_nat_lt_nodup. exact labelsSorted.
  - apply bounded_bytes_in_full_alphabet.
    clear labelsSorted.
    induction children as [|label child rest IH]; [constructor|].
    cbn in childrenWellFormed |- *.
    destruct childrenWellFormed as [labelByte [_ restWellFormed]].
    constructor; [exact labelByte|]. apply IH. exact restWellFormed.
Qed.

Lemma encode_act_varint_nonempty : forall value,
  encode_act_varint value <> [].
Proof.
  intro value. unfold encode_act_varint.
  destruct (value <=? ACT_VARINT_BIAS); [discriminate|].
  destruct (little_endian_digits 8 value); discriminate.
Qed.

Theorem every_pending_node_encoding_is_nonempty : forall origin node,
  encode_pending_node origin node <> [].
Proof.
  intros origin node. destruct node; cbn [encode_pending_node]; discriminate.
Qed.

Theorem every_line_object_encoding_is_nonempty : forall line,
  encode_act_varint (length line) ++ line <> [].
Proof.
  intros line empty.
  apply app_eq_nil in empty as [varintEmpty _].
  exact (encode_act_varint_nonempty (length line) varintEmpty).
Qed.

Lemma append_arena_position_exact : forall bytes state,
  writer_position (append_arena bytes state) =
    writer_position state + length bytes.
Proof.
  intros. unfold writer_position, append_arena. simpl.
  rewrite length_app. lia.
Qed.

Lemma push_pending_node_position_exact : forall node state id after,
  push_pending_node node state = (id, after) ->
  id = writer_position state /\
  writer_position after = writer_position state +
    length (encode_pending_node (writer_position state) node).
Proof.
  intros node state id after pushed.
  unfold push_pending_node in pushed. inversion pushed; subst.
  split; [reflexivity|apply append_arena_position_exact].
Qed.

Lemma add_or_reuse_line_arena_effect : forall line_hash line state id after,
  add_or_reuse_line line_hash line state = (id, after) ->
  after = state \/
  (id = writer_position state /\
   aws_arena after = aws_arena state ++
     encode_act_varint (length line) ++ line).
Proof.
  intros line_hash line state id after added.
  unfold add_or_reuse_line in added.
  destruct (lookup_line_hash (line_hash line) (aws_lines state)) as [previous|]
    eqn:found.
  - destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line);
      inversion added; subst; auto.
  - inversion added; subst; auto.
Qed.

Lemma add_or_reuse_line_cache_effect : forall line_hash line state id after,
  add_or_reuse_line line_hash line state = (id, after) ->
  after = state \/
  (id = writer_position state /\
   aws_lines after = put_line_hash
     {| lce_hash := line_hash line; lce_bytes := line; lce_id := id |}
     (aws_lines state)).
Proof.
  intros line_hash line state id after added.
  unfold add_or_reuse_line in added.
  destruct (lookup_line_hash (line_hash line) (aws_lines state)) as [previous|]
    eqn:found.
  - destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line);
      inversion added; subst; auto.
  - inversion added; subst; auto.
Qed.

Theorem traced_push_pending_node_preserves_consistency : forall node state,
  traced_writer_consistent state ->
  traced_writer_consistent (snd (traced_push_pending_node node state)).
Proof.
  intros node state consistent.
  unfold traced_push_pending_node.
  destruct (push_pending_node node (tws_state state)) as [id after]
    eqn:pushed. cbn.
  unfold push_pending_node in pushed.
  inversion pushed; subst id after; clear pushed.
  unfold traced_writer_consistent in *.
  cbn [tws_state tws_objects].
  rewrite physical_writer_bytes_app. cbn [physical_writer_bytes].
  unfold append_arena. cbn. rewrite consistent, app_nil_r. reflexivity.
Qed.

Theorem traced_add_or_reuse_line_preserves_consistency : forall line_hash line state,
  traced_writer_consistent state ->
  traced_writer_consistent
    (snd (traced_add_or_reuse_line line_hash line state)).
Proof.
  intros line_hash line state consistent.
  unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [id after] eqn:added. cbn.
  pose proof (add_or_reuse_line_arena_effect
    line_hash line (tws_state state) id after added) as effect.
  destruct effect as [reused|[idExact arenaExact]].
  - subst after. rewrite Nat.eqb_refl. exact consistent.
  - assert (length (aws_arena after) <>
            length (aws_arena (tws_state state))) as arenaLengthChanged.
    { intro sameLength.
      rewrite arenaExact, !length_app in sameLength.
      pose proof (every_line_object_encoding_is_nonempty line) as nonempty.
      assert (0 < length (encode_act_varint (length line) ++ line)) by
        (destruct (encode_act_varint (length line) ++ line);
         [contradiction|cbn; lia]).
      rewrite length_app in H. lia. }
    apply Nat.eqb_neq in arenaLengthChanged. rewrite arenaLengthChanged.
    unfold traced_writer_consistent in *.
    cbn [tws_state tws_objects].
    rewrite physical_writer_bytes_app, physical_writer_bytes_singleton.
    cbn [physical_writer_object_bytes].
    rewrite arenaExact, consistent, app_assoc. reflexivity.
Qed.

Theorem traced_push_pending_node_preserves_layout : forall node state,
  traced_writer_consistent state ->
  traced_writer_layout state ->
  traced_writer_layout (snd (traced_push_pending_node node state)).
Proof.
  intros node state consistent layout.
  unfold traced_push_pending_node.
  destruct (push_pending_node node (tws_state state)) as [id after]
    eqn:pushed. cbn.
  unfold traced_writer_layout in *.
  apply physical_writer_objects_layout_snoc; [exact layout|].
  cbn [physical_writer_object_start].
  pose proof (push_pending_node_position_exact
    node (tws_state state) id after pushed) as [idExact _].
  rewrite idExact. unfold writer_position.
  unfold traced_writer_consistent in consistent. rewrite consistent. reflexivity.
Qed.

Theorem traced_add_or_reuse_line_preserves_layout : forall line_hash line state,
  traced_writer_consistent state ->
  traced_writer_layout state ->
  traced_writer_layout
    (snd (traced_add_or_reuse_line line_hash line state)).
Proof.
  intros line_hash line state consistent layout.
  unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [id after] eqn:added. cbn.
  pose proof (add_or_reuse_line_arena_effect
    line_hash line (tws_state state) id after added) as effect.
  destruct effect as [reused|[idExact arenaExact]].
  - subst after. rewrite Nat.eqb_refl. exact layout.
  - assert (length (aws_arena after) <>
            length (aws_arena (tws_state state))) as arenaLengthChanged.
    { intro sameLength.
      rewrite arenaExact, !length_app in sameLength.
      pose proof (every_line_object_encoding_is_nonempty line) as nonempty.
      assert (0 < length (encode_act_varint (length line) ++ line)) by
        (destruct (encode_act_varint (length line) ++ line);
         [contradiction|cbn; lia]).
      rewrite length_app in H. lia. }
    apply Nat.eqb_neq in arenaLengthChanged. rewrite arenaLengthChanged.
    unfold traced_writer_layout in *.
    apply physical_writer_objects_layout_snoc; [exact layout|].
    cbn [physical_writer_object_start]. rewrite idExact.
    unfold writer_position. unfold traced_writer_consistent in consistent.
    rewrite consistent. reflexivity.
Qed.

Theorem traced_push_pending_node_preserves_invariant : forall node state,
  traced_writer_invariant state ->
  traced_writer_invariant (snd (traced_push_pending_node node state)).
Proof.
  intros node state [consistent layout]. split.
  - apply traced_push_pending_node_preserves_consistency. exact consistent.
  - apply traced_push_pending_node_preserves_layout; assumption.
Qed.

Theorem traced_add_or_reuse_line_preserves_invariant : forall line_hash line state,
  traced_writer_invariant state ->
  traced_writer_invariant
    (snd (traced_add_or_reuse_line line_hash line state)).
Proof.
  intros line_hash line state [consistent layout]. split.
  - apply traced_add_or_reuse_line_preserves_consistency. exact consistent.
  - apply traced_add_or_reuse_line_preserves_layout; assumption.
Qed.

Theorem traced_push_pending_node_preserves_cache_consistency : forall node state,
  traced_writer_cache_consistent state ->
  traced_writer_cache_consistent
    (snd (traced_push_pending_node node state)).
Proof.
  intros node state cacheConsistent.
  unfold traced_push_pending_node.
  destruct (push_pending_node node (tws_state state)) as [id after]
    eqn:pushed. cbn.
  unfold push_pending_node in pushed. inversion pushed; subst id after.
  unfold traced_writer_cache_consistent in *.
  cbn [tws_state tws_objects append_arena].
  rewrite replay_physical_line_cache_app. cbn.
  exact cacheConsistent.
Qed.

Theorem traced_add_or_reuse_line_preserves_cache_consistency :
  forall line_hash line state,
    traced_writer_cache_consistent state ->
    traced_writer_cache_consistent
      (snd (traced_add_or_reuse_line line_hash line state)).
Proof.
  intros line_hash line state cacheConsistent.
  unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [id after] eqn:added. cbn.
  pose proof (add_or_reuse_line_arena_effect
    line_hash line (tws_state state) id after added) as arenaEffect.
  pose proof (add_or_reuse_line_cache_effect
    line_hash line (tws_state state) id after added) as cacheEffect.
  destruct arenaEffect as [reused|[idExact arenaExact]].
  - subst after. rewrite Nat.eqb_refl. exact cacheConsistent.
  - assert (length (aws_arena after) <>
            length (aws_arena (tws_state state))) as arenaLengthChanged.
    { intro sameLength.
      rewrite arenaExact, !length_app in sameLength.
      pose proof (every_line_object_encoding_is_nonempty line) as nonempty.
      assert (0 < length (encode_act_varint (length line) ++ line)) by
        (destruct (encode_act_varint (length line) ++ line);
         [contradiction|cbn; lia]).
      rewrite length_app in H. lia. }
    apply Nat.eqb_neq in arenaLengthChanged. rewrite arenaLengthChanged.
    destruct cacheEffect as [impossible|[_ cacheExact]].
    { subst after. apply Nat.eqb_neq in arenaLengthChanged.
      exfalso. exact (arenaLengthChanged eq_refl). }
    unfold traced_writer_cache_consistent in *.
    cbn [tws_state tws_objects].
    rewrite replay_physical_line_cache_app. cbn.
    rewrite cacheExact, cacheConsistent. reflexivity.
Qed.

Theorem traced_push_pending_node_preserves_full_invariant : forall node state,
  traced_writer_full_invariant state ->
  traced_writer_full_invariant (snd (traced_push_pending_node node state)).
Proof.
  intros node state [invariant cacheConsistent]. split.
  - apply traced_push_pending_node_preserves_invariant. exact invariant.
  - apply traced_push_pending_node_preserves_cache_consistency.
    exact cacheConsistent.
Qed.

Theorem traced_add_or_reuse_line_preserves_full_invariant :
  forall line_hash line state,
    traced_writer_full_invariant state ->
    traced_writer_full_invariant
      (snd (traced_add_or_reuse_line line_hash line state)).
Proof.
  intros line_hash line state [invariant cacheConsistent]. split.
  - apply traced_add_or_reuse_line_preserves_invariant. exact invariant.
  - exact (traced_add_or_reuse_line_preserves_cache_consistency
      line_hash line state cacheConsistent).
Qed.

Theorem traced_push_pending_children_erases : forall labels nodes state children final,
  traced_push_pending_children labels nodes state = Some (children, final) ->
  push_pending_children labels nodes (tws_state state) =
    Some (children, tws_state final).
Proof.
  induction labels as [|label labels IH]; intros nodes state children final traced.
  - destruct nodes as [|node nodes];
      cbn [traced_push_pending_children] in traced; [|discriminate].
    inversion traced; subst. reflexivity.
  - destruct nodes as [|node nodes]; [cbn in traced; discriminate|].
    unfold traced_push_pending_children in traced.
    fold traced_push_pending_children in traced.
    destruct (traced_push_pending_node node state) as [id after_node]
      eqn:tracedNode.
    destruct (traced_push_pending_children labels nodes after_node)
      as [tail_result|] eqn:tracedTail.
    + destruct tail_result as [tail_children tail_final].
      pose proof (traced_push_pending_node_erases
        node state id after_node tracedNode) as erasedNode.
      pose proof (IH nodes after_node tail_children tail_final tracedTail)
        as erasedTail.
      inversion traced; subst; clear traced.
      cbn [push_pending_children]. rewrite erasedNode, erasedTail. reflexivity.
    + cbn in traced. discriminate.
Qed.

Theorem traced_push_pending_children_preserves_consistency :
  forall labels nodes state children final,
    traced_writer_consistent state ->
    traced_push_pending_children labels nodes state = Some (children, final) ->
    traced_writer_consistent final.
Proof.
  induction labels as [|label labels IH];
    intros nodes state children final consistent traced.
  - destruct nodes as [|node nodes];
      cbn [traced_push_pending_children] in traced; [|discriminate].
    inversion traced; subst. exact consistent.
  - destruct nodes as [|node nodes]; [cbn in traced; discriminate|].
    unfold traced_push_pending_children in traced.
    fold traced_push_pending_children in traced.
    destruct (traced_push_pending_node node state) as [id after_node]
      eqn:tracedNode.
    destruct (traced_push_pending_children labels nodes after_node)
      as [tail_result|] eqn:tracedTail.
    + destruct tail_result as [tail_children tail_final].
      assert (traced_writer_consistent after_node) as afterNodeConsistent.
      { pose proof (traced_push_pending_node_preserves_consistency node state consistent)
          as preserved.
        rewrite tracedNode in preserved. exact preserved. }
      pose proof (IH nodes after_node tail_children tail_final
        afterNodeConsistent tracedTail) as tailConsistent.
      inversion traced; subst; clear traced. exact tailConsistent.
    + cbn in traced. discriminate.
Qed.

Theorem traced_push_pending_children_preserves_invariant :
  forall labels nodes state children final,
    traced_writer_invariant state ->
    traced_push_pending_children labels nodes state = Some (children, final) ->
    traced_writer_invariant final.
Proof.
  induction labels as [|label labels IH];
    intros nodes state children final invariant traced.
  - destruct nodes as [|node nodes];
      cbn [traced_push_pending_children] in traced; [|discriminate].
    inversion traced; subst. exact invariant.
  - destruct nodes as [|node nodes]; [cbn in traced; discriminate|].
    unfold traced_push_pending_children in traced.
    fold traced_push_pending_children in traced.
    destruct (traced_push_pending_node node state) as [id after_node]
      eqn:tracedNode.
    destruct (traced_push_pending_children labels nodes after_node)
      as [tail_result|] eqn:tracedTail.
    + destruct tail_result as [tail_children tail_final].
      assert (traced_writer_invariant after_node) as afterNodeInvariant.
      { pose proof (traced_push_pending_node_preserves_invariant
          node state invariant) as preserved.
        rewrite tracedNode in preserved. exact preserved. }
      pose proof (IH nodes after_node tail_children tail_final
        afterNodeInvariant tracedTail) as tailInvariant.
      inversion traced; subst; clear traced. exact tailInvariant.
    + cbn in traced. discriminate.
Qed.

Theorem traced_push_pending_children_preserves_full_invariant :
  forall labels nodes state children final,
    traced_writer_full_invariant state ->
    traced_push_pending_children labels nodes state = Some (children, final) ->
    traced_writer_full_invariant final.
Proof.
  induction labels as [|label labels IH];
    intros nodes state children final invariant traced.
  - destruct nodes as [|node nodes];
      cbn [traced_push_pending_children] in traced; [|discriminate].
    inversion traced; subst. exact invariant.
  - destruct nodes as [|node nodes]; [cbn in traced; discriminate|].
    unfold traced_push_pending_children in traced.
    fold traced_push_pending_children in traced.
    destruct (traced_push_pending_node node state) as [id after_node]
      eqn:tracedNode.
    destruct (traced_push_pending_children labels nodes after_node)
      as [tail_result|] eqn:tracedTail.
    + destruct tail_result as [tail_children tail_final].
      assert (traced_writer_full_invariant after_node) as afterNodeInvariant.
      { pose proof (traced_push_pending_node_preserves_full_invariant
          node state invariant) as preserved.
        rewrite tracedNode in preserved. exact preserved. }
      pose proof (IH nodes after_node tail_children tail_final
        afterNodeInvariant tracedTail) as tailInvariant.
      inversion traced; subst; clear traced. exact tailInvariant.
    + cbn in traced. discriminate.
Qed.

Lemma traced_push_pending_children_total : forall labels nodes state,
  length labels = length nodes ->
  exists children final,
    traced_push_pending_children labels nodes state = Some (children, final) /\
    map fst children = labels /\ length children = length nodes.
Proof.
  induction labels as [|label labels IH]; intros nodes state sameLength.
  - destruct nodes; [|discriminate].
    exists [], state. repeat split; reflexivity.
  - destruct nodes as [|node nodes]; [discriminate|].
    simpl in sameLength. inversion sameLength as [tailLength].
    destruct (traced_push_pending_node node state) as [id afterNode]
      eqn:tracedNode.
    destruct (IH nodes afterNode tailLength) as
      [children [final [tailRun [tailLabels tailCount]]]].
    exists ((label, id) :: children), final.
    cbn [traced_push_pending_children]. rewrite tracedNode, tailRun.
    repeat split; simpl; congruence.
Qed.

Lemma push_pending_children_total : forall labels nodes state,
  length labels = length nodes ->
  exists children final,
    push_pending_children labels nodes state = Some (children, final) /\
    map fst children = labels /\ length children = length nodes.
Proof.
  induction labels as [|label labels IH]; intros nodes state sameLength.
  - destruct nodes; [|discriminate].
    exists [], state. repeat split; reflexivity.
  - destruct nodes as [|node nodes]; [discriminate|].
    simpl in sameLength. inversion sameLength as [tailLength].
    destruct (push_pending_node node state) as [id afterNode] eqn:pushed.
    destruct (IH nodes afterNode tailLength) as
      [children [final [tailRun [tailLabels tailCount]]]].
    exists ((label, id) :: children), final.
    cbn [push_pending_children]. rewrite pushed, tailRun.
    repeat split; simpl; congruence.
Qed.

Theorem source_writer_is_total : forall line_hash,
  (forall tree state,
      exists root final,
        build_pending_tree line_hash tree state = Some (root, final)) /\
  (forall forest state,
      exists nodes final,
        build_pending_forest line_hash forest state = Some (nodes, final) /\
        length nodes = length (forest_labels forest)).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state.
    destruct (childrenIH state) as
      [childNodes [afterDescendants [childrenRun childrenLength]]].
    destruct (push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [pushRun [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    destruct line as [|first rest].
    + cbn [build_pending_tree]. rewrite childrenRun, pushRun. eauto.
    + destruct (add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      cbn [build_pending_tree]. rewrite childrenRun, pushRun, lineRun.
      destruct childIds as [|childId childRest].
      * eauto.
      * destruct (push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:wrapperRun.
        eauto.
  - intro state. exists [], state. split; reflexivity.
  - intros label child childIH rest restIH state.
    destruct (childIH state) as [childNode [afterChild childRun]].
    destruct (restIH afterChild) as
      [restNodes [final [restRun restLength]]].
    exists (childNode :: restNodes), final.
    cbn [build_pending_forest]. rewrite childRun, restRun.
    split; [reflexivity|]. simpl. now rewrite restLength.
Qed.

Theorem traced_source_writer_is_total : forall line_hash,
  (forall tree state,
      exists root final,
        traced_build_pending_tree line_hash tree state = Some (root, final)) /\
  (forall forest state,
      exists nodes final,
        traced_build_pending_forest line_hash forest state = Some (nodes, final) /\
        length nodes = length (forest_labels forest)).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state.
    destruct (childrenIH state) as
      [childNodes [afterDescendants [childrenRun childrenLength]]].
    destruct (traced_push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [pushRun [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    destruct line as [|first rest].
    + cbn [traced_build_pending_tree]. rewrite childrenRun, pushRun. eauto.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      cbn [traced_build_pending_tree]. rewrite childrenRun, pushRun, lineRun.
      destruct childIds as [|childId childRest].
      * eauto.
      * destruct (traced_push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:wrapperRun.
        eauto.
  - intro state. exists [], state. split; reflexivity.
  - intros label child childIH rest restIH state.
    destruct (childIH state) as [childNode [afterChild childRun]].
    destruct (restIH afterChild) as
      [restNodes [final [restRun restLength]]].
    exists (childNode :: restNodes), final.
    cbn [traced_build_pending_forest]. rewrite childRun, restRun.
    split; [reflexivity|]. simpl. now rewrite restLength.
Qed.

Theorem traced_source_writer_is_total_and_consistent : forall line_hash,
  (forall tree state,
      traced_writer_consistent state ->
      exists root final,
        traced_build_pending_tree line_hash tree state = Some (root, final) /\
        traced_writer_consistent final) /\
  (forall forest state,
      traced_writer_consistent state ->
      exists nodes final,
        traced_build_pending_forest line_hash forest state = Some (nodes, final) /\
        length nodes = length (forest_labels forest) /\
        traced_writer_consistent final).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state stateConsistent.
    destruct (childrenIH state stateConsistent) as
      [childNodes [afterDescendants
        [childrenRun [childrenLength descendantsConsistent]]]].
    destruct (traced_push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [pushRun [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    pose proof (traced_push_pending_children_preserves_consistency
      (forest_labels children) childNodes afterDescendants childIds afterChildren
      descendantsConsistent pushRun) as childrenConsistent.
    destruct line as [|first rest].
    + exists (PendingBranch value childIds), afterChildren.
      split; [|exact childrenConsistent].
      cbn [traced_build_pending_tree]. rewrite childrenRun, pushRun. reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      assert (traced_writer_consistent afterLine) as lineConsistent.
      { pose proof (traced_add_or_reuse_line_preserves_consistency
          line_hash (first :: rest) afterChildren childrenConsistent) as preserved.
        rewrite lineRun in preserved. exact preserved. }
      destruct childIds as [|childId childRest].
      * exists (PendingLine value None lineId), afterLine.
        split; [|exact lineConsistent].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun. reflexivity.
      * destruct (traced_push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:wrapperRun.
        assert (traced_writer_consistent afterWrapper) as wrapperConsistent.
        { pose proof (traced_push_pending_node_preserves_consistency
            (PendingBranch value (childId :: childRest)) afterLine lineConsistent)
            as preserved.
          rewrite wrapperRun in preserved. exact preserved. }
        exists (PendingLine None (Some wrapper) lineId), afterWrapper.
        split; [|exact wrapperConsistent].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun, wrapperRun. reflexivity.
  - intros state stateConsistent.
    exists [], state. repeat split; assumption || reflexivity.
  - intros label child childIH rest restIH state stateConsistent.
    destruct (childIH state stateConsistent) as
      [childNode [afterChild [childRun childConsistent]]].
    destruct (restIH afterChild childConsistent) as
      [restNodes [final [restRun [restLength finalConsistent]]]].
    exists (childNode :: restNodes), final.
    split.
    + cbn [traced_build_pending_forest]. rewrite childRun, restRun. reflexivity.
    + split; [|exact finalConsistent]. simpl. now rewrite restLength.
Qed.

Theorem traced_source_writer_is_total_and_invariant : forall line_hash,
  (forall tree state,
      traced_writer_invariant state ->
      exists root final,
        traced_build_pending_tree line_hash tree state = Some (root, final) /\
        traced_writer_invariant final) /\
  (forall forest state,
      traced_writer_invariant state ->
      exists nodes final,
        traced_build_pending_forest line_hash forest state = Some (nodes, final) /\
        length nodes = length (forest_labels forest) /\
        traced_writer_invariant final).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state stateInvariant.
    destruct (childrenIH state stateInvariant) as
      [childNodes [afterDescendants
        [childrenRun [childrenLength descendantsInvariant]]]].
    destruct (traced_push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [pushRun [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    pose proof (traced_push_pending_children_preserves_invariant
      (forest_labels children) childNodes afterDescendants childIds afterChildren
      descendantsInvariant pushRun) as childrenInvariant.
    destruct line as [|first rest].
    + exists (PendingBranch value childIds), afterChildren.
      split; [|exact childrenInvariant].
      cbn [traced_build_pending_tree]. rewrite childrenRun, pushRun. reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      assert (traced_writer_invariant afterLine) as lineInvariant.
      { pose proof (traced_add_or_reuse_line_preserves_invariant
          line_hash (first :: rest) afterChildren childrenInvariant) as preserved.
        rewrite lineRun in preserved. exact preserved. }
      destruct childIds as [|childId childRest].
      * exists (PendingLine value None lineId), afterLine.
        split; [|exact lineInvariant].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun. reflexivity.
      * destruct (traced_push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:wrapperRun.
        assert (traced_writer_invariant afterWrapper) as wrapperInvariant.
        { pose proof (traced_push_pending_node_preserves_invariant
            (PendingBranch value (childId :: childRest)) afterLine lineInvariant)
            as preserved.
          rewrite wrapperRun in preserved. exact preserved. }
        exists (PendingLine None (Some wrapper) lineId), afterWrapper.
        split; [|exact wrapperInvariant].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun, wrapperRun. reflexivity.
  - intros state stateInvariant.
    exists [], state. split; [reflexivity|].
    split; [reflexivity|exact stateInvariant].
  - intros label child childIH rest restIH state stateInvariant.
    destruct (childIH state stateInvariant) as
      [childNode [afterChild [childRun childInvariant]]].
    destruct (restIH afterChild childInvariant) as
      [restNodes [final [restRun [restLength finalInvariant]]]].
    exists (childNode :: restNodes), final.
    split.
    + cbn [traced_build_pending_forest]. rewrite childRun, restRun. reflexivity.
    + split; [|exact finalInvariant]. simpl. now rewrite restLength.
Qed.

Theorem traced_source_writer_is_total_and_full_invariant : forall line_hash,
  (forall tree state,
      traced_writer_full_invariant state ->
      exists root final,
        traced_build_pending_tree line_hash tree state = Some (root, final) /\
        traced_writer_full_invariant final) /\
  (forall forest state,
      traced_writer_full_invariant state ->
      exists nodes final,
        traced_build_pending_forest line_hash forest state = Some (nodes, final) /\
        length nodes = length (forest_labels forest) /\
        traced_writer_full_invariant final).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state stateInvariant.
    destruct (childrenIH state stateInvariant) as
      [childNodes [afterDescendants
        [childrenRun [childrenLength descendantsInvariant]]]].
    destruct (traced_push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [pushRun [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    pose proof (traced_push_pending_children_preserves_full_invariant
      (forest_labels children) childNodes afterDescendants childIds afterChildren
      descendantsInvariant pushRun) as childrenInvariant.
    destruct line as [|first rest].
    + exists (PendingBranch value childIds), afterChildren.
      split; [|exact childrenInvariant].
      cbn [traced_build_pending_tree]. rewrite childrenRun, pushRun. reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      assert (traced_writer_full_invariant afterLine) as lineInvariant.
      { pose proof (traced_add_or_reuse_line_preserves_full_invariant
          line_hash (first :: rest) afterChildren childrenInvariant) as preserved.
        rewrite lineRun in preserved. exact preserved. }
      destruct childIds as [|childId childRest].
      * exists (PendingLine value None lineId), afterLine.
        split; [|exact lineInvariant].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun. reflexivity.
      * destruct (traced_push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:wrapperRun.
        assert (traced_writer_full_invariant afterWrapper) as wrapperInvariant.
        { pose proof (traced_push_pending_node_preserves_full_invariant
            (PendingBranch value (childId :: childRest)) afterLine lineInvariant)
            as preserved.
          rewrite wrapperRun in preserved. exact preserved. }
        exists (PendingLine None (Some wrapper) lineId), afterWrapper.
        split; [|exact wrapperInvariant].
        cbn [traced_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun, wrapperRun. reflexivity.
  - intros state stateInvariant.
    exists [], state. split; [reflexivity|].
    split; [reflexivity|exact stateInvariant].
  - intros label child childIH rest restIH state stateInvariant.
    destruct (childIH state stateInvariant) as
      [childNode [afterChild [childRun childInvariant]]].
    destruct (restIH afterChild childInvariant) as
      [restNodes [final [restRun [restLength finalInvariant]]]].
    exists (childNode :: restNodes), final.
    split.
    + cbn [traced_build_pending_forest]. rewrite childRun, restRun. reflexivity.
    + split; [|exact finalInvariant]. simpl. now rewrite restLength.
Qed.

Theorem traced_source_writer_erases_exactly : forall line_hash,
  (forall tree state,
      exists root final,
        traced_build_pending_tree line_hash tree state = Some (root, final) /\
        build_pending_tree line_hash tree (tws_state state) =
          Some (root, tws_state final)) /\
  (forall forest state,
      exists nodes final,
        traced_build_pending_forest line_hash forest state = Some (nodes, final) /\
        build_pending_forest line_hash forest (tws_state state) =
          Some (nodes, tws_state final) /\
        length nodes = length (forest_labels forest)).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state.
    destruct (childrenIH state) as
      [childNodes [afterDescendants
        [tracedChildren [sourceChildren childrenLength]]]].
    destruct (traced_push_pending_children_total
      (forest_labels children) childNodes afterDescendants)
      as [childIds [afterChildren [tracedPush [labelsExact idsLength]]]];
      [symmetry; exact childrenLength|].
    pose proof (traced_push_pending_children_erases
      (forest_labels children) childNodes afterDescendants childIds afterChildren
      tracedPush) as sourcePush.
    destruct line as [|first rest].
    + exists (PendingBranch value childIds), afterChildren.
      split.
      * cbn [traced_build_pending_tree].
        rewrite tracedChildren, tracedPush. reflexivity.
      * cbn [build_pending_tree]. rewrite sourceChildren, sourcePush. reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:tracedLine.
      pose proof (traced_add_or_reuse_line_erases line_hash (first :: rest)
        afterChildren lineId afterLine tracedLine) as sourceLine.
      destruct childIds as [|childId childRest].
      * exists (PendingLine value None lineId), afterLine.
        split.
        -- cbn [traced_build_pending_tree].
           rewrite tracedChildren, tracedPush, tracedLine. reflexivity.
        -- cbn [build_pending_tree].
           rewrite sourceChildren, sourcePush, sourceLine. reflexivity.
      * destruct (traced_push_pending_node
          (PendingBranch value (childId :: childRest)) afterLine)
          as [wrapper afterWrapper] eqn:tracedWrapper.
        pose proof (traced_push_pending_node_erases
          (PendingBranch value (childId :: childRest)) afterLine
          wrapper afterWrapper tracedWrapper) as sourceWrapper.
        exists (PendingLine None (Some wrapper) lineId), afterWrapper.
        split.
        -- cbn [traced_build_pending_tree].
           rewrite tracedChildren, tracedPush, tracedLine, tracedWrapper.
           reflexivity.
        -- cbn [build_pending_tree].
           rewrite sourceChildren, sourcePush, sourceLine, sourceWrapper.
           reflexivity.
  - intro state. exists [], state. repeat split; reflexivity.
  - intros label child childIH rest restIH state.
    destruct (childIH state) as
      [childNode [afterChild [tracedChild sourceChild]]].
    destruct (restIH afterChild) as
      [restNodes [final [tracedRest [sourceRest restLength]]]].
    exists (childNode :: restNodes), final.
    split.
    + cbn [traced_build_pending_forest].
      rewrite tracedChild, tracedRest. reflexivity.
    + split.
      * cbn [build_pending_forest]. rewrite sourceChild, sourceRest. reflexivity.
      * simpl. now rewrite restLength.
Qed.

Corollary encode_writer_tree_with_certificate_is_total : forall line_hash tree,
  exists bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects).
Proof.
  intros line_hash tree.
  destruct (proj1 (traced_source_writer_is_total line_hash) tree
    {| tws_state := empty_writer_state; tws_objects := [] |})
    as [root [beforeRoot built]].
  unfold encode_writer_tree_with_certificate. rewrite built.
  destruct (traced_push_pending_node root beforeRoot) as [rootId final]. eauto.
Qed.

Theorem encode_writer_tree_with_certificate_erases_exactly :
  forall line_hash tree bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) ->
    encode_writer_tree line_hash tree = Some bytes.
Proof.
  intros line_hash tree bytes objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  destruct (proj1 (traced_source_writer_erases_exactly line_hash) tree initial)
    as [root [beforeRoot [tracedBuilt sourceBuilt]]].
  unfold encode_writer_tree_with_certificate in encoded. fold initial in encoded.
  rewrite tracedBuilt in encoded.
  destruct (traced_push_pending_node root beforeRoot) as [rootId final]
    eqn:tracedRoot.
  pose proof (traced_push_pending_node_erases
    root beforeRoot rootId final tracedRoot) as sourceRoot.
  inversion encoded; subst bytes objects.
  unfold initial in sourceBuilt. cbn in sourceBuilt.
  unfold encode_writer_tree. rewrite sourceBuilt, sourceRoot. reflexivity.
Qed.

Theorem source_certificate_has_exact_physical_bytes : forall line_hash tree,
  exists bytes objects root_id root prefix,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) /\
    objects = prefix ++ [WriterNodeObject root_id root] /\
    bytes = ACT_MAGIC ++ le_fixed 8 root_id ++
      physical_writer_bytes objects ++ ACT_TRAILER.
Proof.
  intros line_hash tree.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  assert (traced_writer_consistent initial) as initialConsistent.
  { unfold initial, traced_writer_consistent, empty_writer_state,
      physical_writer_bytes. reflexivity. }
  destruct (proj1 (traced_source_writer_is_total_and_consistent line_hash)
    tree initial initialConsistent) as
    [root [beforeRoot [built beforeRootConsistent]]].
  destruct (traced_push_pending_node root beforeRoot) as [rootId final]
    eqn:rootRun.
  assert (traced_writer_consistent final) as finalConsistent.
  { pose proof (traced_push_pending_node_preserves_consistency
      root beforeRoot beforeRootConsistent) as preserved.
    rewrite rootRun in preserved. exact preserved. }
  exists (ACT_MAGIC ++ le_fixed 8 rootId ++
            aws_arena (tws_state final) ++ ACT_TRAILER),
    (tws_objects final), rootId, root, (tws_objects beforeRoot).
  split.
  - unfold encode_writer_tree_with_certificate. fold initial.
    rewrite built, rootRun. reflexivity.
  - split.
    + unfold traced_push_pending_node in rootRun.
      destruct (push_pending_node root (tws_state beforeRoot))
        as [actualId actualFinal] eqn:pushed.
      inversion rootRun; subst. reflexivity.
    + unfold traced_writer_consistent in finalConsistent.
      now rewrite finalConsistent.
Qed.

Fixpoint physical_node_object_count (objects : list physical_writer_object) : nat :=
  match objects with
  | [] => 0
  | WriterLineObject _ _ _ :: rest => physical_node_object_count rest
  | WriterNodeObject _ _ :: rest => S (physical_node_object_count rest)
  end.

Lemma every_physical_writer_object_encoding_is_nonempty : forall object,
  physical_writer_object_bytes object <> [].
Proof.
  intro object. destruct object as [id hash line|id node]; cbn.
  - apply every_line_object_encoding_is_nonempty.
  - apply every_pending_node_encoding_is_nonempty.
Qed.

Theorem physical_node_object_count_is_byte_bounded : forall objects,
  physical_node_object_count objects <= length (physical_writer_bytes objects).
Proof.
  induction objects as [|object rest IH]; [reflexivity|].
  unfold physical_writer_bytes in *. cbn in *.
  rewrite length_app.
  pose proof (every_physical_writer_object_encoding_is_nonempty object)
    as nonempty.
  assert (0 < length (physical_writer_object_bytes object)) by
    (destruct (physical_writer_object_bytes object);
     [contradiction|cbn; lia]).
  destruct object; cbn [physical_node_object_count] in *; lia.
Qed.

Lemma physical_writer_layout_member_start_lower_bound :
  forall position objects object,
    physical_writer_objects_layout position objects ->
    In object objects ->
    position <= physical_writer_object_start object.
Proof.
  intros position objects. revert position.
  induction objects as [|head rest IH]; intros position object layout present.
  - contradiction.
  - cbn [physical_writer_objects_layout] in layout.
    destruct layout as [headAt restLayout].
    cbn in present. destruct present as [present|present].
    + subst object. rewrite headAt. lia.
    + pose proof (IH
        (position + length (physical_writer_object_bytes head))
        object restLayout present) as lower. lia.
Qed.

Lemma physical_writer_layout_starts_strongly_sorted :
  forall position objects,
    physical_writer_objects_layout position objects ->
    StronglySorted Nat.lt (map physical_writer_object_start objects).
Proof.
  intros position objects. revert position.
  induction objects as [|head rest IH]; intros position layout.
  - constructor.
  - cbn [physical_writer_objects_layout] in layout.
    destruct layout as [headAt restLayout].
    cbn [map]. constructor.
    + exact (IH
        (position + length (physical_writer_object_bytes head)) restLayout).
    + apply Forall_forall. intros candidate present.
      apply in_map_iff in present as [object [candidateExact objectPresent]].
      subst candidate.
      pose proof (physical_writer_layout_member_start_lower_bound
        (position + length (physical_writer_object_bytes head)) rest object
        restLayout objectPresent) as lower.
      pose proof (every_physical_writer_object_encoding_is_nonempty head)
        as nonempty.
      assert (0 < length (physical_writer_object_bytes head)).
      { destruct (physical_writer_object_bytes head);
          [contradiction|cbn; lia]. }
      rewrite headAt. lia.
Qed.

Corollary physical_writer_layout_starts_nodup :
  forall position objects,
    physical_writer_objects_layout position objects ->
    NoDup (map physical_writer_object_start objects).
Proof.
  intros position objects layout.
  apply strongly_sorted_nat_lt_nodup.
  eapply physical_writer_layout_starts_strongly_sorted. exact layout.
Qed.

Theorem source_certificate_output_is_exact :
  forall line_hash tree bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) ->
    exists root_id root prefix,
      objects = prefix ++ [WriterNodeObject root_id root] /\
      bytes = ACT_MAGIC ++ le_fixed 8 root_id ++
        physical_writer_bytes objects ++ ACT_TRAILER.
Proof.
  intros line_hash tree bytes objects encoded.
  destruct (source_certificate_has_exact_physical_bytes line_hash tree)
    as [actualBytes [actualObjects [rootId [root [prefix
      [actualEncoded [objectsExact bytesExact]]]]]]].
  rewrite encoded in actualEncoded. inversion actualEncoded; subst.
  exists rootId, root, prefix. auto.
Qed.

Theorem source_certificate_objects_are_contiguous :
  forall line_hash tree bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) ->
    physical_writer_objects_layout ACT_HEADER_LEN objects.
Proof.
  intros line_hash tree bytes objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  assert (traced_writer_invariant initial) as initialInvariant.
  { split.
    - unfold initial, traced_writer_consistent, empty_writer_state,
        physical_writer_bytes. reflexivity.
    - unfold initial, traced_writer_layout. reflexivity. }
  destruct (proj1 (traced_source_writer_is_total_and_invariant line_hash)
    tree initial initialInvariant) as
    [root [beforeRoot [built beforeRootInvariant]]].
  destruct (traced_push_pending_node root beforeRoot) as [rootId final]
    eqn:rootRun.
  pose proof (traced_push_pending_node_preserves_invariant
    root beforeRoot beforeRootInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant.
  unfold encode_writer_tree_with_certificate in encoded. fold initial in encoded.
  rewrite built, rootRun in encoded. inversion encoded; subst bytes objects.
  exact (proj2 finalInvariant).
Qed.

Theorem source_certificate_cache_matches_recorded_objects :
  forall line_hash tree bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) ->
    exists root before_root root_id final,
      traced_build_pending_tree line_hash tree
        {| tws_state := empty_writer_state; tws_objects := [] |} =
          Some (root, before_root) /\
      traced_push_pending_node root before_root = (root_id, final) /\
      objects = tws_objects final /\
      aws_lines (tws_state final) =
        replay_physical_line_cache objects [].
Proof.
  intros line_hash tree bytes objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  assert (traced_writer_full_invariant initial) as initialInvariant.
  { repeat split; reflexivity. }
  destruct (proj1 (traced_source_writer_is_total_and_full_invariant line_hash)
    tree initial initialInvariant) as
    [root [beforeRoot [built beforeRootInvariant]]].
  destruct (traced_push_pending_node root beforeRoot) as [rootId final]
    eqn:rootRun.
  pose proof (traced_push_pending_node_preserves_full_invariant
    root beforeRoot beforeRootInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant.
  unfold encode_writer_tree_with_certificate in encoded. fold initial in encoded.
  rewrite built, rootRun in encoded. inversion encoded; subst bytes objects.
  exists root, beforeRoot, rootId, final.
  repeat split; try assumption.
  exact (proj2 finalInvariant).
Qed.

(** This bound is producer-side and pre-acceptance: it assumes only that the
    executable writer returned a certificate.  It does not assume parsing,
    discovery, validation, or token success. *)
Theorem canonical_writer_physical_node_count_le_image_bytes :
  forall line_hash tree bytes objects,
    encode_writer_tree_with_certificate line_hash tree = Some (bytes, objects) ->
    physical_node_object_count objects <= length bytes.
Proof.
  intros line_hash tree bytes objects encoded.
  destruct (source_certificate_output_is_exact
    line_hash tree bytes objects encoded) as
    [rootId [root [prefix [_ bytesExact]]]].
  pose proof (physical_node_object_count_is_byte_bounded objects) as objectBound.
  rewrite bytesExact, !length_app in *. lia.
Qed.

Corollary encode_writer_tree_is_total : forall line_hash tree,
  exists bytes, encode_writer_tree line_hash tree = Some bytes.
Proof.
  intros line_hash tree.
  destruct (proj1 (source_writer_is_total line_hash) tree empty_writer_state)
    as [root [beforeRoot built]].
  unfold encode_writer_tree. rewrite built.
  destruct (push_pending_node root beforeRoot) as [rootId final]. eauto.
Qed.

Definition fixture_line_hash (line : list bbyte) : nat := length line.

Definition three_way_writer_tree : writer_tree :=
  WriterNode [] None
    (WriterCons 10 (WriterNode [] (Some 0) WriterNil)
      (WriterCons 20 (WriterNode [] (Some 0) WriterNil)
        (WriterCons 30 (WriterNode [] (Some 0) WriterNil) WriterNil))).

Definition line_writer_tree : writer_tree :=
  WriterNode [97; 98] (Some 0) WriterNil.

Definition same_line_writer_tree : writer_tree :=
  WriterNode [] None
    (WriterCons 97 (WriterNode [120] (Some 0) WriterNil)
      (WriterCons 98 (WriterNode [120] (Some 1) WriterNil) WriterNil)).

Definition colliding_line_writer_tree : writer_tree :=
  WriterNode [] None
    (WriterCons 97 (WriterNode [120] (Some 0) WriterNil)
      (WriterCons 98 (WriterNode [121] (Some 1) WriterNil) WriterNil)).

Definition aba_colliding_line_writer_tree : writer_tree :=
  WriterNode [] None
    (WriterCons 97 (WriterNode [120] (Some 0) WriterNil)
      (WriterCons 98 (WriterNode [121] (Some 1) WriterNil)
        (WriterCons 99 (WriterNode [120] (Some 2) WriterNil) WriterNil))).

Definition forced_collision_hash (_line : list bbyte) : nat := 0.

Definition line_collision_mismatch (line_hash : list bbyte -> nat)
    (line : list bbyte) (state : act_writer_state) : bool :=
  match lookup_line_hash (line_hash line) (aws_lines state) with
  | None => false
  | Some previous =>
      if list_eq_dec Nat.eq_dec (lce_bytes previous) line then false else true
  end.

Definition collision_observed_add_or_reuse_line
    (line_hash : list bbyte -> nat) (line : list bbyte)
    (state : traced_writer_state) (count : nat)
    : (nat * traced_writer_state) * nat :=
  (traced_add_or_reuse_line line_hash line state,
   count + if line_collision_mismatch line_hash line (tws_state state)
           then 1 else 0).

Fixpoint traced_line_sequence (line_hash : list bbyte -> nat)
    (lines : list (list bbyte)) (state : traced_writer_state)
    : list nat * traced_writer_state :=
  match lines with
  | [] => ([], state)
  | line :: rest =>
      let '(id, after) := traced_add_or_reuse_line line_hash line state in
      let '(ids, final) := traced_line_sequence line_hash rest after in
      (id :: ids, final)
  end.

Fixpoint collision_observed_line_sequence (line_hash : list bbyte -> nat)
    (lines : list (list bbyte)) (state : traced_writer_state) (count : nat)
    : (list nat * traced_writer_state) * nat :=
  match lines with
  | [] => (([], state), count)
  | line :: rest =>
      let '((id, after), next) :=
        collision_observed_add_or_reuse_line line_hash line state count in
      let '((ids, final), total) :=
        collision_observed_line_sequence line_hash rest after next in
      ((id :: ids, final), total)
  end.

Theorem collision_observation_erases_traced_result :
  forall line_hash line state count,
  fst (collision_observed_add_or_reuse_line line_hash line state count) =
    traced_add_or_reuse_line line_hash line state.
Proof. reflexivity. Qed.

Theorem collision_observation_erases_writer_result :
  forall line_hash line state count id after total,
  collision_observed_add_or_reuse_line line_hash line state count =
    ((id, after), total) ->
  add_or_reuse_line line_hash line (tws_state state) = (id, tws_state after).
Proof.
  intros line_hash line state count id after total observed.
  apply traced_add_or_reuse_line_erases.
  exact (f_equal fst observed).
Qed.

Theorem collision_observation_counts_exactly_unequal_bucket_hits :
  forall line_hash line state,
  line_collision_mismatch line_hash line state = true <->
  exists previous,
    lookup_line_hash (line_hash line) (aws_lines state) = Some previous /\
    lce_bytes previous <> line.
Proof.
  intros line_hash line state. unfold line_collision_mismatch.
  destruct (lookup_line_hash (line_hash line) (aws_lines state))
    as [previous |] eqn:found.
  - destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line)
      as [equal | unequal].
    + split; [discriminate |]. intros [other [same different]].
      inversion same; subst. contradiction.
    + split; [intros _; exists previous; auto | intros _; reflexivity].
  - split; [discriminate |]. intros [previous [impossible _]]. discriminate.
Qed.

Theorem collision_observation_step_is_zero_or_one :
  forall line_hash line state count,
  let total := snd
    (collision_observed_add_or_reuse_line line_hash line state count) in
  total = count \/ total = S count.
Proof.
  intros line_hash line state count.
  unfold collision_observed_add_or_reuse_line; cbn.
  destruct (line_collision_mismatch line_hash line (tws_state state));
    [right | left]; lia.
Qed.

Theorem collision_observed_sequence_erases_exactly :
  forall line_hash lines state count,
  fst (collision_observed_line_sequence line_hash lines state count) =
    traced_line_sequence line_hash lines state.
Proof.
  intros line_hash lines. induction lines as [|line rest IH];
    intros state count; [reflexivity |].
  cbn [collision_observed_line_sequence traced_line_sequence
    collision_observed_add_or_reuse_line].
  destruct (traced_add_or_reuse_line line_hash line state) as [id after].
  specialize (IH after
    (count + if line_collision_mismatch line_hash line (tws_state state)
             then 1 else 0)).
  destruct (collision_observed_line_sequence line_hash rest after
    (count + if line_collision_mismatch line_hash line (tws_state state)
             then 1 else 0)) as [[ids final] total].
  cbn in IH. rewrite <- IH. reflexivity.
Qed.

Definition forced_abaa_collision_observation :=
  collision_observed_line_sequence forced_collision_hash
    [[120]; [121]; [120]; [120]]
    {| tws_state := empty_writer_state; tws_objects := [] |} 0.

Theorem forced_abaa_counts_two_mismatches_and_reuses_last_id :
  snd forced_abaa_collision_observation = 2 /\
  fst (fst forced_abaa_collision_observation) =
    [ACT_HEADER_LEN; ACT_HEADER_LEN + 2;
     ACT_HEADER_LEN + 4; ACT_HEADER_LEN + 4] /\
  aws_arena (tws_state (snd (fst forced_abaa_collision_observation))) =
    [1; 120; 1; 121; 1; 120] /\
  length (tws_objects (snd (fst forced_abaa_collision_observation))) = 3.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem collision_misses_and_equal_reuse_do_not_increment :
  snd (collision_observed_line_sequence forced_collision_hash
    [[120]; [120]; [120]]
    {| tws_state := empty_writer_state; tws_objects := [] |} 7) = 7.
Proof. vm_compute. reflexivity. Qed.

Theorem counting_every_occupied_bucket_is_not_collision_counting :
  line_collision_mismatch forced_collision_hash [120]
    {| aws_arena := [];
       aws_lines := [{| lce_hash := 0; lce_bytes := [120]; lce_id := 19 |}] |}
    = false /\
  lookup_line_hash 0
    [{| lce_hash := 0; lce_bytes := [120]; lce_id := 19 |}] <> None.
Proof. vm_compute. split; [reflexivity | discriminate]. Qed.

Theorem concrete_writer_matches_pathmap_three_way_image :
  encode_writer_tree fixture_line_hash three_way_writer_tree =
    Some ACT_THREE_WAY_IMAGE.
Proof. vm_compute. reflexivity. Qed.

Theorem concrete_writer_matches_pathmap_line_image :
  encode_writer_tree fixture_line_hash line_writer_tree = Some ACT_LINE_IMAGE.
Proof. vm_compute. reflexivity. Qed.

Theorem concrete_writer_reuses_equal_lines_under_equal_hash :
  encode_writer_tree forced_collision_hash same_line_writer_tree =
    Some (ACT_MAGIC ++ le_fixed 8 24 ++
      [1;120;192;0;2;192;1;5;2;6;97;98] ++ ACT_TRAILER).
Proof. vm_compute. reflexivity. Qed.

Theorem concrete_writer_collision_mismatch_allocates_and_overwrites :
  encode_writer_tree forced_collision_hash colliding_line_writer_tree =
    Some (ACT_MAGIC ++ le_fixed 8 26 ++
      [1;120;1;121;192;0;4;192;1;5;2;6;97;98] ++ ACT_TRAILER).
Proof. vm_compute. reflexivity. Qed.

(** PathMap 0.3.4 retains one cache entry per hash.  Under an A-B-A collision,
    B overwrites A, so the second A is emitted again rather than reusing its
    first line object.  This closed witness freezes the current upstream byte
    contract; it is not an endorsement of collision-dependent consensus bytes. *)
Theorem concrete_writer_aba_collision_reemits_first_line :
  encode_writer_tree forced_collision_hash aba_colliding_line_writer_tree =
    Some (ACT_MAGIC ++ le_fixed 8 31 ++
      [1;120;1;121;1;120;
       192;0;6;192;1;7;192;2;8;3;9;97;98;99] ++ ACT_TRAILER).
Proof. vm_compute. reflexivity. Qed.

Theorem concrete_writer_images_parse_with_the_v3_physical_parser :
  parse_act_header
      (match encode_writer_tree fixture_line_hash three_way_writer_tree with
       | Some bytes => bytes | None => [] end) =
    Some (22, [64;0;64;0;64;0;3;6;10;20;30] ++ ACT_TRAILER) /\
  parse_act_header
      (match encode_writer_tree fixture_line_hash line_writer_tree with
       | Some bytes => bytes | None => [] end) =
    Some (19, [2;97;98;192;0;3] ++ ACT_TRAILER).
Proof.
  rewrite concrete_writer_matches_pathmap_three_way_image,
    concrete_writer_matches_pathmap_line_image.
  exact (conj (proj1 exact_three_way_actree03_producer_parser_roundtrip)
    (proj1 exact_line_actree03_producer_parser_roundtrip)).
Qed.

Lemma byte_ok_true : forall byte, byte < 256 -> byte_ok byte = true.
Proof.
  intros byte bounded. unfold byte_ok. now apply Nat.ltb_lt.
Qed.

Lemma sorted_adjacent_pairs : forall labels,
  StronglySorted Nat.lt labels ->
  Forall (fun pair => fst pair < snd pair) (combine labels (tl labels)).
Proof.
  intros labels sorted.
  induction sorted as [|head tail sortedTail adjacentTail headBeforeTail].
  - constructor.
  - destruct tail as [|next rest].
    + constructor.
    + simpl. constructor.
      * inversion headBeforeTail; subst. simpl in *. exact H1.
      * exact adjacentTail.
Qed.

Lemma sparse_labels_valid_of_canonical_order : forall labels,
  Forall (fun byte => byte < 256) labels ->
  StronglySorted Nat.lt labels ->
  sparse_labels_valid labels = true.
Proof.
  intros labels bytes sorted. unfold sparse_labels_valid.
  apply andb_true_iff. split.
  - apply forallb_forall. intros byte present.
    apply byte_ok_true.
    rewrite Forall_forall in bytes.
    exact (bytes byte present).
  - pose proof (sorted_adjacent_pairs labels sorted) as adjacent.
    apply forallb_forall. intros pair present.
    rewrite Forall_forall in adjacent.
    apply Nat.ltb_lt. exact (adjacent pair present).
Qed.

Lemma parse_branch_labels_sparse_writer_roundtrip : forall labels suffix,
  length labels < 32 ->
  Forall (fun byte => byte < 256) labels ->
  StronglySorted Nat.lt labels ->
  parse_branch_labels (length labels) (labels ++ suffix) =
    Some (labels, length labels, suffix).
Proof.
  intros labels suffix sparse bytes sorted.
  unfold parse_branch_labels.
  destruct (length labels <? 32) eqn:countSparse.
  - rewrite take_exact_app_exact.
    rewrite (sparse_labels_valid_of_canonical_order labels bytes sorted).
    reflexivity.
  - apply Nat.ltb_ge in countSparse. lia.
Qed.

Lemma parse_optional_value_writer_roundtrip : forall value suffix,
  (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
  parse_optional_value
    (match value with None => false | Some _ => true end)
    (optional_value_bytes value ++ suffix) =
  Some (value, length (optional_value_bytes value), suffix).
Proof.
  intros [scalar|] suffix bounded.
  - cbn [optional_value_bytes parse_optional_value].
    rewrite encode_act_varint_parse_roundtrip; [reflexivity|].
    apply bounded. reflexivity.
  - reflexivity.
Qed.

Lemma parse_optional_backward_writer_roundtrip : forall origin target suffix,
  target <= origin -> origin - target < Nat.pow 256 8 ->
  parse_optional_backward origin true
    (relative_bytes origin target ++ suffix) =
  Some (Some target, length (relative_bytes origin target), suffix).
Proof.
  intros origin target suffix backward bounded.
  unfold relative_bytes, parse_optional_backward.
  rewrite encode_act_varint_parse_roundtrip by exact bounded.
  cbn.
  unfold backward_target.
  destruct (origin - target <=? origin) eqn:relativeFits.
  - replace (origin - (origin - target)) with target by lia. reflexivity.
  - apply Nat.leb_gt in relativeFits. lia.
Qed.

Lemma testbit_six_false_below_32 : forall count,
  count < 32 -> Nat.testbit count 6 = false.
Proof.
  intros count bounded.
  apply Nat.testbit_unique with (l := count) (h := 0); cbn; lia.
Qed.

Lemma testbit_six_true_above_64_below_96 : forall count,
  count < 32 -> Nat.testbit (count + 64) 6 = true.
Proof.
  intros count bounded.
  apply Nat.testbit_unique with (l := count) (h := 0); cbn; lia.
Qed.

Lemma testbit_seven_false_below_128 : forall head,
  head < 128 -> Nat.testbit head 7 = false.
Proof.
  intros head bounded.
  apply Nat.testbit_unique with (l := head) (h := 0); cbn; lia.
Qed.

Lemma sparse_branch_head_facts : forall value labels,
  length labels < 32 ->
  byte_ok (branch_head value labels) = true /\
  Nat.testbit (branch_head value labels) 7 = false /\
  Nat.testbit (branch_head value labels) 6 =
    match value with None => false | Some _ => true end /\
  branch_head value labels mod 64 = length labels.
Proof.
  intros [scalar|] labels sparse; unfold branch_head, ACT_VALUE_FLAG.
  - rewrite Nat.min_l by lia. repeat split.
    + apply Nat.leb_le. lia.
    + apply testbit_seven_false_below_128. lia.
    + apply testbit_six_true_above_64_below_96. exact sparse.
    + symmetry. apply Nat.mod_unique with (q := 1); lia.
  - rewrite Nat.min_l by lia. repeat split.
    + apply Nat.leb_le. lia.
    + apply testbit_seven_false_below_128. lia.
    + apply testbit_six_false_below_32. lia.
    + replace (length labels + 0) with (length labels) by lia.
      apply Nat.mod_small. lia.
Qed.

Lemma first_child_id_some_labels_nonempty : forall children first,
  first_child_id children = Some first ->
  labels_of_children children <> [].
Proof.
  intros children first present.
  destruct children as [|[label id] rest]; cbn in present; [discriminate|].
  cbn. discriminate.
Qed.

Theorem sparse_branch_record_writer_roundtrip :
  forall origin value children first suffix,
  first_child_id children = Some first ->
  length (labels_of_children children) < 32 ->
  Forall (fun byte => byte < 256) (labels_of_children children) ->
  StronglySorted Nat.lt (labels_of_children children) ->
  (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
  first <= origin -> origin - first < Nat.pow 256 8 ->
  parse_node_record origin
    (encode_pending_node origin (PendingBranch value children) ++ suffix) =
  Some ({| pn_id := origin;
           pn_encoded_len :=
             1 + length (optional_value_bytes value) +
             length (relative_bytes origin first) +
             length (labels_of_children children);
           pn_value := value;
           pn_edge := ParsedBranch (Some first) (labels_of_children children) |},
        suffix).
Proof.
  intros origin value children first suffix firstPresent sparse bytes sorted
    valueBounded backward relativeBounded.
  pose proof (sparse_branch_head_facts value (labels_of_children children) sparse)
    as [headByte [headBranch [headValue headCount]]].
  pose proof (first_child_id_some_labels_nonempty children first firstPresent)
    as labelsNonempty.
  unfold encode_pending_node; fold (labels_of_children children).
  rewrite firstPresent.
  cbn [app].
  unfold parse_node_record.
  rewrite headByte, headValue, headBranch.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_value_writer_roundtrip by exact valueBounded.
  cbn.
  unfold parse_branch_record.
  rewrite headCount.
  assert (Nat.eqb (length (labels_of_children children)) 0 = false)
    as labelsPresent.
  { apply Nat.eqb_neq. intro empty.
    apply labelsNonempty. now apply length_zero_iff_nil. }
  rewrite labelsPresent.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_backward_writer_roundtrip by assumption.
  cbn.
  unfold encoded_branch_labels.
  destruct (length (labels_of_children children) <? 32) eqn:sparseDecision.
  - rewrite parse_branch_labels_sparse_writer_roundtrip by assumption.
    cbn. rewrite labelsPresent. reflexivity.
  - apply Nat.ltb_ge in sparseDecision. lia.
Qed.

Theorem empty_branch_record_writer_roundtrip : forall origin value suffix,
  (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
  parse_node_record origin
    (encode_pending_node origin (PendingBranch value []) ++ suffix) =
  Some ({| pn_id := origin;
           pn_encoded_len := 1 + length (optional_value_bytes value);
           pn_value := value;
           pn_edge := ParsedBranch None [] |},
        suffix).
Proof.
  intros origin value suffix valueBounded.
  pose proof (sparse_branch_head_facts value [] ltac:(cbn; lia))
    as [headByte [headBranch [headValue headCount]]].
  unfold encode_pending_node, labels_of_children, first_child_id,
    encoded_branch_labels. cbn [map app].
  unfold parse_node_record.
  rewrite headByte, headValue, headBranch.
  rewrite <- app_assoc.
  rewrite parse_optional_value_writer_roundtrip by exact valueBounded.
  unfold parse_branch_record.
  rewrite headCount. cbn.
  replace (S (length (optional_value_bytes value) + 0 + 0))
    with (S (length (optional_value_bytes value))) by lia.
  reflexivity.
Qed.

Definition optional_backward_bytes (origin : nat) (target : option nat)
    : list bbyte :=
  match target with
  | None => []
  | Some id => relative_bytes origin id
  end.

Lemma parse_optional_backward_bytes_roundtrip : forall origin target suffix,
  (forall id, target = Some id ->
    id <= origin /\ origin - id < Nat.pow 256 8) ->
  parse_optional_backward origin
    (match target with None => false | Some _ => true end)
    (optional_backward_bytes origin target ++ suffix) =
  Some (target, length (optional_backward_bytes origin target), suffix).
Proof.
  intros origin [target|] suffix bounded.
  - cbn [optional_backward_bytes].
    apply parse_optional_backward_writer_roundtrip.
    + exact (proj1 (bounded target eq_refl)).
    + exact (proj2 (bounded target eq_refl)).
  - reflexivity.
Qed.

Lemma line_head_facts : forall value child,
  byte_ok (line_head value child) = true /\
  Nat.testbit (line_head value child) 7 = true /\
  Nat.testbit (line_head value child) 6 =
    match value with None => false | Some _ => true end /\
  Nat.land (line_head value child) 62 = 0 /\
  Nat.testbit (line_head value child) 0 =
    match child with None => false | Some _ => true end.
Proof.
  intros [value|] [child|]; vm_compute; repeat split; reflexivity.
Qed.

Theorem line_record_writer_roundtrip : forall origin value child line_id suffix,
  (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
  (forall id, child = Some id ->
    id <= origin /\ origin - id < Nat.pow 256 8) ->
  line_id <= origin -> origin - line_id < Nat.pow 256 8 ->
  parse_node_record origin
    (encode_pending_node origin (PendingLine value child line_id) ++ suffix) =
  Some ({| pn_id := origin;
           pn_encoded_len :=
             1 + length (optional_value_bytes value) +
             length (optional_backward_bytes origin child) +
             length (relative_bytes origin line_id);
           pn_value := value;
           pn_edge := ParsedLine child (Some line_id) |},
        suffix).
Proof.
  intros origin value child line_id suffix valueBounded childBounded
    lineBackward lineRelativeBounded.
  pose proof (line_head_facts value child)
    as [headByte [headLine [headValue [headReserved headChild]]]].
  unfold encode_pending_node.
  change (match child with
          | None => []
          | Some target => relative_bytes origin target
          end) with (optional_backward_bytes origin child).
  cbn [app].
  unfold parse_node_record.
  rewrite headByte, headValue, headLine.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_value_writer_roundtrip by exact valueBounded.
  unfold parse_line_record.
  rewrite headReserved, headChild.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_backward_bytes_roundtrip by exact childBounded.
  repeat rewrite <- app_assoc.
  unfold relative_bytes.
  rewrite encode_act_varint_parse_roundtrip by exact lineRelativeBounded.
  cbn. unfold backward_target.
  destruct (origin - line_id <=? origin) eqn:relativeFits.
  - replace (origin - (origin - line_id)) with line_id by lia.
    reflexivity.
  - apply Nat.leb_gt in relativeFits. lia.
Qed.

(** The dense branch form is a 256-bit characteristic vector.  The proof is
    factored through the exact [setbit] loop used by the writer, so the parser
    theorem below does not assume that masks and labels are inverse. *)
Definition selected_label_bit (labels : list bbyte) (octet bit : nat) : bool :=
  existsb (Nat.eqb (octet * 8 + bit)) labels.

Lemma mask_fold_testbit : forall labels octet candidates accumulator query,
  Nat.testbit
    (fold_left (fun mask bit =>
       if selected_label_bit labels octet bit
       then Nat.setbit mask bit else mask)
      candidates accumulator) query =
  Nat.testbit accumulator query ||
  existsb (fun bit =>
    selected_label_bit labels octet bit && Nat.eqb bit query) candidates.
Proof.
  intros labels octet candidates.
  induction candidates as [|candidate rest IH]; intros accumulator query.
  - cbn. now rewrite orb_false_r.
  - cbn [fold_left]. rewrite IH.
    unfold selected_label_bit at 1.
    destruct (existsb (Nat.eqb (octet * 8 + candidate)) labels)
      eqn:selected.
    + rewrite Nat.setbit_eqb. cbn. unfold selected_label_bit. rewrite selected.
      repeat rewrite orb_assoc.
      rewrite (orb_comm (candidate =? query) (Nat.testbit accumulator query)).
      reflexivity.
    + cbn. unfold selected_label_bit. rewrite selected. cbn. reflexivity.
Qed.

Lemma existsb_selected_unique_bit : forall labels octet query,
  query < 8 ->
  existsb (fun bit =>
    selected_label_bit labels octet bit && Nat.eqb bit query) (seq 0 8) =
  selected_label_bit labels octet query.
Proof.
  intros labels octet query bounded.
  apply eq_true_iff_eq. split.
  - rewrite existsb_exists. intros [candidate [present selectedAndEqual]].
    apply andb_true_iff in selectedAndEqual as [selected equal].
    apply Nat.eqb_eq in equal. subst candidate. exact selected.
  - intro selected.
    rewrite existsb_exists. exists query. split.
    + apply in_seq. lia.
    + apply andb_true_iff. split; [exact selected|apply Nat.eqb_refl].
Qed.

Lemma mask_octet_testbit : forall labels octet bit,
  bit < 8 ->
  Nat.testbit (mask_octet labels octet) bit =
  selected_label_bit labels octet bit.
Proof.
  intros labels octet bit bounded.
  unfold mask_octet.
  change (fold_left
      (fun mask bit0 =>
         if existsb (Nat.eqb (octet * 8 + bit0)) labels
         then Nat.setbit mask bit0 else mask) (seq 0 8) 0) with
    (fold_left
      (fun mask bit0 =>
         if selected_label_bit labels octet bit0
         then Nat.setbit mask bit0 else mask) (seq 0 8) 0).
  rewrite mask_fold_testbit, Nat.bits_0. cbn.
  apply existsb_selected_unique_bit. exact bounded.
Qed.

Lemma seq_is_translated_zero_seq : forall base count,
  seq base count = map (fun offset => base + offset) (seq 0 count).
Proof.
  intros base count. revert base.
  induction count as [|count IH]; intro base; [reflexivity|].
  cbn. f_equal.
  - lia.
  - rewrite (IH (S base)).
    rewrite <- (seq_shift count 0), map_map.
    apply map_ext. intros offset. lia.
Qed.

Lemma mask_byte_labels_is_filtered_octet : forall base bits,
  mask_byte_labels base bits =
  map (fun bit => base + bit)
    (filter (fun bit => Nat.testbit bits bit) (seq 0 8)).
Proof.
  intros base bits. unfold mask_byte_labels.
  destruct (bits =? 0) eqn:zero.
  - apply Nat.eqb_eq in zero. subst bits. vm_compute. reflexivity.
  - destruct (bits =? 255) eqn:full.
    + apply Nat.eqb_eq in full. subst bits.
      change (seq base 8 =
        map (fun bit => base + bit) (seq 0 8)).
      apply seq_is_translated_zero_seq.
    + reflexivity.
Qed.

Definition label_present (labels : list bbyte) (candidate : nat) : bool :=
  existsb (Nat.eqb candidate) labels.

Lemma decoded_mask_octet_is_filtered_block : forall labels octet,
  mask_byte_labels (octet * 8) (mask_octet labels octet) =
  filter (label_present labels) (seq (octet * 8) 8).
Proof.
  intros labels octet.
  rewrite mask_byte_labels_is_filtered_octet.
  replace (seq (octet * 8) 8) with
    (map (fun bit => octet * 8 + bit) (seq 0 8))
    by (symmetry; apply seq_is_translated_zero_seq).
  rewrite filter_map_swap.
  f_equal. apply filter_ext_in. intros bit present.
  apply in_seq in present.
  rewrite mask_octet_testbit by lia.
  unfold selected_label_bit, label_present. reflexivity.
Qed.

Lemma mask_labels_from_dense_blocks : forall labels octet count base,
  base = octet * 8 ->
  mask_labels_from base
    (map (mask_octet labels) (seq octet count)) =
  filter (label_present labels) (seq base (count * 8)).
Proof.
  intros labels octet count. revert octet.
  induction count as [|count IH]; intros octet base baseAt; [cbn; reflexivity|].
  subst base.
  cbn [seq map mask_labels_from].
  rewrite decoded_mask_octet_is_filtered_block.
  rewrite (IH (S octet) (octet * 8 + 8)) by lia.
  replace (S count * 8) with (8 + count * 8) by lia.
  rewrite seq_app, filter_app.
  reflexivity.
Qed.

Lemma dense_mask_decodes_to_enumeration : forall labels,
  mask_labels (dense_label_mask labels) =
  filter (label_present labels) (seq 0 256).
Proof.
  intro labels. unfold mask_labels, dense_label_mask.
  change 256 with (32 * 8).
  apply mask_labels_from_dense_blocks with (octet := 0). reflexivity.
Qed.

Lemma forall_filter_preserved : forall (P : nat -> Prop) f values,
  Forall P values -> Forall P (filter f values).
Proof.
  intros P f values all.
  induction all as [|value rest valueP restP IH]; [constructor|].
  cbn. destruct (f value); [constructor|]; assumption.
Qed.

Lemma strongly_sorted_filter_preserved : forall f values,
  StronglySorted Nat.lt values ->
  StronglySorted Nat.lt (filter f values).
Proof.
  intros f values sorted.
  induction sorted as [|value rest restSorted restFiltered valueBefore];
    [constructor|].
  cbn. destruct (f value).
  - constructor; [exact restFiltered|].
    apply forall_filter_preserved. exact valueBefore.
  - exact restFiltered.
Qed.

Lemma seq_head_before_tail : forall start count,
  Forall (Nat.lt start) (seq (S start) count).
Proof.
  intros start count. apply Forall_forall.
  intros value present. apply in_seq in present. lia.
Qed.

Lemma seq_is_strongly_sorted : forall start count,
  StronglySorted Nat.lt (seq start count).
Proof.
  intros start count. revert start.
  induction count as [|count IH]; intro start; [constructor|].
  cbn. constructor.
  - apply IH.
  - apply seq_head_before_tail.
Qed.

Lemma strongly_sorted_nat_extensional : forall left right,
  StronglySorted Nat.lt left ->
  StronglySorted Nat.lt right ->
  (forall value, In value left <-> In value right) ->
  left = right.
Proof.
  intros left right leftSorted. revert right.
  induction leftSorted as [|leftHead leftTail leftTailSorted leftTailIH leftBefore];
    intros right rightSorted sameMembers.
  - destruct right as [|rightHead rightTail]; [reflexivity|].
    exfalso. specialize (sameMembers rightHead). cbn in sameMembers. tauto.
  - destruct right as [|rightHead rightTail].
    + exfalso. specialize (sameMembers leftHead). cbn in sameMembers. tauto.
    + inversion rightSorted as [|? ? rightTailSorted rightBefore]; subst.
      assert (headsEqual : leftHead = rightHead).
      { assert (leftInRight : In leftHead (rightHead :: rightTail)).
        { apply (proj1 (sameMembers leftHead)). cbn. auto. }
        assert (rightInLeft : In rightHead (leftHead :: leftTail)).
        { apply (proj2 (sameMembers rightHead)). cbn. auto. }
        cbn in leftInRight, rightInLeft.
        destruct leftInRight as [equal|leftInTail]; [symmetry; exact equal|].
        destruct rightInLeft as [equal|rightInTail]; [exact equal|].
        rewrite Forall_forall in leftBefore, rightBefore.
        specialize (leftBefore rightHead rightInTail).
        specialize (rightBefore leftHead leftInTail). lia. }
      subst rightHead. f_equal.
      apply leftTailIH; [exact rightTailSorted|].
      intro value. specialize (sameMembers value). cbn in sameMembers.
      split; intro present.
      * destruct sameMembers as [forward backward].
        specialize (forward (or_intror present)).
        destruct forward as [equal|tail]; [|exact tail].
        rewrite Forall_forall in leftBefore.
        specialize (leftBefore value present). lia.
      * destruct (sameMembers) as [forward backward].
        specialize (backward (or_intror present)).
        destruct backward as [equal|tail]; [subst|exact tail].
        rewrite Forall_forall in rightBefore.
        specialize (rightBefore value present). lia.
Qed.

Lemma dense_enumeration_membership : forall labels value,
  Forall (fun byte => byte < 256) labels ->
  In value (filter (label_present labels) (seq 0 256)) <->
  In value labels.
Proof.
  intros labels value bytes. rewrite filter_In. split.
  - intros [_ selected]. unfold label_present in selected.
    rewrite existsb_exists in selected.
    destruct selected as [witness [present equal]].
    apply Nat.eqb_eq in equal. subst witness. exact present.
  - intro present. split.
    + apply in_seq. rewrite Forall_forall in bytes.
      specialize (bytes value present). lia.
    + unfold label_present. rewrite existsb_exists.
      exists value. split; [exact present|apply Nat.eqb_refl].
Qed.

Theorem dense_label_mask_writer_roundtrip : forall labels,
  Forall (fun byte => byte < 256) labels ->
  StronglySorted Nat.lt labels ->
  mask_labels (dense_label_mask labels) = labels.
Proof.
  intros labels bytes sorted. rewrite dense_mask_decodes_to_enumeration.
  apply strongly_sorted_nat_extensional.
  - apply strongly_sorted_filter_preserved. apply seq_is_strongly_sorted.
  - exact sorted.
  - intro value. apply dense_enumeration_membership. exact bytes.
Qed.

Lemma dense_label_mask_length : forall labels,
  length (dense_label_mask labels) = 32.
Proof.
  intro labels. unfold dense_label_mask. rewrite length_map, length_seq.
  reflexivity.
Qed.

Lemma parse_branch_labels_dense_writer_roundtrip : forall labels suffix,
  32 <= length labels ->
  Forall (fun byte => byte < 256) labels ->
  StronglySorted Nat.lt labels ->
  parse_branch_labels 32 (dense_label_mask labels ++ suffix) =
    Some (labels, 32, suffix).
Proof.
  intros labels suffix dense bytes sorted.
  unfold parse_branch_labels.
  rewrite Nat.ltb_irrefl, Nat.eqb_refl.
  replace 32 with (length (dense_label_mask labels)) at 1
    by (apply dense_label_mask_length).
  rewrite take_exact_app_exact.
  rewrite dense_label_mask_writer_roundtrip by assumption.
  match goal with
  | |- (if ?condition then _ else _) = _ =>
      destruct condition eqn:conditionValue
  end.
  - reflexivity.
  - apply Nat.leb_gt in conditionValue. exfalso.
    apply (proj1 (Nat.lt_nge (length labels) 32) conditionValue).
    exact dense.
Qed.

Lemma dense_branch_head_facts : forall value labels,
  32 <= length labels ->
  byte_ok (branch_head value labels) = true /\
  Nat.testbit (branch_head value labels) 7 = false /\
  Nat.testbit (branch_head value labels) 6 =
    match value with None => false | Some _ => true end /\
  branch_head value labels mod 64 = 32.
Proof.
  intros [scalar|] labels dense; unfold branch_head, ACT_VALUE_FLAG.
  - rewrite Nat.min_r by lia. repeat split; vm_compute; reflexivity.
  - rewrite Nat.min_r by lia. repeat split; vm_compute; reflexivity.
Qed.

Theorem dense_branch_record_writer_roundtrip :
  forall origin value children first suffix,
  first_child_id children = Some first ->
  32 <= length (labels_of_children children) ->
  Forall (fun byte => byte < 256) (labels_of_children children) ->
  StronglySorted Nat.lt (labels_of_children children) ->
  (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
  first <= origin -> origin - first < Nat.pow 256 8 ->
  parse_node_record origin
    (encode_pending_node origin (PendingBranch value children) ++ suffix) =
  Some ({| pn_id := origin;
           pn_encoded_len :=
             1 + length (optional_value_bytes value) +
             length (relative_bytes origin first) + 32;
           pn_value := value;
           pn_edge := ParsedBranch (Some first) (labels_of_children children) |},
        suffix).
Proof.
  intros origin value children first suffix firstPresent dense bytes sorted
    valueBounded backward relativeBounded.
  pose proof (dense_branch_head_facts value (labels_of_children children) dense)
    as [headByte [headBranch [headValue headCount]]].
  pose proof (first_child_id_some_labels_nonempty children first firstPresent)
    as labelsNonempty.
  unfold encode_pending_node; fold (labels_of_children children).
  rewrite firstPresent. cbn [app].
  unfold parse_node_record.
  rewrite headByte, headValue, headBranch.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_value_writer_roundtrip by exact valueBounded.
  unfold parse_branch_record. rewrite headCount.
  repeat rewrite <- app_assoc.
  rewrite parse_optional_backward_writer_roundtrip by assumption.
  unfold encoded_branch_labels.
  destruct (length (labels_of_children children) <? 32) eqn:denseDecision.
  - apply Nat.ltb_lt in denseDecision. lia.
  - rewrite parse_branch_labels_dense_writer_roundtrip by assumption.
    cbn.
    assert (Nat.eqb (length (labels_of_children children)) 0 = false)
      as labelsPresent by (apply Nat.eqb_neq; lia).
    rewrite labelsPresent. reflexivity.
Qed.

(** A complete image parser must derive its reachability index from the bytes;
    a caller-supplied plan is not evidence.  The work list below is a PDA.  Its
    token list is created internally from the image length and is therefore a
    structural termination witness, not a public traversal-depth limit.  Every
    successful transition consumes a previously unseen, strictly backward
    node identifier, so a canonical image cannot exhaust the tokens. *)

Record parsed_line_object : Type := {
  plo_id : nat;
  plo_encoded_len : nat;
  plo_bytes : list bbyte
}.

Definition parse_line_object (line_id arena_end : nat) (image : list bbyte)
    : option parsed_line_object :=
  if (ACT_HEADER_LEN <=? line_id) && (line_id <? arena_end) then
    match parse_act_varint (skipn line_id image) with
    | None => None
    | Some parsed =>
        match take_exact (vr_value parsed) (vr_rest parsed) with
        | None => None
        | Some (line, _) =>
            let encoded_len := vr_consumed parsed + length line in
            if (0 <? encoded_len) && (line_id + encoded_len <=? arena_end)
            then Some {| plo_id := line_id;
                         plo_encoded_len := encoded_len;
                         plo_bytes := line |}
            else None
        end
    end
  else None.

Fixpoint derive_direct_child_ids (labels : list bbyte) (cursor arena_end : nat)
    (image : list bbyte) : option (list nat) :=
  match labels with
  | [] => Some []
  | _ :: rest =>
      match parse_node_record cursor (skipn cursor image) with
      | None => None
      | Some (node, _) =>
          let next := cursor + pn_encoded_len node in
          if (0 <? pn_encoded_len node) && (next <=? arena_end) then
            match derive_direct_child_ids rest next arena_end image with
            | None => None
            | Some ids => Some (cursor :: ids)
            end
          else None
      end
  end.

Definition parsed_node_id_eq (id : nat) (node : parsed_node) : bool :=
  Nat.eqb id (pn_id node).

Definition parsed_line_id_eq (id : nat) (line : parsed_line_object) : bool :=
  Nat.eqb id (plo_id line).

Record physical_discovery : Type := {
  pd_pending : list nat;
  pd_nodes : list parsed_node;
  pd_lines : list parsed_line_object
}.

Definition branch_references (node : parsed_node) (arena_end : nat)
    (image : list bbyte) : option (list nat) :=
  match pn_edge node with
  | ParsedBranch None labels =>
      match labels with [] => Some [] | _ :: _ => None end
  | ParsedBranch (Some first) labels =>
      derive_direct_child_ids labels first arena_end image
  | ParsedLine child _ =>
      Some (match child with None => [] | Some id => [id] end)
  end.

(** Byte-derived counterpart of production's [DirectSemanticNode].  A line
    node and its optional physical branch wrapper are admitted as one semantic
    trie node; the wrapper remains recorded so producer-order replay can charge
    its physical bytes at the exact callback position. *)
Record direct_semantic_node_model : Type := {
  dsm_id : nat;
  dsm_encoded_len : nat;
  dsm_value : option nat;
  dsm_child_parent : nat;
  dsm_child_labels : list bbyte;
  dsm_child_ids : list nat;
  dsm_line : option parsed_line_object;
  dsm_wrapper : option (nat * nat)
}.

(** A proof-only recursive certificate retains the semantic ownership that a
    flat physical-object trace necessarily forgets.  It mirrors PathMap's
    callback discipline: descendants are completed first, direct-child roots
    are then emitted contiguously, an optional branch wrapper is emitted for a
    compressed line with children, and the current root remains pending until
    its parent (or the top-level encoder) publishes it. *)

Inductive source_semantic_tree : Type :=
| SourceSemanticNode
    (root_id root_encoded_len : nat)
    (root_node : pending_node)
    (line : list bbyte)
    (value : option nat)
    (children : source_semantic_forest)
    (line_id : option nat)
    (line_hash : option nat)
    (wrapper : option (nat * nat))
with source_semantic_forest : Type :=
| SourceSemanticNil
| SourceSemanticCons
    (label : bbyte)
    (child : source_semantic_tree)
    (rest : source_semantic_forest).

Scheme source_semantic_tree_ind' := Induction for source_semantic_tree Sort Prop
with source_semantic_forest_ind' :=
  Induction for source_semantic_forest Sort Prop.
Combined Scheme source_semantic_tree_forest_ind
  from source_semantic_tree_ind', source_semantic_forest_ind'.

Definition source_semantic_root_id (tree : source_semantic_tree) : nat :=
  match tree with SourceSemanticNode id _ _ _ _ _ _ _ _ => id end.

Definition source_semantic_root_encoded_len
    (tree : source_semantic_tree) : nat :=
  match tree with SourceSemanticNode _ encoded_len _ _ _ _ _ _ _ => encoded_len end.

Definition source_semantic_root_node (tree : source_semantic_tree)
    : pending_node :=
  match tree with SourceSemanticNode _ _ node _ _ _ _ _ _ => node end.

Fixpoint source_semantic_forest_labels (forest : source_semantic_forest)
    : list bbyte :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons label _ rest =>
      label :: source_semantic_forest_labels rest
  end.

Fixpoint source_semantic_forest_ids (forest : source_semantic_forest)
    : list nat :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_root_id child :: source_semantic_forest_ids rest
  end.

Fixpoint source_semantic_reify (tree : source_semantic_tree) : writer_tree :=
  match tree with
  | SourceSemanticNode _ _ _ line value children _ _ _ =>
      WriterNode line value (source_semantic_forest_reify children)
  end
with source_semantic_forest_reify (forest : source_semantic_forest)
    : writer_forest :=
  match forest with
  | SourceSemanticNil => WriterNil
  | SourceSemanticCons label child rest =>
      WriterCons label (source_semantic_reify child)
        (source_semantic_forest_reify rest)
  end.

Fixpoint source_semantic_node_count (tree : source_semantic_tree) : nat :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      S (source_semantic_forest_node_count children)
  end
with source_semantic_forest_node_count (forest : source_semantic_forest) : nat :=
  match forest with
  | SourceSemanticNil => 0
  | SourceSemanticCons _ child rest =>
      source_semantic_node_count child +
      source_semantic_forest_node_count rest
  end.

Theorem source_semantic_count_matches_reification :
  (forall tree,
      source_semantic_node_count tree =
        writer_tree_nodes (source_semantic_reify tree)) /\
  (forall forest,
      source_semantic_forest_node_count forest =
        writer_forest_nodes (source_semantic_forest_reify forest)).
Proof.
  apply source_semantic_tree_forest_ind; intros; cbn; lia.
Qed.

Record pending_source_semantic_node : Type := {
  pss_line : list bbyte;
  pss_value : option nat;
  pss_children : source_semantic_forest;
  pss_line_id : option nat;
  pss_line_hash : option nat;
  pss_wrapper : option (nat * nat);
  pss_root : pending_node
}.

Definition finalize_pending_source_semantic_node
    (root_id : nat) (pending : pending_source_semantic_node)
    : source_semantic_tree :=
  SourceSemanticNode root_id
    (length (encode_pending_node root_id (pss_root pending)))
    (pss_root pending)
    (pss_line pending) (pss_value pending) (pss_children pending)
    (pss_line_id pending) (pss_line_hash pending) (pss_wrapper pending).

Fixpoint certified_push_pending_children
    (labels : list bbyte) (pending : list pending_source_semantic_node)
    (state : traced_writer_state)
    : option (source_semantic_forest * traced_writer_state) :=
  match labels, pending with
  | [], [] => Some (SourceSemanticNil, state)
  | label :: label_rest, child :: child_rest =>
      let '(root_id, after_child) :=
        traced_push_pending_node (pss_root child) state in
      let child_certificate :=
        finalize_pending_source_semantic_node root_id child in
      match certified_push_pending_children
              label_rest child_rest after_child with
      | None => None
      | Some (rest, final) =>
          Some (SourceSemanticCons label child_certificate rest, final)
      end
  | _, _ => None
  end.

Section CertifiedSemanticSourceWriter.

Variable certified_line_hash : list bbyte -> nat.

Fixpoint certified_build_pending_tree (tree : writer_tree)
    (state : traced_writer_state)
    : option (pending_source_semantic_node * traced_writer_state) :=
  match tree with
  | WriterNode line value children =>
      match certified_build_pending_forest children state with
      | None => None
      | Some (pending_children, after_descendants) =>
          match certified_push_pending_children
                  (forest_labels children) pending_children after_descendants with
          | None => None
          | Some (certified_children, after_children) =>
              let child_ids := source_semantic_forest_ids certified_children in
              match line with
              | [] =>
                  Some
                    ({| pss_line := [];
                        pss_value := value;
                        pss_children := certified_children;
                        pss_line_id := None;
                        pss_line_hash := None;
                        pss_wrapper := None;
                        pss_root := PendingBranch value
                          (combine (source_semantic_forest_labels certified_children)
                            child_ids) |},
                     after_children)
              | first :: rest =>
                  let complete_line := first :: rest in
                  let '(line_id, after_line) :=
                    traced_add_or_reuse_line
                      certified_line_hash complete_line after_children in
                  match child_ids with
                  | [] =>
                      Some
                        ({| pss_line := complete_line;
                            pss_value := value;
                            pss_children := certified_children;
                            pss_line_id := Some line_id;
                            pss_line_hash := Some (certified_line_hash complete_line);
                            pss_wrapper := None;
                            pss_root := PendingLine value None line_id |},
                         after_line)
                  | first_child :: child_rest =>
                      let wrapper_node :=
                        PendingBranch value
                          (combine
                            (source_semantic_forest_labels certified_children)
                            (first_child :: child_rest)) in
                      let '(wrapper_id, after_wrapper) :=
                        traced_push_pending_node wrapper_node after_line in
                      Some
                        ({| pss_line := complete_line;
                            pss_value := value;
                            pss_children := certified_children;
                            pss_line_id := Some line_id;
                            pss_line_hash := Some (certified_line_hash complete_line);
                            pss_wrapper := Some
                              (wrapper_id,
                               length (encode_pending_node wrapper_id wrapper_node));
                            pss_root :=
                              PendingLine None (Some wrapper_id) line_id |},
                         after_wrapper)
                  end
              end
          end
      end
  end
with certified_build_pending_forest (forest : writer_forest)
    (state : traced_writer_state)
    : option (list pending_source_semantic_node * traced_writer_state) :=
  match forest with
  | WriterNil => Some ([], state)
  | WriterCons _ child rest =>
      match certified_build_pending_tree child state with
      | None => None
      | Some (pending_child, after_child) =>
          match certified_build_pending_forest rest after_child with
          | None => None
          | Some (pending_rest, final) =>
              Some (pending_child :: pending_rest, final)
          end
      end
  end.

Definition encode_writer_tree_with_semantic_certificate (tree : writer_tree)
    : option (list bbyte * source_semantic_tree *
              list physical_writer_object) :=
  let initial := {| tws_state := empty_writer_state; tws_objects := [] |} in
  match certified_build_pending_tree tree initial with
  | None => None
  | Some (pending_root, before_root) =>
      let '(root_id, final) :=
        traced_push_pending_node (pss_root pending_root) before_root in
      let certificate :=
        finalize_pending_source_semantic_node root_id pending_root in
      Some (ACT_MAGIC ++ le_fixed 8 root_id ++
            aws_arena (tws_state final) ++ ACT_TRAILER,
            certificate,
            tws_objects final)
  end.

End CertifiedSemanticSourceWriter.

Definition source_semantic_erase (tree : source_semantic_tree)
    : direct_semantic_node_model :=
  match tree with
  | SourceSemanticNode root_id root_len _ line value children line_id _ wrapper =>
      {| dsm_id := root_id;
         dsm_encoded_len := root_len;
         dsm_value := value;
         dsm_child_parent :=
           match wrapper with Some (wrapper_id, _) => wrapper_id
           | None => root_id end;
         dsm_child_labels := source_semantic_forest_labels children;
         dsm_child_ids := source_semantic_forest_ids children;
         dsm_line :=
           match line_id with
           | None => None
           | Some id =>
               Some {| plo_id := id;
                       plo_encoded_len :=
                         length (encode_act_varint (length line)) + length line;
                       plo_bytes := line |}
           end;
         dsm_wrapper := wrapper |}
  end.

Definition pending_source_semantic_reify
    (pending : pending_source_semantic_node) : writer_tree :=
  WriterNode (pss_line pending) (pss_value pending)
    (source_semantic_forest_reify (pss_children pending)).

Fixpoint pending_source_semantic_forest_reify
    (labels : list bbyte) (pending : list pending_source_semantic_node)
    : option writer_forest :=
  match labels, pending with
  | [], [] => Some WriterNil
  | label :: label_rest, child :: child_rest =>
      match pending_source_semantic_forest_reify label_rest child_rest with
      | None => None
      | Some rest =>
          Some (WriterCons label (pending_source_semantic_reify child) rest)
      end
  | _, _ => None
  end.

Lemma finalized_pending_source_semantic_reifies_exactly :
  forall root_id pending,
    source_semantic_reify
      (finalize_pending_source_semantic_node root_id pending) =
    pending_source_semantic_reify pending.
Proof. reflexivity. Qed.

Theorem certified_push_pending_children_erases :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    traced_push_pending_children labels (map pss_root pending) state =
      Some
        (combine (source_semantic_forest_labels certified)
          (source_semantic_forest_ids certified), final) /\
    pending_source_semantic_forest_reify labels pending =
      Some (source_semantic_forest_reify certified) /\
    source_semantic_forest_labels certified = labels.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - cbn [certified_push_pending_children] in certifiedRun.
    inversion certifiedRun; subst. repeat split; reflexivity.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    destruct (IH pending afterChild certifiedRest final restRun) as
      [tracedRest [reifiedRest labelsExact]].
    split.
    + change
        ((let '(id, afterNode) :=
            traced_push_pending_node (pss_root child) state in
          match traced_push_pending_children labels
                  (map pss_root pending) afterNode with
          | None => None
          | Some (children, actualFinal) =>
              Some ((label, id) :: children, actualFinal)
          end) =
         Some
           ((label, rootId) ::
              combine (source_semantic_forest_labels certifiedRest)
                (source_semantic_forest_ids certifiedRest), final)).
      rewrite childRun, tracedRest. reflexivity.
    + split.
      * cbn [pending_source_semantic_forest_reify].
        rewrite reifiedRest. reflexivity.
      * cbn [source_semantic_forest_labels]. now rewrite labelsExact.
Qed.

Theorem certified_source_writer_erases_and_reifies : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      traced_build_pending_tree line_hash tree state =
        Some (pss_root pending, final) /\
      pending_source_semantic_reify pending = tree) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      traced_build_pending_forest line_hash forest state =
        Some (map pss_root pending, final) /\
      pending_source_semantic_forest_reify
        (forest_labels forest) pending = Some forest).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    destruct (childrenIH state pendingChildren afterDescendants childrenRun) as
      [tracedChildren pendingChildrenReify].
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    destruct (certified_push_pending_children_erases
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [tracedPush [certifiedChildrenReify certifiedLabels]].
    assert (source_semantic_forest_reify certifiedChildren = children)
      as childrenExact.
    { rewrite pendingChildrenReify in certifiedChildrenReify.
      inversion certifiedChildrenReify. reflexivity. }
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      split.
      * cbn [traced_build_pending_tree].
        rewrite tracedChildren, tracedPush. reflexivity.
      * unfold pending_source_semantic_reify.
        cbn in childrenExact |- *. congruence.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      destruct certifiedChildren as [|certifiedLabel certifiedChild certifiedRest].
      * inversion certifiedRun; subst pending final.
        split.
        -- cbn [traced_build_pending_tree].
           rewrite tracedChildren, tracedPush, lineRun. reflexivity.
        -- unfold pending_source_semantic_reify.
           cbn in childrenExact |- *. congruence.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons certifiedLabel certifiedChild certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons certifiedLabel certifiedChild certifiedRest)))).
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine]
          in certifiedRun, tracedPush.
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        injection certifiedRun as pendingExact finalExact.
        subst pending final.
        split.
        -- cbn [traced_build_pending_tree].
           rewrite tracedChildren, tracedPush, lineRun.
           reflexivity.
        -- unfold pending_source_semantic_reify.
           cbn in childrenExact |- *. congruence.
  - intros state pending final certifiedRun.
    inversion certifiedRun; subst pending final. split; reflexivity.
  - intros label child childIH rest restIH state pending final certifiedRun.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    destruct (childIH state pendingChild afterChild childRun) as
      [tracedChild childReify].
    destruct (restIH afterChild pendingRest final restRun) as
      [tracedRest restReify].
    split.
    + cbn [traced_build_pending_forest].
      rewrite tracedChild, tracedRest. reflexivity.
    + cbn [forest_labels pending_source_semantic_forest_reify].
      rewrite childReify, restReify. reflexivity.
Qed.

Theorem semantic_certificate_erases_and_reifies_exactly :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    encode_writer_tree_with_certificate line_hash tree =
      Some (bytes, objects) /\
    source_semantic_reify certificate = tree.
Proof.
  intros line_hash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree line_hash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  destruct (proj1 (certified_source_writer_erases_and_reifies line_hash)
    tree initial pending beforeRoot built) as [tracedBuilt reified].
  split.
  - unfold encode_writer_tree_with_certificate. fold initial.
    rewrite tracedBuilt, rootRun. reflexivity.
  - rewrite finalized_pending_source_semantic_reifies_exactly.
    exact reified.
Qed.

Theorem certified_push_pending_children_total :
  forall labels pending state,
    length labels = length pending ->
    exists certified final,
      certified_push_pending_children labels pending state =
        Some (certified, final).
Proof.
  induction labels as [|label labels IH]; intros pending state lengths;
    destruct pending as [|child pending]; cbn in lengths.
  - exists SourceSemanticNil, state. reflexivity.
  - discriminate lengths.
  - discriminate lengths.
  - inversion lengths as [tailLengths].
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (IH pending afterChild tailLengths) as
      [certifiedRest [final restRun]].
    exists
      (SourceSemanticCons label
        (finalize_pending_source_semantic_node rootId child) certifiedRest),
      final.
    cbn [certified_push_pending_children].
    rewrite childRun, restRun. reflexivity.
Qed.

Theorem certified_source_writer_is_total : forall line_hash,
  (forall tree state,
      exists pending final,
        certified_build_pending_tree line_hash tree state =
          Some (pending, final)) /\
  (forall forest state,
      exists pending final,
        certified_build_pending_forest line_hash forest state =
          Some (pending, final) /\
        length pending = length (forest_labels forest)).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state.
    destruct (childrenIH state) as
      [pendingChildren [afterDescendants [childrenRun childrenLength]]].
    destruct (certified_push_pending_children_total
      (forest_labels children) pendingChildren afterDescendants)
      as [certifiedChildren [afterChildren pushRun]];
      [symmetry; exact childrenLength|].
    destruct line as [|first rest].
    + exists
        {| pss_line := [];
           pss_value := value;
           pss_children := certifiedChildren;
           pss_line_id := None;
           pss_line_hash := None;
           pss_wrapper := None;
           pss_root := PendingBranch value
             (combine (source_semantic_forest_labels certifiedChildren)
               (source_semantic_forest_ids certifiedChildren)) |},
        afterChildren.
      cbn [certified_build_pending_tree]. rewrite childrenRun, pushRun.
      reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      destruct certifiedChildren as [|label child certifiedRest].
      * exists
          {| pss_line := first :: rest;
             pss_value := value;
             pss_children := SourceSemanticNil;
             pss_line_id := Some lineId;
             pss_line_hash := Some (line_hash (first :: rest));
             pss_wrapper := None;
             pss_root := PendingLine value None lineId |},
          afterLine.
        cbn [certified_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun. reflexivity.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        exists
          {| pss_line := first :: rest;
             pss_value := value;
             pss_children := SourceSemanticCons label child certifiedRest;
             pss_line_id := Some lineId;
             pss_line_hash := Some (line_hash (first :: rest));
             pss_wrapper := Some
               (wrapperId, length (encode_pending_node wrapperId wrapperNode));
             pss_root := PendingLine None (Some wrapperId) lineId |},
          afterWrapper.
        cbn [certified_build_pending_tree].
        rewrite childrenRun, pushRun, lineRun.
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine].
        unfold traced_push_pending_node, push_pending_node in wrapperRun.
        inversion wrapperRun; subst wrapperId afterWrapper. reflexivity.
  - intro state. exists [], state. split; reflexivity.
  - intros label child childIH rest restIH state.
    destruct (childIH state) as [pendingChild [afterChild childRun]].
    destruct (restIH afterChild) as
      [pendingRest [final [restRun restLength]]].
    exists (pendingChild :: pendingRest), final.
    split.
    + cbn [certified_build_pending_forest].
      rewrite childRun, restRun. reflexivity.
    + change (S (length pendingRest) = S (length (forest_labels rest))).
      now rewrite restLength.
Qed.

Corollary encode_writer_tree_with_semantic_certificate_is_total :
  forall line_hash tree,
    exists bytes certificate objects,
      encode_writer_tree_with_semantic_certificate line_hash tree =
        Some (bytes, certificate, objects).
Proof.
  intros line_hash tree.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  destruct (proj1 (certified_source_writer_is_total line_hash) tree initial)
    as [pending [beforeRoot built]].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  exists
    (ACT_MAGIC ++ le_fixed 8 rootId ++
      aws_arena (tws_state final) ++ ACT_TRAILER),
    (finalize_pending_source_semantic_node rootId pending),
    (tws_objects final).
  unfold encode_writer_tree_with_semantic_certificate. fold initial.
  rewrite built, rootRun. reflexivity.
Qed.

Definition option_wrapper_count (wrapper : option (nat * nat)) : nat :=
  match wrapper with None => 0 | Some _ => 1 end.

Fixpoint source_semantic_wrapper_count (tree : source_semantic_tree) : nat :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ wrapper =>
      option_wrapper_count wrapper +
      source_semantic_forest_wrapper_count children
  end
with source_semantic_forest_wrapper_count
    (forest : source_semantic_forest) : nat :=
  match forest with
  | SourceSemanticNil => 0
  | SourceSemanticCons _ child rest =>
      source_semantic_wrapper_count child +
      source_semantic_forest_wrapper_count rest
  end.

Definition source_semantic_physical_node_count
    (tree : source_semantic_tree) : nat :=
  source_semantic_node_count tree + source_semantic_wrapper_count tree.

Definition source_semantic_forest_physical_node_count
    (forest : source_semantic_forest) : nat :=
  source_semantic_forest_node_count forest +
  source_semantic_forest_wrapper_count forest.

Definition pending_source_emitted_node_count
    (pending : pending_source_semantic_node) : nat :=
  source_semantic_forest_physical_node_count (pss_children pending) +
  option_wrapper_count (pss_wrapper pending).

Fixpoint pending_source_list_emitted_node_count
    (pending : list pending_source_semantic_node) : nat :=
  match pending with
  | [] => 0
  | child :: rest =>
      pending_source_emitted_node_count child +
      pending_source_list_emitted_node_count rest
  end.

Lemma finalized_pending_physical_node_count : forall root_id pending,
  source_semantic_physical_node_count
    (finalize_pending_source_semantic_node root_id pending) =
  S (pending_source_emitted_node_count pending).
Proof.
  intros root_id pending.
  unfold source_semantic_physical_node_count,
    pending_source_emitted_node_count,
    source_semantic_forest_physical_node_count.
  unfold finalize_pending_source_semantic_node.
  cbn [source_semantic_node_count source_semantic_wrapper_count]. lia.
Qed.

Lemma physical_node_object_count_app : forall left right,
  physical_node_object_count (left ++ right) =
    physical_node_object_count left + physical_node_object_count right.
Proof.
  induction left as [|object rest IH]; intro right; [reflexivity|].
  destruct object; cbn; rewrite IH; lia.
Qed.

Lemma traced_push_pending_node_increments_physical_node_count :
  forall node state,
    physical_node_object_count
      (tws_objects (snd (traced_push_pending_node node state))) =
    S (physical_node_object_count (tws_objects state)).
Proof.
  intros node state. unfold traced_push_pending_node.
  destruct (push_pending_node node (tws_state state)); cbn.
  rewrite physical_node_object_count_app. cbn. lia.
Qed.

Lemma traced_add_or_reuse_line_preserves_physical_node_count :
  forall line_hash line state,
    physical_node_object_count
      (tws_objects (snd (traced_add_or_reuse_line line_hash line state))) =
    physical_node_object_count (tws_objects state).
Proof.
  intros line_hash line state. unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [id after]; cbn.
  destruct (Nat.eqb
    (length (aws_arena after))
    (length (aws_arena (tws_state state)))).
  - reflexivity.
  - rewrite physical_node_object_count_app. cbn. lia.
Qed.

Theorem certified_push_pending_children_count_exact :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    physical_node_object_count (tws_objects final) =
      physical_node_object_count (tws_objects state) + length pending /\
    source_semantic_forest_physical_node_count certified =
      pending_source_list_emitted_node_count pending + length pending.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final. split.
    + cbn. lia.
    + unfold source_semantic_forest_physical_node_count. reflexivity.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    destruct (IH pending afterChild certifiedRest final restRun) as
      [restPhysical restSemantic].
    pose proof (traced_push_pending_node_increments_physical_node_count
      (pss_root child) state) as childPhysical.
    rewrite childRun in childPhysical.
    split.
    + cbn in childPhysical |- *. lia.
    + unfold source_semantic_forest_physical_node_count in *.
      cbn [source_semantic_forest_node_count
        source_semantic_forest_wrapper_count
        pending_source_list_emitted_node_count].
      pose proof (finalized_pending_physical_node_count rootId child).
      unfold source_semantic_physical_node_count in H.
      cbn [length] in H, restSemantic |- *. lia.
Qed.

Theorem certified_source_writer_physical_count_exact : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      physical_node_object_count (tws_objects final) =
        physical_node_object_count (tws_objects state) +
        pending_source_emitted_node_count pending) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      physical_node_object_count (tws_objects final) =
        physical_node_object_count (tws_objects state) +
        pending_source_list_emitted_node_count pending).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    pose proof (childrenIH state pendingChildren afterDescendants childrenRun)
      as descendantsCount.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    destruct (certified_push_pending_children_count_exact
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [childrenRootsCount certifiedChildrenCount].
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      unfold pending_source_emitted_node_count.
      cbn [pss_children pss_wrapper option_wrapper_count].
      unfold source_semantic_forest_physical_node_count in
        certifiedChildrenCount |- *.
      lia.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_preserves_physical_node_count
        line_hash (first :: rest) afterChildren) as lineCount.
      rewrite lineRun in lineCount.
      cbn in lineCount.
      assert (physical_node_object_count (tws_objects afterChildren) =
        physical_node_object_count (tws_objects state) +
        source_semantic_forest_physical_node_count certifiedChildren)
        as childrenTotalCount.
      { unfold source_semantic_forest_physical_node_count in
          certifiedChildrenCount |- *. lia. }
      assert (physical_node_object_count (tws_objects afterLine) =
        physical_node_object_count (tws_objects state) +
        source_semantic_forest_physical_node_count certifiedChildren)
        as afterLineCount by lia.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        unfold pending_source_emitted_node_count.
        unfold source_semantic_forest_physical_node_count in
          afterLineCount |- *.
        cbn [pss_children pss_wrapper option_wrapper_count
          source_semantic_forest_node_count
          source_semantic_forest_wrapper_count] in afterLineCount |- *.
        lia.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        pose proof (traced_push_pending_node_increments_physical_node_count
          wrapperNode afterLine) as wrapperCount.
        rewrite wrapperRun in wrapperCount.
        assert (physical_node_object_count (tws_objects afterWrapper) =
          physical_node_object_count (tws_objects state) +
          source_semantic_forest_physical_node_count
            (SourceSemanticCons label child certifiedRest) + 1)
          as countExact.
        { cbn in wrapperCount. lia. }
        unfold traced_push_pending_node, push_pending_node in wrapperRun.
        inversion wrapperRun; subst wrapperId afterWrapper.
        injection certifiedRun as pendingExact finalExact.
        subst pending final.
        unfold pending_source_emitted_node_count.
        cbn [pss_children pss_wrapper option_wrapper_count].
        unfold wrapperNode in countExact.
        cbn [source_semantic_forest_labels source_semantic_forest_ids combine]
          in countExact.
        rewrite countExact. lia.
  - intros state pending final certifiedRun.
    inversion certifiedRun; subst pending final.
    cbn [pending_source_list_emitted_node_count]. lia.
  - intros label child childIH rest restIH state pending final certifiedRun.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    pose proof (childIH state pendingChild afterChild childRun) as childCount.
    pose proof (restIH afterChild pendingRest final restRun) as restCount.
    cbn [pending_source_list_emitted_node_count]. lia.
Qed.

Theorem semantic_certificate_physical_node_count_exact :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    physical_node_object_count objects =
      source_semantic_physical_node_count certificate.
Proof.
  intros line_hash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree line_hash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  pose proof (proj1 (certified_source_writer_physical_count_exact line_hash)
    tree initial pending beforeRoot built) as beforeRootCount.
  pose proof (traced_push_pending_node_increments_physical_node_count
    (pss_root pending) beforeRoot) as rootCount.
  rewrite rootRun in rootCount.
  pose proof (finalized_pending_physical_node_count rootId pending)
    as semanticCount.
  unfold initial in beforeRootCount. cbn in beforeRootCount.
  cbn in rootCount.
  lia.
Qed.

Theorem semantic_certificate_node_count_matches_source :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_node_count certificate = writer_tree_nodes tree.
Proof.
  intros line_hash tree bytes certificate objects encoded.
  pose proof (semantic_certificate_erases_and_reifies_exactly
    line_hash tree bytes certificate objects encoded) as [_ reified].
  rewrite <- reified.
  exact (proj1 source_semantic_count_matches_reification certificate).
Qed.

Theorem semantic_certificate_node_count_is_byte_bounded :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_node_count certificate <= length bytes /\
    writer_tree_nodes tree <= length bytes.
Proof.
  intros line_hash tree bytes certificate objects encoded.
  pose proof (semantic_certificate_erases_and_reifies_exactly
    line_hash tree bytes certificate objects encoded) as [erased reified].
  pose proof (canonical_writer_physical_node_count_le_image_bytes
    line_hash tree bytes objects erased) as physicalBound.
  pose proof (semantic_certificate_physical_node_count_exact
    line_hash tree bytes certificate objects encoded) as physicalExact.
  pose proof (semantic_certificate_node_count_matches_source
    line_hash tree bytes certificate objects encoded) as semanticExact.
  unfold source_semantic_physical_node_count in physicalExact.
  lia.
Qed.

Lemma act_header_prefix_length : forall root_id,
  length (ACT_MAGIC ++ le_fixed 8 root_id) = ACT_HEADER_LEN.
Proof. intro root_id. vm_compute. reflexivity. Qed.

Lemma skipn_length_app_exact : forall (A : Type) (prefix suffix : list A),
  skipn (length prefix) (prefix ++ suffix) = suffix.
Proof.
  intros A prefix suffix.
  rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity.
Qed.

Lemma physical_writer_layout_focus_start : forall position before object after,
  physical_writer_objects_layout position (before ++ object :: after) ->
  physical_writer_object_start object =
    position + length (physical_writer_bytes before).
Proof.
  intros position before. revert position.
  induction before as [|head rest IH]; intros position object after layout.
  - cbn [physical_writer_objects_layout physical_writer_bytes] in layout |- *.
    destruct layout as [objectAt _]. now rewrite Nat.add_0_r.
  - cbn [physical_writer_objects_layout] in layout.
    destruct layout as [headAt restLayout].
    specialize (IH
      (position + length (physical_writer_object_bytes head))
      object after restLayout).
    change (physical_writer_object_start object =
      position +
        length (physical_writer_object_bytes head ++
          physical_writer_bytes rest)).
    rewrite length_app. lia.
Qed.

Lemma physical_writer_layout_focus_bounds : forall position before object after,
  physical_writer_objects_layout position (before ++ object :: after) ->
  position <= physical_writer_object_start object /\
  physical_writer_object_start object +
      length (physical_writer_object_bytes object) <=
    position +
      length (physical_writer_bytes (before ++ object :: after)).
Proof.
  intros position before object after layout.
  pose proof (physical_writer_layout_focus_start
    position before object after layout) as objectStart.
  split; [lia|].
  rewrite physical_writer_bytes_app.
  cbn [physical_writer_bytes].
  change (physical_writer_object_start object +
      length (physical_writer_object_bytes object) <=
    position +
      length (physical_writer_bytes before ++
        (physical_writer_object_bytes object ++
         physical_writer_bytes after))).
  rewrite !length_app. lia.
Qed.

Theorem physical_writer_object_has_exact_image_suffix :
  forall root_id before object after,
    physical_writer_objects_layout ACT_HEADER_LEN
      (before ++ object :: after) ->
    skipn (physical_writer_object_start object)
      (ACT_MAGIC ++ le_fixed 8 root_id ++
       physical_writer_bytes (before ++ object :: after) ++ ACT_TRAILER) =
    physical_writer_object_bytes object ++
      physical_writer_bytes after ++ ACT_TRAILER.
Proof.
  intros rootId before object after layout.
  pose proof (physical_writer_layout_focus_start
    ACT_HEADER_LEN before object after layout) as objectStart.
  set (prefix :=
    ACT_MAGIC ++ le_fixed 8 rootId ++ physical_writer_bytes before).
  replace
    (ACT_MAGIC ++ le_fixed 8 rootId ++
       physical_writer_bytes (before ++ object :: after) ++ ACT_TRAILER)
    with
    (prefix ++
      (physical_writer_object_bytes object ++
       physical_writer_bytes after ++ ACT_TRAILER)).
  2: {
    unfold prefix. rewrite physical_writer_bytes_app.
    rewrite physical_writer_bytes_cons.
    repeat rewrite app_assoc. reflexivity. }
  replace (physical_writer_object_start object) with (length prefix).
  - apply skipn_length_app_exact.
Qed.

Theorem parse_line_object_writer_roundtrip :
  forall line_id arena_end image line suffix,
    ACT_HEADER_LEN <= line_id ->
    line_id < arena_end ->
    line_id +
      (length (encode_act_varint (length line)) + length line) <= arena_end ->
    length line < Nat.pow 256 8 ->
    skipn line_id image = encode_act_varint (length line) ++ line ++ suffix ->
    parse_line_object line_id arena_end image =
      Some {| plo_id := line_id;
              plo_encoded_len :=
                length (encode_act_varint (length line)) + length line;
              plo_bytes := line |}.
Proof.
  intros lineId arenaEnd image line suffix afterHeader beforeEnd fits bounded
    exactSuffix.
  unfold parse_line_object.
  assert (ACT_HEADER_LEN <=? lineId = true) as headerCheck
    by (apply Nat.leb_le; exact afterHeader).
  assert (lineId <? arenaEnd = true) as arenaCheck
    by (apply Nat.ltb_lt; exact beforeEnd).
  rewrite headerCheck, arenaCheck. cbn.
  rewrite exactSuffix.
  rewrite encode_act_varint_parse_roundtrip by exact bounded.
  cbn.
  rewrite take_exact_app_exact.
  assert (0 < length (encode_act_varint (length line)) + length line)
    as nonempty.
  { pose proof (encode_act_varint_nonempty (length line)) as encodedNonempty.
    destruct (encode_act_varint (length line)); [contradiction|cbn; lia]. }
  destruct (length (encode_act_varint (length line)) + length line)
    as [|encodedRest] eqn:encodedLength; [lia|].
  cbn.
  assert ((lineId + S encodedRest <=? arenaEnd) = true) as fitsCheck.
  { apply Nat.leb_le. exact fits. }
  rewrite fitsCheck. reflexivity.
Qed.

Theorem physical_line_object_parses_from_exact_image :
  forall root_id before line_id hash line after,
    physical_writer_objects_layout ACT_HEADER_LEN
      (before ++ WriterLineObject line_id hash line :: after) ->
    length line < Nat.pow 256 8 ->
    parse_line_object line_id
      (ACT_HEADER_LEN +
        length (physical_writer_bytes
          (before ++ WriterLineObject line_id hash line :: after)))
      (ACT_MAGIC ++ le_fixed 8 root_id ++
       physical_writer_bytes
         (before ++ WriterLineObject line_id hash line :: after) ++
       ACT_TRAILER) =
    Some {| plo_id := line_id;
            plo_encoded_len :=
              length (encode_act_varint (length line)) + length line;
            plo_bytes := line |}.
Proof.
  intros rootId before lineId hash line after layout bounded.
  pose proof (physical_writer_layout_focus_start ACT_HEADER_LEN before
    (WriterLineObject lineId hash line) after layout) as lineStart.
  pose proof (physical_writer_layout_focus_bounds ACT_HEADER_LEN before
    (WriterLineObject lineId hash line) after layout) as [afterHeader fits].
  pose proof (physical_writer_object_has_exact_image_suffix rootId before
    (WriterLineObject lineId hash line) after layout) as exactSuffix.
  cbn [physical_writer_object_start physical_writer_object_bytes] in
    lineStart, fits, exactSuffix.
  assert (lineId <
    ACT_HEADER_LEN +
      length (physical_writer_bytes
        (before ++ WriterLineObject lineId hash line :: after))) as beforeEnd.
  { pose proof (encode_act_varint_nonempty (length line)) as nonempty.
    destruct (encode_act_varint (length line)); [contradiction|].
    cbn in fits |- *. lia. }
  eapply parse_line_object_writer_roundtrip with
    (suffix := physical_writer_bytes after ++ ACT_TRAILER).
  - rewrite lineStart. lia.
  - exact beforeEnd.
  - rewrite length_app in fits. exact fits.
  - exact bounded.
  - repeat rewrite <- app_assoc in exactSuffix. exact exactSuffix.
Qed.

Definition pending_branch_encoded_len (origin : nat) (value : option nat)
    (children : list (bbyte * nat)) : nat :=
  1 + length (optional_value_bytes value) +
  match first_child_id children with
  | None => length (encoded_branch_labels (labels_of_children children))
  | Some first =>
      length (relative_bytes origin first) +
      length (encoded_branch_labels (labels_of_children children))
  end.

Lemma pending_branch_encoded_len_exact : forall origin value children,
  length (encode_pending_node origin (PendingBranch value children)) =
    pending_branch_encoded_len origin value children.
Proof.
  intros origin value children.
  unfold encode_pending_node, pending_branch_encoded_len.
  destruct (first_child_id children); repeat rewrite length_app; cbn; lia.
Qed.

Theorem branch_record_writer_roundtrip :
  forall origin value children suffix,
    Forall (fun byte => byte < 256) (labels_of_children children) ->
    StronglySorted Nat.lt (labels_of_children children) ->
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    (forall first, first_child_id children = Some first ->
      first <= origin /\ origin - first < Nat.pow 256 8) ->
    parse_node_record origin
      (encode_pending_node origin (PendingBranch value children) ++ suffix) =
    Some ({| pn_id := origin;
             pn_encoded_len := pending_branch_encoded_len origin value children;
             pn_value := value;
             pn_edge := ParsedBranch (first_child_id children)
               (labels_of_children children) |},
          suffix).
Proof.
  intros origin value children suffix bytes sorted valueBounded firstBounded.
  destruct children as [|[firstLabel firstId] rest].
  - change (labels_of_children []) with (@nil bbyte).
    change (first_child_id []) with (@None nat).
    replace (pending_branch_encoded_len origin value []) with
      (1 + length (optional_value_bytes value)).
    2: { unfold pending_branch_encoded_len.
         change (first_child_id []) with (@None nat).
         change (encoded_branch_labels (labels_of_children [])) with
           (@nil bbyte).
         cbn. lia. }
    apply empty_branch_record_writer_roundtrip. exact valueBounded.
  - assert (first_child_id ((firstLabel, firstId) :: rest) = Some firstId)
      as firstPresent by reflexivity.
    destruct (length (labels_of_children ((firstLabel, firstId) :: rest)) <? 32)
      eqn:sparseDecision.
    + apply Nat.ltb_lt in sparseDecision.
      pose proof (sparse_branch_record_writer_roundtrip origin value
        ((firstLabel, firstId) :: rest) firstId suffix firstPresent
        sparseDecision bytes sorted valueBounded
        (proj1 (firstBounded firstId firstPresent))
        (proj2 (firstBounded firstId firstPresent))) as parsed.
      rewrite parsed. f_equal. f_equal.
      unfold pending_branch_encoded_len, encoded_branch_labels.
      assert ((length (labels_of_children ((firstLabel, firstId) :: rest)) <? 32)
        = true) as sparseCheck by (apply Nat.ltb_lt; exact sparseDecision).
      rewrite firstPresent, sparseCheck. f_equal. lia.
    + apply Nat.ltb_ge in sparseDecision.
      pose proof (dense_branch_record_writer_roundtrip origin value
        ((firstLabel, firstId) :: rest) firstId suffix firstPresent
        sparseDecision bytes sorted valueBounded
        (proj1 (firstBounded firstId firstPresent))
        (proj2 (firstBounded firstId firstPresent))) as parsed.
      rewrite parsed. f_equal. f_equal.
      unfold pending_branch_encoded_len, encoded_branch_labels.
      rewrite firstPresent.
      destruct (length (labels_of_children ((firstLabel, firstId) :: rest)) <? 32)
        eqn:impossible; [apply Nat.ltb_lt in impossible; lia|].
      rewrite dense_label_mask_length. f_equal. lia.
Qed.

Definition pending_line_encoded_len (origin : nat) (value : option nat)
    (child : option nat) (line_id : nat) : nat :=
  1 + length (optional_value_bytes value) +
  length (optional_backward_bytes origin child) +
  length (relative_bytes origin line_id).

Lemma pending_line_encoded_len_exact : forall origin value child line_id,
  length (encode_pending_node origin (PendingLine value child line_id)) =
    pending_line_encoded_len origin value child line_id.
Proof.
  intros origin value [child|] lineId;
    unfold encode_pending_node, pending_line_encoded_len,
      optional_backward_bytes; repeat rewrite length_app; cbn; lia.
Qed.

Theorem line_record_writer_roundtrip_exact_len :
  forall origin value child line_id suffix,
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    (forall id, child = Some id ->
      id <= origin /\ origin - id < Nat.pow 256 8) ->
    line_id <= origin -> origin - line_id < Nat.pow 256 8 ->
    parse_node_record origin
      (encode_pending_node origin (PendingLine value child line_id) ++ suffix) =
    Some ({| pn_id := origin;
             pn_encoded_len :=
               pending_line_encoded_len origin value child line_id;
             pn_value := value;
             pn_edge := ParsedLine child (Some line_id) |},
          suffix).
Proof.
  intros origin value child lineId suffix valueBounded childBounded
    lineBackward lineRelativeBounded.
  apply line_record_writer_roundtrip; assumption.
Qed.

Definition pending_node_writer_domain (origin : nat) (node : pending_node)
    : Prop :=
  match node with
  | PendingBranch value children =>
      Forall (fun byte => byte < 256) (labels_of_children children) /\
      StronglySorted Nat.lt (labels_of_children children) /\
      (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) /\
      (forall first, first_child_id children = Some first ->
        first <= origin /\ origin - first < Nat.pow 256 8)
  | PendingLine value child line_id =>
      (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) /\
      (forall id, child = Some id ->
        id <= origin /\ origin - id < Nat.pow 256 8) /\
      line_id <= origin /\ origin - line_id < Nat.pow 256 8
  end.

Definition pending_node_parsed_model (origin : nat) (node : pending_node)
    : parsed_node :=
  match node with
  | PendingBranch value children =>
      {| pn_id := origin;
         pn_encoded_len := length (encode_pending_node origin node);
         pn_value := value;
         pn_edge := ParsedBranch (first_child_id children)
           (labels_of_children children) |}
  | PendingLine value child line_id =>
      {| pn_id := origin;
         pn_encoded_len := length (encode_pending_node origin node);
         pn_value := value;
         pn_edge := ParsedLine child (Some line_id) |}
  end.

Theorem pending_node_record_writer_roundtrip :
  forall origin node suffix,
    pending_node_writer_domain origin node ->
    parse_node_record origin (encode_pending_node origin node ++ suffix) =
      Some (pending_node_parsed_model origin node, suffix).
Proof.
  intros origin node suffix domain.
  destruct node as [value children|value child lineId].
  - destruct domain as [bytes [sorted [valueBounded firstBounded]]].
    cbn [pending_node_parsed_model].
    rewrite pending_branch_encoded_len_exact.
    apply branch_record_writer_roundtrip; assumption.
  - destruct domain as [valueBounded [childBounded
      [lineBackward lineRelativeBounded]]].
    cbn [pending_node_parsed_model].
    rewrite pending_line_encoded_len_exact.
    apply line_record_writer_roundtrip_exact_len; assumption.
Qed.

Theorem physical_node_object_parses_from_exact_image :
  forall root_id before node_id node after,
    physical_writer_objects_layout ACT_HEADER_LEN
      (before ++ WriterNodeObject node_id node :: after) ->
    pending_node_writer_domain node_id node ->
    parse_node_record node_id
      (skipn node_id
        (ACT_MAGIC ++ le_fixed 8 root_id ++
         physical_writer_bytes
           (before ++ WriterNodeObject node_id node :: after) ++
         ACT_TRAILER)) =
    Some (pending_node_parsed_model node_id node,
      physical_writer_bytes after ++ ACT_TRAILER).
Proof.
  intros rootId before nodeId node after layout domain.
  pose proof (physical_writer_object_has_exact_image_suffix rootId before
    (WriterNodeObject nodeId node) after layout) as exactSuffix.
  cbn [physical_writer_object_start physical_writer_object_bytes]
    in exactSuffix.
  rewrite exactSuffix.
  apply pending_node_record_writer_roundtrip. exact domain.
Qed.

Fixpoint physical_node_projection (objects : list physical_writer_object)
    : list physical_writer_object :=
  match objects with
  | [] => []
  | WriterLineObject _ _ _ :: rest => physical_node_projection rest
  | WriterNodeObject id node :: rest =>
      WriterNodeObject id node :: physical_node_projection rest
  end.

Definition source_semantic_root_object (tree : source_semantic_tree)
    : physical_writer_object :=
  WriterNodeObject (source_semantic_root_id tree)
    (source_semantic_root_node tree).

Fixpoint source_semantic_forest_root_objects
    (forest : source_semantic_forest) : list physical_writer_object :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_root_object child ::
      source_semantic_forest_root_objects rest
  end.

Definition source_semantic_own_wrapper_objects
    (tree : source_semantic_tree) : list physical_writer_object :=
  match tree with
  | SourceSemanticNode _ _ _ _ value children _ _ wrapper =>
      match wrapper with
      | None => []
      | Some (wrapper_id, _) =>
          [WriterNodeObject wrapper_id
            (PendingBranch value
              (combine (source_semantic_forest_labels children)
                (source_semantic_forest_ids children)))]
      end
  end.

Fixpoint source_semantic_pending_node_objects
    (tree : source_semantic_tree) : list physical_writer_object :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      source_semantic_forest_pending_node_objects children ++
      source_semantic_forest_root_objects children ++
      source_semantic_own_wrapper_objects tree
  end
with source_semantic_forest_pending_node_objects
    (forest : source_semantic_forest) : list physical_writer_object :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_pending_node_objects child ++
      source_semantic_forest_pending_node_objects rest
  end.

Definition pending_source_node_objects
    (pending : pending_source_semantic_node) : list physical_writer_object :=
  source_semantic_forest_pending_node_objects (pss_children pending) ++
  source_semantic_forest_root_objects (pss_children pending) ++
  match pss_wrapper pending with
  | None => []
  | Some (wrapper_id, _) =>
      [WriterNodeObject wrapper_id
        (PendingBranch (pss_value pending)
          (combine (source_semantic_forest_labels (pss_children pending))
            (source_semantic_forest_ids (pss_children pending))))]
  end.

Fixpoint pending_source_list_node_objects
    (pending : list pending_source_semantic_node) : list physical_writer_object :=
  match pending with
  | [] => []
  | child :: rest =>
      pending_source_node_objects child ++
      pending_source_list_node_objects rest
  end.

Lemma finalized_pending_node_objects_exact : forall root_id pending,
  source_semantic_pending_node_objects
    (finalize_pending_source_semantic_node root_id pending) =
  pending_source_node_objects pending.
Proof. reflexivity. Qed.

Lemma physical_node_projection_app : forall left right,
  physical_node_projection (left ++ right) =
    physical_node_projection left ++ physical_node_projection right.
Proof.
  induction left as [|object rest IH]; intro right; [reflexivity|].
  destruct object; cbn; rewrite IH; reflexivity.
Qed.

Lemma traced_push_pending_node_appends_projection : forall node state,
  physical_node_projection
    (tws_objects (snd (traced_push_pending_node node state))) =
  physical_node_projection (tws_objects state) ++
    [WriterNodeObject (writer_position (tws_state state)) node].
Proof.
  intros node state. unfold traced_push_pending_node, push_pending_node.
  cbn [snd tws_objects]. rewrite physical_node_projection_app. reflexivity.
Qed.

Lemma traced_add_or_reuse_line_preserves_projection :
  forall line_hash line state,
    physical_node_projection
      (tws_objects (snd (traced_add_or_reuse_line line_hash line state))) =
    physical_node_projection (tws_objects state).
Proof.
  intros line_hash line state. unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line line_hash line (tws_state state))
    as [id after]. cbn.
  destruct (Nat.eqb
    (length (aws_arena after))
    (length (aws_arena (tws_state state)))).
  - reflexivity.
  - rewrite physical_node_projection_app. cbn. now rewrite app_nil_r.
Qed.

Definition line_cache_entry_witnessed
    (objects : list physical_writer_object) (entry : line_cache_entry) : Prop :=
  In (WriterLineObject (lce_id entry) (lce_hash entry) (lce_bytes entry))
    objects.

Lemma remove_line_hash_preserves_witnesses : forall hash objects entries,
  Forall (line_cache_entry_witnessed objects) entries ->
  Forall (line_cache_entry_witnessed objects)
    (remove_line_hash hash entries).
Proof.
  intros hash objects entries witnessed.
  induction witnessed as [|entry rest entryWitness restWitness IH];
    [constructor|].
  cbn [remove_line_hash]. destruct (hash =? lce_hash entry).
  - exact IH.
  - constructor; assumption.
Qed.

Lemma put_line_hash_preserves_witnesses : forall objects entry entries,
  line_cache_entry_witnessed objects entry ->
  Forall (line_cache_entry_witnessed objects) entries ->
  Forall (line_cache_entry_witnessed objects) (put_line_hash entry entries).
Proof.
  intros objects entry entries entryWitness entriesWitness.
  unfold put_line_hash. constructor; [exact entryWitness|].
  apply remove_line_hash_preserves_witnesses. exact entriesWitness.
Qed.

Lemma line_cache_witnesses_monotone : forall smaller larger entries,
  incl smaller larger ->
  Forall (line_cache_entry_witnessed smaller) entries ->
  Forall (line_cache_entry_witnessed larger) entries.
Proof.
  intros smaller larger entries included witnessed.
  apply Forall_forall. intros entry present.
  apply Forall_forall with (x := entry) in witnessed; [|exact present].
  unfold line_cache_entry_witnessed in *. apply included. exact witnessed.
Qed.

Theorem replay_physical_line_cache_is_witnessed :
  forall remaining universe cache,
    incl remaining universe ->
    Forall (line_cache_entry_witnessed universe) cache ->
    Forall (line_cache_entry_witnessed universe)
      (replay_physical_line_cache remaining cache).
Proof.
  induction remaining as [|object rest IH];
    intros universe cache included cacheWitnesses; [exact cacheWitnesses|].
  destruct object as [id hash line|id node].
  - cbn [replay_physical_line_cache]. apply IH.
    + intros candidate present. apply included. now right.
    + apply put_line_hash_preserves_witnesses.
      * unfold line_cache_entry_witnessed. apply included. now left.
      * exact cacheWitnesses.
  - cbn [replay_physical_line_cache]. apply IH.
    + intros candidate present. apply included. now right.
    + exact cacheWitnesses.
Qed.

Corollary replayed_line_cache_entries_are_recorded : forall objects,
  Forall (line_cache_entry_witnessed objects)
    (replay_physical_line_cache objects []).
Proof.
  intro objects. apply replay_physical_line_cache_is_witnessed.
  - apply incl_refl.
  - constructor.
Qed.

Lemma lookup_line_hash_returns_member : forall hash entries entry,
  lookup_line_hash hash entries = Some entry -> In entry entries.
Proof.
  intros hash entries. induction entries as [|candidate rest IH];
    intros entry found; [discriminate found|].
  cbn [lookup_line_hash] in found.
  destruct (hash =? lce_hash candidate) eqn:matches.
  - inversion found; subst. now left.
  - right. apply IH. exact found.
Qed.

Lemma lookup_line_hash_returns_matching_hash : forall hash entries entry,
  lookup_line_hash hash entries = Some entry -> lce_hash entry = hash.
Proof.
  intros hash entries. induction entries as [|candidate rest IH];
    intros entry found; [discriminate found|].
  cbn [lookup_line_hash] in found.
  destruct (hash =? lce_hash candidate) eqn:matches.
  - apply Nat.eqb_eq in matches. inversion found; subst entry.
    symmetry. exact matches.
  - apply IH. exact found.
Qed.

Theorem lookup_replayed_line_cache_has_physical_witness :
  forall objects hash entry,
    lookup_line_hash hash (replay_physical_line_cache objects []) =
      Some entry ->
    line_cache_entry_witnessed objects entry.
Proof.
  intros objects hash entry found.
  pose proof (replayed_line_cache_entries_are_recorded objects) as witnessed.
  apply Forall_forall with (x := entry) in witnessed; [exact witnessed|].
  apply lookup_line_hash_returns_member with (hash := hash). exact found.
Qed.

Lemma add_or_reuse_line_reference_case :
  forall line_hash line state objects id after,
    Forall (line_cache_entry_witnessed objects) (aws_lines state) ->
    add_or_reuse_line line_hash line state = (id, after) ->
    (after = state /\
      In (WriterLineObject id (line_hash line) line) objects) \/
    (id = writer_position state /\
      aws_arena after = aws_arena state ++
        encode_act_varint (length line) ++ line /\
      aws_lines after = put_line_hash
        {| lce_hash := line_hash line; lce_bytes := line; lce_id := id |}
        (aws_lines state)).
Proof.
  intros lineHash line state objects id after cacheWitnessed added.
  unfold add_or_reuse_line in added.
  destruct (lookup_line_hash (lineHash line) (aws_lines state))
    as [previous|] eqn:found.
  - destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line)
      as [lineExact|lineDifferent].
    + pose proof (lookup_line_hash_returns_member
        (lineHash line) (aws_lines state) previous found) as member.
      pose proof (lookup_line_hash_returns_matching_hash
        (lineHash line) (aws_lines state) previous found) as hashExact.
      apply Forall_forall with (x := previous) in cacheWitnessed;
        [|exact member].
      inversion added; subst id after.
      left. split; [reflexivity|].
      unfold line_cache_entry_witnessed in cacheWitnessed.
      destruct previous as [previousHash previousBytes previousId].
      cbn in lineExact, hashExact, cacheWitnessed |- *.
      subst previousBytes previousHash. exact cacheWitnessed.
    + inversion added; subst id after. right. repeat split; reflexivity.
  - inversion added; subst id after. right. repeat split; reflexivity.
Qed.

Theorem traced_add_or_reuse_line_references_recorded_object :
  forall line_hash line state,
    traced_writer_cache_consistent state ->
    let '(id, after) := traced_add_or_reuse_line line_hash line state in
    In (WriterLineObject id (line_hash line) line) (tws_objects after).
Proof.
  intros lineHash line state cacheConsistent.
  unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line lineHash line (tws_state state))
    as [id after] eqn:added. cbn.
  unfold traced_writer_cache_consistent in cacheConsistent.
  assert (Forall (line_cache_entry_witnessed (tws_objects state))
    (aws_lines (tws_state state))) as cacheWitnessed.
  { rewrite cacheConsistent. apply replayed_line_cache_entries_are_recorded. }
  destruct (add_or_reuse_line_reference_case lineHash line
    (tws_state state) (tws_objects state) id after cacheWitnessed added)
    as [[reused witness]|[idExact [arenaExact cacheExact]]].
  - subst after. rewrite Nat.eqb_refl. exact witness.
  - assert (length (aws_arena after) <>
      length (aws_arena (tws_state state))) as arenaLengthChanged.
    { intro samePosition.
      rewrite arenaExact, !length_app in samePosition.
      pose proof (encode_act_varint_nonempty (length line)) as nonempty.
      destruct (encode_act_varint (length line)); [contradiction|cbn in *; lia]. }
    apply Nat.eqb_neq in arenaLengthChanged. rewrite arenaLengthChanged.
    apply in_or_app. right. cbn. left. now rewrite idExact.
Qed.

Definition source_semantic_own_line_objects
    (tree : source_semantic_tree) : list physical_writer_object :=
  match tree with
  | SourceSemanticNode _ _ _ line _ _ line_id line_hash _ =>
      match line_id, line_hash with
      | Some id, Some hash => [WriterLineObject id hash line]
      | _, _ => []
      end
  end.

Fixpoint source_semantic_referenced_line_objects
    (tree : source_semantic_tree) : list physical_writer_object :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      source_semantic_forest_referenced_line_objects children ++
      source_semantic_own_line_objects tree
  end
with source_semantic_forest_referenced_line_objects
    (forest : source_semantic_forest) : list physical_writer_object :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_referenced_line_objects child ++
      source_semantic_forest_referenced_line_objects rest
  end.

Definition pending_source_referenced_line_objects
    (pending : pending_source_semantic_node) : list physical_writer_object :=
  source_semantic_forest_referenced_line_objects (pss_children pending) ++
  match pss_line_id pending, pss_line_hash pending with
  | Some id, Some hash =>
      [WriterLineObject id hash (pss_line pending)]
  | _, _ => []
  end.

Fixpoint pending_source_list_referenced_line_objects
    (pending : list pending_source_semantic_node) : list physical_writer_object :=
  match pending with
  | [] => []
  | child :: rest =>
      pending_source_referenced_line_objects child ++
      pending_source_list_referenced_line_objects rest
  end.

Lemma finalized_pending_referenced_lines_exact : forall root_id pending,
  source_semantic_referenced_line_objects
    (finalize_pending_source_semantic_node root_id pending) =
  pending_source_referenced_line_objects pending.
Proof. reflexivity. Qed.

Lemma traced_push_pending_node_objects_prefix : forall node state,
  exists suffix,
    tws_objects (snd (traced_push_pending_node node state)) =
      tws_objects state ++ suffix.
Proof.
  intros node state. unfold traced_push_pending_node.
  destruct (push_pending_node node (tws_state state)) as [id after];
    exists [WriterNodeObject id node]; reflexivity.
Qed.

Lemma traced_add_or_reuse_line_objects_prefix : forall line_hash line state,
  exists suffix,
    tws_objects (snd (traced_add_or_reuse_line line_hash line state)) =
      tws_objects state ++ suffix.
Proof.
  intros lineHash line state. unfold traced_add_or_reuse_line.
  destruct (add_or_reuse_line lineHash line (tws_state state)) as [id after].
  cbn. destruct (length (aws_arena after) =?
    length (aws_arena (tws_state state))).
  - exists []. now rewrite app_nil_r.
  - exists [WriterLineObject id (lineHash line) line]. reflexivity.
Qed.

Theorem certified_push_pending_children_objects_prefix :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    exists suffix, tws_objects final = tws_objects state ++ suffix.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst. exists []. now rewrite app_nil_r.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst actualFinal.
    destruct (traced_push_pending_node_objects_prefix
      (pss_root child) state) as [childSuffix childPrefix].
    rewrite childRun in childPrefix.
    cbn in childPrefix.
    destruct (IH pending afterChild certifiedRest final restRun)
      as [restSuffix restPrefix].
    exists (childSuffix ++ restSuffix).
    rewrite restPrefix, childPrefix, app_assoc. reflexivity.
Qed.

Theorem certified_source_writer_objects_prefix : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      exists suffix, tws_objects final = tws_objects state ++ suffix) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      exists suffix, tws_objects final = tws_objects state ++ suffix).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    destruct (childrenIH state pendingChildren afterDescendants childrenRun)
      as [descendantSuffix descendantPrefix].
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    destruct (certified_push_pending_children_objects_prefix
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [childrenSuffix childrenPrefix].
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      exists (descendantSuffix ++ childrenSuffix).
      rewrite childrenPrefix, descendantPrefix, app_assoc. reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      destruct (traced_add_or_reuse_line_objects_prefix
        line_hash (first :: rest) afterChildren) as [lineSuffix linePrefix].
      rewrite lineRun in linePrefix.
      cbn in linePrefix.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        exists (descendantSuffix ++ childrenSuffix ++ lineSuffix).
        rewrite linePrefix, childrenPrefix, descendantPrefix.
        repeat rewrite app_assoc. reflexivity.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        destruct (traced_push_pending_node_objects_prefix wrapperNode afterLine)
          as [wrapperSuffix wrapperPrefix].
        rewrite wrapperRun in wrapperPrefix.
        cbn in wrapperPrefix.
        assert (tws_objects afterWrapper = tws_objects state ++
          (descendantSuffix ++ childrenSuffix ++ lineSuffix ++ wrapperSuffix))
          as allPrefix.
        { rewrite wrapperPrefix, linePrefix, childrenPrefix, descendantPrefix.
          repeat rewrite app_assoc. reflexivity. }
        unfold traced_push_pending_node, push_pending_node in wrapperRun.
        inversion wrapperRun; subst wrapperId afterWrapper.
        injection certifiedRun as pendingExact finalExact.
        subst pending final.
        exists (descendantSuffix ++ childrenSuffix ++ lineSuffix ++
          wrapperSuffix).
        exact allPrefix.
  - intros state pending final certifiedRun.
    inversion certifiedRun; subst. exists []. now rewrite app_nil_r.
  - intros label child childIH rest restIH state pending final certifiedRun.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    destruct (childIH state pendingChild afterChild childRun)
      as [childSuffix childPrefix].
    destruct (restIH afterChild pendingRest final restRun)
      as [restSuffix restPrefix].
    exists (childSuffix ++ restSuffix).
    rewrite restPrefix, childPrefix, app_assoc. reflexivity.
Qed.

Theorem certified_source_writer_preserves_full_invariant : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      traced_writer_full_invariant state ->
      traced_writer_full_invariant final) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      traced_writer_full_invariant state ->
      traced_writer_full_invariant final).
Proof.
  intro line_hash. split.
  - intros tree state pending final certifiedRun stateInvariant.
    pose proof (proj1 (certified_source_writer_erases_and_reifies line_hash)
      tree state pending final certifiedRun) as [tracedRun _].
    destruct (proj1 (traced_source_writer_is_total_and_full_invariant line_hash)
      tree state stateInvariant) as
      [actualRoot [actualFinal [actualRun actualInvariant]]].
    rewrite tracedRun in actualRun. inversion actualRun; subst. exact actualInvariant.
  - intros forest state pending final certifiedRun stateInvariant.
    pose proof (proj2 (certified_source_writer_erases_and_reifies line_hash)
      forest state pending final certifiedRun) as [tracedRun _].
    destruct (proj2 (traced_source_writer_is_total_and_full_invariant line_hash)
      forest state stateInvariant) as
      [actualPending [actualFinal [actualRun [_ actualInvariant]]]].
    rewrite tracedRun in actualRun. inversion actualRun; subst. exact actualInvariant.
Qed.

Theorem certified_push_pending_children_referenced_lines_exact :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    source_semantic_forest_referenced_line_objects certified =
      pending_source_list_referenced_line_objects pending.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; reflexivity.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild].
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    cbn [source_semantic_forest_referenced_line_objects
      pending_source_list_referenced_line_objects].
    rewrite finalized_pending_referenced_lines_exact.
    rewrite (IH pending afterChild certifiedRest final restRun). reflexivity.
Qed.

Lemma objects_prefix_incl :
  forall (before after suffix : list physical_writer_object),
  after = before ++ suffix -> incl before after.
Proof.
  intros before after suffix exact object present. subst after.
  apply in_or_app. now left.
Qed.

Theorem certified_source_writer_referenced_lines_are_recorded :
  forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      traced_writer_full_invariant state ->
      incl (pending_source_referenced_line_objects pending)
        (tws_objects final)) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      traced_writer_full_invariant state ->
      incl (pending_source_list_referenced_line_objects pending)
        (tws_objects final)).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun
      stateInvariant.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    pose proof (childrenIH state pendingChildren afterDescendants childrenRun
      stateInvariant) as descendantLines.
    pose proof (proj2 (certified_source_writer_preserves_full_invariant line_hash)
      children state pendingChildren afterDescendants childrenRun stateInvariant)
      as descendantsInvariant.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    pose proof (certified_push_pending_children_referenced_lines_exact
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as childLinesExact.
    destruct (certified_push_pending_children_objects_prefix
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [childrenSuffix childrenPrefix].
    pose proof (certified_push_pending_children_erases
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [tracedPush [_ _]].
    pose proof (traced_push_pending_children_preserves_full_invariant
      (forest_labels children) (map pss_root pendingChildren)
      afterDescendants
      (combine (source_semantic_forest_labels certifiedChildren)
        (source_semantic_forest_ids certifiedChildren))
      afterChildren descendantsInvariant tracedPush) as childrenInvariant.
    assert (incl (source_semantic_forest_referenced_line_objects
      certifiedChildren) (tws_objects afterChildren)) as childLines.
    { rewrite childLinesExact. intros object present.
      apply (objects_prefix_incl (tws_objects afterDescendants)
        (tws_objects afterChildren) childrenSuffix childrenPrefix).
      apply descendantLines. exact present. }
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      unfold pending_source_referenced_line_objects. cbn.
      intros object present. apply in_app_or in present as [present|present].
      * apply childLines. exact present.
      * contradiction.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_references_recorded_object
        line_hash (first :: rest) afterChildren (proj2 childrenInvariant))
        as currentLine.
      rewrite lineRun in currentLine.
      destruct (traced_add_or_reuse_line_objects_prefix
        line_hash (first :: rest) afterChildren) as [lineSuffix linePrefix].
      rewrite lineRun in linePrefix.
      assert (incl (source_semantic_forest_referenced_line_objects
        certifiedChildren) (tws_objects afterLine)) as priorLines.
      { intros object present.
        apply (objects_prefix_incl (tws_objects afterChildren)
          (tws_objects afterLine) lineSuffix linePrefix).
        apply childLines. exact present. }
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        unfold pending_source_referenced_line_objects. cbn.
        intros object present. destruct present as [present|present].
        -- subst object. exact currentLine.
        -- contradiction.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        destruct (traced_push_pending_node_objects_prefix wrapperNode afterLine)
          as [wrapperSuffix wrapperPrefix].
        rewrite wrapperRun in wrapperPrefix.
        inversion certifiedRun; subst pending final.
        unfold pending_source_referenced_line_objects. cbn.
        intros object present. apply in_app_or in present as [present|present].
        -- apply in_or_app. left. apply priorLines. exact present.
        -- destruct present as [present|present].
           ++ subst object.
              apply in_or_app. left. exact currentLine.
           ++ contradiction.
  - intros state pending final certifiedRun stateInvariant.
    inversion certifiedRun; subst pending final.
    intros object present. contradiction.
  - intros label child childIH rest restIH state pending final certifiedRun
      stateInvariant.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    pose proof (childIH state pendingChild afterChild childRun stateInvariant)
      as childLines.
    pose proof (proj1 (certified_source_writer_preserves_full_invariant line_hash)
      child state pendingChild afterChild childRun stateInvariant)
      as childInvariant.
    pose proof (restIH afterChild pendingRest final restRun childInvariant)
      as restLines.
    destruct (proj2 (certified_source_writer_objects_prefix line_hash)
      rest afterChild pendingRest final restRun) as [restSuffix restPrefix].
    cbn [pending_source_list_referenced_line_objects].
    intros object present. apply in_app_or in present as [present|present].
    + apply (objects_prefix_incl (tws_objects afterChild)
        (tws_objects final) restSuffix restPrefix).
      apply childLines. exact present.
    + apply restLines. exact present.
Qed.

Theorem semantic_certificate_referenced_lines_are_recorded :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    incl (source_semantic_referenced_line_objects certificate) objects.
Proof.
  intros lineHash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  assert (traced_writer_full_invariant initial) as initialInvariant
    by (repeat split; reflexivity).
  pose proof (proj1 (certified_source_writer_referenced_lines_are_recorded
    lineHash) tree initial pending beforeRoot built initialInvariant)
    as pendingLines.
  destruct (traced_push_pending_node_objects_prefix
    (pss_root pending) beforeRoot) as [rootSuffix rootPrefix].
  rewrite rootRun in rootPrefix.
  rewrite finalized_pending_referenced_lines_exact.
  intros object present.
  apply (objects_prefix_incl (tws_objects beforeRoot)
    (tws_objects final) rootSuffix rootPrefix).
  apply pendingLines. exact present.
Qed.

Lemma physical_node_projection_in_original : forall objects object,
  In object (physical_node_projection objects) -> In object objects.
Proof.
  induction objects as [|candidate rest IH]; intros object present;
    [contradiction|].
  destruct candidate.
  - right. apply IH. exact present.
  - cbn in present. destruct present as [present|present].
    + now left.
    + right. apply IH. exact present.
Qed.

Lemma physical_node_projection_start_in_original : forall objects id,
  In id
    (map physical_writer_object_start (physical_node_projection objects)) ->
  In id (map physical_writer_object_start objects).
Proof.
  intros objects id present.
  apply in_map_iff in present as [object [idExact objectPresent]].
  apply in_map_iff. exists object. split; [exact idExact|].
  apply physical_node_projection_in_original. exact objectPresent.
Qed.

Lemma physical_node_projection_starts_nodup : forall objects,
  NoDup (map physical_writer_object_start objects) ->
  NoDup
    (map physical_writer_object_start (physical_node_projection objects)).
Proof.
  induction objects as [|object rest IH]; intro startsNoDup.
  - constructor.
  - inversion startsNoDup as [|headId tailIds headFresh tailNoDup];
      subst headId tailIds.
    destruct object as [lineId hash line|nodeId node].
    + cbn [physical_node_projection map]. apply IH. exact tailNoDup.
    + cbn [physical_node_projection map]. constructor.
      * intro present. apply headFresh.
        apply physical_node_projection_start_in_original. exact present.
      * apply IH. exact tailNoDup.
Qed.

Corollary physical_layout_node_starts_nodup : forall position objects,
  physical_writer_objects_layout position objects ->
  NoDup
    (map physical_writer_object_start (physical_node_projection objects)).
Proof.
  intros position objects layout.
  apply physical_node_projection_starts_nodup.
  eapply physical_writer_layout_starts_nodup. exact layout.
Qed.

Theorem semantic_certificate_has_exact_image_and_layout :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    bytes = ACT_MAGIC ++ le_fixed 8 (source_semantic_root_id certificate) ++
      physical_writer_bytes objects ++ ACT_TRAILER /\
    physical_writer_objects_layout ACT_HEADER_LEN objects.
Proof.
  intros lineHash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  assert (traced_writer_full_invariant initial) as initialInvariant
    by (repeat split; reflexivity).
  pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
    tree initial pending beforeRoot built initialInvariant) as beforeInvariant.
  pose proof (traced_push_pending_node_preserves_full_invariant
    (pss_root pending) beforeRoot beforeInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant.
  cbn in finalInvariant.
  split.
  - cbn [source_semantic_root_id].
    unfold traced_writer_full_invariant, traced_writer_invariant in finalInvariant.
    destruct finalInvariant as [[consistent layout] cache].
    unfold traced_writer_consistent in consistent. now rewrite consistent.
  - exact (proj2 (proj1 finalInvariant)).
Qed.

Theorem recorded_physical_node_parses :
  forall root_id objects node_id node,
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    In (WriterNodeObject node_id node) objects ->
    pending_node_writer_domain node_id node ->
    exists before after,
      objects = before ++ WriterNodeObject node_id node :: after /\
      parse_node_record node_id
        (skipn node_id
          (ACT_MAGIC ++ le_fixed 8 root_id ++
           physical_writer_bytes objects ++ ACT_TRAILER)) =
      Some (pending_node_parsed_model node_id node,
        physical_writer_bytes after ++ ACT_TRAILER).
Proof.
  intros rootId objects nodeId node layout present domain.
  apply in_split in present as [before [after objectsExact]].
  exists before, after. split; [exact objectsExact|].
  subst objects.
  apply physical_node_object_parses_from_exact_image; assumption.
Qed.

Theorem recorded_physical_line_parses :
  forall root_id objects line_id hash line,
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    In (WriterLineObject line_id hash line) objects ->
    length line < Nat.pow 256 8 ->
    parse_line_object line_id
      (ACT_HEADER_LEN + length (physical_writer_bytes objects))
      (ACT_MAGIC ++ le_fixed 8 root_id ++
       physical_writer_bytes objects ++ ACT_TRAILER) =
    Some {| plo_id := line_id;
            plo_encoded_len :=
              length (encode_act_varint (length line)) + length line;
            plo_bytes := line |}.
Proof.
  intros rootId objects lineId hash line layout present bounded.
  apply in_split in present as [before [after objectsExact]].
  subst objects.
  apply physical_line_object_parses_from_exact_image; assumption.
Qed.

Theorem certified_push_pending_children_projection_exact :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    physical_node_projection (tws_objects final) =
      physical_node_projection (tws_objects state) ++
      source_semantic_forest_root_objects certified /\
    source_semantic_forest_pending_node_objects certified =
      pending_source_list_node_objects pending.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final. split.
    + cbn [source_semantic_forest_root_objects]. now rewrite app_nil_r.
    + reflexivity.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    destruct (IH pending afterChild certifiedRest final restRun) as
      [restProjection restPending].
    pose proof (traced_push_pending_node_appends_projection
      (pss_root child) state) as childProjection.
    rewrite childRun in childProjection.
    cbn in childProjection.
    pose proof (traced_push_pending_node_erases
      (pss_root child) state rootId afterChild childRun) as childErases.
    pose proof (push_pending_node_position_exact
      (pss_root child) (tws_state state) rootId (tws_state afterChild)
      childErases) as [rootExact _].
    split.
    + cbn [source_semantic_forest_root_objects source_semantic_root_object
        source_semantic_root_id].
      rewrite restProjection, childProjection, rootExact.
      rewrite <- app_assoc. reflexivity.
    + cbn [source_semantic_forest_pending_node_objects
        pending_source_list_node_objects].
      rewrite finalized_pending_node_objects_exact, restPending. reflexivity.
Qed.

Theorem certified_source_writer_projection_exact : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      physical_node_projection (tws_objects final) =
        physical_node_projection (tws_objects state) ++
        pending_source_node_objects pending) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      physical_node_projection (tws_objects final) =
        physical_node_projection (tws_objects state) ++
        pending_source_list_node_objects pending).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    pose proof (childrenIH state pendingChildren afterDescendants childrenRun)
      as descendantsProjection.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    destruct (certified_push_pending_children_projection_exact
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [childrenProjection pendingChildrenExact].
    assert (physical_node_projection (tws_objects afterChildren) =
      physical_node_projection (tws_objects state) ++
      source_semantic_forest_pending_node_objects certifiedChildren ++
      source_semantic_forest_root_objects certifiedChildren)
      as childrenExact.
    { rewrite childrenProjection, descendantsProjection,
        <- pendingChildrenExact. repeat rewrite app_assoc. reflexivity. }
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      unfold pending_source_node_objects. cbn [pss_wrapper pss_children].
      rewrite childrenExact. now repeat rewrite app_nil_r.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_preserves_projection
        line_hash (first :: rest) afterChildren) as lineProjection.
      rewrite lineRun in lineProjection.
      cbn in lineProjection.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        unfold pending_source_node_objects. cbn.
        rewrite lineProjection, childrenExact. reflexivity.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        pose proof (f_equal snd wrapperRun) as afterWrapperState.
        cbn in afterWrapperState.
        pose proof (traced_push_pending_node_appends_projection
          wrapperNode afterLine) as wrapperProjection.
        rewrite wrapperRun in wrapperProjection.
        cbn in wrapperProjection.
        pose proof (traced_push_pending_node_erases
          wrapperNode afterLine wrapperId afterWrapper wrapperRun)
          as wrapperErases.
        pose proof (push_pending_node_position_exact wrapperNode
          (tws_state afterLine) wrapperId (tws_state afterWrapper)
          wrapperErases) as [wrapperExact _].
        subst wrapperNode.
        cbn [source_semantic_forest_labels source_semantic_forest_ids combine]
          in afterWrapperState.
        cbn in certifiedRun.
        rewrite afterWrapperState in certifiedRun.
        inversion certifiedRun; subst pending final.
        unfold pending_source_node_objects. cbn [pss_wrapper pss_children].
        rewrite wrapperProjection, lineProjection, childrenExact.
        repeat rewrite app_assoc. reflexivity.
  - intros state pending final certifiedRun.
    inversion certifiedRun; subst pending final. now rewrite app_nil_r.
  - intros label child childIH rest restIH state pending final certifiedRun.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    pose proof (childIH state pendingChild afterChild childRun)
      as childProjection.
    pose proof (restIH afterChild pendingRest final restRun)
      as restProjection.
    cbn [pending_source_list_node_objects].
    rewrite restProjection, childProjection. repeat rewrite app_assoc.
    reflexivity.
Qed.

Theorem semantic_certificate_node_projection_exact :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    physical_node_projection objects =
      source_semantic_pending_node_objects certificate ++
      [source_semantic_root_object certificate].
Proof.
  intros line_hash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree line_hash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  pose proof (proj1 (certified_source_writer_projection_exact line_hash)
    tree initial pending beforeRoot built) as pendingProjection.
  pose proof (traced_push_pending_node_appends_projection
    (pss_root pending) beforeRoot) as rootProjection.
  rewrite rootRun in rootProjection.
  cbn in rootProjection.
  pose proof (traced_push_pending_node_erases
    (pss_root pending) beforeRoot rootId final rootRun) as rootErases.
  pose proof (push_pending_node_position_exact (pss_root pending)
    (tws_state beforeRoot) rootId (tws_state final) rootErases)
    as [rootExact _].
  rewrite rootProjection, pendingProjection.
  cbn [physical_node_projection tws_objects initial].
  rewrite finalized_pending_node_objects_exact.
  unfold source_semantic_root_object. cbn [source_semantic_root_id
    source_semantic_root_node].
  rewrite rootExact. reflexivity.
Qed.

Theorem semantic_certificate_node_objects_are_recorded :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    incl
      (source_semantic_pending_node_objects certificate ++
       [source_semantic_root_object certificate])
      objects.
Proof.
  intros lineHash tree bytes certificate objects encoded object present.
  apply physical_node_projection_in_original.
  pose proof (semantic_certificate_node_projection_exact
    lineHash tree bytes certificate objects encoded) as projectionExact.
  now rewrite projectionExact.
Qed.

(** The filtered node projection above is useful for global accounting, but
    direct-child parsing needs the stronger physical fact that PathMap emits
    those roots consecutively with no intervening line object.  The following
    certificate lemmas retain that exact suffix and its byte positions. *)

Lemma traced_push_pending_node_result_objects_exact :
  forall node state id final,
    traced_push_pending_node node state = (id, final) ->
    tws_objects final =
      tws_objects state ++ [WriterNodeObject id node].
Proof.
  intros node state id final pushed.
  unfold traced_push_pending_node in pushed.
  destruct (push_pending_node node (tws_state state))
    as [actualId actualFinal] eqn:sourcePush.
  inversion pushed; reflexivity.
Qed.

Theorem certified_push_pending_children_objects_exact :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    tws_objects final =
      tws_objects state ++ source_semantic_forest_root_objects certified.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final.
    cbn [source_semantic_forest_root_objects]. now rewrite app_nil_r.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    pose proof (traced_push_pending_node_result_objects_exact
      (pss_root child) state rootId afterChild childRun) as childObjects.
    pose proof (IH pending afterChild certifiedRest final restRun)
      as restObjects.
    cbn [source_semantic_forest_root_objects source_semantic_root_object
      source_semantic_root_id source_semantic_root_node].
    rewrite restObjects, childObjects, <- app_assoc. reflexivity.
Qed.

Fixpoint source_semantic_forest_root_bytes
    (forest : source_semantic_forest) : nat :=
  match forest with
  | SourceSemanticNil => 0
  | SourceSemanticCons _ child rest =>
      source_semantic_root_encoded_len child +
      source_semantic_forest_root_bytes rest
  end.

Fixpoint source_semantic_forest_root_layout (position : nat)
    (forest : source_semantic_forest) : Prop :=
  match forest with
  | SourceSemanticNil => True
  | SourceSemanticCons _ child rest =>
      source_semantic_root_id child = position /\
      source_semantic_forest_root_layout
        (position + source_semantic_root_encoded_len child) rest
  end.

Theorem certified_push_pending_children_root_layout_exact :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    source_semantic_forest_root_layout
      (writer_position (tws_state state)) certified /\
    writer_position (tws_state final) =
      writer_position (tws_state state) +
      source_semantic_forest_root_bytes certified.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final.
    cbn [source_semantic_forest_root_layout
      source_semantic_forest_root_bytes]. split; [exact I|lia].
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    pose proof (traced_push_pending_node_erases
      (pss_root child) state rootId afterChild childRun) as childErases.
    pose proof (push_pending_node_position_exact
      (pss_root child) (tws_state state) rootId (tws_state afterChild)
      childErases) as [rootExact afterChildExact].
    destruct (IH pending afterChild certifiedRest final restRun)
      as [restLayout finalExact].
    cbn [source_semantic_forest_root_layout
      source_semantic_forest_root_bytes source_semantic_root_id
      source_semantic_root_encoded_len].
    split.
    + split; [exact rootExact|].
      rewrite rootExact.
      cbn [source_semantic_root_encoded_len
        finalize_pending_source_semantic_node].
      rewrite <- afterChildExact. exact restLayout.
    + rewrite finalExact, afterChildExact, rootExact.
      cbn [source_semantic_root_encoded_len
        finalize_pending_source_semantic_node]. lia.
Qed.

Fixpoint source_semantic_forest_root_domains
    (forest : source_semantic_forest) : Prop :=
  match forest with
  | SourceSemanticNil => True
  | SourceSemanticCons _ child rest =>
      source_semantic_root_encoded_len child =
        length (encode_pending_node (source_semantic_root_id child)
          (source_semantic_root_node child)) /\
      pending_node_writer_domain
        (source_semantic_root_id child) (source_semantic_root_node child) /\
      source_semantic_forest_root_domains rest
  end.

Theorem derive_direct_child_ids_from_source_roots :
  forall root_id objects forest cursor,
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    incl (source_semantic_forest_root_objects forest) objects ->
    source_semantic_forest_root_layout cursor forest ->
    source_semantic_forest_root_domains forest ->
    derive_direct_child_ids
      (source_semantic_forest_labels forest) cursor
      (ACT_HEADER_LEN + length (physical_writer_bytes objects))
      (ACT_MAGIC ++ le_fixed 8 root_id ++
       physical_writer_bytes objects ++ ACT_TRAILER) =
      Some (source_semantic_forest_ids forest).
Proof.
  intros rootId objects forest.
  induction forest as [|label child rest restIH];
    intro cursor; intros layout included rootsLayout domains.
  - reflexivity.
  - cbn [source_semantic_forest_root_layout
      source_semantic_forest_root_domains] in rootsLayout, domains.
    destruct rootsLayout as [childAt restLayout].
    destruct domains as [childLengthExact [childDomain restDomains]].
    assert (In (source_semantic_root_object child) objects) as childPresent.
    { apply included. cbn [source_semantic_forest_root_objects]. now left. }
    unfold source_semantic_root_object in childPresent.
    pose proof (recorded_physical_node_parses rootId objects
      (source_semantic_root_id child) (source_semantic_root_node child)
      layout childPresent childDomain) as childParses.
    destruct childParses as [before [after [objectsExact parsed]]].
    pose proof (physical_writer_layout_focus_bounds ACT_HEADER_LEN before
      (WriterNodeObject (source_semantic_root_id child)
        (source_semantic_root_node child)) after) as childBounds.
    rewrite <- objectsExact in childBounds.
    specialize (childBounds layout).
    destruct childBounds as [childAfterHeader childEndsInArena].
    assert (0 < source_semantic_root_encoded_len child) as childPositive.
    { rewrite childLengthExact.
      pose proof (every_pending_node_encoding_is_nonempty
        (source_semantic_root_id child) (source_semantic_root_node child))
        as nonempty.
      destruct (encode_pending_node (source_semantic_root_id child)
        (source_semantic_root_node child)); [contradiction|cbn; lia]. }
    assert (source_semantic_root_id child +
      source_semantic_root_encoded_len child <=
      ACT_HEADER_LEN + length (physical_writer_bytes objects)) as childFits.
    { destruct child as [childId childLen childNode line value children
        lineId lineHash wrapper].
      cbn [source_semantic_root_id source_semantic_root_encoded_len
        source_semantic_root_node physical_writer_object_start
        physical_writer_object_bytes] in childLengthExact,
        childEndsInArena |- *.
      rewrite childLengthExact.
      exact childEndsInArena. }
    cbn [source_semantic_forest_labels source_semantic_forest_ids
      derive_direct_child_ids].
    rewrite childAt in parsed, childLengthExact, childFits.
    destruct child as [childId childLen childNode line value children
      lineId lineHash wrapper].
    cbn [source_semantic_root_id source_semantic_root_encoded_len
      source_semantic_root_node pending_node_parsed_model] in childAt, parsed,
      childLengthExact, childPositive, childFits, restLayout |- *.
    subst childId.
    rewrite childLengthExact in childPositive, childFits, restLayout.
    rewrite parsed.
    assert (pn_encoded_len (pending_node_parsed_model cursor childNode) =
      length (encode_pending_node cursor childNode)) as parsedLength
      by (destruct childNode; reflexivity).
    rewrite parsedLength.
    replace (0 <? length (encode_pending_node cursor childNode)) with true
      by (symmetry; apply Nat.ltb_lt; exact childPositive).
    replace (cursor + length (encode_pending_node cursor childNode) <=?
      ACT_HEADER_LEN + length (physical_writer_bytes objects)) with true
      by (symmetry; apply Nat.leb_le; exact childFits).
    cbn [andb].
    assert (incl (source_semantic_forest_root_objects rest) objects)
      as restIncluded.
    { intros object present. apply included.
      cbn [source_semantic_forest_root_objects]. now right. }
    rewrite (restIH
      (cursor + length (encode_pending_node cursor childNode))
      layout restIncluded restLayout restDomains).
    reflexivity.
Qed.

Fixpoint source_semantic_tree_shape (tree : source_semantic_tree) : Prop :=
  match tree with
  | SourceSemanticNode rootId rootLen rootNode line value children
      lineId lineHash wrapper =>
      source_semantic_forest_shape children /\
      rootLen = length (encode_pending_node rootId rootNode) /\
      match line, children with
      | [], _ =>
          lineId = None /\ lineHash = None /\ wrapper = None /\
          rootNode = PendingBranch value
            (combine (source_semantic_forest_labels children)
              (source_semantic_forest_ids children))
      | first :: rest, SourceSemanticNil =>
          exists physicalLineId hash,
            lineId = Some physicalLineId /\
            lineHash = Some hash /\
            wrapper = None /\
            rootNode = PendingLine value None physicalLineId
      | first :: rest, SourceSemanticCons label child certifiedRest =>
          exists physicalLineId hash wrapperId wrapperLen,
            lineId = Some physicalLineId /\
            lineHash = Some hash /\
            wrapper = Some (wrapperId, wrapperLen) /\
            wrapperLen = length (encode_pending_node wrapperId
              (PendingBranch value
                (combine
                  (source_semantic_forest_labels
                    (SourceSemanticCons label child certifiedRest))
                  (source_semantic_forest_ids
                    (SourceSemanticCons label child certifiedRest))))) /\
            rootNode = PendingLine None (Some wrapperId) physicalLineId
      end
  end
with source_semantic_forest_shape
    (forest : source_semantic_forest) : Prop :=
  match forest with
  | SourceSemanticNil => True
  | SourceSemanticCons _ child rest =>
      source_semantic_tree_shape child /\
      source_semantic_forest_shape rest
  end.

Definition pending_source_semantic_shape
    (pending : pending_source_semantic_node) : Prop :=
  source_semantic_forest_shape (pss_children pending) /\
  match pss_line pending, pss_children pending with
  | [], _ =>
      pss_line_id pending = None /\
      pss_line_hash pending = None /\
      pss_wrapper pending = None /\
      pss_root pending = PendingBranch (pss_value pending)
        (combine (source_semantic_forest_labels (pss_children pending))
          (source_semantic_forest_ids (pss_children pending)))
  | first :: rest, SourceSemanticNil =>
      exists physicalLineId hash,
        pss_line_id pending = Some physicalLineId /\
        pss_line_hash pending = Some hash /\
        pss_wrapper pending = None /\
        pss_root pending = PendingLine (pss_value pending) None physicalLineId
  | first :: rest, SourceSemanticCons label child certifiedRest =>
      exists physicalLineId hash wrapperId wrapperLen,
        pss_line_id pending = Some physicalLineId /\
        pss_line_hash pending = Some hash /\
        pss_wrapper pending = Some (wrapperId, wrapperLen) /\
        wrapperLen = length (encode_pending_node wrapperId
          (PendingBranch (pss_value pending)
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest))))) /\
        pss_root pending = PendingLine None (Some wrapperId) physicalLineId
  end.

Lemma finalize_pending_source_semantic_node_preserves_shape :
  forall rootId pending,
    pending_source_semantic_shape pending ->
    source_semantic_tree_shape
      (finalize_pending_source_semantic_node rootId pending).
Proof.
  intros rootId pending shaped.
  destruct pending as [line value children lineId lineHash wrapper rootNode].
  unfold pending_source_semantic_shape in shaped.
  cbn [finalize_pending_source_semantic_node source_semantic_tree_shape
    pss_line pss_value pss_children pss_line_id pss_line_hash pss_wrapper
    pss_root] in shaped |- *.
  destruct shaped as [childrenShape localShape].
  repeat split; assumption.
Qed.

Theorem certified_push_pending_children_preserves_shape :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    Forall pending_source_semantic_shape pending ->
    source_semantic_forest_shape certified.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun pendingShapes;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final. exact I.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    inversion pendingShapes as [|actualChild actualPending childShape restShapes];
      subst actualChild actualPending.
    cbn [source_semantic_forest_shape]. split.
    + apply finalize_pending_source_semantic_node_preserves_shape.
      exact childShape.
    + apply (IH pending afterChild certifiedRest final restRun restShapes).
Qed.

Theorem certified_source_writer_preserves_shape : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      pending_source_semantic_shape pending) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      Forall pending_source_semantic_shape pending).
Proof.
  intro line_hash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final certifiedRun.
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest line_hash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    pose proof (childrenIH state pendingChildren afterDescendants childrenRun)
      as pendingChildrenShapes.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    pose proof (certified_push_pending_children_preserves_shape
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun pendingChildrenShapes)
      as certifiedChildrenShape.
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      unfold pending_source_semantic_shape. cbn.
      split; [exact certifiedChildrenShape|]. repeat split; reflexivity.
    + destruct (traced_add_or_reuse_line line_hash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        unfold pending_source_semantic_shape. cbn.
        split; [exact certifiedChildrenShape|].
        exists lineId, (line_hash (first :: rest)).
        repeat split; reflexivity.
      *
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine]
          in certifiedRun.
        inversion certifiedRun; subst pending final.
        unfold pending_source_semantic_shape. cbn.
        split; [exact certifiedChildrenShape|].
        exists lineId, (line_hash (first :: rest)),
          (writer_position (tws_state afterLine)),
          (length (encode_pending_node
            (writer_position (tws_state afterLine))
            (PendingBranch value
              ((label, source_semantic_root_id child) ::
               combine (source_semantic_forest_labels certifiedRest)
                 (source_semantic_forest_ids certifiedRest))))).
        repeat split; reflexivity.
  - intros state pending final certifiedRun.
    inversion certifiedRun; subst pending final. constructor.
  - intros label child childIH rest restIH state pending final certifiedRun.
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree line_hash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest line_hash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    constructor.
    + apply (childIH state pendingChild afterChild childRun).
    + apply (restIH afterChild pendingRest final restRun).
Qed.

Theorem semantic_certificate_has_exact_shape :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_tree_shape certificate.
Proof.
  intros lineHash tree bytes certificate objects encoded.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  apply finalize_pending_source_semantic_node_preserves_shape.
  apply (proj1 (certified_source_writer_preserves_shape lineHash)
    tree initial pending beforeRoot built).
Qed.

Lemma source_semantic_forest_combine_labels : forall forest,
  labels_of_children
    (combine (source_semantic_forest_labels forest)
      (source_semantic_forest_ids forest)) =
  source_semantic_forest_labels forest.
Proof.
  induction forest as [|label child rest IH]; [reflexivity|].
  change (label :: labels_of_children
    (combine (source_semantic_forest_labels rest)
      (source_semantic_forest_ids rest)) =
    label :: source_semantic_forest_labels rest).
  now rewrite IH.
Qed.

Lemma source_semantic_forest_combine_ids : forall forest,
  map snd
    (combine (source_semantic_forest_labels forest)
      (source_semantic_forest_ids forest)) =
  source_semantic_forest_ids forest.
Proof.
  induction forest as [|label child rest IH]; [reflexivity|].
  change (source_semantic_root_id child ::
    map snd
      (combine (source_semantic_forest_labels rest)
        (source_semantic_forest_ids rest)) =
    source_semantic_root_id child :: source_semantic_forest_ids rest).
  now rewrite IH.
Qed.

Lemma source_semantic_forest_first_child_id : forall forest,
  first_child_id
    (combine (source_semantic_forest_labels forest)
      (source_semantic_forest_ids forest)) =
  match source_semantic_forest_ids forest with
  | [] => None
  | first :: _ => Some first
  end.
Proof. intros [|label child rest]; reflexivity. Qed.

Definition option_nat_is_none (value : option nat) : bool :=
  match value with None => true | Some _ => false end.

Definition no_value_free_unary_semantic (value : option nat)
    (children : list nat) : bool :=
  negb (option_nat_is_none value && Nat.eqb (length children) 1).

Definition parsed_node_fits_arena (arena_end : nat) (node : parsed_node) : bool :=
  (0 <? pn_encoded_len node) &&
  (pn_id node + pn_encoded_len node <=? arena_end).

Definition line_object_precedes_node (node_id : nat)
    (line : parsed_line_object) : bool :=
  negb (Nat.eqb (length (plo_bytes line)) 0) &&
  (plo_id line <? node_id) &&
  (plo_id line + plo_encoded_len line <=? node_id).

Definition source_semantic_wrapper_node (value : option nat)
    (children : source_semantic_forest) : pending_node :=
  PendingBranch value
    (combine (source_semantic_forest_labels children)
      (source_semantic_forest_ids children)).

Definition source_semantic_children_root_layout
    (children : source_semantic_forest) : Prop :=
  match source_semantic_forest_ids children with
  | [] => True
  | first :: _ => source_semantic_forest_root_layout first children
  end.

(** Local direct-parser obligations are mathematical producer facts, not a
    restatement of parser success.  They isolate byte bounds, backward
    references, canonical child layout, and the three physical object forms
    so the construction proof can discharge each concern independently. *)
Definition source_semantic_direct_domain (arena_end : nat)
    (tree : source_semantic_tree) : Prop :=
  match tree with
  | SourceSemanticNode rootId rootLen rootNode line value children
      lineId lineHash wrapper =>
      ACT_HEADER_LEN <= rootId /\
      0 < rootLen /\
      rootId + rootLen <= arena_end /\
      pending_node_writer_domain rootId rootNode /\
      source_semantic_forest_root_domains children /\
      source_semantic_children_root_layout children /\
      Forall
        (fun childId => childId <
          match wrapper with Some (wrapperId, _) => wrapperId
          | None => rootId end)
        (source_semantic_forest_ids children) /\
      ~ (value = None /\
         length (source_semantic_forest_ids children) = 1) /\
      (match lineId, lineHash with
       | None, None => True
       | Some physicalLineId, Some _ =>
           ACT_HEADER_LEN <= physicalLineId /\
           0 < length line /\
           length line < Nat.pow 256 8 /\
           physicalLineId +
             (length (encode_act_varint (length line)) + length line) <= rootId
       | _, _ => False
       end) /\
      match wrapper with
      | None => True
      | Some (wrapperId, wrapperLen) =>
          ACT_HEADER_LEN <= wrapperId /\
          0 < wrapperLen /\
          wrapperId + wrapperLen <= rootId /\
          pending_node_writer_domain wrapperId
            (source_semantic_wrapper_node value children) /\
          source_semantic_forest_ids children <> []
      end
  end.

Lemma source_semantic_direct_domain_mono : forall smaller larger tree,
  smaller <= larger ->
  source_semantic_direct_domain smaller tree ->
  source_semantic_direct_domain larger tree.
Proof.
  intros smaller larger
    [rootId rootLen rootNode line value children lineId lineHash wrapper]
    grows domain.
  cbn [source_semantic_direct_domain] in domain |- *.
  destruct domain as
    [rootAfterHeader [rootPositive [rootFits [rootDomain
    [childrenDomains [childrenLayout [childrenBackward
    [noUnary [lineDomain wrapperDomain]]]]]]]]].
  repeat split; try assumption; lia.
Qed.

Lemma physical_writer_layout_member_bounds : forall position objects object,
  physical_writer_objects_layout position objects ->
  In object objects ->
  position <= physical_writer_object_start object /\
  physical_writer_object_start object +
    length (physical_writer_object_bytes object) <=
    position + length (physical_writer_bytes objects).
Proof.
  intros position objects. revert position.
  induction objects as [|head rest IH]; intros position object layout present;
    [contradiction|].
  cbn [physical_writer_objects_layout] in layout.
  destruct layout as [headAt restLayout].
  cbn in present. destruct present as [present|present].
  - subst object. split; [lia|].
    rewrite physical_writer_bytes_cons, length_app. lia.
  - specialize (IH
      (position + length (physical_writer_object_bytes head))
      object restLayout present) as [afterHead endsInRest].
    split; [lia|].
    rewrite physical_writer_bytes_cons, length_app. lia.
Qed.

Lemma traced_writer_recorded_object_bounds : forall state object,
  traced_writer_invariant state ->
  In object (tws_objects state) ->
  ACT_HEADER_LEN <= physical_writer_object_start object /\
  physical_writer_object_start object +
    length (physical_writer_object_bytes object) <=
    writer_position (tws_state state).
Proof.
  intros state object [consistent layout] present.
  pose proof (physical_writer_layout_member_bounds ACT_HEADER_LEN
    (tws_objects state) object layout present) as [afterHeader bounded].
  split; [exact afterHeader|].
  unfold writer_position, traced_writer_consistent in *.
  now rewrite consistent.
Qed.

Lemma forallb_nat_ltb_true : forall bound values,
  Forall (fun value => value < bound) values ->
  forallb (fun value => value <? bound) values = true.
Proof.
  intros bound values bounded.
  induction bounded as [|value rest valueBound restBound IH]; [reflexivity|].
  change ((value <? bound) &&
    forallb (fun candidate => candidate <? bound) rest = true).
  assert (value <? bound = true) as valueTrue
    by (apply Nat.ltb_lt; exact valueBound).
  now rewrite valueTrue, IH.
Qed.

Lemma no_value_free_unary_semantic_true : forall value children,
  ~ (value = None /\ length children = 1) ->
  no_value_free_unary_semantic value children = true.
Proof.
  intros [value|] children noUnary; [reflexivity|].
  destruct children as [|child rest]; [reflexivity|].
  destruct rest as [|next rest]; [exfalso; apply noUnary; auto|reflexivity].
Qed.

Lemma parsed_node_fits_arena_true : forall arenaEnd node,
  0 < pn_encoded_len node ->
  pn_id node + pn_encoded_len node <= arenaEnd ->
  parsed_node_fits_arena arenaEnd node = true.
Proof.
  intros arenaEnd node positive bounded.
  unfold parsed_node_fits_arena.
  assert (0 <? pn_encoded_len node = true) as positiveBool
    by (apply Nat.ltb_lt; exact positive).
  assert (pn_id node + pn_encoded_len node <=? arenaEnd = true)
    as boundedBool by (apply Nat.leb_le; exact bounded).
  now rewrite positiveBool, boundedBool.
Qed.

Lemma line_object_precedes_node_true : forall nodeId line,
  0 < length (plo_bytes line) ->
  0 < plo_encoded_len line ->
  plo_id line + plo_encoded_len line <= nodeId ->
  line_object_precedes_node nodeId line = true.
Proof.
  intros nodeId line nonempty encodedPositive bounded.
  unfold line_object_precedes_node.
  assert (length (plo_bytes line) =? 0 = false) as nonemptyBool
    by (apply Nat.eqb_neq; lia).
  assert (plo_id line <? nodeId = true) as precedesBool
    by (apply Nat.ltb_lt; lia).
  assert (plo_id line + plo_encoded_len line <=? nodeId = true)
    as boundedBool by (apply Nat.leb_le; exact bounded).
  now rewrite nonemptyBool, precedesBool, boundedBool.
Qed.

Lemma source_semantic_child_roots_in_pending : forall tree,
  incl
    (match tree with
     | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
         source_semantic_forest_root_objects children
     end)
    (source_semantic_pending_node_objects tree).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    object present.
  cbn [source_semantic_pending_node_objects].
  apply in_or_app. right. apply in_or_app. now left.
Qed.

Lemma source_semantic_own_wrapper_in_pending : forall tree object,
  In object (source_semantic_own_wrapper_objects tree) ->
  In object (source_semantic_pending_node_objects tree).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    object present.
  cbn [source_semantic_pending_node_objects] in present |- *.
  apply in_or_app. right. apply in_or_app. now right.
Qed.

Lemma source_semantic_own_line_in_referenced : forall tree object,
  In object (source_semantic_own_line_objects tree) ->
  In object (source_semantic_referenced_line_objects tree).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    object present.
  cbn [source_semantic_referenced_line_objects] in present |- *.
  apply in_or_app. now right.
Qed.

Lemma branch_references_source_semantic_children :
  forall rootId objects origin value children,
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    incl (source_semantic_forest_root_objects children) objects ->
    source_semantic_children_root_layout children ->
    source_semantic_forest_root_domains children ->
    branch_references
      (pending_node_parsed_model origin
        (source_semantic_wrapper_node value children))
      (ACT_HEADER_LEN + length (physical_writer_bytes objects))
      (ACT_MAGIC ++ le_fixed 8 rootId ++
       physical_writer_bytes objects ++ ACT_TRAILER) =
      Some (source_semantic_forest_ids children).
Proof.
  intros rootId objects origin value children layout included
    childrenLayout domains.
  destruct children as [|label child rest].
  - reflexivity.
  - unfold branch_references, source_semantic_wrapper_node.
    cbn [pending_node_parsed_model].
    rewrite source_semantic_forest_combine_labels.
    rewrite source_semantic_forest_first_child_id.
    cbn [source_semantic_forest_ids].
    cbn [source_semantic_children_root_layout] in childrenLayout.
    apply derive_direct_child_ids_from_source_roots; assumption.
Qed.

Definition direct_semantic_node_at (id arena_end : nat) (image : list bbyte)
    : option direct_semantic_node_model :=
  if (ACT_HEADER_LEN <=? id) && (id <? arena_end) then
  match parse_node_record id (skipn id image) with
  | None => None
  | Some (node, _) =>
      if parsed_node_fits_arena arena_end node then
        match pn_edge node with
        | ParsedBranch _ labels =>
            match branch_references node arena_end image with
            | None => None
            | Some children =>
                if forallb (fun child => child <? id) children &&
                   no_value_free_unary_semantic (pn_value node) children
                then Some
                  {| dsm_id := id;
                     dsm_encoded_len := pn_encoded_len node;
                     dsm_value := pn_value node;
                     dsm_child_parent := id;
                     dsm_child_labels := labels;
                     dsm_child_ids := children;
                     dsm_line := None;
                     dsm_wrapper := None |}
                else None
            end
        | ParsedLine child line_id =>
            match line_id with
            | None => None
            | Some physical_line_id =>
                match parse_line_object physical_line_id arena_end image with
                | None => None
                | Some line =>
                    if line_object_precedes_node id line then
                      match child with
                      | None =>
                          Some
                            {| dsm_id := id;
                               dsm_encoded_len := pn_encoded_len node;
                               dsm_value := pn_value node;
                               dsm_child_parent := id;
                               dsm_child_labels := [];
                               dsm_child_ids := [];
                               dsm_line := Some line;
                               dsm_wrapper := None |}
                      | Some wrapper_id =>
                          if option_nat_is_none (pn_value node) &&
                             (wrapper_id <? id) then
                            match parse_node_record wrapper_id
                                    (skipn wrapper_id image) with
                            | None => None
                            | Some (wrapper, _) =>
                                if parsed_node_fits_arena arena_end wrapper &&
                                   (wrapper_id + pn_encoded_len wrapper <=? id)
                                then
                                  match pn_edge wrapper with
                                  | ParsedLine _ _ => None
                                  | ParsedBranch _ wrapper_labels =>
                                      match branch_references wrapper arena_end image with
                                      | None => None
                                      | Some children =>
                                          if negb (Nat.eqb (length children) 0) &&
                                             forallb
                                               (fun child => child <? wrapper_id)
                                               children &&
                                             no_value_free_unary_semantic
                                               (pn_value wrapper) children
                                          then Some
                                            {| dsm_id := id;
                                               dsm_encoded_len := pn_encoded_len node;
                                               dsm_value := pn_value wrapper;
                                               dsm_child_parent := wrapper_id;
                                               dsm_child_labels := wrapper_labels;
                                               dsm_child_ids := children;
                                               dsm_line := Some line;
                                               dsm_wrapper :=
                                                 Some (wrapper_id,
                                                   pn_encoded_len wrapper) |}
                                          else None
                                      end
                                  end
                                else None
                            end
                          else None
                      end
                    else None
                end
            end
        end
      else None
  end
  else None.

Lemma pending_node_parsed_model_encoded_len : forall origin node,
  pn_encoded_len (pending_node_parsed_model origin node) =
    length (encode_pending_node origin node).
Proof. intros origin [value children|value child lineId]; reflexivity. Qed.

Lemma pending_node_parsed_model_id : forall origin node,
  pn_id (pending_node_parsed_model origin node) = origin.
Proof. intros origin [value children|value child lineId]; reflexivity. Qed.

Theorem recorded_source_semantic_node_parses_directly :
  forall global_root_id bytes certificate objects,
    bytes = ACT_MAGIC ++ le_fixed 8 global_root_id ++
      physical_writer_bytes objects ++ ACT_TRAILER ->
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    source_semantic_tree_shape certificate ->
    incl
      (source_semantic_pending_node_objects certificate ++
       [source_semantic_root_object certificate]) objects ->
    incl (source_semantic_referenced_line_objects certificate) objects ->
    source_semantic_direct_domain
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) certificate ->
    direct_semantic_node_at
      (source_semantic_root_id certificate)
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes =
      Some (source_semantic_erase certificate).
Proof.
  intros globalRootId bytes certificate objects bytesExact layout
    certificateShape nodesRecorded linesRecorded directDomain.
  destruct certificate as
    [rootId rootLen rootNode line value children lineId lineHashId wrapper].
  cbn [source_semantic_direct_domain source_semantic_root_id
    source_semantic_erase] in directDomain |- *.
  cbn [source_semantic_tree_shape] in certificateShape.
  destruct certificateShape as
    [childrenShape [rootLengthExact localShape]].
  destruct directDomain as
    [rootAfterHeader [rootPositive [rootFits [rootDomain
    [childrenDomains [childrenLayout [childrenBackward
    [noUnary [lineDomain wrapperDomain]]]]]]]]].
  assert (In (WriterNodeObject rootId rootNode) objects) as rootPresent.
  { apply nodesRecorded.
    cbn [source_semantic_pending_node_objects source_semantic_root_object
      source_semantic_root_id source_semantic_root_node].
    apply in_or_app. right. now left. }
  pose proof (recorded_physical_node_parses globalRootId objects rootId rootNode
    layout rootPresent rootDomain) as rootParses.
  destruct rootParses as [rootBefore [rootAfter [_ rootParsed]]].
  assert (incl (source_semantic_forest_root_objects children) objects)
    as childrenRecorded.
  { intros object present. apply nodesRecorded.
    apply in_or_app. left.
    apply (source_semantic_child_roots_in_pending
      (SourceSemanticNode rootId rootLen rootNode line value children
        lineId lineHashId wrapper)). exact present. }
  assert ((ACT_HEADER_LEN <=? rootId) &&
    (rootId <? ACT_HEADER_LEN + length (physical_writer_bytes objects)) = true)
    as rootGate.
  { apply andb_true_iff. split.
    - apply Nat.leb_le. exact rootAfterHeader.
    - apply Nat.ltb_lt. lia. }
  assert (parsed_node_fits_arena
    (ACT_HEADER_LEN + length (physical_writer_bytes objects))
    (pending_node_parsed_model rootId rootNode) = true) as rootParsedFits.
  { apply parsed_node_fits_arena_true.
    - rewrite pending_node_parsed_model_encoded_len, <- rootLengthExact.
      exact rootPositive.
    - rewrite pending_node_parsed_model_id,
        pending_node_parsed_model_encoded_len, <- rootLengthExact.
      exact rootFits. }
  rewrite bytesExact.
  unfold direct_semantic_node_at.
  rewrite rootGate.
  cbn [source_semantic_root_id].
  rewrite rootParsed, rootParsedFits.
  destruct line as [|first rest].
  - destruct localShape as
      [lineIdExact [lineHashExact [wrapperExact rootNodeExact]]].
    subst lineId lineHashId wrapper rootNode.
    pose proof (branch_references_source_semantic_children globalRootId objects
      rootId value children layout childrenRecorded childrenLayout
      childrenDomains) as childReferences.
    unfold source_semantic_wrapper_node in childReferences.
    cbn [pending_node_parsed_model source_semantic_wrapper_node]
      in childReferences.
    cbn [pending_node_parsed_model] in rootParsedFits |- *.
    rewrite childReferences.
    rewrite source_semantic_forest_combine_labels.
    assert (forallb (fun childId => childId <? rootId)
      (source_semantic_forest_ids children) = true) as childrenBackwardBool.
    { apply forallb_nat_ltb_true. exact childrenBackward. }
    rewrite childrenBackwardBool.
    rewrite no_value_free_unary_semantic_true by exact noUnary.
    cbn [pn_edge pn_encoded_len pn_value andb].
    rewrite rootLengthExact. reflexivity.
  - destruct children as [|label child certifiedRest].
    + destruct localShape as
        [physicalLineId [hash [lineIdExact [lineHashExact
        [wrapperExact rootNodeExact]]]]].
      subst lineId lineHashId wrapper rootNode.
      destruct lineDomain as
        [lineAfterHeader [lineNonempty [lineBound lineBeforeRoot]]].
      assert (In (WriterLineObject physicalLineId hash (first :: rest)) objects)
        as linePresent.
      { apply linesRecorded.
        apply source_semantic_own_line_in_referenced.
        cbn [source_semantic_own_line_objects]. now left. }
      pose proof (recorded_physical_line_parses globalRootId objects physicalLineId
        hash (first :: rest) layout linePresent lineBound) as lineParsed.
      assert (0 < length (encode_act_varint (length (first :: rest))) +
        length (first :: rest)) as lineEncodedPositive.
      { pose proof (encode_act_varint_nonempty (length (first :: rest)))
          as varintNonempty.
        destruct (encode_act_varint (length (first :: rest)));
          [contradiction|cbn; lia]. }
      pose proof (line_object_precedes_node_true rootId
        {| plo_id := physicalLineId;
           plo_encoded_len :=
             length (encode_act_varint (length (first :: rest))) +
             length (first :: rest);
           plo_bytes := first :: rest |}
        ltac:(cbn; lia) lineEncodedPositive lineBeforeRoot) as linePrecedes.
      cbn [pending_node_parsed_model] in rootParsedFits |- *.
      cbn [pn_edge pn_value pn_encoded_len].
      rewrite lineParsed, linePrecedes, rootLengthExact. reflexivity.
    + destruct localShape as
        [physicalLineId [hash [wrapperId [wrapperLen
        [lineIdExact [lineHashExact [wrapperExact
        [wrapperLengthExact rootNodeExact]]]]]]]].
      subst lineId lineHashId wrapper rootNode.
      destruct lineDomain as
        [lineAfterHeader [lineNonempty [lineBound lineBeforeRoot]]].
      destruct wrapperDomain as
        [wrapperAfterHeader [wrapperPositive [wrapperBeforeRoot
        [wrapperWriterDomain childrenNonempty]]]].
      assert (In (WriterLineObject physicalLineId hash (first :: rest)) objects)
        as linePresent.
      { apply linesRecorded.
        apply source_semantic_own_line_in_referenced.
        cbn [source_semantic_own_line_objects]. now left. }
      pose proof (recorded_physical_line_parses globalRootId objects physicalLineId
        hash (first :: rest) layout linePresent lineBound) as lineParsed.
      set (wrapperNode := source_semantic_wrapper_node value
        (SourceSemanticCons label child certifiedRest)).
      assert (wrapperLen = length (encode_pending_node wrapperId wrapperNode))
        as wrapperNodeLengthExact.
      { unfold wrapperNode, source_semantic_wrapper_node.
        exact wrapperLengthExact. }
      assert (In (WriterNodeObject wrapperId wrapperNode) objects)
        as wrapperPresent.
      { apply nodesRecorded. apply in_or_app. left.
        apply (source_semantic_own_wrapper_in_pending
          (SourceSemanticNode rootId rootLen
            (PendingLine None (Some wrapperId) physicalLineId)
            (first :: rest) value
            (SourceSemanticCons label child certifiedRest)
            (Some physicalLineId) (Some hash)
            (Some (wrapperId, wrapperLen)))).
        cbn [source_semantic_own_wrapper_objects]. now left. }
      pose proof (recorded_physical_node_parses globalRootId objects wrapperId
        wrapperNode layout wrapperPresent wrapperWriterDomain) as wrapperParses.
      destruct wrapperParses as
        [wrapperBefore [wrapperAfter [_ wrapperParsed]]].
      assert (0 < length (encode_act_varint (length (first :: rest))) +
        length (first :: rest)) as lineEncodedPositive.
      { pose proof (encode_act_varint_nonempty (length (first :: rest)))
          as varintNonempty.
        destruct (encode_act_varint (length (first :: rest)));
          [contradiction|cbn; lia]. }
      pose proof (line_object_precedes_node_true rootId
        {| plo_id := physicalLineId;
           plo_encoded_len :=
             length (encode_act_varint (length (first :: rest))) +
             length (first :: rest);
           plo_bytes := first :: rest |}
        ltac:(cbn; lia) lineEncodedPositive lineBeforeRoot) as linePrecedes.
      assert (parsed_node_fits_arena
        (ACT_HEADER_LEN + length (physical_writer_bytes objects))
        (pending_node_parsed_model wrapperId wrapperNode) = true)
        as wrapperParsedFits.
      { apply parsed_node_fits_arena_true.
        - rewrite pending_node_parsed_model_encoded_len,
            <- wrapperNodeLengthExact. exact wrapperPositive.
        - rewrite pending_node_parsed_model_id,
            pending_node_parsed_model_encoded_len,
            <- wrapperNodeLengthExact. lia. }
      assert (wrapperId +
        pn_encoded_len (pending_node_parsed_model wrapperId wrapperNode) <=?
        rootId = true) as wrapperBeforeRootBool.
      { apply Nat.leb_le. rewrite pending_node_parsed_model_encoded_len,
          <- wrapperNodeLengthExact. exact wrapperBeforeRoot. }
      assert (wrapperId <? rootId = true) as wrapperStrictlyBeforeRoot
        by (apply Nat.ltb_lt; lia).
      assert (forallb (fun childId => childId <? wrapperId)
        (source_semantic_forest_ids
          (SourceSemanticCons label child certifiedRest)) = true)
        as childrenBackwardBool.
      { apply forallb_nat_ltb_true. exact childrenBackward. }
      assert (negb (Nat.eqb
        (length (source_semantic_forest_ids
          (SourceSemanticCons label child certifiedRest))) 0) = true)
        as childrenNonemptyBool.
      { cbn [source_semantic_forest_ids]. reflexivity. }
      pose proof (branch_references_source_semantic_children globalRootId objects
        wrapperId value (SourceSemanticCons label child certifiedRest)
        layout childrenRecorded childrenLayout childrenDomains)
        as childReferences.
      unfold source_semantic_wrapper_node in childReferences.
      cbn [pending_node_parsed_model] in childReferences.
      cbn [pending_node_parsed_model] in rootParsedFits |- *.
      cbn [pn_edge pn_value pn_encoded_len].
      rewrite lineParsed, linePrecedes, wrapperStrictlyBeforeRoot,
        wrapperParsed, wrapperParsedFits, wrapperBeforeRootBool.
      unfold wrapperNode, source_semantic_wrapper_node.
      cbn [pending_node_parsed_model].
      rewrite childReferences.
      rewrite source_semantic_forest_combine_labels.
      rewrite childrenNonemptyBool, childrenBackwardBool.
      rewrite no_value_free_unary_semantic_true by exact noUnary.
      cbn [option_nat_is_none pn_edge pn_value pn_encoded_len andb].
      rewrite rootLengthExact, wrapperNodeLengthExact.
      reflexivity.
Qed.

Theorem semantic_certificate_root_parses_directly :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_direct_domain
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) certificate ->
    direct_semantic_node_at
      (source_semantic_root_id certificate)
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes =
      Some (source_semantic_erase certificate).
Proof.
  intros lineHash tree bytes certificate objects encoded directDomain.
  pose proof (semantic_certificate_has_exact_image_and_layout
    lineHash tree bytes certificate objects encoded) as [bytesExact layout].
  eapply recorded_source_semantic_node_parses_directly.
  - exact bytesExact.
  - exact layout.
  - eapply semantic_certificate_has_exact_shape; eassumption.
  - eapply semantic_certificate_node_objects_are_recorded; eassumption.
  - eapply semantic_certificate_referenced_lines_are_recorded; eassumption.
  - exact directDomain.
Qed.

(** A whole certificate is parser-ready only when every semantic node, not
    merely the outer root, satisfies the producer-derived direct domain.  The
    corresponding parser predicate deliberately retains the recursive trie
    shape; it does not flatten the certificate into an entry list. *)
Fixpoint source_semantic_tree_direct_domains (arena_end : nat)
    (tree : source_semantic_tree) : Prop :=
  source_semantic_direct_domain arena_end tree /\
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      source_semantic_forest_direct_domains arena_end children
  end
with source_semantic_forest_direct_domains (arena_end : nat)
    (forest : source_semantic_forest) : Prop :=
  match forest with
  | SourceSemanticNil => True
  | SourceSemanticCons _ child rest =>
      source_semantic_tree_direct_domains arena_end child /\
      source_semantic_forest_direct_domains arena_end rest
  end.

Theorem source_semantic_all_direct_domains_mono :
  (forall tree smaller larger,
      smaller <= larger ->
      source_semantic_tree_direct_domains smaller tree ->
      source_semantic_tree_direct_domains larger tree) /\
  (forall forest smaller larger,
      smaller <= larger ->
      source_semantic_forest_direct_domains smaller forest ->
      source_semantic_forest_direct_domains larger forest).
Proof.
  apply source_semantic_tree_forest_ind.
  - intros rootId rootLen rootNode line value children childrenIH lineId
      lineHash wrapper smaller larger grows domains.
    cbn [source_semantic_tree_direct_domains] in domains |- *.
    destruct domains as [rootDomain childrenDomains]. split.
    + eapply source_semantic_direct_domain_mono; eassumption.
    + eapply childrenIH; eassumption.
  - intros smaller larger grows domains. exact I.
  - intros label child childIH rest restIH smaller larger grows domains.
    cbn [source_semantic_forest_direct_domains] in domains |- *.
    destruct domains as [childDomains restDomains]. split.
    + eapply childIH; eassumption.
    + eapply restIH; eassumption.
Qed.

(** A pending semantic root has no physical root ID yet.  Readiness is
    therefore quantified over every later root position at or beyond the
    writer position at which the callback returned it.  This is the exact
    delayed-root discipline of PathMap's catamorphism. *)
Definition pending_source_semantic_direct_ready (position : nat)
    (pending : pending_source_semantic_node) : Prop :=
  ACT_HEADER_LEN <= position /\
  source_semantic_forest_direct_domains position (pss_children pending) /\
  forall root_id,
    position <= root_id ->
    root_id + length (encode_pending_node root_id (pss_root pending)) <
      Nat.pow 256 8 ->
    source_semantic_direct_domain
      (root_id + length (encode_pending_node root_id (pss_root pending)))
      (finalize_pending_source_semantic_node root_id pending).

Lemma pending_source_semantic_direct_ready_mono :
  forall smaller larger pending,
    smaller <= larger ->
    pending_source_semantic_direct_ready smaller pending ->
    pending_source_semantic_direct_ready larger pending.
Proof.
  intros smaller larger pending grows
    [afterHeader [childrenDomains rootReady]].
  unfold pending_source_semantic_direct_ready. split; [lia|]. split.
  - eapply (proj2 source_semantic_all_direct_domains_mono); eassumption.
  - intros rootId rootAfterLarger rootBounded.
    apply rootReady; [lia|exact rootBounded].
Qed.

Lemma pending_source_semantic_direct_ready_finalizes :
  forall position root_id pending,
    pending_source_semantic_direct_ready position pending ->
    position <= root_id ->
    root_id + length (encode_pending_node root_id (pss_root pending)) <
      Nat.pow 256 8 ->
    source_semantic_tree_direct_domains
      (root_id + length (encode_pending_node root_id (pss_root pending)))
      (finalize_pending_source_semantic_node root_id pending).
Proof.
  intros position rootId pending
    [afterHeader [childrenDomains rootReady]] rootAfterPosition rootBounded.
  cbn [source_semantic_tree_direct_domains]. split.
  - apply rootReady; assumption.
  - apply (proj2 source_semantic_all_direct_domains_mono
      (pss_children pending) position
      (rootId + length (encode_pending_node rootId (pss_root pending)))).
    + lia.
    + exact childrenDomains.
Qed.

Lemma pending_source_semantic_direct_ready_forall_mono :
  forall smaller larger pending,
    smaller <= larger ->
    Forall (pending_source_semantic_direct_ready smaller) pending ->
    Forall (pending_source_semantic_direct_ready larger) pending.
Proof.
  intros smaller larger pending grows ready.
  induction ready as [|child rest childReady restReady IH]; constructor.
  - eapply pending_source_semantic_direct_ready_mono; eassumption.
  - exact IH.
Qed.

Lemma writer_position_after_header : forall state,
  ACT_HEADER_LEN <= writer_position state.
Proof. intro state. unfold writer_position. lia. Qed.

Lemma source_semantic_root_layout_implies_children_layout :
  forall position forest,
    source_semantic_forest_root_layout position forest ->
    source_semantic_children_root_layout forest.
Proof.
  intros position [|label child rest] layout; [exact I|].
  cbn [source_semantic_forest_root_layout] in layout.
  destruct layout as [childAt restLayout].
  cbn [source_semantic_children_root_layout
    source_semantic_forest_root_layout].
  split; [reflexivity|]. now rewrite <- childAt in restLayout.
Qed.

Theorem certified_push_pending_children_preserves_direct_readiness :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    Forall
      (pending_source_semantic_direct_ready
        (writer_position (tws_state state))) pending ->
    writer_position (tws_state final) < Nat.pow 256 8 ->
    source_semantic_forest_direct_domains
      (writer_position (tws_state final)) certified /\
    source_semantic_forest_root_domains certified /\
    Forall (fun child_id =>
      child_id < writer_position (tws_state final))
      (source_semantic_forest_ids certified).
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final certifiedRun pendingReady finalBound;
    destruct pending as [|child pending].
  - inversion certifiedRun; subst certified final.
    cbn [source_semantic_forest_direct_domains
      source_semantic_forest_root_domains source_semantic_forest_ids].
    repeat split; constructor.
  - discriminate certifiedRun.
  - discriminate certifiedRun.
  - cbn [certified_push_pending_children] in certifiedRun.
    destruct (traced_push_pending_node (pss_root child) state)
      as [rootId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst certified actualFinal.
    inversion pendingReady as
      [|actualChild actualPending childReady restReady];
      subst actualChild actualPending.
    pose proof (traced_push_pending_node_erases
      (pss_root child) state rootId afterChild childRun) as childErases.
    pose proof (push_pending_node_position_exact
      (pss_root child) (tws_state state) rootId (tws_state afterChild)
      childErases) as [rootExact afterChildExact].
    pose proof (certified_push_pending_children_root_layout_exact
      labels pending afterChild certifiedRest final restRun)
      as [restLayout finalExact].
    assert (writer_position (tws_state afterChild) <=
      writer_position (tws_state final)) as afterChildBeforeFinal.
    { rewrite finalExact. lia. }
    assert (writer_position (tws_state afterChild) < Nat.pow 256 8)
      as afterChildBound by lia.
    assert (writer_position (tws_state state) <=
      writer_position (tws_state afterChild)) as stateBeforeChild.
    { rewrite afterChildExact. lia. }
    assert (source_semantic_tree_direct_domains
      (writer_position (tws_state afterChild))
      (finalize_pending_source_semantic_node rootId child)) as childDomains.
    { subst rootId.
      rewrite afterChildExact.
      apply (pending_source_semantic_direct_ready_finalizes
        (writer_position (tws_state state))
        (writer_position (tws_state state)) child childReady);
        [lia|rewrite <- afterChildExact; exact afterChildBound]. }
    pose proof (pending_source_semantic_direct_ready_forall_mono
      (writer_position (tws_state state))
      (writer_position (tws_state afterChild)) pending stateBeforeChild
      restReady) as restReadyAfterChild.
    destruct (IH pending afterChild certifiedRest final restRun
      restReadyAfterChild finalBound) as
      [restDomains [restRootDomains restIdsBeforeFinal]].
    assert (source_semantic_tree_direct_domains
      (writer_position (tws_state final))
      (finalize_pending_source_semantic_node rootId child))
      as childDomainsAtFinal.
    { eapply (proj1 source_semantic_all_direct_domains_mono);
        eassumption. }
    cbn [source_semantic_forest_direct_domains
      source_semantic_forest_root_domains source_semantic_forest_ids].
    split; [now split|]. split.
    + split; [reflexivity|]. split.
      * destruct childDomains as [childDirect childDescendants].
        destruct child as
          [line value children lineId lineHash wrapper rootNode].
        cbn [finalize_pending_source_semantic_node
          source_semantic_direct_domain] in childDirect.
        tauto.
      * exact restRootDomains.
    + constructor; [|exact restIdsBeforeFinal].
      pose proof (every_pending_node_encoding_is_nonempty
        (writer_position (tws_state state)) (pss_root child))
        as rootEncodingNonempty.
      assert (0 < length (encode_pending_node
        (writer_position (tws_state state)) (pss_root child))).
      { destruct (encode_pending_node
          (writer_position (tws_state state)) (pss_root child));
          [contradiction|cbn; lia]. }
      cbn [source_semantic_root_id finalize_pending_source_semantic_node].
      eapply Nat.lt_le_trans with
        (m := writer_position (tws_state afterChild)).
      * rewrite afterChildExact. lia.
      * exact afterChildBeforeFinal.
Qed.

Definition canonical_writer_forest_shape (forest : writer_forest) : Prop :=
  writer_forest_well_formed forest /\
  writer_forest_values_below_u64 forest /\
  writer_forest_has_no_value_free_unary forest.

Lemma writer_forest_well_formed_labels_bounded : forall forest,
  writer_forest_well_formed forest ->
  Forall (fun label => label < 256) (forest_labels forest).
Proof.
  induction forest as [|label child rest restIH]; intro wellFormed.
  - constructor.
  - cbn [writer_forest_well_formed] in wellFormed.
    destruct wellFormed as [labelBound [childWellFormed restWellFormed]].
    cbn [forest_labels]. constructor; [exact labelBound|].
    apply restIH. exact restWellFormed.
Qed.

Lemma source_semantic_branch_writer_domain :
  forall origin value children,
    Forall (fun label => label < 256)
      (source_semantic_forest_labels children) ->
    StronglySorted Nat.lt (source_semantic_forest_labels children) ->
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    Forall (fun child_id => child_id < origin)
      (source_semantic_forest_ids children) ->
    origin < Nat.pow 256 8 ->
    pending_node_writer_domain origin
      (source_semantic_wrapper_node value children).
Proof.
  intros origin value children labelsBound labelsSorted valueBound
    childrenBackward originBound.
  unfold source_semantic_wrapper_node, pending_node_writer_domain.
  rewrite source_semantic_forest_combine_labels.
  split; [exact labelsBound|]. split; [exact labelsSorted|].
  split; [exact valueBound|].
  intros firstId firstPresent.
  rewrite source_semantic_forest_first_child_id in firstPresent.
  destruct (source_semantic_forest_ids children) as [|childId rest]
    eqn:childIds; [discriminate firstPresent|].
  inversion firstPresent; subst firstId.
  inversion childrenBackward as [|actual restIds childBefore restBefore];
    subst actual restIds.
  split; lia.
Qed.

Lemma source_semantic_line_writer_domain :
  forall origin value child line_id,
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    (forall child_id, child = Some child_id -> child_id < origin) ->
    line_id < origin ->
    origin < Nat.pow 256 8 ->
    pending_node_writer_domain origin (PendingLine value child line_id).
Proof.
  intros origin value child lineId valueBound childBefore lineBefore
    originBound.
  cbn [pending_node_writer_domain]. split; [exact valueBound|]. split.
  - intros childId childExact.
    pose proof (childBefore childId childExact). split; lia.
  - split; lia.
Qed.

Lemma traced_writer_objects_prefix_position_mono :
  forall before after suffix,
    traced_writer_consistent before ->
    traced_writer_consistent after ->
    tws_objects after = tws_objects before ++ suffix ->
    writer_position (tws_state before) <= writer_position (tws_state after).
Proof.
  intros before after suffix beforeConsistent afterConsistent objectsExact.
  unfold writer_position, traced_writer_consistent in *.
  rewrite beforeConsistent, afterConsistent, objectsExact,
    physical_writer_bytes_app, length_app. lia.
Qed.

Theorem certified_source_writer_position_mono : forall line_hash,
  (forall tree state pending final,
      traced_writer_full_invariant state ->
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      writer_position (tws_state state) <=
        writer_position (tws_state final)) /\
  (forall forest state pending final,
      traced_writer_full_invariant state ->
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      writer_position (tws_state state) <=
        writer_position (tws_state final)).
Proof.
  intro lineHash. split.
  - intros tree state pending final stateInvariant built.
    destruct (proj1 (certified_source_writer_objects_prefix lineHash)
      tree state pending final built) as [suffix objectsExact].
    pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
      tree state pending final built stateInvariant) as finalInvariant.
    eapply traced_writer_objects_prefix_position_mono.
    + exact (proj1 (proj1 stateInvariant)).
    + exact (proj1 (proj1 finalInvariant)).
    + exact objectsExact.
  - intros forest state pending final stateInvariant built.
    destruct (proj2 (certified_source_writer_objects_prefix lineHash)
      forest state pending final built) as [suffix objectsExact].
    pose proof (proj2 (certified_source_writer_preserves_full_invariant lineHash)
      forest state pending final built stateInvariant) as finalInvariant.
    eapply traced_writer_objects_prefix_position_mono.
    + exact (proj1 (proj1 stateInvariant)).
    + exact (proj1 (proj1 finalInvariant)).
    + exact objectsExact.
Qed.

Lemma Forall_nat_lt_mono : forall values smaller larger,
  Forall (fun value => value < smaller) values ->
  smaller <= larger ->
  Forall (fun value => value < larger) values.
Proof.
  intros values smaller larger bounded grows.
  induction bounded as [|value rest valueBound restBound IH]; constructor;
    [lia|exact IH].
Qed.

Lemma encoded_line_object_length_positive : forall line : list bbyte,
  0 < length (encode_act_varint (length line)) + length line.
Proof.
  intro line.
  pose proof (encode_act_varint_nonempty (length line)) as nonempty.
  destruct (encode_act_varint (length line)); [contradiction|cbn; lia].
Qed.

Lemma line_length_le_encoded_line_object_length : forall line : list bbyte,
  length line <= length (encode_act_varint (length line) ++ line).
Proof. intro line. rewrite length_app. lia. Qed.

Lemma source_semantic_forest_labels_ids_length : forall forest,
  length (source_semantic_forest_labels forest) =
  length (source_semantic_forest_ids forest).
Proof.
  induction forest as [|label child rest IH]; cbn; lia.
Qed.

Lemma traced_add_or_reuse_line_position_mono :
  forall line_hash line state id after,
    traced_writer_full_invariant state ->
    traced_add_or_reuse_line line_hash line state = (id, after) ->
    writer_position (tws_state state) <= writer_position (tws_state after).
Proof.
  intros lineHash line state id after stateInvariant added.
  destruct (traced_add_or_reuse_line_objects_prefix lineHash line state)
    as [suffix objectsExact].
  rewrite added in objectsExact. cbn in objectsExact.
  pose proof (traced_add_or_reuse_line_preserves_full_invariant
    lineHash line state stateInvariant) as afterInvariant.
  rewrite added in afterInvariant. cbn in afterInvariant.
  eapply traced_writer_objects_prefix_position_mono.
  - exact (proj1 (proj1 stateInvariant)).
  - exact (proj1 (proj1 afterInvariant)).
  - exact objectsExact.
Qed.

Lemma pending_branch_source_semantic_direct_ready :
  forall position value children,
    ACT_HEADER_LEN <= position ->
    position < Nat.pow 256 8 ->
    source_semantic_forest_direct_domains position children ->
    source_semantic_forest_root_domains children ->
    source_semantic_children_root_layout children ->
    Forall (fun child_id => child_id < position)
      (source_semantic_forest_ids children) ->
    Forall (fun label => label < 256)
      (source_semantic_forest_labels children) ->
    StronglySorted Nat.lt (source_semantic_forest_labels children) ->
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    ~ (value = None /\ length (source_semantic_forest_ids children) = 1) ->
    pending_source_semantic_direct_ready position
      {| pss_line := [];
         pss_value := value;
         pss_children := children;
         pss_line_id := None;
         pss_line_hash := None;
         pss_wrapper := None;
         pss_root := source_semantic_wrapper_node value children |}.
Proof.
  intros position value children afterHeader positionBound childrenDomains
    rootDomains childrenLayout childrenBefore labelsBound labelsSorted
    valueBound noUnary.
  unfold pending_source_semantic_direct_ready. cbn [pss_children].
  split; [exact afterHeader|]. split; [exact childrenDomains|].
  intros rootId rootAfterPosition rootBounded.
  cbn [finalize_pending_source_semantic_node source_semantic_direct_domain
    pss_root pss_line pss_value pss_line_id pss_line_hash pss_wrapper].
  assert (rootId < Nat.pow 256 8) as rootIdBound by lia.
  assert (Forall (fun childId => childId < rootId)
    (source_semantic_forest_ids children)) as childrenBeforeRoot.
  { eapply Forall_nat_lt_mono; eassumption. }
  assert (pending_node_writer_domain rootId
    (source_semantic_wrapper_node value children)) as rootDomain.
  { apply source_semantic_branch_writer_domain; assumption. }
  assert (0 < length (encode_pending_node rootId
    (source_semantic_wrapper_node value children))) as rootPositive.
  { pose proof (every_pending_node_encoding_is_nonempty rootId
      (source_semantic_wrapper_node value children)) as nonempty.
    destruct (encode_pending_node rootId
      (source_semantic_wrapper_node value children));
      [contradiction|cbn; lia]. }
  split; [lia|].
  split; [exact rootPositive|].
  split; [lia|].
  split; [exact rootDomain|].
  split; [exact rootDomains|].
  split; [exact childrenLayout|].
  split; [exact childrenBeforeRoot|].
  split; [exact noUnary|].
  split; exact I.
Qed.

Lemma pending_line_leaf_source_semantic_direct_ready :
  forall position line value line_id line_hash,
    ACT_HEADER_LEN <= position ->
    position < Nat.pow 256 8 ->
    line <> [] ->
    ACT_HEADER_LEN <= line_id ->
    length line < Nat.pow 256 8 ->
    line_id + (length (encode_act_varint (length line)) + length line) <=
      position ->
    (forall scalar, value = Some scalar -> scalar < Nat.pow 256 8) ->
    pending_source_semantic_direct_ready position
      {| pss_line := line;
         pss_value := value;
         pss_children := SourceSemanticNil;
         pss_line_id := Some line_id;
         pss_line_hash := Some line_hash;
         pss_wrapper := None;
         pss_root := PendingLine value None line_id |}.
Proof.
  intros position line value lineId lineHash afterHeader positionBound
    lineNonempty lineAfterHeader lineBound lineEndsBefore valueBound.
  unfold pending_source_semantic_direct_ready. cbn [pss_children].
  split; [exact afterHeader|]. split; [exact I|].
  intros rootId rootAfterPosition rootBounded.
  cbn [finalize_pending_source_semantic_node source_semantic_direct_domain
    pss_root pss_line pss_value pss_line_id pss_line_hash pss_wrapper
    source_semantic_forest_ids source_semantic_forest_root_domains
    source_semantic_children_root_layout].
  assert (rootId < Nat.pow 256 8) as rootIdBound by lia.
  pose proof (encoded_line_object_length_positive line) as linePositive.
  assert (lineId < rootId) as lineBeforeRoot by lia.
  assert (pending_node_writer_domain rootId
    (PendingLine value None lineId)) as rootDomain.
  { apply source_semantic_line_writer_domain; try assumption.
    intros childId impossible. discriminate impossible. }
  assert (0 < length (encode_pending_node rootId
    (PendingLine value None lineId))) as rootPositive.
  { pose proof (every_pending_node_encoding_is_nonempty rootId
      (PendingLine value None lineId)) as nonempty.
    destruct (encode_pending_node rootId (PendingLine value None lineId));
      [contradiction|cbn; lia]. }
  assert (0 < length line) as lineLengthPositive.
  { destruct line; [contradiction|cbn; lia]. }
  split; [lia|].
  split; [exact rootPositive|].
  split; [lia|].
  split; [exact rootDomain|].
  split; [exact I|].
  split; [exact I|].
  split; [constructor|].
  split; [intros [_ impossible]; discriminate impossible|].
  split.
  - repeat split; try assumption; lia.
  - exact I.
Qed.

Lemma pending_line_wrapper_source_semantic_direct_ready :
  forall position line value children line_id line_hash wrapper_id wrapper_len,
    ACT_HEADER_LEN <= position ->
    position < Nat.pow 256 8 ->
    line <> [] ->
    source_semantic_forest_direct_domains position children ->
    source_semantic_forest_root_domains children ->
    source_semantic_children_root_layout children ->
    Forall (fun child_id => child_id < wrapper_id)
      (source_semantic_forest_ids children) ->
    source_semantic_forest_ids children <> [] ->
    ~ (value = None /\ length (source_semantic_forest_ids children) = 1) ->
    ACT_HEADER_LEN <= line_id ->
    length line < Nat.pow 256 8 ->
    line_id + (length (encode_act_varint (length line)) + length line) <=
      wrapper_id ->
    ACT_HEADER_LEN <= wrapper_id ->
    0 < wrapper_len ->
    wrapper_len = length (encode_pending_node wrapper_id
      (source_semantic_wrapper_node value children)) ->
    wrapper_id + wrapper_len <= position ->
    pending_node_writer_domain wrapper_id
      (source_semantic_wrapper_node value children) ->
    pending_source_semantic_direct_ready position
      {| pss_line := line;
         pss_value := value;
         pss_children := children;
         pss_line_id := Some line_id;
         pss_line_hash := Some line_hash;
         pss_wrapper := Some (wrapper_id, wrapper_len);
         pss_root := PendingLine None (Some wrapper_id) line_id |}.
Proof.
  intros position line value children lineId lineHash wrapperId wrapperLen
    afterHeader positionBound lineNonempty childrenDomains rootDomains
    childrenLayout childrenBeforeWrapper childrenNonempty noUnary
    lineAfterHeader lineBound lineEndsBeforeWrapper wrapperAfterHeader
    wrapperPositive wrapperLengthExact wrapperEndsBefore wrapperDomain.
  unfold pending_source_semantic_direct_ready. cbn [pss_children].
  split; [exact afterHeader|]. split; [exact childrenDomains|].
  intros rootId rootAfterPosition rootBounded.
  cbn [finalize_pending_source_semantic_node source_semantic_direct_domain
    pss_root pss_line pss_value pss_line_id pss_line_hash pss_wrapper].
  assert (rootId < Nat.pow 256 8) as rootIdBound by lia.
  pose proof (encoded_line_object_length_positive line) as linePositive.
  assert (lineId < rootId) as lineBeforeRoot by lia.
  assert (wrapperId < rootId) as wrapperBeforeRoot by lia.
  assert (pending_node_writer_domain rootId
    (PendingLine None (Some wrapperId) lineId)) as rootDomain.
  { apply source_semantic_line_writer_domain; try assumption.
    - intros scalar impossible. discriminate impossible.
    - intros childId childExact. inversion childExact; subst childId.
      exact wrapperBeforeRoot. }
  assert (0 < length (encode_pending_node rootId
    (PendingLine None (Some wrapperId) lineId))) as rootPositive.
  { pose proof (every_pending_node_encoding_is_nonempty rootId
      (PendingLine None (Some wrapperId) lineId)) as nonempty.
    destruct (encode_pending_node rootId
      (PendingLine None (Some wrapperId) lineId));
      [contradiction|cbn; lia]. }
  assert (0 < length line) as lineLengthPositive.
  { destruct line; [contradiction|cbn; lia]. }
  split; [lia|].
  split; [exact rootPositive|].
  split; [lia|].
  split; [exact rootDomain|].
  split; [exact rootDomains|].
  split; [exact childrenLayout|].
  split; [exact childrenBeforeWrapper|].
  split; [exact noUnary|].
  split.
  - repeat split; try assumption; lia.
  - split; [exact wrapperAfterHeader|].
    split; [exact wrapperPositive|].
    split; [lia|].
    split; assumption.
Qed.

Theorem certified_source_writer_preserves_direct_readiness : forall line_hash,
  (forall tree state pending final,
      canonical_writer_shape tree ->
      traced_writer_full_invariant state ->
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      writer_position (tws_state final) < Nat.pow 256 8 ->
      pending_source_semantic_direct_ready
        (writer_position (tws_state final)) pending) /\
  (forall forest state pending final,
      canonical_writer_forest_shape forest ->
      traced_writer_full_invariant state ->
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      writer_position (tws_state final) < Nat.pow 256 8 ->
      Forall
        (pending_source_semantic_direct_ready
          (writer_position (tws_state final))) pending).
Proof.
  intro lineHash.
  apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final canonical
      stateInvariant certifiedRun finalBound.
    unfold canonical_writer_shape in canonical.
    destruct canonical as [wellFormed [valuesBounded noUnary]].
    cbn [writer_well_formed] in wellFormed.
    destruct wellFormed as [lineBytes [labelsSorted childrenWellFormed]].
    cbn [writer_values_below_u64] in valuesBounded.
    destruct valuesBounded as [valueBound childrenValuesBounded].
    cbn [writer_has_no_value_free_unary] in noUnary.
    destruct noUnary as [localNoUnary childrenNoUnary].
    cbn [certified_build_pending_tree] in certifiedRun.
    destruct (certified_build_pending_forest lineHash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate certifiedRun].
    pose proof (proj2 (certified_source_writer_preserves_full_invariant lineHash)
      children state pendingChildren afterDescendants childrenRun
      stateInvariant) as descendantsInvariant.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:pushRun;
      [|discriminate certifiedRun].
    pose proof (certified_push_pending_children_erases
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [tracedPush [childrenReify certifiedLabels]].
    pose proof (traced_push_pending_children_preserves_full_invariant
      (forest_labels children) (map pss_root pendingChildren)
      afterDescendants
      (combine (source_semantic_forest_labels certifiedChildren)
        (source_semantic_forest_ids certifiedChildren))
      afterChildren descendantsInvariant tracedPush) as childrenInvariant.
    pose proof (certified_push_pending_children_root_layout_exact
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren pushRun) as
      [childrenRootLayout afterChildrenExact].
    assert (writer_position (tws_state afterDescendants) <=
      writer_position (tws_state afterChildren)) as descendantsBeforeChildren.
    { rewrite afterChildrenExact. lia. }
    assert (Forall (fun label => label < 256)
      (source_semantic_forest_labels certifiedChildren))
      as certifiedLabelsBounded.
    { rewrite certifiedLabels.
      apply writer_forest_well_formed_labels_bounded.
      exact childrenWellFormed. }
    assert (StronglySorted Nat.lt
      (source_semantic_forest_labels certifiedChildren))
      as certifiedLabelsSorted by (now rewrite certifiedLabels).
    assert (~ (value = None /\
      length (source_semantic_forest_ids certifiedChildren) = 1))
      as certifiedNoUnary.
    { intros [valueEmpty idsUnary]. apply localNoUnary. split;
        [exact valueEmpty|].
      rewrite <- source_semantic_forest_labels_ids_length in idsUnary.
      now rewrite certifiedLabels in idsUnary. }
    destruct line as [|first rest].
    + inversion certifiedRun; subst pending final.
      assert (writer_position (tws_state afterDescendants) < Nat.pow 256 8)
        as descendantsBound by lia.
      pose proof (childrenIH state pendingChildren afterDescendants
        ltac:(repeat split; assumption) stateInvariant
        childrenRun descendantsBound) as pendingChildrenReady.
      destruct (certified_push_pending_children_preserves_direct_readiness
        (forest_labels children) pendingChildren afterDescendants
        certifiedChildren afterChildren pushRun pendingChildrenReady finalBound)
        as [childrenDomains [childrenRootDomains childrenBefore]].
      apply pending_branch_source_semantic_direct_ready; try assumption.
      * apply writer_position_after_header.
      * apply source_semantic_root_layout_implies_children_layout
          with (position := writer_position (tws_state afterDescendants)).
        exact childrenRootLayout.
    + destruct (traced_add_or_reuse_line lineHash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_preserves_full_invariant
        lineHash (first :: rest) afterChildren childrenInvariant)
        as lineInvariant.
      rewrite lineRun in lineInvariant. cbn in lineInvariant.
      pose proof (traced_add_or_reuse_line_position_mono
        lineHash (first :: rest) afterChildren lineId afterLine
        childrenInvariant lineRun) as childrenBeforeLine.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion certifiedRun; subst pending final.
        assert (writer_position (tws_state afterDescendants) < Nat.pow 256 8)
          as descendantsBound by lia.
        pose proof (childrenIH state pendingChildren afterDescendants
          ltac:(repeat split; assumption)
          stateInvariant childrenRun descendantsBound)
          as pendingChildrenReady.
        destruct (certified_push_pending_children_preserves_direct_readiness
          (forest_labels children) pendingChildren afterDescendants
          SourceSemanticNil afterChildren pushRun pendingChildrenReady)
          as [childrenDomains [childrenRootDomains childrenBefore]];
          [lia|].
        pose proof (traced_add_or_reuse_line_references_recorded_object
          lineHash (first :: rest) afterChildren (proj2 childrenInvariant))
          as linePresent.
        rewrite lineRun in linePresent. cbn in linePresent.
        pose proof (traced_writer_recorded_object_bounds afterLine
          (WriterLineObject lineId (lineHash (first :: rest)) (first :: rest))
          (proj1 lineInvariant) linePresent) as
          [lineAfterHeader lineEndsBefore].
        cbn [physical_writer_object_start physical_writer_object_bytes]
          in lineAfterHeader, lineEndsBefore.
        assert (length (first :: rest) <=
          writer_position (tws_state afterLine)) as lineLengthFits.
        { pose proof (line_length_le_encoded_line_object_length
            (first :: rest)). lia. }
        assert (length (first :: rest) < Nat.pow 256 8) as lineBound.
        { lia. }
        apply pending_line_leaf_source_semantic_direct_ready.
        -- apply writer_position_after_header.
        -- exact finalBound.
        -- discriminate.
        -- exact lineAfterHeader.
        -- exact lineBound.
        -- rewrite <- length_app. exact lineEndsBefore.
        -- exact valueBound.
      * set (wrapperNode := source_semantic_wrapper_node value
          (SourceSemanticCons label child certifiedRest)).
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine]
          in certifiedRun.
        fold wrapperNode in certifiedRun.
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        inversion certifiedRun; subst pending final.
        assert (afterWrapper =
          snd (traced_push_pending_node wrapperNode afterLine))
          as afterWrapperExact by (rewrite wrapperRun; reflexivity).
        unfold traced_push_pending_node, push_pending_node
          in afterWrapperExact.
        pose proof (traced_push_pending_node_erases wrapperNode afterLine
          wrapperId afterWrapper wrapperRun) as wrapperErases.
        pose proof (push_pending_node_position_exact wrapperNode
          (tws_state afterLine) wrapperId (tws_state afterWrapper)
          wrapperErases) as [wrapperAtLine wrapperEndExact].
        assert (writer_position (tws_state afterLine) <=
          writer_position (tws_state afterWrapper)) as lineBeforeWrapper.
        { rewrite wrapperEndExact. lia. }
        assert (writer_position (tws_state afterWrapper) < Nat.pow 256 8)
          as wrapperBound.
        { rewrite afterWrapperExact.
          unfold wrapperNode, source_semantic_wrapper_node.
          cbn [source_semantic_forest_labels source_semantic_forest_ids combine].
          exact finalBound. }
        assert (writer_position (tws_state afterDescendants) < Nat.pow 256 8)
          as descendantsBound.
        { eapply Nat.le_lt_trans; [|exact wrapperBound]. lia. }
        pose proof (childrenIH state pendingChildren afterDescendants
          ltac:(repeat split; assumption)
          stateInvariant childrenRun descendantsBound)
          as pendingChildrenReady.
        destruct (certified_push_pending_children_preserves_direct_readiness
          (forest_labels children) pendingChildren afterDescendants
          (SourceSemanticCons label child certifiedRest) afterChildren
          pushRun pendingChildrenReady) as
          [childrenDomainsAtChildren
            [childrenRootDomains childrenBeforeChildren]];
          [lia|].
        assert (source_semantic_forest_direct_domains
          (writer_position (tws_state afterWrapper))
          (SourceSemanticCons label child certifiedRest))
          as childrenDomains.
        { apply (proj2 source_semantic_all_direct_domains_mono
            (SourceSemanticCons label child certifiedRest)
            (writer_position (tws_state afterChildren))
            (writer_position (tws_state afterWrapper))).
          - lia.
          - exact childrenDomainsAtChildren. }
        assert (source_semantic_children_root_layout
          (SourceSemanticCons label child certifiedRest)) as childrenLayout.
        { apply source_semantic_root_layout_implies_children_layout
            with (position := writer_position (tws_state afterDescendants)).
          exact childrenRootLayout. }
        assert (Forall (fun childId => childId < wrapperId)
          (source_semantic_forest_ids
            (SourceSemanticCons label child certifiedRest)))
          as childrenBeforeWrapper.
        { eapply Forall_nat_lt_mono; [exact childrenBeforeChildren|].
          rewrite wrapperAtLine. exact childrenBeforeLine. }
        pose proof (traced_add_or_reuse_line_references_recorded_object
          lineHash (first :: rest) afterChildren (proj2 childrenInvariant))
          as linePresent.
        rewrite lineRun in linePresent. cbn in linePresent.
        pose proof (traced_writer_recorded_object_bounds afterLine
          (WriterLineObject lineId (lineHash (first :: rest)) (first :: rest))
          (proj1 lineInvariant) linePresent) as
          [lineAfterHeader lineEndsBefore].
        cbn [physical_writer_object_start physical_writer_object_bytes]
          in lineAfterHeader, lineEndsBefore.
        assert (length (first :: rest) <=
          writer_position (tws_state afterLine)) as lineLengthFits.
        { pose proof (line_length_le_encoded_line_object_length
            (first :: rest)). lia. }
        assert (length (first :: rest) < Nat.pow 256 8) as lineBound.
        { lia. }
        assert (0 < length (encode_pending_node wrapperId wrapperNode))
          as wrapperPositive.
        { pose proof (every_pending_node_encoding_is_nonempty
            wrapperId wrapperNode) as nonempty.
          destruct (encode_pending_node wrapperId wrapperNode);
            [contradiction|cbn; lia]. }
        assert (pending_node_writer_domain wrapperId wrapperNode)
          as wrapperDomain.
        { unfold wrapperNode.
          apply source_semantic_branch_writer_domain; try assumption.
          rewrite wrapperAtLine. lia. }
        assert (pending_source_semantic_direct_ready
          (writer_position (tws_state afterWrapper))
          {| pss_line := first :: rest;
             pss_value := value;
             pss_children := SourceSemanticCons label child certifiedRest;
             pss_line_id := Some lineId;
             pss_line_hash := Some (lineHash (first :: rest));
             pss_wrapper := Some (wrapperId,
               length (encode_pending_node wrapperId wrapperNode));
             pss_root := PendingLine None (Some wrapperId) lineId |})
          as pendingReady.
        { unfold wrapperNode in wrapperEndExact, wrapperPositive,
            wrapperDomain |- *.
          apply pending_line_wrapper_source_semantic_direct_ready.
          - apply writer_position_after_header.
          - exact wrapperBound.
          - discriminate.
          - exact childrenDomains.
          - exact childrenRootDomains.
          - exact childrenLayout.
          - exact childrenBeforeWrapper.
          - cbn [source_semantic_forest_ids]. discriminate.
          - exact certifiedNoUnary.
          - exact lineAfterHeader.
          - exact lineBound.
          - rewrite <- length_app. rewrite wrapperAtLine.
            exact lineEndsBefore.
          - rewrite wrapperAtLine. apply writer_position_after_header.
          - exact wrapperPositive.
          - reflexivity.
          - rewrite wrapperEndExact, wrapperAtLine. lia.
          - exact wrapperDomain. }
        rewrite afterWrapperExact in pendingReady.
        unfold wrapperNode, source_semantic_wrapper_node in pendingReady.
        cbn [source_semantic_forest_labels source_semantic_forest_ids combine
          encode_pending_node labels_of_children first_child_id app]
          in pendingReady.
        rewrite wrapperAtLine in pendingReady.
        exact pendingReady.
  - intros state pending final canonical stateInvariant certifiedRun finalBound.
    inversion certifiedRun; subst pending final. constructor.
  - intros label child childIH rest restIH state pending final canonical
      stateInvariant certifiedRun finalBound.
    unfold canonical_writer_forest_shape in canonical.
    destruct canonical as [wellFormed [valuesBounded noUnary]].
    cbn [writer_forest_well_formed] in wellFormed.
    destruct wellFormed as [labelBound [childWellFormed restWellFormed]].
    cbn [writer_forest_values_below_u64] in valuesBounded.
    destruct valuesBounded as [childValues restValues].
    cbn [writer_forest_has_no_value_free_unary] in noUnary.
    destruct noUnary as [childNoUnary restNoUnary].
    cbn [certified_build_pending_forest] in certifiedRun.
    destruct (certified_build_pending_tree lineHash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate certifiedRun].
    destruct (certified_build_pending_forest lineHash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate certifiedRun].
    inversion certifiedRun; subst pending actualFinal.
    pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
      child state pendingChild afterChild childRun stateInvariant)
      as childInvariant.
    pose proof (proj2 (certified_source_writer_position_mono lineHash)
      rest afterChild pendingRest final childInvariant restRun)
      as childBeforeFinal.
    assert (writer_position (tws_state afterChild) < Nat.pow 256 8)
      as childBound by lia.
    pose proof (childIH state pendingChild afterChild
      ltac:(unfold canonical_writer_shape; repeat split; assumption)
      stateInvariant childRun childBound) as childReady.
    pose proof (restIH afterChild pendingRest final
      ltac:(unfold canonical_writer_forest_shape; repeat split; assumption)
      childInvariant restRun finalBound) as restReady.
    constructor; [|exact restReady].
    eapply pending_source_semantic_direct_ready_mono; eassumption.
Qed.

Fixpoint source_semantic_tree_parses_directly (arena_end : nat)
    (bytes : list bbyte) (tree : source_semantic_tree) : Prop :=
  direct_semantic_node_at (source_semantic_root_id tree) arena_end bytes =
    Some (source_semantic_erase tree) /\
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      source_semantic_forest_parses_directly arena_end bytes children
  end
with source_semantic_forest_parses_directly (arena_end : nat)
    (bytes : list bbyte) (forest : source_semantic_forest) : Prop :=
  match forest with
  | SourceSemanticNil => True
  | SourceSemanticCons _ child rest =>
      source_semantic_tree_parses_directly arena_end bytes child /\
      source_semantic_forest_parses_directly arena_end bytes rest
  end.

Theorem recorded_source_semantic_nodes_parse_directly :
  forall global_root_id bytes objects,
    bytes = ACT_MAGIC ++ le_fixed 8 global_root_id ++
      physical_writer_bytes objects ++ ACT_TRAILER ->
    physical_writer_objects_layout ACT_HEADER_LEN objects ->
    (forall tree,
      source_semantic_tree_shape tree ->
      incl
        (source_semantic_pending_node_objects tree ++
         [source_semantic_root_object tree]) objects ->
      incl (source_semantic_referenced_line_objects tree) objects ->
      source_semantic_tree_direct_domains
        (ACT_HEADER_LEN + length (physical_writer_bytes objects)) tree ->
      source_semantic_tree_parses_directly
        (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes tree) /\
    (forall forest,
      source_semantic_forest_shape forest ->
      incl
        (source_semantic_forest_pending_node_objects forest ++
         source_semantic_forest_root_objects forest) objects ->
      incl (source_semantic_forest_referenced_line_objects forest) objects ->
      source_semantic_forest_direct_domains
        (ACT_HEADER_LEN + length (physical_writer_bytes objects)) forest ->
      source_semantic_forest_parses_directly
        (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes forest).
Proof.
  intros globalRootId bytes objects bytesExact layout.
  apply source_semantic_tree_forest_ind.
  - intros rootId rootLen rootNode line value children childrenIH lineId
      lineHash wrapper shape nodesRecorded linesRecorded allDomains.
    pose proof shape as nodeShape.
    cbn [source_semantic_tree_shape] in shape.
    destruct shape as [childrenShape localShape].
    cbn [source_semantic_tree_direct_domains] in allDomains.
    destruct allDomains as [rootDomain childrenDomains].
    cbn [source_semantic_tree_parses_directly]. split.
    + eapply recorded_source_semantic_node_parses_directly.
      * exact bytesExact.
      * exact layout.
      * exact nodeShape.
      * exact nodesRecorded.
      * exact linesRecorded.
      * exact rootDomain.
    + apply childrenIH.
      * exact childrenShape.
      * intros object present. apply in_app_or in present as [present|present].
        -- apply nodesRecorded. apply in_or_app. left.
           cbn [source_semantic_pending_node_objects].
           apply in_or_app. left. exact present.
        -- apply nodesRecorded. apply in_or_app. left.
           cbn [source_semantic_pending_node_objects].
           apply in_or_app. right. apply in_or_app. left. exact present.
      * intros object present. apply linesRecorded.
        cbn [source_semantic_referenced_line_objects].
        apply in_or_app. left. exact present.
      * exact childrenDomains.
  - intros shape nodesRecorded linesRecorded allDomains. exact I.
  - intros label child childIH rest restIH shape nodesRecorded
      linesRecorded allDomains.
    cbn [source_semantic_forest_shape] in shape.
    destruct shape as [childShape restShape].
    cbn [source_semantic_forest_direct_domains] in allDomains.
    destruct allDomains as [childDomains restDomains].
    cbn [source_semantic_forest_parses_directly]. split.
    + apply childIH.
      * exact childShape.
      * intros object present. apply in_app_or in present as [present|present].
        -- apply nodesRecorded. cbn [source_semantic_forest_pending_node_objects
             source_semantic_forest_root_objects].
           apply in_or_app. left. apply in_or_app. left. exact present.
        -- cbn in present. destruct present as [present|present];
             [subst object|contradiction].
           apply nodesRecorded. cbn [source_semantic_forest_pending_node_objects
             source_semantic_forest_root_objects].
           apply in_or_app. right. now left.
      * intros object present. apply linesRecorded.
        cbn [source_semantic_forest_referenced_line_objects].
        apply in_or_app. left. exact present.
      * exact childDomains.
    + apply restIH.
      * exact restShape.
      * intros object present. apply in_app_or in present as [present|present].
        -- apply nodesRecorded. cbn [source_semantic_forest_pending_node_objects
             source_semantic_forest_root_objects].
           apply in_or_app. left. apply in_or_app. right. exact present.
        -- apply nodesRecorded. cbn [source_semantic_forest_pending_node_objects
             source_semantic_forest_root_objects].
           apply in_or_app. right. now right.
      * intros object present. apply linesRecorded.
        cbn [source_semantic_forest_referenced_line_objects].
        apply in_or_app. right. exact present.
      * exact restDomains.
Qed.

Lemma encode_act_varint_length_le_9 : forall value,
  length (encode_act_varint value) <= 9.
Proof.
  intro value. unfold encode_act_varint.
  destruct (value <=? ACT_VARINT_BIAS); cbn [length].
  - lia.
  - apply le_n_S. apply little_endian_digits_length_bounded.
Qed.

Lemma optional_value_bytes_length_le_9 : forall value,
  length (optional_value_bytes value) <= 9.
Proof.
  intros [scalar|]; cbn [optional_value_bytes].
  - apply encode_act_varint_length_le_9.
  - exact (Nat.le_0_l 9).
Qed.

Lemma relative_bytes_length_le_9 : forall origin target,
  length (relative_bytes origin target) <= 9.
Proof.
  intros origin target. unfold relative_bytes.
  apply encode_act_varint_length_le_9.
Qed.

Lemma encoded_branch_labels_length_le_32 : forall labels,
  length (encoded_branch_labels labels) <= 32.
Proof.
  intro labels. unfold encoded_branch_labels.
  destruct (length labels <? 32) eqn:sparse.
  - apply Nat.ltb_lt in sparse. exact (Nat.lt_le_incl _ _ sparse).
  - rewrite dense_label_mask_length. lia.
Qed.

(** A fixed liability for a root that PathMap's Catamorphism has returned but
    whose parent has not emitted yet.  The cap is representation-derived:
    branch = head + value + first-child delta + dense mask = 1+9+9+32;
    line = head + value + child delta + line delta = 1+9+9+9. *)
Definition pending_node_size_cap (node : pending_node) : nat :=
  match node with
  | PendingBranch _ _ => 51
  | PendingLine _ _ _ => 28
  end.

Fixpoint pending_nodes_size_cap (nodes : list pending_source_semantic_node)
    : nat :=
  match nodes with
  | [] => 0
  | node :: rest =>
      pending_node_size_cap (pss_root node) + pending_nodes_size_cap rest
  end.

Lemma pending_node_encoded_length_le_cap : forall origin node,
  length (encode_pending_node origin node) <= pending_node_size_cap node.
Proof.
  intros origin [value children|value child lineId].
  - rewrite pending_branch_encoded_len_exact.
    unfold pending_branch_encoded_len, pending_node_size_cap.
    pose proof (optional_value_bytes_length_le_9 value) as valueBound.
    pose proof (encoded_branch_labels_length_le_32
      (labels_of_children children)) as labelsBound.
    destruct (first_child_id children) as [first|] eqn:firstChild.
    + pose proof (relative_bytes_length_le_9 origin first) as relativeBound.
      lia.
    + lia.
  - rewrite pending_line_encoded_len_exact.
    unfold pending_line_encoded_len, pending_node_size_cap,
      optional_backward_bytes.
    pose proof (optional_value_bytes_length_le_9 value) as valueBound.
    pose proof (relative_bytes_length_le_9 origin lineId) as lineBound.
    destruct child as [childId|].
    + pose proof (relative_bytes_length_le_9 origin childId) as childBound.
      lia.
    + cbn [length]. lia.
Qed.

Lemma traced_push_pending_node_position_le_cap : forall node state id final,
  traced_push_pending_node node state = (id, final) ->
  writer_position (tws_state final) <=
    writer_position (tws_state state) + pending_node_size_cap node.
Proof.
  intros node state id final pushed.
  pose proof (traced_push_pending_node_erases node state id final pushed)
    as erased.
  pose proof (push_pending_node_position_exact node (tws_state state)
    id (tws_state final) erased) as [_ positionExact].
  pose proof (pending_node_encoded_length_le_cap
    (writer_position (tws_state state)) node) as encodedBound.
  lia.
Qed.

Lemma traced_add_or_reuse_line_position_upper_bound :
  forall line_hash line state id final,
    traced_add_or_reuse_line line_hash line state = (id, final) ->
    writer_position (tws_state final) <=
      writer_position (tws_state state) + 9 + length line.
Proof.
  intros lineHash line state id final added.
  pose proof (traced_add_or_reuse_line_erases
    lineHash line state id final added) as erased.
  pose proof (add_or_reuse_line_arena_effect
    lineHash line (tws_state state) id (tws_state final) erased) as effect.
  destruct effect as [reused|[_ arenaExact]].
  - rewrite reused. lia.
  - unfold writer_position. rewrite arenaExact, !length_app.
    pose proof (encode_act_varint_length_le_9 (length line)) as varintBound.
    lia.
Qed.

Lemma certified_push_pending_children_position_upper_bound :
  forall labels pending state certified final,
    certified_push_pending_children labels pending state =
      Some (certified, final) ->
    writer_position (tws_state final) <=
      writer_position (tws_state state) + pending_nodes_size_cap pending.
Proof.
  induction labels as [|label labels IH];
    intros pending state certified final pushed;
    destruct pending as [|child pending].
  - inversion pushed; subst certified final. cbn. lia.
  - discriminate pushed.
  - discriminate pushed.
  - cbn [certified_push_pending_children] in pushed.
    destruct (traced_push_pending_node (pss_root child) state)
      as [childId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[certifiedRest actualFinal]|] eqn:restRun;
      [|discriminate pushed].
    inversion pushed; subst certified actualFinal.
    pose proof (traced_push_pending_node_position_le_cap
      (pss_root child) state childId afterChild childRun) as childBound.
    pose proof (IH pending afterChild certifiedRest final restRun) as restBound.
    cbn [pending_nodes_size_cap]. lia.
Qed.

(** The producer's conservative byte budget is proved against its actual
    delayed-root callback discipline, not against a flattened tree.  The
    forest clause carries the byte liabilities of every pending sibling root;
    the tree clause carries the current pending root. *)
Theorem certified_source_writer_size_upper_bound : forall line_hash,
  (forall tree state pending final,
      certified_build_pending_tree line_hash tree state =
        Some (pending, final) ->
      writer_position (tws_state final) +
        pending_node_size_cap (pss_root pending) <=
      writer_position (tws_state state) + writer_size_upper_bound tree) /\
  (forall forest state pending final,
      certified_build_pending_forest line_hash forest state =
        Some (pending, final) ->
      writer_position (tws_state final) + pending_nodes_size_cap pending <=
      writer_position (tws_state state) +
        writer_forest_size_upper_bound forest).
Proof.
  intro lineHash. apply writer_tree_forest_ind.
  - intros line value children childrenIH state pending final built.
    cbn [certified_build_pending_tree] in built.
    destruct (certified_build_pending_forest lineHash children state)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate built].
    pose proof (childrenIH state pendingChildren afterDescendants childrenRun)
      as descendantsBound.
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:childrenPush;
      [|discriminate built].
    pose proof (certified_push_pending_children_position_upper_bound
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren childrenPush) as childrenBound.
    assert (writer_position (tws_state afterChildren) <=
      writer_position (tws_state state) +
        writer_forest_size_upper_bound children) as afterChildrenBound by lia.
    destruct line as [|first rest].
    + inversion built; subst pending final.
      change (writer_position (tws_state afterChildren) + 51 <=
        writer_position (tws_state state) +
          (96 + 0 + writer_forest_size_upper_bound children)).
      lia.
    + destruct (traced_add_or_reuse_line lineHash (first :: rest)
        afterChildren) as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_position_upper_bound
        lineHash (first :: rest) afterChildren lineId afterLine lineRun)
        as lineBound.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion built; subst pending final.
        change (writer_position (tws_state afterLine) + 28 <=
          writer_position (tws_state state) +
            (96 + length (first :: rest) +
              writer_forest_size_upper_bound children)).
        cbn [length] in *. lia.
      * set (wrapperNode :=
          PendingBranch value
            (combine
              (source_semantic_forest_labels
                (SourceSemanticCons label child certifiedRest))
              (source_semantic_forest_ids
                (SourceSemanticCons label child certifiedRest)))).
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine]
          in built.
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        injection built as pendingExact finalExact.
        subst pending final.
        pose proof (traced_push_pending_node_position_le_cap
          wrapperNode afterLine wrapperId afterWrapper wrapperRun)
          as wrapperBound.
        unfold wrapperNode in wrapperBound.
        unfold traced_push_pending_node, push_pending_node in wrapperRun.
        inversion wrapperRun; subst wrapperId afterWrapper.
        cbn [writer_size_upper_bound pending_node_size_cap pss_root tws_state
          length] in *.
        lia.
  - intros state pending final built.
    inversion built; subst pending final. cbn [pending_nodes_size_cap]. lia.
  - intros label child childIH rest restIH state pending final built.
    cbn [certified_build_pending_forest] in built.
    destruct (certified_build_pending_tree lineHash child state)
      as [[pendingChild afterChild]|] eqn:childRun;
      [|discriminate built].
    destruct (certified_build_pending_forest lineHash rest afterChild)
      as [[pendingRest actualFinal]|] eqn:restRun;
      [|discriminate built].
    inversion built; subst pending actualFinal.
    pose proof (childIH state pendingChild afterChild childRun) as childBound.
    pose proof (restIH afterChild pendingRest final restRun) as restBound.
    cbn [writer_forest_size_upper_bound pending_nodes_size_cap]. lia.
Qed.

Theorem canonical_writer_semantic_certificate_image_bound :
  forall line_hash tree bytes certificate objects,
    canonical_writer_tree tree ->
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    ACT_HEADER_LEN + length (physical_writer_bytes objects) <
      Nat.pow 256 8.
Proof.
  intros lineHash tree bytes certificate objects canonical encoded.
  destruct canonical as [wellFormed [valuesBounded [noUnary sizeBound]]].
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  pose proof (proj1 (certified_source_writer_size_upper_bound lineHash)
    tree initial pending beforeRoot built) as buildBound.
  pose proof (traced_push_pending_node_position_le_cap
    (pss_root pending) beforeRoot rootId final rootRun) as rootBound.
  assert (writer_position (tws_state initial) = ACT_HEADER_LEN)
    as initialPosition by reflexivity.
  assert (writer_position (tws_state final) <=
    ACT_HEADER_LEN + writer_size_upper_bound tree) as finalSizeBound.
  { rewrite initialPosition in buildBound. lia. }
  assert (traced_writer_full_invariant initial) as initialInvariant
    by (repeat split; reflexivity).
  pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
    tree initial pending beforeRoot built initialInvariant) as beforeInvariant.
  pose proof (traced_push_pending_node_preserves_full_invariant
    (pss_root pending) beforeRoot beforeInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant. cbn in finalInvariant.
  unfold traced_writer_full_invariant, traced_writer_invariant
    in finalInvariant.
  destruct finalInvariant as [[consistent layout] cache].
  unfold traced_writer_consistent in consistent.
  unfold writer_position in finalSizeBound.
  rewrite consistent in finalSizeBound. lia.
Qed.

Theorem semantic_certificate_has_all_direct_domains :
  forall line_hash tree bytes certificate objects,
    canonical_writer_shape tree ->
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    ACT_HEADER_LEN + length (physical_writer_bytes objects) <
      Nat.pow 256 8 ->
    source_semantic_tree_direct_domains
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) certificate.
Proof.
  intros lineHash tree bytes certificate objects canonical encoded imageBound.
  set (initial :=
    {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash tree initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  assert (traced_writer_full_invariant initial) as initialInvariant
    by (repeat split; reflexivity).
  pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
    tree initial pending beforeRoot built initialInvariant) as beforeInvariant.
  pose proof (traced_push_pending_node_preserves_full_invariant
    (pss_root pending) beforeRoot beforeInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant. cbn in finalInvariant.
  assert (writer_position (tws_state final) =
    ACT_HEADER_LEN + length (physical_writer_bytes (tws_objects final)))
    as finalPositionExact.
  { unfold writer_position, traced_writer_full_invariant,
      traced_writer_invariant in finalInvariant.
    destruct finalInvariant as [[consistent layout] cache].
    unfold traced_writer_consistent in consistent.
    unfold writer_position. now rewrite consistent. }
  assert (writer_position (tws_state final) < Nat.pow 256 8)
    as finalBound by (rewrite finalPositionExact; exact imageBound).
  pose proof (traced_push_pending_node_erases
    (pss_root pending) beforeRoot rootId final rootRun) as rootErases.
  pose proof (push_pending_node_position_exact (pss_root pending)
    (tws_state beforeRoot) rootId (tws_state final) rootErases)
    as [rootAtEnd finalAfterRoot].
  subst rootId.
  assert (writer_position (tws_state beforeRoot) < Nat.pow 256 8)
    as beforeRootBound by lia.
  pose proof (proj1 (certified_source_writer_preserves_direct_readiness lineHash)
    tree initial pending beforeRoot canonical initialInvariant built
    beforeRootBound) as pendingReady.
  pose proof (pending_source_semantic_direct_ready_finalizes
    (writer_position (tws_state beforeRoot))
    (writer_position (tws_state beforeRoot)) pending pendingReady)
    as allDomains.
  specialize (allDomains (le_n (writer_position (tws_state beforeRoot)))).
  assert (writer_position (tws_state beforeRoot) +
    length (encode_pending_node (writer_position (tws_state beforeRoot))
      (pss_root pending)) <
    Nat.pow 256 8) as rootBounded.
  { rewrite <- finalAfterRoot. exact finalBound. }
  specialize (allDomains rootBounded).
  rewrite <- finalAfterRoot in allDomains.
  rewrite finalPositionExact in allDomains.
  exact allDomains.
Qed.

Theorem semantic_certificate_all_nodes_parse_directly :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_tree_direct_domains
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) certificate ->
    source_semantic_tree_parses_directly
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes certificate.
Proof.
  intros lineHash tree bytes certificate objects encoded allDomains.
  pose proof (semantic_certificate_has_exact_image_and_layout
    lineHash tree bytes certificate objects encoded) as [bytesExact layout].
  apply (proj1 (recorded_source_semantic_nodes_parse_directly
    (source_semantic_root_id certificate) bytes objects bytesExact layout)
    certificate).
  - eapply semantic_certificate_has_exact_shape; eassumption.
  - eapply semantic_certificate_node_objects_are_recorded; eassumption.
  - eapply semantic_certificate_referenced_lines_are_recorded; eassumption.
  - exact allDomains.
Qed.

Theorem canonical_semantic_certificate_has_all_direct_domains :
  forall line_hash tree bytes certificate objects,
    canonical_writer_tree tree ->
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_tree_direct_domains
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) certificate.
Proof.
  intros lineHash tree bytes certificate objects canonical encoded.
  eapply semantic_certificate_has_all_direct_domains.
  - apply canonical_writer_tree_has_canonical_shape. exact canonical.
  - exact encoded.
  - eapply canonical_writer_semantic_certificate_image_bound; eassumption.
Qed.

Theorem canonical_writer_semantic_certificate_parses_every_node :
  forall line_hash tree bytes certificate objects,
    canonical_writer_tree tree ->
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    source_semantic_tree_parses_directly
      (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes certificate.
Proof.
  intros lineHash tree bytes certificate objects canonical encoded.
  apply (semantic_certificate_all_nodes_parse_directly
    lineHash tree bytes certificate objects encoded).
  eapply canonical_semantic_certificate_has_all_direct_domains; eassumption.
Qed.

Theorem direct_semantic_three_way_root_matches_production_shape :
  direct_semantic_node_at 22 27 ACT_THREE_WAY_IMAGE =
    Some {| dsm_id := 22; dsm_encoded_len := 5; dsm_value := None;
            dsm_child_parent := 22; dsm_child_labels := [10;20;30];
            dsm_child_ids := [16;18;20]; dsm_line := None;
            dsm_wrapper := None |}.
Proof. vm_compute. reflexivity. Qed.

Theorem direct_semantic_line_root_matches_production_shape :
  direct_semantic_node_at 19 22 ACT_LINE_IMAGE =
    Some {| dsm_id := 19; dsm_encoded_len := 3; dsm_value := Some 0;
            dsm_child_parent := 19; dsm_child_labels := [];
            dsm_child_ids := [];
            dsm_line := Some
              {| plo_id := 16; plo_encoded_len := 3; plo_bytes := [97;98] |};
            dsm_wrapper := None |}.
Proof. vm_compute. reflexivity. Qed.

Definition direct_semantic_id_eq (id : nat)
    (node : direct_semantic_node_model) : bool := Nat.eqb id (dsm_id node).

Fixpoint find_direct_semantic_node (id : nat)
    (nodes : list direct_semantic_node_model)
    : option direct_semantic_node_model :=
  match nodes with
  | [] => None
  | node :: rest =>
      if direct_semantic_id_eq id node then Some node
      else find_direct_semantic_node id rest
  end.

Definition direct_semantic_physical_ids (node : direct_semantic_node_model)
    : list nat :=
  dsm_id node ::
  match dsm_wrapper node with None => [] | Some (wrapper, _) => [wrapper] end.

Fixpoint source_semantic_preorder (tree : source_semantic_tree)
    : list source_semantic_tree :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      tree :: source_semantic_forest_preorder children
  end
with source_semantic_forest_preorder (forest : source_semantic_forest)
    : list source_semantic_tree :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_preorder child ++ source_semantic_forest_preorder rest
  end.

Definition source_semantic_erased_preorder (tree : source_semantic_tree)
    : list direct_semantic_node_model :=
  map source_semantic_erase (source_semantic_preorder tree).

Definition source_semantic_preorder_physical_ids
    (tree : source_semantic_tree) : list nat :=
  concat
    (map (fun node => direct_semantic_physical_ids (source_semantic_erase node))
      (source_semantic_preorder tree)).

Definition source_semantic_forest_preorder_physical_ids
    (forest : source_semantic_forest) : list nat :=
  concat
    (map (fun node => direct_semantic_physical_ids (source_semantic_erase node))
      (source_semantic_forest_preorder forest)).

Theorem source_semantic_preorder_length :
  (forall tree,
      length (source_semantic_preorder tree) =
        source_semantic_node_count tree) /\
  (forall forest,
      length (source_semantic_forest_preorder forest) =
        source_semantic_forest_node_count forest).
Proof.
  apply source_semantic_tree_forest_ind; intros; cbn; rewrite ?length_app; lia.
Qed.

Lemma direct_semantic_physical_ids_erase : forall tree,
  direct_semantic_physical_ids (source_semantic_erase tree) =
    source_semantic_root_id tree ::
    map physical_writer_object_start (source_semantic_own_wrapper_objects tree).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper].
  destruct wrapper as [[wrapperId wrapperLen]|]; reflexivity.
Qed.

Lemma source_semantic_preorder_physical_ids_unfold : forall tree,
  source_semantic_preorder_physical_ids tree =
    direct_semantic_physical_ids (source_semantic_erase tree) ++
    source_semantic_forest_preorder_physical_ids
      (match tree with
       | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
       end).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper].
  unfold source_semantic_preorder_physical_ids,
    source_semantic_forest_preorder_physical_ids.
  cbn [source_semantic_preorder]. reflexivity.
Qed.

Lemma source_semantic_forest_preorder_physical_ids_unfold : forall forest,
  source_semantic_forest_preorder_physical_ids forest =
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      source_semantic_preorder_physical_ids child ++
      source_semantic_forest_preorder_physical_ids rest
  end.
Proof.
  intros [|label child rest]; [reflexivity|].
  unfold source_semantic_preorder_physical_ids,
    source_semantic_forest_preorder_physical_ids.
  cbn [source_semantic_forest_preorder]. rewrite map_app, concat_app.
  reflexivity.
Qed.

Theorem source_semantic_preorder_physical_ids_projection_permutation :
  (forall tree,
      Permutation (source_semantic_preorder_physical_ids tree)
        (map physical_writer_object_start
          (source_semantic_pending_node_objects tree ++
           [source_semantic_root_object tree]))) /\
  (forall forest,
      Permutation (source_semantic_forest_preorder_physical_ids forest)
        (map physical_writer_object_start
          (source_semantic_forest_pending_node_objects forest ++
           source_semantic_forest_root_objects forest))).
Proof.
  apply source_semantic_tree_forest_ind.
  - intros rootId rootLen rootNode line value children childrenIH lineId
      lineHash wrapper.
    rewrite source_semantic_preorder_physical_ids_unfold,
      direct_semantic_physical_ids_erase.
    cbn [source_semantic_pending_node_objects source_semantic_root_object].
    rewrite !map_app.
    rewrite map_app in childrenIH.
    destruct wrapper as [[wrapperId wrapperLen]|].
    + cbn [source_semantic_own_wrapper_objects map].
      eapply Permutation_trans.
      * eapply Permutation_app_head. exact childrenIH.
      * eapply Permutation_trans.
        -- apply Permutation_app_comm.
        -- cbn [source_semantic_root_id physical_writer_object_start
             source_semantic_root_object].
           set (forestIds :=
             map physical_writer_object_start
               (source_semantic_forest_pending_node_objects children) ++
             map physical_writer_object_start
               (source_semantic_forest_root_objects children)).
           rewrite (app_assoc
             (map physical_writer_object_start
               (source_semantic_forest_pending_node_objects children))
             (map physical_writer_object_start
               (source_semantic_forest_root_objects children))
             [wrapperId]).
           fold forestIds.
           rewrite <- (app_assoc forestIds [wrapperId] [rootId]).
           eapply Permutation_app_head. apply perm_swap.
    + cbn [source_semantic_own_wrapper_objects map].
      rewrite !app_nil_r.
      eapply Permutation_trans.
      * eapply Permutation_app_head. exact childrenIH.
      * apply Permutation_app_comm.
  - constructor.
  - intros label child childIH rest restIH.
    rewrite source_semantic_forest_preorder_physical_ids_unfold.
    cbn [source_semantic_forest_pending_node_objects
      source_semantic_forest_root_objects].
    rewrite !map_app.
    rewrite map_app in childIH, restIH.
    eapply Permutation_trans.
    + eapply Permutation_app.
      * exact childIH.
      * exact restIH.
    + repeat rewrite <- app_assoc.
      eapply Permutation_app_head.
      cbn [map].
      change (Permutation
        (physical_writer_object_start (source_semantic_root_object child) ::
         map physical_writer_object_start
           (source_semantic_forest_pending_node_objects rest) ++
         map physical_writer_object_start
           (source_semantic_forest_root_objects rest))
        (map physical_writer_object_start
           (source_semantic_forest_pending_node_objects rest) ++
         physical_writer_object_start (source_semantic_root_object child) ::
         map physical_writer_object_start
           (source_semantic_forest_root_objects rest))).
      apply Permutation_middle.
Qed.

Theorem semantic_certificate_preorder_physical_ids_nodup :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    NoDup (source_semantic_preorder_physical_ids certificate).
Proof.
  intros lineHash tree bytes certificate objects encoded.
  pose proof (semantic_certificate_has_exact_image_and_layout
    lineHash tree bytes certificate objects encoded) as [_ layout].
  pose proof (physical_layout_node_starts_nodup
    ACT_HEADER_LEN objects layout) as physicalNoDup.
  pose proof (semantic_certificate_node_projection_exact
    lineHash tree bytes certificate objects encoded) as projectionExact.
  rewrite projectionExact in physicalNoDup.
  eapply Permutation_NoDup.
  - apply Permutation_sym.
    exact (proj1
      source_semantic_preorder_physical_ids_projection_permutation certificate).
  - exact physicalNoDup.
Qed.

Fixpoint source_semantic_forest_to_list (forest : source_semantic_forest)
    : list source_semantic_tree :=
  match forest with
  | SourceSemanticNil => []
  | SourceSemanticCons _ child rest =>
      child :: source_semantic_forest_to_list rest
  end.

Definition source_work_preorder (work : list source_semantic_tree)
    : list source_semantic_tree :=
  concat (map source_semantic_preorder work).

Definition source_work_physical_ids (work : list source_semantic_tree)
    : list nat :=
  concat (map source_semantic_preorder_physical_ids work).

Definition source_work_root_ids (work : list source_semantic_tree)
    : list nat := map source_semantic_root_id work.

Fixpoint source_work_count (work : list source_semantic_tree) : nat :=
  match work with
  | [] => 0
  | tree :: rest => source_semantic_node_count tree + source_work_count rest
  end.

Lemma source_work_preorder_app : forall left right,
  source_work_preorder (left ++ right) =
    source_work_preorder left ++ source_work_preorder right.
Proof.
  intros left right. unfold source_work_preorder.
  rewrite map_app, concat_app. reflexivity.
Qed.

Lemma source_work_physical_ids_app : forall left right,
  source_work_physical_ids (left ++ right) =
    source_work_physical_ids left ++ source_work_physical_ids right.
Proof.
  intros left right. unfold source_work_physical_ids.
  rewrite map_app, concat_app. reflexivity.
Qed.

Lemma source_work_root_ids_app : forall left right,
  source_work_root_ids (left ++ right) =
    source_work_root_ids left ++ source_work_root_ids right.
Proof. intros left right. unfold source_work_root_ids. apply map_app. Qed.

Lemma source_work_count_app : forall left right,
  source_work_count (left ++ right) =
    source_work_count left + source_work_count right.
Proof.
  induction left as [|tree rest IH]; intro right; cbn; rewrite ?IH; lia.
Qed.

Lemma source_semantic_forest_to_list_preorder : forall forest,
  source_work_preorder (source_semantic_forest_to_list forest) =
    source_semantic_forest_preorder forest.
Proof.
  induction forest as [|label child rest IH]; [reflexivity|].
  change (source_semantic_preorder child ++
    source_work_preorder (source_semantic_forest_to_list rest) =
    source_semantic_preorder child ++ source_semantic_forest_preorder rest).
  now rewrite IH.
Qed.

Lemma source_semantic_forest_to_list_physical_ids : forall forest,
  source_work_physical_ids (source_semantic_forest_to_list forest) =
    source_semantic_forest_preorder_physical_ids forest.
Proof.
  induction forest as [|label child rest IH]; [reflexivity|].
  rewrite source_semantic_forest_preorder_physical_ids_unfold.
  change (source_semantic_preorder_physical_ids child ++
    source_work_physical_ids (source_semantic_forest_to_list rest) =
    source_semantic_preorder_physical_ids child ++
      source_semantic_forest_preorder_physical_ids rest).
  now rewrite IH.
Qed.

Lemma source_semantic_forest_to_list_count : forall forest,
  source_work_count (source_semantic_forest_to_list forest) =
    source_semantic_forest_node_count forest.
Proof.
  induction forest as [|label child rest IH]; cbn; lia.
Qed.

Lemma source_semantic_forest_to_list_root_ids : forall forest,
  source_work_root_ids (source_semantic_forest_to_list forest) =
    source_semantic_forest_ids forest.
Proof.
  induction forest as [|label child rest IH].
  - reflexivity.
  - change (source_semantic_root_id child ::
      source_work_root_ids (source_semantic_forest_to_list rest) =
      source_semantic_root_id child :: source_semantic_forest_ids rest).
    now rewrite IH.
Qed.

Lemma source_work_preorder_step : forall tree rest,
  source_work_preorder (tree :: rest) =
    tree :: source_work_preorder
      (source_semantic_forest_to_list
        (match tree with
         | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
         end) ++ rest).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    rest.
  cbn [source_work_preorder source_semantic_preorder].
  rewrite source_work_preorder_app,
    source_semantic_forest_to_list_preorder.
  reflexivity.
Qed.

Lemma source_work_physical_ids_step : forall tree rest,
  source_work_physical_ids (tree :: rest) =
    direct_semantic_physical_ids (source_semantic_erase tree) ++
    source_work_physical_ids
      (source_semantic_forest_to_list
        (match tree with
         | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
         end) ++ rest).
Proof.
  intros tree rest.
  change (source_semantic_preorder_physical_ids tree ++
    source_work_physical_ids rest =
    direct_semantic_physical_ids (source_semantic_erase tree) ++
    source_work_physical_ids
      (source_semantic_forest_to_list
        (match tree with
         | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
         end) ++ rest)).
  rewrite source_semantic_preorder_physical_ids_unfold,
    source_work_physical_ids_app,
    source_semantic_forest_to_list_physical_ids.
  now rewrite app_assoc.
Qed.

Lemma source_work_count_step : forall tree rest,
  source_work_count (tree :: rest) =
    S (source_work_count
      (source_semantic_forest_to_list
        (match tree with
         | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
         end) ++ rest)).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    rest.
  cbn [source_work_count source_semantic_node_count].
  rewrite source_work_count_app, source_semantic_forest_to_list_count. lia.
Qed.

Lemma source_work_root_ids_step : forall tree rest,
  source_semantic_forest_ids
    (match tree with
     | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
     end) ++ source_work_root_ids rest =
  source_work_root_ids
    (source_semantic_forest_to_list
      (match tree with
       | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
       end) ++ rest).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    rest.
  rewrite source_work_root_ids_app,
    source_semantic_forest_to_list_root_ids. reflexivity.
Qed.

Definition physical_id_is_fresh (seen : list nat) (id : nat) : bool :=
  negb (existsb (Nat.eqb id) seen).

Lemma physical_id_is_fresh_true_iff : forall seen id,
  physical_id_is_fresh seen id = true <-> ~ In id seen.
Proof.
  intros seen id. unfold physical_id_is_fresh. split.
  - intros fresh present.
    apply negb_true_iff in fresh.
    assert (existsb (Nat.eqb id) seen = true) as found.
    { apply existsb_exists. exists id. split; [exact present|].
      apply Nat.eqb_refl. }
    congruence.
  - intros absent. apply negb_true_iff.
    destruct (existsb (Nat.eqb id) seen) eqn:found; [|reflexivity].
    apply existsb_exists in found.
    destruct found as [candidate [present equal]].
    apply Nat.eqb_eq in equal. subst candidate. contradiction.
Qed.

Lemma forallb_physical_ids_fresh_of_nodup : forall current remaining seen,
  NoDup ((current ++ remaining) ++ seen) ->
  forallb (physical_id_is_fresh seen) current = true.
Proof.
  induction current as [|id current IH]; intros remaining seen unique;
    [reflexivity|].
  cbn [app] in unique. inversion unique as [|id' tail absent uniqueTail];
    subst id' tail.
  cbn [forallb]. apply andb_true_iff. split.
  - apply physical_id_is_fresh_true_iff. intro present.
    apply absent. apply in_or_app. right. exact present.
  - apply (IH remaining seen). exact uniqueTail.
Qed.

Lemma nodup_rotate_work_prefix : forall (current remaining seen : list nat),
  NoDup ((current ++ remaining) ++ seen) ->
  NoDup ((remaining ++ current) ++ seen).
Proof.
  intros current remaining seen unique.
  eapply Permutation_NoDup.
  - apply Permutation_app_tail. apply Permutation_app_comm.
  - exact unique.
Qed.

Definition source_work_parses_directly (arena_end : nat)
    (image : list bbyte) (work : list source_semantic_tree) : Prop :=
  Forall (source_semantic_tree_parses_directly arena_end image) work.

Lemma source_semantic_forest_to_list_parses_directly :
  forall arena_end image forest,
    source_semantic_forest_parses_directly arena_end image forest ->
    source_work_parses_directly arena_end image
      (source_semantic_forest_to_list forest).
Proof.
  intros arenaEnd image forest. induction forest as [|label child rest IH];
    intro parses.
  - constructor.
  - cbn [source_semantic_forest_parses_directly] in parses.
    destruct parses as [childParses restParses].
    constructor; [exact childParses|].
    apply IH. exact restParses.
Qed.

Lemma source_work_parses_directly_step : forall arena_end image tree rest,
  source_work_parses_directly arena_end image (tree :: rest) ->
  source_work_parses_directly arena_end image
    (source_semantic_forest_to_list
      (match tree with
       | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
       end) ++ rest).
Proof.
  intros arenaEnd image tree rest parses.
  inversion parses as [|tree' rest' treeParses restParses]; subst.
  apply Forall_app. split; [|exact restParses].
  apply source_semantic_forest_to_list_parses_directly.
  destruct tree. exact (proj2 treeParses).
Qed.

Record direct_semantic_discovery : Type := {
  dsd_pending : list nat;
  dsd_nodes : list direct_semantic_node_model;
  dsd_seen_physical_ids : list nat
}.

(** Each transition consumes one semantic node.  The token values are
    irrelevant; the later producer-side bound must prove that an image-sized
    internal token list cannot be exhausted for a canonical writer image. *)
Fixpoint discover_direct_semantic_nodes (tokens : list nat) (arena_end : nat)
    (image : list bbyte) (state : direct_semantic_discovery)
    : option direct_semantic_discovery :=
  match dsd_pending state with
  | [] => Some state
  | id :: pending_rest =>
      match tokens with
      | [] => None
      | _ :: token_rest =>
          match direct_semantic_node_at id arena_end image with
          | None => None
          | Some node =>
              let physical_ids := direct_semantic_physical_ids node in
              if forallb
                   (physical_id_is_fresh (dsd_seen_physical_ids state))
                   physical_ids
              then discover_direct_semantic_nodes token_rest arena_end image
                {| dsd_pending := dsm_child_ids node ++ pending_rest;
                   dsd_nodes := node :: dsd_nodes state;
                   dsd_seen_physical_ids :=
                     physical_ids ++ dsd_seen_physical_ids state |}
              else None
          end
      end
  end.

Theorem discover_source_work_exact :
  forall tokens work arena_end image accumulated seen,
    source_work_count work <= length tokens ->
    source_work_parses_directly arena_end image work ->
    NoDup (source_work_physical_ids work ++ seen) ->
    exists final_seen,
      discover_direct_semantic_nodes tokens arena_end image
        {| dsd_pending := source_work_root_ids work;
           dsd_nodes := accumulated;
           dsd_seen_physical_ids := seen |} =
      Some {| dsd_pending := [];
              dsd_nodes :=
                rev (map source_semantic_erase
                  (source_work_preorder work)) ++ accumulated;
              dsd_seen_physical_ids := final_seen |}.
Proof.
  induction tokens as [|token tokenRest IH];
    intros work arenaEnd image accumulated seen enough parses unique.
  - destruct work as [|tree rest].
    + exists seen. reflexivity.
    + pose proof (source_work_count_step tree rest) as countStep.
      cbn [length] in enough. lia.
  - destruct work as [|tree rest].
    + exists seen. reflexivity.
    + set (nextWork :=
        source_semantic_forest_to_list
          (match tree with
           | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
           end) ++ rest).
      assert (nextEnough : source_work_count nextWork <= length tokenRest).
      { pose proof (source_work_count_step tree rest) as countStep.
        fold nextWork in countStep. cbn [length] in enough. lia. }
      assert (nextParses :
        source_work_parses_directly arenaEnd image nextWork).
      { unfold nextWork. eapply source_work_parses_directly_step.
        exact parses. }
      assert (rootParses :
        direct_semantic_node_at (source_semantic_root_id tree)
          arenaEnd image = Some (source_semantic_erase tree)).
      { inversion parses as [|tree' rest' treeParses restParses]; subst.
        destruct tree. exact (proj1 treeParses). }
      assert (fresh : forallb
        (physical_id_is_fresh seen)
        (direct_semantic_physical_ids (source_semantic_erase tree)) = true).
      { apply forallb_physical_ids_fresh_of_nodup with
          (remaining := source_work_physical_ids nextWork).
        unfold nextWork.
        rewrite <- source_work_physical_ids_step.
        exact unique. }
      assert (nextUnique : NoDup
        (source_work_physical_ids nextWork ++
         (direct_semantic_physical_ids (source_semantic_erase tree) ++ seen))).
      { rewrite app_assoc.
        apply nodup_rotate_work_prefix.
        unfold nextWork.
        rewrite <- source_work_physical_ids_step.
        exact unique. }
      destruct (IH nextWork arenaEnd image
        (source_semantic_erase tree :: accumulated)
        (direct_semantic_physical_ids (source_semantic_erase tree) ++ seen)
        nextEnough nextParses nextUnique) as [finalSeen discovered].
      exists finalSeen.
      unfold source_work_root_ids at 1.
      change (discover_direct_semantic_nodes (token :: tokenRest)
        arenaEnd image
        {| dsd_pending := source_semantic_root_id tree ::
             source_work_root_ids rest;
           dsd_nodes := accumulated;
           dsd_seen_physical_ids := seen |} =
        Some {| dsd_pending := [];
                dsd_nodes := rev (map source_semantic_erase
                  (source_work_preorder (tree :: rest))) ++ accumulated;
                dsd_seen_physical_ids := finalSeen |}).
      unfold discover_direct_semantic_nodes.
      cbv beta iota zeta delta [dsd_pending dsd_seen_physical_ids dsd_nodes].
      rewrite rootParses, fresh.
      assert (childIds : dsm_child_ids (source_semantic_erase tree) =
        source_semantic_forest_ids
          (match tree with
           | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
           end)) by (destruct tree; reflexivity).
      rewrite childIds, source_work_root_ids_step.
      fold nextWork.
      change (discover_direct_semantic_nodes tokenRest arenaEnd image
        {| dsd_pending := source_work_root_ids nextWork;
           dsd_nodes := source_semantic_erase tree :: accumulated;
           dsd_seen_physical_ids :=
             direct_semantic_physical_ids (source_semantic_erase tree) ++ seen |}
        = Some {| dsd_pending := [];
                  dsd_nodes := rev (map source_semantic_erase
                    (source_work_preorder (tree :: rest))) ++ accumulated;
                  dsd_seen_physical_ids := finalSeen |}).
      rewrite discovered.
      f_equal.
      rewrite source_work_preorder_step.
      fold nextWork.
      cbn [map rev].
      rewrite <- app_assoc. reflexivity.
Qed.

Theorem canonical_writer_semantic_certificate_discovery_exact :
  forall line_hash tree bytes certificate objects,
    canonical_writer_tree tree ->
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    exists final_seen,
      discover_direct_semantic_nodes (seq 0 (length bytes))
        (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes
        {| dsd_pending := [source_semantic_root_id certificate];
           dsd_nodes := [];
           dsd_seen_physical_ids := [] |} =
      Some {| dsd_pending := [];
              dsd_nodes := rev (source_semantic_erased_preorder certificate);
              dsd_seen_physical_ids := final_seen |}.
Proof.
  intros lineHash tree bytes certificate objects canonical encoded.
  pose proof (proj1 (semantic_certificate_node_count_is_byte_bounded
    lineHash tree bytes certificate objects encoded)) as countBound.
  pose proof (canonical_writer_semantic_certificate_parses_every_node
    lineHash tree bytes certificate objects canonical encoded) as parses.
  pose proof (semantic_certificate_preorder_physical_ids_nodup
    lineHash tree bytes certificate objects encoded) as unique.
  destruct (discover_source_work_exact
    (seq 0 (length bytes)) [certificate]
    (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes [] [])
    as [finalSeen discovered].
  - cbn [source_work_count]. rewrite length_seq. lia.
  - constructor; [exact parses|constructor].
  - unfold source_work_physical_ids. cbn. now repeat rewrite app_nil_r.
  - exists finalSeen.
    unfold source_work_root_ids, source_work_preorder,
      source_semantic_erased_preorder in discovered.
    cbn [map concat] in discovered.
    repeat rewrite app_nil_r in discovered.
    exact discovered.
Qed.

Definition direct_semantic_node_ids
    (nodes : list direct_semantic_node_model) : list nat :=
  map dsm_id nodes.

Lemma direct_semantic_node_ids_in_physical : forall nodes id,
  In id (direct_semantic_node_ids nodes) ->
  In id (concat (map direct_semantic_physical_ids nodes)).
Proof.
  induction nodes as [|node rest IH]; intros id present; [contradiction|].
  cbn [direct_semantic_node_ids map concat] in present |- *.
  destruct present as [same|present].
  - subst id. apply in_or_app. left.
    cbn [direct_semantic_physical_ids]. now left.
  - apply in_or_app. right. apply IH. exact present.
Qed.

Lemma direct_semantic_physical_nodup_implies_node_ids_nodup : forall nodes,
  NoDup (concat (map direct_semantic_physical_ids nodes)) ->
  NoDup (direct_semantic_node_ids nodes).
Proof.
  induction nodes as [|node rest IH]; intro unique; [constructor|].
  destruct node as [id encodedLen value childParent labels children line wrapper].
  destruct wrapper as [[wrapperId wrapperLen]|].
  - cbn [direct_semantic_physical_ids map concat] in unique |- *.
    inversion unique as [|id' tail idAbsent tailUnique]; subst id' tail.
    inversion tailUnique as [|wrapperId' tail wrapperAbsent restUnique];
      subst wrapperId' tail.
    constructor.
    + intro rootPresent. apply idAbsent. right.
      eapply direct_semantic_node_ids_in_physical. exact rootPresent.
    + apply IH. exact restUnique.
  - cbn [direct_semantic_physical_ids map concat] in unique |- *.
    inversion unique as [|id' tail idAbsent restUnique]; subst id' tail.
    constructor.
    + intro rootPresent. apply idAbsent.
      eapply direct_semantic_node_ids_in_physical. exact rootPresent.
    + apply IH. exact restUnique.
Qed.

Lemma find_direct_semantic_node_by_membership : forall nodes node,
  NoDup (direct_semantic_node_ids nodes) ->
  In node nodes ->
  find_direct_semantic_node (dsm_id node) nodes = Some node.
Proof.
  induction nodes as [|head rest IH]; intros node unique present;
    [contradiction|].
  cbn [direct_semantic_node_ids map] in unique.
  inversion unique as [|headId tail headAbsent restUnique]; subst headId tail.
  destruct present as [same|present].
  - subst node. cbn [find_direct_semantic_node direct_semantic_id_eq].
    unfold direct_semantic_id_eq. now rewrite Nat.eqb_refl.
  - cbn [find_direct_semantic_node direct_semantic_id_eq].
    unfold direct_semantic_id_eq.
    destruct (Nat.eqb (dsm_id node) (dsm_id head)) eqn:equal.
    + apply Nat.eqb_eq in equal.
      exfalso. apply headAbsent. rewrite <- equal.
      apply in_map. exact present.
    + apply IH; assumption.
Qed.

Theorem find_source_semantic_node_in_reversed_preorder :
  forall certificate tree,
    NoDup (source_semantic_preorder_physical_ids certificate) ->
    In tree (source_semantic_preorder certificate) ->
    find_direct_semantic_node (source_semantic_root_id tree)
      (rev (source_semantic_erased_preorder certificate)) =
      Some (source_semantic_erase tree).
Proof.
  intros certificate tree physicalUnique present.
  assert (nodeIdsUnique : NoDup (direct_semantic_node_ids
    (source_semantic_erased_preorder certificate))).
  { apply direct_semantic_physical_nodup_implies_node_ids_nodup.
    unfold source_semantic_erased_preorder,
      source_semantic_preorder_physical_ids.
    now rewrite map_map. }
  assert (reversedIdsUnique : NoDup (direct_semantic_node_ids
    (rev (source_semantic_erased_preorder certificate)))).
  { unfold direct_semantic_node_ids in *. rewrite map_rev.
    apply NoDup_rev. exact nodeIdsUnique. }
  assert (erasedPresent : In (source_semantic_erase tree)
    (rev (source_semantic_erased_preorder certificate))).
  { apply (proj1 (in_rev (source_semantic_erased_preorder certificate)
      (source_semantic_erase tree))).
    unfold source_semantic_erased_preorder.
    apply in_map. exact present. }
  assert (idExact : dsm_id (source_semantic_erase tree) =
    source_semantic_root_id tree) by (destruct tree; reflexivity).
  rewrite <- idExact.
  apply find_direct_semantic_node_by_membership; assumption.
Qed.

Record direct_line_replay_state : Type := {
  dlr_position : nat;
  dlr_lines : list line_cache_entry
}.

Definition direct_replay_state_of_writer (state : act_writer_state)
    : direct_line_replay_state :=
  {| dlr_position := writer_position state;
     dlr_lines := aws_lines state |}.

Definition parsed_replayed_line (line_id : nat) (line : list bbyte)
    : parsed_line_object :=
  {| plo_id := line_id;
     plo_encoded_len :=
       length (encode_act_varint (length line)) + length line;
     plo_bytes := line |}.

Definition allocate_replayed_line (hash : nat) (line : parsed_line_object)
    (state : direct_line_replay_state) : option direct_line_replay_state :=
  if Nat.eqb (plo_id line) (dlr_position state) then
    Some
      {| dlr_position := dlr_position state + plo_encoded_len line;
         dlr_lines := put_line_hash
           {| lce_hash := hash; lce_bytes := plo_bytes line;
              lce_id := plo_id line |}
           (dlr_lines state) |}
  else None.

Definition replay_canonical_line (line_hash : list bbyte -> nat)
    (line : parsed_line_object) (state : direct_line_replay_state)
    : option direct_line_replay_state :=
  let hash := line_hash (plo_bytes line) in
  match lookup_line_hash hash (dlr_lines state) with
  | None => allocate_replayed_line hash line state
  | Some previous =>
      if list_eq_dec Nat.eq_dec (lce_bytes previous) (plo_bytes line)
      then if Nat.eqb (lce_id previous) (plo_id line) then Some state else None
      else allocate_replayed_line hash line state
  end.

Theorem replay_canonical_line_matches_add_or_reuse :
  forall line_hash line before line_id after,
    add_or_reuse_line line_hash line before = (line_id, after) ->
    replay_canonical_line line_hash (parsed_replayed_line line_id line)
      (direct_replay_state_of_writer before) =
    Some (direct_replay_state_of_writer after).
Proof.
  intros lineHash line before lineId after added.
  unfold add_or_reuse_line in added.
  unfold replay_canonical_line, parsed_replayed_line,
    direct_replay_state_of_writer.
  destruct (lookup_line_hash (lineHash line) (aws_lines before))
    as [previous|] eqn:found.
  - destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line)
      as [equal|different].
    + inversion added; subst lineId after.
      cbn. rewrite found.
      destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line)
        as [equal'|different']; [|contradiction].
      now rewrite Nat.eqb_refl.
    + inversion added; subst lineId after.
      cbn. rewrite found.
      destruct (list_eq_dec Nat.eq_dec (lce_bytes previous) line)
        as [equal'|different']; [contradiction|].
      unfold allocate_replayed_line. cbn.
      rewrite Nat.eqb_refl. f_equal.
      unfold writer_position. cbn [length].
      rewrite !length_app. f_equal; lia.
  - inversion added; subst lineId after.
    cbn. rewrite found. unfold allocate_replayed_line. cbn.
    rewrite Nat.eqb_refl. f_equal.
    unfold writer_position. cbn [length].
    rewrite !length_app. f_equal; lia.
Qed.

Fixpoint direct_children_encoded_len (children : list nat)
    (nodes : list direct_semantic_node_model) : option nat :=
  match children with
  | [] => Some 0
  | child :: rest =>
      match find_direct_semantic_node child nodes,
            direct_children_encoded_len rest nodes with
      | Some node, Some tail => Some (dsm_encoded_len node + tail)
      | _, _ => None
      end
  end.

Definition source_nodes_index (certificate : source_semantic_tree)
    (nodes : list direct_semantic_node_model) : Prop :=
  forall source_node,
    In source_node (source_semantic_preorder certificate) ->
    find_direct_semantic_node (source_semantic_root_id source_node) nodes =
      Some (source_semantic_erase source_node).

Theorem reversed_source_nodes_index : forall certificate,
  NoDup (source_semantic_preorder_physical_ids certificate) ->
  source_nodes_index certificate
    (rev (source_semantic_erased_preorder certificate)).
Proof.
  intros certificate unique sourceNode present.
  eapply find_source_semantic_node_in_reversed_preorder; eassumption.
Qed.

Lemma source_semantic_root_in_preorder : forall tree,
  In tree (source_semantic_preorder tree).
Proof. intros []; cbn [source_semantic_preorder]. now left. Qed.

Theorem direct_children_encoded_len_source_forest_exact :
  forall certificate nodes forest,
    source_nodes_index certificate nodes ->
    incl (source_semantic_forest_preorder forest)
      (source_semantic_preorder certificate) ->
    direct_children_encoded_len (source_semantic_forest_ids forest) nodes =
      Some (source_semantic_forest_root_bytes forest).
Proof.
  intros certificate nodes forest. induction forest as [|label child rest IH];
    intros indexed included; [reflexivity|].
  assert (childPresent : In child (source_semantic_preorder certificate)).
  { apply included. cbn [source_semantic_forest_preorder].
    apply in_or_app. left. apply source_semantic_root_in_preorder. }
  assert (restIncluded : incl (source_semantic_forest_preorder rest)
    (source_semantic_preorder certificate)).
  { intros node present. apply included.
    cbn [source_semantic_forest_preorder].
    apply in_or_app. right. exact present. }
  specialize (IH indexed restIncluded).
  cbn [source_semantic_forest_ids direct_children_encoded_len
    source_semantic_forest_root_bytes].
  rewrite (indexed child childPresent), IH.
  destruct child. reflexivity.
Qed.

Lemma source_semantic_children_preorder_in_parent : forall tree,
  incl
    (source_semantic_forest_preorder
      (match tree with
       | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
       end))
    (source_semantic_preorder tree).
Proof.
  intros [rootId rootLen rootNode line value children lineId lineHash wrapper]
    node present.
  cbn [source_semantic_preorder]. now right.
Qed.

Theorem direct_children_encoded_len_source_exact :
  forall certificate nodes tree,
    source_nodes_index certificate nodes ->
    incl (source_semantic_preorder tree)
      (source_semantic_preorder certificate) ->
    direct_children_encoded_len
      (dsm_child_ids (source_semantic_erase tree)) nodes =
    Some
      (source_semantic_forest_root_bytes
        (match tree with
         | SourceSemanticNode _ _ _ _ _ children _ _ _ => children
         end)).
Proof.
  intros certificate nodes tree indexed included.
  destruct tree as [rootId rootLen rootNode line value children lineId lineHash
    wrapper].
  cbn [source_semantic_erase dsm_child_ids].
  eapply direct_children_encoded_len_source_forest_exact; [exact indexed|].
  intros node present. apply included.
  cbn [source_semantic_preorder]. now right.
Qed.

Definition replay_direct_children (children : list nat)
    (child_bytes : nat) (state : direct_line_replay_state)
    : option direct_line_replay_state :=
  match children with
  | [] => Some state
  | first :: _ =>
      if Nat.eqb first (dlr_position state) then
        Some {| dlr_position := dlr_position state + child_bytes;
                dlr_lines := dlr_lines state |}
      else None
  end.

Lemma push_pending_node_preserves_line_cache :
  forall node before id after,
    push_pending_node node before = (id, after) ->
    aws_lines after = aws_lines before.
Proof.
  intros node before id after pushed.
  unfold push_pending_node in pushed. inversion pushed. reflexivity.
Qed.

Lemma traced_push_pending_node_preserves_line_cache :
  forall node before id after,
    traced_push_pending_node node before = (id, after) ->
    aws_lines (tws_state after) = aws_lines (tws_state before).
Proof.
  intros node before id after pushed.
  pose proof (traced_push_pending_node_erases node before id after pushed)
    as erased.
  eapply push_pending_node_preserves_line_cache. exact erased.
Qed.

Lemma certified_push_pending_children_preserves_line_cache :
  forall labels pending before children after,
    certified_push_pending_children labels pending before =
      Some (children, after) ->
    aws_lines (tws_state after) = aws_lines (tws_state before).
Proof.
  induction labels as [|label labels IH]; intros pending before children after
    built; destruct pending as [|child pending].
  - inversion built. reflexivity.
  - discriminate built.
  - discriminate built.
  - cbn [certified_push_pending_children] in built.
    destruct (traced_push_pending_node (pss_root child) before)
      as [childId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pending afterChild)
      as [[rest final]|] eqn:restRun; [|discriminate built].
    inversion built; subst children final.
    rewrite (IH pending afterChild rest after restRun).
    eapply traced_push_pending_node_preserves_line_cache.
    exact childRun.
Qed.

Theorem replay_certified_children_exact :
  forall labels pending before children after,
    certified_push_pending_children labels pending before =
      Some (children, after) ->
    replay_direct_children (source_semantic_forest_ids children)
      (source_semantic_forest_root_bytes children)
      (direct_replay_state_of_writer (tws_state before)) =
    Some (direct_replay_state_of_writer (tws_state after)).
Proof.
  intros labels pending before children after built.
  pose proof (certified_push_pending_children_root_layout_exact
    labels pending before children after built) as [layout positionExact].
  pose proof (certified_push_pending_children_preserves_line_cache
    labels pending before children after built) as cacheExact.
  destruct children as [|label child rest].
  - cbn [replay_direct_children source_semantic_forest_ids
      source_semantic_forest_root_bytes] in *.
    f_equal. unfold direct_replay_state_of_writer. f_equal;
      [lia|symmetry; exact cacheExact].
  - cbn [source_semantic_forest_root_layout] in layout.
    destruct layout as [firstExact restLayout].
    cbn [replay_direct_children source_semantic_forest_ids
      direct_replay_state_of_writer].
    rewrite firstExact, Nat.eqb_refl.
    f_equal. unfold direct_replay_state_of_writer. f_equal;
      [symmetry; exact positionExact|symmetry; exact cacheExact].
Qed.

Definition replay_direct_semantic_finish (line_hash : list bbyte -> nat)
    (node : direct_semantic_node_model)
    (nodes : list direct_semantic_node_model)
    (state : direct_line_replay_state) : option direct_line_replay_state :=
  match direct_children_encoded_len (dsm_child_ids node) nodes with
  | None => None
  | Some child_bytes =>
      let after_children := replay_direct_children
        (dsm_child_ids node) child_bytes state in
      match after_children with
      | None => None
      | Some children_state =>
          let after_line :=
            match dsm_line node with
            | None => Some children_state
            | Some line => replay_canonical_line line_hash line children_state
            end in
          match after_line with
          | None => None
          | Some line_state =>
              match dsm_wrapper node with
              | None => Some line_state
              | Some (wrapper, wrapper_len) =>
                  if Nat.eqb wrapper (dlr_position line_state) then
                    Some {| dlr_position := dlr_position line_state + wrapper_len;
                            dlr_lines := dlr_lines line_state |}
                  else None
              end
          end
      end
  end.

Fixpoint replay_source_semantic_tree (line_hash : list bbyte -> nat)
    (nodes : list direct_semantic_node_model) (tree : source_semantic_tree)
    (state : direct_line_replay_state) : option direct_line_replay_state :=
  match tree with
  | SourceSemanticNode _ _ _ _ _ children _ _ _ =>
      match replay_source_semantic_forest line_hash nodes children state with
      | None => None
      | Some after_descendants =>
          replay_direct_semantic_finish line_hash
            (source_semantic_erase tree) nodes after_descendants
      end
  end
with replay_source_semantic_forest (line_hash : list bbyte -> nat)
    (nodes : list direct_semantic_node_model) (forest : source_semantic_forest)
    (state : direct_line_replay_state) : option direct_line_replay_state :=
  match forest with
  | SourceSemanticNil => Some state
  | SourceSemanticCons _ child rest =>
      match replay_source_semantic_tree line_hash nodes child state with
      | None => None
      | Some after_child =>
          replay_source_semantic_forest line_hash nodes rest after_child
      end
  end.

Fixpoint source_semantic_forest_finalizes
    (pending : list pending_source_semantic_node)
    (forest : source_semantic_forest) : Prop :=
  match pending, forest with
  | [], SourceSemanticNil => True
  | pending_child :: pending_rest,
      SourceSemanticCons _ child rest =>
      child = finalize_pending_source_semantic_node
        (source_semantic_root_id child) pending_child /\
      source_semantic_forest_finalizes pending_rest rest
  | _, _ => False
  end.

Lemma certified_push_pending_children_finalizes :
  forall labels pending before forest after,
    certified_push_pending_children labels pending before =
      Some (forest, after) ->
    source_semantic_forest_finalizes pending forest.
Proof.
  induction labels as [|label labels IH]; intros pending before forest after
    pushed; destruct pending as [|pendingChild pendingRest].
  - inversion pushed. reflexivity.
  - discriminate pushed.
  - discriminate pushed.
  - cbn [certified_push_pending_children] in pushed.
    destruct (traced_push_pending_node (pss_root pendingChild) before)
      as [childId afterChild] eqn:childRun.
    destruct (certified_push_pending_children labels pendingRest afterChild)
      as [[rest final]|] eqn:restRun; [|discriminate pushed].
    inversion pushed; subst forest final.
    cbn [source_semantic_forest_finalizes source_semantic_root_id].
    split; [reflexivity|].
    eapply IH. exact restRun.
Qed.

Lemma replay_wrapper_matches_traced_push : forall node before wrapper_id after,
  traced_push_pending_node node before = (wrapper_id, after) ->
  (if Nat.eqb wrapper_id
        (dlr_position (direct_replay_state_of_writer (tws_state before)))
   then Some
     {| dlr_position :=
          dlr_position (direct_replay_state_of_writer (tws_state before)) +
          length (encode_pending_node wrapper_id node);
        dlr_lines :=
          dlr_lines (direct_replay_state_of_writer (tws_state before)) |}
   else None) =
  Some (direct_replay_state_of_writer (tws_state after)).
Proof.
  intros node before wrapperId after pushed.
  pose proof (traced_push_pending_node_erases node before wrapperId after pushed)
    as erased.
  pose proof (push_pending_node_position_exact node (tws_state before)
    wrapperId (tws_state after) erased) as [idExact positionExact].
  pose proof (traced_push_pending_node_preserves_line_cache
    node before wrapperId after pushed) as cacheExact.
  unfold direct_replay_state_of_writer. cbn.
  rewrite idExact, Nat.eqb_refl. f_equal. f_equal.
  - symmetry. exact positionExact.
  - symmetry. exact cacheExact.
Qed.

Lemma certified_build_pending_tree_children_exact :
  forall line_hash line value children before pending after pending_children
    after_descendants certified_children after_children,
    certified_build_pending_forest line_hash children before =
      Some (pending_children, after_descendants) ->
    certified_push_pending_children (forest_labels children) pending_children
      after_descendants = Some (certified_children, after_children) ->
    certified_build_pending_tree line_hash (WriterNode line value children)
      before = Some (pending, after) ->
    pss_children pending = certified_children.
Proof.
  intros lineHash line value children before pending after pendingChildren
    afterDescendants certifiedChildren afterChildren childrenRun childrenPush
    built.
  cbn [certified_build_pending_tree] in built.
  rewrite childrenRun, childrenPush in built.
  destruct line as [|first rest].
  - inversion built. reflexivity.
  - destruct (traced_add_or_reuse_line lineHash (first :: rest) afterChildren)
      as [lineId afterLine].
    destruct certifiedChildren as [|label child certifiedRest].
    + inversion built. reflexivity.
    + destruct (traced_push_pending_node
        (PendingBranch value
          (combine
            (source_semantic_forest_labels
              (SourceSemanticCons label child certifiedRest))
            (source_semantic_forest_ids
              (SourceSemanticCons label child certifiedRest)))) afterLine).
      inversion built. reflexivity.
Qed.

Theorem certified_source_writer_recursive_replay_exact : forall line_hash,
  (forall source before pending after,
      certified_build_pending_tree line_hash source before =
        Some (pending, after) ->
      forall root_id certificate nodes,
        source_nodes_index certificate nodes ->
        incl
          (source_semantic_preorder
            (finalize_pending_source_semantic_node root_id pending))
          (source_semantic_preorder certificate) ->
        replay_source_semantic_tree line_hash nodes
          (finalize_pending_source_semantic_node root_id pending)
          (direct_replay_state_of_writer (tws_state before)) =
        Some (direct_replay_state_of_writer (tws_state after))) /\
  (forall source before pending after,
      certified_build_pending_forest line_hash source before =
        Some (pending, after) ->
      forall semantic_forest certificate nodes,
        source_semantic_forest_finalizes pending semantic_forest ->
        source_nodes_index certificate nodes ->
        incl (source_semantic_forest_preorder semantic_forest)
          (source_semantic_preorder certificate) ->
        replay_source_semantic_forest line_hash nodes semantic_forest
          (direct_replay_state_of_writer (tws_state before)) =
        Some (direct_replay_state_of_writer (tws_state after))).
Proof.
  intro lineHash. apply writer_tree_forest_ind.
  - intros line value children childrenIH before pending after built rootId
      certificate nodes indexed included.
    pose proof built as builtOriginal.
    cbn [certified_build_pending_tree] in built.
    destruct (certified_build_pending_forest lineHash children before)
      as [[pendingChildren afterDescendants]|] eqn:childrenRun;
      [|discriminate built].
    destruct (certified_push_pending_children (forest_labels children)
      pendingChildren afterDescendants)
      as [[certifiedChildren afterChildren]|] eqn:childrenPush;
      [|discriminate built].
    assert (childrenFinalize :
      source_semantic_forest_finalizes pendingChildren certifiedChildren).
    { eapply certified_push_pending_children_finalizes. exact childrenPush. }
    pose proof (certified_build_pending_tree_children_exact lineHash line value
      children before pending after pendingChildren afterDescendants
      certifiedChildren afterChildren childrenRun childrenPush builtOriginal)
      as pendingChildrenExact.
    assert (childrenIncluded :
      incl (source_semantic_forest_preorder certifiedChildren)
        (source_semantic_preorder certificate)).
    { intros node present. apply included.
      rewrite <- pendingChildrenExact in present.
      cbn [finalize_pending_source_semantic_node source_semantic_preorder].
      now right. }
    pose proof (childrenIH before pendingChildren afterDescendants childrenRun
      certifiedChildren certificate nodes childrenFinalize indexed
      childrenIncluded) as descendantsReplay.
    pose proof (direct_children_encoded_len_source_forest_exact
      certificate nodes certifiedChildren indexed childrenIncluded)
      as childrenLength.
    pose proof (replay_certified_children_exact
      (forest_labels children) pendingChildren afterDescendants
      certifiedChildren afterChildren childrenPush) as childrenReplay.
    destruct line as [|first rest].
    + inversion built; subst pending after.
      cbn [replay_source_semantic_tree finalize_pending_source_semantic_node
        source_semantic_erase pss_children pss_line pss_value pss_line_id
        pss_line_hash pss_wrapper pss_root dsm_child_ids dsm_line dsm_wrapper].
      rewrite descendantsReplay.
      unfold replay_direct_semantic_finish.
      cbv beta iota zeta delta [dsm_child_ids dsm_line dsm_wrapper].
      rewrite childrenLength, childrenReplay.
      reflexivity.
    + destruct (traced_add_or_reuse_line lineHash (first :: rest) afterChildren)
        as [lineId afterLine] eqn:lineRun.
      pose proof (traced_add_or_reuse_line_erases lineHash (first :: rest)
        afterChildren lineId afterLine lineRun) as lineErased.
      pose proof (replay_canonical_line_matches_add_or_reuse
        lineHash (first :: rest) (tws_state afterChildren) lineId
        (tws_state afterLine) lineErased) as lineReplay.
      destruct certifiedChildren as [|label child certifiedRest].
      * inversion built; subst pending after.
        cbn [replay_source_semantic_tree finalize_pending_source_semantic_node
          source_semantic_erase pss_children pss_line pss_value pss_line_id
          pss_line_hash pss_wrapper pss_root dsm_child_ids dsm_line dsm_wrapper].
        rewrite descendantsReplay.
        unfold replay_direct_semantic_finish.
        cbv beta iota zeta delta [dsm_child_ids dsm_line dsm_wrapper].
        rewrite childrenLength, childrenReplay.
        fold (parsed_replayed_line lineId (first :: rest)).
        rewrite lineReplay.
        reflexivity.
      * set (wrapperNode := PendingBranch value
          ((label, source_semantic_root_id child) ::
           combine (source_semantic_forest_labels certifiedRest)
             (source_semantic_forest_ids certifiedRest))).
        cbn [source_semantic_forest_ids source_semantic_forest_labels combine]
          in built.
        fold wrapperNode in built.
        destruct (traced_push_pending_node wrapperNode afterLine)
          as [wrapperId afterWrapper] eqn:wrapperRun.
        inversion built; subst pending after.
        pose proof (replay_wrapper_matches_traced_push wrapperNode afterLine
          wrapperId afterWrapper wrapperRun) as wrapperReplay.
        cbn [replay_source_semantic_tree finalize_pending_source_semantic_node
          source_semantic_erase pss_children pss_line pss_value pss_line_id
          pss_line_hash pss_wrapper pss_root dsm_child_ids dsm_line dsm_wrapper].
        rewrite descendantsReplay.
        unfold replay_direct_semantic_finish.
        cbv beta iota zeta delta [dsm_child_ids dsm_line dsm_wrapper].
        rewrite childrenLength, childrenReplay.
        fold (parsed_replayed_line lineId (first :: rest)).
        rewrite lineReplay.
        fold wrapperNode.
        exact wrapperReplay.
  - intros before pending after built semanticForest certificate nodes
      finalizes indexed included.
    inversion built; subst pending after.
    destruct semanticForest; [reflexivity|contradiction].
  - intros label sourceChild childIH sourceRest restIH before pending after built
      semanticForest certificate nodes finalizes indexed included.
    cbn [certified_build_pending_forest] in built.
    destruct (certified_build_pending_tree lineHash sourceChild before)
      as [[pendingChild afterChild]|] eqn:childRun; [|discriminate built].
    destruct (certified_build_pending_forest lineHash sourceRest afterChild)
      as [[pendingRest final]|] eqn:restRun; [|discriminate built].
    inversion built; subst pending final.
    destruct semanticForest as [|semanticLabel semanticChild semanticRest];
      [contradiction|].
    cbn [source_semantic_forest_finalizes] in finalizes.
    destruct finalizes as [childExact restFinalizes].
    assert (childIncluded : incl (source_semantic_preorder semanticChild)
      (source_semantic_preorder certificate)).
    { intros node present. apply included.
      cbn [source_semantic_forest_preorder].
      apply in_or_app. left. exact present. }
    assert (restIncluded : incl
      (source_semantic_forest_preorder semanticRest)
      (source_semantic_preorder certificate)).
    { intros node present. apply included.
      cbn [source_semantic_forest_preorder].
      apply in_or_app. right. exact present. }
    pose proof (childIH before pendingChild afterChild childRun
      (source_semantic_root_id semanticChild) certificate nodes indexed)
      as childReplay.
    rewrite <- childExact in childReplay.
    specialize (childReplay childIncluded).
    pose proof (restIH afterChild pendingRest after restRun semanticRest
      certificate nodes restFinalizes indexed restIncluded) as restReplay.
    cbn [replay_source_semantic_forest].
    rewrite childReplay, restReplay. reflexivity.
Qed.

Inductive direct_replay_task : Type :=
| DirectReplayEnter (id : nat)
| DirectReplayFinish (id : nat).

Fixpoint direct_enter_tasks (children : list nat) : list direct_replay_task :=
  match children with
  | [] => []
  | child :: rest => DirectReplayEnter child :: direct_enter_tasks rest
  end.

Fixpoint replay_direct_semantic_plan (tokens : list nat)
    (line_hash : list bbyte -> nat) (nodes : list direct_semantic_node_model)
    (tasks : list direct_replay_task) (state : direct_line_replay_state)
    : option direct_line_replay_state :=
  match tasks with
  | [] => Some state
  | task :: task_rest =>
      match tokens with
      | [] => None
      | _ :: token_rest =>
          match task with
          | DirectReplayEnter id =>
              match find_direct_semantic_node id nodes with
              | None => None
              | Some node =>
                  replay_direct_semantic_plan token_rest line_hash nodes
                    (direct_enter_tasks (dsm_child_ids node) ++
                     DirectReplayFinish id :: task_rest) state
              end
          | DirectReplayFinish id =>
              match find_direct_semantic_node id nodes with
              | None => None
              | Some node =>
                  match replay_direct_semantic_finish line_hash node nodes state with
                  | None => None
                  | Some after =>
                      replay_direct_semantic_plan token_rest line_hash nodes
                        task_rest after
                  end
              end
          end
      end
  end.

Theorem semantic_certificate_recursive_replay_exact :
  forall line_hash source bytes certificate objects nodes,
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    source_nodes_index certificate nodes ->
    exists before_root,
      replay_source_semantic_tree line_hash nodes certificate
        {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |} =
      Some (direct_replay_state_of_writer (tws_state before_root)) /\
      dlr_position (direct_replay_state_of_writer (tws_state before_root)) =
        source_semantic_root_id certificate.
Proof.
  intros lineHash source bytes certificate objects nodes encoded indexed.
  set (initial := {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash source initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  pose proof (proj1 (certified_source_writer_recursive_replay_exact lineHash)
    source initial pending beforeRoot built rootId
    (finalize_pending_source_semantic_node rootId pending) nodes indexed)
    as replayed.
  specialize (replayed (incl_refl _)).
  assert (initialExact :
    direct_replay_state_of_writer (tws_state initial) =
      {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |}) by reflexivity.
  rewrite initialExact in replayed.
  exists beforeRoot. split; [exact replayed|].
  pose proof (traced_push_pending_node_erases (pss_root pending) beforeRoot
    rootId final rootRun) as erased.
  pose proof (proj1 (push_pending_node_position_exact (pss_root pending)
    (tws_state beforeRoot) rootId (tws_state final) erased)) as rootExact.
  unfold direct_replay_state_of_writer. cbn [source_semantic_root_id].
  symmetry. exact rootExact.
Qed.

Lemma replay_direct_semantic_plan_enter_step :
  forall token tokens line_hash nodes id node tasks state,
    find_direct_semantic_node id nodes = Some node ->
    replay_direct_semantic_plan (token :: tokens) line_hash nodes
      (DirectReplayEnter id :: tasks) state =
    replay_direct_semantic_plan tokens line_hash nodes
      (direct_enter_tasks (dsm_child_ids node) ++
       DirectReplayFinish id :: tasks) state.
Proof.
  intros token tokens lineHash nodes id node tasks state found.
  cbn [replay_direct_semantic_plan]. rewrite found. reflexivity.
Qed.

Lemma replay_direct_semantic_plan_finish_step :
  forall token tokens line_hash nodes id node tasks state after,
    find_direct_semantic_node id nodes = Some node ->
    replay_direct_semantic_finish line_hash node nodes state = Some after ->
    replay_direct_semantic_plan (token :: tokens) line_hash nodes
      (DirectReplayFinish id :: tasks) state =
    replay_direct_semantic_plan tokens line_hash nodes tasks after.
Proof.
  intros token tokens lineHash nodes id node tasks state after found finished.
  cbn [replay_direct_semantic_plan]. rewrite found, finished. reflexivity.
Qed.

Lemma repeat_two_successor_decompose : forall (token : nat) count tail,
  repeat token (2 * S count) ++ tail =
    token :: (repeat token (2 * count) ++ token :: tail).
Proof.
  intros token count tail.
  replace (2 * S count) with (S (2 * count + 1)) by lia.
  cbn [repeat]. rewrite repeat_app. cbn [repeat].
  change (token :: ((repeat token (2 * count) ++ [token]) ++ tail) =
    token :: (repeat token (2 * count) ++ token :: tail)).
  f_equal. rewrite <- app_assoc. reflexivity.
Qed.

Lemma repeat_two_sum_decompose : forall (token : nat) left right tail,
  repeat token (2 * (left + right)) ++ tail =
    repeat token (2 * left) ++ repeat token (2 * right) ++ tail.
Proof.
  intros token left right tail.
  replace (2 * (left + right)) with (2 * left + 2 * right) by lia.
  rewrite repeat_app, app_assoc. reflexivity.
Qed.

Theorem replay_plan_executes_source_semantics :
  forall line_hash certificate nodes,
    source_nodes_index certificate nodes ->
    (forall tree state after tail_tokens tail_tasks,
      incl (source_semantic_preorder tree)
        (source_semantic_preorder certificate) ->
      replay_source_semantic_tree line_hash nodes tree state = Some after ->
      replay_direct_semantic_plan
        (repeat 0 (2 * source_semantic_node_count tree) ++ tail_tokens)
        line_hash nodes
        (DirectReplayEnter (source_semantic_root_id tree) :: tail_tasks) state =
      replay_direct_semantic_plan tail_tokens line_hash nodes tail_tasks after) /\
    (forall forest state after tail_tokens tail_tasks,
      incl (source_semantic_forest_preorder forest)
        (source_semantic_preorder certificate) ->
      replay_source_semantic_forest line_hash nodes forest state = Some after ->
      replay_direct_semantic_plan
        (repeat 0 (2 * source_semantic_forest_node_count forest) ++ tail_tokens)
        line_hash nodes
        (direct_enter_tasks (source_semantic_forest_ids forest) ++ tail_tasks)
        state =
      replay_direct_semantic_plan tail_tokens line_hash nodes tail_tasks after).
Proof.
  intros lineHash certificate nodes indexed.
  apply source_semantic_tree_forest_ind.
  - intros rootId rootLen rootNode line value children childrenIH lineId
      lineHashRecorded wrapper state after tailTokens tailTasks included replayed.
    cbn [replay_source_semantic_tree] in replayed.
    destruct (replay_source_semantic_forest lineHash nodes children state)
      as [afterDescendants|] eqn:descendantsReplay;
      [|discriminate replayed].
    assert (currentPresent : In
      (SourceSemanticNode rootId rootLen rootNode line value children lineId
        lineHashRecorded wrapper)
      (source_semantic_preorder certificate)).
    { apply included. cbn [source_semantic_preorder]. now left. }
    pose proof (indexed
      (SourceSemanticNode rootId rootLen rootNode line value children lineId
        lineHashRecorded wrapper) currentPresent) as found.
    assert (childrenIncluded : incl
      (source_semantic_forest_preorder children)
      (source_semantic_preorder certificate)).
    { intros node present. apply included.
      cbn [source_semantic_preorder]. now right. }
    cbn [source_semantic_node_count].
    rewrite repeat_two_successor_decompose.
    cbn [source_semantic_root_id] in found |- *.
    rewrite (replay_direct_semantic_plan_enter_step 0
      (repeat 0 (2 * source_semantic_forest_node_count children) ++
       0 :: tailTokens) lineHash nodes rootId
      (source_semantic_erase
        (SourceSemanticNode rootId rootLen rootNode line value children lineId
          lineHashRecorded wrapper)) tailTasks state found).
    cbn [source_semantic_erase dsm_child_ids].
    rewrite (childrenIH state afterDescendants (0 :: tailTokens)
      (DirectReplayFinish rootId :: tailTasks) childrenIncluded
      descendantsReplay).
    rewrite (replay_direct_semantic_plan_finish_step 0 tailTokens lineHash
      nodes rootId
      (source_semantic_erase
        (SourceSemanticNode rootId rootLen rootNode line value children lineId
          lineHashRecorded wrapper)) tailTasks afterDescendants after found
      replayed).
    reflexivity.
  - intros state after tailTokens tailTasks included replayed.
    inversion replayed; subst after. reflexivity.
  - intros label child childIH rest restIH state after tailTokens tailTasks
      included replayed.
    cbn [replay_source_semantic_forest] in replayed.
    destruct (replay_source_semantic_tree lineHash nodes child state)
      as [afterChild|] eqn:childReplay; [|discriminate replayed].
    assert (childIncluded : incl (source_semantic_preorder child)
      (source_semantic_preorder certificate)).
    { intros node present. apply included.
      cbn [source_semantic_forest_preorder].
      apply in_or_app. left. exact present. }
    assert (restIncluded : incl (source_semantic_forest_preorder rest)
      (source_semantic_preorder certificate)).
    { intros node present. apply included.
      cbn [source_semantic_forest_preorder].
      apply in_or_app. right. exact present. }
    cbn [source_semantic_forest_node_count].
    rewrite repeat_two_sum_decompose.
    cbn [source_semantic_forest_ids direct_enter_tasks].
    cbn [app].
    rewrite (childIH state afterChild
      (repeat 0 (2 * source_semantic_forest_node_count rest) ++ tailTokens)
      (direct_enter_tasks (source_semantic_forest_ids rest) ++ tailTasks)
      childIncluded childReplay).
    rewrite (restIH afterChild after tailTokens tailTasks restIncluded replayed).
    reflexivity.
Qed.

Lemma replay_direct_semantic_plan_tokens_extensional :
  forall left right line_hash nodes tasks state,
    length left = length right ->
    replay_direct_semantic_plan left line_hash nodes tasks state =
    replay_direct_semantic_plan right line_hash nodes tasks state.
Proof.
  induction left as [|leftToken leftRest IH]; intros right lineHash nodes tasks
    state sameLength; destruct right as [|rightToken rightRest];
    try discriminate sameLength.
  - reflexivity.
  - destruct tasks as [|task taskRest]; [reflexivity|].
    destruct task as [id|id].
    + cbn [replay_direct_semantic_plan].
      destruct (find_direct_semantic_node id nodes); [apply IH|reflexivity].
      cbn [length] in sameLength. lia.
    + cbn [replay_direct_semantic_plan].
      destruct (find_direct_semantic_node id nodes) as [node|];
        [|reflexivity].
      destruct (replay_direct_semantic_finish lineHash node nodes state);
        [apply IH|reflexivity].
      cbn [length] in sameLength. lia.
Qed.

Lemma replay_direct_semantic_plan_no_tasks : forall tokens line_hash nodes state,
  replay_direct_semantic_plan tokens line_hash nodes [] state = Some state.
Proof. intros [|token tokens] lineHash nodes state; reflexivity. Qed.

Theorem semantic_certificate_pda_replay_exact :
  forall line_hash source bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    exists replayed,
      replay_direct_semantic_plan (seq 0 (2 * length bytes)) line_hash
        (rev (source_semantic_erased_preorder certificate))
        [DirectReplayEnter (source_semantic_root_id certificate)]
        {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |} =
      Some replayed /\
      dlr_position replayed = source_semantic_root_id certificate.
Proof.
  intros lineHash source bytes certificate objects encoded.
  pose proof (semantic_certificate_preorder_physical_ids_nodup
    lineHash source bytes certificate objects encoded) as physicalUnique.
  pose proof (reversed_source_nodes_index certificate physicalUnique) as indexed.
  destruct (semantic_certificate_recursive_replay_exact
    lineHash source bytes certificate objects
    (rev (source_semantic_erased_preorder certificate)) encoded indexed)
    as [beforeRoot [recursiveReplay rootPosition]].
  pose proof (proj1 (semantic_certificate_node_count_is_byte_bounded
    lineHash source bytes certificate objects encoded)) as countBound.
  set (extraTokens := repeat 0
    (2 * (length bytes - source_semantic_node_count certificate))).
  pose proof (proj1 (replay_plan_executes_source_semantics lineHash certificate
    (rev (source_semantic_erased_preorder certificate)) indexed)
    certificate {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |}
    (direct_replay_state_of_writer (tws_state beforeRoot)) extraTokens []
    (incl_refl _) recursiveReplay) as executed.
  rewrite replay_direct_semantic_plan_no_tasks in executed.
  assert (sameLength :
    length (seq 0 (2 * length bytes)) =
    length
      (repeat 0 (2 * source_semantic_node_count certificate) ++ extraTokens)).
  { rewrite length_seq, length_app, !repeat_length.
    unfold extraTokens. rewrite repeat_length. lia. }
  pose proof (replay_direct_semantic_plan_tokens_extensional
    (seq 0 (2 * length bytes))
    (repeat 0 (2 * source_semantic_node_count certificate) ++ extraTokens)
    lineHash (rev (source_semantic_erased_preorder certificate))
    [DirectReplayEnter (source_semantic_root_id certificate)]
    {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |} sameLength)
    as tokenExact.
  exists (direct_replay_state_of_writer (tws_state beforeRoot)). split.
  - rewrite tokenExact. exact executed.
  - exact rootPosition.
Qed.

(** The final root is appended after every descendant object and therefore
    closes the semantic arena exactly.  This is the producer-side boundary
    fact used by the canonical-image validator; it does not depend on parsing
    the emitted image back into a tree. *)
Theorem semantic_certificate_root_closes_arena :
  forall line_hash source bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    source_semantic_root_id certificate +
      source_semantic_root_encoded_len certificate =
    ACT_HEADER_LEN + length (physical_writer_bytes objects).
Proof.
  intros lineHash source bytes certificate objects encoded.
  set (initial := {| tws_state := empty_writer_state; tws_objects := [] |}).
  unfold encode_writer_tree_with_semantic_certificate in encoded.
  fold initial in encoded.
  destruct (certified_build_pending_tree lineHash source initial)
    as [[pending beforeRoot]|] eqn:built; [|discriminate encoded].
  destruct (traced_push_pending_node (pss_root pending) beforeRoot)
    as [rootId final] eqn:rootRun.
  inversion encoded; subst bytes certificate objects.
  assert (initialInvariant : traced_writer_full_invariant initial)
    by (repeat split; reflexivity).
  pose proof (proj1 (certified_source_writer_preserves_full_invariant lineHash)
    source initial pending beforeRoot built initialInvariant) as beforeInvariant.
  pose proof (traced_push_pending_node_preserves_full_invariant
    (pss_root pending) beforeRoot beforeInvariant) as finalInvariant.
  rewrite rootRun in finalInvariant. cbn in finalInvariant.
  pose proof (traced_push_pending_node_erases (pss_root pending) beforeRoot
    rootId final rootRun) as erased.
  pose proof (push_pending_node_position_exact (pss_root pending)
    (tws_state beforeRoot) rootId (tws_state final) erased)
    as [rootExact positionExact].
  unfold traced_writer_full_invariant, traced_writer_invariant,
    traced_writer_consistent in finalInvariant.
  destruct finalInvariant as [[arenaExact layout] cache].
  cbn [source_semantic_root_id source_semantic_root_encoded_len
    finalize_pending_source_semantic_node].
  rewrite <- arenaExact.
  unfold writer_position in rootExact, positionExact.
  rewrite rootExact.
  symmetry. exact positionExact.
Qed.

Theorem semantic_certificate_image_boundaries :
  forall line_hash source bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    length bytes =
      ACT_HEADER_LEN + length (physical_writer_bytes objects) +
        length ACT_TRAILER /\
    skipn (ACT_HEADER_LEN + length (physical_writer_bytes objects)) bytes =
      ACT_TRAILER.
Proof.
  intros lineHash source bytes certificate objects encoded.
  pose proof (proj1 (semantic_certificate_has_exact_image_and_layout
    lineHash source bytes certificate objects encoded)) as bytesExact.
  rewrite bytesExact. split.
  - pose proof (act_header_prefix_length
      (source_semantic_root_id certificate)) as headerLength.
    rewrite length_app in headerLength.
    repeat rewrite length_app. lia.
  - set (prefix := ACT_MAGIC ++
      le_fixed 8 (source_semantic_root_id certificate) ++
      physical_writer_bytes objects).
    assert (prefixLength : length prefix =
      ACT_HEADER_LEN + length (physical_writer_bytes objects)).
    { pose proof (act_header_prefix_length
        (source_semantic_root_id certificate)) as headerLength.
      rewrite length_app in headerLength.
      unfold prefix. repeat rewrite length_app. lia. }
    change (skipn (ACT_HEADER_LEN + length (physical_writer_bytes objects))
      (prefix ++ ACT_TRAILER) = ACT_TRAILER).
    rewrite <- prefixLength, skipn_app, skipn_all, Nat.sub_diag.
    reflexivity.
Qed.

Theorem canonical_semantic_certificate_header_and_root_bounds :
  forall line_hash source bytes certificate objects,
    canonical_writer_tree source ->
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    parse_act_header bytes = Some
      (source_semantic_root_id certificate,
       physical_writer_bytes objects ++ ACT_TRAILER) /\
    ACT_HEADER_LEN <= source_semantic_root_id certificate /\
    source_semantic_root_id certificate <
      ACT_HEADER_LEN + length (physical_writer_bytes objects).
Proof.
  intros lineHash source bytes certificate objects canonical encoded.
  pose proof (proj1 (semantic_certificate_has_exact_image_and_layout
    lineHash source bytes certificate objects encoded)) as bytesExact.
  pose proof (canonical_writer_semantic_certificate_image_bound
    lineHash source bytes certificate objects canonical encoded)
    as imageBound.
  pose proof (canonical_semantic_certificate_has_all_direct_domains
    lineHash source bytes certificate objects canonical encoded)
    as allDomains.
  destruct certificate as [rootId rootLen rootNode line value children lineId
    lineHashRecorded wrapper].
  cbn [source_semantic_tree_direct_domains] in allDomains.
  destruct allDomains as [rootDomain descendantsDomains].
  cbn [source_semantic_direct_domain] in rootDomain.
  destruct rootDomain as [afterHeader [rootPositive [rootFits remaining]]].
  cbn [source_semantic_root_id] in bytesExact |- *.
  split.
  - rewrite bytesExact. apply exact_header_parses_root_slot. lia.
  - split; [exact afterHeader|]. lia.
Qed.

Record validated_direct_image : Type := {
  vdi_root : nat;
  vdi_arena_end : nat;
  vdi_nodes : list direct_semantic_node_model
}.

Definition validate_direct_canonical_image (line_hash : list bbyte -> nat)
    (image : list bbyte) : option validated_direct_image :=
  if ACT_HEADER_LEN + length ACT_TRAILER <=? length image then
    let arena_end := length image - length ACT_TRAILER in
    match parse_act_header image with
    | None => None
    | Some (root, _) =>
        if (ACT_HEADER_LEN <=? root) && (root <? arena_end) &&
           (if list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER
            then true else false) then
          match discover_direct_semantic_nodes (seq 0 (length image)) arena_end image
            {| dsd_pending := [root]; dsd_nodes := [];
               dsd_seen_physical_ids := [] |} with
          | None => None
          | Some discovered =>
              match find_direct_semantic_node root (dsd_nodes discovered) with
              | None => None
              | Some root_node =>
                  match replay_direct_semantic_plan
                    (seq 0 (2 * length image)) line_hash
                    (dsd_nodes discovered) [DirectReplayEnter root]
                    {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |} with
                  | None => None
                  | Some replayed =>
                      if Nat.eqb (dlr_position replayed) root &&
                         Nat.eqb (root + dsm_encoded_len root_node) arena_end
                      then Some {| vdi_root := root; vdi_arena_end := arena_end;
                                   vdi_nodes := dsd_nodes discovered |}
                      else None
                  end
              end
          end
        else None
    end
  else None.

Definition direct_canonical_image_accepts (line_hash : list bbyte -> nat)
    (image : list bbyte) : bool :=
  match validate_direct_canonical_image line_hash image with
  | Some _ => true
  | None => false
  end.

(** End-to-end completeness for the independently structured validator.  The
    proof composes producer boundary facts, direct parser discovery, exact
    source-node indexing, and the iterative replay PDA; no recursive decoder
    result is assumed by the validator. *)
Theorem validate_accepts_semantic_certificate :
  forall line_hash source bytes certificate objects,
    canonical_writer_tree source ->
    encode_writer_tree_with_semantic_certificate line_hash source =
      Some (bytes, certificate, objects) ->
    validate_direct_canonical_image line_hash bytes =
      Some {| vdi_root := source_semantic_root_id certificate;
              vdi_arena_end :=
                ACT_HEADER_LEN + length (physical_writer_bytes objects);
              vdi_nodes :=
                rev (source_semantic_erased_preorder certificate) |}.
Proof.
  intros lineHash source bytes certificate objects canonical encoded.
  set (arenaEnd := ACT_HEADER_LEN + length (physical_writer_bytes objects)).
  pose proof (semantic_certificate_image_boundaries
    lineHash source bytes certificate objects encoded)
    as [lengthExact trailerExact].
  pose proof (canonical_semantic_certificate_header_and_root_bounds
    lineHash source bytes certificate objects canonical encoded)
    as [headerExact [rootAfterHeader rootBeforeEnd]].
  pose proof (semantic_certificate_root_closes_arena
    lineHash source bytes certificate objects encoded) as rootCloses.
  destruct (canonical_writer_semantic_certificate_discovery_exact
    lineHash source bytes certificate objects canonical encoded)
    as [finalSeen discovered].
  pose proof (semantic_certificate_preorder_physical_ids_nodup
    lineHash source bytes certificate objects encoded) as physicalUnique.
  pose proof (reversed_source_nodes_index certificate physicalUnique) as indexed.
  pose proof (indexed certificate (source_semantic_root_in_preorder certificate))
    as rootFound.
  destruct (semantic_certificate_pda_replay_exact
    lineHash source bytes certificate objects encoded)
    as [replayed [replayedExact replayPosition]].
  assert (arenaSubtract : length bytes - length ACT_TRAILER = arenaEnd).
  { unfold arenaEnd. lia. }
  assert (minimumGate :
    (ACT_HEADER_LEN + length ACT_TRAILER <=? length bytes) = true).
  { apply Nat.leb_le. unfold arenaEnd in lengthExact. lia. }
  assert (rootAfterHeaderGate :
    (ACT_HEADER_LEN <=? source_semantic_root_id certificate) = true)
    by (apply Nat.leb_le; exact rootAfterHeader).
  assert (rootBeforeEndGate :
    (source_semantic_root_id certificate <? arenaEnd) = true).
  { apply Nat.ltb_lt. unfold arenaEnd. exact rootBeforeEnd. }
  assert (rootEncodedLength :
    dsm_encoded_len (source_semantic_erase certificate) =
      source_semantic_root_encoded_len certificate)
    by (destruct certificate; reflexivity).
  unfold validate_direct_canonical_image.
  rewrite minimumGate, arenaSubtract, headerExact.
  rewrite rootAfterHeaderGate, rootBeforeEndGate.
  fold arenaEnd in trailerExact.
  rewrite trailerExact.
  destruct (list_eq_dec Nat.eq_dec ACT_TRAILER ACT_TRAILER)
    as [trailerEqual|trailerDifferent]; [|contradiction].
  fold arenaEnd in discovered.
  rewrite discovered.
  cbn [dsd_nodes].
  rewrite rootFound, replayedExact, replayPosition, Nat.eqb_refl.
  rewrite rootEncodedLength.
  assert (rootCloseGate :
    Nat.eqb
      (source_semantic_root_id certificate +
       source_semantic_root_encoded_len certificate) arenaEnd = true).
  { apply Nat.eqb_eq. unfold arenaEnd. exact rootCloses. }
  rewrite rootCloseGate. reflexivity.
Qed.

(** The semantic certificate is ghost evidence rather than a second output
    format.  Consequently the completeness result transfers to every byte
    image produced by the ordinary writer API. *)
Theorem validate_accepts_every_canonical_writer_output :
  forall line_hash source bytes,
    canonical_writer_tree source ->
    encode_writer_tree line_hash source = Some bytes ->
    exists certificate objects,
      encode_writer_tree_with_semantic_certificate line_hash source =
        Some (bytes, certificate, objects) /\
      validate_direct_canonical_image line_hash bytes =
        Some {| vdi_root := source_semantic_root_id certificate;
                vdi_arena_end :=
                  ACT_HEADER_LEN + length (physical_writer_bytes objects);
                vdi_nodes :=
                  rev (source_semantic_erased_preorder certificate) |}.
Proof.
  intros lineHash source bytes canonical ordinaryEncoded.
  destruct (encode_writer_tree_with_semantic_certificate_is_total
    lineHash source) as [certificateBytes [certificate [objects certified]]].
  pose proof (proj1 (semantic_certificate_erases_and_reifies_exactly
    lineHash source certificateBytes certificate objects certified))
    as traced.
  pose proof (encode_writer_tree_with_certificate_erases_exactly
    lineHash source certificateBytes objects traced) as ordinaryCertified.
  rewrite ordinaryEncoded in ordinaryCertified.
  inversion ordinaryCertified; subst certificateBytes.
  exists certificate, objects. split; [exact certified|].
  eapply validate_accepts_semantic_certificate; eassumption.
Qed.

Theorem validated_direct_image_is_sound : forall line_hash image validated,
  validate_direct_canonical_image line_hash image = Some validated ->
  skipn (vdi_arena_end validated) image = ACT_TRAILER /\
  exists discovered root_node replayed,
    discover_direct_semantic_nodes (seq 0 (length image))
      (vdi_arena_end validated) image
      {| dsd_pending := [vdi_root validated]; dsd_nodes := [];
         dsd_seen_physical_ids := [] |} = Some discovered /\
    vdi_nodes validated = dsd_nodes discovered /\
    find_direct_semantic_node (vdi_root validated) (vdi_nodes validated) =
      Some root_node /\
    replay_direct_semantic_plan (seq 0 (2 * length image)) line_hash
      (vdi_nodes validated) [DirectReplayEnter (vdi_root validated)]
      {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |} = Some replayed /\
    dlr_position replayed = vdi_root validated /\
    vdi_root validated + dsm_encoded_len root_node =
      vdi_arena_end validated.
Proof.
  intros line_hash image validated accepted.
  unfold validate_direct_canonical_image in accepted.
  destruct (ACT_HEADER_LEN + length ACT_TRAILER <=? length image)
    eqn:minimum; [|discriminate].
  remember (length image - length ACT_TRAILER) as arena_end.
  destruct (parse_act_header image) as [[root arena]|] eqn:header;
    [|discriminate].
  destruct ((ACT_HEADER_LEN <=? root) && (root <? arena_end) &&
    (if list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER
     then true else false)) eqn:headerGate; [|discriminate].
  destruct (discover_direct_semantic_nodes (seq 0 (length image)) arena_end image
    {| dsd_pending := [root]; dsd_nodes := [];
       dsd_seen_physical_ids := [] |})
    as [discovered|] eqn:discoveredRun; [|discriminate].
  destruct (find_direct_semantic_node root (dsd_nodes discovered))
    as [rootNode|] eqn:rootFound; [|discriminate].
  destruct (replay_direct_semantic_plan (seq 0 (2 * length image)) line_hash
    (dsd_nodes discovered) [DirectReplayEnter root]
    {| dlr_position := ACT_HEADER_LEN; dlr_lines := [] |})
    as [replayed|] eqn:replayRun; [|discriminate].
  destruct (Nat.eqb (dlr_position replayed) root &&
    Nat.eqb (root + dsm_encoded_len rootNode) arena_end)
    eqn:finalGate; [|discriminate].
  inversion accepted; subst validated; clear accepted.
  apply andb_true_iff in finalGate as [positionExact rootCloses].
  apply Nat.eqb_eq in positionExact, rootCloses.
  apply andb_true_iff in headerGate as [_ trailerGate].
  destruct (list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER)
    as [trailerExact|trailerWrong]; [|discriminate].
  split; [exact trailerExact|].
  exists discovered, rootNode, replayed.
  repeat split; assumption || reflexivity.
Qed.

Theorem direct_validator_accepts_exact_three_way_fixture :
  direct_canonical_image_accepts fixture_line_hash ACT_THREE_WAY_IMAGE = true.
Proof. vm_compute. reflexivity. Qed.

Theorem direct_validator_accepts_exact_line_fixture :
  direct_canonical_image_accepts fixture_line_hash ACT_LINE_IMAGE = true.
Proof. vm_compute. reflexivity. Qed.

Theorem direct_validator_accepts_exact_equal_line_reuse_fixture :
  direct_canonical_image_accepts forced_collision_hash
    (match encode_writer_tree forced_collision_hash same_line_writer_tree with
     | Some image => image | None => [] end) = true.
Proof. vm_compute. reflexivity. Qed.

Theorem direct_validator_accepts_exact_aba_collision_fixture :
  direct_canonical_image_accepts forced_collision_hash
    (match encode_writer_tree forced_collision_hash aba_colliding_line_writer_tree with
     | Some image => image | None => [] end) = true.
Proof. vm_compute. reflexivity. Qed.

Definition NONCANONICAL_UNARY_BRANCH_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 18 ++ [64;0;1;2;97] ++ ACT_TRAILER.

Definition NONCANONICAL_EMPTY_LINE_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 17 ++ [0;192;0;1] ++ ACT_TRAILER.

Definition NONCANONICAL_LINE_VALUE_AND_CHILD_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 20 ++
    [1;120;64;0;193;0;2;4] ++ ACT_TRAILER.

Definition NONCANONICAL_LINE_CHILD_IS_LINE_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 21 ++
    [1;120;192;0;2;129;3;5] ++ ACT_TRAILER.

Definition NONCANONICAL_EMPTY_WRAPPER_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 19 ++ [1;120;0;129;1;3] ++ ACT_TRAILER.

Definition NONCANONICAL_CHILD_BEFORE_ARENA_IMAGE : list bbyte :=
  ACT_MAGIC ++ le_fixed 8 16 ++ [65;0;16;97] ++ ACT_TRAILER.

Theorem direct_validator_rejects_production_shape_violations :
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_UNARY_BRANCH_IMAGE = false /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_EMPTY_LINE_IMAGE = false /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_LINE_VALUE_AND_CHILD_IMAGE = false /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_LINE_CHILD_IS_LINE_IMAGE = false /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_EMPTY_WRAPPER_IMAGE = false /\
  direct_canonical_image_accepts fixture_line_hash
    NONCANONICAL_CHILD_BEFORE_ARENA_IMAGE = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Definition add_referenced_line (node : parsed_node) (arena_end : nat)
    (image : list bbyte) (lines : list parsed_line_object)
    : option (list parsed_line_object) :=
  match pn_edge node with
  | ParsedBranch _ _ => Some lines
  | ParsedLine _ None => None
  | ParsedLine _ (Some line_id) =>
      if existsb (parsed_line_id_eq line_id) lines then Some lines
      else
        match parse_line_object line_id arena_end image with
        | None => None
        | Some line => Some (line :: lines)
        end
  end.

Fixpoint discover_physical_objects (tokens : list nat) (arena_end : nat)
    (image : list bbyte) (state : physical_discovery)
    : option physical_discovery :=
  match pd_pending state with
  | [] => Some state
  | id :: pending_rest =>
      match tokens with
      | [] => None
      | _ :: token_rest =>
          if existsb (parsed_node_id_eq id) (pd_nodes state) then None
          else if (ACT_HEADER_LEN <=? id) && (id <? arena_end) then
            match parse_node_record id (skipn id image) with
            | None => None
            | Some (node, _) =>
                if (0 <? pn_encoded_len node) &&
                   (id + pn_encoded_len node <=? arena_end) then
                  match branch_references node arena_end image,
                        add_referenced_line node arena_end image
                          (pd_lines state) with
                  | Some references, Some lines =>
                      if forallb (fun child => child <? id) references then
                        discover_physical_objects token_rest arena_end image
                          {| pd_pending := references ++ pending_rest;
                             pd_nodes := node :: pd_nodes state;
                             pd_lines := lines |}
                      else None
                  | _, _ => None
                  end
                else None
            end
          else None
      end
  end.

Definition object_span := (nat * nat)%type.

Definition node_span (node : parsed_node) : object_span :=
  (pn_id node, pn_id node + pn_encoded_len node).

Definition line_span (line : parsed_line_object) : object_span :=
  (plo_id line, plo_id line + plo_encoded_len line).

Fixpoint insert_span (span : object_span) (spans : list object_span)
    : list object_span :=
  match spans with
  | [] => [span]
  | head :: rest =>
      if fst span <=? fst head then span :: spans
      else head :: insert_span span rest
  end.

Fixpoint sort_spans (spans : list object_span) : list object_span :=
  match spans with
  | [] => []
  | span :: rest => insert_span span (sort_spans rest)
  end.

Fixpoint spans_cover_exactly (expected arena_end : nat)
    (spans : list object_span) : bool :=
  match spans with
  | [] => Nat.eqb expected arena_end
  | (start, finish) :: rest =>
      Nat.eqb start expected && (start <? finish) &&
      spans_cover_exactly finish arena_end rest
  end.

Definition physical_spans (state : physical_discovery) : list object_span :=
  map node_span (pd_nodes state) ++ map line_span (pd_lines state).

Record validated_physical_image : Type := {
  vpi_root : nat;
  vpi_arena_end : nat;
  vpi_nodes : list parsed_node;
  vpi_lines : list parsed_line_object
}.

Definition validate_physical_image (image : list bbyte)
    : option validated_physical_image :=
  if ACT_HEADER_LEN + length ACT_TRAILER <=? length image then
    let arena_end := length image - length ACT_TRAILER in
    match parse_act_header image with
    | None => None
    | Some (root, _) =>
        if (ACT_HEADER_LEN <=? root) && (root <? arena_end) &&
           (if list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER
            then true else false) then
          match discover_physical_objects (seq 0 (length image)) arena_end image
            {| pd_pending := [root]; pd_nodes := []; pd_lines := [] |} with
          | None => None
          | Some state =>
              match find (parsed_node_id_eq root) (pd_nodes state) with
              | None => None
              | Some root_node =>
                  if Nat.eqb (root + pn_encoded_len root_node) arena_end &&
                     spans_cover_exactly ACT_HEADER_LEN arena_end
                       (sort_spans (physical_spans state))
                  then Some {| vpi_root := root;
                               vpi_arena_end := arena_end;
                               vpi_nodes := pd_nodes state;
                               vpi_lines := pd_lines state |}
                  else None
              end
          end
        else None
    end
  else None.

Definition physical_image_accepts (image : list bbyte) : bool :=
  match validate_physical_image image with Some _ => true | None => false end.

(** These witnesses establish causally that exact span coverage is weaker than
    production semantic admission.  Each malformed image covers its arena
    perfectly, yet violates a distinct [DirectSemanticNode] rule. *)
Theorem span_only_validator_admits_the_shape_counterexamples :
  physical_image_accepts NONCANONICAL_UNARY_BRANCH_IMAGE = true /\
  physical_image_accepts NONCANONICAL_EMPTY_LINE_IMAGE = true /\
  physical_image_accepts NONCANONICAL_LINE_VALUE_AND_CHILD_IMAGE = true /\
  physical_image_accepts NONCANONICAL_LINE_CHILD_IS_LINE_IMAGE = true /\
  physical_image_accepts NONCANONICAL_EMPTY_WRAPPER_IMAGE = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Theorem validated_physical_image_is_sound : forall image validated,
  validate_physical_image image = Some validated ->
  skipn (vpi_arena_end validated) image = ACT_TRAILER /\
  exists root_node,
    In root_node (vpi_nodes validated) /\
    pn_id root_node = vpi_root validated /\
    vpi_root validated + pn_encoded_len root_node = vpi_arena_end validated /\
    spans_cover_exactly ACT_HEADER_LEN (vpi_arena_end validated)
      (sort_spans
        (map node_span (vpi_nodes validated) ++
         map line_span (vpi_lines validated))) = true.
Proof.
  intros image validated accepted.
  unfold validate_physical_image in accepted.
  destruct (ACT_HEADER_LEN + length ACT_TRAILER <=? length image)
    eqn:minimum; [|discriminate].
  remember (length image - length ACT_TRAILER) as arena_end.
  destruct (parse_act_header image) as [[root arena]|] eqn:header;
    [|discriminate].
  destruct ((ACT_HEADER_LEN <=? root) && (root <? arena_end) &&
    (if list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER
     then true else false)) eqn:headerGate; [|discriminate].
  destruct (discover_physical_objects (seq 0 (length image)) arena_end image
    {| pd_pending := [root]; pd_nodes := []; pd_lines := [] |})
    as [state|] eqn:discovered; [|discriminate].
  destruct (find (parsed_node_id_eq root) (pd_nodes state))
    as [root_node|] eqn:rootFound; [|discriminate].
  destruct (Nat.eqb (root + pn_encoded_len root_node) arena_end &&
    spans_cover_exactly ACT_HEADER_LEN arena_end
      (sort_spans (physical_spans state))) eqn:finalGate; [|discriminate].
  inversion accepted; subst validated; clear accepted.
  apply andb_true_iff in finalGate as [rootCloses coverage].
  apply Nat.eqb_eq in rootCloses.
  apply find_some in rootFound as [rootPresent rootId].
  unfold parsed_node_id_eq in rootId. apply Nat.eqb_eq in rootId.
  apply andb_true_iff in headerGate as [_ trailerGate].
  destruct (list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER)
    as [trailerExact|trailerWrong]; [|discriminate].
  split; [exact trailerExact|].
  exists root_node. repeat split.
  - exact rootPresent.
  - symmetry. exact rootId.
  - exact rootCloses.
  - unfold physical_spans in coverage. exact coverage.
Qed.

Lemma exact_span_coverage_bounds_object_count : forall expected arena_end spans,
  spans_cover_exactly expected arena_end spans = true ->
  expected <= arena_end /\ length spans <= arena_end - expected.
Proof.
  intros expected arena_end spans. revert expected.
  induction spans as [|[start finish] rest IH]; intros expected covered.
  - cbn in covered. apply Nat.eqb_eq in covered. subst arena_end.
    split; [lia|cbn; lia].
  - cbn in covered.
    apply andb_true_iff in covered as [starts restCovered].
    apply andb_true_iff in starts as [startsAt positive].
    apply Nat.eqb_eq in startsAt. apply Nat.ltb_lt in positive.
    subst start.
    specialize (IH finish restCovered) as [finishBound countBound].
    split; [lia|]. cbn.
    apply Nat.le_add_le_sub_l.
    assert (length rest + finish <= arena_end) as tailFits.
    { eapply Nat.le_trans.
      - apply Nat.add_le_mono_r. exact countBound.
      - rewrite (Nat.sub_add finish arena_end finishBound). reflexivity. }
    lia.
Qed.

Lemma length_insert_span : forall span spans,
  length (insert_span span spans) = S (length spans).
Proof.
  intros span spans. induction spans as [|head rest IH]; [reflexivity|].
  cbn [insert_span]. destruct (fst span <=? fst head); cbn; lia.
Qed.

Lemma length_sort_spans : forall spans,
  length (sort_spans spans) = length spans.
Proof.
  induction spans as [|span rest IH]; [reflexivity|].
  cbn [sort_spans]. rewrite length_insert_span, IH. reflexivity.
Qed.

(** Exact physical coverage gives every accepted node/line a distinct
    nonempty byte span, so an accepted image's observed object count is byte
    bounded.  This is deliberately only a post-acceptance resource theorem;
    the non-circular token-sufficiency proof must start from the canonical
    writer invariant and prove discovery succeeds. *)
Theorem accepted_image_observed_object_count_is_byte_bounded : forall image validated,
  validate_physical_image image = Some validated ->
  length (vpi_nodes validated) + length (vpi_lines validated) <= length image.
Proof.
  intros image validated accepted.
  pose proof (validated_physical_image_is_sound image validated accepted)
    as [_ [rootNode [_ [_ [_ coverage]]]]].
  pose proof (exact_span_coverage_bounds_object_count
    ACT_HEADER_LEN (vpi_arena_end validated)
    (sort_spans
      (map node_span (vpi_nodes validated) ++
       map line_span (vpi_lines validated))) coverage) as [_ countBound].
  rewrite length_sort_spans, length_app, !length_map in countBound.
  unfold validate_physical_image in accepted.
  destruct (ACT_HEADER_LEN + length ACT_TRAILER <=? length image)
    eqn:minimum; [|discriminate].
  remember (length image - length ACT_TRAILER) as arena_end.
  destruct (parse_act_header image) as [[root arena]|] eqn:header;
    [|discriminate].
  destruct ((ACT_HEADER_LEN <=? root) && (root <? arena_end) &&
    (if list_eq_dec Nat.eq_dec (skipn arena_end image) ACT_TRAILER
     then true else false)); [|discriminate].
  destruct (discover_physical_objects (seq 0 (length image)) arena_end image
    {| pd_pending := [root]; pd_nodes := []; pd_lines := [] |}); [|discriminate].
  destruct (find (parsed_node_id_eq root) (pd_nodes p)); [|discriminate].
  destruct (Nat.eqb (root + pn_encoded_len p0) arena_end &&
    spans_cover_exactly ACT_HEADER_LEN arena_end
      (sort_spans (physical_spans p))); [|discriminate].
  inversion accepted; subst validated; clear accepted.
  simpl in countBound.
  eapply Nat.le_trans; [exact countBound|].
  eapply Nat.le_trans; [apply Nat.le_sub_l|].
  subst arena_end. apply Nat.le_sub_l.
Qed.

Theorem concrete_three_way_image_is_derived_without_certificate :
  physical_image_accepts ACT_THREE_WAY_IMAGE = true.
Proof. vm_compute. reflexivity. Qed.

Theorem concrete_line_image_is_derived_without_certificate :
  physical_image_accepts ACT_LINE_IMAGE = true.
Proof. vm_compute. reflexivity. Qed.

Theorem physical_parser_rejects_nonzero_or_extra_trailer :
  physical_image_accepts
    (firstn (length ACT_THREE_WAY_IMAGE - 1) ACT_THREE_WAY_IMAGE ++ [1]) = false /\
  physical_image_accepts (ACT_THREE_WAY_IMAGE ++ [0]) = false.
Proof. vm_compute. auto. Qed.

(** * Concrete generated-protobuf staging and publication machine

    This is the event-level refinement of the generated Rust transitions in
    [models/codegen/schema.rs] and the pausable reader in
    [models/src/rust/epathmap_trie_codec.rs].  In particular:

    - [next_value_bytes] yields one private protobuf body;
    - the heterogeneous protobuf PDA decodes that body;
    - [push_value] attaches it only to [OwnedPendingEpmDecode];
    - a non-canonical body is latched, rather than reported immediately;
    - [finish] checks representation structure before reporting that latch;
    - [replace_decoded_representation_with] validates its right-hand side
      before installing it into the EPathMap held by the private decoder frame.

    A protobuf message may repeat tag 9, so there may be more than one private
    representation install.  None is externally observable: the EPathMap is
    still owned by the PDA frame.  External publication occurs exactly once,
    and only when the complete message is returned successfully. *)

Record protobuf_value_body : Type := {
  pvb_decodes : bool;
  pvb_attaches : bool;
  pvb_canonical : bool
}.

Inductive epm_stage_error : Type :=
| EpmEnvelopeError
| EpmValueDecodeError (ordinal : nat)
| EpmValueAttachError (ordinal : nat)
| EpmRepresentationError
| EpmNoncanonicalValueError (ordinal : nat)
| EpmPrivateInstallModeError
| EpmMessageCompletionError.

Inductive epm_pda_event : Type :=
| EpmEnvelopeAccepted
| EpmBodyYielded (ordinal : nat)
| EpmBodyDecodeStarted (ordinal : nat)
| EpmPrivateValueAttached (ordinal : nat)
| EpmNoncanonicalValueLatched (ordinal : nat)
| EpmPrivateFinishStarted
| EpmPrivateRepresentationReady
| EpmPrivateRepresentationInstalled
| EpmMessageCompletionStarted
| EpmExternalValuePublished
| EpmFailed (error : epm_stage_error).

Inductive epm_pda_outcome : Type :=
| EpmPdaSucceeded
| EpmPdaFailed (error : epm_stage_error).

Record epm_private_drive : Type := {
  epd_events : list epm_pda_event;
  epd_error : option epm_stage_error;
  epd_first_noncanonical : option nat
}.

Definition latch_first_noncanonical (current : option nat)
    (ordinal : nat) (canonical : bool) : option nat :=
  if canonical then current
  else match current with Some prior => Some prior | None => Some ordinal end.

Fixpoint drive_epm_value_bodies (ordinal : nat)
    (bodies : list protobuf_value_body) (first_noncanonical : option nat)
    : epm_private_drive :=
  match bodies with
  | [] =>
      {| epd_events := [EpmPrivateFinishStarted];
         epd_error := None;
         epd_first_noncanonical := first_noncanonical |}
  | body :: rest =>
      let prefix := [EpmBodyYielded ordinal; EpmBodyDecodeStarted ordinal] in
      if pvb_decodes body then
        if pvb_attaches body then
          let first' := latch_first_noncanonical first_noncanonical ordinal
            (pvb_canonical body) in
          let latch := if pvb_canonical body then []
                       else [EpmNoncanonicalValueLatched ordinal] in
          let tail := drive_epm_value_bodies (S ordinal) rest first' in
          {| epd_events := prefix ++ [EpmPrivateValueAttached ordinal] ++
                           latch ++ epd_events tail;
             epd_error := epd_error tail;
             epd_first_noncanonical := epd_first_noncanonical tail |}
        else
          {| epd_events := prefix ++ [EpmFailed (EpmValueAttachError ordinal)];
             epd_error := Some (EpmValueAttachError ordinal);
             epd_first_noncanonical := first_noncanonical |}
      else
        {| epd_events := prefix ++ [EpmFailed (EpmValueDecodeError ordinal)];
           epd_error := Some (EpmValueDecodeError ordinal);
           epd_first_noncanonical := first_noncanonical |}
  end.

Record epm_snapshot_input : Type := {
  esi_envelope_valid : bool;
  esi_bodies : list protobuf_value_body;
  esi_representation_valid : bool;
  esi_parent_mode_compatible : bool
}.

Record epm_snapshot_run : Type := {
  esr_events : list epm_pda_event;
  esr_outcome : epm_pda_outcome
}.

Definition finish_epm_private_drive (input : epm_snapshot_input)
    (drive : epm_private_drive) : epm_snapshot_run :=
  match epd_error drive with
  | Some error =>
      {| esr_events := EpmEnvelopeAccepted :: epd_events drive;
         esr_outcome := EpmPdaFailed error |}
  | None =>
      if esi_representation_valid input then
        let ready := EpmEnvelopeAccepted :: epd_events drive ++
                     [EpmPrivateRepresentationReady] in
        match epd_first_noncanonical drive with
        | Some ordinal =>
            {| esr_events := ready ++ [EpmFailed (EpmNoncanonicalValueError ordinal)];
               esr_outcome := EpmPdaFailed (EpmNoncanonicalValueError ordinal) |}
        | None =>
            if esi_parent_mode_compatible input then
              {| esr_events := ready ++ [EpmPrivateRepresentationInstalled];
                 esr_outcome := EpmPdaSucceeded |}
            else
              {| esr_events := ready ++ [EpmFailed EpmPrivateInstallModeError];
                 esr_outcome := EpmPdaFailed EpmPrivateInstallModeError |}
        end
      else
        {| esr_events := EpmEnvelopeAccepted :: epd_events drive ++
                         [EpmFailed EpmRepresentationError];
           esr_outcome := EpmPdaFailed EpmRepresentationError |}
  end.

Definition run_epm_snapshot_pda (input : epm_snapshot_input)
    : epm_snapshot_run :=
  if esi_envelope_valid input then
    finish_epm_private_drive input
      (drive_epm_value_bodies 0 (esi_bodies input) None)
  else
    {| esr_events := [EpmFailed EpmEnvelopeError];
       esr_outcome := EpmPdaFailed EpmEnvelopeError |}.

Fixpoint epm_private_install_count (events : list epm_pda_event) : nat :=
  match events with
  | [] => 0
  | EpmPrivateRepresentationInstalled :: rest => S (epm_private_install_count rest)
  | _ :: rest => epm_private_install_count rest
  end.

Fixpoint epm_external_publication_count (events : list epm_pda_event) : nat :=
  match events with
  | [] => 0
  | EpmExternalValuePublished :: rest =>
      S (epm_external_publication_count rest)
  | _ :: rest => epm_external_publication_count rest
  end.

Fixpoint epm_yield_count (events : list epm_pda_event) : nat :=
  match events with
  | [] => 0
  | EpmBodyYielded _ :: rest => S (epm_yield_count rest)
  | _ :: rest => epm_yield_count rest
  end.

Fixpoint epm_attach_count (events : list epm_pda_event) : nat :=
  match events with
  | [] => 0
  | EpmPrivateValueAttached _ :: rest => S (epm_attach_count rest)
  | _ :: rest => epm_attach_count rest
  end.

Lemma epm_private_install_count_app : forall left right,
  epm_private_install_count (left ++ right) =
  epm_private_install_count left + epm_private_install_count right.
Proof.
  induction left as [|event rest IH]; intros right; [reflexivity|].
  destruct event; cbn; rewrite IH; reflexivity.
Qed.

Lemma epm_external_publication_count_app : forall left right,
  epm_external_publication_count (left ++ right) =
  epm_external_publication_count left +
  epm_external_publication_count right.
Proof.
  induction left as [|event rest IH]; intros right; [reflexivity|].
  destruct event; cbn; rewrite IH; reflexivity.
Qed.

Lemma epm_yield_count_app : forall left right,
  epm_yield_count (left ++ right) =
  epm_yield_count left + epm_yield_count right.
Proof.
  induction left as [|event rest IH]; intros right; [reflexivity|].
  destruct event; cbn; rewrite IH; reflexivity.
Qed.

Lemma epm_attach_count_app : forall left right,
  epm_attach_count (left ++ right) =
  epm_attach_count left + epm_attach_count right.
Proof.
  induction left as [|event rest IH]; intros right; [reflexivity|].
  destruct event; cbn; rewrite IH; reflexivity.
Qed.

Theorem private_drive_never_publishes : forall ordinal bodies first,
  epm_private_install_count
    (epd_events (drive_epm_value_bodies ordinal bodies first)) = 0.
Proof.
  intros ordinal bodies first. revert ordinal first.
  induction bodies as [|body rest IH]; intros ordinal first; [reflexivity|].
  cbn. destruct (pvb_decodes body), (pvb_attaches body); cbn;
    try reflexivity.
  destruct (pvb_canonical body); cbn;
    rewrite ?epm_private_install_count_app, IH; reflexivity.
Qed.

Theorem private_drive_errors_are_body_local : forall ordinal bodies first error,
  epd_error (drive_epm_value_bodies ordinal bodies first) = Some error ->
  exists failed_ordinal,
    error = EpmValueDecodeError failed_ordinal \/
    error = EpmValueAttachError failed_ordinal.
Proof.
  intros ordinal bodies. revert ordinal.
  induction bodies as [|body rest IH]; intros ordinal first error failed;
    [discriminate|].
  cbn in failed.
  destruct (pvb_decodes body) eqn:decodes.
  - destruct (pvb_attaches body) eqn:attaches.
    + destruct (pvb_canonical body); cbn in failed;
        eapply IH; exact failed.
    + inversion failed; subst error.
      exists ordinal. auto.
  - inversion failed; subst error.
    exists ordinal. auto.
Qed.

Theorem successful_private_drive_counts_every_body : forall bodies ordinal first,
  epd_error (drive_epm_value_bodies ordinal bodies first) = None ->
  epm_yield_count (epd_events
    (drive_epm_value_bodies ordinal bodies first)) = length bodies /\
  epm_attach_count (epd_events
    (drive_epm_value_bodies ordinal bodies first)) = length bodies.
Proof.
  induction bodies as [|body rest IH]; intros ordinal first noError.
  - auto.
  - cbn in noError |- *.
    destruct (pvb_decodes body) eqn:decodes; [|discriminate].
    destruct (pvb_attaches body) eqn:attaches; [|discriminate].
    destruct (pvb_canonical body) eqn:canonical.
    + cbn in noError |- *.
      specialize (IH (S ordinal) first noError) as [yieldExact attachExact].
      rewrite ?epm_yield_count_app, ?epm_attach_count_app.
      cbn; rewrite yieldExact, attachExact; auto.
    + cbn in noError |- *.
      specialize (IH (S ordinal)
        (match first with Some prior => Some prior | None => Some ordinal end)
        noError) as [yieldExact attachExact].
      rewrite ?epm_yield_count_app, ?epm_attach_count_app.
      cbn; rewrite yieldExact, attachExact; auto.
Qed.

Theorem failed_epm_snapshot_has_no_private_install : forall input error,
  esr_outcome (run_epm_snapshot_pda input) = EpmPdaFailed error ->
  epm_private_install_count (esr_events (run_epm_snapshot_pda input)) = 0.
Proof.
  intros input error failed.
  unfold run_epm_snapshot_pda in *.
  destruct (esi_envelope_valid input) eqn:envelope; [|reflexivity].
  unfold finish_epm_private_drive in *.
  remember (drive_epm_value_bodies 0 (esi_bodies input) None) as drive.
  pose proof (private_drive_never_publishes
    0 (esi_bodies input) None) as driveNoPublish.
  rewrite <- Heqdrive in driveNoPublish.
  destruct (epd_error drive) as [driveError|] eqn:driveStatus.
  - cbn. rewrite driveNoPublish. reflexivity.
  - destruct (esi_representation_valid input) eqn:representation.
    + destruct (epd_first_noncanonical drive) as [ordinal|] eqn:canonicality.
      * cbn. repeat rewrite epm_private_install_count_app.
        cbn. rewrite driveNoPublish. reflexivity.
      * destruct (esi_parent_mode_compatible input) eqn:compatible.
        -- discriminate.
        -- cbn. repeat rewrite epm_private_install_count_app.
           cbn. rewrite driveNoPublish. reflexivity.
    + cbn. repeat rewrite epm_private_install_count_app.
      cbn. rewrite driveNoPublish. reflexivity.
Qed.

Theorem successful_epm_snapshot_installs_once_and_last : forall input,
  esr_outcome (run_epm_snapshot_pda input) = EpmPdaSucceeded ->
  epm_private_install_count (esr_events (run_epm_snapshot_pda input)) = 1 /\
  exists private_prefix,
    esr_events (run_epm_snapshot_pda input) =
      private_prefix ++ [EpmPrivateRepresentationInstalled].
Proof.
  intros input succeeded.
  unfold run_epm_snapshot_pda in *.
  destruct (esi_envelope_valid input) eqn:envelope; [|discriminate].
  unfold finish_epm_private_drive in *.
  remember (drive_epm_value_bodies 0 (esi_bodies input) None) as drive.
  pose proof (private_drive_never_publishes
    0 (esi_bodies input) None) as driveNoPublish.
  rewrite <- Heqdrive in driveNoPublish.
  destruct (epd_error drive) as [driveError|] eqn:driveStatus; [discriminate|].
  destruct (esi_representation_valid input) eqn:representation; [|discriminate].
  destruct (epd_first_noncanonical drive) as [ordinal|] eqn:canonicality;
    [discriminate|].
  destruct (esi_parent_mode_compatible input) eqn:compatible; [|discriminate].
  cbn. repeat rewrite epm_private_install_count_app. cbn.
  rewrite driveNoPublish. split; [reflexivity|].
  exists (EpmEnvelopeAccepted :: epd_events drive ++
    [EpmPrivateRepresentationReady]). reflexivity.
Qed.

Theorem successful_epm_snapshot_accounts_for_every_private_body : forall input,
  esr_outcome (run_epm_snapshot_pda input) = EpmPdaSucceeded ->
  epm_yield_count (esr_events (run_epm_snapshot_pda input)) =
    length (esi_bodies input) /\
  epm_attach_count (esr_events (run_epm_snapshot_pda input)) =
    length (esi_bodies input).
Proof.
  intros input succeeded.
  unfold run_epm_snapshot_pda in *.
  destruct (esi_envelope_valid input) eqn:envelope; [|discriminate].
  unfold finish_epm_private_drive in *.
  remember (drive_epm_value_bodies 0 (esi_bodies input) None) as drive.
  destruct (epd_error drive) as [driveError|] eqn:driveStatus; [discriminate|].
  destruct (esi_representation_valid input) eqn:representation; [|discriminate].
  destruct (epd_first_noncanonical drive) as [ordinal|] eqn:canonicality;
    [discriminate|].
  destruct (esi_parent_mode_compatible input) eqn:compatible; [|discriminate].
  pose proof (successful_private_drive_counts_every_body
    (esi_bodies input) 0 None) as bodyCounts.
  rewrite Heqdrive in driveStatus.
  specialize (bodyCounts driveStatus) as [yields attaches].
  rewrite <- Heqdrive in yields, attaches.
  cbn. rewrite ?epm_yield_count_app, ?epm_attach_count_app.
  cbn. rewrite yields, attaches. lia.
Qed.

Theorem representation_failure_precedes_latched_noncanonicality : forall input drive ordinal,
  esi_envelope_valid input = true ->
  drive_epm_value_bodies 0 (esi_bodies input) None = drive ->
  epd_error drive = None ->
  epd_first_noncanonical drive = Some ordinal ->
  esi_representation_valid input = false ->
  esr_outcome (run_epm_snapshot_pda input) =
    EpmPdaFailed EpmRepresentationError.
Proof.
  intros input drive ordinal envelope drove noDriveError latched invalid.
  unfold run_epm_snapshot_pda. rewrite envelope.
  unfold finish_epm_private_drive. rewrite drove, noDriveError, invalid.
  reflexivity.
Qed.

Theorem private_install_mode_failure_occurs_after_representation_ready : forall input,
  esr_outcome (run_epm_snapshot_pda input) =
    EpmPdaFailed EpmPrivateInstallModeError ->
  exists prefix,
    esr_events (run_epm_snapshot_pda input) =
      prefix ++ [EpmPrivateRepresentationReady;
                 EpmFailed EpmPrivateInstallModeError].
Proof.
  intros input failed.
  unfold run_epm_snapshot_pda in *.
  destruct (esi_envelope_valid input) eqn:envelope; cbn in failed; [|discriminate].
  unfold finish_epm_private_drive in *.
  remember (drive_epm_value_bodies 0 (esi_bodies input) None) as drive.
  destruct (epd_error drive) as [driveError|] eqn:driveStatus;
    cbn in failed.
  - rewrite Heqdrive in driveStatus.
    destruct (private_drive_errors_are_body_local
      0 (esi_bodies input) None driveError driveStatus)
      as [failedOrdinal [decodeError|attachError]];
      subst driveError; discriminate.
  -
  destruct (esi_representation_valid input) eqn:representation;
    cbn in failed; [|discriminate].
  destruct (epd_first_noncanonical drive) as [ordinal|] eqn:canonicality;
    cbn in failed; [discriminate|].
  destruct (esi_parent_mode_compatible input) eqn:compatible;
    cbn in failed; [discriminate|].
  exists (EpmEnvelopeAccepted :: epd_events drive).
  cbn. f_equal. symmetry.
  replace [EpmPrivateRepresentationReady;
           EpmFailed EpmPrivateInstallModeError]
    with ([EpmPrivateRepresentationReady] ++
          [EpmFailed EpmPrivateInstallModeError]) by reflexivity.
  apply app_assoc.
Qed.

Theorem private_drive_never_externally_publishes : forall ordinal bodies first,
  epm_external_publication_count
    (epd_events (drive_epm_value_bodies ordinal bodies first)) = 0.
Proof.
  intros ordinal bodies first. revert ordinal first.
  induction bodies as [|body rest IH]; intros ordinal first; [reflexivity|].
  cbn. destruct (pvb_decodes body), (pvb_attaches body); cbn;
    try reflexivity.
  destruct (pvb_canonical body); cbn;
    rewrite ?epm_external_publication_count_app, IH; reflexivity.
Qed.

Theorem one_snapshot_frame_never_externally_publishes : forall input,
  epm_external_publication_count
    (esr_events (run_epm_snapshot_pda input)) = 0.
Proof.
  intro input. unfold run_epm_snapshot_pda.
  destruct (esi_envelope_valid input); [|reflexivity].
  unfold finish_epm_private_drive.
  remember (drive_epm_value_bodies 0 (esi_bodies input) None) as drive.
  pose proof (private_drive_never_externally_publishes
    0 (esi_bodies input) None) as driveNoExternal.
  rewrite <- Heqdrive in driveNoExternal.
  destruct (epd_error drive) as [driveError|].
  - cbn. exact driveNoExternal.
  - destruct (esi_representation_valid input).
    + destruct (epd_first_noncanonical drive) as [ordinal|].
      * cbn. repeat rewrite epm_external_publication_count_app.
        cbn. rewrite driveNoExternal. reflexivity.
      * destruct (esi_parent_mode_compatible input); cbn;
          repeat rewrite epm_external_publication_count_app;
          cbn; rewrite driveNoExternal; reflexivity.
    + cbn. repeat rewrite epm_external_publication_count_app.
      cbn. rewrite driveNoExternal. reflexivity.
Qed.

Fixpoint drive_epm_snapshot_fields (snapshots : list epm_snapshot_input)
    : epm_snapshot_run :=
  match snapshots with
  | [] => {| esr_events := []; esr_outcome := EpmPdaSucceeded |}
  | snapshot :: rest =>
      let head := run_epm_snapshot_pda snapshot in
      match esr_outcome head with
      | EpmPdaFailed error => head
      | EpmPdaSucceeded =>
          let tail := drive_epm_snapshot_fields rest in
          {| esr_events := esr_events head ++ esr_events tail;
             esr_outcome := esr_outcome tail |}
      end
  end.

Theorem snapshot_field_drive_never_externally_publishes : forall snapshots,
  epm_external_publication_count
    (esr_events (drive_epm_snapshot_fields snapshots)) = 0.
Proof.
  induction snapshots as [|snapshot rest IH]; [reflexivity|].
  cbn [drive_epm_snapshot_fields].
  remember (run_epm_snapshot_pda snapshot) as head eqn:headRun.
  destruct head as [headEvents headOutcome]. cbn in headRun |- *.
  pose proof (one_snapshot_frame_never_externally_publishes snapshot)
    as headNoExternal.
  rewrite <- headRun in headNoExternal. cbn in headNoExternal.
  destruct headOutcome; cbn.
  - rewrite epm_external_publication_count_app, headNoExternal, IH.
    reflexivity.
  - exact headNoExternal.
Qed.

Theorem successful_snapshot_field_drive_installs_each_field : forall snapshots,
  esr_outcome (drive_epm_snapshot_fields snapshots) = EpmPdaSucceeded ->
  epm_private_install_count
    (esr_events (drive_epm_snapshot_fields snapshots)) = length snapshots.
Proof.
  induction snapshots as [|snapshot rest IH]; intro succeeded; [reflexivity|].
  cbn [drive_epm_snapshot_fields] in succeeded |- *.
  remember (run_epm_snapshot_pda snapshot) as head eqn:headRun.
  destruct head as [headEvents headOutcome]. cbn in headRun |- *.
  destruct headOutcome.
  - remember (drive_epm_snapshot_fields rest) as tail eqn:tailRun.
    destruct tail as [tailEvents tailOutcome]. cbn in tailRun, succeeded |- *.
    destruct tailOutcome; [|discriminate].
    assert (esr_outcome (run_epm_snapshot_pda snapshot) = EpmPdaSucceeded)
      as headSucceeded by (rewrite <- headRun; reflexivity).
    pose proof (proj1
      (successful_epm_snapshot_installs_once_and_last snapshot headSucceeded))
      as headCount.
    rewrite <- headRun in headCount. cbn in headCount.
    specialize (IH eq_refl). cbn in IH.
    rewrite epm_private_install_count_app, headCount, IH. reflexivity.
  - discriminate.
Qed.

Record epm_message_input : Type := {
  emi_snapshots : list epm_snapshot_input;
  emi_completion_valid : bool
}.

Definition run_epm_message_pda (input : epm_message_input)
    : epm_snapshot_run :=
  let private := drive_epm_snapshot_fields (emi_snapshots input) in
  match esr_outcome private with
  | EpmPdaFailed error => private
  | EpmPdaSucceeded =>
      if emi_completion_valid input then
        {| esr_events := esr_events private ++
             [EpmMessageCompletionStarted; EpmExternalValuePublished];
           esr_outcome := EpmPdaSucceeded |}
      else
        {| esr_events := esr_events private ++
             [EpmMessageCompletionStarted; EpmFailed EpmMessageCompletionError];
           esr_outcome := EpmPdaFailed EpmMessageCompletionError |}
  end.

Theorem failed_complete_message_has_no_external_publication : forall input error,
  esr_outcome (run_epm_message_pda input) = EpmPdaFailed error ->
  epm_external_publication_count (esr_events (run_epm_message_pda input)) = 0.
Proof.
  intros input error failed. unfold run_epm_message_pda in *.
  remember (drive_epm_snapshot_fields (emi_snapshots input)) as private.
  pose proof (snapshot_field_drive_never_externally_publishes
    (emi_snapshots input)) as noExternal.
  rewrite <- Heqprivate in noExternal.
  destruct private as [privateEvents privateOutcome].
  cbn in noExternal, failed |- *.
  destruct privateOutcome.
  - destruct (emi_completion_valid input); [discriminate|].
    cbn. rewrite epm_external_publication_count_app, noExternal.
    reflexivity.
  - exact noExternal.
Qed.

Theorem successful_complete_message_publishes_exactly_once_and_last : forall input,
  esr_outcome (run_epm_message_pda input) = EpmPdaSucceeded ->
  epm_external_publication_count (esr_events (run_epm_message_pda input)) = 1 /\
  exists private_prefix,
    esr_events (run_epm_message_pda input) =
      private_prefix ++ [EpmExternalValuePublished].
Proof.
  intros input succeeded. unfold run_epm_message_pda in *.
  remember (drive_epm_snapshot_fields (emi_snapshots input)) as private.
  pose proof (snapshot_field_drive_never_externally_publishes
    (emi_snapshots input)) as noExternal.
  rewrite <- Heqprivate in noExternal.
  destruct private as [privateEvents privateOutcome].
  cbn in noExternal, succeeded |- *.
  destruct privateOutcome; cbn in succeeded |- *; [|discriminate].
  destruct (emi_completion_valid input); cbn in succeeded; [|discriminate].
  cbn. rewrite epm_external_publication_count_app, noExternal.
  split; [reflexivity|].
  exists (privateEvents ++ [EpmMessageCompletionStarted]).
  replace [EpmMessageCompletionStarted; EpmExternalValuePublished]
    with ([EpmMessageCompletionStarted] ++ [EpmExternalValuePublished])
    by reflexivity.
  apply app_assoc.
Qed.

Definition repeated_snapshot_message : epm_message_input :=
  {| emi_snapshots :=
       [{| esi_envelope_valid := true; esi_bodies := [];
           esi_representation_valid := true;
           esi_parent_mode_compatible := true |};
        {| esi_envelope_valid := true; esi_bodies := [];
           esi_representation_valid := true;
           esi_parent_mode_compatible := true |}];
     emi_completion_valid := true |}.

Theorem repeated_tag9_installs_privately_twice_but_publishes_once :
  epm_private_install_count
    (esr_events (run_epm_message_pda repeated_snapshot_message)) = 2 /\
  epm_external_publication_count
    (esr_events (run_epm_message_pda repeated_snapshot_message)) = 1.
Proof. split; reflexivity. Qed.

Definition second_snapshot_failure_message : epm_message_input :=
  {| emi_snapshots :=
       [{| esi_envelope_valid := true; esi_bodies := [];
           esi_representation_valid := true;
           esi_parent_mode_compatible := true |};
        {| esi_envelope_valid := true; esi_bodies := [];
           esi_representation_valid := false;
           esi_parent_mode_compatible := true |}];
     emi_completion_valid := true |}.

Theorem later_snapshot_failure_keeps_prior_install_private :
  epm_private_install_count
    (esr_events (run_epm_message_pda second_snapshot_failure_message)) = 1 /\
  epm_external_publication_count
    (esr_events (run_epm_message_pda second_snapshot_failure_message)) = 0 /\
  esr_outcome (run_epm_message_pda second_snapshot_failure_message) =
    EpmPdaFailed EpmRepresentationError.
Proof. repeat split; reflexivity. Qed.

Print Assumptions little_endian_digits_decode.
Print Assumptions little_endian_digits_last_nonzero.
Print Assumptions encode_act_varint_is_canonical.
Print Assumptions encode_act_varint_parse_roundtrip.
Print Assumptions encode_act_varint_has_exact_u64_bound.
Print Assumptions append_arena_position_exact.
Print Assumptions push_pending_node_position_exact.
Print Assumptions add_or_reuse_line_arena_effect.
Print Assumptions strongly_sorted_nat_lt_nodup.
Print Assumptions bounded_bytes_in_full_alphabet.
Print Assumptions canonical_child_labels_have_at_most_256_members.
Print Assumptions canonical_writer_tree_has_canonical_shape.
Print Assumptions encode_act_varint_nonempty.
Print Assumptions every_pending_node_encoding_is_nonempty.
Print Assumptions every_line_object_encoding_is_nonempty.
Print Assumptions physical_writer_bytes_app.
Print Assumptions physical_writer_bytes_singleton.
Print Assumptions physical_writer_objects_layout_snoc.
Print Assumptions traced_push_pending_node_erases.
Print Assumptions traced_add_or_reuse_line_erases.
Print Assumptions traced_push_pending_node_preserves_consistency.
Print Assumptions traced_add_or_reuse_line_preserves_consistency.
Print Assumptions traced_push_pending_node_preserves_layout.
Print Assumptions traced_add_or_reuse_line_preserves_layout.
Print Assumptions traced_push_pending_node_preserves_invariant.
Print Assumptions traced_add_or_reuse_line_preserves_invariant.
Print Assumptions traced_push_pending_children_erases.
Print Assumptions traced_push_pending_children_preserves_consistency.
Print Assumptions traced_push_pending_children_preserves_invariant.
Print Assumptions traced_push_pending_children_total.
Print Assumptions push_pending_children_total.
Print Assumptions source_writer_is_total.
Print Assumptions traced_source_writer_is_total.
Print Assumptions traced_source_writer_is_total_and_consistent.
Print Assumptions traced_source_writer_is_total_and_invariant.
Print Assumptions traced_source_writer_is_total_and_full_invariant.
Print Assumptions traced_source_writer_erases_exactly.
Print Assumptions encode_writer_tree_with_certificate_is_total.
Print Assumptions encode_writer_tree_with_certificate_erases_exactly.
Print Assumptions source_certificate_has_exact_physical_bytes.
Print Assumptions every_physical_writer_object_encoding_is_nonempty.
Print Assumptions physical_node_object_count_is_byte_bounded.
Print Assumptions source_certificate_output_is_exact.
Print Assumptions source_certificate_objects_are_contiguous.
Print Assumptions source_certificate_cache_matches_recorded_objects.
Print Assumptions canonical_writer_physical_node_count_le_image_bytes.
Print Assumptions source_semantic_count_matches_reification.
Print Assumptions traced_push_pending_node_result_objects_exact.
Print Assumptions certified_push_pending_children_erases.
Print Assumptions certified_push_pending_children_objects_exact.
Print Assumptions certified_push_pending_children_root_layout_exact.
Print Assumptions derive_direct_child_ids_from_source_roots.
Print Assumptions certified_source_writer_erases_and_reifies.
Print Assumptions semantic_certificate_erases_and_reifies_exactly.
Print Assumptions certified_push_pending_children_total.
Print Assumptions certified_source_writer_is_total.
Print Assumptions encode_writer_tree_with_semantic_certificate_is_total.
Print Assumptions certified_push_pending_children_count_exact.
Print Assumptions certified_source_writer_physical_count_exact.
Print Assumptions semantic_certificate_physical_node_count_exact.
Print Assumptions semantic_certificate_node_count_matches_source.
Print Assumptions semantic_certificate_node_count_is_byte_bounded.
Print Assumptions finalize_pending_source_semantic_node_preserves_shape.
Print Assumptions certified_push_pending_children_preserves_shape.
Print Assumptions certified_source_writer_preserves_shape.
Print Assumptions semantic_certificate_has_exact_shape.
Print Assumptions physical_writer_layout_focus_start.
Print Assumptions physical_writer_layout_focus_bounds.
Print Assumptions physical_writer_object_has_exact_image_suffix.
Print Assumptions parse_line_object_writer_roundtrip.
Print Assumptions physical_line_object_parses_from_exact_image.
Print Assumptions branch_record_writer_roundtrip.
Print Assumptions line_record_writer_roundtrip_exact_len.
Print Assumptions pending_node_record_writer_roundtrip.
Print Assumptions physical_node_object_parses_from_exact_image.
Print Assumptions replay_physical_line_cache_is_witnessed.
Print Assumptions lookup_replayed_line_cache_has_physical_witness.
Print Assumptions traced_add_or_reuse_line_references_recorded_object.
Print Assumptions certified_push_pending_children_projection_exact.
Print Assumptions certified_source_writer_projection_exact.
Print Assumptions semantic_certificate_node_projection_exact.
Print Assumptions certified_source_writer_objects_prefix.
Print Assumptions certified_source_writer_preserves_full_invariant.
Print Assumptions certified_source_writer_referenced_lines_are_recorded.
Print Assumptions semantic_certificate_referenced_lines_are_recorded.
Print Assumptions semantic_certificate_node_objects_are_recorded.
Print Assumptions semantic_certificate_has_exact_image_and_layout.
Print Assumptions recorded_physical_node_parses.
Print Assumptions recorded_physical_line_parses.
Print Assumptions branch_references_source_semantic_children.
Print Assumptions source_semantic_direct_domain_mono.
Print Assumptions physical_writer_layout_member_bounds.
Print Assumptions traced_writer_recorded_object_bounds.
Print Assumptions recorded_source_semantic_node_parses_directly.
Print Assumptions semantic_certificate_root_parses_directly.
Print Assumptions source_semantic_all_direct_domains_mono.
Print Assumptions pending_source_semantic_direct_ready_mono.
Print Assumptions pending_source_semantic_direct_ready_finalizes.
Print Assumptions certified_push_pending_children_preserves_direct_readiness.
Print Assumptions source_semantic_branch_writer_domain.
Print Assumptions source_semantic_line_writer_domain.
Print Assumptions certified_source_writer_position_mono.
Print Assumptions pending_branch_source_semantic_direct_ready.
Print Assumptions pending_line_leaf_source_semantic_direct_ready.
Print Assumptions pending_line_wrapper_source_semantic_direct_ready.
Print Assumptions certified_source_writer_preserves_direct_readiness.
Print Assumptions recorded_source_semantic_nodes_parse_directly.
Print Assumptions encode_act_varint_length_le_9.
Print Assumptions encoded_branch_labels_length_le_32.
Print Assumptions pending_node_encoded_length_le_cap.
Print Assumptions traced_push_pending_node_position_le_cap.
Print Assumptions traced_add_or_reuse_line_position_upper_bound.
Print Assumptions certified_push_pending_children_position_upper_bound.
Print Assumptions certified_source_writer_size_upper_bound.
Print Assumptions canonical_writer_semantic_certificate_image_bound.
Print Assumptions semantic_certificate_has_all_direct_domains.
Print Assumptions semantic_certificate_all_nodes_parse_directly.
Print Assumptions canonical_semantic_certificate_has_all_direct_domains.
Print Assumptions canonical_writer_semantic_certificate_parses_every_node.
Print Assumptions encode_writer_tree_is_total.
Print Assumptions concrete_writer_matches_pathmap_three_way_image.
Print Assumptions concrete_writer_matches_pathmap_line_image.
Print Assumptions concrete_writer_reuses_equal_lines_under_equal_hash.
Print Assumptions concrete_writer_collision_mismatch_allocates_and_overwrites.
Print Assumptions concrete_writer_aba_collision_reemits_first_line.
Print Assumptions collision_observation_erases_traced_result.
Print Assumptions collision_observation_erases_writer_result.
Print Assumptions collision_observation_counts_exactly_unequal_bucket_hits.
Print Assumptions collision_observation_step_is_zero_or_one.
Print Assumptions collision_observed_sequence_erases_exactly.
Print Assumptions forced_abaa_counts_two_mismatches_and_reuses_last_id.
Print Assumptions collision_misses_and_equal_reuse_do_not_increment.
Print Assumptions counting_every_occupied_bucket_is_not_collision_counting.
Print Assumptions concrete_writer_images_parse_with_the_v3_physical_parser.
Print Assumptions sparse_labels_valid_of_canonical_order.
Print Assumptions parse_branch_labels_sparse_writer_roundtrip.
Print Assumptions parse_optional_value_writer_roundtrip.
Print Assumptions parse_optional_backward_writer_roundtrip.
Print Assumptions sparse_branch_record_writer_roundtrip.
Print Assumptions empty_branch_record_writer_roundtrip.
Print Assumptions line_record_writer_roundtrip.
Print Assumptions mask_octet_testbit.
Print Assumptions dense_label_mask_writer_roundtrip.
Print Assumptions parse_branch_labels_dense_writer_roundtrip.
Print Assumptions dense_branch_record_writer_roundtrip.
Print Assumptions direct_semantic_three_way_root_matches_production_shape.
Print Assumptions direct_semantic_line_root_matches_production_shape.
Print Assumptions discover_source_work_exact.
Print Assumptions semantic_certificate_preorder_physical_ids_nodup.
Print Assumptions canonical_writer_semantic_certificate_discovery_exact.
Print Assumptions find_source_semantic_node_in_reversed_preorder.
Print Assumptions reversed_source_nodes_index.
Print Assumptions direct_children_encoded_len_source_forest_exact.
Print Assumptions direct_children_encoded_len_source_exact.
Print Assumptions replay_canonical_line_matches_add_or_reuse.
Print Assumptions replay_certified_children_exact.
Print Assumptions certified_source_writer_recursive_replay_exact.
Print Assumptions semantic_certificate_recursive_replay_exact.
Print Assumptions replay_plan_executes_source_semantics.
Print Assumptions replay_direct_semantic_plan_tokens_extensional.
Print Assumptions semantic_certificate_pda_replay_exact.
Print Assumptions semantic_certificate_root_closes_arena.
Print Assumptions semantic_certificate_image_boundaries.
Print Assumptions canonical_semantic_certificate_header_and_root_bounds.
Print Assumptions validate_accepts_semantic_certificate.
Print Assumptions validate_accepts_every_canonical_writer_output.
Print Assumptions validated_direct_image_is_sound.
Print Assumptions direct_validator_accepts_exact_three_way_fixture.
Print Assumptions direct_validator_accepts_exact_line_fixture.
Print Assumptions direct_validator_accepts_exact_equal_line_reuse_fixture.
Print Assumptions direct_validator_accepts_exact_aba_collision_fixture.
Print Assumptions direct_validator_rejects_production_shape_violations.
Print Assumptions span_only_validator_admits_the_shape_counterexamples.
Print Assumptions validated_physical_image_is_sound.
Print Assumptions exact_span_coverage_bounds_object_count.
Print Assumptions accepted_image_observed_object_count_is_byte_bounded.
Print Assumptions concrete_three_way_image_is_derived_without_certificate.
Print Assumptions concrete_line_image_is_derived_without_certificate.
Print Assumptions physical_parser_rejects_nonzero_or_extra_trailer.
Print Assumptions private_drive_never_publishes.
Print Assumptions private_drive_errors_are_body_local.
Print Assumptions successful_private_drive_counts_every_body.
Print Assumptions failed_epm_snapshot_has_no_private_install.
Print Assumptions successful_epm_snapshot_installs_once_and_last.
Print Assumptions successful_epm_snapshot_accounts_for_every_private_body.
Print Assumptions representation_failure_precedes_latched_noncanonicality.
Print Assumptions private_install_mode_failure_occurs_after_representation_ready.
Print Assumptions private_drive_never_externally_publishes.
Print Assumptions one_snapshot_frame_never_externally_publishes.
Print Assumptions snapshot_field_drive_never_externally_publishes.
Print Assumptions successful_snapshot_field_drive_installs_each_field.
Print Assumptions failed_complete_message_has_no_external_publication.
Print Assumptions successful_complete_message_publishes_exactly_once_and_last.
Print Assumptions repeated_tag9_installs_privately_twice_but_publishes_once.
Print Assumptions later_snapshot_failure_keeps_prior_install_private.

End ActConcreteV5.
