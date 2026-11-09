From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA RPB3SemanticReducer
  RPB3RecursiveAuxiliarySourceReference RPB3SourceIndexedFrames.

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

Theorem rpb3_compiled_program_evaluates_to_result :
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

Theorem generated_descriptor_raw_wire_source_indexed_summary_equivalence :
  forall hash_fn type_id bytes,
    SourceIndexedFrames.source_indexed_root_summary hash_fn type_id bytes =
    match RPB3RecursiveAuxiliarySourceReference.recursive_mixed_root_reference
        hash_fn type_id bytes with
    | Some results =>
        match SourceIndexedFrames.close_message
            (SourceIndexedFrames.source_generated_root_field type_id bytes)
            type_id results with
        | Some root =>
            Some [RPB3RecursiveAuxiliarySourceReference.recursive_result_semantic
              root]
        | None => None
        end
    | None => None
    end.
Proof.
  exact SourceIndexedFrames.source_indexed_root_has_universal_recursive_summary.
Qed.

Theorem generated_descriptor_raw_wire_source_indexed_verdict_equivalence :
  forall hash_fn type_id bytes,
    SourceIndexedFrames.source_indexed_root_accepts hash_fn type_id bytes =
    match RPB3RecursiveAuxiliarySourceReference.recursive_mixed_root_reference
        hash_fn type_id bytes with
    | Some results =>
        match SourceIndexedFrames.close_message
            (SourceIndexedFrames.source_generated_root_field type_id bytes)
            type_id results with
        | Some root =>
            negb (RPB3SemanticReducer.sem_definitely_refused
              (RPB3RecursiveAuxiliarySourceReference.recursive_result_semantic
                root))
        | None => false
        end
    | None => false
    end.
Proof.
  exact SourceIndexedFrames.source_indexed_root_has_universal_recursive_verdict.
Qed.

Print Assumptions generated_descriptor_raw_wire_source_indexed_summary_equivalence.
Print Assumptions generated_descriptor_raw_wire_source_indexed_verdict_equivalence.

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

Module CompactPersistentCheckpoints.

Module T := PersistentFrameRealization.
Module P := PersistentFocusedOwners.
Module F := PersistentFaultRealization.

Inductive address := DirectSlot (slot : nat) | LinkedSlot (slot : nat).

Definition encode_address position :=
  match position with DirectSlot slot => 2 * slot | LinkedSlot slot => 2 * slot + 1 end.

Theorem encoded_addresses_are_injective left right :
  encode_address left = encode_address right -> left = right.
Proof. destruct left, right; cbn; intros same; try (f_equal; lia); lia. Qed.

Theorem encoded_address_recovers_slot_and_tag position :
  encode_address position / 2 = match position with DirectSlot slot | LinkedSlot slot => slot end /\
  encode_address position mod 2 = match position with DirectSlot _ => 0 | LinkedSlot _ => 1 end.
Proof.
  destruct position; unfold encode_address; split.
  - replace (2 * slot) with (slot * 2) by lia. apply Nat.div_mul. lia.
  - replace (2 * slot) with (slot * 2) by lia. apply Nat.mod_mul. lia.
  - replace (2 * slot + 1) with (1 + slot * 2) by lia.
    rewrite Nat.div_add; [cbn; lia|lia].
  - rewrite Nat.add_mod, Nat.mul_mod; cbn; lia.
Qed.

Theorem tagged_address_fits_existing_nonzero_vector_bound slot element_size byte_limit word_limit :
  2 <= element_size -> (slot + 1) * element_size <= byte_limit ->
  byte_limit <= word_limit ->
  encode_address (DirectSlot slot) < word_limit /\
  encode_address (LinkedSlot slot) < word_limit.
Proof. cbn. nia. Qed.

Section Store.
Context {Span : Type}.

Record checkpoint := Checkpoint {
  focus : @T.semantic_path Span;
  owners : list (@T.held_owner Span);
  stamp : nat;
  fault : option P.fault_error
}.

Definition apply_fault (path : @T.semantic_path Span) error :=
  match error with None => path | Some error => F.kill_typed path error end.

Definition graph current := apply_fault (T.reify_owners (stamp current) (focus current) (owners current)) (fault current).

Definition plain path position := Checkpoint path [] position None.

Definition compact_plain current := apply_fault (focus current) (fault current).

Theorem ownerless_stamp_and_fault_compaction_preserve_exact_typed_graph current position :
  owners current = [] -> graph (plain (compact_plain current) position) = graph current.
Proof. destruct current; cbn. intro empty. subst owners0. reflexivity. Qed.

Theorem ownerless_compaction_preserves_private_terminal_and_error_replacement current position error :
  owners current = [] ->
  T.path_private_terminal (graph (plain (compact_plain current) position)) = T.path_private_terminal (graph current) /\
  F.kill_typed (graph (plain (compact_plain current) position)) error = F.kill_typed (graph current) error.
Proof.
  intro empty. rewrite (ownerless_stamp_and_fault_compaction_preserve_exact_typed_graph current position empty).
  split; reflexivity.
Qed.

Record storage := Storage {
  direct : list (@T.semantic_path Span);
  linked : list checkpoint
}.

Definition lookup stored handle position :=
  match handle with
  | DirectSlot slot => option_map (fun path => plain path position) (nth_error (direct stored) slot)
  | LinkedSlot slot => nth_error (linked stored) slot
  end.

Definition publish stored current :=
  match owners current with
  | [] => (Storage (direct stored ++ [compact_plain current]) (linked stored), DirectSlot (length (direct stored)))
  | _ :: _ => (Storage (direct stored) (linked stored ++ [current]), LinkedSlot (length (linked stored)))
  end.

Lemma last_lookup {A : Type} (prefix : list A) value : nth_error (prefix ++ [value]) (length prefix) = Some value.
Proof. rewrite nth_error_app2; [rewrite Nat.sub_diag; reflexivity|lia]. Qed.

Theorem publication_returns_exact_graph_at_every_current_byte_length stored current position :
  let '(next, handle) := publish stored current in
  exists selected, lookup next handle position = Some selected /\ graph selected = graph current.
Proof.
  unfold publish. destruct (owners current) as [|owner rest] eqn:shape; cbn [lookup direct linked].
  - rewrite last_lookup. exists (plain (compact_plain current) position). split; [reflexivity|].
    now apply ownerless_stamp_and_fault_compaction_preserve_exact_typed_graph.
  - rewrite last_lookup. exists current. split; reflexivity.
Qed.

Theorem publication_preserves_every_previous_lookup stored current handle position selected :
  lookup stored handle position = Some selected ->
  let '(next, _) := publish stored current in lookup next handle position = Some selected.
Proof.
  destruct stored as [direct linked], handle; unfold lookup; cbn.
  - destruct (nth_error direct slot) eqn:found; cbn; [|discriminate].
    intro same. unfold publish; cbn. destruct (owners current); unfold lookup; cbn; [|now rewrite found].
    rewrite (@P.old_lookup_after_append _ direct [compact_plain current] slot s found). exact same.
  - intro found. unfold publish; cbn. destruct (owners current); unfold lookup; cbn; [exact found|].
    now apply P.old_lookup_after_append.
Qed.

Theorem plain_publication_allocates_no_linked_checkpoint stored current :
  owners current = [] ->
  let '(next, _) := publish stored current in
  linked next = linked stored /\ length (direct next) = S (length (direct stored)).
Proof. intro empty. unfold publish. rewrite empty. cbn. split; [reflexivity|rewrite length_app; cbn; lia]. Qed.

Theorem linked_publication_retains_the_exact_saved_stamp_focus_and_fault stored current :
  owners current <> [] ->
  let '(next, handle) := publish stored current in
  direct next = direct stored /\
  forall position, lookup next handle position = Some current.
Proof.
  intro nonempty. unfold publish. destruct (owners current); [contradiction|]. cbn. split; [reflexivity|].
  intro position. unfold lookup; cbn. apply last_lookup.
Qed.

End Store.

Print Assumptions encoded_addresses_are_injective.
Print Assumptions encoded_address_recovers_slot_and_tag.
Print Assumptions tagged_address_fits_existing_nonzero_vector_bound.
Print Assumptions ownerless_stamp_and_fault_compaction_preserve_exact_typed_graph.
Print Assumptions ownerless_compaction_preserves_private_terminal_and_error_replacement.
Print Assumptions publication_returns_exact_graph_at_every_current_byte_length.
Print Assumptions publication_preserves_every_previous_lookup.
Print Assumptions plain_publication_allocates_no_linked_checkpoint.
Print Assumptions linked_publication_retains_the_exact_saved_stamp_focus_and_fault.

End CompactPersistentCheckpoints.

Module CheckedACTLineTransport.

Import CanonicalKeyProtocol IndexedWindowTransport.

Inductive phase := LineEdge | LineValue | LineLeave | Boundary.

Record cursor := Cursor {
  cursor_identity : nat;
  cursor_generation : nat;
  cursor_record : nat;
  cursor_context : list nat;
  cursor_length : nat;
  cursor_index : nat;
  cursor_phase : phase;
  cursor_events : nat;
  cursor_last : option provenance
}.

Definition phase_eq_dec (x y : phase) : {x = y} + {x <> y}.
Proof. decide equality. Defined.

Definition cursor_eq_dec (x y : cursor) : {x = y} + {x <> y}.
Proof.
  decide equality; try apply Nat.eq_dec; try apply phase_eq_dec;
    repeat decide equality; apply Nat.eq_dec.
Defined.

Inductive event :=
| Edge (value : located_byte)
| AbsentValue (source : provenance)
| Leave (source : provenance).

Definition cursor_at (current : cursor) (index : nat) (stage : phase)
    (events : nat) (last : option provenance) :=
  Cursor (cursor_identity current) (cursor_generation current)
    (cursor_record current) (cursor_context current) (cursor_length current)
    index stage events last.

Definition scalar_next (source : nat -> located_byte) (current : cursor)
    : option (cursor * event) :=
  let index := cursor_index current in
  match cursor_phase current with
  | LineEdge =>
      if S index <? cursor_length current then
        let byte := source index in
        Some (cursor_at current index LineValue (S (cursor_events current))
          (Some (byte_provenance byte)), Edge byte)
      else None
  | LineValue =>
      if S index <? cursor_length current then
        let position := byte_provenance (source index) in
        Some (cursor_at current (S index) LineEdge (S (cursor_events current))
          (Some position), AbsentValue position)
      else None
  | LineLeave =>
      if index <? cursor_length current then
        let position := byte_provenance (source (cursor_length current - S index)) in
        Some (cursor_at current (S index) LineLeave (S (cursor_events current))
          (Some position), Leave position)
      else None
  | Boundary => None
  end.

