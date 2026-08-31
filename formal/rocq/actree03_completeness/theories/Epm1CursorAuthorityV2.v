From Stdlib Require Import Arith PeanoNat NArith Lia Lists.List Bool Sorting.Sorted.

Import ListNotations.

(**
  REJECTED CHECKPOINT: Candidate V2 components for cursor-native EPM1
  reconstruction.  This file is not a complete production authority: the
  exact executable ACTree03 byte refinement and generated EpmFrame trace
  binding are intentionally left to the next revision and are listed in
  FORMAL-AUTHORITY.md.

  This file intentionally does not import the rejected transition-simulation
  model.  The logical tree, flat-event parser, direct denotation, streaming
  body machine, PathMap adapter, publication model, and cost semantics are
  defined independently here.  Naturals representing bytes are range checked
  by [is_u8]; no theorem silently identifies an arbitrary natural with a byte.
*)

Definition byte := nat.
Definition path := list byte.
Definition body := nat.
Definition is_u8 (candidate : byte) : Prop := candidate < 256.

Inductive canonical_tree : Type :=
| CNode : option nat -> canonical_forest -> canonical_tree
with canonical_forest : Type :=
| FNil : canonical_forest
| FCons : byte -> canonical_tree -> canonical_forest -> canonical_forest.

Scheme canonical_tree_forest_ind_tree := Induction for canonical_tree Sort Prop
with canonical_tree_forest_ind_forest := Induction for canonical_forest Sort Prop.
Combined Scheme canonical_tree_forest_mutind
  from canonical_tree_forest_ind_tree, canonical_tree_forest_ind_forest.

Fixpoint forest_labels (forest : canonical_forest) : list byte :=
  match forest with
  | FNil => []
  | FCons label _ rest => label :: forest_labels rest
  end.

Fixpoint tree_canonical (tree : canonical_tree) : Prop :=
  match tree with
  | CNode _ children => forest_canonical None children
  end
with forest_canonical (previous : option byte) (forest : canonical_forest) : Prop :=
  match forest with
  | FNil => True
  | FCons label child rest =>
      is_u8 label /\
      (match previous with None => True | Some before => before < label end) /\
      tree_canonical child /\ forest_canonical (Some label) rest
  end.

Fixpoint all_values_zero_tree (tree : canonical_tree) : Prop :=
  match tree with
  | CNode value children =>
      (match value with None => True | Some ordinal => ordinal = 0 end) /\
      all_values_zero_forest children
  end
with all_values_zero_forest (forest : canonical_forest) : Prop :=
  match forest with
  | FNil => True
  | FCons _ child rest =>
      all_values_zero_tree child /\ all_values_zero_forest rest
  end.

Fixpoint tree_ordinals (tree : canonical_tree) : list nat :=
  match tree with
  | CNode value children =>
      match value with
      | None => forest_ordinals children
      | Some ordinal => ordinal :: forest_ordinals children
      end
  end
with forest_ordinals (forest : canonical_forest) : list nat :=
  match forest with
  | FNil => []
  | FCons _ child rest => tree_ordinals child ++ forest_ordinals rest
  end.

Definition root_productive (tree : canonical_tree) : Prop :=
  match tree with
  | CNode value children => value <> None \/ children <> FNil
  end.

Inductive epm_input : Type :=
| NeutralEmptyInput
| TypedSetInput (tree : canonical_tree)
| TypedMapInput (declared_bodies : nat) (tree : canonical_tree).

Definition valid_epm_input (input : epm_input) : Prop :=
  match input with
  | NeutralEmptyInput => True
  | TypedSetInput tree =>
      tree_canonical tree /\ root_productive tree /\ all_values_zero_tree tree
  | TypedMapInput declared tree =>
      tree_canonical tree /\ root_productive tree /\
      tree_ordinals tree = seq 0 declared
  end.

Lemma forest_canonical_lower_bound :
  forall forest lower,
    forest_canonical (Some lower) forest ->
    Forall (fun label => lower < label) (forest_labels forest).
Proof.
  induction forest as [|label child rest IH]; intros lower canonical; simpl.
  - constructor.
  - destruct canonical as [_ [above [_ restCanonical]]].
    constructor; [exact above |].
    specialize (IH label restCanonical).
    rewrite Forall_forall in IH. rewrite Forall_forall.
    intros candidate member. specialize (IH candidate member). lia.
Qed.

Lemma canonical_forest_labels_strongly_sorted :
  forall forest previous,
    forest_canonical previous forest ->
    StronglySorted Nat.lt (forest_labels forest).
Proof.
  induction forest as [|label child rest IH]; intros previous canonical; simpl.
  - constructor.
  - destruct canonical as [_ [_ [_ restCanonical]]].
    constructor.
    + apply (IH (Some label)). exact restCanonical.
    + apply forest_canonical_lower_bound. exact restCanonical.
Qed.

Lemma canonical_forest_labels_nodup :
  forall forest previous,
    forest_canonical previous forest -> NoDup (forest_labels forest).
Proof.
  induction forest as [|label child rest IH]; intros previous canonical; simpl.
  - constructor.
  - destruct canonical as [_ [_ [_ restCanonical]]]. constructor.
    + intro member.
      pose proof (forest_canonical_lower_bound rest label restCanonical) as above.
      rewrite Forall_forall in above. specialize (above label member). lia.
    + apply (IH (Some label)). exact restCanonical.
Qed.

Theorem topology_only_maps_are_valid_when_productive_and_canonical :
  forall tree,
    tree_canonical tree -> root_productive tree -> tree_ordinals tree = [] ->
    valid_epm_input (TypedMapInput 0 tree).
Proof. intros tree canonical productive ordinals. simpl. auto. Qed.

Record extensional_trie : Type := {
  terminal_paths : list path;
  ordinal_bindings : list (path * nat)
}.

Definition append_denotation (left right : extensional_trie) : extensional_trie :=
  {| terminal_paths := terminal_paths left ++ terminal_paths right;
     ordinal_bindings := ordinal_bindings left ++ ordinal_bindings right |}.

(** Direct recursive denotation.  It does not generate flat events and does
    not call any writer, zipper, close, or insertion helper. *)
Fixpoint denote_tree_at (is_root : bool) (prefix : path)
    (tree : canonical_tree) : extensional_trie :=
  match tree with
  | CNode value children =>
      let descendants := denote_forest_at prefix children in
      let self_terminal :=
        match value, children, is_root with
        | Some _, _, _ => [prefix]
        | None, FNil, false => [prefix]
        | _, _, _ => []
        end in
      let self_binding :=
        match value with Some ordinal => [(prefix, ordinal)] | None => [] end in
      {| terminal_paths := self_terminal ++ terminal_paths descendants;
         ordinal_bindings := self_binding ++ ordinal_bindings descendants |}
  end
with denote_forest_at (prefix : path) (forest : canonical_forest)
    : extensional_trie :=
  match forest with
  | FNil => {| terminal_paths := []; ordinal_bindings := [] |}
  | FCons label child rest =>
      append_denotation
        (denote_tree_at false (prefix ++ [label]) child)
        (denote_forest_at prefix rest)
  end.

Definition denote_tree (tree : canonical_tree) : extensional_trie :=
  denote_tree_at true [] tree.

Lemma denotation_paths_extend_focus :
  (forall tree isRoot prefix terminal,
      In terminal (terminal_paths (denote_tree_at isRoot prefix tree)) ->
      exists suffix, terminal = prefix ++ suffix) /\
  (forall forest prefix terminal,
      In terminal (terminal_paths (denote_forest_at prefix forest)) ->
      exists label suffix, terminal = prefix ++ label :: suffix).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH isRoot prefix terminal member.
    destruct value as [ordinal |].
    + simpl in member. destruct member as [equal | member].
      * subst. exists []. now rewrite app_nil_r.
      * destruct (childrenIH prefix terminal member) as [label [suffix same]].
        exists (label :: suffix). exact same.
    + destruct children as [|label child rest].
      * destruct isRoot; simpl in member.
        { contradiction. }
        { destruct member as [equal | impossible]; [|contradiction].
          subst. exists []. now rewrite app_nil_r. }
      * simpl in member.
        destruct (childrenIH prefix terminal member) as [next [suffix same]].
        exists (next :: suffix). exact same.
  - intros prefix terminal member. contradiction.
  - intros label child childIH rest restIH prefix terminal member. simpl in member.
    apply in_app_or in member. destruct member as [inChild | inRest].
    + destruct (childIH false (prefix ++ [label]) terminal inChild) as [suffix same].
      exists label, suffix. rewrite same.
      generalize prefix as head. induction head; simpl; congruence.
    + apply restIH. exact inRest.
Qed.

Lemma value_free_root_is_not_an_endpoint :
  forall children,
    ~ In [] (terminal_paths (denote_tree (CNode None children))).
Proof.
  intros children member. unfold denote_tree in member.
  destruct children as [|label child rest]; simpl in member; [contradiction |].
  destruct (proj2 denotation_paths_extend_focus
    (FCons label child rest) [] [] member) as [next [suffix impossible]].
  discriminate.
Qed.

Example neutral_empty_has_no_typed_tree : valid_epm_input NeutralEmptyInput.
Proof. exact I. Qed.

Example typed_value_free_root_is_rejected :
  ~ valid_epm_input (TypedMapInput 0 (CNode None FNil)).
Proof. simpl. tauto. Qed.

Inductive logical_event : Type :=
| EvValue : option nat -> logical_event
| EvEdge : byte -> logical_event
| EvLeave : logical_event.

Definition neutral_virtual_events : list logical_event :=
  [EvValue None; EvLeave].

Fixpoint emit_tree (tree : canonical_tree) : list logical_event :=
  match tree with
  | CNode value children => EvValue value :: emit_forest children ++ [EvLeave]
  end
with emit_forest (forest : canonical_forest) : list logical_event :=
  match forest with
  | FNil => []
  | FCons label child rest =>
      EvEdge label :: emit_tree child ++ emit_forest rest
  end.

Fixpoint tree_size (tree : canonical_tree) : nat :=
  match tree with CNode _ children => S (forest_size children) end
with forest_size (forest : canonical_forest) : nat :=
  match forest with
  | FNil => 0
  | FCons _ child rest => S (tree_size child + forest_size rest)
  end.

(** Deterministic prefix parser.  [parse_forest_fuel] owns and consumes the
    unique closing Leave of its containing node. *)
Fixpoint parse_tree_fuel (fuel : nat) (events : list logical_event)
    : option (canonical_tree * list logical_event) :=
  match fuel, events with
  | 0, _ => None
  | S remaining, EvValue value :: rest =>
      match parse_forest_fuel remaining rest with
      | Some (children, tail) => Some (CNode value children, tail)
      | None => None
      end
  | _, _ => None
  end
with parse_forest_fuel (fuel : nat) (events : list logical_event)
    : option (canonical_forest * list logical_event) :=
  match fuel, events with
  | 0, _ => None
  | S _, EvLeave :: rest => Some (FNil, rest)
  | S remaining, EvEdge label :: rest =>
      match parse_tree_fuel remaining rest with
      | None => None
      | Some (child, after_child) =>
          match parse_forest_fuel remaining after_child with
          | None => None
          | Some (siblings, tail) => Some (FCons label child siblings, tail)
          end
      end
  | _, _ => None
  end.

Definition parse_event_program (events : list logical_event)
    : option canonical_tree :=
  match parse_tree_fuel (S (length events)) events with
  | Some (tree, []) => Some tree
  | _ => None
  end.

Lemma parse_sound_mutual : forall fuel,
  (forall events tree tail,
      parse_tree_fuel fuel events = Some (tree, tail) ->
      events = emit_tree tree ++ tail) /\
  (forall events forest tail,
      parse_forest_fuel fuel events = Some (forest, tail) ->
      events = emit_forest forest ++ EvLeave :: tail).
Proof.
  induction fuel as [|fuel [treeIH forestIH]].
  - split; intros; discriminate.
  - split.
    + intros events tree tail parsed.
      destruct events as [|event rest]; [discriminate |].
      destruct event; try discriminate.
      simpl in parsed.
      destruct (parse_forest_fuel fuel rest) as [[children after] |]
        eqn:childrenParsed; [|discriminate].
      inversion parsed. subst.
      specialize (forestIH _ _ _ childrenParsed). subst.
      simpl. f_equal.
      generalize (emit_forest children) as prefix. induction prefix; simpl; congruence.
    + intros events forest tail parsed.
      destruct events as [|event rest]; [discriminate |].
      destruct event as [value|label|].
      * discriminate.
      * simpl in parsed.
        destruct (parse_tree_fuel fuel rest) as [[child afterChild] |]
          eqn:childParsed; [|discriminate].
        destruct (parse_forest_fuel fuel afterChild) as [[siblings after] |]
          eqn:siblingsParsed; [|discriminate].
        inversion parsed. subst.
        specialize (treeIH _ _ _ childParsed).
        specialize (forestIH _ _ _ siblingsParsed). subst.
        simpl. now rewrite app_assoc.
      * simpl in parsed. inversion parsed. reflexivity.
Qed.

Lemma parse_tree_sound :
  forall fuel events tree tail,
    parse_tree_fuel fuel events = Some (tree, tail) ->
    events = emit_tree tree ++ tail.
Proof.
  intros. eapply (proj1 (parse_sound_mutual fuel)); eauto.
Qed.

Lemma emit_size_bounds_mutual :
  (forall tree, tree_size tree <= length (emit_tree tree)) /\
  (forall forest, forest_size forest <= length (emit_forest forest)).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH. simpl. rewrite app_length. simpl. lia.
  - simpl. lia.
  - intros label child childIH rest restIH. simpl. rewrite app_length. lia.
Qed.

Lemma parse_complete_mutual :
  (forall tree fuel tail,
      tree_size tree < fuel ->
      parse_tree_fuel fuel (emit_tree tree ++ tail) = Some (tree, tail)) /\
  (forall forest fuel tail,
      forest_size forest < fuel ->
      parse_forest_fuel fuel (emit_forest forest ++ EvLeave :: tail) =
        Some (forest, tail)).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH fuel tail enough.
    destruct fuel as [|fuel]; [lia |]. simpl.
    replace ((emit_forest children ++ [EvLeave]) ++ tail)
      with (emit_forest children ++ EvLeave :: tail)
      by (generalize (emit_forest children) as prefix;
          induction prefix; simpl; congruence).
    rewrite childrenIH; [reflexivity |]. simpl in enough. lia.
  - intros fuel tail enough. destruct fuel; [lia |]. reflexivity.
  - intros label child childIH rest restIH fuel tail enough.
    destruct fuel as [|fuel]; [lia |]. simpl in enough. simpl.
    replace ((emit_tree child ++ emit_forest rest) ++ EvLeave :: tail)
      with (emit_tree child ++ (emit_forest rest ++ EvLeave :: tail)).
    2:{ generalize (emit_tree child) as prefix. induction prefix; simpl; congruence. }
    rewrite (childIH fuel (emit_forest rest ++ EvLeave :: tail)); [|lia].
    rewrite (restIH fuel tail); [reflexivity | lia].
Qed.

Theorem valid_event_parser_complete :
  forall tree, parse_event_program (emit_tree tree) = Some tree.
Proof.
  intros tree. unfold parse_event_program.
  pose proof (proj1 parse_complete_mutual tree
    (S (length (emit_tree tree))) []) as complete.
  assert (tree_size tree < S (length (emit_tree tree))).
  { pose proof (proj1 emit_size_bounds_mutual tree). lia. }
  specialize (complete H). rewrite app_nil_r in complete. rewrite complete. reflexivity.
Qed.

Theorem valid_event_parser_sound :
  forall events tree,
    parse_event_program events = Some tree -> events = emit_tree tree.
Proof.
  intros events tree accepted. unfold parse_event_program in accepted.
  destruct (parse_tree_fuel (S (length events)) events)
    as [[candidate tail] |] eqn:parsed; [|discriminate].
  destruct tail; [|discriminate]. inversion accepted. subst.
  pose proof (parse_tree_sound _ _ _ _ parsed). now rewrite app_nil_r in H.
Qed.

Theorem valid_event_parser_unique :
  forall events left right,
    parse_event_program events = Some left ->
    parse_event_program events = Some right -> left = right.
Proof. intros. congruence. Qed.

Theorem valid_program_has_exact_root_close :
  forall tree, exists prefix, emit_tree tree = prefix ++ [EvLeave].
Proof.
  intros [value children]. exists (EvValue value :: emit_forest children).
  reflexivity.
Qed.

Theorem event_after_root_leave_is_rejected :
  forall tree extra,
    parse_event_program (emit_tree tree ++ [extra]) = None.
Proof.
  intros tree extra. unfold parse_event_program.
  pose proof (proj1 parse_complete_mutual tree
    (S (length (emit_tree tree ++ [extra]))) [extra]) as complete.
  assert (tree_size tree < S (length (emit_tree tree ++ [extra]))).
  { pose proof (proj1 emit_size_bounds_mutual tree). rewrite app_length. simpl. lia. }
  specialize (complete H). rewrite complete. reflexivity.
Qed.

Theorem neutral_virtual_walk_is_not_a_typed_empty_act :
  parse_event_program neutral_virtual_events = Some (CNode None FNil) /\
  ~ valid_epm_input (TypedMapInput 0 (CNode None FNil)) /\
  ~ valid_epm_input (TypedSetInput (CNode None FNil)).
Proof. vm_compute. intuition discriminate. Qed.

(** ** Canonical ACTree03 physical-admission bridge *)

Inductive canonical_varint : nat -> list byte -> Prop :=
| VarintLast : forall value,
    value < 128 -> canonical_varint value [value]
| VarintMore : forall low high suffix,
    low < 128 -> 0 < high -> canonical_varint high suffix ->
    canonical_varint (low + 128 * high) ((128 + low) :: suffix).

Lemma canonical_varint_bytes_are_u8 :
  forall value bytes,
    canonical_varint value bytes -> Forall is_u8 bytes.
Proof.
  intros value bytes encoded. induction encoded.
  - constructor; [unfold is_u8; lia | constructor].
  - constructor; [unfold is_u8; lia | exact IHencoded].
Qed.

Lemma canonical_varint_nonempty :
  forall value bytes, canonical_varint value bytes -> bytes <> [].
Proof. intros value bytes encoded; inversion encoded; discriminate. Qed.

Inductive branch_encoding : Type :=
| SparseBranches (labels : list byte)
| DenseMask (mask_bytes : list byte).

Definition bit_is_set (labels : list byte) (candidate : nat) : nat :=
  if existsb (Nat.eqb candidate) labels then 1 else 0.

