From Stdlib Require Import Bool.

Module DepthProbeClassification.

Inductive Outcome := Passed | TimedOut | Panicked | StackExhausted
  | MemoryExhausted | Signaled | ExitFailure.

Definition classify (timeout success panic abort native_stack stack_message
    allocation_message signaled : bool) : Outcome :=
  if timeout then TimedOut else
  if success then Passed else
  if panic then Panicked else
  if native_stack || (abort && stack_message) then StackExhausted else
  if abort && allocation_message then MemoryExhausted else
  if signaled then Signaled else ExitFailure.

Theorem timeout_dominates : forall success panic abort native_stack stack_message
    allocation_message signaled,
  classify true success panic abort native_stack stack_message allocation_message signaled = TimedOut.
Proof. reflexivity. Qed.

Theorem clean_success_ignores_diagnostics : forall panic abort native_stack stack_message
    allocation_message signaled,
  classify false true panic abort native_stack stack_message allocation_message signaled = Passed.
Proof. reflexivity. Qed.

Theorem panic_dominates_stack_text : forall abort native_stack stack_message
    allocation_message signaled,
  classify false false true abort native_stack stack_message allocation_message signaled = Panicked.
Proof. reflexivity. Qed.

Theorem native_stack_status_qualifies : forall abort stack_message allocation_message signaled,
  classify false false false abort true stack_message allocation_message signaled = StackExhausted.
Proof. reflexivity. Qed.

Theorem abort_and_stack_text_qualify : forall allocation_message signaled,
  classify false false false true false true allocation_message signaled = StackExhausted.
Proof. reflexivity. Qed.

Theorem unqualified_stack_text_is_not_a_ceiling : forall stack_message allocation_message signaled,
  classify false false false false false stack_message allocation_message signaled <> StackExhausted.
Proof. intros; destruct signaled; discriminate. Qed.

Theorem abort_without_diagnostics_is_not_a_ceiling : forall signaled,
  classify false false false true false false false signaled <> StackExhausted.
Proof. intros []; discriminate. Qed.

Theorem allocation_failure_is_not_a_ceiling : forall signaled,
  classify false false false true false false true signaled = MemoryExhausted.
Proof. reflexivity. Qed.

Theorem ambiguous_signal_remains_signal : forall stack_message allocation_message,
  classify false false false false false stack_message allocation_message true = Signaled.
Proof. reflexivity. Qed.

Theorem ordinary_failure_remains_failure : forall stack_message allocation_message,
  classify false false false false false stack_message allocation_message false = ExitFailure.
Proof. reflexivity. Qed.

Theorem stack_classification_exact : forall timeout success panic abort native_stack
    stack_message allocation_message signaled,
  classify timeout success panic abort native_stack stack_message allocation_message signaled = StackExhausted <->
  timeout = false /\ success = false /\ panic = false /\
    (native_stack = true \/ (abort = true /\ stack_message = true)).
Proof.
  intros [] [] [] [] [] [] [] []; cbn; intuition discriminate.
Qed.

Theorem passed_classification_exact : forall timeout success panic abort native_stack
    stack_message allocation_message signaled,
  classify timeout success panic abort native_stack stack_message allocation_message signaled = Passed <->
  timeout = false /\ success = true.
Proof.
  intros [] [] [] [] [] [] [] []; cbn; intuition discriminate.
Qed.

Definition text_first_mutant (stack_message : bool) :=
  if stack_message then StackExhausted else Panicked.

Example deceptive_panic_rejects_text_first_mutant :
  classify false false true false false true false false <> text_first_mutant true.
Proof. discriminate. Qed.

Example timeout_rejects_false_ceiling_mutant :
  classify true false false true false true false true <> StackExhausted.
Proof. discriminate. Qed.

Print Assumptions timeout_dominates.
Print Assumptions clean_success_ignores_diagnostics.
Print Assumptions panic_dominates_stack_text.
Print Assumptions native_stack_status_qualifies.
Print Assumptions abort_and_stack_text_qualify.
Print Assumptions unqualified_stack_text_is_not_a_ceiling.
Print Assumptions abort_without_diagnostics_is_not_a_ceiling.
Print Assumptions allocation_failure_is_not_a_ceiling.
Print Assumptions ambiguous_signal_remains_signal.
Print Assumptions ordinary_failure_remains_failure.
Print Assumptions stack_classification_exact.
Print Assumptions passed_classification_exact.
Print Assumptions deceptive_panic_rejects_text_first_mutant.
Print Assumptions timeout_rejects_false_ceiling_mutant.

End DepthProbeClassification.
