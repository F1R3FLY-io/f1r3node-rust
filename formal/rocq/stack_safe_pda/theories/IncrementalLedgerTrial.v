From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import GenerationCheckpointPublication.

Import ListNotations.

(**
  Failure-atomic bounded private ledger trials.

  A trial runs the two-phase cursor machine of [IncrementalReachability] on a
  private workspace: activation of the added root occurrences drains first,
  then deactivation of the removed ones.  Every transition is one of the
  actual [cursor_step] constructors, chosen by the same schedule rule as the
  executable model, and is charged before it is accepted.  The budgets are
  separate domains: the checked reference-count capacity, the temporary
  add-before-remove union inventory, peak live scratch, cumulative reference
  events, cumulative child-list expansions, externally quoted read bytes, and
  the final inventory.  A refused trial returns its predecessor unchanged; a
  drained trial returns prepared evidence, which carries no durable receipt or
  owner authority.

  The variants other than [Faithful] are negative controls.  Each is refuted
  by a closed counterexample theorem below.
*)

Module CheckedLedgerTrial.

Import IncrementalReachability.
Import CheckedReachability.

Record trial_environment := TrialEnvironment {
  graph : nat -> list nat;
  universe : list nat;
  object_bytes : nat -> nat;
  quoted_read : nat -> nat;
  consumed_read : nat -> nat
}.

Inductive budget_axis :=
| CountAxis
| UnionObjectsAxis
| UnionBytesAxis
| ScratchAxis
| ReferenceAxis
| ExpansionAxis
| QuotedBytesAxis
| FinalObjectsAxis
| FinalBytesAxis.

Definition budget_axis_eq_dec (left right : budget_axis) : {left = right} + {left <> right}.
Proof. decide equality. Defined.

Inductive trial_variant :=
| Faithful
| PartialCommit
| FinalOnUnion
| AllocationsAsScratch
| SkipContribution
| DuplicateContribution
| UnchargedInitialization.

Definition trial_limits := budget_axis -> nat.

Record trial_usage := TrialUsage {
  peak_count : nat;
  peak_objects : nat;
  peak_bytes : nat;
  peak_scratch : nat;
  references : nat;
  expansions : nat;
  quoted_bytes : nat;
  consumed_bytes : nat
}.

Inductive refusal :=
| BudgetRefusal (axis : budget_axis)
| UnauthorizedRemoval
| ZeroCountDecrement
| InventoryUnderflow
| PotentialExhausted.

Record predecessor := Predecessor {
  base_ledger : ledger;
  base_roots : counter;
  base_objects : nat;
  base_bytes : nat
}.

Record trial_state := TrialState {
  workspace : configuration;
  live_objects : nat;
  live_bytes : nat;
  usage : trial_usage
}.

Inductive attempt :=
| AttemptStep (next : trial_state)
| AttemptInvalid (why : refusal)
| AttemptDrained.

Inductive step_result :=
| StepAccepted (next : trial_state)
| StepRefused (why : refusal)
| StepDrained.

Inductive phase_result :=
| PhaseDrained (final : trial_state)
| PhaseRefused (why : refusal) (at_refusal : trial_state).

Inductive core_result :=
| CorePrepared (final : trial_state)
| CoreRefused (why : refusal) (at_refusal : trial_state).

Record prepared_evidence := PreparedEvidence {
  prepared_ledger : ledger;
  prepared_roots : counter;
  prepared_objects : nat;
  prepared_bytes : nat;
  prepared_usage : trial_usage
}.

Inductive trial_outcome :=
| TrialPrepared (evidence : prepared_evidence)
| TrialRefused (why : refusal).

Record trial_result := TrialResult {
  retained : predecessor;
  outcome : trial_outcome
}.

Definition unit_weight (_ : nat) : nat := 1.

Definition live_scratch (machine : configuration) : nat :=
  length (events machine) + length (frames machine).

Definition step_axes : list budget_axis :=
  [CountAxis; UnionObjectsAxis; UnionBytesAxis; ScratchAxis; ReferenceAxis;
   ExpansionAxis; QuotedBytesAxis].

Definition axis_limit (variant : trial_variant) (limits : trial_limits)
  (axis : budget_axis) : nat :=
  match variant, axis with
  | FinalOnUnion, UnionObjectsAxis => limits FinalObjectsAxis
  | FinalOnUnion, UnionBytesAxis => limits FinalBytesAxis
  | _, _ => limits axis
  end.

Definition step_usage (spent : trial_usage) (axis : budget_axis) : nat :=
  match axis with
  | CountAxis => peak_count spent
  | UnionObjectsAxis => peak_objects spent
  | UnionBytesAxis => peak_bytes spent
  | ScratchAxis => peak_scratch spent
  | ReferenceAxis => references spent
  | ExpansionAxis => expansions spent
  | QuotedBytesAxis => quoted_bytes spent
  | FinalObjectsAxis | FinalBytesAxis => 0
  end.

Definition axis_usage (variant : trial_variant) (spent : trial_usage)
  (axis : budget_axis) : nat :=
  match variant, axis with
  | AllocationsAsScratch, ScratchAxis => expansions spent
  | _, _ => step_usage spent axis
  end.

Definition exceeds (variant : trial_variant) (limits : trial_limits)
  (spent : trial_usage) (axis : budget_axis) : bool :=
  axis_limit variant limits axis <? axis_usage variant spent axis.

Definition first_exceeded (variant : trial_variant) (limits : trial_limits)
  (spent : trial_usage) : option budget_axis :=
  find (exceeds variant limits spent) step_axes.

Definition guard (variant : trial_variant) (budget : option trial_limits)
  (current : trial_state) : option budget_axis :=
  match budget with
  | None => None
  | Some limits => first_exceeded variant limits (usage current)
  end.

Definition final_exceeded (limits : trial_limits) (current : trial_state) :
  option budget_axis :=
  if limits FinalObjectsAxis <? live_objects current then Some FinalObjectsAxis
  else if limits FinalBytesAxis <? live_bytes current then Some FinalBytesAxis
  else None.

Definition final_guard (budget : option trial_limits) (current : trial_state) :
  option budget_axis :=
  match budget with
  | None => None
  | Some limits => final_exceeded limits current
  end.

Definition charge_reference (spent : trial_usage) : trial_usage :=
  TrialUsage (peak_count spent) (peak_objects spent) (peak_bytes spent)
    (peak_scratch spent) (S (references spent)) (expansions spent)
    (quoted_bytes spent) (consumed_bytes spent).

Definition charge_expansion (env : trial_environment) (vertex : nat)
  (spent : trial_usage) : trial_usage :=
  TrialUsage (peak_count spent) (peak_objects spent) (peak_bytes spent)
    (peak_scratch spent) (references spent) (S (expansions spent))
    (quoted_bytes spent + quoted_read env vertex)
    (consumed_bytes spent + consumed_read env vertex).

Definition with_peaks (count objects bytes : nat) (machine : configuration)
  (spent : trial_usage) : trial_usage :=
  TrialUsage (Nat.max (peak_count spent) count) (Nat.max (peak_objects spent) objects)
    (Nat.max (peak_bytes spent) bytes) (Nat.max (peak_scratch spent) (live_scratch machine))
    (references spent) (expansions spent) (quoted_bytes spent) (consumed_bytes spent).

Definition event_attempt (env : trial_environment) (mode : phase) (state : ledger)
  (vertex : nat) (rest : list nat) (cursors : list child_cursor) (objects bytes : nat)
  (spent : trial_usage) : attempt :=
  match mode with
  | Activation =>
      AttemptStep (TrialState
        (Configuration (increment state vertex) rest
          (increment_cursors (graph env) state vertex cursors))
        (objects + activated_weight state vertex unit_weight)
        (bytes + activated_weight state vertex (object_bytes env))
        (with_peaks (S (counts state vertex))
          (objects + activated_weight state vertex unit_weight)
          (bytes + activated_weight state vertex (object_bytes env))
          (Configuration (increment state vertex) rest
            (increment_cursors (graph env) state vertex cursors))
          (if counts state vertex =? 0 then charge_expansion env vertex spent else spent)))
  | Deactivation =>
      if counts state vertex =? 0 then AttemptInvalid ZeroCountDecrement else
      match checked_sub objects (deactivated_weight state vertex unit_weight),
            checked_sub bytes (deactivated_weight state vertex (object_bytes env)) with
      | Some next_objects, Some next_bytes =>
          AttemptStep (TrialState
            (Configuration (decrement state vertex) rest
              (decrement_cursors (graph env) state vertex cursors))
            next_objects next_bytes
            (with_peaks 0 next_objects next_bytes
              (Configuration (decrement state vertex) rest
                (decrement_cursors (graph env) state vertex cursors))
              (if counts state vertex =? 1 then charge_expansion env vertex spent else spent)))
      | _, _ => AttemptInvalid InventoryUnderflow
      end
  end.

Definition transferred_events (variant : trial_variant) (child : nat)
  (explicit : list nat) : list nat :=
  match variant with
  | SkipContribution => explicit
  | DuplicateContribution => child :: child :: explicit
  | _ => child :: explicit
  end.

Definition cursor_attempt (variant : trial_variant) (state : ledger)
  (explicit : list nat) (parent : nat) (remaining : list nat)
  (cursors : list child_cursor) (objects bytes : nat) (spent : trial_usage) : attempt :=
  match remaining with
  | [] =>
      AttemptStep (TrialState (Configuration state explicit cursors) objects bytes
        (with_peaks 0 objects bytes (Configuration state explicit cursors) spent))
  | child :: rest =>
      AttemptStep (TrialState
        (Configuration state (transferred_events variant child explicit)
          ((parent, rest) :: cursors))
        objects bytes
        (with_peaks 0 objects bytes
          (Configuration state (transferred_events variant child explicit)
            ((parent, rest) :: cursors))
          (charge_reference spent)))
  end.

Definition attempt_step (env : trial_environment) (variant : trial_variant)
  (mode : phase) (prefer : bool) (current : trial_state) : attempt :=
  match current with
  | TrialState (Configuration state explicit cursors) objects bytes spent =>
      match cursors, explicit with
      | (parent, remaining) :: suspended, [] =>
          cursor_attempt variant state [] parent remaining suspended objects bytes spent
      | (parent, remaining) :: suspended, vertex :: rest =>
          if prefer
          then cursor_attempt variant state explicit parent remaining suspended
            objects bytes spent
          else event_attempt env mode state vertex rest cursors objects bytes spent
      | [], vertex :: rest => event_attempt env mode state vertex rest [] objects bytes spent
      | [], [] => AttemptDrained
      end
  end.

Definition checked_step (env : trial_environment) (variant : trial_variant)
  (budget : option trial_limits) (mode : phase) (prefer : bool)
  (current : trial_state) : step_result :=
  match attempt_step env variant mode prefer current with
  | AttemptStep next =>
      match guard variant budget next with
      | None => StepAccepted next
      | Some axis => StepRefused (BudgetRefusal axis)
      end
  | AttemptInvalid why => StepRefused why
  | AttemptDrained => StepDrained
  end.

Definition is_drained (machine : configuration) : bool :=
  match events machine, frames machine with
  | [], [] => true
  | _, _ => false
  end.

Fixpoint run_phase (env : trial_environment) (variant : trial_variant)
  (budget : option trial_limits) (schedule : nat -> bool) (mode : phase)
  (fuel index : nat) (current : trial_state) : phase_result :=
  match fuel with
  | 0 =>
      if is_drained (workspace current) then PhaseDrained current
      else PhaseRefused PotentialExhausted current
  | S remaining =>
      match checked_step env variant budget mode (schedule index) current with
      | StepAccepted next =>
          run_phase env variant budget schedule mode remaining (S index) next
      | StepRefused why => PhaseRefused why current
      | StepDrained => PhaseDrained current
      end
  end.

Definition charges_initialization (variant : trial_variant) : bool :=
  match variant with
  | UnchargedInitialization => false
  | _ => true
  end.

Definition activation_start (variant : trial_variant) (base : predecessor)
  (added : list nat) : trial_state :=
  TrialState (Configuration (base_ledger base) added []) (base_objects base) (base_bytes base)
    (TrialUsage 0 (base_objects base) (base_bytes base)
      (if charges_initialization variant
       then live_scratch (Configuration (base_ledger base) added []) else 0)
      (if charges_initialization variant then length added else 0) 0 0 0).

Definition deactivation_start (variant : trial_variant) (middle : trial_state)
  (removed : list nat) : trial_state :=
  TrialState (Configuration (resident (workspace middle)) removed [])
    (live_objects middle) (live_bytes middle)
    (TrialUsage (peak_count (usage middle)) (peak_objects (usage middle))
      (peak_bytes (usage middle))
      (if charges_initialization variant
       then Nat.max (peak_scratch (usage middle))
         (live_scratch (Configuration (resident (workspace middle)) removed []))
       else peak_scratch (usage middle))
      (if charges_initialization variant
       then references (usage middle) + length removed else references (usage middle))
      (expansions (usage middle)) (quoted_bytes (usage middle))
      (consumed_bytes (usage middle))).

