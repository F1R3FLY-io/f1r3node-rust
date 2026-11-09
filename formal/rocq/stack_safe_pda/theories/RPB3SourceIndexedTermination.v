From Stdlib Require Import List Arith Lia Wf_nat.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3RawRegionTree RPB3SemanticReducer
  RPB3RecursiveAuxiliarySourceReference RPB3SourceIndexedFrames.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Semantic := RPB3SemanticReducer.
Module Recursive := RPB3RecursiveAuxiliarySourceReference.
Module Frames := RPB3SourceIndexedFrames.SourceIndexedFrames.

Inductive configuration :=
| Active (jobs : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result)
| Refused.

Definition progress
    (par_child : list nat -> option (bool * Semantic.semantic_summary))
    (hash_fn : list nat -> nat) (bytes : list nat)
    (job : Frames.source_job) (rest : list Frames.source_job)
    (values : list Recursive.recursive_mixed_result) : configuration :=
  match job with
  | Frames.Visit program (Source.SourceScalar span field) =>
      match Recursive.checked_generated_scalar_raw bytes program
          (Source.SourceScalar span field) with
      | None => Refused
      | Some raw =>
          let semantic := Semantic.reduce_semantic_field raw [] in
          if Semantic.sem_definitely_refused semantic then Refused
          else Active rest (Recursive.RecursiveScalarResult raw semantic :: values)
      end
  | Frames.Visit program (Source.SourceMessage span field children) =>
      match Recursive.checked_generated_raw bytes program
              (Source.SourceMessage span field children),
            Schema.descriptor_child_type_id field with
      | Some raw, Some type_id =>
          match Schema.lookup_projected_program type_id [] [] with
          | None => Refused
          | Some child_program =>
              Active
                (Frames.VisitForest child_program children ::
                 Frames.Close raw type_id (length children) :: rest) values
          end
      | _, _ => Refused
      end
  | Frames.Visit program (Source.SourceAuxiliary span field) =>
      match Recursive.recursive_auxiliary_reference par_child hash_fn
          bytes program (Schema.descriptor_slot field)
          (Source.SourceAuxiliary span field) with
      | None => Refused
      | Some result => Active rest (result :: values)
      end
  | Frames.VisitForest program [] => Active rest values
  | Frames.VisitForest program (first :: siblings) =>
      Active
        (Frames.Visit program first ::
         Frames.VisitForest program siblings :: rest) values
  | Frames.Close raw type_id arity =>
      if arity <=? length values then
        let children := rev (firstn arity values) in
        match Frames.close_message raw type_id children with
        | Some result => Active rest (result :: skipn arity values)
        | None => Refused
        end
      else Refused
  end.

Definition advance par_child hash_fn bytes (state : configuration)
    : option configuration :=
  match state with
  | Active (job :: rest) values =>
      Some (progress par_child hash_fn bytes job rest values)
  | Active [] _ | Refused => None
  end.

Inductive reaches par_child hash_fn bytes : configuration -> configuration -> Prop :=
| reaches_refl : forall state, reaches par_child hash_fn bytes state state
| reaches_step : forall before after final,
    advance par_child hash_fn bytes before = Some after ->
    reaches par_child hash_fn bytes after final ->
    reaches par_child hash_fn bytes before final.

Definition terminal (state : configuration) : Prop :=
  match state with
  | Active [] _ | Refused => True
  | Active (_ :: _) _ => False
  end.

Definition job_cost (job : Frames.source_job) : nat :=
  match job with
  | Frames.Visit _ node => Frames.source_visit_cost node
  | Frames.VisitForest _ nodes => Frames.source_forest_cost nodes
  | Frames.Close _ _ _ => 1
  end.

Fixpoint pending_cost (jobs : list Frames.source_job) : nat :=
  match jobs with
  | [] => 0
  | job :: rest => job_cost job + pending_cost rest
  end.

Definition potential (state : configuration) : nat :=
  match state with
  | Active jobs _ => pending_cost jobs
  | Refused => 0
  end.

Definition observation (state : configuration)
    : option (list Recursive.recursive_mixed_result) :=
  match state with
  | Active [] values => Some values
  | _ => None
  end.

