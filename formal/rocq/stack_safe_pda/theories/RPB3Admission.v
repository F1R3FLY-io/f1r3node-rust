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

Definition endpoint_cache := option (nat * nat).

Definition extend_endpoint_cache (prefer_inner : bool) (ending : nat) (rest : endpoint_cache) : endpoint_cache :=
  match rest with
  | None => Some (ending, 0)
  | Some (other, index) => Some (Nat.min ending other,
      if (if prefer_inner then ending <=? other else ending <? other) then 0 else S index)
  end.

Fixpoint compute_endpoint_cache prefer_inner (endings : list nat) : endpoint_cache :=
  match endings with
  | [] => None
  | first :: rest => extend_endpoint_cache prefer_inner first (compute_endpoint_cache prefer_inner rest)
  end.

Definition cached_due stamp (cache : endpoint_cache) :=
  match cache with Some (ending, index) => if ending =? stamp then Some index else None | None => None end.

Fixpoint scan_due (prefer_inner : bool) stamp (endings : list nat) : option nat :=
  match endings with
  | [] => None
  | first :: rest =>
      if first =? stamp then
        if prefer_inner then Some 0 else
          match scan_due prefer_inner stamp rest with Some index => Some (S index) | None => Some 0 end
      else option_map S (scan_due prefer_inner stamp rest)
  end.

Lemma computed_cache_has_lower_bound prefer_inner endings stamp ending index :
  Forall (fun value => stamp <= value) endings ->
  compute_endpoint_cache prefer_inner endings = Some (ending, index) -> stamp <= ending.
Proof.
  revert ending index. induction endings as [|first rest IH]; intros ending index bounds found; cbn in found; [discriminate|].
  inversion bounds; subst. unfold extend_endpoint_cache in found.
  destruct (compute_endpoint_cache prefer_inner rest) as [[other position]|] eqn:tail.
  - inversion found; subst. apply Nat.min_glb; [assumption|]. eapply IH; eauto.
  - inversion found; subst. assumption.
Qed.

Lemma cached_due_extends_exact_scan prefer_inner stamp ending rest :
  stamp <= ending ->
  (forall other index, rest = Some (other, index) -> stamp <= other) ->
  cached_due stamp (extend_endpoint_cache prefer_inner ending rest) =
    if ending =? stamp then
      if prefer_inner then Some 0 else
        match cached_due stamp rest with Some index => Some (S index) | None => Some 0 end
    else option_map S (cached_due stamp rest).
Proof.
  intros head_bound tail_bound. destruct rest as [[other index]|]; [|destruct prefer_inner; cbn; now destruct (ending =? stamp)].
  assert (other_bound : stamp <= other) by (eapply tail_bound; reflexivity).
  destruct (Nat.eq_dec ending stamp) as [head_equal|head_distinct];
    destruct (Nat.eq_dec other stamp) as [tail_equal|tail_distinct].
  - subst ending other. destruct prefer_inner; unfold cached_due, extend_endpoint_cache;
      rewrite Nat.min_id, ?Nat.leb_refl, ?Nat.ltb_irrefl, ?Nat.eqb_refl; reflexivity.
  - subst ending. assert (lt : (stamp <? other) = true) by (apply Nat.ltb_lt; lia).
    assert (le : (stamp <=? other) = true) by (apply Nat.leb_le; lia).
    assert (neq : (other =? stamp) = false) by (apply Nat.eqb_neq; assumption).
    destruct prefer_inner; unfold cached_due, extend_endpoint_cache;
      rewrite Nat.min_l by lia; rewrite ?lt, ?le, ?Nat.eqb_refl, ?neq; reflexivity.
  - subst other. assert (lt : (ending <? stamp) = false) by (apply Nat.ltb_ge; lia).
    assert (le : (ending <=? stamp) = false) by (apply Nat.leb_gt; lia).
    assert (neq : (ending =? stamp) = false) by (apply Nat.eqb_neq; assumption).
    destruct prefer_inner; unfold cached_due, extend_endpoint_cache;
      rewrite Nat.min_r by lia; rewrite ?lt, ?le, ?Nat.eqb_refl, ?neq; reflexivity.
  - assert (head_neq : (ending =? stamp) = false) by (apply Nat.eqb_neq; assumption).
    assert (tail_neq : (other =? stamp) = false) by (apply Nat.eqb_neq; assumption).
    assert (minimum_neq : (Nat.min ending other =? stamp) = false).
    { apply Nat.eqb_neq. destruct (le_dec ending other); [rewrite Nat.min_l by lia|rewrite Nat.min_r by lia]; lia. }
    destruct prefer_inner; unfold cached_due, extend_endpoint_cache; cbn;
      rewrite minimum_neq, head_neq, tail_neq; reflexivity.
Qed.

Lemma computed_cache_selects_exact_scan prefer_inner stamp endings :
  Forall (fun ending => stamp <= ending) endings ->
  cached_due stamp (compute_endpoint_cache prefer_inner endings) = scan_due prefer_inner stamp endings.
Proof.
  induction endings as [|first rest IH]; intro bounds; cbn; [reflexivity|].
  inversion bounds; subst. rewrite cached_due_extends_exact_scan; [now rewrite IH|assumption|].
  intros other index found. eapply computed_cache_has_lower_bound; eauto.
Qed.

Record owner_caches := OwnerCaches {
  any_boundary_cache : endpoint_cache;
  path_boundary_cache : endpoint_cache
}.

Record scheduled_owner := ScheduledOwner {
  scheduled_value : cached_owner;
  scheduled_caches : owner_caches
}.

Definition top_caches (owners : list scheduled_owner) :=
  match owners with [] => OwnerCaches None None | first :: _ => scheduled_caches first end.

Definition extend_owner_caches current rest :=
  OwnerCaches
    (extend_endpoint_cache false (owner_bound (owner_value current)) (any_boundary_cache rest))
    (extend_endpoint_cache true (owner_path_end (owner_value current)) (path_boundary_cache rest)).

Definition push_scheduled current owners :=
  ScheduledOwner current (extend_owner_caches current (top_caches owners)) :: owners.

Fixpoint schedule_owners (owners : list cached_owner) : list scheduled_owner :=
  match owners with [] => [] | first :: rest => push_scheduled first (schedule_owners rest) end.

Fixpoint scheduled_valid (owners : list scheduled_owner) : Prop :=
  match owners with
  | [] => True
  | first :: rest => scheduled_caches first = extend_owner_caches (scheduled_value first) (top_caches rest) /\
      scheduled_valid rest
  end.

Definition scheduled_endpoints owners :=
  map (fun current => owner_path_end (owner_value (scheduled_value current))) owners.
Definition scheduled_bounds owners :=
  map (fun current => owner_bound (owner_value (scheduled_value current))) owners.

Lemma valid_cache_projection owners :
  scheduled_valid owners ->
  any_boundary_cache (top_caches owners) = compute_endpoint_cache false (scheduled_bounds owners) /\
  path_boundary_cache (top_caches owners) = compute_endpoint_cache true (scheduled_endpoints owners).
Proof.
  induction owners as [|first rest IH]; intro valid; cbn in *; [auto|].
  destruct valid as [head tail]. destruct (IH tail) as [any paths]. rewrite head; cbn. now rewrite any, paths.
Qed.

Theorem both_prefix_caches_preserve_push_pop_and_opposite_tie_rules stamp owners :
  scheduled_valid owners ->
  Forall (fun bound => stamp <= bound) (scheduled_bounds owners) ->
  Forall (fun ending => stamp <= ending) (scheduled_endpoints owners) ->
  cached_due stamp (any_boundary_cache (top_caches owners)) = scan_due false stamp (scheduled_bounds owners) /\
  cached_due stamp (path_boundary_cache (top_caches owners)) = scan_due true stamp (scheduled_endpoints owners) /\
  (forall current, scheduled_valid (push_scheduled current owners) /\ tl (push_scheduled current owners) = owners).
Proof.
  intros valid bounds endpoints. destruct (valid_cache_projection owners valid) as [any paths].
  rewrite any, paths, !computed_cache_selects_exact_scan by assumption. repeat split; auto.
Qed.

Lemma schedule_owners_valid owners : scheduled_valid (schedule_owners owners).
Proof. induction owners; cbn; auto. Qed.

Inductive boundary_error := BoundaryTruncated (source offset : nat) | BoundaryRefusal (code source offset : nat).
Inductive boundary_view :=
| BoundaryDead (error : boundary_error)
| BoundaryPending (id : nat)
| BoundaryUnfinished
| BoundaryCompleted (current : leaf).

Record boundary_observation := BoundaryObservation {
  boundary_result : boundary_view;
  boundary_pops : nat;
  boundary_calls : list nat
}.

Definition unchanged_boundary view := BoundaryObservation view 0 [].
Definition note_call slot popped result :=
  BoundaryObservation (boundary_result result) (popped + boundary_pops result) (slot :: boundary_calls result).

Definition owner_due stamp current := owner_path_end (owner_value (scheduled_value current)) =? stamp.

Fixpoint eager_unwind
    (classify : leaf -> boundary_view)
    (complete : region_metadata -> nat -> leaf -> completion_result)
    stamp source offset view (owners : list scheduled_owner) : boundary_observation :=
  match owners with
  | [] => unchanged_boundary view
  | first :: rest =>
      let current := owner_value (scheduled_value first) in
      match view with
      | BoundaryDead error => eager_unwind classify complete stamp source offset (BoundaryDead error) rest
      | BoundaryPending id => eager_unwind classify complete stamp source offset (BoundaryPending id) rest
      | BoundaryUnfinished => eager_unwind classify complete stamp source offset
          (if owner_due stamp first then BoundaryDead (BoundaryTruncated source offset) else BoundaryUnfinished) rest
      | BoundaryCompleted child =>
          if owner_due stamp first then
            match complete (owner_metadata current) (owner_region_end current - stamp) child with
            | CompletedOwner next => note_call (region_slot (owner_metadata current)) 1
                (eager_unwind classify complete stamp source offset (classify next) rest)
            | RefusedOwner code at_source at_offset => note_call (region_slot (owner_metadata current)) 0
                (eager_unwind classify complete stamp source offset
                  (BoundaryDead (BoundaryRefusal code at_source at_offset)) rest)
            end
          else eager_unwind classify complete stamp source offset BoundaryUnfinished rest
      end
  end.

Fixpoint deepest_due_settlement
    (classify : leaf -> boundary_view)
    (complete : region_metadata -> nat -> leaf -> completion_result)
    stamp source offset view (owners : list scheduled_owner) : boundary_observation :=
  match view with
  | BoundaryDead error => unchanged_boundary (BoundaryDead error)
  | BoundaryPending id => unchanged_boundary (BoundaryPending id)
  | _ =>
      match owners with
      | [] => unchanged_boundary view
      | first :: rest =>
          match cached_due stamp (path_boundary_cache (top_caches owners)) with
          | None => unchanged_boundary BoundaryUnfinished
          | Some (S _) => unchanged_boundary (BoundaryDead (BoundaryTruncated source offset))
          | Some 0 =>
              match view with
              | BoundaryCompleted child =>
                  let current := owner_value (scheduled_value first) in
                  match complete (owner_metadata current) (owner_region_end current - stamp) child with
                  | CompletedOwner next => note_call (region_slot (owner_metadata current)) 1
                      (deepest_due_settlement classify complete stamp source offset (classify next) rest)
                  | RefusedOwner code at_source at_offset => note_call (region_slot (owner_metadata current)) 0
                      (unchanged_boundary (BoundaryDead (BoundaryRefusal code at_source at_offset)))
                  end
              | _ => unchanged_boundary (BoundaryDead (BoundaryTruncated source offset))
              end
          end
      end
  end.

Lemma eager_dead_is_exact classify complete stamp source offset error owners :
  eager_unwind classify complete stamp source offset (BoundaryDead error) owners = unchanged_boundary (BoundaryDead error).
Proof. induction owners; cbn; auto. Qed.

Lemma eager_pending_is_exact classify complete stamp source offset id owners :
  eager_unwind classify complete stamp source offset (BoundaryPending id) owners = unchanged_boundary (BoundaryPending id).
Proof. induction owners; cbn; auto. Qed.

Lemma eager_unfinished_scans_for_outer_boundary classify complete stamp source offset owners :
  eager_unwind classify complete stamp source offset BoundaryUnfinished owners =
  unchanged_boundary (match scan_due true stamp (scheduled_endpoints owners) with
    | Some _ => BoundaryDead (BoundaryTruncated source offset) | None => BoundaryUnfinished end).
Proof.
  induction owners as [|first rest IH]; cbn; [reflexivity|].
  unfold owner_due. destruct (owner_path_end (owner_value (scheduled_value first)) =? stamp).
  - apply eager_dead_is_exact.
  - rewrite IH. unfold scheduled_endpoints. destruct (scan_due true stamp
      (map (fun current => owner_path_end (owner_value (scheduled_value current))) rest)); reflexivity.
Qed.

Lemma valid_path_cache_due stamp owners :
  scheduled_valid owners -> Forall (fun ending => stamp <= ending) (scheduled_endpoints owners) ->
  cached_due stamp (path_boundary_cache (top_caches owners)) = scan_due true stamp (scheduled_endpoints owners).
Proof.
  intros valid bounds. destruct (valid_cache_projection owners valid) as [_ paths]. rewrite paths.
  now apply computed_cache_selects_exact_scan.
Qed.

Theorem deepest_due_settlement_equals_eager_unwind_under_exact_local_completion
    classify complete stamp source offset view owners :
  scheduled_valid owners -> Forall (fun ending => stamp <= ending) (scheduled_endpoints owners) ->
  deepest_due_settlement classify complete stamp source offset view owners =
    eager_unwind classify complete stamp source offset view owners.
