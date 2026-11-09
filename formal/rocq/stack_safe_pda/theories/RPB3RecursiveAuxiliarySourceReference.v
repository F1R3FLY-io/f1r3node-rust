From Stdlib Require Import List Bool Arith NArith Lia Wf_nat.
From Stdlib Require Import Arith.Peano_dec.
From StackSafePDA Require Import StackSafePDA EPM1 EPathMap RPB3WireCursor
  RPB3GeneratedFieldRefinement RPB3RawRegionTree
  RPB3SourceReferenceIntegration RPB3SourceScalarReduction
  RPB3SemanticReducer EPM1ACTree03
  RPB3AuxiliaryTypedReference.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Reference := RPB3SourceReferenceIntegration.
Module Scalar := RPB3SourceScalarReduction.
Module Semantic := RPB3SemanticReducer.
Module Auxiliary := RPB3AuxiliaryTypedReference.
Module Wire := RPB3WireCursor.

(** This file builds source-preserving candidate summaries for nested
    StringMap and PathMap values. Auxiliary summaries below are constructed
    after source, grammar, and physical checks; a separate refinement theorem
    must still relate them to the complete generated semantic reducer and
    production score families. The executable examples establish acceptance
    by this reference, not production admission equivalence. *)

Definition par_raw (body : list nat) : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message
    body (length body) (Some 1) [] [].

Definition full_par_source_reference
    (body : list nat) (semantic_default : bool)
    (summary : Schema.typed_summary) : Prop :=
  exists program nodes converted,
    Schema.lookup_projected_program 1 [] [] = Some program /\
    Source.decode_region (S (length body)) body program
      0 (length body) = Some nodes /\
    Source.source_par_semantic_default body nodes =
      Some semantic_default /\
    Source.sequence_source_results
      (map (Source.raw_tree_of_source (S (length body)) body) nodes) =
        Some converted /\
    Forall2 (fun node child =>
      Source.raw_tree_of_source (S (length body)) body node = Some child)
      nodes converted /\
    summary = Schema.reduce_field (par_raw body)
      (map Schema.typed_reference converted) /\
    Schema.summary_valid summary = true /\
    Schema.summary_body summary = body.

Lemma source_sequence_has_positional_raw_trees :
  forall (A B : Type) (convert : A -> option B) source converted,
    Source.sequence_source_results (map convert source) =
      Some converted ->
    Forall2 (fun original result => convert original = Some result)
      source converted.
Proof.
  intros A B convert source.
  induction source as [|first rest IH]; intros converted accepted.
  - simpl in accepted. inversion accepted. constructor.
  - simpl in accepted.
    destruct (convert first) as [first_result|] eqn:first_converted;
      [|discriminate].
    destruct (Source.sequence_source_results (map convert rest))
      as [rest_results|] eqn:rest_converted; [|discriminate].
    inversion accepted; subst converted.
    constructor; [exact first_converted|].
    exact (IH rest_results eq_refl).
Qed.

Lemma refused_source_sequence_has_refused_member :
  forall (A B : Type) (convert : A -> option B) source,
    Source.sequence_source_results (map convert source) = None ->
    exists original,
      In original source /\ convert original = None.
Proof.
  intros A B convert source.
  induction source as [|first rest IH]; intro refused.
  - discriminate.
  - simpl in refused.
    destruct (convert first) as [converted|] eqn:first_result.
    + destruct (Source.sequence_source_results (map convert rest))
        as [converted_rest|] eqn:rest_result; [discriminate|].
      destruct (IH eq_refl) as [original [member failed]].
      exists original. split; [right; exact member|exact failed].
    + exists first. split; [left; reflexivity|exact first_result].
Qed.

Lemma decoded_epm1_varint_has_strictly_shorter_suffix :
  forall input value suffix,
    EPM1.decode_varint input = Some (value, suffix) ->
    length suffix < length input.
Proof.
  induction input as [|first rest IH]; intros value suffix decoded.
  - rewrite EPM1.decode_varint_nil in decoded. discriminate.
  - rewrite EPM1.decode_varint_cons in decoded.
    destruct (first <? 128).
    + inversion decoded; subst. simpl. lia.
    + destruct (first <? 256); [|discriminate].
      destruct (EPM1.decode_varint rest)
        as [[quotient after]|] eqn:recursive; [|discriminate].
      destruct (quotient =? 0); [discriminate|].
      inversion decoded; subst suffix.
      specialize (IH quotient after eq_refl).
      simpl. lia.
Qed.

Lemma decoded_epm1_frame_has_strictly_smaller_payload_and_suffix :
  forall input payload suffix,
    EPM1.decode_frame input = Some (payload, suffix) ->
    length payload < length input /\
    length suffix < length input.
Proof.
  intros input payload suffix decoded.
  unfold EPM1.decode_frame in decoded.
  destruct (EPM1.decode_varint input)
    as [[declared rest]|] eqn:prefix; [|discriminate].
  destruct (declared <=? length rest) eqn:fits; [|discriminate].
  inversion decoded; subst payload suffix.
  apply Nat.leb_le in fits.
  pose proof (decoded_epm1_varint_has_strictly_shorter_suffix
    input declared rest prefix) as shorter.
  rewrite firstn_length, skipn_length, Nat.min_l by lia.
  split; lia.
Qed.

Lemma decoded_epm1_frames_have_strictly_smaller_values :
  forall count input values suffix,
    EPM1.decode_frames count input = Some (values, suffix) ->
    Forall (fun value => length value < length input) values.
Proof.
  induction count as [|count IH]; intros input values suffix decoded.
  - simpl in decoded. inversion decoded; constructor.
  - simpl in decoded.
    destruct (EPM1.decode_frame input) as [[head rest]|]
      eqn:frame; [|discriminate].
    destruct (EPM1.decode_frames count rest) as [[tail after]|]
      eqn:later; [|discriminate].
    inversion decoded; subst values suffix.
    destruct (decoded_epm1_frame_has_strictly_smaller_payload_and_suffix
      input head rest frame) as [head_short rest_short].
    constructor; [exact head_short|].
    rewrite Forall_forall.
    intros value member.
    pose proof (IH rest tail after later) as descendants.
    rewrite Forall_forall in descendants.
    specialize (descendants value member).
    lia.
Qed.

Lemma decoded_epm1_payload_has_strictly_smaller_value_bodies :
  forall mode_byte payload snapshot suffix,
    EPM1.decode_epm1_prefix
      (EPM1.epm_magic ++ [EPM1.epm_version; mode_byte] ++ payload) =
      Some (snapshot, suffix) ->
    Forall (fun value => length value < length payload)
      (EPM1.value_bodies snapshot).
Proof.
  intros mode_byte payload snapshot suffix decoded.
  unfold EPM1.decode_epm1_prefix, EPM1.epm_magic,
    EPM1.epm_version in decoded.
  simpl in decoded.
  destruct mode_byte as [|[|[|later]]]; try discriminate;
    destruct (EPM1.decode_frame payload)
      as [[arena after_arena]|] eqn:arena_frame;
      try discriminate;
    destruct (EPM1.decode_varint after_arena)
      as [[count framed_values]|] eqn:value_count;
      try discriminate;
    destruct (EPM1.decode_frames count framed_values)
      as [[values after_values]|] eqn:decoded_values;
      try discriminate;
    inversion decoded; subst snapshot suffix;
    destruct (decoded_epm1_frame_has_strictly_smaller_payload_and_suffix
      payload arena after_arena arena_frame) as [_ arena_short];
    pose proof (decoded_epm1_varint_has_strictly_shorter_suffix
      after_arena count framed_values value_count) as frames_short;
    pose proof (decoded_epm1_frames_have_strictly_smaller_values
      count framed_values values after_values decoded_values) as bounded;
    rewrite Forall_forall in *;
    intros value member;
    specialize (bounded value member);
    lia.
Qed.

Theorem decoded_epm1_value_bodies_are_strictly_shorter_than_snapshot :
  forall encoded snapshot,
    EPM1.decode_epm1 encoded = Some snapshot ->
    Forall (fun value => length value < length encoded)
      (EPM1.value_bodies snapshot).
Proof.
  intros encoded snapshot accepted.
  unfold EPM1.decode_epm1 in accepted.
  destruct (EPM1.decode_epm1_prefix encoded)
    as [[parsed suffix]|] eqn:prefix; [|discriminate].
  destruct suffix as [|trailing rest]; [|discriminate].
  destruct (EPM1.mode_well_formedb parsed); [|discriminate].
  inversion accepted; subst snapshot.
  unfold EPM1.decode_epm1_prefix in prefix.
  repeat match goal with
  | prefix : context [match ?subject with _ => _ end] |- _ =>
      destruct subject eqn:?;
      cbn -[EPM1.decode_frame EPM1.decode_varint EPM1.decode_frames]
        in prefix |- *;
      try discriminate
  end.
  all: inversion prefix; subst parsed;
    match goal with
    | frame : EPM1.decode_frame ?payload = Some (?arena, ?after_arena),
      count : EPM1.decode_varint ?after_arena = Some (?n, ?framed),
      values : EPM1.decode_frames ?n ?framed = Some (?bodies, ?suffix)
      |- _ =>
        pose proof
          (decoded_epm1_frame_has_strictly_smaller_payload_and_suffix
            payload arena after_arena frame) as [_ arena_short];
        pose proof
          (decoded_epm1_varint_has_strictly_shorter_suffix
            after_arena n framed count) as framed_short;
        pose proof
          (decoded_epm1_frames_have_strictly_smaller_values
            n framed bodies suffix values) as bounded
    end;
    rewrite Forall_forall in *;
    intros value member;
    specialize (bounded value member);
    lia.
Qed.

Theorem refined_string_map_value_is_strictly_inside_parent_bytes :
  forall bytes program outer_span outer_field typed value_span body,
    Source.refine_string_map_entry bytes program
      (Source.SourceAuxiliary outer_span outer_field) = Some typed ->
    Source.typed_map_value_source typed = Some (value_span, body) ->
    Wire.span_payload_start outer_span <
      Wire.span_payload_start value_span /\
    Wire.span_end value_span <= Wire.span_end outer_span /\
    Wire.span_end outer_span <= length bytes /\
    length body < length bytes.
Proof.
  intros bytes program outer_span outer_field typed value_span body
    refined value_source.
  unfold Source.refine_string_map_entry in refined.
  destruct (Source.decode_string_map_entry_source bytes program
    (Source.SourceAuxiliary outer_span outer_field))
    as [entry|] eqn:decoded; [|discriminate].
  destruct (Schema.lookup_projected_program
    (Source.map_value_type_id entry) [] [])
    as [value_program|]; [|discriminate].
  inversion refined; subst typed.
  simpl in value_source.
  destruct (Source.map_value_span entry) as [actual_span|]
    eqn:present; [|discriminate].
  inversion value_source; subst value_span body.
  destruct (Source.decoded_string_map_entry_has_actual_constructor_shape
    bytes program (Source.SourceAuxiliary outer_span outer_field)
    entry decoded) as
    [outer [carried [spans [node_shape
      [_ [_ [_ [_ [framed shaped]]]]]]]]].
  inversion node_shape; subst outer carried.
  rewrite present in shaped.
  assert (member : In actual_span spans).
  { inversion shaped; subst; simpl in *; try discriminate; auto. }
  pose proof (Source.parsed_region_spans_stay_inside_their_region
    bytes (Wire.span_payload_start outer_span)
    (Wire.span_end outer_span) true spans framed) as bounded.
  pose proof (Source.source_forall_member _ _ _ _ bounded member)
    as [_ [strict_inside [value_order end_inside]]].
  assert (parent_inside : Wire.span_end outer_span <= length bytes).
  { unfold Wire.worklist_region in framed.
    destruct (Wire.region_ok bytes
      (Wire.span_payload_start outer_span)
      (Wire.span_end outer_span)) eqn:guard; [|discriminate].
    unfold Wire.region_ok in guard.
    apply andb_true_iff in guard as [outer_bounds _].
    apply andb_true_iff in outer_bounds as [_ end_guard].
    now apply Nat.leb_le in end_guard. }
  repeat split; try lia.
  unfold Source.map_span_body.
  rewrite firstn_length, skipn_length, Nat.min_l by lia.
  lia.
Qed.

Lemma typed_reference_of_ordered_forest :
  forall (children : list (@tree Schema.raw_field)),
    Schema.typed_reference_forest (forest_of_list children) =
      map Schema.typed_reference children.
Proof.
  induction children as [|first rest IH]; simpl; [reflexivity|].
  now rewrite IH.
Qed.

Lemma semantic_reference_of_ordered_forest :
  forall (children : list (@tree Schema.raw_field)),
    Semantic.semantic_reduce_forest (forest_of_list children) =
      map Semantic.semantic_reduce children.
Proof.
  induction children as [|first rest IH]; simpl; [reflexivity|].
  now rewrite IH.
Qed.

Theorem accepted_par_has_full_ordered_recursive_reference :
  forall body semantic_default summary,
    Reference.source_reference_par_of_body body =
      Some (semantic_default, summary) ->
    full_par_source_reference body semantic_default summary.
