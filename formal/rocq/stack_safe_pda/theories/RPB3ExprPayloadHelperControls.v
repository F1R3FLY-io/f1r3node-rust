(** Computed controls for the expression payload score helpers (leaf L02 of
    the score-arms design).

    Each family [Example] states the target helper's result and the projected
    oracle result on concrete children, and proves both by [vm_compute].
    Expected values are written out independently of the helpers, so a swapped
    constructor, a wrong default or a swapped operand fails to compile.  The
    red controls then show that the fixed-point check rejects what it must and
    leaves alone what it must. *)

From Stdlib Require Import List Bool Arith NArith ZArith.
From StackSafePDA Require Import RPB3GeneratedFieldRefinement
  RPB3SemanticReducer RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleExprPayload
  RPB3IndependentOracleRecursive.

Import ListNotations.

(** ** Concrete children *)

Definition target_scalar (tag : nat) (wire : wire_kind) (body : list nat)
    : semantic_summary :=
  SemanticSummary tag wire Scalar 0 body false None ScoreNotDerived
    false false.

Definition target_message (tag type_id : nat) (body : list nat)
    (score : canonical_score) : semantic_summary :=
  SemanticSummary tag LengthDelimited Message type_id body false
    (Some true) (ExactScore score) false false.

(** A pair as an EMap parent sees it: the carrier score of the pair child. *)
Definition target_kvp (pair_body : list nat) (key_score : canonical_score)
    (identity_bytes : list nat) : semantic_summary :=
  target_message 1 34 pair_body
    (score_constructor 0 [key_score; ScoreBytes identity_bytes]).

Definition target_incomplete_pair (pair_body : list nat) : semantic_summary :=
  SemanticSummary 1 LengthDelimited Message 34 pair_body false
    (Some false) ScoreNotDerived false false.

Definition target_scan (seen : list nat) : scan_state :=
  ScanState true None seen [] [].

Definition source_raw (tag : nat) (wire : wire_kind) (payload : payload_kind)
    (body : list nat) (type_id : option nat) : raw_field :=
  RawField tag wire payload body (length body) type_id [] [].

Definition source_scalar (tag : nat) (wire : wire_kind) (body : list nat)
    (value : source_scalar_value) : source_exact_summary :=
  SourceExactSummary
    (TypedSourceScalar (source_raw tag wire Scalar body None) value)
    (SourceFamilyAnalysis None None false false false) None.

Definition source_message (tag type_id : nat) (body : list nat)
    (nested : list typed_source_value) (score : source_score)
    : source_exact_summary :=
  SourceExactSummary
    (TypedSourceMessage
      (source_raw tag LengthDelimited Message body (Some type_id)) nested)
    (SourceFamilyAnalysis (Some true) (Some score) false false false) None.

Definition source_kvp (pair_body : list nat) (key_score : source_score)
    (identity_bytes : list nat) : source_exact_summary :=
  source_message 1 34 pair_body []
    (source_score_constructor 0 [key_score; SourceBytes identity_bytes]).

Definition source_incomplete_pair (pair_body : list nat)
    : source_exact_summary :=
  SourceExactSummary
    (TypedSourceMessage
      (source_raw 1 LengthDelimited Message pair_body (Some 34)) [])
    (SourceFamilyAnalysis (Some false) None false false false) None.

Definition source_scan (seen : list nat) : source_admission_state :=
  SourceAdmissionState true None seen.

Definition first_score : canonical_score := ScoreInteger 11.
Definition second_score : canonical_score := ScoreInteger 22.
Definition bound_var_score : canonical_score :=
  score_constructor 50 [ScoreInteger 4].

Definition source_first_score : source_score := SourceInteger 11.
Definition source_second_score : source_score := SourceInteger 22.
Definition source_bound_var_score : source_score :=
  source_score_constructor 50 [SourceInteger 4].

(** Both reducers produce [expected]: the target directly, the oracle after
    projection to target scores. *)
