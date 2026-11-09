(** * Correspondence of the numeric child readers on actual children

    [Target.exact_pbint32_or_absent] reads the semantic summary projected from
    an independent-oracle child as a protobuf [int32], and
    [Target.new_bind_count_word] reads the bind count of a [New] body as a
    zigzag-encoded [sint32].  [Process.source_i32_or_absent] reads the
    oracle's own typed value of the same child.  This theory compares these
    readers on the children the oracle's decoder and evaluator actually
    produce for a process family
    ([Correspondence.process_actual_children]).

    The i32 statement requested for this theory quantifies over every [tag].
    It is false: a varint field of a non-[int32] kind carrying the byte [1]
    is read as the value [1] by the target reader, while the oracle reads a
    non-[int32] scalar and refuses (a bool field carrying the byte [1], the
    example of the refutation below) or reads the [sint32] zigzag value [-1].
    The exported correspondence theorem therefore keeps the requested name
    and carries one extra premise on the tag, the contract predicate
    [i32_faithful_contract].  The refutation of the unrestricted statement is
    the closed theorem [process_actual_children_i32_unrestricted_is_false].
    The corollary [process_actual_children_i32_corresponds_at_reader_slots]
    shows that the premise holds at both slots the semantic reducer actually
    reads, [(19, 5)] and [(21, 3)], and [i32_premise_characterizes_field]
    shows that on every field of every process family the premise holds
    exactly when the correspondence holds for all actual children, which is
    the sense in which the restriction is minimal.

    The [sint32] reader of the [New] family needs no premise:
    [process_actual_children_new_word_corresponds] relates
    [Target.new_bind_count_word] to [Process.source_i32_or_absent] at tag [1]
    of family [20] unconditionally, because the only selected child that can
    reach it is the generated [sint32] scalar of that field. *)

From Stdlib Require Import List Bool NArith ZArith.
From StackSafePDA Require Import EPM1 RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleProjection RPB3IndependentOracleProcessProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge
  RPB3IndependentOracleProcessChildCorrespondence.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Target := RPB3SemanticReducer.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module ScalarBridge := RPB3IndependentOracleProcessScalarDecoderBridge.
Module Correspondence := RPB3IndependentOracleProcessChildCorrespondence.

(** ** The minimal restriction of the tag

    A contract is the protobuf kind and default policy that the generated
    table assigns to a tag.  This predicate enumerates exactly the contracts
    on which the i32 target reader agrees with the source reader: an [int32]
    whose default is forbidden.  Every other varint kind is rejected, because
    the target decodes any varint body as an [int32] while the oracle reads it
    through the declared kind (an encoded [1] is the value [1] to the target
    and a bool, a [u32], a [u64] or the [sint32] value [-1] to the oracle); a
    default-allowed [int32] is rejected because the target's default-forbidden
    rule refuses an encoded zero that the oracle accepts; and [sfixed32] is
    rejected because the oracle reads it as an [i32] while the target reader
    refuses the fixed wire.  Every remaining kind is faithful: its wire is not
    a varint and the oracle never reads it as an [i32], so both readers refuse.
    A tag without a contract is faithful, as for the bool readers: such a tag
    cannot select a scalar child, so a selected child is a message or an
    auxiliary field, on which both readers refuse, and an absent child reads
    [0] on both sides.  The two halves of "exactly" are the theorems
    [accepted_scalar_value_matches_i32] (agreement on every accepted shape)
    and [i32_faithful_kind_rejection_is_forced] (a disagreement on every
    rejected shape). *)
Definition i32_faithful_kind (kind : Data.protobuf_kind)
    (forbidden : bool) : bool :=
  match kind with
  | Data.PBInt32 => forbidden
  | Data.PBBool | Data.PBSint32 | Data.PBSfixed32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 => false
  | _ => true
  end.

Definition i32_faithful_contract
    (contract : option Admission.source_scalar_contract) : bool :=
  match contract with
  | None => true
  | Some selected =>
      i32_faithful_kind (Admission.source_contract_kind selected)
        (Admission.source_contract_default_forbidden selected)
  end.

(** ** The reader, one child at a time *)