Definition budgeted_observation par_child hash_fn bytes
    (state : configuration) (steps : nat)
    : option (list Recursive.recursive_mixed_result) :=
  match state with
  | Active jobs values =>
      Frames.run_source_frames steps par_child hash_fn bytes jobs values
  | Refused => None
  end.

Lemma job_cost_positive : forall job, 0 < job_cost job.
Proof.
  intros [program node|program nodes|raw type_id arity];
    cbn [job_cost].
  - apply Frames.source_visit_cost_positive.
  - apply Frames.source_forest_cost_positive.
  - lia.
Qed.

Lemma progress_decreases : forall par_child hash_fn bytes job rest values,
  potential (progress par_child hash_fn bytes job rest values) <
  potential (Active (job :: rest) values).
Proof.
  intros par_child hash_fn bytes job rest values.
  pose proof (job_cost_positive job) as positive.
  destruct job as [program node|program nodes|raw type_id arity].
  - destruct node as [span field|span field children|span field].
    + cbn [progress potential pending_cost job_cost] in *.
      destruct (Recursive.checked_generated_scalar_raw bytes program
        (Source.SourceScalar span field)) as [converted|];
        [destruct (Semantic.sem_definitely_refused
          (Semantic.reduce_semantic_field converted []))|];
        cbn [potential pending_cost] in *; lia.
    + pose proof (Frames.source_message_cost_matches_forest
        span field children) as message_cost.
      cbn [progress potential pending_cost job_cost] in *.
      destruct (Recursive.checked_generated_raw bytes program
        (Source.SourceMessage span field children)) as [converted|];
        destruct (Schema.descriptor_child_type_id field) as [type_id|];
        try (cbn [potential pending_cost] in *; lia).
      destruct (Schema.lookup_projected_program type_id [] [])
        as [child_program|]; cbn [potential pending_cost job_cost] in *;
        lia.
    + cbn [progress potential pending_cost job_cost] in *.
      destruct (Recursive.recursive_auxiliary_reference par_child hash_fn
        bytes program (Schema.descriptor_slot field)
        (Source.SourceAuxiliary span field));
        cbn [potential pending_cost] in *; lia.
  - destruct nodes as [|first siblings].
    + cbn [progress potential pending_cost job_cost
        Frames.source_forest_cost] in *; lia.
    + cbn [progress potential pending_cost job_cost
        Frames.source_forest_cost] in *; lia.
  - cbn [progress potential pending_cost job_cost] in *.
    destruct (arity <=? length values) eqn:enough.
    + destruct (Frames.close_message raw type_id
        (rev (firstn arity values))) as [result|];
        cbn [potential pending_cost] in *; lia.
    + cbn [potential pending_cost] in *; lia.
Qed.

Lemma advance_decreases : forall par_child hash_fn bytes before after,
  advance par_child hash_fn bytes before = Some after ->
  potential after < potential before.
Proof.
  intros par_child hash_fn bytes [jobs values|] after step;
    [destruct jobs as [|job rest]|]; try discriminate.
  cbn [advance] in step.
  inversion step; subst after.
  apply progress_decreases.
Qed.

Lemma progress_matches_budgeted_step :
  forall par_child hash_fn bytes job rest values steps,
    Frames.run_source_frames (S steps) par_child hash_fn bytes
      (job :: rest) values =
    budgeted_observation par_child hash_fn bytes
      (progress par_child hash_fn bytes job rest values) steps.
