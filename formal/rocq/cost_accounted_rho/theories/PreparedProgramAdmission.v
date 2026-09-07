From Stdlib Require Import Lists.List ZArith.ZArith NArith.NArith Lia.
Import ListNotations.
Set Implicit Arguments.

Section Admission.
Context {Process Signature Random Source Environment Diagnostic FinalCost Merge Budget Space : Type}.
Variable empty_merge : Merge.

Inductive failure :=
| ParserFailure (diagnostic : Diagnostic)
| OperatorUndefined (diagnostic : Diagnostic)
| OperatorExpected (diagnostic : Diagnostic)
| AggregateFailure (members : list failure)
| LocatedFailure (inner : failure)
| OtherFailure (diagnostic : Diagnostic)
| InvalidInitialBudget (value : Z).

Inductive reported_cost :=
| InvalidBudgetCost
| PreparationCost
| FinalizedCost (cost : FinalCost).

Record evaluation := {
  evaluation_cost : reported_cost;
  evaluation_errors : list failure;
  evaluation_merge : Merge
}.

Definition reject (cost : reported_cost) (errors : list failure) : evaluation :=
  {| evaluation_cost := cost; evaluation_errors := errors;
     evaluation_merge := empty_merge |}.

Inductive preparation :=
| Prepared (process : Process)
| PreparationFailed (diagnostic : Diagnostic).

Inductive reduction :=
| Reduced
| ReductionFailed (error : failure).

Record prepared_program := { prepared_process : Process }.

Inductive signed_tree :=
| Sealed (process : Process) (signature : Signature)
| Funding (signature : Signature) (amount : N)
| Together (first second : signed_tree).

Definition metered (process : Process) (signature : Signature) (amount : N) :=
  Together (Sealed process signature) (Funding signature amount).

Fixpoint token_projection (tree : signed_tree) : option (Signature * N) :=
  match tree with
  | Sealed _ _ => None
  | Funding signature amount => Some (signature, amount)
  | Together first second =>
      match token_projection first with
      | Some token => Some token
      | None => token_projection second
      end
  end.

Fixpoint process_projection (tree : signed_tree) : option Process :=
  match tree with
  | Sealed process _ => Some process
  | Funding _ _ => None
  | Together first second =>
      match process_projection first with
      | Some process => Some process
      | None => process_projection second
      end
  end.

Theorem metered_token_exact : forall process signature amount,
  token_projection (metered process signature amount) = Some (signature, amount).
Proof. reflexivity. Qed.

Theorem metered_process_exact : forall process signature amount,
  process_projection (metered process signature amount) = Some process.
Proof. reflexivity. Qed.

Theorem nonnegative_budget_roundtrip : forall amount,
  (0 <= amount)%Z -> Z.of_N (Z.to_N amount) = amount.
Proof. intros amount H. apply Z2N.id. exact H. Qed.

Inductive program :=
| Return (result : evaluation)
| PrepareSource (source : Source) (environment : Environment)
    (next : preparation -> program)
| ReadSignature (next : Signature -> program)
| InitializeBudget (signature : Signature) (amount : N) (next : program)
| ClearMerge (next : program)
| InvokeReducer (process : Process) (random : Random) (next : reduction -> program)
| ReadMerge (next : Merge -> program)
| FinalizeCost (next : FinalCost -> program)
| InvariantAbort.

Definition finish_error (error : failure) : program :=
  match error with
  | ParserFailure _ | OperatorUndefined _ | OperatorExpected _ =>
      Return (reject PreparationCost [error])
  | AggregateFailure members =>
      FinalizeCost (fun cost => Return (reject (FinalizedCost cost) members))
  | _ => FinalizeCost (fun cost => Return (reject (FinalizedCost cost) [error]))
  end.

Definition finish_reduction (outcome : reduction) : program :=
  match outcome with
  | Reduced =>
      ReadMerge (fun merge =>
        FinalizeCost (fun cost =>
          Return {| evaluation_cost := FinalizedCost cost;
                    evaluation_errors := []; evaluation_merge := merge |}))
  | ReductionFailed error => finish_error error
  end.

Definition monolithic (source : Source) (environment : Environment)
    (initial : Z) (random : Random) : program :=
  if (initial <? 0)%Z then
    Return (reject InvalidBudgetCost [InvalidInitialBudget initial])
  else
    PrepareSource source environment (fun outcome =>
      match outcome with
      | PreparationFailed diagnostic =>
          Return (reject PreparationCost [ParserFailure diagnostic])
      | Prepared process =>
          ReadSignature (fun signature =>
            InitializeBudget signature (Z.to_N initial)
              (ClearMerge (InvokeReducer process random finish_reduction)))
      end).