Definition both_reducers_score (target_result : option canonical_score)
    (oracle_result : option source_score) (expected : canonical_score)
    : Prop :=
  target_result = Some expected /\
  option_map project_score oracle_result = Some expected.

(** ** Family controls *)

(** Var 6.  Sint32 bodies: [6] is 3, [7] is -4 and [0] is 0.  BoundVar 0 is
    canonical for the oneof, which the forbidden-default helper would reject
    (last conjunct). *)
Example var_family_scores :
  both_reducers_score
    (exact_var_score [target_scalar 1 Varint [6]])
    (source_var_score [source_scalar 1 Varint [6] (SourceI32 3)])
    (score_constructor 50 [ScoreInteger 3]) /\
  both_reducers_score
    (exact_var_score [target_scalar 2 Varint [7]])
    (source_var_score [source_scalar 2 Varint [7] (SourceI32 (-4))])
    (score_constructor 51 [ScoreInteger 18446744073709551612]) /\
  both_reducers_score
    (exact_var_score [target_message 3 7 [] (ScoreInteger 0)])
    (source_var_score [source_message 3 7 [] [] (SourceInteger 0)])
    (score_constructor 52 []) /\
  both_reducers_score
    (exact_var_score [])
    (source_var_score [])
    (ScoreInteger 0) /\
  both_reducers_score
    (exact_var_score [target_scalar 1 Varint [0]])
    (source_var_score [source_scalar 1 Varint [0] (SourceI32 0)])
    (score_constructor 50 [ScoreInteger 0]) /\
  exact_i32_or_absent 1 [target_scalar 1 Varint [0]] = None.
Proof. vm_compute. repeat split. Qed.

(** EVar 35: a present empty Var scores 0 and is still constructor 100; an
    absent [v] has no score. *)
Example evar_family_scores :
  both_reducers_score
    (exact_expr_payload_score 35 [target_message 1 6 [8] bound_var_score])
    (source_expr_payload_score 35
      [source_message 1 6 [8] [] source_bound_var_score])
    (score_constructor 100 [bound_var_score]) /\
  both_reducers_score
    (exact_expr_payload_score 35 [target_message 1 6 [] (ScoreInteger 0)])
    (source_expr_payload_score 35 [source_message 1 6 [] [] (SourceInteger 0)])
    (score_constructor 100 [ScoreInteger 0]) /\
  exact_expr_payload_score 35 [] = None /\
  source_expr_payload_score 35 [] = None.
Proof. vm_compute. repeat split. Qed.

(** ENot 36 and ENeg 37. *)
Example unary_family_scores :
  both_reducers_score
    (exact_expr_payload_score 36 [target_message 1 1 [] first_score])
    (source_expr_payload_score 36 [source_message 1 1 [] [] source_first_score])
    (score_constructor 112 [first_score]) /\
  both_reducers_score
    (exact_expr_payload_score 37 [target_message 1 1 [] first_score])
    (source_expr_payload_score 37 [source_message 1 1 [] [] source_first_score])
    (score_constructor 101 [first_score]).
Proof. vm_compute. repeat split. Qed.

(** Every binary type 38..54, against the constructor table in type-id order,
    with distinct operands so that swapping them fails. *)
Definition binary_type_ids : list nat :=
  [38; 39; 40; 41; 42; 43; 44; 45; 46; 47; 48; 49; 50; 51; 52; 53; 54].

Definition binary_expected (constructor : N) : canonical_score :=
  score_constructor constructor [first_score; second_score].

Definition binary_expected_scores : list canonical_score :=
  [binary_expected 102; binary_expected 103; binary_expected 122;
   binary_expected 104; binary_expected 105; binary_expected 106;
   binary_expected 107; binary_expected 108; binary_expected 109;
   binary_expected 110; binary_expected 111; binary_expected 113;
   binary_expected 114; binary_expected 118; binary_expected 119;
   binary_expected 120; binary_expected 121].

