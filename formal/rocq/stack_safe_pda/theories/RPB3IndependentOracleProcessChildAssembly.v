(** * Assembly of the process-child correspondence on actual children

    [ProcessProjection.process_child_correspondence] bundles the equations
    between the target's child readers and the independent oracle's child
    readers that the process-family score proofs consume.  This theory proves
    the record on the children that the oracle's decoder and evaluator
    actually produce ([Correspondence.process_actual_children]).

    Most fields are corollaries of the per-reader theorems already proved:
    [RPB3IndependentOracleProcessChildCorrespondence] (the scalar-true and
    bool readers), [RPB3IndependentOracleProcessNumericChildren] (the i32
    reader and the [New] word), [RPB3IndependentOracleProcessTypedChildren]
    (the score readers) and [RPB3IndependentOracleProcessExprChildren] (the
    [Expr] arm).  The record guards the boolean and i32 fields by the
    slot functions of [ProcessProjection], which enumerate exactly the pairs
    [(family, tag)] the score functions read, so the only bridge needed is a
    case analysis showing that a slot function accepts a pair only if the
    pair is in the reader-slot list of the corresponding theorem.

    The new content is the three per-child atoms:

    - [process_actual_children_scalar_arm_corresponds]: a child of the [Expr]
      family has scalar payload exactly when its typed value is a scalar,
      because the evaluator builds a [TypedSourceScalar] value only for a
      scalar node.
    - [process_actual_children_receive_default_atom_corresponds]: the oracle's
      typed Par-default test on a [Receive] child agrees with the target's
      test on the child's body.
    - [process_actual_children_match_default_atom_corresponds]: the same for
      the guard of a [MatchCase] child of a [Match].

    The two default atoms hold for every actual child and need no extra
    premise.  An evaluated child at the [Receive] slot 8 or the [Match] slot 2
    comes from a message node whose evaluation ran the same
    [Source.decode_region] call that the target reader runs, at the same fuel
    [S (length body)], so the two sides read the same decoded nodes.  The
    oracle's typed default is then the source-owned default of those nodes
    ([RecursiveProjection.positional_par_root_default_agrees]).  The [MatchCase]
    guard is one level deeper and is the only place fuel and offsets differ:
    the target reads the guard's nodes as stored inside the case decode, while
    the oracle decodes the guard's own body afresh.
    [RegionChild.decoded_message_body_has_exact_rebased_children] states that
    the fresh decode is the rebased stored forest, and a semantic default of
    [Some true] depends only on descriptor fields, which rebasing preserves
    ([source_par_semantic_default_true_ignores_rebase]). *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SemanticReducer RPB3IndependentOracleCore RPB3IndependentOracleProcess
  RPB3IndependentOracleProjection RPB3IndependentOracleProcessProjection
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleRegionRebaseBridge RPB3IndependentOracleRegionChildBridge
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessChildCorrespondence
  RPB3IndependentOracleProcessNumericChildren
  RPB3IndependentOracleProcessTypedChildren
  RPB3IndependentOracleProcessExprChildren.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module RecursiveProjection := RPB3IndependentOracleRecursiveProjection.
Module Rebase := RPB3IndependentOracleRegionRebaseBridge.
Module RegionChild := RPB3IndependentOracleRegionChildBridge.

(* Disabled deliberately.  [Correspondence] and [ChildProvenance] resolve
   through the identical aliases that the imported theories already declare,
   and the per-child theories that have no such alias are named in full at
   their use sites.  Each of these libraries nests further aliases, so a local
   [Module X := Library] re-processes its whole structure.  In a profiled build
   the first four aliases below cost 159 s and about 7.9 GB (ChildProvenance
   17 s and 1.1 GB, Correspondence 27 s and 1.6 GB, Numeric 55 s and 2.9 GB,
   Typed 59 s and 2.3 GB) on top of the 6.9 GB of the imports, and the build
   was killed at its 16 GB cap during the fifth alias, before any proof ran.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module Correspondence := RPB3IndependentOracleProcessChildCorrespondence.
Module Numeric := RPB3IndependentOracleProcessNumericChildren.
Module Typed := RPB3IndependentOracleProcessTypedChildren.
Module ExprChildren := RPB3IndependentOracleProcessExprChildren.
*)

(** ** What an evaluated child records about its source node *)

(** The evaluator stores the converted raw field in the value it builds for an
    auxiliary node.  The walk through the evaluator's nested matches is the
    one [Correspondence.finish_auxiliary] performs for the sibling lemmas. *)
Lemma evaluated_auxiliary_node_retains_source_raw :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceAuxiliary span field) = Some child ->
    Source.raw_field_of_source body (Source.SourceAuxiliary span field) =
      Some (Core.source_summary_raw child).
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: reflexivity.
Qed.