Definition packed_mask_byte (labels : list byte) (byte_index : nat) : nat :=
  fold_left Nat.add
    (map (fun bit => bit_is_set labels (8 * byte_index + bit) * Nat.pow 2 bit)
         (seq 0 8)) 0.

Definition canonical_dense_mask (labels : list byte) : list byte :=
  map (packed_mask_byte labels) (seq 0 32).

Definition branch_encoding_valid (labels : list byte)
    (encoding : branch_encoding) : Prop :=
  Forall is_u8 labels /\ StronglySorted Nat.lt labels /\
  match encoding with
  | SparseBranches encoded =>
      length labels < 32 /\ encoded = labels
  | DenseMask mask =>
      32 <= length labels /\ mask = canonical_dense_mask labels /\
      length mask = 32
  end.

Inductive physical_act : Type :=
| PhysicalBranch : option nat -> branch_encoding -> physical_forest -> physical_act
| PhysicalLine : list byte -> option nat -> branch_encoding ->
    physical_forest -> physical_act
with physical_forest : Type :=
| PhysicalNil : physical_forest
| PhysicalCons : byte -> physical_act -> physical_forest -> physical_forest.

Scheme physical_act_forest_ind_act := Induction for physical_act Sort Prop
with physical_act_forest_ind_forest := Induction for physical_forest Sort Prop.
Combined Scheme physical_act_forest_mutind
  from physical_act_forest_ind_act, physical_act_forest_ind_forest.

Fixpoint physical_labels (forest : physical_forest) : list byte :=
  match forest with
  | PhysicalNil => []
  | PhysicalCons label _ rest => label :: physical_labels rest
  end.

Fixpoint physical_valid (image : physical_act) : Prop :=
  match image with
  | PhysicalBranch _ encoding children =>
      branch_encoding_valid (physical_labels children) encoding /\
      physical_forest_valid children
  | PhysicalLine suffix _ encoding children =>
      suffix <> [] /\ Forall is_u8 suffix /\
      branch_encoding_valid (physical_labels children) encoding /\
      physical_forest_valid children
  end
with physical_forest_valid (forest : physical_forest) : Prop :=
  match forest with
  | PhysicalNil => True
  | PhysicalCons label child rest =>
      is_u8 label /\ physical_valid child /\ physical_forest_valid rest
  end.

Fixpoint expand_line (suffix : list byte) (terminal : canonical_tree) : canonical_tree :=
  match suffix with
  | [] => terminal
  | label :: rest => CNode None (FCons label (expand_line rest terminal) FNil)
  end.

Fixpoint expand_physical (image : physical_act) : canonical_tree :=
  match image with
  | PhysicalBranch value _ children => CNode value (expand_physical_forest children)
  | PhysicalLine suffix value _ children =>
      expand_line suffix (CNode value (expand_physical_forest children))
  end
with expand_physical_forest (forest : physical_forest) : canonical_forest :=
  match forest with
  | PhysicalNil => FNil
  | PhysicalCons label child rest =>
      FCons label (expand_physical child) (expand_physical_forest rest)
  end.

Record physical_record : Type := {
  record_offset : nat;
  record_length : nat;
  record_relative_offsets : list nat;
  record_offset_varints : list (nat * list byte);
  record_line_suffix : list byte;
  record_branch_encoding : branch_encoding
}.

Definition record_offsets_canonical (record : physical_record) : Prop :=
  Forall (fun pair => canonical_varint (fst pair) (snd pair))
    (record_offset_varints record) /\
  map fst (record_offset_varints record) = record_relative_offsets record.

Definition records_cover_without_gaps (arena_length : nat)
    (records : list physical_record) : Prop :=
  fold_left Nat.add (map record_length records) 0 = arena_length /\
  NoDup (map record_offset records).

Definition producer_order (root_offset : nat) (records : list physical_record) : Prop :=
  Forall (fun record =>
    Forall (fun relative => relative < record_offset record)
      (record_relative_offsets record)) records /\
  match rev records with
  | [] => root_offset = 0
  | root :: _ => root_offset = record_offset root
  end.

Record actree03_image : Type := {
  act_bytes : list byte;
  act_magic : list byte;
  act_root_offset : nat;
  act_root_varint : list byte;
  act_records : list physical_record;
  act_arena_length : nat;
  act_trailer : list byte;
  act_physical_root : physical_act
}.

Definition all_records_reachable (image : actree03_image) : Prop :=
  length (act_records image) = tree_size (expand_physical (act_physical_root image)).

Definition accepted_actree03 (image : actree03_image) : Prop :=
  act_magic image = [65; 67; 84; 114; 101; 101; 48; 51] /\
  Forall is_u8 (act_bytes image) /\
  canonical_varint (act_root_offset image) (act_root_varint image) /\
  Forall record_offsets_canonical (act_records image) /\
  records_cover_without_gaps (act_arena_length image) (act_records image) /\
  producer_order (act_root_offset image) (act_records image) /\
  all_records_reachable image /\
  physical_valid (act_physical_root image) /\
  act_trailer image = repeat 0 9 /\
  act_bytes image = act_magic image ++ act_root_varint image ++
    concat (map (fun record =>
      concat (map snd (record_offset_varints record)) ++
      match record_branch_encoding record with
      | SparseBranches labels => labels
      | DenseMask mask => mask
      end ++ record_line_suffix record) (act_records image)) ++
    act_trailer image.

Definition actree03_logical_tree (image : actree03_image) : canonical_tree :=
  expand_physical (act_physical_root image).

Theorem accepted_actree03_checked_varints :
  forall image,
    accepted_actree03 image ->
    canonical_varint (act_root_offset image) (act_root_varint image) /\
    Forall record_offsets_canonical (act_records image).
Proof. intros image accepted. unfold accepted_actree03 in accepted. tauto. Qed.

Theorem accepted_actree03_offsets_reachability_and_producer_order :
  forall image,
    accepted_actree03 image ->
    records_cover_without_gaps (act_arena_length image) (act_records image) /\
    producer_order (act_root_offset image) (act_records image) /\
    all_records_reachable image.
Proof. intros image accepted. unfold accepted_actree03 in accepted. tauto. Qed.

Theorem accepted_actree03_exact_emitted_events :
  forall image,
    accepted_actree03 image ->
    parse_event_program (emit_tree (actree03_logical_tree image)) =
      Some (actree03_logical_tree image).
Proof. intros. apply valid_event_parser_complete. Qed.

(** ** Extensional PathMap owned-zipper adapter *)

Definition path_eq_dec : forall left right : path, {left = right} + {left <> right} :=
  list_eq_dec Nat.eq_dec.

Fixpoint remove_path {A : Type} (needle : path) (entries : list (path * A))
    : list (path * A) :=
  match entries with
  | [] => []
  | (candidate, value) :: rest =>
      if path_eq_dec needle candidate
      then remove_path needle rest
      else (candidate, value) :: remove_path needle rest
  end.

Fixpoint path_member (needle : path) (paths : list path) : bool :=
  match paths with
  | [] => false
  | candidate :: rest =>
      if path_eq_dec needle candidate then true else path_member needle rest
  end.

Definition ensure_terminal (needle : path) (paths : list path) : list path :=
  if path_member needle paths then paths else needle :: paths.

Record pathmap_model : Type := {
  pm_terminals : list path;
  pm_values : list (path * body)
}.

Record owned_zipper : Type := {
  zipper_focus : path;
  zipper_contents : pathmap_model
}.

Definition zipper_set_val (value : body) (zipper : owned_zipper) : owned_zipper :=
  {| zipper_focus := zipper_focus zipper;
     zipper_contents :=
       {| pm_terminals := ensure_terminal (zipper_focus zipper)
                             (pm_terminals (zipper_contents zipper));
          pm_values := (zipper_focus zipper, value) ::
            remove_path (zipper_focus zipper) (pm_values (zipper_contents zipper)) |} |}.

Definition zipper_create_path (zipper : owned_zipper) : owned_zipper :=
  {| zipper_focus := zipper_focus zipper;
     zipper_contents :=
       {| pm_terminals := ensure_terminal (zipper_focus zipper)
                             (pm_terminals (zipper_contents zipper));
          pm_values := pm_values (zipper_contents zipper) |} |}.

Definition zipper_descend (label : byte) (zipper : owned_zipper) : owned_zipper :=
  {| zipper_focus := zipper_focus zipper ++ [label];
     zipper_contents := zipper_contents zipper |}.

Definition zipper_parent (focus : path) : path := rev (tl (rev focus)).

Definition zipper_ascend (zipper : owned_zipper) : bool * owned_zipper :=
  match zipper_focus zipper with
  | [] => (false, zipper)
  | _ :: _ =>
      (true,
       {| zipper_focus := zipper_parent (zipper_focus zipper);
          zipper_contents := zipper_contents zipper |})
  end.

Definition zipper_into_map (zipper : owned_zipper) : pathmap_model :=
  zipper_contents zipper.

Lemma path_member_spec :
  forall needle paths, path_member needle paths = true <-> In needle paths.
Proof.
  intros needle paths. induction paths as [|candidate rest IH]; simpl.
  - split; [discriminate | contradiction].
  - destruct (path_eq_dec needle candidate) as [same | different].
    + subst. tauto.
    + rewrite IH. split; intros member.
      * right. exact member.
      * destruct member as [same | member]; [congruence | exact member].
Qed.

Lemma root_set_val_has_complete_extensional_effect :
  forall contents value,
    let after := zipper_set_val value
      {| zipper_focus := []; zipper_contents := contents |} in
    In [] (pm_terminals (zipper_contents after)) /\
    In ([], value) (pm_values (zipper_contents after)) /\
    zipper_focus after = [].
Proof.
  intros contents value. simpl. unfold ensure_terminal.
  destruct (path_member [] (pm_terminals contents)) eqn:member; simpl; repeat split; auto.
  apply path_member_spec. exact member.
Qed.

Lemma set_val_preserves_unrelated_values :
  forall zipper value other old,
    other <> zipper_focus zipper ->
    In (other, old) (pm_values (zipper_contents zipper)) ->
    In (other, old) (pm_values (zipper_contents (zipper_set_val value zipper))).
Proof.
  intros [focus [terminals values]] value other old different member. simpl in *.
  right. induction values as [|[candidate found] rest IH]; simpl in *; [contradiction |].
  destruct (path_eq_dec focus candidate).
  - subst. destruct member as [same | member].
    + inversion same. exfalso. apply different. symmetry. exact H0.
    + apply IH. exact member.
  - destruct member as [same | member].
    + left. exact same.
    + right. apply IH. exact member.
Qed.

Lemma nonroot_create_path_preserves_values :
  forall zipper,
    pm_values (zipper_contents (zipper_create_path zipper)) =
    pm_values (zipper_contents zipper).
Proof. intros []; reflexivity. Qed.

Lemma ascent_boolean_and_focus_are_exact :
  forall zipper,
    fst (zipper_ascend zipper) = true <-> zipper_focus zipper <> [].
Proof.
  intros [focus contents]. destruct focus; simpl; split; intros; congruence.
Qed.

Lemma into_map_preserves_complete_contents :
  forall zipper, zipper_into_map zipper = zipper_contents zipper.
Proof. simpl. auto. Qed.

(** ** Streaming bodies and integrated generated-machine publication *)

Inductive decode_error : Type :=
| FramingFailure
| ChildPdaFailure (ordinal : nat)
| DeferredActFailure
| ModeOrOrdinalFailure
| NoncanonicalBodyFailure (ordinal : nat)
| SourceIdentityFailure
| ExtraEventFailure.

Record body_packet : Type := {
  packet_wire_bytes : nat;
  packet_decoded : option body;
  packet_canonical : bool
}.

Record framed_snapshot : Type := {
  framing_fields_valid : bool;
  trailing_bytes_absent : bool;
  declared_body_count : nat;
  wire_body_packets : list body_packet;
  framing_packets_scanned : nat;
  framed_input : epm_input;
  immutable_source_identity : nat
}.

Definition framing_accepts (snapshot : framed_snapshot) : bool :=
  framing_fields_valid snapshot && trailing_bytes_absent snapshot &&
  Nat.eqb (declared_body_count snapshot) (length (wire_body_packets snapshot)) &&
  Nat.eqb (framing_packets_scanned snapshot) (length (wire_body_packets snapshot)).

Record stream_state : Type := {
  stream_remaining : list body_packet;
  stream_bindings : list (path * nat);
  stream_next_ordinal : nat;
  stream_private : pathmap_model;
  stream_first_noncanonical : option nat;
  stream_child_error : option nat;
  stream_association_bad : bool;
  stream_packets_consumed : nat;
  stream_protobuf_bytes : nat;
  stream_path_bytes : nat;
  stream_pathmap_writes : nat
}.

Definition pathmap_set_at (location : path) (value : body)
    (contents : pathmap_model) : pathmap_model :=
  zipper_into_map
    (zipper_set_val value
      {| zipper_focus := location; zipper_contents := contents |}).

Definition note_noncanonical (ordinal : nat) (canonical : bool)
    (previous : option nat) : option nat :=
  match previous with
  | Some first => Some first
  | None => if canonical then None else Some ordinal
  end.

(** One packet is consumed from the wire stream and at most one head binding
    is consumed.  There is no ordinal-indexed body table and no [nth_error]. *)
Definition stream_body_step (state : stream_state) : stream_state :=
  match stream_remaining state with
  | [] => state
  | packet :: remaining =>
      let ordinal := stream_next_ordinal state in
      match packet_decoded packet with
      | None =>
          {| stream_remaining := remaining;
             stream_bindings := stream_bindings state;
             stream_next_ordinal := ordinal;
             stream_private := stream_private state;
             stream_first_noncanonical := stream_first_noncanonical state;
             stream_child_error := Some ordinal;
             stream_association_bad := stream_association_bad state;
             stream_packets_consumed := S (stream_packets_consumed state);
             stream_protobuf_bytes := stream_protobuf_bytes state + packet_wire_bytes packet;
             stream_path_bytes := stream_path_bytes state;
             stream_pathmap_writes := stream_pathmap_writes state |}
      | Some decoded =>
          match stream_bindings state with
          | (location, binding_ordinal) :: bindings =>
              let association_ok := Nat.eqb binding_ordinal ordinal in
              {| stream_remaining := remaining;
                 stream_bindings := if association_ok then bindings
                                    else stream_bindings state;
                 stream_next_ordinal := S ordinal;
                 stream_private :=
                   if association_ok
                   then pathmap_set_at location decoded (stream_private state)
                   else stream_private state;
                 stream_first_noncanonical :=
                   note_noncanonical ordinal (packet_canonical packet)
                     (stream_first_noncanonical state);
                 stream_child_error := stream_child_error state;
                 stream_association_bad :=
                   stream_association_bad state || negb association_ok;
                 stream_packets_consumed := S (stream_packets_consumed state);
                 stream_protobuf_bytes :=
                   stream_protobuf_bytes state + packet_wire_bytes packet;
                 stream_path_bytes := stream_path_bytes state + length location;
                 stream_pathmap_writes :=
                   stream_pathmap_writes state + if association_ok then 1 else 0 |}
          | [] =>
              {| stream_remaining := remaining;
                 stream_bindings := [];
                 stream_next_ordinal := S ordinal;
                 stream_private := stream_private state;
                 stream_first_noncanonical :=
                   note_noncanonical ordinal (packet_canonical packet)
                     (stream_first_noncanonical state);
                 stream_child_error := stream_child_error state;
                 stream_association_bad := true;
                 stream_packets_consumed := S (stream_packets_consumed state);
                 stream_protobuf_bytes :=
                   stream_protobuf_bytes state + packet_wire_bytes packet;
                 stream_path_bytes := stream_path_bytes state;
                 stream_pathmap_writes := stream_pathmap_writes state |}
          end
      end
  end.

Fixpoint stream_all_bodies (fuel : nat) (state : stream_state) : stream_state :=
  match fuel with
  | 0 => state
  | S remaining =>
      match stream_child_error state, stream_remaining state with
      | Some _, _ => state
      | None, [] => state
      | None, _ :: _ => stream_all_bodies remaining (stream_body_step state)
      end
  end.

Definition initial_stream (snapshot : framed_snapshot)
    (bindings : list (path * nat)) (private : pathmap_model) : stream_state :=
  {| stream_remaining := wire_body_packets snapshot;
     stream_bindings := bindings;
     stream_next_ordinal := 0;
     stream_private := private;
     stream_first_noncanonical := None;
     stream_child_error := None;
     stream_association_bad := false;
     stream_packets_consumed := 0;
     stream_protobuf_bytes := 0;
     stream_path_bytes := 0;
     stream_pathmap_writes := 0 |}.

Lemma successful_stream_step_associates_exact_head_ordinal :
  forall state packet remaining location ordinal bindings decoded,
    stream_remaining state = packet :: remaining ->
    packet_decoded packet = Some decoded ->
    stream_bindings state = (location, ordinal) :: bindings ->
    stream_next_ordinal state = ordinal ->
    stream_bindings (stream_body_step state) = bindings /\
    In (location, decoded) (pm_values (stream_private (stream_body_step state))) /\
    stream_next_ordinal (stream_body_step state) = S ordinal.
Proof.
  intros state packet remaining location ordinal bindings decoded
    hasPacket decodedPacket hasBinding nextOrdinal.
  unfold stream_body_step. rewrite hasPacket, decodedPacket, hasBinding, nextOrdinal.
  rewrite Nat.eqb_refl. simpl. repeat split; auto.
Qed.

Lemma stream_step_consumes_exactly_one_packet :
  forall state packet remaining,
    stream_remaining state = packet :: remaining ->
    stream_remaining (stream_body_step state) = remaining /\
    stream_packets_consumed (stream_body_step state) =
      S (stream_packets_consumed state).
Proof.
  intros state packet remaining packetHead.
  unfold stream_body_step. rewrite packetHead.
  destruct (packet_decoded packet) as [decoded |]; [|auto].
  destruct (stream_bindings state) as [|[location ordinal] bindings]; auto.
Qed.

Lemma mismatched_binding_is_latched_without_suppressing_body_consumption :
  forall state packet remaining location bindingOrdinal bindings decoded,
    stream_remaining state = packet :: remaining ->
    packet_decoded packet = Some decoded ->
    stream_bindings state = (location, bindingOrdinal) :: bindings ->
    bindingOrdinal <> stream_next_ordinal state ->
    stream_remaining (stream_body_step state) = remaining /\
    stream_association_bad (stream_body_step state) = true /\
    stream_next_ordinal (stream_body_step state) =
      S (stream_next_ordinal state).
