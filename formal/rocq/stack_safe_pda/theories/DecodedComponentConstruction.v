From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA EPM1.

Import ListNotations.
Set Implicit Arguments.

Module DecodedComponentConstruction.

Definition bytes := list nat.
Definition arena := list bytes.
Definition lookup (values : arena) (handle : nat) : bytes :=
  match nth_error values handle with Some body => body | None => [] end.

Definition field (tag body : bytes) : bytes :=
  tag ++ EPM1.encode_varint (length body) ++ body.

Inductive label :=
| Literal (body : bytes)
| Slot (tag : bytes) (ordinal : nat)
| Delimited (tag : bytes)
| Sequence.

Definition shell := @tree label.

Definition slot_algebra (part : label) (children : list (list nat)) : list nat :=
  match part with
  | Slot _ ordinal => ordinal :: concat children
  | _ => concat children
  end.

Definition slots (subject : shell) := fold_tree slot_algebra subject.

Definition planned_body (values : arena) (handles : list nat) (ordinal : nat) : bytes :=
  match nth_error handles ordinal with
  | Some handle => lookup values handle
  | None => []
  end.

Definition emit_algebra (body_at : nat -> bytes) (part : label)
    (children : list bytes) : bytes :=
  match part with
  | Literal body => body ++ concat children
  | Slot tag ordinal => field tag (body_at ordinal) ++ concat children
  | Delimited tag => field tag (concat children)
  | Sequence => concat children
  end.

Definition resident_children (values : arena) (handles : list nat) : arena :=
  map (lookup values) handles.

Definition resident_emit (values : arena) (subject : shell) (handles : list nat) :=
  fold_tree (emit_algebra (lookup (resident_children values handles))) subject.

Definition planned_emit (values : arena) (subject : shell) (handles : list nat) :=
  fold_tree (emit_algebra (planned_body values handles)) subject.

Lemma planned_body_matches_resident values handles ordinal :
  planned_body values handles ordinal =
  lookup (resident_children values handles) ordinal.
Proof.
  unfold planned_body, lookup, resident_children.
  rewrite nth_error_map. destruct (nth_error handles ordinal); reflexivity.
Qed.

Lemma emission_algebra_respects_lookup left right :
  (forall ordinal, left ordinal = right ordinal) ->
  forall part children,
    emit_algebra left part children = emit_algebra right part children.
Proof. intros same part children. destruct part; cbn; auto. now rewrite same. Qed.

Lemma emission_fold_respects_lookup left right :
  (forall ordinal, left ordinal = right ordinal) ->
  (forall subject, fold_tree (emit_algebra left) subject =
                   fold_tree (emit_algebra right) subject) /\
  (forall subjects, fold_forest (emit_algebra left) subjects =
                    fold_forest (emit_algebra right) subjects).
Proof.
  intros same. apply tree_forest_induction.
  - intros part children IH.
    change (emit_algebra left part (fold_forest (emit_algebra left) children) =
      emit_algebra right part (fold_forest (emit_algebra right) children)).
    rewrite IH. apply emission_algebra_respects_lookup. exact same.
  - reflexivity.
  - intros subject IH subjects IHs.
    change (fold_tree (emit_algebra left) subject :: fold_forest (emit_algebra left) subjects =
      fold_tree (emit_algebra right) subject :: fold_forest (emit_algebra right) subjects).
    now rewrite IH, IHs.
Qed.

Theorem ordered_hole_substitution_is_exact values subject handles :
  planned_emit values subject handles = resident_emit values subject handles.
Proof.
  unfold planned_emit, resident_emit.
  apply (proj1 (emission_fold_respects_lookup
    (planned_body values handles) (lookup (resident_children values handles))
    (planned_body_matches_resident values handles))).
Qed.

Definition emission_machine values subject handles :=
  run (emit_algebra (planned_body values handles)) (compile_tree subject) [].

Theorem worklist_emission_refines_resident_substitution values subject handles :
  emission_machine values subject handles = Some [resident_emit values subject handles].