Definition prepared_handoff (prepared : prepared_program) (initial : Z)
    (random : Random) : program :=
  if (initial <? 0)%Z then
    Return (reject InvalidBudgetCost [InvalidInitialBudget initial])
  else
    ReadSignature (fun signature =>
      let signed := metered (prepared_process prepared) signature (Z.to_N initial) in
      match token_projection signed, process_projection signed with
      | Some (signer, amount), Some process =>
          InitializeBudget signer amount
            (ClearMerge (InvokeReducer process random finish_reduction))
      | _, _ => InvariantAbort
      end).

Definition extracted (source : Source) (environment : Environment)
    (initial : Z) (random : Random) : program :=
  if (initial <? 0)%Z then
    Return (reject InvalidBudgetCost [InvalidInitialBudget initial])
  else
    PrepareSource source environment (fun outcome =>
      match outcome with
      | PreparationFailed diagnostic =>
          Return (reject PreparationCost [ParserFailure diagnostic])
      | Prepared process =>
          prepared_handoff {| prepared_process := process |} initial random
      end).

Theorem extracted_equals_monolithic : forall source environment initial random,
  extracted source environment initial random =
  monolithic source environment initial random.
Proof.
  intros. unfold extracted, monolithic, prepared_handoff.
  destruct (initial <? 0)%Z; reflexivity.
Qed.

Theorem negative_budget_rejects_before_preparation :
  forall source environment initial random,
  (initial < 0)%Z ->
  extracted source environment initial random =
    Return (reject InvalidBudgetCost [InvalidInitialBudget initial]).
Proof.
  intros. unfold extracted. apply Z.ltb_lt in H. rewrite H. reflexivity.
Qed.

Theorem prepared_handoff_exact_operations : forall prepared initial random,
  (0 <= initial)%Z ->
  prepared_handoff prepared initial random =
    ReadSignature (fun signature =>
      InitializeBudget signature (Z.to_N initial)
        (ClearMerge (InvokeReducer (prepared_process prepared) random finish_reduction))).
Proof.
  intros. unfold prepared_handoff.
  assert ((initial <? 0)%Z = false) by (apply Z.ltb_ge; exact H).
  rewrite H0. reflexivity.
Qed.

Inductive event :=
| SourcePrepared (source : Source) (environment : Environment)
| SignatureRead (signature : Signature)
| BudgetInitialized (signature : Signature) (amount : N)
| MergeCleared
| ReducerInvoked (process : Process) (random : Random)
| MergeRead (merge : Merge)
| CostFinalized (cost : FinalCost).

Inductive execution_trace : program -> list event -> evaluation -> Prop :=
| TraceReturn : forall result, execution_trace (Return result) [] result
| TracePrepare : forall source environment next response events result,
    execution_trace (next response) events result ->
    execution_trace (PrepareSource source environment next)
      (SourcePrepared source environment :: events) result
| TraceSignature : forall next response events result,
    execution_trace (next response) events result ->
    execution_trace (ReadSignature next) (SignatureRead response :: events) result
| TraceInitialize : forall signature amount next events result,
    execution_trace next events result ->
    execution_trace (InitializeBudget signature amount next)
      (BudgetInitialized signature amount :: events) result
| TraceClear : forall next events result,
    execution_trace next events result ->
    execution_trace (ClearMerge next) (MergeCleared :: events) result
| TraceReduce : forall process random next response events result,
    execution_trace (next response) events result ->
    execution_trace (InvokeReducer process random next)
      (ReducerInvoked process random :: events) result
| TraceRead : forall next response events result,
    execution_trace (next response) events result ->
    execution_trace (ReadMerge next) (MergeRead response :: events) result
| TraceFinalize : forall next response events result,
    execution_trace (next response) events result ->
    execution_trace (FinalizeCost next) (CostFinalized response :: events) result.

Theorem extracted_trace_equivalence :
  forall source environment initial random events result,
  execution_trace (extracted source environment initial random) events result <->
  execution_trace (monolithic source environment initial random) events result.
Proof. intros. rewrite extracted_equals_monolithic. reflexivity. Qed.

Definition finish_shape (events : list event) : Prop :=
  events = [] \/
  (exists cost, events = [CostFinalized cost]) \/
  (exists merge cost, events = [MergeRead merge; CostFinalized cost]).

Ltac finish_trace :=
  repeat match goal with
  | H : execution_trace (Return _) _ _ |- _ => inversion H; subst; clear H
  | H : execution_trace (ReadMerge _) _ _ |- _ => inversion H; subst; clear H
  | H : execution_trace (FinalizeCost _) _ _ |- _ => inversion H; subst; clear H
  end.