Definition target_exact_i32_view (tag : nat)
    (child : Core.source_exact_summary) : option Z :=
  match Target.sem_wire (Projection.project_source_summary child),
      Target.sem_payload (Projection.project_source_summary child),
      Target.sem_definitely_refused (Projection.project_source_summary child) with
  | Schema.Varint, Schema.Scalar, false =>
      match Scalar.scalar_semantic_from_source
          (Scalar.ScalarRule tag Data.PBInt32 true)
          (Target.sem_body (Projection.project_source_summary child)) with
      | Some (Scalar.ScalarI32 value) => Some value
      | _ => None
      end
  | _, _, _ => None
  end.

Definition target_exact_i32_of_body (tag : nat) (wire : Schema.wire_kind)
    (body : list nat) : option Z :=
  match wire with
  | Schema.Varint =>
      match Scalar.scalar_semantic_from_source
          (Scalar.ScalarRule tag Data.PBInt32 true) body with
      | Some (Scalar.ScalarI32 value) => Some value
      | _ => None
      end
  | _ => None
  end.

Definition source_i32_of_scalar (scalar : Core.source_scalar_value)
    : option Z :=
  match scalar with
  | Core.SourceI32 value => Some value
  | _ => None
  end.

Definition source_i32_view (child : Core.source_exact_summary) : option Z :=
  if Core.source_summary_refused child then None else
  match Core.source_summary_value child with
  | Core.TypedSourceScalar _ (Core.SourceI32 value) => Some value
  | _ => None
  end.

Lemma exact_pbint32_or_absent_selected_child :
  forall tag children,
    Target.exact_pbint32_or_absent tag
      (ProcessProjection.projected_process_children children) =
    match Process.source_child_at tag children with
    | None => Some 0%Z
    | Some child => target_exact_i32_view tag child
    end.
Proof.
  intros tag children.
  unfold Target.exact_pbint32_or_absent.
  rewrite ChildProvenance.projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|]; reflexivity.
Qed.

Lemma source_i32_or_absent_selected_child :
  forall tag children,
    Process.source_i32_or_absent tag children =
    match Process.source_child_at tag children with
    | None => Some 0%Z
    | Some child => source_i32_view child
    end.
Proof. intros tag children. reflexivity. Qed.

Lemma target_exact_i32_view_of_scalar :
  forall tag child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Core.source_summary_refused child = false ->
    Schema.raw_payload raw = Schema.Scalar ->
    target_exact_i32_view tag child =
      target_exact_i32_of_body tag (Schema.raw_wire raw) (Schema.raw_body raw).
Proof.
  intros tag child raw scalar typed unrefused payload.
  unfold Core.source_summary_refused in unrefused.
  unfold target_exact_i32_view, target_exact_i32_of_body,
    Projection.project_source_summary, Core.source_summary_raw.
  cbn [Target.sem_wire Target.sem_payload Target.sem_definitely_refused
    Target.sem_body].
  rewrite typed, unrefused.
  cbn [Core.source_value_raw].
  rewrite payload.
  destruct (Schema.raw_wire raw); reflexivity.
Qed.

Lemma source_i32_view_of_scalar :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Core.source_summary_refused child = false ->
    source_i32_view child = source_i32_of_scalar scalar.
Proof.
  intros child raw scalar typed unrefused.
  unfold source_i32_view.
  rewrite unrefused, typed.
  destruct scalar; reflexivity.
Qed.

(** A child that is not a scalar reads as absent on both readers: the target
    reader tests the payload and the source reader tests the typed value. *)
Lemma non_scalar_child_i32_views_are_absent :
  forall child,
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Scalar = false ->
    Correspondence.value_is_scalar (Core.source_summary_value child) = false ->
    (forall tag, target_exact_i32_view tag child = None) /\
    source_i32_view child = None.
Proof.
  intros child not_scalar not_value.
  split.
  - intro tag.
    unfold target_exact_i32_view, Projection.project_source_summary.
    cbn [Target.sem_wire Target.sem_payload Target.sem_definitely_refused
      Target.sem_body].
    destruct (Schema.raw_payload (Core.source_summary_raw child));
      [discriminate not_scalar| | |];
      destruct (Schema.raw_wire (Core.source_summary_raw child)); reflexivity.
  - unfold source_i32_view.
    destruct (Core.source_summary_value child); try discriminate not_value.
    all: destruct (Core.source_summary_refused child); reflexivity.
Qed.

(** ** Per-kind agreement on an accepted scalar *)

