From Stdlib Require Import List Arith Bool Lia.
From StackSafePDA Require Import EPM1 SubstitutionRegionFraming
  SubstitutionRegionEmission SubstitutionRegionPublication.

Import ListNotations.
Import SubstitutionRegionFraming.SubstitutionRegionFraming.
Import SubstitutionRegionEmission.SubstitutionRegionEmission.
Import SubstitutionRegionPublication.SubstitutionRegionPublication.
Set Implicit Arguments.

Module SubstitutionRegionDecomposition.

Inductive wire := Varint | Fixed64 | Delimited | Fixed32.

Definition wire_of (key : nat) : option wire :=
  match key mod 8 with
  | 0 => Some Varint | 1 => Some Fixed64
  | 2 => Some Delimited | 5 => Some Fixed32 | _ => None
  end.

Definition take_body (size : nat) (input : bytes) : option (bytes * bytes) :=
  if size <=? length input then Some (firstn size input, skipn size input) else None.

Definition scan_body (kind : wire) (input : bytes)
  : option ((bytes * bytes) * bytes) :=
  match kind with
  | Varint =>
      match scan_groups 10 true input with
      | Some (value, rest) => Some ((encode_varint value, encode_varint value), rest)
      | None => None
      end
  | Fixed64 | Fixed32 =>
      let size := match kind with Fixed64 => 8 | _ => 4 end in
      match take_body size input with
      | Some (body, rest) => Some ((body, body), rest) | None => None
      end
  | Delimited =>
      match scan_groups 10 true input with
      | Some (size, after_length) =>
          match take_body size after_length with
          | Some (body, rest) => Some ((body, encode_frame body), rest)
          | None => None
          end
      | None => None
      end
  end.

Lemma taken_body_splits_source : forall size input body rest,
  take_body size input = Some (body, rest) ->
  input = body ++ rest /\ length body = size.
Proof.
  intros size input body rest H; unfold take_body in H.
  destruct (size <=? length input) eqn:Enough; try discriminate.
  apply Nat.leb_le in Enough; inversion H; subst.
  split; [symmetry; apply firstn_skipn | rewrite length_firstn; apply Nat.min_l; exact Enough].
Qed.

Theorem successful_body_scan_reconstructs_source : forall kind input body encoded rest,
  scan_body kind input = Some ((body, encoded), rest) ->
  input = encoded ++ rest /\ (kind = Delimited -> encoded = encode_frame body).
Proof.
  intros kind input body encoded rest H; destruct kind; cbn [scan_body] in H.
  - destruct (scan_groups 10 true input) as [[value tail]|] eqn:Scan; try discriminate.
    inversion H; subst; split.
    + exact (@successful_bounded_varint_reencodes_consumed_prefix input value rest Scan).
    + discriminate.
  - destruct (take_body 8 input) as [[payload tail]|] eqn:Take; try discriminate.
    inversion H; subst; split.
    + exact (proj1 (@taken_body_splits_source 8 input encoded rest Take)).
    + discriminate.
  - destruct (scan_groups 10 true input) as [[size after_length]|] eqn:Scan; try discriminate.
    destruct (take_body size after_length) as [[payload tail]|] eqn:Take; try discriminate.
    inversion H; subst; split; [|reflexivity].
    destruct (@taken_body_splits_source size after_length body rest Take) as [Source Length].
    rewrite (@successful_bounded_varint_reencodes_consumed_prefix input size after_length Scan).
    unfold encode_frame.
    rewrite <- Length, Source; apply app_assoc.
  - destruct (take_body 4 input) as [[payload tail]|] eqn:Take; try discriminate.
    inversion H; subst; split.
    + exact (proj1 (@taken_body_splits_source 4 input encoded rest Take)).
    + discriminate.
Qed.

Record scanned_field := Field {
  field_key : nat;
  field_wire : wire;
  field_body : bytes;
  field_bytes : bytes
}.

Definition field_tag (f : scanned_field) := field_key f / 8.

Definition scan_field (max_tag : nat) (input : bytes)
  : option (scanned_field * bytes) :=
  match scan_groups 10 true input with
  | Some (key, after_key) =>
      if (0 <? key / 8) && (key / 8 <=? max_tag) then
        match wire_of key with
        | Some kind =>
            match scan_body kind after_key with
            | Some ((body, encoded), rest) =>
                Some (Field key kind body (encode_varint key ++ encoded), rest)
            | None => None
            end
        | None => None
        end
      else None
  | None => None
  end.