Proof.
  revert view. induction owners as [|first rest IH]; intros view valid bounds.
  - now destruct view.
  - destruct valid as [cache valid]. inversion bounds; subst.
    assert (due : cached_due stamp (path_boundary_cache (top_caches (first :: rest))) =
      scan_due true stamp (scheduled_endpoints (first :: rest))).
    { apply valid_path_cache_due; cbn; auto. }
    destruct view as [error|id| |child].
    + cbn. symmetry. apply eager_dead_is_exact.
    + cbn. symmetry. apply eager_pending_is_exact.
    + rewrite eager_unfinished_scans_for_outer_boundary. cbn [deepest_due_settlement].
      rewrite due. destruct (scan_due true stamp (scheduled_endpoints (first :: rest))) as [[|index]|]; reflexivity.
    + cbn [deepest_due_settlement]. rewrite due. unfold scheduled_endpoints. cbn [map scan_due].
      cbn [eager_unwind]. unfold owner_due.
      destruct (owner_path_end (owner_value (scheduled_value first)) =? stamp) eqn:boundary.
      * destruct (complete (owner_metadata (owner_value (scheduled_value first)))
          (owner_region_end (owner_value (scheduled_value first)) - stamp) child) as [next|code at_source at_offset].
        -- rewrite IH by assumption. reflexivity.
        -- rewrite eager_dead_is_exact. reflexivity.
      * rewrite eager_unfinished_scans_for_outer_boundary.
        unfold scheduled_endpoints. destruct (scan_due true stamp
          (map (fun current => owner_path_end (owner_value (scheduled_value current))) rest)); reflexivity.
Qed.

Definition local_completion_binding
    (left right : region_metadata -> nat -> leaf -> completion_result) :=
  forall metadata remaining child, left metadata remaining child = right metadata remaining child.

Lemma eager_local_completion_ext classify left right stamp source offset view owners :
  local_completion_binding left right ->
  eager_unwind classify left stamp source offset view owners = eager_unwind classify right stamp source offset view owners.
Proof.
  intro binding. revert view. induction owners as [|first rest IH]; intro view; cbn; [reflexivity|].
  destruct view; try apply IH.
  destruct (owner_due stamp first); [rewrite binding|apply IH].
  destruct (right (owner_metadata (owner_value (scheduled_value first)))
    (owner_region_end (owner_value (scheduled_value first)) - stamp) current); now rewrite IH.
Qed.

Theorem settlement_refinement_needs_only_pointwise_local_completion_authority
    classify shared modeled stamp source offset view owners :
  scheduled_valid owners -> Forall (fun ending => stamp <= ending) (scheduled_endpoints owners) ->
  local_completion_binding shared modeled ->
  deepest_due_settlement classify modeled stamp source offset view owners =
    eager_unwind classify shared stamp source offset view owners.
Proof.
  intros valid bounds binding. rewrite deepest_due_settlement_equals_eager_unwind_under_exact_local_completion by assumption.
  symmetry. now apply eager_local_completion_ext.
Qed.

Theorem pending_zero_boundaries_and_existing_dead_errors_do_not_invoke_completion
    classify complete stamp source offset owners id error :
  deepest_due_settlement classify complete stamp source offset (BoundaryPending id) owners =
    unchanged_boundary (BoundaryPending id) /\
  deepest_due_settlement classify complete stamp source offset (BoundaryDead error) owners =
    unchanged_boundary (BoundaryDead error).
Proof. destruct owners; split; reflexivity. Qed.

Theorem skipped_unfinished_inner_owners_refuse_at_the_current_endpoint
    classify complete stamp source offset first rest index child :
  cached_due stamp (path_boundary_cache (top_caches (first :: rest))) = Some (S index) ->
  deepest_due_settlement classify complete stamp source offset (BoundaryCompleted child) (first :: rest) =
    unchanged_boundary (BoundaryDead (BoundaryTruncated source offset)).
Proof. intro due. cbn [deepest_due_settlement]. now rewrite due. Qed.

Fixpoint settlement_owner_checks classify complete stamp (source offset : nat) view (owners : list scheduled_owner) : nat :=
  match view with
  | BoundaryDead _ | BoundaryPending _ => 0
  | _ => match owners with
    | [] => 0
    | first :: rest =>
      match cached_due stamp (path_boundary_cache (top_caches owners)), view with
      | Some 0, BoundaryCompleted child =>
        match complete (owner_metadata (owner_value (scheduled_value first)))
          (owner_region_end (owner_value (scheduled_value first)) - stamp) child with
        | CompletedOwner next => S (settlement_owner_checks classify complete stamp source offset (classify next) rest)
        | RefusedOwner _ _ _ => 1
        end
      | _, _ => 1
      end
    end
  end.

Theorem focused_settlement_checks_are_bounded_by_successful_pops_plus_one
    classify complete stamp source offset view owners :
  settlement_owner_checks classify complete stamp source offset view owners <=
    S (boundary_pops (deepest_due_settlement classify complete stamp source offset view owners)).
Proof.
  revert view. induction owners as [|first rest IH]; intro view; destruct view; cbn; try lia.
  - destruct (cached_due stamp (path_boundary_cache (scheduled_caches first))) as [[|index]|]; cbn; lia.
  - destruct (cached_due stamp (path_boundary_cache (scheduled_caches first))) as [[|index]|]; cbn; try lia.
    destruct (complete (owner_metadata (owner_value (scheduled_value first)))
      (owner_region_end (owner_value (scheduled_value first)) - stamp) current); cbn; [specialize (IH (classify current0)); lia|lia].
Qed.

Definition boundary_error_fields error :=
  match error with
  | BoundaryTruncated source offset => (0, source, offset)
  | BoundaryRefusal code source offset => (S code, source, offset)
  end.

Inductive boundary_request :=
| BoundaryReturnedError (error : boundary_error)
| BoundaryAccepted (view : boundary_view) (event clock : nat).

Definition deliver_boundary
    (settle : boundary_view -> boundary_observation)
    before candidate before_consumed after_consumed before_clock request :=
  match request with
  | BoundaryReturnedError error =>
      (Some error, None, World (FocusedRepresentation before) before_consumed before_clock)
  | BoundaryAccepted view event clock =>
      let result := settle view in
      let choice := match boundary_result result with
        | BoundaryDead error => let '(code, source, offset) := boundary_error_fields error in DeferredRefusal code source offset
        | _ => AcceptedStep
        end in
      (None, Some event, select_transaction choice before candidate before_consumed after_consumed before_clock clock)
  end.

Theorem settlement_preserves_exact_returned_error_public_event_clock_and_selected_world
    classify complete stamp source offset owners before candidate before_consumed after_consumed before_clock request :
  scheduled_valid owners -> Forall (fun ending => stamp <= ending) (scheduled_endpoints owners) ->
  deliver_boundary (fun view => deepest_due_settlement classify complete stamp source offset view owners)
    before candidate before_consumed after_consumed before_clock request =
  deliver_boundary (fun view => eager_unwind classify complete stamp source offset view owners)
    before candidate before_consumed after_consumed before_clock request.
Proof.
  intros valid bounds. destruct request; cbn [deliver_boundary]; [reflexivity|].
  now rewrite deepest_due_settlement_equals_eager_unwind_under_exact_local_completion by assumption.
Qed.

Theorem refusal_after_successful_pops_restores_immediate_pre_step_graph_and_keeps_event_clock
    settle before candidate before_consumed after_consumed before_clock view event clock error first rest popped calls :
  focused_owners before = first :: rest ->
  settle view = BoundaryObservation (BoundaryDead error) (S popped) calls ->
  let '(code, source, offset) := boundary_error_fields error in
  deliver_boundary settle before candidate before_consumed after_consumed before_clock (BoundaryAccepted view event clock) =
    (None, Some event, World (ReifiedRepresentation (kill_graph code source offset (focused_graph before))) after_consumed clock).
Proof.
  intros owners refused. unfold deliver_boundary. rewrite refused; cbn.
  destruct error; cbn; now rewrite owners.
Qed.

Definition settlement_control_owner slot region_end path_end :=
  CachedOwner (Owner (control_metadata slot) region_end path_end) None.
Definition completed_control_view (_ : leaf) := BoundaryCompleted control_leaf.
Definition settlement_control_complete metadata (remaining : nat) (child : leaf) :=
  if region_slot metadata =? 1 then RefusedOwner 77 8 91 else CompletedOwner control_leaf.
Definition equal_control_spine := schedule_owners
  [settlement_control_owner 3 9 9; settlement_control_owner 2 9 9; settlement_control_owner 1 12 9].
Definition unfinished_control_spine := schedule_owners
  [settlement_control_owner 2 12 12; settlement_control_owner 1 12 9].

Theorem boundary_scheduler_equal_endpoints_pending_and_rollback_positive_controls :
  cached_due 9 (any_boundary_cache (top_caches equal_control_spine)) = Some 2 /\
  cached_due 9 (path_boundary_cache (top_caches equal_control_spine)) = Some 0 /\
  deepest_due_settlement completed_control_view settlement_control_complete 9 2 31
    (BoundaryCompleted control_leaf) equal_control_spine =
    BoundaryObservation (BoundaryDead (BoundaryRefusal 77 8 91)) 2 [3; 2; 1] /\
  eager_unwind completed_control_view settlement_control_complete 9 2 31
    (BoundaryCompleted control_leaf) equal_control_spine =
    BoundaryObservation (BoundaryDead (BoundaryRefusal 77 8 91)) 2 [3; 2; 1] /\
  deepest_due_settlement completed_control_view settlement_control_complete 9 2 31
    (BoundaryPending 17) equal_control_spine = unchanged_boundary (BoundaryPending 17) /\
  deliver_boundary (fun view => deepest_due_settlement completed_control_view settlement_control_complete 9 2 31 view equal_control_spine)
    control_focused (Focused 4 control_leaf control_owners) 3 4 18
    (BoundaryAccepted (BoundaryCompleted control_leaf) 71 19) =
    (None, Some 71, World (ReifiedRepresentation (kill_graph 78 8 91 (focused_graph control_focused))) 4 19).
Proof. repeat split; reflexivity. Qed.

Theorem boundary_scheduler_causal_controls_reject_wrong_ties_gap_completion_and_region_only_expiry :
  cached_due 9 (any_boundary_cache (top_caches equal_control_spine)) <>
    cached_due 9 (path_boundary_cache (top_caches equal_control_spine)) /\
  cached_due 9 (path_boundary_cache (top_caches unfinished_control_spine)) = Some 1 /\
  deepest_due_settlement completed_control_view settlement_control_complete 9 2 31
    (BoundaryCompleted control_leaf) unfinished_control_spine =
    unchanged_boundary (BoundaryDead (BoundaryTruncated 2 31)) /\
  deepest_due_settlement completed_control_view settlement_control_complete 9 2 31
    (BoundaryDead (BoundaryRefusal 255 7 64)) unfinished_control_spine =
    unchanged_boundary (BoundaryDead (BoundaryRefusal 255 7 64)) /\
  let region_only := schedule_owners [settlement_control_owner 2 9 11] in
  cached_due 9 (any_boundary_cache (top_caches region_only)) = Some 0 /\
  cached_due 9 (path_boundary_cache (top_caches region_only)) = None /\
  deepest_due_settlement completed_control_view settlement_control_complete 9 2 31
    (BoundaryCompleted control_leaf) region_only = unchanged_boundary BoundaryUnfinished.
Proof. repeat split; vm_compute; congruence. Qed.

Print Assumptions both_prefix_caches_preserve_push_pop_and_opposite_tie_rules.
Print Assumptions deepest_due_settlement_equals_eager_unwind_under_exact_local_completion.
Print Assumptions settlement_refinement_needs_only_pointwise_local_completion_authority.
Print Assumptions pending_zero_boundaries_and_existing_dead_errors_do_not_invoke_completion.
Print Assumptions skipped_unfinished_inner_owners_refuse_at_the_current_endpoint.
Print Assumptions focused_settlement_checks_are_bounded_by_successful_pops_plus_one.
Print Assumptions settlement_preserves_exact_returned_error_public_event_clock_and_selected_world.
Print Assumptions refusal_after_successful_pops_restores_immediate_pre_step_graph_and_keeps_event_clock.
Print Assumptions boundary_scheduler_equal_endpoints_pending_and_rollback_positive_controls.
Print Assumptions boundary_scheduler_causal_controls_reject_wrong_ties_gap_completion_and_region_only_expiry.

End FocusedRegionOwners.

Module PersistentFocusedOwners.

Module F := FocusedRegionOwners.

Definition boundary_handle := option (nat * nat).

Definition choose_boundary (prefer_new : bool) (ending index : nat) (previous : boundary_handle) : boundary_handle :=
  match previous with
  | None => Some (ending, index)
  | Some (old_end, old_index) =>
      if (if prefer_new then ending <=? old_end else ending <? old_end)
      then Some (ending, index) else Some (old_end, old_index)
  end.

Record owner_cell := OwnerCell {
  cell_owner : F.owner;
  cell_parent : option nat;
  cell_all_boundary : boundary_handle;
  cell_path_boundary : boundary_handle;
  cell_outermost : nat
}.

Definition owner_arena := list owner_cell.
Definition owner_entries := list (nat * owner_cell).

Definition lookup (arena : owner_arena) (root : option nat) :=
  match root with None => None | Some index => nth_error arena index end.

Definition read_all arena root :=
  match lookup arena root with None => None | Some current => cell_all_boundary current end.
Definition read_path arena root :=
  match lookup arena root with None => None | Some current => cell_path_boundary current end.
Definition read_outer arena root :=
  match lookup arena root with None => None | Some current => Some (cell_outermost current) end.

Fixpoint reference_all (entries : owner_entries) : boundary_handle :=
  match entries with
  | [] => None
  | (index, current) :: rest =>
      choose_boundary false (F.owner_bound (cell_owner current)) index (reference_all rest)
  end.

Fixpoint reference_path (entries : owner_entries) : boundary_handle :=
  match entries with
  | [] => None
  | (index, current) :: rest =>
      choose_boundary true (F.owner_path_end (cell_owner current)) index (reference_path rest)
  end.

Fixpoint reference_outer (entries : owner_entries) : option nat :=
  match entries with
  | [] => None
  | (index, _) :: rest =>
      match reference_outer rest with None => Some index | Some outer => Some outer end
  end.

Inductive linked (arena : owner_arena) : option nat -> owner_entries -> Prop :=
| linked_empty : linked arena None []
| linked_cons index current rest :
    nth_error arena index = Some current ->
    linked arena (cell_parent current) rest ->
    (forall parent, cell_parent current = Some parent -> parent < index) ->
    cell_all_boundary current =
      choose_boundary false (F.owner_bound (cell_owner current)) index (reference_all rest) ->
    cell_path_boundary current =
      choose_boundary true (F.owner_path_end (cell_owner current)) index (reference_path rest) ->
    Some (cell_outermost current) =
      match reference_outer rest with None => Some index | Some outer => Some outer end ->
    linked arena (Some index) ((index, current) :: rest).