Lemma evaluated_node_retains_source_raw :
  forall fuel hash_fn body program node child,
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program node = Some child ->
    Source.raw_field_of_source body node =
      Some (Core.source_summary_raw child).
Proof.
  intros fuel hash_fn body program node child evaluated.
  destruct node as [span field|span field nested|span field].
  - destruct (ChildProvenance.accepted_source_scalar_node_has_typed_value
      (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
      span field child evaluated)
      as [raw [scalar [converted [typed _]]]].
    unfold Core.source_summary_raw.
    rewrite typed.
    exact converted.
  - destruct (ChildProvenance.accepted_source_message_node_has_typed_value
      fuel hash_fn body program span field nested child evaluated)
      as [raw [nested_values [converted [typed _]]]].
    unfold Core.source_summary_raw.
    rewrite typed.
    exact converted.
  - exact (evaluated_auxiliary_node_retains_source_raw _ hash_fn body program
      span field child evaluated).
Qed.

(** The tag the oracle reads from an evaluated child's value is the tag of the
    node's generated descriptor. *)
Lemma evaluated_matched_node_value_tag_is_descriptor_tag :
  forall fuel hash_fn body program node child,
    Source.source_node_matches_generated_descriptor program node ->
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program node = Some child ->
    Process.source_value_tag (Core.source_summary_value child) =
      Schema.descriptor_tag (Source.source_field node).
Proof.
  intros fuel hash_fn body program node child [selected _] evaluated.
  destruct (Source.raw_field_of_source_preserves_wire_and_body body node
    (Core.source_summary_raw child)
    (evaluated_node_retains_source_raw fuel hash_fn body program node child
      evaluated)) as [raw_tag _].
  destruct (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [descriptor_tag _].
  change (Schema.raw_tag (Core.source_summary_raw child) =
    Schema.descriptor_tag (Source.source_field node)).
  rewrite raw_tag.
  symmetry.
  exact descriptor_tag.
Qed.

(** A matched node's descriptor is what the generated table resolves at the
    descriptor's own tag. *)
Lemma matched_node_resolves_at_its_tag :
  forall program node,
    Source.source_node_matches_generated_descriptor program node ->
    Schema.resolve_tag (Schema.program_fields program)
      (Schema.descriptor_tag (Source.source_field node)) =
      Some (Source.source_field node).
Proof.
  intros program node [selected _].
  pose proof (Source.resolved_source_descriptor_has_actual_tag_and_wire program
    (Source.source_span node) (Source.source_field node) selected)
    as [tag_is_number _].
  rewrite tag_is_number.
  unfold Source.source_descriptor in selected.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Source.Wire.span_number (Source.source_span node)))
    as [candidate|] eqn:resolved; [|discriminate selected].
  destruct (Schema.wire_eqb (Source.wire_kind_of_span (Source.source_span node))
      (Schema.descriptor_wire candidate)); [|discriminate selected].
  inversion selected; subst candidate.
  reflexivity.
Qed.

(** ** The generated descriptors of the slots the default readers select

    Each fact is one closed computation over the generated table, in the style
    of [expr_fields_contract_checked_true].  The proposition is computed from
    the table, so after [vm_compute] the goal mentions only the payload kind
    and child type of a single descriptor. *)

Definition generated_message_slot (family tag child_type : nat) : Prop :=
  match Schema.lookup_projected_program family [] [] with
  | None => False
  | Some program =>
      match Schema.resolve_tag (Schema.program_fields program) tag with
      | None => False
      | Some descriptor =>
          Schema.descriptor_payload descriptor = Schema.Message /\
          Schema.descriptor_child_type_id descriptor = Some child_type
      end
  end.

Lemma generated_message_slot_describes_field :
  forall family tag child_type program descriptor,
    generated_message_slot family tag child_type ->
    Schema.lookup_projected_program family [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) tag = Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Message /\
    Schema.descriptor_child_type_id descriptor = Some child_type.
Proof.
  intros family tag child_type program descriptor slot looked_up resolved.
  unfold generated_message_slot in slot.
  rewrite looked_up in slot.
  cbv beta iota in slot.
  rewrite resolved in slot.
  exact slot.
Qed.

Lemma receive_condition_slot_is_par_message : generated_message_slot 19 8 1.
Proof. vm_compute. split; reflexivity. Qed.

Lemma match_cases_slot_is_match_case_message : generated_message_slot 22 2 21.
Proof. vm_compute. split; reflexivity. Qed.

Lemma match_case_guard_slot_is_par_message : generated_message_slot 21 4 1.
Proof. vm_compute. split; reflexivity. Qed.

(** ** An accepted node at a message slot

    A node whose generated descriptor has [Message] payload can only be a
    [SourceMessage], because [Source.raw_field_of_source] refuses the other
    two constructors under that payload.  Its evaluation then ran the
    generated message step, which exposes the decoded region and the
    positional evaluation of its children. *)
Lemma evaluated_message_field_node_has_structure :
  forall fuel hash_fn body family program node child child_type,
    Schema.lookup_projected_program family [] [] = Some program ->
    Source.source_node_matches_generated_descriptor program node ->
    Schema.descriptor_payload (Source.source_field node) = Schema.Message ->
    Schema.descriptor_child_type_id (Source.source_field node) =
      Some child_type ->
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program node = Some child ->
    exists nested_fuel span field nested raw nested_program nested_nodes
        nested_children,
      node = Source.SourceMessage span field nested /\
      Source.raw_field_of_source body node = Some raw /\
      Core.source_summary_raw child = raw /\
      Schema.raw_wire raw = Schema.LengthDelimited /\
      Schema.raw_payload raw = Schema.Message /\
      Schema.raw_program_type_id raw = Some child_type /\
      Core.source_summary_value child =
        Core.TypedSourceMessage raw
          (map Core.source_summary_value nested_children) /\
      Schema.lookup_projected_program child_type [] [] = Some nested_program /\
      Source.decode_region (S (length (Schema.raw_body raw)))
        (Schema.raw_body raw) nested_program 0
        (length (Schema.raw_body raw)) = Some nested_nodes /\
      Forall2 (fun nested_node nested_child =>
        Source.source_node_matches_generated_descriptor nested_program
          nested_node /\
        Oracle.source_evaluate_node
          (Oracle.source_evaluate_message nested_fuel hash_fn) hash_fn
          (Schema.raw_body raw) nested_program nested_node = Some nested_child)
        nested_nodes nested_children.
Proof.
  intros fuel hash_fn body family program node child child_type looked_up
    matched message_payload child_type_found evaluated.
  pose proof (evaluated_node_retains_source_raw fuel hash_fn body program node
    child evaluated) as retained.
  destruct node as [span field|span field nested|span field];
    cbn [Source.source_field] in message_payload, child_type_found.
  - destruct (ChildProvenance.converted_scalar_raw_has_scalar_shape body span
      field (Core.source_summary_raw child) retained)
      as [_ [_ scalar_payload]].
    rewrite message_payload in scalar_payload.
    discriminate scalar_payload.
  - destruct matched as [resolved _].
    cbn [Source.source_span Source.source_field] in resolved.
    destruct (ChildProvenance.accepted_generated_message_node_has_descriptor_provenance
      fuel hash_fn body family program span field nested child looked_up
      resolved evaluated)
      as [raw [value_children [observed_type [converted [typed [same_raw
        [_ [wire [payload [raw_type field_type]]]]]]]]]].
    assert (same_observed : observed_type = child_type) by congruence.
    subst observed_type.
    unfold Oracle.source_evaluate_node in evaluated.
    rewrite converted in evaluated.
    change (Oracle.source_evaluate_message fuel hash_fn raw = Some child)
      in evaluated.
    destruct fuel as [|smaller]; [discriminate evaluated|].
    destruct (ChildProvenance.accepted_independent_message_has_generated_children
      smaller hash_fn raw child evaluated)
      as [type_id [nested_program [nested_nodes [nested_children
        [_ [family_type [program_found [decoded [shape aligned]]]]]]]]].
    assert (same_family_type : type_id = child_type) by congruence.
    subst type_id.
    exists smaller, span, field, nested, raw, nested_program, nested_nodes,
      nested_children.
    split; [reflexivity|].
    split; [exact converted|].
    split; [exact same_raw|].
    split; [exact wire|].
    split; [exact payload|].
    split; [exact raw_type|].
    split.
    { rewrite shape. reflexivity. }
    split; [exact program_found|].
    split; [exact decoded|].
    exact aligned.
  - unfold Source.raw_field_of_source in retained.
    cbn [Source.source_span Source.source_field] in retained.
    rewrite message_payload in retained.
    simpl in retained.
    discriminate retained.
Qed.

(** ** The typed Par default against the decoded nodes *)

(** The typed default of an accepted Par evaluation is the source default of
    the nodes the evaluation decoded. *)
Lemma accepted_par_evaluation_default_matches_decode :
  forall fuel hash_fn raw nodes children,
    Schema.raw_program_type_id raw = Some 1 ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) Source.generated_par_source_program 0
      (length (Schema.raw_body raw)) = Some nodes ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor
        Source.generated_par_source_program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn
        (Schema.raw_body raw) Source.generated_par_source_program node =
        Some child)
      nodes children ->
    Process.source_par_typed_default
      (Core.TypedSourceMessage raw
        (map Core.source_summary_value children)) =
    Source.source_par_semantic_default (Schema.raw_body raw) nodes.