(** Kinds whose decoder never produces [Core.SourceI32]. *)
Definition i32_free_kind (kind : Data.protobuf_kind) : bool :=
  match kind with
  | Data.PBFixed32 | Data.PBFloat
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes => true
  | _ => false
  end.

Lemma i32_free_kind_decode_is_not_i32 :
  forall kind forbidden body scalar,
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    i32_free_kind kind = true ->
    source_i32_of_scalar scalar = None.
Proof.
  intros kind forbidden body scalar decoded free.
  destruct kind; try discriminate free;
    cbv beta iota zeta delta
      [Core.independently_decode_scalar Core.source_fixed_scalar] in decoded;
    reduce_guards decoded;
    injection decoded as same_scalar; subst scalar; reflexivity.
Qed.

Lemma decoded_forbidden_pbint32_is_i32 :
  forall body scalar,
    Core.independently_decode_scalar Data.PBInt32 true body = Some scalar ->
    exists value, scalar = Core.SourceI32 value.
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar, Core.source_numeric_scalar,
    Core.source_numeric_nondefault in decoded.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate decoded].
  cbn [negb orb] in decoded.
  destruct (N.eqb number 0); [discriminate decoded|].
  cbn [negb] in decoded.
  destruct (N.leb number 2147483647 || N.leb 18446744071562067968 number);
    [|discriminate decoded].
  injection decoded as same_scalar.
  exists (Core.source_signed_64 number).
  symmetry; exact same_scalar.
Qed.

Lemma accepted_scalar_value_matches_i32 :
  forall kind forbidden wire tag body scalar,
    i32_faithful_kind kind forbidden = true ->
    Admission.source_scalar_wire kind = Some wire ->
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    target_exact_i32_of_body tag wire body = source_i32_of_scalar scalar.
Proof.
  intros kind forbidden wire tag body scalar faithful wire_found decoded.
  destruct kind; cbn [i32_faithful_kind] in faithful;
    try discriminate faithful; try discriminate wire_found;
    cbn [Admission.source_scalar_wire] in wire_found;
    injection wire_found as same_wire; subst wire;
    [ subst forbidden;
      destruct (decoded_forbidden_pbint32_is_i32 body scalar decoded)
        as [value same_scalar];
      subst scalar;
      unfold target_exact_i32_of_body;
      rewrite (proj1 (ScalarBridge.pbint32_scalar_decoder_equivalence
        tag body value) decoded);
      reflexivity
    | rewrite (i32_free_kind_decode_is_not_i32 _ forbidden body scalar
        decoded eq_refl);
      reflexivity .. ].
Qed.

(** Conversely, every contract shape the predicate rejects is genuinely
    unfaithful, whether or not a process family declares it: for each
    rejected pair [(kind, forbidden)] there is a body the oracle accepts on
    which the target reader of that field and the source reader disagree.
    Together with [accepted_scalar_value_matches_i32] this makes
    [i32_faithful_kind] an exact characterization at the level of contract
    shapes. *)
Theorem i32_faithful_kind_rejection_is_forced :
  forall kind forbidden tag,
    i32_faithful_kind kind forbidden = false ->
    exists wire body scalar,
      Admission.source_scalar_wire kind = Some wire /\
      Core.independently_decode_scalar kind forbidden body = Some scalar /\
      target_exact_i32_of_body tag wire body <> source_i32_of_scalar scalar.
Proof.
  intros kind forbidden tag rejected.
  destruct kind; cbn [i32_faithful_kind] in rejected;
    try discriminate rejected.
  - exists Schema.Varint, [1], (Core.SourceBool true).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - subst forbidden.
    exists Schema.Varint, [0], (Core.SourceI32 0%Z).
    split; [reflexivity|split].
    + vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Varint, [1], (Core.SourceI32 (-1)%Z).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Fixed32, [1; 0; 0; 0], (Core.SourceI32 1%Z).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Varint, [1], (Core.SourceU32 1%N).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Varint, [1], (Core.SourceI64 1%Z).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Varint, [1], (Core.SourceI64 (-1)%Z).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
  - exists Schema.Varint, [1], (Core.SourceU64 1%N).
    split; [reflexivity|split].
    + destruct forbidden; vm_compute; reflexivity.
    + intro same; vm_compute in same; discriminate same.
Qed.

(** ** The restricted correspondence

    The premise is the contract predicate above.  Since the contract is read
    from the generated table of the family, the premise is a finite,
    decidable condition on the pair [(family, tag)]. *)