Lemma old_lookup_after_append {A : Type} (arena suffix : list A) index value :
  nth_error arena index = Some value -> nth_error (arena ++ suffix) index = Some value.
Proof.
  intro found. rewrite nth_error_app1; [exact found|].
  apply nth_error_Some. rewrite found. discriminate.
Qed.

Theorem immutable_extension_preserves_every_linked_checkpoint arena suffix root entries :
  linked arena root entries -> linked (arena ++ suffix) root entries.
Proof.
  intro valid. induction valid; constructor; auto.
  eapply old_lookup_after_append; eauto.
Qed.

Lemma linked_root_in_bounds arena index entries :
  linked arena (Some index) entries -> index < length arena.
Proof.
  intro valid. inversion valid; subst. apply nth_error_Some. congruence.
Qed.

Theorem cached_linked_reads_refine_exact_list_selection arena root entries :
  linked arena root entries ->
  read_all arena root = reference_all entries /\
  read_path arena root = reference_path entries /\
  read_outer arena root = reference_outer entries.
Proof.
  intro valid. destruct valid; [repeat split; reflexivity|].
  unfold read_all, read_path, read_outer, lookup. rewrite H. cbn. auto.
Qed.

Definition make_cell arena parent value :=
  let index := length arena in
  OwnerCell value parent
    (choose_boundary false (F.owner_bound value) index (read_all arena parent))
    (choose_boundary true (F.owner_path_end value) index (read_path arena parent))
    (match read_outer arena parent with None => index | Some outer => outer end).

Theorem append_owner_constructs_valid_backward_links_and_exact_caches arena root entries value :
  linked arena root entries ->
  let cell := make_cell arena root value in
  linked (arena ++ [cell]) (Some (length arena)) ((length arena, cell) :: entries).
Proof.
  intro valid. cbn zeta. apply linked_cons.
  - rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. reflexivity.
  - cbn. apply immutable_extension_preserves_every_linked_checkpoint. exact valid.
  - cbn. intros parent same. subst root. eapply linked_root_in_bounds; eauto.
  - cbn. destruct (cached_linked_reads_refine_exact_list_selection valid) as [all _]. now rewrite all.
  - cbn. destruct (cached_linked_reads_refine_exact_list_selection valid) as [_ [paths _]]. now rewrite paths.
  - cbn. destruct (cached_linked_reads_refine_exact_list_selection valid) as [_ [_ outer]].
    rewrite outer. now destruct (reference_outer entries).
Qed.

Definition projected_owners (entries : owner_entries) :=
  map (fun entry => F.CachedOwner (cell_owner (snd entry)) None) entries.

Definition projected_graph stamp focus entries :=
  F.reify stamp (F.LeafGraph focus) (projected_owners entries).

Definition observes arena root stamp focus graph :=
  exists entries, linked arena root entries /\ graph = projected_graph stamp focus entries.

Theorem extension_preserves_exact_reified_graph_and_old_frame_lookups
    {FrameValue : Type} arena extra root graph stamp focus
    (frames more_frames : list FrameValue) frame_index frame_value :
  observes arena root stamp focus graph -> nth_error frames frame_index = Some frame_value ->
  observes (arena ++ extra) root stamp focus graph /\
  nth_error (frames ++ more_frames) frame_index = Some frame_value.
Proof.
  intros [entries [owners observed]] frame. split.
  - exists entries. split; [|exact observed].
    apply immutable_extension_preserves_every_linked_checkpoint. exact owners.
  - eapply old_lookup_after_append; eauto.
Qed.

Theorem pop_restores_the_exact_preexisting_prefix arena root entries value :
  linked arena root entries ->
  let cell := make_cell arena root value in
  cell_parent cell = root /\
  linked (arena ++ [cell]) (cell_parent cell) entries /\
  read_all (arena ++ [cell]) (cell_parent cell) = reference_all entries /\
  read_path (arena ++ [cell]) (cell_parent cell) = reference_path entries /\
  read_outer (arena ++ [cell]) (cell_parent cell) = reference_outer entries.
Proof.
  intro valid. cbn zeta. split; [reflexivity|].
  assert (preserved : linked (arena ++ [make_cell arena root value]) root entries).
  { apply immutable_extension_preserves_every_linked_checkpoint. exact valid. }
  split; [exact preserved|]. apply cached_linked_reads_refine_exact_list_selection. exact preserved.
Qed.

Record focused_checkpoint := FocusedCheckpoint {
  checkpoint_focus : F.leaf;
  checkpoint_owners : option nat;
  checkpoint_stamp : nat;
  checkpoint_bytes : list CanonicalKeyProtocol.located_byte
}.

Record history := History {
  history_states : list focused_checkpoint;
  history_clock : nat
}.

Definition restore (store : history) index := nth_error (history_states store) index.
Definition publish (store : history) next next_clock :=
  History (history_states store ++ [next]) next_clock.

Theorem publishing_a_sibling_preserves_every_old_stamp_focus_and_byte_history
    store index previous next next_clock :
  restore store index = Some previous ->
  restore (publish store next next_clock) index = Some previous.
Proof. unfold restore, publish; cbn. apply old_lookup_after_append. Qed.

Definition allocate_obligation store :=
  (history_clock store, History (history_states store) (S (history_clock store))).

Theorem historical_restore_never_rewinds_the_global_obligation_clock store index :
  let '(first, after_first) := allocate_obligation store in
  let restored := restore after_first index in
  let '(second, after_second) := allocate_obligation after_first in
  restored = restore store index /\ first < second /\
  history_states after_second = history_states store /\
  history_clock after_second = history_clock store + 2.
Proof. cbn. repeat split; auto; lia. Qed.

Theorem equal_endpoint_caches_use_opposite_owner_handles ending inner outer :
  choose_boundary false ending inner (Some (ending, outer)) = Some (ending, outer) /\
  choose_boundary true ending inner (Some (ending, outer)) = Some (ending, inner).
Proof. unfold choose_boundary. rewrite Nat.ltb_irrefl, Nat.leb_refl. auto. Qed.

Print Assumptions immutable_extension_preserves_every_linked_checkpoint.
Print Assumptions cached_linked_reads_refine_exact_list_selection.
Print Assumptions append_owner_constructs_valid_backward_links_and_exact_caches.
Print Assumptions extension_preserves_exact_reified_graph_and_old_frame_lookups.
Print Assumptions pop_restores_the_exact_preexisting_prefix.
Print Assumptions publishing_a_sibling_preserves_every_old_stamp_focus_and_byte_history.
Print Assumptions historical_restore_never_rewinds_the_global_obligation_clock.
Print Assumptions equal_endpoint_caches_use_opposite_owner_handles.

Record fault_error := FaultError {
  fault_code : nat;
  fault_source : nat;
  fault_offset : nat
}.

Definition dead_status error := F.DeadPath (fault_code error) (fault_source error) (fault_offset error).

Definition kill_metadata (metadata : F.path_metadata) error :=
  F.PathMetadata (F.path_escape_allowed metadata) (dead_status error)
    (F.path_completed_segments metadata) (F.path_first_tag metadata) (F.path_stable metadata).

Definition kill_root (graph : F.path_graph) error :=
  match graph with
  | F.LeafGraph current =>
      F.LeafGraph (F.Leaf (kill_metadata (F.leaf_metadata current) error) (F.leaf_frames current))
  | F.RegionGraph metadata region_remaining path_remaining child =>
      F.RegionGraph
        (F.RegionMetadata (F.region_slot metadata) (kill_metadata (F.region_owner metadata) error)
          (F.region_previous metadata) (F.region_declared_length metadata)
          (F.region_start_depth metadata) (F.region_next metadata))
        region_remaining path_remaining child
  end.

Definition root_metadata (graph : F.path_graph) :=
  match graph with
  | F.LeafGraph current => F.leaf_metadata current
  | F.RegionGraph metadata _ _ _ => F.region_owner metadata
  end.

Lemma killed_root_has_exact_error graph error :
  F.path_status_value (root_metadata (kill_root graph error)) = dead_status error.
Proof. destruct graph; reflexivity. Qed.

Fixpoint propagate_dead_from_hidden_leaf (graph : F.path_graph) error :=
  match graph with
  | F.LeafGraph _ => kill_root graph error
  | F.RegionGraph _ _ _ child =>
      match F.path_status_value (root_metadata (propagate_dead_from_hidden_leaf child error)) with
      | F.DeadPath code source offset => kill_root graph (FaultError code source offset)
      | _ => graph
      end
  end.

Theorem refusing_hidden_leaf_replaces_only_the_original_root_error graph error :
  propagate_dead_from_hidden_leaf graph error = kill_root graph error.
Proof.
  induction graph as [current|metadata region_remaining path_remaining child IH]; cbn; [reflexivity|].
  rewrite IH, killed_root_has_exact_error. destruct error; reflexivity.
Qed.

Theorem root_error_replacement_preserves_all_slots_and_last_error graph first second :
  F.graph_slots (kill_root graph first) = F.graph_slots graph /\
  kill_root (kill_root graph first) second = kill_root graph second.
Proof. destruct graph; repeat split; reflexivity. Qed.

Inductive terminal_view :=
| TerminalOther
| TerminalObligation (id source offset : nat) (awaiting : bool).

Definition unresolved := (nat * nat)%type.

Definition resolve_preflight requested terminal : option unresolved :=
  match terminal with
  | TerminalOther => Some (0, 0)
  | TerminalObligation id source offset awaiting =>
      if (requested =? id) && awaiting then None else Some (source, offset)
  end.

Definition reject_preflight requested caller terminal : option unresolved :=
  match terminal with
  | TerminalOther => Some caller
  | TerminalObligation id source offset _ =>
      if requested =? id then None else Some (source, offset)
  end.

Theorem resolve_and_reject_preflight_preserve_their_distinct_phase_and_source_rules
    id source offset caller :
  resolve_preflight id (TerminalObligation id source offset false) = Some (source, offset) /\
  reject_preflight id caller (TerminalObligation id source offset false) = None /\
  resolve_preflight id (TerminalObligation id source offset true) = None /\
  reject_preflight id caller (TerminalObligation id source offset true) = None /\
  resolve_preflight id TerminalOther = Some (0, 0) /\
  reject_preflight id caller TerminalOther = Some caller.
Proof. unfold resolve_preflight, reject_preflight. rewrite Nat.eqb_refl. repeat split; reflexivity. Qed.

Theorem wrong_id_returns_the_hidden_tag_source_for_both_operations
    requested actual source offset awaiting caller :
  requested <> actual ->
  resolve_preflight requested (TerminalObligation actual source offset awaiting) = Some (source, offset) /\
  reject_preflight requested caller (TerminalObligation actual source offset awaiting) = Some (source, offset).
Proof.
  intro different. unfold resolve_preflight, reject_preflight.
  assert (wrong : (requested =? actual) = false) by (apply Nat.eqb_neq; exact different).
  rewrite wrong. auto.
Qed.

Record fault_state := FaultState {
  fault_saved : focused_checkpoint;
  fault_selected_entries : owner_entries;
  fault_current_bytes : list CanonicalKeyProtocol.located_byte;
  fault_selected_error : fault_error
}.

Definition fault_graph current :=
  kill_root (projected_graph (checkpoint_stamp (fault_saved current))
    (checkpoint_focus (fault_saved current)) (fault_selected_entries current))
    (fault_selected_error current).

Definition fault_edge current byte :=
  FaultState (fault_saved current) (fault_selected_entries current)
    (fault_current_bytes current ++ [byte]) (fault_selected_error current).

Definition visible_fault (has_value : bool)
    (live_observation : option nat * bool * option fault_error) graph :=
  match F.path_status_value (root_metadata graph) with
  | F.DeadPath code source offset =>
      (None, false, if has_value then Some (FaultError code source offset) else None)
  | _ => live_observation
  end.

Theorem focused_fault_observations_match_the_exact_root_dead_branches current has_value live_observation :
  visible_fault has_value live_observation (fault_graph current) =
    (None, false, if has_value then Some (fault_selected_error current) else None).
Proof.
  unfold visible_fault, fault_graph. rewrite killed_root_has_exact_error.
  destruct (fault_selected_error current); reflexivity.
Qed.

Record fault_world := FaultWorld {
  world_current : fault_state;
  world_history : list fault_state;
  world_clock : nat
}.

Definition dead_step world byte :=
  let next := fault_edge (world_current world) byte in
  FaultWorld next (world_history world ++ [next]) (world_clock world).

Theorem dead_edge_preserves_selected_graph_stamp_and_clock_while_appending_current_bytes
    world byte :
  let before := world_current world in
  let after := world_current (dead_step world byte) in
  fault_graph after = fault_graph before /\
  checkpoint_stamp (fault_saved after) = checkpoint_stamp (fault_saved before) /\
  fault_current_bytes after = fault_current_bytes before ++ [byte] /\
  world_clock (dead_step world byte) = world_clock world /\
  length (world_history (dead_step world byte)) = S (length (world_history world)).
Proof. cbn. repeat split; auto. rewrite length_app. cbn. lia. Qed.

Definition force_fault current := (fault_graph current, fault_current_bytes current).

Theorem fault_forcing_uses_saved_graph_stamp_but_current_suffix_bytes current byte :
  force_fault (fault_edge current byte) = (fault_graph current, fault_current_bytes current ++ [byte]).
Proof. reflexivity. Qed.

Theorem fault_preflight_causal_controls_distinguish_payload_and_rolled_back_events :
  resolve_preflight 3 (TerminalObligation 3 7 11 false) = Some (7, 11) /\
  reject_preflight 3 (8, 19) (TerminalObligation 3 7 11 false) = None /\
  resolve_preflight 4 TerminalOther <> resolve_preflight 4 (TerminalObligation 4 7 13 true) /\
  reject_preflight 4 (8, 19) TerminalOther <> reject_preflight 4 (8, 19) (TerminalObligation 4 7 13 true).
Proof. repeat split; cbn; congruence. Qed.

