From Stdlib Require Import List Bool Arith.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3IndependentOracleCore.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Core := RPB3IndependentOracleCore.

(** Read the scalar contract from the generated program rather than from the
    scalar admission reducer.  An arm has its own protobuf kind; a standalone
    slot also carries the default policy. *)
Record source_scalar_contract := SourceScalarContract {
  source_contract_kind : Data.protobuf_kind;
  source_contract_default_forbidden : bool
}.

Definition source_contract_in_arm (arm : Data.projection_arm)
    : option source_scalar_contract :=
  match Data.arm_wire arm, Schema.projected_arm_payload arm with
  | Some _, Schema.Scalar =>
      Some (SourceScalarContract (Data.arm_protobuf_kind arm) false)
  | _, _ => None
  end.

Definition source_contract_in_slot (slot : Data.projection_slot)
    (tag : nat) : option source_scalar_contract :=
  match Data.slot_arms slot with
  | [] =>
      if Data.slot_min_tag slot =? tag then
        match Data.slot_wire slot, Schema.projected_slot_payload slot with
        | Some _, Schema.Scalar =>
            Some (SourceScalarContract (Data.slot_protobuf_kind slot)
              (match Data.slot_default slot with
               | Data.DefaultForbidden => true
               | _ => false
               end))
        | _, _ => None
        end
      else None
  | arms =>
      match find (fun arm => Data.arm_tag arm =? tag) arms with
      | Some arm => source_contract_in_arm arm
      | None => None
      end
  end.

Fixpoint source_contract_in_slots (slots : list Data.projection_slot)
    (tag : nat) : option source_scalar_contract :=
  match slots with
  | [] => None
  | slot :: rest =>
      match source_contract_in_slot slot tag with
      | Some contract => Some contract
      | None => source_contract_in_slots rest tag
      end
  end.

Definition source_scalar_contract_for (type_id tag : nat)
    : option source_scalar_contract :=
  match find (fun row => Data.program_type_id row =? type_id)
      Data.generated_programs with
  | Some row => source_contract_in_slots (Data.program_slots row) tag
  | None => None
  end.

Definition source_scalar_wire (kind : Data.protobuf_kind)
    : option Schema.wire_kind :=
  match kind with
  | Data.PBBool | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 => Some Schema.Varint
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat => Some Schema.Fixed32
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble => Some Schema.Fixed64
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes => Some Schema.LengthDelimited
  | _ => None
  end.

Definition source_raw_bytes_well_formed (raw : Schema.raw_field) : bool :=
  (Schema.raw_declared_length raw =? length (Schema.raw_body raw)) &&
    Core.bytes_in_range (Schema.raw_body raw).

Definition source_scalar_value_of_raw (program : Schema.generated_program)
    (raw : Schema.raw_field) : option Core.source_scalar_value :=
  match Schema.raw_payload raw, Schema.raw_program_type_id raw,
      Schema.resolve_tag (Schema.program_fields program)
        (Schema.raw_tag raw),
      source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) with
  | Schema.Scalar, None, Some descriptor, Some contract =>
      match source_scalar_wire (source_contract_kind contract) with
      | Some wire =>
          if source_raw_bytes_well_formed raw &&
              Schema.wire_eqb (Schema.raw_wire raw) wire &&
              Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
              Schema.payload_eqb (Schema.descriptor_payload descriptor)
                Schema.Scalar
          then Core.independently_decode_scalar
            (source_contract_kind contract)
            (source_contract_default_forbidden contract)
            (Schema.raw_body raw)
          else None
      | None => None
      end
  | _, _, _, _ => None
  end.

Definition source_typed_scalar (program : Schema.generated_program)
    (raw : Schema.raw_field) : option Core.typed_source_value :=
  option_map (Core.TypedSourceScalar raw)
    (source_scalar_value_of_raw program raw).