Proof.
  unfold emission_machine.
  rewrite pda_fold_equivalent_to_recursive_fold.
  change (Some [planned_emit values subject handles] =
    Some [resident_emit values subject handles]).
  now rewrite ordered_hole_substitution_is_exact.
Qed.

Theorem emitted_program_has_one_instruction_per_shell_node (subject : shell) :
  length (compile_tree subject) = tree_nodes subject.
Proof. exact (proj1 compiled_program_has_one_instruction_per_node subject). Qed.

Record plan := Plan {
  plan_shell : shell;
  plan_handles : list nat
}.

Definition slots_exact (subject : shell) (handles : list nat) : bool :=
  if list_eq_dec Nat.eq_dec (slots subject) (seq 0 (length handles))
  then true else false.

Definition handles_valid (values : arena) (handles : list nat) : bool :=
  forallb (fun handle => handle <? length values) handles.

Inductive result := Declined | Rejected | Ready (pending : plan).

Definition prepare (values : arena) (complete : bool) (subject : shell)
    (handles : list nat) : arena * result :=
  (values,
    if complete then Declined
    else if slots_exact subject handles && handles_valid values handles
    then Ready (Plan subject handles)
    else Rejected).

Theorem successful_plan_has_exact_ordered_slots values subject handles pending :
  snd (prepare values false subject handles) = Ready pending ->
  pending = Plan subject handles /\
  slots subject = seq 0 (length handles) /\
  handles_valid values handles = true.
Proof.
  unfold prepare, slots_exact; cbn.
  destruct (list_eq_dec Nat.eq_dec (slots subject) (seq 0 (length handles)))
    as [ordered|unordered]; cbn; [|discriminate].
  destruct (handles_valid values handles) eqn:valid; [|discriminate].
  intros accepted. inversion accepted. subst. auto.
Qed.

Theorem successful_plan_consumes_exactly_all_children values subject handles pending :
  snd (prepare values false subject handles) = Ready pending ->
  length (slots subject) = length handles.
Proof.
  intros accepted.
  destruct (@successful_plan_has_exact_ordered_slots values subject handles pending accepted)
    as [_ [ordered _]].
  rewrite ordered, length_seq. reflexivity.
Qed.

Theorem missing_or_extra_children_are_rejected values subject handles :
  length (slots subject) <> length handles ->
  snd (prepare values false subject handles) = Rejected.
Proof.
  intros different. unfold prepare, slots_exact; cbn.
  destruct (list_eq_dec Nat.eq_dec (slots subject) (seq 0 (length handles)))
    as [ordered|unordered]; cbn; [|reflexivity].
  exfalso. apply different. rewrite ordered, length_seq. reflexivity.
Qed.

Theorem permuted_or_repeated_slots_are_rejected values subject handles :
  slots subject <> seq 0 (length handles) ->
  snd (prepare values false subject handles) = Rejected.
Proof.
  intros different. unfold prepare, slots_exact; cbn.
  destruct (list_eq_dec Nat.eq_dec (slots subject) (seq 0 (length handles)));
    [contradiction|reflexivity].
Qed.

Theorem accepted_handles_refer_to_existing_children values subject handles pending :
  snd (prepare values false subject handles) = Ready pending ->
  forall handle, In handle handles -> exists body, nth_error values handle = Some body.
Proof.
  intros accepted handle member.
  destruct (@successful_plan_has_exact_ordered_slots values subject handles pending accepted)
    as [_ [_ valid]].
  unfold handles_valid in valid. rewrite forallb_forall in valid.
  specialize (valid handle member). apply Nat.ltb_lt in valid.
  destruct (nth_error values handle) eqn:found; [eauto|].
  apply nth_error_None in found. lia.
Qed.

Theorem complete_shell_declines_without_arena_change values subject handles :
  prepare values true subject handles = (values, Declined).
Proof. reflexivity. Qed.

Theorem every_prepare_preserves_prior_arena values complete subject handles :
  fst (prepare values complete subject handles) = values.
Proof. reflexivity. Qed.

Theorem prior_denotations_survive_arena_extension (values extra : arena) handle body :
  nth_error values handle = Some body ->
  nth_error (values ++ extra) handle = Some body.