Definition authorized (roots : counter) (removed : list nat) : bool :=
  forallb (fun vertex => occurrences removed vertex <=? roots vertex) removed.

Definition run_removal (env : trial_environment) (variant : trial_variant)
  (budget : option trial_limits) (schedule : nat -> bool) (removed : list nat)
  (middle : trial_state) : core_result :=
  match guard variant budget (deactivation_start variant middle removed) with
  | Some axis => CoreRefused (BudgetRefusal axis) (deactivation_start variant middle removed)
  | None =>
      match run_phase env variant budget schedule Deactivation
        (remaining_work (graph env) Deactivation (universe env)
          (workspace (deactivation_start variant middle removed))) 0
        (deactivation_start variant middle removed) with
      | PhaseRefused why at_refusal => CoreRefused why at_refusal
      | PhaseDrained final =>
          match final_guard budget final with
          | Some axis => CoreRefused (BudgetRefusal axis) final
          | None => CorePrepared final
          end
      end
  end.

Definition run_core (env : trial_environment) (variant : trial_variant)
  (budget : option trial_limits) (schedule : nat -> bool) (base : predecessor)
  (added removed : list nat) : core_result :=
  if authorized (add_roots (base_roots base) added) removed then
    match guard variant budget (activation_start variant base added) with
    | Some axis => CoreRefused (BudgetRefusal axis) (activation_start variant base added)
    | None =>
        match run_phase env variant budget schedule Activation
          (remaining_work (graph env) Activation (universe env)
            (workspace (activation_start variant base added))) 0
          (activation_start variant base added) with
        | PhaseRefused why at_refusal => CoreRefused why at_refusal
        | PhaseDrained middle => run_removal env variant budget schedule removed middle
        end
    end
  else CoreRefused UnauthorizedRemoval (activation_start variant base added).

Definition partial_commit (base : predecessor) (at_refusal : trial_state) : predecessor :=
  Predecessor (resident (workspace at_refusal)) (base_roots base)
    (live_objects at_refusal) (live_bytes at_refusal).

Definition evidence_of (base : predecessor) (added removed : list nat)
  (final : trial_state) : prepared_evidence :=
  PreparedEvidence (resident (workspace final))
    (remove_roots (add_roots (base_roots base) added) removed)
    (live_objects final) (live_bytes final) (usage final).

Definition run_trial (env : trial_environment) (variant : trial_variant)
  (budget : option trial_limits) (schedule : nat -> bool) (base : predecessor)
  (added removed : list nat) : trial_result :=
  match run_core env variant budget schedule base added removed with
  | CorePrepared final => TrialResult base (TrialPrepared (evidence_of base added removed final))
  | CoreRefused why at_refusal =>
      match variant with
      | PartialCommit => TrialResult (partial_commit base at_refusal) (TrialRefused why)
      | _ => TrialResult base (TrialRefused why)
      end
  end.

Local Notation trial env budget schedule base added removed :=
  (run_trial env Faithful budget schedule base added removed).

Definition edges_closed (env : trial_environment) : Prop :=
  forall parent child, In parent (universe env) -> In child (graph env parent) ->
    In child (universe env).

Definition edges_descend (env : trial_environment) (rank : nat -> nat) : Prop :=
  forall parent child, In parent (universe env) -> In child (graph env parent) ->
    rank child < rank parent.

Definition exact_counters (env : trial_environment) (current : trial_state) : Prop :=
  live_objects current = inventory (universe env) (resident (workspace current)) unit_weight /\
  live_bytes current =
    inventory (universe env) (resident (workspace current)) (object_bytes env).

Definition exact_predecessor (env : trial_environment) (base : predecessor) : Prop :=
  base_objects base = inventory (universe env) (base_ledger base) unit_weight /\
  base_bytes base = inventory (universe env) (base_ledger base) (object_bytes env).

Definition trial_input_valid (env : trial_environment) (base : predecessor)
  (added removed : list nat) : Prop :=
  NoDup (universe env) /\ edges_closed env /\ coherent (base_ledger base) /\
  quiescent (graph env) (universe env) (base_roots base) (base_ledger base) /\
  exact_predecessor env base /\
  Forall (fun vertex => In vertex (universe env)) added /\
  Forall (fun vertex => In vertex (universe env)) removed.

Definition usage_le (before after : trial_usage) : Prop :=
  peak_count before <= peak_count after /\ peak_objects before <= peak_objects after /\
  peak_bytes before <= peak_bytes after /\ peak_scratch before <= peak_scratch after /\
  references before <= references after /\ expansions before <= expansions after /\
  quoted_bytes before <= quoted_bytes after /\ consumed_bytes before <= consumed_bytes after.

Definition measured (evidence : prepared_evidence) (axis : budget_axis) : nat :=
  match axis with
  | FinalObjectsAxis => prepared_objects evidence
  | FinalBytesAxis => prepared_bytes evidence
  | _ => step_usage (prepared_usage evidence) axis
  end.

Definition exact_limits (evidence : prepared_evidence) : trial_limits := measured evidence.

Definition with_limit (limits : trial_limits) (axis : budget_axis) (value : nat) :
  trial_limits :=
  fun query => if budget_axis_eq_dec query axis then value else limits query.

Definition limits_cover (limits : trial_limits) (evidence : prepared_evidence) : Prop :=
  forall axis, measured evidence axis <= limits axis.

Lemma first_exceeded_none_iff : forall variant limits spent,
  first_exceeded variant limits spent = None <->
  forall axis, In axis step_axes ->
    axis_usage variant spent axis <= axis_limit variant limits axis.
Proof.
  intros variant limits spent. unfold first_exceeded. split.
  - intros missing axis member.
    pose proof (@find_none _ (exceeds variant limits spent) step_axes missing axis member)
      as quiet.
    unfold exceeds in quiet. apply Nat.ltb_ge in quiet. exact quiet.
  - intros bounded.
    destruct (find (exceeds variant limits spent) step_axes) as [axis|] eqn:found;
      [|reflexivity].
    apply find_some in found. destruct found as [member above].
    unfold exceeds in above. apply Nat.ltb_lt in above.
    specialize (bounded axis member). lia.
Qed.

Lemma first_exceeded_some : forall variant limits spent axis,
  first_exceeded variant limits spent = Some axis ->
  In axis step_axes /\ exceeds variant limits spent axis = true.
Proof. intros variant limits spent axis found. now apply find_some in found. Qed.

Lemma usage_le_refl : forall spent, usage_le spent spent.
Proof. intros spent. unfold usage_le. lia. Qed.

Lemma usage_le_trans : forall first second third,
  usage_le first second -> usage_le second third -> usage_le first third.
Proof. unfold usage_le. intros first second third left right. lia. Qed.

Lemma axis_usage_monotone : forall variant before after axis,
  usage_le before after -> axis_usage variant before axis <= axis_usage variant after axis.
Proof.
  intros variant before after axis bounded. unfold usage_le in bounded.
  destruct variant, axis; cbn [axis_usage step_usage]; lia.
Qed.

Lemma exceeds_monotone : forall variant limits before after axis,
  usage_le before after -> exceeds variant limits before axis = true ->
  exceeds variant limits after axis = true.
Proof.
  intros variant limits before after axis bounded above. unfold exceeds in *.
  apply Nat.ltb_lt in above. apply Nat.ltb_lt.
  pose proof (axis_usage_monotone variant before after axis bounded). lia.
Qed.

Lemma first_exceeded_antitone : forall variant limits before after,
  usage_le before after -> first_exceeded variant limits after = None ->
  first_exceeded variant limits before = None.
Proof.
  intros variant limits before after bounded quiet.
  rewrite first_exceeded_none_iff in quiet |- *. intros axis member.
  specialize (quiet axis member).
  pose proof (axis_usage_monotone variant before after axis bounded). lia.
Qed.

Lemma attempt_step_cases : forall env variant mode prefer current next,
  attempt_step env variant mode prefer current = AttemptStep next ->
  (exists vertex rest, events (workspace current) = vertex :: rest /\
     event_attempt env mode (resident (workspace current)) vertex rest
       (frames (workspace current)) (live_objects current) (live_bytes current)
       (usage current) = AttemptStep next) \/
  (exists parent remaining suspended,
     frames (workspace current) = (parent, remaining) :: suspended /\
     cursor_attempt variant (resident (workspace current)) (events (workspace current))
       parent remaining suspended (live_objects current) (live_bytes current)
       (usage current) = AttemptStep next).
Proof.
  intros env variant mode prefer [[state explicit cursors] objects bytes spent] next step.
  cbn [attempt_step workspace resident events frames live_objects live_bytes usage] in *.
  destruct cursors as [|[parent remaining] suspended]; destruct explicit as [|vertex rest].
  - discriminate.
  - left. exists vertex, rest. split; [reflexivity|exact step].
  - right. exists parent, remaining, suspended. split; [reflexivity|exact step].
  - destruct prefer.
    + right. exists parent, remaining, suspended. split; [reflexivity|exact step].
    + left. exists vertex, rest. split; [reflexivity|exact step].
Qed.

Lemma event_attempt_usage_monotone :
  forall env mode state vertex rest cursors objects bytes spent next,
  event_attempt env mode state vertex rest cursors objects bytes spent = AttemptStep next ->
  usage_le spent (usage next).
Proof.
  intros env mode state vertex rest cursors objects bytes spent next step.
  destruct mode; cbn [event_attempt] in step.
  - injection step as <-.
    destruct (counts state vertex =? 0); cbn; unfold usage_le; cbn; lia.
  - destruct (counts state vertex =? 0); [discriminate|].
    destruct (checked_sub objects (deactivated_weight state vertex unit_weight))
      as [next_objects|]; [|discriminate].
    destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)))
      as [next_bytes|]; [|discriminate].
    injection step as <-.
    destruct (counts state vertex =? 1); cbn; unfold usage_le; cbn; lia.
Qed.

Lemma cursor_attempt_usage_monotone :
  forall variant state explicit parent remaining suspended objects bytes spent next,
  cursor_attempt variant state explicit parent remaining suspended objects bytes spent =
    AttemptStep next ->
  usage_le spent (usage next).
Proof.
  intros variant state explicit parent remaining suspended objects bytes spent next step.
  destruct remaining as [|child rest]; cbn [cursor_attempt] in step; injection step as <-;
    cbn; unfold usage_le; cbn; lia.
Qed.

Lemma attempt_usage_monotone : forall env variant mode prefer current next,
  attempt_step env variant mode prefer current = AttemptStep next ->
  usage_le (usage current) (usage next).
Proof.
  intros env variant mode prefer current next step.
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [_ step]]]|[parent [remaining [suspended [_ step]]]]].
  - eapply event_attempt_usage_monotone; exact step.
  - eapply cursor_attempt_usage_monotone; exact step.
Qed.

Lemma attempt_sound : forall env mode prefer current next,
  attempt_step env Faithful mode prefer current = AttemptStep next ->
  cursor_step (graph env) mode (workspace current) (workspace next).
Proof.
  intros env mode prefer [[state explicit cursors] objects bytes spent] next step.
  cbn [attempt_step workspace] in *.
  destruct cursors as [|[parent remaining] suspended]; destruct explicit as [|vertex rest].
  - discriminate.
  - destruct mode; cbn [event_attempt] in step.
    + injection step as <-. cbn [workspace]. apply activate_event.
    + destruct (counts state vertex =? 0); [discriminate|].
      destruct (checked_sub objects (deactivated_weight state vertex unit_weight));
        [|discriminate].
      destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)));
        [|discriminate].
      injection step as <-. cbn [workspace]. apply deactivate_event.
  - destruct remaining as [|child tail]; cbn [cursor_attempt transferred_events] in step;
      injection step as <-; cbn [workspace]; constructor.
  - destruct prefer.
    + destruct remaining as [|child tail]; cbn [cursor_attempt transferred_events] in step;
        injection step as <-; cbn [workspace]; constructor.
    + destruct mode; cbn [event_attempt] in step.
      * injection step as <-. cbn [workspace]. apply activate_event.
      * destruct (counts state vertex =? 0); [discriminate|].
        destruct (checked_sub objects (deactivated_weight state vertex unit_weight));
          [|discriminate].
        destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)));
          [|discriminate].
        injection step as <-. cbn [workspace]. apply deactivate_event.
Qed.

Lemma deactivation_event_is_funded :
  forall env roots state vertex rest cursors objects bytes,
  NoDup (universe env) ->
  valid_configuration (graph env) Deactivation (universe env) roots
    (Configuration state (vertex :: rest) cursors) ->
  objects = inventory (universe env) state unit_weight ->
  bytes = inventory (universe env) state (object_bytes env) ->
  (counts state vertex =? 0) = false /\
  (exists next_objects,
     checked_sub objects (deactivated_weight state vertex unit_weight) = Some next_objects) /\
  (exists next_bytes,
     checked_sub bytes (deactivated_weight state vertex (object_bytes env)) =
       Some next_bytes).