Print Assumptions refusing_hidden_leaf_replaces_only_the_original_root_error.
Print Assumptions root_error_replacement_preserves_all_slots_and_last_error.
Print Assumptions resolve_and_reject_preflight_preserve_their_distinct_phase_and_source_rules.
Print Assumptions wrong_id_returns_the_hidden_tag_source_for_both_operations.
Print Assumptions focused_fault_observations_match_the_exact_root_dead_branches.
Print Assumptions dead_edge_preserves_selected_graph_stamp_and_clock_while_appending_current_bytes.
Print Assumptions fault_forcing_uses_saved_graph_stamp_but_current_suffix_bytes.
Print Assumptions fault_preflight_causal_controls_distinguish_payload_and_rolled_back_events.

Definition cache_matches (entries : owner_entries)
    (address_cache : boundary_handle) (position_cache : F.endpoint_cache) : Prop :=
  match address_cache, position_cache with
  | None, None => True
  | Some (ending, address), Some (same_end, position) =>
      ending = same_end /\ exists cell, nth_error entries position = Some (address, cell)
  | _, _ => False
  end.

Fixpoint reference_boundary prefer_new (endpoint : owner_cell -> nat) (entries : owner_entries) :=
  match entries with
  | [] => None
  | (address, cell) :: rest =>
      choose_boundary prefer_new (endpoint cell) address (reference_boundary prefer_new endpoint rest)
  end.

Lemma choose_boundary_matches_positional_extension prefer_new endpoint address cell entries
    address_cache position_cache :
  cache_matches entries address_cache position_cache ->
  cache_matches ((address, cell) :: entries)
    (choose_boundary prefer_new endpoint address address_cache)
    (F.extend_endpoint_cache prefer_new endpoint position_cache).
Proof.
  destruct address_cache as [[ending target]|], position_cache as [[other position]|];
    cbn [cache_matches]; try contradiction.
  - intros [same [selected found]]. subst other.
    unfold choose_boundary, F.extend_endpoint_cache.
    destruct prefer_new; cbn -[Nat.ltb].
    + destruct (endpoint <=? ending) eqn:chosen; cbn [cache_matches].
      * apply Nat.leb_le in chosen. rewrite Nat.min_l by assumption.
        split; [reflexivity|exists cell; reflexivity].
      * apply Nat.leb_gt in chosen. rewrite Nat.min_r by lia.
        split; [reflexivity|exists selected; exact found].
    + destruct (endpoint <? ending) eqn:chosen; cbn [cache_matches].
      * apply Nat.ltb_lt in chosen. rewrite Nat.min_l by lia.
        split; [reflexivity|exists cell; reflexivity].
      * apply Nat.ltb_ge in chosen. rewrite Nat.min_r by assumption.
        split; [reflexivity|exists selected; exact found].
  - intros _. cbn [choose_boundary F.extend_endpoint_cache cache_matches].
    split; [reflexivity|exists cell; reflexivity].
Qed.

Theorem address_caches_refine_positional_selection_without_address_arithmetic
    prefer_new endpoint entries :
  cache_matches entries (reference_boundary prefer_new endpoint entries)
    (F.compute_endpoint_cache prefer_new (map (fun entry => endpoint (snd entry)) entries)).
Proof.
  induction entries as [|[address cell] rest IH]; cbn; [exact I|].
  apply choose_boundary_matches_positional_extension. exact IH.
Qed.

Lemma reference_all_is_generic entries :
  reference_all entries = reference_boundary false (fun cell => F.owner_bound (cell_owner cell)) entries.
Proof. induction entries as [|[address cell] rest IH]; cbn; [reflexivity|now rewrite IH]. Qed.

Lemma reference_path_is_generic entries :
  reference_path entries = reference_boundary true (fun cell => F.owner_path_end (cell_owner cell)) entries.
Proof. induction entries as [|[address cell] rest IH]; cbn; [reflexivity|now rewrite IH]. Qed.

Definition entry_bounds (entries : owner_entries) := map (fun entry => F.owner_bound (cell_owner (snd entry))) entries.
Definition entry_endpoints (entries : owner_entries) := map (fun entry => F.owner_path_end (cell_owner (snd entry))) entries.

Lemma scheduled_projection_has_exact_endpoints entries :
  F.scheduled_bounds (F.schedule_owners (projected_owners entries)) = entry_bounds entries /\
  F.scheduled_endpoints (F.schedule_owners (projected_owners entries)) = entry_endpoints entries.
Proof.
  induction entries as [|[address cell] rest [bounds endpoints]]; [split; reflexivity|].
  change ((F.owner_bound (cell_owner cell) ::
      F.scheduled_bounds (F.schedule_owners (projected_owners rest))) =
      F.owner_bound (cell_owner cell) :: entry_bounds rest /\
    (F.owner_path_end (cell_owner cell) ::
      F.scheduled_endpoints (F.schedule_owners (projected_owners rest))) =
      F.owner_path_end (cell_owner cell) :: entry_endpoints rest).
  now rewrite bounds, endpoints.
Qed.

Theorem linked_cached_reads_refine_the_existing_scheduled_owner_caches arena root entries :
  linked arena root entries ->
  let scheduled := F.schedule_owners (projected_owners entries) in
  cache_matches entries (read_all arena root) (F.any_boundary_cache (F.top_caches scheduled)) /\
  cache_matches entries (read_path arena root) (F.path_boundary_cache (F.top_caches scheduled)).
Proof.
  intro valid. cbn zeta.
  destruct (cached_linked_reads_refine_exact_list_selection valid) as [all [paths _]].
  destruct (F.valid_cache_projection _ (F.schedule_owners_valid (projected_owners entries))) as [any scheduled_paths].
  destruct (scheduled_projection_has_exact_endpoints entries) as [bounds endpoints].
  rewrite all, paths, any, scheduled_paths, bounds, endpoints,
    reference_all_is_generic, reference_path_is_generic.
  split; apply address_caches_refine_positional_selection_without_address_arithmetic.
Qed.

Definition address_due stamp (cache : boundary_handle) :=
  match cache with Some (ending, address) => if ending =? stamp then Some address else None | None => None end.

Definition translate_position (entries : owner_entries) position :=
  match position with
  | None => None
  | Some index => option_map fst (nth_error entries index)
  end.

Theorem due_address_is_exactly_the_translated_scheduler_position entries address_cache position_cache stamp :
  cache_matches entries address_cache position_cache ->
  address_due stamp address_cache = translate_position entries (F.cached_due stamp position_cache).
Proof.
  destruct address_cache as [[ending address]|], position_cache as [[other position]|];
    cbn [cache_matches]; try contradiction; [|reflexivity].
  intros [same [cell found]]. subst other.
  unfold address_due, F.cached_due. destruct (ending =? stamp); [|reflexivity].
  cbn [translate_position]. now rewrite found.
Qed.

Lemma linked_entry_lookup arena root entries address cell :
  linked arena root entries -> In (address, cell) entries -> nth_error arena address = Some cell.
Proof.
  intros valid. induction valid; cbn; [contradiction|].
  intros [same|inside]; [inversion same; subst; assumption|now apply IHvalid].
Qed.

Theorem selected_cached_address_is_a_reachable_historical_owner arena root entries address_cache
    position_cache ending address position :
  linked arena root entries -> cache_matches entries address_cache position_cache ->
  address_cache = Some (ending, address) -> position_cache = Some (ending, position) ->
  exists cell, nth_error entries position = Some (address, cell) /\
    nth_error arena address = Some cell.
Proof.
  intros valid matches selected scheduled. subst address_cache position_cache.
  destruct matches as [_ [cell found]]. exists cell. split; [exact found|].
  eapply linked_entry_lookup; [exact valid|]. eapply nth_error_In; exact found.
Qed.

Print Assumptions address_caches_refine_positional_selection_without_address_arithmetic.
Print Assumptions linked_cached_reads_refine_the_existing_scheduled_owner_caches.
Print Assumptions due_address_is_exactly_the_translated_scheduler_position.
Print Assumptions selected_cached_address_is_a_reachable_historical_owner.

Lemma linked_addresses_are_below_parent_bound arena root entries :
  linked arena root entries -> forall bound,
  (forall index, root = Some index -> index < bound) ->
  forall address cell, In (address, cell) entries -> address < bound.
Proof.
  intro valid. induction valid as [|index current rest found parents IH backward all paths outer];
    intros bound bounded address cell inside; [contradiction|].
  cbn in inside. destruct inside as [same|inside].
  - inversion same; subst. apply bounded. reflexivity.
  - pose proof (IH index backward address cell inside) as earlier.
    pose proof (bounded index eq_refl) as root_bound. lia.
Qed.

Theorem selected_root_address_is_exactly_position_zero arena root entries position cell :
  linked arena (Some root) entries ->
  nth_error entries position = Some (root, cell) -> position = 0.
Proof.
  intros valid selected. inversion valid; subst.
  destruct position; [reflexivity|]. cbn in selected.
  assert (inside : In (root, cell) rest) by (eapply nth_error_In; exact selected).
  assert (earlier : root < root).
  { eapply linked_addresses_are_below_parent_bound; eauto. }
  lia.
Qed.

Theorem cached_address_due_refines_exact_scan_under_no_overdue_boundary
    prefer_new endpoint entries stamp :
  Forall (fun ending => stamp <= ending) (map (fun entry => endpoint (snd entry)) entries) ->
  address_due stamp (reference_boundary prefer_new endpoint entries) =
    translate_position entries
      (F.scan_due prefer_new stamp (map (fun entry => endpoint (snd entry)) entries)).
Proof.
  intro bounds.
  pose proof (@due_address_is_exactly_the_translated_scheduler_position entries _ _ stamp
    (address_caches_refine_positional_selection_without_address_arithmetic prefer_new endpoint entries))
    as translated.
  rewrite F.computed_cache_selects_exact_scan in translated by exact bounds.
  exact translated.
Qed.

Print Assumptions selected_root_address_is_exactly_position_zero.
Print Assumptions cached_address_due_refines_exact_scan_under_no_overdue_boundary.

Lemma reference_boundary_selects_its_targets_endpoint prefer_new endpoint entries :
  forall ending address, reference_boundary prefer_new endpoint entries = Some (ending, address) ->
  exists cell, In (address, cell) entries /\ endpoint cell = ending.
Proof.
  induction entries as [|[index cell] rest IH]; intros ending address selected; [discriminate|].
  cbn [reference_boundary] in selected.
  destruct (reference_boundary prefer_new endpoint rest) as [[old_end old_address]|] eqn:tail.
  - unfold choose_boundary in selected.
    destruct (if prefer_new then endpoint cell <=? old_end else endpoint cell <? old_end).
    + inversion selected; subst. exists cell. split; [now left|reflexivity].
    + inversion selected; subst.
      destruct (IH ending address eq_refl) as [target [inside same]].
      exists target. split; [now right|exact same].
  - cbn [choose_boundary] in selected. inversion selected; subst.
    exists cell. split; [now left|reflexivity].
Qed.

Theorem concrete_cached_target_has_exact_endpoint_and_arena_identity
    arena root entries prefer_new endpoint ending address :
  linked arena root entries ->
  reference_boundary prefer_new endpoint entries = Some (ending, address) ->
  exists cell, In (address, cell) entries /\ nth_error arena address = Some cell /\
    endpoint cell = ending.
Proof.
  intros valid selected.
  destruct (reference_boundary_selects_its_targets_endpoint prefer_new endpoint entries selected)
    as [cell [inside same]].
  exists cell. split; [exact inside|]. split; [|exact same].
  eapply linked_entry_lookup; eauto.
Qed.

Print Assumptions concrete_cached_target_has_exact_endpoint_and_arena_identity.

Lemma linked_root_position_classification arena root entries address_cache position_cache stamp :
  linked arena (Some root) entries -> cache_matches entries address_cache position_cache ->
  option_map (fun address => address =? root) (address_due stamp address_cache) =
  option_map (fun position => position =? 0) (F.cached_due stamp position_cache).
Proof.
  intros valid matches.
  destruct address_cache as [[ending address]|], position_cache as [[other position]|];
    cbn [cache_matches] in matches; try contradiction; [|reflexivity].
  destruct matches as [same [cell found]]. subst other.
  unfold address_due, F.cached_due. destruct (ending =? stamp); [|reflexivity].
  cbn [option_map]. destruct position.
  - inversion valid; subst. cbn in found. inversion found; subst. now rewrite Nat.eqb_refl.
  - assert (different : address <> root).
    { intro same. subst address.
      pose proof (@selected_root_address_is_exactly_position_zero arena root entries (S position) cell valid found). discriminate. }
    apply Nat.eqb_neq in different. now rewrite different.
Qed.

Inductive settlement_command :=
| StopSettlement (view : F.boundary_view) (checks : nat)
| CompleteSettlement (parent : option nat) (current : F.owner) (child : F.leaf).

Definition linked_command arena root stamp source offset view : option settlement_command :=
  match view with
  | F.BoundaryDead _ | F.BoundaryPending _ => Some (StopSettlement view 0)
  | _ =>
      match root with
      | None => Some (StopSettlement view 0)
      | Some address =>
          match nth_error arena address with
          | None => None
          | Some cell =>
              match option_map (fun selected => selected =? address)
                (address_due stamp (cell_path_boundary cell)) with
              | None => Some (StopSettlement F.BoundaryUnfinished 1)
              | Some false => Some (StopSettlement
                  (F.BoundaryDead (F.BoundaryTruncated source offset)) 1)
              | Some true =>
                  match view with
                  | F.BoundaryCompleted child =>
                      Some (CompleteSettlement (cell_parent cell) (cell_owner cell) child)
                  | _ => Some (StopSettlement
                      (F.BoundaryDead (F.BoundaryTruncated source offset)) 1)
                  end
              end
          end
      end
  end.

Definition entry_root (entries : owner_entries) :=
  match entries with [] => None | (address, _) :: _ => Some address end.

Lemma linked_root_is_entry_root arena root entries :
  linked arena root entries -> root = entry_root entries.
Proof. intro valid. destruct valid; reflexivity. Qed.

Definition positional_command stamp source offset view entries :=
  match view with
  | F.BoundaryDead _ | F.BoundaryPending _ => StopSettlement view 0
  | _ => match entries with
    | [] => StopSettlement view 0
    | (_, cell) :: rest =>
        match F.cached_due stamp (F.path_boundary_cache
          (F.top_caches (F.schedule_owners (projected_owners entries)))) with
        | None => StopSettlement F.BoundaryUnfinished 1
        | Some (S _) => StopSettlement (F.BoundaryDead (F.BoundaryTruncated source offset)) 1
        | Some 0 => match view with
          | F.BoundaryCompleted child => CompleteSettlement (entry_root rest) (cell_owner cell) child
          | _ => StopSettlement (F.BoundaryDead (F.BoundaryTruncated source offset)) 1
          end
        end
    end
  end.

