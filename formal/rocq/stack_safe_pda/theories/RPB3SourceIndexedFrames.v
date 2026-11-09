From Stdlib Require Import List Bool Arith Lia NArith ZArith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer
  RPB3RecursiveAuxiliarySourceReference.

Import ListNotations.
Set Implicit Arguments.

Module SourceIndexedFrames.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Semantic := RPB3SemanticReducer.

Definition source_generated_root_field (type_id : nat)
    (bytes : list nat) : Schema.raw_field :=
  Schema.RawField 0 Schema.LengthDelimited Schema.Message
    bytes (length bytes) (Some type_id) [] [].

Inductive source_job :=
| Visit (program : Schema.generated_program) (node : Source.source_tree)
| VisitForest (program : Schema.generated_program)
    (nodes : list Source.source_tree)
| Close (raw : Schema.raw_field) (type_id arity : nat).

Definition close_message (raw : Schema.raw_field) (type_id : nat)
    (children : list Recursive.recursive_mixed_result)
    : option Recursive.recursive_mixed_result :=
  let map_ordered :=
    if type_id =? 20 then
      Recursive.ordered_recursive_new_map_returnsb None
        (Recursive.recursive_new_map_returns children)
    else true in
  if map_ordered then
    let semantic := Semantic.reduce_semantic_field raw
      (map Recursive.recursive_result_semantic children) in
    if Semantic.sem_definitely_refused semantic then None
    else Some (Recursive.RecursiveMessageResult raw semantic children)
  else None.

Fixpoint run_source_frames (steps : nat)
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (jobs : list source_job) (values : list Recursive.recursive_mixed_result)
    : option (list Recursive.recursive_mixed_result) :=
  match steps with
  | 0 => match jobs with [] => Some values | _ => None end
  | S remaining =>
      match jobs with
      | [] => Some values
      | Visit program (Source.SourceScalar span field) :: rest =>
          match Recursive.checked_generated_scalar_raw bytes program
              (Source.SourceScalar span field) with
          | None => None
          | Some raw =>
              let semantic := Semantic.reduce_semantic_field raw [] in
              if Semantic.sem_definitely_refused semantic then None
              else run_source_frames remaining par_child hash_fn bytes rest
                (Recursive.RecursiveScalarResult raw semantic :: values)
          end
      | Visit program (Source.SourceMessage span field children) :: rest =>
          match Recursive.checked_generated_raw bytes program
                  (Source.SourceMessage span field children),
                Schema.descriptor_child_type_id field with
          | Some raw, Some type_id =>
              match Schema.lookup_projected_program type_id [] [] with
              | None => None
              | Some child_program =>
                  run_source_frames remaining par_child hash_fn bytes
                    (VisitForest child_program children ::
                     Close raw type_id (length children) :: rest) values
              end
          | _, _ => None
          end
      | Visit program (Source.SourceAuxiliary span field) :: rest =>
          match Recursive.recursive_auxiliary_reference par_child hash_fn
              bytes program (Schema.descriptor_slot field)
              (Source.SourceAuxiliary span field) with
          | None => None
          | Some result =>
              run_source_frames remaining par_child hash_fn bytes rest
                (result :: values)
          end
      | VisitForest program [] :: rest =>
          run_source_frames remaining par_child hash_fn bytes rest values
      | VisitForest program (first :: siblings) :: rest =>
          run_source_frames remaining par_child hash_fn bytes
            (Visit program first :: VisitForest program siblings :: rest)
            values
      | Close raw type_id arity :: rest =>
          if arity <=? length values then
            let children := rev (firstn arity values) in
            match close_message raw type_id children with
            | Some result =>
                run_source_frames remaining par_child hash_fn bytes rest
                  (result :: skipn arity values)
            | None => None
            end
          else None
      end
  end.

Fixpoint source_visit_cost (node : Source.source_tree) : nat :=
  match node with
  | Source.SourceMessage _ _ children =>
      let fix children_cost (remaining : list Source.source_tree) : nat :=
        match remaining with
        | [] => 1
        | first :: siblings =>
            S (source_visit_cost first + children_cost siblings)
        end in
      S (children_cost children + 1)
  | _ => 1
  end.