Theorem reduction_finishing_shape : forall outcome events result,
  execution_trace (finish_reduction outcome) events result -> finish_shape events.
Proof.
  intros outcome events result H.
  destruct outcome as [|error].
  - cbn [finish_reduction] in H. finish_trace.
    unfold finish_shape. eauto.
  - destruct error; cbn [finish_reduction finish_error] in H;
      finish_trace; unfold finish_shape; eauto.
Qed.

Theorem admitted_trace_shape : forall prepared initial random events result,
  (0 <= initial)%Z ->
  execution_trace (prepared_handoff prepared initial random) events result ->
  exists signature tail,
    events = SignatureRead signature :: BudgetInitialized signature (Z.to_N initial) ::
      MergeCleared :: ReducerInvoked (prepared_process prepared) random :: tail
    /\ finish_shape tail.
Proof.
  intros prepared initial random events result Hnonnegative Htrace.
  rewrite prepared_handoff_exact_operations in Htrace by exact Hnonnegative.
  inversion Htrace; subst; clear Htrace.
  match goal with H : execution_trace (InitializeBudget _ _ _) _ _ |- _ =>
    inversion H; subst; clear H end.
  match goal with H : execution_trace (ClearMerge _) _ _ |- _ =>
    inversion H; subst; clear H end.
  match goal with H : execution_trace (InvokeReducer _ _ _) _ _ |- _ =>
    inversion H; subst; clear H end.
  do 2 eexists. split; [reflexivity|].
  eapply reduction_finishing_shape. eassumption.
Qed.

Definition preparation_count (events : list event) : nat :=
  length (filter (fun e => match e with SourcePrepared _ _ => true | _ => false end) events).
Definition initialization_count (events : list event) : nat :=
  length (filter (fun e => match e with BudgetInitialized _ _ => true | _ => false end) events).
Definition invocation_count (events : list event) : nat :=
  length (filter (fun e => match e with ReducerInvoked _ _ => true | _ => false end) events).

Theorem admitted_once_without_reparse :
  forall prepared initial random events result,
  (0 <= initial)%Z ->
  execution_trace (prepared_handoff prepared initial random) events result ->
  preparation_count events = 0 /\ initialization_count events = 1 /\ invocation_count events = 1.
Proof.
  intros prepared initial random events result Hnonnegative Htrace.
  destruct (@admitted_trace_shape prepared initial random events result Hnonnegative Htrace)
    as [signature [tail [Hevents Htail]]].
  subst events. destruct Htail as [Hempty | [[cost Hcost] | [merge [cost Hcost]]]];
    subst tail; repeat split; reflexivity.
Qed.

Theorem source_trace_shape : forall source environment initial random events result,
  (0 <= initial)%Z ->
  execution_trace (extracted source environment initial random) events result ->
  exists tail,
    events = SourcePrepared source environment :: tail /\
    ((exists diagnostic, tail = [] /\
        result = reject PreparationCost [ParserFailure diagnostic]) \/
     (exists process, execution_trace
        (prepared_handoff {| prepared_process := process |} initial random) tail result)).
Proof.
  intros source environment initial random events result Hnonnegative Htrace.
  unfold extracted in Htrace.
  assert ((initial <? 0)%Z = false) by (apply Z.ltb_ge; exact Hnonnegative).
  rewrite H in Htrace. inversion Htrace; subst; clear Htrace.
  match goal with response : preparation |- _ => destruct response end.
  - eexists. split; [reflexivity|]. right. eexists. eassumption.
  - match goal with H : execution_trace (Return _) _ _ |- _ =>
      inversion H; subst; clear H end.
    exists []. split; [reflexivity|]. left. eexists. split; reflexivity.
Qed.

Theorem source_prepares_exactly_once : forall source environment initial random events result,
  (0 <= initial)%Z ->
  execution_trace (extracted source environment initial random) events result ->
  preparation_count events = 1.
Proof.
  intros source environment initial random events result Hnonnegative Htrace.
  destruct (@source_trace_shape source environment initial random events result
    Hnonnegative Htrace) as [tail [Hevents Htail]].
  subst events. destruct Htail as [[diagnostic [Hempty Hresult]] | [process Hhandoff]].
  - subst tail. reflexivity.
  - destruct (@admitted_once_without_reparse {| prepared_process := process |}
      initial random tail result Hnonnegative Hhandoff) as [Hcount _].
    change (S (preparation_count tail) = 1). rewrite Hcount. reflexivity.
