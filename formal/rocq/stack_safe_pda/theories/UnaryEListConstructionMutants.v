From Stdlib Require Import List Arith.
From StackSafePDA Require Import UnaryEListConstruction.

Import ListNotations.
Module U := UnaryEListConstruction.UnaryEListConstruction.

Definition terminal : U.bytes := [42; 2; 16; 0].

Theorem wrong_expr_tag_is_detected :
  U.field [162; 2] (U.field [10] terminal) <> U.wrap_expr terminal.
Proof. vm_compute. discriminate. Qed.

Theorem wrong_child_length_is_detected :
  U.expr_prefix (U.make_frame 3) ++ terminal <> U.wrap_expr terminal.
Proof. vm_compute. discriminate. Qed.

Theorem omitted_terminal_is_detected :
  U.emit_par (U.plan 1 (length terminal)) [] <> U.resident_reference 1 terminal.
Proof. vm_compute. discriminate. Qed.

Theorem duplicated_terminal_is_detected :
  U.emit_par (U.plan 1 (length terminal)) (terminal ++ terminal) <>
    U.resident_reference 1 terminal.
Proof. vm_compute. discriminate. Qed.

Theorem wrong_remainder_score_is_detected :
  Some ([2] ++ U.integer_atom 6 ++ U.integer_atom 0 ++
    U.terminal_score [] false ++ U.close) <> U.direct_expr_score 1 [] false.
Proof. vm_compute. discriminate. Qed.

Theorem terminal_connective_loss_is_detected :
  U.direct_expr_score 1 [] false <> U.direct_expr_score 1 [] true.
Proof. vm_compute. discriminate. Qed.

Theorem missing_par_score_close_is_detected :
  Some (U.elist_head ++ U.par_head ++ U.integer_atom 0 ++ U.close) <>
    U.direct_expr_score 1 [] false.
Proof. vm_compute. discriminate. Qed.

Theorem wrong_score_trailer_is_detected :
  U.endpoint [1] terminal <> [82; 80; 66; 3; 5] ++ [1] ++ terminal ++ [0; 0; 0; 0; 0; 0; 0; 0].
Proof. vm_compute. discriminate. Qed.

Theorem off_by_one_depth_is_detected :
  U.emit_par (U.plan 1 (length terminal)) terminal <> U.resident_reference 2 terminal.
Proof. vm_compute. discriminate. Qed.

Theorem zero_depth_metadata_reset_is_detected :
  U.wrapped_metadata 0 (U.RootMetadata [7] true 2) <> U.RootMetadata [] false 1.
Proof. discriminate. Qed.

Theorem unchecked_size_acceptance_is_detected : U.checked_size 31 32 <> Some 32.
Proof. vm_compute. discriminate. Qed.

Theorem frame_overflow_cannot_publish :
  U.checked_positive_endpoint 3 0 terminal [] false = None.
Proof. vm_compute. reflexivity. Qed.

Theorem key_reservation_overflow_cannot_publish :
  U.checked_positive_endpoint 32 0 terminal [] false = None.
Proof. vm_compute. reflexivity. Qed.

Theorem positive_checked_construction_is_nonvacuous :
  exists result, U.checked_positive_endpoint 256 0 terminal [] false = Some result.
Proof. vm_compute. eexists. reflexivity. Qed.

Print Assumptions wrong_expr_tag_is_detected.
Print Assumptions wrong_child_length_is_detected.
Print Assumptions omitted_terminal_is_detected.
Print Assumptions duplicated_terminal_is_detected.
Print Assumptions wrong_remainder_score_is_detected.
Print Assumptions terminal_connective_loss_is_detected.
Print Assumptions missing_par_score_close_is_detected.
Print Assumptions wrong_score_trailer_is_detected.
Print Assumptions off_by_one_depth_is_detected.
Print Assumptions zero_depth_metadata_reset_is_detected.
Print Assumptions unchecked_size_acceptance_is_detected.
Print Assumptions frame_overflow_cannot_publish.
Print Assumptions key_reservation_overflow_cannot_publish.
Print Assumptions positive_checked_construction_is_nonvacuous.
