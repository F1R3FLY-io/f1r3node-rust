(** * RPB3TypedValid: score-free typed validity of raw field trees.

    Design: the score-arms design, section 4, R3 (leaf L15).
    Rust authority: models/src/rust/rholang/protobuf_admission.rs (P below).

    A raw field tree is typed-valid when every node satisfies:

    - TV1: the node is structurally admissible
      ([structurally_admissible]): its field has a generated program, or it
      is a StringMap or PathMap payload node of the shape the decoder emits,
      a childless length-delimited leaf with empty requirement lists and a
      body in the map-entry or EPM1 grammar;
    - TV2: every Scalar child passes the generated scalar rule selected by
      its parent's program and its tag, with the wire and payload the
      generated descriptor names (the contract of
      [checked_generated_scalar_raw], see
      [scalar_contract_holds_matches_checked_generated_scalar_raw]);
    - TV3: the body of a node whose parent re-reads that body satisfies the
      independent byte grammar [reread_body_canonical];
    - TV4: every child is typed-valid.

    Typed validity deliberately mentions no score, no ordering, no slot, no
    refusal and no oracle: refusal is a legitimate reducer result, and
    defining validity through oracle acceptance would make the totality
    theorem R3 vacuous.

    Deviations from the design (each with a closed proof of why it is
    needed):

    - D1, parent-relative TV2.  The design writes [scalar_contract_holds
      field = true] for a field alone, but the generated scalar rule is
      selected by (parent program, tag) and a Scalar [raw_field] carries no
      program id (its [raw_program_type_id] is [None], otherwise TV1 fails).
      No function of the field alone can be the contract:
      [scalar_contract_is_not_field_local] refutes the unary reading with a
      field the rules of two parents disagree on.  The contract is therefore
      [scalar_contract_holds parent field], checked by each node over its
      Scalar children.

    - D2, type 64 in TV3.  The design lists 25, 26, 61, 62, 63 as the types
      whose parent re-reads the body.  The GUnforgeable parent also inspects
      the body of a GSysAuthToken child: exact_unforgeable_score demands
      [length (sem_body child) =? 0] (RPB3SemanticReducer.v, the
      [Some (type_id, constructor, false)] arm).  Without TV3 covering 64, a
      nonempty-body token is unrefused and scoreless on a complete
      NameComponent, which is limbo for R3:
      [five_type_tv3_leaves_gsysauthtoken_in_limbo].  The table below gives
      64 the empty field list, which is the protobuf grammar of a message
      with no fields (Rust: GSysAuthToken has an empty generated program, so
      a field in its body is refused at P:7842-7843).

    The nested [Forall_forest] shape from the design is accepted by the
    positivity checker, so it is kept as designed.

    Consequence of TV2 as written, recorded for the consumers: a Scalar root
    has no parent, so TV2 is vacuous for it.  This does not affect the
    totality statement.

    Auxiliary payloads.  [generated_wire_program] is [None] for StringMap
    and PathMap payload fields, so the first form of TV1 kept every New
    carrying map entries and every present EPathMap or EZipper auxiliary
    out of the totality statement.  TV1 is now the disjunction above; the
    section on the shape of these nodes gives what the auxiliary disjunct
    states and the decoder lines it rests on.  Its limits are deliberate:

    - it is node-local.  A StringMap is not checked against the slot of its
      parent, and the strictly ascending keys a parent frame requires
      (P:785-791) are a sibling ordering, excluded like every other ordering;
    - the map value, the EPM1 values and the ACTree03 arena stay opaque
      bytes, because the decoder builds no children for them;
    - an explicit empty map value is rejected at the byte level, mirroring
      P:3425-3431.  A nonempty value whose Par is still semantically default
      (for example, one holding only locally_free) needs the deep re-read
      of the value ([source_par_semantic_default], P:1619) and is left to
      that re-read.

    The current reducer refuses every tree containing an auxiliary node:
    [tree_with_auxiliary_node_is_refused].  The totality obligation for
    these trees is therefore discharged through the refusal disjunct of
    [reducer_result_acceptable]; the complete rows of the PathMap payload
    class stay unreachable without a refusal on this reducer
    ([pathmap_payload_complete_row_is_refused_by_the_current_reducer]). *)

From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3SourceScalarReduction RPB3SemanticReducer RPB3ScoreClass.
From StackSafePDA Require RPB3RawRegionTree RPB3WireCursor
  RPB3AuxiliaryTypedReference RPB3RecursiveAuxiliarySourceReference
  RPB3DescriptorProjectionData EPM1.

Import ListNotations.

(** ** Small helpers over the tree type of the proof kernel *)

Definition tree_field (subject : @tree raw_field) : raw_field :=
  match subject with
  | Node field _ => field
  end.

Fixpoint forest_fields (subjects : @forest raw_field) : list raw_field :=
  match subjects with
  | FNil => []
  | FCons subject rest => tree_field subject :: forest_fields rest
  end.

Definition generated_program_present (field : raw_field) : bool :=
  match generated_wire_program field with
  | Some _ => true
  | None => false
  end.

Lemma generated_program_present_iff :
  forall field,
    generated_program_present field = true <->
    generated_wire_program field <> None.
Proof.
  intro field.
  unfold generated_program_present.
  destruct (generated_wire_program field) as [program|];
    split; intro premise; congruence.
Qed.

(** ** TV2: the scalar contract, relative to the parent's program

    The same contract as [checked_generated_scalar_raw]
    (RPB3RecursiveAuxiliarySourceReference.v): the tag resolves in the
    parent's generated program to a Scalar descriptor with the child's wire,
    the generated rule accepts the child's body, and the raw body is valid.
    A non-Scalar child owes nothing here. *)

Definition scalar_contract_holds (parent : generated_program)
    (field : raw_field) : bool :=
  match raw_payload field with
  | Scalar =>
      match resolve_tag (program_fields parent) (raw_tag field),
          generated_scalar_rule parent (raw_tag field) with
      | Some descriptor, Some rule =>
          wire_eqb (raw_wire field) (descriptor_wire descriptor) &&
          payload_eqb (descriptor_payload descriptor) Scalar &&
          scalar_body_valid rule (raw_body field) &&
          valid_raw_body field
      | _, _ => false
      end
  | Message | StringMap | PathMap => true
  end.

(** TV2 as checked by a node over its children: the rule selection needs
    the node's own program.  A node without a program owes nothing here, it
    fails TV1 on its own. *)
Definition children_scalar_contract_holds (field : raw_field)
    (children : @forest raw_field) : bool :=
  match generated_wire_program field with
  | Some program => forallb (scalar_contract_holds program) (forest_fields children)
  | None => true
  end.

(** ** TV3: the independent byte grammar for re-read bodies

    Written directly over bytes, without [RPB3WireCursor], without the
    decoder and without any oracle module, so the later proofs that the
    decoder re-reads these bodies compare two independent artifacts. *)

(** A protobuf varint in canonical form at the head of [input]: little
    endian groups, at most ten of them, the tenth holding only the top bit
    of a u64, and no trailing zero group. *)
Fixpoint varint_groups_from (groups_left group : nat) (accumulated : N)
    (input : list nat) {struct groups_left} : option (N * list nat) :=
  match groups_left with
  | O => None
  | S later_groups =>
      match input with
      | [] => None
      | byte :: rest =>
          if (255 <? byte) || ((group =? 9) && (1 <? byte)) then None
          else
            let value :=
              N.add accumulated
                (N.mul (N.of_nat (Nat.modulo byte 128))
                  (N.pow 128%N (N.of_nat group))) in
            if byte <? 128 then
              if (0 <? group) && (byte =? 0) then None
              else Some (value, rest)
            else varint_groups_from later_groups (S group) value rest
      end
  end.

Definition canonical_varint_prefix (input : list nat)
    : option (N * list nat) :=
  varint_groups_from 10 0 0%N input.

(** After a field key: the canonical length, exactly that many bytes each
    below 256, and at least one of them, because the bytes field has a
    forbidden default (P:7443-7453, "explicit protobuf scalar default").
    The length is compared as [N] before [N.to_nat], so an enormous declared
    length costs nothing. *)
Definition bytes_field_tail (after_key : list nat) : option (list nat) :=
  match canonical_varint_prefix after_key with
  | None => None
  | Some (declared, rest) =>
      if N.leb declared (N.of_nat (length rest)) then
        let count := N.to_nat declared in
        if (0 <? count) && forallb (fun byte => byte <? 256) (firstn count rest)
        then Some (skipn count rest)
        else None
      else None
  end.

(** After a varint key of a uint32 field with a forbidden default: a
    canonical varint in [1, 2^32 - 1] (P:7358-7376). *)
Definition u32_field_tail (after_key : list nat) : option (list nat) :=
  match canonical_varint_prefix after_key with
  | None => None
  | Some (value, rest) =>
      if N.ltb 0%N value && N.leb value 4294967295%N then Some rest else None
  end.

Inductive reread_field_kind := RereadBytes | RereadU32.

(** One field a re-read body may carry: its single canonical key byte and
    its payload kind.  Field numbers 1 and 2 with wire types 0 and 2 encode
    below 128, so the canonical key is exactly one byte. *)
Record reread_field := RereadField {
  reread_key : nat;
  reread_kind : reread_field_kind
}.

Definition reread_field_tail (field : reread_field) (after_key : list nat)
    : option (list nat) :=
  match reread_kind field with
  | RereadBytes => bytes_field_tail after_key
  | RereadU32 => u32_field_tail after_key
  end.

(** The grammar: each expected field in schema order, present at most once,
    and nothing else.  A body that ends early leaves the later fields
    absent; with no field expected only the empty body is accepted.  This
    captures the admission checks that reject a body with a field outside
    the generated schema, a wrong wire, a decreasing field or a duplicate
    (prepare_message_field, P:7831-7878). *)
Fixpoint reread_body_matches (expected : list reread_field)
    (body : list nat) : bool :=
  match expected with
  | [] => match body with [] => true | _ :: _ => false end
  | field :: later =>
      match body with
      | [] => true
      | byte :: rest =>
          if byte =? reread_key field then
            match reread_field_tail field rest with
            | Some suffix => reread_body_matches later suffix
            | None => false
            end
          else reread_body_matches later body
      end
  end.

(** The re-read field tables, from the generated descriptors of the five
    design types and of GSysAuthToken (deviation D2).  Rust:
    reduce_big_rational (P:1127-1137), reduce_fixed_point (P:1140-1147),
    reduce_bytes_leaf (P:1171-1184); the parents read those summaries at
    P:2856-2898 and P:3023-3083. *)
Definition reread_fields_of (type_id : nat) : option (list reread_field) :=
  if type_id =? 25 then
    Some [RereadField 10 RereadBytes; RereadField 18 RereadBytes]
  else if type_id =? 26 then
    Some [RereadField 10 RereadBytes; RereadField 16 RereadU32]
  else if (type_id =? 61) || (type_id =? 62) || (type_id =? 63) then
    Some [RereadField 10 RereadBytes]
  else if type_id =? 64 then Some []
  else None.

Definition reread_body_canonical (field : raw_field) : bool :=
  match raw_payload field, raw_program_type_id field with
  | Message, Some type_id =>
      match reread_fields_of type_id with
      | Some expected => reread_body_matches expected (raw_body field)
      | None => true
      end
  | _, _ => true
  end.

(** ** TV1: the shape of StringMap and PathMap payload nodes

    The decoder builds these nodes without reading their bodies.
    RPB3RawRegionTree.v: [decode_region] emits [SourceAuxiliary span field]
    for a StringMap or PathMap descriptor (lines 479-480);
    [raw_field_of_source] labels it with the span's tag, wire and body, the
    empty requirement lists, and the descriptor's child type id (a PathMap
    has none), lines 2998-3014; [raw_tree_of_source] gives it no children,
    line 3220; [source_map_auxiliary_has_exact_raw_leaf] and
    [source_pathmap_auxiliary_has_exact_raw_leaf] (lines 3250-3280) are the
    exact results.

    The auxiliary frames read the body later: a map entry through
    [decode_string_map_entry_source] (RPB3RawRegionTree.v:1208-1234), whose
    accepted field sequences are [parse_string_map_entry_spans] (lines
    1145-1157), and a snapshot through [EPM1.decode_epm1] (EPM1.v:286-343).
    Rust: map_step_with_source (P:8067-8201) and
    parse_indexed_canonical_snapshot_prefix (epathmap_trie_codec.rs:634-715).

    The two grammars below state what those readers require, directly over
    bytes and without calling them; the differential checks at the end of
    this file compare them with the readers.  Neither says anything about a
    score, a slot, the order of sibling fields, a refusal or the oracle.
    The bytes inside a map value, the ACTree03 arena and the EPM1 value
    frames are opaque at this level, exactly as the decoder leaves them. *)

(** A canonical length prefix and that many following bytes.  The declared
    length is compared as [N] before [N.to_nat], so an enormous length costs
    nothing.  The range of the payload bytes is not checked here:
    [valid_raw_body], which [auxiliary_leaf_shape] requires, covers the
    whole body. *)
Definition framed_bytes (input : list nat) : option (list nat * list nat) :=
  match canonical_varint_prefix input with
  | None => None
  | Some (declared, rest) =>
      if N.leb declared (N.of_nat (length rest)) then
        let count := N.to_nat declared in
        Some (firstn count rest, skipn count rest)
      else None
  end.

(** A map entry body: an optional field 1 then an optional field 2, each
    length-delimited (keys 10 and 18), in that order and at most once, and
    nothing else (RPB3RawRegionTree.v:1145-1157; P:8132-8170).  A present key
    is nonempty UTF-8 (P:8137-8146, [reduce_map_key] at lines 1630-1638).  A
    present value is nonempty: an explicit value equal to the omitted
    default is rejected (P:3425-3431, the [Some (false, value)] demand of
    [reduce_map_par] at lines 1640-1651).  An entry with both fields omitted
    is the default entry, which is accepted. *)
Definition string_map_entry_body_canonical (body : list nat) : bool :=
  let after_key :=
    match body with
    | 10 :: rest =>
        match framed_bytes rest with
        | Some (key, suffix) =>
            if (0 <? length key) && RPB3RawRegionTree.utf8_bytes key
            then Some suffix
            else None
        | None => None
        end
    | _ => Some body
    end in
  match after_key with
  | None => false
  | Some [] => true
  | Some (18 :: rest) =>
      match framed_bytes rest with
      | Some (value, []) => 0 <? length value
      | _ => false
      end
  | Some (_ :: _) => false
  end.