Proof.
  intros present. rewrite nth_error_app1; [exact present|].
  apply nth_error_Some. rewrite present. discriminate.
Qed.

Inductive event := RetainSlot (handle : nat) | SemanticSort | MaterializeChild.

Definition planning_events pending := map RetainSlot (plan_handles pending).

Theorem modeled_plan_never_sorts_or_materializes pending :
  ~ In SemanticSort (planning_events pending) /\
  ~ In MaterializeChild (planning_events pending).
Proof.
  unfold planning_events. split; intros member;
    apply in_map_iff in member; destruct member as [handle [bad _]]; discriminate.
Qed.

Theorem modeled_plan_retains_each_handle_once pending :
  length (planning_events pending) = length (plan_handles pending).
Proof. unfold planning_events. apply length_map. Qed.

Definition leaf part : shell := Node part FNil.
Definition absent : shell := leaf Sequence.
Definition present_empty : shell := leaf (Slot [10] 0).

Theorem absent_and_present_empty_are_distinct :
  planned_emit [[]] absent [] = [] /\
  planned_emit [[]] present_empty [0] = [10; 0].
Proof. split; reflexivity. Qed.

Record endpoint := Endpoint {
  endpoint_family : nat;
  endpoint_score : bytes;
  endpoint_body : bytes;
  endpoint_score_length : nat;
  endpoint_multiplicity : nat;
  endpoint_locally_free : bytes;
  endpoint_connective : bool
}.

Definition finish values pending family score multiplicity locally_free connective :=
  Endpoint family score
    (planned_emit values (plan_shell pending) (plan_handles pending))
    (length score) multiplicity locally_free connective.

Definition resident_endpoint values subject handles family score multiplicity locally_free connective :=
  Endpoint family score (resident_emit values subject handles)
    (length score) multiplicity locally_free connective.

Theorem final_endpoint_equals_resident values subject handles pending
    family score multiplicity locally_free connective :
  snd (prepare values false subject handles) = Ready pending ->
  finish values pending family score multiplicity locally_free connective =
  resident_endpoint values subject handles family score multiplicity locally_free connective.
Proof.
  intros accepted.
  destruct (@successful_plan_has_exact_ordered_slots values subject handles pending accepted)
    as [-> _].
  change (Endpoint family score (planned_emit values subject handles)
    (length score) multiplicity locally_free connective =
    Endpoint family score (resident_emit values subject handles)
    (length score) multiplicity locally_free connective).
  now rewrite ordered_hole_substitution_is_exact.
Qed.

Theorem successful_plan_uses_supplied_nonsemantic_score values subject handles pending
    family score multiplicity locally_free connective :
  snd (prepare values false subject handles) = Ready pending ->
  endpoint_score (finish values pending family score multiplicity locally_free connective) = score.
Proof. intros. reflexivity. Qed.

Theorem nested_field_length_prefix_round_trips tag body :
  skipn (length tag) (field tag body) = EPM1.encode_varint (length body) ++ body /\
  EPM1.decode_varint (EPM1.encode_varint (length body) ++ body) =
    Some (length body, body).
Proof.
  split.
  - unfold field. apply EPM1.skipn_length_app.
  - apply EPM1.canonical_varint_round_trip.
Qed.

Definition two_slots first second : shell :=
  Node Sequence (FCons (leaf (Slot [10] first))
    (FCons (leaf (Slot [10] second)) FNil)).

Theorem ordered_distinct_handles_are_accepted :
  snd (prepare [[7]; [9]] false (two_slots 0 1) [0; 1]) =
    Ready (Plan (two_slots 0 1) [0; 1]).
Proof. reflexivity. Qed.

Theorem misordered_slot_mutant_is_rejected :
  snd (prepare [[7]; [9]] false (two_slots 1 0) [0; 1]) = Rejected.
Proof. reflexivity. Qed.

Theorem missing_slot_mutant_is_rejected :
  snd (prepare [[7]; [9]] false (leaf (Slot [10] 0)) [0; 1]) = Rejected.
Proof. reflexivity. Qed.

Theorem duplicated_slot_mutant_is_rejected :
  snd (prepare [[7]; [9]] false (two_slots 0 0) [0; 1]) = Rejected.
