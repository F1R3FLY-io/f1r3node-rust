From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool Sorting.Sorted.
Require Import Epm1CursorAuthorityV2 Epm1CursorPositiveV3
  Epm1CursorPositiveV4 Epm1ActConcreteV5.

Import ListNotations.
Import PositiveV3 PositiveV4 ActConcreteV5.

(**
  Positional EPM1 association authority, revision V6.

  V2 defines the positional body PDA and its owned-zipper publication model;
  V3--V5 prove the physical ACTree03 writer/parser contract.  This file joins
  those developments.  ACT values are occurrence markers, never body indexes:
  every logical value endpoint stores zero, while independently framed bodies
  are consumed in the same lexicographic endpoint order.

  The source-to-writer translation below is proof-only.  It deliberately uses
  structural occurrence identity and has no allocation-address argument.  Its
  physical image is therefore invariant under CoW sharing and repeated equal
  values.  Production admission remains restricted by [canonical_writer_tree]
  and [admitted_corrected_map_input]; no theorem admits arbitrary bytes or an
  unchecked endpoint list.
*)
Module PositionalAssociationV6.

Fixpoint canonical_zero_writer_tree (tree : canonical_tree) : writer_tree :=
  match tree with
  | CNode value children =>
      WriterNode []
        (match value with Some _ => Some 0 | None => None end)
        (canonical_zero_writer_forest children)
  end
with canonical_zero_writer_forest (forest : canonical_forest) : writer_forest :=
  match forest with
  | FNil => WriterNil
  | FCons label child rest =>
      WriterCons label (canonical_zero_writer_tree child)
        (canonical_zero_writer_forest rest)
  end.

Fixpoint writer_value_bindings_tree_at (prefix : path) (tree : writer_tree)
    : list (path * nat) :=
  match tree with
  | WriterNode line value children =>
      let focus := prefix ++ line in
      (match value with Some marker => [(focus, marker)] | None => [] end) ++
      writer_value_bindings_forest_at focus children
  end
with writer_value_bindings_forest_at (prefix : path) (forest : writer_forest)
    : list (path * nat) :=
  match forest with
  | WriterNil => []
  | WriterCons label child rest =>
      writer_value_bindings_tree_at (prefix ++ [label]) child ++
      writer_value_bindings_forest_at prefix rest
  end.

Definition writer_value_bindings (tree : writer_tree) : list (path * nat) :=
  writer_value_bindings_tree_at [] tree.

Definition zero_binding (endpoint : path) : path * nat := (endpoint, 0).

Lemma canonical_zero_writer_binding_lockstep_mutual :
  (forall tree prefix,
      writer_value_bindings_tree_at prefix (canonical_zero_writer_tree tree) =
        map zero_binding (source_body_paths_tree_at prefix tree)) /\
  (forall forest prefix,
      writer_value_bindings_forest_at prefix
        (canonical_zero_writer_forest forest) =
        map zero_binding (source_body_paths_forest_at prefix forest)).
Proof.
  apply canonical_tree_forest_mutind.
  - intros value children childrenIH prefix. cbn.
    rewrite app_nil_r. destruct value; cbn; rewrite childrenIH; reflexivity.
  - intro prefix. reflexivity.
  - intros label child childIH rest restIH prefix. cbn.
    rewrite childIH, restIH, map_app. reflexivity.
Qed.

Theorem canonical_zero_writer_bindings_are_source_lexicographic :
  forall source,
    writer_value_bindings (canonical_zero_writer_tree source) =
      map zero_binding (source_body_cursor_paths source).
Proof.
  intros source. unfold writer_value_bindings, source_body_cursor_paths.
  apply (proj1 canonical_zero_writer_binding_lockstep_mutual).
Qed.

Lemma zero_binding_projections : forall endpoints,
  map fst (map zero_binding endpoints) = endpoints /\
  map snd (map zero_binding endpoints) = repeat 0 (length endpoints).
Proof.
  induction endpoints as [|endpoint rest IH]; cbn.
  - auto.
  - destruct IH as [paths markers]. now rewrite paths, markers.
Qed.

Theorem canonical_zero_writer_has_one_zero_sentinel_per_logical_endpoint :
  forall source,
    map fst (writer_value_bindings (canonical_zero_writer_tree source)) =
      source_body_cursor_paths source /\
    map snd (writer_value_bindings (canonical_zero_writer_tree source)) =
      repeat 0 (length (source_body_cursor_paths source)).