(** [count] consecutive length-prefixed frames at the head of [input]. *)
Fixpoint frames_consume (count : nat) (input : list nat)
    : option (list nat) :=
  match count with
  | O => Some input
  | S later =>
      match framed_bytes input with
      | Some (_, rest) => frames_consume later rest
      | None => None
      end
  end.

(** The three storage modes of the envelope (EPM1.v:268-278, Rust
    epathmap_trie_codec.rs:923-987): neutral has no arena and no values, set
    has an arena and no values, map has an arena. *)
Definition pathmap_mode_accepts (mode : nat) (arena : list nat)
    (value_count : N) : bool :=
  match mode with
  | 0 => (length arena =? 0) && N.eqb value_count 0%N
  | 1 => (0 <? length arena) && N.eqb value_count 0%N
  | 2 => 0 <? length arena
  | _ => false
  end.

(** The EPM1 envelope (EPM1.v:7-21): magic EPM1, version 1, the mode byte, a
    framed arena, a canonical value count, exactly that many framed values,
    and nothing after them.  The count is compared with the remaining length
    before [N.to_nat]: every frame consumes at least one byte, so this guard
    cannot change the verdict and only keeps an enormous count cheap (the
    analogous bound in Rust is at epathmap_trie_codec.rs:692-701). *)
Definition pathmap_snapshot_body_canonical (body : list nat) : bool :=
  match body with
  | 69 :: 80 :: 77 :: 49 :: 1 :: mode :: payload =>
      match framed_bytes payload with
      | None => false
      | Some (arena, after_arena) =>
          match canonical_varint_prefix after_arena with
          | None => false
          | Some (value_count, framed_values) =>
              if pathmap_mode_accepts mode arena value_count then
                if N.leb value_count (N.of_nat (length framed_values)) then
                  match frames_consume (N.to_nat value_count) framed_values with
                  | Some [] => true
                  | _ => false
                  end
                else false
              else false
          end
      end
  | _ => false
  end.

(** The field descriptors of the generated registry, and the value types its
    StringMap fields carry.  The registry is the descriptor authority here,
    so the value-type check is not a hard-coded list. *)
Definition generated_descriptors : list field_descriptor :=
  flat_map (fun program =>
    program_fields (project_generated_program program [] []))
    RPB3DescriptorProjectionData.GeneratedData.generated_programs.

Definition generated_string_map_value_types : list nat :=
  flat_map (fun descriptor =>
    match descriptor_payload descriptor, descriptor_child_type_id descriptor with
    | StringMap, Some value_type => [value_type]
    | _, _ => []
    end) generated_descriptors.

Example generated_string_map_value_types_are_exactly_par :
  generated_string_map_value_types = [1].
Proof. vm_compute. reflexivity. Qed.

(** The child type id of the label: a generated map value type for a
    StringMap, and nothing for a PathMap. *)
Definition auxiliary_descriptor_shape (field : raw_field) : bool :=
  match raw_payload field, raw_program_type_id field with
  | StringMap, Some value_type =>
      existsb (Nat.eqb value_type) generated_string_map_value_types
  | PathMap, None => true
  | _, _ => false
  end.

Definition auxiliary_body_canonical (field : raw_field) : bool :=
  match raw_payload field with
  | StringMap => string_map_entry_body_canonical (raw_body field)
  | PathMap => pathmap_snapshot_body_canonical (raw_body field)
  | Scalar | Message => false
  end.

Definition no_required_annotations (field : raw_field) : bool :=
  match raw_wire_required field, raw_semantic_required field with
  | [], [] => true
  | _, _ => false
  end.

(** The node the decoder emits for an auxiliary field: no children, the
    length-delimited wire, a valid raw body, no requirement lists, the
    right child type id, and a body in the grammar of its payload kind. *)
Definition auxiliary_leaf_shape (field : raw_field)
    (children : @forest raw_field) : bool :=
  match children with
  | FNil =>
      wire_eqb (raw_wire field) LengthDelimited &&
      valid_raw_body field &&
      no_required_annotations field &&
      auxiliary_descriptor_shape field &&
      auxiliary_body_canonical field
  | FCons _ _ => false
  end.

(** TV1. *)
Definition structurally_admissible (field : raw_field)
    (children : @forest raw_field) : Prop :=
  generated_wire_program field <> None \/
  auxiliary_leaf_shape field children = true.

Definition structurally_admissibleb (field : raw_field)
    (children : @forest raw_field) : bool :=
  generated_program_present field || auxiliary_leaf_shape field children.

Lemma structurally_admissibleb_iff :
  forall field children,
    structurally_admissibleb field children = true <->
    structurally_admissible field children.
Proof.
  intros field children.
  unfold structurally_admissibleb, structurally_admissible.
  split.
  - intro checked.
    apply orb_prop in checked as [present|shaped].
    + left. exact (proj1 (generated_program_present_iff field) present).
    + right. exact shaped.
  - intros [present|shaped].
    + apply orb_true_intro.
      left. exact (proj2 (generated_program_present_iff field) present).
    + apply orb_true_intro.
      right. exact shaped.
Qed.

Lemma auxiliary_leaf_shape_is_a_childless_auxiliary :
  forall field children,
    auxiliary_leaf_shape field children = true ->
    children = FNil /\
    (raw_payload field = StringMap \/ raw_payload field = PathMap) /\
    generated_wire_program field = None.
Proof.
  intros field children shaped.
  destruct children as [|subject rest]; [|discriminate shaped].
  split; [reflexivity|].
  cbv beta iota delta [auxiliary_leaf_shape] in shaped.
  apply andb_prop in shaped as [_ body_ok].
  unfold auxiliary_body_canonical in body_ok.
  unfold generated_wire_program.
  destruct (raw_payload field) eqn:payload; try discriminate body_ok.
  - split; [left; reflexivity|].
    destruct (raw_program_type_id field); reflexivity.
  - split; [right; reflexivity|].
    destruct (raw_program_type_id field); reflexivity.
Qed.

(** ** Typed validity *)

Inductive Forall_forest (P : @tree raw_field -> Prop)
    : @forest raw_field -> Prop :=
| Forall_forest_nil : Forall_forest P FNil
| Forall_forest_cons : forall subject rest,
    P subject ->
    Forall_forest P rest ->
    Forall_forest P (FCons subject rest).

Inductive typed_valid_tree : @tree raw_field -> Prop :=
| TypedValidNode : forall field children,
    structurally_admissible field children ->
    children_scalar_contract_holds field children = true ->
    reread_body_canonical field = true ->
    Forall_forest typed_valid_tree children ->
    typed_valid_tree (Node field children).

(** ** The decidable checker *)

Fixpoint every_node (check : raw_field -> @forest raw_field -> bool)
    (subject : @tree raw_field) {struct subject} : bool :=
  match subject with
  | Node field children =>
      check field children && every_node_forest check children
  end
with every_node_forest (check : raw_field -> @forest raw_field -> bool)
    (subjects : @forest raw_field) {struct subjects} : bool :=
  match subjects with
  | FNil => true
  | FCons subject rest =>
      every_node check subject && every_node_forest check rest
  end.

Definition node_typed_valid (field : raw_field)
    (children : @forest raw_field) : bool :=
  structurally_admissibleb field children &&
  children_scalar_contract_holds field children &&
  reread_body_canonical field.

Definition typed_valid_treeb : @tree raw_field -> bool :=
  every_node node_typed_valid.

Definition typed_valid_forestb : @forest raw_field -> bool :=
  every_node_forest node_typed_valid.

Lemma typed_valid_tree_node_iff :
  forall field children,
    typed_valid_tree (Node field children) <->
    structurally_admissible field children /\
    children_scalar_contract_holds field children = true /\
    reread_body_canonical field = true /\
    Forall_forest typed_valid_tree children.
Proof.
  intros field children.
  split.
  - intro valid.
    inversion valid; subst.
    repeat split; assumption.
  - intros [present [contract [reread forest]]].
    apply TypedValidNode; assumption.
Qed.

Lemma children_scalar_contract_holds_of_program :
  forall field children program,
    generated_wire_program field = Some program ->
    children_scalar_contract_holds field children =
      forallb (scalar_contract_holds program) (forest_fields children).
Proof.
  intros field children program selected.
  unfold children_scalar_contract_holds.
  rewrite selected.
  reflexivity.
Qed.

Lemma typed_valid_tree_scalar_child_contract :
  forall field children program child,
    typed_valid_tree (Node field children) ->
    generated_wire_program field = Some program ->
    In child (forest_fields children) ->
    scalar_contract_holds program child = true.
Proof.
  intros field children program child valid selected member.
  destruct (proj1 (typed_valid_tree_node_iff field children) valid)
    as [_ [contract _]].
  rewrite (children_scalar_contract_holds_of_program
    field children program selected) in contract.
  exact (proj1 (forallb_forall _ _) contract child member).
Qed.

Lemma typed_valid_reflection_mutual :
  (forall subject : @tree raw_field,
    typed_valid_treeb subject = true <-> typed_valid_tree subject) /\
  (forall subjects : @forest raw_field,
    typed_valid_forestb subjects = true <->
    Forall_forest typed_valid_tree subjects).
Proof.
  unfold typed_valid_treeb, typed_valid_forestb.
  apply tree_forest_induction.
  - intros field children children_reflect.
    cbn [every_node].
    split.
    + intro checked.
      apply andb_prop in checked as [node_ok forest_ok].
      unfold node_typed_valid in node_ok.
      apply andb_prop in node_ok as [partial reread_ok].
      apply andb_prop in partial as [present contract_ok].
      apply TypedValidNode.
      * exact (proj1 (structurally_admissibleb_iff field children) present).
      * exact contract_ok.
      * exact reread_ok.
      * exact (proj1 children_reflect forest_ok).
    + intro valid.
      destruct (proj1 (typed_valid_tree_node_iff field children) valid)
        as [present [contract_ok [reread_ok forest_ok]]].
      apply andb_true_intro.
      split.
      * unfold node_typed_valid.
        apply andb_true_intro.
        split.
        -- apply andb_true_intro.
           split.
           ++ exact (proj2 (structurally_admissibleb_iff field children) present).
           ++ exact contract_ok.
        -- exact reread_ok.
      * exact (proj2 children_reflect forest_ok).
  - split.
    + intro checked.
      constructor.
    + intro valid.
      reflexivity.
  - intros subject subject_reflect rest rest_reflect.
    cbn [every_node_forest].
    split.
    + intro checked.
      apply andb_prop in checked as [head_ok rest_ok].
      constructor.
      * exact (proj1 subject_reflect head_ok).
      * exact (proj1 rest_reflect rest_ok).
    + intro valid.
      inversion valid; subst.
      apply andb_true_intro.
      split.
      * apply (proj2 subject_reflect); assumption.
      * apply (proj2 rest_reflect); assumption.
Qed.

Theorem typed_valid_treeb_reflect :
  forall subject : @tree raw_field,
    typed_valid_treeb subject = true <-> typed_valid_tree subject.
Proof.
  exact (proj1 typed_valid_reflection_mutual).
Qed.

Theorem typed_valid_forestb_reflect :
  forall subjects : @forest raw_field,
    typed_valid_forestb subjects = true <->
    Forall_forest typed_valid_tree subjects.
Proof.
  exact (proj2 typed_valid_reflection_mutual).
Qed.

(** A typed-valid node either has a generated program or is an auxiliary
    leaf; the consumers case-split on this instead of unfolding TV1. *)
Theorem typed_valid_node_is_generated_or_auxiliary_leaf :
  forall field children,
    typed_valid_tree (Node field children) ->
    generated_wire_program field <> None \/
    (generated_wire_program field = None /\
     children = FNil /\
     (raw_payload field = StringMap \/ raw_payload field = PathMap) /\
     auxiliary_leaf_shape field children = true).
Proof.
  intros field children valid.
  destruct (proj1 (typed_valid_tree_node_iff field children) valid)
    as [[present|shaped] _].
  - left. exact present.
  - right.
    destruct (auxiliary_leaf_shape_is_a_childless_auxiliary field children shaped)
      as [no_children [payload no_program]].
    exact (conj no_program (conj no_children (conj payload shaped))).
Qed.

(** ** The auxiliary disjunct of TV1 is the decoder's leaf

    Every auxiliary descriptor of the registry is length-delimited and names
    the child type id the raw conversion needs.  With that registry fact and
    the two exact-leaf theorems of RPB3RawRegionTree.v, the node the decoder
    builds for an auxiliary field satisfies [auxiliary_leaf_shape] as soon
    as its body is valid and in the grammar.  Body validity is not a new
    obligation of this theorem: on a decoded root it is
    [decoded_root_raw_field_passes_reference_body_guard]
    (RPB3RawRegionTree.v:3135). *)

Definition auxiliary_descriptor_contract (descriptor : field_descriptor)
    : bool :=
  match descriptor_payload descriptor, descriptor_child_type_id descriptor with
  | StringMap, Some value_type =>
      wire_eqb (descriptor_wire descriptor) LengthDelimited &&
      existsb (Nat.eqb value_type) generated_string_map_value_types
  | PathMap, None => wire_eqb (descriptor_wire descriptor) LengthDelimited
  | StringMap, None | PathMap, Some _ => false
  | _, _ => true
  end.

Example generated_auxiliary_descriptors_satisfy_the_contract :
  forallb auxiliary_descriptor_contract generated_descriptors = true.
Proof. vm_compute. reflexivity. Qed.

Lemma resolve_tag_member :
  forall fields tag descriptor,
    resolve_tag fields tag = Some descriptor ->
    In descriptor fields.
Proof.
  induction fields as [|field rest rest_member]; intros tag descriptor resolved.
  - discriminate resolved.
  - cbn [resolve_tag] in resolved.
    destruct (descriptor_tag field =? tag).
    + inversion resolved; subst descriptor.
      left. reflexivity.
    + right. exact (rest_member tag descriptor resolved).
Qed.

Lemma lookup_projected_program_field_is_generated :
  forall type_id program descriptor,
    lookup_projected_program type_id [] [] = Some program ->
    In descriptor (program_fields program) ->
    In descriptor generated_descriptors.