Proof.
  intros body semantic_default summary accepted.
  destruct (Reference.source_reference_par_body_has_decoded_reference
    body semantic_default summary accepted)
    as [program [nodes [root [looked_up [decoded
      [_ [observed [converted [recursive [original [valid _]]]]]]]]]]].
  unfold Reference.checked_source_raw_par_root in converted.
  destruct (Schema.program_family program =? 1); [|discriminate].
  destruct (forallb
    (Reference.checked_source_scalar_tree (S (length body))
      body program) nodes); [|discriminate].
  destruct (Source.sequence_source_results
    (map (Source.raw_tree_of_source (S (length body)) body) nodes))
    as [children|] eqn:ordered; [|discriminate].
  inversion converted; subst root.
  exists program, nodes, children.
  repeat split; try assumption.
  - exact (source_sequence_has_positional_raw_trees
      Source.source_tree (@tree Schema.raw_field)
      (Source.raw_tree_of_source (S (length body)) body)
      nodes children ordered).
  - unfold par_raw.
    simpl in recursive.
    rewrite typed_reference_of_ordered_forest in recursive.
    now symmetry.
Qed.

Theorem full_par_reference_fixes_complete_score_body_and_child_order :
  forall body semantic_default summary,
    full_par_source_reference body semantic_default summary ->
    exists converted,
      Schema.summary_complete summary =
        Schema.summary_complete (Schema.reduce_field (par_raw body)
          (map Schema.typed_reference converted)) /\
      Schema.summary_score summary =
        Schema.summary_score (Schema.reduce_field (par_raw body)
          (map Schema.typed_reference converted)) /\
      Schema.summary_body summary = body /\
      Schema.summary_children summary =
        map (fun child => Schema.observe_child
          (Schema.typed_reference child)) converted /\
      Schema.summary_ordered_slots summary =
        Schema.summary_ordered_slots (Schema.reduce_field (par_raw body)
          (map Schema.typed_reference converted)).
Proof.
  intros body semantic_default summary
    [program [nodes [converted [_ [_ [_ [_ [_ [same [_ original]]]]]]]]]].
  exists converted.
  subst summary.
  repeat split; try reflexivity.
  unfold Schema.reduce_field.
  cbn [Schema.summary_children].
  now rewrite map_map.
Qed.

Theorem full_par_reference_has_independent_semantic_lane_and_score :
  forall body semantic_default summary,
    full_par_source_reference body semantic_default summary ->
    exists converted semantic,
      semantic = Semantic.reduce_semantic_field (par_raw body)
        (map Semantic.semantic_reduce converted) /\
      Semantic.sem_body semantic = body /\
      Semantic.sem_complete semantic =
        Semantic.sem_complete (Semantic.reduce_semantic_field
          (par_raw body) (map Semantic.semantic_reduce converted)) /\
      Semantic.sem_score_evidence semantic =
        Semantic.sem_score_evidence (Semantic.reduce_semantic_field
          (par_raw body) (map Semantic.semantic_reduce converted)) /\
      Schema.summary_children summary =
        map (fun child => Schema.observe_child
          (Schema.typed_reference child)) converted.
Proof.
  intros body semantic_default summary
    [program [nodes [converted [_ [_ [_ [_ [_ [same [_ _]]]]]]]]]].
  exists converted,
    (Semantic.semantic_reduce
      (Node (par_raw body) (forest_of_list converted))).
  simpl [Semantic.semantic_reduce].
  rewrite semantic_reference_of_ordered_forest.
  subst summary.
  repeat split; try reflexivity.
  unfold Schema.reduce_field.
  cbn [Schema.summary_children].
  now rewrite map_map.
Qed.

Theorem accepted_pathmap_values_have_full_ordered_recursive_references :
  forall hash_fn bytes program slot node raw snapshot trie values,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot node =
      Some (Auxiliary.PathMapResult raw snapshot trie values) ->
    Forall2 (fun body result =>
      full_par_source_reference body (fst result) (snd result))
      (EPM1.value_bodies snapshot) values.
Proof.
  intros hash_fn bytes program slot node raw snapshot trie values accepted.
  destruct (Auxiliary.accepted_pathmap_has_physical_and_positional_reference
    hash_fn bytes program slot node raw snapshot trie values accepted)
    as [_ [_ positional]].
  clear accepted.
  induction positional as [|body result bodies results head tail IH].
  - constructor.
  - destruct result as [semantic_default summary].
    constructor; [|exact IH].
    apply accepted_par_has_full_ordered_recursive_reference.
    exact (proj1 head).
Qed.

Theorem accepted_string_map_value_has_full_ordered_recursive_reference :
  forall hash_fn bytes program slot node raw entry,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot node =
      Some (Auxiliary.StringMapResult raw entry) ->
    exists typed,
      Source.refine_string_map_entry bytes program node = Some typed /\
      Schema.descriptor_slot (Source.typed_map_outer_field typed) = slot /\
      match Source.typed_map_value_source typed with
      | None =>
          Source.returned_map_par Schema.typed_summary entry = None /\
          Source.returned_map_par_span Schema.typed_summary entry = None
      | Some (span, body) =>
          exists summary,
            Source.returned_map_par Schema.typed_summary entry =
              Some summary /\
            Source.returned_map_par_span Schema.typed_summary entry =
              Some span /\
            full_par_source_reference body false summary
      end.
Proof.
  intros hash_fn bytes program slot node raw entry accepted.
  destruct (Auxiliary.accepted_string_map_has_typed_entry_source
    hash_fn bytes program slot node raw entry accepted)
    as [typed [refined [selected reduced]]].
  exists typed.
  split; [exact refined|].
  split; [exact selected|].
  destruct (Source.reduced_typed_map_return_matches_source
    Schema.typed_summary Reference.source_reference_par_of_body
    typed entry reduced) as [_ value].
  destruct (Source.typed_map_value_source typed)
    as [[span body]|] eqn:value_source.
  - destruct value as [summary [decoded [returned present]]].
    exists summary.
    repeat split; try assumption.
    now apply accepted_par_has_full_ordered_recursive_reference.
  - exact value.
Qed.

Theorem accepted_default_pathmap_value_has_full_recursive_reference :
  forall hash_fn,
    exists raw trie summary,
      Auxiliary.typed_auxiliary_root hash_fn 31 3
        Auxiliary.default_value_pathmap_wire =
          Some (Auxiliary.PathMapResult raw
            Auxiliary.default_value_pathmap_snapshot
            (Some trie) [(true, summary)]) /\
      full_par_source_reference [] true summary.
Proof.
  intro hash_fn.
  destruct (Auxiliary.default_map_value_has_typed_auxiliary_reference
    hash_fn) as [raw [trie [summary [accepted _]]]].
  exists raw, trie, summary.
  split; [exact accepted|].
  pose proof (Auxiliary.accepted_auxiliary_root_has_decoded_generated_source
    hash_fn 31 3 Auxiliary.default_value_pathmap_wire
    (Auxiliary.PathMapResult raw
      Auxiliary.default_value_pathmap_snapshot (Some trie)
      [(true, summary)]) accepted)
    as [program [nodes [node [_ [_ [_ [_ referred]]]]]]].
  pose proof (accepted_pathmap_values_have_full_ordered_recursive_references
    hash_fn Auxiliary.default_value_pathmap_wire program 3 node
    raw Auxiliary.default_value_pathmap_snapshot (Some trie)
    [(true, summary)] referred) as positional.
  cbn [Auxiliary.default_value_pathmap_snapshot
    EPM1.value_bodies fst snd] in positional.
  inversion positional; subst.
  assumption.
Qed.

Inductive mixed_typed_result : Type :=
| MixedScalarResult
    (raw : Schema.raw_field) (semantic : Semantic.semantic_summary)
| MixedMessageResult
    (raw : Schema.raw_field) (semantic : Semantic.semantic_summary)
    (children : list mixed_typed_result)
| MixedAuxiliaryResult
    (auxiliary : Auxiliary.auxiliary_result)
    (semantic : Semantic.semantic_summary).

Definition mixed_result_raw (result : mixed_typed_result)
    : Schema.raw_field :=
  match result with
  | MixedScalarResult raw _ | MixedMessageResult raw _ _ => raw
  | MixedAuxiliaryResult auxiliary _ =>
      Auxiliary.auxiliary_result_raw auxiliary
  end.

Definition semantic_par_score_of_body (body : list nat)
    : option Semantic.canonical_score :=
  match Reference.source_reference_par_of_body body,
        Schema.lookup_projected_program 1 [] [],
        Source.decode_root 1 body with
  | Some _, Some program, Some nodes =>
      match Reference.checked_source_raw_par_root body program nodes with
      | Some root =>
          let semantic := Semantic.semantic_reduce root in
          if Semantic.sem_definitely_refused semantic then None
          else match Semantic.sem_score_evidence semantic with
               | Semantic.ExactScore score => Some score
               | Semantic.ScoreNotDerived => None
               end
      | None => None
      end
  | _, _, _ => None
  end.

Definition empty_par_canonical_score : Semantic.canonical_score :=
  Semantic.score_constructor 999 [Semantic.ScoreInteger 0%N].

Definition auxiliary_semantic_cut
    (result : Auxiliary.auxiliary_result)
    : option Semantic.semantic_summary :=
  let raw := Auxiliary.auxiliary_result_raw result in
  let score :=
    match result with
    | Auxiliary.StringMapResult _ entry =>
        let value_score :=
          match Source.returned_map_par Schema.typed_summary entry with
          | None => Some empty_par_canonical_score
          | Some summary =>
              semantic_par_score_of_body (Schema.summary_body summary)
          end in
        option_map (fun value =>
          Semantic.ScoreNode (Semantic.score_forest_of_list
            [Semantic.ScoreString
              (Source.returned_map_key Schema.typed_summary entry); value]))
          value_score
    | Auxiliary.PathMapResult _ _ _ _ =>
        Some (Semantic.ScoreBytes (Schema.raw_body raw))
    end in
  option_map (fun exact =>
    Semantic.SemanticSummary
      (Schema.raw_tag raw) (Schema.raw_wire raw) (Schema.raw_payload raw)
      (match Schema.raw_program_type_id raw with
       | Some type_id => type_id
       | None => 0
       end)
      (Schema.raw_body raw) false (Some true)
      (Semantic.ExactScore exact) false false) score.

Definition mixed_result_semantic (result : mixed_typed_result)
    : Semantic.semantic_summary :=
  match result with
  | MixedScalarResult _ semantic | MixedMessageResult _ semantic _ =>
      semantic
  | MixedAuxiliaryResult _ semantic => semantic
  end.

Fixpoint mixed_new_map_returns (children : list mixed_typed_result)
    : list (Source.typed_map_return Schema.typed_summary) :=
  match children with
  | [] => []
  | MixedAuxiliaryResult (Auxiliary.StringMapResult _ entry) _ :: rest =>
      entry :: mixed_new_map_returns rest
  | _ :: rest => mixed_new_map_returns rest
  end.

Fixpoint ordered_mixed_new_map_returnsb
    (prior : option (list nat))
    (entries : list (Source.typed_map_return Schema.typed_summary))
    : bool :=
  match entries with
  | [] => true
  | entry :: rest =>
      Source.map_key_follows prior
        (Source.returned_map_key Schema.typed_summary entry) &&
      ordered_mixed_new_map_returnsb
        (Some (Source.returned_map_key Schema.typed_summary entry)) rest
  end.

Lemma ordered_mixed_new_map_returnsb_iff_source_order :
  forall prior entries,
    ordered_mixed_new_map_returnsb prior entries = true <->
    Source.ordered_map_returns_after prior entries.
Proof.
  intros prior entries.
  revert prior.
  induction entries as [|entry rest IH]; intros prior; simpl.
  - split; intro; [exact I|reflexivity].
  - rewrite andb_true_iff, IH. tauto.
Qed.

Theorem exact_auxiliary_cut_has_score_completion_and_original_body :
  forall auxiliary semantic,
    auxiliary_semantic_cut auxiliary = Some semantic ->
    exists score,
      Semantic.sem_score_evidence semantic = Semantic.ExactScore score /\
      Semantic.sem_complete semantic = Some true /\
      Semantic.sem_body semantic =
        Schema.raw_body (Auxiliary.auxiliary_result_raw auxiliary) /\
      Semantic.sem_definitely_refused semantic = false.
Proof.
  intros [raw entry|raw snapshot trie values] semantic accepted;
    unfold auxiliary_semantic_cut in accepted; simpl in accepted.
  - destruct (match Source.returned_map_par Schema.typed_summary entry with
      | None => Some empty_par_canonical_score
      | Some summary =>
          semantic_par_score_of_body (Schema.summary_body summary)
      end) as [value_score|] eqn:derived; [|discriminate].
    inversion accepted; subst semantic.
    eexists. repeat split; reflexivity.
  - inversion accepted; subst semantic.
    eexists. repeat split; reflexivity.
Qed.

Theorem exact_pathmap_cut_uses_original_snapshot_as_score_atom :
  forall raw snapshot trie values semantic,
    auxiliary_semantic_cut
      (Auxiliary.PathMapResult raw snapshot trie values) =
        Some semantic ->
    Semantic.sem_score_evidence semantic =
      Semantic.ExactScore
        (Semantic.ScoreBytes (Schema.raw_body raw)) /\
    Semantic.sem_complete semantic = Some true.
Proof.
  intros raw snapshot trie values semantic accepted.
  unfold auxiliary_semantic_cut in accepted.
  inversion accepted; subst semantic.
  now split.
Qed.

Theorem exact_string_map_cut_uses_ordered_key_and_value_score :
  forall raw entry semantic,
    auxiliary_semantic_cut (Auxiliary.StringMapResult raw entry) =
      Some semantic ->
    exists value_score,
      (match Source.returned_map_par Schema.typed_summary entry with
       | None => value_score = empty_par_canonical_score
       | Some summary =>
           semantic_par_score_of_body (Schema.summary_body summary) =
             Some value_score
       end) /\
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore
          (Semantic.ScoreNode (Semantic.score_forest_of_list
            [Semantic.ScoreString
              (Source.returned_map_key Schema.typed_summary entry);
             value_score])).
