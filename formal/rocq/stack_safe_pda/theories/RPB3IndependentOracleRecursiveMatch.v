(** Final assembly of the recursive mixed reference against the independent
    oracle.

    Statements proved here (all closed under the global context; see the
    [Print Assumptions] block at the end of the file):

    - [recursive_mixed_reference_matches_oracle]: for every hash function and
      every body, the recursive mixed Par reference equals the projection of
      the independent oracle's scored Par answer.
    - [recursive_node_matches_oracle]: for every node returned by [decode_root]
      of a program that the generated lookup resolves, the oracle's evaluation
      of that node has the [source_target_child_verdict] against the recursive
      source reference.  The restriction to nodes [decode_root] actually
      returns is essential: matching over arbitrary trees has a finite
      counterexample.
    - [recursive_root_children_match_oracle]: the unrejected oracle evaluation
      of the decoded root nodes and the recursive target's child list project
      to each other, through
      [RecursiveProjection.source_target_child_lists_project].

    Proof outline, in file order:

    - Fuel saturation.  [saturated_oracle] evaluates a raw field at fuel
      [S (length (raw_body raw))]; [source_evaluate_message_beyond_body_length]
      shows that any larger fuel computes the same answer, so the callback of
      [source_evaluate_node] can be replaced by [saturated_oracle] on every
      decoded node ([source_evaluate_node_callback_ext]).
    - Scan reflection.  [source_scan_children_valid_of_target_valid] shows that
      a target scan that is valid on the projected children makes the source
      scan valid, for faithful children.
    - Family 20 key order.  [new_order_bridge] shows that the oracle's
      [source_string_map_keys_ordered] and the target's
      [ordered_recursive_new_map_returnsb] agree on every child list accepted
      by the source scan of the generated New program.  The invariant
      ([new_map_order_state]) has three reachable states; a non-map child
      resets the oracle's memory but cannot precede a later map child in a
      valid scan, because the program has one repeated map slot.
    - One message.  [message_children_agreement] classifies the outcome of a
      message's children into the refused and accepted cases, and
      [message_node_verdict_from_outcome] turns that outcome into the node
      verdict.  [message_node_saturated_summary] collects the equations that
      hold for a decoded message node: the child program, the decode of its
      children, and the oracle's saturated equation over the absolute child
      nodes (the rebased decode is carried by [decoded_message_body_has_exact_rebased_children]).
    - The inner induction on decode fuel ([decoded_node_verdict_by_fuel]) gives
      the verdict for every node of a decoded region, given the Par theorem on
      strictly shorter bodies.
    - The Par root ([par_reference_matches_oracle_step]) and the outer strong
      induction on body length give the first theorem.  The node theorem and
      the root-list corollary follow.

    Proof-engineering notes, recorded so that these routes are not re-attempted:

    - [rewrite Recursive.local_recursive_children_are_global_children] directly
      after [cbn [Recursive.recursive_source_reference]] reports that no
      subterm matches, because the fold sits under the binders of the
      [checked_generated_raw], [descriptor_child_type_id] and [lookup] matches.
      Destruct those three matches first, as in
      [recursive_message_reference_unfold].
    - [destruct t eqn:E] also replaces [t] in the local context: premises stated
      about [t] then read [Some x = Some _], so they take [eq_refl], not [E]
      ([message_children_agreement]).
    - [settle_option_equation] needed a branch for [option_map _ _ = Some _];
      without it the tactic stops with a no-matching-clauses error.
    - Using [settle_option_equation] on the PathMap arm of
      [new_map_child_view_of_node] loses the result constructor that the
      conclusion inspects; that arm destructs its scrutinees explicitly.
    - [try assumption] after [apply message_outcome_when_ordered] also closes
      [target_map_ordered 20 results = true] by conversion, so the number of
      remaining goals depends on the case; the proof uses explicit goal
      selectors instead. *)

From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPM1 EPM1ACTree03 RPB3RecursiveParUnfold
  RPB3IndependentOracleAdequacy RPB3IndependentOracleScalarVerdictBridge
  RPB3IndependentOracleStringMapVerdictBridge
  RPB3IndependentOraclePathMapVerdictBridge
  RPB3IndependentOracleRegionChildBridge
  RPB3IndependentOracleStringMapTypedRebaseBridge
  RPB3IndependentOracleMessageAssembly
  RPB3IndependentOracleParMessageBodyDecodeBridge
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleMutants.

Import ListNotations.

(** * Fuel saturation of the independent oracle *)

(** The oracle evaluated at the fuel that saturates its own body. *)
Definition saturated_oracle (hash_fn : list nat -> nat)
    (raw : Schema.raw_field) : option Core.source_exact_summary :=
  Oracle.source_evaluate_message (S (length (Schema.raw_body raw))) hash_fn raw.

Lemma source_evaluate_message_beyond_body_length :
  forall fuel hash_fn raw,
    length (Schema.raw_body raw) < fuel ->
    Oracle.source_evaluate_message fuel hash_fn raw =
    saturated_oracle hash_fn raw.
Proof.
  intros fuel hash_fn raw enough.
  destruct (Nat.le_exists_sub (S (length (Schema.raw_body raw))) fuel)
    as [extra [split_fuel _]]; [lia|].
  rewrite split_fuel.
  rewrite (Nat.add_comm extra).
  exact (source_evaluate_message_saturates_at_body_length extra hash_fn raw).
Qed.

(** Two evaluator callbacks that agree on every strictly smaller body produce
    the same result on each node of a decoded region of that body. *)
Lemma source_evaluate_node_callback_ext :
  forall (recurse_left recurse_right :
      Schema.raw_field -> option Core.source_exact_summary)
    hash_fn body program fuel nodes node,
    (forall raw, length (Schema.raw_body raw) < length body ->
      recurse_left raw = recurse_right raw) ->
    Source.decode_region fuel body program 0 (length body) = Some nodes ->
    In node nodes ->
    Oracle.source_evaluate_node recurse_left hash_fn body program node =
    Oracle.source_evaluate_node recurse_right hash_fn body program node.
Proof.
  intros recurse_left recurse_right hash_fn body program fuel nodes node
    agree decoded member.
  unfold Oracle.source_evaluate_node.
  destruct (Source.raw_field_of_source body node)
    as [child_raw|] eqn:converted; [|reflexivity].
  destruct node as [span field|span field nested|span field];
    cbn beta iota zeta; try reflexivity.
  - rewrite agree; [reflexivity|].
    exact (Fuel.decoded_source_raw_child_body_is_strictly_smaller
      fuel body program nodes (Source.SourceMessage span field nested)
      child_raw decoded member converted).
  - destruct (Schema.descriptor_payload field) eqn:role;
      try reflexivity.
    + destruct (Source.refine_string_map_entry body program
          (Source.SourceAuxiliary span field))
        as [entry|] eqn:refined; [|reflexivity].
      destruct (Oracle.source_valid_string_map_key
          (Source.typed_map_key_source entry))
        as [key|] eqn:key_valid; [|reflexivity].
      destruct (Source.typed_map_value_source entry)
        as [[value_span value_body]|] eqn:value_source;
        [|reflexivity].
      rewrite (agree (Oracle.source_par_raw value_body)); [reflexivity|].
      change (length value_body < length body).
      exact (Fuel.refined_string_map_par_body_is_strictly_smaller
        body program span field entry value_span value_body
        refined value_source).
    + destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
          (Schema.raw_body child_raw)) as [[snapshot trie]|]
        eqn:decoded_epm; [|reflexivity].
      assert (selected_agree :
        Source.sequence_source_results
          (map (fun value_body =>
            source_selected_unrefused_par
              (recurse_left (Oracle.source_par_raw value_body)))
            (EPM1.value_bodies snapshot)) =
        Source.sequence_source_results
          (map (fun value_body =>
            source_selected_unrefused_par
              (recurse_right (Oracle.source_par_raw value_body)))
            (EPM1.value_bodies snapshot))).
      {
        apply source_sequence_agrees_on_members.
        intros value_body member_value.
        rewrite (agree (Oracle.source_par_raw value_body)); [reflexivity|].
        change (length value_body < length body).
        exact (Fuel.decoded_pathmap_par_body_is_strictly_smaller
          body (Source.SourceAuxiliary span field) child_raw
          hash_fn snapshot trie value_body converted decoded_epm
          member_value).
      }
      unfold source_selected_unrefused_par in selected_agree.
      rewrite selected_agree.
      reflexivity.
Qed.

(** * Reverse scan projection

    [Projection.source_scan_children_project_valid] proves that a source scan
    that stays valid projects to a valid target scan.  The verdict relation also
    needs the converse on the children the oracle actually selects: a target
    scan over their projections that stays valid means the source scan stays
    valid.  The converse needs one per-child fact the target does not check,
    the well-formedness of the raw body, and one it reads only through a
    default: a message payload carries a program type id. *)

Definition child_raw_is_faithful (child : Core.source_exact_summary) : Prop :=
  Admission.source_raw_bytes_well_formed (Core.source_summary_raw child) = true /\
  (Schema.raw_payload (Core.source_summary_raw child) = Schema.Message ->
   exists observed,
     Schema.raw_program_type_id (Core.source_summary_raw child) = Some observed).

Lemma payload_eqb_true :
  forall left right, Schema.payload_eqb left right = true -> left = right.
Proof.
  intros [] [] same; cbn in same; try discriminate same; reflexivity.
Qed.

(** The target's admissibility guard, read off an accepted [Target.scan_child]
    step on a valid state. *)
Lemma target_scan_child_accepted_guards :
  forall program state child descriptor,
    Schema.scan_valid state = true ->
    Schema.resolve_tag (Schema.program_fields program) (Target.sem_tag child) =
      Some descriptor ->
    Schema.scan_valid (Target.scan_child program state child) = true ->
    negb (Target.sem_definitely_refused child) = true /\
    Schema.wire_eqb (Target.sem_wire child) (Schema.descriptor_wire descriptor) = true /\
    Schema.payload_eqb (Target.sem_payload child)
      (Schema.descriptor_payload descriptor) = true /\
    match Schema.descriptor_payload descriptor with
    | Schema.Message =>
        match Schema.descriptor_child_type_id descriptor with
        | Some type_id => Target.sem_type_id child =? type_id
        | None => false
        end
    | _ => true
    end = true /\
    (Schema.descriptor_slot descriptor <? Schema.program_slot_count program) = true /\
    Schema.slot_follows (Schema.scan_last_slot state)
      (Schema.descriptor_slot descriptor)
      (Schema.descriptor_repeated descriptor) = true /\
    (Schema.descriptor_repeated descriptor ||
      negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
        (Schema.scan_seen state))) = true /\
    Schema.scan_last_slot (Target.scan_child program state child) =
      Some (Schema.descriptor_slot descriptor) /\
    Schema.scan_seen (Target.scan_child program state child) =
      Schema.descriptor_slot descriptor :: Schema.scan_seen state.
Proof.
  intros program state child descriptor valid resolved accepted.
  unfold Target.scan_child in accepted |- *.
  rewrite valid, resolved in accepted.
  rewrite valid, resolved.
  cbv beta iota zeta in accepted |- *.
  match type of accepted with
  | Schema.scan_valid (if ?guard then _ else _) = true =>
      destruct guard eqn:admissible
  end.
  - repeat rewrite andb_true_iff in admissible.
    destruct admissible as [[[[[[unrefused wire] payload] type_matches]
      slot_bound] slot_order] slot_unique].
    repeat split; assumption.
  - cbn in accepted. discriminate accepted.
Qed.

