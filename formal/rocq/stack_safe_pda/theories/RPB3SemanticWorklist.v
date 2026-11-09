From Stdlib Require Import List Arith Lia NArith.
From StackSafePDA Require Import StackSafePDA RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Semantic := RPB3SemanticReducer.

Inductive semantic_job :=
| Visit (subject : @tree Schema.raw_field)
| VisitForest (subjects : @forest Schema.raw_field)
| Close (field : Schema.raw_field) (arity : nat).

Fixpoint visit_cost (subject : @tree Schema.raw_field) : nat :=
  match subject with Node _ children => S (forest_cost children + 1) end
with forest_cost (subjects : @forest Schema.raw_field) : nat :=
  match subjects with
  | FNil => 1
  | FCons subject rest => S (visit_cost subject + forest_cost rest)
  end.

Definition job_cost (job : semantic_job) : nat :=
  match job with
  | Visit subject => visit_cost subject
  | VisitForest subjects => forest_cost subjects
  | Close _ _ => 1
  end.

Fixpoint remaining_cost (jobs : list semantic_job) : nat :=
  match jobs with
  | [] => 0
  | job :: rest => job_cost job + remaining_cost rest
  end.

Lemma every_semantic_job_has_positive_cost :
  forall job, 0 < job_cost job.
Proof.
  intros [subject|subjects|field arity].
  - destruct subject as [field children]. simpl. lia.
  - destruct subjects as [|first rest]; simpl; lia.
  - simpl. lia.
Qed.

Theorem pending_semantic_frames_fit_the_structural_budget :
  forall jobs, length jobs <= remaining_cost jobs.
Proof.
  induction jobs as [|job rest IH]; simpl; [lia|].
  pose proof (every_semantic_job_has_positive_cost job). lia.
Qed.

Fixpoint run_semantic_frames (steps : nat) (jobs : list semantic_job)
    (values : list Semantic.semantic_summary)
    : option (list Semantic.semantic_summary) :=
  match steps with
  | 0 => match jobs with [] => Some values | _ => None end
  | S remaining =>
      match jobs with
      | [] => Some values
      | Visit (Node field children) :: rest =>
          run_semantic_frames remaining
            (VisitForest children :: Close field (forest_arity children) :: rest)
            values
      | VisitForest FNil :: rest =>
          run_semantic_frames remaining rest values
      | VisitForest (FCons child siblings) :: rest =>
          run_semantic_frames remaining
            (Visit child :: VisitForest siblings :: rest) values
      | Close field arity :: rest =>
          if arity <=? length values then
            run_semantic_frames remaining rest
              (Semantic.reduce_semantic_field field
                (rev (firstn arity values)) :: skipn arity values)
          else None
      end
  end.

Definition semantic_worklist (subject : @tree Schema.raw_field)
    : option (list Semantic.semantic_summary) :=
  run_semantic_frames (visit_cost subject) [Visit subject] [].

Definition job_program (job : semantic_job)
    : list (@instruction Schema.raw_field) :=
  match job with
  | Visit subject => compile_tree subject
  | VisitForest subjects => compile_forest subjects
  | Close field arity => [Reduce field arity]
  end.

Fixpoint pending_program (jobs : list semantic_job)
    : list (@instruction Schema.raw_field) :=
  match jobs with
  | [] => []
  | job :: rest => job_program job ++ pending_program rest
  end.

Lemma visit_cost_decreases field children rest :
  S (remaining_cost (VisitForest children ::
      Close field (forest_arity children) :: rest)) =
    remaining_cost (Visit (Node field children) :: rest).
Proof. simpl. lia. Qed.

Lemma forest_cons_cost_decreases child siblings rest :
  S (remaining_cost (Visit child :: VisitForest siblings :: rest)) =
    remaining_cost (VisitForest (FCons child siblings) :: rest).
Proof. simpl. lia. Qed.

Lemma forest_nil_cost_decreases rest :
  S (remaining_cost rest) = remaining_cost (VisitForest FNil :: rest).
Proof. reflexivity. Qed.

Lemma close_cost_decreases field arity rest :
  S (remaining_cost rest) = remaining_cost (Close field arity :: rest).
Proof. reflexivity. Qed.

Theorem semantic_frames_refine_postorder steps :
  forall jobs values, remaining_cost jobs <= steps ->
    run_semantic_frames steps jobs values =
      @run Schema.raw_field Semantic.semantic_summary
        Semantic.reduce_semantic_field (pending_program jobs) values.
