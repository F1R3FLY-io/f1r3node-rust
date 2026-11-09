From Stdlib Require Import List Bool Arith Lia.
From StackSafePDA Require Import StackSafePDA RPB3DescriptorProjectionData.

Import ListNotations.
Set Implicit Arguments.

(** A finite, source-shaped slice of the RPB3 message admission driver.
    [resolve_protobuf_field] in match_schema.rs maps an actual tag to a Prost
    slot and selected oneof arm. [message_field_step] in protobuf_admission.rs
    checks wire type, nondecreasing slot order, and singular/oneof duplicates
    before dispatch. This theory gives those checks a typed summary algebra.

    The raw field body and score plan remain symbolic. In particular, this
    model does not establish canonical varint decoding, byte-exact score
    grammar, all generated programs, specialized reducers, or error location. *)

Inductive wire_kind := Varint | Fixed64 | LengthDelimited | Fixed32.
Inductive payload_kind := Scalar | Message | StringMap | PathMap.

Definition wire_eqb (left right : wire_kind) : bool :=
  match left, right with
  | Varint, Varint | Fixed64, Fixed64
  | LengthDelimited, LengthDelimited | Fixed32, Fixed32 => true
  | _, _ => false
  end.

Definition payload_eqb (left right : payload_kind) : bool :=
  match left, right with
  | Scalar, Scalar | Message, Message
  | StringMap, StringMap | PathMap, PathMap => true
  | _, _ => false
  end.

Record field_descriptor := FieldDescriptor {
  descriptor_tag : nat;
  descriptor_slot : nat;
  descriptor_wire : wire_kind;
  descriptor_repeated : bool;
  descriptor_payload : payload_kind;
  descriptor_oneof : option nat;
  descriptor_child_type_id : option nat
}.

Record generated_program := GeneratedProgram {
  program_family : nat;
  program_slot_count : nat;
  program_fields : list field_descriptor;
  program_wire_required : list nat;
  program_semantic_required : list nat
}.

(** Adapter for the live-generated Rust descriptor projection. Oneof arm tags
    are actual wire tags, while all arms retain their one Prost slot. The
    requirements lists are reducer-specific and are not in the generated
    descriptor; callers must state them separately. *)
Definition projected_wire (wire : GeneratedData.wire_kind) : wire_kind :=
  match wire with
  | GeneratedData.WTVarint => Varint
  | GeneratedData.WTFixed64 => Fixed64
  | GeneratedData.WTLengthDelimited => LengthDelimited
  | GeneratedData.WTFixed32 => Fixed32
  end.

Definition projected_slot_payload
    (slot : GeneratedData.projection_slot) : payload_kind :=
  match GeneratedData.slot_field_kind slot with
  | GeneratedData.FKOptionalMessage | GeneratedData.FKMessageSequence => Message
  | GeneratedData.FKStringMessageMap => StringMap
  | GeneratedData.FKPathMap => PathMap
  | _ => Scalar
  end.

Definition projected_arm_payload
    (arm : GeneratedData.projection_arm) : payload_kind :=
  match GeneratedData.arm_payload arm with
  | GeneratedData.PKMessage => Message
  | _ => Scalar
  end.

Definition projected_arm_field (slot : GeneratedData.projection_slot)
    (arm : GeneratedData.projection_arm) : list field_descriptor :=
  match GeneratedData.arm_wire arm with
  | None => []
  | Some wire => [FieldDescriptor (GeneratedData.arm_tag arm)
      (GeneratedData.slot_index slot) (projected_wire wire) false
      (projected_arm_payload arm) (Some (GeneratedData.slot_index slot))
      (GeneratedData.arm_child_id arm)]
  end.