Proof.
  intros env roots state vertex rest cursors objects bytes unique valid exact_objects
    exact_bytes.
  destruct valid as [agreement [balance [event_domain _]]].
  cbn [phase_balance resident events frames] in agreement, balance, event_domain.
  pose proof (pending_decrement_cannot_underflow balance) as positive.
  apply Forall_inv in event_domain.
  split; [apply Nat.eqb_neq; lia|]. subst objects bytes. split.
  - exact (exact_inventory_removal_cannot_underflow vertex unit_weight unique event_domain
      agreement).
  - exact (exact_inventory_removal_cannot_underflow vertex (object_bytes env) unique
      event_domain agreement).
Qed.

Lemma attempt_complete : forall env mode roots current next,
  NoDup (universe env) ->
  valid_configuration (graph env) mode (universe env) roots (workspace current) ->
  exact_counters env current ->
  cursor_step (graph env) mode (workspace current) next ->
  exists prefer following,
    attempt_step env Faithful mode prefer current = AttemptStep following /\
    workspace following = next.
Proof.
  intros env mode roots [machine objects bytes spent] next unique valid exact transition.
  destruct exact as [exact_objects exact_bytes].
  cbn [workspace live_objects live_bytes] in valid, transition, exact_objects, exact_bytes.
  revert valid exact_objects exact_bytes.
  destruct transition as [state vertex rest cursors|state vertex rest cursors|
    mode state explicit parent child remaining cursors|mode state explicit parent cursors];
    intros valid exact_objects exact_bytes; cbn [resident] in exact_objects, exact_bytes.
  - exists false. cbn [attempt_step].
    destruct cursors as [|[parent remaining] suspended]; cbn [event_attempt];
      eexists; split; reflexivity.
  - destruct (deactivation_event_is_funded env roots state vertex rest cursors objects bytes
      unique valid exact_objects exact_bytes)
      as [nonzero [[next_objects removal_objects] [next_bytes removal_bytes]]].
    exists false. cbn [attempt_step].
    destruct cursors as [|[parent remaining] suspended]; cbn [event_attempt];
      rewrite nonzero, removal_objects, removal_bytes; eexists; split; reflexivity.
  - exists true. cbn [attempt_step].
    destruct explicit as [|vertex rest]; cbn [cursor_attempt transferred_events];
      eexists; split; reflexivity.
  - exists true. cbn [attempt_step].
    destruct explicit as [|vertex rest]; cbn [cursor_attempt];
      eexists; split; reflexivity.
Qed.

Lemma attempt_never_invalid : forall env mode roots prefer current why,
  NoDup (universe env) ->
  valid_configuration (graph env) mode (universe env) roots (workspace current) ->
  exact_counters env current ->
  attempt_step env Faithful mode prefer current <> AttemptInvalid why.
Proof.
  intros env mode roots prefer [[state explicit cursors] objects bytes spent] why unique valid
    exact.
  destruct exact as [exact_objects exact_bytes].
  cbn [workspace live_objects live_bytes resident] in valid, exact_objects, exact_bytes.
  cbn [attempt_step].
  destruct cursors as [|[parent remaining] suspended]; destruct explicit as [|vertex rest].
  - discriminate.
  - destruct mode; cbn [event_attempt]; [discriminate|].
    destruct (deactivation_event_is_funded env roots state vertex rest [] objects bytes
      unique valid exact_objects exact_bytes)
      as [nonzero [[next_objects removal_objects] [next_bytes removal_bytes]]].
    rewrite nonzero, removal_objects, removal_bytes. discriminate.
  - destruct remaining; cbn [cursor_attempt]; discriminate.
  - destruct prefer; [destruct remaining; cbn [cursor_attempt]; discriminate|].
    destruct mode; cbn [event_attempt]; [discriminate|].
    destruct (deactivation_event_is_funded env roots state vertex rest
      ((parent, remaining) :: suspended) objects bytes unique valid exact_objects exact_bytes)
      as [nonzero [[next_objects removal_objects] [next_bytes removal_bytes]]].
    rewrite nonzero, removal_objects, removal_bytes. discriminate.
Qed.

Lemma event_attempt_exact : forall env mode state vertex rest cursors objects bytes spent next,
  NoDup (universe env) -> In vertex (universe env) -> coherent state ->
  objects = inventory (universe env) state unit_weight ->
  bytes = inventory (universe env) state (object_bytes env) ->
  event_attempt env mode state vertex rest cursors objects bytes spent = AttemptStep next ->
  exact_counters env next.
Proof.
  intros env mode state vertex rest cursors objects bytes spent next unique member agreement
    exact_objects exact_bytes step.
  destruct mode; cbn [event_attempt] in step.
  - injection step as <-. unfold exact_counters. cbn [live_objects live_bytes workspace resident].
    rewrite (increment_changes_unique_inventory vertex unit_weight unique member agreement).
    rewrite (increment_changes_unique_inventory vertex (object_bytes env) unique member
      agreement).
    subst objects bytes. split; reflexivity.
  - destruct (counts state vertex =? 0); [discriminate|].
    destruct (checked_sub objects (deactivated_weight state vertex unit_weight))
      as [next_objects|] eqn:removal_objects; [|discriminate].
    destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)))
      as [next_bytes|] eqn:removal_bytes; [|discriminate].
    injection step as <-. unfold exact_counters. cbn [live_objects live_bytes workspace resident].
    split.
    + exact (checked_inventory_remove_is_exact vertex unit_weight unique member agreement
        exact_objects removal_objects).
    + exact (checked_inventory_remove_is_exact vertex (object_bytes env) unique member
        agreement exact_bytes removal_bytes).
Qed.

Lemma attempt_exact : forall env mode roots prefer current next,
  NoDup (universe env) ->
  valid_configuration (graph env) mode (universe env) roots (workspace current) ->
  exact_counters env current ->
  attempt_step env Faithful mode prefer current = AttemptStep next ->
  exact_counters env next.
Proof.
  intros env mode roots prefer current next unique valid [exact_objects exact_bytes] step.
  destruct valid as [agreement [_ [event_domain _]]].
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [shape step]]]|[parent [remaining [suspended [_ step]]]]].
  - rewrite shape in event_domain. apply Forall_inv in event_domain.
    eapply event_attempt_exact; eassumption.
  - destruct remaining; cbn [cursor_attempt] in step; injection step as <-;
      split; assumption.
Qed.

Lemma is_drained_iff : forall machine, is_drained machine = true <-> drained machine.
Proof.
  intros [state explicit cursors]. unfold is_drained, drained. cbn [events frames].
  destruct explicit as [|vertex rest]; destruct cursors as [|cursor suspended].
  - split; [intros _; split; reflexivity|intros _; reflexivity].
  - split; [discriminate|intros [_ empty]; discriminate].
  - split; [discriminate|intros [empty _]; discriminate].
  - split; [discriminate|intros [empty _]; discriminate].
Qed.

Lemma event_attempt_is_not_drained :
  forall env mode state vertex rest cursors objects bytes spent,
  event_attempt env mode state vertex rest cursors objects bytes spent <> AttemptDrained.
Proof.
  intros env mode state vertex rest cursors objects bytes spent.
  destruct mode; cbn [event_attempt]; [discriminate|].
  destruct (counts state vertex =? 0); [discriminate|].
  destruct (checked_sub objects (deactivated_weight state vertex unit_weight));
    [|discriminate].
  destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)));
    discriminate.
Qed.

Lemma attempt_drained_iff : forall env variant mode prefer current,
  attempt_step env variant mode prefer current = AttemptDrained <-> drained (workspace current).
Proof.
  intros env variant mode prefer [[state explicit cursors] objects bytes spent].
  unfold drained. cbn [attempt_step workspace events frames].
  destruct cursors as [|[parent remaining] suspended]; destruct explicit as [|vertex rest].
  - split; [intros _; split; reflexivity|intros _; reflexivity].
  - split; [|intros [empty _]; discriminate].
    intros drained_event. exfalso. exact (event_attempt_is_not_drained _ _ _ _ _ _ _ _ _
      drained_event).
  - split; [|intros [_ empty]; discriminate].
    destruct remaining; cbn [cursor_attempt]; discriminate.
  - split; [|intros [empty _]; discriminate].
    destruct prefer; [destruct remaining; cbn [cursor_attempt]; discriminate|].
    intros drained_event. exfalso. exact (event_attempt_is_not_drained _ _ _ _ _ _ _ _ _
      drained_event).
Qed.

Lemma attempt_invalid_reason : forall env variant mode prefer current why,
  attempt_step env variant mode prefer current = AttemptInvalid why ->
  why = ZeroCountDecrement \/ why = InventoryUnderflow.
Proof.
  intros env variant mode prefer [[state explicit cursors] objects bytes spent] why invalid.
  cbn [attempt_step] in invalid.
  assert (event_reason : forall vertex rest cursors,
    event_attempt env mode state vertex rest cursors objects bytes spent = AttemptInvalid why ->
    why = ZeroCountDecrement \/ why = InventoryUnderflow).
  { intros vertex rest cursors' event. destruct mode; cbn [event_attempt] in event;
      [discriminate|].
    destruct (counts state vertex =? 0); [injection event as <-; left; reflexivity|].
    destruct (checked_sub objects (deactivated_weight state vertex unit_weight));
      [|injection event as <-; right; reflexivity].
    destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)));
      [discriminate|injection event as <-; right; reflexivity]. }
  destruct cursors as [|[parent remaining] suspended]; destruct explicit as [|vertex rest].
  - discriminate.
  - exact (event_reason vertex rest [] invalid).
  - destruct remaining; cbn [cursor_attempt] in invalid; discriminate.
  - destruct prefer; [destruct remaining; cbn [cursor_attempt] in invalid; discriminate|].
    exact (event_reason vertex rest _ invalid).
Qed.

Lemma attempt_live_scratch_reserved : forall env variant mode prefer current next,
  attempt_step env variant mode prefer current = AttemptStep next ->
  live_scratch (workspace next) <= peak_scratch (usage next).
Proof.
  intros env variant mode prefer current next step.
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [_ step]]]|[parent [remaining [suspended [_ step]]]]].
  - destruct mode; cbn [event_attempt] in step.
    + injection step as <-. cbn [workspace usage with_peaks peak_scratch]. lia.
    + destruct (counts (resident (workspace current)) vertex =? 0); [discriminate|].
      destruct (checked_sub (live_objects current)
        (deactivated_weight (resident (workspace current)) vertex unit_weight));
        [|discriminate].
      destruct (checked_sub (live_bytes current)
        (deactivated_weight (resident (workspace current)) vertex (object_bytes env)));
        [|discriminate].
      injection step as <-. cbn [workspace usage with_peaks peak_scratch]. lia.
  - destruct remaining; cbn [cursor_attempt] in step; injection step as <-;
      cbn [workspace usage with_peaks peak_scratch]; lia.
Qed.

Lemma activation_attempt_tracks_peak : forall env prefer current next,
  peak_objects (usage current) = live_objects current ->
  peak_bytes (usage current) = live_bytes current ->
  attempt_step env Faithful Activation prefer current = AttemptStep next ->
  peak_objects (usage next) = live_objects next /\ peak_bytes (usage next) = live_bytes next.
Proof.
  intros env prefer current next peak_objects_live peak_bytes_live step.
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [_ step]]]|[parent [remaining [suspended [_ step]]]]].
  - cbn [event_attempt] in step. injection step as <-.
    cbn [usage live_objects live_bytes with_peaks peak_objects peak_bytes].
    destruct (counts (resident (workspace current)) vertex =? 0);
      cbn [charge_expansion peak_objects peak_bytes]; lia.
  - destruct remaining; cbn [cursor_attempt] in step; injection step as <-;
      cbn [usage live_objects live_bytes with_peaks peak_objects peak_bytes charge_reference];
      lia.
Qed.

Lemma deactivation_attempt_keeps_peak : forall env prefer current next,
  live_objects current <= peak_objects (usage current) ->
  live_bytes current <= peak_bytes (usage current) ->
  attempt_step env Faithful Deactivation prefer current = AttemptStep next ->
  peak_objects (usage next) = peak_objects (usage current) /\
  peak_bytes (usage next) = peak_bytes (usage current) /\
  live_objects next <= peak_objects (usage next) /\ live_bytes next <= peak_bytes (usage next).
Proof.
  intros env prefer current next objects_bounded bytes_bounded step.
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [_ step]]]|[parent [remaining [suspended [_ step]]]]].
  - cbn [event_attempt] in step.
    destruct (counts (resident (workspace current)) vertex =? 0); [discriminate|].
    destruct (checked_sub (live_objects current)
      (deactivated_weight (resident (workspace current)) vertex unit_weight))
      as [next_objects|] eqn:removal_objects; [|discriminate].
    destruct (checked_sub (live_bytes current)
      (deactivated_weight (resident (workspace current)) vertex (object_bytes env)))
      as [next_bytes|] eqn:removal_bytes; [|discriminate].
    apply checked_sub_is_exact in removal_objects.
    apply checked_sub_is_exact in removal_bytes.
    injection step as <-.
    cbn [usage live_objects live_bytes with_peaks peak_objects peak_bytes].
    destruct (counts (resident (workspace current)) vertex =? 1);
      cbn [charge_expansion peak_objects peak_bytes]; lia.
  - destruct remaining; cbn [cursor_attempt] in step; injection step as <-;
      cbn [usage live_objects live_bytes with_peaks peak_objects peak_bytes charge_reference];
      lia.
