From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA.

Import ListNotations.
Set Implicit Arguments.

(**
  Proof-first RPB3 admission kernel.

  The generated Rust schema table is represented by [local_valid], a
  parameter supplied by the executable generated-table differential.  This
  theory proves the traversal/refinement obligation independently: a typed
  post-order reference and an explicit instruction worklist return the same
  admission bit for every finite schema tree.  Wire parsing, score algebra,
  and the concrete table binding remain separate boundaries and are therefore
  not hidden behind an axiom here.
*)

Section RPB3.

Variable local_valid : nat -> bool.

Definition rpb3_algebra (label : nat) (children : list bool) : bool :=
  local_valid label && forallb (fun value => value) children.

Definition rpb3_recursive_validate (subject : @tree nat) : bool :=
  @fold_tree nat bool rpb3_algebra subject.

Definition rpb3_pda_result (subject : @tree nat) : option (list bool) :=
  @run nat bool rpb3_algebra (compile_tree subject) [].

Definition rpb3_pda_validate (subject : @tree nat) : bool :=
  match rpb3_pda_result subject with
  | Some [value] => value
  | _ => false
  end.

Lemma rpb3_pda_result_exact :
  forall subject,
    rpb3_pda_result subject = Some [rpb3_recursive_validate subject].
Proof.
  intro subject.
  unfold rpb3_pda_result, rpb3_recursive_validate.
  apply pda_fold_equivalent_to_recursive_fold.
Qed.

Theorem rpb3_pda_equivalent_to_recursive_validation :
  forall subject,
    rpb3_pda_validate subject = rpb3_recursive_validate subject.
Proof.
  intro subject.
  unfold rpb3_pda_validate.
  rewrite rpb3_pda_result_exact.
  reflexivity.
Qed.

Theorem rpb3_pda_terminates_without_call_stack_recursion :
  forall subject,
    exists result, rpb3_pda_result subject = Some result.
Proof.
  intro subject.
  exists [rpb3_recursive_validate subject].
  apply rpb3_pda_result_exact.
Qed.

Theorem rpb3_compiled_instruction_count_is_finite :
  forall subject,
    length (compile_tree subject) = @tree_nodes nat subject.
Proof.
  intro subject.
  exact (proj1 compiled_program_has_one_instruction_per_node subject).
Qed.

Theorem rpb3_admission_is_deterministic :
  forall subject left right,
    rpb3_pda_result subject = Some left ->
    rpb3_pda_result subject = Some right ->
    left = right.
Proof.
  intros subject left right left_result right_result.
  rewrite rpb3_pda_result_exact in left_result.
  rewrite rpb3_pda_result_exact in right_result.
  now inversion left_result; inversion right_result.
Qed.

End RPB3.

Print Assumptions rpb3_pda_equivalent_to_recursive_validation.

Module CanonicalKeyProtocol.

Record located_byte := LocatedByte {
  byte_value : nat;
  byte_source : nat;
  byte_offset : nat
}.

Record region := Region {
  region_owner : nat;
  region_bytes : list located_byte;
  region_eof_source : nat;
  region_eof_offset : nat
}.

Definition source_refines (sources : nat -> list nat) (input : region) : Prop :=
  Forall (fun b => byte_value b < 256 /\
    nth_error (sources (byte_source b)) (byte_offset b) = Some (byte_value b))
    (region_bytes input).

Inductive frame_kind := KeyFrame | MessageFrame | EpmFrame.
Inductive request_role := RawParEscape | NestedMap | SchemaMessage | SchemaEpm.
Inductive map_mode := EmptyMode | SetMode | MapMode.
Inductive summary :=
| KeySummary (stable : bool)
| MessageSummary (stable : bool)
| EpmSummary (mode : map_mode) (stable : bool).

Definition role_kind (role : request_role) : frame_kind :=
  match role with
  | RawParEscape | SchemaMessage => MessageFrame
  | NestedMap | SchemaEpm => EpmFrame
  end.

Definition summary_kind (value : summary) : frame_kind :=
  match value with
  | KeySummary _ => KeyFrame
  | MessageSummary _ => MessageFrame
  | EpmSummary _ _ => EpmFrame
  end.

Definition kind_eqb (left right : frame_kind) : bool :=
  match left, right with
  | KeyFrame, KeyFrame | MessageFrame, MessageFrame | EpmFrame, EpmFrame => true
  | _, _ => false
  end.

Definition role_eqb (left right : request_role) : bool :=
  match left, right with
  | RawParEscape, RawParEscape | NestedMap, NestedMap
  | SchemaMessage, SchemaMessage | SchemaEpm, SchemaEpm => true
  | _, _ => false
  end.

Lemma kind_eqb_exact : forall left right,
  kind_eqb left right = true <-> left = right.
Proof. destruct left, right; simpl; intuition discriminate. Qed.

Lemma role_eqb_exact : forall left right,
  role_eqb left right = true <-> left = right.
Proof. destruct left, right; simpl; intuition discriminate. Qed.

Record reducer_state := ReducerState {
  reducer_token : nat;
  reducer_stable : bool;
  reducer_wait : option (nat * request_role)
}.

Record frame := Frame {
  frame_kind_of : frame_kind;
  frame_input : region;
  frame_cursor : nat;
  frame_reducer : reducer_state
}.

Record request := Request {
  request_id : nat;
  request_role_of : request_role;
  request_payload : region;
  request_resume_token : nat
}.

Record suspended := Suspended {
  saved_parent : frame;
  saved_request : request
}.

Definition fresh_frame (kind : frame_kind) (input : region) : frame :=
  Frame kind input 0 (ReducerState 0 true None).

Definition awaiting (parent : frame) (child : request) : bool :=
  match reducer_wait (frame_reducer parent) with
  | Some (id, role) =>
      (id =? request_id child) && role_eqb role (request_role_of child)
  | None => false
  end.

Definition resolved_stability (role : request_role) (value : summary) : option bool :=
  match role, value with
  | RawParEscape, MessageSummary false => Some false
  | NestedMap, EpmSummary MapMode true => Some true
  | SchemaMessage, MessageSummary stable => Some stable
  | SchemaEpm, EpmSummary _ stable => Some stable
  | _, _ => None
  end.

Definition advance (current : frame) (count : nat) (next : reducer_state)
    : option frame :=
  match reducer_wait (frame_reducer current) with
  | Some _ => None
  | None =>
      if (0 <? count) &&
         (frame_cursor current + count <=? length (region_bytes (frame_input current)))
      then Some (Frame (frame_kind_of current) (frame_input current)
        (frame_cursor current + count) next)
      else None
  end.

Definition dispatch (current : frame) (child : request)
    : option (frame * suspended) :=
  if awaiting current child then
    Some (fresh_frame (role_kind (request_role_of child)) (request_payload child),
      Suspended current child)
  else None.

Definition resume (waiting : suspended) (value : summary) : option frame :=
  let parent := saved_parent waiting in
  let child := saved_request waiting in
  if awaiting parent child then
    match resolved_stability (request_role_of child) value with
    | Some stable => Some (Frame (frame_kind_of parent) (frame_input parent)
        (frame_cursor parent)
        (ReducerState (request_resume_token child)
          (reducer_stable (frame_reducer parent) && stable) None))
    | None => None
    end
  else None.

Definition finished (current : frame) (value : summary) : bool :=
  (frame_cursor current =? length (region_bytes (frame_input current))) &&
  match reducer_wait (frame_reducer current) with
  | Some _ => false
  | None => kind_eqb (frame_kind_of current) (summary_kind value) &&
      match value with
      | KeySummary stable => Bool.eqb stable (reducer_stable (frame_reducer current))
      | _ => true
      end
  end.

Inductive refusal := ParserRefusal | CursorRefusal | DispatchRefusal
  | CompletionRefusal | ResolutionRefusal | ChildRefusal (code : nat)
  | ForbiddenEffectRefusal.

Inductive outcome := Rejected (reason : refusal)
  | Accepted (input : region) (value : summary).

Inductive machine := Running (current : frame) (parents : list suspended)
  | Halted (result : outcome).

Inductive effect := ReadBytes (input : region) (start count : nat)
  | PushChild (child : request) | PopChild (child : request)
  | AcceptRoot | Refuse (reason : refusal)
  | TypedDecode | ReplayEndpoint | ReconstructKey.

Inductive command := Consume (count : nat) (next : reducer_state)
  | Dispatch (child : request) | Complete (value : summary)
  | Fail (code : nat) | Forbidden (attempt : effect).

Definition reject (reason : refusal) : machine * list effect :=
  (Halted (Rejected reason), [Refuse reason]).

Definition transport (current : frame) (parents : list suspended) (action : command)
    : machine * list effect :=
  match action with
  | Consume count next =>
      match advance current count next with
      | Some updated => (Running updated parents,
          [ReadBytes (frame_input current) (frame_cursor current) count])
      | None => reject CursorRefusal
      end
  | Dispatch child =>
      match dispatch current child with
      | Some (active, waiting) => (Running active (waiting :: parents), [PushChild child])
      | None => reject DispatchRefusal
      end
  | Complete value =>
      if finished current value then
        match parents with
        | [] => (Halted (Accepted (frame_input current) value), [AcceptRoot])
        | waiting :: rest =>
            match resume waiting value with
            | Some parent => (Running parent rest, [PopChild (saved_request waiting)])
            | None => reject ResolutionRefusal
            end
        end
      else reject CompletionRefusal
  | Fail code => reject (ChildRefusal code)
  | Forbidden _ => reject ForbiddenEffectRefusal
  end.

Definition step (authorize : frame -> command -> bool) (state : machine)
    (action : command) : machine * list effect :=
  match state with
  | Halted result => (Halted result, [])
  | Running current parents =>
      if authorize current action then transport current parents action
      else reject ParserRefusal
  end.

Fixpoint run (authorize : frame -> command -> bool) (actions : list command)
    (state : machine) : machine * list effect :=
  match actions with
  | [] => (state, [])
  | action :: rest =>
      let '(next, first) := step authorize state action in
      let '(last, later) := run authorize rest next in
      (last, first ++ later)
  end.

Definition stable_only (result : outcome) : bool :=
  match result with Accepted _ (KeySummary true) => true | _ => false end.

Definition validity_stability (result : outcome) : option bool :=
  match result with Accepted _ (KeySummary stable) => Some stable | _ => None end.

Fixpoint linked (current : frame) (parents : list suspended) : Prop :=
  match parents with
  | [] => frame_kind_of current = KeyFrame
  | waiting :: rest =>
      frame_input current = request_payload (saved_request waiting) /\
      frame_kind_of current = role_kind (request_role_of (saved_request waiting)) /\
      awaiting (saved_parent waiting) (saved_request waiting) = true /\
      linked (saved_parent waiting) rest
  end.

Fixpoint root_input (current : frame) (parents : list suspended) : region :=
  match parents with
  | [] => frame_input current
  | waiting :: rest => root_input (saved_parent waiting) rest
  end.

Definition retains_root (input : region) (state : machine) : Prop :=
  match state with
  | Running current parents => linked current parents /\ root_input current parents = input
  | Halted (Accepted output value) => output = input /\ summary_kind value = KeyFrame
  | Halted (Rejected _) => True
  end.

Lemma awaiting_exact : forall parent child,
  awaiting parent child = true <->
  reducer_wait (frame_reducer parent) = Some (request_id child, request_role_of child).
Proof.
  intros parent child. unfold awaiting.
  destruct (reducer_wait (frame_reducer parent)) as [[id role]|] eqn:H; simpl.
  - rewrite Bool.andb_true_iff, Nat.eqb_eq, role_eqb_exact.
    split; intros E; [destruct E; subst; reflexivity|inversion E; auto].
  - split; discriminate.
Qed.

Lemma advance_exact : forall current count next updated,
  advance current count next = Some updated ->
  reducer_wait (frame_reducer current) = None /\ 0 < count /\
  frame_cursor current + count <= length (region_bytes (frame_input current)) /\
  updated = Frame (frame_kind_of current) (frame_input current)
    (frame_cursor current + count) next.
Proof.
  intros current count next updated. unfold advance.
  destruct (reducer_wait (frame_reducer current)) as [waiting|] eqn:W; try discriminate.
  destruct ((0 <? count) &&
    (frame_cursor current + count <=? length (region_bytes (frame_input current)))) eqn:B;
    try discriminate.
  intro E. inversion E; subst. apply Bool.andb_true_iff in B.
  destruct B as [P B]. apply Nat.ltb_lt in P. apply Nat.leb_le in B. auto.
Qed.

Lemma dispatch_exact : forall current child active waiting,
  dispatch current child = Some (active, waiting) ->
  awaiting current child = true /\
  active = fresh_frame (role_kind (request_role_of child)) (request_payload child) /\
  waiting = Suspended current child.
Proof.
  intros current child active waiting. unfold dispatch.
  destruct (awaiting current child) eqn:E; try discriminate.
  intro H. inversion H; subst; auto.
Qed.

Lemma resume_exact : forall waiting value parent,
  resume waiting value = Some parent ->
  awaiting (saved_parent waiting) (saved_request waiting) = true /\
  exists stable,
    resolved_stability (request_role_of (saved_request waiting)) value = Some stable /\
    parent = Frame (frame_kind_of (saved_parent waiting))
      (frame_input (saved_parent waiting)) (frame_cursor (saved_parent waiting))
      (ReducerState (request_resume_token (saved_request waiting))
        (reducer_stable (frame_reducer (saved_parent waiting)) && stable) None).
Proof.
  intros waiting value parent. unfold resume.
  destruct (awaiting (saved_parent waiting) (saved_request waiting)) eqn:A; try discriminate.
  destruct (resolved_stability (request_role_of (saved_request waiting)) value) eqn:R;
    try discriminate.
  intro E. inversion E; subst. split; auto. exists b; auto.
Qed.

Lemma finished_exact : forall current value,
  finished current value = true ->
  frame_cursor current = length (region_bytes (frame_input current)) /\
  reducer_wait (frame_reducer current) = None /\
  frame_kind_of current = summary_kind value.
Proof.
  intros current value. unfold finished. rewrite Bool.andb_true_iff.
  intros [C S]. apply Nat.eqb_eq in C.
  destruct (reducer_wait (frame_reducer current)) eqn:W; try discriminate.
  apply Bool.andb_true_iff in S. destruct S as [S _].
  apply kind_eqb_exact in S. auto.
Qed.

Lemma linked_frame_ext : forall current updated parents,
  frame_kind_of updated = frame_kind_of current ->
  frame_input updated = frame_input current ->
  linked current parents -> linked updated parents.
Proof.
  intros current updated [|waiting rest] K I L; simpl in *; congruence.
Qed.

Lemma root_frame_ext : forall current updated parents,
  frame_input updated = frame_input current ->
  root_input updated parents = root_input current parents.
Proof. intros current updated [|waiting rest] I; simpl; auto. Qed.

Lemma transport_retains_root : forall input current parents action next events,
  retains_root input (Running current parents) ->
  transport current parents action = (next, events) -> retains_root input next.
Proof.
  intros input current parents action next events [L R].
  destruct action as [count state|child|value|code|attempt]; simpl.
  - destruct (advance current count state) as [updated|] eqn:A; intro E;
      inversion E; subst; simpl; auto.
    apply advance_exact in A. destruct A as [_ [_ [_ A]]]. subst updated.
    split.
    + eapply linked_frame_ext with (current := current); [reflexivity|reflexivity|exact L].
    + rewrite root_frame_ext with (current := current); reflexivity.
  - destruct (dispatch current child) as [[active waiting]|] eqn:D; intro E;
      inversion E; subst; simpl; auto.
    apply dispatch_exact in D. destruct D as [A [D W]]. subst active waiting.
    simpl. repeat split; auto.
  - destruct (finished current value) eqn:F; [|intro E; inversion E; subst; exact I].
    apply finished_exact in F. destruct F as [C [W K]].
    destruct parents as [|waiting rest].
    + intro E. inversion E; subst. simpl in *. split; auto. congruence.
    + destruct (resume waiting value) as [parent|] eqn:S; intro E;
        inversion E; subst; simpl; auto.
      apply resume_exact in S. destruct S as [_ [stable [_ S]]]. subst parent.
      simpl in L. destruct L as [_ [_ [_ L]]].
      split.
      * eapply linked_frame_ext with (current := saved_parent waiting);
          [reflexivity|reflexivity|exact L].
      * rewrite root_frame_ext with (current := saved_parent waiting); reflexivity.
  - intro E. inversion E; subst; exact I.
  - intro E. inversion E; subst; exact I.
Qed.

Lemma step_retains_root : forall authorize input state action next events,
  retains_root input state -> step authorize state action = (next, events) ->
  retains_root input next.
Proof.
  intros authorize input [current parents|result] action next events H; simpl.
  - destruct (authorize current action); [eapply transport_retains_root; eauto|].
    intro E. inversion E; subst; exact I.
  - intro E. inversion E; subst; exact H.
Qed.

Lemma run_retains_root : forall authorize actions input state next events,
  retains_root input state -> run authorize actions state = (next, events) ->
  retains_root input next.
Proof.
  intros authorize actions. induction actions as [|action rest IH];
    intros input state next events H; simpl.
  - intro E. inversion E; subst; exact H.
  - destruct (step authorize state action) as [middle first] eqn:S.
    destruct (run authorize rest middle) as [last later] eqn:R.
    intro E. inversion E; subst. eapply IH.
    + eapply step_retains_root; [exact H|exact S].
    + exact R.
Qed.

Theorem standalone_acceptance_preserves_exact_root : forall authorize actions input output value events,
  run authorize actions (Running (fresh_frame KeyFrame input) []) =
    (Halted (Accepted output value), events) ->
  output = input /\ exists stable, value = KeySummary stable.
Proof.
  intros authorize actions input output value events E.
  assert (H : retains_root input (Running (fresh_frame KeyFrame input) [])).
  { split; reflexivity. }
  pose proof (@run_retains_root authorize actions input _ _ _ H E) as R.
  simpl in R. destruct R as [R K].
  split; auto. destruct value; simpl in K; try discriminate; eauto.
Qed.

Theorem completion_requires_full_consumption_without_pending_child : forall current value,
  finished current value = true ->
  frame_cursor current = length (region_bytes (frame_input current)) /\
  reducer_wait (frame_reducer current) = None /\
  frame_kind_of current = summary_kind value.
Proof. exact finished_exact. Qed.

Theorem saved_parent_id_kind_and_resolution_are_exact : forall waiting value parent,
  resume waiting value = Some parent ->
  reducer_wait (frame_reducer (saved_parent waiting)) =
    Some (request_id (saved_request waiting), request_role_of (saved_request waiting)) /\
  summary_kind value = role_kind (request_role_of (saved_request waiting)) /\
  frame_cursor parent = frame_cursor (saved_parent waiting) /\
  frame_input parent = frame_input (saved_parent waiting) /\
  reducer_wait (frame_reducer parent) = None.
Proof.
  intros waiting value parent E. apply resume_exact in E.
  destruct E as [A [stable [S P]]]. subst parent. apply awaiting_exact in A.
  simpl. repeat split; auto.
  destruct (request_role_of (saved_request waiting)), value;
    simpl in S; try discriminate; auto;
    repeat match goal with H : context[match ?b with _ => _ end] |- _ => destruct b end;
    discriminate.
Qed.

Theorem canonical_resolution_rules_preserve_valid_unstable :
  resolved_stability RawParEscape (MessageSummary false) = Some false /\
  resolved_stability RawParEscape (MessageSummary true) = None /\
  resolved_stability NestedMap (EpmSummary MapMode true) = Some true /\
  resolved_stability NestedMap (EpmSummary MapMode false) = None /\
  (forall stable, resolved_stability NestedMap (EpmSummary EmptyMode stable) = None) /\
  (forall stable, resolved_stability NestedMap (EpmSummary SetMode stable) = None).
Proof. repeat split; reflexivity. Qed.

Theorem suspended_cursor_and_ordered_read_are_preserved :
  (forall current child active waiting,
    dispatch current child = Some (active, waiting) ->
    saved_parent waiting = current /\ saved_request waiting = child /\
    frame_cursor active = 0 /\ frame_input active = request_payload child) /\
  (forall current count state updated,
    advance current count state = Some updated ->
    frame_cursor updated = frame_cursor current + count /\
    frame_cursor updated <= length (region_bytes (frame_input current)) /\
    transport current [] (Consume count state) =
      (Running updated [], [ReadBytes (frame_input current) (frame_cursor current) count])) /\
  (forall current id role count state,
    reducer_wait (frame_reducer current) = Some (id, role) ->
    advance current count state = None).
Proof.
  split.
  - intros current child active waiting D. apply dispatch_exact in D.
    destruct D as [_ [A W]]. subst. simpl. auto.
  - split.
    + intros current count state updated A. pose proof A as B. apply advance_exact in B.
      destruct B as [_ [_ [B U]]]. subst. simpl. repeat split; auto.
      unfold transport. rewrite A. reflexivity.
    + intros current id role count state W. unfold advance. rewrite W. reflexivity.
Qed.

Theorem child_failure_is_terminal : forall authorize actions reason,
  run authorize actions (Halted (Rejected reason)) = (Halted (Rejected reason), []) /\
  forall current parents code,
    transport current parents (Fail code) = reject (ChildRefusal code).
Proof.
  intros authorize actions reason. split.
  - induction actions; simpl; [reflexivity|rewrite IHactions; reflexivity].
  - reflexivity.
Qed.

Theorem child_payload_retains_owner_bytes_and_provenance : forall sources current child active waiting,
  dispatch current child = Some (active, waiting) ->
  source_refines sources (request_payload child) ->
  frame_input active = request_payload child /\
  region_owner (frame_input active) = region_owner (request_payload child) /\
  region_bytes (frame_input active) = region_bytes (request_payload child) /\
  region_eof_source (frame_input active) = region_eof_source (request_payload child) /\
  region_eof_offset (frame_input active) = region_eof_offset (request_payload child) /\
  source_refines sources (frame_input active).
