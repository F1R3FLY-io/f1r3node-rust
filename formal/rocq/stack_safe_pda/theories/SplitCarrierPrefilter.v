From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import EPM1.

Import ListNotations.
Set Implicit Arguments.

Module SplitCarrierPrefilter.

Record Outer := OuterState {
  other_occurrences : nat;
  expr_occurrences : nat;
  root_locally_free : list nat;
  root_connective : bool
}.

Record ListBody := ListState {
  children : list nat;
  list_locally_free : list nat;
  list_connective : bool;
  list_remainder : option nat
}.

Inductive Expr := Absent | ListExpr (body : ListBody) | OtherExpr (tag : nat).
Inductive Hint := IsList | NotList | Inconclusive.
Inductive Event := InspectHeader | DecodeExpr.

Definition eligible (outer : Outer) : bool :=
  Nat.eqb (other_occurrences outer) 0 && Nat.eqb (expr_occurrences outer) 1 &&
  match root_locally_free outer with [] => true | _ => false end &&
  negb (root_connective outer).

Definition inspect (expr : Expr) : option ListBody :=
  match expr with
  | ListExpr body =>
      match list_locally_free body, list_connective body, list_remainder body with
      | [], false, None => Some body
      | _, _, _ => None
      end
  | _ => None
  end.

Definition reference (outer : Outer) (expr : Expr) : option ListBody :=
  if eligible outer then inspect expr else None.

Definition filtered (outer : Outer) (expr : Expr) (hint : Hint) : option ListBody :=
  if eligible outer then
    match hint with NotList => None | _ => inspect expr end
  else None.

Definition trace (outer : Outer) (hint : Hint) : list Event :=
  if eligible outer then
    match hint with NotList => [InspectHeader] | _ => [InspectHeader; DecodeExpr] end
  else [].

Definition sound (expr : Expr) (hint : Hint) : Prop :=
  hint = NotList -> forall body, expr <> ListExpr body.

Definition canonical_hint (expr : Expr) : Hint :=
  match expr with
  | Absent => NotList
  | ListExpr _ => IsList
  | OtherExpr tag => if Nat.eqb tag 20 then Inconclusive else NotList
  end.

Theorem list_header : EPM1.encode_varint (20 * 8 + 2) = [162; 1].
Proof. vm_compute. reflexivity. Qed.

Theorem outer_false : forall outer expr hint,
  eligible outer = false -> filtered outer expr hint = None /\ trace outer hint = [].
Proof. intros. unfold filtered, trace. rewrite H. auto. Qed.

Theorem header_list : forall body, canonical_hint (ListExpr body) = IsList.
Proof. reflexivity. Qed.

Theorem header_nonlist : forall tag,
  tag <> 20 -> canonical_hint (OtherExpr tag) = NotList.
Proof. intros. unfold canonical_hint. apply Nat.eqb_neq in H. now rewrite H. Qed.

Theorem reference_success_is_list : forall outer expr body,
  reference outer expr = Some body -> expr = ListExpr body.
Proof.
  intros outer expr body result. unfold reference in result.
  destruct (eligible outer); [|discriminate].
  destruct expr as [|[cs lf cu rem]|tag]; cbn [inspect] in result; try discriminate.
  destruct lf, cu, rem; inversion result; reflexivity.
Qed.

Theorem conclusive_rejection_sound : forall outer expr,
  sound expr NotList -> reference outer expr = None.
Proof.
  intros outer expr valid. destruct (reference outer expr) eqn:result; [|reflexivity].
  apply reference_success_is_list in result. subst expr.
  exfalso. exact (valid eq_refl l eq_refl).
Qed.

Theorem result_equivalence : forall outer expr hint,
  sound expr hint -> filtered outer expr hint = reference outer expr.
Proof.
  intros outer expr hint valid. destruct hint.
  - unfold filtered, reference. destruct (eligible outer); reflexivity.
  - rewrite (conclusive_rejection_sound outer valid).
    unfold filtered. destruct (eligible outer); reflexivity.
  - unfold filtered, reference. destruct (eligible outer); reflexivity.
Qed.

Theorem observations_equivalence : forall (Observation : Type)
  (observe : option ListBody -> Observation) outer expr hint,
  sound expr hint -> observe (filtered outer expr hint) = observe (reference outer expr).
Proof. intros. now rewrite result_equivalence. Qed.

Theorem rejection_has_no_decode : forall outer, ~ In DecodeExpr (trace outer NotList).
Proof. intros. unfold trace. destruct (eligible outer); simpl; intuition discriminate. Qed.

Theorem inconclusive_preserves_reference : forall outer expr,
  filtered outer expr Inconclusive = reference outer expr.