Proof.
  intros par_child hash_fn bytes job rest values steps.
  destruct job as [program node|program nodes|raw type_id arity].
  - destruct node as [span field|span field children|span field].
    + cbn [Frames.run_source_frames progress budgeted_observation].
      destruct (Recursive.checked_generated_scalar_raw bytes program
        (Source.SourceScalar span field)) as [converted|]; [|reflexivity].
      destruct (Semantic.sem_definitely_refused
        (Semantic.reduce_semantic_field converted [])); reflexivity.
    + cbn [Frames.run_source_frames progress budgeted_observation].
      destruct (Recursive.checked_generated_raw bytes program
        (Source.SourceMessage span field children)) as [converted|];
        destruct (Schema.descriptor_child_type_id field) as [type_id|];
        try reflexivity.
      destruct (Schema.lookup_projected_program type_id [] [])
        as [child_program|]; reflexivity.
    + cbn [Frames.run_source_frames progress budgeted_observation].
      destruct (Recursive.recursive_auxiliary_reference par_child hash_fn
        bytes program (Schema.descriptor_slot field)
        (Source.SourceAuxiliary span field)); reflexivity.
  - destruct nodes as [|first siblings];
      cbn [Frames.run_source_frames progress budgeted_observation];
      reflexivity.
  - cbn [Frames.run_source_frames progress budgeted_observation].
    destruct (arity <=? length values) eqn:enough; [|reflexivity].
    destruct (Frames.close_message raw type_id
      (rev (firstn arity values))); reflexivity.
Qed.

Lemma advance_matches_budgeted_step :
  forall par_child hash_fn bytes before after steps,
    advance par_child hash_fn bytes before = Some after ->
    budgeted_observation par_child hash_fn bytes before (S steps) =
    budgeted_observation par_child hash_fn bytes after steps.
Proof.
  intros par_child hash_fn bytes [jobs values|] after steps step;
    [destruct jobs as [|job rest]|]; try discriminate.
  cbn [advance] in step.
  inversion step; subst after.
  cbn [budgeted_observation].
  apply progress_matches_budgeted_step.
Qed.

Lemma terminal_budgeted_observation :
  forall par_child hash_fn bytes state steps,
    terminal state ->
    budgeted_observation par_child hash_fn bytes state steps =
    observation state.
Proof.
  intros par_child hash_fn bytes [jobs values|] steps done;
    [destruct jobs as [|job rest]|]; try contradiction.
  - destruct steps; reflexivity.
  - reflexivity.
Qed.

Theorem every_source_indexed_configuration_terminates :
  forall par_child hash_fn bytes state,
    exists final,
      reaches par_child hash_fn bytes state final /\ terminal final.
Proof.
  intros par_child hash_fn bytes.
  apply (well_founded_induction
    (well_founded_ltof configuration potential)
    (fun state => exists final,
      reaches par_child hash_fn bytes state final /\ terminal final)).
  intros state IH.
  destruct state as [jobs values|].
  - destruct jobs as [|job rest].
    + exists (Active [] values). split; [constructor|exact I].
    + pose (next := progress par_child hash_fn bytes job rest values).
      destruct (IH next) as [final [path done]].
      { unfold next. apply progress_decreases. }
      exists final. split; [|exact done].
      eapply reaches_step; [reflexivity|exact path].
  - exists Refused. split; [constructor|exact I].
Qed.

Theorem source_indexed_reach_matches_every_sufficient_budget :
  forall par_child hash_fn bytes before final,
    reaches par_child hash_fn bytes before final ->
    terminal final ->
    forall steps, potential before <= steps ->
      budgeted_observation par_child hash_fn bytes before steps =
      observation final.
Proof.
  intros par_child hash_fn bytes before final path.
  induction path as [state|before after final step path IH];
    intros done steps enough.
  - apply terminal_budgeted_observation. exact done.
  - pose proof (advance_decreases _ _ _ _ _ step) as smaller.
    destruct steps as [|remaining]; [lia|].
    rewrite (advance_matches_budgeted_step _ _ _ _ _ _ step).
    apply IH; [exact done|lia].
Qed.

Lemma terminal_observation_injective : forall left right,
  terminal left -> terminal right ->
  observation left = observation right -> left = right.
Proof.
  intros left right left_done right_done same.
  destruct left as [left_jobs left_values|],
    right as [right_jobs right_values|];
    cbn [terminal observation] in *; try contradiction;
    try congruence.
  - destruct left_jobs as [|left_job left_rest],
      right_jobs as [|right_job right_rest];
      cbn [terminal observation] in *; try contradiction;
      try discriminate; congruence.
  - destruct left_jobs as [|left_job left_rest];
      cbn [terminal observation] in *; try contradiction;
      discriminate.
  - destruct right_jobs as [|right_job right_rest];
      cbn [terminal observation] in *; try contradiction;
      discriminate.
Qed.

