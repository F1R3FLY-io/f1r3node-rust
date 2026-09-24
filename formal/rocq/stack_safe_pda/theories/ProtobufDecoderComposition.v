From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import ProtobufOwnedSlots.
Import ListNotations.
Set Implicit Arguments.

Module ProtobufDecoderComposition.

(* This is a payload-parametric control model, not a protobuf parser. Input
   identities, byte counts, locations and errors are supplied observations.
   Concrete parsing and constructor correspondence remain native obligations. *)
Inductive frame_kind := NodeFrame | AttachFrame | EpmFrame.
Record frame (V : Type) := Frame {
  kind : frame_kind;
  payload : V;
  limit : nat;
  input_identity : nat;
  location : option (nat * nat)
}.
Arguments Frame {V}.
Arguments kind {V}.
Arguments payload {V}.
Arguments limit {V}.
Arguments input_identity {V}.
Arguments location {V}.

Definition map_frame {V W} (f : V -> W) (k : frame V) :=
  Frame (kind k) (f (payload k)) (limit k) (input_identity k) (location k).

(* Frames are represented nearest-continuation first. The Rust Vec is read in
   reverse; establishing that correspondence is a generated-routing premise. *)
Fixpoint contexts {V} (frames : list (frame V)) : list (nat * nat) :=
  match frames with
  | [] => []
  | k :: tail => match kind k, location k with
    | AttachFrame, Some loc => loc :: contexts tail
    | _, _ => contexts tail
    end
  end.

Theorem mapped_frame_preserves_control {V W} (f : V -> W) k :
  kind (map_frame f k) = kind k /\ limit (map_frame f k) = limit k /\
  input_identity (map_frame f k) = input_identity k /\
  location (map_frame f k) = location k.
Proof. repeat split; reflexivity. Qed.

Theorem mapped_frames_preserve_context_order {V W} (f : V -> W) ks :
  contexts (map (map_frame f) ks) = contexts ks.
Proof.
  induction ks as [|[k v l i loc] tail IH]; cbn; [reflexivity|].
  destruct k, loc; cbn; now rewrite IH.
Qed.

Definition decorate {V} (error : nat) (frames : list (frame V)) :=
  (error, contexts frames).

Theorem selected_error_and_contexts_are_preserved {V W} (f : V -> W) e ks :
  decorate e (map (map_frame f) ks) = decorate e ks.
Proof. unfold decorate. now rewrite mapped_frames_preserve_context_order. Qed.

Record input := Input { identity : nat; remaining : nat }.
Record control := Control {
  current : input;
  saved : list input;
  epm_depth : nat
}.
Definition enter (body : input) (s : control) :=
  Control body (current s :: saved s) (epm_depth s).
Definition consume_body (s : control) :=
  Control (Input (identity (current s)) 0) (saved s) (epm_depth s).
Definition leave (s : control) : option control :=
  match remaining (current s), saved s with
  | 0, parent :: tail => Some (Control parent tail (epm_depth s))
  | _, _ => None
  end.
Definition begin_snapshot (s : control) : bool * control :=
  (Nat.eqb (epm_depth s) 0,
   Control (current s) (saved s) (S (epm_depth s))).
Definition admit_snapshot (accepted : bool) (s : control) : option control :=
  if accepted then
    match epm_depth s with
    | 0 => None
    | S depth => Some (Control (current s) (saved s) depth)
    end
  else None.

Theorem completed_body_restores_exact_parent body s :
  leave (consume_body (enter body s)) = Some s.
Proof. destruct s; reflexivity. Qed.
Theorem unfinished_body_cannot_restore id n parent tail depth :
  leave (Control (Input id (S n)) (parent :: tail) depth) = None.
Proof. reflexivity. Qed.
Theorem absent_parent_cannot_restore body depth :
  leave (Control body [] depth) = None.
Proof. destruct body as [id [|n]]; reflexivity. Qed.
Theorem enter_preserves_admission_depth body s :
  epm_depth (enter body s) = epm_depth s.
Proof. reflexivity. Qed.
Theorem nested_inputs_restore_lifo first second s :
  leave (consume_body (enter second (enter first s))) = Some (enter first s).
Proof. apply completed_body_restores_exact_parent. Qed.
Theorem snapshot_policy_uses_preincrement_depth s :
  fst (begin_snapshot s) = true <-> epm_depth s = 0.
