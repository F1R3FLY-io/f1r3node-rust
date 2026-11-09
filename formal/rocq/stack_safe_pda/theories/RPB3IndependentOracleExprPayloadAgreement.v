(** * Agreement of the two reducers on the expression payload families

    Leaf L12 of the score-arms design.  The target's arms and the oracle's
    fall-throughs of [source_expr_payload_family_score] and
    [source_expr_payload_completeness] were added together; this theory proves
    the three facts the message assembly asks of a family, for the 29 payload
    families, on the children the oracle actually evaluates.

    Family groups (design section 3):
    - [6; 27; 28; 29; 30; 33; 35 .. 54] score only when complete; each is one
      bridge theorem of leaves L03-L08 ([scored_payload_bridge_facts]);
    - [34] is the EMap pair carrier: it scores only when complete and its
      bridge is the second half of the EMap bridge;
    - [56] and [57] are the VarRef and ConnectiveBody carriers: they score
      regardless of completeness and have no completeness of their own.

    Refusal: for these families the only refusal disjunct that can fire is the
    fixed-point disjunct added by the flip, the oracle's
    [source_collection_outside_fixed_point] on one side and the target's
    [payload_fixed_point_violated] on the other (leaf L11 equates them).

    The assembly imports this theory; this theory must therefore not import
    RPB3IndependentOracleMessageAssembly. *)

From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SemanticReducer RPB3RecursiveAuxiliarySourceReference
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleRecursive RPB3IndependentOracleProjection
  RPB3IndependentOracleExprPayload
  RPB3IndependentOracleVarScoreBridge
  RPB3IndependentOracleUnaryExprScoreBridge
  RPB3IndependentOracleBinaryExprScoreBridge
  RPB3IndependentOracleOrderedCollectionScoreBridge
  RPB3IndependentOracleEMapCarrierScoreBridge
  RPB3IndependentOracleEMethodScoreBridge
  RPB3IndependentOracleConnectiveArmScoreBridge
  RPB3IndependentOracleFixedPointRefusalBridge.

Import ListNotations.

Local Notation projected children :=
  (map RPB3IndependentOracleProjection.project_source_summary children)
  (only parsing).
Local Notation source_scan program children :=
  (RPB3IndependentOracleAdmission.source_scan_children program children)
  (only parsing).
Local Notation target_scan program children :=
  (fold_left (RPB3SemanticReducer.scan_child program)
    (map RPB3IndependentOracleProjection.project_source_summary children)
    RPB3GeneratedFieldRefinement.empty_scan)
  (only parsing).

(** The expression payload families: every type id given a target arm by the
    flip. *)
Definition expr_payload_family_ids : list nat :=
  [6; 27; 28; 29; 30; 33; 34; 35; 36; 37; 38; 39; 40; 41; 42; 43; 44; 45; 46;
   47; 48; 49; 50; 51; 52; 53; 54; 56; 57].

(** The payload families whose score is produced only for complete children
    and read through [source_expr_payload_score]: all but the carriers
    34, 56 and 57. *)
Definition scored_payload_family_ids : list nat :=
  [6; 27; 28; 29; 30; 33; 35; 36; 37; 38; 39; 40; 41; 42; 43; 44; 45; 46; 47;
   48; 49; 50; 51; 52; 53; 54].

(** ** The oracle's fall-throughs on the payload families

    A payload literal is on no arm of the Par, Cost, Process and Name
    dispatches, so both oracle functions reduce to their payload fall-through
    by computation on the literal. *)

Lemma message_completeness_on_payload_family :
  forall family scan children,
    In family expr_payload_family_ids ->
    RPB3IndependentOracleRecursive.source_message_completeness family scan children =
    RPB3IndependentOracleExprPayload.source_expr_payload_completeness
      family scan children.
Proof.
  intros family scan children member.
  unfold expr_payload_family_ids in member.
  repeat (destruct member as [same|member]; [subst family; reflexivity|]).
  exfalso. exact member.
Qed.

Lemma message_score_on_payload_family :
  forall family complete children,
    In family expr_payload_family_ids ->
    RPB3IndependentOracleRecursive.source_message_score family complete children =
    RPB3IndependentOracleExprPayload.source_expr_payload_family_score
      family complete children.
Proof.
  intros family complete children member.
  unfold expr_payload_family_ids in member.
  repeat (destruct member as [same|member];
    [subst family; destruct complete as [[|]|]; reflexivity|]).
  exfalso. exact member.
Qed.