Fixpoint source_forest_cost (nodes : list Source.source_tree) : nat :=
  match nodes with
  | [] => 1
  | first :: siblings =>
      S (source_visit_cost first + source_forest_cost siblings)
  end.

Lemma source_message_cost_matches_forest :
  forall span field children,
    source_visit_cost (Source.SourceMessage span field children) =
      S (source_forest_cost children + 1).
Proof.
  intros span field children.
  induction children as [|first siblings IH].
  - reflexivity.
  - cbn [source_visit_cost source_forest_cost] in *.
    lia.
Qed.

Lemma source_visit_cost_positive :
  forall node, 0 < source_visit_cost node.
Proof.
  intros [span field|span field children|span field];
    cbn [source_visit_cost]; lia.
Qed.

Lemma source_forest_cost_positive :
  forall nodes, 0 < source_forest_cost nodes.
Proof.
  intros [|first siblings]; cbn [source_forest_cost]; lia.
Qed.

Lemma close_after_exact_source_children :
  forall steps par_child hash_fn bytes raw type_id children rest values,
    run_source_frames (S steps) par_child hash_fn bytes
      (Close raw type_id (length children) :: rest)
      (rev children ++ values) =
    match close_message raw type_id children with
    | Some result =>
        run_source_frames steps par_child hash_fn bytes rest
          (result :: values)
    | None => None
    end.
Proof.
  intros steps par_child hash_fn bytes raw type_id children rest values.
  cbn [run_source_frames].
  assert (enough : length children <=? length (rev children ++ values) = true).
  { apply Nat.leb_le. rewrite length_app, length_rev. lia. }
  rewrite enough.
  assert (prefix :
      firstn (length children) (rev children ++ values) = rev children).
  { replace (length children) with (length (rev children))
      by now rewrite length_rev.
    rewrite firstn_app, firstn_all, Nat.sub_diag.
    now rewrite app_nil_r. }
  assert (suffix :
      skipn (length children) (rev children ++ values) = values).
  { replace (length children) with (length (rev children))
      by now rewrite length_rev.
    rewrite skipn_app, skipn_all, Nat.sub_diag.
    reflexivity. }
  rewrite prefix, suffix, rev_involutive.
  reflexivity.
Qed.

Theorem source_indexed_frames_refine_recursive_reference :
  forall budget,
    (forall par_child hash_fn bytes program node rest values steps,
      source_visit_cost node <= budget ->
      run_source_frames (source_visit_cost node + steps) par_child hash_fn
        bytes (Visit program node :: rest) values =
      match Recursive.recursive_source_reference par_child hash_fn bytes
          program node with
      | Some result =>
          run_source_frames steps par_child hash_fn bytes rest
            (result :: values)
      | None => None
      end) /\
    (forall par_child hash_fn bytes program nodes rest values steps,
      source_forest_cost nodes <= budget ->
      run_source_frames (source_forest_cost nodes + steps) par_child hash_fn
        bytes (VisitForest program nodes :: rest) values =
      match Recursive.recursive_source_children par_child hash_fn bytes
          program nodes with
      | Some results =>
          run_source_frames steps par_child hash_fn bytes rest
            (rev results ++ values)
      | None => None
      end).
