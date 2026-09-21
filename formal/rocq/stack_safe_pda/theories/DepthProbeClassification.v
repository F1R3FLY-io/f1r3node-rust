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

Definition stack_survival (outcome : Outcome) : bool + Outcome :=
  match outcome with
  | Passed => inl true
  | StackExhausted => inl false
  | failure => inr failure
  end.

Theorem survival_true_exact : forall outcome,
  stack_survival outcome = inl true <-> outcome = Passed.
Proof. intros []; cbn; intuition discriminate. Qed.

Theorem survival_false_exact : forall outcome,
  stack_survival outcome = inl false <-> outcome = StackExhausted.
Proof. intros []; cbn; intuition discriminate. Qed.

Theorem survival_error_preserved : forall outcome,
  outcome <> Passed -> outcome <> StackExhausted ->
  stack_survival outcome = inr outcome.
Proof. intros []; cbn; intuition congruence. Qed.

Example panicked_exit_rejects_boolean_status_mutant :
  stack_survival Panicked <> inl false.
Proof. discriminate. Qed.

Definition checked_survival {E : Type} (observed : Outcome + E) : bool + (Outcome + E) :=
  match observed with
  | inl outcome => match stack_survival outcome with
                  | inl survived => inl survived
                  | inr failure => inr (inl failure)
                  end
  | inr error => inr (inr error)
  end.

Theorem survival_transport_error_preserved : forall E (error : E),
  checked_survival (inr error) = inr (inr error).
Proof. reflexivity. Qed.

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
Print Assumptions survival_true_exact.
Print Assumptions survival_false_exact.
Print Assumptions survival_error_preserved.
Print Assumptions panicked_exit_rejects_boolean_status_mutant.
Print Assumptions survival_transport_error_preserved.

End DepthProbeClassification.

Module DepthProbeCorePolicy.

Inductive InstalledPolicy := SuppressionInstalled | Unsupported.
Inductive CorePolicy := Installed | ExplicitlyAllowed | PlatformUnsupported.

Definition apply_policy {E : Type} (allow : bool)
    (installer : unit -> InstalledPolicy + E) : (CorePolicy + E) * bool :=
  if allow then (inl ExplicitlyAllowed, false) else
    (match installer tt with
     | inl SuppressionInstalled => inl Installed
     | inl Unsupported => inl PlatformUnsupported
     | inr error => inr error
     end, true).

Definition may_run {E : Type} (result : CorePolicy + E) : bool :=
  match result with inl _ => true | inr _ => false end.

Theorem explicit_allow_skips_installer : forall E installer,
  @apply_policy E true installer = (inl ExplicitlyAllowed, false).
Proof. reflexivity. Qed.

Theorem suppression_installed_exact : forall E allow installer,
  @apply_policy E allow installer = (inl Installed, true) <->
  allow = false /\ installer tt = inl SuppressionInstalled.
Proof.
  intros E [] installer; cbn; try (split; intros; intuition discriminate).
  destruct (installer tt) as [[]|error]; split; intros; intuition discriminate.
Qed.

Theorem unsupported_preserved : forall E,
  @apply_policy E false (fun _ => inl Unsupported) =
    (inl PlatformUnsupported, true).
Proof. reflexivity. Qed.

Theorem installation_error_preserved : forall E (error : E),
  apply_policy false (fun _ => inr error) = (inr error, true).
Proof. reflexivity. Qed.

Theorem installation_error_prevents_body : forall E (error : E),
  may_run (fst (apply_policy false (fun _ => inr error))) = false.
Proof. reflexivity. Qed.

Theorem explicit_allow_permits_body : forall E installer,
  may_run (fst (@apply_policy E true installer)) = true.
Proof. reflexivity. Qed.

Theorem unsupported_not_installed : forall E,
  fst (@apply_policy E false (fun _ => inl Unsupported)) <> inl Installed.
Proof. discriminate. Qed.

Theorem failed_policy_is_not_stack_evidence :
  DepthProbeClassification.classify false false false false false false false false
    <> DepthProbeClassification.StackExhausted.
Proof. discriminate. Qed.

Definition selected_policy {E : Type} (selected allow : bool)
    (installer : unit -> InstalledPolicy + E) :=
  if selected then Some (apply_policy allow installer) else None.

Theorem unselected_child_does_not_apply_policy : forall E allow installer,
  @selected_policy E false allow installer = None.
Proof. reflexivity. Qed.

Print Assumptions explicit_allow_skips_installer.
Print Assumptions suppression_installed_exact.
Print Assumptions unsupported_preserved.
Print Assumptions installation_error_preserved.
Print Assumptions installation_error_prevents_body.
Print Assumptions explicit_allow_permits_body.
Print Assumptions unsupported_not_installed.
Print Assumptions failed_policy_is_not_stack_evidence.
Print Assumptions unselected_child_does_not_apply_policy.

End DepthProbeCorePolicy.