Lemma delimited_wire_has_tag_two : forall key,
  wire_of key = Some Delimited -> key mod 8 = 2.
Proof.
  intros key H; unfold wire_of in H.
  destruct (key mod 8) as [|[|[|[|[|[|n]]]]]]; inversion H; reflexivity.
Qed.

Theorem successful_field_scan_reconstructs_source : forall max_tag input f rest,
  scan_field max_tag input = Some (f, rest) ->
  input = field_bytes f ++ rest.
Proof.
  intros max_tag input f rest H; unfold scan_field in H.
  destruct (scan_groups 10 true input) as [[key after_key]|] eqn:Key; try discriminate.
  destruct ((0 <? key / 8) && (key / 8 <=? max_tag)); try discriminate.
  destruct (wire_of key) as [kind|]; try discriminate.
  destruct (scan_body kind after_key) as [[[body encoded] tail]|] eqn:Body; try discriminate.
  inversion H; subst.
  rewrite (@successful_bounded_varint_reencodes_consumed_prefix input key after_key Key).
  rewrite (proj1 (@successful_body_scan_reconstructs_source kind after_key body encoded rest Body)).
  cbn [field_bytes]; apply app_assoc.
Qed.

Theorem successful_delimited_field_has_exact_header : forall max_tag input f rest,
  scan_field max_tag input = Some (f, rest) ->
  field_wire f = Delimited ->
  field_bytes f = delimited (field_tag f) (field_body f).
Proof.
  intros max_tag input f rest H Kind; unfold scan_field in H.
  destruct (scan_groups 10 true input) as [[key after_key]|] eqn:Key; try discriminate.
  destruct ((0 <? key / 8) && (key / 8 <=? max_tag)); try discriminate.
  destruct (wire_of key) as [kind|] eqn:Wire; try discriminate.
  destruct (scan_body kind after_key) as [[[body encoded] tail]|] eqn:Body; try discriminate.
  inversion H; subst; cbn [field_wire] in Kind; subst kind.
  pose proof (@delimited_wire_has_tag_two key Wire) as Tag.
  pose proof (Nat.div_mod key 8 ltac:(lia)) as KeyParts.
  pose proof (proj2 (@successful_body_scan_reconstructs_source Delimited after_key body encoded rest Body) eq_refl) as Encoded.
  cbn [field_bytes field_tag field_key field_body].
  rewrite Encoded; unfold delimited, header, encode_frame.
  replace key with (8 * (key / 8) + 2) at 1 by lia.
  apply app_assoc.
Qed.

Section Builder.
Variable cuts : nat -> bytes.
Variable prefix : nat -> bytes.
Variable groups : nat -> list nat.

Definition certificate := field_certificate cuts prefix groups.
Definition source_piece_at := source_piece cuts prefix groups.

Inductive state :=
| Ready : list (piece * bytes) -> bytes -> state
| Suspended : list (piece * bytes) -> scanned_field -> bytes -> state.

Definition invariant (a : arena) (source : bytes) (s : state) : Prop :=
  match s with
  | Ready fields rest =>
      certificate a fields /\ source = frame_source fields ++ rest
  | Suspended fields held rest =>
      certificate a fields /\
      source = frame_source fields ++ field_bytes held ++ rest /\
      field_bytes held = delimited (field_tag held) (field_body held)
  end.

Lemma append_certificate : forall a fields p encoded,
  certificate a fields -> encoded = source_piece_at a p ->
  certificate a (fields ++ [(p, encoded)]).
Proof.
  intros; unfold certificate, field_certificate in *.
  apply Forall_app; split; [assumption|constructor; cbn; [assumption|constructor]].
Qed.

Lemma appended_frame_source : forall fields p encoded,
  frame_source (fields ++ [(p, encoded)]) = frame_source fields ++ encoded.
Proof.
  intros; unfold frame_source; rewrite map_app, concat_app; cbn.
  now rewrite app_nil_r.
Qed.

Theorem initial_frame_has_source_invariant : forall a source,
  invariant a source (Ready [] source).
Proof. intros; split; [constructor|reflexivity]. Qed.

Theorem borrowed_field_establishes_certificate : forall a f,
  field_bytes f = source_piece_at a (Borrowed (field_bytes f)).
Proof. reflexivity. Qed.