Example binary_family_scores :
  map (fun type_id =>
    exact_expr_payload_score type_id
      [target_message 1 1 [] first_score;
       target_message 2 1 [] second_score]) binary_type_ids =
    map Some binary_expected_scores /\
  map (fun type_id =>
    option_map project_score (source_expr_payload_score type_id
      [source_message 1 1 [] [] source_first_score;
       source_message 2 1 [] [] source_second_score])) binary_type_ids =
    map Some binary_expected_scores.
Proof. vm_compute. repeat split. Qed.

(** EList 27.  A locally_free child (tag 3) never enters the score; an absent
    remainder scores -1. *)
Example elist_family_scores :
  both_reducers_score
    (exact_expr_payload_score 27
      [target_message 1 1 [] first_score;
       target_message 1 1 [] second_score;
       target_scalar 3 LengthDelimited [7];
       target_scalar 4 Varint [1];
       target_message 5 6 [8] bound_var_score])
    (source_expr_payload_score 27
      [source_message 1 1 [] [] source_first_score;
       source_message 1 1 [] [] source_second_score;
       source_scalar 3 LengthDelimited [7] (SourceBlob [7]);
       source_scalar 4 Varint [1] (SourceBool true);
       source_message 5 6 [8] [] source_bound_var_score])
    (score_constructor 6
      [bound_var_score; first_score; second_score; ScoreInteger 1]) /\
  both_reducers_score
    (exact_expr_payload_score 27 [target_message 1 1 [] first_score])
    (source_expr_payload_score 27 [source_message 1 1 [] [] source_first_score])
    (score_constructor 6
      [ScoreInteger 18446744073709551615; first_score; ScoreInteger 0]).
Proof. vm_compute. repeat split. Qed.

(** ETuple 28 has no remainder slot. *)
Example etuple_family_scores :
  both_reducers_score
    (exact_expr_payload_score 28
      [target_message 1 1 [] first_score;
       target_message 1 1 [] second_score;
       target_scalar 3 LengthDelimited [7];
       target_scalar 4 Varint [1]])
    (source_expr_payload_score 28
      [source_message 1 1 [] [] source_first_score;
       source_message 1 1 [] [] source_second_score;
       source_scalar 3 LengthDelimited [7] (SourceBlob [7]);
       source_scalar 4 Varint [1] (SourceBool true)])
    (score_constructor 7 [first_score; second_score; ScoreInteger 1]).
Proof. vm_compute. repeat split. Qed.

(** ESet 29, with the absent-remainder default and with a remainder. *)
Example eset_family_scores :
  both_reducers_score
    (exact_expr_payload_score 29
      [target_message 1 1 [] first_score;
       target_message 1 1 [] second_score])
    (source_expr_payload_score 29
      [source_message 1 1 [] [] source_first_score;
       source_message 1 1 [] [] source_second_score])
    (score_constructor 8
      [ScoreInteger 18446744073709551615; first_score; second_score;
       ScoreInteger 0]) /\
  both_reducers_score
    (exact_expr_payload_score 29
      [target_message 1 1 [] first_score;
       target_message 5 6 [8] bound_var_score])
    (source_expr_payload_score 29
      [source_message 1 1 [] [] source_first_score;
       source_message 5 6 [8] [] source_bound_var_score])
    (score_constructor 8 [bound_var_score; first_score; ScoreInteger 0]).
Proof. vm_compute. repeat split. Qed.

(** EMap 30: key scores only, read from the pair carriers.  The identity bytes
    inside the carriers do not enter the score. *)
