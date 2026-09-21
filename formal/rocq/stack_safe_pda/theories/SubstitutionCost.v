From Stdlib Require Import List Arith Lia.
From StackSafePDA Require Import SubstitutionRegionEmission.
Import ListNotations.
Set Implicit Arguments.

Module SubstitutionCost.

Inductive metric :=
| RootCopies | RootBytes | BorrowedRegions | OriginalBytes | ShallowBytes
| CompactBytes | ResidentBytes | ArenaCount | FinishAttempts | FinishSuccesses
| WorkSteps | SharedImports | FieldVisits | MessageNodes | ParCuts | RootKeys | RootKeyBytes.

Inductive event :=
| CopyRoot : nat -> event
| BorrowRegion : event
| EmitOriginal : nat -> event
| EmitShallow : nat -> event
| EmitCompact : nat -> event
| DecodeResident : nat -> event
| CreateArena | BeginFinish | FinishOk | VisitWork
| ImportShared | VisitField | PublishNode | CutPar : event
| EmitRootKey : nat -> event.

Definition weight (m : metric) (e : event) : nat :=
  match m, e with
  | RootCopies, CopyRoot _ => 1
  | RootBytes, CopyRoot n => n
  | BorrowedRegions, BorrowRegion => 1
  | OriginalBytes, EmitOriginal n => n
  | ShallowBytes, EmitShallow n => n
  | CompactBytes, EmitCompact n => n
  | ResidentBytes, DecodeResident n => n
  | ArenaCount, CreateArena => 1
  | FinishAttempts, BeginFinish => 1
  | FinishSuccesses, FinishOk => 1
  | WorkSteps, VisitWork => 1
  | SharedImports, ImportShared => 1
  | FieldVisits, VisitField => 1
  | MessageNodes, PublishNode => 1
  | ParCuts, CutPar => 1
  | RootKeys, EmitRootKey _ => 1
  | RootKeyBytes, EmitRootKey n => n
  | _, _ => 0
  end.

Fixpoint total (m : metric) (trace : list event) : nat :=
  match trace with [] => 0 | e :: rest => weight m e + total m rest end.

Fixpoint accumulate (m : metric) (trace : list event) (counter : nat) : nat :=
  match trace with
  | [] => counter
  | e :: rest => accumulate m rest (counter + weight m e)
  end.

Theorem accumulation_refines_reference : forall m trace counter,
  accumulate m trace counter = counter + total m trace.
Proof.
  intros m trace; induction trace as [|e rest IH]; intros; cbn; [lia|].
  rewrite IH; lia.
Qed.

Theorem concatenation_adds_costs : forall m left right,
  total m (left ++ right) = total m left + total m right.
Proof. intros m left; induction left; intros; cbn; [reflexivity|rewrite IHleft; lia]. Qed.

Theorem prefix_cost_never_exceeds_complete_trace : forall m prefix suffix,
  total m prefix <= total m (prefix ++ suffix).
Proof. intros; rewrite concatenation_adds_costs; lia. Qed.

Theorem shared_references_copy_no_body_bytes : forall trace,
  Forall (fun e => e = BorrowRegion \/ e = ImportShared) trace ->
  total RootBytes trace = 0 /\ total OriginalBytes trace = 0 /\
  total ShallowBytes trace = 0 /\ total CompactBytes trace = 0 /\
  total ResidentBytes trace = 0.
Proof.
  intros trace H; induction H; cbn; [auto|].
  destruct H as [B|B]; subst x; cbn; exact IHForall.
Qed.

Theorem disjoint_root_partition_copies_exact_source : forall sizes source_bytes,
  fold_right Nat.add 0 sizes = source_bytes ->
  total RootBytes (map CopyRoot sizes) = source_bytes.
Proof.
  intros sizes source_bytes H; subst.
  induction sizes; cbn; [reflexivity|now rewrite IHsizes].
Qed.

Theorem one_root_copy_per_endpoint : forall sizes,
  total RootCopies (map CopyRoot sizes) = length sizes.
Proof. induction sizes; cbn; [reflexivity|now rewrite IHsizes]. Qed.

Definition occurrence_trace (entry : nat * list event) := snd entry.
Definition endpoint_trace (entries : list (nat * list event)) :=
  concat (map occurrence_trace entries).