Proof. reflexivity. Qed.

Theorem reordered_or_duplicated_children_change_observation :
  planned_emit [[7]; [9]] (two_slots 0 1) [0; 1] <>
    planned_emit [[7]; [9]] (two_slots 0 1) [1; 0] /\
  planned_emit [[7]; [9]] (two_slots 0 1) [0; 1] <>
    planned_emit [[7]; [9]] (two_slots 0 1) [0; 0].
Proof. split; discriminate. Qed.

Theorem bad_framing_mutant_changes_body_extent :
  field [10] [7; 9] = [10; 2; 7; 9] /\
  field [10] [7; 9] <> [10; 1; 7; 9] /\
  EPM1.decode_varint [1; 7; 9] = Some (1, [7; 9]).
Proof. repeat split; discriminate. Qed.

Print Assumptions ordered_hole_substitution_is_exact.
Print Assumptions worklist_emission_refines_resident_substitution.
Print Assumptions emitted_program_has_one_instruction_per_shell_node.
Print Assumptions successful_plan_has_exact_ordered_slots.
Print Assumptions successful_plan_consumes_exactly_all_children.
Print Assumptions missing_or_extra_children_are_rejected.
Print Assumptions permuted_or_repeated_slots_are_rejected.
Print Assumptions accepted_handles_refer_to_existing_children.
Print Assumptions complete_shell_declines_without_arena_change.
Print Assumptions every_prepare_preserves_prior_arena.
Print Assumptions prior_denotations_survive_arena_extension.
Print Assumptions modeled_plan_never_sorts_or_materializes.
Print Assumptions modeled_plan_retains_each_handle_once.
Print Assumptions absent_and_present_empty_are_distinct.
Print Assumptions final_endpoint_equals_resident.
Print Assumptions successful_plan_uses_supplied_nonsemantic_score.
Print Assumptions nested_field_length_prefix_round_trips.
Print Assumptions ordered_distinct_handles_are_accepted.
Print Assumptions misordered_slot_mutant_is_rejected.
Print Assumptions missing_slot_mutant_is_rejected.
Print Assumptions duplicated_slot_mutant_is_rejected.
Print Assumptions reordered_or_duplicated_children_change_observation.
Print Assumptions bad_framing_mutant_changes_body_extent.

End DecodedComponentConstruction.

Module FragmentComparison.

Definition bytes := list nat.
Inductive piece := Literal (body : bytes) | Graph (denotation : bytes).
Definition emit_piece p := match p with Literal b | Graph b => b end.
Definition stream ps := concat (map emit_piece ps).
Definition borrowed ps : option bytes :=
  match ps with [] => Some [] | [Literal b] => Some b | _ => None end.

Lemma borrowed_is_exact ps b : borrowed ps = Some b -> stream ps = b.
Proof.
  destruct ps as [|p ps]; cbn; intros H; try congruence.
  destruct p; destruct ps; cbn in H; try discriminate.
  inversion H; subst. cbn. now rewrite app_nil_r.
Qed.

Fixpoint lex (left right : bytes) : comparison :=
  match left, right with
  | [], [] => Eq | [], _ => Lt | _, [] => Gt
  | x :: xs, y :: ys =>
      match Nat.compare x y with Eq => lex xs ys | order => order end
  end.

Definition optimized left right :=
  match borrowed left, borrowed right with
  | Some l, Some r => lex l r
  | _, _ => lex (stream left) (stream right)
  end.

Theorem comparison_refines left right :
  optimized left right = lex (stream left) (stream right).
Proof.
  unfold optimized. destruct (borrowed left) eqn:L;
    destruct (borrowed right) eqn:R; try reflexivity.
  rewrite (borrowed_is_exact left L), (borrowed_is_exact right R). reflexivity.
Qed.

Definition then_compare first second := match first with Eq => second | _ => first end.
Definition component tag_left tag_right score_left score_right body_left body_right :=
  then_compare (Nat.compare tag_left tag_right)
    (then_compare (optimized score_left score_right) (optimized body_left body_right)).

