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