Proof.
  induction budget as [|budget [IHvisit IHforest]].
  - split.
    + intros par_child hash_fn bytes program node rest values steps bound.
      pose proof (source_visit_cost_positive node). lia.
    + intros par_child hash_fn bytes program nodes rest values steps bound.
      pose proof (source_forest_cost_positive nodes). lia.
  - split.
    + intros par_child hash_fn bytes program node rest values steps bound.
      destruct node as [span field|span field children|span field].
      * cbn [source_visit_cost Nat.add run_source_frames
          Recursive.recursive_source_reference].
        destruct (Recursive.checked_generated_scalar_raw bytes program
          (Source.SourceScalar span field)) as [raw|]; [|reflexivity].
        destruct (Semantic.sem_definitely_refused
          (Semantic.reduce_semantic_field raw [])); reflexivity.
      * rewrite source_message_cost_matches_forest in bound |- *.
        cbn [Nat.add run_source_frames
          Recursive.recursive_source_reference].
        destruct (Recursive.checked_generated_raw bytes program
          (Source.SourceMessage span field children)) as [raw|];
          [|destruct (Schema.descriptor_child_type_id field);
             reflexivity].
        destruct (Schema.descriptor_child_type_id field)
          as [type_id|]; [|reflexivity].
        destruct (Schema.lookup_projected_program type_id [] [])
          as [child_program|]; [|reflexivity].
        rewrite Recursive.local_recursive_children_are_global_children.
        replace (source_forest_cost children + 1 + steps)
          with (source_forest_cost children + S steps) by lia.
        rewrite (IHforest par_child hash_fn bytes child_program children
          (Close raw type_id (length children) :: rest) values (S steps))
          by lia.
        destruct (Recursive.recursive_source_children par_child hash_fn
          bytes child_program children) as [converted|] eqn:child_results;
          [|reflexivity].
        pose proof (@Recursive.accepted_recursive_children_preserve_source_order
          par_child hash_fn bytes child_program children converted
          child_results) as aligned.
        apply Forall2_length in aligned.
        rewrite aligned.
        rewrite close_after_exact_source_children.
        unfold close_message.
        reflexivity.
      * cbn [source_visit_cost Nat.add run_source_frames
          Recursive.recursive_source_reference].
        destruct (Recursive.recursive_auxiliary_reference par_child hash_fn
          bytes program (Schema.descriptor_slot field)
          (Source.SourceAuxiliary span field)); reflexivity.
    + intros par_child hash_fn bytes program nodes rest values steps bound.
      destruct nodes as [|first siblings].
      * cbn [source_forest_cost Nat.add run_source_frames
          Recursive.recursive_source_children].
        reflexivity.
      * cbn [source_forest_cost Nat.add run_source_frames
          Recursive.recursive_source_children] in *.
        replace (source_visit_cost first + source_forest_cost siblings + steps)
          with (source_visit_cost first +
            (source_forest_cost siblings + steps)) by lia.
        rewrite (IHvisit par_child hash_fn bytes program first
          (VisitForest program siblings :: rest) values
          (source_forest_cost siblings + steps)) by lia.
        destruct (Recursive.recursive_source_reference par_child hash_fn
          bytes program first) as [head|] eqn:first_result;
          [|reflexivity].
        rewrite (IHforest par_child hash_fn bytes program siblings rest
          (head :: values) steps) by lia.
        destruct (Recursive.recursive_source_children par_child hash_fn
          bytes program siblings) as [tail|] eqn:tail_results;
          [|reflexivity].
        cbn [rev].
        now rewrite <- app_assoc.
Qed.

Corollary source_indexed_forest_has_exact_recursive_stack :
  forall par_child hash_fn bytes program nodes,
    run_source_frames (source_forest_cost nodes) par_child hash_fn bytes
      [VisitForest program nodes] [] =
    match Recursive.recursive_source_children par_child hash_fn bytes
        program nodes with
    | Some results => Some (rev results)
    | None => None
    end.
Proof.
  intros par_child hash_fn bytes program nodes.
  destruct (source_indexed_frames_refine_recursive_reference
    (source_forest_cost nodes)) as [_ forest].
  specialize (forest par_child hash_fn bytes program nodes [] [] 0
    (le_n (source_forest_cost nodes))).
  rewrite Nat.add_0_r in forest.
  cbn [run_source_frames] in forest.
  destruct (Recursive.recursive_source_children par_child hash_fn bytes
    program nodes) as [results|]; [|exact forest].
  now rewrite app_nil_r in forest.
Qed.

Definition source_indexed_root_full_result (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat)
    : option Recursive.recursive_mixed_result :=
  match Schema.lookup_projected_program type_id [] [],
      Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      match run_source_frames (source_forest_cost nodes + 1)
          (Recursive.recursive_par_reference hash_fn) hash_fn bytes
          [VisitForest program nodes;
           Close (source_generated_root_field
             type_id bytes) type_id (length nodes)] [] with
      | Some [root] => Some root
      | _ => None
      end
  | _, _ => None
  end.