Qed.

Theorem negative_source_has_no_events : forall source environment initial random events result,
  (initial < 0)%Z ->
  execution_trace (extracted source environment initial random) events result ->
  events = [].
Proof.
  intros source environment initial random events result Hnegative Htrace.
  rewrite negative_budget_rejects_before_preparation in Htrace by exact Hnegative.
  inversion Htrace. reflexivity.
Qed.

Theorem preparation_failure_no_admission : forall diagnostic events result,
  execution_trace (Return (reject PreparationCost [ParserFailure diagnostic])) events result ->
  events = [] /\ evaluation_cost result = PreparationCost
    /\ evaluation_merge result = empty_merge.
Proof. intros. inversion H; subst. repeat split; reflexivity. Qed.

Theorem empty_aggregate_finalizes_without_errors : forall cost,
  execution_trace (finish_error (AggregateFailure [])) [CostFinalized cost]
    (reject (FinalizedCost cost) []).
Proof. intros. constructor. constructor. Qed.

Theorem located_operator_uses_finalized_cost : forall diagnostic cost,
  execution_trace (finish_error (LocatedFailure (OperatorUndefined diagnostic)))
    [CostFinalized cost]
    (reject (FinalizedCost cost) [LocatedFailure (OperatorUndefined diagnostic)]).
Proof. intros. constructor. constructor. Qed.

Theorem direct_operator_has_zero_cost : forall diagnostic,
  finish_error (OperatorUndefined diagnostic) =
    Return (reject PreparationCost [OperatorUndefined diagnostic]).
Proof. reflexivity. Qed.

Inductive ownership_slot :=
| Ready (prepared : prepared_program)
| Consumed.
Inductive access := Borrow | Consume.

Inductive ownership_step : ownership_slot -> access -> ownership_slot -> Prop :=
| BorrowReady : forall prepared, ownership_step (Ready prepared) Borrow (Ready prepared)
| ConsumeReady : forall prepared, ownership_step (Ready prepared) Consume Consumed.

Inductive ownership_run : ownership_slot -> list access -> ownership_slot -> Prop :=
| OwnershipDone : forall slot, ownership_run slot [] slot
| OwnershipNext : forall first operation middle rest last,
    ownership_step first operation middle ->
    ownership_run middle rest last ->
    ownership_run first (operation :: rest) last.

Theorem borrowing_preserves_prepared : forall prepared next,
  ownership_step (Ready prepared) Borrow next -> next = Ready prepared.
Proof. intros. inversion H. reflexivity. Qed.

Theorem consuming_empties_slot : forall prepared next,
  ownership_step (Ready prepared) Consume next -> next = Consumed.
Proof. intros. inversion H. reflexivity. Qed.

Theorem consumed_cannot_step : forall operation next,
  ~ ownership_step Consumed operation next.
Proof. intros operation next H. inversion H. Qed.

Theorem consumed_run_is_empty : forall operations last,
  ownership_run Consumed operations last -> operations = [] /\ last = Consumed.
Proof.
  intros operations last H. inversion H; subst.
  - split; reflexivity.
  - exfalso. eapply consumed_cannot_step. eassumption.
Qed.

Definition consumption_count (operations : list access) :=
  length (filter (fun operation => match operation with Borrow => false | Consume => true end)
    operations).

Theorem prepared_is_consumed_at_most_once : forall first operations last,
  ownership_run first operations last -> consumption_count operations <= 1.
Proof.
  intros first operations last H. induction H.
  - cbn. lia.
  - inversion H; subst.
    + exact IHownership_run.
    + destruct (consumed_run_is_empty H0) as [Hempty _].
      subst rest. cbn. lia.
Qed.

Inductive prepared_dispatch : ownership_slot -> Z -> Random -> ownership_slot -> program -> Prop :=
| DispatchReady : forall prepared initial random,
    prepared_dispatch (Ready prepared) initial random Consumed
      (prepared_handoff prepared initial random).

Theorem dispatch_consumes_exact_artifact : forall prepared initial random next code,
  prepared_dispatch (Ready prepared) initial random next code ->
  next = Consumed /\ code = prepared_handoff prepared initial random.
Proof. intros. inversion H. split; reflexivity. Qed.

Theorem consumed_cannot_dispatch : forall initial random next code,
  ~ prepared_dispatch Consumed initial random next code.
Proof. intros initial random next code H. inversion H. Qed.

Theorem dispatched_execution_once : forall prepared initial random next code events result,
  (0 <= initial)%Z ->
  prepared_dispatch (Ready prepared) initial random next code ->
  execution_trace code events result ->
  next = Consumed /\ preparation_count events = 0 /\
    initialization_count events = 1 /\ invocation_count events = 1.