Proof.
  intros state packet remaining location bindingOrdinal bindings decoded
    hasPacket decodedPacket hasBinding mismatch.
  unfold stream_body_step. rewrite hasPacket, decodedPacket, hasBinding.
  destruct (Nat.eqb bindingOrdinal (stream_next_ordinal state)) eqn:equal.
  - apply Nat.eqb_eq in equal. contradiction.
  - destruct (stream_association_bad state); simpl; auto.
Qed.

Lemma unreferenced_body_is_latched_without_suppressing_consumption :
  forall state packet remaining decoded,
    stream_remaining state = packet :: remaining ->
    packet_decoded packet = Some decoded ->
    stream_bindings state = [] ->
    stream_remaining (stream_body_step state) = remaining /\
    stream_association_bad (stream_body_step state) = true /\
    stream_next_ordinal (stream_body_step state) =
      S (stream_next_ordinal state).
Proof.
  intros state packet remaining decoded hasPacket decodedPacket noBindings.
  unfold stream_body_step. rewrite hasPacket, decodedPacket, noBindings. auto.
Qed.

Lemma child_failure_stops_before_later_packets :
  forall fuel state ordinal,
    stream_child_error state = Some ordinal ->
    stream_all_bodies fuel state = state.
Proof.
  intros fuel. destruct fuel; intros; simpl; [reflexivity |].
  rewrite H. reflexivity.
Qed.

Lemma decoded_stream_step_preserves_no_child_error :
  forall state packet remaining decoded,
    stream_remaining state = packet :: remaining ->
    stream_child_error state = None ->
    packet_decoded packet = Some decoded ->
    stream_child_error (stream_body_step state) = None.
Proof.
  intros state packet remaining decoded hasPacket noChild decodedPacket.
  unfold stream_body_step. rewrite hasPacket, decodedPacket.
  destruct (stream_bindings state) as [|[location ordinal] bindings];
    simpl; exact noChild.
Qed.

Lemma decoded_stream_is_completely_consumed :
  forall packets state,
    stream_remaining state = packets ->
    stream_child_error state = None ->
    Forall (fun packet => exists decoded, packet_decoded packet = Some decoded) packets ->
    stream_remaining (stream_all_bodies (length packets) state) = [].
Proof.
  induction packets as [|packet rest IH]; intros state hasPackets noChild decoded.
  - simpl. exact hasPackets.
  - inversion decoded as [|head tail [value decodedHead] decodedTail]; subst.
    simpl. rewrite noChild, hasPackets. simpl.
    apply IH.
    + apply (proj1 (stream_step_consumes_exactly_one_packet
        state packet rest hasPackets)).
    + eapply decoded_stream_step_preserves_no_child_error; eauto.
    + exact decodedTail.
Qed.

Lemma no_child_failure_consumes_complete_framed_stream :
  forall packets snapshot bindings private,
    wire_body_packets snapshot = packets ->
    Forall (fun packet => exists decoded, packet_decoded packet = Some decoded) packets ->
    stream_remaining
      (stream_all_bodies (length packets)
        (initial_stream snapshot bindings private)) = [].
Proof.
  intros packets snapshot bindings private packetsSame decoded.
  apply decoded_stream_is_completely_consumed; simpl; auto.
Qed.

(** Independent recursive body denotation.  Unlike [stream_body_step], this
    semantics has no writer state, zipper, insertion helper, or transition
    frame.  It consumes the wire packets and logical bindings structurally
    from their heads and returns an extensional association collection. *)
Inductive direct_body_result : Type :=
| DirectChildFailure : nat -> nat -> direct_body_result
| DirectBodiesDone :
    list (path * nat) -> nat -> list (path * body) -> option nat -> bool -> nat ->
    direct_body_result.

Fixpoint direct_body_denotation (next : nat)
    (bindings : list (path * nat)) (packets : list body_packet)
    : direct_body_result :=
  match packets with
  | [] => DirectBodiesDone bindings next [] None false 0
  | packet :: rest =>
      match packet_decoded packet with
      | None => DirectChildFailure next 1
      | Some decoded =>
          let association :=
            match bindings with
            | (location, ordinal) :: tail =>
                if Nat.eqb ordinal next
                then (tail, Some (location, decoded), false)
                else (bindings, None, true)
            | [] => ([], None, true)
            end in
          let '(remainingBindings, associationValue, associationBad) := association in
          match direct_body_denotation (S next) remainingBindings rest with
          | DirectChildFailure ordinal consumed =>
              DirectChildFailure ordinal (S consumed)
          | DirectBodiesDone finalBindings finalNext values firstBad tailBad consumed =>
              let canonicalBad := if packet_canonical packet then firstBad else Some next in
              DirectBodiesDone finalBindings finalNext
                (match associationValue with
                 | Some entry => entry :: values
                 | None => values
                 end)
                canonicalBad (associationBad || tailBad) (S consumed)
          end
      end
  end.

Lemma direct_body_child_failure_is_first_in_wire_order :
  forall prefix next bindings bad rest ordinal consumed,
    Forall (fun packet => exists decoded, packet_decoded packet = Some decoded) prefix ->
    ordinal = next + length prefix ->
    packet_decoded bad = None ->
    direct_body_denotation next bindings (prefix ++ bad :: rest) =
      DirectChildFailure ordinal consumed ->
    consumed = S (length prefix).
Proof.
  induction prefix as [|packet prefix IH]; intros next bindings bad rest ordinal consumed
    decoded before badPacket result.
  - simpl in before. subst ordinal. simpl in result. rewrite badPacket in result.
    inversion result. reflexivity.
  - inversion decoded as [|head tail [value decodedPacket] decodedTail]; subst.
    simpl in result. rewrite decodedPacket in result.
    destruct bindings as [|[location bindingOrdinal] bindingTail].
    + simpl in result.
      destruct (direct_body_denotation (S next) [] (prefix ++ bad :: rest))
        eqn:tailResult;
        try discriminate.
      inversion result. subst. f_equal.
      replace (next + S (length prefix)) with (S next + length prefix)
        in tailResult by lia.
      eapply IH; [exact decodedTail | reflexivity | exact badPacket | exact tailResult].
    + destruct (Nat.eqb bindingOrdinal next) eqn:association.
      * simpl in result.
        destruct (direct_body_denotation (S next) bindingTail
          (prefix ++ bad :: rest)) eqn:tailResult; try discriminate.
        inversion result. subst. f_equal.
        replace (next + S (length prefix)) with (S next + length prefix)
          in tailResult by lia.
        eapply IH; [exact decodedTail | reflexivity | exact badPacket | exact tailResult].
      * simpl in result.
        destruct (direct_body_denotation (S next)
          ((location, bindingOrdinal) :: bindingTail)
          (prefix ++ bad :: rest)) eqn:tailResult; try discriminate.
        inversion result. subst. f_equal.
        replace (next + S (length prefix)) with (S next + length prefix)
          in tailResult by lia.
        eapply IH; [exact decodedTail | reflexivity | exact badPacket | exact tailResult].
Qed.

Lemma direct_body_complete_consumption :
  forall packets next bindings finalBindings finalNext values firstBad associationBad consumed,
    Forall (fun packet => exists decoded, packet_decoded packet = Some decoded) packets ->
    direct_body_denotation next bindings packets =
      DirectBodiesDone finalBindings finalNext values firstBad associationBad consumed ->
    consumed = length packets /\ finalNext = next + length packets.
Proof.
  induction packets as [|packet rest IH]; intros next bindings finalBindings finalNext
    values firstBad associationBad consumed decoded result.
  - simpl in result. inversion result. subst. simpl. lia.
  - inversion decoded as [|head tail [value decodedPacket] decodedTail]; subst.
    simpl in result. rewrite decodedPacket in result.
    destruct bindings as [|[location ordinal] bindingTail].
    + simpl in result.
      destruct (direct_body_denotation (S next) [] rest) eqn:tailResult;
        try discriminate.
      inversion result. subst.
      specialize (IH (S next) [] _ _ _ _ _ _ decodedTail tailResult).
      destruct IH. simpl. lia.
    + simpl in result.
      destruct (Nat.eqb ordinal next) eqn:equal.
      * simpl in result.
        destruct (direct_body_denotation (S next) bindingTail rest)
          eqn:tailResult; try discriminate.
        inversion result. subst.
        specialize (IH (S next) bindingTail _ _ _ _ _ _ decodedTail tailResult).
        destruct IH. simpl. lia.
      * simpl in result.
        destruct (direct_body_denotation (S next)
          ((location, ordinal) :: bindingTail) rest) eqn:tailResult;
          try discriminate.
        inversion result. subst.
        specialize (IH (S next) ((location, ordinal) :: bindingTail)
          _ _ _ _ _ _ decodedTail tailResult).
        destruct IH. simpl. lia.
Qed.

Definition direct_error_selection (framingOk : bool)
    (bodyResult : direct_body_result) (deferredActBad modeBad identityOk : bool)
    : option decode_error :=
  if negb framingOk then Some FramingFailure else
  match bodyResult with
  | DirectChildFailure ordinal _ => Some (ChildPdaFailure ordinal)
  | DirectBodiesDone remaining _ _ firstBad associationBad _ =>
      if deferredActBad then Some DeferredActFailure else
      if modeBad || associationBad || negb (Nat.eqb (length remaining) 0)
      then Some ModeOrOrdinalFailure else
      match firstBad with
      | Some ordinal => Some (NoncanonicalBodyFailure ordinal)
      | None => if negb identityOk then Some SourceIdentityFailure else None
      end
  end.

Theorem direct_oracle_child_outranks_deferred_structure :
  forall framing ordinal consumed modeBad identityOk,
    framing = true ->
    direct_error_selection framing (DirectChildFailure ordinal consumed)
      true modeBad identityOk = Some (ChildPdaFailure ordinal).
Proof. intros. subst. reflexivity. Qed.

Theorem direct_oracle_structure_outranks_noncanonical :
  forall remaining next values ordinal associationBad consumed identityOk,
    direct_error_selection true
      (DirectBodiesDone remaining next values (Some ordinal) associationBad consumed)
      true false identityOk = Some DeferredActFailure.
Proof. simpl. auto. Qed.

Record parent_value : Type := {
  parent_contents : pathmap_model;
  parent_public_root_identity : nat
}.

Record publication_state : Type := {
  publication_parent_before : parent_value;
  publication_parent_after : parent_value;
  publication_source_identity : nat;
  publication_private_root_identity : nat;
  publication_bound_source_identity : option nat;
  publication_source_owned : bool;
  publication_finish_count : nat;
  publication_published : bool
}.

Definition publication_identities_separate (state : publication_state) : Prop :=
  publication_source_identity state <>
  publication_private_root_identity state.

Definition select_integrated_error
    (framing_ok : bool) (stream : stream_state)
    (deferred_act_bad mode_bad : bool) (identity_ok : bool)
    : option decode_error :=
  if negb framing_ok then Some FramingFailure else
  match stream_child_error stream with
  | Some ordinal => Some (ChildPdaFailure ordinal)
  | None =>
      if deferred_act_bad then Some DeferredActFailure else
      if mode_bad || stream_association_bad stream ||
           negb (Nat.eqb (length (stream_bindings stream)) 0)
      then Some ModeOrOrdinalFailure else
      match stream_first_noncanonical stream with
      | Some ordinal => Some (NoncanonicalBodyFailure ordinal)
      | None => if negb identity_ok then Some SourceIdentityFailure else None
      end
  end.

Definition finish_publication (before : parent_value) (source private_root : nat)
    (bound_source : option nat) (private : pathmap_model)
    (failure : option decode_error) : publication_state :=
  match failure, bound_source with
  | None, Some bound =>
      if Nat.eqb bound source then
        {| publication_parent_before := before;
           publication_parent_after :=
             {| parent_contents := private;
                parent_public_root_identity := private_root |};
           publication_source_identity := source;
           publication_private_root_identity := private_root;
           publication_bound_source_identity := Some bound;
           publication_source_owned := false;
           publication_finish_count := 1;
           publication_published := true |}
      else
        {| publication_parent_before := before;
           publication_parent_after := before;
           publication_source_identity := source;
           publication_private_root_identity := private_root;
           publication_bound_source_identity := Some bound;
           publication_source_owned := false;
           publication_finish_count := 1;
           publication_published := false |}
  | _, _ =>
      {| publication_parent_before := before;
         publication_parent_after := before;
         publication_source_identity := source;
         publication_private_root_identity := private_root;
         publication_bound_source_identity := bound_source;
         publication_source_owned := false;
         publication_finish_count := 1;
         publication_published := false |}
  end.

Theorem every_failure_leaves_parent_unchanged :
  forall before source privateRoot bound private failure,
    failure <> None ->
    publication_parent_after
      (finish_publication before source privateRoot bound private failure) = before.
Proof.
  intros before source privateRoot bound private failure failed.
  destruct failure; [reflexivity | contradiction].
Qed.

Theorem publication_is_exactly_once_and_consumes_source :
  forall before source privateRoot bound private failure,
    publication_finish_count
      (finish_publication before source privateRoot bound private failure) = 1 /\
    publication_source_owned
      (finish_publication before source privateRoot bound private failure) = false.
Proof.
  intros. destruct failure as [failure |]; simpl; [auto |].
  destruct bound as [bound |]; simpl; [|auto].
  destruct (Nat.eqb bound source); auto.
Qed.

Theorem publication_requires_bound_immutable_source_identity :
  forall before source privateRoot bound private failure,
    publication_published
      (finish_publication before source privateRoot bound private failure) = true ->
    failure = None /\ bound = Some source.
Proof.
  intros before source privateRoot bound private failure published.
  destruct failure; [discriminate |]. destruct bound as [bound |]; [|discriminate].
  simpl in published. destruct (Nat.eqb bound source) eqn:equal; [|discriminate].
  apply Nat.eqb_eq in equal. subst. auto.
Qed.

(** Names and shapes mirror the generated control structure, while the
    extensional specification above remains independently defined. *)
Record EpmFrame : Type := {
  epm_parent : parent_value;
  epm_limit : nat;
  epm_pending : stream_state;
  epm_source_identity : nat;
  epm_private_root_identity : nat;
  epm_bound_source_identity : option nat;
  epm_deferred_act_bad : bool;
  epm_mode_bad : bool
}.

Inductive Frame : Type :=
| FrameEpm : EpmFrame -> Frame
| FrameAttachEPathMapSnapshotValue : EpmFrame -> body_packet -> Frame
| FrameChildPar : body_packet -> Frame
| FrameNodeEPathMap : parent_value -> nat -> Frame.

Record Machine : Type := {
  machine_frames : list Frame;
  machine_finished : option publication_state
}.

Inductive step_epm_snapshot : Machine -> Machine -> Prop :=
| StepEpmChild : forall machine frame packet packetTail remaining,
    machine_frames machine = FrameEpm frame :: remaining ->
    stream_child_error (epm_pending frame) = None ->
    stream_remaining (epm_pending frame) = packet :: packetTail ->
    step_epm_snapshot machine
      {| machine_frames := FrameChildPar packet ::
          FrameAttachEPathMapSnapshotValue frame packet :: remaining;
         machine_finished := None |}
| StepEpmAttach : forall machine frame packet packetTail remaining,
    machine_frames machine = FrameChildPar packet ::
      FrameAttachEPathMapSnapshotValue frame packet :: remaining ->
    stream_remaining (epm_pending frame) = packet :: packetTail ->
    step_epm_snapshot machine
      {| machine_frames := FrameEpm
          {| epm_parent := epm_parent frame;
             epm_limit := epm_limit frame;
             epm_pending := stream_body_step (epm_pending frame);
             epm_source_identity := epm_source_identity frame;
             epm_private_root_identity := epm_private_root_identity frame;
             epm_bound_source_identity := epm_bound_source_identity frame;
             epm_deferred_act_bad := epm_deferred_act_bad frame;
             epm_mode_bad := epm_mode_bad frame |} :: remaining;
         machine_finished := None |}
| StepEpmFinish : forall machine frame remaining error publication,
    machine_frames machine = FrameEpm frame :: remaining ->
    (stream_remaining (epm_pending frame) = [] \/
     stream_child_error (epm_pending frame) <> None) ->
    error = select_integrated_error true (epm_pending frame)
      (epm_deferred_act_bad frame) (epm_mode_bad frame)
      (match epm_bound_source_identity frame with
       | Some bound => Nat.eqb bound (epm_source_identity frame)
       | None => false
       end) ->
    publication = finish_publication (epm_parent frame)
      (epm_source_identity frame) (epm_private_root_identity frame)
      (epm_bound_source_identity frame)
      (stream_private (epm_pending frame)) error ->
    step_epm_snapshot machine
      {| machine_frames := FrameNodeEPathMap
          (publication_parent_after publication) (epm_limit frame) :: remaining;
         machine_finished := Some publication |}.

Theorem generated_finish_refines_extensional_publication :
  forall before after frame remaining publication,
    machine_frames before = FrameEpm frame :: remaining ->
    step_epm_snapshot before after ->
    machine_finished after = Some publication ->
    publication = finish_publication (epm_parent frame)
      (epm_source_identity frame) (epm_private_root_identity frame)
      (epm_bound_source_identity frame)
      (stream_private (epm_pending frame))
      (select_integrated_error true (epm_pending frame)
        (epm_deferred_act_bad frame) (epm_mode_bad frame)
        (match epm_bound_source_identity frame with
         | Some bound => Nat.eqb bound (epm_source_identity frame)
         | None => false
         end)).
Proof.
  intros before after frame remaining publication starts stepped finished.
  inversion stepped; subst; simpl in finished; try discriminate.
  rewrite starts in H. inversion H. subst.
  inversion finished. subst. reflexivity.
Qed.

(** Exact production precedence: framing is complete first; an immediate
    child failure outranks deferred ACT; ACT/mode/ordinal outranks recorded
    canonical-body drift. *)
Theorem child_error_outranks_deferred_act :
  forall stream ordinal modeBad identityOk,
    stream_child_error stream = Some ordinal ->
    select_integrated_error true stream true modeBad identityOk =
      Some (ChildPdaFailure ordinal).
Proof. intros. unfold select_integrated_error. rewrite H. reflexivity. Qed.

Theorem deferred_act_outranks_noncanonical :
  forall stream ordinal modeBad identityOk,
    stream_child_error stream = None ->
    stream_first_noncanonical stream = Some ordinal ->
    select_integrated_error true stream true modeBad identityOk =
      Some DeferredActFailure.
Proof. intros. unfold select_integrated_error. rewrite H. reflexivity. Qed.

Theorem ordinal_association_failure_outranks_noncanonical :
  forall stream ordinal identityOk,
    stream_child_error stream = None ->
    stream_association_bad stream = true ->
    stream_first_noncanonical stream = Some ordinal ->
    select_integrated_error true stream false false identityOk =
      Some ModeOrOrdinalFailure.
