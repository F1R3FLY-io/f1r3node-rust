From Stdlib Require Import Bool List Arith Lia.
From StackSafePDA Require Import
  PathMapPhysicalOwnership
  PathMapReversePruneTeardown
  PathMapCanonicalStreamPublication
  TwoLevelRSpaceAuthority
  TwoLevelRSpaceOperations
  TwoLevelRSpaceRepresentation.

Import ListNotations.
Set Implicit Arguments.

(** Operational ownership composition for an outer channel PathMap whose real
    values are independently guarded inner Bag PathMaps.

    The model deliberately separates three authorities:

    - the immutable outer PathMap anchor;
    - each independently reference-counted inner Bag anchor; and
    - the explicit teardown control stack.

    A final outer release may temporarily suspend its outer frame while a
    final inner Bag release runs.  No inner transition can enter another inner
    frame.  The native stack bound of two is therefore derived from reachable
    transitions rather than asserted as a constant or controlled by fuel. *)
Module TwoLevelRSpaceGuardedOwnership.

Module PO := PathMapPhysicalOwnership.
Module TD := PathMapReversePruneTeardown.
Module CP := PathMapCanonicalStreamPublication.
Module RA := TwoLevelRSpaceAuthority.
Module RO := TwoLevelRSpaceOperations.
Module RR := TwoLevelRSpaceRepresentation.

Record bag_cell : Type := BagCell {
  bag_anchor_id : nat;
  bag_physical_anchor : PO.physical_anchor;
  bag_allocation_region : PO.allocation_region;
  bag_lifecycle : TD.anchor_state;
  bag_logical_tree : TD.trie;
  bag_final_teardown_count : nat
}.

Definition bag_cell_consistent (cell : bag_cell) : Prop :=
  PO.anchor_identity (bag_physical_anchor cell) = bag_anchor_id cell /\
  PO.anchor_arc_wrappers (bag_physical_anchor cell) =
    TD.anchor_wrapper_arcs (bag_lifecycle cell) /\
  PO.anchor_raw_headers (bag_physical_anchor cell) =
    TD.anchor_raw_headers (bag_lifecycle cell) /\
  PO.anchor_nodes (bag_physical_anchor cell) =
    PO.region_nodes (bag_allocation_region cell) /\
  TD.anchor_well_formed (bag_lifecycle cell) /\
  bag_final_teardown_count cell <= 1.

Definition shallow_clone_anchor (anchor : PO.physical_anchor) :
    PO.physical_anchor := PO.shared_arc_clone anchor.

Theorem shallow_clone_changes_only_the_arc_wrapper_count : forall anchor,
  PO.anchor_identity (shallow_clone_anchor anchor) =
    PO.anchor_identity anchor /\
  PO.anchor_root_mode (shallow_clone_anchor anchor) =
    PO.anchor_root_mode anchor /\
  PO.anchor_nodes (shallow_clone_anchor anchor) = PO.anchor_nodes anchor /\
  PO.anchor_logical_paths (shallow_clone_anchor anchor) =
    PO.anchor_logical_paths anchor /\
  PO.anchor_raw_headers (shallow_clone_anchor anchor) =
    PO.anchor_raw_headers anchor /\
  PO.anchor_arc_wrappers (shallow_clone_anchor anchor) =
    S (PO.anchor_arc_wrappers anchor).
Proof. intro anchor. repeat split; reflexivity. Qed.

Theorem shallow_real_slot_clone_creates_no_inner_raw_storage : forall cell,
  PO.anchor_raw_headers
    (shallow_clone_anchor (bag_physical_anchor cell)) =
      PO.anchor_raw_headers (bag_physical_anchor cell) /\
  PO.anchor_nodes
    (shallow_clone_anchor (bag_physical_anchor cell)) =
      PO.anchor_nodes (bag_physical_anchor cell) /\
  PO.anchor_logical_paths
    (shallow_clone_anchor (bag_physical_anchor cell)) =
      PO.anchor_logical_paths (bag_physical_anchor cell).
Proof.
  intro cell. apply PO.shared_arc_clone_creates_no_raw_owner.
Qed.

Theorem nonfinal_wrapper_release_cannot_touch_raw_storage : forall others,
  TD.anchor_transition_trace (TD.nonlast_wrapper_drop others) =
    [TD.DropNonlastWrapperArc] /\
  TD.anchor_raw_headers
    (TD.anchor_transition_final (TD.nonlast_wrapper_drop others)) = 1 /\
  TD.anchor_root_payload_copies
    (TD.anchor_transition_final (TD.nonlast_wrapper_drop others)) = 1.
Proof. apply TD.nonlast_wrapper_drop_cannot_touch_the_raw_pathmap. Qed.

Theorem make_mut_registers_exactly_one_well_formed_fresh_anchor : forall others,
  TD.anchor_well_formed
    (TD.make_mut_old_final (TD.shared_arc_make_mut others)) /\
  TD.anchor_well_formed
    (TD.make_mut_fresh_final (TD.shared_arc_make_mut others)) /\
  TD.anchor_raw_headers
    (TD.make_mut_fresh_final (TD.shared_arc_make_mut others)) = 1 /\
  TD.anchor_root_payload_copies
    (TD.make_mut_fresh_final (TD.shared_arc_make_mut others)) = 1.
Proof. apply TD.make_mut_creates_one_well_formed_fresh_anchor. Qed.

(** A release-plan entry is ghost evidence derived at final outer teardown
    from stable Arc ownership.  [None] means this outer value releases a
    nonfinal Bag wrapper.  [Some tree] means it elects the final wrapper and
    must drain exactly that inner Bag tree. *)
Record release_plan_entry : Type := ReleasePlanEntry {
  planned_bag_id : nat;
  planned_final_tree : option TD.trie
}.

Definition release_plan : Type := list release_plan_entry.

Fixpoint lookup_release_plan
    (selected : nat) (plan : release_plan) : option (option TD.trie) :=
  match plan with
  | [] => None
  | entry :: tail =>
      if Nat.eq_dec selected (planned_bag_id entry)
      then Some (planned_final_tree entry)
      else lookup_release_plan selected tail
  end.

Definition release_plan_unique (plan : release_plan) : Prop :=
  NoDup (map planned_bag_id plan).

Inductive ownership_event : Type :=
| OuterPdaEvent : TD.teardown_event -> ownership_event
| ReleaseRealSlot : nat -> ownership_event
| BeginInnerPda : nat -> ownership_event
| InnerPdaEvent : nat -> TD.teardown_event -> ownership_event
| EndInnerPda : nat -> ownership_event
| CompleteOuterPda : ownership_event.

Fixpoint lift_inner_trace
    (bag_id : nat) (trace : list TD.teardown_event) : list ownership_event :=
  match trace with
  | [] => []
  | event :: tail =>
      InnerPdaEvent bag_id event :: lift_inner_trace bag_id tail
  end.

Definition final_inner_segment
    (bag_id : nat) (tree : TD.trie) : list ownership_event :=
  BeginInnerPda bag_id ::
  lift_inner_trace bag_id (TD.terminal_two_phase_trace tree) ++
  [EndInnerPda bag_id].

Definition expand_outer_event
    (plan : release_plan) (event : TD.teardown_event) : list ownership_event :=
  match event with
  | TD.DisposeValue (TD.CandidatePosition bag_id) =>
      ReleaseRealSlot bag_id ::
      match lookup_release_plan bag_id plan with
      | Some (Some tree) => final_inner_segment bag_id tree
      | _ => []
      end
  | _ => [OuterPdaEvent event]
  end.

Fixpoint expand_outer_trace
    (plan : release_plan) (trace : list TD.teardown_event)
    : list ownership_event :=
  match trace with
  | [] => []
  | event :: tail =>
      expand_outer_event plan event ++ expand_outer_trace plan tail
  end.

Definition generated_teardown_schedule
    (outer : TD.trie) (plan : release_plan) : list ownership_event :=
  expand_outer_trace plan (TD.terminal_two_phase_trace outer) ++
  [CompleteOuterPda].

Fixpoint candidate_position_ids (payloads : list TD.payload) : list nat :=
  match payloads with
  | [] => []
  | TD.CandidatePosition bag_id :: tail =>
      bag_id :: candidate_position_ids tail
  | _ :: tail => candidate_position_ids tail
  end.

Fixpoint released_real_slot_ids (events : list ownership_event) : list nat :=
  match events with
  | [] => []
  | ReleaseRealSlot bag_id :: tail =>
      bag_id :: released_real_slot_ids tail
  | _ :: tail => released_real_slot_ids tail
  end.

Fixpoint begun_inner_ids (events : list ownership_event) : list nat :=
  match events with
  | [] => []
  | BeginInnerPda bag_id :: tail => bag_id :: begun_inner_ids tail
  | _ :: tail => begun_inner_ids tail
  end.

