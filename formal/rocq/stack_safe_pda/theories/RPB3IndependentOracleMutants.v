From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleProjection.

Import ListNotations.

Module Schema := RPB3GeneratedFieldRefinement.
Module Target := RPB3SemanticReducer.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.

Definition process_message_child (raw : Schema.raw_field)
    (values : list Core.typed_source_value) (score : Core.source_score)
    : Core.source_exact_summary :=
  Core.SourceExactSummary (Core.TypedSourceMessage raw values)
    (Core.SourceFamilyAnalysis (Some true) (Some score)
      false false false) None.

Definition bundle_body_child (value : N) : Core.source_exact_summary :=
  process_message_child
    (Schema.RawField 1 Schema.LengthDelimited Schema.Message [] 0
      (Some 1) [] []) [] (Core.SourceInteger value).

Definition bundle_omit_body_score_mutant
    (children : list Core.source_exact_summary) : option Core.source_score :=
  match Process.source_child_score 1 children,
      Process.source_bool_or_absent 2 children,
      Process.source_bool_or_absent 3 children with
  | Some _, Some write, Some read =>
      Some (Core.source_score_constructor
        (Process.source_bundle_constructor read write) [])
  | _, _, _ => None
  end.

Definition bundle_actual (value : N) : option Core.source_score :=
  Process.source_process_family_score 8 (Some true)
    [bundle_body_child value].

Definition bundle_mutant (value : N) : option Core.source_score :=
  bundle_omit_body_score_mutant [bundle_body_child value].

Theorem bundle_omitted_body_score_is_detected :
  Process.source_child_score 1 [bundle_body_child 7%N] =
    Some (Core.SourceInteger 7%N) /\
  bundle_actual 7%N <> bundle_mutant 7%N /\
  bundle_actual 7%N <> bundle_actual 8%N /\
  bundle_mutant 7%N = bundle_mutant 8%N /\
  Target.exact_bundle_score
    [Projection.project_source_summary (bundle_body_child 7%N)] =
    option_map Core.project_score (bundle_actual 7%N).
Proof.
  repeat split; vm_compute; congruence.
Qed.

Definition default_receive_condition : Core.source_exact_summary :=
  process_message_child
    (Schema.RawField 8 Schema.LengthDelimited Schema.Message [] 0
      (Some 1) [] []) [] (Core.SourceInteger 0%N).

Definition nondefault_receive_connective : Core.typed_source_value :=
  Core.TypedSourceScalar
    (Schema.RawField 10 Schema.Varint Schema.Scalar [1] 1
      None [] []) (Core.SourceBool true).

Definition nondefault_receive_condition : Core.source_exact_summary :=
  process_message_child
    (Schema.RawField 8 Schema.LengthDelimited Schema.Message [80; 1] 2
      (Some 1) [] []) [nondefault_receive_connective]
    (Core.SourceInteger 0%N).

Definition receive_seen_state : Admission.source_admission_state :=
  Admission.SourceAdmissionState true (Some 1) [1].

Definition receive_actual_refusal
    (condition : Core.source_exact_summary) : bool :=
  Process.source_process_family_refused 19
    (Process.source_process_completeness 19 receive_seen_state [condition])
    receive_seen_state [condition].

Definition ignore_receive_default_mutant (type_id : nat)
    (complete : option bool) (state : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : bool :=
  if type_id =? 19 then false
  else Process.source_process_family_refused type_id complete state children.

Definition receive_mutant_refusal
    (condition : Core.source_exact_summary) : bool :=
  ignore_receive_default_mutant 19
    (Process.source_process_completeness 19 receive_seen_state [condition])
    receive_seen_state [condition].

Theorem receive_explicit_default_refusal_is_detected :
  Process.source_par_typed_default
    (Core.source_summary_value default_receive_condition) = Some true /\
  Process.source_par_typed_default
    (Core.source_summary_value nondefault_receive_condition) = Some false /\
  Process.source_process_completeness 19 receive_seen_state
    [default_receive_condition] = Some true /\
  receive_actual_refusal default_receive_condition = true /\
  receive_actual_refusal nondefault_receive_condition = false /\
  receive_mutant_refusal default_receive_condition = false /\
  receive_mutant_refusal nondefault_receive_condition = false /\
  Target.complete_receive_has_default_condition 19 (Some true)
    [Projection.project_source_summary default_receive_condition] = true.
Proof.
  repeat split; vm_compute; congruence.
Qed.

Definition new_uri_child (byte : nat) : Core.source_exact_summary :=
  Core.SourceExactSummary
    (Core.TypedSourceScalar
      (Schema.RawField 3 Schema.LengthDelimited Schema.Scalar
        [byte] 1 None [] []) (Core.SourceText [byte]))
    (Core.SourceFamilyAnalysis (Some true)
      (Some (Core.SourceString [byte])) false false false) None.

Definition new_uri_seen_state : Admission.source_admission_state :=
  Admission.SourceAdmissionState true (Some 1) [1].

Definition new_uri_actual_refusal
    (children : list Core.source_exact_summary) : bool :=
  Process.source_process_family_refused 20
    (Process.source_process_completeness 20 new_uri_seen_state children)
    new_uri_seen_state children.

Definition ignore_new_uri_order_mutant (type_id : nat)
    (complete : option bool) (state : Admission.source_admission_state)
    (children : list Core.source_exact_summary) : bool :=
  if type_id =? 20 then false
  else Process.source_process_family_refused type_id complete state children.

Theorem new_uri_order_refusal_is_detected :
  Process.source_new_uri_ordered [new_uri_child 1; new_uri_child 2] = true /\
  Process.source_new_uri_ordered [new_uri_child 2; new_uri_child 1] = false /\
  new_uri_actual_refusal [new_uri_child 1; new_uri_child 2] = false /\
  new_uri_actual_refusal [new_uri_child 2; new_uri_child 1] = true /\
  ignore_new_uri_order_mutant 20 (Some true) new_uri_seen_state
    [new_uri_child 2; new_uri_child 1] = false /\
  Target.new_uri_ordered
    (map Projection.project_source_summary
      [new_uri_child 2; new_uri_child 1]) = false.
Proof.
  repeat split; vm_compute; congruence.
Qed.

Print Assumptions bundle_omitted_body_score_is_detected.
Print Assumptions receive_explicit_default_refusal_is_detected.
Print Assumptions new_uri_order_refusal_is_detected.