Proof.
  intros. unfold select_integrated_error. rewrite H, H0. reflexivity.
Qed.

Theorem framing_failure_prevents_child_or_act_selection :
  forall stream actBad modeBad identityOk,
    select_integrated_error false stream actBad modeBad identityOk =
      Some FramingFailure.
Proof. reflexivity. Qed.

Theorem accepted_framing_scanned_every_body_extent :
  forall snapshot,
    framing_accepts snapshot = true ->
    framing_packets_scanned snapshot = length (wire_body_packets snapshot) /\
    declared_body_count snapshot = length (wire_body_packets snapshot) /\
    trailing_bytes_absent snapshot = true.
Proof.
  intros snapshot accepted. unfold framing_accepts in accepted.
  repeat rewrite andb_true_iff in accepted.
  destruct accepted as [[[fields trailing] declared] scanned].
  repeat split; try (apply Nat.eqb_eq; assumption); assumption.
Qed.

(** Logical occurrence ordinals are positions in canonical producer order,
    never identities or addresses of stored values. *)
Definition occurrence_ordinals (storage_identities : list nat) : list nat :=
  seq 0 (length storage_identities).

Fixpoint first_index_from (needle : nat) (next : nat) (identities : list nat)
    : nat :=
  match identities with
  | [] => next
  | identity :: rest =>
      if Nat.eqb needle identity then next
      else first_index_from needle (S next) rest
  end.

Definition pointer_collapsing_ordinals (identities : list nat) : list nat :=
  map (fun identity => first_index_from identity 0 identities) identities.

Theorem occurrence_ordinals_ignore_storage_allocation :
  forall shared unshared,
    length shared = length unshared ->
    occurrence_ordinals shared = occurrence_ordinals unshared.
Proof. intros. unfold occurrence_ordinals. now rewrite H. Qed.

Example two_logical_occurrences_may_share_one_storage_identity :
  occurrence_ordinals [41; 41] = [0; 1] /\
  pointer_collapsing_ordinals [41; 41] = [0; 0].
Proof. vm_compute. auto. Qed.

(** ** Integration-owned ordinal Catamorphism adapter

    PathMap remains the sole ACT byte writer.  The adapter observes logical
    endpoint Enter events, stores only the assigned [u64] ordinal in the
    explicit heap frame, and supplies a short-lived reference to that same
    ordinal on bottom-up Exit.  It never observes a [T] address. *)
Record endpoint_observation : Type := {
  observed_path : path;
  observed_has_value : bool;
  observed_storage_identity : nat
}.

Record ordinal_frame : Type := {
  ordinal_frame_path : path;
  ordinal_frame_value : option nat
}.

Fixpoint assign_endpoint_ordinals (next : nat)
    (observations : list endpoint_observation)
    : list ordinal_frame * nat :=
  match observations with
  | [] => ([], next)
  | observation :: rest =>
      let assigned :=
        if observed_has_value observation then Some next else None in
      let next' :=
        if observed_has_value observation then S next else next in
      let '(frames, finalNext) := assign_endpoint_ordinals next' rest in
      ({| ordinal_frame_path := observed_path observation;
          ordinal_frame_value := assigned |} :: frames, finalNext)
  end.

Definition valued_endpoint_paths (observations : list endpoint_observation)
    : list path :=
  map observed_path (filter observed_has_value observations).

Definition cata_endpoint_paths (frames : list ordinal_frame) : list path :=
  map ordinal_frame_path
    (filter (fun frame => match ordinal_frame_value frame with
                          | Some _ => true | None => false end) frames).

Definition cata_endpoint_ordinals (frames : list ordinal_frame) : list nat :=
  fold_right (fun frame tail =>
    match ordinal_frame_value frame with Some ordinal => ordinal :: tail
    | None => tail end) [] frames.

Lemma ordinal_catamorphism_aligns_paths_and_ordinals :
  forall observations next frames finalNext,
    assign_endpoint_ordinals next observations = (frames, finalNext) ->
    cata_endpoint_paths frames = valued_endpoint_paths observations /\
    cata_endpoint_ordinals frames = seq next (length (valued_endpoint_paths observations)) /\
    finalNext = next + length (valued_endpoint_paths observations).
Proof.
  induction observations as [|observation rest IH]; intros next frames finalNext assigned.
  - simpl in assigned. inversion assigned. subst. simpl. repeat split; lia.
  - destruct observation as [observationPath hasValue storageIdentity].
    simpl in assigned. destruct hasValue.
    + destruct (assign_endpoint_ordinals (S next) rest)
        as [tailFrames tailNext] eqn:tailAssigned.
      pose proof (IH (S next) tailFrames tailNext tailAssigned) as tailIH.
      inversion assigned. subst. clear assigned.
      destruct tailIH as [paths [ordinals final]].
      split.
      * unfold cata_endpoint_paths, valued_endpoint_paths in paths |- *.
        simpl in paths |- *. now rewrite paths.
      * split.
        { unfold cata_endpoint_ordinals, valued_endpoint_paths in ordinals |- *.
          simpl in ordinals |- *. now rewrite ordinals. }
        { unfold valued_endpoint_paths in final |- *. simpl in final |- *.
          rewrite final. lia. }
    + destruct (assign_endpoint_ordinals next rest)
        as [tailFrames tailNext] eqn:tailAssigned.
      pose proof (IH next tailFrames tailNext tailAssigned) as tailIH.
      inversion assigned. subst. clear assigned.
      destruct tailIH as [paths [ordinals final]].
      split.
      * unfold cata_endpoint_paths, valued_endpoint_paths in paths |- *.
        simpl in paths |- *. exact paths.
      * split.
        { unfold cata_endpoint_ordinals, valued_endpoint_paths in ordinals |- *.
          simpl in ordinals |- *. exact ordinals. }
        { unfold valued_endpoint_paths in final |- *. simpl in final |- *. exact final. }
Qed.

Definition ordinal_mapper (frame : ordinal_frame) : option nat :=
  ordinal_frame_value frame.

Theorem compressed_leaf_mapper_reentry_is_idempotent :
  forall frame,
    ordinal_mapper frame = ordinal_mapper frame /\
    ordinal_mapper frame = ordinal_mapper frame.
Proof. auto. Qed.

Definition erase_storage_identities (observations : list endpoint_observation)
    : list endpoint_observation :=
  map (fun observation =>
    {| observed_path := observed_path observation;
       observed_has_value := observed_has_value observation;
       observed_storage_identity := 0 |}) observations.

Lemma ordinal_assignment_is_storage_identity_independent :
  forall observations next,
    assign_endpoint_ordinals next (erase_storage_identities observations) =
    assign_endpoint_ordinals next observations.
Proof.
  induction observations as [|observation rest IH]; intros next.
  - reflexivity.
  - destruct observation as [observationPath hasValue storageIdentity].
    simpl. destruct hasValue; simpl; rewrite IH; reflexivity.
Qed.

Example shared_value_address_has_distinct_logical_ordinals :
  let observations :=
    [{| observed_path := [1]; observed_has_value := true;
        observed_storage_identity := 41 |};
     {| observed_path := [2]; observed_has_value := true;
        observed_storage_identity := 41 |}] in
  cata_endpoint_ordinals (fst (assign_endpoint_ordinals 0 observations)) = [0; 1].
Proof. reflexivity. Qed.

(** Cached traversal is intentionally occurrence-preserving: contextual
    ordinal state makes a shared-subtrie cached result invalid for reuse. *)
Definition occurrence_preserving_cached_cata := assign_endpoint_ordinals.

Theorem cached_cata_delegates_to_occurrence_traversal :
  forall next observations,
    occurrence_preserving_cached_cata next observations =
    assign_endpoint_ordinals next observations.
Proof. reflexivity. Qed.

Definition max_u64_exclusive : N := N.pow 2 64.

Definition checked_assign_below (exclusive next : N) : option (N * N) :=
  if N.ltb next exclusive then Some (next, N.succ next) else None.

Definition checked_assign_ordinal (next : N) : option (N * N) :=
  checked_assign_below max_u64_exclusive next.

Lemma checked_assign_ordinal_range_and_advance :
  forall next ordinal after : N,
    checked_assign_below max_u64_exclusive next = Some (ordinal, after) ->
    ordinal = next /\ after = N.succ next /\ N.lt ordinal max_u64_exclusive.
Proof.
  intros next ordinal after assigned. unfold checked_assign_below in assigned.
  destruct (N.ltb next max_u64_exclusive) eqn:within; [|discriminate].
  inversion assigned. subst. apply N.ltb_lt in within. auto.
Qed.

Definition mapper_invocation_count (frame : ordinal_frame) : nat :=
  match ordinal_frame_value frame with
  | None => 0
  | Some _ => 2
  end.

Theorem duplicate_mapper_calls_do_not_duplicate_logical_occurrences :
  forall frames,
    length (cata_endpoint_ordinals frames) =
    length (filter (fun frame => match ordinal_frame_value frame with
                                | Some _ => true | None => false end) frames).
Proof.
  induction frames as [|frame rest IH]; simpl.
  - reflexivity.
  - destruct (ordinal_frame_value frame); simpl; f_equal; exact IH.
Qed.

(** A separate canonical zipper pass observes the same logical endpoint
    predicate and path order.  It may stream protobuf bodies without keeping
    references or a body table. *)
Definition canonical_body_pass_paths := valued_endpoint_paths.

Theorem act_ordinal_and_body_pass_are_byte_aligned :
  forall observations frames finalNext,
    assign_endpoint_ordinals 0 observations = (frames, finalNext) ->
    cata_endpoint_paths frames = canonical_body_pass_paths observations /\
    cata_endpoint_ordinals frames =
      seq 0 (length (canonical_body_pass_paths observations)).
Proof.
  intros. unfold canonical_body_pass_paths.
  pose proof (ordinal_catamorphism_aligns_paths_and_ordinals
    observations 0 frames finalNext H) as [paths [ordinals final]]. auto.
Qed.

Definition ordinal_cata_frame_memory_units (active_depth : nat) : nat :=
  active_depth.

Theorem ordinal_cata_requires_depth_frames_not_node_rank_storage :
  forall activeDepth,
    ordinal_cata_frame_memory_units activeDepth = activeDepth.
Proof. reflexivity. Qed.

(** ** Synthetic ordinal zipper refinement

    These transitions model only integration-owned state.  Every topology
    movement is delegated unchanged to the wrapped PathMap read zipper.  A
    witness owns [option nat], so no source [T] or address survives a call. *)
Record ordinal_zipper_frame : Type := {
  ozf_path : path;
  ozf_path_len : nat;
  ozf_witness : option nat
}.

Record ordinal_zipper_state : Type := {
  oz_focus : path;
  oz_next : nat;
  oz_stack : list ordinal_zipper_frame;
  oz_emitted : list (path * nat);
  oz_failed : bool
}.

Definition ordinal_arrive (location : path) (hasValue : bool)
    (state : ordinal_zipper_state) : ordinal_zipper_state :=
  if oz_failed state then state else
  let witness := if hasValue then Some (oz_next state) else None in
  {| oz_focus := location;
     oz_next := if hasValue then S (oz_next state) else oz_next state;
     oz_stack := {| ozf_path := location;
                    ozf_path_len := length location;
                    ozf_witness := witness |} :: oz_stack state;
     oz_emitted := if hasValue
                   then oz_emitted state ++ [(location, oz_next state)]
                   else oz_emitted state;
     oz_failed := false |}.

Definition ordinal_reset (rootHasValue : bool)
    (state : ordinal_zipper_state) : ordinal_zipper_state :=
  ordinal_arrive [] rootHasValue
    {| oz_focus := [];
       oz_next := oz_next state;
       oz_stack := [];
       oz_emitted := oz_emitted state;
       oz_failed := oz_failed state |}.

Definition strict_prefix (prefix whole : path) : Prop :=
  exists suffix, suffix <> [] /\ whole = prefix ++ suffix.

Definition ordinal_descend_first_byte := ordinal_arrive.
Definition ordinal_descend_until := ordinal_arrive.
Definition ordinal_descend_indexed_byte := ordinal_arrive.

Fixpoint pop_to_prefix (target : path) (frames : list ordinal_zipper_frame)
    : list ordinal_zipper_frame :=
  match frames with
  | [] => []
  | frame :: rest =>
      if path_eq_dec (ozf_path frame) target then frame :: rest
      else pop_to_prefix target rest
  end.

Definition ordinal_ascend_to (target : path) (state : ordinal_zipper_state)
    : ordinal_zipper_state :=
  {| oz_focus := target;
     oz_next := oz_next state;
     oz_stack := pop_to_prefix target (oz_stack state);
     oz_emitted := oz_emitted state;
     oz_failed := oz_failed state |}.

Definition ordinal_ascend_until := ordinal_ascend_to.
Definition ordinal_ascend_byte := ordinal_ascend_to.

Definition ordinal_current_witness (state : ordinal_zipper_state) : option nat :=
  match oz_stack state with
  | frame :: _ => ozf_witness frame
  | [] => None
  end.

Lemma valued_arrival_assigns_once_and_advances :
  forall state location,
    oz_failed state = false ->
    ordinal_current_witness (ordinal_arrive location true state) =
      Some (oz_next state) /\
    oz_next (ordinal_arrive location true state) = S (oz_next state) /\
    oz_emitted (ordinal_arrive location true state) =
      oz_emitted state ++ [(location, oz_next state)].
Proof. intros. unfold ordinal_arrive. rewrite H. simpl. auto. Qed.

Lemma value_free_arrival_does_not_consume_an_ordinal :
  forall state location,
    oz_failed state = false ->
    ordinal_current_witness (ordinal_arrive location false state) = None /\
    oz_next (ordinal_arrive location false state) = oz_next state /\
    oz_emitted (ordinal_arrive location false state) = oz_emitted state.
Proof. intros. unfold ordinal_arrive. rewrite H. simpl. auto. Qed.

Lemma reset_handles_root_without_nonempty_path_assumption :
  forall state,
    oz_failed state = false ->
    oz_focus (ordinal_reset true state) = [] /\
    ordinal_current_witness (ordinal_reset true state) = Some (oz_next state).
Proof. intros. unfold ordinal_reset, ordinal_arrive. rewrite H. split; reflexivity. Qed.

Lemma ascent_preserves_counter_and_emission :
  forall target state,
    oz_next (ordinal_ascend_to target state) = oz_next state /\
    oz_emitted (ordinal_ascend_to target state) = oz_emitted state.
Proof. auto. Qed.

Lemma witness_mapper_reentry_is_stable_across_reads :
  forall state ordinal,
    ordinal_current_witness state = Some ordinal ->
    ordinal_current_witness state = Some ordinal /\
    ordinal_current_witness state = Some ordinal.
Proof. auto. Qed.

Inductive delegated_move : Type :=
| DReset : endpoint_observation -> delegated_move
| DDescendFirstByte : endpoint_observation -> delegated_move
| DDescendUntil : endpoint_observation -> delegated_move
| DDescendIndexedByte : endpoint_observation -> delegated_move
| DAscendUntil : path -> delegated_move
| DAscendByte : path -> delegated_move.

Definition delegated_destination (movement : delegated_move) : option path :=
  match movement with
  | DReset _ => Some []
  | DDescendFirstByte observation
  | DDescendUntil observation
  | DDescendIndexedByte observation => Some (observed_path observation)
  | DAscendUntil target | DAscendByte target => Some target
  end.

Definition ordinal_move (movement : delegated_move)
    (state : ordinal_zipper_state) : ordinal_zipper_state :=
  match movement with
  | DReset observation => ordinal_reset (observed_has_value observation) state
  | DDescendFirstByte observation =>
      ordinal_descend_first_byte (observed_path observation)
        (observed_has_value observation) state
  | DDescendUntil observation =>
      ordinal_descend_until (observed_path observation)
        (observed_has_value observation) state
  | DDescendIndexedByte observation =>
      ordinal_descend_indexed_byte (observed_path observation)
        (observed_has_value observation) state
  | DAscendUntil target => ordinal_ascend_until target state
  | DAscendByte target => ordinal_ascend_byte target state
  end.

Lemma every_delegated_move_preserves_the_underlying_destination :
  forall movement state destination,
    delegated_destination movement = Some destination ->
    oz_focus (ordinal_move movement state) = destination \/ oz_failed state = true.
Proof.
  intros movement state destination target. destruct movement; simpl in target;
    inversion target; subst;
    unfold ordinal_move, ordinal_reset, ordinal_descend_first_byte,
      ordinal_descend_until, ordinal_descend_indexed_byte, ordinal_arrive,
      ordinal_ascend_until, ordinal_ascend_byte, ordinal_ascend_to;
    destruct (oz_failed state); simpl; auto.
Qed.

Definition ordinal_adapter_runtime_units (stackDepth : nat) : nat := stackDepth.

Theorem synthetic_ordinal_zipper_has_depth_only_private_state :
  forall stackDepth,
    ordinal_adapter_runtime_units stackDepth = stackDepth.
Proof. reflexivity. Qed.

(** The preceding current-only witness candidate is not a refinement of the
    real jumping cata: PathMap creates one witness and reuses it while it
    ascends through a unary chain.  A correct safe wrapper must snapshot all
    active frames into immutable witness-owned storage. *)
Definition ordinal_witness_snapshot (state : ordinal_zipper_state)
    : list ordinal_zipper_frame := oz_stack state.

Definition popped_frame_count (witness : list ordinal_zipper_frame)
    (live : list ordinal_zipper_frame) : nat :=
  length witness - length live.

Definition witness_value_for_live_focus
    (witness live : list ordinal_zipper_frame) : option nat :=
  match nth_error witness (popped_frame_count witness live) with
  | Some frame => ozf_witness frame
  | None => None
  end.

Example current_only_witness_is_stale_after_first_ascent :
  let leaf := {| ozf_path := [1; 2]; ozf_path_len := 2;
                 ozf_witness := Some 2 |} in
  let parent := {| ozf_path := [1]; ozf_path_len := 1;
                   ozf_witness := Some 1 |} in
  ordinal_current_witness
    {| oz_focus := [1]; oz_next := 3; oz_stack := [parent];
       oz_emitted := []; oz_failed := false |} = Some 1 /\
  ozf_witness leaf = Some 2.
Proof. split; reflexivity. Qed.

Example full_stack_witness_tracks_multi_value_unary_ascent :
  let leaf := {| ozf_path := [1; 2]; ozf_path_len := 2;
                 ozf_witness := Some 2 |} in
  let parent := {| ozf_path := [1]; ozf_path_len := 1;
                   ozf_witness := Some 1 |} in
  let root := {| ozf_path := []; ozf_path_len := 0;
                 ozf_witness := Some 0 |} in
  let witness := [leaf; parent; root] in
  witness_value_for_live_focus witness [leaf; parent; root] = Some 2 /\
  witness_value_for_live_focus witness [parent; root] = Some 1 /\
  witness_value_for_live_focus witness [root] = Some 0.