Definition projected_slot_fields
    (slot : GeneratedData.projection_slot) : list field_descriptor :=
  match GeneratedData.slot_arms slot with
  | [] =>
      match GeneratedData.slot_wire slot with
      | None => []
      | Some wire => [FieldDescriptor (GeneratedData.slot_min_tag slot)
          (GeneratedData.slot_index slot) (projected_wire wire)
          (GeneratedData.slot_repeated slot) (projected_slot_payload slot) None
          (GeneratedData.slot_child_id slot)]
      end
  | arms => flat_map (projected_arm_field slot) arms
  end.

Definition project_generated_program (program : GeneratedData.projection_program)
    (wire_required semantic_required : list nat) : generated_program :=
  GeneratedProgram (GeneratedData.program_type_id program)
    (length (GeneratedData.program_slots program))
    (flat_map projected_slot_fields (GeneratedData.program_slots program))
    wire_required semantic_required.

Definition lookup_projected_program (type_id : nat)
    (wire_required semantic_required : list nat) : option generated_program :=
  option_map (fun program => project_generated_program program
    wire_required semantic_required)
    (find (fun program => GeneratedData.program_type_id program =? type_id)
      GeneratedData.generated_programs).

Definition empty_program : generated_program := GeneratedProgram 0 0 [] [] [].

Fixpoint resolve_tag (fields : list field_descriptor) (tag : nat)
    : option field_descriptor :=
  match fields with
  | [] => None
  | field :: rest =>
      if descriptor_tag field =? tag then Some field else resolve_tag rest tag
  end.

Definition program_well_formed (program : generated_program) : Prop :=
  NoDup (map descriptor_tag (program_fields program)) /\
  Forall (fun field => 0 < descriptor_tag field /\
    descriptor_slot field < program_slot_count program /\
    match descriptor_oneof field with
    | None => True
    | Some group => descriptor_repeated field = false /\
        forall other, In other (program_fields program) ->
          descriptor_oneof other = Some group ->
          descriptor_slot other = descriptor_slot field
    end) (program_fields program).

(** Each node is one already-decoded message or scalar field. A node's program
    is the descriptor table for its immediate children. [raw_body] is an opaque
    byte slice; [raw_declared_length] checks its finite ownership bound.
    There is not yet a proof that parent body bytes contain these child fields
    at exact encoded ranges. The program shape is looked up by type ID from
    the generated table; reducer-specific requirements remain explicit.
    The witnesses below are typed-field controls rather than raw-wire
    admissions. *)
Record raw_field := RawField {
  raw_tag : nat;
  raw_wire : wire_kind;
  raw_payload : payload_kind;
  raw_body : list nat;
  raw_declared_length : nat;
  raw_program_type_id : option nat;
  raw_wire_required : list nat;
  raw_semantic_required : list nat
}.

Definition resolved_raw_program (field : raw_field)
    : option generated_program :=
  (** Map and EPathMap need their own frame/result constructors. They are
      refused here rather than treated as ordinary message programs. *)
  match raw_payload field, raw_program_type_id field with
  | Message, Some type_id => lookup_projected_program type_id
      (raw_wire_required field) (raw_semantic_required field)
  | Scalar, None => Some empty_program
  | _, _ => None
  end.

Inductive score_plan := ScorePlan
  (family : nat) (semantic : bool) (ordered_children : list score_plan).

Record child_observation := ChildObservation {
  observed_tag : nat;
  observed_body_bytes : list nat;
  observed_score_plan : score_plan;
  observed_complete : bool
}.

Record typed_summary := TypedSummary {
  summary_tag : nat;
  summary_wire : wire_kind;
  summary_payload : payload_kind;
  summary_program_type_id : nat;
  summary_body : list nat;
  summary_valid : bool;
  summary_complete : bool;
  summary_score : score_plan;
  summary_ordered_slots : list nat;
  summary_children : list child_observation
}.

Definition observe_child (child : typed_summary) : child_observation :=
  ChildObservation (summary_tag child) (summary_body child)
    (summary_score child) (summary_complete child).