Proof.
  intros sources current child active waiting D S. apply dispatch_exact in D.
  destruct D as [_ [D _]]. subst. simpl.
  repeat apply conj; try reflexivity; exact S.
Qed.

Theorem stable_only_caller_preserves_three_outcomes :
  (forall result, stable_only result = true <-> validity_stability result = Some true) /\
  (forall input, validity_stability (Accepted input (KeySummary false)) = Some false /\
    stable_only (Accepted input (KeySummary false)) = false) /\
  (forall reason, validity_stability (Rejected reason) = None /\
    stable_only (Rejected reason) = false) /\
  (forall result reference,
    validity_stability result = reference ->
    stable_only result = match reference with Some true => true | _ => false end).
Proof.
  split.
  - intros result. destruct result as [reason|input [stable|stable|mode stable]]; simpl;
      try (split; discriminate); destruct stable; simpl; intuition discriminate.
  - split; [intros; split; reflexivity|].
    split; [intros; split; reflexivity|].
    intros result reference E. subst reference.
    destruct result as [reason|input [stable|stable|mode stable]];
      simpl; reflexivity.
Qed.

Definition native_effect (event : effect) : Prop :=
  match event with TypedDecode | ReplayEndpoint | ReconstructKey => False | _ => True end.

Lemma transport_effects_are_native : forall current parents action,
  Forall native_effect (snd (transport current parents action)).
Proof.
  intros current parents [count state|child|value|code|attempt]; simpl.
  - destruct (advance current count state); repeat constructor.
  - destruct (dispatch current child) as [[active waiting]|]; repeat constructor.
  - destruct (finished current value); [destruct parents as [|waiting rest]|];
      try (repeat constructor).
    destruct (resume waiting value); repeat constructor.
  - repeat constructor.
  - repeat constructor.
Qed.

Lemma step_effects_are_native : forall authorize state action,
  Forall native_effect (snd (step authorize state action)).
Proof.
  intros authorize [current parents|result] action; simpl.
  - destruct (authorize current action); [apply transport_effects_are_native|repeat constructor].
  - constructor.
Qed.

Theorem shared_loop_has_no_forbidden_transport_effect : forall authorize actions state next events,
  run authorize actions state = (next, events) -> Forall native_effect events.
Proof.
  intros authorize actions. induction actions as [|action rest IH];
    intros state next events; simpl.
  - intro E. inversion E; constructor.
  - destruct (step authorize state action) as [middle first] eqn:S.
    destruct (run authorize rest middle) as [last later] eqn:R.
    intro E. inversion E; subst. apply Forall_app. split.
    + pose proof (step_effects_are_native authorize state action) as N. rewrite S in N. exact N.
    + eapply IH; exact R.
Qed.

Definition control_region := Region 7 [LocatedByte 42 3 10] 3 11.
Definition control_payload := Region 7 [LocatedByte 0 3 20] 3 21.
Definition control_request := Request 9 RawParEscape control_payload 31.
Definition control_wait := ReducerState 20 true (Some (9, RawParEscape)).
Definition control_parent := Frame KeyFrame control_region 1 control_wait.
Definition control_child := fresh_frame MessageFrame control_payload.
Definition control_authorize (_ : frame) (_ : command) := true.
Definition control_script :=
  [Consume 1 control_wait; Dispatch control_request;
   Consume 1 (ReducerState 8 false None); Complete (MessageSummary false);
   Complete (KeySummary false)].

Theorem standalone_protocol_positive_controls :
  fst (run control_authorize control_script
    (Running (fresh_frame KeyFrame control_region) [])) =
    Halted (Accepted control_region (KeySummary false)) /\
  fst (run control_authorize
    [Consume 1 (ReducerState 1 true None); Complete (KeySummary true)]
    (Running (fresh_frame KeyFrame control_region) [])) =
    Halted (Accepted control_region (KeySummary true)) /\
  resume (Suspended
    (Frame KeyFrame control_region 1 (ReducerState 1 true (Some (9, NestedMap))))
    (Request 9 NestedMap control_payload 31)) (EpmSummary MapMode true) =
    Some (Frame KeyFrame control_region 1 (ReducerState 31 true None)).
Proof. repeat split; reflexivity. Qed.

Definition omit_consumption (current : frame) (value : summary) :=
  Accepted (frame_input current) value.
Definition omit_resolution (_ : suspended) (_ : summary) := true.
Definition unstable_as_invalid (stable : bool) : option bool :=
  if stable then Some true else None.

Theorem standalone_protocol_causal_refusal_controls :
  finished (fresh_frame KeyFrame control_region) (KeySummary true) = false /\
  omit_consumption (fresh_frame KeyFrame control_region) (KeySummary true) =
    Accepted control_region (KeySummary true) /\
  finished control_parent (KeySummary true) = false /\
  advance control_parent 1 (ReducerState 0 true None) = None /\
  resume (Suspended control_parent
    (Request 10 RawParEscape control_payload 31)) (MessageSummary false) = None /\
  resume (Suspended control_parent
    (Request 9 NestedMap control_payload 31)) (EpmSummary MapMode true) = None /\
  resume (Suspended control_parent control_request) (EpmSummary MapMode true) = None /\
  resume (Suspended control_parent control_request) (MessageSummary true) = None /\
  omit_resolution (Suspended control_parent control_request) (MessageSummary true) = true /\
  unstable_as_invalid false <> validity_stability (Accepted control_region (KeySummary false)) /\
  fst (run control_authorize
    [Consume 1 control_wait; Dispatch control_request; Fail 42; Complete (KeySummary true)]
    (Running (fresh_frame KeyFrame control_region) [])) = Halted (Rejected (ChildRefusal 42)) /\
  fst (step control_authorize (Running (fresh_frame KeyFrame control_region) [])
    (Forbidden TypedDecode)) = Halted (Rejected ForbiddenEffectRefusal).
Proof. repeat split; discriminate || reflexivity. Qed.

Record parser_refinement
    (authorize : frame -> command -> bool)
    (sources : nat -> list nat)
    (read_authority : frame -> nat -> reducer_state -> Prop)
    (payload_authority : frame -> request -> Prop)
    (terminal_authority : region -> summary -> Prop) : Prop := ParserRefinement {
  authorized_read_refines : forall current count next,
    authorize current (Consume count next) = true ->
    read_authority current count next;
  authorized_payload_refines : forall current child,
    authorize current (Dispatch child) = true ->
    payload_authority current child /\ source_refines sources (request_payload child);
  authorized_terminal_refines : forall current value,
    authorize current (Complete value) = true ->
    source_refines sources (frame_input current) /\
    terminal_authority (frame_input current) value
}.

Definition checked_outcome (sources : nat -> list nat)
    (terminal_authority : region -> summary -> Prop) (state : machine) : Prop :=
  match state with
  | Halted (Accepted input value) =>
      source_refines sources input /\ terminal_authority input value
  | _ => True
  end.

Lemma transport_preserves_checked_outcome : forall authorize sources reads payloads terminals,
  parser_refinement authorize sources reads payloads terminals ->
  forall current parents action next events,
  authorize current action = true ->
  transport current parents action = (next, events) ->
  checked_outcome sources terminals next.
Proof.
  intros authorize sources reads payloads terminals P current parents action next events A.
  destruct action as [count state|child|value|code|attempt]; simpl.
  - destruct (advance current count state); intro E; inversion E; subst; exact I.
  - destruct (dispatch current child) as [[active waiting]|];
      intro E; inversion E; subst; exact I.
  - destruct (finished current value); [destruct parents as [|waiting rest]|].
    + intro E. inversion E; subst. simpl.
      eapply authorized_terminal_refines; [exact P|exact A].
    + destruct (resume waiting value); intro E; inversion E; subst; exact I.
    + intro E. inversion E; subst; exact I.
  - intro E. inversion E; subst; exact I.
  - intro E. inversion E; subst; exact I.
Qed.

Lemma run_preserves_checked_outcome : forall authorize sources reads payloads terminals,
  parser_refinement authorize sources reads payloads terminals ->
  forall actions state next events,
  checked_outcome sources terminals state ->
  run authorize actions state = (next, events) ->
  checked_outcome sources terminals next.
Proof.
  intros authorize sources reads payloads terminals P actions.
  induction actions as [|action rest IH]; intros state next events H; simpl.
  - intro E. inversion E; subst; exact H.
  - destruct (step authorize state action) as [middle first] eqn:S.
    destruct (run authorize rest middle) as [last later] eqn:R.
    intro E. inversion E; subst. eapply IH; [|exact R].
    destruct state as [current parents|result]; simpl in S.
    + destruct (authorize current action) eqn:A.
      * eapply transport_preserves_checked_outcome; [exact P|exact A|exact S].
      * inversion S; subst; exact I.
    + inversion S; subst; exact H.
Qed.

Theorem standalone_acceptance_inherits_explicit_parser_authority :
  forall authorize sources reads payloads terminals,
  parser_refinement authorize sources reads payloads terminals ->
  forall actions input output value events,
  run authorize actions (Running (fresh_frame KeyFrame input) []) =
    (Halted (Accepted output value), events) ->
  output = input /\ (exists stable, value = KeySummary stable) /\
  source_refines sources input /\ terminals input value.
Proof.
  intros authorize sources reads payloads terminals P actions input output value events E.
  pose proof (@standalone_acceptance_preserves_exact_root authorize actions input output value events E)
    as [R K]. subst output.
  assert (C : checked_outcome sources terminals (Halted (Accepted input value))).
  { eapply run_preserves_checked_outcome
      with (state := Running (fresh_frame KeyFrame input) []);
      [exact P|exact I|exact E]. }
  simpl in C. tauto.
Qed.

Print Assumptions standalone_acceptance_inherits_explicit_parser_authority.
Print Assumptions completion_requires_full_consumption_without_pending_child.
Print Assumptions saved_parent_id_kind_and_resolution_are_exact.
Print Assumptions canonical_resolution_rules_preserve_valid_unstable.
Print Assumptions suspended_cursor_and_ordered_read_are_preserved.
Print Assumptions child_failure_is_terminal.
Print Assumptions child_payload_retains_owner_bytes_and_provenance.
Print Assumptions stable_only_caller_preserves_three_outcomes.
Print Assumptions shared_loop_has_no_forbidden_transport_effect.
Print Assumptions standalone_protocol_positive_controls.
Print Assumptions standalone_protocol_causal_refusal_controls.

End CanonicalKeyProtocol.

Module ExclusiveCurrentState.

Section Storage.

Context {State World Input Observation Error : Type}.

Definition transition_result := ((State * World * Observation) + (World * Error))%type.

Variable checked_transition : State -> World -> Input -> transition_result.

Record history := History {
  history_current : State;
  history_previous : list State
}.

Inductive exclusive := Current (value : State).

Definition exclusive_current (store : exclusive) : State :=
  match store with Current value => value end.

Definition history_entries (store : history) : list State :=
  rev (history_previous store) ++ [history_current store].

Definition exclusive_entries (store : exclusive) : list State :=
  match store with Current value => [value] end.

Definition append_history (store : history) (next : State) : history :=
  History next (history_current store :: history_previous store).

Record result (Storage : Type) := Result {
  result_store : Storage;
  result_world : World;
  result_observations : list Observation;
  result_attempted : list Input;
  result_error : option Error
}.

Arguments Result {Storage}.

Definition add_observation {Storage : Type} (input : Input) (observation : Observation)
    (rest : result Storage) : result Storage :=
  Result (result_store rest) (result_world rest)
    (observation :: result_observations rest) (input :: result_attempted rest)
    (result_error rest).

Fixpoint historical_run (inputs : list Input) (store : history) (world : World)
    : result history :=
  match inputs with
  | [] => Result store world [] [] None
  | input :: rest =>
      match checked_transition (history_current store) world input with
      | inl (next, next_world, observation) =>
          add_observation input observation
            (historical_run rest (append_history store next) next_world)
      | inr (next_world, error) => Result store next_world [] [input] (Some error)
      end
  end.

Fixpoint exclusive_run (inputs : list Input) (store : exclusive) (world : World)
    : result exclusive :=
  match inputs with
  | [] => Result store world [] [] None
  | input :: rest =>
      match checked_transition (exclusive_current store) world input with
      | inl (next, next_world, observation) =>
          add_observation input observation
            (exclusive_run rest (Current next) next_world)
      | inr (next_world, error) => Result store next_world [] [input] (Some error)
      end
  end.

Record observation_view := ObservationView {
  view_current : State;
  view_world : World;
  view_observations : list Observation;
  view_attempted : list Input;
  view_error : option Error
}.

Definition historical_view (output : result history) : observation_view :=
  ObservationView (history_current (result_store output)) (result_world output)
    (result_observations output) (result_attempted output) (result_error output).

Definition exclusive_view (output : result exclusive) : observation_view :=
  ObservationView (exclusive_current (result_store output)) (result_world output)
    (result_observations output) (result_attempted output) (result_error output).

Lemma add_observation_preserves_view : forall input observation historical exclusive_result,
  historical_view historical = exclusive_view exclusive_result ->
  historical_view (add_observation input observation historical) =
    exclusive_view (add_observation input observation exclusive_result).
Proof.
  intros input observation [hs hw ho hi he] [es ew eo ei ee].
  unfold historical_view, exclusive_view, add_observation; simpl.
  intro E. inversion E; subst; reflexivity.
Qed.

Theorem historical_storage_has_exact_nonempty_latest_projection : forall store,
  history_entries store <> [] /\
  hd_error (rev (history_entries store)) = Some (history_current store) /\
  length (history_entries store) = S (length (history_previous store)).
Proof.
  intros [current previous]. unfold history_entries; simpl.
  repeat apply conj.
  - intro E. apply (f_equal (@length State)) in E.
    rewrite length_app, length_rev in E. simpl in E. lia.
  - rewrite rev_app_distr. simpl. reflexivity.
  - rewrite length_app, length_rev. simpl. lia.
Qed.

Theorem exclusive_and_history_have_identical_full_observations : forall inputs store world,
  historical_view (historical_run inputs store world) =
  exclusive_view (exclusive_run inputs (Current (history_current store)) world).
Proof.
  induction inputs as [|input rest IH]; intros store world; simpl.
  - reflexivity.
  - destruct (checked_transition (history_current store) world input)
      as [[[next next_world] observation]|[next_world error]] eqn:E; simpl.
    + apply add_observation_preserves_view. apply IH.
    + reflexivity.
Qed.

Theorem checked_failure_stops_both_drivers_at_the_same_state :
  forall input rest store world next_world error,
  checked_transition (history_current store) world input = inr (next_world, error) ->
  historical_run (input :: rest) store world =
    Result store next_world [] [input] (Some error) /\
  exclusive_run (input :: rest) (Current (history_current store)) world =
    Result (Current (history_current store)) next_world [] [input] (Some error).
Proof. intros input rest store world next_world error E. simpl. rewrite E. split; reflexivity. Qed.

Inductive grammar_execution
    (grammar : State -> World -> Input -> transition_result -> Prop)
    : list Input -> State -> World -> observation_view -> Prop :=
| GrammarDone : forall state world,
    grammar_execution grammar [] state world
      (ObservationView state world [] [] None)
| GrammarRejected : forall input rest state world next_world error,
    grammar state world input (inr (next_world, error)) ->
    grammar_execution grammar (input :: rest) state world
      (ObservationView state next_world [] [input] (Some error))
| GrammarAccepted : forall input rest state world next next_world observation output,
    grammar state world input (inl (next, next_world, observation)) ->
    grammar_execution grammar rest next next_world output ->
    grammar_execution grammar (input :: rest) state world
      (ObservationView (view_current output) (view_world output)
        (observation :: view_observations output) (input :: view_attempted output)
        (view_error output)).

Lemma exclusive_run_inherits_grammar : forall grammar,
  (forall state world input,
    grammar state world input (checked_transition state world input)) ->
  forall inputs current world,
  grammar_execution grammar inputs current world
    (exclusive_view (exclusive_run inputs (Current current) world)).
Proof.
  intros grammar G inputs. induction inputs as [|input rest IH]; intros current world; simpl.
  - constructor.
  - destruct (checked_transition current world input)
      as [[[next next_world] observation]|[next_world error]] eqn:E.
    + unfold exclusive_view, add_observation; simpl.
      apply GrammarAccepted with (next := next) (next_world := next_world)
        (output := exclusive_view (exclusive_run rest (Current next) next_world)).
      * specialize (G current world input). rewrite E in G. exact G.
      * exact (IH next next_world).
    + apply GrammarRejected. specialize (G current world input). rewrite E in G. exact G.
Qed.

Theorem both_storage_policies_inherit_the_explicit_grammar_premise : forall grammar,
  (forall state world input,
    grammar state world input (checked_transition state world input)) ->
  forall inputs store world,
  grammar_execution grammar inputs (history_current store) world
    (historical_view (historical_run inputs store world)) /\
  grammar_execution grammar inputs (history_current store) world
    (exclusive_view (exclusive_run inputs (Current (history_current store)) world)).
Proof.
  intros grammar G inputs store world.
  rewrite exclusive_and_history_have_identical_full_observations.
  split; apply exclusive_run_inherits_grammar; exact G.
Qed.

Theorem exclusive_storage_retains_exactly_one_state : forall inputs current world,
  length (exclusive_entries (result_store (exclusive_run inputs (Current current) world))) = 1.
Proof.
  intros inputs current world.
  destruct (result_store (exclusive_run inputs (Current current) world)); reflexivity.
Qed.

Lemma append_history_length : forall store next,
  length (history_entries (append_history store next)) = S (length (history_entries store)).
Proof.
  intros [current previous] next. unfold history_entries, append_history; simpl.
  repeat rewrite length_app. simpl. lia.
Qed.

Theorem historical_growth_counts_only_successful_transitions : forall inputs store world,
  length (history_entries (result_store (historical_run inputs store world))) =
    length (history_entries store) +
      length (result_observations (historical_run inputs store world)).
Proof.
  induction inputs as [|input rest IH]; intros store world; simpl.
  - lia.
  - destruct (checked_transition (history_current store) world input)
      as [[[next next_world] observation]|[next_world error]] eqn:E; simpl.
    + specialize (IH (append_history store next) next_world).
      rewrite append_history_length in IH. simpl. lia.
    + lia.
Qed.

Lemma exclusive_world_invariant : forall invariant : World -> Prop,
  (forall state world input next next_world observation,
    invariant world ->
    checked_transition state world input = inl (next, next_world, observation) ->
    invariant next_world) ->
  (forall state world input next_world error,
    invariant world -> checked_transition state world input = inr (next_world, error) ->
    invariant next_world) ->
  forall inputs current world,
  invariant world -> invariant (result_world (exclusive_run inputs (Current current) world)).
Proof.
  intros invariant P Q inputs. induction inputs as [|input rest IH]; intros current world W; simpl.
  - exact W.
  - destruct (checked_transition current world input)
      as [[[next next_world] observation]|[next_world error]] eqn:E; simpl.
    + apply IH. eapply P; [exact W|exact E].
    + eapply Q; [exact W|exact E].
Qed.

Theorem shared_world_effects_and_invariants_are_preserved :
  (forall inputs store world,
    result_world (historical_run inputs store world) =
    result_world (exclusive_run inputs (Current (history_current store)) world)) /\
  (forall invariant : World -> Prop,
    (forall state world input next next_world observation,
      invariant world ->
      checked_transition state world input = inl (next, next_world, observation) ->
      invariant next_world) ->
    (forall state world input next_world error,
      invariant world -> checked_transition state world input = inr (next_world, error) ->
      invariant next_world) ->
    forall inputs store world, invariant world ->
    invariant (result_world (historical_run inputs store world)) /\
    invariant (result_world (exclusive_run inputs (Current (history_current store)) world))).
Proof.
  assert (W : forall inputs store world,
    result_world (historical_run inputs store world) =
    result_world (exclusive_run inputs (Current (history_current store)) world)).
  { intros inputs store world.
    pose proof (exclusive_and_history_have_identical_full_observations inputs store world) as E.
    exact (f_equal view_world E). }
  split; [exact W|].
  intros invariant P Q inputs store world I. rewrite W.
  split; apply exclusive_world_invariant; assumption.
Qed.

End Storage.

Record control_state := ControlState {
  control_value : nat;
  control_stable : bool
}.

Record control_world := ControlWorld {
  control_frames : nat;
  control_payload_slots : nat;
  control_next_id : nat
}.

Inductive control_input := Advance | BecomeUnstable | Resolve | RefuseInput.

Definition control_transition (state : control_state) (world : control_world)
    (input : control_input) : ((control_state * control_world * bool) + (control_world * nat))%type :=
  match input with
  | Advance => inl
      (ControlState (S (control_value state)) (control_stable state),
       ControlWorld (S (control_frames world)) (control_payload_slots world + 2)
         (control_next_id world), control_stable state)
  | BecomeUnstable => inl
      (ControlState (control_value state) false,
       ControlWorld (control_frames world) (control_payload_slots world)
         (S (control_next_id world)), false)
  | Resolve => inl
      (ControlState (control_value state + control_next_id world) (control_stable state),
       ControlWorld (S (control_frames world)) (control_payload_slots world)
         (control_next_id world), control_stable state)
  | RefuseInput => inr
      (ControlWorld (S (control_frames world)) (control_payload_slots world + 3)
        (control_next_id world), 42)
  end.

Definition initial_control_state := ControlState 0 true.
Definition initial_control_world := ControlWorld 3 7 11.
Definition control_history := History initial_control_state [].
Definition control_exclusive := Current initial_control_state.

Theorem exclusive_storage_positive_controls :
  historical_view (historical_run control_transition
    [Advance; BecomeUnstable; Resolve] control_history initial_control_world) =
    ObservationView (ControlState 13 false) (ControlWorld 5 9 12)
      [true; false; false] [Advance; BecomeUnstable; Resolve] None /\
  exclusive_view (exclusive_run control_transition
    [Advance; BecomeUnstable; Resolve] control_exclusive initial_control_world) =
    ObservationView (ControlState 13 false) (ControlWorld 5 9 12)
      [true; false; false] [Advance; BecomeUnstable; Resolve] None /\
  length (history_entries (result_store (historical_run control_transition
    [Advance; BecomeUnstable; Resolve] control_history initial_control_world))) = 4 /\
  length (exclusive_entries (result_store (exclusive_run control_transition
    [Advance; BecomeUnstable; Resolve] control_exclusive initial_control_world))) = 1.