Theorem process_actual_children_i32_corresponds :
  forall fuel hash_fn family program body children tag,
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    i32_faithful_contract
      (Admission.source_scalar_contract_for family tag) = true ->
    Target.exact_pbint32_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_i32_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag actual faithful.
  rewrite exact_pbint32_or_absent_selected_child,
    source_i32_or_absent_selected_child.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  change (target_exact_i32_view tag child = source_i32_view child).
  destruct (Correspondence.selected_child_has_source_node fuel hash_fn family
    program body children tag child actual selected)
    as [same_tag [same_family [node evaluated]]].
  destruct (Correspondence.evaluated_node_cases fuel hash_fn body program node
    child evaluated)
    as [[raw [scalar [typed [unrefused [payload accepted]]]]]
       |[not_scalar not_value]].
  - destruct (Correspondence.source_scalar_value_gate program raw scalar accepted)
      as [contract [wire [contract_found [wire_found [wire_same decoded]]]]].
    assert (raw_tag_is_selected : Schema.raw_tag raw = tag).
    { rewrite <- same_tag.
      unfold Core.source_summary_tag, Core.source_summary_raw.
      rewrite typed.
      reflexivity. }
    rewrite same_family, raw_tag_is_selected in contract_found.
    rewrite contract_found in faithful.
    cbn [i32_faithful_contract] in faithful.
    rewrite (target_exact_i32_view_of_scalar tag child raw scalar typed
      unrefused payload).
    rewrite wire_same.
    rewrite (source_i32_view_of_scalar child raw scalar typed unrefused).
    exact (accepted_scalar_value_matches_i32 _ _ wire tag _ scalar
      faithful wire_found decoded).
  - destruct (non_scalar_child_i32_views_are_absent child not_scalar not_value)
      as [i32_view source_view].
    rewrite (i32_view tag), source_view.
    reflexivity.
Qed.

(** ** The premise holds at both slots the reducer reads

    The semantic reducer of the process families reads
    [exact_pbint32_or_absent] at Receive tag 5 ([bind_count]) and at MatchCase
    tag 3 ([free_count]).  Each of those slots is a default-forbidden [int32]. *)
Definition i32_reader_slots : list (nat * nat) := [(19, 5); (21, 3)].

Lemma i32_reader_slots_are_forbidden_int32s :
  forall family tag,
    In (family, tag) i32_reader_slots ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBInt32 true).
Proof.
  intros family tag member.
  unfold i32_reader_slots in member.
  destruct member as [same|[same|[]]];
    inversion same; subst family tag;
    vm_compute; reflexivity.
Qed.

Theorem process_actual_children_i32_corresponds_at_reader_slots :
  forall fuel hash_fn family program body children tag,
    In (family, tag) i32_reader_slots ->
    Correspondence.process_actual_children fuel hash_fn family program body
      children ->
    Target.exact_pbint32_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_i32_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag slot actual.
  apply (process_actual_children_i32_corresponds fuel hash_fn family
    program body children tag actual).
  rewrite (i32_reader_slots_are_forbidden_int32s family tag slot).
  reflexivity.
Qed.

(** ** The closed refutation of the unrestricted statement

    The witness is the family's generated decoder and evaluator run on a
    concrete body, so [Correspondence.process_actual_children] holds of it by
    computation, as for the bool refutations.  Field 3 of [Send] is
    [persistent], a default-forbidden [bool].  The body is the key byte
    [(3 << 3) | 0 = 24] and the varint [1].  The target reader sees the
    varint value [1]; the oracle reads the bool [true], so it has no [i32] at
    all. *)
Definition send_persistent_body : list nat := [24; 1].

Example send_persistent_contract :
  Admission.source_scalar_contract_for 15 3 =
    Some (Admission.SourceScalarContract Data.PBBool true).
Proof. vm_compute. reflexivity. Qed.

Lemma send_persistent_witness_is_actual :
  Correspondence.process_actual_children 1 Correspondence.witness_hash_fn 15
    (Correspondence.witness_program 15) send_persistent_body
    (Correspondence.witness_children 15 send_persistent_body).
Proof.
  apply Correspondence.witness_is_actual.
  - exact (or_intror (or_introl eq_refl)).
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
Qed.