Proof.
  intros type_id program descriptor found member.
  unfold lookup_projected_program in found.
  match type of found with
  | context [find ?test ?programs] =>
      destruct (find test programs) as [candidate|] eqn:selected
  end.
  2: { discriminate found. }
  cbn [option_map] in found.
  injection found as same_program.
  subst program.
  apply find_some in selected as [candidate_member _].
  unfold generated_descriptors.
  apply in_flat_map.
  exists candidate.
  split; [exact candidate_member|exact member].
Qed.

Lemma wire_eqb_true_implies_equal :
  forall one other,
    wire_eqb one other = true -> one = other.
Proof.
  intros one other same.
  destruct one, other; try reflexivity; discriminate same.
Qed.

Lemma auxiliary_leaf_shape_intro :
  forall field,
    raw_wire field = LengthDelimited ->
    valid_raw_body field = true ->
    raw_wire_required field = [] ->
    raw_semantic_required field = [] ->
    auxiliary_descriptor_shape field = true ->
    auxiliary_body_canonical field = true ->
    auxiliary_leaf_shape field FNil = true.
Proof.
  intros field wire valid wire_required semantic_required shape canonical.
  unfold auxiliary_leaf_shape, no_required_annotations.
  rewrite wire, valid, wire_required, semantic_required, shape, canonical.
  reflexivity.
Qed.

Theorem decoded_auxiliary_leaf_is_structurally_admissible :
  forall fuel bytes type_id program span descriptor raw,
    lookup_projected_program type_id [] [] = Some program ->
    RPB3RawRegionTree.source_descriptor program span = Some descriptor ->
    RPB3RawRegionTree.raw_tree_of_source (S fuel) bytes
      (RPB3RawRegionTree.SourceAuxiliary span descriptor) =
      Some (Node raw FNil) ->
    valid_raw_body raw = true ->
    auxiliary_body_canonical raw = true ->
    structurally_admissible raw FNil.
Proof.
  intros fuel bytes type_id program span descriptor raw
    found selected decoded valid canonical.
  right.
  assert (member_and_wire :
    In descriptor (program_fields program) /\
    wire_eqb (RPB3RawRegionTree.wire_kind_of_span span)
      (descriptor_wire descriptor) = true).
  { unfold RPB3RawRegionTree.source_descriptor in selected.
    destruct (resolve_tag (program_fields program)
      (RPB3WireCursor.span_number span)) as [candidate|] eqn:resolved;
      [|discriminate selected].
    destruct (wire_eqb (RPB3RawRegionTree.wire_kind_of_span span)
      (descriptor_wire candidate)) eqn:wire_ok;
      [|discriminate selected].
    inversion selected; subst candidate.
    split; [exact (resolve_tag_member _ _ _ resolved)|exact wire_ok]. }
  destruct member_and_wire as [member wire_matches].
  pose proof (proj1 (forallb_forall _ _)
    generated_auxiliary_descriptors_satisfy_the_contract descriptor
    (lookup_projected_program_field_is_generated type_id program descriptor
      found member)) as contract.
  unfold auxiliary_descriptor_contract in contract.
  destruct (descriptor_payload descriptor) eqn:payload.
  - exfalso.
    cbn [RPB3RawRegionTree.raw_tree_of_source
      RPB3RawRegionTree.raw_field_of_source
      RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field] in decoded.
    rewrite payload in decoded.
    discriminate decoded.
  - exfalso.
    cbn [RPB3RawRegionTree.raw_tree_of_source
      RPB3RawRegionTree.raw_field_of_source
      RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field] in decoded.
    rewrite payload in decoded.
    discriminate decoded.
  - destruct (descriptor_child_type_id descriptor) as [value_type|] eqn:child_id.
    2: { discriminate contract. }
    cbv beta iota in contract.
    apply andb_prop in contract as [wire_contract value_contract].
    pose proof (RPB3RawRegionTree.source_map_auxiliary_has_exact_raw_leaf
      bytes span descriptor value_type fuel payload child_id) as exact_leaf.
    rewrite exact_leaf in decoded.
    injection decoded as same_raw.
    subst raw.
    apply auxiliary_leaf_shape_intro.
    + cbn [raw_wire].
      rewrite (wire_eqb_true_implies_equal _ _ wire_matches).
      exact (wire_eqb_true_implies_equal _ _ wire_contract).
    + exact valid.
    + reflexivity.
    + reflexivity.
    + exact value_contract.
    + exact canonical.
  - destruct (descriptor_child_type_id descriptor) as [value_type|] eqn:child_id.
    { discriminate contract. }
    cbv beta iota in contract.
    pose proof (RPB3RawRegionTree.source_pathmap_auxiliary_has_exact_raw_leaf
      bytes span descriptor fuel payload child_id) as exact_leaf.
    rewrite exact_leaf in decoded.
    injection decoded as same_raw.
    subst raw.
    apply auxiliary_leaf_shape_intro.
    + cbn [raw_wire].
      rewrite (wire_eqb_true_implies_equal _ _ wire_matches).
      exact (wire_eqb_true_implies_equal _ _ contract).
    + exact valid.
    + reflexivity.
    + reflexivity.
    + reflexivity.
    + exact canonical.
Qed.

(** ** TV2 is the contract of checked_generated_scalar_raw

    The hypotheses are what the decoder establishes for a scalar node of
    its region: the node stores the descriptor its span selects, and the
    node has a raw field. *)

Theorem scalar_contract_holds_matches_checked_generated_scalar_raw :
  forall bytes program span field raw,
    RPB3RawRegionTree.source_descriptor program span = Some field ->
    RPB3RawRegionTree.raw_field_of_source bytes
      (RPB3RawRegionTree.SourceScalar span field) = Some raw ->
    RPB3RecursiveAuxiliarySourceReference.checked_generated_scalar_raw
      bytes program (RPB3RawRegionTree.SourceScalar span field) = Some raw <->
    scalar_contract_holds program raw = true.
Proof.
  intros bytes program span field raw selected converted.
  assert (resolved :
    resolve_tag (program_fields program)
      (RPB3WireCursor.span_number span) = Some field /\
    wire_eqb (RPB3RawRegionTree.wire_kind_of_span span)
      (descriptor_wire field) = true).
  { unfold RPB3RawRegionTree.source_descriptor in selected.
    destruct (resolve_tag (program_fields program)
      (RPB3WireCursor.span_number span)) as [candidate|];
      [|discriminate].
    destruct (wire_eqb (RPB3RawRegionTree.wire_kind_of_span span)
      (descriptor_wire candidate)) eqn:wire_ok;
      [|discriminate].
    inversion selected; subst candidate.
    split; [reflexivity|exact wire_ok]. }
  destruct resolved as [tag_resolved wire_matches].
  pose proof converted as shape.
  unfold RPB3RawRegionTree.raw_field_of_source in shape.
  simpl in shape.
  destruct (descriptor_payload field) eqn:field_payload;
    simpl in shape; try discriminate.
  inversion shape; subst raw.
  clear shape.
  unfold RPB3RecursiveAuxiliarySourceReference.checked_generated_scalar_raw,
    RPB3RecursiveAuxiliarySourceReference.checked_generated_raw,
    source_scalar_raw, scalar_contract_holds.
  cbn [RPB3RawRegionTree.source_span RPB3RawRegionTree.source_field
    raw_tag raw_wire raw_payload raw_body].
  rewrite selected, converted, tag_resolved.
  cbn beta iota zeta.
  destruct (RPB3AuxiliaryTypedReference.descriptor_eq_dec field field)
    as [_|different].
  2: { exfalso. apply different. reflexivity. }
  destruct (generated_scalar_rule program
    (RPB3WireCursor.span_number span)) as [rule|].
  - cbn beta delta [raw_body] iota zeta.
    rewrite wire_matches, field_payload.
    cbn beta delta [payload_eqb andb] iota zeta.
    destruct (valid_raw_body _); destruct (scalar_body_valid rule _);
      cbn beta delta [andb] iota zeta;
      split; intro premise; try reflexivity; try discriminate.
  - cbn beta iota zeta.
    destruct (valid_raw_body _); cbn beta iota zeta;
      split; intro premise; try reflexivity; try discriminate.
Qed.

(** ** The closed counterexample to a field-local TV2 (deviation D1)

    The same raw field is accepted under the Expr program (tag 1 is the
    GBool oneof arm, default allowed) and refused under the PCost program
    (tag 1 is cost with a forbidden default), so no function of the field
    alone can coincide with the contract. *)

Definition generated_program_of (type_id : nat) : generated_program :=
  match lookup_projected_program type_id [] [] with
  | Some program => program
  | None => empty_program
  end.

Definition scalar_zero_at_tag_one : raw_field :=
  RawField 1 Varint Scalar [0] 1 None [] [].

Example scalar_contract_depends_on_the_parent :
  scalar_contract_holds (generated_program_of 24) scalar_zero_at_tag_one = true /\
  scalar_contract_holds (generated_program_of 4) scalar_zero_at_tag_one = false.
Proof. vm_compute. split; reflexivity. Qed.

Theorem scalar_contract_is_not_field_local :
  ~ exists check : raw_field -> bool,
      forall type_id program field,
        lookup_projected_program type_id [] [] = Some program ->
        check field = scalar_contract_holds program field.
Proof.
  intros [check agrees].
  pose proof (agrees 24 (generated_program_of 24) scalar_zero_at_tag_one)
    as under_expr.
  pose proof (agrees 4 (generated_program_of 4) scalar_zero_at_tag_one)
    as under_pcost.
  destruct scalar_contract_depends_on_the_parent as [accepted refused].
  rewrite accepted in under_expr.
  rewrite refused in under_pcost.
  assert (expr_found :
    lookup_projected_program 24 [] [] = Some (generated_program_of 24))
    by (vm_compute; reflexivity).
  assert (pcost_found :
    lookup_projected_program 4 [] [] = Some (generated_program_of 4))
    by (vm_compute; reflexivity).
  specialize (under_expr expr_found).
  specialize (under_pcost pcost_found).
  congruence.
Qed.

(** ** The acceptance table of R3 (design section 4)

    BindPayload reads the table's slash as per-type: the required child of
    ReceiveBind is its source and those of MatchCase are pattern and source.
    Reading the slash as a disjunction of the two flags would be vacuous at
    both types, because the reducer leaves the other family's flag false. *)

Definition exact_evidence (summary : semantic_summary) : bool :=
  match sem_score_evidence summary with
  | ExactScore _ => true
  | ScoreNotDerived => false
  end.

Definition bind_payload_required_child_absent
    (summary : semantic_summary) : bool :=
  if sem_type_id summary =? 16 then negb (sem_bind_source_present summary)
  else if sem_type_id summary =? 21 then negb (sem_case_required_present summary)
  else false.

Definition class_result_ok (summary : semantic_summary) : bool :=
  match score_class_of (sem_type_id summary) with
  | ClassPar | ClassCostPart => exact_evidence summary
  | ClassProcessComponent | ClassExprComponent =>
      match sem_complete summary with
      | Some _ => exact_evidence summary
      | None => false
      end
  | ClassCostComposite | ClassNameComponent =>
      match sem_complete summary with
      | Some true => exact_evidence summary
      | _ => false
      end
  | ClassBindPayload =>
      exact_evidence summary || bind_payload_required_child_absent summary
  | ClassPathMapPayload | ClassExprPayload =>
      match sem_complete summary, sem_score_evidence summary with
      | Some true, ExactScore _ => true
      | Some false, ScoreNotDerived => true
      | _, _ => false
      end
  | ClassParentCarrier => true
  | ClassNoStandalone | ClassOpaque => negb (exact_evidence summary)
  | Unclassified => false
  end.

Definition reducer_result_acceptable (summary : semantic_summary) : bool :=
  sem_definitely_refused summary ||
  payload_eqb (sem_payload summary) Scalar ||
  class_result_ok summary.

Definition acceptance_probe (type_id : nat) (complete : option bool)
    (evidence : score_evidence) (bind_source case_required : bool)
    : semantic_summary :=
  SemanticSummary 0 LengthDelimited Message type_id [] false complete
    evidence bind_source case_required.

Definition some_score : canonical_score := ScoreInteger 1%N.

Example acceptance_table_scored_classes :
  (* Par and cost parts: any completeness, exact evidence only. *)
  class_result_ok (acceptance_probe 1 None (ExactScore some_score) false false) = true /\
  class_result_ok (acceptance_probe 1 None ScoreNotDerived false false) = false /\
  class_result_ok (acceptance_probe 9 (Some false) (ExactScore some_score) false false) = true /\
  (* Process and Expr components: known completeness and exact evidence. *)
  class_result_ok (acceptance_probe 8 (Some false) (ExactScore some_score) false false) = true /\
  class_result_ok (acceptance_probe 24 None (ExactScore some_score) false false) = false /\
  class_result_ok (acceptance_probe 15 (Some true) ScoreNotDerived false false) = false /\
  (* Composite cost and name components: complete and exact only. *)
  class_result_ok (acceptance_probe 13 (Some true) (ExactScore some_score) false false) = true /\
  class_result_ok (acceptance_probe 60 (Some false) (ExactScore some_score) false false) = false /\
  class_result_ok (acceptance_probe 55 (Some true) ScoreNotDerived false false) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example acceptance_table_payload_classes :
  (* Bind payloads: exact, or scoreless only when the required child is absent. *)
  class_result_ok (acceptance_probe 16 None (ExactScore some_score) true false) = true /\
  class_result_ok (acceptance_probe 16 None ScoreNotDerived false false) = true /\
  class_result_ok (acceptance_probe 16 None ScoreNotDerived true false) = false /\
  class_result_ok (acceptance_probe 21 None ScoreNotDerived false false) = true /\
  class_result_ok (acceptance_probe 21 None ScoreNotDerived false true) = false /\
  (* PathMap and Expr payloads: complete exact or incomplete scoreless. *)
  class_result_ok (acceptance_probe 31 (Some true) (ExactScore some_score) false false) = true /\
  class_result_ok (acceptance_probe 32 (Some false) ScoreNotDerived false false) = true /\
  class_result_ok (acceptance_probe 31 (Some true) ScoreNotDerived false false) = false /\
  class_result_ok (acceptance_probe 36 (Some false) (ExactScore some_score) false false) = false /\
  class_result_ok (acceptance_probe 36 None (ExactScore some_score) false false) = false /\
  (* Parent carriers accept anything; no-standalone and opaque types never score. *)
  class_result_ok (acceptance_probe 34 None ScoreNotDerived false false) = true /\
  class_result_ok (acceptance_probe 56 (Some true) (ExactScore some_score) false false) = true /\
  class_result_ok (acceptance_probe 25 (Some true) ScoreNotDerived false false) = true /\
  class_result_ok (acceptance_probe 25 (Some true) (ExactScore some_score) false false) = false /\
  class_result_ok (acceptance_probe 2 None (ExactScore some_score) false false) = false /\
  (* Types outside the table are never acceptable. *)
  class_result_ok (acceptance_probe 0 None (ExactScore some_score) false false) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example reducer_result_acceptable_disjuncts :
  let refused := SemanticSummary 0 LengthDelimited Message 1 [] true None
    ScoreNotDerived false false in
  let scalar := SemanticSummary 0 Varint Scalar 0 [1] false None
    ScoreNotDerived false false in
  let limbo := acceptance_probe 60 (Some true) ScoreNotDerived false false in
  reducer_result_acceptable refused = true /\
  reducer_result_acceptable scalar = true /\
  reducer_result_acceptable limbo = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** Witnesses by computation *)