Proof.
  intros fuel hash_fn raw nodes children raw_type decoded aligned.
  apply (RecursiveProjection.positional_par_root_default_agrees
    (Schema.raw_body raw) raw nodes children raw_type).
  apply (RecursiveProjection.selected_independent_par_children_are_positional
    fuel hash_fn (Schema.raw_body raw) nodes children decoded).
  exact (ChildProvenance.aligned_evaluated_children_are_selected
    Source.generated_par_source_program
    (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn (Schema.raw_body raw) Source.generated_par_source_program)
    nodes children aligned).
Qed.

(** The target's Par default test, given the decode it performs. *)
Lemma par_source_is_semantic_default_of_decode :
  forall body nodes,
    Source.decode_region (S (length body)) body
      Source.generated_par_source_program 0 (length body) = Some nodes ->
    Target.par_source_is_semantic_default body =
      match Source.source_par_semantic_default body nodes with
      | Some true => true
      | _ => false
      end.
Proof.
  intros body nodes decoded.
  unfold Target.par_source_is_semantic_default.
  rewrite Source.generated_par_source_lookup_is_present.
  cbv beta iota.
  rewrite decoded.
  reflexivity.
Qed.

(** ** Rebasing preserves a [Some true] semantic default

    A semantic default of [Some true] says that no component and no connective
    node is present, which reads only descriptor fields. *)

Lemma source_field_rebase_tree :
  forall base node,
    Source.source_field (Rebase.rebase_tree base node) =
      Source.source_field node.
Proof.
  intros base [span field|span field children|span field]; reflexivity.
Qed.

Lemma source_par_component_rebase :
  forall base node,
    Source.source_par_component (Rebase.rebase_tree base node) =
      Source.source_par_component node.
Proof.
  intros base node.
  unfold Source.source_par_component.
  rewrite source_field_rebase_tree.
  reflexivity.
Qed.

Lemma source_par_connective_node_rebase :
  forall base node,
    Source.source_par_connective_node (Rebase.rebase_tree base node) =
      Source.source_par_connective_node node.
Proof.
  intros base node.
  unfold Source.source_par_connective_node.
  rewrite source_field_rebase_tree.
  reflexivity.
Qed.

Lemma par_component_count_rebase :
  forall base nodes,
    Source.source_par_component_count (map (Rebase.rebase_tree base) nodes) =
      Source.source_par_component_count nodes.
Proof.
  intros base nodes.
  unfold Source.source_par_component_count.
  induction nodes as [|node rest IH]; [reflexivity|].
  cbn [map filter].
  rewrite source_par_component_rebase.
  destruct (Source.source_par_component node); cbn [length].
  - rewrite IH. reflexivity.
  - exact IH.
Qed.

Lemma find_par_connective_rebase_none :
  forall base nodes,
    find Source.source_par_connective_node
      (map (Rebase.rebase_tree base) nodes) = None <->
    find Source.source_par_connective_node nodes = None.
Proof.
  intros base nodes.
  induction nodes as [|node rest IH]; [split; intro; reflexivity|].
  cbn [map find].
  rewrite source_par_connective_node_rebase.
  destruct (Source.source_par_connective_node node).
  - split; intro impossible; discriminate impossible.
  - exact IH.
Qed.

Lemma source_par_semantic_default_true_ignores_rebase :
  forall bytes rebased_bytes base nodes,
    Source.source_par_semantic_default rebased_bytes
      (map (Rebase.rebase_tree base) nodes) = Some true <->
    Source.source_par_semantic_default bytes nodes = Some true.
Proof.
  intros bytes rebased_bytes base nodes.
  pose proof (par_component_count_rebase base nodes) as same_count.
  pose proof (find_par_connective_rebase_none base nodes)
    as [none_forward none_backward].
  split; intro accepted.
  - apply Source.source_par_semantic_default_is_exactly_empty in accepted
      as [count_zero no_connective].
    apply Source.source_par_semantic_default_is_exactly_empty.
    split.
    + rewrite <- same_count. exact count_zero.
    + exact (none_forward no_connective).
  - apply Source.source_par_semantic_default_is_exactly_empty in accepted
      as [count_zero no_connective].
    apply Source.source_par_semantic_default_is_exactly_empty.
    split.
    + rewrite same_count. exact count_zero.
    + exact (none_backward no_connective).
Qed.

Lemma some_true_view_respects_iff :
  forall left right : option bool,
    (left = Some true <-> right = Some true) ->
    match left with Some true => true | _ => false end =
    match right with Some true => true | _ => false end.
Proof.
  intros left right [forward backward].
  destruct left as [[|]|], right as [[|]|]; try reflexivity.
  all: first
    [ specialize (forward eq_refl); discriminate forward
    | specialize (backward eq_refl); discriminate backward ].
Qed.

(** ** Alignment of decoded nodes and evaluated children *)

(** The first node carrying a tag and the first evaluated value carrying that
    tag are the two halves of one aligned pair. *)
Lemma evaluated_aligned_children_find_tag :
  forall fuel hash_fn body program tag nodes children,
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
        node = Some child) nodes children ->
    (find (fun node =>
        Schema.descriptor_tag (Source.source_field node) =? tag) nodes = None /\
     find (fun value => Process.source_value_tag value =? tag)
       (map Core.source_summary_value children) = None) \/
    (exists node child,
      find (fun node =>
        Schema.descriptor_tag (Source.source_field node) =? tag) nodes =
        Some node /\
      find (fun value => Process.source_value_tag value =? tag)
        (map Core.source_summary_value children) =
        Some (Core.source_summary_value child) /\
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program
        node = Some child).