Theorem par_field_establishes_certificate : forall a max_tag input f rest cut,
  scan_field max_tag input = Some (f, rest) ->
  field_wire f = Delimited -> cuts cut = field_body f ->
  field_bytes f = source_piece_at a (Par (field_tag f) cut).
Proof.
  intros; unfold source_piece_at, source_piece.
  rewrite H1; eapply successful_delimited_field_has_exact_header; eauto.
Qed.

Theorem ready_field_consumption_preserves_source : forall a source fields input max_tag f rest p,
  invariant a source (Ready fields input) ->
  scan_field max_tag input = Some (f, rest) ->
  field_bytes f = source_piece_at a p ->
  invariant a source (Ready (fields ++ [(p, field_bytes f)]) rest).
Proof.
  intros a source fields input max_tag f rest p [Cert Source] Scan Piece.
  split.
  - apply append_certificate; assumption.
  - rewrite appended_frame_source, Source.
    rewrite (@successful_field_scan_reconstructs_source max_tag input f rest Scan).
    apply app_assoc.
Qed.

Theorem child_descent_preserves_source : forall a source fields input max_tag f rest,
  invariant a source (Ready fields input) ->
  scan_field max_tag input = Some (f, rest) ->
  field_wire f = Delimited ->
  invariant a source (Suspended fields f rest).
Proof.
  intros a source fields input max_tag f rest [Cert Source] Scan Kind.
  split; [exact Cert|split].
  - rewrite Source, (@successful_field_scan_reconstructs_source max_tag input f rest Scan).
    reflexivity.
  - eapply successful_delimited_field_has_exact_header; eauto.
Qed.

Theorem published_child_closes_suspended_field : forall a source fields held rest node retain,
  invariant a source (Suspended fields held rest) ->
  sources a node = field_body held ->
  invariant a source
    (Ready (fields ++ [(Child (field_tag held) node retain, field_bytes held)]) rest).
Proof.
  intros a source fields held rest node retain [Cert [Source Header]] ChildSource.
  split.
  - apply append_certificate; [exact Cert|].
    unfold source_piece_at, source_piece; rewrite ChildSource; exact Header.
  - rewrite appended_frame_source, Source; apply app_assoc.
Qed.

Theorem completed_frame_supplies_publication_certificate : forall a source fields,
  invariant a source (Ready fields []) ->
  certificate a fields /\ source = frame_source fields.
Proof. intros a source fields [Cert Source]; rewrite app_nil_r in Source; auto. Qed.

Theorem completed_frame_reconstructs_original : forall a source fields,
  valid cuts prefix groups a ->
  invariant a source (Ready fields []) ->
  denotation cuts prefix groups a Original (map fst fields) = source.
Proof.
  intros a source fields Valid Done.
  destruct (completed_frame_supplies_publication_certificate Done) as [Cert Source].
  rewrite Source; apply certified_fields_reconstruct_original; assumption.
Qed.


Lemma original_value_frames_are_encoded_frames : forall identifiers,
  concat (map (fun cut => value_frame Original (cuts cut)) identifiers) =
  encode_frames (map cuts identifiers).
Proof.
  induction identifiers; cbn [map concat encode_frames]; [reflexivity|].
  rewrite IHidentifiers; reflexivity.
Qed.

Theorem epathmap_field_establishes_certificate :
  forall a max_tag input f rest group table value_count,
  scan_field max_tag input = Some (f, rest) ->
  field_wire f = Delimited ->
  field_body f = prefix group ++ table ->
  decode_frames value_count table = Some (map cuts (groups group), []) ->
  field_bytes f = source_piece_at a (EPathMap (field_tag f) group).
Proof.
  intros a max_tag input f rest group table value_count Scan Kind Body Frames.
  rewrite (@successful_delimited_field_has_exact_header max_tag input f rest Scan Kind).
  unfold source_piece_at, source_piece, group_body.
  rewrite original_value_frames_are_encoded_frames, Body.
  f_equal; exact (@epm_value_frames_reconstruct_complete_suffix
    (prefix group) table value_count (map cuts (groups group)) Frames).
Qed.

Theorem prior_piece_source_survives_publication : forall a ps sizes source p,
  backward (count a) p ->
  source_piece_at (append cuts prefix groups a ps sizes source) p = source_piece_at a p.