Lemma message_extra_refused_on_payload_family :
  forall family complete scan children,
    In family expr_payload_family_ids ->
    RPB3IndependentOracleRecursive.source_message_extra_refused family complete
      scan children =
    RPB3IndependentOracleRecursive.source_collection_outside_fixed_point family
      complete children.
Proof.
  intros family complete scan children member.
  unfold expr_payload_family_ids in member.
  repeat (destruct member as [same|member]; [subst family; reflexivity|]).
  exfalso. exact member.
Qed.

(** ** The target on the payload families *)

Lemma component_completeness_on_payload_family :
  forall family state children,
    In family expr_payload_family_ids ->
    RPB3SemanticReducer.component_completeness family state children =
    RPB3SemanticReducer.expr_payload_completeness family state children.
Proof.
  intros family state children member.
  unfold expr_payload_family_ids in member.
  repeat (destruct member as [same|member]; [subst family; reflexivity|]).
  exfalso. exact member.
Qed.

(** Of the four refusal disjuncts of the target, only the fixed-point one can
    fire on a payload family. *)
Lemma target_refusal_on_payload_family :
  forall family state complete children,
    In family expr_payload_family_ids ->
    (RPB3SemanticReducer.order_definitely_bad
       (if family =? 1 then RPB3SemanticReducer.raw_par_component_order children
        else if (family =? 20) && RPB3SemanticReducer.seen_slot state 1
        then Some (RPB3SemanticReducer.new_uri_ordered children)
        else Some true) ||
     RPB3SemanticReducer.complete_receive_has_default_condition family complete
       children ||
     RPB3SemanticReducer.complete_match_has_default_guard family complete
       children ||
     RPB3SemanticReducer.payload_fixed_point_violated family complete children)
    = RPB3SemanticReducer.payload_fixed_point_violated family complete children.
Proof.
  intros family state complete children member.
  unfold expr_payload_family_ids in member.
  repeat (destruct member as [same|member]; [subst family; reflexivity|]).
  exfalso. exact member.
Qed.

(** ** Bridge facts of the scored families *)

Lemma exact_or_unknown_of_projected_score :
  forall (target_score : option RPB3SemanticReducer.canonical_score)
      (source_score : option RPB3IndependentOracleCore.source_score),
    target_score =
      option_map RPB3IndependentOracleCore.project_score source_score ->
    RPB3IndependentOracleProjection.project_source_evidence source_score =
    RPB3SemanticReducer.exact_or_unknown target_score.
Proof.
  intros target_score [score|] same; subst target_score; reflexivity.
Qed.

(** For a scored family the score and completeness bridges of leaves L03-L08
    apply; [exact] checks that the family-specific helper of each bridge is the
    dispatch's image at the literal. *)
Lemma scored_payload_bridge_facts :
  forall fuel hash_fn family program body nodes children,
    In family scored_payload_family_ids ->
    RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] =
      Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0
      (length body) = Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn)
             hash_fn body program)
        nodes) = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (source_scan program children) = true ->
    RPB3SemanticReducer.exact_expr_payload_score family (projected children) =
      option_map RPB3IndependentOracleCore.project_score
        (RPB3IndependentOracleExprPayload.source_expr_payload_score family
          children) /\
    RPB3SemanticReducer.expr_payload_completeness family
      (target_scan program children) (projected children) =
    RPB3IndependentOracleExprPayload.source_expr_payload_completeness family
      (source_scan program children) children.
