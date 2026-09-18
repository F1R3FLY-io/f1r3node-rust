From Stdlib Require Import List Arith Bool Lia.
Require Import Epm1CursorAuthorityV2 Epm1CursorPositiveV3 Epm1CursorPositiveV4
  Epm1ActConcreteV5 Epm1PositionalAssociationV6.

Import ListNotations.
Import PositiveV3 PositiveV4 ActConcreteV5 PositionalAssociationV6.
Module ValueSummary.

Record summary := Summary {
  value_count : nat;
  first_nonzero : option (nat * nat);
  first_nonsequential : option (nat * nat * nat)
}.
Definition empty_summary := Summary 0 None None.
Definition first_or {A} (prior candidate : option A) : option A :=
  match prior with Some _ => prior | None => candidate end.
Definition diagnostic_offset node :=
  match dsm_wrapper node with
  | Some (wrapper, _) => wrapper
  | None => dsm_id node
  end.
Definition observe state node :=
  match dsm_value node with
  | None => state
  | Some value =>
      Summary (S (value_count state))
        (first_or (first_nonzero state)
          (if value =? 0 then None else Some (diagnostic_offset node, value)))
        (first_or (first_nonsequential state)
          (if value =? value_count state then None
           else Some (diagnostic_offset node, value, value_count state)))
  end.
Definition summarize nodes := fold_left observe nodes empty_summary.

Fixpoint bindings nodes : list (nat * nat) :=
  match nodes with
  | [] => []
  | node :: rest =>
      match dsm_value node with
      | None => bindings rest
      | Some value => (diagnostic_offset node, value) :: bindings rest
      end
  end.
Definition nonzero_reference entries :=
  find (fun entry : nat * nat => negb (snd entry =? 0)) entries.
Fixpoint nonsequential_reference ordinal entries : option (nat * nat * nat) :=
  match entries with
  | [] => None
  | (offset, value) :: rest =>
      if value =? ordinal then nonsequential_reference (S ordinal) rest
      else Some (offset, value, ordinal)
  end.

Theorem accumulated_count_equals_valued_observation_count : forall nodes state,
  value_count (fold_left observe nodes state) =
    value_count state + length (bindings nodes).
Proof.
  induction nodes as [|node rest IH]; intro state; cbn [fold_left bindings length].
  - lia.
  - rewrite IH. destruct (dsm_value node) eqn:Value;
      unfold observe; rewrite Value; cbn; lia.
Qed.

Theorem first_nonzero_matches_independent_find : forall nodes state,
  first_nonzero (fold_left observe nodes state) =
    first_or (first_nonzero state) (nonzero_reference (bindings nodes)).
Proof.
  induction nodes as [|node rest IH]; intro state; cbn [fold_left bindings].
  - destruct (first_nonzero state); reflexivity.
  - rewrite IH. destruct (dsm_value node) as [value|] eqn:Value;
      unfold observe; rewrite Value; cbn [first_nonzero].
    + unfold nonzero_reference; cbn [find snd].
      destruct (value =? 0); destruct (first_nonzero state); reflexivity.
    + reflexivity.
Qed.

Theorem first_nonsequential_matches_value_ordinal_reference : forall nodes state,
  first_nonsequential (fold_left observe nodes state) =
    first_or (first_nonsequential state)
      (nonsequential_reference (value_count state) (bindings nodes)).
Proof.
  induction nodes as [|node rest IH]; intro state; cbn [fold_left bindings].
  - destruct (first_nonsequential state); reflexivity.
  - rewrite IH. destruct (dsm_value node) as [value|] eqn:Value;
      unfold observe; rewrite Value; cbn [first_nonsequential value_count].
    + cbn [nonsequential_reference].
      destruct (value =? value_count state);
        destruct (first_nonsequential state); reflexivity.
    + reflexivity.
Qed.

Theorem concrete_summary_refines_independent_inventory : forall nodes,
  summarize nodes = Summary (length (bindings nodes))
    (nonzero_reference (bindings nodes))
    (nonsequential_reference 0 (bindings nodes)).
Proof.
  intro nodes.
  pose proof (accumulated_count_equals_valued_observation_count nodes empty_summary) as Count.
  pose proof (first_nonzero_matches_independent_find nodes empty_summary) as Nonzero.
  pose proof (first_nonsequential_matches_value_ordinal_reference nodes empty_summary) as Nonseq.
  cbn [empty_summary value_count first_nonzero first_nonsequential first_or Nat.add] in *.
  unfold summarize. destruct (fold_left observe nodes empty_summary).
  cbn in Count, Nonzero, Nonseq. subst. reflexivity.
Qed.

Theorem latched_nonzero_diagnostic_is_retained : forall nodes state diagnostic,
  first_nonzero state = Some diagnostic ->
  first_nonzero (fold_left observe nodes state) = Some diagnostic.
Proof. intros. rewrite first_nonzero_matches_independent_find, H; reflexivity. Qed.