Fixpoint inner_disposed_payloads
    (selected : nat) (events : list ownership_event) : list TD.payload :=
  match events with
  | [] => []
  | InnerPdaEvent bag_id (TD.DisposeValue payload) :: tail =>
      if Nat.eq_dec selected bag_id
      then payload :: inner_disposed_payloads selected tail
      else inner_disposed_payloads selected tail
  | _ :: tail => inner_disposed_payloads selected tail
  end.

Lemma released_real_slots_app : forall left right,
  released_real_slot_ids (left ++ right) =
    released_real_slot_ids left ++ released_real_slot_ids right.
Proof.
  induction left as [|event tail IH]; intro right; simpl; [reflexivity |].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma begun_inner_ids_app : forall left right,
  begun_inner_ids (left ++ right) =
    begun_inner_ids left ++ begun_inner_ids right.
Proof.
  induction left as [|event tail IH]; intro right; simpl; [reflexivity |].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma inner_disposed_payloads_app : forall selected left right,
  inner_disposed_payloads selected (left ++ right) =
    inner_disposed_payloads selected left ++
    inner_disposed_payloads selected right.
Proof.
  intros selected left. induction left as [|event tail IH]; intro right;
    simpl; [reflexivity |].
  destruct event as
      [outer_event | released | begun | bag_id inner_event | ended |]; simpl.
  - apply IH.
  - apply IH.
  - apply IH.
  - destruct inner_event as
        [payload | payload | | | | payload | par_id children |
         par_id child | | width | | |].
    + simpl. apply IH.
    + simpl. destruct (Nat.eq_dec selected bag_id); simpl;
        rewrite (IH right); reflexivity.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
    + simpl. apply IH.
  - apply IH.
  - apply IH.
Qed.

Lemma lifted_inner_trace_releases_no_outer_slot : forall bag_id trace,
  released_real_slot_ids (lift_inner_trace bag_id trace) = [].
Proof.
  intros bag_id trace. induction trace; simpl; [reflexivity |].
  now rewrite IHtrace.
Qed.

Lemma lifted_inner_trace_begins_no_nested_inner : forall bag_id trace,
  begun_inner_ids (lift_inner_trace bag_id trace) = [].
Proof.
  intros bag_id trace. induction trace; simpl; [reflexivity |].
  now rewrite IHtrace.
Qed.

Lemma lifted_inner_trace_preserves_disposed_payloads : forall bag_id trace,
  inner_disposed_payloads bag_id (lift_inner_trace bag_id trace) =
    TD.disposed_values trace.
Proof.
  intros bag_id trace. induction trace as [|event tail IH];
    simpl; [reflexivity |].
  destruct event; simpl; try exact IH.
  destruct (Nat.eq_dec bag_id bag_id); [now rewrite IH | contradiction].
Qed.

Lemma expanded_event_releases_exact_candidate_position : forall plan event,
  released_real_slot_ids (expand_outer_event plan event) =
    candidate_position_ids (TD.disposed_values [event]).
Proof.
  intros plan event. destruct event; simpl; try reflexivity.
  destruct p; simpl; try reflexivity.
  destruct (lookup_release_plan n plan) as [[tree|]|] eqn:Hlookup;
    simpl; try reflexivity.
  unfold final_inner_segment. simpl.
  rewrite released_real_slots_app,
    lifted_inner_trace_releases_no_outer_slot. reflexivity.
Qed.

Theorem expanded_outer_trace_releases_every_real_slot_occurrence_once :
  forall plan trace,
    released_real_slot_ids (expand_outer_trace plan trace) =
    candidate_position_ids (TD.disposed_values trace).
Proof.
  intros plan trace. induction trace as [|event tail IH];
    simpl; [reflexivity |].
  rewrite released_real_slots_app,
    expanded_event_releases_exact_candidate_position, IH.
  destruct event; simpl; try reflexivity.
  destruct p; reflexivity.
Qed.

Theorem terminal_outer_teardown_hands_off_every_real_slot_occurrence_once :
  forall outer plan,
    released_real_slot_ids (generated_teardown_schedule outer plan) =
    candidate_position_ids (TD.preorder_payloads_tree outer).
Proof.
  intros outer plan. unfold generated_teardown_schedule.
  rewrite released_real_slots_app. simpl. rewrite app_nil_r.
  rewrite expanded_outer_trace_releases_every_real_slot_occurrence_once.
  pose proof
    (TD.terminal_two_phase_trace_is_exact_and_topology_complete outer)
    as [_ [Hdisposed _]].
  now rewrite Hdisposed.
Qed.

Theorem final_inner_segment_preserves_complete_occurrence_sequence : forall
    bag_id tree,
  inner_disposed_payloads bag_id (final_inner_segment bag_id tree) =
    TD.preorder_payloads_tree tree.
Proof.
  intros bag_id tree. unfold final_inner_segment. simpl.
  rewrite inner_disposed_payloads_app. simpl. rewrite app_nil_r.
  rewrite lifted_inner_trace_preserves_disposed_payloads.
  pose proof
    (TD.terminal_two_phase_trace_is_exact_and_topology_complete tree)
    as [_ [Hdisposed _]].
  exact Hdisposed.
Qed.

Inductive control_frame : Type :=
| OuterFrame : control_frame
| InnerFrame : nat -> control_frame.

Inductive control_outcome : Type :=
| ContinueWith : list control_frame -> control_outcome
| TeardownComplete : control_outcome.

Definition drive_control
    (control : list control_frame) (event : ownership_event)
    : option control_outcome :=
  match control, event with
  | [OuterFrame], OuterPdaEvent _ => Some (ContinueWith [OuterFrame])
  | [OuterFrame], ReleaseRealSlot _ => Some (ContinueWith [OuterFrame])
  | [OuterFrame], BeginInnerPda bag_id =>
      Some (ContinueWith [InnerFrame bag_id; OuterFrame])
  | [InnerFrame active; OuterFrame], InnerPdaEvent selected _ =>
      if Nat.eq_dec active selected
      then Some (ContinueWith [InnerFrame active; OuterFrame])
      else None
  | [InnerFrame active; OuterFrame], EndInnerPda selected =>
      if Nat.eq_dec active selected
      then Some (ContinueWith [OuterFrame])
      else None
  | [OuterFrame], CompleteOuterPda => Some TeardownComplete
  | _, _ => None
  end.

Fixpoint drive_schedule
    (control : list control_frame) (events : list ownership_event)
    : option control_outcome :=
  match events with
  | [] => Some (ContinueWith control)
  | event :: tail =>
      match drive_control control event with
      | Some (ContinueWith next) => drive_schedule next tail
      | Some TeardownComplete =>
          match tail with
          | [] => Some TeardownComplete
          | _ :: _ => None
          end
      | None => None
      end
  end.

Lemma drive_lifted_inner_trace : forall bag_id trace,
  drive_schedule [InnerFrame bag_id; OuterFrame]
    (lift_inner_trace bag_id trace) =
  Some (ContinueWith [InnerFrame bag_id; OuterFrame]).
Proof.
  intros bag_id trace. induction trace as [|event tail IH];
    simpl; [reflexivity |].
  destruct (Nat.eq_dec bag_id bag_id); [exact IH | contradiction].
Qed.

Lemma drive_lifted_inner_trace_then : forall bag_id trace suffix,
  drive_schedule [InnerFrame bag_id; OuterFrame]
    (lift_inner_trace bag_id trace ++ suffix) =
  drive_schedule [InnerFrame bag_id; OuterFrame] suffix.
Proof.
  intros bag_id trace. induction trace as [|event tail IH];
    intro suffix; simpl; [reflexivity |].
  destruct (Nat.eq_dec bag_id bag_id); [apply IH | contradiction].
Qed.

Lemma drive_final_inner_segment : forall bag_id tree,
  drive_schedule [OuterFrame] (final_inner_segment bag_id tree) =
  Some (ContinueWith [OuterFrame]).
Proof.
  intros bag_id tree. unfold final_inner_segment. simpl.
  rewrite drive_lifted_inner_trace_then. simpl.
  destruct (Nat.eq_dec bag_id bag_id); [reflexivity | contradiction].
Qed.

Lemma drive_expanded_outer_event : forall plan event,
  drive_schedule [OuterFrame] (expand_outer_event plan event) =
  Some (ContinueWith [OuterFrame]).
Proof.
  intros plan event. destruct event; simpl; try reflexivity.
  destruct p; simpl; try reflexivity.
  destruct (lookup_release_plan n plan) as [[tree|]|] eqn:Hlookup;
    simpl; try reflexivity.
  apply drive_final_inner_segment.
Qed.

Lemma drive_schedule_app_continue : forall control middle left right,
  drive_schedule control left = Some (ContinueWith middle) ->
  drive_schedule middle right = drive_schedule control (left ++ right).