Proof.
  intros raw entry semantic accepted.
  unfold auxiliary_semantic_cut in accepted; simpl in accepted.
  destruct (Source.returned_map_par Schema.typed_summary entry)
    as [summary|] eqn:present.
  - destruct (semantic_par_score_of_body (Schema.summary_body summary))
      as [value_score|] eqn:derived; [|discriminate].
    inversion accepted; subst semantic.
    exists value_score. now split.
  - inversion accepted; subst semantic.
    exists empty_par_canonical_score. now split.
Qed.

Definition checked_generated_raw (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Schema.raw_field :=
  match Source.source_descriptor program (Source.source_span node),
        Source.raw_field_of_source bytes node with
  | Some selected, Some raw =>
      if Auxiliary.descriptor_eq_dec (Source.source_field node) selected then
        if Schema.valid_raw_body raw then Some raw else None
      else None
  | _, _ => None
  end.

Definition checked_generated_scalar_raw (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    : option Schema.raw_field :=
  match checked_generated_raw bytes program node,
        Scalar.source_scalar_raw bytes program node with
  | Some _, Some raw => Some raw
  | _, _ => None
  end.

Lemma checked_generated_raw_preserves_descriptor_body_and_validity :
  forall bytes program node raw,
    checked_generated_raw bytes program node = Some raw ->
    Source.source_descriptor program (Source.source_span node) =
      Some (Source.source_field node) /\
    Source.raw_field_of_source bytes node = Some raw /\
    Schema.raw_body raw = Source.source_body bytes node /\
    Schema.valid_raw_body raw = true.
Proof.
  intros bytes program node raw accepted.
  unfold checked_generated_raw in accepted.
  destruct (Source.source_descriptor program (Source.source_span node))
    as [selected|] eqn:descriptor; [|discriminate].
  destruct (Source.raw_field_of_source bytes node)
    as [candidate|] eqn:converted; [|discriminate].
  destruct (Auxiliary.descriptor_eq_dec
    (Source.source_field node) selected) as [same|different];
    [subst selected|discriminate].
  destruct (Schema.valid_raw_body candidate) eqn:valid;
    [|discriminate].
  inversion accepted; subst raw.
  repeat split; try assumption.
  exact (proj1 (proj2 (proj2 (proj2
    (Source.raw_field_of_source_preserves_wire_and_body
      bytes node candidate converted))))).
Qed.

Theorem checked_generated_scalar_uses_generated_and_grammar_validated_raw :
  forall bytes program node raw,
    checked_generated_scalar_raw bytes program node = Some raw ->
    checked_generated_raw bytes program node = Some raw /\
    Scalar.source_scalar_raw bytes program node = Some raw.
Proof.
  intros bytes program node raw accepted.
  unfold checked_generated_scalar_raw in accepted.
  destruct (checked_generated_raw bytes program node)
    as [generated|] eqn:checked; [|discriminate].
  destruct (Scalar.source_scalar_raw bytes program node)
    as [validated|] eqn:scalar; [|discriminate].
  inversion accepted; subst validated.
  destruct (checked_generated_raw_preserves_descriptor_body_and_validity
    bytes program node generated checked)
    as [_ [source _]].
  pose proof (Scalar.accepted_source_scalar_reuses_raw_tree_leaf
    0 bytes program node raw scalar) as leaf.
  unfold Scalar.source_scalar_raw in scalar.
  destruct node as [span field|span field children|span field];
    simpl in scalar; try discriminate.
  cbn [Source.raw_tree_of_source] in leaf.
  rewrite source in leaf.
  inversion leaf; subst generated.
  now split.
Qed.

Fixpoint mixed_source_reference (hash_fn : list nat -> nat)
    (bytes : list nat) (program : Schema.generated_program)
    (node : Source.source_tree) {struct node}
    : option mixed_typed_result :=
  match node with
  | Source.SourceScalar _ _ =>
      match checked_generated_scalar_raw bytes program node with
      | None => None
      | Some raw =>
          let semantic := Semantic.reduce_semantic_field raw [] in
          if Semantic.sem_definitely_refused semantic then None
          else Some (MixedScalarResult raw semantic)
      end
  | Source.SourceMessage _ field children =>
      match checked_generated_raw bytes program node,
            Schema.descriptor_child_type_id field with
      | Some raw, Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | None => None
          | Some child_program =>
              let fix visit_children (remaining : list Source.source_tree)
                  : option (list mixed_typed_result) :=
                match remaining with
                | [] => Some []
                | first :: rest =>
                    match mixed_source_reference hash_fn bytes
                            child_program first,
                          visit_children rest with
                    | Some head, Some tail => Some (head :: tail)
                    | _, _ => None
                    end
                end in
              match visit_children children with
              | None => None
              | Some converted =>
                  let map_ordered :=
                    if type_id =? 20 then
                      ordered_mixed_new_map_returnsb None
                        (mixed_new_map_returns converted)
                    else true in
                  if map_ordered then
                    let semantic := Semantic.reduce_semantic_field raw
                      (map mixed_result_semantic converted) in
                    if Semantic.sem_definitely_refused semantic then None
                    else Some (MixedMessageResult raw semantic converted)
                  else None
              end
          end
      | _, _ => None
      end
  | Source.SourceAuxiliary _ _ =>
      match Auxiliary.typed_auxiliary_reference hash_fn bytes program
          (Schema.descriptor_slot (Source.source_field node)) node with
      | None => None
      | Some auxiliary =>
          option_map (MixedAuxiliaryResult auxiliary)
            (auxiliary_semantic_cut auxiliary)
      end
  end.

Fixpoint mixed_children_reference (hash_fn : list nat -> nat)
    (bytes : list nat) (program : Schema.generated_program)
    (nodes : list Source.source_tree) {struct nodes}
    : option (list mixed_typed_result) :=
  match nodes with
  | [] => Some []
  | first :: rest =>
      match mixed_source_reference hash_fn bytes program first,
            mixed_children_reference hash_fn bytes program rest with
      | Some head, Some tail => Some (head :: tail)
      | _, _ => None
      end
  end.

Lemma local_mixed_children_are_global_children :
  forall hash_fn bytes program nodes,
    (let fix visit_children (remaining : list Source.source_tree)
        : option (list mixed_typed_result) :=
      match remaining with
      | [] => Some []
      | first :: rest =>
          match mixed_source_reference hash_fn bytes program first,
                visit_children rest with
          | Some head, Some tail => Some (head :: tail)
          | _, _ => None
          end
      end in visit_children nodes) =
    mixed_children_reference hash_fn bytes program nodes.
Proof.
  intros hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; simpl; [reflexivity|].
  now rewrite <- IH.
Qed.

Definition mixed_root_reference (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat)
    : option (list mixed_typed_result) :=
  match Schema.lookup_projected_program type_id [] [],
        Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      mixed_children_reference hash_fn bytes program nodes
  | _, _ => None
  end.

Lemma accepted_mixed_children_preserve_source_order :
  forall hash_fn bytes program nodes results,
    mixed_children_reference hash_fn bytes program nodes = Some results ->
    Forall2 (fun node result =>
      mixed_source_reference hash_fn bytes program node = Some result)
      nodes results.
Proof.
  intros hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; intros results accepted.
  - simpl in accepted. inversion accepted. constructor.
  - simpl in accepted.
    destruct (mixed_source_reference hash_fn bytes program first)
      as [head|] eqn:converted_first; [|discriminate].
    destruct (mixed_children_reference hash_fn bytes program rest)
      as [tail|] eqn:converted_rest; [|discriminate].
    inversion accepted; subst results.
    constructor; [exact converted_first|].
    exact (IH tail eq_refl).
Qed.

Theorem accepted_mixed_message_has_ordered_typed_children_and_original_body :
  forall hash_fn bytes program span field children raw semantic results,
    mixed_source_reference hash_fn bytes program
      (Source.SourceMessage span field children) =
        Some (MixedMessageResult raw semantic results) ->
    exists type_id child_program,
      Schema.descriptor_child_type_id field = Some type_id /\
      Schema.lookup_projected_program type_id [] [] =
        Some child_program /\
      checked_generated_raw bytes program
        (Source.SourceMessage span field children) = Some raw /\
      Forall2 (fun child result =>
        mixed_source_reference hash_fn bytes child_program child =
          Some result) children results /\
      semantic = Semantic.reduce_semantic_field raw
        (map mixed_result_semantic results) /\
      Semantic.sem_body semantic =
        Source.source_body bytes (Source.SourceMessage span field children) /\
      Semantic.sem_definitely_refused semantic = false.
Proof.
  intros hash_fn bytes program span field children raw semantic results
    accepted.
  cbn [mixed_source_reference] in accepted.
  destruct (checked_generated_raw bytes program
    (Source.SourceMessage span field children)) as [candidate|]
    eqn:checked; [|discriminate].
  destruct (Schema.descriptor_child_type_id field)
    as [type_id|] eqn:child_type; [|discriminate].
  destruct (Schema.lookup_projected_program type_id [] [])
    as [child_program|] eqn:looked_up; [|discriminate].
  rewrite local_mixed_children_are_global_children in accepted.
  destruct (mixed_children_reference hash_fn bytes child_program children)
    as [converted|] eqn:descendants; [|discriminate].
  destruct (if type_id =? 20 then
      ordered_mixed_new_map_returnsb None
        (mixed_new_map_returns converted) else true) eqn:ordered;
    [|discriminate].
  destruct (Semantic.sem_definitely_refused
    (Semantic.reduce_semantic_field candidate
      (map mixed_result_semantic converted))) eqn:refused;
    [discriminate|].
  inversion accepted; subst raw semantic results.
  exists type_id, child_program.
  repeat split; try assumption; try reflexivity.
  - now apply accepted_mixed_children_preserve_source_order.
  - destruct (checked_generated_raw_preserves_descriptor_body_and_validity
      bytes program (Source.SourceMessage span field children)
      candidate checked) as [_ [_ [body _]]].
    simpl. exact body.
Qed.

Theorem accepted_mixed_new_has_strict_typed_map_key_order :
  forall hash_fn bytes program span field children raw semantic results,
    Schema.descriptor_child_type_id field = Some 20 ->
    mixed_source_reference hash_fn bytes program
      (Source.SourceMessage span field children) =
        Some (MixedMessageResult raw semantic results) ->
    Source.ordered_map_returns_after None
      (mixed_new_map_returns results).
Proof.
  intros hash_fn bytes program span field children raw semantic results
    child_type accepted.
  cbn [mixed_source_reference] in accepted.
  destruct (checked_generated_raw bytes program
    (Source.SourceMessage span field children)) as [candidate|];
    [|discriminate].
  rewrite child_type in accepted.
  destruct (Schema.lookup_projected_program 20 [] [])
    as [child_program|]; [|discriminate].
  cbn [Nat.eqb] in accepted.
  rewrite local_mixed_children_are_global_children in accepted.
  destruct (mixed_children_reference hash_fn bytes child_program children)
    as [converted|]; [|discriminate].
  destruct (ordered_mixed_new_map_returnsb None
    (mixed_new_map_returns converted)) eqn:ordered; [|discriminate].
  destruct (Semantic.sem_definitely_refused
    (Semantic.reduce_semantic_field candidate
      (map mixed_result_semantic converted))); [discriminate|].
  inversion accepted; subst results.
  now apply (proj1 (ordered_mixed_new_map_returnsb_iff_source_order
    None (mixed_new_map_returns converted))).
Qed.

Theorem accepted_mixed_root_preserves_generated_source_order :
  forall hash_fn type_id bytes results,
    mixed_root_reference hash_fn type_id bytes = Some results ->
    exists program nodes,
      Schema.lookup_projected_program type_id [] [] = Some program /\
      Source.decode_region (S (length bytes)) bytes program
        0 (length bytes) = Some nodes /\
      Forall (Source.source_node_matches_generated_descriptor program)
        nodes /\
      Forall2 (fun node result =>
        mixed_source_reference hash_fn bytes program node = Some result)
        nodes results.
Proof.
  intros hash_fn type_id bytes results accepted.
  unfold mixed_root_reference in accepted.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_root type_id bytes)
    as [nodes|] eqn:decoded; [|discriminate].
  exists program, nodes.
  assert (source : Source.decode_region (S (length bytes)) bytes program
    0 (length bytes) = Some nodes).
  { unfold Source.decode_root in decoded.
    now rewrite looked_up in decoded. }
  repeat split; try assumption.
  - now apply (Source.decoded_region_nodes_match_generated_source
      (S (length bytes)) bytes program 0 (length bytes) nodes source).
  - now apply accepted_mixed_children_preserve_source_order.
Qed.

Definition nested_new_map_par_wire : list nat :=
  [34; 5; 34; 3; 10; 1; 97].

Definition nested_pathmap_par_wire : list nat :=
  [42; 38; 130; 2; 35] ++ Auxiliary.empty_pathmap_wire.

Example nested_new_string_map_has_mixed_typed_reference :
  forall hash_fn,
    exists new_raw new_semantic map_value map_semantic,
      mixed_root_reference hash_fn 1 nested_new_map_par_wire =
        Some [MixedMessageResult new_raw new_semantic
          [MixedAuxiliaryResult map_value map_semantic]].
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Example nested_expr_pathmap_has_mixed_typed_reference :
  forall hash_fn,
    exists expr_raw expr_semantic
        path_raw path_semantic path_value path_value_semantic,
      mixed_root_reference hash_fn 1 nested_pathmap_par_wire =
        Some [MixedMessageResult expr_raw expr_semantic
          [MixedMessageResult path_raw path_semantic
            [MixedAuxiliaryResult path_value path_value_semantic]]].
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Inductive recursive_auxiliary_result : Type :=
| RecursiveStringMapResult
    (raw : Schema.raw_field)
    (entry : Source.typed_map_return Semantic.semantic_summary)
| RecursivePathMapResult
    (raw : Schema.raw_field) (snapshot : EPM1.epm1_snapshot)
    (trie : option EPM1ACTree03.P.ByteTrie)
    (values : list (bool * Semantic.semantic_summary)).

Definition recursive_auxiliary_raw (result : recursive_auxiliary_result)
    : Schema.raw_field :=
  match result with
  | RecursiveStringMapResult raw _ | RecursivePathMapResult raw _ _ _ => raw
  end.

Definition recursive_auxiliary_semantic
    (result : recursive_auxiliary_result)
    : option Semantic.semantic_summary :=
  let raw := recursive_auxiliary_raw result in
  let score :=
    match result with
    | RecursiveStringMapResult _ entry =>
        let value_score :=
          match Source.returned_map_par Semantic.semantic_summary entry with
          | None => Some empty_par_canonical_score
          | Some value =>
              match Semantic.sem_score_evidence value with
              | Semantic.ExactScore score => Some score
              | Semantic.ScoreNotDerived => None
              end
          end in
        option_map (fun value =>
          Semantic.ScoreNode (Semantic.score_forest_of_list
            [Semantic.ScoreString
              (Source.returned_map_key Semantic.semantic_summary entry);
             value])) value_score
    | RecursivePathMapResult _ _ _ _ =>
        Some (Semantic.ScoreBytes (Schema.raw_body raw))
    end in
  option_map (fun exact =>
    Semantic.SemanticSummary
      (Schema.raw_tag raw) (Schema.raw_wire raw) (Schema.raw_payload raw)
      (match Schema.raw_program_type_id raw with
       | Some type_id => type_id
       | None => 0
       end)
      (Schema.raw_body raw) false (Some true)
      (Semantic.ExactScore exact) false false) score.

Theorem recursive_string_map_score_preserves_key_and_value_score :
  forall raw entry semantic,
    recursive_auxiliary_semantic
      (RecursiveStringMapResult raw entry) = Some semantic ->
    exists value_score,
      (match Source.returned_map_par Semantic.semantic_summary entry with
       | None => value_score = empty_par_canonical_score
       | Some value =>
           Semantic.sem_score_evidence value =
             Semantic.ExactScore value_score
       end) /\
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore
          (Semantic.ScoreNode (Semantic.score_forest_of_list
            [Semantic.ScoreString
              (Source.returned_map_key Semantic.semantic_summary entry);
             value_score])) /\
      Semantic.sem_complete semantic = Some true.
Proof.
  intros raw entry semantic accepted.
  unfold recursive_auxiliary_semantic in accepted; simpl in accepted.
  destruct (Source.returned_map_par Semantic.semantic_summary entry)
    as [value|] eqn:present.
  - destruct (Semantic.sem_score_evidence value)
      as [value_score|] eqn:score; [|discriminate].
    inversion accepted; subst semantic.
    exists value_score. now repeat split.
  - inversion accepted; subst semantic.
    exists empty_par_canonical_score. now repeat split.
Qed.

Inductive recursive_mixed_result : Type :=
| RecursiveScalarResult
    (raw : Schema.raw_field) (semantic : Semantic.semantic_summary)
| RecursiveMessageResult
    (raw : Schema.raw_field) (semantic : Semantic.semantic_summary)
    (children : list recursive_mixed_result)
| RecursiveAuxiliaryResult
    (auxiliary : recursive_auxiliary_result)
    (semantic : Semantic.semantic_summary).

Definition recursive_result_semantic (result : recursive_mixed_result)
    : Semantic.semantic_summary :=
  match result with
  | RecursiveScalarResult _ semantic
  | RecursiveMessageResult _ semantic _
  | RecursiveAuxiliaryResult _ semantic => semantic
  end.

Fixpoint recursive_new_map_returns (children : list recursive_mixed_result)
    : list (Source.typed_map_return Semantic.semantic_summary) :=
  match children with
  | [] => []
  | RecursiveAuxiliaryResult (RecursiveStringMapResult _ entry) _ :: rest =>
      entry :: recursive_new_map_returns rest
  | _ :: rest => recursive_new_map_returns rest
  end.

Fixpoint ordered_recursive_new_map_returnsb
    (prior : option (list nat))
    (entries : list (Source.typed_map_return Semantic.semantic_summary))
    : bool :=
  match entries with
  | [] => true
  | entry :: rest =>
      Source.map_key_follows prior
        (Source.returned_map_key Semantic.semantic_summary entry) &&
      ordered_recursive_new_map_returnsb
        (Some (Source.returned_map_key Semantic.semantic_summary entry)) rest
  end.

Lemma ordered_recursive_new_map_returnsb_iff_source_order :
  forall prior entries,
    ordered_recursive_new_map_returnsb prior entries = true <->
    Source.ordered_map_returns_after prior entries.
Proof.
  intros prior entries.
  revert prior.
  induction entries as [|entry rest IH]; intros prior; simpl.
  - split; intro; [exact I|reflexivity].
  - rewrite andb_true_iff, IH. tauto.
Qed.

Definition recursive_auxiliary_reference
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (slot : nat)
    (node : Source.source_tree)
    : option recursive_mixed_result :=
  match node, checked_generated_raw bytes program node with
  | Source.SourceAuxiliary _ field, Some raw =>
      if Schema.descriptor_slot field =? slot then
        let auxiliary :=
          match Schema.descriptor_payload field with
          | Schema.StringMap =>
              option_map (RecursiveStringMapResult raw)
                (Source.reduce_source_map_node bytes program slot
                  par_child node)
          | Schema.PathMap =>
              match EPM1ACTree03.decode_epm1_actree03 hash_fn
                  (Source.source_body bytes node) with
              | Some (snapshot, trie) =>
                  option_map (RecursivePathMapResult raw snapshot trie)
                    (Source.sequence_source_results
                      (map par_child (EPM1.value_bodies snapshot)))
              | None => None
              end
          | Schema.Scalar | Schema.Message => None
          end in
        match auxiliary with
        | Some result =>
            option_map (RecursiveAuxiliaryResult result)
              (recursive_auxiliary_semantic result)
        | None => None
        end
      else None
  | _, _ => None
  end.

Theorem recursive_pathmap_reference_preserves_physical_decode_and_value_order :
  forall par_child hash_fn bytes program slot span field raw
      snapshot trie values semantic,
    recursive_auxiliary_reference par_child hash_fn bytes program slot
      (Source.SourceAuxiliary span field) =
      Some (RecursiveAuxiliaryResult
        (RecursivePathMapResult raw snapshot trie values) semantic) ->
    Source.source_descriptor program span = Some field /\
    Source.raw_field_of_source bytes
      (Source.SourceAuxiliary span field) = Some raw /\
    Schema.raw_body raw =
      Source.source_body bytes (Source.SourceAuxiliary span field) /\
    EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)) =
        Some (snapshot, trie) /\
    Forall2 (fun body value => par_child body = Some value)
      (EPM1.value_bodies snapshot) values /\
    Semantic.sem_score_evidence semantic =
      Semantic.ExactScore (Semantic.ScoreBytes (Schema.raw_body raw)) /\
    Semantic.sem_complete semantic = Some true /\
    Semantic.sem_body semantic = Schema.raw_body raw.