Proof. unfold begin_snapshot; cbn. apply Nat.eqb_eq. Qed.
Theorem successful_admission_balances_depth s :
  admit_snapshot true (snd (begin_snapshot s)) = Some s.
Proof. destruct s; reflexivity. Qed.
Theorem failed_admission_has_no_continuation s :
  admit_snapshot false s = None.
Proof. reflexivity. Qed.
Theorem admission_without_snapshot_is_rejected body parents :
  admit_snapshot true (Control body parents 0) = None.
Proof. reflexivity. Qed.

(* The representation relation is unchanged by routing control. Its one-step
   premise must be discharged by POS/PPA/DCC/EPR plus the concrete adapters. *)
Section Routing.
Context {P R Event Error : Type}.
Variable observe : P -> R.
Variable planned_step : P -> Event -> P * option Error.
Variable reference_step : R -> Event -> R * option Error.
Hypothesis step_corresponds : forall p e,
  let '(next, failure) := planned_step p e in
  (observe next, failure) = reference_step (observe p) e.

Fixpoint drive {S} (step : S -> Event -> S * option Error)
    (s : S) (events : list Event) : S * option Error * list Event :=
  match events with
  | [] => (s, None, [])
  | event :: tail =>
      let '(next, failure) := step s event in
      match failure with
      | Some error => (next, Some error, tail)
      | None => drive step next tail
      end
  end.
Definition observation (result : P * option Error * list Event) :=
  let '(s, failure, suffix) := result in (observe s, failure, suffix).

Theorem routing_composes_payload_refinement events p :
  observation (drive planned_step p events) =
  drive reference_step (observe p) events.
Proof.
  revert p. induction events as [|event tail IH]; intros p; cbn; [reflexivity|].
  pose proof (step_corresponds p event) as H.
  destruct (planned_step p event) as [next failure] eqn:step.
  rewrite <- H. destruct failure; cbn; [reflexivity|apply IH].
Qed.
Theorem first_error_leaves_suffix_unvisited p e next error tail :
  planned_step p e = (next, Some error) ->
  drive planned_step p (e :: tail) = (next, Some error, tail).
Proof. intros H. cbn. now rewrite H. Qed.
End Routing.

Inductive boundary := Reopenable | ComponentConsumed | EpmValueConsumed | PublicRoot.
Inductive held (V H : Type) := Live (value : V) | Frozen (handle : H).
Arguments Live {V H}.
Arguments Frozen {V H}.
Definition finish_boundary {V H} (freeze : V -> H) b (v : V) : held V H :=
  match b with Reopenable => Live v | _ => Frozen (freeze v) end.
Definition held_denotation {V H R} (live : V -> R) (frozen : H -> R) x :=
  match x with Live v => live v | Frozen h => frozen h end.

Theorem reopenable_child_is_not_frozen {V H} (freeze : V -> H) v :
  finish_boundary freeze Reopenable v = Live v.
Proof. reflexivity. Qed.
Theorem freeze_only_at_consuming_boundary {V H} (freeze : V -> H) b v h :
  finish_boundary freeze b v = Frozen h -> b <> Reopenable.
Proof. destruct b; cbn; intros; congruence. Qed.
Theorem freezing_preserves_denotation {V H R} (freeze : V -> H)
    (live : V -> R) (frozen : H -> R) :
  (forall v, frozen (freeze v) = live v) -> forall b v,
  held_denotation live frozen (finish_boundary freeze b v) = live v.
Proof. intros correct b v. destruct b; cbn; auto. Qed.
Theorem reopen_preserves_existing_builder {V} (default child : V) :
  ProtobufOwnedSlots.ProtobufOwnedSlots.take_reopen default (Some child) = (child, None).
Proof. apply ProtobufOwnedSlots.ProtobufOwnedSlots.reopen_present_preserves_child. Qed.

(* Final projection is fallible. The abstract protocol keeps the arena alive
   during projection and guarded retirement, then releases it before exposing
   a public result. Rust lifetime/destructor correspondence remains separate. *)
Inductive terminal_event := AttemptProjection | RetireGuard (id : nat)
  | RetireArena | PublishRoot.
Inductive projection (V E : Type) := Projected (value : V) | ProjectionFailed (error : E).
Arguments Projected {V E}.
Arguments ProjectionFailed {V E}.
Definition terminal_trace {V E} (result : projection V E) (guards : list nat) :=
  [AttemptProjection] ++ map RetireGuard guards ++ [RetireArena] ++
  match result with Projected _ => [PublishRoot] | ProjectionFailed _ => [] end.