Example emap_family_scores :
  both_reducers_score
    (exact_expr_payload_score 30
      [target_kvp [10; 2] first_score [1];
       target_kvp [10; 3] second_score [2];
       target_scalar 3 LengthDelimited [7];
       target_scalar 4 Varint [1];
       target_message 5 6 [8] bound_var_score])
    (source_expr_payload_score 30
      [source_kvp [10; 2] source_first_score [1];
       source_kvp [10; 3] source_second_score [2];
       source_scalar 3 LengthDelimited [7] (SourceBlob [7]);
       source_scalar 4 Varint [1] (SourceBool true);
       source_message 5 6 [8] [] source_bound_var_score])
    (score_constructor 9
      [bound_var_score; first_score; second_score; ScoreInteger 1]) /\
  both_reducers_score
    (exact_expr_payload_score 30 [])
    (source_expr_payload_score 30 [])
    (score_constructor 9 [ScoreInteger 18446744073709551615; ScoreInteger 0]).
Proof. vm_compute. repeat split. Qed.

(** KeyValuePair 34 carrier.  The key is a Par with one Expr and a
    locally_free field: [42;2;16;2] is exprs (tag 5) holding GInt, and
    [74;1;7] is the root tag-9 field, so the identity is [42;2;16;2].  The
    value child does not enter the carrier. *)
Example kvp_carrier_scores :
  both_reducers_score
    (exact_kvp_carrier
      [target_message 1 1 [42; 2; 16; 2; 74; 1; 7] first_score;
       target_message 2 1 [42; 0] second_score])
    (source_kvp_carrier
      [source_message 1 1 [42; 2; 16; 2; 74; 1; 7]
        [TypedSourceMessage
           (source_raw 5 LengthDelimited Message [16; 2] (Some 24)) [];
         TypedSourceScalar
           (source_raw 9 LengthDelimited Scalar [7] None) (SourceBlob [7])]
        source_first_score;
       source_message 2 1 [42; 0] [] source_second_score])
    (score_constructor 0 [first_score; ScoreBytes [42; 2; 16; 2]]).
Proof. vm_compute. repeat split. Qed.

(** EMethod 33.  The name is tag 1, the target tag 2, the arguments tag 3 and
    the flag tag 6.  A tag-5 locally_free child with body [1] must not be
    mistaken for the flag; an absent name is the empty string. *)
Example emethod_family_scores :
  both_reducers_score
    (exact_expr_payload_score 33
      [target_scalar 1 LengthDelimited [109; 110];
       target_message 2 1 [] first_score;
       target_message 3 1 [] second_score;
       target_message 3 1 [] first_score;
       target_scalar 6 Varint [1]])
    (source_expr_payload_score 33
      [source_scalar 1 LengthDelimited [109; 110] (SourceText [109; 110]);
       source_message 2 1 [] [] source_first_score;
       source_message 3 1 [] [] source_second_score;
       source_message 3 1 [] [] source_first_score;
       source_scalar 6 Varint [1] (SourceBool true)])
    (score_constructor 115
      [ScoreString [109; 110]; first_score; second_score; first_score;
       ScoreInteger 1]) /\
  both_reducers_score
    (exact_expr_payload_score 33
      [target_message 2 1 [] first_score;
       target_scalar 5 LengthDelimited [1]])
    (source_expr_payload_score 33
      [source_message 2 1 [] [] source_first_score;
       source_scalar 5 LengthDelimited [1] (SourceBlob [1])])
    (score_constructor 115 [ScoreString []; first_score; ScoreInteger 0]).
Proof. vm_compute. repeat split. Qed.

(** VarRef 56 carrier: index tag 1 is 3 and depth tag 2 is 1. *)
Example var_ref_carrier_scores :
  both_reducers_score
    (exact_var_ref_carrier
      [target_scalar 1 Varint [6]; target_scalar 2 Varint [2]])
    (source_var_ref_carrier
      [source_scalar 1 Varint [6] (SourceI32 3);
       source_scalar 2 Varint [2] (SourceI32 1)])
    (score_constructor 403 [ScoreInteger 3; ScoreInteger 1]).
Proof. vm_compute. repeat split. Qed.