(** A faithful child accepted by the target's guard is accepted by the
    source's guard. *)
Lemma source_descriptor_guard_of_target_guards :
  forall descriptor child,
    child_raw_is_faithful child ->
    negb (Target.sem_definitely_refused
      (Projection.project_source_summary child)) = true ->
    Schema.wire_eqb (Target.sem_wire (Projection.project_source_summary child))
      (Schema.descriptor_wire descriptor) = true ->
    Schema.payload_eqb (Target.sem_payload (Projection.project_source_summary child))
      (Schema.descriptor_payload descriptor) = true ->
    match Schema.descriptor_payload descriptor with
    | Schema.Message =>
        match Schema.descriptor_child_type_id descriptor with
        | Some type_id =>
            Target.sem_type_id (Projection.project_source_summary child) =? type_id
        | None => false
        end
    | _ => true
    end = true ->
    Admission.source_child_matches_descriptor descriptor child = true.
Proof.
  intros descriptor child [well_formed type_present] unrefused wire payload
    type_matches.
  change (Target.sem_definitely_refused (Projection.project_source_summary child))
    with (Core.source_summary_refused child) in unrefused.
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)) in wire.
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)) in payload.
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (Projection.project_source_type_id (Core.source_summary_raw child))
    in type_matches.
  unfold Admission.source_child_matches_descriptor.
  cbv zeta.
  rewrite unrefused, well_formed, wire, payload.
  cbn [andb].
  destruct (Schema.descriptor_payload descriptor) eqn:descriptor_kind;
    try reflexivity.
  (* The descriptor names a message; the child's raw is a message too. *)
  assert (raw_message :
      Schema.raw_payload (Core.source_summary_raw child) = Schema.Message).
  { apply payload_eqb_true. exact payload. }
  destruct (type_present raw_message) as [observed observed_type].
  unfold Projection.project_source_type_id in type_matches.
  rewrite observed_type in type_matches.
  rewrite observed_type.
  destruct (Schema.descriptor_child_type_id descriptor) as [expected|];
    [|discriminate type_matches].
  rewrite Nat.eqb_sym.
  exact type_matches.
Qed.

(** One target-accepted step with synchronized states is one source-accepted
    step, and the states stay synchronized. *)
Lemma target_scan_step_reflects_source_valid :
  forall program source_state target_state child,
    child_raw_is_faithful child ->
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Schema.scan_valid
      (Target.scan_child program target_state
        (Projection.project_source_summary child)) = true ->
    Admission.source_scan_valid
      (Admission.source_scan_step program source_state child) = true /\
    Admission.source_scan_last_slot
      (Admission.source_scan_step program source_state child) =
      Schema.scan_last_slot
        (Target.scan_child program target_state
          (Projection.project_source_summary child)) /\
    Admission.source_scan_seen_slots
      (Admission.source_scan_step program source_state child) =
      Schema.scan_seen
        (Target.scan_child program target_state
          (Projection.project_source_summary child)).
Proof.
  intros program source_state target_state child faithful source_valid
    target_valid last_same seen_same accepted.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|] eqn:resolved.
  - assert (target_resolved :
      Schema.resolve_tag (Schema.program_fields program)
        (Target.sem_tag (Projection.project_source_summary child)) =
        Some descriptor) by exact resolved.
    destruct (target_scan_child_accepted_guards program target_state
      (Projection.project_source_summary child) descriptor target_valid
      target_resolved accepted)
      as [unrefused [wire [payload [type_matches [slot_bound [slot_order
        [slot_unique [target_last target_seen]]]]]]]].
    pose proof (source_descriptor_guard_of_target_guards descriptor child
      faithful unrefused wire payload type_matches) as child_guard.
    assert (source_order :
      Admission.source_slot_follows
        (Admission.source_scan_last_slot source_state)
        (Schema.descriptor_slot descriptor)
        (Schema.descriptor_repeated descriptor) = true).
    { rewrite last_same. exact slot_order. }
    assert (source_unique :
      (Schema.descriptor_repeated descriptor ||
        negb (existsb (Nat.eqb (Schema.descriptor_slot descriptor))
          (Admission.source_scan_seen_slots source_state))) = true).
    { rewrite seen_same. exact slot_unique. }
    unfold Admission.source_scan_step.
    rewrite source_valid, resolved.
    cbv beta iota zeta.
    rewrite child_guard, slot_bound, source_order, source_unique.
    cbn [andb Admission.source_scan_valid Admission.source_scan_last_slot
      Admission.source_scan_seen_slots].
    rewrite target_last, target_seen, seen_same.
    repeat split; reflexivity.
  - (* An unresolved tag fails the target step as well. *)
    exfalso.
    unfold Target.scan_child in accepted.
    rewrite target_valid in accepted.
    change (Target.sem_tag (Projection.project_source_summary child))
      with (Core.source_summary_tag child) in accepted.
    rewrite resolved in accepted.
    cbn in accepted.
    discriminate accepted.
Qed.

(** A target scan over the projections of faithful children that stays valid
    means the source scan of those children stays valid, with the same last
    slot and seen slots. *)
Lemma source_scan_valid_of_target_scan_valid :
  forall program children source_state target_state,
    Forall child_raw_is_faithful children ->
    Admission.source_scan_valid source_state = true ->
    Schema.scan_valid target_state = true ->
    Admission.source_scan_last_slot source_state =
      Schema.scan_last_slot target_state ->
    Admission.source_scan_seen_slots source_state =
      Schema.scan_seen target_state ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) target_state) = true ->
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) children source_state) = true.
Proof.
  intros program children.
  induction children as [|child rest IH];
    intros source_state target_state faithful source_valid target_valid
      last_same seen_same accepted.
  - exact source_valid.
  - inversion faithful as [|? ? child_faithful rest_faithful]; subst.
    cbn [map fold_left] in accepted |- *.
    assert (step_valid :
      Schema.scan_valid (Target.scan_child program target_state
        (Projection.project_source_summary child)) = true).
    { destruct (Schema.scan_valid (Target.scan_child program target_state
        (Projection.project_source_summary child))) eqn:observed;
        [reflexivity|].
      exfalso.
      assert (rest_invalid :
        forall (children : list Target.semantic_summary) state,
          Schema.scan_valid state = false ->
          Schema.scan_valid (fold_left (Target.scan_child program) children state) = false).
      { clear. induction children as [|head tail IHtail]; intros state invalid.
        - exact invalid.
        - cbn [fold_left].
          apply IHtail.
          unfold Target.scan_child.
          rewrite invalid.
          exact invalid. }
      pose proof (rest_invalid
        (map Projection.project_source_summary rest) _ observed) as impossible.
      rewrite accepted in impossible.
      discriminate impossible. }
    destruct (target_scan_step_reflects_source_valid program source_state
      target_state child child_faithful source_valid target_valid
      last_same seen_same step_valid) as [next_valid [next_last next_seen]].
    exact (IH _ _ rest_faithful next_valid step_valid next_last next_seen
      accepted).
Qed.

Corollary source_scan_children_valid_of_target_valid :
  forall program children,
    Forall child_raw_is_faithful children ->
    Schema.scan_valid
      (fold_left (Target.scan_child program)
        (map Projection.project_source_summary children) Schema.empty_scan) = true ->
    Admission.source_scan_valid
      (Admission.source_scan_children program children) = true.
Proof.
  intros program children faithful accepted.
  unfold Admission.source_scan_children.
  apply (source_scan_valid_of_target_scan_valid program children
    Admission.source_empty_scan Schema.empty_scan faithful);
    try reflexivity.
  exact accepted.
Qed.

(** * Per-child raw facts of the oracle's node evaluation *)

(** The nested-match walk of [RPB3IndependentOracleProcessChildCorrespondence],
    restated here so this file stays self-contained. *)
Ltac settle_option_equation evaluated :=
  cbv beta iota zeta in evaluated;
  lazymatch type of evaluated with
  | Some _ = Some _ => idtac
  | None = Some _ => discriminate evaluated
  | option_map _ _ = Some _ =>
      unfold option_map in evaluated;
      settle_option_equation evaluated
  | (match ?scrutinee with _ => _ end) = Some _ =>
      destruct scrutinee; settle_option_equation evaluated
  end.

Lemma evaluated_node_retains_converted_raw :
  forall (recurse : Schema.raw_field -> option Core.source_exact_summary)
      hash_fn bytes program node summary,
    (forall raw evaluated, recurse raw = Some evaluated ->
      Core.source_summary_raw evaluated = raw) ->
    Oracle.source_evaluate_node recurse hash_fn bytes program node = Some summary ->
    exists raw,
      Source.raw_field_of_source bytes node = Some raw /\
      Core.source_summary_raw summary = raw.
Proof.
  intros recurse hash_fn bytes program node summary recurse_retains evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source bytes node)
    as [raw|] eqn:converted; [|discriminate evaluated].
  exists raw. split; [reflexivity|].
  destruct node as [span field|span field children|span field].
  - destruct (Admission.source_typed_scalar program raw) as [value|] eqn:typed;
      cbn [option_map] in evaluated; [|discriminate evaluated].
    injection evaluated as same; subst summary.
    exact (RecursiveProjection.accepted_source_typed_scalar_retains_raw
      program raw value typed).
  - exact (recurse_retains raw summary evaluated).
  - destruct (Schema.descriptor_payload field);
      [discriminate evaluated|discriminate evaluated| |].
    all: settle_option_equation evaluated.
    all: injection evaluated as same; subst summary.
    all: reflexivity.
Qed.

Lemma decoded_node_child_is_faithful :
  forall (recurse : Schema.raw_field -> option Core.source_exact_summary)
      hash_fn bytes program fuel start finish nodes node summary,
    (forall raw evaluated, recurse raw = Some evaluated ->
      Core.source_summary_raw evaluated = raw) ->
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In node nodes ->
    Oracle.source_evaluate_node recurse hash_fn bytes program node = Some summary ->
    child_raw_is_faithful summary.
Proof.
  intros recurse hash_fn bytes program fuel start finish nodes node summary
    recurse_retains decoded member evaluated.
  destruct (evaluated_node_retains_converted_raw recurse hash_fn bytes program
    node summary recurse_retains evaluated) as [raw [converted same_raw]].
  unfold child_raw_is_faithful.
  rewrite same_raw.
  split.
  - exact (RegionChild.decoded_node_raw_body_is_valid fuel bytes program start
      finish nodes node raw decoded member converted).
  - intros raw_message.
    destruct node as [span field|span field children|span field].
    + destruct (converted_scalar_raw_has_scalar_shape bytes span field raw converted)
        as [scalar_payload _].
      rewrite scalar_payload in raw_message.
      discriminate raw_message.
    + pose proof (Source.raw_message_field_uses_generated_child_type
        bytes span field children raw converted) as same_type.
      unfold Source.raw_field_of_source in converted.
      cbn [Source.source_span Source.source_field] in converted.
      destruct (Schema.descriptor_payload field);
        cbn [Schema.payload_eqb] in converted; try discriminate converted.
      destruct (Schema.descriptor_child_type_id field) as [type_id|];
        cbn [option_map] in converted; [|discriminate converted].
      exists type_id. exact same_type.
    + exfalso.
      pose proof (Source.raw_field_of_source_preserves_wire_and_body bytes
        (Source.SourceAuxiliary span field) raw converted)
        as [_ [_ [payload_is _]]].
      cbn [Source.source_field] in payload_is.
      rewrite raw_message in payload_is.
      unfold Source.raw_field_of_source in converted.
      cbn [Source.source_span Source.source_field] in converted.
      rewrite <- payload_is in converted.
      discriminate converted.
Qed.

(** * Positional transport of per-node facts *)

Lemma sequenced_results_satisfy :
  forall (A B : Type) (evaluate : A -> option B) (property : B -> Prop)
      nodes results,
    Source.sequence_source_results (map evaluate nodes) = Some results ->
    (forall node result, In node nodes -> evaluate node = Some result ->
      property result) ->
    Forall property results.
Proof.
  intros A B evaluate property nodes.
  induction nodes as [|node rest IH]; intros results selected all_facts.
  - cbn in selected. injection selected as same. subst results. constructor.
  - cbn [map Source.sequence_source_results] in selected.
    destruct (evaluate node) as [head|] eqn:evaluated; [|discriminate selected].
    destruct (Source.sequence_source_results (map evaluate rest))
      as [tail|] eqn:remaining; [|discriminate selected].
    injection selected as same. subst results.
    constructor.
    + eapply all_facts; [left; reflexivity|exact evaluated].
    + apply IH; [reflexivity|].
      intros member result inside produced.
      eapply all_facts; [right; exact inside|exact produced].
Qed.

Lemma positional_results_zip :
  forall (A B C : Type)
      (evaluate : A -> option B) (target : A -> option C)
      (property : B -> C -> Prop) nodes summaries results,
    Forall2 (fun node summary => evaluate node = Some summary) nodes summaries ->
    Forall2 (fun node result => target node = Some result) nodes results ->
    (forall node summary result, In node nodes ->
      evaluate node = Some summary -> target node = Some result ->
      property summary result) ->
    Forall2 property summaries results.