Proof. repeat split; reflexivity. Qed.

Example full_stack_witness_handles_value_free_structural_stop :
  let leaf := {| ozf_path := [1; 2]; ozf_path_len := 2;
                 ozf_witness := Some 1 |} in
  let branch := {| ozf_path := [1]; ozf_path_len := 1;
                   ozf_witness := None |} in
  let root := {| ozf_path := []; ozf_path_len := 0;
                 ozf_witness := Some 0 |} in
  let witness := [leaf; branch; root] in
  witness_value_for_live_focus witness [leaf; branch; root] = Some 1 /\
  witness_value_for_live_focus witness [branch; root] = None /\
  witness_value_for_live_focus witness [root] = Some 0.
Proof. repeat split; reflexivity. Qed.

Definition witness_snapshot_copy_work (activeFrames : nat) : nat := activeFrames.

Fixpoint sum_nats (values : list nat) : nat :=
  match values with [] => 0 | value :: rest => value + sum_nats rest end.

Definition witness_snapshot_total_work (leafDepths : list nat) : nat :=
  sum_nats (map witness_snapshot_copy_work leafDepths).

Example dense_prefix_branching_witness_copy_is_sloped :
  witness_snapshot_total_work [4; 4; 4; 4] = 16 /\
  length [4; 4; 4; 4] = 4.
Proof. split; reflexivity. Qed.

Theorem witness_snapshot_cost_is_sum_of_active_depths :
  forall leafDepths,
    witness_snapshot_total_work leafDepths = sum_nats leafDepths.
Proof.
  induction leafDepths as [|depth rest IH].
  - reflexivity.
  - unfold witness_snapshot_total_work in IH |- *. simpl. rewrite IH. reflexivity.
Qed.

Inductive ordinal_architecture_candidate : Type :=
| SafeWitnessSnapshotA
| PostorderOrdinalB
| ZeroSentinelC
| LocalActWriterD
| UpstreamPathAwareE.

Inductive candidate_obstacle : Type :=
| SlopedWitnessCopy
| ConsensusOrderChange
| FormatVersionChange
| DuplicatedEncoderAuthority
| RequiresUpstreamApi
| PublicRawRepresentationBreaksKeyBound.

Definition candidate_obstacles (candidate : ordinal_architecture_candidate)
    : list candidate_obstacle :=
  match candidate with
  | SafeWitnessSnapshotA =>
      [SlopedWitnessCopy; PublicRawRepresentationBreaksKeyBound]
  | PostorderOrdinalB => [ConsensusOrderChange]
  | ZeroSentinelC => [FormatVersionChange]
  | LocalActWriterD => [DuplicatedEncoderAuthority]
  | UpstreamPathAwareE => [RequiresUpstreamApi]
  end.

Theorem no_candidate_is_unconditionally_authorized_by_v2 :
  forall candidate, candidate_obstacles candidate <> [].
Proof. destruct candidate; discriminate. Qed.

(** Canonical-key decoder image, after the self-delimiting byte segments have
    been tokenized.  A bare key contains exactly one segment.  A split key
    contains zero or more segments plus its terminal marker.  Consequently
    the only strict valued-key prefix is a bare first segment of a split key.
    The PathMap root is a focus, not an encoded key. *)
Inductive canonical_key : Type :=
| CanonicalBare : nat -> canonical_key
| CanonicalSplit : list nat -> canonical_key.

Inductive strict_canonical_key_prefix : canonical_key -> canonical_key -> Prop :=
| BarePrefixesSplit : forall segment tail,
    strict_canonical_key_prefix
      (CanonicalBare segment) (CanonicalSplit (segment :: tail)).

Theorem no_bare_key_strictly_prefixes_a_bare_key :
  forall left right,
    ~ strict_canonical_key_prefix (CanonicalBare left) (CanonicalBare right).
Proof. intros left right impossible. inversion impossible. Qed.

Theorem no_split_key_strictly_prefixes_any_canonical_key :
  forall segments candidate,
    ~ strict_canonical_key_prefix (CanonicalSplit segments) candidate.
Proof. intros segments candidate impossible. inversion impossible. Qed.

Theorem canonical_key_strict_prefix_chain_has_at_most_two_values :
  forall first second third,
    strict_canonical_key_prefix first second ->
    ~ strict_canonical_key_prefix second third.
Proof.
  intros first second third firstPrefix secondPrefix.
  inversion firstPrefix. subst. inversion secondPrefix.
Qed.

Inductive admitted_epathmap_source : Type :=
| CheckedCanonicalKeys : list canonical_key -> admitted_epathmap_source
| PublicRawPathMap : list path -> admitted_epathmap_source.

Definition fixed_two_frame_witness_admissible
    (source : admitted_epathmap_source) : Prop :=
  match source with
  | CheckedCanonicalKeys _ => True
  | PublicRawPathMap _ => False
  end.

Theorem public_raw_pathmap_cannot_use_the_two_frame_bound_without_validation :
  forall raw, ~ fixed_two_frame_witness_admissible (PublicRawPathMap raw).
Proof. simpl. auto. Qed.

Definition zero_sentinel_map_ordinals (logicalValueCount : nat) : list nat :=
  repeat 0 logicalValueCount.

Theorem zero_sentinel_preserves_implicit_lex_body_cardinality :
  forall count,
    length (zero_sentinel_map_ordinals count) = count /\
    Forall (fun ordinal => ordinal = 0) (zero_sentinel_map_ordinals count).
Proof.
  intros. unfold zero_sentinel_map_ordinals. split.
  - apply repeat_length.
  - induction count; simpl; constructor; auto.
Qed.

Example postorder_ordinals_can_break_lex_streaming_alignment :
  [1; 0] <> seq 0 2.
Proof. discriminate. Qed.

(** ** Corrected pre-release EPM1 candidate: positional map bodies

    [ZeroSentinelC] does not use an ACT value as an ordinal.  Each valued
    endpoint carries the canonical zero sentinel and the separately framed
    protobuf bodies are associated, one at a time, with value endpoints in
    canonical lexicographic order.  The private map is an owned write zipper;
    it is never converted to a list of decoded [Par] values.  This is a
    candidate for amending the unreleased EPM1 definition, not an assertion
    that a deployed EPM1 version may silently change. *)

Inductive corrected_mode : Type :=
| CorrectedNeutral
| CorrectedSet
| CorrectedMap.

Record corrected_topology : Type := {
  corrected_terminals : list path;
  corrected_value_endpoints : list path
}.

Fixpoint source_body_paths_tree_at (prefix : path) (tree : canonical_tree)
    : list path :=
  match tree with
  | CNode value children =>
      (match value with Some _ => [prefix] | None => [] end) ++
      source_body_paths_forest_at prefix children
  end
with source_body_paths_forest_at (prefix : path) (forest : canonical_forest)
    : list path :=
  match forest with
  | FNil => []
  | FCons label child rest =>
      source_body_paths_tree_at (prefix ++ [label]) child ++
      source_body_paths_forest_at prefix rest
  end.

Definition source_body_cursor_paths (tree : canonical_tree) : list path :=
  source_body_paths_tree_at [] tree.

Definition act_value_cursor_paths (tree : canonical_tree) : list path :=
  map fst (ordinal_bindings (denote_tree tree)).

Lemma source_and_act_cursor_paths_mutual :
  (forall tree prefix isRoot,
      source_body_paths_tree_at prefix tree =
      map fst (ordinal_bindings (denote_tree_at isRoot prefix tree))) /\
  (forall forest prefix,
      source_body_paths_forest_at prefix forest =
      map fst (ordinal_bindings (denote_forest_at prefix forest))).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH prefix isRoot.
    simpl. destruct value; simpl; rewrite childrenIH; reflexivity.
  - intros prefix. reflexivity.
  - intros label child childIH rest restIH prefix. simpl.
    rewrite map_app, (childIH (prefix ++ [label]) false), restIH. reflexivity.
Qed.

Theorem independent_source_and_act_lex_walks_are_lockstep :
  forall tree,
    source_body_cursor_paths tree = act_value_cursor_paths tree /\
    length (source_body_cursor_paths tree) =
      length (act_value_cursor_paths tree).
Proof.
  intros tree. unfold source_body_cursor_paths, act_value_cursor_paths, denote_tree.
  rewrite (proj1 source_and_act_cursor_paths_mutual tree [] true). auto.
Qed.

Fixpoint zero_tree_values (tree : canonical_tree) : canonical_tree :=
  match tree with
  | CNode value children =>
      CNode (match value with Some _ => Some 0 | None => None end)
        (zero_forest_values children)
  end
with zero_forest_values (forest : canonical_forest) : canonical_forest :=
  match forest with
  | FNil => FNil
  | FCons label child rest =>
      FCons label (zero_tree_values child) (zero_forest_values rest)
  end.

Lemma zero_values_preserve_body_walk_mutual :
  (forall tree prefix,
      source_body_paths_tree_at prefix (zero_tree_values tree) =
      source_body_paths_tree_at prefix tree) /\
  (forall forest prefix,
      source_body_paths_forest_at prefix (zero_forest_values forest) =
      source_body_paths_forest_at prefix forest).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH prefix. simpl.
    destruct value; simpl; rewrite childrenIH; reflexivity.
  - reflexivity.
  - intros. simpl. rewrite H, H0. reflexivity.
Qed.

Definition corrected_topology_of_tree (tree : canonical_tree)
    : corrected_topology :=
  {| corrected_terminals := terminal_paths (denote_tree (zero_tree_values tree));
     corrected_value_endpoints := act_value_cursor_paths (zero_tree_values tree) |}.

(** This is the logical producer/parser binding.  It is deliberately not the
    still-missing ACTree03 byte-level refinement theorem. *)
Definition corrected_logical_act_binding (source : canonical_tree)
    (events : list logical_event) (topology : corrected_topology) : Prop :=
  events = emit_tree (zero_tree_values source) /\
  parse_event_program events = Some (zero_tree_values source) /\
  topology = corrected_topology_of_tree source.

Theorem corrected_logical_act_parser_is_bound_to_canonical_producer :
  forall source,
    corrected_logical_act_binding source
      (emit_tree (zero_tree_values source)) (corrected_topology_of_tree source).
Proof.
  intros. unfold corrected_logical_act_binding. repeat split; auto.
  apply valid_event_parser_complete.
Qed.

Theorem corrected_act_and_source_body_cursor_exhaust_together :
  forall source events topology,
    corrected_logical_act_binding source events topology ->
    corrected_value_endpoints topology = source_body_cursor_paths source /\
    length (corrected_value_endpoints topology) =
      length (source_body_cursor_paths source).
Proof.
  intros source events topology [_ [_ sameTopology]]. subst.
  change (act_value_cursor_paths (zero_tree_values source) =
      source_body_cursor_paths source /\
    length (act_value_cursor_paths (zero_tree_values source)) =
      length (source_body_cursor_paths source)).
  assert (sameWalk : act_value_cursor_paths (zero_tree_values source) =
      source_body_cursor_paths source).
  { transitivity (source_body_cursor_paths (zero_tree_values source)).
    - symmetry. apply (proj1
        (independent_source_and_act_lex_walks_are_lockstep
          (zero_tree_values source))).
    - unfold source_body_cursor_paths.
      apply (proj1 zero_values_preserve_body_walk_mutual). }
  now rewrite sameWalk.
Qed.

Example corrected_lockstep_covers_root_and_prefixed_value :
  let source :=
    CNode (Some 9) (FCons 1 (CNode (Some 12) FNil) FNil) in
  source_body_cursor_paths source = [[]; [1]] /\
  corrected_value_endpoints (corrected_topology_of_tree source) = [[]; [1]].
Proof. simpl. auto. Qed.

Example corrected_lockstep_covers_topology_only_leaf :
  let source := CNode None (FCons 1 (CNode None FNil) FNil) in
  source_body_cursor_paths source = [] /\
  corrected_value_endpoints (corrected_topology_of_tree source) = [].
Proof. simpl. auto. Qed.

Definition corrected_topology_valid (mode : corrected_mode)
    (topology : corrected_topology) : Prop :=
  match mode with
  | CorrectedNeutral =>
      corrected_terminals topology = [] /\
      corrected_value_endpoints topology = []
  | CorrectedSet =>
      corrected_terminals topology <> [] /\
      corrected_terminals topology = corrected_value_endpoints topology /\
      NoDup (corrected_value_endpoints topology)
  | CorrectedMap =>
      corrected_terminals topology <> [] /\
      NoDup (corrected_terminals topology) /\
      NoDup (corrected_value_endpoints topology) /\
      (forall endpoint, In endpoint (corrected_value_endpoints topology) ->
         In endpoint (corrected_terminals topology)) /\
      (In [] (corrected_terminals topology) ->
         In [] (corrected_value_endpoints topology))
  end.

Fixpoint replay_topology (paths : list path) (private : pathmap_model)
    : pathmap_model :=
  match paths with
  | [] => private
  | endpoint :: rest =>
      replay_topology rest
        (zipper_into_map
          (zipper_create_path
            {| zipper_focus := endpoint; zipper_contents := private |}))
  end.

Fixpoint replay_set (paths : list path) (private : pathmap_model)
    : pathmap_model :=
  match paths with
  | [] => private
  | endpoint :: rest =>
      replay_set rest (pathmap_set_at endpoint 0 private)
  end.

Lemma ensure_terminal_membership :
  forall added terminals candidate,
    In candidate (ensure_terminal added terminals) <->
    candidate = added \/ In candidate terminals.
Proof.
  intros added terminals candidate. unfold ensure_terminal.
  destruct (path_member added terminals) eqn:present; simpl.
  - apply path_member_spec in present. split; intros member.
    + right. exact member.
    + destruct member as [same | member]; [subst; exact present | exact member].
  - split; intros member.
    + destruct member as [same | member].
      * left. symmetry. exact same.
      * right. exact member.
    + destruct member as [same | member].
      * left. symmetry. exact same.
      * right. exact member.
Qed.

Lemma ensure_terminal_nodup :
  forall added terminals,
    NoDup terminals -> NoDup (ensure_terminal added terminals).
Proof.
  intros added terminals unique. unfold ensure_terminal.
  destruct (path_member added terminals) eqn:present; [exact unique |].
  constructor.
  - intro member. apply path_member_spec in member. congruence.
  - exact unique.
Qed.

Lemma replay_topology_extensional_membership :
  forall paths private candidate,
    In candidate (pm_terminals (replay_topology paths private)) <->
    In candidate paths \/ In candidate (pm_terminals private).
Proof.
  induction paths as [|head rest IH]; intros private candidate.
  - simpl. tauto.
  - simpl. rewrite IH. unfold zipper_into_map, zipper_create_path. simpl.
    rewrite ensure_terminal_membership. intuition congruence.
Qed.

Lemma replay_topology_has_no_duplicate_terminals :
  forall paths private,
    NoDup paths -> NoDup (pm_terminals private) ->
    (forall candidate, In candidate paths ->
       ~ In candidate (pm_terminals private)) ->
    NoDup (pm_terminals (replay_topology paths private)).
Proof.
  induction paths as [|head rest IH]; intros private pathsUnique privateUnique disjoint.
  - exact privateUnique.
  - inversion pathsUnique as [|head' rest' headFresh restUnique]; subst.
    simpl. apply IH.
    + exact restUnique.
    + apply ensure_terminal_nodup. exact privateUnique.
    + intros candidate inRest inEnsured.
      apply ensure_terminal_membership in inEnsured.
      destruct inEnsured as [same | inPrivate].
      * subst. contradiction.
      * eapply disjoint; [right; exact inRest | exact inPrivate].
Qed.

Lemma remove_path_membership :
  forall (A : Type) needle candidate value (entries : list (path * A)),
    In (candidate, value) (remove_path needle entries) <->
    candidate <> needle /\ In (candidate, value) entries.
Proof.
  intros A needle candidate value entries.
  induction entries as [|[headPath headValue] rest IH]; simpl.
  - tauto.
  - destruct (path_eq_dec needle headPath) as [same | different].
    + subst. rewrite IH. split.
      * intros [notSame member]. split; [exact notSame | right; exact member].
      * intros [notSame [equal | member]].
        { inversion equal. exfalso. apply notSame. symmetry. exact H0. }
        { split; assumption. }
    + simpl. rewrite IH. split.
      * intros [equal | [notSame member]].
        { inversion equal. subst. split; [congruence | left; reflexivity]. }
        { split; [exact notSame | right; exact member]. }
      * intros [notSame [equal | member]].
        { left. exact equal. }
        { right. split; assumption. }
Qed.

Lemma pathmap_set_at_membership :
  forall location installed candidate value private,
    In (candidate, value) (pm_values (pathmap_set_at location installed private)) <->
    (candidate = location /\ value = installed) \/
    (candidate <> location /\ In (candidate, value) (pm_values private)).
Proof.
  intros. unfold pathmap_set_at, zipper_into_map, zipper_set_val. simpl.
  rewrite remove_path_membership. split; intros member.
  - destruct member as [same | old].
    + inversion same. auto.
    + right. exact old.
  - destruct member as [[same valueSame] | old].
    + subst. left. reflexivity.
    + right. exact old.
Qed.

Lemma remove_path_keys_nodup :
  forall (A : Type) needle (entries : list (path * A)),
    NoDup (map fst entries) -> NoDup (map fst (remove_path needle entries)).
Proof.
  intros A needle entries. induction entries as [|[head value] rest IH]; simpl.
  - auto.
  - intro unique. inversion unique as [|head' rest' fresh restUnique]; subst.
    destruct (path_eq_dec needle head).
    + apply IH. exact restUnique.
    + simpl. constructor.
      * intro member. apply in_map_iff in member.
        destruct member as [[candidate found] [same member]]. simpl in same. subst.
        apply remove_path_membership in member. destruct member as [_ member].
        apply fresh. apply in_map_iff. exists (head, found). split; auto.
      * apply IH. exact restUnique.
Qed.

Lemma pathmap_set_at_keys_nodup :
  forall location installed private,
    NoDup (map fst (pm_values private)) ->
    NoDup (map fst (pm_values (pathmap_set_at location installed private))).
Proof.
  intros. unfold pathmap_set_at, zipper_into_map, zipper_set_val. simpl.
  constructor.
  - intro member. apply in_map_iff in member.
    destruct member as [[candidate value] [same member]]. simpl in same. subst.
    apply remove_path_membership in member. tauto.
  - apply remove_path_keys_nodup. exact H.
Qed.