Proof.
  intros fuel hash_fn family program body nodes children member looked_up
    decoded sequenced valid.
  destruct (in_dec Nat.eq_dec family [35; 36; 37]) as [unary|not_unary].
  { exact (RPB3IndependentOracleUnaryExprScoreBridge.unary_payload_projects
      fuel hash_fn family program body nodes children unary looked_up decoded
      sequenced valid). }
  destruct (in_dec Nat.eq_dec family
      [38; 39; 40; 41; 42; 43; 44; 45; 46; 47; 48; 49; 50; 51; 52; 53; 54])
    as [binary|not_binary].
  { exact (RPB3IndependentOracleBinaryExprScoreBridge.binary_payload_projects
      fuel hash_fn family program body nodes children binary looked_up decoded
      sequenced valid). }
  unfold scored_payload_family_ids in member.
  destruct member as
    [same_var|[same_list|[same_tuple|[same_set|[same_map|[same_method|rest]]]]]].
  - subst family.
    exact (RPB3IndependentOracleVarScoreBridge.var_score_projects fuel hash_fn
      program body nodes children looked_up decoded sequenced valid).
  - subst family.
    exact (RPB3IndependentOracleOrderedCollectionScoreBridge.ordered_collection_projects
      fuel hash_fn 27 6%N true program body nodes children (or_introl eq_refl)
      looked_up decoded sequenced valid).
  - subst family.
    exact (RPB3IndependentOracleOrderedCollectionScoreBridge.ordered_collection_projects
      fuel hash_fn 28 7%N false program body nodes children
      (or_intror (or_introl eq_refl)) looked_up decoded sequenced valid).
  - subst family.
    exact (RPB3IndependentOracleOrderedCollectionScoreBridge.ordered_collection_projects
      fuel hash_fn 29 8%N true program body nodes children
      (or_intror (or_intror (or_introl eq_refl))) looked_up decoded sequenced
      valid).
  - subst family.
    exact (proj1 (RPB3IndependentOracleEMapCarrierScoreBridge.emap_payload_projects
      fuel hash_fn program body nodes children) looked_up decoded sequenced valid).
  - subst family.
    exact (RPB3IndependentOracleEMethodScoreBridge.emethod_payload_projects
      fuel hash_fn program body nodes children looked_up decoded sequenced valid).
  - exfalso.
    change (In family ([35; 36; 37] ++
      [38; 39; 40; 41; 42; 43; 44; 45; 46; 47; 48; 49; 50; 51; 52; 53; 54]))
      in rest.
    destruct (in_app_or _ _ _ rest) as [unary|binary].
    + exact (not_unary unary).
    + exact (not_binary binary).
Qed.

(** ** The score dispatch of the scored families

    Both dispatches of a scored family read the payload score exactly when the
    completeness is [Some true]. *)

Lemma scored_source_family_score_unfolds :
  forall family complete children,
    In family scored_payload_family_ids ->
    RPB3IndependentOracleExprPayload.source_expr_payload_family_score family
      complete children =
    match complete with
    | Some true =>
        RPB3IndependentOracleExprPayload.source_expr_payload_score family
          children
    | _ => None
    end.
Proof.
  intros family complete children member.
  unfold scored_payload_family_ids in member.
  repeat (destruct member as [same|member];
    [subst family; destruct complete as [[|]|]; reflexivity|]).
  exfalso. exact member.
Qed.

Lemma scored_target_evidence_unfolds :
  forall family complete children,
    In family scored_payload_family_ids ->
    RPB3SemanticReducer.score_evidence_for family complete children =
    match complete with
    | Some true =>
        RPB3SemanticReducer.exact_or_unknown
          (RPB3SemanticReducer.exact_expr_payload_score family children)
    | _ => RPB3SemanticReducer.ScoreNotDerived
    end.
Proof.
  intros family complete children member.
  unfold scored_payload_family_ids in member.
  repeat (destruct member as [same|member];
    [subst family; destruct complete as [[|]|]; reflexivity|]).
  exfalso. exact member.
Qed.

Lemma scored_payload_score_agrees :
  forall fuel hash_fn family program body nodes children complete,
    In family scored_payload_family_ids ->
    RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] =
      Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0
      (length body) = Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn)
             hash_fn body program)
        nodes) = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (source_scan program children) = true ->
    RPB3IndependentOracleProjection.project_source_evidence
      (RPB3IndependentOracleExprPayload.source_expr_payload_family_score family
        complete children) =
    RPB3SemanticReducer.score_evidence_for family complete (projected children).
Proof.
  intros fuel hash_fn family program body nodes children complete member
    looked_up decoded sequenced valid.
  destruct (scored_payload_bridge_facts fuel hash_fn family program body nodes
    children member looked_up decoded sequenced valid) as [score _].
  rewrite (scored_source_family_score_unfolds family complete children member).
  rewrite (scored_target_evidence_unfolds family complete (projected children)
    member).
  destruct complete as [[|]|].
  - exact (exact_or_unknown_of_projected_score _ _ score).
  - reflexivity.
  - reflexivity.
Qed.

(** ** All payload families

    The scored families are handled above, and the carriers 34, 56 and 57
    are the only remaining members of [expr_payload_family_ids]. *)
Lemma payload_family_facts :
  forall fuel hash_fn family program body nodes children,
    In family expr_payload_family_ids ->
    RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] =
      Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0
      (length body) = Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn)
             hash_fn body program)
        nodes) = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (source_scan program children) = true ->
    RPB3SemanticReducer.expr_payload_completeness family
      (target_scan program children) (projected children) =
    RPB3IndependentOracleExprPayload.source_expr_payload_completeness family
      (source_scan program children) children /\
    (forall complete,
      RPB3IndependentOracleProjection.project_source_evidence
        (RPB3IndependentOracleExprPayload.source_expr_payload_family_score family
          complete children) =
      RPB3SemanticReducer.score_evidence_for family complete
        (projected children)).