Proof.
  intros A B C evaluate target property nodes summaries results
    selected accepted facts.
  revert results accepted facts.
  induction selected as [|node summary nodes' summaries' chosen rest IH];
    intros results accepted facts.
  - inversion accepted; subst. constructor.
  - inversion accepted as [|node_copy result nodes_copy results' produced later];
      subst.
    constructor.
    + eapply facts; [left; reflexivity|exact chosen|exact produced].
    + apply IH; [exact later|].
      intros member s r inside evaluated targeted.
      eapply facts; [right; exact inside|exact evaluated|exact targeted].
Qed.

(** * Family 20: the string-map key order bridge

    The oracle's order check ([Oracle.source_string_map_keys_ordered]) compares
    adjacent [TypedSourceStringMap] children with equal tags and forgets the
    previous key at any other child.  The target
    ([Recursive.ordered_recursive_new_map_returnsb]) compares consecutive map
    entries regardless of what lies between them.  They agree on a valid family
    20 scan: the single map slot is contiguous there, because the slot order of
    the scan forbids a map child after a later slot. *)

(** The fragment of a child pair the order checks read: the map key, and
    whether the child is a map child at all. *)
Definition new_map_child_view
    (child : Core.source_exact_summary)
    (result : Recursive.recursive_mixed_result) : Prop :=
  match Core.source_summary_value child with
  | Core.TypedSourceStringMap raw key _ =>
      Schema.raw_payload raw = Schema.StringMap /\
      match result with
      | Recursive.RecursiveAuxiliaryResult
          (Recursive.RecursiveStringMapResult _ entry) _ =>
          Source.returned_map_key Semantic.semantic_summary entry = key
      | _ => False
      end
  | other =>
      Schema.raw_payload (Core.source_value_raw other) <> Schema.StringMap /\
      match result with
      | Recursive.RecursiveAuxiliaryResult
          (Recursive.RecursiveStringMapResult _ _) _ => False
      | _ => True
      end
  end.

(** The three reachable relations between the oracle's memory of the previous
    map child, the target's prior key, and the last accepted slot. *)
Definition new_map_order_state
    (previous : option (nat * list nat)) (prior : option (list nat))
    (last_slot : option nat) : Prop :=
  (previous = None /\ prior = None) \/
  (exists key, previous = Some (4, key) /\ prior = Some key /\
    last_slot = Some 3) \/
  (exists key slot, previous = None /\ prior = Some key /\
    last_slot = Some slot /\ 3 < slot).

Lemma resolve_tag_some :
  forall fields tag descriptor,
    Schema.resolve_tag fields tag = Some descriptor ->
    In descriptor fields /\ Schema.descriptor_tag descriptor = tag.
Proof.
  induction fields as [|head rest IH]; intros tag descriptor resolved;
    cbn [Schema.resolve_tag] in resolved; [discriminate resolved|].
  destruct (Schema.descriptor_tag head =? tag) eqn:same.
  - injection resolved as same_descriptor. subst descriptor.
    apply Nat.eqb_eq in same.
    split; [left; reflexivity|exact same].
  - destruct (IH tag descriptor resolved) as [member same_tag].
    split; [right; exact member|exact same_tag].
Qed.

(** The generated New table declares exactly one string-map field. *)
Lemma new_program_string_map_descriptor :
  forall descriptor,
    In descriptor (Schema.program_fields Source.generated_new_program) ->
    Schema.descriptor_payload descriptor = Schema.StringMap ->
    Schema.descriptor_tag descriptor = 4 /\
    Schema.descriptor_slot descriptor = 3 /\
    Schema.descriptor_repeated descriptor = true.
Proof.
  intros descriptor member map_payload.
  vm_compute in member.
  repeat (destruct member as [member|member];
    [subst descriptor; vm_compute in map_payload;
     try discriminate map_payload; repeat split; reflexivity|]).
  contradiction.
Qed.

Lemma new_program_non_map_slot :
  forall descriptor,
    In descriptor (Schema.program_fields Source.generated_new_program) ->
    Schema.descriptor_payload descriptor <> Schema.StringMap ->
    Schema.descriptor_slot descriptor <> 3.
Proof.
  intros descriptor member not_map.
  vm_compute in member.
  repeat (destruct member as [member|member];
    [subst descriptor; first [ vm_compute; discriminate |
       exfalso; apply not_map; vm_compute; reflexivity ] |]).
  contradiction.
Qed.

(** The guards of an accepted source scan step on a valid state. *)
Lemma source_scan_step_accepted_guards :
  forall program state child,
    Admission.source_scan_valid state = true ->
    Admission.source_scan_valid
      (Admission.source_scan_step program state child) = true ->
    exists descriptor,
      Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child) = Some descriptor /\
      Admission.source_child_matches_descriptor descriptor child = true /\
      Admission.source_slot_follows (Admission.source_scan_last_slot state)
        (Schema.descriptor_slot descriptor)
        (Schema.descriptor_repeated descriptor) = true /\
      Admission.source_scan_last_slot
        (Admission.source_scan_step program state child) =
        Some (Schema.descriptor_slot descriptor).
Proof.
  intros program state child valid accepted.
  unfold Admission.source_scan_step in accepted |- *.
  rewrite valid in accepted |- *.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|] eqn:resolved;
    [|cbn in accepted; discriminate accepted].
  exists descriptor.
  cbv beta iota zeta in accepted |- *.
  match type of accepted with
  | Admission.source_scan_valid (if ?guard then _ else _) = true =>
      destruct guard eqn:admissible
  end.
  - repeat rewrite andb_true_iff in admissible.
    destruct admissible as [[[child_guard slot_bound] slot_order] slot_unique].
    repeat split; try reflexivity; assumption.
  - cbn in accepted. discriminate accepted.
Qed.

Lemma source_child_guard_payload :
  forall descriptor child,
    Admission.source_child_matches_descriptor descriptor child = true ->
    Schema.raw_payload (Core.source_summary_raw child) =
      Schema.descriptor_payload descriptor.
Proof.
  intros descriptor child guard.
  unfold Admission.source_child_matches_descriptor in guard.
  cbv zeta in guard.
  repeat rewrite andb_true_iff in guard.
  destruct guard as [[[[_ _] _] same_payload] _].
  apply payload_eqb_true.
  exact same_payload.
Qed.

Lemma source_scan_rest_valid_of_fold_valid :
  forall program rest state,
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) rest state) = true ->
    Admission.source_scan_valid state = true.
Proof.
  intros program rest state accepted.
  destruct (Admission.source_scan_valid state) eqn:observed; [reflexivity|].
  pose proof (Projection.source_scan_invalid_stays_invalid program rest state
    observed) as impossible.
  rewrite accepted in impossible.
  discriminate impossible.
Qed.

Lemma new_order_bridge_after :
  forall children results,
    Forall2 new_map_child_view children results ->
    forall state previous prior,
      Admission.source_scan_valid state = true ->
      Admission.source_scan_valid
        (fold_left (Admission.source_scan_step Source.generated_new_program)
          children state) = true ->
      new_map_order_state previous prior (Admission.source_scan_last_slot state) ->
      Oracle.source_string_map_keys_ordered_after previous children =
      Recursive.ordered_recursive_new_map_returnsb prior
        (Recursive.recursive_new_map_returns results).
Proof.
  intros children results views.
  induction views as [|child result rest_children rest_results view views_rest IH];
    intros state previous prior state_valid scan_valid invariant.
  - reflexivity.
  - cbn [fold_left] in scan_valid.
    set (next := Admission.source_scan_step Source.generated_new_program state child)
      in *.
    assert (next_valid : Admission.source_scan_valid next = true)
      by exact (source_scan_rest_valid_of_fold_valid
        Source.generated_new_program rest_children next scan_valid).
    destruct (source_scan_step_accepted_guards Source.generated_new_program
      state child state_valid next_valid)
      as [descriptor [resolved [child_guard [follows last_slot]]]].
    fold next in last_slot.
    destruct (resolve_tag_some _ _ _ resolved) as [member same_tag].
    pose proof (source_child_guard_payload descriptor child child_guard)
      as raw_payload_same.
    unfold new_map_child_view in view.
    unfold Oracle.source_string_map_keys_ordered_after;
      fold Oracle.source_string_map_keys_ordered_after.
    unfold Core.source_summary_raw in raw_payload_same.
    unfold Core.source_summary_tag, Core.source_summary_raw in same_tag.
    destruct (Core.source_summary_value child) as [raw scalar|raw nested|raw key value|raw snapshot values]
      eqn:child_value.
    + (* scalar child *)
      cbn [Core.source_value_raw] in view, raw_payload_same, same_tag.
      destruct view as [not_map shape].
      assert (not_map_payload : Schema.descriptor_payload descriptor <> Schema.StringMap).
      { rewrite <- raw_payload_same. exact not_map. }
      pose proof (new_program_non_map_slot descriptor member not_map_payload) as slot_not_map.
      cbv beta iota zeta.
      destruct result as [? ?|? ? ?|[? ?|? ? ? ?] ?]; [| | contradiction shape |];
        cbn [Recursive.recursive_new_map_returns];
        apply (IH next None prior next_valid scan_valid);
        unfold new_map_order_state in invariant |- *;
        rewrite last_slot;
        (destruct invariant as [[-> ->]|[[k [-> [-> last3]]]|[k [s [-> [-> [lasts more]]]]]]];
         [ left; split; reflexivity
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite last3 in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; exact strict|];
           exfalso; apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; apply slot_not_map; symmetry; exact same
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite lasts in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; lia|];
           apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; lia ]).
    + (* message child *)
      cbn [Core.source_value_raw] in view, raw_payload_same, same_tag.
      destruct view as [not_map shape].
      assert (not_map_payload : Schema.descriptor_payload descriptor <> Schema.StringMap).
      { rewrite <- raw_payload_same. exact not_map. }
      pose proof (new_program_non_map_slot descriptor member not_map_payload) as slot_not_map.
      cbv beta iota zeta.
      destruct result as [? ?|? ? ?|[? ?|? ? ? ?] ?]; [| | contradiction shape |];
        cbn [Recursive.recursive_new_map_returns];
        apply (IH next None prior next_valid scan_valid);
        unfold new_map_order_state in invariant |- *;
        rewrite last_slot;
        (destruct invariant as [[-> ->]|[[k [-> [-> last3]]]|[k [s [-> [-> [lasts more]]]]]]];
         [ left; split; reflexivity
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite last3 in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; exact strict|];
           exfalso; apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; apply slot_not_map; symmetry; exact same
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite lasts in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; lia|];
           apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; lia ]).
    + (* string-map child *)
      cbn [Core.source_value_raw] in raw_payload_same, same_tag.
      destruct view as [map_payload shape].
      destruct result as [? ?|? ? ?|[? entry|? ? ? ?] ?];
        try contradiction shape.
      assert (descriptor_map : Schema.descriptor_payload descriptor = Schema.StringMap).
      { rewrite <- raw_payload_same. exact map_payload. }
      destruct (new_program_string_map_descriptor descriptor member descriptor_map)
        as [descriptor_tag_four [descriptor_slot_three descriptor_repeated_true]].
      assert (raw_tag_four : Schema.raw_tag raw = 4) by lia.
      cbn [Recursive.recursive_new_map_returns
        Recursive.ordered_recursive_new_map_returnsb].
      rewrite shape.
      unfold new_map_order_state in invariant.
      rewrite descriptor_slot_three, descriptor_repeated_true in follows.
      rewrite descriptor_slot_three in last_slot.
      (destruct invariant as [[-> ->]|[[k [-> [-> last3]]]|[k [s [-> [-> [lasts more]]]]]]]).
      * cbn [Source.map_key_follows andb].
        rewrite raw_tag_four.
        apply (IH next (Some (4, key)) (Some key) next_valid scan_valid).
        rewrite last_slot.
        unfold new_map_order_state.
        right. left. exists key. repeat split; reflexivity.
      * rewrite raw_tag_four.
        cbn [Source.map_key_follows].
        rewrite Nat.eqb_refl.
        f_equal.
        apply (IH next (Some (4, key)) (Some key) next_valid scan_valid).
        rewrite last_slot.
        unfold new_map_order_state.
        right. left. exists key. repeat split; reflexivity.
      * exfalso.
        rewrite lasts in follows.
        unfold Admission.source_slot_follows in follows.
        apply orb_true_iff in follows as [strict|same].
        -- apply Nat.ltb_lt in strict. lia.
        -- apply andb_true_iff in same as [same _].
           apply Nat.eqb_eq in same. lia.
    + (* pathmap child *)
      cbn [Core.source_value_raw] in view, raw_payload_same, same_tag.
      destruct view as [not_map shape].
      assert (not_map_payload : Schema.descriptor_payload descriptor <> Schema.StringMap).
      { rewrite <- raw_payload_same. exact not_map. }
      pose proof (new_program_non_map_slot descriptor member not_map_payload) as slot_not_map.
      cbv beta iota zeta.
      destruct result as [? ?|? ? ?|[? ?|? ? ? ?] ?]; [| | contradiction shape |];
        cbn [Recursive.recursive_new_map_returns];
        apply (IH next None prior next_valid scan_valid);
        unfold new_map_order_state in invariant |- *;
        rewrite last_slot;
        (destruct invariant as [[-> ->]|[[k [-> [-> last3]]]|[k [s [-> [-> [lasts more]]]]]]];
         [ left; split; reflexivity
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite last3 in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; exact strict|];
           exfalso; apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; apply slot_not_map; symmetry; exact same
         | right; right; exists k, (Schema.descriptor_slot descriptor);
           repeat split; try reflexivity;
           rewrite lasts in follows;
           unfold Admission.source_slot_follows in follows;
           apply orb_true_iff in follows as [strict|same];
           [apply Nat.ltb_lt in strict; lia|];
           apply andb_true_iff in same as [same _];
           apply Nat.eqb_eq in same; lia ]).