Qed.

Lemma attempt_respects_sound_quotes : forall env variant mode prefer current next,
  (forall vertex, consumed_read env vertex <= quoted_read env vertex) ->
  consumed_bytes (usage current) <= quoted_bytes (usage current) ->
  attempt_step env variant mode prefer current = AttemptStep next ->
  consumed_bytes (usage next) <= quoted_bytes (usage next).
Proof.
  intros env variant mode prefer current next sound covered step.
  apply attempt_step_cases in step.
  destruct step as [[vertex [rest [_ step]]]|[parent [remaining [suspended [_ step]]]]].
  - specialize (sound vertex). destruct mode; cbn [event_attempt] in step.
    + injection step as <-. cbn [usage with_peaks consumed_bytes quoted_bytes].
      destruct (counts (resident (workspace current)) vertex =? 0);
        cbn [charge_expansion consumed_bytes quoted_bytes]; lia.
    + destruct (counts (resident (workspace current)) vertex =? 0); [discriminate|].
      destruct (checked_sub (live_objects current)
        (deactivated_weight (resident (workspace current)) vertex unit_weight));
        [|discriminate].
      destruct (checked_sub (live_bytes current)
        (deactivated_weight (resident (workspace current)) vertex (object_bytes env)));
        [|discriminate].
      injection step as <-. cbn [usage with_peaks consumed_bytes quoted_bytes].
      destruct (counts (resident (workspace current)) vertex =? 1);
        cbn [charge_expansion consumed_bytes quoted_bytes]; lia.
  - destruct remaining; cbn [cursor_attempt] in step; injection step as <-;
      cbn [usage with_peaks consumed_bytes quoted_bytes charge_reference]; lia.
Qed.

Definition phase_result_state (result : phase_result) : trial_state :=
  match result with
  | PhaseDrained final => final
  | PhaseRefused _ at_refusal => at_refusal
  end.

Lemma run_phase_invariant : forall env variant budget schedule mode
  (invariant : trial_state -> Prop),
  (forall prefer current next, invariant current ->
     attempt_step env variant mode prefer current = AttemptStep next ->
     guard variant budget next = None -> invariant next) ->
  forall fuel index current, invariant current ->
    invariant (phase_result_state
      (run_phase env variant budget schedule mode fuel index current)).
Proof.
  intros env variant budget schedule mode invariant preserved fuel.
  induction fuel as [|remaining IH]; intros index current holds.
  - cbn [run_phase]. destruct (is_drained (workspace current)); exact holds.
  - cbn [run_phase]. unfold checked_step.
    destruct (attempt_step env variant mode (schedule index) current)
      as [next|why|] eqn:step; [|exact holds|exact holds].
    destruct (guard variant budget next) as [axis|] eqn:quiet; [exact holds|].
    exact (IH (S index) next (preserved _ _ _ holds step quiet)).
Qed.

Lemma run_phase_usage_monotone : forall env variant budget schedule mode fuel index current,
  usage_le (usage current)
    (usage (phase_result_state
      (run_phase env variant budget schedule mode fuel index current))).
Proof.
  intros env variant budget schedule mode fuel index current.
  apply (run_phase_invariant env variant budget schedule mode
    (fun state => usage_le (usage current) (usage state))); [|apply usage_le_refl].
  intros prefer before after bounded step _.
  exact (usage_le_trans _ _ _ bounded (attempt_usage_monotone _ _ _ _ _ _ step)).
Qed.

Lemma run_phase_drained_usage : forall env variant budget schedule mode fuel index current
  final,
  run_phase env variant budget schedule mode fuel index current = PhaseDrained final ->
  usage_le (usage current) (usage final).
Proof.
  intros env variant budget schedule mode fuel index current final drained_run.
  pose proof (run_phase_usage_monotone env variant budget schedule mode fuel index current)
    as bounded.
  rewrite drained_run in bounded. exact bounded.
Qed.

Lemma run_phase_drained_final : forall env variant budget schedule mode fuel index current
  final,
  run_phase env variant budget schedule mode fuel index current = PhaseDrained final ->
  drained (workspace final).
Proof.
  intros env variant budget schedule mode fuel.
  induction fuel as [|remaining IH]; intros index current final drained_run.
  - cbn [run_phase] in drained_run.
    destruct (is_drained (workspace current)) eqn:done; [|discriminate].
    injection drained_run as <-. now apply is_drained_iff.
  - cbn [run_phase] in drained_run. unfold checked_step in drained_run.
    destruct (attempt_step env variant mode (schedule index) current)
      as [next|why|] eqn:step; [|discriminate|].
    + destruct (guard variant budget next); [discriminate|].
      exact (IH _ _ _ drained_run).
    + injection drained_run as <-. now apply (attempt_drained_iff env variant mode
        (schedule index)).
Qed.

Lemma run_phase_budget_split : forall env variant limits schedule mode fuel index current
  final,
  run_phase env variant None schedule mode fuel index current = PhaseDrained final ->
  run_phase env variant (Some limits) schedule mode fuel index current = PhaseDrained final \/
  exists axis at_refusal,
    run_phase env variant (Some limits) schedule mode fuel index current =
      PhaseRefused (BudgetRefusal axis) at_refusal /\
    In axis step_axes /\ exceeds variant limits (usage final) axis = true.
Proof.
  intros env variant limits schedule mode fuel.
  induction fuel as [|remaining IH]; intros index current final unbudgeted.
  - left. cbn [run_phase] in *. exact unbudgeted.
  - cbn [run_phase] in *. unfold checked_step in *.
    destruct (attempt_step env variant mode (schedule index) current)
      as [next|why|] eqn:step; [|discriminate|left; exact unbudgeted].
    cbn [guard] in *.
    destruct (first_exceeded variant limits (usage next)) as [axis|] eqn:refused.
    + right. exists axis, current. split; [reflexivity|].
      apply first_exceeded_some in refused. destruct refused as [member above].
      split; [exact member|].
      eapply exceeds_monotone; [|exact above].
      exact (run_phase_drained_usage _ _ _ _ _ _ _ _ _ unbudgeted).
    + exact (IH _ _ _ unbudgeted).
Qed.

Lemma run_phase_within_limits : forall env variant limits schedule mode fuel index current
  final,
  run_phase env variant None schedule mode fuel index current = PhaseDrained final ->
  first_exceeded variant limits (usage final) = None ->
  run_phase env variant (Some limits) schedule mode fuel index current = PhaseDrained final.
Proof.
  intros env variant limits schedule mode fuel index current final unbudgeted quiet.
  destruct (run_phase_budget_split env variant limits schedule mode fuel index current final
    unbudgeted) as [same|[axis [at_refusal [_ [member above]]]]]; [exact same|].
  exfalso. rewrite first_exceeded_none_iff in quiet. specialize (quiet axis member).
  unfold exceeds in above. apply Nat.ltb_lt in above. lia.
Qed.

Lemma run_phase_budgeted_drained : forall env variant limits schedule mode fuel index current
  final,
  first_exceeded variant limits (usage current) = None ->
  run_phase env variant (Some limits) schedule mode fuel index current = PhaseDrained final ->
  run_phase env variant None schedule mode fuel index current = PhaseDrained final /\
  first_exceeded variant limits (usage final) = None.
Proof.
  intros env variant limits schedule mode fuel.
  induction fuel as [|remaining IH]; intros index current final quiet budgeted.
  - cbn [run_phase] in *.
    destruct (is_drained (workspace current)); [|discriminate].
    injection budgeted as <-. split; [reflexivity|exact quiet].
  - cbn [run_phase] in *. unfold checked_step in *.
    destruct (attempt_step env variant mode (schedule index) current)
      as [next|why|] eqn:step; [|discriminate|].
    + cbn [guard] in *.
      destruct (first_exceeded variant limits (usage next)) as [axis|] eqn:next_quiet;
        [discriminate|].
      exact (IH _ _ _ next_quiet budgeted).
    + injection budgeted as <-. split; [reflexivity|exact quiet].
Qed.

Definition trial_invariant (env : trial_environment) (mode : phase) (roots : counter)
  (current : trial_state) : Prop :=
  valid_configuration (graph env) mode (universe env) roots (workspace current) /\
  exact_counters env current.

Lemma faithful_attempt_preserves_trial_invariant : forall env mode roots prefer current next,
  NoDup (universe env) -> edges_closed env ->
  trial_invariant env mode roots current ->
  attempt_step env Faithful mode prefer current = AttemptStep next ->
  trial_invariant env mode roots next.
Proof.
  intros env mode roots prefer current next unique closed [valid exact] step.
  split.
  - exact (@cursor_step_preserves_configuration (graph env) (universe env) roots mode
      (workspace current) (workspace next) unique closed (attempt_sound _ _ _ _ _ step)
      valid).
  - exact (attempt_exact _ _ _ _ _ _ unique valid exact step).
Qed.

Lemma run_phase_preserves_trial_invariant : forall env budget schedule mode roots fuel index
  current,
  NoDup (universe env) -> edges_closed env ->
  trial_invariant env mode roots current ->
  trial_invariant env mode roots
    (phase_result_state (run_phase env Faithful budget schedule mode fuel index current)).
Proof.
  intros env budget schedule mode roots fuel index current unique closed holds.
  apply (run_phase_invariant env Faithful budget schedule mode
    (trial_invariant env mode roots)); [|exact holds].
  intros prefer before after before_holds step _.
  exact (faithful_attempt_preserves_trial_invariant _ _ _ _ _ _ unique closed before_holds
    step).
Qed.

Lemma run_phase_is_a_cursor_execution : forall env budget schedule mode fuel index current,
  cursor_execution (graph env) mode (workspace current)
    (workspace (phase_result_state
      (run_phase env Faithful budget schedule mode fuel index current))).
Proof.
  intros env budget schedule mode fuel index current.
  apply (run_phase_invariant env Faithful budget schedule mode
    (fun state => cursor_execution (graph env) mode (workspace current) (workspace state)));
    [|constructor].
  intros prefer before after execution step _.
  eapply execution_step; [exact execution|exact (attempt_sound _ _ _ _ _ step)].
Qed.

Lemma run_phase_drains : forall env mode roots schedule fuel index current,
  NoDup (universe env) -> edges_closed env ->
  trial_invariant env mode roots current ->
  remaining_work (graph env) mode (universe env) (workspace current) <= fuel ->
  exists final,
    run_phase env Faithful None schedule mode fuel index current = PhaseDrained final.
Proof.
  intros env mode roots schedule fuel.
  induction fuel as [|remaining IH]; intros index current unique closed holds budget.
  - exists current. cbn [run_phase].
    assert (idle : pending_work (workspace current) = 0).
    { unfold remaining_work in budget. lia. }
    apply zero_pending_work_is_exactly_drained in idle. apply is_drained_iff in idle.
    rewrite idle. reflexivity.
  - cbn [run_phase]. unfold checked_step.
    destruct (attempt_step env Faithful mode (schedule index) current)
      as [next|why|] eqn:step.
    + destruct holds as [valid exact].
      pose proof (@every_cursor_step_spends_exactly_one (graph env) (universe env) roots mode
        (workspace current) (workspace next) unique valid (attempt_sound _ _ _ _ _ step))
        as spent.
      assert (smaller : remaining_work (graph env) mode (universe env) (workspace next) <=
        remaining) by lia.
      exact (IH (S index) next unique closed
        (faithful_attempt_preserves_trial_invariant _ _ _ _ _ _ unique closed
          (conj valid exact) step) smaller).
    + destruct holds as [valid exact].
      exfalso. exact (attempt_never_invalid _ _ _ _ _ _ unique valid exact step).
    + exists current. reflexivity.
Qed.

Lemma deactivation_start_usage_monotone : forall variant middle removed,
  usage_le (usage middle) (usage (deactivation_start variant middle removed)).
Proof.
  intros variant middle removed. unfold usage_le, deactivation_start.
  cbn [usage peak_count peak_objects peak_bytes peak_scratch references expansions
    quoted_bytes consumed_bytes].
  destruct (charges_initialization variant); lia.
Qed.

Lemma final_exceeded_none_iff : forall limits current,
  final_exceeded limits current = None <->
  live_objects current <= limits FinalObjectsAxis /\
  live_bytes current <= limits FinalBytesAxis.
Proof.
  intros limits current. unfold final_exceeded.
  destruct (limits FinalObjectsAxis <? live_objects current) eqn:objects_above.
  - apply Nat.ltb_lt in objects_above.
    split; [intros contra; discriminate contra|intros [objects _]; exfalso; lia].
  - apply Nat.ltb_ge in objects_above.
    destruct (limits FinalBytesAxis <? live_bytes current) eqn:bytes_above.
    + apply Nat.ltb_lt in bytes_above.
      split; [intros contra; discriminate contra|intros [_ bytes]; exfalso; lia].
    + apply Nat.ltb_ge in bytes_above.
      split; [intros _; split; assumption|intros _; reflexivity].