Proof.
  intros par_child hash_fn bytes program slot span field raw
    snapshot trie values semantic accepted.
  unfold recursive_auxiliary_reference in accepted.
  destruct (checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [candidate|] eqn:checked;
    [|discriminate].
  destruct (Schema.descriptor_slot field =? slot);
    [|discriminate].
  destruct (Schema.descriptor_payload field);
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot par_child
      (Source.SourceAuxiliary span field)) as [entry|];
      cbn in accepted; try discriminate.
    destruct (recursive_auxiliary_semantic
      (RecursiveStringMapResult candidate entry));
      discriminate.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[decoded_snapshot decoded_trie]|] eqn:physical;
      [|discriminate].
    destruct (Source.sequence_source_results
      (map par_child (EPM1.value_bodies decoded_snapshot)))
      as [converted|] eqn:ordered; [|discriminate].
    destruct (recursive_auxiliary_semantic
      (RecursivePathMapResult candidate decoded_snapshot
        decoded_trie converted)) as [derived|] eqn:scored;
      [|discriminate].
    inversion accepted; subst.
    destruct (checked_generated_raw_preserves_descriptor_body_and_validity
      bytes program (Source.SourceAuxiliary span field)
      raw checked) as [descriptor [source [body _]]].
    repeat split; try assumption.
    + exact (source_sequence_has_positional_raw_trees
        (list nat) (bool * Semantic.semantic_summary) par_child
        (EPM1.value_bodies snapshot) values ordered).
Qed.

Theorem recursive_string_map_reference_reenters_guarded_par_value :
  forall par_child hash_fn bytes program slot span field raw
      entry semantic,
    recursive_auxiliary_reference par_child hash_fn bytes program slot
      (Source.SourceAuxiliary span field) =
      Some (RecursiveAuxiliaryResult
        (RecursiveStringMapResult raw entry) semantic) ->
    Source.guarded_source_map_member bytes program slot par_child
      (Source.SourceAuxiliary span field) entry /\
    Source.source_descriptor program span = Some field /\
    Schema.raw_body raw =
      Source.source_body bytes (Source.SourceAuxiliary span field) /\
    Semantic.sem_complete semantic = Some true /\
    Semantic.sem_body semantic = Schema.raw_body raw /\
    exists value_score,
      (match Source.returned_map_par Semantic.semantic_summary entry with
       | None => value_score = empty_par_canonical_score
       | Some value =>
           Semantic.sem_score_evidence value =
             Semantic.ExactScore value_score
       end) /\
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore
          (Semantic.ScoreNode (Semantic.score_forest_of_list
            [Semantic.ScoreString
              (Source.returned_map_key Semantic.semantic_summary entry);
             value_score])) /\
      Semantic.sem_complete semantic = Some true.
Proof.
  intros par_child hash_fn bytes program slot span field raw
    entry semantic accepted.
  unfold recursive_auxiliary_reference in accepted.
  destruct (checked_generated_raw bytes program
    (Source.SourceAuxiliary span field)) as [candidate|] eqn:checked;
    [|discriminate].
  destruct (Schema.descriptor_slot field =? slot);
    [|discriminate].
  destruct (Schema.descriptor_payload field);
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program slot par_child
      (Source.SourceAuxiliary span field)) as [returned|]
      eqn:reduced; [|discriminate].
    cbn in accepted.
    destruct (recursive_auxiliary_semantic
      (RecursiveStringMapResult candidate returned))
      as [derived|] eqn:scored;
      cbn in accepted.
    2: discriminate.
    inversion accepted; subst.
    destruct (checked_generated_raw_preserves_descriptor_body_and_validity
      bytes program (Source.SourceAuxiliary span field)
      raw checked) as [descriptor [_ [body _]]].
    repeat split; try assumption.
    + apply (proj1 (Source.guarded_source_map_member_exact_reduction
        Semantic.semantic_summary bytes program slot par_child
        (Source.SourceAuxiliary span field) entry)).
      exact reduced.
    + unfold recursive_auxiliary_semantic in scored.
      destruct (Source.returned_map_par Semantic.semantic_summary entry)
        as [value|]; simpl in scored.
      * destruct (Semantic.sem_score_evidence value);
          inversion scored; reflexivity.
      * inversion scored; reflexivity.
    + unfold recursive_auxiliary_semantic in scored.
      destruct (Source.returned_map_par Semantic.semantic_summary entry)
        as [value|]; simpl in scored.
      * destruct (Semantic.sem_score_evidence value);
          inversion scored; reflexivity.
      * inversion scored; reflexivity.
    + now apply (recursive_string_map_score_preserves_key_and_value_score
        raw entry semantic).
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|discriminate].
    destruct (Source.sequence_source_results
      (map par_child (EPM1.value_bodies snapshot))) as [converted|];
      cbn in accepted; try discriminate.
Qed.

Fixpoint recursive_source_reference
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (node : Source.source_tree)
    {struct node} : option recursive_mixed_result :=
  match node with
  | Source.SourceScalar _ _ =>
      match checked_generated_scalar_raw bytes program node with
      | None => None
      | Some raw =>
          let semantic := Semantic.reduce_semantic_field raw [] in
          if Semantic.sem_definitely_refused semantic then None
          else Some (RecursiveScalarResult raw semantic)
      end
  | Source.SourceMessage _ field children =>
      match checked_generated_raw bytes program node,
            Schema.descriptor_child_type_id field with
      | Some raw, Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | None => None
          | Some child_program =>
              let fix visit_children (remaining : list Source.source_tree)
                  : option (list recursive_mixed_result) :=
                match remaining with
                | [] => Some []
                | first :: rest =>
                    match recursive_source_reference par_child hash_fn bytes
                            child_program first,
                          visit_children rest with
                    | Some head, Some tail => Some (head :: tail)
                    | _, _ => None
                    end
                end in
              match visit_children children with
              | None => None
              | Some converted =>
                  let map_ordered :=
                    if type_id =? 20 then
                      ordered_recursive_new_map_returnsb None
                        (recursive_new_map_returns converted)
                    else true in
                  if map_ordered then
                    let semantic := Semantic.reduce_semantic_field raw
                      (map recursive_result_semantic converted) in
                    if Semantic.sem_definitely_refused semantic then None
                    else Some (RecursiveMessageResult raw semantic converted)
                  else None
              end
          end
      | _, _ => None
      end
  | Source.SourceAuxiliary _ field =>
      recursive_auxiliary_reference par_child hash_fn bytes program
        (Schema.descriptor_slot field) node
  end.