(** ConnectiveBody 57 carrier. *)
Example connective_body_carrier_scores :
  both_reducers_score
    (exact_connective_body_carrier
      [target_message 1 1 [] first_score;
       target_message 1 1 [] second_score])
    (source_connective_body_carrier
      [source_message 1 1 [] [] source_first_score;
       source_message 1 1 [] [] source_second_score])
    (score_constructor 0 [first_score; second_score]).
Proof. vm_compute. repeat split. Qed.

(** Connective arms: And (tag 1) and Or (tag 2) replace the carrier constructor
    with 401 and 402; VarRef (tag 4) reads its carrier as the score. *)
Example connective_and_or_varref_arms :
  both_reducers_score
    (exact_connective_message_arm
      (target_message 1 57 []
        (score_constructor 0 [first_score; second_score])))
    (source_connective_message_arm
      (source_message 1 57 [] []
        (source_score_constructor 0
          [source_first_score; source_second_score])))
    (score_constructor 401 [first_score; second_score]) /\
  both_reducers_score
    (exact_connective_message_arm
      (target_message 2 57 []
        (score_constructor 0 [first_score; second_score])))
    (source_connective_message_arm
      (source_message 2 57 [] []
        (source_score_constructor 0
          [source_first_score; source_second_score])))
    (score_constructor 402 [first_score; second_score]) /\
  both_reducers_score
    (exact_connective_message_arm
      (target_message 4 56 []
        (score_constructor 403 [ScoreInteger 3; ScoreInteger 1])))
    (source_connective_message_arm
      (source_message 4 56 [] []
        (source_score_constructor 403 [SourceInteger 3; SourceInteger 1])))
    (score_constructor 403 [ScoreInteger 3; ScoreInteger 1]).
Proof. vm_compute. repeat split. Qed.

(** ** Identity and completeness *)

(** Only the root tag-9 field is removed.  [74;1;9] is the root locally_free
    (slot 7); [90;5;...] is bundles (tag 11, slot 9) holding a Bundle whose
    body Par contains its own locally_free, which must stay. *)
Example par_identity_strips_only_root_locally_free :
  par_identity_bytes [74; 1; 9; 90; 5; 10; 3; 74; 1; 7] =
    [90; 5; 10; 3; 74; 1; 7] /\
  source_par_identity_bytes
    (TypedSourceMessage
      (source_raw 1 LengthDelimited Message
        [74; 1; 9; 90; 5; 10; 3; 74; 1; 7] (Some 1))
      [TypedSourceScalar
         (source_raw 9 LengthDelimited Scalar [9] None) (SourceBlob [9]);
       TypedSourceMessage
         (source_raw 11 LengthDelimited Message [10; 3; 74; 1; 7] (Some 8))
         []]) =
    [90; 5; 10; 3; 74; 1; 7].
Proof. vm_compute. repeat split. Qed.

Definition target_low_pair : semantic_summary :=
  target_kvp [10; 0] (ScoreInteger 1) [1].
Definition target_high_pair : semantic_summary :=
  target_kvp [10; 1] (ScoreInteger 2) [2].
Definition source_low_pair : source_exact_summary :=
  source_kvp [10; 0] (SourceInteger 1) [1].
Definition source_high_pair : source_exact_summary :=
  source_kvp [10; 1] (SourceInteger 2) [2].

Definition completeness_type_ids : list nat :=
  [6; 27; 28; 29; 33; 34; 35; 36; 37; 38; 54; 55; 56; 57].

Definition completeness_with_first_slot_seen : list (option bool) :=
  [Some true; Some true; Some true; Some true; Some false; Some false;
   Some true; Some true; Some true; Some false; Some false;
   None; None; None].

(** Slot rules on a state where only slot 0 is seen, then where both are; EMap
    is the conjunction over its pairs and ignores its other children. *)