Qed.

Lemma final_exceeded_some : forall limits current axis,
  final_exceeded limits current = Some axis ->
  (axis = FinalObjectsAxis /\ limits FinalObjectsAxis < live_objects current) \/
  (axis = FinalBytesAxis /\ limits FinalBytesAxis < live_bytes current).
Proof.
  intros limits current axis above. unfold final_exceeded in above.
  destruct (limits FinalObjectsAxis <? live_objects current) eqn:objects_above.
  - injection above as <-. left. split; [reflexivity|]. now apply Nat.ltb_lt.
  - destruct (limits FinalBytesAxis <? live_bytes current) eqn:bytes_above;
      [|discriminate].
    injection above as <-. right. split; [reflexivity|]. now apply Nat.ltb_lt.
Qed.

Lemma occurrences_absent : forall listed query,
  ~ In query listed -> occurrences listed query = 0.
Proof.
  induction listed as [|head tail IH]; intros query absent; [reflexivity|].
  cbn [occurrences]. destruct (Nat.eqb query head) eqn:same.
  - apply Nat.eqb_eq in same. subst head. exfalso. apply absent. now left.
  - cbn. apply IH. intros member. apply absent. now right.
Qed.

Lemma authorized_iff : forall roots removed,
  authorized roots removed = true <->
  forall vertex, occurrences removed vertex <= roots vertex.
Proof.
  intros roots removed. unfold authorized. rewrite forallb_forall. split.
  - intros bounded vertex.
    destruct (in_dec Nat.eq_dec vertex removed) as [member|absent].
    + apply Nat.leb_le. exact (bounded vertex member).
    + rewrite (occurrences_absent removed vertex absent). lia.
  - intros bounded vertex _. apply Nat.leb_le. apply bounded.
Qed.

Lemma drained_configuration : forall machine,
  drained machine -> machine = Configuration (resident machine) [] [].
Proof.
  intros [state explicit cursors] [no_events no_frames]. cbn [events frames] in *.
  subst explicit cursors. reflexivity.
Qed.

Lemma core_prepared_inversion : forall env variant budget schedule base added removed final,
  run_core env variant budget schedule base added removed = CorePrepared final ->
  authorized (add_roots (base_roots base) added) removed = true /\
  guard variant budget (activation_start variant base added) = None /\
  exists middle,
    run_phase env variant budget schedule Activation
      (remaining_work (graph env) Activation (universe env)
        (workspace (activation_start variant base added))) 0
      (activation_start variant base added) = PhaseDrained middle /\
    guard variant budget (deactivation_start variant middle removed) = None /\
    run_phase env variant budget schedule Deactivation
      (remaining_work (graph env) Deactivation (universe env)
        (workspace (deactivation_start variant middle removed))) 0
      (deactivation_start variant middle removed) = PhaseDrained final /\
    final_guard budget final = None.
Proof.
  intros env variant budget schedule base added removed final prepared.
  unfold run_core in prepared.
  destruct (authorized (add_roots (base_roots base) added) removed) eqn:allowed;
    [|discriminate].
  destruct (guard variant budget (activation_start variant base added)) as [axis|]
    eqn:start; [discriminate|].
  destruct (run_phase env variant budget schedule Activation
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start variant base added))) 0
    (activation_start variant base added)) as [middle|why at_refusal] eqn:activated;
    [|discriminate].
  unfold run_removal in prepared.
  destruct (guard variant budget (deactivation_start variant middle removed)) as [axis|]
    eqn:restart; [discriminate|].
  destruct (run_phase env variant budget schedule Deactivation
    (remaining_work (graph env) Deactivation (universe env)
      (workspace (deactivation_start variant middle removed))) 0
    (deactivation_start variant middle removed)) as [last|why at_refusal] eqn:deactivated;
    [|discriminate].
  destruct (final_guard budget last) as [axis|] eqn:closing; [discriminate|].
  injection prepared as same. subst last.
  split; [reflexivity|]. split; [reflexivity|]. exists middle.
  split; [reflexivity|]. split; [exact restart|]. split; [exact deactivated|exact closing].
Qed.

Lemma core_bounded_prepared : forall env variant limits schedule base added removed final,
  run_core env variant (Some limits) schedule base added removed = CorePrepared final ->
  run_core env variant None schedule base added removed = CorePrepared final /\
  first_exceeded variant limits (usage final) = None /\ final_exceeded limits final = None.
Proof.
  intros env variant limits schedule base added removed final prepared.
  destruct (core_prepared_inversion _ _ _ _ _ _ _ _ prepared)
    as [allowed [start [middle [activated [restart [deactivated closing]]]]]].
  cbn [guard final_guard] in start, restart, closing.
  destruct (run_phase_budgeted_drained _ _ _ _ _ _ _ _ _ start activated)
    as [activated_free middle_quiet].
  destruct (run_phase_budgeted_drained _ _ _ _ _ _ _ _ _ restart deactivated)
    as [deactivated_free final_quiet].
  split; [|split; assumption].
  unfold run_core. rewrite allowed. cbn [guard].
  rewrite activated_free. unfold run_removal. cbn [guard].
  rewrite deactivated_free. reflexivity.
Qed.

Lemma core_within_limits : forall env variant limits schedule base added removed final,
  run_core env variant None schedule base added removed = CorePrepared final ->
  first_exceeded variant limits (usage final) = None ->
  final_exceeded limits final = None ->
  run_core env variant (Some limits) schedule base added removed = CorePrepared final.
Proof.
  intros env variant limits schedule base added removed final prepared final_quiet closing.
  destruct (core_prepared_inversion _ _ _ _ _ _ _ _ prepared)
    as [allowed [_ [middle [activated [_ [deactivated _]]]]]].
  pose proof (run_phase_drained_usage _ _ _ _ _ _ _ _ _ activated) as first_half.
  pose proof (run_phase_drained_usage _ _ _ _ _ _ _ _ _ deactivated) as second_half.
  pose proof (deactivation_start_usage_monotone variant middle removed) as handoff.
  assert (middle_quiet : first_exceeded variant limits (usage middle) = None).
  { eapply first_exceeded_antitone; [|exact final_quiet].
    exact (usage_le_trans _ _ _ handoff second_half). }
  assert (restart_quiet : first_exceeded variant limits
    (usage (deactivation_start variant middle removed)) = None).
  { eapply first_exceeded_antitone; [exact second_half|exact final_quiet]. }
  assert (start_quiet : first_exceeded variant limits
    (usage (activation_start variant base added)) = None).
  { eapply first_exceeded_antitone; [exact first_half|exact middle_quiet]. }
  unfold run_core. rewrite allowed. cbn [guard]. rewrite start_quiet.
  rewrite (run_phase_within_limits _ _ _ _ _ _ _ _ _ activated middle_quiet).
  unfold run_removal. cbn [guard]. rewrite restart_quiet.
  rewrite (run_phase_within_limits _ _ _ _ _ _ _ _ _ deactivated final_quiet).
  cbn [final_guard]. rewrite closing. reflexivity.
Qed.

Lemma core_budget_split : forall env variant limits schedule base added removed final,
  run_core env variant None schedule base added removed = CorePrepared final ->
  run_core env variant (Some limits) schedule base added removed = CorePrepared final \/
  exists axis at_refusal,
    run_core env variant (Some limits) schedule base added removed =
      CoreRefused (BudgetRefusal axis) at_refusal /\
    ((In axis step_axes /\ exceeds variant limits (usage final) axis = true) \/
     final_exceeded limits final = Some axis).
Proof.
  intros env variant limits schedule base added removed final prepared.
  destruct (core_prepared_inversion _ _ _ _ _ _ _ _ prepared)
    as [allowed [_ [middle [activated [_ [deactivated _]]]]]].
  pose proof (run_phase_drained_usage _ _ _ _ _ _ _ _ _ activated) as first_half.
  pose proof (run_phase_drained_usage _ _ _ _ _ _ _ _ _ deactivated) as second_half.
  pose proof (deactivation_start_usage_monotone variant middle removed) as handoff.
  unfold run_core. rewrite allowed. cbn [guard].
  destruct (first_exceeded variant limits (usage (activation_start variant base added)))
    as [axis|] eqn:start.
  - right. exists axis, (activation_start variant base added). split; [reflexivity|].
    left. apply first_exceeded_some in start. destruct start as [member above].
    split; [exact member|]. eapply exceeds_monotone; [|exact above].
    exact (usage_le_trans _ _ _ first_half (usage_le_trans _ _ _ handoff second_half)).
  - destruct (run_phase_budget_split env variant limits schedule Activation _ 0 _ middle
      activated) as [activated_bounded|[axis [at_refusal [refused [member above]]]]].
    + rewrite activated_bounded. unfold run_removal. cbn [guard].
      destruct (first_exceeded variant limits
        (usage (deactivation_start variant middle removed))) as [axis|] eqn:restart.
      * right. exists axis, (deactivation_start variant middle removed).
        split; [reflexivity|]. left.
        apply first_exceeded_some in restart. destruct restart as [member above].
        split; [exact member|]. eapply exceeds_monotone; [exact second_half|exact above].
      * destruct (run_phase_budget_split env variant limits schedule Deactivation _ 0 _
          final deactivated)
          as [deactivated_bounded|[axis [at_refusal [refused [member above]]]]].
        -- rewrite deactivated_bounded. cbn [final_guard].
           destruct (final_exceeded limits final) as [axis|] eqn:closing.
           ++ right. exists axis, final. split; [reflexivity|]. right. reflexivity.
           ++ left. reflexivity.
        -- right. exists axis, at_refusal. rewrite refused. split; [reflexivity|].
           left. split; [exact member|exact above].
    + right. exists axis, at_refusal. rewrite refused. split; [reflexivity|].
      left. split; [exact member|]. eapply exceeds_monotone; [|exact above].
      exact (usage_le_trans _ _ _ handoff second_half).
Qed.

Lemma limits_cover_iff : forall limits base added removed final,
  limits_cover limits (evidence_of base added removed final) <->
  first_exceeded Faithful limits (usage final) = None /\ final_exceeded limits final = None.
Proof.
  intros limits base added removed final.
  rewrite first_exceeded_none_iff, final_exceeded_none_iff. unfold limits_cover. split.
  - intros cover. split.
    + intros axis _. specialize (cover axis). destruct axis; cbn in *; lia.
    + split; [exact (cover FinalObjectsAxis)|exact (cover FinalBytesAxis)].
  - intros [steps [objects bytes]] axis.
    destruct axis;
      [ exact (steps CountAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps UnionObjectsAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps UnionBytesAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps ScratchAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps ReferenceAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps ExpansionAxis ltac:(cbn [step_axes In]; auto 10))
      | exact (steps QuotedBytesAxis ltac:(cbn [step_axes In]; auto 10))
      | exact objects
      | exact bytes ].
Qed.

Lemma trial_prepared_inversion : forall env budget schedule base added removed kept evidence,
  trial env budget schedule base added removed = TrialResult kept (TrialPrepared evidence) ->
  kept = base /\ exists final,
    run_core env Faithful budget schedule base added removed = CorePrepared final /\
    evidence = evidence_of base added removed final.
Proof.
  intros env budget schedule base added removed kept evidence prepared.
  unfold run_trial in prepared.
  destruct (run_core env Faithful budget schedule base added removed)
    as [final|why at_refusal] eqn:core.
  - injection prepared as same_evidence same_base. subst kept evidence.
    split; [reflexivity|]. exists final. split; reflexivity.
  - discriminate prepared.
Qed.

Lemma core_prepares_without_limits : forall env schedule base added removed,
  trial_input_valid env base added removed ->
  (forall vertex, occurrences removed vertex <= add_roots (base_roots base) added vertex) ->
  exists final, run_core env Faithful None schedule base added removed = CorePrepared final.