Theorem linked_dispatch_refines_exact_positional_command arena root entries stamp source offset view :
  linked arena root entries ->
  linked_command arena root stamp source offset view = Some (positional_command stamp source offset view entries).
Proof.
  intro valid. destruct view as [error|id| |child]; try reflexivity;
    destruct valid as [|address cell rest found parents backward all paths outer]; try reflexivity.
  all: assert (valid : linked arena (Some address) ((address, cell) :: rest))
    by (constructor; assumption).
  all: destruct (linked_cached_reads_refine_the_existing_scheduled_owner_caches valid) as [_ matches].
  all: pose proof (@linked_root_position_classification arena address ((address, cell) :: rest)
    _ _ stamp valid matches) as selection.
  all: unfold read_path, lookup in selection; rewrite found in selection.
  all: unfold linked_command; rewrite found, selection.
  all: unfold positional_command.
  all: destruct (F.cached_due stamp (F.path_boundary_cache
    (F.top_caches (F.schedule_owners (projected_owners ((address, cell) :: rest))))))
    as [[|position]|]; try reflexivity.
  rewrite (linked_root_is_entry_root parents). reflexivity.
Qed.

Inductive linked_settlement arena classify complete stamp source offset :
    option nat -> F.boundary_view -> option nat -> F.boundary_observation -> nat -> Prop :=
| settlement_stop root view observed checks :
    linked_command arena root stamp source offset view = Some (StopSettlement observed checks) ->
    linked_settlement arena classify complete stamp source offset root view root
      (F.unchanged_boundary observed) checks
| settlement_refuse root view parent current child code at_source at_offset :
    linked_command arena root stamp source offset view = Some (CompleteSettlement parent current child) ->
    complete (F.owner_metadata current) (F.owner_region_end current - stamp) child =
      F.RefusedOwner code at_source at_offset ->
    linked_settlement arena classify complete stamp source offset root view root
      (F.note_call (F.region_slot (F.owner_metadata current)) 0
        (F.unchanged_boundary (F.BoundaryDead (F.BoundaryRefusal code at_source at_offset)))) 1
| settlement_pop root view parent current child next final observed checks :
    linked_command arena root stamp source offset view = Some (CompleteSettlement parent current child) ->
    complete (F.owner_metadata current) (F.owner_region_end current - stamp) child = F.CompletedOwner next ->
    linked_settlement arena classify complete stamp source offset parent (classify next) final observed checks ->
    linked_settlement arena classify complete stamp source offset root view final
      (F.note_call (F.region_slot (F.owner_metadata current)) 1 observed) (S checks).

Print Assumptions linked_dispatch_refines_exact_positional_command.

Theorem linked_settlement_terminates_with_exact_observations_checks_and_retained_prefix
    arena root entries classify complete stamp source offset :
  linked arena root entries -> forall view,
  let scheduled := F.schedule_owners (projected_owners entries) in
  let observed := F.deepest_due_settlement classify complete stamp source offset view scheduled in
  exists final,
    linked_settlement arena classify complete stamp source offset root view final observed
      (F.settlement_owner_checks classify complete stamp source offset view scheduled) /\
    linked arena final (skipn (F.boundary_pops observed) entries).
Proof.
  intro valid. induction valid as [|address cell rest found parents IH backward all paths outer];
    intro view; cbn zeta.
  - destruct view; exists None;
      (split; [apply settlement_stop; reflexivity|rewrite skipn_nil; exact (@linked_empty arena)]).
  - assert (valid : linked arena (Some address) ((address, cell) :: rest))
      by (constructor; assumption).
    pose proof (@linked_dispatch_refines_exact_positional_command arena (Some address)
      ((address, cell) :: rest) stamp source offset view valid) as command.
    destruct view as [error|id| |child].
    + exists (Some address). split; [apply settlement_stop; exact command|exact valid].
    + exists (Some address). split; [apply settlement_stop; exact command|exact valid].
    + cbn [projected_owners map F.schedule_owners F.push_scheduled
        F.deepest_due_settlement F.settlement_owner_checks F.top_caches F.scheduled_caches].
      unfold positional_command in command.
      destruct (F.cached_due stamp (F.path_boundary_cache
        (F.top_caches (F.schedule_owners (projected_owners ((address, cell) :: rest))))))
        as [[|position]|] eqn:due;
        cbn [projected_owners map F.schedule_owners F.push_scheduled F.top_caches F.scheduled_caches] in due;
        rewrite due; exists (Some address);
        (split; [apply settlement_stop; exact command|exact valid]).
    + cbn [projected_owners map F.schedule_owners F.push_scheduled
        F.deepest_due_settlement F.settlement_owner_checks F.top_caches F.scheduled_caches].
      unfold positional_command in command.
      destruct (F.cached_due stamp (F.path_boundary_cache
        (F.top_caches (F.schedule_owners (projected_owners ((address, cell) :: rest))))))
        as [[|position]|] eqn:due;
        cbn [projected_owners map F.schedule_owners F.push_scheduled F.top_caches F.scheduled_caches] in due;
        rewrite due; cbn [F.scheduled_value F.owner_value snd].
      * destruct (complete (F.owner_metadata (cell_owner cell))
          (F.owner_region_end (cell_owner cell) - stamp) child) as [next|code at_source at_offset] eqn:completed.
        -- destruct (IH (classify next)) as [final [run retained]].
           exists final. split.
           ++ eapply settlement_pop; [exact command|exact completed|].
              now rewrite <- (linked_root_is_entry_root parents).
           ++ exact retained.
        -- exists (Some address). split.
           ++ eapply settlement_refuse; [exact command|exact completed].
           ++ exact valid.
      * exists (Some address). split; [apply settlement_stop; exact command|exact valid].
      * exists (Some address). split; [apply settlement_stop; exact command|exact valid].
Qed.

Theorem linked_settlement_has_eager_semantics_and_pop_bounded_owner_work
    arena root entries classify complete stamp source offset view :
  linked arena root entries ->
  Forall (fun ending => stamp <= ending) (entry_endpoints entries) ->
  exists final observed checks,
    linked_settlement arena classify complete stamp source offset root view final observed checks /\
    observed = F.eager_unwind classify complete stamp source offset view
      (F.schedule_owners (projected_owners entries)) /\
    checks <= S (F.boundary_pops observed) /\
    linked arena final (skipn (F.boundary_pops observed) entries).
Proof.
  intros valid bounds.
  destruct (@linked_settlement_terminates_with_exact_observations_checks_and_retained_prefix
    arena root entries classify complete stamp source offset valid view) as [final [run retained]].
  exists final,
    (F.deepest_due_settlement classify complete stamp source offset view
      (F.schedule_owners (projected_owners entries))),
    (F.settlement_owner_checks classify complete stamp source offset view
      (F.schedule_owners (projected_owners entries))).
  split; [exact run|]. split.
  - apply F.deepest_due_settlement_equals_eager_unwind_under_exact_local_completion.
    + apply F.schedule_owners_valid.
    + rewrite (proj2 (scheduled_projection_has_exact_endpoints entries)). exact bounds.
  - split; [apply F.focused_settlement_checks_are_bounded_by_successful_pops_plus_one|exact retained].
Qed.

Print Assumptions linked_settlement_terminates_with_exact_observations_checks_and_retained_prefix.
Print Assumptions linked_settlement_has_eager_semantics_and_pop_bounded_owner_work.

Theorem linked_settlement_is_deterministic arena classify complete stamp source offset
    root view final observed checks :
  linked_settlement arena classify complete stamp source offset root view final observed checks ->
  forall other_final other_observed other_checks,
    linked_settlement arena classify complete stamp source offset root view
      other_final other_observed other_checks ->
    (final, observed, checks) = (other_final, other_observed, other_checks).
Proof.
  intro run. induction run; intros other_final other_observed other_checks other;
    inversion other; subst.
  all: match goal with
    | left : ?command = Some ?a, right : ?command = Some ?b |- _ =>
        assert (same : a = b) by congruence; clear right; inversion same; subst; clear same
    end.
  all: try congruence.
  match goal with
  | left : ?completion = F.CompletedOwner ?a, right : ?completion = F.CompletedOwner ?b |- _ =>
      assert (same : a = b) by congruence; subst b; clear right
  end.
  match goal with
  | IH : forall _ _ _, linked_settlement _ _ _ _ _ _ _ _ _ _ _ -> _,
      tail : linked_settlement _ _ _ _ _ _ _ _ _ _ _ |- _ =>
      specialize (IH _ _ _ tail); inversion IH; subst; reflexivity
  end.
Qed.

Theorem every_linked_run_preserves_eager_result_prefix_and_pop_bounded_checks
    arena root entries classify complete stamp source offset view final observed checks :
  linked arena root entries ->
  Forall (fun ending => stamp <= ending) (entry_endpoints entries) ->
  linked_settlement arena classify complete stamp source offset root view final observed checks ->
  observed = F.eager_unwind classify complete stamp source offset view
    (F.schedule_owners (projected_owners entries)) /\
  checks <= S (F.boundary_pops observed) /\
  linked arena final (skipn (F.boundary_pops observed) entries).
Proof.
  intros valid bounds run.
  destruct (@linked_settlement_has_eager_semantics_and_pop_bounded_owner_work
    arena root entries classify complete stamp source offset view valid bounds)
    as [expected_final [expected_observed [expected_checks [expected_run properties]]]].
  pose proof (@linked_settlement_is_deterministic arena classify complete stamp source offset
    root view final observed checks run expected_final expected_observed expected_checks expected_run) as same.
  inversion same; subst. exact properties.
Qed.

Print Assumptions linked_settlement_is_deterministic.
Print Assumptions every_linked_run_preserves_eager_result_prefix_and_pop_bounded_checks.

End PersistentFocusedOwners.

Module PersistentFrameRealization.

Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.

Definition source_location := (nat * nat)%type.
Inductive obligation_kind := RawParEscape | NestedEpm1.
Inductive varint_role :=
| ScalarRole | ScaleRole | BytesRole (utf8 : bool) | CountRole | RegionLengthRole
| ObligationLengthRole (kind : obligation_kind) (tag_source : source_location).

Record varint_snapshot := VarintSnapshot {
  varint_value : nat;
  varint_shift : nat;
  varint_bytes : nat
}.

Record utf8_snapshot := Utf8Snapshot {
  utf8_remaining : nat;
  utf8_next_min : nat;
  utf8_next_max : nat
}.

Record obligation_snapshot := ObligationSnapshot {
  obligation_id : nat;
  obligation_type : obligation_kind;
  obligation_payload_len : nat;
  obligation_tag_source : source_location
}.

Inductive obligation_phase :=
| PayloadPhase (remaining : nat)
| AwaitingPhase (payload_end : source_location).

Record raw_path := RawPath {
  raw_metadata : F.path_metadata;
  raw_stack : option nat
}.

Section Storage.

Context {Span : Type}.

Inductive inactive_payload :=
| TagPayload (escape_allowed : bool)
| VarintPayload (role : varint_role) (state : varint_snapshot)
| BytesPayload (remaining : nat) (utf8 : option utf8_snapshot)
| KidsPayload (remaining : nat)
| RegionLengthPayload (remaining : nat) (previous : option Span) (state : varint_snapshot)
| ObligationPayload (obligation : obligation_snapshot) (phase : obligation_phase) (payload_start : nat).

Inductive stored_frame :=
| InactiveFrame (payload : inactive_payload) (next : option nat)
| ActiveRegionFrame (remaining : nat) (previous : option Span)
    (declared_len path_remaining start_depth : nat) (child : raw_path) (next : option nat).

Inductive frame_tree :=
| EmptyTree
| InactiveTree (slot : nat) (payload : inactive_payload) (next : frame_tree)
| RegionTree (slot remaining : nat) (previous : option Span)
    (declared_len path_remaining start_depth : nat) (child_metadata : F.path_metadata)
    (child next : frame_tree).

Definition tree_root tree :=
  match tree with EmptyTree => None | InactiveTree slot _ _ | RegionTree slot _ _ _ _ _ _ _ _ => Some slot end.

Fixpoint physical_tree (frames : list stored_frame) tree : Prop :=
  match tree with
  | EmptyTree => True
  | InactiveTree slot payload next =>
      nth_error frames slot = Some (InactiveFrame payload (tree_root next)) /\ physical_tree frames next
  | RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next =>
      nth_error frames slot = Some (ActiveRegionFrame remaining previous declared_len path_remaining start_depth
        (RawPath child_metadata (tree_root child)) (tree_root next)) /\
      physical_tree frames child /\ physical_tree frames next
  end.

Record semantic_path := SemanticPath {
  semantic_metadata : F.path_metadata;
  semantic_frames : frame_tree
}.

Definition raw_of path := RawPath (semantic_metadata path) (tree_root (semantic_frames path)).
Definition physical_path frames raw path := raw = raw_of path /\ physical_tree frames (semantic_frames path).

Theorem typed_physical_frames_survive_arbitrary_append frames suffix tree :
  physical_tree frames tree -> physical_tree (frames ++ suffix) tree.
Proof.
  induction tree; cbn; intros represented; [exact I| |].
  - destruct represented as [found next]. split.
    + eapply P.old_lookup_after_append; exact found.
    + now apply IHtree.
  - destruct represented as [found [child next]]. split.
    + eapply P.old_lookup_after_append; exact found.
    + split; [now apply IHtree1|now apply IHtree2].
Qed.

Theorem typed_path_metadata_and_all_payloads_survive_append frames suffix raw path :
  physical_path frames raw path -> physical_path (frames ++ suffix) raw path.
Proof.
  intros [same represented]. split; [exact same|].
  now apply typed_physical_frames_survive_arbitrary_append.
Qed.

Record held_owner := HeldOwner {
  held_metadata : F.path_metadata;
  held_slot : nat;
  held_region_end : nat;
  held_path_end : nat;
  held_previous : option Span;
  held_declared_len : nat;
  held_start_depth : nat;
  held_next : frame_tree
}.