Proof.
  intros fuel hash_fn body program tag nodes children aligned.
  induction aligned as [|node child rest_nodes rest_children
      [matched evaluated] rest IH].
  - left. split; reflexivity.
  - cbn [find map].
    rewrite (evaluated_matched_node_value_tag_is_descriptor_tag fuel hash_fn
      body program node child matched evaluated).
    destruct (Schema.descriptor_tag (Source.source_field node) =? tag)
      eqn:tag_found.
    + right.
      exists node, child.
      split; [reflexivity|].
      split; [reflexivity|].
      split; [exact matched|exact evaluated].
    + exact IH.
Qed.

(** ** The atoms *)

Theorem process_actual_children_scalar_arm_corresponds :
  forall fuel hash_fn program body children child,
    Correspondence.process_actual_children fuel hash_fn 24 program body
      children ->
    In child children ->
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Scalar =
    match Core.source_summary_value child with
    | Core.TypedSourceScalar _ _ => true
    | _ => false
    end.
Proof.
  intros fuel hash_fn program body children child actual member.
  destruct
    (RPB3IndependentOracleProcessTypedChildren.actual_child_has_matched_node
      fuel hash_fn 24 program body children child actual member)
    as [_ [node [_ evaluated]]].
  destruct (Correspondence.evaluated_node_cases fuel hash_fn body program node
    child evaluated)
    as [[raw [scalar [typed [_ [payload _]]]]]|[not_scalar not_value]].
  - unfold Core.source_summary_raw.
    rewrite typed.
    cbn [Core.source_value_raw].
    rewrite payload.
    reflexivity.
  - rewrite not_scalar.
    unfold Correspondence.value_is_scalar in not_value.
    destruct (Core.source_summary_value child);
      try discriminate not_value; reflexivity.