Record scan_state := ScanState {
  scan_valid : bool;
  scan_last_slot : option nat;
  scan_seen : list nat;
  scan_ordered_slots : list nat;
  scan_ordered_scores : list score_plan
}.

Definition empty_scan : scan_state := ScanState true None [] [] [].

Definition slot_follows (last : option nat) (slot : nat) (repeated : bool) : bool :=
  match last with
  | None => true
  | Some previous => (previous <? slot) || ((previous =? slot) && repeated)
  end.

(** A message arm/slot names the child RhoMatchProgram in the generated Rust
    table. Map and EPathMap payloads are separate frame roles; their returned
    summary type binding needs a dedicated auxiliary-frame refinement. *)
Definition message_child_type_matches
    (field : field_descriptor) (child : typed_summary) : bool :=
  match descriptor_payload field with
  | Message =>
      match descriptor_child_type_id field with
      | Some type_id => summary_program_type_id child =? type_id
      | None => false
      end
  | Scalar | StringMap | PathMap => true
  end.

Definition checked_field (program : generated_program)
    (state : scan_state) (child : typed_summary) : scan_state :=
  if scan_valid state then
    match resolve_tag (program_fields program) (summary_tag child) with
    | None => ScanState false (scan_last_slot state) (scan_seen state)
        (scan_ordered_slots state)
        (scan_ordered_scores state)
    | Some field =>
        let slot := descriptor_slot field in
        let admissible :=
          summary_valid child &&
          wire_eqb (summary_wire child) (descriptor_wire field) &&
          payload_eqb (summary_payload child) (descriptor_payload field) &&
          message_child_type_matches field child &&
          (slot <? program_slot_count program) &&
          slot_follows (scan_last_slot state) slot (descriptor_repeated field) &&
          (descriptor_repeated field || negb (existsb (Nat.eqb slot) (scan_seen state))) in
        if admissible then
          ScanState true (Some slot) (slot :: scan_seen state)
            (scan_ordered_slots state ++ [slot])
            (scan_ordered_scores state ++ [summary_score child])
        else ScanState false (scan_last_slot state) (scan_seen state)
          (scan_ordered_slots state)
          (scan_ordered_scores state)
    end
  else state.

Definition all_present (required seen : list nat) : bool :=
  forallb (fun slot => existsb (Nat.eqb slot) seen) required.

Definition valid_raw_body (field : raw_field) : bool :=
  (raw_declared_length field =? length (raw_body field)) &&
  forallb (fun byte => byte <? 256) (raw_body field).

Definition reduce_field (field : raw_field) (children : list typed_summary)
    : typed_summary :=
  let resolved := resolved_raw_program field in
  let program := match resolved with Some program => program | None => empty_program end in
  let scanned := fold_left (checked_field program) children empty_scan in
  let scalar_has_no_children :=
    match raw_payload field with Scalar => match children with [] => true | _ => false end
    | _ => true end in
  let valid := (match resolved with Some _ => true | None => false end) &&
    valid_raw_body field && scalar_has_no_children &&
    scan_valid scanned &&
    all_present (program_wire_required program) (scan_seen scanned) in
  let complete :=
    all_present (program_semantic_required program) (scan_seen scanned) &&
    forallb summary_complete children in
  TypedSummary (raw_tag field) (raw_wire field) (raw_payload field)
    (program_family program)
    (raw_body field) valid complete
    (ScorePlan (program_family program) complete
      (scan_ordered_scores scanned))
    (scan_ordered_slots scanned)
    (map observe_child children).

(** This recursive typed interpreter is the independent specification. It
    reduces the original tree directly, without consulting worklist states. *)
Fixpoint typed_reference (subject : @tree raw_field) : typed_summary :=
  match subject with
  | Node field children => reduce_field field (typed_reference_forest children)
  end
with typed_reference_forest (subjects : @forest raw_field) : list typed_summary :=
  match subjects with
  | FNil => []
  | FCons subject rest => typed_reference subject :: typed_reference_forest rest
  end.