(** This is a source-owned ordered-slot scan.  It consumes the summaries that
    the independent recursive evaluator will construct, including its own
    refusal bit, and does not call the semantic reducer's scan. *)
Record source_admission_state := SourceAdmissionState {
  source_scan_valid : bool;
  source_scan_last_slot : option nat;
  source_scan_seen_slots : list nat
}.

Definition source_empty_scan : source_admission_state :=
  SourceAdmissionState true None [].

Definition source_slot_follows (last : option nat) (slot : nat)
    (repeated : bool) : bool :=
  match last with
  | None => true
  | Some previous =>
      (previous <? slot) || ((previous =? slot) && repeated)
  end.

Definition source_child_matches_descriptor
    (descriptor : Schema.field_descriptor)
    (child : Core.source_exact_summary) : bool :=
  let raw := Core.source_summary_raw child in
  negb (Core.source_summary_refused child) &&
    source_raw_bytes_well_formed raw &&
    Schema.wire_eqb (Schema.raw_wire raw)
      (Schema.descriptor_wire descriptor) &&
    Schema.payload_eqb (Schema.raw_payload raw)
      (Schema.descriptor_payload descriptor) &&
    match Schema.descriptor_payload descriptor with
    | Schema.Message =>
        match Schema.descriptor_child_type_id descriptor,
            Schema.raw_program_type_id raw with
        | Some expected, Some observed => expected =? observed
        | _, _ => false
        end
    | _ => true
    end.

Definition source_scan_step (program : Schema.generated_program)
    (state : source_admission_state)
    (child : Core.source_exact_summary) : source_admission_state :=
  if source_scan_valid state then
    match Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child) with
    | None => SourceAdmissionState false
        (source_scan_last_slot state) (source_scan_seen_slots state)
    | Some descriptor =>
        let slot := Schema.descriptor_slot descriptor in
        if source_child_matches_descriptor descriptor child &&
            (slot <? Schema.program_slot_count program) &&
            source_slot_follows (source_scan_last_slot state) slot
              (Schema.descriptor_repeated descriptor) &&
            (Schema.descriptor_repeated descriptor ||
              negb (existsb (Nat.eqb slot) (source_scan_seen_slots state)))
        then SourceAdmissionState true (Some slot)
          (slot :: source_scan_seen_slots state)
        else SourceAdmissionState false
          (source_scan_last_slot state) (source_scan_seen_slots state)
    end
  else state.

Definition source_scan_children (program : Schema.generated_program)
    (children : list Core.source_exact_summary) : source_admission_state :=
  fold_left (source_scan_step program) children source_empty_scan.

Definition source_seen_slot (state : source_admission_state)
    (slot : nat) : bool :=
  existsb (Nat.eqb slot) (source_scan_seen_slots state).

Definition source_required_pathmaps_present
    (program : Schema.generated_program)
    (state : source_admission_state) : bool :=
  forallb (fun descriptor =>
    match Schema.descriptor_payload descriptor with
    | Schema.PathMap => source_seen_slot state
        (Schema.descriptor_slot descriptor)
    | _ => true
    end) (Schema.program_fields program).

Lemma source_scan_step_invalid_sticky :
  forall program state child,
    source_scan_valid state = false ->
    source_scan_step program state child = state.
Proof.
  intros program state child invalid.
  unfold source_scan_step. now rewrite invalid.
Qed.

Lemma source_scan_step_checks_prior_refusal :
  forall program state child,
    Core.source_summary_refused child = true ->
    source_scan_valid (source_scan_step program state child) = false \/
    source_scan_valid state = false.
Proof.
  intros program state child refused.
  unfold source_scan_step.
  destruct (source_scan_valid state) eqn:valid; [|now right].
  left.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|]; [|reflexivity].
  unfold source_child_matches_descriptor.
  rewrite refused. reflexivity.
Qed.

Print Assumptions source_scan_step_invalid_sticky.
Print Assumptions source_scan_step_checks_prior_refusal.