Lemma replay_set_extensional_membership :
  forall paths private candidate value,
    In (candidate, value) (pm_values (replay_set paths private)) <->
    (value = 0 /\ In candidate paths) \/
    (~ In candidate paths /\ In (candidate, value) (pm_values private)).
Proof.
  induction paths as [|head rest IH]; intros private candidate value.
  - simpl. tauto.
  - simpl. rewrite IH, pathmap_set_at_membership. simpl.
    destruct (path_eq_dec candidate head) as [same | different].
    + subst. split; intros.
      * destruct H as [[valueZero inRest] |
          [notInRest [[same valueZero] | [impossible _]]]].
        { left. split; [exact valueZero | left; reflexivity]. }
        { left. split; [exact valueZero | left; reflexivity]. }
        { contradiction. }
      * destruct H as [[valueZero _] | [notMember _]].
        { destruct (in_dec path_eq_dec head rest) as [inRest | notInRest].
          - left. split; assumption.
          - right. split; [exact notInRest |]. left. auto. }
        { exfalso. apply notMember. left. reflexivity. }
    + split; intros source.
      * destruct source as [[valueZero inRest] |
          [notInRest [[same valueSame] | [notSame inPrivate]]]].
        { left. split; [exact valueZero | right; exact inRest]. }
        { contradiction. }
        { right. split.
          - intros [same | inRest].
            { apply different. symmetry. exact same. }
            { apply notInRest. exact inRest. }
          - exact inPrivate. }
      * destruct source as [[valueZero [same | inRest]] | [notInAll inPrivate]].
        { exfalso. apply different. symmetry. exact same. }
        { left. split; assumption. }
        { right. split.
          - intro inRest. apply notInAll. right. exact inRest.
          - right. split; [exact different | exact inPrivate]. }
Qed.

Corollary replay_set_from_empty_is_exact :
  forall paths candidate value,
    In (candidate, value)
      (pm_values (replay_set paths {| pm_terminals := []; pm_values := [] |})) <->
    value = 0 /\ In candidate paths.
Proof.
  intros. rewrite replay_set_extensional_membership. simpl. tauto.
Qed.

Lemma replay_set_has_no_duplicate_value_keys :
  forall paths private,
    NoDup (map fst (pm_values private)) ->
    NoDup (map fst (pm_values (replay_set paths private))).
Proof.
  induction paths as [|head rest IH]; intros private unique.
  - exact unique.
  - simpl. apply IH. apply pathmap_set_at_keys_nodup. exact unique.
Qed.

Lemma replay_topology_preserves_existing_terminal :
  forall paths private endpoint,
    In endpoint (pm_terminals private) ->
    In endpoint (pm_terminals (replay_topology paths private)).
Proof.
  induction paths as [|head rest IH]; intros private endpoint present.
  - exact present.
  - simpl. apply IH. unfold zipper_into_map, zipper_create_path. simpl.
    unfold ensure_terminal.
    destruct (path_member head (pm_terminals private)); simpl; auto.
Qed.

Lemma replay_topology_preserves_value_free_endpoint :
  forall paths private endpoint,
    In endpoint paths ->
    In endpoint (pm_terminals (replay_topology paths private)).
Proof.
  induction paths as [|head rest IH]; intros private endpoint member.
  - contradiction.
  - simpl in member |- *. destruct member as [same | member].
    + subst. apply replay_topology_preserves_existing_terminal.
      unfold zipper_into_map, zipper_create_path. simpl.
      unfold ensure_terminal.
      destruct (path_member endpoint (pm_terminals private)) eqn:present; simpl.
      * apply path_member_spec. exact present.
      * left. reflexivity.
    + apply IH with (private :=
        zipper_into_map
          (zipper_create_path
            {| zipper_focus := head; zipper_contents := private |})).
      exact member.
Qed.

Lemma replay_set_preserves_installed_unit :
  forall paths private endpoint,
    In (endpoint, 0) (pm_values private) ->
    In (endpoint, 0) (pm_values (replay_set paths private)).
Proof.
  induction paths as [|head rest IH]; intros private endpoint present.
  - exact present.
  - simpl. apply IH. unfold pathmap_set_at.
    destruct (path_eq_dec endpoint head) as [same | different].
    + subst. unfold zipper_into_map, zipper_set_val. simpl. left. reflexivity.
    + apply set_val_preserves_unrelated_values.
      * simpl. exact different.
      * exact present.
Qed.

Lemma replay_set_installs_every_unit_endpoint :
  forall paths private endpoint,
    In endpoint paths ->
    In (endpoint, 0) (pm_values (replay_set paths private)).
Proof.
  induction paths as [|head rest IH]; intros private endpoint member.
  - contradiction.
  - simpl in member |- *. destruct member as [same | member].
    + subst. apply replay_set_preserves_installed_unit.
      unfold pathmap_set_at, zipper_into_map, zipper_set_val. simpl. left. reflexivity.
    + apply IH with (private := pathmap_set_at head 0 private). exact member.
Qed.

Inductive pending_body_target : Type :=
| StoreAtEndpoint (location : path)
| DiscardForDeferredAct.

Record positional_decode_state : Type := {
  positional_remaining_endpoints : list path;
  positional_remaining_packets : list body_packet;
  positional_pending : option pending_body_target;
  positional_private : pathmap_model;
  positional_owned_focus : path;
  positional_parent_before : pathmap_model;
  positional_next_ordinal : nat;
  positional_pairs : list (path * body);
  positional_first_noncanonical : option nat;
  positional_child_error : option nat;
  positional_deferred_act : bool;
  positional_association_bad : bool;
  positional_publications : nat
}.

Definition positional_initial_map (topology : corrected_topology)
    (packets : list body_packet) (parent : pathmap_model)
    (act_valid : bool) : positional_decode_state :=
  let private := replay_topology (corrected_terminals topology)
      {| pm_terminals := []; pm_values := [] |} in
  {| positional_remaining_endpoints :=
       if act_valid then corrected_value_endpoints topology else [];
     positional_remaining_packets := packets;
     positional_pending := None;
     positional_private := private;
     positional_owned_focus := [];
     positional_parent_before := parent;
     positional_next_ordinal := 0;
     positional_pairs := [];
     positional_first_noncanonical := None;
     positional_child_error := None;
     positional_deferred_act := negb act_valid;
     positional_association_bad := false;
     positional_publications := 0 |}.

Record admitted_corrected_map_input : Type := {
  admitted_source_tree : canonical_tree;
  admitted_act_events : list logical_event;
  admitted_topology : corrected_topology;
  admitted_logical_binding : corrected_logical_act_binding
    admitted_source_tree admitted_act_events admitted_topology;
  admitted_topology_invariant : corrected_topology_valid CorrectedMap
    admitted_topology
}.

Definition positional_initial_admitted_map
    (admitted : admitted_corrected_map_input)
    (packets : list body_packet) (parent : pathmap_model)
    : positional_decode_state :=
  positional_initial_map (admitted_topology admitted) packets parent true.

(** [positional_yield] moves the owned zipper to the borrowed endpoint before
    the generated child PDA is yielded.  It consumes neither the packet nor
    the body.  When ACT validation failed, a discard target preserves the
    production precedence by still decoding framed child bodies in wire
    order. *)
Definition positional_yield (state : positional_decode_state)
    : positional_decode_state :=
  match positional_pending state, positional_remaining_packets state with
  | Some _, _ => state
  | None, [] => state
  | None, _ :: _ =>
      match positional_remaining_endpoints state with
      | endpoint :: endpoints =>
          {| positional_remaining_endpoints := endpoints;
             positional_remaining_packets := positional_remaining_packets state;
             positional_pending := Some (StoreAtEndpoint endpoint);
             positional_private := positional_private state;
             positional_owned_focus := endpoint;
             positional_parent_before := positional_parent_before state;
             positional_next_ordinal := positional_next_ordinal state;
             positional_pairs := positional_pairs state;
             positional_first_noncanonical := positional_first_noncanonical state;
             positional_child_error := positional_child_error state;
             positional_deferred_act := positional_deferred_act state;
             positional_association_bad := positional_association_bad state;
             positional_publications := positional_publications state |}
      | [] =>
          {| positional_remaining_endpoints := [];
             positional_remaining_packets := positional_remaining_packets state;
             positional_pending := Some DiscardForDeferredAct;
             positional_private := positional_private state;
             positional_owned_focus := positional_owned_focus state;
             positional_parent_before := positional_parent_before state;
             positional_next_ordinal := positional_next_ordinal state;
             positional_pairs := positional_pairs state;
             positional_first_noncanonical := positional_first_noncanonical state;
             positional_child_error := positional_child_error state;
             positional_deferred_act := positional_deferred_act state;
             positional_association_bad := true;
             positional_publications := positional_publications state |}
      end
  end.

(** [positional_push] is the only mutation point.  Its endpoint is owned in
    [positional_pending], so no borrowed ACT cursor path lives across
    [set_val].  It consumes exactly the packet that caused the suspension. *)
Definition positional_push (state : positional_decode_state)
    : positional_decode_state :=
  match positional_pending state, positional_remaining_packets state with
  | None, _ => state
  | Some _, [] => state
  | Some target, packet :: packets =>
      let ordinal := positional_next_ordinal state in
      match packet_decoded packet with
      | None =>
          {| positional_remaining_endpoints := positional_remaining_endpoints state;
             positional_remaining_packets := packets;
             positional_pending := None;
             positional_private := positional_private state;
             positional_owned_focus := positional_owned_focus state;
             positional_parent_before := positional_parent_before state;
             positional_next_ordinal := ordinal;
             positional_pairs := positional_pairs state;
             positional_first_noncanonical := positional_first_noncanonical state;
             positional_child_error := Some ordinal;
             positional_deferred_act := positional_deferred_act state;
             positional_association_bad := positional_association_bad state;
             positional_publications := positional_publications state |}
      | Some decoded =>
          let private :=
            match target with
            | StoreAtEndpoint endpoint =>
                zipper_into_map
                  (zipper_set_val decoded
                    {| zipper_focus := positional_owned_focus state;
                       zipper_contents := positional_private state |})
            | DiscardForDeferredAct => positional_private state
            end in
          let pairs :=
            match target with
            | StoreAtEndpoint endpoint =>
                (endpoint, decoded) :: positional_pairs state
            | DiscardForDeferredAct => positional_pairs state
            end in
          {| positional_remaining_endpoints := positional_remaining_endpoints state;
             positional_remaining_packets := packets;
             positional_pending := None;
             positional_private := private;
             positional_owned_focus := positional_owned_focus state;
             positional_parent_before := positional_parent_before state;
             positional_next_ordinal := S ordinal;
             positional_pairs := pairs;
             positional_first_noncanonical :=
               note_noncanonical ordinal (packet_canonical packet)
                 (positional_first_noncanonical state);
             positional_child_error := positional_child_error state;
             positional_deferred_act := positional_deferred_act state;
             positional_association_bad := positional_association_bad state;
             positional_publications := positional_publications state |}
      end
  end.

Definition positional_round (state : positional_decode_state)
    : positional_decode_state := positional_push (positional_yield state).

Fixpoint positional_run (fuel : nat) (state : positional_decode_state)
    : positional_decode_state :=
  match fuel with
  | 0 => state
  | S rest =>
      match positional_child_error state, positional_remaining_packets state with
      | Some _, _ => state
      | None, [] => state
      | None, _ :: _ => positional_run rest (positional_round state)
      end
  end.

Definition canonical_packet_of_body (decoded : body) : body_packet :=
  {| packet_wire_bytes := 1;
     packet_decoded := Some decoded;
     packet_canonical := true |}.

Definition canonical_packets_of_bodies (bodies : list body) : list body_packet :=
  map canonical_packet_of_body bodies.

Fixpoint install_endpoint_bodies (endpoints : list path) (bodies : list body)
    (private : pathmap_model) : pathmap_model :=
  match endpoints, bodies with
  | endpoint :: endpointRest, decoded :: bodyRest =>
      install_endpoint_bodies endpointRest bodyRest
        (pathmap_set_at endpoint decoded private)
  | _, _ => private
  end.

Definition positional_finished (state : positional_decode_state) : Prop :=
  positional_remaining_packets state = [] /\
  positional_pending state = None.

Definition empty_listb {A : Type} (items : list A) : bool :=
  match items with [] => true | _ :: _ => false end.

Definition empty_optionb {A : Type} (item : option A) : bool :=
  match item with None => true | Some _ => false end.

Definition positional_successb (state : positional_decode_state) : bool :=
  empty_listb (positional_remaining_packets state) &&
  empty_optionb (positional_pending state) &&
  empty_listb (positional_remaining_endpoints state) &&
  empty_optionb (positional_child_error state) &&
  negb (positional_deferred_act state) &&
  negb (positional_association_bad state) &&
  empty_optionb (positional_first_noncanonical state) &&
  Nat.eqb (positional_publications state) 0.

Definition positional_success (state : positional_decode_state) : Prop :=
  positional_successb state = true.

Definition positional_publish (state : positional_decode_state)
    : pathmap_model * nat :=
  if positional_successb state then
    (zipper_into_map
       {| zipper_focus := [];
          zipper_contents := positional_private state |}, 1)
  else (positional_parent_before state, 0).

Definition positional_final_error (state : positional_decode_state)
    : option decode_error :=
  match positional_child_error state with
  | Some ordinal => Some (ChildPdaFailure ordinal)
  | None =>
      if positional_deferred_act state then Some DeferredActFailure
      else if positional_association_bad state then Some ModeOrOrdinalFailure
      else match positional_remaining_endpoints state with
           | _ :: _ => Some ModeOrOrdinalFailure
           | [] =>
               match positional_first_noncanonical state with
               | Some ordinal => Some (NoncanonicalBodyFailure ordinal)
               | None => None
               end
           end
  end.

Theorem yield_does_not_consume_or_decode_a_body :
  forall state,
    positional_remaining_packets (positional_yield state) =
      positional_remaining_packets state /\
    positional_next_ordinal (positional_yield state) =
      positional_next_ordinal state /\
    positional_private (positional_yield state) = positional_private state.
Proof.
  intros [endpoints packets pending private parent next pairs noncanonical
    child deferred association publications]. unfold positional_yield. simpl.
  destruct pending; [auto |].
  destruct packets; [auto |].
  destruct endpoints; auto.
Qed.

Theorem successful_yield_has_exactly_one_pending_endpoint :
  forall state packet packets endpoint endpoints,
    positional_pending state = None ->
    positional_remaining_packets state = packet :: packets ->
    positional_remaining_endpoints state = endpoint :: endpoints ->
    positional_pending (positional_yield state) =
      Some (StoreAtEndpoint endpoint) /\
    positional_remaining_endpoints (positional_yield state) = endpoints /\
    positional_owned_focus (positional_yield state) = endpoint.
Proof.
  intros. unfold positional_yield. now rewrite H, H0, H1.
Qed.

Theorem push_without_pending_endpoint_is_a_noop :
  forall state,
    positional_pending state = None -> positional_push state = state.
Proof. intros state none. unfold positional_push. now rewrite none. Qed.

Theorem pushed_endpoint_body_is_paired_once_at_the_owned_focus :
  forall state endpoint packet packets decoded,
    positional_pending state = Some (StoreAtEndpoint endpoint) ->
    positional_owned_focus state = endpoint ->
    positional_remaining_packets state = packet :: packets ->
    packet_decoded packet = Some decoded ->
    positional_remaining_packets (positional_push state) = packets /\
    positional_pending (positional_push state) = None /\
    positional_pairs (positional_push state) =
      (endpoint, decoded) :: positional_pairs state /\
    In (endpoint, decoded)
      (pm_values (positional_private (positional_push state))).
Proof.
  intros state endpoint packet packets decoded pending focus packetsHead decodedBody.
  unfold positional_push. rewrite pending, packetsHead, decodedBody, focus. simpl.
  repeat split; auto.
Qed.

Theorem root_body_uses_owned_zipper_root_set_val :
  forall state packet packets decoded,
    positional_pending state = Some (StoreAtEndpoint []) ->
    positional_owned_focus state = [] ->
    positional_remaining_packets state = packet :: packets ->
    packet_decoded packet = Some decoded ->
    In ([], decoded) (pm_values (positional_private (positional_push state))).
Proof.
  intros. eapply (proj2 (proj2 (proj2
    (pushed_endpoint_body_is_paired_once_at_the_owned_focus
      state [] packet packets decoded H H0 H1 H2)))).
Qed.

Theorem decoded_body_never_mutates_without_an_endpoint_target :
  forall state packet packets decoded,
    positional_pending state = Some DiscardForDeferredAct ->
    positional_remaining_packets state = packet :: packets ->
    packet_decoded packet = Some decoded ->
    positional_private (positional_push state) = positional_private state /\
    positional_pairs (positional_push state) = positional_pairs state.
Proof. intros. unfold positional_push. now rewrite H, H0, H1. Qed.

Theorem child_failure_outranks_deferred_act_in_positional_decoder :
  forall state packet packets target,
    positional_pending state = Some target ->
    positional_remaining_packets state = packet :: packets ->
    packet_decoded packet = None ->
    positional_child_error (positional_push state) =
      Some (positional_next_ordinal state) /\
    positional_deferred_act (positional_push state) =
      positional_deferred_act state.
Proof. intros. unfold positional_push. now rewrite H, H0, H1. Qed.

Theorem positional_error_precedence_is_child_then_act_then_association_then_body :
  forall state childOrdinal,
    positional_child_error state = Some childOrdinal ->
    positional_final_error state = Some (ChildPdaFailure childOrdinal).
Proof. intros. unfold positional_final_error. now rewrite H. Qed.

Theorem positional_deferred_act_outranks_noncanonical :
  forall state,
    positional_child_error state = None ->
    positional_deferred_act state = true ->
    positional_final_error state = Some DeferredActFailure.
Proof. intros. unfold positional_final_error. now rewrite H, H0. Qed.

Theorem positional_association_outranks_noncanonical :
  forall state,
    positional_child_error state = None ->
    positional_deferred_act state = false ->
    positional_association_bad state = true ->
    positional_final_error state = Some ModeOrOrdinalFailure.
Proof. intros. unfold positional_final_error. now rewrite H, H0, H1. Qed.

Theorem too_few_bodies_leave_a_dangling_endpoint :
  forall state endpoint endpoints,
    positional_child_error state = None ->
    positional_deferred_act state = false ->
    positional_association_bad state = false ->
    positional_remaining_packets state = [] ->
    positional_remaining_endpoints state = endpoint :: endpoints ->
    positional_final_error state = Some ModeOrOrdinalFailure.
Proof.
  intros. unfold positional_final_error. now rewrite H, H0, H1, H3.
Qed.