(** The machine expands message/field forests one frame at a time. Its pending
    jobs and completed summaries are explicit lists; execution never invokes
    the recursive typed reference or a precompiled instruction sequence. *)
Inductive frame_job :=
| Explore (subject : @tree raw_field)
| ExploreForest (subjects : @forest raw_field)
| Finish (field : raw_field) (arity : nat).

Fixpoint visit_cost (subject : @tree raw_field) : nat :=
  match subject with Node _ children => S (forest_cost children + 1) end
with forest_cost (subjects : @forest raw_field) : nat :=
  match subjects with
  | FNil => 1
  | FCons subject rest => S (visit_cost subject + forest_cost rest)
  end.

Definition job_cost (job : frame_job) : nat :=
  match job with
  | Explore subject => visit_cost subject
  | ExploreForest subjects => forest_cost subjects
  | Finish _ _ => 1
  end.

Fixpoint frame_budget (jobs : list frame_job) : nat :=
  match jobs with [] => 0 | job :: rest => job_cost job + frame_budget rest end.

Lemma every_pending_job_has_positive_cost :
  forall job, 0 < job_cost job.
Proof.
  intros [subject|subjects|field arity].
  - destruct subject as [field children]. simpl. lia.
  - destruct subjects as [|first rest]; simpl; lia.
  - simpl. lia.
Qed.

Theorem pending_frame_count_never_exceeds_remaining_budget :
  forall jobs, length jobs <= frame_budget jobs.
Proof.
  induction jobs as [|job rest IH]; simpl; [lia|].
  pose proof (every_pending_job_has_positive_cost job). lia.
Qed.

Lemma explore_budget_decreases_exactly_once field children rest :
  S (frame_budget (ExploreForest children ::
      Finish field (forest_arity children) :: rest)) =
    frame_budget (Explore (Node field children) :: rest).
Proof. simpl. lia. Qed.

Lemma forest_cons_budget_decreases_exactly_once child siblings rest :
  S (frame_budget (Explore child :: ExploreForest siblings :: rest)) =
    frame_budget (ExploreForest (FCons child siblings) :: rest).
Proof. simpl. lia. Qed.

Lemma forest_nil_budget_decreases_exactly_once rest :
  S (frame_budget rest) = frame_budget (ExploreForest FNil :: rest).
Proof. reflexivity. Qed.

Lemma finish_budget_decreases_exactly_once field arity rest :
  S (frame_budget rest) = frame_budget (Finish field arity :: rest).
Proof. reflexivity. Qed.

Fixpoint run_frames (fuel : nat) (jobs : list frame_job)
    (values : list typed_summary) : option (list typed_summary) :=
  match fuel with
  | 0 => match jobs with [] => Some values | _ => None end
  | S remaining =>
      match jobs with
      | [] => Some values
      | Explore (Node field children) :: rest =>
          run_frames remaining
            (ExploreForest children :: Finish field (forest_arity children) :: rest) values
      | ExploreForest FNil :: rest => run_frames remaining rest values
      | ExploreForest (FCons child siblings) :: rest =>
          run_frames remaining
            (Explore child :: ExploreForest siblings :: rest) values
      | Finish field arity :: rest =>
          if arity <=? length values then
            run_frames remaining rest
              (reduce_field field (rev (firstn arity values)) :: skipn arity values)
          else None
      end
  end.

Definition admission_worklist (subject : @tree raw_field)
    : option (list typed_summary) :=
  run_frames (visit_cost subject) [Explore subject] [].

(** The generic postorder program is used only as a proof bridge. It is not
    constructed by [run_frames]. Each expansion step preserves its denotation,
    while [frame_budget] decreases by exactly one. *)
Definition job_program (job : frame_job) : list (@instruction raw_field) :=
  match job with
  | Explore subject => compile_tree subject
  | ExploreForest subjects => compile_forest subjects
  | Finish field arity => [Reduce field arity]
  end.