Proof. repeat split; reflexivity. Qed.

Definition wrong_oldest_current (store : @history control_state) : control_state :=
  hd (history_current store) (rev (history_previous store)).

Definition wrong_reset_world_transition (state : control_state) (_ : control_world)
    (input : control_input) := control_transition state initial_control_world input.

Definition wrong_refusal_transition (state : control_state) (world : control_world)
    (input : control_input) : ((control_state * control_world * bool) + (control_world * nat))%type :=
  match input with
  | RefuseInput => inl (state, world, control_stable state)
  | _ => control_transition state world input
  end.

Definition wrong_error_rollback_transition (state : control_state) (world : control_world)
    (input : control_input) :=
  match control_transition state world input with
  | inl success => inl success
  | inr (_, error) => inr (world, error)
  end.

Theorem exclusive_storage_causal_refusal_controls :
  wrong_oldest_current (append_history control_history (ControlState 1 true)) <>
    history_current (append_history control_history (ControlState 1 true)) /\
  exclusive_view (exclusive_run wrong_reset_world_transition [Advance; Resolve]
    control_exclusive initial_control_world) <>
    exclusive_view (exclusive_run control_transition [Advance; Resolve]
      control_exclusive initial_control_world) /\
  exclusive_view (exclusive_run control_transition
    [BecomeUnstable; RefuseInput; Advance] control_exclusive initial_control_world) =
    ObservationView (ControlState 0 false) (ControlWorld 4 10 12)
      [false] [BecomeUnstable; RefuseInput] (Some 42) /\
  exclusive_view (exclusive_run wrong_refusal_transition
    [BecomeUnstable; RefuseInput; Advance] control_exclusive initial_control_world) <>
    exclusive_view (exclusive_run control_transition
      [BecomeUnstable; RefuseInput; Advance] control_exclusive initial_control_world) /\
  exclusive_view (exclusive_run wrong_error_rollback_transition
    [BecomeUnstable; RefuseInput; Advance] control_exclusive initial_control_world) <>
    exclusive_view (exclusive_run control_transition
      [BecomeUnstable; RefuseInput; Advance] control_exclusive initial_control_world).
Proof. repeat split; discriminate || reflexivity. Qed.

Print Assumptions historical_storage_has_exact_nonempty_latest_projection.
Print Assumptions exclusive_and_history_have_identical_full_observations.
Print Assumptions checked_failure_stops_both_drivers_at_the_same_state.
Print Assumptions both_storage_policies_inherit_the_explicit_grammar_premise.
Print Assumptions exclusive_storage_retains_exactly_one_state.
Print Assumptions historical_growth_counts_only_successful_transitions.
Print Assumptions shared_world_effects_and_invariants_are_preserved.
Print Assumptions exclusive_storage_positive_controls.
Print Assumptions exclusive_storage_causal_refusal_controls.

End ExclusiveCurrentState.

From Stdlib Require Import Sorting.Permutation.

Module ReusableFrameOwnership.

Record obligation_metadata := ObligationMetadata {
  obligation_number : nat;
  obligation_kind : nat;
  obligation_payload_start : nat;
  obligation_source : nat;
  obligation_offset : nat;
  obligation_status : nat
}.

Record frame_metadata := FrameMetadata {
  grammar_fields : list nat;
  frame_obligation : option obligation_metadata
}.

Record cell := Cell {
  cell_metadata : frame_metadata;
  cell_next : option nat;
  cell_region_child : option nat;
  cell_is_region : bool
}.

Inductive graph :=
| EmptyGraph
| LinkGraph (slot : nat) (metadata : frame_metadata) (next : graph)
| RegionGraph (slot : nat) (metadata : frame_metadata) (child next : graph).

Definition graph_root (value : graph) : option nat :=
  match value with
  | EmptyGraph => None
  | LinkGraph slot _ _ | RegionGraph slot _ _ _ => Some slot
  end.

Fixpoint graph_slots (value : graph) : list nat :=
  match value with
  | EmptyGraph => []
  | LinkGraph slot _ next => slot :: graph_slots next
  | RegionGraph slot _ child next => slot :: graph_slots child ++ graph_slots next
  end.

Fixpoint lookup (slot : nat) (cells : list (nat * cell)) : option cell :=
  match cells with
  | [] => None
  | (index, value) :: rest =>
      if Nat.eq_dec slot index then Some value else lookup slot rest
  end.

Fixpoint realizes (cells : list (nat * cell)) (value : graph) : Prop :=
  match value with
  | EmptyGraph => True
  | LinkGraph slot metadata next =>
      lookup slot cells = Some (Cell metadata (graph_root next) None false) /\
      realizes cells next
  | RegionGraph slot metadata child next =>
      lookup slot cells = Some (Cell metadata (graph_root next) (graph_root child) true) /\
      realizes cells child /\ realizes cells next
  end.

Record pool := Pool {
  pool_capacity : nat;
  pool_cells : list (nat * cell);
  pool_free : list nat
}.

Definition pool_ids (value : pool) := map fst (pool_cells value).

Definition pool_valid (value : pool) : Prop :=
  NoDup (pool_ids value ++ pool_free value) /\
  (forall slot, In slot (pool_ids value ++ pool_free value) ->
    slot < pool_capacity value) /\
  length (pool_cells value) + length (pool_free value) = pool_capacity value.

Definition reserve (value : pool) (contents : cell) : nat * pool :=
  match pool_free value with
  | slot :: rest =>
      (slot, Pool (pool_capacity value)
        ((slot, contents) :: pool_cells value) rest)
  | [] =>
      (pool_capacity value, Pool (S (pool_capacity value))
        ((pool_capacity value, contents) :: pool_cells value) [])
  end.

Lemma lookup_some_is_occupied slot cells contents :
  lookup slot cells = Some contents -> In slot (map fst cells).
Proof.
  induction cells as [|[index value] rest IH]; cbn; intro found; try discriminate.
  destruct (Nat.eq_dec slot index); [now left|right; now apply IH].
Qed.

Lemma absent_lookup slot cells :
  ~ In slot (map fst cells) -> lookup slot cells = None.
Proof.
  induction cells as [|[index value] rest IH]; cbn; intro absent; auto.
  destruct (Nat.eq_dec slot index); [subst; exfalso; apply absent; now left|].
  apply IH. intro inside. apply absent. now right.
Qed.

Lemma nodup_append_disjoint {A : Type} (left right : list A) :
  NoDup (left ++ right) -> forall value, In value left -> ~ In value right.
Proof.
  induction left as [|head tail IH]; cbn; intros distinct value inside; try contradiction.
  inversion distinct as [|? ? absent rest]; subst.
  destruct inside as [<-|inside].
  - intro found. apply absent. apply in_or_app. now right.
  - now apply (IH rest value inside).
Qed.

Arguments nodup_append_disjoint {A left right} _ value _ _.

Lemma allocation_returns_fresh_cell value contents slot after :
  pool_valid value -> reserve value contents = (slot, after) ->
  ~ In slot (pool_ids value) /\
  pool_cells after = (slot, contents) :: pool_cells value /\
  pool_valid after.
Proof.
  destruct value as [capacity cells free].
  unfold pool_valid, pool_ids, reserve; cbn.
  intros (distinct & bounded & size) allocated.
  destruct free as [|head tail].
  - inversion allocated; subst slot after; clear allocated. cbn in *.
    rewrite app_nil_r in *.
    assert (fresh : ~ In capacity (map fst cells)).
    { intro inside. specialize (bounded capacity inside). lia. }
    split; [exact fresh|]. split; [reflexivity|]. split.
    + constructor; assumption.
    + split.
      * intros index [<-|inside]; [lia|specialize (bounded index inside); lia].
      * lia.
  - inversion allocated; subst slot after; clear allocated. cbn in *.
    assert (fresh : ~ In head (map fst cells ++ tail)).
    { exact (NoDup_remove_2 _ _ _ distinct). }
    split.
    + intro inside. apply fresh. apply in_or_app. now left.
    + split; [reflexivity|]. split.
      * constructor; [exact fresh|exact (NoDup_remove_1 _ _ _ distinct)].
      * split.
        -- intros index [<-|inside].
           ++ apply bounded. apply in_or_app. right. now left.
           ++ apply bounded. apply in_app_or in inside. apply in_or_app.
              destruct inside; [now left|right; now right].
        -- lia.
Qed.

Lemma allocation_preserves_every_original_cell value contents slot after :
  pool_valid value -> reserve value contents = (slot, after) ->
  forall index old, lookup index (pool_cells value) = Some old ->
    lookup index (pool_cells after) = Some old.
Proof.
  intros valid allocated index old found.
  destruct (allocation_returns_fresh_cell contents valid allocated) as (fresh & cells & _).
  rewrite cells. cbn. destruct (Nat.eq_dec index slot); [|exact found].
  subst. exfalso. apply fresh. now apply lookup_some_is_occupied in found.
Qed.

Lemma realizes_uses_only_occupied cells value :
  realizes cells value -> forall slot, In slot (graph_slots value) ->
    In slot (map fst cells).
Proof.
  induction value as [|index metadata next IH|index metadata child IHchild next IHnext];
    cbn; intros valid slot inside; try contradiction.
  - destruct valid as [found rest]. destruct inside as [<-|inside].
    + now apply lookup_some_is_occupied in found.
    + now apply IH.
  - destruct valid as [found [left right]]. destruct inside as [<-|inside].
    + now apply lookup_some_is_occupied in found.
    + apply in_app_or in inside. destruct inside.
      * now apply IHchild.
      * now apply IHnext.
Qed.

Arguments realizes_uses_only_occupied {cells value} _ slot _.

Lemma realizes_transfers_on_its_exact_cells before after value :
  realizes before value ->
  (forall slot, In slot (graph_slots value) -> lookup slot after = lookup slot before) ->
  realizes after value.
Proof.
  induction value as [|slot metadata next IH|slot metadata child IHchild next IHnext];
    cbn; intros valid same; auto.
  - destruct valid as [found rest]. split.
    + rewrite same; [exact found|now left].
    + apply IH; [exact rest|]. intros index inside. apply same. now right.
  - destruct valid as [found [left right]]. split.
    + rewrite same; [exact found|now left].
    + split.
      * apply IHchild; [exact left|]. intros index inside. apply same.
        right. apply in_or_app. now left.
      * apply IHnext; [exact right|]. intros index inside. apply same.
        right. apply in_or_app. now right.
Qed.

Arguments realizes_transfers_on_its_exact_cells {before after value} _ _.

Lemma allocation_preserves_graph value contents slot after root :
  pool_valid value -> reserve value contents = (slot, after) ->
  realizes (pool_cells value) root -> realizes (pool_cells after) root.
Proof.
  intros valid allocated represented. apply (realizes_transfers_on_its_exact_cells represented).
  intros index inside.
  destruct (allocation_returns_fresh_cell contents valid allocated) as (fresh & cells & _).
  rewrite cells. cbn. destruct (Nat.eq_dec index slot); auto.
  subst. exfalso. apply fresh. now apply (realizes_uses_only_occupied represented).
Qed.

Arguments allocation_preserves_graph {value contents slot after root} _ _ _.

Lemma realized_graph_is_unique_at_its_root cells original revised :
  realizes cells original -> realizes cells revised ->
  graph_root original = graph_root revised -> original = revised.
Proof.
  revert revised.
  induction original as [|slot metadata next IHnext|slot metadata child IHchild next IHnext];
    intros revised old_real new_real same; destruct revised; cbn in *; try discriminate; auto.
  - inversion same; subst slot0.
    destruct old_real as [old_head old_next], new_real as [new_head new_next].
    rewrite old_head in new_head. inversion new_head; subst metadata0.
    f_equal. now apply IHnext.
  - inversion same; subst slot0.
    destruct old_real as [old_head _], new_real as [new_head _].
    rewrite old_head in new_head. discriminate.
  - inversion same; subst slot0.
    destruct old_real as [old_head _], new_real as [new_head _].
    rewrite old_head in new_head. discriminate.
  - inversion same; subst slot0.
    destruct old_real as [old_head [old_child old_next]],
      new_real as [new_head [new_child new_next]].
    rewrite old_head in new_head. inversion new_head; subst metadata0.
    f_equal; [now apply IHchild|now apply IHnext].
Qed.

Arguments realized_graph_is_unique_at_its_root {cells original revised} _ _ _.

Lemma realizes_transfers_preserved_cells before after root :
  realizes before root ->
  (forall slot contents, lookup slot before = Some contents -> lookup slot after = Some contents) ->
  realizes after root.
Proof.
  intro represented. induction root; cbn in *; intro retained; auto.
  - destruct represented as [found next_real]. split; [now apply retained|now apply IHroot].
  - destruct represented as [found [child_real next_real]]. split; [now apply retained|].
    split; [now apply IHroot1|now apply IHroot2].
Qed.

Theorem link_construction_refines_concrete_slot_contents value next metadata slot after :
  pool_valid value -> realizes (pool_cells value) next -> NoDup (graph_slots next) ->
  reserve value (Cell metadata (graph_root next) None false) = (slot, after) ->
  realizes (pool_cells after) (LinkGraph slot metadata next) /\
  NoDup (graph_slots (LinkGraph slot metadata next)).
Proof.
  intros valid represented owned allocated.
  destruct (allocation_returns_fresh_cell _ valid allocated) as (fresh & cells & _).
  split; cbn.
  - split.
    + rewrite cells. cbn. destruct (Nat.eq_dec slot slot); congruence.
    + exact (allocation_preserves_graph valid allocated represented).
  - constructor; [|exact owned]. intro inside. apply fresh.
    now apply (realizes_uses_only_occupied represented).
Qed.

Theorem region_construction_preserves_disjoint_child_and_outer value child next metadata slot after :
  pool_valid value -> realizes (pool_cells value) child ->
  realizes (pool_cells value) next ->
  NoDup (graph_slots child ++ graph_slots next) ->
  reserve value (Cell metadata (graph_root next) (graph_root child) true) = (slot, after) ->
  realizes (pool_cells after) (RegionGraph slot metadata child next) /\
  NoDup (graph_slots (RegionGraph slot metadata child next)) /\
  (forall index, In index (graph_slots child) -> ~ In index (graph_slots next)).
Proof.
  intros valid left right owned allocated.
  destruct (allocation_returns_fresh_cell _ valid allocated) as (fresh & cells & _).
  split; cbn.
  - split.
    + rewrite cells. cbn. destruct (Nat.eq_dec slot slot); congruence.
    + split; eapply allocation_preserves_graph; eauto.
  - split.
    + constructor; [|exact owned]. intro inside. apply fresh.
      apply in_app_or in inside. destruct inside.
      * now apply (realizes_uses_only_occupied left).
      * now apply (realizes_uses_only_occupied right).
    + exact (nodup_append_disjoint owned).
Qed.

Inductive reservations : pool -> pool -> list (nat * cell) -> Prop :=
| ReservationsEmpty value : reservations value value []
| ReservationsStep initial before made contents slot after :
    reservations initial before made ->
    reserve before contents = (slot, after) ->
    reservations initial after ((slot, contents) :: made).

Theorem reservation_journal_retains_original_cells initial after made :
  pool_valid initial -> reservations initial after made ->
  pool_valid after /\ pool_cells after = made ++ pool_cells initial /\
  (forall slot contents, lookup slot (pool_cells initial) = Some contents ->
    lookup slot (pool_cells after) = Some contents).
Proof.
  intros valid trace. induction trace.
  - split; [exact valid|]. split; [reflexivity|auto].
  - destruct (IHtrace valid) as [before_valid [cells retained]].
    destruct (allocation_returns_fresh_cell contents before_valid H) as (_ & after_cells & after_valid).
    split; [exact after_valid|]. split.
    + rewrite after_cells, cells. reflexivity.
    + intros index old found.
      eapply allocation_preserves_every_original_cell.
      * exact before_valid.
      * exact H.
      * exact (retained index old found).
Qed.

Theorem reserved_slots_are_neither_live_nor_in_flight initial before made contents slot after :
  pool_valid initial -> reservations initial before made ->
  reserve before contents = (slot, after) ->
  ~ In slot (pool_ids initial) /\ ~ In slot (map fst made).
Proof.
  intros valid trace allocated.
  destruct (reservation_journal_retains_original_cells valid trace) as [before_valid [cells _]].
  destruct (allocation_returns_fresh_cell contents before_valid allocated) as [fresh _].
  unfold pool_ids in fresh. rewrite cells, map_app in fresh.
  split; intro inside; apply fresh; apply in_or_app; [now right|now left].
Qed.

Inductive local_edit : graph -> graph -> list nat -> list nat -> Prop :=
| EditKeep value : local_edit value value [] []
| EditPush value slot metadata :
    local_edit value (LinkGraph slot metadata value) [slot] []
| EditPushRegion value slot metadata :
    local_edit value (RegionGraph slot metadata EmptyGraph value) [slot] []
| EditPop slot metadata next :
    local_edit (LinkGraph slot metadata next) next [] [slot]
| EditReplace slot replacement metadata next new_metadata :
    local_edit (LinkGraph slot metadata next)
      (LinkGraph replacement new_metadata next) [replacement] [slot]
| EditRegion slot replacement metadata child next revised new_metadata made retired :
    local_edit child revised made retired ->
    local_edit (RegionGraph slot metadata child next)
      (RegionGraph replacement new_metadata revised next)
      (replacement :: made) (slot :: retired)
| EditRegionComplete slot metadata child next made retired :
    local_edit child EmptyGraph made retired ->
    local_edit (RegionGraph slot metadata child next) next made (slot :: retired)
| EditRegionDead slot metadata child next revised made retired :
    local_edit child revised made retired ->
    local_edit (RegionGraph slot metadata child next)
      (RegionGraph slot metadata child next) made made
| EditReturnedError original revised made retired :
    local_edit original revised made retired -> local_edit original original made made
| EditSequence original middle final made_left retired_left made_right retired_right :
    local_edit original middle made_left retired_left ->
    local_edit middle final made_right retired_right ->
    local_edit original final (made_left ++ made_right) (retired_left ++ retired_right).

Inductive concrete_edit (cells : list (nat * cell)) :
    graph -> graph -> list nat -> list nat -> Prop :=
| ConcreteKeep value : concrete_edit cells value value [] []
| ConcretePush value slot metadata :
    lookup slot cells = Some (Cell metadata (graph_root value) None false) ->
    concrete_edit cells value (LinkGraph slot metadata value) [slot] []
| ConcretePushRegion value slot metadata :
    lookup slot cells = Some (Cell metadata (graph_root value) None true) ->
    concrete_edit cells value (RegionGraph slot metadata EmptyGraph value) [slot] []
| ConcretePop slot metadata next :
    concrete_edit cells (LinkGraph slot metadata next) next [] [slot]
| ConcreteReplace slot replacement metadata next new_metadata :
    lookup replacement cells = Some (Cell new_metadata (graph_root next) None false) ->
    concrete_edit cells (LinkGraph slot metadata next)
      (LinkGraph replacement new_metadata next) [replacement] [slot]
| ConcreteRegion slot replacement metadata child next revised new_metadata made retired :
    concrete_edit cells child revised made retired ->
    lookup replacement cells = Some (Cell new_metadata (graph_root next) (graph_root revised) true) ->
    concrete_edit cells (RegionGraph slot metadata child next)
      (RegionGraph replacement new_metadata revised next)
      (replacement :: made) (slot :: retired)
| ConcreteRegionComplete slot metadata child next made retired :
    concrete_edit cells child EmptyGraph made retired ->
    concrete_edit cells (RegionGraph slot metadata child next) next made (slot :: retired)
| ConcreteRegionSaved slot metadata child next revised made retired :
    concrete_edit cells child revised made retired ->
    concrete_edit cells (RegionGraph slot metadata child next)
      (RegionGraph slot metadata child next) made made
| ConcreteReturnedError original revised made retired :
    concrete_edit cells original revised made retired ->
    concrete_edit cells original original made made
| ConcreteSequence original middle final made_left retired_left made_right retired_right :
    concrete_edit cells original middle made_left retired_left ->
    concrete_edit cells middle final made_right retired_right ->
    concrete_edit cells original final (made_left ++ made_right) (retired_left ++ retired_right).

Theorem concrete_edit_derives_both_realization_and_exact_local_journal
    cells original final made retired :
  concrete_edit cells original final made retired ->
  local_edit original final made retired /\
  (realizes cells original -> realizes cells final).
Proof.
  intro edited. induction edited.
  - split; [apply EditKeep|auto].
  - split; [apply EditPush|]. cbn. auto.
  - split; [apply EditPushRegion|]. cbn. auto.
  - split; [apply EditPop|]. cbn. tauto.
  - split; [apply EditReplace|]. cbn. tauto.
  - destruct IHedited as [journal represented]. split.
    + now apply EditRegion.
    + cbn. tauto.
  - destruct IHedited as [journal represented]. split.
    + now apply EditRegionComplete.
    + cbn. tauto.
  - destruct IHedited as [journal represented]. split.
    + now apply EditRegionDead with revised retired.
    + auto.
  - destruct IHedited as [journal represented]. split.
    + now apply EditReturnedError with revised retired.
    + auto.
  - destruct IHedited1 as [left_journal left_real], IHedited2 as [right_journal right_real].
    split; [eapply EditSequence; eauto|auto].
Qed.

Theorem local_owner_journals_have_exact_slot_conservation original final made retired :
  local_edit original final made retired ->
  Permutation (graph_slots original ++ made) (graph_slots final ++ retired).
Proof.
  intro edited. apply (proj2 (Permutation_count_occ Nat.eq_dec _ _)).
  intro index. induction edited; cbn;
    repeat rewrite count_occ_app in *; cbn in *;
    repeat match goal with
    | |- context [if Nat.eq_dec ?left ?right then _ else _] =>
        destruct (Nat.eq_dec left right)
    end; lia.