Proof.
  intros env schedule base added removed
    [unique [closed [agreement [balance [counted [added_domain removed_domain]]]]]] bounded.
  assert (allowed : authorized (add_roots (base_roots base) added) removed = true)
    by now apply authorized_iff.
  assert (start_holds : trial_invariant env Activation (add_roots (base_roots base) added)
    (activation_start Faithful base added)).
  { split.
    - exact (@activation_initial_configuration_is_valid (graph env) (universe env)
        (base_roots base) (base_ledger base) added agreement balance added_domain).
    - destruct counted as [objects bytes]. split; [exact objects|exact bytes]. }
  destruct (run_phase_drains env Activation (add_roots (base_roots base) added) schedule
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start Faithful base added))) 0
    (activation_start Faithful base added) unique closed start_holds (le_n _))
    as [middle activated].
  pose proof (run_phase_preserves_trial_invariant env None schedule Activation
    (add_roots (base_roots base) added)
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start Faithful base added))) 0
    (activation_start Faithful base added) unique closed start_holds) as middle_holds.
  rewrite activated in middle_holds. cbn [phase_result_state] in middle_holds.
  pose proof (run_phase_drained_final _ _ _ _ _ _ _ _ _ activated) as middle_drained.
  destruct middle_holds as [[middle_agreement [middle_balance _]]
    [middle_objects middle_bytes]].
  destruct middle_drained as [no_events no_frames].
  cbn [phase_balance] in middle_balance. rewrite no_events, no_frames in middle_balance.
  pose proof (@drained_activation_is_quiescent (graph env) (universe env)
    (add_roots (base_roots base) added) (resident (workspace middle)) middle_balance)
    as middle_quiescent.
  assert (restart_holds : trial_invariant env Deactivation
    (remove_roots (add_roots (base_roots base) added) removed)
    (deactivation_start Faithful middle removed)).
  { split.
    - exact (@deactivation_initial_configuration_is_valid (graph env) (universe env)
        (add_roots (base_roots base) added) (resident (workspace middle)) removed
        middle_agreement middle_quiescent removed_domain bounded).
    - split; [exact middle_objects|exact middle_bytes]. }
  destruct (run_phase_drains env Deactivation
    (remove_roots (add_roots (base_roots base) added) removed) schedule
    (remaining_work (graph env) Deactivation (universe env)
      (workspace (deactivation_start Faithful middle removed))) 0
    (deactivation_start Faithful middle removed) unique closed restart_holds (le_n _))
    as [final deactivated].
  exists final. unfold run_core. rewrite allowed. cbn [guard].
  rewrite activated. unfold run_removal. cbn [guard]. rewrite deactivated. reflexivity.
Qed.

Lemma activation_run_tracks_peak : forall env budget schedule fuel index current,
  peak_objects (usage current) = live_objects current ->
  peak_bytes (usage current) = live_bytes current ->
  peak_objects (usage (phase_result_state
      (run_phase env Faithful budget schedule Activation fuel index current))) =
    live_objects (phase_result_state
      (run_phase env Faithful budget schedule Activation fuel index current)) /\
  peak_bytes (usage (phase_result_state
      (run_phase env Faithful budget schedule Activation fuel index current))) =
    live_bytes (phase_result_state
      (run_phase env Faithful budget schedule Activation fuel index current)).
Proof.
  intros env budget schedule fuel index current objects bytes.
  apply (run_phase_invariant env Faithful budget schedule Activation
    (fun state => peak_objects (usage state) = live_objects state /\
      peak_bytes (usage state) = live_bytes state)); [|split; assumption].
  intros prefer before after [before_objects before_bytes] step _.
  exact (activation_attempt_tracks_peak _ _ _ _ before_objects before_bytes step).
Qed.

Lemma deactivation_run_keeps_peak : forall env budget schedule fuel index current,
  live_objects current <= peak_objects (usage current) ->
  live_bytes current <= peak_bytes (usage current) ->
  peak_objects (usage (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current))) =
    peak_objects (usage current) /\
  peak_bytes (usage (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current))) =
    peak_bytes (usage current) /\
  live_objects (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current)) <=
    peak_objects (usage (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current))) /\
  live_bytes (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current)) <=
    peak_bytes (usage (phase_result_state
      (run_phase env Faithful budget schedule Deactivation fuel index current))).
Proof.
  intros env budget schedule fuel index current objects bytes.
  apply (run_phase_invariant env Faithful budget schedule Deactivation
    (fun state => peak_objects (usage state) = peak_objects (usage current) /\
      peak_bytes (usage state) = peak_bytes (usage current) /\
      live_objects state <= peak_objects (usage state) /\
      live_bytes state <= peak_bytes (usage state)));
    [|split; [reflexivity|split; [reflexivity|split; assumption]]].
  intros prefer before after [kept_objects [kept_bytes [objects_below bytes_below]]] step _.
  destruct (deactivation_attempt_keeps_peak _ _ _ _ objects_below bytes_below step)
    as [same_objects [same_bytes [next_objects next_bytes]]].
  split; [congruence|]. split; [congruence|]. split; assumption.
Qed.

Lemma prepared_core_facts : forall env budget schedule base added removed final,
  trial_input_valid env base added removed ->
  run_core env Faithful budget schedule base added removed = CorePrepared final ->
  exists middle,
    cursor_execution (graph env) Activation (Configuration (base_ledger base) added [])
      (Configuration (resident (workspace middle)) [] []) /\
    cursor_execution (graph env) Deactivation
      (Configuration (resident (workspace middle)) removed [])
      (Configuration (resident (workspace final)) [] []) /\
    (forall vertex, occurrences removed vertex <= add_roots (base_roots base) added vertex) /\
    exact_counters env middle /\ exact_counters env final /\
    peak_objects (usage final) = live_objects middle /\
    peak_bytes (usage final) = live_bytes middle.
Proof.
  intros env budget schedule base added removed final valid prepared.
  pose proof valid as
    [unique [closed [agreement [balance [counted [added_domain removed_domain]]]]]].
  destruct (core_prepared_inversion _ _ _ _ _ _ _ _ prepared)
    as [allowed [_ [middle [activated [_ [deactivated _]]]]]].
  rewrite authorized_iff in allowed.
  exists middle.
  assert (start_holds : trial_invariant env Activation (add_roots (base_roots base) added)
    (activation_start Faithful base added)).
  { split.
    - exact (@activation_initial_configuration_is_valid (graph env) (universe env)
        (base_roots base) (base_ledger base) added agreement balance added_domain).
    - destruct counted as [objects bytes]. split; [exact objects|exact bytes]. }
  pose proof (run_phase_preserves_trial_invariant env budget schedule Activation
    (add_roots (base_roots base) added)
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start Faithful base added))) 0
    (activation_start Faithful base added) unique closed start_holds) as middle_holds.
  pose proof (run_phase_is_a_cursor_execution env budget schedule Activation
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start Faithful base added))) 0
    (activation_start Faithful base added)) as first_execution.
  pose proof (activation_run_tracks_peak env budget schedule
    (remaining_work (graph env) Activation (universe env)
      (workspace (activation_start Faithful base added))) 0
    (activation_start Faithful base added) eq_refl eq_refl) as middle_peaks.
  rewrite activated in middle_holds, first_execution, middle_peaks.
  cbn [phase_result_state] in middle_holds, first_execution, middle_peaks.
  pose proof (run_phase_drained_final _ _ _ _ _ _ _ _ _ activated) as middle_drained.
  rewrite (drained_configuration _ middle_drained) in first_execution.
  destruct middle_holds as [[middle_agreement [middle_balance _]] middle_counted].
  destruct middle_drained as [no_events no_frames].
  cbn [phase_balance] in middle_balance. rewrite no_events, no_frames in middle_balance.
  pose proof (@drained_activation_is_quiescent (graph env) (universe env)
    (add_roots (base_roots base) added) (resident (workspace middle)) middle_balance)
    as middle_quiescent.
  destruct middle_peaks as [middle_peak_objects middle_peak_bytes].
  assert (restart_holds : trial_invariant env Deactivation
    (remove_roots (add_roots (base_roots base) added) removed)
    (deactivation_start Faithful middle removed)).
  { split.
    - exact (@deactivation_initial_configuration_is_valid (graph env) (universe env)
        (add_roots (base_roots base) added) (resident (workspace middle)) removed
        middle_agreement middle_quiescent removed_domain allowed).
    - destruct middle_counted as [objects bytes]. split; [exact objects|exact bytes]. }
  assert (restart_objects : live_objects (deactivation_start Faithful middle removed) <=
    peak_objects (usage (deactivation_start Faithful middle removed))).
  { cbn [deactivation_start live_objects usage peak_objects]. lia. }
  assert (restart_bytes : live_bytes (deactivation_start Faithful middle removed) <=
    peak_bytes (usage (deactivation_start Faithful middle removed))).
  { cbn [deactivation_start live_bytes usage peak_bytes]. lia. }
  pose proof (run_phase_preserves_trial_invariant env budget schedule Deactivation
    (remove_roots (add_roots (base_roots base) added) removed)
    (remaining_work (graph env) Deactivation (universe env)
      (workspace (deactivation_start Faithful middle removed))) 0
    (deactivation_start Faithful middle removed) unique closed restart_holds) as final_holds.
  pose proof (run_phase_is_a_cursor_execution env budget schedule Deactivation
    (remaining_work (graph env) Deactivation (universe env)
      (workspace (deactivation_start Faithful middle removed))) 0
    (deactivation_start Faithful middle removed)) as second_execution.
  pose proof (deactivation_run_keeps_peak env budget schedule
    (remaining_work (graph env) Deactivation (universe env)
      (workspace (deactivation_start Faithful middle removed))) 0
    (deactivation_start Faithful middle removed) restart_objects restart_bytes)
    as final_peaks.
  rewrite deactivated in final_holds, second_execution, final_peaks.
  cbn [phase_result_state] in final_holds, second_execution, final_peaks.
  pose proof (run_phase_drained_final _ _ _ _ _ _ _ _ _ deactivated) as final_drained.
  rewrite (drained_configuration _ final_drained) in second_execution.
  destruct final_holds as [_ final_counted].
  destruct final_peaks as [final_peak_objects [final_peak_bytes _]].
  cbn [deactivation_start usage peak_objects peak_bytes] in final_peak_objects,
    final_peak_bytes.
  split; [exact first_execution|]. split; [exact second_execution|].
  split; [exact allowed|]. split; [exact middle_counted|]. split; [exact final_counted|].
  split; congruence.
Qed.

Definition scratch_reserved (limits : trial_limits) (current : trial_state) : Prop :=
  live_scratch (workspace current) <= peak_scratch (usage current) /\
  peak_scratch (usage current) <= limits ScratchAxis.

Theorem checked_attempts_are_exactly_scheduled_cursor_steps :
  forall env mode roots current,
  (forall prefer next,
     attempt_step env Faithful mode prefer current = AttemptStep next ->
     cursor_step (graph env) mode (workspace current) (workspace next)) /\
  (forall prefer,
     attempt_step env Faithful mode prefer current = AttemptDrained <->
     drained (workspace current)) /\
  (NoDup (universe env) ->
   valid_configuration (graph env) mode (universe env) roots (workspace current) ->
   exact_counters env current ->
   (forall prefer why, attempt_step env Faithful mode prefer current <> AttemptInvalid why) /\
   forall machine, cursor_step (graph env) mode (workspace current) machine ->
     exists prefer next,
       attempt_step env Faithful mode prefer current = AttemptStep next /\
       workspace next = machine).
Proof.
  intros env mode roots current. split; [|split].
  - intros prefer next step. exact (attempt_sound _ _ _ _ _ step).
  - intros prefer. apply attempt_drained_iff.
  - intros unique valid counted. split.
    + intros prefer why. exact (attempt_never_invalid _ _ _ _ _ _ unique valid counted).
    + intros machine transition.
      exact (attempt_complete _ _ _ _ _ unique valid counted transition).
Qed.

Theorem checked_step_is_failure_atomic :
  forall env variant budget schedule mode fuel index current why,
  checked_step env variant budget mode (schedule index) current = StepRefused why ->
  run_phase env variant budget schedule mode (S fuel) index current =
    PhaseRefused why current /\
  ((exists limits next axis, budget = Some limits /\ why = BudgetRefusal axis /\
      attempt_step env variant mode (schedule index) current = AttemptStep next /\
      first_exceeded variant limits (usage next) = Some axis) \/
   attempt_step env variant mode (schedule index) current = AttemptInvalid why).
Proof.
  intros env variant budget schedule mode fuel index current why refused.
  split; [cbn [run_phase]; rewrite refused; reflexivity|].
  unfold checked_step in refused.
  destruct (attempt_step env variant mode (schedule index) current)
    as [next|reason|] eqn:step;
    [|right; injection refused as <-; reflexivity|discriminate].
  left. destruct budget as [limits|]; cbn [guard] in refused; [|discriminate].
  destruct (first_exceeded variant limits (usage next)) as [axis|] eqn:exceeded;
    [|discriminate].
  injection refused as <-. exists limits, next, axis.
  split; [reflexivity|]. split; [reflexivity|]. split; [reflexivity|exact exceeded].
Qed.

Theorem accepted_event_reservations_are_checked_arithmetic :
  forall env limits mode prefer current next vertex rest,
  events (workspace current) = vertex :: rest ->
  (prefer = false \/ frames (workspace current) = []) ->
  checked_step env Faithful (Some limits) mode prefer current = StepAccepted next ->
  match mode with
  | Activation =>
      checked_increment (limits CountAxis) (resident (workspace current)) vertex =
        Some (resident (workspace next)) /\
      checked_add (limits UnionObjectsAxis) (live_objects current)
        (activated_weight (resident (workspace current)) vertex unit_weight) =
        Some (live_objects next) /\
      checked_add (limits UnionBytesAxis) (live_bytes current)
        (activated_weight (resident (workspace current)) vertex (object_bytes env)) =
        Some (live_bytes next)
  | Deactivation =>
      checked_decrement (resident (workspace current)) vertex =
        Some (resident (workspace next)) /\
      checked_sub (live_objects current)
        (deactivated_weight (resident (workspace current)) vertex unit_weight) =
        Some (live_objects next) /\
      checked_sub (live_bytes current)
        (deactivated_weight (resident (workspace current)) vertex (object_bytes env)) =
        Some (live_bytes next)
  end.