Theorem source_indexed_terminal_is_unique :
  forall par_child hash_fn bytes start left right,
    reaches par_child hash_fn bytes start left -> terminal left ->
    reaches par_child hash_fn bytes start right -> terminal right ->
    left = right.
Proof.
  intros par_child hash_fn bytes start left right
    left_path left_done right_path right_done.
  apply terminal_observation_injective; try assumption.
  transitivity (budgeted_observation par_child hash_fn bytes start
    (potential start)).
  - symmetry. eapply source_indexed_reach_matches_every_sufficient_budget;
      [exact left_path|exact left_done|lia].
  - eapply source_indexed_reach_matches_every_sufficient_budget;
      [exact right_path|exact right_done|lia].
Qed.

Definition root_configuration type_id bytes : configuration :=
  match Schema.lookup_projected_program type_id [] [],
      Source.decode_root type_id bytes with
  | Some program, Some nodes =>
      Active
        [Frames.VisitForest program nodes;
         Frames.Close (Frames.source_generated_root_field type_id bytes)
           type_id (length nodes)] []
  | _, _ => Refused
  end.

Definition only_root
    (result : option (list Recursive.recursive_mixed_result))
    : option Recursive.recursive_mixed_result :=
  match result with
  | Some [root] => Some root
  | _ => None
  end.

Definition terminal_root_result (state : configuration)
    : option Recursive.recursive_mixed_result :=
  only_root (observation state).

Lemma root_budget_matches_existing_result : forall hash_fn type_id bytes,
  only_root
    (budgeted_observation (Recursive.recursive_par_reference hash_fn)
      hash_fn bytes (root_configuration type_id bytes)
      (potential (root_configuration type_id bytes))) =
  Frames.source_indexed_root_full_result hash_fn type_id bytes.
Proof.
  intros hash_fn type_id bytes.
  unfold root_configuration, Frames.source_indexed_root_full_result.
  destruct (Schema.lookup_projected_program type_id [] [])
    as [program|];
    destruct (Source.decode_root type_id bytes) as [nodes|];
    cbn [potential pending_cost job_cost
      budgeted_observation only_root]; reflexivity.
Qed.

Theorem terminal_root_result_for_any_reach :
  forall hash_fn type_id bytes final,
    reaches (Recursive.recursive_par_reference hash_fn) hash_fn bytes
      (root_configuration type_id bytes) final ->
    terminal final ->
    terminal_root_result final =
      Frames.source_indexed_root_full_result hash_fn type_id bytes.
Proof.
  intros hash_fn type_id bytes final path done.
  unfold terminal_root_result.
  rewrite <- root_budget_matches_existing_result.
  f_equal.
  symmetry.
  eapply source_indexed_reach_matches_every_sufficient_budget;
    [exact path|exact done|lia].
Qed.

Theorem fuel_free_source_indexed_root_terminates_deterministically :
  forall hash_fn type_id bytes,
    exists final,
      reaches (Recursive.recursive_par_reference hash_fn) hash_fn bytes
        (root_configuration type_id bytes) final /\
      terminal final /\
      (forall another,
        reaches (Recursive.recursive_par_reference hash_fn) hash_fn bytes
          (root_configuration type_id bytes) another ->
        terminal another -> another = final) /\
      terminal_root_result final =
        Frames.source_indexed_root_full_result hash_fn type_id bytes.
Proof.
  intros hash_fn type_id bytes.
  destruct (every_source_indexed_configuration_terminates
    (Recursive.recursive_par_reference hash_fn) hash_fn bytes
    (root_configuration type_id bytes))
    as [final [path done]].
  exists final. repeat split; try assumption.
  - intros another another_path another_done.
    eapply source_indexed_terminal_is_unique;
      [exact another_path|exact another_done|exact path|exact done].
  - now apply terminal_root_result_for_any_reach.
Qed.

Print Assumptions every_source_indexed_configuration_terminates.
Print Assumptions source_indexed_reach_matches_every_sufficient_budget.
Print Assumptions source_indexed_terminal_is_unique.
Print Assumptions terminal_root_result_for_any_reach.
Print Assumptions fuel_free_source_indexed_root_terminates_deterministically.