Qed.

Theorem process_actual_children_receive_default_atom_corresponds :
  forall fuel hash_fn program body children child,
    Correspondence.process_actual_children fuel hash_fn 19 program body
      children ->
    In child children ->
    ((Core.source_summary_tag child =? 8) &&
      negb (Core.source_summary_refused child) &&
      match Process.source_par_typed_default
          (Core.source_summary_value child) with
      | Some true => true | _ => false end) =
    ((Target.sem_tag (Projection.project_source_summary child) =? 8) &&
      (Target.sem_type_id (Projection.project_source_summary child) =? 1) &&
      Schema.wire_eqb
        (Target.sem_wire (Projection.project_source_summary child))
        Schema.LengthDelimited &&
      Schema.payload_eqb
        (Target.sem_payload (Projection.project_source_summary child))
        Schema.Message &&
      negb (Target.sem_definitely_refused
        (Projection.project_source_summary child)) &&
      Target.par_source_is_semantic_default
        (Target.sem_body (Projection.project_source_summary child))).
Proof.
  intros fuel hash_fn program body children child actual member.
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (Projection.project_source_type_id (Core.source_summary_raw child)).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_body (Projection.project_source_summary child))
    with (Schema.raw_body (Core.source_summary_raw child)).
  destruct (Core.source_summary_tag child =? 8) eqn:tag_matches.
  2: { reflexivity. }
  apply Nat.eqb_eq in tag_matches.
  destruct
    (RPB3IndependentOracleProcessTypedChildren.actual_child_has_matched_node
      fuel hash_fn 19 program body children child actual member)
    as [looked_up [node [matched evaluated]]].
  pose proof (evaluated_matched_node_value_tag_is_descriptor_tag fuel hash_fn
    body program node child matched evaluated) as value_tag.
  assert (field_tag : Schema.descriptor_tag (Source.source_field node) = 8).
  { rewrite <- value_tag. exact tag_matches. }
  pose proof (matched_node_resolves_at_its_tag program node matched)
    as resolved.
  rewrite field_tag in resolved.
  destruct (generated_message_slot_describes_field 19 8 1 program
    (Source.source_field node) receive_condition_slot_is_par_message
    looked_up resolved)
    as [message_payload child_is_par].
  destruct (evaluated_message_field_node_has_structure fuel hash_fn body 19
    program node child 1 looked_up matched message_payload child_is_par
    evaluated)
    as [nested_fuel [span [field [nested [raw [nested_program [nested_nodes
      [nested_children [_ [_ [same_raw [wire [payload [raw_type [typed
      [program_found [decoded aligned]]]]]]]]]]]]]]]]].
  rewrite Source.generated_par_source_lookup_is_present in program_found.
  injection program_found as same_program.
  subst nested_program.
  rewrite same_raw, typed.
  rewrite (accepted_par_evaluation_default_matches_decode nested_fuel hash_fn
    raw nested_nodes nested_children raw_type decoded aligned).
  rewrite (par_source_is_semantic_default_of_decode (Schema.raw_body raw)
    nested_nodes decoded).
  assert (type_test : (Projection.project_source_type_id raw =? 1) = true).
  { unfold Projection.project_source_type_id.
    rewrite raw_type.
    reflexivity. }
  rewrite type_test, wire, payload.
  reflexivity.