Proof.
  intros prepared initial random next code events result Hnonnegative Hdispatch Htrace.
  inversion Hdispatch; subst. split; [reflexivity|].
  eapply admitted_once_without_reparse; eassumption.
Qed.

Inductive runtime_result :=
| Evaluated (result : evaluation)
| Escaped (error : failure).

Record runtime_state := {
  state_budget : Budget;
  state_merge : Merge;
  state_space : Space
}.

Definition replay_event
    (host_operation : runtime_state -> event -> runtime_state)
    (state : runtime_state) (operation : event) :=
  match operation with
  | SourcePrepared _ _ => state
  | _ => host_operation state operation
  end.

Definition replay_trace (host_operation : runtime_state -> event -> runtime_state)
    (state : runtime_state) (events : list event) :=
  fold_left (replay_event host_operation) events state.

Theorem preparation_failure_preserves_host_state :
  forall host_operation state source environment,
  replay_trace host_operation state [SourcePrepared source environment] = state.
Proof. reflexivity. Qed.

Theorem no_operations_preserves_host_state : forall host_operation state,
  replay_trace host_operation state [] = state.
Proof. reflexivity. Qed.

Definition restore_space (checkpoint : Space) (state : runtime_state) :=
  {| state_budget := state_budget state; state_merge := state_merge state;
     state_space := checkpoint |}.

Definition finish_wrapper (checkpoint : Space) (state : runtime_state)
    (result : runtime_result) : runtime_state :=
  match result with
  | Evaluated evaluation =>
      match evaluation_errors evaluation with
      | [] => state
      | _ :: _ => restore_space checkpoint state
      end
  | Escaped _ => restore_space checkpoint state
  end.

Theorem wrapper_preserves_budget_and_merge : forall checkpoint state result,
  state_budget (finish_wrapper checkpoint state result) = state_budget state /\
  state_merge (finish_wrapper checkpoint state result) = state_merge state.
Proof.
  intros checkpoint state [result|error]; [destruct result as [cost errors merge]; destruct errors|];
    split; reflexivity.
Qed.

Theorem wrapper_restores_nonempty_errors : forall checkpoint state cost error rest merge,
  state_space (finish_wrapper checkpoint state
    (Evaluated {| evaluation_cost := cost; evaluation_errors := error :: rest;
                  evaluation_merge := merge |})) = checkpoint.
Proof. reflexivity. Qed.

Theorem wrapper_restores_outer_error : forall checkpoint state error,
  state_space (finish_wrapper checkpoint state (Escaped error)) = checkpoint.
Proof. reflexivity. Qed.

Theorem empty_aggregate_does_not_rollback : forall checkpoint state cost,
  finish_wrapper checkpoint state (Evaluated (reject (FinalizedCost cost) [])) = state.
Proof. reflexivity. Qed.

End Admission.

Print Assumptions metered_token_exact.
Print Assumptions metered_process_exact.
Print Assumptions nonnegative_budget_roundtrip.
Print Assumptions extracted_equals_monolithic.
Print Assumptions negative_budget_rejects_before_preparation.
Print Assumptions prepared_handoff_exact_operations.
Print Assumptions extracted_trace_equivalence.
Print Assumptions reduction_finishing_shape.
Print Assumptions admitted_trace_shape.
Print Assumptions admitted_once_without_reparse.
Print Assumptions source_trace_shape.
Print Assumptions source_prepares_exactly_once.
Print Assumptions negative_source_has_no_events.
Print Assumptions preparation_failure_no_admission.
Print Assumptions preparation_failure_preserves_host_state.
Print Assumptions no_operations_preserves_host_state.
Print Assumptions empty_aggregate_finalizes_without_errors.
Print Assumptions located_operator_uses_finalized_cost.
Print Assumptions direct_operator_has_zero_cost.
Print Assumptions borrowing_preserves_prepared.
Print Assumptions consuming_empties_slot.
Print Assumptions consumed_cannot_step.
Print Assumptions consumed_run_is_empty.
Print Assumptions prepared_is_consumed_at_most_once.
Print Assumptions dispatch_consumes_exact_artifact.
Print Assumptions consumed_cannot_dispatch.
Print Assumptions dispatched_execution_once.
Print Assumptions wrapper_preserves_budget_and_merge.
Print Assumptions wrapper_restores_nonempty_errors.
Print Assumptions wrapper_restores_outer_error.
Print Assumptions empty_aggregate_does_not_rollback.