Fixpoint pending_program (jobs : list frame_job) : list (@instruction raw_field) :=
  match jobs with
  | [] => []
  | job :: rest => job_program job ++ pending_program rest
  end.

Lemma frame_machine_refines_postorder_program fuel :
  forall jobs values, frame_budget jobs <= fuel ->
    run_frames fuel jobs values =
      @run raw_field typed_summary reduce_field (pending_program jobs) values.
Proof.
  induction fuel as [|fuel IH]; intros jobs values bound.
  - destruct jobs as [|job rest]; [reflexivity|].
    destruct job as [subject|subjects|field arity].
    + destruct subject as [field children]. simpl in bound. lia.
    + destruct subjects as [|first siblings]; simpl in bound; lia.
    + simpl in bound. lia.
  - destruct jobs as [|job rest]; [reflexivity|].
    destruct job as [subject|subjects|field arity].
    + destruct subject as [field children].
      simpl [run_frames].
      rewrite (IH (ExploreForest children :: Finish field (forest_arity children) :: rest)
        values) by (pose proof (explore_budget_decreases_exactly_once field children rest); lia).
      simpl [pending_program job_program compile_tree].
      now rewrite <- app_assoc.
    + destruct subjects as [|first siblings].
      * simpl [run_frames].
        rewrite (IH rest values) by (pose proof (forest_nil_budget_decreases_exactly_once rest); lia).
        reflexivity.
      * simpl [run_frames].
        rewrite (IH (Explore first :: ExploreForest siblings :: rest) values)
          by (pose proof (forest_cons_budget_decreases_exactly_once first siblings rest); lia).
        simpl [pending_program job_program compile_forest].
        now rewrite <- app_assoc.
    + simpl [run_frames pending_program job_program run].
      destruct (arity <=? length values) eqn:enough; [|reflexivity].
      apply IH. pose proof (finish_budget_decreases_exactly_once field arity rest). lia.
Qed.

Lemma typed_reference_is_recursive_fold :
  (forall subject, typed_reference subject =
    @fold_tree raw_field typed_summary reduce_field subject) /\
  (forall subjects, typed_reference_forest subjects =
    @fold_forest raw_field typed_summary reduce_field subjects).
Proof.
  apply tree_forest_induction; intros; simpl; congruence.
Qed.

Theorem explicit_worklist_equals_independent_typed_reference :
  forall subject, admission_worklist subject = Some [typed_reference subject].
Proof.
  intro subject. unfold admission_worklist.
  rewrite frame_machine_refines_postorder_program by (simpl; lia).
  simpl [pending_program job_program].
  rewrite app_nil_r.
  rewrite (proj1 typed_reference_is_recursive_fold).
  apply pda_fold_equivalent_to_recursive_fold.
Qed.

Theorem explicit_frames_finish_within_the_structural_budget :
  forall subject, exists summary,
    run_frames (visit_cost subject) [Explore subject] [] = Some [summary].
Proof.
  intro subject. exists (typed_reference subject).
  apply explicit_worklist_equals_independent_typed_reference.
Qed.

Inductive admission_outcome :=
| AcceptedSummary (summary : typed_summary)
| Refused.

Definition classify_summary (summary : typed_summary) : admission_outcome :=
  if summary_valid summary then AcceptedSummary summary else Refused.

Definition reference_outcome (subject : @tree raw_field) : admission_outcome :=
  classify_summary (typed_reference subject).

Definition worklist_outcome (subject : @tree raw_field) : admission_outcome :=
  match admission_worklist subject with
  | Some [summary] => classify_summary summary
  | _ => Refused
  end.

Theorem explicit_worklist_preserves_modeled_summary_or_refusal :
  forall subject, worklist_outcome subject = reference_outcome subject.
Proof.
  intro subject. unfold worklist_outcome, reference_outcome.
  rewrite explicit_worklist_equals_independent_typed_reference. reflexivity.