Proof.
  induction steps as [|steps IH]; intros jobs values bound.
  - destruct jobs as [|job rest]; [reflexivity|].
    destruct job as [subject|subjects|field arity].
    + destruct subject as [field children]. simpl in bound. lia.
    + destruct subjects as [|first siblings]; simpl in bound; lia.
    + simpl in bound. lia.
  - destruct jobs as [|job rest]; [reflexivity|].
    destruct job as [subject|subjects|field arity].
    + destruct subject as [field children].
      simpl [run_semantic_frames].
      rewrite (IH (VisitForest children ::
          Close field (forest_arity children) :: rest) values)
        by (pose proof (visit_cost_decreases field children rest); lia).
      simpl [pending_program job_program compile_tree].
      now rewrite <- app_assoc.
    + destruct subjects as [|first siblings].
      * simpl [run_semantic_frames].
        rewrite (IH rest values)
          by (pose proof (forest_nil_cost_decreases rest); lia).
        reflexivity.
      * simpl [run_semantic_frames].
        rewrite (IH (Visit first :: VisitForest siblings :: rest) values)
          by (pose proof (forest_cons_cost_decreases first siblings rest); lia).
        simpl [pending_program job_program compile_forest].
        now rewrite <- app_assoc.
    + simpl [run_semantic_frames pending_program job_program run].
      destruct (arity <=? length values) eqn:enough; [|reflexivity].
      apply IH. pose proof (close_cost_decreases field arity rest). lia.
Qed.

Theorem close_frame_restores_exact_source_child_order :
  forall field children tail rest steps,
    run_semantic_frames (S steps)
      (Close field (length children) :: rest) (rev children ++ tail) =
    run_semantic_frames steps rest
      (Semantic.reduce_semantic_field field children :: tail).
Proof.
  intros field children tail rest steps.
  cbn [run_semantic_frames].
  set (completed := rev children).
  replace (length children) with (length completed)
    by (unfold completed; now rewrite length_rev).
  assert (enough : (length completed <=? length (completed ++ tail)) = true).
  { apply Nat.leb_le. rewrite length_app. lia. }
  rewrite enough, firstn_length_app, skipn_length_app.
  unfold completed. now rewrite rev_involutive.
Qed.

Lemma semantic_reference_is_fold :
  (forall subject, Semantic.semantic_reduce subject =
    @fold_tree Schema.raw_field Semantic.semantic_summary
      Semantic.reduce_semantic_field subject) /\
  (forall subjects, Semantic.semantic_reduce_forest subjects =
    @fold_forest Schema.raw_field Semantic.semantic_summary
      Semantic.reduce_semantic_field subjects).
Proof.
  apply tree_forest_induction; intros; simpl; congruence.
Qed.

Theorem semantic_worklist_equals_recursive_summary :
  forall subject,
    semantic_worklist subject = Some [Semantic.semantic_reduce subject].
Proof.
  intro subject. unfold semantic_worklist.
  rewrite semantic_frames_refine_postorder by (simpl; lia).
  simpl [pending_program job_program].
  rewrite app_nil_r.
  rewrite (proj1 semantic_reference_is_fold).
  apply pda_fold_equivalent_to_recursive_fold.
Qed.

Theorem semantic_worklist_terminates_with_exact_summary :
  forall subject, exists summary,
    run_semantic_frames (visit_cost subject) [Visit subject] [] =
      Some [summary] /\ summary = Semantic.semantic_reduce subject.
Proof.
  intro subject. exists (Semantic.semantic_reduce subject).
  split; [apply semantic_worklist_equals_recursive_summary|reflexivity].
Qed.

Inductive semantic_result :=
| DefinitelyRefused
| ScoreNotDerivedSummary (summary : Semantic.semantic_summary)
| ExactScoreSummary (summary : Semantic.semantic_summary).

Definition classify_semantic (summary : Semantic.semantic_summary)
    : semantic_result :=
  if Semantic.sem_definitely_refused summary then DefinitelyRefused
  else match Semantic.sem_score_evidence summary with
       | Semantic.ExactScore _ => ExactScoreSummary summary
       | Semantic.ScoreNotDerived => ScoreNotDerivedSummary summary
       end.

Definition recursive_semantic_result (subject : @tree Schema.raw_field)
    : semantic_result :=
  classify_semantic (Semantic.semantic_reduce subject).

Definition worklist_semantic_result (subject : @tree Schema.raw_field)
    : semantic_result :=
  match semantic_worklist subject with
  | Some [summary] => classify_semantic summary
  | _ => DefinitelyRefused
  end.

Theorem semantic_worklist_preserves_full_summary_and_score_state :
  forall subject,
    worklist_semantic_result subject = recursive_semantic_result subject.