Proof.
  intros env limits mode prefer [[state explicit cursors] objects bytes spent] next vertex
    rest shape selected accepted.
  cbn [workspace events frames resident live_objects live_bytes] in *. subst explicit.
  assert (event : attempt_step env Faithful mode prefer
      (TrialState (Configuration state (vertex :: rest) cursors) objects bytes spent) =
    event_attempt env mode state vertex rest cursors objects bytes spent).
  { cbn [attempt_step]. destruct cursors as [|[parent remaining] suspended];
      [reflexivity|].
    destruct selected as [later|empty]; [subst prefer; reflexivity|discriminate]. }
  unfold checked_step in accepted. rewrite event in accepted. cbn [guard] in accepted.
  destruct mode; cbn [event_attempt] in accepted |- *.
  - match type of accepted with
    | context [first_exceeded ?variant ?bounds ?spent] =>
        destruct (first_exceeded variant bounds spent) as [axis|] eqn:quiet;
          [discriminate|]
    end.
    injection accepted as <-. cbn [workspace resident live_objects live_bytes].
    rewrite first_exceeded_none_iff in quiet.
    pose proof (quiet CountAxis ltac:(cbn [step_axes In]; auto 10)) as count_bound.
    pose proof (quiet UnionObjectsAxis ltac:(cbn [step_axes In]; auto 10))
      as objects_bound.
    pose proof (quiet UnionBytesAxis ltac:(cbn [step_axes In]; auto 10)) as bytes_bound.
    cbn [axis_usage axis_limit step_usage usage with_peaks peak_count peak_objects
      peak_bytes] in count_bound, objects_bound, bytes_bound.
    split; [|split].
    + apply (proj2 (checked_increment_refines_natural_update (limits CountAxis) state vertex
        (increment state vertex))). split; [reflexivity|].
      destruct (counts state vertex =? 0); cbn [charge_expansion peak_count] in count_bound;
        lia.
    + apply (proj2 (checked_add_is_exact (limits UnionObjectsAxis) objects
        (activated_weight state vertex unit_weight)
        (objects + activated_weight state vertex unit_weight))). split; [reflexivity|].
      destruct (counts state vertex =? 0);
        cbn [charge_expansion peak_objects] in objects_bound; lia.
    + apply (proj2 (checked_add_is_exact (limits UnionBytesAxis) bytes
        (activated_weight state vertex (object_bytes env))
        (bytes + activated_weight state vertex (object_bytes env)))). split; [reflexivity|].
      destruct (counts state vertex =? 0);
        cbn [charge_expansion peak_bytes] in bytes_bound; lia.
  - destruct (counts state vertex =? 0) eqn:zero; [discriminate|].
    destruct (checked_sub objects (deactivated_weight state vertex unit_weight))
      as [next_objects|] eqn:objects_removed; [|discriminate].
    destruct (checked_sub bytes (deactivated_weight state vertex (object_bytes env)))
      as [next_bytes|] eqn:bytes_removed; [|discriminate].
    match type of accepted with
    | context [first_exceeded ?variant ?bounds ?spent] =>
        destruct (first_exceeded variant bounds spent) as [axis|] eqn:quiet;
          [discriminate|]
    end.
    injection accepted as <-. cbn [workspace resident live_objects live_bytes].
    split; [|split; reflexivity].
    apply (proj2 (checked_decrement_refines_natural_update state vertex
      (decrement state vertex))). split; [reflexivity|].
    apply Nat.eqb_neq in zero. lia.
Qed.

Theorem every_trial_retains_the_bit_identical_predecessor :
  forall env variant budget schedule base added removed,
  variant <> PartialCommit ->
  retained (run_trial env variant budget schedule base added removed) = base.
Proof.
  intros env variant budget schedule base added removed faithful_retention.
  unfold run_trial.
  destruct (run_core env variant budget schedule base added removed)
    as [final|why at_refusal]; [reflexivity|].
  destruct variant; try reflexivity.
  exfalso. apply faithful_retention. reflexivity.
Qed.

Theorem prepared_trial_agrees_with_cold_ledger :
  forall env budget schedule base added removed kept evidence rank cold,
  trial_input_valid env base added removed -> edges_descend env rank ->
  trial env budget schedule base added removed = TrialResult kept (TrialPrepared evidence) ->
  coherent cold ->
  quiescent (graph env) (universe env)
    (remove_roots (add_roots (base_roots base) added) removed) cold ->
  (forall vertex, counts (prepared_ledger evidence) vertex = counts cold vertex /\
     active (prepared_ledger evidence) vertex = active cold vertex) /\
  prepared_roots evidence = remove_roots (add_roots (base_roots base) added) removed /\
  prepared_objects evidence = inventory (universe env) cold unit_weight /\
  prepared_bytes evidence = inventory (universe env) cold (object_bytes env).
Proof.
  intros env budget schedule base added removed kept evidence rank cold valid descending
    prepared cold_coherent cold_quiescent.
  destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ prepared) as [_ [final [core ->]]].
  destruct (prepared_core_facts _ _ _ _ _ _ _ valid core)
    as [middle [first_execution [second_execution [bounded [_
      [[final_objects final_bytes] _]]]]]].
  pose proof valid as
    [unique [closed [agreement [balance [_ [added_domain removed_domain]]]]]].
  pose proof (@add_before_remove_agrees_with_cold_ledger (graph env) (universe env)
    (base_roots base) (base_ledger base) added removed (resident (workspace middle))
    (resident (workspace final)) cold rank unique closed descending agreement balance
    added_domain removed_domain bounded first_execution second_execution cold_coherent
    cold_quiescent) as agrees.
  cbn [evidence_of prepared_ledger prepared_roots prepared_objects prepared_bytes].
  split; [exact agrees|]. split; [reflexivity|].
  split.
  - rewrite final_objects. unfold inventory.
    apply selected_sum_extensional. intros vertex. exact (proj2 (agrees vertex)).
  - rewrite final_bytes. unfold inventory.
    apply selected_sum_extensional. intros vertex. exact (proj2 (agrees vertex)).
Qed.

Theorem union_peak_is_the_add_phase_cold_inventory :
  forall env budget schedule base added removed kept evidence rank union,
  trial_input_valid env base added removed -> edges_descend env rank ->
  trial env budget schedule base added removed = TrialResult kept (TrialPrepared evidence) ->
  coherent union ->
  quiescent (graph env) (universe env) (add_roots (base_roots base) added) union ->
  peak_objects (prepared_usage evidence) = inventory (universe env) union unit_weight /\
  peak_bytes (prepared_usage evidence) = inventory (universe env) union (object_bytes env).
Proof.
  intros env budget schedule base added removed kept evidence rank union valid descending
    prepared union_coherent union_quiescent.
  destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ prepared) as [_ [final [core ->]]].
  destruct (prepared_core_facts _ _ _ _ _ _ _ valid core)
    as [middle [first_execution [_ [_ [[middle_objects middle_bytes]
      [_ [peak_objects_final peak_bytes_final]]]]]]].
  pose proof valid as [unique [closed [agreement [balance [_ [added_domain _]]]]]].
  pose proof (@activation_initial_configuration_is_valid (graph env) (universe env)
    (base_roots base) (base_ledger base) added agreement balance added_domain)
    as start_valid.
  destruct (@drained_execution_has_exact_root_reachability (graph env) (universe env)
    (add_roots (base_roots base) added) Activation
    (Configuration (base_ledger base) added []) (resident (workspace middle)) rank
    unique closed descending start_valid first_execution)
    as [middle_coherent [middle_quiescent _]].
  pose proof (@quiescent_ledger_is_unique (graph env) (universe env)
    (add_roots (base_roots base) added) (resident (workspace middle)) union rank
    middle_coherent middle_quiescent union_coherent union_quiescent descending) as same.
  cbn [evidence_of prepared_usage].
  rewrite peak_objects_final, peak_bytes_final, middle_objects, middle_bytes.
  split; unfold inventory; apply selected_sum_extensional; intros vertex;
    exact (proj2 (same vertex)).
Qed.

Theorem checked_trial_is_the_guarded_unbounded_trial :
  forall env limits schedule base added removed evidence,
  trial env (Some limits) schedule base added removed =
    TrialResult base (TrialPrepared evidence) <->
  trial env None schedule base added removed = TrialResult base (TrialPrepared evidence) /\
  limits_cover limits evidence.
Proof.
  intros env limits schedule base added removed evidence. split.
  - intros bounded.
    destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ bounded) as [_ [final [core ->]]].
    destruct (core_bounded_prepared _ _ _ _ _ _ _ _ core) as [free [steps closing]].
    split.
    + unfold run_trial. rewrite free. reflexivity.
    + apply limits_cover_iff. split; assumption.
  - intros [free cover].
    destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ free) as [_ [final [core ->]]].
    apply limits_cover_iff in cover. destruct cover as [steps closing].
    unfold run_trial. rewrite (core_within_limits _ _ _ _ _ _ _ _ core steps closing).
    reflexivity.
Qed.

Theorem trial_accepts_at_exact_measured_limits :
  forall env schedule base added removed evidence,
  trial env None schedule base added removed = TrialResult base (TrialPrepared evidence) ->
  trial env (Some (exact_limits evidence)) schedule base added removed =
    TrialResult base (TrialPrepared evidence).
Proof.
  intros env schedule base added removed evidence free.
  apply checked_trial_is_the_guarded_unbounded_trial. split; [exact free|].
  intros axis. unfold exact_limits. lia.
Qed.

Theorem trial_refuses_one_below_on_that_axis :
  forall env schedule base added removed evidence axis,
  trial env None schedule base added removed = TrialResult base (TrialPrepared evidence) ->
  0 < measured evidence axis ->
  trial env (Some (with_limit (exact_limits evidence) axis (pred (measured evidence axis))))
    schedule base added removed = TrialResult base (TrialRefused (BudgetRefusal axis)).
Proof.
  intros env schedule base added removed evidence axis free positive.
  destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ free) as [_ [final [core ->]]].
  set (lowered := with_limit (exact_limits (evidence_of base added removed final)) axis
    (pred (measured (evidence_of base added removed final) axis))).
  assert (lowered_axis :
    lowered axis < measured (evidence_of base added removed final) axis).
  { unfold lowered, with_limit.
    destruct (budget_axis_eq_dec axis axis) as [_|different]; [lia|contradiction]. }
  assert (lowered_other : forall other, other <> axis ->
    lowered other = measured (evidence_of base added removed final) other).
  { intros other different. unfold lowered, with_limit, exact_limits.
    destruct (budget_axis_eq_dec other axis) as [same|_]; [contradiction|reflexivity]. }
  destruct (core_budget_split env Faithful lowered schedule base added removed final core)
    as [prepared|[refused_axis [at_refusal [refused cause]]]].
  - exfalso.
    destruct (core_bounded_prepared _ _ _ _ _ _ _ _ prepared) as [_ [steps closing]].
    assert (cover : limits_cover lowered (evidence_of base added removed final))
      by (apply limits_cover_iff; split; assumption).
    specialize (cover axis). lia.
  - assert (same_axis : refused_axis = axis).
    { destruct (budget_axis_eq_dec refused_axis axis) as [same|different]; [exact same|].
      exfalso. pose proof (lowered_other refused_axis different) as unchanged.
      destruct cause as [[member above]|closing].
      - unfold exceeds in above. apply Nat.ltb_lt in above.
        cbn [axis_limit axis_usage] in above. rewrite unchanged in above.
        destruct refused_axis;
          cbn [measured evidence_of prepared_usage prepared_objects prepared_bytes
            step_usage] in above; lia.
      - apply final_exceeded_some in closing.
        destruct closing as [[-> above]|[-> above]]; rewrite unchanged in above;
          cbn [measured evidence_of prepared_objects prepared_bytes] in above; lia. }
    subst refused_axis. unfold run_trial. rewrite refused. reflexivity.
Qed.

Theorem every_valid_authorized_trial_prepares_without_limits :
  forall env schedule base added removed,
  trial_input_valid env base added removed ->
  (forall vertex, occurrences removed vertex <= add_roots (base_roots base) added vertex) ->
  exists evidence,
    trial env None schedule base added removed = TrialResult base (TrialPrepared evidence).
Proof.
  intros env schedule base added removed valid bounded.
  destruct (core_prepares_without_limits env schedule base added removed valid bounded)
    as [final core].
  exists (evidence_of base added removed final). unfold run_trial. rewrite core.
  reflexivity.
Qed.

Theorem refused_trial_retries_from_identical_predecessor :
  forall env budget retry schedule base added removed why evidence,
  outcome (trial env budget schedule base added removed) = TrialRefused why ->
  trial env None schedule base added removed = TrialResult base (TrialPrepared evidence) ->
  limits_cover retry evidence ->
  trial env (Some retry) schedule (retained (trial env budget schedule base added removed))
    added removed = TrialResult base (TrialPrepared evidence).