Proof.
  intros control middle left. revert control middle.
  induction left as [|event tail IH]; intros control middle right Hleft;
    simpl in *.
  - inversion Hleft. reflexivity.
  - destruct (drive_control control event) as [[next|]|] eqn:Hdrive.
    + eapply IH. exact Hleft.
    + destruct tail; discriminate.
    + discriminate.
Qed.

Lemma drive_expanded_outer_trace : forall plan trace,
  drive_schedule [OuterFrame] (expand_outer_trace plan trace) =
  Some (ContinueWith [OuterFrame]).
Proof.
  intros plan trace. induction trace as [|event tail IH];
    simpl; [reflexivity |].
  pose proof
    (@drive_schedule_app_continue
      [OuterFrame] [OuterFrame]
      (expand_outer_event plan event)
      (expand_outer_trace plan tail)
      (drive_expanded_outer_event plan event)) as Happ.
  rewrite <- Happ. exact IH.
Qed.

Theorem every_generated_teardown_schedule_completes : forall outer plan,
  drive_schedule [OuterFrame] (generated_teardown_schedule outer plan) =
    Some TeardownComplete.
Proof.
  intros outer plan. unfold generated_teardown_schedule.
  pose proof
    (@drive_schedule_app_continue
      [OuterFrame] [OuterFrame]
      (expand_outer_trace plan (TD.terminal_two_phase_trace outer))
      [CompleteOuterPda]
      (drive_expanded_outer_trace plan
        (TD.terminal_two_phase_trace outer))) as Happ.
  rewrite <- Happ. reflexivity.
Qed.

Inductive machine_status : Type :=
| MachineRunning
| MachineComplete
| MachineFailStopped.

Record machine_state : Type := MachineState {
  machine_pending : list ownership_event;
  machine_control : list control_frame;
  machine_status_value : machine_status;
  machine_live_observation : nat;
  machine_snapshot_observations : list nat;
  machine_publication_count : nat;
  machine_recursive_raw_drop_count : nat
}.

Definition initial_machine
    (outer : TD.trie) (plan : release_plan)
    (live_observation : nat) (snapshots : list nat)
    : machine_state :=
  MachineState
    (generated_teardown_schedule outer plan)
    [OuterFrame]
    MachineRunning
    live_observation
    snapshots
    0
    0.

Inductive machine_step : machine_state -> machine_state -> Prop :=
| DriveMachine : forall pending event rest control next live snapshots pubs raw,
    pending = event :: rest ->
    drive_control control event = Some (ContinueWith next) ->
    machine_step
      (MachineState pending control MachineRunning live snapshots pubs raw)
      (MachineState rest next MachineRunning live snapshots pubs raw)
| CompleteMachine : forall event control live snapshots pubs raw,
    drive_control control event = Some TeardownComplete ->
    machine_step
      (MachineState [event] control MachineRunning live snapshots pubs raw)
      (MachineState [] [] MachineComplete live snapshots pubs raw)
| FailStopMachine : forall event rest control live snapshots pubs raw,
    machine_step
      (MachineState (event :: rest) control MachineRunning
        live snapshots pubs raw)
      (MachineState [] [] MachineFailStopped live snapshots pubs raw).

Inductive reachable_from (initial : machine_state) : machine_state -> Prop :=
| ReachableInitial : reachable_from initial initial
| ReachableStep : forall before after,
    reachable_from initial before ->
    machine_step before after ->
    reachable_from initial after.

Definition control_shape (control : list control_frame) : Prop :=
  control = [] \/
  control = [OuterFrame] \/
  exists bag_id, control = [InnerFrame bag_id; OuterFrame].

Lemma drive_control_preserves_bounded_shape : forall control event outcome,
  control_shape control ->
  drive_control control event = Some outcome ->
  match outcome with
  | ContinueWith next => control_shape next
  | TeardownComplete => True
  end.
Proof.
  intros control event outcome Hshape Hdrive.
  destruct Hshape as [Hempty | [Houter | [bag_id Hinner]]].
  - subst control. destruct event; discriminate.
  - subst control. destruct event; simpl in Hdrive.
    + inversion Hdrive; subst. right. left. reflexivity.
    + inversion Hdrive; subst. right. left. reflexivity.
    + inversion Hdrive; subst. right. right. eexists. reflexivity.
    + discriminate.
    + discriminate.
    + inversion Hdrive; subst. exact I.
  - subst control. destruct event; simpl in Hdrive; try discriminate.
    + destruct (Nat.eq_dec bag_id n); try discriminate.
      inversion Hdrive; subst. right. right. eexists. reflexivity.
    + destruct (Nat.eq_dec bag_id n); try discriminate.
      inversion Hdrive; subst. right. left. reflexivity.
Qed.

Theorem every_reachable_machine_has_at_most_two_control_frames : forall
    outer plan live snapshots state,
  reachable_from (initial_machine outer plan live snapshots) state ->
  length (machine_control state) <= 2.
Proof.
  intros outer plan live snapshots state Hreachable.
  assert (Hshape : control_shape (machine_control state)).
  { induction Hreachable as [|before after Hbefore IH Hstep].
    - right. left. reflexivity.
    - inversion Hstep; subst; simpl.
      + pose proof
          (@drive_control_preserves_bounded_shape
            control event (ContinueWith next) IH H0) as Hnext.
        exact Hnext.
      + left. reflexivity.
      + left. reflexivity.
  }
  destruct Hshape as [-> | [-> | [bag_id ->]]]; simpl; lia.
Qed.

Theorem every_live_machine_step_strictly_decreases_ghost_work : forall
    before after,
  machine_step before after ->
  length (machine_pending after) < length (machine_pending before).
Proof.
  intros before after Hstep. inversion Hstep; subst; simpl; lia.
Qed.

Theorem terminal_machine_states_have_no_successor : forall state,
  machine_status_value state <> MachineRunning ->
  forall after, ~ machine_step state after.
Proof.
  intros [pending control status live snapshots pubs raw] Hterminal after Hstep.
  inversion Hstep; subst; contradiction.
Qed.

Theorem every_machine_step_preserves_live_and_snapshot_observations : forall
    before after,
  machine_step before after ->
  machine_live_observation after = machine_live_observation before /\
  machine_snapshot_observations after =
    machine_snapshot_observations before.
Proof. intros before after Hstep. inversion Hstep; subst; split; reflexivity. Qed.

Theorem teardown_never_publishes_or_recursively_drops_raw_pathmaps : forall
    before after,
  machine_step before after ->
  machine_publication_count after = machine_publication_count before /\
  machine_recursive_raw_drop_count after =
    machine_recursive_raw_drop_count before.
Proof. intros before after Hstep. inversion Hstep; subst; split; reflexivity. Qed.

Theorem fail_stop_is_terminal_without_partial_publication_or_recursive_drop :
  forall live snapshots pubs raw,
  let failed :=
    MachineState [] [] MachineFailStopped live snapshots pubs raw in
  machine_live_observation failed = live /\
  machine_snapshot_observations failed = snapshots /\
  machine_publication_count failed = pubs /\
  machine_recursive_raw_drop_count failed = raw /\
  forall after, ~ machine_step failed after.
Proof.
  intros. simpl. repeat split; try reflexivity.
  intros after Hstep. inversion Hstep.
Qed.

Theorem arbitrary_finite_outer_and_inner_depth_has_the_same_native_bound :
  forall outer plan live snapshots state,
  reachable_from (initial_machine outer plan live snapshots) state ->
  length (machine_control state) <= 2.
Proof. apply every_reachable_machine_has_at_most_two_control_frames. Qed.

(** Final-inner election is a filter over outer real-slot occurrences.  This
    makes the distinction between "dispose every wrapper occurrence once" and
    "tear down an underlying Bag anchor after its final wrapper once"
    explicit. *)
Definition release_plan_elects_final
    (plan : release_plan) (bag_id : nat) : bool :=
  match lookup_release_plan bag_id plan with
  | Some (Some _) => true
  | _ => false
  end.

Definition finally_elected_ids
    (plan : release_plan) (ids : list nat) : list nat :=
  filter (release_plan_elects_final plan) ids.

Lemma final_inner_segment_begins_exactly_once : forall bag_id tree,
  begun_inner_ids (final_inner_segment bag_id tree) = [bag_id].
Proof.
  intros bag_id tree. unfold final_inner_segment. simpl.
  rewrite begun_inner_ids_app,
    lifted_inner_trace_begins_no_nested_inner. reflexivity.
Qed.

Lemma expanded_outer_event_begins_exactly_the_final_candidate : forall
    plan event,
  begun_inner_ids (expand_outer_event plan event) =
  finally_elected_ids plan
    (candidate_position_ids (TD.disposed_values [event])).