Fixpoint recursive_source_children
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (program : Schema.generated_program) (nodes : list Source.source_tree)
    {struct nodes} : option (list recursive_mixed_result) :=
  match nodes with
  | [] => Some []
  | first :: rest =>
      match recursive_source_reference par_child hash_fn bytes program first,
            recursive_source_children par_child hash_fn bytes program rest with
      | Some head, Some tail => Some (head :: tail)
      | _, _ => None
      end
  end.

Lemma local_recursive_children_are_global_children :
  forall par_child hash_fn bytes program nodes,
    (let fix visit_children (remaining : list Source.source_tree)
        : option (list recursive_mixed_result) :=
      match remaining with
      | [] => Some []
      | first :: rest =>
          match recursive_source_reference par_child hash_fn bytes
                  program first,
                visit_children rest with
          | Some head, Some tail => Some (head :: tail)
          | _, _ => None
          end
      end in visit_children nodes) =
    recursive_source_children par_child hash_fn bytes program nodes.
Proof.
  intros par_child hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; simpl; [reflexivity|].
  now rewrite <- IH.
Qed.

Lemma accepted_recursive_children_preserve_source_order :
  forall par_child hash_fn bytes program nodes results,
    recursive_source_children par_child hash_fn bytes program nodes =
      Some results ->
    Forall2 (fun node result =>
      recursive_source_reference par_child hash_fn bytes program node =
        Some result) nodes results.
Proof.
  intros par_child hash_fn bytes program nodes.
  induction nodes as [|first rest IH]; intros results accepted.
  - simpl in accepted. inversion accepted. constructor.
  - simpl in accepted.
    destruct (recursive_source_reference par_child hash_fn bytes
      program first) as [head|] eqn:converted_first;
      [|discriminate].
    destruct (recursive_source_children par_child hash_fn bytes
      program rest) as [tail|] eqn:converted_rest;
      [|discriminate].
    inversion accepted; subst results.
    constructor; [exact converted_first|].
    exact (IH tail eq_refl).
Qed.

Theorem accepted_recursive_new_preserves_source_and_strict_map_key_order :
  forall par_child hash_fn bytes program span field children
      raw semantic results,
    Schema.descriptor_child_type_id field = Some 20 ->
    recursive_source_reference par_child hash_fn bytes program
      (Source.SourceMessage span field children) =
        Some (RecursiveMessageResult raw semantic results) ->
    exists child_program,
      Schema.lookup_projected_program 20 [] [] = Some child_program /\
      checked_generated_raw bytes program
        (Source.SourceMessage span field children) = Some raw /\
      Forall2 (fun child result =>
        recursive_source_reference par_child hash_fn bytes
          child_program child = Some result) children results /\
      Source.ordered_map_returns_after None
        (recursive_new_map_returns results) /\
      semantic = Semantic.reduce_semantic_field raw
        (map recursive_result_semantic results) /\
      Semantic.sem_body semantic =
        Source.source_body bytes (Source.SourceMessage span field children) /\
      Semantic.sem_definitely_refused semantic = false.
Proof.
  intros par_child hash_fn bytes program span field children
    raw semantic results child_type accepted.
  cbn [recursive_source_reference] in accepted.
  destruct (checked_generated_raw bytes program
    (Source.SourceMessage span field children)) as [candidate|]
    eqn:checked; [|discriminate].
  rewrite child_type in accepted.
  destruct (Schema.lookup_projected_program 20 [] [])
    as [child_program|] eqn:looked_up; [|discriminate].
  cbn [Nat.eqb] in accepted.
  rewrite local_recursive_children_are_global_children in accepted.
  destruct (recursive_source_children par_child hash_fn bytes
    child_program children) as [converted|] eqn:descendants;
    [|discriminate].
  destruct (ordered_recursive_new_map_returnsb None
    (recursive_new_map_returns converted)) eqn:ordered;
    [|discriminate].
  destruct (Semantic.sem_definitely_refused
    (Semantic.reduce_semantic_field candidate
      (map recursive_result_semantic converted))) eqn:refused;
    [discriminate|].
  inversion accepted; subst raw semantic results.
  exists child_program.
  repeat split; try assumption; try reflexivity.
  - now apply accepted_recursive_children_preserve_source_order.
  - now apply ordered_recursive_new_map_returnsb_iff_source_order.
  - destruct (checked_generated_raw_preserves_descriptor_body_and_validity
      bytes program (Source.SourceMessage span field children)
      candidate checked) as [_ [_ [body _]]].
    simpl. exact body.
Qed.

Lemma complete_new_reducer_uses_exact_child_scores :
  forall raw children score,
    let semantic := Semantic.reduce_semantic_field raw children in
    Semantic.sem_type_id semantic = 20 ->
    Semantic.sem_complete semantic = Some true ->
    Semantic.exact_new_score children = Some score ->
    Semantic.sem_score_evidence semantic = Semantic.ExactScore score.
Proof.
  intros raw children score semantic semantic_type complete exact.
  subst semantic.
  unfold Semantic.reduce_semantic_field in *.
  cbn [Semantic.sem_type_id Semantic.sem_complete
    Semantic.sem_score_evidence] in *.
  rewrite complete, semantic_type.
  simpl [Semantic.score_evidence_for Semantic.exact_or_unknown].
  now rewrite exact.
Qed.

Theorem accepted_complete_recursive_new_score_retains_ordered_injections :
  forall par_child hash_fn bytes program span field children
      raw semantic results bind_count body_score injections,
    Schema.descriptor_child_type_id field = Some 20 ->
    recursive_source_reference par_child hash_fn bytes program
      (Source.SourceMessage span field children) =
        Some (RecursiveMessageResult raw semantic results) ->
    Semantic.sem_type_id semantic = 20 ->
    Semantic.sem_complete semantic = Some true ->
    Semantic.new_bind_count_word
      (map recursive_result_semantic results) = Some bind_count ->
    Semantic.exact_child_score 2
      (map recursive_result_semantic results) = Some body_score ->
    Semantic.exact_selected_scores 4
      (map recursive_result_semantic results) = Some injections ->
    Source.ordered_map_returns_after None
      (recursive_new_map_returns results) /\
    Semantic.sem_score_evidence semantic =
      Semantic.ExactScore (Semantic.score_constructor 303
        (Semantic.ScoreInteger bind_count ::
          (Semantic.scores_or_absent
            (map (fun child => Semantic.ScoreString
              (Semantic.sem_body child))
              (filter (fun child => Semantic.sem_tag child =? 3)
                (map recursive_result_semantic results))) ++
           Semantic.scores_or_absent injections ++ [body_score]))).
Proof.
  intros par_child hash_fn bytes program span field children
    raw semantic results bind_count body_score injections
    child_type accepted semantic_type complete bind body selected.
  destruct (accepted_recursive_new_preserves_source_and_strict_map_key_order
    par_child hash_fn bytes program span field children
    raw semantic results child_type accepted) as
    [child_program [_ [_ [_ [ordered [reduced _]]]]]].
  split; [exact ordered|].
  pose proof (Semantic.new_score_retains_all_uris_injections_and_body
    (map recursive_result_semantic results)
    bind_count body_score injections bind body selected) as exact.
  rewrite reduced in semantic_type, complete |- *.
  now apply (complete_new_reducer_uses_exact_child_scores
    raw (map recursive_result_semantic results)).
Qed.

Definition shorter_body (child parent : list nat) : Prop :=
  length child < length parent.

Definition guarded_recursive_par_child (parent : list nat)
    (recurse : forall child, shorter_body child parent ->
      option (bool * Semantic.semantic_summary))
    (child : list nat) : option (bool * Semantic.semantic_summary) :=
  match lt_dec (length child) (length parent) with
  | left shorter => recurse child shorter
  | right _ => None
  end.

Lemma source_body_length_at_most_parent_bytes :
  forall bytes node,
    length (Source.source_body bytes node) <= length bytes.
Proof.
  intros bytes node.
  unfold Source.source_body.
  rewrite firstn_length, skipn_length.
  destruct (le_dec
    (Wire.span_end (Source.source_span node) -
      Wire.span_payload_start (Source.source_span node))
    (length bytes - Wire.span_payload_start (Source.source_span node)));
    [rewrite Nat.min_l by lia|rewrite Nat.min_r by lia]; lia.
Qed.

Theorem decoded_pathmap_values_satisfy_recursive_length_guard :
  forall parent node hash_fn snapshot trie value
      (recurse : forall child, shorter_body child parent ->
        option (bool * Semantic.semantic_summary)),
    EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body parent node) = Some (snapshot, trie) ->
    In value (EPM1.value_bodies snapshot) ->
    exists shorter : shorter_body value parent,
      guarded_recursive_par_child parent recurse value =
        recurse value shorter.
Proof.
  intros parent node hash_fn snapshot trie value recurse
    physical member.
  pose proof (EPM1ACTree03.decode_epm1_actree03_refines_epm1
    hash_fn (Source.source_body parent node) snapshot trie physical)
    as decoded.
  pose proof (decoded_epm1_value_bodies_are_strictly_shorter_than_snapshot
    (Source.source_body parent node) snapshot decoded) as bounded.
  rewrite Forall_forall in bounded.
  specialize (bounded value member).
  pose proof (source_body_length_at_most_parent_bytes parent node)
    as inside.
  assert (shorter : shorter_body value parent).
  { unfold shorter_body.
    eapply Nat.lt_le_trans; [exact bounded|exact inside]. }
  exists shorter.
  unfold guarded_recursive_par_child.
  destruct (lt_dec (@length nat value) (@length nat parent))
    as [selected|too_long].
  - assert (selected = shorter) by apply Peano_dec.le_unique.
    subst selected. reflexivity.
  - exfalso. apply too_long. exact shorter.
Qed.

Theorem refined_string_map_value_satisfies_recursive_length_guard :
  forall parent program outer_span outer_field typed value_span value
      (recurse : forall child, shorter_body child parent ->
        option (bool * Semantic.semantic_summary)),
    Source.refine_string_map_entry parent program
      (Source.SourceAuxiliary outer_span outer_field) = Some typed ->
    Source.typed_map_value_source typed = Some (value_span, value) ->
    exists shorter : shorter_body value parent,
      guarded_recursive_par_child parent recurse value =
        recurse value shorter.
Proof.
  intros parent program outer_span outer_field typed value_span value
    recurse refined present.
  destruct (refined_string_map_value_is_strictly_inside_parent_bytes
    parent program outer_span outer_field typed value_span value
    refined present) as [_ [_ [_ shorter]]].
  exists shorter.
  unfold guarded_recursive_par_child.
  destruct (lt_dec (length value) (length parent))
    as [selected|too_long].
  - assert (selected = shorter) by apply Peano_dec.le_unique.
    subst selected. reflexivity.
  - exfalso. apply too_long. exact shorter.
Qed.

Lemma shorter_body_is_well_founded : well_founded shorter_body.
Proof.
  unfold shorter_body.
  apply (well_founded_ltof (list nat) (@length nat)).
Defined.

Definition recursive_par_step (hash_fn : list nat -> nat)
    (body : list nat)
    (recurse : forall child, shorter_body child body ->
      option (bool * Semantic.semantic_summary))
    : option (bool * Semantic.semantic_summary) :=
  let nested_par := guarded_recursive_par_child body recurse in
  match Schema.lookup_projected_program 1 [] [],
        Source.decode_root 1 body with
  | Some program, Some nodes =>
      match recursive_source_children nested_par hash_fn body program nodes,
            Source.source_par_semantic_default body nodes with
      | Some children, Some semantic_default =>
          let semantic := Semantic.reduce_semantic_field (par_raw body)
            (map recursive_result_semantic children) in
          if Semantic.sem_definitely_refused semantic then None
          else match Semantic.sem_score_evidence semantic with
               | Semantic.ExactScore _ =>
                   Some (semantic_default, semantic)
               | Semantic.ScoreNotDerived => None
               end
      | _, _ => None
      end
  | _, _ => None
  end.

Definition recursive_par_reference (hash_fn : list nat -> nat)
    : list nat -> option (bool * Semantic.semantic_summary) :=
  Fix shorter_body_is_well_founded
    (fun _ => option (bool * Semantic.semantic_summary))
    (recursive_par_step hash_fn).

Lemma recursive_par_step_has_source_default_and_body :
  forall hash_fn body recurse semantic_default semantic,
    recursive_par_step hash_fn body recurse =
      Some (semantic_default, semantic) ->
    exists program nodes,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_root 1 body = Some nodes /\
      Source.source_par_semantic_default body nodes =
        Some semantic_default /\
      Semantic.sem_body semantic = body /\
      Semantic.sem_definitely_refused semantic = false /\
      exists score,
        Semantic.sem_score_evidence semantic =
          Semantic.ExactScore score.