Qed.

Theorem local_commit_journal_is_exact_and_never_retires_a_live_frame original final made retired :
  local_edit original final made retired ->
  NoDup (graph_slots original ++ made) ->
  NoDup (graph_slots final) /\ NoDup retired /\
  (forall slot, In slot retired -> ~ In slot (graph_slots final)) /\
  (forall slot, In slot (graph_slots original ++ made) <->
    In slot (graph_slots final) \/ In slot retired).
Proof.
  intros edited owned.
  pose proof (local_owner_journals_have_exact_slot_conservation edited) as balance.
  pose proof (Permutation_NoDup balance owned) as result_owned.
  split; [exact (NoDup_app_remove_r _ _ result_owned)|].
  split; [exact (NoDup_app_remove_l _ _ result_owned)|]. split.
  - intros slot retired_here live_here.
    exact (nodup_append_disjoint result_owned slot live_here retired_here).
  - intro slot. rewrite <- in_app_iff.
    split; intro inside.
    + exact (Permutation_in slot balance inside).
    + exact (Permutation_in slot (Permutation_sym balance) inside).
Qed.

Theorem physical_reservation_freshness_discharges_edit_ownership initial after drafts original :
  pool_valid initial -> reservations initial after drafts ->
  realizes (pool_cells initial) original -> NoDup (graph_slots original) ->
  NoDup (graph_slots original ++ map fst drafts).
Proof.
  intros valid trace represented owned.
  destruct (reservation_journal_retains_original_cells valid trace) as [[distinct _] [cells _]].
  unfold pool_ids in distinct. rewrite cells, map_app in distinct.
  assert (all_owned : NoDup (map fst drafts ++ map fst (pool_cells initial))).
  { exact (NoDup_app_remove_r _ _ distinct). }
  apply NoDup_app; [exact owned|exact (NoDup_app_remove_r _ _ all_owned)|].
  intros slot in_original in_drafts.
  apply (nodup_append_disjoint all_owned slot in_drafts).
  exact (realizes_uses_only_occupied represented slot in_original).
Qed.

Arguments physical_reservation_freshness_discharges_edit_ownership
  {initial after drafts original} _ _ _ _.

Inductive release : pool -> nat -> pool -> Prop :=
| ReleaseSlot capacity prefix slot contents suffix free :
    release (Pool capacity (prefix ++ (slot, contents) :: suffix) free) slot
      (Pool capacity (prefix ++ suffix) (slot :: free)).

Lemma release_preserves_pool_validity before slot after :
  pool_valid before -> release before slot after -> pool_valid after.
Proof.
  intros valid released. destruct released.
  unfold pool_valid, pool_ids in *; cbn in *.
  repeat rewrite map_app in *; cbn in *.
  destruct valid as [distinct [bounded size]].
  assert (order : Permutation
    ((map fst prefix ++ slot :: map fst suffix) ++ free)
    ((map fst prefix ++ map fst suffix) ++ slot :: free)).
  { repeat rewrite <- app_assoc. apply Permutation_app_head. apply Permutation_middle. }
  split; [exact (Permutation_NoDup order distinct)|]. split.
  - intros index inside. apply bounded. eapply Permutation_in; [apply Permutation_sym; exact order|exact inside].
  - repeat rewrite length_app in *; cbn in *; lia.
Qed.

Lemma release_preserves_other_lookups before released after slot :
  release before released after -> slot <> released ->
  lookup slot (pool_cells after) = lookup slot (pool_cells before).