Theorem component_order_refines tl tr sl sr bl br :
  component tl tr sl sr bl br =
  then_compare (Nat.compare tl tr)
    (then_compare (lex (stream sl) (stream sr)) (lex (stream bl) (stream br))).
Proof. unfold component. now rewrite !comparison_refines. Qed.

Theorem equal_score_keeps_protobuf_tiebreak tl sl sr bl br :
  stream sl = stream sr ->
  component tl tl sl sr bl br = lex (stream bl) (stream br).
Proof.
  intros H. rewrite component_order_refines, Nat.compare_refl, H.
  assert (Same : forall xs, lex xs xs = Eq).
  { induction xs; cbn; auto. now rewrite Nat.compare_refl. }
  rewrite Same. reflexivity.
Qed.

Definition cursor_starts left right :=
  match borrowed left, borrowed right with Some _, Some _ => 0 | _, _ => 2 end.

Theorem literal_comparison_has_no_cursor l r :
  cursor_starts [Literal l] [Literal r] = 0.
Proof. reflexivity. Qed.

Theorem graph_representation_retains_fallback l r :
  cursor_starts [Graph l] [Literal r] = 2 /\
  optimized [Graph l] [Literal r] = lex l r.
Proof. split; cbn; now rewrite ?app_nil_r. Qed.

Theorem ignoring_tiebreak_mutant_is_detected :
  component 4 4 [Literal [1]] [Literal [1]] [Literal [2]] [Literal [3]] <> Eq.
Proof. discriminate. Qed.

Theorem ignoring_fragment_suffix_mutant_is_detected :
  optimized [Literal [1]; Literal [2]] [Literal [1]] <> Eq.
Proof. discriminate. Qed.

Print Assumptions borrowed_is_exact.
Print Assumptions comparison_refines.
Print Assumptions component_order_refines.
Print Assumptions equal_score_keeps_protobuf_tiebreak.
Print Assumptions literal_comparison_has_no_cursor.
Print Assumptions graph_representation_retains_fallback.
Print Assumptions ignoring_tiebreak_mutant_is_detected.
Print Assumptions ignoring_fragment_suffix_mutant_is_detected.

End FragmentComparison.

Module GroundLeafConstruction.

Definition bytes := list nat.
Inductive score_label := Atom (encoded : bytes) | Branch.
Definition atom b : @tree score_label := Node (Atom b) FNil.
Definition branch children : @tree score_label := Node Branch children.
Definition score_algebra label children :=
  match label with Atom b => b | Branch => [2] ++ concat children ++ [0] end.
Definition encode_score := fold_tree score_algebra.

Inductive shape :=
| Absent (constructor : bytes)
| Scalar (constructor payload : bytes)
| Rational (constructor numerator denominator : bytes)
| Fixed (constructor unscaled scale : bytes).

Definition score_tree subject :=
  match subject with
  | Absent c => branch (FCons (atom c) FNil)
  | Scalar c p => branch (FCons (atom c) (FCons (atom p) FNil))
  | Rational c n d => branch (FCons (atom c) (FCons (atom n) (FCons (atom d) FNil)))
  | Fixed c u s => branch (FCons (atom c) (FCons (atom u)
      (FCons (branch (FCons (atom s) FNil)) FNil)))
  end.

Definition direct_score subject :=
  match subject with
  | Absent c => [2] ++ c ++ [0]
  | Scalar c p => [2] ++ c ++ p ++ [0]
  | Rational c n d => [2] ++ c ++ n ++ d ++ [0]
  | Fixed c u s => [2] ++ c ++ u ++ [2] ++ s ++ [0; 0]
  end.

Theorem direct_score_refines_tree subject :
  direct_score subject = encode_score (score_tree subject).
Proof.
  destruct subject; unfold encode_score, score_tree, direct_score;
    cbn [branch atom fold_tree fold_forest score_algebra concat];
    repeat rewrite app_nil_r; repeat rewrite <- app_assoc; reflexivity.
Qed.