Definition message_node (tag type_id : nat) (body : list nat)
    (children : @forest raw_field) : @tree raw_field :=
  Node (RawField tag LengthDelimited Message body (length body)
    (Some type_id) [] []) children.

Definition scalar_leaf (tag : nat) (wire : wire_kind) (body : list nat)
    : @tree raw_field :=
  Node (RawField tag wire Scalar body (length body) None [] []) FNil.

Definition gbigrat_witness : @tree raw_field :=
  message_node 36 25 [10; 1; 3; 18; 1; 5]
    (FCons (scalar_leaf 1 LengthDelimited [3])
      (FCons (scalar_leaf 2 LengthDelimited [5]) FNil)).

Definition expr_gbigrat_witness : @tree raw_field :=
  message_node 5 24 [162; 2; 6; 10; 1; 3; 18; 1; 5]
    (FCons gbigrat_witness FNil).

Definition par_gbigrat_witness : @tree raw_field :=
  message_node 0 1 [42; 9; 162; 2; 6; 10; 1; 3; 18; 1; 5]
    (FCons expr_gbigrat_witness FNil).

Definition par_ground_int_witness : @tree raw_field :=
  message_node 0 1 [42; 2; 16; 14]
    (FCons (message_node 5 24 [16; 14]
      (FCons (scalar_leaf 2 Varint [14]) FNil)) FNil).

Definition gfixedpoint_witness : @tree raw_field :=
  message_node 37 26 [10; 1; 4; 16; 2]
    (FCons (scalar_leaf 1 LengthDelimited [4])
      (FCons (scalar_leaf 2 Varint [2]) FNil)).

Definition gprivate_witness : @tree raw_field :=
  message_node 1 61 [10; 3; 1; 2; 3]
    (FCons (scalar_leaf 1 LengthDelimited [1; 2; 3]) FNil).

Definition gdeployid_witness : @tree raw_field :=
  message_node 2 62 [10; 2; 8; 9]
    (FCons (scalar_leaf 1 LengthDelimited [8; 9]) FNil).

Definition gdeployerid_witness : @tree raw_field :=
  message_node 3 63 [10; 2; 7; 7]
    (FCons (scalar_leaf 1 LengthDelimited [7; 7]) FNil).

Definition gsysauthtoken_witness : @tree raw_field :=
  message_node 4 64 [] FNil.

Definition gunforgeable_token_witness : @tree raw_field :=
  message_node 7 60 [34; 0] (FCons gsysauthtoken_witness FNil).

(** Witnesses carrying auxiliary payloads.  Each is exactly the tree the
    decoder produces for its wire bytes, proved below through
    [RPB3RawRegionTree.raw_root_of_bytes]. *)

Definition string_map_leaf (body : list nat) : @tree raw_field :=
  Node (RawField 4 LengthDelimited StringMap body (length body) (Some 1) [] [])
    FNil.

Definition pathmap_leaf (body : list nat) : @tree raw_field :=
  Node (RawField 9 LengthDelimited PathMap body (length body) None [] [])
    FNil.

(** A New with bind_count 1 (sint32 2), the body Par of the ground integer
    witness, and one injection whose key is the single byte 97 and whose
    value is that same Par. *)
Definition new_with_map_entry_wire : list nat :=
  [8; 2] ++ [18; 4; 42; 2; 16; 14] ++ [34; 9; 10; 1; 97; 18; 4; 42; 2; 16; 14].

Definition new_with_map_entry_witness : @tree raw_field :=
  message_node 0 20 new_with_map_entry_wire
    (FCons (scalar_leaf 1 Varint [2])
      (FCons
        (message_node 2 1 [42; 2; 16; 14]
          (FCons (message_node 5 24 [16; 14]
            (FCons (scalar_leaf 2 Varint [14]) FNil)) FNil))
        (FCons (string_map_leaf [10; 1; 97; 18; 4; 42; 2; 16; 14]) FNil))).

Definition empty_snapshot_body : list nat :=
  EPM1.encode_epm1 RPB3AuxiliaryTypedReference.empty_pathmap_snapshot.

Definition epathmap_with_snapshot_witness : @tree raw_field :=
  message_node 0 31 RPB3AuxiliaryTypedReference.empty_pathmap_wire
    (FCons (pathmap_leaf empty_snapshot_body) FNil).

(** An EZipper has no PathMap field of its own: its auxiliary sits inside
    the EPathMap message child at its slot 0. *)
Definition ezipper_with_snapshot_wire : list nat :=
  [10; 35] ++ RPB3AuxiliaryTypedReference.empty_pathmap_wire.

Definition ezipper_with_snapshot_witness : @tree raw_field :=
  message_node 0 32 ezipper_with_snapshot_wire
    (FCons (message_node 1 31 RPB3AuxiliaryTypedReference.empty_pathmap_wire
      (FCons (pathmap_leaf empty_snapshot_body) FNil)) FNil).

Definition default_value_snapshot_body : list nat :=
  EPM1.encode_epm1 RPB3AuxiliaryTypedReference.default_value_pathmap_snapshot.

Definition epathmap_with_default_value_witness : @tree raw_field :=
  message_node 0 31 RPB3AuxiliaryTypedReference.default_value_pathmap_wire
    (FCons (pathmap_leaf default_value_snapshot_body) FNil).

Definition empty_ezipper_witness : @tree raw_field :=
  message_node 0 32 [] FNil.

Definition empty_epathmap_witness : @tree raw_field :=
  message_node 0 31 [] FNil.

Ltac typed_valid_by_compute :=
  apply (proj1 (typed_valid_treeb_reflect _));
  vm_compute;
  reflexivity.

Ltac not_typed_valid_by_compute :=
  let valid := fresh "valid" in
  intro valid;
  let checked := fresh "checked" in
  pose proof (proj2 (typed_valid_treeb_reflect _) valid) as checked;
  vm_compute in checked;
  discriminate checked.

Theorem par_ground_int_is_typed_valid :
  typed_valid_tree par_ground_int_witness.
Proof. typed_valid_by_compute. Qed.

Theorem par_gbigrat_is_typed_valid :
  typed_valid_tree par_gbigrat_witness.
Proof. typed_valid_by_compute. Qed.

Theorem expr_gbigrat_is_typed_valid :
  typed_valid_tree expr_gbigrat_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gbigrat_field_is_typed_valid :
  typed_valid_tree gbigrat_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gfixedpoint_field_is_typed_valid :
  typed_valid_tree gfixedpoint_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gprivate_field_is_typed_valid :
  typed_valid_tree gprivate_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gdeployid_field_is_typed_valid :
  typed_valid_tree gdeployid_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gdeployerid_field_is_typed_valid :
  typed_valid_tree gdeployerid_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gsysauthtoken_field_is_typed_valid :
  typed_valid_tree gsysauthtoken_witness.
Proof. typed_valid_by_compute. Qed.

Theorem gunforgeable_token_is_typed_valid :
  typed_valid_tree gunforgeable_token_witness.
Proof. typed_valid_by_compute. Qed.

(** The auxiliary witnesses are exactly what the decoder produces. *)

Example new_with_map_entry_is_the_decoded_tree :
  RPB3RawRegionTree.raw_root_of_bytes 20 new_with_map_entry_wire =
    Some new_with_map_entry_witness.
Proof. vm_compute. reflexivity. Qed.

Example epathmap_with_snapshot_is_the_decoded_tree :
  RPB3RawRegionTree.raw_root_of_bytes 31
    RPB3AuxiliaryTypedReference.empty_pathmap_wire =
    Some epathmap_with_snapshot_witness.
Proof. vm_compute. reflexivity. Qed.

Example ezipper_with_snapshot_is_the_decoded_tree :
  RPB3RawRegionTree.raw_root_of_bytes 32 ezipper_with_snapshot_wire =
    Some ezipper_with_snapshot_witness.
Proof. vm_compute. reflexivity. Qed.

Example epathmap_with_default_value_is_the_decoded_tree :
  RPB3RawRegionTree.raw_root_of_bytes 31
    RPB3AuxiliaryTypedReference.default_value_pathmap_wire =
    Some epathmap_with_default_value_witness.
Proof. vm_compute. reflexivity. Qed.

Theorem new_with_map_entry_is_typed_valid :
  typed_valid_tree new_with_map_entry_witness.
Proof. typed_valid_by_compute. Qed.

Theorem epathmap_with_snapshot_is_typed_valid :
  typed_valid_tree epathmap_with_snapshot_witness.
Proof. typed_valid_by_compute. Qed.

Theorem ezipper_with_snapshot_is_typed_valid :
  typed_valid_tree ezipper_with_snapshot_witness.
Proof. typed_valid_by_compute. Qed.

Theorem epathmap_with_default_value_is_typed_valid :
  typed_valid_tree epathmap_with_default_value_witness.
Proof. typed_valid_by_compute. Qed.

Theorem empty_ezipper_is_typed_valid :
  typed_valid_tree empty_ezipper_witness.
Proof. typed_valid_by_compute. Qed.

Theorem empty_epathmap_is_typed_valid :
  typed_valid_tree empty_epathmap_witness.
Proof. typed_valid_by_compute. Qed.

(** The byte grammars of the auxiliary bodies, on accepted and rejected
    encodings. *)

Example string_map_entry_grammar_controls :
  (* The four field shapes the decoder accepts, and a multi-byte UTF-8 key. *)
  string_map_entry_body_canonical [] = true /\
  string_map_entry_body_canonical [10; 1; 97] = true /\
  string_map_entry_body_canonical [18; 1; 5] = true /\
  string_map_entry_body_canonical [10; 1; 97; 18; 1; 5] = true /\
  string_map_entry_body_canonical [10; 2; 195; 169; 18; 1; 5] = true /\
  (* Explicit empty key and explicit empty value. *)
  string_map_entry_body_canonical [10; 0] = false /\
  string_map_entry_body_canonical [18; 0] = false /\
  string_map_entry_body_canonical [10; 1; 97; 18; 0] = false /\
  (* Reversed fields, a duplicate, an unknown field, a wrong wire type. *)
  string_map_entry_body_canonical [18; 1; 5; 10; 1; 97] = false /\
  string_map_entry_body_canonical [10; 1; 97; 10; 1; 98] = false /\
  string_map_entry_body_canonical [18; 1; 5; 18; 1; 6] = false /\
  string_map_entry_body_canonical [26; 1; 3] = false /\
  string_map_entry_body_canonical [8; 1] = false /\
  (* Keys that are not UTF-8. *)
  string_map_entry_body_canonical [10; 1; 255] = false /\
  string_map_entry_body_canonical [10; 2; 195; 1] = false /\
  (* Truncation, a non-canonical length, and trailing bytes. *)
  string_map_entry_body_canonical [10; 5; 97] = false /\
  string_map_entry_body_canonical [10; 129; 0; 97] = false /\
  string_map_entry_body_canonical [18; 1; 5; 1] = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example pathmap_snapshot_grammar_controls :
  (* Neutral, set and map modes; a map with values, including an empty one. *)
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 0; 0; 0] = true /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 1; 1; 7; 0] = true /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 0] = true /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 1; 1; 5] = true /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 1; 0] = true /\
  (* The mode rules: an empty arena where one is required, and values or an
     arena where none is allowed. *)
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 0; 0] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 1; 0; 0] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 1; 1; 7; 1; 1; 5] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 0; 1; 7; 0] = false /\
  (* Wrong magic, version and mode byte. *)
  pathmap_snapshot_body_canonical [69; 80; 77; 48; 1; 2; 1; 7; 0] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 2; 2; 1; 7; 0] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 3; 1; 7; 0] = false /\
  (* Truncated arena, missing count, too few values, trailing bytes. *)
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 5; 7] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 2; 1; 5] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 0; 9] = false /\
  (* Non-canonical varints in the arena length and in the value count. *)
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 129; 0; 7; 0] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1; 2; 1; 7; 128; 0] = false /\
  (* Enormous declared lengths are rejected without building them. *)
  pathmap_snapshot_body_canonical
    [69; 80; 77; 49; 1; 2; 1; 7; 255; 255; 255; 255; 255; 255; 255; 255; 255; 1] = false /\
  pathmap_snapshot_body_canonical
    [69; 80; 77; 49; 1; 2; 255; 255; 255; 255; 255; 255; 255; 255; 255; 1] = false /\
  (* Too short to hold the header. *)
  pathmap_snapshot_body_canonical [] = false /\
  pathmap_snapshot_body_canonical [69; 80; 77; 49; 1] = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** The witnesses reach exact scores under the unmodified reducer, so the
    grammar and the reducer's re-read agree on them. *)
Example witnesses_have_acceptable_reducer_results :
  forallb (fun witness => reducer_result_acceptable (semantic_reduce witness))
    [par_ground_int_witness; par_gbigrat_witness; expr_gbigrat_witness;
     gbigrat_witness; gfixedpoint_witness; gprivate_witness;
     gdeployid_witness; gdeployerid_witness; gsysauthtoken_witness;
     gunforgeable_token_witness] = true.
Proof. vm_compute. reflexivity. Qed.

Example expr_gbigrat_scores_exactly :
  sem_definitely_refused (semantic_reduce expr_gbigrat_witness) = false /\
  sem_complete (semantic_reduce expr_gbigrat_witness) = Some true /\
  sem_score_evidence (semantic_reduce expr_gbigrat_witness) =
    ExactScore (score_constructor 16 [ScoreBytes [3]; ScoreBytes [5]]).
Proof. vm_compute. repeat split; reflexivity. Qed.