Proof.
  intros fuel hash_fn family program body nodes children member looked_up
    decoded sequenced valid.
  destruct (in_dec Nat.eq_dec family scored_payload_family_ids)
    as [scored|not_scored].
  - split.
    + exact (proj2 (scored_payload_bridge_facts fuel hash_fn family program body
        nodes children scored looked_up decoded sequenced valid)).
    + intro complete.
      exact (scored_payload_score_agrees fuel hash_fn family program body nodes
        children complete scored looked_up decoded sequenced valid).
  - unfold expr_payload_family_ids in member.
    assert (carrier : family = 34 \/ family = 56 \/ family = 57).
    { repeat (destruct member as [same|member];
        [ subst family;
          first
            [ (left; reflexivity)
            | (right; left; reflexivity)
            | (right; right; reflexivity)
            | (exfalso; apply not_scored;
               unfold scored_payload_family_ids; cbn [In]; tauto) ]
        | ]).
      exfalso. exact member. }
    destruct carrier as [same|[same|same]]; subst family.
    + destruct (proj2
        (RPB3IndependentOracleEMapCarrierScoreBridge.emap_payload_projects
          fuel hash_fn program body nodes children) looked_up decoded sequenced
          valid) as [kvp complete_bridge].
      split.
      * exact complete_bridge.
      * intros [[|]|].
        -- exact (exact_or_unknown_of_projected_score
             (RPB3SemanticReducer.exact_kvp_carrier (projected children))
             (RPB3IndependentOracleExprPayload.source_kvp_carrier children)
             kvp).
        -- reflexivity.
        -- reflexivity.
    + destruct (RPB3IndependentOracleConnectiveArmScoreBridge.connective_message_arm_projects
        fuel hash_fn 56 program body nodes children looked_up decoded sequenced
        valid) as [_ [_ var_ref]].
      split.
      * reflexivity.
      * intro complete.
        exact (exact_or_unknown_of_projected_score
          (RPB3SemanticReducer.exact_var_ref_carrier (projected children))
          (RPB3IndependentOracleExprPayload.source_var_ref_carrier children)
          (var_ref eq_refl)).
    + destruct (RPB3IndependentOracleConnectiveArmScoreBridge.connective_message_arm_projects
        fuel hash_fn 57 program body nodes children looked_up decoded sequenced
        valid) as [_ [body_carrier _]].
      split.
      * reflexivity.
      * intro complete.
        exact (exact_or_unknown_of_projected_score
          (RPB3SemanticReducer.exact_connective_body_carrier (projected children))
          (RPB3IndependentOracleExprPayload.source_connective_body_carrier
            children)
          (body_carrier eq_refl)).
Qed.

(** ** The agreement the message assembly asks of a family

    The three conjuncts are the three fields of the assembly's
    [family_agreement], at the scan state of the given children. *)
Theorem expr_payload_family_agreement :
  forall fuel hash_fn family program body nodes children,
    In family expr_payload_family_ids ->
    RPB3GeneratedFieldRefinement.lookup_projected_program family [] [] =
      Some program ->
    RPB3RawRegionTree.decode_region (S (length body)) body program 0
      (length body) = Some nodes ->
    RPB3RawRegionTree.sequence_source_results
      (map (RPB3IndependentOracleRecursive.source_evaluate_node
             (RPB3IndependentOracleRecursive.source_evaluate_message fuel hash_fn)
             hash_fn body program)
        nodes) = Some children ->
    RPB3IndependentOracleAdmission.source_scan_valid
      (source_scan program children) = true ->
    RPB3IndependentOracleRecursive.source_message_completeness family
      (source_scan program children) children =
    RPB3SemanticReducer.component_completeness family
      (target_scan program children) (projected children) /\
    RPB3IndependentOracleProjection.project_source_evidence
      (RPB3IndependentOracleRecursive.source_message_score family
        (RPB3IndependentOracleRecursive.source_message_completeness family
          (source_scan program children) children) children) =
    RPB3SemanticReducer.score_evidence_for family
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)
      (projected children) /\
    RPB3IndependentOracleRecursive.source_message_extra_refused family
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)
      (source_scan program children) children =
    (RPB3SemanticReducer.order_definitely_bad
       (if family =? 1
        then RPB3SemanticReducer.raw_par_component_order (projected children)
        else if (family =? 20) &&
                RPB3SemanticReducer.seen_slot (target_scan program children) 1
        then Some (RPB3SemanticReducer.new_uri_ordered (projected children))
        else Some true) ||
     RPB3SemanticReducer.complete_receive_has_default_condition family
       (RPB3IndependentOracleRecursive.source_message_completeness family
         (source_scan program children) children)
       (projected children) ||
     RPB3SemanticReducer.complete_match_has_default_guard family
       (RPB3IndependentOracleRecursive.source_message_completeness family
         (source_scan program children) children)
       (projected children) ||
     RPB3SemanticReducer.payload_fixed_point_violated family
       (RPB3IndependentOracleRecursive.source_message_completeness family
         (source_scan program children) children)
       (projected children)).