Proof.
  intros plan event. destruct event; simpl; try reflexivity.
  destruct p; simpl; try reflexivity.
  unfold release_plan_elects_final.
  destruct (lookup_release_plan n plan) as [[tree|]|] eqn:Hlookup;
    simpl; try reflexivity.
  apply final_inner_segment_begins_exactly_once.
Qed.

Theorem expanded_outer_trace_begins_exactly_final_inner_anchors : forall
    plan trace,
  begun_inner_ids (expand_outer_trace plan trace) =
  finally_elected_ids plan (candidate_position_ids (TD.disposed_values trace)).
Proof.
  intros plan trace. induction trace as [|event tail IH];
    simpl; [reflexivity |].
  rewrite begun_inner_ids_app,
    expanded_outer_event_begins_exactly_the_final_candidate, IH.
  destruct event; simpl; try reflexivity.
  destruct p; simpl; try reflexivity.
  destruct (release_plan_elects_final plan n); reflexivity.
Qed.

Lemma filter_preserves_nat_nodup : forall
    (predicate : nat -> bool) (ids : list nat),
  NoDup ids -> NoDup (filter predicate ids).
Proof.
  intros predicate ids Hunique. induction Hunique as [|head tail Hnotin Htail IH];
    simpl; [constructor |].
  destruct (predicate head) eqn:Hkeep.
  - constructor; [|exact IH].
    intro Hin. apply filter_In in Hin as [Hin _]. contradiction.
  - exact IH.
Qed.

Theorem final_inner_anchor_is_elected_at_most_once : forall plan trace,
  NoDup (candidate_position_ids (TD.disposed_values trace)) ->
  NoDup (begun_inner_ids (expand_outer_trace plan trace)).
Proof.
  intros plan trace Hunique.
  rewrite expanded_outer_trace_begins_exactly_final_inner_anchors.
  now apply filter_preserves_nat_nodup.
Qed.

Definition every_released_wrapper_is_final
    (plan : release_plan) (ids : list nat) : Prop :=
  forall bag_id, In bag_id ids -> release_plan_elects_final plan bag_id = true.

Lemma filter_all_true_is_identity : forall
    (predicate : nat -> bool) (ids : list nat),
  (forall value, In value ids -> predicate value = true) ->
  filter predicate ids = ids.
Proof.
  intros predicate ids. induction ids as [|head tail IH]; intro Hall;
    simpl; [reflexivity |].
  rewrite Hall by (left; reflexivity). f_equal. apply IH.
  intros value Hin. apply Hall. right. exact Hin.
Qed.

Theorem every_final_wrapper_elects_exactly_one_inner_teardown : forall
    plan trace,
  every_released_wrapper_is_final plan
    (candidate_position_ids (TD.disposed_values trace)) ->
  begun_inner_ids (expand_outer_trace plan trace) =
    candidate_position_ids (TD.disposed_values trace).
Proof.
  intros plan trace Hall.
  rewrite expanded_outer_trace_begins_exactly_final_inner_anchors.
  now apply filter_all_true_is_identity.
Qed.

Theorem inner_control_can_never_enter_another_inner_frame : forall
    active event outcome,
  drive_control [InnerFrame active; OuterFrame] event = Some outcome ->
  match outcome with
  | ContinueWith next =>
      next = [InnerFrame active; OuterFrame] \/ next = [OuterFrame]
  | TeardownComplete => False
  end.
Proof.
  intros active event outcome Hdrive.
  destruct event; simpl in Hdrive; try discriminate.
  - destruct (Nat.eq_dec active n); try discriminate.
    inversion Hdrive; subst. left. reflexivity.
  - destruct (Nat.eq_dec active n); try discriminate.
    inversion Hdrive; subst. right. reflexivity.
Qed.

Theorem reachable_terminal_state_closes_every_cursor_and_barrier : forall
    outer plan live snapshots state,
  reachable_from (initial_machine outer plan live snapshots) state ->
  machine_status_value state <> MachineRunning ->
  machine_pending state = [] /\ machine_control state = [].
Proof.
  intros outer plan live snapshots state Hreachable Hterminal.
  induction Hreachable as [|before after Hbefore IH Hstep].
  - exfalso. apply Hterminal. reflexivity.
  - inversion Hstep; subst; simpl in *.
    + contradiction.
    + split; reflexivity.
    + split; reflexivity.
Qed.

Inductive gated_publication : list control_frame -> Prop :=
| PublishWithNoMutationLease : gated_publication [].

Theorem publication_is_unrepresentable_with_a_live_mutation_lease : forall
    control,
  gated_publication control -> control = [].
Proof. intros control Hpublish. inversion Hpublish. reflexivity. Qed.

Theorem outer_clone_trace_is_one_constant_time_wrapper_event : forall others,
  TD.anchor_transition_trace (TD.immutable_wrapper_clone others) =
    [TD.CloneWrapperArc].
Proof. reflexivity. Qed.

Definition outer_clone_bag_heap (heap : list bag_cell) : list bag_cell := heap.

Theorem outer_clone_does_not_clone_any_inner_bag : forall heap,
  outer_clone_bag_heap heap = heap.
Proof. reflexivity. Qed.

Theorem exact_value_update_preserves_external_alias : forall
    write alias_region alias,
  PO.value_update_write_isolated write alias_region ->
  PO.cow_value_update_preserves_alias write alias_region alias alias.
Proof. apply PO.isolated_cow_value_update_preserves_external_alias. Qed.

Theorem structural_insertion_is_fully_preflighted : forall
    source additions candidate,
  CP.native_insertion_preflight source additions = Some candidate ->
  exists result,
    CP.NativeCowInsertionRefines source additions result /\ result = candidate.
Proof. apply CP.native_insertion_has_no_semantic_failure_after_preflight. Qed.

Theorem structural_removal_uses_an_ownership_disjoint_rebuild : forall
    (source_anchor candidate_anchor : nat)
    (source_paths candidate_paths : list PO.path)
    (source_region candidate_region : PO.allocation_region),
  PO.allocation_region_well_formed source_region ->
  PO.allocation_region_well_formed candidate_region ->
  PO.region_covers_rebuild_root source_paths source_region ->
  PO.region_covers_rebuild_root candidate_paths candidate_region ->
  PO.region_allocation source_region <> PO.region_allocation candidate_region ->
  PO.structural_candidate_disjoint source_region candidate_region.
Proof.
  intros. pose proof
    (@PO.independent_region_rebuilds_are_structurally_disjoint
      source_anchor source_paths source_region
      candidate_anchor candidate_paths candidate_region
      H H0 H1 H2 H3) as [Hdisjoint _].
  exact Hdisjoint.
Qed.

Theorem abort_preserves_live_authority_snapshots_and_aliases : forall world,
  CP.world_live_authority (CP.abort_world_before_publication world) =
    CP.world_live_authority world /\
  CP.world_external_aliases (CP.abort_world_before_publication world) =
    CP.world_external_aliases world /\
  CP.world_staged_candidate (CP.abort_world_before_publication world) = None.
Proof. apply CP.abort_preserves_live_authority_and_external_aliases. Qed.

Theorem publication_swaps_once_and_retires_the_old_handle_once : forall
    world new_root,
  CP.world_live_authority (CP.publish_world world new_root) = new_root /\
  CP.world_disposed_root_handles (CP.publish_world world new_root) =
    CP.atomic_root_identity (CP.world_live_authority world) ::
      CP.world_disposed_root_handles world.
Proof.
  intros. split.
  - exact (proj1 (CP.publish_is_atomic_and_preserves_external_aliases
      world new_root)).
  - apply CP.publish_disposes_old_live_handle_exactly_once_at_head.
Qed.

Theorem abort_cannot_dispose_borrowed_candidate_storage : forall
    world candidate_identity footprint identity,
  CP.world_staged_candidate world = Some (candidate_identity, footprint) ->
  In identity
    (CP.footprint_node_identities (CP.candidate_borrowed_footprint footprint)) ->
  ~ In identity
    (CP.footprint_node_identities (CP.candidate_private_footprint footprint)).
Proof. apply CP.private_candidate_unwind_cannot_drop_borrowed_alias_node. Qed.

Theorem terminal_release_preserves_disjoint_sibling_storage : forall
    subject sibling owned_region sibling_region,
  PO.anchor_nodes subject = PO.region_nodes owned_region ->
  PO.anchor_nodes sibling = PO.region_nodes sibling_region ->
  PO.structural_candidate_disjoint owned_region sibling_region ->
  PO.physical_image_intact_after_release subject sibling.
Proof. apply TD.terminal_region_release_preserves_disjoint_sibling. Qed.

Theorem phase_b_starts_only_after_zero_value_validation : forall
    phase_a zero_values,
  TD.run_phase_b phase_a zero_values =
    TD.BeginTopologyPhase ::
      TD.drain_tree (TD.phase_a_value_free_root phase_a) /\
  TD.removed_without_prune (TD.run_phase_b phase_a zero_values) = [] /\
  TD.disposed_values (TD.run_phase_b phase_a zero_values) = [].