Theorem positional_round_consumes_one_decoded_packet :
  forall state packet packets decoded,
    positional_pending state = None ->
    positional_remaining_packets state = packet :: packets ->
    packet_decoded packet = Some decoded ->
    positional_remaining_packets (positional_round state) = packets /\
    positional_pending (positional_round state) = None /\
    positional_next_ordinal (positional_round state) =
      S (positional_next_ordinal state) /\
    positional_child_error (positional_round state) =
      positional_child_error state.
Proof.
  intros state packet packets decoded noPending packetsHead decodedBody.
  unfold positional_round, positional_yield, positional_push.
  rewrite noPending, packetsHead.
  destruct (positional_remaining_endpoints state) as [|endpoint endpoints];
    simpl; rewrite decodedBody; auto.
Qed.

Lemma positional_happy_round_exact :
  forall state endpoint endpoints decoded bodies,
    positional_pending state = None ->
    positional_remaining_endpoints state = endpoint :: endpoints ->
    positional_remaining_packets state =
      canonical_packet_of_body decoded :: canonical_packets_of_bodies bodies ->
    positional_child_error state = None ->
    let after := positional_round state in
    positional_remaining_endpoints after = endpoints /\
    positional_remaining_packets after = canonical_packets_of_bodies bodies /\
    positional_pending after = None /\
    positional_owned_focus after = endpoint /\
    positional_private after =
      pathmap_set_at endpoint decoded (positional_private state) /\
    positional_pairs after =
      (endpoint, decoded) :: positional_pairs state /\
    positional_child_error after = None /\
    positional_first_noncanonical after =
      positional_first_noncanonical state /\
    positional_deferred_act after = positional_deferred_act state /\
    positional_association_bad after = positional_association_bad state.
Proof.
  intros state endpoint endpoints decoded bodies noPending endpointHead packetHead noChild.
  unfold positional_round, positional_yield, positional_push.
  rewrite noPending, endpointHead, packetHead. simpl. rewrite noChild.
  unfold pathmap_set_at, zipper_into_map, zipper_set_val. simpl.
  assert (canonicalSame : note_noncanonical (positional_next_ordinal state) true
      (positional_first_noncanonical state) =
      positional_first_noncanonical state).
  { unfold note_noncanonical. destruct (positional_first_noncanonical state);
      reflexivity. }
  rewrite canonicalSame.
  repeat split; reflexivity.
Qed.

Theorem positional_happy_run_is_complete_and_extensional :
  forall endpoints bodies state,
    length endpoints = length bodies ->
    positional_pending state = None ->
    positional_remaining_endpoints state = endpoints ->
    positional_remaining_packets state = canonical_packets_of_bodies bodies ->
    positional_child_error state = None ->
    let after := positional_run (length bodies) state in
    positional_remaining_endpoints after = [] /\
    positional_remaining_packets after = [] /\
    positional_pending after = None /\
    rev (positional_pairs after) =
      rev (positional_pairs state) ++ combine endpoints bodies /\
    positional_private after =
      install_endpoint_bodies endpoints bodies (positional_private state) /\
    positional_child_error after = None /\
    positional_first_noncanonical after =
      positional_first_noncanonical state /\
    positional_deferred_act after = positional_deferred_act state /\
    positional_association_bad after = positional_association_bad state.
Proof.
  induction endpoints as [|endpoint endpointRest IH];
    intros bodies state sameLength noPending endpointState packetState noChild.
  - destruct bodies; [|simpl in sameLength; discriminate].
    simpl in *. subst. repeat split; auto. now rewrite app_nil_r.
  - destruct bodies as [|decoded bodyRest]; [discriminate |].
    simpl in sameLength. inversion sameLength as [restLength].
    assert (runStep :
      positional_run (length (decoded :: bodyRest)) state =
      positional_run (length bodyRest) (positional_round state)).
    { simpl. rewrite noChild, packetState. reflexivity. }
    rewrite runStep.
    pose proof (positional_happy_round_exact state endpoint endpointRest decoded
      bodyRest noPending endpointState packetState noChild) as round.
    destruct round as [roundEndpoints [roundPackets [roundPending
      [roundFocus [roundPrivate [roundPairs roundChild]]]]]].
    destruct roundChild as [roundChild [roundCanonical
      [roundAct roundAssociation]]].
    pose proof (IH bodyRest (positional_round state) restLength
      roundPending roundEndpoints roundPackets roundChild) as tail.
    simpl in tail. destruct tail as [finalEndpoints [finalPackets [finalPending
      [finalPairs [finalPrivate [finalChild [finalCanonical
        [finalAct finalAssociation]]]]]]]].
    repeat split; auto.
    + rewrite finalPairs, roundPairs. simpl. rewrite <- app_assoc. reflexivity.
    + rewrite finalPrivate, roundPrivate. reflexivity.
    + now rewrite finalCanonical, roundCanonical.
    + now rewrite finalAct, roundAct.
    + now rewrite finalAssociation, roundAssociation.
Qed.

Theorem admitted_map_happy_run_refines_independent_lockstep :
  forall admitted bodies parent,
    length (corrected_value_endpoints (admitted_topology admitted)) =
      length bodies ->
    let initial := positional_initial_admitted_map admitted
      (canonical_packets_of_bodies bodies) parent in
    let after := positional_run (length bodies) initial in
    positional_remaining_endpoints after = [] /\
    positional_remaining_packets after = [] /\
    positional_pending after = None /\
    rev (positional_pairs after) =
      combine (corrected_value_endpoints (admitted_topology admitted)) bodies /\
    positional_private after =
      install_endpoint_bodies
        (corrected_value_endpoints (admitted_topology admitted)) bodies
        (replay_topology (corrected_terminals (admitted_topology admitted))
          {| pm_terminals := []; pm_values := [] |}) /\
    positional_child_error after = None /\
    positional_first_noncanonical after = None /\
    positional_deferred_act after = false /\
    positional_association_bad after = false.
Proof.
  intros admitted bodies parent sameLength. simpl.
  pose proof (positional_happy_run_is_complete_and_extensional
    (corrected_value_endpoints (admitted_topology admitted)) bodies
    (positional_initial_admitted_map admitted
      (canonical_packets_of_bodies bodies) parent)
    sameLength) as run.
  simpl in run. specialize (run eq_refl eq_refl eq_refl eq_refl).
  destruct run as [endpoints [packets [pending [pairs [private
    [child [canonical [act association]]]]]]]].
  repeat split; assumption.
Qed.

Theorem admitted_topology_cannot_be_an_unchecked_unsorted_or_duplicate_list :
  forall admitted,
    corrected_topology_valid CorrectedMap (admitted_topology admitted) /\
    corrected_logical_act_binding (admitted_source_tree admitted)
      (admitted_act_events admitted) (admitted_topology admitted).
Proof.
  intros. split; [apply admitted_topology_invariant | apply admitted_logical_binding].
Qed.

Lemma pathmap_set_at_existing_terminal_preserves_terminal_list :
  forall endpoint decoded private,
    In endpoint (pm_terminals private) ->
    pm_terminals (pathmap_set_at endpoint decoded private) =
      pm_terminals private.
Proof.
  intros. unfold pathmap_set_at, zipper_into_map, zipper_set_val. simpl.
  unfold ensure_terminal. apply path_member_spec in H. now rewrite H.
Qed.

Lemma install_endpoint_bodies_preserves_terminal_list :
  forall endpoints bodies private,
    (forall endpoint, In endpoint endpoints ->
       In endpoint (pm_terminals private)) ->
    pm_terminals (install_endpoint_bodies endpoints bodies private) =
      pm_terminals private.
Proof.
  induction endpoints as [|endpoint rest IH]; intros bodies private contained.
  - reflexivity.
  - destruct bodies as [|decoded bodyRest]; [reflexivity |]. simpl.
    rewrite IH.
    + apply pathmap_set_at_existing_terminal_preserves_terminal_list.
      apply contained. left. reflexivity.
    + intros candidate inRest.
      rewrite pathmap_set_at_existing_terminal_preserves_terminal_list.
      * apply contained. right. exact inRest.
      * apply contained. left. reflexivity.
Qed.

Lemma install_endpoint_bodies_preserves_unique_value_keys :
  forall endpoints bodies private,
    NoDup (map fst (pm_values private)) ->
    NoDup (map fst
      (pm_values (install_endpoint_bodies endpoints bodies private))).
Proof.
  induction endpoints as [|endpoint rest IH]; intros bodies private unique.
  - exact unique.
  - destruct bodies as [|decoded bodyRest]; [exact unique |]. simpl.
    apply IH. apply pathmap_set_at_keys_nodup. exact unique.
Qed.

Theorem one_extra_body_is_consumed_then_rejected :
  forall state packet decoded,
    positional_pending state = None ->
    positional_remaining_endpoints state = [] ->
    positional_remaining_packets state = [packet] ->
    packet_decoded packet = Some decoded ->
    positional_child_error state = None ->
    positional_deferred_act state = false ->
    positional_final_error (positional_round state) =
      Some ModeOrOrdinalFailure.
Proof.
  intros state packet decoded noPending noEndpoints packets decodedBody noChild noAct.
  unfold positional_round, positional_yield, positional_push.
  rewrite noPending, packets, noEndpoints. simpl. rewrite decodedBody. simpl.
  unfold positional_final_error. simpl. now rewrite noChild, noAct.
Qed.

Theorem positional_failure_never_publishes_private_map :
  forall state,
    ~ positional_success state ->
    positional_publish state = (positional_parent_before state, 0).
Proof.
  intros state failed. unfold positional_publish.
  unfold positional_success in failed.
  destruct (positional_successb state) eqn:success; [contradiction | reflexivity].
Qed.

Theorem positional_success_returns_one_publication_result :
  forall state,
    positional_success state ->
    positional_publish state = (positional_private state, 1).
Proof.
  intros state success. unfold positional_publish.
  unfold positional_success in success. now rewrite success.
Qed.

Definition positional_with_publications (state : positional_decode_state)
    (count : nat) : positional_decode_state :=
  {| positional_remaining_endpoints := positional_remaining_endpoints state;
     positional_remaining_packets := positional_remaining_packets state;
     positional_pending := positional_pending state;
     positional_private := positional_private state;
     positional_owned_focus := positional_owned_focus state;
     positional_parent_before := positional_parent_before state;
     positional_next_ordinal := positional_next_ordinal state;
     positional_pairs := positional_pairs state;
     positional_first_noncanonical := positional_first_noncanonical state;
     positional_child_error := positional_child_error state;
     positional_deferred_act := positional_deferred_act state;
     positional_association_bad := positional_association_bad state;
     positional_publications := count |}.

Definition positional_publish_transition (state : positional_decode_state)
    : positional_decode_state :=
  if positional_successb state
  then positional_with_publications state 1
  else state.

Theorem actual_publication_transition_is_exactly_once :
  forall state,
    positional_success state ->
    positional_publications (positional_publish_transition state) = 1 /\
    positional_publish_transition (positional_publish_transition state) =
      positional_publish_transition state.
Proof.
  intros state success. unfold positional_success in success.
  assert (first : positional_publish_transition state =
      positional_with_publications state 1).
  { unfold positional_publish_transition. now rewrite success. }
  rewrite first. split; [reflexivity |].
  assert (notAgain : positional_successb
      (positional_with_publications state 1) = false).
  { unfold positional_successb, positional_with_publications. simpl.
    repeat rewrite andb_false_r. reflexivity. }
  unfold positional_publish_transition at 1. rewrite notAgain. reflexivity.
Qed.

Theorem topology_only_map_needs_no_body_slot :
  forall topology parent,
    corrected_value_endpoints topology = [] ->
    positional_remaining_endpoints
      (positional_initial_map topology [] parent true) = [] /\
    positional_remaining_packets
      (positional_initial_map topology [] parent true) = [].
Proof. intros. simpl. now rewrite H. Qed.

Example corrected_root_value_and_value_free_leaf_are_distinct :
  corrected_topology_valid CorrectedMap
    {| corrected_terminals := [[]; [1]];
       corrected_value_endpoints := [[]] |}.
Proof.
  simpl. split; [discriminate |]. split.
  - repeat constructor; simpl; intuition congruence.
  - split.
    + repeat constructor; simpl; intuition congruence.
    + split; intros.
      * destruct H as [same | impossible]; [subst; simpl; auto | contradiction].
      * simpl. auto.
Qed.

Example corrected_topology_only_map_has_no_root_endpoint :
  corrected_topology_valid CorrectedMap
    {| corrected_terminals := [[1]];
       corrected_value_endpoints := [] |}.
Proof.
  simpl. split; [discriminate |]. split.
  - repeat constructor; simpl; intuition congruence.
  - split.
    + constructor.
    + split; simpl; intros; intuition congruence.
Qed.

Example corrected_neutral_empty_is_publishable_without_act_or_bodies :
  corrected_topology_valid CorrectedNeutral
    {| corrected_terminals := []; corrected_value_endpoints := [] |}.
Proof. simpl. auto. Qed.

Example corrected_typed_empty_set_is_rejected :
  ~ corrected_topology_valid CorrectedSet
    {| corrected_terminals := []; corrected_value_endpoints := [] |}.
Proof. simpl. tauto. Qed.

Definition corrected_act_bindings (topology : corrected_topology)
    : list (path * nat) :=
  map (fun endpoint => (endpoint, 0))
    (corrected_value_endpoints topology).

Theorem corrected_act_is_all_zero_and_keeps_lex_endpoint_order :
  forall topology,
    map fst (corrected_act_bindings topology) =
      corrected_value_endpoints topology /\
    map snd (corrected_act_bindings topology) =
      repeat 0 (length (corrected_value_endpoints topology)).
Proof.
  intros topology. unfold corrected_act_bindings.
  induction (corrected_value_endpoints topology) as [|endpoint rest IH]; simpl.
  - auto.
  - destruct IH as [paths zeros]. now rewrite paths, zeros.
Qed.

(** [value_count] is evaluated with executable machines rather than a policy
    proposition.  Both principled machines scan every body extent before a
    child PDA runs.  The retained format additionally validates its declared
    count during framing.  A naive ACT-first removal is shown observably wrong
    because it can let malformed ACT suppress an earlier child error. *)

Fixpoint first_child_failure_from (ordinal : nat) (packets : list body_packet)
    : option nat :=
  match packets with
  | [] => None
  | packet :: rest =>
      match packet_decoded packet with
      | None => Some ordinal
      | Some _ => first_child_failure_from (S ordinal) rest
      end
  end.

Fixpoint first_noncanonical_from (ordinal : nat) (packets : list body_packet)
    : option nat :=
  match packets with
  | [] => None
  | packet :: rest =>
      if packet_canonical packet then first_noncanonical_from (S ordinal) rest
      else Some ordinal
  end.

Definition after_children_result (act_valid : bool) (endpoint_count : nat)
    (packets : list body_packet) : option decode_error :=
  if negb act_valid then Some DeferredActFailure
  else if negb (Nat.eqb endpoint_count (length packets))
       then Some ModeOrOrdinalFailure
       else match first_noncanonical_from 0 packets with
            | Some ordinal => Some (NoncanonicalBodyFailure ordinal)
            | None => None
            end.

Definition retained_count_machine (framing_fields_ok trailing_absent : bool)
    (declared_count endpoint_count : nat) (act_valid : bool)
    (packets : list body_packet) : option decode_error :=
  if negb (framing_fields_ok && trailing_absent &&
      Nat.eqb declared_count (length packets)) then Some FramingFailure
  else match first_child_failure_from 0 packets with
       | Some ordinal => Some (ChildPdaFailure ordinal)
       | None => after_children_result act_valid endpoint_count packets
       end.

Definition scanned_count_free_machine (framing_fields_ok trailing_absent : bool)
    (endpoint_count : nat) (act_valid : bool) (packets : list body_packet)
    : option decode_error :=
  if negb (framing_fields_ok && trailing_absent) then Some FramingFailure
  else match first_child_failure_from 0 packets with
       | Some ordinal => Some (ChildPdaFailure ordinal)
       | None => after_children_result act_valid endpoint_count packets
       end.

Definition naive_act_first_count_free_machine (act_valid : bool)
    (endpoint_count : nat) (packets : list body_packet) : option decode_error :=
  if negb act_valid then Some DeferredActFailure
  else match first_child_failure_from 0 packets with
       | Some ordinal => Some (ChildPdaFailure ordinal)
       | None => after_children_result act_valid endpoint_count packets
       end.

Theorem retained_and_scanned_count_free_agree_when_declared_count_is_exact :
  forall fields trailing endpointCount actValid packets,
    retained_count_machine fields trailing (length packets) endpointCount
      actValid packets =
    scanned_count_free_machine fields trailing endpointCount actValid packets.
Proof.
  intros. unfold retained_count_machine, scanned_count_free_machine.
  rewrite Nat.eqb_refl. destruct fields, trailing; reflexivity.
Qed.

Example act_first_count_removal_breaks_child_before_act_precedence :
  let malformedChild :=
    {| packet_wire_bytes := 1; packet_decoded := None;
       packet_canonical := true |} in
  retained_count_machine true true 1 0 false [malformedChild] =
    Some (ChildPdaFailure 0) /\
  scanned_count_free_machine true true 0 false [malformedChild] =
    Some (ChildPdaFailure 0) /\
  naive_act_first_count_free_machine false 0 [malformedChild] =
    Some DeferredActFailure.
Proof. simpl. auto. Qed.

Example retained_count_rejects_a_mismatched_hint_during_framing :
  let malformedChild :=
    {| packet_wire_bytes := 1; packet_decoded := None;
       packet_canonical := true |} in
  retained_count_machine true true 2 1 true [malformedChild] =
    Some FramingFailure.
Proof. reflexivity. Qed.

Record entry_trie_count_certificate : Type := {
  entry_trie_maintained_len : nat;
  entry_trie_value_paths : list path;
  entry_trie_len_exact : entry_trie_maintained_len =
    length entry_trie_value_paths
}.

Inductive retained_count_route : Type :=
| CountFromEntryTrieMaintainedLen
| CountFromPlannedBorrowedCursorPass
| CountFromPublicRawValueZipperPass.

Definition retained_count_work (route : retained_count_route)
    (logical_values : nat) : nat :=
  match route with
  | CountFromEntryTrieMaintainedLen => 1
  | CountFromPlannedBorrowedCursorPass => logical_values
  | CountFromPublicRawValueZipperPass => logical_values
  end.

Theorem maintained_entry_trie_count_is_exact_without_mapper_callbacks :
  forall certificate,
    entry_trie_maintained_len certificate =
      length (entry_trie_value_paths certificate) /\
    retained_count_work CountFromEntryTrieMaintainedLen
      (entry_trie_maintained_len certificate) = 1.