Definition owner_realizes frames stamp owner :=
  physical_tree frames (held_next owner) /\
  exists birth region_remaining path_remaining old_child old_child_tree,
    nth_error frames (held_slot owner) = Some
      (ActiveRegionFrame region_remaining (held_previous owner) (held_declared_len owner)
        path_remaining (held_start_depth owner) old_child (tree_root (held_next owner))) /\
    physical_path frames old_child (SemanticPath (raw_metadata old_child) old_child_tree) /\
    held_region_end owner = birth + region_remaining /\
    held_path_end owner = birth + path_remaining /\
    birth <= stamp /\ stamp <= held_region_end owner /\ stamp <= held_path_end owner.

Definition wrap_owner stamp owner child :=
  SemanticPath (held_metadata owner)
    (RegionTree (held_slot owner) (held_region_end owner - stamp) (held_previous owner)
      (held_declared_len owner) (held_path_end owner - stamp) (held_start_depth owner)
      (semantic_metadata child) (semantic_frames child) (held_next owner)).

Fixpoint reify_owners stamp current owners :=
  match owners with [] => current | owner :: rest => reify_owners stamp (wrap_owner stamp owner current) rest end.

Inductive overlay_realizes frames stamp : semantic_path -> list held_owner -> semantic_path -> Prop :=
| overlay_leaf focus : physical_tree frames (semantic_frames focus) -> overlay_realizes frames stamp focus [] focus
| overlay_outer focus owners inner owner :
    overlay_realizes frames stamp focus owners inner -> owner_realizes frames stamp owner ->
    overlay_realizes frames stamp focus (owners ++ [owner]) (wrap_owner stamp owner inner).

Lemma reify_owners_append stamp focus left right :
  reify_owners stamp focus (left ++ right) = reify_owners stamp (reify_owners stamp focus left) right.
Proof. revert focus. induction left; intro focus; cbn; [reflexivity|apply IHleft]. Qed.

Theorem typed_overlay_derives_exact_logical_graph frames stamp focus owners graph :
  overlay_realizes frames stamp focus owners graph -> graph = reify_owners stamp focus owners.
Proof.
  intro represented. induction represented; [reflexivity|].
  rewrite reify_owners_append. cbn. now rewrite <- IHrepresented.
Qed.

Lemma owner_realizes_append frames suffix stamp owner :
  owner_realizes frames stamp owner -> owner_realizes (frames ++ suffix) stamp owner.
Proof.
  intros [next [birth [region_remaining [path_remaining [child [child_tree [found [child_physical facts]]]]]]]]. split.
  - now apply typed_physical_frames_survive_arbitrary_append.
  - exists birth, region_remaining, path_remaining, child, child_tree. split.
    + eapply P.old_lookup_after_append; exact found.
    + split; [now apply typed_path_metadata_and_all_payloads_survive_append|exact facts].
Qed.

Theorem every_typed_overlay_checkpoint_survives_frame_append frames suffix stamp focus owners graph :
  overlay_realizes frames stamp focus owners graph -> overlay_realizes (frames ++ suffix) stamp focus owners graph.
Proof.
  intro represented. induction represented.
  - constructor. now apply typed_physical_frames_survive_arbitrary_append.
  - apply overlay_outer; [exact IHrepresented|now apply owner_realizes_append].
Qed.

Fixpoint erase_frame_ids tree :=
  match tree with
  | EmptyTree => EmptyTree
  | InactiveTree _ payload next => InactiveTree 0 payload (erase_frame_ids next)
  | RegionTree _ remaining previous declared_len path_remaining start_depth metadata child next =>
      RegionTree 0 remaining previous declared_len path_remaining start_depth metadata
        (erase_frame_ids child) (erase_frame_ids next)
  end.

Definition same_path_semantics left right :=
  semantic_metadata left = semantic_metadata right /\
  erase_frame_ids (semantic_frames left) = erase_frame_ids (semantic_frames right).

Definition materialize_owner frames stamp owner child :=
  let frame := ActiveRegionFrame (held_region_end owner - stamp) (held_previous owner)
    (held_declared_len owner) (held_path_end owner - stamp) (held_start_depth owner)
    (raw_of child) (tree_root (held_next owner)) in
  (frames ++ [frame], SemanticPath (held_metadata owner)
    (RegionTree (length frames) (held_region_end owner - stamp) (held_previous owner)
      (held_declared_len owner) (held_path_end owner - stamp) (held_start_depth owner)
      (semantic_metadata child) (semantic_frames child) (held_next owner))).

Theorem materializing_one_owner_uses_a_fresh_slot_and_preserves_semantics_and_history frames stamp owner child :
  owner_realizes frames stamp owner -> physical_tree frames (semantic_frames child) ->
  let '(next_frames, next_path) := materialize_owner frames stamp owner child in
  physical_tree next_frames (semantic_frames next_path) /\
  tree_root (semantic_frames next_path) = Some (length frames) /\
  same_path_semantics next_path (wrap_owner stamp owner child) /\
  length next_frames = S (length frames) /\
  (forall slot frame, nth_error frames slot = Some frame -> nth_error next_frames slot = Some frame).
Proof.
  intros [next owner_fields] child_fields. cbn [materialize_owner semantic_frames].
  split.
  - cbn [physical_tree]. split.
    + rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. reflexivity.
    + split; now apply typed_physical_frames_survive_arbitrary_append.
  - split; [reflexivity|]. split.
    + split; reflexivity.
    + split; [rewrite length_app; cbn; lia|].
      intros slot frame found. eapply P.old_lookup_after_append; exact found.
Qed.

Lemma path_semantics_refl path : same_path_semantics path path.
Proof. split; reflexivity. Qed.

Lemma path_semantics_trans left middle right :
  same_path_semantics left middle -> same_path_semantics middle right -> same_path_semantics left right.
Proof. intros [metadata frames] [metadata2 frames2]. split; congruence. Qed.

Lemma wrap_owner_preserves_semantic_correspondence stamp owner left right :
  same_path_semantics left right -> same_path_semantics (wrap_owner stamp owner left) (wrap_owner stamp owner right).
Proof.
  intros [metadata frames]. split; [reflexivity|].
  cbn [wrap_owner semantic_frames erase_frame_ids]. now rewrite metadata, frames.
Qed.

Lemma reify_owners_preserves_semantic_correspondence stamp owners left right :
  same_path_semantics left right -> same_path_semantics (reify_owners stamp left owners) (reify_owners stamp right owners).
Proof.
  revert left right. induction owners; intros left right same; cbn; [exact same|].
  apply IHowners. now apply wrap_owner_preserves_semantic_correspondence.
Qed.

Fixpoint materialize_spine frames stamp focus owners :=
  match owners with
  | [] => (frames, focus)
  | owner :: rest =>
      let '(next_frames, next_focus) := materialize_owner frames stamp owner focus in
      materialize_spine next_frames stamp next_focus rest
  end.

Theorem full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner
    frames stamp focus owners :
  physical_tree frames (semantic_frames focus) -> Forall (owner_realizes frames stamp) owners ->
  let '(next_frames, next_path) := materialize_spine frames stamp focus owners in
  exists suffix,
    next_frames = frames ++ suffix /\ length suffix = length owners /\
    physical_tree next_frames (semantic_frames next_path) /\
    same_path_semantics next_path (reify_owners stamp focus owners).
Proof.
  revert frames focus. induction owners as [|owner rest IH]; intros frames focus physical owners.
  - exists []. cbn. rewrite app_nil_r. repeat split; auto.
  - inversion owners as [|? ? held tail]; subst.
    pose proof (@materializing_one_owner_uses_a_fresh_slot_and_preserves_semantics_and_history
      frames stamp owner focus held physical) as one.
    destruct (materialize_owner frames stamp owner focus) as [next_frames next_focus] eqn:step.
    destruct one as [next_physical [root [same [size old_cells]]]].
    assert (tail_realizes : Forall (owner_realizes next_frames stamp) rest).
    { unfold materialize_owner in step. inversion step; subst next_frames.
      eapply Forall_impl; [|exact tail]. intros retained represents.
      now apply owner_realizes_append. }
    pose proof (IH next_frames next_focus next_physical tail_realizes) as later.
    cbn [materialize_spine]. rewrite step.
    destruct (materialize_spine next_frames stamp next_focus rest) as [final_frames final_path] eqn:result.
    destruct later as [suffix [extension [count [final_physical final_same]]]].
    unfold materialize_owner in step. inversion step; subst next_frames next_focus.
    eexists. split.
    + rewrite extension, <- app_assoc. reflexivity.
    + split; [cbn; now rewrite count|]. split; [exact final_physical|].
      eapply path_semantics_trans; [exact final_same|].
      cbn [reify_owners]. apply reify_owners_preserves_semantic_correspondence. exact same.
Qed.

Theorem forced_spine_preserves_every_old_physical_and_focused_checkpoint
    frames stamp focus owners old_raw old_path old_stamp old_focus old_owners old_graph :
  physical_tree frames (semantic_frames focus) -> Forall (owner_realizes frames stamp) owners ->
  physical_path frames old_raw old_path -> overlay_realizes frames old_stamp old_focus old_owners old_graph ->
  let '(next_frames, _) := materialize_spine frames stamp focus owners in
  physical_path next_frames old_raw old_path /\
  overlay_realizes next_frames old_stamp old_focus old_owners old_graph /\
  (forall slot frame, nth_error frames slot = Some frame -> nth_error next_frames slot = Some frame).
Proof.
  intros physical valid_owners old_physical old_overlay.
  pose proof (@full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner
    frames stamp focus owners physical valid_owners) as forced.
  destruct (materialize_spine frames stamp focus owners) as [next_frames next_path].
  destruct forced as [suffix [extension rest]]. subst next_frames.
  split; [now apply typed_path_metadata_and_all_payloads_survive_append|].
  split; [now apply every_typed_overlay_checkpoint_survives_frame_append|].
  intros slot frame found. eapply P.old_lookup_after_append; exact found.
Qed.

Theorem capturing_a_physical_region_establishes_its_exact_held_owner_and_child
    frames stamp metadata slot remaining previous declared_len path_remaining start_depth child_metadata child next :
  physical_tree frames
    (RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next) ->
  let owner := HeldOwner metadata slot (stamp + remaining) (stamp + path_remaining)
    previous declared_len start_depth next in
  owner_realizes frames stamp owner /\
  wrap_owner stamp owner (SemanticPath child_metadata child) =
    SemanticPath metadata
      (RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next).
Proof.
  intros [found [child_physical next_physical]]. cbn zeta. split.
  - split; [exact next_physical|].
    exists stamp, remaining, path_remaining, (RawPath child_metadata (tree_root child)), child.
    cbn. split; [exact found|]. split.
    + split; [reflexivity|exact child_physical].
    + repeat split; lia.
  - unfold wrap_owner. cbn. f_equal; f_equal; lia.
Qed.

Theorem advancing_a_valid_graph_stamp_preserves_held_cells_and_decreases_residuals
    frames stamp later owner :
  owner_realizes frames stamp owner -> stamp <= later ->
  later <= held_region_end owner -> later <= held_path_end owner ->
  owner_realizes frames later owner /\
  held_region_end owner - later <= held_region_end owner - stamp /\
  held_path_end owner - later <= held_path_end owner - stamp.
Proof.
  intros [next [birth [region_remaining [path_remaining [child [child_tree
    [found [child_physical [region_end [path_end [born bounds]]]]]]]]]]] advanced region_bound path_bound.
  split.
  - split; [exact next|]. exists birth, region_remaining, path_remaining, child, child_tree.
    split; [exact found|]. split; [exact child_physical|]. repeat split; try assumption; lia.
  - split; lia.
Qed.

Fixpoint private_terminal tree : P.terminal_view :=
  match tree with
  | RegionTree _ _ _ _ _ _ _ child _ => private_terminal child
  | InactiveTree _ (ObligationPayload obligation phase _) _ =>
      let '(source, offset) := obligation_tag_source obligation in
      P.TerminalObligation (obligation_id obligation) source offset
        (match phase with AwaitingPhase _ => true | PayloadPhase _ => false end)
  | _ => P.TerminalOther
  end.

Definition path_private_terminal path := private_terminal (semantic_frames path).

Theorem typed_overlay_private_terminal_is_the_selected_focus_terminal stamp owners focus :
  path_private_terminal (reify_owners stamp focus owners) = path_private_terminal focus.
Proof.
  revert focus. induction owners; intro focus; cbn [reify_owners]; [reflexivity|].
  rewrite IHowners. reflexivity.
Qed.

Lemma erasing_frame_ids_preserves_private_terminal tree :
  private_terminal (erase_frame_ids tree) = private_terminal tree.
Proof. induction tree; cbn; [reflexivity|destruct payload; reflexivity|exact IHtree1]. Qed.

Lemma equal_typed_path_semantics_preserve_private_terminal left right :
  same_path_semantics left right -> path_private_terminal left = path_private_terminal right.
Proof.
  intros [_ same]. unfold path_private_terminal.
  rewrite <- (erasing_frame_ids_preserves_private_terminal (semantic_frames left)),
    <- (erasing_frame_ids_preserves_private_terminal (semantic_frames right)). now rewrite same.
Qed.

Theorem typed_materialization_preserves_exact_private_resolve_and_reject_preflight
    frames stamp focus owners requested caller :
  physical_tree frames (semantic_frames focus) -> Forall (owner_realizes frames stamp) owners ->
  let '(_, forced) := materialize_spine frames stamp focus owners in
  P.resolve_preflight requested (path_private_terminal forced) =
    P.resolve_preflight requested (path_private_terminal focus) /\
  P.reject_preflight requested caller (path_private_terminal forced) =
    P.reject_preflight requested caller (path_private_terminal focus).
Proof.
  intros physical valid_owners.
  pose proof (@full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner
    frames stamp focus owners physical valid_owners) as result.
  destruct (materialize_spine frames stamp focus owners) as [next_frames forced].
  destruct result as [suffix [extension [count [represented same]]]].
  apply equal_typed_path_semantics_preserve_private_terminal in same.
  rewrite typed_overlay_private_terminal_is_the_selected_focus_terminal in same.
  rewrite same. split; reflexivity.
Qed.