Theorem multiplicity_change_preserves_input_work : forall entries replace m,
  total m (endpoint_trace (map (fun e => (replace (fst e), snd e)) entries)) =
  total m (endpoint_trace entries).
Proof.
  intros entries replace m; unfold endpoint_trace.
  rewrite map_map; reflexivity.
Qed.

Definition no_finish (e : event) :=
  e <> CreateArena /\ e <> BeginFinish /\ e <> FinishOk.

Lemma no_finish_trace_has_zero_counters : forall trace,
  Forall no_finish trace ->
  total ArenaCount trace = 0 /\ total FinishAttempts trace = 0 /\
  total FinishSuccesses trace = 0.
Proof.
  intros trace H; induction H; cbn; [auto|].
  unfold no_finish in H; destruct x; cbn in *; intuition congruence.
Qed.

Theorem successful_machine_has_one_arena_and_finish : forall body,
  Forall no_finish body ->
  let trace := CreateArena :: body ++ [BeginFinish; FinishOk] in
  total ArenaCount trace = 1 /\ total FinishAttempts trace = 1 /\
  total FinishSuccesses trace = 1.
Proof.
  intros body H; pose proof (no_finish_trace_has_zero_counters H) as [A [B C]].
  cbn; repeat rewrite concatenation_adds_costs; cbn; rewrite A, B, C; auto.
Qed.

Theorem prefinish_error_has_no_finish_attempt : forall prefix,
  Forall no_finish prefix ->
  total FinishAttempts (CreateArena :: prefix) = 0 /\
  total FinishSuccesses (CreateArena :: prefix) = 0.
Proof.
  intros prefix H; pose proof (no_finish_trace_has_zero_counters H) as [A [B C]].
  cbn; auto.
Qed.

Theorem identity_bypass_has_no_arena_or_finish :
  total ArenaCount [] = 0 /\ total FinishAttempts [] = 0 /\
  total FinishSuccesses [] = 0.
Proof. auto. Qed.

Theorem local_budget_implies_total_budget : forall m trace budgets,
  Forall2 (fun e budget => weight m e <= budget) trace budgets ->
  total m trace <= fold_right Nat.add 0 budgets.
Proof.
  intros m trace budgets H; induction H; cbn; [lia|lia].
Qed.

Theorem declared_linear_partition_bounds_observed_cost :
  forall m trace budgets input output a b c,
  Forall2 (fun e budget => weight m e <= budget) trace budgets ->
  fold_right Nat.add 0 budgets <= a * input + b * output + c ->
  total m trace <= a * input + b * output + c.
Proof.
  intros; eapply Nat.le_trans; [eapply local_budget_implies_total_budget; eauto|eauto].
Qed.

Theorem shallow_cuts_emit_no_descendant_payload : forall body,
  SubstitutionRegionEmission.value_body SubstitutionRegionEmission.Shallow body = [].
Proof. reflexivity. Qed.

Theorem compact_cuts_emit_no_descendant_payload : forall body,
  SubstitutionRegionEmission.value_body SubstitutionRegionEmission.Compact body = [].
Proof. reflexivity. Qed.

Theorem original_cut_payload_cost_is_exact : forall body,
  length (SubstitutionRegionEmission.value_body SubstitutionRegionEmission.Original body) =
  length body.
Proof. reflexivity. Qed.

Print Assumptions accumulation_refines_reference.
Print Assumptions concatenation_adds_costs.
Print Assumptions prefix_cost_never_exceeds_complete_trace.
Print Assumptions shared_references_copy_no_body_bytes.
Print Assumptions disjoint_root_partition_copies_exact_source.
Print Assumptions one_root_copy_per_endpoint.
Print Assumptions multiplicity_change_preserves_input_work.
Print Assumptions no_finish_trace_has_zero_counters.
Print Assumptions successful_machine_has_one_arena_and_finish.
Print Assumptions prefinish_error_has_no_finish_attempt.
Print Assumptions identity_bypass_has_no_arena_or_finish.
Print Assumptions local_budget_implies_total_budget.
Print Assumptions declared_linear_partition_bounds_observed_cost.
Print Assumptions shallow_cuts_emit_no_descendant_payload.
Print Assumptions compact_cuts_emit_no_descendant_payload.
Print Assumptions original_cut_payload_cost_is_exact.

End SubstitutionCost.