Qed.

Definition reference_accepts (subject : @tree raw_field) : bool :=
  summary_valid (typed_reference subject).

Definition worklist_accepts (subject : @tree raw_field) : bool :=
  match admission_worklist subject with
  | Some [summary] => summary_valid summary
  | _ => false
  end.

Theorem explicit_worklist_has_exact_admission_verdict :
  forall subject, worklist_accepts subject = reference_accepts subject.
Proof.
  intro subject. unfold worklist_accepts, reference_accepts.
  rewrite explicit_worklist_equals_independent_typed_reference. reflexivity.
Qed.

(** The concrete descriptor shapes below come from the live Rust-generated
    projection. Required semantic slots remain an explicit reducer premise:
    Send's channel is the modeled required slot. *)
Definition send_program : generated_program :=
  match lookup_projected_program 15 [] [0] with
  | Some program => program
  | None => empty_program
  end.

Definition expr_program : generated_program :=
  match lookup_projected_program 24 [] [] with
  | Some program => program
  | None => empty_program
  end.

Definition raw_par_program : generated_program :=
  project_generated_program GeneratedData.raw_par_program [] [].

Definition wrong_message_child_program : generated_program :=
  match lookup_projected_program 2 [] [] with
  | Some program => program
  | None => empty_program
  end.

Definition raw_root (program : generated_program) : raw_field :=
  RawField 0 LengthDelimited Message [] 0 (Some (program_family program))
    (program_wire_required program) (program_semantic_required program).

Definition raw_child (tag : nat) (wire : wire_kind) (payload : payload_kind) :
    @tree raw_field :=
  Node (RawField tag wire payload [] 0
    (match payload with Message => Some 1 | _ => None end) [] []) FNil.

Definition raw_par_child_with_fields (tag : nat)
    (children : @forest raw_field) : @tree raw_field :=
  Node (RawField tag LengthDelimited Message [] 0 (Some 1) [] []) children.

Definition valid_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (raw_child 1 LengthDelimited Message)
      (FCons (raw_child 2 LengthDelimited Message)
        (FCons (raw_child 2 LengthDelimited Message) FNil))).

Definition decreasing_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (raw_child 3 Varint Scalar)
      (FCons (raw_child 1 LengthDelimited Message) FNil)).

Definition duplicate_singular_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (raw_child 3 Varint Scalar)
      (FCons (raw_child 3 Varint Scalar) FNil)).

Definition duplicate_oneof_expr : @tree raw_field :=
  Node (raw_root expr_program)
    (FCons (raw_child 1 Varint Scalar)
      (FCons (raw_child 2 Varint Scalar) FNil)).

Definition wrong_wire_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (raw_child 1 Fixed32 Message) FNil).

Definition wrong_declared_length_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (Node (RawField 1 LengthDelimited Message [] 1 (Some 1) [] []) FNil) FNil).

Definition wrong_child_program_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (Node (RawField 1 LengthDelimited Message [] 0
      (Some (program_family wrong_message_child_program)) [] []) FNil) FNil).

Definition distinct_score_send : @tree raw_field :=
  Node (raw_root send_program)
    (FCons (raw_par_child_with_fields 1 FNil)
      (FCons (raw_par_child_with_fields 2
        (FCons (raw_child 9 LengthDelimited Scalar) FNil))
        (FCons (raw_par_child_with_fields 2
          (FCons (raw_child 9 LengthDelimited Scalar)
            (FCons (raw_child 10 Varint Scalar) FNil))) FNil))).