Proof.
  intros source. rewrite canonical_zero_writer_bindings_are_source_lexicographic.
  apply zero_binding_projections.
Qed.

(** An address observation is ghost input to a rejected implementation.  The
    accepted writer view cannot inspect it, so equal values and shared CoW
    allocations retain distinct structural occurrences. *)
Definition address_observation := path -> nat.

Definition address_independent_writer (_ : address_observation)
    (source : canonical_tree) : writer_tree :=
  canonical_zero_writer_tree source.

Theorem canonical_writer_and_image_are_address_independent :
  forall source left_address right_address line_hash,
    address_independent_writer left_address source =
      address_independent_writer right_address source /\
    encode_writer_tree line_hash (address_independent_writer left_address source) =
      encode_writer_tree line_hash (address_independent_writer right_address source).
Proof. intros. split; reflexivity. Qed.

Theorem canonical_zero_writer_is_accepted_by_concrete_act_parser :
  forall line_hash source bytes,
    canonical_writer_tree (canonical_zero_writer_tree source) ->
    encode_writer_tree line_hash (canonical_zero_writer_tree source) = Some bytes ->
    exists certificate objects,
      encode_writer_tree_with_semantic_certificate line_hash
        (canonical_zero_writer_tree source) =
        Some (bytes, certificate, objects) /\
      validate_direct_canonical_image line_hash bytes =
        Some {| vdi_root := source_semantic_root_id certificate;
                vdi_arena_end :=
                  ACT_HEADER_LEN + length (physical_writer_bytes objects);
                vdi_nodes :=
                  rev (source_semantic_erased_preorder certificate) |}.
Proof.
  intros. eapply validate_accepts_every_canonical_writer_output; eassumption.
Qed.

Lemma positional_yield_preserves_publication_count : forall state,
  positional_publications (positional_yield state) =
    positional_publications state.
Proof.
  intros [endpoints packets pending private focus parent ordinal pairs
    noncanonical child deferred association publications].
  unfold positional_yield. cbn.
  destruct pending; [reflexivity|].
  destruct packets; [reflexivity|].
  destruct endpoints; reflexivity.
Qed.

Lemma positional_push_preserves_publication_count : forall state,
  positional_publications (positional_push state) =
    positional_publications state.
Proof.
  intro state. unfold positional_push.
  destruct (positional_pending state); [|reflexivity].
  destruct (positional_remaining_packets state) as [|packet packets];
    [reflexivity|].
  destruct (packet_decoded packet); reflexivity.
Qed.

Lemma positional_run_preserves_publication_count : forall fuel state,
  positional_publications (positional_run fuel state) =
    positional_publications state.
Proof.
  induction fuel as [|fuel IH]; intro state; [reflexivity|].
  cbn. destruct (positional_child_error state); [reflexivity|].
  destruct (positional_remaining_packets state); [reflexivity|].
  rewrite IH. unfold positional_round.
  rewrite positional_push_preserves_publication_count,
    positional_yield_preserves_publication_count. reflexivity.
Qed.

Theorem admitted_positional_run_publishes_exactly_once :
  forall admitted bodies parent,
    length (corrected_value_endpoints (admitted_topology admitted)) =
      length bodies ->
    let initial := positional_initial_admitted_map admitted
      (canonical_packets_of_bodies bodies) parent in
    let after := positional_run (length bodies) initial in
    positional_success after /\
    positional_publish after = (positional_private after, 1) /\
    positional_publications (positional_publish_transition after) = 1 /\
    positional_publish_transition (positional_publish_transition after) =
      positional_publish_transition after.
Proof.
  intros admitted bodies parent sameLength initial after.
  pose proof (admitted_map_happy_run_refines_independent_lockstep
    admitted bodies parent sameLength) as run.
  fold initial in run. fold after in run.
  destruct run as [endpoints [packets [pending [pairs [private
    [child [noncanonical [deferred association]]]]]]]].
  assert (noPublications : positional_publications after = 0).
  { unfold after. rewrite positional_run_preserves_publication_count.
    unfold initial, positional_initial_admitted_map, positional_initial_map.
    reflexivity. }
  assert (success : positional_success after).
  { unfold after, positional_success, positional_successb.
    unfold after in noPublications.
    rewrite endpoints, packets, pending, child, noncanonical, deferred,
      association, noPublications. reflexivity. }
  split; [exact success|].
  split; [apply positional_success_returns_one_publication_result; exact success|].
  apply actual_publication_transition_is_exactly_once. exact success.