Proof.
  intros a ps sizes source p Old; destruct p; cbn [source_piece_at source_piece]; auto.
  unfold backward in Old.
  cbn [append sources]; rewrite update_other by lia; reflexivity.
Qed.

Theorem prior_certificate_survives_publication : forall a ps sizes source fields,
  certificate a fields ->
  Forall (fun field => backward (count a) (fst field)) fields ->
  certificate (append cuts prefix groups a ps sizes source) fields.
Proof.
  intros a ps sizes source fields Cert Bounds.
  unfold certificate, field_certificate in *.
  induction Cert; inversion Bounds; subst; constructor; auto.
  rewrite prior_piece_source_survives_publication by assumption; assumption.
Qed.

Theorem suspended_parent_survives_child_publication :
  forall a ps sizes child_source parent_source fields held rest,
  invariant a parent_source (Suspended fields held rest) ->
  Forall (fun field => backward (count a) (fst field)) fields ->
  invariant (append cuts prefix groups a ps sizes child_source)
    parent_source (Suspended fields held rest).
Proof.
  intros a ps sizes child_source parent_source fields held rest [Cert Rest] Bounds.
  split; [eapply prior_certificate_survives_publication; eauto|exact Rest].
Qed.

Theorem completed_frame_publishes_valid_arena :
  forall limit word64 a source fields next,
  valid cuts prefix groups a ->
  invariant a source (Ready fields []) ->
  Forall (backward (count a)) (map fst fields) ->
  publish cuts prefix groups limit word64 a (map fst fields) source = Some next ->
  valid cuts prefix groups next.
Proof.
  intros limit word64 a source fields next Valid Done Back Published.
  destruct (completed_frame_supplies_publication_certificate Done) as [Cert Source].
  rewrite Source in Published.
  eapply successful_checked_publication_establishes_invariants; eauto.
Qed.

End Builder.

Module DispatchAndSlots.

Inductive action :=
| BorrowField
| CutPar
| DescendMessage (type_id : nat) (retain_empty : bool)
| DescendMap (value_type_id : nat)
| AdmitSnapshot.

Inductive field_shape :=
| ScalarField
| OptionalField (type_id : nat)
| RepeatedField (type_id : nat)
| MapField (value_type_id : nat)
| SnapshotField
| MessageArm (type_id : nat)
| InconsistentField.

Definition child_action par_id type_id retain :=
  if Nat.eqb par_id type_id then CutPar else DescendMessage type_id retain.

Definition action_for par_id shape : option action :=
  match shape with
  | ScalarField => Some BorrowField
  | OptionalField child | MessageArm child => Some (child_action par_id child true)
  | RepeatedField child => Some (child_action par_id child false)
  | MapField child => Some (DescendMap child)
  | SnapshotField => Some AdmitSnapshot
  | InconsistentField => None
  end.

Record descriptor := Descriptor {
  slot : nat;
  repeated : bool;
  expected_wire : wire;
  shape : field_shape
}.

Definition wire_eq_dec (a b : wire) : {a = b} + {a <> b}.
Proof. decide equality. Defined.

Inductive refusal := WrongWire | DuplicateSlot | InconsistentSchema.
Inductive decision := Accepted (selected : action) | Refused (reason : refusal).

Definition dispatch par_id (known : option descriptor) actual (seen : nat -> bool) :=
  match known with
  | None => Accepted BorrowField
  | Some d =>
      if wire_eq_dec actual (expected_wire d) then
        if negb (repeated d) && seen (slot d) then Refused DuplicateSlot
        else match action_for par_id (shape d) with
             | Some a => Accepted a | None => Refused InconsistentSchema
             end
      else Refused WrongWire
  end.

Theorem unknown_field_borrows_without_singular_check : forall par_id actual seen,
  dispatch par_id None actual seen = Accepted BorrowField.
Proof. reflexivity. Qed.

Theorem wrong_wire_precedes_slot_and_shape_checks : forall par_id d actual seen,
  actual <> expected_wire d ->
  dispatch par_id (Some d) actual seen = Refused WrongWire.
Proof.
  intros; unfold dispatch; destruct (wire_eq_dec actual (expected_wire d)); congruence.
Qed.

Theorem duplicate_slot_precedes_shape_check : forall par_id d seen,
  repeated d = false -> seen (slot d) = true ->
  dispatch par_id (Some d) (expected_wire d) seen = Refused DuplicateSlot.