Qed.

(** The guard reading of one [MatchCase] body: the oracle's typed default of
    the guard value agrees with the target's source default of the guard's
    nested nodes. *)
Lemma match_case_default_guard_matches_decode :
  forall nested_fuel hash_fn raw program nodes children,
    Schema.lookup_projected_program 21 [] [] = Some program ->
    Source.decode_region (S (length (Schema.raw_body raw)))
      (Schema.raw_body raw) program 0 (length (Schema.raw_body raw)) =
      Some nodes ->
    Forall2 (fun node child =>
      Source.source_node_matches_generated_descriptor program node /\
      Oracle.source_evaluate_node
        (Oracle.source_evaluate_message nested_fuel hash_fn) hash_fn
        (Schema.raw_body raw) program node = Some child)
      nodes children ->
    match find (fun value => Process.source_value_tag value =? 4)
        (map Core.source_summary_value children) with
    | Some guard =>
        match Process.source_par_typed_default guard with
        | Some true => true
        | _ => false
        end
    | None => false
    end =
    match find (fun node =>
        Schema.descriptor_tag (Source.source_field node) =? 4) nodes with
    | Some (Source.SourceMessage _ _ guard_children) =>
        Target.par_source_nodes_are_semantic_default (Schema.raw_body raw)
          guard_children
    | _ => false
    end.
Proof.
  intros nested_fuel hash_fn raw program nodes children looked_up decoded
    aligned.
  destruct (evaluated_aligned_children_find_tag nested_fuel hash_fn
    (Schema.raw_body raw) program 4 nodes children aligned)
    as [[no_node no_value]|[node [child [node_found [value_found
      [matched evaluated]]]]]].
  - rewrite no_value, no_node.
    reflexivity.
  - rewrite value_found, node_found.
    apply find_some in node_found as [node_member tag_is_four].
    apply Nat.eqb_eq in tag_is_four.
    pose proof (matched_node_resolves_at_its_tag program node matched)
      as resolved.
    rewrite tag_is_four in resolved.
    destruct (generated_message_slot_describes_field 21 4 1 program
      (Source.source_field node) match_case_guard_slot_is_par_message
      looked_up resolved)
      as [message_payload child_is_par].
    destruct (evaluated_message_field_node_has_structure nested_fuel hash_fn
      (Schema.raw_body raw) 21 program node child 1 looked_up matched
      message_payload child_is_par evaluated)
      as [guard_fuel [span [field [guard_children [guard_raw [guard_program
        [guard_nodes [guard_summaries [node_shape [guard_converted
        [_ [_ [_ [guard_type [guard_typed [guard_found
        [guard_decoded guard_aligned]]]]]]]]]]]]]]]]].
    rewrite Source.generated_par_source_lookup_is_present in guard_found.
    injection guard_found as same_guard_program.
    subst guard_program.
    subst node.
    cbv beta iota.
    rewrite guard_typed.
    rewrite (accepted_par_evaluation_default_matches_decode guard_fuel hash_fn
      guard_raw guard_nodes guard_summaries guard_type guard_decoded
      guard_aligned).
    destruct (RegionChild.decoded_message_body_has_exact_rebased_children
      (length (Schema.raw_body raw)) (Schema.raw_body raw) program 0
      (length (Schema.raw_body raw)) nodes span field guard_children guard_raw
      ltac:(lia) ltac:(lia) decoded node_member guard_converted)
      as [fresh_type [fresh_program [fresh_type_found [fresh_program_found
        fresh_decoded]]]].
    assert (fresh_type_is_one : fresh_type = 1) by congruence.
    subst fresh_type.
    rewrite Source.generated_par_source_lookup_is_present
      in fresh_program_found.
    injection fresh_program_found as same_fresh_program.
    subst fresh_program.
    rewrite guard_decoded in fresh_decoded.
    injection fresh_decoded as same_guard_nodes.
    subst guard_nodes.
    unfold Target.par_source_nodes_are_semantic_default.
    apply some_true_view_respects_iff.
    apply source_par_semantic_default_true_ignores_rebase.