Proof. apply TD.phase_b_requires_zero_values_and_only_prunes_topology. Qed.

Theorem barriers_are_installed_only_for_absent_parents_and_retired_once :
  TD.prune_segment false =
    [TD.InstallBarrier; TD.PruneSpan 1; TD.DetachBarrier] /\
  ~ In TD.InstallBarrier (TD.prune_segment true).
Proof.
  split.
  - apply TD.absent_parent_is_guarded_before_and_ungarded_after_prune.
  - apply TD.barrier_never_overwrites_a_real_parent_value.
Qed.

Theorem canonical_empty_outer_or_inner_root_has_work_free_final_drop : forall
    authority,
  TD.release_trace
    (TD.release_final_anchor authority TD.canonical_empty_root) = [] /\
  TD.removed_without_prune
    (TD.release_trace
      (TD.release_final_anchor authority TD.canonical_empty_root)) = [] /\
  TD.disposed_values
    (TD.release_trace
      (TD.release_final_anchor authority TD.canonical_empty_root)) = [].
Proof.
  intro authority.
  pose proof (TD.canonical_empty_final_anchor_is_an_ordinary_work_free_drop
    authority) as [Htrace [Hremoved [Hdisposed _]]].
  repeat split; assumption.
Qed.

Theorem terminal_outer_or_inner_release_never_recursively_drops_nonempty_raw :
  forall authority subject,
  TD.residual_campaign_edges (TD.release_terminal_root authority subject) = 0 /\
  TD.recursively_dropped_unique_edges
    (TD.release_terminal_root authority subject) = 0.
Proof. apply TD.terminal_release_never_uses_recursive_nonempty_pathmap_drop. Qed.

Theorem zipper_navigation_preserves_one_authority_identity : forall state,
  CP.moving_zipper_identity (CP.destination_zipper_step state) =
    CP.moving_zipper_identity state.
Proof. apply CP.destination_zipper_step_preserves_single_identity. Qed.

Theorem zipper_navigation_closes_with_topology_derived_work_not_a_depth_cap :
  forall state,
  let work := S (CP.moving_zipper_ascents state +
                 length (CP.moving_zipper_descents state)) in
  CP.moving_zipper_ascents (CP.drive_destination_zipper work state) = O /\
  CP.moving_zipper_descents (CP.drive_destination_zipper work state) = [].
Proof. apply CP.destination_zipper_closes_without_depth_cap. Qed.

(** In-place outer delta batches.  [RSpaceDataStore::apply_delta] applies a
    batch to the outer channel authority in place.  It no longer clones the
    index, writes the clone and then replaces the live index with it, the
    replacement whose drop drained the whole previous outer map.

    - [in_place_outer_batch world updated replaced] installs the entries,
      image and metadata of [updated] as the live authority.  It keeps the
      external aliases, the staged candidate and the disposed nodes.  The
      disposed values become [replaced] followed by the old ones, so only
      the values of the replaced slots are disposed.
    - A unique live root keeps its identity: the batch writes in place and
      releases no root handle.  A live root that an external alias still
      holds is copied first: the live authority takes a fresh identity, and
      the alias keeps the old version.  The copy releases the live owner's
      handle on the old root, as [Arc::make_mut] does, so the old identity
      is put in front of the disposed root handles, the release that
      [CP.publish_world] records ([outer_batch_released_root_handles]).
    - [apply_outer_batch world None] is a refused batch.  Every error is
      raised before the first write, so the world is unchanged.

    Scope.  There is no abort transition after the first in-place write.  A
    panic after it is fail-stop: it poisons the hot-store write lock, and
    every later access stops the process.  That is [MachineFailStopped] of
    the teardown machine above.  [GuardInvPublishSwapsAndRetiresOnce] and the
    abort invariants still hold; they describe staged publication, not the
    outer delta. *)
Definition root_with_identity (root : CP.AtomicRootAuthority)
    (new_identity : nat) : CP.AtomicRootAuthority :=
  {| CP.atomic_root_identity := new_identity;
     CP.atomic_root_entries := CP.atomic_root_entries root;
     CP.atomic_root_image := CP.atomic_root_image root;
     CP.atomic_root_metadata := CP.atomic_root_metadata root;
     CP.atomic_root_entries_canonical := CP.atomic_root_entries_canonical root;
     CP.atomic_root_image_exact := CP.atomic_root_image_exact root;
     CP.atomic_root_metadata_exact := CP.atomic_root_metadata_exact root |}.

Definition live_root_is_shared (world : CP.TransactionWorldV2) : bool :=
  existsb (Nat.eqb (CP.atomic_root_identity (CP.world_live_authority world)))
    (CP.external_alias_root_identities (CP.world_external_aliases world)).

Definition outer_batch_released_root_handles (world : CP.TransactionWorldV2)
    : list nat :=
  if live_root_is_shared world
  then [CP.atomic_root_identity (CP.world_live_authority world)]
  else [].

Definition in_place_outer_batch (world : CP.TransactionWorldV2)
    (updated : CP.AtomicRootAuthority) (replaced : list nat)
    : CP.TransactionWorldV2 :=
  {| CP.world_live_authority :=
       root_with_identity updated
         (if live_root_is_shared world
          then CP.allocate_fresh_candidate_identity
                 (CP.world_live_authority world)
                 (CP.world_external_aliases world)
          else CP.atomic_root_identity (CP.world_live_authority world));
     CP.world_external_aliases := CP.world_external_aliases world;
     CP.world_staged_candidate := CP.world_staged_candidate world;
     CP.world_disposed_root_handles :=
       outer_batch_released_root_handles world
         ++ CP.world_disposed_root_handles world;
     CP.world_disposed_nodes := CP.world_disposed_nodes world;
     CP.world_disposed_values := replaced ++ CP.world_disposed_values world |}.

Definition apply_outer_batch (world : CP.TransactionWorldV2)
    (outcome : option (CP.AtomicRootAuthority * list nat)%type)
    : CP.TransactionWorldV2 :=
  match outcome with
  | None => world
  | Some (updated, replaced) => in_place_outer_batch world updated replaced
  end.

Theorem refused_outer_batch_preserves_the_world :
  forall world, apply_outer_batch world None = world.
Proof. intro world. reflexivity. Qed.