Theorem capture_from_a_physical_path_copies_the_actual_enclosing_metadata_and_stack
    frames stamp raw metadata slot remaining previous declared_len path_remaining start_depth child_metadata child next :
  physical_path frames raw (SemanticPath metadata
    (RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next)) ->
  let owner := HeldOwner (raw_metadata raw) slot (stamp + remaining) (stamp + path_remaining)
    previous declared_len start_depth next in
  raw_metadata raw = metadata /\ raw_stack raw = Some slot /\
  owner_realizes frames stamp owner /\
  wrap_owner stamp owner (SemanticPath child_metadata child) =
    SemanticPath metadata
      (RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next).
Proof.
  intros [same represented]. subst raw. cbn zeta.
  split; [reflexivity|]. split; [reflexivity|].
  now apply capturing_a_physical_region_establishes_its_exact_held_owner_and_child.
Qed.

End Storage.

Print Assumptions typed_physical_frames_survive_arbitrary_append.
Print Assumptions typed_path_metadata_and_all_payloads_survive_append.
Print Assumptions typed_overlay_derives_exact_logical_graph.
Print Assumptions every_typed_overlay_checkpoint_survives_frame_append.
Print Assumptions materializing_one_owner_uses_a_fresh_slot_and_preserves_semantics_and_history.
Print Assumptions full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner.
Print Assumptions forced_spine_preserves_every_old_physical_and_focused_checkpoint.
Print Assumptions capturing_a_physical_region_establishes_its_exact_held_owner_and_child.
Print Assumptions advancing_a_valid_graph_stamp_preserves_held_cells_and_decreases_residuals.
Print Assumptions typed_overlay_private_terminal_is_the_selected_focus_terminal.
Print Assumptions typed_materialization_preserves_exact_private_resolve_and_reject_preflight.
Print Assumptions capture_from_a_physical_path_copies_the_actual_enclosing_metadata_and_stack.

End PersistentFrameRealization.

Module PersistentOwnerProjection.

Module T := PersistentFrameRealization.
Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.
Module S := ReusableFrameOwnership.

Section Projection.

Context {Span : Type}.
Variable observe_span : Span -> option (list CanonicalKeyProtocol.located_byte).
Variable summarize_frame : @T.stored_frame Span -> S.frame_metadata.

Definition observe_previous previous :=
  match previous with
  | None => Some None
  | Some span => option_map (@Some (list CanonicalKeyProtocol.located_byte)) (observe_span span)
  end.

Fixpoint project_slots (tree : @T.frame_tree Span) : S.graph :=
  match tree with
  | T.EmptyTree => S.EmptyGraph
  | T.InactiveTree slot payload next =>
      S.LinkGraph slot (summarize_frame (T.InactiveFrame payload (T.tree_root next))) (project_slots next)
  | T.RegionTree slot remaining previous declared_len path_remaining start_depth metadata child next =>
      S.RegionGraph slot (summarize_frame (T.ActiveRegionFrame remaining previous declared_len
        path_remaining start_depth (T.RawPath metadata (T.tree_root child)) (T.tree_root next)))
        (project_slots child) (project_slots next)
  end.

Fixpoint typed_slots (tree : @T.frame_tree Span) : list nat :=
  match tree with
  | T.EmptyTree => []
  | T.InactiveTree slot _ next => slot :: typed_slots next
  | T.RegionTree slot _ _ _ _ _ _ child next => slot :: typed_slots child ++ typed_slots next
  end.

Lemma slot_projection_preserves_all_frame_ids tree : S.graph_slots (project_slots tree) = typed_slots tree.
Proof. induction tree; cbn; congruence. Qed.

Fixpoint project_tree metadata (tree : @T.frame_tree Span) : option F.path_graph :=
  match tree with
  | T.RegionTree slot remaining previous declared_len path_remaining start_depth child_metadata child next =>
      match observe_previous previous, project_tree child_metadata child with
      | Some previous_bytes, Some projected_child =>
          Some (F.RegionGraph (F.RegionMetadata slot metadata previous_bytes declared_len start_depth
            (project_slots next)) remaining path_remaining projected_child)
      | _, _ => None
      end
  | _ => Some (F.LeafGraph (F.Leaf metadata (project_slots tree)))
  end.

Definition project_path (path : @T.semantic_path Span) :=
  project_tree (T.semantic_metadata path) (T.semantic_frames path).

Definition project_owner (owner : @T.held_owner Span) :=
  option_map (fun previous => F.Owner
    (F.RegionMetadata (T.held_slot owner) (T.held_metadata owner) previous
      (T.held_declared_len owner) (T.held_start_depth owner) (project_slots (T.held_next owner)))
    (T.held_region_end owner) (T.held_path_end owner))
    (observe_previous (T.held_previous owner)).

Definition owners_match (owners : list (@T.held_owner Span)) (entries : P.owner_entries) :=
  Forall2 (fun owner entry => project_owner owner = Some (P.cell_owner (snd entry))) owners entries.

Theorem invalid_previous_span_refuses_owner_projection owner span :
  T.held_previous owner = Some span -> observe_span span = None -> project_owner owner = None.
Proof. intros previous invalid. unfold project_owner, observe_previous. now rewrite previous, invalid. Qed.

Theorem projected_owner_preserves_exact_scheduler_fields owner projected :
  project_owner owner = Some projected ->
  F.owner_region_end projected = T.held_region_end owner /\
  F.owner_path_end projected = T.held_path_end owner /\
  F.region_slot (F.owner_metadata projected) = T.held_slot owner /\
  F.region_owner (F.owner_metadata projected) = T.held_metadata owner /\
  F.region_declared_length (F.owner_metadata projected) = T.held_declared_len owner /\
  F.region_start_depth (F.owner_metadata projected) = T.held_start_depth owner /\
  F.region_next (F.owner_metadata projected) = project_slots (T.held_next owner) /\
  observe_previous (T.held_previous owner) = Some (F.region_previous (F.owner_metadata projected)).
Proof.
  unfold project_owner. destruct (observe_previous (T.held_previous owner)) as [previous|] eqn:observed;
    cbn; intro same; [|discriminate]. inversion same; subst. repeat split; reflexivity.
Qed.

Lemma project_wrapped_owner stamp owner child projected_owner projected_child :
  project_owner owner = Some projected_owner -> project_path child = Some projected_child ->
  project_path (T.wrap_owner stamp owner child) =
    Some (F.residual_graph stamp projected_owner projected_child).
Proof.
  unfold project_owner. destruct (observe_previous (T.held_previous owner)) as [previous|] eqn:observed;
    cbn; intros same child_matches; [|discriminate]. inversion same; subst.
  unfold project_path, T.wrap_owner; cbn [T.semantic_metadata T.semantic_frames project_tree].
  unfold project_path in child_matches. rewrite observed, child_matches. reflexivity.
Qed.

Theorem typed_reification_commutes_with_exact_linked_owner_projection stamp focus owners entries projected :
  owners_match owners entries -> project_path focus = Some projected ->
  project_path (T.reify_owners stamp focus owners) =
    Some (F.reify stamp projected (P.projected_owners entries)).
Proof.
  intros matches. revert focus projected. induction matches; intros focus projected focused; cbn.
  - exact focused.
  - apply IHmatches. eapply project_wrapped_owner; eauto.
Qed.

Theorem projected_paths_preserve_frame_addresses metadata tree graph :
  project_tree metadata tree = Some graph -> F.graph_slots graph = typed_slots tree.
Proof.
  revert metadata graph. induction tree; intros metadata graph projected.
  - inversion projected; reflexivity.
  - inversion projected; subst. cbn. now rewrite slot_projection_preserves_all_frame_ids.
  - cbn [project_tree] in projected.
    destruct (observe_previous previous) as [previous_bytes|]; [|discriminate].
    destruct (project_tree child_metadata tree1) as [child_graph|] eqn:child; [|discriminate].
    inversion projected; subst. cbn.
    rewrite slot_projection_preserves_all_frame_ids, (IHtree1 _ _ child). reflexivity.
Qed.

Theorem typed_overlay_is_the_selected_linked_graph frames stamp focus owners graph arena root entries leaf :
  T.overlay_realizes frames stamp focus owners graph -> P.linked arena root entries ->
  owners_match owners entries -> project_path focus = Some (F.LeafGraph leaf) ->
  project_path graph = Some (P.projected_graph stamp leaf entries).
Proof.
  intros overlay linked matches focused.
  rewrite (T.typed_overlay_derives_exact_logical_graph overlay).
  now apply typed_reification_commutes_with_exact_linked_owner_projection.
Qed.

End Projection.

Print Assumptions invalid_previous_span_refuses_owner_projection.
Print Assumptions projected_owner_preserves_exact_scheduler_fields.
Print Assumptions typed_reification_commutes_with_exact_linked_owner_projection.
Print Assumptions projected_paths_preserve_frame_addresses.
Print Assumptions typed_overlay_is_the_selected_linked_graph.

End PersistentOwnerProjection.

Module PersistentFaultRealization.

Module T := PersistentFrameRealization.
Module Q := PersistentOwnerProjection.
Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.
Module S := ReusableFrameOwnership.

Section Faults.

Context {Span : Type}.
Variable observe_span : Span -> option (list CanonicalKeyProtocol.located_byte).
Variable summarize_frame : @T.stored_frame Span -> S.frame_metadata.

Definition kill_typed (path : @T.semantic_path Span) error :=
  T.SemanticPath (P.kill_metadata (T.semantic_metadata path) error) (T.semantic_frames path).

Theorem root_error_replacement_commutes_with_checked_projection path graph error :
  Q.project_path observe_span summarize_frame path = Some graph ->
  Q.project_path observe_span summarize_frame (kill_typed path error) = Some (P.kill_root graph error).
Proof.
  destruct path as [metadata tree]. destruct tree; unfold Q.project_path, kill_typed;
    cbn [T.semantic_metadata T.semantic_frames Q.project_tree]; intro projected.
  - inversion projected; reflexivity.
  - inversion projected; reflexivity.
  - destruct (Q.observe_previous observe_span previous) as [previous_bytes|]; [|discriminate].
    destruct (Q.project_tree observe_span summarize_frame child_metadata tree1) as [child_graph|];
      [|discriminate]. inversion projected; reflexivity.
Qed.

Lemma killing_preserves_typed_correspondence left right error :
  T.same_path_semantics left right -> T.same_path_semantics (kill_typed left error) (kill_typed right error).
Proof. intros [metadata frames]. split; cbn; [now rewrite metadata|exact frames]. Qed.

Record typed_fault := TypedFault {
  selected_focus : @T.semantic_path Span;
  selected_owners : list (@T.held_owner Span);
  selected_stamp : nat;
  selected_bytes : list CanonicalKeyProtocol.located_byte;
  current_bytes : list CanonicalKeyProtocol.located_byte;
  selected_error : P.fault_error
}.

Definition selected_graph current :=
  T.reify_owners (selected_stamp current) (selected_focus current) (selected_owners current).
Definition typed_fault_graph current := kill_typed (selected_graph current) (selected_error current).

Definition coherent frames arena current legacy :=
  T.physical_tree frames (T.semantic_frames (selected_focus current)) /\
  Forall (T.owner_realizes frames (selected_stamp current)) (selected_owners current) /\
  P.linked arena (P.checkpoint_owners (P.fault_saved legacy)) (P.fault_selected_entries legacy) /\
  Q.owners_match observe_span summarize_frame (selected_owners current) (P.fault_selected_entries legacy) /\
  Q.project_path observe_span summarize_frame (selected_focus current) =
    Some (F.LeafGraph (P.checkpoint_focus (P.fault_saved legacy))) /\
  selected_stamp current = P.checkpoint_stamp (P.fault_saved legacy) /\
  selected_bytes current = P.checkpoint_bytes (P.fault_saved legacy) /\
  selected_stamp current = length (selected_bytes current) /\
  current_bytes current = P.fault_current_bytes legacy /\
  selected_error current = P.fault_selected_error legacy /\
  exists suffix, current_bytes current = selected_bytes current ++ suffix.

Theorem coherent_typed_fault_projects_to_exact_selected_legacy_fault frames arena current legacy :
  coherent frames arena current legacy ->
  Q.project_path observe_span summarize_frame (typed_fault_graph current) = Some (P.fault_graph legacy).
Proof.
  intros [physical [owners [linked [matches [focus [stamp [bytes [stamp_bytes [current_bytes_same [error suffix]]]]]]]]]].
  unfold typed_fault_graph, P.fault_graph. rewrite <- error, <- stamp.
  apply root_error_replacement_commutes_with_checked_projection.
  unfold selected_graph, P.projected_graph.
  now apply Q.typed_reification_commutes_with_exact_linked_owner_projection.
Qed.

Definition append_dead_byte current byte :=
  TypedFault (selected_focus current) (selected_owners current) (selected_stamp current)
    (selected_bytes current) (current_bytes current ++ [byte]) (selected_error current).

Theorem appending_dead_bytes_preserves_selected_fault_coherence frames arena current legacy byte :
  coherent frames arena current legacy ->
  coherent frames arena (append_dead_byte current byte) (P.fault_edge legacy byte).
Proof.
  unfold coherent. cbn [append_dead_byte P.fault_edge].
  intros [physical [owners [linked [matches [focus [stamp [bytes [stamp_bytes [current_same [error [suffix extension]]]]]]]]]]].
  do 8 (split; [assumption|]).
  unfold append_dead_byte, P.fault_edge. cbn.
  split; [now rewrite current_same|]. split; [exact error|].
  exists (suffix ++ [byte]). now rewrite extension, app_assoc.
Qed.

Theorem dead_byte_keeps_saved_stamp_and_graph_but_extends_current_provenance current byte :
  selected_stamp (append_dead_byte current byte) = selected_stamp current /\
  typed_fault_graph (append_dead_byte current byte) = typed_fault_graph current /\
  current_bytes (append_dead_byte current byte) = current_bytes current ++ [byte].
Proof. repeat split; reflexivity. Qed.

Definition force_typed_fault frames current :=
  let '(next_frames, path) := T.materialize_spine frames (selected_stamp current)
    (selected_focus current) (selected_owners current) in
  (next_frames, kill_typed path (selected_error current), current_bytes current).