Qed.

Theorem new_order_bridge :
  forall children results,
    Forall2 new_map_child_view children results ->
    Admission.source_scan_valid
      (Admission.source_scan_children Source.generated_new_program children) = true ->
    Oracle.source_string_map_keys_ordered children =
    Recursive.ordered_recursive_new_map_returnsb None
      (Recursive.recursive_new_map_returns results).
Proof.
  intros children results views valid.
  unfold Oracle.source_string_map_keys_ordered.
  apply (new_order_bridge_after children results views
    Admission.source_empty_scan None None).
  - reflexivity.
  - exact valid.
  - left. split; reflexivity.
Qed.

(** * The map view of a decoded node *)

Lemma new_map_child_view_of_node :
  forall (recurse : Schema.raw_field -> option Core.source_exact_summary)
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      hash_fn bytes program fuel start finish nodes node summary result,
    (forall raw evaluated, recurse raw = Some evaluated ->
      exists children,
        Core.source_summary_value evaluated = Core.TypedSourceMessage raw children /\
        Schema.raw_payload raw = Schema.Message) ->
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In node nodes ->
    Oracle.source_evaluate_node recurse hash_fn bytes program node = Some summary ->
    Recursive.recursive_source_reference par_child hash_fn bytes program node =
      Some result ->
    new_map_child_view summary result.
Proof.
  intros recurse par_child hash_fn bytes program fuel start finish nodes node
    summary result recurse_shape decoded member evaluated target.
  destruct node as [span field|span field children|span field].
  - destruct (accepted_source_scalar_node_has_typed_value recurse hash_fn bytes
      program span field summary evaluated)
      as [raw [scalar [_ [typed payload]]]].
    cbn [Recursive.recursive_source_reference] in target.
    settle_option_equation target.
    injection target as same_result. subst result.
    unfold new_map_child_view.
    rewrite typed.
    cbn [Core.source_value_raw].
    split; [rewrite payload; discriminate|exact I].
  - unfold Oracle.source_evaluate_node in evaluated.
    destruct (Source.raw_field_of_source bytes
      (Source.SourceMessage span field children)) as [raw|] eqn:converted;
      [|discriminate evaluated].
    destruct (recurse_shape raw summary evaluated) as [sub [typed payload]].
    cbn [Recursive.recursive_source_reference] in target.
    destruct (Recursive.checked_generated_raw bytes program
      (Source.SourceMessage span field children)) as [checked|];
      [|discriminate target].
    destruct (Schema.descriptor_child_type_id field) as [type_id|];
      [|discriminate target].
    destruct (Schema.lookup_projected_program type_id [] [])
      as [child_program|]; [|discriminate target].
    rewrite Recursive.local_recursive_children_are_global_children in target.
    settle_option_equation target.
    injection target as same_result. subst result.
    unfold new_map_child_view.
    rewrite typed.
    cbn [Core.source_value_raw].
    split; [rewrite payload; discriminate|exact I].
  - destruct (Schema.descriptor_payload field) eqn:kind.
    + exfalso.
      unfold Oracle.source_evaluate_node in evaluated.
      destruct (Source.raw_field_of_source bytes
        (Source.SourceAuxiliary span field)) as [raw|] eqn:converted;
        [|discriminate evaluated].
      rewrite kind in evaluated.
      discriminate evaluated.
    + exfalso.
      unfold Oracle.source_evaluate_node in evaluated.
      destruct (Source.raw_field_of_source bytes
        (Source.SourceAuxiliary span field)) as [raw|] eqn:converted;
        [|discriminate evaluated].
      rewrite kind in evaluated.
      discriminate evaluated.
    + (* StringMap *)
      destruct (Source.source_forall_member _ _ _ _
        (Source.decoded_region_nodes_match_generated_source
          fuel bytes program start finish nodes decoded) member) as [selected _].
      cbn [Source.source_span Source.source_field] in selected.
      rewrite (oracle_string_map_node_unfold recurse hash_fn bytes program span
        field kind) in evaluated.
      rewrite (target_string_map_node_unfold par_child hash_fn bytes program span
        field kind selected) in target.
      rewrite (RegionChild.decoded_node_checked_raw_agrees fuel bytes program
        start finish nodes (Source.SourceAuxiliary span field) decoded member)
        in target.
      destruct (Source.raw_field_of_source bytes
        (Source.SourceAuxiliary span field)) as [raw|] eqn:converted;
        [|discriminate evaluated].
      destruct (Source.refine_string_map_entry bytes program
        (Source.SourceAuxiliary span field)) as [entry|] eqn:refined;
        [|discriminate evaluated].
      pose proof (Source.raw_field_of_source_preserves_wire_and_body bytes
        (Source.SourceAuxiliary span field) raw converted)
        as [_ [_ [raw_payload_is _]]].
      cbn [Source.source_field] in raw_payload_is.
      unfold oracle_sources_result in evaluated.
      destruct (Oracle.source_valid_string_map_key
        (Source.typed_map_key_source entry)) as [key|] eqn:key_valid;
        [|discriminate evaluated].
      unfold target_sources_result in target.
      destruct (Source.reduce_map_key (Source.typed_map_key_source entry))
        as [[reduced_key key_span]|] eqn:reduced;
        [|discriminate target].
      assert (keys_equal : key = reduced_key).
      { rewrite (string_map_key_gates_agree
          (Source.typed_map_key_source entry)
          (fun source_span source_key source_equation =>
            refined_string_map_key_is_in_byte_range bytes program span field
              entry source_span source_key refined source_equation))
          in key_valid.
        rewrite reduced in key_valid.
        cbn in key_valid.
        injection key_valid as same.
        exact (eq_sym same). }
      subst reduced_key.
      unfold oracle_keyed_result in evaluated.
      settle_option_equation evaluated.
      injection evaluated as same_summary. subst summary.
      unfold new_map_child_view.
      cbn [Core.source_summary_value].
      split; [rewrite raw_payload_is, kind; reflexivity|].
      unfold target_keyed_answer in target.
      destruct (Source.reduce_map_par par_child
        (Source.typed_map_value_source entry)) as [[value value_span]|];
        cbn in target; try discriminate target.
      destruct (Recursive.recursive_auxiliary_semantic
        (Recursive.RecursiveStringMapResult raw
          (Source.TypedMapReturn Semantic.semantic_summary key key_span value
            value_span))) as [semantic|];
        cbn in target; [|discriminate target].
      injection target as same_result. subst result.
      reflexivity.
    + (* PathMap *)
      unfold Oracle.source_evaluate_node in evaluated.
      destruct (Source.raw_field_of_source bytes
        (Source.SourceAuxiliary span field)) as [raw|] eqn:converted;
        [|discriminate evaluated].
      pose proof (Source.raw_field_of_source_preserves_wire_and_body bytes
        (Source.SourceAuxiliary span field) raw converted)
        as [_ [_ [raw_payload_is _]]].
      cbn [Source.source_field] in raw_payload_is.
      rewrite kind in evaluated.
      settle_option_equation evaluated.
      injection evaluated as same_summary. subst summary.
      cbn [Recursive.recursive_source_reference] in target.
      unfold Recursive.recursive_auxiliary_reference in target.
      rewrite (RegionChild.decoded_node_checked_raw_agrees fuel bytes program
        start finish nodes (Source.SourceAuxiliary span field) decoded member)
        in target.
      rewrite converted in target.
      rewrite kind in target.
      cbv beta iota zeta in target.
      rewrite Nat.eqb_refl in target.
      cbv beta iota zeta in target.
      destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
        (Source.source_body bytes (Source.SourceAuxiliary span field)))
        as [[target_snapshot target_trie]|]; [|discriminate target].
      destruct (Source.sequence_source_results
        (map par_child (EPM1.value_bodies target_snapshot)))
        as [target_values|]; cbn [option_map] in target;
        [|discriminate target].
      cbv beta iota in target.
      destruct (Recursive.recursive_auxiliary_semantic
        (Recursive.RecursivePathMapResult raw target_snapshot target_trie
          target_values)) as [semantic|]; cbn [option_map] in target;
        [|discriminate target].
      injection target as same_result. subst result.
      unfold new_map_child_view.
      cbn [Core.source_summary_value Core.source_value_raw].
      split; [rewrite raw_payload_is, kind; discriminate|exact I].
Qed.

(** * Target unfolding of one message node *)

Definition target_map_ordered (type_id : nat)
    (results : list Recursive.recursive_mixed_result) : bool :=
  if type_id =? 20
  then Recursive.ordered_recursive_new_map_returnsb None
    (Recursive.recursive_new_map_returns results)
  else true.

Definition target_after_children (raw : Schema.raw_field) (type_id : nat)
    (results : list Recursive.recursive_mixed_result)
    : option Recursive.recursive_mixed_result :=
  if target_map_ordered type_id results then
    let semantic := Semantic.reduce_semantic_field raw
      (map Recursive.recursive_result_semantic results) in
    if Semantic.sem_definitely_refused semantic then None
    else Some (Recursive.RecursiveMessageResult raw semantic results)
  else None.

Definition target_message_result
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (raw : Schema.raw_field) (type_id : nat)
    (child_program : Schema.generated_program)
    (children : list Source.source_tree)
    : option Recursive.recursive_mixed_result :=
  match Recursive.recursive_source_children par_child hash_fn bytes
      child_program children with
  | None => None
  | Some results => target_after_children raw type_id results
  end.

Lemma recursive_message_reference_unfold :
  forall par_child hash_fn bytes program span field children raw type_id
      child_program,
    Recursive.checked_generated_raw bytes program
      (Source.SourceMessage span field children) = Some raw ->
    Schema.descriptor_child_type_id field = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some child_program ->
    Recursive.recursive_source_reference par_child hash_fn bytes program
      (Source.SourceMessage span field children) =
    target_message_result par_child hash_fn bytes raw type_id child_program
      children.
Proof.
  intros par_child hash_fn bytes program span field children raw type_id
    child_program checked child_type looked_up.
  cbn [Recursive.recursive_source_reference].
  destruct (Recursive.checked_generated_raw bytes program
    (Source.SourceMessage span field children)) as [checked_raw|];
    [|discriminate checked].
  injection checked as same_raw. subst checked_raw.
  destruct (Schema.descriptor_child_type_id field) as [child_type_id|];
    [|discriminate child_type].
  injection child_type as same_type. subst child_type_id.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [looked_up_program|]; [|discriminate looked_up].
  injection looked_up as same_program. subst looked_up_program.
  rewrite Recursive.local_recursive_children_are_global_children.
  unfold target_message_result, target_after_children, target_map_ordered.
  reflexivity.
Qed.

Lemma source_message_summary_refuses_unordered_keys :
  forall raw program children,
    Oracle.source_string_map_keys_ordered children = false ->
    Core.source_summary_refused
      (Oracle.source_message_summary raw program children) = true.
Proof.
  intros raw program children unordered.
  unfold Core.source_summary_refused, Oracle.source_message_summary.
  cbn [Core.source_summary_analysis Core.source_family_refused].
  rewrite unordered.
  cbn [negb].
  rewrite orb_true_r.
  reflexivity.
Qed.