Proof. intros. unfold filtered, reference. destruct (eligible outer); reflexivity. Qed.

Theorem list_hint_retains_metadata_checks : forall outer body,
  filtered outer (ListExpr body) IsList = reference outer (ListExpr body).
Proof. intros. unfold filtered, reference. destruct (eligible outer); reflexivity. Qed.

Theorem outer_guards_not_bypassed : forall outer expr hint,
  eligible outer = false -> filtered outer expr hint = reference outer expr.
Proof. intros. unfold filtered, reference. now rewrite H. Qed.

Theorem returned_children_are_exact : forall outer expr hint body,
  sound expr hint -> filtered outer expr hint = Some body ->
  expr = ListExpr body /\ reference outer expr = Some body.
Proof.
  intros outer expr hint body valid result. rewrite result_equivalence in result by assumption.
  split; [now apply reference_success_is_list in result|assumption].
Qed.

Theorem canonical_hint_sound : forall expr, sound expr (canonical_hint expr).
Proof.
  intros expr. destruct expr as [|body|tag]; unfold sound, canonical_hint.
  - discriminate.
  - discriminate.
  - destruct (Nat.eqb tag 20); discriminate.
Qed.

Theorem canonical_result_equivalence : forall outer expr,
  filtered outer expr (canonical_hint expr) = reference outer expr.
Proof. intros. apply result_equivalence, canonical_hint_sound. Qed.

Definition clear_outer := OuterState 0 1 [] false.
Definition clear_list := ListState [4; 9; 4] [] false None.

Theorem wire_header_controls :
  EPM1.encode_varint (20 * 8 + 2) <> EPM1.encode_varint (21 * 8 + 2) /\
  EPM1.encode_varint (20 * 8 + 2) <> EPM1.encode_varint (20 * 8) /\
  canonical_hint (OtherExpr 21) = NotList /\ canonical_hint (OtherExpr 22) = NotList.
Proof. vm_compute. repeat split; discriminate. Qed.

Theorem outer_guard_mutant_controls :
  reference clear_outer (ListExpr clear_list) = Some clear_list /\
  reference (OuterState 1 1 [] false) (ListExpr clear_list) = None /\
  reference (OuterState 0 2 [] false) (ListExpr clear_list) = None /\
  reference (OuterState 0 0 [] false) (ListExpr clear_list) = None /\
  reference (OuterState 0 1 [1] false) (ListExpr clear_list) = None /\
  reference (OuterState 0 1 [] true) (ListExpr clear_list) = None.
Proof. repeat split; reflexivity. Qed.

Theorem list_metadata_mutant_controls :
  filtered clear_outer (ListExpr (ListState [4] [1] false None)) IsList = None /\
  filtered clear_outer (ListExpr (ListState [4] [] true None)) IsList = None /\
  filtered clear_outer (ListExpr (ListState [4] [] false (Some 0))) IsList = None /\
  filtered clear_outer (ListExpr (ListState [] [] false None)) IsList =
    Some (ListState [] [] false None).
Proof. repeat split; reflexivity. Qed.

Theorem hint_mutant_controls :
  filtered clear_outer (ListExpr clear_list) Inconclusive = Some clear_list /\
  filtered clear_outer Absent IsList = None /\
  filtered clear_outer (OtherExpr 22) IsList = None /\
  filtered clear_outer (ListExpr clear_list) NotList <> reference clear_outer (ListExpr clear_list).
Proof. repeat split; discriminate. Qed.

Theorem decode_order_mutant_control :
  trace clear_outer NotList <> [InspectHeader; DecodeExpr] /\
  trace clear_outer IsList = [InspectHeader; DecodeExpr] /\
  trace clear_outer Inconclusive = [InspectHeader; DecodeExpr].
Proof. repeat split; discriminate. Qed.

Print Assumptions list_header.
Print Assumptions outer_false.
Print Assumptions header_list.
Print Assumptions header_nonlist.
Print Assumptions reference_success_is_list.
Print Assumptions conclusive_rejection_sound.
Print Assumptions result_equivalence.
Print Assumptions observations_equivalence.
Print Assumptions rejection_has_no_decode.
Print Assumptions inconclusive_preserves_reference.
Print Assumptions list_hint_retains_metadata_checks.
Print Assumptions outer_guards_not_bypassed.
Print Assumptions returned_children_are_exact.
Print Assumptions canonical_hint_sound.
Print Assumptions canonical_result_equivalence.
Print Assumptions wire_header_controls.
Print Assumptions outer_guard_mutant_controls.
Print Assumptions list_metadata_mutant_controls.
Print Assumptions hint_mutant_controls.
Print Assumptions decode_order_mutant_control.

End SplitCarrierPrefilter.