Qed.

Definition endpoint_count_certificate (endpoints : list path)
    : entry_trie_count_certificate :=
  {| entry_trie_maintained_len := length endpoints;
     entry_trie_value_paths := endpoints;
     entry_trie_len_exact := eq_refl |}.

Theorem positional_entry_count_and_count_free_machine_agree :
  forall endpoints bodies,
    length endpoints = length bodies ->
    entry_trie_maintained_len (endpoint_count_certificate endpoints) =
      length endpoints /\
    length (canonical_packets_of_bodies bodies) = length bodies /\
    retained_count_machine true true (length bodies) (length endpoints) true
      (canonical_packets_of_bodies bodies) =
    scanned_count_free_machine true true (length endpoints) true
      (canonical_packets_of_bodies bodies).
Proof.
  intros endpoints bodies sameLength. split; [reflexivity|].
  split; [unfold canonical_packets_of_bodies; apply length_map|].
  pose proof (retained_and_scanned_count_free_agree_when_declared_count_is_exact
    true true (length endpoints) true (canonical_packets_of_bodies bodies))
    as countAgreement.
  unfold canonical_packets_of_bodies in countAgreement |- *.
  now rewrite length_map in countAgreement.
Qed.

Definition positional_error_precedence_contract : Prop :=
  (forall trailing declared endpoint_count act_valid packets,
      retained_count_machine false trailing declared endpoint_count act_valid
        packets = Some FramingFailure) /\
  (forall state child_ordinal,
      positional_child_error state = Some child_ordinal ->
      positional_final_error state = Some (ChildPdaFailure child_ordinal)) /\
  (forall state,
      positional_child_error state = None ->
      positional_deferred_act state = true ->
      positional_final_error state = Some DeferredActFailure) /\
  (forall state,
      positional_child_error state = None ->
      positional_deferred_act state = false ->
      positional_association_bad state = true ->
      positional_final_error state = Some ModeOrOrdinalFailure) /\
  (forall state ordinal,
      positional_child_error state = None ->
      positional_deferred_act state = false ->
      positional_association_bad state = false ->
      positional_remaining_endpoints state = [] ->
      positional_first_noncanonical state = Some ordinal ->
      positional_final_error state = Some (NoncanonicalBodyFailure ordinal)).

Theorem positional_error_precedence_is_total :
  positional_error_precedence_contract.
Proof.
  unfold positional_error_precedence_contract. split.
  - intros. reflexivity.
  - split.
    + apply positional_error_precedence_is_child_then_act_then_association_then_body.
    + split.
      * apply positional_deferred_act_outranks_noncanonical.
      * split.
        -- apply positional_association_outranks_noncanonical.
        -- intros. unfold positional_final_error.
           now rewrite H, H0, H1, H2, H3.
Qed.

(** The capstone binds the physical writer/parser, structural occurrence
    identity, source/body lexicographic association, the pausable positional
    PDA, exactly-once publication, maintained/fallback counts, and error
    precedence in one statement. *)