(** The target's reduction of a resolved message refuses an invalid scan. *)
Lemma reduce_semantic_field_refuses_invalid_target_scan :
  forall raw type_id program children,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Schema.scan_valid
      (fold_left (Target.scan_child program) children Schema.empty_scan) = false ->
    Target.sem_definitely_refused (Target.reduce_semantic_field raw children) = true.
Proof.
  intros raw type_id program children payload family looked_up invalid.
  rewrite (reduce_semantic_field_resolved raw type_id program children payload
    family looked_up
    (generated_program_has_requested_family type_id program looked_up)).
  cbn [Target.sem_definitely_refused].
  rewrite invalid.
  cbn [negb].
  rewrite orb_true_r.
  reflexivity.
Qed.

(** * Agreement of one message's children *)

Definition target_children_refuse (type_id : nat) (raw : Schema.raw_field)
    (target_children : option (list Recursive.recursive_mixed_result)) : Prop :=
  match target_children with
  | None => True
  | Some results =>
      target_map_ordered type_id results = false \/
      Semantic.sem_definitely_refused
        (Semantic.reduce_semantic_field raw
          (map Recursive.recursive_result_semantic results)) = true
  end.

Definition message_children_outcome
    (raw : Schema.raw_field) (type_id : nat)
    (child_program : Schema.generated_program)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (children : list Source.source_tree)
    (evaluate : Source.source_tree -> option Core.source_exact_summary) : Prop :=
  match Source.sequence_source_results (map evaluate children) with
  | None =>
      Recursive.recursive_source_children par_child hash_fn bytes
        child_program children = None
  | Some summaries =>
      let summary := Oracle.source_message_summary raw child_program summaries in
      (Core.source_summary_refused summary = true /\
       target_children_refuse type_id raw
         (Recursive.recursive_source_children par_child hash_fn bytes
           child_program children)) \/
      (Core.source_summary_refused summary = false /\
       exists results,
         Recursive.recursive_source_children par_child hash_fn bytes
           child_program children = Some results /\
         target_map_ordered type_id results = true /\
         Semantic.reduce_semantic_field raw
           (map Recursive.recursive_result_semantic results) =
         Projection.project_source_summary summary)
  end.

Lemma message_outcome_when_ordered :
  forall raw type_id child_program
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      hash_fn bytes children summaries results,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some child_program ->
    actual_unrefused_children child_program summaries ->
    (type_id = 20 -> Oracle.source_string_map_keys_ordered summaries = true) ->
    Recursive.recursive_source_children par_child hash_fn bytes child_program
      children = Some results ->
    map Recursive.recursive_result_semantic results =
      map Projection.project_source_summary summaries ->
    target_map_ordered type_id results = true ->
    (Core.source_summary_refused
       (Oracle.source_message_summary raw child_program summaries) = true /\
     target_children_refuse type_id raw
       (Recursive.recursive_source_children par_child hash_fn bytes
         child_program children)) \/
    (Core.source_summary_refused
       (Oracle.source_message_summary raw child_program summaries) = false /\
     exists results',
       Recursive.recursive_source_children par_child hash_fn bytes
         child_program children = Some results' /\
       target_map_ordered type_id results' = true /\
       Semantic.reduce_semantic_field raw
         (map Recursive.recursive_result_semantic results') =
       Projection.project_source_summary
         (Oracle.source_message_summary raw child_program summaries)).
Proof.
  intros raw type_id child_program par_child hash_fn bytes children summaries
    results payload family looked_up actual order_premise target_children
    same_semantics ordered.
  pose proof (source_message_summary_projects_to_reducer raw type_id
    child_program summaries payload family looked_up actual order_premise)
    as projects.
  rewrite target_children.
  destruct (Core.source_summary_refused
    (Oracle.source_message_summary raw child_program summaries)) eqn:refused.
  - left. split; [reflexivity|].
    unfold target_children_refuse.
    right.
    rewrite same_semantics.
    rewrite <- projects.
    exact refused.
  - right. split; [reflexivity|].
    exists results. split; [reflexivity|].
    split; [exact ordered|].
    rewrite same_semantics.
    symmetry. exact projects.
Qed.

Lemma message_children_agreement :
  forall raw type_id child_program
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      hash_fn bytes children
      (evaluate : Source.source_tree -> option Core.source_exact_summary),
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some child_program ->
    (forall node, In node children ->
      RecursiveProjection.source_target_child_verdict (evaluate node)
        (Recursive.recursive_source_reference par_child hash_fn bytes
          child_program node)) ->
    (forall summaries,
      Source.sequence_source_results (map evaluate children) = Some summaries ->
      Forall child_raw_is_faithful summaries) ->
    (forall summaries,
      Source.sequence_source_results (map evaluate children) = Some summaries ->
      Admission.source_scan_valid
        (Admission.source_scan_children child_program summaries) = true ->
      actual_unrefused_children child_program summaries) ->
    (forall summaries results,
      type_id = 20 ->
      Source.sequence_source_results (map evaluate children) = Some summaries ->
      Recursive.recursive_source_children par_child hash_fn bytes child_program
        children = Some results ->
      Admission.source_scan_valid
        (Admission.source_scan_children child_program summaries) = true ->
      Oracle.source_string_map_keys_ordered summaries =
      Recursive.ordered_recursive_new_map_returnsb None
        (Recursive.recursive_new_map_returns results)) ->
    message_children_outcome raw type_id child_program par_child hash_fn bytes
      children evaluate.
Proof.
  intros raw type_id child_program par_child hash_fn bytes children evaluate
    payload family looked_up verdicts faithful witness map_order.
  unfold message_children_outcome.
  destruct (Source.sequence_source_results (map evaluate children))
    as [summaries|] eqn:selected.
  - cbv beta iota zeta.
    destruct (Admission.source_scan_valid
      (Admission.source_scan_children child_program summaries)) eqn:scan.
    + (* the source scan is valid: all children are unrefused *)
      assert (unrefused : Forall (fun child =>
          Core.source_summary_refused child = false) summaries).
      { apply (valid_source_scan_children_are_unrefused child_program summaries
          Admission.source_empty_scan).
        exact scan. }
      destruct (RecursiveProjection.accepted_source_child_list_projects_to_recursive
        par_child hash_fn bytes child_program children evaluate summaries
        verdicts selected unrefused)
        as [results [target_children same_semantics]].
      (* [destruct ... eqn:selected] already rewrote the sequence in the local
         context, so these premises now read [Some summaries = Some _]. *)
      pose proof (witness summaries eq_refl scan) as actual.
      destruct (Nat.eqb type_id 20) eqn:is_new.
      * apply Nat.eqb_eq in is_new. subst type_id.
        pose proof (map_order summaries results eq_refl eq_refl target_children
          scan) as order_same.
        destruct (Recursive.ordered_recursive_new_map_returnsb None
          (Recursive.recursive_new_map_returns results)) eqn:ordered.
        -- (* [try assumption] would also close the [target_map_ordered] goal
              by conversion; the explicit goal selector keeps the shape fixed *)
           apply (message_outcome_when_ordered raw 20 child_program par_child
             hash_fn bytes children summaries results payload family looked_up
             actual); [|exact target_children|exact same_semantics|].
           ++ intros _. exact order_same.
           ++ unfold target_map_ordered. rewrite Nat.eqb_refl. exact ordered.
        -- left. split.
           ++ apply source_message_summary_refuses_unordered_keys.
              exact order_same.
           ++ rewrite target_children.
              unfold target_children_refuse.
              left.
              unfold target_map_ordered.
              rewrite Nat.eqb_refl.
              exact ordered.
      * apply (message_outcome_when_ordered raw type_id child_program par_child
          hash_fn bytes children summaries results payload family looked_up
          actual); [|exact target_children|exact same_semantics|].
        -- intros same_type.
           apply Nat.eqb_neq in is_new.
           contradiction.
        -- unfold target_map_ordered. rewrite is_new. reflexivity.
    + (* the source scan is invalid: both sides refuse *)
      left. split.
      * exact (Oracle.source_message_summary_refuses_invalid_scan raw
          child_program summaries scan).
      * destruct (Recursive.recursive_source_children par_child hash_fn bytes
          child_program children) as [results|] eqn:target_children; [|exact I].
        unfold target_children_refuse.
        right.
        destruct (RecursiveProjection.accepted_recursive_child_list_recovers_source
          par_child hash_fn bytes child_program children evaluate results
          verdicts target_children)
          as [summaries' [selected' [_ same_semantics]]].
        rewrite selected in selected'.
        injection selected' as same_summaries. subst summaries'.
        rewrite same_semantics.
        destruct (Schema.scan_valid
          (fold_left (Target.scan_child child_program)
            (map Projection.project_source_summary summaries)
            Schema.empty_scan)) eqn:target_scan.
        -- exfalso.
           pose proof (source_scan_children_valid_of_target_valid child_program
             summaries (faithful summaries eq_refl) target_scan) as valid.
           rewrite scan in valid.
           discriminate valid.
        -- exact (reduce_semantic_field_refuses_invalid_target_scan raw type_id
             child_program (map Projection.project_source_summary summaries)
             payload family looked_up target_scan).
  - cbv beta iota zeta.
    pose proof (RecursiveProjection.source_target_child_lists_project par_child
      hash_fn bytes child_program children evaluate verdicts) as projected.
    unfold RecursiveProjection.source_admitted_children in projected.
    destruct (Source.sequence_source_results
      (map RecursiveProjection.source_unrejected_child (map evaluate children)))
      as [unrejected|] eqn:unrejected_eq.
    + destruct (RecursiveProjection.source_unrejected_results_recover
        (map evaluate children) unrejected unrejected_eq) as [recovered _].
      rewrite selected in recovered.
      discriminate recovered.
    + cbn [option_map] in projected.
      destruct (Recursive.recursive_source_children par_child hash_fn bytes
        child_program children); [discriminate projected|reflexivity].
Qed.

Lemma target_after_children_none_of_refuse :
  forall raw type_id results,
    target_map_ordered type_id results = false \/
    Semantic.sem_definitely_refused
      (Semantic.reduce_semantic_field raw
        (map Recursive.recursive_result_semantic results)) = true ->
    target_after_children raw type_id results = None.
Proof.
  intros raw type_id results [unordered|refused].
  - unfold target_after_children. rewrite unordered. reflexivity.
  - unfold target_after_children.
    destruct (target_map_ordered type_id results); [|reflexivity].
    cbv zeta.
    rewrite refused.
    reflexivity.
Qed.

Lemma message_node_verdict_from_outcome :
  forall raw type_id child_program par_child hash_fn bytes children evaluate,
    message_children_outcome raw type_id child_program par_child hash_fn bytes
      children evaluate ->
    RecursiveProjection.source_target_child_verdict
      (option_map (Oracle.source_message_summary raw child_program)
        (Source.sequence_source_results (map evaluate children)))
      (target_message_result par_child hash_fn bytes raw type_id child_program
        children).
Proof.
  intros raw type_id child_program par_child hash_fn bytes children evaluate
    outcome.
  unfold message_children_outcome in outcome.
  unfold target_message_result.
  destruct (Source.sequence_source_results (map evaluate children))
    as [summaries|] eqn:selected.
  - cbv beta iota zeta in outcome.
    cbn [option_map].
    destruct outcome as [[refused refuse_target]|[unrefused [results
      [target_children [ordered projected]]]]].
    + unfold target_children_refuse in refuse_target.
      destruct (Recursive.recursive_source_children par_child hash_fn bytes
        child_program children) as [results|] eqn:target_children.
      * (* reduce the match on [Some results] so that the child list is visible
           to the rewrite *)
        cbv beta iota.
        rewrite (target_after_children_none_of_refuse raw type_id results
          refuse_target).
        exact refused.
      * exact refused.
    + rewrite target_children.
      unfold target_after_children.
      rewrite ordered.
      cbv beta iota zeta.
      rewrite projected.
      assert (not_refused : Semantic.sem_definitely_refused
        (Projection.project_source_summary
          (Oracle.source_message_summary raw child_program summaries)) = false)
        by exact unrefused.
      rewrite not_refused.
      unfold RecursiveProjection.source_target_child_verdict.
      split; [exact unrefused|reflexivity].
  - cbv beta iota zeta in outcome.
    rewrite outcome.
    exact I.
Qed.

(** * The saturated oracle on one message node *)

Lemma saturated_oracle_retains_raw :
  forall hash_fn raw evaluated,
    saturated_oracle hash_fn raw = Some evaluated ->
    Core.source_summary_raw evaluated = raw.
Proof.
  intros hash_fn raw evaluated accepted.
  unfold saturated_oracle in accepted.
  exact (RecursiveProjection.source_evaluate_message_retains_raw _ hash_fn raw
    evaluated accepted).
Qed.

Lemma saturated_oracle_has_message_shape :
  forall hash_fn raw evaluated,
    saturated_oracle hash_fn raw = Some evaluated ->
    exists children,
      Core.source_summary_value evaluated =
        Core.TypedSourceMessage raw children /\
      Schema.raw_payload raw = Schema.Message.
Proof.
  intros hash_fn raw evaluated accepted.
  unfold saturated_oracle in accepted.
  exact (accepted_independent_message_has_typed_value _ hash_fn raw evaluated
    accepted).
Qed.

(** Everything the node theorem needs about one decoded message node. *)
Lemma message_node_saturated_summary :
  forall hash_fn fuel bytes program start finish nodes span field children raw,
    finish - start <= fuel ->
    finish <= length bytes ->
    Source.decode_region (S fuel) bytes program start finish = Some nodes ->
    In (Source.SourceMessage span field children) nodes ->
    Source.raw_field_of_source bytes
      (Source.SourceMessage span field children) = Some raw ->
    exists type_id child_program,
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some type_id /\
      Schema.lookup_projected_program type_id [] [] = Some child_program /\
      Schema.descriptor_child_type_id field = Some type_id /\
      Source.decode_region fuel bytes child_program
        (Source.Wire.span_payload_start span) (Source.Wire.span_end span) =
        Some children /\
      Source.Wire.span_end span <= finish /\
      start < Source.Wire.span_payload_start span /\
      Source.Wire.span_payload_start span <= Source.Wire.span_end span /\
      saturated_oracle hash_fn raw =
        option_map (Oracle.source_message_summary raw child_program)
          (Source.sequence_source_results
            (map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes child_program) children)) /\
      (forall summaries,
        Source.sequence_source_results
          (map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
            bytes child_program) children) = Some summaries ->
        Admission.source_scan_valid
          (Admission.source_scan_children child_program summaries) = true ->
        actual_unrefused_children child_program summaries).