Inductive wire := Scalars (body : bytes)
| OneEdge (prefix tag child suffix : bytes).
Definition wire_shell subject : DecodedComponentConstruction.shell :=
  let literal b := Node (DecodedComponentConstruction.Literal b) FNil in
  match subject with
  | Scalars b => literal b
  | OneEdge prefix tag child suffix =>
      Node DecodedComponentConstruction.Sequence (FCons (literal prefix)
        (FCons (Node (DecodedComponentConstruction.Delimited tag)
          (FCons (literal child) FNil)) (FCons (literal suffix) FNil)))
  end.
Definition direct_wire subject :=
  match subject with
  | Scalars b => b
  | OneEdge prefix tag child suffix =>
      prefix ++ DecodedComponentConstruction.field tag child ++ suffix
  end.

Theorem direct_wire_refines_generated_program subject :
  direct_wire subject =
  DecodedComponentConstruction.planned_emit [] (wire_shell subject) [].
Proof.
  destruct subject; unfold direct_wire, wire_shell,
    DecodedComponentConstruction.planned_emit;
    cbn [fold_tree fold_forest DecodedComponentConstruction.emit_algebra concat];
    repeat rewrite app_nil_r; reflexivity.
Qed.

Record leaf := Leaf {
  score_shape : shape;
  wire_program : wire;
  path_segment : option bytes;
  path_stable : bool
}.
Record component := Component {
  score_bytes : bytes;
  protobuf_bytes : bytes;
  segment : option bytes;
  stable : bool
}.
Definition general leaf := Component
  (encode_score (score_tree (score_shape leaf)))
  (DecodedComponentConstruction.planned_emit [] (wire_shell (wire_program leaf)) [])
  (path_segment leaf) (path_stable leaf).
Definition direct leaf := Component (direct_score (score_shape leaf))
  (direct_wire (wire_program leaf)) (path_segment leaf) (path_stable leaf).

Theorem complete_component_refines leaf : direct leaf = general leaf.
Proof.
  unfold direct, general.
  now rewrite direct_score_refines_tree, direct_wire_refines_generated_program.
Qed.

Inductive expression := Ground (value : leaf) | VariableExpr | Recursive.
Inductive outcome := Built (value : component) | ArityError | UseGeneral.
Definition dispatch expression supplied :=
  match expression with
  | Ground value => if Nat.eqb supplied 0 then Built (direct value) else ArityError
  | _ => UseGeneral
  end.
Definition reference expression supplied :=
  match expression with
  | Ground value => if Nat.eqb supplied 0 then Built (general value) else ArityError
  | _ => UseGeneral
  end.

Theorem restricted_dispatch_refines expression supplied :
  dispatch expression supplied = reference expression supplied.
Proof.
  destruct expression; cbn; try reflexivity.
  rewrite complete_component_refines. reflexivity.
Qed.

Theorem recursive_empty_children_still_require_general :
  dispatch Recursive 0 = UseGeneral.
Proof. reflexivity. Qed.
Theorem ground_extra_children_rejected value n :
  dispatch (Ground value) (S n) = ArityError.
Proof. reflexivity. Qed.
Theorem owned_fragment_preserves_prefix (prior : list bytes) body :
  firstn (length prior) (prior ++ [body]) = prior.
Proof. rewrite firstn_app, firstn_all, Nat.sub_diag. now rewrite app_nil_r. Qed.
Theorem owned_fragment_has_exact_payload (prior : list bytes) body :
  nth_error (prior ++ [body]) (length prior) = Some body.
Proof. rewrite nth_error_app2 by lia. now rewrite Nat.sub_diag. Qed.
Theorem flattened_fixed_scale_mutant_is_detected :
  direct_score (Fixed [11] [12] [13]) <>
  direct_score (Rational [11] [12] [13]).
Proof. discriminate. Qed.

Print Assumptions direct_score_refines_tree.
Print Assumptions direct_wire_refines_generated_program.
Print Assumptions complete_component_refines.
Print Assumptions restricted_dispatch_refines.
Print Assumptions recursive_empty_children_still_require_general.
Print Assumptions ground_extra_children_rejected.
Print Assumptions owned_fragment_preserves_prefix.
Print Assumptions owned_fragment_has_exact_payload.
Print Assumptions flattened_fixed_scale_mutant_is_detected.

End GroundLeafConstruction.
