(** * The two reducers refuse the same non-canonical ESet and EMap bodies

    Leaf L11 of the score-arms design.  On the children the oracle evaluates
    from a decoded body, the target's [payload_fixed_point_violated] applied to
    the projected children equals the oracle's
    [source_collection_outside_fixed_point].

    Citations: P is models/src/rust/rholang/protobuf_admission.rs.  An ESet
    keeps its elements strictly ascending by (score bytes, raw Par body) with
    globally unique semantic identities (P:3098-3116).  An EMap does the same
    over complete pairs by (key score, whole pair body) with unique key
    identities, and the check is effective only when the map is complete
    (P:3158-3184).

    The proof has four parts:

    - [body_leq_total] and [nat_list_eqb_is_mutual_leq]: the byte order is
      total, which is what makes the target's and the oracle's two ways of
      writing the strict comparison agree, and mutual order is byte equality;
    - [ascending_keys_project] and [identities_unique_project]: the two list
      predicates agree through the score projection;
    - [aligned_child_keyed_facts]: an evaluated child carries the projected
      typed-message score, and a child that scores as a Par has the identity
      that deliverable L10 computes from its raw body;
    - [canonical_eset_projects] and [canonical_emap_projects]: keyed sequencing
      commutes with the projection.

    The red control runs on a body that is really decoded and evaluated: an
    ESet of three Par elements with one score, ascending raw bodies, and
    identities I, J, I, so the repeat is not adjacent. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleProcessProjection RPB3IndependentOracleProjection
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3SemanticReducer RPB3IndependentOracleExprPayload
  RPB3IndependentOracleRecursive
  RPB3IndependentOracleParIdentityBridge
  RPB3IndependentOracleParOrderBridge.

Import ListNotations.

(** ** The byte order is total, and mutual order is equality *)

Lemma body_leq_total :
  forall left right,
    body_leq left right = true \/ body_leq right left = true.
Proof.
  induction left as [|first rest IH]; intros [|second suffix].
  - left. reflexivity.
  - left. reflexivity.
  - right. reflexivity.
  - cbn [body_leq].
    destruct (first <? second) eqn:first_less.
    + left. reflexivity.
    + destruct (second <? first) eqn:second_less.
      * right. reflexivity.
      * exact (IH suffix).
Qed.

Lemma nat_list_eqb_is_mutual_leq :
  forall left right,
    nat_list_eqb left right = body_leq left right && body_leq right left.
Proof.
  induction left as [|first rest IH]; intros [|second suffix].
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - cbn [nat_list_eqb body_leq].
    destruct (first =? second) eqn:same.
    + apply Nat.eqb_eq in same.
      subst second.
      rewrite Nat.ltb_irrefl.
      exact (IH suffix).
    + apply Nat.eqb_neq in same.
      destruct (first <? second) eqn:first_less.
      * apply Nat.ltb_lt in first_less.
        assert (not_second : (second <? first) = false).
        { apply Nat.ltb_ge. lia. }
        rewrite not_second.
        reflexivity.
      * destruct (second <? first) eqn:second_less.
        -- reflexivity.
        -- exfalso.
           apply Nat.ltb_ge in first_less.
           apply Nat.ltb_ge in second_less.
           lia.
Qed.

Lemma nat_list_eqb_matches_source_bytes_eqb :
  forall left right,
    nat_list_eqb left right = source_bytes_eqb left right.
Proof.
  intros left right.
  rewrite nat_list_eqb_is_mutual_leq.
  unfold source_bytes_eqb.
  (* [source_bytes_leq] and [body_leq] have the same body, so the two sides are
     convertible; a [rewrite] with [projected_process_bytes_leq] would match the
     target's [body_leq] by conversion and leave the goal unchanged. *)
  reflexivity.
Qed.

Lemma existsb_pointwise :
  forall (A : Type) (f g : A -> bool) elements,
    (forall element, f element = g element) ->
    existsb f elements = existsb g elements.
Proof.
  intros A f g elements same.
  induction elements as [|element rest IH]; [reflexivity|].
  cbn [existsb].
  rewrite same, IH.
  reflexivity.
Qed.

Lemma identities_unique_project :
  forall identities,
    payload_identities_unique identities =
    source_identities_unique identities.
Proof.
  induction identities as [|identity_bytes rest IH]; [reflexivity|].
  cbn [payload_identities_unique source_identities_unique].
  rewrite IH.
  rewrite (existsb_pointwise _ (nat_list_eqb identity_bytes)
    (source_bytes_eqb identity_bytes) rest
    (nat_list_eqb_matches_source_bytes_eqb identity_bytes)).
  reflexivity.
Qed.

(** ** The strict comparison

    The target writes the score comparison as "equal, then compare bodies,
    otherwise forward"; the oracle writes it as "strictly less, strictly
    greater, then compare bodies".  They agree exactly when the score order is
    total: on two score encodings that are mutually unordered the target
    returns false and the oracle returns true. *)

Lemma strict_before_case :
  forall forward backward tie,
    forward = true \/ backward = true ->
    (if forward && backward then negb tie else forward) =
    (if negb backward then true
     else if negb forward then false else negb tie).
Proof.
  intros [|] [|] tie total; try reflexivity.
  destruct total as [impossible|impossible]; discriminate impossible.
Qed.

Lemma strict_before_projects :
  forall left right,
    score_body_strict_before
      (project_score (fst left), snd left)
      (project_score (fst right), snd right) =
    source_key_strict_before left right.
Proof.
  intros left right.
  unfold score_body_strict_before, source_key_strict_before, source_bytes_lt.
  cbv zeta.
  cbn [fst snd].
  rewrite !source_score_bytes_projects.
  (* The oracle's [source_bytes_leq] is convertible with [body_leq]; the
     equation is checked by conversion. *)
  exact (strict_before_case _ _ _ (body_leq_total _ _)).
Qed.

Lemma payload_ascending_unfold_pair :
  forall first second tail,
    payload_keys_strictly_ascending (first :: second :: tail) =
    score_body_strict_before first second &&
    payload_keys_strictly_ascending (second :: tail).
Proof. reflexivity. Qed.

Lemma source_ascending_unfold_pair :
  forall first second tail,
    source_keys_strictly_ascending (first :: second :: tail) =
    source_key_strict_before first second &&
    source_keys_strictly_ascending (second :: tail).
Proof. reflexivity. Qed.

Lemma ascending_keys_project :
  forall keys,
    payload_keys_strictly_ascending
      (map (fun key : source_score * list nat =>
        (project_score (fst key), snd key)) keys) =
    source_keys_strictly_ascending keys.
Proof.
  induction keys as [|first rest IH]; [reflexivity|].
  destruct rest as [|second tail]; [reflexivity|].
  cbn [map] in IH |- *.
  rewrite payload_ascending_unfold_pair, source_ascending_unfold_pair.
  rewrite IH, strict_before_projects.
  reflexivity.
Qed.

(** ** Selecting the element children *)

Lemma filter_tag_projects :
  forall tag children,
    filter (fun child => sem_tag child =? tag)
      (map project_source_summary children) =
    map project_source_summary
      (filter (fun child => source_summary_tag child =? tag) children).
Proof.
  intros tag children.
  induction children as [|head rest IH]; [reflexivity|].
  cbn [map filter].
  change (sem_tag (project_source_summary head)) with (source_summary_tag head).
  destruct (source_summary_tag head =? tag); cbn [map]; rewrite IH; reflexivity.
Qed.

Lemma forall_filter_preserved :
  forall (A : Type) (predicate : A -> Prop) (keep : A -> bool) elements,
    Forall predicate elements -> Forall predicate (filter keep elements).
Proof.
  intros A predicate keep elements all_hold.
  apply Forall_forall.
  intros element member.
  apply filter_In in member as [inside _].
  rewrite Forall_forall in all_hold.
  exact (all_hold element inside).
Qed.

Lemma sequence_map_option_map :
  forall (A B C : Type) (source_key : A -> option B)
      (target_key : A -> option C) (project_key : B -> C) (elements : list A),
    Forall (fun element =>
      target_key element = option_map project_key (source_key element))
      elements ->
    sequence_source_results (map target_key elements) =
    option_map (map project_key)
      (sequence_source_results (map source_key elements)).
Proof.
  intros A B C source_key target_key project_key elements related.
  induction related as [|element rest head_related tail_related IH];
    [reflexivity|].
  cbn [map sequence_source_results].
  rewrite head_related.
  destruct (source_key element) as [key|]; cbn [option_map]; [|reflexivity].
  rewrite IH.
  destruct (sequence_source_results (map source_key rest)); reflexivity.
Qed.

Lemma canonical_eset_children_unfold :
  forall children,
    canonical_eset_children children =
    option_map (fun keys =>
      payload_keys_strictly_ascending keys &&
      payload_identities_unique
        (map (fun child => par_identity_bytes (sem_body child))
          (filter (fun child => sem_tag child =? 1) children)))
      (sequence_source_results
        (map eset_element_key
          (filter (fun child => sem_tag child =? 1) children))).
Proof. intro children. reflexivity. Qed.

Lemma canonical_emap_children_unfold :
  forall children,
    canonical_emap_children children =
    option_map (fun keyed =>
      payload_keys_strictly_ascending (map fst keyed) &&
      payload_identities_unique (map snd keyed))
      (sequence_source_results
        (map emap_pair_key
          (filter (fun child => sem_tag child =? 1) children))).
Proof. intro children. reflexivity. Qed.

Lemma source_collection_canonical_unfold :
  forall order_key children,
    source_collection_canonical order_key children =
    option_map (fun keyed =>
      source_keys_strictly_ascending (map fst keyed) &&
      source_identities_unique (map snd keyed))
      (sequence_source_results
        (map order_key
          (filter (fun child => source_summary_tag child =? 1) children))).
Proof. intros order_key children. reflexivity. Qed.

(** ** Facts about evaluated children

    The score projection and the shape of an auxiliary node are restated from
    the connective-arm bridge (leaf L09), which this leaf must not import. *)

Lemma forall2_right_forall :
  forall (A B : Type) (relation : A -> B -> Prop) (predicate : B -> Prop)
      lefts rights,
    (forall left_value right_value,
      relation left_value right_value -> predicate right_value) ->
    Forall2 relation lefts rights -> Forall predicate rights.
Proof.
  intros A B relation predicate lefts rights implied related.
  induction related as [|left_value right_value lefts' rights' head rest IH].
  - constructor.
  - constructor.
    + exact (implied left_value right_value head).
    + exact IH.
Qed.

(** Walk the nested matches of an auxiliary-node evaluation down to its
    success constructor.  A [None] branch is refuted by the equation to
    [Some]. *)
Ltac finish_auxiliary evaluated :=
  cbv beta iota zeta in evaluated;
  lazymatch type of evaluated with
  | Some _ = Some _ => idtac
  | None = Some _ => discriminate evaluated
  | (match ?scrutinee with _ => _ end) = Some _ =>
      destruct scrutinee; finish_auxiliary evaluated
  end.

Definition value_is_message (value : typed_source_value) : bool :=
  match value with
  | TypedSourceMessage _ _ => true
  | _ => false
  end.

(** An auxiliary map node never evaluates to a message-shaped value. *)
Lemma evaluated_auxiliary_node_is_not_message :
  forall recurse hash_fn body program span field child,
    source_evaluate_node recurse hash_fn body program
      (SourceAuxiliary span field) = Some child ->
    payload_eqb (raw_payload (source_summary_raw child)) Message = false /\
    value_is_message (source_summary_value child) = false.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold source_evaluate_node in evaluated.
  destruct (raw_field_of_source body (SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (raw_field_of_source_preserves_wire_and_body
    body (SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [source_field] in raw_payload_is_descriptor.
  destruct (descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: cbn [source_summary_raw source_value_raw source_summary_value
    value_is_message].
  all: rewrite raw_payload_is_descriptor.
  all: split; reflexivity.
Qed.

(** A scalar or auxiliary child is not a typed message on either side; a
    message child carries the descriptor's own child type, and the score is
    the source score. *)
Lemma typed_message_score_projects :
  forall fuel hash_fn body family program node child expected,
    lookup_projected_program family [] [] = Some program ->
    source_node_matches_generated_descriptor program node ->
    source_evaluate_node
      (source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    exact_typed_message_score expected (project_source_summary child) =
    option_map project_score
      (source_payload_typed_message_score expected child).
Proof.
  intros fuel hash_fn body family program node child expected
    looked_up matched evaluated.
  destruct matched as [selected _].
  destruct node as [span field|span field nested|span field];
    cbn [source_span source_field] in selected.
  - destruct (accepted_source_scalar_node_has_typed_value
      _ hash_fn body program span field child evaluated)
      as [raw [scalar [_ [typed payload]]]].
    assert (retained : source_summary_raw child = raw).
    { unfold source_summary_raw. rewrite typed. reflexivity. }
    unfold exact_typed_message_score, source_payload_typed_message_score.
    change (sem_payload (project_source_summary child))
      with (raw_payload (source_summary_raw child)).
    rewrite retained, payload.
    cbn [payload_eqb].
    rewrite andb_false_r.
    rewrite typed.
    reflexivity.
  - exact (accepted_generated_message_node_typed_score_projects
      fuel hash_fn body family program span field nested child expected
      looked_up selected evaluated).
  - destruct (evaluated_auxiliary_node_is_not_message
      _ hash_fn body program span field child evaluated)
      as [payload value_shape].
    unfold exact_typed_message_score, source_payload_typed_message_score.
    change (sem_payload (project_source_summary child))
      with (raw_payload (source_summary_raw child)).
    rewrite payload.
    rewrite andb_false_r.
    destruct (source_summary_value child)
      as [some_raw some_scalar|some_raw some_nested
        |some_raw some_key some_value|some_raw some_snapshot some_values];
      cbn [value_is_message] in value_shape;
      try discriminate value_shape;
      reflexivity.
Qed.

Lemma typed_score_exposes_type :
  forall expected child score,
    source_payload_typed_message_score expected child = Some score ->
    source_summary_type_id child = Some expected.
Proof.
  intros expected child score scored.
  unfold source_payload_typed_message_score in scored.
  destruct (source_summary_value child); try discriminate scored.
  destruct (source_summary_type_id child) as [observed|]; [|discriminate scored].
  destruct ((observed =? expected) && negb (source_summary_refused child))
    eqn:guard; [|discriminate scored].
  apply andb_true_iff in guard as [same_type _].
  apply Nat.eqb_eq in same_type.
  rewrite same_type.
  reflexivity.
Qed.

(** What the ordering check reads from one child: its typed scores project,
    and a child that scores as a Par has the identity that [par_identity_bytes]
    computes from its raw body. *)
Definition keyed_child_facts (child : source_exact_summary) : Prop :=
  (forall expected,
    exact_typed_message_score expected (project_source_summary child) =
    option_map project_score
      (source_payload_typed_message_score expected child)) /\
  (forall score,
    source_payload_typed_message_score 1 child = Some score ->
    par_identity_bytes (source_summary_body child) =
    source_par_identity_bytes (source_summary_value child)).

Lemma aligned_child_keyed_facts :
  forall fuel hash_fn body family program node child,
    lookup_projected_program family [] [] = Some program ->
    source_node_matches_generated_descriptor program node ->
    source_evaluate_node
      (source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    keyed_child_facts child.
Proof.
  intros fuel hash_fn body family program node child
    looked_up matched evaluated.
  split.
  - intro expected.
    exact (typed_message_score_projects fuel hash_fn body family program node
      child expected looked_up matched evaluated).
  - intros score scored.
    destruct node as [span field|span field nested|span field].
    + destruct (accepted_source_scalar_node_has_typed_value
        _ hash_fn body program span field child evaluated)
        as [raw [scalar [_ [typed _]]]].
      unfold source_payload_typed_message_score in scored.
      rewrite typed in scored.
      discriminate scored.
    + pose proof (typed_score_exposes_type 1 child score scored) as typed_one.
      unfold source_evaluate_node in evaluated.
      destruct (raw_field_of_source body (SourceMessage span field nested))
        as [child_raw|] eqn:converted; [|discriminate evaluated].
      pose proof (source_evaluate_message_retains_raw fuel hash_fn child_raw
        child evaluated) as retained.
      destruct fuel as [|smaller]; [discriminate evaluated|].
      assert (par_type : raw_program_type_id child_raw = Some 1).
      { rewrite <- retained. exact typed_one. }
      pose proof (par_identity_bytes_project_of_evaluated smaller hash_fn
        child_raw child evaluated par_type) as same_identity.
      change (source_summary_body child)
        with (raw_body (source_summary_raw child)).
      rewrite retained.
      exact same_identity.
    + destruct (evaluated_auxiliary_node_is_not_message
        _ hash_fn body program span field child evaluated)
        as [_ value_shape].
      unfold source_payload_typed_message_score in scored.
      destruct (source_summary_value child)
        as [some_raw some_scalar|some_raw some_nested
          |some_raw some_key some_value|some_raw some_snapshot some_values];
        cbn [value_is_message] in value_shape;
        first [discriminate value_shape|discriminate scored].
Qed.

Lemma evaluated_children_keyed_facts :
  forall fuel hash_fn family program body nodes children,
    lookup_projected_program family [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    Forall keyed_child_facts children.
Proof.
  intros fuel hash_fn family program body nodes children
    looked_up decoded selected.
  pose proof (generated_selected_children_are_aligned program
    (source_evaluate_node (source_evaluate_message fuel hash_fn)
      hash_fn body program) nodes children
    (decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
    (selected_source_evaluator_outputs_are_positional _ _ _ _ _ selected))
    as aligned.
  eapply forall2_right_forall; [|exact aligned].
  intros node child [matched evaluated].
  exact (aligned_child_keyed_facts fuel hash_fn body family program node child
    looked_up matched evaluated).
Qed.

(** ** The ESet check *)

Lemma eset_key_relation :
  forall child,
    keyed_child_facts child ->
    eset_element_key (project_source_summary child) =
    option_map (fun key : (source_score * list nat) * list nat =>
      (project_score (fst (fst key)), snd (fst key)))
      (source_element_order_key child).
Proof.
  intros child [typed_scores _].
  unfold eset_element_key, source_element_order_key.
  rewrite (typed_scores 1).
  destruct (source_payload_typed_message_score 1 child); reflexivity.
Qed.

Lemma element_identities_agree :
  forall selected keyed,
    Forall keyed_child_facts selected ->
    Forall2 (fun child key => source_element_order_key child = Some key)
      selected keyed ->
    map (fun child => par_identity_bytes (sem_body (project_source_summary child)))
      selected =
    map snd keyed.
Proof.
  intros selected keyed facts related.
  induction related as [|child key selected' keyed' head_related tail_related IH];
    [reflexivity|].
  cbn [map].
  rewrite (IH (Forall_inv_tail facts)).
  f_equal.
  unfold source_element_order_key in head_related.
  destruct (source_payload_typed_message_score 1 child) as [score|] eqn:scored;
    [|discriminate head_related].
  injection head_related as same_key.
  subst key.
  destruct (Forall_inv facts) as [_ same_identity].
  change (sem_body (project_source_summary child))
    with (source_summary_body child).
  exact (same_identity score scored).
Qed.

Lemma canonical_eset_projects :
  forall children,
    Forall keyed_child_facts children ->
    canonical_eset_children (map project_source_summary children) =
    source_collection_canonical source_element_order_key children.
Proof.
  intros children facts.
  rewrite canonical_eset_children_unfold, source_collection_canonical_unfold.
  rewrite filter_tag_projects.
  remember (filter (fun child => source_summary_tag child =? 1) children)
    as selected eqn:selected_is.
  assert (selected_facts : Forall keyed_child_facts selected).
  { rewrite selected_is. apply forall_filter_preserved. exact facts. }
  assert (target_sequence :
    sequence_source_results
      (map eset_element_key (map project_source_summary selected)) =
    option_map
      (map (fun key : (source_score * list nat) * list nat =>
        (project_score (fst (fst key)), snd (fst key))))
      (sequence_source_results (map source_element_order_key selected))).
  { rewrite map_map.
    apply sequence_map_option_map.
    apply Forall_forall.
    intros child member.
    rewrite Forall_forall in selected_facts.
    exact (eset_key_relation child (selected_facts child member)). }
  rewrite target_sequence.
  destruct (sequence_source_results (map source_element_order_key selected))
    as [keyed|] eqn:sequenced; [|reflexivity].
  pose proof (selected_source_evaluator_outputs_are_positional _ _
    source_element_order_key selected keyed sequenced) as positional.
  cbn [option_map].
  assert (same_keys :
    map (fun key : (source_score * list nat) * list nat =>
      (project_score (fst (fst key)), snd (fst key))) keyed =
    map (fun key : source_score * list nat =>
      (project_score (fst key), snd key)) (map fst keyed)).
  { rewrite map_map. reflexivity. }
  assert (same_identities :
    map (fun child => par_identity_bytes (sem_body child))
      (map project_source_summary selected) =
    map snd keyed).
  { rewrite map_map.
    exact (element_identities_agree selected keyed selected_facts positional). }
  rewrite same_keys, ascending_keys_project, same_identities,
    identities_unique_project.
  reflexivity.
Qed.

(** ** The EMap check *)

Lemma kvp_carrier_parts_project :
  forall child,
    keyed_child_facts child ->
    kvp_carrier_parts (project_source_summary child) =
    option_map (fun parts : source_score * list nat =>
      (project_score (fst parts), snd parts))
      (source_kvp_carrier_parts child).
Proof.
  intros child [typed_scores _].
  unfold kvp_carrier_parts, source_kvp_carrier_parts.
  rewrite (typed_scores 34).
  destruct (source_payload_typed_message_score 34 child) as [score|];
    [|reflexivity].
  destruct score as [value|text|bytes|forest]; try reflexivity.
  destruct forest as [|head tail]; try reflexivity.
  destruct head as [constructor|head_text|head_bytes|head_forest];
    try reflexivity.
  destruct tail as [|key_score tail]; try reflexivity.
  destruct tail as [|identity_score tail]; try reflexivity.
  destruct identity_score
    as [identity_value|identity_text|identity_bytes|identity_forest];
    try reflexivity.
  destruct tail as [|next tail]; try reflexivity.
  cbn [option_map project_score project_score_forest].
  destruct (N.eqb constructor 0); reflexivity.
Qed.

Lemma emap_key_relation :
  forall child,
    keyed_child_facts child ->
    emap_pair_key (project_source_summary child) =
    option_map (fun key : (source_score * list nat) * list nat =>
      ((project_score (fst (fst key)), snd (fst key)), snd key))
      (source_pair_order_key child).
Proof.
  intros child facts.
  unfold emap_pair_key, source_pair_order_key.
  rewrite (kvp_carrier_parts_project child facts).
  destruct (source_kvp_carrier_parts child); reflexivity.
Qed.

Lemma canonical_emap_projects :
  forall children,
    Forall keyed_child_facts children ->
    canonical_emap_children (map project_source_summary children) =
    source_collection_canonical source_pair_order_key children.
Proof.
  intros children facts.
  rewrite canonical_emap_children_unfold, source_collection_canonical_unfold.
  rewrite filter_tag_projects.
  remember (filter (fun child => source_summary_tag child =? 1) children)
    as selected eqn:selected_is.
  assert (selected_facts : Forall keyed_child_facts selected).
  { rewrite selected_is. apply forall_filter_preserved. exact facts. }
  assert (target_sequence :
    sequence_source_results
      (map emap_pair_key (map project_source_summary selected)) =
    option_map
      (map (fun key : (source_score * list nat) * list nat =>
        ((project_score (fst (fst key)), snd (fst key)), snd key)))
      (sequence_source_results (map source_pair_order_key selected))).
  { rewrite map_map.
    apply sequence_map_option_map.
    apply Forall_forall.
    intros child member.
    rewrite Forall_forall in selected_facts.
    exact (emap_key_relation child (selected_facts child member)). }
  rewrite target_sequence.
  destruct (sequence_source_results (map source_pair_order_key selected))
    as [keyed|]; [|reflexivity].
  cbn [option_map].
  assert (same_keys :
    map fst (map (fun key : (source_score * list nat) * list nat =>
      ((project_score (fst (fst key)), snd (fst key)), snd key)) keyed) =
    map (fun key : source_score * list nat =>
      (project_score (fst key), snd key)) (map fst keyed)).
  { rewrite !map_map. reflexivity. }
  assert (same_identities :
    map snd (map (fun key : (source_score * list nat) * list nat =>
      ((project_score (fst (fst key)), snd (fst key)), snd key)) keyed) =
    map snd keyed).
  { rewrite map_map. reflexivity. }
  rewrite same_keys, ascending_keys_project, same_identities,
    identities_unique_project.
  reflexivity.
Qed.

(** ** The bridge *)

Theorem fixed_point_refusal_projects :
  forall fuel hash_fn family program body nodes children complete,
    lookup_projected_program family [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    payload_fixed_point_violated family complete
      (map project_source_summary children) =
    source_collection_outside_fixed_point family complete children.
Proof.
  intros fuel hash_fn family program body nodes children complete
    looked_up decoded selected.
  pose proof (evaluated_children_keyed_facts fuel hash_fn family program body
    nodes children looked_up decoded selected) as facts.
  clear looked_up.
  unfold payload_fixed_point_violated, source_collection_outside_fixed_point.
  do 29 (destruct family as [|family]; [reflexivity|]).
  destruct family as [|family].
  - destruct complete as [[|]|].
    + rewrite (canonical_eset_projects children facts).
      reflexivity.
    + reflexivity.
    + reflexivity.
  - destruct family as [|family].
    + destruct complete as [[|]|].
      * rewrite (canonical_emap_projects children facts).
        reflexivity.
      * reflexivity.
      * reflexivity.
    + reflexivity.
Qed.

(** ** Red control

    A mutant whose ESet uniqueness compares only adjacent identities.  The
    control is a real ESet body: three Par elements whose root locally_free
    fields ascend, so the raw bodies ascend while the scores stay equal; the
    first and third elements share the nested [locally_free] value [7] and
    therefore one identity, and the middle element differs.  The genuine
    target and the oracle report a violation; the mutant accepts the body, so
    the bridge statement fails for the mutant. *)

Fixpoint adjacent_identities_unique (identities : list (list nat)) : bool :=
  match identities with
  | [] | [_] => true
  | first :: ((second :: _) as rest) =>
      negb (nat_list_eqb first second) && adjacent_identities_unique rest
  end.

Definition mutant_canonical_eset_children (children : list semantic_summary)
    : option bool :=
  let elements := filter (fun child => sem_tag child =? 1) children in
  option_map (fun keys =>
    payload_keys_strictly_ascending keys &&
    adjacent_identities_unique
      (map (fun child => par_identity_bytes (sem_body child)) elements))
    (sequence_source_results (map eset_element_key elements)).

Definition mutant_payload_fixed_point_violated (type_id : nat)
    (complete : option bool) (children : list semantic_summary) : bool :=
  match type_id, complete with
  | 29, Some true =>
      order_definitely_bad (mutant_canonical_eset_children children)
  | 30, Some true =>
      order_definitely_bad (canonical_emap_children children)
  | _, _ => false
  end.

(** The bridge statement, for an arbitrary candidate checker. *)
Definition fixed_point_refusal_statement
    (checker : nat -> option bool -> list semantic_summary -> bool) : Prop :=
  forall fuel hash_fn family program body nodes children complete,
    lookup_projected_program family [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    checker family complete (map project_source_summary children) =
    source_collection_outside_fixed_point family complete children.

Corollary genuine_checker_satisfies_the_bridge :
  fixed_point_refusal_statement payload_fixed_point_violated.
Proof. exact fixed_point_refusal_projects. Qed.

Definition control_eset_body : list nat :=
  [10; 10; 74; 1; 1; 90; 5; 10; 3; 74; 1; 7;
   10; 10; 74; 1; 2; 90; 5; 10; 3; 74; 1; 8;
   10; 10; 74; 1; 3; 90; 5; 10; 3; 74; 1; 7].

Definition control_fuel : nat := 30.

Definition control_hash_fn : list nat -> nat := fun _ => 0.

Definition control_program : generated_program :=
  match lookup_projected_program 29 [] [] with
  | Some program => program
  | None => empty_program
  end.

Definition control_nodes : list source_tree :=
  match decode_region (S (length control_eset_body)) control_eset_body
      control_program 0 (length control_eset_body) with
  | Some nodes => nodes
  | None => []
  end.

Definition control_children : list source_exact_summary :=
  match sequence_source_results
      (map (source_evaluate_node
        (source_evaluate_message control_fuel control_hash_fn)
        control_hash_fn control_eset_body control_program) control_nodes) with
  | Some children => children
  | None => []
  end.

Lemma control_lookup :
  lookup_projected_program 29 [] [] = Some control_program.
Proof. vm_compute. reflexivity. Qed.

Lemma control_decode :
  decode_region (S (length control_eset_body)) control_eset_body
    control_program 0 (length control_eset_body) = Some control_nodes.
Proof. vm_compute. reflexivity. Qed.

Lemma control_evaluate :
  sequence_source_results
    (map (source_evaluate_node
      (source_evaluate_message control_fuel control_hash_fn)
      control_hash_fn control_eset_body control_program) control_nodes) =
    Some control_children.
Proof. vm_compute. reflexivity. Qed.

(** The three elements have raw bodies in ascending order and the identities
    I, J, I: the repeat is not adjacent. *)
Example control_identities_repeat_without_being_adjacent :
  map source_summary_body control_children =
    [[74; 1; 1; 90; 5; 10; 3; 74; 1; 7];
     [74; 1; 2; 90; 5; 10; 3; 74; 1; 8];
     [74; 1; 3; 90; 5; 10; 3; 74; 1; 7]] /\
  map (fun child => source_par_identity_bytes (source_summary_value child))
    control_children =
    [[90; 5; 10; 3; 74; 1; 7];
     [90; 5; 10; 3; 74; 1; 8];
     [90; 5; 10; 3; 74; 1; 7]].
Proof. vm_compute. repeat split. Qed.

(** The order leg passes for the mutant, which is why it accepts the body;
    the genuine target and the oracle both report the violation. *)
Example control_fixed_point_outcomes :
  mutant_canonical_eset_children
    (map project_source_summary control_children) = Some true /\
  canonical_eset_children
    (map project_source_summary control_children) = Some false /\
  source_collection_canonical source_element_order_key control_children =
    Some false /\
  mutant_payload_fixed_point_violated 29 (Some true)
    (map project_source_summary control_children) = false /\
  payload_fixed_point_violated 29 (Some true)
    (map project_source_summary control_children) = true /\
  source_collection_outside_fixed_point 29 (Some true) control_children = true.
Proof. vm_compute. repeat split. Qed.

Theorem adjacent_only_mutant_fails_the_bridge :
  ~ fixed_point_refusal_statement mutant_payload_fixed_point_violated.
Proof.
  intro holds.
  pose proof (holds control_fuel control_hash_fn 29 control_program
    control_eset_body control_nodes control_children (Some true)
    control_lookup control_decode control_evaluate) as agreement.
  vm_compute in agreement.
  discriminate agreement.
Qed.

Print Assumptions body_leq_total.
Print Assumptions nat_list_eqb_is_mutual_leq.
Print Assumptions nat_list_eqb_matches_source_bytes_eqb.
Print Assumptions existsb_pointwise.
Print Assumptions identities_unique_project.
Print Assumptions strict_before_case.
Print Assumptions strict_before_projects.
Print Assumptions payload_ascending_unfold_pair.
Print Assumptions source_ascending_unfold_pair.
Print Assumptions ascending_keys_project.
Print Assumptions filter_tag_projects.
Print Assumptions forall_filter_preserved.
Print Assumptions sequence_map_option_map.
Print Assumptions canonical_eset_children_unfold.
Print Assumptions canonical_emap_children_unfold.
Print Assumptions source_collection_canonical_unfold.
Print Assumptions forall2_right_forall.
Print Assumptions evaluated_auxiliary_node_is_not_message.
Print Assumptions typed_message_score_projects.
Print Assumptions typed_score_exposes_type.
Print Assumptions aligned_child_keyed_facts.
Print Assumptions evaluated_children_keyed_facts.
Print Assumptions eset_key_relation.
Print Assumptions element_identities_agree.
Print Assumptions canonical_eset_projects.
Print Assumptions kvp_carrier_parts_project.
Print Assumptions emap_key_relation.
Print Assumptions canonical_emap_projects.
Print Assumptions fixed_point_refusal_projects.
Print Assumptions genuine_checker_satisfies_the_bridge.
Print Assumptions control_lookup.
Print Assumptions control_decode.
Print Assumptions control_evaluate.
Print Assumptions control_identities_repeat_without_being_adjacent.
Print Assumptions control_fixed_point_outcomes.
Print Assumptions adjacent_only_mutant_fails_the_bridge.