Proof.
  intros hash_fn fuel bytes program start finish nodes span field children raw
    region_fits finish_bound decoded member converted.
  assert (payload : Schema.raw_payload raw = Schema.Message).
  { unfold Source.raw_field_of_source in converted.
    cbn [Source.source_span Source.source_field] in converted.
    destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
      cbn [Schema.payload_eqb] in converted; try discriminate converted.
    destruct (Schema.descriptor_child_type_id field);
      cbn [option_map] in converted; [|discriminate converted].
    injection converted as same.
    rewrite <- same.
    reflexivity. }
  destruct (RegionChild.decoded_message_body_has_exact_rebased_children fuel
    bytes program start finish nodes span field children raw region_fits
    finish_bound decoded member converted)
    as [type_id [child_program [raw_type [looked_up rebased_decoded]]]].
  pose proof (BodyDecode.decoded_region_message_children_from_actual_decoder
    fuel bytes program start finish nodes decoded) as provenance.
  rewrite Forall_forall in provenance.
  specialize (provenance _ member).
  cbn [BodyDecode.node_children_from_decoder] in provenance.
  destruct provenance as [provenance_type [provenance_program
    [provenance_child_type [provenance_lookup children_decoded]]]].
  pose proof (Source.raw_message_field_uses_generated_child_type bytes span
    field children raw converted) as raw_child_type.
  rewrite raw_type in raw_child_type.
  rewrite <- raw_child_type in provenance_child_type.
  injection provenance_child_type as same_type.
  subst provenance_type.
  rewrite looked_up in provenance_lookup.
  injection provenance_lookup as same_program.
  subst provenance_program.
  pose proof (Source.raw_field_of_source_preserves_wire_and_body bytes
    (Source.SourceMessage span field children) raw converted)
    as [_ [_ [_ [body_is _]]]].
  destruct (Source.decoded_region_nodes_have_checked_source_windows (S fuel)
    bytes program start finish nodes decoded) as [_ parent_windows].
  rewrite Forall_forall in parent_windows.
  specialize (parent_windows _ member).
  unfold Source.source_node_inside_region in parent_windows.
  cbn [Source.source_span] in parent_windows.
  destruct parent_windows as [start_before [payload_ordered end_within]].
  destruct (Source.decoded_region_nodes_have_checked_source_windows fuel
    bytes child_program (Source.Wire.span_payload_start span)
    (Source.Wire.span_end span) children children_decoded)
    as [child_finish_bound child_windows].
  exists type_id, child_program.
  set (rebased := map (Rebase.rebase_tree (Source.Wire.span_payload_start span))
    children) in *.
  assert (body_slice : Schema.raw_body raw =
    Rebase.region_slice bytes (Source.Wire.span_payload_start span)
      (Source.Wire.span_end span)).
  { rewrite body_is. reflexivity. }
  assert (evaluation_chain :
    map (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message (length (Schema.raw_body raw)) hash_fn)
      hash_fn (Schema.raw_body raw) child_program) rebased =
    map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
      child_program) children).
  { transitivity (map (Oracle.source_evaluate_node (saturated_oracle hash_fn)
      hash_fn (Schema.raw_body raw) child_program) rebased).
    - apply map_ext_in.
      intros node node_member.
      apply (source_evaluate_node_callback_ext
        (Oracle.source_evaluate_message (length (Schema.raw_body raw)) hash_fn)
        (saturated_oracle hash_fn) hash_fn (Schema.raw_body raw) child_program
        (S (length (Schema.raw_body raw))) rebased node);
        [|exact rebased_decoded|exact node_member].
      intros child_raw shorter.
      apply source_evaluate_message_beyond_body_length.
      exact shorter.
    - rewrite body_slice.
      unfold rebased.
      apply (TypedRebase.source_node_evaluations_rebase_pointwise
        (saturated_oracle hash_fn) hash_fn bytes child_program
        (Source.Wire.span_payload_start span) (Source.Wire.span_end span)
        children); [|lia].
      rewrite Forall_forall in child_windows |- *.
      intros node node_member.
      specialize (child_windows node node_member).
      unfold Source.source_node_inside_region in child_windows.
      cbv beta.
      lia. }
  assert (saturated_equation : saturated_oracle hash_fn raw =
    option_map (Oracle.source_message_summary raw child_program)
      (Source.sequence_source_results
        (map (Oracle.source_evaluate_node
          (Oracle.source_evaluate_message (length (Schema.raw_body raw)) hash_fn)
          hash_fn (Schema.raw_body raw) child_program) rebased))).
  { unfold saturated_oracle.
    rewrite Oracle.source_evaluate_message_unfold.
    exact (Oracle.source_evaluate_message_step_selected_children _ hash_fn raw
      type_id child_program rebased payload raw_type looked_up rebased_decoded). }
  (* the explicit [conj] keeps the ten components and their order visible *)
  refine (conj payload (conj raw_type (conj looked_up (conj _
    (conj children_decoded (conj end_within (conj start_before
      (conj payload_ordered (conj _ _))))))))).
  - rewrite <- raw_child_type. reflexivity.
  - rewrite saturated_equation, evaluation_chain. reflexivity.
  - intros summaries selected valid.
    unfold actual_unrefused_children.
    exists (length (Schema.raw_body raw)), hash_fn, (Schema.raw_body raw), rebased.
    split; [exact rebased_decoded|].
    split; [|exact valid].
    rewrite evaluation_chain.
    exact selected.
Qed.

(** An auxiliary node whose descriptor payload is neither map form has no raw
    field, so both sides produce nothing. *)
Lemma auxiliary_node_plain_payload_verdict :
  forall (recurse : Schema.raw_field -> option Core.source_exact_summary)
      (par_child : list nat -> option (bool * Semantic.semantic_summary))
      hash_fn bytes program fuel start finish nodes span field,
    Source.decode_region fuel bytes program start finish = Some nodes ->
    In (Source.SourceAuxiliary span field) nodes ->
    Schema.descriptor_payload field = Schema.Scalar \/
    Schema.descriptor_payload field = Schema.Message ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node recurse hash_fn bytes program
        (Source.SourceAuxiliary span field))
      (Recursive.recursive_source_reference par_child hash_fn bytes program
        (Source.SourceAuxiliary span field)).
Proof.
  intros recurse par_child hash_fn bytes program fuel start finish nodes span
    field decoded member kind.
  assert (no_raw : Source.raw_field_of_source bytes
      (Source.SourceAuxiliary span field) = None).
  { unfold Source.raw_field_of_source.
    cbn [Source.source_span Source.source_field].
    destruct kind as [kind|kind]; rewrite kind; reflexivity. }
  assert (oracle_none : Oracle.source_evaluate_node recurse hash_fn bytes
      program (Source.SourceAuxiliary span field) = None).
  { unfold Oracle.source_evaluate_node. rewrite no_raw. reflexivity. }
  assert (target_none : Recursive.recursive_source_reference par_child hash_fn
      bytes program (Source.SourceAuxiliary span field) = None).
  { cbn [Recursive.recursive_source_reference].
    unfold Recursive.recursive_auxiliary_reference.
    rewrite (RegionChild.decoded_node_checked_raw_agrees fuel bytes program
      start finish nodes (Source.SourceAuxiliary span field) decoded member),
      no_raw.
    reflexivity. }
  rewrite oracle_none, target_none.
  exact I.
Qed.

(** The inner induction, on decode fuel: every node of a decoded region of
    [bytes] has the same verdict on both sides, given the Par theorem for every
    strictly shorter body. *)
Lemma decoded_node_verdict_by_fuel :
  forall hash_fn bytes
      (par_child : list nat -> option (bool * Semantic.semantic_summary)),
    (forall value, length value < length bytes ->
      par_child value = Recursive.recursive_par_reference hash_fn value) ->
    (forall value, length value < length bytes ->
      Recursive.recursive_par_reference hash_fn value =
      option_map ParAnswer.project_par_answer
        (ParAnswer.oracle_scored_par_reference hash_fn value)) ->
    forall fuel family program start finish nodes,
      Schema.lookup_projected_program family [] [] = Some program ->
      finish - start <= fuel ->
      finish <= length bytes ->
      Source.decode_region (S fuel) bytes program start finish = Some nodes ->
      forall node, In node nodes ->
        RecursiveProjection.source_target_child_verdict
          (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
            program node)
          (Recursive.recursive_source_reference par_child hash_fn bytes program
            node).
