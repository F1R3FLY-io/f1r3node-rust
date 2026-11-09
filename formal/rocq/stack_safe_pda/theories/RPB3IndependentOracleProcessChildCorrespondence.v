(** * Correspondence of the two boolean child readers on actual children

    [Target.scalar_true_or_absent] and [Target.exact_bool_or_absent] read the
    semantic summary projected from an independent-oracle child.
    [Process.source_bool_or_absent] reads the oracle's own typed value of the
    same child.  This theory compares the three readers on the children the
    oracle's decoder and evaluator actually produce for a process family
    ([process_actual_children]).

    The statements requested for this theory quantify over every [tag].
    They are false.  A varint field of any non-bool protobuf kind carrying the
    byte [1] is indistinguishable, at both target readers, from an encoded
    [true], while the oracle reads a non-bool scalar; and a default-allowed
    bool (a oneof arm) can carry an encoded zero that the default-forbidden
    target rule rejects.  The exported correspondence theorems therefore keep
    the requested names and carry one extra premise on the tag, the contract
    predicates [scalar_true_faithful_contract] and
    [exact_bool_faithful_contract].  The refutations of the unrestricted
    statements are the closed theorems
    [process_actual_children_scalar_true_unrestricted_is_false] and
    [process_actual_children_bool_unrestricted_is_false].  Corollaries show
    that the premise holds at every slot that a process family of the semantic
    reducer actually reads, and [scalar_true_premise_characterizes_field] and
    [exact_bool_premise_characterizes_field] show that on every field of every
    process family the premise holds exactly when the correspondence holds
    for all actual children, which is the sense in which the restriction is
    minimal. *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import EPM1 RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3SourceScalarReduction RPB3IndependentOracleCore
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleProcessProjection RPB3IndependentOracleRecursive
  RPB3IndependentOracleProjection RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3IndependentOracleProcessScalarDecoderBridge.

Import ListNotations.

Module Data := RPB3DescriptorProjectionData.GeneratedData.
Module Schema := RPB3GeneratedFieldRefinement.
Module Source := RPB3RawRegionTree.
Module Target := RPB3SemanticReducer.
Module Scalar := RPB3SourceScalarReduction.
Module Core := RPB3IndependentOracleCore.
Module Admission := RPB3IndependentOracleAdmission.
Module Process := RPB3IndependentOracleProcess.
Module Projection := RPB3IndependentOracleProjection.
Module ProcessProjection := RPB3IndependentOracleProcessProjection.
Module Oracle := RPB3IndependentOracleRecursive.
Module ChildProvenance := RPB3IndependentOracleProcessChildProvenance.
Module ScalarBridge := RPB3IndependentOracleProcessScalarDecoderBridge.

(** The children that the oracle evaluator produces for one body of a
    process family: the family's generated program decodes the body, the
    evaluator reads every decoded node, and the source-owned ordered scan
    accepts the result. *)
Definition process_actual_children
    (fuel : nat) (hash_fn : list nat -> nat) (family : nat)
    (program : Schema.generated_program) (body : list nat)
    (children : list Core.source_exact_summary) : Prop :=
  In family [8; 15; 16; 19; 20; 21; 22; 23; 24] /\
  Schema.lookup_projected_program family [] [] = Some program /\
  Admission.source_scan_valid
    (Admission.source_scan_children program children) = true /\
  exists nodes,
    Source.decode_region (S (length body)) body program 0 (length body) = Some nodes /\
    Source.sequence_source_results
      (map (Oracle.source_evaluate_node
             (Oracle.source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children.

(** ** The minimal restriction of the tag

    A contract is the protobuf kind and default policy that the generated
    table assigns to a tag.  These predicates enumerate exactly the contracts
    on which the corresponding reader agrees with the source reader.  A tag
    without a contract is faithful: such a tag cannot select a scalar child,
    so a selected child is a message or an auxiliary field, on which both
    readers refuse, and an absent child reads [false] on both sides. *)
Definition scalar_true_faithful_kind (kind : Data.protobuf_kind)
    (forbidden : bool) : bool :=
  match kind with
  | Data.PBBool => forbidden
  | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 => false
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes => false
  | _ => true
  end.

Definition exact_bool_faithful_kind (kind : Data.protobuf_kind)
    (forbidden : bool) : bool :=
  match kind with
  | Data.PBBool => forbidden
  | Data.PBInt32 | Data.PBSint32 | Data.PBUint32
  | Data.PBInt64 | Data.PBSint64 | Data.PBUint64 => false
  | _ => true
  end.

Definition scalar_true_faithful_contract
    (contract : option Admission.source_scalar_contract) : bool :=
  match contract with
  | None => true
  | Some selected =>
      scalar_true_faithful_kind (Admission.source_contract_kind selected)
        (Admission.source_contract_default_forbidden selected)
  end.

Definition exact_bool_faithful_contract
    (contract : option Admission.source_scalar_contract) : bool :=
  match contract with
  | None => true
  | Some selected =>
      exact_bool_faithful_kind (Admission.source_contract_kind selected)
        (Admission.source_contract_default_forbidden selected)
  end.

(** ** The readers, one child at a time *)

Definition target_body_is_true (body : list nat) : option bool :=
  match body with
  | [1] => Some true
  | _ => None
  end.

Definition target_scalar_true_view (child : Core.source_exact_summary)
    : option bool :=
  match Target.sem_payload (Projection.project_source_summary child),
      Target.sem_body (Projection.project_source_summary child) with
  | Schema.Scalar, [1] => Some true
  | _, _ => None
  end.

Definition target_exact_bool_view (tag : nat)
    (child : Core.source_exact_summary) : option bool :=
  match Target.sem_wire (Projection.project_source_summary child),
      Target.sem_payload (Projection.project_source_summary child),
      Target.sem_definitely_refused (Projection.project_source_summary child) with
  | Schema.Varint, Schema.Scalar, false =>
      match Scalar.scalar_semantic_from_source
          (Scalar.ScalarRule tag Data.PBBool true)
          (Target.sem_body (Projection.project_source_summary child)) with
      | Some (Scalar.ScalarBool value) => Some value
      | _ => None
      end
  | _, _, _ => None
  end.

Definition target_exact_bool_of_body (tag : nat) (wire : Schema.wire_kind)
    (body : list nat) : option bool :=
  match wire with
  | Schema.Varint =>
      match Scalar.scalar_semantic_from_source
          (Scalar.ScalarRule tag Data.PBBool true) body with
      | Some (Scalar.ScalarBool value) => Some value
      | _ => None
      end
  | _ => None
  end.

Definition source_bool_of_scalar (scalar : Core.source_scalar_value)
    : option bool :=
  match scalar with
  | Core.SourceBool value => Some value
  | _ => None
  end.

Definition source_bool_view (child : Core.source_exact_summary)
    : option bool :=
  if Core.source_summary_refused child then None else
  match Core.source_summary_value child with
  | Core.TypedSourceScalar _ (Core.SourceBool value) => Some value
  | _ => None
  end.

Definition value_is_scalar (value : Core.typed_source_value) : bool :=
  match value with
  | Core.TypedSourceScalar _ _ => true
  | _ => false
  end.

Lemma scalar_true_or_absent_selected_child :
  forall tag children,
    Target.scalar_true_or_absent tag
      (ProcessProjection.projected_process_children children) =
    match Process.source_child_at tag children with
    | None => Some false
    | Some child => target_scalar_true_view child
    end.
Proof.
  intros tag children.
  unfold Target.scalar_true_or_absent.
  rewrite ChildProvenance.projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|]; reflexivity.
Qed.

Lemma exact_bool_or_absent_selected_child :
  forall tag children,
    Target.exact_bool_or_absent tag
      (ProcessProjection.projected_process_children children) =
    match Process.source_child_at tag children with
    | None => Some false
    | Some child => target_exact_bool_view tag child
    end.
Proof.
  intros tag children.
  unfold Target.exact_bool_or_absent.
  rewrite ChildProvenance.projected_process_child_at.
  destruct (Process.source_child_at tag children) as [child|]; reflexivity.
Qed.

Lemma source_bool_or_absent_selected_child :
  forall tag children,
    Process.source_bool_or_absent tag children =
    match Process.source_child_at tag children with
    | None => Some false
    | Some child => source_bool_view child
    end.
Proof. intros tag children. reflexivity. Qed.

(** The target reader compares the literal body with [[1]]. *)
Lemma target_body_is_true_of_non_singleton :
  forall body, body <> [1] -> target_body_is_true body = None.
Proof.
  intros body not_one.
  destruct body as [|head tail]; [reflexivity|].
  destruct head as [|[|high]]; [reflexivity| |reflexivity].
  destruct tail as [|next rest].
  - exact (False_ind _ (not_one eq_refl)).
  - reflexivity.
Qed.

Lemma target_scalar_true_view_of_scalar :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Schema.raw_payload raw = Schema.Scalar ->
    target_scalar_true_view child = target_body_is_true (Schema.raw_body raw).
Proof.
  intros child raw scalar typed payload.
  unfold target_scalar_true_view, target_body_is_true,
    Projection.project_source_summary, Core.source_summary_raw.
  cbn [Target.sem_payload Target.sem_body].
  rewrite typed.
  cbn [Core.source_value_raw].
  rewrite payload.
  destruct (Schema.raw_body raw) as [|head tail]; [reflexivity|].
  destruct head as [|[|high]]; [reflexivity| |reflexivity].
  destruct tail; reflexivity.
Qed.

Lemma target_exact_bool_view_of_scalar :
  forall tag child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Core.source_summary_refused child = false ->
    Schema.raw_payload raw = Schema.Scalar ->
    target_exact_bool_view tag child =
      target_exact_bool_of_body tag (Schema.raw_wire raw) (Schema.raw_body raw).
Proof.
  intros tag child raw scalar typed unrefused payload.
  unfold Core.source_summary_refused in unrefused.
  unfold target_exact_bool_view, target_exact_bool_of_body,
    Projection.project_source_summary, Core.source_summary_raw.
  cbn [Target.sem_wire Target.sem_payload Target.sem_definitely_refused
    Target.sem_body].
  rewrite typed, unrefused.
  cbn [Core.source_value_raw].
  rewrite payload.
  destruct (Schema.raw_wire raw); reflexivity.
Qed.

Lemma source_bool_view_of_scalar :
  forall child raw scalar,
    Core.source_summary_value child = Core.TypedSourceScalar raw scalar ->
    Core.source_summary_refused child = false ->
    source_bool_view child = source_bool_of_scalar scalar.
Proof.
  intros child raw scalar typed unrefused.
  unfold source_bool_view.
  rewrite unrefused, typed.
  destruct scalar; reflexivity.
Qed.

(** A child that is not a scalar reads as absent on every reader: the target
    readers test the payload and the source reader tests the typed value. *)
Lemma non_scalar_child_views_are_absent :
  forall child,
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Scalar = false ->
    value_is_scalar (Core.source_summary_value child) = false ->
    target_scalar_true_view child = None /\
    (forall tag, target_exact_bool_view tag child = None) /\
    source_bool_view child = None.
Proof.
  intros child not_scalar not_value.
  split; [|split].
  - unfold target_scalar_true_view, Projection.project_source_summary.
    cbn [Target.sem_payload Target.sem_body].
    destruct (Schema.raw_payload (Core.source_summary_raw child));
      [discriminate not_scalar|reflexivity|reflexivity|reflexivity].
  - intro tag.
    unfold target_exact_bool_view, Projection.project_source_summary.
    cbn [Target.sem_wire Target.sem_payload Target.sem_definitely_refused
      Target.sem_body].
    destruct (Schema.raw_payload (Core.source_summary_raw child));
      [discriminate not_scalar| | |];
      destruct (Schema.raw_wire (Core.source_summary_raw child)); reflexivity.
  - unfold source_bool_view.
    destruct (Core.source_summary_value child); try discriminate not_value.
    all: destruct (Core.source_summary_refused child); reflexivity.
Qed.

(** ** What the independent decoder accepts

    The default-forbidden bool decoder accepts exactly the one-byte body [[1]]:
    the canonical varint decoder rejects redundant continuation bytes, so a
    decoded value of one has no spelling other than the single byte [1]. *)

Lemma decoded_varint_one_is_singleton :
  forall body value,
    EPM1.decode_varint body = Some (value, []) ->
    value = 1 ->
    body = [1].
Proof.
  intros [|head tail] value parsed value_is_one.
  - rewrite EPM1.decode_varint_nil in parsed. discriminate parsed.
  - rewrite EPM1.decode_varint_cons in parsed.
    destruct (head <? 128) eqn:small.
    + inversion parsed; subst; reflexivity.
    + destruct (head <? 256) eqn:physical; [|discriminate parsed].
      destruct (EPM1.decode_varint tail) as [[quotient suffix]|] eqn:rest;
        [|discriminate parsed].
      destruct (quotient =? 0) eqn:nonzero; [discriminate parsed|].
      apply Nat.eqb_neq in nonzero.
      assert (reconstructed : value = head - 128 + 128 * quotient)
        by now inversion parsed.
      exfalso.
      lia.
Qed.

Lemma independently_decoded_one_is_singleton :
  forall body,
    Core.independently_decoded_u64 body = Some 1%N ->
    body = [1].
Proof.
  intros body accepted.
  destruct (Core.independently_decoded_u64_has_canonical_full_body
    body 1%N accepted) as [decoded [parsed [same_value _]]].
  apply (decoded_varint_one_is_singleton body decoded parsed).
  apply Nat2N.inj.
  rewrite <- same_value.
  reflexivity.
Qed.

Lemma decoded_forbidden_bool_is_one :
  forall body scalar,
    Core.independently_decode_scalar Data.PBBool true body = Some scalar ->
    scalar = Core.SourceBool true /\ body = [1].
Proof.
  intros body scalar decoded.
  unfold Core.independently_decode_scalar, Core.source_numeric_scalar,
    Core.source_numeric_nondefault in decoded.
  destruct (Core.independently_decoded_u64 body) as [value|] eqn:parsed;
    [|discriminate decoded].
  cbn [negb orb] in decoded.
  destruct (N.eqb value 0) eqn:zero; [discriminate decoded|].
  cbn [negb] in decoded.
  destruct (N.leb value 1) eqn:small; [|discriminate decoded].
  assert (value_is_one : value = 1%N).
  { apply N.leb_le in small. apply N.eqb_neq in zero. lia. }
  subst value.
  injection decoded as same_scalar.
  split.
  - rewrite <- same_scalar. reflexivity.
  - exact (independently_decoded_one_is_singleton body parsed).
Qed.

(** Eliminate a chain of boolean guards at the head of a decoder equation.
    Every guard that fails selects the decoder's [None] branch, which the
    equation to [Some] refutes. *)
Ltac reduce_guards decoded :=
  cbv beta iota zeta in decoded;
  lazymatch type of decoded with
  | (if ?condition then _ else _) = _ =>
      destruct condition; [reduce_guards decoded | discriminate decoded]
  | _ => idtac
  end.

Definition fixed_width_kind (kind : Data.protobuf_kind) : bool :=
  match kind with
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble => true
  | _ => false
  end.

(** Kinds whose decoder never produces [Core.SourceBool]. *)
Definition bool_free_kind (kind : Data.protobuf_kind) : bool :=
  match kind with
  | Data.PBSfixed32 | Data.PBFixed32 | Data.PBFloat
  | Data.PBSfixed64 | Data.PBFixed64 | Data.PBDouble
  | Data.PBString | Data.PBRepeatedString
  | Data.PBBytes | Data.PBRepeatedBytes => true
  | _ => false
  end.

Lemma bool_free_kind_decode_is_not_bool :
  forall kind forbidden body scalar,
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    bool_free_kind kind = true ->
    source_bool_of_scalar scalar = None.
Proof.
  intros kind forbidden body scalar decoded free.
  destruct kind; try discriminate free;
    cbv beta iota zeta delta
      [Core.independently_decode_scalar Core.source_fixed_scalar] in decoded;
    reduce_guards decoded;
    injection decoded as same_scalar; subst scalar; reflexivity.
Qed.

(** A fixed-width body has four or eight bytes, so it is never [[1]]. *)
Lemma fixed_width_decode_has_no_singleton_body :
  forall kind forbidden body scalar,
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    fixed_width_kind kind = true ->
    body <> [1].
Proof.
  intros kind forbidden body scalar decoded fixed same_body.
  subst body.
  destruct kind; try discriminate fixed;
    vm_compute in decoded; discriminate decoded.
Qed.

(** ** Per-kind agreement on an accepted scalar *)

Lemma accepted_scalar_value_matches_scalar_true :
  forall kind forbidden body scalar,
    scalar_true_faithful_kind kind forbidden = true ->
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    target_body_is_true body = source_bool_of_scalar scalar.
Proof.
  intros kind forbidden body scalar faithful decoded.
  destruct kind; cbn [scalar_true_faithful_kind] in faithful;
    try discriminate faithful; try discriminate decoded;
    [ subst forbidden;
      destruct (decoded_forbidden_bool_is_one body scalar decoded)
        as [same_scalar same_body];
      subst scalar body;
      reflexivity
    | rewrite (bool_free_kind_decode_is_not_bool _ forbidden body scalar
        decoded eq_refl);
      apply target_body_is_true_of_non_singleton;
      exact (fixed_width_decode_has_no_singleton_body _ forbidden body scalar
        decoded eq_refl) .. ].
Qed.

Lemma accepted_scalar_value_matches_exact_bool :
  forall kind forbidden wire tag body scalar,
    exact_bool_faithful_kind kind forbidden = true ->
    Admission.source_scalar_wire kind = Some wire ->
    Core.independently_decode_scalar kind forbidden body = Some scalar ->
    target_exact_bool_of_body tag wire body = source_bool_of_scalar scalar.
Proof.
  intros kind forbidden wire tag body scalar faithful wire_found decoded.
  destruct kind; cbn [exact_bool_faithful_kind] in faithful;
    try discriminate faithful; try discriminate wire_found;
    cbn [Admission.source_scalar_wire] in wire_found;
    injection wire_found as same_wire; subst wire;
    [ subst forbidden;
      destruct (decoded_forbidden_bool_is_one body scalar decoded)
        as [same_scalar same_body];
      subst scalar body;
      vm_compute; reflexivity
    | rewrite (bool_free_kind_decode_is_not_bool _ forbidden body scalar
        decoded eq_refl);
      reflexivity .. ].
Qed.

(** ** The children that the evaluator produces *)

Lemma scalar_wire_eqb_is_same_wire :
  forall left right, Schema.wire_eqb left right = true -> left = right.
Proof. intros [] [] same; try discriminate same; reflexivity. Qed.

(** The admission gate of an accepted scalar exposes the contract that read
    it, the wire the contract requires and the decoder's verdict on the body. *)
Lemma source_scalar_value_gate :
  forall program raw scalar,
    Admission.source_scalar_value_of_raw program raw = Some scalar ->
    exists contract wire,
      Admission.source_scalar_contract_for (Schema.program_family program)
        (Schema.raw_tag raw) = Some contract /\
      Admission.source_scalar_wire (Admission.source_contract_kind contract) =
        Some wire /\
      Schema.raw_wire raw = wire /\
      Core.independently_decode_scalar (Admission.source_contract_kind contract)
        (Admission.source_contract_default_forbidden contract)
        (Schema.raw_body raw) = Some scalar.
Proof.
  intros program raw scalar accepted.
  unfold Admission.source_scalar_value_of_raw in accepted.
  destruct (Schema.raw_payload raw); try discriminate accepted.
  destruct (Schema.raw_program_type_id raw); try discriminate accepted.
  destruct (Schema.resolve_tag (Schema.program_fields program)
      (Schema.raw_tag raw)) as [descriptor|]; try discriminate accepted.
  destruct (Admission.source_scalar_contract_for (Schema.program_family program)
      (Schema.raw_tag raw)) as [contract|] eqn:contract_found;
    try discriminate accepted.
  destruct (Admission.source_scalar_wire (Admission.source_contract_kind contract))
    as [wire|] eqn:wire_found; try discriminate accepted.
  destruct (Admission.source_raw_bytes_well_formed raw &&
      Schema.wire_eqb (Schema.raw_wire raw) wire &&
      Schema.wire_eqb wire (Schema.descriptor_wire descriptor) &&
      Schema.payload_eqb (Schema.descriptor_payload descriptor) Schema.Scalar)
    eqn:guarded; [|discriminate accepted].
  apply andb_true_iff in guarded as [guarded _].
  apply andb_true_iff in guarded as [guarded _].
  apply andb_true_iff in guarded as [_ wire_same].
  exists contract, wire.
  repeat split; try exact contract_found; try exact wire_found;
    try exact accepted.
  exact (scalar_wire_eqb_is_same_wire _ _ wire_same).
Qed.

Lemma evaluated_scalar_node_is_gated :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceScalar span field) = Some child ->
    exists raw scalar,
      Core.source_summary_value child = Core.TypedSourceScalar raw scalar /\
      Core.source_summary_refused child = false /\
      Schema.raw_payload raw = Schema.Scalar /\
      Admission.source_scalar_value_of_raw program raw = Some scalar.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceScalar span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  destruct (Admission.source_typed_scalar program raw)
    as [value|] eqn:typed; [|discriminate evaluated].
  inversion evaluated; subst child.
  unfold Admission.source_typed_scalar in typed.
  destruct (Admission.source_scalar_value_of_raw program raw)
    as [scalar|] eqn:decoded; [|discriminate typed].
  inversion typed; subst value.
  exists raw, scalar.
  split; [reflexivity|].
  split; [reflexivity|].
  split; [|exact decoded].
  exact (proj1 (ChildProvenance.converted_scalar_raw_has_scalar_shape
    body span field raw converted)).
Qed.

(** Walk the nested matches of an auxiliary-node evaluation down to its
    success constructor.  A [None] branch is refuted by the equation to
    [Some]. *)
Ltac finish_auxiliary evaluated :=
  cbv beta iota zeta in evaluated;
  lazymatch type of evaluated with
  | Some _ = Some _ => idtac
  | None = Some _ => discriminate evaluated
  | (match ?scrutinee with _ => _ end) = Some _ =>
      destruct scrutinee; finish_auxiliary evaluated
  end.

Lemma evaluated_auxiliary_node_is_not_scalar :
  forall recurse hash_fn body program span field child,
    Oracle.source_evaluate_node recurse hash_fn body program
      (Source.SourceAuxiliary span field) = Some child ->
    Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
      Schema.Scalar = false /\
    value_is_scalar (Core.source_summary_value child) = false.
Proof.
  intros recurse hash_fn body program span field child evaluated.
  unfold Oracle.source_evaluate_node in evaluated.
  destruct (Source.raw_field_of_source body (Source.SourceAuxiliary span field))
    as [raw|] eqn:converted; [|discriminate evaluated].
  pose proof (Source.raw_field_of_source_preserves_wire_and_body
    body (Source.SourceAuxiliary span field) raw converted)
    as [_ [_ [raw_payload_is_descriptor _]]].
  cbn [Source.source_field] in raw_payload_is_descriptor.
  destruct (Schema.descriptor_payload field) eqn:descriptor_kind;
    [discriminate evaluated|discriminate evaluated| |].
  all: finish_auxiliary evaluated.
  all: injection evaluated as same_child; subst child.
  all: cbn [Core.source_summary_raw Core.source_value_raw
    Core.source_summary_value value_is_scalar].
  all: rewrite raw_payload_is_descriptor.
  all: split; reflexivity.
Qed.

Lemma evaluated_node_cases :
  forall fuel hash_fn body program node child,
    Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
      hash_fn body program node = Some child ->
    (exists raw scalar,
       Core.source_summary_value child = Core.TypedSourceScalar raw scalar /\
       Core.source_summary_refused child = false /\
       Schema.raw_payload raw = Schema.Scalar /\
       Admission.source_scalar_value_of_raw program raw = Some scalar) \/
    (Schema.payload_eqb (Schema.raw_payload (Core.source_summary_raw child))
       Schema.Scalar = false /\
     value_is_scalar (Core.source_summary_value child) = false).
Proof.
  intros fuel hash_fn body program node child evaluated.
  destruct node as [span field|span field nested|span field].
  - left.
    exact (evaluated_scalar_node_is_gated _ hash_fn body program span field
      child evaluated).
  - right.
    destruct (ChildProvenance.accepted_source_message_node_has_typed_value
      fuel hash_fn body program span field nested child evaluated)
      as [raw [nested_children [_ [typed payload]]]].
    unfold Core.source_summary_raw.
    rewrite typed.
    cbn [Core.source_value_raw value_is_scalar].
    rewrite payload.
    split; reflexivity.
  - right.
    exact (evaluated_auxiliary_node_is_not_scalar _ hash_fn body program span
      field child evaluated).
Qed.

Lemma child_of_sequenced_nodes_has_node :
  forall (A B : Type) (evaluate : A -> option B) nodes results result,
    Source.sequence_source_results (map evaluate nodes) = Some results ->
    In result results ->
    exists node, In node nodes /\ evaluate node = Some result.
Proof.
  intros A B evaluate nodes.
  induction nodes as [|node rest IH]; intros results result selected member;
    simpl in selected.
  - inversion selected; subst.
    simpl in member.
    contradiction.
  - destruct (evaluate node) as [head|] eqn:evaluated; [|discriminate].
    destruct (Source.sequence_source_results (map evaluate rest))
      as [tail|] eqn:remaining; [|discriminate].
    inversion selected; subst results.
    destruct member as [same|later].
    + subst result. exists node. split; [left; reflexivity|exact evaluated].
    + destruct (IH tail result eq_refl later) as [partner [inside pairing]].
      exists partner. split; [right; exact inside|exact pairing].
Qed.

Lemma selected_child_has_source_node :
  forall fuel hash_fn family program body children tag child,
    process_actual_children fuel hash_fn family program body children ->
    Process.source_child_at tag children = Some child ->
    Core.source_summary_tag child = tag /\
    Schema.program_family program = family /\
    exists node,
      Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
        hash_fn body program node = Some child.
Proof.
  intros fuel hash_fn family program body children tag child actual selected.
  unfold process_actual_children in actual.
  destruct actual as [_ [looked_up [_ [nodes [_ sequenced]]]]].
  unfold Process.source_child_at in selected.
  apply find_some in selected as [member tag_match].
  apply Nat.eqb_eq in tag_match.
  split; [exact tag_match|].
  split.
  - exact (ScalarBridge.generated_program_has_requested_family family program
      looked_up).
  - destruct (child_of_sequenced_nodes_has_node _ _
      (Oracle.source_evaluate_node (Oracle.source_evaluate_message fuel hash_fn)
        hash_fn body program) nodes children child sequenced member)
      as [node [_ evaluated]].
    exists node. exact evaluated.
Qed.

(** ** The restricted correspondences

    The premises are the two contract predicates above.  Since the contract
    is read from the generated table of the family, the premise is a finite,
    decidable condition on the pair [(family, tag)]. *)

Theorem process_actual_children_scalar_true_corresponds :
  forall fuel hash_fn family program body children tag,
    process_actual_children fuel hash_fn family program body children ->
    scalar_true_faithful_contract
      (Admission.source_scalar_contract_for family tag) = true ->
    Target.scalar_true_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag actual faithful.
  rewrite scalar_true_or_absent_selected_child,
    source_bool_or_absent_selected_child.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  change (target_scalar_true_view child = source_bool_view child).
  destruct (selected_child_has_source_node fuel hash_fn family program body
    children tag child actual selected)
    as [same_tag [same_family [node evaluated]]].
  destruct (evaluated_node_cases fuel hash_fn body program node child evaluated)
    as [[raw [scalar [typed [unrefused [payload accepted]]]]]
       |[not_scalar not_value]].
  - destruct (source_scalar_value_gate program raw scalar accepted)
      as [contract [wire [contract_found [wire_found [wire_same decoded]]]]].
    assert (raw_tag_is_selected : Schema.raw_tag raw = tag).
    { rewrite <- same_tag.
      unfold Core.source_summary_tag, Core.source_summary_raw.
      rewrite typed.
      reflexivity. }
    rewrite same_family, raw_tag_is_selected in contract_found.
    rewrite contract_found in faithful.
    cbn [scalar_true_faithful_contract] in faithful.
    rewrite (target_scalar_true_view_of_scalar child raw scalar typed payload).
    rewrite (source_bool_view_of_scalar child raw scalar typed unrefused).
    exact (accepted_scalar_value_matches_scalar_true _ _ _ _ faithful decoded).
  - destruct (non_scalar_child_views_are_absent child not_scalar not_value)
      as [true_view [_ source_view]].
    rewrite true_view, source_view.
    reflexivity.
Qed.

Theorem process_actual_children_bool_corresponds :
  forall fuel hash_fn family program body children tag,
    process_actual_children fuel hash_fn family program body children ->
    exact_bool_faithful_contract
      (Admission.source_scalar_contract_for family tag) = true ->
    Target.exact_bool_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag actual faithful.
  rewrite exact_bool_or_absent_selected_child,
    source_bool_or_absent_selected_child.
  destruct (Process.source_child_at tag children) as [child|] eqn:selected;
    [|reflexivity].
  change (target_exact_bool_view tag child = source_bool_view child).
  destruct (selected_child_has_source_node fuel hash_fn family program body
    children tag child actual selected)
    as [same_tag [same_family [node evaluated]]].
  destruct (evaluated_node_cases fuel hash_fn body program node child evaluated)
    as [[raw [scalar [typed [unrefused [payload accepted]]]]]
       |[not_scalar not_value]].
  - destruct (source_scalar_value_gate program raw scalar accepted)
      as [contract [wire [contract_found [wire_found [wire_same decoded]]]]].
    assert (raw_tag_is_selected : Schema.raw_tag raw = tag).
    { rewrite <- same_tag.
      unfold Core.source_summary_tag, Core.source_summary_raw.
      rewrite typed.
      reflexivity. }
    rewrite same_family, raw_tag_is_selected in contract_found.
    rewrite contract_found in faithful.
    cbn [exact_bool_faithful_contract] in faithful.
    rewrite (target_exact_bool_view_of_scalar tag child raw scalar typed
      unrefused payload).
    rewrite wire_same.
    rewrite (source_bool_view_of_scalar child raw scalar typed unrefused).
    exact (accepted_scalar_value_matches_exact_bool _ _ wire tag _ scalar
      faithful wire_found decoded).
  - destruct (non_scalar_child_views_are_absent child not_scalar not_value)
      as [_ [bool_view source_view]].
    rewrite (bool_view tag), source_view.
    reflexivity.
Qed.

(** ** The premise holds at every slot the reducer reads

    The semantic reducer of the process families reads [scalar_true_or_absent]
    at Bundle tags 2, 3, Send tags 3, 6 and If tag 5, and
    [exact_bool_or_absent] at Receive tags 3, 4, 7 and Match tag 5.  Each of
    those slots is a default-forbidden [bool]. *)
Definition scalar_true_reader_slots : list (nat * nat) :=
  [(8, 2); (8, 3); (15, 3); (15, 6); (23, 5)].

Definition exact_bool_reader_slots : list (nat * nat) :=
  [(19, 3); (19, 4); (19, 7); (22, 5)].

Lemma scalar_true_reader_slots_are_forbidden_bools :
  forall family tag,
    In (family, tag) scalar_true_reader_slots ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBBool true).
Proof.
  intros family tag member.
  unfold scalar_true_reader_slots in member.
  destruct member as [same|[same|[same|[same|[same|[]]]]]];
    inversion same; subst family tag;
    vm_compute; reflexivity.
Qed.

Lemma exact_bool_reader_slots_are_forbidden_bools :
  forall family tag,
    In (family, tag) exact_bool_reader_slots ->
    Admission.source_scalar_contract_for family tag =
      Some (Admission.SourceScalarContract Data.PBBool true).
Proof.
  intros family tag member.
  unfold exact_bool_reader_slots in member.
  destruct member as [same|[same|[same|[same|[]]]]];
    inversion same; subst family tag;
    vm_compute; reflexivity.
Qed.

Theorem process_actual_children_scalar_true_corresponds_at_reader_slots :
  forall fuel hash_fn family program body children tag,
    In (family, tag) scalar_true_reader_slots ->
    process_actual_children fuel hash_fn family program body children ->
    Target.scalar_true_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag slot actual.
  apply (process_actual_children_scalar_true_corresponds fuel hash_fn family
    program body children tag actual).
  rewrite (scalar_true_reader_slots_are_forbidden_bools family tag slot).
  reflexivity.
Qed.

Theorem process_actual_children_bool_corresponds_at_reader_slots :
  forall fuel hash_fn family program body children tag,
    In (family, tag) exact_bool_reader_slots ->
    process_actual_children fuel hash_fn family program body children ->
    Target.exact_bool_or_absent tag
      (ProcessProjection.projected_process_children children) =
    Process.source_bool_or_absent tag children.
Proof.
  intros fuel hash_fn family program body children tag slot actual.
  apply (process_actual_children_bool_corresponds fuel hash_fn family
    program body children tag actual).
  rewrite (exact_bool_reader_slots_are_forbidden_bools family tag slot).
  reflexivity.
Qed.

(** ** Closed refutations of the unrestricted statements

    Each witness below is the family's generated decoder and evaluator run on
    a concrete body, so [process_actual_children] holds of it by computation.
    [hash_fn] is arbitrary (no witness decodes a path map) and [fuel] 1 is
    enough because no witness has a message child. *)

Definition witness_hash_fn (_ : list nat) : nat := 0.

Definition witness_program (family : nat) : Schema.generated_program :=
  match Schema.lookup_projected_program family [] [] with
  | Some program => program
  | None => Schema.empty_program
  end.

Definition witness_nodes (family : nat) (body : list nat)
    : list Source.source_tree :=
  match Source.decode_region (S (length body)) body (witness_program family)
      0 (length body) with
  | Some nodes => nodes
  | None => []
  end.

Definition witness_evaluation (family : nat) (body : list nat)
    : option (list Core.source_exact_summary) :=
  Source.sequence_source_results
    (map (Oracle.source_evaluate_node
           (Oracle.source_evaluate_message 1 witness_hash_fn) witness_hash_fn
           body (witness_program family))
      (witness_nodes family body)).

Definition witness_children (family : nat) (body : list nat)
    : list Core.source_exact_summary :=
  match witness_evaluation family body with
  | Some children => children
  | None => []
  end.

Lemma witness_is_actual :
  forall family body,
    In family [8; 15; 16; 19; 20; 21; 22; 23; 24] ->
    Schema.lookup_projected_program family [] [] =
      Some (witness_program family) ->
    Source.decode_region (S (length body)) body (witness_program family)
      0 (length body) = Some (witness_nodes family body) ->
    witness_evaluation family body = Some (witness_children family body) ->
    Admission.source_scan_valid
      (Admission.source_scan_children (witness_program family)
        (witness_children family body)) = true ->
    process_actual_children 1 witness_hash_fn family (witness_program family)
      body (witness_children family body).
Proof.
  intros family body member looked_up decoded sequenced scan.
  unfold process_actual_children.
  split; [exact member|].
  split; [exact looked_up|].
  split; [exact scan|].
  exists (witness_nodes family body).
  split; [exact decoded|exact sequenced].
Qed.

(** Field 1 of [New] is [bind_count], a default-forbidden [sint32].  The
    body is the key byte [(1 << 3) | 0 = 8] and the varint [1], which the
    zigzag reading maps to [-1].  Both target readers see an encoded [true]
    while the source reader sees a non-bool scalar. *)
Definition new_bind_count_body : list nat := [8; 1].

Example new_bind_count_contract :
  Admission.source_scalar_contract_for 20 1 =
    Some (Admission.SourceScalarContract Data.PBSint32 true).
Proof. vm_compute. reflexivity. Qed.

Lemma new_bind_count_witness_is_actual :
  process_actual_children 1 witness_hash_fn 20 (witness_program 20)
    new_bind_count_body (witness_children 20 new_bind_count_body).
Proof.
  apply witness_is_actual.
  - exact (or_intror (or_intror (or_intror (or_intror (or_introl eq_refl))))).
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
Qed.

Example new_bind_count_witness_value :
  map Core.source_summary_value (witness_children 20 new_bind_count_body) =
    [Core.TypedSourceScalar
      (Schema.RawField 1 Schema.Varint Schema.Scalar [1] 1 None [] [])
      (Core.SourceI32 (-1)%Z)].
Proof. vm_compute. reflexivity. Qed.

Theorem process_actual_children_scalar_true_unrestricted_is_false :
  ~ (forall fuel hash_fn family program body children tag,
      process_actual_children fuel hash_fn family program body children ->
      Target.scalar_true_or_absent tag
        (ProcessProjection.projected_process_children children) =
      Process.source_bool_or_absent tag children).
Proof.
  intro unrestricted.
  pose proof (unrestricted 1 witness_hash_fn 20 (witness_program 20)
    new_bind_count_body (witness_children 20 new_bind_count_body) 1
    new_bind_count_witness_is_actual) as disagreement.
  vm_compute in disagreement.
  discriminate disagreement.
Qed.

Theorem process_actual_children_bool_unrestricted_is_false :
  ~ (forall fuel hash_fn family program body children tag,
      process_actual_children fuel hash_fn family program body children ->
      Target.exact_bool_or_absent tag
        (ProcessProjection.projected_process_children children) =
      Process.source_bool_or_absent tag children).
Proof.
  intro unrestricted.
  pose proof (unrestricted 1 witness_hash_fn 20 (witness_program 20)
    new_bind_count_body (witness_children 20 new_bind_count_body) 1
    new_bind_count_witness_is_actual) as disagreement.
  vm_compute in disagreement.
  discriminate disagreement.
Qed.

(** ** Sharpness of the premise

    "Is a bool field" is not enough: a default-allowed [bool] (here the oneof
    arm [Expr.gBool], tag 1, body [[8; 0]]) reads [false] on the source side
    and is refused by both target readers. *)

Definition expr_gbool_zero_body : list nat := [8; 0].

Lemma expr_gbool_zero_witness_is_actual :
  process_actual_children 1 witness_hash_fn 24 (witness_program 24)
    expr_gbool_zero_body (witness_children 24 expr_gbool_zero_body).
Proof.
  apply witness_is_actual.
  - exact (or_intror (or_intror (or_intror (or_intror (or_intror (or_intror
      (or_intror (or_intror (or_introl eq_refl))))))))).
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
Qed.

Example expr_gbool_zero_witness_value :
  map Core.source_summary_value (witness_children 24 expr_gbool_zero_body) =
    [Core.TypedSourceScalar
      (Schema.RawField 1 Schema.Varint Schema.Scalar [0] 1 None [] [])
      (Core.SourceBool false)].
Proof. vm_compute. reflexivity. Qed.

Theorem default_allowed_bool_breaks_both_correspondences :
  Admission.source_scalar_contract_for 24 1 =
    Some (Admission.SourceScalarContract Data.PBBool false) /\
  Target.scalar_true_or_absent 1
    (ProcessProjection.projected_process_children
      (witness_children 24 expr_gbool_zero_body)) <>
    Process.source_bool_or_absent 1
      (witness_children 24 expr_gbool_zero_body) /\
  Target.exact_bool_or_absent 1
    (ProcessProjection.projected_process_children
      (witness_children 24 expr_gbool_zero_body)) <>
    Process.source_bool_or_absent 1
      (witness_children 24 expr_gbool_zero_body).
Proof.
  split; [vm_compute; reflexivity|split];
    intro same; vm_compute in same; discriminate same.
Qed.

(** A length-delimited scalar breaks only the scalar-true reader: [Send]
    [locally_free] (tag 5, a default-forbidden [bytes]) with the one-byte
    payload [[1]] looks like an encoded [true] to a reader that ignores the
    wire, while the wire-checking reader refuses it, as does the source
    reader, which sees a blob. *)

Definition send_locally_free_body : list nat := [42; 1; 1].

Lemma send_locally_free_witness_is_actual :
  process_actual_children 1 witness_hash_fn 15 (witness_program 15)
    send_locally_free_body (witness_children 15 send_locally_free_body).
Proof.
  apply witness_is_actual.
  - exact (or_intror (or_introl eq_refl)).
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
  - vm_compute. reflexivity.
Qed.

Example send_locally_free_witness_value :
  map Core.source_summary_value (witness_children 15 send_locally_free_body) =
    [Core.TypedSourceScalar
      (Schema.RawField 5 Schema.LengthDelimited Schema.Scalar [1] 1 None [] [])
      (Core.SourceBlob [1])].
Proof. vm_compute. reflexivity. Qed.

Theorem bytes_tag_breaks_only_scalar_true_correspondence :
  Admission.source_scalar_contract_for 15 5 =
    Some (Admission.SourceScalarContract Data.PBBytes true) /\
  Target.scalar_true_or_absent 5
    (ProcessProjection.projected_process_children
      (witness_children 15 send_locally_free_body)) <>
    Process.source_bool_or_absent 5
      (witness_children 15 send_locally_free_body) /\
  Target.exact_bool_or_absent 5
    (ProcessProjection.projected_process_children
      (witness_children 15 send_locally_free_body)) =
    Process.source_bool_or_absent 5
      (witness_children 15 send_locally_free_body).
Proof.
  split; [vm_compute; reflexivity|split].
  - intro same. vm_compute in same. discriminate same.
  - vm_compute. reflexivity.
Qed.

(** ** The premises are exact on the fields of the process families

    The sharpness witnesses above are representatives of the rejected
    contract shapes.  The finite certificate below covers every field that
    the generated program of a process family declares: for each field whose
    contract the premise rejects, the check computes a concrete body (the
    field key followed by a varint [1], a varint [0] for a default-allowed
    bool, or the one-byte length-delimited payload [[1]]), runs the decoder and
    evaluator on it, and checks that the witness is an actual-children
    instance on which the reader and the source disagree.  Together with the
    restricted correspondence theorems this makes each premise an exact
    characterization on those fields, which is the sense in which the
    restriction is minimal. *)

Definition process_families : list nat := [8; 15; 16; 19; 20; 21; 22; 23; 24].

Definition process_field_tags (family : nat) : list nat :=
  map Schema.descriptor_tag (Schema.program_fields (witness_program family)).

Definition witness_varint_value (kind : Data.protobuf_kind) : nat :=
  match kind with
  | Data.PBBool => 0
  | _ => 1
  end.

Definition witness_body_for (family tag : nat) : list nat :=
  match Admission.source_scalar_contract_for family tag with
  | Some contract =>
      match Admission.source_scalar_wire
          (Admission.source_contract_kind contract) with
      | Some Schema.Varint =>
          EPM1.encode_varint (tag * 8) ++
            [witness_varint_value (Admission.source_contract_kind contract)]
      | Some Schema.LengthDelimited =>
          EPM1.encode_varint (tag * 8 + 2) ++ [1; 1]
      | _ => []
      end
  | None => []
  end.

Definition option_is_some {A : Type} (value : option A) : bool :=
  match value with
  | Some _ => true
  | None => false
  end.

Definition actual_check (family : nat) (body : list nat) : bool :=
  existsb (Nat.eqb family) process_families &&
  option_is_some (Schema.lookup_projected_program family [] []) &&
  option_is_some (Source.decode_region (S (length body)) body
    (witness_program family) 0 (length body)) &&
  option_is_some (witness_evaluation family body) &&
  Admission.source_scan_valid
    (Admission.source_scan_children (witness_program family)
      (witness_children family body)).

Lemma actual_check_sound :
  forall family body,
    actual_check family body = true ->
    process_actual_children 1 witness_hash_fn family (witness_program family)
      body (witness_children family body).
Proof.
  intros family body checked.
  unfold actual_check in checked.
  apply andb_true_iff in checked as [checked scan].
  apply andb_true_iff in checked as [checked evaluated].
  apply andb_true_iff in checked as [checked decoded].
  apply andb_true_iff in checked as [member looked_up].
  apply witness_is_actual.
  - apply existsb_exists in member as [candidate [candidate_member same]].
    apply Nat.eqb_eq in same.
    subst candidate.
    exact candidate_member.
  - unfold witness_program.
    destruct (Schema.lookup_projected_program family [] [])
      as [program|]; [reflexivity|discriminate looked_up].
  - unfold witness_nodes.
    destruct (Source.decode_region (S (length body)) body
      (witness_program family) 0 (length body))
      as [nodes|]; [reflexivity|discriminate decoded].
  - unfold witness_children.
    destruct (witness_evaluation family body)
      as [children|]; [reflexivity|discriminate evaluated].
  - exact scan.
Qed.

Definition option_bool_eqb (left right : option bool) : bool :=
  match left, right with
  | Some first, Some second => Bool.eqb first second
  | None, None => true
  | _, _ => false
  end.

Lemma option_bool_eqb_of_equal :
  forall left right, left = right -> option_bool_eqb left right = true.
Proof.
  intros left right same.
  subst right.
  destruct left as [[|]|]; reflexivity.
Qed.

Definition scalar_true_necessity_table : bool :=
  forallb (fun family =>
    forallb (fun tag =>
      scalar_true_faithful_contract
        (Admission.source_scalar_contract_for family tag) ||
      (actual_check family (witness_body_for family tag) &&
       negb (option_bool_eqb
         (Target.scalar_true_or_absent tag
           (ProcessProjection.projected_process_children
             (witness_children family (witness_body_for family tag))))
         (Process.source_bool_or_absent tag
           (witness_children family (witness_body_for family tag))))))
      (process_field_tags family))
    process_families.

Lemma scalar_true_necessity_table_is_true :
  scalar_true_necessity_table = true.
Proof. vm_compute. reflexivity. Qed.

Definition exact_bool_necessity_table : bool :=
  forallb (fun family =>
    forallb (fun tag =>
      exact_bool_faithful_contract
        (Admission.source_scalar_contract_for family tag) ||
      (actual_check family (witness_body_for family tag) &&
       negb (option_bool_eqb
         (Target.exact_bool_or_absent tag
           (ProcessProjection.projected_process_children
             (witness_children family (witness_body_for family tag))))
         (Process.source_bool_or_absent tag
           (witness_children family (witness_body_for family tag))))))
      (process_field_tags family))
    process_families.

Lemma exact_bool_necessity_table_is_true :
  exact_bool_necessity_table = true.
Proof. vm_compute. reflexivity. Qed.

Theorem scalar_true_premise_is_necessary_on_process_fields :
  forall family tag,
    In family process_families ->
    In tag (process_field_tags family) ->
    scalar_true_faithful_contract
      (Admission.source_scalar_contract_for family tag) = false ->
    exists body,
      process_actual_children 1 witness_hash_fn family (witness_program family)
        body (witness_children family body) /\
      Target.scalar_true_or_absent tag
        (ProcessProjection.projected_process_children
          (witness_children family body)) <>
      Process.source_bool_or_absent tag (witness_children family body).
Proof.
  intros family tag in_family in_tags unfaithful.
  pose proof scalar_true_necessity_table_is_true as table.
  unfold scalar_true_necessity_table in table.
  rewrite forallb_forall in table.
  specialize (table family in_family).
  cbv beta in table.
  rewrite forallb_forall in table.
  specialize (table tag in_tags).
  cbv beta in table.
  rewrite unfaithful in table.
  cbn [orb] in table.
  apply andb_true_iff in table as [actual disagreement].
  exists (witness_body_for family tag).
  split.
  - exact (actual_check_sound family (witness_body_for family tag) actual).
  - intro same.
    rewrite (option_bool_eqb_of_equal _ _ same) in disagreement.
    discriminate disagreement.
Qed.

Theorem exact_bool_premise_is_necessary_on_process_fields :
  forall family tag,
    In family process_families ->
    In tag (process_field_tags family) ->
    exact_bool_faithful_contract
      (Admission.source_scalar_contract_for family tag) = false ->
    exists body,
      process_actual_children 1 witness_hash_fn family (witness_program family)
        body (witness_children family body) /\
      Target.exact_bool_or_absent tag
        (ProcessProjection.projected_process_children
          (witness_children family body)) <>
      Process.source_bool_or_absent tag (witness_children family body).
Proof.
  intros family tag in_family in_tags unfaithful.
  pose proof exact_bool_necessity_table_is_true as table.
  unfold exact_bool_necessity_table in table.
  rewrite forallb_forall in table.
  specialize (table family in_family).
  cbv beta in table.
  rewrite forallb_forall in table.
  specialize (table tag in_tags).
  cbv beta in table.
  rewrite unfaithful in table.
  cbn [orb] in table.
  apply andb_true_iff in table as [actual disagreement].
  exists (witness_body_for family tag).
  split.
  - exact (actual_check_sound family (witness_body_for family tag) actual).
  - intro same.
    rewrite (option_bool_eqb_of_equal _ _ same) in disagreement.
    discriminate disagreement.
Qed.

(** The exact form of minimality: on a field of a process family, the premise
    holds if and only if the correspondence holds for all actual children. *)
Theorem scalar_true_premise_characterizes_field :
  forall family tag,
    In family process_families ->
    In tag (process_field_tags family) ->
    (scalar_true_faithful_contract
       (Admission.source_scalar_contract_for family tag) = true <->
     (forall fuel hash_fn program body children,
        process_actual_children fuel hash_fn family program body children ->
        Target.scalar_true_or_absent tag
          (ProcessProjection.projected_process_children children) =
        Process.source_bool_or_absent tag children)).
Proof.
  intros family tag in_family in_tags.
  split.
  - intros faithful fuel hash_fn program body children actual.
    exact (process_actual_children_scalar_true_corresponds fuel hash_fn family
      program body children tag actual faithful).
  - intro universal.
    destruct (scalar_true_faithful_contract
        (Admission.source_scalar_contract_for family tag)) eqn:faithful;
      [reflexivity|].
    destruct (scalar_true_premise_is_necessary_on_process_fields family tag
      in_family in_tags faithful) as [body [actual disagreement]].
    exact (False_ind _ (disagreement (universal _ _ _ _ _ actual))).
Qed.

Theorem exact_bool_premise_characterizes_field :
  forall family tag,
    In family process_families ->
    In tag (process_field_tags family) ->
    (exact_bool_faithful_contract
       (Admission.source_scalar_contract_for family tag) = true <->
     (forall fuel hash_fn program body children,
        process_actual_children fuel hash_fn family program body children ->
        Target.exact_bool_or_absent tag
          (ProcessProjection.projected_process_children children) =
        Process.source_bool_or_absent tag children)).
Proof.
  intros family tag in_family in_tags.
  split.
  - intros faithful fuel hash_fn program body children actual.
    exact (process_actual_children_bool_corresponds fuel hash_fn family
      program body children tag actual faithful).
  - intro universal.
    destruct (exact_bool_faithful_contract
        (Admission.source_scalar_contract_for family tag)) eqn:faithful;
      [reflexivity|].
    destruct (exact_bool_premise_is_necessary_on_process_fields family tag
      in_family in_tags faithful) as [body [actual disagreement]].
    exact (False_ind _ (disagreement (universal _ _ _ _ _ actual))).
Qed.

Print Assumptions scalar_true_or_absent_selected_child.
Print Assumptions exact_bool_or_absent_selected_child.
Print Assumptions source_bool_or_absent_selected_child.
Print Assumptions target_body_is_true_of_non_singleton.
Print Assumptions target_scalar_true_view_of_scalar.
Print Assumptions target_exact_bool_view_of_scalar.
Print Assumptions source_bool_view_of_scalar.
Print Assumptions non_scalar_child_views_are_absent.
Print Assumptions decoded_varint_one_is_singleton.
Print Assumptions independently_decoded_one_is_singleton.
Print Assumptions decoded_forbidden_bool_is_one.
Print Assumptions bool_free_kind_decode_is_not_bool.
Print Assumptions fixed_width_decode_has_no_singleton_body.
Print Assumptions accepted_scalar_value_matches_scalar_true.
Print Assumptions accepted_scalar_value_matches_exact_bool.
Print Assumptions scalar_wire_eqb_is_same_wire.
Print Assumptions source_scalar_value_gate.
Print Assumptions evaluated_scalar_node_is_gated.
Print Assumptions evaluated_auxiliary_node_is_not_scalar.
Print Assumptions evaluated_node_cases.
Print Assumptions child_of_sequenced_nodes_has_node.
Print Assumptions selected_child_has_source_node.
Print Assumptions process_actual_children_scalar_true_corresponds.
Print Assumptions process_actual_children_bool_corresponds.
Print Assumptions scalar_true_reader_slots_are_forbidden_bools.
Print Assumptions exact_bool_reader_slots_are_forbidden_bools.
Print Assumptions process_actual_children_scalar_true_corresponds_at_reader_slots.
Print Assumptions process_actual_children_bool_corresponds_at_reader_slots.
Print Assumptions witness_is_actual.
Print Assumptions new_bind_count_contract.
Print Assumptions new_bind_count_witness_is_actual.
Print Assumptions new_bind_count_witness_value.
Print Assumptions process_actual_children_scalar_true_unrestricted_is_false.
Print Assumptions process_actual_children_bool_unrestricted_is_false.
Print Assumptions expr_gbool_zero_witness_is_actual.
Print Assumptions expr_gbool_zero_witness_value.
Print Assumptions default_allowed_bool_breaks_both_correspondences.
Print Assumptions send_locally_free_witness_is_actual.
Print Assumptions send_locally_free_witness_value.
Print Assumptions bytes_tag_breaks_only_scalar_true_correspondence.
Print Assumptions actual_check_sound.
Print Assumptions option_bool_eqb_of_equal.
Print Assumptions scalar_true_necessity_table_is_true.
Print Assumptions exact_bool_necessity_table_is_true.
Print Assumptions scalar_true_premise_is_necessary_on_process_fields.
Print Assumptions exact_bool_premise_is_necessary_on_process_fields.
Print Assumptions scalar_true_premise_characterizes_field.
Print Assumptions exact_bool_premise_characterizes_field.