Theorem unique_outer_batch_keeps_its_identity_and_retires_no_root :
  forall world updated replaced,
    live_root_is_shared world = false ->
    CP.atomic_root_identity
      (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
      CP.atomic_root_identity (CP.world_live_authority world) /\
    CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
      CP.world_disposed_root_handles world.
Proof.
  intros world updated replaced Hunique.
  unfold in_place_outer_batch. rewrite Hunique.
  unfold outer_batch_released_root_handles. rewrite Hunique.
  split; reflexivity.
Qed.

Theorem shared_outer_batch_forks_a_fresh_identity_and_preserves_aliases :
  forall world updated replaced,
    live_root_is_shared world = true ->
    CP.world_external_aliases (in_place_outer_batch world updated replaced) =
      CP.world_external_aliases world /\
    In (CP.atomic_root_identity (CP.world_live_authority world))
      (CP.external_alias_root_identities (CP.world_external_aliases world)) /\
    CP.fresh_candidate_identity (CP.world_live_authority world)
      (CP.world_external_aliases world)
      (CP.atomic_root_identity
        (CP.world_live_authority (in_place_outer_batch world updated replaced))) /\
    CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
      CP.atomic_root_identity (CP.world_live_authority world)
        :: CP.world_disposed_root_handles world.
Proof.
  intros world updated replaced Hshared.
  split; [reflexivity |]. split.
  - unfold live_root_is_shared in Hshared.
    apply existsb_exists in Hshared.
    destruct Hshared as [alias_identity [Hin Hequal]].
    apply Nat.eqb_eq in Hequal. rewrite Hequal. exact Hin.
  - unfold in_place_outer_batch. rewrite Hshared. split.
    + exact (@CP.allocated_candidate_identity_is_genuinely_outside_complete_footprint
        (CP.world_live_authority world) (CP.world_external_aliases world)).
    + unfold outer_batch_released_root_handles. rewrite Hshared. reflexivity.
Qed.

Theorem outer_batch_disposes_only_the_replaced_slot_values :
  forall world updated replaced,
    CP.world_disposed_values (in_place_outer_batch world updated replaced) =
      replaced ++ CP.world_disposed_values world /\
    CP.world_disposed_nodes (in_place_outer_batch world updated replaced) =
      CP.world_disposed_nodes world /\
    CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
      outer_batch_released_root_handles world
        ++ CP.world_disposed_root_handles world.
Proof.
  intros world updated replaced. split.
  - reflexivity.
  - split; reflexivity.
Qed.

Theorem outer_batch_installs_the_published_entries :
  forall world updated replaced,
    CP.atomic_root_entries
      (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
      CP.atomic_root_entries
        (CP.world_live_authority (CP.publish_world world updated)) /\
    CP.atomic_root_image
      (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
      CP.atomic_root_image
        (CP.world_live_authority (CP.publish_world world updated)) /\
    CP.atomic_root_metadata
      (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
      CP.atomic_root_metadata
        (CP.world_live_authority (CP.publish_world world updated)) /\
    CP.world_external_aliases (in_place_outer_batch world updated replaced) =
      CP.world_external_aliases (CP.publish_world world updated).
Proof.
  intros world updated replaced.
  split; [reflexivity |]. split; [reflexivity |]. split; reflexivity.
Qed.

Theorem in_place_outer_batch_is_refusal_atomic_and_publication_equivalent :
  forall world updated replaced,
    apply_outer_batch world None = world /\
    (live_root_is_shared world = false ->
     CP.atomic_root_identity
       (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
       CP.atomic_root_identity (CP.world_live_authority world) /\
     CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
       CP.world_disposed_root_handles world) /\
    (live_root_is_shared world = true ->
     CP.world_external_aliases (in_place_outer_batch world updated replaced) =
       CP.world_external_aliases world /\
     In (CP.atomic_root_identity (CP.world_live_authority world))
       (CP.external_alias_root_identities (CP.world_external_aliases world)) /\
     CP.fresh_candidate_identity (CP.world_live_authority world)
       (CP.world_external_aliases world)
       (CP.atomic_root_identity
         (CP.world_live_authority (in_place_outer_batch world updated replaced))) /\
     CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
       CP.atomic_root_identity (CP.world_live_authority world)
         :: CP.world_disposed_root_handles world) /\
    (CP.world_disposed_values (in_place_outer_batch world updated replaced) =
       replaced ++ CP.world_disposed_values world /\
     CP.world_disposed_nodes (in_place_outer_batch world updated replaced) =
       CP.world_disposed_nodes world /\
     CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
       outer_batch_released_root_handles world
         ++ CP.world_disposed_root_handles world) /\
    (CP.atomic_root_entries
       (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
       CP.atomic_root_entries
         (CP.world_live_authority (CP.publish_world world updated)) /\
     CP.atomic_root_image
       (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
       CP.atomic_root_image
         (CP.world_live_authority (CP.publish_world world updated)) /\
     CP.atomic_root_metadata
       (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
       CP.atomic_root_metadata
         (CP.world_live_authority (CP.publish_world world updated)) /\
     CP.world_external_aliases (in_place_outer_batch world updated replaced) =
       CP.world_external_aliases (CP.publish_world world updated)).
Proof.
  intros world updated replaced.
  split; [exact (@refused_outer_batch_preserves_the_world world) |].
  split;
    [exact (@unique_outer_batch_keeps_its_identity_and_retires_no_root
       world updated replaced) |].
  split;
    [exact (@shared_outer_batch_forks_a_fresh_identity_and_preserves_aliases
       world updated replaced) |].
  split;
    [exact (@outer_batch_disposes_only_the_replaced_slot_values
       world updated replaced) |].
  exact (@outer_batch_installs_the_published_entries world updated replaced).
Qed.

Inductive guarded_ownership_invariant_id : Type :=
| GuardInvOuterCloneOneEvent
| GuardInvOuterCloneNoRawStorage
| GuardInvOuterCloneNoInnerBagClone
| GuardInvRealSlotCloneArcOnly
| GuardInvRealSlotCloneNoRawStorage
| GuardInvOuterNonfinalDropNoRawStorage
| GuardInvInnerNonfinalDropNoRawStorage
| GuardInvMakeMutFreshAnchorRegistered
| GuardInvValueUpdatePreservesAlias
| GuardInvStructuralInsertionPreflight
| GuardInvStructuralRemovalDisjoint
| GuardInvAbortPreservesAuthorityAliases
| GuardInvPublishSwapsAndRetiresOnce
| GuardInvAbortCannotDisposeBorrowed
| GuardInvSiblingReleasePreserved
| GuardInvOuterRealSlotHandoffExact
| GuardInvBarrierNeverRoutedAsRealSlot
| GuardInvFinalInnerAtMostOnce
| GuardInvFinalInnerExactlyOnce
| GuardInvInnerOccurrenceSequenceExact
| GuardInvPhaseBAfterZeroValidation
| GuardInvAbsentParentBarrierPaired
| GuardInvRealParentNeverOverwritten
| GuardInvPublicAuthorityHasNoBarrier
| GuardInvCanonicalEmptyWorkFreeDrop
| GuardInvGeneratedScheduleCompletes
| GuardInvEveryStepDecreasesWork
| GuardInvReachableStackAtMostTwo
| GuardInvInnerCannotNestInner
| GuardInvNoRecursiveRawDrop
| GuardInvArbitraryDepthSameBound
| GuardInvZipperIdentityStable
| GuardInvZipperWorkTopologyDerived
| GuardInvPublicationRequiresNoLease
| GuardInvTerminalClosesControl
| GuardInvStepPreservesObservations
| GuardInvTeardownNeverPublishes
| GuardInvFailureTerminal
| GuardInvFailurePreservesObservations
| GuardInvFailureNoPartialOrRecursiveDrop
| GuardInvOuterBatchRefusalPreservesWorld
| GuardInvOuterBatchUniqueInPlace
| GuardInvOuterBatchSharedForkPreservesAliases
| GuardInvOuterBatchDisposesOnlyReplacedSlots
| GuardInvOuterBatchMatchesPublication.

Definition guarded_ownership_invariant_catalog :
    list guarded_ownership_invariant_id :=
  [ GuardInvOuterCloneOneEvent;
    GuardInvOuterCloneNoRawStorage;
    GuardInvOuterCloneNoInnerBagClone;
    GuardInvRealSlotCloneArcOnly;
    GuardInvRealSlotCloneNoRawStorage;
    GuardInvOuterNonfinalDropNoRawStorage;
    GuardInvInnerNonfinalDropNoRawStorage;
    GuardInvMakeMutFreshAnchorRegistered;
    GuardInvValueUpdatePreservesAlias;
    GuardInvStructuralInsertionPreflight;
    GuardInvStructuralRemovalDisjoint;
    GuardInvAbortPreservesAuthorityAliases;
    GuardInvPublishSwapsAndRetiresOnce;
    GuardInvAbortCannotDisposeBorrowed;
    GuardInvSiblingReleasePreserved;
    GuardInvOuterRealSlotHandoffExact;
    GuardInvBarrierNeverRoutedAsRealSlot;
    GuardInvFinalInnerAtMostOnce;
    GuardInvFinalInnerExactlyOnce;
    GuardInvInnerOccurrenceSequenceExact;
    GuardInvPhaseBAfterZeroValidation;
    GuardInvAbsentParentBarrierPaired;
    GuardInvRealParentNeverOverwritten;
    GuardInvPublicAuthorityHasNoBarrier;
    GuardInvCanonicalEmptyWorkFreeDrop;
    GuardInvGeneratedScheduleCompletes;
    GuardInvEveryStepDecreasesWork;
    GuardInvReachableStackAtMostTwo;
    GuardInvInnerCannotNestInner;
    GuardInvNoRecursiveRawDrop;
    GuardInvArbitraryDepthSameBound;
    GuardInvZipperIdentityStable;
    GuardInvZipperWorkTopologyDerived;
    GuardInvPublicationRequiresNoLease;
    GuardInvTerminalClosesControl;
    GuardInvStepPreservesObservations;
    GuardInvTeardownNeverPublishes;
    GuardInvFailureTerminal;
    GuardInvFailurePreservesObservations;
    GuardInvFailureNoPartialOrRecursiveDrop;
    GuardInvOuterBatchRefusalPreservesWorld;
    GuardInvOuterBatchUniqueInPlace;
    GuardInvOuterBatchSharedForkPreservesAliases;
    GuardInvOuterBatchDisposesOnlyReplacedSlots;
    GuardInvOuterBatchMatchesPublication ].

Definition guarded_ownership_invariant_binding
    (invariant : guarded_ownership_invariant_id) : Prop :=
  match invariant with
  | GuardInvOuterCloneOneEvent =>
      forall others,
        TD.anchor_transition_trace (TD.immutable_wrapper_clone others) =
          [TD.CloneWrapperArc]
  | GuardInvOuterCloneNoRawStorage =>
      forall anchor,
        PO.anchor_raw_headers (shallow_clone_anchor anchor) =
          PO.anchor_raw_headers anchor /\
        PO.anchor_nodes (shallow_clone_anchor anchor) = PO.anchor_nodes anchor /\
        PO.anchor_logical_paths (shallow_clone_anchor anchor) =
          PO.anchor_logical_paths anchor
  | GuardInvOuterCloneNoInnerBagClone =>
      forall heap, outer_clone_bag_heap heap = heap
  | GuardInvRealSlotCloneArcOnly =>
      forall anchor,
        PO.anchor_arc_wrappers (shallow_clone_anchor anchor) =
          S (PO.anchor_arc_wrappers anchor) /\
        PO.anchor_identity (shallow_clone_anchor anchor) =
          PO.anchor_identity anchor
  | GuardInvRealSlotCloneNoRawStorage =>
      forall cell,
        PO.anchor_raw_headers
          (shallow_clone_anchor (bag_physical_anchor cell)) =
            PO.anchor_raw_headers (bag_physical_anchor cell) /\
        PO.anchor_nodes
          (shallow_clone_anchor (bag_physical_anchor cell)) =
            PO.anchor_nodes (bag_physical_anchor cell) /\
        PO.anchor_logical_paths
          (shallow_clone_anchor (bag_physical_anchor cell)) =
            PO.anchor_logical_paths (bag_physical_anchor cell)
  | GuardInvOuterNonfinalDropNoRawStorage =>
      forall others,
        TD.anchor_transition_trace (TD.nonlast_wrapper_drop others) =
          [TD.DropNonlastWrapperArc] /\
        TD.anchor_raw_headers
          (TD.anchor_transition_final (TD.nonlast_wrapper_drop others)) = 1
  | GuardInvInnerNonfinalDropNoRawStorage =>
      forall others,
        TD.anchor_transition_trace (TD.nonlast_wrapper_drop others) =
          [TD.DropNonlastWrapperArc] /\
        TD.anchor_root_payload_copies
          (TD.anchor_transition_final (TD.nonlast_wrapper_drop others)) = 1
  | GuardInvMakeMutFreshAnchorRegistered =>
      forall others,
        TD.anchor_well_formed
          (TD.make_mut_fresh_final (TD.shared_arc_make_mut others)) /\
        TD.anchor_raw_headers
          (TD.make_mut_fresh_final (TD.shared_arc_make_mut others)) = 1
  | GuardInvValueUpdatePreservesAlias =>
      forall write alias_region alias,
        PO.value_update_write_isolated write alias_region ->
        PO.cow_value_update_preserves_alias write alias_region alias alias
  | GuardInvStructuralInsertionPreflight =>
      forall source additions candidate,
        CP.native_insertion_preflight source additions = Some candidate ->
        exists result,
          CP.NativeCowInsertionRefines source additions result /\
          result = candidate
  | GuardInvStructuralRemovalDisjoint =>
      forall (source_anchor candidate_anchor : nat)
        (source_paths candidate_paths : list PO.path)
        (source_region candidate_region : PO.allocation_region),
        PO.allocation_region_well_formed source_region ->
        PO.allocation_region_well_formed candidate_region ->
        PO.region_covers_rebuild_root source_paths source_region ->
        PO.region_covers_rebuild_root candidate_paths candidate_region ->
        PO.region_allocation source_region <>
          PO.region_allocation candidate_region ->
        PO.structural_candidate_disjoint source_region candidate_region
  | GuardInvAbortPreservesAuthorityAliases =>
      forall world,
        CP.world_live_authority (CP.abort_world_before_publication world) =
          CP.world_live_authority world /\
        CP.world_external_aliases (CP.abort_world_before_publication world) =
          CP.world_external_aliases world /\
        CP.world_staged_candidate (CP.abort_world_before_publication world) =
          None
  | GuardInvPublishSwapsAndRetiresOnce =>
      forall world new_root,
        CP.world_live_authority (CP.publish_world world new_root) = new_root /\
        CP.world_disposed_root_handles (CP.publish_world world new_root) =
          CP.atomic_root_identity (CP.world_live_authority world) ::
            CP.world_disposed_root_handles world
  | GuardInvAbortCannotDisposeBorrowed =>
      forall world candidate_identity footprint identity,
        CP.world_staged_candidate world =
          Some (candidate_identity, footprint) ->
        In identity
          (CP.footprint_node_identities
            (CP.candidate_borrowed_footprint footprint)) ->
        ~ In identity
          (CP.footprint_node_identities
            (CP.candidate_private_footprint footprint))
  | GuardInvSiblingReleasePreserved =>
      forall subject sibling owned_region sibling_region,
        PO.anchor_nodes subject = PO.region_nodes owned_region ->
        PO.anchor_nodes sibling = PO.region_nodes sibling_region ->
        PO.structural_candidate_disjoint owned_region sibling_region ->
        PO.physical_image_intact_after_release subject sibling
  | GuardInvOuterRealSlotHandoffExact =>
      forall outer plan,
        released_real_slot_ids (generated_teardown_schedule outer plan) =
        candidate_position_ids (TD.preorder_payloads_tree outer)
  | GuardInvBarrierNeverRoutedAsRealSlot =>
      forall plan,
        released_real_slot_ids
          (expand_outer_event plan TD.InstallBarrier) = []
  | GuardInvFinalInnerAtMostOnce =>
      forall plan trace,
        NoDup (candidate_position_ids (TD.disposed_values trace)) ->
        NoDup (begun_inner_ids (expand_outer_trace plan trace))
  | GuardInvFinalInnerExactlyOnce =>
      forall plan trace,
        every_released_wrapper_is_final plan
          (candidate_position_ids (TD.disposed_values trace)) ->
        begun_inner_ids (expand_outer_trace plan trace) =
          candidate_position_ids (TD.disposed_values trace)
  | GuardInvInnerOccurrenceSequenceExact =>
      forall bag_id tree,
        inner_disposed_payloads bag_id (final_inner_segment bag_id tree) =
          TD.preorder_payloads_tree tree
  | GuardInvPhaseBAfterZeroValidation =>
      forall phase_a zero_values,
        TD.run_phase_b phase_a zero_values =
          TD.BeginTopologyPhase ::
            TD.drain_tree (TD.phase_a_value_free_root phase_a) /\
        TD.removed_without_prune (TD.run_phase_b phase_a zero_values) = [] /\
        TD.disposed_values (TD.run_phase_b phase_a zero_values) = []
  | GuardInvAbsentParentBarrierPaired =>
      TD.prune_segment false =
        [TD.InstallBarrier; TD.PruneSpan 1; TD.DetachBarrier]
  | GuardInvRealParentNeverOverwritten =>
      ~ In TD.InstallBarrier (TD.prune_segment true)
  | GuardInvPublicAuthorityHasNoBarrier =>
      forall authority public,
        RR.project_public_authority authority = Some public ->
        RR.barrier_count authority = 0
  | GuardInvCanonicalEmptyWorkFreeDrop =>
      forall authority,
        TD.release_trace
          (TD.release_final_anchor authority TD.canonical_empty_root) = []
  | GuardInvGeneratedScheduleCompletes =>
      forall outer plan,
        drive_schedule [OuterFrame]
          (generated_teardown_schedule outer plan) = Some TeardownComplete
  | GuardInvEveryStepDecreasesWork =>
      forall before after,
        machine_step before after ->
        length (machine_pending after) < length (machine_pending before)
  | GuardInvReachableStackAtMostTwo =>
      forall outer plan live snapshots state,
        reachable_from (initial_machine outer plan live snapshots) state ->
        length (machine_control state) <= 2
  | GuardInvInnerCannotNestInner =>
      forall active event outcome,
        drive_control [InnerFrame active; OuterFrame] event = Some outcome ->
        match outcome with
        | ContinueWith next =>
            next = [InnerFrame active; OuterFrame] \/ next = [OuterFrame]
        | TeardownComplete => False
        end
  | GuardInvNoRecursiveRawDrop =>
      forall authority subject,
        TD.recursively_dropped_unique_edges
          (TD.release_terminal_root authority subject) = 0
  | GuardInvArbitraryDepthSameBound =>
      forall outer plan live snapshots state,
        reachable_from (initial_machine outer plan live snapshots) state ->
        length (machine_control state) <= 2
  | GuardInvZipperIdentityStable =>
      forall state,
        CP.moving_zipper_identity (CP.destination_zipper_step state) =
          CP.moving_zipper_identity state
  | GuardInvZipperWorkTopologyDerived =>
      forall state,
        let work := S (CP.moving_zipper_ascents state +
                       length (CP.moving_zipper_descents state)) in
        CP.moving_zipper_ascents
          (CP.drive_destination_zipper work state) = O /\
        CP.moving_zipper_descents
          (CP.drive_destination_zipper work state) = []
  | GuardInvPublicationRequiresNoLease =>
      forall control, gated_publication control -> control = []
  | GuardInvTerminalClosesControl =>
      forall outer plan live snapshots state,
        reachable_from (initial_machine outer plan live snapshots) state ->
        machine_status_value state <> MachineRunning ->
        machine_pending state = [] /\ machine_control state = []
  | GuardInvStepPreservesObservations =>
      forall before after,
        machine_step before after ->
        machine_live_observation after = machine_live_observation before /\
        machine_snapshot_observations after =
          machine_snapshot_observations before
  | GuardInvTeardownNeverPublishes =>
      forall before after,
        machine_step before after ->
        machine_publication_count after = machine_publication_count before
  | GuardInvFailureTerminal =>
      forall live snapshots pubs raw after,
        ~ machine_step
          (MachineState [] [] MachineFailStopped live snapshots pubs raw)
          after
  | GuardInvFailurePreservesObservations =>
      forall live snapshots pubs raw,
        machine_live_observation
          (MachineState [] [] MachineFailStopped live snapshots pubs raw) =
            live /\
        machine_snapshot_observations
          (MachineState [] [] MachineFailStopped live snapshots pubs raw) =
            snapshots
  | GuardInvFailureNoPartialOrRecursiveDrop =>
      forall live snapshots pubs raw,
        machine_publication_count
          (MachineState [] [] MachineFailStopped live snapshots pubs raw) =
            pubs /\
        machine_recursive_raw_drop_count
          (MachineState [] [] MachineFailStopped live snapshots pubs raw) = raw
  | GuardInvOuterBatchRefusalPreservesWorld =>
      forall world, apply_outer_batch world None = world
  | GuardInvOuterBatchUniqueInPlace =>
      forall world updated replaced,
        live_root_is_shared world = false ->
        CP.atomic_root_identity
          (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
          CP.atomic_root_identity (CP.world_live_authority world) /\
        CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
          CP.world_disposed_root_handles world
  | GuardInvOuterBatchSharedForkPreservesAliases =>
      forall world updated replaced,
        live_root_is_shared world = true ->
        CP.world_external_aliases (in_place_outer_batch world updated replaced) =
          CP.world_external_aliases world /\
        In (CP.atomic_root_identity (CP.world_live_authority world))
          (CP.external_alias_root_identities (CP.world_external_aliases world)) /\
        CP.fresh_candidate_identity (CP.world_live_authority world)
          (CP.world_external_aliases world)
          (CP.atomic_root_identity
            (CP.world_live_authority (in_place_outer_batch world updated replaced))) /\
        CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
          CP.atomic_root_identity (CP.world_live_authority world)
            :: CP.world_disposed_root_handles world
  | GuardInvOuterBatchDisposesOnlyReplacedSlots =>
      forall world updated replaced,
        CP.world_disposed_values (in_place_outer_batch world updated replaced) =
          replaced ++ CP.world_disposed_values world /\
        CP.world_disposed_nodes (in_place_outer_batch world updated replaced) =
          CP.world_disposed_nodes world /\
        CP.world_disposed_root_handles (in_place_outer_batch world updated replaced) =
          outer_batch_released_root_handles world
            ++ CP.world_disposed_root_handles world
  | GuardInvOuterBatchMatchesPublication =>
      forall world updated replaced,
        CP.atomic_root_entries
          (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
          CP.atomic_root_entries
            (CP.world_live_authority (CP.publish_world world updated)) /\
        CP.atomic_root_image
          (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
          CP.atomic_root_image
            (CP.world_live_authority (CP.publish_world world updated)) /\
        CP.atomic_root_metadata
          (CP.world_live_authority (in_place_outer_batch world updated replaced)) =
          CP.atomic_root_metadata
            (CP.world_live_authority (CP.publish_world world updated)) /\
        CP.world_external_aliases (in_place_outer_batch world updated replaced) =
          CP.world_external_aliases (CP.publish_world world updated)
  end.

Theorem every_guarded_ownership_invariant_is_bound : forall invariant,
  guarded_ownership_invariant_binding invariant.
Proof.
  intro invariant. destruct invariant; cbn.
  - apply outer_clone_trace_is_one_constant_time_wrapper_event.
  - intro anchor. apply PO.shared_arc_clone_creates_no_raw_owner.
  - apply outer_clone_does_not_clone_any_inner_bag.
  - intro anchor.
    destruct (shallow_clone_changes_only_the_arc_wrapper_count anchor)
      as [_ [_ [_ [_ [_ Harcs]]]]]. split; [exact Harcs | reflexivity].
  - apply shallow_real_slot_clone_creates_no_inner_raw_storage.
  - intro others.
    destruct (nonfinal_wrapper_release_cannot_touch_raw_storage others)
      as [Htrace [Hraw _]]. split; assumption.
  - intro others.
    destruct (nonfinal_wrapper_release_cannot_touch_raw_storage others)
      as [Htrace [_ Hpayload]]. split; assumption.
  - intro others.
    destruct (make_mut_registers_exactly_one_well_formed_fresh_anchor others)
      as [_ [Hwell [Hraw _]]]. split; assumption.
  - apply exact_value_update_preserves_external_alias.
  - apply structural_insertion_is_fully_preflighted.
  - apply structural_removal_uses_an_ownership_disjoint_rebuild.
  - apply abort_preserves_live_authority_snapshots_and_aliases.
  - apply publication_swaps_once_and_retires_the_old_handle_once.
  - apply abort_cannot_dispose_borrowed_candidate_storage.
  - apply terminal_release_preserves_disjoint_sibling_storage.
  - apply terminal_outer_teardown_hands_off_every_real_slot_occurrence_once.
  - intro plan. reflexivity.
  - apply final_inner_anchor_is_elected_at_most_once.
  - apply every_final_wrapper_elects_exactly_one_inner_teardown.
  - apply final_inner_segment_preserves_complete_occurrence_sequence.
  - apply phase_b_starts_only_after_zero_value_validation.
  - apply TD.absent_parent_is_guarded_before_and_ungarded_after_prune.
  - apply TD.barrier_never_overwrites_a_real_parent_value.
  - apply RR.successful_public_projection_contains_no_barrier.
  - intro authority.
    exact (proj1
      (TD.canonical_empty_final_anchor_is_an_ordinary_work_free_drop
        authority)).
  - apply every_generated_teardown_schedule_completes.
  - apply every_live_machine_step_strictly_decreases_ghost_work.
  - apply every_reachable_machine_has_at_most_two_control_frames.
  - apply inner_control_can_never_enter_another_inner_frame.
  - intros authority subject.
    exact (proj2
      (terminal_outer_or_inner_release_never_recursively_drops_nonempty_raw
        authority subject)).
  - apply arbitrary_finite_outer_and_inner_depth_has_the_same_native_bound.
  - apply zipper_navigation_preserves_one_authority_identity.
  - apply zipper_navigation_closes_with_topology_derived_work_not_a_depth_cap.
  - apply publication_is_unrepresentable_with_a_live_mutation_lease.
  - apply reachable_terminal_state_closes_every_cursor_and_barrier.
  - apply every_machine_step_preserves_live_and_snapshot_observations.
  - intros before after Hstep.
    exact (proj1
      (teardown_never_publishes_or_recursively_drops_raw_pathmaps Hstep)).
  - intros live snapshots pubs raw after Hstep. inversion Hstep.
  - intros. split; reflexivity.
  - intros. split; reflexivity.
  - exact @refused_outer_batch_preserves_the_world.
  - exact @unique_outer_batch_keeps_its_identity_and_retires_no_root.
  - exact @shared_outer_batch_forks_a_fresh_identity_and_preserves_aliases.
  - exact @outer_batch_disposes_only_the_replaced_slot_values.
  - exact @outer_batch_installs_the_published_entries.
Qed.

Theorem guarded_ownership_invariant_catalog_is_unique_complete_and_exact :
  NoDup guarded_ownership_invariant_catalog /\
  Forall guarded_ownership_invariant_binding
    guarded_ownership_invariant_catalog /\
  length guarded_ownership_invariant_catalog = 45.
Proof.
  split.
  - unfold guarded_ownership_invariant_catalog.
    repeat match goal with
    | |- NoDup (_ :: _) => constructor; [simpl; intuition discriminate |]
    | |- NoDup [] => constructor
    end.
  - split.
    + apply Forall_forall. intros invariant _.
      apply every_guarded_ownership_invariant_is_bound.
    + reflexivity.
Qed.

Print Assumptions terminal_outer_teardown_hands_off_every_real_slot_occurrence_once.
Print Assumptions final_inner_segment_preserves_complete_occurrence_sequence.
Print Assumptions every_generated_teardown_schedule_completes.
Print Assumptions every_reachable_machine_has_at_most_two_control_frames.
Print Assumptions every_live_machine_step_strictly_decreases_ghost_work.
Print Assumptions fail_stop_is_terminal_without_partial_publication_or_recursive_drop.
Print Assumptions in_place_outer_batch_is_refusal_atomic_and_publication_equivalent.

End TwoLevelRSpaceGuardedOwnership.