Fixpoint scalar_run source (count : nat) current : option (cursor * list event) :=
  match count with
  | 0 => Some (current, [])
  | S remaining =>
      match scalar_next source current with
      | None => None
      | Some (next, observed) =>
          option_map (fun '(final, observations) => (final, observed :: observations))
            (scalar_run source remaining next)
      end
  end.

Fixpoint forward_events source (index width : nat) : list event :=
  match width with
  | 0 => []
  | S rest =>
      Edge (source index) :: AbsentValue (byte_provenance (source index)) ::
        forward_events source (S index) rest
  end.

Fixpoint leave_events source (size index width : nat) : list event :=
  match width with
  | 0 => []
  | S rest =>
      Leave (byte_provenance (source (size - S index))) ::
        leave_events source size (S index) rest
  end.

Definition forward source width current :=
  match width with
  | 0 => current
  | S _ => cursor_at current (cursor_index current + width) LineEdge
      (cursor_events current + 2 * width)
      (Some (byte_provenance (source (cursor_index current + width - 1))))
  end.

Definition unwind source width current :=
  match width with
  | 0 => current
  | S _ => cursor_at current (cursor_index current + width) LineLeave
      (cursor_events current + width)
      (Some (byte_provenance (source (cursor_length current - (cursor_index current + width)))))
  end.

Lemma forward_successor source width current :
  forward source width (forward source 1 current) = forward source (S width) current.
Proof.
  destruct width; [reflexivity|].
  unfold forward, cursor_at; cbn. f_equal; try lia.
  f_equal. f_equal. f_equal. lia.
Qed.

Lemma unwind_successor source width current :
  unwind source width (unwind source 1 current) = unwind source (S width) current.
Proof.
  destruct width; [reflexivity|].
  unfold unwind, cursor_at; cbn. f_equal; try lia.
  f_equal. f_equal. f_equal. lia.
Qed.

Lemma one_forward_pair source current :
  cursor_phase current = LineEdge ->
  S (cursor_index current) < cursor_length current ->
  scalar_run source 2 current =
    Some (forward source 1 current,
      [Edge (source (cursor_index current));
       AbsentValue (byte_provenance (source (cursor_index current)))]).
Proof.
  intros stage bounded.
  assert (guard : S (cursor_index current) <? cursor_length current = true)
    by (apply Nat.ltb_lt; exact bounded).
  unfold scalar_run, scalar_next. rewrite stage, guard. cbn -[Nat.ltb].
  rewrite guard. cbn. unfold forward, cursor_at; cbn.
  replace (cursor_index current + 1 - 1) with (cursor_index current) by lia.
  replace (cursor_index current + 1) with (S (cursor_index current)) by lia.
  replace (cursor_events current + 2) with (S (S (cursor_events current))) by lia.
  reflexivity.
Qed.

Lemma scalar_run_add source first second current :
  scalar_run source (first + second) current =
    match scalar_run source first current with
    | None => None
    | Some (middle, prefix) =>
        option_map (fun '(final, suffix) => (final, prefix ++ suffix))
          (scalar_run source second middle)
    end.
Proof.
  revert current. induction first; intro current; cbn.
  - destruct (scalar_run source second current) as [[final suffix]|]; reflexivity.
  -
  destruct (scalar_next source current) as [[next observed]|]; cbn; [|reflexivity].
  rewrite IHfirst.
  destruct (scalar_run source first next) as [[middle prefix]|]; cbn; [|reflexivity].
  destruct (scalar_run source second middle) as [[final suffix]|]; reflexivity.
Qed.

Theorem checked_line_expands_to_exact_scalar_events source width current :
  cursor_phase current = LineEdge ->
  cursor_index current + width < cursor_length current ->
  scalar_run source (2 * width) current =
    Some (forward source width current, forward_events source (cursor_index current) width).
Proof.
  revert current. induction width; intros current stage bounded; [reflexivity|].
  replace (2 * S width) with (2 + 2 * width) by lia.
  rewrite scalar_run_add, one_forward_pair by (auto; lia).
  cbn -[scalar_run forward]. rewrite IHwidth.
  - cbn -[forward]. rewrite forward_successor. unfold forward, cursor_at; cbn.
    now replace (cursor_index current + 1) with (S (cursor_index current)) by lia.
  - reflexivity.
  - unfold forward, cursor_at; cbn. lia.
Qed.

Lemma one_leave source current :
  cursor_phase current = LineLeave ->
  cursor_index current < cursor_length current ->
  scalar_next source current =
    Some (unwind source 1 current,
      Leave (byte_provenance (source (cursor_length current - S (cursor_index current))))).
Proof.
  intros stage bounded. unfold scalar_next. rewrite stage.
  assert (guard : cursor_index current <? cursor_length current = true)
    by (apply Nat.ltb_lt; exact bounded).
  rewrite guard. unfold unwind, cursor_at; cbn.
  now rewrite !Nat.add_1_r.
Qed.

Theorem checked_unwind_expands_to_exact_scalar_leaves source width current :
  cursor_phase current = LineLeave ->
  cursor_index current + width <= cursor_length current ->
  scalar_run source width current =
    Some (unwind source width current,
      leave_events source (cursor_length current) (cursor_index current) width).
Proof.
  revert current. induction width; intros current stage bounded; [reflexivity|].
  cbn [scalar_run]. rewrite one_leave by (auto; lia).
  rewrite IHwidth.
  - cbn -[unwind]. rewrite unwind_successor.
    unfold unwind, cursor_at; cbn. now rewrite Nat.add_1_r.
  - reflexivity.
  - unfold unwind, cursor_at; cbn. lia.
Qed.

Inductive direction := Forward | Unwind.

Record offer := Offer {
  offer_origin : cursor;
  offer_source : nat -> located_byte;
  offer_direction : direction;
  offer_width : nat
}.

Definition guard current request :=
  if cursor_eq_dec current (offer_origin request) then
    (0 <? offer_width request) &&
    match offer_direction request, cursor_phase current with
    | Forward, LineEdge =>
        cursor_index current + offer_width request <? cursor_length current
    | Unwind, LineLeave =>
        cursor_index current + offer_width request <=? cursor_length current
    | _, _ => false
    end
  else false.

Definition commit current request :=
  if guard current request then
    Some (match offer_direction request with
      | Forward => forward (offer_source request) (offer_width request) current
      | Unwind => unwind (offer_source request) (offer_width request) current
      end)
  else None.

Theorem stale_offer_refuses_without_a_new_cursor current request :
  current <> offer_origin request -> commit current request = None.
Proof.
  intro stale. unfold commit, guard.
  destruct (cursor_eq_dec current (offer_origin request)); [contradiction|reflexivity].
Qed.

Theorem zero_width_offer_is_refused current request :
  offer_width request = 0 -> commit current request = None.
Proof.
  intro zero. unfold commit, guard.
  destruct (cursor_eq_dec current (offer_origin request)); [rewrite zero|]; reflexivity.
Qed.

Definition rewind current :=
  Cursor (cursor_identity current) (S (cursor_generation current))
    (cursor_record current) (cursor_context current) (cursor_length current)
    0 LineEdge (cursor_events current) (cursor_last current).

Theorem rewind_invalidates_an_old_offer source current mode width :
  commit (rewind current) (Offer current source mode width) = None.
Proof.
  apply stale_offer_refuses_without_a_new_cursor.
  intro equal. apply (f_equal cursor_generation) in equal. cbn in equal. lia.
Qed.

Theorem every_forward_prefix_keeps_the_final_byte_scalar source width current prefix :
  cursor_phase current = LineEdge ->
  cursor_index current + width < cursor_length current ->
  prefix <= width ->
  scalar_run source (2 * prefix) current =
    Some (forward source prefix current, forward_events source (cursor_index current) prefix) /\
  cursor_index (forward source prefix current) < cursor_length current.
Proof.
  intros stage bounded within. split.
  - apply checked_line_expands_to_exact_scalar_events; auto; lia.
  - destruct prefix; unfold forward, cursor_at; cbn; lia.
Qed.

Theorem every_unwind_prefix_stops_before_the_node_leave source width current prefix :
  cursor_phase current = LineLeave ->
  cursor_index current + width <= cursor_length current ->
  prefix <= width ->
  scalar_run source prefix current =
    Some (unwind source prefix current,
      leave_events source (cursor_length current) (cursor_index current) prefix) /\
  cursor_index (unwind source prefix current) <= cursor_length current.
Proof.
  intros stage bounded within. split.
  - apply checked_unwind_expands_to_exact_scalar_leaves; auto; lia.
  - destruct prefix; unfold unwind, cursor_at; cbn; lia.
Qed.

Theorem committed_forward_preserves_other_topology_authority source width current :
  cursor_context (forward source width current) = cursor_context current /\
  cursor_record (forward source width current) = cursor_record current /\
  cursor_identity (forward source width current) = cursor_identity current /\
  cursor_generation (forward source width current) = cursor_generation current /\
  cursor_length (forward source width current) = cursor_length current.
Proof. destruct width; repeat split; reflexivity. Qed.

Theorem committed_unwind_preserves_other_topology_authority source width current :
  cursor_context (unwind source width current) = cursor_context current /\
  cursor_record (unwind source width current) = cursor_record current /\
  cursor_identity (unwind source width current) = cursor_identity current /\
  cursor_generation (unwind source width current) = cursor_generation current /\
  cursor_length (unwind source width current) = cursor_length current.
Proof. destruct width; repeat split; reflexivity. Qed.

Theorem accepted_commit_has_the_exact_scalar_trace current request next :
  commit current request = Some next ->
  match offer_direction request with
  | Forward =>
      scalar_run (offer_source request) (2 * offer_width request) current =
        Some (next, forward_events (offer_source request) (cursor_index current) (offer_width request))
  | Unwind =>
      scalar_run (offer_source request) (offer_width request) current =
        Some (next, leave_events (offer_source request) (cursor_length current)
          (cursor_index current) (offer_width request))
  end.
Proof.
  unfold commit. destruct (guard current request) eqn:accepted; [|discriminate].
  intro result. inversion result; subst next.
  unfold guard in accepted.
  destruct (cursor_eq_dec current (offer_origin request)); [|discriminate].
  apply andb_true_iff in accepted. destruct accepted as [positive bound].
  destruct (offer_direction request) eqn:direction;
    destruct (cursor_phase current) eqn:stage; try discriminate.
  - apply checked_line_expands_to_exact_scalar_events; auto. now apply Nat.ltb_lt.
  - apply checked_unwind_expands_to_exact_scalar_leaves; auto. now apply Nat.leb_le.
Qed.

Lemma forward_position source width current :
  cursor_index (forward source width current) = cursor_index current + width /\
  cursor_events (forward source width current) = cursor_events current + 2 * width /\
  cursor_length (forward source width current) = cursor_length current.
Proof. destruct width; unfold forward, cursor_at; cbn; repeat split; lia. Qed.

Theorem odd_forward_prefix_preserves_the_pending_absent_value source width current prefix :
  cursor_phase current = LineEdge ->
  cursor_index current + width < cursor_length current ->
  prefix < width ->
  scalar_run source (2 * prefix + 1) current =
    Some (cursor_at current (cursor_index current + prefix) LineValue
      (S (cursor_events current + 2 * prefix))
      (Some (byte_provenance (source (cursor_index current + prefix)))),
      forward_events source (cursor_index current) prefix ++
        [Edge (source (cursor_index current + prefix))]).
Proof.
  intros stage bounded within.
  rewrite scalar_run_add, checked_line_expands_to_exact_scalar_events by (auto; lia).
  pose proof (@forward_position source prefix current) as [position [events size]].
  assert (next_stage : cursor_phase (forward source prefix current) = LineEdge)
    by (destruct prefix; cbn [forward cursor_at]; assumption || reflexivity).
  assert (next_guard : S (cursor_index (forward source prefix current)) <?
      cursor_length (forward source prefix current) = true).
  { apply Nat.ltb_lt. rewrite position, size. lia. }
  cbn [scalar_run]. unfold scalar_next. rewrite next_stage, next_guard.
  cbn -[forward]. rewrite position, events.
  destruct prefix; cbn [forward cursor_at] in *; reflexivity.
Qed.

Theorem expanded_event_counts_preserve_logical_work source index size width :
  length (forward_events source index width) = 2 * width /\
  length (leave_events source size index width) = width.
Proof.
  revert index. induction width; intro index; cbn; [auto|].
  destruct (IHwidth (S index)); split; lia.
Qed.

Inductive concrete_reverse_stage :=
| ChildrenStage (pending : nat)
| LeaveStage
| ReverseBoundary.

Definition normalize_reverse_stage stage current :=
  match stage with
  | ChildrenStage 0 | LeaveStage =>
      Some (cursor_at current (cursor_index current) LineLeave
        (cursor_events current) (cursor_last current))
  | _ => None
  end.

Definition concrete_reverse_run source width stage current :=
  match width with
  | 0 => Some (stage, current, [])
  | S _ =>
      match normalize_reverse_stage stage current with
      | None => None
      | Some normalized =>
          option_map (fun '(next, events) => (LeaveStage, next, events))
            (scalar_run source width normalized)
      end
  end.

Definition concrete_reverse_commit source width stage current :=
  match normalize_reverse_stage stage current with
  | None => None
  | Some normalized =>
      if (0 <? width) && (cursor_index current + width <=? cursor_length current)
      then Some (LeaveStage, unwind source width normalized)
      else None
  end.

Theorem pending_children_never_admit_reverse_transport source width current pending :
  concrete_reverse_commit source width (ChildrenStage (S pending)) current = None.
Proof. reflexivity. Qed.

Theorem reverse_normalization_changes_only_the_unobservable_phase stage current normalized :
  normalize_reverse_stage stage current = Some normalized ->
  cursor_phase normalized = LineLeave /\
  cursor_index normalized = cursor_index current /\
  cursor_length normalized = cursor_length current /\
  cursor_context normalized = cursor_context current /\
  cursor_record normalized = cursor_record current /\
  cursor_identity normalized = cursor_identity current /\
  cursor_generation normalized = cursor_generation current /\
  cursor_events normalized = cursor_events current /\
  cursor_last normalized = cursor_last current.
Proof.
  destruct stage as [[|pending]| |]; cbn; try discriminate;
    intro accepted; inversion accepted; repeat split; reflexivity.
Qed.

Theorem accepted_reverse_entry_commit_matches_the_exact_scalar_trace
    source width stage current next :
  concrete_reverse_commit source width stage current = Some (LeaveStage, next) ->
  concrete_reverse_run source width stage current =
    Some (LeaveStage, next,
      leave_events source (cursor_length current) (cursor_index current) width).
Proof.
  unfold concrete_reverse_commit.
  destruct (normalize_reverse_stage stage current) as [normalized|] eqn:normalized_at;
    [|discriminate].
  destruct ((0 <? width) && (cursor_index current + width <=? cursor_length current))
    eqn:guard; [|discriminate].
  intro accepted. inversion accepted; subst next.
  apply andb_true_iff in guard. destruct guard as [positive bounded].
  apply Nat.ltb_lt in positive. apply Nat.leb_le in bounded.
  pose proof (reverse_normalization_changes_only_the_unobservable_phase
    stage current normalized_at) as [phase [index [size rest]]].
  destruct width; [lia|]. cbn [concrete_reverse_run]. rewrite normalized_at.
  rewrite checked_unwind_expands_to_exact_scalar_leaves by (auto; rewrite index, size; exact bounded).
  cbn. now rewrite index, size.
Qed.

Theorem reverse_entry_refuses_zero_or_excessive_width_before_normalization
    source width stage current :
  width = 0 \/ cursor_length current < cursor_index current + width ->
  concrete_reverse_commit source width stage current = None.
Proof.
  intro invalid. unfold concrete_reverse_commit.
  destruct (normalize_reverse_stage stage current); [|reflexivity].
  destruct invalid as [->|excessive]; [reflexivity|].
  assert (cursor_index current + width <=? cursor_length current = false)
    by (apply Nat.leb_gt; exact excessive).
  now rewrite H, andb_false_r.
Qed.

Definition split_located_source (position : nat -> provenance)
    (read_byte : nat -> nat) (index : nat) : located_byte :=
  let '(owner, offset) := position index in
  LocatedByte (read_byte index) owner offset.

Definition observed_scalar_next (position : nat -> provenance)
    (read_byte : nat -> nat) (current : cursor)
    : option ((cursor * event) * list provenance) :=
  let index := cursor_index current in
  match cursor_phase current with
  | LineEdge =>
      if S index <? cursor_length current then
        let byte := split_located_source position read_byte index in
        Some ((cursor_at current index LineValue (S (cursor_events current))
          (Some (position index)), Edge byte), [position index])
      else None
  | LineValue =>
      if S index <? cursor_length current then
        let place := position index in
        Some ((cursor_at current (S index) LineEdge (S (cursor_events current))
          (Some place), AbsentValue place), [])
      else None
  | LineLeave =>
      if index <? cursor_length current then
        let place := position (cursor_length current - S index) in
        Some ((cursor_at current (S index) LineLeave (S (cursor_events current))
          (Some place), Leave place), [])
      else None
  | Boundary => None
  end.

Definition event_source_reads (observed : event) : list provenance :=
  match observed with Edge byte => [byte_provenance byte] | _ => [] end.

Fixpoint events_source_reads (observations : list event) : list provenance :=
  match observations with
  | [] => []
  | observed :: rest => event_source_reads observed ++ events_source_reads rest
  end.

Fixpoint observed_scalar_run position read_byte count current
    : option ((cursor * list event) * list provenance) :=
  match count with
  | 0 => Some ((current, []), [])
  | S remaining =>
      match observed_scalar_next position read_byte current with
      | None => None
      | Some ((next, observed), reads) =>
          option_map
            (fun '((final, observations), later_reads) =>
              ((final, observed :: observations), reads ++ later_reads))
            (observed_scalar_run position read_byte remaining next)
      end
  end.

Lemma split_located_source_has_exact_provenance position read_byte index :
  byte_provenance (split_located_source position read_byte index) = position index.
Proof.
  unfold split_located_source. destruct (position index); reflexivity.
Qed.

Theorem observed_scalar_next_refines_exact_event_and_reads position read_byte current :
  observed_scalar_next position read_byte current =
  option_map (fun '(next, observed) =>
    ((next, observed), event_source_reads observed))
    (scalar_next (split_located_source position read_byte) current).
Proof.
  unfold observed_scalar_next, scalar_next.
  destruct (cursor_phase current); try reflexivity;
    destruct (S (cursor_index current) <? cursor_length current) eqn:forward_guard;
    try destruct (cursor_index current <? cursor_length current) eqn:reverse_guard;
    cbn; repeat rewrite split_located_source_has_exact_provenance; reflexivity.
Qed.

Theorem metadata_steps_do_not_depend_on_byte_fetch position left right current :
  cursor_phase current <> LineEdge ->
  observed_scalar_next position left current =
    observed_scalar_next position right current.
Proof.
  intro metadata. unfold observed_scalar_next.
  destruct (cursor_phase current); [contradiction | reflexivity | reflexivity | reflexivity].
Qed.

Theorem observed_scalar_run_refines_exact_events_and_reads position read_byte count :
  forall current,
  observed_scalar_run position read_byte count current =
  option_map (fun '(final, observations) =>
    ((final, observations), events_source_reads observations))
    (scalar_run (split_located_source position read_byte) count current).
Proof.
  induction count as [|remaining IH]; intro current; [reflexivity |].
  cbn [observed_scalar_run scalar_run].
  rewrite observed_scalar_next_refines_exact_event_and_reads.
  destruct (scalar_next (split_located_source position read_byte) current)
    as [[next observed] |]; [|reflexivity].
  cbn. rewrite IH.
  destruct (scalar_run (split_located_source position read_byte) remaining next)
    as [[final observations] |]; reflexivity.
Qed.

Lemma forward_events_read_exact_suffix position read_byte index width :
  events_source_reads
    (forward_events (split_located_source position read_byte) index width) =
    map position (seq index width).
Proof.
  revert index. induction width; intro index; [reflexivity |].
  cbn [forward_events events_source_reads event_source_reads seq map].
  rewrite split_located_source_has_exact_provenance, IHwidth. reflexivity.
Qed.

Lemma leave_events_have_no_source_reads source size index width :
  events_source_reads (leave_events source size index width) = [].
Proof.
  revert index. induction width; intro index; [reflexivity |].
  cbn [leave_events events_source_reads event_source_reads]. apply IHwidth.
Qed.

Theorem observed_forward_reads_each_interior_suffix_byte_once
    position read_byte width current :
  cursor_phase current = LineEdge ->
  cursor_index current + width < cursor_length current ->
  observed_scalar_run position read_byte (2 * width) current =
    Some ((forward (split_located_source position read_byte) width current,
      forward_events (split_located_source position read_byte)
        (cursor_index current) width),
      map position (seq (cursor_index current) width)).
Proof.
  intros stage bounded. rewrite observed_scalar_run_refines_exact_events_and_reads.
  rewrite checked_line_expands_to_exact_scalar_events by assumption.
  cbn [option_map]. rewrite forward_events_read_exact_suffix. reflexivity.
Qed.

Theorem observed_unwind_performs_no_source_reads position read_byte width current :
  cursor_phase current = LineLeave ->
  cursor_index current + width <= cursor_length current ->
  observed_scalar_run position read_byte width current =
    Some ((unwind (split_located_source position read_byte) width current,
      leave_events (split_located_source position read_byte)
        (cursor_length current) (cursor_index current) width), []).
Proof.
  intros stage bounded. rewrite observed_scalar_run_refines_exact_events_and_reads.
  rewrite checked_unwind_expands_to_exact_scalar_leaves by assumption.
  cbn [option_map]. rewrite leave_events_have_no_source_reads. reflexivity.
Qed.

Fixpoint observed_rewinds (count : nat) (current : cursor)
    : cursor * list provenance :=
  match count with
  | 0 => (current, [])
  | S remaining => observed_rewinds remaining (rewind current)
  end.

Theorem finite_rewinds_preserve_source_free_generation_authority count current :
  snd (observed_rewinds count current) = [] /\
  cursor_generation (fst (observed_rewinds count current)) =
    count + cursor_generation current /\
  cursor_identity (fst (observed_rewinds count current)) = cursor_identity current /\
  cursor_record (fst (observed_rewinds count current)) = cursor_record current /\
  cursor_events (fst (observed_rewinds count current)) = cursor_events current.
Proof.
  revert current. induction count; intro current; [repeat split; reflexivity |].
  cbn [observed_rewinds]. specialize (IHcount (rewind current)).
  destruct IHcount as [reads [generation [identity [record events]]]].
  split; [exact reads |].
  split.
  - rewrite generation.
    change (count + S (cursor_generation current) =
      S (count + cursor_generation current)).
    apply Nat.add_succ_r.
  - split; [exact identity |]. split; [exact record | exact events].
Qed.

Print Assumptions observed_scalar_next_refines_exact_event_and_reads.
Print Assumptions metadata_steps_do_not_depend_on_byte_fetch.
Print Assumptions observed_scalar_run_refines_exact_events_and_reads.
Print Assumptions observed_forward_reads_each_interior_suffix_byte_once.
Print Assumptions observed_unwind_performs_no_source_reads.
Print Assumptions finite_rewinds_preserve_source_free_generation_authority.
Print Assumptions pending_children_never_admit_reverse_transport.
Print Assumptions reverse_normalization_changes_only_the_unobservable_phase.
Print Assumptions accepted_reverse_entry_commit_matches_the_exact_scalar_trace.
Print Assumptions reverse_entry_refuses_zero_or_excessive_width_before_normalization.
Print Assumptions checked_line_expands_to_exact_scalar_events.
Print Assumptions checked_unwind_expands_to_exact_scalar_leaves.
Print Assumptions stale_offer_refuses_without_a_new_cursor.
Print Assumptions zero_width_offer_is_refused.
Print Assumptions rewind_invalidates_an_old_offer.
Print Assumptions every_forward_prefix_keeps_the_final_byte_scalar.
Print Assumptions every_unwind_prefix_stops_before_the_node_leave.
Print Assumptions committed_forward_preserves_other_topology_authority.
Print Assumptions committed_unwind_preserves_other_topology_authority.
Print Assumptions accepted_commit_has_the_exact_scalar_trace.
Print Assumptions odd_forward_prefix_preserves_the_pending_absent_value.
Print Assumptions expanded_event_counts_preserve_logical_work.

End CheckedACTLineTransport.

Module NormalizedSourceSpans.

Import CanonicalKeyProtocol IndexedWindowTransport.

Definition adjacent (left right : window) :=
  (window_owner left =? window_owner right) &&
  (window_start left + window_width left =? window_start right).

Definition combined (left right : window) :=
  Window (window_owner left) (window_start left)
    (window_width left + window_width right) (window_anchor right).

Definition coalesce (limit : nat) (left right : window) : option window :=
  if adjacent left right &&
      (window_start left + window_width left + window_width right <=? limit)
  then Some (combined left right)
  else None.

Lemma adjacent_exact left right :
  adjacent left right = true <->
  window_owner left = window_owner right /\
  window_start left + window_width left = window_start right.
Proof. unfold adjacent. rewrite andb_true_iff, !Nat.eqb_eq. reflexivity. Qed.

Lemma split_prefix {A : Type} (input : list A) left right :
  firstn (left + right) input =
    firstn left input ++ firstn right (skipn left input).
Proof.
  revert input. induction left; intros input; cbn; [reflexivity|].
  destruct input; cbn; [now rewrite firstn_nil|]. now rewrite IHleft.
Qed.

Theorem coalescing_accepts_exact_identity_adjacency_and_bounds limit left right result :
  coalesce limit left right = Some result <->
  window_owner left = window_owner right /\
  window_start left + window_width left = window_start right /\
  window_start left + window_width left + window_width right <= limit /\
  result = combined left right.
Proof.
  unfold coalesce. destruct (adjacent left right &&
      (window_start left + window_width left + window_width right <=? limit)) eqn:guard.
  - rewrite andb_true_iff, adjacent_exact, Nat.leb_le in guard.
    split; [intro accepted; inversion accepted; tauto|intros (_ & _ & _ & ->); reflexivity].
  - split; [discriminate|]. intros (same & next & fits & ->).
    assert (adjacent left right = true) by (apply adjacent_exact; tauto).
    assert (window_start left + window_width left + window_width right <=? limit = true)
      by (apply Nat.leb_le; exact fits).
    rewrite H, H0 in guard. discriminate.
Qed.

Theorem coalescing_preserves_exact_located_bytes limit root left right result :
  coalesce limit left right = Some result ->
  window_bytes root result = window_bytes root left ++ window_bytes root right.
Proof.
  intro accepted. apply coalescing_accepts_exact_identity_adjacency_and_bounds in accepted.
  destruct accepted as (same & next & fits & ->).
  unfold window_bytes, combined; cbn.
  rewrite split_prefix, skipn_skipn.
  replace (window_width left + window_start left) with (window_start right) by lia.
  reflexivity.
Qed.

Theorem coalescing_preserves_root_validity_and_the_right_end limit root left right result :
  window_valid root left -> window_valid root right ->
  coalesce limit left right = Some result ->
  window_valid root result /\
  window_end root result = window_end root right /\
  window_start result + window_width result <= limit.
Proof.
  intros [left_owner left_bound] [right_owner right_bound] accepted.
  apply coalescing_accepts_exact_identity_adjacency_and_bounds in accepted.
  destruct accepted as (same & next & fits & ->).
  unfold window_valid, combined, window_end; cbn.
  split; [split; [exact left_owner|lia]|]. split; [|lia].
  replace (window_start left + (window_width left + window_width right))
    with (window_start right + window_width right) by lia.
  reflexivity.
Qed.

Theorem different_backing_identity_never_coalesces limit left right :
  window_owner left <> window_owner right -> coalesce limit left right = None.
Proof.
  intro different. unfold coalesce, adjacent.
  assert (window_owner left =? window_owner right = false) by (apply Nat.eqb_neq; exact different).
  now rewrite H.
Qed.

Theorem coalescing_is_associative_without_intermediate_source_layers left middle right :
  combined (combined left middle) right = combined left (combined middle right).
Proof. unfold combined; cbn. f_equal; lia. Qed.

Theorem repeated_subviews_normalize_to_the_original_source
    limit original a b middle c d final :
  checked_subwindow limit original a b = Some middle ->
  checked_subwindow limit middle c d = Some final ->
  checked_subwindow limit original (a + c) (a + d) = Some final.
Proof.
  intros outer inner. apply checked_subwindow_exact in outer.
  destruct outer as (ordered & bounded & fits & ->).
  apply checked_subwindow_exact in inner. cbn in inner.
  destruct inner as (inner_ordered & inner_bounded & inner_fits & ->).
  apply checked_subwindow_exact. repeat split; try lia.
  f_equal; try lia.
  destruct (d =? b - a) eqn:inner_end;
    destruct (b =? window_width original) eqn:outer_end;
    destruct (a + d =? window_width original) eqn:direct_end; try reflexivity;
    repeat match goal with
    | H : (_ =? _) = true |- _ => apply Nat.eqb_eq in H
    | H : (_ =? _) = false |- _ => apply Nat.eqb_neq in H
    end; exfalso; lia.
Qed.

Theorem normalized_subviews_preserve_each_physical_position
    limit root parent start stop child offset byte :
  window_valid root parent ->
  checked_subwindow limit parent start stop = Some child ->
  offset < stop - start ->
  nth_error (window_bytes root parent) (start + offset) = Some byte ->
  window_position root child offset = Some (Position offset (byte_provenance byte) BytePosition).
Proof.
  intros valid accepted within found.
  pose proof (@nested_windows_preserve_owner_bounds_and_exact_slice
    limit root parent start stop child valid accepted) as [child_valid [_ [_ bytes]]].
  apply checked_subwindow_exact in accepted.
  destruct accepted as (ordered & bounded & fits & shape).
  apply indexed_byte_positions_preserve_exact_physical_provenance.
  - rewrite shape; cbn; exact within.
  - rewrite bytes, nth_error_firstn.
    assert (offset <? stop - start = true) by (apply Nat.ltb_lt; exact within).
    rewrite H, nth_error_skipn. exact found.
Qed.

Theorem coalesced_boundary_scalar_remains_in_the_original_backing
    limit root prefix terminal result byte :
  window_valid root prefix -> window_valid root terminal ->
  window_width terminal = 1 ->
  nth_error (region_bytes root) (window_start terminal) = Some byte ->
  coalesce limit prefix terminal = Some result ->
  window_owner result = window_owner prefix /\
  window_bytes root result = window_bytes root prefix ++ [byte] /\
  window_end root result = window_end root terminal.
Proof.
  intros prefix_valid terminal_valid one found accepted.
  pose proof (@coalescing_preserves_root_validity_and_the_right_end
    limit root prefix terminal result prefix_valid terminal_valid accepted)
    as [_ [same_end _]].
  pose proof (@coalescing_preserves_exact_located_bytes limit root prefix terminal result accepted)
    as same_bytes.
  apply coalescing_accepts_exact_identity_adjacency_and_bounds in accepted.
  destruct accepted as (_ & _ & _ & ->).
  split; [reflexivity|]. split; [|exact same_end].
  rewrite same_bytes. unfold window_bytes at 2. rewrite one.
  rewrite <- (Nat.add_0_r (window_start terminal)) in found.
  rewrite <- nth_error_skipn in found.
  destruct (skipn (window_start terminal) (region_bytes root)) as [|head rest];
    cbn in found; [discriminate|]. inversion found; subst. reflexivity.
Qed.

Definition source_slot := nat.
Definition source_store := list region.

Definition retain_source (stored : source_store) (root : region) :=
  (stored ++ [root], length stored).

Theorem retaining_a_source_preserves_every_existing_backing stored root slot previous :
  nth_error stored slot = Some previous ->
  nth_error (fst (retain_source stored root)) slot = Some previous.
Proof.
  intro found. unfold retain_source; cbn.
  rewrite nth_error_app1; [exact found|].
  apply nth_error_Some. rewrite found. discriminate.
Qed.

Theorem retained_source_slot_denotes_the_original_root stored root :
  nth_error (fst (retain_source stored root)) (snd (retain_source stored root)) = Some root.
Proof. unfold retain_source; cbn. rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. reflexivity. Qed.

Print Assumptions coalescing_accepts_exact_identity_adjacency_and_bounds.
Print Assumptions coalescing_preserves_exact_located_bytes.
Print Assumptions coalescing_preserves_root_validity_and_the_right_end.
Print Assumptions different_backing_identity_never_coalesces.
Print Assumptions coalescing_is_associative_without_intermediate_source_layers.
Print Assumptions repeated_subviews_normalize_to_the_original_source.
Print Assumptions normalized_subviews_preserve_each_physical_position.
Print Assumptions coalesced_boundary_scalar_remains_in_the_original_backing.
Print Assumptions retaining_a_source_preserves_every_existing_backing.
Print Assumptions retained_source_slot_denotes_the_original_root.

End NormalizedSourceSpans.

Module AbsolutePayloadRealization.

Module T := PersistentFrameRealization.
Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.
Module D := PersistentFaultRealization.
Module O := StrictOpaqueInterior.
Import CanonicalKeyProtocol.

Inductive physical_phase := PayloadMarker | AwaitingMarker (endpoint : T.source_location).

Section Payloads.
Context {Span : Type}.

Record physical_leaf := PhysicalLeaf {
  leaf_metadata : F.path_metadata;
  leaf_slot : nat;
  leaf_obligation : T.obligation_snapshot;
  leaf_phase : physical_phase;
  leaf_payload_start : nat;
  leaf_next : @T.frame_tree Span
}.

Definition payload_end current :=
  leaf_payload_start current + T.obligation_payload_len (leaf_obligation current).

Definition realize_phase stamp current :=
  match leaf_phase current with
  | PayloadMarker => T.PayloadPhase (payload_end current - stamp)
  | AwaitingMarker endpoint => T.AwaitingPhase endpoint
  end.

Definition realize_leaf stamp current :=
  T.SemanticPath (leaf_metadata current)
    (T.InactiveTree (leaf_slot current)
      (T.ObligationPayload (leaf_obligation current) (realize_phase stamp current)
        (leaf_payload_start current)) (leaf_next current)).

Fixpoint countdown_tree width (tree : @T.frame_tree Span) :=
  match tree with
  | T.InactiveTree slot (T.ObligationPayload obligation (T.PayloadPhase remaining) start) next =>
      T.InactiveTree slot (T.ObligationPayload obligation (T.PayloadPhase (remaining - width)) start) next
  | T.RegionTree slot remaining previous declared path_remaining start metadata child next =>
      T.RegionTree slot (remaining - width) previous declared (path_remaining - width) start metadata
        (countdown_tree width child) next
  | other => other
  end.

Definition typed_countdown width current :=
  T.SemanticPath (T.semantic_metadata current) (countdown_tree width (T.semantic_frames current)).

Fixpoint strict_tree width (tree : @T.frame_tree Span) : Prop :=
  match tree with
  | T.InactiveTree _ (T.ObligationPayload _ (T.PayloadPhase remaining) _) _ => width < remaining
  | T.RegionTree _ remaining _ _ path_remaining _ metadata child _ =>
      width < remaining /\ width < path_remaining /\
      F.path_status_value metadata = F.OpenPath /\ strict_tree width child
  | _ => False
  end.

Definition strict_graph width current :=
  F.path_status_value (T.semantic_metadata current) = F.OpenPath /\
  strict_tree width (T.semantic_frames current).

Definition strict_owner stamp width (owner : @T.held_owner Span) :=
  F.path_status_value (T.held_metadata owner) = F.OpenPath /\
  stamp + width < T.held_region_end owner /\ stamp + width < T.held_path_end owner.

Definition strict_window stamp width current owners :=
  leaf_phase current = PayloadMarker /\
  F.path_status_value (leaf_metadata current) = F.OpenPath /\
  leaf_payload_start current <= stamp /\ stamp + width < payload_end current /\
  Forall (strict_owner stamp width) owners.

Definition realize_graph stamp current owners := T.reify_owners stamp (realize_leaf stamp current) owners.

Theorem physical_payload_realizes_all_existing_typed_obligation_fields stamp current :
  leaf_phase current = PayloadMarker ->
  T.semantic_metadata (realize_leaf stamp current) = leaf_metadata current /\
  T.semantic_frames (realize_leaf stamp current) =
    T.InactiveTree (leaf_slot current)
      (T.ObligationPayload (leaf_obligation current) (T.PayloadPhase (payload_end current - stamp))
        (leaf_payload_start current)) (leaf_next current).
Proof. intro phase. unfold realize_leaf, realize_phase. rewrite phase. split; reflexivity. Qed.

Lemma leaf_stamp_advance_is_typed_countdown stamp width current :
  leaf_phase current = PayloadMarker ->
  realize_leaf (stamp + width) current = typed_countdown width (realize_leaf stamp current).
Proof.
  intro phase. unfold realize_leaf, realize_phase, typed_countdown. rewrite phase.
  cbn. f_equal. f_equal. f_equal. f_equal. lia.
Qed.

Lemma wrapping_an_owner_commutes_with_typed_countdown stamp width owner current :
  typed_countdown width (T.wrap_owner stamp owner current) =
  T.wrap_owner (stamp + width) owner (typed_countdown width current).
Proof.
  destruct current. unfold typed_countdown, T.wrap_owner. cbn.
  f_equal. f_equal; lia.
Qed.

Lemma owner_spine_commutes_with_typed_countdown stamp width current owners :
  typed_countdown width (T.reify_owners stamp current owners) =
  T.reify_owners (stamp + width) (typed_countdown width current) owners.
Proof.
  revert current. induction owners; intro current; cbn; [reflexivity|].
  rewrite IHowners, wrapping_an_owner_commutes_with_typed_countdown. reflexivity.
Qed.

Theorem absolute_stamp_advance_changes_exactly_the_typed_payload_and_owner_residuals
    stamp width current owners :
  leaf_phase current = PayloadMarker ->
  realize_graph (stamp + width) current owners = typed_countdown width (realize_graph stamp current owners).
Proof.
  intro phase. unfold realize_graph. rewrite owner_spine_commutes_with_typed_countdown.
  now rewrite leaf_stamp_advance_is_typed_countdown.
Qed.

Lemma strict_owner_spine stamp width current owners :
  strict_graph width current -> Forall (strict_owner stamp width) owners ->
  strict_graph width (T.reify_owners stamp current owners).
Proof.
  revert current. induction owners as [|owner rest IH]; intros current strict valid; cbn; [exact strict|].
  inversion valid as [|? ? held tail]; subst. apply IH; [|exact tail].
  destruct strict as [live bounded]. destruct held as [owner_live [region path]].
  unfold strict_graph, T.wrap_owner. cbn. split; [exact owner_live|].
  repeat split; try assumption; lia.
Qed.

Theorem strict_absolute_window_realizes_a_live_typed_graph stamp width current owners :
  strict_window stamp width current owners -> strict_graph width (realize_graph stamp current owners).
Proof.
  intros [phase [live [started [bounded owners_valid]]]].
  unfold realize_graph. apply strict_owner_spine; [|exact owners_valid].
  unfold strict_graph, realize_leaf, realize_phase. rewrite phase. cbn. split; [exact live|lia].
Qed.

Lemma strict_window_prefix stamp width prefix rest current owners :
  strict_window stamp width current owners -> prefix + rest <= width ->
  strict_window (stamp + prefix) rest current owners.
Proof.
  intros [phase [live [started [bounded owners_valid]]]] inside.
  repeat split; try assumption; try lia.
  eapply Forall_impl; [|exact owners_valid]. intros owner [owner_live [region path]].
  unfold strict_owner. repeat split; try assumption; lia.
Qed.

Theorem every_historical_absolute_prefix_retains_all_strict_typed_boundaries
    stamp width prefix current owners :
  strict_window stamp width current owners -> prefix <= width ->
  strict_graph (width - prefix) (realize_graph (stamp + prefix) current owners).
Proof.
  intros valid inside. apply strict_absolute_window_realizes_a_live_typed_graph.
  eapply strict_window_prefix; [exact valid|lia].
Qed.

Theorem strict_absolute_prefix_preserves_physical_continuations_and_held_owners
    frames suffix stamp width prefix current owners :
  strict_window stamp width current owners -> prefix <= width ->
  T.physical_tree frames (leaf_next current) ->
  Forall (T.owner_realizes frames stamp) owners ->
  T.physical_tree (frames ++ suffix) (leaf_next current) /\
  Forall (T.owner_realizes (frames ++ suffix) (stamp + prefix)) owners /\
  strict_graph (width - prefix) (realize_graph (stamp + prefix) current owners).
Proof.
  intros valid inside continuation held.
  split; [now apply T.typed_physical_frames_survive_arbitrary_append|]. split.
  - destruct valid as [_ [_ [_ [_ bounded]]]].
    rewrite Forall_forall in held, bounded |- *. intros owner present.
    specialize (held owner present). specialize (bounded owner present).
    destruct bounded as [_ [region path]]. apply T.owner_realizes_append.
    pose proof (@T.advancing_a_valid_graph_stamp_preserves_held_cells_and_decreases_residuals
      Span frames stamp (stamp + prefix) owner held) as advanced.
    apply advanced; lia.
  - now apply every_historical_absolute_prefix_retains_all_strict_typed_boundaries.
Qed.

Theorem absolute_payload_preserves_the_exact_typed_private_terminal stamp current owners :
  leaf_phase current = PayloadMarker ->
  let obligation := leaf_obligation current in
  let '(source, offset) := T.obligation_tag_source obligation in
  T.path_private_terminal (realize_graph stamp current owners) =
    P.TerminalObligation (T.obligation_id obligation) source offset false.
Proof.
  intro phase. unfold realize_graph. rewrite T.typed_overlay_private_terminal_is_the_selected_focus_terminal.
  unfold T.path_private_terminal, realize_leaf, realize_phase. rewrite phase. cbn.
  destruct (T.obligation_tag_source (leaf_obligation current)). reflexivity.
Qed.

Theorem absolute_payload_private_preflight_keeps_exact_id_phase_and_source
    stamp current owners requested caller :
  leaf_phase current = PayloadMarker ->
  let obligation := leaf_obligation current in
  let '(source, offset) := T.obligation_tag_source obligation in
  P.resolve_preflight requested (T.path_private_terminal (realize_graph stamp current owners)) = Some (source, offset) /\
  P.reject_preflight requested caller (T.path_private_terminal (realize_graph stamp current owners)) =
    (if requested =? T.obligation_id obligation then None else Some (source, offset)).
Proof.
  intro phase. pose proof (@absolute_payload_preserves_the_exact_typed_private_terminal stamp current owners phase) as terminal.
  cbn zeta in terminal |- *.
  destruct (T.obligation_tag_source (leaf_obligation current)) as [source offset]. rewrite terminal.
  unfold P.resolve_preflight, P.reject_preflight. rewrite andb_false_r. split; reflexivity.
Qed.

Inductive strict_scalar_step : @T.semantic_path Span -> located_byte -> @T.semantic_path Span -> O.event -> Prop :=
| StrictScalarStep current byte id source offset :
    strict_graph 1 current ->
    T.path_private_terminal current = P.TerminalObligation id source offset false ->
    strict_scalar_step current byte (typed_countdown 1 current) (O.PayloadByte id byte false).

Inductive strict_scalar_run : list located_byte -> @T.semantic_path Span -> @T.semantic_path Span -> list O.event -> Prop :=
| StrictScalarNil current : strict_scalar_run [] current current []
| StrictScalarCons byte rest current next final observed observations :
    strict_scalar_step current byte next observed ->
    strict_scalar_run rest next final observations ->
    strict_scalar_run (byte :: rest) current final (observed :: observations).

Theorem strict_absolute_window_refines_repeated_typed_scalar_payload_steps
    input stamp current owners :
  strict_window stamp (length input) current owners ->
  strict_scalar_run input (realize_graph stamp current owners)
    (realize_graph (stamp + length input) current owners)
    (map (fun byte => O.PayloadByte (T.obligation_id (leaf_obligation current)) byte false) input).
Proof.
  revert stamp. induction input as [|byte rest IH]; intros stamp valid.
  - cbn. rewrite Nat.add_0_r. constructor.
  - assert (one : strict_window stamp 1 current owners).
    { replace stamp with (stamp + 0) by lia. eapply strict_window_prefix; [exact valid|cbn; lia]. }
    assert (later : strict_window (stamp + 1) (length rest) current owners).
    { eapply strict_window_prefix; [exact valid|cbn; lia]. }
    pose proof (IH (stamp + 1) later) as tail.
    replace (stamp + 1 + length rest) with (stamp + length (byte :: rest)) in tail by (cbn; lia).
    cbn [map]. eapply StrictScalarCons; [|exact tail].
    rewrite absolute_stamp_advance_changes_exactly_the_typed_payload_and_owner_residuals by exact (proj1 valid).
    destruct (T.obligation_tag_source (leaf_obligation current)) as [source offset] eqn:source_is.
    eapply StrictScalarStep.
    + now apply strict_absolute_window_realizes_a_live_typed_graph.
    + pose proof (@absolute_payload_preserves_the_exact_typed_private_terminal stamp current owners (proj1 valid)) as terminal.
      cbn zeta in terminal.
      rewrite source_is in terminal. exact terminal.
Qed.

Record checkpoint := Checkpoint {
  checkpoint_leaf : physical_leaf;
  checkpoint_owners : list (@T.held_owner Span);
  checkpoint_saved_stamp : nat;
  checkpoint_fault : option P.fault_error
}.

Definition selected_stamp current byte_length :=
  match checkpoint_fault current with
  | Some _ => checkpoint_saved_stamp current
  | None =>
      match F.path_status_value (leaf_metadata (checkpoint_leaf current)) with
      | F.DeadPath _ _ _ => checkpoint_saved_stamp current
      | _ => byte_length
      end
  end.

Definition graph_at stamp current :=
  let graph := realize_graph stamp (checkpoint_leaf current) (checkpoint_owners current) in
  match checkpoint_fault current with None => graph | Some error => D.kill_typed graph error end.

Definition selected_graph current byte_length := graph_at (selected_stamp current byte_length) current.

Theorem live_absolute_checkpoint_uses_the_selected_byte_prefix current byte_length :
  checkpoint_fault current = None ->
  F.path_status_value (leaf_metadata (checkpoint_leaf current)) = F.OpenPath ->
  selected_graph current byte_length = graph_at byte_length current.
Proof. intros no_fault live. unfold selected_graph, selected_stamp. now rewrite no_fault, live. Qed.

Theorem saved_fault_stamp_preserves_the_entire_typed_graph_under_dead_edges current first later error :
  checkpoint_fault current = Some error ->
  selected_graph current first = graph_at (checkpoint_saved_stamp current) current /\
  selected_graph current later = selected_graph current first.
Proof.
  intro fault. unfold selected_graph, selected_stamp. rewrite fault. split; reflexivity.
Qed.

Theorem saved_direct_dead_stamp_is_required_even_without_a_fault_field
    current first later code source offset :
  checkpoint_fault current = None ->
  F.path_status_value (leaf_metadata (checkpoint_leaf current)) = F.DeadPath code source offset ->
  selected_graph current first = graph_at (checkpoint_saved_stamp current) current /\
  selected_graph current later = selected_graph current first.
Proof.
  intros no_fault dead. unfold selected_graph, selected_stamp. rewrite no_fault, dead. split; reflexivity.
Qed.

Theorem publishing_any_checkpoint_preserves_every_historical_prefix_and_shared_alias
    stored suffix slot previous :
  nth_error stored slot = Some previous ->
  nth_error (stored ++ suffix) slot = Some previous /\
  forall byte_length,
    option_map (fun current => selected_graph current byte_length) (nth_error (stored ++ suffix) slot) =
    Some (selected_graph previous byte_length).
Proof.
  intro found. assert (retained : nth_error (stored ++ suffix) slot = Some previous).
  { eapply P.old_lookup_after_append; exact found. }
  split; [exact retained|]. intro byte_length. now rewrite retained.
Qed.

Theorem explicit_awaiting_phase_keeps_its_original_physical_endpoint stamp later current endpoint :
  leaf_phase current = AwaitingMarker endpoint ->
  realize_leaf stamp current = realize_leaf later current /\
  realize_phase later current = T.AwaitingPhase endpoint.
Proof.
  intro pending. unfold realize_leaf, realize_phase. rewrite pending. split; reflexivity.
Qed.

End Payloads.

Definition control_metadata := F.PathMetadata true F.OpenPath 0 None true.
Definition control_obligation := T.ObligationSnapshot 17 T.RawParEscape 4 (7, 11).
Definition control_leaf := @PhysicalLeaf unit control_metadata 3 control_obligation PayloadMarker 5 T.EmptyTree.
Definition control_fault := P.FaultError 9 7 12.
Definition control_dead := @Checkpoint unit control_leaf [] 5 (Some control_fault).

Theorem moving_an_ownerless_dead_byte_stamp_changes_its_hidden_typed_payload :
  graph_at 5 control_dead <> graph_at 7 control_dead /\
  selected_graph control_dead 5 = selected_graph control_dead 7.
Proof. split; [discriminate|reflexivity]. Qed.

Theorem dropping_the_stamp_after_folding_a_fault_into_direct_metadata_is_not_sound :
  let killed := @PhysicalLeaf unit (P.kill_metadata control_metadata control_fault) 3
    control_obligation PayloadMarker 5 T.EmptyTree in
  let direct_dead := @Checkpoint unit killed [] 5 None in
  graph_at 5 direct_dead <> graph_at 7 direct_dead /\
  selected_graph direct_dead 5 = selected_graph direct_dead 7.
Proof. split; [discriminate|reflexivity]. Qed.

Print Assumptions physical_payload_realizes_all_existing_typed_obligation_fields.
Print Assumptions absolute_stamp_advance_changes_exactly_the_typed_payload_and_owner_residuals.
Print Assumptions strict_absolute_window_realizes_a_live_typed_graph.
Print Assumptions every_historical_absolute_prefix_retains_all_strict_typed_boundaries.
Print Assumptions strict_absolute_prefix_preserves_physical_continuations_and_held_owners.
Print Assumptions absolute_payload_preserves_the_exact_typed_private_terminal.
Print Assumptions absolute_payload_private_preflight_keeps_exact_id_phase_and_source.
Print Assumptions strict_absolute_window_refines_repeated_typed_scalar_payload_steps.
Print Assumptions live_absolute_checkpoint_uses_the_selected_byte_prefix.
Print Assumptions saved_fault_stamp_preserves_the_entire_typed_graph_under_dead_edges.
Print Assumptions saved_direct_dead_stamp_is_required_even_without_a_fault_field.
Print Assumptions publishing_any_checkpoint_preserves_every_historical_prefix_and_shared_alias.
Print Assumptions explicit_awaiting_phase_keeps_its_original_physical_endpoint.
Print Assumptions moving_an_ownerless_dead_byte_stamp_changes_its_hidden_typed_payload.
Print Assumptions dropping_the_stamp_after_folding_a_fault_into_direct_metadata_is_not_sound.

End AbsolutePayloadRealization.

Module AbsolutePayloadCheckpointBoundary.

Module A := AbsolutePayloadRealization.
Module T := PersistentFrameRealization.
Module F := FocusedRegionOwners.
Module P := PersistentFocusedOwners.
Module C := CompactPersistentCheckpoints.
Module D := PersistentFaultRealization.
Import CanonicalKeyProtocol IndexedWindowTransport.

Section Storage.
Context {Span : Type}.

Definition dead_leaf (current : @A.physical_leaf Span) :=
  match F.path_status_value (A.leaf_metadata current) with F.DeadPath _ _ _ => true | _ => false end.

Definition needs_link (current : @A.checkpoint Span) :=
  match A.checkpoint_owners current, A.checkpoint_fault current with
  | [], None => dead_leaf (A.checkpoint_leaf current)
  | _, _ => true
  end.

Definition retime position (current : @A.checkpoint Span) :=
  A.Checkpoint (A.checkpoint_leaf current) (A.checkpoint_owners current)
    (A.selected_stamp current position) (A.checkpoint_fault current).

Definition plain current position := @A.Checkpoint Span current [] position None.

Record storage := Storage {
  direct : list (@A.physical_leaf Span);
  linked : list (@A.checkpoint Span)
}.

Definition lookup stored address position :=
  match address with
  | C.DirectSlot slot => option_map (fun current => plain current position) (nth_error (direct stored) slot)
  | C.LinkedSlot slot => option_map (retime position) (nth_error (linked stored) slot)
  end.

Definition publish stored current :=
  if needs_link current then
    (Storage (direct stored) (linked stored ++ [current]), C.LinkedSlot (length (linked stored)))
  else
    (Storage (direct stored ++ [A.checkpoint_leaf current]) (linked stored), C.DirectSlot (length (direct stored))).

Theorem direct_publication_requires_no_owners_no_fault_and_nondead_metadata current :
  needs_link current = false <->
  A.checkpoint_owners current = [] /\ A.checkpoint_fault current = None /\
  dead_leaf (A.checkpoint_leaf current) = false.
Proof.
  destruct current as [leaf owners stamp fault]. unfold needs_link; cbn.
  destruct owners, fault; cbn; split; intro valid; try discriminate; intuition congruence.
Qed.

Theorem retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields current first later :
  A.selected_stamp (retime first current) later = A.selected_stamp current later /\
  A.selected_graph (retime first current) later = A.selected_graph current later /\
  A.checkpoint_leaf (retime first current) = A.checkpoint_leaf current /\
  A.checkpoint_owners (retime first current) = A.checkpoint_owners current /\
  A.checkpoint_fault (retime first current) = A.checkpoint_fault current.
Proof.
  destruct current as [leaf owners stamp fault].
  unfold retime, A.selected_graph, A.selected_stamp, A.graph_at; cbn.
  destruct fault; [repeat split; reflexivity|].
  destruct (F.path_status_value (A.leaf_metadata leaf)); repeat split; reflexivity.
Qed.

Lemma direct_retiming_is_plain current position :
  needs_link current = false -> retime position current = plain (A.checkpoint_leaf current) position.
Proof.
  rewrite direct_publication_requires_no_owners_no_fault_and_nondead_metadata.
  destruct current as [leaf owners stamp fault]. cbn. intros [-> [-> nondead]].
  unfold retime, plain, A.selected_stamp, dead_leaf in *. cbn in *.
  destruct (F.path_status_value (A.leaf_metadata leaf)); congruence.
Qed.

Theorem publication_returns_exact_fields_at_the_selected_byte_stamp stored current position :
  let '(next, address) := publish stored current in
  lookup next address position = Some (retime position current).
Proof.
  unfold publish. destruct (needs_link current) eqn:shape; unfold lookup; cbn.
  - now rewrite C.last_lookup.
  - rewrite C.last_lookup. cbn. now rewrite direct_retiming_is_plain.
Qed.

Definition valid_selected (frames : list (@T.stored_frame Span)) (current : @A.checkpoint Span) position :=
  let stamp := A.selected_stamp current position in
  let leaf := A.checkpoint_leaf current in
  T.physical_tree frames (A.leaf_next leaf) /\
  Forall (T.owner_realizes frames stamp) (A.checkpoint_owners current) /\
  match A.leaf_phase leaf with
  | A.PayloadMarker => A.leaf_payload_start leaf <= stamp /\ stamp < A.payload_end leaf
  | A.AwaitingMarker _ => stamp = A.payload_end leaf
  end.

Lemma valid_selected_survives_retiming frames current first later :
  valid_selected frames (retime first current) later <-> valid_selected frames current later.
Proof.
  unfold valid_selected.
  rewrite (proj1 (retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields current first later)).
  reflexivity.
Qed.

Theorem publication_preserves_valid_selected_typed_interpretation frames stored current position :
  valid_selected frames current position ->
  let '(next, address) := publish stored current in
  exists selected,
    lookup next address position = Some selected /\ valid_selected frames selected position /\
    A.selected_graph selected position = A.selected_graph current position.
Proof.
  intro valid. pose proof (publication_returns_exact_fields_at_the_selected_byte_stamp stored current position) as found.
  destruct (publish stored current) as [next address]. exists (retime position current).
  split; [exact found|]. split.
  - now apply valid_selected_survives_retiming.
  - exact (proj1 (proj2 (retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields current position position))).
Qed.

Theorem every_dead_or_fault_checkpoint_retains_its_saved_stamp_in_linked_storage stored current :
  (exists error, A.checkpoint_fault current = Some error) \/ dead_leaf (A.checkpoint_leaf current) = true ->
  publish stored current =
    (Storage (direct stored) (linked stored ++ [current]), C.LinkedSlot (length (linked stored))) /\
  forall position, lookup (fst (publish stored current)) (snd (publish stored current)) position = Some current.
Proof.
  intro frozen.
  assert (link : needs_link current = true).
  { destruct current as [leaf owners stamp fault]. unfold needs_link; cbn in *.
    destruct owners, fault; cbn in *; try reflexivity.
    destruct frozen as [[error impossible]|dead]; [discriminate|exact dead]. }
  assert (same : forall position, retime position current = current).
  { destruct current as [leaf owners stamp fault]. unfold retime, A.selected_stamp; cbn in *.
    destruct fault; [reflexivity|]. destruct frozen as [[error impossible]|dead]; [discriminate|].
    unfold dead_leaf in dead. destruct (F.path_status_value (A.leaf_metadata leaf)); try discriminate; reflexivity. }
  split; [unfold publish; now rewrite link|]. intro position.
  pose proof (publication_returns_exact_fields_at_the_selected_byte_stamp stored current position) as found.
  destruct (publish stored current); cbn. now rewrite same in found.
Qed.

Theorem publication_preserves_every_old_direct_and_linked_lookup stored current address position previous :
  lookup stored address position = Some previous ->
  let '(next, _) := publish stored current in lookup next address position = Some previous.
Proof.
  destruct stored as [direct linked], address; unfold lookup; cbn;
    destruct (nth_error _ slot) eqn:found; cbn; try discriminate;
    intro same; unfold publish; cbn; destruct (needs_link current); unfold lookup; cbn.
  - now rewrite found.
  - rewrite (@P.old_lookup_after_append _ direct [A.checkpoint_leaf current] slot p found). exact same.
  - rewrite (@P.old_lookup_after_append _ linked [current] slot c found). exact same.
  - now rewrite found.
Qed.

Theorem ownerless_live_publication_does_not_allocate_a_linked_checkpoint stored current :
  A.checkpoint_owners current = [] -> A.checkpoint_fault current = None ->
  dead_leaf (A.checkpoint_leaf current) = false ->
  let '(next, _) := publish stored current in
  linked next = linked stored /\ length (direct next) = S (length (direct stored)).
Proof.
  intros owners fault live. unfold publish, needs_link. rewrite owners, fault, live. cbn.
  split; [reflexivity|rewrite length_app; cbn; lia].
Qed.

End Storage.

Inductive scalar_event :=
| ObligationStarted (obligation : T.obligation_snapshot)
| ObligationByte (id : nat) (byte : located_byte) (last : bool).

Theorem native_consumed_subtraction_equals_the_derived_absolute_end start declared stamp :
  start <= stamp -> stamp <= start + declared ->
  stamp - start <= declared /\
  declared - (stamp - start) = start + declared - stamp /\
  declared - (stamp - start) <= declared.
Proof. intros started bounded. repeat split; lia. Qed.

Section ScalarBoundaries.
Context {Span : Type}.

Record birth := Birth {
  born_leaf : @A.physical_leaf Span;
  born_clock : nat;
  born_event : scalar_event
}.

Record typed_birth := TypedBirth {
  born_path : @T.semantic_path Span;
  typed_born_clock : nat;
  typed_born_event : scalar_event
}.

Definition start_absolute metadata slot kind tag_source declared start length_source next clock :=
  let obligation := T.ObligationSnapshot clock kind declared tag_source in
  Birth (A.PhysicalLeaf metadata slot obligation
    (match declared with 0 => A.AwaitingMarker length_source | S _ => A.PayloadMarker end) start next)
    (S clock) (ObligationStarted obligation).

Definition start_typed metadata slot kind tag_source declared start length_source next clock :=
  let obligation := T.ObligationSnapshot clock kind declared tag_source in
  TypedBirth (T.SemanticPath metadata (T.InactiveTree slot
    (T.ObligationPayload obligation
      (match declared with 0 => T.AwaitingPhase length_source | S _ => T.PayloadPhase declared end) start) next))
    (S clock) (ObligationStarted obligation).

Theorem absolute_obligation_creation_refines_the_typed_graph_event_and_fresh_clock
    metadata slot kind tag_source declared start length_source next clock :
  let actual := start_absolute metadata slot kind tag_source declared start length_source next clock in
  let reference := start_typed metadata slot kind tag_source declared start length_source next clock in
  A.realize_leaf start (born_leaf actual) = born_path reference /\
  born_clock actual = typed_born_clock reference /\ born_event actual = typed_born_event reference /\
  T.obligation_id (A.leaf_obligation (born_leaf actual)) = clock /\ born_clock actual = S clock.
Proof.
  unfold start_absolute, start_typed. destruct declared; cbn [born_leaf born_path born_clock typed_born_clock born_event typed_born_event].
  - repeat split; reflexivity.
  - unfold A.realize_leaf, A.realize_phase, A.payload_end; cbn.
    replace (start + S declared - start) with (S declared) by lia. repeat split; reflexivity.
Qed.

Theorem zero_length_creation_is_explicitly_pending_at_the_length_source
    metadata slot kind tag_source start length_source next clock :
  let created := start_absolute metadata slot kind tag_source 0 start length_source next clock in
  A.leaf_phase (born_leaf created) = A.AwaitingMarker length_source /\
  A.realize_phase start (born_leaf created) = T.AwaitingPhase length_source /\
  born_event created = ObligationStarted (T.ObligationSnapshot clock kind 0 tag_source) /\
  born_clock created = S clock.
Proof. repeat split; reflexivity. Qed.

Theorem positive_length_creation_stays_payload_even_when_the_numeric_end_exceeds_a_native_limit
    metadata slot kind tag_source declared start length_source next clock native_limit :
  0 < declared -> native_limit < start + declared ->
  let created := start_absolute metadata slot kind tag_source declared start length_source next clock in
  A.leaf_phase (born_leaf created) = A.PayloadMarker /\
  A.payload_end (born_leaf created) = start + declared /\
  A.realize_phase start (born_leaf created) = T.PayloadPhase declared /\ born_clock created = S clock.
Proof.
  intros positive beyond. destruct declared; [lia|]. cbn [start_absolute born_leaf born_clock].
  unfold A.realize_phase, A.payload_end; cbn.
  replace (start + S declared - start) with (S declared) by lia. repeat split; reflexivity.
Qed.

Definition finish_leaf fresh endpoint (current : @A.physical_leaf Span) :=
  A.PhysicalLeaf (A.leaf_metadata current) fresh (A.leaf_obligation current)
    (A.AwaitingMarker endpoint) (A.leaf_payload_start current) (A.leaf_next current).

Definition absolute_byte stamp fresh byte current :=
  match A.leaf_phase current, F.path_status_value (A.leaf_metadata current) with
  | A.PayloadMarker, F.OpenPath =>
      if (A.leaf_payload_start current <=? stamp) && (stamp <? A.payload_end current) then
        let last := S stamp =? A.payload_end current in
        Some ((if last then finish_leaf fresh (byte_provenance byte) current else current),
          ObligationByte (T.obligation_id (A.leaf_obligation current)) byte last)
      else None
  | _, _ => None
  end.

Definition typed_byte fresh byte (current : @T.semantic_path Span) :=
  match F.path_status_value (T.semantic_metadata current), T.semantic_frames current with
  | F.OpenPath, T.InactiveTree _ (T.ObligationPayload obligation (T.PayloadPhase remaining) start) next =>
      if 0 <? remaining then
        let residual := remaining - 1 in
        let last := residual =? 0 in
        Some (T.SemanticPath (T.semantic_metadata current)
          (T.InactiveTree fresh (T.ObligationPayload obligation
            (if last then T.AwaitingPhase (byte_provenance byte) else T.PayloadPhase residual) start) next),
          ObligationByte (T.obligation_id obligation) byte last)
      else None
  | _, _ => None
  end.

Definition typed_nonfinal fresh stamp (current : @A.physical_leaf Span) :=
  T.SemanticPath (A.leaf_metadata current)
    (T.InactiveTree fresh (T.ObligationPayload (A.leaf_obligation current)
      (T.PayloadPhase (A.payload_end current - stamp - 1)) (A.leaf_payload_start current)) (A.leaf_next current)).

Theorem nonfinal_scalar_retains_the_physical_leaf_and_refines_the_fresh_typed_frame
    stamp fresh byte current :
  A.leaf_phase current = A.PayloadMarker ->
  F.path_status_value (A.leaf_metadata current) = F.OpenPath ->
  A.leaf_payload_start current <= stamp -> S stamp < A.payload_end current ->
  absolute_byte stamp fresh byte current =
    Some (current, ObligationByte (T.obligation_id (A.leaf_obligation current)) byte false) /\
  typed_byte fresh byte (A.realize_leaf stamp current) =
    Some (typed_nonfinal fresh stamp current, ObligationByte (T.obligation_id (A.leaf_obligation current)) byte false) /\
  T.same_path_semantics (A.realize_leaf (S stamp) current) (typed_nonfinal fresh stamp current).
Proof.
  intros phase live started interior.
  assert (at_start : A.leaf_payload_start current <=? stamp = true) by (apply Nat.leb_le; exact started).
  assert (within : stamp <? A.payload_end current = true) by (apply Nat.ltb_lt; lia).
  assert (not_final : S stamp =? A.payload_end current = false) by (apply Nat.eqb_neq; lia).
  split.
  - unfold absolute_byte. now rewrite phase, live, at_start, within, not_final.
  - assert (positive : 0 <? A.payload_end current - stamp = true) by (apply Nat.ltb_lt; lia).
    assert (residual : A.payload_end current - stamp - 1 =? 0 = false) by (apply Nat.eqb_neq; lia).
    split.
    + unfold typed_byte, A.realize_leaf, A.realize_phase. rewrite phase. cbn -[Nat.ltb Nat.leb Nat.eqb Nat.sub].
      rewrite live, positive, residual. reflexivity.
    + unfold T.same_path_semantics, A.realize_leaf, A.realize_phase, typed_nonfinal. rewrite phase. cbn.
      split; [reflexivity|]. f_equal. f_equal. f_equal. lia.
Qed.

Theorem final_scalar_creates_the_exact_awaiting_frame_at_the_actual_physical_byte
    stamp fresh byte current :
  A.leaf_phase current = A.PayloadMarker ->
  F.path_status_value (A.leaf_metadata current) = F.OpenPath ->
  A.leaf_payload_start current <= stamp -> S stamp = A.payload_end current ->
  let next := finish_leaf fresh (byte_provenance byte) current in
  absolute_byte stamp fresh byte current =
    Some (next, ObligationByte (T.obligation_id (A.leaf_obligation current)) byte true) /\
  typed_byte fresh byte (A.realize_leaf stamp current) =
    Some (A.realize_leaf (S stamp) next, ObligationByte (T.obligation_id (A.leaf_obligation current)) byte true) /\
  A.leaf_phase next = A.AwaitingMarker (byte_source byte, byte_offset byte) /\
  A.leaf_obligation next = A.leaf_obligation current /\
  A.leaf_payload_start next = A.leaf_payload_start current /\ A.leaf_next next = A.leaf_next current.
Proof.
  intros phase live started ending. cbn zeta.
  assert (at_start : A.leaf_payload_start current <=? stamp = true) by (apply Nat.leb_le; exact started).
  assert (within : stamp <? A.payload_end current = true) by (apply Nat.ltb_lt; lia).
  assert (last : S stamp =? A.payload_end current = true) by (apply Nat.eqb_eq; exact ending).
  split.
  - unfold absolute_byte. now rewrite phase, live, at_start, within, last.
  - split.
    + unfold typed_byte, A.realize_leaf, A.realize_phase. rewrite phase. cbn -[Nat.ltb Nat.leb Nat.eqb Nat.sub].
      rewrite live. replace (A.payload_end current - stamp) with 1 by lia. reflexivity.
    + repeat split; reflexivity.
Qed.

Theorem invalid_stamps_and_explicit_pending_phases_never_feed_a_payload_byte stamp fresh byte current :
  (stamp < A.leaf_payload_start current \/ A.payload_end current <= stamp) ->
    absolute_byte stamp fresh byte current = None.
Proof.
  intro invalid. unfold absolute_byte.
  destruct (A.leaf_phase current), (F.path_status_value (A.leaf_metadata current)); try reflexivity.
  destruct invalid as [early|late].
  - assert (A.leaf_payload_start current <=? stamp = false) by (apply Nat.leb_gt; exact early).
    now rewrite H.
  - assert (stamp <? A.payload_end current = false) by (apply Nat.ltb_ge; exact late).
    now rewrite H, andb_false_r.
Qed.

Theorem explicit_pending_phase_refuses_even_if_the_stamp_looks_like_live_payload stamp fresh byte current endpoint :
  A.leaf_phase current = A.AwaitingMarker endpoint -> absolute_byte stamp fresh byte current = None.
Proof. intro pending. unfold absolute_byte. rewrite pending. destruct (F.path_status_value (A.leaf_metadata current)); reflexivity. Qed.

Theorem consecutive_births_use_distinct_actual_obligation_ids_without_checkpoint_clock_state
    metadata slot kind tag_source declared start length_source next clock :
  let first := start_absolute metadata slot kind tag_source declared start length_source next clock in
  let second := start_absolute metadata slot kind tag_source declared start length_source next (born_clock first) in
  T.obligation_id (A.leaf_obligation (born_leaf first)) < T.obligation_id (A.leaf_obligation (born_leaf second)) /\
  born_clock second = clock + 2.
Proof. cbn. split; lia. Qed.

End ScalarBoundaries.

Inductive private_operation := Resolve | Reject.

Section PrivatePreflight.
Context {Span : Type}.

Record world := World {
  world_checkpoints : @storage Span;
  world_frames : list (@T.stored_frame Span);
  world_owners : P.owner_arena;
  world_bytes : list located_byte;
  world_current : C.address;
  world_restore : list (C.address * list located_byte);
  world_clock : nat
}.

Lemma typed_root_fault_does_not_change_the_private_terminal (path : @T.semantic_path Span) error :
  T.path_private_terminal (D.kill_typed path error) = T.path_private_terminal path.
Proof. reflexivity. Qed.

Theorem selected_checkpoint_private_terminal_preserves_phase_id_and_source_even_when_dead
    (current : @A.checkpoint Span) position :
  let leaf := A.checkpoint_leaf current in
  let obligation := A.leaf_obligation leaf in
  let '(source, offset) := T.obligation_tag_source obligation in
  T.path_private_terminal (A.selected_graph current position) =
    P.TerminalObligation (T.obligation_id obligation) source offset
      (match A.leaf_phase leaf with A.PayloadMarker => false | A.AwaitingMarker _ => true end).
Proof.
  cbn zeta. unfold A.selected_graph, A.graph_at.
  destruct (A.checkpoint_fault current); [rewrite typed_root_fault_does_not_change_the_private_terminal|];
    unfold A.realize_graph; rewrite T.typed_overlay_private_terminal_is_the_selected_focus_terminal;
    unfold T.path_private_terminal, A.realize_leaf, A.realize_phase;
    destruct (A.leaf_phase (A.checkpoint_leaf current)); cbn;
    destruct (T.obligation_tag_source (A.leaf_obligation (A.checkpoint_leaf current))); reflexivity.
Qed.

Definition private_preflight operation requested caller current :=
  match lookup (world_checkpoints current) (world_current current) (length (world_bytes current)) with
  | None => match operation with Resolve => Some (0, 0) | Reject => Some caller end
  | Some selected =>
      let terminal := T.path_private_terminal (A.selected_graph selected (length (world_bytes current))) in
      match operation with
      | Resolve => P.resolve_preflight requested terminal
      | Reject => P.reject_preflight requested caller terminal
      end
  end.

Definition guarded_private operation requested caller (accepted : world -> world) current :=
  match private_preflight operation requested caller current with
  | Some error => (Some error, current)
  | None => (None, accepted current)
  end.

Theorem failed_private_preflight_preserves_the_complete_concrete_world
    operation requested caller accepted current error :
  private_preflight operation requested caller current = Some error ->
  guarded_private operation requested caller accepted current = (Some error, current).
Proof. intro refused. unfold guarded_private. now rewrite refused. Qed.

Theorem wrong_id_private_operations_return_the_hidden_source_without_mutating_any_world_field
    operation requested caller accepted current selected :
  lookup (world_checkpoints current) (world_current current) (length (world_bytes current)) = Some selected ->
  requested <> T.obligation_id (A.leaf_obligation (A.checkpoint_leaf selected)) ->
  let '(source, offset) := T.obligation_tag_source (A.leaf_obligation (A.checkpoint_leaf selected)) in
  private_preflight operation requested caller current = Some (source, offset) /\
  guarded_private operation requested caller accepted current = (Some (source, offset), current).
Proof.
  intros found different.
  pose proof (@selected_checkpoint_private_terminal_preserves_phase_id_and_source_even_when_dead
    selected (length (world_bytes current))) as terminal.
  cbn zeta in terminal |- *.
  destruct (T.obligation_tag_source (A.leaf_obligation (A.checkpoint_leaf selected))) as [source offset].
  assert (wrong : requested =? T.obligation_id (A.leaf_obligation (A.checkpoint_leaf selected)) = false)
    by (apply Nat.eqb_neq; exact different).
  assert (refused : private_preflight operation requested caller current = Some (source, offset)).
  { unfold private_preflight. rewrite found. destruct operation; rewrite terminal;
      unfold P.resolve_preflight, P.reject_preflight; now rewrite wrong. }
  split; [exact refused|]. now apply failed_private_preflight_preserves_the_complete_concrete_world.
Qed.

Theorem matching_payload_resolve_is_nonmutating_while_reject_preflight_passes
    caller accepted current selected :
  lookup (world_checkpoints current) (world_current current) (length (world_bytes current)) = Some selected ->
  A.leaf_phase (A.checkpoint_leaf selected) = A.PayloadMarker ->
  let obligation := A.leaf_obligation (A.checkpoint_leaf selected) in
  let '(source, offset) := T.obligation_tag_source obligation in
  private_preflight Resolve (T.obligation_id obligation) caller current = Some (source, offset) /\
  guarded_private Resolve (T.obligation_id obligation) caller accepted current = (Some (source, offset), current) /\
  private_preflight Reject (T.obligation_id obligation) caller current = None.
Proof.
  intros found payload.
  pose proof (@selected_checkpoint_private_terminal_preserves_phase_id_and_source_even_when_dead
    selected (length (world_bytes current))) as terminal.
  cbn zeta in terminal |- *.
  destruct (T.obligation_tag_source (A.leaf_obligation (A.checkpoint_leaf selected))) as [source offset].
  rewrite payload in terminal.
  assert (refused : private_preflight Resolve (T.obligation_id (A.leaf_obligation (A.checkpoint_leaf selected))) caller current =
    Some (source, offset)).
  { unfold private_preflight. rewrite found, terminal. unfold P.resolve_preflight. now rewrite andb_false_r. }
  split; [exact refused|]. split.
  - now apply failed_private_preflight_preserves_the_complete_concrete_world.
  - unfold private_preflight. rewrite found, terminal. unfold P.reject_preflight. now rewrite Nat.eqb_refl.
Qed.

End PrivatePreflight.

Print Assumptions direct_publication_requires_no_owners_no_fault_and_nondead_metadata.
Print Assumptions retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields.
Print Assumptions publication_returns_exact_fields_at_the_selected_byte_stamp.
Print Assumptions publication_preserves_valid_selected_typed_interpretation.
Print Assumptions every_dead_or_fault_checkpoint_retains_its_saved_stamp_in_linked_storage.
Print Assumptions publication_preserves_every_old_direct_and_linked_lookup.
Print Assumptions ownerless_live_publication_does_not_allocate_a_linked_checkpoint.
Print Assumptions native_consumed_subtraction_equals_the_derived_absolute_end.
Print Assumptions absolute_obligation_creation_refines_the_typed_graph_event_and_fresh_clock.
Print Assumptions zero_length_creation_is_explicitly_pending_at_the_length_source.
Print Assumptions positive_length_creation_stays_payload_even_when_the_numeric_end_exceeds_a_native_limit.
Print Assumptions nonfinal_scalar_retains_the_physical_leaf_and_refines_the_fresh_typed_frame.
Print Assumptions final_scalar_creates_the_exact_awaiting_frame_at_the_actual_physical_byte.
Print Assumptions invalid_stamps_and_explicit_pending_phases_never_feed_a_payload_byte.
Print Assumptions explicit_pending_phase_refuses_even_if_the_stamp_looks_like_live_payload.
Print Assumptions consecutive_births_use_distinct_actual_obligation_ids_without_checkpoint_clock_state.
Print Assumptions selected_checkpoint_private_terminal_preserves_phase_id_and_source_even_when_dead.
Print Assumptions failed_private_preflight_preserves_the_complete_concrete_world.
Print Assumptions wrong_id_private_operations_return_the_hidden_source_without_mutating_any_world_field.
Print Assumptions matching_payload_resolve_is_nonmutating_while_reject_preflight_passes.

End AbsolutePayloadCheckpointBoundary.

Module PersistentOpaqueStateReuse.

Import CanonicalKeyProtocol.

Definition before_cache position (cache : PersistentFocusedOwners.boundary_handle) :=
  match cache with None => true | Some (ending, _) => position <? ending end.

Lemma before_chosen_boundary position bound address previous :
  before_cache position (PersistentFocusedOwners.choose_boundary false bound address previous) = true <->
  position < bound /\ before_cache position previous = true.
Proof.
  destruct previous as [[ending target]|]; unfold PersistentFocusedOwners.choose_boundary, before_cache; cbn -[Nat.ltb].
  - destruct (bound <? ending) eqn:choice;
      rewrite !Nat.ltb_lt; apply Nat.ltb_lt in choice || apply Nat.ltb_ge in choice; lia.
  - rewrite Nat.ltb_lt. tauto.
Qed.

Lemma reference_cache_checks_every_held_bound position entries :
  before_cache position (PersistentFocusedOwners.reference_all entries) = true <->
  Forall (fun entry => position < FocusedRegionOwners.owner_bound (PersistentFocusedOwners.cell_owner (snd entry))) entries.
Proof.
  induction entries as [|[address cell] rest IH]; cbn [PersistentFocusedOwners.reference_all].
  - split; intro valid; [constructor|reflexivity].
  - rewrite before_chosen_boundary, IH, Forall_cons_iff. reflexivity.
Qed.

Section Refinement.
Context {Span : Type}.
Variable observe_span : Span -> option (list located_byte).
Variable summarize_frame : @PersistentFrameRealization.stored_frame Span -> ReusableFrameOwnership.frame_metadata.

Theorem checked_linked_cache_implies_every_typed_owner_is_strict
    arena root entries owners stamp width :
  PersistentFocusedOwners.linked arena root entries ->
  PersistentOwnerProjection.owners_match observe_span summarize_frame owners entries ->
  Forall (fun owner => FocusedRegionOwners.path_status_value (PersistentFrameRealization.held_metadata owner) = FocusedRegionOwners.OpenPath) owners ->
  before_cache (stamp + width) (PersistentFocusedOwners.read_all arena root) = true ->
  Forall (AbsolutePayloadRealization.strict_owner stamp width) owners.
Proof.
  intros linked matches open checked.
  rewrite (proj1 (PersistentFocusedOwners.cached_linked_reads_refine_exact_list_selection linked)) in checked.
  apply reference_cache_checks_every_held_bound in checked.
  clear linked. revert open checked.
  induction matches as [|owner [address cell] owners entries projected matches IH].
  - intros. constructor.
  - intros open checked. inversion open as [|? ? head_open tail_open]; subst.
    inversion checked as [|? ? head_bound tail_bound]; subst.
    constructor; [|now apply IH].
    pose proof (@PersistentOwnerProjection.projected_owner_preserves_exact_scheduler_fields
      Span observe_span summarize_frame owner (PersistentFocusedOwners.cell_owner cell) projected)
      as [region [path fields]].
    unfold FocusedRegionOwners.owner_bound in head_bound. cbn in head_bound, region, path.
    rewrite region, path in head_bound.
    unfold AbsolutePayloadRealization.strict_owner. split; [exact head_open|].
    split; eapply Nat.lt_le_trans; [exact head_bound|apply Nat.le_min_l|
      exact head_bound|apply Nat.le_min_r].
Qed.

Definition checked_interior arena root limit available stamp width (current : @AbsolutePayloadRealization.checkpoint Span) :=
  match AbsolutePayloadRealization.checkpoint_fault current,
      FocusedRegionOwners.path_status_value (AbsolutePayloadRealization.leaf_metadata (AbsolutePayloadRealization.checkpoint_leaf current)),
      AbsolutePayloadRealization.leaf_phase (AbsolutePayloadRealization.checkpoint_leaf current) with
  | None, FocusedRegionOwners.OpenPath, AbsolutePayloadRealization.PayloadMarker =>
      (AbsolutePayloadRealization.leaf_payload_start (AbsolutePayloadRealization.checkpoint_leaf current) <=? stamp) &&
      (width <? PersistentFrameRealization.obligation_payload_len (AbsolutePayloadRealization.leaf_obligation (AbsolutePayloadRealization.checkpoint_leaf current)) -
        (stamp - AbsolutePayloadRealization.leaf_payload_start (AbsolutePayloadRealization.checkpoint_leaf current))) &&
      (0 <? width) && (width <=? available) && (stamp + width <=? limit) &&
      before_cache (stamp + width) (PersistentFocusedOwners.read_all arena root)
  | _, _, _ => false
  end.

Theorem checked_native_guard_realizes_the_existing_strict_absolute_window
    arena root entries limit available stamp width current :
  PersistentFocusedOwners.linked arena root entries ->
  PersistentOwnerProjection.owners_match observe_span summarize_frame (AbsolutePayloadRealization.checkpoint_owners current) entries ->
  Forall (fun owner => FocusedRegionOwners.path_status_value (PersistentFrameRealization.held_metadata owner) = FocusedRegionOwners.OpenPath)
    (AbsolutePayloadRealization.checkpoint_owners current) ->
  checked_interior arena root limit available stamp width current = true ->
  AbsolutePayloadRealization.checkpoint_fault current = None /\ 0 < width /\ width <= available /\
  stamp + width <= limit /\
  AbsolutePayloadRealization.strict_window stamp width (AbsolutePayloadRealization.checkpoint_leaf current) (AbsolutePayloadRealization.checkpoint_owners current).
Proof.
  intros linked matches open checked.
  unfold checked_interior in checked.
  destruct (AbsolutePayloadRealization.checkpoint_fault current) eqn:fault; [discriminate|].
  destruct (FocusedRegionOwners.path_status_value (AbsolutePayloadRealization.leaf_metadata (AbsolutePayloadRealization.checkpoint_leaf current))) eqn:live;
    try discriminate.
  destruct (AbsolutePayloadRealization.leaf_phase (AbsolutePayloadRealization.checkpoint_leaf current)) eqn:phase; [|discriminate].
  rewrite !andb_true_iff, !Nat.leb_le, !Nat.ltb_lt in checked.
  destruct checked as [[[[[started payload] positive] source] native] owners].
  split; [reflexivity|]. split; [exact positive|]. split; [exact source|].
  split; [exact native|]. unfold AbsolutePayloadRealization.strict_window.
  split; [exact phase|]. split; [exact live|]. split; [exact started|].
  split; [unfold AbsolutePayloadRealization.payload_end; lia|].
  eapply checked_linked_cache_implies_every_typed_owner_is_strict; eauto.
Qed.

Theorem guarded_state_reuse_refines_every_located_prefix
    arena root entries limit available stamp input current prefix :
  PersistentFocusedOwners.linked arena root entries ->
  PersistentOwnerProjection.owners_match observe_span summarize_frame (AbsolutePayloadRealization.checkpoint_owners current) entries ->
  Forall (fun owner => FocusedRegionOwners.path_status_value (PersistentFrameRealization.held_metadata owner) = FocusedRegionOwners.OpenPath)
    (AbsolutePayloadRealization.checkpoint_owners current) ->
  checked_interior arena root limit available stamp (length input) current = true ->
  prefix <= length input ->
  AbsolutePayloadRealization.strict_scalar_run (firstn prefix input) (AbsolutePayloadRealization.selected_graph current stamp)
    (AbsolutePayloadRealization.selected_graph current (stamp + prefix))
    (map (fun byte => StrictOpaqueInterior.PayloadByte
      (PersistentFrameRealization.obligation_id (AbsolutePayloadRealization.leaf_obligation (AbsolutePayloadRealization.checkpoint_leaf current))) byte false)
      (firstn prefix input)).
Proof.
  intros linked matches open checked within.
  destruct (@checked_native_guard_realizes_the_existing_strict_absolute_window
    arena root entries limit available stamp (length input) current linked matches open checked)
    as [fault [_ [_ [_ strict]]]].
  assert (live : FocusedRegionOwners.path_status_value (AbsolutePayloadRealization.leaf_metadata (AbsolutePayloadRealization.checkpoint_leaf current)) = FocusedRegionOwners.OpenPath)
    by exact (proj1 (proj2 strict)).
  rewrite !AbsolutePayloadRealization.live_absolute_checkpoint_uses_the_selected_byte_prefix by assumption.
  unfold AbsolutePayloadRealization.graph_at. rewrite fault.
  assert (size : length (firstn prefix input) = prefix) by (rewrite length_firstn, Nat.min_l; auto).
  replace (stamp + prefix) with (stamp + length (firstn prefix input)) by now rewrite size.
  apply AbsolutePayloadRealization.strict_absolute_window_refines_repeated_typed_scalar_payload_steps.
  replace stamp with (stamp + 0) by lia.
  eapply AbsolutePayloadRealization.strict_window_prefix; [exact strict|rewrite size; lia].
Qed.

Theorem guarded_scalar_reuses_the_physical_leaf_with_the_exact_scalar_event
    arena root entries limit available stamp current fresh byte :
  PersistentFocusedOwners.linked arena root entries ->
  PersistentOwnerProjection.owners_match observe_span summarize_frame (AbsolutePayloadRealization.checkpoint_owners current) entries ->
  Forall (fun owner => FocusedRegionOwners.path_status_value (PersistentFrameRealization.held_metadata owner) = FocusedRegionOwners.OpenPath)
    (AbsolutePayloadRealization.checkpoint_owners current) ->
  checked_interior arena root limit available stamp 1 current = true ->
  AbsolutePayloadCheckpointBoundary.absolute_byte stamp fresh byte (AbsolutePayloadRealization.checkpoint_leaf current) =
    Some (AbsolutePayloadRealization.checkpoint_leaf current,
      AbsolutePayloadCheckpointBoundary.ObligationByte (PersistentFrameRealization.obligation_id (AbsolutePayloadRealization.leaf_obligation (AbsolutePayloadRealization.checkpoint_leaf current))) byte false).
Proof.
  intros linked matches open checked.
  destruct (@checked_native_guard_realizes_the_existing_strict_absolute_window
    arena root entries limit available stamp 1 current linked matches open checked)
    as [_ [_ [_ [_ [phase [live [started [interior owners]]]]]]]].
  apply (proj1 (@AbsolutePayloadCheckpointBoundary.nonfinal_scalar_retains_the_physical_leaf_and_refines_the_fresh_typed_frame
    Span stamp fresh byte (AbsolutePayloadRealization.checkpoint_leaf current)
    phase live started ltac:(lia))).
Qed.

End Refinement.

Section Publication.
Context {Span : Type}.

Definition reuse_byte_handle (current : CompactPersistentCheckpoints.address * Span) (next_bytes : Span) := (fst current, next_bytes).

Theorem reuse_keeps_the_exact_state_address_and_requires_the_full_byte_handle
    (current : CompactPersistentCheckpoints.address * Span) next_bytes :
  fst (reuse_byte_handle current next_bytes) = fst current /\
  snd (reuse_byte_handle current next_bytes) = next_bytes /\
  (reuse_byte_handle current next_bytes = current <-> next_bytes = snd current).
Proof.
  destruct current as [address bytes]. cbn.
  split; [reflexivity|]. split; [reflexivity|].
  split; intro same; [now inversion same|now subst].
Qed.

Definition scalar_world (current : @AbsolutePayloadCheckpointBoundary.world Span) byte :=
  AbsolutePayloadCheckpointBoundary.World (AbsolutePayloadCheckpointBoundary.world_checkpoints current) (AbsolutePayloadCheckpointBoundary.world_frames current) (AbsolutePayloadCheckpointBoundary.world_owners current)
    (AbsolutePayloadCheckpointBoundary.world_bytes current ++ [byte]) (AbsolutePayloadCheckpointBoundary.world_current current)
    ((AbsolutePayloadCheckpointBoundary.world_current current, AbsolutePayloadCheckpointBoundary.world_bytes current) :: AbsolutePayloadCheckpointBoundary.world_restore current)
    (AbsolutePayloadCheckpointBoundary.world_clock current).

Theorem reused_scalar_publishes_only_the_byte_and_exact_restore_predecessor current byte :
  AbsolutePayloadCheckpointBoundary.world_checkpoints (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_checkpoints current /\
  AbsolutePayloadCheckpointBoundary.world_frames (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_frames current /\
  AbsolutePayloadCheckpointBoundary.world_owners (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_owners current /\
  AbsolutePayloadCheckpointBoundary.world_clock (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_clock current /\
  AbsolutePayloadCheckpointBoundary.world_current (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_current current /\
  AbsolutePayloadCheckpointBoundary.world_bytes (scalar_world current byte) = AbsolutePayloadCheckpointBoundary.world_bytes current ++ [byte] /\
  AbsolutePayloadCheckpointBoundary.world_restore (scalar_world current byte) =
    (AbsolutePayloadCheckpointBoundary.world_current current, AbsolutePayloadCheckpointBoundary.world_bytes current) :: AbsolutePayloadCheckpointBoundary.world_restore current /\
  forall address position,
    AbsolutePayloadCheckpointBoundary.lookup (AbsolutePayloadCheckpointBoundary.world_checkpoints (scalar_world current byte)) address position =
    AbsolutePayloadCheckpointBoundary.lookup (AbsolutePayloadCheckpointBoundary.world_checkpoints current) address position.
Proof. repeat split; intros; reflexivity. Qed.

Theorem every_reused_byte_handle_selects_the_existing_published_state
    (stored : @AbsolutePayloadCheckpointBoundary.storage Span) current position :
  let '(retained, address) := AbsolutePayloadCheckpointBoundary.publish stored current in
  AbsolutePayloadCheckpointBoundary.lookup retained address position = Some (AbsolutePayloadCheckpointBoundary.retime position current) /\
  AbsolutePayloadRealization.selected_graph (AbsolutePayloadCheckpointBoundary.retime position current) position = AbsolutePayloadRealization.selected_graph current position.
Proof.
  pose proof (AbsolutePayloadCheckpointBoundary.publication_returns_exact_fields_at_the_selected_byte_stamp stored current position) as found.
  destruct (AbsolutePayloadCheckpointBoundary.publish stored current) as [retained address].
  split; [exact found|].
  exact (proj1 (proj2 (AbsolutePayloadCheckpointBoundary.retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields
    current position position))).
Qed.

End Publication.

Print Assumptions checked_linked_cache_implies_every_typed_owner_is_strict.
Print Assumptions checked_native_guard_realizes_the_existing_strict_absolute_window.
Print Assumptions guarded_state_reuse_refines_every_located_prefix.
Print Assumptions guarded_scalar_reuses_the_physical_leaf_with_the_exact_scalar_event.
Print Assumptions reuse_keeps_the_exact_state_address_and_requires_the_full_byte_handle.
Print Assumptions reused_scalar_publishes_only_the_byte_and_exact_restore_predecessor.
Print Assumptions every_reused_byte_handle_selects_the_existing_published_state.

End PersistentOpaqueStateReuse.

Module CompressedScalarUndo.

Import CanonicalKeyProtocol IndexedWindowTransport.

Record handle := Handle {
  handle_state : CompactPersistentCheckpoints.address;
  handle_span : window
}.

Definition depth current := window_width (handle_span current).

Definition shorter current width :=
  Handle (handle_state current)
    (Window (window_owner (handle_span current)) (window_start (handle_span current)) width None).

Lemma shorter_depth current width : depth (shorter current width) = width.
Proof. reflexivity. Qed.

Lemma shorter_state current width : handle_state (shorter current width) = handle_state current.
Proof. reflexivity. Qed.

Record run := Run {
  latest : handle;
  first_depth : nat
}.

Definition same_state left right :=
  CompactPersistentCheckpoints.encode_address (handle_state left) =?
  CompactPersistentCheckpoints.encode_address (handle_state right).

Lemma same_state_exact left right :
  same_state left right = true <-> handle_state left = handle_state right.
Proof.
  unfold same_state. rewrite Nat.eqb_eq.
  split; [apply CompactPersistentCheckpoints.encoded_addresses_are_injective|now intros ->].
Qed.

Definition remember previous history :=
  match history with
  | [] => [Run previous (depth previous)]
  | first :: rest =>
      if same_state previous (latest first)
      then Run previous (first_depth first) :: rest
      else Run previous (depth previous) :: history
  end.

Record pop_result := PopResult {
  restored : handle;
  remaining_runs : list run;
  prefix_slices : nat
}.

Definition pop history :=
  match history with
  | [] => None
  | first :: rest =>
      Some (if depth (latest first) =? first_depth first
        then PopResult (latest first) rest 0
        else PopResult (latest first)
          (Run (shorter (latest first) (depth (latest first) - 1)) (first_depth first) :: rest) 1)
  end.

Definition observation := (CompactPersistentCheckpoints.address * list located_byte)%type.

Fixpoint descending state bytes count : list observation :=
  match count with
  | 0 => []
  | S remaining =>
      (state, bytes) :: descending state (firstn (length bytes - 1) bytes) remaining
  end.

Inductive scalar_chain : list located_byte -> list observation -> Prop :=
| ChainRoot : scalar_chain [] []
| ChainEdge state prefix byte history :
    scalar_chain prefix history ->
    scalar_chain (prefix ++ [byte]) ((state, prefix) :: history).

Lemma scalar_chain_head current state prefix history :
  scalar_chain current ((state, prefix) :: history) ->
  (exists byte, current = prefix ++ [byte]) /\ scalar_chain prefix history.
Proof. intro chain. inversion chain; subst. split; [eauto|assumption]. Qed.

Lemma trim_app_single (bytes : list located_byte) byte :
  firstn (length (bytes ++ [byte]) - 1) (bytes ++ [byte]) = bytes.
Proof.
  rewrite length_app. cbn [length].
  replace (length bytes + 1 - 1) with (length bytes) by lia.
  rewrite firstn_app, firstn_all, Nat.sub_diag. cbn. now rewrite app_nil_r.
Qed.

Section Bytes.
Variable roots : nat -> region.

Definition bytes current :=
  window_bytes (roots (window_owner (handle_span current))) (handle_span current).

Definition valid_handle current :=
  window_valid (roots (window_owner (handle_span current))) (handle_span current).

Definition observe current : observation := (handle_state current, bytes current).

Definition valid_run current :=
  valid_handle (latest current) /\ first_depth current <= depth (latest current).

Definition valid_history := Forall valid_run.

Lemma make_run_valid current first :
  valid_handle current -> first <= depth current -> valid_run (Run current first).
Proof. intros valid bounded. split; assumption. Qed.

Definition expand_run current :=
  descending (handle_state (latest current)) (bytes (latest current))
    (S (depth (latest current) - first_depth current)).

Fixpoint expand history :=
  match history with [] => [] | first :: rest => expand_run first ++ expand rest end.

Lemma handle_length current :
  valid_handle current -> length (bytes current) = depth current.
Proof. apply valid_window_length. Qed.

Lemma shorter_is_valid current width :
  valid_handle current -> width <= depth current -> valid_handle (shorter current width).
Proof.
  destruct current as [state [owner start count anchor]].
  unfold valid_handle, shorter, depth, window_valid; cbn.
  intros [same bounded] within. split; [exact same|lia].
Qed.

Lemma shorter_bytes current width :
  width <= depth current -> bytes (shorter current width) = firstn width (bytes current).
Proof.
  destruct current as [state [owner start count anchor]].
  unfold bytes, shorter, depth, window_bytes; cbn.
  intro within. now rewrite firstn_firstn, Nat.min_l.
Qed.

Theorem strict_shortening_is_the_existing_checked_source_window current width limit :
  width < depth current -> window_start (handle_span current) + width <= limit ->
  checked_subwindow limit (handle_span current) 0 width =
    Some (handle_span (shorter current width)).
Proof.
  intros strict fits. unfold depth in strict. apply checked_subwindow_exact.
  split; [lia|]. split; [lia|]. split; [exact fits|].
  unfold shorter; cbn.
  assert (width =? window_width (handle_span current) = false) by
    (apply Nat.eqb_neq; unfold depth in strict; lia).
  rewrite H, Nat.add_0_r, Nat.sub_0_r. reflexivity.
Qed.

Theorem zero_prefix_normalization_is_observational current :
  bytes (shorter current 0) = [].
Proof. rewrite shorter_bytes by lia. reflexivity. Qed.

Lemma expand_cons first rest :
  expand (first :: rest) =
    observe (latest first) ::
      (descending (handle_state (latest first))
        (firstn (length (bytes (latest first)) - 1) (bytes (latest first)))
        (depth (latest first) - first_depth first) ++ expand rest).
Proof. reflexivity. Qed.

Theorem compressed_emptiness_is_exact history :
  expand history = [] <-> history = [].
Proof.
  destruct history as [|first rest]; [tauto|].
  rewrite expand_cons. split; discriminate.
Qed.

Theorem remembering_refines_exact_scalar_predecessor_push previous history :
  valid_handle previous -> valid_history history ->
  scalar_chain (bytes previous) (expand history) ->
  expand (remember previous history) = observe previous :: expand history /\
  valid_history (remember previous history).
Proof.
  intros valid history_valid chain.
  destruct history as [|first rest].
  - cbn [remember expand]. unfold expand_run. cbn [latest first_depth].
    rewrite Nat.sub_diag. cbn [descending app].
    split; [reflexivity|]. constructor; [apply make_run_valid; [exact valid|lia]|constructor].
  - inversion history_valid as [|? ? first_valid rest_valid]; subst.
    destruct first_valid as [latest_valid lower].
    rewrite expand_cons in chain.
    apply scalar_chain_head in chain.
    destruct chain as [[byte extension] tail_chain].
    assert (next_depth : depth previous = S (depth (latest first))).
    { rewrite <- (handle_length valid), extension, length_app,
        (handle_length latest_valid). cbn. lia. }
    cbn [remember]. destruct (same_state previous (latest first)) eqn:same.
    + apply same_state_exact in same.
      split.
      * cbn [expand]. unfold expand_run. cbn [latest first_depth].
        replace (depth previous - first_depth first) with
          (S (depth (latest first) - first_depth first)) by lia.
        cbn [descending app]. unfold observe.
        rewrite extension, same, trim_app_single. reflexivity.
      * constructor; [apply make_run_valid; [exact valid|lia]|exact rest_valid].
    + split.
      * cbn [expand]. unfold expand_run. cbn [latest first_depth].
        rewrite Nat.sub_diag. reflexivity.
      * constructor; [apply make_run_valid; [exact valid|lia]|exact history_valid].
Qed.

Theorem popping_refines_exact_scalar_head_and_tail history result :
  valid_history history -> pop history = Some result ->
  expand history = observe (restored result) :: expand (remaining_runs result) /\
  valid_handle (restored result) /\ valid_history (remaining_runs result) /\
  prefix_slices result <= 1.
Proof.
  intros history_valid popped.
  destruct history as [|first rest]; [discriminate|].
  inversion history_valid as [|? ? first_valid rest_valid]; subst.
  destruct first_valid as [latest_valid lower].
  cbn [pop] in popped.
  destruct (depth (latest first) =? first_depth first) eqn:boundary;
    inversion popped; subst result; cbn [restored remaining_runs prefix_slices].
  - apply Nat.eqb_eq in boundary. split.
    + cbn [expand]. unfold expand_run. rewrite boundary, Nat.sub_diag. reflexivity.
    + split; [exact latest_valid|]. split; [exact rest_valid|lia].
  - apply Nat.eqb_neq in boundary.
    assert (positive : 0 < depth (latest first)) by lia.
    split.
    + rewrite expand_cons. cbn [expand]. unfold expand_run. cbn [latest first_depth].
      unfold observe. rewrite shorter_bytes by lia.
      rewrite shorter_depth, shorter_state.
      rewrite (handle_length latest_valid).
      replace (S (depth (latest first) - 1 - first_depth first)) with
        (depth (latest first) - first_depth first) by lia.
      reflexivity.
    + split; [exact latest_valid|]. split; [|lia].
      constructor; [|exact rest_valid]. apply make_run_valid.
      * apply shorter_is_valid; [exact latest_valid|lia].
      * rewrite shorter_depth. lia.
Qed.


Lemma descending_prefixes_have_valid_handles current count :
  valid_handle current -> count <= S (depth current) ->
  Forall (fun entry => exists saved, valid_handle saved /\ observe saved = entry)
    (descending (handle_state current) (bytes current) count).
Proof.
  revert current. induction count as [|count IH]; intros current valid bounded; [constructor|].
  cbn [descending]. constructor.
  - exists current. split; [exact valid|reflexivity].
  - destruct count as [|count]; [constructor|].
    assert (within : depth current - 1 <= depth current) by lia.
    assert (next_valid : valid_handle (shorter current (depth current - 1)))
      by (apply shorter_is_valid; assumption).
    pose proof (IH (shorter current (depth current - 1)) next_valid
      ltac:(rewrite shorter_depth; lia)) as represented.
    rewrite shorter_state in represented.
    rewrite shorter_bytes in represented by exact within.
    rewrite (handle_length valid). exact represented.
Qed.

Theorem every_compressed_historical_prefix_is_a_valid_located_handle history :
  valid_history history ->
  Forall (fun entry => exists saved, valid_handle saved /\ observe saved = entry) (expand history).
Proof.
  intro valid. induction valid as [|first rest [head lower] tail IH]; cbn [expand]; [constructor|].
  apply Forall_app. split; [|exact IH].
  apply descending_prefixes_have_valid_handles; [exact head|lia].
Qed.

Theorem successful_edge_preserves_scalar_topology previous next byte history :
  valid_handle previous -> valid_history history ->
  scalar_chain (bytes previous) (expand history) ->
  bytes next = bytes previous ++ [byte] ->
  scalar_chain (bytes next) (expand (remember previous history)) /\
  valid_history (remember previous history).
Proof.
  intros valid history_valid chain appended.
  destruct (remembering_refines_exact_scalar_predecessor_push valid history_valid chain)
    as [exact_history next_valid].
  split; [|exact next_valid]. rewrite appended, exact_history.
  unfold observe. constructor. exact chain.
Qed.

Theorem successful_leave_preserves_scalar_topology current history result :
  valid_history history -> scalar_chain (bytes current) (expand history) ->
  pop history = Some result ->
  scalar_chain (bytes (restored result)) (expand (remaining_runs result)) /\
  valid_handle (restored result) /\ valid_history (remaining_runs result).
Proof.
  intros valid chain popped.
  destruct (popping_refines_exact_scalar_head_and_tail valid popped)
    as [expanded [restored_valid [tail_valid work]]].
  rewrite expanded in chain.
  destruct (scalar_chain_head chain) as [_ tail].
  split; [exact tail|]. split; assumption.
Qed.

Theorem byte_preserving_private_operations_preserve_the_exact_undo_topology
    current next history :
  bytes next = bytes current ->
  scalar_chain (bytes current) (expand history) ->
  scalar_chain (bytes next) (expand history).
Proof. intros same chain. now rewrite same. Qed.

Section Observation.
Context {Span : Type}.

Definition selected (stored : @AbsolutePayloadCheckpointBoundary.storage Span) current :=
  option_map
    (fun checkpoint => (AbsolutePayloadRealization.selected_graph checkpoint (length (bytes current)),
      bytes current))
    (AbsolutePayloadCheckpointBoundary.lookup stored (handle_state current) (length (bytes current))).

Theorem exact_state_and_located_bytes_preserve_every_selected_graph_and_query
    (stored : @AbsolutePayloadCheckpointBoundary.storage Span) left right :
  observe left = observe right ->
  selected stored left = selected stored right /\
  forall (Result : Type) (query : option (PersistentFrameRealization.semantic_path * list located_byte) -> Result),
    query (selected stored left) = query (selected stored right).
Proof.
  unfold observe. intro same. inversion same as [[state_same bytes_same]].
  assert (selected_same : selected stored left = selected stored right).
  { unfold selected. now rewrite state_same, bytes_same. }
  split; [exact selected_same|]. intros. now rewrite selected_same.
Qed.

Definition install_restored (current : @AbsolutePayloadCheckpointBoundary.world Span)
    previous history :=
  AbsolutePayloadCheckpointBoundary.World
    (AbsolutePayloadCheckpointBoundary.world_checkpoints current)
    (AbsolutePayloadCheckpointBoundary.world_frames current)
    (AbsolutePayloadCheckpointBoundary.world_owners current)
    (bytes previous) (handle_state previous) history
    (AbsolutePayloadCheckpointBoundary.world_clock current).

Definition scalar_leave (current : @AbsolutePayloadCheckpointBoundary.world Span) :=
  match AbsolutePayloadCheckpointBoundary.world_restore current with
  | [] => None
  | (state, prefix) :: rest =>
      Some (AbsolutePayloadCheckpointBoundary.World
        (AbsolutePayloadCheckpointBoundary.world_checkpoints current)
        (AbsolutePayloadCheckpointBoundary.world_frames current)
        (AbsolutePayloadCheckpointBoundary.world_owners current)
        prefix state rest (AbsolutePayloadCheckpointBoundary.world_clock current))
  end.

Theorem compressed_pop_is_the_exact_existing_scalar_world_transition
    (current : @AbsolutePayloadCheckpointBoundary.world Span) history result :
  valid_history history ->
  AbsolutePayloadCheckpointBoundary.world_restore current = expand history ->
  pop history = Some result ->
  scalar_leave current =
    Some (install_restored current (restored result) (expand (remaining_runs result))).
Proof.
  intros valid agrees popped.
  destruct (popping_refines_exact_scalar_head_and_tail valid popped) as [expanded other].
  unfold scalar_leave. rewrite agrees, expanded. reflexivity.
Qed.

Theorem restoring_never_changes_any_checkpoint_frame_owner_or_obligation_clock
    (current : @AbsolutePayloadCheckpointBoundary.world Span) previous history :
  let next := install_restored current previous history in
  AbsolutePayloadCheckpointBoundary.world_checkpoints next =
    AbsolutePayloadCheckpointBoundary.world_checkpoints current /\
  AbsolutePayloadCheckpointBoundary.world_frames next =
    AbsolutePayloadCheckpointBoundary.world_frames current /\
  AbsolutePayloadCheckpointBoundary.world_owners next =
    AbsolutePayloadCheckpointBoundary.world_owners current /\
  AbsolutePayloadCheckpointBoundary.world_clock next =
    AbsolutePayloadCheckpointBoundary.world_clock current /\
  AbsolutePayloadCheckpointBoundary.world_current next = handle_state previous /\
  AbsolutePayloadCheckpointBoundary.world_bytes next = bytes previous /\
  AbsolutePayloadCheckpointBoundary.world_restore next = history.
Proof. repeat split; reflexivity. Qed.

End Observation.

Lemma scalar_chain_depth current history :
  scalar_chain current history -> length history = length current.
Proof. intro chain. induction chain; cbn; [reflexivity|]. rewrite length_app. cbn. lia. Qed.

Theorem compressed_topology_depth_and_empty_root_are_exact current history :
  scalar_chain (bytes current) (expand history) ->
  length (expand history) = length (bytes current) /\
  (history = [] <-> bytes current = []).
Proof.
  intro chain. pose proof (scalar_chain_depth chain) as same.
  split; [exact same|]. split.
  - intro empty. subst history. cbn in chain. inversion chain. reflexivity.
  - intro empty. apply compressed_emptiness_is_exact.
    apply length_zero_iff_nil. rewrite same, empty. reflexivity.
Qed.

Inductive leave_branch :=
| RestorePredecessor
| FirstRootLeave
| RootUnderflow (source : provenance).

Definition classify_leave (empty root_left : bool) source :=
  if empty then if root_left then RootUnderflow source else FirstRootLeave
  else RestorePredecessor.

Inductive topology_error :=
| UnresolvedAt (source : provenance)
| IncompleteAt (source : provenance)
| AfterRootAt (source : provenance).

Definition classify_finish empty root_left pending source :=
  match pending with
  | Some endpoint => Some (UnresolvedAt endpoint)
  | None => if root_left && empty then None else Some (IncompleteAt source)
  end.

Definition is_empty {Value : Type} (values : list Value) :=
  match values with [] => true | _ => false end.

Theorem compression_preserves_root_leave_underflow_and_finish_error_priority
    history root_left pending source :
  classify_leave (is_empty history) root_left source =
    classify_leave (is_empty (expand history)) root_left source /\
  classify_finish (is_empty history) root_left pending source =
    classify_finish (is_empty (expand history)) root_left pending source.
Proof.
  destruct history as [|first rest]; [split; reflexivity|].
  rewrite expand_cons. split; reflexivity.
Qed.

Definition guarded_edge_publication {Store Error : Type} (root_left : bool) source
    (result : Error + (Store * handle)) stored current history :=
  if root_left then (Some (inl (AfterRootAt source)), stored, current, history)
  else match result with
  | inl error => (Some (inr error), stored, current, history)
  | inr (next_store, next) => (None, next_store, next, remember current history)
  end.

Theorem root_left_refuses_an_edge_without_touching_stores_current_or_history
    {Store Error : Type} root_left source (result : Error + (Store * handle))
    (stored : Store) current history :
  root_left = true ->
  guarded_edge_publication root_left source result stored current history =
    (Some (inl (AfterRootAt source)), stored, current, history).
Proof. intros ->. reflexivity. Qed.

Theorem returned_edge_error_preserves_its_exact_source_stores_current_and_history
    {Store Error : Type} source (error : Error) (stored : Store) current history :
  guarded_edge_publication false source (inl error) stored current history =
    (Some (inr error), stored, current, history).
Proof. reflexivity. Qed.

Theorem successful_edges_publish_exactly_the_previous_not_the_returned_state
    {Store Error : Type} source (stored next_store : Store) current next history :
  @guarded_edge_publication Store Error false source (inr (next_store, next))
    stored current history = (None, next_store, next, remember current history).
Proof. reflexivity. Qed.

Theorem a_pop_inspects_one_run_and_performs_exactly_zero_or_one_prefix_slice first rest :
  pop (first :: rest) =
    Some (if depth (latest first) =? first_depth first
      then PopResult (latest first) rest 0
      else PopResult (latest first)
        (Run (shorter (latest first) (depth (latest first) - 1)) (first_depth first) :: rest) 1).
Proof. reflexivity. Qed.


Theorem remembering_touches_only_the_latest_run_and_adds_at_most_one_record previous history :
  length history <= length (remember previous history) /\
  length (remember previous history) <= S (length history).
Proof.
  destruct history as [|first rest]; cbn [remember length]; [lia|].
  destruct (same_state previous (latest first)); cbn; lia.
Qed.

Theorem a_nonsingleton_run_has_a_safe_strict_predecessor first :
  valid_run first -> depth (latest first) <> first_depth first ->
  0 < depth (latest first) /\ first_depth first <= depth (latest first) - 1.
Proof. intros [valid bounded] different. lia. Qed.


Definition run_state current := handle_state (latest current).

Fixpoint maximal history : Prop :=
  match history with
  | [] => True
  | first :: rest =>
      (match rest with [] => True | second :: _ => run_state first <> run_state second end) /\
      maximal rest
  end.

Lemma maximal_tail first rest : maximal (first :: rest) -> maximal rest.
Proof. intros [_ tail]. exact tail. Qed.

Lemma maximal_head_state first replacement rest :
  run_state replacement = run_state first ->
  maximal (first :: rest) -> maximal (replacement :: rest).
Proof.
  intros same normalized. destruct rest as [|second tail]; [exact normalized|].
  change (run_state replacement <> run_state second /\ maximal (second :: tail)).
  change (run_state first <> run_state second /\ maximal (second :: tail)) in normalized.
  now rewrite same.
Qed.

Lemma descending_has_one_exact_state state data count :
  Forall (fun entry => fst entry = state) (descending state data count).
Proof.
  revert data. induction count; intro data; cbn [descending]; [constructor|].
  constructor; [reflexivity|apply IHcount].
Qed.

Theorem every_run_is_a_nonempty_block_of_one_exact_state first :
  expand_run first <> [] /\
  Forall (fun entry => fst entry = run_state first) (expand_run first).
Proof.
  split; [discriminate|]. apply descending_has_one_exact_state.
Qed.

Theorem remembering_preserves_maximal_adjacent_state_runs previous history :
  maximal history -> maximal (remember previous history).
Proof.
  intro normalized. destruct history as [|first rest].
  - cbn [remember maximal]. tauto.
  - cbn [remember]. destruct (same_state previous (latest first)) eqn:same.
    + apply same_state_exact in same.
      eapply maximal_head_state; [|exact normalized].
      exact same.
    + change (handle_state previous <> run_state first /\ maximal (first :: rest)).
      split; [|exact normalized].
      unfold run_state; cbn [latest]. intro equal.
      apply same_state_exact in equal. congruence.
Qed.

Theorem popping_preserves_maximal_adjacent_state_runs history result :
  maximal history -> pop history = Some result -> maximal (remaining_runs result).
Proof.
  intros normalized popped. destruct history as [|first rest]; [discriminate|].
  cbn [pop] in popped.
  destruct (depth (latest first) =? first_depth first);
    inversion popped; subst result; cbn [remaining_runs].
  - now apply maximal_tail in normalized.
  - eapply maximal_head_state; [reflexivity|exact normalized].
Qed.

End Bytes.

Print Assumptions strict_shortening_is_the_existing_checked_source_window.
Print Assumptions zero_prefix_normalization_is_observational.
Print Assumptions compressed_emptiness_is_exact.
Print Assumptions remembering_refines_exact_scalar_predecessor_push.
Print Assumptions popping_refines_exact_scalar_head_and_tail.
Print Assumptions every_compressed_historical_prefix_is_a_valid_located_handle.
Print Assumptions successful_edge_preserves_scalar_topology.
Print Assumptions successful_leave_preserves_scalar_topology.
Print Assumptions byte_preserving_private_operations_preserve_the_exact_undo_topology.
Print Assumptions exact_state_and_located_bytes_preserve_every_selected_graph_and_query.
Print Assumptions compressed_pop_is_the_exact_existing_scalar_world_transition.
Print Assumptions restoring_never_changes_any_checkpoint_frame_owner_or_obligation_clock.
Print Assumptions compressed_topology_depth_and_empty_root_are_exact.
Print Assumptions compression_preserves_root_leave_underflow_and_finish_error_priority.
Print Assumptions root_left_refuses_an_edge_without_touching_stores_current_or_history.
Print Assumptions returned_edge_error_preserves_its_exact_source_stores_current_and_history.
Print Assumptions successful_edges_publish_exactly_the_previous_not_the_returned_state.
Print Assumptions a_pop_inspects_one_run_and_performs_exactly_zero_or_one_prefix_slice.
Print Assumptions remembering_touches_only_the_latest_run_and_adds_at_most_one_record.
Print Assumptions a_nonsingleton_run_has_a_safe_strict_predecessor.
Print Assumptions every_run_is_a_nonempty_block_of_one_exact_state.
Print Assumptions remembering_preserves_maximal_adjacent_state_runs.
Print Assumptions popping_preserves_maximal_adjacent_state_runs.

End CompressedScalarUndo.

Module BalancedSourceComposition.

Import CanonicalKeyProtocol IndexedWindowTransport.

Inductive tree :=
| Leaf (view : window)
| Join2 (cached_length cached_height : nat) (left right : tree)
| Join3 (cached_length cached_height : nat) (left middle right : tree).

Definition size current :=
  match current with
  | Leaf view => window_width view
  | Join2 count _ _ _ | Join3 count _ _ _ _ => count
  end.

Definition height current :=
  match current with
  | Leaf _ => 0
  | Join2 _ rank _ _ | Join3 _ rank _ _ _ => rank
  end.

Definition node2 left right :=
  Join2 (size left + size right) (S (height left)) left right.

Definition node3 left middle right :=
  Join3 (size left + size middle + size right) (S (height left)) left middle right.

Fixpoint terminals current :=
  match current with
  | Leaf view => [view]
  | Join2 _ _ first last => terminals first ++ terminals last
  | Join3 _ _ first middle last => terminals first ++ terminals middle ++ terminals last
  end.

Inductive balanced : tree -> Prop :=
| BalancedLeaf view : 0 < window_width view -> balanced (Leaf view)
| Balanced2 left right :
    balanced left -> balanced right -> height left = height right ->
    balanced (node2 left right)
| Balanced3 left middle right :
    balanced left -> balanced middle -> balanced right ->
    height left = height middle -> height left = height right ->
    balanced (node3 left middle right).

Inductive pieces :=
| One (single : tree)
| Two (first second : tree).

Definition piece_height current :=
  match current with One single => height single | Two first _ => height first end.

Definition piece_size current :=
  match current with One single => size single | Two first second => size first + size second end.

Definition piece_terminals current :=
  match current with
  | One single => terminals single
  | Two first second => terminals first ++ terminals second
  end.

Definition balanced_pieces current :=
  match current with
  | One single => balanced single
  | Two first second => balanced first /\ balanced second /\ height first = height second
  end.

Record join_result := Joined {
  joined_pieces : pieces;
  boundary_visits : nat;
  rebuilt_nodes : nat
}.

Definition absorb_right2 left result :=
  Joined
    (match joined_pieces result with
    | One next => One (node2 left next)
    | Two first second => One (node3 left first second)
    end)
    (S (boundary_visits result)) (S (rebuilt_nodes result)).

Definition absorb_right3 left middle result :=
  Joined
    (match joined_pieces result with
    | One next => One (node3 left middle next)
    | Two first second => Two (node2 left middle) (node2 first second)
    end)
    (S (boundary_visits result))
    (match joined_pieces result with
    | One _ => S (rebuilt_nodes result)
    | Two _ _ => 2 + rebuilt_nodes result
    end).

Definition absorb_left2 result right :=
  Joined
    (match joined_pieces result with
    | One next => One (node2 next right)
    | Two first second => One (node3 first second right)
    end)
    (S (boundary_visits result)) (S (rebuilt_nodes result)).

Definition absorb_left3 result middle right :=
  Joined
    (match joined_pieces result with
    | One next => One (node3 next middle right)
    | Two first second => Two (node2 first second) (node2 middle right)
    end)
    (S (boundary_visits result))
    (match joined_pieces result with
    | One _ => S (rebuilt_nodes result)
    | Two _ _ => 2 + rebuilt_nodes result
    end).

Fixpoint join_right left right :=
  if height left =? height right then Joined (Two left right) 1 0
  else match left with
  | Leaf _ => Joined (Two left right) 1 0
  | Join2 _ _ first last => absorb_right2 first (join_right last right)
  | Join3 _ _ first middle last => absorb_right3 first middle (join_right last right)
  end.

Fixpoint join_left left right {struct right} :=
  if height left =? height right then Joined (Two left right) 1 0
  else match right with
  | Leaf _ => Joined (Two left right) 1 0
  | Join2 _ _ first last => absorb_left2 (join_left left first) last
  | Join3 _ _ first middle last => absorb_left3 (join_left left first) middle last
  end.

Definition finish current :=
  match current with One single => single | Two first second => node2 first second end.

Definition concatenate left right :=
  if height left <=? height right then join_left left right else join_right left right.

Definition concatenate_root left right := finish (joined_pieces (concatenate left right)).

Definition checked_concatenate limit left right :=
  if size left + size right <=? limit then Some (concatenate_root left right) else None.

Lemma balanced_shape2 count rank left right :
  balanced (Join2 count rank left right) ->
  balanced left /\ balanced right /\ height left = height right /\
  count = size left + size right /\ rank = S (height left).
Proof. intro valid. inversion valid; subst. repeat split; assumption || reflexivity. Qed.

Lemma balanced_shape3 count rank left middle right :
  balanced (Join3 count rank left middle right) ->
  balanced left /\ balanced middle /\ balanced right /\
  height left = height middle /\ height left = height right /\
  count = size left + size middle + size right /\ rank = S (height left).
Proof. intro valid. inversion valid; subst. repeat split; assumption || reflexivity. Qed.

Lemma balanced_size_positive current : balanced current -> 0 < size current.
Proof. intro valid. induction valid; cbn [node2 node3 size]; lia. Qed.

Lemma balanced_height_bounded_by_bytes current :
  balanced current -> height current < size current.
Proof.
  intro valid. induction valid; cbn [node2 node3 size height] in *; lia.
Qed.

Definition join_contract left right rank result :=
  balanced_pieces (joined_pieces result) /\
  piece_height (joined_pieces result) = rank /\
  piece_size (joined_pieces result) = size left + size right /\
  piece_terminals (joined_pieces result) = terminals left ++ terminals right.

Lemma absorb_right2_contract first last right result :
  balanced first -> height first = height last ->
  join_contract last right (height last) result ->
  join_contract (node2 first last) right (S (height last)) (absorb_right2 first result).
Proof.
  intros first_valid same (valid & rank & count & contents).
  destruct result as [[next|next tail] visits rebuilt];
    cbn [joined_pieces balanced_pieces piece_height piece_size piece_terminals] in *;
    unfold join_contract, absorb_right2; cbn [joined_pieces piece_height piece_size piece_terminals node2 node3 size height terminals].
  - split; [constructor; [exact first_valid|exact valid|lia]|].
    repeat split; try lia. now rewrite contents, app_assoc.
  - destruct valid as (next_valid & tail_valid & next_same).
    split; [constructor; [exact first_valid|exact next_valid|exact tail_valid|lia|lia]|].
    repeat split; try lia. now rewrite contents, app_assoc.
Qed.

Lemma absorb_right3_contract first middle last right result :
  balanced first -> balanced middle -> height first = height middle ->
  height first = height last ->
  join_contract last right (height last) result ->
  join_contract (node3 first middle last) right (S (height last)) (absorb_right3 first middle result).
Proof.
  intros first_valid middle_valid same_middle same (valid & rank & count & contents).
  destruct result as [[next|next tail] visits rebuilt];
    cbn [joined_pieces balanced_pieces piece_height piece_size piece_terminals] in *;
    unfold join_contract, absorb_right3; cbn [joined_pieces piece_height piece_size piece_terminals node2 node3 size height terminals].
  - split; [constructor; [exact first_valid|exact middle_valid|exact valid|lia|lia]|].
    repeat split; try lia. now rewrite contents, !app_assoc.
  - destruct valid as (next_valid & tail_valid & next_same).
    split.
    + split; [constructor; assumption|]. split; [constructor; assumption|].
      cbn [node2 height]. lia.
    + repeat split; try lia. now rewrite contents, !app_assoc.
Qed.

Lemma absorb_left2_contract left first last result :
  balanced last -> height first = height last ->
  join_contract left first (height first) result ->
  join_contract left (node2 first last) (S (height first)) (absorb_left2 result last).
Proof.
  intros last_valid same (valid & rank & count & contents).
  destruct result as [[next|next tail] visits rebuilt];
    cbn [joined_pieces balanced_pieces piece_height piece_size piece_terminals] in *;
    unfold join_contract, absorb_left2; cbn [joined_pieces piece_height piece_size piece_terminals node2 node3 size height terminals].
  - split; [constructor; [exact valid|exact last_valid|lia]|].
    repeat split; try lia. now rewrite contents, app_assoc.
  - destruct valid as (next_valid & tail_valid & next_same).
    split; [constructor; [exact next_valid|exact tail_valid|exact last_valid|lia|lia]|].
    repeat split; try lia. now rewrite !app_assoc, contents.
Qed.

Lemma absorb_left3_contract left first middle last result :
  balanced middle -> balanced last -> height first = height middle ->
  height first = height last ->
  join_contract left first (height first) result ->
  join_contract left (node3 first middle last) (S (height first)) (absorb_left3 result middle last).
Proof.
  intros middle_valid last_valid same_middle same (valid & rank & count & contents).
  destruct result as [[next|next tail] visits rebuilt];
    cbn [joined_pieces balanced_pieces piece_height piece_size piece_terminals] in *;
    unfold join_contract, absorb_left3; cbn [joined_pieces piece_height piece_size piece_terminals node2 node3 size height terminals].
  - split; [constructor; [exact valid|exact middle_valid|exact last_valid|lia|lia]|].
    repeat split; try lia. now rewrite contents, !app_assoc.
  - destruct valid as (next_valid & tail_valid & next_same).
    split.
    + split; [constructor; assumption|]. split; [constructor; [exact middle_valid|exact last_valid|lia]|].
      cbn [node2 height]. lia.
    + repeat split; try lia. now rewrite contents, !app_assoc.
Qed.

Lemma equal_height_contract left right :
  balanced left -> balanced right -> height left = height right ->
  join_contract left right (height left) (Joined (Two left right) 1 0).
Proof. intros left_valid right_valid same. repeat split; assumption || reflexivity. Qed.

Lemma join_right_equal left right :
  height left = height right -> join_right left right = Joined (Two left right) 1 0.
Proof.
  intro same. destruct left; cbn [join_right]; rewrite <- same, Nat.eqb_refl; reflexivity.
Qed.

Lemma join_left_equal left right :
  height left = height right -> join_left left right = Joined (Two left right) 1 0.
Proof.
  intro same. destruct right; cbn [join_left]; rewrite same, Nat.eqb_refl; reflexivity.
Qed.

Theorem right_spine_rebuilding_keeps_the_exact_other_children first last right :
  height (node2 first last) <> height right ->
  join_right (node2 first last) right = absorb_right2 first (join_right last right).
Proof.
  intro different. cbn [join_right node2 height].
  assert (S (height first) =? height right = false) by (apply Nat.eqb_neq; exact different).
  now rewrite H.
Qed.

Theorem right_three_spine_rebuilding_keeps_the_exact_other_children first middle last right :
  height (node3 first middle last) <> height right ->
  join_right (node3 first middle last) right = absorb_right3 first middle (join_right last right).
Proof.
  intro different. cbn [join_right node3 height].
  assert (S (height first) =? height right = false) by (apply Nat.eqb_neq; exact different).
  now rewrite H.
Qed.

Theorem left_spine_rebuilding_keeps_the_exact_other_children left first last :
  height left <> height (node2 first last) ->
  join_left left (node2 first last) = absorb_left2 (join_left left first) last.
Proof.
  intro different. cbn [join_left node2 height].
  assert (height left =? S (height first) = false) by (apply Nat.eqb_neq; exact different).
  now rewrite H.
Qed.

Theorem left_three_spine_rebuilding_keeps_the_exact_other_children left first middle last :
  height left <> height (node3 first middle last) ->
  join_left left (node3 first middle last) = absorb_left3 (join_left left first) middle last.
Proof.
  intro different. cbn [join_left node3 height].
  assert (height left =? S (height first) = false) by (apply Nat.eqb_neq; exact different).
  now rewrite H.
Qed.

Theorem concrete_right_join_preserves_balance_caches_terminals_and_boundary_work left :
  balanced left -> forall right,
  balanced right -> height right <= height left ->
  join_contract left right (height left) (join_right left right) /\
  boundary_visits (join_right left right) = S (height left - height right) /\
  rebuilt_nodes (join_right left right) <= 2 * (height left - height right).
Proof.
  intro left_valid.
  induction left_valid as [view nonempty|first last first_valid first_IH last_valid last_IH same|
    first middle last first_valid first_IH middle_valid middle_IH last_valid last_IH same_middle same];
    intros right right_valid ordered.
  - assert (equal : height (Leaf view) = height right) by (cbn [height] in *; lia).
    rewrite join_right_equal by exact equal.
    split; [apply equal_height_contract; [constructor; exact nonempty|exact right_valid|exact equal]|].
    cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
  - destruct (Nat.eq_dec (height (node2 first last)) (height right)) as [equal|different].
    + rewrite join_right_equal by exact equal.
      split; [apply equal_height_contract; [constructor; assumption|exact right_valid|exact equal]|].
      cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
    + rewrite right_spine_rebuilding_keeps_the_exact_other_children by exact different.
      assert (lower : height right <= height last) by (cbn [node2 height] in *; lia).
      destruct (last_IH right right_valid lower) as [contract [visits rebuilt]].
      split.
      * cbn [node2 height]. rewrite same.
        now apply absorb_right2_contract.
      * unfold absorb_right2. cbn [boundary_visits rebuilt_nodes node2 height]. rewrite visits.
        split; lia.
  - destruct (Nat.eq_dec (height (node3 first middle last)) (height right)) as [equal|different].
    + rewrite join_right_equal by exact equal.
      split; [apply equal_height_contract; [constructor; assumption|exact right_valid|exact equal]|].
      cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
    + rewrite right_three_spine_rebuilding_keeps_the_exact_other_children by exact different.
      assert (lower : height right <= height last) by (cbn [node3 height] in *; lia).
      destruct (last_IH right right_valid lower) as [contract [visits rebuilt]].
      split.
      * cbn [node3 height]. rewrite same.
        now apply absorb_right3_contract.
      * unfold absorb_right3. cbn [boundary_visits rebuilt_nodes node3 height]. rewrite visits.
        destruct (joined_pieces (join_right last right)); split; lia.
Qed.

Theorem concrete_left_join_preserves_balance_caches_terminals_and_boundary_work right :
  balanced right -> forall left,
  balanced left -> height left <= height right ->
  join_contract left right (height right) (join_left left right) /\
  boundary_visits (join_left left right) = S (height right - height left) /\
  rebuilt_nodes (join_left left right) <= 2 * (height right - height left).
Proof.
  intro right_valid.
  induction right_valid as [view nonempty|first last first_valid first_IH last_valid last_IH same|
    first middle last first_valid first_IH middle_valid middle_IH last_valid last_IH same_middle same];
    intros left left_valid ordered.
  - assert (equal : height left = height (Leaf view)) by (cbn [height] in *; lia).
    rewrite join_left_equal by exact equal.
    split; [rewrite <- equal; apply equal_height_contract; [exact left_valid|constructor; exact nonempty|exact equal]|].
    cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
  - destruct (Nat.eq_dec (height left) (height (node2 first last))) as [equal|different].
    + rewrite join_left_equal by exact equal.
      split; [rewrite <- equal; apply equal_height_contract; [exact left_valid|constructor; assumption|exact equal]|].
      cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
    + rewrite left_spine_rebuilding_keeps_the_exact_other_children by exact different.
      assert (lower : height left <= height first) by (cbn [node2 height] in *; lia).
      destruct (first_IH left left_valid lower) as [contract [visits rebuilt]].
      split.
      * now apply absorb_left2_contract.
      * unfold absorb_left2. cbn [boundary_visits rebuilt_nodes node2 height]. rewrite visits.
        split; lia.
  - destruct (Nat.eq_dec (height left) (height (node3 first middle last))) as [equal|different].
    + rewrite join_left_equal by exact equal.
      split; [rewrite <- equal; apply equal_height_contract; [exact left_valid|constructor; assumption|exact equal]|].
      cbn [boundary_visits rebuilt_nodes]. rewrite equal, Nat.sub_diag. split; reflexivity.
    + rewrite left_three_spine_rebuilding_keeps_the_exact_other_children by exact different.
      assert (lower : height left <= height first) by (cbn [node3 height] in *; lia).
      destruct (first_IH left left_valid lower) as [contract [visits rebuilt]].
      split.
      * now apply absorb_left3_contract.
      * unfold absorb_left3. cbn [boundary_visits rebuilt_nodes node3 height]. rewrite visits.
        destruct (joined_pieces (join_left left first)); split; lia.
Qed.

Definition height_gap left right :=
  Nat.max (height left) (height right) - Nat.min (height left) (height right).

Definition total_rebuilt result :=
  rebuilt_nodes result +
    match joined_pieces result with One _ => 0 | Two _ _ => 1 end.

Lemma finish_preserves_piece_contract current :
  balanced_pieces current ->
  balanced (finish current) /\
  size (finish current) = piece_size current /\
  terminals (finish current) = piece_terminals current /\
  piece_height current <= height (finish current) <= S (piece_height current).
Proof.
  destruct current as [single|first second]; cbn [balanced_pieces finish piece_size piece_terminals piece_height].
  - intro valid. repeat split; assumption || reflexivity || lia.
  - intros (first_valid & second_valid & same).
    split; [constructor; assumption|]. cbn [node2 size height terminals].
    repeat split; reflexivity || lia.
Qed.

Theorem concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost left right :
  balanced left -> balanced right ->
  balanced (concatenate_root left right) /\
  size (concatenate_root left right) = size left + size right /\
  terminals (concatenate_root left right) = terminals left ++ terminals right /\
  Nat.max (height left) (height right) <= height (concatenate_root left right) <=
    S (Nat.max (height left) (height right)) /\
  boundary_visits (concatenate left right) = S (height_gap left right) /\
  total_rebuilt (concatenate left right) <= S (2 * height_gap left right).
Proof.
  intros left_valid right_valid.
  unfold concatenate_root, concatenate, height_gap.
  destruct (height left <=? height right) eqn:direction.
  - apply Nat.leb_le in direction.
    destruct (concrete_left_join_preserves_balance_caches_terminals_and_boundary_work
      right_valid left_valid direction) as [(valid & rank & count & contents) [visits rebuilt]].
    destruct (finish_preserves_piece_contract _ valid) as (finished & same_size & same_terminals & bounds).
    rewrite Nat.max_r, Nat.min_l by exact direction.
    split; [exact finished|]. split; [now rewrite same_size|].
    split; [now rewrite same_terminals|]. split; [now rewrite <- rank|].
    split; [exact visits|]. unfold total_rebuilt.
    destruct (joined_pieces (join_left left right)); lia.
  - apply Nat.leb_gt in direction.
    destruct (concrete_right_join_preserves_balance_caches_terminals_and_boundary_work
      left_valid right_valid ltac:(lia)) as [(valid & rank & count & contents) [visits rebuilt]].
    destruct (finish_preserves_piece_contract _ valid) as (finished & same_size & same_terminals & bounds).
    rewrite Nat.max_l, Nat.min_r by lia.
    split; [exact finished|]. split; [now rewrite same_size|].
    split; [now rewrite same_terminals|]. split; [now rewrite <- rank|].
    split; [exact visits|]. unfold total_rebuilt.
    destruct (joined_pieces (join_right left right)); lia.
Qed.

Fixpoint native_metadata limit current : Prop :=
  size current <= limit /\ height current <= limit /\
  match current with
  | Leaf _ => True
  | Join2 _ _ first last => native_metadata limit first /\ native_metadata limit last
  | Join3 _ _ first middle last =>
      native_metadata limit first /\ native_metadata limit middle /\ native_metadata limit last
  end.

Lemma balanced_sizes_bound_every_cached_sum_and_height limit current :
  balanced current -> size current <= limit -> native_metadata limit current.
Proof.
  intro valid. induction valid; intro fits.
  - cbn [native_metadata]. split; [exact fits|]. split; [cbn [height]; lia|exact I].
  - change (size (node2 left right) <= limit /\ height (node2 left right) <= limit /\
      native_metadata limit left /\ native_metadata limit right).
    split; [exact fits|]. split.
    + pose proof (balanced_height_bounded_by_bytes (Balanced2 valid1 valid2 H)). lia.
    + split; [apply IHvalid1|apply IHvalid2]; cbn [node2 size] in fits; lia.
  - change (size (node3 left middle right) <= limit /\ height (node3 left middle right) <= limit /\
      native_metadata limit left /\ native_metadata limit middle /\ native_metadata limit right).
    split; [exact fits|]. split.
    + pose proof (balanced_height_bounded_by_bytes (Balanced3 valid1 valid2 valid3 H H0)). lia.
    + split; [apply IHvalid1|split; [apply IHvalid2|apply IHvalid3]]; cbn [node3 size] in fits; lia.
Qed.

Theorem checked_concatenation_has_only_the_exact_native_length_preflight limit left right result :
  balanced left -> balanced right ->
  (checked_concatenate limit left right = Some result <->
    size left + size right <= limit /\ result = concatenate_root left right).
Proof.
  intros left_valid right_valid. unfold checked_concatenate.
  destruct (size left + size right <=? limit) eqn:fits.
  - apply Nat.leb_le in fits. split; [intro same; inversion same; subst; tauto|intros [_ ->]; reflexivity].
  - apply Nat.leb_gt in fits. split; [discriminate|intros [bound _]; lia].
Qed.

Theorem checked_concatenation_preserves_all_native_cached_lengths_and_heights limit left right result :
  balanced left -> balanced right ->
  checked_concatenate limit left right = Some result ->
  balanced result /\ size result = size left + size right /\ native_metadata limit result.
Proof.
  intros left_valid right_valid accepted.
  apply checked_concatenation_has_only_the_exact_native_length_preflight in accepted; try assumption.
  destruct accepted as [fits ->].
  destruct (concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost left_valid right_valid)
    as [valid [count rest]].
  split; [exact valid|]. split; [exact count|].
  apply balanced_sizes_bound_every_cached_sum_and_height; [exact valid|now rewrite count].
Qed.

Theorem balanced_height_has_a_concrete_exponential_terminal_lower_bound current :
  balanced current -> 2 ^ height current <= length (terminals current).
Proof.
  intro valid. induction valid; cbn [node2 node3 terminals height].
  - reflexivity.
  - rewrite length_app, Nat.pow_succ_r by lia. rewrite <- H in IHvalid2. lia.
  - rewrite !length_app, Nat.pow_succ_r by lia.
    rewrite <- H in IHvalid2. rewrite <- H0 in IHvalid3. lia.
Qed.

Section LocatedDenotation.
Variable roots : nat -> region.

Definition terminal_bytes view := window_bytes (roots (window_owner view)) view.
Definition denotation current := concat (map terminal_bytes (terminals current)).
Definition valid_sources current :=
  Forall (fun view => window_valid (roots (window_owner view)) view) (terminals current).

Lemma balanced_cache_is_the_sum_of_terminal_widths current :
  balanced current -> size current = fold_right (fun view rest => window_width view + rest) 0 (terminals current).
Proof.
  intro valid. induction valid; cbn [size node2 node3 terminals].
  - cbn [fold_right]. lia.
  - rewrite fold_right_app. rewrite <- IHvalid2.
    assert (sum_app : forall views tail,
      fold_right (fun view rest => window_width view + rest) tail views =
      fold_right (fun view rest => window_width view + rest) 0 views + tail).
    { induction views; intro tail; cbn; [reflexivity|rewrite IHviews; lia]. }
    rewrite sum_app, <- IHvalid1. reflexivity.
  - assert (sum_app : forall views tail,
      fold_right (fun view rest => window_width view + rest) tail views =
      fold_right (fun view rest => window_width view + rest) 0 views + tail).
    { induction views; intro tail; cbn; [reflexivity|rewrite IHviews; lia]. }
    rewrite !fold_right_app.
    rewrite (sum_app (terminals left)), (sum_app (terminals middle)).
    rewrite <- IHvalid1, <- IHvalid2, <- IHvalid3. lia.
Qed.

Lemma source_validity_gives_exact_denotation_length current :
  balanced current -> valid_sources current -> length (denotation current) = size current.
Proof.
  intros balanced_current valid.
  rewrite balanced_cache_is_the_sum_of_terminal_widths by exact balanced_current.
  unfold denotation. unfold valid_sources in valid.
  induction valid as [|view rest head_valid rest_valid IH]; cbn; [reflexivity|].
  rewrite length_app, IH. unfold terminal_bytes. now rewrite valid_window_length.
Qed.

Theorem actual_join_preserves_arbitrary_located_bytes_and_valid_original_sources left right :
  balanced left -> balanced right -> valid_sources left -> valid_sources right ->
  denotation (concatenate_root left right) = denotation left ++ denotation right /\
  valid_sources (concatenate_root left right) /\
  length (denotation (concatenate_root left right)) = size left + size right.
Proof.
  intros left_balanced right_balanced left_valid right_valid.
  destruct (concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost left_balanced right_balanced)
    as [balanced_result [count [ordered rest]]].
  assert (valid_result : valid_sources (concatenate_root left right)).
  { unfold valid_sources. rewrite ordered. apply Forall_app. tauto. }
  split.
  - unfold denotation. rewrite ordered, map_app, concat_app. reflexivity.
  - split; [exact valid_result|].
    rewrite source_validity_gives_exact_denotation_length; assumption.
Qed.

Definition checked_leaf limit view :=
  if (0 <? window_width view) &&
      (window_owner view =? region_owner (roots (window_owner view))) &&
      (window_start view + window_width view <=? length (region_bytes (roots (window_owner view)))) &&
      (window_start view + window_width view <=? limit)
  then Some (Leaf view) else None.

Theorem checked_terminal_leaf_retains_the_exact_original_window limit view result :
  checked_leaf limit view = Some result <->
  0 < window_width view /\ window_valid (roots (window_owner view)) view /\
  window_start view + window_width view <= limit /\ result = Leaf view.
Proof.
  unfold checked_leaf.
  destruct ((0 <? window_width view) &&
      (window_owner view =? region_owner (roots (window_owner view))) &&
      (window_start view + window_width view <=? length (region_bytes (roots (window_owner view)))) &&
      (window_start view + window_width view <=? limit)) eqn:guard.
  - repeat rewrite andb_true_iff in guard.
    rewrite Nat.ltb_lt, Nat.eqb_eq, !Nat.leb_le in guard.
    unfold window_valid. split; [intro same; inversion same; subst; tauto|intros [_ [_ [_ ->]]]; reflexivity].
  - split; [discriminate|]. intros (nonempty & [same valid] & fits & ->).
    assert (guard_true : (0 <? window_width view) &&
      (window_owner view =? region_owner (roots (window_owner view))) &&
      (window_start view + window_width view <=? length (region_bytes (roots (window_owner view)))) &&
      (window_start view + window_width view <=? limit) = true).
    { repeat rewrite andb_true_iff. rewrite Nat.ltb_lt, Nat.eqb_eq, !Nat.leb_le. tauto. }
    congruence.
Qed.

Theorem checked_terminal_leaf_has_exact_bytes_and_representable_metadata limit view result :
  checked_leaf limit view = Some result ->
  balanced result /\ valid_sources result /\ native_metadata limit result /\
  denotation result = window_bytes (roots (window_owner view)) view.
Proof.
  intro accepted. apply checked_terminal_leaf_retains_the_exact_original_window in accepted.
  destruct accepted as (nonempty & valid & fits & ->).
  split; [constructor; exact nonempty|].
  split; [constructor; [exact valid|constructor]|].
  split.
  - apply balanced_sizes_bound_every_cached_sum_and_height; [constructor; exact nonempty|cbn [size]; lia].
  - unfold denotation; cbn [terminals map concat]. now rewrite app_nil_r.
Qed.

End LocatedDenotation.

Inductive subtree : tree -> tree -> Prop :=
| SubtreeRoot current : subtree current current
| Subtree2Left current count rank first last :
    subtree current first -> subtree current (Join2 count rank first last)
| Subtree2Right current count rank first last :
    subtree current last -> subtree current (Join2 count rank first last)
| Subtree3Left current count rank first middle last :
    subtree current first -> subtree current (Join3 count rank first middle last)
| Subtree3Middle current count rank first middle last :
    subtree current middle -> subtree current (Join3 count rank first middle last)
| Subtree3Right current count rank first middle last :
    subtree current last -> subtree current (Join3 count rank first middle last).

Lemma subtree_transitive first middle last :
  subtree first middle -> subtree middle last -> subtree first last.
Proof. intros inside outside. induction outside; eauto using subtree. Qed.

Definition reference_closed (available : tree -> Prop) :=
  forall parent child, available parent -> subtree child parent -> available child.

Inductive reference_assembly (available : tree -> Prop) : tree -> nat -> Prop :=
| RetainReference current : available current -> reference_assembly available current 0
| Allocate2 first last first_cost last_cost total :
    reference_assembly available first first_cost ->
    reference_assembly available last last_cost ->
    total = S (first_cost + last_cost) ->
    reference_assembly available (node2 first last) total
| Allocate3 first middle last first_cost middle_cost last_cost total :
    reference_assembly available first first_cost ->
    reference_assembly available middle middle_cost ->
    reference_assembly available last last_cost ->
    total = S (first_cost + middle_cost + last_cost) ->
    reference_assembly available (node3 first middle last) total.

Definition piece_assembly available current cost :=
  match current with
  | One single => reference_assembly available single cost
  | Two first second => exists first_cost second_cost,
      reference_assembly available first first_cost /\
      reference_assembly available second second_cost /\ cost = first_cost + second_cost
  end.

Lemma reference_absorb_right2 available first result :
  available first ->
  piece_assembly available (joined_pieces result) (rebuilt_nodes result) ->
  piece_assembly available (joined_pieces (absorb_right2 first result))
    (rebuilt_nodes (absorb_right2 first result)).
Proof.
  intros retained built. destruct result as [[next|next tail] visits cost];
    cbn [piece_assembly joined_pieces rebuilt_nodes] in built;
    cbn [absorb_right2 joined_pieces rebuilt_nodes piece_assembly].
  - eapply Allocate2; [constructor; exact retained|exact built|reflexivity].
  - destruct built as (next_cost & tail_cost & next_built & tail_built & ->).
    eapply Allocate3; [constructor; exact retained|exact next_built|exact tail_built|reflexivity].
Qed.

Lemma reference_absorb_right3 available first middle result :
  available first -> available middle ->
  piece_assembly available (joined_pieces result) (rebuilt_nodes result) ->
  piece_assembly available (joined_pieces (absorb_right3 first middle result))
    (rebuilt_nodes (absorb_right3 first middle result)).
Proof.
  intros first_retained middle_retained built.
  destruct result as [[next|next tail] visits cost];
    cbn [piece_assembly joined_pieces rebuilt_nodes] in built;
    cbn [absorb_right3 joined_pieces rebuilt_nodes piece_assembly].
  - eapply Allocate3; [constructor; exact first_retained|constructor; exact middle_retained|exact built|reflexivity].
  - destruct built as (next_cost & tail_cost & next_built & tail_built & ->).
    exists 1, (S (next_cost + tail_cost)). repeat split.
    + eapply Allocate2; [constructor; exact first_retained|constructor; exact middle_retained|reflexivity].
    + eapply Allocate2; [exact next_built|exact tail_built|reflexivity].
Qed.

Lemma reference_absorb_left2 available result last :
  available last ->
  piece_assembly available (joined_pieces result) (rebuilt_nodes result) ->
  piece_assembly available (joined_pieces (absorb_left2 result last))
    (rebuilt_nodes (absorb_left2 result last)).
Proof.
  intros retained built. destruct result as [[next|next tail] visits cost];
    cbn [piece_assembly joined_pieces rebuilt_nodes] in built;
    cbn [absorb_left2 joined_pieces rebuilt_nodes piece_assembly].
  - eapply Allocate2; [exact built|constructor; exact retained|lia].
  - destruct built as (next_cost & tail_cost & next_built & tail_built & ->).
    eapply Allocate3; [exact next_built|exact tail_built|constructor; exact retained|lia].
Qed.

Lemma reference_absorb_left3 available result middle last :
  available middle -> available last ->
  piece_assembly available (joined_pieces result) (rebuilt_nodes result) ->
  piece_assembly available (joined_pieces (absorb_left3 result middle last))
    (rebuilt_nodes (absorb_left3 result middle last)).
Proof.
  intros middle_retained last_retained built.
  destruct result as [[next|next tail] visits cost];
    cbn [piece_assembly joined_pieces rebuilt_nodes] in built;
    cbn [absorb_left3 joined_pieces rebuilt_nodes piece_assembly].
  - eapply Allocate3; [exact built|constructor; exact middle_retained|constructor; exact last_retained|lia].
  - destruct built as (next_cost & tail_cost & next_built & tail_built & ->).
    exists (S (next_cost + tail_cost)), 1. repeat split.
    + eapply Allocate2; [exact next_built|exact tail_built|reflexivity].
    + eapply Allocate2; [constructor; exact middle_retained|constructor; exact last_retained|reflexivity].
    + lia.
Qed.

Lemma joined_pair_retains_both_exact_references available first last :
  available first -> available last ->
  piece_assembly available (Two first last) 0.
Proof.
  intros first_retained last_retained. exists 0, 0.
  repeat split; constructor; assumption.
Qed.

Lemma right_join_has_an_exact_reference_assembly available left :
  reference_closed available -> available left -> forall right, available right ->
  piece_assembly available (joined_pieces (join_right left right)) (rebuilt_nodes (join_right left right)).
Proof.
  intros closed. induction left as [view|count rank first first_IH last last_IH|
    count rank first first_IH middle middle_IH last last_IH]; intros retained right right_retained;
    cbn [join_right]; destruct (height _ =? height right).
  all: try (apply joined_pair_retains_both_exact_references; assumption).
  - apply reference_absorb_right2.
    + eapply closed; [exact retained|apply Subtree2Left; constructor].
    + apply last_IH; [eapply closed; [exact retained|apply Subtree2Right; constructor]|exact right_retained].
  - apply reference_absorb_right3.
    + eapply closed; [exact retained|apply Subtree3Left; constructor].
    + eapply closed; [exact retained|apply Subtree3Middle; constructor].
    + apply last_IH; [eapply closed; [exact retained|apply Subtree3Right; constructor]|exact right_retained].
Qed.

Lemma left_join_has_an_exact_reference_assembly available right :
  reference_closed available -> available right -> forall left, available left ->
  piece_assembly available (joined_pieces (join_left left right)) (rebuilt_nodes (join_left left right)).
Proof.
  intros closed. induction right as [view|count rank first first_IH last last_IH|
    count rank first first_IH middle middle_IH last last_IH]; intros retained left left_retained;
    cbn [join_left]; destruct (height left =? height _).
  all: try (apply joined_pair_retains_both_exact_references; assumption).
  - apply reference_absorb_left2.
    + eapply closed; [exact retained|apply Subtree2Right; constructor].
    + apply first_IH; [eapply closed; [exact retained|apply Subtree2Left; constructor]|exact left_retained].
  - apply reference_absorb_left3.
    + eapply closed; [exact retained|apply Subtree3Middle; constructor].
    + eapply closed; [exact retained|apply Subtree3Right; constructor].
    + apply first_IH; [eapply closed; [exact retained|apply Subtree3Left; constructor]|exact left_retained].
Qed.

Definition input_reference left right candidate :=
  subtree candidate left \/ subtree candidate right.

Theorem concatenation_reuses_exact_input_subtree_references_with_the_counted_new_joins left right :
  reference_assembly (input_reference left right) (concatenate_root left right)
    (total_rebuilt (concatenate left right)).
Proof.
  assert (closed : reference_closed (input_reference left right)).
  { intros parent child retained inside. destruct retained as [inside_left|inside_right];
      [left|right]; eapply subtree_transitive; eassumption. }
  assert (left_retained : input_reference left right left) by (left; constructor).
  assert (right_retained : input_reference left right right) by (right; constructor).
  assert (built : piece_assembly (input_reference left right)
    (joined_pieces (concatenate left right)) (rebuilt_nodes (concatenate left right))).
  { unfold concatenate. destruct (height left <=? height right).
    - apply left_join_has_an_exact_reference_assembly; assumption.
    - apply right_join_has_an_exact_reference_assembly; assumption. }
  unfold concatenate_root, total_rebuilt.
  destruct (joined_pieces (concatenate left right)) as [single|first second];
    cbn [finish piece_assembly] in *.
  - now rewrite Nat.add_0_r.
  - destruct built as (first_cost & second_cost & first_built & second_built & count).
    eapply Allocate2; [exact first_built|exact second_built|lia].
Qed.

Definition root := option tree.

Definition root_size current := match current with None => 0 | Some value => size value end.

Definition checked_root_concatenate limit (left right : root) :=
  match left, right with
  | None, _ => if root_size right <=? limit then Some right else None
  | _, None => if root_size left <=? limit then Some left else None
  | Some first, Some last => option_map (@Some tree) (checked_concatenate limit first last)
  end.

Theorem empty_data_roots_are_checked_identities_without_selecting_an_endpoint limit current :
  root_size current <= limit ->
  checked_root_concatenate limit None current = Some current /\
  checked_root_concatenate limit current None = Some current.
Proof.
  intro fits. assert (root_size current <=? limit = true) by (apply Nat.leb_le; exact fits).
  split.
  - cbn [checked_root_concatenate]. now rewrite H.
  - destruct current; cbn [checked_root_concatenate]; now rewrite H.
Qed.

Definition publish_root (stored : list tree) current := (stored ++ [current], length stored).

Theorem publishing_a_composed_root_preserves_every_exact_historical_root stored current slot historical :
  nth_error stored slot = Some historical ->
  nth_error (fst (publish_root stored current)) slot = Some historical.
Proof.
  intro found. unfold publish_root; cbn.
  rewrite nth_error_app1; [exact found|].
  apply nth_error_Some. rewrite found. discriminate.
Qed.

Theorem root_publication_returns_the_exact_composed_tree_without_flattening stored current :
  nth_error (fst (publish_root stored current)) (snd (publish_root stored current)) = Some current.
Proof.
  unfold publish_root; cbn. rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. reflexivity.
Qed.

Theorem retaining_a_terminal_source_preserves_each_historical_window stored backing slot historical view :
  nth_error stored slot = Some historical ->
  window_bytes historical view =
    window_bytes
      (match nth_error (fst (NormalizedSourceSpans.retain_source stored backing)) slot with
       | Some retained => retained | None => historical end) view.
Proof.
  intro found.
  rewrite (@NormalizedSourceSpans.retaining_a_source_preserves_every_existing_backing
    stored backing slot historical found).
  reflexivity.
Qed.

Theorem preserved_original_sources_preserve_every_historical_root_denotation
    earlier later current :
  (forall view, In view (terminals current) ->
    earlier (window_owner view) = later (window_owner view)) ->
  denotation earlier current = denotation later current.
Proof.
  intro preserved. unfold denotation, terminal_bytes.
  f_equal. apply map_ext_in. intros view found. now rewrite preserved by exact found.
Qed.

Print Assumptions right_spine_rebuilding_keeps_the_exact_other_children.
Print Assumptions right_three_spine_rebuilding_keeps_the_exact_other_children.
Print Assumptions left_spine_rebuilding_keeps_the_exact_other_children.
Print Assumptions left_three_spine_rebuilding_keeps_the_exact_other_children.
Print Assumptions concrete_right_join_preserves_balance_caches_terminals_and_boundary_work.
Print Assumptions concrete_left_join_preserves_balance_caches_terminals_and_boundary_work.
Print Assumptions concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost.
Print Assumptions checked_concatenation_has_only_the_exact_native_length_preflight.
Print Assumptions checked_concatenation_preserves_all_native_cached_lengths_and_heights.
Print Assumptions balanced_height_has_a_concrete_exponential_terminal_lower_bound.
Print Assumptions actual_join_preserves_arbitrary_located_bytes_and_valid_original_sources.
Print Assumptions checked_terminal_leaf_retains_the_exact_original_window.
Print Assumptions checked_terminal_leaf_has_exact_bytes_and_representable_metadata.
Print Assumptions concatenation_reuses_exact_input_subtree_references_with_the_counted_new_joins.
Print Assumptions empty_data_roots_are_checked_identities_without_selecting_an_endpoint.
Print Assumptions publishing_a_composed_root_preserves_every_exact_historical_root.
Print Assumptions root_publication_returns_the_exact_composed_tree_without_flattening.
Print Assumptions retaining_a_terminal_source_preserves_each_historical_window.
Print Assumptions preserved_original_sources_preserve_every_historical_root_denotation.

End BalancedSourceComposition.

Module BalancedSourceConcatDriver.

Import BalancedSourceComposition.

Inductive continuation :=
| Right2 (first : tree)
| Right3 (first middle : tree)
| Left2 (last : tree)
| Left3 (middle last : tree).

Definition absorb saved result :=
  match saved with
  | Right2 first => absorb_right2 first result
  | Right3 first middle => absorb_right3 first middle result
  | Left2 last => absorb_left2 result last
  | Left3 middle last => absorb_left3 result middle last
  end.

Fixpoint resume frames result :=
  match frames with
  | [] => result
  | saved :: rest => resume rest (absorb saved result)
  end.

Record boundary := Boundary {
  pending_frames : list continuation;
  boundary_left : tree;
  boundary_right : tree
}.

Definition boundary_result stopped :=
  Joined (Two (boundary_left stopped) (boundary_right stopped)) 1 0.

Fixpoint seek_right first last frames :=
  if height first =? height last then Boundary frames first last
  else match first with
  | Leaf _ => Boundary frames first last
  | Join2 _ _ left_child right_child => seek_right right_child last (Right2 left_child :: frames)
  | Join3 _ _ left_child middle right_child =>
      seek_right right_child last (Right3 left_child middle :: frames)
  end.

Fixpoint seek_left first last frames {struct last} :=
  if height first =? height last then Boundary frames first last
  else match last with
  | Leaf _ => Boundary frames first last
  | Join2 _ _ left_child right_child => seek_left first left_child (Left2 right_child :: frames)
  | Join3 _ _ left_child middle right_child =>
      seek_left first left_child (Left3 middle right_child :: frames)
  end.

Definition run_boundary stopped := resume (pending_frames stopped) (boundary_result stopped).

Definition driver first last :=
  run_boundary (if height first <=? height last
    then seek_left first last [] else seek_right first last []).

Theorem continuation_pop_applies_exactly_the_selected_absorption saved frames result :
  resume (saved :: frames) result = resume frames (absorb saved result).
Proof. reflexivity. Qed.

Theorem right_descent_and_pop_equal_the_recursive_join_with_every_counter first last frames :
  run_boundary (seek_right first last frames) = resume frames (join_right first last).
Proof.
  revert last frames.
  induction first as [view|count rank left_child left_IH right_child right_IH|
    count rank left_child left_IH middle middle_IH right_child right_IH];
    intros last frames; cbn [seek_right join_right];
    destruct (height _ =? height last); try reflexivity.
  - rewrite right_IH. reflexivity.
  - rewrite right_IH. reflexivity.
Qed.

Theorem left_descent_and_pop_equal_the_recursive_join_with_every_counter first last frames :
  run_boundary (seek_left first last frames) = resume frames (join_left first last).
Proof.
  revert first frames.
  induction last as [view|count rank left_child left_IH right_child right_IH|
    count rank left_child left_IH middle middle_IH right_child right_IH];
    intros first frames; cbn [seek_left join_left];
    destruct (height first =? height _); try reflexivity.
  - rewrite left_IH. reflexivity.
  - rewrite left_IH. reflexivity.
Qed.

Theorem explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations first last :
  driver first last = concatenate first last.
Proof.
  unfold driver, concatenate. destruct (height first <=? height last).
  - apply left_descent_and_pop_equal_the_recursive_join_with_every_counter.
  - apply right_descent_and_pop_equal_the_recursive_join_with_every_counter.
Qed.

Definition checked_driver limit first last :=
  if size first + size last <=? limit
  then Some (finish (joined_pieces (driver first last))) else None.

Theorem explicit_stack_driver_preserves_the_exact_native_preflight_and_published_tree
    limit first last :
  checked_driver limit first last = checked_concatenate limit first last.
Proof.
  unfold checked_driver, checked_concatenate, concatenate_root.
  now rewrite explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations.
Qed.

Lemma resume_visits frames result :
  boundary_visits (resume frames result) = length frames + boundary_visits result.
Proof.
  revert result. induction frames as [|saved rest IH]; intro result; cbn [resume length]; [reflexivity|].
  rewrite IH. destruct saved; cbn [absorb absorb_right2 absorb_right3 absorb_left2 absorb_left3 boundary_visits]; lia.
Qed.

Theorem right_descent_stores_exactly_the_facing_height_gap first last frames :
  balanced first -> balanced last -> height last <= height first ->
  length (pending_frames (seek_right first last frames)) = length frames + height first - height last.
Proof.
  intros first_valid last_valid ordered.
  pose proof (right_descent_and_pop_equal_the_recursive_join_with_every_counter first last frames) as same.
  apply (f_equal boundary_visits) in same.
  unfold run_boundary in same. rewrite !resume_visits in same.
  cbn [boundary_result boundary_visits] in same.
  destruct (concrete_right_join_preserves_balance_caches_terminals_and_boundary_work
    first_valid last_valid ordered) as [_ [visits _]].
  rewrite visits in same. lia.
Qed.

Theorem left_descent_stores_exactly_the_facing_height_gap first last frames :
  balanced first -> balanced last -> height first <= height last ->
  length (pending_frames (seek_left first last frames)) = length frames + height last - height first.
Proof.
  intros first_valid last_valid ordered.
  pose proof (left_descent_and_pop_equal_the_recursive_join_with_every_counter first last frames) as same.
  apply (f_equal boundary_visits) in same.
  unfold run_boundary in same. rewrite !resume_visits in same.
  cbn [boundary_result boundary_visits] in same.
  destruct (concrete_left_join_preserves_balance_caches_terminals_and_boundary_work
    last_valid first_valid ordered) as [_ [visits _]].
  rewrite visits in same. lia.
Qed.

Inductive control :=
| DescendRight (first last : tree)
| DescendLeft (first last : tree)
| Returning (result : join_result).

Record machine := Machine {
  active : control;
  stack : list continuation
}.

Definition step current :=
  match active current with
  | DescendRight first last =>
      if height first =? height last
      then Some (Machine (Returning (Joined (Two first last) 1 0)) (stack current))
      else match first with
      | Leaf _ => Some (Machine (Returning (Joined (Two first last) 1 0)) (stack current))
      | Join2 _ _ left_child right_child =>
          Some (Machine (DescendRight right_child last) (Right2 left_child :: stack current))
      | Join3 _ _ left_child middle right_child =>
          Some (Machine (DescendRight right_child last) (Right3 left_child middle :: stack current))
      end
  | DescendLeft first last =>
      if height first =? height last
      then Some (Machine (Returning (Joined (Two first last) 1 0)) (stack current))
      else match last with
      | Leaf _ => Some (Machine (Returning (Joined (Two first last) 1 0)) (stack current))
      | Join2 _ _ left_child right_child =>
          Some (Machine (DescendLeft first left_child) (Left2 right_child :: stack current))
      | Join3 _ _ left_child middle right_child =>
          Some (Machine (DescendLeft first left_child) (Left3 middle right_child :: stack current))
      end
  | Returning result =>
      match stack current with
      | [] => None
      | saved :: rest => Some (Machine (Returning (absorb saved result)) rest)
      end
  end.

Inductive execution : machine -> nat -> machine -> Prop :=
| Executed current : execution current 0 current
| ExecutedStep current next final count :
    step current = Some next -> execution next count final ->
    execution current (S count) final.

Lemma execution_append first middle last first_count last_count :
  execution first first_count middle -> execution middle last_count last ->
  execution first (first_count + last_count) last.
Proof.
  intros prefix suffix. induction prefix; cbn; [exact suffix|].
  econstructor; eauto.
Qed.

Lemma popping_executes_exactly_the_saved_frame_count frames result :
  execution (Machine (Returning result) frames) (length frames)
    (Machine (Returning (resume frames result)) []).
Proof.
  revert result. induction frames as [|saved rest IH]; intro result; cbn [length resume].
  - constructor.
  - econstructor; [reflexivity|apply IH].
Qed.

Lemma right_descent_executes_to_its_explicit_boundary first last frames :
  exists pushes,
  execution (Machine (DescendRight first last) frames) (S pushes)
    (Machine (Returning (boundary_result (seek_right first last frames)))
      (pending_frames (seek_right first last frames))) /\
  length (pending_frames (seek_right first last frames)) = pushes + length frames.
Proof.
  revert last frames.
  induction first as [view|count rank left_child left_IH right_child right_IH|
    count rank left_child left_IH middle middle_IH right_child right_IH];
    intros last frames; cbn [seek_right];
    destruct (height _ =? height last) eqn:equal.
  all: try (exists 0; split; [econstructor; [cbn [step active stack]; rewrite equal; reflexivity|constructor]|reflexivity]).
  - destruct (right_IH last (Right2 left_child :: frames)) as (pushes & ran & count_same).
    exists (S pushes). split.
    + econstructor; [cbn [step active stack]; rewrite equal; reflexivity|exact ran].
    + cbn [length] in count_same. lia.
  - destruct (right_IH last (Right3 left_child middle :: frames)) as (pushes & ran & count_same).
    exists (S pushes). split.
    + econstructor; [cbn [step active stack]; rewrite equal; reflexivity|exact ran].
    + cbn [length] in count_same. lia.
Qed.

Lemma left_descent_executes_to_its_explicit_boundary first last frames :
  exists pushes,
  execution (Machine (DescendLeft first last) frames) (S pushes)
    (Machine (Returning (boundary_result (seek_left first last frames)))
      (pending_frames (seek_left first last frames))) /\
  length (pending_frames (seek_left first last frames)) = pushes + length frames.
Proof.
  revert first frames.
  induction last as [view|count rank left_child left_IH right_child right_IH|
    count rank left_child left_IH middle middle_IH right_child right_IH];
    intros first frames; cbn [seek_left];
    destruct (height first =? height _) eqn:equal.
  all: try (exists 0; split; [econstructor; [cbn [step active stack]; rewrite equal; reflexivity|constructor]|reflexivity]).
  - destruct (left_IH first (Left2 right_child :: frames)) as (pushes & ran & count_same).
    exists (S pushes). split.
    + econstructor; [cbn [step active stack]; rewrite equal; reflexivity|exact ran].
    + cbn [length] in count_same. lia.
  - destruct (left_IH first (Left3 middle right_child :: frames)) as (pushes & ran & count_same).
    exists (S pushes). split.
    + econstructor; [cbn [step active stack]; rewrite equal; reflexivity|exact ran].
    + cbn [length] in count_same. lia.
Qed.

Definition start first last :=
  Machine (if height first <=? height last then DescendLeft first last else DescendRight first last) [].

Theorem explicit_concat_loop_terminates_without_any_fuel_or_depth_cap first last :
  exists count,
  execution (start first last) count (Machine (Returning (concatenate first last)) []) /\
  step (Machine (Returning (concatenate first last)) []) = None.
Proof.
  unfold start. destruct (height first <=? height last) eqn:direction.
  - destruct (left_descent_executes_to_its_explicit_boundary first last []) as (pushes & descended & frames).
    pose proof (popping_executes_exactly_the_saved_frame_count
      (pending_frames (seek_left first last [])) (boundary_result (seek_left first last []))) as popped.
    pose proof (execution_append descended popped) as ran.
    fold (run_boundary (seek_left first last [])) in ran.
    rewrite left_descent_and_pop_equal_the_recursive_join_with_every_counter in ran.
    cbn [resume] in ran. unfold concatenate. rewrite direction.
    eexists. split; [exact ran|reflexivity].
  - destruct (right_descent_executes_to_its_explicit_boundary first last []) as (pushes & descended & frames).
    pose proof (popping_executes_exactly_the_saved_frame_count
      (pending_frames (seek_right first last [])) (boundary_result (seek_right first last []))) as popped.
    pose proof (execution_append descended popped) as ran.
    fold (run_boundary (seek_right first last [])) in ran.
    rewrite right_descent_and_pop_equal_the_recursive_join_with_every_counter in ran.
    cbn [resume] in ran. unfold concatenate. rewrite direction.
    eexists. split; [exact ran|reflexivity].
Qed.

Definition frontier_bound bound current :=
  match active current with
  | DescendRight first last =>
      balanced first /\ balanced last /\ height last <= height first /\
      length (stack current) + height first - height last <= bound
  | DescendLeft first last =>
      balanced first /\ balanced last /\ height first <= height last /\
      length (stack current) + height last - height first <= bound
  | Returning _ => length (stack current) <= bound
  end.

Lemma frontier_bound_contains_current_stack bound current :
  frontier_bound bound current -> length (stack current) <= bound.
Proof.
  destruct current as [[first last|first last|result] frames];
    cbn [frontier_bound active stack]; intros; lia || tauto.
Qed.

Lemma one_loop_step_preserves_the_frontier_bound bound current next :
  frontier_bound bound current -> step current = Some next -> frontier_bound bound next.
Proof.
  destruct current as [[first last|first last|result] frames];
    cbn [frontier_bound step active stack].
  - intros (first_valid & last_valid & ordered & bounded) stepped.
    destruct (height first =? height last) eqn:equal.
    + inversion stepped; subst next. cbn [frontier_bound active stack]. lia.
    + apply Nat.eqb_neq in equal. destruct first as [view|count rank left_child right_child|
        count rank left_child middle right_child].
      * inversion stepped; subst next. cbn [frontier_bound active stack]. lia.
      * destruct (balanced_shape2 first_valid) as (left_valid & right_valid & same & count_same & rank_same).
        inversion stepped; subst next. cbn [frontier_bound active stack length].
        cbn [height] in ordered, bounded, equal. subst rank.
        repeat split; assumption || lia.
      * destruct (balanced_shape3 first_valid) as
          (left_valid & middle_valid & right_valid & same_middle & same & count_same & rank_same).
        inversion stepped; subst next. cbn [frontier_bound active stack length].
        cbn [height] in ordered, bounded, equal. subst rank.
        repeat split; assumption || lia.
  - intros (first_valid & last_valid & ordered & bounded) stepped.
    destruct (height first =? height last) eqn:equal.
    + inversion stepped; subst next. cbn [frontier_bound active stack]. lia.
    + apply Nat.eqb_neq in equal. destruct last as [view|count rank left_child right_child|
        count rank left_child middle right_child].
      * inversion stepped; subst next. cbn [frontier_bound active stack]. lia.
      * destruct (balanced_shape2 last_valid) as (left_valid & right_valid & same & count_same & rank_same).
        inversion stepped; subst next. cbn [frontier_bound active stack length].
        cbn [height] in ordered, bounded, equal. subst rank.
        repeat split; assumption || lia.
      * destruct (balanced_shape3 last_valid) as
          (left_valid & middle_valid & right_valid & same_middle & same & count_same & rank_same).
        inversion stepped; subst next. cbn [frontier_bound active stack length].
        cbn [height] in ordered, bounded, equal. subst rank.
        repeat split; assumption || lia.
  - intros bounded stepped. destruct frames as [|saved rest]; [discriminate|].
    inversion stepped; subst next. cbn [frontier_bound active stack].
    cbn [length] in bounded. lia.
Qed.

Lemma execution_preserves_the_frontier_bound bound first count last :
  execution first count last -> frontier_bound bound first -> frontier_bound bound last.
Proof.
  intro ran. induction ran; intro bounded; [exact bounded|].
  apply IHran. eapply one_loop_step_preserves_the_frontier_bound; eassumption.
Qed.

Theorem every_reachable_loop_state_uses_at_most_the_original_height_gap_frames
    first last count current :
  balanced first -> balanced last ->
  execution (start first last) count current ->
  length (stack current) <= height_gap first last.
Proof.
  intros first_valid last_valid ran.
  apply frontier_bound_contains_current_stack.
  eapply execution_preserves_the_frontier_bound; [exact ran|].
  unfold start, height_gap. cbn [frontier_bound active stack].
  destruct (height first <=? height last) eqn:direction.
  - apply Nat.leb_le in direction. rewrite Nat.max_r, Nat.min_l by exact direction.
    cbn [frontier_bound active stack length]. repeat split; assumption || lia.
  - apply Nat.leb_gt in direction. rewrite Nat.max_l, Nat.min_r by lia.
    cbn [frontier_bound active stack length]. repeat split; assumption || lia.
Qed.

Theorem balanced_concat_loop_finishes_in_exactly_twice_the_height_gap_plus_one_steps
    first last :
  balanced first -> balanced last ->
  execution (start first last) (S (2 * height_gap first last))
    (Machine (Returning (concatenate first last)) []).
Proof.
  intros first_valid last_valid.
  unfold start. destruct (height first <=? height last) eqn:direction.
  - pose proof direction as ordered. apply Nat.leb_le in ordered.
    destruct (left_descent_executes_to_its_explicit_boundary first last []) as (pushes & descended & frames).
    pose proof (left_descent_stores_exactly_the_facing_height_gap [] first_valid last_valid ordered) as depth.
    cbn [length] in frames, depth.
    pose proof (popping_executes_exactly_the_saved_frame_count
      (pending_frames (seek_left first last [])) (boundary_result (seek_left first last []))) as popped.
    pose proof (execution_append descended popped) as ran.
    fold (run_boundary (seek_left first last [])) in ran.
    rewrite left_descent_and_pop_equal_the_recursive_join_with_every_counter in ran. cbn [resume] in ran.
    unfold concatenate, height_gap. rewrite direction, Nat.max_r, Nat.min_l by exact ordered.
    replace (S (2 * (height last - height first))) with
      (S pushes + length (pending_frames (seek_left first last []))) by lia.
    exact ran.
  - pose proof direction as ordered. apply Nat.leb_gt in ordered.
    destruct (right_descent_executes_to_its_explicit_boundary first last []) as (pushes & descended & frames).
    pose proof (right_descent_stores_exactly_the_facing_height_gap [] first_valid last_valid ltac:(lia)) as depth.
    cbn [length] in frames, depth.
    pose proof (popping_executes_exactly_the_saved_frame_count
      (pending_frames (seek_right first last [])) (boundary_result (seek_right first last []))) as popped.
    pose proof (execution_append descended popped) as ran.
    fold (run_boundary (seek_right first last [])) in ran.
    rewrite right_descent_and_pop_equal_the_recursive_join_with_every_counter in ran. cbn [resume] in ran.
    unfold concatenate, height_gap. rewrite direction, Nat.max_l, Nat.min_r by lia.
    replace (S (2 * (height first - height last))) with
      (S pushes + length (pending_frames (seek_right first last []))) by lia.
    exact ran.
Qed.

Print Assumptions continuation_pop_applies_exactly_the_selected_absorption.
Print Assumptions right_descent_and_pop_equal_the_recursive_join_with_every_counter.
Print Assumptions left_descent_and_pop_equal_the_recursive_join_with_every_counter.
Print Assumptions explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations.
Print Assumptions explicit_stack_driver_preserves_the_exact_native_preflight_and_published_tree.
Print Assumptions right_descent_stores_exactly_the_facing_height_gap.
Print Assumptions left_descent_stores_exactly_the_facing_height_gap.
Print Assumptions explicit_concat_loop_terminates_without_any_fuel_or_depth_cap.
Print Assumptions every_reachable_loop_state_uses_at_most_the_original_height_gap_frames.
Print Assumptions balanced_concat_loop_finishes_in_exactly_twice_the_height_gap_plus_one_steps.

End BalancedSourceConcatDriver.

Module CheckedBalancedSourceSplit.

Import CanonicalKeyProtocol IndexedWindowTransport BalancedSourceComposition.

Definition root_height current := match current with None => 0 | Some value => height value end.
Definition root_terminals current := match current with None => [] | Some value => terminals value end.
Definition root_balanced current := match current with None => True | Some value => balanced value end.

Record glue_result := Glued {
  glued_tree : tree;
  glue_visits : nat;
  glue_rebuilt : nat
}.

Definition freeze_join joined :=
  Glued (finish (joined_pieces joined)) (boundary_visits joined) (total_rebuilt joined).

Definition gr1 partial sibling :=
  match partial with
  | None => Glued sibling 0 0
  | Some value => freeze_join (concatenate value sibling)
  end.

Definition gl1 sibling partial :=
  match partial with
  | None => Glued sibling 0 0
  | Some value => freeze_join (concatenate sibling value)
  end.

Definition gr2 partial first last :=
  match partial with
  | None => Glued (node2 first last) 0 1
  | Some value => freeze_join (absorb_left2 (concatenate value first) last)
  end.

Definition gl2 first last partial :=
  match partial with
  | None => Glued (node2 first last) 0 1
  | Some value => freeze_join (absorb_right2 first (concatenate last value))
  end.

Definition glue_contract partial sibling_height expected_size expected_terminals result :=
  balanced (glued_tree result) /\
  size (glued_tree result) = expected_size /\
  terminals (glued_tree result) = expected_terminals /\
  sibling_height <= height (glued_tree result) <= S sibling_height /\
  root_height partial <= height (glued_tree result) /\
  glue_visits result + root_height partial <= height (glued_tree result) + 1 /\
  glue_rebuilt result + 2 * root_height partial <= 2 * height (glued_tree result) + 1.

Lemma concat_piece_contract first last :
  balanced first -> balanced last ->
  join_contract first last (Nat.max (height first) (height last)) (concatenate first last) /\
  boundary_visits (concatenate first last) = S (height_gap first last) /\
  rebuilt_nodes (concatenate first last) <= 2 * height_gap first last.
Proof.
  intros first_valid last_valid. unfold concatenate, height_gap.
  destruct (height first <=? height last) eqn:direction.
  - apply Nat.leb_le in direction. rewrite Nat.max_r, Nat.min_l by exact direction.
    now apply concrete_left_join_preserves_balance_caches_terminals_and_boundary_work.
  - apply Nat.leb_gt in direction. rewrite Nat.max_l, Nat.min_r by lia.
    apply concrete_right_join_preserves_balance_caches_terminals_and_boundary_work; assumption || lia.
Qed.

Lemma freeze_join_has_height_potential partial first last rank joined :
  join_contract first last rank joined ->
  root_height partial <= rank ->
  boundary_visits joined + root_height partial <= rank + 1 ->
  rebuilt_nodes joined + 2 * root_height partial <= 2 * rank ->
  glue_contract partial rank (size first + size last) (terminals first ++ terminals last)
    (freeze_join joined).
Proof.
  intros (valid & cached_height & cached_size & ordered) partial_height visits rebuilt.
  destruct (finish_preserves_piece_contract _ valid) as (finished & count & contents & heights).
  unfold glue_contract, freeze_join; cbn [glued_tree glue_visits glue_rebuilt].
  split; [exact finished|]. split; [now rewrite count|].
  split; [now rewrite contents|]. split; [now rewrite <- cached_height|].
  split; [lia|]. split; [lia|].
  unfold total_rebuilt. destruct (joined_pieces joined); lia.
Qed.

Theorem one_right_sibling_glue_preserves_order_balance_caches_and_height_potential partial sibling :
  root_balanced partial -> balanced sibling -> root_height partial <= height sibling ->
  glue_contract partial (height sibling) (root_size partial + size sibling)
    (root_terminals partial ++ terminals sibling) (gr1 partial sibling).
Proof.
  destruct partial as [value|]; cbn [root_balanced root_height root_size root_terminals gr1].
  - intros partial_valid sibling_valid ordered.
    destruct (concat_piece_contract partial_valid sibling_valid) as [contract [visits rebuilt]].
    rewrite Nat.max_r in contract by exact ordered.
    unfold height_gap in visits, rebuilt.
    rewrite Nat.max_r, Nat.min_l in visits, rebuilt by exact ordered.
    apply freeze_join_has_height_potential; assumption || (cbn [root_height]; lia).
  - intros _ valid _. unfold glue_contract; cbn [glued_tree glue_visits glue_rebuilt root_height].
    repeat split; assumption || reflexivity || lia.
Qed.

Theorem one_left_sibling_glue_preserves_order_balance_caches_and_height_potential sibling partial :
  root_balanced partial -> balanced sibling -> root_height partial <= height sibling ->
  glue_contract partial (height sibling) (size sibling + root_size partial)
    (terminals sibling ++ root_terminals partial) (gl1 sibling partial).
Proof.
  destruct partial as [value|]; cbn [root_balanced root_height root_size root_terminals gl1].
  - intros partial_valid sibling_valid ordered.
    destruct (concat_piece_contract sibling_valid partial_valid) as [contract [visits rebuilt]].
    rewrite Nat.max_l in contract by exact ordered.
    unfold height_gap in visits, rebuilt.
    rewrite Nat.max_l, Nat.min_r in visits, rebuilt by exact ordered.
    apply freeze_join_has_height_potential; assumption || (cbn [root_height]; lia).
  - intros _ valid _. unfold glue_contract; cbn [glued_tree glue_visits glue_rebuilt root_height].
    rewrite Nat.add_0_r, app_nil_r. repeat split; assumption || reflexivity || lia.
Qed.

Theorem two_right_siblings_glue_preserves_order_without_finishing_the_intermediate_join
    partial first last :
  root_balanced partial -> balanced first -> balanced last ->
  height first = height last -> root_height partial <= height first ->
  glue_contract partial (height first)
    (root_size partial + size first + size last)
    (root_terminals partial ++ terminals first ++ terminals last) (gr2 partial first last) /\
  height (glued_tree (gr2 partial first last)) = S (height first).
Proof.
  destruct partial as [value|]; cbn [root_balanced root_height root_size root_terminals gr2].
  - intros partial_valid first_valid last_valid same ordered.
    destruct (concat_piece_contract partial_valid first_valid) as [contract [visits rebuilt]].
    rewrite Nat.max_r in contract by exact ordered.
    unfold height_gap in visits, rebuilt.
    rewrite Nat.max_r, Nat.min_l in visits, rebuilt by exact ordered.
    destruct contract as (valid & cached_height & cached_size & contents).
    remember (concatenate value first) as joined.
    destruct joined as [[next|next tail] inspected built];
      cbn [joined_pieces piece_height piece_size piece_terminals balanced_pieces boundary_visits rebuilt_nodes] in *;
      unfold glue_contract, freeze_join, absorb_left2, total_rebuilt;
      cbn [glued_tree glue_visits glue_rebuilt joined_pieces rebuilt_nodes boundary_visits finish piece_height root_height node2 node3 size height terminals].
    + split; [|lia]. split; [constructor; assumption || lia|].
      split; [lia|]. split; [now rewrite contents, app_assoc|].
      repeat split; lia.
    + destruct valid as (next_valid & tail_valid & same_tail).
      split; [|lia]. split; [constructor; assumption || lia|].
      split; [lia|]. split; [now rewrite !app_assoc, contents|].
      repeat split; lia.
  - intros _ first_valid last_valid same _. unfold glue_contract.
    cbn [glued_tree glue_visits glue_rebuilt root_height node2 size height terminals].
    split; [|reflexivity]. split; [constructor; assumption|].
    repeat split; reflexivity || lia.
Qed.

Theorem two_left_siblings_glue_preserves_order_without_finishing_the_intermediate_join
    first last partial :
  root_balanced partial -> balanced first -> balanced last ->
  height first = height last -> root_height partial <= height last ->
  glue_contract partial (height last)
    (size first + size last + root_size partial)
    (terminals first ++ terminals last ++ root_terminals partial) (gl2 first last partial) /\
  height (glued_tree (gl2 first last partial)) = S (height last).
Proof.
  destruct partial as [value|]; cbn [root_balanced root_height root_size root_terminals gl2].
  - intros partial_valid first_valid last_valid same ordered.
    destruct (concat_piece_contract last_valid partial_valid) as [contract [visits rebuilt]].
    rewrite Nat.max_l in contract by exact ordered.
    unfold height_gap in visits, rebuilt.
    rewrite Nat.max_l, Nat.min_r in visits, rebuilt by exact ordered.
    destruct contract as (valid & cached_height & cached_size & contents).
    remember (concatenate last value) as joined.
    destruct joined as [[next|next tail] inspected built];
      cbn [joined_pieces piece_height piece_size piece_terminals balanced_pieces boundary_visits rebuilt_nodes] in *;
      unfold glue_contract, freeze_join, absorb_right2, total_rebuilt;
      cbn [glued_tree glue_visits glue_rebuilt joined_pieces rebuilt_nodes boundary_visits finish piece_height root_height node2 node3 size height terminals].
    + split; [|lia]. split; [constructor; assumption || lia|].
      split; [lia|]. split; [now rewrite contents|].
      repeat split; lia.
    + destruct valid as (next_valid & tail_valid & same_tail).
      split; [|lia]. split; [constructor; assumption || lia|].
      split; [lia|]. split; [now rewrite contents|].
      repeat split; lia.
  - intros _ first_valid last_valid same _. unfold glue_contract.
    cbn [glued_tree glue_visits glue_rebuilt root_height node2 size height terminals].
    rewrite Nat.add_0_r, app_nil_r.
    split; [|lia]. split; [constructor; assumption|].
    repeat split; reflexivity || lia.
Qed.

Record split_result := Split {
  split_left : root;
  split_right : root;
  split_visits : nat;
  split_rebuilt : nat
}.

Definition attach_right divided glued :=
  Split (split_left divided) (Some (glued_tree glued))
    (S (split_visits divided + glue_visits glued))
    (split_rebuilt divided + glue_rebuilt glued).

Definition attach_left glued divided :=
  Split (Some (glued_tree glued)) (split_right divided)
    (S (split_visits divided + glue_visits glued))
    (split_rebuilt divided + glue_rebuilt glued).

Definition attach_both before divided after :=
  Split (Some (glued_tree before)) (Some (glued_tree after))
    (S (split_visits divided + glue_visits before + glue_visits after))
    (split_rebuilt divided + glue_rebuilt before + glue_rebuilt after).

Fixpoint split_tree limit current cut :=
  if cut <=? size current then
    if cut =? 0 then Some (Split None (Some current) 1 0)
    else if cut =? size current then Some (Split (Some current) None 1 0)
    else match current with
    | Leaf view =>
        match checked_subwindow limit view 0 cut,
              checked_subwindow limit view cut (window_width view) with
        | Some before, Some after => Some (Split (Some (Leaf before)) (Some (Leaf after)) 1 2)
        | _, _ => None
        end
    | Join2 _ _ first last =>
        if cut <=? size first
        then option_map (fun divided => attach_right divided (gr1 (split_right divided) last))
          (split_tree limit first cut)
        else option_map (fun divided => attach_left (gl1 first (split_left divided)) divided)
          (split_tree limit last (cut - size first))
    | Join3 _ _ first middle last =>
        if cut <=? size first
        then option_map (fun divided => attach_right divided (gr2 (split_right divided) middle last))
          (split_tree limit first cut)
        else if cut <=? size first + size middle
        then option_map (fun divided =>
          attach_both (gl1 first (split_left divided)) divided (gr1 (split_right divided) last))
          (split_tree limit middle (cut - size first))
        else option_map (fun divided => attach_left (gl2 first middle (split_left divided)) divided)
          (split_tree limit last (cut - (size first + size middle)))
    end
  else None.

Definition split_root limit current cut :=
  match current with
  | None => if cut =? 0 then Some (Split None None 1 0) else None
  | Some value => split_tree limit value cut
  end.

Theorem out_of_range_split_cuts_are_refused_before_descent limit current cut :
  size current < cut -> split_tree limit current cut = None.
Proof.
  intro outside. destruct current; cbn [split_tree];
    assert (cut <=? size _ = false) as rejected by (apply Nat.leb_gt; exact outside);
    now rewrite rejected.
Qed.

Theorem zero_and_full_tree_cuts_retain_the_exact_original_root limit current :
  split_tree limit current 0 = Some (Split None (Some current) 1 0) /\
  (0 < size current ->
   split_tree limit current (size current) = Some (Split (Some current) None 1 0)).
Proof.
  split.
  - destruct current; reflexivity.
  - intro positive.
    assert (size current =? 0 = false) as nonzero by (apply Nat.eqb_neq; lia).
    destruct current; cbn [split_tree]; rewrite Nat.leb_refl, nonzero, Nat.eqb_refl; reflexivity.
Qed.

Definition terminal_bounds limit current :=
  Forall (fun view => window_start view + window_width view <= limit) (terminals current).

Theorem checked_terminal_construction_and_concatenation_preserve_representable_source_windows
    roots limit view leaf first last :
  checked_leaf roots limit view = Some leaf ->
  terminal_bounds limit leaf /\
  (balanced first -> balanced last ->
   terminal_bounds limit first -> terminal_bounds limit last ->
   terminal_bounds limit (concatenate_root first last)).
Proof.
  intro accepted.
  apply checked_terminal_leaf_retains_the_exact_original_window in accepted.
  destruct accepted as (nonempty & valid & fits & ->).
  split; [constructor; [exact fits|constructor]|].
  intros first_valid last_valid first_fits last_fits.
  destruct (concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost first_valid last_valid)
    as [_ [_ [ordered _]]].
  unfold terminal_bounds. rewrite ordered. apply Forall_app. tauto.
Qed.

Section Sources.
Variable roots : nat -> region.
Variable limit : nat.
Variable allowed_sources : list nat.

Definition legal_window view :=
  window_valid (roots (window_owner view)) view /\
  window_start view + window_width view <= limit /\
  In (window_owner view) allowed_sources.

Definition legal_root current :=
  root_balanced current /\ Forall legal_window (root_terminals current).

Definition root_bytes current :=
  concat (map (terminal_bytes roots) (root_terminals current)).

Lemma none_is_legal : legal_root None.
Proof. split; [exact I|constructor]. Qed.

Lemma legal_root_length current :
  legal_root current -> length (root_bytes current) = root_size current.
Proof.
  destruct current as [value|]; [|reflexivity].
  intros [balanced_value valid].
  apply source_validity_gives_exact_denotation_length; [exact balanced_value|].
  eapply Forall_impl; [|exact valid]. intros view [within rest]. exact within.
Qed.

Lemma legal_root_from_terminal_equality partial rank expected_size expected_terms glued :
  glue_contract partial rank expected_size expected_terms glued ->
  Forall legal_window expected_terms ->
  legal_root (Some (glued_tree glued)) /\
  root_bytes (Some (glued_tree glued)) = concat (map (terminal_bytes roots) expected_terms).
Proof.
  intros (balanced_glued & count & contents & rest) valid.
  split.
  - split; [exact balanced_glued|]. cbn [root_terminals]. now rewrite contents.
  - unfold root_bytes. cbn [root_terminals]. now rewrite contents.
Qed.

Lemma checked_nonempty_subwindow_preserves_legality_and_original_source view first last child :
  legal_window view -> first < last -> last <= window_width view ->
  checked_subwindow limit view first last = Some child ->
  legal_window child /\ balanced (Leaf child) /\
  window_width child = last - first /\
  window_owner child = window_owner view /\
  window_bytes (roots (window_owner child)) child =
    firstn (last - first) (skipn first (window_bytes (roots (window_owner view)) view)).
Proof.
  intros [valid [fits permitted]] nonempty bounded accepted.
  pose proof (@nested_windows_preserve_owner_bounds_and_exact_slice
    limit (roots (window_owner view)) view first last child valid accepted)
    as (child_valid & same_owner & child_fits & contents).
  pose proof accepted as shape. apply checked_subwindow_exact in shape.
  destruct shape as (_ & _ & _ & shape).
  assert (width : window_width child = last - first) by (rewrite shape; reflexivity).
  split.
  - unfold legal_window. rewrite same_owner.
    split; [exact child_valid|]. split; assumption.
  - split; [constructor; rewrite width; lia|].
    split; [exact width|]. split; [exact same_owner|]. now rewrite same_owner.
Qed.

Record split_facts current cut result : Prop := SplitFacts {
  left_legal : legal_root (split_left result);
  right_legal : legal_root (split_right result);
  left_size : root_size (split_left result) = cut;
  right_size : root_size (split_right result) = size current - cut;
  cut_in_range : cut <= size current;
  left_height : root_height (split_left result) <= height current;
  right_height : root_height (split_right result) <= height current;
  exact_reconstruction : root_bytes (split_left result) ++ root_bytes (split_right result) = denotation roots current;
  visits_potential : split_visits result <=
    3 * height current + 1 + root_height (split_left result) + root_height (split_right result);
  rebuilt_potential : split_rebuilt result <=
    2 * height current + 2 + 2 * (root_height (split_left result) + root_height (split_right result))
}.

Lemma attach_right_preserves_split_facts original child siblings cut divided glued :
  height original = S (height child) ->
  size original = size child + size siblings ->
  denotation roots original = denotation roots child ++ denotation roots siblings ->
  legal_root (Some siblings) -> split_facts child cut divided ->
  glue_contract (split_right divided) (height child)
    (root_size (split_right divided) + size siblings)
    (root_terminals (split_right divided) ++ terminals siblings) glued ->
  split_facts original cut (attach_right divided glued).
Proof.
  intros parent_height parent_size parent_bytes siblings_legal divided_facts glued_facts.
  destruct divided_facts as [left_valid right_valid left_count right_count cut_bounded left_rank right_rank reconstructed visited rebuilt].
  assert (legal_terms : Forall legal_window
    (root_terminals (split_right divided) ++ terminals siblings)).
  { apply Forall_app. split; [exact (proj2 right_valid)|exact (proj2 siblings_legal)]. }
  destruct (legal_root_from_terminal_equality glued_facts legal_terms) as [glued_valid glued_bytes].
  rewrite map_app, concat_app in glued_bytes.
  change (root_bytes (Some (glued_tree glued)) =
    root_bytes (split_right divided) ++ denotation roots siblings) in glued_bytes.
  destruct glued_facts as [balanced_glued [count [contents [rank [grows [glue_visit glue_build]]]]]].
  constructor; cbn [attach_right split_left split_right split_visits split_rebuilt root_size root_height].
  - exact left_valid.
  - exact glued_valid.
  - exact left_count.
  - lia.
  - lia.
  - lia.
  - lia.
  - rewrite glued_bytes, app_assoc, reconstructed. symmetry. exact parent_bytes.
  - lia.
  - lia.
Qed.

Lemma attach_left_preserves_split_facts original siblings child cut divided glued :
  height original = S (height child) ->
  size original = size siblings + size child ->
  denotation roots original = denotation roots siblings ++ denotation roots child ->
  legal_root (Some siblings) -> split_facts child cut divided ->
  glue_contract (split_left divided) (height child)
    (size siblings + root_size (split_left divided))
    (terminals siblings ++ root_terminals (split_left divided)) glued ->
  split_facts original (size siblings + cut) (attach_left glued divided).
Proof.
  intros parent_height parent_size parent_bytes siblings_legal divided_facts glued_facts.
  destruct divided_facts as [left_valid right_valid left_count right_count cut_bounded left_rank right_rank reconstructed visited rebuilt].
  assert (legal_terms : Forall legal_window
    (terminals siblings ++ root_terminals (split_left divided))).
  { apply Forall_app. split; [exact (proj2 siblings_legal)|exact (proj2 left_valid)]. }
  destruct (legal_root_from_terminal_equality glued_facts legal_terms) as [glued_valid glued_bytes].
  rewrite map_app, concat_app in glued_bytes.
  change (root_bytes (Some (glued_tree glued)) =
    denotation roots siblings ++ root_bytes (split_left divided)) in glued_bytes.
  destruct glued_facts as [balanced_glued [count [contents [rank [grows [glue_visit glue_build]]]]]].
  constructor; cbn [attach_left split_left split_right split_visits split_rebuilt root_size root_height].
  - exact glued_valid.
  - exact right_valid.
  - lia.
  - lia.
  - lia.
  - lia.
  - lia.
  - rewrite glued_bytes, <- app_assoc, reconstructed. symmetry. exact parent_bytes.
  - lia.
  - lia.
Qed.

Lemma attach_both_preserves_split_facts original first child last cut divided before after :
  height original = S (height child) ->
  size original = size first + size child + size last ->
  denotation roots original = denotation roots first ++ denotation roots child ++ denotation roots last ->
  legal_root (Some first) -> legal_root (Some last) -> split_facts child cut divided ->
  glue_contract (split_left divided) (height child)
    (size first + root_size (split_left divided))
    (terminals first ++ root_terminals (split_left divided)) before ->
  glue_contract (split_right divided) (height child)
    (root_size (split_right divided) + size last)
    (root_terminals (split_right divided) ++ terminals last) after ->
  split_facts original (size first + cut) (attach_both before divided after).
Proof.
  intros parent_height parent_size parent_bytes first_legal last_legal divided_facts before_facts after_facts.
  destruct divided_facts as [left_valid right_valid left_count right_count cut_bounded left_rank right_rank reconstructed visited rebuilt].
  assert (before_terms : Forall legal_window (terminals first ++ root_terminals (split_left divided))).
  { apply Forall_app. split; [exact (proj2 first_legal)|exact (proj2 left_valid)]. }
  assert (after_terms : Forall legal_window (root_terminals (split_right divided) ++ terminals last)).
  { apply Forall_app. split; [exact (proj2 right_valid)|exact (proj2 last_legal)]. }
  destruct (legal_root_from_terminal_equality before_facts before_terms) as [before_valid before_bytes].
  destruct (legal_root_from_terminal_equality after_facts after_terms) as [after_valid after_bytes].
  rewrite map_app, concat_app in before_bytes, after_bytes.
  change (root_bytes (Some (glued_tree before)) =
    denotation roots first ++ root_bytes (split_left divided)) in before_bytes.
  change (root_bytes (Some (glued_tree after)) =
    root_bytes (split_right divided) ++ denotation roots last) in after_bytes.
  destruct before_facts as [before_balanced [before_count [before_contents [before_rank [before_grows [before_visits before_build]]]]]].
  destruct after_facts as [after_balanced [after_count [after_contents [after_rank [after_grows [after_visits after_build]]]]]].
  constructor; cbn [attach_both split_left split_right split_visits split_rebuilt root_size root_height].
  - exact before_valid.
  - exact after_valid.
  - lia.
  - lia.
  - lia.
  - lia.
  - lia.
  - rewrite before_bytes, after_bytes, parent_bytes, <- reconstructed. now rewrite !app_assoc.
  - lia.
  - lia.
Qed.

Lemma zero_cut_has_exact_split_facts current :
  legal_root (Some current) -> split_facts current 0 (Split None (Some current) 1 0).
Proof.
  intro valid. constructor;
    cbn [split_left split_right split_visits split_rebuilt root_size root_height].
  - exact none_is_legal.
  - exact valid.
  - reflexivity.
  - lia.
  - lia.
  - lia.
  - lia.
  - reflexivity.
  - lia.
  - lia.
Qed.

Lemma full_cut_has_exact_split_facts current :
  legal_root (Some current) -> split_facts current (size current) (Split (Some current) None 1 0).
Proof.
  intro valid. constructor;
    cbn [split_left split_right split_visits split_rebuilt root_size root_height].
  - exact valid.
  - exact none_is_legal.
  - reflexivity.
  - lia.
  - lia.
  - lia.
  - lia.
  - change (denotation roots current ++ [] = denotation roots current). apply app_nil_r.
  - lia.
  - lia.
Qed.

Lemma total_split_from_strict_cuts current :
  balanced current -> Forall legal_window (terminals current) ->
  (forall cut, 0 < cut < size current ->
    exists divided, split_tree limit current cut = Some divided /\ split_facts current cut divided) ->
  forall cut, cut <= size current ->
    exists divided, split_tree limit current cut = Some divided /\ split_facts current cut divided.
Proof.
  intros balanced_current legal_current interior cut bounded.
  assert (valid : legal_root (Some current)) by (split; assumption).
  destruct (Nat.eq_dec cut 0) as [->|nonzero].
  - exists (Split None (Some current) 1 0). split.
    + apply zero_and_full_tree_cuts_retain_the_exact_original_root.
    + now apply zero_cut_has_exact_split_facts.
  - destruct (Nat.eq_dec cut (size current)) as [->|nonfull].
    + exists (Split (Some current) None 1 0). split.
      * apply zero_and_full_tree_cuts_retain_the_exact_original_root. lia.
      * now apply full_cut_has_exact_split_facts.
    + apply interior. lia.
Qed.

Lemma legal_window_admits_every_checked_in_range_subwindow view first last :
  legal_window view -> first <= last -> last <= window_width view ->
  exists child, checked_subwindow limit view first last = Some child.
Proof.
  intros [_ [fits _]] ordered bounded.
  assert (guard : subwindow_guard limit view first last = true).
  { apply subwindow_guard_exact. repeat split; lia. }
  unfold checked_subwindow. rewrite guard. eauto.
Qed.

Lemma an_interior_terminal_cut_has_exact_legal_split_facts view cut :
  legal_window view -> 0 < cut < window_width view ->
  exists divided,
    split_tree limit (Leaf view) cut = Some divided /\ split_facts (Leaf view) cut divided.
Proof.
  intros valid strict.
  destruct (@legal_window_admits_every_checked_in_range_subwindow view 0 cut valid ltac:(lia) ltac:(lia))
    as [before before_checked].
  destruct (@legal_window_admits_every_checked_in_range_subwindow view cut (window_width view)
    valid ltac:(lia) ltac:(lia)) as [after after_checked].
  destruct (@checked_nonempty_subwindow_preserves_legality_and_original_source
    view 0 cut before valid ltac:(lia) ltac:(lia) before_checked)
    as (before_legal & before_balanced & before_width & before_owner & before_bytes).
  destruct (@checked_nonempty_subwindow_preserves_legality_and_original_source
    view cut (window_width view) after valid ltac:(lia) ltac:(lia) after_checked)
    as (after_legal & after_balanced & after_width & after_owner & after_bytes).
  exists (Split (Some (Leaf before)) (Some (Leaf after)) 1 2).
  split.
  - cbn [split_tree size].
    assert (inside : cut <=? window_width view = true) by (apply Nat.leb_le; lia).
    assert (nonzero : cut =? 0 = false) by (apply Nat.eqb_neq; lia).
    assert (nonfull : cut =? window_width view = false) by (apply Nat.eqb_neq; lia).
    rewrite inside, nonzero, nonfull, before_checked, after_checked. reflexivity.
  - constructor;
      cbn [split_left split_right split_visits split_rebuilt root_size root_height size height].
    + split; [exact before_balanced|]. constructor; [exact before_legal|constructor].
    + split; [exact after_balanced|]. constructor; [exact after_legal|constructor].
    + lia.
    + exact after_width.
    + lia.
    + lia.
    + lia.
    + unfold root_bytes, denotation; cbn [root_terminals terminals map concat].
      rewrite !app_nil_r. unfold terminal_bytes.
      rewrite before_bytes, after_bytes. cbn [skipn]. rewrite Nat.sub_0_r.
      assert (after_all : firstn (window_width view - cut)
        (skipn cut (window_bytes (roots (window_owner view)) view)) =
        skipn cut (window_bytes (roots (window_owner view)) view)).
      { apply firstn_all2. rewrite length_skipn, valid_window_length by exact (proj1 valid). lia. }
      rewrite after_all. apply firstn_skipn.
    + lia.
    + lia.
Qed.

Lemma legal_node2 first last :
  legal_root (Some first) -> legal_root (Some last) -> height first = height last ->
  legal_root (Some (node2 first last)).
Proof.
  intros [first_balanced first_legal] [last_balanced last_legal] same.
  split; [constructor; assumption|].
  cbn [root_terminals node2 terminals]. apply Forall_app. tauto.
Qed.

Lemma node2_denotation first last :
  denotation roots (node2 first last) = denotation roots first ++ denotation roots last.
Proof. unfold denotation; cbn [node2 terminals]. now rewrite map_app, concat_app. Qed.

Lemma node3_denotation first middle last :
  denotation roots (node3 first middle last) =
    denotation roots first ++ denotation roots middle ++ denotation roots last.
Proof. unfold denotation; cbn [node3 terminals]. now rewrite !map_app, !concat_app. Qed.

Lemma split_node2_interior_equation first last cut :
  0 < cut < size (node2 first last) ->
  split_tree limit (node2 first last) cut =
    if cut <=? size first
    then option_map (fun divided => attach_right divided (gr1 (split_right divided) last))
      (split_tree limit first cut)
    else option_map (fun divided => attach_left (gl1 first (split_left divided)) divided)
      (split_tree limit last (cut - size first)).
Proof.
  intro strict. cbn [split_tree node2 size].
  assert (inside : cut <=? size first + size last = true) by (apply Nat.leb_le; cbn [node2 size] in strict; lia).
  assert (nonzero : cut =? 0 = false) by (apply Nat.eqb_neq; lia).
  assert (nonfull : cut =? size first + size last = false) by (apply Nat.eqb_neq; cbn [node2 size] in strict; lia).
  now rewrite inside, nonzero, nonfull.
Qed.

Lemma split_node3_interior_equation first middle last cut :
  0 < cut < size (node3 first middle last) ->
  split_tree limit (node3 first middle last) cut =
    if cut <=? size first
    then option_map (fun divided => attach_right divided (gr2 (split_right divided) middle last))
      (split_tree limit first cut)
    else if cut <=? size first + size middle
    then option_map (fun divided =>
      attach_both (gl1 first (split_left divided)) divided (gr1 (split_right divided) last))
      (split_tree limit middle (cut - size first))
    else option_map (fun divided => attach_left (gl2 first middle (split_left divided)) divided)
      (split_tree limit last (cut - (size first + size middle))).
Proof.
  intro strict. cbn [split_tree node3 size].
  assert (inside : cut <=? size first + size middle + size last = true)
    by (apply Nat.leb_le; cbn [node3 size] in strict; lia).
  assert (nonzero : cut =? 0 = false) by (apply Nat.eqb_neq; lia).
  assert (nonfull : cut =? size first + size middle + size last = false)
    by (apply Nat.eqb_neq; cbn [node3 size] in strict; lia).
  now rewrite inside, nonzero, nonfull.
Qed.

Theorem concrete_checked_split_is_total_preserves_sources_and_telescopes_both_output_heights current :
  balanced current -> forall cut,
  Forall legal_window (terminals current) -> cut <= size current ->
  exists divided, split_tree limit current cut = Some divided /\ split_facts current cut divided.
Proof.
  intro current_balanced.
  induction current_balanced as [view positive|
    first last first_balanced first_IH last_balanced last_IH same|
    first middle last first_balanced first_IH middle_balanced middle_IH last_balanced last_IH same_middle same];
    intros cut windows bounded.
  - eapply total_split_from_strict_cuts; [constructor; exact positive|exact windows| |exact bounded].
    intros inside strict. apply an_interior_terminal_cut_has_exact_legal_split_facts; [|exact strict].
    inversion windows; assumption.
  - eapply total_split_from_strict_cuts; [constructor; assumption|exact windows| |exact bounded].
    intros inside strict.
    cbn [node2 terminals] in windows. apply Forall_app in windows.
    destruct windows as [first_windows last_windows].
    destruct (inside <=? size first) eqn:direction.
    + pose proof direction as in_first. apply Nat.leb_le in in_first.
      destruct (first_IH inside first_windows in_first) as [divided [accepted facts]].
      exists (attach_right divided (gr1 (split_right divided) last)). split.
      * rewrite split_node2_interior_equation by exact strict. now rewrite direction, accepted.
      * eapply attach_right_preserves_split_facts.
        -- reflexivity.
        -- reflexivity.
        -- apply node2_denotation.
        -- split; assumption.
        -- exact facts.
        -- rewrite same. apply one_right_sibling_glue_preserves_order_balance_caches_and_height_potential.
           ++ exact (proj1 (right_legal facts)).
           ++ exact last_balanced.
           ++ pose proof (right_height facts). lia.
    + pose proof direction as after_first. apply Nat.leb_gt in after_first.
      assert (in_last : inside - size first <= size last) by (cbn [node2 size] in strict; lia).
      destruct (last_IH (inside - size first) last_windows in_last) as [divided [accepted facts]].
      exists (attach_left (gl1 first (split_left divided)) divided). split.
      * rewrite split_node2_interior_equation by exact strict. now rewrite direction, accepted.
      * replace inside with (size first + (inside - size first)) by lia.
        eapply attach_left_preserves_split_facts.
        -- cbn [node2 height]. now rewrite same.
        -- reflexivity.
        -- apply node2_denotation.
        -- split; assumption.
        -- exact facts.
        -- rewrite <- same. apply one_left_sibling_glue_preserves_order_balance_caches_and_height_potential.
           ++ exact (proj1 (left_legal facts)).
           ++ exact first_balanced.
           ++ pose proof (left_height facts). lia.
  - eapply total_split_from_strict_cuts; [constructor; assumption|exact windows| |exact bounded].
    intros inside strict.
    cbn [node3 terminals] in windows. apply Forall_app in windows.
    destruct windows as [first_windows rest_windows]. apply Forall_app in rest_windows.
    destruct rest_windows as [middle_windows last_windows].
    destruct (inside <=? size first) eqn:first_direction.
    + pose proof first_direction as in_first. apply Nat.leb_le in in_first.
      destruct (first_IH inside first_windows in_first) as [divided [accepted facts]].
      exists (attach_right divided (gr2 (split_right divided) middle last)). split.
      * rewrite split_node3_interior_equation by exact strict. now rewrite first_direction, accepted.
      * pose proof (@two_right_siblings_glue_preserves_order_without_finishing_the_intermediate_join
          (split_right divided) middle last (proj1 (right_legal facts)) middle_balanced last_balanced ltac:(lia)
          ltac:(pose proof (right_height facts); lia)) as [glued _].
        eapply attach_right_preserves_split_facts with (siblings := node2 middle last).
        -- reflexivity.
        -- cbn [node3 node2 size]. lia.
        -- rewrite node3_denotation, node2_denotation. reflexivity.
        -- apply legal_node2; [split; assumption|split; assumption|lia].
        -- exact facts.
        -- cbn [node2 size terminals].
           replace (root_size (split_right divided) + (size middle + size last)) with
             (root_size (split_right divided) + size middle + size last) by lia.
           now rewrite same_middle.
    + pose proof first_direction as after_first. apply Nat.leb_gt in after_first.
      destruct (inside <=? size first + size middle) eqn:middle_direction.
      * pose proof middle_direction as in_middle. apply Nat.leb_le in in_middle.
        assert (middle_cut : inside - size first <= size middle) by lia.
        destruct (middle_IH (inside - size first) middle_windows middle_cut) as [divided [accepted facts]].
        exists (attach_both (gl1 first (split_left divided)) divided (gr1 (split_right divided) last)). split.
        -- rewrite split_node3_interior_equation by exact strict.
           now rewrite first_direction, middle_direction, accepted.
        -- replace inside with (size first + (inside - size first)) by lia.
           eapply attach_both_preserves_split_facts.
           ++ cbn [node3 height]. now rewrite same_middle.
           ++ reflexivity.
           ++ apply node3_denotation.
           ++ split; assumption.
           ++ split; assumption.
           ++ exact facts.
           ++ rewrite <- same_middle.
              apply one_left_sibling_glue_preserves_order_balance_caches_and_height_potential.
              ** exact (proj1 (left_legal facts)).
              ** exact first_balanced.
              ** pose proof (left_height facts). lia.
           ++ replace (height middle) with (height last) by lia.
              apply one_right_sibling_glue_preserves_order_balance_caches_and_height_potential.
              ** exact (proj1 (right_legal facts)).
              ** exact last_balanced.
              ** pose proof (right_height facts). lia.
      * pose proof middle_direction as after_middle. apply Nat.leb_gt in after_middle.
        assert (in_last : inside - (size first + size middle) <= size last)
          by (cbn [node3 size] in strict; lia).
        destruct (last_IH (inside - (size first + size middle)) last_windows in_last)
          as [divided [accepted facts]].
        exists (attach_left (gl2 first middle (split_left divided)) divided). split.
        -- rewrite split_node3_interior_equation by exact strict.
           now rewrite first_direction, middle_direction, accepted.
        -- replace inside with ((size first + size middle) + (inside - (size first + size middle))) by lia.
           change (split_facts (node3 first middle last)
             (size (node2 first middle) + (inside - (size first + size middle)))
             (attach_left (gl2 first middle (split_left divided)) divided)).
           pose proof (@two_left_siblings_glue_preserves_order_without_finishing_the_intermediate_join
             first middle (split_left divided) (proj1 (left_legal facts)) first_balanced middle_balanced same_middle
             ltac:(pose proof (left_height facts); lia)) as [glued _].
           eapply attach_left_preserves_split_facts with (siblings := node2 first middle).
           ++ cbn [node3 height]. now rewrite same.
           ++ reflexivity.
           ++ rewrite node3_denotation, node2_denotation. apply app_assoc.
           ++ apply legal_node2; [split; assumption|split; assumption|exact same_middle].
           ++ exact facts.
           ++ cbn [node2 size terminals].
              replace (height last) with (height middle) by lia.
              rewrite <- app_assoc. exact glued.
Qed.

Theorem split_reconstruction_is_the_exact_located_firstn_and_skipn current cut divided :
  split_facts current cut divided ->
  root_bytes (split_left divided) = firstn cut (denotation roots current) /\
  root_bytes (split_right divided) = skipn cut (denotation roots current).
Proof.
  intro facts.
  pose proof (legal_root_length (left_legal facts)) as count.
  rewrite (left_size facts) in count.
  rewrite <- (exact_reconstruction facts), <- count.
  split.
  - rewrite firstn_app, firstn_all, Nat.sub_diag. cbn. now rewrite app_nil_r.
  - rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity.
Qed.

Theorem both_output_height_potentials_give_the_linear_split_work_bound current cut divided :
  split_facts current cut divided ->
  split_visits divided <= 5 * height current + 1 /\
  split_rebuilt divided <= 6 * height current + 2.
Proof. intros [left_valid right_valid left_count right_count cut_bounded left_rank right_rank bytes visits rebuilt]. lia. Qed.

Definition root_metadata limit current :=
  match current with None => True | Some value => native_metadata limit value end.

Theorem checked_split_outputs_preserve_all_native_lengths_heights_and_terminal_bounds current cut divided :
  size current <= limit -> split_facts current cut divided ->
  root_metadata limit (split_left divided) /\ root_metadata limit (split_right divided) /\
  Forall legal_window (root_terminals (split_left divided) ++ root_terminals (split_right divided)).
Proof.
  intros fits facts.
  destruct facts as [left_valid right_valid left_count right_count cut_bounded left_rank right_rank bytes visits rebuilt].
  split.
  - destruct (split_left divided); cbn [root_metadata]; [|exact I].
    apply balanced_sizes_bound_every_cached_sum_and_height; [exact (proj1 left_valid)|].
    cbn [root_size] in left_count. lia.
  - split.
    + destruct (split_right divided); cbn [root_metadata]; [|exact I].
      apply balanced_sizes_bound_every_cached_sum_and_height; [exact (proj1 right_valid)|].
      cbn [root_size] in right_count. lia.
    + apply Forall_app. split; [exact (proj2 left_valid)|exact (proj2 right_valid)].
Qed.

Theorem checked_optional_split_is_total_exact_and_linearly_bounded current cut :
  legal_root current -> root_size current <= limit -> cut <= root_size current ->
  exists divided, split_root limit current cut = Some divided /\
    root_balanced (split_left divided) /\ root_balanced (split_right divided) /\
    root_size (split_left divided) = cut /\ root_size (split_right divided) = root_size current - cut /\
    root_bytes (split_left divided) = firstn cut (root_bytes current) /\
    root_bytes (split_right divided) = skipn cut (root_bytes current) /\
    root_height (split_left divided) <= root_height current /\
    root_height (split_right divided) <= root_height current /\
    root_metadata limit (split_left divided) /\ root_metadata limit (split_right divided) /\
    Forall legal_window (root_terminals (split_left divided) ++ root_terminals (split_right divided)) /\
    split_visits divided <= 5 * root_height current + 1 /\
    split_rebuilt divided <= 6 * root_height current + 2.
Proof.
  destruct current as [value|].
  - intros [balanced_value valid] fits bounded.
    destruct (concrete_checked_split_is_total_preserves_sources_and_telescopes_both_output_heights
      balanced_value valid bounded) as [divided [accepted facts]].
    destruct (split_reconstruction_is_the_exact_located_firstn_and_skipn facts) as [before after].
    destruct (both_output_height_potentials_give_the_linear_split_work_bound facts) as [visits rebuilt].
    destruct (checked_split_outputs_preserve_all_native_lengths_heights_and_terminal_bounds fits facts)
      as [left_metadata [right_metadata sources]].
    exists divided. split; [exact accepted|].
    split; [exact (proj1 (left_legal facts))|]. split; [exact (proj1 (right_legal facts))|].
    split; [exact (left_size facts)|]. split; [exact (right_size facts)|].
    split; [exact before|]. split; [exact after|].
    split; [exact (left_height facts)|]. split; [exact (right_height facts)|].
    split; [exact left_metadata|]. split; [exact right_metadata|].
    split; [exact sources|]. split; assumption.
  - intros _ _ bounded. cbn [root_size] in bounded. assert (cut = 0) by lia. subst cut.
    exists (Split None None 1 0).
    cbn [split_root split_left split_right split_visits split_rebuilt root_balanced root_size root_height root_metadata root_terminals root_bytes].
    repeat split; try constructor; lia.
Qed.

End Sources.

Theorem valid_source_windows_and_terminal_bounds_identify_only_the_original_backings
    roots limit current :
  balanced current -> valid_sources roots current -> terminal_bounds limit current ->
  legal_root roots limit (map window_owner (terminals current)) (Some current).
Proof.
  intros balanced_current valid fits. split; [exact balanced_current|].
  unfold valid_sources, terminal_bounds in *.
  apply Forall_forall. intros view present. unfold legal_window.
  rewrite Forall_forall in valid, fits.
  split; [now apply valid|]. split; [now apply fits|].
  apply in_map. exact present.
Qed.

Theorem valid_cached_metadata_does_not_substitute_for_terminal_end_bounds :
  let view := Window 7 11 2 None in
  let backing := Region 7 (repeat (LocatedByte 203 99 4) 13) 99 17 in
  balanced (Leaf view) /\ window_valid backing view /\ native_metadata 10 (Leaf view) /\
  split_tree 10 (Leaf view) 1 = None.
Proof.
  cbn. split.
  - apply BalancedLeaf. cbn. lia.
  - vm_compute. repeat split; lia || reflexivity.
Qed.

Definition retain_output (stored : list tree) output :=
  match output with None => stored | Some value => fst (publish_root stored value) end.

Definition publish_split_outputs stored divided :=
  retain_output (retain_output stored (split_left divided)) (split_right divided).

Lemma retain_output_preserves_historical_root stored output slot historical :
  nth_error stored slot = Some historical ->
  nth_error (retain_output stored output) slot = Some historical.
Proof.
  intro found. destruct output; [apply publishing_a_composed_root_preserves_every_exact_historical_root|]; exact found.
Qed.

Theorem publishing_both_optional_split_outputs_preserves_every_exact_historical_root
    stored divided slot historical :
  nth_error stored slot = Some historical ->
  nth_error (publish_split_outputs stored divided) slot = Some historical.
Proof.
  intro found. apply retain_output_preserves_historical_root.
  now apply retain_output_preserves_historical_root.
Qed.

Print Assumptions one_right_sibling_glue_preserves_order_balance_caches_and_height_potential.
Print Assumptions one_left_sibling_glue_preserves_order_balance_caches_and_height_potential.
Print Assumptions two_right_siblings_glue_preserves_order_without_finishing_the_intermediate_join.
Print Assumptions two_left_siblings_glue_preserves_order_without_finishing_the_intermediate_join.
Print Assumptions out_of_range_split_cuts_are_refused_before_descent.
Print Assumptions zero_and_full_tree_cuts_retain_the_exact_original_root.
Print Assumptions checked_terminal_construction_and_concatenation_preserve_representable_source_windows.
Print Assumptions checked_nonempty_subwindow_preserves_legality_and_original_source.
Print Assumptions split_node2_interior_equation.
Print Assumptions split_node3_interior_equation.
Print Assumptions concrete_checked_split_is_total_preserves_sources_and_telescopes_both_output_heights.
Print Assumptions split_reconstruction_is_the_exact_located_firstn_and_skipn.
Print Assumptions both_output_height_potentials_give_the_linear_split_work_bound.
Print Assumptions checked_split_outputs_preserve_all_native_lengths_heights_and_terminal_bounds.
Print Assumptions checked_optional_split_is_total_exact_and_linearly_bounded.
Print Assumptions valid_source_windows_and_terminal_bounds_identify_only_the_original_backings.
Print Assumptions valid_cached_metadata_does_not_substitute_for_terminal_end_bounds.
Print Assumptions publishing_both_optional_split_outputs_preserves_every_exact_historical_root.

End CheckedBalancedSourceSplit.

Module BalancedSourceSplitDriver.

Import CanonicalKeyProtocol IndexedWindowTransport BalancedSourceComposition CheckedBalancedSourceSplit.

Definition driver_gr1 partial sibling :=
  match partial with
  | None => Glued sibling 0 0
  | Some value => freeze_join (BalancedSourceConcatDriver.driver value sibling)
  end.

Definition driver_gl1 sibling partial :=
  match partial with
  | None => Glued sibling 0 0
  | Some value => freeze_join (BalancedSourceConcatDriver.driver sibling value)
  end.

Definition driver_gr2 partial first last :=
  match partial with
  | None => Glued (node2 first last) 0 1
  | Some value => freeze_join (absorb_left2 (BalancedSourceConcatDriver.driver value first) last)
  end.

Definition driver_gl2 first last partial :=
  match partial with
  | None => Glued (node2 first last) 0 1
  | Some value => freeze_join (absorb_right2 first (BalancedSourceConcatDriver.driver last value))
  end.

Theorem split_glue_uses_the_verified_concat_driver_without_intermediate_finalization
    partial first last :
  driver_gr1 partial last = gr1 partial last /\
  driver_gl1 first partial = gl1 first partial /\
  driver_gr2 partial first last = gr2 partial first last /\
  driver_gl2 first last partial = gl2 first last partial.
Proof.
  destruct partial; cbn [driver_gr1 driver_gl1 driver_gr2 driver_gl2 gr1 gl1 gr2 gl2];
    repeat rewrite BalancedSourceConcatDriver.explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations;
    repeat split.
Qed.

Inductive continuation :=
| FromFirst2 (last : tree)
| FromLast2 (first : tree)
| FromFirst3 (middle last : tree)
| FromMiddle3 (first last : tree)
| FromLast3 (first middle : tree).

Definition rebuild saved divided :=
  match saved with
  | FromFirst2 last => attach_right divided (driver_gr1 (split_right divided) last)
  | FromLast2 first => attach_left (driver_gl1 first (split_left divided)) divided
  | FromFirst3 middle last => attach_right divided (driver_gr2 (split_right divided) middle last)
  | FromMiddle3 first last =>
      attach_both (driver_gl1 first (split_left divided)) divided (driver_gr1 (split_right divided) last)
  | FromLast3 first middle => attach_left (driver_gl2 first middle (split_left divided)) divided
  end.

Definition recursive_rebuild saved divided :=
  match saved with
  | FromFirst2 last => attach_right divided (gr1 (split_right divided) last)
  | FromLast2 first => attach_left (gl1 first (split_left divided)) divided
  | FromFirst3 middle last => attach_right divided (gr2 (split_right divided) middle last)
  | FromMiddle3 first last => attach_both (gl1 first (split_left divided)) divided (gr1 (split_right divided) last)
  | FromLast3 first middle => attach_left (gl2 first middle (split_left divided)) divided
  end.

Theorem every_split_frame_rebuilds_the_exact_recursive_outputs_and_counters saved divided :
  rebuild saved divided = recursive_rebuild saved divided.
Proof.
  destruct saved; unfold rebuild, recursive_rebuild, driver_gr1, driver_gl1, driver_gr2, driver_gl2;
    destruct (split_left divided); destruct (split_right divided);
    cbn [gr1 gl1 gr2 gl2];
    repeat rewrite BalancedSourceConcatDriver.explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations;
    reflexivity.
Qed.

Fixpoint resume frames result :=
  match frames with
  | [] => result
  | saved :: rest => resume rest (option_map (rebuild saved) result)
  end.

Theorem failed_split_results_propagate_without_any_glue_or_publication frames :
  resume frames None = None.
Proof. induction frames; cbn [resume]; assumption || reflexivity. Qed.

Inductive control :=
| Descending (current : root) (cut : nat)
| Returning (result : option split_result).

Record machine := Machine {
  active : control;
  stack : list continuation
}.

Definition descend limit current cut frames :=
  match current with
  | None => Machine (Returning (if cut =? 0 then Some (Split None None 1 0) else None)) frames
  | Some value =>
      if cut <=? size value then
        if cut =? 0 then Machine (Returning (Some (Split None (Some value) 1 0))) frames
        else if cut =? size value then Machine (Returning (Some (Split (Some value) None 1 0))) frames
        else match value with
        | Leaf view => Machine (Returning
            (match checked_subwindow limit view 0 cut,
                   checked_subwindow limit view cut (window_width view) with
             | Some before, Some after => Some (Split (Some (Leaf before)) (Some (Leaf after)) 1 2)
             | _, _ => None
             end)) frames
        | Join2 _ _ first last =>
            if cut <=? size first
            then Machine (Descending (Some first) cut) (FromFirst2 last :: frames)
            else Machine (Descending (Some last) (cut - size first)) (FromLast2 first :: frames)
        | Join3 _ _ first middle last =>
            if cut <=? size first
            then Machine (Descending (Some first) cut) (FromFirst3 middle last :: frames)
            else if cut <=? size first + size middle
            then Machine (Descending (Some middle) (cut - size first)) (FromMiddle3 first last :: frames)
            else Machine (Descending (Some last) (cut - (size first + size middle))) (FromLast3 first middle :: frames)
        end
      else Machine (Returning None) frames
  end.

Definition step limit current :=
  match active current with
  | Descending value cut => Some (descend limit value cut (stack current))
  | Returning result => match stack current with
      | [] => None
      | saved :: rest => Some (Machine (Returning (option_map (rebuild saved) result)) rest)
      end
  end.

Definition meaning limit current :=
  resume (stack current)
    (match active current with
     | Descending value cut => split_root limit value cut
     | Returning result => result
     end).

Lemma resume_recursive_frame saved frames result :
  resume (saved :: frames) result = resume frames (option_map (recursive_rebuild saved) result).
Proof.
  cbn [resume]. destruct result; [|reflexivity].
  cbn [option_map]. now rewrite every_split_frame_rebuilds_the_exact_recursive_outputs_and_counters.
Qed.

Lemma descent_preserves_meaning limit current cut frames :
  meaning limit (descend limit current cut frames) = resume frames (split_root limit current cut).
Proof.
  destruct current as [value|]; [|reflexivity].
  destruct value as [view|count rank first last|count rank first middle last];
    cbn [descend split_root split_tree];
    repeat match goal with |- context [if ?guard then _ else _] => destruct guard end;
    try reflexivity.
  all: lazymatch goal with
  | |- meaning _ (Machine (Descending (Some ?child) ?offset) (?saved :: ?rest)) = ?rhs =>
      change (resume (saved :: rest) (split_tree limit child offset) = rhs)
  end.
  all: rewrite resume_recursive_frame; reflexivity.
Qed.

Theorem each_explicit_split_step_preserves_the_exact_recursive_interpretation limit current next :
  step limit current = Some next -> meaning limit next = meaning limit current.
Proof.
  destruct current as [[value cut|result] frames]; cbn [step active stack].
  - intro same. inversion same; subst next. apply descent_preserves_meaning.
  - destruct frames; [discriminate|]. intro same. inversion same; subst next. reflexivity.
Qed.

Inductive execution limit : machine -> nat -> machine -> Prop :=
| Executed current : execution limit current 0 current
| ExecutedStep current next final count :
    step limit current = Some next -> execution limit next count final ->
    execution limit current (S count) final.

Lemma execution_append limit first middle last first_count last_count :
  execution limit first first_count middle -> execution limit middle last_count last ->
  execution limit first (first_count + last_count) last.
Proof. intros prefix suffix. induction prefix; cbn; [exact suffix|]. econstructor; eauto. Qed.

Lemma execution_preserves_meaning limit first count last :
  execution limit first count last -> meaning limit last = meaning limit first.
Proof.
  intro ran. induction ran; [reflexivity|]. rewrite IHran.
  now apply each_explicit_split_step_preserves_the_exact_recursive_interpretation.
Qed.

Lemma returning_executes_exactly_one_pop_per_saved_frame limit frames result :
  execution limit (Machine (Returning result) frames) (length frames)
    (Machine (Returning (resume frames result)) []).
Proof.
  revert result. induction frames as [|saved rest IH]; intro result; cbn [length resume].
  - constructor.
  - econstructor; [reflexivity|apply IH].
Qed.

Lemma one_descent_step_precedes_any_successor_execution limit current cut frames :
  (exists count result, execution limit (descend limit current cut frames) count
    (Machine (Returning result) [])) ->
  exists count result, execution limit (Machine (Descending current cut) frames) count
    (Machine (Returning result) []).
Proof.
  intros (count & result & ran). exists (S count), result.
  econstructor; [reflexivity|exact ran].
Qed.

Lemma tree_descent_terminates_for_every_finite_tree limit current :
  forall cut frames, exists count result,
  execution limit (Machine (Descending (Some current) cut) frames) count
    (Machine (Returning result) []).
Proof.
  induction current as [view|count rank first first_IH last last_IH|
    count rank first first_IH middle middle_IH last last_IH]; intros cut frames;
    apply one_descent_step_precedes_any_successor_execution; cbn [descend];
    repeat match goal with |- context [if ?guard then _ else _] => destruct guard end;
    try solve [do 2 eexists; apply returning_executes_exactly_one_pop_per_saved_frame];
    first [apply first_IH|apply middle_IH|apply last_IH].
Qed.

Definition start current cut := Machine (Descending current cut) [].

Theorem explicit_split_loop_terminates_without_fuel_or_a_depth_cap limit current cut :
  exists count, execution limit (start current cut) count
    (Machine (Returning (split_root limit current cut)) []) /\
  step limit (Machine (Returning (split_root limit current cut)) []) = None.
Proof.
  assert (terminates : exists count result,
    execution limit (start current cut) count (Machine (Returning result) [])).
  { destruct current as [value|].
    - apply tree_descent_terminates_for_every_finite_tree.
    - apply one_descent_step_precedes_any_successor_execution.
      cbn [descend]. do 2 eexists. apply returning_executes_exactly_one_pop_per_saved_frame. }
  destruct terminates as (count & result & ran).
  pose proof (execution_preserves_meaning ran) as same.
  change (result = split_root limit current cut) in same. subst result.
  exists count. split; [exact ran|reflexivity].
Qed.

Theorem every_terminal_split_execution_has_the_exact_recursive_result_and_counters
    limit current cut count result :
  execution limit (start current cut) count (Machine (Returning result) []) ->
  result = split_root limit current cut.
Proof. intro ran. exact (execution_preserves_meaning ran). Qed.

Definition frontier_bound bound current :=
  match active current with
  | Descending value _ => root_balanced value /\ length (stack current) + root_height value <= bound
  | Returning _ => length (stack current) <= bound
  end.

Lemma descent_preserves_frontier_bound limit current cut frames bound :
  root_balanced current -> length frames + root_height current <= bound ->
  frontier_bound bound (descend limit current cut frames).
Proof.
  destruct current as [value|].
  - cbn [root_balanced root_height]. intros valid bounded.
    destruct value as [view|count rank first last|count rank first middle last].
    + cbn [descend].
      repeat match goal with |- context [if ?guard then _ else _] => destruct guard end;
        cbn [frontier_bound active stack]; cbn [height] in bounded; lia.
    + destruct (balanced_shape2 valid) as (first_valid & last_valid & same & count_same & rank_same).
      cbn [height] in bounded. subst rank. cbn [descend].
      repeat match goal with |- context [if ?guard then _ else _] => destruct guard end;
        cbn [frontier_bound active stack root_balanced root_height length];
        repeat split; assumption || lia.
    + destruct (balanced_shape3 valid) as
        (first_valid & middle_valid & last_valid & same_middle & same & count_same & rank_same).
      cbn [height] in bounded. subst rank. cbn [descend].
      repeat match goal with |- context [if ?guard then _ else _] => destruct guard end;
        cbn [frontier_bound active stack root_balanced root_height length];
        repeat split; assumption || lia.
  - cbn [root_balanced root_height descend]. intros _ bounded.
    cbn [frontier_bound active stack]. lia.
Qed.

Lemma one_step_preserves_frontier_bound limit bound current next :
  frontier_bound bound current -> step limit current = Some next -> frontier_bound bound next.
Proof.
  destruct current as [[value cut|result] frames]; cbn [frontier_bound step active stack].
  - intros [valid bounded] same. inversion same; subst next.
    now apply descent_preserves_frontier_bound.
  - intros bounded same. destruct frames; [discriminate|].
    inversion same; subst next. cbn [frontier_bound active stack]. cbn [length] in bounded. lia.
Qed.

Lemma execution_preserves_frontier_bound limit bound first count last :
  execution limit first count last -> frontier_bound bound first -> frontier_bound bound last.
Proof.
  intro ran. induction ran; intro bounded; [exact bounded|].
  apply IHran. eapply one_step_preserves_frontier_bound; eassumption.
Qed.

Theorem every_reachable_split_state_retains_at_most_the_input_height_frames
    limit current cut count stopped :
  root_balanced current -> execution limit (start current cut) count stopped ->
  length (stack stopped) <= root_height current.
Proof.
  intros valid ran.
  assert (bounded : frontier_bound (root_height current) stopped).
  { eapply execution_preserves_frontier_bound; [exact ran|].
    cbn [frontier_bound start active stack length]. split; [exact valid|lia]. }
  destruct stopped as [[value offset|result] frames]; cbn [frontier_bound active stack] in *; lia.
Qed.

Theorem out_of_range_machine_cuts_return_failure_without_pushing_a_frame
    limit current cut frames :
  size current < cut ->
  step limit (Machine (Descending (Some current) cut) frames) =
    Some (Machine (Returning None) frames).
Proof.
  intro outside. cbn [step active stack descend].
  assert (cut <=? size current = false) as refused by (apply Nat.leb_gt; exact outside).
  now rewrite refused.
Qed.

Theorem empty_optional_data_roots_preserve_the_exact_zero_cut_guard limit cut frames :
  step limit (Machine (Descending None cut) frames) =
    Some (Machine (Returning (if cut =? 0 then Some (Split None None 1 0) else None)) frames).
Proof. reflexivity. Qed.

Theorem zero_and_full_machine_cuts_keep_the_exact_original_data_root limit current frames :
  step limit (Machine (Descending (Some current) 0) frames) =
    Some (Machine (Returning (Some (Split None (Some current) 1 0))) frames) /\
  (0 < size current ->
   step limit (Machine (Descending (Some current) (size current)) frames) =
    Some (Machine (Returning (Some (Split (Some current) None 1 0))) frames)).
Proof.
  split; [reflexivity|]. intro positive. cbn [step active stack descend].
  assert (size current =? 0 = false) as nonzero by (apply Nat.eqb_neq; lia).
  now rewrite Nat.leb_refl, nonzero, Nat.eqb_refl.
Qed.

Theorem interior_leaf_machine_steps_keep_both_exact_checked_subwindow_results
    limit view cut frames :
  0 < cut < window_width view ->
  step limit (Machine (Descending (Some (Leaf view)) cut) frames) =
    Some (Machine (Returning
      (match checked_subwindow limit view 0 cut,
             checked_subwindow limit view cut (window_width view) with
       | Some before, Some after => Some (Split (Some (Leaf before)) (Some (Leaf after)) 1 2)
       | _, _ => None
       end)) frames).
Proof.
  intro strict. cbn [step active stack descend size].
  assert (cut <=? window_width view = true) as within by (apply Nat.leb_le; lia).
  assert (cut =? 0 = false) as nonzero by (apply Nat.eqb_neq; lia).
  assert (cut =? window_width view = false) as interior by (apply Nat.eqb_neq; lia).
  now rewrite within, nonzero, interior.
Qed.

Theorem node2_machine_descent_selects_and_rebases_the_exact_child
    limit count rank first last cut frames :
  0 < cut < count ->
  step limit (Machine (Descending (Some (Join2 count rank first last)) cut) frames) =
    Some (if cut <=? size first
      then Machine (Descending (Some first) cut) (FromFirst2 last :: frames)
      else Machine (Descending (Some last) (cut - size first)) (FromLast2 first :: frames)).
Proof.
  intro strict. cbn [step active stack descend size].
  assert (cut <=? count = true) as within by (apply Nat.leb_le; lia).
  assert (cut =? 0 = false) as nonzero by (apply Nat.eqb_neq; lia).
  assert (cut =? count = false) as interior by (apply Nat.eqb_neq; lia).
  now rewrite within, nonzero, interior.
Qed.

Theorem node3_machine_descent_keeps_both_boundaries_and_full_last_child_rebase
    limit count rank first middle last cut frames :
  0 < cut < count ->
  step limit (Machine (Descending (Some (Join3 count rank first middle last)) cut) frames) =
    Some (if cut <=? size first
      then Machine (Descending (Some first) cut) (FromFirst3 middle last :: frames)
      else if cut <=? size first + size middle
      then Machine (Descending (Some middle) (cut - size first)) (FromMiddle3 first last :: frames)
      else Machine (Descending (Some last) (cut - (size first + size middle))) (FromLast3 first middle :: frames)).
Proof.
  intro strict. cbn [step active stack descend size].
  assert (cut <=? count = true) as within by (apply Nat.leb_le; lia).
  assert (cut =? 0 = false) as nonzero by (apply Nat.eqb_neq; lia).
  assert (cut =? count = false) as interior by (apply Nat.eqb_neq; lia).
  now rewrite within, nonzero, interior.
Qed.

Theorem successful_tree_executions_inherit_the_full_recursive_split_facts
    roots limit allowed current cut count divided :
  balanced current ->
  Forall (legal_window roots limit allowed) (terminals current) -> cut <= size current ->
  execution limit (start (Some current) cut) count (Machine (Returning (Some divided)) []) ->
  split_facts roots limit allowed current cut divided.
Proof.
  intros valid windows within ran.
  destruct (concrete_checked_split_is_total_preserves_sources_and_telescopes_both_output_heights
    valid windows within) as [expected [accepted facts]].
  pose proof (every_terminal_split_execution_has_the_exact_recursive_result_and_counters ran) as same.
  cbn [split_root] in same. rewrite accepted in same. now inversion same; subst expected.
Qed.

Theorem legal_optional_inputs_reach_the_exact_successful_recursive_result
    roots limit allowed current cut :
  legal_root roots limit allowed current -> root_size current <= limit -> cut <= root_size current ->
  exists count divided,
    execution limit (start current cut) count (Machine (Returning (Some divided)) []) /\
    root_balanced (split_left divided) /\ root_balanced (split_right divided) /\
    root_size (split_left divided) = cut /\ root_size (split_right divided) = root_size current - cut /\
    root_bytes roots (split_left divided) = firstn cut (root_bytes roots current) /\
    root_bytes roots (split_right divided) = skipn cut (root_bytes roots current) /\
    root_height (split_left divided) <= root_height current /\
    root_height (split_right divided) <= root_height current /\
    root_metadata limit (split_left divided) /\ root_metadata limit (split_right divided) /\
    Forall (legal_window roots limit allowed)
      (root_terminals (split_left divided) ++ root_terminals (split_right divided)) /\
    split_visits divided <= 5 * root_height current + 1 /\
    split_rebuilt divided <= 6 * root_height current + 2.
Proof.
  intros legal fits within.
  destruct (checked_optional_split_is_total_exact_and_linearly_bounded legal fits within)
    as [divided [accepted facts]].
  destruct (explicit_split_loop_terminates_without_fuel_or_a_depth_cap limit current cut) as [count [ran stopped]].
  rewrite accepted in ran. exists count, divided. split; assumption.
Qed.

Print Assumptions split_glue_uses_the_verified_concat_driver_without_intermediate_finalization.
Print Assumptions every_split_frame_rebuilds_the_exact_recursive_outputs_and_counters.
Print Assumptions failed_split_results_propagate_without_any_glue_or_publication.
Print Assumptions each_explicit_split_step_preserves_the_exact_recursive_interpretation.
Print Assumptions explicit_split_loop_terminates_without_fuel_or_a_depth_cap.
Print Assumptions every_terminal_split_execution_has_the_exact_recursive_result_and_counters.
Print Assumptions every_reachable_split_state_retains_at_most_the_input_height_frames.
Print Assumptions out_of_range_machine_cuts_return_failure_without_pushing_a_frame.
Print Assumptions empty_optional_data_roots_preserve_the_exact_zero_cut_guard.
Print Assumptions zero_and_full_machine_cuts_keep_the_exact_original_data_root.
Print Assumptions interior_leaf_machine_steps_keep_both_exact_checked_subwindow_results.
Print Assumptions node2_machine_descent_selects_and_rebases_the_exact_child.
Print Assumptions node3_machine_descent_keeps_both_boundaries_and_full_last_child_rebase.
Print Assumptions successful_tree_executions_inherit_the_full_recursive_split_facts.
Print Assumptions legal_optional_inputs_reach_the_exact_successful_recursive_result.

End BalancedSourceSplitDriver.

Module BalancedSplitRegionPositions.

Import CanonicalKeyProtocol IndexedWindowTransport BalancedSourceComposition
  CheckedBalancedSourceSplit.

Lemma every_in_range_region_position_exists original offset :
  offset <= length (region_bytes original) ->
  exists origin, root_position original offset = Some origin.
Proof.
  intro within. destruct (Nat.eq_dec offset (length (region_bytes original))) as [at_end|inside].
  - subst offset. exists (region_end original). apply root_position_at_end.
  - assert (present : nth_error (region_bytes original) offset <> None).
    { apply nth_error_Some. lia. }
    destruct (nth_error (region_bytes original) offset) as [byte|] eqn:found; [|contradiction].
    exists (byte_provenance byte). unfold root_position. now rewrite found.
Qed.

Theorem iterative_split_outputs_preserve_every_existing_anchored_region_position
    roots limit allowed current cut count divided original :
  legal_root roots limit allowed current ->
  root_size current <= limit -> cut <= root_size current ->
  region_bytes original = root_bytes roots current ->
  BalancedSourceSplitDriver.execution limit
    (BalancedSourceSplitDriver.start current cut) count
    (BalancedSourceSplitDriver.Machine (BalancedSourceSplitDriver.Returning (Some divided)) []) ->
  exists seam,
    root_position original cut = Some seam /\
    let left := Region (region_owner original) (root_bytes roots (split_left divided))
      (fst seam) (snd seam) in
    let right := Region (region_owner original) (root_bytes roots (split_right divided))
      (region_eof_source original) (region_eof_offset original) in
    (forall offset,
      window_position left (Window (region_owner original) 0 (root_size (split_left divided)) None) offset =
      window_position original
        (anchor_window (Window (region_owner original) 0 cut None) seam) offset) /\
    (forall offset,
      window_position right (Window (region_owner original) 0 (root_size (split_right divided)) None) offset =
      window_position original
        (anchor_window (Window (region_owner original) cut
          (length (region_bytes original) - cut) None) (region_end original)) offset).
Proof.
  intros legal fits within bytes ran.
  pose proof (legal_root_length legal) as length_is_size.
  assert (original_length : length (region_bytes original) = root_size current)
    by now rewrite bytes.
  destruct (@every_in_range_region_position_exists original cut ltac:(lia)) as [seam found].
  destruct (checked_optional_split_is_total_exact_and_linearly_bounded legal fits within)
    as [expected [accepted facts]].
  pose proof (BalancedSourceSplitDriver.every_terminal_split_execution_has_the_exact_recursive_result_and_counters ran)
    as same.
  rewrite accepted in same. inversion same; subst expected.
  destruct facts as (_ & _ & left_size & right_size & left_bytes & right_bytes & rest).
  assert (left_view_valid : window_valid original (Window (region_owner original) 0 cut None)).
  { unfold window_valid. cbn. split; [reflexivity|lia]. }
  assert (right_view_valid : window_valid original
      (Window (region_owner original) cut (length (region_bytes original) - cut) None)).
  { unfold window_valid. cbn. split; [reflexivity|lia]. }
  exists seam. split; [exact found|]. cbn zeta. split.
  - intro offset. rewrite left_size.
    pose proof (@rebased_frozen_and_inline_anchored_windows_have_identical_positions
      original (Window (region_owner original) 0 cut None) seam left_view_valid offset) as positions.
    cbn [window_owner window_width] in positions.
    rewrite <- bytes in left_bytes. rewrite left_bytes.
    unfold rebase_anchored_region, window_bytes in positions. cbn [window_owner window_start window_width skipn] in positions.
    symmetry. exact positions.
  - intro offset. rewrite right_size, <- original_length.
    pose proof (@rebased_frozen_and_inline_anchored_windows_have_identical_positions
      original (Window (region_owner original) cut (length (region_bytes original) - cut) None)
      (region_end original) right_view_valid offset) as positions.
    cbn [window_owner window_width] in positions.
    rewrite <- bytes in right_bytes. rewrite right_bytes.
    unfold rebase_anchored_region, window_bytes in positions.
    cbn [window_owner window_start window_width] in positions.
    rewrite firstn_all2 in positions by (rewrite length_skipn; lia).
    unfold region_end in positions. cbn [fst snd] in positions.
    symmetry. exact positions.
Qed.

Print Assumptions iterative_split_outputs_preserve_every_existing_anchored_region_position.

End BalancedSplitRegionPositions.

Module BalancedSourceNativeCapacity.

Import BalancedSourceComposition.

Lemma nonempty_terminal_occurrences_are_paid_for_by_cached_bytes current :
  balanced current -> length (terminals current) <= size current.
Proof.
  intro valid. induction valid; cbn [terminals size node2 node3].
  - cbn [length]. lia.
  - rewrite length_app. lia.
  - rewrite !length_app. lia.
Qed.

Theorem representable_balanced_sources_have_height_below_the_native_word_width
    current word_bits :
  balanced current -> size current < 2 ^ word_bits ->
  height current < word_bits.
Proof.
  intros valid fits.
  pose proof (balanced_height_has_a_concrete_exponential_terminal_lower_bound valid) as lower.
  pose proof (nonempty_terminal_occurrences_are_paid_for_by_cached_bytes valid) as bytes.
  apply (proj2 (Nat.pow_lt_mono_r_iff 2 (height current) word_bits ltac:(lia))).
  lia.
Qed.

Definition native_frontier_capacity word_bits := 1 + 2 * (word_bits - 1).

Theorem the_proved_local_frontier_fits_the_representation_derived_fixed_capacity
    current word_bits held :
  balanced current -> size current < 2 ^ word_bits ->
  held <= 1 + 2 * height current ->
  held <= native_frontier_capacity word_bits.
Proof.
  intros valid fits bounded.
  pose proof (@representable_balanced_sources_have_height_below_the_native_word_width
    current word_bits valid fits).
  unfold native_frontier_capacity. lia.
Qed.

Example native_frontier_capacity_is_127_slots_for_64_bit_lengths :
  native_frontier_capacity 64 = 127.
Proof. reflexivity. Qed.

Example native_frontier_capacity_is_63_slots_for_32_bit_lengths :
  native_frontier_capacity 32 = 63.
Proof. reflexivity. Qed.

Print Assumptions representable_balanced_sources_have_height_below_the_native_word_width.
Print Assumptions the_proved_local_frontier_fits_the_representation_derived_fixed_capacity.
Print Assumptions native_frontier_capacity_is_127_slots_for_64_bit_lengths.
Print Assumptions native_frontier_capacity_is_63_slots_for_32_bit_lengths.

End BalancedSourceNativeCapacity.

Module CheckedConstructorScheduling.

Section Scheduling.

Context {Parent Stats Request Child Error : Type}.

Variable prepare : Stats -> Request -> (Stats + Error)%type.
Variable install_wait : Parent -> Request -> Parent.
Variable construct : Request -> (Child + Error)%type.
Variable publish : Parent -> Stats -> Child -> (Stats + Error)%type.

Inductive event := PrepareAttempt | ConstructAttempt | PublishAttempt | ResumeAttempt.

Inductive observation :=
| Ready (parent : Parent) (stats : Stats) (child : Child)
| Failed (error : Error).

Record pending := Pending {
  pending_parent : Parent;
  pending_stats : Stats;
  pending_request : Request
}.

Definition eager parent stats request : observation * list event :=
  match prepare stats request with
  | inr error => (Failed error, [PrepareAttempt])
  | inl prepared =>
      match construct request with
      | inr error => (Failed error, [PrepareAttempt; ConstructAttempt])
      | inl child =>
          let waiting := install_wait parent request in
          match publish waiting prepared child with
          | inr error => (Failed error, [PrepareAttempt; ConstructAttempt; PublishAttempt])
          | inl published => (Ready waiting published child,
              [PrepareAttempt; ConstructAttempt; PublishAttempt])
          end
      end
  end.

Definition defer parent stats request : (pending + Error)%type :=
  match prepare stats request with
  | inr error => inr error
  | inl prepared => inl (Pending (install_wait parent request) prepared request)
  end.

Definition realize waiting : observation * list event :=
  match construct (pending_request waiting) with
  | inr error => (Failed error, [ConstructAttempt])
  | inl child =>
      match publish (pending_parent waiting) (pending_stats waiting) child with
      | inr error => (Failed error, [ConstructAttempt; PublishAttempt])
      | inl published => (Ready (pending_parent waiting) published child,
          [ConstructAttempt; PublishAttempt])
      end
  end.

Definition deferred parent stats request : observation * list event :=
  match defer parent stats request with
  | inr error => (Failed error, [PrepareAttempt])
  | inl waiting =>
      let '(result, events) := realize waiting in
      (result, PrepareAttempt :: events)
  end.

Definition resume (continuation : Parent -> Stats -> Child -> observation)
    (result : observation * list event) : observation * list event :=
  match result with
  | (Failed error, events) => (Failed error, events)
  | (Ready parent stats child, events) =>
      (continuation parent stats child, events ++ [ResumeAttempt])
  end.

Theorem deferred_construction_preserves_terminal_observations_and_attempt_order
    parent stats request :
  deferred parent stats request = eager parent stats request.
Proof.
  unfold deferred, defer, eager.
  destruct (prepare stats request) as [prepared|error]; [|reflexivity].
  unfold realize. cbn [pending_parent pending_stats pending_request].
  destruct (construct request) as [child|error]; [|reflexivity].
  destruct (publish (install_wait parent request) prepared child); reflexivity.
Qed.

Theorem checked_preparation_errors_precede_construction_and_publication
    parent stats request error :
  prepare stats request = inr error ->
  deferred parent stats request = (Failed error, [PrepareAttempt]).
Proof. intro failed. unfold deferred, defer. now rewrite failed. Qed.

Theorem constructor_errors_precede_publication_even_if_publication_would_fail
    parent stats request prepared error :
  prepare stats request = inl prepared -> construct request = inr error ->
  deferred parent stats request = (Failed error, [PrepareAttempt; ConstructAttempt]).
Proof.
  intros prepared_exact failed. unfold deferred, defer.
  rewrite prepared_exact. unfold realize. cbn [pending_request]. now rewrite failed.
Qed.

Theorem publication_errors_follow_exactly_one_successful_constructor
    parent stats request prepared child error :
  prepare stats request = inl prepared -> construct request = inl child ->
  publish (install_wait parent request) prepared child = inr error ->
  deferred parent stats request =
    (Failed error, [PrepareAttempt; ConstructAttempt; PublishAttempt]).
Proof.
  intros prepared_exact constructed failed. unfold deferred, defer.
  rewrite prepared_exact. unfold realize. cbn [pending_request pending_parent pending_stats].
  now rewrite constructed, failed.
Qed.

Theorem successful_deferred_dispatch_preserves_exact_parent_child_and_published_counts
    parent stats request prepared child published :
  prepare stats request = inl prepared -> construct request = inl child ->
  publish (install_wait parent request) prepared child = inl published ->
  deferred parent stats request =
    (Ready (install_wait parent request) published child,
      [PrepareAttempt; ConstructAttempt; PublishAttempt]).
Proof.
  intros prepared_exact constructed counted. unfold deferred, defer.
  rewrite prepared_exact. unfold realize. cbn [pending_request pending_parent pending_stats].
  now rewrite constructed, counted.
Qed.

Theorem rejected_construction_or_publication_never_resumes_a_parent_or_child
    parent stats request error events continuation :
  deferred parent stats request = (Failed error, events) ->
  resume continuation (deferred parent stats request) = (Failed error, events).
Proof. intro failed. rewrite failed. reflexivity. Qed.

End Scheduling.

Print Assumptions deferred_construction_preserves_terminal_observations_and_attempt_order.
Print Assumptions checked_preparation_errors_precede_construction_and_publication.
Print Assumptions constructor_errors_precede_publication_even_if_publication_would_fail.
Print Assumptions publication_errors_follow_exactly_one_successful_constructor.
Print Assumptions successful_deferred_dispatch_preserves_exact_parent_child_and_published_counts.
Print Assumptions rejected_construction_or_publication_never_resumes_a_parent_or_child.

End CheckedConstructorScheduling.

Module CachedSourcePrefixes.

Section LocatedContents.

Context {A : Type}.

Definition selected (bytes : list A) start width :=
  skipn start (firstn (start + width) bytes).

Definition prefix_extension (before after : list A) :=
  exists suffix, after = before ++ suffix.

Record cache := Cached {
  covered : nat;
  contents : list A
}.

Definition valid (bytes : list A) saved :=
  covered saved <= length bytes /\ contents saved = firstn (covered saved) bytes.

Definition grow (bytes : list A) saved stop :=
  Cached stop (contents saved ++ firstn (stop - covered saved) (skipn (covered saved) bytes)).

Inductive action := Refused | Reused | Extended.

Definition request limit (bytes : list A) saved stop : cache * action :=
  if (stop <=? limit) && (stop <=? length bytes) then
    if stop <=? covered saved then (saved, Reused)
    else (grow bytes saved stop, Extended)
  else (saved, Refused).

Lemma bounded_prefix_after_append (before suffix : list A) width :
  width <= length before -> firstn width (before ++ suffix) = firstn width before.
Proof.
  intro fits. rewrite firstn_app.
  replace (width - length before) with 0 by lia.
  cbn. now rewrite app_nil_r.
Qed.

Theorem bounded_handles_survive_prefix_monotone_descriptor_growth before after start width :
  prefix_extension before after -> start + width <= length before ->
  selected after start width = selected before start width.
Proof.
  intros [suffix ->] fits. unfold selected.
  now rewrite bounded_prefix_after_append by exact fits.
Qed.

Theorem cached_prefix_survives_descriptor_growth before after saved :
  prefix_extension before after -> valid before saved -> valid after saved.
Proof.
  intros [suffix ->] [fits exact_bytes]. unfold valid.
  split; [rewrite length_app; lia|].
  now rewrite bounded_prefix_after_append by exact fits.
Qed.

Theorem cached_delta_extension_has_exact_requested_contents bytes saved stop :
  valid bytes saved -> covered saved <= stop -> stop <= length bytes ->
  valid bytes (grow bytes saved stop).
Proof.
  intros [fits exact_bytes] forward within. unfold valid, grow; cbn.
  split; [exact within|]. rewrite exact_bytes.
  rewrite <- NormalizedSourceSpans.split_prefix.
  replace (covered saved + (stop - covered saved)) with stop by lia.
  reflexivity.
Qed.

Theorem cached_selection_matches_the_descriptor bytes saved start width :
  valid bytes saved -> start + width <= covered saved ->
  selected (contents saved) start width = selected bytes start width.
Proof.
  intros [_ ->] fits. unfold selected. rewrite firstn_firstn.
  now rewrite Nat.min_l by exact fits.
Qed.

Theorem extending_a_cache_preserves_all_older_bounded_handles bytes saved stop start width :
  valid bytes saved -> covered saved <= stop -> stop <= length bytes ->
  start + width <= covered saved ->
  selected (contents (grow bytes saved stop)) start width =
    selected (contents saved) start width.
Proof.
  intros cached forward within old_handle.
  rewrite (@cached_selection_matches_the_descriptor bytes (grow bytes saved stop) start width).
  - symmetry. now apply cached_selection_matches_the_descriptor.
  - now apply cached_delta_extension_has_exact_requested_contents.
  - cbn [grow covered]. lia.
Qed.

Theorem refused_requests_preserve_the_exact_cache limit bytes saved stop :
  limit < stop \/ length bytes < stop -> request limit bytes saved stop = (saved, Refused).
Proof.
  intro outside. unfold request.
  destruct ((stop <=? limit) && (stop <=? length bytes)) eqn:guard; [|reflexivity].
  rewrite andb_true_iff, !Nat.leb_le in guard. exfalso. lia.
Qed.

Theorem repeated_or_shorter_exports_reuse_the_existing_cache limit bytes saved stop :
  valid bytes saved -> stop <= covered saved -> stop <= limit ->
  request limit bytes saved stop = (saved, Reused).
Proof.
  intros [fits _] prior within. unfold request.
  assert (stop <=? limit = true) by (apply Nat.leb_le; exact within).
  assert (stop <=? length bytes = true) by (apply Nat.leb_le; lia).
  assert (stop <=? covered saved = true) by (apply Nat.leb_le; exact prior).
  now rewrite H, H0, H1.
Qed.

Theorem successful_requests_keep_cache_valid_and_cover_the_requested_prefix
    limit bytes saved stop result effect :
  valid bytes saved -> request limit bytes saved stop = (result, effect) ->
  effect <> Refused -> valid bytes result /\ stop <= covered result /\ stop <= limit.
Proof.
  intros cached accepted success. unfold request in accepted.
  destruct ((stop <=? limit) && (stop <=? length bytes)) eqn:guard.
  - rewrite andb_true_iff, !Nat.leb_le in guard.
    destruct (stop <=? covered saved) eqn:prior; inversion accepted; subst.
    + apply Nat.leb_le in prior. tauto.
    + apply Nat.leb_gt in prior. split.
      * apply cached_delta_extension_has_exact_requested_contents; try exact cached; lia.
      * cbn [grow covered]. lia.
  - inversion accepted; subst. contradiction.
Qed.

End LocatedContents.

Print Assumptions bounded_handles_survive_prefix_monotone_descriptor_growth.
Print Assumptions cached_prefix_survives_descriptor_growth.
Print Assumptions cached_delta_extension_has_exact_requested_contents.
Print Assumptions cached_selection_matches_the_descriptor.
Print Assumptions extending_a_cache_preserves_all_older_bounded_handles.
Print Assumptions refused_requests_preserve_the_exact_cache.
Print Assumptions repeated_or_shorter_exports_reuse_the_existing_cache.
Print Assumptions successful_requests_keep_cache_valid_and_cover_the_requested_prefix.

End CachedSourcePrefixes.

Module BulkCompressedUndo.

Import CompressedScalarUndo.

Lemma shorter_shorter current first second :
  shorter (shorter current first) second = shorter current second.
Proof. destruct current as [state [owner start width anchor]]. reflexivity. Qed.

Lemma same_shorter_left current width other :
  same_state (shorter current width) other = same_state current other.
Proof. reflexivity. Qed.

Lemma same_shorter_self current first second :
  same_state (shorter current first) (shorter current second) = true.
Proof. apply same_state_exact. reflexivity. Qed.

Fixpoint remember_steps final start width history :=
  match width with
  | 0 => history
  | S count => remember (shorter final (start + count))
      (remember_steps final start count history)
  end.

Definition remember_closed final start width history :=
  let previous := shorter final (start + width - 1) in
  match history with
  | [] => [Run previous start]
  | first :: rest =>
      if same_state final (latest first)
      then Run previous (first_depth first) :: rest
      else Run previous start :: history
  end.

Theorem positive_bulk_remember_is_exactly_iterated_scalar_remember
    final start width history :
  0 < width ->
  remember_closed final start width history = remember_steps final start width history.
Proof.
  destruct width as [|count]; [lia|]. intros _.
  induction count as [|count IH].
  - cbn [remember_steps]. unfold remember_closed.
    replace (start + 1 - 1) with start by lia.
    rewrite Nat.add_0_r. destruct history as [|first rest]; cbn [remember].
    + rewrite shorter_depth. reflexivity.
    + rewrite same_shorter_left, shorter_depth. reflexivity.
  - change (remember_closed final start (S (S count)) history =
      remember (shorter final (start + S count)) (remember_steps final start (S count) history)).
    rewrite <- IH.
    unfold remember_closed. destruct history as [|first rest]; cbn [remember latest first_depth].
    + rewrite same_shorter_self. f_equal. f_equal. f_equal. lia.
    + destruct (same_state final (latest first)); cbn [remember latest first_depth];
        rewrite same_shorter_self; f_equal; f_equal; f_equal; lia.
Qed.

Definition checked_remember final start width history :=
  if (0 <? width) && (start + width <=? depth final)
  then Some (remember_closed final start width history)
  else None.

Theorem bulk_remember_refuses_zero_or_outside_prefix final start width history :
  width = 0 \/ depth final < start + width ->
  checked_remember final start width history = None.
Proof.
  intros [->|outside]; unfold checked_remember; [reflexivity|].
  assert (start + width <=? depth final = false) by (apply Nat.leb_gt; exact outside).
  rewrite H, andb_false_r. reflexivity.
Qed.

Theorem accepted_bulk_remember_has_exact_scalar_history final start width history result :
  checked_remember final start width history = Some result ->
  0 < width /\ start + width <= depth final /\
  result = remember_steps final start width history.
Proof.
  unfold checked_remember. destruct ((0 <? width) && (start + width <=? depth final)) eqn:guard;
    [|discriminate]. intro accepted. inversion accepted; subst.
  rewrite andb_true_iff, Nat.ltb_lt, Nat.leb_le in guard.
  split; [tauto|]. split; [tauto|]. apply positive_bulk_remember_is_exactly_iterated_scalar_remember. tauto.
Qed.

Definition population first := S (depth (latest first) - first_depth first).

Definition restored_after first width :=
  match width with
  | 0 | 1 => latest first
  | S count => shorter (latest first) (depth (latest first) - count)
  end.

Definition remainder_after first rest width :=
  if width =? population first then rest
  else Run (shorter (latest first) (depth (latest first) - width)) (first_depth first) :: rest.

Definition checked_pop width history :=
  match history with
  | [] => None
  | first :: rest =>
      if (0 <? width) && (first_depth first <=? depth (latest first)) &&
          (width <=? population first)
      then Some (restored_after first width, remainder_after first rest width)
      else None
  end.

Fixpoint pop_steps width history : option (handle * list run) :=
  match width with
  | 0 => None
  | S count =>
      match pop history with
      | None => None
      | Some result =>
          match count with
          | 0 => Some (restored result, remaining_runs result)
          | S _ => pop_steps count (remaining_runs result)
          end
      end
  end.

Lemma positive_pop_steps_closed count first rest :
  first_depth first <= depth (latest first) -> S count <= population first ->
  pop_steps (S count) (first :: rest) =
    Some (restored_after first (S count), remainder_after first rest (S count)).
Proof.
  revert first rest. induction count as [|count IH]; intros [current lower] rest valid bound.
  - cbn [pop_steps pop latest first_depth restored remaining_runs restored_after].
    unfold remainder_after, population; cbn [latest first_depth] in *.
    destruct (depth current =? lower) eqn:at_end.
    + apply Nat.eqb_eq in at_end. rewrite at_end, Nat.sub_diag. reflexivity.
    + apply Nat.eqb_neq in at_end.
      assert (1 =? S (depth current - lower) = false) by (apply Nat.eqb_neq; lia).
      rewrite H. reflexivity.
  - assert (strict : lower < depth current) by (unfold population in bound; cbn in bound; lia).
    assert (boundary : depth current =? lower = false) by (apply Nat.eqb_neq; lia).
    change ((match pop (Run current lower :: rest) with
      | Some result => pop_steps (S count) (remaining_runs result)
      | None => None end) = Some (restored_after (Run current lower) (S (S count)),
        remainder_after (Run current lower) rest (S (S count)))).
    cbn [pop latest first_depth]. rewrite boundary.
    change (pop_steps (S count) (Run (shorter current (depth current - 1)) lower :: rest) =
      Some (restored_after (Run current lower) (S (S count)),
        remainder_after (Run current lower) rest (S (S count)))).
    rewrite IH.
    2: { cbn [latest first_depth]. rewrite shorter_depth. lia. }
    2: { unfold population in *; cbn [latest first_depth] in *; rewrite shorter_depth. lia. }
    unfold remainder_after, population, restored_after; cbn [latest first_depth].
    rewrite !shorter_depth, !shorter_shorter.
    replace (depth current - 1 - S count) with (depth current - S (S count)) by lia.
    assert (same_guard : (S count =? S (depth current - 1 - lower)) =
      (S (S count) =? S (depth current - lower))).
    { apply Bool.eq_true_iff_eq. rewrite !Nat.eqb_eq. lia. }
    rewrite same_guard.
    destruct count as [|count].
    + reflexivity.
    + cbn [restored_after]. rewrite ?shorter_shorter.
      replace (depth current - 1 - S count) with (depth current - S (S count)) by lia.
      reflexivity.
Qed.

Theorem accepted_run_bounded_pop_is_exactly_iterated_scalar_pop width history result :
  checked_pop width history = Some result -> pop_steps width history = Some result.
Proof.
  destruct history as [|first rest]; [discriminate|]. unfold checked_pop.
  destruct ((0 <? width) && (first_depth first <=? depth (latest first)) &&
      (width <=? population first)) eqn:guard; [|discriminate].
  intro accepted. inversion accepted; subst.
  rewrite !andb_true_iff, Nat.ltb_lt, !Nat.leb_le in guard.
  destruct width as [|count]; [lia|]. apply positive_pop_steps_closed; tauto.
Qed.

Theorem run_bounded_pop_refuses_zero_invalid_run_or_excess width first rest :
  width = 0 \/ depth (latest first) < first_depth first \/ population first < width ->
  checked_pop width (first :: rest) = None.
Proof.
  unfold checked_pop. intros [->|[bad|outside]]; [reflexivity| |].
  - assert (first_depth first <=? depth (latest first) = false) by (apply Nat.leb_gt; exact bad).
    rewrite H, andb_false_r. reflexivity.
  - assert (width <=? population first = false) by (apply Nat.leb_gt; exact outside).
    rewrite H, andb_false_r. reflexivity.
Qed.

Section Located.
Variable roots : nat -> CanonicalKeyProtocol.region.

Fixpoint predecessor_observations final start width :=
  match width with
  | 0 => []
  | S count => observe roots (shorter final (start + count)) ::
      predecessor_observations final start count
  end.

Lemma next_prefix_adds_one_located_byte final width :
  valid_handle roots final -> width < depth final ->
  exists byte, bytes roots (shorter final (S width)) =
    bytes roots (shorter final width) ++ [byte].
Proof.
  intros valid strict.
  assert (nonempty : 0 < length (skipn width (bytes roots final))).
  { rewrite length_skipn, (handle_length valid). lia. }
  destruct (skipn width (bytes roots final)) as [|byte tail] eqn:suffix;
    [cbn in nonempty; lia|].
  exists byte. rewrite !shorter_bytes by lia.
  replace (S width) with (width + 1) by lia.
  rewrite NormalizedSourceSpans.split_prefix, suffix. reflexivity.
Qed.

Lemma remember_steps_located final start width history :
  valid_handle roots final -> start + width <= depth final ->
  valid_history roots history ->
  scalar_chain (bytes roots (shorter final start)) (expand roots history) ->
  valid_history roots (remember_steps final start width history) /\
  scalar_chain (bytes roots (shorter final (start + width)))
    (expand roots (remember_steps final start width history)) /\
  expand roots (remember_steps final start width history) =
    predecessor_observations final start width ++ expand roots history.
Proof.
  intros final_valid. induction width as [|width IH]; intros bound history_valid chain.
  - cbn [remember_steps predecessor_observations]. rewrite Nat.add_0_r. tauto.
  - destruct (IH ltac:(lia) history_valid chain) as [prior_valid [prior_chain prior_observed]].
    assert (prefix_valid : valid_handle roots (shorter final (start + width)))
      by (apply shorter_is_valid; [exact final_valid|lia]).
    destruct (remembering_refines_exact_scalar_predecessor_push prefix_valid prior_valid prior_chain)
      as [observed preserved].
    split; [exact preserved|]. split.
    + replace (start + S width) with (S (start + width)) by lia.
      destruct (next_prefix_adds_one_located_byte (width := start + width)
        final_valid ltac:(lia)) as [byte appended].
      change (scalar_chain (bytes roots (shorter final (S (start + width))))
        (expand roots (remember (shorter final (start + width))
          (remember_steps final start width history)))).
      rewrite appended, observed. unfold observe. constructor. exact prior_chain.
    + change (expand roots (remember (shorter final (start + width))
        (remember_steps final start width history)) =
        predecessor_observations final start (S width) ++ expand roots history).
      rewrite observed, prior_observed. reflexivity.
Qed.

Theorem accepted_bulk_remember_preserves_valid_exact_located_scalar_history
    final start width history result :
  valid_handle roots final -> valid_history roots history ->
  scalar_chain (firstn start (bytes roots final)) (expand roots history) ->
  checked_remember final start width history = Some result ->
  valid_history roots result /\
  scalar_chain (firstn (start + width) (bytes roots final)) (expand roots result) /\
  expand roots result = predecessor_observations final start width ++ expand roots history.
Proof.
  intros final_valid history_valid chain accepted.
  destruct (@accepted_bulk_remember_has_exact_scalar_history final start width history result accepted)
    as [positive [bound ->]].
  assert (initial : scalar_chain (bytes roots (shorter final start)) (expand roots history)).
  { rewrite shorter_bytes by lia. exact chain. }
  pose proof (@remember_steps_located final start width history
    final_valid bound history_valid initial) as result.
  rewrite shorter_bytes in result by lia. exact result.
Qed.

Theorem each_bulk_predecessor_retains_its_exact_located_prefix final start width index :
  valid_handle roots final -> start + width <= depth final -> index < width ->
  valid_handle roots (shorter final (start + index)) /\
  observe roots (shorter final (start + index)) =
    (handle_state final, firstn (start + index) (bytes roots final)).
Proof.
  intros valid bounded within. split.
  - apply shorter_is_valid; [exact valid|lia].
  - unfold observe. rewrite shorter_state, shorter_bytes by lia. reflexivity.
Qed.

Lemma pop_steps_located count history last rest :
  valid_history roots history -> pop_steps (S count) history = Some (last, rest) ->
  valid_handle roots last /\ valid_history roots rest /\
  nth_error (expand roots history) count = Some (observe roots last) /\
  expand roots rest = skipn (S count) (expand roots history).
Proof.
  revert history last rest. induction count as [|count IH]; intros history last rest valid accepted.
  - cbn [pop_steps] in accepted.
    destruct (pop history) as [result|] eqn:popped; [|discriminate].
    inversion accepted; subst.
    destruct (popping_refines_exact_scalar_head_and_tail valid popped)
      as [expanded [head [tail work]]].
    split; [exact head|]. split; [exact tail|].
    split; rewrite expanded; reflexivity.
  - change ((match pop history with
      | Some result => pop_steps (S count) (remaining_runs result)
      | None => None end) = Some (last, rest)) in accepted.
    destruct (pop history) as [result|] eqn:popped; [|discriminate].
    destruct (popping_refines_exact_scalar_head_and_tail valid popped)
      as [expanded [head [tail work]]].
    destruct (IH _ _ _ tail accepted) as [last_valid [rest_valid [selected remainder]]].
    split; [exact last_valid|]. split; [exact rest_valid|].
    split; rewrite expanded; cbn; assumption.
Qed.

Theorem accepted_bulk_pop_preserves_exact_located_selection_and_remaining_history
    width history last rest :
  valid_history roots history -> checked_pop width history = Some (last, rest) ->
  valid_handle roots last /\ valid_history roots rest /\
  nth_error (expand roots history) (width - 1) = Some (observe roots last) /\
  expand roots rest = skipn width (expand roots history).
Proof.
  intros valid accepted.
  pose proof (@accepted_run_bounded_pop_is_exactly_iterated_scalar_pop
    width history (last, rest) accepted) as scalar.
  destruct width as [|count]; [discriminate|].
  replace (S count - 1) with count by lia.
  eapply pop_steps_located; eauto.
Qed.

End Located.

Theorem positive_bounded_remember_constructively_accepts final start width history :
  0 < width -> start + width <= depth final ->
  checked_remember final start width history =
    Some (remember_closed final start width history).
Proof.
  intros positive bounded. unfold checked_remember.
  assert (0 <? width = true) by (apply Nat.ltb_lt; exact positive).
  assert (start + width <=? depth final = true) by (apply Nat.leb_le; exact bounded).
  rewrite H, H0. reflexivity.
Qed.

Theorem positive_valid_run_bounded_pop_constructively_accepts width first rest :
  0 < width -> first_depth first <= depth (latest first) -> width <= population first ->
  checked_pop width (first :: rest) =
    Some (restored_after first width, remainder_after first rest width).
Proof.
  intros positive valid bounded. unfold checked_pop.
  assert (0 <? width = true) by (apply Nat.ltb_lt; exact positive).
  assert (first_depth first <=? depth (latest first) = true) by (apply Nat.leb_le; exact valid).
  assert (width <=? population first = true) by (apply Nat.leb_le; exact bounded).
  rewrite H, H0, H1. reflexivity.
Qed.

Print Assumptions positive_bulk_remember_is_exactly_iterated_scalar_remember.
Print Assumptions bulk_remember_refuses_zero_or_outside_prefix.
Print Assumptions accepted_bulk_remember_has_exact_scalar_history.
Print Assumptions accepted_run_bounded_pop_is_exactly_iterated_scalar_pop.
Print Assumptions run_bounded_pop_refuses_zero_invalid_run_or_excess.
Print Assumptions each_bulk_predecessor_retains_its_exact_located_prefix.
Print Assumptions accepted_bulk_remember_preserves_valid_exact_located_scalar_history.
Print Assumptions accepted_bulk_pop_preserves_exact_located_selection_and_remaining_history.
Print Assumptions positive_bounded_remember_constructively_accepts.
Print Assumptions positive_valid_run_bounded_pop_constructively_accepts.

End BulkCompressedUndo.

Module RetainedAdmissionHistory.

Section Projection.
Context {Occurrence Key Error : Type}.
Variable project : Occurrence -> Key.
Variable decide : option Key -> Occurrence -> option Error.

Definition latest (history : list Occurrence) : option Key :=
  option_map project (hd_error (rev history)).

Definition consistent (state : list Occurrence * option Key) : Prop :=
  snd state = latest (fst state).

Definition cached_step (state : list Occurrence * option Key) (candidate : Occurrence)
    : Error + (list Occurrence * option Key) :=
  match decide (snd state) candidate with
  | Some error => inl error
  | None => inr (fst state ++ [candidate], Some (project candidate))
  end.

Definition projected_step (history : list Occurrence) (candidate : Occurrence)
    : Error + list Occurrence :=
  match decide (latest history) candidate with
  | Some error => inl error
  | None => inr (history ++ [candidate])
  end.

Definition erase (result : Error + (list Occurrence * option Key))
    : Error + list Occurrence :=
  match result with inl error => inl error | inr state => inr (fst state) end.

Lemma latest_append history candidate :
  latest (history ++ [candidate]) = Some (project candidate).
Proof. unfold latest. rewrite rev_app_distr. reflexivity. Qed.

Lemma empty_consistent : consistent ([], None).
Proof. reflexivity. Qed.

Lemma cached_success_preserves_consistency state candidate next :
  cached_step state candidate = inr next -> consistent next.
Proof.
  unfold cached_step, consistent.
  destruct (decide (snd state) candidate); intro H; inversion H; subst.
  cbn. symmetry. apply latest_append.
Qed.

Theorem exact_step_error_and_history state candidate :
  consistent state -> erase (cached_step state candidate) =
    projected_step (fst state) candidate.
Proof.
  unfold consistent, cached_step, projected_step. intro H. rewrite H.
  destruct (decide (latest (fst state)) candidate); reflexivity.
Qed.

Fixpoint cached_run (input : list Occurrence) (state : list Occurrence * option Key)
    : Error + list Occurrence :=
  match input with
  | [] => inr (fst state)
  | candidate :: rest =>
      match cached_step state candidate with
      | inl error => inl error
      | inr next => cached_run rest next
      end
  end.

Fixpoint projected_run (input history : list Occurrence) : Error + list Occurrence :=
  match input with
  | [] => inr history
  | candidate :: rest =>
      match projected_step history candidate with
      | inl error => inl error
      | inr next => projected_run rest next
      end
  end.

Theorem exact_trace_first_error_and_accepted_history input state :
  consistent state -> cached_run input state = projected_run input (fst state).
Proof.
  revert state. induction input as [|candidate rest IH]; intros state valid; [reflexivity|].
  cbn. pose proof (exact_step_error_and_history candidate valid) as step.
  destruct (cached_step state candidate) as [error|next] eqn:old;
    cbn [erase] in step; rewrite <- step; [reflexivity|].
  apply IH. eapply cached_success_preserves_consistency; exact old.
Qed.

Theorem rejected_candidate_preserves_retained_history history candidate error :
  decide (latest history) candidate = Some error ->
  projected_step history candidate = inl error.
Proof. unfold projected_step. intro H. rewrite H. reflexivity. Qed.

Definition replace_slot (slots : nat -> list Occurrence) (slot : nat)
    (history : list Occurrence) : nat -> list Occurrence :=
  fun index => if Nat.eqb index slot then history else slots index.

Theorem unrelated_slot_is_unchanged slots slot history other :
  other <> slot -> latest (replace_slot slots slot history other) = latest (slots other).
Proof.
  intro H. unfold replace_slot. apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

End Projection.

Example retaining_first_instead_of_last_is_detected :
  @latest nat nat (fun value => value) [1; 2] <>
    option_map (fun value : nat => value) (hd_error [1; 2]).
Proof. discriminate. Qed.

Print Assumptions latest_append.
Print Assumptions empty_consistent.
Print Assumptions cached_success_preserves_consistency.
Print Assumptions exact_step_error_and_history.
Print Assumptions exact_trace_first_error_and_accepted_history.
Print Assumptions rejected_candidate_preserves_retained_history.
Print Assumptions unrelated_slot_is_unchanged.
Print Assumptions retaining_first_instead_of_last_is_detected.

End RetainedAdmissionHistory.

(** Accepted message fields arrive in nondecreasing generated Prost-slot order.
    The production message frame retains one occurrence list per slot, so a
    later semantic reducer may concatenate those lists only if the indexed
    update preserves the original child order. This finite worklist states
    that obligation independently of a particular summary type. *)
Module OrderedSemanticSlots.

Section Buckets.
Variable Value : Type.

Definition buckets := nat -> list Value.
Definition state := (option nat * buckets)%type.

Definition append_at (saved : buckets) (slot : nat) (value : Value)
    : buckets :=
  fun index => if index =? slot then saved index ++ [value] else saved index.

Definition flatten (slot_count : nat) (saved : buckets) : list Value :=
  flat_map saved (seq 0 slot_count).

Definition empty_after (last : option nat) (saved : buckets) : Prop :=
  forall index,
    (match last with None => True | Some previous => previous < index end) ->
    saved index = [].

Definition follows (last : option nat) (slot : nat) : bool :=
  match last with None => true | Some previous => previous <=? slot end.

Fixpoint accept (slot_count : nat) (events : list (nat * Value))
    (current : state) : option state :=
  match events with
  | [] => Some current
  | (slot, value) :: rest =>
      if (slot <? slot_count) && follows (fst current) slot then
        accept slot_count rest
          (Some slot, append_at (snd current) slot value)
      else None
  end.

Lemma flat_map_empty_suffix : forall start width (saved : buckets),
  (forall index, start <= index -> saved index = []) ->
  flat_map saved (seq start width) = [].
Proof.
  intros start width. revert start.
  induction width as [|width IH]; intros start saved empty; simpl; [reflexivity|].
  rewrite (empty start) by lia. simpl.
  apply IH. intros index later. apply empty. lia.
Qed.

Lemma flat_map_before_slot_unchanged : forall start width saved slot value,
  start + width <= slot ->
  flat_map (append_at saved slot value) (seq start width) =
    flat_map saved (seq start width).
Proof.
  intros start width. revert start.
  induction width as [|width IH];
    intros start saved slot value before; simpl; [reflexivity|].
  unfold append_at at 1.
  destruct (start =? slot) eqn:same.
  - apply Nat.eqb_eq in same. lia.
  - rewrite (IH (S start) saved slot value) by lia. reflexivity.
Qed.

Lemma append_at_empty_after : forall saved slot value,
  (forall index, slot < index -> saved index = []) ->
  forall index, slot < index -> append_at saved slot value index = [].
Proof.
  intros saved slot value empty index later.
  unfold append_at.
  destruct (index =? slot) eqn:same.
  - apply Nat.eqb_eq in same. lia.
  - apply empty. exact later.
Qed.

Lemma flatten_append_at : forall slot_count saved slot value,
  slot < slot_count ->
  (forall index, slot < index -> saved index = []) ->
  flatten slot_count (append_at saved slot value) =
    flatten slot_count saved ++ [value].
Proof.
  intros slot_count saved slot value within empty.
  assert (split_seq : seq 0 slot_count =
      seq 0 slot ++ slot :: seq (S slot) (slot_count - S slot)).
  { assert (split_len : slot_count = slot + S (slot_count - S slot)) by lia.
    rewrite split_len at 1. rewrite seq_app. reflexivity. }
  unfold flatten. rewrite !split_seq, !flat_map_app.
  simpl [flat_map].
  rewrite (@flat_map_before_slot_unchanged 0 slot saved slot value) by lia.
  unfold append_at at 1. rewrite Nat.eqb_refl.
  rewrite (@flat_map_empty_suffix (S slot) (slot_count - S slot)
    (append_at saved slot value)).
  2: { intros index later. apply append_at_empty_after; [exact empty|lia]. }
  rewrite (@flat_map_empty_suffix (S slot) (slot_count - S slot) saved).
  2: { intros index later. apply empty. lia. }
  repeat rewrite app_nil_r. now rewrite app_assoc.
Qed.

Lemma empty_after_follows : forall last saved slot,
  empty_after last saved -> follows last slot = true ->
  forall index, slot < index -> saved index = [].
Proof.
  intros [previous|] saved slot empty allowed index later.
  - unfold follows in allowed. apply Nat.leb_le in allowed.
    apply empty. simpl. lia.
  - apply empty. simpl. exact I.
Qed.

Theorem accepted_occurrences_keep_exact_source_order :
  forall slot_count events last saved final_last final_saved,
    empty_after last saved ->
    accept slot_count events (last, saved) = Some (final_last, final_saved) ->
    empty_after final_last final_saved /\
    flatten slot_count final_saved =
      flatten slot_count saved ++ map snd events.
Proof.
  intros slot_count events.
  induction events as [|[slot value] rest IH];
    intros last saved final_last final_saved empty returned.
  - simpl in returned. inversion returned; subst.
    split; [exact empty|now rewrite app_nil_r].
  - simpl in returned.
    destruct ((slot <? slot_count) && follows last slot) eqn:allowed;
      [|discriminate].
    apply andb_true_iff in allowed as [within follows_last].
    apply Nat.ltb_lt in within.
    pose proof (@empty_after_follows last saved slot empty follows_last)
      as empty_above_slot.
    assert (new_empty : empty_after (Some slot) (append_at saved slot value)).
    { intros index later. simpl in later.
      apply append_at_empty_after; [exact empty_above_slot|exact later]. }
    destruct (IH (Some slot) (append_at saved slot value)
      final_last final_saved new_empty returned) as [final_empty final_order].
    split; [exact final_empty|].
    simpl [map]. rewrite final_order.
    rewrite (@flatten_append_at slot_count saved slot value within empty_above_slot).
    now rewrite <- app_assoc.
Qed.

Theorem decreasing_or_out_of_range_occurrence_is_refused :
  forall slot_count last saved slot value rest,
    slot_count <= slot \/
    (exists previous, last = Some previous /\ slot < previous) ->
    accept slot_count ((slot, value) :: rest) (last, saved) = None.
Proof.
  intros slot_count last saved slot value rest bad.
  simpl. destruct bad as [outside|[previous [same decreasing]]].
  - assert (bad_slot : (slot <? slot_count) = false).
    { apply Nat.ltb_ge. exact outside. }
    rewrite bad_slot. reflexivity.
  - subst last. unfold follows.
    assert (bad_order : (previous <=? slot) = false).
    { apply Nat.leb_gt. exact decreasing. }
    rewrite bad_order. now rewrite andb_false_r.
Qed.

End Buckets.

Definition empty_nat_buckets : buckets nat := fun _ => [].

Example three_source_ordered_occurrences_are_preserved :
  option_map (fun final => flatten 3 (snd final))
    (accept 3 [(0, 10); (0, 11); (2, 20)]
      (None, empty_nat_buckets)) = Some [10; 11; 20].
Proof. vm_compute. reflexivity. Qed.

Print Assumptions accepted_occurrences_keep_exact_source_order.
Print Assumptions decreasing_or_out_of_range_occurrence_is_refused.
Print Assumptions three_source_ordered_occurrences_are_preserved.

End OrderedSemanticSlots.

(** * Concrete non-vacuity, termination and determinism

    The admission machine of [SourceIndexedFrames] on concrete generated wire
    inputs, all through the same reducer: an admitted ordered New, an admitted
    Par whose Send children are in score order and an accepted exact claimed
    score; refusals for a malformed unforgeable payload, a trailing root byte, a
    New with reversed map keys, a Par whose Send children are out of score order
    and a forged claimed score.  The machine terminates on every input and its
    terminal result is unique, with no fuel and no depth cap. *)

From StackSafePDA Require RPB3SourceIndexedTermination.

Definition concrete_witness_hash (_ : list nat) : nat := 0.

Definition concrete_admitted_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 97; 34; 3; 10; 1; 98].

Definition concrete_reversed_new_body : list nat :=
  [18; 0; 34; 3; 10; 1; 98; 34; 3; 10; 1; 97].

Definition concrete_trailing_new_body : list nat :=
  concrete_admitted_new_body ++ [0].

Definition concrete_malformed_unforgeable_body : list nat :=
  [10; 3; 10; 2; 42].

Definition concrete_ascending_send_par_body : list nat :=
  [10; 0; 10; 2; 24; 1].

Definition concrete_descending_send_par_body : list nat :=
  [10; 2; 24; 1; 10; 0].

Theorem concrete_nonvacuity_and_termination :
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 20
    concrete_admitted_new_body = true /\
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 1
    concrete_ascending_send_par_body = true /\
  SourceIndexedFrames.source_indexed_root_claimed_score concrete_witness_hash 20
    concrete_admitted_new_body
    (RPB3SemanticReducer.canonical_score_bytes
      RPB3RecursiveAuxiliarySourceReference.ordered_two_absent_value_maps_new_score) =
    SourceIndexedFrames.ClaimedScoreAccepted /\
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 60
    concrete_malformed_unforgeable_body = false /\
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 20
    concrete_trailing_new_body = false /\
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 20
    concrete_reversed_new_body = false /\
  SourceIndexedFrames.source_indexed_root_accepts concrete_witness_hash 1
    concrete_descending_send_par_body = false /\
  SourceIndexedFrames.source_indexed_root_claimed_score concrete_witness_hash 20
    concrete_admitted_new_body
    (RPB3SemanticReducer.canonical_score_bytes
      RPB3RecursiveAuxiliarySourceReference.ordered_two_absent_value_maps_new_score ++ [0]) =
    SourceIndexedFrames.ClaimedScoreRejected /\
  (forall hash_fn type_id bytes,
    exists final,
      RPB3SourceIndexedTermination.reaches
        (RPB3RecursiveAuxiliarySourceReference.recursive_par_reference hash_fn)
        hash_fn bytes
        (RPB3SourceIndexedTermination.root_configuration type_id bytes) final /\
      RPB3SourceIndexedTermination.terminal final /\
      (forall another,
        RPB3SourceIndexedTermination.reaches
          (RPB3RecursiveAuxiliarySourceReference.recursive_par_reference hash_fn)
          hash_fn bytes
          (RPB3SourceIndexedTermination.root_configuration type_id bytes) another ->
        RPB3SourceIndexedTermination.terminal another -> another = final) /\
      RPB3SourceIndexedTermination.terminal_root_result final =
        SourceIndexedFrames.source_indexed_root_full_result hash_fn type_id bytes).
Proof.
  refine (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ _)))))))).
  1-8: vm_compute; reflexivity.
  exact RPB3SourceIndexedTermination.fuel_free_source_indexed_root_terminates_deterministically.
Qed.

Print Assumptions concrete_nonvacuity_and_termination.
