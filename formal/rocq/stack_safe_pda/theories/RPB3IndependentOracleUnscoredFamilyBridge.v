From Stdlib Require Import List.
From StackSafePDA Require Import RPB3SemanticReducer
  RPB3IndependentOracleProcess RPB3IndependentOracleName
  RPB3IndependentOracleProjection.

Import ListNotations.

Module Target := RPB3SemanticReducer.
Module Process := RPB3IndependentOracleProcess.
Module Name := RPB3IndependentOracleName.
Module Projection := RPB3IndependentOracleProjection.

(** Reflects absence from a list into the computable listing test, so that
    each peeled literal identifier below is decided by evaluation. *)
Lemma absent_type_id_is_unlisted :
  forall type_id ids,
    ~ In type_id ids -> Target.listed_score_type type_id ids = false.
Proof.
  intros type_id ids absent.
  destruct (Target.listed_score_type type_id ids) eqn:listed.
  - exfalso.
    exact (absent (proj1 (Target.listed_score_type_iff type_id ids) listed)).
  - reflexivity.
Qed.

(** A listed literal contradicts the reflected premise by computation; any
    other peeled case reduces all three dispatches to their default arms. *)
Ltac close_unscored_literal_case unlisted :=
  first
    [ discriminate unlisted
    | (split; [|split]; reflexivity) ].

(** The deepest literal among the three dispatches is 60, so sixty-one
    successor peelings of [type_id] expose every literal arm and leave a
    residual tail in which only the default arms can match.  The unbounded
    [type_id] is thus covered through the finite depth of the [match], and
    the [Some false] fall-through of the target is excluded because its
    nonsemantic identifiers are all listed. *)
Lemma unscored_family_summaries_agree :
  forall type_id complete children,
    ~ In type_id [1;8;9;10;11;12;13;15;16;19;20;21;22;23;24;31;32;55;60] ->
    Target.score_evidence_for type_id complete
      (map Projection.project_source_summary children) = Target.ScoreNotDerived /\
    Process.source_process_family_score type_id complete children = None /\
    Name.source_name_family_score type_id complete children = None.
Proof.
  intros type_id complete children unscored.
  pose proof (absent_type_id_is_unlisted type_id _ unscored) as unlisted.
  clear unscored.
  destruct complete as [[|]|];
    do 61 (destruct type_id as [|type_id];
           [close_unscored_literal_case unlisted|]);
    close_unscored_literal_case unlisted.
Qed.

Print Assumptions absent_type_id_is_unlisted.
Print Assumptions unscored_family_summaries_agree.