Example payload_completeness_agrees :
  map (fun type_id => expr_payload_completeness type_id (target_scan [0]) [])
    completeness_type_ids = completeness_with_first_slot_seen /\
  map (fun type_id =>
    source_expr_payload_completeness type_id (source_scan [0]) [])
    completeness_type_ids = completeness_with_first_slot_seen /\
  map (fun type_id =>
    expr_payload_completeness type_id (target_scan [1; 0]) [])
    [33; 34; 38; 54] = [Some true; Some true; Some true; Some true] /\
  map (fun type_id =>
    source_expr_payload_completeness type_id (source_scan [1; 0]) [])
    [33; 34; 38; 54] = [Some true; Some true; Some true; Some true] /\
  expr_payload_completeness 30 (target_scan [])
    [target_low_pair; target_incomplete_pair [10; 9];
     target_scalar 4 Varint [1]] = Some false /\
  source_expr_payload_completeness 30 (source_scan [])
    [source_low_pair; source_incomplete_pair [10; 9];
     source_scalar 4 Varint [1] (SourceBool true)] = Some false /\
  expr_payload_completeness 30 (target_scan [])
    [target_low_pair; target_high_pair;
     target_scalar 4 Varint [1]] = Some true /\
  source_expr_payload_completeness 30 (source_scan [])
    [source_low_pair; source_high_pair;
     source_scalar 4 Varint [1] (SourceBool true)] = Some true.
Proof. vm_compute. repeat split. Qed.

(** The oracle's single fall-through: carriers regardless of completeness, the
    pair carrier and the payload families only when complete. *)
Example source_family_score_gates_on_completeness :
  option_map project_score (source_expr_payload_family_score 57 None
    [source_message 1 1 [] [] source_first_score]) =
    Some (score_constructor 0 [first_score]) /\
  option_map project_score (source_expr_payload_family_score 56 None
    [source_scalar 1 Varint [6] (SourceI32 3)]) =
    Some (score_constructor 403 [ScoreInteger 3; ScoreInteger 0]) /\
  source_expr_payload_family_score 34 (Some false)
    [source_message 1 1 [] [] source_first_score] = None /\
  option_map project_score (source_expr_payload_family_score 34 (Some true)
    [source_message 1 1 [] [] source_first_score]) =
    Some (score_constructor 0 [first_score; ScoreBytes []]) /\
  source_expr_payload_family_score 35 (Some false)
    [source_message 1 6 [] [] (SourceInteger 0)] = None /\
  option_map project_score (source_expr_payload_family_score 35 (Some true)
    [source_message 1 6 [] [] (SourceInteger 0)]) =
    Some (score_constructor 100 [ScoreInteger 0]) /\
  source_expr_payload_family_score 55 (Some true) [] = None.
Proof. vm_compute. repeat split. Qed.

(** ** Ordering: green and red controls *)

(** The comparator is strict, breaks score ties on the raw body, and orders a
    strict prefix first. *)
Example strict_comparators_agree :
  score_body_strict_before (first_score, [42; 0]) (first_score, [42; 0])
    = false /\
  source_key_strict_before
    (source_first_score, [42; 0]) (source_first_score, [42; 0]) = false /\
  score_body_strict_before (first_score, [42]) (first_score, [42; 0])
    = true /\
  source_key_strict_before
    (source_first_score, [42]) (source_first_score, [42; 0]) = true /\
  score_body_strict_before (first_score, [42; 0]) (first_score, [42])
    = false /\
  source_key_strict_before
    (source_first_score, [42; 0]) (source_first_score, [42]) = false /\
  score_body_strict_before (ScoreInteger 1, [74]) (ScoreInteger 2, [42])
    = true /\
  source_key_strict_before
    (SourceInteger 1, [74]) (SourceInteger 2, [42]) = true /\
  score_body_strict_before (ScoreInteger 2, [42]) (ScoreInteger 1, [74])
    = false /\
  source_key_strict_before
    (SourceInteger 2, [42]) (SourceInteger 1, [74]) = false.
Proof. vm_compute. repeat split. Qed.