Theorem latched_nonsequential_diagnostic_is_retained : forall nodes state diagnostic,
  first_nonsequential state = Some diagnostic ->
  first_nonsequential (fold_left observe nodes state) = Some diagnostic.
Proof. intros. rewrite first_nonsequential_matches_value_ordinal_reference, H; reflexivity. Qed.

Theorem absent_nonzero_is_exactly_all_zero_markers : forall entries,
  nonzero_reference entries = None <-> Forall (fun entry => snd entry = 0) entries.
Proof.
  induction entries as [|[offset value] rest IH].
  - split; intro; constructor.
  - unfold nonzero_reference in *; cbn [find snd].
    destruct (value =? 0) eqn:Zero; cbn.
    + apply Nat.eqb_eq in Zero.
      rewrite IH. split; intro H; [constructor; auto|inversion H; assumption].
    + apply Nat.eqb_neq in Zero.
      split; intro H; [discriminate|inversion H; contradiction].
Qed.

Theorem value_inventory_is_node_bounded : forall nodes,
  length (bindings nodes) <= length nodes.
Proof.
  induction nodes as [|node rest IH]; cbn [bindings length].
  - lia.
  - destruct (dsm_value node); cbn; lia.
Qed.

Lemma bindings_append : forall left right,
  bindings (left ++ right) = bindings left ++ bindings right.
Proof.
  induction left as [|node rest IH]; intro right; cbn [bindings app]; auto.
  destruct (dsm_value node); cbn; now rewrite IH.
Qed.

Theorem semantic_preorder_markers_match_writer_bindings :
  (forall tree prefix,
    map snd (bindings (map source_semantic_erase (source_semantic_preorder tree))) =
    map snd (writer_value_bindings_tree_at prefix (source_semantic_reify tree))) /\
  (forall forest prefix,
    map snd (bindings (map source_semantic_erase (source_semantic_forest_preorder forest))) =
    map snd (writer_value_bindings_forest_at prefix (source_semantic_forest_reify forest))).
Proof.
  apply source_semantic_tree_forest_ind.
  - intros id len root line value children IH line_id line_hash wrapper prefix.
    cbn [source_semantic_preorder map bindings source_semantic_erase dsm_value
      source_semantic_reify writer_value_bindings_tree_at].
    destruct value; cbn [map app snd]; rewrite (IH (prefix ++ line)); reflexivity.
  - intro prefix; reflexivity.
  - intros label child Child rest Rest prefix.
    cbn [source_semantic_forest_preorder source_semantic_forest_reify
      writer_value_bindings_forest_at].
    rewrite map_app, bindings_append, !map_app,
      (Child (prefix ++ [label])), (Rest prefix); reflexivity.
Qed.

Theorem certified_summary_markers_are_source_writer_markers :
  forall line_hash tree bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    map snd (bindings (source_semantic_erased_preorder certificate)) =
      map snd (writer_value_bindings tree).
Proof.
  intros lineHash tree bytes certificate objects Encoded.
  pose proof (proj2 (semantic_certificate_erases_and_reifies_exactly
    lineHash tree bytes certificate objects Encoded)) as Reified.
  unfold source_semantic_erased_preorder, writer_value_bindings.
  rewrite (proj1 semantic_preorder_markers_match_writer_bindings certificate []), Reified.
  reflexivity.
Qed.

Theorem certified_summary_count_fits_resident_image :
  forall line_hash tree bytes certificate objects limit,
    encode_writer_tree_with_semantic_certificate line_hash tree =
      Some (bytes, certificate, objects) ->
    length bytes <= limit ->
    value_count (summarize (source_semantic_erased_preorder certificate)) <= limit.
Proof.
  intros lineHash tree bytes certificate objects limit Encoded Limit.
  rewrite concrete_summary_refines_independent_inventory; cbn [value_count].
  pose proof (value_inventory_is_node_bounded (source_semantic_erased_preorder certificate))
    as Count.
  unfold source_semantic_erased_preorder in Count; rewrite length_map in Count.
  rewrite (proj1 source_semantic_preorder_length certificate) in Count.
  pose proof (proj1 (semantic_certificate_node_count_is_byte_bounded
    lineHash tree bytes certificate objects Encoded)) as Bound.
  unfold source_semantic_erased_preorder. lia.
Qed.

Theorem canonical_zero_writer_summary_has_exact_count_and_zero_markers :
  forall line_hash source bytes certificate objects,
    encode_writer_tree_with_semantic_certificate line_hash
      (canonical_zero_writer_tree source) = Some (bytes, certificate, objects) ->
    value_count (summarize (source_semantic_erased_preorder certificate)) =
      length (source_body_cursor_paths source) /\
    first_nonzero (summarize (source_semantic_erased_preorder certificate)) = None.