Definition recursive_root_full_result (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat)
    : option Recursive.recursive_mixed_result :=
  match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
  | Some children =>
      close_message (source_generated_root_field type_id bytes)
        type_id children
  | None => None
  end.

Theorem source_indexed_root_full_result_refines_recursive_reference :
  forall hash_fn type_id bytes,
    source_indexed_root_full_result hash_fn type_id bytes =
      recursive_root_full_result hash_fn type_id bytes.
Proof.
  intros hash_fn type_id bytes.
  unfold source_indexed_root_full_result,
    recursive_root_full_result,
    Recursive.recursive_mixed_root_reference.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|]; [|destruct (Source.decode_root type_id bytes);
                   reflexivity].
  destruct (Source.decode_root type_id bytes)
    as [nodes|]; [|reflexivity].
  destruct (source_indexed_frames_refine_recursive_reference
    (source_forest_cost nodes)) as [_ forest].
  specialize (forest (Recursive.recursive_par_reference hash_fn) hash_fn
    bytes program nodes
    [Close (source_generated_root_field type_id bytes)
      type_id (length nodes)] [] 1
    (le_n (source_forest_cost nodes))).
  rewrite forest.
  destruct (Recursive.recursive_source_children
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program nodes)
    as [children|] eqn:child_results; [|reflexivity].
  pose proof (@Recursive.accepted_recursive_children_preserve_source_order
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program nodes
    children child_results) as aligned.
  apply Forall2_length in aligned.
  rewrite aligned.
  rewrite close_after_exact_source_children.
  destruct (close_message
    (source_generated_root_field type_id bytes)
    type_id children); reflexivity.
Qed.

Definition source_indexed_root_summary (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat)
    : option (list Semantic.semantic_summary) :=
  match Schema.lookup_projected_program type_id [] [],
      Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      match run_source_frames (source_forest_cost nodes + 1)
          (Recursive.recursive_par_reference hash_fn) hash_fn bytes
          [VisitForest program nodes;
           Close (source_generated_root_field
             type_id bytes) type_id (length nodes)] [] with
      | Some [root] => Some [Recursive.recursive_result_semantic root]
      | _ => None
      end
  | _, _ => None
  end.

Theorem source_indexed_root_has_universal_recursive_summary :
  forall hash_fn type_id bytes,
    source_indexed_root_summary hash_fn type_id bytes =
    match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
    | Some results =>
        match close_message
            (source_generated_root_field type_id bytes)
            type_id results with
        | Some root => Some [Recursive.recursive_result_semantic root]
        | None => None
        end
    | None => None
    end.
Proof.
  intros hash_fn type_id bytes.
  unfold source_indexed_root_summary,
    Recursive.recursive_mixed_root_reference.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|]; [|destruct (Source.decode_root type_id bytes);
                   reflexivity].
  destruct (Source.decode_root type_id bytes)
    as [nodes|]; [|reflexivity].
  destruct (source_indexed_frames_refine_recursive_reference
    (source_forest_cost nodes)) as [_ forest].
  specialize (forest (Recursive.recursive_par_reference hash_fn) hash_fn
    bytes program nodes
    [Close (source_generated_root_field type_id bytes)
      type_id (length nodes)] [] 1
    (le_n (source_forest_cost nodes))).
  rewrite forest.
  destruct (Recursive.recursive_source_children
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program nodes)
    as [results|] eqn:child_results; [|reflexivity].
  pose proof (@Recursive.accepted_recursive_children_preserve_source_order
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes program nodes
    results child_results) as aligned.
  apply Forall2_length in aligned.
  rewrite aligned.
  rewrite close_after_exact_source_children.
  destruct (close_message
    (source_generated_root_field type_id bytes)
    type_id results); reflexivity.
Qed.