Example par_gbigrat_scores_exactly :
  sem_definitely_refused (semantic_reduce par_gbigrat_witness) = false /\
  exact_evidence (semantic_reduce par_gbigrat_witness) = true.
Proof. vm_compute. split; reflexivity. Qed.

(** ** What the current reducer returns on the auxiliary witnesses

    Recorded with its exact values.  Every tree that holds an auxiliary leaf
    is refused: the leaf has no generated program (RPB3SemanticReducer.v
    lines 176-181), so it is refused at line 2304; its parent's [scan_child]
    then fails at line 201 and the parent is refused at line 2307.  The
    empty EPathMap is refused at line 2308 for its absent mandatory
    snapshot. *)

Example auxiliary_witnesses_have_acceptable_reducer_results :
  forallb (fun witness => reducer_result_acceptable (semantic_reduce witness))
    [new_with_map_entry_witness; epathmap_with_snapshot_witness;
     ezipper_with_snapshot_witness; epathmap_with_default_value_witness;
     empty_ezipper_witness; empty_epathmap_witness;
     string_map_leaf [10; 1; 97]; pathmap_leaf empty_snapshot_body] = true.
Proof. vm_compute. reflexivity. Qed.

Example new_with_map_entry_reducer_result :
  let summary := semantic_reduce new_with_map_entry_witness in
  sem_type_id summary = 20 /\
  sem_definitely_refused summary = true /\
  sem_complete summary = Some true /\
  sem_score_evidence summary = ScoreNotDerived /\
  class_result_ok summary = false /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example epathmap_with_snapshot_reducer_result :
  let summary := semantic_reduce epathmap_with_snapshot_witness in
  sem_type_id summary = 31 /\
  sem_definitely_refused summary = true /\
  sem_complete summary = Some false /\
  sem_score_evidence summary = ScoreNotDerived /\
  class_result_ok summary = true /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example ezipper_with_snapshot_reducer_result :
  let summary := semantic_reduce ezipper_with_snapshot_witness in
  sem_type_id summary = 32 /\
  sem_definitely_refused summary = true /\
  sem_complete summary = Some false /\
  sem_score_evidence summary = ScoreNotDerived /\
  class_result_ok summary = true /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example epathmap_with_default_value_reducer_result :
  let summary := semantic_reduce epathmap_with_default_value_witness in
  sem_definitely_refused summary = true /\
  sem_complete summary = Some false /\
  sem_score_evidence summary = ScoreNotDerived /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** The leaves themselves are refused, and their parents are refused because
    of them: removing the leaf from the New leaves it unrefused. *)
Example auxiliary_leaves_are_refused :
  sem_definitely_refused (semantic_reduce (string_map_leaf [10; 1; 97])) = true /\
  sem_definitely_refused (semantic_reduce (pathmap_leaf empty_snapshot_body)) = true /\
  sem_definitely_refused
    (semantic_reduce
      (message_node 0 20 [8; 2; 18; 4; 42; 2; 16; 14]
        (FCons (scalar_leaf 1 Varint [2])
          (FCons
            (message_node 2 1 [42; 2; 16; 14]
              (FCons (message_node 5 24 [16; 14]
                (FCons (scalar_leaf 2 Varint [14]) FNil)) FNil))
            FNil)))) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** The rows of the PathMap payload class that the current reducer reaches
    without refusing: only the incomplete, scoreless row, and only through
    an EZipper with no pathmap child.  An EPathMap always refuses, and an
    empty one is refused for its absent snapshot. *)
Example empty_ezipper_reaches_the_incomplete_row :
  let summary := semantic_reduce empty_ezipper_witness in
  sem_definitely_refused summary = false /\
  sem_complete summary = Some false /\
  sem_score_evidence summary = ScoreNotDerived /\
  class_result_ok summary = true /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example empty_epathmap_is_refused_for_its_absent_snapshot :
  let summary := semantic_reduce empty_epathmap_witness in
  sem_definitely_refused summary = true /\
  sem_complete summary = Some false /\
  sem_score_evidence summary = ScoreNotDerived /\
  reducer_result_acceptable summary = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** Red controls *)

Definition explicit_empty_bigrat_field : raw_field :=
  RawField 36 LengthDelimited Message [10; 0] 2 (Some 25) [] [].

Definition explicit_empty_bigrat_tree : @tree raw_field :=
  Node explicit_empty_bigrat_field
    (FCons (scalar_leaf 1 LengthDelimited []) FNil).

Example explicit_empty_bigrat_fails_tv3_and_tv2 :
  reread_body_canonical explicit_empty_bigrat_field = false /\
  children_scalar_contract_holds explicit_empty_bigrat_field
    (FCons (scalar_leaf 1 LengthDelimited []) FNil) = false.
Proof. vm_compute. split; reflexivity. Qed.

Theorem explicit_empty_bigrat_is_not_typed_valid :
  ~ typed_valid_tree explicit_empty_bigrat_tree.
Proof. not_typed_valid_by_compute. Qed.

Definition reread_probe (type_id : nat) (body : list nat) : raw_field :=
  RawField 1 LengthDelimited Message body (length body) (Some type_id) [] [].

Example reread_grammar_gbigrat_controls :
  reread_body_canonical (reread_probe 25 []) = true /\
  reread_body_canonical (reread_probe 25 [10; 1; 3]) = true /\
  reread_body_canonical (reread_probe 25 [18; 1; 5]) = true /\
  reread_body_canonical (reread_probe 25 [10; 1; 3; 18; 1; 5]) = true /\
  reread_body_canonical (reread_probe 25 [10; 0]) = false /\
  reread_body_canonical (reread_probe 25 [18; 0]) = false /\
  reread_body_canonical (reread_probe 25 [18; 1; 5; 10; 1; 3]) = false /\
  reread_body_canonical (reread_probe 25 [10; 1; 3; 10; 1; 4]) = false /\
  reread_body_canonical (reread_probe 25 [10; 129; 0; 3]) = false /\
  reread_body_canonical (reread_probe 25 [10; 2; 3]) = false /\
  reread_body_canonical (reread_probe 25 [10; 1; 3; 1]) = false /\
  reread_body_canonical (reread_probe 25 [8; 3]) = false /\
  reread_body_canonical (reread_probe 25 [26; 1; 9]) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example reread_grammar_gfixedpoint_controls :
  reread_body_canonical (reread_probe 26 []) = true /\
  reread_body_canonical (reread_probe 26 [10; 1; 4]) = true /\
  reread_body_canonical (reread_probe 26 [16; 1]) = true /\
  reread_body_canonical (reread_probe 26 [10; 1; 4; 16; 2]) = true /\
  reread_body_canonical (reread_probe 26 [16; 255; 255; 255; 255; 15]) = true /\
  reread_body_canonical (reread_probe 26 [16; 0]) = false /\
  reread_body_canonical (reread_probe 26 [16; 128; 128; 128; 128; 16]) = false /\
  reread_body_canonical (reread_probe 26 [16; 128; 0]) = false /\
  reread_body_canonical (reread_probe 26 [18; 1; 2]) = false /\
  reread_body_canonical (reread_probe 26 [10; 0; 16; 2]) = false /\
  reread_body_canonical (reread_probe 26 [16; 2; 10; 1; 4]) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example reread_grammar_bytes_leaf_controls :
  reread_body_canonical (reread_probe 61 []) = true /\
  reread_body_canonical (reread_probe 61 [10; 1; 7]) = true /\
  reread_body_canonical (reread_probe 62 [10; 1; 7]) = true /\
  reread_body_canonical (reread_probe 63 [10; 1; 7]) = true /\
  reread_body_canonical (reread_probe 61 [10; 0]) = false /\
  reread_body_canonical (reread_probe 62 [10; 1; 7; 10; 1; 8]) = false /\
  reread_body_canonical (reread_probe 63 [18; 1; 7]) = false /\
  reread_body_canonical (reread_probe 64 []) = true /\
  reread_body_canonical (reread_probe 64 [9]) = false /\
  reread_body_canonical (reread_probe 1 [9; 9; 9]) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** Each typed-validity condition can fail independently of the others. *)

Definition wrong_tv1_message : @tree raw_field :=
  Node (RawField 1 LengthDelimited Message [] 0 (Some 65) [] []) FNil.

(** Not a red control any more.  Under the first form of TV1 this tree was
    the control [wrong_tv1_auxiliary]: a StringMap leaf was not typed-valid
    because it has no generated program.  That is exactly the coverage gap
    the auxiliary-aware TV1 closes.  An entry with both fields omitted is
    the default map entry, which the decoder accepts
    ([parse_string_map_entry_spans [] = Some (None, None)],
    RPB3RawRegionTree.v:1148), so the tree is now a positive witness, and
    [red_controls_are_not_typed_valid] below uses malformed auxiliary nodes
    in its place. *)
Definition empty_map_entry_leaf : @tree raw_field :=
  Node (RawField 4 LengthDelimited StringMap [] 0 (Some 1) [] []) FNil.

Example empty_map_entry_leaf_failed_the_original_tv1 :
  generated_wire_program (tree_field empty_map_entry_leaf) = None.
Proof. reflexivity. Qed.

Theorem empty_map_entry_leaf_is_typed_valid :
  typed_valid_tree empty_map_entry_leaf.
Proof. typed_valid_by_compute. Qed.

Definition wrong_tv2_wire : @tree raw_field :=
  message_node 0 24 [16; 14] (FCons (scalar_leaf 2 LengthDelimited [14]) FNil).

Definition wrong_tv2_body : @tree raw_field :=
  message_node 0 1 [80; 2] (FCons (scalar_leaf 10 Varint [2]) FNil).

Definition wrong_tv2_tag : @tree raw_field :=
  message_node 0 1 [24; 1] (FCons (scalar_leaf 3 Varint [1]) FNil).

Definition wrong_tv3_body : @tree raw_field :=
  message_node 36 25 [1; 2; 3] FNil.

Definition wrong_tv4_grandchild : @tree raw_field :=
  message_node 0 1 [42; 2; 18; 0]
    (FCons (message_node 5 24 [18; 0]
      (FCons wrong_tv3_body FNil)) FNil).

(** Malformed auxiliary nodes: a map entry with its fields reversed and a
    snapshot with the wrong magic, alone and inside the parents that carry
    them.  The parents are otherwise well formed, so these fail TV1 only at
    the auxiliary leaf, and TV4 carries that up the tree. *)

Definition malformed_map_entry_leaf : @tree raw_field :=
  string_map_leaf [18; 1; 5; 10; 1; 97].

Definition new_with_malformed_map_entry : @tree raw_field :=
  message_node 0 20 [34; 6; 18; 1; 5; 10; 1; 97]
    (FCons malformed_map_entry_leaf FNil).

Definition malformed_snapshot_body : list nat :=
  [69; 80; 77; 48; 1; 2; 1; 7; 0].

Definition malformed_pathmap_leaf : @tree raw_field :=
  pathmap_leaf malformed_snapshot_body.

Definition epathmap_with_malformed_snapshot : @tree raw_field :=
  message_node 0 31 ([74; 9] ++ malformed_snapshot_body)
    (FCons malformed_pathmap_leaf FNil).

Definition ezipper_with_malformed_snapshot : @tree raw_field :=
  message_node 0 32 ([10; 11; 74; 9] ++ malformed_snapshot_body)
    (FCons (message_node 1 31 ([74; 9] ++ malformed_snapshot_body)
      (FCons malformed_pathmap_leaf FNil)) FNil).

Theorem red_controls_are_not_typed_valid :
  ~ typed_valid_tree wrong_tv1_message /\
  (* The conjunct for the former [wrong_tv1_auxiliary] is removed because
     that tree is now typed-valid; see [empty_map_entry_leaf]. *)
  ~ typed_valid_tree malformed_map_entry_leaf /\
  ~ typed_valid_tree new_with_malformed_map_entry /\
  ~ typed_valid_tree malformed_pathmap_leaf /\
  ~ typed_valid_tree epathmap_with_malformed_snapshot /\
  ~ typed_valid_tree ezipper_with_malformed_snapshot /\
  ~ typed_valid_tree wrong_tv2_wire /\
  ~ typed_valid_tree wrong_tv2_body /\
  ~ typed_valid_tree wrong_tv2_tag /\
  ~ typed_valid_tree wrong_tv3_body /\
  ~ typed_valid_tree wrong_tv4_grandchild.
Proof.
  repeat split; not_typed_valid_by_compute.
Qed.

(** Every clause of [auxiliary_leaf_shape] and of the two grammars can fail
    on its own. *)

Definition auxiliary_leaf_with (tag : nat) (wire : wire_kind)
    (payload : payload_kind) (body : list nat) (declared : nat)
    (child : option nat) : @tree raw_field :=
  Node (RawField tag wire payload body declared child [] []) FNil.

Definition valid_snapshot_body : list nat :=
  [69; 80; 77; 49; 1; 2; 1; 7; 0].