Proof.
  intros hash_fn bytes par_child par_child_is_reference par_induction.
  induction fuel as [|fuel IH]; intros family program start finish nodes
    looked_up region_fits finish_bound decoded node member.
  - (* a region of width zero holds no node *)
    exfalso.
    destruct (Source.decoded_region_nodes_have_checked_source_windows 1 bytes
      program start finish nodes decoded) as [_ windows].
    rewrite Forall_forall in windows.
    specialize (windows node member).
    unfold Source.source_node_inside_region in windows.
    lia.
  - destruct node as [span field|span field children|span field].
    + exact (generated_scalar_node_verdict (saturated_oracle hash_fn) par_child
        hash_fn bytes family program (S (S fuel)) start finish nodes span field
        looked_up decoded member).
    + destruct (Source.raw_field_of_source bytes
        (Source.SourceMessage span field children)) as [raw|] eqn:converted.
      * destruct (message_node_saturated_summary hash_fn (S fuel) bytes program
          start finish nodes span field children raw region_fits finish_bound
          decoded member converted)
          as [type_id [child_program [payload [raw_type [child_lookup
            [child_type [children_decoded [end_within [start_before
              [payload_ordered [saturated_equation witness]]]]]]]]]]].
        assert (child_verdicts : forall node, In node children ->
          RecursiveProjection.source_target_child_verdict
            (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes child_program node)
            (Recursive.recursive_source_reference par_child hash_fn bytes
              child_program node)).
        { intros child_node child_member.
          apply (IH type_id child_program
            (Source.Wire.span_payload_start span) (Source.Wire.span_end span)
            children child_lookup); [lia|lia|exact children_decoded|exact child_member]. }
        assert (faithful : forall summaries,
          Source.sequence_source_results
            (map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes child_program) children) = Some summaries ->
          Forall child_raw_is_faithful summaries).
        { intros summaries selected.
          apply (sequenced_results_satisfy Source.source_tree
            Core.source_exact_summary
            (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes child_program) child_raw_is_faithful children summaries
            selected).
          intros child_node summary child_member evaluated.
          exact (decoded_node_child_is_faithful (saturated_oracle hash_fn)
            hash_fn bytes child_program (S fuel)
            (Source.Wire.span_payload_start span) (Source.Wire.span_end span)
            children child_node summary
            (saturated_oracle_retains_raw hash_fn) children_decoded child_member
            evaluated). }
        assert (map_order : forall summaries results,
          type_id = 20 ->
          Source.sequence_source_results
            (map (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes child_program) children) = Some summaries ->
          Recursive.recursive_source_children par_child hash_fn bytes
            child_program children = Some results ->
          Admission.source_scan_valid
            (Admission.source_scan_children child_program summaries) = true ->
          Oracle.source_string_map_keys_ordered summaries =
          Recursive.ordered_recursive_new_map_returnsb None
            (Recursive.recursive_new_map_returns results)).
        { intros summaries results is_new selected target_children scan.
          subst type_id.
          assert (same_program : child_program = Source.generated_new_program).
          { rewrite unordered_injections_lookup in child_lookup.
            injection child_lookup as same. exact (eq_sym same). }
          subst child_program.
          apply (new_order_bridge summaries results); [|exact scan].
          pose proof (RecursiveProjection.selected_source_evaluator_outputs_are_positional
            Source.source_tree Core.source_exact_summary
            (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes Source.generated_new_program) children summaries selected)
            as source_positional.
          pose proof (Recursive.accepted_recursive_children_preserve_source_order
            par_child hash_fn bytes Source.generated_new_program children
            results target_children) as target_positional.
          exact (positional_results_zip Source.source_tree
            Core.source_exact_summary Recursive.recursive_mixed_result
            (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn
              bytes Source.generated_new_program)
            (Recursive.recursive_source_reference par_child hash_fn bytes
              Source.generated_new_program)
            new_map_child_view children summaries results
            source_positional target_positional
            (fun child_node summary result child_member evaluated targeted =>
              new_map_child_view_of_node (saturated_oracle hash_fn) par_child
                hash_fn bytes Source.generated_new_program (S fuel)
                (Source.Wire.span_payload_start span)
                (Source.Wire.span_end span) children child_node summary result
                (saturated_oracle_has_message_shape hash_fn)
                children_decoded child_member evaluated targeted)). }
        pose proof (message_children_agreement raw type_id child_program
          par_child hash_fn bytes children
          (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
            child_program) payload raw_type child_lookup child_verdicts faithful
          witness map_order) as outcome.
        assert (oracle_node : Oracle.source_evaluate_node
            (saturated_oracle hash_fn) hash_fn bytes program
            (Source.SourceMessage span field children) =
            saturated_oracle hash_fn raw).
        { unfold Oracle.source_evaluate_node. rewrite converted. reflexivity. }
        assert (checked_raw : Recursive.checked_generated_raw bytes program
            (Source.SourceMessage span field children) = Some raw).
        { rewrite (RegionChild.decoded_node_checked_raw_agrees (S (S fuel)) bytes
            program start finish nodes
            (Source.SourceMessage span field children) decoded member).
          exact converted. }
        rewrite oracle_node, saturated_equation.
        rewrite (recursive_message_reference_unfold par_child hash_fn bytes
          program span field children raw type_id child_program checked_raw
          child_type child_lookup).
        exact (message_node_verdict_from_outcome raw type_id child_program
          par_child hash_fn bytes children
          (Oracle.source_evaluate_node (saturated_oracle hash_fn) hash_fn bytes
            child_program) outcome).
      * (* no raw field: both sides produce nothing *)
        assert (oracle_none : Oracle.source_evaluate_node
            (saturated_oracle hash_fn) hash_fn bytes program
            (Source.SourceMessage span field children) = None).
        { unfold Oracle.source_evaluate_node. rewrite converted. reflexivity. }
        assert (target_none : Recursive.recursive_source_reference par_child
            hash_fn bytes program (Source.SourceMessage span field children) =
            None).
        { cbn [Recursive.recursive_source_reference].
          rewrite (RegionChild.decoded_node_checked_raw_agrees (S (S fuel))
            bytes program start finish nodes
            (Source.SourceMessage span field children) decoded member).
          rewrite converted.
          reflexivity. }
        rewrite oracle_none, target_none.
        exact I.
    + destruct (Schema.descriptor_payload field) eqn:payload_kind.
      * exact (auxiliary_node_plain_payload_verdict (saturated_oracle hash_fn)
          par_child hash_fn bytes program (S (S fuel)) start finish nodes span
          field decoded member (or_introl payload_kind)).
      * exact (auxiliary_node_plain_payload_verdict (saturated_oracle hash_fn)
          par_child hash_fn bytes program (S (S fuel)) start finish nodes span
          field decoded member (or_intror payload_kind)).
      * exact (generated_string_map_node_verdict (saturated_oracle hash_fn)
          par_child hash_fn bytes program (S (S fuel)) start finish nodes span
          field (fun value _ => eq_refl) par_child_is_reference par_induction
          decoded member payload_kind).
      * exact (generated_pathmap_node_verdict (saturated_oracle hash_fn)
          par_child hash_fn bytes program (S (S fuel)) start finish nodes span
          field (fun value _ => eq_refl) par_child_is_reference par_induction
          decoded member payload_kind).
Qed.

(** * The Par root *)

(** The Par-specific part of the root comparison: the target's scored answer
    against the projection of the oracle's scored answer. *)
Lemma par_root_answers_agree :
  forall (body : list nat) (nodes : list Source.source_tree)
      (raw : Schema.raw_field) (summaries : list Core.source_exact_summary)
      (results : list Recursive.recursive_mixed_result),
    Schema.raw_program_type_id raw = Some 1 ->
    Forall2 (RecursiveProjection.par_default_child_decoded body) nodes summaries ->
    Core.source_summary_refused
      (Oracle.source_message_summary raw Source.generated_par_source_program
        summaries) = false ->
    Semantic.reduce_semantic_field raw
      (map Recursive.recursive_result_semantic results) =
    Projection.project_source_summary
      (Oracle.source_message_summary raw Source.generated_par_source_program
        summaries) ->
    match Some results, Source.source_par_semantic_default body nodes with
    | Some children, Some semantic_default =>
        let semantic := Semantic.reduce_semantic_field raw
          (map Recursive.recursive_result_semantic children) in
        if Semantic.sem_definitely_refused semantic then None
        else match Semantic.sem_score_evidence semantic with
             | Semantic.ExactScore _ => Some (semantic_default, semantic)
             | Semantic.ScoreNotDerived => None
             end
    | _, _ => None
    end =
    option_map ParAnswer.project_par_answer
      (ParAnswer.source_scored_par_result
        (Oracle.source_message_summary raw Source.generated_par_source_program
          summaries)).
Proof.
  intros body nodes raw summaries results raw_type aligned unrefused semantics.
  assert (default_agrees :
    Core.source_summary_semantic_default
      (Oracle.source_message_summary raw Source.generated_par_source_program
        summaries) =
    Source.source_par_semantic_default body nodes).
  { unfold Oracle.source_message_summary.
    change (Schema.program_family Source.generated_par_source_program) with 1.
    cbn [Core.source_summary_semantic_default Nat.eqb].
    apply RecursiveProjection.positional_par_root_default_agrees;
      [exact raw_type|exact aligned]. }
  unfold ParAnswer.source_scored_par_result.
  rewrite unrefused.
  rewrite default_agrees.
  destruct (Source.source_par_semantic_default body nodes)
    as [semantic_default|]; cbn beta iota zeta.
  - rewrite semantics.
    assert (not_refused : Semantic.sem_definitely_refused
      (Projection.project_source_summary
        (Oracle.source_message_summary raw Source.generated_par_source_program
          summaries)) = false) by exact unrefused.
    rewrite not_refused.
    change (Semantic.sem_score_evidence
      (Projection.project_source_summary
        (Oracle.source_message_summary raw Source.generated_par_source_program
          summaries)))
      with (Projection.project_source_evidence
        (Core.source_summary_exact_score
          (Oracle.source_message_summary raw Source.generated_par_source_program
            summaries))).
    destruct (Core.source_summary_exact_score
      (Oracle.source_message_summary raw Source.generated_par_source_program
        summaries)) as [score|]; reflexivity.
  - reflexivity.
Qed.

Lemma source_evaluate_message_step_undecoded :
  forall recurse hash_fn raw type_id program,
    Schema.raw_payload raw = Schema.Message ->
    Schema.raw_program_type_id raw = Some type_id ->
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) = None ->
    Oracle.source_evaluate_message_step recurse hash_fn raw = None.
Proof.
  intros recurse hash_fn raw type_id program payload family looked_up decoded.
  unfold Oracle.source_evaluate_message_step.
  rewrite payload, family, looked_up.
  cbv beta iota zeta.
  rewrite decoded.
  reflexivity.
Qed.

Lemma par_reference_matches_oracle_step :
  forall hash_fn body,
    (forall value, length value < length body ->
      Recursive.recursive_par_reference hash_fn value =
      option_map ParAnswer.project_par_answer
        (ParAnswer.oracle_scored_par_reference hash_fn value)) ->
    Recursive.recursive_par_reference hash_fn body =
    option_map ParAnswer.project_par_answer
      (ParAnswer.oracle_scored_par_reference hash_fn body).
Proof.
  intros hash_fn body par_induction.
  rewrite recursive_par_reference_unfold.
  unfold Recursive.recursive_par_step, ParAnswer.oracle_scored_par_reference.
  rewrite Oracle.source_evaluate_message_unfold.
  rewrite Source.generated_par_source_lookup_is_present.
  cbv beta iota zeta.
  unfold Source.decode_root.
  rewrite Source.generated_par_source_lookup_is_present.
  cbv beta iota zeta.
  destruct (Source.decode_region (S (length body)) body
    Source.generated_par_source_program 0 (length body)) as [nodes|] eqn:decoded.
  2:{ (* the body does not decode: both sides produce nothing *)
    rewrite (source_evaluate_message_step_undecoded
      (Oracle.source_evaluate_message (length body) hash_fn) hash_fn
      (Oracle.source_par_raw body) 1 Source.generated_par_source_program
      eq_refl eq_refl Source.generated_par_source_lookup_is_present decoded).
    reflexivity. }
  (* Unfolding the step by [cbn] would expose the raw field in the summary
     as well; the selected-children equation keeps it folded. *)
  rewrite (Oracle.source_evaluate_message_step_selected_children
    (Oracle.source_evaluate_message (length body) hash_fn) hash_fn
    (Oracle.source_par_raw body) 1 Source.generated_par_source_program nodes
    eq_refl eq_refl Source.generated_par_source_lookup_is_present decoded).
  change (Schema.raw_body (Oracle.source_par_raw body)) with body.
  change (Recursive.par_raw body) with (Oracle.source_par_raw body).
  cbv beta iota zeta.
  set (nested_par := Recursive.guarded_recursive_par_child body
    (fun child _ => Recursive.recursive_par_reference hash_fn child)).
  assert (nested_par_is_reference : forall value, length value < length body ->
    nested_par value = Recursive.recursive_par_reference hash_fn value).
  { intros value shorter.
    unfold nested_par, Recursive.guarded_recursive_par_child.
    destruct (lt_dec (length value) (length body)) as [_|not_shorter];
      [reflexivity|contradiction]. }
  set (evaluate := Oracle.source_evaluate_node (saturated_oracle hash_fn)
    hash_fn body Source.generated_par_source_program).
  set (raw := Oracle.source_par_raw body).
  assert (verdicts : forall node, In node nodes ->
    RecursiveProjection.source_target_child_verdict (evaluate node)
      (Recursive.recursive_source_reference nested_par hash_fn body
        Source.generated_par_source_program node)).
  { intros node member.
    unfold evaluate.
    apply (decoded_node_verdict_by_fuel hash_fn body nested_par
      nested_par_is_reference par_induction (length body) 1
      Source.generated_par_source_program 0 (length body) nodes
      Source.generated_par_source_lookup_is_present); [lia|lia|exact decoded|exact member]. }
  assert (fueled_sequence : Source.sequence_source_results
    (map (Oracle.source_evaluate_node
      (Oracle.source_evaluate_message (length body) hash_fn) hash_fn body
      Source.generated_par_source_program) nodes) =
    Source.sequence_source_results (map evaluate nodes)).
  { apply source_sequence_agrees_on_members.
    intros node member.
    unfold evaluate.
    apply (source_evaluate_node_callback_ext
      (Oracle.source_evaluate_message (length body) hash_fn)
      (saturated_oracle hash_fn) hash_fn body
      Source.generated_par_source_program (S (length body)) nodes node);
      [|exact decoded|exact member].
    intros child_raw shorter.
    apply source_evaluate_message_beyond_body_length.
    exact shorter. }
  assert (faithful : forall summaries,
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Forall child_raw_is_faithful summaries).
  { intros summaries selected.
    apply (sequenced_results_satisfy Source.source_tree
      Core.source_exact_summary evaluate child_raw_is_faithful nodes summaries
      selected).
    intros node summary member evaluated.
    exact (decoded_node_child_is_faithful (saturated_oracle hash_fn) hash_fn
      body Source.generated_par_source_program (S (length body)) 0 (length body)
      nodes node summary (saturated_oracle_retains_raw hash_fn) decoded member
      evaluated). }
  assert (witness : forall summaries,
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Admission.source_scan_valid
      (Admission.source_scan_children Source.generated_par_source_program
        summaries) = true ->
    actual_unrefused_children Source.generated_par_source_program summaries).
  { intros summaries selected valid.
    unfold actual_unrefused_children.
    exists (length body), hash_fn, body, nodes.
    split; [exact decoded|].
    split; [|exact valid].
    rewrite fueled_sequence.
    exact selected. }
  assert (map_order : forall summaries results,
    1 = 20 ->
    Source.sequence_source_results (map evaluate nodes) = Some summaries ->
    Recursive.recursive_source_children nested_par hash_fn body
      Source.generated_par_source_program nodes = Some results ->
    Admission.source_scan_valid
      (Admission.source_scan_children Source.generated_par_source_program
        summaries) = true ->
    Oracle.source_string_map_keys_ordered summaries =
    Recursive.ordered_recursive_new_map_returnsb None
      (Recursive.recursive_new_map_returns results)).
  { intros summaries results impossible. discriminate impossible. }
  pose proof (message_children_agreement raw 1
    Source.generated_par_source_program nested_par hash_fn body nodes evaluate
    eq_refl eq_refl Source.generated_par_source_lookup_is_present verdicts
    faithful witness map_order) as outcome.
  unfold message_children_outcome in outcome.
  rewrite fueled_sequence.
  destruct (Source.sequence_source_results (map evaluate nodes))
    as [summaries|] eqn:selected.
  - cbv beta iota zeta in outcome.
    cbn [option_map].
    destruct outcome as [[refused refuse_target]|[unrefused [results
      [target_children [ordered projected]]]]].
    + (* the oracle's root is refused and so is the target's *)
      unfold ParAnswer.source_scored_par_result.
      rewrite refused.
      cbn [option_map].
      unfold target_children_refuse in refuse_target.
      destruct (Recursive.recursive_source_children nested_par hash_fn body
        Source.generated_par_source_program nodes) as [results|];
        [|reflexivity].
      cbv beta iota in refuse_target.
      destruct refuse_target as [unordered|target_refused].
      * unfold target_map_ordered in unordered.
        cbn [Nat.eqb] in unordered.
        discriminate unordered.
      * destruct (Source.source_par_semantic_default body nodes) as [default|];
          [|reflexivity].
        cbv beta iota zeta.
        rewrite target_refused.
        reflexivity.
    + (* both sides accept and project the same summary.  After the
         [destruct ... eqn:selected] above, [fueled_sequence] already states
         that the fueled sequence selects [summaries]. *)
      rewrite target_children.
      assert (aligned : Forall2
        (RecursiveProjection.par_default_child_decoded body) nodes summaries).
      { exact (RecursiveProjection.selected_independent_par_children_are_positional
          (length body) hash_fn body nodes summaries decoded fueled_sequence). }
      exact (par_root_answers_agree body nodes raw summaries results eq_refl
        aligned unrefused projected).
  - (* an evaluator failure on both sides *)
    cbv beta iota zeta in outcome.
    cbn [option_map].
    rewrite outcome.
    reflexivity.