Proof.
  intros env budget retry schedule base added removed why evidence _ free cover.
  rewrite (every_trial_retains_the_bit_identical_predecessor env Faithful budget schedule
    base added removed ltac:(discriminate)).
  apply checked_trial_is_the_guarded_unbounded_trial. split; assumption.
Qed.

Theorem live_scratch_is_bounded_by_its_reserved_peak :
  forall env limits schedule base added removed middle mode fuel index current,
  references (usage (activation_start Faithful base added)) = length added /\
  references (usage (deactivation_start Faithful middle removed)) =
    references (usage middle) + length removed /\
  (guard Faithful (Some limits) (activation_start Faithful base added) = None ->
     scratch_reserved limits (activation_start Faithful base added)) /\
  (guard Faithful (Some limits) (deactivation_start Faithful middle removed) = None ->
     scratch_reserved limits (deactivation_start Faithful middle removed)) /\
  (scratch_reserved limits current ->
     scratch_reserved limits (phase_result_state
       (run_phase env Faithful (Some limits) schedule mode fuel index current))).
Proof.
  intros env limits schedule base added removed middle mode fuel index current.
  assert (guarded_scratch : forall state, guard Faithful (Some limits) state = None ->
    peak_scratch (usage state) <= limits ScratchAxis).
  { intros state quiet. cbn [guard] in quiet. rewrite first_exceeded_none_iff in quiet.
    exact (quiet ScratchAxis ltac:(cbn [step_axes In]; auto 10)). }
  split; [reflexivity|]. split; [reflexivity|]. split; [|split].
  - intros quiet. split; [|exact (guarded_scratch _ quiet)].
    cbn [activation_start workspace usage peak_scratch charges_initialization]. lia.
  - intros quiet. split; [|exact (guarded_scratch _ quiet)].
    cbn [deactivation_start workspace usage peak_scratch charges_initialization]. lia.
  - intros reserved. apply (run_phase_invariant env Faithful (Some limits) schedule mode
      (scratch_reserved limits)); [|exact reserved].
    intros prefer before after _ step quiet. split.
    + exact (attempt_live_scratch_reserved _ _ _ _ _ _ step).
    + exact (guarded_scratch _ quiet).
Qed.

Theorem sound_quotes_bound_consumed_read_bytes :
  forall env limits schedule base added removed kept evidence,
  (forall vertex, consumed_read env vertex <= quoted_read env vertex) ->
  trial env (Some limits) schedule base added removed =
    TrialResult kept (TrialPrepared evidence) ->
  consumed_bytes (prepared_usage evidence) <= quoted_bytes (prepared_usage evidence) /\
  quoted_bytes (prepared_usage evidence) <= limits QuotedBytesAxis.
Proof.
  intros env limits schedule base added removed kept evidence sound prepared.
  destruct (trial_prepared_inversion _ _ _ _ _ _ _ _ prepared) as [_ [final [core ->]]].
  destruct (core_prepared_inversion _ _ _ _ _ _ _ _ core)
    as [_ [_ [middle [activated [_ [deactivated _]]]]]].
  destruct (core_bounded_prepared _ _ _ _ _ _ _ _ core) as [_ [steps _]].
  cbn [evidence_of prepared_usage]. split.
  - assert (covered : forall variant mode budget fuel index current last,
      run_phase env variant budget schedule mode fuel index current = PhaseDrained last ->
      consumed_bytes (usage current) <= quoted_bytes (usage current) ->
      consumed_bytes (usage last) <= quoted_bytes (usage last)).
    { intros variant mode budget fuel index current last drained_run start.
      pose proof (run_phase_invariant env variant budget schedule mode
        (fun state => consumed_bytes (usage state) <= quoted_bytes (usage state))
        (fun prefer before after before_covered step _ =>
           attempt_respects_sound_quotes env variant mode prefer before after sound
             before_covered step)
        fuel index current start) as kept_covered.
      rewrite drained_run in kept_covered. exact kept_covered. }
    apply (covered _ _ _ _ _ _ _ deactivated).
    cbn [deactivation_start usage consumed_bytes quoted_bytes].
    apply (covered _ _ _ _ _ _ _ activated).
    cbn [activation_start usage consumed_bytes quoted_bytes]. lia.
  - rewrite first_exceeded_none_iff in steps.
    exact (steps QuotedBytesAxis ltac:(cbn [step_axes In]; auto 10)).
Qed.

Definition no_edges (_ : nat) : list nat := [].

Definition single_edge (vertex : nat) : list nat := if vertex =? 1 then [0] else [].

Definition chain_edges (vertex : nat) : list nat :=
  match vertex with
  | 1 => [0]
  | 2 => [1]
  | 3 => [2]
  | _ => []
  end.

Definition no_reads (_ : nat) : nat := 0.

Definition fixture (edges : nat -> list nat) (vertices : list nat)
  (consumed : nat -> nat) : trial_environment :=
  TrialEnvironment edges vertices unit_weight no_reads consumed.

Definition edge_fixture : trial_environment := fixture single_edge [0; 1] no_reads.
Definition dup_fixture : trial_environment := fixture single_edge [0; 1; 2] no_reads.
Definition swap_fixture : trial_environment := fixture no_edges [0; 1] no_reads.
Definition chain_fixture : trial_environment := fixture chain_edges [0; 1; 2; 3] no_reads.
Definition isolated_fixture : trial_environment := fixture no_edges [0] no_reads.

Definition empty_predecessor : predecessor :=
  Predecessor {| counts := fun _ => 0; active := fun _ => false |} (fun _ => 0) 0 0.

Definition rooted_predecessor : predecessor :=
  Predecessor
    {| counts := fun vertex => if vertex =? 0 then 1 else 0;
       active := fun vertex => vertex =? 0 |}
    (fun vertex => if vertex =? 0 then 1 else 0) 1 1.

Definition prefer_cursors (_ : nat) : bool := true.

Definition generous (_ : budget_axis) : nat := 9.

Definition reference_limits : trial_limits := with_limit generous ReferenceAxis 1.

Definition swap_limits : trial_limits :=
  with_limit (with_limit generous UnionObjectsAxis 2) FinalObjectsAxis 1.

Definition scratch_limits : trial_limits := with_limit generous ScratchAxis 2.

Definition quote_limits : trial_limits := with_limit generous QuotedBytesAxis 0.

Theorem unsound_quote_admits_unbounded_consumption : forall excess,
  match trial (fixture no_edges [0] (fun _ => S excess)) (Some quote_limits) prefer_cursors
          empty_predecessor [0] [] with
  | TrialResult _ (TrialPrepared evidence) =>
      quoted_bytes (prepared_usage evidence) <= quote_limits QuotedBytesAxis /\
      excess < consumed_bytes (prepared_usage evidence)
  | TrialResult _ (TrialRefused _) => False
  end.
Proof. intros excess. vm_compute. lia. Qed.

Theorem partial_commit_mutant_replaces_the_predecessor :
  trial edge_fixture (Some reference_limits) prefer_cursors empty_predecessor [1] [] =
    TrialResult empty_predecessor (TrialRefused (BudgetRefusal ReferenceAxis)) /\
  outcome (run_trial edge_fixture PartialCommit (Some reference_limits) prefer_cursors
    empty_predecessor [1] []) = TrialRefused (BudgetRefusal ReferenceAxis) /\
  counts (base_ledger (retained (run_trial edge_fixture PartialCommit
    (Some reference_limits) prefer_cursors empty_predecessor [1] []))) 1 = 1 /\
  counts (base_ledger empty_predecessor) 1 = 0.
Proof. vm_compute. repeat split. Qed.

Theorem final_union_conflation_mutant_refuses_exact_final_limit :
  match trial swap_fixture (Some swap_limits) prefer_cursors rooted_predecessor [1] [0] with
  | TrialResult _ (TrialPrepared evidence) =>
      peak_objects (prepared_usage evidence) = 2 /\ prepared_objects evidence = 1
  | TrialResult _ (TrialRefused _) => False
  end /\
  outcome (run_trial swap_fixture FinalOnUnion (Some swap_limits) prefer_cursors
    rooted_predecessor [1] [0]) = TrialRefused (BudgetRefusal UnionObjectsAxis) /\
  outcome (trial swap_fixture (Some (with_limit swap_limits FinalObjectsAxis 0))
    prefer_cursors rooted_predecessor [1] [0]) =
    TrialRefused (BudgetRefusal FinalObjectsAxis).
Proof. vm_compute. repeat split. Qed.

Theorem scratch_allocation_conflation_mutant_refuses_admissible_trial :
  match trial chain_fixture (Some scratch_limits) prefer_cursors empty_predecessor [3] [] with
  | TrialResult _ (TrialPrepared evidence) =>
      peak_scratch (prepared_usage evidence) = 2 /\ expansions (prepared_usage evidence) = 4
  | TrialResult _ (TrialRefused _) => False
  end /\
  outcome (run_trial chain_fixture AllocationsAsScratch (Some scratch_limits) prefer_cursors
    empty_predecessor [3] []) = TrialRefused (BudgetRefusal ScratchAxis).
Proof. vm_compute. repeat split. Qed.

Theorem skipped_cursor_contribution_mutant_is_not_quiescent :
  match run_trial edge_fixture SkipContribution None prefer_cursors empty_predecessor [1] []
  with
  | TrialResult _ (TrialPrepared evidence) =>
      ~ quiescent single_edge [0; 1] (prepared_roots evidence) (prepared_ledger evidence)
  | TrialResult _ (TrialRefused _) => False
  end.
Proof.
  vm_compute. intros balanced. specialize (balanced 0). vm_compute in balanced.
  discriminate balanced.
Qed.

Theorem duplicated_cursor_contribution_mutant_is_not_quiescent :
  match run_trial dup_fixture DuplicateContribution None prefer_cursors empty_predecessor [1]
    [] with
  | TrialResult _ (TrialPrepared evidence) =>
      ~ quiescent single_edge [0; 1; 2] (prepared_roots evidence) (prepared_ledger evidence)
  | TrialResult _ (TrialRefused _) => False
  end.
Proof.
  vm_compute. intros balanced. specialize (balanced 0). vm_compute in balanced.
  discriminate balanced.
Qed.

Theorem uncharged_phase_initialization_mutant_overruns_scratch :
  outcome (trial isolated_fixture (Some scratch_limits) prefer_cursors rooted_predecessor
    [0; 0; 0] []) = TrialRefused (BudgetRefusal ScratchAxis) /\
  match run_trial isolated_fixture UnchargedInitialization (Some scratch_limits)
    prefer_cursors rooted_predecessor [0; 0; 0] [] with
  | TrialResult _ (TrialPrepared evidence) =>
      peak_scratch (prepared_usage evidence) <= scratch_limits ScratchAxis /\
      scratch_limits ScratchAxis < live_scratch
        (workspace (activation_start UnchargedInitialization rooted_predecessor [0; 0; 0]))
  | TrialResult _ (TrialRefused _) => False
  end.
Proof. vm_compute. split; [reflexivity|lia]. Qed.

End CheckedLedgerTrial.

Print Assumptions CheckedLedgerTrial.checked_attempts_are_exactly_scheduled_cursor_steps.
Print Assumptions CheckedLedgerTrial.checked_step_is_failure_atomic.
Print Assumptions CheckedLedgerTrial.accepted_event_reservations_are_checked_arithmetic.
Print Assumptions CheckedLedgerTrial.every_trial_retains_the_bit_identical_predecessor.
Print Assumptions CheckedLedgerTrial.prepared_trial_agrees_with_cold_ledger.
Print Assumptions CheckedLedgerTrial.union_peak_is_the_add_phase_cold_inventory.
Print Assumptions CheckedLedgerTrial.checked_trial_is_the_guarded_unbounded_trial.
Print Assumptions CheckedLedgerTrial.trial_accepts_at_exact_measured_limits.
Print Assumptions CheckedLedgerTrial.trial_refuses_one_below_on_that_axis.
Print Assumptions CheckedLedgerTrial.every_valid_authorized_trial_prepares_without_limits.
Print Assumptions CheckedLedgerTrial.refused_trial_retries_from_identical_predecessor.
Print Assumptions CheckedLedgerTrial.live_scratch_is_bounded_by_its_reserved_peak.
Print Assumptions CheckedLedgerTrial.sound_quotes_bound_consumed_read_bytes.
Print Assumptions CheckedLedgerTrial.unsound_quote_admits_unbounded_consumption.
Print Assumptions CheckedLedgerTrial.partial_commit_mutant_replaces_the_predecessor.
Print Assumptions CheckedLedgerTrial.final_union_conflation_mutant_refuses_exact_final_limit.
Print Assumptions CheckedLedgerTrial.scratch_allocation_conflation_mutant_refuses_admissible_trial.
Print Assumptions CheckedLedgerTrial.skipped_cursor_contribution_mutant_is_not_quiescent.
Print Assumptions CheckedLedgerTrial.duplicated_cursor_contribution_mutant_is_not_quiescent.
Print Assumptions CheckedLedgerTrial.uncharged_phase_initialization_mutant_overruns_scratch.