Proof.
  intros; unfold dispatch; destruct (wire_eq_dec (expected_wire d) (expected_wire d));
    [now rewrite H, H0 | congruence].
Qed.

Theorem accepted_dispatch_has_exact_generated_action : forall par_id d actual seen a,
  dispatch par_id (Some d) actual seen = Accepted a ->
  actual = expected_wire d /\
  (repeated d = true \/ seen (slot d) = false) /\
  action_for par_id (shape d) = Some a.
Proof.
  intros par_id d actual seen a H; unfold dispatch in H.
  destruct (wire_eq_dec actual (expected_wire d)); [|discriminate].
  destruct (repeated d) eqn:Repeat; destruct (seen (slot d)) eqn:Seen;
    cbn in H; try discriminate;
    destruct (action_for par_id (shape d)) eqn:Selected; inversion H; subst;
    repeat split; auto.
Qed.

Definition permits (a : action) (p : piece) : Prop :=
  match a, p with
  | BorrowField, Borrowed _ => True
  | CutPar, Par _ _ => True
  | DescendMessage _ retain, Child _ _ actual => actual = retain
  | DescendMap _, Child _ _ actual => actual = false
  | AdmitSnapshot, EPathMap _ _ => True
  | _, _ => False
  end.

Theorem recursive_dispatch_cannot_be_replaced_by_borrowing : forall a body,
  a <> BorrowField -> ~ permits a (Borrowed body).
Proof. intros a body H; destruct a; cbn; tauto. Qed.

Theorem optional_and_oneof_children_share_presence : forall par_id child,
  action_for par_id (OptionalField child) = action_for par_id (MessageArm child).
Proof. reflexivity. Qed.

Theorem nonpar_child_presence_is_exact : forall par_id child,
  par_id <> child ->
  action_for par_id (OptionalField child) = Some (DescendMessage child true) /\
  action_for par_id (RepeatedField child) = Some (DescendMessage child false).
Proof.
  intros; unfold action_for, child_action; destruct (Nat.eqb par_id child) eqn:E.
  - apply Nat.eqb_eq in E; contradiction.
  - auto.
Qed.

Theorem dispatched_piece_preserves_source_and_action :
  forall cuts prefix groups a source fields input max_tag f rest par_id d seen selected p,
  invariant cuts prefix groups a source (Ready fields input) ->
  scan_field max_tag input = Some (f, rest) ->
  dispatch par_id d (field_wire f) seen = Accepted selected ->
  permits selected p ->
  field_bytes f = source_piece cuts prefix groups a p ->
  invariant cuts prefix groups a source (Ready (fields ++ [(p, field_bytes f)]) rest) /\
  permits selected p.
Proof.
  intros; split; [eapply ready_field_consumption_preserves_source; eauto | assumption].
Qed.

Inductive slot_request :=
| SubstituteSlot (context : nat)
| PreserveSlot
| DropSlot
| MissingSlot (context : nat)
| EmptySlot (planned_empty : nat).

Inductive child_job :=
| SubstituteJob (cut context : nat)
| PreserveJob (cut : nat)
| MissingJob (context : nat)
| EmptyJob (planned_empty : nat).

Inductive selects : list slot_request -> list nat -> list child_job -> list nat -> Prop :=
| SelectDone : forall cuts, selects [] cuts [] cuts
| SelectSubstitute : forall ctx requests cut cuts jobs rest,
    selects requests cuts jobs rest ->
    selects (SubstituteSlot ctx :: requests) (cut :: cuts) (SubstituteJob cut ctx :: jobs) rest
| SelectPreserve : forall requests cut cuts jobs rest,
    selects requests cuts jobs rest ->
    selects (PreserveSlot :: requests) (cut :: cuts) (PreserveJob cut :: jobs) rest
| SelectDrop : forall requests cut cuts jobs rest,
    selects requests cuts jobs rest ->
    selects (DropSlot :: requests) (cut :: cuts) jobs rest
| SelectMissing : forall ctx requests cuts jobs rest,
    selects requests cuts jobs rest ->
    selects (MissingSlot ctx :: requests) cuts (MissingJob ctx :: jobs) rest
| SelectEmpty : forall value requests cuts jobs rest,
    selects requests cuts jobs rest ->
    selects (EmptySlot value :: requests) cuts (EmptyJob value :: jobs) rest.