Qed.

(** * The two theorems *)

Theorem recursive_mixed_reference_matches_oracle :
  forall hash_fn body,
    Recursive.recursive_par_reference hash_fn body =
    option_map ParAnswer.project_par_answer
      (ParAnswer.oracle_scored_par_reference hash_fn body).
Proof.
  intros hash_fn.
  assert (bounded : forall size body, length body <= size ->
    Recursive.recursive_par_reference hash_fn body =
    option_map ParAnswer.project_par_answer
      (ParAnswer.oracle_scored_par_reference hash_fn body)).
  { induction size as [|size IH]; intros body body_bound;
      apply par_reference_matches_oracle_step; intros value shorter.
    - exfalso. lia.
    - apply IH. lia. }
  intros body.
  exact (bounded (length body) body (Nat.le_refl _)).
Qed.

Theorem recursive_node_matches_oracle :
  forall hash_fn bytes type_id program nodes node,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_root type_id bytes = Some nodes ->
    In node nodes ->
    RecursiveProjection.source_target_child_verdict
      (Oracle.source_evaluate_node
        (Oracle.source_evaluate_message (length bytes) hash_fn)
        hash_fn bytes program node)
      (Recursive.recursive_source_reference
        (Recursive.recursive_par_reference hash_fn)
        hash_fn bytes program node).
Proof.
  intros hash_fn bytes type_id program nodes node looked_up rooted member.
  assert (decoded : Source.decode_region (S (length bytes)) bytes program 0
      (length bytes) = Some nodes).
  { unfold Source.decode_root in rooted.
    rewrite looked_up in rooted.
    exact rooted. }
  rewrite (source_evaluate_node_callback_ext
    (Oracle.source_evaluate_message (length bytes) hash_fn)
    (saturated_oracle hash_fn) hash_fn bytes program (S (length bytes)) nodes
    node).
  - apply (decoded_node_verdict_by_fuel hash_fn bytes
      (Recursive.recursive_par_reference hash_fn) (fun value _ => eq_refl)
      (fun value _ => recursive_mixed_reference_matches_oracle hash_fn value)
      (length bytes) type_id program 0 (length bytes) nodes looked_up);
      [lia|lia|exact decoded|exact member].
  - intros child_raw shorter.
    apply source_evaluate_message_beyond_body_length.
    exact shorter.
  - exact decoded.
  - exact member.
Qed.

(** The root-list form: the unrejected oracle evaluation of all decoded root
    nodes and the recursive target's child list project to each other. *)
Corollary recursive_root_children_match_oracle :
  forall hash_fn bytes type_id program nodes,
    Schema.lookup_projected_program type_id [] [] = Some program ->
    Source.decode_root type_id bytes = Some nodes ->
    option_map (map Projection.project_source_summary)
      (RecursiveProjection.source_admitted_children
        (Oracle.source_evaluate_node
          (Oracle.source_evaluate_message (length bytes) hash_fn)
          hash_fn bytes program) nodes) =
    option_map (map Recursive.recursive_result_semantic)
      (Recursive.recursive_source_children
        (Recursive.recursive_par_reference hash_fn)
        hash_fn bytes program nodes).
Proof.
  intros hash_fn bytes type_id program nodes looked_up rooted.
  apply RecursiveProjection.source_target_child_lists_project.
  intros node member.
  exact (recursive_node_matches_oracle hash_fn bytes type_id program nodes node
    looked_up rooted member).
Qed.

(** * Non-vacuity of the first theorem *)

Definition theorem_left_side (body : list nat)
    : option (bool * Semantic.semantic_summary) :=
  Recursive.recursive_par_reference ParAnswer.constant_hash body.

Definition theorem_right_side (body : list nat)
    : option (bool * Semantic.semantic_summary) :=
  option_map ParAnswer.project_par_answer
    (ParAnswer.oracle_scored_par_reference ParAnswer.constant_hash body).

(** On each body both sides are [Some], with the recorded semantic default,
    and they are equal. *)
Example both_sides_accept_and_agree_on_empty_body :
  option_map fst (theorem_left_side []) = Some true /\
  option_map fst (theorem_right_side []) = Some true /\
  theorem_left_side [] = theorem_right_side [].
Proof. refine (conj _ (conj _ _)); vm_compute; reflexivity. Qed.

Example both_sides_accept_and_agree_on_connective_body :
  option_map fst (theorem_left_side [80; 1]) = Some false /\
  option_map fst (theorem_right_side [80; 1]) = Some false /\
  theorem_left_side [80; 1] = theorem_right_side [80; 1].
Proof. refine (conj _ (conj _ _)); vm_compute; reflexivity. Qed.

Example both_sides_accept_and_agree_on_pathmap_value_body :
  option_map fst (theorem_left_side
    (ParAnswer.pathmap_par_with_values [[80; 1]])) = Some false /\
  option_map fst (theorem_right_side
    (ParAnswer.pathmap_par_with_values [[80; 1]])) = Some false /\
  theorem_left_side (ParAnswer.pathmap_par_with_values [[80; 1]]) =
  theorem_right_side (ParAnswer.pathmap_par_with_values [[80; 1]]).
Proof. refine (conj _ (conj _ _)); vm_compute; reflexivity. Qed.

Example both_sides_accept_and_agree_on_nested_map_value_wire :
  option_map fst
    (theorem_left_side Recursive.nested_map_value_pathmap_par_wire) = Some false /\
  option_map fst
    (theorem_right_side Recursive.nested_map_value_pathmap_par_wire) = Some false /\
  theorem_left_side Recursive.nested_map_value_pathmap_par_wire =
  theorem_right_side Recursive.nested_map_value_pathmap_par_wire.
Proof. refine (conj _ (conj _ _)); vm_compute; reflexivity. Qed.

(** * Red control

    The mutant [bundle_omit_body_score_mutant] drops the bundle's body score.
    On the witness body, which is one bundle field holding an empty Par body and
    a set write flag, the children are read from the actual decoded witness and
    evaluated by the real oracle; the mutant's score and the real score
    differ. *)

Definition bundle_red_control_body : list nat := [90; 4; 10; 0; 16; 1].

Definition bundle_red_control_children
    : option (list Core.source_exact_summary) :=
  match Source.decode_root 1 bundle_red_control_body with
  | Some [Source.SourceMessage _ field children] =>
      match Schema.descriptor_child_type_id field with
      | Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | Some program =>
              Source.sequence_source_results
                (map (Oracle.source_evaluate_node
                  (saturated_oracle ParAnswer.constant_hash)
                  ParAnswer.constant_hash bundle_red_control_body program)
                  children)
          | None => None
          end
      | None => None
      end
  | _ => None
  end.

Example bundle_omit_body_score_mutant_differs_on_witness :
  match bundle_red_control_children with
  | Some children =>
      bundle_omit_body_score_mutant children <>
      Process.source_process_family_score 8 (Some true) children
  | None => False
  end.
Proof. vm_compute. intro same_score. discriminate same_score. Qed.

(** * Closure of every exported result *)

Print Assumptions source_evaluate_message_beyond_body_length.
Print Assumptions source_evaluate_node_callback_ext.
Print Assumptions payload_eqb_true.
Print Assumptions target_scan_child_accepted_guards.
Print Assumptions source_descriptor_guard_of_target_guards.
Print Assumptions target_scan_step_reflects_source_valid.
Print Assumptions source_scan_valid_of_target_scan_valid.
Print Assumptions source_scan_children_valid_of_target_valid.
Print Assumptions evaluated_node_retains_converted_raw.
Print Assumptions decoded_node_child_is_faithful.
Print Assumptions sequenced_results_satisfy.
Print Assumptions positional_results_zip.
Print Assumptions resolve_tag_some.
Print Assumptions new_program_string_map_descriptor.
Print Assumptions new_program_non_map_slot.
Print Assumptions source_scan_step_accepted_guards.
Print Assumptions source_child_guard_payload.
Print Assumptions source_scan_rest_valid_of_fold_valid.
Print Assumptions new_order_bridge_after.
Print Assumptions new_order_bridge.
Print Assumptions new_map_child_view_of_node.
Print Assumptions recursive_message_reference_unfold.
Print Assumptions source_message_summary_refuses_unordered_keys.
Print Assumptions reduce_semantic_field_refuses_invalid_target_scan.
Print Assumptions message_outcome_when_ordered.
Print Assumptions message_children_agreement.
Print Assumptions target_after_children_none_of_refuse.
Print Assumptions message_node_verdict_from_outcome.
Print Assumptions saturated_oracle_retains_raw.
Print Assumptions saturated_oracle_has_message_shape.
Print Assumptions message_node_saturated_summary.
Print Assumptions auxiliary_node_plain_payload_verdict.
Print Assumptions decoded_node_verdict_by_fuel.
Print Assumptions par_root_answers_agree.
Print Assumptions source_evaluate_message_step_undecoded.
Print Assumptions par_reference_matches_oracle_step.
Print Assumptions recursive_mixed_reference_matches_oracle.
Print Assumptions recursive_node_matches_oracle.
Print Assumptions recursive_root_children_match_oracle.
Print Assumptions both_sides_accept_and_agree_on_empty_body.
Print Assumptions both_sides_accept_and_agree_on_connective_body.
Print Assumptions both_sides_accept_and_agree_on_pathmap_value_body.
Print Assumptions both_sides_accept_and_agree_on_nested_map_value_wire.
Print Assumptions bundle_omit_body_score_mutant_differs_on_witness.
