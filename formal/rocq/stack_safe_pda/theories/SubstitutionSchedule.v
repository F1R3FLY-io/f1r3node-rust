From Stdlib Require Import List Arith Bool ZArith Lia.
Import ListNotations.
Set Implicit Arguments.

Module SubstitutionSchedule.

Record context := Ctx { depth : Z; shift_delta : Z }.
Definition deeper c := Ctx (depth c + 1) (shift_delta c).
Definition shifted c n := Ctx (depth c) (shift_delta c + n).
Definition lookup_key level base c k := (level + (base + shift_delta c) - k - 1)%Z.

Theorem binder_shift_composes : forall c a b,
  shifted (shifted c a) b = shifted c (a + b).
Proof. intros [d s] a b; unfold shifted; cbn; f_equal; lia. Qed.

Theorem pattern_depth_and_binder_shift_commute : forall c n,
  deeper (shifted c n) = shifted (deeper c) n.
Proof. reflexivity. Qed.

Theorem borrowed_environment_key_equals_owned_shift : forall level base c n k,
  lookup_key level base (shifted c n) k = lookup_key level (base + n) c k.
Proof. intros; unfold lookup_key, shifted; cbn; lia. Qed.

Section Orders.
Context {A : Type}.

Fixpoint restore (order : list nat) (values : list A) : option (list A) :=
  match order with
  | [] => Some []
  | index :: tail =>
      match nth_error values index, restore tail values with
      | Some value, Some rest => Some (value :: rest)
      | _, _ => None
      end
  end.

Fixpoint identity_order (values : list A) : list nat :=
  match values with [] => [] | _ :: tail => 0 :: map S (identity_order tail) end.

Lemma restore_app : forall left right values xs ys,
  restore left values = Some xs -> restore right values = Some ys ->
  restore (left ++ right) values = Some (xs ++ ys).
Proof.
  induction left as [|i tail IH]; intros right values xs ys L R; cbn in *.
  - inversion L; subst; exact R.
  - destruct (nth_error values i); destruct (restore tail values) eqn:T; try discriminate.
    inversion L; subst; cbn; rewrite (IH right values l ys T R); reflexivity.
Qed.

Lemma restore_extension : forall order values suffix selected,
  restore order values = Some selected ->
  restore order (values ++ suffix) = Some selected.
Proof.
  induction order as [|i tail IH]; intros values suffix selected H; cbn in *; auto.
  destruct (nth_error values i) eqn:N; destruct (restore tail values) eqn:T; try discriminate.
  inversion H; subst.
  rewrite nth_error_app1.
  - rewrite N, (IH values suffix l T); reflexivity.
  - apply nth_error_Some; rewrite N; discriminate.
Qed.

Lemma restore_shift : forall order values prefix selected,
  restore order values = Some selected ->
  restore (map (Nat.add (length prefix)) order) (prefix ++ values) = Some selected.
Proof.
  induction order as [|i tail IH]; intros values prefix selected H; cbn in *; auto.
  destruct (nth_error values i) eqn:N; destruct (restore tail values) eqn:T; try discriminate.
  inversion H; subst.
  rewrite nth_error_app2 by lia.
  replace (length prefix + i - length prefix) with i by lia.
  rewrite N, (IH values prefix l T); reflexivity.
Qed.

Theorem identity_order_restores_every_value : forall values,
  restore (identity_order values) values = Some values.
Proof.
  induction values as [|value tail IH]; [reflexivity|].
  change (match restore (map S (identity_order tail)) (value :: tail) with
    | Some values => Some (value :: values) | None => None end = Some (value :: tail)).
  pose proof (@restore_shift (identity_order tail) tail [value] tail IH) as Shift.
  change (restore (map S (identity_order tail)) (value :: tail) = Some tail) in Shift.
  now rewrite Shift.
Qed.

Record plan := Plan { semantic : list A; wire_order : list nat }.
Definition valid_plan p wire := restore (wire_order p) (semantic p) = Some wire.
Definition identity_plan values := Plan values (identity_order values).
Definition append_plan a b :=
  Plan (semantic a ++ semantic b)
    (wire_order a ++ map (Nat.add (length (semantic a))) (wire_order b)).
