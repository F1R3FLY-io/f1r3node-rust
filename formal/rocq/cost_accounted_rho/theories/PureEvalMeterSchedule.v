(** The reservation schedule for the pure evaluator's explicit-stack driver.

    This is a structural abstraction of [rho-pure-eval/src/eval.rs]. A node's
    entry event precedes any child; each operand extraction precedes the next
    operand; and concatenation, equality inspection, and comparison scanning
    occur only after the requisite child evaluations. The [Emit] frames are
    where Rust invokes the backing meter. The proof says an explicit worklist
    has the same ordered event stream as the recursive specification and that
    its potential strictly decreases. It does not model the byte cost of an
    event, Rust ownership, or the spatial matcher; those require differential
    tests against the source implementation and the backing-walker model.

    Changed by the merge of feature/cost-accounted-rho (DR-131, plan step
    1.8): no meter runs at the [Emit] frames now, because DR-131 removes the
    backing meter of the pure evaluator. The proofs stay unchanged, and they
    keep their meaning: the explicit-stack driver gives the event stream of
    the recursive specification in the same order, and the driver
    terminates. The sentence above about the backing meter and the
    backing-walker model describes the code before DR-131.
*)
From Stdlib Require Import List Arith Lia.
Import ListNotations.

Module PureEvalMeterSchedule.

Inductive Event := EnterPar | EnterExpr | Extract | Concat
  | InspectLeft | InspectRight | ScanComparison | Combine.

Inductive Term :=
  | Literal
  | EmptyPar
  | ParNext (prefix value : Term)
  | Unary (operand : Term)
  | Binary (lhs rhs : Term)
  | Equality (lhs rhs : Term)
  | Comparison (lhs rhs : Term).

Fixpoint reference (term : Term) : list Event :=
  match term with
  | Literal => [EnterExpr]
  | EmptyPar => [EnterPar]
  | ParNext prefix value =>
      reference prefix ++ reference value ++ [Concat]
  | Unary operand =>
      EnterExpr :: reference operand ++ [Extract; Combine]
  | Binary lhs rhs =>
      EnterExpr :: reference lhs ++ [Extract] ++
        reference rhs ++ [Extract; Combine]
  | Equality lhs rhs =>
      EnterExpr :: reference lhs ++ reference rhs ++
        [InspectLeft; InspectRight; Combine]
  | Comparison lhs rhs =>
      EnterExpr :: reference lhs ++ [Extract] ++
        reference rhs ++ [Extract; ScanComparison; Combine]
  end.

Inductive Frame := Evaluate (term : Term) | Emit (event : Event).

Definition expand (term : Term) : list Frame :=
  match term with
  | Literal => [Emit EnterExpr]
  | EmptyPar => [Emit EnterPar]
  | ParNext prefix value =>
      [Evaluate prefix; Evaluate value; Emit Concat]
  | Unary operand =>
      [Emit EnterExpr; Evaluate operand; Emit Extract; Emit Combine]
  | Binary lhs rhs =>
      [Emit EnterExpr; Evaluate lhs; Emit Extract;
       Evaluate rhs; Emit Extract; Emit Combine]
  | Equality lhs rhs =>
      [Emit EnterExpr; Evaluate lhs; Evaluate rhs;
       Emit InspectLeft; Emit InspectRight; Emit Combine]
  | Comparison lhs rhs =>
      [Emit EnterExpr; Evaluate lhs; Emit Extract;
       Evaluate rhs; Emit Extract; Emit ScanComparison; Emit Combine]
  end.

Fixpoint meaning (work : list Frame) : list Event :=
  match work with
  | [] => []
  | Emit event :: rest => event :: meaning rest
  | Evaluate term :: rest => reference term ++ meaning rest
  end.

Lemma meaning_app : forall first second,
  meaning (first ++ second) = meaning first ++ meaning second.
Proof.
  induction first as [|frame rest IH]; intros second; cbn.
  - reflexivity.
  - destruct frame; cbn; rewrite IH; repeat rewrite app_assoc; reflexivity.
Qed.

Lemma expand_preserves_meaning : forall term rest,
  meaning (expand term ++ rest) = meaning (Evaluate term :: rest).
Proof.
  intros term rest. rewrite meaning_app.
  destruct term; cbn; repeat rewrite app_assoc; reflexivity.
Qed.

Fixpoint size (term : Term) : nat :=
  match term with
  | Literal | EmptyPar => 1
  | Unary child => 1 + size child
  | ParNext lhs rhs | Binary lhs rhs
  | Equality lhs rhs | Comparison lhs rhs =>
      1 + size lhs + size rhs
  end.

Definition weight (frame : Frame) : nat :=
  match frame with
  | Emit _ => 1
  | Evaluate term => 10 * size term
  end.

Fixpoint potential (work : list Frame) : nat :=
  match work with
  | [] => 0
  | frame :: rest => weight frame + potential rest
  end.

Lemma potential_app : forall first second,
  potential (first ++ second) = potential first + potential second.
Proof.
  induction first as [|frame rest IH]; intros second; cbn; [lia|].
  rewrite IH. lia.
Qed.

Lemma size_positive : forall term, 0 < size term.
Proof. induction term; cbn; lia. Qed.

Lemma expand_decreases_potential : forall term rest,
  potential (expand term ++ rest) < potential (Evaluate term :: rest).
Proof.
  intros term rest. rewrite potential_app. destruct term; cbn; lia.
Qed.

Fixpoint execute (fuel : nat) (work : list Frame) : option (list Event) :=
  match work with
  | [] => Some []
  | frame :: rest =>
      match fuel with
      | 0 => None
      | S remaining =>
          match frame with
          | Emit event => option_map (cons event) (execute remaining rest)
          | Evaluate term => execute remaining (expand term ++ rest)
          end
      end
  end.

Theorem worklist_exactly_matches_reference : forall fuel work,
  potential work <= fuel -> execute fuel work = Some (meaning work).
Proof.
  induction fuel as [|fuel IH]; intros work bound.
  - destruct work as [|frame rest]; [reflexivity|].
    cbn in bound. destruct frame; cbn in bound;
      try (pose proof (size_positive term)); lia.
  - destruct work as [|frame rest]; [reflexivity|].
    destruct frame as [term|event]; cbn.
    + rewrite (IH (expand term ++ rest)).
      * rewrite expand_preserves_meaning. reflexivity.
      * pose proof (expand_decreases_potential term rest). lia.
    + rewrite (IH rest); [reflexivity|]. cbn in bound. lia.
Qed.

Corollary evaluation_has_exact_event_order : forall term,
  execute (potential [Evaluate term]) [Evaluate term] =
    Some (reference term).
Proof.
  intro term.
  replace (reference term) with (meaning [Evaluate term])
    by (cbn; rewrite app_nil_r; reflexivity).
  apply worklist_exactly_matches_reference. lia.
Qed.

End PureEvalMeterSchedule.