Example send_persistent_witness_value :
  map Core.source_summary_value
      (Correspondence.witness_children 15 send_persistent_body) =
    [Core.TypedSourceScalar
      (Schema.RawField 3 Schema.Varint Schema.Scalar [1] 1 None [] [])
      (Core.SourceBool true)].
Proof. vm_compute. reflexivity. Qed.

Theorem process_actual_children_i32_unrestricted_is_false :
  ~ (forall fuel hash_fn family program body children tag,
      Correspondence.process_actual_children fuel hash_fn family program body
        children ->
      Target.exact_pbint32_or_absent tag
        (ProcessProjection.projected_process_children children) =
      Process.source_i32_or_absent tag children).
Proof.
  intro unrestricted.
  pose proof (unrestricted 1 Correspondence.witness_hash_fn 15
    (Correspondence.witness_program 15) send_persistent_body
    (Correspondence.witness_children 15 send_persistent_body) 3
    send_persistent_witness_is_actual) as disagreement.
  vm_compute in disagreement.
  discriminate disagreement.
Qed.

(** Field 1 of [New] is the [sint32] [bind_count]: on the same actual witness
    as the bool refutations the target reader sees the varint value [1], while
    the oracle reads its zigzag value [-1].  This is the disagreement the
    unconditional [New] theorem at the end of this file resolves, by reading
    the word through the zigzag decoding on both sides. *)
Theorem new_bind_count_exposes_i32_reader_mismatch :
  Target.exact_pbint32_or_absent 1
    (ProcessProjection.projected_process_children
      (Correspondence.witness_children 20
        Correspondence.new_bind_count_body)) = Some 1%Z /\
  Process.source_i32_or_absent 1
    (Correspondence.witness_children 20
      Correspondence.new_bind_count_body) = Some (-1)%Z.
Proof. split; vm_compute; reflexivity. Qed.

(** ** The premise is exact on the fields of the process families

    The finite certificate below covers every field that the generated
    program of a process family declares, in the style of the bool necessity
    tables of [RPB3IndependentOracleProcessChildCorrespondence]: for each
    field whose contract the premise rejects, the check computes the body
    consisting of the field key and the varint [1], runs the decoder and
    evaluator on it, and checks that the witness is an actual-children
    instance on which the target reader and the source reader disagree.
    Together with the restricted correspondence theorem this makes the
    premise an exact characterization on those fields, which is the sense in
    which the restriction is minimal. *)

Definition i32_witness_body_for (family tag : nat) : list nat :=
  match Admission.source_scalar_contract_for family tag with
  | Some contract =>
      match Admission.source_scalar_wire
          (Admission.source_contract_kind contract) with
      | Some Schema.Varint => EPM1.encode_varint (tag * 8) ++ [1]
      | _ => []
      end
  | None => []
  end.

Definition option_i32_eqb (left right : option Z) : bool :=
  match left, right with
  | Some first, Some second => Z.eqb first second
  | None, None => true
  | _, _ => false
  end.

Lemma option_i32_eqb_of_equal :
  forall left right, left = right -> option_i32_eqb left right = true.
Proof.
  intros left right same.
  subst right.
  destruct left as [value|];
    [exact (proj2 (Z.eqb_eq value value) eq_refl)|reflexivity].
Qed.

Definition i32_necessity_table : bool :=
  forallb (fun family =>
    forallb (fun tag =>
      i32_faithful_contract
        (Admission.source_scalar_contract_for family tag) ||
      (Correspondence.actual_check family (i32_witness_body_for family tag) &&
       negb (option_i32_eqb
         (Target.exact_pbint32_or_absent tag
           (ProcessProjection.projected_process_children
             (Correspondence.witness_children family
               (i32_witness_body_for family tag))))
         (Process.source_i32_or_absent tag
           (Correspondence.witness_children family
             (i32_witness_body_for family tag))))))
      (Correspondence.process_field_tags family))
    Correspondence.process_families.

Lemma i32_necessity_table_is_true :
  i32_necessity_table = true.
Proof. vm_compute. reflexivity. Qed.

Theorem i32_premise_is_necessary_on_process_fields :
  forall family tag,
    In family Correspondence.process_families ->
    In tag (Correspondence.process_field_tags family) ->
    i32_faithful_contract
      (Admission.source_scalar_contract_for family tag) = false ->
    exists body,
      Correspondence.process_actual_children 1 Correspondence.witness_hash_fn
        family (Correspondence.witness_program family)
        body (Correspondence.witness_children family body) /\
      Target.exact_pbint32_or_absent tag
        (ProcessProjection.projected_process_children
          (Correspondence.witness_children family body)) <>
      Process.source_i32_or_absent tag
        (Correspondence.witness_children family body).