Proof.
  intros lineHash source bytes certificate objects Encoded.
  pose proof (certified_summary_markers_are_source_writer_markers
    lineHash _ bytes certificate objects Encoded) as Markers.
  rewrite (proj2 (canonical_zero_writer_has_one_zero_sentinel_per_logical_endpoint source))
    in Markers.
  rewrite concrete_summary_refines_independent_inventory; cbn [value_count first_nonzero].
  split.
  - pose proof (f_equal (@length nat) Markers) as Count.
    rewrite length_map, repeat_length in Count. exact Count.
  - apply absent_nonzero_is_exactly_all_zero_markers.
    rewrite Forall_forall. intros [offset value] Present; cbn.
    assert (In value (map snd (bindings (source_semantic_erased_preorder certificate))))
      as ValuePresent by (apply in_map with (f := @snd nat nat) in Present; exact Present).
    rewrite Markers in ValuePresent. now apply repeat_spec in ValuePresent.
Qed.

Theorem admitted_canonical_image_summary_uses_forward_preorder :
  forall line_hash tree bytes,
    canonical_writer_tree tree ->
    encode_writer_tree line_hash tree = Some bytes ->
    exists certificate objects admitted,
      validate_direct_canonical_image line_hash bytes = Some admitted /\
      encode_writer_tree_with_semantic_certificate line_hash tree =
        Some (bytes, certificate, objects) /\
      summarize (rev (vdi_nodes admitted)) =
        summarize (source_semantic_erased_preorder certificate) /\
      map snd (bindings (rev (vdi_nodes admitted))) =
        map snd (writer_value_bindings tree) /\
      value_count (summarize (rev (vdi_nodes admitted))) <= length bytes.
Proof.
  intros lineHash tree bytes Canonical Encoded.
  destruct (validate_accepts_every_canonical_writer_output lineHash tree bytes Canonical Encoded)
    as [certificate [objects [Certified Accepted]]].
  eexists certificate, objects, _.
  split; [exact Accepted|]. split; [exact Certified|].
  cbn [vdi_nodes]; rewrite rev_involutive. split; [reflexivity|].
  split.
  - now apply (certified_summary_markers_are_source_writer_markers
      lineHash tree bytes certificate objects).
  - eapply certified_summary_count_fits_resident_image; eauto.
Qed.

Definition zero_summary state :=
  match first_nonzero state with None => true | Some _ => false end.

Definition typed_mode_accepts (map_mode logical_empty : bool) declared state :=
  negb logical_empty && (zero_summary state &&
    (if map_mode then value_count state =? declared else declared =? 0)).

Theorem zero_summary_decides_exactly_all_zero_markers : forall nodes,
  zero_summary (summarize nodes) = true <->
    Forall (fun entry => snd entry = 0) (bindings nodes).
Proof.
  intro nodes. rewrite <- absent_nonzero_is_exactly_all_zero_markers.
  rewrite concrete_summary_refines_independent_inventory.
  unfold zero_summary; cbn [first_nonzero].
  destruct (nonzero_reference (bindings nodes)); split; intro H;
    try discriminate; reflexivity.
Qed.

Theorem typed_mode_decision_uses_derived_count_and_zero_markers :
  forall nodes map_mode logical_empty declared,
    typed_mode_accepts map_mode logical_empty declared (summarize nodes) = true <->
    logical_empty = false /\
    Forall (fun entry => snd entry = 0) (bindings nodes) /\
    (if map_mode then length (bindings nodes) = declared else declared = 0).
Proof.
  intros nodes map_mode logical_empty declared. unfold typed_mode_accepts.
  rewrite !andb_true_iff, negb_true_iff, zero_summary_decides_exactly_all_zero_markers.
  rewrite concrete_summary_refines_independent_inventory; cbn [value_count].
  destruct map_mode; rewrite Nat.eqb_eq; reflexivity.
Qed.

Print Assumptions zero_summary_decides_exactly_all_zero_markers.
Print Assumptions typed_mode_decision_uses_derived_count_and_zero_markers.
Print Assumptions accumulated_count_equals_valued_observation_count.
Print Assumptions first_nonzero_matches_independent_find.
Print Assumptions first_nonsequential_matches_value_ordinal_reference.
Print Assumptions concrete_summary_refines_independent_inventory.
Print Assumptions latched_nonzero_diagnostic_is_retained.
Print Assumptions latched_nonsequential_diagnostic_is_retained.
Print Assumptions absent_nonzero_is_exactly_all_zero_markers.
Print Assumptions value_inventory_is_node_bounded.
Print Assumptions bindings_append.
Print Assumptions semantic_preorder_markers_match_writer_bindings.
Print Assumptions certified_summary_markers_are_source_writer_markers.
Print Assumptions certified_summary_count_fits_resident_image.
Print Assumptions canonical_zero_writer_summary_has_exact_count_and_zero_markers.
Print Assumptions admitted_canonical_image_summary_uses_forward_preorder.
End ValueSummary.