Definition malformed_auxiliary_nodes : list (@tree raw_field) :=
  [ (* A StringMap with the wrong wire, no value type, or a value type that
       no generated map carries. *)
    auxiliary_leaf_with 4 Varint StringMap [] 0 (Some 1);
    auxiliary_leaf_with 4 LengthDelimited StringMap [] 0 None;
    auxiliary_leaf_with 4 LengthDelimited StringMap [] 0 (Some 24);
    auxiliary_leaf_with 4 LengthDelimited StringMap [] 0 (Some 999);
    (* A StringMap with children or with requirement lists. *)
    Node (RawField 4 LengthDelimited StringMap [] 0 (Some 1) [] [])
      (FCons (scalar_leaf 1 Varint [2]) FNil);
    Node (RawField 4 LengthDelimited StringMap [] 0 (Some 1) [3] []) FNil;
    Node (RawField 4 LengthDelimited StringMap [] 0 (Some 1) [] [3]) FNil;
    (* A declared length that disagrees with the body, and an out-of-range
       body byte. *)
    auxiliary_leaf_with 4 LengthDelimited StringMap [10; 1; 97] 9 (Some 1);
    string_map_leaf [18; 1; 256];
    (* Map entries the decoder's reader rejects. *)
    malformed_map_entry_leaf;
    string_map_leaf [10; 1; 97; 10; 1; 98];
    string_map_leaf [10; 0];
    string_map_leaf [10; 1; 255];
    string_map_leaf [18; 0];
    string_map_leaf [26; 1; 3];
    string_map_leaf [10; 5; 97];
    (* A PathMap with a child type id, the wrong wire, children, a
       requirement list, or a declared length that disagrees with the body. *)
    auxiliary_leaf_with 9 LengthDelimited PathMap valid_snapshot_body 9 (Some 31);
    auxiliary_leaf_with 9 Varint PathMap valid_snapshot_body 9 None;
    Node (RawField 9 LengthDelimited PathMap valid_snapshot_body 9 None [] [])
      (FCons (scalar_leaf 1 Varint [2]) FNil);
    Node (RawField 9 LengthDelimited PathMap valid_snapshot_body 9 None [3] [])
      FNil;
    auxiliary_leaf_with 9 LengthDelimited PathMap valid_snapshot_body 8 None;
    (* Snapshots the EPM1 reader rejects. *)
    malformed_pathmap_leaf;
    pathmap_leaf [];
    pathmap_leaf [69; 80; 77; 49; 2; 2; 1; 7; 0];
    pathmap_leaf [69; 80; 77; 49; 1; 3; 1; 7; 0];
    pathmap_leaf [69; 80; 77; 49; 1; 2; 5; 7];
    pathmap_leaf [69; 80; 77; 49; 1; 2; 1; 7; 0; 9];
    pathmap_leaf [69; 80; 77; 49; 1; 1; 1; 7; 1; 1; 5];
    pathmap_leaf [69; 80; 77; 49; 1; 0; 1; 7; 0] ].

Example malformed_auxiliary_nodes_fail_the_checker :
  forallb (fun subject => negb (typed_valid_treeb subject))
    malformed_auxiliary_nodes = true.
Proof. vm_compute. reflexivity. Qed.

Theorem malformed_auxiliary_nodes_are_not_typed_valid :
  forall subject,
    In subject malformed_auxiliary_nodes ->
    ~ typed_valid_tree subject.
Proof.
  intros subject member valid.
  pose proof (proj1 (forallb_forall _ _)
    malformed_auxiliary_nodes_fail_the_checker subject member) as rejected.
  cbv beta in rejected.
  rewrite (proj2 (typed_valid_treeb_reflect subject) valid) in rejected.
  discriminate rejected.
Qed.

(** The nearest well-formed neighbours pass, so the failures above are
    caused by the clause each control breaks. *)
Example auxiliary_red_controls_have_green_neighbours :
  typed_valid_treeb (pathmap_leaf valid_snapshot_body) = true /\
  typed_valid_treeb (string_map_leaf [10; 1; 97]) = true /\
  typed_valid_treeb
    (auxiliary_leaf_with 4 LengthDelimited StringMap [10; 1; 97] 3 (Some 1)) = true /\
  typed_valid_treeb
    (auxiliary_leaf_with 9 LengthDelimited PathMap valid_snapshot_body 9 None) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** The closed counterexample to the five-type TV3 (deviation D2)

    With TV3 restricted to the design's five types, the GUnforgeable tree
    below passes every condition, yet the reducer leaves it unrefused and
    scoreless on a complete NameComponent.  Its token has a nonempty body
    and no children; Rust refuses such a token in its own frame, but a raw
    field tree does not tie the body to the children. *)

Definition five_type_reread_body_canonical (field : raw_field) : bool :=
  match raw_program_type_id field with
  | Some 64 => true
  | _ => reread_body_canonical field
  end.

Definition five_type_node_typed_valid (field : raw_field)
    (children : @forest raw_field) : bool :=
  structurally_admissibleb field children &&
  children_scalar_contract_holds field children &&
  five_type_reread_body_canonical field.

Definition gsysauthtoken_limbo_tree : @tree raw_field :=
  message_node 7 60 [34; 1; 9] (FCons (message_node 4 64 [9] FNil) FNil).

Theorem five_type_tv3_leaves_gsysauthtoken_in_limbo :
  every_node five_type_node_typed_valid gsysauthtoken_limbo_tree = true /\
  typed_valid_treeb gsysauthtoken_limbo_tree = false /\
  sem_definitely_refused (semantic_reduce gsysauthtoken_limbo_tree) = false /\
  sem_complete (semantic_reduce gsysauthtoken_limbo_tree) = Some true /\
  sem_score_evidence (semantic_reduce gsysauthtoken_limbo_tree) =
    ScoreNotDerived /\
  reducer_result_acceptable (semantic_reduce gsysauthtoken_limbo_tree) = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** Differential check of TV3 against the reducer's own re-read

    The grammar is written independently of the decoder.  This compares
    the two on every byte string of length at most three over an alphabet
    chosen to hit the keys, canonical and non-canonical varint bytes and
    out-of-range bytes, and on every sequence of at most three field-shaped
    chunks. *)

Definition reread_summary (tag type_id : nat) (body : list nat)
    : semantic_summary :=
  SemanticSummary tag LengthDelimited Message type_id body false (Some true)
    ScoreNotDerived false false.

Definition option_has_value {A : Type} (value : option A) : bool :=
  match value with
  | Some _ => true
  | None => false
  end.

Definition reducer_rereads_body (type_id : nat) (body : list nat) : bool :=
  if type_id =? 25 then
    option_has_value (exact_bigrat_parent_score (reread_summary 36 25 body))
  else if type_id =? 26 then
    option_has_value (exact_fixed_point_parent_score (reread_summary 37 26 body))
  else if type_id =? 64 then
    option_has_value (exact_unforgeable_score [reread_summary 4 64 body])
  else option_has_value (exact_unforgeable_leaf_bytes type_id body).

Fixpoint byte_words (alphabet : list nat) (word_length : nat)
    : list (list nat) :=
  match word_length with
  | O => [[]]
  | S shorter =>
      flat_map (fun first => map (cons first) (byte_words alphabet shorter))
        alphabet
  end.

Definition short_bodies : list (list nat) :=
  flat_map (byte_words [0; 1; 2; 5; 8; 10; 16; 18; 127; 128; 129; 255; 256])
    [0; 1; 2; 3].

Fixpoint chunk_sequences (chunks : list (list nat)) (count : nat)
    : list (list nat) :=
  match count with
  | O => [[]]
  | S fewer =>
      flat_map (fun chunk => map (app chunk) (chunk_sequences chunks fewer))
        chunks
  end.

Definition field_chunks : list (list nat) :=
  [[10; 0]; [10; 1; 5]; [10; 2; 7; 7]; [18; 0]; [18; 1; 5]; [16; 0]; [16; 2];
   [16; 200; 1]; [16; 128; 0]; [16; 255; 255; 255; 255; 15];
   [16; 128; 128; 128; 128; 16]; [8; 5]; [26; 1; 3]; [34; 1; 9]; [1];
   [129; 0]; [200]; [10; 129; 0; 3]; [10; 200; 1]].

Definition structured_bodies : list (list nat) :=
  flat_map (chunk_sequences field_chunks) [0; 1; 2; 3].

Definition differential_candidates : list (list nat) :=
  short_bodies ++ structured_bodies.

(** The one candidate the [vm_compute] sweep below leaves out.  Its first
    key is the two bytes [200; 1] and that field's value is the byte 16, so
    its second key is the five-byte varint of 2^32 - 1, whose field number
    536870911 is the largest [field_number_within_limit] accepts.
    [framing_field] binds [N.to_nat] of that number in a [let] before it
    looks at the wire type, so the call-by-value VM builds all 536870911
    unary [S] cells, about 8.6 GiB, even though wire type 7 then rejects
    the key and the number is never read.  With this body included the
    sweep was killed at a 16 GiB memory cap.  Lazy reduction does not force
    the unused binding, so the same agreement is proved for this body by
    [lazy]; every candidate is therefore covered. *)
Definition maximal_key_body : list nat :=
  [200; 1; 16; 255; 255; 255; 255; 15].

Definition vm_differential_candidates : list (list nat) :=
  filter (fun body => negb (nat_list_eqb body maximal_key_body))
    differential_candidates.

Example vm_differential_omits_exactly_one_candidate :
  N.of_nat (length differential_candidates) = 9620%N /\
  N.of_nat (length vm_differential_candidates) = 9619%N.
Proof. vm_compute. split; reflexivity. Qed.

Example reread_grammar_agrees_with_the_reducer_reread :
  forallb (fun type_id =>
    forallb (fun body =>
      Bool.eqb (reread_body_canonical (reread_probe type_id body))
        (reducer_rereads_body type_id body))
      vm_differential_candidates)
    [25; 26; 61; 62; 63; 64] = true.
Proof. vm_compute. reflexivity. Qed.

Example reread_grammar_agrees_on_the_maximal_key_body :
  forallb (fun type_id =>
    Bool.eqb (reread_body_canonical (reread_probe type_id maximal_key_body))
      (reducer_rereads_body type_id maximal_key_body))
    [25; 26; 61; 62; 63; 64] = true.
Proof. lazy. reflexivity. Qed.

(** ** Differential checks of the auxiliary grammars against the readers

    The grammars are written independently of the decoder, so these compare
    two artifacts.  The map-entry reference uses the framing of
    [decode_string_map_entry_source], [parse_string_map_entry_spans] and
    [reduce_map_key], then the byte-level part of [reduce_map_par]: a
    present value is nonempty.  [worklist_region] rejects any byte outside
    0..255 in the region, while the grammar leaves payload bytes to
    [valid_raw_body], so the comparison conjoins that byte check on the
    grammar side.  The PathMap reference is [EPM1.decode_epm1], which, like
    the grammar, checks byte range only in varint positions.

    The candidates contain no multi-group varint that could reach a key
    position with a large field number, and no large EPM1 varint, so the
    unary numbers involved stay small (the hazard recorded at
    [maximal_key_body]). *)

Definition bytes_below_256 (body : list nat) : bool :=
  forallb (fun byte => byte <? 256) body.

Definition map_entry_reference (body : list nat) : bool :=
  match RPB3WireCursor.worklist_region body 0 (length body) true with
  | RPB3WireCursor.Parsed spans =>
      match RPB3RawRegionTree.parse_string_map_entry_spans spans with
      | Some (key_span, value_span) =>
          let key_source :=
            option_map (fun span =>
              (span, RPB3RawRegionTree.map_span_body body span)) key_span in
          match RPB3RawRegionTree.reduce_map_key key_source with
          | Some _ =>
              match value_span with
              | Some span =>
                  match RPB3RawRegionTree.map_span_body body span with
                  | [] => false
                  | _ :: _ => true
                  end
              | None => true
              end
          | None => false
          end
      | None => false
      end
  | _ => false
  end.

Definition map_entry_chunks : list (list nat) :=
  [[10; 0]; [10; 1; 97]; [10; 2; 195; 169]; [10; 1; 255]; [10; 2; 195; 1];
   [18; 0]; [18; 1; 5]; [18; 2; 42; 0]; [18; 1; 256];
   [8; 1]; [26; 1; 3]; [1]; [200; 1; 1];
   [10; 129; 0; 97]; [18; 129; 0; 5]; [10; 5; 97]; [18; 9; 7];
   [10; 128; 2]; [18; 128; 2]].

Definition map_entry_candidates : list (list nat) :=
  flat_map (chunk_sequences map_entry_chunks) [0; 1; 2; 3].

Definition pathmap_headers : list (list nat) :=
  [[69; 80; 77; 49; 1; 0]; [69; 80; 77; 49; 1; 1]; [69; 80; 77; 49; 1; 2];
   [69; 80; 77; 49; 1; 3]; [69; 80; 77; 48; 1; 2]; [69; 80; 77; 49; 2; 2];
   [69; 80; 77; 49; 1]].

Definition pathmap_arena_frames : list (list nat) :=
  [[0]; [1; 7]; [2; 7; 256]; [3; 7]; [129; 0; 7]; [256]].

Definition pathmap_value_counts : list (list nat) :=
  [[0]; [1]; [2]; [128; 0]; [256]].

Definition pathmap_value_frames : list (list nat) :=
  [[]; [0]; [1; 5]; [2; 5]; [1; 5; 1; 6]; [0; 0]; [1; 256]; [200]].

Definition pathmap_trailers : list (list nat) := [[]; [9]].

Definition pathmap_candidates : list (list nat) :=
  flat_map (fun header =>
    flat_map (fun arena =>
      flat_map (fun count =>
        flat_map (fun values =>
          map (fun trailer => header ++ arena ++ count ++ values ++ trailer)
            pathmap_trailers)
          pathmap_value_frames)
        pathmap_value_counts)
      pathmap_arena_frames)
    pathmap_headers.

Example auxiliary_differentials_cover_their_candidates :
  N.of_nat (length map_entry_candidates) = 7240%N /\
  N.of_nat (length pathmap_candidates) = 3360%N.
Proof. vm_compute. split; reflexivity. Qed.

Example map_entry_grammar_agrees_with_the_decoder :
  forallb (fun body =>
    Bool.eqb (bytes_below_256 body && string_map_entry_body_canonical body)
      (map_entry_reference body))
    map_entry_candidates = true.
Proof. vm_compute. reflexivity. Qed.

Example pathmap_grammar_agrees_with_epm1 :
  forallb (fun body =>
    Bool.eqb (pathmap_snapshot_body_canonical body)
      (option_has_value (EPM1.decode_epm1 body)))
    pathmap_candidates = true.
Proof. vm_compute. reflexivity. Qed.

(** The real snapshots of the typed-reference witnesses agree as well. *)
Example pathmap_grammar_accepts_the_typed_reference_snapshots :
  pathmap_snapshot_body_canonical empty_snapshot_body = true /\
  pathmap_snapshot_body_canonical default_value_snapshot_body = true /\
  option_has_value (EPM1.decode_epm1 empty_snapshot_body) = true /\
  option_has_value (EPM1.decode_epm1 default_value_snapshot_body) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

(** ** What the current reducer does with auxiliary payloads

    Line numbers are those of RPB3SemanticReducer.v.  An auxiliary field has
    no generated program (lines 176-181), so [reduce_semantic_field] refuses
    it at line 2304.  A refused child makes the parent's [scan_child] fail
    at line 201, which refuses the parent at line 2307, so the refusal
    reaches the root.  An EPathMap is refused even without an auxiliary
    child, by the mandatory snapshot check at line 2308.  These are general
    statements about the current reducer, not about the witnesses. *)

Lemma no_program_field_is_refused_by_the_current_reducer :
  forall field children,
    generated_wire_program field = None ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children no_program.
  unfold reduce_semantic_field.
  rewrite no_program.
  reflexivity.
Qed.