Proof.
  intros hash_fn body recurse semantic_default semantic accepted.
  unfold recursive_par_step in accepted.
  destruct (Schema.lookup_projected_program 1 [] [])
    as [program|] eqn:looked_up; [|discriminate].
  destruct (Source.decode_root 1 body)
    as [nodes|] eqn:decoded; [|discriminate].
  destruct (recursive_source_children
    (guarded_recursive_par_child body recurse)
    hash_fn body program nodes) as [children|]; [|discriminate].
  destruct (Source.source_par_semantic_default body nodes)
    as [observed|] eqn:source_default; [|discriminate].
  set (reduced := Semantic.reduce_semantic_field (par_raw body)
    (map recursive_result_semantic children)) in *.
  destruct (Semantic.sem_definitely_refused reduced)
    eqn:not_refused; [discriminate|].
  destruct (Semantic.sem_score_evidence reduced)
    as [score|] eqn:scored; [|discriminate].
  inversion accepted; subst semantic_default semantic.
  exists program, nodes.
  split; [reflexivity|].
  split; [reflexivity|].
  split; [exact source_default|].
  split.
  - unfold reduced, Semantic.reduce_semantic_field, par_raw.
    reflexivity.
  - split; [exact not_refused|].
    exists score. exact scored.
Qed.

Theorem accepted_recursive_par_has_original_body_and_source_default :
  forall hash_fn body semantic_default semantic,
    recursive_par_reference hash_fn body =
      Some (semantic_default, semantic) ->
    exists program nodes,
      Schema.lookup_projected_program 1 [] [] = Some program /\
      Source.decode_root 1 body = Some nodes /\
      Source.source_par_semantic_default body nodes =
        Some semantic_default /\
      Semantic.sem_body semantic = body /\
      Semantic.sem_definitely_refused semantic = false /\
      exists score,
        Semantic.sem_score_evidence semantic =
          Semantic.ExactScore score.
Proof.
  intros hash_fn body semantic_default semantic accepted.
  unfold recursive_par_reference, Fix in accepted.
  destruct (shorter_body_is_well_founded body) as [recurse].
  cbn [Fix_F] in accepted.
  now apply recursive_par_step_has_source_default_and_body in accepted.
Qed.

Theorem independently_accepted_typed_and_recursive_par_agree_on_source :
  forall hash_fn body typed_default typed_summary recursive_default
      recursive_summary,
    Reference.source_reference_par_of_body body =
      Some (typed_default, typed_summary) ->
    recursive_par_reference hash_fn body =
      Some (recursive_default, recursive_summary) ->
    typed_default = recursive_default /\
    Schema.summary_body typed_summary =
      Semantic.sem_body recursive_summary /\
    Semantic.sem_definitely_refused recursive_summary = false /\
    exists score,
      Semantic.sem_score_evidence recursive_summary =
        Semantic.ExactScore score.
Proof.
  intros hash_fn body typed_default typed_summary recursive_default
    recursive_summary typed recursive.
  destruct (Reference.source_reference_par_body_has_decoded_reference
    body typed_default typed_summary typed) as
    [typed_program [typed_nodes [typed_root
      [typed_lookup [typed_decoded [_ [typed_source
        [_ [_ [typed_body _]]]]]]]]]].
  destruct (accepted_recursive_par_has_original_body_and_source_default
    hash_fn body recursive_default recursive_summary recursive) as
    [recursive_program [recursive_nodes
      [recursive_lookup [recursive_decoded [recursive_source
        [recursive_body [not_refused exact]]]]]]].
  rewrite typed_lookup in recursive_lookup.
  inversion recursive_lookup; subst recursive_program.
  unfold Source.decode_root in recursive_decoded.
  rewrite typed_lookup, typed_decoded in recursive_decoded.
  inversion recursive_decoded; subst recursive_nodes.
  rewrite typed_source in recursive_source.
  inversion recursive_source; subst recursive_default.
  repeat split; try assumption; try reflexivity.
  now rewrite typed_body, recursive_body.
Qed.

Definition recursive_mixed_root_reference
    (hash_fn : list nat -> nat) (type_id : nat) (bytes : list nat)
    : option (list recursive_mixed_result) :=
  match Schema.lookup_projected_program type_id [] [],
        Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      recursive_source_children (recursive_par_reference hash_fn)
        hash_fn bytes program nodes
  | _, _ => None
  end.

Example recursive_reference_refuses_default_par_connective :
  forall hash_fn,
    recursive_mixed_root_reference hash_fn 1 [80; 0] = None.
Proof. intro hash_fn. vm_compute. reflexivity. Qed.

Definition reversed_incomplete_send_par_wire : list nat :=
  [10; 2; 24; 1; 10; 0].

Example typed_reference_accepts_reversed_incomplete_sends :
  exists semantic_default summary,
    Reference.source_reference_par_of_body
      reversed_incomplete_send_par_wire =
        Some (semantic_default, summary).
Proof. vm_compute. repeat eexists. Qed.

Example recursive_reference_refuses_reversed_incomplete_sends :
  forall hash_fn,
    recursive_par_reference hash_fn
      reversed_incomplete_send_par_wire = None.
Proof. intro hash_fn. vm_compute. reflexivity. Qed.

Example recursive_reference_keeps_oneof_zero_bool :
  forall hash_fn,
    exists raw semantic,
      recursive_mixed_root_reference hash_fn 24 [8; 0] =
        Some [RecursiveScalarResult raw semantic].
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Definition nested_map_value_pathmap_par_wire : list nat :=
  let entry := [10; 1; 107] ++
    [18] ++ EPM1.encode_frame nested_pathmap_par_wire in
  let new_body := [34] ++ EPM1.encode_frame entry in
  [34] ++ EPM1.encode_frame new_body.

Example recursive_map_value_reenters_nested_pathmap :
  forall hash_fn, exists results,
    recursive_mixed_root_reference hash_fn 1
      nested_map_value_pathmap_par_wire = Some results.
Proof. intro hash_fn. vm_compute. repeat eexists. Qed.

Example legacy_typed_par_refuses_accepted_nested_auxiliary :
  forall hash_fn,
    Reference.source_reference_par_of_body
      nested_map_value_pathmap_par_wire = None /\
    exists semantic_default semantic,
      recursive_par_reference hash_fn
        nested_map_value_pathmap_par_wire =
          Some (semantic_default, semantic).
Proof.
  intro hash_fn. split.
  - vm_compute. reflexivity.
  - vm_compute. repeat eexists.
Qed.

Definition ordered_two_absent_value_maps_new_wire : list nat :=
  [34; 12; 18; 0;
   34; 3; 10; 1; 97;
   34; 3; 10; 1; 98].

Definition reversed_two_absent_value_maps_new_wire : list nat :=
  [34; 12; 18; 0;
   34; 3; 10; 1; 98;
   34; 3; 10; 1; 97].

Definition absent_value_map_score (key : list nat)
    : Semantic.canonical_score :=
  Semantic.ScoreNode (Semantic.score_forest_of_list
    [Semantic.ScoreString key; empty_par_canonical_score]).

Definition ordered_two_absent_value_maps_new_score
    : Semantic.canonical_score :=
  Semantic.score_constructor 303
    [Semantic.ScoreInteger 0%N; Semantic.ScoreInteger 0%N;
     absent_value_map_score [97]; absent_value_map_score [98];
     empty_par_canonical_score].

Example ordered_two_absent_value_maps_preserve_keys_and_complete_new_score :
  forall hash_fn,
    match recursive_mixed_root_reference hash_fn 1
        ordered_two_absent_value_maps_new_wire with
    | Some [RecursiveMessageResult _ semantic children] =>
        map (Source.returned_map_key Semantic.semantic_summary)
          (recursive_new_map_returns children) = [[97]; [98]] /\
        Semantic.sem_complete semantic = Some true /\
        Semantic.sem_score_evidence semantic =
          Semantic.ExactScore ordered_two_absent_value_maps_new_score
    | _ => False
    end.
Proof. intro hash_fn. vm_compute. repeat split; reflexivity. Qed.

Example reversed_two_absent_value_maps_are_refused_by_parent_order :
  forall hash_fn,
    recursive_mixed_root_reference hash_fn 1
      reversed_two_absent_value_maps_new_wire = None.
Proof. intro hash_fn. vm_compute. reflexivity. Qed.

Example ordered_two_absent_value_maps_have_mixed_typed_reference :
  forall hash_fn,
    match mixed_root_reference hash_fn 1
        ordered_two_absent_value_maps_new_wire with
    | Some [MixedMessageResult _ _ children] =>
        map (Source.returned_map_key Schema.typed_summary)
          (mixed_new_map_returns children) = [[97]; [98]]
    | _ => False
    end.
Proof. intro hash_fn. vm_compute. reflexivity. Qed.

Example reversed_two_absent_value_maps_have_no_mixed_typed_reference :
  forall hash_fn,
    mixed_root_reference hash_fn 1
      reversed_two_absent_value_maps_new_wire = None.
Proof. intro hash_fn. vm_compute. reflexivity. Qed.

Definition pathmap_par_value_observation
    (typed : bool * Schema.typed_summary)
    (recursive : bool * Semantic.semantic_summary) : Prop :=
  fst typed = fst recursive /\
  Schema.summary_body (snd typed) = Semantic.sem_body (snd recursive).

Inductive pathmap_mixed_result_relation :
    mixed_typed_result -> recursive_mixed_result -> Prop :=
| PathMapMixedResultRelation :
    forall raw snapshot trie typed_values recursive_values semantic,
      Forall2 pathmap_par_value_observation
        typed_values recursive_values ->
      pathmap_mixed_result_relation
        (MixedAuxiliaryResult
          (Auxiliary.PathMapResult raw snapshot trie typed_values)
          semantic)
        (RecursiveAuxiliaryResult
          (RecursivePathMapResult raw snapshot trie recursive_values)
          semantic).

Lemma pathmap_semantic_cuts_agree_for_any_values :
  forall raw snapshot trie typed_values recursive_values,
    auxiliary_semantic_cut
      (Auxiliary.PathMapResult raw snapshot trie typed_values) =
    recursive_auxiliary_semantic
      (RecursivePathMapResult raw snapshot trie recursive_values).
Proof. reflexivity. Qed.

Lemma positional_pathmap_values_have_recursive_source_sequence :
  forall par_child bodies typed_values recursive_values,
    Forall2 (fun body typed =>
      Reference.source_reference_par_of_body body = Some typed /\
      Schema.summary_body (snd typed) = body /\
      Schema.summary_valid (snd typed) = true)
      bodies typed_values ->
    Forall2 (fun typed recursive =>
      pathmap_par_value_observation typed recursive /\
      par_child (Schema.summary_body (snd typed)) = Some recursive)
      typed_values recursive_values ->
    Source.sequence_source_results (map par_child bodies) =
      Some recursive_values /\
    Forall2 pathmap_par_value_observation typed_values recursive_values.
Proof.
  intros par_child bodies typed_values recursive_values positional.
  revert recursive_values.
  induction positional as
    [|body typed bodies typed_values head_properties later IH];
    intros recursive_values related.
  - inversion related; subst. split; [reflexivity|constructor].
  - inversion related as
      [|typed' recursive typed_tail recursive_tail
        [observed child] remaining]; subst.
    destruct head_properties as [_ [original _]].
    destruct (IH recursive_tail remaining) as [tail_sequence tail_related].
    split.
    + simpl. rewrite <- original, child, tail_sequence. reflexivity.
    + constructor; assumption.
Qed.

Lemma independently_accepted_pathmap_value_sequences_agree :
  forall hash_fn bodies typed_values recursive_values,
    Forall2 (fun body typed =>
      Reference.source_reference_par_of_body body = Some typed /\
      Schema.summary_body (snd typed) = body /\
      Schema.summary_valid (snd typed) = true)
      bodies typed_values ->
    Forall2 (fun body recursive =>
      recursive_par_reference hash_fn body = Some recursive)
      bodies recursive_values ->
    Forall2 (fun typed recursive =>
      pathmap_par_value_observation typed recursive /\
      recursive_par_reference hash_fn
        (Schema.summary_body (snd typed)) = Some recursive)
      typed_values recursive_values.
Proof.
  intros hash_fn bodies typed_values recursive_values typed.
  revert recursive_values.
  induction typed as
    [|body typed bodies typed_values typed_head typed_tail IH];
    intros recursive_values recursive.
  - inversion recursive; constructor.
  - inversion recursive as
      [|recursive_body recursive_value recursive_bodies recursive_tail
        recursive_head recursive_rest]; subst.
    constructor.
    + destruct typed as [typed_default typed_summary].
      destruct recursive_value as
        [recursive_default recursive_summary].
      destruct typed_head as [typed_source [typed_body _]].
      destruct (independently_accepted_typed_and_recursive_par_agree_on_source
        hash_fn body typed_default typed_summary recursive_default
        recursive_summary typed_source recursive_head) as
        [same_default [same_body _]].
      split.
      * unfold pathmap_par_value_observation; simpl.
        now split.
      * simpl in typed_body |- *.
        rewrite typed_body.
        exact recursive_head.
    + now apply IH.
Qed.

Theorem independently_accepted_pathmap_sources_have_same_physical_values :
  forall hash_fn bytes program slot span field
      typed_raw typed_snapshot typed_trie typed_values
      recursive_raw recursive_snapshot recursive_trie recursive_values
      recursive_semantic,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot
      (Source.SourceAuxiliary span field) =
      Some (Auxiliary.PathMapResult typed_raw typed_snapshot
        typed_trie typed_values) ->
    recursive_auxiliary_reference (recursive_par_reference hash_fn)
      hash_fn bytes program slot (Source.SourceAuxiliary span field) =
      Some (RecursiveAuxiliaryResult
        (RecursivePathMapResult recursive_raw recursive_snapshot
          recursive_trie recursive_values) recursive_semantic) ->
    typed_raw = recursive_raw /\
    typed_snapshot = recursive_snapshot /\
    typed_trie = recursive_trie /\
    Forall2 (fun typed recursive =>
      pathmap_par_value_observation typed recursive /\
      recursive_par_reference hash_fn
        (Schema.summary_body (snd typed)) = Some recursive)
      typed_values recursive_values.