Example send_row_is_live_generated : program_family send_program = 15 /\
  program_slot_count send_program = 6 /\
  descriptor_slot (hd (FieldDescriptor 0 0 Varint false Scalar None None)
    (program_fields send_program)) = 0.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example send_generated_repeated_data_slot :
  match resolve_tag (program_fields send_program) 2 with
  | Some field => descriptor_slot field = 1 /\
      descriptor_repeated field = true /\
      descriptor_payload field = Message
  | None => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example send_generated_channel_names_raw_par_child :
  program_family raw_par_program = 1 /\
  program_family wrong_message_child_program = 2 /\
  match resolve_tag (program_fields send_program) 1 with
  | Some field => descriptor_child_type_id field = Some 1 /\
      descriptor_payload field = Message
  | None => False
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example expr_generated_oneof_shares_one_singular_slot :
  program_family expr_program = 24 /\
  (match resolve_tag (program_fields expr_program) 1,
    resolve_tag (program_fields expr_program) 2 with
   | Some lhs, Some rhs =>
       descriptor_slot lhs = descriptor_slot rhs /\
       descriptor_oneof lhs = Some 0 /\
       descriptor_oneof rhs = Some 0
   | _, _ => False
   end).
Proof. vm_compute. repeat split; reflexivity. Qed.

Example valid_repeated_send_is_accepted :
  reference_accepts valid_send = true /\ worklist_accepts valid_send = true /\
  summary_ordered_slots (typed_reference valid_send) = [0; 1; 1].
Proof. vm_compute. repeat split; reflexivity. Qed.

Example decreasing_slot_is_refused :
  reference_accepts decreasing_send = false /\
  worklist_accepts decreasing_send = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example duplicate_singular_slot_is_refused :
  reference_accepts duplicate_singular_send = false /\
  worklist_accepts duplicate_singular_send = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example distinct_oneof_arms_cannot_occupy_the_same_slot :
  reference_accepts duplicate_oneof_expr = false /\
  worklist_accepts duplicate_oneof_expr = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example wrong_wire_and_declared_length_are_refused :
  worklist_accepts wrong_wire_send = false /\
  worklist_accepts wrong_declared_length_send = false.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example wrong_generated_child_program_is_refused :
  summary_valid (typed_reference
    (Node (RawField 1 LengthDelimited Message [] 0
      (Some (program_family wrong_message_child_program)) [] []) FNil)) = true /\
  reference_accepts wrong_child_program_send = false /\
  worklist_outcome wrong_child_program_send = Refused.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example malformed_input_returns_refusal_not_an_incomplete_summary :
  worklist_outcome wrong_wire_send = Refused /\
  worklist_outcome wrong_declared_length_send = Refused /\
  worklist_outcome duplicate_oneof_expr = Refused.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example valid_score_plan_keeps_repeated_child_occurrence_order :
  summary_score (typed_reference distinct_score_send) =
    ScorePlan 15 true [ScorePlan 1 true [];
      ScorePlan 1 true [ScorePlan 0 true []];
      ScorePlan 1 true [ScorePlan 0 true []; ScorePlan 0 true []]].
Proof. vm_compute. repeat split; reflexivity. Qed.

Example ordered_child_observations_preserve_score_plans :
  map observed_tag (summary_children (typed_reference distinct_score_send)) = [1; 2; 2] /\
  map observed_score_plan (summary_children (typed_reference distinct_score_send)) =
    [ScorePlan 1 true [];
      ScorePlan 1 true [ScorePlan 0 true []];
      ScorePlan 1 true [ScorePlan 0 true []; ScorePlan 0 true []]].
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions explicit_worklist_equals_independent_typed_reference.
Print Assumptions explicit_worklist_preserves_modeled_summary_or_refusal.
Print Assumptions pending_frame_count_never_exceeds_remaining_budget.
Print Assumptions explicit_frames_finish_within_the_structural_budget.
Print Assumptions explicit_worklist_has_exact_admission_verdict.
Print Assumptions valid_repeated_send_is_accepted.
Print Assumptions decreasing_slot_is_refused.
Print Assumptions distinct_oneof_arms_cannot_occupy_the_same_slot.
Print Assumptions send_generated_channel_names_raw_par_child.
Print Assumptions wrong_generated_child_program_is_refused.
