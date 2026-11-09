From Stdlib Require Import List Arith.
From StackSafePDA Require Import RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleProcess
  RPB3IndependentOracleProjection RPB3IndependentOracleProcessProjection
  RPB3IndependentOracleRecursive.

Import ListNotations.

Module Oracle := RPB3IndependentOracleRecursive.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.

(** The source and target score trees are distinct mutually inductive pairs
    with the same shape, so their serializers are related by one simultaneous
    induction over a source score and its forest. *)
Scheme source_score_mutual_ind := Induction for Core.source_score Sort Prop
with source_score_forest_mutual_ind :=
  Induction for Core.source_score_forest Sort Prop.
Combined Scheme source_score_and_forest_ind
  from source_score_mutual_ind, source_score_forest_mutual_ind.

Lemma source_integer_bytes_projects :
  forall bits,
    Oracle.source_integer_bytes bits = Target.score_integer_bytes bits.
Proof. reflexivity. Qed.

Lemma source_terminated_bytes_projects :
  forall bytes,
    Oracle.source_terminated_bytes bytes =
      Target.score_terminated_bytes bytes.
Proof.
  induction bytes as [|byte rest IH];
    cbn [Oracle.source_terminated_bytes Target.score_terminated_bytes].
  - reflexivity.
  - now rewrite IH.
Qed.

Lemma source_score_and_forest_bytes_project :
  (forall score,
    Oracle.source_score_bytes score =
      Target.canonical_score_bytes (Core.project_score score)) /\
  (forall forest,
    Oracle.source_score_forest_bytes forest =
      Target.canonical_score_forest_bytes
        (Core.project_score_forest forest)).
Proof.
  apply source_score_and_forest_ind;
    cbn [Oracle.source_score_bytes Oracle.source_score_forest_bytes
      Core.project_score Core.project_score_forest
      Target.canonical_score_bytes Target.canonical_score_forest_bytes].
  - exact source_integer_bytes_projects.
  - intro bytes. now rewrite source_terminated_bytes_projects.
  - intro bytes. now rewrite source_terminated_bytes_projects.
  - intros children children_project. now rewrite children_project.
  - reflexivity.
  - intros first first_project rest rest_project.
    now rewrite first_project, rest_project.
Qed.

Theorem source_score_bytes_projects :
  forall s, Oracle.source_score_bytes s =
            Target.canonical_score_bytes (Core.project_score s).
Proof. exact (proj1 source_score_and_forest_bytes_project). Qed.

Lemma source_and_known_projects :
  forall left right,
    Oracle.source_and_known left right = Target.and_known left right.
Proof. intros [[|]|] [[|]|]; reflexivity. Qed.

Lemma source_score_body_order_projects :
  forall left right,
    Oracle.source_score_body_order left right =
      Target.score_body_order
        (Projection.project_source_summary left)
        (Projection.project_source_summary right).
Proof.
  intros left right.
  unfold Oracle.source_score_body_order, Target.score_body_order.
  change (Target.sem_score_evidence (Projection.project_source_summary left))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score left)).
  change (Target.sem_score_evidence (Projection.project_source_summary right))
    with (Projection.project_source_evidence
      (Core.source_summary_exact_score right)).
  change (Target.sem_body (Projection.project_source_summary left))
    with (Core.source_summary_body left).
  change (Target.sem_body (Projection.project_source_summary right))
    with (Core.source_summary_body right).
  destruct (Core.source_summary_exact_score left) as [left_score|];
    destruct (Core.source_summary_exact_score right) as [right_score|];
    cbn [Projection.project_source_evidence].
  - rewrite !source_score_bytes_projects.
    rewrite !ProcessProjection.projected_process_bytes_leq.
    reflexivity.
  - reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

(** These two equations name one unfolding of each Par-order recursion.  The
    recursive call is on a cons cell, so a blind [simpl] or [cbn] of either
    fixpoint would also unfold that call and hide the induction hypothesis. *)
Lemma source_par_component_order_unfold_pair :
  forall first second tail,
    Oracle.source_par_component_order (first :: second :: tail) =
      Oracle.source_and_known
        (if Core.source_summary_tag first =?
            Core.source_summary_tag second
         then Oracle.source_score_body_order first second else Some true)
        (Oracle.source_par_component_order (second :: tail)).
Proof. reflexivity. Qed.

Lemma raw_par_component_order_unfold_pair :
  forall first second tail,
    Target.raw_par_component_order (first :: second :: tail) =
      Target.and_known
        (if Target.sem_tag first =? Target.sem_tag second
         then Target.score_body_order first second else Some true)
        (Target.raw_par_component_order (second :: tail)).
Proof. reflexivity. Qed.

Lemma source_par_component_order_projects :
  forall children,
    Oracle.source_par_component_order children =
      Target.raw_par_component_order
        (map Projection.project_source_summary children).
Proof.
  induction children as [|first rest IH]; [reflexivity|].
  destruct rest as [|second tail]; [reflexivity|].
  cbn [map] in IH |- *.
  rewrite source_par_component_order_unfold_pair,
    raw_par_component_order_unfold_pair.
  rewrite source_and_known_projects, IH, source_score_body_order_projects.
  change (Target.sem_tag (Projection.project_source_summary first))
    with (Core.source_summary_tag first).
  change (Target.sem_tag (Projection.project_source_summary second))
    with (Core.source_summary_tag second).
  reflexivity.
Qed.

Print Assumptions source_integer_bytes_projects.
Print Assumptions source_terminated_bytes_projects.
Print Assumptions source_score_and_forest_bytes_project.
Print Assumptions source_score_bytes_projects.
Print Assumptions source_and_known_projects.
Print Assumptions source_score_body_order_projects.
Print Assumptions source_par_component_order_unfold_pair.
Print Assumptions raw_par_component_order_unfold_pair.
Print Assumptions source_par_component_order_projects.