(** Red: a complete EMap with descending keys is outside the fixed point. *)
Example unordered_emap_is_violated :
  payload_fixed_point_violated 30 (Some true)
    [target_high_pair; target_low_pair] = true /\
  source_collection_outside_fixed_point 30 (Some true)
    [source_high_pair; source_low_pair] = true.
Proof. vm_compute. repeat split. Qed.

(** Red: the same out-of-order pairs in an incomplete EMap are not refused by
    this check. *)
Example incomplete_emap_is_not_violated :
  payload_fixed_point_violated 30 (Some false)
    [target_high_pair; target_low_pair] = false /\
  source_collection_outside_fixed_point 30 (Some false)
    [source_high_pair; source_low_pair] = false.
Proof. vm_compute. repeat split. Qed.

(** Green: ascending keys with distinct identities; the flag and remainder
    children are not pairs. *)
Example ordered_emap_is_not_violated :
  payload_fixed_point_violated 30 (Some true)
    [target_low_pair; target_high_pair;
     target_scalar 4 Varint [1];
     target_message 5 6 [8] bound_var_score] = false /\
  source_collection_outside_fixed_point 30 (Some true)
    [source_low_pair; source_high_pair;
     source_scalar 4 Varint [1] (SourceBool true);
     source_message 5 6 [8] [] source_bound_var_score] = false.
Proof. vm_compute. repeat split. Qed.

(** Equal key scores tie-break on the whole pair body, and a repeated key
    identity is a violation even when the keys ascend. *)
Example emap_tie_break_and_identity_controls :
  payload_fixed_point_violated 30 (Some true)
    [target_kvp [10; 0] (ScoreInteger 1) [1];
     target_kvp [10; 1] (ScoreInteger 1) [2]] = false /\
  source_collection_outside_fixed_point 30 (Some true)
    [source_kvp [10; 0] (SourceInteger 1) [1];
     source_kvp [10; 1] (SourceInteger 1) [2]] = false /\
  payload_fixed_point_violated 30 (Some true)
    [target_kvp [10; 1] (ScoreInteger 1) [2];
     target_kvp [10; 0] (ScoreInteger 1) [1]] = true /\
  source_collection_outside_fixed_point 30 (Some true)
    [source_kvp [10; 1] (SourceInteger 1) [2];
     source_kvp [10; 0] (SourceInteger 1) [1]] = true /\
  payload_fixed_point_violated 30 (Some true)
    [target_kvp [10; 0] (ScoreInteger 1) [7];
     target_kvp [10; 1] (ScoreInteger 2) [7]] = true /\
  source_collection_outside_fixed_point 30 (Some true)
    [source_kvp [10; 0] (SourceInteger 1) [7];
     source_kvp [10; 1] (SourceInteger 2) [7]] = true.
Proof. vm_compute. repeat split. Qed.

(** Three ESet elements with one score and ascending bodies.  Their identities
    are [], [42;0] and []: the first and third repeat without being adjacent,
    which an adjacent-only check would accept.  The first two conjuncts show
    the order leg passes and locate the repeat. *)
Definition target_duplicate_identity_elements : list semantic_summary :=
  [target_message 1 1 [] first_score;
   target_message 1 1 [42; 0] first_score;
   target_message 1 1 [74; 1; 7] first_score].

Definition source_duplicate_identity_elements : list source_exact_summary :=
  [source_message 1 1 [] [] source_first_score;
   source_message 1 1 [42; 0]
     [TypedSourceMessage
        (source_raw 5 LengthDelimited Message [] (Some 24)) []]
     source_first_score;
   source_message 1 1 [74; 1; 7]
     [TypedSourceScalar
        (source_raw 9 LengthDelimited Scalar [7] None) (SourceBlob [7])]
     source_first_score].

Example non_adjacent_duplicate_identity_eset_is_violated :
  payload_keys_strictly_ascending
    [(first_score, []); (first_score, [42; 0]);
     (first_score, [74; 1; 7])] = true /\
  map par_identity_bytes [[]; [42; 0]; [74; 1; 7]] = [[]; [42; 0]; []] /\
  payload_fixed_point_violated 29 (Some true)
    target_duplicate_identity_elements = true /\
  source_collection_outside_fixed_point 29 (Some true)
    source_duplicate_identity_elements = true.