Definition source_first source patterns :=
  Plan (source :: patterns) (map S (identity_order patterns) ++ [0]).
Definition case_source_first source pattern guard :=
  Plan [source; pattern; guard] [1; 0; 2].

Theorem identity_plan_correct : forall values, valid_plan (identity_plan values) values.
Proof. apply identity_order_restores_every_value. Qed.

Theorem append_plan_preserves_wire_association : forall a b left right,
  valid_plan a left -> valid_plan b right ->
  valid_plan (append_plan a b) (left ++ right).
Proof.
  intros a b left right L R; unfold valid_plan, append_plan in *; cbn.
  apply restore_app.
  - now apply restore_extension.
  - now apply restore_shift.
Qed.

Theorem source_first_restores_patterns_then_source : forall source patterns,
  valid_plan (source_first source patterns) (patterns ++ [source]).
Proof.
  intros; unfold valid_plan, source_first; cbn [wire_order semantic].
  apply restore_app; [|reflexivity].
  pose proof (@restore_shift (identity_order patterns) patterns [source] patterns
    (identity_order_restores_every_value patterns)) as H.
  exact H.
Qed.

Theorem case_source_first_restores_pattern_source_guard : forall source pattern guard,
  valid_plan (case_source_first source pattern guard) [pattern; source; guard].
Proof. reflexivity. Qed.

Fixpoint append_plans (plans : list plan) :=
  match plans with
  | [] => identity_plan []
  | p :: rest => append_plan p (append_plans rest)
  end.

Theorem appended_blocks_preserve_each_wire_association : forall plans wires,
  Forall2 valid_plan plans wires ->
  valid_plan (append_plans plans) (concat wires).
Proof.
  intros plans wires H; induction H; cbn.
  - reflexivity.
  - now apply append_plan_preserves_wire_association.
Qed.

Theorem restoration_preserves_result_mapping : forall (B : Type) (f : A -> B) order values selected,
  restore order values = Some selected ->
  let restore_b := fix go indices :=
    match indices with
    | [] => Some []
    | index :: tail =>
      match nth_error (map f values) index, go tail with
      | Some value, Some rest => Some (value :: rest)
      | _, _ => None
      end
    end in restore_b order = Some (map f selected).
Proof.
  intros B f order; induction order as [|i tail IH]; intros values selected H; cbn in *.
  - inversion H; reflexivity.
  - destruct (nth_error values i) eqn:N; destruct (restore tail values) eqn:T; try discriminate.
    inversion H; subst; rewrite nth_error_map, N; cbn; rewrite (IH values l T); reflexivity.
Qed.


Definition rotate_target_last (values : list A) :=
  match values with [] => [] | target :: arguments => arguments ++ [target] end.

Theorem method_rotation_restores_target_and_arguments : forall target arguments,
  restore (length arguments :: identity_order arguments)
    (rotate_target_last (target :: arguments)) = Some (target :: arguments).
Proof.
  intros; cbn [rotate_target_last restore].
  rewrite nth_error_app2 by lia.
  rewrite Nat.sub_diag; cbn [nth_error].
  rewrite (@restore_extension (identity_order arguments) arguments [target] arguments
    (identity_order_restores_every_value arguments)).
  reflexivity.
Qed.

End Orders.

Inductive job :=
| Substitute (term : nat) (ctx : context)
| Preserve (term : nat)
| Missing (ctx : context)
| Empty.

Definition required (preserve : bool) c term :=
  match term with
  | Some value => if preserve then Preserve value else Substitute value c
  | None => Missing c
  end.
Definition optional preserve c term :=
  match term with Some value => required preserve c (Some value) | None => Empty end.

Record bind_input := BindInput { patterns : list nat; source : option nat }.
Definition bind_patterns preserve c b :=
  map (fun term => required preserve (deeper c) (Some term)) (patterns b).
Definition bind_plan preserve c b :=
  source_first (required preserve c (source b)) (bind_patterns preserve c b).
Definition bind_wire preserve c b :=
  bind_patterns preserve c b ++ [required preserve c (source b)].