Theorem forcing_a_coherent_fault_preserves_exact_typed_observation_and_all_historical_cells
    frames arena current legacy :
  coherent frames arena current legacy ->
  let '(next_frames, forced, bytes) := force_typed_fault frames current in
  exists suffix,
    next_frames = frames ++ suffix /\ length suffix = length (selected_owners current) /\
    T.physical_tree next_frames (T.semantic_frames forced) /\
    T.same_path_semantics forced (typed_fault_graph current) /\
    bytes = P.fault_current_bytes legacy /\
    F.path_status_value (T.semantic_metadata forced) = P.dead_status (P.fault_selected_error legacy) /\
    (forall slot cell, nth_error frames slot = Some cell -> nth_error next_frames slot = Some cell).
Proof.
  intros [physical [owners [linked [matches [focus [stamp [bytes [stamp_bytes [current_same [error extension]]]]]]]]]].
  pose proof (@T.full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner
    Span frames (selected_stamp current) (selected_focus current) (selected_owners current) physical owners) as result.
  unfold force_typed_fault.
  destruct (T.materialize_spine frames (selected_stamp current) (selected_focus current) (selected_owners current))
    as [next_frames path].
  destruct result as [suffix [appended [count [physical_path same]]]].
  exists suffix. split; [exact appended|]. split; [exact count|].
  split; [exact physical_path|]. split.
  - now apply killing_preserves_typed_correspondence.
  - split; [exact current_same|]. split.
    + cbn [kill_typed T.semantic_metadata P.kill_metadata]. now rewrite error.
    + subst next_frames. intros slot cell found. eapply P.old_lookup_after_append; exact found.
Qed.

Theorem forcing_a_coherent_fault_preserves_hidden_terminal_preflight frames arena current legacy requested caller :
  coherent frames arena current legacy ->
  let '(_, forced, _) := force_typed_fault frames current in
  P.resolve_preflight requested (T.path_private_terminal forced) =
    P.resolve_preflight requested (T.path_private_terminal (selected_focus current)) /\
  P.reject_preflight requested caller (T.path_private_terminal forced) =
    P.reject_preflight requested caller (T.path_private_terminal (selected_focus current)).
Proof.
  intro valid.
  pose proof (forcing_a_coherent_fault_preserves_exact_typed_observation_and_all_historical_cells valid) as result.
  destruct (force_typed_fault frames current) as [[next_frames forced] bytes].
  destruct result as [suffix [_ [_ [_ [same rest]]]]].
  apply T.equal_typed_path_semantics_preserve_private_terminal in same.
  unfold typed_fault_graph, kill_typed, T.path_private_terminal in same.
  cbn [T.semantic_frames] in same.
  change (T.path_private_terminal forced = T.path_private_terminal (selected_graph current)) in same.
  unfold selected_graph in same. rewrite T.typed_overlay_private_terminal_is_the_selected_focus_terminal in same.
  now rewrite same.
Qed.

End Faults.

Print Assumptions root_error_replacement_commutes_with_checked_projection.
Print Assumptions coherent_typed_fault_projects_to_exact_selected_legacy_fault.
Print Assumptions appending_dead_bytes_preserves_selected_fault_coherence.
Print Assumptions dead_byte_keeps_saved_stamp_and_graph_but_extends_current_provenance.
Print Assumptions forcing_a_coherent_fault_preserves_exact_typed_observation_and_all_historical_cells.
Print Assumptions forcing_a_coherent_fault_preserves_hidden_terminal_preflight.

End PersistentFaultRealization.

Module EffectfulLinkedSettlement.

Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.

Section Effects.

Context {World : Type}.
Variable classify : World -> F.leaf -> F.boundary_view.
Variable complete : World -> F.region_metadata -> nat -> F.leaf -> World * F.completion_result.

Fixpoint reference stamp source offset world view (entries : P.owner_entries)
    : World * F.boundary_observation * nat :=
  match entries with
  | [] => (world, F.unchanged_boundary view, 0)
  | (_, cell) :: rest =>
      match P.positional_command stamp source offset view entries with
      | P.StopSettlement observed checks => (world, F.unchanged_boundary observed, checks)
      | P.CompleteSettlement _ owner child =>
          let '(next_world, outcome) := complete world (F.owner_metadata owner)
            (F.owner_region_end owner - stamp) child in
          match outcome with
          | F.CompletedOwner next =>
              let '(final_world, observed, checks) := reference stamp source offset next_world
                (classify next_world next) rest in
              (final_world, F.note_call (F.region_slot (F.owner_metadata owner)) 1 observed, S checks)
          | F.RefusedOwner code at_source at_offset =>
              (next_world, F.note_call (F.region_slot (F.owner_metadata owner)) 0
                (F.unchanged_boundary (F.BoundaryDead (F.BoundaryRefusal code at_source at_offset))), 1)
          end
      end
  end.

Inductive run arena stamp source offset :
    World -> option nat -> F.boundary_view -> World -> option nat -> F.boundary_observation -> nat -> Prop :=
| run_stop world root view observed checks :
    P.linked_command arena root stamp source offset view = Some (P.StopSettlement observed checks) ->
    run arena stamp source offset world root view world root (F.unchanged_boundary observed) checks
| run_refuse world root view parent owner child next_world code at_source at_offset :
    P.linked_command arena root stamp source offset view = Some (P.CompleteSettlement parent owner child) ->
    complete world (F.owner_metadata owner) (F.owner_region_end owner - stamp) child =
      (next_world, F.RefusedOwner code at_source at_offset) ->
    run arena stamp source offset world root view next_world root
      (F.note_call (F.region_slot (F.owner_metadata owner)) 0
        (F.unchanged_boundary (F.BoundaryDead (F.BoundaryRefusal code at_source at_offset)))) 1
| run_pop world root view parent owner child next_world next final_world final observed checks :
    P.linked_command arena root stamp source offset view = Some (P.CompleteSettlement parent owner child) ->
    complete world (F.owner_metadata owner) (F.owner_region_end owner - stamp) child =
      (next_world, F.CompletedOwner next) ->
    run arena stamp source offset next_world parent (classify next_world next) final_world final observed checks ->
    run arena stamp source offset world root view final_world final
      (F.note_call (F.region_slot (F.owner_metadata owner)) 1 observed) (S checks).

Theorem effectful_linked_traversal_terminates_with_exact_world_trace_and_retained_prefix
    arena root entries stamp source offset :
  P.linked arena root entries -> forall world view,
  let '(final_world, observed, checks) := reference stamp source offset world view entries in
  exists final,
    run arena stamp source offset world root view final_world final observed checks /\
    P.linked arena final (skipn (F.boundary_pops observed) entries).
Proof.
  intro valid. induction valid as [|address cell rest found parents IH backward all paths outer];
    intros world view.
  - destruct view; exists None;
      (split; [apply run_stop; reflexivity|rewrite skipn_nil; exact (@P.linked_empty arena)]).
  - assert (valid : P.linked arena (Some address) ((address, cell) :: rest))
      by (constructor; assumption).
    pose proof (@P.linked_dispatch_refines_exact_positional_command arena (Some address)
      ((address, cell) :: rest) stamp source offset view valid) as command.
    cbn [reference].
    destruct view as [error|id| |child].
    + exists (Some address). split; [apply run_stop; exact command|exact valid].
    + exists (Some address). split; [apply run_stop; exact command|exact valid].
    + unfold P.positional_command in command |- *.
      destruct (F.cached_due stamp (F.path_boundary_cache
        (F.top_caches (F.schedule_owners (P.projected_owners ((address, cell) :: rest))))))
        as [[|position]|]; exists (Some address);
        (split; [apply run_stop; exact command|exact valid]).
    + unfold P.positional_command in command |- *.
      destruct (F.cached_due stamp (F.path_boundary_cache
        (F.top_caches (F.schedule_owners (P.projected_owners ((address, cell) :: rest))))))
        as [[|position]|].
      * destruct (complete world (F.owner_metadata (P.cell_owner cell))
          (F.owner_region_end (P.cell_owner cell) - stamp) child)
          as [next_world [next|code at_source at_offset]] eqn:completed.
        -- pose proof (IH next_world (classify next_world next)) as later.
           destruct (reference stamp source offset next_world (classify next_world next) rest)
             as [[final_world observed] checks].
           destruct later as [final [tail retained]]. exists final. split.
           ++ eapply run_pop; [exact command|exact completed|].
              now rewrite <- (P.linked_root_is_entry_root parents).
           ++ exact retained.
        -- exists (Some address). split; [eapply run_refuse; eauto|exact valid].
      * exists (Some address). split; [apply run_stop; exact command|exact valid].
      * exists (Some address). split; [apply run_stop; exact command|exact valid].
Qed.

Theorem effectful_linked_runs_are_deterministic arena stamp source offset
    world root view final_world final observed checks :
  run arena stamp source offset world root view final_world final observed checks ->
  forall other_world other_final other_observed other_checks,
    run arena stamp source offset world root view other_world other_final other_observed other_checks ->
    (final_world, final, observed, checks) = (other_world, other_final, other_observed, other_checks).
Proof.
  intro execution. induction execution; intros other_world other_final other_observed other_checks other;
    inversion other; subst.
  all: match goal with
    | left : ?command = Some ?a, right : ?command = Some ?b |- _ =>
        assert (same : a = b) by congruence; clear right; inversion same; subst; clear same
    end.
  all: try congruence.
  all: match goal with
    | left : ?callback = (?w1, F.CompletedOwner ?a), right : ?callback = (?w2, F.CompletedOwner ?b) |- _ =>
        assert (same : (w1, a) = (w2, b)) by congruence;
        clear right; inversion same; subst; clear same
    end.
  match goal with
    | IH : forall _ _ _ _, run _ _ _ _ _ _ _ _ _ _ _ -> _,
      tail : run _ _ _ _ _ _ _ _ _ _ _ |- _ =>
        specialize (IH _ _ _ _ tail); inversion IH; subst; reflexivity
  end.
Qed.

Theorem every_effectful_run_refines_reference_world_and_callback_order
    arena root entries stamp source offset world view final_world final observed checks :
  P.linked arena root entries ->
  run arena stamp source offset world root view final_world final observed checks ->
  reference stamp source offset world view entries = (final_world, observed, checks) /\
  P.linked arena final (skipn (F.boundary_pops observed) entries).
Proof.
  intros valid execution.
  pose proof (@effectful_linked_traversal_terminates_with_exact_world_trace_and_retained_prefix
    arena root entries stamp source offset valid world view) as exists_run.
  destruct (reference stamp source offset world view entries) as [[expected_world expected_observed] expected_checks]
    eqn:expected.
  destruct exists_run as [expected_final [expected_run retained]].
  pose proof (effectful_linked_runs_are_deterministic execution expected_run) as same.
  inversion same; subst. auto.
Qed.

Lemma positional_stop_checks_bounded stamp source offset view entries observed checks :
  P.positional_command stamp source offset view entries = P.StopSettlement observed checks -> checks <= 1.
Proof.
  unfold P.positional_command. destruct view; destruct entries as [|[address cell] rest];
    try (intro same; inversion same; lia).
  all: destruct (F.cached_due stamp (F.path_boundary_cache
    (F.top_caches (F.schedule_owners (P.projected_owners ((address, cell) :: rest)))))) as [[|index]|];
    intro same; inversion same; lia.
Qed.

Lemma reference_owner_checks_bounded stamp source offset entries : forall world view,
  let '(_, observed, checks) := reference stamp source offset world view entries in
  checks <= S (F.boundary_pops observed).
Proof.
  induction entries as [|[address cell] rest IH]; intros world view; cbn [reference]; [cbn; lia|].
  destruct (P.positional_command stamp source offset view ((address, cell) :: rest))
    as [observed checks|parent owner child] eqn:command.
  - cbn. eapply positional_stop_checks_bounded; exact command.
  - destruct (complete world (F.owner_metadata owner) (F.owner_region_end owner - stamp) child)
      as [next_world [next|code at_source at_offset]] eqn:completed; [|cbn; lia].
    pose proof (IH next_world (classify next_world next)) as bounded.
    destruct (reference stamp source offset next_world (classify next_world next) rest)
      as [[final_world observed] checks]. cbn in bounded |- *. lia.
Qed.

Theorem effectful_owner_checks_remain_pop_bounded arena root entries stamp source offset
    world view final_world final observed checks :
  P.linked arena root entries ->
  run arena stamp source offset world root view final_world final observed checks ->
  checks <= S (F.boundary_pops observed).
Proof.
  intros valid execution.
  destruct (every_effectful_run_refines_reference_world_and_callback_order valid execution) as [same retained].
  pose proof (reference_owner_checks_bounded stamp source offset entries world view) as bounded.
  now rewrite same in bounded.
Qed.

End Effects.

Definition control_outer_owner := F.Owner (F.control_metadata 1) 12 9.
Definition control_outer := P.make_cell [] None control_outer_owner.
Definition control_inner := P.make_cell [control_outer] (Some 0) (F.Owner (F.control_metadata 2) 9 9).
Definition control_arena := [control_outer; control_inner].
Definition control_entries := [(1, control_inner); (0, control_outer)].
Definition control_classify (_ : nat) := F.completed_control_view.
Definition control_complete clock metadata remaining child :=
  (S clock, F.settlement_control_complete metadata remaining child).

Theorem outer_refusal_retains_inner_and_refusing_callback_effects_control :
  P.linked control_arena (Some 1) control_entries /\
  reference control_classify control_complete 9 2 31 0 (F.BoundaryCompleted F.control_leaf) control_entries =
    (2, F.BoundaryObservation (F.BoundaryDead (F.BoundaryRefusal 77 8 91)) 1 [2; 1], 2).
Proof.
  split.
  - pose proof (@P.append_owner_constructs_valid_backward_links_and_exact_caches
      [] None [] control_outer_owner (@P.linked_empty [])) as outer.
    change (P.linked [control_outer] (Some 0) [(0, control_outer)]) in outer.
    exact (@P.append_owner_constructs_valid_backward_links_and_exact_caches
      [control_outer] (Some 0) [(0, control_outer)] (F.Owner (F.control_metadata 2) 9 9) outer).
  - reflexivity.
Qed.

Print Assumptions effectful_linked_traversal_terminates_with_exact_world_trace_and_retained_prefix.
Print Assumptions effectful_linked_runs_are_deterministic.
Print Assumptions every_effectful_run_refines_reference_world_and_callback_order.
Print Assumptions effectful_owner_checks_remain_pop_bounded.
Print Assumptions outer_refusal_retains_inner_and_refusing_callback_effects_control.

End EffectfulLinkedSettlement.