Fixpoint consumed_slots requests :=
  match requests with
  | [] => 0
  | (SubstituteSlot _ | PreserveSlot | DropSlot) :: tail => S (consumed_slots tail)
  | _ :: tail => consumed_slots tail
  end.

Fixpoint emitted_jobs requests :=
  match requests with
  | [] => 0
  | DropSlot :: tail => emitted_jobs tail
  | _ :: tail => S (emitted_jobs tail)
  end.

Theorem selection_consumes_exact_prefix : forall requests cuts jobs rest,
  selects requests cuts jobs rest ->
  rest = skipn (consumed_slots requests) cuts /\
  length cuts = consumed_slots requests + length rest.
Proof.
  intros requests cuts jobs rest H; induction H; cbn in *; intuition lia.
Qed.

Theorem selection_job_count_includes_synthetic_excludes_dropped : forall requests cuts jobs rest,
  selects requests cuts jobs rest -> length jobs = emitted_jobs requests.
Proof. intros requests cuts jobs rest H; induction H; cbn; congruence. Qed.

Theorem finished_selection_exhausts_cuts : forall requests cuts jobs,
  selects requests cuts jobs [] -> length cuts = consumed_slots requests.
Proof.
  intros; pose proof (proj2 (selection_consumes_exact_prefix H)); cbn in *; lia.
Qed.

Theorem reverse_push_executes_children_before_continuation : forall (A : Type) (work children : list A) kont,
  rev (work ++ [kont] ++ rev children) = children ++ kont :: rev work.
Proof.
  intros; rewrite !rev_app_distr, rev_involutive, <- app_assoc; reflexivity.
Qed.

Example selection_is_not_a_permutation_of_cuts :
  selects [DropSlot; EmptySlot 99; PreserveSlot] [10; 20]
    [EmptyJob 99; PreserveJob 20] [].
Proof. repeat constructor. Qed.

Print Assumptions unknown_field_borrows_without_singular_check.
Print Assumptions wrong_wire_precedes_slot_and_shape_checks.
Print Assumptions duplicate_slot_precedes_shape_check.
Print Assumptions accepted_dispatch_has_exact_generated_action.
Print Assumptions recursive_dispatch_cannot_be_replaced_by_borrowing.
Print Assumptions optional_and_oneof_children_share_presence.
Print Assumptions nonpar_child_presence_is_exact.
Print Assumptions dispatched_piece_preserves_source_and_action.
Print Assumptions selection_consumes_exact_prefix.
Print Assumptions selection_job_count_includes_synthetic_excludes_dropped.
Print Assumptions finished_selection_exhausts_cuts.
Print Assumptions reverse_push_executes_children_before_continuation.
Print Assumptions selection_is_not_a_permutation_of_cuts.

End DispatchAndSlots.


Example empty_delimited_field_is_present :
  scan_field 1 [10; 0] = Some (Field 10 Delimited [] [10; 0], []).
Proof. reflexivity. Qed.

Example truncated_delimited_field_is_refused :
  scan_field 1 [10; 2; 42] = None.
Proof. reflexivity. Qed.

Example unknown_wire_kind_is_refused :
  scan_field 1 [11; 0] = None.
Proof. reflexivity. Qed.


Print Assumptions taken_body_splits_source.
Print Assumptions successful_body_scan_reconstructs_source.
Print Assumptions successful_field_scan_reconstructs_source.
Print Assumptions successful_delimited_field_has_exact_header.
Print Assumptions initial_frame_has_source_invariant.
Print Assumptions borrowed_field_establishes_certificate.
Print Assumptions par_field_establishes_certificate.
Print Assumptions ready_field_consumption_preserves_source.
Print Assumptions child_descent_preserves_source.
Print Assumptions published_child_closes_suspended_field.
Print Assumptions completed_frame_supplies_publication_certificate.
Print Assumptions completed_frame_reconstructs_original.

Print Assumptions original_value_frames_are_encoded_frames.
Print Assumptions epathmap_field_establishes_certificate.
Print Assumptions prior_piece_source_survives_publication.
Print Assumptions prior_certificate_survives_publication.
Print Assumptions suspended_parent_survives_child_publication.
Print Assumptions completed_frame_publishes_valid_arena.
Print Assumptions empty_delimited_field_is_present.
Print Assumptions truncated_delimited_field_is_refused.
Print Assumptions unknown_wire_kind_is_refused.

End SubstitutionRegionDecomposition.