Definition receive_tail preserve c n body condition :=
  [required preserve (shifted c n) body; optional preserve (shifted c n) condition].
Definition receive_plan preserve c n binds body condition :=
  append_plan (append_plans (map (bind_plan preserve c) binds))
    (identity_plan (receive_tail preserve c n body condition)).
Definition receive_wire preserve c n binds body condition :=
  concat (map (bind_wire preserve c) binds) ++ receive_tail preserve c n body condition.

Theorem receive_plan_restores_each_bind_and_shifted_tail : forall preserve c n binds body condition,
  valid_plan (receive_plan preserve c n binds body condition)
    (receive_wire preserve c n binds body condition).
Proof.
  intros; unfold receive_plan, receive_wire; apply append_plan_preserves_wire_association.
  - apply appended_blocks_preserve_each_wire_association.
    induction binds; cbn; constructor; auto.
    apply source_first_restores_patterns_then_source.
  - apply identity_plan_correct.
Qed.

Record case_input := CaseInput {
  pattern : option nat;
  case_source : option nat;
  guard : option nat;
  free_count : Z
}.
Definition retain_case preserve input :=
  preserve || match pattern input, case_source input with Some _, Some _ => true | _, _ => false end.
Definition case_jobs preserve c input :=
  (required preserve (shifted c (free_count input)) (case_source input),
   required preserve (deeper c) (pattern input),
   optional preserve (shifted c (free_count input)) (guard input)).
Definition match_case_plan preserve c input :=
  if retain_case preserve input then
    let '(source, pattern, guard) := case_jobs preserve c input in
    case_source_first source pattern guard
  else identity_plan [].
Definition match_case_wire preserve c input :=
  if retain_case preserve input then
    let '(source, pattern, guard) := case_jobs preserve c input in [pattern; source; guard]
  else [].
Definition match_plan preserve c target cases :=
  append_plan (identity_plan [required preserve c target])
    (append_plans (map (match_case_plan preserve c) cases)).
Definition match_wire preserve c target cases :=
  required preserve c target :: concat (map (match_case_wire preserve c) cases).

Theorem match_case_plan_restores_only_retained_cases : forall preserve c input,
  valid_plan (match_case_plan preserve c input) (match_case_wire preserve c input).
Proof.
  intros; unfold match_case_plan, match_case_wire.
  destruct (retain_case preserve input); [apply case_source_first_restores_pattern_source_guard|reflexivity].
Qed.

Theorem match_plan_restores_target_and_retained_case_slots : forall preserve c target cases,
  valid_plan (match_plan preserve c target cases) (match_wire preserve c target cases).
Proof.
  intros; unfold match_plan, match_wire.
  change (valid_plan
    (append_plan (identity_plan [required preserve c target])
      (append_plans (map (match_case_plan preserve c) cases)))
    ([required preserve c target] ++ concat (map (match_case_wire preserve c) cases))).
  apply append_plan_preserves_wire_association; [apply identity_plan_correct|].
  apply appended_blocks_preserve_each_wire_association.
  induction cases; cbn; constructor; auto using match_case_plan_restores_only_retained_cases.
Qed.

Definition new_plan preserve c n body injections :=
  identity_plan (required preserve (shifted c n) body :: map Preserve injections).

Theorem new_plan_preserves_injections_without_substituting_them :
  forall preserve c n body injections,
  valid_plan (new_plan preserve c n body injections)
    (required preserve (shifted c n) body :: map Preserve injections).
Proof. intros; apply identity_plan_correct. Qed.

Section Evaluation.
Context {Value Error : Type}.
Variable evaluate : job -> (Error + Value)%type.

Fixpoint recursive_jobs jobs : (Error + list Value)%type :=
  match jobs with
  | [] => inr []
  | first :: tail =>
    match evaluate first with
    | inl error => inl error
    | inr value =>
      match recursive_jobs tail with
      | inl error => inl error | inr values => inr (value :: values)
      end
    end
  end.

Fixpoint accumulated_jobs jobs acc : (Error + list Value)%type :=
  match jobs with
  | [] => inr (rev acc)
  | first :: tail =>
    match evaluate first with
    | inl error => inl error
    | inr value => accumulated_jobs tail (value :: acc)
    end
  end.