Theorem auxiliary_field_is_refused_by_the_current_reducer :
  forall field children,
    raw_payload field = StringMap \/ raw_payload field = PathMap ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children [payload|payload];
    apply no_program_field_is_refused_by_the_current_reducer;
    unfold generated_wire_program;
    rewrite payload;
    destruct (raw_program_type_id field); reflexivity.
Qed.

Lemma scan_child_keeps_invalid :
  forall program state child,
    scan_valid state = false ->
    scan_child program state child = state.
Proof.
  intros program state child invalid.
  unfold scan_child.
  rewrite invalid.
  reflexivity.
Qed.

Lemma fold_scan_keeps_invalid :
  forall program children state,
    scan_valid state = false ->
    fold_left (scan_child program) children state = state.
Proof.
  intros program children.
  induction children as [|child rest rest_keeps]; intros state invalid.
  - reflexivity.
  - cbn [fold_left].
    rewrite (scan_child_keeps_invalid program state child invalid).
    exact (rest_keeps state invalid).
Qed.

Lemma refused_child_invalidates_scan :
  forall program state child,
    sem_definitely_refused child = true ->
    scan_valid (scan_child program state child) = false.
Proof.
  intros program state child refused.
  unfold scan_child.
  destruct (scan_valid state) eqn:state_valid.
  - destruct (resolve_tag (program_fields program) (sem_tag child))
      as [descriptor|].
    + cbv beta iota zeta.
      rewrite refused.
      reflexivity.
    + reflexivity.
  - exact state_valid.
Qed.

Lemma refused_child_keeps_fold_invalid :
  forall program children state,
    existsb sem_definitely_refused children = true ->
    scan_valid (fold_left (scan_child program) children state) = false.
Proof.
  intros program children.
  induction children as [|child rest rest_invalid];
    intros state refused_somewhere.
  - discriminate refused_somewhere.
  - cbn [fold_left].
    cbn [existsb] in refused_somewhere.
    apply orb_prop in refused_somewhere as [here|later].
    + rewrite fold_scan_keeps_invalid.
      * exact (refused_child_invalidates_scan program state child here).
      * exact (refused_child_invalidates_scan program state child here).
    + exact (rest_invalid (scan_child program state child) later).
Qed.

Lemma invalid_scan_is_refused :
  forall field children program,
    generated_wire_program field = Some program ->
    scan_valid (fold_left (scan_child program) children empty_scan) = false ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children program resolved invalid.
  unfold reduce_semantic_field.
  rewrite resolved.
  cbn [sem_definitely_refused].
  rewrite invalid.
  cbn [negb].
  rewrite orb_true_r.
  reflexivity.
Qed.

Lemma refused_child_refuses_node :
  forall field children,
    existsb sem_definitely_refused children = true ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children refused_child.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - apply (invalid_scan_is_refused field children program resolved).
    exact (refused_child_keeps_fold_invalid program children empty_scan
      refused_child).
  - exact (no_program_field_is_refused_by_the_current_reducer
      field children resolved).
Qed.

Fixpoint tree_has_auxiliary (subject : @tree raw_field) : bool :=
  match subject with
  | Node field children =>
      match raw_payload field with
      | StringMap | PathMap => true
      | Scalar | Message => forest_has_auxiliary children
      end
  end
with forest_has_auxiliary (subjects : @forest raw_field) : bool :=
  match subjects with
  | FNil => false
  | FCons subject rest =>
      tree_has_auxiliary subject || forest_has_auxiliary rest
  end.

Lemma tree_with_auxiliary_node_is_refused_mutual :
  (forall subject : @tree raw_field,
    tree_has_auxiliary subject = true ->
    sem_definitely_refused (semantic_reduce subject) = true) /\
  (forall subjects : @forest raw_field,
    forest_has_auxiliary subjects = true ->
    existsb sem_definitely_refused (semantic_reduce_forest subjects) = true).
Proof.
  apply tree_forest_induction.
  - intros field children children_refused has_auxiliary.
    cbn [tree_has_auxiliary] in has_auxiliary.
    cbn [semantic_reduce].
    destruct (raw_payload field) eqn:payload.
    + apply refused_child_refuses_node.
      exact (children_refused has_auxiliary).
    + apply refused_child_refuses_node.
      exact (children_refused has_auxiliary).
    + apply auxiliary_field_is_refused_by_the_current_reducer.
      left. exact payload.
    + apply auxiliary_field_is_refused_by_the_current_reducer.
      right. exact payload.
  - intro has_auxiliary.
    discriminate has_auxiliary.
  - intros subject subject_refused rest rest_refused has_auxiliary.
    cbn [forest_has_auxiliary] in has_auxiliary.
    apply orb_prop in has_auxiliary as [here|later].
    + cbn [semantic_reduce_forest existsb].
      rewrite (subject_refused here).
      reflexivity.
    + cbn [semantic_reduce_forest existsb].
      rewrite (rest_refused later).
      apply orb_true_r.
Qed.

Theorem tree_with_auxiliary_node_is_refused :
  forall subject : @tree raw_field,
    tree_has_auxiliary subject = true ->
    sem_definitely_refused (semantic_reduce subject) = true.
Proof.
  exact (proj1 tree_with_auxiliary_node_is_refused_mutual).
Qed.

(** The totality obligation for a tree containing an auxiliary node, on the
    current reducer. *)
Theorem tree_with_auxiliary_node_is_acceptable :
  forall subject : @tree raw_field,
    tree_has_auxiliary subject = true ->
    reducer_result_acceptable (semantic_reduce subject) = true.
Proof.
  intros subject has_auxiliary.
  unfold reducer_result_acceptable.
  rewrite (tree_with_auxiliary_node_is_refused subject has_auxiliary).
  reflexivity.
Qed.

Definition descriptor_accepts_child_type (descriptor : field_descriptor)
    (child : semantic_summary) : bool :=
  match descriptor_payload descriptor with
  | Message =>
      match descriptor_child_type_id descriptor with
      | Some type_id => sem_type_id child =? type_id
      | None => false
      end
  | _ => true
  end.

(** What [scan_child] demands for a scan to stay valid: the state was valid
    and the child is unrefused and fits the descriptor its tag selects. *)
Lemma scan_child_stays_valid_only_if :
  forall program state child next,
    scan_child program state child = next ->
    scan_valid next = true ->
    exists descriptor,
      scan_valid state = true /\
      resolve_tag (program_fields program) (sem_tag child) = Some descriptor /\
      sem_definitely_refused child = false /\
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true /\
      descriptor_accepts_child_type descriptor child = true /\
      scan_seen next = descriptor_slot descriptor :: scan_seen state.
Proof.
  intros program state child next stepped valid.
  unfold scan_child in stepped.
  destruct (scan_valid state) eqn:state_valid.
  2: { cbv beta iota in stepped.
       subst next.
       rewrite state_valid in valid.
       discriminate valid. }
  destruct (resolve_tag (program_fields program) (sem_tag child))
    as [descriptor|] eqn:resolved.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota zeta in stepped.
  match type of stepped with
  | (if ?test then _ else _) = _ => destruct test eqn:admissible
  end.
  2: { cbv beta iota in stepped.
       subst next.
       discriminate valid. }
  cbv beta iota in stepped.
  apply andb_prop in admissible as [first_six unique_ok].
  apply andb_prop in first_six as [first_five follows_ok].
  apply andb_prop in first_five as [first_four range_ok].
  apply andb_prop in first_four as [first_three type_ok].
  apply andb_prop in first_three as [first_two payload_ok].
  apply andb_prop in first_two as [not_refused wire_ok].
  subst next.
  exists descriptor.
  split; [first [exact state_valid|reflexivity]|].
  split; [first [exact resolved|reflexivity]|].
  split; [exact (proj1 (negb_true_iff _) not_refused)|].
  split; [exact payload_ok|].
  split; [exact type_ok|].
  reflexivity.
Qed.

(** If no unrefused child can occupy [slot], the scan never records it. *)
Lemma fold_scan_keeps_slot_unseen :
  forall program slot children state,
    (forall descriptor child,
      In descriptor (program_fields program) ->
      descriptor_slot descriptor = slot ->
      In child children ->
      sem_definitely_refused child = false ->
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true ->
      descriptor_accepts_child_type descriptor child = true ->
      False) ->
    (scan_valid state = false \/ seen_slot state slot = false) ->
    scan_valid (fold_left (scan_child program) children state) = false \/
    seen_slot (fold_left (scan_child program) children state) slot = false.
Proof.
  intros program slot children.
  induction children as [|child rest rest_unseen]; intros state blocked invariant.
  - exact invariant.
  - cbn [fold_left].
    apply rest_unseen.
    + intros descriptor later member same_slot later_member.
      exact (blocked descriptor later member same_slot
        (or_intror later_member)).
    + destruct (scan_valid (scan_child program state child)) eqn:valid_after.
      2: { left. reflexivity. }
      right.
      destruct (scan_child_stays_valid_only_if program state child _
        eq_refl valid_after)
        as [descriptor [was_valid [resolved
          [not_refused [payload_ok [type_ok seen_after]]]]]].
      destruct invariant as [was_invalid|unseen_before].
      { rewrite was_valid in was_invalid. discriminate was_invalid. }
      unfold seen_slot in unseen_before |- *.
      rewrite seen_after.
      cbn [existsb].
      destruct (slot =? descriptor_slot descriptor) eqn:same_slot.
      * exfalso.
        apply Nat.eqb_eq in same_slot.
        exact (blocked descriptor child
          (resolve_tag_member _ _ _ resolved) (eq_sym same_slot)
          (or_introl eq_refl) not_refused payload_ok type_ok).
      * exact unseen_before.
Qed.

Lemma mandatory_pathmaps_absent :
  forall program descriptor state,
    In descriptor (program_fields program) ->
    descriptor_payload descriptor = PathMap ->
    seen_slot state (descriptor_slot descriptor) = false ->
    mandatory_pathmaps_present program state = false.
Proof.
  intros program descriptor state member payload unseen.
  destruct (mandatory_pathmaps_present program state) eqn:all_present.
  2: { reflexivity. }
  exfalso.
  unfold mandatory_pathmaps_present in all_present.
  pose proof (proj1 (forallb_forall _ _) all_present descriptor member) as here.
  cbv beta in here.
  rewrite payload in here.
  cbn [payload_eqb] in here.
  rewrite unseen in here.
  discriminate here.
Qed.

Lemma absent_mandatory_pathmap_is_refused :
  forall field children program,
    generated_wire_program field = Some program ->
    mandatory_pathmaps_present program
      (fold_left (scan_child program) children empty_scan) = false ->
    sem_definitely_refused (reduce_semantic_field field children) = true.
Proof.
  intros field children program resolved absent.
  unfold reduce_semantic_field.
  rewrite resolved.
  cbn [sem_definitely_refused].
  rewrite absent.
  cbn [negb].
  rewrite orb_true_r.
  reflexivity.
Qed.

Lemma reduce_semantic_field_payload :
  forall field children,
    sem_payload (reduce_semantic_field field children) = raw_payload field.
Proof.
  intros field children.
  reflexivity.
Qed.

Lemma reduce_semantic_field_type_id :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_type_id (reduce_semantic_field field children) = program_family program.
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma reduce_semantic_field_complete :
  forall field children program,
    generated_wire_program field = Some program ->
    sem_complete (reduce_semantic_field field children) =
    component_completeness (program_family program)
      (fold_left (scan_child program) children empty_scan) children.
Proof.
  intros field children program resolved.
  unfold reduce_semantic_field.
  rewrite resolved.
  reflexivity.
Qed.

Lemma semantic_reduce_forest_member :
  forall subjects child,
    In child (semantic_reduce_forest subjects) ->
    exists subject, child = semantic_reduce subject.
Proof.
  induction subjects as [|head rest rest_member]; intros child member.
  - cbn [semantic_reduce_forest In] in member.
    destruct member.
  - cbn [semantic_reduce_forest In] in member.
    destruct member as [here|later].
    + exists head. symmetry. exact here.
    + exact (rest_member child later).
Qed.

Lemma reduced_pathmap_summary_is_refused :
  forall subject,
    payload_eqb (sem_payload (semantic_reduce subject)) PathMap = true ->
    sem_definitely_refused (semantic_reduce subject) = true.
Proof.
  intros [field children] is_pathmap.
  cbn [semantic_reduce] in is_pathmap |- *.
  rewrite reduce_semantic_field_payload in is_pathmap.
  destruct (raw_payload field) eqn:payload; try discriminate is_pathmap.
  apply auxiliary_field_is_refused_by_the_current_reducer.
  right.
  exact payload.
Qed.

Lemma lookup_projected_program_family :
  forall type_id program,
    lookup_projected_program type_id [] [] = Some program ->
    program_family program = type_id.
Proof.
  intros type_id program found.
  unfold lookup_projected_program in found.
  match type of found with
  | context [find ?test ?programs] =>
      destruct (find test programs) as [candidate|] eqn:selected
  end.
  2: { discriminate found. }
  cbn [option_map] in found.
  injection found as same_program.
  subst program.
  apply find_some in selected as [_ matched].
  cbv beta in matched.
  apply Nat.eqb_eq in matched.
  exact matched.
Qed.

Lemma generated_program_is_lookup_of_its_family :
  forall field program type_id,
    generated_wire_program field = Some program ->
    program_family program = type_id ->
    type_id <> 0 ->
    program = generated_program_of type_id.
Proof.
  intros field program type_id resolved family nonzero.
  unfold generated_wire_program in resolved.
  destruct (raw_payload field);
    destruct (raw_program_type_id field) as [declared_type|];
    try discriminate resolved.
  - injection resolved as same_program.
    subst program.
    exfalso.
    apply nonzero.
    symmetry.
    exact family.
  - cbv beta iota in resolved.
    pose proof (lookup_projected_program_family _ _ resolved) as same_family.
    assert (same_type : declared_type = type_id) by congruence.
    rewrite same_type in resolved.
    unfold generated_program_of.
    rewrite resolved.
    reflexivity.
Qed.

Definition epathmap_snapshot_descriptor : field_descriptor :=
  FieldDescriptor 9 3 LengthDelimited false PathMap None None.

Lemma epathmap_snapshot_descriptor_is_generated :
  In epathmap_snapshot_descriptor (program_fields (generated_program_of 31)).
Proof.
  apply (resolve_tag_member _ 9).
  vm_compute.
  reflexivity.
Qed.

Definition slot_is_pathmap_payload (slot : nat)
    (descriptor : field_descriptor) : bool :=
  negb (descriptor_slot descriptor =? slot) ||
  payload_eqb (descriptor_payload descriptor) PathMap.