Proof.
  intros family tag in_family in_tags unfaithful.
  pose proof i32_necessity_table_is_true as table.
  unfold i32_necessity_table in table.
  rewrite forallb_forall in table.
  specialize (table family in_family).
  cbv beta in table.
  rewrite forallb_forall in table.
  specialize (table tag in_tags).
  cbv beta in table.
  rewrite unfaithful in table.
  cbn [orb] in table.
  apply andb_true_iff in table as [actual disagreement].
  exists (i32_witness_body_for family tag).
  split.
  - exact (Correspondence.actual_check_sound family
      (i32_witness_body_for family tag) actual).
  - intro same.
    rewrite (option_i32_eqb_of_equal _ _ same) in disagreement.
    discriminate disagreement.
Qed.

(** The exact form of minimality: on a field of a process family, the premise
    holds if and only if the correspondence holds for all actual children. *)
Theorem i32_premise_characterizes_field :
  forall family tag,
    In family Correspondence.process_families ->
    In tag (Correspondence.process_field_tags family) ->
    (i32_faithful_contract
       (Admission.source_scalar_contract_for family tag) = true <->
     (forall fuel hash_fn program body children,
        Correspondence.process_actual_children fuel hash_fn family program body
          children ->
        Target.exact_pbint32_or_absent tag
          (ProcessProjection.projected_process_children children) =
        Process.source_i32_or_absent tag children)).
Proof.
  intros family tag in_family in_tags.
  split.
  - intros faithful fuel hash_fn program body children actual.
    exact (process_actual_children_i32_corresponds fuel hash_fn family
      program body children tag actual faithful).
  - intro universal.
    destruct (i32_faithful_contract
        (Admission.source_scalar_contract_for family tag)) eqn:faithful;
      [reflexivity|].
    destruct (i32_premise_is_necessary_on_process_fields family tag
      in_family in_tags faithful) as [body [actual disagreement]].
    exact (False_ind _ (disagreement (universal _ _ _ _ _ actual))).
Qed.

(** ** The bind-count word of a [New] body

    The target reader of [New] reads tag [1] from the body alone: it decodes
    the varint, bounds it by [2^32 - 1], and applies the zigzag map to the
    full 64-bit word, without checking the wire or the payload.  The oracle
    reads the child's typed scalar.  Because the only child that can carry tag
    [1] in family [20] is the generated [sint32] scalar of that field, the two
    agree on every actual child, without any premise on the tag.  A non-scalar
    child at that tag is excluded by the ordered scan: the scan matches every
    accepted child against the descriptor of its tag, and the descriptor of
    tag [1] in family [20] has scalar payload. *)

Definition target_new_word_of_body (body : list nat) : option N :=
  match Scalar.compact_varint_value body with
  | Some encoded =>
      if N.leb encoded 4294967295 then
        Some (Target.signed_score_word (Scalar.zigzag_signed encoded))
      else None
  | None => None
  end.

Definition target_new_word_view (child : Core.source_exact_summary)
    : option N :=
  target_new_word_of_body
    (Target.sem_body (Projection.project_source_summary child)).

Lemma new_bind_count_word_selected_child :
  forall children,
    Target.new_bind_count_word
      (ProcessProjection.projected_process_children children) =
    match Process.source_child_at 1 children with
    | None => Some 0%N
    | Some child => target_new_word_view child
    end.
Proof.
  intro children.
  unfold Target.new_bind_count_word.
  rewrite (ChildProvenance.projected_process_child_at 1 children).
  destruct (Process.source_child_at 1 children) as [child|]; reflexivity.
Qed.

Lemma source_new_word_selected_child :
  forall children,
    option_map Process.source_signed_word
      (Process.source_i32_or_absent 1 children) =
    match Process.source_child_at 1 children with
    | None => Some 0%N
    | Some child =>
        option_map Process.source_signed_word (source_i32_view child)
    end.
Proof.
  intro children.
  rewrite source_i32_or_absent_selected_child.
  destruct (Process.source_child_at 1 children) as [child|]; reflexivity.
Qed.