Theorem accumulated_evaluation_preserves_results_and_first_error : forall jobs acc,
  accumulated_jobs jobs acc =
    match recursive_jobs jobs with
    | inl error => inl error
    | inr values => inr (rev acc ++ values)
    end.
Proof.
  induction jobs as [|first tail IH]; intros acc; cbn.
  - now rewrite app_nil_r.
  - destruct (evaluate first); [reflexivity|].
    rewrite IH; destruct (recursive_jobs tail); [reflexivity|].
    cbn; rewrite <- app_assoc; reflexivity.
Qed.

Theorem scheduled_evaluation_equals_recursive_job_evaluation : forall jobs,
  accumulated_jobs jobs [] = recursive_jobs jobs.
Proof.
  intros; rewrite accumulated_evaluation_preserves_results_and_first_error.
  destruct (recursive_jobs jobs); reflexivity.
Qed.

Theorem first_job_error_prevents_suffix_evaluation : forall first tail error,
  evaluate first = inl error -> recursive_jobs (first :: tail) = inl error.
Proof. intros; cbn; now rewrite H. Qed.

End Evaluation.

Print Assumptions binder_shift_composes.
Print Assumptions pattern_depth_and_binder_shift_commute.
Print Assumptions borrowed_environment_key_equals_owned_shift.
Print Assumptions identity_order_restores_every_value.
Print Assumptions append_plan_preserves_wire_association.
Print Assumptions source_first_restores_patterns_then_source.
Print Assumptions case_source_first_restores_pattern_source_guard.
Print Assumptions appended_blocks_preserve_each_wire_association.
Print Assumptions restoration_preserves_result_mapping.
Print Assumptions method_rotation_restores_target_and_arguments.
Print Assumptions receive_plan_restores_each_bind_and_shifted_tail.
Print Assumptions match_case_plan_restores_only_retained_cases.
Print Assumptions match_plan_restores_target_and_retained_case_slots.
Print Assumptions new_plan_preserves_injections_without_substituting_them.
Print Assumptions accumulated_evaluation_preserves_results_and_first_error.
Print Assumptions scheduled_evaluation_equals_recursive_job_evaluation.
Print Assumptions first_job_error_prevents_suffix_evaluation.

Section OpaquePreservation.

Context {Byte Score Metadata : Type}.
Variable nonsemantic : Score.

Record admitted_component := {
  admitted_family : nat;
  admitted_body : list Byte;
  admitted_multiplicity : nat;
  admitted_metadata : Metadata
}.

Record opaque_plan := {
  opaque_source : admitted_component;
  opaque_score : Score;
  opaque_jobs : list job
}.

Definition preserve_incomplete (source : admitted_component) : opaque_plan :=
  {| opaque_source := source; opaque_score := nonsemantic; opaque_jobs := [] |}.

Theorem incomplete_preservation_keeps_exact_body : forall source,
  admitted_body (opaque_source (preserve_incomplete source)) = admitted_body source.
Proof. reflexivity. Qed.

Theorem incomplete_preservation_keeps_family_count_and_metadata : forall source,
  (admitted_family (opaque_source (preserve_incomplete source)),
   admitted_multiplicity (opaque_source (preserve_incomplete source)),
   admitted_metadata (opaque_source (preserve_incomplete source))) =
  (admitted_family source, admitted_multiplicity source, admitted_metadata source).
Proof. reflexivity. Qed.

Theorem incomplete_preservation_has_no_required_child_jobs : forall source,
  opaque_jobs (preserve_incomplete source) = [].
Proof. reflexivity. Qed.

Theorem incomplete_preservation_uses_admission_nonsemantic_score : forall source,
  opaque_score (preserve_incomplete source) = nonsemantic.
Proof. reflexivity. Qed.

End OpaquePreservation.

Print Assumptions incomplete_preservation_keeps_exact_body.
Print Assumptions incomplete_preservation_keeps_family_count_and_metadata.
Print Assumptions incomplete_preservation_has_no_required_child_jobs.
Print Assumptions incomplete_preservation_uses_admission_nonsemantic_score.

End SubstitutionSchedule.