Proof.
  intros hash_fn bytes program slot span field
    typed_raw typed_snapshot typed_trie typed_values
    recursive_raw recursive_snapshot recursive_trie recursive_values
    recursive_semantic typed recursive.
  destruct (Auxiliary.accepted_auxiliary_has_exact_generated_raw_leaf
    hash_fn bytes program slot (Source.SourceAuxiliary span field)
    (Auxiliary.PathMapResult typed_raw typed_snapshot
      typed_trie typed_values) typed) as
    [typed_span [typed_field [source_raw
      [source_node [_ [_ [typed_source [_ [typed_raw_is_source _]]]]]]]]].
  inversion source_node; subst typed_span typed_field.
  simpl in typed_raw_is_source.
  subst source_raw.
  destruct (Auxiliary.accepted_pathmap_has_physical_and_positional_reference
    hash_fn bytes program slot (Source.SourceAuxiliary span field)
    typed_raw typed_snapshot typed_trie typed_values typed) as
    [typed_physical [_ typed_positional]].
  destruct (recursive_pathmap_reference_preserves_physical_decode_and_value_order
    (recursive_par_reference hash_fn) hash_fn bytes program slot span
    field recursive_raw recursive_snapshot recursive_trie
    recursive_values recursive_semantic recursive) as
    [_ [recursive_source [_ [recursive_physical
      [recursive_positional _]]]]].
  rewrite typed_source in recursive_source.
  inversion recursive_source; subst recursive_raw.
  rewrite typed_physical in recursive_physical.
  inversion recursive_physical;
    subst recursive_snapshot recursive_trie.
  repeat split; try reflexivity.
  eapply independently_accepted_pathmap_value_sequences_agree;
    eassumption.
Qed.

Theorem typed_pathmap_recursive_refusal_has_refused_nested_par :
  forall hash_fn bytes program slot span field raw snapshot trie
      typed_values,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot
      (Source.SourceAuxiliary span field) =
      Some (Auxiliary.PathMapResult raw snapshot trie typed_values) ->
    recursive_auxiliary_reference (recursive_par_reference hash_fn)
      hash_fn bytes program slot (Source.SourceAuxiliary span field) =
      None ->
    exists body,
      In body (EPM1.value_bodies snapshot) /\
      recursive_par_reference hash_fn body = None.
Proof.
  intros hash_fn bytes program slot span field raw snapshot trie
    typed_values typed refused.
  cbn [Auxiliary.typed_auxiliary_reference] in typed.
  destruct (Source.source_descriptor program span) as [selected|]
    eqn:descriptor; [|discriminate].
  destruct (Auxiliary.descriptor_eq_dec field selected)
    as [same|different]; [subst selected|discriminate].
  destruct (Schema.descriptor_slot field =? slot) eqn:selected;
    [|discriminate].
  apply Nat.eqb_eq in selected. subst slot.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate|]
    eqn:raw_source; [|discriminate].
  destruct (Schema.valid_raw_body candidate) eqn:valid;
    [|discriminate].
  destruct (Schema.descriptor_payload field) eqn:payload;
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program
      (Schema.descriptor_slot field)
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)); discriminate.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes
        (Source.SourceAuxiliary span field)))
      as [[decoded_snapshot decoded_trie]|] eqn:physical;
      [|discriminate].
    destruct (Auxiliary.typed_snapshot_values decoded_snapshot)
      as [converted|] eqn:values; [|discriminate].
    inversion typed; subst.
    assert (checked : checked_generated_raw bytes program
      (Source.SourceAuxiliary span field) = Some raw).
    { unfold checked_generated_raw.
      cbn [Source.source_span Source.source_field].
      rewrite descriptor, raw_source, valid.
      destruct (Auxiliary.descriptor_eq_dec field field)
        as [_|different]; [reflexivity|contradiction]. }
    unfold recursive_auxiliary_reference in refused.
    rewrite checked, Nat.eqb_refl, payload, physical in refused.
    destruct (Source.sequence_source_results
      (map (recursive_par_reference hash_fn)
        (EPM1.value_bodies snapshot))) as [recursive_values|]
      eqn:recursive_sequence.
    + cbn [recursive_auxiliary_semantic recursive_auxiliary_raw]
        in refused.
      discriminate.
    + exact (refused_source_sequence_has_refused_member
        (list nat) (bool * Semantic.semantic_summary)
        (recursive_par_reference hash_fn)
        (EPM1.value_bodies snapshot) recursive_sequence).
Qed.

Theorem pathmap_source_independent_typed_recursive_bridge :
  forall par_child hash_fn bytes program slot node raw snapshot trie
      typed_values recursive_values,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot node =
      Some (Auxiliary.PathMapResult raw snapshot trie typed_values) ->
    Forall2 (fun typed recursive =>
      pathmap_par_value_observation typed recursive /\
      par_child (Schema.summary_body (snd typed)) = Some recursive)
      typed_values recursive_values ->
    exists semantic,
      mixed_source_reference hash_fn bytes program node =
        Some (MixedAuxiliaryResult
          (Auxiliary.PathMapResult raw snapshot trie typed_values) semantic) /\
      recursive_source_reference par_child hash_fn bytes program node =
        Some (RecursiveAuxiliaryResult
          (RecursivePathMapResult raw snapshot trie recursive_values)
          semantic) /\
      pathmap_mixed_result_relation
        (MixedAuxiliaryResult
          (Auxiliary.PathMapResult raw snapshot trie typed_values) semantic)
        (RecursiveAuxiliaryResult
          (RecursivePathMapResult raw snapshot trie recursive_values)
          semantic) /\
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore
          (Semantic.ScoreBytes (Schema.raw_body raw)) /\
      Semantic.sem_body semantic = Schema.raw_body raw /\
      Semantic.sem_complete semantic = Some true.
Proof.
  intros par_child hash_fn bytes program slot node raw snapshot trie
    typed_values recursive_values typed related.
  pose proof typed as source_accepted.
  destruct node as [span field|span field children|span field];
    cbn [Auxiliary.typed_auxiliary_reference] in typed;
    try discriminate.
  destruct (Source.source_descriptor program span) as [selected|]
    eqn:descriptor; [|discriminate].
  destruct (Auxiliary.descriptor_eq_dec field selected)
    as [same|different]; [subst selected|discriminate].
  destruct (Schema.descriptor_slot field =? slot) eqn:selected;
    [|discriminate].
  apply Nat.eqb_eq in selected. subst slot.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate|]
    eqn:raw_source; [|discriminate].
  destruct (Schema.valid_raw_body candidate) eqn:valid;
    [|discriminate].
  destruct (Schema.descriptor_payload field) eqn:payload;
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program
      (Schema.descriptor_slot field)
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)); discriminate.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes
        (Source.SourceAuxiliary span field)))
      as [[decoded_snapshot decoded_trie]|] eqn:physical;
      [|discriminate].
    destruct (Auxiliary.typed_snapshot_values decoded_snapshot)
      as [converted|] eqn:values; [|discriminate].
    inversion typed; subst.
    pose proof
      (Auxiliary.typed_snapshot_values_have_positional_references
        (EPM1.value_bodies snapshot) typed_values values) as positional.
    destruct (positional_pathmap_values_have_recursive_source_sequence
      par_child (EPM1.value_bodies snapshot) typed_values
      recursive_values positional related)
      as [recursive_values_source observed].
    assert (checked : checked_generated_raw bytes program
      (Source.SourceAuxiliary span field) = Some raw).
    { unfold checked_generated_raw.
      cbn [Source.source_span Source.source_field].
      rewrite descriptor, raw_source, valid.
      destruct (Auxiliary.descriptor_eq_dec field field)
        as [_|different]; [reflexivity|contradiction]. }
    destruct (auxiliary_semantic_cut
      (Auxiliary.PathMapResult raw snapshot trie typed_values))
      as [semantic|] eqn:scored.
    2: { unfold auxiliary_semantic_cut in scored.
         cbn in scored. discriminate. }
    assert (recursive_scored : recursive_auxiliary_semantic
      (RecursivePathMapResult raw snapshot trie recursive_values) =
        Some semantic).
    { rewrite <- (pathmap_semantic_cuts_agree_for_any_values
        raw snapshot trie typed_values recursive_values). exact scored. }
    destruct (exact_pathmap_cut_uses_original_snapshot_as_score_atom
      raw snapshot trie typed_values semantic scored) as [score complete].
    destruct (exact_auxiliary_cut_has_score_completion_and_original_body
      (Auxiliary.PathMapResult raw snapshot trie typed_values)
      semantic scored) as [_ [_ [_ [body _]]]].
    exists semantic. repeat split.
    + cbn [mixed_source_reference Source.source_field].
      now rewrite source_accepted, scored.
    + cbn [recursive_source_reference].
      unfold recursive_auxiliary_reference.
      rewrite checked, Nat.eqb_refl, payload, physical,
        recursive_values_source.
      cbn [map Source.sequence_source_results].
      change
        (option_map
          (RecursiveAuxiliaryResult
            (RecursivePathMapResult raw snapshot trie recursive_values))
          (recursive_auxiliary_semantic
            (RecursivePathMapResult raw snapshot trie recursive_values)) =
          Some (RecursiveAuxiliaryResult
            (RecursivePathMapResult raw snapshot trie recursive_values)
            semantic)).
      now rewrite recursive_scored.
    + exact observed.
    + exact score.
    + exact body.
    + exact complete.
Qed.

Lemma present_string_map_semantic_cuts_agree :
  forall raw key key_span value_span typed_value recursive_value value_score,
    semantic_par_score_of_body (Schema.summary_body typed_value) =
      Some value_score ->
    Semantic.sem_score_evidence recursive_value =
      Semantic.ExactScore value_score ->
    auxiliary_semantic_cut
      (Auxiliary.StringMapResult raw
        (Source.TypedMapReturn Schema.typed_summary key key_span
          (Some typed_value) (Some value_span))) =
    recursive_auxiliary_semantic
      (RecursiveStringMapResult raw
        (Source.TypedMapReturn Semantic.semantic_summary key key_span
          (Some recursive_value) (Some value_span))).
Proof.
  intros raw key key_span value_span typed_value recursive_value
    value_score typed_score recursive_score.
  unfold auxiliary_semantic_cut, recursive_auxiliary_semantic.
  simpl. now rewrite typed_score, recursive_score.
Qed.

Theorem present_string_map_source_independent_typed_recursive_bridge :
  forall par_child hash_fn bytes program slot span field raw typed_entry
      source_entry value_span body recursive_value value_score,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot
      (Source.SourceAuxiliary span field) =
      Some (Auxiliary.StringMapResult raw typed_entry) ->
    Source.refine_string_map_entry bytes program
      (Source.SourceAuxiliary span field) = Some source_entry ->
    Source.typed_map_value_source source_entry =
      Some (value_span, body) ->
    par_child body = Some (false, recursive_value) ->
    semantic_par_score_of_body body = Some value_score ->
    Semantic.sem_score_evidence recursive_value =
      Semantic.ExactScore value_score ->
    exists typed_value recursive_entry semantic,
      Reference.source_reference_par_of_body body =
        Some (false, typed_value) /\
      Schema.summary_body typed_value = body /\
      Source.reduce_source_map_node bytes program slot par_child
        (Source.SourceAuxiliary span field) = Some recursive_entry /\
      Source.returned_map_key Schema.typed_summary typed_entry =
        Source.returned_map_key Semantic.semantic_summary recursive_entry /\
      Source.returned_map_key_span Schema.typed_summary typed_entry =
        Source.returned_map_key_span Semantic.semantic_summary recursive_entry /\
      Source.returned_map_par_span Schema.typed_summary typed_entry =
        Source.returned_map_par_span Semantic.semantic_summary recursive_entry /\
      Source.returned_map_par Schema.typed_summary typed_entry =
        Some typed_value /\
      Source.returned_map_par Semantic.semantic_summary recursive_entry =
        Some recursive_value /\
      mixed_source_reference hash_fn bytes program
        (Source.SourceAuxiliary span field) =
        Some (MixedAuxiliaryResult
          (Auxiliary.StringMapResult raw typed_entry) semantic) /\
      recursive_source_reference par_child hash_fn bytes program
        (Source.SourceAuxiliary span field) =
        Some (RecursiveAuxiliaryResult
          (RecursiveStringMapResult raw recursive_entry) semantic) /\
      Semantic.sem_complete semantic = Some true /\
      Semantic.sem_body semantic = Schema.raw_body raw.