Lemma target_new_word_view_of_scalar :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    target_new_word_view child = target_new_word_of_body (Schema.raw_body raw).
Proof.
  intros child raw scalar typed.
  unfold target_new_word_view, Projection.project_source_summary,
    Core.source_summary_raw.
  cbn [Target.sem_body].
  rewrite typed.
  reflexivity.
Qed.

Lemma decoded_new_sint32_word_matches :
  forall body scalar,
    Core.independently_decode_scalar Data.PBSint32 true body = Some scalar ->
    target_new_word_of_body body =
      option_map Process.source_signed_word (source_i32_of_scalar scalar).
Proof.
  intros body scalar decoded.
  unfold target_new_word_of_body.
  rewrite <- ScalarBridge.full_u64_decoder_agreement.
  unfold Core.independently_decode_scalar, Core.source_numeric_scalar,
    Core.source_numeric_nondefault in decoded.
  destruct (Core.independently_decoded_u64 body) as [number|];
    [|discriminate decoded].
  cbn [negb orb] in decoded.
  destruct (N.eqb number 0); [discriminate decoded|].
  cbn [negb] in decoded.
  destruct (N.leb number 4294967295); [|discriminate decoded].
  injection decoded as same_scalar.
  subst scalar.
  reflexivity.
Qed.

(** One accepted scan step certifies the descriptor of the child's tag. *)
Lemma valid_source_scan_step_has_matching_descriptor :
  forall program state child,
    Admission.source_scan_valid
      (Admission.source_scan_step program state child) = true ->
    exists descriptor,
      Schema.resolve_tag (Schema.program_fields program)
        (Core.source_summary_tag child) = Some descriptor /\
      Admission.source_child_matches_descriptor descriptor child = true.
Proof.
  intros program state child valid.
  assert (state_valid : Admission.source_scan_valid state = true).
  { destruct (Admission.source_scan_valid state) eqn:observed;
      [reflexivity|].
    rewrite (Admission.source_scan_step_invalid_sticky
      program state child observed) in valid.
    rewrite observed in valid.
    discriminate valid. }
  unfold Admission.source_scan_step in valid.
  rewrite state_valid in valid.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Core.source_summary_tag child)) as [descriptor|] eqn:resolved;
    [|discriminate valid].
  exists descriptor.
  split; [reflexivity|].
  destruct (Admission.source_child_matches_descriptor descriptor child)
    eqn:matches; [reflexivity|].
  discriminate valid.
Qed.

Lemma valid_source_scan_children_match_descriptors :
  forall program children state,
    Admission.source_scan_valid
      (fold_left (Admission.source_scan_step program) children state) = true ->
    Forall (fun child =>
      exists descriptor,
        Schema.resolve_tag (Schema.program_fields program)
          (Core.source_summary_tag child) = Some descriptor /\
        Admission.source_child_matches_descriptor descriptor child = true)
      children.
Proof.
  intros program children.
  induction children as [|child rest IH]; intros state valid;
    [constructor|].
  simpl in valid.
  set (next := Admission.source_scan_step program state child) in *.
  assert (next_valid : Admission.source_scan_valid next = true).
  { destruct (Admission.source_scan_valid next) eqn:observed;
      [reflexivity|].
    pose proof (ChildProvenance.invalid_source_scan_stays_invalid
      program rest next observed) as impossible.
    rewrite valid in impossible.
    discriminate impossible. }
  constructor.
  - exact (valid_source_scan_step_has_matching_descriptor program state child
      next_valid).
  - exact (IH next valid).
Qed.

(** Tag [1] of family [20] is a scalar field. *)
Lemma new_bind_count_descriptor_is_scalar :
  forall program descriptor,
    Schema.lookup_projected_program 20 [] [] = Some program ->
    Schema.resolve_tag (Schema.program_fields program) 1 = Some descriptor ->
    Schema.descriptor_payload descriptor = Schema.Scalar.
Proof.
  intros program descriptor looked_up resolved.
  vm_compute in looked_up.
  injection looked_up as same_program.
  subst program.
  vm_compute in resolved.
  injection resolved as same_descriptor.
  subst descriptor.
  reflexivity.
Qed.

Theorem process_actual_children_new_word_corresponds :
  forall fuel hash_fn program body children,
    Correspondence.process_actual_children fuel hash_fn 20 program body
      children ->
    Target.new_bind_count_word
      (ProcessProjection.projected_process_children children) =
    option_map Process.source_signed_word
      (Process.source_i32_or_absent 1 children).