Proof.
  intros fuel hash_fn family program body nodes children member looked_up
    decoded sequenced valid.
  destruct (payload_family_facts fuel hash_fn family program body nodes children
    member looked_up decoded sequenced valid) as [complete_bridge score_bridge].
  repeat split.
  - rewrite (message_completeness_on_payload_family family
      (source_scan program children) children member).
    rewrite (component_completeness_on_payload_family family
      (target_scan program children) (projected children) member).
    symmetry.
    exact complete_bridge.
  - rewrite (message_score_on_payload_family family
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children) children member).
    exact (score_bridge
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)).
  - rewrite (message_extra_refused_on_payload_family family
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)
      (source_scan program children) children member).
    rewrite (target_refusal_on_payload_family family
      (target_scan program children)
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)
      (projected children) member).
    symmetry.
    exact (RPB3IndependentOracleFixedPointRefusalBridge.fixed_point_refusal_projects
      fuel hash_fn family program body nodes children
      (RPB3IndependentOracleRecursive.source_message_completeness family
        (source_scan program children) children)
      looked_up decoded sequenced).
Qed.

(** ** Red control of the flip

    The Par body [[42; 2; 42; 0]] holds an Expr whose ENot arm is present and
    empty.  Before the flip this Par had no score: the ENot child had no
    completeness, so neither reducer derived an exact Par score, which is why
    RPB3IndependentOracleParAnswer used it as its unscored PathMap value.  With
    the arms the ENot child is known incomplete (its required body is absent),
    so the Expr parent is incomplete, and an incomplete Expr carries the
    non-semantic exact score: both reducers now answer an exact Par score.  The
    second control is the body that
    replaced it: it is rejected by both reducers, and the arms do not change
    that. *)

Example flip_red_control_previously_unscored_body_now_scores :
  exists source_summary target_summary,
    RPB3IndependentOracleRecursive.source_par_reference (fun _ => 0)
      [42; 2; 42; 0] = Some (false, source_summary) /\
    RPB3RecursiveAuxiliarySourceReference.recursive_par_reference (fun _ => 0)
      [42; 2; 42; 0] = Some (false, target_summary) /\
    option_map RPB3IndependentOracleCore.project_score
      (RPB3IndependentOracleCore.source_summary_exact_score source_summary) =
    Some (RPB3SemanticReducer.score_constructor 999
      [RPB3SemanticReducer.score_constructor 2147483647 [];
       RPB3SemanticReducer.ScoreInteger 0]) /\
    RPB3SemanticReducer.sem_score_evidence target_summary =
    RPB3SemanticReducer.ExactScore
      (RPB3SemanticReducer.score_constructor 999
        [RPB3SemanticReducer.score_constructor 2147483647 [];
         RPB3SemanticReducer.ScoreInteger 0]).
Proof. vm_compute. repeat eexists; repeat split; reflexivity. Qed.

Example flip_red_control_replacement_body_is_rejected :
  RPB3IndependentOracleRecursive.source_par_reference (fun _ => 0)
    [42; 5; 162; 2; 2; 10; 0] = None /\
  RPB3RecursiveAuxiliarySourceReference.recursive_par_reference (fun _ => 0)
    [42; 5; 162; 2; 2; 10; 0] = None.
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions message_completeness_on_payload_family.
Print Assumptions message_score_on_payload_family.
Print Assumptions message_extra_refused_on_payload_family.
Print Assumptions component_completeness_on_payload_family.
Print Assumptions target_refusal_on_payload_family.
Print Assumptions exact_or_unknown_of_projected_score.
Print Assumptions scored_payload_bridge_facts.
Print Assumptions scored_source_family_score_unfolds.
Print Assumptions scored_target_evidence_unfolds.
Print Assumptions scored_payload_score_agrees.
Print Assumptions payload_family_facts.
Print Assumptions expr_payload_family_agreement.
Print Assumptions flip_red_control_previously_unscored_body_now_scores.
Print Assumptions flip_red_control_replacement_body_is_rejected.