Proof.
  intro subject. unfold worklist_semantic_result, recursive_semantic_result.
  rewrite semantic_worklist_equals_recursive_summary. reflexivity.
Qed.

Theorem semantic_worklist_is_deterministic :
  forall subject left right,
    semantic_worklist subject = Some [left] ->
    semantic_worklist subject = Some [right] -> left = right.
Proof. intros subject left right left_result right_result. congruence. Qed.

Definition source_semantic_worklist (type_id : nat) (bytes : list nat)
    : option (list Semantic.semantic_summary) :=
  match Source.raw_root_of_bytes type_id bytes with
  | Some root => semantic_worklist root
  | None => None
  end.

Definition source_recursive_summary (type_id : nat) (bytes : list nat)
    : option Semantic.semantic_summary :=
  option_map Semantic.semantic_reduce
    (Source.raw_root_of_bytes type_id bytes).

Theorem source_worklist_equals_recursive_summary :
  forall type_id bytes,
    source_semantic_worklist type_id bytes =
      option_map (fun summary => [summary])
        (source_recursive_summary type_id bytes).
Proof.
  intros type_id bytes.
  unfold source_semantic_worklist, source_recursive_summary.
  destruct (Source.raw_root_of_bytes type_id bytes) as [root|];
    [apply semantic_worklist_equals_recursive_summary|reflexivity].
Qed.

Theorem source_worklist_retains_the_original_root_body :
  forall type_id bytes summary,
    source_semantic_worklist type_id bytes = Some [summary] ->
    Semantic.sem_body summary = bytes.
Proof.
  intros type_id bytes summary returned.
  unfold source_semantic_worklist in returned.
  destruct (Source.raw_root_of_bytes type_id bytes) as [root|]
    eqn:converted; [|discriminate].
  rewrite semantic_worklist_equals_recursive_summary in returned.
  inversion returned; subst summary.
  destruct (Source.raw_root_of_bytes_preserves_original_body
    type_id bytes root converted) as [children identity].
  subst root. reflexivity.
Qed.

Example incomplete_send_remains_a_summary :
  match semantic_worklist Semantic.empty_incomplete_send with
  | Some [summary] =>
      Semantic.sem_complete summary = Some false /\
      Semantic.sem_definitely_refused summary = false
  | _ => False
  end.
Proof. vm_compute. split; reflexivity. Qed.

Example reversed_incomplete_send_is_refused :
  worklist_semantic_result Semantic.reversed_incomplete_sends =
    DefinitelyRefused.
Proof. vm_compute. reflexivity. Qed.

Example complete_bundles_retain_distinct_exact_scores :
  match semantic_worklist Semantic.read_only_bundle,
      semantic_worklist Semantic.write_only_bundle with
  | Some [read], Some [write] =>
      Semantic.sem_score_evidence read =
        Semantic.ExactScore (Semantic.score_constructor 306%N
          [Semantic.score_constructor 999%N [Semantic.ScoreInteger 0%N]]) /\
      Semantic.sem_score_evidence write =
        Semantic.ExactScore (Semantic.score_constructor 307%N
          [Semantic.score_constructor 999%N [Semantic.ScoreInteger 0%N]])
  | _, _ => False
  end.
Proof.
  repeat rewrite semantic_worklist_equals_recursive_summary.
  destruct Semantic.complete_bundles_have_distinct_exact_scores
    as [_ [_ [read write]]].
  now split.
Qed.

Example nested_send_source_worklist_retains_original_body :
  match source_semantic_worklist 1 [10; 2; 24; 1] with
  | Some [summary] =>
      Semantic.sem_body summary = [10; 2; 24; 1] /\
      Semantic.sem_type_id summary = 1
  | _ => False
  end.
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions pending_semantic_frames_fit_the_structural_budget.
Print Assumptions semantic_frames_refine_postorder.
Print Assumptions close_frame_restores_exact_source_child_order.
Print Assumptions semantic_worklist_equals_recursive_summary.
Print Assumptions semantic_worklist_terminates_with_exact_summary.
Print Assumptions semantic_worklist_preserves_full_summary_and_score_state.
Print Assumptions semantic_worklist_is_deterministic.
Print Assumptions source_worklist_equals_recursive_summary.
Print Assumptions source_worklist_retains_the_original_root_body.
Print Assumptions incomplete_send_remains_a_summary.
Print Assumptions reversed_incomplete_send_is_refused.
Print Assumptions complete_bundles_retain_distinct_exact_scores.
Print Assumptions nested_send_source_worklist_retains_original_body.