Qed.

Theorem process_actual_children_match_default_atom_corresponds :
  forall fuel hash_fn program body children child,
    Correspondence.process_actual_children fuel hash_fn 22 program body
      children ->
    In child children ->
    Process.source_match_case_has_default_guard child =
    Target.match_case_has_present_default_guard
      (Projection.project_source_summary child).
Proof.
  intros fuel hash_fn program body children child actual member.
  unfold Process.source_match_case_has_default_guard,
    Target.match_case_has_present_default_guard.
  change (Target.sem_tag (Projection.project_source_summary child))
    with (Core.source_summary_tag child).
  change (Target.sem_type_id (Projection.project_source_summary child))
    with (Projection.project_source_type_id (Core.source_summary_raw child)).
  change (Target.sem_wire (Projection.project_source_summary child))
    with (Schema.raw_wire (Core.source_summary_raw child)).
  change (Target.sem_payload (Projection.project_source_summary child))
    with (Schema.raw_payload (Core.source_summary_raw child)).
  change (Target.sem_definitely_refused
      (Projection.project_source_summary child))
    with (Core.source_summary_refused child).
  change (Target.sem_body (Projection.project_source_summary child))
    with (Schema.raw_body (Core.source_summary_raw child)).
  destruct (Core.source_summary_tag child =? 2) eqn:tag_matches.
  2: { reflexivity. }
  apply Nat.eqb_eq in tag_matches.
  destruct
    (RPB3IndependentOracleProcessTypedChildren.actual_child_has_matched_node
      fuel hash_fn 22 program body children child actual member)
    as [looked_up [node [matched evaluated]]].
  pose proof (evaluated_matched_node_value_tag_is_descriptor_tag fuel hash_fn
    body program node child matched evaluated) as value_tag.
  assert (field_tag : Schema.descriptor_tag (Source.source_field node) = 2).
  { rewrite <- value_tag. exact tag_matches. }
  pose proof (matched_node_resolves_at_its_tag program node matched)
    as resolved.
  rewrite field_tag in resolved.
  destruct (generated_message_slot_describes_field 22 2 21 program
    (Source.source_field node) match_cases_slot_is_match_case_message
    looked_up resolved)
    as [message_payload child_is_case].
  destruct (evaluated_message_field_node_has_structure fuel hash_fn body 22
    program node child 21 looked_up matched message_payload child_is_case
    evaluated)
    as [nested_fuel [span [field [nested [raw [nested_program [nested_nodes
      [nested_children [_ [_ [same_raw [wire [payload [raw_type [typed
      [program_found [decoded aligned]]]]]]]]]]]]]]]]].
  rewrite same_raw.
  assert (type_test : (Projection.project_source_type_id raw =? 21) = true).
  { unfold Projection.project_source_type_id.
    rewrite raw_type.
    reflexivity. }
  assert (wire_test :
    Schema.wire_eqb (Schema.raw_wire raw) Schema.LengthDelimited = true).
  { rewrite wire. reflexivity. }
  assert (payload_test :
    Schema.payload_eqb (Schema.raw_payload raw) Schema.Message = true).
  { rewrite payload. reflexivity. }
  rewrite type_test, wire_test, payload_test.
  rewrite typed.
  destruct (Core.source_summary_refused child) eqn:refused_bit.
  { reflexivity. }
  cbv beta iota delta [andb negb].
  rewrite raw_type, program_found.
  cbv beta iota.
  rewrite decoded.
  exact (match_case_default_guard_matches_decode nested_fuel hash_fn raw
    nested_program nested_nodes nested_children program_found decoded aligned).
Qed.

(** ** The reader-slot bridges

    Each slot function of [ProcessProjection] is a finite match on the pair
    [(family, tag)].  Reducing it down to its leaves leaves only the pairs the
    function accepts, each of which is a member of the reader-slot list of the
    corresponding theorem. *)

Ltac reduce_slot_match slot :=
  cbv beta iota in slot;
  lazymatch type of slot with
  | (match ?scrutinee with _ => _ end) = true =>
      destruct scrutinee; reduce_slot_match slot
  | false = true => discriminate slot
  | true = true => idtac
  end.

Ltac close_reader_slot :=
  cbn [In];
  repeat (first [left; reflexivity | right]).

Lemma process_scalar_true_slot_is_reader_slot :
  forall family tag,
    ProcessProjection.process_scalar_true_slot family tag = true ->
    In (family, tag) Correspondence.scalar_true_reader_slots.
Proof.
  intros family tag slot.
  unfold ProcessProjection.process_scalar_true_slot in slot.
  reduce_slot_match slot.
  all: unfold Correspondence.scalar_true_reader_slots; close_reader_slot.
Qed.

