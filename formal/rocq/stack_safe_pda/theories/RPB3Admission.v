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