Theorem epm1_positional_act_body_association_capstone :
  forall line_hash source bytes admitted bodies parent,
    admitted_source_tree admitted = source ->
    canonical_writer_tree (canonical_zero_writer_tree source) ->
    encode_writer_tree line_hash (canonical_zero_writer_tree source) = Some bytes ->
    length (source_body_cursor_paths source) = length bodies ->
    exists certificate objects after,
      validate_direct_canonical_image line_hash bytes =
        Some {| vdi_root := source_semantic_root_id certificate;
                vdi_arena_end :=
                  ACT_HEADER_LEN + length (physical_writer_bytes objects);
                vdi_nodes :=
                  rev (source_semantic_erased_preorder certificate) |} /\
      encode_writer_tree_with_semantic_certificate line_hash
        (canonical_zero_writer_tree source) =
        Some (bytes, certificate, objects) /\
      map fst (writer_value_bindings (canonical_zero_writer_tree source)) =
        source_body_cursor_paths source /\
      map snd (writer_value_bindings (canonical_zero_writer_tree source)) =
        repeat 0 (length (source_body_cursor_paths source)) /\
      corrected_value_endpoints (admitted_topology admitted) =
        source_body_cursor_paths source /\
      after = positional_run (length bodies)
        (positional_initial_admitted_map admitted
          (canonical_packets_of_bodies bodies) parent) /\
      rev (positional_pairs after) =
        combine (source_body_cursor_paths source) bodies /\
      positional_publish after = (positional_private after, 1) /\
      positional_publications (positional_publish_transition after) = 1 /\
      positional_publish_transition (positional_publish_transition after) =
        positional_publish_transition after /\
      entry_trie_maintained_len
        (endpoint_count_certificate (source_body_cursor_paths source)) =
        length (source_body_cursor_paths source) /\
      retained_count_machine true true (length bodies)
        (length (source_body_cursor_paths source)) true
        (canonical_packets_of_bodies bodies) =
      scanned_count_free_machine true true
        (length (source_body_cursor_paths source)) true
        (canonical_packets_of_bodies bodies) /\
      (forall left_address right_address,
        encode_writer_tree line_hash
          (address_independent_writer left_address source) =
        encode_writer_tree line_hash
          (address_independent_writer right_address source)) /\
      positional_error_precedence_contract /\
      release_strategy_recommendation observed_campaign_history =
        Some AmendUnreleasedEpm1.
Proof.
  intros lineHash source bytes admitted bodies parent sourceExact
    writerCanonical encoded bodyCount.
  destruct (canonical_zero_writer_is_accepted_by_concrete_act_parser
    lineHash source bytes writerCanonical encoded)
    as [certificate [objects [certified accepted]]].
  assert (endpointExact :
    corrected_value_endpoints (admitted_topology admitted) =
      source_body_cursor_paths source).
  { pose proof (corrected_act_and_source_body_cursor_exhaust_together
      (admitted_source_tree admitted) (admitted_act_events admitted)
      (admitted_topology admitted) (admitted_logical_binding admitted))
      as [same _]. now rewrite sourceExact in same. }
  assert (admittedCount :
    length (corrected_value_endpoints (admitted_topology admitted)) =
      length bodies) by now rewrite endpointExact.
  set (after := positional_run (length bodies)
    (positional_initial_admitted_map admitted
      (canonical_packets_of_bodies bodies) parent)).
  pose proof (admitted_map_happy_run_refines_independent_lockstep
    admitted bodies parent admittedCount) as run.
  fold after in run.
  destruct run as [remainingEndpoints [remainingPackets [pending [pairs
    [private [child [noncanonical [deferred association]]]]]]]].
  pose proof (admitted_positional_run_publishes_exactly_once
    admitted bodies parent admittedCount) as publication.
  fold after in publication.
  destruct publication as [success [published [publishedOnce idempotent]]].
  pose proof (canonical_zero_writer_has_one_zero_sentinel_per_logical_endpoint
    source) as [bindingPaths bindingMarkers].
  pose proof (positional_entry_count_and_count_free_machine_agree
    (source_body_cursor_paths source) bodies bodyCount)
    as [entryCount [packetCount countAgreement]].
  exists certificate, objects, after. repeat split; try assumption.
  - now rewrite endpointExact in pairs.
  - eapply positional_error_precedence_is_child_then_act_then_association_then_body;
      eassumption.
  - eapply positional_deferred_act_outranks_noncanonical; eassumption.
  - eapply positional_association_outranks_noncanonical; eassumption.
  - apply (proj2 (proj2 (proj2 (proj2
      positional_error_precedence_is_total)))); assumption.
Qed.

Print Assumptions canonical_zero_writer_has_one_zero_sentinel_per_logical_endpoint.
Print Assumptions canonical_writer_and_image_are_address_independent.
Print Assumptions canonical_zero_writer_is_accepted_by_concrete_act_parser.
Print Assumptions admitted_positional_run_publishes_exactly_once.
Print Assumptions positional_entry_count_and_count_free_machine_agree.
Print Assumptions positional_error_precedence_is_total.
Print Assumptions campaign_policy_authorizes_amending_unreleased_epm1.
Print Assumptions epm1_positional_act_body_association_capstone.

End PositionalAssociationV6.