Lemma process_exact_bool_slot_is_reader_slot :
  forall family tag,
    ProcessProjection.process_exact_bool_slot family tag = true ->
    In (family, tag) Correspondence.exact_bool_reader_slots.
Proof.
  intros family tag slot.
  unfold ProcessProjection.process_exact_bool_slot in slot.
  reduce_slot_match slot.
  all: unfold Correspondence.exact_bool_reader_slots; close_reader_slot.
Qed.

Lemma process_i32_slot_is_reader_slot :
  forall family tag,
    ProcessProjection.process_i32_slot family tag = true ->
    In (family, tag)
      RPB3IndependentOracleProcessNumericChildren.i32_reader_slots.
Proof.
  intros family tag slot.
  unfold ProcessProjection.process_i32_slot in slot.
  reduce_slot_match slot.
  all: unfold RPB3IndependentOracleProcessNumericChildren.i32_reader_slots;
    close_reader_slot.
Qed.

(** ** The assembled record *)

Theorem process_actual_children_correspondence :
  forall fuel hash_fn family program body children,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    ProcessProjection.process_child_correspondence family children.
Proof.
  intros fuel hash_fn family program body children actual.
  constructor.
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_child_score_corresponds
      fuel hash_fn family program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_selected_scores_correspond
      fuel hash_fn family program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_optional_score_corresponds
      fuel hash_fn family program body children actual).
  - intros tag slot.
    exact (Correspondence.process_actual_children_scalar_true_corresponds_at_reader_slots
      fuel hash_fn family program body children tag
      (process_scalar_true_slot_is_reader_slot family tag slot) actual).
  - intros tag slot.
    exact (Correspondence.process_actual_children_bool_corresponds_at_reader_slots
      fuel hash_fn family program body children tag
      (process_exact_bool_slot_is_reader_slot family tag slot) actual).
  - intros tag slot.
    exact (RPB3IndependentOracleProcessNumericChildren.process_actual_children_i32_corresponds_at_reader_slots
      fuel hash_fn family program body children tag
      (process_i32_slot_is_reader_slot family tag slot) actual).
  - intro new_family.
    subst family.
    exact (RPB3IndependentOracleProcessNumericChildren.process_actual_children_new_word_corresponds
      fuel hash_fn program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_typed_child_corresponds
      fuel hash_fn family program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_typed_selected_corresponds
      fuel hash_fn family program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_typed_optional_corresponds
      fuel hash_fn family program body children actual).
  - exact (RPB3IndependentOracleProcessTypedChildren.process_actual_children_typed_fallback_corresponds
      fuel hash_fn family program body children actual).
  - intros expr_family child member.
    subst family.
    exact (RPB3IndependentOracleProcessExprChildren.process_actual_children_expr_arm_corresponds
      fuel hash_fn program body children child actual member).
  - intros expr_family child member.
    subst family.
    exact (process_actual_children_scalar_arm_corresponds fuel hash_fn program
      body children child actual member).
  - intros receive_family child member.
    subst family.
    exact (process_actual_children_receive_default_atom_corresponds fuel
      hash_fn program body children child actual member).
  - intros match_family child member.
    subst family.
    exact (process_actual_children_match_default_atom_corresponds fuel hash_fn
      program body children child actual member).
Qed.

Print Assumptions evaluated_auxiliary_node_retains_source_raw.
Print Assumptions evaluated_node_retains_source_raw.
Print Assumptions evaluated_matched_node_value_tag_is_descriptor_tag.
Print Assumptions matched_node_resolves_at_its_tag.
Print Assumptions generated_message_slot_describes_field.
Print Assumptions receive_condition_slot_is_par_message.
Print Assumptions match_cases_slot_is_match_case_message.
Print Assumptions match_case_guard_slot_is_par_message.
Print Assumptions evaluated_message_field_node_has_structure.
Print Assumptions accepted_par_evaluation_default_matches_decode.
Print Assumptions par_source_is_semantic_default_of_decode.
Print Assumptions source_field_rebase_tree.
Print Assumptions source_par_component_rebase.
Print Assumptions source_par_connective_node_rebase.
Print Assumptions par_component_count_rebase.
Print Assumptions find_par_connective_rebase_none.
Print Assumptions source_par_semantic_default_true_ignores_rebase.
Print Assumptions some_true_view_respects_iff.
Print Assumptions evaluated_aligned_children_find_tag.
Print Assumptions process_actual_children_scalar_arm_corresponds.
Print Assumptions process_actual_children_receive_default_atom_corresponds.
Print Assumptions match_case_default_guard_matches_decode.
Print Assumptions process_actual_children_match_default_atom_corresponds.
Print Assumptions process_scalar_true_slot_is_reader_slot.
Print Assumptions process_exact_bool_slot_is_reader_slot.
Print Assumptions process_i32_slot_is_reader_slot.
Print Assumptions process_actual_children_correspondence.