Example epathmap_slot_three_holds_only_the_pathmap :
  forallb (slot_is_pathmap_payload 3)
    (program_fields (generated_program_of 31)) = true.
Proof. vm_compute. reflexivity. Qed.

Definition slot_holds_message_of (slot type_id : nat)
    (descriptor : field_descriptor) : bool :=
  negb (descriptor_slot descriptor =? slot) ||
  match descriptor_payload descriptor, descriptor_child_type_id descriptor with
  | Message, Some child_type => child_type =? type_id
  | _, _ => false
  end.

Example ezipper_slot_zero_holds_only_the_epathmap :
  forallb (slot_holds_message_of 0 31)
    (program_fields (generated_program_of 32)) = true.
Proof. vm_compute. reflexivity. Qed.

(** No EPathMap escapes refusal on the current reducer: its mandatory
    snapshot (slot 3) can only be recorded for a PathMap child, every such
    child is refused, and [scan_child] rejects refused children. *)
Theorem epathmap_is_always_refused_by_the_current_reducer :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 31 ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program resolved family.
  pose proof (generated_program_is_lookup_of_its_family field program 31
    resolved family (ltac:(discriminate) : 31 <> 0)) as same_program.
  subst program.
  assert (blocked :
    forall descriptor child,
      In descriptor (program_fields (generated_program_of 31)) ->
      descriptor_slot descriptor = 3 ->
      In child (semantic_reduce_forest children) ->
      sem_definitely_refused child = false ->
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true ->
      descriptor_accepts_child_type descriptor child = true ->
      False).
  { intros descriptor child member same_slot child_member not_refused
      payload_ok _.
    pose proof (proj1 (forallb_forall _ _)
      epathmap_slot_three_holds_only_the_pathmap descriptor member)
      as only_pathmap.
    unfold slot_is_pathmap_payload in only_pathmap.
    rewrite same_slot in only_pathmap.
    assert (is_pathmap : descriptor_payload descriptor = PathMap).
    { destruct (descriptor_payload descriptor);
        try discriminate only_pathmap; reflexivity. }
    rewrite is_pathmap in payload_ok.
    destruct (semantic_reduce_forest_member children child child_member)
      as [subject same_child].
    subst child.
    rewrite (reduced_pathmap_summary_is_refused subject payload_ok)
      in not_refused.
    discriminate not_refused. }
  destruct (fold_scan_keeps_slot_unseen (generated_program_of 31) 3
    (semantic_reduce_forest children) empty_scan blocked (or_intror eq_refl))
    as [invalid|unseen].
  - exact (invalid_scan_is_refused field _ (generated_program_of 31)
      resolved invalid).
  - apply (absent_mandatory_pathmap_is_refused field _
      (generated_program_of 31) resolved).
    apply (mandatory_pathmaps_absent (generated_program_of 31)
      epathmap_snapshot_descriptor).
    + exact epathmap_snapshot_descriptor_is_generated.
    + reflexivity.
    + exact unseen.
Qed.

Lemma reduced_epathmap_summary_is_refused :
  forall subject,
    sem_type_id (semantic_reduce subject) = 31 ->
    sem_definitely_refused (semantic_reduce subject) = true.
Proof.
  intros [field children] is_epathmap.
  cbn [semantic_reduce] in is_epathmap |- *.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  - rewrite (reduce_semantic_field_type_id field _ program resolved)
      in is_epathmap.
    exact (epathmap_is_always_refused_by_the_current_reducer
      field children program resolved is_epathmap).
  - exact (no_program_field_is_refused_by_the_current_reducer
      field _ resolved).
Qed.

(** An EZipper counts as complete only when its slot 0 holds an EPathMap
    child (RPB3SemanticReducer.v, the [32] arm of [component_completeness]),
    and every EPathMap is refused. *)
Theorem ezipper_complete_row_is_refused_by_the_current_reducer :
  forall field children program,
    generated_wire_program field = Some program ->
    program_family program = 32 ->
    sem_complete
      (reduce_semantic_field field (semantic_reduce_forest children)) =
      Some true ->
    sem_definitely_refused
      (reduce_semantic_field field (semantic_reduce_forest children)) = true.
Proof.
  intros field children program resolved family complete.
  pose proof (generated_program_is_lookup_of_its_family field program 32
    resolved family (ltac:(discriminate) : 32 <> 0)) as same_program.
  subst program.
  rewrite (reduce_semantic_field_complete field _ (generated_program_of 32)
    resolved) in complete.
  rewrite family in complete.
  cbn [component_completeness] in complete.
  injection complete as seen_zero.
  assert (blocked :
    forall descriptor child,
      In descriptor (program_fields (generated_program_of 32)) ->
      descriptor_slot descriptor = 0 ->
      In child (semantic_reduce_forest children) ->
      sem_definitely_refused child = false ->
      payload_eqb (sem_payload child) (descriptor_payload descriptor) = true ->
      descriptor_accepts_child_type descriptor child = true ->
      False).
  { intros descriptor child member same_slot child_member not_refused
      payload_ok type_ok.
    pose proof (proj1 (forallb_forall _ _)
      ezipper_slot_zero_holds_only_the_epathmap descriptor member)
      as only_epathmap.
    unfold slot_holds_message_of in only_epathmap.
    rewrite same_slot in only_epathmap.
    unfold descriptor_accepts_child_type in type_ok.
    destruct (descriptor_payload descriptor);
      [discriminate only_epathmap| |
       discriminate only_epathmap|discriminate only_epathmap].
    destruct (descriptor_child_type_id descriptor) as [child_type|];
      [|discriminate only_epathmap].
    assert (child_type_is_epathmap : child_type = 31).
    { apply Nat.eqb_eq. exact only_epathmap. }
    subst child_type.
    assert (child_is_epathmap : sem_type_id child = 31).
    { apply Nat.eqb_eq. exact type_ok. }
    destruct (semantic_reduce_forest_member children child child_member)
      as [subject same_child].
    subst child.
    rewrite (reduced_epathmap_summary_is_refused subject child_is_epathmap)
      in not_refused.
    discriminate not_refused. }
  destruct (fold_scan_keeps_slot_unseen (generated_program_of 32) 0
    (semantic_reduce_forest children) empty_scan blocked (or_intror eq_refl))
    as [invalid|unseen].
  - exact (invalid_scan_is_refused field _ (generated_program_of 32)
      resolved invalid).
  - exfalso.
    rewrite seen_zero in unseen.
    discriminate unseen.
Qed.

(** The complete row of the PathMap payload class is unreachable without a
    refusal on the current reducer.  [class_result_ok] accepts that row
    deliberately (the design's row for a future reducer with auxiliary
    frames) and is not weakened; on this reducer the row is simply not
    reached unrefused. *)
Theorem pathmap_payload_complete_row_is_refused_by_the_current_reducer :
  forall subject : @tree raw_field,
    (sem_type_id (semantic_reduce subject) = 31 \/
     sem_type_id (semantic_reduce subject) = 32) ->
    sem_complete (semantic_reduce subject) = Some true ->
    sem_definitely_refused (semantic_reduce subject) = true.
Proof.
  intros [field children] is_pathmap_payload complete.
  cbn [semantic_reduce] in is_pathmap_payload, complete |- *.
  destruct (generated_wire_program field) as [program|] eqn:resolved.
  2: { exact (no_program_field_is_refused_by_the_current_reducer
         field _ resolved). }
  rewrite (reduce_semantic_field_type_id field _ program resolved)
    in is_pathmap_payload.
  destruct is_pathmap_payload as [family|family].
  - exact (epathmap_is_always_refused_by_the_current_reducer
      field children program resolved family).
  - exact (ezipper_complete_row_is_refused_by_the_current_reducer
      field children program resolved family complete).
Qed.

Print Assumptions generated_program_present_iff.
Print Assumptions typed_valid_tree_node_iff.
Print Assumptions children_scalar_contract_holds_of_program.
Print Assumptions typed_valid_tree_scalar_child_contract.
Print Assumptions typed_valid_reflection_mutual.
Print Assumptions typed_valid_treeb_reflect.
Print Assumptions typed_valid_forestb_reflect.
Print Assumptions scalar_contract_holds_matches_checked_generated_scalar_raw.
Print Assumptions scalar_contract_depends_on_the_parent.
Print Assumptions scalar_contract_is_not_field_local.
Print Assumptions acceptance_table_scored_classes.
Print Assumptions acceptance_table_payload_classes.
Print Assumptions reducer_result_acceptable_disjuncts.
Print Assumptions par_ground_int_is_typed_valid.
Print Assumptions par_gbigrat_is_typed_valid.
Print Assumptions expr_gbigrat_is_typed_valid.
Print Assumptions gbigrat_field_is_typed_valid.
Print Assumptions gfixedpoint_field_is_typed_valid.
Print Assumptions gprivate_field_is_typed_valid.
Print Assumptions gdeployid_field_is_typed_valid.
Print Assumptions gdeployerid_field_is_typed_valid.
Print Assumptions gsysauthtoken_field_is_typed_valid.
Print Assumptions gunforgeable_token_is_typed_valid.
Print Assumptions witnesses_have_acceptable_reducer_results.
Print Assumptions expr_gbigrat_scores_exactly.
Print Assumptions par_gbigrat_scores_exactly.
Print Assumptions explicit_empty_bigrat_fails_tv3_and_tv2.
Print Assumptions explicit_empty_bigrat_is_not_typed_valid.
Print Assumptions reread_grammar_gbigrat_controls.
Print Assumptions reread_grammar_gfixedpoint_controls.
Print Assumptions reread_grammar_bytes_leaf_controls.
Print Assumptions red_controls_are_not_typed_valid.
Print Assumptions five_type_tv3_leaves_gsysauthtoken_in_limbo.
Print Assumptions vm_differential_omits_exactly_one_candidate.
Print Assumptions reread_grammar_agrees_with_the_reducer_reread.
Print Assumptions reread_grammar_agrees_on_the_maximal_key_body.
Print Assumptions generated_string_map_value_types_are_exactly_par.
Print Assumptions structurally_admissibleb_iff.
Print Assumptions auxiliary_leaf_shape_is_a_childless_auxiliary.
Print Assumptions typed_valid_node_is_generated_or_auxiliary_leaf.
Print Assumptions generated_auxiliary_descriptors_satisfy_the_contract.
Print Assumptions resolve_tag_member.
Print Assumptions lookup_projected_program_field_is_generated.
Print Assumptions wire_eqb_true_implies_equal.
Print Assumptions auxiliary_leaf_shape_intro.
Print Assumptions decoded_auxiliary_leaf_is_structurally_admissible.
Print Assumptions new_with_map_entry_is_the_decoded_tree.
Print Assumptions epathmap_with_snapshot_is_the_decoded_tree.
Print Assumptions ezipper_with_snapshot_is_the_decoded_tree.
Print Assumptions epathmap_with_default_value_is_the_decoded_tree.
Print Assumptions new_with_map_entry_is_typed_valid.
Print Assumptions epathmap_with_snapshot_is_typed_valid.
Print Assumptions ezipper_with_snapshot_is_typed_valid.
Print Assumptions epathmap_with_default_value_is_typed_valid.
Print Assumptions empty_ezipper_is_typed_valid.
Print Assumptions empty_epathmap_is_typed_valid.
Print Assumptions string_map_entry_grammar_controls.
Print Assumptions pathmap_snapshot_grammar_controls.
Print Assumptions auxiliary_witnesses_have_acceptable_reducer_results.
Print Assumptions new_with_map_entry_reducer_result.
Print Assumptions epathmap_with_snapshot_reducer_result.
Print Assumptions ezipper_with_snapshot_reducer_result.
Print Assumptions epathmap_with_default_value_reducer_result.
Print Assumptions auxiliary_leaves_are_refused.
Print Assumptions empty_ezipper_reaches_the_incomplete_row.
Print Assumptions empty_epathmap_is_refused_for_its_absent_snapshot.
Print Assumptions empty_map_entry_leaf_failed_the_original_tv1.
Print Assumptions empty_map_entry_leaf_is_typed_valid.
Print Assumptions malformed_auxiliary_nodes_fail_the_checker.
Print Assumptions malformed_auxiliary_nodes_are_not_typed_valid.
Print Assumptions auxiliary_red_controls_have_green_neighbours.
Print Assumptions auxiliary_differentials_cover_their_candidates.
Print Assumptions map_entry_grammar_agrees_with_the_decoder.
Print Assumptions pathmap_grammar_agrees_with_epm1.
Print Assumptions pathmap_grammar_accepts_the_typed_reference_snapshots.
Print Assumptions no_program_field_is_refused_by_the_current_reducer.
Print Assumptions auxiliary_field_is_refused_by_the_current_reducer.
Print Assumptions scan_child_keeps_invalid.
Print Assumptions fold_scan_keeps_invalid.
Print Assumptions refused_child_invalidates_scan.
Print Assumptions refused_child_keeps_fold_invalid.
Print Assumptions invalid_scan_is_refused.
Print Assumptions refused_child_refuses_node.
Print Assumptions tree_with_auxiliary_node_is_refused_mutual.
Print Assumptions tree_with_auxiliary_node_is_refused.
Print Assumptions tree_with_auxiliary_node_is_acceptable.
Print Assumptions scan_child_stays_valid_only_if.
Print Assumptions fold_scan_keeps_slot_unseen.
Print Assumptions mandatory_pathmaps_absent.
Print Assumptions absent_mandatory_pathmap_is_refused.
Print Assumptions reduce_semantic_field_payload.
Print Assumptions reduce_semantic_field_type_id.
Print Assumptions reduce_semantic_field_complete.
Print Assumptions semantic_reduce_forest_member.
Print Assumptions reduced_pathmap_summary_is_refused.
Print Assumptions lookup_projected_program_family.
Print Assumptions generated_program_is_lookup_of_its_family.
Print Assumptions epathmap_snapshot_descriptor_is_generated.
Print Assumptions epathmap_slot_three_holds_only_the_pathmap.
Print Assumptions ezipper_slot_zero_holds_only_the_epathmap.
Print Assumptions epathmap_is_always_refused_by_the_current_reducer.
Print Assumptions reduced_epathmap_summary_is_refused.
Print Assumptions ezipper_complete_row_is_refused_by_the_current_reducer.
Print Assumptions pathmap_payload_complete_row_is_refused_by_the_current_reducer.