Proof.
  intros par_child hash_fn bytes program slot span field raw typed_entry
    source_entry value_span body recursive_value value_score
    accepted refined present recursive_child typed_score recursive_score.
  pose proof accepted as source_accepted.
  destruct (Auxiliary.accepted_string_map_has_typed_entry_source
    hash_fn bytes program slot (Source.SourceAuxiliary span field)
    raw typed_entry accepted)
    as [source_candidate [candidate_refined
      [selected_slot typed_reduced]]].
  rewrite refined in candidate_refined.
  inversion candidate_refined; subst source_candidate.
  destruct (Source.reduced_typed_map_return_matches_source
    Schema.typed_summary Reference.source_reference_par_of_body
    source_entry typed_entry typed_reduced) as [_ value_source].
  rewrite present in value_source.
  destruct value_source as [typed_value
    [typed_child [typed_return typed_span]]].
  destruct (Auxiliary.typed_snapshot_value_has_original_reference
    body false typed_value typed_child) as [_ [original _]].
  unfold Source.reduce_typed_map_entry in typed_reduced.
  destruct (Source.reduce_map_key
    (Source.typed_map_key_source source_entry))
    as [[key key_span]|] eqn:key_source; [|discriminate].
  unfold Source.reduce_map_par in typed_reduced.
  rewrite present, typed_child in typed_reduced.
  inversion typed_reduced; subst typed_entry.
  set (recursive_entry :=
    Source.TypedMapReturn Semantic.semantic_summary key key_span
      (Some recursive_value) (Some value_span)).
  assert (recursive_reduced :
    Source.reduce_source_map_node bytes program slot par_child
      (Source.SourceAuxiliary span field) = Some recursive_entry).
  { unfold Source.reduce_source_map_node.
    rewrite refined, selected_slot, Nat.eqb_refl.
    unfold Source.reduce_typed_map_entry.
    rewrite key_source.
    unfold Source.reduce_map_par.
    now rewrite present, recursive_child. }
  cbn [Auxiliary.typed_auxiliary_reference] in accepted.
  destruct (Source.source_descriptor program span) as [descriptor|]
    eqn:generated; [|discriminate].
  destruct (Auxiliary.descriptor_eq_dec field descriptor)
    as [same|different]; [subst descriptor|discriminate].
  destruct (Schema.descriptor_slot field =? slot) eqn:in_slot;
    [|discriminate].
  apply Nat.eqb_eq in in_slot. subst slot.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate|]
    eqn:raw_source; [|discriminate].
  destruct (Schema.valid_raw_body candidate) eqn:valid;
    [|discriminate].
  destruct (Schema.descriptor_payload field) eqn:payload;
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program
      (Schema.descriptor_slot field)
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)) as [returned|]
      eqn:typed_source; [|discriminate].
    inversion accepted; subst candidate returned.
    assert (checked : checked_generated_raw bytes program
      (Source.SourceAuxiliary span field) = Some raw).
    { unfold checked_generated_raw.
      cbn [Source.source_span Source.source_field].
      rewrite generated, raw_source, valid.
      destruct (Auxiliary.descriptor_eq_dec field field)
        as [_|different]; [reflexivity|contradiction]. }
    assert (semantic_agreement :
      auxiliary_semantic_cut
        (Auxiliary.StringMapResult raw
          (Source.TypedMapReturn Schema.typed_summary key key_span
            (Some typed_value) (Some value_span))) =
      recursive_auxiliary_semantic
        (RecursiveStringMapResult raw recursive_entry)).
    { unfold recursive_entry.
      apply (present_string_map_semantic_cuts_agree
        raw key key_span value_span typed_value recursive_value
        value_score).
      - now rewrite original.
      - exact recursive_score. }
    destruct (auxiliary_semantic_cut
      (Auxiliary.StringMapResult raw
        (Source.TypedMapReturn Schema.typed_summary key key_span
          (Some typed_value) (Some value_span))))
      as [semantic|] eqn:scored.
    2: { unfold auxiliary_semantic_cut in scored.
         simpl in scored. rewrite original, typed_score in scored.
         discriminate. }
    assert (recursive_scored : recursive_auxiliary_semantic
      (RecursiveStringMapResult raw recursive_entry) = Some semantic).
    { now rewrite <- semantic_agreement. }
    destruct (exact_auxiliary_cut_has_score_completion_and_original_body
      (Auxiliary.StringMapResult raw
        (Source.TypedMapReturn Schema.typed_summary key key_span
          (Some typed_value) (Some value_span))) semantic scored)
      as [_ [_ [complete [original_body _]]]].
    exists typed_value, recursive_entry, semantic.
    repeat split; try assumption; try reflexivity.
    + cbn [mixed_source_reference Source.source_field].
      now rewrite source_accepted, scored.
    + cbn [recursive_source_reference].
      unfold recursive_auxiliary_reference.
      rewrite checked, Nat.eqb_refl, payload, recursive_reduced.
      change
        (option_map
          (RecursiveAuxiliaryResult
            (RecursiveStringMapResult raw recursive_entry))
          (recursive_auxiliary_semantic
            (RecursiveStringMapResult raw recursive_entry)) =
          Some (RecursiveAuxiliaryResult
            (RecursiveStringMapResult raw recursive_entry) semantic)).
      now rewrite recursive_scored.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes (Source.SourceAuxiliary span field)))
      as [[snapshot trie]|]; [|discriminate].
    destruct (Auxiliary.typed_snapshot_values snapshot); discriminate.
Qed.

Theorem empty_pathmap_source_independent_typed_recursive_bridge :
  forall par_child hash_fn bytes program slot node raw snapshot trie,
    Auxiliary.typed_auxiliary_reference hash_fn bytes program slot node =
      Some (Auxiliary.PathMapResult raw snapshot trie []) ->
    exists semantic,
      mixed_source_reference hash_fn bytes program node =
        Some (MixedAuxiliaryResult
          (Auxiliary.PathMapResult raw snapshot trie []) semantic) /\
      recursive_source_reference par_child hash_fn bytes program node =
        Some (RecursiveAuxiliaryResult
          (RecursivePathMapResult raw snapshot trie []) semantic) /\
      pathmap_mixed_result_relation
        (MixedAuxiliaryResult
          (Auxiliary.PathMapResult raw snapshot trie []) semantic)
        (RecursiveAuxiliaryResult
          (RecursivePathMapResult raw snapshot trie []) semantic) /\
      Semantic.sem_score_evidence semantic =
        Semantic.ExactScore
          (Semantic.ScoreBytes (Schema.raw_body raw)) /\
      Semantic.sem_body semantic = Schema.raw_body raw /\
      Semantic.sem_complete semantic = Some true.
Proof.
  intros par_child hash_fn bytes program slot node raw snapshot trie
    typed.
  pose proof typed as source_accepted.
  destruct node as [span field|span field children|span field];
    cbn [Auxiliary.typed_auxiliary_reference] in typed;
    try discriminate.
  destruct (Source.source_descriptor program span) as [selected|]
    eqn:descriptor; [|discriminate].
  destruct (Auxiliary.descriptor_eq_dec field selected)
    as [same|different]; [subst selected|discriminate].
  destruct (Schema.descriptor_slot field =? slot) eqn:selected;
    [|discriminate].
  apply Nat.eqb_eq in selected. subst slot.
  destruct (Source.raw_field_of_source bytes
    (Source.SourceAuxiliary span field)) as [candidate|]
    eqn:raw_source; [|discriminate].
  destruct (Schema.valid_raw_body candidate) eqn:valid;
    [|discriminate].
  destruct (Schema.descriptor_payload field) eqn:payload;
    try discriminate.
  - destruct (Source.reduce_source_map_node bytes program
      (Schema.descriptor_slot field)
      Reference.source_reference_par_of_body
      (Source.SourceAuxiliary span field)); discriminate.
  - destruct (EPM1ACTree03.decode_epm1_actree03 hash_fn
      (Source.source_body bytes
        (Source.SourceAuxiliary span field)))
      as [[decoded_snapshot decoded_trie]|] eqn:physical;
      [|discriminate].
    destruct (Auxiliary.typed_snapshot_values decoded_snapshot)
      as [typed_values|] eqn:values; [|discriminate].
    inversion typed; subst.
    assert (empty_bodies : EPM1.value_bodies snapshot = []).
    { unfold Auxiliary.typed_snapshot_values in values.
      pose proof
        (Auxiliary.typed_snapshot_values_have_positional_references
          (EPM1.value_bodies snapshot) [] values) as positional.
      now inversion positional. }
    assert (checked : checked_generated_raw bytes program
      (Source.SourceAuxiliary span field) = Some raw).
    { unfold checked_generated_raw.
      cbn [Source.source_span Source.source_field].
      rewrite descriptor, raw_source, valid.
      destruct (Auxiliary.descriptor_eq_dec field field)
        as [_|different]; [reflexivity|contradiction]. }
    destruct (auxiliary_semantic_cut
      (Auxiliary.PathMapResult raw snapshot trie []))
      as [semantic|] eqn:scored.
    2: { unfold auxiliary_semantic_cut in scored.
         cbn in scored. discriminate. }
    assert (recursive_scored : recursive_auxiliary_semantic
      (RecursivePathMapResult raw snapshot trie []) = Some semantic).
    { rewrite <- (pathmap_semantic_cuts_agree_for_any_values
        raw snapshot trie [] []). exact scored. }
    destruct (exact_pathmap_cut_uses_original_snapshot_as_score_atom
      raw snapshot trie [] semantic scored) as [score complete].
    destruct (exact_auxiliary_cut_has_score_completion_and_original_body
      (Auxiliary.PathMapResult raw snapshot trie []) semantic scored)
      as [exact [_ [_ [body _]]]].
    exists semantic. repeat split.
    + cbn [mixed_source_reference Source.source_field].
      now rewrite source_accepted, scored.
    + cbn [recursive_source_reference].
      unfold recursive_auxiliary_reference.
      rewrite checked, Nat.eqb_refl, payload, physical, empty_bodies.
      cbn [map Source.sequence_source_results].
      change
        (option_map
          (RecursiveAuxiliaryResult
            (RecursivePathMapResult raw snapshot trie []))
          (recursive_auxiliary_semantic
            (RecursivePathMapResult raw snapshot trie [])) =
          Some (RecursiveAuxiliaryResult
            (RecursivePathMapResult raw snapshot trie []) semantic)).
      now rewrite recursive_scored.
    + constructor.
    + exact score.
    + exact body.
    + exact complete.
Qed.

Example empty_pathmap_generated_roots_have_related_results :
  forall hash_fn,
    exists typed recursive,
      mixed_root_reference hash_fn 31 Auxiliary.empty_pathmap_wire =
        Some [typed] /\
      recursive_mixed_root_reference hash_fn 31
        Auxiliary.empty_pathmap_wire = Some [recursive] /\
      pathmap_mixed_result_relation typed recursive.
Proof.
  intro hash_fn. vm_compute.
  eexists. eexists. repeat split; try reflexivity.
  constructor.
Qed.

Example malformed_pathmap_is_refused_by_both_mixed_roots :
  forall hash_fn,
    mixed_root_reference hash_fn 31 Auxiliary.invalid_actree03_wire =
      None /\
    recursive_mixed_root_reference hash_fn 31
      Auxiliary.invalid_actree03_wire = None.
Proof. intro hash_fn. vm_compute. now split. Qed.

Print Assumptions accepted_par_has_full_ordered_recursive_reference.
Print Assumptions full_par_reference_fixes_complete_score_body_and_child_order.
Print Assumptions full_par_reference_has_independent_semantic_lane_and_score.
Print Assumptions accepted_pathmap_values_have_full_ordered_recursive_references.
Print Assumptions accepted_string_map_value_has_full_ordered_recursive_reference.
Print Assumptions accepted_default_pathmap_value_has_full_recursive_reference.
Print Assumptions accepted_mixed_root_preserves_generated_source_order.
Print Assumptions accepted_mixed_message_has_ordered_typed_children_and_original_body.
Print Assumptions ordered_mixed_new_map_returnsb_iff_source_order.
Print Assumptions accepted_mixed_new_has_strict_typed_map_key_order.
Print Assumptions exact_auxiliary_cut_has_score_completion_and_original_body.
Print Assumptions exact_pathmap_cut_uses_original_snapshot_as_score_atom.
Print Assumptions exact_string_map_cut_uses_ordered_key_and_value_score.
Print Assumptions nested_new_string_map_has_mixed_typed_reference.
Print Assumptions nested_expr_pathmap_has_mixed_typed_reference.
Print Assumptions checked_generated_scalar_uses_generated_and_grammar_validated_raw.
Print Assumptions recursive_pathmap_reference_preserves_physical_decode_and_value_order.
Print Assumptions recursive_string_map_reference_reenters_guarded_par_value.
Print Assumptions recursive_string_map_score_preserves_key_and_value_score.
Print Assumptions accepted_recursive_new_preserves_source_and_strict_map_key_order.
Print Assumptions accepted_complete_recursive_new_score_retains_ordered_injections.
Print Assumptions recursive_reference_refuses_default_par_connective.
Print Assumptions recursive_reference_keeps_oneof_zero_bool.
Print Assumptions recursive_map_value_reenters_nested_pathmap.
Print Assumptions ordered_two_absent_value_maps_preserve_keys_and_complete_new_score.
Print Assumptions reversed_two_absent_value_maps_are_refused_by_parent_order.
Print Assumptions ordered_two_absent_value_maps_have_mixed_typed_reference.
Print Assumptions reversed_two_absent_value_maps_have_no_mixed_typed_reference.
Print Assumptions decoded_epm1_value_bodies_are_strictly_shorter_than_snapshot.
Print Assumptions refined_string_map_value_is_strictly_inside_parent_bytes.
Print Assumptions decoded_pathmap_values_satisfy_recursive_length_guard.
Print Assumptions refined_string_map_value_satisfies_recursive_length_guard.
Print Assumptions pathmap_semantic_cuts_agree_for_any_values.
Print Assumptions positional_pathmap_values_have_recursive_source_sequence.
Print Assumptions pathmap_source_independent_typed_recursive_bridge.
Print Assumptions present_string_map_semantic_cuts_agree.
Print Assumptions present_string_map_source_independent_typed_recursive_bridge.
Print Assumptions empty_pathmap_source_independent_typed_recursive_bridge.
Print Assumptions empty_pathmap_generated_roots_have_related_results.
Print Assumptions malformed_pathmap_is_refused_by_both_mixed_roots.