Definition published {V E} (result : projection V E) : option V :=
  match result with Projected v => Some v | ProjectionFailed _ => None end.
Definition selected_failure {V E} (result : projection V E) : option E :=
  match result with Projected _ => None | ProjectionFailed e => Some e end.
Definition alive_after (alive : bool) (event : terminal_event) :=
  match event with RetireArena => false | _ => alive end.

Theorem guards_preserve_live_arena guards :
  fold_left alive_after (map RetireGuard guards) true = true.
Proof. induction guards; cbn; auto. Qed.
Theorem projection_and_retirement_use_live_arena guards :
  fold_left alive_after ([AttemptProjection] ++ map RetireGuard guards) true = true.
Proof. cbn. apply guards_preserve_live_arena. Qed.
Theorem final_projection_failure_is_not_published {V E} (e : E) :
  published (@ProjectionFailed V E e) = None /\
  selected_failure (@ProjectionFailed V E e) = Some e.
Proof. split; reflexivity. Qed.
Theorem projection_failure_trace_has_no_publication {V E} (e : E) guards :
  ~ In PublishRoot (terminal_trace (@ProjectionFailed V E e) guards).
Proof.
  unfold terminal_trace. cbn. intros [bad|rest]; [discriminate|].
  apply in_app_or in rest. destruct rest as [member|[bad|empty]].
  - apply in_map_iff in member. destruct member as [id [bad _]]. discriminate.
  - discriminate.
  - contradiction.
Qed.
Theorem success_retires_guards_and_arena_before_publication {V E} (v : V) guards :
  terminal_trace (@Projected V E v) guards =
  AttemptProjection :: (map RetireGuard guards ++ [RetireArena; PublishRoot]).
Proof. reflexivity. Qed.
Theorem every_guard_has_one_retirement_per_owned_slot {V E}
    (result : projection V E) guards :
  length (filter (fun e => match e with RetireGuard _ => true | _ => false end)
    (terminal_trace result guards)) = length guards.
Proof.
  unfold terminal_trace. cbn. rewrite !filter_app. cbn.
  assert (length (filter (fun e => match e with RetireGuard _ => true | _ => false end)
    (map RetireGuard guards)) = length guards) as H.
  { induction guards; cbn; auto. }
  destruct result; cbn; rewrite app_nil_r; exact H.
Qed.

Theorem reversed_context_mutant_is_observable :
  contexts [Frame AttachFrame tt 0 0 (Some (1, 2));
            Frame AttachFrame tt 0 0 (Some (3, 4))] <>
  [(3, 4); (1, 2)].
Proof. discriminate. Qed.
Theorem premature_freeze_mutant_loses_reopenable_form :
  finish_boundary (fun n : nat => n) ComponentConsumed 7 <>
  finish_boundary (fun n : nat => n) Reopenable 7.
Proof. discriminate. Qed.

Print Assumptions mapped_frame_preserves_control.
Print Assumptions mapped_frames_preserve_context_order.
Print Assumptions selected_error_and_contexts_are_preserved.
Print Assumptions completed_body_restores_exact_parent.
Print Assumptions unfinished_body_cannot_restore.
Print Assumptions absent_parent_cannot_restore.
Print Assumptions enter_preserves_admission_depth.
Print Assumptions nested_inputs_restore_lifo.
Print Assumptions snapshot_policy_uses_preincrement_depth.
Print Assumptions successful_admission_balances_depth.
Print Assumptions failed_admission_has_no_continuation.
Print Assumptions admission_without_snapshot_is_rejected.
Print Assumptions routing_composes_payload_refinement.
Print Assumptions first_error_leaves_suffix_unvisited.
Print Assumptions reopenable_child_is_not_frozen.
Print Assumptions freeze_only_at_consuming_boundary.
Print Assumptions freezing_preserves_denotation.
Print Assumptions reopen_preserves_existing_builder.
Print Assumptions guards_preserve_live_arena.
Print Assumptions projection_and_retirement_use_live_arena.
Print Assumptions final_projection_failure_is_not_published.
Print Assumptions projection_failure_trace_has_no_publication.
Print Assumptions success_retires_guards_and_arena_before_publication.
Print Assumptions every_guard_has_one_retirement_per_owned_slot.
Print Assumptions reversed_context_mutant_is_observable.
Print Assumptions premature_freeze_mutant_loses_reopenable_form.

End ProtobufDecoderComposition.