Proof. intros. split; [apply entry_trie_len_exact | reflexivity]. Qed.

Record planned_body_metadata : Type := {
  planned_encoded_length : nat;
  planned_is_zero : bool
}.

Fixpoint planned_total_encoded_length (values : list planned_body_metadata)
    : nat :=
  match values with
  | [] => 0
  | value :: rest =>
      planned_encoded_length value + planned_total_encoded_length rest
  end.

Fixpoint planned_zero_count (values : list planned_body_metadata) : nat :=
  match values with
  | [] => 0
  | value :: rest =>
      (if planned_is_zero value then 1 else 0) + planned_zero_count rest
  end.

Record planned_cursor_state : Type := {
  planned_cursor_source_len : nat;
  planned_cursor_index : nat;
  planned_cursor_has_pending : bool
}.

Definition planned_cursor_yield (cursor : planned_cursor_state)
    : planned_cursor_state :=
  if negb (planned_cursor_has_pending cursor) &&
      Nat.ltb (planned_cursor_index cursor) (planned_cursor_source_len cursor)
  then {| planned_cursor_source_len := planned_cursor_source_len cursor;
          planned_cursor_index := S (planned_cursor_index cursor);
          planned_cursor_has_pending := true |}
  else cursor.

Definition planned_cursor_owned_occurrences (cursor : planned_cursor_state) : nat :=
  if planned_cursor_has_pending cursor then 1 else 0.

Theorem planned_cursor_retains_at_most_one_occurrence :
  forall cursor,
    planned_cursor_owned_occurrences (planned_cursor_yield cursor) <= 1.
Proof.
  intros [sourceLen index pending]. unfold planned_cursor_yield,
    planned_cursor_owned_occurrences. simpl. destruct pending; simpl; auto.
  destruct (Nat.ltb index sourceLen); simpl; auto.
Qed.

Definition planned_repeatable_pass_work (values : list planned_body_metadata)
    : nat := 3 * length values.

Definition planned_pausable_cursor_private_cells (active_depth : nat) : nat :=
  active_depth + 1.

Theorem three_borrowed_cursor_passes_have_linear_work_and_constant_occurrence_state :
  forall values,
    planned_repeatable_pass_work values = 3 * length values /\
    planned_cursor_owned_occurrences
      {| planned_cursor_source_len := length values;
         planned_cursor_index := 0;
         planned_cursor_has_pending := false |} = 0.
Proof. intros. auto. Qed.

(** Non-authoritative release decision evidence.  Git history establishes
    branch visibility, not absence of persisted/deployed snapshots. *)
Inductive deployment_evidence : Type :=
| DeploymentUnknown
| NoPersistedOrDeployedEpm1Found
| PersistedOrDeployedEpm1Found.

Inductive compatibility_policy : Type :=
| PreserveUnknownLegacyImages
| BreakingUnknownLegacyImagesAuthorized.

Record format_history_manifest : Type := {
  epm1_absent_from_dev : bool;
  epm1_absent_from_origin_dev : bool;
  introducing_commit_has_release_tag : bool;
  observed_deployment_evidence : deployment_evidence;
  campaign_compatibility_policy : compatibility_policy
}.

Definition observed_campaign_history : format_history_manifest :=
  {| epm1_absent_from_dev := true;
     epm1_absent_from_origin_dev := true;
     introducing_commit_has_release_tag := false;
     observed_deployment_evidence := DeploymentUnknown;
     campaign_compatibility_policy := BreakingUnknownLegacyImagesAuthorized |}.

Inductive release_encoding_strategy : Type :=
| AmendUnreleasedEpm1
| AddDualVersionEpm2.

Definition release_strategy_recommendation
    (manifest : format_history_manifest) : option release_encoding_strategy :=
  match observed_deployment_evidence manifest,
        campaign_compatibility_policy manifest with
  | PersistedOrDeployedEpm1Found, _ => Some AddDualVersionEpm2
  | NoPersistedOrDeployedEpm1Found, _ => Some AmendUnreleasedEpm1
  | DeploymentUnknown, PreserveUnknownLegacyImages => None
  | DeploymentUnknown, BreakingUnknownLegacyImagesAuthorized =>
      Some AmendUnreleasedEpm1
  end.

Theorem campaign_policy_authorizes_amending_unreleased_epm1 :
  release_strategy_recommendation observed_campaign_history =
    Some AmendUnreleasedEpm1.
Proof. reflexivity. Qed.

(** ** Streaming ordered lookup and byte-native body admission

    Canonical path order is represented by a checked monotone rank supplied
    by the admitted ACT/source lockstep cursor.  The executable merge owns
    only the two current cursor heads.  It never collects all canonical keys
    and never performs requested-key by all-key nested scans. *)

Record ranked_path : Type := {
  ranked_order : nat;
  ranked_bytes : path
}.

Record ordered_lookup_result : Type := {
  lookup_answers : list (path * option nat);
  lookup_comparisons : nat
}.

Fixpoint ordered_merge_fuel (fuel : nat) (requests endpoints : list ranked_path)
    (ordinal : nat) : ordered_lookup_result :=
  match fuel with
  | 0 => {| lookup_answers := []; lookup_comparisons := 0 |}
  | S rest =>
      match requests, endpoints with
      | [], _ => {| lookup_answers := []; lookup_comparisons := 0 |}
      | request :: requestRest, [] =>
          let tail := ordered_merge_fuel rest requestRest [] ordinal in
          {| lookup_answers :=
               (ranked_bytes request, None) :: lookup_answers tail;
             lookup_comparisons := S (lookup_comparisons tail) |}
      | request :: requestRest, endpoint :: endpointRest =>
          if Nat.ltb (ranked_order request) (ranked_order endpoint) then
            let tail := ordered_merge_fuel rest requestRest endpoints ordinal in
            {| lookup_answers :=
                 (ranked_bytes request, None) :: lookup_answers tail;
               lookup_comparisons := S (lookup_comparisons tail) |}
          else if Nat.eqb (ranked_order request) (ranked_order endpoint) then
            let tail := ordered_merge_fuel rest requestRest endpointRest
              (S ordinal) in
            {| lookup_answers :=
                 (ranked_bytes request, Some ordinal) :: lookup_answers tail;
               lookup_comparisons := S (lookup_comparisons tail) |}
          else
            let tail := ordered_merge_fuel rest requests endpointRest
              (S ordinal) in
            {| lookup_answers := lookup_answers tail;
               lookup_comparisons := S (lookup_comparisons tail) |}
      end
  end.

Definition ordered_merge_lookup (requests endpoints : list ranked_path)
    : ordered_lookup_result :=
  ordered_merge_fuel (length requests + length endpoints) requests endpoints 0.

Lemma ordered_merge_comparison_bound :
  forall fuel requests endpoints ordinal,
    lookup_comparisons (ordered_merge_fuel fuel requests endpoints ordinal) <= fuel.
Proof.
  induction fuel as [|fuel IH]; intros requests endpoints ordinal; simpl; [lia |].
  destruct requests as [|request requestRest]; simpl; [lia |].
  destruct endpoints as [|endpoint endpointRest]; simpl.
  - specialize (IH requestRest [] ordinal). lia.
  - destruct (Nat.ltb (ranked_order request) (ranked_order endpoint)); simpl.
    + specialize (IH requestRest (endpoint :: endpointRest) ordinal). lia.
    + destruct (Nat.eqb (ranked_order request) (ranked_order endpoint)); simpl.
      * specialize (IH requestRest endpointRest (S ordinal)). lia.
      * specialize (IH (request :: requestRest) endpointRest (S ordinal)). lia.
Qed.

Theorem ordered_merge_lookup_is_linear_in_requests_plus_endpoints :
  forall requests endpoints,
    lookup_comparisons (ordered_merge_lookup requests endpoints) <=
      length requests + length endpoints.
Proof. intros. apply ordered_merge_comparison_bound. Qed.

Example ordered_merge_reports_present_and_absent_without_full_key_collection :
  let requests :=
    [{| ranked_order := 1; ranked_bytes := [1] |};
     {| ranked_order := 3; ranked_bytes := [3] |}] in
  let endpoints :=
    [{| ranked_order := 1; ranked_bytes := [1] |};
     {| ranked_order := 2; ranked_bytes := [2] |}] in
  lookup_answers (ordered_merge_lookup requests endpoints) =
    [([1], Some 0); ([3], None)].
Proof. reflexivity. Qed.

Definition streaming_lookup_private_cells (act_depth : nat) : nat :=
  act_depth + 2.

Definition nested_key_scan_comparisons (requested endpoint_count : nat) : nat :=
  requested * endpoint_count.

Example nested_scan_is_asymptotically_distinct_from_merge :
  nested_key_scan_comparisons 16 16 = 256 /\
  16 + 16 = 32.
Proof. auto. Qed.

Inductive canonical_body_admission_strategy : Type :=
| ReencodeAndCompareBody
| ValidateStrictProtobufBytes.

Definition body_admission_allocation_count
    (strategy : canonical_body_admission_strategy) (body_count : nat) : nat :=
  match strategy with
  | ReencodeAndCompareBody => body_count
  | ValidateStrictProtobufBytes => 0
  end.

Definition body_admission_pass_bytes (preflight decode : nat) : nat :=
  preflight + decode.

Theorem strict_byte_native_admission_has_no_reserialization_allocation :
  forall bodyCount,
    body_admission_allocation_count ValidateStrictProtobufBytes bodyCount = 0.
Proof. reflexivity. Qed.

Theorem retained_preflight_and_child_decode_are_linear_in_bytes :
  forall preflightBytes decodeBytes,
    body_admission_pass_bytes preflightBytes decodeBytes =
      preflightBytes + decodeBytes.
Proof. reflexivity. Qed.

(** ** Honest cost semantics *)

Record work_cost : Type := {
  cost_framing_bytes : nat;
  cost_act_parser_bytes : nat;
  cost_logical_events : nat;
  cost_path_bytes : nat;
  cost_body_packets : nat;
  cost_protobuf_bytes : nat;
  cost_allocations : nat;
  cost_pathmap_operations : nat;
  cost_plan_records : nat;
  cost_peak_depth : nat
}.

Definition observed_cost (snapshot_bytes arena_bytes event_count allocation_count
    plan_records peak_depth : nat) (stream : stream_state) : work_cost :=
  {| cost_framing_bytes := snapshot_bytes;
     cost_act_parser_bytes := arena_bytes;
     cost_logical_events := event_count;
     cost_path_bytes := stream_path_bytes stream;
     cost_body_packets := stream_packets_consumed stream;
     cost_protobuf_bytes := stream_protobuf_bytes stream;
     cost_allocations := allocation_count;
     cost_pathmap_operations := stream_pathmap_writes stream;
     cost_plan_records := plan_records;
     cost_peak_depth := peak_depth |}.

Definition honest_plan_memory_units (plan_records peak_depth : nat) : nat :=
  plan_records + peak_depth.

Theorem current_validated_plan_memory_is_nodes_plus_depth :
  forall planRecords peakDepth,
    honest_plan_memory_units planRecords peakDepth = planRecords + peakDepth.
Proof. reflexivity. Qed.

Theorem observed_cost_counts_every_named_work_class :
  forall snapshotBytes arenaBytes events allocations plan depth stream,
    let cost := observed_cost snapshotBytes arenaBytes events allocations plan depth stream in
    cost_framing_bytes cost = snapshotBytes /\
    cost_act_parser_bytes cost = arenaBytes /\
    cost_logical_events cost = events /\
    cost_path_bytes cost = stream_path_bytes stream /\
    cost_body_packets cost = stream_packets_consumed stream /\
    cost_protobuf_bytes cost = stream_protobuf_bytes stream /\
    cost_allocations cost = allocations /\
    cost_pathmap_operations cost = stream_pathmap_writes stream /\
    cost_plan_records cost = plan /\ cost_peak_depth cost = depth.
Proof. intros. repeat split; reflexivity. Qed.

Print Assumptions value_free_root_is_not_an_endpoint.
Print Assumptions topology_only_maps_are_valid_when_productive_and_canonical.
Print Assumptions canonical_forest_labels_strongly_sorted.
Print Assumptions canonical_forest_labels_nodup.
Print Assumptions typed_value_free_root_is_rejected.
Print Assumptions valid_event_parser_complete.
Print Assumptions valid_event_parser_sound.
Print Assumptions valid_event_parser_unique.
Print Assumptions valid_program_has_exact_root_close.
Print Assumptions event_after_root_leave_is_rejected.
Print Assumptions neutral_virtual_walk_is_not_a_typed_empty_act.
Print Assumptions accepted_actree03_checked_varints.
Print Assumptions accepted_actree03_offsets_reachability_and_producer_order.
Print Assumptions accepted_actree03_exact_emitted_events.
Print Assumptions root_set_val_has_complete_extensional_effect.
Print Assumptions set_val_preserves_unrelated_values.
Print Assumptions ascent_boolean_and_focus_are_exact.
Print Assumptions successful_stream_step_associates_exact_head_ordinal.
Print Assumptions stream_step_consumes_exactly_one_packet.
Print Assumptions mismatched_binding_is_latched_without_suppressing_body_consumption.
Print Assumptions unreferenced_body_is_latched_without_suppressing_consumption.
Print Assumptions child_failure_stops_before_later_packets.
Print Assumptions no_child_failure_consumes_complete_framed_stream.
Print Assumptions direct_body_child_failure_is_first_in_wire_order.
Print Assumptions direct_body_complete_consumption.
Print Assumptions direct_oracle_child_outranks_deferred_structure.
Print Assumptions direct_oracle_structure_outranks_noncanonical.
Print Assumptions every_failure_leaves_parent_unchanged.
Print Assumptions publication_is_exactly_once_and_consumes_source.
Print Assumptions publication_requires_bound_immutable_source_identity.
Print Assumptions generated_finish_refines_extensional_publication.
Print Assumptions child_error_outranks_deferred_act.
Print Assumptions deferred_act_outranks_noncanonical.
Print Assumptions ordinal_association_failure_outranks_noncanonical.
Print Assumptions framing_failure_prevents_child_or_act_selection.
Print Assumptions accepted_framing_scanned_every_body_extent.
Print Assumptions occurrence_ordinals_ignore_storage_allocation.
Print Assumptions two_logical_occurrences_may_share_one_storage_identity.
Print Assumptions ordinal_catamorphism_aligns_paths_and_ordinals.
Print Assumptions compressed_leaf_mapper_reentry_is_idempotent.
Print Assumptions ordinal_assignment_is_storage_identity_independent.
Print Assumptions shared_value_address_has_distinct_logical_ordinals.
Print Assumptions cached_cata_delegates_to_occurrence_traversal.
Print Assumptions checked_assign_ordinal_range_and_advance.
Print Assumptions duplicate_mapper_calls_do_not_duplicate_logical_occurrences.
Print Assumptions act_ordinal_and_body_pass_are_byte_aligned.
Print Assumptions ordinal_cata_requires_depth_frames_not_node_rank_storage.
Print Assumptions valued_arrival_assigns_once_and_advances.
Print Assumptions value_free_arrival_does_not_consume_an_ordinal.
Print Assumptions reset_handles_root_without_nonempty_path_assumption.
Print Assumptions ascent_preserves_counter_and_emission.
Print Assumptions witness_mapper_reentry_is_stable_across_reads.
Print Assumptions every_delegated_move_preserves_the_underlying_destination.
Print Assumptions synthetic_ordinal_zipper_has_depth_only_private_state.
Print Assumptions current_only_witness_is_stale_after_first_ascent.
Print Assumptions full_stack_witness_tracks_multi_value_unary_ascent.
Print Assumptions full_stack_witness_handles_value_free_structural_stop.
Print Assumptions dense_prefix_branching_witness_copy_is_sloped.
Print Assumptions witness_snapshot_cost_is_sum_of_active_depths.
Print Assumptions no_candidate_is_unconditionally_authorized_by_v2.
Print Assumptions no_bare_key_strictly_prefixes_a_bare_key.
Print Assumptions no_split_key_strictly_prefixes_any_canonical_key.
Print Assumptions canonical_key_strict_prefix_chain_has_at_most_two_values.
Print Assumptions public_raw_pathmap_cannot_use_the_two_frame_bound_without_validation.
Print Assumptions zero_sentinel_preserves_implicit_lex_body_cardinality.
Print Assumptions postorder_ordinals_can_break_lex_streaming_alignment.
Print Assumptions independent_source_and_act_lex_walks_are_lockstep.
Print Assumptions corrected_logical_act_parser_is_bound_to_canonical_producer.
Print Assumptions corrected_act_and_source_body_cursor_exhaust_together.
Print Assumptions replay_topology_extensional_membership.
Print Assumptions replay_topology_has_no_duplicate_terminals.
Print Assumptions replay_set_from_empty_is_exact.
Print Assumptions replay_set_has_no_duplicate_value_keys.
Print Assumptions successful_yield_has_exactly_one_pending_endpoint.
Print Assumptions pushed_endpoint_body_is_paired_once_at_the_owned_focus.
Print Assumptions root_body_uses_owned_zipper_root_set_val.
Print Assumptions decoded_body_never_mutates_without_an_endpoint_target.
Print Assumptions positional_error_precedence_is_child_then_act_then_association_then_body.
Print Assumptions too_few_bodies_leave_a_dangling_endpoint.
Print Assumptions positional_round_consumes_one_decoded_packet.
Print Assumptions one_extra_body_is_consumed_then_rejected.
Print Assumptions positional_failure_never_publishes_private_map.
Print Assumptions positional_success_returns_one_publication_result.
Print Assumptions actual_publication_transition_is_exactly_once.
Print Assumptions admitted_map_happy_run_refines_independent_lockstep.
Print Assumptions admitted_topology_cannot_be_an_unchecked_unsorted_or_duplicate_list.
Print Assumptions corrected_act_is_all_zero_and_keeps_lex_endpoint_order.
Print Assumptions retained_and_scanned_count_free_agree_when_declared_count_is_exact.
Print Assumptions act_first_count_removal_breaks_child_before_act_precedence.
Print Assumptions maintained_entry_trie_count_is_exact_without_mapper_callbacks.
Print Assumptions three_borrowed_cursor_passes_have_linear_work_and_constant_occurrence_state.
Print Assumptions ordered_merge_lookup_is_linear_in_requests_plus_endpoints.
Print Assumptions ordered_merge_reports_present_and_absent_without_full_key_collection.
Print Assumptions strict_byte_native_admission_has_no_reserialization_allocation.
Print Assumptions retained_preflight_and_child_decode_are_linear_in_bytes.
Print Assumptions current_validated_plan_memory_is_nodes_plus_depth.
Print Assumptions observed_cost_counts_every_named_work_class.