Proof.
  intros fuel hash_fn program body children actual.
  rewrite new_bind_count_word_selected_child, source_new_word_selected_child.
  destruct (Process.source_child_at 1 children) as [child|] eqn:selected;
    [|reflexivity].
  change (target_new_word_view child =
    option_map Process.source_signed_word (source_i32_view child)).
  destruct (Correspondence.selected_child_has_source_node fuel hash_fn 20
    program body children 1 child actual selected)
    as [same_tag [same_family [node evaluated]]].
  destruct (Correspondence.evaluated_node_cases fuel hash_fn body program node
    child evaluated)
    as [[raw [scalar [typed [unrefused [payload accepted]]]]]
       |[not_scalar not_value]].
  - destruct (Correspondence.source_scalar_value_gate program raw scalar accepted)
      as [contract [wire [contract_found [wire_found [wire_same decoded]]]]].
    assert (raw_tag_is_selected : Schema.raw_tag raw = 1).
    { rewrite <- same_tag.
      unfold Core.source_summary_tag, Core.source_summary_raw.
      rewrite typed.
      reflexivity. }
    rewrite same_family, raw_tag_is_selected,
      Correspondence.new_bind_count_contract in contract_found.
    injection contract_found as same_contract.
    subst contract.
    cbn [Admission.source_contract_kind
      Admission.source_contract_default_forbidden] in decoded.
    rewrite (source_i32_view_of_scalar child raw scalar typed unrefused).
    rewrite (target_new_word_view_of_scalar child raw scalar typed).
    exact (decoded_new_sint32_word_matches (Schema.raw_body raw) scalar
      decoded).
  - exfalso.
    unfold Correspondence.process_actual_children in actual.
    destruct actual as [_ [looked_up [scan _]]].
    unfold Process.source_child_at in selected.
    apply find_some in selected as [member _].
    pose proof (valid_source_scan_children_match_descriptors program children
      Admission.source_empty_scan scan) as all_match.
    destruct (proj1 (Forall_forall _ _) all_match child member)
      as [descriptor [resolved matches]].
    rewrite same_tag in resolved.
    pose proof (new_bind_count_descriptor_is_scalar program descriptor
      looked_up resolved) as scalar_descriptor.
    unfold Admission.source_child_matches_descriptor in matches.
    cbv zeta in matches.
    apply andb_true_iff in matches as [matches _].
    apply andb_true_iff in matches as [_ payload_matches].
    rewrite scalar_descriptor in payload_matches.
    rewrite payload_matches in not_scalar.
    discriminate not_scalar.
Qed.

Print Assumptions exact_pbint32_or_absent_selected_child.
Print Assumptions source_i32_or_absent_selected_child.
Print Assumptions target_exact_i32_view_of_scalar.
Print Assumptions source_i32_view_of_scalar.
Print Assumptions non_scalar_child_i32_views_are_absent.
Print Assumptions i32_free_kind_decode_is_not_i32.
Print Assumptions decoded_forbidden_pbint32_is_i32.
Print Assumptions accepted_scalar_value_matches_i32.
Print Assumptions i32_faithful_kind_rejection_is_forced.
Print Assumptions process_actual_children_i32_corresponds.
Print Assumptions i32_reader_slots_are_forbidden_int32s.
Print Assumptions process_actual_children_i32_corresponds_at_reader_slots.
Print Assumptions send_persistent_contract.
Print Assumptions send_persistent_witness_is_actual.
Print Assumptions send_persistent_witness_value.
Print Assumptions process_actual_children_i32_unrestricted_is_false.
Print Assumptions new_bind_count_exposes_i32_reader_mismatch.
Print Assumptions option_i32_eqb_of_equal.
Print Assumptions i32_necessity_table_is_true.
Print Assumptions i32_premise_is_necessary_on_process_fields.
Print Assumptions i32_premise_characterizes_field.
Print Assumptions new_bind_count_word_selected_child.
Print Assumptions source_new_word_selected_child.
Print Assumptions target_new_word_view_of_scalar.
Print Assumptions decoded_new_sint32_word_matches.
Print Assumptions valid_source_scan_step_has_matching_descriptor.
Print Assumptions valid_source_scan_children_match_descriptors.
Print Assumptions new_bind_count_descriptor_is_scalar.
Print Assumptions process_actual_children_new_word_corresponds.