Proof.
  intros step different. destruct step as [capacity prefix removed contents suffix free]. cbn.
  induction prefix as [|[index contents'] rest IH]; cbn.
  - destruct (Nat.eq_dec slot removed); congruence.
  - destruct (Nat.eq_dec slot index); auto.
Qed.

Inductive releases : pool -> list nat -> pool -> Prop :=
| ReleasesEmpty value : releases value [] value
| ReleasesStep before slot middle rest after :
    release before slot middle -> releases middle rest after ->
    releases before (slot :: rest) after.

Lemma release_journal_preserves_other_lookups before retired after slot :
  releases before retired after -> ~ In slot retired ->
  lookup slot (pool_cells after) = lookup slot (pool_cells before).
Proof.
  intros steps. induction steps; intro absent; auto.
  rewrite IHsteps; [|intro inside; apply absent; now right].
  apply (release_preserves_other_lookups H). intro same. apply absent. now left.
Qed.

Arguments release_journal_preserves_other_lookups {before retired after slot} _ _.

Lemma release_journal_preserves_validity before retired after :
  pool_valid before -> releases before retired after -> pool_valid after.
Proof.
  intros valid steps. induction steps; auto.
  apply IHsteps. now apply (release_preserves_pool_validity valid H).
Qed.

Lemma release_journal_accounts_for_exact_cells before retired after :
  releases before retired after ->
  Permutation (pool_ids before) (pool_ids after ++ retired).
Proof.
  intro steps. apply (proj2 (Permutation_count_occ Nat.eq_dec _ _)).
  intro index. induction steps; [now rewrite app_nil_r|].
  destruct H. unfold pool_ids in *; cbn in *.
  repeat rewrite map_app in *; repeat rewrite count_occ_app in *; cbn in *.
  repeat match goal with
  | |- context [if ?decision then _ else _] => destruct decision
  end; congruence || lia.
Qed.

Theorem commit_preserves_the_new_concrete_graph_and_removes_exact_consumed_slots
    initial work committed original final drafts made retired :
  pool_valid initial -> reservations initial work drafts ->
  realizes (pool_cells initial) original -> NoDup (graph_slots original) ->
  Permutation (pool_ids initial) (graph_slots original) ->
  Permutation made (map fst drafts) ->
  concrete_edit (pool_cells work) original final made retired ->
  releases work retired committed ->
  pool_valid committed /\ realizes (pool_cells committed) final /\
  Permutation (pool_ids committed) (graph_slots final).
Proof.
  intros valid reserved old_real old_owned complete journal_order concrete freed.
  destruct (reservation_journal_retains_original_cells valid reserved) as [work_valid [cells retained]].
  destruct (concrete_edit_derives_both_realization_and_exact_local_journal concrete)
    as [edited materialized].
  assert (new_real : realizes (pool_cells work) final).
  { apply materialized. eapply realizes_transfers_preserved_cells; eauto. }
  pose proof (physical_reservation_freshness_discharges_edit_ownership
    valid reserved old_real old_owned) as all_owned.
  assert (edit_owned : NoDup (graph_slots original ++ made)).
  { eapply Permutation_NoDup; [apply Permutation_app_head; apply Permutation_sym; exact journal_order|exact all_owned]. }
  destruct (local_commit_journal_is_exact_and_never_retires_a_live_frame edited edit_owned)
    as (_ & _ & no_live & _).
  split; [exact (release_journal_preserves_validity work_valid freed)|]. split.
  - apply (realizes_transfers_on_its_exact_cells new_real). intros slot inside.
    apply (release_journal_preserves_other_lookups freed).
    intro retired_here. exact (no_live slot retired_here inside).
  - apply (proj2 (Permutation_count_occ Nat.eq_dec _ _)). intro index.
    pose proof (proj1 (Permutation_count_occ Nat.eq_dec _ _)
      (release_journal_accounts_for_exact_cells freed) index) as released_count.
    pose proof (proj1 (Permutation_count_occ Nat.eq_dec _ _)
      (local_owner_journals_have_exact_slot_conservation edited) index) as edit_count.
    pose proof (proj1 (Permutation_count_occ Nat.eq_dec _ _) complete index) as initial_count.
    pose proof (proj1 (Permutation_count_occ Nat.eq_dec _ _) journal_order index) as made_count.
    unfold pool_ids in released_count. rewrite cells, map_app in released_count.
    repeat rewrite count_occ_app in *. unfold pool_ids in *. lia.
Qed.

Inductive owner_status := OwnerOpen | OwnerDead (error : nat).

Record owner := Owner {
  owner_graph : graph;
  owner_state : owner_status;
  owner_first_source : option (nat * nat);
  owner_stable : bool;
  owner_obligation_clock : nat
}.

Definition kill (value : owner) (error : nat) :=
  Owner (owner_graph value) (OwnerDead error) (owner_first_source value)
    (owner_stable value) (owner_obligation_clock value).

Definition install_graph (value : owner) (root : graph) :=
  Owner root (owner_state value) (owner_first_source value)
    (owner_stable value) (owner_obligation_clock value).

Definition selects_saved_graph (saved candidate : owner) : bool :=
  match graph_root (owner_graph saved), graph_root (owner_graph candidate) with
  | None, None => true
  | Some saved_slot, Some candidate_slot => Nat.eqb saved_slot candidate_slot
  | _, _ => false
  end.

Theorem held_root_identity_selects_exact_graph_independently_of_dead_status
    cells saved candidate :
  realizes cells (owner_graph saved) -> realizes cells (owner_graph candidate) ->
  selects_saved_graph saved candidate = true <-> owner_graph saved = owner_graph candidate.
Proof.
  intros old_real new_real. unfold selects_saved_graph. split.
  - intro selected. apply (realized_graph_is_unique_at_its_root old_real new_real).
    destruct (graph_root (owner_graph saved)), (graph_root (owner_graph candidate));
      try discriminate; auto. apply Nat.eqb_eq in selected. now subst.
  - intro same. rewrite same. destruct (graph_root (owner_graph candidate)); auto.
    apply Nat.eqb_refl.
Qed.

Theorem fresh_rebuild_and_owned_pop_cannot_alias_the_held_parent_root
    slot replacement metadata next :
  ~ In replacement (graph_slots (LinkGraph slot metadata next)) ->
  NoDup (graph_slots (LinkGraph slot metadata next)) ->
  Some replacement <> Some slot /\ graph_root next <> Some slot.
Proof.
  cbn. intros fresh owned. split.
  - intro same. inversion same. apply fresh. now left.
  - intro same. inversion owned as [|? ? absent _]. apply absent.
    destruct next; cbn in *; try discriminate; inversion same; now left.
Qed.

Inductive local_result :=
| ReturnedError (error : nat)
| RegionChildDead (error : nat)
| ShallowDead (root : graph) (error : nat)
| Successful (root : graph).

Definition selected_owner (saved : owner) (result : local_result) :=
  match result with
  | ReturnedError _ => saved
  | RegionChildDead error => kill saved error
  | ShallowDead root error => kill (install_graph saved root) error
  | Successful root => install_graph saved root
  end.

Theorem returned_error_restores_exact_owner_and_original_obligation_cells
    initial work aborted drafts saved error :
  pool_valid initial -> reservations initial work drafts ->
  realizes (pool_cells initial) (owner_graph saved) ->
  releases work (map fst drafts) aborted ->
  selected_owner saved (ReturnedError error) = saved /\
  realizes (pool_cells aborted) (owner_graph saved) /\
  (forall slot contents, lookup slot (pool_cells initial) = Some contents ->
    lookup slot (pool_cells aborted) = Some contents).
Proof.
  intros valid reserved represented freed.
  destruct (reservation_journal_retains_original_cells valid reserved) as [[distinct _] [cells retained]].
  unfold pool_ids in distinct. rewrite cells, map_app in distinct.
  assert (separate : forall slot, In slot (map fst (pool_cells initial)) ->
      ~ In slot (map fst drafts)).
  { intros slot old new.
    exact (nodup_append_disjoint (NoDup_app_remove_r _ _ distinct) slot new old).
  }
  assert (restored : forall slot contents, lookup slot (pool_cells initial) = Some contents ->
    lookup slot (pool_cells aborted) = Some contents).
  { intros slot contents found.
    rewrite (release_journal_preserves_other_lookups freed).
    - now apply retained.
    - apply separate. now apply lookup_some_is_occupied in found.
  }
  split; [reflexivity|]. split; [|exact restored].
  eapply realizes_transfers_preserved_cells; eauto.
Qed.

Theorem deferred_dead_retains_the_selected_local_graph_and_metadata saved root error :
  owner_graph (selected_owner saved (RegionChildDead error)) = owner_graph saved /\
  owner_graph (selected_owner saved (ShallowDead root error)) = root /\
  owner_state (selected_owner saved (ShallowDead root error)) = OwnerDead error /\
  owner_first_source (kill saved error) = owner_first_source saved /\
  owner_stable (kill saved error) = owner_stable saved /\
  owner_obligation_clock (kill saved error) = owner_obligation_clock saved.
Proof. repeat split; reflexivity. Qed.

Theorem region_dead_discards_child_replacements_without_retiring_original_children
    slot metadata child next revised made retired :
  local_edit child revised made retired ->
  local_edit (RegionGraph slot metadata child next)
    (RegionGraph slot metadata child next) made made /\
  Permutation (graph_slots (RegionGraph slot metadata child next) ++ made)
    (graph_slots (RegionGraph slot metadata child next) ++ made).
Proof. intro edited. split; [now apply EditRegionDead with revised retired|reflexivity]. Qed.

Inductive pool_event : pool -> pool -> Prop :=
| AllocateEvent before contents slot after :
    reserve before contents = (slot, after) -> pool_event before after
| RetireEvent before slot after : release before slot after -> pool_event before after.

Lemma allocation_capacity_grows_only_at_full_occupancy before contents slot after :
  pool_valid before -> reserve before contents = (slot, after) ->
  pool_capacity after <= Nat.max (pool_capacity before) (length (pool_cells after)).
Proof.
  destruct before as [capacity cells free].
  unfold reserve, pool_valid; cbn. intros (_ & _ & size) allocated.
  destruct free; inversion allocated; subst slot after; cbn in *.
  - rewrite Nat.add_0_r in size. subst capacity. apply Nat.le_max_r.
  - apply Nat.le_max_l.
Qed.

Inductive bounded_pool_run (limit : nat) : pool -> pool -> nat -> Prop :=
| BoundedPoolEmpty value : bounded_pool_run limit value value 0
| BoundedPoolStep initial before after count :
    bounded_pool_run limit initial before count ->
    pool_valid before -> pool_event before after ->
    length (pool_cells after) <= limit ->
    bounded_pool_run limit initial after (S count).

Theorem physical_capacity_is_bounded_by_peak_live_plus_in_flight_not_transition_count
    limit initial after count :
  pool_capacity initial <= limit -> bounded_pool_run limit initial after count ->
  pool_capacity after <= limit.
Proof.
  intros initial_bound steps. induction steps; auto.
  specialize (IHsteps initial_bound).
  destruct H0.
  - pose proof (allocation_capacity_grows_only_at_full_occupancy _ H H0). lia.
  - destruct H0. cbn in *. exact IHsteps.
Qed.

Theorem reservation_occupancy_is_exact_live_originals_plus_in_flight_replacements
    initial after drafts :
  pool_valid initial -> reservations initial after drafts ->
  length (pool_cells after) = length (pool_cells initial) + length drafts.
Proof.
  intros valid trace.
  destruct (reservation_journal_retains_original_cells valid trace) as [_ [cells _]].
  rewrite cells, length_app. lia.
Qed.

Definition control_metadata := FrameMetadata [7; 3] (Some (ObligationMetadata 11 2 4 9 6 1)).
Definition control_cell := Cell control_metadata None None false.
Definition control_graph := LinkGraph 0 control_metadata EmptyGraph.
Definition control_pool := Pool 1 [(0, control_cell)] [].
Definition control_owner := Owner control_graph OwnerOpen (Some (9, 6)) false 12.
Definition control_reserved := Pool 2 [(1, control_cell); (0, control_cell)] [].
Definition control_committed := Pool 2 [(1, control_cell)] [0].
Definition control_reused := Pool 2 [(0, control_cell); (1, control_cell)] [].

Theorem reusable_slot_positive_transaction_controls :
  reserve control_pool control_cell = (1, control_reserved) /\
  release control_reserved 0 control_committed /\
  reserve control_committed control_cell = (0, control_reused) /\
  pool_capacity control_reused = 2 /\
  local_edit control_graph (LinkGraph 1 control_metadata EmptyGraph) [1] [0] /\
  local_edit (RegionGraph 2 control_metadata control_graph EmptyGraph)
    (RegionGraph 2 control_metadata control_graph EmptyGraph) [1] [1].
Proof.
  repeat split; try reflexivity.
  - exact (ReleaseSlot 2 [(1, control_cell)] 0 control_cell [] []).
  - apply EditReplace.
  - apply EditRegionDead with (revised := LinkGraph 1 control_metadata EmptyGraph) (retired := [0]).
    apply EditReplace.
Qed.

Definition wrong_eager_reuse := Pool 1 [(0, Cell (FrameMetadata [99] None) None None false)] [].
Definition wrong_clear_dead value error :=
  Owner EmptyGraph (OwnerDead error) (owner_first_source value)
    (owner_stable value) (owner_obligation_clock value).

Theorem reusable_slot_causal_ownership_and_dead_controls :
  lookup 0 (pool_cells wrong_eager_reuse) <> lookup 0 (pool_cells control_pool) /\
  ~ realizes (pool_cells wrong_eager_reuse) control_graph /\
  ~ NoDup (graph_slots (RegionGraph 2 control_metadata control_graph control_graph)) /\
  owner_graph (wrong_clear_dead control_owner 4) <>
    owner_graph (selected_owner control_owner (RegionChildDead 4)) /\
  owner_graph (selected_owner (install_graph control_owner EmptyGraph)
    (ShallowDead control_graph 4)) <>
    owner_graph (selected_owner (install_graph control_owner EmptyGraph) (RegionChildDead 4)) /\
  owner_graph (selected_owner control_owner (ReturnedError 4)) = control_graph /\
  lookup 0 (pool_cells control_committed) <> lookup 0 (pool_cells control_pool).
Proof.
  repeat split; try discriminate; try reflexivity.
  - cbn. intros [bad _]. discriminate.
  - cbn. intro duplicate. inversion duplicate as [|? ? _ tail].
    inversion tail as [|? ? absent _]. apply absent. now left.
Qed.

Definition control_multi_reserved :=
  Pool 3 [(2, control_cell); (1, control_cell); (0, control_cell)] [].
Definition control_multi_committed := Pool 3 [(2, control_cell)] [1; 0].
Definition control_region_cells :=
  [(2, Cell control_metadata (Some 3) (Some 0) true);
   (3, control_cell); (0, control_cell)].

Theorem ordered_multi_edit_region_completion_and_reuse_controls :
  reservations control_pool control_multi_reserved [(2, control_cell); (1, control_cell)] /\
  concrete_edit (pool_cells control_multi_reserved) control_graph
    (LinkGraph 2 control_metadata EmptyGraph) [1; 2] [0; 1] /\
  Permutation [1; 2] (map fst [(2, control_cell); (1, control_cell)]) /\
  releases control_multi_reserved [0; 1] control_multi_committed /\
  reserve control_multi_committed control_cell =
    (1, Pool 3 [(1, control_cell); (2, control_cell)] [0]) /\
  concrete_edit [(2, Cell control_metadata (Some 0) None true); (0, control_cell)]
    control_graph (RegionGraph 2 control_metadata EmptyGraph control_graph) [2] [] /\
  concrete_edit control_region_cells
    (RegionGraph 2 control_metadata control_graph (LinkGraph 3 control_metadata EmptyGraph))
    (LinkGraph 3 control_metadata EmptyGraph) [] [2; 0] /\
  ~ local_edit
    (RegionGraph 2 control_metadata control_graph (LinkGraph 3 control_metadata EmptyGraph))
    (LinkGraph 3 control_metadata EmptyGraph) [] [2] /\
  selects_saved_graph control_owner
    (kill (install_graph control_owner (LinkGraph 2 control_metadata EmptyGraph)) 4) = false.
Proof.
  repeat split; try reflexivity.
  - eapply ReservationsStep with (before := control_reserved).
    + eapply ReservationsStep with (before := control_pool).
      * apply ReservationsEmpty.
      * reflexivity.
    + reflexivity.
  - change (concrete_edit (pool_cells control_multi_reserved) control_graph
      (LinkGraph 2 control_metadata EmptyGraph) ([1] ++ [2]) ([0] ++ [1])).
    eapply ConcreteSequence with (middle := LinkGraph 1 control_metadata EmptyGraph).
    + apply ConcreteReplace. reflexivity.
    + apply ConcreteReplace. reflexivity.
  - apply perm_swap.
  - eapply ReleasesStep with (middle := Pool 3 [(2, control_cell); (1, control_cell)] [0]).
    + exact (ReleaseSlot 3 [(2, control_cell); (1, control_cell)] 0 control_cell [] []).
    + eapply ReleasesStep.
      * exact (ReleaseSlot 3 [(2, control_cell)] 1 control_cell [] [0]).
      * apply ReleasesEmpty.
  - apply ConcretePushRegion. reflexivity.
  - apply ConcreteRegionComplete. apply ConcretePop.
  - intro omitted.
    pose proof (Permutation_length (local_owner_journals_have_exact_slot_conservation omitted)) as count.
    discriminate count.
Qed.

Print Assumptions link_construction_refines_concrete_slot_contents.
Print Assumptions region_construction_preserves_disjoint_child_and_outer.
Print Assumptions reservation_journal_retains_original_cells.
Print Assumptions reserved_slots_are_neither_live_nor_in_flight.
Print Assumptions local_owner_journals_have_exact_slot_conservation.
Print Assumptions concrete_edit_derives_both_realization_and_exact_local_journal.
Print Assumptions local_commit_journal_is_exact_and_never_retires_a_live_frame.
Print Assumptions physical_reservation_freshness_discharges_edit_ownership.
Print Assumptions commit_preserves_the_new_concrete_graph_and_removes_exact_consumed_slots.
Print Assumptions returned_error_restores_exact_owner_and_original_obligation_cells.
Print Assumptions held_root_identity_selects_exact_graph_independently_of_dead_status.
Print Assumptions fresh_rebuild_and_owned_pop_cannot_alias_the_held_parent_root.
Print Assumptions deferred_dead_retains_the_selected_local_graph_and_metadata.
Print Assumptions region_dead_discards_child_replacements_without_retiring_original_children.
Print Assumptions physical_capacity_is_bounded_by_peak_live_plus_in_flight_not_transition_count.
Print Assumptions reservation_occupancy_is_exact_live_originals_plus_in_flight_replacements.
Print Assumptions reusable_slot_positive_transaction_controls.
Print Assumptions reusable_slot_causal_ownership_and_dead_controls.
Print Assumptions ordered_multi_edit_region_completion_and_reuse_controls.

End ReusableFrameOwnership.

Module IndexedWindowTransport.

Import CanonicalKeyProtocol.

Definition provenance := (nat * nat)%type.
Definition byte_provenance (value : located_byte) : provenance :=
  (byte_source value, byte_offset value).
Definition region_end (value : region) : provenance :=
  (region_eof_source value, region_eof_offset value).

Record window := Window {
  window_owner : nat;
  window_start : nat;
  window_width : nat;
  window_anchor : option provenance
}.

Definition window_valid (root : region) (view : window) : Prop :=
  window_owner view = region_owner root /\
  window_start view + window_width view <= length (region_bytes root).

Definition window_bytes (root : region) (view : window) : list located_byte :=
  firstn (window_width view) (skipn (window_start view) (region_bytes root)).

Fixpoint append_reference (input : list located_byte) (count : nat)
    (accumulated : list located_byte) : list located_byte :=
  match count, input with
  | S rest, byte :: tail => append_reference tail rest (accumulated ++ [byte])
  | _, _ => accumulated
  end.

Theorem accumulated_located_prefix_is_exact input count accumulated :
  append_reference input count accumulated = accumulated ++ firstn count input.
Proof.
  revert input accumulated. induction count; intros input accumulated;
    destruct input; cbn; try rewrite app_nil_r; try reflexivity.
  rewrite IHcount, <- app_assoc. reflexivity.
Qed.

Definition prefix_guard (limit : nat) (root : region) (consumed start stop : nat) : bool :=
  (start <=? stop) && (stop <=? consumed) &&
  (consumed <=? length (region_bytes root)) && (consumed <=? limit).

Definition checked_window (limit : nat) (root : region) (consumed start stop : nat)
    : option window :=
  if prefix_guard limit root consumed start stop then
    Some (Window (region_owner root) start (stop - start) None)
  else None.

Lemma prefix_guard_exact limit root consumed start stop :
  prefix_guard limit root consumed start stop = true <->
  start <= stop /\ stop <= consumed /\
  consumed <= length (region_bytes root) /\ consumed <= limit.
Proof.
  unfold prefix_guard. repeat rewrite andb_true_iff. repeat rewrite Nat.leb_le. tauto.
Qed.

Theorem checked_window_accepts_exact_consumed_and_integer_bounds
    limit root consumed start stop view :
  checked_window limit root consumed start stop = Some view <->
  start <= stop /\ stop <= consumed /\
  consumed <= length (region_bytes root) /\ consumed <= limit /\
  view = Window (region_owner root) start (stop - start) None.
Proof.
  unfold checked_window. destruct (prefix_guard limit root consumed start stop) eqn:guard.
  - apply prefix_guard_exact in guard. split.
    + intro accepted. inversion accepted; subst. tauto.
    + intros (_ & _ & _ & _ & ->). reflexivity.
  - split; [discriminate|]. intros (ordered & bounded & available & fits & _).
    assert (prefix_guard limit root consumed start stop = true) by
      (apply prefix_guard_exact; tauto). congruence.
Qed.

Lemma valid_window_length root view :
  window_valid root view -> length (window_bytes root view) = window_width view.
Proof.
  intros [_ bounded]. unfold window_bytes. apply firstn_length_le.
  rewrite length_skipn. lia.
Qed.

Theorem checked_window_has_exact_length_and_representable_absolute_end
    limit root consumed start stop view :
  checked_window limit root consumed start stop = Some view ->
  window_valid root view /\ length (window_bytes root view) = stop - start /\
  window_start view + window_width view = stop /\
  window_start view + window_width view <= limit.
Proof.
  intro accepted. apply checked_window_accepts_exact_consumed_and_integer_bounds in accepted.
  destruct accepted as (ordered & bounded & available & fits & ->).
  assert (valid : window_valid root (Window (region_owner root) start (stop - start) None)).
  { split; cbn; lia. }
  split; [exact valid|]. split.
  - exact (valid_window_length valid).
  - cbn. lia.
Qed.

Theorem indexed_slice_equals_independently_accumulated_prefix
    limit root consumed start stop view :
  checked_window limit root consumed start stop = Some view ->
  window_bytes root view =
    firstn (stop - start)
      (skipn start (append_reference (region_bytes root) consumed [])).
Proof.
  intro accepted. apply checked_window_accepts_exact_consumed_and_integer_bounds in accepted.
  destruct accepted as (ordered & bounded & available & fits & ->).
  unfold window_bytes. cbn. rewrite accumulated_located_prefix_is_exact. cbn.
  rewrite skipn_firstn_comm, firstn_firstn, Nat.min_l; [reflexivity|lia].
Qed.

Definition subwindow_guard (limit : nat) (parent : window) (start stop : nat) :=
  (start <=? stop) && (stop <=? window_width parent) &&
  (window_start parent + stop <=? limit).

Definition checked_subwindow (limit : nat) (parent : window) (start stop : nat)
    : option window :=
  if subwindow_guard limit parent start stop then
    Some (Window (window_owner parent) (window_start parent + start) (stop - start)
      (if stop =? window_width parent then window_anchor parent else None))
  else None.

Lemma subwindow_guard_exact limit parent start stop :
  subwindow_guard limit parent start stop = true <->
  start <= stop /\ stop <= window_width parent /\ window_start parent + stop <= limit.
Proof.
  unfold subwindow_guard. repeat rewrite andb_true_iff. repeat rewrite Nat.leb_le. tauto.
Qed.

Lemma checked_subwindow_exact limit parent start stop child :
  checked_subwindow limit parent start stop = Some child <->
  start <= stop /\ stop <= window_width parent /\ window_start parent + stop <= limit /\
  child = Window (window_owner parent) (window_start parent + start) (stop - start)
    (if stop =? window_width parent then window_anchor parent else None).
Proof.
  unfold checked_subwindow. destruct (subwindow_guard limit parent start stop) eqn:guard.
  - apply subwindow_guard_exact in guard. split.
    + intro accepted. inversion accepted; subst. tauto.
    + intros (_ & _ & _ & ->). reflexivity.
  - split; [discriminate|]. intros (ordered & bounded & fits & _).
    assert (subwindow_guard limit parent start stop = true) by
      (apply subwindow_guard_exact; tauto). congruence.
Qed.

Theorem nested_windows_preserve_owner_bounds_and_exact_slice
    limit root parent start stop child :
  window_valid root parent -> checked_subwindow limit parent start stop = Some child ->
  window_valid root child /\ window_owner child = window_owner parent /\
  window_start child + window_width child <= limit /\
  window_bytes root child = firstn (stop - start) (skipn start (window_bytes root parent)).
Proof.
  intros [owner bounded] accepted. apply checked_subwindow_exact in accepted.
  destruct accepted as (ordered & inside & fits & ->). cbn.
  split; [split; cbn; [exact owner|lia]|]. split; [reflexivity|]. split; [lia|].
  unfold window_bytes. cbn.
  rewrite skipn_firstn_comm, firstn_firstn, Nat.min_l by lia.
  rewrite skipn_skipn. replace (start + window_start parent) with (window_start parent + start) by lia.
  reflexivity.
Qed.

Theorem checked_empty_windows_do_not_read_a_following_byte
    limit root consumed point view :
  checked_window limit root consumed point point = Some view ->
  window_bytes root view = [] /\ window_width view = 0 /\
  window_start view = point /\ point <= consumed /\ consumed <= limit.
Proof.
  intro accepted. apply checked_window_accepts_exact_consumed_and_integer_bounds in accepted.
  destruct accepted as (_ & bounded & available & fits & ->).
  unfold window_bytes. cbn. rewrite Nat.sub_diag. cbn. tauto.
Qed.

Definition root_position (root : region) (offset : nat) : option provenance :=
  match nth_error (region_bytes root) offset with
  | Some byte => Some (byte_provenance byte)
  | None => if offset =? length (region_bytes root) then Some (region_end root) else None
  end.

Definition window_end (root : region) (view : window) : option provenance :=
  match window_anchor view with
  | Some anchor => Some anchor
  | None => root_position root (window_start view + window_width view)
  end.

Inductive position_kind := BytePosition | EndPosition.
Record position := Position {
  position_logical : nat;
  position_physical : provenance;
  position_kind_of : position_kind
}.

Definition window_position (root : region) (view : window) (offset : nat) : option position :=
  if offset <? window_width view then
    option_map (fun byte => Position offset (byte_provenance byte) BytePosition)
      (nth_error (region_bytes root) (window_start view + offset))
  else if offset =? window_width view then
    option_map (fun anchor => Position offset anchor EndPosition) (window_end root view)
  else None.

Definition anchor_window (view : window) (anchor : provenance) : window :=
  Window (window_owner view) (window_start view) (window_width view) (Some anchor).

Theorem explicit_completion_anchor_changes_only_the_end root view anchor :
  window_bytes root (anchor_window view anchor) = window_bytes root view /\
  (forall offset, offset < window_width view ->
    window_position root (anchor_window view anchor) offset = window_position root view offset) /\
  window_position root (anchor_window view anchor) (window_width view) =
    Some (Position (window_width view) anchor EndPosition) /\
  (forall offset, window_width view < offset ->
    window_position root (anchor_window view anchor) offset = None).
Proof.
  split; [reflexivity|]. split.
  - intros offset inside. unfold window_position, anchor_window; cbn -[Nat.ltb Nat.eqb].
    assert (offset <? window_width view = true) by (apply Nat.ltb_lt; exact inside).
    now rewrite H.
  - split.
    + unfold window_position, anchor_window; cbn -[Nat.ltb Nat.eqb].
      rewrite Nat.ltb_irrefl, Nat.eqb_refl. reflexivity.
    + intros offset outside. unfold window_position, anchor_window; cbn -[Nat.ltb Nat.eqb].
      assert (offset <? window_width view = false) by (apply Nat.ltb_ge; lia).
      assert (offset =? window_width view = false) by (apply Nat.eqb_neq; lia).
      now rewrite H, H0.
Qed.

Theorem earlier_and_equal_end_subviews_select_the_correct_boundary
    limit root parent start stop child :
  checked_subwindow limit parent start stop = Some child ->
  (stop < window_width parent ->
    window_anchor child = None /\
    window_end root child = root_position root (window_start parent + stop)) /\
  (stop = window_width parent ->
    window_anchor child = window_anchor parent /\ window_end root child = window_end root parent).
Proof.
  intro accepted. apply checked_subwindow_exact in accepted.
  destruct accepted as (ordered & bounded & fits & ->). cbn. split.
  - intro earlier. assert (stop =? window_width parent = false) by (apply Nat.eqb_neq; lia).
    rewrite H. split; [reflexivity|]. unfold window_end; cbn.
    f_equal. lia.
  - intro same. subst stop. rewrite Nat.eqb_refl. split; [reflexivity|].
    unfold window_end; cbn.
    replace (window_start parent + start + (window_width parent - start)) with
      (window_start parent + window_width parent) by lia. reflexivity.
Qed.

Lemma window_nth_is_source_nth root view offset :
  offset < window_width view ->
  nth_error (window_bytes root view) offset =
    nth_error (region_bytes root) (window_start view + offset).
Proof.
  intro inside. unfold window_bytes. rewrite nth_error_firstn.
  assert (offset <? window_width view = true) by (apply Nat.ltb_lt; exact inside).
  rewrite H. apply nth_error_skipn.
Qed.

Theorem indexed_byte_positions_preserve_exact_physical_provenance root view offset byte :
  offset < window_width view -> nth_error (window_bytes root view) offset = Some byte ->
  window_position root view offset = Some (Position offset (byte_provenance byte) BytePosition).
Proof.
  intros inside found. rewrite window_nth_is_source_nth in found by exact inside.
  unfold window_position.
  assert (offset <? window_width view = true) by (apply Nat.ltb_lt; exact inside).
  now rewrite H, found.
Qed.

Lemma root_position_at_end root :
  root_position root (length (region_bytes root)) = Some (region_end root).
Proof.
  unfold root_position.
  assert (nth_error (region_bytes root) (length (region_bytes root)) = None) by
    (apply nth_error_None; lia).
  now rewrite H, Nat.eqb_refl.
Qed.

Definition rebase_anchored_region (root : region) (view : window) (anchor : provenance) : region :=
  Region (window_owner view) (window_bytes root view) (fst anchor) (snd anchor).

Theorem rebased_frozen_and_inline_anchored_windows_have_identical_positions root view anchor :
  window_valid root view -> forall offset,
  window_position root (anchor_window view anchor) offset =
  window_position (rebase_anchored_region root view anchor)
    (Window (window_owner view) 0 (window_width view) None) offset.
Proof.
  intros valid offset. unfold window_position. cbn -[Nat.ltb Nat.eqb window_bytes window_end].
  destruct (offset <? window_width view) eqn:inside.
  - apply Nat.ltb_lt in inside.
    now rewrite window_nth_is_source_nth by exact inside.
  - destruct (offset =? window_width view) eqn:at_end; [|reflexivity].
    apply Nat.eqb_eq in at_end. subst offset.
    pose proof (root_position_at_end (rebase_anchored_region root view anchor)) as boundary.
    cbn -[root_position window_bytes] in boundary.
    rewrite (valid_window_length valid) in boundary.
    unfold rebase_anchored_region in boundary.
    unfold window_end, rebase_anchored_region; cbn -[root_position window_bytes].
    rewrite boundary. destruct anchor. reflexivity.
Qed.

Fixpoint byte_lexicographic (left right : list nat) : comparison :=
  match left, right with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | head_left :: tail_left, head_right :: tail_right =>
      match Nat.compare head_left head_right with
      | Eq => byte_lexicographic tail_left tail_right
      | order => order
      end
  end.

Fixpoint compare_located (left right : list located_byte) : comparison * option provenance :=
  match left, right with
  | [], [] => (Eq, None)
  | [], _ :: _ => (Lt, None)
  | _ :: _, [] => (Gt, None)
  | head_left :: tail_left, head_right :: tail_right =>
      match Nat.compare (byte_value head_left) (byte_value head_right) with
      | Eq => compare_located tail_left tail_right
      | order => (order, Some (byte_provenance head_right))
      end
  end.

Definition reported_comparison (left right : list located_byte) (endpoint : provenance) :=
  let '(order, mismatch) := compare_located left right in
  (order, match mismatch with Some source => source | None => endpoint end).

Theorem located_comparison_refines_content_only_lexicographic_order left right :
  fst (compare_located left right) =
    byte_lexicographic (map byte_value left) (map byte_value right).
Proof.
  revert right. induction left; intros right; destruct right; cbn; auto.
  destruct (Nat.compare (byte_value a) (byte_value l)); cbn; auto.
Qed.

Lemma equal_content_prefixes_cancel prefix_left prefix_right tail_left tail_right :
  map byte_value prefix_left = map byte_value prefix_right ->
  compare_located (prefix_left ++ tail_left) (prefix_right ++ tail_right) =
    compare_located tail_left tail_right.
Proof.
  revert prefix_right. induction prefix_left; intros prefix_right same;
    destruct prefix_right; cbn in *; try discriminate; auto.
  inversion same as [[head_equal tail_equal]].
  rewrite head_equal, Nat.compare_refl. now apply IHprefix_left.
Qed.

Theorem first_content_mismatch_reports_the_current_physical_byte
    prefix_left prefix_right tail_left tail_right byte_left byte_right endpoint :
  map byte_value prefix_left = map byte_value prefix_right ->
  byte_value byte_left <> byte_value byte_right ->
  reported_comparison (prefix_left ++ byte_left :: tail_left)
    (prefix_right ++ byte_right :: tail_right) endpoint =
    (Nat.compare (byte_value byte_left) (byte_value byte_right), byte_provenance byte_right).
Proof.
  intros equal_prefix different. unfold reported_comparison.
  rewrite equal_content_prefixes_cancel by exact equal_prefix. cbn.
  destruct (Nat.compare (byte_value byte_left) (byte_value byte_right)) eqn:order; auto.
  apply Nat.compare_eq_iff in order. contradiction.
Qed.

Theorem equal_and_proper_prefix_comparisons_preserve_endpoint_fallback
    prefix_left prefix_right extra endpoint :
  map byte_value prefix_left = map byte_value prefix_right -> extra <> [] ->
  reported_comparison prefix_left prefix_right endpoint = (Eq, endpoint) /\
  reported_comparison prefix_left (prefix_right ++ extra) endpoint = (Lt, endpoint) /\
  reported_comparison (prefix_left ++ extra) prefix_right endpoint = (Gt, endpoint).
Proof.
  intros same nonempty. unfold reported_comparison.
  pose proof (equal_content_prefixes_cancel prefix_left prefix_right [] [] same) as equal_case.
  rewrite !app_nil_r in equal_case. rewrite equal_case. split; [reflexivity|].
  pose proof (equal_content_prefixes_cancel prefix_left prefix_right [] extra same) as shorter.
  pose proof (equal_content_prefixes_cancel prefix_left prefix_right extra [] same) as longer.
  rewrite app_nil_r in shorter, longer. rewrite shorter, longer.
  destruct extra; [contradiction|]. split; reflexivity.
Qed.

Definition completion_anchor (root : region) (consumed : nat) : option provenance :=
  match consumed with
  | 0 => None
  | S previous => option_map byte_provenance (nth_error (region_bytes root) previous)
  end.

Theorem completion_anchor_comes_from_the_final_consumed_byte root consumed final :
  0 < consumed ->
  nth_error (append_reference (region_bytes root) consumed []) (consumed - 1) = Some final ->
  completion_anchor root consumed = Some (byte_provenance final).
Proof.
  intros nonempty found. rewrite accumulated_located_prefix_is_exact in found. cbn in found.
  rewrite nth_error_firstn in found.
  assert (consumed - 1 <? consumed = true) by (apply Nat.ltb_lt; lia).
  rewrite H in found. destruct consumed; [lia|]. cbn in *.
  rewrite Nat.sub_0_r in found. now rewrite found.
Qed.

Lemma forall_window_bytes (P : located_byte -> Prop) input start width :
  Forall P input -> Forall P (firstn width (skipn start input)).
Proof.
  intro all_bytes.
  assert (tail_bytes : Forall P (skipn start input)).
  { revert input all_bytes. induction start; intros input all_bytes; cbn; auto.
    destruct input; cbn; [constructor|]. inversion all_bytes; subst. now apply IHstart. }
  remember (skipn start input) as remaining eqn:remaining_eq.
  clear remaining_eq input start all_bytes.
  revert remaining tail_bytes. induction width; intros remaining tail_bytes; cbn; [constructor|].
  destruct remaining; cbn; [constructor|]. inversion tail_bytes; subst. constructor; auto.
Qed.

Theorem indexed_and_rebased_windows_preserve_source_refinement sources root view anchor :
  source_refines sources root ->
  Forall (fun byte => byte_value byte < 256 /\
    nth_error (sources (byte_source byte)) (byte_offset byte) = Some (byte_value byte))
    (window_bytes root view) /\
  source_refines sources (rebase_anchored_region root view anchor).
Proof.
  intro source_valid. unfold source_refines, rebase_anchored_region; cbn.
  unfold window_bytes. split; apply forall_window_bytes; exact source_valid.
Qed.

Lemma borrowed_physical_offsets_require_a_separate_checked_origin
    origin limit root view offset :
  window_valid root view -> origin + length (region_bytes root) <= limit ->
  offset <= window_width view -> origin + window_start view + offset <= limit.
Proof. intros [_ bounded] origin_fits inside. lia. Qed.

Record shared_backing := SharedBacking {
  backing_leases : nat;
  backing_region : region
}.

Definition backing_heap := nat -> option shared_backing.

Definition update_backing (heap : backing_heap) (owner : nat) (entry : option shared_backing)
    : backing_heap :=
  fun requested => if Nat.eq_dec requested owner then entry else heap requested.

Lemma updated_backing_same heap owner entry : update_backing heap owner entry owner = entry.
Proof. unfold update_backing. destruct (Nat.eq_dec owner owner); congruence. Qed.

Lemma updated_backing_other heap owner entry requested :
  requested <> owner -> update_backing heap owner entry requested = heap requested.
Proof. intro different. unfold update_backing. destruct (Nat.eq_dec requested owner); congruence. Qed.

Definition retain_backing (heap : backing_heap) (owner : nat) : option backing_heap :=
  match heap owner with
  | Some (SharedBacking (S count) root) =>
      Some (update_backing heap owner (Some (SharedBacking (S (S count)) root)))
  | _ => None
  end.

Definition release_backing (heap : backing_heap) (owner : nat) : backing_heap :=
  match heap owner with
  | Some (SharedBacking (S (S count)) root) =>
      update_backing heap owner (Some (SharedBacking (S count) root))
  | Some _ => update_backing heap owner None
  | None => heap
  end.

Definition observe_window (heap : backing_heap) (view : window)
    : option (list located_byte * option provenance) :=
  match heap (window_owner view) with
  | Some (SharedBacking (S _) root) => Some (window_bytes root view, window_end root view)
  | _ => None
  end.

Theorem suspended_window_retention_survives_parent_release heap view count root :
  heap (window_owner view) = Some (SharedBacking (S count) root) ->
  exists retained,
    retain_backing heap (window_owner view) = Some retained /\
    release_backing retained (window_owner view) (window_owner view) =
      Some (SharedBacking (S count) root) /\
    observe_window (release_backing retained (window_owner view)) view = observe_window heap view /\
    (forall other, other <> window_owner view ->
      release_backing retained (window_owner view) other = heap other).
Proof.
  intro present.
  exists (update_backing heap (window_owner view) (Some (SharedBacking (S (S count)) root))).
  split.
  - unfold retain_backing. now rewrite present.
  - unfold release_backing. rewrite updated_backing_same. split.
    + apply updated_backing_same.
    + split.
      * unfold observe_window. now rewrite updated_backing_same, present.
      * intros other different. now rewrite !updated_backing_other by exact different.
Qed.

Definition frame_windows := nat -> option window.
Definition recycle_frame_window (cells : frame_windows) (slot : nat) (replacement : window)
    : frame_windows :=
  fun requested => if Nat.eq_dec requested slot then Some replacement else cells requested.

Definition observe_frame_window (heap : backing_heap) (cells : frame_windows) (slot : nat) :=
  match cells slot with Some view => observe_window heap view | None => None end.

Theorem captured_descriptor_survives_recycling_its_frame_cell
    heap cells slot captured replacement count root :
  cells slot = Some captured ->
  heap (window_owner captured) = Some (SharedBacking (S count) root) ->
  exists retained,
    retain_backing heap (window_owner captured) = Some retained /\
    recycle_frame_window cells slot replacement slot = Some replacement /\
    observe_window (release_backing retained (window_owner captured)) captured =
      observe_frame_window heap cells slot.
Proof.
  intros captured_here present.
  destruct (@suspended_window_retention_survives_parent_release heap captured count root present)
    as (retained & acquired & alive & observed & others).
  exists retained. split; [exact acquired|]. split.
  - unfold recycle_frame_window. destruct (Nat.eq_dec slot slot); congruence.
  - unfold observe_frame_window. now rewrite captured_here, observed.
Qed.

Record span_request := SpanRequest {
  span_consumed : nat;
  span_start : nat;
  span_stop : nat;
  span_completion : provenance
}.

Definition indexed_transport (limit : nat) (root : region) (request : span_request) : option region :=
  option_map (fun view => rebase_anchored_region root view (span_completion request))
    (checked_window limit root (span_consumed request) (span_start request) (span_stop request)).

Definition reference_transport (limit : nat) (root : region) (request : span_request) : option region :=
  if (span_consumed request <=? length (region_bytes root)) && (span_consumed request <=? limit) then
    let prefix := append_reference (region_bytes root) (span_consumed request) [] in
    if (span_start request <=? span_stop request) && (span_stop request <=? length prefix) then
      Some (Region (region_owner root)
        (firstn (span_stop request - span_start request) (skipn (span_start request) prefix))
        (fst (span_completion request)) (snd (span_completion request)))
    else None
  else None.

Theorem indexed_and_append_reference_transport_observations_are_identical limit root request :
  indexed_transport limit root request = reference_transport limit root request.
Proof.
  destruct request as [consumed start stop anchor].
  unfold indexed_transport, reference_transport; cbn.
  destruct (consumed <=? length (region_bytes root)) eqn:available;
    destruct (consumed <=? limit) eqn:fits; cbn.
  - assert (prefix_length : length (append_reference (region_bytes root) consumed []) = consumed).
    { rewrite accumulated_located_prefix_is_exact. cbn.
      apply firstn_length_le. now apply Nat.leb_le in available. }
    rewrite prefix_length.
    unfold checked_window, prefix_guard. rewrite available, fits, !andb_true_r.
    destruct ((start <=? stop) && (stop <=? consumed)) eqn:bounded; cbn; auto.
    assert (accepted : checked_window limit root consumed start stop =
        Some (Window (region_owner root) start (stop - start) None)).
    { unfold checked_window, prefix_guard. now rewrite available, fits, !andb_true_r, bounded. }
    unfold rebase_anchored_region. f_equal.
    now rewrite (@indexed_slice_equals_independently_accumulated_prefix limit root consumed
      start stop (Window (region_owner root) start (stop - start) None) accepted).
  - unfold checked_window, prefix_guard. rewrite available, fits, andb_false_r. reflexivity.
  - unfold checked_window, prefix_guard. rewrite available, fits, andb_false_r. reflexivity.
  - unfold checked_window, prefix_guard. rewrite available, fits, !andb_false_r. reflexivity.
Qed.

Section Protocol.

Context {State World Input Observation Error : Type}.

Variable root : region.
Variable limit : nat.
Variable pending_refusal : State -> World -> Input -> option Error.
Variable request_for : State -> World -> Input -> span_request.
Variable shared_grammar : State -> World -> Input -> option region ->
  ((State * World * Observation) + (World * Error))%type.

Definition guarded_transport_step (transport : nat -> region -> span_request -> option region)
    (state : State) (world : World) (input : Input) :=
  match pending_refusal state world input with
  | Some error => inr (world, error)
  | None => shared_grammar state world input (transport limit root (request_for state world input))
  end.

Lemma guarded_transport_steps_are_identical state world input :
  guarded_transport_step indexed_transport state world input =
    guarded_transport_step reference_transport state world input.
Proof.
  unfold guarded_transport_step. destruct (pending_refusal state world input); auto.
  now rewrite indexed_and_append_reference_transport_observations_are_identical.
Qed.

Theorem pending_preflight_refuses_before_state_world_or_consumed_offset_advance
    state world input error inputs :
  pending_refusal state world input = Some error ->
  guarded_transport_step indexed_transport state world input = inr (world, error) /\
  @ExclusiveCurrentState.exclusive_run State World Input Observation Error
    (guarded_transport_step indexed_transport) (input :: inputs)
    (ExclusiveCurrentState.Current state) world =
  @ExclusiveCurrentState.Result World Input Observation Error
    (@ExclusiveCurrentState.exclusive State) (ExclusiveCurrentState.Current state)
    world [] [input] (Some error).
Proof.
  intro pending. split.
  - unfold guarded_transport_step. now rewrite pending.
  - cbn. unfold guarded_transport_step. now rewrite pending.
Qed.

Theorem shared_grammar_and_span_correspondence_preserve_full_protocol_observations
    (arena_step indexed_step : State -> World -> Input ->
      ((State * World * Observation) + (World * Error))%type) :
  (forall state world input, arena_step state world input =
    guarded_transport_step reference_transport state world input) ->
  (forall state world input, indexed_step state world input =
    guarded_transport_step indexed_transport state world input) ->
  forall inputs state world,
  @ExclusiveCurrentState.exclusive_run State World Input Observation Error
    arena_step inputs (ExclusiveCurrentState.Current state) world =
  @ExclusiveCurrentState.exclusive_run State World Input Observation Error
    indexed_step inputs (ExclusiveCurrentState.Current state) world.
Proof.
  intros arena_correspondence indexed_correspondence inputs.
  induction inputs as [|input rest IH]; intros state world; cbn; auto.
  rewrite arena_correspondence, indexed_correspondence, guarded_transport_steps_are_identical.
  destruct (guarded_transport_step reference_transport state world input)
    as [[[next next_world] observation]|[next_world error]]; cbn; auto.
  now rewrite IH.
Qed.

End Protocol.

Definition control_root := Region 7
  [LocatedByte 10 3 2; LocatedByte 20 9 1; LocatedByte 30 4 8; LocatedByte 40 6 3] 88 501.
Definition control_view := Window 7 1 1 None.
Definition control_anchored := anchor_window control_view (9, 1).
Definition control_heap : backing_heap := fun owner =>
  if Nat.eq_dec owner 7 then Some (SharedBacking 1 control_root) else None.
Definition control_cells : frame_windows := fun slot =>
  if Nat.eq_dec slot 2 then Some control_anchored else None.

Definition wrong_full_input_window limit root (consumed : nat) start stop :=
  checked_window limit root (length (region_bytes root)) start stop.

Theorem concrete_window_anchor_lifetime_and_ordering_positive_controls :
  checked_window 16 control_root 2 1 2 = Some control_view /\
  checked_window 16 control_root 2 2 2 = Some (Window 7 2 0 None) /\
  completion_anchor control_root 2 = Some (9, 1) /\
  window_bytes control_root control_anchored = [LocatedByte 20 9 1] /\
  window_position control_root control_anchored 0 = Some (Position 0 (9, 1) BytePosition) /\
  window_position control_root control_anchored 1 = Some (Position 1 (9, 1) EndPosition) /\
  checked_subwindow 16 (Window 7 0 3 (Some (99, 700))) 1 2 = Some (Window 7 1 1 None) /\
  checked_subwindow 16 (Window 7 0 3 (Some (99, 700))) 1 3 = Some (Window 7 1 2 (Some (99, 700))) /\
  reported_comparison [LocatedByte 10 99 100; LocatedByte 40 99 101]
    [LocatedByte 10 3 2; LocatedByte 20 9 1] (77, 900) = (Gt, (9, 1)) /\
  reported_comparison [LocatedByte 10 99 100; LocatedByte 20 99 101]
    [LocatedByte 10 3 2] (77, 900) = (Gt, (77, 900)) /\
  match retain_backing control_heap 7 with
  | Some retained => observe_window (release_backing retained 7) control_anchored
  | None => None
  end = Some ([LocatedByte 20 9 1], Some (9, 1)).
Proof. repeat split; reflexivity. Qed.

Theorem concrete_transport_causal_controls_reject_future_bytes_wrong_anchors_and_borrowed_frames :
  checked_window 16 control_root 2 1 3 = None /\
  wrong_full_input_window 16 control_root 2 1 3 = Some (Window 7 1 2 None) /\
  checked_window 1 control_root 2 1 2 = None /\
  window_end control_root control_view <> window_end control_root control_anchored /\
  window_end control_root (Window 7 2 0 None) <>
    window_end control_root (anchor_window (Window 7 2 0 None) (9, 1)) /\
  observe_window (release_backing control_heap 7) control_anchored = None /\
  observe_window control_heap control_anchored <>
    observe_frame_window control_heap
      (recycle_frame_window control_cells 2 (Window 7 2 1 None)) 2.
Proof. repeat split; discriminate || reflexivity. Qed.

Print Assumptions accumulated_located_prefix_is_exact.
Print Assumptions checked_window_accepts_exact_consumed_and_integer_bounds.
Print Assumptions checked_window_has_exact_length_and_representable_absolute_end.
Print Assumptions indexed_slice_equals_independently_accumulated_prefix.
Print Assumptions nested_windows_preserve_owner_bounds_and_exact_slice.
Print Assumptions checked_empty_windows_do_not_read_a_following_byte.
Print Assumptions explicit_completion_anchor_changes_only_the_end.
Print Assumptions earlier_and_equal_end_subviews_select_the_correct_boundary.
Print Assumptions indexed_byte_positions_preserve_exact_physical_provenance.
Print Assumptions rebased_frozen_and_inline_anchored_windows_have_identical_positions.
Print Assumptions located_comparison_refines_content_only_lexicographic_order.
Print Assumptions first_content_mismatch_reports_the_current_physical_byte.
Print Assumptions equal_and_proper_prefix_comparisons_preserve_endpoint_fallback.
Print Assumptions completion_anchor_comes_from_the_final_consumed_byte.
Print Assumptions indexed_and_rebased_windows_preserve_source_refinement.
Print Assumptions suspended_window_retention_survives_parent_release.
Print Assumptions captured_descriptor_survives_recycling_its_frame_cell.
Print Assumptions indexed_and_append_reference_transport_observations_are_identical.
Print Assumptions pending_preflight_refuses_before_state_world_or_consumed_offset_advance.
Print Assumptions shared_grammar_and_span_correspondence_preserve_full_protocol_observations.
Print Assumptions concrete_window_anchor_lifetime_and_ordering_positive_controls.
Print Assumptions concrete_transport_causal_controls_reject_future_bytes_wrong_anchors_and_borrowed_frames.

End IndexedWindowTransport.

Module StrictOpaqueInterior.

Import CanonicalKeyProtocol.
Import IndexedWindowTransport.

Inductive opaque_kind := RawParPayload | NestedEpmPayload.
Inductive path_mode :=
| LiveOpaque
| PendingChild
| DeadPath (code : nat) (source : provenance)
| NonOpaque (tag : nat).

Record metadata := Metadata {
  metadata_id : nat;
  metadata_kind : opaque_kind;
  metadata_tag_source : provenance;
  metadata_payload_start : nat;
  metadata_anchor : option provenance;
  metadata_stable : bool
}.

Record owner := Owner {
  owner_region_remaining : nat;
  owner_path_remaining : nat;
  owner_saved_state : list nat;
  owner_previous : list located_byte;
  owner_next : option nat
}.

Record state := State {
  state_mode : path_mode;
  state_consumed : nat;
  state_payload_remaining : nat;
  state_owners : list owner;
  state_metadata : metadata
}.

Fixpoint owner_bounds (owners : list owner) : list nat :=
  match owners with
  | [] => []
  | current :: rest => owner_region_remaining current :: owner_path_remaining current :: owner_bounds rest
  end.

Definition bounds (current : state) := state_payload_remaining current :: owner_bounds (state_owners current).
Definition strict_interior (width : nat) (current : state) :=
  Forall (fun remaining => width < remaining) (bounds current).

Fixpoint bounded_minimum (remaining : list nat) (available : nat) :=
  match remaining with
  | [] => available
  | first :: rest => Nat.min first (bounded_minimum rest available)
  end.

Lemma below_minimum width remaining available :
  width < bounded_minimum remaining available <->
  width < available /\ Forall (fun value => width < value) remaining.
Proof.
  induction remaining as [|first rest IH]; cbn.
  - split; [intro H; split; [exact H|constructor]|tauto].
  - assert (split_min : width < Nat.min first (bounded_minimum rest available) <->
      width < first /\ width < bounded_minimum rest available).
    { destruct (le_dec first (bounded_minimum rest available)).
      - rewrite Nat.min_l by lia. lia.
      - rewrite Nat.min_r by lia. lia. }
    rewrite split_min, IH. split.
    + intros (head & available_ok & tail). split; [exact available_ok|constructor; assumption].
    + intros (available_ok & all). inversion all; subst. tauto.
Qed.

Lemma minimum_at_most_available remaining available : bounded_minimum remaining available <= available.
Proof.
  induction remaining as [|first rest IH]; cbn; [lia|].
  destruct (le_dec first (bounded_minimum rest available)).
  - rewrite Nat.min_l by lia. lia.
  - rewrite Nat.min_r by lia. lia.
Qed.

Definition available_prefix (limit input_length : nat) (current : state) :=
  Nat.min (input_length - state_consumed current) (limit - state_consumed current).

Definition interior_width (limit input_length : nat) (current : state) :=
  Nat.pred (bounded_minimum (bounds current) (available_prefix limit input_length current)).

Definition eligible (current : state) :=
  match state_mode current with LiveOpaque => true | _ => false end.

Record plan := Plan {
  plan_from : nat;
  plan_id : nat;
  plan_width : nat
}.

Definition plan_interior (limit input_length : nat) (current : state) : option plan :=
  if eligible current && (state_consumed current <=? input_length) && (state_consumed current <=? limit) then
    let width := interior_width limit input_length current in
    if 0 <? width then Some (Plan (state_consumed current) (metadata_id (state_metadata current)) width)
    else None
  else None.

Definition decrement_owner (width : nat) (current : owner) :=
  Owner (owner_region_remaining current - width) (owner_path_remaining current - width)
    (owner_saved_state current) (owner_previous current) (owner_next current).

Definition bulk_decrement (width : nat) (current : state) :=
  State (state_mode current) (state_consumed current + width)
    (state_payload_remaining current - width)
    (map (decrement_owner width) (state_owners current)) (state_metadata current).

Definition plan_guard (limit input_length : nat) (current : state) (request : plan) :=
  eligible current && (state_consumed current <=? input_length) && (state_consumed current <=? limit) &&
  (plan_from request =? state_consumed current) &&
  (plan_id request =? metadata_id (state_metadata current)) &&
  (0 <? plan_width request) && (plan_width request <=? interior_width limit input_length current).

Definition try_advance (limit input_length : nat) (current : state) (request : plan) : option state :=
  if plan_guard limit input_length current request then Some (bulk_decrement (plan_width request) current)
  else None.

Theorem planner_returns_the_exact_positive_strict_minimum limit input_length current request :
  plan_interior limit input_length current = Some request ->
  state_mode current = LiveOpaque /\ state_consumed current <= input_length /\
  state_consumed current <= limit /\
  request = Plan (state_consumed current) (metadata_id (state_metadata current))
    (interior_width limit input_length current) /\
  0 < plan_width request /\ strict_interior (plan_width request) current /\
  state_consumed current + plan_width request < input_length /\
  state_consumed current + plan_width request < limit.
Proof.
  unfold plan_interior. destruct (eligible current && (state_consumed current <=? input_length) &&
    (state_consumed current <=? limit)) eqn:preflight; [|discriminate].
  destruct (0 <? interior_width limit input_length current) eqn:nonzero; [|discriminate].
  intro accepted. inversion accepted; subst request; cbn.
  repeat rewrite andb_true_iff in preflight. destruct preflight as [[live available] fits].
  apply Nat.leb_le in available. apply Nat.leb_le in fits. apply Nat.ltb_lt in nonzero.
  assert (mode : state_mode current = LiveOpaque).
  { unfold eligible in live. destruct (state_mode current); congruence. }
  assert (strict : interior_width limit input_length current <
      bounded_minimum (bounds current) (available_prefix limit input_length current)).
  { unfold interior_width in *. lia. }
  apply below_minimum in strict. destruct strict as [within all].
  unfold available_prefix in within.
  assert (input_bound : interior_width limit input_length current < input_length - state_consumed current).
  { destruct (le_dec (input_length - state_consumed current) (limit - state_consumed current));
      [rewrite Nat.min_l in within by lia|rewrite Nat.min_r in within by lia]; lia. }
  assert (limit_bound : interior_width limit input_length current < limit - state_consumed current).
  { destruct (le_dec (input_length - state_consumed current) (limit - state_consumed current));
      [rewrite Nat.min_l in within by lia|rewrite Nat.min_r in within by lia]; lia. }
  unfold strict_interior. repeat split; auto; lia.
Qed.

Lemma guarded_request_has_strict_bounds limit input_length current request :
  plan_guard limit input_length current request = true ->
  state_mode current = LiveOpaque /\ 0 < plan_width request /\
  strict_interior (plan_width request) current /\
  state_consumed current + plan_width request < input_length /\
  state_consumed current + plan_width request < limit.
Proof.
  unfold plan_guard. repeat rewrite andb_true_iff. rewrite Nat.ltb_lt, !Nat.leb_le.
  intros ((((((live & available) & fits) & from) & id) & positive) & bounded).
  assert (mode : state_mode current = LiveOpaque).
  { unfold eligible in live. destruct (state_mode current); congruence. }
  assert (strict : plan_width request < bounded_minimum (bounds current)
    (available_prefix limit input_length current)).
  { unfold interior_width in bounded. lia. }
  apply below_minimum in strict. destruct strict as [within all].
  unfold available_prefix in within.
  destruct (le_dec (input_length - state_consumed current) (limit - state_consumed current));
    [rewrite Nat.min_l in within by lia|rewrite Nat.min_r in within by lia];
    unfold strict_interior; repeat split; auto; lia.
Qed.

Theorem a_fresh_plan_is_accepted_without_rechecking_payload_bytes limit input_length current request :
  plan_interior limit input_length current = Some request ->
  try_advance limit input_length current request = Some (bulk_decrement (plan_width request) current).
Proof.
  intro accepted.
  pose proof (@planner_returns_the_exact_positive_strict_minimum limit input_length current request accepted)
    as (mode & available & fits & shape & positive & rest).
  subst request. unfold try_advance, plan_guard, eligible. rewrite mode. cbn -[Nat.ltb Nat.leb Nat.eqb].
  assert (state_consumed current <=? input_length = true) by (apply Nat.leb_le; exact available).
  assert (state_consumed current <=? limit = true) by (apply Nat.leb_le; exact fits).
  assert (0 <? interior_width limit input_length current = true) by (apply Nat.ltb_lt; exact positive).
  now rewrite H, H0, H1, !Nat.eqb_refl, Nat.leb_refl.
Qed.

Lemma owner_bounds_after_decrement width owners :
  owner_bounds (map (decrement_owner width) owners) = map (fun value => value - width) (owner_bounds owners).
Proof. induction owners; cbn; [reflexivity|]. now rewrite IHowners. Qed.

Lemma bounds_after_decrement width current :
  bounds (bulk_decrement width current) = map (fun value => value - width) (bounds current).
Proof. unfold bounds, bulk_decrement; cbn. now rewrite owner_bounds_after_decrement. Qed.

Lemma decrement_zero current : bulk_decrement 0 current = current.
Proof.
  destruct current as [mode consumed payload owners info]. unfold bulk_decrement; cbn.
  rewrite ?Nat.add_0_r, ?Nat.sub_0_r. f_equal.
  induction owners as [|[region path saved previous next] rest IH]; cbn; auto.
  rewrite IH. f_equal. unfold decrement_owner; cbn. now rewrite !Nat.sub_0_r.
Qed.

Lemma decrement_composes first second current :
  bulk_decrement second (bulk_decrement first current) = bulk_decrement (first + second) current.
Proof.
  destruct current as [mode consumed payload owners info]. unfold bulk_decrement; cbn.
  f_equal; try lia. rewrite map_map. apply map_ext.
  intros [region path saved previous next]. unfold decrement_owner; cbn. f_equal; lia.
Qed.

Lemma strict_after_decrement first rest current :
  strict_interior (first + rest) current -> strict_interior rest (bulk_decrement first current).
Proof.
  unfold strict_interior. rewrite bounds_after_decrement.
  intro bounded. apply Forall_map. eapply Forall_impl; [|exact bounded]. cbn. intros; lia.
Qed.

Lemma strict_smaller small big current :
  small <= big -> strict_interior big current -> strict_interior small current.
Proof.
  unfold strict_interior. intros size bounded. eapply Forall_impl; [|exact bounded]. cbn. intros; lia.
Qed.

Inductive event :=
| PayloadByte (id : nat) (byte : located_byte) (last : bool)
| BoundaryObservation (tag : nat).

Definition scalar_decrement_owner (current : owner) :=
  Owner (Nat.pred (owner_region_remaining current)) (Nat.pred (owner_path_remaining current))
    (owner_saved_state current) (owner_previous current) (owner_next current).

Definition scalar_decrement (current : state) :=
  State (state_mode current) (S (state_consumed current)) (Nat.pred (state_payload_remaining current))
    (map scalar_decrement_owner (state_owners current)) (state_metadata current).

Lemma scalar_decrement_is_one_bulk_step current : scalar_decrement current = bulk_decrement 1 current.
Proof.
  destruct current as [mode consumed payload owners info]. unfold scalar_decrement, bulk_decrement; cbn.
  f_equal; try lia. apply map_ext.
  intros [region path saved previous next]. unfold scalar_decrement_owner, decrement_owner; cbn.
  f_equal; lia.
Qed.

Definition scalar_payload (current : state) (byte : located_byte) : option (state * event) :=
  if eligible current && forallb (fun remaining => 0 <? remaining) (bounds current) then
    Some (scalar_decrement current,
      PayloadByte (metadata_id (state_metadata current)) byte (state_payload_remaining current =? 1))
  else None.

Fixpoint scalar_run (input : list located_byte) (current : state) : option (state * list event) :=
  match input with
  | [] => Some (current, [])
  | byte :: rest =>
      match scalar_payload current byte with
      | Some (next, observed) =>
          option_map (fun '(final, observations) => (final, observed :: observations)) (scalar_run rest next)
      | None => None
      end
  end.

Lemma one_strict_scalar_step current byte :
  state_mode current = LiveOpaque -> strict_interior 1 current ->
  scalar_payload current byte = Some (bulk_decrement 1 current,
    PayloadByte (metadata_id (state_metadata current)) byte false).
Proof.
  intros mode bounded. unfold scalar_payload, eligible. rewrite mode. cbn -[bounds forallb Nat.ltb Nat.eqb].
  assert (positive : forallb (fun remaining => 0 <? remaining) (bounds current) = true).
  { apply forallb_forall. intros remaining present. apply Nat.ltb_lt.
    unfold strict_interior in bounded. rewrite Forall_forall in bounded.
    specialize (bounded remaining present). lia. }
  assert (payload : state_payload_remaining current =? 1 = false).
  { apply Nat.eqb_neq. unfold strict_interior, bounds in bounded. inversion bounded. lia. }
  now rewrite positive, payload, scalar_decrement_is_one_bulk_step.
Qed.

Theorem strict_bulk_equals_repeated_scalar_countdown input current :
  state_mode current = LiveOpaque -> strict_interior (length input) current ->
  scalar_run input current = Some (bulk_decrement (length input) current,
    map (fun byte => PayloadByte (metadata_id (state_metadata current)) byte false) input).
Proof.
  revert current. induction input as [|byte rest IH]; intros current mode bounded; cbn.
  - now rewrite decrement_zero.
  - assert (one : strict_interior 1 current).
    { eapply strict_smaller; [|exact bounded]. cbn. lia. }
    rewrite (@one_strict_scalar_step current byte mode one).
    rewrite IH.
    + cbn. rewrite decrement_composes. reflexivity.
    + exact mode.
    + apply strict_after_decrement. exact bounded.
Qed.

Theorem successful_bulk_preserves_all_metadata_and_positive_residuals limit input_length current request next :
  try_advance limit input_length current request = Some next ->
  state_mode next = LiveOpaque /\ state_metadata next = state_metadata current /\
  map owner_saved_state (state_owners next) = map owner_saved_state (state_owners current) /\
  map owner_previous (state_owners next) = map owner_previous (state_owners current) /\
  map owner_next (state_owners next) = map owner_next (state_owners current) /\
  bounds next = map (fun value => value - plan_width request) (bounds current) /\
  Forall (fun value => 0 < value) (bounds next) /\
  state_consumed next = state_consumed current + plan_width request.
Proof.
  unfold try_advance. destruct (plan_guard limit input_length current request) eqn:guard; [|discriminate].
  intro accepted. inversion accepted; subst next.
  pose proof (@guarded_request_has_strict_bounds limit input_length current request guard)
    as (mode & positive & bounded & rest).
  split; [exact mode|]. split; [reflexivity|].
  split; [cbn; rewrite map_map; reflexivity|].
  split; [cbn; rewrite map_map; reflexivity|].
  split; [cbn; rewrite map_map; reflexivity|].
  split; [apply bounds_after_decrement|]. split; [|reflexivity].
  rewrite bounds_after_decrement. apply Forall_map.
  unfold strict_interior in bounded. eapply Forall_impl; [|exact bounded]. cbn. intros; lia.
Qed.

Definition driver_projection (observations : list event) :=
  filter (fun observed => match observed with PayloadByte _ _ false => false | _ => true end) observations.

Theorem only_ignored_nonfinal_payload_events_are_erased input current :
  state_mode current = LiveOpaque -> strict_interior (length input) current ->
  exists next observations,
    scalar_run input current = Some (next, observations) /\
    next = bulk_decrement (length input) current /\ driver_projection observations = [].
Proof.
  intros mode bounded.
  exists (bulk_decrement (length input) current),
    (map (fun byte => PayloadByte (metadata_id (state_metadata current)) byte false) input).
  split; [now apply strict_bulk_equals_repeated_scalar_countdown|]. split; [reflexivity|].
  clear mode bounded. unfold driver_projection. induction input; cbn; auto.
Qed.

Theorem bulk_cursor_consumption_preserves_exact_next_located_byte limit input current request next :
  try_advance limit (length input) current request = Some next ->
  let skipped := firstn (plan_width request) (skipn (state_consumed current) input) in
  length skipped = plan_width request /\
  skipn (plan_width request) (skipn (state_consumed current) input) = skipn (state_consumed next) input /\
  exists byte,
    nth_error input (state_consumed next) = Some byte /\
    nth_error (skipn (state_consumed current) input) (plan_width request) = Some byte /\
    option_map byte_provenance (nth_error input (state_consumed next)) = Some (byte_provenance byte).
Proof.
  unfold try_advance. destruct (plan_guard limit (length input) current request) eqn:guard; [|discriminate].
  intro accepted. inversion accepted; subst next. cbn.
  pose proof (@guarded_request_has_strict_bounds limit (length input) current request guard)
    as (mode & positive & bounded & available & fits).
  split.
  - apply firstn_length_le. rewrite length_skipn. lia.
  - split.
    + rewrite skipn_skipn. f_equal. lia.
    + destruct (nth_error input (state_consumed current + plan_width request)) eqn:found.
      * exists l. split; [reflexivity|]. split; [|reflexivity].
        rewrite nth_error_skipn. exact found.
      * apply nth_error_None in found. lia.
Qed.

Theorem accepted_bulk_refines_the_actual_checked_input_prefix limit input current request next :
  try_advance limit (length input) current request = Some next ->
  let skipped := firstn (plan_width request) (skipn (state_consumed current) input) in
  scalar_run skipped current = Some (next,
    map (fun byte => PayloadByte (metadata_id (state_metadata current)) byte false) skipped).
Proof.
  unfold try_advance. destruct (plan_guard limit (length input) current request) eqn:guard; [|discriminate].
  intro accepted. inversion accepted; subst next.
  pose proof (@guarded_request_has_strict_bounds limit (length input) current request guard)
    as (mode & positive & bounded & available & fits).
  assert (width : length (firstn (plan_width request) (skipn (state_consumed current) input)) =
    plan_width request).
  { apply firstn_length_le. rewrite length_skipn. lia. }
  pose proof (@strict_bulk_equals_repeated_scalar_countdown
    (firstn (plan_width request) (skipn (state_consumed current) input)) current mode) as reference.
  rewrite width in reference. now apply reference.
Qed.

Theorem every_intermediate_prefix_stays_before_all_completion_boundaries
    limit input_length current request next prefix :
  try_advance limit input_length current request = Some next -> prefix <= plan_width request ->
  state_mode (bulk_decrement prefix current) = LiveOpaque /\
  state_metadata (bulk_decrement prefix current) = state_metadata current /\
  Forall (fun value => 0 < value) (bounds (bulk_decrement prefix current)) /\
  state_consumed (bulk_decrement prefix current) < input_length /\
  state_consumed (bulk_decrement prefix current) < limit.
Proof.
  unfold try_advance. destruct (plan_guard limit input_length current request) eqn:guard; [|discriminate].
  intros accepted inside.
  pose proof (@guarded_request_has_strict_bounds limit input_length current request guard)
    as (mode & positive & bounded & available & fits).
  split; [exact mode|]. split; [reflexivity|]. split.
  - rewrite bounds_after_decrement. apply Forall_map.
    unfold strict_interior in bounded. eapply Forall_impl; [|exact bounded]. cbn. intros; lia.
  - cbn. lia.
Qed.

Theorem the_following_scalar_boundary_uses_the_same_state_and_physical_byte
    {Observation : Type} (boundary : state -> located_byte -> Observation)
    input current byte :
  state_mode current = LiveOpaque -> strict_interior (length input) current ->
  exists observations,
    scalar_run input current = Some (bulk_decrement (length input) current, observations) /\
    option_map (fun '(next, _) => boundary next byte) (scalar_run input current) =
      Some (boundary (bulk_decrement (length input) current) byte).
Proof.
  intros mode bounded. rewrite strict_bulk_equals_repeated_scalar_countdown by assumption.
  eexists. split; reflexivity.
Qed.

Theorem pending_dead_and_nonopaque_modes_do_not_plan_or_advance limit input_length current request :
  state_mode current <> LiveOpaque ->
  plan_interior limit input_length current = None /\ try_advance limit input_length current request = None.
Proof.
  intro ineligible. unfold plan_interior, try_advance, plan_guard, eligible.
  destruct (state_mode current); cbn; try contradiction; split; reflexivity.
Qed.

Theorem stale_origin_or_obligation_id_refuses_without_an_updated_state limit input_length current request :
  plan_from request <> state_consumed current \/ plan_id request <> metadata_id (state_metadata current) ->
  try_advance limit input_length current request = None.
Proof.
  intro stale. unfold try_advance, plan_guard.
  destruct stale as [origin|id].
  - apply Nat.eqb_neq in origin. rewrite origin. cbn. now rewrite !andb_false_r.
  - apply Nat.eqb_neq in id. rewrite id. cbn. now rewrite !andb_false_r.
Qed.

Theorem declared_lengths_larger_than_the_machine_limit_are_bounded_before_conversion
    limit consumed available payload info :
  2 <= available -> consumed + available <= limit -> available <= payload ->
  plan_interior limit (consumed + available)
    (State LiveOpaque consumed payload [] info) =
  Some (Plan consumed (metadata_id info) (available - 1)).
Proof.
  intros useful fits declared. unfold plan_interior, eligible, interior_width, bounds,
    available_prefix; cbn -[Nat.leb Nat.ltb Nat.min].
  assert (consumed <=? consumed + available = true) by (apply Nat.leb_le; lia).
  assert (consumed <=? limit = true) by (apply Nat.leb_le; lia).
  rewrite H, H0. cbn -[Nat.min Nat.ltb].
  replace (consumed + available - consumed) with available by lia.
  rewrite (Nat.min_l available (limit - consumed)) by lia.
  rewrite (Nat.min_r payload available) by lia.
  replace (Nat.pred available) with (available - 1) by lia.
  assert (0 <? available - 1 = true) by (apply Nat.ltb_lt; lia). now rewrite H1.
Qed.

Definition control_metadata := Metadata 17 NestedEpmPayload (4, 91) 3 None true.
Definition control_state := State LiveOpaque 3 4
  [Owner 9 8 [31; 32] [LocatedByte 11 6 20] (Some 8); Owner 4 3 [33] [] None]
  control_metadata.
Definition control_bytes := [LocatedByte 170 2 31; LocatedByte 187 9 8].

Theorem strict_bulk_positive_boundary_and_projection_controls :
  plan_interior 32 7 control_state = Some (Plan 3 17 2) /\
  try_advance 32 7 control_state (Plan 3 17 2) = Some (bulk_decrement 2 control_state) /\
  bounds (bulk_decrement 2 control_state) = [2; 7; 6; 2; 1] /\
  scalar_run control_bytes control_state = Some (bulk_decrement 2 control_state,
    [PayloadByte 17 (LocatedByte 170 2 31) false; PayloadByte 17 (LocatedByte 187 9 8) false]) /\
  driver_projection [PayloadByte 17 (LocatedByte 170 2 31) false;
    PayloadByte 17 (LocatedByte 187 9 8) true; BoundaryObservation 91] =
    [PayloadByte 17 (LocatedByte 187 9 8) true; BoundaryObservation 91] /\
  plan_interior 32 7 (State LiveOpaque 3 4 [Owner 2 8 [] [] None] control_metadata) =
    Some (Plan 3 17 1) /\
  plan_interior 32 7 (State LiveOpaque 3 64 [] control_metadata) = Some (Plan 3 17 3).
Proof. repeat split; reflexivity. Qed.

Definition omit_owner_bounds (limit input_length : nat) (current : state) :=
  Nat.pred (Nat.min (state_payload_remaining current) (available_prefix limit input_length current)).
Definition include_the_boundary (limit input_length : nat) (current : state) :=
  bounded_minimum (bounds current) (available_prefix limit input_length current).

Theorem strict_bulk_causal_controls_reject_missing_bounds_boundary_crossing_and_stale_requests :
  omit_owner_bounds 32 7 control_state = 3 /\
  include_the_boundary 32 7 control_state = 3 /\
  owner_path_remaining (nth 1 (state_owners (bulk_decrement 3 control_state))
    (Owner 0 0 [] [] None)) = 0 /\
  try_advance 32 7 control_state (Plan 3 17 3) = None /\
  try_advance 32 7 control_state (Plan 4 17 2) = None /\
  try_advance 32 7 control_state (Plan 3 18 2) = None /\
  plan_interior 32 4 control_state = None /\
  plan_interior 32 3 control_state = None /\
  plan_interior 32 2 control_state = None /\
  plan_interior 32 7 (State PendingChild 3 4 [] control_metadata) = None /\
  plan_interior 32 7 (State (DeadPath 41 (7, 9)) 3 4 [] control_metadata) = None /\
  plan_interior 32 7 (State (NonOpaque 4) 3 4 [] control_metadata) = None.
Proof. repeat split; reflexivity. Qed.

Print Assumptions planner_returns_the_exact_positive_strict_minimum.
Print Assumptions a_fresh_plan_is_accepted_without_rechecking_payload_bytes.
Print Assumptions strict_bulk_equals_repeated_scalar_countdown.
Print Assumptions successful_bulk_preserves_all_metadata_and_positive_residuals.
Print Assumptions only_ignored_nonfinal_payload_events_are_erased.
Print Assumptions bulk_cursor_consumption_preserves_exact_next_located_byte.
Print Assumptions accepted_bulk_refines_the_actual_checked_input_prefix.
Print Assumptions every_intermediate_prefix_stays_before_all_completion_boundaries.
Print Assumptions the_following_scalar_boundary_uses_the_same_state_and_physical_byte.
Print Assumptions pending_dead_and_nonopaque_modes_do_not_plan_or_advance.
Print Assumptions stale_origin_or_obligation_id_refuses_without_an_updated_state.
Print Assumptions declared_lengths_larger_than_the_machine_limit_are_bounded_before_conversion.
Print Assumptions strict_bulk_positive_boundary_and_projection_controls.
Print Assumptions strict_bulk_causal_controls_reject_missing_bounds_boundary_crossing_and_stale_requests.

End StrictOpaqueInterior.

Module FocusedRegionOwners.

Module Slots := ReusableFrameOwnership.
Import CanonicalKeyProtocol.

Inductive path_status := OpenPath | SplitPath | DeadPath (code source offset : nat).

Record path_metadata := PathMetadata {
  path_escape_allowed : bool;
  path_status_value : path_status;
  path_completed_segments : nat;
  path_first_tag : option (nat * nat * nat);
  path_stable : bool
}.

Record region_metadata := RegionMetadata {
  region_slot : nat;
  region_owner : path_metadata;
  region_previous : option (list located_byte);
  region_declared_length : nat;
  region_start_depth : nat;
  region_next : Slots.graph
}.

Record leaf := Leaf {
  leaf_metadata : path_metadata;
  leaf_frames : Slots.graph
}.

Inductive path_graph :=
| LeafGraph (current : leaf)
| RegionGraph (metadata : region_metadata) (region_remaining path_remaining : nat)
    (child : path_graph).

Fixpoint graph_slots (value : path_graph) : list nat :=
  match value with
  | LeafGraph current => Slots.graph_slots (leaf_frames current)
  | RegionGraph metadata _ _ child =>
      region_slot metadata :: graph_slots child ++ Slots.graph_slots (region_next metadata)
  end.

Record owner := Owner {
  owner_metadata : region_metadata;
  owner_region_end : nat;
  owner_path_end : nat
}.

Definition owner_bound (current : owner) := Nat.min (owner_region_end current) (owner_path_end current).

Definition minimum_push (bound : nat) (rest : option nat) :=
  Some (match rest with None => bound | Some other => Nat.min bound other end).

Record cached_owner := CachedOwner {
  owner_value : owner;
  owner_cached_minimum : option nat
}.

Definition cached_minimum (owners : list cached_owner) :=
  match owners with [] => None | current :: _ => owner_cached_minimum current end.

Definition push_owner (current : owner) (owners : list cached_owner) :=
  CachedOwner current (minimum_push (owner_bound current) (cached_minimum owners)) :: owners.

Fixpoint cache_valid (owners : list cached_owner) : Prop :=
  match owners with
  | [] => True
  | current :: rest =>
      owner_cached_minimum current = minimum_push (owner_bound (owner_value current)) (cached_minimum rest) /\
      cache_valid rest
  end.

Definition residual_graph (stamp : nat) (current : owner) (child : path_graph) :=
  RegionGraph (owner_metadata current) (owner_region_end current - stamp)
    (owner_path_end current - stamp) child.

Fixpoint reify (stamp : nat) (current : path_graph) (owners : list cached_owner) : path_graph :=
  match owners with
  | [] => current
  | first :: rest => reify stamp (residual_graph stamp (owner_value first) current) rest
  end.

Definition absolute_owner stamp metadata region_remaining path_remaining :=
  Owner metadata (stamp + region_remaining) (stamp + path_remaining).

Fixpoint decompose (stamp : nat) (current : path_graph) (owners : list cached_owner)
    : leaf * list cached_owner :=
  match current with
  | LeafGraph focused => (focused, owners)
  | RegionGraph metadata region_remaining path_remaining child =>
      decompose stamp child (push_owner (absolute_owner stamp metadata region_remaining path_remaining) owners)
  end.

Lemma absolute_residual_exact stamp metadata region_remaining path_remaining child :
  residual_graph stamp (absolute_owner stamp metadata region_remaining path_remaining) child =
    RegionGraph metadata region_remaining path_remaining child.
Proof.
  unfold residual_graph, absolute_owner; cbn. f_equal; lia.
Qed.

Lemma decompose_reifies stamp current owners :
  let '(focused, suspended) := decompose stamp current owners in
  reify stamp (LeafGraph focused) suspended = reify stamp current owners.
Proof.
  revert owners. induction current as [focused|metadata region_remaining path_remaining child IH]; intro owners; cbn.
  - reflexivity.
  - specialize (IH (push_owner (absolute_owner stamp metadata region_remaining path_remaining) owners)).
    destruct (decompose stamp child (push_owner (absolute_owner stamp metadata region_remaining path_remaining) owners))
      as [focused suspended]. cbn in IH. rewrite absolute_residual_exact in IH. exact IH.
Qed.

Theorem concrete_decomposition_reifies_every_path_and_region_field stamp current focused owners :
  decompose stamp current [] = (focused, owners) ->
  reify stamp (LeafGraph focused) owners = current.
Proof.
  intro observed. pose proof (decompose_reifies stamp current []) as exact.
  rewrite observed in exact. exact exact.
Qed.

Theorem cached_minimum_push_and_pop_preserve_the_exact_prefix current owners :
  cache_valid owners ->
  cache_valid (push_owner current owners) /\
  cached_minimum (push_owner current owners) = minimum_push (owner_bound current) (cached_minimum owners) /\
  tl (push_owner current owners) = owners.
Proof. intro valid. unfold push_owner; cbn. repeat split; auto. Qed.

Lemma decompose_cache_valid stamp current owners :
  cache_valid owners -> cache_valid (snd (decompose stamp current owners)).
Proof.
  revert owners. induction current as [focused|metadata region_remaining path_remaining child IH]; intros owners valid; cbn.
  - exact valid.
  - apply IH. cbn. auto.
Qed.

Definition owner_held_slots (current : owner) :=
  region_slot (owner_metadata current) :: Slots.graph_slots (region_next (owner_metadata current)).

Fixpoint detached_slots (focused : path_graph) (owners : list cached_owner) :=
  match owners with
  | [] => graph_slots focused
  | first :: rest => owner_held_slots (owner_value first) ++ detached_slots focused rest
  end.

Lemma reify_slot_membership stamp focused owners slot :
  In slot (graph_slots (reify stamp focused owners)) <->
  In slot (graph_slots focused) \/ exists current, In current owners /\ In slot (owner_held_slots (owner_value current)).
Proof.
  revert focused. induction owners as [|first rest IH]; intro focused; cbn.
  - split; [auto|intros [inside|[current [inside _]]]; [exact inside|contradiction]].
  - rewrite IH. unfold residual_graph, owner_held_slots; cbn. rewrite in_app_iff.
    split.
    + intros [[same|[child|tail]]|[current [inside held]]].
      * right. exists first. split; [now left|now left].
      * now left.
      * right. exists first. split; [now left|now right].
      * right. exists current. split; [now right|exact held].
    + intros [child|[current [[same|inside] held]]].
      * left. right. now left.
      * subst current. destruct held as [same|tail]; [left; now left|left; right; now right].
      * right. exists current. auto.
Qed.

Theorem detached_spine_preserves_exact_frame_membership_and_count stamp focused owners :
  (forall slot, In slot (graph_slots (reify stamp focused owners)) <-> In slot (detached_slots focused owners)) /\
  length (graph_slots (reify stamp focused owners)) = length (detached_slots focused owners).
Proof.
  split.
  - intro slot. rewrite reify_slot_membership.
    induction owners as [|first rest IH]; cbn.
    + split; [intros [inside|[current [inside _]]]; [exact inside|contradiction]|auto].
    + rewrite in_app_iff. rewrite <- IH. firstorder subst; auto.
  - revert focused. induction owners as [|first rest IH]; intro focused; cbn; [reflexivity|].
    rewrite IH, app_length. unfold owner_held_slots, residual_graph; cbn.
    clear IH. induction rest as [|current rest IH]; cbn in *.
    + rewrite app_length. lia.
    + rewrite !app_length in *. lia.
Qed.

Lemma nodup_append_left {A : Type} (left right : list A) : NoDup (left ++ right) -> NoDup left.
Proof.
  induction left as [|first rest IH]; intro distinct; [constructor|].
  inversion distinct; subst. constructor.
  - intro inside. apply H1. apply in_or_app. now left.
  - now apply IH.
Qed.

Lemma reify_nodup_focus stamp focused owners :
  NoDup (graph_slots (reify stamp focused owners)) -> NoDup (graph_slots focused).
Proof.
  revert focused. induction owners as [|first rest IH]; intros focused distinct; [exact distinct|].
  cbn in distinct. apply IH in distinct. unfold residual_graph in distinct; cbn in distinct.
  inversion distinct; subst. now apply nodup_append_left in H2.
Qed.

Theorem detached_region_child_and_outer_continuation_are_disjoint stamp focused first rest :
  NoDup (graph_slots (reify stamp focused (first :: rest))) ->
  forall slot, In slot (graph_slots focused) -> ~ In slot (owner_held_slots (owner_value first)).
Proof.
  intro distinct. cbn in distinct. apply reify_nodup_focus in distinct.
  unfold residual_graph in distinct; cbn in distinct. inversion distinct; subst.
  intros slot child [same|tail].
  - subst slot. apply H1. apply in_or_app. now left.
  - eapply Slots.nodup_append_disjoint; eauto.
Qed.

Definition subtract_owner width current :=
  match current with
  | RegionGraph metadata region_remaining path_remaining child =>
      RegionGraph metadata (region_remaining - width) (path_remaining - width) child
  | LeafGraph focused => LeafGraph focused
  end.

Fixpoint decrement_region_graph width current :=
  match current with
  | LeafGraph focused => LeafGraph focused
  | RegionGraph metadata region_remaining path_remaining child =>
      RegionGraph metadata (region_remaining - width) (path_remaining - width)
        (decrement_region_graph width child)
  end.

Theorem widened_endpoints_recover_exact_residuals_without_early_overflow
    stamp remaining machine_limit declared_limit :
  stamp <= machine_limit -> remaining <= declared_limit ->
  stamp + remaining <= machine_limit + declared_limit /\
  stamp + remaining - stamp = remaining.
Proof. lia. Qed.

Lemma endpoint_shift remaining stamp width : remaining - (stamp + width) = remaining - stamp - width.
Proof. lia. Qed.

Lemma stamp_shift_reifies stamp width focused owners :
  reify (stamp + width) (decrement_region_graph width focused) owners =
    decrement_region_graph width (reify stamp focused owners).
Proof.
  revert focused. induction owners as [|first rest IH]; intro focused; cbn; [reflexivity|].
  rewrite <- IH. unfold residual_graph; cbn. rewrite !endpoint_shift. reflexivity.
Qed.

Theorem advancing_graph_stamp_decrements_every_owner_without_rewriting_spine stamp width focused owners :
  reify (stamp + width) (LeafGraph focused) owners =
    decrement_region_graph width (reify stamp (LeafGraph focused) owners).
Proof. exact (stamp_shift_reifies stamp width (LeafGraph focused) owners). Qed.

Definition minimum_above stamp bound :=
  match bound with None => True | Some ending => stamp < ending end.

Lemma minimum_push_above stamp ending rest :
  minimum_above stamp (minimum_push ending rest) <-> stamp < ending /\ minimum_above stamp rest.
Proof.
  destruct rest as [other|]; cbn.
  - destruct (le_dec ending other).
    + rewrite Nat.min_l by lia. lia.
    + rewrite Nat.min_r by lia. lia.
  - tauto.
Qed.

Theorem cached_minimum_checks_all_enclosing_region_and_path_endpoints stamp owners :
  cache_valid owners ->
  (minimum_above stamp (cached_minimum owners) <->
    Forall (fun current => stamp < owner_region_end (owner_value current) /\
      stamp < owner_path_end (owner_value current)) owners).
Proof.
  induction owners as [|first rest IH]; intro valid; cbn in *.
  - split; auto.
  - destruct valid as [cache valid]. rewrite cache, minimum_push_above, (IH valid).
    unfold owner_bound. assert (minimum : stamp < Nat.min (owner_region_end (owner_value first))
      (owner_path_end (owner_value first)) <->
      stamp < owner_region_end (owner_value first) /\ stamp < owner_path_end (owner_value first)).
    { destruct (le_dec (owner_region_end (owner_value first)) (owner_path_end (owner_value first))).
      - rewrite Nat.min_l by lia. lia.
      - rewrite Nat.min_r by lia. lia. }
    rewrite minimum. split; [intros [head tail]; constructor; assumption|].
    intro all. inversion all; subst. auto.
Qed.

Definition expired stamp current := owner_bound (owner_value current) <=? stamp.

Fixpoint first_expired stamp (outer_first : list cached_owner) : option cached_owner :=
  match outer_first with
  | [] => None
  | first :: rest => if expired stamp first then Some first else first_expired stamp rest
  end.

Theorem zero_boundary_selection_is_outer_first_with_no_skipped_predecessor stamp owners selected :
  first_expired stamp (rev owners) = Some selected ->
  exists before after,
    rev owners = before ++ selected :: after /\
    Forall (fun current => expired stamp current = false) before /\ expired stamp selected = true.
Proof.
  generalize (rev owners). intro outer_first. induction outer_first as [|first rest IH]; cbn; [discriminate|].
  destruct (expired stamp first) eqn:head.
  - intro found. inversion found; subst. exists [], rest. cbn. repeat split; auto.
  - intro found. destruct (IH found) as [before [after [shape [clear chosen]]]].
    exists (first :: before), after. cbn. split; [now rewrite shape|]. split; [constructor; assumption|exact chosen].
Qed.

Record focused := Focused {
  graph_stamp : nat;
  focused_leaf : leaf;
  focused_owners : list cached_owner
}.

Definition focused_graph current :=
  reify (graph_stamp current) (LeafGraph (focused_leaf current)) (focused_owners current).

Record shallow_result := ShallowResult {
  shallow_leaf : leaf;
  shallow_event : nat;
  shallow_clock : nat
}.

Fixpoint eager_shallow (action : leaf -> shallow_result) (current : path_graph)
    : path_graph * nat * nat :=
  match current with
  | LeafGraph current =>
      let after := action current in (LeafGraph (shallow_leaf after), shallow_event after, shallow_clock after)
  | RegionGraph metadata remaining path_remaining child =>
      let '(next, event, clock) := eager_shallow action child in
      (RegionGraph metadata (remaining - 1) (path_remaining - 1) next, event, clock)
  end.

Definition ordinary_focused (action : leaf -> shallow_result) current :=
  let after := action (focused_leaf current) in
  (Focused (graph_stamp current + 1) (shallow_leaf after) (focused_owners current),
    shallow_event after, shallow_clock after).

Lemma eager_shallow_reify action stamp current owners next event clock :
  eager_shallow action current = (next, event, clock) ->
  eager_shallow action (reify stamp current owners) =
    (reify (stamp + 1) next owners, event, clock).
Proof.
  revert current next event clock. induction owners as [|first rest IH]; intros current next event clock step; cbn; [exact step|].
  apply IH. unfold residual_graph; cbn. rewrite step. rewrite !endpoint_shift. reflexivity.
Qed.

Theorem ordinary_shared_shallow_action_refines_eager_owners_and_preserves_exact_events action current next event clock :
  ordinary_focused action current = (next, event, clock) ->
  eager_shallow action (focused_graph current) = (focused_graph next, event, clock) /\
  focused_owners next = focused_owners current.
Proof.
  destruct current as [stamp current owners]. unfold ordinary_focused; cbn.
  destruct (action current) as [after observed allocated] eqn:local.
  intro accepted. inversion accepted; subst; clear accepted. split; [|reflexivity].
  unfold focused_graph; cbn. eapply eager_shallow_reify. cbn. now rewrite local.
Qed.

Definition ordinary_binding (grammar action : leaf -> shallow_result) :=
  forall current, grammar current = action current.

Lemma eager_shallow_inherits_local_action_binding grammar action current :
  ordinary_binding grammar action -> eager_shallow grammar current = eager_shallow action current.
Proof.
  intro binding. induction current as [current|metadata remaining path_remaining child IH]; cbn.
  - now rewrite binding.
  - now rewrite IH.
Qed.

Theorem explicit_shared_grammar_binding_transfers_ordinary_observations grammar action current next event clock :
  ordinary_binding grammar action ->
  ordinary_focused action current = (next, event, clock) ->
  eager_shallow grammar (focused_graph current) = (focused_graph next, event, clock).
Proof.
  intros binding step. rewrite (eager_shallow_inherits_local_action_binding (focused_graph current) binding).
  now apply ordinary_shared_shallow_action_refines_eager_owners_and_preserves_exact_events in step as [step _].
Qed.

Definition leaf_pending (current : leaf) : option nat :=
  match leaf_frames current with
  | Slots.LinkGraph _ metadata _ =>
      match Slots.frame_obligation metadata with
      | Some obligation => if Slots.obligation_status obligation =? 1
          then Some (Slots.obligation_number obligation) else None
      | None => None
      end
  | _ => None
  end.

Definition is_pending current := match leaf_pending current with Some _ => true | None => false end.

Inductive completion_result :=
| CompletedOwner (current : leaf)
| RefusedOwner (code source offset : nat).

Inductive completion_observation :=
| SuspendedCompletion (current : focused) (popped : nat)
| RefusedCompletion (current : focused) (code source offset popped : nat).

Definition add_pop result :=
  match result with
  | SuspendedCompletion current popped => SuspendedCompletion current (S popped)
  | RefusedCompletion current code source offset popped => RefusedCompletion current code source offset (S popped)
  end.

Fixpoint complete_equal_suffix
    (complete : region_metadata -> nat -> leaf -> completion_result)
    (stamp : nat) (current : leaf) (owners : list cached_owner) : completion_observation :=
  if is_pending current then SuspendedCompletion (Focused stamp current owners) 0 else
  match owners with
  | [] => SuspendedCompletion (Focused stamp current []) 0
  | first :: rest =>
      if owner_path_end (owner_value first) =? stamp then
        match complete (owner_metadata (owner_value first))
          (owner_region_end (owner_value first) - stamp) current with
        | CompletedOwner next => add_pop (complete_equal_suffix complete stamp next rest)
        | RefusedOwner code source offset => RefusedCompletion (Focused stamp current owners) code source offset 0
        end
      else SuspendedCompletion (Focused stamp current owners) 0
  end.

Definition completion_focus result :=
  match result with
  | SuspendedCompletion current _ | RefusedCompletion current _ _ _ _ => current
  end.

Definition completion_pops result :=
  match result with
  | SuspendedCompletion _ popped | RefusedCompletion _ _ _ _ popped => popped
  end.

Theorem incremental_equal_boundary_completion_only_removes_the_consumed_suffix complete stamp current owners :
  let result := complete_equal_suffix complete stamp current owners in
  completion_pops result + length (focused_owners (completion_focus result)) = length owners /\
  focused_owners (completion_focus result) = skipn (completion_pops result) owners.
Proof.
  revert current. induction owners as [|first rest IH]; intro current; cbn;
    destruct (is_pending current) eqn:pending; cbn; try (split; reflexivity).
  destruct (owner_path_end (owner_value first) =? stamp) eqn:boundary; cbn; [|split; reflexivity].
  destruct (complete (owner_metadata (owner_value first)) (owner_region_end (owner_value first) - stamp) current)
    as [next|code source offset]; cbn; [|split; reflexivity].
  specialize (IH next). destruct (complete_equal_suffix complete stamp next rest); cbn in *;
    destruct IH as [count suffix]; (split; [lia|exact suffix]).
Qed.

Theorem pending_child_at_zero_boundary_preserves_all_owners_without_completion complete stamp current owners id :
  leaf_pending current = Some id ->
  complete_equal_suffix complete stamp current owners = SuspendedCompletion (Focused stamp current owners) 0.
Proof.
  intro pending. destruct owners; cbn; unfold is_pending; rewrite pending; reflexivity.
Qed.

Definition resolve_focused (resolve : leaf -> leaf) (id : nat) (current : focused) : option focused :=
  match leaf_pending (focused_leaf current) with
  | Some expected => if id =? expected then
      Some (Focused (graph_stamp current) (resolve (focused_leaf current)) (focused_owners current)) else None
  | None => None
  end.

Theorem wrong_obligation_id_refuses_without_a_candidate_or_owner_change resolve id expected current :
  leaf_pending (focused_leaf current) = Some expected -> id <> expected ->
  resolve_focused resolve id current = None.
Proof.
  intros pending wrong. unfold resolve_focused. rewrite pending.
  apply Nat.eqb_neq in wrong. now rewrite wrong.
Qed.

Definition kill_metadata code source offset current :=
  PathMetadata (path_escape_allowed current) (DeadPath code source offset)
    (path_completed_segments current) (path_first_tag current) (path_stable current).

Definition kill_graph code source offset current :=
  match current with
  | LeafGraph current => LeafGraph (Leaf (kill_metadata code source offset (leaf_metadata current)) (leaf_frames current))
  | RegionGraph metadata remaining path_remaining child =>
      RegionGraph (RegionMetadata (region_slot metadata)
        (kill_metadata code source offset (region_owner metadata)) (region_previous metadata)
        (region_declared_length metadata) (region_start_depth metadata) (region_next metadata))
        remaining path_remaining child
  end.

Inductive representation := FocusedRepresentation (current : focused) | ReifiedRepresentation (current : path_graph).

Definition represented_graph current :=
  match current with FocusedRepresentation current => focused_graph current | ReifiedRepresentation current => current end.

Record world := World {
  world_representation : representation;
  input_consumed : nat;
  obligation_clock : nat
}.

Inductive selection := ReturnedError | AcceptedStep | DeferredRefusal (code source offset : nat).

Definition select_transaction (choice : selection) (before : focused) (candidate : focused)
    (before_consumed after_consumed before_clock after_clock : nat) : world :=
  match choice with
  | ReturnedError => World (FocusedRepresentation before) before_consumed before_clock
  | AcceptedStep => World (FocusedRepresentation candidate) after_consumed after_clock
  | DeferredRefusal code source offset =>
      let selected := match focused_owners before with
        | [] => focused_graph candidate
        | _ :: _ => focused_graph before
        end in
      World (ReifiedRepresentation (kill_graph code source offset selected)) after_consumed after_clock
  end.

Theorem transaction_selection_uses_the_pre_step_graph_for_outer_refusal_but_keeps_global_clock
    before candidate first rest before_consumed after_consumed before_clock after_clock code source offset :
  focused_owners before = first :: rest ->
  select_transaction (DeferredRefusal code source offset) before candidate
    before_consumed after_consumed before_clock after_clock =
  World (ReifiedRepresentation (kill_graph code source offset (focused_graph before))) after_consumed after_clock.
Proof. intro owners. unfold select_transaction. now rewrite owners. Qed.

Theorem shallow_fresh_frame_selection_and_returned_error_are_distinct
    before candidate before_consumed after_consumed before_clock after_clock code source offset :
  focused_owners before = [] ->
  select_transaction (DeferredRefusal code source offset) before candidate
    before_consumed after_consumed before_clock after_clock =
    World (ReifiedRepresentation (kill_graph code source offset (focused_graph candidate))) after_consumed after_clock /\
  select_transaction ReturnedError before candidate before_consumed after_consumed before_clock after_clock =
    World (FocusedRepresentation before) before_consumed before_clock.
Proof. intro owners. unfold select_transaction. rewrite owners. auto. Qed.

Definition shared_protocol := path_graph -> nat -> nat -> nat -> path_graph * nat * nat.

Definition reified_protocol_step (shared : shared_protocol) (request : nat) (current : world) : option world :=
  match world_representation current with
  | FocusedRepresentation _ => None
  | ReifiedRepresentation graph =>
      let '(next, consumed, clock) := shared graph (input_consumed current) (obligation_clock current) request in
      Some (World (ReifiedRepresentation next) consumed clock)
  end.

Theorem reified_fault_operations_use_the_exact_same_shared_protocol_without_reaccounting
    shared request graph consumed clock next next_consumed next_clock :
  shared graph consumed clock request = (next, next_consumed, next_clock) ->
  reified_protocol_step shared request (World (ReifiedRepresentation graph) consumed clock) =
    Some (World (ReifiedRepresentation next) next_consumed next_clock).
Proof. intro result. unfold reified_protocol_step; cbn. now rewrite result. Qed.

Definition control_path := PathMetadata false OpenPath 2 (Some (13, 7, 91)) true.
Definition control_frame := Slots.FrameMetadata [4; 8] None.
Definition control_leaf := Leaf control_path (Slots.LinkGraph 3 control_frame Slots.EmptyGraph).
Definition control_metadata slot := RegionMetadata slot control_path (Some [LocatedByte 8 2 31])
  7 3 (Slots.LinkGraph (slot + 10) control_frame Slots.EmptyGraph).
Definition control_outer := absolute_owner 3 (control_metadata 1) 12 8.
Definition control_inner := absolute_owner 3 (control_metadata 2) 9 6.
Definition control_owners := push_owner control_inner (push_owner control_outer []).
Definition control_focused := Focused 3 control_leaf control_owners.
Definition control_action current := ShallowResult current 71 19.
Definition control_pending := Leaf control_path (Slots.LinkGraph 4
  (Slots.FrameMetadata [] (Some (Slots.ObligationMetadata 17 2 3 7 91 1))) Slots.EmptyGraph).
Definition complete_control metadata (remaining : nat) (current : leaf) := CompletedOwner
  (Leaf (region_owner metadata) (region_next metadata)).

Theorem focused_representation_accounting_completion_and_selection_positive_controls :
  cache_valid control_owners /\
  cached_minimum control_owners = Some 9 /\
  ordinary_focused control_action control_focused = (Focused 4 control_leaf control_owners, 71, 19) /\
  first_expired 11 (rev control_owners) = Some (hd (CachedOwner control_outer None) (push_owner control_outer [])) /\
  completion_pops (complete_equal_suffix complete_control 9 control_leaf
    (push_owner (Owner (control_metadata 2) 9 9) (push_owner (Owner (control_metadata 1) 12 9) []))) = 2 /\
  complete_equal_suffix complete_control 9 control_pending
    (push_owner (Owner (control_metadata 2) 9 9) []) =
    SuspendedCompletion (Focused 9 control_pending (push_owner (Owner (control_metadata 2) 9 9) [])) 0 /\
  resolve_focused (fun current => current) 17 (Focused 9 control_pending control_owners) =
    Some (Focused 9 control_pending control_owners).
Proof. repeat split; reflexivity. Qed.

Definition consumed_residual (consumed : nat) (current : owner) := owner_path_end current - consumed.
Definition inner_first_zero stamp owners := first_expired stamp owners.
Definition fault_suffix graph consumed (clock : nat) request :=
  (if request =? 0 then graph else kill_graph 93 8 consumed graph, consumed + 1, clock).

Theorem focused_causal_controls_reject_wrong_stamps_wrong_boundary_order_and_dead_absorption :
  owner_path_end control_inner - 3 = 6 /\ consumed_residual 5 control_inner = 4 /\
  first_expired 11 (rev control_owners) <> inner_first_zero 11 control_owners /\
  resolve_focused (fun current => current) 18 (Focused 9 control_pending control_owners) = None /\
  focused_graph control_focused <> focused_graph (Focused 5 control_leaf control_owners) /\
  reified_protocol_step fault_suffix 1
    (World (ReifiedRepresentation (kill_graph 41 2 31 (focused_graph control_focused))) 20 19) =
    Some (World (ReifiedRepresentation (kill_graph 93 8 20 (focused_graph control_focused))) 21 19) /\
  obligation_clock (select_transaction (DeferredRefusal 41 2 31) control_focused
    (Focused 4 control_leaf control_owners) 3 4 18 19) = 19.
Proof.
  repeat split; vm_compute; congruence.
Qed.

Print Assumptions concrete_decomposition_reifies_every_path_and_region_field.
Print Assumptions cached_minimum_push_and_pop_preserve_the_exact_prefix.
Print Assumptions detached_spine_preserves_exact_frame_membership_and_count.
Print Assumptions detached_region_child_and_outer_continuation_are_disjoint.
Print Assumptions widened_endpoints_recover_exact_residuals_without_early_overflow.
Print Assumptions advancing_graph_stamp_decrements_every_owner_without_rewriting_spine.
Print Assumptions cached_minimum_checks_all_enclosing_region_and_path_endpoints.
Print Assumptions zero_boundary_selection_is_outer_first_with_no_skipped_predecessor.
Print Assumptions ordinary_shared_shallow_action_refines_eager_owners_and_preserves_exact_events.
Print Assumptions explicit_shared_grammar_binding_transfers_ordinary_observations.
Print Assumptions incremental_equal_boundary_completion_only_removes_the_consumed_suffix.
Print Assumptions pending_child_at_zero_boundary_preserves_all_owners_without_completion.
Print Assumptions wrong_obligation_id_refuses_without_a_candidate_or_owner_change.
Print Assumptions transaction_selection_uses_the_pre_step_graph_for_outer_refusal_but_keeps_global_clock.
Print Assumptions shallow_fresh_frame_selection_and_returned_error_are_distinct.
Print Assumptions reified_fault_operations_use_the_exact_same_shared_protocol_without_reaccounting.
Print Assumptions focused_representation_accounting_completion_and_selection_positive_controls.
Print Assumptions focused_causal_controls_reject_wrong_stamps_wrong_boundary_order_and_dead_absorption.

End FocusedRegionOwners.