Proof. vm_compute. repeat split. Qed.

(** ESet order compares the score before the body; the flag and remainder
    children are not elements. *)
Example eset_score_dominates_body :
  payload_fixed_point_violated 29 (Some true)
    [target_message 1 1 [74; 1; 7] (ScoreInteger 1);
     target_message 1 1 [42; 0] (ScoreInteger 2);
     target_scalar 4 Varint [1];
     target_message 5 6 [8] bound_var_score] = false /\
  source_collection_outside_fixed_point 29 (Some true)
    [source_message 1 1 [74; 1; 7]
       [TypedSourceScalar
          (source_raw 9 LengthDelimited Scalar [7] None) (SourceBlob [7])]
       (SourceInteger 1);
     source_message 1 1 [42; 0]
       [TypedSourceMessage
          (source_raw 5 LengthDelimited Message [] (Some 24)) []]
       (SourceInteger 2);
     source_scalar 4 Varint [1] (SourceBool true);
     source_message 5 6 [8] [] source_bound_var_score] = false /\
  payload_fixed_point_violated 29 (Some true)
    [target_message 1 1 [42; 0] (ScoreInteger 2);
     target_message 1 1 [74; 1; 7] (ScoreInteger 1)] = true /\
  source_collection_outside_fixed_point 29 (Some true)
    [source_message 1 1 [42; 0]
       [TypedSourceMessage
          (source_raw 5 LengthDelimited Message [] (Some 24)) []]
       (SourceInteger 2);
     source_message 1 1 [74; 1; 7]
       [TypedSourceScalar
          (source_raw 9 LengthDelimited Scalar [7] None) (SourceBlob [7])]
       (SourceInteger 1)] = true.
Proof. vm_compute. repeat split. Qed.

(** An element whose score is not derived gives no evidence of a violation. *)
Example underived_element_is_not_a_violation :
  payload_fixed_point_violated 29 (Some true)
    [target_message 1 1 [42; 2; 16; 2] (ScoreInteger 2);
     SemanticSummary 1 LengthDelimited Message 1 [42; 0] false (Some true)
       ScoreNotDerived false false] = false /\
  source_collection_outside_fixed_point 29 (Some true)
    [source_message 1 1 [42; 2; 16; 2] [] (SourceInteger 2);
     SourceExactSummary
       (TypedSourceMessage
         (source_raw 1 LengthDelimited Message [42; 0] (Some 1)) [])
       (SourceFamilyAnalysis (Some true) None false false false) None]
    = false.
Proof. vm_compute. repeat split. Qed.

Print Assumptions var_family_scores.
Print Assumptions evar_family_scores.
Print Assumptions unary_family_scores.
Print Assumptions binary_family_scores.
Print Assumptions elist_family_scores.
Print Assumptions etuple_family_scores.
Print Assumptions eset_family_scores.
Print Assumptions emap_family_scores.
Print Assumptions kvp_carrier_scores.
Print Assumptions emethod_family_scores.
Print Assumptions var_ref_carrier_scores.
Print Assumptions connective_body_carrier_scores.
Print Assumptions connective_and_or_varref_arms.
Print Assumptions par_identity_strips_only_root_locally_free.
Print Assumptions payload_completeness_agrees.
Print Assumptions source_family_score_gates_on_completeness.
Print Assumptions strict_comparators_agree.
Print Assumptions unordered_emap_is_violated.
Print Assumptions incomplete_emap_is_not_violated.
Print Assumptions ordered_emap_is_not_violated.
Print Assumptions emap_tie_break_and_identity_controls.
Print Assumptions non_adjacent_duplicate_identity_eset_is_violated.
Print Assumptions eset_score_dominates_body.
Print Assumptions underived_element_is_not_a_violation.