Definition source_indexed_root_accepts (hash_fn : list nat -> nat)
    (type_id : nat) (bytes : list nat) : bool :=
  match source_indexed_root_summary hash_fn type_id bytes with
  | Some [root] => negb (Semantic.sem_definitely_refused root)
  | _ => false
  end.

Theorem source_indexed_root_has_universal_recursive_verdict :
  forall hash_fn type_id bytes,
    source_indexed_root_accepts hash_fn type_id bytes =
    match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
    | Some results =>
        match close_message
            (source_generated_root_field type_id bytes)
            type_id results with
        | Some root =>
            negb (Semantic.sem_definitely_refused
              (Recursive.recursive_result_semantic root))
        | None => false
        end
    | None => false
    end.
Proof.
  intros hash_fn type_id bytes.
  unfold source_indexed_root_accepts.
  rewrite source_indexed_root_has_universal_recursive_summary.
  destruct (Recursive.recursive_mixed_root_reference hash_fn type_id bytes)
    as [results|]; [|reflexivity].
  destruct (close_message
    (source_generated_root_field type_id bytes)
    type_id results); reflexivity.
Qed.

Inductive claimed_score_result :=
| ClaimedScoreAccepted
| ClaimedScoreRejected
| ClaimedScoreUnknown.

Fixpoint score_bytes_equal (left right : list nat) : bool :=
  match left, right with
  | [], [] => true
  | left_byte :: left_rest, right_byte :: right_rest =>
      (left_byte =? right_byte) &&
      score_bytes_equal left_rest right_rest
  | _, _ => false
  end.

Definition compare_claimed_score (semantic : Semantic.semantic_summary)
    (claimed_score : list nat) : claimed_score_result :=
  if Semantic.sem_definitely_refused semantic then ClaimedScoreRejected
  else match Semantic.sem_score_evidence semantic with
  | Semantic.ExactScore score =>
      if score_bytes_equal (Semantic.canonical_score_bytes score) claimed_score
      then ClaimedScoreAccepted else ClaimedScoreRejected
  | Semantic.ScoreNotDerived => ClaimedScoreUnknown
  end.

Definition source_indexed_root_claimed_score (hash_fn : list nat -> nat)
    (type_id : nat) (bytes claimed_score : list nat)
    : claimed_score_result :=
  match source_indexed_root_summary hash_fn type_id bytes with
  | Some [root] => compare_claimed_score root claimed_score
  | _ => ClaimedScoreRejected
  end.

Definition recursive_root_claimed_score (hash_fn : list nat -> nat)
    (type_id : nat) (bytes claimed_score : list nat)
    : claimed_score_result :=
  match Recursive.recursive_mixed_root_reference hash_fn type_id bytes with
  | Some results =>
      match close_message
          (source_generated_root_field type_id bytes)
          type_id results with
      | Some root =>
          compare_claimed_score
            (Recursive.recursive_result_semantic root) claimed_score
      | None => ClaimedScoreRejected
      end
  | None => ClaimedScoreRejected
  end.

Theorem source_indexed_root_claimed_score_refines_recursive_reference :
  forall hash_fn type_id bytes claimed_score,
    source_indexed_root_claimed_score hash_fn type_id bytes claimed_score =
      recursive_root_claimed_score hash_fn type_id bytes claimed_score.
Proof.
  intros hash_fn type_id bytes claimed_score.
  unfold source_indexed_root_claimed_score,
    recursive_root_claimed_score.
  rewrite source_indexed_root_has_universal_recursive_summary.
  destruct (Recursive.recursive_mixed_root_reference hash_fn type_id bytes)
    as [results|]; [|reflexivity].
  destruct (close_message
    (source_generated_root_field type_id bytes)
    type_id results); reflexivity.
Qed.

Print Assumptions source_indexed_frames_refine_recursive_reference.
Print Assumptions source_indexed_root_full_result_refines_recursive_reference.
Print Assumptions source_indexed_root_has_universal_recursive_summary.
Print Assumptions source_indexed_root_has_universal_recursive_verdict.
Print Assumptions source_indexed_root_claimed_score_refines_recursive_reference.

End SourceIndexedFrames.
